//! Achievements in the tray app: watches linked sources while games run,
//! queues new unlocks for the Steam plugin (which shows them with Steam's own
//! achievement popup), falls back to a small topmost window when Steam can't,
//! and serves the app's achievements UI.
//!
//! Local API (routed from `agent.rs`):
//! ```text
//! GET  /v1/steam/apps/{appid}/achievements[?account=]   game page: progress and list
//! GET  /v1/games/{key}/achievements/{id}/icon            local icon as a data URL
//! GET  /v1/games/{key}/icon                              the game's icon, for achievements without one
//! GET  /v1/achievements/events?after={seq}               new unlocks to show
//! POST /v1/achievements/events/{seq}/ack?shown=steam|failed
//! ```
//! See `docs/plan-achievements.md`.

use crate::commands::{blocking, CmdResult};
use crate::watcher;
use anyhow::{anyhow, Result};
use chrono::Utc;
use ludonimbus_core::achievements::{self as ach, detect, retro, Binding, GameView, NewUnlock, SourceKind};
use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::sync::Ctx;
use ludonimbus_core::{logger, process};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

fn log(msg: impl AsRef<str>) {
    logger::log("achievements", msg);
}

const TICK: Duration = Duration::from_secs(2);
const RA_EVERY: Duration = Duration::from_secs(30);
/// How long the Steam plugin has to show a popup before the fallback window does.
const ACK_WAIT: Duration = Duration::from_millis(3500);
const KEEP_EVENTS: usize = 50;
pub(crate) const TOAST_LABEL: &str = "achievement-toast";

// ---------------------------------------------------------------------------
// Event queue for the Steam plugin

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    seq: u64,
    key: String,
    game_name: String,
    /// The Steam shortcut's app id on this PC (Steam needs it for its popup).
    appid: Option<u32>,
    id: String,
    name: String,
    description: String,
    /// `https://…` or a `data:` URL.
    icon: Option<String>,
    unlocked_at: chrono::DateTime<Utc>,
    hardcore: bool,
    points: Option<u32>,
    grade: Option<ach::Grade>,
    test: bool,
    #[serde(skip)]
    shown: Option<String>,
}

struct Queue {
    seq: u64,
    events: VecDeque<Event>,
    plugin_seen: Option<Instant>,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { seq: 0, events: VecDeque::new(), plugin_seen: None });

/// Numbers start from the launch time, so they keep growing across restarts
/// of the app: a plugin still holding the last run's number gets this run's
/// unlocks, and a plugin that synced at 0 isn't stuck on `after=0`, which
/// `api_events` reads as "just (re)loaded, start from now".
fn seed(q: &mut Queue) {
    if q.seq == 0 {
        q.seq = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0).max(1);
    }
}

/// The achievement's icon, else the game's.
fn icon_for(key: &str, a: &ach::Achievement) -> Option<String> {
    let own = match a.icon.as_deref() {
        Some(i) if ach::is_web(i) => return Some(i.to_string()),
        Some(p) => ach::icon_data_url(std::path::Path::new(p)).ok(),
        None => None,
    };
    own.or_else(|| game_icon_url(key))
}

fn game_icon_url(key: &str) -> Option<String> {
    ach::game_icon_path(key).and_then(|p| ach::icon_data_url(&p).ok())
}

fn push_event(app: &AppHandle, g: &GameEntry, u: &NewUnlock, test: bool) {
    let ev = enqueue(g, u, test);
    log(format!("{}: unlocked {} ({})", g.key, ev.id, ev.name));
    let _ = app.emit("achievement-unlocked", &ev);
    // The fallback window, unless the plugin shows it in Steam.
    let app = app.clone();
    std::thread::spawn(move || {
        if !needs_fallback(ev.seq) || !ach::settings().fallback_popup {
            return;
        }
        log(format!("{}: showing the fallback popup for {}", ev.key, ev.id));
        show_fallback(&app, &ev);
    });
}

