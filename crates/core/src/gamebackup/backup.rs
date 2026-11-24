//! Back up a game folder: read its files in a fixed order as one stream, cut
//! it into chunks, upload the chunks the cloud doesn't have yet (in
//! parallel), then commit a manifest. An interrupted backup resumes by
//! running it again: chunks already uploaded are skipped.

use super::chunker::{Chunker, Params};
use super::format::{ChunkRef, FileEntry, Manifest, Summary, FORMAT};
use super::scan::{self, ScannedFile};
use super::{chunk, remote, retry, Cancelled};
use crate::cloud::store::Store;
use crate::config::{DeviceConfig, GameEntry};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BackupOptions {
    pub root: PathBuf,
    /// The user's own exclude patterns (defaults and saves are added).
    pub user_exclude: Vec<String>,
    pub threads: usize,
    pub params: Params,
    /// Versions to keep afterwards; `None` keeps all.
    pub keep: Option<usize>,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// scanning | listing | uploading | verifying | committing | cleaning
    pub phase: &'static str,
    /// Bytes read (uploading), or items done in the other phases.
    pub done: u64,
    pub total: u64,
    pub uploaded: u64,
    pub new_chunks: usize,
    pub reused_chunks: usize,
    pub elapsed_secs: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    /// Nothing changed since the newest version.
    Unchanged { version: Summary },
    Created {
        version: Summary,
        uploaded: u64,
        new_chunks: usize,
        reused_chunks: usize,
        /// Old versions deleted afterwards (keep setting).
        pruned: Vec<String>,
        freed: u64,
    },
}

/// Reads the scanned files one after another, hashing each one and checking
/// it doesn't change while being read.
struct StreamReader<'a> {
    files: &'a [ScannedFile],
    idx: usize,
    cur: Option<(std::io::Take<std::fs::File>, u64)>,
    hasher: blake3::Hasher,
    hashes: Vec<String>,
    read: &'a AtomicU64,
}

impl<'a> StreamReader<'a> {
    fn new(files: &'a [ScannedFile], read: &'a AtomicU64) -> Self {
        StreamReader { files, idx: 0, cur: None, hasher: blake3::Hasher::new(), hashes: Vec::with_capacity(files.len()), read }
    }

    fn changed(f: &ScannedFile) -> std::io::Error {
        std::io::Error::other(crate::tr!(
            "读取时文件变了：{}。请关掉游戏后再备份",
            "A file changed while it was read: {}. Close the game, then back up again",
            f.rel
        ))
    }
}

impl Read for StreamReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let Some(f) = self.files.get(self.idx) else { return Ok(0) };
            if self.cur.is_none() {
                let file = std::fs::File::open(&f.abs).map_err(|e| {
                    std::io::Error::new(e.kind(), crate::tr!("无法读取 {}：{e}", "Can't read {}: {e}", f.abs.display()))
                })?;
                self.cur = Some((file.take(f.size), 0));
            }
            let (file, done) = self.cur.as_mut().expect("open");
            let n = file.read(buf)?;
            if n > 0 {
                self.hasher.update(&buf[..n]);
                *done += n as u64;
                self.read.fetch_add(n as u64, Ordering::Relaxed);
                return Ok(n);
            }
            let now = file.get_ref().metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            if *done != f.size || now != f.size {
                return Err(Self::changed(f));
            }
            self.hashes.push(std::mem::take(&mut self.hasher).finalize().to_hex().to_string());
            self.cur = None;
            self.idx += 1;
        }
    }
}

fn rel_inside(root: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(root).ok()?;
    let s = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
    (!s.is_empty()).then_some(s)
}

