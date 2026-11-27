//! Background watcher (runs while the app sits in the tray):
//! - backs up games that were started without the Steam wrapper, after they exit
//! - recovers when the wrapper was killed before it could upload (e.g. the user
//!   pressed "Stop" in Steam): its running marker is left behind, so we sync
//! - records the playtime of games started without the wrapper, and of
//!   sessions whose wrapper was terminated
//! - syncs Steam screenshots after a game stops, and every 30 minutes
//! - tells the UI when a game stops so statuses refresh
//! - notices `shortcuts.vdf` changes so the UI can flag broken shortcuts
//! - finishes the syncs after playing that a wrapper handed over, and
//!   uploads what waits in the queue (see [`Backlog`])
//! - keeps Steam games that Ludonimbus manages without the wrapper up to
//!   date while they aren't running (see [`Unwrapped`])

use chrono::{DateTime, Utc};
use ludonimbus_core::catchup::{self, Wrapper};
use ludonimbus_core::playtime::{self, Via};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::steamcloud::{self, Polled};
use ludonimbus_core::sync::{self, Ctx, Resolution};
use ludonimbus_core::{config, logger, paths, process, spec, steam};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub key: String,
    pub name: String,
    pub outcome: Option<sync::Outcome>,
    pub error: Option<String>,
}

/// Whether `ludonimbus-run` is currently handling this game (or its marker
/// is being written: see [`catchup::wrapper`]).
pub fn runner_active(key: &str) -> bool {
    catchup::wrapper(key) == Wrapper::Running
}

/// What the wrapper is doing: "preLaunch", "playing" or "uploading";
/// `None` when none is running.
pub fn runner_phase(key: &str) -> Option<String> {
    catchup::runner_phase(key)
}

