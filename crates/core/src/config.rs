//! User configuration stored at `%APPDATA%\Ludonimbus\config.json`.

use crate::{lockfile::FileLock, paths, util};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use ts_rs::TS;

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
    /// Steam screenshots of linked games in the cloud.
    pub screenshots: ScreenshotMode,
    /// Handheld mode: the launch wrapper asks its questions in a full-screen
    /// window instead of system dialogs, and "Off" also keeps Ludonimbus
    /// from opening in the handheld view by itself (see [`crate::handheld`]).
    pub handheld: HandheldMode,
    /// The Switch client's connection over the local network (see
    /// [`crate::nxlan`]). Off unless the user turns it on.
    pub nx_lan: bool,
    /// Games on this device keep their saves to themselves unless told
    /// otherwise: a game that hasn't synced here yet gets a save line of its
    /// own instead of joining the one its other devices share
    /// (docs/plan-v2.md, section 7).
    pub independent_saves: bool,
    /// Before playing, how long the launch wrapper waits for the cloud at
    /// most; then the game starts with the saves on this device. A newer
    /// version that is being downloaded by then is waited for, unless the
    /// user says not to (docs/plan-v2.md, section 5.5).
    pub launch_wait_secs: u64,
    /// After playing, how long the launch wrapper waits for the upload; the
    /// rest is done in the background.
    pub exit_wait_secs: u64,
    /// After playing, always wait until the upload is done (for someone who
    /// turns the device off right after).
    pub wait_for_upload: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HandheldMode {
    /// When Steam starts the game from Big Picture / Game Mode, or the Xbox
    /// full screen experience is on.
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScreenshotMode {
    /// Screenshots stay on this PC (they can take a lot of cloud space).
    #[default]
    Off,
    /// Upload screenshots taken on this PC.
    Upload,
    /// Upload, and download the other devices' into this PC's Steam.
    Sync,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Onboarding {
    /// When the guide was finished or closed; `None` = not seen yet.
    pub done_at: Option<chrono::DateTime<chrono::Utc>>,
    /// What the user said they use Ludonimbus for (emulators, recomps, steamThirdParty, multiDevice).
    pub usage: Vec<String>,
}

/// The configuration's version. 2 since 1.0: a game may say which save line
/// it shares and through which folders. Older versions of Ludonimbus read a
/// newer file as it is and leave out what they don't know when they write
/// it.
pub const VERSION: u32 = 2;

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            version: VERSION,
            device: DeviceConfig::generate(),
            cloud: CloudConfig::None,
            steam: SteamConfig::default(),
            retention: Retention::default(),
            network_timeout_secs: 8,
            games: Vec::new(),
            previous_cloud: None,
            language: "system".into(),
            onboarding: Onboarding::default(),
            screenshots: ScreenshotMode::Off,
            handheld: HandheldMode::Auto,
            nx_lan: false,
            independent_saves: false,
            launch_wait_secs: 10,
            exit_wait_secs: 10,
            wait_for_upload: false,
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
        let name = std::env::var("COMPUTERNAME").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| crate::tr!("我的电脑", "My PC"));
        DeviceConfig { id: format!("d-{}", ulid::Ulid::new().to_string().to_lowercase()), name }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, TS)]
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

/// Which kind of cloud a [`CloudConfig`] is: the `kind` it has in JSON
/// (and [`crate::cloud::kind_id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CloudKind {
    None,
    LocalFolder,
    GoogleDrive,
    WebDav,
    S3,
    BaiduPan,
    Quark,
}

