//! The WebDAV and S3 stores against stand-in servers: a real WebDAV server
//! (`dav-server`, in memory) and a small S3 that keeps objects in memory.
//! In front of both, answers can be swapped for "busy" ones, to see which
//! requests are sent again, after how long, and which never are.

use chrono::{DateTime, Utc};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use ludonimbus_core::cloud;
use ludonimbus_core::cloud::s3::S3;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::webdav::WebDav;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Tries again are logged: into a folder of the test's own.
fn home() {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LUDONIMBUS_HOME", tmp.path());
        tmp
    });
}

/// An answer given instead of the real one, to the next request whose
/// "METHOD path?query" starts with `matches`.
struct Trouble {
    matches: &'static str,
    status: u16,
    retry_after: Option<&'static str>,
    body: &'static str,
}

#[derive(Default)]
struct Server {
    trouble: VecDeque<Trouble>,
    /// "METHOD path?query" of every request.
    log: Vec<String>,
}

type State = Arc<Mutex<Server>>;

impl Server {
    fn busy(&mut self, matches: &'static str, status: u16, retry_after: Option<&'static str>) {
        self.trouble.push_back(Trouble { matches, status, retry_after, body: "" });
    }

    fn count(&self, prefix: &str) -> usize {
        self.log.iter().filter(|l| l.starts_with(prefix)).count()
    }
}

type Reply = hyper::Response<Full<Bytes>>;
type Answer = Pin<Box<dyn Future<Output = Reply> + Send>>;
type Handle = Arc<dyn Fn(hyper::Request<Incoming>) -> Answer + Send + Sync>;

/// Serve `handle` on a free port, answering what `state` has queued
/// instead (after reading the whole request, as a server does).
fn serve(state: State, handle: Handle) -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let (state, handle) = (state.clone(), handle.clone());
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                        let (state, handle) = (state.clone(), handle.clone());
                        async move {
                            let line = format!("{} {}", req.method(), req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/"));
                            let trouble = {
                                let mut s = state.lock().unwrap();
                                s.log.push(line.clone());
                                let i = s.trouble.iter().position(|t| line.starts_with(t.matches));
                                i.and_then(|i| s.trouble.remove(i))
                            };
                            let reply = match trouble {
                                Some(t) => {
                                    let _ = req.into_body().collect().await;
                                    let mut r = hyper::Response::builder().status(t.status);
                                    if let Some(v) = t.retry_after {
                                        r = r.header("Retry-After", v);
                                    }
                                    r.body(Full::new(Bytes::from(t.body))).unwrap()
                                }
                                None => handle(req).await,
                            };
                            Ok::<_, Infallible>(reply)
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(stream), service).await;
                });
            }
        });
    });
    rx.recv().unwrap()
}

fn entry<'a>(list: &'a [Entry], name: &str) -> &'a Entry {
    list.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} not listed"))
}

/// A listing time from the server's clock, which is this computer's.
fn just_now(t: Option<DateTime<Utc>>) -> bool {
    let now = Utc::now();
    t.is_some_and(|t| t > now - chrono::Duration::minutes(1) && t <= now + chrono::Duration::seconds(5))
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let out = f();
    (out, t.elapsed())
}

fn big_file(dir: &std::path::Path) -> (std::path::PathBuf, Vec<u8>) {
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let path = dir.join("big.zip");
    std::fs::write(&path, &data).unwrap();
    (path, data)
}

// ---------------------------------------------------------------------------
// WebDAV

fn start_dav() -> (u16, State) {
    let dav = dav_server::DavHandler::builder().filesystem(dav_server::memfs::MemFs::new()).locksystem(dav_server::fakels::FakeLs::new()).build_handler();
    let state = State::default();
    let handle: Handle = Arc::new(move |req: hyper::Request<Incoming>| -> Answer {
        let dav = dav.clone();
        Box::pin(async move {
            let (parts, body) = dav.handle(req).await.into_parts();
            let body = body.collect().await.map(|b| b.to_bytes()).unwrap_or_default();
            hyper::Response::from_parts(parts, Full::new(body))
        })
    });
    (serve(state.clone(), handle), state)
}

