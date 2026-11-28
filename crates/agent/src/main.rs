//! `ludonimbus-agent`: Ludonimbus without a window, for Steam Deck Game Mode
//! and other screens without a desktop (docs/plan-platforms.md §2.1). Run by
//! `systemd --user` (see `scripts/install-linux.sh`).
//!
//! - The local API for the Decky plugin (and the Millennium plugin on a
//!   Linux desktop without the tray app): the same server as the app's,
//!   `ludonimbus_core::localapi`. Without a window it has no "open in
//!   Ludonimbus" and no game backups; achievements are read but not watched,
//!   so no unlock events come from here. See docs/steam-plugin.md.
//! - The tray app's background work: finishing the sessions of launch
//!   wrappers that were terminated, uploading screenshots, the Switch's LAN
//!   connection, and the uploads that wait ([`backlog`]): the syncs after
//!   playing a wrapper handed over, and the versions in the queue (made
//!   while the cloud couldn't be reached or syncing was paused), every few
//!   minutes and right after syncing goes on again.
//! - While syncing is paused ([`ludonimbus_core::pause`]) nothing goes to
//!   the cloud from here: no backlog, no screenshots; a terminated wrapper's
//!   session is recorded and its saves wait in the queue.
//!
//! It steps aside while the desktop app runs: the app writes its own
//! `agent.json` and its watcher does the same work.

use ludonimbus_core::config::AppConfig;
use ludonimbus_core::localapi::{self, Busy, Guard, Host};
use ludonimbus_core::sync::{self, Ctx};
use ludonimbus_core::{catchup, config, logger, pause, process};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the uploads that wait are looked at.
const BACKLOG_EVERY: Duration = Duration::from_secs(3 * 60);

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

/// The local API's host without a window: the games this process syncs.
struct Headless {
    busy: Busy,
}

impl Host for Headless {
    fn begin(&self, key: &str) -> Option<Guard> {
        self.busy.begin(key)
    }

    fn is_busy(&self, key: &str) -> bool {
        self.busy.is_busy(key)
    }
}

/// Uploads that wait, as the tray app's watcher does them: finish the syncs
/// after playing that launch wrappers handed over, then upload the versions
/// in the queue. Never for a game that is being played or synced. `false`
/// when something couldn't be done (the cloud wasn't reached, mostly).
fn backlog(busy: &Busy, cfg: &AppConfig) -> bool {
    let ctx = Ctx::from_config(cfg.clone());
    let cloud = match ctx.cloud() {
        Ok(c) => c,
        Err(e) if ludonimbus_core::cloud::is_not_configured(&e) => return true,
        Err(e) => {
            log(format!("waiting uploads: no cloud ({e:#})"));
            return false;
        }
    };
    let procs = process::process_names();
    let free = |key: &str| {
        let up = cfg.game(key).is_some_and(|g| catchup::watch_names(g).iter().any(|n| procs.contains(&n.to_lowercase())));
        catchup::runner_phase(key).is_none() && !up && !busy.is_busy(key) && !sync::sync_running(key)
    };
    let mut ok = true;
    let owed: Vec<String> = sync::owed_keys().into_iter().filter(|k| free(k)).collect();
    for key in &owed {
        if pause::is_paused() {
            return ok;
        }
        let Some(_busy) = busy.begin(key) else { continue };
        match sync::finish_owed(&ctx, key) {
            Ok(None) => {}
            Ok(Some(out)) => {
                log(format!("{key}: post-exit (handed over) {out:?}"));
                if out.is_final() {
                    if let Some(g) = cfg.game(key) {
                        ludonimbus_core::achievements::after_exit(&ctx, g);
                    }
                } else {
                    ok = false;
                }
            }
            Err(e) => {
                log(format!("{key}: post-exit (handed over) failed: {e:#}"));
                ok = false;
            }
        }
    }
    for key in sync::pending_keys().into_iter().filter(|k| free(k) && !owed.contains(k)) {
        if pause::is_paused() {
            return ok;
        }
        let Some(_busy) = busy.begin(&key) else { continue };
        match sync::flush_pending(cloud, &key) {
            Ok(0) => {}
            Ok(n) => log(format!("{key}: uploaded {n} waiting versions")),
            Err(e) => {
                log(format!("{key}: uploading waiting versions failed: {e:#}"));
                ok = false;
            }
        }
    }
    ok
}

fn main() {
    // Inside the Flatpak: the user's folders, where the rest of Ludonimbus is.
    ludonimbus_core::paths::use_host_folders();
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
    let busy = Busy::default();
    let server = match localapi::start(Headless { busy: busy.clone() }) {
        Ok(s) => s,
        Err(e) => {
            log(format!("{e:#}"));
            std::process::exit(1);
        }
    };
    log(format!("local API listening on 127.0.0.1:{}", server.port()));
    // As the desktop app does at start: save locations from Ludusavi's
    // database for the games it finds the saves of, so restores here are
    // checked against them too.
    std::thread::spawn(|| {
        if let Err(e) = ludonimbus_core::games::refresh_ludusavi_saves(None) {
            log(format!("the save locations from Ludusavi's database were not updated: {e:#}"));
        }
    });

    let me = std::process::id();
    let mut last_shots = Instant::now() - Duration::from_secs(25 * 60);
    let mut nx: Option<ludonimbus_core::nxlan::Server> = None;
    let mut next_backlog = Instant::now() + Duration::from_secs(20);
    let backlog_running = Arc::new(AtomicBool::new(false));
    let mut was_paused = pause::is_paused();
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
        let paused = pause::is_paused();
        if !app_runs {
            if let Ok(cfg) = config::load() {
                catchup::stale_runners(&cfg, None, &|m| log(m));
                if !paused && last_shots.elapsed() > Duration::from_secs(30 * 60) {
                    last_shots = Instant::now();
                    for g in cfg.games.iter().filter(|g| g.steam.is_some()) {
                        catchup::screenshots(&cfg, &g.key, &|m| log(m));
                    }
                }
                // What waits goes up right after syncing goes on again, and
                // every few minutes; one pass at a time, on its own thread.
                let resumed = was_paused && !paused;
                if !paused && (resumed || Instant::now() >= next_backlog) && !backlog_running.swap(true, Ordering::SeqCst) {
                    next_backlog = Instant::now() + BACKLOG_EVERY;
                    if resumed {
                        log("syncing goes on again: uploading what waits");
                    }
                    let (busy, running) = (busy.clone(), backlog_running.clone());
                    std::thread::spawn(move || {
                        if !backlog(&busy, &cfg) {
                            log("some waiting uploads are left for later");
                        }
                        running.store(false, Ordering::SeqCst);
                    });
                }
            }
        }
        was_paused = paused;
        std::thread::sleep(Duration::from_secs(5));
    }
}
