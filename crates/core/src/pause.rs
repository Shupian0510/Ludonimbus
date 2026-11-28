//! Syncing paused on this device: from the Steam Deck's quick access menu
//! (the Decky plugin), the local API, or the app. While it is paused this
//! device sends nothing to the cloud and takes nothing from it.
//!
//! - No sync runs in the background: not after a game that ran outside the
//!   launch wrapper, not the backlog of uploads, not screenshots, not the
//!   Steam games kept up to date without the wrapper.
//! - The launch wrapper starts the game at once, without the sync before
//!   it. After playing, the saves are kept as a version in this device's
//!   upload queue, as when the cloud can't be reached (see [`crate::sync`]),
//!   and go up once syncing goes on. 「保留这一份」 (keep this one) queues
//!   its version the same way.
//! - What the user asks for that needs the cloud (sync now, restore, taking
//!   the cloud's saves in a conflict) is refused with [`Paused`] as the
//!   reason.
//!
//! **Resuming.** Whoever serves the local API (the desktop app or
//! `ludonimbus-agent`) then uploads what waits in the queue and finishes the
//! syncs after playing that are owed, one game at a time, in the background,
//! as when the cloud is back after being away. Versions queued while paused
//! go up in the order they were made, each built on the one before. Nothing
//! is pulled on resume: when another device moved on meanwhile, its newer
//! saves stay in the cloud for the user to take (or for the next launch
//! through Steam to bring), and saves here that went another way are a
//! conflict the user settles, as after playing offline.
//!
//! Kept in `paused.json` in the data folder, so that the launch wrapper, the
//! agent and the app (separate programs) agree, and it lasts across
//! restarts until it is resumed.

use crate::paths;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Syncing is paused on this device, since `since`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Pause {
    pub since: DateTime<Utc>,
}

/// Why something that needs the cloud wasn't done: syncing is paused.
#[derive(Debug, Clone, Copy)]
pub struct Paused;

impl std::fmt::Display for Paused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::tr!("同步已暂停", "Syncing is paused"))
    }
}

impl std::error::Error for Paused {}

pub fn file() -> PathBuf {
    paths::data_dir().join("paused.json")
}

/// The pause, when syncing is paused. A file that can't be read counts as
/// paused since it was written: whoever wrote it meant to pause.
pub fn get() -> Option<Pause> {
    let bytes = std::fs::read(file()).ok()?;
    serde_json::from_slice(&bytes).ok().or_else(|| {
        let since = std::fs::metadata(file()).and_then(|m| m.modified()).map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now());
        Some(Pause { since })
    })
}

pub fn is_paused() -> bool {
    file().exists()
}

/// Pause syncing (again: the first time stays).
pub fn pause() -> Result<Pause> {
    if let Some(p) = get() {
        return Ok(p);
    }
    let p = Pause { since: Utc::now() };
    std::fs::create_dir_all(paths::data_dir())?;
    crate::util::atomic_write(&file(), &serde_json::to_vec(&p)?)?;
    crate::logger::log("sync", "syncing paused on this device");
    Ok(p)
}

/// Go on syncing. `true` when it was paused.
pub fn resume() -> Result<bool> {
    match std::fs::remove_file(file()) {
        Ok(()) => {
            crate::logger::log("sync", "syncing resumed on this device");
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// `Err(Paused)` while syncing is paused.
pub fn check() -> Result<()> {
    if is_paused() {
        return Err(Paused.into());
    }
    Ok(())
}

/// Whether an error is [`Paused`].
pub fn is_paused_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Paused>().is_some())
}
