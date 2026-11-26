//! Cloud storage.
//!
//! Layout 2 (docs/plan-v2.md, section 3; same for every kind of cloud):
//! ```text
//! <root>/format.json
//! <root>/library/<game id>.json                    game records
//! <root>/devices/<device-id>.json
//! <root>/games/<folder>/saves/<line>/<ULID>.json   manifests (+ pins, index.json, trash/)
//! <root>/games/<folder>/packs/<pack>.zip           their content
//! <root>/games/<folder>/game.json                  the save rules, as 0.6 wrote them
//! <root>/games/<folder>/snapshots/<ULID>.zip       (+ .json) what 0.6 wrote, left where it is
//! <root>/games/<folder>/playtime/<device-id>.json
//! <root>/games/<folder>/screenshots/<device-id>/<name>.jpg (+ thumbnails/)
//! ```
//!
//! Every backend, Google Drive included, only implements the small
//! path-based [`store::Store`] trait. [`Cloud`] is what the program talks
//! to: the store, the layout on top of it ([`saves`], [`library`],
//! [`upgrade`]) and what this device remembers of it. Data that is not a
//! save snapshot (playtime, screenshots, ...) goes through [`Cloud::files`].
//!
//! [`repo::Repo`] and [`CloudProvider`] are the layout of 0.6, kept as it
//! was: tests use them to act as a device that has not been updated.

pub mod baidu;
pub mod cached;
pub mod client;
pub mod decide;
pub mod gdrive;
pub mod library;
pub mod line;
pub mod local;
pub mod memory;
pub mod migrate;
pub mod oauth;
pub mod packs;
pub mod quark;
pub mod repo;
pub mod retention;
pub mod s3;
pub mod saves;
pub mod secrets;
pub mod store;
pub mod upgrade;
pub mod v2;
pub mod webdav;

use crate::config::{self, AppConfig, CloudConfig, GameEntry};
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use crate::state;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use store::Store;

pub use client::{own_line_id, wanted_line, Cloud, OpenLine};

/// The cloud cannot be reached right now (offline, timeout, folder missing).
#[derive(Debug)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::tr!("云端暂时无法访问：{}", "The cloud can't be reached right now: {}", self.0))
    }
}
impl std::error::Error for Unavailable {}

/// No cloud provider configured.
#[derive(Debug)]
pub struct NotConfigured;

impl std::fmt::Display for NotConfigured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::tr!("还没有连接云盘", "No cloud connected yet"))
    }
}
impl std::error::Error for NotConfigured {}

pub fn is_unavailable(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Unavailable>().is_some())
}

pub fn is_not_configured(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<NotConfigured>().is_some())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub last_seen: DateTime<Utc>,
    /// The newest cloud layout this device's Ludonimbus knows; 0 in a record
    /// written by 0.6.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub layout: u32,
    /// The save lines this device syncs, and where it is on each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installs: Vec<DeviceInstall>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One game on a device, as other devices need to know it: retention keeps
/// the snapshot each device synced last ([`retention::DeviceBase`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceInstall {
    /// Id of the game record.
    pub game: String,
    pub line: String,
    /// The folder the line is written to: what names the line for good.
    pub dir: String,
    /// The snapshot this device synced last, and when.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<DateTime<Utc>>,
}

/// Game definition stored in the cloud so other devices can recognise it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudGame {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub saves: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub wait_for: Vec<String>,
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    #[serde(default)]
    pub ludusavi_title: Option<String>,
    /// The library group the user chose, so other devices show the game
    /// in the same group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What identifies the game on any platform (`nx:…`, `ludusavi:…`,
    /// `xbox:…`, `name:…`), so other devices can tell versions of the same
    /// game apart from other games (see [`crate::savelib`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    /// The user's "same game as" link ([`GameEntry::game`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
}

impl From<&GameEntry> for CloudGame {
    fn from(g: &GameEntry) -> Self {
        CloudGame {
            key: g.key.clone(),
            name: g.name.clone(),
            adapter: g.adapter.clone(),
            saves: g.saves.clone(),
            exclude: g.exclude.clone(),
            wait_for: g.wait_for.clone(),
            roots: g.roots.clone(),
            ludusavi_title: g.ludusavi_title.clone(),
            source: g.source.clone(),
            ids: crate::savelib::ids(g),
            game: g.game.clone(),
        }
    }
}

