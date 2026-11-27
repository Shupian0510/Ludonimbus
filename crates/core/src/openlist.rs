//! [OpenList](https://github.com/OpenListTeam/OpenList) (AGPL-3.0), run by
//! Ludonimbus to reach Quark through WebDAV.
//!
//! Quark has no API for third-party apps; OpenList's Quark driver talks to
//! the Quark web API with the cookie of a signed-in browser. Ludonimbus
//! downloads the official OpenList release once (version and SHA-256 pinned
//! here), runs it hidden on 127.0.0.1 with a random admin password, mounts
//! Quark at [`QUARK_MOUNT`] and adds a WebDAV-only user limited to the
//! Ludonimbus folder. OpenList is not modified or bundled.
//!
//! Layout under `%APPDATA%\Ludonimbus\openlist`:
//!
//! ```text
//! v4.2.10/openlist.exe   the program
//! data/                  OpenList's config.json and database (holds the Quark cookie)
//! ludonimbus.json        port and pid of the running server
//! ```
//!
//! Passwords (admin, WebDAV user) are kept in the Windows Credential Manager.
//! See docs/plan-phase3.md section 1 for the measurements behind the
//! listing cache and the forced refreshes.

use crate::cloud::{secrets, Unavailable};
use crate::{lockfile::FileLock, paths, tr, util};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const VERSION: &str = "v4.2.10";
pub const ASSET: &str = "openlist-windows-amd64.zip";
pub const SHA256: &str = "e5317fb186a541558421ffebd4b0b9579d0292f9e884025f0b85994bbf6c7c81";
/// Size of [`ASSET`] in bytes (shown before downloading).
pub const SIZE: u64 = 59_999_218;
pub const PROJECT_URL: &str = "https://github.com/OpenListTeam/OpenList";
pub const LICENSE: &str = "AGPL-3.0";

/// Where Quark is mounted inside OpenList.
pub const QUARK_MOUNT: &str = "/quark";
const WEBDAV_USER: &str = "ludonimbus";
/// Minutes OpenList keeps directory listings. Its own writes update the
/// cache; folders that other devices write to are refreshed before reading.
const CACHE_MINUTES: u32 = 30;
/// OpenList permission bits of the WebDAV user: mkdir/upload, rename, move,
/// remove, WebDAV read, WebDAV write.
const WEBDAV_PERMISSIONS: u32 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 9);

pub fn download_url() -> String {
    format!("{PROJECT_URL}/releases/download/{VERSION}/{ASSET}")
}

pub fn home() -> PathBuf {
    paths::data_dir().join("openlist")
}

pub fn exe() -> PathBuf {
    home().join(VERSION).join("openlist.exe")
}

/// Credential Manager entry for one of OpenList's passwords (tied to the
/// data folder, so tests with their own `LUDONIMBUS_HOME` don't touch the real ones).
fn key(which: &str) -> String {
    format!("openlist:{which}@{}", home().display())
}

fn data_dir() -> PathBuf {
    home().join("data")
}

fn state_path() -> PathBuf {
    home().join("ludonimbus.json")
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    port: u16,
    pid: Option<u32>,
    /// OpenList creates its admin user on the first start only.
    admin_created: bool,
}

