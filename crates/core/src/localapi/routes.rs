//! Authorization, routing and the endpoints of the local API.

use super::http::{query_param, unescape, Request, Response};
use super::{events, log, note_result, Host, Shared, API_VERSION};
use crate::achievements as ach;
use crate::config::{self, AppConfig, GameEntry};
use crate::snapshot::Trigger;
use crate::sync::{self, Ctx, Outcome, Resolution, Status, StatusReport};
use crate::{catchup, playtime, savelib, state, steam, steamcloud, tr};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// A cached status younger than this is returned as is, even with `refresh=1`.
const FRESH_SECS: i64 = 60;

pub(crate) struct ApiError {
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

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::new(500, e.to_string())
    }
}

type ApiResult = Result<Value, ApiError>;

pub(crate) fn handle(shared: &Shared, req: &Request) -> Response {
    let result = authorize(req, &shared.token, shared.port).and_then(|()| route(&shared.host, req));
    match result {
        Ok(v) => Response::json(200, &v),
        Err(e) => {
            if e.status >= 500 {
                log(format!("{} {} → {}: {}", req.method, req.path, e.status, e.message));
            }
            Response::error(e.status, &e.message)
        }
    }
}

/// Same length and bytes, in a time that doesn't depend on where they differ.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn authorize(req: &Request, token: &str, port: u16) -> Result<(), ApiError> {
    if req.header("Origin").is_some() {
        return Err(ApiError::new(403, "browser requests are not allowed"));
    }
    let host_ok = req.header("Host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    if !host_ok {
        return Err(ApiError::new(403, "bad host"));
    }
    if !req.header("Authorization").is_some_and(|a| same_secret(a, &format!("Bearer {token}"))) {
        return Err(ApiError::new(401, "missing or wrong token"));
    }
    Ok(())
}

fn route(host: &Arc<dyn Host>, req: &Request) -> ApiResult {
    let q = |name: &str| query_param(&req.query, name);
    let account = || q("account").filter(|a| !a.is_empty());
    let parts: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["v1", "ping"]) => Ok(json!({
            "app": "ludonimbus",
            "version": crate::APP_VERSION,
            "apiVersion": API_VERSION,
            "lang": crate::i18n::current().code(),
            "headless": host.headless(),
        })),
        ("GET", ["v1", "steam", "apps", appid]) => {
            let refresh = q("refresh").is_some_and(|v| v == "1" || v == "true");
            steam_app_status(host.as_ref(), parse_appid(appid)?, account(), refresh)
        }
        ("GET", ["v1", "steam", "apps", appid, "achievements"]) => app_achievements(parse_appid(appid)?, account()),
        ("GET", ["v1", "steam", "playtime"]) => steam_playtimes(account()),
        ("POST", ["v1", "games", key, "sync"]) => start_sync(host, key, q("resolution")),
        ("POST", ["v1", "games", key, "open"]) => {
            find_by_key(&config::load()?, key)?;
            if !host.open_game(key) {
                return Err(not_here());
            }
            Ok(json!({ "ok": true }))
        }
        ("GET", ["v1", "games", key, "snapshots"]) => snapshots(host.as_ref(), key),
        ("POST", ["v1", "games", key, "restore", id]) => start_restore(host, key, id),
        ("GET", ["v1", "games", key, "playtime"]) => {
            let cfg = config::load()?;
            find_by_key(&cfg, key)?;
            Ok(serde_json::to_value(playtime::summary(&cfg, key))?)
        }
        ("GET", ["v1", "games", key, "gamebackup"]) => {
            find_by_key(&config::load()?, key)?;
            Ok(host.game_backup_status(key).ok_or_else(not_here)??)
        }
        ("POST", ["v1", "games", key, "gamebackup", "install"]) => {
            find_by_key(&config::load()?, key)?;
            Ok(host.game_backup_install(key).ok_or_else(not_here)??)
        }
        ("GET", ["v1", "games", key, "newer"]) => {
            let ctx = Ctx::load()?;
            let g = find_by_key(&ctx.cfg, key)?;
            Ok(serde_json::to_value(savelib::newer_version(&ctx, &g)?)?)
        }
        ("POST", ["v1", "games", key, "newer", id, "apply"]) => apply_newer(host, key, id),
        ("POST", ["v1", "games", key, "newer", id, "dismiss"]) => {
            let id = valid_id(id)?;
            find_by_key(&config::load()?, key)?;
            savelib::dismiss_newer(key, id)?;
            Ok(json!({ "ok": true }))
        }
        ("GET", ["v1", "games", key, "icon"]) => {
            find_by_key(&config::load()?, key)?;
            let path = ach::game_icon_path(key).ok_or_else(|| ApiError::new(404, "no icon"))?;
            Ok(json!({ "dataUrl": ach::icon_data_url(&path)? }))
        }
        ("GET", ["v1", "games", key, "achievements", id, "icon"]) => {
            if id.is_empty() || id.len() > 256 {
                return Err(ApiError::new(400, "bad achievement id"));
            }
            find_by_key(&config::load()?, key)?;
            let path = ach::icon_path(key, id).ok_or_else(|| ApiError::new(404, "no icon"))?;
            Ok(json!({ "dataUrl": ach::icon_data_url(&path)? }))
        }
        ("GET", ["v1", "achievements", "events"]) => {
            let after = match q("after") {
                None | Some("") => 0,
                Some(s) => s.parse().map_err(|_| ApiError::new(400, "bad after"))?,
            };
            Ok(events::list(after))
        }
        ("POST", ["v1", "achievements", "events", seq, "ack"]) => {
            let seq: u64 = seq.parse().map_err(|_| ApiError::new(400, "bad seq"))?;
            let shown = match q("shown") {
                None => "steam",
                Some(s @ ("steam" | "failed")) => s,
                Some(_) => return Err(ApiError::new(400, "bad shown")),
            };
            events::ack(seq, shown, q("reason").map(unescape).as_deref());
            Ok(json!({ "ok": true }))
        }
        // API 5: the plugins as Steam's hands (localapi::plugin).
        ("GET", ["v1", "steam", "plugin", "poll"]) => steam_plugin::poll(req),
        ("POST", ["v1", "steam", "plugin", "done", id]) => steam_plugin::done(req, id),
        ("POST", ["v1", "steam", "apps", appid, "report"]) => steam_plugin::report(req, parse_appid(appid)?),
        ("POST", ["v1", "games", key, "steam-cloud", "take-over"]) => steam_plugin::take_over(host, key),
        _ => Err(ApiError::new(404, "not found")),
    }
}

