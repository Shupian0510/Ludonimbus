//! Passwords and secret keys of WebDAV / S3 clouds, kept in the Windows
//! Credential Manager (never in `config.json`). Google Drive keeps its
//! refresh token separately, see [`super::oauth`].

use crate::config::CloudConfig;
use anyhow::{anyhow, Context, Result};

const SERVICE: &str = "Ludonimbus";

fn trim_url(s: &str) -> &str {
    s.trim().trim_end_matches('/')
}

/// Credential name for a cloud, `None` for clouds without a secret here.
pub fn user_for(cloud: &CloudConfig) -> Option<String> {
    match cloud {
        CloudConfig::WebDav { url, username, .. } => Some(format!("webdav:{}@{}", username.trim(), trim_url(url))),
        CloudConfig::S3 { endpoint, access_key_id, .. } => {
            Some(format!("s3:{}@{}", access_key_id.trim(), trim_url(endpoint)))
        }
        CloudConfig::BaiduPan { .. } => Some(super::baidu::KEYRING_USER.to_string()),
        _ => None,
    }
}

fn entry(user: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, user)?)
}

pub fn store(user: &str, secret: &str) -> Result<()> {
    entry(user)?
        .set_password(secret)
        .context(crate::tr!("无法把密码保存到 Windows 凭据管理器", "Couldn't save the password to Windows Credential Manager"))
}

pub fn load(user: &str) -> Result<Option<String>> {
    match entry(user)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!(crate::tr!("无法读取保存的密码：{e}", "Couldn't read the saved password: {e}"))),
    }
}

pub fn clear(user: &str) -> Result<()> {
    match entry(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!(crate::tr!("无法删除保存的密码：{e}", "Couldn't delete the saved password: {e}"))),
    }
}

pub fn load_for(cloud: &CloudConfig) -> Result<Option<String>> {
    match user_for(cloud) {
        Some(u) => load(&u),
        None => Ok(None),
    }
}

/// Forget the secret of `dropped` unless one of `keep` uses the same one.
pub fn release(dropped: &CloudConfig, keep: &[&CloudConfig]) {
    let Some(user) = user_for(dropped) else { return };
    if keep.iter().any(|k| user_for(k).as_deref() == Some(user.as_str())) {
        return;
    }
    let _ = clear(&user);
}
