//! Per-device sync state (`%APPDATA%\Ludonimbus\state.json`).

use crate::cloud::decide::Base;
use crate::vv::Vv;
use crate::{lockfile::FileLock, paths, util};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub games: BTreeMap<String, GameState>,
    /// Last time this device refreshed its entry in the cloud.
    pub device_seen_at: Option<DateTime<Utc>>,
    /// What this device knows about the cloud it is connected to. Forgotten
    /// when another cloud is connected.
    #[serde(skip_serializing_if = "CloudSeen::is_unknown")]
    pub cloud: CloudSeen,
}

/// The layout of the connected cloud as this device last saw it, so that a
/// sync doesn't have to read `format.json` every time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudSeen {
    /// 0 = never looked; 1 = the layout of 0.6, not upgraded yet; 2 = the
    /// current layout.
    pub layout: u32,
    /// A newer Ludonimbus wrote it: this build must not write.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// When `format.json` was read last.
    pub checked_at: Option<DateTime<Utc>>,
}

impl CloudSeen {
    fn is_unknown(&self) -> bool {
        self.layout == 0 && self.checked_at.is_none()
    }
}

/// Where a game's save line is in the cloud, as worked out from its game
/// record: remembered so that a check before playing needs no look at the
/// library. The folders of a line never change once it exists; a line can
/// only gain folders to read, which the refresh after playing picks up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LineRef {
    /// Id of the game record.
    pub game: String,
    pub line: String,
    /// The line the game on this device asked for when this was worked out
    /// (empty: the shared one). It can differ from `line`, which is the
    /// line's id on the card it is on now.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub asked: String,
    /// The game's folders under `games/`.
    pub folders: Vec<String>,
    /// The line's folders: the one written to first.
    pub dirs: Vec<String>,
    /// When the game record was looked at last.
    pub checked_at: Option<DateTime<Utc>>,
}

impl LineRef {
    /// The folder the line is written to. It never changes, so it is what
    /// names the line across devices whatever card it is on.
    pub fn dir(&self) -> &str {
        self.dirs.first().map(String::as_str).unwrap_or_default()
    }
}

/// The last time saves of the cloud replaced the ones on this device, so
/// it can be taken back ([`crate::sync::undo`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Replaced {
    /// The version that holds what was here.
    pub back: String,
    /// The version the device is at since. Once it is at another one
    /// (it saved on top), there is nothing to take back any more.
    pub now: String,
    /// Whose saves they were replaced with.
    pub device: String,
    pub at: DateTime<Utc>,
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
    /// Version vector of that snapshot. Empty in a state written by 0.6: the
    /// save line is then asked, by id.
    #[serde(skip_serializing_if = "Vv::is_empty")]
    pub last_synced_vv: Vv,
    /// The highest count this device used for itself on the game's save
    /// line. Lost counts are found again in the line's snapshots.
    #[serde(skip_serializing_if = "is_zero")]
    pub counter: u64,
    /// The game's save line in the cloud.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineRef>,
    /// When old versions and unused packs of this game were last cleaned up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleaned_at: Option<DateTime<Utc>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<Replaced>,
    /// Versions made here that couldn't be uploaded complete
    /// ([`crate::sync`]'s set-aside), until the user dismisses them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unsent: Vec<Unsent>,
}

