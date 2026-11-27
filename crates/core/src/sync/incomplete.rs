//! Versions that miss a pack (docs/plan-v2.md, section 3.4), and the copies
//! of versions this device keeps. A version found missing a pack when it is
//! fetched is marked in the cloud, so that no device tries it again
//! ([`note_missing`]); after playing, this device puts back what the marked
//! versions it wrote, or whose saves it has, lack ([`repair_marked`]). A
//! version of this device that can never be uploaded complete is set aside
//! ([`set_aside`]): it stays in local-backups ([`keep_local_copy`]) and is
//! listed until the user dismisses it ([`unsent`]).

use super::publish::other_lines_of;
use super::queue::{add_to_view, queue_dir, repair_zips, unqueue};
use super::{base_file, log, remember_base};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Place};
use crate::cloud::v2::{self, FileRef, IndexEntry, Manifest};
use crate::cloud::{self, packs, Cloud, OpenLine};
use crate::snapshot::{self, LocalState, SnapshotMeta};
use crate::{paths, state};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use std::path::Path;

/// Keep a copy of a queued snapshot on this PC (safety net for the saves a
/// pull is about to replace). The newest 10 of a game stay.
pub(super) fn keep_local_copy(key: &str, manifest: &Manifest) -> Result<()> {
    let dir = paths::local_backups_dir().join(key);
    std::fs::create_dir_all(&dir)?;
    let queue = queue_dir(key);
    for pack in manifest.packs() {
        let zip = queue.join(format!("{pack}.zip"));
        if zip.is_file() {
            std::fs::copy(&zip, dir.join(format!("{pack}.zip")))?;
        }
    }
    std::fs::write(dir.join(format!("{}.json", manifest.id)), serde_json::to_vec_pretty(manifest)?)?;
    // What couldn't be uploaded complete stays until the user dismisses it.
    let unsent: Vec<String> = state::game(key).unsent.into_iter().map(|u| format!("{}.json", u.id)).collect();
    let mut kept: Vec<_> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| !p.file_name().is_some_and(|n| unsent.iter().any(|u| n == u.as_str())))
        .collect();
    kept.sort();
    while kept.len() > 10 {
        let json = kept.remove(0);
        let id = json.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&id) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(())
}

pub(super) fn incomplete_error() -> anyhow::Error {
    anyhow!(crate::tr!(
        "这个版本不完整：云端缺少它的内容包，暂时不能使用",
        "This version is incomplete: a content pack of it is missing from the cloud, so it can't be used for now"
    ))
}

/// A version of the line turned out to miss `pack` when it was fetched:
/// mark it in the cloud ([`saves::mark`]), so no device tries to take it
/// again, and say so in `cache` and `view`. Nothing is written to a cloud
/// this build may not write; the next fetch finds it again then.
pub(super) fn note_missing(cloud: &Cloud, place: &Place, cache: &mut cloud::line::LineCache, view: Option<&mut LineView>, id: &str, pack: &str) {
    if !cloud.access().is_ok_and(|a| a == v2::Access::ReadWrite) {
        return;
    }
    match saves::mark(cloud.files(), place, cache, id, &[pack.to_string()], cloud.device_id(), Utc::now()) {
        Ok(()) => {
            let mark = cache.snapshots.get(id).and_then(|e| e.incomplete.clone());
            if let Some(entry) = view.and_then(|v| v.snapshots.iter_mut().find(|s| s.id == id)) {
                entry.incomplete = mark.or_else(|| Some(v2::MarkRef { missing: vec![pack.to_string()], ..Default::default() }));
            }
            log(format!("{id} misses the content pack {pack}; marked, no device takes it until the pack is back"));
        }
        Err(e) => log(format!("{id} misses the content pack {pack}; marking it failed: {e:#}")),
    }
}

/// Put back what the versions of the line marked as missing a pack lack,
/// when this device is likely to have it: it wrote them, or its saves are
/// exactly one of them (section 3.4). The content is looked for in the saves
/// here, the zips this device keeps and the game's other packs
/// ([`saves::put_back`]); a version made whole loses its mark. One this
/// device wrote and can't make whole is let go ([`saves::retire`]): its
/// writer had its chance. No request at all while nothing in the line is
/// marked. After playing only.
pub(super) fn repair_marked(cloud: &Cloud, line: &mut OpenLine, local: &LocalState) -> Result<()> {
    let me = cloud.device_id().to_string();
    let targets: Vec<IndexEntry> = line
        .view
        .snapshots
        .iter()
        .filter(|s| s.is_incomplete() && (s.device_id == me || (!local.is_empty() && s.content_hash == local.hash)))
        .cloned()
        .collect();
    if targets.is_empty() || cloud.access()? != v2::Access::ReadWrite {
        return Ok(());
    }
    let others = other_lines_of(cloud, &line.place)?;
    let files = local.pack_files();
    let zips = repair_zips(&line.key);
    let mut changed = false;
    for t in targets {
        let outcome = {
            let Ok(manifest) = saves::manifest(cloud.files(), &line.place, &line.view, &t.id) else { continue };
            let mut lines: Vec<(&Place, &LineView)> = vec![(&line.place, &line.view)];
            lines.extend(others.iter().map(|(p, v)| (p, v)));
            let sources = saves::Sources { local: &files, zips: &zips };
            saves::put_back(cloud.files(), &lines, &line.place, &manifest, None, sources, &paths::staging_dir(), &me, Utc::now())?
        };
        match outcome {
            saves::Repair::Complete | saves::Repair::Repaired(_) => {
                saves::unmark(cloud.files(), &line.place, &mut line.cache, &t.id)?;
                log(format!("{}: {} is whole again", line.key, t.id));
                changed = true;
            }
            saves::Repair::Lost(missing) if t.device_id == me && saves::can_retire(&line.place, &line.view, &t.id) => {
                saves::retire(cloud.files(), &line.place, &line.view, &t.id, cloud.device_name(), Utc::now())?;
                line.cache.snapshots.remove(&t.id);
                log(format!("{}: {} misses {missing:?}, found nowhere; moved to Recently deleted", line.key, t.id));
                changed = true;
            }
            saves::Repair::Lost(_) => {}
        }
    }
    if changed {
        cloud.reopen(line)?;
    }
    Ok(())
}

