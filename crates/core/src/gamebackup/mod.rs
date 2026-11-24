//! Game backups: the game's own files (not its saves) in the cloud, so a game
//! that is gone from this PC can be installed again from there. Design and
//! cloud layout: docs/plan-game-backup.md.
//!
//! Everything here works on a path-based [`Store`]; callers pass
//! `provider.files()` of the current cloud, tests a local folder or WebDAV.
//!
//! - [`scan`]      which files of the folder go in (saves and caches don't)
//! - [`chunker`]   content-defined chunking of the folder stream
//! - [`chunk`]     stored chunk objects (zstd or raw), named by BLAKE3
//! - [`format`]    manifests and summaries of versions
//! - [`remote`]    the cloud layout, the lock, deleting and cleaning up
//! - [`backup`] / [`restore`] / [`install`] the three steps the user sees
//! - [`speedtest`] upload / download speed of a cloud

pub mod backup;
pub mod chunk;
pub mod chunker;
pub mod format;
pub mod install;
pub mod remote;
pub mod restore;
pub mod scan;
pub mod speedtest;

use crate::cloud::store::Store;
use crate::cloud::{self, CloudProvider, NotConfigured};
use crate::config::{AppConfig, GameEntry};
use crate::{paths, spec, util};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The current cloud; use `.files()` for the path-based store.
pub fn open(cfg: &AppConfig) -> Result<Box<dyn CloudProvider>> {
    cloud::open(cfg)?.ok_or_else(|| anyhow!(NotConfigured))
}

/// The user stopped the backup or restore (it continues next time).
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::tr!("已停止，下次会从断点继续", "Stopped; it continues where it left off next time"))
    }
}
impl std::error::Error for Cancelled {}

pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Cancelled>().is_some())
}

/// Run `f`, retrying after 2, 8 and 30 seconds when it fails (busy servers,
/// dropped connections). Stops waiting when cancelled.
pub(crate) fn retry<T>(cancel: &AtomicBool, mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut waits = [2u64, 8, 30].into_iter();
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                let Some(secs) = waits.next() else { return Err(e) };
                for _ in 0..secs * 10 {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

/// Clouds where backups work but are slow (see the plan, section 1.4).
pub fn is_slow_cloud(kind: &str) -> bool {
    matches!(kind, "baiduPan" | "quark")
}

/// Parallel uploads / downloads by default for a cloud kind.
pub fn default_threads(kind: &str) -> usize {
    if kind == "quark" {
        2
    } else {
        4
    }
}

// ---------------------------------------------------------------------------
// Per-game settings on this PC (%APPDATA%\Ludonimbus\gamebackup.json)

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameSettings {
    /// Folder to back up; `None` = the game's folder.
    pub root: Option<PathBuf>,
    /// The user's own exclude patterns; `None` = those of the newest version.
    pub exclude: Option<Vec<String>>,
    /// Versions to keep: `None` = 2, `Some(0)` = all.
    pub keep: Option<usize>,
    /// The last restore started on this PC, to offer continuing it.
    pub last_restore: Option<PendingRestore>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestore {
    pub version: String,
    pub target: PathBuf,
}

pub const DEFAULT_KEEP: usize = 2;

impl GameSettings {
    /// Versions kept after a backup (`None` = all).
    pub fn keep_count(&self) -> Option<usize> {
        match self.keep {
            None => Some(DEFAULT_KEEP),
            Some(0) => None,
            Some(n) => Some(n),
        }
    }
}

fn settings_file() -> PathBuf {
    paths::data_dir().join("gamebackup.json")
}

fn load_all() -> BTreeMap<String, GameSettings> {
    std::fs::read(settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn settings(key: &str) -> GameSettings {
    load_all().remove(key).unwrap_or_default()
}

pub fn update_settings(key: &str, f: impl FnOnce(&mut GameSettings)) -> Result<GameSettings> {
    let mut all = load_all();
    let s = all.entry(key.to_string()).or_default();
    f(s);
    let out = s.clone();
    util::atomic_write(&settings_file(), &serde_json::to_vec_pretty(&all)?)?;
    Ok(out)
}

/// The folder a backup of `game` takes.
pub fn backup_root(game: &GameEntry, s: &GameSettings) -> Option<PathBuf> {
    s.root.clone().or_else(|| spec::install_dir(game))
}

/// The user's exclude patterns: this PC's, else those of the newest version.
pub fn user_excludes(s: &GameSettings, latest: Option<&format::Manifest>) -> Vec<String> {
    s.exclude.clone().or_else(|| latest.map(|m| m.user_exclude.clone())).unwrap_or_default()
}

/// The newest version and its manifest, if any.
pub fn latest(store: &dyn Store, key: &str) -> Result<Option<(format::Summary, format::Manifest)>> {
    match remote::list_versions(store, key)?.pop() {
        Some(s) => {
            let m = remote::read_manifest(store, key, &s)?;
            Ok(Some((s, m)))
        }
        None => Ok(None),
    }
}
