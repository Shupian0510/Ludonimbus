//! The Baidu Netdisk store against a mock of the xpan API that enforces the
//! documented rules: access tokens (and single-use refresh tokens), the
//! `/apps/<app name>` restriction, 4 MB slices with MD5 block lists,
//! `User-Agent: pan.baidu.com` for downloads, and no implicit parent folders.
//! Runs as one test because the cache folder is chosen through an
//! environment variable.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use ludonimbus_core::cloud::baidu::{BaiduClient, BaiduPan, Endpoints, TokenSink};
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::{self, migrate, repo::Repo, CloudProvider};
use ludonimbus_core::snapshot::{SnapshotMeta, Trigger};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const APP: &str = "/apps/Ludonimbus";
const SLICE: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct Upload {
    path: String,
    size: usize,
    blocks: Vec<String>,
    parts: BTreeMap<usize, Vec<u8>>,
}

#[derive(Default)]
struct Mock {
    base: String,
    files: BTreeMap<String, (Vec<u8>, u64, i64)>,
    dirs: BTreeSet<String>,
    uploads: HashMap<String, Upload>,
    access: BTreeSet<String>,
    refresh: String,
    next: u64,
    device_polls: usize,
    refreshes: usize,
}

fn md5hex(b: &[u8]) -> String {
    Md5::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn parent(p: &str) -> String {
    match p.rsplit_once('/') {
        Some(("", _)) => "/".into(),
        Some((d, _)) => d.into(),
        None => "/".into(),
    }
}

impl Mock {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn issue(&mut self) -> Value {
        let n = self.id();
        let (at, rt) = (format!("at-{n}"), format!("rt-{n}"));
        self.access.insert(at.clone());
        self.refresh = rt.clone();
        json!({ "access_token": at, "refresh_token": rt, "expires_in": 2592000, "scope": "basic netdisk" })
    }

    fn allowed(p: &str) -> bool {
        p == APP || p.starts_with(&format!("{APP}/"))
    }

    fn handle(&mut self, method: &str, path: &str, q: &HashMap<String, String>, form: &HashMap<String, String>, ua: &str, body: &[u8], ctype: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let ok = |v: Value| (200, vec![], v.to_string().into_bytes());
        let get = |k: &str| q.get(k).or_else(|| form.get(k)).cloned().unwrap_or_default();

        // OAuth --------------------------------------------------------------
        if path == "/oauth/2.0/device/code" {
            assert_eq!(get("client_id"), "app-key");
            assert_eq!(get("scope"), "basic,netdisk");
            return ok(json!({ "device_code": "dev-1", "user_code": "ABCD1234", "verification_url": format!("{}/device", self.base), "qrcode_url": format!("{}/qr.png", self.base), "expires_in": 300, "interval": 0 }));
        }
        if path == "/oauth/2.0/token" {
            assert_eq!(get("client_secret"), "secret-key");
            return match get("grant_type").as_str() {
                "device_token" => {
                    assert_eq!(get("code"), "dev-1");
                    self.device_polls += 1;
                    if self.device_polls < 3 {
                        (400, vec![], json!({"error": "authorization_pending", "error_description": "User has not yet completed the authorization"}).to_string().into_bytes())
                    } else {
                        ok(self.issue())
                    }
                }
                "refresh_token" => {
                    if get("refresh_token") != self.refresh {
                        return (400, vec![], json!({"error": "invalid_grant", "error_description": "refresh token has been used"}).to_string().into_bytes());
                    }
                    self.refreshes += 1;
                    ok(self.issue())
                }
                g => panic!("unexpected grant {g}"),
            };
        }

        // Downloads (dlink) -------------------------------------------------
        if let Some(id) = path.strip_prefix("/file/") {
            if ua != "pan.baidu.com" || !self.access.contains(&get("access_token")) {
                return (403, vec![], json!({"error_code": 31326, "error_msg": "anti hotlinking"}).to_string().into_bytes());
            }
            return (302, vec![("Location".into(), format!("{}/real/{id}", self.base))], vec![]);
        }
        if let Some(id) = path.strip_prefix("/real/") {
            let id: u64 = id.parse().unwrap();
            return match self.files.values().find(|f| f.1 == id) {
                Some(f) => (200, vec![], f.0.clone()),
                None => (404, vec![], vec![]),
            };
        }

        if !self.access.contains(&get("access_token")) {
            return ok(json!({"errno": -6, "errmsg": "access token invalid"}));
        }
        let m = get("method");
        match (path, m.as_str()) {
            ("/api/quota", _) => ok(json!({"errno": 0, "total": 2_199_023_255_552u64, "used": 1_073_741_824u64})),
            ("/rest/2.0/xpan/nas", "uinfo") => ok(json!({"errno": 0, "baidu_name": "player", "netdisk_name": "玩家", "vip_type": 0, "uk": 1})),
            ("/rest/2.0/xpan/file", "list") => {
                let dir = get("dir");
                if !Self::allowed(&dir) {
                    return ok(json!({"errno": -7}));
                }
                if !self.dirs.contains(&dir) {
                    return ok(json!({"errno": -9}));
                }
                let mut list = Vec::new();
                for d in self.dirs.iter().filter(|d| parent(d) == dir && **d != dir) {
                    list.push(json!({"path": d, "server_filename": d.rsplit('/').next(), "isdir": 1, "size": 0, "fs_id": 1, "server_mtime": 1}));
                }
                for (p, (data, id, mtime)) in self.files.iter().filter(|(p, _)| parent(p) == dir) {
                    list.push(json!({"path": p, "server_filename": p.rsplit('/').next(), "isdir": 0, "size": data.len(), "fs_id": id, "server_mtime": mtime, "md5": md5hex(data)}));
                }
                ok(json!({"errno": 0, "list": list}))
            }
            ("/rest/2.0/xpan/file", "create") if get("isdir") == "1" => {
                let p = get("path");
                if !Self::allowed(&p) {
                    return ok(json!({"errno": -7}));
                }
                if !self.dirs.contains(&parent(&p)) {
                    return ok(json!({"errno": -9}));
                }
                if !self.dirs.insert(p) {
                    return ok(json!({"errno": -8}));
                }
                ok(json!({"errno": 0}))
            }
            ("/rest/2.0/xpan/file", "precreate") => {
                let p = get("path");
                if !Self::allowed(&p) {
                    return ok(json!({"errno": 31064}));
                }
                if !self.dirs.contains(&parent(&p)) {
                    return ok(json!({"errno": -9}));
                }
                assert_eq!(get("autoinit"), "1");
                assert_eq!(get("rtype"), "3");
                let blocks: Vec<String> = serde_json::from_str(&get("block_list")).unwrap();
                let id = format!("up-{}", self.id());
                let n = blocks.len();
                self.uploads.insert(id.clone(), Upload { path: p, size: get("size").parse().unwrap(), blocks, parts: BTreeMap::new() });
                ok(json!({"errno": 0, "uploadid": id, "return_type": 1, "block_list": (0..n).collect::<Vec<_>>()}))
            }
            ("/rest/2.0/pcs/file", "locateupload") => ok(json!({"error_code": 0, "servers": [{"server": self.base.clone()}], "expire": 60})),
            ("/rest/2.0/pcs/superfile2", "upload") => {
                assert_eq!(method, "POST");
                let boundary = ctype.split("boundary=").nth(1).expect("multipart boundary").to_string();
                let start = body.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let end_marker = format!("\r\n--{boundary}--");
                let end = body.windows(end_marker.len()).rposition(|w| w == end_marker.as_bytes()).unwrap();
                let part = body[start..end].to_vec();
                let Some(up) = self.uploads.get_mut(&get("uploadid")) else {
                    return (400, vec![], json!({"error_code": 31355}).to_string().into_bytes());
                };
                assert_eq!(up.path, get("path"));
                let md5 = md5hex(&part);
                up.parts.insert(get("partseq").parse().unwrap(), part);
                ok(json!({"md5": md5, "request_id": 1}))
            }
            ("/rest/2.0/xpan/file", "create") => {
                let Some(up) = self.uploads.remove(&get("uploadid")) else { return ok(json!({"errno": 31355})) };
                assert_eq!(up.path, get("path"));
                assert_eq!(get("rtype"), "3");
                let blocks: Vec<String> = serde_json::from_str(&get("block_list")).unwrap();
                assert_eq!(blocks, up.blocks);
                if up.parts.len() != blocks.len() {
                    return ok(json!({"errno": 31363}));
                }
                let mut data = Vec::new();
                for (i, part) in up.parts.values().enumerate() {
                    assert_eq!(md5hex(part), blocks[i], "slice {i} md5");
                    if i + 1 < blocks.len() {
                        assert_eq!(part.len(), SLICE, "slices are 4 MB");
                    }
                    data.extend_from_slice(part);
                }
                assert_eq!(data.len(), up.size);
                let id = self.id();
                let mtime = id as i64;
                self.files.insert(up.path.clone(), (data, id, mtime));
                ok(json!({"errno": 0, "fs_id": id, "path": up.path}))
            }
            ("/rest/2.0/xpan/multimedia", "filemetas") => {
                assert_eq!(get("dlink"), "1");
                let ids: Vec<u64> = serde_json::from_str(&get("fsids")).unwrap();
                let list: Vec<Value> = ids
                    .iter()
                    .filter(|id| self.files.values().any(|f| f.1 == **id))
                    .map(|id| json!({"fs_id": id, "dlink": format!("{}/file/{id}?fid=x", self.base)}))
                    .collect();
                ok(json!({"errno": 0, "list": list}))
            }
            ("/rest/2.0/xpan/file", "filemanager") => {
                assert_eq!(get("opera"), "delete");
                let paths: Vec<String> = serde_json::from_str(&get("filelist")).unwrap();
                let mut info = Vec::new();
                let mut failed = false;
                for p in paths {
                    if self.files.remove(&p).is_some() {
                        info.push(json!({"errno": 0, "path": p}));
                    } else {
                        failed = true;
                        info.push(json!({"errno": -9, "path": p}));
                    }
                }
                ok(json!({"errno": if failed { 12 } else { 0 }, "info": info}))
            }
            (p, m) => panic!("unexpected request {p} method={m}"),
        }
    }
}

fn parse_pairs(s: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(s).into_owned().collect()
}

fn start_mock() -> (String, Arc<Mutex<Mock>>) {
    let state = Arc::new(Mutex::new(Mock::default()));
    let (tx, rx) = std::sync::mpsc::channel();
    let st = state.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
            {
                let mut m = st.lock().unwrap();
                m.base = base.clone();
                m.dirs.insert("/".into());
                m.dirs.insert("/apps".into());
            }
            tx.send(base).unwrap();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let st = st.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                        let st = st.clone();
                        async move {
                            let method = req.method().to_string();
                            let path = req.uri().path().to_string();
                            let q = parse_pairs(req.uri().query().unwrap_or("").as_bytes());
                            let ua = req.headers().get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                            let ctype = req.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                            let body = req.into_body().collect().await.unwrap().to_bytes();
                            let form = if ctype.starts_with("application/x-www-form-urlencoded") { parse_pairs(&body) } else { HashMap::new() };
                            let (status, headers, out) = st.lock().unwrap().handle(&method, &path, &q, &form, &ua, &body, &ctype);
                            let mut resp = hyper::Response::builder().status(status);
                            for (k, v) in headers {
                                resp = resp.header(k, v);
                            }
                            Ok::<_, Infallible>(resp.body(Full::new(Bytes::from(out))).unwrap())
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    });
    (rx.recv().unwrap(), state)
}