/// Whether the game is being played: through the launch wrapper, or started
/// any other way (directly, by an emulator, by the Xbox app), by the
/// process names core goes by ([`watch_names`]).
pub fn game_running(g: &config::GameEntry) -> bool {
    runner_active(&g.key) || process::is_running(&watch_names(g))
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

/// Process names that mean "this game is running" (as core has them).
pub fn watch_names(g: &config::GameEntry) -> Vec<String> {
    catchup::watch_names(g)
}

fn vdf_stamp(cfg: &config::AppConfig) -> Option<SystemTime> {
    let root = steam::root(cfg)?;
    cfg.games.iter().filter_map(|g| g.steam.as_ref()).filter_map(|l| std::fs::metadata(steam::shortcuts_path(&root, &l.account)).ok()?.modified().ok()).max()
}

/// Sync a game after it ran, on a thread of its own: a slow cloud never
/// holds up noticing other games start and stop, nor their playtime. One
/// sync of a game at a time: when this app syncs it already, this one is
/// noted as owed ([`sync::owe`]) and done after that one ([`Backlog`]).
fn background_sync(app: &AppHandle, cfg: &config::AppConfig, g: &config::GameEntry, why: &str) {
    let Some(busy) = crate::busy::begin(&g.key) else {
        logger::log("watcher", format!("{}: {why}, but a sync is already running; syncing after it", g.key));
        if let Err(e) = sync::owe(&g.key) {
            logger::log("watcher", format!("{}: can't note the sync for later ({e:#})", g.key));
        }
        return;
    };
    logger::log("watcher", format!("{}: {why}, syncing", g.key));
    let (app, cfg, g) = (app.clone(), cfg.clone(), g.clone());
    std::thread::spawn(move || {
        let _busy = busy;
        // Give the game a moment to flush its saves.
        std::thread::sleep(Duration::from_secs(2));
        let ctx = Ctx::from_config(cfg);
        let result = sync::sync(&ctx, &g, Resolution::Auto, Trigger::Passive);
        logger::log("watcher", format!("{}: background sync {:?}", g.key, result.as_ref().map_err(|e| e.to_string())));
        let _ = app.emit(
            "sync-finished",
            SyncEvent { key: g.key.clone(), name: g.name.clone(), outcome: result.as_ref().ok().cloned(), error: result.err().map(|e| format!("{e:#}")) },
        );
    });
}

/// The first look at the backlog after the app starts.
const BACKLOG_FIRST: Duration = Duration::from_secs(10);
/// How often the backlog is looked at while the cloud answers.
const BACKLOG_EVERY: Duration = Duration::from_secs(60);
/// The longest pause while the cloud can't be reached.
const BACKLOG_MAX: Duration = Duration::from_secs(5 * 60);

/// Uploads that wait (docs/plan-v2.md, section 5.5): syncs after playing a
/// launch wrapper handed over when it stopped waiting, and versions in the
/// queue. Looked at shortly after the app starts, then every so often, less
/// often while the cloud can't be reached; a sync handed over just now is
/// done right away. Never for a game that is running or being synced.
struct Backlog {
    next: Instant,
    pause: Duration,
    /// Handed-over syncs tried since the last regular look.
    tried: HashSet<String>,
    /// A pass runs on its own thread.
    running: Arc<AtomicBool>,
    /// How the last pass went: `true` when nothing failed.
    last: Arc<Mutex<Option<bool>>>,
}

impl Backlog {
    fn new() -> Backlog {
        Backlog {
            next: Instant::now() + BACKLOG_FIRST,
            pause: BACKLOG_EVERY,
            tried: HashSet::new(),
            running: Arc::new(AtomicBool::new(false)),
            last: Arc::new(Mutex::new(None)),
        }
    }

    /// Called every tick of the watcher.
    fn tick(&mut self, app: &AppHandle, cfg: &config::AppConfig, procs: &[String]) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        if let Some(ok) = self.last.lock().ok().and_then(|mut l| l.take()) {
            self.pause = if ok { BACKLOG_EVERY } else { (self.pause * 2).clamp(BACKLOG_EVERY, BACKLOG_MAX) };
            self.next = Instant::now() + self.pause;
        }
        // Not while it is played or synced: the wrapper's own syncs come first.
        let free = |key: &str| {
            let up = cfg.game(key).is_some_and(|g| watch_names(g).iter().any(|n| procs.contains(n)));
            !runner_active(key) && !up && !crate::busy::is_busy(key) && !sync::sync_running(key)
        };
        let owed: Vec<String> = sync::owed_keys().into_iter().filter(|k| free(k)).collect();
        let regular = Instant::now() >= self.next;
        if !regular && owed.iter().all(|k| self.tried.contains(k)) {
            return;
        }
        let pending: Vec<String> = if regular { sync::pending_keys().into_iter().filter(|k| free(k) && !owed.contains(k)).collect() } else { Vec::new() };
        if regular {
            self.tried.clear();
            self.next = Instant::now() + self.pause;
        }
        self.tried.extend(owed.iter().cloned());
        if owed.is_empty() && pending.is_empty() {
            return;
        }
        self.running.store(true, Ordering::SeqCst);
        let (app, cfg, running, last) = (app.clone(), cfg.clone(), self.running.clone(), self.last.clone());
        std::thread::spawn(move || {
            let ok = backlog_pass(&app, &cfg, &owed, &pending);
            if let Ok(mut l) = last.lock() {
                *l = Some(ok);
            }
            running.store(false, Ordering::SeqCst);
        });
    }
}

