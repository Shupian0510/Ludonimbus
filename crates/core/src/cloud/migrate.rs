//! Copy everything from one cloud to another when the user switches clouds.
//!
//! Files are copied as they are, under the names they have. Snapshots keep
//! their ids, so each device's "last synced" state stays valid after the
//! switch. What the target already has is skipped, so an interrupted copy
//! can simply be run again. The source is never changed.
//!
//! Content goes before what refers to it (packs and zips before manifests),
//! so the target never shows a version it can't restore. Indexes are not
//! copied: they are caches. Game records the target already has are merged
//! with ([`library::save`]).
//!
//! At the end the target is brought to the current layout
//! ([`upgrade::run`]): a target that only got what 0.6 wrote becomes a cloud
//! of layout 2 the way the first sync would have made it one, and every line
//! gets its index.
//!
//! Playtime, screenshots and artwork are not copied here: each device
//! uploads its own again after the switch.

use super::library;
use super::store::{join, Entry, Store};
use super::upgrade;
use super::v2::{self, Access};
use crate::paths;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashSet;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "MigrateProgress")]
pub struct Progress {
    pub game: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "MigrateReport")]
pub struct Report {
    /// Games with at least one version in the old cloud.
    pub games: usize,
    /// Versions copied, and versions the new cloud had already.
    pub copied: usize,
    pub skipped: usize,
    pub bytes: u64,
}

/// One file to copy.
struct Item {
    game: String,
    path: String,
    size: u64,
    /// The manifest of a version (what [`Report::copied`] counts).
    version: bool,
}

/// A folder as both clouds have it: the files of the source, and the names
/// the target has.
struct Both {
    files: Vec<Entry>,
    have: HashSet<String>,
}

fn both(from: &dyn Store, to: &dyn Store, dir: &str) -> Result<Both> {
    let mut files: Vec<Entry> = from.list(dir)?.into_iter().filter(|e| !e.is_dir).collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let have = if files.is_empty() { HashSet::new() } else { to.list(dir)?.into_iter().map(|e| e.name).collect() };
    Ok(Both { files, have })
}

/// Whether a file of a line's folder is a version (not a pin, not the index).
fn is_manifest(name: &str) -> bool {
    name.ends_with(".json") && !name.ends_with(v2::PIN_SUFFIX) && name != v2::INDEX_FILE
}

/// Everything of one game that the target lacks, in the order it is copied.
fn plan_game(from: &dyn Store, to: &dyn Store, key: &str, work: &mut Vec<Item>, report: &mut Report) -> Result<()> {
    let base = join(&["games", key]);
    let mut versions = 0usize;
    let mut add = |dir: &str, e: &Entry, have: &HashSet<String>, version: bool, work: &mut Vec<Item>, report: &mut Report| {
        if have.contains(&e.name) {
            report.skipped += usize::from(version);
        } else {
            work.push(Item { game: key.to_string(), path: join(&[dir, &e.name]), size: e.size, version });
        }
        versions += usize::from(version);
    };

    // Content first: packs, then what 0.6 kept beside its manifests.
    let packs = v2::packs_dir(key);
    let b = both(from, to, &packs)?;
    for e in b.files.iter().filter(|e| e.name.ends_with(".zip")) {
        add(&packs, e, &b.have, false, work, report);
    }
    let old = v2::legacy_dir(key);
    let b = both(from, to, &old)?;
    for e in b.files.iter().filter(|e| e.name.ends_with(".zip")) {
        add(&old, e, &b.have, false, work, report);
    }
    for e in b.files.iter().filter(|e| e.name.ends_with(".json")) {
        add(&old, e, &b.have, is_manifest(&e.name), work, report);
    }
    let old_trash = join(&[&base, "trash"]);
    let b = both(from, to, &old_trash)?;
    for e in b.files.iter().filter(|e| e.name.ends_with(".json")) {
        add(&old_trash, e, &b.have, false, work, report);
    }

    // The save lines: manifests, pins, what was deleted. Not the index.
    let saves = join(&[&base, "saves"]);
    let mut lines: Vec<String> = from.list(&saves)?.into_iter().filter(|e| e.is_dir).map(|e| e.name).collect();
    lines.sort();
    for line in lines {
        let dir = join(&[&saves, &line]);
        let b = both(from, to, &dir)?;
        for e in b.files.iter().filter(|e| e.name.ends_with(".json") && e.name != v2::INDEX_FILE) {
            add(&dir, e, &b.have, is_manifest(&e.name), work, report);
        }
        let trash = v2::trash_dir(&dir);
        let b = both(from, to, &trash)?;
        for e in b.files.iter().filter(|e| e.name.ends_with(".json")) {
            add(&trash, e, &b.have, false, work, report);
        }
    }
    report.games += usize::from(versions > 0);
    Ok(())
}

