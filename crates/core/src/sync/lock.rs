//! One sync of a game at a time on this device, across processes
//! ([`lock_game`]), and the sync after playing that the launch wrapper
//! didn't wait for: noted as owed ([`owe`]) and done by whoever syncs the
//! game next, or by the tray app ([`finish_owed`]).

use super::gate::{Gate, GaveUp};
use super::queue::{queue_dir, queued};
use super::run::sync_with;
use super::{log, Ctx, Outcome, Resolution};
use crate::lockfile::FileLock;
use crate::snapshot::Trigger;
use crate::{paths, util};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;

thread_local! {
    /// Games whose lock this thread holds: an operation that holds it may
    /// run another one (choosing a line runs a sync).
    static HELD: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

fn lock_file(key: &str) -> PathBuf {
    paths::locks_dir().join(format!("sync-{key}.lock"))
}

/// The right to read, decide about and write a game's save line on this
/// device, until dropped ([`lock_game`]).
pub struct GameLock {
    key: Option<String>,
    _file: Option<FileLock>,
}

impl Drop for GameLock {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            let _ = HELD.try_with(|h| h.borrow_mut().remove(key));
        }
    }
}

/// One sync of a game at a time on this device: the launch wrapper, the
/// process that finishes its upload, the tray app and the command line all
/// take this lock first. Waits for whoever has it, however long that takes,
/// unless `gate` is given up meanwhile (an error that ends a sync as
/// [`Outcome::Offline`]). A holder that is gone doesn't keep it
/// ([`FileLock::hold`]).
pub fn lock_game(key: &str, gate: &Gate) -> Result<GameLock> {
    if HELD.with(|h| h.borrow().contains(key)) {
        return Ok(GameLock { key: None, _file: None });
    }
    let mut told = false;
    let file = FileLock::hold(&lock_file(key), &mut || {
        if !told {
            log(format!("{key}: waiting for another sync of this game to finish"));
            told = true;
        }
        !gate.given_up()
    })?;
    let Some(file) = file else { return Err(GaveUp.into()) };
    HELD.with(|h| h.borrow_mut().insert(key.to_string()));
    Ok(GameLock { key: Some(key.to_string()), _file: Some(file) })
}

/// Whether a sync of `key` runs on this device right now, in any process.
pub fn sync_running(key: &str) -> bool {
    FileLock::is_held(&lock_file(key))
}

// ---------------------------------------------------- owed after playing

fn owed_file(key: &str) -> PathBuf {
    paths::owed_dir().join(format!("{key}.json"))
}

/// Note that the sync of `key` after playing is still owed: the launch
/// wrapper stopped waiting for it and exits. Whoever syncs the game next
/// with a final outcome settles it; [`finish_owed`] is there for just that.
pub fn owe(key: &str) -> Result<()> {
    let note = serde_json::json!({ "since": Utc::now(), "pid": std::process::id() });
    util::atomic_write(&owed_file(key), note.to_string().as_bytes())
}

/// [`owe`] for a sync in the background ([`Trigger::Passive`]) that
/// couldn't be done now: the tray app noticed a game end while the game
/// was being synced, or a terminated wrapper's session couldn't be
/// finished. Done later as such a sync, which only uploads for most games
/// (`run`'s `only_uploads`). A sync the launch wrapper owes already stays
/// as it is.
pub fn owe_passive(key: &str) -> Result<()> {
    if is_owed(key) {
        return Ok(());
    }
    let note = serde_json::json!({ "since": Utc::now(), "pid": std::process::id(), "passive": true });
    util::atomic_write(&owed_file(key), note.to_string().as_bytes())
}

/// Whether the sync `key` owes is one in the background ([`owe_passive`]).
fn owed_passive(key: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(owed_file(key)) else { return false };
    serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| v["passive"].as_bool() == Some(true))
}

pub fn is_owed(key: &str) -> bool {
    owed_file(key).is_file()
}

/// The games whose sync after playing is still owed.
pub fn owed_keys() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(paths::owed_dir()) else { return Vec::new() };
    let mut keys: Vec<String> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    keys.sort();
    keys
}

/// What a game still owes the cloud after playing, done in the background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlog {
    pub state: BacklogState,
    /// Versions waiting on this device.
    pub versions: usize,
    /// How much there is to upload for them.
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BacklogState {
    /// A sync of the game runs right now, in any process.
    Uploading,
    /// Nobody is at it: the cloud couldn't be reached, and it is tried again later.
    Waiting,
}

/// What `key` still owes the cloud: versions that wait in the queue, or a
/// sync after playing a wrapper handed over ([`owe`]). `None` when nothing.
pub fn backlog(key: &str) -> Option<Backlog> {
    let versions = queued(key).len();
    if versions == 0 && !is_owed(key) {
        return None;
    }
    let bytes = std::fs::read_dir(queue_dir(key))
        .map(|rd| rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "zip")).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0);
    let state = if sync_running(key) { BacklogState::Uploading } else { BacklogState::Waiting };
    Some(Backlog { state, versions, bytes })
}

pub(super) fn settle_owed(key: &str) {
    if std::fs::remove_file(owed_file(key)).is_ok() {
        log(format!("{key}: the sync after playing is done"));
    }
}

/// Do the sync of `key` after playing that is still owed ([`owe`]): the
/// usual sync, as right after the game (as the background's, for one the
/// background owes: [`owe_passive`]). `None` when nothing is owed (any
/// more). The note goes once the outcome is final ([`Outcome::is_final`]),
/// and stays while the cloud can't be reached. A conflict is only noted:
/// the next launch of the game asks.
pub fn finish_owed(ctx: &Ctx, key: &str) -> Result<Option<Outcome>> {
    let gate = Gate::default();
    let _lock = lock_game(key, &gate)?;
    // Whoever had the lock may have done it meanwhile.
    if !is_owed(key) {
        return Ok(None);
    }
    // Being played again: no sync may change its saves now, and the one
    // after this session settles it.
    if matches!(crate::catchup::runner_phase(key).as_deref(), Some("preLaunch" | "playing")) {
        log(format!("{key}: being played again; the sync after this session does what was owed"));
        return Ok(None);
    }
    let Some(game) = ctx.cfg.game(key).filter(|g| g.auto_sync).cloned() else {
        // The game is gone from this device, or doesn't sync by itself now.
        settle_owed(key);
        return Ok(None);
    };
    let trigger = if owed_passive(key) { Trigger::Passive } else { Trigger::AutoExit };
    sync_with(ctx, &game, Resolution::Auto, trigger, &gate).map(Some)
}