/// Wait for the plugin to show the popup in Steam; true when it didn't.
fn needs_fallback(seq: u64) -> bool {
    let plugin_alive = QUEUE.lock().unwrap().plugin_seen.is_some_and(|t| t.elapsed() < Duration::from_secs(10));
    if !plugin_alive {
        return true;
    }
    let start = Instant::now();
    while start.elapsed() < ACK_WAIT {
        std::thread::sleep(Duration::from_millis(250));
        if shown(seq).as_deref() == Some("steam") {
            return false;
        }
    }
    true
}

fn enqueue(g: &GameEntry, u: &NewUnlock, test: bool) -> Event {
    let mut q = QUEUE.lock().unwrap();
    seed(&mut q);
    q.seq += 1;
    let ev = Event {
        seq: q.seq,
        key: g.key.clone(),
        game_name: g.name.clone(),
        appid: g.steam.as_ref().map(|l| l.appid).filter(|a| *a != 0),
        id: u.achievement.id.clone(),
        name: u.achievement.name.clone(),
        description: u.achievement.description.clone(),
        icon: icon_for(&g.key, &u.achievement),
        unlocked_at: u.unlocked_at,
        hardcore: u.hardcore,
        points: u.achievement.points,
        grade: u.achievement.grade,
        test,
        shown: None,
    };
    q.events.push_back(ev.clone());
    while q.events.len() > KEEP_EVENTS {
        q.events.pop_front();
    }
    ev
}

fn shown(seq: u64) -> Option<String> {
    QUEUE.lock().unwrap().events.iter().find(|e| e.seq == seq).and_then(|e| e.shown.clone())
}

pub fn api_events(query_after: Option<&str>) -> Result<Value> {
    let after: u64 = query_after.and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut q = QUEUE.lock().unwrap();
    seed(&mut q);
    q.plugin_seen = Some(Instant::now());
    // A plugin that just (re)loaded starts from now: old unlocks aren't replayed.
    let events: Vec<&Event> = if after == 0 { Vec::new() } else { q.events.iter().filter(|e| e.seq > after).collect() };
    Ok(json!({ "seq": q.seq, "events": events, "lang": ludonimbus_core::i18n::current().code() }))
}