fn refuse_newer(store: &dyn Store) -> Result<()> {
    if let Access::ReadOnly { min_app, .. } = v2::access(v2::read_format(store)?.as_ref()) {
        bail!(crate::tr!(
            "云端的数据是更新版本的 Ludonimbus 写的（需要 {} 或更高）。请先更新这台设备上的 Ludonimbus",
            "The data in the cloud was written by a newer Ludonimbus ({} or later is needed). Update Ludonimbus on this device first",
            if min_app.is_empty() { "?".to_string() } else { min_app }
        ));
    }
    Ok(())
}

/// Copy the games' save data, the game records and the devices from `from`
/// to `to`, then bring `to` to the current layout. `device` is this
/// device's id and name.
pub fn copy_all(from: &dyn Store, to: &dyn Store, device: (&str, &str), mut progress: impl FnMut(&Progress)) -> Result<Report> {
    from.check().context(crate::tr!("无法访问原来的云盘", "Can't reach the old cloud"))?;
    to.check().context(crate::tr!("无法访问新的云盘", "Can't reach the new cloud"))?;
    refuse_newer(from)?;
    refuse_newer(to)?;
    let mut report = Report::default();

    let mut keys: Vec<String> = from.list("games")?.into_iter().filter(|e| e.is_dir).map(|e| e.name).collect();
    keys.sort();
    let mut work = Vec::new();
    for key in &keys {
        plan_game(from, to, key, &mut work, &mut report)?;
    }

    let tmp = paths::staging_dir().join(format!("migrate-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<()> {
        for (i, item) in work.iter().enumerate() {
            progress(&Progress { game: item.game.clone(), done: i, total: work.len() });
            if item.path.ends_with(".zip") {
                let file = tmp.join("file.zip");
                from.download(&item.path, &file)?;
                let sent = to.upload(&item.path, &file);
                let _ = std::fs::remove_file(&file);
                sent?;
            } else {
                // Gone meanwhile: deleted on another device.
                let Some(bytes) = from.read(&item.path)? else { continue };
                to.write(&item.path, &bytes)?;
            }
            report.bytes += item.size;
            report.copied += usize::from(item.version);
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    progress(&Progress { game: String::new(), done: work.len(), total: work.len() });

    // The games' save rules as 0.6 keeps them: the source's are the current
    // ones.
    for key in &keys {
        let path = join(&["games", key, "game.json"]);
        if let Some(bytes) = from.read(&path)? {
            to.write(&path, &bytes)?;
        }
    }

    // Game records: as they are where the target has none, merged where it
    // has one.
    let now = Utc::now();
    let b = both(from, to, library::DIR)?;
    for e in b.files.iter().filter(|e| e.name.ends_with(".json")) {
        let path = join(&[library::DIR, &e.name]);
        let Some(bytes) = from.read(&path)? else { continue };
        if !b.have.contains(&e.name) {
            to.write(&path, &bytes)?;
        } else if let Ok(record) = serde_json::from_slice::<v2::GameRecord>(&bytes) {
            library::save(to, &record, device.0, now)?;
        }
    }

    for e in from.list("devices").unwrap_or_default() {
        if e.is_dir || !e.name.ends_with(".json") {
            continue;
        }
        let path = join(&["devices", &e.name]);
        if let Ok(Some(bytes)) = from.read(&path) {
            let _ = to.write(&path, &bytes);
        }
    }

    upgrade::run(to, device.0, device.1, now, &mut |_| {})?;
    Ok(report)
}
