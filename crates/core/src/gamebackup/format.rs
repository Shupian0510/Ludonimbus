//! Manifests and summaries of game backup versions (format 2).
//!
//! A version is two files: `<id>.manifest` (zstd-compressed JSON listing
//! every component with its files and chunks) and `<id>.json` (a small
//! summary, written last as the commit marker, holding the manifest's
//! BLAKE3 hash).
//!
//! A component is one part of a game that is versioned and placed on its
//! own: the program (emulator, recompilation), the game's files (ROM, disc
//! image, updates), its settings and mods, firmware, or a whole folder.
//! File paths start with a base, e.g. `{program}/Cemu.exe`,
//! `{content}/Game.wua`, `{appdata}/Cemu/mlc01/…`; restoring maps the bases
//! to folders on this PC (see [`super::components::Bases`]).

use super::chunker::Params;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT: u32 = 2;

/// One file, in stream order. Short field names keep manifests of games with
/// tens of thousands of files small.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// `{base}/relative/path`, `/`-separated.
    pub p: String,
    /// Size in bytes.
    pub s: u64,
    /// BLAKE3 of the content.
    pub h: String,
    /// Modification time, Unix seconds.
    pub m: i64,
    /// Nanoseconds of the modification time.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub n: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One chunk of a component's stream, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRef {
    /// BLAKE3 of the raw content = object name.
    pub h: String,
    /// Raw size.
    pub s: u64,
    /// Stored (object) size.
    pub z: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Emulator, recompilation, port.
    Program,
    /// ROM, disc image, game folder, updates, DLC.
    Content,
    /// The game's settings and mods.
    Config,
    /// Keys, firmware, BIOS.
    Firmware,
    /// A whole folder (PC games, and games whose parts aren't known).
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub kind: Kind,
    /// "Cemu", "Zelda 64: Recompiled", or the game's name.
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Folders the `{program}`, `{content}` and `{extraN}` bases stood for
    /// when backing up.
    #[serde(default)]
    pub bases: BTreeMap<String, String>,
    /// Program: its exe (`{program}/…`).
    #[serde(default)]
    pub exe: Option<String>,
    /// Content: the file or folder the launch options name.
    #[serde(default)]
    pub source: Option<String>,
    /// Program: the launcher that builds the game from its files on each PC
    /// (what it builds is not backed up).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    pub files: Vec<FileEntry>,
    /// Folders no file creates (empty, or only excluded content).
    #[serde(default)]
    pub dirs: Vec<String>,
    pub chunks: Vec<ChunkRef>,
    pub total_size: u64,
}

impl Component {
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

    pub fn summary(&self) -> ComponentSummary {
        ComponentSummary {
            kind: self.kind,
            name: self.name.clone(),
            version: self.version.clone(),
            total_size: self.total_size,
            file_count: self.files.len(),
        }
    }
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
    /// The game's rule (`adapters/*.yaml`) when backing up.
    #[serde(default)]
    pub adapter: Option<String>,
    /// The game's launch options when backing up (rewritten after a restore
    /// when the game's files moved).
    #[serde(default)]
    pub args: String,
    pub chunking: Params,
    /// The user's own exclude patterns, reused by the next backup.
    #[serde(default)]
    pub user_exclude: Vec<String>,
    pub components: Vec<Component>,
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
pub struct ComponentSummary {
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    pub total_size: u64,
    pub file_count: usize,
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
    pub components: Vec<ComponentSummary>,
    pub total_size: u64,
    pub stored_size: u64,
    pub new_bytes: u64,
    pub file_count: usize,
    /// BLAKE3 of the `.manifest` file as stored.
    pub manifest_hash: String,
    pub manifest_size: u64,
    #[serde(default)]
    pub comment: String,
}

impl Manifest {
    pub fn component(&self, kind: Kind) -> Option<&Component> {
        self.components.iter().find(|c| c.kind == kind)
    }

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
            components: self.components.iter().map(Component::summary).collect(),
            total_size: self.total_size,
            stored_size: self.stored_size,
            new_bytes: self.new_bytes,
            file_count: self.components.iter().map(|c| c.files.len()).sum(),
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
        let v: serde_json::Value = serde_json::from_slice(&json)?;
        let format = v["format"].as_u64().unwrap_or(0) as u32;
        if format > FORMAT {
            bail!(crate::tr!(
                "版本 {} 是更新的 Ludonimbus 做的（格式 {format}），请先更新 Ludonimbus",
                "Version {} was made by a newer Ludonimbus (format {format}). Update Ludonimbus first",
                summary.id
            ));
        }
        if format < FORMAT {
            bail!(crate::tr!(
                "版本 {} 是测试版做的旧格式，请删掉它重新备份",
                "Version {} uses an old test format. Delete it and back up again",
                summary.id
            ));
        }
        Ok(serde_json::from_value(v)?)
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
            adapter: Some("cemu".into()),
            args: "-g \"D:\\ROMs\\g.wua\"".into(),
            chunking: Params::default(),
            user_exclude: Vec::new(),
            components: vec![Component {
                kind: Kind::Content,
                name: "Game".into(),
                version: Some("v1".into()),
                bases: BTreeMap::from([("content".into(), "D:\\ROMs".into())]),
                exe: None,
                source: Some("{content}/g.wua".into()),
                build: None,
                files: vec![
                    FileEntry { p: "{content}/a".into(), s: 3, h: "x".into(), m: 1, n: 0 },
                    FileEntry { p: "{content}/b/c".into(), s: 5, h: "y".into(), m: 2, n: 7 },
                ],
                dirs: vec!["{content}/empty".into()],
                chunks: vec![ChunkRef { h: "z".into(), s: 8, z: 14 }],
                total_size: 8,
            }],
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
        assert_eq!(summary.components[0].kind, Kind::Content);
        let back = Manifest::decode(&bytes, &summary).unwrap();
        assert_eq!(back.components[0].files, m.components[0].files);
        assert_eq!(back.components[0].file_offsets(), vec![0, 3]);
        let mut bad = bytes.clone();
        bad[0] ^= 1;
        assert!(Manifest::decode(&bad, &summary).is_err());
    }
}
