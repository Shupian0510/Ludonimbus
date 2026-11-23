//! Background watcher (runs while the app sits in the tray):
//! - backs up games that were started without the Steam wrapper, after they exit
//! - notices `shortcuts.vdf` changes so the UI can flag broken shortcuts

use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution};
use ludonimbus_core::{config, logger, paths, process, spec, steam, util};
use serde::Serialize;
use std::collections::HashSet;
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

/// Whether `ludonimbus-run` is currently handling this game.
pub fn runner_active(key: &str) -> bool {
    let p = paths::running_dir().join(format!("{key}.json"));
    let Ok(text) = std::fs::read_to_string(&p) else {
        return false;
    };
    let pid = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["pid"].as_u64())
        .unwrap_or(0) as u32;
    if pid != 0 && process::pid_alive(pid) {
        return true;
    }
    let _ = std::fs::remove_file(&p);
    false
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

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut observed: HashSet<String> = HashSet::new();
        let mut last_vdf: Option<SystemTime> = None;
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            tick += 1;
            let Ok(cfg) = config::load() else { continue };
            let procs = process::process_names();
            let mut changed = false;

            for g in cfg.games.iter().filter(|g| g.auto_sync) {
                let names = watch_names(g);
                let running = names.iter().any(|n| procs.contains(n));
                let wrapped = runner_active(&g.key);
                if running && !wrapped {
                    if observed.insert(g.key.clone()) {
                        logger::log("watcher", format!("{}: started outside the Steam wrapper", g.key));
                        changed = true;
                    }
                } else if !running && observed.remove(&g.key) {
                    changed = true;
                    if wrapped {
                        continue;
                    }
                    // Give the game a moment to flush its saves.
                    std::thread::sleep(Duration::from_secs(2));
                    let ctx = Ctx::from_config(cfg.clone());
                    let result = sync::sync(&ctx, g, Resolution::Auto, Trigger::Passive);
                    logger::log(
                        "watcher",
                        format!("{}: passive sync {:?}", g.key, result.as_ref().map_err(|e| e.to_string())),
                    );
                    let ev = SyncEvent {
                        key: g.key.clone(),
                        name: g.name.clone(),
                        outcome: result.as_ref().ok().cloned(),
                        error: result.err().map(|e| format!("{e:#}")),
                    };
                    let _ = app.emit("sync-finished", ev);
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
