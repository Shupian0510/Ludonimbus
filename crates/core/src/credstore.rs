//! Where passwords, keys and sign-ins are kept: the Windows Credential
//! Manager, the macOS keychain, or on Linux the Secret Service (KWallet,
//! GNOME Keyring) when there is one. Steam Deck Game Mode and other
//! sessions without a Secret Service get a file only this user can read
//! (`credentials.json`, mode 0600) instead (docs/plan-platforms.md §1.1).

use anyhow::{anyhow, Result};

fn entry(service: &str, user: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(service, user)?)
}

#[cfg(target_os = "linux")]
mod file {
    use crate::{lockfile::FileLock, paths, util};
    use anyhow::Result;
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn path() -> std::path::PathBuf {
        paths::data_dir().join("credentials.json")
    }

    fn read() -> BTreeMap<String, String> {
        std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Mode 0600 from the start: the file is never readable by others, not
    /// even for a moment.
    fn write(m: &BTreeMap<String, String>) -> Result<()> {
        util::write_private(&path(), &serde_json::to_vec(m)?)
    }

    pub fn get(id: &str) -> Option<String> {
        read().get(id).cloned()
    }

    pub fn set(id: &str, secret: Option<&str>) -> Result<()> {
        // The app, the background service and the launch wrapper may each
        // save a sign-in at the same time; without the lock one of the two
        // would be lost.
        let _lock = FileLock::acquire(&paths::data_dir().join("credentials.lock"), Duration::from_secs(10))?;
        let mut m = read();
        let changed = match secret {
            Some(s) => m.insert(id.to_string(), s.to_string()).as_deref() != Some(s),
            None => m.remove(id).is_some(),
        };
        if changed {
            write(&m)?;
        }
        Ok(())
    }
}

/// The saved secret, if any.
pub fn get(service: &str, user: &str) -> Result<Option<String>> {
    let res = entry(service, user).and_then(|e| match e.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!(crate::tr!("无法读取保存的密码：{e}", "Couldn't read the saved password: {e}"))),
    });
    #[cfg(target_os = "linux")]
    {
        let id = format!("{service}/{user}");
        match res {
            Ok(Some(s)) => return Ok(Some(s)),
            Ok(None) | Err(_) => return Ok(file::get(&id)),
        }
    }
    #[allow(unreachable_code)]
    res
}

/// Save a secret (replacing the old one).
pub fn set(service: &str, user: &str, secret: &str) -> Result<()> {
    let res =
        entry(service, user).and_then(|e| e.set_password(secret).map_err(|e| anyhow!(crate::tr!("无法保存密码：{e}", "Couldn't save the password: {e}"))));
    #[cfg(target_os = "linux")]
    {
        let id = format!("{service}/{user}");
        return match res {
            // Saved in the Secret Service: no copy in the file.
            Ok(()) => file::set(&id, None),
            Err(_) => file::set(&id, Some(secret)),
        };
    }
    #[allow(unreachable_code)]
    res
}

/// Forget a secret.
pub fn delete(service: &str, user: &str) -> Result<()> {
    let res = entry(service, user).and_then(|e| match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!(crate::tr!("无法删除保存的密码：{e}", "Couldn't delete the saved password: {e}"))),
    });
    #[cfg(target_os = "linux")]
    {
        let _ = res;
        return file::set(&format!("{service}/{user}"), None);
    }
    #[allow(unreachable_code)]
    res
}
