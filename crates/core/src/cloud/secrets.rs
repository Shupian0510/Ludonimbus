//! Passwords and secret keys of WebDAV / S3 clouds, the Baidu Netdisk sign-in
//! and the SecretKey of an own Baidu app, kept in the system's credential
//! store (never in `config.json`, see [`crate::credstore`]). Google Drive
//! keeps its refresh token separately, see [`super::oauth`].

use crate::config::CloudConfig;
use anyhow::Result;

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

pub fn store(user: &str, secret: &str) -> Result<()> {
    crate::credstore::set(SERVICE, user, secret)
}

pub fn load(user: &str) -> Result<Option<String>> {
    crate::credstore::get(SERVICE, user)
}

pub fn clear(user: &str) -> Result<()> {
    crate::credstore::delete(SERVICE, user)
}

pub fn load_for(cloud: &CloudConfig) -> Result<Option<String>> {
    match user_for(cloud) {
        Some(u) => load(&u),
        None => Ok(None),
    }
}

/// Forget the secret of `dropped` unless one of `keep` uses the same one.
pub fn release(dropped: &CloudConfig, keep: &[&CloudConfig]) {
    if let CloudConfig::BaiduPan { app_key, .. } = dropped {
        let same_app = |k: &&CloudConfig| matches!(k, CloudConfig::BaiduPan { app_key: a, .. } if a.trim() == app_key.trim());
        if !app_key.trim().is_empty() && !keep.iter().any(same_app) {
            let _ = clear(&super::baidu::app_secret_user(app_key));
        }
    }
    // Quark's sign-in lives in OpenList's data folder.
    if matches!(dropped, CloudConfig::Quark { .. }) && !keep.iter().any(|k| matches!(k, CloudConfig::Quark { .. })) {
        crate::openlist::forget();
        return;
    }
    let Some(user) = user_for(dropped) else { return };
    if keep.iter().any(|k| user_for(k).as_deref() == Some(user.as_str())) {
        return;
    }
    let _ = clear(&user);
}
