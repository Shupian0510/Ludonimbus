//! "Cloud" in a plain folder: a NAS share, a USB drive, or a folder kept in
//! sync by Google Drive for desktop / OneDrive. Also used by the tests.

use super::{AccountInfo, CloudGame, CloudProvider, DeviceInfo, Unavailable};
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use crate::util;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub struct LocalFolder {
    root: PathBuf,
}

impl LocalFolder {
    pub fn new(root: PathBuf) -> Self {
        LocalFolder { root }
    }

    fn check(&self) -> Result<()> {
        if !self.root.is_dir() {
            return Err(anyhow!(Unavailable(format!(
                "文件夹不存在或无法访问：{}",
                self.root.display()
            ))));
        }
        Ok(())
    }

    fn snapshots_dir(&self, game: &str) -> PathBuf {
        self.root.join("games").join(game).join("snapshots")
    }
}

impl CloudProvider for LocalFolder {
    fn kind(&self) -> &'static str {
        "localFolder"
    }

    fn account(&self) -> Result<AccountInfo> {
        self.check()?;
        Ok(AccountInfo {
            label: self.root.display().to_string(),
            used: None,
            total: None,
        })
    }

    fn list_snapshots(&self, game: &str) -> Result<Vec<SnapshotSummary>> {
        self.check()?;
        let dir = self.snapshots_dir(game);
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Ok(out);
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") && p.with_extension("zip").is_file() {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(meta) = serde_json::from_str::<SnapshotMeta>(&text) {
                        out.push(SnapshotSummary::from(&meta));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    fn get_meta(&self, game: &str, id: &str) -> Result<SnapshotMeta> {
        self.check()?;
        let p = self.snapshots_dir(game).join(format!("{id}.json"));
        let text = std::fs::read_to_string(&p).with_context(|| format!("找不到快照 {id}"))?;
        Ok(serde_json::from_str(&text)?)
    }

    fn upload(&self, meta: &SnapshotMeta, zip: &Path) -> Result<()> {
        self.check()?;
        let dir = self.snapshots_dir(&meta.game);
        std::fs::create_dir_all(&dir)?;
        let dest = dir.join(format!("{}.zip", meta.id));
        let tmp = dir.join(format!("{}.zip.lntmp", meta.id));
        std::fs::copy(zip, &tmp).context("复制快照到云端文件夹失败")?;
        std::fs::rename(&tmp, &dest)?;
        // The metadata file is written last and acts as the commit marker.
        util::atomic_write(&dir.join(format!("{}.json", meta.id)), &serde_json::to_vec_pretty(meta)?)?;
        Ok(())
    }

    fn download(&self, game: &str, id: &str, dest: &Path) -> Result<()> {
        self.check()?;
        let src = self.snapshots_dir(game).join(format!("{id}.zip"));
        std::fs::copy(&src, dest).with_context(|| format!("找不到快照文件 {}", src.display()))?;
        Ok(())
    }

    fn delete(&self, game: &str, id: &str) -> Result<()> {
        self.check()?;
        let dir = self.snapshots_dir(game);
        let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
        let _ = std::fs::remove_file(dir.join(format!("{id}.zip")));
        Ok(())
    }

    fn update_meta(&self, meta: &SnapshotMeta) -> Result<()> {
        self.check()?;
        let p = self.snapshots_dir(&meta.game).join(format!("{}.json", meta.id));
        util::atomic_write(&p, &serde_json::to_vec_pretty(meta)?)
    }

    fn put_device(&self, device: &DeviceInfo) -> Result<()> {
        self.check()?;
        let p = self.root.join("devices").join(format!("{}.json", device.id));
        util::atomic_write(&p, &serde_json::to_vec_pretty(device)?)
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        self.check()?;
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.root.join("devices")) {
            for e in rd.flatten() {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    if let Ok(d) = serde_json::from_str(&text) {
                        out.push(d);
                    }
                }
            }
        }
        Ok(out)
    }

    fn put_game(&self, game: &CloudGame) -> Result<()> {
        self.check()?;
        let p = self.root.join("games").join(&game.key).join("game.json");
        util::atomic_write(&p, &serde_json::to_vec_pretty(game)?)
    }

    fn list_games(&self) -> Result<Vec<CloudGame>> {
        self.check()?;
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.root.join("games")) {
            for e in rd.flatten() {
                if let Ok(text) = std::fs::read_to_string(e.path().join("game.json")) {
                    if let Ok(g) = serde_json::from_str(&text) {
                        out.push(g);
                    }
                }
            }
        }
        Ok(out)
    }
}
