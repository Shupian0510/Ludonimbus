//! Cloud layout 2: what its files contain and where they live
//! (docs/plan-v2.md, section 3). Reading a save line through a `Store` is in
//! [`super::line`].
//!
//! Rules every type here follows (section 3.9):
//! - Fields this build doesn't know are kept in `extra` and written back, so
//!   an older build never erases what a newer one added.
//! - Values a newer build may extend (`trigger`, `platform`, `scope`) are
//!   plain strings, so reading never fails on a new one.
//! - The outer fields of a manifest (`v`, `id`, `line`, `vv`, `side`,
//!   `deviceId`, `createdAt`, `requires`) keep their meaning forever: a build
//!   that can't restore a snapshot can still tell which one is newest.

use super::store::{is_safe_name, join, Store};
use crate::vv::Vv;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The layout this build reads and writes.
pub const LAYOUT: u32 = 2;
/// The `requires` entries this build understands, in `format.json` and in
/// manifests.
pub const SUPPORTED: &[&str] = &[];

pub const FORMAT_FILE: &str = "format.json";
pub const INDEX_FILE: &str = "index.json";
pub const PIN_SUFFIX: &str = ".pin.json";

/// Fields this build doesn't know.
pub type Extra = serde_json::Map<String, serde_json::Value>;

fn is_false(b: &bool) -> bool {
    !*b
}

fn one() -> u32 {
    1
}

/// The entries of `requires` this build doesn't understand.
pub fn unknown(requires: &[String]) -> Vec<String> {
    requires.iter().filter(|r| !SUPPORTED.contains(&r.as_str())).cloned().collect()
}

// ---------------------------------------------------------------- format.json

/// `format.json` in the cloud root.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatFile {
    pub layout: u32,
    /// The first Ludonimbus version that can write this cloud (shown in the
    /// "please update" message).
    #[serde(default)]
    pub min_app: String,
    /// Features in use that a build must know to write here.
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_by: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// What this build may do with a cloud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// No `format.json`: the 0.6 layout, not migrated yet.
    Legacy,
    ReadWrite,
    /// Written by a newer Ludonimbus: never upload, change or delete anything.
    ReadOnly { layout: u32, unknown: Vec<String>, min_app: String },
}

/// `None` when the cloud has no `format.json`. A file that can't be parsed is
/// an error, never "legacy": treating a damaged marker as the old layout
/// would let this build write the wrong way.
pub fn read_format(store: &dyn Store) -> Result<Option<FormatFile>> {
    match store.read(FORMAT_FILE)? {
        None => Ok(None),
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .with_context(|| crate::tr!("云端的 format.json 读不懂", "Can't understand format.json in the cloud")),
    }
}

pub fn access(format: Option<&FormatFile>) -> Access {
    let Some(f) = format else { return Access::Legacy };
    let unknown = unknown(&f.requires);
    if f.layout > LAYOUT || !unknown.is_empty() {
        return Access::ReadOnly {
            layout: f.layout,
            unknown,
            min_app: f.min_app.clone(),
        };
    }
    if f.layout < LAYOUT {
        return Access::Legacy;
    }
    Access::ReadWrite
}

// ---------------------------------------------------------------------- paths

pub fn game_record_path(game: &str) -> String {
    join(&["library", &format!("{game}.json")])
}

/// Where a game's content packs live. `folder` is one of the record's
/// `folders`.
pub fn packs_dir(folder: &str) -> String {
    join(&["games", folder, "packs"])
}

pub fn pack_path(folder: &str, pack: &str) -> String {
    join(&[&packs_dir(folder), &format!("{pack}.zip")])
}

/// The default place of a save line's manifests (a line records its own
/// `path`, so merged and migrated lines can live elsewhere).
pub fn line_dir(folder: &str, line: &str) -> String {
    join(&["games", folder, "saves", line])
}

pub fn manifest_path(line_dir: &str, id: &str) -> String {
    join(&[line_dir, &format!("{id}.json")])
}

pub fn pin_path(line_dir: &str, id: &str) -> String {
    join(&[line_dir, &format!("{id}{PIN_SUFFIX}")])
}

pub fn index_path(line_dir: &str) -> String {
    join(&[line_dir, INDEX_FILE])
}

/// Where 0.6 keeps the snapshots of the game `key`: manifest and zip side by
/// side. A migrated line reads it as one of its folders ([`Line::also`]);
/// nothing in it is ever moved.
pub fn legacy_dir(key: &str) -> String {
    join(&["games", key, "snapshots"])
}

