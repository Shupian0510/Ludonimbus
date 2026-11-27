//! The local HTTP API for the Steam plugins while the app sits in the tray.
//! The server, its endpoints and `agent.json` are in core
//! (`crates/core/src/localapi`, shared with the headless `ludonimbus-agent`);
//! this is what the app adds: its list of games being synced, telling the
//! window when a sync the plugin started ends, showing a game in the window,
//! and game backups.

use crate::{busy, gamebackup, show_main, watcher};
use ludonimbus_core::config::GameEntry;
use ludonimbus_core::localapi::{self, Guard, Host};
use ludonimbus_core::logger;
use ludonimbus_core::sync::Outcome;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

struct App(AppHandle);

impl Host for App {
    fn headless(&self) -> bool {
        false
    }

    fn begin(&self, key: &str) -> Option<Guard> {
        busy::begin(key).map(|g| Box::new(g) as Guard)
    }

    fn is_busy(&self, key: &str) -> bool {
        busy::is_busy(key)
    }

    fn game_running(&self, g: &GameEntry) -> bool {
        watcher::game_running(g)
    }

    fn finished(&self, g: &GameEntry, result: &anyhow::Result<Outcome>) {
        let event = watcher::SyncEvent {
            key: g.key.clone(),
            name: g.name.clone(),
            outcome: result.as_ref().ok().cloned(),
            error: result.as_ref().err().map(|e| format!("{e:#}")),
        };
        let _ = self.0.emit("sync-finished", event);
    }

    fn open_game(&self, key: &str) -> bool {
        show_main(&self.0);
        let _ = self.0.emit("open-game", key.to_string());
        true
    }

    fn game_backup_status(&self, key: &str) -> Option<anyhow::Result<serde_json::Value>> {
        Some(gamebackup::agent_status(key))
    }

    fn game_backup_install(&self, key: &str) -> Option<anyhow::Result<serde_json::Value>> {
        Some(gamebackup::agent_install(&self.0, key))
    }
}

/// The running server: it stops when dropped.
static SERVER: OnceLock<localapi::Server> = OnceLock::new();

/// Start the API and publish it in `agent.json`.
pub fn spawn(app: AppHandle) {
    let server = match localapi::start(App(app)) {
        Ok(s) => s,
        Err(e) => {
            log(format!("{e:#}"));
            return;
        }
    };
    match server.publish() {
        Ok(()) => log(format!("local API listening on 127.0.0.1:{}", server.port())),
        Err(e) => {
            log(format!("could not write {}: {e:#}", localapi::agent_file().display()));
            return;
        }
    }
    let _ = SERVER.set(server);
}

/// Remove `agent.json` when this process wrote it (called on exit).
pub fn remove_agent_file() {
    localapi::remove_agent_file();
}
