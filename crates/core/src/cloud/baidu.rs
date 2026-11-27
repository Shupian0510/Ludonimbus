//! Baidu Netdisk (百度网盘) through the official open platform (xpan API).
//!
//! Sign-in uses the OAuth device-code flow: Ludonimbus shows a code and a QR
//! code, the user approves in the browser or the Baidu Netdisk app, and no
//! redirect URI is needed. The publisher registers one app on
//! pan.baidu.com/union and ships its AppKey / SecretKey with releases
//! (`LUDONIMBUS_BAIDU_APP_KEY` / `LUDONIMBUS_BAIDU_SECRET_KEY` /
//! `LUDONIMBUS_BAIDU_APP_NAME` at build time), see docs/baidu-netdisk-setup.md.
//!
//! An app may only touch `/apps/<app name>/` (shown to users as
//! 「我的应用数据/<app name>」), which is the root of this store; paths are
//! never allowed to climb out of it (`..` is dropped).
//!
//! Users may sign in with their own app instead (「使用自己的百度应用」).
//! Its SecretKey is kept in the system credential store under
//! [`app_secret_user`], never in `config.json`.
//!
//! Tokens: the access token lives 30 days, the refresh token 10 years but is
//! single-use, so refreshes are serialized across processes with a file lock
//! and the new pair is saved right away.

use super::store::{join, Entry, Store};
use super::Unavailable;
use crate::lockfile::FileLock;
use crate::{logger, paths};
use anyhow::{anyhow, bail, Context, Result};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const KEYRING_USER: &str = "baidu-pan-token";
const SCOPE: &str = "basic,netdisk";
/// Fixed slice size for regular accounts (members may use larger slices,
/// 4 MB works for everyone).
const SLICE: usize = 4 * 1024 * 1024;
const UA: &str = "pan.baidu.com";

/// AppKey / SecretKey of the registered Baidu app.
#[derive(Debug, Clone)]
pub struct BaiduClient {
    pub app_key: String,
    pub secret_key: String,
    pub app_name: String,
}

impl BaiduClient {
    /// Configured values win; otherwise the ones built into the release. A
    /// configured AppKey never borrows the built-in SecretKey.
    pub fn resolve(app_key: &str, secret_key: &str, app_name: &str) -> Option<Self> {
        let pick = |v: &str, builtin: Option<&str>| {
            let v = v.trim();
            if v.is_empty() {
                builtin.unwrap_or("").trim().to_string()
            } else {
                v.to_string()
            }
        };
        let own = !app_key.trim().is_empty();
        let app_key = pick(app_key, option_env!("LUDONIMBUS_BAIDU_APP_KEY"));
        let secret_key = if own { secret_key.trim().to_string() } else { pick(secret_key, option_env!("LUDONIMBUS_BAIDU_SECRET_KEY")) };
        // The app name is one folder under /apps.
        let mut app_name = pick(app_name, option_env!("LUDONIMBUS_BAIDU_APP_NAME")).replace(['/', '\\'], "");
        if app_name.is_empty() || app_name == "." || app_name == ".." {
            app_name = "Ludonimbus".into();
        }
        (!app_key.is_empty() && !secret_key.is_empty()).then_some(BaiduClient { app_key, secret_key, app_name })
    }

    pub fn has_builtin() -> bool {
        option_env!("LUDONIMBUS_BAIDU_APP_KEY").is_some_and(|s| !s.is_empty()) && option_env!("LUDONIMBUS_BAIDU_SECRET_KEY").is_some_and(|s| !s.is_empty())
    }

    pub fn builtin_app_name() -> String {
        option_env!("LUDONIMBUS_BAIDU_APP_NAME").filter(|s| !s.trim().is_empty()).unwrap_or("Ludonimbus").trim().to_string()
    }
}

/// Server addresses (overridden by the tests).
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub oauth: String,
    pub pan: String,
    pub pcs: String,
    /// Shortest pause between device-code polls (Baidu asks for > 5 s).
    pub min_poll: Duration,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            oauth: "https://openapi.baidu.com".into(),
            pan: "https://pan.baidu.com".into(),
            pcs: "https://d.pcs.baidu.com".into(),
            min_poll: Duration::from_secs(6),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl Token {
    fn from_json(v: &Value) -> Option<Token> {
        Some(Token {
            access_token: v["access_token"].as_str()?.to_string(),
            refresh_token: v["refresh_token"].as_str()?.to_string(),
            expires_at: chrono::Utc::now().timestamp() + v["expires_in"].as_i64().unwrap_or(2_592_000),
        })
    }

    fn fresh(&self) -> bool {
        chrono::Utc::now().timestamp() < self.expires_at - 86_400
    }
}

/// Credential name of the SecretKey of the user's own app with `app_key`.
pub fn app_secret_user(app_key: &str) -> String {
    format!("baidu-app-secret:{}", app_key.trim())
}

