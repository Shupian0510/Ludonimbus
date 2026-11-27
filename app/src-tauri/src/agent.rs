//! Local HTTP API for the Ludonimbus Steam plugin (Millennium) and other local
//! tools. It runs while the app sits in the tray.
//!
//! The server listens on `127.0.0.1` at a random port. The port and a token
//! that changes on every launch are written to `%APPDATA%\Ludonimbus\agent.json`;
//! every request must send `Authorization: Bearer <token>`. Requests that come
//! from a web page (they always carry `Origin`) and requests whose `Host` is
//! not loopback (DNS rebinding) are refused.
//!
//! ```text
//! GET  /v1/ping
//! GET  /v1/steam/apps/{appid}[?account={id}&refresh=1]   cloud save status of a Steam shortcut, or of a game Steam installed (API 4)
//! GET  /v1/steam/playtime[?account={id}]                  playtime totals of every linked shortcut (API 2)
//! POST /v1/games/{key}/sync[?resolution=cloud|local]     start "sync now" in the background (resolution: API 3)
//! POST /v1/games/{key}/open                               show the game in the Ludonimbus window
//! GET  /v1/games/{key}/snapshots                          version history in the cloud (API 2)
//! POST /v1/games/{key}/restore/{id}                       restore a version in the background (API 2)
//! GET  /v1/games/{key}/playtime                           playtime over all devices (API 2)
//! GET  /v1/games/{key}/gamebackup                         game files missing? newest game backup, running job (API 2)
//! POST /v1/games/{key}/gamebackup/install                 show the game with the restore dialog open (API 2)
//! GET  /v1/games/{key}/newer                              newer saves in another version of the game (API 3)
//! POST /v1/games/{key}/newer/{id}/apply                   apply them in the background (API 3)
//! POST /v1/games/{key}/newer/{id}/dismiss                 don't offer them again (API 3)
//! ```
//! `crates/agent` serves the same protocol without a window (Steam Deck Game Mode).
//! Achievements (`/v1/steam/apps/{appid}/achievements`, `/v1/achievements/…`)
//! are handled in `achievements.rs`.

use crate::{achievements, busy, commands, show_main, watcher};
use chrono::{DateTime, Utc};
use ludonimbus_core::config::{self, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution, Status, StatusReport};
use ludonimbus_core::{logger, paths, playtime, process, state, steam, steamcloud, util};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Method, Request, Response, Server};

/// 2 (0.6.0): version history, restore, playtime.
/// 3 (0.7.0): conflict resolution on sync, newer saves in another version.
/// 4 (1.0.0): what a game still owes the cloud after playing (`backlog`);
/// games Steam installed (`store`); each version's `platform`.
pub const API_VERSION: u32 = 4;

/// A cached status younger than this is returned as is, even with `refresh=1`.
const FRESH_SECS: i64 = 60;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
    pub api_version: u32,
}

pub fn agent_file() -> PathBuf {
    paths::data_dir().join("agent.json")
}

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

/// Start the API server on a background thread.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let server = match Server::http("127.0.0.1:0") {
            Ok(s) => s,
            Err(e) => {
                log(format!("could not start the local API: {e}"));
                return;
            }
        };
        let Some(port) = server.server_addr().to_ip().map(|a| a.port()) else {
            log("local API has no TCP address");
            return;
        };
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let info = AgentInfo {
            port,
            token: token.clone(),
            pid: std::process::id(),
            version: ludonimbus_core::APP_VERSION.to_string(),
            api_version: API_VERSION,
        };
        // It holds the token: only this user may read it (Linux, macOS).
        match serde_json::to_vec_pretty(&info).map_err(anyhow::Error::from).and_then(|b| util::write_private(&agent_file(), &b)) {
            Ok(()) => log(format!("local API listening on 127.0.0.1:{port}")),
            Err(e) => {
                log(format!("could not write {}: {e:#}", agent_file().display()));
                return;
            }
        }
        for req in server.incoming_requests() {
            let app = app.clone();
            let token = token.clone();
            std::thread::spawn(move || handle(&app, req, &token, port));
        }
    });
}