/// A version made on this device that couldn't be uploaded complete: a pack
/// it took files from was gone from the cloud, and their content was nowhere
/// any more. What it brought itself is kept in local-backups; the game's
/// saves tab shows it once, until the user dismisses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "UnsentSave")]
pub struct Unsent {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub total_size: u64,
    #[serde(default)]
    pub file_count: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    /// When it was taken off the queue.
    pub at: DateTime<Utc>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl GameState {
    /// What this device synced last, for [`crate::cloud::decide`].
    pub fn base(&self) -> Option<Base> {
        Some(Base { id: self.last_synced_id.clone()?, vv: self.last_synced_vv.clone(), content_hash: self.last_synced_hash.clone().unwrap_or_default() })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    /// The newest of the versions to choose from.
    pub cloud_id: String,
    pub cloud_device: String,
    pub cloud_created_at: DateTime<Utc>,
    pub local_hash: String,
    pub detected_at: DateTime<Utc>,
    /// Every version to choose from, oldest first: one normally, several
    /// when the line itself has forked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// The saves on this device are one more choice.
    #[serde(default = "yes")]
    pub local_is_candidate: bool,
}

fn yes() -> bool {
    true
}

fn lock() -> Result<FileLock> {
    FileLock::acquire(&paths::data_dir().join("state.lock"), Duration::from_secs(10))
}

/// The state as the file has it. No file is an empty state. A file that
/// can't be read (another program kept it open longer than
/// [`util::read_retrying`] waits) is an error: an empty state written back
/// over it would forget every game's sync. One that isn't valid JSON is kept
/// beside it as `state.json.broken`, and the state starts empty.
fn try_read() -> Result<State> {
    let path = paths::state_file();
    match util::read_retrying(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            let _ = std::fs::copy(&path, path.with_extension("json.broken"));
            crate::logger::log("sync", format!("{} isn't valid ({e}); kept as state.json.broken, starting empty", path.display()));
            State::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(anyhow::Error::new(e).context(crate::tr!("无法读取 {}", "Can't read {}", path.display()))),
    }
}

/// [`try_read`] for looking only: an unreadable file looks empty.
fn read() -> State {
    try_read().unwrap_or_default()
}

pub fn load() -> State {
    read()
}

pub fn game(key: &str) -> GameState {
    read().games.get(key).cloned().unwrap_or_default()
}

pub fn update<T>(f: impl FnOnce(&mut State) -> T) -> Result<T> {
    let _guard = lock()?;
    let mut s = try_read()?;
    let out = f(&mut s);
    util::atomic_write(&paths::state_file(), &serde_json::to_vec_pretty(&s)?)?;
    Ok(out)
}

pub fn update_game<T>(key: &str, f: impl FnOnce(&mut GameState) -> T) -> Result<T> {
    update(|s| f(s.games.entry(key.to_string()).or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program that keeps the state file open (a virus scanner, the
    /// indexer) makes an update fail; it never writes an empty state over
    /// every game's sync.
    #[cfg(windows)]
    #[test]
    fn an_update_never_writes_over_a_state_it_could_not_read() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mut env = crate::paths::TestEnv::new();
        env.set("LUDONIMBUS_HOME", tmp.path());
        update_game("a", |g| g.last_synced_id = Some("01JA".into())).unwrap();
        let before = std::fs::read(paths::state_file()).unwrap();

        // Opened without sharing: nobody else can open it while it is held.
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(paths::state_file()).unwrap();
        assert!(update_game("b", |g| g.last_synced_id = Some("01JB".into())).is_err());
        drop(held);

        assert_eq!(std::fs::read(paths::state_file()).unwrap(), before);
        assert_eq!(game("a").last_synced_id.as_deref(), Some("01JA"));
        update_game("b", |g| g.last_synced_id = Some("01JB".into())).unwrap();
        assert_eq!(game("a").last_synced_id.as_deref(), Some("01JA"));
    }

    #[test]
    fn a_broken_state_file_is_kept_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = crate::paths::TestEnv::new();
        env.set("LUDONIMBUS_HOME", tmp.path());
        std::fs::create_dir_all(paths::data_dir()).unwrap();
        std::fs::write(paths::state_file(), b"{ not json").unwrap();
        update_game("a", |g| g.last_synced_id = Some("01JA".into())).unwrap();
        assert_eq!(std::fs::read(paths::state_file().with_extension("json.broken")).unwrap(), b"{ not json");
        assert_eq!(game("a").last_synced_id.as_deref(), Some("01JA"));
    }
}
