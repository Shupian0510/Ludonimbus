//! Game backups: the game's own files (not its saves) in the cloud, so a game
//! that is gone from this PC can be installed again from there. Design and
//! cloud layout: docs/plan-game-backup.md.
//!
//! - [`speedtest`] upload / download speed of the current cloud

pub mod speedtest;

use crate::cloud::{self, gdrive, oauth, store::Store, NotConfigured};
use crate::config::{AppConfig, CloudConfig};
use anyhow::{anyhow, Result};

/// The current cloud as a path-based store (Google Drive included).
pub fn open_store(cfg: &AppConfig) -> Result<Box<dyn Store>> {
    let timeout = cloud::timeout(cfg);
    match &cfg.cloud {
        CloudConfig::None => Err(anyhow!(NotConfigured)),
        CloudConfig::GoogleDrive {
            client_id,
            client_secret,
            folder,
            ..
        } => {
            let client = oauth::GoogleClient::resolve(client_id, client_secret).ok_or_else(|| {
                anyhow!(crate::tr!(
                    "Google Drive 缺少 OAuth Client ID，请在设置里填写",
                    "Google Drive has no OAuth Client ID. Enter one in Settings"
                ))
            })?;
            let token = oauth::load_refresh_token()?.ok_or_else(|| {
                anyhow!(crate::tr!(
                    "Google Drive 登录已失效，请在设置里重新连接",
                    "The Google Drive sign-in has expired. Reconnect in Settings"
                ))
            })?;
            Ok(Box::new(gdrive::GoogleDrive::new(client, token, folder.clone(), timeout)))
        }
        other => cloud::build_store(other, None, timeout),
    }
}
