//! `ludonimbus-agent`: Ludonimbus without a window, for Steam Deck Game Mode
//! and other screens without a desktop (docs/plan-platforms.md §2.1). Run by
//! `systemd --user` (see `scripts/install-linux.sh`).
//!
//! - The local API for the Decky plugin: the same protocol as the app's
//!   (`app/src-tauri/src/agent.rs`), API 4: port and token in `agent.json`,
//!   `Authorization: Bearer <token>`, no browser requests.
//! - The tray app's background work: finishing the sessions of launch
//!   wrappers that were terminated, uploading screenshots.
//!
//! It steps aside while the desktop app runs: the app writes its own
//! `agent.json` and its watcher does the same work.
//!
//! ```text
//! GET  /v1/ping
//! GET  /v1/steam/apps/{appid}[?account={id}&refresh=1]   cloud save status of a Steam shortcut, or of a game Steam installed (API 4)
//! GET  /v1/steam/playtime[?account={id}]                  playtime totals of every linked shortcut
//! POST /v1/games/{key}/sync[?resolution=cloud|local]      sync now (in the background)
//! GET  /v1/games/{key}/snapshots                          version history in the cloud
//! POST /v1/games/{key}/restore/{id}                       restore a version (in the background)
//! GET  /v1/games/{key}/playtime                           playtime over all devices
//! GET  /v1/games/{key}/newer                              newer saves in another version of the game (API 3)
//! POST /v1/games/{key}/newer/{id}/apply                   apply them (in the background, API 3)
//! POST /v1/games/{key}/newer/{id}/dismiss                 don't offer them again (API 3)
//! ```

use anyhow::Result;
use chrono::{DateTime, Utc};
use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution, Status, StatusReport};
use ludonimbus_core::{catchup, logger, paths, playtime, process, savelib, state, steam, steamcloud, transfer, util};
use rand::RngCore;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

const API_VERSION: u32 = 4;
const FRESH_SECS: i64 = 60;

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

fn agent_file() -> std::path::PathBuf {
    paths::data_dir().join("agent.json")
}

/// The pid in `agent.json`, when that process is alive.
fn live_owner() -> Option<u32> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(agent_file()).ok()?).ok()?;
    let pid = v["pid"].as_u64()? as u32;
    process::pid_alive(pid).then_some(pid)
}

// ---------------------------------------------------------------------------
// Games being synced by this process

static BUSY: Mutex<Option<HashSet<String>>> = Mutex::new(None);

struct Busy(String);

impl Busy {
    fn begin(key: &str) -> Option<Busy> {
        let mut b = BUSY.lock().unwrap();
        b.get_or_insert_with(HashSet::new).insert(key.to_string()).then(|| Busy(key.to_string()))
    }
    fn is_busy(key: &str) -> bool {
        BUSY.lock().unwrap().as_ref().is_some_and(|b| b.contains(key))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(b) = BUSY.lock().unwrap().as_mut() {
            b.remove(&self.0);
        }
    }
}

// ---------------------------------------------------------------------------

