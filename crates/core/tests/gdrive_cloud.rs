//! The Google Drive store against a stand-in for the Drive API (v3) that
//! keeps files by id, answers the searches the store makes, takes uploads
//! through sessions, and turns away requests whose access token it doesn't
//! know. Runs as one test because the folder cache is found through an
//! environment variable.

use ludonimbus_core::cloud;
use ludonimbus_core::cloud::gdrive::GoogleDrive;
use ludonimbus_core::cloud::oauth::GoogleClient;
use ludonimbus_core::cloud::store::Store;
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FOLDER: &str = "application/vnd.google-apps.folder";

struct File {
    id: String,
    name: String,
    parent: String,
    folder: bool,
    data: Vec<u8>,
    created: i64,
    modified: i64,
}

/// What an upload session was opened for: a new file, or new content for
/// one that is there.
enum Session {
    Create { name: String, parent: String, len: usize },
    Update { id: String, len: usize },
}

/// An answer given instead of the real one, to the next request whose
/// "METHOD path" starts with `matches`.
struct Trouble {
    matches: &'static str,
    status: u16,
    retry_after: Option<&'static str>,
    body: String,
}

#[derive(Default)]
struct Drive {
    base: String,
    files: Vec<File>,
    sessions: HashMap<String, Session>,
    tokens: HashSet<String>,
    clock: i64,
    trouble: VecDeque<Trouble>,
    /// "METHOD path" of every request.
    log: Vec<String>,
    /// For each request that carried an upload's bytes: its Content-Length,
    /// whether it was chunked, and how many bytes came.
    sent: Vec<(Option<String>, bool, usize)>,
}