/// How long a deleted version stays in 「最近删除」 before it is purged.
pub const TRASH_DAYS: i64 = 30;

/// A version the user deleted: hidden from the history on every device and
/// restorable until it is purged [`TRASH_DAYS`] after `deleted_at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrashedSnapshot {
    pub snapshot: SnapshotSummary,
    pub deleted_at: DateTime<Utc>,
    /// Name of the device it was deleted on.
    #[serde(default)]
    pub deleted_by: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub label: String,
    pub used: Option<u64>,
    pub total: Option<u64>,
}

pub trait CloudProvider: Send + Sync {
    fn kind(&self) -> &'static str;
    fn account(&self) -> Result<AccountInfo>;
    fn list_snapshots(&self, game: &str) -> Result<Vec<SnapshotSummary>>;
    fn get_meta(&self, game: &str, id: &str) -> Result<SnapshotMeta>;
    fn upload(&self, meta: &SnapshotMeta, zip: &Path) -> Result<()>;
    fn download(&self, game: &str, id: &str, dest: &Path) -> Result<()>;
    /// Delete a snapshot for good (retention of automatic snapshots).
    fn delete(&self, game: &str, id: &str) -> Result<()>;
    /// Move a snapshot to 「最近删除」: it disappears from the history on
    /// every device (older Ludonimbus versions included) but its files stay
    /// in the Ludonimbus folder until [`CloudProvider::purge`]. `by` is the
    /// device name shown next to it.
    fn trash(&self, game: &str, id: &str, by: &str) -> Result<()>;
    /// Snapshots in 「最近删除」, newest deletion first.
    fn list_trash(&self, game: &str) -> Result<Vec<TrashedSnapshot>>;
    /// Put a snapshot from 「最近删除」 back into the history.
    fn untrash(&self, game: &str, id: &str) -> Result<()>;
    /// Delete a snapshot in 「最近删除」 for good.
    fn purge(&self, game: &str, id: &str) -> Result<()>;
    /// Update comment / lock flag.
    fn update_meta(&self, meta: &SnapshotMeta) -> Result<()>;
    fn put_device(&self, device: &DeviceInfo) -> Result<()>;
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    fn put_game(&self, game: &CloudGame) -> Result<()>;
    fn list_games(&self) -> Result<Vec<CloudGame>>;
    /// The same cloud folder as path-based storage, for data that is not a
    /// save snapshot (playtime, screenshots, ...). Paths are relative to the
    /// Ludonimbus folder, e.g. `games/<key>/playtime/<device>.json`.
    fn files(&self) -> &dyn Store;
}

pub fn timeout(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.network_timeout_secs.max(3))
}

/// Open the configured cloud. `Ok(None)` when no cloud is configured.
pub fn open(cfg: &AppConfig) -> Result<Option<Cloud>> {
    match &cfg.cloud {
        CloudConfig::None => Ok(None),
        other => Ok(Some(Cloud::new(build_store(other, None, timeout(cfg))?, &cfg.device.id, &cfg.device.name).for_config(cfg))),
    }
}

/// Bring the connected cloud to the current layout if 0.6 wrote it, without
/// waiting for the first sync to do it. Does nothing when there is no cloud
/// or it is up to date; the app calls this in the background when it starts.
pub fn upgrade_if_needed(cfg: &AppConfig) -> Result<()> {
    match open(cfg)? {
        Some(cloud) => cloud.writable(),
        None => Ok(()),
    }
}

impl<T: Store + ?Sized> Store for Box<T> {
    fn kind(&self) -> &'static str {
        (**self).kind()
    }
    fn label(&self) -> String {
        (**self).label()
    }
    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        (**self).quota()
    }
    fn check(&self) -> Result<()> {
        (**self).check()
    }
    fn list(&self, dir: &str) -> Result<Vec<store::Entry>> {
        (**self).list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        (**self).read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        (**self).download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        (**self).write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        (**self).upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        (**self).remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        (**self).cache_scope()
    }
}

