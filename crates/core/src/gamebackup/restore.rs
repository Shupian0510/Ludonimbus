//! Restore a game backup version into a folder: chunks are downloaded in
//! parallel, checked against their names and written at their offsets
//! (one chunk can span many small files). Finished chunks are recorded in
//! `.ludonimbus-restore.json` in the target folder, so an interrupted restore
//! of the same version continues there. At the end every file is checked
//! against its hash.

use super::format::{Manifest, Summary};
use super::{chunk, remote, retry, Cancelled};
use crate::cloud::store::Store;
use crate::util;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const JOURNAL: &str = ".ludonimbus-restore.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// preparing | downloading | verifying | finishing
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub elapsed_secs: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub target: String,
    pub files: usize,
    pub bytes: u64,
    /// Chunks a previous, interrupted restore had already written.
    pub resumed_chunks: usize,
    /// Files that failed the final check and were downloaded again.
    pub repaired_files: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct Journal {
    version: String,
    done: Vec<usize>,
}

/// `target` joined with a `/`-separated path from a manifest, refusing
/// anything that could leave `target`.
pub fn safe_join(target: &Path, rel: &str) -> Result<PathBuf> {
    let mut out = target.to_path_buf();
    for part in rel.split('/') {
        let p = Path::new(part);
        let ok = !part.is_empty()
            && part != "."
            && part != ".."
            && !part.contains(['\\', ':'])
            && p.components().all(|c| matches!(c, Component::Normal(_)));
        if !ok {
            bail!(crate::tr!("备份里有不安全的路径：{rel}", "The backup has an unsafe path: {rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

/// The folder a restore goes to when the user picked `chosen`: `chosen`
/// itself when it is missing, empty or holds an unfinished restore of this
/// version; otherwise a folder named after the backed-up one inside it.
pub fn resolve_target(chosen: &Path, manifest_root: &str, version: &str) -> PathBuf {
    let empty = std::fs::read_dir(chosen).map(|mut d| d.next().is_none()).unwrap_or(true);
    if empty || journal_version(chosen).as_deref() == Some(version) {
        return chosen.to_path_buf();
    }
    let name = Path::new(manifest_root.trim_end_matches(['\\', '/']))
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Game".into());
    chosen.join(name)
}

/// The version an unfinished restore into `target` was restoring.
pub fn journal_version(target: &Path) -> Option<String> {
    let bytes = std::fs::read(target.join(JOURNAL)).ok()?;
    serde_json::from_slice::<Journal>(&bytes).ok().map(|j| j.version)
}

#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // The nearest folder that exists (the target may not exist yet).
    let mut d = dir.to_path_buf();
    while !d.exists() {
        d = d.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = d.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub fn free_space(_: &Path) -> Option<u64> {
    None
}

/// Where each chunk's bytes go: (file index, offset in file, offset in chunk, length).
fn spans(m: &Manifest) -> Vec<Vec<(usize, u64, usize, usize)>> {
    let offsets = m.file_offsets();
    let mut out = Vec::with_capacity(m.chunks.len());
    let mut start = 0u64;
    for c in &m.chunks {
        let end = start + c.s;
        let mut v = Vec::new();
        let mut i = offsets.partition_point(|&o| o <= start).saturating_sub(1);
        while i < m.files.len() && offsets[i] < end {
            let (fo, fs) = (offsets[i], m.files[i].s);
            let from = start.max(fo);
            let to = end.min(fo + fs);
            if to > from {
                v.push((i, from - fo, (from - start) as usize, (to - from) as usize));
            }
            i += 1;
        }
        out.push(v);
        start = end;
    }
    out
}

fn hash_file(p: &Path) -> Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

struct Run<'a> {
    store: &'a dyn Store,
    key: &'a str,
    m: &'a Manifest,
    target: &'a Path,
    paths: Vec<PathBuf>,
    spans: Vec<Vec<(usize, u64, usize, usize)>>,
    threads: usize,
    cancel: &'a AtomicBool,
    progress: &'a (dyn Fn(&Progress) + Sync),
    started: Instant,
    done: Mutex<HashSet<usize>>,
    last_flush: Mutex<Instant>,
}

impl Run<'_> {
    fn report(&self, phase: &'static str, done: u64, total: u64) {
        (self.progress)(&Progress { phase, done, total, elapsed_secs: self.started.elapsed().as_secs_f64() });
    }

    fn flush(&self) -> Result<()> {
        let done: Vec<usize> = {
            let mut v: Vec<usize> = self.done.lock().unwrap().iter().copied().collect();
            v.sort_unstable();
            v
        };
        let j = Journal { version: self.m.id.clone(), done };
        util::atomic_write(&self.target.join(JOURNAL), &serde_json::to_vec(&j)?)
    }

    fn write_chunk(&self, idx: usize, raw: &[u8]) -> Result<()> {
        for &(fi, file_off, chunk_off, len) in &self.spans[idx] {
            let p = &self.paths[fi];
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .with_context(|| crate::tr!("无法写入 {}", "Can't write {}", p.display()))?;
            f.seek(SeekFrom::Start(file_off))?;
            f.write_all(&raw[chunk_off..chunk_off + len])
                .with_context(|| crate::tr!("无法写入 {}", "Can't write {}", p.display()))?;
        }
        Ok(())
    }

    /// Download and write every chunk index in `todo` (grouped by chunk id,
    /// so a chunk used twice is downloaded once).
    fn download(&self, todo: &[usize]) -> Result<()> {
        let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for &i in todo {
            groups.entry(self.m.chunks[i].h.as_str()).or_default().push(i);
        }
        let groups: Vec<(&str, Vec<usize>)> = groups.into_iter().collect();
        let total: u64 = todo.iter().map(|&i| self.m.chunks[i].s).sum();
        let bytes = AtomicU64::new(0);
        let next = AtomicUsize::new(0);
        let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        self.report("downloading", 0, total);
        std::thread::scope(|s| {
            for _ in 0..self.threads {
                s.spawn(|| loop {
                    if self.cancel.load(Ordering::Relaxed) || failed.lock().unwrap().is_some() {
                        return;
                    }
                    let Some((id, idxs)) = groups.get(next.fetch_add(1, Ordering::SeqCst)) else { return };
                    let c = &self.m.chunks[idxs[0]];
                    let result = retry(self.cancel, || {
                        self.store.read(&remote::chunk_path(self.key, id))?.ok_or_else(|| {
                            anyhow!(crate::tr!(
                                "云端缺少数据块 {id}，这个版本可能已被删除或损坏",
                                "Chunk {id} is missing in the cloud; this version may have been deleted or damaged"
                            ))
                        })
                    })
                    .and_then(|obj| chunk::decode(&obj, id, c.s))
                    .and_then(|raw| {
                        for &i in idxs {
                            self.write_chunk(i, &raw)?;
                        }
                        Ok(())
                    });
                    match result {
                        Ok(()) => {
                            self.done.lock().unwrap().extend(idxs.iter().copied());
                            let n = bytes.fetch_add(c.s * idxs.len() as u64, Ordering::SeqCst) + c.s * idxs.len() as u64;
                            self.report("downloading", n, total);
                            let mut last = self.last_flush.lock().unwrap();
                            if last.elapsed() >= Duration::from_secs(5) {
                                *last = Instant::now();
                                drop(last);
                                let _ = self.flush();
                            }
                        }
                        Err(e) => {
                            failed.lock().unwrap().get_or_insert(e);
                        }
                    }
                });
            }
        });
        self.flush()?;
        if let Some(e) = failed.into_inner().unwrap() {
            return Err(e);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!(Cancelled));
        }
        Ok(())
    }

    /// Indices of files whose content doesn't match the manifest.
    fn verify(&self, which: &[usize]) -> Result<Vec<usize>> {
        let total: u64 = which.iter().map(|&i| self.m.files[i].s).sum();
        let bytes = AtomicU64::new(0);
        let next = AtomicUsize::new(0);
        let bad = Mutex::new(Vec::new());
        let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        self.report("verifying", 0, total);
        std::thread::scope(|s| {
            for _ in 0..self.threads {
                s.spawn(|| loop {
                    if self.cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(&i) = which.get(next.fetch_add(1, Ordering::SeqCst)) else { return };
                    let f = &self.m.files[i];
                    match hash_file(&self.paths[i]) {
                        Ok(h) if h == f.h => {}
                        Ok(_) => bad.lock().unwrap().push(i),
                        Err(e) => {
                            failed.lock().unwrap().get_or_insert(e);
                            return;
                        }
                    }
                    let n = bytes.fetch_add(f.s, Ordering::SeqCst) + f.s;
                    self.report("verifying", n, total);
                });
            }
        });
        if let Some(e) = failed.into_inner().unwrap() {
            return Err(e);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!(Cancelled));
        }
        let mut bad = bad.into_inner().unwrap();
        bad.sort_unstable();
        Ok(bad)
    }
}

/// Restore `summary` of game `key` into `target` (created when missing).
pub fn restore(
    store: &dyn Store,
    key: &str,
    summary: &Summary,
    target: &Path,
    threads: usize,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
) -> Result<(Manifest, Report)> {
    let started = Instant::now();
    (progress)(&Progress { phase: "preparing", done: 0, total: 0, elapsed_secs: 0.0 });
    let m = remote::read_manifest(store, key, summary)?;
    let resuming = journal_version(target).as_deref() == Some(m.id.as_str());
    if !resuming {
        if let Some(free) = free_space(target) {
            if free < m.total_size + 64 * 1024 * 1024 {
                bail!(crate::tr!(
                    "目标磁盘空间不够：需要 {} MB，只剩 {} MB",
                    "Not enough space on the target disk: {} MB needed, {} MB free",
                    m.total_size / 1_048_576 + 64,
                    free / 1_048_576
                ));
            }
        }
    }
    std::fs::create_dir_all(target).with_context(|| crate::tr!("无法创建文件夹 {}", "Can't create the folder {}", target.display()))?;
    for d in &m.dirs {
        std::fs::create_dir_all(safe_join(target, d)?)?;
    }
    let mut paths = Vec::with_capacity(m.files.len());
    for f in &m.files {
        let p = safe_join(target, &f.p)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let keep = resuming && std::fs::metadata(&p).is_ok_and(|md| md.is_file() && md.len() == f.s);
        if !keep {
            let file = std::fs::File::create(&p).with_context(|| crate::tr!("无法创建 {}", "Can't create {}", p.display()))?;
            file.set_len(f.s)?;
        }
        paths.push(p);
    }
    let done: HashSet<usize> = if resuming {
        std::fs::read(target.join(JOURNAL))
            .ok()
            .and_then(|b| serde_json::from_slice::<Journal>(&b).ok())
            .map(|j| j.done.into_iter().filter(|&i| i < m.chunks.len()).collect())
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let resumed_chunks = done.len();
    let run = Run {
        store,
        key,
        spans: spans(&m),
        m: &m,
        target,
        paths,
        threads: threads.clamp(1, 16),
        cancel,
        progress,
        started,
        done: Mutex::new(done),
        last_flush: Mutex::new(Instant::now()),
    };
    run.flush()?;
    let todo: Vec<usize> = {
        let done = run.done.lock().unwrap();
        (0..m.chunks.len()).filter(|i| !done.contains(i)).collect()
    };
    run.download(&todo)?;

    let all: Vec<usize> = (0..m.files.len()).collect();
    let bad = run.verify(&all)?;
    let repaired_files = bad.len();
    if !bad.is_empty() {
        // Download the chunks of the damaged files again, then check them once more.
        let bad_set: HashSet<usize> = bad.iter().copied().collect();
        let redo: Vec<usize> = (0..m.chunks.len()).filter(|&c| run.spans[c].iter().any(|s| bad_set.contains(&s.0))).collect();
        {
            let mut done = run.done.lock().unwrap();
            for c in &redo {
                done.remove(c);
            }
        }
        run.download(&redo)?;
        let still = run.verify(&bad)?;
        if let Some(&i) = still.first() {
            bail!(crate::tr!(
                "恢复后的文件校验不通过：{}（共 {} 个）。请检查磁盘后重试",
                "Restored files failed the check: {} ({} in all). Check the disk and try again",
                m.files[i].p,
                still.len()
            ));
        }
    }

    run.report("finishing", 0, 0);
    for (f, p) in m.files.iter().zip(&run.paths) {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(p) {
            let t = std::time::UNIX_EPOCH + Duration::new(f.m.max(0) as u64, f.n.min(999_999_999));
            let _ = file.set_modified(t);
        }
    }
    let _ = std::fs::remove_file(target.join(JOURNAL));
    let report = Report {
        target: target.display().to_string(),
        files: m.files.len(),
        bytes: m.total_size,
        resumed_chunks,
        repaired_files,
    };
    Ok((m, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_paths_that_leave_the_target() {
        let t = Path::new("C:\\games\\x");
        assert!(safe_join(t, "a/b.txt").is_ok());
        for bad in ["../x", "a/../../x", "C:/x", "a\\..\\x", "", "a//b", "/x"] {
            assert!(safe_join(t, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn picks_a_subfolder_for_a_folder_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let chosen = tmp.path().join("Games");
        assert_eq!(resolve_target(&chosen, "D:\\Old\\Zelda", "v1"), chosen);
        std::fs::create_dir_all(chosen.join("Other")).unwrap();
        assert_eq!(resolve_target(&chosen, "D:\\Old\\Zelda\\", "v1"), chosen.join("Zelda"));
        std::fs::write(chosen.join(JOURNAL), r#"{"version":"v1","done":[]}"#).unwrap();
        assert_eq!(resolve_target(&chosen, "D:\\Old\\Zelda", "v1"), chosen);
    }
}
