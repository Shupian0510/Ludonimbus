//! The queue: versions of this device that wait for the cloud
//! (`pending/<game>/`), each complete on this device, and how a new version
//! gets there ([`stage_local`] for the saves here, [`queue_files`] for files
//! handed to this device). A device counts as synced to a version from the
//! moment it is in the queue, so the line it decides by is the cloud's with
//! the queue on top ([`with_queue`]). Uploading what waits is the next part,
//! `publish`.

use super::{base_vv, mark_synced, platform, remember_base, Ctx, COMMENT};
use crate::cloud::decide;
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Draft, Place};
use crate::cloud::v2::{self, FileRef, IndexEntry, Manifest, Origin};
use crate::cloud::{self, library, line, packs, OpenLine};
use crate::config::{self, GameEntry};
use crate::paths;
use crate::snapshot::{self, LocalState, SnapshotMeta, Trigger};
use crate::state::{self, GameState};
use crate::vv::{self, Vv};
use anyhow::Result;
use chrono::Utc;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(super) fn queue_dir(key: &str) -> PathBuf {
    paths::pending_dir().join(key)
}

/// A version 0.6 left in the queue, as a manifest of layout 2. Its zip is
/// the snapshot's own pack as it is; the vector is given when it is
/// uploaded ([`settle_queued`]).
fn from_06(meta: SnapshotMeta) -> Manifest {
    let mut extra = v2::Extra::new();
    if !meta.roots.is_empty() {
        extra.insert(snapshot::ROOTS.to_string(), serde_json::json!(meta.roots));
    }
    if !meta.comment.trim().is_empty() {
        extra.insert(COMMENT.to_string(), meta.comment.clone().into());
    }
    Manifest {
        v: v2::LAYOUT,
        id: meta.id,
        game: meta.game,
        line: library::MAIN_LINE.to_string(),
        vv: Vv::new(),
        parent: meta.parent,
        device_id: meta.device_id,
        device_name: meta.device_name,
        platform: String::new(),
        created_at: meta.created_at,
        trigger: meta.trigger.as_str().to_string(),
        side: !meta.trigger.is_mainline(),
        content_hash: meta.content_hash,
        total_size: meta.total_size,
        files: meta
            .files
            .into_iter()
            .map(|f| FileRef { path: f.path, size: f.size, hash: f.hash, mtime: f.mtime, pack: None, extra: v2::Extra::new() })
            .collect(),
        game_version: meta.game_version,
        installed_version: meta.installed_version,
        info: None,
        origin: None,
        requires: Vec::new(),
        app_version: meta.app_version,
        extra,
    }
}

/// The versions of `key` waiting on this device for the cloud, oldest
/// first. The flag says the file in the queue is one 0.6 wrote.
pub(super) fn queued(key: &str) -> Vec<(Manifest, bool)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(queue_dir(key)) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        if value.get("v").and_then(|v| v.as_u64()) == Some(u64::from(v2::LAYOUT)) {
            if let Ok(m) = serde_json::from_value::<Manifest>(value) {
                out.push((m, false));
            }
        } else if let Ok(meta) = serde_json::from_value::<SnapshotMeta>(value) {
            out.push((from_06(meta), true));
        }
    }
    out.sort_by(|a, b| a.0.created_at.cmp(&b.0.created_at).then_with(|| a.0.id.cmp(&b.0.id)));
    out
}

/// Versions of `key` waiting on this device for the cloud, oldest first.
pub fn pending_metas(key: &str) -> Vec<SnapshotMeta> {
    queued(key).iter().map(|(m, _)| SnapshotMeta::of(m, key, None)).collect()
}

/// The zips that hold the files of a version waiting in the queue (each
/// file as `files/<path>` in one of them). Empty when the version isn't
/// waiting.
pub fn pending_zips(key: &str, id: &str) -> Vec<PathBuf> {
    let Some((m, _)) = queued(key).into_iter().find(|(m, _)| m.id == id) else { return Vec::new() };
    let dir = queue_dir(key);
    let zips: Vec<PathBuf> = m.packs().iter().map(|p| dir.join(format!("{p}.zip"))).collect();
    if zips.iter().all(|z| z.is_file()) {
        zips
    } else {
        Vec::new()
    }
}

