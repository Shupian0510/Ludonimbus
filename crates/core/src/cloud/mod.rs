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

pub mod gdrive;
pub mod local;
pub mod oauth;

use crate::config::{AppConfig, CloudConfig, GameEntry};
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The cloud cannot be reached right now (offline, timeout, folder missing).
#[derive(Debug)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "云端暂时无法访问：{}", self.0)
    }
}
impl std::error::Error for Unavailable {}

/// No cloud provider configured.
#[derive(Debug)]
pub struct NotConfigured;

impl std::fmt::Display for NotConfigured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "还没有连接云盘")
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
}

/// Open the configured provider. `Ok(None)` when no cloud is configured.
pub fn open(cfg: &AppConfig) -> Result<Option<Box<dyn CloudProvider>>> {
    let timeout = std::time::Duration::from_secs(cfg.network_timeout_secs.max(3));
    match &cfg.cloud {
        CloudConfig::None => Ok(None),
        CloudConfig::LocalFolder { path } => Ok(Some(Box::new(local::LocalFolder::new(path.clone())))),
        CloudConfig::GoogleDrive {
            client_id,
            client_secret,
            folder,
            ..
        } => {
            let client = oauth::GoogleClient::resolve(client_id, client_secret)
                .ok_or_else(|| anyhow!("Google Drive 缺少 OAuth Client ID，请在设置里填写"))?;
            let token = oauth::load_refresh_token()?
                .ok_or_else(|| anyhow!("Google Drive 登录已失效，请在设置里重新连接"))?;
            Ok(Some(Box::new(gdrive::GoogleDrive::new(client, token, folder.clone(), timeout))))
        }
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