fn client(app: &str) -> BaiduClient {
    BaiduClient {
        app_key: "app-key".into(),
        secret_key: "secret-key".into(),
        app_name: app.into(),
    }
}

fn endpoints(base: &str) -> Endpoints {
    Endpoints {
        oauth: base.into(),
        pan: base.into(),
        pcs: base.into(),
        min_poll: Duration::from_millis(10),
    }
}

fn meta(game: &str, id: &str, parent: Option<&str>) -> SnapshotMeta {
    SnapshotMeta {
        id: id.into(),
        game: game.into(),
        parent: parent.map(str::to_string),
        device_id: "d-a".into(),
        device_name: "PC-A".into(),
        created_at: chrono::Utc::now(),
        trigger: Trigger::AutoExit,
        roots: BTreeMap::new(),
        content_hash: format!("hash-{id}"),
        files: Vec::new(),
        total_size: 3,
        app_version: "test".into(),
        comment: String::new(),
        locked: false,
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn baidu_cloud() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let (base, mock) = start_mock();
    let ep = endpoints(&base);
    let timeout = Duration::from_secs(10);

    // Cancelling the sign-in stops the wait.
    let cancelled = AtomicBool::new(true);
    let err = cloud::baidu_sign_in(&client("Ludonimbus"), &ep, |_| {}, &cancelled, timeout).unwrap_err();
    assert!(format!("{err:#}").contains("取消"), "{err:#}");
    mock.lock().unwrap().device_polls = 0;

    // Device-code sign-in: pending twice, then approved; the connection test
    // creates the app folder and writes / lists / reads / deletes a file.
    let mut shown = None;
    let (token, name, account) =
        cloud::baidu_sign_in(&client("Ludonimbus"), &ep, |c| shown = Some(c.clone()), &AtomicBool::new(false), timeout).unwrap();
    let code = shown.unwrap();
    assert_eq!(code.user_code, "ABCD1234");
    assert!(code.browser_url().ends_with("/device?code=ABCD1234"));
    assert_eq!(name, "玩家");
    assert_eq!(account.label, "百度网盘 · 我的应用数据/Ludonimbus");
    assert_eq!(account.total, Some(2_199_023_255_552));
    assert_eq!(mock.lock().unwrap().device_polls, 3);

    let s = BaiduPan::new(client("Ludonimbus"), ep.clone(), token.clone(), TokenSink::Memory, timeout);
    assert!(s.list("nothing/here").unwrap().is_empty());
    assert_eq!(s.read("missing.json").unwrap(), None);
    s.remove("missing.json").unwrap();

    // Nested folders are created explicitly before uploading.
    s.write("games/fe/game.json", b"{\"v\":1}").unwrap();
    assert_eq!(s.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    let games = s.list("games").unwrap();
    assert!(games.len() == 1 && games[0].is_dir && games[0].name == "fe");
    let before = s.list("games/fe").unwrap();
    s.write("games/fe/game.json", b"{\"v\":22}").unwrap();
    let after = s.list("games/fe").unwrap();
    assert_eq!(after.len(), 1, "overwritten, not renamed");
    assert_ne!(before[0].tag, after[0].tag);
    assert_eq!(s.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":22}"[..]));

    // A 9 MB file goes up in three slices and comes back through the
    // redirecting download link.
    let big: Vec<u8> = (0..9_000_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmp.path().join("big.zip");
    write_file(&src, &big);
    s.upload("games/fe/snapshots/big.zip", &src).unwrap();
    let back = tmp.path().join("big-back.zip");
    s.download("games/fe/snapshots/big.zip", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big);
    s.remove("games/fe/snapshots/big.zip").unwrap();
    assert!(s.list("games/fe/snapshots").unwrap().is_empty());
    assert!(s.download("games/fe/snapshots/big.zip", &back).is_err());

    // The server revokes the access token: the store refreshes once (the
    // refresh token is single-use) and carries on.
    mock.lock().unwrap().access.clear();
    assert!(s.read("games/fe/game.json").unwrap().is_some());
    assert_eq!(mock.lock().unwrap().refreshes, 1);
    assert_ne!(s.token(), token);
    // The old refresh token no longer works.
    let stale = BaiduPan::new(client("Ludonimbus"), ep.clone(), token, TokenSink::Memory, timeout);
    mock.lock().unwrap().access.clear();
    let err = stale.list("games").unwrap_err();
    assert!(format!("{err:#}").contains("重新连接"), "{err:#}");

    // The whole layout on top of it, filled by copying a folder cloud.
    let local = Repo::new(ludonimbus_core::cloud::local::LocalFolder::new(tmp.path().join("local-cloud")));
    std::fs::create_dir_all(tmp.path().join("local-cloud")).unwrap();
    for (i, id) in ["01AAA", "01BBB"].iter().enumerate() {
        let zip = tmp.path().join(format!("{id}.zip"));
        write_file(&zip, format!("zip-{id}").as_bytes());
        local.upload(&meta("fe", id, (i > 0).then_some("01AAA")), &zip).unwrap();
    }
    let dav = Repo::new(s);
    let report = migrate::copy_all(&local, &dav, &["fe".to_string()], |_| {}).unwrap();
    assert_eq!((report.copied, report.skipped), (2, 0));
    let list = dav.list_snapshots("fe").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].parent.as_deref(), Some("01AAA"));
    let out = tmp.path().join("dl.zip");
    dav.download("fe", "01BBB", &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"zip-01BBB");
    let mut m = dav.get_meta("fe", "01AAA").unwrap();
    m.comment = "通关前".into();
    dav.update_meta(&m).unwrap();
    assert_eq!(dav.list_snapshots("fe").unwrap()[0].comment, "通关前");
    dav.delete("fe", "01BBB").unwrap();
    assert_eq!(dav.list_snapshots("fe").unwrap().len(), 1);

    // An app name that does not match the registered one cannot write.
    let t = dav.store().token();
    let wrong = BaiduPan::new(client("Other"), ep, t, TokenSink::Memory, timeout);
    let err = cloud::probe_store(&wrong).unwrap_err();
    assert!(format!("{err:#}").contains("我的应用数据/Other"), "{err:#}");
}
