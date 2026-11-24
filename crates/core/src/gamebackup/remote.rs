//! Game backups in the cloud (format 2), on any path-based [`Store`]:
//!
//! ```text
//! gamebackup/chunks/<blake3>                        chunks of all games (stored once)
//! gamebackup/games/<key>/versions/<id>.manifest     manifest (zstd JSON)
//! gamebackup/games/<key>/versions/<id>.json         summary, written last (commit marker)
//! gamebackup/games/<key>/lock.json                  device backing up this game
//! gamebackup/cleanup.json                           device deleting unused chunks
//! ```
//!
//! Kept apart from the save snapshots (`games/<key>/…` at the root): their
//! listing, retention and copying to another cloud never see game backups.
//! Chunks are shared by all games, so deleting unused ones looks at every
//! game's versions and waits while any game is being backed up.

use super::format::{Manifest, Summary};
use crate::cloud::store::{join, Store};
use crate::paths;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const PREFIX: &str = "gamebackup";
/// A lock not refreshed for this long belongs to a device that stopped.
const LOCK_MINUTES: i64 = 30;

pub fn chunks_dir() -> String {
    join(&[PREFIX, "chunks"])
}
pub fn chunk_path(id: &str) -> String {
    join(&[PREFIX, "chunks", id])
}
fn games_dir() -> String {
    join(&[PREFIX, "games"])
}
fn versions_dir(key: &str) -> String {
    join(&[PREFIX, "games", key, "versions"])
}
fn manifest_path(key: &str, id: &str) -> String {
    join(&[PREFIX, "games", key, "versions", &format!("{id}.manifest")])
}
fn summary_path(key: &str, id: &str) -> String {
    join(&[PREFIX, "games", key, "versions", &format!("{id}.json")])
}
fn game_lock_path(key: &str) -> String {
    join(&[PREFIX, "games", key, "lock.json"])
}
fn cleanup_lock_path() -> String {
    join(&[PREFIX, "cleanup.json"])
}

