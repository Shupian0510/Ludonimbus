//! "Cloud" in a plain folder: a NAS share, a USB drive, or a folder kept in
//! sync by Google Drive for desktop / OneDrive. Also used by the tests.

use super::store::{temp_name, Entry, Store};
use super::Unavailable;
use crate::util;
use anyhow::{anyhow, Context, Result};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub struct LocalFolder {
    root: PathBuf,
}

impl LocalFolder {
    pub fn new(root: PathBuf) -> Self {
        LocalFolder { root }
    }

    fn abs(&self, path: &str) -> PathBuf {
        let mut p = self.root.clone();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }
}

/// A folder a sync client (OneDrive, Dropbox, Google Drive for desktop…)
/// keeps in the cloud, offered as a place for the "local folder" cloud.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFolder {
    /// The sync client, e.g. "OneDrive".
    pub label: String,
    /// `Ludonimbus` inside the synced folder: where saves would go.
    pub path: String,
    /// That folder is already there (another PC may have put saves in it).
    pub existing: bool,
}

/// Synced folders found on this PC.
pub fn sync_folders() -> Vec<SyncFolder> {
    let mut list: Vec<(String, PathBuf)> = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(p) = std::env::var_os(var).map(PathBuf::from) {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            list.push((name.unwrap_or_else(|| "OneDrive".into()), p));
        }
    }
    // Dropbox writes where its folders are to info.json.
    for base in [dirs::config_dir(), dirs::data_local_dir()].into_iter().flatten() {
        let Ok(text) = std::fs::read_to_string(base.join("Dropbox").join("info.json")) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        for kind in ["personal", "business"] {
            if let Some(p) = v.get(kind).and_then(|k| k.get("path")).and_then(|p| p.as_str()) {
                list.push(("Dropbox".into(), PathBuf::from(p)));
            }
        }
    }
    if let Some(h) = dirs::home_dir() {
        list.push(("Dropbox".into(), h.join("Dropbox")));
        list.push(("iCloud Drive".into(), h.join("iCloudDrive")));
        list.push(("Google Drive".into(), h.join("Google Drive")));
        list.push(("Synology Drive".into(), h.join("SynologyDrive")));
        list.push((crate::tr!("坚果云", "Nutstore"), h.join("Nutstore")));
        list.push((crate::tr!("百度网盘同步空间", "Baidu Sync"), h.join("BaiduSyncdisk")));
    }
    // Google Drive for desktop shows up as a drive with "My Drive" in it.
    for letter in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if !drive.is_dir() {
            continue;
        }
        for sub in ["My Drive", "我的云端硬盘"] {
            list.push(("Google Drive".into(), drive.join(sub)));
        }
        list.push((crate::tr!("百度网盘同步空间", "Baidu Sync"), drive.join("BaiduSyncdisk")));
    }

    let mut seen = std::collections::HashSet::new();
    let list: Vec<(String, PathBuf)> =
        list.into_iter().filter(|(_, p)| p.is_dir() && seen.insert(util::norm_path(&p.to_string_lossy()))).collect();
    let count = |l: &str| list.iter().filter(|(x, _)| x == l).count();
    list.iter()
        .map(|(label, p)| {
            let dir = p.join("Ludonimbus");
            let label = if count(label) > 1 { format!("{label} · {}", p.display()) } else { label.clone() };
            SyncFolder { label, existing: dir.is_dir(), path: dir.display().to_string() }
        })
        .collect()
}

impl Store for LocalFolder {
    fn kind(&self) -> &'static str {
        "localFolder"
    }

    fn label(&self) -> String {
        self.root.display().to_string()
    }

    fn check(&self) -> Result<()> {
        if !self.root.is_dir() {
            return Err(anyhow!(Unavailable(crate::tr!(
                "文件夹不存在或无法访问：{}",
                "The folder doesn't exist or can't be accessed: {}",
                self.root.display()
            ))));
        }
        Ok(())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.check()?;
        let Ok(rd) = std::fs::read_dir(self.abs(dir)) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            out.push(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir: md.is_dir(),
                size: md.len(),
                tag: format!("{mtime}-{}", md.len()),
            });
        }
        Ok(out)
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        match std::fs::read(self.abs(path)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow!(e).context(crate::tr!("无法读取 {path}", "Can't read {path}"))),
        }
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.check()?;
        let src = self.abs(path);
        std::fs::copy(&src, dest).with_context(|| crate::tr!("找不到快照文件 {}", "Snapshot file not found: {}", src.display()))?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.check()?;
        util::atomic_write(&self.abs(path), data)
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.check()?;
        let dest = self.abs(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.abs(&temp_name(path));
        std::fs::copy(src, &tmp).context(crate::tr!("复制快照到云端文件夹失败", "Couldn't copy the snapshot to the cloud folder"))?;
        std::fs::rename(&tmp, &dest)?;
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<()> {
        self.check()?;
        match std::fs::remove_file(self.abs(path)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!(e).context(crate::tr!("无法删除 {path}", "Can't delete {path}"))),
        }
    }
}