/// The game key when `line_dir` is a folder of the 0.6 layout
/// (`games/<key>/snapshots`).
pub fn legacy_key(line_dir: &str) -> Option<&str> {
    let mut parts = line_dir.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("games"), Some(key), Some("snapshots"), None) if !key.is_empty() => Some(key),
        _ => None,
    }
}

/// Where the versions the user deleted from `line_dir` are recorded
/// (`<id>.json` each). For a folder of the 0.6 layout that is where 0.6 has
/// its own (`games/<key>/trash`), so a device still on 0.6 sees them too.
pub fn trash_dir(line_dir: &str) -> String {
    match legacy_key(line_dir) {
        Some(key) => join(&["games", key, "trash"]),
        None => join(&[line_dir, "trash"]),
    }
}

pub fn trash_path(line_dir: &str, id: &str) -> String {
    join(&[&trash_dir(line_dir), &format!("{id}.json")])
}

/// Snapshot ids are ULIDs. Anything else in a line's folder isn't a manifest
/// (a file kind added by a later version) and is left alone.
pub fn is_snapshot_id(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

// ---------------------------------------------------------------- game record

/// `library/<game id>.json`: the game one card stands for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRecord {
    pub id: String,
    pub name: String,
    /// The game's identities on each platform, e.g. `steam:413150`.
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Data folders under `games/`. New data goes to the first; all are read.
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Set on the record that lost when two devices created the same game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
    /// When the record was made. Of two records for the same game, the one
    /// made first stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub rev: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl GameRecord {
    /// Refuse a record whose places in the cloud aren't plain ones: its id
    /// and each of its folders one name, each line's path and the places it
    /// reads relative paths with no `..`, empty segment, `\` or drive. Every
    /// read and write of the game's saves goes where these say, and a record
    /// is a file anyone who can write to the cloud can change.
    pub fn check(&self) -> Result<()> {
        let path = |p: &str| !p.is_empty() && p.split('/').all(is_safe_name);
        let names = std::iter::once(&self.id).chain(&self.folders).find(|n| !is_safe_name(n));
        let paths = || self.lines.iter().flat_map(|l| std::iter::once(&l.path).chain(&l.also)).find(|p| !path(p));
        match names.or_else(paths) {
            None => Ok(()),
            Some(bad) => bail!(crate::tr!(
                "云端的游戏记录 {} 里有不安全的路径：{}",
                "The game record {} in the cloud has an unsafe path: {}",
                self.id,
                bad
            )),
        }
    }
}

pub const SCOPE_SHARED: &str = "shared";
pub const SCOPE_INDEPENDENT: &str = "independent";

/// A save line: saves that can read each other and share one history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub id: String,
    /// Which set of canonical paths its snapshots use: `pc`, `nx`,
    /// `adapter:<rule>` or `rule:<id>`.
    pub format: String,
    #[serde(default = "shared")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where new manifests are written.
    pub path: String,
    /// Other places that are only read: the folder from before the
    /// migration, a line merged into this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,
    /// Which files its saves are: a fingerprint of where the game they were
    /// first uploaded from looks for them ([`super::upgrade::line_def`]).
    /// Two lines of one format with the same value hold the same files, so
    /// one history can serve both. Without a value nothing is known, and
    /// nothing is assumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def: Option<String>,
    /// Versions of the game that keep these saves in other folders, and
    /// which folder of theirs is which folder here: what a user confirmed
    /// once on one device, so that the next device with such a version
    /// joins without asking (docs/plan-v2.md, section 6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binds: Vec<LineBind>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How the versions of a game that look for their saves in one way take
/// part in a save line whose files are kept in another.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineBind {
    /// Which files those versions keep ([`Line::def`] of a line of theirs).
    pub def: String,
    pub map: Vec<crate::bind::Bind>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn shared() -> String {
    SCOPE_SHARED.to_string()
}

impl Line {
    pub fn is_independent(&self) -> bool {
        self.scope == SCOPE_INDEPENDENT
    }
}

// ------------------------------------------------------------------- manifest

