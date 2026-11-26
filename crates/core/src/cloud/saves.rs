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
//!   manifest back exactly as 0.6 would;
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
//! | [`publish`], some files taken from older packs | the same + 1 list |
//! | [`manifest`] | 1 read |
//! | [`fetch`] | 1 download per pack that holds a wanted file |
//! | [`keep`] | 1 write; renaming a kept save 1 read + 1 write |
//! | [`unkeep`] | 1 remove per folder of the line |
//! | [`trash`] | 1 read + 1 write + 1 remove |
//! | [`trashed`] | 1 list per folder of the line + 1 read per version in it |
//! | [`untrash`] | 1 read + 1 write + 1 remove (+ 1 list in a folder of 0.6) |
//! | [`purge`] | 1 read + 1 remove (+ 1 remove in a folder of 0.6: the zip) |
//! | [`clean`] | see there |
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
//!   be gone by the time it is uploaded.

// Contract only: the functions below are still to be written. Remove this
// line with the last `todo!()`.
#![allow(unused_variables, dead_code, unused_imports)]

use super::line::{LineCache, LineView};
use super::packs::{self, LocalFile};
use super::retention::{self, DeviceBase};
use super::store::Store;
use super::v2::{self, GameRecord, IndexEntry, Info, Line, Manifest, Origin};
use crate::vv::Vv;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;

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
        todo!()
    }

    /// The line of a game of 0.6 in a cloud that has not been migrated yet:
    /// exactly what the migration will make of the key ([`super::upgrade`]),
    /// so reading works before it has run. Game and folder are `key`, the
    /// line is `main` in `games/<key>/saves/main`, and `games/<key>/snapshots`
    /// is read as well.
    pub fn of_key(key: &str) -> Place {
        todo!()
    }

    /// Where the line is written to.
    pub fn dir(&self) -> &str {
        &self.dirs[0]
    }

    /// The folder that holds the manifest of `id` in `view` (a view of this
    /// place).
    pub fn dir_of(&self, view: &LineView, id: &str) -> Option<&str> {
        todo!()
    }
}

/// Read the line: [`super::line::load_all`] over the place's folders.
pub fn open(store: &dyn Store, place: &Place, cache: &mut LineCache) -> Result<LineView> {
    todo!()
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
    todo!()
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
    todo!()
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
    todo!()
}

/// The snapshots waiting in `dir`, oldest first. A missing folder is empty;
/// a `.json` that can't be understood is skipped.
pub fn staged(dir: &Path) -> Result<Vec<Manifest>> {
    todo!()
}

/// Remove a snapshot's files from `dir` (after [`publish`], or to drop it).
pub fn unstage(dir: &Path, manifest: &Manifest) -> Result<()> {
    todo!()
}

/// Upload a staged snapshot and make it part of the line.
///
/// In this order, each step only when the one before succeeded:
///
/// 1. upload the snapshot's own packs from `dir` into the game's first
///    folder, `<id>` first, then `<id>-2`, ...;
/// 2. write the manifest into [`Place::dir`]: from here on the snapshot
///    exists;
/// 3. [`packs::verify`] with `files`: a pack this manifest borrows that
///    another device cleaned up meanwhile is put back. `files` are the save
///    files as they are now, for that case only; pass none when the saves
///    may have changed since (an upload that waited): a snapshot staged
///    without a base borrows nothing;
/// 4. add the manifest to `cache` and write the index.
///
/// An error leaves nothing to undo: packs without a manifest are not a
/// snapshot and are cleaned up later ([`clean`]); a manifest without its
/// index entry is found by the next listing. Calling it again for the same
/// snapshot is safe and ends in the same state (the manifest is written
/// with the same bytes). It doesn't remove anything from `dir`.
///
/// Nothing is ever written into a folder of 0.6.
pub fn publish(
    store: &dyn Store,
    place: &Place,
    cache: &mut LineCache,
    manifest: &Manifest,
    dir: &Path,
    files: &[LocalFile],
    device_id: &str,
) -> Result<()> {
    todo!()
}

// ---------------------------------------------------------------------- fetch

/// The whole manifest of a snapshot of `view`, from the folder that holds
/// it. An error when the view doesn't have it or it has been deleted since.
pub fn manifest(store: &dyn Store, place: &Place, view: &LineView, id: &str) -> Result<Manifest> {
    todo!()
}

/// Fetch files of a snapshot into `dest` (`<dest>/<canonical path>`): all,
/// or the paths in `only`. [`packs::read`] with the game's folders and the
/// folder the manifest is in, so `dest` is as it was after any error, and a
/// pack that is gone is a [`packs::MissingPack`]. Putting the files where
/// the game looks for them is the caller's business.
pub fn fetch(
    store: &dyn Store,
    place: &Place,
    view: &LineView,
    manifest: &Manifest,
    only: Option<&[String]>,
    dest: &Path,
    staging: &Path,
) -> Result<()> {
    todo!()
}

