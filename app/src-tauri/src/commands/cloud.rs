//! Connecting a cloud (a local folder, WebDAV / S3, Google Drive, Baidu
//! Netdisk, Quark through OpenList), copying from the previous one, and the
//! connected cloud's account and devices.

use super::{blocking, CmdResult};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::cloud::{self, oauth, AccountInfo, DeviceInfo};
use ludonimbus_core::config::{self, CloudConfig};
use ludonimbus_core::sync::Ctx;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Folders kept in sync by OneDrive, Dropbox, Google Drive for desktop and
/// the like, for the "local folder" cloud.
#[tauri::command]
pub async fn cloud_folder_suggestions() -> CmdResult<Vec<cloud::local::SyncFolder>> {
    blocking(|| Ok(cloud::local::sync_folders())).await
}

#[tauri::command]
pub async fn cloud_use_folder(path: String) -> CmdResult<()> {
    blocking(move || {
        let p = PathBuf::from(&path);
        std::fs::create_dir_all(&p)?;
        cloud::switch_to(CloudConfig::LocalFolder { path: p }, None)
    })
    .await
}

/// Use the saved secret when the form leaves it empty for the cloud that is
/// already connected.
fn secret_for(new: &CloudConfig, secret: &str) -> Result<Option<String>> {
    if !secret.is_empty() {
        return Ok(Some(secret.to_string()));
    }
    match cloud::secrets::load_for(new)? {
        Some(s) => Ok(Some(s)),
        None => match new {
            CloudConfig::WebDav { .. } => bail!(ludonimbus_core::tr!("请填写 WebDAV 密码", "Enter the WebDAV password")),
            CloudConfig::S3 { .. } => bail!(ludonimbus_core::tr!("请填写 Secret Key", "Enter the Secret Key")),
            _ => Ok(None),
        },
    }
}

fn check_store_config(c: &CloudConfig) -> Result<()> {
    match c {
        CloudConfig::WebDav { .. } | CloudConfig::S3 { .. } => Ok(()),
        _ => bail!(ludonimbus_core::tr!("这里只能连接 WebDAV 或 S3 兼容存储", "Only WebDAV or S3-compatible storage can be connected here")),
    }
}

/// Try WebDAV / S3 settings without saving them.
#[tauri::command]
pub async fn cloud_test(cloud: CloudConfig, secret: String) -> CmdResult<AccountInfo> {
    blocking(move || {
        check_store_config(&cloud)?;
        let secret = secret_for(&cloud, &secret)?;
        let timeout = cloud::timeout(&config::load()?).max(std::time::Duration::from_secs(15));
        cloud::probe(&cloud, secret.as_deref(), timeout)
    })
    .await
}

/// Test WebDAV / S3 settings, then switch to them.
#[tauri::command]
pub async fn cloud_connect(cloud: CloudConfig, secret: String) -> CmdResult<String> {
    blocking(move || {
        check_store_config(&cloud)?;
        let secret = secret_for(&cloud, &secret)?;
        let timeout = cloud::timeout(&config::load()?).max(std::time::Duration::from_secs(15));
        let account = cloud::probe(&cloud, secret.as_deref(), timeout)?;
        cloud::switch_to(cloud, secret.as_deref())?;
        Ok(account.label)
    })
    .await
}

/// Copy all saves from the previous cloud into the current one.
#[tauri::command]
pub async fn cloud_migrate(app: tauri::AppHandle) -> CmdResult<cloud::migrate::Report> {
    use tauri::Emitter;
    blocking(move || {
        cloud::migrate_previous(|p| {
            let _ = app.emit("cloud-migrate-progress", p.clone());
        })
    })
    .await
}

static LOGIN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sign in to Baidu Netdisk (device code). The code and QR code are sent to
/// the UI as `baidu-device-code` and the approval page opens in the browser.
#[tauri::command]
pub async fn cloud_connect_baidu(app: tauri::AppHandle, app_key: String, secret_key: String, app_name: String) -> CmdResult<String> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    blocking(move || {
        // The masked placeholder means "keep the saved SecretKey" (an empty
        // one makes connect_baidu look it up).
        let secret = if secret_key.starts_with('•') { String::new() } else { secret_key };
        cloud::connect_baidu(
            &app_key,
            &secret,
            &app_name,
            |code| {
                let _ = app.emit("baidu-device-code", code.clone());
                let _ = opener::open_browser(code.browser_url());
            },
            &LOGIN_CANCEL,
        )
    })
    .await
}

