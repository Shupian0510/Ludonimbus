//! Achievements for non-Steam games: definitions and unlocks read from
//! RetroAchievements or from emulators' local trophy files, merged across
//! devices and shown in the app and the Steam plugin.
//!
//! ```text
//! %APPDATA%\Ludonimbus\achievements\settings.json    RA user, per-game sources, popup settings
//! %APPDATA%\Ludonimbus\achievements\<key>.json       cached definitions + merged unlocks
//! cloud: games/<key>/achievements/<device-id>.json   this device's unlocks (append-only)
//!        games/<key>/achievements/schema.json        definitions (icons stay local)
//! ```
//!
//! - [`retro`]  RetroAchievements Web API (read-only, user's own key)
//! - [`trophy`] PS trophy files: shadPS4, Lantern Engine, RPCS3
//! - [`detect`] finds a trophy source for a game from its adapter
//! - [`cloud`]  per-device unlock files in the cloud
//!
//! Plan and research: `docs/plan-achievements.md`.

pub mod cloud;
pub mod detect;
pub mod retro;
pub mod rules;
pub mod trophy;
pub mod trp;

use crate::config::{AppConfig, GameEntry};
use crate::lockfile::FileLock;
use crate::{paths, util};
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Retroachievements,
    Shadps4,
    Lantern,
    Rpcs3,
    /// A PS4 port / recompilation in its own folder.
    Ps4port,
    /// Save rules (TrophyNX format).
    Rules,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Retroachievements => "RetroAchievements",
            SourceKind::Shadps4 => "shadPS4",
            SourceKind::Lantern => "Lantern Engine",
            SourceKind::Rpcs3 => "RPCS3",
            SourceKind::Ps4port => "PS4 port",
            SourceKind::Rules => "TrophyNX",
        }
    }
}

/// What the interface calls a binding's source: Ludonimbus's own save rules
/// are told apart from TrophyNX's.
pub fn binding_label(b: &Binding) -> &'static str {
    if b.source == SourceKind::Rules && !rules::is_title(&b.source_id) {
        "Ludonimbus"
    } else {
        b.source.label()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Platinum,
    Gold,
    Silver,
    Bronze,
}

impl Grade {
    /// PS trophy type letter (`ttype`): P, G, S, B.
    pub fn from_letter(s: &str) -> Option<Grade> {
        match s.trim().to_ascii_uppercase().as_str() {
            "P" => Some(Grade::Platinum),
            "G" => Some(Grade::Gold),
            "S" => Some(Grade::Silver),
            "B" => Some(Grade::Bronze),
            _ => None,
        }
    }
}

/// Where a game's achievements come from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub source: SourceKind,
    /// RetroAchievements game id, or the trophy set id (`NPWR12345_00`).
    #[serde(default)]
    pub source_id: String,
    /// Trophy sources: the emulator's `user` folder (shadPS4 / Lantern) or
    /// the trophy set folder (RPCS3).
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Show unlocks with Steam's achievement popup; `None` = the default.
    #[serde(default)]
    pub popup: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// RetroAchievements user name (the Web API key is in the credential store).
    pub ra_user: Option<String>,
    /// Stable RA user id, learned from the first successful request.
    pub ra_ulid: Option<String>,
    pub games: BTreeMap<String, Binding>,
    /// Default for [`Binding::popup`].
    pub steam_popup: bool,
    /// Ludonimbus's own topmost window when Steam could not show the popup.
    pub fallback_popup: bool,
    /// Games the user unlinked: never linked automatically again.
    pub unlinked: BTreeSet<String>,
    /// PS4 trophy key the user entered (32 hex characters), to read trophy
    /// names from their own games' `trophy00.trp`. Only in this PC's
    /// settings file: never uploaded, never shipped with Ludonimbus.
    pub ps4_trophy_key: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ra_user: None,
            ra_ulid: None,
            games: BTreeMap::new(),
            steam_popup: true,
            fallback_popup: true,
            unlinked: BTreeSet::new(),
            ps4_trophy_key: None,
        }
    }
}