/// Build a path-based store. `secret` overrides the saved password / key
/// (used to test new settings before saving them); clouds signed in to in
/// the browser (Google Drive, Baidu Netdisk) always use their saved sign-in.
pub fn build_store(cloud: &CloudConfig, secret: Option<&str>, timeout: Duration) -> Result<Box<dyn Store>> {
    let secret = |missing: &str| -> Result<String> {
        match secret {
            Some(s) => Ok(s.to_string()),
            None => secrets::load_for(cloud)?.ok_or_else(|| anyhow!(crate::tr!("{missing}，请在设置里重新连接", "{missing}. Reconnect in Settings"))),
        }
    };
    Ok(match cloud {
        CloudConfig::LocalFolder { path } => Box::new(local::LocalFolder::new(path.clone())),
        CloudConfig::WebDav { url, username, folder } => Box::new(webdav::WebDav::new(
            url,
            username,
            &secret(&crate::tr!("找不到保存的 WebDAV 密码", "The saved WebDAV password is missing"))?,
            folder,
            timeout,
        )?),
        CloudConfig::S3 {
            endpoint,
            region,
            bucket,
            prefix,
            access_key_id,
            path_style,
        } => Box::new(s3::S3::new(
            endpoint,
            region,
            bucket,
            prefix,
            access_key_id,
            &secret(&crate::tr!("找不到保存的 S3 Secret Key", "The saved S3 Secret Key is missing"))?,
            *path_style,
            timeout,
        )?),
        CloudConfig::BaiduPan {
            app_key,
            secret_key,
            app_name,
            ..
        } => {
            let secret_key = baidu::saved_app_secret(app_key, secret_key)?;
            let client = baidu::BaiduClient::resolve(app_key, &secret_key, app_name)
                .ok_or_else(|| anyhow!(crate::tr!("百度网盘缺少应用的 AppKey / SecretKey，请在设置里填写", "Baidu Netdisk is missing the app's AppKey / SecretKey. Enter them in Settings")))?;
            let token = baidu::load_token()?.ok_or_else(|| anyhow!(crate::tr!("百度网盘登录已失效，请在设置里重新连接", "The Baidu Netdisk sign-in has expired. Reconnect in Settings")))?;
            Box::new(baidu::BaiduPan::new(
                client,
                baidu::Endpoints::default(),
                token,
                baidu::TokenSink::Keyring,
                timeout,
            ))
        }
        CloudConfig::Quark { folder } => Box::new(quark::Quark::open(folder, timeout)?),
        CloudConfig::GoogleDrive {
            client_id,
            client_secret,
            folder,
            ..
        } => {
            let client = oauth::GoogleClient::resolve(client_id, client_secret)
                .ok_or_else(|| anyhow!(crate::tr!("Google Drive 缺少 OAuth Client ID，请在设置里填写", "Google Drive has no OAuth Client ID. Enter one in Settings")))?;
            let token = oauth::load_refresh_token()?
                .ok_or_else(|| anyhow!(crate::tr!("Google Drive 登录已失效，请在设置里重新连接", "The Google Drive sign-in has expired. Reconnect in Settings")))?;
            Box::new(gdrive::GoogleDrive::new(client, token, folder.clone(), timeout))
        }
        CloudConfig::None => return Err(anyhow!(NotConfigured)),
    })
}

/// Test new store settings end to end: reach the folder, write a file, see it
/// in the listing, read it back and delete it.
pub fn probe(cloud: &CloudConfig, secret: Option<&str>, timeout: Duration) -> Result<AccountInfo> {
    probe_store(build_store(cloud, secret, timeout)?.as_ref())
}

