//! The Switch client's connection to this PC over the local network
//! (docs/plan-platforms.md §4.3, S1; protocol in docs/nx-protocol.md).
//!
//! The Switch homebrew (`Ludonimbus-NX`) only reads and writes its saves;
//! this PC does the rest with the cloud it already uses: version history,
//! conflicts, the save library. The Switch is one more device, and one more
//! version of each game (`nxsave-<title id>` / `nxdevice-<title id>`, the
//! same roots as Ryujinx and Eden), so its saves carry over to emulators and
//! back.
//!
//! - Off unless the user turns it on (`AppConfig::nx_lan`).
//! - HTTPS with a self-signed certificate; the Switch keeps its
//!   fingerprint from pairing and trusts nothing else.
//! - Pairing: this PC shows a 6-digit code for 5 minutes (5 tries), the
//!   Switch sends it and gets a device token (only its hash is kept here).
//! - Discovery: a UDP broadcast `LUDONIMBUS_DISCOVER` on port 47717 is
//!   answered with this PC's name, port and fingerprint.

use crate::cloud::{Cloud, CloudGame, DeviceInfo};
use crate::config::AppConfig;
use crate::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use crate::{config, paths, savelib, switch, sync, util};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 47716;
pub const DISCOVERY_PORT: u16 = 47717;
/// Version of docs/nx-protocol.md this PC speaks.
pub const PROTOCOL: u32 = 2;
const DISCOVER: &[u8] = b"LUDONIMBUS_DISCOVER";
/// Largest upload accepted (a zip of one game's saves).
const MAX_UPLOAD: usize = 512 * 1024 * 1024;

fn dir() -> std::path::PathBuf {
    paths::data_dir().join("nx")
}

// ---------------------------------------------------------------------------
// Paired Switches

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paired {
    /// The Switch's own device id (kept on its SD card).
    pub id: String,
    pub name: String,
    /// SHA-256 of its token.
    pub token_hash: String,
    pub paired_at: DateTime<Utc>,
    #[serde(default)]
    pub last_seen: Option<DateTime<Utc>>,
}

fn devices_file() -> std::path::PathBuf {
    dir().join("devices.json")
}

pub fn devices() -> Vec<Paired> {
    std::fs::read(devices_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_devices(list: &[Paired]) -> Result<()> {
    util::atomic_write(&devices_file(), &serde_json::to_vec_pretty(list)?)
}

/// Forget a paired Switch: its token stops working.
pub fn remove_device(id: &str) -> Result<()> {
    let mut list = devices();
    list.retain(|d| d.id != id);
    save_devices(&list)
}

fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Pairing {
    code: String,
    until: Instant,
    tries: u32,
}

static PAIRING: Mutex<Option<Pairing>> = Mutex::new(None);

/// Start pairing: a 6-digit code the Switch must send within 5 minutes.
pub fn start_pairing() -> (String, DateTime<Utc>) {
    let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
    *PAIRING.lock().unwrap() = Some(Pairing { code: code.clone(), until: Instant::now() + Duration::from_secs(300), tries: 0 });
    (code, Utc::now() + chrono::Duration::seconds(300))
}

pub fn cancel_pairing() {
    *PAIRING.lock().unwrap() = None;
}

/// The code being shown, while pairing.
pub fn pairing_code() -> Option<String> {
    let p = PAIRING.lock().unwrap();
    p.as_ref().filter(|p| Instant::now() < p.until).map(|p| p.code.clone())
}

/// Check a code from a Switch and give it a token.
fn pair(code: &str, id: &str, name: &str) -> Result<String, (u16, String)> {
    let mut slot = PAIRING.lock().unwrap();
    let Some(p) = slot.as_mut().filter(|p| Instant::now() < p.until) else {
        return Err((403, "not pairing: start pairing on the PC".into()));
    };
    if p.code != code.trim() {
        p.tries += 1;
        if p.tries >= 5 {
            *slot = None;
        }
        return Err((403, "wrong code".into()));
    }
    *slot = None;
    drop(slot);
    if id.trim().is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err((400, "bad device id".into()));
    }
    let mut raw = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut raw);
    let token = hex(&raw);
    let mut list = devices();
    list.retain(|d| d.id != id);
    list.push(Paired { id: id.into(), name: name.trim().chars().take(40).collect(), token_hash: hash_token(&token), paired_at: Utc::now(), last_seen: None });
    save_devices(&list).map_err(|e| (500, format!("{e:#}")))?;
    Ok(token)
}

fn authorize(header: Option<&str>) -> Result<Paired, (u16, String)> {
    let token = header.and_then(|h| h.strip_prefix("Bearer ")).ok_or((401, "missing token".to_string()))?;
    let hash = hash_token(token.trim());
    let mut list = devices();
    let d = list.iter_mut().find(|d| d.token_hash == hash).ok_or((401, "unknown device".to_string()))?;
    d.last_seen = Some(Utc::now());
    let found = d.clone();
    let _ = save_devices(&list);
    Ok(found)
}

// ---------------------------------------------------------------------------
// TLS identity

