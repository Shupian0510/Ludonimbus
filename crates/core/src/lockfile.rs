//! Cross-process lock based on exclusively created lock files.
//!
//! The GUI, the Steam wrapper and the CLI may all touch `config.json` and
//! `state.json`; every read-modify-write goes through one of these locks.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => {
                    return Ok(FileLock {
                        path: path.to_path_buf(),
                    })
                }
                Err(_) => {
                    // Break stale locks left behind by a crashed process.
                    if let Ok(meta) = std::fs::metadata(path) {
                        let age = meta
                            .modified()
                            .ok()
                            .and_then(|m| SystemTime::now().duration_since(m).ok())
                            .unwrap_or_default();
                        if age > Duration::from_secs(30) {
                            let _ = std::fs::remove_file(path);
                            continue;
                        }
                    }
                    if start.elapsed() > timeout {
                        bail!("等待文件锁超时：{}", path.display());
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
