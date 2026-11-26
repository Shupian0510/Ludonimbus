//! Cross-process lock based on exclusively created lock files.
//!
//! The GUI, the Steam wrapper and the CLI may all touch `config.json` and
//! `state.json`; every read-modify-write goes through one of these locks.
//! [`FileLock::hold`] is the kind for work that may take minutes, such as
//! syncing a game.
//!
//! A lock file says who holds it: the process id, and when that process
//! started (an id is reused once its process is gone). A lock whose holder
//! is gone is taken over at once. A short lock ([`FileLock::acquire`]) is
//! also broken after 30 s, as one a hung process never let go of; a long one
//! never is while its holder runs. A lock file that doesn't say who holds it
//! (written by an older version, or by a process stopped before it could)
//! is broken after 30 s.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// A lock file nobody claims, or a short lock, is left behind after this.
const STALE_AFTER: Duration = Duration::from_secs(30);

/// How often a waiter looks again.
const POLL: Duration = Duration::from_millis(40);

/// What a lock file says about its holder.
#[derive(Serialize, Deserialize, PartialEq)]
struct Holder {
    pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started: Option<u64>,
    /// Held for as long as the work takes ([`FileLock::hold`]).
    #[serde(default)]
    long: bool,
    /// Tells two locks of one process apart.
    #[serde(default)]
    token: String,
}

pub struct FileLock {
    path: PathBuf,
    /// What this lock wrote into its file.
    content: String,
}

impl FileLock {
    /// A short lock: wait for it up to `timeout`.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        let start = Instant::now();
        match Self::take(path, false, &mut || start.elapsed() <= timeout)? {
            Some(lock) => Ok(lock),
            None => bail!(crate::tr!("等待文件锁超时：{}", "Timed out waiting for the file lock: {}", path.display())),
        }
    }

    /// A long lock: wait for it as long as `keep_waiting` says so. `Ok(None)`
    /// when it said stop before the lock was free.
    pub fn hold(path: &Path, keep_waiting: &mut dyn FnMut() -> bool) -> Result<Option<Self>> {
        Self::take(path, true, keep_waiting)
    }

    /// Whether someone holds the lock at `path` right now.
    pub fn is_held(path: &Path) -> bool {
        path.exists() && stale(path).is_none()
    }

    fn take(path: &Path, long: bool, keep_waiting: &mut dyn FnMut() -> bool) -> Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pid = std::process::id();
        let me = Holder { pid, started: crate::process::started(pid), long, token: ulid::Ulid::new().to_string() };
        let content = serde_json::to_string(&me)?;
        // Errors other than "it exists", one after the other: a folder that
        // can't be written to is no lock to wait for.
        let mut failures = 0;
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut file) => {
                    let _ = file.write_all(content.as_bytes());
                    return Ok(Some(FileLock { path: path.to_path_buf(), content }));
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::AlreadyExists && !path.exists() {
                        failures += 1;
                        if failures > 50 {
                            return Err(anyhow::Error::new(e).context(path.display().to_string()));
                        }
                    } else {
                        failures = 0;
                    }
                    if let Some(seen) = stale(path) {
                        // Only what was judged: another process may have
                        // taken it over meanwhile.
                        if std::fs::read_to_string(path).ok().as_deref() == Some(seen.as_str()) {
                            let _ = std::fs::remove_file(path);
                        }
                        continue;
                    }
                    if !keep_waiting() {
                        return Ok(None);
                    }
                    std::thread::sleep(POLL);
                }
            }
        }
    }
}

/// The content of the lock file at `path` when it is left behind and may be
/// broken; `None` while its holder still has it (or it is gone).
fn stale(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let age = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).unwrap_or_default();
    let left = match serde_json::from_str::<Holder>(&text) {
        Ok(h) if h.pid != 0 => {
            let gone = !crate::process::pid_alive(h.pid)
                || matches!((h.started, crate::process::started(h.pid)), (Some(a), Some(b)) if a != b);
            gone || (!h.long && age > STALE_AFTER)
        }
        _ => age > STALE_AFTER,
    };
    left.then_some(text)
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // A short lock may have been broken and taken by another meanwhile.
        if std::fs::read_to_string(&self.path).ok().as_deref() == Some(self.content.as_str()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(path: &Path, by: Duration) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - by).unwrap();
    }

    /// Wait for a long lock up to `max`.
    fn hold_for(path: &Path, max: Duration) -> Option<FileLock> {
        let start = Instant::now();
        FileLock::hold(path, &mut || start.elapsed() < max).unwrap()
    }

    #[test]
    fn a_second_holder_waits() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        let first = hold_for(&path, Duration::ZERO).expect("free");
        assert!(FileLock::is_held(&path));
        // Another thread of this process waits like another process would.
        let other = path.clone();
        let waiter = std::thread::spawn(move || hold_for(&other, Duration::from_millis(300)).is_some());
        assert!(!waiter.join().unwrap());
        // Once it is let go, the next one gets it, without waiting long.
        let path2 = path.clone();
        let waiter = std::thread::spawn(move || hold_for(&path2, Duration::from_secs(5)).is_some());
        std::thread::sleep(Duration::from_millis(100));
        drop(first);
        assert!(waiter.join().unwrap());
        assert!(!path.exists());
        assert!(!FileLock::is_held(&path));
    }

    #[test]
    fn a_lock_whose_holder_is_gone_is_taken_over() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        // A holder that is gone (no process has this id), just now.
        let gone = Holder { pid: u32::MAX - 1, started: Some(1), long: true, token: "t".into() };
        std::fs::write(&path, serde_json::to_string(&gone).unwrap()).unwrap();
        assert!(!FileLock::is_held(&path));
        let start = Instant::now();
        assert!(hold_for(&path, Duration::from_secs(2)).is_some());
        assert!(start.elapsed() < Duration::from_secs(1));
        // This process, but one that started at another time: the id was
        // reused.
        let pid = std::process::id();
        if let Some(started) = crate::process::started(pid) {
            let reused = Holder { pid, started: Some(started + 1), long: true, token: "t".into() };
            std::fs::write(&path, serde_json::to_string(&reused).unwrap()).unwrap();
            assert!(hold_for(&path, Duration::from_secs(2)).is_some());
        }
    }

    #[test]
    fn a_live_holder_is_not_broken_however_old() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        let held = hold_for(&path, Duration::ZERO).expect("free");
        age(&path, Duration::from_secs(40));
        assert!(FileLock::is_held(&path));
        assert!(hold_for(&path, Duration::from_millis(300)).is_none());
        // A short lock that old is taken as left behind, as it always was.
        drop(held);
        let short = FileLock::acquire(&path, Duration::ZERO).unwrap();
        age(&path, Duration::from_secs(40));
        let again = FileLock::acquire(&path, Duration::from_secs(2)).unwrap();
        // The broken one doesn't remove the lock that replaced it.
        drop(short);
        assert!(path.exists());
        drop(again);
        assert!(!path.exists());
        // So is a lock file that doesn't say who holds it.
        std::fs::write(&path, b"").unwrap();
        assert!(hold_for(&path, Duration::from_millis(200)).is_none());
        age(&path, Duration::from_secs(40));
        assert!(hold_for(&path, Duration::from_secs(1)).is_some());
    }
}
