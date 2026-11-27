//! Library management: adding games (from a Steam shortcut, by hand, from
//! the cloud), changing and removing them, and the pickers and Ludusavi
//! lookups the forms use. Scanning this PC for games is in `library.rs`.

use super::{blocking, CmdResult};
use anyhow::{anyhow, bail};
use ludonimbus_core::cloud::CloudGame;
use ludonimbus_core::config;
use ludonimbus_core::state;
use ludonimbus_core::sync::Ctx;
use ludonimbus_core::{games, join, ludusavi, relocate, spec};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[tauri::command]
pub async fn add_from_shortcut(
    account: String,
    appid: u32,
    name: Option<String>,
    adapter: Option<String>,
    saves: Vec<String>,
    key: Option<String>,
) -> CmdResult<String> {
    blocking(move || {
        let g = games::add_from_shortcut(&account, appid, games::NewGame { name, adapter, saves, key, ludusavi_title: None })?;
        Ok(adopt_cloud_key(g.key))
    })
    .await
}

#[tauri::command]
pub async fn add_custom(name: String, exe: Option<String>, saves: Vec<String>, ludusavi_title: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let exe = exe.filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        Ok(adopt_cloud_key(games::add_custom(name.trim(), exe, saves, ludusavi_title, None)?.key))
    })
    .await
}

/// A Switch game just added that the cloud already has under another key
/// (added on another device, maybe in another emulator): link to it instead
/// of starting a second cloud entry. Best effort: offline or without a
/// cloud the new key stays.
pub fn adopt_cloud_key(key: String) -> String {
    let switch = config::load().ok().is_some_and(|c| c.game(&key).is_some_and(|g| games::switch_title(&g.saves).is_some()));
    if !switch {
        return key;
    }
    match Ctx::load().and_then(|ctx| ctx.cloud()?.list_games()).and_then(|list| games::adopt_cloud_key(&key, &list)) {
        Ok(Some(linked)) => {
            ludonimbus_core::logger::log("app", format!("{key}: the cloud already has this Switch game as {linked}; linked to it"));
            linked
        }
        _ => key,
    }
}

#[tauri::command]
pub async fn cloud_games() -> CmdResult<Vec<CloudGame>> {
    blocking(|| {
        let ctx = Ctx::load()?;
        let list = ctx.cloud()?.list_games()?;
        Ok(list.into_iter().filter(|g| ctx.cfg.game(&g.key).is_none()).collect())
    })
    .await
}

#[tauri::command]
pub async fn add_from_cloud(key: String, account: Option<String>, appid: Option<u32>, exe: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let cg = ctx
            .cloud()?
            .list_games()?
            .into_iter()
            .find(|g| g.key == key)
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("云端找不到这个游戏", "This game wasn't found in the cloud")))?;
        let g = games::add_from_cloud(&cg, account.as_deref(), appid, exe.map(PathBuf::from))?;
        Ok(g.key)
    })
    .await
}

/// What to change in a game. A missing field stays as it is.
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
pub struct GamePatch {
    name: Option<String>,
    saves: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    wait_for: Option<Vec<String>>,
    args: Option<String>,
    auto_sync: Option<bool>,
    upload_on_game_exit: Option<bool>,
    /// Library group; empty = the detected source.
    source: Option<String>,
}

