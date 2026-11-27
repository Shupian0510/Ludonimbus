//! Cross-process lock: an OS lock on a guard file, and the lock file older
//! versions use.
//!
//! The GUI, the Steam wrapper and the CLI may all touch `config.json` and
//! `state.json`; every read-modify-write goes through one of these locks.
//! [`FileLock::hold`] is the kind for work that may take minutes, such as
//! syncing a game.
//!
//! The lock itself is an OS lock (`File::try_lock`) on `<lock file>.guard`.
//! The system lets go of it when its process ends, however that happens,
//! and nobody can break it: a holder that is slow (a Baidu sign-in refreshed
//! over a bad connection, where two refreshes at once lose the sign-in) or
//! asleep (a handheld suspended mid-write) keeps it. A hung holder keeps it
//! too; whoever waits for it gives up after the time it waits. The guard
//! file is never removed: one removed while another process waits on it
//! would let two processes lock two different files.
//!
//! Versions up to 0.6 lock by creating the lock file itself and removing it
//! when done, and one of them may still run during an update (a game started
//! before it). So whoever holds the guard also creates the lock file, and
//! waits while there is one. The lock file says who holds it: the process
//! id, and when that process started (an id is reused once its process is
//! gone). One left by a process that is gone is taken over at once, and so
//! is one this version wrote: whoever holds the guard knows nobody else of
//! this version holds the lock. One written by an older version is judged
//! as before: a short lock ([`FileLock::acquire`]) is broken after 30 s, a
//! long one only when its holder is gone, and one that doesn't say who
//! holds it (0.6 writes it empty) after 30 s. An older version breaks any
//! lock file after 30 s and removes it when done; that can't be changed in
//! a version already out, and lasts only until it is closed.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// A lock file of an older version nobody claims, or its short lock, is left
/// behind after this.
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
    /// Written by a holder of the guard (this version). Older versions
    /// ignore the field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    guarded: bool,
}

pub struct FileLock {
    path: PathBuf,
    /// What this lock wrote into its file.
    content: String,
    /// The locked guard file; `None` where the file system has no OS locks,
    /// and the lock file alone is the lock, as in older versions.
    guard: Option<File>,
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
        path.exists() && stale(path, false).is_none()
    }

    fn take(path: &Path, long: bool, keep_waiting: &mut dyn FnMut() -> bool) -> Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // A guard that can't be opened leaves the lock file alone, which
        // says what is wrong with the folder when it can't be created either.
        let mut guard = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(guard_path(path)).ok();
        while let Some(file) = &guard {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    if !keep_waiting() {
                        return Ok(None);
                    }
                    std::thread::sleep(POLL);
                }
                Err(TryLockError::Error(_)) => guard = None,
            }
        }
        let pid = std::process::id();
        let me = Holder { pid, started: crate::process::started(pid), long, token: ulid::Ulid::new().to_string(), guarded: guard.is_some() };
        let content = serde_json::to_string(&me)?;
        // Errors other than "it exists", one after the other: a folder that
        // can't be written to is no lock to wait for.
        let mut failures = 0;
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut file) => {
                    let _ = file.write_all(content.as_bytes());
                    return Ok(Some(FileLock { path: path.to_path_buf(), content, guard }));
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
                    if let Some(seen) = stale(path, guard.is_some()) {
                        // Only what was judged: an older version may have
                        // taken it over meanwhile. No process of this version
                        // touches it while the guard is held.
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

/// The file whose OS lock is the lock at `path`.
fn guard_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".guard");
    path.with_file_name(name)
}