/// SecretKey of the user's own app: `legacy` (from a `config.json` written
/// before 0.6.3) when set, else the one in the credential store. Empty for
/// the built-in app.
pub fn saved_app_secret(app_key: &str, legacy: &str) -> Result<String> {
    if !legacy.trim().is_empty() {
        return Ok(legacy.trim().to_string());
    }
    if app_key.trim().is_empty() {
        return Ok(String::new());
    }
    Ok(super::secrets::load(&app_secret_user(app_key))?.unwrap_or_default())
}

pub fn load_token() -> Result<Option<Token>> {
    match super::secrets::load(KEYRING_USER)? {
        Some(s) => Ok(serde_json::from_str(&s).ok()),
        None => Ok(None),
    }
}

pub fn store_token(t: &Token) -> Result<()> {
    super::secrets::store(KEYRING_USER, &serde_json::to_string(t)?)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout.max(Duration::from_secs(20)))
        .timeout_write(timeout.max(Duration::from_secs(20)))
        .user_agent(UA)
        .build()
}

fn bulk_agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(timeout).timeout_read(Duration::from_secs(300)).timeout_write(Duration::from_secs(300)).user_agent(UA).build()
}

/// JSON body of a response, also for HTTP error statuses (Baidu often puts
/// its error code in a 4xx body).
fn json_of(res: Result<ureq::Response, ureq::Error>) -> Result<Value> {
    match res {
        Ok(r) => {
            let text = r.into_string().map_err(|e| anyhow!(Unavailable(e.to_string())))?;
            serde_json::from_str(&text).with_context(|| {
                crate::tr!("百度网盘返回了无法识别的内容：{}", "Baidu Netdisk returned something unexpected: {}", text.chars().take(200).collect::<String>())
            })
        }
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            match serde_json::from_str::<Value>(&text) {
                Ok(v) if v.is_object() => Ok(v),
                _ if code >= 500 => Err(anyhow!(Unavailable(crate::tr!("百度网盘服务器错误 {code}", "Baidu Netdisk server error {code}")))),
                _ => Err(anyhow!(crate::tr!(
                    "百度网盘返回错误 {code}：{}",
                    "Baidu Netdisk returned error {code}: {}",
                    text.chars().take(200).collect::<String>()
                ))),
            }
        }
        Err(ureq::Error::Transport(t)) => Err(anyhow!(Unavailable(t.to_string()))),
    }
}

fn errno(v: &Value) -> i64 {
    v.get("errno").and_then(Value::as_i64).or_else(|| v.get("error_code").and_then(Value::as_i64)).unwrap_or(0)
}

fn is_token_error(code: i64) -> bool {
    matches!(code, -6 | 110 | 20016 | 20017 | 31045)
}

fn is_rate_limited(code: i64) -> bool {
    matches!(code, 20012 | 31034)
}

/// User-facing error for a Baidu error number.
fn errno_error(code: i64, v: &Value, app_name: &str) -> anyhow::Error {
    let msg = ["errmsg", "error_msg", "show_msg"].iter().find_map(|k| v.get(*k).and_then(Value::as_str)).unwrap_or("").to_string();
    match code {
        c if is_token_error(c) => anyhow!(crate::tr!("百度网盘登录已失效（{c}），请在设置里重新连接百度网盘", "The Baidu Netdisk sign-in has expired ({c}). Reconnect Baidu Netdisk in Settings")),
        c if is_rate_limited(c) => anyhow!(Unavailable(crate::tr!("百度网盘请求太频繁（{c}），稍后会自动重试", "Too many requests to Baidu Netdisk ({c}); retrying automatically later"))),
        6 => anyhow!(Unavailable(crate::tr!("百度网盘暂时不允许访问用户数据（6），请 10 分钟后再试", "Baidu Netdisk doesn't allow access to user data right now (6). Try again in 10 minutes"))),
        -1 => anyhow!(crate::tr!("百度网盘会员权益已过期（-1）", "Your Baidu Netdisk membership has expired (-1)")),
        -10 => anyhow!(crate::tr!("百度网盘空间不足（-10）", "Not enough space on Baidu Netdisk (-10)")),
        20011 => anyhow!(crate::tr!("这个百度网盘应用还在审核中，只有前 10 个授权的用户可以使用（20011）", "This Baidu Netdisk app is still under review; only its first 10 users can use it (20011)")),
        20013 => anyhow!(crate::tr!("百度网盘应用没有这个接口的权限（20013），请在开放平台申请对应接口", "The Baidu Netdisk app has no permission for this API (20013). Apply for it on the open platform")),
        20020 | 31064 | -7 => anyhow!(crate::tr!(
            "百度网盘不允许访问这个位置（{code}）。Ludonimbus 只能读写「我的应用数据/{app_name}」，请确认应用名称和开放平台里登记的一致",
            "Baidu Netdisk doesn't allow access to this location ({code}). Ludonimbus can only use \"我的应用数据/{app_name}\"; check that the app name matches the one registered on the open platform"
        )),
        31326 => anyhow!(crate::tr!("百度网盘拒绝了下载请求（31326 防盗链）", "Baidu Netdisk refused the download (31326, hotlink protection)")),
        c => anyhow!(crate::tr!("百度网盘返回错误 {c}{}", "Baidu Netdisk returned error {c}{}", if msg.is_empty() { String::new() } else { crate::tr!("：{msg}", ": {msg}") })),
    }
}