/// Finish the handed-over syncs of `owed` and upload what waits for
/// `pending`. `false` when something couldn't be done (the cloud wasn't
/// reached, mostly). Only what changed is told to the UI: a cloud that
/// stays away doesn't show a message every few minutes.
fn backlog_pass(app: &AppHandle, cfg: &config::AppConfig, owed: &[String], pending: &[String]) -> bool {
    let ctx = Ctx::from_config(cfg.clone());
    let cloud = match ctx.cloud() {
        Ok(c) => c,
        Err(e) if ludonimbus_core::cloud::is_not_configured(&e) => return true,
        Err(e) => {
            logger::log("watcher", format!("waiting uploads: no cloud ({e:#})"));
            return false;
        }
    };
    let name = |key: &str| cfg.game(key).map(|g| g.name.clone()).unwrap_or_else(|| key.to_string());
    let mut ok = true;
    for key in owed {
        let Some(_busy) = crate::busy::begin(key) else { continue };
        match sync::finish_owed(&ctx, key) {
            Ok(None) => {}
            Ok(Some(out)) => {
                logger::log("watcher", format!("{key}: post-exit (handed over) {out:?}"));
                if out.is_final() {
                    // What the wrapper would have done after it.
                    if let Some(g) = cfg.game(key) {
                        ludonimbus_core::achievements::after_exit(&ctx, g);
                    }
                    let _ = app.emit("sync-finished", SyncEvent { key: key.clone(), name: name(key), outcome: Some(out), error: None });
                } else {
                    ok = false;
                }
            }
            Err(e) => {
                logger::log("watcher", format!("{key}: post-exit (handed over) failed: {e:#}"));
                ok = false;
            }
        }
    }
    for key in pending {
        let Some(_busy) = crate::busy::begin(key) else { continue };
        match sync::flush_pending(cloud, key) {
            Ok(0) => {}
            Ok(n) => {
                logger::log("watcher", format!("{key}: uploaded {n} waiting versions"));
                let _ = app.emit("sync-finished", SyncEvent { key: key.clone(), name: name(key), outcome: None, error: None });
            }
            Err(e) => {
                logger::log("watcher", format!("{key}: uploading waiting versions failed: {e:#}"));
                ok = false;
            }
        }
    }
    ok
}

/// How often Steam games managed without the wrapper are looked at.
const UNWRAPPED_EVERY: Duration = Duration::from_secs(5 * 60);
/// The longest pause while the cloud can't be reached.
const UNWRAPPED_MAX: Duration = Duration::from_secs(30 * 60);
/// With many such games a look takes one request each: never more than
/// one a minute on average.
const UNWRAPPED_EACH: Duration = Duration::from_secs(60);

/// Steam games Ludonimbus manages that don't start through the wrapper
/// (docs/plan-v2.md, section 8.3): nothing syncs before they are played, so
/// every few minutes, while one isn't running or being synced, a newer
/// version in the cloud is pulled ([`steamcloud::poll`], which checks that
/// Steam Cloud is still off first). One listing per game when there is
/// nothing to do; less often with many such games and while the cloud
/// can't be reached, as in [`Backlog`]. After the game, the usual sync of a
/// game that ran outside the wrapper.
struct Unwrapped {
    next: Instant,
    pause: Duration,
    /// The pause the number of games asks for at least.
    floor: Duration,
    running: Arc<AtomicBool>,
    last: Arc<Mutex<Option<bool>>>,
}

impl Unwrapped {
    fn new() -> Unwrapped {
        Unwrapped {
            next: Instant::now() + BACKLOG_FIRST,
            pause: UNWRAPPED_EVERY,
            floor: UNWRAPPED_EVERY,
            running: Arc::new(AtomicBool::new(false)),
            last: Arc::new(Mutex::new(None)),
        }
    }