impl CloudConfig {
    pub fn kind(&self) -> CloudKind {
        match self {
            CloudConfig::None => CloudKind::None,
            CloudConfig::LocalFolder { .. } => CloudKind::LocalFolder,
            CloudConfig::GoogleDrive { .. } => CloudKind::GoogleDrive,
            CloudConfig::WebDav { .. } => CloudKind::WebDav,
            CloudConfig::S3 { .. } => CloudKind::S3,
            CloudConfig::BaiduPan { .. } => CloudKind::BaiduPan,
            CloudConfig::Quark { .. } => CloudKind::Quark,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Retention {
    /// Always keep this many newest automatic snapshots.
    pub keep_recent: usize,
    /// Additionally keep the newest snapshot of each day for this many days.
    pub keep_daily_days: i64,
}

impl Default for Retention {
    fn default() -> Self {
        Retention { keep_recent: 20, keep_daily_days: 30 }
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
    /// The save locations of `ludusavi_title` in Ludusavi's database, as
    /// patterns like `saves` (worked out on this device from the database's
    /// rules, [`crate::games::ludusavi_saves`]). A scan still asks Ludusavi;
    /// a restore writes only files under these, the user's own `saves`, or
    /// files a scan finds now. Patterns the user adds stay in `saves`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ludusavi_saves: Vec<String>,
    /// What `ludusavi_saves` were worked out from (Ludusavi's version and
    /// database, the title, the game's folders here), so they are worked
    /// out again once that changes. `None`: not yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ludusavi_from: Option<String>,
    /// Upload when the game process exits instead of waiting for the
    /// launcher to close. `None` = the adapter's default.
    pub upload_on_game_exit: Option<bool>,
    pub steam: Option<SteamLink>,
    /// A game installed through Steam itself (not a non-Steam shortcut,
    /// that is [`GameEntry::steam`]): whether Steam Cloud or Ludonimbus
    /// keeps its saves (docs/plan-v2.md, section 8; [`crate::steamcloud`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreLink>,
    #[serde(default = "yes")]
    pub auto_sync: bool,
    /// Library group chosen by the user (e.g. an emulator's name for a game
    /// added with its own save folders); `None` = the detected source, see
    /// [`crate::games::source`].
    pub source: Option<String>,
    /// Which game this is, set by the user: entries with the same value are
    /// versions of one game in the save library (`solo:…` = not the same as
    /// any other). `None` = worked out from [`crate::savelib::ids`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    /// The save line this version follows in the cloud: `main` is the one
    /// the game's devices share, any other a line of its own
    /// (docs/plan-v2.md, section 7). `None`: nothing was chosen; it is the
    /// shared line, unless the device's default says otherwise the first
    /// time the game syncs ([`AppConfig::independent_saves`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The game record in the connected cloud whose saves this version
    /// continues, once it turned out to be a version of a game the cloud
    /// has under another key (docs/plan-v2.md, section 6). `None`: the
    /// record named like its key. Forgotten when another cloud is connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    /// Which folder of the save line it shares is which folder of this
    /// version, when it keeps the same saves elsewhere than the version the
    /// line began with ([`crate::bind`]). Empty: its files are the line's
    /// as they are. Goes with `record`, and is forgotten with it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bind: Vec<crate::bind::Bind>,
    /// Save lines of the game the user chose not to share with this
    /// version, by the files they hold ([`crate::cloud::v2::Line::def`]):
    /// not suggested again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub declined: Vec<String>,
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

/// A game installed through Steam, in the library. Its three states: not
/// syncing by itself (`auto_sync` off) is Steam's; [`StoreMode::Backup`]
/// keeps versions of what Steam Cloud syncs; [`StoreMode::Managed`] syncs
/// it like any other game, with Steam Cloud off for it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StoreLink {
    /// The Steam account (the folder name under `userdata`).
    pub account: String,
    pub appid: u32,
    pub mode: StoreMode,
    /// The game's launch options in Steam go through the wrapper
    /// (`"<runner>" --game <key> -- %command%`). Stays set when the game
    /// goes back to backup only by itself: the wrapper does no harm there.
    pub wrapper: bool,
    /// The launch options before the wrapper was put in, given back on
    /// hand-back. `None`: there were none.
    pub original_options: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreMode {
    /// Steam Cloud stays on; Ludonimbus stores versions and never writes
    /// into the game's saves by itself.
    #[default]
    Backup,
    /// Steam Cloud is off for the game; Ludonimbus pulls and uploads.
    Managed,
}

impl GameEntry {
    /// Forget what was settled about this game in the cloud that is
    /// connected: another cloud knows nothing of it.
    pub fn forget_cloud(&mut self) {
        self.record = None;
        self.bind.clear();
        self.declined.clear();
    }

    /// A game installed through Steam whose saves Steam Cloud keeps:
    /// Ludonimbus records their history and writes nothing into them by
    /// itself (docs/plan-v2.md, section 8.2).
    pub fn backup_only(&self) -> bool {
        self.store.as_ref().is_some_and(|s| s.mode != StoreMode::Managed)
    }
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
    FileLock::acquire(&paths::data_dir().join("config.lock"), Duration::from_secs(10))
}

fn read_file() -> Result<Option<AppConfig>> {
    let path = paths::config_file();
    if !path.exists() {
        return Ok(None);
    }
    let bytes = util::read_retrying(&path).with_context(|| crate::tr!("无法读取配置文件 {}", "Can't read the configuration file {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let cfg: AppConfig =
        serde_json::from_str(&text).with_context(|| crate::tr!("配置文件格式错误 {}", "The configuration file is malformed: {}", path.display()))?;
    // Every program reads the configuration first, so this keeps texts made
    // in Rust in the user's language (also after it changes in Settings).
    crate::i18n::set(&cfg.language);
    Ok(Some(cfg))
}

/// The first time this version changes a configuration an older one wrote,
/// that file is kept next to it as `config.v1.json`: what to put back when
/// going back to the older version (docs/plan-v2.md, 12.4). Kept once, and
/// never changed afterwards.
fn keep_older(path: &std::path::Path) {
    let copy = path.with_file_name("config.v1.json");
    if copy.exists() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let older =
        serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("version").and_then(|n| n.as_u64())).is_some_and(|n| n < u64::from(VERSION));
    if older {
        let _ = std::fs::write(copy, text);
    }
}

fn write_file(cfg: &AppConfig) -> Result<()> {
    let path = paths::config_file();
    keep_older(&path);
    let mut cfg = cfg.clone();
    cfg.version = VERSION;
    util::atomic_write(&path, &serde_json::to_vec_pretty(&cfg)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloud_kind_is_the_kind_its_config_has_in_json() {
        let s = String::new;
        let all = [
            CloudConfig::None,
            CloudConfig::LocalFolder { path: PathBuf::from("cloud") },
            CloudConfig::GoogleDrive { client_id: s(), client_secret: s(), folder: s(), account: None },
            CloudConfig::WebDav { url: s(), username: s(), folder: s() },
            CloudConfig::S3 { endpoint: s(), region: s(), bucket: s(), prefix: s(), access_key_id: s(), path_style: false },
            CloudConfig::BaiduPan { app_key: s(), secret_key: s(), app_name: s(), account: None },
            CloudConfig::Quark { folder: s() },
        ];
        for c in &all {
            let kind = serde_json::to_value(c.kind()).unwrap();
            assert_eq!(kind, serde_json::to_value(c).unwrap()["kind"]);
            assert_eq!(kind, crate::cloud::kind_id(c));
        }
    }
}
