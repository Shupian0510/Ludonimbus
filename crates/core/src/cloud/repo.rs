//! The Ludonimbus cloud layout on top of any path-based [`Store`].
//!
//! A snapshot is committed when its `.json` exists next to its `.zip`: the
//! zip is uploaded first and the metadata last, so an interrupted upload is
//! never listed. Stores without Drive-style file properties need one read per
//! snapshot to list the history; those reads are cached on disk by the
//! store's change tag, so only new or edited snapshots are fetched again.

use super::store::{join, Store};
use super::{AccountInfo, CloudGame, CloudProvider, DeviceInfo};
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use crate::{paths, util};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Repo<S: Store> {
    store: S,
    cache: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    tag: String,
    body: String,
}

fn hex(s: &str, len: usize) -> String {
    blake3::hash(s.as_bytes()).to_hex()[..len].to_string()
}

fn snapshots_dir(game: &str) -> String {
    join(&["games", game, "snapshots"])
}

impl<S: Store> Repo<S> {
    pub fn new(store: S) -> Self {
        let cache = store
            .cache_scope()
            .map(|scope| paths::data_dir().join("cloud-cache").join(hex(&scope, 16)));
        Repo { store, cache }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    fn cache_file(&self, path: &str) -> Option<PathBuf> {
        self.cache.as_ref().map(|d| d.join(format!("{}.json", hex(path, 32))))
    }

    /// Read a small file, reusing the cached copy while its tag is unchanged.
    fn read_tagged(&self, path: &str, tag: &str) -> Result<Option<Vec<u8>>> {
        let file = self.cache_file(path).filter(|_| !tag.is_empty());
        if let Some(f) = &file {
            if let Some(c) = std::fs::read(f).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok()) {
                if c.tag == tag {
                    return Ok(Some(c.body.into_bytes()));
                }
            }
        }
        let body = self.store.read(path)?;
        if let (Some(f), Some(b)) = (&file, &body) {
            if let Ok(text) = std::str::from_utf8(b) {
                let cached = Cached {
                    tag: tag.to_string(),
                    body: text.to_string(),
                };
                if let Ok(bytes) = serde_json::to_vec(&cached) {
                    let _ = util::atomic_write(f, &bytes);
                }
            }
        }
        Ok(body)
    }

    fn forget(&self, path: &str) {
        if let Some(f) = self.cache_file(path) {
            let _ = std::fs::remove_file(f);
        }
    }

    fn write_json<T: Serialize>(&self, path: &str, value: &T) -> Result<()> {
        self.forget(path);
        self.store.write(path, &serde_json::to_vec_pretty(value)?)
    }
}

impl<S: Store> CloudProvider for Repo<S> {
    fn kind(&self) -> &'static str {
        self.store.kind()
    }

    fn account(&self) -> Result<AccountInfo> {
        self.store.check()?;
        let (used, total) = self.store.quota().unwrap_or((None, None));
        Ok(AccountInfo {
            label: self.store.label(),
            used,
            total,
        })
    }

    fn list_snapshots(&self, game: &str) -> Result<Vec<SnapshotSummary>> {
        let dir = snapshots_dir(game);
        let entries = self.store.list(&dir)?;
        let files: HashSet<&str> = entries.iter().filter(|e| !e.is_dir).map(|e| e.name.as_str()).collect();
        let mut out = Vec::new();
        for e in entries.iter().filter(|e| !e.is_dir) {
            let Some(id) = e.name.strip_suffix(".json") else { continue };
            if !files.contains(format!("{id}.zip").as_str()) {
                continue;
            }
            if let Some(bytes) = self.read_tagged(&join(&[&dir, &e.name]), &e.tag)? {
                if let Ok(meta) = serde_json::from_slice::<SnapshotMeta>(&bytes) {
                    out.push(SnapshotSummary::from(&meta));
                }
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    fn get_meta(&self, game: &str, id: &str) -> Result<SnapshotMeta> {
        let bytes = self
            .store
            .read(&join(&[&snapshots_dir(game), &format!("{id}.json")]))?
            .ok_or_else(|| anyhow!(crate::tr!("找不到快照 {id}", "Snapshot {id} not found")))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn upload(&self, meta: &SnapshotMeta, zip: &Path) -> Result<()> {
        let dir = snapshots_dir(&meta.game);
        self.store.upload(&join(&[&dir, &format!("{}.zip", meta.id)]), zip)?;
        // The metadata file is written last and acts as the commit marker.
        self.write_json(&join(&[&dir, &format!("{}.json", meta.id)]), meta)
    }

    fn download(&self, game: &str, id: &str, dest: &Path) -> Result<()> {
        self.store.download(&join(&[&snapshots_dir(game), &format!("{id}.zip")]), dest)
    }

    fn delete(&self, game: &str, id: &str) -> Result<()> {
        let dir = snapshots_dir(game);
        let json = join(&[&dir, &format!("{id}.json")]);
        self.forget(&json);
        self.store.remove(&json)?;
        self.store.remove(&join(&[&dir, &format!("{id}.zip")]))
    }

    fn update_meta(&self, meta: &SnapshotMeta) -> Result<()> {
        self.write_json(&join(&[&snapshots_dir(&meta.game), &format!("{}.json", meta.id)]), meta)
    }

    fn put_device(&self, device: &DeviceInfo) -> Result<()> {
        self.write_json(&join(&["devices", &format!("{}.json", device.id)]), device)
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut out = Vec::new();
        for e in self.store.list("devices")? {
            if e.is_dir || !e.name.ends_with(".json") {
                continue;
            }
            if let Some(bytes) = self.read_tagged(&join(&["devices", &e.name]), &e.tag)? {
                if let Ok(d) = serde_json::from_slice(&bytes) {
                    out.push(d);
                }
            }
        }
        Ok(out)
    }

    fn put_game(&self, game: &CloudGame) -> Result<()> {
        self.write_json(&join(&["games", &game.key, "game.json"]), game)
    }

    fn list_games(&self) -> Result<Vec<CloudGame>> {
        let mut out = Vec::new();
        for e in self.store.list("games")? {
            if !e.is_dir {
                continue;
            }
            if let Some(bytes) = self.store.read(&join(&["games", &e.name, "game.json"]))? {
                if let Ok(g) = serde_json::from_slice(&bytes) {
                    out.push(g);
                }
            }
        }
        Ok(out)
    }

    fn files(&self) -> &dyn Store {
        &self.store
    }
}