/// Committed versions of a game, oldest first.
pub fn list_versions(store: &dyn Store, key: &str) -> Result<Vec<Summary>> {
    let entries = store.list(&versions_dir(key))?;
    let names: HashSet<&str> = entries.iter().filter(|e| !e.is_dir).map(|e| e.name.as_str()).collect();
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir) {
        let Some(id) = e.name.strip_suffix(".json") else { continue };
        if !names.contains(format!("{id}.manifest").as_str()) {
            continue;
        }
        if let Some(bytes) = store.read(&summary_path(key, id))? {
            if let Ok(s) = serde_json::from_slice::<Summary>(&bytes) {
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Games that have game backups.
pub fn list_games(store: &dyn Store) -> Result<Vec<String>> {
    Ok(store.list(&games_dir())?.into_iter().filter(|e| e.is_dir).map(|e| e.name).collect())
}

/// Manifests never change once written, so they are kept on this PC by hash.
fn cache_file(hash: &str) -> PathBuf {
    paths::data_dir().join("gamebackup-cache").join("manifests").join(format!("{hash}.zst"))
}

pub fn read_manifest(store: &dyn Store, key: &str, summary: &Summary) -> Result<Manifest> {
    let cached = cache_file(&summary.manifest_hash);
    if let Ok(bytes) = std::fs::read(&cached) {
        if let Ok(m) = Manifest::decode(&bytes, summary) {
            return Ok(m);
        }
    }
    let bytes = store.read(&manifest_path(key, &summary.id))?.ok_or_else(|| {
        anyhow!(crate::tr!(
            "版本 {} 已不在云端（可能刚被删除）",
            "Version {} is no longer in the cloud (it may have just been deleted)",
            summary.id
        ))
    })?;
    let m = Manifest::decode(&bytes, summary)?;
    let _ = crate::util::atomic_write(&cached, &bytes);
    Ok(m)
}

/// Write the manifest, then the summary that commits it.
pub fn commit_version(store: &dyn Store, manifest: &Manifest) -> Result<Summary> {
    let (bytes, summary) = manifest.encode()?;
    store.write(&manifest_path(&manifest.game, &manifest.id), &bytes)?;
    store.write(&summary_path(&manifest.game, &manifest.id), &serde_json::to_vec_pretty(&summary)?)?;
    let _ = crate::util::atomic_write(&cache_file(&summary.manifest_hash), &bytes);
    Ok(summary)
}

/// Uncommit (summary first), then delete the manifest. Chunks stay until
/// [`collect_garbage`].
pub fn delete_version(store: &dyn Store, key: &str, id: &str) -> Result<()> {
    store.remove(&summary_path(key, id))?;
    store.remove(&manifest_path(key, id))
}

/// Chunks in the cloud: name → stored size.
pub fn list_chunks(store: &dyn Store) -> Result<HashMap<String, u64>> {
    Ok(store
        .list(&chunks_dir())?
        .into_iter()
        .filter(|e| !e.is_dir && e.name.len() == 64 && !e.name.contains('.'))
        .map(|e| (e.name, e.size))
        .collect())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcReport {
    pub deleted_chunks: usize,
    pub freed_bytes: u64,
    pub kept_chunks: usize,
    /// Not cleaned up because a device is backing up (it is done later).
    pub postponed: bool,
}

/// Delete chunks no committed version of any game uses. Skipped (and
/// reported as postponed) while another device backs up a game: its chunks
/// are not committed yet. `own_game` is the game whose lock the caller holds.
pub fn collect_garbage(store: &dyn Store, device: (&str, &str), own_game: Option<&str>, progress: &dyn Fn(usize, usize)) -> Result<GcReport> {
    let lock = match Lock::acquire_path(store, &cleanup_lock_path(), device.0, device.1, "cleanup") {
        Ok(l) => l,
        Err(_) => return Ok(GcReport { postponed: true, ..Default::default() }),
    };
    let games = list_games(store)?;
    for g in &games {
        if Some(g.as_str()) == own_game {
            continue;
        }
        if lock_holder_at(store, &game_lock_path(g))?.is_some() {
            lock.release()?;
            return Ok(GcReport { postponed: true, ..Default::default() });
        }
    }
    let mut used = HashSet::new();
    for g in &games {
        for s in list_versions(store, g)? {
            for c in read_manifest(store, g, &s)?.components {
                used.extend(c.chunks.into_iter().map(|c| c.h));
            }
        }
    }
    let chunks = list_chunks(store)?;
    let unused: Vec<(&String, &u64)> = chunks.iter().filter(|(id, _)| !used.contains(*id)).collect();
    let mut report = GcReport { kept_chunks: chunks.len() - unused.len(), ..Default::default() };
    for (i, (id, size)) in unused.iter().enumerate() {
        store.remove(&chunk_path(id))?;
        report.deleted_chunks += 1;
        report.freed_bytes += **size;
        progress(i + 1, unused.len());
        if i % 50 == 49 {
            lock.refresh()?;
        }
    }
    lock.release()?;
    Ok(report)
}

/// Delete versions of `key` beyond the newest `keep`, then unused chunks.
/// Returns the deleted version ids.
pub fn prune(
    store: &dyn Store,
    key: &str,
    keep: usize,
    device: (&str, &str),
    progress: &dyn Fn(usize, usize),
) -> Result<(Vec<String>, GcReport)> {
    let versions = list_versions(store, key)?;
    let keep = keep.max(1);
    let mut deleted = Vec::new();
    if versions.len() > keep {
        for s in &versions[..versions.len() - keep] {
            delete_version(store, key, &s.id)?;
            deleted.push(s.id.clone());
        }
    }
    let gc = if deleted.is_empty() { GcReport::default() } else { collect_garbage(store, device, Some(key), progress)? };
    Ok((deleted, gc))
}

// ---------------------------------------------------------------------------
// Locks

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockInfo {
    pub device_id: String,
    pub device_name: String,
    /// backup | cleanup
    pub kind: String,
    pub since: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    /// Tells two takes of the same device apart.
    pub token: String,
}

pub struct Lock<'a> {
    store: &'a dyn Store,
    path: String,
    info: std::sync::Mutex<LockInfo>,
    released: bool,
}

fn lock_holder_at(store: &dyn Store, path: &str) -> Result<Option<LockInfo>> {
    Ok(store
        .read(path)?
        .and_then(|b| serde_json::from_slice::<LockInfo>(&b).ok())
        .filter(|l| l.expires > Utc::now()))
}

/// Who is backing up a game, when someone is and the lock has not expired.
pub fn lock_holder(store: &dyn Store, key: &str) -> Result<Option<LockInfo>> {
    lock_holder_at(store, &game_lock_path(key))
}

/// Who is deleting unused chunks right now.
pub fn cleanup_holder(store: &dyn Store) -> Result<Option<LockInfo>> {
    lock_holder_at(store, &cleanup_lock_path())
}

impl<'a> Lock<'a> {
    /// Take the backup lock of a game. Fails when another device (or task)
    /// holds it, or while unused chunks are being deleted.
    pub fn acquire(store: &'a dyn Store, key: &str, device_id: &str, device_name: &str, kind: &str) -> Result<Lock<'a>> {
        if let Some(l) = cleanup_holder(store)? {
            bail!(crate::tr!(
                "「{}」正在清理云端不再使用的数据，请过几分钟再试",
                "\"{}\" is cleaning up data no longer used in the cloud. Try again in a few minutes",
                l.device_name
            ));
        }
        Self::acquire_path(store, &game_lock_path(key), device_id, device_name, kind)
    }

    fn acquire_path(store: &'a dyn Store, path: &str, device_id: &str, device_name: &str, kind: &str) -> Result<Lock<'a>> {
        if let Some(l) = lock_holder_at(store, path)? {
            let what = if l.kind == "cleanup" { crate::tr!("清理", "cleaning up") } else { crate::tr!("备份", "backing up") };
            bail!(crate::tr!(
                "「{}」正在{what}这个游戏（从 {} 开始），请等它完成再试",
                "\"{}\" is {what} this game (since {}). Try again when it has finished",
                l.device_name,
                l.since.with_timezone(&chrono::Local).format("%H:%M")
            ));
        }
        let now = Utc::now();
        let info = LockInfo {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            kind: kind.to_string(),
            since: now,
            expires: now + Duration::minutes(LOCK_MINUTES),
            token: ulid::Ulid::new().to_string(),
        };
        store.write(path, &serde_json::to_vec_pretty(&info)?)?;
        // Two devices writing at once: the last write wins, the other backs off.
        let back = lock_holder_at(store, path)?;
        if back.as_ref().map(|l| &l.token) != Some(&info.token) {
            bail!(crate::tr!("另一台设备刚开始处理这个游戏，请稍后再试", "Another device just started working on this game. Try again later"));
        }
        Ok(Lock { store, path: path.to_string(), info: std::sync::Mutex::new(info), released: false })
    }

    /// Push the expiry time forward (long uploads).
    pub fn refresh(&self) -> Result<()> {
        let bytes = {
            let mut info = self.info.lock().unwrap();
            info.expires = Utc::now() + Duration::minutes(LOCK_MINUTES);
            serde_json::to_vec_pretty(&*info)?
        };
        self.store.write(&self.path, &bytes)
    }

    pub fn release(mut self) -> Result<()> {
        self.released = true;
        self.remove()
    }

    fn remove(&self) -> Result<()> {
        // Only our own lock: an expired one may have been taken over.
        let token = self.info.lock().unwrap().token.clone();
        match lock_holder_at(self.store, &self.path)? {
            Some(l) if l.token != token => Ok(()),
            _ => self.store.remove(&self.path),
        }
    }
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.remove();
        }
    }
}
