//! Scanning local saves, hashing them and packing / applying zip snapshots.
//!
//! A snapshot zip contains `files/<root>/<relative path>` entries plus
//! `snapshot.json` (the [`SnapshotMeta`]).

use crate::config::GameEntry;
use crate::{ludusavi, paths, spec};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    /// Uploaded after the game exited (Steam wrapper).
    AutoExit,
    /// Uploaded before launch because local saves changed outside Ludonimbus.
    PreLaunch,
    /// Uploaded after the background watcher saw the game exit.
    Passive,
    /// Uploaded by "sync now" in the app / CLI.
    SyncNow,
    /// Created by the user.
    Manual,
    /// Created after restoring an older version, making it the newest.
    Restore,
    /// Local saves that were about to be overwritten by a restore / pull.
    PreRestore,
    /// The side that lost a conflict.
    ConflictKeep,
}

impl Trigger {
    /// Mainline snapshots form the history; side snapshots never become head.
    pub fn is_mainline(self) -> bool {
        !matches!(self, Trigger::PreRestore | Trigger::ConflictKeep)
    }
    /// Kept forever by retention.
    pub fn is_pinned(self) -> bool {
        matches!(self, Trigger::Manual)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    pub id: String,
    pub game: String,
    pub parent: Option<String>,
    pub device_id: String,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub trigger: Trigger,
    /// Absolute root folders on the device that made the snapshot.
    pub roots: BTreeMap<String, String>,
    pub content_hash: String,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub app_version: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    pub parent: Option<String>,
    pub device_id: String,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub trigger: Trigger,
    pub content_hash: String,
    pub total_size: u64,
    pub file_count: usize,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub locked: bool,
}

impl From<&SnapshotMeta> for SnapshotSummary {
    fn from(m: &SnapshotMeta) -> Self {
        SnapshotSummary {
            id: m.id.clone(),
            parent: m.parent.clone(),
            device_id: m.device_id.clone(),
            device_name: m.device_name.clone(),
            created_at: m.created_at,
            trigger: m.trigger,
            content_hash: m.content_hash.clone(),
            total_size: m.total_size,
            file_count: m.files.len(),
            comment: m.comment.clone(),
            locked: m.locked,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFile {
    /// `root/relative/path`
    pub logical: String,
    #[serde(skip)]
    pub abs: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

/// Current local saves of a game, hashed.
#[derive(Debug, Clone)]
pub struct LocalState {
    pub files: Vec<LocalFile>,
    pub entries: Vec<FileEntry>,
    pub hash: String,
    pub total_size: u64,
    pub roots: BTreeMap<String, PathBuf>,
}

impl LocalState {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

pub const EMPTY_HASH: &str = "empty";

fn has_glob_meta(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_end_matches('/');
        if p.is_empty() {
            b.add(GlobBuilder::new("**").literal_separator(true).build()?);
            continue;
        }
        for variant in [p.to_string(), format!("{p}/**")] {
            b.add(
                GlobBuilder::new(&variant)
                    .case_insensitive(true)
                    .literal_separator(true)
                    .build()
                    .with_context(|| crate::tr!("无效的通配符：{variant}", "Invalid wildcard: {variant}"))?,
            );
        }
    }
    Ok(b.build()?)
}

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .map(|t| DateTime::<Utc>::from(t).timestamp())
        .unwrap_or(0)
}

fn rel_slash(base: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(base).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Find the save files of a game on this device.
pub fn scan(game: &GameEntry) -> Result<(Vec<LocalFile>, BTreeMap<String, PathBuf>)> {
    let spec = spec::effective(game);
    let mut roots = spec::resolve_roots(game, &spec)?;

    if let Some(title) = &spec.ludusavi_title {
        let mut out = BTreeMap::new();
        for abs in ludusavi::resolve_files(title)? {
            let logical = spec::tokenize(&abs, &roots);
            let root_name = logical.split('/').next().unwrap_or_default().to_string();
            if let Some(p) = spec::resolve_root_name(&root_name, &roots) {
                roots.entry(root_name).or_insert(p);
            }
            if let Ok(meta) = std::fs::metadata(&abs) {
                if meta.is_file() {
                    out.insert(
                        logical.to_lowercase(),
                        LocalFile {
                            logical,
                            size: meta.len(),
                            mtime: mtime_of(&meta),
                            abs,
                        },
                    );
                }
            }
        }
        return Ok((out.into_values().collect(), roots));
    }

    if spec.saves.is_empty() {
        bail!(crate::tr!("「{}」还没有设置存档位置", "\"{}\" has no save location yet", game.name));
    }

    // Exclusions grouped by root.
    let mut excl_by_root: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &spec.exclude {
        let (root, rest) = spec::split_pattern(e)?;
        excl_by_root.entry(root).or_default().push(rest);
    }
    let mut excl_sets: BTreeMap<String, GlobSet> = BTreeMap::new();
    for (root, pats) in &excl_by_root {
        excl_sets.insert(root.clone(), build_globset(pats)?);
    }

    let mut out: BTreeMap<String, LocalFile> = BTreeMap::new();
    for pattern in &spec.saves {
        let (root, rest) = spec::split_pattern(pattern)?;
        let base = spec::resolve_root_name(&root, &roots)
            .ok_or_else(|| anyhow!(crate::tr!("无法确定根目录 {{{root}}}（游戏路径未设置？）", "Can't resolve the root {{{root}}} (is the game location set?)")))?;
        // Walk from the longest literal prefix to avoid scanning huge trees.
        let comps: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
        let literal: Vec<&str> = comps
            .iter()
            .take_while(|c| !has_glob_meta(c))
            .copied()
            .collect();
        let walk_root = literal.iter().fold(base.clone(), |p, c| p.join(c));
        if !walk_root.exists() {
            continue;
        }
        let matcher = build_globset(std::slice::from_ref(&rest))?;
        let excl = excl_sets.get(&root);
        for entry in walkdir::WalkDir::new(&walk_root).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(rel) = rel_slash(&base, entry.path()) else {
                continue;
            };
            if !rest.is_empty() && !matcher.is_match(&rel) {
                continue;
            }
            if excl.is_some_and(|x| x.is_match(&rel)) {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let logical = format!("{root}/{rel}");
            out.insert(
                logical.to_lowercase(),
                LocalFile {
                    logical,
                    abs: entry.path().to_path_buf(),
                    size: meta.len(),
                    mtime: mtime_of(&meta),
                },
            );
        }
    }
    Ok((out.into_values().collect(), roots))
}

fn hash_file(p: &Path) -> Result<String> {
    let mut f = File::open(p).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", p.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn content_hash(entries: &[FileEntry]) -> String {
    if entries.is_empty() {
        return EMPTY_HASH.to_string();
    }
    let mut lines: Vec<String> = entries
        .iter()
        .map(|e| format!("{}\0{}", e.path.to_lowercase(), e.hash))
        .collect();
    lines.sort();
    blake3::hash(lines.join("\n").as_bytes()).to_hex().to_string()
}

/// Scan and hash the local saves of a game.
pub fn local_state(game: &GameEntry) -> Result<LocalState> {
    let (files, roots) = scan(game)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut total = 0;
    for f in &files {
        entries.push(FileEntry {
            path: f.logical.clone(),
            size: f.size,
            hash: hash_file(&f.abs)?,
            mtime: f.mtime,
        });
        total += f.size;
    }
    let hash = content_hash(&entries);
    Ok(LocalState {
        files,
        entries,
        hash,
        total_size: total,
        roots,
    })
}

/// Pack a snapshot zip into the staging folder.
pub fn build(
    game: &GameEntry,
    local: &LocalState,
    trigger: crate::snapshot::Trigger,
    parent: Option<String>,
    device: (&str, &str),
    comment: &str,
) -> Result<(SnapshotMeta, PathBuf)> {
    let id = ulid::Ulid::new().to_string();
    let used_roots: HashSet<&str> = local
        .entries
        .iter()
        .filter_map(|e| e.path.split('/').next())
        .collect();
    let roots = local
        .roots
        .iter()
        .filter(|(k, _)| used_roots.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
        .collect();
    let meta = SnapshotMeta {
        id: id.clone(),
        game: game.key.clone(),
        parent,
        device_id: device.0.to_string(),
        device_name: device.1.to_string(),
        created_at: Utc::now(),
        trigger,
        roots,
        content_hash: local.hash.clone(),
        files: local.entries.clone(),
        total_size: local.total_size,
        app_version: crate::APP_VERSION.to_string(),
        comment: comment.to_string(),
        locked: false,
    };
    let zip_path = paths::staging_dir().join(format!("{id}.zip"));
    write_zip(&zip_path, local, &meta)?;
    Ok((meta, zip_path))
}

fn write_zip(zip_path: &Path, local: &LocalState, meta: &SnapshotMeta) -> Result<()> {
    let file = File::create(zip_path)
        .with_context(|| crate::tr!("无法创建 {}", "Can't create {}", zip_path.display()))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(false);
    for f in &local.files {
        zw.start_file(format!("files/{}", f.logical), opts)?;
        let mut src = File::open(&f.abs)
            .with_context(|| crate::tr!("无法读取存档文件 {}", "Can't read the save file {}", f.abs.display()))?;
        std::io::copy(&mut src, &mut zw)?;
    }
    zw.start_file("snapshot.json", opts)?;
    zw.write_all(&serde_json::to_vec_pretty(meta)?)?;
    zw.finish()?;
    Ok(())
}

/// Read the metadata stored inside a snapshot zip.
pub fn read_meta(zip_path: &Path) -> Result<SnapshotMeta> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    let mut entry = archive
        .by_name("snapshot.json")
        .map_err(|_| anyhow!(crate::tr!("快照文件损坏：缺少 snapshot.json", "The snapshot is damaged: snapshot.json is missing")))?;
    let mut s = String::new();
    entry.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

fn validate_logical(p: &str) -> Result<()> {
    let ok = !p.is_empty()
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    if !ok {
        bail!(crate::tr!("快照中包含不安全的路径：{p}", "The snapshot contains an unsafe path: {p}"));
    }
    Ok(())
}

/// Resolve a logical path against this device's roots.
pub fn resolve_logical(roots: &BTreeMap<String, PathBuf>, logical: &str) -> Result<PathBuf> {
    validate_logical(logical)?;
    let (root, rest) = logical.split_once('/').ok_or_else(|| anyhow!(crate::tr!("无效路径 {logical}", "Invalid path {logical}")))?;
    let base = spec::resolve_root_name(root, roots)
        .ok_or_else(|| anyhow!(crate::tr!("本机没有根目录 {{{root}}}，无法恢复 {logical}", "This PC has no root {{{root}}}, so {logical} can't be restored")))?;
    Ok(rest.split('/').fold(base, |p, c| p.join(c)))
}

/// Make the local saves exactly match a snapshot: write its files and remove
/// local save files that are not part of it. Callers must protect the
/// current saves first (pre-restore snapshot).
pub fn apply(zip_path: &Path, meta: &SnapshotMeta, game: &GameEntry) -> Result<()> {
    let (current, mut roots) = scan(game).unwrap_or_else(|_| {
        let s = spec::effective(game);
        (Vec::new(), spec::resolve_roots(game, &s).unwrap_or_default())
    });
    let adapter = game.adapter.as_deref().and_then(crate::adapters::find);
    if let Some(a) = adapter {
        // Saves an emulator numbers per PC may have to be created first.
        let mut used: Vec<String> = meta.files.iter().filter_map(|f| f.path.split('/').next().map(str::to_string)).collect();
        used.dedup();
        crate::emulators::prepare_restore(a, &mut roots, &used)?;
    }
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    // Resolve every destination first so nothing is written if a root is missing.
    let mut plan = Vec::with_capacity(meta.files.len());
    for fe in &meta.files {
        plan.push((fe, resolve_logical(&roots, &fe.path)?));
    }
    let keep: HashSet<String> = meta.files.iter().map(|f| f.path.to_lowercase()).collect();
    for (fe, dest) in &plan {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", parent.display()))?;
        }
        let tmp = dest.with_file_name(format!(
            "{}.lntmp",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ));
        {
            let mut entry = archive
                .by_name(&format!("files/{}", fe.path))
                .map_err(|_| anyhow!(crate::tr!("快照中缺少文件 {}", "The snapshot is missing the file {}", fe.path)))?;
            let mut out = File::create(&tmp)
                .with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
            std::io::copy(&mut entry, &mut out)?;
            out.flush()?;
        }
        std::fs::rename(&tmp, dest).with_context(|| crate::tr!("无法替换 {}", "Can't replace {}", dest.display()))?;
        if fe.mtime > 0 {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dest) {
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(fe.mtime as u64);
                let _ = f.set_times(std::fs::FileTimes::new().set_modified(t));
            }
        }
    }
    for f in current {
        if !keep.contains(&f.logical.to_lowercase()) {
            let _ = std::fs::remove_file(&f.abs);
        }
    }
    if let Some(a) = adapter {
        crate::emulators::finish_restore(a, &roots)?;
    }
    Ok(())
}

/// Keep a copy of a snapshot on this PC (safety net for pre-restore copies).
pub fn keep_local_copy(meta: &SnapshotMeta, zip: &Path) -> Result<()> {
    let dir = paths::local_backups_dir().join(&meta.game);
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(zip, dir.join(format!("{}.zip", meta.id)))?;
    std::fs::write(
        dir.join(format!("{}.json", meta.id)),
        serde_json::to_vec_pretty(meta)?,
    )?;
    // Keep the newest 10.
    let mut zips: Vec<_> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "zip"))
        .collect();
    zips.sort();
    while zips.len() > 10 {
        let p = zips.remove(0);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("json"));
    }
    Ok(())
}