/// A snapshot: the list of its files and where their content is. Written
/// once, never changed. A snapshot of the 0.6 layout parses as one too (all
/// its files are in its own pack; `roots`, `comment` and `locked` end up in
/// `extra`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default = "one")]
    pub v: u32,
    pub id: String,
    pub game: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub line: String,
    #[serde(default, skip_serializing_if = "Vv::is_empty")]
    pub vv: Vv,
    /// For display only; what is newest comes from `vv`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub device_id: String,
    pub device_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    pub created_at: DateTime<Utc>,
    pub trigger: String,
    /// Kept beside the history and never the newest version (saves from
    /// before a restore, the side that lost a conflict).
    #[serde(default, skip_serializing_if = "is_false")]
    pub side: bool,
    pub content_hash: String,
    pub total_size: u64,
    pub files: Vec<FileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Info>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Features a build must know to restore this snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app_version: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    /// Canonical path, e.g. `saves/Farm_1/SaveGameInfo`.
    pub path: String,
    pub size: u64,
    pub hash: String,
    #[serde(default)]
    pub mtime: i64,
    /// The pack holding this file's content; `None` = the snapshot's own
    /// pack (`packs/<snapshot id>.zip`).
    #[serde(rename = "in", default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// What the saves say about themselves, where a rule knows how to read it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Where a snapshot made by restoring, copying or converting came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub line: String,
    pub snapshot: String,
    /// `restore`, `copy`, `convert:<converter>`, or `share` (taken in from
    /// a file someone shared: no game, line or snapshot to name).
    pub how: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Fields of a 0.6 snapshot that v2 has no field for (they are in `extra`).
const LOCKED: &str = "locked";
const COMMENT: &str = "comment";

fn side_trigger(trigger: &str) -> bool {
    matches!(trigger, "pre-restore" | "conflict-keep")
}

impl Manifest {
    /// Never the newest version. 0.6 snapshots have no `side` field; there
    /// the trigger says it.
    pub fn is_side(&self) -> bool {
        self.side || side_trigger(&self.trigger)
    }

    /// The packs this snapshot needs.
    pub fn packs(&self) -> BTreeSet<String> {
        self.files.iter().map(|f| f.pack.clone().unwrap_or_else(|| self.id.clone())).collect()
    }

    /// The features this build lacks to restore it; empty = restorable.
    pub fn unsupported(&self) -> Vec<String> {
        unknown(&self.requires)
    }

    /// The manifest without its file list, as kept in a line's index.
    pub fn summary(&self) -> IndexEntry {
        // What a 0.6 snapshot says in its own file and has to follow it
        // into the index. The lock: a locked snapshot has no pin until the
        // migration writes one, and retention must not take it for an
        // automatic snapshot. The note: v2 has names for kept saves only, and
        // a note on any other old version would no longer be shown.
        let mut extra = Extra::new();
        if self.extra.get(LOCKED).and_then(|v| v.as_bool()) == Some(true) {
            extra.insert(LOCKED.to_string(), true.into());
        }
        if let Some(note) = self.extra.get(COMMENT).and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty()) {
            extra.insert(COMMENT.to_string(), note.into());
        }
        IndexEntry {
            id: self.id.clone(),
            vv: self.vv.clone(),
            parent: self.parent.clone(),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            platform: self.platform.clone(),
            created_at: self.created_at,
            trigger: self.trigger.clone(),
            side: self.side,
            content_hash: self.content_hash.clone(),
            total_size: self.total_size,
            file_count: self.files.len(),
            packs: self.packs().into_iter().collect(),
            game_version: self.game_version.clone(),
            installed_version: self.installed_version.clone(),
            info: self.info.clone(),
            origin: self.origin.clone(),
            requires: self.requires.clone(),
            pin: None,
            extra,
        }
    }
}

// ------------------------------------------------------------------------ pin

/// `<snapshot id>.pin.json`: a snapshot the user keeps. While the file
/// exists the snapshot is never cleaned up; `name` may be empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A pin as remembered by an index or a local cache: valid while the pin
/// file's tag in the folder listing is still `tag`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tag: String,
}

// ---------------------------------------------------------------------- index

