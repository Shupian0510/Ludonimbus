//! Sharing one save with someone else (docs/plan-v2.md, section 9).
//!
//! A `.lnsave` file is a zip: `share.json` (which game, what the save was
//! made on, its files with their hashes) and the files as `files/<path>`,
//! under the paths of the save line they are from. Nothing in it says whose
//! it was: no device, no account, no folder of the PC.
//!
//! Taking one in never changes the saves on the device, and the save never
//! becomes the newest version by itself: it goes beside the history of the
//! game's line, kept, and its new owner makes it current when they want it.

use crate::cloud::v2::Origin;
use crate::config::GameEntry;
use crate::snapshot::{self, FileEntry, SnapshotMeta, Trigger};
use crate::sync::{self, Ctx, Foreign};
use crate::{paths, savelib};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// The file extension of a shared save.
pub const EXT: &str = "lnsave";
/// The version of the file's layout this Ludonimbus writes and reads.
pub const FORMAT: u32 = 1;
const INFO: &str = "share.json";
/// The largest save taken in: its files are held in memory on the way.
const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// The device a shared save is said to be from.
const DEVICE: &str = "shared";

/// What a shared save says about itself (`share.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub format: u32,
    /// The game, by name and by what identifies it ([`savelib::ids`]).
    pub name: String,
    pub ids: Vec<String>,
    /// What the save was made on, e.g. `pc:steam`.
    #[serde(default)]
    pub platform: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    /// The name the save was kept under, if it had one.
    #[serde(default)]
    pub note: String,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub app_version: String,
}

/// Write version `id` of the game's line to `dest` as a shared save.
pub fn export(ctx: &Ctx, game: &GameEntry, id: &str, dest: &Path) -> Result<Share> {
    export_as(ctx, &game.key, &game.name, savelib::ids(game), id, dest)
}