// ----------------------------------------------------------------- kept saves

/// Keep a snapshot, or rename a kept one (section 3.7): write
/// `<id>.pin.json` into [`Place::dir`], wherever the manifest is. For a
/// snapshot `view` shows with a pin, the pin is read first and only its name
/// changes (its note, time, device and unknown fields stay); otherwise a new
/// pin is written with `now` and `device_id`. An error when the view doesn't
/// have the snapshot.
pub fn keep(store: &dyn Store, place: &Place, view: &LineView, id: &str, name: &str, device_id: &str, now: DateTime<Utc>) -> Result<()> {
    todo!()
}

/// Stop keeping a snapshot: remove its pin from every folder of the line.
/// It is an automatic snapshot again and goes the way [`clean`] says.
pub fn unkeep(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    todo!()
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
}

/// Delete a version the way the user does: into "recently deleted".
///
/// Refused, with a message for the user, for a head of the line (the other
/// devices would be asked about a version that is gone) and for a kept save
/// (stop keeping it first). An error when the view doesn't have it.
///
/// The record first (`{deletedAt, deletedBy, meta}` in
/// [`v2::trash_path`] of the folder the manifest is in, `meta` being the
/// manifest exactly as it is in the cloud, unknown fields included), then
/// the manifest is removed: an interruption leaves the version in the
/// history rather than nowhere. Its packs stay where they are.
pub fn trash(store: &dyn Store, place: &Place, view: &LineView, id: &str, by: &str, now: DateTime<Utc>) -> Result<()> {
    todo!()
}

/// What is in "recently deleted" for this line, newest deletion first.
///
/// A record whose manifest is in the line (again) is not listed: the
/// deletion was interrupted, or the version was put back meanwhile. In a
/// folder of 0.6, a record whose zip is gone was half purged: it is removed
/// and not listed, as 0.6 does. Records that can't be understood are
/// skipped.
pub fn trashed(store: &dyn Store, place: &Place) -> Result<Vec<Trashed>> {
    todo!()
}

/// Put a deleted version back into the history: the manifest is written
/// back where it was, byte for byte what the record holds, then the record
/// is removed. An error for the user when it is no longer there (purged
/// meanwhile; in a folder of 0.6 also when its zip is gone).
///
/// The version is an automatic snapshot again. A caller that doesn't want
/// [`clean`] to take it away the next day keeps it ([`keep`]).
pub fn untrash(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    todo!()
}

/// Delete a version in "recently deleted" for good. Refused when its
/// manifest is back in the line (another device restored it).
///
/// The record is removed; in a folder of 0.6 the snapshot's zip first, as
/// 0.6 does. Packs of layout 2 are not touched here: whether another
/// snapshot still uses one takes all the game's lines to tell, which is
/// [`clean`]'s work. They are gone after the next [`clean`] that finds them
/// unused and older than a day.
pub fn purge(store: &dyn Store, place: &Place, id: &str) -> Result<()> {
    todo!()
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
}

/// Clean up one game: `lines` are all its save lines (packs belong to the
/// game, so whether one is still used takes every line to tell).
///
/// 1. Each line is read again ([`open`]) and [`retention::plan`] is asked.
///    The snapshots it names are deleted (their manifests; pins of deleted
///    snapshots too), except those in a folder of 0.6, which are never
///    touched. Nothing is deleted from a line with manifests this build
///    can't read ([`LineView::unreadable`]): what it can't read might be the
///    newest version.
/// 2. Versions deleted by the user more than [`super::TRASH_DAYS`] days
///    before `now` are purged ([`purge`]), in folders of 0.6 as well: it is
///    the rule 0.6 has.
/// 3. Packs: the packs still used are those of every snapshot left in every
///    line and of every version in "recently deleted". Each `packs/` folder
///    of the game is listed and [`packs::unused`] names candidates (older
///    than `grace`). If there are any, every line and its trash are listed
///    once more, and only packs still unused then are deleted: a device that
///    wrote a manifest in between keeps its packs (section 3.4, rule 2).
/// 4. The index of a line is written again only if reading it said so
///    ([`LineView::needs_index`]); entries for manifests that are gone do no
///    harm and disappear with the next upload.
///
/// Requests: per line its listings and one listing per trash folder (plus a
/// read per record in it); per game folder one listing of `packs/`; one
/// remove per file deleted; and the lines' listings once more when a pack
/// is about to be deleted. Meant to run at most about once a day per game,
/// after an upload or in the background, never before playing.
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
    todo!()
}
