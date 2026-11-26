//! Bringing a cloud written by 0.6 (layout 1) to layout 2
//! (docs/plan-v2.md, section 12). Done once per cloud, by the first device
//! that runs a build with layout 2 and wants to write.
//!
//! (Copying from one cloud to another when the user switches clouds is
//! [`super::migrate`]; this is not that.)
//!
//! **No file of 0.6 is moved, changed or deleted.** The upgrade only adds:
//! game records, pins, indexes and, last of all, `format.json`. So going
//! back is deleting what was added (section 12.4), and a device still on
//! 0.6 keeps working in its own folders.
//!
//! # What is added
//!
//! For every folder `games/<key>/` that has a `game.json` 0.6 can read or at
//! least one snapshot in `snapshots/`:
//!
//! 1. **A game record** `library/<key>.json`:
//!    - `id` and the one entry of `folders`: the key. `name`: from
//!      `game.json`, the key when there is none.
//!    - `createdAt`: when the oldest snapshot in the folder was made; none
//!      for a folder without snapshots. Every device that does the upgrade
//!      must write the same record, so nothing in it comes from this device
//!      or from the clock (`updatedAt` and `updatedBy`, which
//!      [`library::save`] sets, are the exception).
//!    - one line: `id` `main`, shared, `path` `games/<key>/saves/main`,
//!      `also` `[games/<key>/snapshots]` ([`v2::legacy_dir`]), `format` from
//!      the save rules of the game ([`line_format`]).
//!    - `ids` from the game's definition ([`record_ids`]), `rule` when a
//!      cross-platform rule covers it.
//!
//!    The record is saved with [`library::save`], so one that is there
//!    already (an upgrade that broke off, another device at the same time)
//!    is merged with, not overwritten.
//! 2. **One card per game**: records that share a strong id become one
//!    ([`library::dedupe_with`]; survivor written first). Their save lines
//!    are *not* joined: each key keeps its own history, as in 0.6, under the
//!    one card. Whether two versions save the same files is not something
//!    the upgrade can know; joining lines comes with bindings and the
//!    question that goes with them (section 6).
//! 3. **Pins**: a snapshot of the old folder that is locked, or was made by
//!    hand (`manual`), gets `<id>.pin.json` in the line's new folder: `name`
//!    is its note (empty when it has none), `createdAt` and `deviceId` are
//!    the snapshot's. Skipped when the listing shows a pin for it already.
//!    From here on the pin says "kept"; the `locked` in the old file is
//!    history.
//! 4. **The index** of each line, in its new folder, covering the old
//!    folder's snapshots and the pins. A device that reads the line
//!    afterwards with nothing cached makes its two listings and one read.
//! 5. **`format.json`**, when everything else is there:
//!    `{layout: 2, minApp: "1.0.0", requires: [], migratedAt, migratedBy}`.
//!
//! A cloud with no games at all gets only step 5: that is how a new cloud
//! becomes one of layout 2.
//!
//! # Doing it twice, or two at once
//!
//! Every step gives the same result when it is done again, by the same
//! device or by another: an upgrade that broke off is simply run again, and
//! two devices that both run it leave what one would have left. The lock
//! (`upgrade.lock.json` in the cloud's root, taken and released like the one
//! of [`crate::gamebackup::remote::Lock`], refreshed while it runs) only
//! saves the second device the work; nothing depends on it holding.
//!
//! # Requests
//!
//! | | requests |
//! |---|---|
//! | a cloud that is of layout 2 already | 1 read |
//! | per game | 1 list of `games/<key>/snapshots` and of the new folder, 1 read of `game.json`, 1 read per snapshot not cached on this device, 1 read + 1 write for its record, 1 write per pin, 1 list + 1 write for the index |
//! | once | 1 read of `format.json`, the lock, 1 list of `games/`, 1 write of `format.json` |
//!
//! Snapshots are read through the store the caller passes: with
//! [`super::cached::Cached`] around it, a device that used 0.6 has them
//! already.

// Contract only: the functions below are still to be written. Remove this
// line with the last `todo!()`.
#![allow(unused_variables, dead_code, unused_imports)]

use super::library;
use super::store::Store;
use super::v2::{self, Access, GameRecord};
use super::CloudGame;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

/// The first version that writes layout 2.
pub const MIN_APP: &str = "1.0.0";
pub const LOCK_FILE: &str = "upgrade.lock.json";

/// How far the upgrade is, for a progress bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Name of the game being worked on.
    pub game: String,
    pub done: usize,
    pub total: usize,
}

/// What an upgrade did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The cloud was of layout 2 already; nothing was done.
    pub already: bool,
    /// Game records written (one per key).
    pub games: usize,
    /// Records that turned out to be a game another record stands for.
    pub merged: usize,
    /// Snapshots of 0.6 now reachable through a line.
    pub snapshots: usize,
    /// Pins written for locked snapshots and manual backups.
    pub pins: usize,
}

/// The ids a game record gets from a game as 0.6 defines it.
///
/// From what `savelib::cloud_ids` works out for it:
///
/// | 0.6 | record |
/// |---|---|
/// | `nx:…`, `ludusavi:…`, `xbox:…` | the same |
/// | `fmt:<rule>` (a cross-platform rule) | `rule:<rule>` |
/// | `name:<scope>:<key>` | dropped; [`library::name_id`] of the game's name instead (a weak id: a suggestion, never a merge) |
///
/// and from the definition itself:
///
/// - a built-in rule for one game (`adapter` that is not an emulator's):
///   `adapter:<adapter key>`. Two keys with the same such rule are the same
///   game.
/// - the link a development build let the user set (`game`): `link:<value>`
///   for entries linked together; an entry the user kept apart (`solo:…`)
///   gets no strong id at all, only its name, so nothing merges it.
pub fn record_ids(game: &CloudGame) -> Vec<String> {
    todo!()
}

/// The format of the line a game of 0.6 gets, from the scope its saves have
/// there (`savelib::save_scope`): `pc` → `pc`; `switch` → `nx`; anything
/// else (a built-in rule, an emulator) → `adapter:<scope>`.
pub fn line_format(game: &CloudGame) -> String {
    todo!()
}

/// The record the upgrade writes for one key (before [`library::save`] and
/// before records are merged). `game` is `None` for a folder without a
/// `game.json`; `oldest` is when the oldest snapshot in its old folder was
/// made.
pub fn record_for(key: &str, game: Option<&CloudGame>, oldest: Option<DateTime<Utc>>) -> GameRecord {
    todo!()
}

/// Bring the cloud behind `store` to layout 2.
///
/// - Layout 2 already: `Report { already: true, .. }` after one read.
/// - Written by a newer Ludonimbus ([`Access::ReadOnly`]): an error that
///   tells the user to update; nothing is written.
/// - Another device holds the lock: an error that says which and since
///   when; nothing is written.
/// - Otherwise the steps of the module documentation, `format.json` last.
///   An error on the way (no connection) leaves a cloud that is still of
///   layout 1 and that the next run finishes.
///
/// `now` is the caller's clock (`migratedAt`, the lock). `progress` is
/// called before each game.
pub fn run(
    store: &dyn Store,
    device_id: &str,
    device_name: &str,
    now: DateTime<Utc>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Report> {
    todo!()
}