/// Back up `opts.root` for `game` to `store`.
pub fn backup(
    store: &dyn Store,
    game: &GameEntry,
    device: &DeviceConfig,
    opts: &BackupOptions,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
) -> Result<Outcome> {
    let started = Instant::now();
    let key = game.key.as_str();
    let report = |phase: &'static str, done: u64, total: u64, uploaded: u64, new: usize, reused: usize| {
        progress(&Progress {
            phase,
            done,
            total,
            uploaded,
            new_chunks: new,
            reused_chunks: reused,
            elapsed_secs: started.elapsed().as_secs_f64(),
        })
    };
    let stop = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            Err(anyhow!(Cancelled))
        } else {
            Ok(())
        }
    };

    report("scanning", 0, 0, 0, 0, 0);
    store.check()?;
    let exclude = scan::excludes_for(game, &opts.root, &opts.user_exclude);
    let sc = scan::scan(&opts.root, &exclude)?;
    if sc.files.is_empty() {
        bail!(crate::tr!("文件夹里没有可以备份的文件：{}", "There are no files to back up in {}", opts.root.display()));
    }
    stop()?;

    let versions = remote::list_versions(store, key)?;
    if let Some(last) = versions.last() {
        if let Ok(m) = remote::read_manifest(store, key, last) {
            let same = m.chunking == opts.params
                && m.exclude == exclude
                && m.dirs == sc.dirs
                && m.files.len() == sc.files.len()
                && m.files.iter().zip(&sc.files).all(|(a, b)| a.p == b.rel && a.s == b.size && a.m == b.mtime);
            if same {
                return Ok(Outcome::Unchanged { version: last.clone() });
            }
        }
    }

    let lock = remote::Lock::acquire(store, key, &device.id, &device.name, "backup")?;
    report("listing", 0, 0, 0, 0, 0);
    let existing = remote::list_chunks(store, key)?;
    stop()?;

    let threads = opts.threads.clamp(1, 16);
    let total = sc.total;
    let read = AtomicU64::new(0);
    let uploaded = AtomicU64::new(0);
    let new_chunks = AtomicUsize::new(0);
    let reused = AtomicUsize::new(0);
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let done = AtomicBool::new(false);
    let (tx, rx) = mpsc::sync_channel::<(String, Vec<u8>)>(threads * 2);
    let rx = Mutex::new(rx);
    let is_stopping = || cancel.load(Ordering::Relaxed) || failed.lock().unwrap().is_some();

    let mut refs: Vec<ChunkRef> = Vec::new();
    let mut reader = StreamReader::new(&sc.files, &read);
    let read_result: Result<()> = std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let item = rx.lock().unwrap().recv();
                let Ok((id, raw)) = item else { return };
                // Keep draining after a failure so the reader never blocks.
                if is_stopping() {
                    continue;
                }
                let result = chunk::encode(&raw).and_then(|obj| {
                    retry(cancel, || store.write(&remote::chunk_path(key, &id), &obj))?;
                    Ok(obj.len() as u64)
                });
                match result {
                    Ok(n) => {
                        let up = uploaded.fetch_add(n, Ordering::SeqCst) + n;
                        let new = new_chunks.fetch_add(1, Ordering::SeqCst) + 1;
                        report("uploading", read.load(Ordering::Relaxed), total, up, new, reused.load(Ordering::Relaxed));
                    }
                    Err(e) => {
                        failed.lock().unwrap().get_or_insert(e);
                    }
                }
            });
        }
        // Keep the lock alive during long uploads.
        s.spawn(|| {
            let mut last = Instant::now();
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                if last.elapsed() >= Duration::from_secs(300) {
                    last = Instant::now();
                    let _ = lock.refresh();
                }
            }
        });

        let result = (|| -> Result<()> {
            let mut seen = HashSet::new();
            let mut chunker = Chunker::new(&mut reader, opts.params);
            while let Some(data) = chunker.next_chunk()? {
                if is_stopping() {
                    return Ok(());
                }
                let id = chunk::id(&data);
                refs.push(ChunkRef { h: id.clone(), s: data.len() as u64, z: 0 });
                if seen.insert(id.clone()) && !existing.contains_key(&id) {
                    if tx.send((id, data)).is_err() {
                        return Ok(());
                    }
                } else {
                    let r = reused.fetch_add(1, Ordering::SeqCst) + 1;
                    report(
                        "uploading",
                        read.load(Ordering::Relaxed),
                        total,
                        uploaded.load(Ordering::Relaxed),
                        new_chunks.load(Ordering::Relaxed),
                        r,
                    );
                }
            }
            Ok(())
        })();
        // Workers end once the channel is empty; the refresher ends now (what
        // is still queued uploads in seconds).
        drop(tx);
        done.store(true, Ordering::Relaxed);
        result
    });
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e);
    }
    stop()?;
    read_result?;
    let hashes = std::mem::take(&mut reader.hashes);
    if hashes.len() != sc.files.len() {
        bail!(crate::tr!("读取文件时出错，请重试", "Reading the files went wrong. Try again"));
    }

    report("verifying", 0, refs.len() as u64, uploaded.load(Ordering::SeqCst), new_chunks.load(Ordering::SeqCst), reused.load(Ordering::SeqCst));
    let present = remote::list_chunks(store, key)?;
    let mut stored_size = 0;
    let mut counted = HashSet::new();
    for r in &mut refs {
        let Some(&z) = present.get(&r.h).filter(|z| **z > 0) else {
            bail!(crate::tr!(
                "上传后云端少了数据块 {}，请重新备份（已传的部分不会重传）",
                "Chunk {} is missing in the cloud after uploading. Back up again (what was uploaded is kept)",
                r.h
            ));
        };
        r.z = z;
        if counted.insert(r.h.clone()) {
            stored_size += z;
        }
    }
    stop()?;

    report("committing", 0, 0, uploaded.load(Ordering::SeqCst), new_chunks.load(Ordering::SeqCst), reused.load(Ordering::SeqCst));
    let manifest = Manifest {
        format: FORMAT,
        id: ulid::Ulid::new().to_string(),
        game: key.to_string(),
        name: game.name.clone(),
        created_at: chrono::Utc::now(),
        device_id: device.id.clone(),
        device_name: device.name.clone(),
        app_version: crate::APP_VERSION.to_string(),
        root: opts.root.display().to_string(),
        exe: game.exe.as_deref().and_then(|e| rel_inside(&opts.root, e)),
        work_dir: game.work_dir.as_deref().and_then(|w| rel_inside(&opts.root, w)),
        chunking: opts.params,
        exclude,
        user_exclude: opts.user_exclude.clone(),
        files: sc
            .files
            .iter()
            .zip(hashes)
            .map(|(f, h)| FileEntry { p: f.rel.clone(), s: f.size, h, m: f.mtime })
            .collect(),
        dirs: sc.dirs.clone(),
        chunks: refs,
        total_size: sc.total,
        stored_size,
        new_bytes: uploaded.load(Ordering::SeqCst),
        comment: opts.comment.clone(),
    };
    let version = remote::commit_version(store, &manifest)?;

    let (mut pruned, mut freed) = (Vec::new(), 0);
    if let Some(keep) = opts.keep {
        report("cleaning", 0, 0, manifest.new_bytes, new_chunks.load(Ordering::SeqCst), reused.load(Ordering::SeqCst));
        let (deleted, gc) = remote::prune(store, key, keep, &|d, t| {
            report("cleaning", d as u64, t as u64, manifest.new_bytes, new_chunks.load(Ordering::SeqCst), reused.load(Ordering::SeqCst))
        })?;
        pruned = deleted;
        freed = gc.freed_bytes;
    }
    lock.release()?;
    Ok(Outcome::Created {
        version,
        uploaded: manifest.new_bytes,
        new_chunks: new_chunks.load(Ordering::SeqCst),
        reused_chunks: reused.load(Ordering::SeqCst),
        pruned,
        freed,
    })
}

/// What a backup of `root` would contain (the app shows it before starting).
pub fn preview(game: &GameEntry, root: &Path, user_exclude: &[String]) -> Result<scan::ScanSummary> {
    let exclude = scan::excludes_for(game, root, user_exclude);
    Ok(scan::scan(root, &exclude)?.summary(root))
}
