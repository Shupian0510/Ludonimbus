//! Commands for adding games: scanning this PC, finding saves, artwork from
//! SteamGridDB, and adding games to Steam.

use crate::commands::{blocking, with_steam_closed, CmdResult};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::discover::{self, Found};
use ludonimbus_core::savefind::{self, Candidate};
use ludonimbus_core::steamgriddb::{self as sgdb, SgdbGame, SgdbImage};
use ludonimbus_core::{config, games, localart, steam, util};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Scanning

/// Programs the user picked or a scan found since the app started: with the
/// library's own, the ones "start with the game" may run.
static OFFERED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Remember a program the user picked (see [`savefind_watch_start`]).
pub fn offer_exe(path: &str) {
    OFFERED.lock().unwrap().insert(util::norm_path(path));
}

fn offer_found(found: Vec<Found>) -> Vec<Found> {
    for exe in found.iter().filter_map(|f| f.exe.as_deref()) {
        offer_exe(exe);
    }
    found
}

/// Steam shortcuts (all accounts), Epic and GOG games. Fast.
#[tauri::command]
pub async fn scan_quick() -> CmdResult<Vec<Found>> {
    blocking(|| Ok(offer_found(discover::quick(&config::load()?)))).await
}

/// Games Ludusavi finds saves for. Takes up to a minute.
#[tauri::command]
pub async fn scan_ludusavi() -> CmdResult<Vec<Found>> {
    blocking(|| discover::ludusavi_scan(&config::load()?).map(offer_found)).await
}

/// Library folders of Steam, Xbox, Epic, GOG, EA and Ubisoft on this PC.
#[tauri::command]
pub async fn game_folders() -> CmdResult<Vec<discover::GameFolder>> {
    blocking(|| Ok(discover::game_folders(&config::load()?))).await
}

/// Games in a folder the user picked.
#[tauri::command]
pub async fn scan_folder(path: String) -> CmdResult<Vec<Found>> {
    blocking(move || {
        let dir = PathBuf::from(&path);
        if !dir.is_dir() {
            bail!(ludonimbus_core::tr!("文件夹不存在：{path}", "The folder doesn't exist: {path}"));
        }
        Ok(offer_found(discover::folder_scan(&config::load()?, &dir)))
    })
    .await
}

/// Add a scanned game. Empty `saves` / missing title use what the scan found.
#[tauri::command]
pub async fn add_found(found: Found, name: String, saves: Vec<String>, ludusavi_title: Option<String>, exe: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let g = games::add_found(&found, games::FoundChoice { name, saves, ludusavi_title, exe: exe.map(PathBuf::from) })?;
        Ok(crate::commands::adopt_cloud_key(g.key))
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    key: String,
    /// The game's name (the emulator's for emulators).
    name: String,
    /// What to show, e.g. "Dolphin（整个模拟器的存档）".
    label: String,
    emulator: bool,
}

/// The built-in rule (emulator, recomp…) for an exe, if any.
#[tauri::command]
pub async fn detect_adapter(exe: String) -> CmdResult<Option<AdapterInfo>> {
    blocking(move || {
        Ok(games::recognize(Path::new(&exe), "", None).map(|r| AdapterInfo {
            key: r.adapter.key.clone(),
            name: r.default_name(),
            label: r.label(),
            emulator: r.adapter.is_emulator(),
        }))
    })
    .await
}

// ---------------------------------------------------------------------------
// Finding saves