#[test]
fn webdav_listing_times_and_busy_answers() {
    home();
    let tmp = tempfile::tempdir().unwrap();
    let (port, server) = start_dav();
    let dav = WebDav::new(&format!("http://127.0.0.1:{port}/"), "u", "p", "Ludonimbus", Duration::from_secs(10)).unwrap();
    let count = |prefix: &str| server.lock().unwrap().count(prefix);
    dav.check().unwrap();
    dav.write("games/fe/a.json", b"one").unwrap();

    // Listings say when each file was written.
    let listed = dav.list("games/fe").unwrap();
    assert!(just_now(entry(&listed, "a.json").modified), "{listed:?}");

    // Busy for a moment: each request that does the same twice is sent again.
    server.lock().unwrap().busy("PROPFIND", 503, Some("0"));
    let before = count("PROPFIND");
    assert_eq!(dav.list("games/fe").unwrap().len(), 1);
    assert_eq!(count("PROPFIND") - before, 2);

    server.lock().unwrap().busy("GET", 429, Some("0"));
    let before = count("GET");
    assert_eq!(dav.read("games/fe/a.json").unwrap().as_deref(), Some(&b"one"[..]));
    assert_eq!(count("GET") - before, 2);

    server.lock().unwrap().busy("PUT", 502, Some("0"));
    let (puts, moves) = (count("PUT"), count("MOVE"));
    dav.write("games/fe/a.json", b"two").unwrap();
    assert_eq!((count("PUT") - puts, count("MOVE") - moves), (2, 1));
    assert_eq!(dav.read("games/fe/a.json").unwrap().as_deref(), Some(&b"two"[..]));

    // A file is sent from its start again.
    let (src, data) = big_file(tmp.path());
    server.lock().unwrap().busy("PUT", 503, Some("0"));
    let puts = count("PUT");
    dav.upload("games/fe/big.zip", &src).unwrap();
    assert_eq!(count("PUT") - puts, 2);
    assert_eq!(dav.read("games/fe/big.zip").unwrap(), Some(data));

    server.lock().unwrap().busy("DELETE", 503, Some("0"));
    let before = count("DELETE");
    dav.remove("games/fe/big.zip").unwrap();
    assert_eq!(count("DELETE") - before, 2);
    assert!(dav.read("games/fe/big.zip").unwrap().is_none());

    // The wait the server asks for...
    server.lock().unwrap().busy("GET", 503, Some("1"));
    let (read, took) = timed(|| dav.read("games/fe/a.json"));
    assert!(read.unwrap().is_some());
    assert!(took >= Duration::from_secs(1) && took < Duration::from_secs(5), "{took:?}");
    // ...unless it is longer than is worth waiting for here: sent once.
    server.lock().unwrap().busy("GET", 503, Some("60"));
    let before = count("GET");
    let (read, took) = timed(|| dav.read("games/fe/a.json"));
    assert!(cloud::is_unavailable(&read.unwrap_err()));
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert_eq!(count("GET") - before, 1);
    // Twice more at most.
    for status in [503, 500, 504] {
        server.lock().unwrap().busy("GET", status, Some("0"));
    }
    let before = count("GET");
    assert!(cloud::is_unavailable(&dav.read("games/fe/a.json").unwrap_err()));
    assert_eq!(count("GET") - before, 3);
    server.lock().unwrap().trouble.clear();

    // Never sent again: MOVE (the second one would move away what is in
    // its place by then) and MKCOL.
    server.lock().unwrap().busy("MOVE", 503, Some("0"));
    let moves = count("MOVE");
    assert!(dav.write("games/fe/a.json", b"three").is_err());
    assert_eq!(count("MOVE") - moves, 1);
    assert_eq!(dav.read("games/fe/a.json").unwrap().as_deref(), Some(&b"two"[..]));

    server.lock().unwrap().busy("MKCOL", 503, Some("0"));
    let made = count("MKCOL");
    assert!(dav.write("games/zelda/a.json", b"one").is_err());
    assert_eq!(count("MKCOL") - made, 1);
    dav.write("games/zelda/a.json", b"one").unwrap();

    // Quark's store decides itself when to try again.
    let once = WebDav::new(&format!("http://127.0.0.1:{port}/"), "u", "p", "Ludonimbus", Duration::from_secs(10)).unwrap().without_retries();
    server.lock().unwrap().busy("GET", 503, Some("0"));
    let before = count("GET");
    assert!(once.read("games/fe/a.json").is_err());
    assert_eq!(count("GET") - before, 1);
}