fn main() {
    let once = std::env::args().any(|a| a == "--once");
    log(format!("ludonimbus-agent {} starting", ludonimbus_core::APP_VERSION));
    #[cfg(unix)]
    if let Err(e) = paths::install_runner() {
        log(format!("could not install the launch wrapper: {e}"));
    }
    if once {
        if let Ok(cfg) = config::load() {
            catchup::stale_runners(&cfg, None, &|m| log(m));
        }
        return;
    }
    let server = match Server::http("127.0.0.1:0") {
        Ok(s) => s,
        Err(e) => {
            log(format!("could not start the local API: {e}"));
            std::process::exit(1);
        }
    };
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let info = json!({
        "port": port,
        "token": token,
        "pid": std::process::id(),
        "version": ludonimbus_core::APP_VERSION,
        "apiVersion": API_VERSION,
        "headless": true,
    });
    let token2 = token.clone();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let token = token2.clone();
            std::thread::spawn(move || handle(req, &token, port));
        }
    });
    log(format!("local API listening on 127.0.0.1:{port}"));

    let me = std::process::id();
    let mut last_shots = Instant::now() - Duration::from_secs(25 * 60);
    let mut nx: Option<ludonimbus_core::nxlan::Server> = None;
    loop {
        // The desktop app runs: it does all of this itself.
        let owner = live_owner();
        let app_runs = owner.is_some_and(|p| p != me);
        if owner.is_none() {
            // It holds the token: only this user may read it.
            match serde_json::to_vec_pretty(&info).map_err(anyhow::Error::from).and_then(|b| util::write_private(&agent_file(), &b)) {
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

// ---------------------------------------------------------------------------
// HTTP plumbing (as in the app's agent.rs)

struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        ApiError { status, message: message.into() }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::new(500, format!("{e:#}"))
    }
}

type ApiResult = std::result::Result<Value, ApiError>;

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn handle(req: Request, token: &str, port: u16) {
    let result = authorize(&req, token, port).and_then(|()| route(&req));
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

fn authorize(req: &Request, token: &str, port: u16) -> std::result::Result<(), ApiError> {
    if header(req, "Origin").is_some() {
        return Err(ApiError::new(403, "browser requests are not allowed"));
    }
    let host_ok = header(req, "Host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    if !host_ok {
        return Err(ApiError::new(403, "bad host"));
    }
    if header(req, "Authorization") != Some(format!("Bearer {token}").as_str()) {
        return Err(ApiError::new(401, "missing or wrong token"));
    }
    Ok(())
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').filter_map(|kv| kv.split_once('=').or(Some((kv, "")))).find(|(k, _)| *k == name).map(|(_, v)| v)
}

fn route(req: &Request) -> ApiResult {
    let (path, query) = req.url().split_once('?').unwrap_or((req.url(), ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (req.method(), parts.as_slice()) {
        (Method::Get, ["v1", "ping"]) => Ok(json!({
            "app": "ludonimbus",
            "version": ludonimbus_core::APP_VERSION,
            "apiVersion": API_VERSION,
            "lang": ludonimbus_core::i18n::current().code(),
            "headless": true,
        })),
        (Method::Get, ["v1", "steam", "apps", appid]) => {
            let appid: u32 = appid.parse().map_err(|_| ApiError::new(400, "bad appid"))?;
            let account = query_param(query, "account").filter(|a| !a.is_empty());
            let refresh = query_param(query, "refresh").is_some_and(|v| v == "1" || v == "true");
            steam_app_status(appid, account, refresh)
        }
        (Method::Get, ["v1", "steam", "playtime"]) => steam_playtimes(query_param(query, "account").filter(|a| !a.is_empty())),
        (Method::Post, ["v1", "games", key, "sync"]) => start_sync(key, query_param(query, "resolution")),
        (Method::Get, ["v1", "games", key, "snapshots"]) => snapshots(key),
        (Method::Post, ["v1", "games", key, "restore", id]) => start_restore(key, id),
        (Method::Get, ["v1", "games", key, "playtime"]) => {
            let cfg = config::load()?;
            find_by_key(&cfg, key)?;
            Ok(serde_json::to_value(playtime::summary(&cfg, key)).map_err(anyhow::Error::from)?)
        }
        (Method::Get, ["v1", "games", key, "newer"]) => newer(key),
        (Method::Post, ["v1", "games", key, "newer", id, "apply"]) => apply_newer(key, id),
        (Method::Post, ["v1", "games", key, "newer", id, "dismiss"]) => {
            savelib::dismiss_newer(key, id)?;
            Ok(json!({ "ok": true }))
        }
        _ => Err(ApiError::new(404, "not found")),
    }
}

// ---------------------------------------------------------------------------
// Status

fn find_by_key(cfg: &AppConfig, key: &str) -> std::result::Result<GameEntry, ApiError> {
    cfg.game(key)
        .cloned()
        .ok_or_else(|| ApiError::new(404, ludonimbus_core::tr!("Ludonimbus 里没有这个游戏（{key}）", "This game isn't in Ludonimbus ({key})")))
}

fn find_game(cfg: &AppConfig, appid: u32, account: Option<&str>) -> Option<GameEntry> {
    steam::game_of_app(cfg, appid, account).cloned()
}

fn phase(g: &GameEntry) -> &'static str {
    if Busy::is_busy(&g.key) {
        return "syncing";
    }
    match catchup::runner_phase(&g.key).as_deref() {
        Some("preLaunch") => "preLaunch",
        Some("uploading") => "uploading",
        Some(_) => "playing",
        None if process::is_running(&catchup::watch_names(g)) => "playing",
        None => "idle",
    }
}

fn cache_check(key: &str, report: &StatusReport) {
    let v = serde_json::to_value(report).ok();
    let _ = state::update_game(key, |g| g.last_check = v);
}

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
    steam_enabled: bool,
    health: String,
    store: Option<steamcloud::State>,
    backlog: Option<sync::Backlog>,
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
    lang: &'static str,
    playtime: playtime::Summary,
    api_version: u32,
}

fn steam_app_status(appid: u32, account: Option<&str>, refresh: bool) -> ApiResult {
    let cfg = config::load()?;
    let Some(g) = find_game(&cfg, appid, account) else {
        return Ok(json!({ "managed": false }));
    };
    let phase = phase(&g);
    let mut st = state::game(&g.key);
    let mut report: Option<StatusReport> = st.last_check.clone().and_then(|v| serde_json::from_value(v).ok());
    let stale = report
        .as_ref()
        .is_none_or(|r| (Utc::now() - r.checked_at).num_seconds() > FRESH_SECS || st.last_sync_at.is_some_and(|t| t > r.checked_at));
    if refresh && stale && phase == "idle" {
        let ctx = Ctx::from_config(cfg.clone());
        let r = sync::check(&ctx, &g);
        cache_check(&g.key, &r);
        report = Some(r);
        st = state::game(&g.key);
    }
    let health = match &g.store {
        Some(_) => steamcloud::wrapper_word(&cfg, &g).to_string(),
        None => serde_json::to_value(steam::health(&cfg, &g)).ok().and_then(|v| v["state"].as_str().map(str::to_string)).unwrap_or_default(),
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
        cloud_head: report.as_ref().and_then(|r| r.head.as_ref()).map(|h| CloudHead { device_name: h.device_name.clone(), created_at: h.created_at }),
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
// Actions

fn busy_reason(g: &GameEntry) -> Option<String> {
    if catchup::runner_phase(&g.key).is_some() || process::is_running(&catchup::watch_names(g)) {
        return Some(ludonimbus_core::tr!("游戏正在运行，请先退出游戏", "The game is running. Quit it first"));
    }
    if Busy::is_busy(&g.key) {
        return Some(ludonimbus_core::tr!("正在同步", "Syncing"));
    }
    None
}

/// Run `f` for a game in the background, once at a time per game.
fn in_background(key: &str, f: impl FnOnce(&Ctx, &GameEntry) -> Result<sync::Outcome> + Send + 'static) -> ApiResult {
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    if let Some(reason) = busy_reason(&g) {
        return Err(ApiError::new(409, reason));
    }
    let guard = Busy::begin(key).ok_or_else(|| ApiError::new(409, ludonimbus_core::tr!("正在同步", "Syncing")))?;
    std::thread::spawn(move || {
        let _guard = guard;
        let ctx = Ctx::from_config(cfg);
        let result = f(&ctx, &g);
        log(format!("{}: {:?}", g.key, result.as_ref().map_err(|e| e.to_string())));
        let _ = state::update_game(&g.key, |s| s.last_error = result.as_ref().err().map(|e| format!("{e:#}")));
        cache_check(&g.key, &sync::check(&ctx, &g));
    });
    Ok(json!({ "started": true }))
}

fn start_sync(key: &str, resolution: Option<&str>) -> ApiResult {
    let res = match resolution {
        Some("cloud") => Resolution::UseCloud,
        Some("local") => Resolution::UseLocal,
        _ => Resolution::Auto,
    };
    in_background(key, move |ctx, g| sync::sync(ctx, g, res, Trigger::SyncNow))
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

fn valid_id(id: &str) -> std::result::Result<String, ApiError> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::new(400, "bad version id"));
    }
    Ok(id.to_string())
}

fn start_restore(key: &str, id: &str) -> ApiResult {
    let id = valid_id(id)?;
    in_background(key, move |ctx, g| sync::restore(ctx, g, &id))
}

fn newer(key: &str) -> ApiResult {
    let ctx = Ctx::load()?;
    let g = find_by_key(&ctx.cfg, key)?;
    Ok(serde_json::to_value(savelib::newer_version(&ctx, &g)?).map_err(anyhow::Error::from)?)
}

fn apply_newer(key: &str, id: &str) -> ApiResult {
    let id = valid_id(id)?;
    let ctx = Ctx::load()?;
    let g = find_by_key(&ctx.cfg, key)?;
    let n = savelib::newer_version(&ctx, &g)?.filter(|n| n.snapshot.id == id).ok_or_else(|| ApiError::new(404, "no such newer version"))?;
    let target = key.to_string();
    in_background(key, move |ctx, _| transfer::apply(ctx, &n.key, &n.snapshot.id, &target, None))
}
