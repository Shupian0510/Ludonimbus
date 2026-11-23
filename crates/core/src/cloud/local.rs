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
