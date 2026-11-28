//! The local HTTP API the Steam plugins call (`ludonimbus_core::localapi`),
//! spoken to over TCP the way the plugins' backends do: on an ephemeral
//! port, with fake hosts (one with a window like the desktop app, one
//! without like `ludonimbus-agent`), a temporary data folder and a local
//! folder as the cloud. Never real saves, Steam files or cloud accounts.

use ludonimbus_core::achievements::{self as ach, Achievement, NewUnlock, Schema, SourceKind};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, SteamLink};
use ludonimbus_core::localapi::{self, events, Busy, Guard, Host, Options};
use ludonimbus_core::state;
use ludonimbus_core::sync::Outcome;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const APPID: u32 = 3_000_000_001;
const KEY: &str = "test-game";

/// One data folder for every test of this file: they run on threads of one
/// process and share its environment. Only `endpoints` changes the library.
fn root() -> &'static Path {
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    ROOT.get_or_init(|| {
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("LUDONIMBUS_HOME", t.path().join("home"));
        t
    })
    .path()
}

/// What the fake hosts were told and what they say.
#[derive(Default)]
struct Fake {
    busy: Busy,
    window: bool,
    running: Mutex<HashSet<String>>,
    finished: Mutex<Vec<(String, Result<(), String>)>>,
    opened: Mutex<Vec<String>>,
}

struct FakeHost(Arc<Fake>);

impl Host for FakeHost {
    fn headless(&self) -> bool {
        !self.0.window
    }
    fn begin(&self, key: &str) -> Option<Guard> {
        self.0.busy.begin(key)
    }
    fn is_busy(&self, key: &str) -> bool {
        self.0.busy.is_busy(key)
    }
    fn game_running(&self, g: &GameEntry) -> bool {
        self.0.running.lock().unwrap().contains(&g.key)
    }
    fn finished(&self, g: &GameEntry, result: &anyhow::Result<Outcome>) {
        let r = result.as_ref().map(|_| ()).map_err(|e| format!("{e:#}"));
        self.0.finished.lock().unwrap().push((g.key.clone(), r));
    }
    fn open_game(&self, key: &str) -> bool {
        if self.0.window {
            self.0.opened.lock().unwrap().push(key.to_string());
        }
        self.0.window
    }
    fn game_backup_status(&self, key: &str) -> Option<anyhow::Result<Value>> {
        self.0.window.then(|| Ok(json!({ "exeMissing": false, "checked": true, "latest": null, "job": null, "key": key })))
    }
    fn game_backup_install(&self, _key: &str) -> Option<anyhow::Result<Value>> {
        self.0.window.then(|| Ok(json!({ "ok": true })))
    }
}

fn serve(window: bool, options: Options) -> (localapi::Server, Arc<Fake>, Api) {
    root();
    let fake = Arc::new(Fake { window, ..Default::default() });
    let server = localapi::start_with(FakeHost(fake.clone()), options).unwrap();
    let api = Api { port: server.port(), token: server.info().token };
    (server, fake, api)
}

/// A client like the plugins' backends: one request per connection.
struct Api {
    port: u16,
    token: String,
}

impl Api {
    /// Send `raw` as it is and read the answer to the end: (status, JSON body).
    fn raw(&self, raw: &[u8]) -> std::io::Result<(u16, Value)> {
        let mut tcp = TcpStream::connect(("127.0.0.1", self.port))?;
        tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
        tcp.write_all(raw)?;
        let mut answer = Vec::new();
        if let Err(e) = tcp.read_to_end(&mut answer) {
            if answer.is_empty() {
                return Err(e);
            }
        }
        let end = answer.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| std::io::Error::other("no answer"))?;
        let head = String::from_utf8_lossy(&answer[..end]).to_string();
        let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| std::io::Error::other(head.clone()))?;
        assert!(head.to_ascii_lowercase().contains("content-type: application/json"), "{head}");
        Ok((status, serde_json::from_slice(&answer[end + 4..]).unwrap_or(Value::Null)))
    }

    fn request(&self, method: &str, path: &str, extra: &str) -> (u16, Value) {
        let body = if method == "POST" { "Content-Length: 0\r\n" } else { "" };
        let raw = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n{body}{extra}\r\n",
            self.port, self.token
        );
        self.raw(raw.as_bytes()).unwrap()
    }

    fn get(&self, path: &str) -> (u16, Value) {
        self.request("GET", path, "")
    }

    fn post(&self, path: &str) -> (u16, Value) {
        self.request("POST", path, "")
    }
}

