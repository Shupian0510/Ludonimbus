//! Manifests and summaries of game backup versions.
//!
//! A version is two files: `<id>.manifest` (zstd-compressed JSON listing
//! every file and chunk) and `<id>.json` (a small summary, written last as
//! the commit marker, holding the manifest's BLAKE3 hash).

use super::chunker::Params;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const FORMAT: u32 = 1;

/// One file, in stream order. Short field names keep manifests of games with
/// tens of thousands of files small.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path relative to the backed-up folder, `/`-separated.
    pub p: String,
    /// Size in bytes.
    pub s: u64,
    /// BLAKE3 of the content.
    pub h: String,
    /// Modification time, Unix seconds.
    pub m: i64,
}

/// One chunk of the stream, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRef {
    /// BLAKE3 of the raw content = object name.
    pub h: String,
    /// Raw size.
    pub s: u64,
    /// Stored (object) size.
    pub z: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub game: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub device_id: String,
    pub device_name: String,
    pub app_version: String,
    /// The folder that was backed up (for information and as the default
    /// place to restore to).
    pub root: String,
    /// The game's program relative to `root`, when it is inside it.
    #[serde(default)]
    pub exe: Option<String>,
    /// Working directory relative to `root`, when set and inside it.
    #[serde(default)]
    pub work_dir: Option<String>,
    pub chunking: Params,
    /// Exclude patterns in effect (defaults, saves, the user's own).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The user's own exclude patterns, reused by the next backup.
    #[serde(default)]
    pub user_exclude: Vec<String>,
    pub files: Vec<FileEntry>,
    /// Folders not created by any file (empty, or only excluded content).
    #[serde(default)]
    pub dirs: Vec<String>,
    pub chunks: Vec<ChunkRef>,
    pub total_size: u64,
    /// Sum of the stored chunk sizes (shared chunks counted once).
    pub stored_size: u64,
    /// Bytes uploaded for this version.
    pub new_bytes: u64,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub format: u32,
    pub id: String,
    pub game: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub device_id: String,
    pub device_name: String,
    pub app_version: String,
    pub root: String,
    pub total_size: u64,
    pub stored_size: u64,
    pub new_bytes: u64,
    pub file_count: usize,
    pub chunk_count: usize,
    /// BLAKE3 of the `.manifest` file as stored.
    pub manifest_hash: String,
    pub manifest_size: u64,
    #[serde(default)]
    pub comment: String,
}

impl Manifest {
    /// The stored `.manifest` bytes and the summary that commits them.
    pub fn encode(&self) -> Result<(Vec<u8>, Summary)> {
        let json = serde_json::to_vec(self)?;
        let bytes = zstd::bulk::compress(&json, 9)?;
        let summary = Summary {
            format: self.format,
            id: self.id.clone(),
            game: self.game.clone(),
            name: self.name.clone(),
            created_at: self.created_at,
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            app_version: self.app_version.clone(),
            root: self.root.clone(),
            total_size: self.total_size,
            stored_size: self.stored_size,
            new_bytes: self.new_bytes,
            file_count: self.files.len(),
            chunk_count: self.chunks.len(),
            manifest_hash: blake3::hash(&bytes).to_hex().to_string(),
            manifest_size: bytes.len() as u64,
            comment: self.comment.clone(),
        };
        Ok((bytes, summary))
    }

    /// Read a `.manifest` checked against its summary.
    pub fn decode(bytes: &[u8], summary: &Summary) -> Result<Manifest> {
        if blake3::hash(bytes).to_hex().as_str() != summary.manifest_hash {
            bail!(crate::tr!(
                "版本 {} 的清单已损坏（哈希不一致）",
                "The manifest of version {} is damaged (hash mismatch)",
                summary.id
            ));
        }
        let json = zstd::stream::decode_all(bytes).map_err(|e| anyhow!(e))?;
        let m: Manifest = serde_json::from_slice(&json)?;
        if m.format > FORMAT {
            bail!(crate::tr!(
                "版本 {} 是更新的 Ludonimbus 做的（格式 {}），请先更新 Ludonimbus",
                "Version {} was made by a newer Ludonimbus (format {}). Update Ludonimbus first",
                m.id,
                m.format
            ));
        }
        Ok(m)
    }

    /// Offset of every file in the stream (same order as `files`).
    pub fn file_offsets(&self) -> Vec<u64> {
        let mut out = Vec::with_capacity(self.files.len());
        let mut at = 0;
        for f in &self.files {
            out.push(at);
            at += f.s;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Manifest {
        Manifest {
            format: FORMAT,
            id: "01J0000000000000000000000A".into(),
            game: "g".into(),
            name: "Game".into(),
            created_at: Utc::now(),
            device_id: "d-1".into(),
            device_name: "PC".into(),
            app_version: "test".into(),
            root: "D:\\Games\\G".into(),
            exe: Some("g.exe".into()),
            work_dir: None,
            chunking: Params::default(),
            exclude: vec!["**/*.log".into()],
            user_exclude: Vec::new(),
            files: vec![
                FileEntry { p: "a".into(), s: 3, h: "x".into(), m: 1 },
                FileEntry { p: "b/c".into(), s: 5, h: "y".into(), m: 2 },
            ],
            dirs: vec!["empty".into()],
            chunks: vec![ChunkRef { h: "z".into(), s: 8, z: 14 }],
            total_size: 8,
            stored_size: 14,
            new_bytes: 14,
            comment: String::new(),
        }
    }

    #[test]
    fn round_trip_and_damage() {
        let m = sample();
        let (bytes, summary) = m.encode().unwrap();
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.chunk_count, 1);
        let back = Manifest::decode(&bytes, &summary).unwrap();
        assert_eq!(back.files, m.files);
        assert_eq!(back.file_offsets(), vec![0, 3]);
        let mut bad = bytes.clone();
        bad[0] ^= 1;
        assert!(Manifest::decode(&bad, &summary).is_err());
    }
}