/// The end-to-end write / list / read / delete test of [`probe`] on any store.
pub fn probe_store(store: &dyn Store) -> Result<AccountInfo> {
    store.check()?;
    let name = format!(".ludonimbus-test-{}", ulid::Ulid::new().to_string().to_lowercase());
    let data = format!("Ludonimbus connection test {}", Utc::now().to_rfc3339());
    store.write(&name, data.as_bytes()).context(crate::tr!("写入测试文件失败", "Couldn't write the test file"))?;
    let result = (|| -> Result<()> {
        let listed = store.list("").context(crate::tr!("列出文件夹失败", "Couldn't list the folder"))?;
        if !listed.iter().any(|e| e.name == name) {
            bail!(crate::tr!("写入的测试文件没有出现在文件列表里，这个服务器可能不完全兼容", "The test file didn't show up in the folder listing; this server may not be fully compatible"));
        }
        let back = store.read(&name).context(crate::tr!("读取测试文件失败", "Couldn't read the test file back"))?;
        if back.as_deref() != Some(data.as_bytes()) {
            bail!(crate::tr!("读回来的测试文件内容不一致，这个服务器可能不完全兼容", "The test file read back differently; this server may not be fully compatible"));
        }
        Ok(())
    })();
    let removed = store.remove(&name);
    result?;
    removed.context(crate::tr!("删除测试文件失败", "Couldn't delete the test file"))?;
    let (used, total) = store.quota().unwrap_or((None, None));
    Ok(AccountInfo {
        label: store.label(),
        used,
        total,
    })
}

/// Sign in to Baidu Netdisk with the device-code flow and test access to the
/// app folder. `on_code` shows the code / QR code to the user (and opens the
/// browser). Nothing is saved.
pub fn baidu_sign_in(
    client: &baidu::BaiduClient,
    ep: &baidu::Endpoints,
    on_code: impl FnOnce(&baidu::DeviceCode),
    cancel: &std::sync::atomic::AtomicBool,
    timeout: Duration,
) -> Result<(baidu::Token, String, AccountInfo)> {
    let code = baidu::start_device(client, ep)?;
    on_code(&code);
    let token = baidu::poll_device(client, ep, &code, cancel)?;
    let name = baidu::user_info(ep, &token).map(|(n, _)| n).unwrap_or_else(|_| crate::tr!("百度网盘", "Baidu Netdisk"));
    let store = baidu::BaiduPan::new(client.clone(), ep.clone(), token, baidu::TokenSink::Memory, timeout);
    let account = probe_store(&store)?;
    Ok((store.token(), name, account))
}

/// Mount Quark in OpenList with the cookie of a signed-in browser, check
/// the Ludonimbus folder and switch to it. OpenList must be installed.
pub fn connect_quark(cookie: &str) -> Result<String> {
    let folder = "Ludonimbus".to_string();
    crate::openlist::connect_quark(cookie, &folder)?;
    let cloud = CloudConfig::Quark { folder };
    let store = build_store(&cloud, None, Duration::from_secs(30))?;
    store.check()?;
    let label = store.label();
    switch_to(cloud, None)?;
    Ok(label)
}