// ---------------------------------------------------------------------------
// S3

/// Objects by key: content and when it was written.
type Objects = Arc<Mutex<BTreeMap<String, (Vec<u8>, DateTime<Utc>)>>>;

fn s3_error(status: u16, code: &str) -> Reply {
    let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>{code}</Message></Error>");
    hyper::Response::builder().status(status).body(Full::new(Bytes::from(body))).unwrap()
}

/// What the S3 stand-in answers: ListObjectsV2 with `delimiter=/`, and
/// GET, PUT (checking the content against its signed hash) and DELETE of
/// objects, path style, in bucket `b`.
fn s3_answer(objects: &Objects, method: &str, uri: &hyper::Uri, sha: Option<&str>, body: &[u8]) -> Reply {
    let ok = |status: u16, body: Vec<u8>| hyper::Response::builder().status(status).body(Full::new(Bytes::from(body))).unwrap();
    let path = percent_encoding::percent_decode_str(uri.path()).decode_utf8_lossy().into_owned();
    let mut objects = objects.lock().unwrap();
    if path == "/b" && method == "GET" {
        let url = url::Url::parse(&format!("http://s3{uri}")).unwrap();
        let q: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
        let prefix = q.get("prefix").cloned().unwrap_or_default();
        let mut contents = String::new();
        let mut prefixes = std::collections::BTreeSet::new();
        for (key, (data, at)) in objects.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)) {
            match key[prefix.len()..].split_once('/') {
                Some((dir, _)) => {
                    prefixes.insert(format!("{prefix}{dir}/"));
                }
                None => {
                    let etag: String = Sha256::digest(data).iter().take(16).map(|b| format!("{b:02x}")).collect();
                    contents += &format!(
                        "<Contents><Key>{key}</Key><LastModified>{}</LastModified><ETag>\"{etag}\"</ETag><Size>{}</Size></Contents>",
                        at.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                        data.len()
                    );
                }
            }
        }
        let prefixes: String = prefixes.iter().map(|p| format!("<CommonPrefixes><Prefix>{p}</Prefix></CommonPrefixes>")).collect();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>b</Name><Prefix>{prefix}</Prefix><IsTruncated>false</IsTruncated>{contents}{prefixes}</ListBucketResult>"
        );
        return ok(200, xml.into_bytes());
    }
    let Some(key) = path.strip_prefix("/b/") else { return s3_error(404, "NoSuchBucket") };
    match method {
        "GET" => match objects.get(key) {
            Some((data, _)) => ok(200, data.clone()),
            None => s3_error(404, "NoSuchKey"),
        },
        "PUT" => {
            let hash: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
            if sha != Some(hash.as_str()) {
                return s3_error(400, "XAmzContentSHA256Mismatch");
            }
            objects.insert(key.to_string(), (body.to_vec(), Utc::now()));
            ok(200, Vec::new())
        }
        "DELETE" => {
            objects.remove(key);
            ok(204, Vec::new())
        }
        _ => s3_error(405, "MethodNotAllowed"),
    }
}

fn start_s3() -> (u16, State) {
    let objects = Objects::default();
    let state = State::default();
    let handle: Handle = Arc::new(move |req: hyper::Request<Incoming>| -> Answer {
        let objects = objects.clone();
        Box::pin(async move {
            let (method, uri) = (req.method().to_string(), req.uri().clone());
            let signed = req.headers().get("authorization").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("AWS4-HMAC-SHA256 "));
            let sha = req.headers().get("x-amz-content-sha256").and_then(|v| v.to_str().ok()).map(str::to_string);
            let body = req.into_body().collect().await.unwrap().to_bytes();
            if !signed {
                return s3_error(403, "AccessDenied");
            }
            s3_answer(&objects, &method, &uri, sha.as_deref(), &body)
        })
    });
    (serve(state.clone(), handle), state)
}

