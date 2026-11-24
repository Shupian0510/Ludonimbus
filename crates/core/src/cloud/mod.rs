//! Cloud storage providers.
//!
//! Layout (same for every provider):
//! ```text
//! <root>/devices/<device-id>.json
//! <root>/games/<game-key>/game.json
//! <root>/games/<game-key>/snapshots/<ULID>.zip   (+ <ULID>.json metadata)
//! ```
//! Snapshots are immutable and append-only; there is no mutable "head"
//! pointer, the newest mainline snapshot is the head.
//!
//! Google Drive implements [`CloudProvider`] directly (it is id-based and
//! stores snapshot summaries in file properties). Every other backend only
//! implements the small path-based [`store::Store`] trait and gets the layout
//! from [`repo::Repo`].

pub mod baidu;
pub mod gdrive;
pub mod local;
pub mod migrate;
pub mod oauth;
pub mod quark;
pub mod repo;
pub mod s3;
pub mod secrets;
pub mod store;
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
        }
    }
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
    fn delete(&self, game: &str, id: &str) -> Result<()>;
    /// Update comment / lock flag.
    fn update_meta(&self, meta: &SnapshotMeta) -> Result<()>;
    fn put_device(&self, device: &DeviceInfo) -> Result<()>;
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    fn put_game(&self, game: &CloudGame) -> Result<()>;
    fn list_games(&self) -> Result<Vec<CloudGame>>;
    /// Path-based access for data other than snapshots (game backups…).
    // TEMPORARY placeholder for the version on `main` (where Google Drive
    // implements `Store` itself); take `main`'s when merging.
    fn files(&self) -> &dyn Store {
        &NoFiles
    }
}

// TEMPORARY, see `CloudProvider::files`: Google Drive has no path-based
// store on this branch yet.
struct NoFiles;

impl Store for NoFiles {
    fn kind(&self) -> &'static str {
        "none"
    }
    fn label(&self) -> String {
        String::new()
    }
    fn check(&self) -> Result<()> {
        bail!(crate::tr!("这个云盘暂时不支持这个功能", "This cloud doesn't support this yet"))
    }
    fn list(&self, _: &str) -> Result<Vec<store::Entry>> {
        self.check().map(|_| Vec::new())
    }
    fn read(&self, _: &str) -> Result<Option<Vec<u8>>> {
        self.check().map(|_| None)
    }
    fn download(&self, _: &str, _: &Path) -> Result<()> {
        self.check()
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<()> {
        self.check()
    }
    fn upload(&self, _: &str, _: &Path) -> Result<()> {
        self.check()
    }
    fn remove(&self, _: &str) -> Result<()> {
        self.check()
    }
}

pub fn timeout(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.network_timeout_secs.max(3))
}

/// Open the configured provider. `Ok(None)` when no cloud is configured.
pub fn open(cfg: &AppConfig) -> Result<Option<Box<dyn CloudProvider>>> {
    open_config(&cfg.cloud, timeout(cfg))
}

/// Open any cloud configuration with its saved credentials.
pub fn open_config(cloud: &CloudConfig, timeout: Duration) -> Result<Option<Box<dyn CloudProvider>>> {
    match cloud {
        CloudConfig::None => Ok(None),
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
            Ok(Some(Box::new(gdrive::GoogleDrive::new(client, token, folder.clone(), timeout))))
        }
        other => Ok(Some(Box::new(repo::Repo::new(build_store(other, None, timeout)?)))),
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
/// (used to test new settings before saving them).
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
            let client = baidu::BaiduClient::resolve(app_key, secret_key, app_name)
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
        CloudConfig::None | CloudConfig::GoogleDrive { .. } => bail!(crate::tr!("这种云盘不能这样打开", "This kind of cloud can't be opened this way")),
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
/// fields use the app built into the release. Returns the account name.
pub fn connect_baidu(
    app_key: &str,
    secret_key: &str,
    app_name: &str,
    on_code: impl FnOnce(&baidu::DeviceCode),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String> {
    let client = baidu::BaiduClient::resolve(app_key, secret_key, app_name)
        .ok_or_else(|| anyhow!(crate::tr!("请先填写百度网盘应用的 AppKey 和 SecretKey", "Enter the Baidu Netdisk app's AppKey and SecretKey first")))?;
    let timeout = timeout(&config::load()?).max(Duration::from_secs(15));
    let (token, name, _) = baidu_sign_in(&client, &baidu::Endpoints::default(), on_code, cancel, timeout)?;
    baidu::store_token(&token)?;
    switch_to(
        CloudConfig::BaiduPan {
            app_key: app_key.trim().to_string(),
            secret_key: secret_key.trim().to_string(),
            app_name: app_name.trim().to_string(),
            account: Some(name.clone()),
        },
        None,
    )?;
    Ok(name)
}

fn reset_markers() -> Result<()> {
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
    let from = open_config(&prev, timeout(&cfg))?.ok_or_else(|| anyhow!(crate::tr!("没有需要复制的旧云盘", "There's no old cloud to copy from")))?;
    let to = open(&cfg)?.ok_or_else(|| anyhow!(NotConfigured))?;
    let keys: Vec<String> = cfg.games.iter().map(|g| g.key.clone()).collect();
    let report = migrate::copy_all(from.as_ref(), to.as_ref(), &keys, progress)?;
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
    DeviceInfo {
        id: cfg.device.id.clone(),
        name: cfg.device.name.clone(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        app_version: crate::APP_VERSION.to_string(),
        last_seen: Utc::now(),
    }
}
