//! The local HTTP API the Steam plugins call: the Millennium plugin on the
//! desktop and the Decky plugin in Steam Deck Game Mode. One implementation
//! for both programs that serve it: the desktop app while it sits in the
//! tray (`app/src-tauri/src/agent.rs`) and the headless `ludonimbus-agent`
//! (`crates/agent`). What differs between them goes through [`Host`].
//!
//! - Listens on `127.0.0.1` at a random port. The port and a token that
//!   changes on every start are written to `agent.json` in the data folder
//!   ([`Server::publish`], only this user can read it); every request must
//!   send `Authorization: Bearer <token>`.
//! - Requests from a web page (they always carry `Origin`) and requests whose
//!   `Host` is not this loopback address (DNS rebinding) are refused.
//! - One request per connection, at most [`Options::max_connections`] served
//!   at once (more wait to be accepted), each read within
//!   [`Options::io_timeout`], the head at most 16 KiB and the body (no
//!   endpoint takes one) at most 16 KiB.
//!
//! ```text
//! GET  /v1/ping
//! GET  /v1/steam/apps/{appid}[?account={id}&refresh=1]   cloud save status of a Steam shortcut, or of a game Steam installed (API 4)
//! GET  /v1/steam/apps/{appid}/achievements[?account=]    achievement progress and list for the game page
//! GET  /v1/steam/playtime[?account={id}]                  playtime totals of every linked shortcut (API 2)
//! POST /v1/games/{key}/sync[?resolution=cloud|local]      start "sync now" in the background (resolution: API 3)
//! POST /v1/games/{key}/open                               show the game in the Ludonimbus window (desktop app)
//! GET  /v1/games/{key}/snapshots                          version history in the cloud (API 2)
//! POST /v1/games/{key}/restore/{id}                       restore a version in the background (API 2)
//! GET  /v1/games/{key}/playtime                           playtime over all devices (API 2)
//! GET  /v1/games/{key}/gamebackup                         game files missing? newest game backup, running job (desktop app, API 2)
//! POST /v1/games/{key}/gamebackup/install                 show the game with the restore dialog open (desktop app, API 2)
//! GET  /v1/games/{key}/newer                              newer saves in another version of the game (API 3)
//! POST /v1/games/{key}/newer/{id}/apply                   apply them in the background (API 3)
//! POST /v1/games/{key}/newer/{id}/dismiss                 don't offer them again (API 3)
//! GET  /v1/games/{key}/icon                               the game's icon, for achievements without one
//! GET  /v1/games/{key}/achievements/{id}/icon             a local achievement icon as a data URL
//! GET  /v1/achievements/events?after={seq}                new unlocks to show (see [`events`])
//! POST /v1/achievements/events/{seq}/ack?shown=steam|failed&reason=
//! GET  /v1/steam/plugin/poll?session=&kind=&version=&caps=&account=   a plugin takes requests for Steam (API 5, see [`plugin`])
//! POST /v1/steam/plugin/done/{id}?ok=&error=&<report>     its answer, with what Steam says now (API 5)
//! POST /v1/steam/apps/{appid}/report?session=&account=&<report>   what Steam says about a game, on its own (API 5)
//! POST /v1/games/{key}/steam-cloud/take-over              hand a backed-up Steam game to Ludonimbus through the plugin (API 5)
//! ```
//!
//! The answers are described in docs/steam-plugin.md.

pub mod events;
mod http;
pub mod plugin;
mod routes;

use crate::config::GameEntry;
use crate::sync::Outcome;
use crate::{catchup, logger, paths, process, util};
use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// 2 (0.6.0): version history, restore, playtime.
/// 3 (0.7.0): conflict resolution on sync, newer saves in another version.
/// 4 (1.0.0): what a game still owes the cloud after playing (`backlog`);
/// games Steam installed (`store`); each version's `platform`.
/// 5 (1.0.0): the plugins as Steam's hands: Steam Cloud switches and launch
/// options of games Steam installed ([`plugin`]); handing a game to
/// Ludonimbus from inside Steam.
pub const API_VERSION: u32 = 5;

fn log(msg: impl AsRef<str>) {
    logger::log("agent", msg);
}

/// A lock whose data stays valid when a thread panicked while holding it
/// (every change made under it is a single step).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// What differs between the desktop app and the headless agent

/// Held while a game is being synced or restored; dropping it ends that.
pub type Guard = Box<dyn Any + Send>;

