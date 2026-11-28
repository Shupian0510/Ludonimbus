//! Steam itself, through the Steam plugins (API 5; docs/steam-plugin.md,
//! "Steam Cloud through the plugin").
//!
//! The plugins run inside Steam and can read and change what Steam's own
//! settings say about a game (`SteamClient.Apps`): its Steam Cloud switch
//! and its launch options. Where Steam keeps the switch on disk isn't
//! documented, and Steam rewrites its files while it runs, so asking Steam
//! is the reliable way. The plugins are clients of the local API, so
//! Ludonimbus asks through a queue: a request waits here until a plugin's
//! next poll ([`poll`]) takes it; the plugin reads first (the switch is a
//! toggle, not a setter), changes only what differs, reads again and
//! answers with what Steam says now ([`done`]).
//!
//! A plugin also reports, on its own, what Steam says about the games
//! Steam installed that are in the library ([`report`]), whenever that
//! changes. Those reports are kept in `steam-plugin.json` in the data
//! folder, so that other programs (the launch wrapper) read them too
//! ([`fresh`]). One counts only while the plugin that sent it still polls:
//! with Steam closed (or the plugin gone), Steam's files count again.
//!
//! Older plugins never poll: they don't advertise anything, and everything
//! goes through Steam's files as before.

use super::lock;
use crate::config::AppConfig;
use crate::{logger, paths, util};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use ts_rs::TS;

fn log(msg: impl AsRef<str>) {
    logger::log("steamplugin", msg);
}

/// A plugin that polled this long ago is still there (it polls every 2 s).
const CONNECTED: Duration = Duration::from_secs(10);
/// How often a plugin's last poll is written down for other programs.
const SEEN_WRITE_SECS: i64 = 20;
/// A plugin whose last poll written down is older than this is gone, and
/// its reports no longer count.
const SEEN_ALIVE_SECS: i64 = 60;
/// Plugins and reports not heard of for this long are dropped from the file.
const FORGET_SECS: i64 = 7 * 24 * 3600;
/// How long a request waits for a plugin to take it.
const TAKE_WAIT: Duration = Duration::from_secs(10);
/// How long a plugin has to answer a request it took (it waits up to 5 s for
/// Steam to show a change).
const ANSWER_WAIT: Duration = Duration::from_secs(25);

/// What a plugin can do in Steam.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SteamPluginCaps")]
pub struct Caps {
    /// Read and turn a game's Steam Cloud switch.
    pub cloud: bool,
    /// Read and set a game's launch options while Steam runs.
    pub launch_options: bool,
}

impl Caps {
    /// `cloud,launchOptions` as a plugin sends it.
    pub fn parse(s: &str) -> Caps {
        let has = |w: &str| s.split(',').any(|c| c.trim() == w);
        Caps { cloud: has("cloud"), launch_options: has("launchOptions") }
    }

    fn or(self, o: Caps) -> Caps {
        Caps { cloud: self.cloud || o.cloud, launch_options: self.launch_options || o.launch_options }
    }
}

/// What Steam itself says about one game, as a plugin read it from the
/// game's app details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppReport {
    /// The game has Steam Cloud at all (`bCloudAvailable`).
    pub available: bool,
    /// The account's switch for every game (`bCloudEnabledForAccount`).
    pub account_enabled: bool,
    /// The game's own switch (`bCloudEnabledForApp`).
    pub app_enabled: bool,
    /// How Steam Cloud stands with the game (`eCloudStatus`): 3 synchronized,
    /// 4 checking, 5 out of sync, 6 uploading, 7 downloading, 8 failed,
    /// 9 conflict, 10 pending elsewhere; 0–2 say nothing.
    pub status: Option<u32>,
    /// The game's launch options (`strLaunchOptions`); `None` when the
    /// plugin couldn't read them.
    pub launch_options: Option<String>,
    /// The plugin run that read it.
    #[serde(default)]
    pub session: String,
    pub at: DateTime<Utc>,
}

impl AppReport {
    /// Steam Cloud syncs the game's saves.
    pub fn on(&self) -> bool {
        self.available && self.account_enabled && self.app_enabled
    }

    /// Whether Steam is done syncing the game with Steam Cloud, when the
    /// status says so either way.
    pub fn synced(&self) -> Option<bool> {
        match self.status {
            Some(3) => Some(true),
            Some(4..=10) => Some(false),
            _ => None,
        }
    }

    /// The same content (who read it and when aside).
    fn same(&self, o: &AppReport) -> bool {
        (self.available, self.account_enabled, self.app_enabled, self.status, &self.launch_options)
            == (o.available, o.account_enabled, o.app_enabled, o.status, &o.launch_options)
    }