/// This PC's certificate and key (PEM) and the certificate's SHA-256
/// fingerprint, made once.
pub fn identity() -> Result<(Vec<u8>, Vec<u8>, String)> {
    let (cert_path, key_path) = (dir().join("cert.pem"), dir().join("key.pem"));
    if !cert_path.is_file() || !key_path.is_file() {
        let ck = rcgen::generate_simple_self_signed(vec!["ludonimbus.local".to_string()])?;
        std::fs::create_dir_all(dir())?;
        util::atomic_write(&cert_path, ck.cert.pem().as_bytes())?;
        util::atomic_write(&key_path, ck.key_pair.serialize_pem().as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let cert = std::fs::read(&cert_path)?;
    let key = std::fs::read(&key_path)?;
    Ok((cert.clone(), key, fingerprint(&cert)?))
}

/// SHA-256 of the certificate (DER), as the Switch shows and pins it.
pub fn fingerprint(cert_pem: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(cert_pem)?;
    let b64: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
    use base64::Engine;
    let der = base64::engine::general_purpose::STANDARD.decode(b64)?;
    Ok(hex(&Sha256::digest(der)))
}

// ---------------------------------------------------------------------------
// Requests

/// One request, independent of the transport (tested without TLS).
pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub auth: Option<&'a str>,
    pub body: &'a [u8],
}

pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

fn ok_json(v: Value) -> Resp {
    Resp { status: 200, content_type: "application/json", body: v.to_string().into_bytes() }
}

fn err(status: u16, message: impl Into<String>) -> Resp {
    Resp { status, content_type: "application/json", body: json!({ "error": message.into() }).to_string().into_bytes() }
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| percent_encoding::percent_decode_str(&v.replace('+', " ")).decode_utf8_lossy().into_owned())
}

pub fn handle(req: &Req) -> Resp {
    let parts: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    if parts.first() != Some(&"nx") || parts.get(1) != Some(&"v1") {
        return err(404, "not found");
    }
    match (req.method, &parts[2..]) {
        ("GET", ["hello"]) => {
            let cfg = config::load().unwrap_or_default();
            ok_json(json!({ "app": "ludonimbus", "version": crate::APP_VERSION, "protocol": PROTOCOL, "name": cfg.device.name, "pairing": pairing_code().is_some() }))
        }
        ("POST", ["pair"]) => {
            #[derive(Deserialize)]
            struct Body {
                code: String,
                id: String,
                name: String,
            }
            let Ok(b) = serde_json::from_slice::<Body>(req.body) else { return err(400, "bad request") };
            match pair(&b.code, &b.id, &b.name) {
                Ok(token) => {
                    let cfg = config::load().unwrap_or_default();
                    ok_json(json!({ "token": token, "name": cfg.device.name }))
                }
                Err((s, m)) => err(s, m),
            }
        }
        _ => {
            let device = match authorize(req.auth) {
                Ok(d) => d,
                Err((s, m)) => return err(s, m),
            };
            match route(req, &parts[2..], &device) {
                Ok(r) => r,
                Err(e) => err(500, format!("{e:#}")),
            }
        }
    }
}

/// A Switch title id from the URL (16 hex digits).
fn title_of(s: &str) -> Result<u64> {
    if s.len() != 16 {
        bail!("bad title id");
    }
    u64::from_str_radix(s, 16).map_err(|_| anyhow!("bad title id"))
}

// ---------------------------------------------------------------------------
// What this PC knows of the cloud
//
// The Switch gets its answers from here at once: the cloud's game list as
// last fetched (the save library's copy) and each game's newest version as
// last listed, both refreshed in the background. A slow cloud (Quark through
// OpenList takes seconds per request) never holds up the Switch, except for
// the one check right before a version is written. The Switch's uploads go
// into this PC's queue of versions not yet in the cloud (`pending/<game>/`,
// like a PC's offline versions) and are uploaded in the background, retried
// until the cloud takes them.

/// A game's newest version counts as known for this long after listing it.
const HEAD_FRESH_FOR: Duration = Duration::from_secs(30);
/// The cloud's game list is fetched again in the background when older.
const LIST_MAX_AGE_MINUTES: i64 = 5;
/// One connection to the cloud serves requests for this long.
const CLOUD_REUSE_FOR: Duration = Duration::from_secs(600);
/// A Switch is registered as a device in the cloud at most this often.
const DEVICE_EVERY: Duration = Duration::from_secs(600);
/// A finished download is kept for the Switch this long.
const JOB_KEPT_FOR: Duration = Duration::from_secs(600);

struct OpenCloud {
    scope: String,
    opened: Instant,
    cloud: Arc<Cloud>,
}

struct Known {
    /// The newest version in the cloud as last listed.
    head: Option<SnapshotSummary>,
    /// Listed successfully at least once (else `head` means nothing).
    listed: bool,
    at: Instant,
}

#[derive(Default)]
struct Hub {
    /// The cloud this is about (all is forgotten when another is connected).
    scope: String,
    heads: HashMap<String, Known>,
    /// Games being listed right now.
    listing: HashSet<String>,
    /// The last failure talking to the cloud, shown on the Switch.
    error: Option<String>,
    device_put: HashMap<String, Instant>,
}

