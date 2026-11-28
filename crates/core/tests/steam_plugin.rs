//! The Steam plugins as Steam's hands (local API 5,
//! `ludonimbus_core::localapi::plugin`): a plugin polls, takes what the app
//! asks of Steam and answers with what Steam says; what it reports counts
//! for the Steam Cloud recheck while it keeps polling, and Steam's files
//! count otherwise. Spoken to over TCP the way the plugins' backends do, with
//! a temporary data folder and a fake Steam folder. Never real Steam files.
//! One test, because the data folder is chosen through an environment
//! variable.

use ludonimbus_core::config::{self, AppConfig, GameEntry, StoreLink, StoreMode};
use ludonimbus_core::localapi::plugin::{self, Caps, Want};
use ludonimbus_core::localapi::{self, Busy, Guard, Host};
use ludonimbus_core::{steam, steamcloud};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const ACCOUNT: &str = "111111111";
const APPID: u32 = 1000;
const KEY: &str = "fake-game";
/// Backup only, for the take-over refusals.
const BACKED: u32 = 2000;
const BACKED_KEY: &str = "backed-game";

/// The game's own Steam Cloud switch, off, in `sharedconfig.vdf`.
const SHARED_GAME_OFF: &str = "\"UserRoamingConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"1000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"CloudEnabled\"\t\t\"0\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

struct Headless(Busy);

impl Host for Headless {
    fn begin(&self, key: &str) -> Option<Guard> {
        self.0.begin(key)
    }
    fn is_busy(&self, key: &str) -> bool {
        self.0.is_busy(key)
    }
}

/// A client like the plugins' backends: one request per connection.
#[derive(Clone)]
struct Api {
    port: u16,
    token: String,
}

impl Api {
    fn request(&self, method: &str, path: &str) -> (u16, Value) {
        let mut tcp = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let body = if method == "POST" { "Content-Length: 0\r\n" } else { "" };
        let raw = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n{body}\r\n", self.port, self.token);
        tcp.write_all(raw.as_bytes()).unwrap();
        let mut answer = Vec::new();
        let _ = tcp.read_to_end(&mut answer);
        let end = answer.windows(4).position(|w| w == b"\r\n\r\n").expect("an answer");
        let head = String::from_utf8_lossy(&answer[..end]).to_string();
        let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap();
        (status, serde_json::from_slice(&answer[end + 4..]).unwrap_or(Value::Null))
    }

    fn poll(&self, session: &str, account: &str) -> Value {
        let (s, v) =
            self.request("GET", &format!("/v1/steam/plugin/poll?session={session}&kind=test&version=1.0.0&caps=cloud%2ClaunchOptions&account={account}"));
        assert_eq!(s, 200, "{v}");
        v
    }
}

/// Percent-encoding of everything but the unreserved characters, as the
/// plugins send query values.
fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A plugin that keeps polling as `session` until it gets a request, then
/// answers it with what `answer` makes of it (the answer's query). Returns
/// the request.
fn plugin_answers(api: &Api, session: &'static str, answer: impl Fn(&Value) -> String + Send + 'static) -> JoinHandle<Value> {
    let api = api.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        loop {
            let v = api.poll(session, ACCOUNT);
            if let Some(req) = v["requests"].as_array().and_then(|a| a.first()).cloned() {
                let (s, _) = api.request("POST", &format!("/v1/steam/plugin/done/{}?session={session}&{}", req["id"], answer(&req)));
                assert_eq!(s, 200);
                return req;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "no request came");
            std::thread::sleep(Duration::from_millis(100));
        }
    })
}

fn library(root: &Path) {
    let steam_root = root.join("steam");
    std::fs::create_dir_all(steam_root.join("userdata").join(ACCOUNT)).unwrap();
    let mut cfg = AppConfig::default();
    cfg.device.name = "PC".into();
    cfg.language = "en".into();
    cfg.steam.root = Some(steam_root);
    cfg.steam.account = Some(ACCOUNT.into());
    let game = |key: &str, appid: u32, mode: StoreMode| GameEntry {
        key: key.into(),
        name: key.into(),
        saves: vec!["{steamuser}/x/**".into()],
        auto_sync: true,
        store: Some(StoreLink { account: ACCOUNT.into(), appid, mode, ..Default::default() }),
        ..Default::default()
    };
    cfg.games.push(game(KEY, APPID, StoreMode::Managed));
    cfg.games.push(game(BACKED_KEY, BACKED, StoreMode::Backup));
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
}

