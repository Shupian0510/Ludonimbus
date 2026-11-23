//! User configuration stored at `%APPDATA%\Ludonimbus\config.json`.

use crate::{lockfile::FileLock, paths, util};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub version: u32,
    pub device: DeviceConfig,
    pub cloud: CloudConfig,
    pub steam: SteamConfig,
    pub retention: Retention,
    /// Seconds to wait for the cloud before treating it as offline.
    pub network_timeout_secs: u64,
    pub games: Vec<GameEntry>,
    /// The cloud used before the last switch, until its data has been copied
    /// to the new one (or the user declined).
    pub previous_cloud: Option<CloudConfig>,
    /// Interface language: "system" (Windows display language), "zh-CN" or "en".
    pub language: String,
    /// First-run guide. Stays on this PC.
    pub onboarding: Onboarding,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Onboarding {
    /// When the guide was finished or closed; `None` = not seen yet.
    pub done_at: Option<chrono::DateTime<chrono::Utc>>,
    /// What the user said they use Ludonimbus for (emulators, recomps, steamThirdParty, multiDevice).
    pub usage: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            version: 1,
            device: DeviceConfig::generate(),
            cloud: CloudConfig::None,
            steam: SteamConfig::default(),
            retention: Retention::default(),
            network_timeout_secs: 8,
            games: Vec::new(),
            previous_cloud: None,
            language: "system".into(),
            onboarding: Onboarding::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    pub id: String,
    pub name: String,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self::generate()
    }
}

