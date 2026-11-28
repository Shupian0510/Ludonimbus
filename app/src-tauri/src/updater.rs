//! In-app updates via the Tauri updater plugin.
//!
//! Releases on GitHub carry a signed installer plus `latest.json`; the app
//! checks `releases/latest/download/latest.json`, verifies the signature with
//! the public key in `tauri.conf.json`, runs the installer and restarts.

use crate::watcher;
use ludonimbus_core::{config, logger};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    available: bool,
    version: Option<String>,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "UpdateProgress")]
pub(crate) struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(u)) => {
            Ok(UpdateInfo { current, available: true, version: Some(u.version.clone()), notes: u.body.clone(), date: u.date.map(|d| d.to_string()) })
        }
        Ok(None) => Ok(UpdateInfo { current, available: false, version: None, notes: None, date: None }),
        Err(e) => {
            logger::log("updater", format!("check failed: {e}"));
            Err(ludonimbus_core::tr!("检查更新失败：{e}", "Couldn't check for updates: {e}"))
        }
    }
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    // A Flatpak can't replace itself: a newer one is installed over it.
    if ludonimbus_core::paths::in_flatpak() {
        return Err(ludonimbus_core::tr!(
            "Flatpak 版不能在应用里更新：下载新的 Ludonimbus.flatpak，用 flatpak install 覆盖安装",
            "The Flatpak can't update itself: download the new Ludonimbus.flatpak and install it over this one with flatpak install"
        ));
    }
    // The installer replaces ludonimbus-run.exe, which is in use while a game
    // launched from Steam is running.
    if let Ok(cfg) = config::load() {
        if let Some(g) = cfg.games.iter().find(|g| watcher::runner_active(&g.key)) {
            return Err(ludonimbus_core::tr!("「{}」正在运行，请退出游戏后再更新", "\"{}\" is running. Quit the game before updating", g.name));
        }
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| ludonimbus_core::tr!("检查更新失败：{e}", "Couldn't check for updates: {e}"))?
        .ok_or_else(|| ludonimbus_core::tr!("已经是最新版本", "Already up to date"))?;
    logger::log("updater", format!("installing {}", update.version));
    let mut downloaded: u64 = 0;
    let app2 = app.clone();
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app2.emit("update-progress", Progress { downloaded, total });
            },
            || logger::log("updater", "download finished"),
        )
        .await
        .map_err(|e| {
            logger::log("updater", format!("install failed: {e}"));
            ludonimbus_core::tr!("更新失败：{e}", "Update failed: {e}")
        })?;
    // On Windows the installer takes over and exits the app; elsewhere restart.
    app.restart();
}