#[test]
fn s3_listing_times_and_busy_answers() {
    home();
    let tmp = tempfile::tempdir().unwrap();
    let (port, server) = start_s3();
    let s3 = S3::new(&format!("http://127.0.0.1:{port}"), "", "b", "Ludonimbus", "AKID", "secret", true, Duration::from_secs(10)).unwrap();
    let count = |prefix: &str| server.lock().unwrap().count(prefix);
    let list = "GET /b?";
    let object = "GET /b/";
    s3.check().unwrap();
    s3.write("games/fe/a.json", b"one").unwrap();

    // Objects say when they were written; prefixes ("folders") don't.
    let listed = s3.list("games/fe").unwrap();
    assert!(just_now(entry(&listed, "a.json").modified), "{listed:?}");
    assert_eq!(entry(&s3.list("games").unwrap(), "fe").modified, None);

    // Busy for a moment: every request is sent again.
    server.lock().unwrap().busy(list, 503, Some("0"));
    let before = count(list);
    assert_eq!(s3.list("games/fe").unwrap().len(), 1);
    assert_eq!(count(list) - before, 2);

    server.lock().unwrap().busy(object, 500, Some("0"));
    let before = count(object);
    assert_eq!(s3.read("games/fe/a.json").unwrap().as_deref(), Some(&b"one"[..]));
    assert_eq!(count(object) - before, 2);

    server.lock().unwrap().busy("PUT", 503, Some("0"));
    let before = count("PUT");
    s3.write("games/fe/a.json", b"two").unwrap();
    assert_eq!(count("PUT") - before, 2);
    assert_eq!(s3.read("games/fe/a.json").unwrap().as_deref(), Some(&b"two"[..]));

    // A file is sent from its start again (the stand-in checks its hash).
    let (src, data) = big_file(tmp.path());
    server.lock().unwrap().busy("PUT", 503, Some("0"));
    let before = count("PUT");
    s3.upload("games/fe/big.zip", &src).unwrap();
    assert_eq!(count("PUT") - before, 2);
    assert_eq!(s3.read("games/fe/big.zip").unwrap(), Some(data));

    server.lock().unwrap().busy("DELETE", 503, Some("0"));
    let before = count("DELETE");
    s3.remove("games/fe/big.zip").unwrap();
    assert_eq!(count("DELETE") - before, 2);
    assert!(s3.read("games/fe/big.zip").unwrap().is_none());

    // The wait the service asks for, up to 10 s; twice more at most.
    server.lock().unwrap().busy(object, 503, Some("1"));
    let (read, took) = timed(|| s3.read("games/fe/a.json"));
    assert!(read.unwrap().is_some());
    assert!(took >= Duration::from_secs(1) && took < Duration::from_secs(5), "{took:?}");
    server.lock().unwrap().busy(object, 503, Some("3600"));
    let before = count(object);
    let (read, took) = timed(|| s3.read("games/fe/a.json"));
    assert!(cloud::is_unavailable(&read.unwrap_err()));
    assert!(took < Duration::from_secs(3), "{took:?}");
    assert_eq!(count(object) - before, 1);
    server.lock().unwrap().trouble.extend((0..3).map(|_| Trouble {
        matches: "PUT",
        status: 503,
        retry_after: Some("0"),
        body: "<Error><Code>SlowDown</Code></Error>",
    }));
    let before = count("PUT");
    let err = s3.write("games/fe/a.json", b"three").unwrap_err();
    assert!(cloud::is_unavailable(&err), "{err:#}");
    assert_eq!(count("PUT") - before, 3);
    assert_eq!(s3.read("games/fe/a.json").unwrap().as_deref(), Some(&b"two"[..]));

    // What is not for the moment is not sent again.
    let before = count(object);
    assert!(s3.read("games/fe/none.json").unwrap().is_none());
    assert_eq!(count(object) - before, 1);
}
