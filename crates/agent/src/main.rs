//! `ludonimbus-agent`: Ludonimbus without a window, for Steam Deck Game Mode
//! and other screens without a desktop (docs/plan-platforms.md §2.1). Run by
//! `systemd --user` (see `scripts/install-linux.sh`).
//!
//! - The local API for the Decky plugin (and the Millennium plugin on a
//!   Linux desktop without the tray app): the same server as the app's,
//!   `ludonimbus_core::localapi`, API 4. Without a window it has no "open in
//!   Ludonimbus" and no game backups; achievements are read but not watched,
//!   so no unlock events come from here. See docs/steam-plugin.md.
//! - The tray app's background work: finishing the sessions of launch
//!   wrappers that were terminated, uploading screenshots, the Switch's LAN
//!   connection.
//!
//! It steps aside while the desktop app runs: the app writes its own
//! `agent.json` and its watcher does the same work.

use ludonimbus_core::localapi::{self, Guard, Host};
use ludonimbus_core::{catchup, config, logger};
use std::time::{Duration, Instant};

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

/// The local API's host without a window: the games this process syncs.
#[derive(Default)]
struct Headless {
    busy: localapi::Busy,
}

impl Host for Headless {
    fn begin(&self, key: &str) -> Option<Guard> {
        self.busy.begin(key)
    }

    fn is_busy(&self, key: &str) -> bool {
        self.busy.is_busy(key)
    }
}

fn main() {
    let once = std::env::args().any(|a| a == "--once");
    log(format!("ludonimbus-agent {} starting", ludonimbus_core::APP_VERSION));
    #[cfg(unix)]
    if let Err(e) = ludonimbus_core::paths::install_runner() {
        log(format!("could not install the launch wrapper: {e}"));
    }
    if once {
        if let Ok(cfg) = config::load() {
            catchup::stale_runners(&cfg, None, &|m| log(m));
        }
        return;
    }
    let server = match localapi::start(Headless::default()) {
        Ok(s) => s,
        Err(e) => {
            log(format!("{e:#}"));
            std::process::exit(1);
        }
    };
    log(format!("local API listening on 127.0.0.1:{}", server.port()));

    let me = std::process::id();
    let mut last_shots = Instant::now() - Duration::from_secs(25 * 60);
    let mut nx: Option<ludonimbus_core::nxlan::Server> = None;
    loop {
        // The desktop app runs: it does all of this itself.
        let owner = localapi::live_owner();
        let app_runs = owner.is_some_and(|p| p != me);
        if owner.is_none() {
            match server.publish() {
                Ok(()) => log("published the local API in agent.json"),
                Err(e) => log(format!("could not write agent.json: {e:#}")),
            }
        }
        // The Switch client's LAN connection: here while the app doesn't run.
        let nx_on = config::load().map(|c| c.nx_lan).unwrap_or(false);
        if nx_on && !app_runs {
            if nx.is_none() {
                match ludonimbus_core::nxlan::start(|m| logger::log("nx", m)) {
                    Ok(s) => nx = Some(s),
                    Err(e) => log(format!("Switch connection: {e:#}")),
                }
            }
        } else {
            nx = None;
        }
        if !app_runs {
            if let Ok(cfg) = config::load() {
                catchup::stale_runners(&cfg, None, &|m| log(m));
                if last_shots.elapsed() > Duration::from_secs(30 * 60) {
                    last_shots = Instant::now();
                    for g in cfg.games.iter().filter(|g| g.steam.is_some()) {
                        catchup::screenshots(&cfg, &g.key, &|m| log(m));
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