/// The snapshots the versions of `key` waiting on this device take files
/// from (the ones they were built on): the device record names them, so
/// that the cloud keeps them for this device until those versions are
/// uploaded ([`cloud::DeviceInstall::borrows`]).
pub fn queued_bases(key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (m, _) in queued(key) {
        if let Some(parent) = m.parent.as_ref().filter(|_| packs::borrows(&m)) {
            if !out.contains(parent) {
                out.push(parent.clone());
            }
        }
    }
    out
}

/// The zips on this device that may hold the content of a pack that is gone
/// from the cloud: what waits in the queue, and the copies in local-backups.
pub(super) fn repair_zips(key: &str) -> Vec<PathBuf> {
    let zips = |dir: PathBuf| -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> =
            std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
        // The newest first: the likeliest to hold what a recent version had.
        out.sort();
        out.reverse();
        out
    };
    let mut out = zips(queue_dir(key));
    out.extend(zips(paths::local_backups_dir().join(key)));
    out
}

/// The games with versions waiting for the cloud.
pub fn pending_keys() -> Vec<String> {
    std::fs::read_dir(paths::pending_dir())
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// Add snapshots to a view of their line, as reading the line again would
/// show them.
pub(super) fn add_to_view(view: &mut LineView, entries: impl IntoIterator<Item = IndexEntry>) {
    let mut all: Vec<(IndexEntry, usize)> = view.snapshots.drain(..).zip(view.places.drain(..)).collect();
    for e in entries {
        if !all.iter().any(|(s, _)| s.id == e.id) {
            all.push((e, 0));
        }
    }
    all.sort_by(|a, b| a.0.created_at.cmp(&b.0.created_at).then_with(|| a.0.id.cmp(&b.0.id)));
    let (snapshots, places): (Vec<IndexEntry>, Vec<usize>) = all.into_iter().unzip();
    let heads = line::heads_of(&snapshots);
    view.snapshots = snapshots;
    view.places = places;
    view.heads = heads;
}

/// Make a waiting version fit the line it is about to join. Nobody has seen
/// it yet, so it may still change. Returns whether it did.
///
/// - It is labelled with the game and line as they are now (the cloud may
///   have been upgraded since it was made).
/// - A version without a vector (0.6 made it, or the line couldn't be read
///   when it was made) gets one: built on its parent.
/// - A version whose vector lacks its parent's changes gets them. That is a
///   version made offline by a device that came from 0.6 and didn't know
///   the vector of the 0.6 snapshot it was at.
pub(super) fn settle_queued(m: &mut Manifest, place: &Place, view: &LineView) -> bool {
    let before = m.clone();
    m.game = place.game.clone();
    m.line = place.line.clone();
    let parent = m.parent.as_deref().and_then(|p| view.get(p)).map(|s| s.vv.clone()).filter(|vv| !vv.is_empty());
    if m.is_side() {
        if m.vv.is_empty() {
            m.vv = parent.unwrap_or_default();
        }
    } else if m.vv.is_empty() {
        let floor = vv::max_count(view.snapshots.iter().map(|s| &s.vv), &m.device_id);
        m.vv = Vv::next(parent.iter(), &m.device_id, floor);
    } else if let Some(parent) = parent {
        if !m.vv.contains(&parent) {
            m.vv.join(&parent);
        }
    }
    *m != before
}

/// The line as it will be once what waits in the queue is uploaded. This
/// device is at one of those versions, so this is the line it decides by.
pub(super) fn with_queue(view: &LineView, place: &Place, key: &str) -> LineView {
    let mut seen = view.clone();
    for (mut m, _) in queued(key) {
        if seen.get(&m.id).is_none() {
            settle_queued(&mut m, place, &seen);
            add_to_view(&mut seen, [m.summary()]);
        }
    }
    seen
}

/// The version "the cloud's" stands for when the user chooses between it
/// and the saves here: the newest head that isn't those saves.
pub(super) fn other_side(view: &LineView, local_hash: &str) -> Option<String> {
    let heads: Vec<&IndexEntry> = view.head_entries().collect();
    heads.iter().rev().find(|h| h.content_hash != local_hash).or(heads.last()).map(|h| h.id.clone())
}

pub(super) fn unqueue(key: &str, m: &Manifest) -> Result<()> {
    saves::unstage(&queue_dir(key), m)?;
    state::update_game(key, |g| g.pending.retain(|x| x != &m.id))
}

/// What a new snapshot is, beyond its files.
#[derive(Default)]
pub(super) struct Shot {
    /// The vectors a mainline snapshot is built on ([`decide::next_vv`]).
    pub(super) bases: Vec<Vv>,
    pub(super) parent: Option<String>,
    pub(super) trigger: Option<Trigger>,
    /// Beside the history: never the newest version.
    pub(super) side: bool,
    pub(super) comment: String,
    pub(super) origin: Option<Origin>,
    /// The manifest to share unchanged files with (large saves).
    pub(super) base: Option<Manifest>,
    /// How long the game had been played when the saves were what they are.
    pub(super) played: Played,
}

/// The playtime a new snapshot records ([`v2::PLAYTIME`]).
#[derive(Default)]
pub(super) enum Played {
    /// The saves here as they are now: the game's playtime now.
    #[default]
    Now,
    /// A copy of an older version: what that one recorded (`None`: it
    /// didn't say, and neither does the copy).
    Was(Option<u64>),
}

/// The game's playtime now, as this device knows it (its own files and its
/// copies of the others'): no request. `None` before any was recorded.
fn playtime_now(ctx: &Ctx, key: &str) -> Option<u64> {
    Some(crate::playtime::summary(&ctx.cfg, key).total_secs).filter(|s| *s > 0)
}

/// Put a snapshot of `files` into the queue of `key`: complete on this
/// device, not yet in the cloud. `carried`: files it takes over from an
/// earlier version without having them ([`saves::draft_with`]).
#[allow(clippy::too_many_arguments)]
fn enqueue(
    key: &str,
    place: &Place,
    draft: &Draft,
    files: &[packs::LocalFile],
    carried: &[FileRef],
    roots: &BTreeMap<String, String>,
    comment: &str,
    base: Option<&Manifest>,
) -> Result<Manifest> {
    let mut manifest = saves::draft_with(place, draft, files, carried, base, &packs::Policy::default());
    if !roots.is_empty() {
        manifest.extra.insert(snapshot::ROOTS.to_string(), serde_json::json!(roots));
    }
    if !comment.trim().is_empty() {
        manifest.extra.insert(COMMENT.to_string(), comment.into());
    }
    saves::stage(&manifest, files, &queue_dir(key))?;
    state::update_game(key, |g| {
        if !g.pending.contains(&manifest.id) {
            g.pending.push(manifest.id.clone());
        }
    })?;
    Ok(manifest)
}

/// A game that follows a line of this device's own because that is the
/// device's default ([`cloud::wanted_line`]) gets that written into its
/// entry before its first version is made: from then on the choice is the
/// game's, and the default no longer decides.
fn note_line(ctx: &Ctx, game: &GameEntry, st: &GameState) -> Result<()> {
    if game.line.is_some() {
        return Ok(());
    }
    let line = cloud::wanted_line(&ctx.cfg, game, Some(st));
    if line == library::MAIN_LINE {
        return Ok(());
    }
    config::update(|c| {
        if let Some(g) = c.games.iter_mut().find(|g| g.key == game.key) {
            if g.line.is_none() {
                g.line = Some(line.clone());
            }
        }
        Ok(())
    })
}

/// Queue a snapshot of this device's saves.
pub(super) fn stage_local(ctx: &Ctx, game: &GameEntry, place: &Place, view: &LineView, local: &LocalState, shot: &Shot) -> Result<Manifest> {
    let st = state::game(&game.key);
    note_line(ctx, game, &st)?;
    let device = ctx.device();
    let view = &with_queue(view, place, &game.key);
    let vv = if shot.side { base_vv(&st, view) } else { decide::next_vv(&shot.bases, view, device.0, st.counter) };
    let draft = Draft {
        vv,
        parent: shot.parent.clone(),
        side: shot.side,
        trigger: shot.trigger.unwrap_or(Trigger::SyncNow).as_str().to_string(),
        platform: platform(game),
        device_id: device.0.to_string(),
        device_name: device.1.to_string(),
        created_at: Utc::now(),
        game_version: crate::crosssaves::local_version(game, &local.files, &local.roots),
        installed_version: crate::gamebackup::version::installed(game),
        info: None,
        origin: shot.origin.clone(),
        playtime: match shot.played {
            Played::Now => playtime_now(ctx, &game.key),
            Played::Was(secs) => secs,
        },
    };
    let manifest = enqueue(&game.key, place, &draft, &local.pack_files(), &local.carried, &local.used_roots(), &shot.comment, shot.base.as_ref())?;
    if !shot.side {
        mark_synced(&game.key, &manifest.id, &manifest.vv, &manifest.content_hash, device.1, device.0)?;
        remember_base(&game.key, Some(&manifest));
    }
    Ok(manifest)
}

/// Files that are not this device's saves (a Switch's, handed over the
/// local network; a save converted for a version that is only in the
/// cloud), to be made a version of the game `key`.
pub struct Foreign<'a> {
    pub key: &'a str,
    /// Canonical path and content of each file.
    pub files: &'a [(String, Vec<u8>)],
    pub trigger: Trigger,
    /// The version the files were made from.
    pub parent: Option<String>,
    /// On top of every newest version, not only of `parent`: the user chose
    /// these files over whatever the cloud has.
    pub over_all: bool,
    /// Id and name of the device the files are from.
    pub device: (&'a str, &'a str),
    pub platform: &'a str,
    pub comment: &'a str,
    pub game_version: Option<String>,
    pub origin: Option<Origin>,
}