static CLOUD: Mutex<Option<OpenCloud>> = Mutex::new(None);
static HUB: LazyLock<Mutex<Hub>> = LazyLock::new(Default::default);
type Log = Arc<dyn Fn(String) + Send + Sync>;
static LOG: Mutex<Option<Log>> = Mutex::new(None);

fn note(msg: String) {
    let log = LOG.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(log) = log {
        log(msg);
    }
}

fn scope_of(cfg: &AppConfig) -> String {
    serde_json::to_string(&cfg.cloud).unwrap_or_default()
}

/// The cloud, opened once and shared (opening Quark checks or starts
/// OpenList and signs in to it).
fn cloud(cfg: &AppConfig) -> Result<Arc<Cloud>> {
    let scope = scope_of(cfg);
    let mut slot = CLOUD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = slot.as_ref().filter(|c| c.scope == scope && c.opened.elapsed() < CLOUD_REUSE_FOR) {
        // A game may have joined another version's line since the cloud
        // was opened ([`crate::join`]).
        for g in &cfg.games {
            c.cloud.know(g);
        }
        return Ok(c.cloud.clone());
    }
    let opened = crate::cloud::open(cfg)?.ok_or_else(|| anyhow!("no cloud is connected on the PC"))?;
    let cloud = Arc::new(opened);
    *slot = Some(OpenCloud { scope, opened: Instant::now(), cloud: cloud.clone() });
    Ok(cloud)
}

fn hub(cfg: &AppConfig) -> std::sync::MutexGuard<'static, Hub> {
    let mut h = HUB.lock().unwrap_or_else(|e| e.into_inner());
    let scope = scope_of(cfg);
    if h.scope != scope {
        *h = Hub { scope, ..Default::default() };
    }
    h
}

fn failed(cfg: &AppConfig, e: &anyhow::Error) {
    note(format!("cloud: {e:#}"));
    hub(cfg).error = Some(format!("{e:#}"));
}

/// A version in this PC's queue as it will be listed once it is uploaded:
/// the newest of its game, unless it is kept beside the history.
fn waiting_summary(m: &SnapshotMeta) -> SnapshotSummary {
    let mut s = SnapshotSummary::from(m);
    s.head = !s.side;
    s
}

/// Versions of `key` on this PC that the cloud doesn't have yet.
fn pending(key: &str) -> Vec<SnapshotSummary> {
    sync::pending_metas(key).iter().map(waiting_summary).collect()
}

fn remember_head(cfg: &AppConfig, key: &str, head: Option<SnapshotSummary>) {
    hub(cfg).heads.insert(key.to_string(), Known { head, listed: true, at: Instant::now() });
}

/// Switch title → (game key, name): this PC's games first, then the cloud's
/// game list as last fetched. Also whether that list was ever fetched; an
/// old or missing list is fetched in the background.
fn titles(cfg: &AppConfig) -> (HashMap<u64, (String, String)>, bool) {
    let cached = savelib::cached(cfg);
    if !cached.fetched_at.is_some_and(|t| Utc::now() - t <= chrono::Duration::minutes(LIST_MAX_AGE_MINUTES)) {
        fetch_list_in_background();
    }
    let mut map = HashMap::new();
    for cg in &cached.games {
        for id in savelib::cloud_ids(cg) {
            if let Some(t) = id.strip_prefix("nx:").and_then(|h| u64::from_str_radix(h, 16).ok()) {
                map.entry(t).or_insert_with(|| (cg.key.clone(), cg.name.clone()));
            }
        }
    }
    for g in &cfg.games {
        if let Some(t) = crate::games::switch_title(&g.saves) {
            map.insert(t, (g.key.clone(), g.name.clone()));
        }
    }
    (map, cached.fetched_at.is_some())
}

/// One title's game. When the copy of the cloud's list doesn't have it, the
/// list is fetched now (another device may just have added the game).
fn game_key(cfg: &AppConfig, title: u64) -> Result<Option<(String, String)>> {
    if let Some(k) = titles(cfg).0.remove(&title) {
        return Ok(Some(k));
    }
    savelib::refresh(cfg, &*cloud(cfg)?)?;
    Ok(titles(cfg).0.remove(&title))
}