/// Remove `agent.json` when this process wrote it (called on exit).
pub fn remove_agent_file() {
    let path = agent_file();
    let ours = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<AgentInfo>(&b).ok())
        .is_some_and(|i| i.pid == std::process::id());
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// HTTP plumbing

struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::new(500, format!("{e:#}"))
    }
}

type ApiResult = Result<Value, ApiError>;

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn handle(app: &AppHandle, req: Request, token: &str, port: u16) {
    let result = authorize(&req, token, port).and_then(|()| route(app, &req));
    let (status, body) = match result {
        Ok(v) => (200, v),
        Err(e) => (e.status, json!({ "error": e.message })),
    };
    let mut resp = Response::from_string(body.to_string()).with_status_code(status);
    if let Ok(h) = Header::from_bytes("Content-Type", "application/json; charset=utf-8") {
        resp.add_header(h);
    }
    let _ = req.respond(resp);
}

fn authorize(req: &Request, token: &str, port: u16) -> Result<(), ApiError> {
    if header(req, "Origin").is_some() {
        return Err(ApiError::new(403, "browser requests are not allowed"));
    }
    let host_ok = header(req, "Host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    if !host_ok {
        return Err(ApiError::new(403, "bad host"));
    }
    let expected = format!("Bearer {token}");
    if header(req, "Authorization") != Some(expected.as_str()) {
        return Err(ApiError::new(401, "missing or wrong token"));
    }
    Ok(())
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('=').or(Some((kv, ""))))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