// ---------------------------------------------------------------------------
// Device-code sign-in

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    #[serde(skip)]
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub qrcode_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

impl DeviceCode {
    /// Page to open on this PC; the code is shown in the app as well in case
    /// the page does not pick it up.
    pub fn browser_url(&self) -> String {
        let sep = if self.verification_url.contains('?') { '&' } else { '?' };
        format!("{}{sep}code={}", self.verification_url, self.user_code)
    }
}

pub fn start_device(client: &BaiduClient, ep: &Endpoints) -> Result<DeviceCode> {
    let url = url::Url::parse_with_params(
        &format!("{}/oauth/2.0/device/code", ep.oauth),
        &[("response_type", "device_code"), ("client_id", client.app_key.as_str()), ("scope", SCOPE)],
    )?;
    let v = json_of(agent(Duration::from_secs(20)).get(url.as_str()).call())?;
    if let Some(err) = v["error"].as_str() {
        bail!(crate::tr!("百度网盘授权失败：{err} {}", "Baidu Netdisk authorization failed: {err} {}", v["error_description"].as_str().unwrap_or("")));
    }
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let device_code = s("device_code");
    if device_code.is_empty() {
        bail!(crate::tr!("百度网盘没有返回设备码：{v}", "Baidu Netdisk returned no device code: {v}"));
    }
    Ok(DeviceCode {
        device_code,
        user_code: s("user_code"),
        verification_url: if s("verification_url").is_empty() { format!("{}/device", ep.oauth) } else { s("verification_url") },
        qrcode_url: s("qrcode_url"),
        expires_in: v["expires_in"].as_u64().unwrap_or(300),
        interval: v["interval"].as_u64().unwrap_or(5),
    })
}

/// Errors that end the device flow; anything else is treated as "not yet".
fn fatal_device_error(err: &str) -> bool {
    matches!(err, "invalid_client" | "unauthorized_client" | "unsupported_grant_type" | "access_denied" | "authorization_declined" | "expired_token")
}

/// Wait until the user approves (or the code expires / `cancel` is set).
pub fn poll_device(client: &BaiduClient, ep: &Endpoints, code: &DeviceCode, cancel: &AtomicBool) -> Result<Token> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in.max(30));
    let mut wait = Duration::from_secs(code.interval).max(ep.min_poll);
    let http = agent(Duration::from_secs(20));
    let mut last = String::new();
    loop {
        let start = Instant::now();
        while start.elapsed() < wait {
            if cancel.load(Ordering::SeqCst) {
                bail!(crate::tr!("已取消百度网盘登录", "Baidu Netdisk sign-in cancelled"));
            }
            std::thread::sleep(Duration::from_millis(100).min(wait));
        }
        if Instant::now() > deadline {
            bail!(crate::tr!(
                "等待百度网盘授权超时，请重新点击连接{}",
                "Timed out waiting for Baidu Netdisk authorization. Click Connect again{}",
                if last.is_empty() { String::new() } else { crate::tr!("（{last}）", " ({last})") }
            ));
        }
        let url = url::Url::parse_with_params(
            &format!("{}/oauth/2.0/token", ep.oauth),
            &[
                ("grant_type", "device_token"),
                ("code", code.device_code.as_str()),
                ("client_id", client.app_key.as_str()),
                ("client_secret", client.secret_key.as_str()),
            ],
        )?;
        let v = match json_of(http.get(url.as_str()).call()) {
            Ok(v) => v,
            Err(e) if super::is_unavailable(&e) => {
                last = format!("{e:#}");
                continue;
            }
            Err(e) => return Err(e),
        };
        if let Some(t) = Token::from_json(&v) {
            return Ok(t);
        }
        let err = v["error"].as_str().unwrap_or("").to_string();
        let desc = v["error_description"].as_str().unwrap_or("").to_string();
        if err.is_empty() {
            bail!(crate::tr!("百度网盘返回了无法识别的授权结果：{v}", "Baidu Netdisk returned an unexpected authorization result: {v}"));
        }
        if fatal_device_error(&err) {
            bail!(match err.as_str() {
                "expired_token" => crate::tr!("授权码已过期，请重新点击连接", "The code has expired. Click Connect again"),
                "access_denied" | "authorization_declined" => crate::tr!("你拒绝了授权", "You declined the authorization"),
                _ => crate::tr!("百度网盘授权失败：{err} {desc}", "Baidu Netdisk authorization failed: {err} {desc}"),
            });
        }
        if err == "slow_down" {
            wait += Duration::from_secs(5);
        }
        if last != err {
            logger::log("baidu", format!("device poll: {err} {desc}"));
        }
        last = err;
    }
}

