//! One save line in the cloud, as the sync uses it: read it, add a snapshot,
//! fetch one, keep one, delete one, clean up (docs/plan-v2.md, sections 3.4,
//! 3.7, 5.4 and 5.6).
//!
//! Nothing here decides anything. What to do is [`super::decide`]; what to
//! keep is [`super::retention`]; where the bytes are is [`super::packs`];
//! what exists is [`super::line`]. This module puts them in the one order
//! that is safe when the connection breaks at any step or another device
//! works on the same line, and it is where the number of requests of each
//! operation is fixed.
//!
//! It knows no devices' folders, no config and no state: saves come in as
//! [`LocalFile`]s and go out into a folder the caller names. Every function
//! takes the store, so the caller chooses what is cached
//! ([`super::cached::Cached`]).
//!
//! # A line in several folders
//!
//! A [`Place`] is a line with the folders it lives in. New manifests, pins
//! and the index go to the first ([`Place::dir`]); the others are only read:
//! the folder of 0.6 from before the migration (`games/<key>/snapshots`,
//! [`v2::legacy_key`]), a line that became part of this one.
//!
//! In a folder of 0.6 this module follows what 0.6 itself does there, so a
//! device still on 0.6 is never surprised:
//!
//! - nothing is ever written into it, except by [`untrash`], which puts a
//!   manifest back where and as 0.6 would: byte for byte what the record
//!   holds;
//! - [`clean`] never deletes a snapshot from it. 0.6 takes the newest
//!   snapshot of its folder for the current progress and pulls it; removing
//!   that one would make every device still on 0.6 go back to the one
//!   before;
//! - [`trash`], [`untrash`] and [`purge`] use `games/<key>/trash/`, the
//!   record format of 0.6 (`{deletedAt, deletedBy, meta}`) and its order of
//!   steps, and the snapshot's zip stays beside where its manifest was.
//!
//! # Requests
//!
//! | operation | requests |
//! |---|---|
//! | [`open`], nothing new | 1 list per folder of the line |
//! | [`open`], new snapshots | the same + 1 read (the index) |
//! | [`publish`], all files in the snapshot's own packs | 1 upload per pack + 2 writes (manifest, index) |
//! | [`publish`], some files taken from older packs | the same + 1 list (+ what putting back a pack found gone costs, see [`put_back`]) |
//! | [`manifest`] | 1 read |
//! | [`fetch`] | 1 download per pack that holds a wanted file |
//! | [`keep`] | 1 write; renaming a kept save 1 read + 1 write |
//! | [`unkeep`] | 1 remove per folder of the line |
//! | [`trash`] | 1 read + 1 write + 1 remove |
//! | [`trashed`] | 1 list per folder of the line (its "recently deleted"); for a folder with versions in it, 1 list more (the folder itself) + 1 read per version |
//! | [`untrash`] | 1 read + 1 write + 1 remove (+ 1 list in a folder of 0.6) |
//! | [`purge`] | 1 read + 1 remove (+ 1 remove in a folder of 0.6: the zip) |
//! | [`clean`] | see there |
//! | [`mark`], [`unmark`] | 1 write; 1 remove |
//! | [`put_back`] | 1 list per game folder (none when the caller has listed them), 1 read per manifest that may share a pack or hold its content, 1 download per pack content is taken from or a mark is checked against, 1 upload per pack put back |
//! | [`repair_queued`] | 1 list per game folder; when a pack is gone, as [`put_back`] without uploading |
//! | [`retire`] | 1 read + 1 write + 2 removes (manifest, mark) |
//!
//! In a line of several folders, [`keep`] (renaming), [`untrash`] and
//! [`purge`] are told only the snapshot's id, so they look for the pin or
//! the record in the line's folders in order: 1 read more per folder looked
//! in before the one that has it ([`purge`]: 1 read more per folder looked
//! in, the one that has it included). A line in one folder costs what the
//! table says.
//!
//! With the one listing of [`open`], a sync after playing is 4 requests for
//! a small save (list, pack, manifest, index) and a check before playing
//! with nothing new is 1. These numbers are asserted by tests against
//! [`super::memory::Memory`]; a change that makes one of them larger has to
//! change the table and say why.
//!
//! # What the caller must do
//!
//! - [`open`] the line (again) before [`publish`] in the same sync, with the
//!   same cache: the index is written from the cache and must not lose what
//!   other devices added.
//! - Keep the [`LineCache`] of each line between runs (it is what makes
//!   "nothing new" one request).
//! - A snapshot made while the cloud can't be reached is made without a
//!   base ([`draft`] with `base: None`), so it borrows from no pack that may
//!   be gone by the time it is uploaded. One made with a base whose upload
//!   then had to wait may find a borrowed pack gone: pass [`publish`] the
//!   save files as they are then, so it can put the pack back.

use super::line::{self, LineCache, LineView};
use super::packs::{self, LocalFile};
use super::retention::{self, DeviceBase};
use super::store::{join, Entry, Store};
use super::v2::{self, FileRef, GameRecord, IndexEntry, Info, Line, Manifest, Origin, Pin};
use crate::vv::Vv;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn not_found() -> anyhow::Error {
    anyhow!(crate::tr!("找不到这个版本", "This version wasn't found"))
}

/// The folder new files of the line go to. Never one of 0.6: nothing is
/// written there.
fn writable(place: &Place) -> Result<&str> {
    let dir = place.dir();
    if v2::legacy_key(dir).is_some() {
        bail!(crate::tr!("这条存档线的文件夹是 0.6 的，不能写入", "This save line's folder is one of Ludonimbus 0.6 and can't be written to"));
    }
    Ok(dir)
}

/// An id that comes from outside becomes part of a path: it must be one.
fn check_id(id: &str) -> Result<()> {
    if !v2::is_snapshot_id(id) {
        bail!(crate::tr!("无效的版本编号：{id}", "Invalid version id: {id}"));
    }
    Ok(())
}

/// Beside a staged pack once [`publish`] got it into the cloud: an upload
/// of several packs that broke off goes on with the ones still missing.
fn sent_mark(dir: &Path, pack: &str) -> PathBuf {
    dir.join(format!("{pack}.sent"))
}

/// Beside a staged snapshot once [`publish`] has written its manifest into
/// the cloud, even if it took it away again: another device may have read
/// it in between, and kept what it said. From then on the manifest stays as
/// it is ([`repair_queued`] no longer changes where its files are).
fn shown_mark(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.shown"))
}

/// Whether [`publish`] ever wrote the manifest of the staged snapshot `id`
/// into the cloud.
pub fn was_shown(dir: &Path, id: &str) -> bool {
    shown_mark(dir, id).is_file()
}

/// The packs `manifest` brings itself (`<id>`, `<id>-2`, ...), in the order
/// they were planned: what [`stage`] builds and [`publish`] uploads.
fn own_packs(manifest: &Manifest) -> Vec<String> {
    let id = manifest.id.as_str();
    let numbered = format!("{id}-");
    let mut own: Vec<String> = manifest
        .packs()
        .into_iter()
        .filter(|p| p == id || p.strip_prefix(&numbered).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        .collect();
    // `<id>-10` after `<id>-9`.
    own.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    own
}

fn remove_local(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| crate::tr!("无法删除 {}", "Can't delete {}", path.display())),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------- place

/// A save line and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The game record's id; the `game` of new manifests.
    pub game: String,
    /// The line's id; the `line` of new manifests.
    pub line: String,
    /// The game's folders under `games/`. New packs go to the first, packs
    /// are looked for in all of them.
    pub folders: Vec<String>,
    /// The line's folders: [`Line::path`], then [`Line::also`]. Never empty.
    pub dirs: Vec<String>,
}

impl Place {
    pub fn of(record: &GameRecord, line: &Line) -> Place {
        let mut dirs = vec![line.path.clone()];
        dirs.extend(line.also.iter().cloned());
        Place { game: record.id.clone(), line: line.id.clone(), folders: record.folders.clone(), dirs }
    }

    /// The line of a game of 0.6 in a cloud that has not been migrated yet:
    /// exactly what the migration will make of the key ([`super::upgrade`]),
    /// so reading works before it has run. Game and folder are `key`, the
    /// line is `main` in `games/<key>/saves/main`, and `games/<key>/snapshots`
    /// is read as well.
    pub fn of_key(key: &str) -> Place {
        Place { game: key.to_string(), line: "main".to_string(), folders: vec![key.to_string()], dirs: vec![v2::line_dir(key, "main"), v2::legacy_dir(key)] }
    }

    /// Where the line is written to.
    pub fn dir(&self) -> &str {
        &self.dirs[0]
    }

    /// The folder that holds the manifest of `id` in `view` (a view of this
    /// place).
    pub fn dir_of(&self, view: &LineView, id: &str) -> Option<&str> {
        view.place(id).and_then(|i| self.dirs.get(i)).map(String::as_str)
    }
}

/// Read the line: [`super::line::load_all`] over the place's folders.
pub fn open(store: &dyn Store, place: &Place, cache: &mut LineCache) -> Result<LineView> {
    let dirs: Vec<&str> = place.dirs.iter().map(String::as_str).collect();
    line::load_all(store, &dirs, cache)
}

// -------------------------------------------------------------- new snapshots