    /// Called every tick of the watcher.
    fn tick(&mut self, app: &AppHandle, cfg: &config::AppConfig, procs: &[String]) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        if let Some(ok) = self.last.lock().ok().and_then(|mut l| l.take()) {
            self.pause = if ok { UNWRAPPED_EVERY } else { (self.pause * 2).clamp(UNWRAPPED_EVERY, UNWRAPPED_MAX) };
            self.next = Instant::now() + self.pause.max(self.floor);
        }
        if Instant::now() < self.next {
            return;
        }
        self.next = Instant::now() + self.pause;
        let games: Vec<config::GameEntry> = cfg
            .games
            .iter()
            .filter(|g| g.auto_sync && g.store.as_ref().is_some_and(|s| s.mode == config::StoreMode::Managed && !s.wrapper))
            .filter(|g| {
                let up = watch_names(g).iter().any(|n| procs.contains(n));
                !runner_active(&g.key) && !up && !crate::busy::is_busy(&g.key) && !sync::sync_running(&g.key)
            })
            .cloned()
            .collect();
        if games.is_empty() {
            return;
        }
        self.floor = UNWRAPPED_EVERY.max(UNWRAPPED_EACH * games.len() as u32);
        self.next = Instant::now() + self.pause.max(self.floor);
        self.running.store(true, Ordering::SeqCst);
        let (app, cfg, running, last) = (app.clone(), cfg.clone(), self.running.clone(), self.last.clone());
        std::thread::spawn(move || {
            let ctx = Ctx::from_config(cfg);
            let mut ok = true;
            for g in &games {
                let Some(_busy) = crate::busy::begin(&g.key) else { continue };
                match steamcloud::poll(&ctx, g) {
                    Ok(Polled::Nothing) => {}
                    Ok(Polled::Offline) => ok = false,
                    Ok(Polled::Demoted(_)) => {
                        logger::log("watcher", format!("{}: Steam Cloud is on again; backup only from now on", g.key));
                        let _ = app.emit("library-changed", ());
                    }
                    Ok(Polled::Synced(out)) => {
                        logger::log("watcher", format!("{}: background sync (no wrapper) {out:?}", g.key));
                        if !out.is_final() {
                            ok = false;
                        }
                        let _ = app.emit("sync-finished", SyncEvent { key: g.key.clone(), name: g.name.clone(), outcome: Some(out), error: None });
                    }
                    Err(e) => {
                        logger::log("watcher", format!("{}: background sync (no wrapper) failed: {e:#}", g.key));
                        ok = false;
                    }
                }
            }
            if let Ok(mut l) = last.lock() {
                *l = Some(ok);
            }
            running.store(false, Ordering::SeqCst);
        });
    }
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
        let mut backlog = Backlog::new();
        let mut unwrapped = Unwrapped::new();
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            tick += 1;
            let Ok(cfg) = config::load() else { continue };
            let procs = process::process_names();
            let mut changed = false;

            for g in &cfg.games {
                let runner = catchup::wrapper(&g.key);
                let game_up = watch_names(g).iter().any(|n| procs.contains(n));
                let eff = spec::effective(g);
                let game_proc_up = eff.wait_for.iter().any(|n| procs.contains(&n.to_lowercase()));
                let game_proc_ended = if game_proc_up {
                    game_procs.insert(g.key.clone());
                    false
                } else {
                    game_procs.remove(&g.key)
                };
                if runner == Wrapper::Running || game_up {
                    // Started outside Steam, launcher still open, game closed.
                    if game_proc_ended && eff.upload_on_game_exit && g.auto_sync && runner != Wrapper::Running && running.get(&g.key) == Some(&false) {
                        background_sync(&app, &cfg, g, "game process exited while the launcher is still open");
                        changed = true;
                    }
                    let started = !running.contains_key(&g.key);
                    if started {
                        seen_at.insert(g.key.clone(), Utc::now());
                    }
                    let wrapped = running.entry(g.key.clone()).or_insert(false);
                    if runner == Wrapper::Running {
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
                if let Wrapper::Gone { started } = runner {
                    changed = true;
                    // Claimed by renaming it first, as `catchup::stale_runners`
                    // does: a launch wrapper finishing the same session (it
                    // does when it finds no tray app) leaves it alone then.
                    // When another program has it, that one finishes it.
                    let marker = paths::running_dir().join(format!("{}.json", g.key));
                    let claimed = marker.with_extension("claimed");
                    if std::fs::rename(&marker, &claimed).is_err() {
                        continue;
                    }
                    if let Some(start) = started {
                        record_playtime(&cfg, g, start, Via::Steam);
                    }
                    let _ = std::fs::remove_file(&claimed);
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

            backlog.tick(&app, &cfg, &procs);
            unwrapped.tick(&app, &cfg, &procs);

            if tick.is_multiple_of(5) {
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
