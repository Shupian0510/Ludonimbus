//! Background watcher (runs while the app sits in the tray):
//! - backs up games that were started without the Steam wrapper, after they exit
//! - recovers when the wrapper was killed before it could upload (e.g. the user
//!   pressed "Stop" in Steam): its running marker is left behind, so we sync
//! - records the playtime of games started without the wrapper, and of
//!   sessions whose wrapper was terminated
//! - syncs Steam screenshots after a game stops, and every 30 minutes
//! - tells the UI when a game stops so statuses refresh
//! - notices `shortcuts.vdf` changes so the UI can flag broken shortcuts

use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution};
use chrono::{DateTime, Utc};
use ludonimbus_core::playtime::{self, Via};
use ludonimbus_core::{config, logger, paths, process, spec, steam, util};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub key: String,
    pub name: String,
    pub outcome: Option<sync::Outcome>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    None,
    Active,
    /// The marker exists but its process is gone: the wrapper was terminated
    /// before it finished (and before it uploaded).
    Stale,
}

fn marker(key: &str) -> std::path::PathBuf {
    paths::running_dir().join(format!("{key}.json"))
}

pub fn runner_state(key: &str) -> Runner {
    let Ok(text) = std::fs::read_to_string(marker(key)) else {
        return Runner::None;
    };
    let pid = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["pid"].as_u64())
        .unwrap_or(0) as u32;
    if pid != 0 && process::pid_alive(pid) {
        Runner::Active
    } else {
        Runner::Stale
    }
}

/// Whether `ludonimbus-run` is currently handling this game.
pub fn runner_active(key: &str) -> bool {
    runner_state(key) == Runner::Active
}

/// What the wrapper is doing: "preLaunch", "playing" or "uploading"
/// (absent for wrappers older than 0.2.0).
pub fn runner_phase(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(marker(key)).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?["phase"]
        .as_str()
        .map(str::to_string)
}

/// When the game started, as written by the wrapper (0.6.0 and later). Gone
/// once the wrapper recorded the session itself.
fn runner_started_at(key: &str) -> Option<DateTime<Utc>> {
    let text = std::fs::read_to_string(marker(key)).ok()?;
    serde_json::from_value(serde_json::from_str::<serde_json::Value>(&text).ok()?["startedAt"].clone()).ok()
}

fn record_playtime(cfg: &config::AppConfig, g: &config::GameEntry, start: DateTime<Utc>, via: Via) {
    match playtime::record(cfg, g, start, Utc::now(), via) {
        Ok(true) => {
            logger::log("watcher", format!("{}: recorded {}s of playtime", g.key, (Utc::now() - start).num_seconds()));
            // Without auto-sync no save sync follows to upload it.
            if !g.auto_sync {
                let (cfg, key) = (cfg.clone(), g.key.clone());
                std::thread::spawn(move || playtime::upload_quietly(&cfg, &key, |m| logger::log("watcher", m)));
            }
        }
        Ok(false) => {}
        Err(e) => logger::log("watcher", format!("{}: playtime: {e:#}", g.key)),
    }
}