/// What the caller knows about a snapshot it wants to make.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    /// From [`super::decide::next_vv`]. For a snapshot on the side: the
    /// vector of the base the saves were made from, unchanged (it is never
    /// compared, and says what the saves contained).
    pub vv: Vv,
    pub parent: Option<String>,
    pub side: bool,
    pub trigger: String,
    pub platform: String,
    pub device_id: String,
    pub device_name: String,
    /// The caller's clock. The snapshot's id is a ULID of this moment, so
    /// ids sort the way the snapshots were made.
    pub created_at: DateTime<Utc>,
    pub game_version: Option<String>,
    pub installed_version: Option<String>,
    pub info: Option<Info>,
    pub origin: Option<Origin>,
}

/// The content hash of a set of save files: the one 0.6 computes
/// (`snapshot::content_hash`: blake3 over the sorted lines
/// `<path in lower case>\0<file hash>`, and `"empty"` for no files), so the
/// same saves compare equal across layouts. Use that function rather than a
/// second copy of it.
pub fn content_hash(files: &[LocalFile]) -> String {
    let entries: Vec<crate::snapshot::FileEntry> =
        files.iter().map(|f| crate::snapshot::FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }).collect();
    crate::snapshot::content_hash(&entries)
}

/// The manifest of a new snapshot of `files` (pure; nothing is in the cloud
/// yet). `v` is [`v2::LAYOUT`], `game` and `line` come from the place, the
/// id is new, `files` and their packs come from
/// [`packs::plan`]`(id, files, base, policy)`, `contentHash` from
/// [`content_hash`], `appVersion` is this build's.
///
/// `base` is the manifest of the snapshot these saves were last in sync
/// with, when the caller has it and the device synced within the last 60
/// days (section 5.4); `None` otherwise, and always for a snapshot made
/// while the cloud can't be reached. A base that is not of layout 2 counts
/// as none ([`packs::plan`] sees to that): packs in a folder of 0.6 are
/// never referred to, because 0.6 deletes them by its own rules.
pub fn draft(place: &Place, draft: &Draft, files: &[LocalFile], base: Option<&Manifest>, policy: &packs::Policy) -> Manifest {
    draft_with(place, draft, files, &[], base, policy)
}

/// [`draft`], with files the snapshot carries over from an earlier one of
/// the line without having them on this device (`carried`): each names the
/// pack that holds it already, and nothing of it is packed or uploaded. A
/// version of a game that shares another platform's line keeps that
/// platform's files in the line this way (docs/plan-v2.md, section 6). They
/// count in the content hash and the size like any file.
pub fn draft_with(place: &Place, draft: &Draft, files: &[LocalFile], carried: &[v2::FileRef], base: Option<&Manifest>, policy: &packs::Policy) -> Manifest {
    let id = ulid::Ulid::from_datetime(std::time::SystemTime::from(draft.created_at)).to_string();
    let mut plan = packs::plan(&id, files, base, policy);
    let mut extra = v2::Extra::new();
    if !plan.sizes.is_empty() {
        extra.insert(packs::PACK_SIZES.to_string(), serde_json::json!(plan.sizes));
    }
    let (content_hash, total_size) = match carried.is_empty() {
        true => (content_hash(files), files.iter().fold(0u64, |sum, f| sum.saturating_add(f.size))),
        false => {
            plan.files.extend(carried.iter().cloned());
            plan.files.sort_by(|a, b| a.path.cmp(&b.path));
            let entries: Vec<crate::snapshot::FileEntry> =
                plan.files.iter().map(|f| crate::snapshot::FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }).collect();
            (crate::snapshot::content_hash(&entries), plan.files.iter().fold(0u64, |sum, f| sum.saturating_add(f.size)))
        }
    };
    Manifest {
        v: v2::LAYOUT,
        id,
        game: place.game.clone(),
        line: place.line.clone(),
        vv: draft.vv.clone(),
        parent: draft.parent.clone(),
        device_id: draft.device_id.clone(),
        device_name: draft.device_name.clone(),
        platform: draft.platform.clone(),
        created_at: draft.created_at,
        trigger: draft.trigger.clone(),
        side: draft.side,
        content_hash,
        total_size,
        files: plan.files,
        game_version: draft.game_version.clone(),
        installed_version: draft.installed_version.clone(),
        info: draft.info.clone(),
        origin: draft.origin.clone(),
        requires: Vec::new(),
        app_version: crate::APP_VERSION.to_string(),
        extra,
    }
}

/// Put a snapshot on this device's disk, ready to upload: every pack the
/// manifest brings itself as `<dir>/<pack id>.zip` ([`packs::build`]), then
/// the manifest as `<dir>/<id>.json`, written last and atomically. From then
/// on the save files may change: the upload needs only `dir`.
///
/// `dir` is the caller's folder for what waits for the cloud; several
/// snapshots share it. A snapshot whose `.json` is missing was not staged
/// completely and doesn't exist ([`staged`] ignores its zips; staging it
/// again replaces them).
pub fn stage(manifest: &Manifest, files: &[LocalFile], dir: &Path) -> Result<()> {
    check_id(&manifest.id)?;
    std::fs::create_dir_all(dir).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", dir.display()))?;
    // Staged again: until the new manifest is in place the snapshot doesn't
    // exist, so zips that are being replaced are never taken for complete.
    let json = dir.join(format!("{}.json", manifest.id));
    remove_local(&json)?;
    let mut built = Vec::new();
    for pack in own_packs(manifest) {
        // Built anew: what an earlier upload sent is not this zip.
        let _ = std::fs::remove_file(sent_mark(dir, &pack));
        match packs::build(manifest, &pack, files, dir) {
            Ok(zip) => built.push(zip),
            Err(e) => {
                for zip in &built {
                    let _ = std::fs::remove_file(zip);
                }
                return Err(e);
            }
        }
    }
    crate::util::atomic_write(&json, &serde_json::to_vec(manifest)?)
}

