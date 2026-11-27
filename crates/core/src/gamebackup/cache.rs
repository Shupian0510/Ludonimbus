//! What this PC already knows about big files it backed up: their hash and
//! chunks, by path, size and modification time. A multi-GB disc image that
//! hasn't changed is then not read again (`%APPDATA%\Ludonimbus\gamebackup-cache\files.json`).

use super::chunker::Params;
use super::format::ChunkRef;
use super::scan::path_key;
use crate::{paths, util};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub size: u64,
    pub mtime: i64,
    pub nanos: u32,
    pub params: (usize, usize, usize),
    pub hash: String,
    /// Raw chunks (`z` is filled in from the cloud when used).
    pub chunks: Vec<(String, u64)>,
}

pub struct FileCache {
    path: PathBuf,
    map: HashMap<String, Entry>,
    dirty: bool,
}

fn key_params(p: &Params) -> (usize, usize, usize) {
    (p.min, p.avg, p.max)
}

impl FileCache {
    pub fn load() -> FileCache {
        let path = paths::data_dir().join("gamebackup-cache").join("files.json");
        let map = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        FileCache { path, map, dirty: false }
    }

    /// The cached hash and chunks of `abs`, if it is unchanged.
    pub fn get(&self, abs: &Path, size: u64, mtime: i64, nanos: u32, params: &Params) -> Option<&Entry> {
        self.map.get(&path_key(abs)).filter(|e| e.size == size && e.mtime == mtime && e.nanos == nanos && e.params == key_params(params))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn put(&mut self, abs: &Path, size: u64, mtime: i64, nanos: u32, params: &Params, hash: String, chunks: &[ChunkRef]) {
        let e = Entry { size, mtime, nanos, params: key_params(params), hash, chunks: chunks.iter().map(|c| (c.h.clone(), c.s)).collect() };
        self.map.insert(path_key(abs), e);
        self.dirty = true;
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        // Forget files that are gone.
        self.map.retain(|k, _| Path::new(k).exists());
        if let Ok(bytes) = serde_json::to_vec(&self.map) {
            if util::atomic_write(&self.path, &bytes).is_ok() {
                self.dirty = false;
            }
        }
    }
}
