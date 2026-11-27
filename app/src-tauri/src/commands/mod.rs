//! Commands invoked by the web UI, one file per area of the app. All heavy
//! work runs on blocking threads ([`blocking`]); what reads or changes one
//! game goes through [`with_game`] or its stricter forms below.
//!
//! main.rs registers each command by its path here (`commands::saves::sync_game`).

use crate::{busy, watcher};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config::GameEntry;
use ludonimbus_core::sync::{self, Ctx};

pub mod cloud;
pub mod games;
pub mod lines;
pub mod overview;
pub mod saves;
pub mod screenshots;
pub mod settings;
pub mod steam;

// What the other modules of the app use from here.
pub(crate) use games::adopt_cloud_key;
pub(crate) use overview::{cloud_only_views, playtime_view, CloudOnlyView, PlaytimeView};
pub(crate) use steam::with_steam_closed;

pub(crate) type CmdResult<T> = std::result::Result<T, String>;

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

fn with_game<T>(key: &str, f: impl FnOnce(&Ctx, &GameEntry) -> Result<T>) -> Result<T> {
    let ctx = Ctx::load()?;
    let game = ctx.game(key)?;
    f(&ctx, &game)
}

fn syncing() -> anyhow::Error {
    anyhow!(ludonimbus_core::tr!("正在同步，请稍候", "Syncing; please wait"))
}

/// [`with_game`] for what reads or changes a game's saves on this PC, or the
/// save line it follows: never while the game is played, through the launch
/// wrapper or started any other way (directly, by an emulator, by the Xbox
/// app: [`watcher::game_running`]), nor while this app syncs it already.
/// `running` says why when it is played.
fn with_free_game<T>(key: &str, running: String, f: impl FnOnce(&Ctx, &GameEntry) -> Result<T>) -> Result<T> {
    if watcher::runner_active(key) {
        bail!(running);
    }
    let _busy = busy::begin(key).ok_or_else(syncing)?;
    with_game(key, |ctx, g| {
        if watcher::game_running(g) {
            bail!(running);
        }
        f(ctx, g)
    })
}

/// [`with_game`] for what changes a game's history in the cloud and nothing
/// on this PC (keep, name or delete a version, put one back from 「最近删除」):
/// one at a time with the syncs of the game, in this app and in any other
/// program ([`sync::lock_game`]). Asks to wait rather than waits.
fn with_free_line<T>(key: &str, f: impl FnOnce(&Ctx, &GameEntry) -> Result<T>) -> Result<T> {
    let _busy = busy::begin(key).ok_or_else(syncing)?;
    if sync::sync_running(key) {
        return Err(syncing());
    }
    let _lock = sync::lock_game(key, &sync::Gate::default())?;
    with_game(key, f)
}

fn restoring_while_running() -> String {
    ludonimbus_core::tr!("游戏正在运行，请先退出游戏再恢复存档", "The game is running. Quit it before restoring saves")
}