fn md5hex(b: &[u8]) -> String {
    Md5::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn time(t: i64) -> String {
    chrono::DateTime::from_timestamp(1_790_000_000 + t, 0).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn error(code: u16, reason: &str) -> String {
    json!({ "error": { "code": code, "message": reason, "errors": [{ "reason": reason }] } }).to_string()
}

type Reply = (u16, Vec<(&'static str, String)>, Vec<u8>);

fn ok(v: Value) -> Reply {
    (200, vec![], v.to_string().into_bytes())
}

impl Drive {
    fn tick(&mut self) -> i64 {
        self.clock += 1;
        self.clock
    }

    fn json(f: &File) -> Value {
        let mut v = json!({
            "id": f.id, "name": f.name, "mimeType": if f.folder { FOLDER } else { "application/octet-stream" },
            "createdTime": time(f.created), "modifiedTime": time(f.modified),
        });
        if !f.folder {
            v["size"] = json!(f.data.len().to_string());
            v["md5Checksum"] = json!(md5hex(&f.data));
        }
        v
    }

    /// The searches the store makes: `name='…'`, `'…' in parents`,
    /// `mimeType='…'` and `trashed=false`, joined by `and`.
    fn search(&self, q: &str) -> Vec<Value> {
        let mut name = None;
        let mut parent = None;
        let mut folders_only = false;
        for clause in q.split(" and ") {
            let quoted = |s: &str| s.trim_matches('\'').replace("\\'", "'");
            if let Some(v) = clause.strip_prefix("name=") {
                name = Some(quoted(v));
            } else if let Some(v) = clause.strip_suffix(" in parents") {
                parent = Some(quoted(v));
            } else if clause == format!("mimeType='{FOLDER}'") {
                folders_only = true;
            } else {
                assert_eq!(clause, "trashed=false", "unexpected search {q}");
            }
        }
        let parent = parent.expect("searches are within a folder");
        self.files
            .iter()
            .filter(|f| f.parent == parent && name.as_ref().is_none_or(|n| &f.name == n) && (!folders_only || f.folder))
            .map(Self::json)
            .collect()
    }

    fn handle(&mut self, method: &str, url: &str, headers: &HashMap<String, String>, body: &[u8]) -> Reply {
        let parsed = url::Url::parse(&format!("http://drive{url}")).unwrap();
        let path = parsed.path().to_string();
        let q: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        self.log.push(format!("{method} {path}"));

        // The bytes of an upload go to the session's own address.
        if let Some(n) = path.strip_prefix("/session/") {
            assert_eq!(method, "PUT");
            self.sent.push((headers.get("content-length").cloned(), headers.get("transfer-encoding").is_some_and(|v| v.contains("chunked")), body.len()));
            let session = self.sessions.remove(n).expect("an open session");
            let now = self.tick();
            let id = match session {
                Session::Create { name, parent, len } => {
                    assert_eq!(body.len(), len, "the length given when the session was opened");
                    let id = format!("f{now}");
                    self.files.push(File { id: id.clone(), name, parent, folder: false, data: body.to_vec(), created: now, modified: now });
                    id
                }
                Session::Update { id, len } => {
                    assert_eq!(body.len(), len, "the length given when the session was opened");
                    let f = self.files.iter_mut().find(|f| f.id == id).expect("the file to update");
                    f.data = body.to_vec();
                    f.modified = now;
                    id
                }
            };
            return ok(json!({ "id": id, "trashed": false }));
        }

        let auth = headers.get("authorization").cloned().unwrap_or_default();
        if !auth.strip_prefix("Bearer ").is_some_and(|t| self.tokens.contains(t)) {
            return (401, vec![], error(401, "authError").into_bytes());
        }
        let line = format!("{method} {path}");
        if self.trouble.front().is_some_and(|t| line.starts_with(t.matches)) {
            let t = self.trouble.pop_front().unwrap();
            let headers = t.retry_after.map(|r| vec![("Retry-After", r.to_string())]).unwrap_or_default();
            return (t.status, headers, t.body.into_bytes());
        }

        let not_found = || (404, vec![], error(404, "notFound").into_bytes());
        match (method, path.as_str()) {
            ("GET", "/drive/v3/files") => ok(json!({ "files": self.search(&q["q"]) })),
            ("POST", "/drive/v3/files") => {
                let meta: Value = serde_json::from_slice(body).unwrap();
                assert_eq!(meta["mimeType"], FOLDER);
                let now = self.tick();
                let id = format!("d{now}");
                let parent = meta["parents"][0].as_str().unwrap().to_string();
                self.files.push(File { id: id.clone(), name: meta["name"].as_str().unwrap().into(), parent, folder: true, data: Vec::new(), created: now, modified: now });
                ok(json!({ "id": id }))
            }
            ("GET", "/drive/v3/about") => ok(json!({ "user": { "emailAddress": "player@example.com" }, "storageQuota": { "usage": "10", "limit": "100" } })),
            ("POST", "/upload/drive/v3/files") => {
                assert_eq!(q["uploadType"], "resumable");
                let meta: Value = serde_json::from_slice(body).unwrap();
                let len = headers["x-upload-content-length"].parse().unwrap();
                let n = self.tick().to_string();
                let session = Session::Create { name: meta["name"].as_str().unwrap().into(), parent: meta["parents"][0].as_str().unwrap().into(), len };
                self.sessions.insert(n.clone(), session);
                (200, vec![("Location", format!("{}/session/{n}", self.base))], Vec::new())
            }
            ("PATCH", p) if p.starts_with("/upload/drive/v3/files/") => {
                let id = p.rsplit('/').next().unwrap().to_string();
                let Some(i) = self.files.iter().position(|f| f.id == id) else { return not_found() };
                if q["uploadType"] == "resumable" {
                    let len = headers["x-upload-content-length"].parse().unwrap();
                    let n = self.tick().to_string();
                    self.sessions.insert(n.clone(), Session::Update { id, len });
                    return (200, vec![("Location", format!("{}/session/{n}", self.base))], Vec::new());
                }
                assert_eq!(q["uploadType"], "media");
                let now = self.tick();
                self.files[i].data = body.to_vec();
                self.files[i].modified = now;
                ok(json!({ "id": id, "trashed": false }))
            }
            ("GET", p) if p.starts_with("/drive/v3/files/") => {
                let id = p.rsplit('/').next().unwrap();
                let Some(f) = self.files.iter().find(|f| f.id == id) else { return not_found() };
                if q.get("alt").is_some_and(|a| a == "media") {
                    (200, vec![], f.data.clone())
                } else {
                    ok(json!({ "trashed": false }))
                }
            }
            ("DELETE", p) if p.starts_with("/drive/v3/files/") => {
                let id = p.rsplit('/').next().unwrap();
                let before = self.files.len();
                self.files.retain(|f| f.id != id);
                if self.files.len() == before {
                    not_found()
                } else {
                    (204, vec![], Vec::new())
                }
            }
            (m, p) => panic!("unexpected request {m} {p}"),
        }
    }

    fn named(&self, name: &str) -> usize {
        self.files.iter().filter(|f| f.name == name).count()
    }

    fn count(&self, line: &str) -> usize {
        self.log.iter().filter(|l| *l == line).count()
    }
}

fn start_drive() -> (String, Arc<Mutex<Drive>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
    let state = Arc::new(Mutex::new(Drive { base: base.clone(), ..Drive::default() }));
    let st = state.clone();
    std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let mut body = Vec::new();
            req.as_reader().read_to_end(&mut body).unwrap();
            let headers: HashMap<String, String> = req.headers().iter().map(|h| (h.field.as_str().as_str().to_ascii_lowercase(), h.value.as_str().to_string())).collect();
            let (status, extra, out) = st.lock().unwrap().handle(req.method().as_str(), req.url(), &headers, &body);
            let mut resp = tiny_http::Response::from_data(out).with_status_code(status);
            for (k, v) in extra {
                resp.add_header(tiny_http::Header::from_bytes(k, v).unwrap());
            }
            let _ = req.respond(resp);
        }
    });
    (base, state)
}

#[test]
fn google_drive() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let (base, drive) = start_drive();
    let renewals = Arc::new(AtomicUsize::new(0));
    let client = GoogleClient { id: "client-id".into(), secret: String::new() };
    let store = GoogleDrive::new(client, "refresh-token".into(), "Ludonimbus".into(), Duration::from_secs(10)).with_servers(
        &format!("{base}/drive/v3"),
        &format!("{base}/upload/drive/v3"),
        {
            let (drive, renewals) = (drive.clone(), renewals.clone());
            move |_| {
                let tok = format!("tok-{}", renewals.fetch_add(1, Ordering::SeqCst) + 1);
                drive.lock().unwrap().tokens.insert(tok.clone());
                Ok((tok, 3600))
            }
        },
    );
    let renewed = || renewals.load(Ordering::SeqCst);

    // Folders are made on the way; a file written is read back.
    store.write("games/fe/game.json", b"{\"v\":1}").unwrap();
    assert_eq!(store.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    assert_eq!(store.quota().unwrap(), (Some(10), Some(100)));
    assert_eq!(renewed(), 1);

    // Drive stops taking the token before this process thinks it has run
    // out (revoked, or the PC slept through its end): one 401, a new token,
    // and the call goes through.
    drive.lock().unwrap().tokens.clear();
    let listed: Vec<String> = store.list("games/fe").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(listed, ["game.json"]);
    assert_eq!(renewed(), 2);
    drive.lock().unwrap().tokens.clear();
    store.write("games/fe/game.json", b"{\"v\":2}").unwrap();
    assert_eq!(renewed(), 3);
    assert_eq!(store.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":2}"[..]));

    let trouble = |matches: &'static str, status: u16, retry_after: Option<&'static str>, body: String| {
        drive.lock().unwrap().trouble.push_back(Trouble { matches, status, retry_after, body });
    };
    let left = || drive.lock().unwrap().trouble.len();

    // Too many requests for a moment: sent again after the wait Drive asks for.
    trouble("", 429, Some("1"), error(429, "rateLimitExceeded"));
    let started = Instant::now();
    assert!(store.read("games/fe/game.json").unwrap().is_some());
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(left(), 0);
    // Drive's other way of saying it.
    trouble("", 403, None, error(403, "userRateLimitExceeded"));
    store.list("games/fe").unwrap();
    // Failing every time: three tries, then the cloud counts as unreachable
    // for now and the sync tries later.
    for _ in 0..4 {
        trouble("", 503, None, "<html>Service Unavailable</html>".into());
    }
    let err = store.list("games/fe").unwrap_err();
    assert!(cloud::is_unavailable(&err), "{err:#}");
    assert_eq!(left(), 1);
    drive.lock().unwrap().trouble.clear();
    // Asked to wait longer than is worth it here, or a limit that doesn't
    // pass by waiting: one try.
    trouble("", 503, Some("3600"), String::new());
    trouble("", 503, None, String::new());
    assert!(cloud::is_unavailable(&store.list("games/fe").unwrap_err()));
    assert_eq!(left(), 1);
    drive.lock().unwrap().trouble.clear();
    trouble("", 403, None, error(403, "dailyLimitExceeded"));
    trouble("", 403, None, error(403, "dailyLimitExceeded"));
    assert!(store.list("games/fe").is_err());
    assert_eq!(left(), 1);
    drive.lock().unwrap().trouble.clear();

    // A folder is never made twice: a lost answer would leave two.
    let made = drive.lock().unwrap().count("POST /drive/v3/files");
    trouble("POST /drive/v3/files", 503, None, String::new());
    assert!(cloud::is_unavailable(&store.write("games/zelda/game.json", b"{}").unwrap_err()));
    assert_eq!(drive.lock().unwrap().count("POST /drive/v3/files"), made + 1);
    store.write("games/zelda/game.json", b"{}").unwrap();
    assert_eq!(drive.lock().unwrap().named("zelda"), 1);

    // A big file goes up from disk through a session, with its length given
    // up front instead of being held in memory, and comes back the same.
    let big: Vec<u8> = (0..6_500_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmp.path().join("big.zip");
    std::fs::write(&src, &big).unwrap();
    store.upload("games/fe/packs/big.zip", &src).unwrap();
    let (length, chunked, n) = drive.lock().unwrap().sent.last().cloned().unwrap();
    assert_eq!((length.as_deref(), chunked, n), (Some("6500000"), false, big.len()));
    let back = tmp.path().join("back.zip");
    store.download("games/fe/packs/big.zip", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big);

    // Replaced by another big file: a session on the file that is there.
    let other: Vec<u8> = big.iter().rev().copied().collect();
    std::fs::write(&src, &other).unwrap();
    store.upload("games/fe/packs/big.zip", &src).unwrap();
    store.download("games/fe/packs/big.zip", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), other);
    assert_eq!(drive.lock().unwrap().named("big.zip"), 1);
    // A small one replaces it in a single request.
    std::fs::write(&src, b"small").unwrap();
    let sessions = drive.lock().unwrap().sent.len();
    store.upload("games/fe/packs/big.zip", &src).unwrap();
    assert_eq!(drive.lock().unwrap().sent.len(), sessions);
    assert_eq!(store.read("games/fe/packs/big.zip").unwrap().as_deref(), Some(&b"small"[..]));

    store.remove("games/fe/packs/big.zip").unwrap();
    assert_eq!(store.read("games/fe/packs/big.zip").unwrap(), None);
    assert_eq!(drive.lock().unwrap().named("big.zip"), 0);
}