fn fetch_list_in_background() {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if let Ok(cfg) = config::load() {
            if let Err(e) = cloud(&cfg).and_then(|c| savelib::refresh(&cfg, &*c)) {
                failed(&cfg, &e);
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

/// List these games' versions in the background, a few at a time (each is a
/// request to the cloud), unless they're being listed already.
fn list_in_background(cfg: &AppConfig, keys: Vec<String>) {
    let keys: Vec<String> = {
        let mut h = hub(cfg);
        keys.into_iter().filter(|k| h.listing.insert(k.clone())).collect()
    };
    if keys.is_empty() {
        return;
    }
    let cfg = cfg.clone();
    std::thread::spawn(move || {
        let heads: Vec<Result<Option<SnapshotSummary>>> = match cloud(&cfg) {
            Ok(cloud) => {
                let mut out = Vec::with_capacity(keys.len());
                for batch in keys.chunks(6) {
                    let cloud = &*cloud;
                    let got: Vec<Result<Option<SnapshotSummary>>> = std::thread::scope(|s| {
                        let jobs: Vec<_> = batch.iter().map(|k| s.spawn(move || cloud.list_snapshots(k).map(|l| sync::mainline_head(&l)))).collect();
                        jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| Err(anyhow!("listing the versions failed")))).collect()
                    });
                    out.extend(got);
                }
                out
            }
            Err(e) => keys.iter().map(|_| Err(anyhow!("{e:#}"))).collect(),
        };
        let mut problem = None;
        {
            let mut h = hub(&cfg);
            for (k, r) in keys.iter().zip(heads) {
                h.listing.remove(k);
                match r {
                    Ok(head) => {
                        h.heads.insert(k.clone(), Known { head, listed: true, at: Instant::now() });
                    }
                    Err(e) => {
                        // Not asked again right away; what was known stays.
                        let old = h.heads.remove(k);
                        let (head, listed) = old.map(|o| (o.head, o.listed)).unwrap_or((None, false));
                        h.heads.insert(k.clone(), Known { head, listed, at: Instant::now() });
                        problem = Some(e);
                    }
                }
            }
            if problem.is_none() {
                h.error = None;
            }
        }
        if let Some(e) = problem {
            failed(&cfg, &e);
        }
    });
}

/// Register the Switch as a device in the cloud (in the background, now and
/// then).
fn register_device(cfg: &AppConfig, d: &Paired, client: String) {
    {
        let mut h = hub(cfg);
        if h.device_put.get(&d.id).is_some_and(|t| t.elapsed() < DEVICE_EVERY) {
            return;
        }
        h.device_put.insert(d.id.clone(), Instant::now());
    }
    let (cfg, d) = (cfg.clone(), d.clone());
    std::thread::spawn(move || {
        let (id, name) = device_tuple(&d);
        // It reaches the cloud through this PC, so it knows the layout this PC knows.
        let info = DeviceInfo {
            id,
            name,
            os: "switch".into(),
            app_version: client,
            last_seen: Utc::now(),
            layout: crate::cloud::v2::LAYOUT,
            installs: Vec::new(),
        };
        if let Err(e) = cloud(&cfg).and_then(|c| c.put_device(&info)) {
            failed(&cfg, &e);
        }
    });
}

static UPLOADING: Mutex<()> = Mutex::new(());

/// The Switch's versions waiting in this PC's queue (all games), oldest first.
fn switch_waiting() -> Vec<SnapshotMeta> {
    let mut out: Vec<SnapshotMeta> =
        sync::pending_keys().iter().flat_map(|k| sync::pending_metas(k)).filter(|m| m.device_id.starts_with("nx-")).collect();
    out.sort_by_key(|m| m.created_at);
    out
}

/// How many of the Switch's versions are waiting to be uploaded.
pub fn waiting_uploads() -> usize {
    switch_waiting().len()
}

/// Upload the Switch's versions waiting now; how many were uploaded.
pub fn upload_waiting() -> Result<usize> {
    let _one = UPLOADING.lock().unwrap_or_else(|e| e.into_inner());
    let waiting = switch_waiting();
    if waiting.is_empty() {
        return Ok(0);
    }
    let cfg = config::load()?;
    let cloud = cloud(&cfg)?;
    for meta in &waiting {
        sync::upload_pending(&cloud, meta)?;
        note(format!("{}: uploaded the Switch's version {}", meta.game, meta.id));
        // Known as the cloud's before it leaves the queue, so an answer
        // never misses it in between.
        let mut h = hub(&cfg);
        h.error = None;
        if let Some(k) = h.heads.get_mut(&meta.game) {
            let mut all: Vec<SnapshotSummary> = k.head.take().into_iter().collect();
            all.push(waiting_summary(meta));
            k.head = sync::mainline_head(&all);
        }
        sync::drop_pending(meta)?;
    }
    Ok(waiting.len())
}

static HOLD_UPLOADS: AtomicBool = AtomicBool::new(false);

/// Tests: keep the Switch's versions in the queue until [`upload_waiting`].
#[doc(hidden)]
pub fn hold_uploads(hold: bool) {
    HOLD_UPLOADS.store(hold, Ordering::SeqCst);
}

/// Keep uploading in the background until the queue is empty, pausing
/// longer after each failure (the cloud may be away for a while).
fn upload_in_background() {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if HOLD_UPLOADS.load(Ordering::SeqCst) || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let mut pause = 5;
        loop {
            match upload_waiting() {
                Ok(0) => break,
                Ok(_) => pause = 5,
                Err(e) => {
                    if let Ok(cfg) = config::load() {
                        failed(&cfg, &e);
                    }
                    note(format!("uploading the Switch's saves failed, trying again in {pause} s"));
                    std::thread::sleep(Duration::from_secs(pause));
                    pause = (pause * 2).min(300);
                }
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
        // Queued while this was finishing.
        if !switch_waiting().is_empty() {
            upload_in_background();
        }
    });
}

// Downloads for the Switch: fetched from the cloud in the background while
// the Switch asks how it's going, then handed over in one piece.

enum JobState {
    Working,
    Done(std::path::PathBuf),
    Failed(String),
}

struct Job {
    state: JobState,
    started: Instant,
    ended: Option<Instant>,
}

static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);