/// An endpoint this program doesn't serve (the headless agent has no window
/// and handles no game backups).
fn not_here() -> ApiError {
    ApiError::new(404, tr!("没有打开 Ludonimbus 应用时不能这样做", "This needs the Ludonimbus app to be running"))
}

fn parse_appid(s: &str) -> Result<u32, ApiError> {
    s.parse().map_err(|_| ApiError::new(400, "bad appid"))
}

/// A version's id (a ULID): letters and digits.
fn valid_id(id: &str) -> Result<&str, ApiError> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::new(400, "bad version id"));
    }
    Ok(id)
}

fn find_by_key(cfg: &AppConfig, key: &str) -> Result<GameEntry, ApiError> {
    cfg.game(key).cloned().ok_or_else(|| ApiError::new(404, tr!("Ludonimbus 里没有这个游戏（{key}）", "This game isn't in Ludonimbus ({key})")))
}

fn syncing() -> ApiError {
    ApiError::new(409, tr!("正在同步", "Syncing"))
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

fn phase(host: &dyn Host, g: &GameEntry) -> &'static str {
    if host.is_busy(&g.key) {
        return "syncing";
    }
    match catchup::runner_phase(&g.key).as_deref() {
        Some("preLaunch") => "preLaunch",
        Some("uploading") => "uploading",
        Some(_) => "playing",
        None if host.game_running(g) => "playing",
        None => "idle",
    }
}

fn cache_check(key: &str, report: &StatusReport) {
    let v = serde_json::to_value(report).ok();
    let _ = state::update_game(key, |g| g.last_check = v);
}

fn steam_app_status(host: &dyn Host, appid: u32, account: Option<&str>, refresh: bool) -> ApiResult {
    let cfg = config::load()?;
    let Some(g) = steam::game_of_app(&cfg, appid, account).cloned() else {
        return Ok(json!({ "managed": false }));
    };
    let phase = phase(host, &g);
    let mut st = state::game(&g.key);
    let mut report: Option<StatusReport> = st.last_check.clone().and_then(|v| serde_json::from_value(v).ok());
    // Old, or taken before the latest sync (e.g. the Steam wrapper uploaded since).
    let stale = report.as_ref().is_none_or(|r| (Utc::now() - r.checked_at).num_seconds() > FRESH_SECS || st.last_sync_at.is_some_and(|t| t > r.checked_at));
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
        lang: crate::i18n::current().code(),
        playtime: playtime::summary(&cfg, &g.key),
        api_version: API_VERSION,
    };
    Ok(serde_json::to_value(body)?)
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

