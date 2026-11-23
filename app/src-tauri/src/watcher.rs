//! Background watcher (runs while the app sits in the tray):
//! - backs up games that were started without the Steam wrapper, after they exit
//! - recovers when the wrapper was killed before it could upload (e.g. the user
//!   pressed "Stop" in Steam): its running marker is left behind, so we sync
//! - tells the UI when a game stops so statuses refresh
//! - notices `shortcuts.vdf` changes so the UI can flag broken shortcuts

use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution};
use ludonimbus_core::{config, logger, paths, process, spec, steam, util};
use serde::Serialize;
use std::collections::HashMap;
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

/// Process names that mean "this game is running".
pub fn watch_names(g: &config::GameEntry) -> Vec<String> {
    let mut names: Vec<String> = spec::effective(g).wait_for.iter().map(|s| s.to_lowercase()).collect();
    if let Some(exe) = &g.exe {
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
    std::thread::spawn(move || {
        // Games currently running → whether the Steam wrapper was handling them.
        let mut running: HashMap<String, bool> = HashMap::new();
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
                if runner == Runner::Active || game_up {
                    let started = !running.contains_key(&g.key);
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
                if runner == Runner::Stale {
                    let _ = std::fs::remove_file(marker(&g.key));
                    changed = true;
                    if g.auto_sync {
                        background_sync(&app, &cfg, g, "the Steam wrapper was terminated before it finished");
                    }
                } else if let Some(wrapped) = was {
                    changed = true;
                    if !wrapped && g.auto_sync {
                        background_sync(&app, &cfg, g, "game started outside the Steam wrapper has exited");
                    }
                    let _ = app.emit("game-stopped", g.key.clone());
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
