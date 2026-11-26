//! Restore a game backup version: each component goes to its place
//! (`{program}` and `{content}` to the folders the user picked, the user
//! folders to this PC's). Files already there with the right content are
//! kept, so an emulator that is already installed isn't written again.
//! Chunks are downloaded in parallel, checked against their names and
//! written at their offsets (one chunk can span many small files). Finished
//! chunks are recorded on this PC, so an interrupted restore of the same
//! version to the same places continues. At the end every written file is
//! checked against its hash.

use super::components::Bases;
use super::format::{Component, Kind, Manifest, Summary};
use super::{chunk, remote, retry, Cancelled};
use crate::cloud::store::Store;
use crate::{paths, util};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where a restore puts things.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    /// The program's folder (or the whole folder). Also where the game's
    /// settings that live next to the program go.
    pub program: PathBuf,
    /// Write the program's files; `false` keeps the program already there.
    pub restore_program: bool,
    /// The folder for the game's own files; `None` = don't restore them.
    pub content: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// preparing | checking | downloading | verifying | finishing
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub elapsed_secs: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Files written.
    pub files: usize,
    /// Files that were already there with the right content.
    pub kept: usize,
    pub bytes: u64,
    /// Chunks a previous, interrupted restore had already written.
    pub resumed_chunks: usize,
    /// Files that failed the final check and were downloaded again.
    pub repaired_files: usize,
}

#[derive(Serialize, Deserialize, Default)]
struct Journal {
    version: String,
    targets: Option<Targets>,
    /// (component, chunk) pairs written.
    done: Vec<(usize, usize)>,
}

fn journal_file(key: &str) -> PathBuf {
    paths::data_dir().join("gamebackup-restore").join(format!("{key}.json"))
}