/// Sign in to Baidu Netdisk, save the token and switch to it. Empty app
/// fields use the app built into the release; an empty SecretKey with an
/// AppKey reuses the saved one. The SecretKey of an own app goes to the
/// credential store, never into `config.json`. Returns the account name.
pub fn connect_baidu(
    app_key: &str,
    secret_key: &str,
    app_name: &str,
    on_code: impl FnOnce(&baidu::DeviceCode),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String> {
    let cfg = config::load()?;
    let secret_key = if secret_key.trim().is_empty() {
        let legacy = match &cfg.cloud {
            CloudConfig::BaiduPan { app_key: k, secret_key, .. } if k.trim() == app_key.trim() => secret_key.as_str(),
            _ => "",
        };
        baidu::saved_app_secret(app_key, legacy)?
    } else {
        secret_key.trim().to_string()
    };
    let client = baidu::BaiduClient::resolve(app_key, &secret_key, app_name)
        .ok_or_else(|| anyhow!(crate::tr!("请先填写百度网盘应用的 AppKey 和 SecretKey", "Enter the Baidu Netdisk app's AppKey and SecretKey first")))?;
    let timeout = timeout(&cfg).max(Duration::from_secs(15));
    let (token, name, _) = baidu_sign_in(&client, &baidu::Endpoints::default(), on_code, cancel, timeout)?;
    baidu::store_token(&token)?;
    let app_key = app_key.trim().to_string();
    if !app_key.is_empty() {
        secrets::store(&baidu::app_secret_user(&app_key), &secret_key)?;
    }
    // Signing in with another app replaces the old one's SecretKey.
    if let CloudConfig::BaiduPan { app_key: old, .. } = &cfg.cloud {
        if !old.trim().is_empty() && old.trim() != app_key {
            let _ = secrets::clear(&baidu::app_secret_user(old));
        }
    }
    switch_to(
        CloudConfig::BaiduPan {
            app_key,
            secret_key: String::new(),
            app_name: app_name.trim().to_string(),
            account: Some(name.clone()),
        },
        None,
    )?;
    Ok(name)
}

/// Move the SecretKey of an own Baidu app out of a `config.json` written
/// before 0.6.3 into the credential store. Runs at start-up.
pub fn move_secrets_out_of_config() -> Result<()> {
    let cfg = config::load()?;
    let legacy = |c: &CloudConfig| matches!(c, CloudConfig::BaiduPan { app_key, secret_key, .. } if !app_key.trim().is_empty() && !secret_key.is_empty());
    if !legacy(&cfg.cloud) && !cfg.previous_cloud.as_ref().is_some_and(legacy) {
        return Ok(());
    }
    config::update(|c| {
        for cloud in std::iter::once(&mut c.cloud).chain(c.previous_cloud.as_mut()) {
            if let CloudConfig::BaiduPan { app_key, secret_key, .. } = cloud {
                if !app_key.trim().is_empty() && !secret_key.is_empty() {
                    secrets::store(&baidu::app_secret_user(app_key), secret_key.trim())?;
                    secret_key.clear();
                }
            }
        }
        Ok(())
    })
}

fn reset_markers() -> Result<()> {
    crate::savelib::forget();
    client::forget();
    state::update(|s| {
        for g in s.games.values_mut() {
            g.cloud_def_uploaded = false;
            g.last_check = None;
        }
    })
}

/// Make `new` the active cloud. `secret` (a WebDAV password / S3 key) is
/// saved first. The old cloud is remembered as `previous_cloud` so its data
/// can be copied over with [`migrate_previous`].
pub fn switch_to(new: CloudConfig, secret: Option<&str>) -> Result<()> {
    if let (Some(s), Some(user)) = (secret, secrets::user_for(&new)) {
        secrets::store(&user, s)?;
    }
    let dropped = config::update(|c| {
        let old = std::mem::replace(&mut c.cloud, new.clone());
        // Google Drive and Baidu keep one sign-in each: switching accounts
        // replaces it, so there is nothing to copy from.
        let same_sign_in = matches!(
            (&old, &new),
            (CloudConfig::GoogleDrive { .. }, CloudConfig::GoogleDrive { .. })
                | (CloudConfig::BaiduPan { .. }, CloudConfig::BaiduPan { .. })
                | (CloudConfig::Quark { .. }, CloudConfig::Quark { .. })
        );
        if old == CloudConfig::None || old == new || same_sign_in {
            return Ok(None);
        }
        let replaced = c.previous_cloud.replace(old);
        // Switching back to the cloud we came from: nothing left to copy.
        if c.previous_cloud.as_ref() == Some(&new) {
            c.previous_cloud = None;
        }
        Ok(replaced)
    })?;
    if let Some(d) = dropped {
        let cfg = config::load()?;
        let mut keep = vec![&cfg.cloud];
        keep.extend(cfg.previous_cloud.as_ref());
        secrets::release(&d, &keep);
    }
    reset_markers()
}

/// Stop syncing with any cloud. Saved credentials are removed; the data in
/// the cloud is kept.
pub fn disconnect() -> Result<()> {
    let cfg = config::load()?;
    let is_google = |c: &CloudConfig| matches!(c, CloudConfig::GoogleDrive { .. });
    if is_google(&cfg.cloud) || cfg.previous_cloud.as_ref().is_some_and(is_google) {
        let _ = oauth::clear_refresh_token();
    }
    secrets::release(&cfg.cloud, &[]);
    if let Some(p) = &cfg.previous_cloud {
        secrets::release(p, &[]);
    }
    config::update(|c| {
        c.cloud = CloudConfig::None;
        c.previous_cloud = None;
        Ok(())
    })?;
    reset_markers()
}

/// Forget the previous cloud (after copying, or when the user declines).
pub fn dismiss_previous() -> Result<()> {
    let cfg = config::load()?;
    let Some(prev) = cfg.previous_cloud.clone() else { return Ok(()) };
    secrets::release(&prev, &[&cfg.cloud]);
    // The Google sign-in is no longer needed once we moved away from Drive.
    if matches!(prev, CloudConfig::GoogleDrive { .. }) && !matches!(cfg.cloud, CloudConfig::GoogleDrive { .. }) {
        let _ = oauth::clear_refresh_token();
    }
    config::update(|c| {
        c.previous_cloud = None;
        Ok(())
    })
}

/// Copy everything from the previous cloud to the current one, then forget
/// the previous cloud.
pub fn migrate_previous(progress: impl FnMut(&migrate::Progress)) -> Result<migrate::Report> {
    let cfg = config::load()?;
    let prev = cfg.previous_cloud.clone().ok_or_else(|| anyhow!(crate::tr!("没有需要复制的旧云盘", "There's no old cloud to copy from")))?;
    if prev == CloudConfig::None || cfg.cloud == CloudConfig::None {
        bail!(crate::tr!("没有需要复制的旧云盘", "There's no old cloud to copy from"));
    }
    let from = build_store(&prev, None, timeout(&cfg))?;
    let to = build_store(&cfg.cloud, None, timeout(&cfg))?;
    let report = migrate::copy_all(from.as_ref(), to.as_ref(), (&cfg.device.id, &cfg.device.name), progress)?;
    dismiss_previous()?;
    reset_markers()?;
    Ok(report)
}

/// Machine name of a cloud kind (matches the `kind` tag in the config).
pub fn kind_id(cloud: &CloudConfig) -> &'static str {
    match cloud {
        CloudConfig::None => "none",
        CloudConfig::LocalFolder { .. } => "localFolder",
        CloudConfig::GoogleDrive { .. } => "googleDrive",
        CloudConfig::WebDav { .. } => "webDav",
        CloudConfig::S3 { .. } => "s3",
        CloudConfig::BaiduPan { .. } => "baiduPan",
        CloudConfig::Quark { .. } => "quark",
    }
}

