//! Commands for the save library (docs/plan-phase5.md §5): games only the
//! cloud has, linking versions of one game, and applying saves from one
//! version to another.

use crate::commands::{blocking, cloud_only_views, CloudOnlyView, CmdResult};
use crate::{busy, watcher};
use anyhow::bail;
use ludonimbus_core::cloud::gamedata::Folders;
use ludonimbus_core::gamebackup::{self, remote};
use ludonimbus_core::sync::{Ctx, Outcome};
use ludonimbus_core::{artsync, config, logger, playtime, process, savelib, tr, transfer};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudLibrary {
    cloud_only: Vec<CloudOnlyView>,
    /// key → family for every game (this PC's and the cloud's).
    families: HashMap<String, String>,
    fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Why the list couldn't be fetched (the cached one is returned).
    error: Option<String>,
}

/// Fetch the cloud's game list (kept on this PC for offline use) and the
/// artwork of games this PC doesn't have yet.
#[tauri::command]
pub async fn refresh_cloud_library() -> CmdResult<CloudLibrary> {
    blocking(|| {
        let ctx = Ctx::load()?;
        let mut error = None;
        if ctx.has_cloud() {
            match ctx.cloud().and_then(|c| savelib::refresh(&ctx.cfg, c)) {
                Ok(list) => {
                    let cloud = ctx.cloud()?;
                    for cg in list.games.iter().filter(|c| ctx.cfg.game(&c.key).is_none()) {
                        if ludonimbus_core::localart::file(&cg.key, "cover").is_none() {
                            let result = Folders::of(cloud, &cg.key).and_then(|at| artsync::pull(cloud.files(), &ctx.cfg, &at, None));
                            if let Err(e) = result {
                                logger::log("app", format!("{}: artwork download failed: {e:#}", cg.key));
                            }
                        }
                    }
                }
                Err(e) => error = Some(format!("{e:#}")),
            }
        }
        // Links taken over from the cloud are in the config now.
        let cfg = config::load()?;
        let cached = savelib::cached(&cfg);
        let families = savelib::index(&cfg, &cached.games);
        Ok(CloudLibrary {
            cloud_only: cloud_only_views(&cfg, &cached.games, &families, &cached.tags),
            families,
            fetched_at: cached.fetched_at,
            error,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameBackupView {
    versions: usize,
    size: u64,
    created_at: chrono::DateTime<chrono::Utc>,
    device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudGameInfo {
    playtime: crate::commands::PlaytimeView,
    /// The newest backup of the game itself, when there is one.
    game_backup: Option<GameBackupView>,
}

/// Playtime and game backup of a game only the cloud has.
#[tauri::command]
pub async fn cloud_game_info(key: String) -> CmdResult<CloudGameInfo> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let cloud = ctx.cloud()?;
        let store = cloud.files();
        // The game's folders; its key's own when the cloud can't say (a game
        // only the cloud has).
        let at = Folders::of(cloud, &key).unwrap_or_else(|_| Folders::of_key(&key));
        let playtime = playtime::sync(store, &ctx.cfg, &at).unwrap_or_else(|_| playtime::summary(&ctx.cfg, &key));
        let _ = artsync::pull(store, &ctx.cfg, &at, None);
        let game_backup = gamebackup::open(&ctx.cfg)
            .and_then(|p| remote::list_versions(p.files(), &key))
            .ok()
            .and_then(|v| {
                let n = v.len();
                v.into_iter().last().map(|s| GameBackupView { versions: n, size: s.total_size, created_at: s.created_at, device_name: s.device_name })
            });
        let playtime = crate::commands::playtime_view(&ctx.cfg, playtime);
        Ok(CloudGameInfo { playtime, game_backup })
    })
    .await
}

/// Show `keys` as versions of one game.
#[tauri::command]
pub async fn link_games(keys: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        let ctx = Ctx::load()?;
        savelib::link(ctx.cloud().ok(), &keys)?;
        Ok(())
    })
    .await
}

/// Stop showing `key` together with other games.
#[tauri::command]
pub async fn unlink_game(key: String) -> CmdResult<()> {
    blocking(move || {
        let ctx = Ctx::load()?;
        savelib::unlink(ctx.cloud().ok(), &key)
    })
    .await
}

fn base_of(base: Option<String>) -> Option<PathBuf> {
    base.map(|b| PathBuf::from(b.trim())).filter(|b| !b.as_os_str().is_empty())
}

/// What applying save version `id` of `source` to `target` would do.
#[tauri::command]
pub async fn transfer_plan(source: String, id: String, target: String, base: Option<String>) -> CmdResult<transfer::Plan> {
    blocking(move || transfer::plan(&Ctx::load()?, &source, &id, &target, base_of(base).as_deref())).await
}

/// Apply save version `id` of `source` to `target`: a game on this PC, or a
/// Switch version only in the cloud (see `transfer`).
#[tauri::command]
pub async fn transfer_apply(source: String, id: String, target: String, base: Option<String>) -> CmdResult<Outcome> {
    blocking(move || {
        let ctx = Ctx::load()?;
        if let Some(g) = ctx.cfg.game(&target) {
            if watcher::runner_active(&target) || process::is_running(&watcher::watch_names(g)) {
                bail!(tr!("「{}」正在运行，请先退出游戏", "\"{}\" is running. Quit it first", g.name));
            }
        }
        let Some(_busy) = busy::begin(&target) else {
            bail!(tr!("正在同步，请稍候", "Syncing; please wait"));
        };
        transfer::apply(&ctx, &source, &id, &target, base_of(base).as_deref())
    })
    .await
}
