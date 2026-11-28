//! Steam: its install folder and accounts, the shortcuts of the games added
//! to it, and the games Steam installed itself.

use super::{blocking, CmdResult};
use crate::{art, busy, watcher};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config;
use ludonimbus_core::localapi::plugin;
use ludonimbus_core::sync::Ctx;
use ludonimbus_core::{games, steam, steamcloud, util};
use serde::Serialize;
use std::path::Path;
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamAccounts")]
pub struct AccountsView {
    accounts: Vec<steam::Account>,
    selected: Option<String>,
    root: Option<String>,
}

#[tauri::command]
pub async fn steam_accounts() -> CmdResult<AccountsView> {
    blocking(|| {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let accounts = root.as_deref().map(steam::accounts).unwrap_or_default();
        let selected = root.as_deref().and_then(|r| steam::default_account(&cfg, r));
        Ok(AccountsView { accounts, selected, root: root.map(|r| r.display().to_string()) })
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutView {
    #[serde(flatten)]
    candidate: games::ShortcutCandidate,
    cover: Option<String>,
    icon: Option<String>,
}

#[tauri::command]
pub async fn steam_shortcuts(account: String) -> CmdResult<Vec<ShortcutView>> {
    blocking(move || {
        let cfg = config::load()?;
        let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
        Ok(games::shortcut_candidates(&cfg, &account)?
            .into_iter()
            .map(|c| {
                let a = steam::art(&root, &account, c.appid);
                ShortcutView {
                    cover: a.cover.map(|f| art::url(&account, c.appid, "cover", &f)),
                    icon: a.icon.map(|f| art::url(&account, c.appid, "icon", &f)),
                    candidate: c,
                }
            })
            .collect())
    })
    .await
}

pub(crate) fn with_steam_closed<T>(close_steam: bool, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
    let was_running = steam::is_running();
    if was_running {
        if !close_steam {
            bail!("STEAM_RUNNING");
        }
        steam::shutdown(&root)?;
    }
    let out = f();
    if was_running {
        let _ = steam::launch(&root);
    }
    out
}

#[tauri::command]
pub async fn steam_set_enabled(key: String, enabled: bool, close_steam: bool) -> CmdResult<()> {
    blocking(move || with_steam_closed(close_steam, || if enabled { games::steam_enable(&key) } else { games::steam_disable(&key) })).await
}

#[tauri::command]
pub async fn steam_repair(key: String, close_steam: bool) -> CmdResult<()> {
    blocking(move || with_steam_closed(close_steam, || games::steam_repair(&key))).await
}

/// Steam install folders found on this PC.
#[tauri::command]
pub async fn steam_find_roots() -> CmdResult<Vec<String>> {
    blocking(|| Ok(steam::find_roots().iter().map(|p| p.display().to_string()).collect())).await
}

/// Use the Steam install a picked folder is in; `None` goes back to
/// detecting it. Returns the folder now in use.
#[tauri::command]
pub async fn steam_set_root(path: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let root = match path.filter(|p| !p.trim().is_empty()) {
            None => None,
            Some(p) => Some(steam::root_of(Path::new(p.trim())).ok_or_else(|| {
                anyhow!(ludonimbus_core::tr!("这个文件夹里没有 Steam（找不到 steam.exe）：{p}", "Steam isn't in this folder (no steam.exe): {p}"))
            })?),
        };
        // The folder Steam itself registered needs no pinning: keep following it.
        let detected = steam::detect_root();
        let pinned = root.filter(|r| !detected.as_ref().is_some_and(|d| util::same_path(&d.to_string_lossy(), &r.to_string_lossy())));
        config::update(|c| {
            c.steam.root = pinned;
            Ok(())
        })?;
        Ok(steam::root(&config::load()?).map(|r| r.display().to_string()))
    })
    .await
}

// ---------------------------------------------------------------------------
// Games installed through Steam (docs/plan-v2.md, section 8)

/// The games Steam installed, and how each stands with Ludonimbus.
#[tauri::command]
pub async fn steam_cloud_apps() -> CmdResult<Vec<steamcloud::StoreApp>> {
    blocking(|| Ok(steamcloud::apps(&config::load()?))).await
}

/// Add games Steam installed to the library, backup only.
#[tauri::command]
pub async fn steam_cloud_add(appids: Vec<u32>) -> CmdResult<Vec<steamcloud::Done>> {
    blocking(move || Ok(steamcloud::add_many(&config::load()?, &appids))).await
}

#[tauri::command]
pub async fn steam_cloud_status(key: String) -> CmdResult<steamcloud::StoreStatus> {
    blocking(move || {
        let cfg = config::load()?;
        steamcloud::status(&cfg, cfg.require_game(&key)?)
    })
    .await
}

/// Refused while the game runs or a sync of it is busy.
fn steam_game_free(key: &str) -> Result<busy::Guard> {
    if watcher::runner_active(key) {
        bail!(ludonimbus_core::tr!("游戏正在运行，退出游戏后再改", "The game is running; change this after you quit"));
    }
    busy::begin(key).ok_or_else(|| anyhow!(ludonimbus_core::tr!("正在同步，请稍候", "Syncing; please wait")))
}

/// The Steam plugin that changes Steam's settings of the game, when the
/// user confirmed letting it (`via_plugin`) and one is connected to this
/// app for the game's account.
fn hands_for(key: &str, via_plugin: bool) -> Result<Option<plugin::Plugin>> {
    if !via_plugin {
        return Ok(None);
    }
    let cfg = config::load()?;
    Ok(cfg.require_game(key)?.store.as_ref().and_then(|l| plugin::Plugin::for_account(&l.account)))
}

/// Refused because Steam runs (its file can't be changed then).
fn steam_running(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains("STEAM_RUNNING")
}

/// Let Ludonimbus keep a Steam game's saves. `wrapper`: through its launch
/// options. Without a plugin (or `via_plugin` off) Steam Cloud has to be
/// off for the game already, and the launch options go into Steam's file,
/// which needs Steam closed (`close_steam` lets Ludonimbus close and reopen
/// it). With `via_plugin` (the user confirmed it) and a Steam plugin that
/// can, Steam turns Steam Cloud off and sets the launch options itself,
/// and stays open.
#[tauri::command]
pub async fn steam_cloud_take_over(key: String, wrapper: bool, close_steam: bool, via_plugin: bool) -> CmdResult<steamcloud::StoreStatus> {
    blocking(move || {
        let _busy = steam_game_free(&key)?;
        let take = || Ctx::load().and_then(|ctx| steamcloud::take_over(&ctx, &key, wrapper));
        let entry = match hands_for(&key, via_plugin)? {
            Some(hands) => match Ctx::load().and_then(|ctx| steamcloud::take_over_through(&ctx, &key, wrapper, &hands)) {
                // The plugin can't set launch options: Steam's file, with Steam closed.
                Err(e) if steam_running(&e) => with_steam_closed(close_steam, take)?,
                r => r?,
            },
            None if wrapper => with_steam_closed(close_steam, take)?,
            None => take()?,
        };
        steamcloud::status(&config::load()?, &entry)
    })
    .await
}

/// Give a Steam game back to Steam Cloud (its launch options as before).
/// With `via_plugin` (the user confirmed it) and a Steam plugin, Steam
/// turns Steam Cloud on again itself.
#[tauri::command]
pub async fn steam_cloud_hand_back(key: String, close_steam: bool, via_plugin: bool) -> CmdResult<steamcloud::StoreStatus> {
    blocking(move || {
        let _busy = steam_game_free(&key)?;
        let wrapped = config::load()?.require_game(&key)?.store.as_ref().is_some_and(|s| s.wrapper);
        let hand = || Ctx::load().and_then(|ctx| steamcloud::hand_back(&ctx, &key));
        let entry = match hands_for(&key, via_plugin)? {
            Some(hands) => match Ctx::load().and_then(|ctx| steamcloud::hand_back_through(&ctx, &key, &hands)) {
                Err(e) if steam_running(&e) => with_steam_closed(close_steam, hand)?,
                r => r?,
            },
            None if wrapped => with_steam_closed(close_steam, hand)?,
            None => hand()?,
        };
        steamcloud::status(&config::load()?, &entry)
    })
    .await
}

/// Open the game's properties in Steam, where its Steam Cloud switch is.
#[tauri::command]
pub async fn steam_cloud_open_properties(key: String) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let game = cfg.require_game(&key)?;
        let link = game
            .store
            .as_ref()
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("「{}」不是从 Steam 安装的游戏", "\"{}\" isn't a game installed through Steam", game.name)))?;
        opener::open(format!("steam://gameproperties/{}", link.appid))?;
        Ok(())
    })
    .await
}