impl DeviceConfig {
    pub fn generate() -> Self {
        let name = std::env::var("COMPUTERNAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| crate::tr!("我的电脑", "My PC"));
        DeviceConfig {
            id: format!("d-{}", ulid::Ulid::new().to_string().to_lowercase()),
            name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CloudConfig {
    #[default]
    None,
    /// Any folder: a NAS share, a USB drive, or a folder synced by
    /// Google Drive for desktop / OneDrive.
    LocalFolder { path: PathBuf },
    GoogleDrive {
        #[serde(default)]
        client_id: String,
        #[serde(default)]
        client_secret: String,
        #[serde(default = "default_drive_folder")]
        folder: String,
        #[serde(default)]
        account: Option<String>,
    },
    /// A WebDAV server: NAS, Nextcloud, Jianguoyun, OpenList / AList.
    /// The password is kept in the Windows Credential Manager.
    WebDav {
        url: String,
        #[serde(default)]
        username: String,
        #[serde(default = "default_drive_folder")]
        folder: String,
    },
    /// S3-compatible object storage (MinIO, Aliyun OSS, Tencent COS, R2…).
    /// The secret key is kept in the Windows Credential Manager.
    S3 {
        endpoint: String,
        #[serde(default)]
        region: String,
        bucket: String,
        #[serde(default = "default_drive_folder")]
        prefix: String,
        access_key_id: String,
        #[serde(default)]
        path_style: bool,
    },
    /// Baidu Netdisk through its open platform. Empty app fields use the
    /// app built into the release. The token is kept in the Windows
    /// Credential Manager.
    BaiduPan {
        #[serde(default)]
        app_key: String,
        #[serde(default)]
        secret_key: String,
        #[serde(default)]
        app_name: String,
        #[serde(default)]
        account: Option<String>,
    },
    /// Quark through the OpenList server Ludonimbus runs (see `openlist`).
    /// The sign-in lives in OpenList's own data folder.
    Quark {
        #[serde(default = "default_drive_folder")]
        folder: String,
    },
}

fn default_drive_folder() -> String {
    "Ludonimbus".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamConfig {
    /// Steam install folder; auto-detected when empty.
    pub root: Option<PathBuf>,
    /// Steam account id (the folder name under `userdata`).
    pub account: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Retention {
    /// Always keep this many newest automatic snapshots.
    pub keep_recent: usize,
    /// Additionally keep the newest snapshot of each day for this many days.
    pub keep_daily_days: i64,
}

impl Default for Retention {
    fn default() -> Self {
        Retention {
            keep_recent: 20,
            keep_daily_days: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameEntry {
    /// Stable id shared by all devices; also the cloud folder name.
    pub key: String,
    pub name: String,
    /// Built-in adapter providing the save rules (see `adapters/builtin.yaml`).
    pub adapter: Option<String>,
    /// Real game executable (what the Steam shortcut used to launch).
    pub exe: Option<PathBuf>,
    /// Original launch arguments.
    pub args: String,
    /// Working directory; defaults to the exe folder.
    pub work_dir: Option<PathBuf>,
    /// Game folder for `{install}`; defaults to the exe folder.
    pub install: Option<PathBuf>,
    /// Save patterns; when non-empty they replace the adapter's.
    pub saves: Vec<String>,
    pub exclude: Vec<String>,
    pub wait_for: Vec<String>,
    /// Extra named roots usable in patterns, e.g. `{"data": "{appdata}/Foo"}`.
    pub roots: BTreeMap<String, String>,
    /// Resolve saves through Ludusavi's database instead of patterns.
    pub ludusavi_title: Option<String>,
    /// Upload when the game process exits instead of waiting for the
    /// launcher to close. `None` = the adapter's default.
    pub upload_on_game_exit: Option<bool>,
    pub steam: Option<SteamLink>,
    #[serde(default = "yes")]
    pub auto_sync: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SteamLink {
    pub account: String,
    pub appid: u32,
    /// Whether the shortcut currently launches through `ludonimbus-run`.
    pub enabled: bool,
    /// Shortcut fields before we wrapped it, restored on disable.
    pub original: Option<OriginalShortcut>,
    /// The shortcut was created by Ludonimbus ("Add to Steam"), so it may be
    /// deleted again when the game is removed.
    #[serde(default)]
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OriginalShortcut {
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub icon: String,
}

impl AppConfig {
    pub fn game(&self, key: &str) -> Option<&GameEntry> {
        self.games.iter().find(|g| g.key == key)
    }

    pub fn game_mut(&mut self, key: &str) -> Option<&mut GameEntry> {
        self.games.iter_mut().find(|g| g.key == key)
    }

    pub fn require_game(&self, key: &str) -> Result<&GameEntry> {
        self.game(key)
            .ok_or_else(|| anyhow!(crate::tr!("找不到游戏「{key}」，请先在 Ludonimbus 里添加", "Game \"{key}\" not found. Add it in Ludonimbus first")))
    }
}

fn lock() -> Result<FileLock> {
    FileLock::acquire(
        &paths::data_dir().join("config.lock"),
        Duration::from_secs(10),
    )
}

fn read_file() -> Result<Option<AppConfig>> {
    let path = paths::config_file();
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| crate::tr!("无法读取配置文件 {}", "Can't read the configuration file {}", path.display()))?;
    let cfg: AppConfig = serde_json::from_str(&text)
        .with_context(|| crate::tr!("配置文件格式错误 {}", "The configuration file is malformed: {}", path.display()))?;
    // Every program reads the configuration first, so this keeps texts made
    // in Rust in the user's language (also after it changes in Settings).
    crate::i18n::set(&cfg.language);
    Ok(Some(cfg))
}

fn write_file(cfg: &AppConfig) -> Result<()> {
    util::atomic_write(&paths::config_file(), &serde_json::to_vec_pretty(cfg)?)
}

/// Load the configuration, creating (and persisting) defaults on first run so
/// the device id stays stable.
pub fn load() -> Result<AppConfig> {
    if let Some(cfg) = read_file()? {
        return Ok(cfg);
    }
    let _guard = lock()?;
    if let Some(cfg) = read_file()? {
        return Ok(cfg);
    }
    let cfg = AppConfig::default();
    write_file(&cfg)?;
    Ok(cfg)
}

/// Read-modify-write under the config lock.
pub fn update<T>(f: impl FnOnce(&mut AppConfig) -> Result<T>) -> Result<T> {
    let _guard = lock()?;
    let mut cfg = read_file()?.unwrap_or_default();
    let out = f(&mut cfg)?;
    write_file(&cfg)?;
    Ok(out)
}