/// The snapshots waiting in `dir`, oldest first. A missing folder is empty;
/// a `.json` that can't be understood is skipped.
pub fn staged(dir: &Path) -> Result<Vec<Manifest>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| crate::tr!("无法读取目录 {}", "Can't read the folder {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else { continue };
        if !v2::is_snapshot_id(id) || !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        match serde_json::from_slice::<Manifest>(&bytes) {
            Ok(m) if m.id == id => out.push(m),
            _ => {}
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Remove a snapshot's files from `dir` (after [`publish`], or to drop it).
pub fn unstage(dir: &Path, manifest: &Manifest) -> Result<()> {
    check_id(&manifest.id)?;
    // The manifest first: without it the zips left by an interruption are
    // not a snapshot.
    remove_local(&dir.join(format!("{}.json", manifest.id)))?;
    // What a write of the manifest cut short left: `<id>.json.lntmp` from
    // older versions, `<id>.json.<pid>-<n>.lntmp` from `util::atomic_write`.
    let prefix = format!("{}.json.", manifest.id);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".lntmp") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    for pack in own_packs(manifest) {
        remove_local(&dir.join(format!("{pack}.zip")))?;
        let _ = std::fs::remove_file(sent_mark(dir, &pack));
    }
    let _ = std::fs::remove_file(shown_mark(dir, &manifest.id));
    Ok(())
}

/// Upload a staged snapshot and make it part of the line.
///
/// In this order, each step only when the one before succeeded:
///
/// 1. upload the snapshot's own packs from `dir` into the game's first
///    folder, `<id>` first, then `<id>-2`, ...; of several packs, the ones
///    an earlier call got through before it broke off are not sent again
///    (one listing of the packs then, to see that they are still there and
///    whole);
/// 2. write the manifest into [`Place::dir`]: from here on the snapshot
///    exists;
/// 3. a look at the packs ([`packs::gone_since`]): a pack this manifest
///    borrows that another device cleaned up meanwhile is put back with
///    `files`, whole for the other snapshots of the line that use it as far
///    as `files` have theirs; one of those that misses a file of it then is
///    marked. `files` are the save files as they are now, for that case
///    only: [`packs::build`] takes only those whose size and hash are still
///    what the manifest says, so the files of an upload that waited may be
///    passed too (pass none when [`packs::borrows`] says the snapshot
///    borrows nothing);
/// 4. add the manifest to `cache` and write the index.
///
/// The snapshot is published only when every step went through: its
/// manifest is in the cloud and so is every pack it names. When step 3
/// fails, the manifest is removed again (as far as the connection lets it):
/// a version whose pack is gone would stay in the history, usually as the
/// newest version, and nobody could fetch it. A borrowed pack that can't be
/// put back because `files` no longer have its content is a
/// [`packs::MissingPack`]; calling again then fails the same way, which is
/// for the caller to settle.
///
/// An error leaves nothing to undo: packs without a manifest are not a
/// snapshot and are cleaned up later ([`clean`]); a manifest without its
/// index entry is found by the next listing. Calling it again for the same
/// snapshot is safe and ends in the same state (the manifest is written
/// with the same bytes, and its packs are checked once more). It doesn't
/// remove anything from `dir`.
///
/// Writing the manifest before looking at the packs is what keeps a
/// clean-up on another device from taking a borrowed pack in between: one
/// that lists the line after the manifest appeared keeps the pack, one that
/// listed it before is caught by the look of step 3.
///
/// Nothing is ever written into a folder of 0.6.
pub fn publish(store: &dyn Store, place: &Place, cache: &mut LineCache, manifest: &Manifest, dir: &Path, files: &[LocalFile], device_id: &str) -> Result<()> {
    check_id(&manifest.id)?;
    let home = writable(place)?;
    let Some(folder) = place.folders.first() else {
        bail!(crate::tr!("这个游戏在云端没有文件夹", "This game has no folder in the cloud"));
    };
    // What `stage` built, not the save files: they may have changed since.
    let own = own_packs(manifest);
    for pack in &own {
        let zip = dir.join(format!("{pack}.zip"));
        if !zip.is_file() {
            bail!(crate::tr!("等待上传的版本不完整：缺少 {}", "The version waiting for upload is incomplete: {} is missing", zip.display()));
        }
    }
    // What an earlier call sent. The note of it is on this device, so the
    // cloud is asked whether it still has those packs, as large as they
    // are here: packs without a manifest are cleaned up once they look old,
    // and the cloud may be another one by now.
    let arrived: BTreeMap<String, u64> = match own.iter().any(|p| sent_mark(dir, p).is_file()) {
        true => store.list(&v2::packs_dir(folder))?.into_iter().filter(|e| !e.is_dir).map(|e| (e.name, e.size)).collect(),
        false => BTreeMap::new(),
    };
    for pack in &own {
        let zip = dir.join(format!("{pack}.zip"));
        let size = std::fs::metadata(&zip).map(|m| m.len()).ok();
        if sent_mark(dir, pack).is_file() && size.is_some() && arrived.get(&format!("{pack}.zip")).copied() == size {
            continue;
        }
        store.upload(&v2::pack_path(folder, pack), &zip)?;
        if own.len() > 1 {
            let _ = std::fs::write(sent_mark(dir, pack), b"");
        }
    }
    let _ = std::fs::write(shown_mark(dir, &manifest.id), b"");
    line::write_manifest(store, home, manifest)?;
    let verified = (|| -> Result<Vec<(String, String)>> {
        let (missing, present) = packs::gone_since(store, &place.folders, manifest)?;
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        // A folder of its own: a pack put back is built under its own name,
        // which may be the name of another snapshot's zip waiting in `dir`.
        let work = Workspace::new(dir)?;
        let view = line_view_of(cache);
        let lines = [(place, &view)];
        let sources = Sources { local: files, zips: &[] };
        let restored = restore_packs(store, &lines, place, manifest, &missing, sources, Some(&present), &work, device_id, Utc::now())?;
        match restored.lost.first() {
            // One of its own: the caller still has it as a zip and uploads
            // it again; anything else is gone.
            Some(pack) if packs::is_own(pack, &manifest.id) => {
                bail!(crate::tr!("本机没有内容包 {pack} 需要的文件", "This device doesn't have the files the content pack {pack} needs"))
            }
            Some(pack) => Err(packs::MissingPack { pack: pack.clone() }.into()),
            None => Ok(restored.marked),
        }
    })();
    match verified {
        Err(e) => {
            let _ = store.remove(&v2::manifest_path(home, &manifest.id));
            return Err(e);
        }
        // Snapshots of the line that take files from a pack put back that
        // weren't found: marked, as the cache now says too.
        Ok(marked) => {
            for (_, id) in marked {
                if let Some(e) = cache.snapshots.get_mut(&id) {
                    e.incomplete = Some(v2::MarkRef::default());
                }
            }
        }
    }
    cache.insert(manifest);
    line::write_index(store, home, cache, device_id)
}

// ---------------------------------------------------------------------- fetch

/// The whole manifest of a snapshot of `view`, from the folder that holds
/// it. An error when the view doesn't have it or it has been deleted since.
pub fn manifest(store: &dyn Store, place: &Place, view: &LineView, id: &str) -> Result<Manifest> {
    let dir = place.dir_of(view, id).ok_or_else(not_found)?;
    line::read_manifest(store, dir, id)?.ok_or_else(not_found)
}

/// Fetch files of a snapshot into `dest` (`<dest>/<canonical path>`): all,
/// or the paths in `only`. [`packs::read`] with the game's folders and the
/// folder the manifest is in, so `dest` is as it was after any error, and a
/// pack that is gone is a [`packs::MissingPack`]. Putting the files where
/// the game looks for them is the caller's business.
pub fn fetch(store: &dyn Store, place: &Place, view: &LineView, manifest: &Manifest, only: Option<&[String]>, dest: &Path, staging: &Path) -> Result<()> {
    let dir = place.dir_of(view, &manifest.id).ok_or_else(not_found)?;
    packs::read(store, &place.folders, dir, manifest, only, dest, staging)
}

// ------------------------------------------------------- incomplete versions
//
// docs/plan-v2.md, section 3.4, "引用的包被删了怎么办". A snapshot a pack of
// which is gone is marked ([`mark`]): no device takes it while the mark is
// there. Whoever has the content puts the pack back ([`put_back`]) and takes
// the mark away ([`unmark`]); one nobody can repair is let go ([`retire`]).

/// The days a version can stay marked before any device's clean-up lets it
/// go, when nobody could put its packs back.
pub const INCOMPLETE_DAYS: i64 = 14;

/// The reason a version let go by [`retire`] has in "recently deleted".
pub const RETIRED: &str = "incomplete";

/// Mark the snapshot `id` of the line as missing `missing` (packs): one
/// write into the line's first folder, and the cache says so at once.
/// `since` is when it was first found; a mark that is there already keeps
/// its time.
pub fn mark(store: &dyn Store, place: &Place, cache: &mut LineCache, id: &str, missing: &[String], device_id: &str, now: DateTime<Utc>) -> Result<()> {
    check_id(id)?;
    let home = writable(place)?;
    let since = cache.snapshots.get(id).and_then(|e| e.incomplete.as_ref()).and_then(|m| m.since).unwrap_or(now);
    let mark = v2::Mark { missing: missing.to_vec(), since: Some(since), device_id: device_id.to_string(), extra: v2::Extra::new() };
    line::write_mark(store, home, id, &mark)?;
    if let Some(entry) = cache.snapshots.get_mut(id) {
        // The tag is not known until the next listing, which reads the mark
        // once more.
        entry.incomplete = Some(v2::MarkRef { missing: mark.missing, since: mark.since, tag: String::new() });
    }
    Ok(())
}

/// Take the mark of `id` away: its packs are all in the cloud again.
pub fn unmark(store: &dyn Store, place: &Place, cache: &mut LineCache, id: &str) -> Result<()> {
    check_id(id)?;
    line::remove_mark(store, writable(place)?, id)?;
    if let Some(entry) = cache.snapshots.get_mut(id) {
        entry.incomplete = None;
    }
    Ok(())
}

/// A view of what `cache` knows, for looking up snapshots by the packs they
/// use. It doesn't know which folder holds each manifest.
fn line_view_of(cache: &LineCache) -> LineView {
    LineView { snapshots: cache.snapshots.values().cloned().collect(), ..LineView::default() }
}

/// The manifest of `id` of `view`, from the folder that holds it; from the
/// line's folders in order when the view doesn't say. `None` when it is
/// gone.
fn manifest_of(store: &dyn Store, place: &Place, view: &LineView, id: &str) -> Result<Option<Manifest>> {
    if let Some(dir) = place.dir_of(view, id) {
        return line::read_manifest(store, dir, id);
    }
    for dir in &place.dirs {
        if let Some(m) = line::read_manifest(store, dir, id)? {
            return Ok(Some(m));
        }
    }
    Ok(None)
}

/// The snapshots of `lines` other than `except` that take files from
/// `pack`: the line each is in (its position in `lines`), its id, and the
/// files it takes from the pack (with the paths, sizes and hashes its
/// manifest gives them). What a pack put back has to hold to be whole for
/// all of them. Their manifests are read; the store keeps what it read once.
fn sharing(store: &dyn Store, lines: &[(&Place, &LineView)], pack: &str, except: &str) -> Result<Vec<(usize, String, Vec<FileRef>)>> {
    let mut out = Vec::new();
    for (n, (place, view)) in lines.iter().enumerate() {
        for s in view.snapshots.iter().filter(|s| s.id != except && s.packs.iter().any(|p| p == pack)) {
            if let Some(m) = manifest_of(store, place, view, &s.id)? {
                out.push((n, s.id.clone(), packs::files_in(&m, pack)));
            }
        }
    }
    Ok(out)
}

/// What [`restore_packs`] did.
#[derive(Debug, Default)]
struct Restored {
    /// Packs put back.
    repaired: Vec<String>,
    /// Packs that stay gone: what the snapshot takes from them was found
    /// nowhere.
    lost: Vec<String>,
    /// Other snapshots that take a file from a pack put back that was found
    /// nowhere, marked now: the line's first folder, and the id.
    marked: Vec<(String, String)>,
    /// Other snapshots that were marked for a pack put back, whole now: their
    /// marks are gone.
    unmarked: Vec<(String, String)>,
}

/// Put back `missing`, packs of `manifest` (in the line `place`) that are
/// gone, each under its own name: with every file `manifest` takes from it,
/// and every file the other snapshots of `lines` take from it that is found
/// too ([`find_content`]), so the pack is whole for as many of them as can
/// be. A pack is put back only when everything `manifest` takes from it is
/// found. Another snapshot that takes a file from it that wasn't found is
/// marked ([`v2::Mark`]): a pack that is there but lacks one of its files
/// is broken in a way no listing shows.
#[allow(clippy::too_many_arguments)]
fn restore_packs(
    store: &dyn Store,
    lines: &[(&Place, &LineView)],
    place: &Place,
    manifest: &Manifest,
    missing: &BTreeSet<String>,
    sources: Sources<'_>,
    present: Option<&BTreeSet<String>>,
    work: &Workspace,
    device_id: &str,
    now: DateTime<Utc>,
) -> Result<Restored> {
    let Some(home) = place.folders.first() else {
        bail!(crate::tr!("这个游戏在云端没有文件夹", "This game has no folder in the cloud"));
    };
    let listed;
    let present = match present {
        Some(p) => p,
        None => {
            listed = packs::present_in(store, &place.folders)?;
            &listed
        }
    };
    let skip = BTreeSet::from([manifest.id.clone()]);
    let mut out = Restored::default();
    for pack in missing {
        let mine = packs::files_in(manifest, pack);
        let others = sharing(store, lines, pack, &manifest.id)?;
        // Each path once; a file another snapshot gives other contents than
        // this one can't be in the same pack, and that snapshot stays out.
        let mut wanted: BTreeMap<String, FileRef> = mine.iter().map(|f| (f.path.clone(), f.clone())).collect();
        for f in others.iter().flat_map(|(_, _, files)| files) {
            wanted.entry(f.path.clone()).or_insert_with(|| f.clone());
        }
        let wanted: Vec<FileRef> = wanted.into_values().collect();
        let found = find_content(store, &wanted, sources, lines, present, &skip, manifest.created_at, work)?;
        let held = |f: &FileRef| found.iter().any(|l| l.path == f.path && l.size == f.size && l.hash == f.hash);
        if !mine.iter().all(held) {
            out.lost.push(pack.clone());
            continue;
        }
        let files: Vec<FileRef> = wanted.into_iter().filter(|f| held(f)).collect();
        packs::rebuild(store, home, manifest, pack, &files, &found, work.path())?;
        out.repaired.push(pack.clone());
        for (n, id, theirs) in others {
            let (their_place, their_view) = lines[n];
            let Some(entry) = their_view.get(&id) else { continue };
            if writable(their_place).is_err() {
                continue;
            }
            let whole = theirs.iter().all(held);
            // Whole now, where this pack was all its mark named and its
            // other packs are there: its mark goes.
            let only_this = entry.incomplete.as_ref().is_some_and(|m| m.missing.iter().all(|p| p == pack));
            if whole && only_this && entry.packs.iter().all(|p| p == pack || present.contains(p)) {
                line::remove_mark(store, their_place.dir(), &id)?;
                out.unmarked.push((their_place.dir().to_string(), id));
            } else if !whole && !entry.is_incomplete() {
                let mark = v2::Mark { missing: vec![pack.clone()], since: Some(now), device_id: device_id.to_string(), extra: v2::Extra::new() };
                line::write_mark(store, their_place.dir(), &id, &mark)?;
                out.marked.push((their_place.dir().to_string(), id));
            }
        }
    }
    Ok(out)
}

/// Whether `pack`, which the cloud lists, holds every file `manifest` takes
/// from it as the manifest has it: downloaded and looked at. For a snapshot
/// that is marked although its packs are all there (another device put one
/// back meanwhile, maybe only partly).
fn holds_all(store: &dyn Store, place: &Place, view: &LineView, manifest: &Manifest, pack: &str, work: &Workspace) -> Result<bool> {
    let files = packs::files_in(manifest, pack);
    let refs: Vec<&FileRef> = files.iter().collect();
    let dir = place.dir_of(view, &manifest.id).unwrap_or(place.dir());
    let into = work.path().join(format!("check-{}", ulid::Ulid::new()));
    let checked = packs::take(store, &place.folders, dir, manifest, pack, &refs, &into, work.path());
    let _ = std::fs::remove_dir_all(&into);
    match checked {
        Ok(()) => Ok(true),
        Err(e) if super::is_unavailable(&e) => Err(e),
        Err(_) => Ok(false),
    }
}

/// Where the content of a pack that is gone may still be on this device.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sources<'a> {
    /// The save files as they are now.
    pub local: &'a [LocalFile],
    /// Zips: packs waiting to be uploaded, copies kept in local-backups.
    pub zips: &'a [PathBuf],
}

