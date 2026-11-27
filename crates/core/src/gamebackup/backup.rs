//! Back up a game: split it into components (see [`super::components`]),
//! read each component's files in a fixed order, cut them into chunks,
//! upload the chunks the cloud doesn't have yet (in parallel), then commit a
//! manifest. Small files are streamed together; files of 8 MB or more are
//! chunked on their own, so their chunks don't depend on their neighbours
//! and an unchanged one is taken from this PC's cache without reading it.
//! An interrupted backup resumes by running it again.

use super::cache::FileCache;
use super::chunker::{Chunker, Params};
use super::components::{self, Planned, PlannedFile};
use super::format::{ChunkRef, Component, FileEntry, Manifest, Summary, FORMAT};
use super::{chunk, remote, retry, Cancelled};
use crate::cloud::store::Store;
use crate::config::{DeviceConfig, GameEntry};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Files this big or bigger are chunked on their own.
pub const BIG_FILE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    pub components: components::Options,
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

/// Reads files one after another, hashing each one and checking it doesn't
/// change while being read.
struct StreamReader<'a> {
    files: &'a [PlannedFile],
    idx: usize,
    cur: Option<(std::io::Take<std::fs::File>, u64)>,
    hasher: blake3::Hasher,
    hashes: Vec<String>,
    read: &'a AtomicU64,
}

impl<'a> StreamReader<'a> {
    fn new(files: &'a [PlannedFile], read: &'a AtomicU64) -> Self {
        StreamReader { files, idx: 0, cur: None, hasher: blake3::Hasher::new(), hashes: Vec::with_capacity(files.len()), read }
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
                return Err(std::io::Error::other(crate::tr!(
                    "读取时文件变了：{}。请关掉游戏后再备份",
                    "A file changed while it was read: {}. Close the game, then back up again",
                    f.abs.display()
                )));
            }
            self.hashes.push(std::mem::take(&mut self.hasher).finalize().to_hex().to_string());
            self.cur = None;
            self.idx += 1;
        }
    }
}

/// Same files (and folders) as a component of the previous version.
fn unchanged(p: &Planned, prev: &Component) -> bool {
    p.kind == prev.kind
        && p.dirs == prev.dirs
        && p.files.len() == prev.files.len()
        && p.files.iter().zip(&prev.files).all(|(a, b)| a.logical == b.p && a.size == b.s && a.mtime == b.m && a.nanos == b.n)
}

/// Chunks on their way to the upload workers: (id, raw bytes).
type Sender = mpsc::SyncSender<(String, Vec<u8>)>;

/// What's in the cloud and this run, shared by the reader and the workers.
struct Upload<'a> {
    store: &'a dyn Store,
    lock: &'a remote::Lock<'a>,
    existing: HashMap<String, u64>,
    seen: Mutex<HashSet<String>>,
    tx: Mutex<Option<Sender>>,
    /// Raw bytes handed to the workers and not compressed yet, and the most
    /// allowed: chunks are up to 24 MiB, so a queue counted in chunks alone
    /// could hold hundreds of MiB.
    queued: Mutex<u64>,
    freed: Condvar,
    budget: u64,
    /// The chunker's buffer (48 MiB by default), kept for the next file.
    buffer: Mutex<Vec<u8>>,
    read: AtomicU64,
    uploaded: AtomicU64,
    new_chunks: AtomicUsize,
    reused: AtomicUsize,
    failed: Mutex<Option<anyhow::Error>>,
    cancel: &'a AtomicBool,
}