fn jobs() -> std::sync::MutexGuard<'static, HashMap<String, Job>> {
    let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    jobs.retain(|_, j| {
        let keep = j.ended.is_none_or(|t| t.elapsed() < JOB_KEPT_FOR);
        if let (false, JobState::Done(p)) = (keep, &j.state) {
            let _ = std::fs::remove_file(p);
        }
        keep
    });
    jobs
}

/// The bindings of this PC's game `key` (an emulator's version of the
/// title): through them the line it shares holds the Switch's files under
/// that line's paths ([`crate::join`]). Empty when the game's files are the
/// line's as they are, or the game is only in the cloud.
fn binds_of(cfg: &AppConfig, key: &str) -> Vec<crate::bind::Bind> {
    cfg.game(key).map(|g| g.bind.clone()).unwrap_or_default()
}

/// One version's files of the Switch game, as a zip for the Switch: from
/// this PC's queue when it's still there, else from the cloud.
fn fetch_for_switch(cfg: &AppConfig, key: &str, id: &str, title: u64, job: &str) -> Result<std::path::PathBuf> {
    let binds = binds_of(cfg, key);
    let queued = sync::pending_zips(key, id);
    let local = if queued.is_empty() { None } else { repack_for_switch(&queued, title, &binds).ok() };
    let body = match local {
        Some(b) => b,
        None => {
            let tmp = paths::staging_dir().join(format!("nx-{job}.zip"));
            let got = cloud(cfg).and_then(|c| c.download(key, id, &tmp)).and_then(|_| repack_for_switch(std::slice::from_ref(&tmp), title, &binds));
            let _ = std::fs::remove_file(&tmp);
            got?
        }
    };
    let out = dir().join("jobs").join(format!("{job}.zip"));
    std::fs::create_dir_all(dir().join("jobs"))?;
    std::fs::write(&out, body)?;
    Ok(out)
}

fn device_tuple(d: &Paired) -> (String, String) {
    (format!("nx-{}", d.id), if d.name.is_empty() { "Switch".into() } else { d.name.clone() })
}

fn status_of(head: &Option<SnapshotSummary>, last: &Option<String>, changed: bool) -> &'static str {
    match (head, last) {
        (None, _) => "upload",
        (Some(h), Some(last)) if &h.id == last => {
            if changed {
                "localNewer"
            } else {
                "synced"
            }
        }
        (Some(_), _) if changed || last.is_none() => "conflict",
        (Some(_), _) => "cloudNewer",
    }
}

fn flag(query: &str, name: &str) -> bool {
    query_param(query, name).is_some_and(|v| v == "1" || v == "true")
}