fn mode(key: &str) -> StoreMode {
    config::load().unwrap().game(key).unwrap().store.as_ref().unwrap().mode
}

fn set_mode(key: &str, mode: StoreMode) {
    config::update(|c| {
        c.game_mut(key).unwrap().store.as_mut().unwrap().mode = mode;
        Ok(())
    })
    .unwrap();
}

#[test]
fn steam_through_the_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    library(tmp.path());
    let server = localapi::start(Headless(Busy::default())).unwrap();
    let api = Api { port: server.port(), token: server.info().token };

    exchange(&api);
    recheck_precedence(&api, tmp.path());
}

/// Requests wait for a plugin's poll, go only to a plugin of their account,
/// and come back with what Steam says; the plugin's answers and reports are
/// written down for other programs.
fn exchange(api: &Api) {
    // No plugin yet (an older plugin never polls): nothing can be asked, and
    // a game can't be handed over from Steam.
    assert_eq!(plugin::connected(ACCOUNT), None);
    let (s, v) = api.request("POST", &format!("/v1/games/{BACKED_KEY}/steam-cloud/take-over"));
    assert!(s == 409 && v["error"].as_str().unwrap().contains("plugin"), "{v}");

    // A plugin polls: it is connected, learns the games to report, and is
    // told to send its reports (this program hasn't heard from it before).
    let v = api.poll("s1", ACCOUNT);
    assert_eq!(v["resend"].as_bool(), Some(true));
    assert_eq!(v["apiVersion"].as_u64(), Some(6));
    let mut watch: Vec<u64> = v["watch"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    watch.sort();
    assert_eq!(watch, [APPID as u64, BACKED as u64]);
    assert_eq!(v["requests"].as_array().map(Vec::len), Some(0));
    assert_eq!(api.poll("s1", ACCOUNT)["resend"].as_bool(), Some(false));
    assert_eq!(plugin::connected(ACCOUNT), Some(Caps { cloud: true, launch_options: true }));
    // Signed in to another account, it can't do anything for this one.
    assert_eq!(plugin::connected("222222222"), None);
    // Bad polls.
    assert_eq!(api.request("GET", "/v1/steam/plugin/poll?session=a%2Fb").0, 400);
    assert_eq!(api.request("GET", "/v1/steam/plugin/poll").0, 400);

    // A game that isn't backup only isn't handed over.
    let (s, v) = api.request("POST", &format!("/v1/games/{KEY}/steam-cloud/take-over"));
    assert!(s == 409 && v["error"].as_str().unwrap().contains("backup only"), "{v}");

    // A report on its own.
    let (s, _) = api.request(
        "POST",
        &format!("/v1/steam/apps/{APPID}/report?session=s1&account={ACCOUNT}&available=1&accountEnabled=1&appEnabled=1&status=3&launchOptions=-novid"),
    );
    assert_eq!(s, 200);
    let r = plugin::fresh(ACCOUNT, APPID).expect("a fresh report");
    assert!(r.on());
    assert_eq!((r.synced(), r.launch_options.as_deref()), (Some(true), Some("-novid")));
    assert_eq!(api.request("POST", &format!("/v1/steam/apps/{APPID}/report?session=s1&account={ACCOUNT}&available=1")).0, 400);
    assert_eq!(api.request("POST", &format!("/v1/steam/apps/{APPID}/report?session=s1&available=1&accountEnabled=1&appEnabled=1")).0, 400);

    // Turning Steam Cloud off: a plugin of another account doesn't get the
    // request; this account's plugin does, and answers with Steam's new state.
    let asking = std::thread::spawn(|| plugin::ask(ACCOUNT, APPID, Want::Cloud(false)));
    std::thread::sleep(Duration::from_millis(100));
    for _ in 0..5 {
        let v = api.poll("s-other", "222222222");
        assert_eq!(v["requests"].as_array().map(Vec::len), Some(0), "{v}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let plugin = plugin_answers(api, "s1", |_| "ok=1&available=1&accountEnabled=1&appEnabled=0&status=3&launchOptions=-novid".into());
    let after = asking.join().unwrap().unwrap();
    let req = plugin.join().unwrap();
    assert_eq!((req["kind"].as_str(), req["on"].as_bool(), req["appid"].as_u64()), (Some("cloud"), Some(false), Some(APPID as u64)));
    assert!(!after.on() && after.available && !after.app_enabled);
    // Written down: other programs (the launch wrapper) see Steam Cloud off.
    assert!(!plugin::fresh(ACCOUNT, APPID).unwrap().on());

    // Launch options travel both ways intact: quotes, spaces, backslashes,
    // `%`, letters beyond ASCII.
    let options = r#""C:\Program Files\Ludonimbus\ludonimbus-run.exe" --game fake-game -- %command% -lang=日本語 +x"#;
    let plugin =
        plugin_answers(api, "s1", |req| format!("ok=1&available=1&accountEnabled=1&appEnabled=0&launchOptions={}", enc(req["value"].as_str().unwrap())));
    let after = plugin::ask(ACCOUNT, APPID, Want::LaunchOptions(options.into())).unwrap();
    let req = plugin.join().unwrap();
    assert_eq!((req["kind"].as_str(), req["value"].as_str()), (Some("launchOptions"), Some(options)));
    assert_eq!(after.launch_options.as_deref(), Some(options));
    assert_eq!(plugin::fresh(ACCOUNT, APPID).unwrap().launch_options.as_deref(), Some(options));

    // A plugin that couldn't do it says why.
    let plugin = plugin_answers(api, "s1", |_| "ok=0&error=Steam%20said%20no&available=1&accountEnabled=1&appEnabled=1".into());
    let err = format!("{:#}", plugin::ask(ACCOUNT, APPID, Want::Cloud(false)).unwrap_err());
    plugin.join().unwrap();
    assert!(err.contains("Steam said no"), "{err}");
    // An answer to a request nobody waits for any more is ignored.
    assert_eq!(api.request("POST", "/v1/steam/plugin/done/999999?session=s1&ok=1").0, 200);
    assert_eq!(api.request("POST", "/v1/steam/plugin/done/abc?session=s1&ok=1").0, 400);
}

/// The recheck before a sync: a fresh report from a plugin that still polls
/// counts first (it sees Steam's switch live), Steam's files otherwise; and
/// a game found with Steam Cloud on again goes back to backup only.
fn recheck_precedence(api: &Api, root: &Path) {
    let shared = steam::sharedconfig_path(&root.join("steam"), ACCOUNT);
    let report = |app_enabled: bool| {
        let q = format!("session=s1&account={ACCOUNT}&available=1&accountEnabled=1&appEnabled={}", if app_enabled { 1 } else { 0 });
        assert_eq!(api.request("POST", &format!("/v1/steam/apps/{APPID}/report?{q}")).0, 200);
    };
    let recheck = || steamcloud::recheck(&config::load().unwrap(), config::load().unwrap().game(KEY).unwrap()).unwrap();

    // Steam's files say the switch is on (no file says it's off), and Steam
    // itself says off, live: the plugin counts. Managed stays managed.
    set_mode(KEY, StoreMode::Managed);
    report(false);
    assert!(recheck().is_none());
    assert_eq!(mode(KEY), StoreMode::Managed);
    let status = steamcloud::status(&config::load().unwrap(), config::load().unwrap().game(KEY).unwrap()).unwrap();
    assert_eq!((status.cloud_enabled, status.plugin.map(|p| p.cloud)), (false, Some(true)));

    // Steam's files say off, and Steam says it is on again (the switch
    // turned itself back on): back to backup only.
    std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
    std::fs::write(&shared, SHARED_GAME_OFF).unwrap();
    report(true);
    let demoted = recheck().expect("Steam Cloud is on again");
    assert_eq!(demoted.store.unwrap().mode, StoreMode::Backup);
    assert_eq!(mode(KEY), StoreMode::Backup);

    // The plugin is gone (Steam closed): what it said last no longer counts,
    // Steam's files do. They say off: managed stays managed.
    set_mode(KEY, StoreMode::Managed);
    let file = localapi::agent_file().with_file_name("steam-plugin.json");
    let mut seen: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    for p in seen["plugins"].as_object_mut().unwrap().values_mut() {
        p["seenAt"] = Value::from("2020-01-01T00:00:00Z");
    }
    std::fs::write(&file, serde_json::to_vec(&seen).unwrap()).unwrap();
    assert!(plugin::fresh(ACCOUNT, APPID).is_none());
    assert!(recheck().is_none());
    assert_eq!(mode(KEY), StoreMode::Managed);
    // And when they say on, back to backup only.
    std::fs::remove_file(&shared).unwrap();
    assert!(recheck().is_some());
    assert_eq!(mode(KEY), StoreMode::Backup);
}
