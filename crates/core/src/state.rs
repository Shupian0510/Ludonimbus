//! Per-device sync state (`%APPDATA%\Ludonimbus\state.json`).

use crate::{lockfile::FileLock, paths, util};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub games: BTreeMap<String, GameState>,
    /// Last time this device refreshed its entry in the cloud.
    pub device_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameState {
    /// Snapshot this device last synced to (uploaded or pulled).
    pub last_synced_id: Option<String>,
    /// Content hash of the local saves at that moment.
    pub last_synced_hash: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_synced_device: Option<String>,
    pub conflict: Option<ConflictInfo>,
    pub last_error: Option<String>,
    /// Snapshots waiting in `pending/` for the cloud to come back.
    pub pending: Vec<String>,
    pub cloud_def_uploaded: bool,
    pub last_check: Option<serde_json::Value>,
    /// Artwork kind → hash of the image last uploaded / downloaded by
    /// [`crate::artsync`].
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub art_pushed: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub art_pulled: std::collections::BTreeMap<String, String>,
    /// A newer save version of another version of the game the user chose
    /// not to apply (see [`crate::savelib::newer_version`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newer_dismissed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub cloud_id: String,
    pub cloud_device: String,
    pub cloud_created_at: DateTime<Utc>,
    pub local_hash: String,
    pub detected_at: DateTime<Utc>,
}

fn lock() -> Result<FileLock> {
    FileLock::acquire(&paths::data_dir().join("state.lock"), Duration::from_secs(10))
}

fn read() -> State {
    std::fs::read_to_string(paths::state_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn load() -> State {
    read()
}

pub fn game(key: &str) -> GameState {
    read().games.get(key).cloned().unwrap_or_default()
}

pub fn update<T>(f: impl FnOnce(&mut State) -> T) -> Result<T> {
    let _guard = lock()?;
    let mut s = read();
    let out = f(&mut s);
    util::atomic_write(&paths::state_file(), &serde_json::to_vec_pretty(&s)?)?;
    Ok(out)
}

pub fn update_game<T>(key: &str, f: impl FnOnce(&mut GameState) -> T) -> Result<T> {
    update(|s| f(s.games.entry(key.to_string()).or_default()))
}