fn route(req: &Req, parts: &[&str], device: &Paired) -> Result<Resp> {
    let cfg = config::load()?;
    match (req.method, parts) {
        // The Switch reports its saves; each gets its status at once, from
        // what this PC knows. `fresh: false`: some of it is being checked
        // with the cloud, ask again in a moment.
        ("POST", ["status"]) => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Item {
                title: String,
                #[serde(default)]
                last_synced_id: Option<String>,
                /// Changed on the Switch since that sync.
                #[serde(default)]
                changed: bool,
            }
            #[derive(Deserialize)]
            struct Body {
                games: Vec<Item>,
            }
            let body: Body = serde_json::from_slice(req.body).context("bad request")?;
            let (known, list_fetched) = titles(&cfg);
            let mut fresh = list_fetched;
            let mut to_list = Vec::new();
            let mut out = Vec::new();
            {
                let h = hub(&cfg);
                for it in body.games.iter().take(500) {
                    let Ok(title) = title_of(&it.title) else { continue };
                    let key = known.get(&title);
                    let (status, head, waiting) = match key {
                        None if !list_fetched => ("checking", None, false),
                        None => ("upload", None, false),
                        Some((k, _)) => {
                            let pend = pending(k);
                            let entry = h.heads.get(k);
                            let busy = h.listing.contains(k);
                            let stale = entry.is_none_or(|e| e.at.elapsed() > HEAD_FRESH_FOR);
                            if stale && !busy {
                                to_list.push(k.clone());
                            }
                            if stale || busy {
                                fresh = false;
                            }
                            match entry.filter(|e| e.listed) {
                                None if stale || busy => ("checking", None, !pend.is_empty()),
                                // Couldn't be listed (see `error`).
                                None => ("unknown", None, !pend.is_empty()),
                                Some(e) => {
                                    let mut all = pend.clone();
                                    all.extend(e.head.clone());
                                    let head = sync::mainline_head(&all);
                                    (status_of(&head, &it.last_synced_id, it.changed), head, !pend.is_empty())
                                }
                            }
                        }
                    };
                    out.push(json!({
                        "title": it.title,
                        "key": key.map(|k| &k.0),
                        "name": key.map(|k| &k.1),
                        "status": status,
                        "head": head,
                        "pending": waiting,
                    }));
                }
            }
            list_in_background(&cfg, to_list);
            register_device(&cfg, device, query_param(req.query, "client").unwrap_or_default());
            if waiting_uploads() > 0 {
                upload_in_background();
            }
            let error = hub(&cfg).error.clone();
            Ok(ok_json(json!({ "games": out, "fresh": fresh, "error": error })))
        }
        ("GET", ["games", title, "snapshots"]) => {
            let title = title_of(title)?;
            let Some((key, _)) = game_key(&cfg, title)? else { return Ok(ok_json(json!({ "snapshots": [] }))) };
            // The queue first: a version uploaded meanwhile is then in both.
            let waiting = pending(&key);
            let listed = cloud(&cfg)?.list_snapshots(&key)?;
            remember_head(&cfg, &key, sync::mainline_head(&listed));
            let mut all: Vec<(SnapshotSummary, bool)> = listed.into_iter().map(|s| (s, false)).collect();
            for p in waiting {
                if !all.iter().any(|(s, _)| s.id == p.id) {
                    all.push((p, true));
                }
            }
            all.sort_by_key(|(s, _)| std::cmp::Reverse(s.created_at));
            let head = sync::mainline_head(&all.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>()).map(|h| h.id);
            let list: Vec<Value> = all
                .into_iter()
                .map(|(s, waiting)| {
                    let mut v = serde_json::to_value(&s).unwrap_or_default();
                    v["pending"] = json!(waiting);
                    v
                })
                .collect();
            Ok(ok_json(json!({ "key": key, "headId": head, "snapshots": list })))
        }
        // Start fetching one version's save files for the Switch.
        ("POST", ["downloads"]) => {
            #[derive(Deserialize)]
            struct Body {
                title: String,
                id: String,
            }
            let b: Body = serde_json::from_slice(req.body).context("bad request")?;
            let title = title_of(&b.title)?;
            if b.id.is_empty() || !b.id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Ok(err(400, "bad id"));
            }
            let Some((key, _)) = game_key(&cfg, title)? else { return Ok(err(404, "no such game")) };
            let mut raw = [0u8; 8];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut raw);
            let job = hex(&raw);
            jobs().insert(job.clone(), Job { state: JobState::Working, started: Instant::now(), ended: None });
            let job2 = job.clone();
            std::thread::spawn(move || {
                let state = match fetch_for_switch(&cfg, &key, &b.id, title, &job2) {
                    Ok(p) => JobState::Done(p),
                    Err(e) => {
                        note(format!("{key}: fetching {} for the Switch failed: {e:#}", b.id));
                        JobState::Failed(format!("{e:#}"))
                    }
                };
                if let Some(j) = jobs().get_mut(&job2) {
                    j.state = state;
                    j.ended = Some(Instant::now());
                }
            });
            Ok(ok_json(json!({ "job": job })))
        }
        ("GET", ["jobs", job]) => {
            let jobs = jobs();
            let Some(j) = jobs.get(*job) else { return Ok(err(404, "no such job")) };
            let seconds = j.ended.unwrap_or_else(Instant::now).duration_since(j.started).as_secs();
            Ok(ok_json(match &j.state {
                JobState::Working => json!({ "state": "working", "seconds": seconds }),
                JobState::Done(_) => json!({ "state": "done", "seconds": seconds }),
                JobState::Failed(e) => json!({ "state": "failed", "seconds": seconds, "error": e }),
            }))
        }
        // The files of a finished download: `nxsave-<title>/…` and
        // `nxdevice-<title>/…` (other files of the game are left out).
        ("GET", ["jobs", job, "file"]) => {
            let path = match jobs().get(*job).map(|j| &j.state) {
                Some(JobState::Done(p)) => p.clone(),
                Some(_) => return Ok(err(409, "not ready")),
                None => return Ok(err(404, "no such job")),
            };
            Ok(Resp { status: 200, content_type: "application/zip", body: std::fs::read(path)? })
        }
        // The Switch's saves become a new version of the game: checked
        // against the cloud now, then queued here and uploaded in the
        // background.
        ("POST", ["games", title, "snapshots"]) => {
            let title = title_of(title)?;
            if req.body.len() > MAX_UPLOAD {
                return Ok(err(413, "too large"));
            }
            let files = unpack_from_switch(req.body, title)?;
            if files.is_empty() {
                return Ok(err(400, "no save files"));
            }
            // `backup`: the Switch's saves are about to be overwritten by a
            // download (the user chose the cloud's in a conflict). Kept as a
            // side version, like a PC's "pre-restore" copy: in the history,
            // never the newest version, never a conflict.
            let backup = flag(req.query, "backup");
            // `force`: on top of the newest version, whatever it is (the user
            // chose to keep the Switch's).
            let force = flag(req.query, "force");
            let cloud = cloud(&cfg)?;
            let key = match game_key(&cfg, title)? {
                Some((k, _)) => k,
                None => {
                    // A game only the Switch has: define it in the cloud,
                    // so emulators on other devices find it by title id.
                    let name = query_param(req.query, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("{title:016X}"));
                    let cg = CloudGame {
                        key: format!("nx-{title:016x}"),
                        name,
                        adapter: None,
                        saves: vec![
                            format!("{{{}}}/**", switch::root_name(switch::SaveKind::Account, title)),
                            format!("{{{}}}/**", switch::root_name(switch::SaveKind::Device, title)),
                        ],
                        exclude: vec![],
                        wait_for: vec![],
                        roots: Default::default(),
                        ludusavi_title: None,
                        source: None,
                        ids: vec![format!("nx:{title:016X}")],
                        game: None,
                    };
                    cloud.put_game(&cg)?;
                    savelib::remember(&cfg, &cg);
                    cg.key
                }
            };
            // The newest version as it is now (the Switch's list may be
            // older), counting versions this PC hasn't uploaded yet (read
            // first: one uploaded meanwhile is then in both).
            let waiting = pending(&key);
            let line = cloud.open(&key)?;
            let listed = crate::cloud::client::summaries(&line.view);
            remember_head(&cfg, &key, sync::mainline_head(&listed));
            let mut all = listed;
            for p in waiting {
                if !all.iter().any(|s| s.id == p.id) {
                    all.push(p);
                }
            }
            let head = sync::mainline_head(&all);
            let asked = query_param(req.query, "parent").filter(|p| !p.is_empty());
            let parent = if backup {
                asked
            } else if force {
                head.as_ref().map(|h| h.id.clone())
            } else {
                match &head {
                    Some(h) if asked.as_deref() != Some(h.id.as_str()) => {
                        return Ok(Resp {
                            status: 409,
                            content_type: "application/json",
                            body: json!({ "error": "the cloud has a newer version", "head": h }).to_string().into_bytes(),
                        });
                    }
                    _ => asked,
                }
            };
            let trigger = if backup { Trigger::PreRestore } else { Trigger::SyncNow };
            let (id, name) = device_tuple(device);
            // The game version that wrote the saves, for games whose rule says how.
            let version = crate::crosssaves::files_version(&[format!("nx:{title:016X}")], "switch", &files, &Default::default());
            // The title is a game of this PC that shares another version's
            // line through bindings: the Switch's files go there under that
            // line's paths, the ones the bindings don't name stay out, and
            // that line's other platform's files go along from the version
            // these files were made from (crate::join).
            let (files, carried) = match cfg.game(&key).filter(|g| !g.bind.is_empty()) {
                None => (files, Vec::new()),
                Some(g) => {
                    let mut mapped: Vec<(String, Vec<u8>)> = files.into_iter().filter_map(|(p, d)| crate::bind::to_canon(&g.bind, &p).map(|c| (c, d))).collect();
                    if mapped.is_empty() {
                        return Ok(err(400, "no save files"));
                    }
                    let (refs, content) = sync::carried_for(&cloud, g, &line, parent.as_deref())?;
                    mapped.extend(content);
                    (mapped, refs)
                }
            };
            // A backup the cloud already holds isn't stored twice.
            let hash = match carried.is_empty() {
                true => snapshot::files_hash(&files),
                false => {
                    let mut entries: Vec<snapshot::FileEntry> =
                        files.iter().map(|(path, data)| snapshot::FileEntry { path: path.clone(), size: data.len() as u64, hash: blake3::hash(data).to_hex().to_string(), mtime: 0 }).collect();
                    entries.extend(carried.iter().map(|f| snapshot::FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }));
                    snapshot::content_hash(&entries)
                }
            };
            if let Some(same) = all.iter().find(|s| backup && s.content_hash == hash) {
                return Ok(ok_json(json!({ "id": same.id, "key": key, "existing": true })));
            }
            let meta = sync::queue_files_carrying(
                &line,
                &sync::Foreign {
                    key: &key,
                    files: &files,
                    trigger,
                    parent,
                    over_all: force,
                    device: (&id, &name),
                    platform: "console:switch",
                    comment: "",
                    game_version: version,
                    origin: None,
                },
                &carried,
            )?;
            upload_in_background();
            Ok(ok_json(json!({ "id": meta.id, "key": key, "pending": true })))
        }
        _ => Ok(err(404, "not found")),
    }
}