/// Short name of a cloud kind for messages.
pub fn kind_name(cloud: &CloudConfig) -> &'static str {
    let en = crate::i18n::is_en();
    match cloud {
        CloudConfig::None => if en { "Not connected" } else { "未连接" },
        CloudConfig::LocalFolder { .. } => if en { "Local / synced folder" } else { "本地 / 同步文件夹" },
        CloudConfig::GoogleDrive { .. } => "Google Drive",
        CloudConfig::WebDav { .. } => "WebDAV",
        CloudConfig::S3 { .. } => if en { "S3-compatible storage" } else { "S3 兼容存储" },
        CloudConfig::BaiduPan { .. } => if en { "Baidu Netdisk" } else { "百度网盘" },
        CloudConfig::Quark { .. } => if en { "Quark" } else { "夸克网盘" },
    }
}

pub fn device_info(cfg: &AppConfig) -> DeviceInfo {
    let state = state::load();
    let installs = cfg
        .games
        .iter()
        .filter_map(|g| {
            let st = state.games.get(&g.key)?;
            let line = st.line.as_ref()?;
            Some(DeviceInstall {
                game: line.game.clone(),
                line: line.line.clone(),
                dir: line.dir().to_string(),
                synced: st.last_synced_id.clone(),
                synced_at: st.last_sync_at,
            })
        })
        .collect();
    DeviceInfo {
        id: cfg.device.id.clone(),
        name: cfg.device.name.clone(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        app_version: crate::APP_VERSION.to_string(),
        last_seen: Utc::now(),
        layout: v2::LAYOUT,
        installs,
    }
}
