//! Game backups in the cloud (format 2), on any path-based [`Store`]:
//!
//! ```text
//! gamebackup/chunks/<blake3>                        chunks of all games (stored once)
//! gamebackup/games/<key>/versions/<id>.manifest     manifest (zstd JSON)
//! gamebackup/games/<key>/versions/<id>.json         summary, written last (commit marker)
//! gamebackup/games/<key>/lock.json                  device backing up this game
//! gamebackup/cleanup.json                           device deleting unused chunks
//! ```
//!
//! Kept apart from the save snapshots (`games/<key>/…` at the root): their
//! listing, retention and copying to another cloud never see game backups.
//! Chunks are shared by all games, so deleting unused ones looks at every
//! game's versions and waits while any game is being backed up.
//!
//! A backup and a cleanup each write their lock first and look for the
//! other's after: whichever starts second sees the first and backs off, so a
//! cleanup never deletes the chunks of a backup that hasn't committed yet.

use super::format::{Manifest, Summary, FORMAT};
use crate::cloud::store::{join, Store};
use crate::paths;
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub const PREFIX: &str = "gamebackup";
/// A lock not refreshed for this long belongs to a device that stopped.
const LOCK_MINUTES: i64 = 30;
/// How often a held lock is pushed forward.
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
/// A backup and a cleanup count each other's locks as live this long past
/// their expiry: the clocks of two devices differ, and one that slept may
/// not have noticed yet that its lock lapsed.
const OTHER_GRACE_MINUTES: i64 = 30;

pub fn chunks_dir() -> String {
    join(&[PREFIX, "chunks"])
}
pub fn chunk_path(id: &str) -> String {
    join(&[PREFIX, "chunks", id])
}
fn games_dir() -> String {
    join(&[PREFIX, "games"])
}
fn versions_dir(key: &str) -> String {
    join(&[PREFIX, "games", key, "versions"])
}
fn manifest_path(key: &str, id: &str) -> String {
    join(&[PREFIX, "games", key, "versions", &format!("{id}.manifest")])
}
fn summary_path(key: &str, id: &str) -> String {
    join(&[PREFIX, "games", key, "versions", &format!("{id}.json")])
}
fn game_lock_path(key: &str) -> String {
    join(&[PREFIX, "games", key, "lock.json"])
}
fn cleanup_lock_path() -> String {
    join(&[PREFIX, "cleanup.json"])
}

