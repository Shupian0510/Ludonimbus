//! Passwords and secret keys of WebDAV / S3 clouds, the Baidu Netdisk sign-in
//! and the SecretKey of an own Baidu app, kept in the system's credential
//! store (never in `config.json`, see [`crate::credstore`]). Google Drive
//! keeps its refresh token separately, see [`super::oauth`]; [`release`]
//! lets go of it as well.

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

/// Whether letting go of `dropped` lets go of the Google sign-in: there is
/// one for every Google Drive cloud ([`super::oauth`]), and none of `keep`
/// is one.
fn drops_google_sign_in(dropped: &CloudConfig, keep: &[&CloudConfig]) -> bool {
    let google = |c: &CloudConfig| matches!(c, CloudConfig::GoogleDrive { .. });
    google(dropped) && !keep.iter().any(|k| google(k))
}

/// Forget the secret of `dropped` unless one of `keep` uses the same one.
/// For Google Drive that is the refresh token, which would otherwise stay
/// in the credential store after the user moved to another cloud.
pub fn release(dropped: &CloudConfig, keep: &[&CloudConfig]) {
    if drops_google_sign_in(dropped, keep) {
        let _ = super::oauth::clear_refresh_token();
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(account: &str) -> CloudConfig {
        CloudConfig::GoogleDrive { client_id: String::new(), client_secret: String::new(), folder: "Ludonimbus".into(), account: Some(account.into()) }
    }

    #[test]
    fn the_google_sign_in_goes_with_the_last_google_drive() {
        let folder = CloudConfig::LocalFolder { path: "D:/Sync".into() };
        // Moved on from Drive: the copy it was kept for is done or declined.
        assert!(drops_google_sign_in(&drive("a"), &[&folder]));
        assert!(drops_google_sign_in(&drive("a"), &[]));
        // Still in use, as the current cloud or the one to copy from.
        assert!(!drops_google_sign_in(&drive("a"), &[&drive("b")]));
        assert!(!drops_google_sign_in(&drive("a"), &[&folder, &drive("a")]));
        // Other clouds have nothing to do with it.
        assert!(!drops_google_sign_in(&folder, &[]));
    }
}