/// What the program serving the API provides. The defaults are the headless
/// agent's: no window, no game backups, nobody to tell when a sync ends.
pub trait Host: Send + Sync + 'static {
    /// No window: `ludonimbus-agent` (said in `ping` and `agent.json`).
    fn headless(&self) -> bool {
        true
    }

    /// Mark a game as being synced by this program; `None` when it is
    /// already (the app shares this with its window and its watcher, so a
    /// game is never synced twice at once).
    fn begin(&self, key: &str) -> Option<Guard>;

    /// Whether this program syncs the game right now.
    fn is_busy(&self, key: &str) -> bool;

    /// Whether the game is being played: through the launch wrapper, or
    /// started any other way.
    fn game_running(&self, g: &GameEntry) -> bool {
        catchup::wrapper(&g.key) == catchup::Wrapper::Running || process::is_running(&catchup::watch_names(g))
    }

    /// A sync, restore or apply the API started has ended (the game is still
    /// marked busy while this runs).
    fn finished(&self, _g: &GameEntry, _result: &Result<Outcome>) {}

    /// Show the game in the app's window; `false` without a window (the
    /// endpoint is then not found).
    fn open_game(&self, _key: &str) -> bool {
        false
    }

    /// `GET /v1/games/{key}/gamebackup` for a game in the library; `None`
    /// where game backups aren't handled (not found).
    fn game_backup_status(&self, _key: &str) -> Option<Result<serde_json::Value>> {
        None
    }

    /// `POST /v1/games/{key}/gamebackup/install`; `None` where game backups
    /// aren't handled (not found).
    fn game_backup_install(&self, _key: &str) -> Option<Result<serde_json::Value>> {
        None
    }
}

/// Games being synced by this program, for a [`Host`] that has no such list
/// of its own (the headless agent).
#[derive(Clone, Default)]
pub struct Busy(Arc<Mutex<HashSet<String>>>);

struct BusyGuard(Busy, String);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        lock(&self.0 .0).remove(&self.1);
    }
}

impl Busy {
    pub fn begin(&self, key: &str) -> Option<Guard> {
        lock(&self.0).insert(key.to_string()).then(|| Box::new(BusyGuard(self.clone(), key.to_string())) as Guard)
    }

    pub fn is_busy(&self, key: &str) -> bool {
        lock(&self.0).contains(key)
    }
}

// ---------------------------------------------------------------------------
// agent.json

/// What `agent.json` says: where the API listens and its token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
    pub api_version: u32,
    /// Written by `ludonimbus-agent`, which has no window.
    #[serde(default)]
    pub headless: bool,
}

pub fn agent_file() -> PathBuf {
    paths::data_dir().join("agent.json")
}

pub fn read_agent_file() -> Option<AgentInfo> {
    serde_json::from_slice(&std::fs::read(agent_file()).ok()?).ok()
}

/// The process that wrote `agent.json`, when it still runs.
pub fn live_owner() -> Option<u32> {
    read_agent_file().map(|i| i.pid).filter(|&pid| process::pid_alive(pid))
}

/// Remove `agent.json` when this process wrote it (on exit).
pub fn remove_agent_file() {
    if read_agent_file().is_some_and(|i| i.pid == std::process::id()) {
        let _ = std::fs::remove_file(agent_file());
    }
}

// ---------------------------------------------------------------------------
// The server

/// Limits of a [`Server`].
#[derive(Debug, Clone)]
pub struct Options {
    /// Connections served at once. The plugins make a few requests at a
    /// time; more connections wait until one ends.
    pub max_connections: usize,
    /// How long a client may take to send its request, and to take the answer.
    pub io_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options { max_connections: 16, io_timeout: Duration::from_secs(10) }
    }
}

/// What the request handlers share.
pub(crate) struct Shared {
    pub host: Arc<dyn Host>,
    pub token: String,
    pub port: u16,
}

/// Connections being served, at most `max`.
struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
    max: usize,
}

/// One connection's place, given back when it ends.
struct Slot(Arc<Slots>);

impl Drop for Slot {
    fn drop(&mut self) {
        *lock(&self.0.used) -= 1;
        self.0.freed.notify_one();
    }
}