/// The content of the lock file at `path` when it is left behind and may be
/// broken; `None` while its holder still has it (or it is gone).
/// `guarded`: the one asking holds the guard, so a lock file of this version
/// is one its writer left behind.
fn stale(path: &Path, guarded: bool) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let age = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).unwrap_or_default();
    let left = match serde_json::from_str::<Holder>(&text) {
        Ok(h) if h.pid != 0 => {
            let gone = !crate::process::pid_alive(h.pid)
                || matches!((h.started, crate::process::started(h.pid)), (Some(a), Some(b)) if a != b);
            if h.guarded {
                guarded || gone
            } else {
                gone || (!h.long && age > STALE_AFTER)
            }
        }
        _ => age > STALE_AFTER,
    };
    left.then_some(text)
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // An older version may have broken it and taken it meanwhile.
        if std::fs::read_to_string(&self.path).ok().as_deref() == Some(self.content.as_str()) {
            let _ = std::fs::remove_file(&self.path);
        }
        // Before the handle closes: Windows may let go of a lock some time
        // after its handle is closed.
        if let Some(guard) = &self.guard {
            let _ = guard.unlock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    fn age(path: &Path, by: Duration) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - by).unwrap();
    }

    /// Wait for a long lock up to `max`.
    fn hold_for(path: &Path, max: Duration) -> Option<FileLock> {
        let start = Instant::now();
        FileLock::hold(path, &mut || start.elapsed() < max).unwrap()
    }

    /// A lock file as an older version writes it (dev builds before the
    /// guard; 0.6 writes it empty).
    fn older(pid: u32, started: Option<u64>, long: bool) -> String {
        serde_json::json!({ "pid": pid, "started": started, "long": long, "token": "t" }).to_string()
    }

    #[test]
    fn a_second_holder_waits() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        let first = hold_for(&path, Duration::ZERO).expect("free");
        assert!(first.guard.is_some(), "this file system has OS locks");
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
        // The guard stays for the next one.
        assert!(guard_path(&path).exists());
    }

    #[test]
    fn a_lock_whose_holder_is_gone_is_taken_over() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        // A holder that is gone (no process has this id), just now.
        std::fs::write(&path, older(u32::MAX - 1, Some(1), true)).unwrap();
        assert!(!FileLock::is_held(&path));
        let start = Instant::now();
        assert!(hold_for(&path, Duration::from_secs(2)).is_some());
        assert!(start.elapsed() < Duration::from_secs(1));
        // This process, but one that started at another time: the id was
        // reused.
        let pid = std::process::id();
        if let Some(started) = crate::process::started(pid) {
            std::fs::write(&path, older(pid, Some(started + 1), true)).unwrap();
            assert!(hold_for(&path, Duration::from_secs(2)).is_some());
        }
        // A lock file of this version whose writer let go of the guard
        // without removing it: taken over at once, though its process runs.
        let left = Holder { pid, started: crate::process::started(pid), long: true, token: "t".into(), guarded: true };
        std::fs::write(&path, serde_json::to_string(&left).unwrap()).unwrap();
        assert!(FileLock::is_held(&path));
        let start = Instant::now();
        assert!(FileLock::acquire(&path, Duration::from_secs(2)).is_ok());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_live_holder_is_not_broken_however_old() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game.lock");
        let held = hold_for(&path, Duration::ZERO).expect("free");
        age(&path, Duration::from_secs(40));
        assert!(FileLock::is_held(&path));
        assert!(hold_for(&path, Duration::from_millis(300)).is_none());
        drop(held);
        // A short lock neither: its holder may be slow, or the computer was
        // asleep while it held it.
        let short = FileLock::acquire(&path, Duration::ZERO).unwrap();
        age(&path, Duration::from_secs(40));
        assert!(FileLock::is_held(&path));
        assert!(FileLock::acquire(&path, Duration::from_millis(300)).is_err());
        drop(short);
        assert!(FileLock::acquire(&path, Duration::ZERO).is_ok());
    }

    #[test]
    fn an_older_versions_lock_is_judged_its_way() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.lock");
        let pid = std::process::id();
        // Its holder runs: waited for.
        std::fs::write(&path, older(pid, crate::process::started(pid), false)).unwrap();
        assert!(FileLock::is_held(&path));
        assert!(FileLock::acquire(&path, Duration::from_millis(200)).is_err());
        // Until it is done with it.
        std::fs::remove_file(&path).unwrap();
        drop(FileLock::acquire(&path, Duration::from_secs(2)).unwrap());
        // A short lock that old is taken as left behind, as that version does.
        std::fs::write(&path, older(pid, crate::process::started(pid), false)).unwrap();
        age(&path, Duration::from_secs(40));
        assert!(!FileLock::is_held(&path));
        let mine = FileLock::acquire(&path, Duration::from_secs(2)).unwrap();
        // An older version that breaks this one after 30 s and takes it:
        // dropping this one doesn't remove the lock that replaced it.
        std::fs::write(&path, b"").unwrap();
        drop(mine);
        assert!(path.exists());
        // 0.6's empty lock file, fresh: waited for; after 30 s: broken.
        assert!(hold_for(&path, Duration::from_millis(200)).is_none());
        age(&path, Duration::from_secs(40));
        assert!(hold_for(&path, Duration::from_secs(1)).is_some());
    }

    #[test]
    fn an_older_version_sees_this_ones_lock() {
        // What an older version reads from the lock file: there is one while
        // held, and its fields are the ones it knows.
        #[derive(Deserialize)]
        struct Older {
            pid: u32,
            long: bool,
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.lock");
        let lock = FileLock::acquire(&path, Duration::ZERO).unwrap();
        let seen: Older = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((seen.pid, seen.long), (std::process::id(), false));
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn one_holder_at_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Arc::new(tmp.path().join("state.lock"));
        // Several take over a left-behind lock at once, then take turns.
        let left = Holder { pid: std::process::id(), started: None, long: false, token: "t".into(), guarded: true };
        std::fs::write(&*path, serde_json::to_string(&left).unwrap()).unwrap();
        let inside = Arc::new(AtomicBool::new(false));
        let turns = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (path, inside, turns) = (path.clone(), inside.clone(), turns.clone());
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        let _lock = FileLock::acquire(&path, Duration::from_secs(20)).unwrap();
                        assert!(!inside.swap(true, Ordering::SeqCst), "two holders at once");
                        std::thread::sleep(Duration::from_millis(1));
                        turns.fetch_add(1, Ordering::SeqCst);
                        inside.store(false, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(turns.load(Ordering::SeqCst), 80);
        assert!(!path.exists());
    }
}