/// The game page's achievements. A game Steam installed has Steam's own.
fn app_achievements(appid: u32, account: Option<&str>) -> ApiResult {
    let cfg = config::load()?;
    let Some(g) = steam::game_of_app(&cfg, appid, account).filter(|g| g.store.is_none()) else {
        return Ok(json!({ "managed": false }));
    };
    let mut v = serde_json::to_value(ach::view(&g.key))?;
    v["managed"] = json!(true);
    v["name"] = json!(g.name);
    v["lang"] = json!(crate::i18n::current().code());
    Ok(v)
}

// ---------------------------------------------------------------------------
// Version history and actions

/// Why the game's saves can't be changed right now.
fn busy_reason(host: &dyn Host, g: &GameEntry) -> Option<String> {
    if host.game_running(g) {
        return Some(tr!("游戏正在运行，请先退出游戏", "The game is running. Quit it first"));
    }
    if host.is_busy(&g.key) {
        return Some(tr!("正在同步", "Syncing"));
    }
    None
}

fn snapshots(host: &dyn Host, key: &str) -> ApiResult {
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    let ctx = Ctx::from_config(cfg);
    let mut list = ctx.cloud()?.list_snapshots(key)?;
    let head = sync::mainline_head(&list).map(|h| h.id);
    list.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    let st = state::game(key);
    Ok(json!({
        "key": key,
        "name": g.name,
        "snapshots": list,
        "headId": head,
        "lastSyncedId": st.last_synced_id,
        "deviceId": ctx.cfg.device.id,
        "pending": st.pending.len(),
        "busy": busy_reason(host, &g),
    }))
}

/// Run `work` for a game on a thread of its own, one at a time per game:
/// answers `{ started: true }` at once.
fn in_background(
    host: &Arc<dyn Host>,
    cfg: AppConfig,
    g: GameEntry,
    what: &'static str,
    work: impl FnOnce(&Ctx, &GameEntry) -> anyhow::Result<Outcome> + Send + 'static,
) -> ApiResult {
    let guard = host.begin(&g.key).ok_or_else(syncing)?;
    let host = host.clone();
    std::thread::Builder::new()
        .name("localapi-job".into())
        .spawn(move || {
            let _guard = guard;
            let ctx = Ctx::from_config(cfg);
            let result = work(&ctx, &g);
            log(format!("{}: {what} from Steam {:?}", g.key, result.as_ref().map_err(|e| format!("{e:#}"))));
            note_result(&g.key, &result);
            cache_check(&g.key, &sync::check(&ctx, &g));
            host.finished(&g, &result);
        })
        .map_err(anyhow::Error::from)?;
    Ok(json!({ "started": true }))
}

fn start_sync(host: &Arc<dyn Host>, key: &str, resolution: Option<&str>) -> ApiResult {
    let res = match resolution {
        None | Some("" | "auto") => Resolution::Auto,
        Some("cloud") => Resolution::UseCloud,
        Some("local") => Resolution::UseLocal,
        Some(_) => return Err(ApiError::new(400, "bad resolution")),
    };
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    if host.game_running(&g) {
        return Err(ApiError::new(409, tr!("游戏正在运行，退出游戏后会自动同步", "The game is running; saves sync after you quit")));
    }
    in_background(host, cfg, g, "sync", move |ctx, g| sync::sync(ctx, g, res, Trigger::SyncNow))
}

fn start_restore(host: &Arc<dyn Host>, key: &str, id: &str) -> ApiResult {
    let id = valid_id(id)?.to_string();
    let cfg = config::load()?;
    let g = find_by_key(&cfg, key)?;
    if let Some(reason) = busy_reason(host.as_ref(), &g) {
        return Err(ApiError::new(409, reason));
    }
    in_background(host, cfg, g, "restore", move |ctx, g| sync::restore(ctx, g, &id))
}

/// Apply the newer saves of another version (the ones `newer` offered).
fn apply_newer(host: &Arc<dyn Host>, key: &str, id: &str) -> ApiResult {
    let id = valid_id(id)?;
    let ctx = Ctx::load()?;
    let g = find_by_key(&ctx.cfg, key)?;
    if let Some(reason) = busy_reason(host.as_ref(), &g) {
        return Err(ApiError::new(409, reason));
    }
    let n = crate::elsewhere::newer(&ctx, &g)?.filter(|n| n.snapshot.id == id).ok_or_else(|| ApiError::new(404, "no such newer version"))?;
    in_background(host, ctx.cfg, g, "apply newer saves", move |ctx, g| crate::elsewhere::take(ctx, g, &n.source, &n.snapshot.id))
}