#[tauri::command]
pub async fn savefind_suggest(name: String, exe: Option<String>, install: Option<String>) -> CmdResult<Vec<Candidate>> {
    blocking(move || {
        let exe = exe.map(PathBuf::from);
        let install = install.map(PathBuf::from).or_else(|| exe.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        Ok(savefind::suggest(&name, exe.as_deref(), install.as_deref()))
    })
    .await
}

static WATCH: Mutex<Option<savefind::Watch>> = Mutex::new(None);

/// A program the user picked, a scan found, or a library game's.
fn may_launch(exe: &Path) -> bool {
    let key = util::norm_path(&exe.to_string_lossy());
    OFFERED.lock().unwrap().contains(&key)
        || config::load().is_ok_and(|cfg| cfg.games.iter().filter_map(|g| g.exe.as_deref()).any(|e| util::norm_path(&e.to_string_lossy()) == key))
}

/// Start recording file changes; launches the game when `exe` is given.
#[tauri::command]
pub async fn savefind_watch_start(exe: Option<String>, args: Option<String>, install: Option<String>) -> CmdResult<()> {
    blocking(move || {
        let exe = exe.map(PathBuf::from).filter(|e| e.is_file());
        if let Some(exe) = exe.as_deref().filter(|e| !may_launch(e)) {
            bail!(ludonimbus_core::tr!("只启动你选择的程序：{}", "Only programs you chose are started: {}", exe.display()));
        }
        let install = install.map(PathBuf::from).or_else(|| exe.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        let mut roots = savefind::save_roots();
        roots.extend(install.clone());
        let watch = savefind::Watch::start(&roots)?;
        *WATCH.lock().unwrap() = Some(watch);
        if let Some(exe) = exe {
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(util::split_args(args.as_deref().unwrap_or("")));
            if let Some(dir) = exe.parent() {
                cmd.current_dir(dir);
            }
            cmd.spawn().map_err(|e| anyhow!(ludonimbus_core::tr!("无法启动 {}：{e}", "Couldn't start {}: {e}", exe.display())))?;
        }
        Ok(())
    })
    .await
}

/// Stop recording and suggest the folders that changed.
#[tauri::command]
pub async fn savefind_watch_stop() -> CmdResult<Vec<Candidate>> {
    blocking(|| {
        let watch = WATCH.lock().unwrap().take().ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有正在进行的记录", "No recording in progress")))?;
        Ok(watch.finish())
    })
    .await
}

// ---------------------------------------------------------------------------
// SteamGridDB and artwork

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SgdbStatus {
    has_key: bool,
    key_page: String,
}

#[tauri::command]
pub async fn sgdb_status() -> CmdResult<SgdbStatus> {
    blocking(|| Ok(SgdbStatus { has_key: sgdb::load_key()?.is_some(), key_page: sgdb::KEY_PAGE.into() })).await
}

/// Check and save the user's API key.
#[tauri::command]
pub async fn sgdb_set_key(key: String) -> CmdResult<()> {
    blocking(move || {
        let key = key.trim().to_string();
        if key.is_empty() {
            bail!(ludonimbus_core::tr!("请填写 API Key", "Enter the API key"));
        }
        sgdb::search(&key, "Celeste")?;
        sgdb::store_key(&key)
    })
    .await
}

#[tauri::command]
pub async fn sgdb_clear_key() -> CmdResult<()> {
    blocking(sgdb::clear_key).await
}

fn key() -> Result<String> {
    sgdb::load_key()?.ok_or_else(|| anyhow!(ludonimbus_core::tr!("请先在设置里填写 SteamGridDB API Key", "Enter a SteamGridDB API key in Settings first")))
}

#[tauri::command]
pub async fn sgdb_search(term: String) -> CmdResult<Vec<SgdbGame>> {
    blocking(move || sgdb::search(&key()?, &term)).await
}

#[tauri::command]
pub async fn sgdb_images(game_id: u64, kind: String) -> CmdResult<Vec<SgdbImage>> {
    blocking(move || sgdb::images(&key()?, game_id, &kind)).await
}

/// Send a game's artwork to the cloud in the background, so other devices
/// show it too (see `artsync`).
fn push_art(key: &str) {
    let key = key.to_string();
    std::thread::spawn(move || {
        let Ok(cfg) = config::load() else { return };
        let Some(g) = cfg.game(&key).cloned() else { return };
        if let Ok(Some(cloud)) = ludonimbus_core::cloud::open(&cfg) {
            let result =
                ludonimbus_core::cloud::gamedata::Folders::of(&cloud, &key).and_then(|at| ludonimbus_core::artsync::push(cloud.files(), &cfg, &at, &g));
            if let Err(e) = result {
                ludonimbus_core::logger::log("app", format!("{key}: artwork upload failed: {e:#}"));
            }
        }
    });
}

/// Use a SteamGridDB image as a game's artwork.
#[tauri::command]
pub async fn art_set_url(key: String, kind: String, url: String) -> CmdResult<()> {
    blocking(move || {
        let file = sgdb::download(&url)?;
        let res = games::set_art(&key, &kind, &file);
        let _ = std::fs::remove_file(&file);
        res?;
        push_art(&key);
        Ok(())
    })
    .await
}

/// Use a local image as a game's artwork.
#[tauri::command]
pub async fn art_set_file(key: String, kind: String, path: String) -> CmdResult<()> {
    blocking(move || {
        games::set_art(&key, &kind, Path::new(&path))?;
        push_art(&key);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn pick_image() -> CmdResult<Option<String>> {
    blocking(|| {
        Ok(rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择图片", "Choose an image"))
            .add_filter(ludonimbus_core::tr!("图片", "Images"), &["png", "jpg", "jpeg", "webp"])
            .pick_file()
            .map(|p| p.display().to_string()))
    })
    .await
}

/// Pick the best-rated SteamGridDB image of every kind the game has no
/// artwork for yet. Does nothing without an API key.
fn auto_art(game_key: &str, name: &str) -> Result<()> {
    let Some(k) = sgdb::load_key()? else { return Ok(()) };
    let Some(found) = sgdb::search(&k, name)?.into_iter().next() else { return Ok(()) };
    for kind in steam::ART_KINDS {
        if localart::file(game_key, kind).is_some() {
            continue;
        }
        if let Some(img) = sgdb::images(&k, found.id, kind)?.into_iter().next() {
            let file = sgdb::download(&img.url)?;
            let res = games::set_art(game_key, kind, &file);
            let _ = std::fs::remove_file(&file);
            res?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Steam

/// Add one game to Steam (Steam is closed and reopened when `close_steam`).
#[tauri::command]
pub async fn steam_add_game(key: String, account: String, close_steam: bool) -> CmdResult<u32> {
    blocking(move || with_steam_closed(close_steam, || games::add_to_steam(&key, &account))).await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamAddResult")]
pub struct BatchResult {
    key: String,
    error: Option<String>,
}

/// Add several games to Steam with one Steam restart, picking artwork from
/// SteamGridDB automatically when `auto_art`.
#[tauri::command]
pub async fn steam_add_games(keys: Vec<String>, account: String, auto_art: bool, close_steam: bool) -> CmdResult<Vec<BatchResult>> {
    blocking(move || {
        let cfg = config::load()?;
        if auto_art {
            for k in &keys {
                if let Some(g) = cfg.game(k) {
                    let _ = self::auto_art(k, &g.name);
                }
            }
        }
        with_steam_closed(close_steam, || {
            Ok(keys.iter().map(|k| BatchResult { key: k.clone(), error: games::add_to_steam(k, &account).err().map(|e| format!("{e:#}")) }).collect())
        })
    })
    .await
}

/// Delete the Steam shortcut Ludonimbus created for a game.
#[tauri::command]
pub async fn steam_remove_game(key: String, close_steam: bool) -> CmdResult<()> {
    blocking(move || with_steam_closed(close_steam, || games::remove_from_steam(&key))).await
}