/// Wait for the `n`th background job the API started to end.
fn finished(fake: &Fake, n: usize) -> (String, Result<(), String>) {
    let started = Instant::now();
    loop {
        if let Some(f) = fake.finished.lock().unwrap().get(n - 1).cloned() {
            // The game is no longer busy once the job's thread has ended.
            while fake.busy.is_busy(&f.0) {
                std::thread::sleep(Duration::from_millis(20));
            }
            return f;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "the job didn't end");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A one-pixel PNG's first bytes: enough for a `data:image/png` URL.
fn png(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"\x89PNG\r\n\x1a\n0000").unwrap();
    path.to_path_buf()
}

fn library(root: &Path) -> PathBuf {
    let game_dir = root.join("game");
    std::fs::create_dir_all(game_dir.join("save")).unwrap();
    std::fs::write(game_dir.join("game.exe"), b"exe").unwrap();
    std::fs::write(game_dir.join("save").join("slot1.sav"), b"v1").unwrap();
    std::fs::create_dir_all(root.join("cloud")).unwrap();
    std::fs::create_dir_all(root.join("steam")).unwrap();
    let mut cfg = AppConfig::default();
    cfg.device.name = "PC-A".into();
    cfg.cloud = CloudConfig::LocalFolder { path: root.join("cloud") };
    // An empty Steam folder: the real one is never read.
    cfg.steam.root = Some(root.join("steam"));
    cfg.games.push(GameEntry {
        key: KEY.into(),
        name: "Test Game".into(),
        exe: Some(game_dir.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        auto_sync: true,
        steam: Some(SteamLink { account: "123".into(), appid: APPID, enabled: true, ..Default::default() }),
        ..Default::default()
    });
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
    game_dir
}

#[test]
fn endpoints() {
    let game_dir = library(root());
    let (_server, fake, api) = serve(true, Options::default());
    let unknown = "/v1/games/no-such-game";

    // Ping.
    let (s, v) = api.get("/v1/ping");
    assert_eq!((s, v["app"].as_str(), v["apiVersion"].as_u64(), v["headless"].as_bool()), (200, Some("ludonimbus"), Some(5), Some(false)));

    // Status of the shortcut: cached until asked to refresh.
    let (s, v) = api.get(&format!("/v1/steam/apps/{APPID}"));
    assert_eq!((s, v["managed"].as_bool(), v["key"].as_str(), v["phase"].as_str()), (200, Some(true), Some(KEY), Some("idle")));
    assert_eq!((v["status"].clone(), v["apiVersion"].as_u64(), v["steamEnabled"].as_bool()), (Value::Null, Some(5), Some(true)));
    let (s, v) = api.get(&format!("/v1/steam/apps/{APPID}?account=123&refresh=1"));
    assert_eq!((s, v["status"].as_str()), (200, Some("notUploaded")));
    assert!(v["checkedAt"].is_string());
    assert_eq!(api.get("/v1/steam/apps/42").1, json!({ "managed": false }));
    assert_eq!(api.get("/v1/steam/apps/abc").0, 400);
    // What the game is doing.
    fake.running.lock().unwrap().insert(KEY.into());
    assert_eq!(api.get(&format!("/v1/steam/apps/{APPID}")).1["phase"], "playing");
    fake.running.lock().unwrap().clear();
    let held = fake.busy.begin(KEY).unwrap();
    assert_eq!(api.get(&format!("/v1/steam/apps/{APPID}")).1["phase"], "syncing");

    // Sync now: not while the app syncs it or it runs; the resolution is checked.
    assert_eq!(api.post(&format!("/v1/games/{KEY}/sync")).0, 409);
    drop(held);
    fake.running.lock().unwrap().insert(KEY.into());
    assert_eq!(api.post(&format!("/v1/games/{KEY}/sync")).0, 409);
    fake.running.lock().unwrap().clear();
    assert_eq!(api.post(&format!("/v1/games/{KEY}/sync?resolution=sideways")).0, 400);
    assert_eq!(api.post(&format!("{unknown}/sync")).0, 404);
    assert_eq!(api.get(&format!("/v1/games/{KEY}/sync")).0, 404, "only POST");
    let (s, v) = api.post(&format!("/v1/games/{KEY}/sync"));
    assert_eq!((s, v), (200, json!({ "started": true })));
    assert_eq!(finished(&fake, 1), (KEY.to_string(), Ok(())));

    // The version history.
    let (s, v) = api.get(&format!("/v1/games/{KEY}/snapshots"));
    assert_eq!(s, 200, "{v}");
    let list = v["snapshots"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    let id = list[0]["id"].as_str().unwrap().to_string();
    assert_eq!((v["headId"].as_str(), v["lastSyncedId"].as_str(), v["busy"].clone()), (Some(id.as_str()), Some(id.as_str()), Value::Null));
    fake.running.lock().unwrap().insert(KEY.into());
    assert!(api.get(&format!("/v1/games/{KEY}/snapshots")).1["busy"].is_string());
    fake.running.lock().unwrap().clear();
    assert_eq!(api.get(&format!("{unknown}/snapshots")).0, 404);
    assert_eq!(api.get(&format!("/v1/steam/apps/{APPID}?refresh=1")).1["status"], "synced");

    // Restore: the id is checked; a failure is the game's lastError until
    // something works again.
    assert_eq!(api.post(&format!("/v1/games/{KEY}/restore/not-an-id")).0, 400);
    assert_eq!(api.post(&format!("{unknown}/restore/{id}")).0, 404);
    fake.running.lock().unwrap().insert(KEY.into());
    assert_eq!(api.post(&format!("/v1/games/{KEY}/restore/{id}")).0, 409);
    fake.running.lock().unwrap().clear();
    assert_eq!(api.post(&format!("/v1/games/{KEY}/restore/01NOSUCHVERSION")).0, 200);
    assert!(finished(&fake, 2).1.is_err());
    assert!(state::game(KEY).last_error.is_some());
    assert!(api.get(&format!("/v1/steam/apps/{APPID}")).1["lastError"].is_string());
    std::fs::write(game_dir.join("save").join("slot1.sav"), b"v2").unwrap();
    assert_eq!(api.post(&format!("/v1/games/{KEY}/restore/{id}")).0, 200);
    assert_eq!(finished(&fake, 3).1, Ok(()));
    assert_eq!(std::fs::read(game_dir.join("save").join("slot1.sav")).unwrap(), b"v1");
    assert_eq!(api.get(&format!("/v1/steam/apps/{APPID}")).1["lastError"], Value::Null);

    // How a result is noted: a queued version or an unreachable cloud keeps
    // what the sync noted, a result with nothing left to do clears it.
    state::update_game(KEY, |s| s.last_error = Some("offline".into())).unwrap();
    localapi::note_result(KEY, &Ok(Outcome::Offline { message: "offline".into() }));
    assert_eq!(state::game(KEY).last_error.as_deref(), Some("offline"));
    localapi::note_result(KEY, &Ok(Outcome::UpToDate));
    assert_eq!(state::game(KEY).last_error, None);
    localapi::note_result(KEY, &Err(anyhow::anyhow!("broken")));
    assert_eq!(state::game(KEY).last_error.as_deref(), Some("broken"));
    localapi::note_result(KEY, &Ok(Outcome::UpToDate));

    // Playtime.
    let (s, v) = api.get(&format!("/v1/games/{KEY}/playtime"));
    assert_eq!((s, v["totalSecs"].as_u64()), (200, Some(0)));
    assert_eq!(api.get(&format!("{unknown}/playtime")).0, 404);
    assert_eq!(api.get("/v1/steam/playtime?account=123"), (200, json!({ "apps": [] })));

    // Newer saves in another version: there is none.
    assert_eq!(api.get(&format!("/v1/games/{KEY}/newer")), (200, Value::Null));
    assert_eq!(api.get(&format!("{unknown}/newer")).0, 404);
    assert_eq!(api.post(&format!("/v1/games/{KEY}/newer/a.b/apply")).0, 400);
    assert_eq!(api.post(&format!("/v1/games/{KEY}/newer/{id}/apply")).0, 404);
    assert_eq!(api.post(&format!("{unknown}/newer/{id}/apply")).0, 404);
    assert_eq!(api.post(&format!("/v1/games/{KEY}/newer/a%2Fb/dismiss")).0, 400);
    assert_eq!(api.post(&format!("{unknown}/newer/{id}/dismiss")).0, 404);
    assert!(!state::load().games.contains_key("no-such-game"), "nothing noted for a game that isn't there");
    assert_eq!(api.post(&format!("/v1/games/{KEY}/newer/{id}/dismiss")), (200, json!({ "ok": true })));
    assert_eq!(state::game(KEY).newer_dismissed.as_deref(), Some(id.as_str()));

    // The window and game backups (the desktop app).
    assert_eq!(api.post(&format!("/v1/games/{KEY}/open")), (200, json!({ "ok": true })));
    assert_eq!(*fake.opened.lock().unwrap(), vec![KEY.to_string()]);
    assert_eq!(api.post(&format!("{unknown}/open")).0, 404);
    let (s, v) = api.get(&format!("/v1/games/{KEY}/gamebackup"));
    assert_eq!((s, v["exeMissing"].as_bool(), v["key"].as_str()), (200, Some(false), Some(KEY)));
    assert_eq!(api.get(&format!("{unknown}/gamebackup")).0, 404);
    assert_eq!(api.post(&format!("/v1/games/{KEY}/gamebackup/install")), (200, json!({ "ok": true })));

    // Achievements of the game page, and icons.
    let (s, v) = api.get(&format!("/v1/steam/apps/{APPID}/achievements?account=123"));
    assert_eq!((s, v["managed"].as_bool(), v["name"].as_str()), (200, Some(true), Some("Test Game")));
    assert_eq!(api.get("/v1/steam/apps/42/achievements").1, json!({ "managed": false }));
    assert_eq!(api.get(&format!("/v1/games/{KEY}/achievements/a1/icon")).0, 404, "no icon yet");
    assert_eq!(api.get(&format!("/v1/games/{KEY}/icon")).0, 404, "no icon yet");
    let icon = png(&root().join("icons").join("a1.png"));
    ach::update_db(KEY, |db| {
        let mut a = ach::placeholder("a1");
        a.icon = Some(icon.to_string_lossy().to_string());
        db.schema = Some(Schema { source: SourceKind::Rules, source_id: KEY.into(), title: None, achievements: vec![a] });
    })
    .unwrap();
    png(&ludonimbus_core::paths::data_dir().join("art").join(KEY).join("icon.png"));
    let (s, v) = api.get(&format!("/v1/games/{KEY}/achievements/a1/icon"));
    assert_eq!(s, 200, "{v}");
    assert!(v["dataUrl"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert!(api.get(&format!("/v1/games/{KEY}/icon")).1["dataUrl"].as_str().unwrap().starts_with("data:image/png"));
    assert_eq!(api.get(&format!("{unknown}/achievements/a1/icon")).0, 404);
    assert_eq!(api.get(&format!("{unknown}/icon")).0, 404);
    assert_eq!(api.get(&format!("/v1/games/{KEY}/achievements/{}/icon", "x".repeat(300))).0, 400);

    // New unlocks for the plugin's popup: a plugin that (re)loads starts
    // from the current number; what it shows is told back.
    let (s, v) = api.get("/v1/achievements/events?after=0");
    assert_eq!((s, v["events"].as_array().map(Vec::len)), (200, Some(0)));
    let seq = v["seq"].as_u64().unwrap();
    assert!(seq > 1_000_000_000_000, "numbers continue across restarts");
    let g = config::load().unwrap().game(KEY).cloned().unwrap();
    let a: Achievement = ach::placeholder("a1");
    let ev = events::push(&g, &NewUnlock { key: KEY.into(), achievement: a, unlocked_at: chrono::Utc::now(), hardcore: false }, false);
    let (s, v) = api.get(&format!("/v1/achievements/events?after={seq}"));
    assert_eq!((s, v["events"][0]["id"].as_str(), v["events"][0]["appid"].as_u64()), (200, Some("a1"), Some(APPID as u64)));
    assert!(v["events"][0]["icon"].as_str().unwrap().starts_with("data:image/png"));
    assert!(events::plugin_alive());
    assert_eq!(api.get("/v1/achievements/events?after=soon").0, 400);
    assert_eq!(api.post(&format!("/v1/achievements/events/{}/ack?shown=steam&reason=overlay%20open", ev.seq)), (200, json!({ "ok": true })));
    assert_eq!(events::shown(ev.seq).as_deref(), Some("steam"));
    assert_eq!(api.post("/v1/achievements/events/x/ack").0, 400);
    assert_eq!(api.post(&format!("/v1/achievements/events/{}/ack?shown=maybe", ev.seq)).0, 400);

    // Nothing else.
    assert_eq!(api.get("/v1/nothing").0, 404);
    assert_eq!(api.request("DELETE", &format!("/v1/games/{KEY}/snapshots"), "").0, 404);

    // Without a window (ludonimbus-agent): the same, minus what needs it.
    let (_headless, _, bare) = serve(false, Options::default());
    assert_eq!(bare.get("/v1/ping").1["headless"], true);
    assert_eq!(bare.get(&format!("/v1/steam/apps/{APPID}")).1["managed"], true);
    assert_eq!(bare.get(&format!("/v1/steam/apps/{APPID}/achievements")).1["managed"], true);
    assert_eq!(bare.get("/v1/achievements/events").0, 200);
    for (method, path) in [("POST", "open"), ("GET", "gamebackup"), ("POST", "gamebackup/install")] {
        let (s, v) = bare.request(method, &format!("/v1/games/{KEY}/{path}"), "");
        assert_eq!(s, 404, "{path}");
        assert!(v["error"].is_string());
    }
}

#[test]
fn refusals() {
    let (_server, _, api) = serve(false, Options { max_connections: 4, io_timeout: Duration::from_secs(2) });
    let port = api.port;
    let ping = |headers: &str| api.raw(format!("GET /v1/ping HTTP/1.1\r\n{headers}\r\n").as_bytes()).unwrap().0;
    let auth = format!("Authorization: Bearer {}\r\n", api.token);
    let host = format!("Host: 127.0.0.1:{port}\r\n");

    assert_eq!(ping(&format!("{host}{auth}")), 200);
    assert_eq!(ping(&format!("Host: localhost:{port}\r\n{auth}")), 200);
    // The token.
    assert_eq!(ping(&host), 401);
    assert_eq!(ping(&format!("{host}Authorization: Bearer nope\r\n")), 401);
    assert_eq!(ping(&format!("{host}Authorization: bearer {}\r\n", api.token)), 401);
    assert_eq!(ping(&format!("{host}Authorization: Bearer {}0\r\n", api.token)), 401);
    // Web pages and DNS rebinding.
    assert_eq!(ping(&format!("{host}{auth}Origin: https://example.com\r\n")), 403);
    assert_eq!(ping(&format!("{host}{auth}Origin: null\r\n")), 403);
    assert_eq!(ping(&format!("Host: evil.example:{port}\r\n{auth}")), 403);
    assert_eq!(ping(&format!("Host: 127.0.0.1\r\n{auth}")), 403);
    assert_eq!(ping(&auth), 403, "no Host");

    // Malformed and oversized requests.
    let send = |raw: &str| api.raw(raw.as_bytes()).map(|(s, _)| s);
    assert_eq!(send("GARBAGE\r\n\r\n").unwrap(), 400);
    assert_eq!(send(&format!("GET /v1/ping HTTP/1.1\r\n{host}{auth}Content-Length: {}\r\n\r\n", 1 << 20)).unwrap(), 413);
    assert_eq!(send(&format!("POST /v1/games/x/sync HTTP/1.1\r\n{host}{auth}Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n")).unwrap(), 411);
    let endless = format!("GET /v1/ping HTTP/1.1\r\n{host}X-Long: {}\r\n\r\n", "a".repeat(64 * 1024));
    assert!(send(&endless).map_or(true, |s| s == 431));
    // A small body is read and ignored.
    assert_eq!(send(&format!("POST /v1/achievements/events/1/ack HTTP/1.1\r\n{host}{auth}Content-Length: 2\r\n\r\n{{}}")).unwrap(), 200);

    // A client that sends nothing (or too slowly) is let go.
    let mut idle = TcpStream::connect(("127.0.0.1", port)).unwrap();
    idle.write_all(b"GET /v1/ping HTTP/1.1\r\n").unwrap();
    assert!(closed_within(&mut idle, Duration::from_secs(6)), "a slow client is closed");
}

/// Whether the server closes this connection before `within` passes.
fn closed_within(tcp: &mut TcpStream, within: Duration) -> bool {
    tcp.set_read_timeout(Some(within)).unwrap();
    let mut buf = [0u8; 256];
    loop {
        match tcp.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(e) => return !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    }
}

#[test]
fn connection_limit() {
    let timeout = Duration::from_secs(1);
    let (server, _, api) = serve(false, Options { max_connections: 2, io_timeout: timeout });
    assert_eq!(api.get("/v1/ping").0, 200);

    // Two connections that send nothing take both places: the next request
    // waits until they are let go, then is answered.
    let mut held: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(("127.0.0.1", api.port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    assert_eq!(api.get("/v1/ping").0, 200);
    assert!(started.elapsed() >= timeout / 2, "answered while both places were taken ({:?})", started.elapsed());
    for tcp in &mut held {
        assert!(closed_within(tcp, timeout * 5), "an idle connection is closed");
    }

    // Many at once are all answered, a few at a time.
    let answers: Vec<u16> = std::thread::scope(|s| {
        let all: Vec<_> = (0..12).map(|_| s.spawn(|| api.get("/v1/ping").0)).collect();
        all.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(answers.iter().all(|&s| s == 200), "{answers:?}");

    // Stopped: the port is let go.
    let port = api.port;
    server.stop();
    let started = Instant::now();
    while TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).is_ok() {
        assert!(started.elapsed() < Duration::from_secs(10), "still listening after stop");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn agent_file() {
    let (server, _, _) = serve(false, Options::default());
    let path = localapi::agent_file();
    assert!(path.starts_with(root()));
    server.publish().unwrap();
    let info = localapi::read_agent_file().unwrap();
    assert_eq!((info.port, info.token.len(), info.pid, info.api_version, info.headless), (server.port(), 64, std::process::id(), 5, true));
    let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(raw["apiVersion"].is_u64() && raw["token"].is_string(), "the plugins read camelCase: {raw}");
    assert_eq!(localapi::live_owner(), Some(std::process::id()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "only this user reads the token");
    }
    // Another server's token differs.
    let (other, _, _) = serve(false, Options::default());
    assert_ne!(other.info().token, info.token);

    // Ours: removed on exit.
    localapi::remove_agent_file();
    assert!(!path.exists());
    // Someone else's (an app from before `headless`, long gone): left alone,
    // and nobody owns it.
    let old = json!({ "port": 1, "token": "t", "pid": u32::MAX - 7, "version": "0.7.0", "apiVersion": 3 });
    std::fs::write(&path, old.to_string()).unwrap();
    assert!(!localapi::read_agent_file().unwrap().headless);
    assert_eq!(localapi::live_owner(), None);
    localapi::remove_agent_file();
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
}