fn route(app: &AppHandle, req: &Request) -> ApiResult {
    let (path, query) = req.url().split_once('?').unwrap_or((req.url(), ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (req.method(), parts.as_slice()) {
        (Method::Get, ["v1", "ping"]) => Ok(json!({
            "app": "ludonimbus",
            "version": ludonimbus_core::APP_VERSION,
            "apiVersion": API_VERSION,
            "lang": ludonimbus_core::i18n::current().code(),
        })),
        (Method::Get, ["v1", "steam", "apps", appid]) => {
            let appid: u32 = appid.parse().map_err(|_| ApiError::new(400, "bad appid"))?;
            let account = query_param(query, "account").filter(|a| !a.is_empty());
            let refresh = query_param(query, "refresh").is_some_and(|v| v == "1" || v == "true");
            steam_app_status(appid, account, refresh)
        }
        (Method::Get, ["v1", "steam", "playtime"]) => steam_playtimes(query_param(query, "account").filter(|a| !a.is_empty())),
        (Method::Post, ["v1", "games", key, "sync"]) => start_sync(app, key, query_param(query, "resolution")),
        (Method::Get, ["v1", "games", key, "newer"]) => {
            let ctx = Ctx::load()?;
            let g = find_by_key(&ctx.cfg, key)?;
            Ok(serde_json::to_value(ludonimbus_core::savelib::newer_version(&ctx, &g)?).map_err(anyhow::Error::from)?)
        }
        (Method::Post, ["v1", "games", key, "newer", id, "apply"]) => apply_newer(app, key, id),
        (Method::Post, ["v1", "games", key, "newer", id, "dismiss"]) => {
            ludonimbus_core::savelib::dismiss_newer(key, id)?;
            Ok(json!({ "ok": true }))
        }
        (Method::Get, ["v1", "games", key, "snapshots"]) => snapshots(key),
        (Method::Post, ["v1", "games", key, "restore", id]) => start_restore(app, key, id),
        (Method::Get, ["v1", "games", key, "playtime"]) => {
            let cfg = config::load()?;
            find_by_key(&cfg, key)?;
            Ok(serde_json::to_value(playtime::summary(&cfg, key)).map_err(anyhow::Error::from)?)
        }
        (Method::Get, ["v1", "games", key, "gamebackup"]) => Ok(crate::gamebackup::agent_status(key)?),
        (Method::Post, ["v1", "games", key, "gamebackup", "install"]) => Ok(crate::gamebackup::agent_install(app, key)?),
        (Method::Post, ["v1", "games", key, "open"]) => {
            find_by_key(&config::load()?, key)?;
            show_main(app);
            let _ = app.emit("open-game", key.to_string());
            Ok(json!({ "ok": true }))
        }
        (Method::Get, ["v1", "steam", "apps", appid, "achievements"]) => {
            let appid: u32 = appid.parse().map_err(|_| ApiError::new(400, "bad appid"))?;
            Ok(achievements::api_app(appid, query_param(query, "account").filter(|a| !a.is_empty()))?)
        }
        (Method::Get, ["v1", "games", key, "achievements", id, "icon"]) => Ok(achievements::api_icon(key, id)?),
        (Method::Get, ["v1", "games", key, "icon"]) => Ok(achievements::api_game_icon(key)?),
        (Method::Get, ["v1", "achievements", "events"]) => Ok(achievements::api_events(query_param(query, "after"))?),
        (Method::Post, ["v1", "achievements", "events", seq, "ack"]) => Ok(achievements::api_ack(seq, query_param(query, "shown"), query_param(query, "reason"))?),
        _ => Err(ApiError::new(404, "not found")),
    }
}

// ---------------------------------------------------------------------------
// Status

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudHead {
    device_name: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameStatus {
    managed: bool,
    key: String,
    name: String,
    auto_sync: bool,
    /// The Steam shortcut (or the launch options of a game Steam
    /// installed) goes through `ludonimbus-run`.
    steam_enabled: bool,
    /// `steam::Health` state, e.g. "ok", "disabled", "changed".
    health: String,
    /// A game Steam installed: who keeps its saves (API 4).
    store: Option<steamcloud::State>,
    /// What is left to do in the background after playing (API 4).
    backlog: Option<sync::Backlog>,
    /// What is happening right now: idle | syncing | preLaunch | playing | uploading.
    phase: &'static str,
    status: Option<Status>,
    message: Option<String>,
    last_sync_at: Option<DateTime<Utc>>,
    last_synced_device: Option<String>,
    cloud_head: Option<CloudHead>,
    pending: usize,
    conflict: bool,
    last_error: Option<String>,
    checked_at: Option<DateTime<Utc>>,
    /// Interface language picked in Ludonimbus ("zh-CN" or "en"), for the plugin's texts.
    lang: &'static str,
    /// Playtime over all devices (local data, refreshed by every sync).
    playtime: playtime::Summary,
    api_version: u32,
}

fn phase(g: &GameEntry) -> &'static str {
    if busy::is_busy(&g.key) {
        return "syncing";
    }
    if watcher::runner_active(&g.key) {
        return match watcher::runner_phase(&g.key).as_deref() {
            Some("preLaunch") => "preLaunch",
            Some("uploading") => "uploading",
            _ => "playing",
        };
    }
    if process::is_running(&watcher::watch_names(g)) {
        "playing"
    } else {
        "idle"
    }
}

fn find_by_key(cfg: &config::AppConfig, key: &str) -> Result<GameEntry, ApiError> {
    cfg.game(key)
        .cloned()
        .ok_or_else(|| ApiError::new(404, ludonimbus_core::tr!("Ludonimbus 里没有这个游戏（{key}）", "This game isn't in Ludonimbus ({key})")))
}

/// The game behind a Steam app id: a shortcut's, or the id of a game Steam
/// installed.
pub(crate) fn find_game(cfg: &config::AppConfig, appid: u32, account: Option<&str>) -> Option<GameEntry> {
    steam::game_of_app(cfg, appid, account).cloned()
}

fn steam_app_status(appid: u32, account: Option<&str>, refresh: bool) -> ApiResult {
    let cfg = config::load()?;
    let Some(g) = find_game(&cfg, appid, account) else {
        return Ok(json!({ "managed": false }));
    };
    let phase = phase(&g);
    let mut st = state::game(&g.key);
    let mut report: Option<StatusReport> = st.last_check.clone().and_then(|v| serde_json::from_value(v).ok());
    // Old, or taken before the latest sync (e.g. the Steam wrapper uploaded since).
    let stale = report.as_ref().is_none_or(|r| {
        (Utc::now() - r.checked_at).num_seconds() > FRESH_SECS || st.last_sync_at.is_some_and(|t| t > r.checked_at)
    });
    if refresh && stale && phase == "idle" {
        let ctx = Ctx::from_config(cfg.clone());
        let r = sync::check(&ctx, &g);
        commands::cache_check(&g.key, &r);
        report = Some(r);
        st = state::game(&g.key);
    }
    let health = match &g.store {
        Some(_) => steamcloud::wrapper_word(&cfg, &g).to_string(),
        None => serde_json::to_value(steam::health(&cfg, &g))
            .ok()
            .and_then(|v| v["state"].as_str().map(str::to_string))
            .unwrap_or_default(),
    };
    let body = GameStatus {
        managed: true,
        key: g.key.clone(),
        name: g.name.clone(),
        auto_sync: g.auto_sync,
        steam_enabled: g.steam.as_ref().is_some_and(|l| l.enabled) || g.store.as_ref().is_some_and(|l| l.wrapper),
        health,
        store: g.store.as_ref().map(|_| steamcloud::state(&g)),
        backlog: sync::backlog(&g.key),
        phase,
        status: report.as_ref().map(|r| r.status),
        message: report.as_ref().and_then(|r| r.message.clone()),
        last_sync_at: st.last_sync_at,
        last_synced_device: st.last_synced_device.clone(),
        cloud_head: report.as_ref().and_then(|r| r.head.as_ref()).map(|h| CloudHead {
            device_name: h.device_name.clone(),
            created_at: h.created_at,
        }),
        pending: st.pending.len(),
        conflict: st.conflict.is_some(),
        last_error: st.last_error.clone(),
        checked_at: report.as_ref().map(|r| r.checked_at),
        lang: ludonimbus_core::i18n::current().code(),
        playtime: playtime::summary(&cfg, &g.key),
        api_version: API_VERSION,
    };
    Ok(serde_json::to_value(body).map_err(anyhow::Error::from)?)
}

// ---------------------------------------------------------------------------
// Actions

/// Apply the newer saves of another version (the one `newer` offered).
fn apply_newer(app: &AppHandle, key: &str, id: &str) -> ApiResult {
    let ctx = Ctx::load()?;
    let g = find_by_key(&ctx.cfg, key)?;
    if let Some(reason) = busy_reason(&g) {
        return Err(ApiError::new(409, reason));
    }
    let n = ludonimbus_core::savelib::newer_version(&ctx, &g)?
        .filter(|n| n.snapshot.id == id)
        .ok_or_else(|| ApiError::new(404, "no such newer version"))?;
    let guard = busy::begin(key).ok_or_else(|| ApiError::new(409, ludonimbus_core::tr!("正在同步", "Syncing")))?;
    let app = app.clone();
    std::thread::spawn(move || {
        let _guard = guard;
        let result = ludonimbus_core::transfer::apply(&ctx, &n.key, &n.snapshot.id, &g.key, None);
        log(format!("{}: applied {} of {} from Steam {:?}", g.key, n.snapshot.id, n.key, result.as_ref().map_err(|e| e.to_string())));
        commands::cache_check(&g.key, &sync::check(&ctx, &g));
        let _ = app.emit(
            "sync-finished",
            watcher::SyncEvent { key: g.key.clone(), name: g.name.clone(), outcome: result.as_ref().ok().cloned(), error: result.err().map(|e| format!("{e:#}")) },
        );
    });
    Ok(json!({ "started": true }))
}

fn start_sync(app: &AppHandle, key: &str, resolution: Option<&str>) -> ApiResult {
    let res = match resolution {
        Some("cloud") => Resolution::UseCloud,
        Some("local") => Resolution::UseLocal,
        _ => Resolution::Auto,
    };
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    if watcher::runner_active(key) || process::is_running(&watcher::watch_names(&g)) {
        return Err(ApiError::new(409, ludonimbus_core::tr!("游戏正在运行，退出游戏后会自动同步", "The game is running; saves sync after you quit")));
    }
    let guard = busy::begin(key).ok_or_else(|| ApiError::new(409, ludonimbus_core::tr!("正在同步", "Syncing")))?;
    let app = app.clone();
    std::thread::spawn(move || {
        let _guard = guard;
        let ctx = Ctx::from_config(cfg);
        let result = sync::sync(&ctx, &g, res, Trigger::SyncNow);
        log(format!("{}: sync from Steam {:?}", g.key, result.as_ref().map_err(|e| e.to_string())));
        if let Err(e) = &result {
            let _ = state::update_game(&g.key, |s| s.last_error = Some(format!("{e:#}")));
        }
        commands::cache_check(&g.key, &sync::check(&ctx, &g));
        let _ = app.emit(
            "sync-finished",
            watcher::SyncEvent {
                key: g.key.clone(),
                name: g.name.clone(),
                outcome: result.as_ref().ok().cloned(),
                error: result.err().map(|e| format!("{e:#}")),
            },
        );
    });
    Ok(json!({ "started": true }))
}

/// Playtime over all devices of every game linked to a shortcut (of `account`
/// when given), in minutes: the plugin shows it as Steam's own playtime.
fn steam_playtimes(account: Option<&str>) -> ApiResult {
    let cfg = config::load()?;
    let apps: Vec<Value> = cfg
        .games
        .iter()
        .filter_map(|g| {
            let link = g.steam.as_ref()?;
            if account.is_some_and(|a| a != link.account) {
                return None;
            }
            let sum = playtime::summary(&cfg, &g.key);
            (sum.total_secs > 0).then(|| json!({ "appid": link.appid, "key": g.key, "minutes": sum.total_secs / 60 }))
        })
        .collect();
    Ok(json!({ "apps": apps }))
}

// ---------------------------------------------------------------------------
// Version history (the list in Steam)

fn busy_reason(g: &GameEntry) -> Option<String> {
    if watcher::runner_active(&g.key) || process::is_running(&watcher::watch_names(g)) {
        return Some(ludonimbus_core::tr!("游戏正在运行，请先退出游戏", "The game is running. Quit it first"));
    }
    if busy::is_busy(&g.key) {
        return Some(ludonimbus_core::tr!("正在同步", "Syncing"));
    }
    None
}

fn snapshots(key: &str) -> ApiResult {
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    let ctx = Ctx::from_config(cfg);
    let mut list = ctx.cloud()?.list_snapshots(key)?;
    let head = sync::mainline_head(&list).map(|h| h.id);
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let st = state::game(key);
    Ok(json!({
        "key": key,
        "name": g.name,
        "snapshots": list,
        "headId": head,
        "lastSyncedId": st.last_synced_id,
        "deviceId": ctx.cfg.device.id,
        "pending": st.pending.len(),
        "busy": busy_reason(&g),
    }))
}

fn start_restore(app: &AppHandle, key: &str, id: &str) -> ApiResult {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::new(400, "bad version id"));
    }
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    if let Some(reason) = busy_reason(&g) {
        return Err(ApiError::new(409, reason));
    }
    let guard = busy::begin(key).ok_or_else(|| ApiError::new(409, ludonimbus_core::tr!("正在同步", "Syncing")))?;
    let (app, id) = (app.clone(), id.to_string());
    std::thread::spawn(move || {
        let _guard = guard;
        let ctx = Ctx::from_config(cfg);
        let result = sync::restore(&ctx, &g, &id);
        log(format!("{}: restore {id} from Steam {:?}", g.key, result.as_ref().map_err(|e| e.to_string())));
        match &result {
            Ok(_) => {
                let _ = state::update_game(&g.key, |s| s.last_error = None);
            }
            Err(e) => {
                let _ = state::update_game(&g.key, |s| s.last_error = Some(format!("{e:#}")));
            }
        }
        commands::cache_check(&g.key, &sync::check(&ctx, &g));
        let _ = app.emit(
            "sync-finished",
            watcher::SyncEvent {
                key: g.key.clone(),
                name: g.name.clone(),
                outcome: result.as_ref().ok().cloned(),
                error: result.err().map(|e| format!("{e:#}")),
            },
        );
    });
    Ok(json!({ "started": true }))
}