fn clean(list: Vec<String>) -> Vec<String> {
    list.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

#[tauri::command]
pub async fn update_game(key: String, patch: GamePatch) -> CmdResult<()> {
    blocking(move || {
        let saves = patch.saves.map(clean);
        let exclude = patch.exclude.map(clean);
        for p in saves.iter().flatten().chain(exclude.iter().flatten()) {
            spec::split_pattern(p)?;
        }
        // Folders matched with another version's say where the files were:
        // with another save location the game keeps its saves apart again.
        if let Some(new) = &saves {
            let ctx = Ctx::load()?;
            if let Some(g) = ctx.cfg.game(&key).filter(|g| !g.bind.is_empty() && &g.saves != new) {
                join::forget(&ctx, g)?;
            }
        }
        games::update_game(&key, |g| {
            if let Some(n) = patch.name.filter(|n| !n.trim().is_empty()) {
                g.name = n.trim().to_string();
            }
            if let Some(s) = saves {
                if s.is_empty() && g.adapter.is_none() && g.ludusavi_title.is_none() {
                    bail!(ludonimbus_core::tr!("至少需要一个存档位置", "At least one save location is needed"));
                }
                g.saves = s;
            }
            if let Some(e) = exclude {
                g.exclude = e;
            }
            if let Some(w) = patch.wait_for {
                g.wait_for = clean(w);
            }
            if let Some(a) = patch.args {
                g.args = a;
            }
            if let Some(a) = patch.auto_sync {
                g.auto_sync = a;
            }
            if let Some(v) = patch.upload_on_game_exit {
                g.upload_on_game_exit = Some(v);
            }
            if let Some(s) = patch.source {
                g.source = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            }
            Ok(())
        })?;
        // Keep the cloud copy of the definition current.
        let _ = state::update_game(&key, |s| s.cloud_def_uploaded = false);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_game(key: String) -> CmdResult<()> {
    blocking(move || games::remove(&key)).await
}

#[tauri::command]
pub async fn relocate_auto(key: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        // No exe yet (added from its saves): look for where it is installed.
        let found = if g.exe.is_none() { ludonimbus_core::discover::locate_game(&cfg, g) } else { relocate::find_candidate(g) };
        Ok(found.map(|p| p.display().to_string()))
    })
    .await
}

#[tauri::command]
pub async fn set_game_exe(key: String, path: String) -> CmdResult<()> {
    blocking(move || games::set_exe(&key, Path::new(&path)).map(|_| ())).await
}

#[tauri::command]
pub async fn pick_exe(start: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let mut d = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择游戏程序", "Choose the game's program"))
            .add_filter(ludonimbus_core::tr!("程序", "Programs"), &["exe", "bat", "cmd", "lnk"]);
        if let Some(s) = start.map(PathBuf::from).and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)) {
            d = d.set_directory(s);
        }
        Ok(d.pick_file().map(|p| p.display().to_string()).inspect(|p| crate::library::offer_exe(p)))
    })
    .await
}

#[tauri::command]
pub async fn pick_folder(title: Option<String>, start: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let mut d = rfd::FileDialog::new().set_title(title.unwrap_or_else(|| ludonimbus_core::tr!("选择文件夹", "Choose a folder")));
        if let Some(s) = start.map(PathBuf::from).and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)) {
            d = d.set_directory(s);
        }
        Ok(d.pick_folder().map(|p| p.display().to_string()))
    })
    .await
}

/// Turn a folder the user picked into a portable save pattern.
#[tauri::command]
pub async fn folder_to_pattern(path: String, exe: Option<String>) -> CmdResult<String> {
    blocking(move || {
        let install = exe.as_deref().map(Path::new).and_then(Path::parent).map(Path::to_path_buf);
        let roots = spec::builtin_roots(install.as_deref());
        let logical = spec::tokenize(Path::new(&path), &roots);
        let (root, rest) = logical.split_once('/').unwrap_or((&logical, ""));
        Ok(if rest.is_empty() { format!("{{{root}}}/**") } else { format!("{{{root}}}/{rest}/**") })
    })
    .await
}

#[tauri::command]
pub async fn ludusavi_search(query: String) -> CmdResult<Vec<String>> {
    blocking(move || ludusavi::search(query.trim())).await
}

#[tauri::command]
pub async fn ludusavi_preview(title: String) -> CmdResult<Vec<String>> {
    blocking(move || Ok(ludusavi::resolve_files(&title)?.iter().map(|p| p.display().to_string()).collect())).await
}