/// A folder of its own for content taken out of zips and packs, removed
/// again when dropped.
pub struct Workspace(PathBuf);

impl Workspace {
    pub fn new(staging: &Path) -> Result<Workspace> {
        let dir = staging.join(format!("repair-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", dir.display()))?;
        Ok(Workspace(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The snapshots of `lines` whose packs may hold content, the ones made
/// closest to `near` first: none that is marked itself, nor one this build
/// can't read, nor the snapshots in `skip`.
fn candidates<'a>(lines: &'a [(&'a Place, &'a LineView)], skip: &BTreeSet<String>, near: DateTime<Utc>) -> Vec<(&'a Place, &'a LineView, &'a IndexEntry)> {
    let mut out: Vec<(&Place, &LineView, &IndexEntry)> = lines
        .iter()
        .flat_map(|(place, view)| view.snapshots.iter().map(move |s| (*place, *view, s)))
        .filter(|(_, _, s)| !skip.contains(&s.id) && !s.is_incomplete() && s.unsupported().is_empty())
        .collect();
    out.sort_by_key(|(_, _, s)| (s.created_at - near).num_seconds().unsigned_abs());
    out
}

/// Find content for `wanted` (files of a snapshot, with their paths, sizes
/// and hashes): what has the same size and hash, in this order (section 3.4,
/// the chain of repair):
///
/// 1. the save files on this device now;
/// 2. the zips on this device (`sources.zips`);
/// 3. another pack of the game in the cloud, found through the manifests of
///    the snapshots of `lines` (read one by one, the closest in time to
///    `near` first, until everything is found) and downloaded whole. Only
///    packs in `present` (the packs the cloud has) are tried.
///
/// Returns what was found, each under the path `wanted` gives it, its bytes
/// in `sources.local` or in `work`. Files found nowhere are left out.
#[allow(clippy::too_many_arguments)]
pub fn find_content(
    store: &dyn Store,
    wanted: &[FileRef],
    sources: Sources<'_>,
    lines: &[(&Place, &LineView)],
    present: &BTreeSet<String>,
    skip: &BTreeSet<String>,
    near: DateTime<Utc>,
    work: &Workspace,
) -> Result<Vec<LocalFile>> {
    let mut found: Vec<LocalFile> = Vec::new();
    let mut open: Vec<&FileRef> = Vec::new();
    for f in wanted {
        if !open.iter().any(|o| o.path == f.path) {
            open.push(f);
        }
    }
    let same = |a: &FileRef, size: u64, hash: &str| a.size == size && a.hash == hash && !hash.is_empty();
    let as_found = |f: &FileRef, source: PathBuf| LocalFile { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime, source };

    // 1. The saves here.
    open.retain(|f| match sources.local.iter().find(|l| same(f, l.size, &l.hash)) {
        Some(l) => {
            found.push(as_found(f, l.source.clone()));
            false
        }
        None => true,
    });

    // 2. Zips on this device.
    for (n, zip) in sources.zips.iter().enumerate() {
        if open.is_empty() {
            break;
        }
        let held = packs::zip_contents(zip);
        let mut take: Vec<(&FileRef, &FileRef)> = Vec::new();
        for f in &open {
            if let Some(h) = held.iter().find(|h| same(f, h.size, &h.hash)) {
                take.push((f, h));
            }
        }
        if take.is_empty() {
            continue;
        }
        let into = work.path().join(format!("zip-{n}"));
        let mut entries: Vec<&FileRef> = take.iter().map(|(_, h)| *h).collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);
        if packs::unzip(zip, &entries, &into).is_err() {
            continue;
        }
        for (f, h) in take {
            found.push(as_found(f, packs_local(&into, &h.path)));
        }
        open.retain(|f| !found.iter().any(|l| l.path == f.path));
    }

    // 3. Other packs in the cloud.
    if open.is_empty() {
        return Ok(found);
    }
    let mut plan: Vec<(&Place, &LineView, Manifest, String, Vec<(FileRef, FileRef)>)> = Vec::new();
    for (place, view, s) in candidates(lines, skip, near) {
        if open.is_empty() {
            break;
        }
        // Only a snapshot with a pack that is there can help; one of 0.6
        // keeps its zip beside its manifest.
        let legacy = place.dir_of(view, &s.id).is_some_and(|d| v2::legacy_key(d).is_some());
        if !legacy && !s.packs.iter().any(|p| present.contains(p)) {
            continue;
        }
        let Some(m) = manifest_of(store, place, view, &s.id)? else { continue };
        if !m.unsupported().is_empty() {
            continue;
        }
        let mut by_pack: BTreeMap<String, Vec<(FileRef, FileRef)>> = BTreeMap::new();
        open.retain(|f| {
            let hit = m.files.iter().find(|c| same(f, c.size, &c.hash) && (legacy || present.contains(c.pack.as_deref().unwrap_or(&m.id))));
            match hit {
                Some(c) => {
                    by_pack.entry(c.pack.clone().unwrap_or_else(|| m.id.clone())).or_default().push(((*f).clone(), c.clone()));
                    false
                }
                None => true,
            }
        });
        for (pack, files) in by_pack {
            plan.push((place, view, m.clone(), pack, files));
        }
    }
    for (n, (place, view, m, pack, files)) in plan.into_iter().enumerate() {
        let dir = place.dir_of(view, &m.id).unwrap_or(place.dir());
        let into = work.path().join(format!("pack-{n}"));
        let mut entries: Vec<&FileRef> = files.iter().map(|(_, c)| c).collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);
        match packs::take(store, &place.folders, dir, &m, &pack, &entries, &into, work.path()) {
            Ok(()) => {
                for (f, c) in &files {
                    found.push(as_found(f, packs_local(&into, &c.path)));
                }
            }
            // No connection is not "nowhere": the caller tries again later.
            Err(e) if super::is_unavailable(&e) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(found)
}

/// Where a file unpacked under `base` is.
fn packs_local(base: &Path, canonical: &str) -> PathBuf {
    canonical.split('/').fold(base.to_path_buf(), |p, c| p.join(c))
}

/// What became of a snapshot a pack of which was gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repair {
    /// Every pack it names is in the cloud (again, or still).
    Complete,
    /// These packs were put back, whole for every snapshot that uses them.
    Repaired(Vec<String>),
    /// These packs are gone and their content was found nowhere.
    Lost(Vec<String>),
}

/// Make a snapshot in the cloud whole again: its packs that are gone are put
/// back (a manifest in the cloud never changes, so the packs are what is
/// repaired), each under its own name, with what it takes from it and as
/// much as is found of what the other snapshots of `lines` take from it.
/// The content comes from [`find_content`]. A pack is put back only when
/// everything this snapshot takes from it is found; another snapshot that
/// misses a file of it then is marked.
///
/// A snapshot that is marked although its packs are all listed (the mark
/// names them) is looked at more closely: the packs its mark names are
/// downloaded, and one that lacks a file it takes from it counts as gone.
/// So a mark goes only when the snapshot is known to be whole.
///
/// `lines` are all the game's lines (packs belong to the game), `place` the
/// line of `manifest`. `present` is what the caller listed of the game's
/// `packs/` folders; without it they are listed here (one listing each).
/// `device_id` and `now` go into the marks it writes.
#[allow(clippy::too_many_arguments)]
pub fn put_back(
    store: &dyn Store,
    lines: &[(&Place, &LineView)],
    place: &Place,
    manifest: &Manifest,
    present: Option<&BTreeSet<String>>,
    sources: Sources<'_>,
    staging: &Path,
    device_id: &str,
    now: DateTime<Utc>,
) -> Result<Repair> {
    if manifest.v < v2::LAYOUT || !manifest.unsupported().is_empty() {
        // A snapshot of 0.6 keeps its zip beside its manifest; one written
        // with features this build doesn't know may keep its files otherwise.
        return Ok(Repair::Complete);
    }
    let listed;
    let present = match present {
        Some(p) => p,
        None => {
            listed = packs::present_in(store, &place.folders)?;
            &listed
        }
    };
    let work = Workspace::new(staging)?;
    let mut missing: BTreeSet<String> = manifest.packs().into_iter().filter(|p| !present.contains(p)).collect();
    let view = lines.iter().find(|(p, _)| p.dir() == place.dir()).map(|(_, v)| *v).cloned().unwrap_or_default();
    // A mark that doesn't say which packs: all of them are looked at.
    let suspect: Vec<String> = match view.get(&manifest.id).and_then(|e| e.incomplete.as_ref()) {
        Some(m) if m.missing.is_empty() => manifest.packs().into_iter().collect(),
        Some(m) => m.missing.clone(),
        None => Vec::new(),
    };
    for pack in suspect.iter().filter(|p| present.contains(*p) && manifest.packs().contains(*p)) {
        if !holds_all(store, place, &view, manifest, pack, &work)? {
            missing.insert(pack.clone());
        }
    }
    if missing.is_empty() {
        return Ok(Repair::Complete);
    }
    let restored = restore_packs(store, lines, place, manifest, &missing, sources, Some(present), &work, device_id, now)?;
    Ok(match restored.lost.is_empty() {
        true => Repair::Repaired(restored.repaired),
        false => Repair::Lost(restored.lost),
    })
}

/// Make a snapshot waiting on this device whole before it is uploaded, when
/// a pack it borrows is gone (it waited longer than the cloud keeps what it
/// builds on, or what it builds on left the line). Nobody has seen it yet,
/// so it may change ([`was_shown`] must be false):
///
/// - a file found with the same path, size and hash in a pack the cloud has
///   (through the manifests of `lines`) is taken from there: nothing to
///   upload;
/// - any other file is looked for as [`find_content`] does and packed anew
///   into a pack of its own (`<id>` when it has none yet, else the next
///   `<id>-<n>`), built into `dir` beside the others, uploaded with them.
///
/// Returns the snapshot as it is to be uploaded now (written into `dir` by
/// the caller), or `None` when nothing it borrows is gone. A file found
/// nowhere is a [`packs::MissingPack`] and nothing changes.
#[allow(clippy::too_many_arguments)]
pub fn repair_queued(
    store: &dyn Store,
    lines: &[(&Place, &LineView)],
    place: &Place,
    manifest: &Manifest,
    sources: Sources<'_>,
    dir: &Path,
    staging: &Path,
) -> Result<Option<Manifest>> {
    let borrowed: BTreeSet<String> = manifest.packs().into_iter().filter(|p| !packs::is_own(p, &manifest.id)).collect();
    if borrowed.is_empty() {
        return Ok(None);
    }
    let present = packs::present_in(store, &place.folders)?;
    let gone: BTreeSet<String> = borrowed.into_iter().filter(|p| !present.contains(p)).collect();
    if gone.is_empty() {
        return Ok(None);
    }
    let wanted: Vec<FileRef> = manifest.files.iter().filter(|f| f.pack.as_ref().is_some_and(|p| gone.contains(p))).cloned().collect();

    // Taken from packs that are there, by reference.
    let skip = BTreeSet::from([manifest.id.clone()]);
    let mut repointed: BTreeMap<String, String> = BTreeMap::new();
    for (place_c, view, s) in candidates(lines, &skip, manifest.created_at) {
        if repointed.len() == wanted.len() {
            break;
        }
        if !s.packs.iter().any(|p| present.contains(p) && !gone.contains(p)) {
            continue;
        }
        let Some(m) = manifest_of(store, place_c, view, &s.id)? else { continue };
        if m.v < v2::LAYOUT || !m.unsupported().is_empty() {
            continue;
        }
        for f in &wanted {
            if repointed.contains_key(&f.path) {
                continue;
            }
            if let Some(c) = m.files.iter().find(|c| c.path == f.path && c.size == f.size && c.hash == f.hash) {
                let pack = c.pack.clone().unwrap_or_else(|| m.id.clone());
                if present.contains(&pack) && !gone.contains(&pack) {
                    repointed.insert(f.path.clone(), pack);
                }
            }
        }
    }

    // The rest from wherever it still is, into a pack of its own.
    let rest: Vec<FileRef> = wanted.iter().filter(|f| !repointed.contains_key(&f.path)).cloned().collect();
    let work = Workspace::new(staging)?;
    let found = find_content(store, &rest, sources, lines, &present, &skip, manifest.created_at, &work)?;
    if let Some(f) = rest.iter().find(|f| !found.iter().any(|l| l.path == f.path)) {
        let pack = f.pack.clone().unwrap_or_default();
        return Err(packs::MissingPack { pack }.into());
    }
    let own = own_packs(manifest);
    let new_pack = match own.iter().any(|p| p == &manifest.id) {
        false => manifest.id.clone(),
        true => {
            let highest = own.iter().filter_map(|p| p.strip_prefix(&format!("{}-", manifest.id))?.parse::<u32>().ok()).max().unwrap_or(1);
            format!("{}-{}", manifest.id, highest + 1)
        }
    };
    let mut out = manifest.clone();
    for f in out.files.iter_mut() {
        if let Some(pack) = repointed.get(&f.path) {
            f.pack = Some(pack.clone());
        } else if rest.iter().any(|r| r.path == f.path) {
            f.pack = (new_pack != manifest.id).then(|| new_pack.clone());
        }
    }
    // What the packs it uses hold, as far as known: the ones that are gone
    // no longer, the new one just what it holds.
    if let Some(sizes) = out.extra.get_mut(packs::PACK_SIZES).and_then(|v| v.as_object_mut()) {
        sizes.retain(|pack, _| !gone.contains(pack));
        if !rest.is_empty() {
            sizes.insert(new_pack.clone(), rest.iter().map(|f| f.size).sum::<u64>().into());
        }
    }
    if !rest.is_empty() {
        let _ = std::fs::remove_file(sent_mark(dir, &new_pack));
        packs::build(&out, &new_pack, &found, dir)?;
    }
    Ok(Some(out))
}

/// Let go of a version nobody could make whole (section 3.4): into "recently
/// deleted", with the reason [`RETIRED`], so heads and decisions move on; it
/// is purged after [`super::TRASH_DAYS`] like any deleted version, and can
/// be put back until then. Its mark goes with it.
///
/// Refused (an error) for a kept save (the user decides about it), for one
/// in a folder of 0.6 (0.6 takes the newest there for the current progress)
/// and when the line has no other version that isn't marked: a line with
/// nothing else keeps it, marked. [`can_retire`] says which.
pub fn retire(store: &dyn Store, place: &Place, view: &LineView, id: &str, by: &str, now: DateTime<Utc>) -> Result<()> {
    if !can_retire(place, view, id) {
        bail!(crate::tr!("这个版本不能移到「最近删除」", "This version can't be moved to Recently deleted"));
    }
    let dir = place.dir_of(view, id).ok_or_else(not_found)?;
    let meta = store.read(&v2::manifest_path(dir, id))?.ok_or_else(not_found)?;
    if !serde_json::from_slice::<Manifest>(&meta).is_ok_and(|m| m.id == id) {
        bail!(crate::tr!("这个版本的清单读不懂", "Can't understand this version's manifest"));
    }
    store.write(&v2::trash_path(dir, id), &record_bytes(now, by, Some(RETIRED), &meta)?)?;
    store.remove(&v2::manifest_path(dir, id))?;
    let _ = line::remove_mark(store, place.dir(), id);
    Ok(())
}

/// Whether [`retire`] may let go of `id`: it is in the view, not kept, not in
/// a folder of 0.6, and the line has another version that is not marked.
pub fn can_retire(place: &Place, view: &LineView, id: &str) -> bool {
    retirable(place, view, id, &BTreeSet::new(), &[])
}

/// [`can_retire`], where the snapshots in `incomplete` count as marked too
/// and those in `gone` are no longer there.
fn retirable(place: &Place, view: &LineView, id: &str, incomplete: &BTreeSet<String>, gone: &[String]) -> bool {
    let Some(entry) = view.get(id) else { return false };
    let legacy = place.dir_of(view, id).is_none_or(|d| v2::legacy_key(d).is_some());
    let whole = |s: &&IndexEntry| s.id != id && !s.is_incomplete() && !incomplete.contains(&s.id) && !gone.contains(&s.id);
    entry.pin.is_none() && !legacy && view.snapshots.iter().any(|s| whole(&s))
}

// ----------------------------------------------------------------- kept saves

/// Keep a snapshot, or rename a kept one (section 3.7): write
/// `<id>.pin.json` into [`Place::dir`], wherever the manifest is. For a
/// snapshot `view` shows with a pin, the pin is read first and only its name
/// changes (its note, time, device and unknown fields stay); otherwise a new
/// pin is written with `now` and `device_id`. An error when the view doesn't
/// have the snapshot.
pub fn keep(store: &dyn Store, place: &Place, view: &LineView, id: &str, name: &str, device_id: &str, now: DateTime<Utc>) -> Result<()> {
    let home = writable(place)?;
    let entry = view.get(id).ok_or_else(not_found)?;
    let mut kept = None;
    if entry.pin.is_some() {
        // The view doesn't say which folder the pin is in; this module and
        // the upgrade write them into the first.
        for dir in &place.dirs {
            if let Some(bytes) = store.read(&v2::pin_path(dir, id))? {
                kept = serde_json::from_slice::<Pin>(&bytes).ok();
                break;
            }
        }
    }
    let pin = match kept {
        Some(pin) => Pin { name: name.to_string(), ..pin },
        // Not kept, unkept meanwhile, or a pin that can't be understood.
        None => Pin { name: name.to_string(), note: String::new(), created_at: Some(now), device_id: device_id.to_string(), extra: v2::Extra::new() },
    };
    line::write_pin(store, home, id, &pin)
}

/// Stop keeping a snapshot: remove its pin from every folder of the line.
/// It is an automatic snapshot again and goes the way [`clean`] says.
pub fn unkeep(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    check_id(id)?;
    for dir in &place.dirs {
        line::remove_pin(store, dir, id)?;
    }
    Ok(())
}

// --------------------------------------------------------------------- delete

/// A version the user deleted. It is in no history any more and can be put
/// back until it is purged, [`super::TRASH_DAYS`] after `deleted_at`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trashed {
    pub snapshot: IndexEntry,
    pub deleted_at: DateTime<Utc>,
    /// Name of the device it was deleted on.
    pub deleted_by: String,
    /// The line folder it was deleted from.
    pub dir: String,
    /// Why, when the user didn't delete it: [`RETIRED`].
    pub reason: Option<String>,
}

/// Delete a version the way the user does: into "recently deleted".
///
/// Refused, with a message for the user, for a head of the line (the other
/// devices would be asked about a version that is gone) and for a kept save
/// (stop keeping it first). An error when the view doesn't have it. A head
/// that is marked as missing a pack is no newest version for anyone and may
/// go; the newest complete one then counts as the head.
///
/// The record first (`{deletedAt, deletedBy, meta}` in
/// [`v2::trash_path`] of the folder the manifest is in, `meta` being the
/// manifest exactly as it is in the cloud, unknown fields included), then
/// the manifest is removed: an interruption leaves the version in the
/// history rather than nowhere. Its packs stay where they are; its mark, if
/// it had one, goes.
pub fn trash(store: &dyn Store, place: &Place, view: &LineView, id: &str, by: &str, now: DateTime<Utc>) -> Result<()> {
    let entry = view.get(id).ok_or_else(not_found)?;
    let head = |v: &LineView| v.head_entries().any(|h| h.id == id);
    if (head(view) && !entry.is_incomplete()) || head(&view.usable()) {
        bail!(crate::tr!("不能删除最新版本", "The latest version can't be deleted"));
    }
    if entry.pin.is_some() {
        bail!(crate::tr!("这是保留的存档，请先取消保留", "This save is kept. Stop keeping it first"));
    }
    let dir = place.dir_of(view, id).ok_or_else(not_found)?;
    let meta = store.read(&v2::manifest_path(dir, id))?.ok_or_else(not_found)?;
    if !serde_json::from_slice::<Manifest>(&meta).is_ok_and(|m| m.id == id) {
        bail!(crate::tr!("这个版本的清单读不懂", "Can't understand this version's manifest"));
    }
    // The record first: an interruption leaves the version in the history
    // rather than nowhere.
    store.write(&v2::trash_path(dir, id), &record_bytes(now, by, None, &meta)?)?;
    store.remove(&v2::manifest_path(dir, id))?;
    if entry.is_incomplete() && v2::legacy_key(place.dir()).is_none() {
        line::remove_mark(store, place.dir(), id)?;
    }
    Ok(())
}

/// What is in "recently deleted" for this line, newest deletion first.
///
/// A record whose manifest is back in the folder it was deleted from is not
/// listed: the deletion was interrupted, or the version was put back
/// meanwhile. In a folder of 0.6, a record whose zip is gone was half
/// purged: it is removed and not listed, as 0.6 does. Records that can't be
/// understood are skipped.
pub fn trashed(store: &dyn Store, place: &Place) -> Result<Vec<Trashed>> {
    let mut out = Vec::new();
    for dir in &place.dirs {
        let records: Vec<String> = store.list(&v2::trash_dir(dir))?.iter().filter_map(record_id).map(str::to_string).collect();
        if records.is_empty() {
            continue;
        }
        let here: BTreeSet<String> = store.list(dir)?.into_iter().filter(|e| !e.is_dir).map(|e| e.name).collect();
        let legacy = v2::legacy_key(dir).is_some();
        for id in records {
            if here.contains(&format!("{id}.json")) {
                continue;
            }
            let path = v2::trash_path(dir, &id);
            if legacy && !here.contains(&format!("{id}.zip")) {
                // What 0.6 does with it (`Repo::list_trash`).
                let _ = store.remove(&path);
                continue;
            }
            let Some(record) = store.read(&path)?.and_then(|bytes| parse_record(&bytes, &id)) else { continue };
            let mut snapshot = record.meta.summary();
            // As the view shows a snapshot of 0.6, so it shows the same once
            // it is put back.
            if snapshot.vv.is_empty() && !snapshot.is_side() {
                snapshot.vv = line::legacy_vv(dir, snapshot.created_at);
            }
            out.push(Trashed { snapshot, deleted_at: record.deleted_at, deleted_by: record.deleted_by, dir: dir.clone(), reason: record.reason });
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.deleted_at));
    Ok(out)
}

/// Put a deleted version back into the history: the manifest is written
/// back where it was, byte for byte what the record holds, then the record
/// is removed. An error for the user when it is no longer there (purged
/// meanwhile; in a folder of 0.6 also when its zip is gone).
///
/// The version is an automatic snapshot again. A caller that doesn't want
/// [`clean`] to take it away the next day keeps it ([`keep`]).
pub fn untrash(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    let gone = || {
        anyhow!(crate::tr!(
            "「最近删除」里没有这个版本了，可能已经被彻底删除",
            "This version is no longer in Recently deleted; it may have been deleted for good"
        ))
    };
    let (dir, bytes) = find_record(store, place, id)?.ok_or_else(gone)?;
    let meta = parse_record(&bytes, id)
        .and_then(|_| raw_field(&bytes, "meta"))
        .ok_or_else(|| anyhow!(crate::tr!("「最近删除」里这个版本的记录读不懂", "Can't understand the record of this version in Recently deleted")))?;
    if v2::legacy_key(dir).is_some() {
        let zip = format!("{id}.zip");
        if !store.list(dir)?.iter().any(|e| !e.is_dir && e.name == zip) {
            return Err(gone());
        }
    }
    store.write(&v2::manifest_path(dir, id), meta)?;
    store.remove(&v2::trash_path(dir, id))
}

/// Delete a version in "recently deleted" for good. Refused when its
/// manifest is back in the folder it was deleted from (another device
/// restored it). A version that is no longer there (purged meanwhile) is
/// nothing to do.
///
/// The record is removed; in a folder of 0.6 the snapshot's zip first, as
/// 0.6 does. Packs of layout 2 are not touched here: whether another
/// snapshot still uses one takes all the game's lines to tell, which is
/// [`clean`]'s work. They are gone after the next [`clean`] that finds them
/// unused and older than a day.
pub fn purge(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    check_id(id)?;
    let dir = match place.dirs.as_slice() {
        [only] => only.as_str(),
        _ => match find_record(store, place, id)? {
            Some((dir, _)) => dir,
            // Purged meanwhile.
            None => return Ok(()),
        },
    };
    if !purge_at(store, dir, id, true)? {
        bail!(crate::tr!("这个版本已经被恢复到版本历史里，不能在这里彻底删除", "This version was restored to the history and can't be deleted for good here"));
    }
    Ok(())
}

/// Purge the version `id` deleted from `dir`. With `check`, its manifest is
/// looked for first, and nothing happens when it is back (`false`).
fn purge_at(store: &dyn Store, dir: &str, id: &str, check: bool) -> Result<bool> {
    if check && store.read(&v2::manifest_path(dir, id))?.is_some() {
        return Ok(false);
    }
    if v2::legacy_key(dir).is_some() {
        store.remove(&join(&[dir, &format!("{id}.zip")]))?;
    }
    store.remove(&v2::trash_path(dir, id))?;
    Ok(true)
}

/// A record of "recently deleted": the format of 0.6 (`repo::TrashRecord`),
/// with any manifest.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    deleted_at: DateTime<Utc>,
    #[serde(default)]
    deleted_by: String,
    /// Why, when it wasn't the user ([`RETIRED`]). 0.6 doesn't know the
    /// field and ignores it.
    #[serde(default)]
    reason: Option<String>,
    meta: Manifest,
}