/// Which files of a snapshot belong to the Switch game `title`.
fn is_switch_path(path: &str, title: u64) -> bool {
    let root = path.split('/').next().unwrap_or("");
    switch::parse_root(root).is_some_and(|(_, t)| t == title)
}

/// The Switch game's files out of the zips of a version (each file as
/// `files/<path>` in one of them), as one zip for the Switch. `binds`: the
/// bindings of the game the title is on this PC; the line's files are then
/// under its paths, and go to the Switch under the Switch's.
fn repack_for_switch(zips: &[std::path::PathBuf], title: u64, binds: &[crate::bind::Bind]) -> Result<Vec<u8>> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for zip in zips {
        let mut src = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
        for i in 0..src.len() {
            let mut e = src.by_index(i)?;
            let Some(path) = e.name().strip_prefix("files/").map(str::to_string) else { continue };
            let path = match binds.is_empty() {
                true => path,
                false => match crate::bind::to_local(binds, &path) {
                    Some(own) => own,
                    None => continue,
                },
            };
            if e.is_dir() || !is_switch_path(&path, title) {
                continue;
            }
            let mut data = Vec::with_capacity(e.size() as usize);
            e.read_to_end(&mut data)?;
            out.start_file(path, opts)?;
            out.write_all(&data)?;
        }
    }
    Ok(out.finish()?.into_inner())
}