/// [`export`] for a game known by its key alone (one only the cloud has):
/// its name and what identifies it are the caller's to give.
pub fn export_as(ctx: &Ctx, key: &str, name: &str, ids: Vec<String>, id: &str, dest: &Path) -> Result<Share> {
    let cloud = ctx.cloud()?;
    let summary = cloud.list_snapshots(key)?.into_iter().find(|s| s.id == id);
    let work = paths::staging_dir().join(format!("share-{}.zip", ulid::Ulid::new()));
    let result = (|| -> Result<Share> {
        cloud.download(key, id, &work)?;
        let meta = snapshot::read_meta(&work)?;
        let share = Share {
            format: FORMAT,
            name: name.to_string(),
            ids,
            platform: summary.as_ref().map(|s| s.platform.clone()).unwrap_or_default(),
            created_at: meta.created_at,
            game_version: meta.game_version.clone(),
            note: summary.as_ref().map(|s| s.comment.clone()).unwrap_or_default(),
            total_size: meta.total_size,
            // When a file was last written says nothing a friend needs.
            files: meta.files.into_iter().map(|f| FileEntry { mtime: 0, ..f }).collect(),
            app_version: crate::APP_VERSION.to_string(),
        };
        // The files as they are in the zip; the description of the version
        // (who made it, on which device, in which folders) stays behind.
        let mut from = zip::ZipArchive::new(File::open(&work)?)?;
        let mut to = zip::ZipWriter::new(File::create(dest).with_context(|| crate::tr!("无法创建 {}", "Can't create {}", dest.display()))?);
        for i in 0..from.len() {
            let entry = from.by_index_raw(i)?;
            if entry.name().starts_with("files/") {
                to.raw_copy_file(entry)?;
            }
        }
        to.start_file(INFO, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
        to.write_all(&serde_json::to_vec_pretty(&share)?)?;
        to.finish()?;
        Ok(share)
    })();
    let _ = std::fs::remove_file(&work);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn damaged() -> anyhow::Error {
    anyhow!(crate::tr!("这个文件不是 Ludonimbus 分享的存档，或者已经损坏", "This file isn't a save shared from Ludonimbus, or it is damaged"))
}

/// What the shared save at `path` says about itself.
pub fn read(path: &Path) -> Result<Share> {
    let mut archive = zip::ZipArchive::new(File::open(path)?).map_err(|_| damaged())?;
    let mut text = String::new();
    archive.by_name(INFO).map_err(|_| damaged())?.read_to_string(&mut text).map_err(|_| damaged())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| damaged())?;
    if value["format"].as_u64().is_some_and(|f| f > u64::from(FORMAT)) {
        bail!(crate::tr!("这份存档是用更新的 Ludonimbus 分享的，请先更新", "This save was shared from a newer Ludonimbus. Update first"));
    }
    serde_json::from_value(value).map_err(|_| damaged())
}

/// Whether the shared save is of this game: they have something that
/// identifies a game in common (a Switch title, a title in Ludusavi's
/// database, an Xbox package, a save format, the name).
fn same_game(share: &Share, game: &GameEntry) -> bool {
    let mine = savelib::ids(game);
    share.ids.iter().any(|id| mine.iter().any(|m| m.eq_ignore_ascii_case(id)))
}

/// The files of the shared save this version of the game has no folder
/// for: outside its bindings, or where a scan of its saves here would never
/// find them. The file comes from someone else, and its paths are where its
/// files would go once it is made current.
fn no_place(share: &Share, game: &GameEntry) -> Result<Vec<String>> {
    let mut out = snapshot::no_place_for(game, &share.files);
    // Each file where this version keeps it.
    let own: Vec<(&str, String)> = share
        .files
        .iter()
        .filter_map(|f| {
            let local = if game.bind.is_empty() { Some(f.path.clone()) } else { crate::bind::to_local(&game.bind, &f.path) };
            Some((f.path.as_str(), local?))
        })
        .collect();
    let outside = match snapshot::outside_saves(game, own.iter().map(|(_, l)| l.as_str()))? {
        Some(outside) => outside,
        // Ludusavi finds the saves, and its database has no save locations
        // of the game here: only the roots can be checked.
        None => {
            let roots = snapshot::scan(game).map(|(_, roots)| roots).unwrap_or_default();
            if roots.is_empty() {
                return Ok(out);
            }
            own.iter().filter(|(_, l)| !roots.keys().any(|r| r.eq_ignore_ascii_case(l.split('/').next().unwrap_or_default()))).map(|(_, l)| l.clone()).collect()
        }
    };
    for (path, local) in &own {
        if outside.contains(local) && !out.iter().any(|o| o == path) {
            out.push(path.to_string());
        }
    }
    Ok(out)
}

/// Take the shared save at `path` into the line of `game`: a version beside
/// its history, kept under the save's name. The saves on this device and
/// the line's newest version stay as they are.
pub fn import(ctx: &Ctx, game: &GameEntry, path: &Path) -> Result<SnapshotMeta> {
    let share = read(path)?;
    if !same_game(&share, game) {
        bail!(crate::tr!("这份存档是「{}」的，不是「{}」的", "This save is of {}, not of {}", share.name, game.name));
    }
    let alien = no_place(&share, game)?;
    if !alien.is_empty() {
        bail!(crate::tr!(
            "这份存档和这个版本的存档格式不一样：{} 个文件在这里没有对应的位置（如 {}）",
            "This save isn't in the format of this version of the game: {} files have no place here (e.g. {})",
            alien.len(),
            alien[0]
        ));
    }
    // The sizes are what the file says: added up without wrapping around.
    let total = share.files.iter().try_fold(0u64, |sum, f| sum.checked_add(f.size));
    if total.is_none_or(|t| t > MAX_BYTES) {
        bail!(crate::tr!("这份存档太大了", "This save is too large"));
    }

    // Every file, whole and as its hash says. No more than a byte past the
    // size it is said to have is read: that is enough to tell it isn't.
    let mut archive = zip::ZipArchive::new(File::open(path)?).map_err(|_| damaged())?;
    let mut files = Vec::with_capacity(share.files.len());
    for f in &share.files {
        let mut data = Vec::new();
        archive.by_name(&format!("files/{}", f.path)).map_err(|_| damaged())?.take(f.size + 1).read_to_end(&mut data).map_err(|_| damaged())?;
        if data.len() as u64 != f.size || blake3::hash(&data).to_hex().as_str() != f.hash {
            return Err(damaged());
        }
        files.push((f.path.clone(), data));
    }
    if files.is_empty() {
        return Err(damaged());
    }

    let cloud = ctx.cloud()?;
    let line = cloud.open(&game.key)?;
    let name = if share.note.trim().is_empty() { crate::tr!("分享的存档", "Shared save") } else { share.note.trim().to_string() };
    let meta = sync::queue_files(
        &line,
        &Foreign {
            key: &game.key,
            files: &files,
            trigger: Trigger::Shared,
            parent: None,
            over_all: false,
            device: (DEVICE, &crate::tr!("分享", "Shared")),
            platform: &share.platform,
            comment: "",
            game_version: share.game_version.clone(),
            origin: Some(Origin { how: "share".to_string(), ..Default::default() }),
        },
    )?;
    // In the cloud at once: it isn't this device's version, and a failed
    // upload leaves nothing half done (the file can be taken in again).
    let uploaded = sync::upload_pending(cloud, &meta);
    sync::drop_pending(&meta)?;
    uploaded?;
    // Kept, so retention never takes it away and it shows among the saves.
    sync::set_snapshot_flags(ctx, game, &meta.id, Some(true), Some(name))?;
    Ok(meta)
}