fn parse_record(bytes: &[u8], id: &str) -> Option<Record> {
    serde_json::from_slice::<Record>(bytes).ok().filter(|r| r.meta.id == id)
}

/// The id of a record in a listing of "recently deleted".
fn record_id(e: &Entry) -> Option<&str> {
    e.name.strip_suffix(".json").filter(|id| !e.is_dir && v2::is_snapshot_id(id))
}

/// A record with `meta` exactly as the manifest is in the cloud, so putting
/// it back writes the same bytes: unknown fields, their order and the way it
/// was written all stay.
fn record_bytes(deleted_at: DateTime<Utc>, by: &str, reason: Option<&str>, meta: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(meta.len() + 128);
    out.extend_from_slice(b"{\"deletedAt\":");
    out.extend(serde_json::to_vec(&deleted_at)?);
    out.extend_from_slice(b",\"deletedBy\":");
    out.extend(serde_json::to_vec(by)?);
    if let Some(reason) = reason {
        out.extend_from_slice(b",\"reason\":");
        out.extend(serde_json::to_vec(reason)?);
    }
    out.extend_from_slice(b",\"meta\":");
    out.extend_from_slice(meta);
    out.push(b'}');
    Ok(out)
}

/// The value of the field `name` of the JSON object `json`, as the bytes it
/// is written with there. `None` when it isn't there or `json` is not an
/// object.
fn raw_field<'a>(json: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let skip_space = |i: &mut usize| {
        while json.get(*i).is_some_and(|b| b.is_ascii_whitespace()) {
            *i += 1;
        }
    };
    let mut i = 0;
    skip_space(&mut i);
    if json.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    loop {
        skip_space(&mut i);
        match json.get(i)? {
            b'}' => return None,
            b',' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let mut keys = serde_json::Deserializer::from_slice(&json[i..]).into_iter::<String>();
        let key = keys.next()?.ok()?;
        i += keys.byte_offset();
        skip_space(&mut i);
        if json.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        skip_space(&mut i);
        let mut values = serde_json::Deserializer::from_slice(&json[i..]).into_iter::<serde::de::IgnoredAny>();
        values.next()?.ok()?;
        let end = i + values.byte_offset();
        if key == name {
            return Some(&json[i..end]);
        }
        i = end;
    }
}