impl Settings {
    pub fn popup_for(&self, key: &str) -> bool {
        self.games.get(key).and_then(|b| b.popup).unwrap_or(self.steam_popup)
    }
}

/// One achievement definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `https://…` for RetroAchievements, an absolute local path for trophies.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub points: Option<u32>,
    #[serde(default)]
    pub grade: Option<Grade>,
}

/// A game's achievement definitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    pub source: SourceKind,
    pub source_id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub achievements: Vec<Achievement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Unlock {
    pub id: String,
    pub unlocked_at: DateTime<Utc>,
    #[serde(default)]
    pub hardcore: bool,
}

/// What a source reported.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub schema: Option<Schema>,
    pub unlocks: Vec<Unlock>,
    /// Ids to record without a popup (e.g. already earned in a loaded save).
    pub quiet: Vec<String>,
}

/// A merged unlock, with the device that got it first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub unlocked_at: DateTime<Utc>,
    #[serde(default)]
    pub hardcore: bool,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub device_name: String,
}

/// Local database of one game.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameDb {
    pub schema: Option<Schema>,
    /// Merged over all devices.
    pub unlocks: BTreeMap<String, Record>,
    /// Found on this device: what goes into this device's cloud file.
    pub mine: BTreeMap<String, Unlock>,
    /// Ids that were announced (or existed before the source was linked).
    pub notified: BTreeSet<String>,
    /// Hash of the last uploaded device file / schema.
    pub uploaded: Option<String>,
    pub uploaded_schema: Option<String>,
    /// Change tags of other devices' cloud files already merged.
    pub remote_tags: BTreeMap<String, String>,
    pub checked_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// The source was read successfully at least once (what it had then is
    /// history, not news).
    pub initialized: bool,
}

impl GameDb {
    /// Merge one unlock; the earliest time wins. Returns true when it changed.
    pub fn merge(&mut self, u: &Unlock, device: &str, device_name: &str) -> bool {
        match self.unlocks.get_mut(&u.id) {
            Some(r) if r.unlocked_at <= u.unlocked_at => {
                if u.hardcore && !r.hardcore {
                    r.hardcore = true;
                    return true;
                }
                false
            }
            _ => {
                self.unlocks.insert(
                    u.id.clone(),
                    Record {
                        unlocked_at: u.unlocked_at,
                        hardcore: u.hardcore,
                        device: device.to_string(),
                        device_name: device_name.to_string(),
                    },
                );
                true
            }
        }
    }

    pub fn find(&self, id: &str) -> Option<&Achievement> {
        self.schema.as_ref().and_then(|s| s.achievements.iter().find(|a| a.id == id))
    }
}

/// A new unlock on this device, to announce.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUnlock {
    pub key: String,
    pub achievement: Achievement,
    pub unlocked_at: DateTime<Utc>,
    pub hardcore: bool,
}

// ---------------------------------------------------------------------------
// Files

pub fn dir() -> PathBuf {
    paths::data_dir().join("achievements")
}

fn settings_file() -> PathBuf {
    dir().join("settings.json")
}

fn db_file(key: &str) -> PathBuf {
    dir().join(format!("{key}.json"))
}