fn unpack_from_switch(body: &[u8], title: u64) -> Result<Vec<(String, Vec<u8>)>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(body)).context("not a zip")?;
    let mut files = Vec::new();
    let mut total = 0usize;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        if e.is_dir() {
            continue;
        }
        let name = e.name().replace('\\', "/");
        if !is_switch_path(&name, title) {
            bail!("unexpected file in the upload: {name}");
        }
        let mut data = Vec::new();
        e.read_to_end(&mut data)?;
        total += data.len();
        if total > MAX_UPLOAD {
            bail!("too large");
        }
        files.push((name, data));
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// Server

pub struct Server {
    http: Arc<tiny_http::Server>,
    stop: Arc<AtomicBool>,
    pub port: u16,
    pub fingerprint: String,
}

impl Server {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.http.unblock();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start listening on the local network (HTTPS on [`PORT`], discovery on
/// [`DISCOVERY_PORT`]).
pub fn start(log: impl Fn(String) + Send + Sync + 'static) -> Result<Server> {
    let (cert, key, fp) = identity()?;
    let http = tiny_http::Server::https(("0.0.0.0", PORT), tiny_http::SslConfig { certificate: cert, private_key: key })
        .map_err(|e| anyhow!("could not listen on port {PORT}: {e}"))?;
    let http = Arc::new(http);
    let stop = Arc::new(AtomicBool::new(false));
    let log: Log = Arc::new(log);
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(log.clone());
    // Versions a Switch left here before the PC was closed.
    upload_in_background();
    {
        let (http, log) = (http.clone(), log.clone());
        std::thread::spawn(move || {
            for mut r in http.incoming_requests() {
                let log = log.clone();
                std::thread::spawn(move || {
                    let url = r.url().to_string();
                    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
                    let auth = r.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string());
                    let mut body = Vec::new();
                    let _ = r.as_reader().take(MAX_UPLOAD as u64 + 1).read_to_end(&mut body);
                    let method = r.method().as_str().to_string();
                    let resp = handle(&Req { method: &method, path, query, auth: auth.as_deref(), body: &body });
                    let why = if resp.status >= 400 && resp.status != 409 {
                        let text = serde_json::from_slice::<Value>(&resp.body).ok().and_then(|v| v["error"].as_str().map(str::to_string));
                        text.map(|t| format!(": {}", t.chars().take(300).collect::<String>())).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    log(format!("{method} {path} → {}{why}", resp.status));
                    let mut out = tiny_http::Response::from_data(resp.body).with_status_code(resp.status);
                    if let Ok(h) = tiny_http::Header::from_bytes("Content-Type", resp.content_type) {
                        out.add_header(h);
                    }
                    let _ = r.respond(out);
                });
            }
        });
    }
    {
        let (stop, fp2) = (stop.clone(), fp.clone());
        std::thread::spawn(move || {
            let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else { return };
            let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 64];
            while !stop.load(Ordering::SeqCst) {
                if let Ok((n, from)) = sock.recv_from(&mut buf) {
                    if &buf[..n] == DISCOVER {
                        let name = config::load().map(|c| c.device.name).unwrap_or_default();
                        let reply = json!({ "app": "ludonimbus", "name": name, "port": PORT, "fingerprint": fp2 }).to_string();
                        let _ = sock.send_to(reply.as_bytes(), from);
                    }
                }
            }
        });
    }
    Ok(Server { http, stop, port: PORT, fingerprint: fp })
}

/// This PC's address on the local network (for typing it on the Switch).
pub fn local_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // No packet is sent: this only picks the interface of the default route.
    s.connect("192.0.2.1:9").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_and_paths() {
        let ck = rcgen::generate_simple_self_signed(vec!["x".to_string()]).unwrap();
        let fp = fingerprint(ck.cert.pem().as_bytes()).unwrap();
        assert_eq!(fp, hex(&Sha256::digest(ck.cert.der())));
        let t = 0x0100_7700_08DD_8000;
        assert!(is_switch_path("nxsave-0100770008dd8000/save.bin", t));
        assert!(is_switch_path("nxdevice-0100770008dd8000/x", t));
        assert!(!is_switch_path("nxsave-0100000000010000/x", t));
        assert!(!is_switch_path("appdata/x", t));
        assert!(title_of("0100770008dd8000").is_ok() && title_of("nope").is_err());
    }
}