// ---------------------------------------------------------------------------
// The plugins as Steam's hands (API 5, localapi::plugin)

mod steam_plugin {
    use super::*;
    use crate::localapi::plugin::{self, AppReport, Caps, Hello};

    /// A query value, unescaped, at most `max` characters.
    fn text(req: &Request, name: &str, max: usize) -> Option<String> {
        query_param(&req.query, name).map(|v| unescape(v).chars().take(max).collect())
    }

    /// A plugin run's id: letters, digits and `-`.
    fn session(req: &Request) -> Result<String, ApiError> {
        text(req, "session", 64)
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .ok_or_else(|| ApiError::new(400, "bad session"))
    }

    /// A Steam account id (the `userdata` folder name).
    fn account(req: &Request) -> Option<String> {
        text(req, "account", 20).filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()))
    }

    fn report_of(req: &Request, session: &str) -> Option<AppReport> {
        // Launch options are at most a few hundred characters; Steam takes more.
        AppReport::from_query(|n| text(req, n, if n == "launchOptions" { 4096 } else { 16 }), session)
    }

    pub(super) fn poll(req: &Request) -> ApiResult {
        let hello = Hello {
            session: session(req)?,
            kind: text(req, "kind", 32).unwrap_or_default(),
            version: text(req, "version", 32).unwrap_or_default(),
            caps: Caps::parse(&text(req, "caps", 200).unwrap_or_default()),
            account: account(req),
        };
        Ok(plugin::poll(hello, config::load().ok().as_ref()))
    }

    pub(super) fn done(req: &Request, id: &str) -> ApiResult {
        let id: u64 = id.parse().map_err(|_| ApiError::new(400, "bad id"))?;
        let ok = text(req, "ok", 8).is_some_and(|v| v == "1" || v == "true");
        let report = session(req).ok().and_then(|s| report_of(req, &s));
        plugin::done(id, ok, text(req, "error", 500).filter(|e| !e.is_empty()), report);
        Ok(json!({ "ok": true }))
    }

    pub(super) fn report(req: &Request, appid: u32) -> ApiResult {
        let session = session(req)?;
        let account = account(req).ok_or_else(|| ApiError::new(400, "bad account"))?;
        let report = report_of(req, &session).ok_or_else(|| ApiError::new(400, "bad report"))?;
        plugin::report(&account, appid, report);
        Ok(json!({ "ok": true }))
    }

    /// "Hand to Ludonimbus" on a game's page in Steam: the game is backup
    /// only, and the plugin turns Steam Cloud off and (when it can) sets the
    /// launch options; in the background, like a sync.
    pub(super) fn take_over(host: &Arc<dyn Host>, key: &str) -> ApiResult {
        let cfg = config::load()?;
        let g = find_by_key(&cfg, key)?;
        let Some(link) = g.store.clone() else {
            return Err(ApiError::new(400, tr!("「{}」不是从 Steam 安装的游戏", "\"{}\" isn't a game installed through Steam", g.name)));
        };
        if steamcloud::state(&g) != steamcloud::State::Backup {
            return Err(ApiError::new(409, tr!("这个游戏现在不是「只备份」", "This game isn't backup only now")));
        }
        let hands = plugin::Plugin::for_account(&link.account).ok_or_else(|| {
            ApiError::new(409, tr!("Steam 插件没有连上 Ludonimbus，请稍后再试", "The Steam plugin isn't connected to Ludonimbus. Try again in a moment"))
        })?;
        if let Some(reason) = busy_reason(host.as_ref(), &g) {
            return Err(ApiError::new(409, reason));
        }
        let wrapper = hands.caps.launch_options;
        in_background(host, cfg, g, "take over from Steam Cloud", move |ctx, g| {
            steamcloud::take_over_through(ctx, &g.key, wrapper, &hands).map(|_| Outcome::UpToDate)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        assert!(same_secret("Bearer ab", "Bearer ab"));
        assert!(!same_secret("Bearer ab", "Bearer ac"));
        assert!(!same_secret("Bearer a", "Bearer ab"));
        assert!(valid_id("01J8ZQ4Y3K5W6X7V8T9S0R1P2N").is_ok());
        for bad in ["", "a/b", "..", "a-b", &"a".repeat(65)] {
            assert!(valid_id(bad).is_err(), "{bad}");
        }
    }
}