fn lock() -> Result<FileLock> {
    FileLock::acquire(&dir().join(".lock"), Duration::from_secs(10))
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn settings() -> Settings {
    read_json(&settings_file())
}

pub fn update_settings<T>(f: impl FnOnce(&mut Settings) -> T) -> Result<T> {
    let _guard = lock()?;
    let mut s = settings();
    let out = f(&mut s);
    util::atomic_write(&settings_file(), &serde_json::to_vec_pretty(&s)?)?;
    Ok(out)
}

pub fn load_db(key: &str) -> GameDb {
    read_json(&db_file(key))
}

pub fn update_db<T>(key: &str, f: impl FnOnce(&mut GameDb) -> T) -> Result<T> {
    let _guard = lock()?;
    let mut db = load_db(key);
    let out = f(&mut db);
    util::atomic_write(&db_file(key), &serde_json::to_vec_pretty(&db)?)?;
    Ok(out)
}

/// Link games whose source is unambiguous (e.g. Bloodborne on Lantern),
/// except ones the user unlinked. Returns the keys linked.
pub fn link_obvious(cfg: &AppConfig) -> Vec<String> {
    let st = settings();
    let mut linked = Vec::new();
    for g in &cfg.games {
        if st.games.contains_key(&g.key) || st.unlinked.contains(&g.key) {
            continue;
        }
        let Some(c) = detect::candidates(g).into_iter().find(|c| c.sure) else { continue };
        match link(cfg, g, c.binding) {
            Ok(_) => linked.push(g.key.clone()),
            Err(e) => log(format!("{}: automatic achievement link failed: {e:#}", g.key)),
        }
    }
    linked
}

/// Forget a game's source and cached data (local only; the cloud keeps its records).
pub fn unlink(key: &str) -> Result<()> {
    update_settings(|s| {
        s.games.remove(key);
        s.unlinked.insert(key.to_string());
    })?;
    let _guard = lock()?;
    let _ = std::fs::remove_file(db_file(key));
    Ok(())
}

/// Link a game to a source. Unlocks that already exist are not announced.
pub fn link(cfg: &AppConfig, game: &GameEntry, binding: Binding) -> Result<GameDb> {
    let same = settings().games.get(&game.key) == Some(&binding);
    update_settings(|s| {
        s.games.insert(game.key.clone(), binding.clone());
        s.unlinked.remove(&game.key);
    })?;
    if !same {
        let _guard = lock()?;
        let _ = std::fs::remove_file(db_file(&game.key));
    }
    refresh(cfg, game, false)?;
    Ok(load_db(&game.key))
}

// ---------------------------------------------------------------------------
// Scanning

/// Read the game's source once.
pub fn scan(cfg: &AppConfig, game: &GameEntry, binding: &Binding) -> Result<Scan> {
    match binding.source {
        SourceKind::Retroachievements => retro::scan(&settings(), binding),
        SourceKind::Shadps4 | SourceKind::Lantern | SourceKind::Rpcs3 | SourceKind::Ps4port => trophy::scan(cfg, game, binding),
        SourceKind::Rules => rules::scan(cfg, game, binding),
    }
}

/// Files that change when an achievement unlocks (watched while the game runs).
pub fn watch_files(game: &GameEntry, binding: &Binding) -> Vec<PathBuf> {
    match binding.source {
        SourceKind::Rules => rules::watch_files(game, binding),
        _ => trophy::watch_files(binding),
    }
}

/// Read the source and record what is new. With `announce`, unlocks this
/// device has not seen before are returned (and marked as announced);
/// otherwise they are only recorded. The first scan after linking never
/// announces anything.
pub fn refresh(cfg: &AppConfig, game: &GameEntry, announce: bool) -> Result<Vec<NewUnlock>> {
    let Some(binding) = settings().games.get(&game.key).cloned() else {
        bail!(crate::tr!("这个游戏还没有关联成就来源", "This game has no achievement source yet"));
    };
    let result = scan(cfg, game, &binding);
    let (device, device_name) = (cfg.device.id.clone(), cfg.device.name.clone());
    update_db(&game.key, |db| {
        db.checked_at = Some(Utc::now());
        let scan = match result {
            Ok(s) => s,
            Err(e) => {
                db.last_error = Some(format!("{e:#}"));
                return Err(e);
            }
        };
        db.last_error = None;
        let first = !db.initialized;
        db.initialized = true;
        if let Some(schema) = scan.schema {
            db.schema = Some(schema);
        }
        let mut fresh = Vec::new();
        for u in &scan.unlocks {
            if db.mine.get(&u.id).is_some_and(|m| m.unlocked_at <= u.unlocked_at && (m.hardcore || !u.hardcore)) {
                continue;
            }
            db.mine.insert(u.id.clone(), u.clone());
            db.merge(u, &device, &device_name);
            if first || !announce || scan.quiet.contains(&u.id) {
                db.notified.insert(u.id.clone());
            } else if db.notified.insert(u.id.clone()) {
                fresh.push(u.clone());
            }
        }
        Ok(fresh
            .into_iter()
            .map(|u| NewUnlock {
                key: game.key.clone(),
                achievement: db.find(&u.id).cloned().unwrap_or_else(|| placeholder(&u.id)),
                unlocked_at: u.unlocked_at,
                hardcore: u.hardcore,
            })
            .collect())
    })?
}

pub(crate) fn log(msg: impl AsRef<str>) {
    crate::logger::log("achievements", msg);
}

/// After a game exits (Steam wrapper or tray): read its source once and sync
/// the cloud records. Nothing is announced; errors are only logged.
pub fn after_exit(ctx: &crate::sync::Ctx, game: &GameEntry) {
    if !settings().games.contains_key(&game.key) {
        return;
    }
    if let Err(e) = refresh(&ctx.cfg, game, false) {
        log(format!("{}: reading achievements failed: {e:#}", game.key));
    }
    sync_cloud(ctx, &game.key);
}

/// Merge other devices' records and upload this device's. Errors are logged.
pub fn sync_cloud(ctx: &crate::sync::Ctx, key: &str) {
    let Ok(cloud) = ctx.cloud() else { return };
    if let Err(e) = cloud::sync(cloud.files(), key, &ctx.cfg.device.id, &ctx.cfg.device.name) {
        log(format!("{key}: achievement cloud sync failed: {e:#}"));
    }
}

/// Shown when the source has unlocks but no names (e.g. Lantern).
pub fn placeholder(id: &str) -> Achievement {
    let n = id.trim_start_matches('0');
    let n = if n.is_empty() { "0" } else { n };
    Achievement {
        id: id.to_string(),
        name: crate::tr!("奖杯 {n}", "Trophy {n}"),
        description: String::new(),
        icon: None,
        hidden: false,
        points: None,
        grade: None,
    }
}

// ---------------------------------------------------------------------------
// View

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Web address of the icon (RetroAchievements).
    pub icon_url: Option<String>,
    /// A local icon exists; fetch it through the app or the local API.
    pub has_icon: bool,
    pub hidden: bool,
    pub points: Option<u32>,
    pub grade: Option<Grade>,
    pub unlocked_at: Option<DateTime<Utc>>,
    pub hardcore: bool,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub key: String,
    pub source: Option<SourceKind>,
    pub source_label: Option<&'static str>,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub total: usize,
    pub unlocked: usize,
    pub points: u32,
    pub points_total: u32,
    pub items: Vec<ItemView>,
    pub popup: bool,
    pub checked_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

fn item(a: &Achievement, r: Option<&Record>) -> ItemView {
    let local = a.icon.as_deref().filter(|i| !is_web(i));
    ItemView {
        id: a.id.clone(),
        name: a.name.clone(),
        description: a.description.clone(),
        icon_url: a.icon.clone().filter(|i| is_web(i)),
        has_icon: local.is_some_and(|p| Path::new(p).is_file()),
        hidden: a.hidden,
        points: a.points,
        grade: a.grade,
        unlocked_at: r.map(|r| r.unlocked_at),
        hardcore: r.is_some_and(|r| r.hardcore),
        device_name: r.map(|r| r.device_name.clone()).filter(|n| !n.is_empty()),
    }
}

pub fn is_web(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

/// Everything the app and the plugin show for a game.
pub fn view(key: &str) -> GameView {
    let st = settings();
    let binding = st.games.get(key);
    let db = load_db(key);
    let mut items: Vec<ItemView> = Vec::new();
    if let Some(schema) = &db.schema {
        items.extend(schema.achievements.iter().map(|a| item(a, db.unlocks.get(&a.id))));
    }
    // Unlocks without a definition (no names available): numbered placeholders.
    for (id, r) in &db.unlocks {
        if !items.iter().any(|i| &i.id == id) {
            items.push(item(&placeholder(id), Some(r)));
        }
    }
    // Unlocked first (newest first), then the rest in the source's order.
    let order: BTreeMap<String, usize> = items.iter().enumerate().map(|(i, it)| (it.id.clone(), i)).collect();
    items.sort_by(|a, b| match (a.unlocked_at, b.unlocked_at) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => order[&a.id].cmp(&order[&b.id]),
    });
    let total = db.schema.as_ref().map(|s| s.achievements.len()).unwrap_or(0).max(items.len());
    let unlocked = items.iter().filter(|i| i.unlocked_at.is_some()).count();
    let points_total = items.iter().filter_map(|i| i.points).sum();
    let points = items.iter().filter(|i| i.unlocked_at.is_some()).filter_map(|i| i.points).sum();
    GameView {
        key: key.to_string(),
        source: binding.map(|b| b.source),
        source_label: binding.map(binding_label),
        source_id: binding.map(|b| b.source_id.clone()).filter(|s| !s.is_empty()),
        title: db.schema.as_ref().and_then(|s| s.title.clone()),
        total,
        unlocked,
        points,
        points_total,
        items,
        popup: st.popup_for(key),
        checked_at: db.checked_at,
        last_error: db.last_error,
    }
}

/// The local icon file of an achievement, if any.
pub fn icon_path(key: &str, id: &str) -> Option<PathBuf> {
    let db = load_db(key);
    let a = db.find(id)?;
    let p = a.icon.as_deref().filter(|i| !is_web(i))?;
    let p = PathBuf::from(p);
    p.is_file().then_some(p)
}

/// `data:` URL of a local icon (PNG / JPEG), for pages that can't read files.
pub fn icon_data_url(path: &Path) -> Result<String> {
    use base64::Engine;
    let bytes = std::fs::read(path)?;
    if bytes.len() > 2 * 1024 * 1024 {
        bail!("icon too large");
    }
    let mime = if bytes.starts_with(&[0xFF, 0xD8]) { "image/jpeg" } else { "image/png" };
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn earliest_unlock_wins() {
        let mut db = GameDb::default();
        let late = Unlock { id: "1".into(), unlocked_at: t("2026-11-20T10:00:00Z"), hardcore: false };
        let early = Unlock { id: "1".into(), unlocked_at: t("2026-11-19T10:00:00Z"), hardcore: false };
        assert!(db.merge(&late, "b", "Laptop"));
        assert!(db.merge(&early, "a", "Desktop"));
        assert!(!db.merge(&late, "b", "Laptop"));
        let r = &db.unlocks["1"];
        assert_eq!(r.unlocked_at, early.unlocked_at);
        assert_eq!(r.device_name, "Desktop");
    }

    #[test]
    fn hardcore_upgrades_an_existing_unlock() {
        let mut db = GameDb::default();
        db.merge(&Unlock { id: "1".into(), unlocked_at: t("2026-11-19T10:00:00Z"), hardcore: false }, "a", "A");
        assert!(db.merge(&Unlock { id: "1".into(), unlocked_at: t("2026-11-20T10:00:00Z"), hardcore: true }, "b", "B"));
        assert!(db.unlocks["1"].hardcore);
        assert_eq!(db.unlocks["1"].device, "a");
    }

    #[test]
    fn placeholder_numbers() {
        assert!(placeholder("012").name.contains("12"));
        assert!(placeholder("000").name.contains('0'));
    }
}