/// What the Quark option shows before downloading OpenList.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenListInfo {
    installed: bool,
    version: &'static str,
    size: u64,
    url: String,
    asset: &'static str,
    project: &'static str,
    license: &'static str,
}

#[tauri::command]
pub async fn openlist_status() -> CmdResult<OpenListInfo> {
    use ludonimbus_core::openlist;
    Ok(OpenListInfo {
        installed: openlist::installed(),
        version: openlist::VERSION,
        size: openlist::SIZE,
        url: openlist::download_url(),
        asset: openlist::ASSET,
        project: openlist::PROJECT_URL,
        license: openlist::LICENSE,
    })
}

/// Download OpenList from GitHub (progress as `openlist-progress`), or
/// install it from a zip the user downloaded.
#[tauri::command]
pub async fn openlist_install(app: tauri::AppHandle, zip: Option<String>) -> CmdResult<()> {
    use tauri::Emitter;
    blocking(move || {
        let emit = |done: u64, total: u64| {
            let _ = app.emit("openlist-progress", serde_json::json!({ "done": done, "total": total }));
        };
        ludonimbus_core::openlist::install(zip.as_deref().map(Path::new), &emit)
    })
    .await
}

#[tauri::command]
pub async fn openlist_pick_zip() -> CmdResult<Option<String>> {
    blocking(|| {
        Ok(rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("选择下载好的 OpenList 安装包", "Choose the downloaded OpenList package"))
            .add_filter("zip", &["zip"])
            .pick_file()
            .map(|p| p.display().to_string()))
    })
    .await
}

/// Sign in to Quark in a private window, mount it in OpenList and switch to
/// it (also used to sign in again when the sign-in expired).
#[tauri::command]
pub async fn cloud_connect_quark(app: tauri::AppHandle) -> CmdResult<String> {
    use std::sync::atomic::Ordering;
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    let cookie = crate::quark_login::sign_in(&app, &LOGIN_CANCEL).await.map_err(|e| format!("{e:#}"))?;
    blocking(move || cloud::connect_quark(&cookie)).await
}

#[tauri::command]
pub async fn cloud_cancel_login() -> CmdResult<()> {
    LOGIN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn cloud_dismiss_previous() -> CmdResult<()> {
    blocking(cloud::dismiss_previous).await
}

#[tauri::command]
pub async fn cloud_connect_google(client_id: String, client_secret: String) -> CmdResult<String> {
    blocking(move || {
        // Keep the stored secret when the UI shows the masked placeholder.
        let current = config::load()?.cloud;
        let secret = if client_secret.starts_with('•') {
            match current {
                CloudConfig::GoogleDrive { client_secret, .. } => client_secret,
                _ => String::new(),
            }
        } else {
            client_secret
        };
        let client = oauth::GoogleClient::resolve(&client_id, &secret)
            .ok_or_else(|| anyhow!(ludonimbus_core::tr!("请先填写 Google OAuth Client ID", "Enter the Google OAuth Client ID first")))?;
        let auth = oauth::authorize(&client, std::time::Duration::from_secs(300))?;
        oauth::store_refresh_token(&auth.refresh_token)?;
        let label = auth.email.clone().unwrap_or_else(|| "Google Drive".into());
        cloud::switch_to(
            CloudConfig::GoogleDrive {
                client_id: client_id.trim().to_string(),
                client_secret: secret.trim().to_string(),
                folder: "Ludonimbus".into(),
                account: auth.email.clone(),
            },
            None,
        )?;
        Ok(label)
    })
    .await
}

#[tauri::command]
pub async fn cloud_disconnect() -> CmdResult<()> {
    blocking(cloud::disconnect).await
}

#[tauri::command]
pub async fn cloud_account() -> CmdResult<AccountInfo> {
    blocking(|| Ctx::load()?.cloud()?.account()).await
}

#[tauri::command]
pub async fn list_devices() -> CmdResult<Vec<DeviceInfo>> {
    blocking(|| {
        let ctx = Ctx::load()?;
        let c = ctx.cloud()?;
        let _ = c.put_device(&cloud::device_info(&ctx.cfg));
        let mut list = c.list_devices()?;
        list.sort_by_key(|a| std::cmp::Reverse(a.last_seen));
        Ok(list)
    })
    .await
}
