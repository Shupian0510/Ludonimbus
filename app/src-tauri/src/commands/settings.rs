//! Settings: this device, how many versions to keep, the launch wrapper's
//! waits, the interface language, and the first-run guide.

use super::{blocking, CmdResult};
use ludonimbus_core::cloud::{self, oauth};
use ludonimbus_core::config::{self, CloudConfig, CloudKind, Retention};
use ludonimbus_core::sync::Ctx;
use ludonimbus_core::{ludusavi, paths, steam};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "Settings")]
pub struct SettingsView {
    device_id: String,
    device_name: String,
    retention: Retention,
    network_timeout_secs: u64,
    steam_root: Option<String>,
    /// The Steam folder was picked in Settings (not auto-detected).
    steam_root_custom: bool,
    steam_account: Option<String>,
    cloud: CloudConfig,
    /// Cloud used before the last switch whose data can still be copied.
    previous_cloud: Option<PreviousCloud>,
    google_builtin: bool,
    /// A Baidu Netdisk app is built into this release.
    baidu_builtin: bool,
    /// App name (= folder under 「我的应用数据」) of the built-in Baidu app.
    baidu_app_name: String,
    data_dir: String,
    runner_path: String,
    runner_exists: bool,
    ludusavi: Option<String>,
    version: String,
    screenshots: config::ScreenshotMode,
    /// Games new to this device keep their saves to themselves.
    independent_saves: bool,
    /// Before playing, how long the launch wrapper waits for the cloud.
    launch_wait_secs: u64,
    /// After playing, how long it waits for the upload before handing it over.
    exit_wait_secs: u64,
    /// After playing, always wait until the upload is done.
    wait_for_upload: bool,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PreviousCloud {
    kind: CloudKind,
    name: String,
}

#[tauri::command]
pub async fn get_settings() -> CmdResult<SettingsView> {
    blocking(|| {
        let cfg = config::load()?;
        let root = steam::root(&cfg);
        let mut cloud = cfg.cloud.clone();
        match &mut cloud {
            CloudConfig::GoogleDrive { client_secret, .. } if !client_secret.is_empty() => {
                *client_secret = "••••••••".into();
            }
            // The SecretKey of an own app lives in the credential store; the
            // UI only learns whether one is saved.
            CloudConfig::BaiduPan { app_key, secret_key, .. } => {
                let saved = cloud::baidu::saved_app_secret(app_key, secret_key).is_ok_and(|s| !s.is_empty());
                *secret_key = if saved { "••••••••".into() } else { String::new() };
            }
            _ => {}
        }
        let runner = paths::runner_exe();
        Ok(SettingsView {
            device_id: cfg.device.id.clone(),
            device_name: cfg.device.name.clone(),
            retention: cfg.retention.clone(),
            network_timeout_secs: cfg.network_timeout_secs,
            steam_account: root.as_deref().and_then(|r| steam::default_account(&cfg, r)),
            steam_root_custom: cfg.steam.root.as_deref().is_some_and(|r| root.as_deref() == Some(r)),
            steam_root: root.map(|r| r.display().to_string()),
            previous_cloud: cfg.previous_cloud.as_ref().map(|p| PreviousCloud { kind: p.kind(), name: previous_name(p) }),
            cloud,
            google_builtin: oauth::GoogleClient::has_builtin(),
            baidu_builtin: cloud::baidu::BaiduClient::has_builtin(),
            baidu_app_name: cloud::baidu::BaiduClient::builtin_app_name(),
            data_dir: paths::data_dir().display().to_string(),
            runner_exists: runner.is_file(),
            runner_path: runner.display().to_string(),
            ludusavi: ludusavi::version(),
            version: ludonimbus_core::APP_VERSION.to_string(),
            screenshots: cfg.screenshots,
            independent_saves: cfg.independent_saves,
            launch_wait_secs: cfg.launch_wait_secs,
            exit_wait_secs: cfg.exit_wait_secs,
            wait_for_upload: cfg.wait_for_upload,
        })
    })
    .await
}

/// What to change in the settings. A missing field stays as it is.
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
pub struct SettingsPatch {
    device_name: Option<String>,
    keep_recent: Option<usize>,
    keep_daily_days: Option<i64>,
    network_timeout_secs: Option<u64>,
    steam_account: Option<String>,
    screenshots: Option<config::ScreenshotMode>,
    independent_saves: Option<bool>,
    launch_wait_secs: Option<u64>,
    exit_wait_secs: Option<u64>,
    wait_for_upload: Option<bool>,
}

/// First-run guide state.
#[tauri::command]
pub async fn get_onboarding() -> CmdResult<config::Onboarding> {
    blocking(|| Ok(config::load()?.onboarding)).await
}

/// The guide was finished or closed; `usage` is what the user said they use Ludonimbus for.
#[tauri::command]
pub async fn finish_onboarding(usage: Vec<String>) -> CmdResult<()> {
    blocking(move || {
        config::update(|c| {
            c.onboarding.done_at = Some(chrono::Utc::now());
            c.onboarding.usage = usage;
            Ok(())
        })
    })
    .await
}

/// The interface language picked in Settings ("system", "zh-CN" or "en").
#[tauri::command]
pub async fn get_language() -> CmdResult<String> {
    blocking(|| Ok(config::load()?.language)).await
}

#[tauri::command]
pub async fn set_language(app: tauri::AppHandle, language: String) -> CmdResult<()> {
    let language = match language.as_str() {
        "zh-CN" | "en" => language,
        _ => "system".to_string(),
    };
    blocking(move || {
        config::update(|c| {
            c.language = language.clone();
            Ok(())
        })?;
        ludonimbus_core::i18n::set(&language);
        Ok(())
    })
    .await?;
    crate::relabel_tray(&app);
    Ok(())
}

#[tauri::command]
pub async fn save_settings(patch: SettingsPatch) -> CmdResult<()> {
    blocking(move || {
        let renamed = patch.device_name.is_some();
        config::update(|c| {
            if let Some(n) = patch.device_name.filter(|n| !n.trim().is_empty()) {
                c.device.name = n.trim().to_string();
            }
            if let Some(v) = patch.keep_recent {
                c.retention.keep_recent = v.clamp(1, 500);
            }
            if let Some(v) = patch.keep_daily_days {
                c.retention.keep_daily_days = v.clamp(0, 3650);
            }
            if let Some(v) = patch.network_timeout_secs {
                c.network_timeout_secs = v.clamp(3, 120);
            }
            if let Some(a) = patch.steam_account {
                c.steam.account = Some(a);
            }
            if let Some(m) = patch.screenshots {
                c.screenshots = m;
            }
            if let Some(v) = patch.independent_saves {
                c.independent_saves = v;
            }
            if let Some(v) = patch.launch_wait_secs {
                c.launch_wait_secs = v.min(600);
            }
            if let Some(v) = patch.exit_wait_secs {
                c.exit_wait_secs = v.min(600);
            }
            if let Some(v) = patch.wait_for_upload {
                c.wait_for_upload = v;
            }
            Ok(())
        })?;
        if renamed {
            let ctx = Ctx::load()?;
            if let Ok(c) = ctx.cloud() {
                let _ = c.put_device(&cloud::device_info(&ctx.cfg));
            }
        }
        Ok(())
    })
    .await
}

fn previous_name(c: &CloudConfig) -> String {
    let detail = match c {
        CloudConfig::LocalFolder { path } => path.display().to_string(),
        CloudConfig::GoogleDrive { account, .. } => account.clone().unwrap_or_default(),
        CloudConfig::WebDav { url, .. } => url.clone(),
        CloudConfig::S3 { bucket, .. } => bucket.clone(),
        CloudConfig::BaiduPan { account, .. } => account.clone().unwrap_or_default(),
        CloudConfig::Quark { folder } => folder.clone(),
        CloudConfig::None => String::new(),
    };
    if detail.is_empty() {
        cloud::kind_name(c).to_string()
    } else {
        ludonimbus_core::tr!("{}（{detail}）", "{} ({detail})", cloud::kind_name(c))
    }
}
