//! Handheld mode (docs/plan-phase5.md §1–2).
//!
//! `ludonimbus-app --handheld` (what the Steam / Xbox app shortcut starts)
//! opens the controller screen full screen; `ludonimbus-app --prompt <file>`
//! shows one question of the launch wrapper full screen and exits. Both run
//! as their own process without tray, watcher or local API (the tray app,
//! when it runs, keeps doing that work), so Steam treats the window as the
//! running "game" and shows it.
//!
//! Opened without either while a controller interface is up (the Xbox full
//! screen experience, Steam Big Picture; see `handheld::handheld_view`),
//! the desktop app shows the handheld view full screen too, and closing it
//! hides the window instead of showing the desktop view.

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
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use ts_rs::TS;

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

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LaunchInfo {
    /// "desktop", "handheld" or "prompt".
    #[ts(type = r#""desktop" | "handheld" | "prompt""#)]
    mode: &'static str,
    prompt: Option<Request>,
}

/// The desktop app was opened while a controller interface is up: it starts
/// in the handheld view, full screen.
pub fn starts_handheld() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| *mode() == Mode::Desktop && config::load().is_ok_and(|c| handheld::handheld_view(&c)))
}

/// The event that switches the desktop app's window to the handheld view.
pub const SHOW_HANDHELD: &str = "show-handheld";

/// Opened again (a second start, e.g. from the Xbox full screen
/// experience's library) while the desktop app runs: the handheld view when
/// a controller interface is up, else the window as it was.
pub fn show_again<R: Runtime>(app: &AppHandle<R>) {
    crate::show_main(app);
    if !config::load().is_ok_and(|c| handheld::handheld_view(&c)) {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_fullscreen(true);
        let _ = w.emit(SHOW_HANDHELD, ());
    }
}

#[tauri::command]
pub async fn launch_mode() -> CmdResult<LaunchInfo> {
    blocking(|| {
        Ok(match mode() {
            Mode::Desktop if starts_handheld() => LaunchInfo { mode: "handheld", prompt: None },
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

/// Zoom the handheld view to the screen (`handheld::view_zoom`), or back to
/// normal for the desktop view.
#[tauri::command]
pub async fn handheld_zoom(window: WebviewWindow, on: bool) -> CmdResult<()> {
    let zoom = match window.current_monitor() {
        Ok(Some(m)) if on => handheld::view_zoom(m.size().width, m.size().height, m.scale_factor()),
        _ => 1.0,
    };
    window.set_zoom(zoom).map_err(|e| e.to_string())
}

/// Close the handheld screen (its own process; the tray app keeps running).
/// In the desktop app it goes back to the desktop view, or, while a
/// controller interface is up (no tray, no desktop to go back to), hides
/// the window as closing it does.
#[tauri::command]
pub async fn quit_handheld(app: AppHandle, window: WebviewWindow) -> CmdResult<()> {
    if *mode() == Mode::Desktop {
        let _ = window.set_fullscreen(false);
        if handheld::controller_shell() {
            let _ = window.hide();
        }
    } else {
        app.exit(0);
    }
    Ok(())
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InSteam {
    account: String,
    appid: u32,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HandheldInfo {
    /// The handheld this is, by its firmware ("ROG Ally"…).
    device: Option<String>,
    /// Started from Steam's controller interface right now.
    steam_gamepad_ui: bool,
    mode: HandheldMode,
    /// The Steam shortcut to the handheld screen.
    in_steam: Option<InSteam>,
    /// The Xbox full screen experience is on right now.
    xbox_fse: bool,
}

#[tauri::command]
pub async fn handheld_info() -> CmdResult<HandheldInfo> {
    blocking(|| {
        let cfg = config::load()?;
        let in_steam = steam::root(&cfg).and_then(|root| {
            steam::accounts(&root).into_iter().find_map(|a| handheld::steam_shortcut(&root, &a.id).map(|s| InSteam { account: a.id.clone(), appid: s.appid }))
        });
        Ok(HandheldInfo {
            device: handheld::device(),
            steam_gamepad_ui: handheld::steam_gamepad_ui(),
            mode: cfg.handheld,
            in_steam,
            xbox_fse: handheld::xbox_fse(),
        })
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
