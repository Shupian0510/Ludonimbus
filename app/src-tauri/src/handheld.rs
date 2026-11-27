//! Handheld mode (docs/plan-phase5.md §1–2).
//!
//! `ludonimbus-app --handheld` (what the Steam / Xbox app shortcut starts)
//! opens the controller screen full screen; `ludonimbus-app --prompt <file>`
//! shows one question of the launch wrapper full screen and exits. Both run
//! as their own process without tray, watcher or local API (the tray app,
//! when it runs, keeps doing that work), so Steam treats the window as the
//! running "game" and shows it.

use crate::commands::{blocking, with_steam_closed, CmdResult};
use anyhow::anyhow;
use ludonimbus_core::config::{self, HandheldMode};
use ludonimbus_core::prompt::{self, Request};
use ludonimbus_core::savelib::{self, Newer};
use ludonimbus_core::sync::Ctx;
use ludonimbus_core::{handheld, steam};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, WebviewWindow};

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Desktop,
    Handheld,
    Prompt(PathBuf),
}

/// How this process was started.
pub fn mode() -> &'static Mode {
    static M: OnceLock<Mode> = OnceLock::new();
    M.get_or_init(|| {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(i) = args.iter().position(|a| a == "--prompt") {
            if let Some(f) = args.get(i + 1) {
                return Mode::Prompt(PathBuf::from(f));
            }
        }
        if args.iter().any(|a| a == handheld::HANDHELD_ARG) {
            return Mode::Handheld;
        }
        Mode::Desktop
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchInfo {
    /// "desktop", "handheld" or "prompt".
    mode: &'static str,
    prompt: Option<Request>,
}

#[tauri::command]
pub async fn launch_mode() -> CmdResult<LaunchInfo> {
    blocking(|| {
        Ok(match mode() {
            Mode::Desktop => LaunchInfo { mode: "desktop", prompt: None },
            Mode::Handheld => LaunchInfo { mode: "handheld", prompt: None },
            Mode::Prompt(f) => LaunchInfo { mode: "prompt", prompt: Some(prompt::read_request(f)?) },
        })
    })
    .await
}

/// The wrapper's question was answered: tell it and close.
#[tauri::command]
pub async fn prompt_answer(app: AppHandle, choice: Option<String>) -> CmdResult<()> {
    if let Mode::Prompt(f) = mode() {
        prompt::write_answer(f, choice.as_deref()).map_err(|e| format!("{e:#}"))?;
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn set_fullscreen(window: WebviewWindow, on: bool) -> CmdResult<()> {
    window.set_fullscreen(on).map_err(|e| e.to_string())
}

/// Close the handheld screen (its own process; the tray app keeps running).
#[tauri::command]
pub async fn quit_handheld(app: AppHandle, window: WebviewWindow) -> CmdResult<()> {
    if *mode() == Mode::Desktop {
        let _ = window.set_fullscreen(false);
    } else {
        app.exit(0);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InSteam {
    account: String,
    appid: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandheldInfo {
    /// The handheld this is, by its firmware ("ROG Ally"…).
    device: Option<String>,
    /// Started from Steam's controller interface right now.
    steam_gamepad_ui: bool,
    mode: HandheldMode,
    /// The Steam shortcut to the handheld screen.
    in_steam: Option<InSteam>,
}

#[tauri::command]
pub async fn handheld_info() -> CmdResult<HandheldInfo> {
    blocking(|| {
        let cfg = config::load()?;
        let in_steam = steam::root(&cfg).and_then(|root| {
            steam::accounts(&root).into_iter().find_map(|a| handheld::steam_shortcut(&root, &a.id).map(|s| InSteam { account: a.id.clone(), appid: s.appid }))
        });
        Ok(HandheldInfo { device: handheld::device(), steam_gamepad_ui: handheld::steam_gamepad_ui(), mode: cfg.handheld, in_steam })
    })
    .await
}

#[tauri::command]
pub async fn set_handheld_mode(mode: HandheldMode) -> CmdResult<()> {
    blocking(move || {
        config::update(|c| {
            c.handheld = mode;
            Ok(())
        })
    })
    .await
}

/// Put Ludonimbus's handheld screen in the Steam library.
#[tauri::command]
pub async fn steam_add_ludonimbus(account: String, close_steam: bool) -> CmdResult<u32> {
    blocking(move || {
        let app = std::env::current_exe()?;
        with_steam_closed(close_steam, || {
            let cfg = config::load()?;
            let root = steam::root(&cfg).ok_or_else(|| anyhow!(ludonimbus_core::tr!("没有找到 Steam", "Steam not found")))?;
            handheld::add_to_steam(&root, &account, &app)
        })
    })
    .await
}

/// Another version of this game with newer saves (see `savelib::newer_version`).
#[tauri::command]
pub async fn newer_version(key: String) -> CmdResult<Option<Newer>> {
    blocking(move || {
        let ctx = Ctx::load()?;
        let g = ctx.game(&key)?;
        savelib::newer_version(&ctx, &g)
    })
    .await
}

#[tauri::command]
pub async fn dismiss_newer(key: String, id: String) -> CmdResult<()> {
    blocking(move || savelib::dismiss_newer(&key, &id)).await
}