/// Make a version from files handed to this device and put it in the queue
/// ([`upload_pending`](super::upload_pending) uploads it). `line` is the game's line as just read.
/// A version of a kind that never is the newest (`pre-restore`) goes beside
/// the history.
pub fn queue_files(line: &OpenLine, f: &Foreign) -> Result<SnapshotMeta> {
    queue_files_carrying(line, f, &[])
}

/// [`queue_files`] for files of a version that shares another platform's
/// line through a rule: `carried` are that platform's files the version
/// takes over ([`carried_for`](super::carried_for)).
pub fn queue_files_carrying(line: &OpenLine, f: &Foreign, carried: &[FileRef]) -> Result<SnapshotMeta> {
    let work = paths::staging_dir().join(format!("files-{}", ulid::Ulid::new()));
    let result = (|| -> Result<Manifest> {
        let mut files = Vec::with_capacity(f.files.len());
        for (path, data) in f.files {
            let dest = snapshot::resolve_in(&work, path)?;
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&dest, data)?;
            files.push(packs::LocalFile::from_disk(path.clone(), dest)?);
        }
        let view = with_queue(&line.view, &line.place, f.key);
        let side = !f.trigger.is_mainline();
        let parent = f.parent.as_deref().and_then(|p| view.get(p)).map(|s| s.vv.clone()).filter(|vv| !vv.is_empty());
        let vv = if side {
            parent.unwrap_or_default()
        } else {
            let bases: Vec<Vv> = match f.over_all {
                true => view.head_entries().map(|h| h.vv.clone()).collect(),
                false => parent.into_iter().collect(),
            };
            decide::next_vv(&bases, &view, f.device.0, 0)
        };
        let draft = Draft {
            vv,
            parent: f.parent.clone(),
            side,
            trigger: f.trigger.as_str().to_string(),
            platform: f.platform.to_string(),
            device_id: f.device.0.to_string(),
            device_name: f.device.1.to_string(),
            created_at: Utc::now(),
            game_version: f.game_version.clone(),
            installed_version: None,
            info: None,
            origin: f.origin.clone(),
            // Files from elsewhere: no playtime of this device says how far
            // they are.
            playtime: None,
        };
        enqueue(f.key, &line.place, &draft, &files, carried, &BTreeMap::new(), f.comment, None)
    })();
    let _ = std::fs::remove_dir_all(&work);
    Ok(SnapshotMeta::of(&result?, f.key, None))
}