fn refresh(client: &BaiduClient, ep: &Endpoints, refresh_token: &str) -> Result<Token> {
    let url = url::Url::parse_with_params(
        &format!("{}/oauth/2.0/token", ep.oauth),
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client.app_key.as_str()),
            ("client_secret", client.secret_key.as_str()),
        ],
    )?;
    let v = json_of(agent(Duration::from_secs(20)).get(url.as_str()).call())?;
    Token::from_json(&v).ok_or_else(|| {
        logger::log("baidu", format!("refresh failed: {v}"));
        anyhow!(crate::tr!("百度网盘登录已失效，请在设置里重新连接百度网盘", "The Baidu Netdisk sign-in has expired. Reconnect Baidu Netdisk in Settings"))
    })
}

/// `(name, vip_type)` of the signed-in account.
pub fn user_info(ep: &Endpoints, token: &Token) -> Result<(String, i64)> {
    let url = url::Url::parse_with_params(
        &format!("{}/rest/2.0/xpan/nas", ep.pan),
        &[("method", "uinfo"), ("vip_version", "v2"), ("access_token", token.access_token.as_str())],
    )?;
    let v = json_of(agent(Duration::from_secs(20)).get(url.as_str()).call())?;
    if errno(&v) != 0 {
        bail!(crate::tr!("无法读取百度网盘账号信息（{}）", "Couldn't read the Baidu Netdisk account ({})", errno(&v)));
    }
    let name = v["netdisk_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or(v["baidu_name"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| crate::tr!("百度网盘", "Baidu Netdisk"));
    Ok((name, v["vip_type"].as_i64().unwrap_or(0)))
}

// ---------------------------------------------------------------------------
// Store

/// Where refreshed tokens go.
pub enum TokenSink {
    /// The Windows Credential Manager (normal use).
    Keyring,
    /// Memory only (connection tests before saving, unit tests).
    Memory,
}

pub struct BaiduPan {
    client: BaiduClient,
    ep: Endpoints,
    root: String,
    token: Mutex<Token>,
    sink: TokenSink,
    agent: ureq::Agent,
    bulk: ureq::Agent,
    known_dirs: Mutex<HashSet<String>>,
    /// Full path → fs_id, filled by listings (downloads need the id).
    fs_ids: Mutex<HashMap<String, u64>>,
    upload_host: Mutex<Option<(String, Instant)>>,
}

impl BaiduPan {
    pub fn new(client: BaiduClient, ep: Endpoints, token: Token, sink: TokenSink, timeout: Duration) -> Self {
        BaiduPan {
            root: format!("/apps/{}", client.app_name),
            client,
            ep,
            token: Mutex::new(token),
            sink,
            agent: agent(timeout),
            bulk: bulk_agent(timeout),
            known_dirs: Mutex::new(HashSet::new()),
            fs_ids: Mutex::new(HashMap::new()),
            upload_host: Mutex::new(None),
        }
    }

    pub fn token(&self) -> Token {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The app folder (`/apps/<app name>`), the root of this store.
    pub fn app_root(&self) -> &str {
        &self.root
    }

    /// Any xpan call with full paths (`GET` with `params`, or `POST` with
    /// `form`), for checks and game backups that use files already in the
    /// netdisk (docs/plan-game-backup.md, section 10). Error numbers in
    /// `expected` come back as `Ok` for the caller to inspect.
    pub fn xpan(&self, endpoint: &str, params: &[(&str, &str)], form: Option<&[(&str, &str)]>, expected: &[i64]) -> Result<Value> {
        // A form changes something (copy, move…) and is not sent twice.
        self.call_with(form.is_none(), expected, |t| {
            let url = self.api_url(endpoint, params, t);
            match form {
                Some(f) => self.agent.post(&url).send_form(f),
                None => self.agent.get(&url).call(),
            }
        })
    }

    /// Absolute netdisk path below the app folder. `.` and `..` segments
    /// are dropped, so no path can point outside 「我的应用数据/<app name>」.
    fn full(&self, path: &str) -> String {
        let rel = join(&[path]).split('/').filter(|s| *s != "." && *s != "..").collect::<Vec<_>>().join("/");
        if rel.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{rel}", self.root)
        }
    }

    /// A valid access token, refreshing it when it expires within a day (or
    /// always when `force`).
    fn access_token(&self, force: bool) -> Result<String> {
        let current = self.token();
        if current.fresh() && !force {
            return Ok(current.access_token);
        }
        let new = match self.sink {
            TokenSink::Memory => refresh(&self.client, &self.ep, &current.refresh_token)?,
            TokenSink::Keyring => {
                // Another process (the Steam wrapper, the tray app) may be
                // refreshing at the same time; refresh tokens are single-use.
                let _lock = FileLock::acquire(&paths::data_dir().join("baidu-token.lock"), Duration::from_secs(30))?;
                match load_token()? {
                    Some(saved) if saved.access_token != current.access_token && saved.fresh() => saved,
                    saved => {
                        let rt = saved.map(|s| s.refresh_token).unwrap_or(current.refresh_token.clone());
                        let t = refresh(&self.client, &self.ep, &rt)?;
                        store_token(&t)?;
                        t
                    }
                }
            }
        };
        let at = new.access_token.clone();
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = new;
        Ok(at)
    }

    /// Run an API call with the access token, refreshing once on token
    /// errors and retrying briefly when rate limited. Error numbers listed in
    /// `expected` are returned as `Ok` for the caller to inspect.
    fn call(&self, expected: &[i64], f: impl Fn(&str) -> Result<ureq::Response, ureq::Error>) -> Result<Value> {
        self.call_with(true, expected, f)
    }

    /// [`Self::call`]. With `again`, a call the server turns away with an
    /// HTTP error for the moment (429, 5xx) is also sent again, as long as it
    /// asks for a short wait ([`super::gdrive::retry_wait`]): only for calls
    /// that do the same when they arrive twice. A rate-limit error number
    /// means the call was not carried out, so that is retried for any call.
    fn call_with(&self, again: bool, expected: &[i64], f: impl Fn(&str) -> Result<ureq::Response, ureq::Error>) -> Result<Value> {
        let mut forced = false;
        let mut tries = 0;
        let mut busy = 0;
        loop {
            let token = self.access_token(false)?;
            let res = f(&token);
            if let Err(ureq::Error::Status(code, r)) = &res {
                if again && super::gdrive::busy_status(*code) {
                    if let Some(wait) = super::gdrive::retry_wait(busy, r.header("Retry-After"), chrono::Utc::now()) {
                        busy += 1;
                        logger::log("baidu", format!("HTTP {code}; trying again in {} ms", wait.as_millis()));
                        std::thread::sleep(wait);
                        continue;
                    }
                }
            }
            let v = json_of(res)?;
            let code = errno(&v);
            if code == 0 || expected.contains(&code) {
                return Ok(v);
            }
            if is_token_error(code) && !forced {
                forced = true;
                self.access_token(true)?;
                continue;
            }
            if is_rate_limited(code) && tries < 2 {
                tries += 1;
                std::thread::sleep(Duration::from_secs(tries));
                continue;
            }
            return Err(errno_error(code, &v, &self.client.app_name));
        }
    }

    fn api_url(&self, path: &str, params: &[(&str, &str)], token: &str) -> String {
        let mut all: Vec<(&str, &str)> = params.to_vec();
        all.push(("access_token", token));
        url::Url::parse_with_params(&format!("{}{path}", self.ep.pan), &all).map(|u| u.to_string()).unwrap_or_default()
    }

    fn list_full(&self, dir: &str) -> Result<Option<Vec<Value>>> {
        let mut out = Vec::new();
        let mut start = 0usize;
        loop {
            let s = start.to_string();
            let v = self.call(&[-9], |t| {
                self.agent
                    .get(&self.api_url("/rest/2.0/xpan/file", &[("method", "list"), ("dir", dir), ("start", &s), ("limit", "1000"), ("web", "0")], t))
                    .call()
            })?;
            if errno(&v) == -9 {
                return Ok(None);
            }
            let page = v["list"].as_array().cloned().unwrap_or_default();
            let n = page.len();
            out.extend(page);
            if n < 1000 {
                return Ok(Some(out));
            }
            start += n;
        }
    }

    fn mkdir(&self, full: &str) -> Result<()> {
        self.call(&[-8, 31061], |t| {
            self.agent.post(&self.api_url("/rest/2.0/xpan/file", &[("method", "create")], t)).send_form(&[("path", full), ("isdir", "1"), ("rtype", "0")])
        })
        .map(|_| ())
    }

    /// Make sure the folders above `path` exist (Baidu does not document
    /// creating them implicitly).
    fn ensure_parents(&self, path: &str) -> Result<()> {
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let parent = segs[..segs.len().saturating_sub(1)].join("/");
        if self.known_dirs.lock().unwrap_or_else(|e| e.into_inner()).contains(&parent) {
            return Ok(());
        }
        if self.list_full(&self.full(&parent))?.is_none() {
            for i in 0..segs.len() {
                let dir = segs[..i].join("/");
                if self.known_dirs.lock().unwrap_or_else(|e| e.into_inner()).contains(&dir) {
                    continue;
                }
                self.mkdir(&self.full(&dir))?;
                self.known_dirs.lock().unwrap_or_else(|e| e.into_inner()).insert(dir);
            }
        }
        let mut known = self.known_dirs.lock().unwrap_or_else(|e| e.into_inner());
        for i in 0..segs.len() {
            known.insert(segs[..i].join("/"));
        }
        Ok(())
    }

    fn upload_host(&self, full: &str, uploadid: &str) -> String {
        if let Some((h, until)) = self.upload_host.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            if Instant::now() < until {
                return h;
            }
        }
        let fallback = self.ep.pcs.clone();
        let res = self.call(&[], |t| {
            let url = url::Url::parse_with_params(
                &format!("{}/rest/2.0/pcs/file", self.ep.pcs),
                &[("method", "locateupload"), ("appid", "250528"), ("access_token", t), ("path", full), ("uploadid", uploadid), ("upload_version", "2.0")],
            )
            .map(|u| u.to_string())
            .unwrap_or_default();
            self.agent.get(&url).call()
        });
        let host = res.ok().and_then(|v| {
            let server = v["servers"].as_array().and_then(|a| a.iter().find_map(|s| s["server"].as_str())).map(str::to_string)?;
            let secs = v["expire"].as_u64().unwrap_or(60).max(30);
            let server = if server.contains("://") { server } else { format!("https://{server}") };
            *self.upload_host.lock().unwrap_or_else(|e| e.into_inner()) = Some((server.clone(), Instant::now() + Duration::from_secs(secs)));
            Some(server)
        });
        host.unwrap_or(fallback)
    }

    fn put(&self, path: &str, body: Body) -> Result<()> {
        if body.len() == 0 {
            bail!(crate::tr!("百度网盘不能上传空文件", "Baidu Netdisk can't upload empty files"));
        }
        self.ensure_parents(path)?;
        let full = self.full(path);
        let sent = self.send(&full, body);
        // Whatever the outcome, the id a listing gave for this path may be
        // gone now: a finished upload (also a rapid one) puts a new file
        // there, and one that failed may have got as far.
        self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&full);
        sent
    }

    /// The upload itself: precreate, the slices the server asks for, create.
    fn send(&self, full: &str, body: Body) -> Result<()> {
        let blocks = body.blocks()?;
        let block_list = serde_json::to_string(&blocks)?;
        let size = body.len().to_string();
        let pre = self.call(&[], |t| {
            self.agent.post(&self.api_url("/rest/2.0/xpan/file", &[("method", "precreate")], t)).send_form(&[
                ("path", full),
                ("size", size.as_str()),
                ("isdir", "0"),
                ("autoinit", "1"),
                ("rtype", "3"),
                ("block_list", block_list.as_str()),
            ])
        })?;
        if pre["return_type"].as_i64() == Some(2) {
            // The server already had this content ("rapid upload").
            return Ok(());
        }
        let uploadid = pre["uploadid"].as_str().unwrap_or("").to_string();
        if uploadid.is_empty() {
            bail!(crate::tr!("百度网盘没有返回上传 id：{pre}", "Baidu Netdisk returned no upload id: {pre}"));
        }
        let todo: Vec<usize> = match pre["block_list"].as_array() {
            Some(a) => a.iter().filter_map(|x| x.as_u64().map(|n| n as usize)).collect(),
            None => (0..blocks.len()).collect(),
        };
        let host = self.upload_host(full, &uploadid);
        for i in todo {
            let chunk = body
                .slice(i)?
                .ok_or_else(|| anyhow!(crate::tr!("百度网盘要求了不存在的分片 {i}", "Baidu Netdisk asked for a slice that doesn't exist: {i}")))?;
            let seq = i.to_string();
            let boundary = format!("----Ludonimbus{}", ulid::Ulid::new());
            let mut form = Vec::with_capacity(chunk.len() + 256);
            form.extend_from_slice(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"blob\"\r\nContent-Type: application/octet-stream\r\n\r\n")
                    .as_bytes(),
            );
            form.extend_from_slice(&chunk);
            drop(chunk);
            form.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            let v = self.call(&[], |t| {
                let url = url::Url::parse_with_params(
                    &format!("{host}/rest/2.0/pcs/superfile2"),
                    &[
                        ("method", "upload"),
                        ("type", "tmpfile"),
                        ("access_token", t),
                        ("path", full),
                        ("uploadid", uploadid.as_str()),
                        ("partseq", seq.as_str()),
                    ],
                )
                .map(|u| u.to_string())
                .unwrap_or_default();
                self.bulk.post(&url).set("Content-Type", &format!("multipart/form-data; boundary={boundary}")).send_bytes(&form)
            })?;
            if let Some(md5) = v["md5"].as_str() {
                if !md5.eq_ignore_ascii_case(&blocks[i]) {
                    bail!(crate::tr!("百度网盘收到的分片 {i} 校验值不一致，请重试", "Baidu Netdisk received slice {i} with a wrong checksum. Please retry"));
                }
            }
        }
        // Not sent twice: a create that went through used up the upload id.
        self.call_with(false, &[], |t| {
            self.agent.post(&self.api_url("/rest/2.0/xpan/file", &[("method", "create")], t)).send_form(&[
                ("path", full),
                ("size", size.as_str()),
                ("isdir", "0"),
                ("rtype", "3"),
                ("uploadid", uploadid.as_str()),
                ("block_list", block_list.as_str()),
            ])
        })?;
        Ok(())
    }

    /// fs_id of a file, from the last listing or by listing its folder.
    fn fs_id(&self, full: &str) -> Result<Option<u64>> {
        if let Some(id) = self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).get(full) {
            return Ok(Some(*id));
        }
        let (dir, _) = full.rsplit_once('/').unwrap_or(("/", full));
        let Some(items) = self.list_full(dir)? else { return Ok(None) };
        let mut ids = self.fs_ids.lock().unwrap_or_else(|e| e.into_inner());
        for it in &items {
            if let (Some(p), Some(id)) = (it["path"].as_str(), it["fs_id"].as_u64()) {
                ids.insert(p.to_string(), id);
            }
        }
        Ok(ids.get(full).copied())
    }

    /// Open a download stream; `None` when the file does not exist.
    ///
    /// The id the last listing gave is gone when the file was replaced since
    /// (by this device or another one), or deleted. Which of the two it was
    /// only a listing can tell, so the folder is listed again and the new id
    /// tried. The file is reported missing only when that listing doesn't
    /// have it: "missing" makes a sync decide things.
    fn open(&self, path: &str, agent: &ureq::Agent) -> Result<Option<ureq::Response>> {
        let full = self.full(path);
        let known = self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).get(&full).copied();
        if let Some(id) = known {
            if let Some(r) = self.open_id(id, agent)? {
                return Ok(Some(r));
            }
            self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&full);
        }
        let Some(id) = self.fs_id(&full)? else { return Ok(None) };
        match self.open_id(id, agent)? {
            Some(r) => Ok(Some(r)),
            None => {
                self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&full);
                Err(anyhow!(Unavailable(crate::tr!(
                    "百度网盘列出了 {path}，但没有给出下载地址，稍后再试",
                    "Baidu Netdisk lists {path} but gave no download link for it; try again later"
                ))))
            }
        }
    }

    /// Open the download of the file with this id; `None` when Baidu has no
    /// such file (any more).
    fn open_id(&self, id: u64, agent: &ureq::Agent) -> Result<Option<ureq::Response>> {
        let ids = format!("[{id}]");
        let v = self.call(&[-9, 31190, -3], |t| {
            self.agent.get(&self.api_url("/rest/2.0/xpan/multimedia", &[("method", "filemetas"), ("fsids", ids.as_str()), ("dlink", "1")], t)).call()
        })?;
        let dlink = if errno(&v) == 0 { v["list"].as_array().and_then(|a| a.first()).and_then(|f| f["dlink"].as_str()).map(str::to_string) } else { None };
        let Some(dlink) = dlink else { return Ok(None) };
        let token = self.access_token(false)?;
        let sep = if dlink.contains('?') { '&' } else { '?' };
        match agent.get(&format!("{dlink}{sep}access_token={token}")).call() {
            Ok(r) => Ok(Some(r)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => {
                let v = json_of(Err(e))?;
                Err(errno_error(errno(&v), &v, &self.client.app_name))
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What [`BaiduPan::put`] sends: bytes already in memory, or a file read a
/// slice at a time, so a big upload never sits in memory whole.
#[derive(Clone, Copy)]
enum Body<'a> {
    Bytes(&'a [u8]),
    /// The file and its length when the upload started.
    File(&'a Path, u64),
}

impl Body<'_> {
    fn len(&self) -> u64 {
        match self {
            Body::Bytes(b) => b.len() as u64,
            Body::File(_, len) => *len,
        }
    }

    fn unreadable(path: &Path) -> String {
        crate::tr!("无法读取 {}", "Can't read {}", path.display())
    }

    /// The MD5 of every 4 MB slice: the block list Baidu wants first.
    fn blocks(&self) -> Result<Vec<String>> {
        match self {
            Body::Bytes(b) => Ok(b.chunks(SLICE).map(|c| hex(&Md5::digest(c))).collect()),
            Body::File(path, len) => {
                let mut f = std::fs::File::open(path).with_context(|| Self::unreadable(path))?;
                let mut buf = vec![0u8; SLICE];
                let mut out = Vec::new();
                let mut left = *len;
                while left > 0 {
                    let n = left.min(SLICE as u64) as usize;
                    // Fails when the file got shorter since the upload began.
                    f.read_exact(&mut buf[..n]).with_context(|| Self::unreadable(path))?;
                    out.push(hex(&Md5::digest(&buf[..n])));
                    left -= n as u64;
                }
                Ok(out)
            }
        }
    }

    /// Slice `i` (4 MB, the last one shorter); `None` past the end.
    fn slice(&self, i: usize) -> Result<Option<Cow<'_, [u8]>>> {
        match self {
            Body::Bytes(b) => Ok(b.chunks(SLICE).nth(i).map(Cow::Borrowed)),
            Body::File(path, len) => {
                let start = i as u64 * SLICE as u64;
                if start >= *len {
                    return Ok(None);
                }
                let mut buf = vec![0u8; (*len - start).min(SLICE as u64) as usize];
                let mut f = std::fs::File::open(path).with_context(|| Self::unreadable(path))?;
                f.seek(SeekFrom::Start(start)).and_then(|_| f.read_exact(&mut buf)).with_context(|| Self::unreadable(path))?;
                Ok(Some(Cow::Owned(buf)))
            }
        }
    }
}

impl Store for BaiduPan {
    fn kind(&self) -> &'static str {
        "baiduPan"
    }

    fn label(&self) -> String {
        crate::tr!("百度网盘 · 我的应用数据/{}", "Baidu Netdisk · 我的应用数据/{}", self.client.app_name)
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        let v = self.call(&[], |t| self.agent.get(&self.api_url("/api/quota", &[("checkfree", "1")], t)).call())?;
        Ok((v["used"].as_u64(), v["total"].as_u64()))
    }

    fn check(&self) -> Result<()> {
        // Validates the token and the access to the app folder.
        self.list_full(&self.root).map(|_| ())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let Some(items) = self.list_full(&self.full(dir))? else { return Ok(Vec::new()) };
        let mut ids = self.fs_ids.lock().unwrap_or_else(|e| e.into_inner());
        Ok(items
            .iter()
            .filter_map(|it| {
                let name = it["server_filename"].as_str()?.to_string();
                let fs_id = it["fs_id"].as_u64().unwrap_or(0);
                if let Some(p) = it["path"].as_str() {
                    ids.insert(p.to_string(), fs_id);
                }
                let size = it["size"].as_u64().unwrap_or(0);
                Some(Entry {
                    is_dir: it["isdir"].as_i64() == Some(1),
                    tag: format!("{fs_id}|{}|{size}|{}", it["server_mtime"].as_i64().unwrap_or(0), it["md5"].as_str().unwrap_or("")),
                    name,
                    size,
                })
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(r) = self.open(path, &self.agent)? else { return Ok(None) };
        let mut buf = Vec::new();
        r.into_reader().read_to_end(&mut buf).map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        Ok(Some(buf))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let r = self.open(path, &self.bulk)?.ok_or_else(|| anyhow!(crate::tr!("云端找不到文件 {path}", "File not found in the cloud: {path}")))?;
        let tmp = dest.with_extension("download");
        let mut out = std::fs::File::create(&tmp).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
        std::io::copy(&mut r.into_reader(), &mut out).map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.put(path, Body::Bytes(data))
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        // Read a slice at a time: a content pack can be as big as the
        // biggest save file it holds.
        let len = std::fs::metadata(src).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", src.display()))?.len();
        self.put(path, Body::File(src, len))
    }

    fn remove(&self, path: &str) -> Result<()> {
        let full = self.full(path);
        let list = serde_json::to_string(&[&full])?;
        // A missing file is reported as -9, or as 12 ("batch failed") with
        // -9 for the item.
        let v = self.call(&[-9, 12], |t| {
            self.agent
                .post(&self.api_url("/rest/2.0/xpan/file", &[("method", "filemanager"), ("opera", "delete")], t))
                .send_form(&[("async", "0"), ("filelist", list.as_str())])
        })?;
        self.fs_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&full);
        let failed = v["info"]
            .as_array()
            .and_then(|a| a.iter().map(errno).find(|&c| c != 0 && c != -9))
            .or_else(|| (errno(&v) == 12 && v["info"].as_array().is_none_or(|a| a.is_empty())).then_some(12));
        match failed {
            Some(c) => Err(errno_error(c, &v, &self.client.app_name)),
            None => Ok(()),
        }
    }

    fn cache_scope(&self) -> Option<String> {
        Some(format!("baidu|{}|{}", self.client.app_key, self.root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_errors() {
        let v = serde_json::json!({});
        assert!(super::super::is_unavailable(&errno_error(31034, &v, "L")));
        assert!(format!("{}", errno_error(20020, &v, "Ludonimbus")).contains("我的应用数据/Ludonimbus"));
        let e = format!("{}", errno_error(-6, &v, "L"));
        assert!(e.contains("重新连接") || e.contains("Reconnect"), "{e}");
        assert_eq!(errno(&serde_json::json!({"error_code": 31326})), 31326);
        assert_eq!(errno(&serde_json::json!({"md5": "x"})), 0);
    }

    #[test]
    fn resolves_client() {
        let c = BaiduClient::resolve("key", "secret", "").unwrap();
        assert_eq!(c.app_name, BaiduClient::builtin_app_name());
        assert_eq!(c.app_key, "key");
        // An own AppKey without its SecretKey is incomplete, whatever is built in.
        assert!(BaiduClient::resolve("key", "", "").is_none());
        assert_eq!(BaiduClient::resolve("key", "secret", "../x").unwrap().app_name, "..x");
        assert_eq!(BaiduClient::resolve("key", "secret", "..").unwrap().app_name, "Ludonimbus");
    }

    #[test]
    fn paths_stay_in_the_app_folder() {
        let client = BaiduClient::resolve("key", "secret", "Ludonimbus").unwrap();
        let token = Token { access_token: "a".into(), refresh_token: "r".into(), expires_at: i64::MAX };
        let pan = BaiduPan::new(client, Endpoints::default(), token, TokenSink::Memory, Duration::from_secs(5));
        assert_eq!(pan.full(""), "/apps/Ludonimbus");
        assert_eq!(pan.full("games/x/snapshots/1.zip"), "/apps/Ludonimbus/games/x/snapshots/1.zip");
        assert_eq!(pan.full("../../我的资源/a.txt"), "/apps/Ludonimbus/我的资源/a.txt");
        assert_eq!(pan.full("games/./../.."), "/apps/Ludonimbus/games");
    }
}