/// The record of the version `id` and the line folder it was deleted from:
/// the first folder whose "recently deleted" has it.
fn find_record<'a>(store: &dyn Store, place: &'a Place, id: &str) -> Result<Option<(&'a str, Vec<u8>)>> {
    check_id(id)?;
    for dir in &place.dirs {
        if let Some(bytes) = store.read(&v2::trash_path(dir, id))? {
            return Ok(Some((dir.as_str(), bytes)));
        }
    }
    Ok(None)
}

/// The records in "recently deleted" of the line folder `dir` that are not
/// in `seen` yet (paths, which this adds to): `None` for one that can't be
/// understood. A record removed since the listing is left out.
fn read_trash(store: &dyn Store, dir: &str, seen: &mut BTreeSet<String>) -> Result<Vec<(String, Option<Record>)>> {
    let mut out = Vec::new();
    for e in store.list(&v2::trash_dir(dir))? {
        let Some(id) = record_id(&e) else { continue };
        let path = v2::trash_path(dir, id);
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(bytes) = store.read(&path)? {
            out.push((id.to_string(), parse_record(&bytes, id)));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------- clean up

/// One line of the game being cleaned.
pub struct Open<'a> {
    pub place: &'a Place,
    pub cache: &'a mut LineCache,
    /// What each known device synced last on this line
    /// ([`retention::DeviceBase`]), this device included.
    pub bases: Vec<DeviceBase>,
}

/// What [`clean`] removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// Snapshots retention let go, by id.
    pub snapshots: Vec<String>,
    /// Versions that had been in "recently deleted" long enough.
    pub purged: Vec<String>,
    /// Packs nothing uses any more.
    pub packs: Vec<String>,
    /// Snapshots found missing a pack, marked now.
    pub marked: Vec<String>,
    /// Snapshots that were missing a pack and are whole again: their packs
    /// were put back, or are there again.
    pub repaired: Vec<String>,
    /// Snapshots nobody could make whole, moved to "recently deleted"
    /// ([`retire`]).
    pub retired: Vec<String>,
}

/// What [`clean_with`] may use to put back a pack that is gone.
#[derive(Clone, Copy)]
pub struct Repairs<'a> {
    /// The save files on this device, read only when a version needs them.
    pub local: &'a dyn Fn() -> Vec<LocalFile>,
    /// Zips on this device ([`Sources::zips`]).
    pub zips: &'a [PathBuf],
    /// Where content is unpacked and packs are built.
    pub staging: &'a Path,
    /// This device's name, for "recently deleted".
    pub device_name: &'a str,
}