fn load_state() -> State {
    std::fs::read(state_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_state(s: &State) -> Result<()> {
    util::atomic_write(&state_path(), &serde_json::to_vec_pretty(s)?)
}

pub fn installed() -> bool {
    exe().is_file()
}

// ---------------------------------------------------------------------------
// Install

fn sha256_file(p: &Path) -> Result<String> {
    let mut f = std::fs::File::open(p).with_context(|| tr!("无法读取 {}", "Can't read {}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Install OpenList from the official release on GitHub, or from `zip` when
/// the user downloaded it already (GitHub can be slow or blocked). Both are
/// checked against the pinned SHA-256. `progress(done, total)` reports the download.
pub fn install(zip: Option<&Path>, progress: &dyn Fn(u64, u64)) -> Result<()> {
    std::fs::create_dir_all(home())?;
    let archive = match zip {
        Some(z) => {
            if sha256_file(z)? != SHA256 {
                bail!(tr!(
                    "这个文件不是 OpenList {VERSION} 的官方安装包 {ASSET}（SHA-256 不一致）",
                    "This file isn't the official OpenList {VERSION} package {ASSET} (SHA-256 mismatch)"
                ));
            }
            z.to_path_buf()
        }
        None => download(progress)?,
    };
    extract(&archive)?;
    if zip.is_none() {
        let _ = std::fs::remove_file(&archive);
    }
    Ok(())
}

fn download(progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    let url = download_url();
    let fail = |e: String| {
        anyhow!(tr!(
            "无法从 GitHub 下载 OpenList：{e}。也可以在浏览器里下载 {url}，再选择下载好的文件",
            "Couldn't download OpenList from GitHub: {e}. You can also download {url} in a browser and choose the file"
        ))
    };
    let resp = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
        .get(&url)
        .call()
        .map_err(|e| fail(e.to_string()))?;
    let total = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(SIZE);
    let part = home().join(format!("{ASSET}.part"));
    let mut file = std::fs::File::create(&part).with_context(|| tr!("无法写入 {}", "Can't write {}", part.display()))?;
    let mut reader = resp.into_reader().take(SIZE * 2);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| fail(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.flush()?;
    drop(file);
    let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if actual != SHA256 {
        let _ = std::fs::remove_file(&part);
        bail!(tr!(
            "下载的 OpenList 校验失败（SHA-256 不一致），请稍后重试",
            "The downloaded OpenList failed its check (SHA-256 mismatch). Try again later"
        ));
    }
    let dest = home().join(ASSET);
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// Take `openlist.exe` out of the release zip.
fn extract(zip_path: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .context(tr!("OpenList 安装包已损坏", "The OpenList package is damaged"))?;
    let index = (0..zip.len())
        .find(|&i| {
            zip.by_index(i)
                .ok()
                .and_then(|e| e.enclosed_name())
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("openlist.exe")))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!(tr!("OpenList 安装包里没有 openlist.exe", "The OpenList package has no openlist.exe")))?;
    let mut entry = zip.by_index(index)?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    util::atomic_write(&exe(), &bytes)
}

// ---------------------------------------------------------------------------
// Run

/// Set when this process started OpenList (see [`release_if_started_here`]).
static STARTED_HERE: AtomicBool = AtomicBool::new(false);

fn ping(port: u16) -> bool {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(1500))
        .build()
        .get(&format!("http://127.0.0.1:{port}/ping"))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .is_some_and(|s| s.contains("pong"))
}

fn free_port(preferred: u16) -> Result<u16> {
    if preferred != 0 && std::net::TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return Ok(preferred);
    }
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | DETACHED_PROCESS
        cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
}

/// Write OpenList's config.json: only reachable from this PC, on `port`.
fn configure(port: u16) -> Result<()> {
    let path = data_dir().join("config.json");
    if !path.exists() {
        std::fs::create_dir_all(data_dir())?;
        // Any command creates the default config.json and database.
        let mut cmd = Command::new(exe());
        cmd.arg("admin").arg("--data").arg(data_dir()).current_dir(home());
        hidden(&mut cmd);
        let _ = cmd.status();
        if !path.exists() {
            bail!(tr!("OpenList 没有生成配置文件", "OpenList didn't create its configuration file"));
        }
    }
    let mut cfg: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    // Without "force", environment variables would override the file.
    cfg["force"] = json!(true);
    cfg["scheme"]["address"] = json!("127.0.0.1");
    cfg["scheme"]["http_port"] = json!(port);
    cfg["scheme"]["https_port"] = json!(-1);
    util::atomic_write(&path, &serde_json::to_vec_pretty(&cfg)?)
}

fn random_secret() -> String {
    format!("{}{}", ulid::Ulid::new(), ulid::Ulid::new()).to_lowercase()
}

pub(crate) fn admin_password() -> Result<String> {
    if let Some(p) = secrets::load(&key("admin"))? {
        return Ok(p);
    }
    let p = random_secret();
    secrets::store(&key("admin"), &p)?;
    Ok(p)
}

pub struct Running {
    pub port: u16,
    /// This call started the server.
    pub started: bool,
}

/// Start OpenList unless it is already running. Errors are "unavailable"
/// (the cloud counts as offline, so a game still starts).
pub fn ensure_running() -> Result<Running> {
    if !installed() {
        bail!(tr!(
            "夸克网盘需要的 OpenList 组件不见了，请在设置里重新连接夸克网盘",
            "The OpenList component Quark needs is missing. Reconnect Quark in Settings"
        ));
    }
    let st = load_state();
    if st.port != 0 && ping(st.port) {
        return Ok(Running { port: st.port, started: false });
    }
    let _lock = FileLock::acquire(&home().join("start.lock"), Duration::from_secs(40))?;
    let mut st = load_state();
    if st.port != 0 && ping(st.port) {
        return Ok(Running { port: st.port, started: false });
    }
    let port = free_port(st.port)?;
    configure(port)?;
    let mut cmd = Command::new(exe());
    cmd.arg("server").arg("--data").arg(data_dir()).current_dir(home());
    hidden(&mut cmd);
    // Since 4.2 the admin user is only created on the first start, and only
    // with this variable set (otherwise OpenList waits for its web setup page).
    if !st.admin_created {
        cmd.env("OPENLIST_ADMIN_PASSWORD", admin_password()?);
    }
    let child = cmd
        .spawn()
        .map_err(|e| anyhow!(Unavailable(tr!("无法启动 OpenList：{e}", "Couldn't start OpenList: {e}"))))?;
    let pid = child.id();
    let started = Instant::now();
    while !ping(port) {
        if started.elapsed() > Duration::from_secs(25) {
            kill(pid);
            return Err(anyhow!(Unavailable(tr!(
                "OpenList 没有启动成功（可能被杀毒软件拦截了）",
                "OpenList didn't start (antivirus software may have blocked it)"
            ))));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    st.port = port;
    st.pid = Some(pid);
    st.admin_created = true;
    save_state(&st)?;
    STARTED_HERE.store(true, Ordering::SeqCst);
    crate::logger::log("openlist", format!("started on port {port} (pid {pid})"));
    Ok(Running { port, started: true })
}

fn kill(pid: u32) {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/F", "/FI", &format!("PID eq {pid}"), "/FI", "IMAGENAME eq openlist.exe"]);
    hidden(&mut cmd);
    let _ = cmd.status();
}

/// Stop the OpenList server Ludonimbus started (if it is still running).
pub fn stop() {
    let mut st = load_state();
    if let Some(pid) = st.pid.take() {
        kill(pid);
        let _ = save_state(&st);
    }
}

/// For short-lived programs (the Steam wrapper): stop OpenList if this
/// process started it and the Ludonimbus app is not running (the app stops
/// it when it exits).
pub fn release_if_started_here() {
    if !STARTED_HERE.load(Ordering::SeqCst) {
        return;
    }
    let app = ["ludonimbus.exe".to_string(), "ludonimbus-app.exe".to_string()];
    if !crate::process::is_running(&app) {
        stop();
    }
}

/// Stop OpenList and delete its data (with the Quark sign-in) and passwords.
/// The program stays, so connecting again doesn't download it again.
pub fn forget() {
    stop();
    // Give the killed process a moment to release its database.
    std::thread::sleep(Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(data_dir());
    let _ = std::fs::remove_file(state_path());
    let _ = secrets::clear(&key("admin"));
    let _ = secrets::clear(&key("webdav"));
}

// ---------------------------------------------------------------------------
// API

/// OpenList's HTTP API, signed in as admin.
pub struct Api {
    base: String,
    agent: ureq::Agent,
    token: String,
}

/// An answer from OpenList that is not a success. It reads as before; the
/// code is kept for [`is_session_expired`].
#[derive(Debug)]
struct Refused {
    code: i64,
    text: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text)
    }
}
impl std::error::Error for Refused {}

/// OpenList answers HTTP 200 with `{ code, message, data }`.
fn unwrap_reply(v: Value, what: &str) -> Result<Value> {
    if v["code"].as_i64() == Some(200) {
        return Ok(v["data"].clone());
    }
    Err(anyhow!(Refused {
        code: v["code"].as_i64().unwrap_or(0),
        text: format!("OpenList {what}: {}", v["message"].as_str().unwrap_or("?")),
    }))
}

/// Whether OpenList turned a call down because its own sign-in (the admin
/// token, which lasts 48 hours) is no longer valid. Code 401 is only ever
/// OpenList's own session; a Quark sign-in that stopped working comes back
/// with another code (see [`is_login_error`]).
pub fn is_session_expired(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Refused>().is_some_and(|r| r.code == 401))
}

impl Api {
    pub fn login(port: u16) -> Result<Api> {
        Api::login_as(port, &admin_password()?)
    }

    /// Sign in with a known admin password (see [`crate::cloud::quark`]).
    pub fn login_as(port: u16, password: &str) -> Result<Api> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .build();
        let base = format!("http://127.0.0.1:{port}");
        let v: Value = agent
            .post(&format!("{base}/api/auth/login"))
            .send_json(json!({ "username": "admin", "password": password }))
            .map_err(|e| anyhow!(Unavailable(format!("OpenList: {e}"))))?
            .into_json()?;
        let token = unwrap_reply(v, "login")?["token"]
            .as_str()
            .ok_or_else(|| anyhow!("OpenList login: no token"))?
            .to_string();
        Ok(Api { base, agent, token })
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let req = self.agent.request(method, &format!("{}{path}", self.base)).set("Authorization", &self.token);
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        let v: Value = res.map_err(|e| anyhow!(Unavailable(format!("OpenList {path}: {e}"))))?.into_json()?;
        unwrap_reply(v, path)
    }

    /// Any `/api/fs/…` call (`list`, `get`, `copy`, `remove`…) on the whole
    /// mounted drive, for checks and game backups that use files already in
    /// the drive (docs/plan-game-backup.md, section 10).
    pub fn fs(&self, op: &str, body: Value) -> Result<Value> {
        self.call("POST", &format!("/api/fs/{op}"), Some(body))
    }

    /// Upload a small file to `path` through OpenList's own API.
    pub fn put_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let encoded = percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string();
        let v: Value = self
            .agent
            .put(&format!("{}/api/fs/put", self.base))
            .set("Authorization", &self.token)
            .set("File-Path", &encoded)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(data)
            .map_err(|e| anyhow!(Unavailable(format!("OpenList /api/fs/put: {e}"))))?
            .into_json()?;
        unwrap_reply(v, "/api/fs/put").map(|_| ())
    }

    /// The storage mounted at `mount`, if any.
    pub fn storage(&self, mount: &str) -> Result<Option<Value>> {
        let list = self.call("GET", "/api/admin/storage/list", None)?;
        Ok(list["content"]
            .as_array()
            .and_then(|a| a.iter().find(|s| s["mount_path"].as_str() == Some(mount)).cloned()))
    }

    /// Create or update the storage at `mount` and wait until it works.
    /// Returns OpenList's status text when it doesn't.
    pub fn put_storage(&self, mount: &str, driver: &str, addition: Value) -> Result<()> {
        let mut body = self.storage(mount)?.unwrap_or_else(|| {
            json!({
                "mount_path": mount, "order": 0, "remark": "Ludonimbus",
                "web_proxy": true, "webdav_policy": "native_proxy", "down_proxy_url": "",
            })
        });
        let update = body.get("id").is_some();
        body["driver"] = json!(driver);
        body["addition"] = json!(addition.to_string());
        body["cache_expiration"] = json!(CACHE_MINUTES);
        body["disabled"] = json!(false);
        let path = if update { "/api/admin/storage/update" } else { "/api/admin/storage/create" };
        self.call("POST", path, Some(body))?;
        let status = self.storage(mount)?.and_then(|s| s["status"].as_str().map(str::to_string)).unwrap_or_default();
        if status != "work" {
            bail!("{status}");
        }
        Ok(())
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        match self.call("POST", "/api/fs/mkdir", Some(json!({ "path": path }))) {
            Err(e) if format!("{e}").to_lowercase().contains("exist") => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// Delete a file or folder (by its full OpenList path).
    pub fn remove_path(&self, path: &str) -> Result<()> {
        let (dir, name) = path.trim_end_matches('/').rsplit_once('/').unwrap_or(("/", path));
        let dir = if dir.is_empty() { "/" } else { dir };
        self.call("POST", "/api/fs/remove", Some(json!({ "dir": dir, "names": [name] }))).map(|_| ())
    }

    /// List `path` straight from the drive (and refresh OpenList's cache).
    pub fn refresh(&self, path: &str) -> Result<()> {
        self.call(
            "POST",
            "/api/fs/list",
            Some(json!({ "path": path, "password": "", "page": 1, "per_page": 1, "refresh": true })),
        )
        .map(|_| ())
    }

    /// Create (or reset) the WebDAV-only user limited to `base_path`, with a
    /// new random password kept in the Credential Manager.
    pub fn ensure_webdav_user(&self, base_path: &str) -> Result<()> {
        let password = random_secret();
        let users = self.call("GET", "/api/admin/user/list", None)?;
        let existing = users["content"]
            .as_array()
            .and_then(|a| a.iter().find(|u| u["username"].as_str() == Some(WEBDAV_USER)).cloned());
        let mut body = existing.clone().unwrap_or_else(|| json!({ "username": WEBDAV_USER, "role": 0 }));
        body["password"] = json!(password);
        body["base_path"] = json!(base_path);
        body["permission"] = json!(WEBDAV_PERMISSIONS);
        body["disabled"] = json!(false);
        let path = if existing.is_some() { "/api/admin/user/update" } else { "/api/admin/user/create" };
        self.call("POST", path, Some(body))?;
        secrets::store(&key("webdav"), &password)
    }
}

/// The WebDAV user's password.
pub fn webdav_password() -> Result<String> {
    secrets::load(&key("webdav"))?.ok_or_else(|| {
        anyhow!(tr!(
            "找不到夸克网盘的连接信息，请在设置里重新连接夸克网盘",
            "The Quark connection details are missing. Reconnect Quark in Settings"
        ))
    })
}

pub const fn webdav_user() -> &'static str {
    WEBDAV_USER
}

/// True for OpenList / Quark errors that mean the Quark sign-in no longer works.
pub fn is_login_error(e: &str) -> bool {
    let e = e.to_lowercase();
    ["require login", "not login", "login first", "401", "guest", "cookie"].iter().any(|k| e.contains(k))
}

/// Mount Quark with the cookie of a signed-in browser, create `folder` and
/// the WebDAV user limited to it.
pub fn connect_quark(cookie: &str, folder: &str) -> Result<()> {
    let run = ensure_running()?;
    let api = Api::login(run.port)?;
    let addition = json!({
        "cookie": cookie, "root_folder_id": "0", "order_by": "none", "order_direction": "asc",
        "use_transcoding_address": false, "only_list_video_file": false,
    });
    api.put_storage(QUARK_MOUNT, "Quark", addition).map_err(|e| {
        anyhow!(tr!(
            "OpenList 无法用这个登录信息访问夸克网盘：{e}",
            "OpenList couldn't open Quark with this sign-in: {e}"
        ))
    })?;
    let base = format!("{QUARK_MOUNT}/{}", folder.trim_matches('/'));
    api.mkdir(&base)?;
    api.ensure_webdav_user(&base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webdav_permissions_match_openlist_bits() {
        assert_eq!(WEBDAV_PERMISSIONS, 952);
    }

    #[test]
    fn login_errors() {
        assert!(is_login_error("failed to list objs: require login [guest]"));
        assert!(!is_login_error("object not found"));
    }

    #[test]
    fn an_expired_openlist_session_is_told_apart() {
        let expired = unwrap_reply(json!({ "code": 401, "message": "token is expired", "data": null }), "/api/fs/list").unwrap_err();
        assert!(is_session_expired(&expired));
        assert!(is_session_expired(&expired.context("refresh")));
        // It reads as before, and not like a Quark sign-in that stopped working.
        let text = format!("{:#}", unwrap_reply(json!({ "code": 401, "message": "token is expired" }), "/api/fs/list").unwrap_err());
        assert_eq!(text, "OpenList /api/fs/list: token is expired");
        assert!(!is_login_error(&text));

        let quark = unwrap_reply(json!({ "code": 500, "message": "failed to list objs: require login [guest]" }), "/api/fs/list").unwrap_err();
        assert!(!is_session_expired(&quark));
        assert!(is_login_error(&format!("{quark:#}")));
        assert!(!is_session_expired(&anyhow!("token is expired")));
        assert_eq!(unwrap_reply(json!({ "code": 200, "data": { "a": 1 } }), "x").unwrap(), json!({ "a": 1 }));
    }
}