/// Every game Steam installed that isn't in the library yet, backup only.
#[tauri::command]
pub async fn steam_cloud_backup_all() -> CmdResult<Vec<steamcloud::Done>> {
    blocking(|| Ok(steamcloud::backup_all(&config::load()?))).await
}

/// Take over every game that is backup only: Steam Cloud off for all games
/// first, or, with `via_plugin` (the user confirmed it) and a Steam plugin
/// that can, Steam turns it off for each of them and stays open.
#[tauri::command]
pub async fn steam_cloud_take_over_all(wrapper: bool, close_steam: bool, via_plugin: bool) -> CmdResult<Vec<steamcloud::Done>> {
    blocking(move || {
        let cfg = config::load()?;
        let hands = steamcloud::steam_account(&cfg).ok().and_then(|(_, account)| plugin::Plugin::for_account(&account));
        if let Some(hands) = hands.filter(|h| via_plugin && (!wrapper || h.caps.launch_options)) {
            return Ctx::load().and_then(|ctx| steamcloud::take_over_all_through(&ctx, wrapper, &hands));
        }
        let take = || Ctx::load().and_then(|ctx| steamcloud::take_over_all(&ctx, wrapper));
        if wrapper {
            with_steam_closed(close_steam, take)
        } else {
            take()
        }
    })
    .await
}