/// Clean up one game: `lines` are all its save lines (packs belong to the
/// game, so whether one is still used takes every line to tell).
///
/// 1. Each line is read again ([`open`]) and [`retention::plan`] is asked.
///    The snapshots it names are deleted (their manifests), except those in
///    a folder of 0.6, which are never touched. Nothing is deleted from a line with manifests this build
///    can't read ([`LineView::unreadable`]): what it can't read might be the
///    newest version. A snapshot retention names had no pin in the view
///    (retention keeps every kept save), but another device may have kept it
///    since the listing: right before its manifest is removed, its pin is
///    looked for in the line's first folder (and in the manifest's folder
///    when that is another one), and a snapshot that has one stays. A kept
///    save is kept while its pin is there (section 3.7). A pin written in
///    the moment between that look and the removal stays behind alone; a
///    listing ignores a pin without its manifest.
/// 2. Versions deleted by the user [`super::TRASH_DAYS`] days or more
///    before `now` (whole days, the rule 0.6 has) are purged, in folders of
///    0.6 as well, except one whose manifest the listing of step 1 shows in
///    the line (see [`trashed`]). In a folder of 0.6 the manifest is looked for once
///    more right before the zip is deleted, as [`purge`] does; elsewhere
///    only the record is removed, and a version put back meanwhile keeps its
///    packs because step 3 counts from fresh listings.
/// 3. Packs: the packs still used are those of every snapshot left in every
///    line and of every version in "recently deleted". Each `packs/` folder
///    of the game is listed and [`packs::unused`] names candidates (older
///    than `grace`). If there are any, every line and its trash are listed
///    once more, and only packs still unused then are deleted: a device that
///    wrote a manifest in between keeps its packs (section 3.4, rule 2).
///    No pack is deleted while a manifest or a record of "recently deleted"
///    can't be understood: which packs it uses is unknown.
/// 4. Versions missing a pack (section 3.4), from what steps 1 and 3 read
///    anyway: a snapshot of layout 2 whose index entry names a pack none of
///    the listings of step 3 has, or one with a mark. [`put_back`] tries to
///    make it whole (with `repairs`; without, nothing is put back). Whole:
///    its mark goes. Not: it is marked ([`mark`]), and let go ([`retire`])
///    when it has been marked for [`INCOMPLETE_DAYS`], or at once when this
///    device wrote it (it had its chance), unless [`can_retire`] says no.
/// 5. The index of a line is written again only if reading it said so
///    ([`LineView::needs_index`]); entries for manifests that are gone do no
///    harm and disappear with the next upload.
///
/// Requests: per line its listings and one listing per trash folder (plus a
/// read per record in it); per game folder one listing of `packs/`; one
/// remove per file deleted, and one read per snapshot retention names, for
/// its pin (two when its manifest is not in the line's first folder); one
/// read more per version purged from a folder of 0.6; and, when a pack is
/// about to be deleted, the lines' listings and their trash folders' once
/// more (plus a read per record that wasn't there the first time). Finding
/// a version that misses a pack costs nothing more; only then are its
/// manifest and the ones that may hold its content read, and the mark
/// written. Meant to run at most about once a day per game, after an upload
/// or in the background, never before playing.
///
/// An error part way leaves the cloud consistent: every step only removes
/// what the steps before made unused. Run again, it carries on.
pub fn clean(
    store: &dyn Store,
    lines: &mut [Open<'_>],
    policy: &retention::Policy,
    grace: chrono::Duration,
    now: DateTime<Utc>,
    device_id: &str,
) -> Result<Cleaned> {
    clean_with(store, lines, policy, grace, now, device_id, None)
}

/// [`clean`], putting back what a version misses from `repairs` where that
/// is found (step 4).
pub fn clean_with(
    store: &dyn Store,
    lines: &mut [Open<'_>],
    policy: &retention::Policy,
    grace: chrono::Duration,
    now: DateTime<Utc>,
    device_id: &str,
    repairs: Option<Repairs<'_>>,
) -> Result<Cleaned> {
    let mut cleaned = Cleaned::default();
    // The packs of whatever is left, and whether something couldn't be read,
    // in which case no pack is known to be unused.
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut blind = false;
    let mut needs_index = vec![false; lines.len()];

    // 1. What retention lets go.
    let mut views = Vec::with_capacity(lines.len());
    for (n, l) in lines.iter_mut().enumerate() {
        let place = l.place;
        let view = open(store, place, &mut *l.cache)?;
        needs_index[n] = view.needs_index;
        let mut gone = BTreeSet::new();
        if view.unreadable.is_empty() {
            for id in retention::plan(&view, &l.bases, policy, now).delete {
                let Some(dir) = place.dir_of(&view, &id) else { continue };
                if v2::legacy_key(dir).is_some() {
                    continue;
                }
                // Kept on another device since the listing.
                let mut pinned = store.read(&v2::pin_path(place.dir(), &id))?.is_some();
                if !pinned && dir != place.dir() {
                    pinned = store.read(&v2::pin_path(dir, &id))?.is_some();
                }
                if pinned {
                    continue;
                }
                store.remove(&v2::manifest_path(dir, &id))?;
                if view.get(&id).is_some_and(IndexEntry::is_incomplete) {
                    line::remove_mark(store, place.dir(), &id)?;
                }
                l.cache.snapshots.remove(&id);
                cleaned.snapshots.push(id.clone());
                gone.insert(id);
            }
        } else {
            blind = true;
        }
        used.extend(view.snapshots.iter().filter(|s| !gone.contains(&s.id)).flat_map(|s| s.packs.iter().cloned()));
        views.push((view, gone));
    }

    // 2. "Recently deleted" that has been there long enough.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (l, (view, _)) in lines.iter().zip(&views) {
        for dir in &l.place.dirs {
            let legacy = v2::legacy_key(dir).is_some();
            for (id, record) in read_trash(store, dir, &mut seen)? {
                let Some(record) = record else {
                    blind = true;
                    continue;
                };
                let expired = (now - record.deleted_at).num_days() >= super::TRASH_DAYS;
                if expired && view.get(&id).is_none() && purge_at(store, dir, &id, legacy)? {
                    cleaned.purged.push(id);
                } else {
                    used.extend(record.meta.packs());
                }
            }
        }
    }

    // 3. Packs nothing uses.
    let mut folders: Vec<String> = Vec::new();
    for l in lines.iter() {
        for f in &l.place.folders {
            if !folders.contains(f) {
                folders.push(f.clone());
            }
        }
    }
    let mut candidates: Vec<(String, String)> = Vec::new();
    // Every pack the game has, for step 4.
    let mut present: BTreeSet<String> = BTreeSet::new();
    for folder in &folders {
        let listing = store.list(&v2::packs_dir(folder))?;
        present.extend(listing.iter().filter(|e| !e.is_dir).filter_map(|e| e.name.strip_suffix(".zip")).map(str::to_string));
        candidates.extend(packs::unused(&listing, &used, now, grace).into_iter().map(|p| (folder.clone(), p)));
    }
    if !candidates.is_empty() && !blind {
        // Listed once more: what was written since the first listing keeps
        // its packs (section 3.4, rule 2).
        for (n, l) in lines.iter_mut().enumerate() {
            let view = open(store, l.place, &mut *l.cache)?;
            needs_index[n] |= view.needs_index;
            blind |= !view.unreadable.is_empty();
            used.extend(view.snapshots.iter().flat_map(|s| s.packs.iter().cloned()));
            for dir in &l.place.dirs {
                for (_, record) in read_trash(store, dir, &mut seen)? {
                    match record {
                        Some(record) => used.extend(record.meta.packs()),
                        None => blind = true,
                    }
                }
            }
        }
        if !blind {
            for (folder, pack) in candidates.into_iter().filter(|(_, p)| !used.contains(p)) {
                store.remove(&v2::pack_path(&folder, &pack))?;
                present.remove(&pack);
                cleaned.packs.push(pack);
            }
        }
    }

    // 4. Versions that miss a pack.
    let mut broken: Vec<(usize, String, bool)> = Vec::new();
    for (n, (l, (view, gone))) in lines.iter().zip(&views).enumerate() {
        for s in view.snapshots.iter().filter(|s| !gone.contains(&s.id) && s.unsupported().is_empty()) {
            // A snapshot of 0.6 keeps its zip beside its manifest.
            if l.place.dir_of(view, &s.id).is_none_or(|d| v2::legacy_key(d).is_some()) {
                continue;
            }
            let lacking = s.packs.iter().any(|p| !present.contains(p));
            if lacking || s.is_incomplete() {
                broken.push((n, s.id.clone(), lacking));
            }
        }
    }
    if !broken.is_empty() {
        let refs: Vec<(&Place, &LineView)> = lines.iter().map(|l| l.place).zip(views.iter().map(|(v, _)| v)).collect();
        let mut local: Option<Vec<LocalFile>> = None;
        // Those nobody could make whole, and whether their time is up.
        let mut lost: Vec<(usize, String, bool)> = Vec::new();
        for (n, id, lacking) in broken {
            let (place, view) = refs[n];
            let Some(entry) = view.get(&id) else { continue };
            let marked = entry.incomplete.clone();
            let outcome = match repairs {
                Some(r) => match manifest_of(store, place, view, &id)? {
                    Some(m) => {
                        let local = local.get_or_insert_with(|| (r.local)());
                        put_back(store, &refs, place, &m, Some(&present), Sources { local, zips: r.zips }, r.staging, device_id, now)?
                    }
                    None => continue,
                },
                None if lacking => Repair::Lost(entry.packs.iter().filter(|p| !present.contains(*p)).cloned().collect()),
                // Marked, and its packs are listed: whether they are whole
                // takes a look this clean-up can't take.
                None => continue,
            };
            if writable(place).is_err() {
                continue;
            }
            let cache = &mut *lines[n].cache;
            match outcome {
                Repair::Complete | Repair::Repaired(_) => {
                    if marked.is_some() {
                        unmark(store, place, cache, &id)?;
                    }
                    cleaned.repaired.push(id);
                }
                Repair::Lost(missing) => {
                    if marked.is_none() {
                        mark(store, place, cache, &id, &missing, device_id, now)?;
                        cleaned.marked.push(id.clone());
                    }
                    let since = marked.and_then(|m| m.since).unwrap_or(now);
                    // Its writer had its chance when it is this device.
                    let due = (now - since).num_days() >= INCOMPLETE_DAYS || entry.device_id == device_id;
                    lost.push((n, id, due));
                }
            }
        }
        let lost_ids: BTreeSet<String> = lost.iter().map(|(_, id, _)| id.clone()).collect();
        let by = repairs.map_or(device_id, |r| r.device_name);
        for (n, id, due) in lost {
            let (place, view) = refs[n];
            let others_lost: BTreeSet<String> = lost_ids.iter().filter(|x| **x != id && !cleaned.retired.contains(x)).cloned().collect();
            if due && retirable(place, view, &id, &others_lost, &cleaned.retired) {
                retire(store, place, view, &id, by, now)?;
                lines[n].cache.snapshots.remove(&id);
                cleaned.retired.push(id);
            }
        }
    }

    // 5. The indexes that need it.
    for (n, l) in lines.iter().enumerate() {
        if needs_index[n] && v2::legacy_key(l.place.dir()).is_none() {
            line::write_index(store, l.place.dir(), &*l.cache, device_id)?;
        }
    }
    Ok(cleaned)
}