/// A waiting version that can never be complete: a pack it borrows was
/// cleaned up while it waited, and its content is nowhere any more, not in
/// the saves here (they changed since), the zips on this device or another
/// pack in the cloud. Its manifest is not in the cloud ([`saves::publish`]
/// took it away again). It leaves the queue for the copies kept on this
/// device ([`keep_local_copy`]), with what it brought itself, so that it no
/// longer holds up the versions after it or every sync of the game, and the
/// game's saves tab lists it once ([`unsent`]). This device's base is then a
/// version the line doesn't have: the next decision builds on it when the
/// line has nothing newer, and asks when it has.
pub(super) fn set_aside(line: &mut OpenLine, m: &Manifest, e: &anyhow::Error) -> Result<()> {
    let key = line.key.clone();
    let note = state::Unsent {
        id: m.id.clone(),
        created_at: m.created_at,
        total_size: m.total_size,
        file_count: m.files.len(),
        platform: m.platform.clone(),
        at: Utc::now(),
    };
    // Noted first: the copies kept on this device let go of the oldest, and
    // this one may be old.
    state::update_game(&key, |g| {
        g.unsent.retain(|u| u.id != note.id);
        g.unsent.push(note);
    })?;
    keep_local_copy(&key, m)?;
    unqueue(&key, m)?;
    // Nothing more builds on it.
    if std::fs::read(base_file(&key)).ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok()).is_some_and(|b| b.id == m.id) {
        remember_base(&key, None);
    }
    if let Some(i) = line.view.snapshots.iter().position(|s| s.id == m.id) {
        line.view.snapshots.remove(i);
        line.view.places.remove(i);
        add_to_view(&mut line.view, std::iter::empty());
    }
    line.cache.snapshots.remove(&m.id);
    log(format!("{key}: {} can't be uploaded any more ({e:#}); it is kept on this device in local-backups", m.id));
    Ok(())
}

/// The versions of `key` this device couldn't upload complete and set aside
/// ([`set_aside`]), oldest first, while their copy is in local-backups and
/// the user hasn't dismissed them.
pub fn unsent(key: &str) -> Vec<state::Unsent> {
    let dir = paths::local_backups_dir().join(key);
    let mut out: Vec<state::Unsent> = state::game(key).unsent.into_iter().filter(|u| dir.join(format!("{}.json", u.id)).is_file()).collect();
    out.sort_by_key(|u| u.created_at);
    out
}

/// Stop listing a version [`unsent`] lists. Its copy stays in local-backups
/// with the others, until newer ones take its place.
pub fn dismiss_unsent(key: &str, id: &str) -> Result<()> {
    state::update_game(key, |g| g.unsent.retain(|u| u.id != id))
}

/// Save what a version [`unsent`] lists brought itself as a zip at `dest`:
/// `files/<path>` and a `snapshot.json` that lists those files only. The
/// files it took from the pack that was gone are not there; it is not a
/// version to restore. Returns how many files it holds.
pub fn export_unsent(key: &str, id: &str, dest: &Path) -> Result<usize> {
    if !v2::is_snapshot_id(id) {
        bail!(crate::tr!("无效的版本编号：{id}", "Invalid version id: {id}"));
    }
    let dir = paths::local_backups_dir().join(key);
    let bytes =
        std::fs::read(dir.join(format!("{id}.json"))).map_err(|_| anyhow!(crate::tr!("本机没有这个版本了", "This version is no longer on this device")))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let work = saves::Workspace::new(&paths::staging_dir())?;
    let mut have: Vec<FileRef> = Vec::new();
    for pack in manifest.packs().into_iter().filter(|p| packs::is_own(p, &manifest.id)) {
        let zip = dir.join(format!("{pack}.zip"));
        if !zip.is_file() {
            continue;
        }
        let held = packs::zip_contents(&zip);
        let refs: Vec<&FileRef> = held.iter().collect();
        packs::unzip(&zip, &refs, work.path())?;
        have.extend(held);
    }
    let mut meta = SnapshotMeta::of(&manifest, key, None);
    meta.files.retain(|f| have.iter().any(|h| h.path == f.path));
    if meta.files.is_empty() {
        bail!(crate::tr!("这个版本在本机没有留下文件", "None of this version's files are left on this device"));
    }
    snapshot::zip_dir(work.path(), &meta, dest)?;
    Ok(meta.files.len())
}
