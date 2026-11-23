//! Quark through the OpenList server Ludonimbus runs (see [`crate::openlist`]).
//!
//! Plain WebDAV underneath, with two changes measured on a real account
//! (docs/plan-phase3.md section 1.4):
//! - uploads go straight to the final name: Quark uploads are atomic, and the
//!   temporary-file rename costs about 5 s per file;
//! - OpenList caches listings, so folders other devices write to are listed
//!   fresh (about 0.3 s) before reading them. Quark's driver finds a folder
//!   through its parent's (cached) listing, so a folder another device created
//!   is only found after refreshing its parent: see [`refresh_walk`].

use super::store::{Entry, Store};
use super::webdav::WebDav;
use super::Unavailable;
use crate::openlist::{self, Api};
use crate::tr;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A folder refreshed this recently is not refreshed again (reading many
/// files of one folder in a row).
const FRESH_FOR: Duration = Duration::from_secs(10);

/// Refresh `dir`; when OpenList doesn't know it (`try_one` returns
/// `Ok(false)`: a folder created elsewhere after its parent was cached),
/// refresh the parents first and try once more. A folder that is still not
/// found really doesn't exist, which is fine: its listing is empty.
fn refresh_walk(dir: &str, try_one: &mut dyn FnMut(&str) -> Result<bool>) -> Result<()> {
    if try_one(dir)? || dir.is_empty() {
        return Ok(());
    }
    refresh_walk(Quark::parent(dir), try_one)?;
    try_one(dir)?;
    Ok(())
}

pub struct Quark {
    dav: WebDav,
    port: u16,
    /// OpenList's admin password, read once when opening.
    admin: String,
    folder: String,
    api: Mutex<Option<Api>>,
    refreshed: Mutex<HashMap<String, Instant>>,
}

impl Quark {
    /// Start OpenList if needed and open the Ludonimbus folder in Quark.
    pub fn open(folder: &str, timeout: Duration) -> Result<Quark> {
        let run = openlist::ensure_running()?;
        let password = openlist::webdav_password()?;
        let admin = openlist::admin_password()?;
        // The WebDAV user's base path is the Ludonimbus folder itself.
        let dav = WebDav::new(&format!("http://127.0.0.1:{}/dav/", run.port), openlist::webdav_user(), &password, "", timeout)?
            .direct_uploads();
        Ok(Quark {
            dav,
            port: run.port,
            admin,
            folder: folder.trim_matches('/').to_string(),
            api: Mutex::new(None),
            refreshed: Mutex::new(HashMap::new()),
        })
    }

    /// List `dir` (relative to the Ludonimbus folder) straight from Quark,
    /// so OpenList's cache has what other devices wrote.
    fn refresh(&self, dir: &str) -> Result<()> {
        let dir = dir.trim_matches('/');
        if self.refreshed.lock().unwrap().get(dir).is_some_and(|t| t.elapsed() < FRESH_FOR) {
            return Ok(());
        }
        refresh_walk(dir, &mut |d| self.refresh_one(d))?;
        self.refreshed.lock().unwrap().insert(dir.to_string(), Instant::now());
        Ok(())
    }

    /// One forced listing; `Ok(false)` when OpenList doesn't find the folder.
    fn refresh_one(&self, dir: &str) -> Result<bool> {
        let mut path = format!("{}/{}", openlist::QUARK_MOUNT, self.folder);
        if !dir.is_empty() {
            path = format!("{path}/{dir}");
        }
        let mut api = self.api.lock().unwrap();
        if api.is_none() {
            *api = Some(Api::login_as(self.port, &self.admin)?);
        }
        match api.as_ref().unwrap().refresh(&path) {
            Ok(()) => Ok(true),
            Err(e) => {
                let text = format!("{e:#}");
                if text.to_lowercase().contains("not found") {
                    Ok(false)
                } else if openlist::is_login_error(&text) {
                    Err(anyhow!(tr!(
                        "夸克网盘登录已失效，请在设置里重新扫码登录",
                        "The Quark sign-in has expired. Scan the QR code again in Settings"
                    )))
                } else if e.downcast_ref::<Unavailable>().is_some() {
                    Err(e)
                } else {
                    Err(anyhow!(Unavailable(text)))
                }
            }
        }
    }

    /// Writes are retried twice (after 2 s and 4 s) when Quark or OpenList
    /// fails for the moment, refreshing the parent folder in between: a
    /// folder Quark has just created may not show up in its listing yet.
    /// Reads are not retried, so a missing network doesn't hold up a game.
    fn retry<T>(&self, path: &str, what: &str, mut f: impl FnMut() -> Result<T>) -> Result<T> {
        let mut wait = Duration::from_secs(2);
        let mut attempt = 0;
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err(e) if attempt < 2 && e.downcast_ref::<Unavailable>().is_some() => {
                    attempt += 1;
                    crate::logger::log("quark", format!("{what} {path} failed ({e:#}); retry {attempt} in {}s", wait.as_secs()));
                    std::thread::sleep(wait);
                    wait *= 2;
                    let dir = Self::parent(path);
                    self.refreshed.lock().unwrap().remove(dir);
                    let _ = self.refresh(dir);
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn parent(path: &str) -> &str {
        path.trim_matches('/').rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    }
}

impl Store for Quark {
    fn kind(&self) -> &'static str {
        "quark"
    }

    fn label(&self) -> String {
        tr!("夸克网盘 · {}", "Quark · {}", self.folder)
    }

    fn check(&self) -> Result<()> {
        self.refresh("")?;
        self.dav.check()
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.refresh(dir)?;
        self.dav.list(dir)
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.refresh(Self::parent(path))?;
        self.dav.read(path)
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.refresh(Self::parent(path))?;
        self.dav.download(path, dest)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.retry(path, "write", || self.dav.write(path, data))
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.retry(path, "upload", || self.dav.upload(path, src))
    }

    fn remove(&self, path: &str) -> Result<()> {
        self.retry(path, "remove", || self.dav.remove(path))
    }

    fn cache_scope(&self) -> Option<String> {
        Some(format!("quark|{}", self.folder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OpenList as Quark's driver behaves: a folder is found through its
    /// parent's cached listing; a forced listing refreshes only that folder.
    struct FakeOpenList {
        on_drive: Vec<&'static str>,
        cached: HashMap<String, Vec<String>>,
        calls: Vec<String>,
    }

    impl FakeOpenList {
        fn children(&self, dir: &str) -> Vec<String> {
            let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            let mut out: Vec<String> = self
                .on_drive
                .iter()
                .filter_map(|p| p.strip_prefix(&prefix))
                .filter(|rest| !rest.is_empty())
                .map(|rest| rest.split('/').next().unwrap().to_string())
                .collect();
            out.sort();
            out.dedup();
            out
        }

        fn refresh(&mut self, dir: &str) -> Result<bool> {
            self.calls.push(dir.to_string());
            if !dir.is_empty() {
                let (parent, name) = dir.rsplit_once('/').unwrap_or(("", dir));
                let known = match self.cached.get(parent) {
                    Some(list) => list.contains(&name.to_string()),
                    None => false,
                };
                if !known {
                    return Ok(false);
                }
            }
            let list = self.children(dir);
            self.cached.insert(dir.to_string(), list);
            Ok(true)
        }
    }

    #[test]
    fn refresh_finds_folders_made_on_another_device() {
        // This device listed games/fe before; another device then added snapshots.
        let mut ol = FakeOpenList {
            on_drive: vec!["games/fe/game.json", "games/fe/snapshots/01A.json", "games/zelda/snapshots/01B.json"],
            cached: HashMap::from([
                (String::new(), vec!["games".to_string()]),
                ("games".to_string(), vec!["fe".to_string()]),
                ("games/fe".to_string(), vec!["game.json".to_string()]),
            ]),
            calls: Vec::new(),
        };
        refresh_walk("games/fe/snapshots", &mut |d| ol.refresh(d)).unwrap();
        assert_eq!(ol.cached["games/fe/snapshots"], ["01A.json"]);
        assert_eq!(ol.calls, ["games/fe/snapshots", "games/fe", "games/fe/snapshots"]);

        // Two new levels.
        ol.calls.clear();
        refresh_walk("games/zelda/snapshots", &mut |d| ol.refresh(d)).unwrap();
        assert_eq!(ol.cached["games/zelda/snapshots"], ["01B.json"]);

        // Really missing: fine, and it stops at the first folder that exists.
        ol.calls.clear();
        refresh_walk("games/none/snapshots", &mut |d| ol.refresh(d)).unwrap();
        assert!(!ol.cached.contains_key("games/none/snapshots"));
        assert_eq!(ol.calls, ["games/none/snapshots", "games/none", "games", "games/none", "games/none/snapshots"]);
    }

    #[test]
    fn parent_folder() {
        assert_eq!(Quark::parent("games/fe/snapshots/01A.json"), "games/fe/snapshots");
        assert_eq!(Quark::parent("devices.json"), "");
    }
}