/// `index.json` of a save line: a summary of every snapshot, so reading a
/// line takes two requests however long it is. Only a cache: the folder
/// listing says what exists, the manifests say what it is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineIndex {
    #[serde(default = "one")]
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    #[serde(default)]
    pub snapshots: Vec<IndexEntry>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vv::is_empty")]
    pub vv: Vv,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub device_id: String,
    pub device_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    pub created_at: DateTime<Utc>,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub side: bool,
    pub content_hash: String,
    pub total_size: u64,
    #[serde(default)]
    pub file_count: usize,
    /// The packs the snapshot needs (deleting a pack needs to know who still
    /// uses it without reading every manifest).
    #[serde(default)]
    pub packs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Info>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Set when the user keeps this snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<PinRef>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl IndexEntry {
    pub fn is_side(&self) -> bool {
        self.side || side_trigger(&self.trigger)
    }

    pub fn unsupported(&self) -> Vec<String> {
        unknown(&self.requires)
    }

    /// Locked on a device running 0.6. Once the migration has run, the pin
    /// it wrote says "kept", not this.
    pub fn was_locked(&self) -> bool {
        self.extra.get(LOCKED).and_then(|v| v.as_bool()) == Some(true)
    }

    /// The note 0.6 kept in the snapshot's own file.
    pub fn old_note(&self) -> Option<&str> {
        self.extra.get(COMMENT).and_then(|v| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A snapshot's `.json` as 0.6 writes it.
    fn v1_meta() -> serde_json::Value {
        json!({
            "id": "01JD8X3K2M0000000000000001",
            "game": "stardew",
            "parent": null,
            "deviceId": "d-01ja",
            "deviceName": "Desktop",
            "createdAt": "2026-11-20T10:00:00Z",
            "trigger": "auto-exit",
            "roots": { "appdata": "C:\\Users\\me\\AppData\\Roaming" },
            "contentHash": "abc",
            "files": [
                { "path": "appdata/StardewValley/Saves/Farm_1/Farm_1", "size": 5, "hash": "h1", "mtime": 1732100000 },
                { "path": "appdata/StardewValley/Saves/Farm_1/SaveGameInfo", "size": 1, "hash": "h2", "mtime": 1732100000 }
            ],
            "totalSize": 6,
            "appVersion": "0.6.2",
            "comment": "before the festival",
            "locked": true
        })
    }

    #[test]
    fn a_v1_snapshot_is_a_manifest_with_one_pack() {
        let m: Manifest = serde_json::from_value(v1_meta()).unwrap();
        assert_eq!(m.v, 1);
        assert!(m.vv.is_empty());
        assert_eq!(m.packs().into_iter().collect::<Vec<_>>(), vec![m.id.clone()]);
        assert!(!m.is_side());
        // What v2 no longer has a field for is still there.
        assert_eq!(m.extra["comment"], "before the festival");
        assert_eq!(m.extra["locked"], true);
        assert!(m.extra.contains_key("roots"));

        let s = m.summary();
        assert!(s.was_locked());
        assert_eq!(s.old_note(), Some("before the festival"));

        let mut side = v1_meta();
        side["trigger"] = json!("pre-restore");
        assert!(serde_json::from_value::<Manifest>(side).unwrap().is_side());
    }

    #[test]
    fn files_can_live_in_other_packs() {
        let m: Manifest = serde_json::from_value(json!({
            "v": 2, "id": "01JF0000000000000000000002", "game": "g", "line": "main",
            "vv": { "d-a": 12, "d-b": 4 },
            "deviceId": "d-a", "deviceName": "Desktop", "platform": "steam",
            "createdAt": "2026-11-26T10:00:00Z", "trigger": "auto-exit",
            "contentHash": "c", "totalSize": 3,
            "files": [
                { "path": "saves/a", "size": 1, "hash": "1" },
                { "path": "saves/b", "size": 1, "hash": "2", "in": "01JE0000000000000000000001" },
                { "path": "saves/c", "size": 1, "hash": "3", "in": "01JF0000000000000000000002-2" }
            ]
        }))
        .unwrap();
        let packs: Vec<String> = m.packs().into_iter().collect();
        assert_eq!(
            packs,
            ["01JE0000000000000000000001", "01JF0000000000000000000002", "01JF0000000000000000000002-2"]
        );
        let s = m.summary();
        assert_eq!(s.packs, packs);
        assert_eq!(s.file_count, 3);
        assert_eq!(s.vv.get("d-a"), 12);
        // `in` is written back under its own name, and only where it was set.
        let out = serde_json::to_value(&m).unwrap();
        assert!(out["files"][0].get("in").is_none());
        assert_eq!(out["files"][1]["in"], "01JE0000000000000000000001");
    }

    #[test]
    fn unknown_fields_and_values_survive_a_rewrite() {
        // A manifest from a later version: a new trigger, a new feature, new
        // fields at every level.
        let future = json!({
            "v": 3, "id": "01JG0000000000000000000003", "game": "g", "line": "main",
            "vv": { "d-a": 1 },
            "deviceId": "d-a", "deviceName": "Deck", "platform": "holo:next",
            "createdAt": "2027-01-01T00:00:00Z", "trigger": "scheduled",
            "contentHash": "c", "totalSize": 9,
            "files": [ { "path": "saves/world", "size": 9, "hash": "h", "mtime": 0, "chunks": ["aa", "bb"] } ],
            "requires": ["chunks"],
            "encryption": { "scheme": "enc1" }
        });
        let m: Manifest = serde_json::from_value(future.clone()).unwrap();
        assert_eq!(m.unsupported(), vec!["chunks".to_string()]);
        assert!(!m.is_side());
        assert_eq!(serde_json::to_value(&m).unwrap(), future);

        let record = json!({
            "id": "stardew-valley-7k3f", "name": "Stardew Valley",
            "ids": ["steam:413150", "nx:0100E65002BB8000"],
            "rule": "stardew-valley",
            "folders": ["stardew-valley-7k3f"],
            "lines": [
                { "id": "main", "format": "rule:stardew-valley", "scope": "shared",
                  "path": "games/stardew-valley-7k3f/saves/main", "owner": "family" },
                { "id": "solo-9d2k", "format": "rule:stardew-valley", "scope": "independent",
                  "name": "Kid's PC", "path": "games/stardew-valley-7k3f/saves/solo-9d2k" }
            ],
            "tags": ["finished"],
            "rev": 12, "updatedAt": "2026-11-26T10:00:00Z", "updatedBy": "d-a",
            "collection": { "shelf": 2 }
        });
        let r: GameRecord = serde_json::from_value(record.clone()).unwrap();
        assert!(!r.lines[0].is_independent());
        assert!(r.lines[1].is_independent());
        assert_eq!(r.lines[0].extra["owner"], "family");
        assert_eq!(serde_json::to_value(&r).unwrap(), record);
    }

    #[test]
    fn access_follows_layout_and_features() {
        assert_eq!(access(None), Access::Legacy);
        let mut f = FormatFile { layout: LAYOUT, min_app: "1.0.0".into(), ..Default::default() };
        assert_eq!(access(Some(&f)), Access::ReadWrite);
        f.requires = vec!["enc1".into()];
        assert_eq!(
            access(Some(&f)),
            Access::ReadOnly { layout: LAYOUT, unknown: vec!["enc1".into()], min_app: "1.0.0".into() }
        );
        let newer = FormatFile { layout: LAYOUT + 1, min_app: "2.0.0".into(), ..Default::default() };
        assert!(matches!(access(Some(&newer)), Access::ReadOnly { layout, .. } if layout == LAYOUT + 1));
    }

    #[test]
    fn paths_and_ids() {
        assert_eq!(game_record_path("stardew-7k3f"), "library/stardew-7k3f.json");
        assert_eq!(pack_path("stardew-7k3f", "01JF"), "games/stardew-7k3f/packs/01JF.zip");
        let dir = line_dir("stardew-7k3f", "main");
        assert_eq!(dir, "games/stardew-7k3f/saves/main");
        assert_eq!(manifest_path(&dir, "01JF"), "games/stardew-7k3f/saves/main/01JF.json");
        assert_eq!(pin_path(&dir, "01JF"), "games/stardew-7k3f/saves/main/01JF.pin.json");
        assert_eq!(index_path(&dir), "games/stardew-7k3f/saves/main/index.json");
        let old = legacy_dir("stardew");
        assert_eq!(old, "games/stardew/snapshots");
        assert_eq!(legacy_key(&old), Some("stardew"));
        assert_eq!(legacy_key(&dir), None);
        assert_eq!(legacy_key("games/stardew/saves/snapshots"), None);
        assert_eq!(trash_path(&old, "01JF"), "games/stardew/trash/01JF.json");
        assert_eq!(trash_path(&dir, "01JF"), "games/stardew-7k3f/saves/main/trash/01JF.json");
        assert!(is_snapshot_id("01JD8X3K2M0000000000000001"));
        assert!(!is_snapshot_id("index"));
        assert!(!is_snapshot_id("01JD8X3K2M0000000000000001.pin"));
    }
}