impl Upload<'_> {
    fn stopping(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.failed.lock().unwrap().is_some() || self.lock.is_lost()
    }

    /// Whether the cloud has this chunk or this run is uploading it.
    fn known(&self, id: &str) -> bool {
        self.existing.contains_key(id) || self.seen.lock().unwrap().contains(id)
    }

    /// Queue a chunk for upload unless the cloud has it. Waits while the
    /// workers have enough raw bytes to go on with (one chunk always fits).
    fn offer(&self, id: String, data: Vec<u8>) -> bool {
        let new = !self.existing.contains_key(&id) && self.seen.lock().unwrap().insert(id.clone());
        if !new {
            self.reused.fetch_add(1, Ordering::SeqCst);
            return true;
        }
        let len = data.len() as u64;
        let mut queued = self.queued.lock().unwrap();
        while *queued > 0 && *queued + len > self.budget {
            if self.stopping() {
                return false;
            }
            queued = self.freed.wait_timeout(queued, Duration::from_millis(100)).unwrap().0;
        }
        *queued += len;
        drop(queued);
        let tx = self.tx.lock().unwrap().clone();
        match tx {
            Some(tx) => tx.send((id, data)).is_ok(),
            None => false,
        }
    }

    /// A worker is done with a chunk's raw bytes.
    fn release(&self, len: usize) {
        *self.queued.lock().unwrap() -= len as u64;
        self.freed.notify_all();
    }

    /// Cut `files` (consecutive small files, or one big file) into chunks.
    fn chunk_files(&self, files: &[PlannedFile], params: Params, chunks: &mut Vec<ChunkRef>) -> Result<Vec<String>> {
        let mut reader = StreamReader::new(files, &self.read);
        {
            let buffer = std::mem::take(&mut *self.buffer.lock().unwrap());
            let mut chunker = Chunker::with_buffer(&mut reader, params, buffer);
            while let Some(data) = chunker.next_chunk()? {
                if self.stopping() {
                    return Err(anyhow!(Cancelled));
                }
                let id = chunk::id(&data);
                chunks.push(ChunkRef { h: id.clone(), s: data.len() as u64, z: 0 });
                if !self.offer(id, data) {
                    return Err(anyhow!(Cancelled));
                }
            }
            *self.buffer.lock().unwrap() = chunker.into_buffer();
        }
        Ok(std::mem::take(&mut reader.hashes))
    }
}

/// Back up `game` to `store`.
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
    let stop = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            Err(anyhow!(Cancelled))
        } else {
            Ok(())
        }
    };
    let simple = |phase: &'static str, done: u64, total: u64| {
        progress(&Progress { phase, done, total, uploaded: 0, new_chunks: 0, reused_chunks: 0, elapsed_secs: started.elapsed().as_secs_f64() })
    };

    simple("scanning", 0, 0);
    store.check()?;
    let planned = components::plan(game, &opts.components)?;
    if planned.iter().all(|c| c.files.is_empty()) {
        bail!(crate::tr!("没有可以备份的文件", "There are no files to back up"));
    }
    stop()?;

    let versions = remote::list_versions(store, key)?;
    let previous: Option<(Summary, Manifest)> = versions
        .last()
        .and_then(|s| remote::read_manifest(store, key, s).ok().map(|m| (s.clone(), m)))
        .filter(|(_, m)| m.chunking == opts.params);
    // Each component: the previous version's when its files are the same.
    let reuse: Vec<Option<Component>> = planned
        .iter()
        .map(|p| previous.as_ref().and_then(|(_, m)| m.component(p.kind)).filter(|c| unchanged(p, c)).cloned())
        .collect();
    if let Some((s, m)) = &previous {
        if reuse.iter().all(Option::is_some) && m.components.len() == planned.len() {
            return Ok(Outcome::Unchanged { version: s.clone() });
        }
    }

    let lock = remote::Lock::acquire(store, key, &device.id, &device.name, "backup")?;
    // The lock is kept alive until the version is committed and old ones are
    // cleaned up, not only while files are read: a cleanup elsewhere must
    // never find it expired while chunks still wait to be committed.
    let out = lock.while_held(|| upload_and_commit(store, game, device, opts, cancel, progress, started, &lock, planned, reuse, previous))?;
    lock.release()?;
    Ok(out)
}