static SCREENSHOTS_BUSY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Upload / download Steam screenshots of the given games (all linked games
/// when `only` is `None`), skipping games that are running. One run at a time.
pub fn sync_screenshots(app: &AppHandle, only: Option<String>) -> Result<ludonimbus_core::screenshots::Report, String> {
    let _guard = SCREENSHOTS_BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let cfg = config::load().map_err(|e| format!("{e:#}"))?;
    let mut total = ludonimbus_core::screenshots::Report::default();
    if cfg.screenshots == config::ScreenshotMode::Off {
        return Ok(total);
    }
    let cloud = match ludonimbus_core::cloud::open(&cfg) {
        Ok(Some(c)) => c,
        Ok(None) => return Ok(total),
        Err(e) => return Err(format!("{e:#}")),
    };
    let procs = process::process_names();
    let log = |m: String| logger::log("screenshots", m);
    let mut last_error = None;
    for g in cfg.games.iter().filter(|g| g.steam.is_some()) {
        if only.as_deref().is_some_and(|k| k != g.key) {
            continue;
        }
        if runner_active(&g.key) || watch_names(g).iter().any(|n| procs.contains(n)) {
            continue;
        }
        match ludonimbus_core::screenshots::sync(cloud.files(), &cfg, g, &log) {
            Ok(r) => {
                if r.uploaded + r.downloaded + r.failed > 0 {
                    log(format!("{}: screenshots {r:?}", g.key));
                }
                total.uploaded += r.uploaded;
                total.downloaded += r.downloaded;
                total.failed += r.failed;
            }
            Err(e) => {
                log(format!("{}: screenshot sync failed: {e:#}", g.key));
                last_error = Some(format!("{e:#}"));
            }
        }
    }
    if total.uploaded + total.downloaded > 0 {
        let _ = app.emit("screenshots-changed", only.clone());
    }
    match last_error {
        Some(e) if only.is_some() => Err(e),
        _ => Ok(total),
    }
}

fn sync_screenshots_later(app: &AppHandle, key: &str) {
    let (app, key) = (app.clone(), key.to_string());
    std::thread::spawn(move || {
        // Let the save upload go first.
        std::thread::sleep(Duration::from_secs(15));
        let _ = sync_screenshots(&app, Some(key));
    });
}

/// Process names that mean "this game is running".
pub fn watch_names(g: &config::GameEntry) -> Vec<String> {
    let mut names: Vec<String> = spec::effective(g).wait_for.iter().map(|s| s.to_lowercase()).collect();
    // Every Xbox app game has the same launch helper: only its own processes count.
    if let Some(exe) = g.exe.as_deref().filter(|e| !ludonimbus_core::xbox::is_launch_helper(e)) {
        names.push(util::file_name_lower(exe));
    }
    names
}

fn vdf_stamp(cfg: &config::AppConfig) -> Option<SystemTime> {
    let root = steam::root(cfg)?;
    cfg.games
        .iter()
        .filter_map(|g| g.steam.as_ref())
        .filter_map(|l| std::fs::metadata(steam::shortcuts_path(&root, &l.account)).ok()?.modified().ok())
        .max()
}

fn background_sync(app: &AppHandle, cfg: &config::AppConfig, g: &config::GameEntry, why: &str) {
    let Some(_busy) = crate::busy::begin(&g.key) else {
        logger::log("watcher", format!("{}: {why}, but a sync is already running", g.key));
        return;
    };
    logger::log("watcher", format!("{}: {why}, syncing", g.key));
    // Give the game a moment to flush its saves.
    std::thread::sleep(Duration::from_secs(2));
    let ctx = Ctx::from_config(cfg.clone());
    let result = sync::sync(&ctx, g, Resolution::Auto, Trigger::Passive);
    logger::log(
        "watcher",
        format!("{}: background sync {:?}", g.key, result.as_ref().map_err(|e| e.to_string())),
    );
    let _ = app.emit(
        "sync-finished",
        SyncEvent {
            key: g.key.clone(),
            name: g.name.clone(),
            outcome: result.as_ref().ok().cloned(),
            error: result.err().map(|e| format!("{e:#}")),
        },
    );
}

pub fn spawn(app: AppHandle) {
    // Screenshots: catch up shortly after start, then every 30 minutes.
    let shots_app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let _ = sync_screenshots(&shots_app, None);
            std::thread::sleep(Duration::from_secs(30 * 60));
        }
    });
    std::thread::spawn(move || {
        // Games currently running → whether the Steam wrapper was handling them.
        let mut running: HashMap<String, bool> = HashMap::new();
        // When games started outside the wrapper were first seen.
        let mut seen_at: HashMap<String, DateTime<Utc>> = HashMap::new();
        // Games whose real game process (`waitFor`) is up, for upload-on-game-exit.
        let mut game_procs: HashSet<String> = HashSet::new();
        let mut last_vdf: Option<SystemTime> = None;
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            tick += 1;
            let Ok(cfg) = config::load() else { continue };
            let procs = process::process_names();
            let mut changed = false;

            for g in &cfg.games {
                let runner = runner_state(&g.key);
                let game_up = watch_names(g).iter().any(|n| procs.contains(n));
                let eff = spec::effective(g);
                let game_proc_up = eff.wait_for.iter().any(|n| procs.contains(&n.to_lowercase()));
                let game_proc_ended = if game_proc_up {
                    game_procs.insert(g.key.clone());
                    false
                } else {
                    game_procs.remove(&g.key)
                };
                if runner == Runner::Active || game_up {
                    // Started outside Steam, launcher still open, game closed.
                    if game_proc_ended
                        && eff.upload_on_game_exit
                        && g.auto_sync
                        && runner != Runner::Active
                        && running.get(&g.key) == Some(&false)
                    {
                        background_sync(&app, &cfg, g, "game process exited while the launcher is still open");
                        changed = true;
                    }
                    let started = !running.contains_key(&g.key);
                    if started {
                        seen_at.insert(g.key.clone(), Utc::now());
                    }
                    let wrapped = running.entry(g.key.clone()).or_insert(false);
                    if runner == Runner::Active {
                        *wrapped = true;
                    }
                    if started {
                        changed = true;
                        if !*wrapped {
                            logger::log("watcher", format!("{}: started outside the Steam wrapper", g.key));
                        }
                    }
                    continue;
                }
                // Not running any more.
                let was = running.remove(&g.key);
                let first_seen = seen_at.remove(&g.key);
                if runner == Runner::Stale {
                    if let Some(start) = runner_started_at(&g.key) {
                        record_playtime(&cfg, g, start, Via::Steam);
                    }
                    let _ = std::fs::remove_file(marker(&g.key));
                    changed = true;
                    sync_screenshots_later(&app, &g.key);
                    if g.auto_sync {
                        background_sync(&app, &cfg, g, "the Steam wrapper was terminated before it finished");
                    }
                } else if let Some(wrapped) = was {
                    changed = true;
                    if let (false, Some(start)) = (wrapped, first_seen) {
                        record_playtime(&cfg, g, start, Via::Direct);
                    }
                    if !wrapped && g.auto_sync {
                        background_sync(&app, &cfg, g, "game started outside the Steam wrapper has exited");
                    }
                    let _ = app.emit("game-stopped", g.key.clone());
                    sync_screenshots_later(&app, &g.key);
                }
            }

            if tick % 5 == 0 {
                let stamp = vdf_stamp(&cfg);
                if stamp != last_vdf {
                    if last_vdf.is_some() {
                        changed = true;
                    }
                    last_vdf = stamp;
                }
            }
            if changed {
                let _ = app.emit("library-changed", ());
            }
        }
    });
}