    /// From the query of a report or an answer: `available`, `accountEnabled`,
    /// `appEnabled` (1 or 0), `status`, `launchOptions` (escaped; absent when
    /// unknown). `None` when the switches are missing.
    pub(crate) fn from_query(q: impl Fn(&str) -> Option<String>, session: &str) -> Option<AppReport> {
        let flag = |name: &str| q(name).map(|v| v == "1" || v == "true");
        Some(AppReport {
            available: flag("available")?,
            account_enabled: flag("accountEnabled")?,
            app_enabled: flag("appEnabled")?,
            status: q("status").and_then(|s| s.parse().ok()),
            launch_options: q("launchOptions"),
            session: session.to_string(),
            at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// steam-plugin.json: what other programs read

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeenPlugin {
    kind: String,
    version: String,
    caps: Caps,
    account: Option<String>,
    seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Seen {
    /// By session.
    #[serde(default)]
    plugins: BTreeMap<String, SeenPlugin>,
    /// By `<account>/<appid>`.
    #[serde(default)]
    apps: BTreeMap<String, AppReport>,
}

fn seen_file() -> PathBuf {
    paths::data_dir().join("steam-plugin.json")
}

fn read_seen() -> Seen {
    std::fs::read(seen_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_seen(seen: &mut Seen) {
    let now = Utc::now();
    seen.plugins.retain(|_, p| (now - p.seen_at).num_seconds() < FORGET_SECS);
    let sessions: Vec<String> = seen.plugins.keys().cloned().collect();
    seen.apps.retain(|_, r| sessions.contains(&r.session));
    match serde_json::to_vec_pretty(&seen) {
        Ok(bytes) => {
            if let Err(e) = util::atomic_write(&seen_file(), &bytes) {
                log(format!("couldn't write steam-plugin.json: {e:#}"));
            }
        }
        Err(e) => log(format!("couldn't write steam-plugin.json: {e}")),
    }
}

fn app_key(account: &str, appid: u32) -> String {
    format!("{account}/{appid}")
}

/// What a plugin that still polls last said about the game `appid` of
/// `account`; `None` without one (no plugin, Steam closed, an older
/// plugin): then Steam's files count.
pub fn fresh(account: &str, appid: u32) -> Option<AppReport> {
    fresh_in(&read_seen(), account, appid)
}

fn fresh_in(seen: &Seen, account: &str, appid: u32) -> Option<AppReport> {
    let r = seen.apps.get(&app_key(account, appid))?;
    let p = seen.plugins.get(&r.session)?;
    ((Utc::now() - p.seen_at).num_seconds() < SEEN_ALIVE_SECS).then(|| r.clone())
}

/// [`fresh`] for many games with one read of the file.
pub struct Fresh(Seen);

impl Fresh {
    pub fn load() -> Fresh {
        Fresh(read_seen())
    }

    pub fn get(&self, account: &str, appid: u32) -> Option<AppReport> {
        fresh_in(&self.0, account, appid)
    }
}

// ---------------------------------------------------------------------------
// The queue (this process: the one serving the local API)

/// What Ludonimbus asks a plugin to do with a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    /// Read what Steam says now.
    Read,
    /// Turn its Steam Cloud switch on or off (read first, toggle if needed).
    Cloud(bool),
    /// Set its launch options (`""`: none).
    LaunchOptions(String),
}

struct Ask {
    id: u64,
    account: String,
    appid: u32,
    want: Want,
    queued: Instant,
}

/// A plugin's answer to a request.
struct Answer {
    ok: bool,
    error: Option<String>,
    report: Option<AppReport>,
}

struct Live {
    caps: Caps,
    account: Option<String>,
    last: Instant,
}

#[derive(Default)]
struct Hub {
    next: u64,
    waiting: Vec<Ask>,
    /// Taken by a plugin, not answered yet: when, and which game.
    taken: HashMap<u64, (Instant, String, u32)>,
    answers: HashMap<u64, Answer>,
    /// The plugins polling this process, by session.
    live: HashMap<String, Live>,
    /// When each session's last poll was written down.
    written: HashMap<String, Instant>,
}

struct Shared {
    hub: Mutex<Hub>,
    changed: Condvar,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared { hub: Mutex::new(Hub::default()), changed: Condvar::new() })
}

/// What a plugin says about itself when it polls.
#[derive(Debug, Clone, Default)]
pub struct Hello {
    pub session: String,
    pub kind: String,
    pub version: String,
    pub caps: Caps,
    /// The Steam account signed in (the `userdata` folder name), when known.
    pub account: Option<String>,
}

/// A plugin polls: it is connected, takes the requests for its account, and
/// learns which games to report. `resend`: this process hasn't heard from
/// this plugin run before, so it should send its reports again (they may
/// have changed while Ludonimbus wasn't listening).
pub(crate) fn poll(hello: Hello, cfg: Option<&AppConfig>) -> Value {
    let s = shared();
    let mut hub = lock(&s.hub);
    let now = Instant::now();
    let resend = !hub.live.contains_key(&hello.session);
    hub.live.insert(hello.session.clone(), Live { caps: hello.caps, account: hello.account.clone(), last: now });
    hub.live.retain(|_, l| now.duration_since(l.last) < Duration::from_secs(600));
    // Written down for other programs now and then: when it is new, and while
    // they have reports of it to go by.
    let due = hub.written.get(&hello.session).is_none_or(|t| now.duration_since(*t).as_secs() as i64 >= SEEN_WRITE_SECS);
    if due {
        hub.written.insert(hello.session.clone(), now);
        let mut seen = read_seen();
        if !seen.plugins.contains_key(&hello.session) || seen.apps.values().any(|r| r.session == hello.session) {
            seen.plugins.insert(
                hello.session.clone(),
                SeenPlugin { kind: hello.kind.clone(), version: hello.version.clone(), caps: hello.caps, account: hello.account.clone(), seen_at: Utc::now() },
            );
            write_seen(&mut seen);
        }
    }
    if resend {
        log(format!("{} plugin {} connected ({:?}, account {:?})", hello.kind, hello.version, hello.caps, hello.account));
    }
    // Requests for its account (all of them when it doesn't know its account).
    let mine = |a: &Ask| hello.account.as_deref().is_none_or(|acc| acc == a.account);
    let (take, keep): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut hub.waiting).into_iter().partition(|a| mine(a) && can(hello.caps, &a.want));
    hub.waiting = keep;
    let requests: Vec<Value> = take
        .iter()
        .map(|a| {
            let mut v = json!({ "id": a.id, "appid": a.appid, "account": a.account });
            match &a.want {
                Want::Read => v["kind"] = json!("read"),
                Want::Cloud(on) => {
                    v["kind"] = json!("cloud");
                    v["on"] = json!(on);
                }
                Want::LaunchOptions(o) => {
                    v["kind"] = json!("launchOptions");
                    v["value"] = json!(o);
                }
            }
            v
        })
        .collect();
    for a in &take {
        hub.taken.insert(a.id, (now, a.account.clone(), a.appid));
    }
    drop(hub);
    if !take.is_empty() {
        s.changed.notify_all();
    }
    let watch: Vec<u32> = cfg
        .map(|c| c.games.iter().filter_map(|g| g.store.as_ref()).filter(|l| hello.account.as_deref().is_none_or(|a| a == l.account)).map(|l| l.appid).collect())
        .unwrap_or_default();
    json!({ "requests": requests, "watch": watch, "resend": resend, "apiVersion": super::API_VERSION })
}

fn can(caps: Caps, want: &Want) -> bool {
    match want {
        Want::Read | Want::Cloud(_) => caps.cloud,
        Want::LaunchOptions(_) => caps.launch_options,
    }
}

/// A plugin says what Steam says about a game now (on its own, or with an
/// answer). Written down when it changed.
pub(crate) fn report(account: &str, appid: u32, report: AppReport) {
    let s = shared();
    let _hub = lock(&s.hub);
    let mut seen = read_seen();
    let key = app_key(account, appid);
    if seen.apps.get(&key).is_some_and(|r| r.session == report.session && r.same(&report)) {
        return;
    }
    // The plugin that sends it is there now (also before its first poll was
    // written down).
    seen.plugins.entry(report.session.clone()).or_default().seen_at = Utc::now();
    seen.apps.insert(key, report);
    write_seen(&mut seen);
}

/// A plugin answers request `id`, with what Steam says about the game
/// afterwards. Unknown ids (timed out) are ignored.
pub(crate) fn done(id: u64, ok: bool, error: Option<String>, with: Option<AppReport>) {
    let s = shared();
    let Some((_, account, appid)) = lock(&s.hub).taken.get(&id).cloned() else { return };
    if let Some(r) = &with {
        report(&account, appid, r.clone());
    }
    let mut hub = lock(&s.hub);
    if hub.taken.remove(&id).is_none() {
        return;
    }
    hub.answers.insert(id, Answer { ok, error, report: with });
    drop(hub);
    s.changed.notify_all();
}

/// What a plugin connected to this process can do for the games of
/// `account`; `None` when none is (none polled in the last seconds, or one
/// signed in to another account).
pub fn connected(account: &str) -> Option<Caps> {
    let hub = lock(&shared().hub);
    let now = Instant::now();
    hub.live
        .values()
        .filter(|l| now.duration_since(l.last) < CONNECTED && l.account.as_deref().is_none_or(|a| a == account))
        .map(|l| l.caps)
        .reduce(Caps::or)
        .filter(|c| c.cloud)
}

/// Ask a plugin to do `want` with the game `appid` of `account`, and wait
/// for its answer: what Steam says afterwards.
pub fn ask(account: &str, appid: u32, want: Want) -> Result<AppReport> {
    let s = shared();
    let mut hub = lock(&s.hub);
    hub.next += 1;
    let id = hub.next;
    hub.waiting.push(Ask { id, account: account.to_string(), appid, want: want.clone(), queued: Instant::now() });
    loop {
        if let Some(a) = hub.answers.remove(&id) {
            drop(hub);
            return match (a.ok, a.report) {
                (true, Some(r)) => Ok(r),
                (true, None) => bail!(crate::tr!("Steam 插件没有说 Steam 现在的设置", "The Steam plugin didn't say what Steam's settings are now")),
                (false, _) => bail!(crate::tr!(
                    "Steam 插件没能完成：{}",
                    "The Steam plugin couldn't do it: {}",
                    a.error.unwrap_or_else(|| crate::tr!("没有说明原因", "no reason given"))
                )),
            };
        }
        let now = Instant::now();
        let late = match hub.taken.get(&id) {
            Some((t, _, _)) => now.duration_since(*t) > ANSWER_WAIT,
            None => hub.waiting.iter().find(|a| a.id == id).is_none_or(|a| now.duration_since(a.queued) > TAKE_WAIT),
        };
        if late {
            hub.waiting.retain(|a| a.id != id);
            hub.taken.remove(&id);
            drop(hub);
            log(format!("{account}/{appid}: no answer from the Steam plugin to {want:?}"));
            bail!(crate::tr!(
                "Steam 插件没有回应。请确认 Steam 开着、插件已启用，再试一次",
                "The Steam plugin didn't answer. Make sure Steam is open and the plugin is enabled, then try again"
            ));
        }
        hub = s.changed.wait_timeout(hub, Duration::from_millis(250)).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// Steam, live, through the plugin connected to this process
/// ([`crate::steamcloud::SteamHands`]).
#[derive(Debug, Clone, Copy)]
pub struct Plugin {
    pub caps: Caps,
}

impl Plugin {
    /// The plugin connected for the games of `account`, when one is and can
    /// turn Steam Cloud switches.
    pub fn for_account(account: &str) -> Option<Plugin> {
        connected(account).map(|caps| Plugin { caps })
    }
}

impl crate::steamcloud::SteamHands for Plugin {
    fn sets_launch_options(&self) -> bool {
        self.caps.launch_options
    }

    fn read(&self, account: &str, appid: u32) -> Result<AppReport> {
        ask(account, appid, Want::Read)
    }

    fn set_cloud(&self, account: &str, appid: u32, on: bool) -> Result<AppReport> {
        ask(account, appid, Want::Cloud(on))
    }

    fn set_launch_options(&self, account: &str, appid: u32, options: &str) -> Result<AppReport> {
        if !self.caps.launch_options {
            return Err(anyhow!("the Steam plugin can't set launch options"));
        }
        ask(account, appid, Want::LaunchOptions(options.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(available: bool, account: bool, app: bool, status: Option<u32>) -> AppReport {
        AppReport { available, account_enabled: account, app_enabled: app, status, launch_options: None, session: "s".into(), at: Utc::now() }
    }

    #[test]
    fn reports() {
        assert!(r(true, true, true, None).on());
        assert!(!r(false, true, true, None).on());
        assert!(!r(true, false, true, None).on());
        assert!(!r(true, true, false, None).on());
        assert_eq!(r(true, true, true, Some(3)).synced(), Some(true));
        assert_eq!(r(true, true, true, Some(9)).synced(), Some(false));
        assert_eq!(r(true, true, true, Some(2)).synced(), None);
        assert_eq!(r(true, true, true, Some(1)).synced(), None);
        assert_eq!(Caps::parse("cloud,launchOptions"), Caps { cloud: true, launch_options: true });
        assert_eq!(Caps::parse("cloud"), Caps { cloud: true, launch_options: false });
        assert_eq!(Caps::parse(""), Caps::default());
        let q = |pairs: &'static [(&'static str, &'static str)]| move |n: &str| pairs.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string());
        let got =
            AppReport::from_query(q(&[("available", "1"), ("accountEnabled", "1"), ("appEnabled", "0"), ("status", "3"), ("launchOptions", "-x")]), "abc")
                .unwrap();
        assert_eq!((got.on(), got.status, got.launch_options.as_deref(), got.session.as_str()), (false, Some(3), Some("-x"), "abc"));
        assert!(AppReport::from_query(q(&[("available", "1")]), "abc").is_none());
    }
}