/// Committed versions of a game, oldest first. A summary that can't be read
/// is left out (it can't be shown or restored); cleaning up doesn't go by
/// this list (see `used_chunks`).
pub fn list_versions(store: &dyn Store, key: &str) -> Result<Vec<Summary>> {
    let entries = store.list(&versions_dir(key))?;
    let names: HashSet<&str> = entries.iter().filter(|e| !e.is_dir).map(|e| e.name.as_str()).collect();
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir) {
        let Some(id) = e.name.strip_suffix(".json") else { continue };
        if !names.contains(format!("{id}.manifest").as_str()) {
            continue;
        }
        if let Some(bytes) = store.read(&summary_path(key, id))? {
            if let Ok(s) = serde_json::from_slice::<Summary>(&bytes) {
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Games that have game backups.
pub fn list_games(store: &dyn Store) -> Result<Vec<String>> {
    Ok(store.list(&games_dir())?.into_iter().filter(|e| e.is_dir).map(|e| e.name).collect())
}

/// Manifests never change once written, so they are kept on this PC by hash.
fn cache_file(hash: &str) -> PathBuf {
    paths::data_dir().join("gamebackup-cache").join("manifests").join(format!("{hash}.zst"))
}

pub fn read_manifest(store: &dyn Store, key: &str, summary: &Summary) -> Result<Manifest> {
    let cached = cache_file(&summary.manifest_hash);
    if let Ok(bytes) = std::fs::read(&cached) {
        if let Ok(m) = Manifest::decode(&bytes, summary) {
            return Ok(m);
        }
    }
    let bytes = store.read(&manifest_path(key, &summary.id))?.ok_or_else(|| {
        anyhow!(crate::tr!(
            "版本 {} 已不在云端（可能刚被删除）",
            "Version {} is no longer in the cloud (it may have just been deleted)",
            summary.id
        ))
    })?;
    let m = Manifest::decode(&bytes, summary)?;
    let _ = crate::util::atomic_write(&cached, &bytes);
    Ok(m)
}

/// Write the manifest, then the summary that commits it.
pub fn commit_version(store: &dyn Store, manifest: &Manifest) -> Result<Summary> {
    let (bytes, summary) = manifest.encode()?;
    store.write(&manifest_path(&manifest.game, &manifest.id), &bytes)?;
    store.write(&summary_path(&manifest.game, &manifest.id), &serde_json::to_vec_pretty(&summary)?)?;
    let _ = crate::util::atomic_write(&cache_file(&summary.manifest_hash), &bytes);
    Ok(summary)
}

/// Uncommit (summary first), then delete the manifest. Chunks stay until
/// [`collect_garbage`].
pub fn delete_version(store: &dyn Store, key: &str, id: &str) -> Result<()> {
    store.remove(&summary_path(key, id))?;
    store.remove(&manifest_path(key, id))
}

/// Chunks in the cloud: name → stored size.
pub fn list_chunks(store: &dyn Store) -> Result<HashMap<String, u64>> {
    Ok(store
        .list(&chunks_dir())?
        .into_iter()
        .filter(|e| !e.is_dir && e.name.len() == 64 && !e.name.contains('.'))
        .map(|e| (e.name, e.size))
        .collect())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcReport {
    pub deleted_chunks: usize,
    pub freed_bytes: u64,
    pub kept_chunks: usize,
    /// Not cleaned up because a device is backing up, or a version can't be
    /// read (it is done later).
    pub postponed: bool,
    /// Why it was postponed, when it wasn't a backup in progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Whether a device is backing up a game other than `own_game`.
fn backing_up(store: &dyn Store, own_game: Option<&str>) -> Result<bool> {
    for g in list_games(store)? {
        if Some(g.as_str()) != own_game && busy_at(store, &game_lock_path(&g))?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Delete chunks no version of any game uses. Skipped (and reported as
/// postponed) while another device backs up a game, since its chunks are not
/// committed yet, and while a version can't be read, since nobody knows
/// which chunks it needs. `own_game` is the game whose lock the caller holds.
pub fn collect_garbage(store: &dyn Store, device: (&str, &str), own_game: Option<&str>, progress: &dyn Fn(usize, usize)) -> Result<GcReport> {
    let postponed = |reason: Option<String>| GcReport { postponed: true, reason, ..Default::default() };
    let lock = match Lock::acquire_path(store, &cleanup_lock_path(), device.0, device.1, "cleanup") {
        Ok(l) => l,
        Err(_) => return Ok(postponed(None)),
    };
    // The cleanup lock is written: a backup starting from now on sees it and
    // waits; one that started before is seen here.
    if backing_up(store, own_game)? {
        lock.release()?;
        return Ok(postponed(None));
    }
    let games = list_games(store)?;
    let used = match used_chunks(store, &games, &lock)? {
        Ok(used) => used,
        Err(reason) => {
            lock.release()?;
            return Ok(postponed(Some(reason)));
        }
    };
    let chunks = list_chunks(store)?;
    // Once more right before deleting: a backup whose lock showed up late
    // (listings of some clouds lag) keeps its chunks.
    lock.confirm()?;
    if backing_up(store, own_game)? {
        lock.release()?;
        return Ok(postponed(None));
    }
    let unused: Vec<(&String, &u64)> = chunks.iter().filter(|(id, _)| !used.contains(*id)).collect();
    let mut report = GcReport { kept_chunks: chunks.len() - unused.len(), ..Default::default() };
    for (i, (id, size)) in unused.iter().enumerate() {
        // Stops here if the lock lapsed: a backup may have started since.
        lock.refresh_if_due()?;
        store.remove(&chunk_path(id))?;
        report.deleted_chunks += 1;
        report.freed_bytes += **size;
        progress(i + 1, unused.len());
    }
    lock.release()?;
    Ok(report)
}

/// The format and chunk ids of a stored `.manifest`, read without its
/// summary. A manifest of a newer format may keep chunks where this version
/// doesn't look, so the caller checks the format.
fn chunk_ids(bytes: &[u8]) -> Result<(u32, Vec<String>)> {
    #[derive(Deserialize)]
    struct Id {
        h: String,
    }
    #[derive(Deserialize)]
    struct Part {
        #[serde(default)]
        chunks: Vec<Id>,
    }
    #[derive(Deserialize)]
    struct Ids {
        #[serde(default)]
        format: u32,
        #[serde(default)]
        components: Vec<Part>,
        #[serde(default)]
        chunks: Vec<Id>,
    }
    let json = zstd::stream::decode_all(bytes)?;
    let ids: Ids = serde_json::from_slice(&json)?;
    Ok((ids.format, ids.components.into_iter().flat_map(|c| c.chunks).chain(ids.chunks).map(|c| c.h).collect()))
}

/// Every chunk the versions of `games` use, from every manifest in their
/// versions folders: committed or not (a backup writes its manifest just
/// before committing), and whether or not its summary can be read. The
/// inner `Err` (a reason) when a committed version's manifest can't be read:
/// its chunks are unknown, so none can be deleted.
fn used_chunks(store: &dyn Store, games: &[String], lock: &Lock) -> Result<std::result::Result<HashSet<String>, String>> {
    let mut used = HashSet::new();
    for g in games {
        let entries = store.list(&versions_dir(g))?;
        let names: HashSet<&str> = entries.iter().filter(|e| !e.is_dir).map(|e| e.name.as_str()).collect();
        for id in entries.iter().filter(|e| !e.is_dir).filter_map(|e| e.name.strip_suffix(".manifest")) {
            lock.refresh_if_due()?;
            let committed = names.contains(format!("{id}.json").as_str());
            // Committed with a readable summary: the checked, cached manifest.
            let summary = match committed {
                true => store.read(&summary_path(g, id))?.and_then(|b| serde_json::from_slice::<Summary>(&b).ok()),
                false => None,
            };
            if let Some(m) = summary.and_then(|s| read_manifest(store, g, &s).ok()) {
                used.extend(m.components.into_iter().flat_map(|c| c.chunks).map(|c| c.h));
                continue;
            }
            // Otherwise straight from the manifest (gone meanwhile: nothing to keep).
            let Some(bytes) = store.read(&manifest_path(g, id))? else { continue };
            match chunk_ids(&bytes) {
                Ok((format, _)) if format > FORMAT => {
                    return Ok(Err(crate::tr!(
                        "{g} 的版本 {id} 是更新的 Ludonimbus 做的，请先更新 Ludonimbus",
                        "Version {id} of {g} was made by a newer Ludonimbus. Update Ludonimbus first"
                    )))
                }
                Ok((_, ids)) => used.extend(ids),
                // An uncommitted manifest that is damaged was never a version.
                Err(_) if !committed => {}
                Err(e) => {
                    return Ok(Err(crate::tr!(
                        "{g} 的版本 {id} 读不出来（{e:#}），请删掉它",
                        "Version {id} of {g} can't be read ({e:#}). Delete it"
                    )))
                }
            }
        }
    }
    Ok(Ok(used))
}

/// Delete versions of `key` beyond the newest `keep`, then unused chunks.
/// Returns the deleted version ids.
pub fn prune(
    store: &dyn Store,
    key: &str,
    keep: usize,
    device: (&str, &str),
    progress: &dyn Fn(usize, usize),
) -> Result<(Vec<String>, GcReport)> {
    let versions = list_versions(store, key)?;
    let keep = keep.max(1);
    let mut deleted = Vec::new();
    if versions.len() > keep {
        for s in &versions[..versions.len() - keep] {
            delete_version(store, key, &s.id)?;
            deleted.push(s.id.clone());
        }
    }
    let gc = if deleted.is_empty() { GcReport::default() } else { collect_garbage(store, device, Some(key), progress)? };
    Ok((deleted, gc))
}

// ---------------------------------------------------------------------------
// Locks

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockInfo {
    pub device_id: String,
    pub device_name: String,
    /// backup | cleanup
    pub kind: String,
    pub since: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    /// Tells two takes of the same device apart.
    pub token: String,
}

pub struct Lock<'a> {
    store: &'a dyn Store,
    path: String,
    info: Mutex<LockInfo>,
    refreshed: Mutex<Instant>,
    /// Found taken over by another device, or lapsed while this one slept.
    lost: AtomicBool,
    released: bool,
}

/// The lock was taken over, or it lapsed while this device slept: what it
/// was doing stops (what was uploaded stays for the next try).
#[derive(Debug)]
pub struct LockLost;

impl std::fmt::Display for LockLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            crate::tr!(
                "这台设备的锁过期了或被另一台设备接手了（可能睡眠了太久），已停止；再试一次会从断点继续",
                "This device's lock expired or another device took it over (it may have slept too long), so it stopped; trying again continues where it left off"
            )
        )
    }
}
impl std::error::Error for LockLost {}

fn read_lock(store: &dyn Store, path: &str) -> Result<Option<LockInfo>> {
    Ok(store.read(path)?.and_then(|b| serde_json::from_slice::<LockInfo>(&b).ok()))
}

fn lock_holder_at(store: &dyn Store, path: &str) -> Result<Option<LockInfo>> {
    Ok(read_lock(store, path)?.filter(|l| l.expires > Utc::now()))
}

/// A lock of the other kind of work (a backup for a cleanup, and the other
/// way round), still counted a while after it expired.
fn busy_at(store: &dyn Store, path: &str) -> Result<Option<LockInfo>> {
    Ok(read_lock(store, path)?.filter(|l| l.expires + Duration::minutes(OTHER_GRACE_MINUTES) > Utc::now()))
}

fn cleaning_up(l: &LockInfo) -> anyhow::Error {
    anyhow!(crate::tr!(
        "「{}」正在清理云端不再使用的数据，请过几分钟再试",
        "\"{}\" is cleaning up data no longer used in the cloud. Try again in a few minutes",
        l.device_name
    ))
}

/// Who is backing up a game, when someone is and the lock has not expired.
pub fn lock_holder(store: &dyn Store, key: &str) -> Result<Option<LockInfo>> {
    lock_holder_at(store, &game_lock_path(key))
}

/// Who is deleting unused chunks right now.
pub fn cleanup_holder(store: &dyn Store) -> Result<Option<LockInfo>> {
    lock_holder_at(store, &cleanup_lock_path())
}

impl<'a> Lock<'a> {
    /// Take the backup lock of a game. Fails when another device (or task)
    /// holds it, or while unused chunks are being deleted.
    pub fn acquire(store: &'a dyn Store, key: &str, device_id: &str, device_name: &str, kind: &str) -> Result<Lock<'a>> {
        if let Some(l) = busy_at(store, &cleanup_lock_path())? {
            return Err(cleaning_up(&l));
        }
        let lock = Self::acquire_path(store, &game_lock_path(key), device_id, device_name, kind)?;
        // Again now that this lock is written: a cleanup that started in
        // between either sees it or is seen here.
        if let Some(l) = busy_at(store, &cleanup_lock_path())? {
            drop(lock);
            return Err(cleaning_up(&l));
        }
        Ok(lock)
    }

    fn acquire_path(store: &'a dyn Store, path: &str, device_id: &str, device_name: &str, kind: &str) -> Result<Lock<'a>> {
        if let Some(l) = lock_holder_at(store, path)? {
            let what = if l.kind == "cleanup" { crate::tr!("清理", "cleaning up") } else { crate::tr!("备份", "backing up") };
            bail!(crate::tr!(
                "「{}」正在{what}这个游戏（从 {} 开始），请等它完成再试",
                "\"{}\" is {what} this game (since {}). Try again when it has finished",
                l.device_name,
                l.since.with_timezone(&chrono::Local).format("%H:%M")
            ));
        }
        let now = Utc::now();
        let info = LockInfo {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            kind: kind.to_string(),
            since: now,
            expires: now + Duration::minutes(LOCK_MINUTES),
            token: ulid::Ulid::new().to_string(),
        };
        store.write(path, &serde_json::to_vec_pretty(&info)?)?;
        // Two devices writing at once: the last write wins, the other backs off.
        let back = lock_holder_at(store, path)?;
        if back.as_ref().map(|l| &l.token) != Some(&info.token) {
            bail!(crate::tr!("另一台设备刚开始处理这个游戏，请稍后再试", "Another device just started working on this game. Try again later"));
        }
        Ok(Lock {
            store,
            path: path.to_string(),
            info: Mutex::new(info),
            refreshed: Mutex::new(Instant::now()),
            lost: AtomicBool::new(false),
            released: false,
        })
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    /// An error once the lock was found lost (no request).
    pub fn held(&self) -> Result<()> {
        if self.is_lost() {
            return Err(anyhow!(LockLost));
        }
        Ok(())
    }

    /// Check in the cloud that the lock is still this one and has not
    /// lapsed. Once it lapsed, a device may have taken it, worked and let it
    /// go again, so it counts as lost even when the file looks untouched.
    pub fn confirm(&self) -> Result<()> {
        self.held()?;
        let (token, lapsed) = {
            let info = self.info.lock().unwrap();
            (info.token.clone(), Utc::now() >= info.expires)
        };
        let taken = read_lock(self.store, &self.path)?.is_some_and(|l| l.token != token);
        if lapsed || taken {
            self.lost.store(true, Ordering::SeqCst);
            return Err(anyhow!(LockLost));
        }
        Ok(())
    }

    /// Push the expiry time forward. Never writes over a lock another device
    /// took, nor brings back one that lapsed.
    pub fn refresh(&self) -> Result<()> {
        self.confirm()?;
        let bytes = {
            let mut info = self.info.lock().unwrap();
            info.expires = Utc::now() + Duration::minutes(LOCK_MINUTES);
            serde_json::to_vec_pretty(&*info)?
        };
        self.store.write(&self.path, &bytes)?;
        *self.refreshed.lock().unwrap() = Instant::now();
        Ok(())
    }

    /// [`Lock::refresh`] when the last one was a while ago.
    pub fn refresh_if_due(&self) -> Result<()> {
        if self.refreshed.lock().unwrap().elapsed() >= REFRESH_EVERY {
            self.refresh()
        } else {
            self.held()
        }
    }

    /// Run `f` while a thread keeps pushing the lock's expiry forward, so
    /// long uploads, the commit and the cleanup after it all stay covered.
    pub fn while_held<T>(&self, f: impl FnOnce() -> T) -> T {
        self.while_held_every(REFRESH_EVERY, f)
    }

    fn while_held_every<T>(&self, every: std::time::Duration, f: impl FnOnce() -> T) -> T {
        struct SetOnDrop<'a>(&'a AtomicBool);
        impl Drop for SetOnDrop<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let done = AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut next = Instant::now() + every;
                while !done.load(Ordering::SeqCst) && !self.is_lost() {
                    std::thread::sleep(std::time::Duration::from_millis(200).min(every));
                    if Instant::now() >= next {
                        // A failed request is tried again soon; a lost lock ends it.
                        let ok = self.refresh().is_ok();
                        next = Instant::now() + if ok { every } else { every.min(std::time::Duration::from_secs(30)) };
                    }
                }
            });
            // Also when `f` panics, or the scope would wait for the thread forever.
            let _stop = SetOnDrop(&done);
            f()
        })
    }

    pub fn release(mut self) -> Result<()> {
        self.released = true;
        self.remove()
    }

    fn remove(&self) -> Result<()> {
        // Only our own lock: once ours expired, another device may hold it.
        let token = self.info.lock().unwrap().token.clone();
        match read_lock(self.store, &self.path)? {
            Some(l) if l.token == token => self.store.remove(&self.path),
            _ => Ok(()),
        }
    }
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::local::LocalFolder;
    use crate::cloud::store::Entry;

    type Hook = Box<dyn FnOnce(&LocalFolder) + Send>;

    /// A folder store that runs `hook` once, the first time `path` is
    /// written or listed: another device doing something at that moment.
    struct Meanwhile {
        inner: LocalFolder,
        path: String,
        hook: Mutex<Option<Hook>>,
    }

    impl Meanwhile {
        fn new(dir: &std::path::Path, path: String, hook: impl FnOnce(&LocalFolder) + Send + 'static) -> Self {
            Meanwhile { inner: LocalFolder::new(dir.to_path_buf()), path, hook: Mutex::new(Some(Box::new(hook))) }
        }
        fn at(&self, path: &str) {
            if path == self.path {
                if let Some(h) = self.hook.lock().unwrap().take() {
                    h(&self.inner);
                }
            }
        }
    }

    impl Store for Meanwhile {
        fn kind(&self) -> &'static str {
            "test"
        }
        fn label(&self) -> String {
            String::new()
        }
        fn check(&self) -> Result<()> {
            self.inner.check()
        }
        fn list(&self, dir: &str) -> Result<Vec<Entry>> {
            self.at(dir);
            self.inner.list(dir)
        }
        fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
            self.inner.read(path)
        }
        fn download(&self, path: &str, dest: &std::path::Path) -> Result<()> {
            self.inner.download(path, dest)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<()> {
            self.at(path);
            self.inner.write(path, data)
        }
        fn upload(&self, path: &str, src: &std::path::Path) -> Result<()> {
            self.inner.upload(path, src)
        }
        fn remove(&self, path: &str) -> Result<()> {
            self.inner.remove(path)
        }
    }

    fn lock_json(kind: &str, expires: DateTime<Utc>) -> Vec<u8> {
        let info = LockInfo {
            device_id: "d-other".into(),
            device_name: "OTHER".into(),
            kind: kind.into(),
            since: Utc::now(),
            expires,
            token: ulid::Ulid::new().to_string(),
        };
        serde_json::to_vec(&info).unwrap()
    }

    #[test]
    fn a_backup_backs_off_from_a_cleanup_that_started_while_it_took_its_lock() {
        let tmp = tempfile::tempdir().unwrap();
        // The cleanup lock appears after the backup looked for it, just before its own lock is written.
        let store = Meanwhile::new(tmp.path(), game_lock_path("g"), |s| {
            s.write(&cleanup_lock_path(), &lock_json("cleanup", Utc::now() + Duration::minutes(30))).unwrap()
        });
        let err = Lock::acquire(&store, "g", "d-a", "PC-A", "backup").err().expect("backs off");
        assert!(format!("{err:#}").contains("OTHER"), "{err:#}");
        assert!(store.read(&game_lock_path("g")).unwrap().is_none(), "its lock is gone again");
    }

    #[test]
    fn a_cleanup_keeps_chunks_of_a_backup_that_shows_up_late() {
        let tmp = tempfile::tempdir().unwrap();
        let id = "ab".repeat(32);
        // A backup takes its lock while the cleanup takes its own.
        let store = Meanwhile::new(tmp.path(), cleanup_lock_path(), |s| {
            s.write(&game_lock_path("g"), &lock_json("backup", Utc::now() + Duration::minutes(30))).unwrap()
        });
        store.write(&chunk_path(&id), b"LNCK").unwrap();
        assert!(collect_garbage(&store, ("d-a", "PC-A"), None, &|_, _| {}).unwrap().postponed);
        // ...or only shows up (slow listings) once the chunks were listed.
        let store = Meanwhile::new(tmp.path(), chunks_dir(), |s| {
            s.write(&game_lock_path("h"), &lock_json("backup", Utc::now() + Duration::minutes(30))).unwrap()
        });
        store.remove(&game_lock_path("g")).unwrap();
        assert!(collect_garbage(&store, ("d-a", "PC-A"), None, &|_, _| {}).unwrap().postponed);
        assert!(store.read(&chunk_path(&id)).unwrap().is_some(), "nothing deleted");
        assert!(cleanup_holder(&store).unwrap().is_none(), "cleanup lock released");
        // A lock that expired a moment ago may be a device whose clock is behind.
        store.write(&game_lock_path("h"), &lock_json("backup", Utc::now() - Duration::minutes(5))).unwrap();
        assert!(collect_garbage(&store, ("d-a", "PC-A"), None, &|_, _| {}).unwrap().postponed);
        // Long expired: the device stopped, the chunk goes.
        store.write(&game_lock_path("h"), &lock_json("backup", Utc::now() - Duration::hours(2))).unwrap();
        let gc = collect_garbage(&store, ("d-a", "PC-A"), None, &|_, _| {}).unwrap();
        assert_eq!((gc.postponed, gc.deleted_chunks), (false, 1));
        assert!(store.read(&chunk_path(&id)).unwrap().is_none());
    }

    #[test]
    fn a_lock_never_overwrites_another_devices_or_comes_back_after_lapsing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFolder::new(tmp.path().to_path_buf());
        let lock = Lock::acquire(&store, "g", "d-a", "PC-A", "backup").unwrap();
        lock.refresh().unwrap();
        // Taken over while this device slept.
        let theirs = lock_json("backup", Utc::now() + Duration::minutes(30));
        store.write(&game_lock_path("g"), &theirs).unwrap();
        let err = lock.refresh().unwrap_err();
        assert!(err.downcast_ref::<LockLost>().is_some(), "{err:#}");
        assert!(lock.is_lost() && lock.held().is_err());
        assert_eq!(store.read(&game_lock_path("g")).unwrap().unwrap(), theirs);
        drop(lock);
        assert_eq!(store.read(&game_lock_path("g")).unwrap().unwrap(), theirs, "theirs is not removed");

        // Lapsed: whatever is in the cloud now, it doesn't count any more.
        store.remove(&game_lock_path("g")).unwrap();
        let lock = Lock::acquire(&store, "g", "d-a", "PC-A", "backup").unwrap();
        lock.info.lock().unwrap().expires = Utc::now() - Duration::seconds(1);
        let before = store.read(&game_lock_path("g")).unwrap();
        assert!(lock.confirm().is_err() && lock.refresh().is_err());
        assert_eq!(store.read(&game_lock_path("g")).unwrap(), before, "not pushed forward");
    }

    #[test]
    fn a_held_lock_is_pushed_forward_until_the_work_is_done() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFolder::new(tmp.path().to_path_buf());
        let lock = Lock::acquire(&store, "g", "d-a", "PC-A", "backup").unwrap();
        let first = lock_holder(&store, "g").unwrap().unwrap().expires;
        let out = lock.while_held_every(std::time::Duration::from_millis(20), || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            7
        });
        assert_eq!(out, 7);
        assert!(lock_holder(&store, "g").unwrap().unwrap().expires > first);
        lock.release().unwrap();
        assert!(lock_holder(&store, "g").unwrap().is_none());
    }
}