/// The part of [`backup`] that holds the lock: upload the new chunks, check
/// they are all there, commit, clean up old versions.
#[allow(clippy::too_many_arguments)]
fn upload_and_commit(
    store: &dyn Store,
    game: &GameEntry,
    device: &DeviceConfig,
    opts: &BackupOptions,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&Progress) + Sync),
    started: Instant,
    lock: &remote::Lock,
    planned: Vec<Planned>,
    reuse: Vec<Option<Component>>,
    previous: Option<(Summary, Manifest)>,
) -> Result<Outcome> {
    let key = game.key.as_str();
    let stop = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            Err(anyhow!(Cancelled))
        } else {
            Ok(())
        }
    };
    progress(&Progress { phase: "listing", done: 0, total: 0, uploaded: 0, new_chunks: 0, reused_chunks: 0, elapsed_secs: started.elapsed().as_secs_f64() });
    let existing = remote::list_chunks(store)?;
    stop()?;

    let threads = opts.threads.clamp(1, 16);
    let total: u64 = planned.iter().zip(&reuse).filter(|(_, r)| r.is_none()).map(|(p, _)| p.total()).sum();
    let (tx, rx) = mpsc::sync_channel::<(String, Vec<u8>)>(threads * 2);
    let rx = Mutex::new(rx);
    let up = Upload {
        store,
        lock,
        existing,
        seen: Mutex::new(HashSet::new()),
        tx: Mutex::new(Some(tx)),
        queued: Mutex::new(0),
        freed: Condvar::new(),
        // Two average chunks per worker keep them busy.
        budget: (threads * 2 * opts.params.avg) as u64,
        buffer: Mutex::new(Vec::new()),
        read: AtomicU64::new(0),
        uploaded: AtomicU64::new(0),
        new_chunks: AtomicUsize::new(0),
        reused: AtomicUsize::new(0),
        failed: Mutex::new(None),
        cancel,
    };
    let report = |phase: &'static str, done: u64, total: u64| {
        progress(&Progress {
            phase,
            done,
            total,
            uploaded: up.uploaded.load(Ordering::Relaxed),
            new_chunks: up.new_chunks.load(Ordering::Relaxed),
            reused_chunks: up.reused.load(Ordering::Relaxed),
            elapsed_secs: started.elapsed().as_secs_f64(),
        })
    };
    let mut cache = FileCache::load();
    let mut built: Vec<Option<(Vec<ChunkRef>, Vec<String>)>> = Vec::new();

    let read_result: Result<()> = std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let item = rx.lock().unwrap().recv();
                let Ok((id, raw)) = item else { return };
                let len = raw.len();
                // Keep draining after a failure so the reader never blocks.
                if up.stopping() {
                    drop(raw);
                    up.release(len);
                    continue;
                }
                // The raw bytes go as soon as they are compressed, not after the upload.
                let encoded = chunk::encode(&raw);
                drop(raw);
                up.release(len);
                let result = encoded.and_then(|obj| {
                    retry(cancel, || up.store.write(&remote::chunk_path(&id), &obj))?;
                    Ok(obj.len() as u64)
                });
                match result {
                    Ok(n) => {
                        up.uploaded.fetch_add(n, Ordering::SeqCst);
                        up.new_chunks.fetch_add(1, Ordering::SeqCst);
                        report("uploading", up.read.load(Ordering::Relaxed), total);
                    }
                    Err(e) => {
                        up.failed.lock().unwrap().get_or_insert(e);
                    }
                }
            });
        }

        let result = (|| -> Result<()> {
            for (p, r) in planned.iter().zip(&reuse) {
                if r.is_some() {
                    built.push(None);
                    continue;
                }
                let mut chunks = Vec::new();
                let mut hashes = Vec::with_capacity(p.files.len());
                let mut i = 0;
                while i < p.files.len() {
                    let f = &p.files[i];
                    if f.size >= BIG_FILE {
                        let cached = cache
                            .get(&f.abs, f.size, f.mtime, f.nanos, &opts.params)
                            .filter(|e| e.chunks.iter().all(|(id, _)| up.known(id)))
                            .cloned();
                        match cached {
                            Some(e) => {
                                up.reused.fetch_add(e.chunks.len(), Ordering::SeqCst);
                                up.read.fetch_add(f.size, Ordering::Relaxed);
                                chunks.extend(e.chunks.into_iter().map(|(h, s)| ChunkRef { h, s, z: 0 }));
                                hashes.push(e.hash);
                            }
                            None => {
                                let from = chunks.len();
                                let h = up.chunk_files(std::slice::from_ref(f), opts.params, &mut chunks)?;
                                cache.put(&f.abs, f.size, f.mtime, f.nanos, &opts.params, h[0].clone(), &chunks[from..]);
                                hashes.extend(h);
                            }
                        }
                        report("uploading", up.read.load(Ordering::Relaxed), total);
                        i += 1;
                        continue;
                    }
                    let j = p.files[i..].iter().position(|f| f.size >= BIG_FILE).map_or(p.files.len(), |k| i + k);
                    hashes.extend(up.chunk_files(&p.files[i..j], opts.params, &mut chunks)?);
                    report("uploading", up.read.load(Ordering::Relaxed), total);
                    i = j;
                }
                built.push(Some((chunks, hashes)));
            }
            Ok(())
        })();
        // Workers end once the channel is empty.
        up.tx.lock().unwrap().take();
        result
    });
    cache.save();
    if let Some(e) = up.failed.lock().unwrap().take() {
        return Err(e);
    }
    lock.held()?;
    stop()?;
    read_result?;

    // Touched but not changed: same content as the newest version.
    if let Some((s, m)) = &previous {
        let same = m.components.len() == planned.len()
            && planned.iter().zip(&reuse).zip(&built).all(|((p, r), b)| {
                r.is_some()
                    || m.component(p.kind).is_some_and(|c| {
                        let hashes = &b.as_ref().expect("built").1;
                        c.dirs == p.dirs
                            && c.files.len() == p.files.len()
                            && c.files.iter().zip(&p.files).zip(hashes).all(|((a, f), h)| a.p == f.logical && a.s == f.size && &a.h == h)
                    })
            });
        if same {
            return Ok(Outcome::Unchanged { version: s.clone() });
        }
    }

    report("verifying", 0, 0);
    let present = remote::list_chunks(store)?;
    let mut comps = Vec::new();
    let mut counted = HashSet::new();
    let mut stored_size = 0;
    for ((p, r), b) in planned.iter().zip(reuse).zip(built) {
        let mut c = match (r, b) {
            (Some(mut prev), _) => {
                prev.name = p.name.clone();
                prev.version = p.version.clone().or(prev.version);
                prev.build = p.build.clone();
                prev
            }
            (None, Some((chunks, hashes))) => {
                if hashes.len() != p.files.len() {
                    bail!(crate::tr!("读取文件时出错，请重试", "Reading the files went wrong. Try again"));
                }
                Component {
                    kind: p.kind,
                    name: p.name.clone(),
                    version: p.version.clone(),
                    bases: p.bases.clone(),
                    exe: p.exe.clone(),
                    source: p.source.clone(),
                    build: p.build.clone(),
                    files: p
                        .files
                        .iter()
                        .zip(hashes)
                        .map(|(f, h)| FileEntry { p: f.logical.clone(), s: f.size, h, m: f.mtime, n: f.nanos })
                        .collect(),
                    dirs: p.dirs.clone(),
                    chunks,
                    total_size: p.total(),
                }
            }
            (None, None) => unreachable!("every component is reused or built"),
        };
        for r in &mut c.chunks {
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
        comps.push(c);
    }
    stop()?;

    report("committing", 0, 0);
    let manifest = Manifest {
        format: FORMAT,
        id: ulid::Ulid::new().to_string(),
        game: key.to_string(),
        name: game.name.clone(),
        created_at: chrono::Utc::now(),
        device_id: device.id.clone(),
        device_name: device.name.clone(),
        app_version: crate::APP_VERSION.to_string(),
        adapter: game.adapter.clone(),
        args: game.args.clone(),
        chunking: opts.params,
        user_exclude: opts.components.user_exclude.clone(),
        total_size: comps.iter().map(|c| c.total_size).sum(),
        components: comps,
        stored_size,
        new_bytes: up.uploaded.load(Ordering::SeqCst),
        comment: opts.comment.clone(),
    };
    // A device that slept past the lock's expiry may have missed a cleanup
    // that deleted chunks this version needs: then it isn't committed.
    lock.confirm()?;
    let version = remote::commit_version(store, &manifest)?;

    let (mut pruned, mut freed) = (Vec::new(), 0);
    if let Some(keep) = opts.keep {
        report("cleaning", 0, 0);
        let (deleted, gc) = remote::prune(store, key, keep, (&device.id, &device.name), &|d, t| report("cleaning", d as u64, t as u64))?;
        pruned = deleted;
        freed = gc.freed_bytes;
    }
    Ok(Outcome::Created {
        version,
        uploaded: manifest.new_bytes,
        new_chunks: up.new_chunks.load(Ordering::SeqCst),
        reused_chunks: up.reused.load(Ordering::SeqCst),
        pruned,
        freed,
    })
}

/// What a backup would contain, by component (the app shows it before starting).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewComponent {
    pub kind: super::format::Kind,
    pub name: String,
    pub version: Option<String>,
    pub files: usize,
    pub bytes: u64,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
}

pub fn preview(game: &GameEntry, opts: &components::Options) -> Result<Vec<PreviewComponent>> {
    Ok(components::plan(game, opts)?
        .into_iter()
        .map(|c| PreviewComponent {
            kind: c.kind,
            version: c.version.clone(),
            files: c.files.len(),
            bytes: c.total(),
            excluded_files: c.excluded_files,
            excluded_bytes: c.excluded_bytes,
            name: c.name,
        })
        .collect())
}