/// The version and places of an unfinished restore of `key` on this PC.
pub fn pending(key: &str) -> Option<(String, Targets)> {
    let j: Journal = serde_json::from_slice(&std::fs::read(journal_file(key)).ok()?).ok()?;
    Some((j.version, j.targets?))
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
            && p.components().all(|c| matches!(c, PathPart::Normal(_)));
        if !ok {
            bail!(crate::tr!("备份里有不安全的路径：{rel}", "The backup has an unsafe path: {rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

/// The folder a restore into `chosen` would use: `chosen` itself when it is
/// missing or empty, otherwise a folder named like the backed-up one inside it.
pub fn resolve_target(chosen: &Path, original: &str) -> PathBuf {
    let empty = std::fs::read_dir(chosen).map(|mut d| d.next().is_none()).unwrap_or(true);
    if empty {
        return chosen.to_path_buf();
    }
    // The folder's name, whichever system the backup was made on.
    let name = original
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|n| !n.is_empty() && !n.ends_with(':'))
        .map(str::to_string)
        .unwrap_or_else(|| "Game".into());
    // Picking the backed-up folder itself (it still has files): restore into it.
    if chosen.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&name)) {
        return chosen.to_path_buf();
    }
    chosen.join(name)
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

/// The bases of a restore: the picked folders, this PC's user folders, and
/// `{extraN}` folders where they were when their drive exists (else inside
/// the game files' folder).
pub fn bases_for(m: &Manifest, t: &Targets) -> Bases {
    let mut b = Bases::new(Some(&t.program), t.content.as_deref());
    let fallback = t.content.clone().unwrap_or_else(|| t.program.clone());
    for c in &m.components {
        for (name, orig) in &c.bases {
            if name.starts_with("extra") && b.get(name).is_none() {
                let p = PathBuf::from(orig);
                // The drive of the folder (`D:\` of `D:\Games\x`).
                let drive: PathBuf = p.components().take(2).collect();
                let drive_ok = p.has_root() && drive.exists();
                b.set(name, if drive_ok { p } else { fallback.join(name) });
            }
        }
    }
    b
}

/// Where each chunk's bytes go: (file index, offset in file, offset in chunk, length).
fn spans(c: &Component) -> Vec<Vec<(usize, u64, usize, usize)>> {
    let offsets = c.file_offsets();
    let mut out = Vec::with_capacity(c.chunks.len());
    let mut start = 0u64;
    for ch in &c.chunks {
        let end = start + ch.s;
        let mut v = Vec::new();
        let mut i = offsets.partition_point(|&o| o <= start).saturating_sub(1);
        while i < c.files.len() && offsets[i] < end {
            let (fo, fs) = (offsets[i], c.files[i].s);
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

/// BLAKE3 of a file, as in manifests.
pub fn hash_file(p: &Path) -> Result<String> {
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

struct Part<'a> {
    c: &'a Component,
    paths: Vec<PathBuf>,
    spans: Vec<Vec<(usize, u64, usize, usize)>>,
    /// Files to write (the others are already right).
    needed: Vec<bool>,
}

struct Run<'a> {
    store: &'a dyn Store,
    key: &'a str,
    version: String,
    targets: Targets,
    parts: Vec<Part<'a>>,
    threads: usize,
    cancel: &'a AtomicBool,
    progress: &'a (dyn Fn(&Progress) + Sync),
    started: Instant,
    done: Mutex<HashSet<(usize, usize)>>,
    last_flush: Mutex<Instant>,
}

impl Run<'_> {
    fn report(&self, phase: &'static str, done: u64, total: u64) {
        (self.progress)(&Progress { phase, done, total, elapsed_secs: self.started.elapsed().as_secs_f64() });
    }

    fn flush(&self) -> Result<()> {
        let mut done: Vec<(usize, usize)> = self.done.lock().unwrap().iter().copied().collect();
        done.sort_unstable();
        let j = Journal { version: self.version.clone(), targets: Some(self.targets.clone()), done };
        util::atomic_write(&journal_file(self.key), &serde_json::to_vec(&j)?)
    }

    fn write_chunk(&self, pi: usize, ci: usize, raw: &[u8]) -> Result<()> {
        let part = &self.parts[pi];
        for &(fi, file_off, chunk_off, len) in &part.spans[ci] {
            if !part.needed[fi] {
                continue;
            }
            let p = &part.paths[fi];
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

    /// Download and write every (component, chunk) in `todo`, grouped by
    /// chunk id so a chunk used twice is downloaded once.
    fn download(&self, todo: &[(usize, usize)]) -> Result<()> {
        let mut groups: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
        for &(pi, ci) in todo {
            groups.entry(self.parts[pi].c.chunks[ci].h.as_str()).or_default().push((pi, ci));
        }
        let groups: Vec<(&str, Vec<(usize, usize)>)> = groups.into_iter().collect();
        let total: u64 = todo.iter().map(|&(pi, ci)| self.parts[pi].c.chunks[ci].s).sum();
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
                    let Some((id, uses)) = groups.get(next.fetch_add(1, Ordering::SeqCst)) else { return };
                    let size = self.parts[uses[0].0].c.chunks[uses[0].1].s;
                    let result = retry(self.cancel, || {
                        self.store.read(&remote::chunk_path(id))?.ok_or_else(|| {
                            anyhow!(crate::tr!(
                                "云端缺少数据块 {id}，这个版本可能已被删除或损坏",
                                "Chunk {id} is missing in the cloud; this version may have been deleted or damaged"
                            ))
                        })
                    })
                    .and_then(|obj| chunk::decode(&obj, id, size))
                    .and_then(|raw| {
                        for &(pi, ci) in uses {
                            self.write_chunk(pi, ci, &raw)?;
                        }
                        Ok(())
                    });
                    match result {
                        Ok(()) => {
                            self.done.lock().unwrap().extend(uses.iter().copied());
                            let add = size * uses.len() as u64;
                            let n = bytes.fetch_add(add, Ordering::SeqCst) + add;
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

    /// (component, file) pairs among `which` whose content is not the
    /// manifest's. Missing files count as wrong.
    fn check(&self, phase: &'static str, which: &[(usize, usize)]) -> Result<Vec<(usize, usize)>> {
        let total: u64 = which.iter().map(|&(pi, fi)| self.parts[pi].c.files[fi].s).sum();
        let bytes = AtomicU64::new(0);
        let next = AtomicUsize::new(0);
        let bad = Mutex::new(Vec::new());
        self.report(phase, 0, total);
        std::thread::scope(|s| {
            for _ in 0..self.threads {
                s.spawn(|| loop {
                    if self.cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(&(pi, fi)) = which.get(next.fetch_add(1, Ordering::SeqCst)) else { return };
                    let f = &self.parts[pi].c.files[fi];
                    let ok = hash_file(&self.parts[pi].paths[fi]).is_ok_and(|h| h == f.h);
                    if !ok {
                        bad.lock().unwrap().push((pi, fi));
                    }
                    let n = bytes.fetch_add(f.s, Ordering::SeqCst) + f.s;
                    self.report(phase, n, total);
                });
            }
        });
        if self.cancel.load(Ordering::Relaxed) {
            return Err(anyhow!(Cancelled));
        }
        let mut bad = bad.into_inner().unwrap();
        bad.sort_unstable();
        Ok(bad)
    }
}

/// Restore `summary` of game `key` to `targets`.
pub fn restore(
    store: &dyn Store,
    key: &str,
    summary: &Summary,
    targets: &Targets,
    threads: usize,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
) -> Result<(Manifest, Report)> {
    let started = Instant::now();
    (progress)(&Progress { phase: "preparing", done: 0, total: 0, elapsed_secs: 0.0 });
    if targets.program.as_os_str().is_empty() || targets.content.as_ref().is_some_and(|c| c.as_os_str().is_empty()) {
        bail!(crate::tr!("没有选择恢复到哪个文件夹", "No folder was chosen to restore to"));
    }
    let m = remote::read_manifest(store, key, summary)?;
    let resuming = pending(key).is_some_and(|(v, t)| v == m.id && &t == targets);
    let bases = bases_for(&m, targets);

    let mut parts = Vec::new();
    for c in &m.components {
        let wanted = match c.kind {
            Kind::Program | Kind::Folder => targets.restore_program,
            Kind::Content => targets.content.is_some(),
            Kind::Config | Kind::Firmware => true,
        };
        if !wanted {
            continue;
        }
        let paths = c.files.iter().map(|f| bases.resolve(&f.p)).collect::<Result<Vec<_>>>()?;
        parts.push(Part { c, spans: spans(c), needed: vec![true; paths.len()], paths });
    }

    let done: HashSet<(usize, usize)> = if resuming {
        std::fs::read(journal_file(key))
            .ok()
            .and_then(|b| serde_json::from_slice::<Journal>(&b).ok())
            .map(|j| j.done.into_iter().filter(|&(pi, ci)| pi < parts.len() && ci < parts[pi].c.chunks.len()).collect())
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let resumed_chunks = done.len();
    let mut run = Run {
        store,
        key,
        version: m.id.clone(),
        targets: targets.clone(),
        parts,
        threads: threads.clamp(1, 16),
        cancel,
        progress,
        started,
        done: Mutex::new(done),
        last_flush: Mutex::new(Instant::now()),
    };

    // Files already there with the right content stay as they are.
    let mut kept = 0;
    if !resuming {
        let existing: Vec<(usize, usize)> = run
            .parts
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                p.paths
                    .iter()
                    .enumerate()
                    .filter(|(fi, path)| std::fs::metadata(path).is_ok_and(|md| md.is_file() && md.len() == p.c.files[*fi].s))
                    .map(move |(fi, _)| (pi, fi))
            })
            .collect();
        let wrong: HashSet<(usize, usize)> = run.check("checking", &existing)?.into_iter().collect();
        for (pi, fi) in existing {
            if !wrong.contains(&(pi, fi)) {
                run.parts[pi].needed[fi] = false;
                kept += 1;
            }
        }
        // Enough space for what will be written, per drive.
        let mut need: HashMap<String, (PathBuf, u64)> = HashMap::new();
        for p in &run.parts {
            for (fi, path) in p.paths.iter().enumerate().filter(|(fi, _)| p.needed[*fi]) {
                let drive = path.components().next().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).unwrap_or_default();
                let e = need.entry(drive).or_insert((path.clone(), 0));
                e.1 += p.c.files[fi].s;
            }
        }
        for (path, bytes) in need.values() {
            if let Some(free) = free_space(path) {
                if free < bytes + 64 * 1024 * 1024 {
                    bail!(crate::tr!(
                        "{} 所在的磁盘空间不够：需要 {} MB，只剩 {} MB",
                        "Not enough space on the disk of {}: {} MB needed, {} MB free",
                        path.display(),
                        bytes / 1_048_576 + 64,
                        free / 1_048_576
                    ));
                }
            }
        }
    }

    // Folders and files to write.
    for c in m.components.iter().filter(|c| run.parts.iter().any(|p| std::ptr::eq(p.c, *c))) {
        for d in &c.dirs {
            std::fs::create_dir_all(bases.resolve(d)?)?;
        }
    }
    for p in &run.parts {
        for (fi, path) in p.paths.iter().enumerate().filter(|(fi, _)| p.needed[*fi]) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| crate::tr!("无法创建文件夹 {}", "Can't create the folder {}", parent.display()))?;
            }
            let keep = resuming && std::fs::metadata(path).is_ok_and(|md| md.is_file() && md.len() == p.c.files[fi].s);
            if !keep {
                let file = std::fs::File::create(path).with_context(|| crate::tr!("无法创建 {}", "Can't create {}", path.display()))?;
                file.set_len(p.c.files[fi].s)?;
            }
        }
    }
    run.flush()?;

    // Chunks that touch a file to write and aren't written yet.
    let todo: Vec<(usize, usize)> = {
        let done = run.done.lock().unwrap();
        run.parts
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                (0..p.c.chunks.len()).filter(move |&ci| p.spans[ci].iter().any(|s| p.needed[s.0])).map(move |ci| (pi, ci))
            })
            .filter(|x| !done.contains(x))
            .collect()
    };
    run.download(&todo)?;

    let written: Vec<(usize, usize)> =
        run.parts.iter().enumerate().flat_map(|(pi, p)| (0..p.paths.len()).filter(move |&fi| p.needed[fi]).map(move |fi| (pi, fi))).collect();
    let bad = run.check("verifying", &written)?;
    let repaired_files = bad.len();
    if !bad.is_empty() {
        // Download the chunks of the damaged files again, then check them once more.
        let bad_set: HashSet<(usize, usize)> = bad.iter().copied().collect();
        let redo: Vec<(usize, usize)> = run
            .parts
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| {
                let bad_set = &bad_set;
                (0..p.c.chunks.len()).filter(move |&ci| p.spans[ci].iter().any(|s| bad_set.contains(&(pi, s.0)))).map(move |ci| (pi, ci))
            })
            .collect();
        {
            let mut done = run.done.lock().unwrap();
            for x in &redo {
                done.remove(x);
            }
        }
        run.download(&redo)?;
        let still = run.check("verifying", &bad)?;
        if let Some(&(pi, fi)) = still.first() {
            bail!(crate::tr!(
                "恢复后的文件校验不通过：{}（共 {} 个）。请检查磁盘后重试",
                "Restored files failed the check: {} ({} in all). Check the disk and try again",
                run.parts[pi].paths[fi].display(),
                still.len()
            ));
        }
    }

    run.report("finishing", 0, 0);
    let mut bytes = 0;
    for &(pi, fi) in &written {
        let f = &run.parts[pi].c.files[fi];
        bytes += f.s;
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&run.parts[pi].paths[fi]) {
            let _ = file.set_modified(std::time::UNIX_EPOCH + Duration::new(f.m.max(0) as u64, f.n.min(999_999_999)));
        }
    }
    let _ = std::fs::remove_file(journal_file(key));
    let report = Report { files: written.len(), kept, bytes, resumed_chunks, repaired_files };
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
        assert_eq!(resolve_target(&chosen, "D:\\Old\\Zelda"), chosen);
        std::fs::create_dir_all(chosen.join("Other")).unwrap();
        assert_eq!(resolve_target(&chosen, "D:\\Old\\Zelda\\"), chosen.join("Zelda"));
        let named = tmp.path().join("Zelda");
        std::fs::create_dir_all(named.join("x")).unwrap();
        assert_eq!(resolve_target(&named, "D:\\Old\\Zelda"), named);
    }
}
