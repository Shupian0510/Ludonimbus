//! Game backups in the cloud, on any path-based [`Store`]:
//!
//! ```text
//! gamebackup/<key>/chunks/<blake3>            chunks, shared by all versions
//! gamebackup/<key>/versions/<id>.manifest     manifest (zstd JSON)
//! gamebackup/<key>/versions/<id>.json         summary, written last (commit marker)
//! gamebackup/<key>/lock.json                  device backing up / cleaning up
//! ```
//!
//! Kept apart from the save snapshots (`games/<key>/…`): their listing,
//! retention and copying to another cloud never see game backups.

use super::format::{Manifest, Summary};
use crate::cloud::store::{join, Store};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const PREFIX: &str = "gamebackup";
/// A lock not refreshed for this long belongs to a device that stopped.
const LOCK_MINUTES: i64 = 30;

pub fn base(key: &str) -> String {
    join(&[PREFIX, key])
}
pub fn chunks_dir(key: &str) -> String {
    join(&[PREFIX, key, "chunks"])
}
pub fn chunk_path(key: &str, id: &str) -> String {
    join(&[PREFIX, key, "chunks", id])
}
fn versions_dir(key: &str) -> String {
    join(&[PREFIX, key, "versions"])
}
fn manifest_path(key: &str, id: &str) -> String {
    join(&[PREFIX, key, "versions", &format!("{id}.manifest")])
}
fn summary_path(key: &str, id: &str) -> String {
    join(&[PREFIX, key, "versions", &format!("{id}.json")])
}
fn lock_path(key: &str) -> String {
    join(&[PREFIX, key, "lock.json"])
}

/// Committed versions, oldest first.
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

pub fn read_manifest(store: &dyn Store, key: &str, summary: &Summary) -> Result<Manifest> {
    let bytes = store.read(&manifest_path(key, &summary.id))?.ok_or_else(|| {
        anyhow!(crate::tr!(
            "版本 {} 已不在云端（可能刚被删除）",
            "Version {} is no longer in the cloud (it may have just been deleted)",
            summary.id
        ))
    })?;
    Manifest::decode(&bytes, summary)
}

/// Write the manifest, then the summary that commits it.
pub fn commit_version(store: &dyn Store, manifest: &Manifest) -> Result<Summary> {
    let (bytes, summary) = manifest.encode()?;
    store.write(&manifest_path(&manifest.game, &manifest.id), &bytes)?;
    store.write(&summary_path(&manifest.game, &manifest.id), &serde_json::to_vec_pretty(&summary)?)?;
    Ok(summary)
}

/// Uncommit (summary first), then delete the manifest. Chunks stay until
/// [`collect_garbage`].
pub fn delete_version(store: &dyn Store, key: &str, id: &str) -> Result<()> {
    store.remove(&summary_path(key, id))?;
    store.remove(&manifest_path(key, id))
}

/// Chunks in the cloud: name → stored size.
pub fn list_chunks(store: &dyn Store, key: &str) -> Result<HashMap<String, u64>> {
    Ok(store
        .list(&chunks_dir(key))?
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
}

/// Delete chunks no committed version uses. Call while holding the lock, so
/// no backup is uploading chunks that are not committed yet.
pub fn collect_garbage(store: &dyn Store, key: &str, progress: &dyn Fn(usize, usize)) -> Result<GcReport> {
    let mut used = HashSet::new();
    for s in list_versions(store, key)? {
        for c in read_manifest(store, key, &s)?.chunks {
            used.insert(c.h);
        }
    }
    let chunks = list_chunks(store, key)?;
    let unused: Vec<(&String, &u64)> = chunks.iter().filter(|(id, _)| !used.contains(*id)).collect();
    let mut report = GcReport { kept_chunks: chunks.len() - unused.len(), ..Default::default() };
    for (i, (id, size)) in unused.iter().enumerate() {
        store.remove(&chunk_path(key, id))?;
        report.deleted_chunks += 1;
        report.freed_bytes += **size;
        progress(i + 1, unused.len());
    }
    Ok(report)
}

/// Delete versions beyond the newest `keep`, then their unused chunks.
/// Returns the deleted version ids.
pub fn prune(store: &dyn Store, key: &str, keep: usize, progress: &dyn Fn(usize, usize)) -> Result<(Vec<String>, GcReport)> {
    let versions = list_versions(store, key)?;
    let keep = keep.max(1);
    let mut deleted = Vec::new();
    if versions.len() > keep {
        for s in &versions[..versions.len() - keep] {
            delete_version(store, key, &s.id)?;
            deleted.push(s.id.clone());
        }
    }
    let gc = if deleted.is_empty() { GcReport::default() } else { collect_garbage(store, key, progress)? };
    Ok((deleted, gc))
}

/// Space used by all chunks of a game in the cloud.
pub fn stored_bytes(store: &dyn Store, key: &str) -> Result<u64> {
    Ok(list_chunks(store, key)?.values().sum())
}

// ---------------------------------------------------------------------------
// Lock

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
    key: String,
    info: std::sync::Mutex<LockInfo>,
    released: bool,
}

/// Who holds the lock of a game, when someone does and it has not expired.
pub fn lock_holder(store: &dyn Store, key: &str) -> Result<Option<LockInfo>> {
    Ok(store
        .read(&lock_path(key))?
        .and_then(|b| serde_json::from_slice::<LockInfo>(&b).ok())
        .filter(|l| l.expires > Utc::now()))
}

impl<'a> Lock<'a> {
    /// Take the lock of a game. Fails when another device (or another task
    /// on this one) holds a lock that has not expired.
    pub fn acquire(store: &'a dyn Store, key: &str, device_id: &str, device_name: &str, kind: &str) -> Result<Lock<'a>> {
        if let Some(l) = lock_holder(store, key)? {
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
        store.write(&lock_path(key), &serde_json::to_vec_pretty(&info)?)?;
        // Two devices writing at once: the last write wins, the other backs off.
        let back = lock_holder(store, key)?;
        if back.as_ref().map(|l| &l.token) != Some(&info.token) {
            bail!(crate::tr!("另一台设备刚开始处理这个游戏，请稍后再试", "Another device just started working on this game. Try again later"));
        }
        Ok(Lock { store, key: key.to_string(), info: std::sync::Mutex::new(info), released: false })
    }

    /// Push the expiry time forward (long uploads).
    pub fn refresh(&self) -> Result<()> {
        let bytes = {
            let mut info = self.info.lock().unwrap();
            info.expires = Utc::now() + Duration::minutes(LOCK_MINUTES);
            serde_json::to_vec_pretty(&*info)?
        };
        self.store.write(&lock_path(&self.key), &bytes)
    }

    pub fn release(mut self) -> Result<()> {
        self.released = true;
        self.remove()
    }

    fn remove(&self) -> Result<()> {
        // Only our own lock: an expired one may have been taken over.
        let token = self.info.lock().unwrap().token.clone();
        match lock_holder(self.store, &self.key)? {
            Some(l) if l.token != token => Ok(()),
            _ => self.store.remove(&lock_path(&self.key)),
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