impl Slots {
    /// Wait for a free place; `None` once the server stops.
    fn take(self: &Arc<Self>, stop: &AtomicBool) -> Option<Slot> {
        let mut used = lock(&self.used);
        let mut since: Option<Instant> = None;
        while *used >= self.max && !stop.load(Ordering::SeqCst) {
            if since.is_none() {
                since = Some(Instant::now());
            }
            used = self.freed.wait_timeout(used, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner()).0;
        }
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        if let Some(t) = since.filter(|t| t.elapsed() > Duration::from_secs(1)) {
            log(format!("all {} connections of the local API were busy for {} ms", self.max, t.elapsed().as_millis()));
        }
        *used += 1;
        Some(Slot(self.clone()))
    }
}

pub struct Server {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    slots: Arc<Slots>,
}

impl Server {
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// What `agent.json` says for this server.
    pub fn info(&self) -> AgentInfo {
        AgentInfo {
            port: self.shared.port,
            token: self.shared.token.clone(),
            pid: std::process::id(),
            version: crate::APP_VERSION.to_string(),
            api_version: API_VERSION,
            headless: self.shared.host.headless(),
        }
    }

    /// Write `agent.json`, so the plugins find this server. It holds the
    /// token: only this user may read it.
    pub fn publish(&self) -> Result<()> {
        util::write_private(&agent_file(), &serde_json::to_vec_pretty(&self.info())?)
    }

    /// Stop accepting connections and let go of the port (requests being
    /// served are finished).
    pub fn stop(&self) {
        if !self.stop.swap(true, Ordering::SeqCst) {
            self.slots.freed.notify_all();
            // Wake the accept loop.
            let _ = TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, self.shared.port).into(), Duration::from_secs(1));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the API on `127.0.0.1` at a random port, with the default limits.
pub fn start(host: impl Host) -> Result<Server> {
    start_with(host, Options::default())
}

pub fn start_with(host: impl Host, options: Options) -> Result<Server> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| anyhow!("could not start the local API: {e}"))?;
    let port = listener.local_addr()?.port();
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let shared = Arc::new(Shared { host: Arc::new(host), token, port });
    let stop = Arc::new(AtomicBool::new(false));
    let slots = Arc::new(Slots { used: Mutex::new(0), freed: Condvar::new(), max: options.max_connections.max(1) });
    let (shared2, stop2, slots2) = (shared.clone(), stop.clone(), slots.clone());
    std::thread::Builder::new().name("localapi".into()).spawn(move || accept(listener, &shared2, &stop2, &slots2, options.io_timeout))?;
    Ok(Server { shared, stop, slots })
}

fn accept(listener: TcpListener, shared: &Arc<Shared>, stop: &AtomicBool, slots: &Arc<Slots>, io_timeout: Duration) {
    loop {
        // A place first: while all are taken, connections wait to be accepted.
        let Some(slot) = slots.take(stop) else { break };
        let conn = listener.accept();
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let tcp = match conn {
            Ok((tcp, _)) => tcp,
            Err(_) => {
                // Out of sockets or the like: don't spin.
                drop(slot);
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let shared = shared.clone();
        let spawned = std::thread::Builder::new().name("localapi-conn".into()).spawn(move || {
            let _slot = slot;
            serve(tcp, &shared, io_timeout);
        });
        if let Err(e) = spawned {
            log(format!("local API: no thread for a connection: {e}"));
        }
    }
}

/// One connection: one request, its answer.
fn serve(mut tcp: TcpStream, shared: &Shared, io_timeout: Duration) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_write_timeout(Some(io_timeout));
    let req = match http::read_request(&mut tcp, io_timeout) {
        Ok(r) => r,
        Err(http::Refused::Gone) => return,
        Err(http::Refused::Answer(resp)) => {
            let _ = http::write_response(&mut tcp, &resp);
            http::linger(&mut tcp);
            return;
        }
    };
    let resp = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| routes::handle(shared, &req))).unwrap_or_else(|_| {
        log(format!("{} {}: the handler panicked", req.method, req.path));
        http::Response::error(500, "internal error")
    });
    let _ = http::write_response(&mut tcp, &resp);
    let _ = tcp.shutdown(Shutdown::Write);
}

/// Note how a sync, restore or apply the API started ended in the game's
/// state, for the plugins' `lastError`: a failure is the error; a result
/// that leaves nothing to do clears it; a version queued or a cloud not
/// reached keeps what the sync itself noted.
#[doc(hidden)]
pub fn note_result(key: &str, result: &Result<Outcome>) {
    let error = match result {
        Err(e) => Some(format!("{e:#}")),
        Ok(o) if o.is_final() => None,
        Ok(_) => return,
    };
    let _ = crate::state::update_game(key, |s| s.last_error = error);
}