/// `%XX` and `+` decoding of a query value.
fn unescape(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn api_ack(seq: &str, shown: Option<&str>, reason: Option<&str>) -> Result<Value> {
    let seq: u64 = seq.parse().map_err(|_| anyhow!("bad seq"))?;
    let shown = shown.unwrap_or("steam").to_string();
    let reason = reason.map(unescape).filter(|r| !r.is_empty());
    let mut q = QUEUE.lock().unwrap();
    if let Some(e) = q.events.iter_mut().find(|e| e.seq == seq) {
        let how = if shown == "steam" { "shown in Steam" } else { "not shown in Steam" };
        log(format!("{}: popup for {} {how}{}", e.key, e.id, reason.map(|r| format!(" ({r})")).unwrap_or_default()));
        e.shown = Some(shown);
    }
    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Fallback popup: a small transparent topmost window (visible over windowed
// and borderless games, not over exclusive fullscreen).

struct Toasts {
    queue: VecDeque<Value>,
    /// The window is open and asks for the next popup when one ends.
    open: bool,
}

static TOASTS: Mutex<Toasts> = Mutex::new(Toasts { queue: VecDeque::new(), open: false });

fn show_fallback(app: &AppHandle, ev: &Event) {
    // Held until the window exists: its first question waits for it.
    let mut toasts = TOASTS.lock().unwrap();
    toasts.queue.push_back(json!({
        "name": ev.name,
        "description": ev.description,
        "icon": ev.icon,
        "game": ev.game_name,
        "hardcore": ev.hardcore,
        "points": ev.points,
        "grade": ev.grade,
        "test": ev.test,
        "lang": ludonimbus_core::i18n::current().code(),
    }));
    if toasts.open && app.get_webview_window(TOAST_LABEL).is_some() {
        return; // the open window picks it up when the current toast ends
    }
    // The last window, told it was done, may not be gone yet.
    let closing = Instant::now();
    while app.get_webview_window(TOAST_LABEL).is_some() && closing.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let (w, h, margin) = (380.0, 104.0, 24.0);
    let (mut x, mut y) = (margin, margin);
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor();
        x = (m.position().x as f64 + m.size().width as f64) / scale - w - margin;
        y = m.position().y as f64 / scale + margin;
    }
    let built = WebviewWindowBuilder::new(app, TOAST_LABEL, WebviewUrl::App("index.html?toast".into()))
        .title("Ludonimbus")
        .inner_size(w, h)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .shadow(false)
        .build();
    toasts.open = built.is_ok();
    match built {
        Ok(win) => {
            let _ = win.set_ignore_cursor_events(true);
        }
        Err(e) => log(format!("could not open the popup window: {e}")),
    }
}

/// The toast window asks for the next popup; `None` closes it. Not on the
/// main thread, which [`show_fallback`] needs while it holds the queue.
#[tauri::command]
pub async fn achievement_toast_next(app: AppHandle) -> CmdResult<Option<Value>> {
    blocking(move || {
        let mut toasts = TOASTS.lock().unwrap();
        let next = toasts.queue.pop_front();
        if next.is_none() {
            toasts.open = false;
            if let Some(w) = app.get_webview_window(TOAST_LABEL) {
                let _ = w.close();
            }
        }
        Ok(next)
    })
    .await
}

// ---------------------------------------------------------------------------
// Watching sources

#[derive(Default)]
struct Watch {
    running: bool,
    fingerprint: u64,
    last_ra: Option<Instant>,
    last_error: Option<Instant>,
}

fn fingerprint(g: &GameEntry, b: &Binding) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in ach::watch_files(g, b) {
        f.hash(&mut h);
        if let Ok(m) = std::fs::metadata(&f) {
            m.len().hash(&mut h);
            m.modified().ok().hash(&mut h);
        }
    }
    h.finish()
}

fn is_running(g: &GameEntry) -> bool {
    watcher::runner_active(&g.key) || process::is_running(&watcher::watch_names(g))
}

/// Read a game's source; announce new unlocks when asked to.
fn check(app: &AppHandle, cfg: &AppConfig, g: &GameEntry, announce: bool) -> Result<usize> {
    let new = ach::refresh(cfg, g, announce)?;
    if !new.is_empty() {
        let popup = ach::settings().popup_for(&g.key);
        for u in &new {
            if popup {
                push_event(app, g, u, false);
            } else {
                log(format!("{}: unlocked {} (no popup for this game)", g.key, u.achievement.id));
            }
        }
    }
    let _ = app.emit("achievements-changed", g.key.clone());
    Ok(new.len())
}

fn ra_has_news(st: &ach::Settings, b: &Binding, key: &str) -> Result<bool> {
    let recent = retro::recent_unlocks(st, 15)?;
    let mine = ach::load_db(key).mine;
    Ok(recent.iter().any(|(game, id)| game == b.source_id.trim() && !mine.contains_key(id)))
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        // Catch up quietly: what happened while the app wasn't running is history.
        std::thread::sleep(Duration::from_secs(5));
        if let Ok(cfg) = config::load() {
            for key in ach::link_obvious(&cfg) {
                log(format!("{key}: achievement source linked automatically"));
            }
            let ctx = Ctx::from_config(cfg.clone());
            for key in ach::settings().games.keys() {
                if let Some(g) = cfg.game(key) {
                    if let Err(e) = check(&app, &cfg, g, false) {
                        log(format!("{key}: {e:#}"));
                    }
                    ach::sync_cloud(&ctx, key);
                }
            }
        }
        let mut watches: HashMap<String, Watch> = HashMap::new();
        loop {
            std::thread::sleep(TICK);
            let Ok(cfg) = config::load() else { continue };
            let st = ach::settings();
            for (key, b) in &st.games {
                let Some(g) = cfg.game(key) else { continue };
                let running = is_running(g);
                let w = watches.entry(key.clone()).or_default();
                let was = w.running;
                w.running = running;
                let result = if running {
                    match b.source {
                        SourceKind::Retroachievements => {
                            if w.last_ra.is_some_and(|t| t.elapsed() < RA_EVERY) {
                                Ok(0)
                            } else {
                                w.last_ra = Some(Instant::now());
                                match ra_has_news(&st, b, key) {
                                    Ok(true) => check(&app, &cfg, g, true),
                                    Ok(false) => Ok(0),
                                    Err(e) => Err(e),
                                }
                            }
                        }
                        _ => {
                            let fp = fingerprint(g, b);
                            if fp != w.fingerprint || !was {
                                w.fingerprint = fp;
                                check(&app, &cfg, g, true)
                            } else {
                                Ok(0)
                            }
                        }
                    }
                } else if was {
                    // The game just quit: last read, then the cloud records.
                    w.last_ra = None;
                    let r = check(&app, &cfg, g, true);
                    let (app2, cfg2, key2) = (app.clone(), cfg.clone(), key.clone());
                    std::thread::spawn(move || {
                        ach::sync_cloud(&Ctx::from_config(cfg2), &key2);
                        let _ = app2.emit("achievements-changed", key2);
                    });
                    r
                } else {
                    Ok(0)
                };
                if let Err(e) = result {
                    if w.last_error.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                        log(format!("{key}: {e:#}"));
                        w.last_error = Some(Instant::now());
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Local API

pub fn api_app(appid: u32, account: Option<&str>) -> Result<Value> {
    let cfg = config::load()?;
    // A game Steam installed has Steam's own achievements there.
    let Some(g) = crate::agent::find_game(&cfg, appid, account).filter(|g| g.store.is_none()) else {
        return Ok(json!({ "managed": false }));
    };
    let mut v = serde_json::to_value(ach::view(&g.key))?;
    v["managed"] = json!(true);
    v["name"] = json!(g.name);
    v["lang"] = json!(ludonimbus_core::i18n::current().code());
    Ok(v)
}

pub fn api_icon(key: &str, id: &str) -> Result<Value> {
    let path = ach::icon_path(key, id).ok_or_else(|| anyhow!("no icon"))?;
    Ok(json!({ "dataUrl": ach::icon_data_url(&path)? }))
}

pub fn api_game_icon(key: &str) -> Result<Value> {
    let path = ach::game_icon_path(key).ok_or_else(|| anyhow!("no icon"))?;
    Ok(json!({ "dataUrl": ach::icon_data_url(&path)? }))
}

// ---------------------------------------------------------------------------
// Commands for the app UI

fn game(key: &str) -> Result<(AppConfig, GameEntry)> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    Ok((cfg, g))
}

#[tauri::command]
pub async fn achievements_view(key: String) -> CmdResult<GameView> {
    blocking(move || Ok(ach::view(&key))).await
}

#[tauri::command]
pub async fn achievements_candidates(key: String) -> CmdResult<Vec<detect::Candidate>> {
    blocking(move || Ok(detect::candidates(&game(&key)?.1))).await
}

fn link_and_sync(app: &AppHandle, key: &str, binding: Binding) -> Result<GameView> {
    let (cfg, g) = game(key)?;
    ach::link(&cfg, &g, binding)?;
    let (app, key2) = (app.clone(), key.to_string());
    std::thread::spawn(move || {
        if let Ok(cfg) = config::load() {
            ach::sync_cloud(&Ctx::from_config(cfg), &key2);
        }
        let _ = app.emit("achievements-changed", key2);
    });
    Ok(ach::view(key))
}

#[tauri::command]
pub async fn achievements_link(app: AppHandle, key: String, binding: Binding) -> CmdResult<GameView> {
    blocking(move || link_and_sync(&app, &key, binding)).await
}

#[tauri::command]
pub async fn achievements_link_file(app: AppHandle, key: String, path: PathBuf) -> CmdResult<GameView> {
    blocking(move || {
        let b = detect::from_path(&path).ok_or_else(|| {
            anyhow!(ludonimbus_core::tr!(
                "认不出这个文件。请选择 TROPUSR.DAT、TROPCONF.SFM、trophies.log 或 shadPS4 的奖杯 XML",
                "Can't tell what this file is. Choose a TROPUSR.DAT, TROPCONF.SFM, trophies.log or shadPS4 trophy XML"
            ))
        })?;
        link_and_sync(&app, &key, b)
    })
    .await
}

#[tauri::command]
pub async fn achievements_pick_file() -> CmdResult<Option<PathBuf>> {
    blocking(|| {
        Ok(rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择奖杯文件", "Choose a trophy file"))
            .add_filter(ludonimbus_core::tr!("奖杯文件", "Trophy files"), &["DAT", "SFM", "log", "xml", "XML"])
            .pick_file())
    })
    .await
}

#[tauri::command]
pub async fn achievements_unlink(key: String) -> CmdResult<()> {
    blocking(move || ach::unlink(&key)).await
}

#[tauri::command]
pub async fn achievements_refresh(app: AppHandle, key: String) -> CmdResult<GameView> {
    blocking(move || {
        let (cfg, g) = game(&key)?;
        let result = check(&app, &cfg, &g, false);
        ach::sync_cloud(&Ctx::from_config(cfg), &key);
        result?;
        Ok(ach::view(&key))
    })
    .await
}

#[tauri::command]
pub async fn achievements_icon(key: String, id: String) -> CmdResult<Option<String>> {
    blocking(move || Ok(ach::icon_path(&key, &id).and_then(|p| ach::icon_data_url(&p).ok()))).await
}

/// The game's icon, shown for achievements that have none.
#[tauri::command]
pub async fn achievements_game_icon(key: String) -> CmdResult<Option<String>> {
    blocking(move || Ok(game_icon_url(&key))).await
}

#[tauri::command]
pub async fn achievements_set_popup(key: String, popup: Option<bool>) -> CmdResult<()> {
    blocking(move || {
        ach::update_settings(|s| {
            if let Some(b) = s.games.get_mut(&key) {
                b.popup = popup;
            }
        })
    })
    .await
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementSettings {
    ra_user: Option<String>,
    ra_connected: bool,
    steam_popup: bool,
    fallback_popup: bool,
    /// Where the PS4 trophy key comes from (shadPS4 or these settings), if anywhere.
    ps4_key_source: Option<ach::trp::KeySource>,
}

#[tauri::command]
pub async fn achievements_settings() -> CmdResult<AchievementSettings> {
    blocking(|| {
        let s = ach::settings();
        Ok(AchievementSettings {
            ra_connected: s.ra_user.is_some() && retro::api_key().ok().flatten().is_some(),
            ra_user: s.ra_user,
            steam_popup: s.steam_popup,
            fallback_popup: s.fallback_popup,
            ps4_key_source: ach::trp::trophy_key().map(|(_, from)| from),
        })
    })
    .await
}

/// Set (or clear, with an empty text) the PS4 trophy key kept in this PC's settings.
#[tauri::command]
pub async fn achievements_set_ps4_key(key: String) -> CmdResult<()> {
    blocking(move || {
        let key = key.trim().to_string();
        if !key.is_empty() && ach::trp::parse_hex_key(&key).is_none() {
            return Err(anyhow!(ludonimbus_core::tr!("密钥应该是 32 个十六进制字符", "The key should be 32 hexadecimal characters")));
        }
        ach::update_settings(|s| s.ps4_trophy_key = (!key.is_empty()).then(|| key.to_ascii_uppercase()))?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn achievements_save_settings(steam_popup: bool, fallback_popup: bool) -> CmdResult<()> {
    blocking(move || {
        ach::update_settings(|s| {
            s.steam_popup = steam_popup;
            s.fallback_popup = fallback_popup;
        })
    })
    .await
}

#[tauri::command]
pub async fn ra_connect(user: String, key: String) -> CmdResult<retro::Profile> {
    blocking(move || retro::connect(&user, &key)).await
}

#[tauri::command]
pub async fn ra_disconnect() -> CmdResult<()> {
    blocking(retro::disconnect).await
}

#[tauri::command]
pub async fn ra_recent_games() -> CmdResult<Vec<retro::RaGame>> {
    blocking(|| retro::recently_played(&ach::settings())).await
}

/// "Test popup": a made-up unlock through the same path as real ones,
/// optionally after a delay (time to switch back to a game).
#[tauri::command]
pub async fn achievements_test_popup(app: AppHandle, key: Option<String>, delay_secs: Option<u64>) -> CmdResult<()> {
    let delay = delay_secs.unwrap_or(0).min(60);
    if delay > 0 {
        let app2 = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(delay));
            if let Err(e) = test_popup(&app2, key) {
                log(format!("test popup failed: {e:#}"));
            }
        });
        return Ok(());
    }
    blocking(move || test_popup(&app, key)).await
}

fn test_popup(app: &AppHandle, key: Option<String>) -> Result<()> {
    let cfg = config::load()?;
    // A game linked to Steam (the running one first), so Steam can show its name and icon.
    let in_steam = |g: &&GameEntry| g.steam.as_ref().is_some_and(|l| l.appid != 0);
    let g = key
        .and_then(|k| cfg.game(&k).cloned())
        .or_else(|| cfg.games.iter().filter(in_steam).find(|g| is_running(g)).cloned())
        .or_else(|| cfg.games.iter().find(in_steam).cloned())
        .unwrap_or_else(|| GameEntry { key: "test".into(), name: "Ludonimbus".into(), ..Default::default() });
    let icon = ach::load_db(&g.key)
        .schema
        .and_then(|s| s.achievements.into_iter().find(|a| a.icon.is_some()))
        .and_then(|a| a.icon);
    let unlock = NewUnlock {
        key: g.key.clone(),
        achievement: ach::Achievement {
            id: "ludonimbus-test".into(),
            name: ludonimbus_core::tr!("Ludonimbus 测试成就", "Ludonimbus test achievement"),
            description: ludonimbus_core::tr!("能看到这个弹窗，说明成就提醒可以正常显示", "If you can see this, achievement popups work"),
            icon,
            hidden: false,
            points: None,
            grade: None,
        },
        unlocked_at: Utc::now(),
        hardcore: false,
    };
    push_event(app, &g, &unlock, true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlock(id: &str) -> NewUnlock {
        NewUnlock {
            key: "g".into(),
            achievement: ach::placeholder(id),
            unlocked_at: Utc::now(),
            hardcore: false,
        }
    }

    #[test]
    fn plugin_event_queue() {
        // Logs go to a scratch data folder, never the real one.
        std::env::set_var("LUDONIMBUS_HOME", std::env::temp_dir().join(format!("ludonimbus-app-test-{}", std::process::id())));
        let g = GameEntry {
            key: "g".into(),
            name: "Game".into(),
            steam: Some(config::SteamLink { appid: 3_000_000_001, ..Default::default() }),
            ..Default::default()
        };
        // Nothing has polled yet: the fallback window is used right away.
        let first = enqueue(&g, &unlock("1"), false);
        assert!(needs_fallback(first.seq));
        // Not 1: numbers continue across restarts of the app.
        assert!(first.seq > 1_000_000_000_000);
        assert_eq!(first.appid, Some(3_000_000_001));

        // A plugin that (re)loads gets the current number, not old events.
        let v = api_events(None).unwrap();
        assert_eq!(v["events"].as_array().unwrap().len(), 0);
        let seq = v["seq"].as_u64().unwrap();

        let second = enqueue(&g, &unlock("2"), false);
        let v = api_events(Some(&seq.to_string())).unwrap();
        let events = v["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "2");
        assert_eq!(events[0]["appid"], 3_000_000_001u64);

        // Shown in Steam: no fallback. Failed: fallback.
        api_ack(&second.seq.to_string(), Some("steam"), Some("overlay")).unwrap();
        assert!(!needs_fallback(second.seq));
        let third = enqueue(&g, &unlock("3"), false);
        api_events(Some(&second.seq.to_string())).unwrap();
        api_ack(&third.seq.to_string(), Some("failed"), Some("NotificationStore%20not%20found")).unwrap();
        assert_eq!(unescape("a%20b+c%E4%B8%AD%2"), "a b c中%2");
        assert!(needs_fallback(third.seq));
    }
}
