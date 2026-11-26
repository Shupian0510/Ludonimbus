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
    /// A kind of snapshot a newer Ludonimbus makes.
    #[serde(other)]
    Unknown,
}

impl Trigger {
    /// The name in the cloud.
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::AutoExit => "auto-exit",
            Trigger::PreLaunch => "pre-launch",
            Trigger::Passive => "passive",
            Trigger::SyncNow => "sync-now",
            Trigger::Manual => "manual",
            Trigger::Restore => "restore",
            Trigger::PreRestore => "pre-restore",
            Trigger::ConflictKeep => "conflict-keep",
            Trigger::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Trigger {
        match s {
            "auto-exit" => Trigger::AutoExit,
            "pre-launch" => Trigger::PreLaunch,
            "passive" => Trigger::Passive,
            "sync-now" => Trigger::SyncNow,
            "manual" => Trigger::Manual,
            "restore" => Trigger::Restore,
            "pre-restore" => Trigger::PreRestore,
            "conflict-keep" => Trigger::ConflictKeep,
            _ => Trigger::Unknown,
        }
    }

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
    #[serde(default)]
    pub roots: BTreeMap<String, String>,
    pub content_hash: String,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub app_version: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub locked: bool,
    /// The game version that wrote these saves, read from the saves where a
    /// rule says how (`adapters/cross-saves.yaml`), e.g. `1.6.15`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    /// The version of the game installed on the device that made the
    /// snapshot: an emulated game's update, a recompilation's release, an
    /// Xbox app package's version, a program's own version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// What the saves were made on, as the version says it (`pc`,
    /// `pc:xbox`, `emu:<rule>`, `recomp:<rule>`, `console:switch`). Empty
    /// where it isn't known (versions of 0.6).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    /// A newest version of its save line (several when the line has forked).
    /// Not set in lists read the way 0.6 reads them: there the newest
    /// mainline snapshot is the head ([`crate::sync::mainline_head`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub head: bool,
    /// Kept beside the history, never the newest version.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub side: bool,
}

/// The absolute folders of the device that made a snapshot. Layout 2 has no
/// field for them; until bindings replace them (docs/plan-v2.md, 4.2) they
/// ride along as an extra field, because applying a save of another version
/// of the game still reads them.
pub const ROOTS: &str = "roots";

impl SnapshotSummary {
    /// A snapshot of a save line as the app lists it. A kept save shows as
    /// locked, its name as the comment; a snapshot of 0.6 that is not kept
    /// still shows the note it had.
    pub fn of(entry: &crate::cloud::v2::IndexEntry, head: bool) -> Self {
        let comment = match &entry.pin {
            Some(pin) if !pin.name.is_empty() => pin.name.clone(),
            _ => entry.old_note().unwrap_or_default().to_string(),
        };
        SnapshotSummary {
            id: entry.id.clone(),
            parent: entry.parent.clone(),
            device_id: entry.device_id.clone(),
            device_name: entry.device_name.clone(),
            created_at: entry.created_at,
            trigger: Trigger::parse(&entry.trigger),
            content_hash: entry.content_hash.clone(),
            total_size: entry.total_size,
            file_count: entry.file_count,
            comment,
            locked: entry.pin.is_some(),
            game_version: entry.game_version.clone(),
            installed_version: entry.installed_version.clone(),
            platform: entry.platform.clone(),
            head,
            side: entry.is_side(),
        }
    }
}

impl SnapshotMeta {
    /// A manifest in the shape the rest of the app reads snapshots in.
    /// `game` is the key the caller knows the game by; `listed` is the
    /// snapshot's entry in its line when the caller has it (for the name it
    /// is kept under).
    pub fn of(manifest: &crate::cloud::v2::Manifest, game: &str, listed: Option<&crate::cloud::v2::IndexEntry>) -> Self {
        let summary = SnapshotSummary::of(listed.unwrap_or(&manifest.summary()), false);
        let roots = manifest
            .extra
            .get(ROOTS)
            .and_then(|v| serde_json::from_value::<BTreeMap<String, String>>(v.clone()).ok())
            .unwrap_or_default();
        SnapshotMeta {
            id: manifest.id.clone(),
            game: game.to_string(),
            parent: manifest.parent.clone(),
            device_id: manifest.device_id.clone(),
            device_name: manifest.device_name.clone(),
            created_at: manifest.created_at,
            trigger: Trigger::parse(&manifest.trigger),
            roots,
            content_hash: manifest.content_hash.clone(),
            files: manifest
                .files
                .iter()
                .map(|f| FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime })
                .collect(),
            total_size: manifest.total_size,
            app_version: manifest.app_version.clone(),
            comment: summary.comment,
            locked: summary.locked,
            game_version: manifest.game_version.clone(),
            installed_version: manifest.installed_version.clone(),
        }
    }
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
            game_version: m.game_version.clone(),
            installed_version: m.installed_version.clone(),
            platform: String::new(),
            head: false,
            side: !m.trigger.is_mainline(),
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

    /// The saves as they go into a snapshot of layout 2.
    pub fn pack_files(&self) -> Vec<crate::cloud::packs::LocalFile> {
        self.files
            .iter()
            .zip(&self.entries)
            .map(|(f, e)| crate::cloud::packs::LocalFile {
                path: e.path.clone(),
                size: e.size,
                hash: e.hash.clone(),
                mtime: e.mtime,
                source: f.abs.clone(),
            })
            .collect()
    }

    /// The folders the saves are in on this device, by root name: only the
    /// roots a file uses.
    pub fn used_roots(&self) -> BTreeMap<String, String> {
        let used: HashSet<&str> = self.entries.iter().filter_map(|e| e.path.split('/').next()).collect();
        self.roots
            .iter()
            .filter(|(k, _)| used.contains(k.as_str()))
            .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned()))
            .collect()
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

/// Find the save files of a game on this device, under the paths of the
/// save line it syncs with. For a version that shares the line of one that
/// keeps the same saves elsewhere ([`GameEntry::bind`]), those are the
/// line's paths, and files outside every binding are not among them.
pub fn scan(game: &GameEntry) -> Result<(Vec<LocalFile>, BTreeMap<String, PathBuf>)> {
    let (files, roots) = scan_own(game)?;
    if game.bind.is_empty() {
        return Ok((files, roots));
    }
    let mut out: BTreeMap<String, LocalFile> = BTreeMap::new();
    for mut f in files {
        if let Some(canon) = crate::bind::to_canon(&game.bind, &f.logical) {
            f.logical = canon;
            out.insert(f.logical.to_lowercase(), f);
        }
    }
    Ok((out.into_values().collect(), roots))
}

/// The files of a line (`files`) this version of the game has no folder
/// for ([`GameEntry::bind`]). None for a version whose files are the line's
/// as they are.
pub fn no_place_for(game: &GameEntry, files: &[FileEntry]) -> Vec<String> {
    if game.bind.is_empty() {
        return Vec::new();
    }
    crate::bind::uncovered(&game.bind, files.iter().map(|f| f.path.as_str()))
}

/// Find the save files of a game on this device, under the paths this
/// version itself keeps them at.
pub fn scan_own(game: &GameEntry) -> Result<(Vec<LocalFile>, BTreeMap<String, PathBuf>)> {
    let spec = spec::effective(game);
    let mut roots = spec::resolve_roots(game, &spec)?;

    if let Some(title) = &spec.ludusavi_title {
        let mut out = BTreeMap::new();
        for abs in ludusavi::resolve_files_in(title, spec::proton_prefix(game).as_deref())? {
            let logical = spec::tokenize(&abs, &roots);
            let root_name = logical.split('/').next().unwrap_or_default().to_string();
            if let Some(p) = spec::resolve_root_name(&root_name, &roots) {
                roots.entry(root_name).or_insert(p);
            }
            if let Ok(meta) = std::fs::metadata(&abs) {
                if meta.is_file() && !is_leftover(&abs) {
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
            if !entry.file_type().is_file() || is_leftover(entry.path()) {
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

/// What [`apply`] names a file while it writes it, and the content it
/// replaces until every file is in place.
const WRITING: &str = ".lntmp";
const REPLACED: &str = ".lnold";

/// A file a write of ours left behind when it was cut short: never a save.
fn is_leftover(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(WRITING) || n.ends_with(REPLACED))
}

fn beside(dest: &Path, suffix: &str) -> PathBuf {
    dest.with_file_name(format!("{}{suffix}", dest.file_name().unwrap_or_default().to_string_lossy()))
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
    let roots = local.used_roots();
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
        game_version: crate::crosssaves::local_version(game, &local.files, &local.roots),
        installed_version: crate::gamebackup::version::installed(game),
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

/// Pack a snapshot from files given in memory as `(logical path, contents)`
/// (what the Switch client uploads, see [`crate::nxlan`]). `game_version`:
/// the game version that wrote them ([`crate::crosssaves::files_version`]).
pub fn build_from_files(
    game_key: &str,
    files: &[(String, Vec<u8>)],
    trigger: Trigger,
    parent: Option<String>,
    device: (&str, &str),
    comment: &str,
    game_version: Option<String>,
) -> Result<(SnapshotMeta, PathBuf)> {
    let now = Utc::now().timestamp();
    let mut entries = Vec::with_capacity(files.len());
    for (path, data) in files {
        validate_logical(path)?;
        entries.push(FileEntry { path: path.clone(), size: data.len() as u64, hash: blake3::hash(data).to_hex().to_string(), mtime: now });
    }
    let id = ulid::Ulid::new().to_string();
    let meta = SnapshotMeta {
        id: id.clone(),
        game: game_key.to_string(),
        parent,
        device_id: device.0.to_string(),
        device_name: device.1.to_string(),
        created_at: Utc::now(),
        trigger,
        roots: BTreeMap::new(),
        content_hash: content_hash(&entries),
        total_size: entries.iter().map(|e| e.size).sum(),
        files: entries,
        app_version: crate::APP_VERSION.to_string(),
        comment: comment.to_string(),
        locked: false,
        game_version,
        installed_version: None,
    };
    let zip_path = paths::staging_dir().join(format!("{id}.zip"));
    let file = File::create(&zip_path)?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (path, data) in files {
        zw.start_file(format!("files/{path}"), opts)?;
        zw.write_all(data)?;
    }
    zw.start_file("snapshot.json", opts)?;
    zw.write_all(&serde_json::to_vec_pretty(&meta)?)?;
    zw.finish()?;
    Ok((meta, zip_path))
}

/// The content hash of files given in memory as `(path, contents)`.
pub fn files_hash(files: &[(String, Vec<u8>)]) -> String {
    let entries: Vec<FileEntry> = files
        .iter()
        .map(|(path, data)| FileEntry { path: path.clone(), size: data.len() as u64, hash: blake3::hash(data).to_hex().to_string(), mtime: 0 })
        .collect();
    content_hash(&entries)
}

/// Where the file `path` of a snapshot is in a folder that holds the
/// snapshot's files as `<dir>/<path>`.
pub fn resolve_in(dir: &Path, path: &str) -> Result<PathBuf> {
    validate_logical(path)?;
    Ok(path.split('/').fold(dir.to_path_buf(), |p, c| p.join(c)))
}

/// Pack the files of a snapshot that lie in `dir` as `<dir>/<path>` into a
/// zip of the shape 0.6 wrote its snapshots in (`files/<path>` and
/// `snapshot.json`).
pub fn zip_dir(dir: &Path, meta: &SnapshotMeta, zip_path: &Path) -> Result<()> {
    let file = File::create(zip_path).with_context(|| crate::tr!("无法创建 {}", "Can't create {}", zip_path.display()))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for f in &meta.files {
        let src = resolve_in(dir, &f.path)?;
        zw.start_file(format!("files/{}", f.path), opts.large_file(f.size >= 0xF000_0000))?;
        let mut src = File::open(&src).with_context(|| crate::tr!("快照中缺少文件 {}", "The snapshot is missing the file {}", f.path))?;
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
///
/// All or nothing (docs/plan-v2.md, section 6, "安全网"): every file is
/// written beside its place and read back first, and one that is not what
/// the version says stops it with the saves untouched. Then the files are
/// put in place, each one's old content kept beside it until all are; a
/// file that can't be replaced (the game has it open) puts the ones before
/// it back.
pub fn apply(zip_path: &Path, meta: &SnapshotMeta, game: &GameEntry) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    apply_with(&meta.files, game, &mut |path, out| {
        let mut entry = archive
            .by_name(&format!("files/{path}"))
            .map_err(|_| anyhow!(crate::tr!("快照中缺少文件 {}", "The snapshot is missing the file {}", path)))?;
        std::io::copy(&mut entry, out)?;
        Ok(())
    })
}

/// [`apply`], from a folder that holds the snapshot's files as
/// `<dir>/<path>` (what [`crate::cloud::saves::fetch`] leaves).
pub fn apply_dir(dir: &Path, files: &[FileEntry], game: &GameEntry) -> Result<()> {
    apply_with(files, game, &mut |path, out| {
        let src = resolve_in(dir, path)?;
        let mut file = File::open(&src).map_err(|_| anyhow!(crate::tr!("快照中缺少文件 {}", "The snapshot is missing the file {}", path)))?;
        std::io::copy(&mut file, out)?;
        Ok(())
    })
}

/// `content(path, out)` writes the content of the snapshot's file `path`.
fn apply_with(files: &[FileEntry], game: &GameEntry, content: &mut dyn FnMut(&str, &mut File) -> Result<()>) -> Result<()> {
    let (current, mut roots) = scan(game).unwrap_or_else(|_| {
        let s = spec::effective(game);
        (Vec::new(), spec::resolve_roots(game, &s).unwrap_or_default())
    });
    // Where each file is in this version of the game: its path in the save
    // line, or the folder bound to it.
    let mut own = Vec::with_capacity(files.len());
    for fe in files {
        if game.bind.is_empty() {
            own.push(fe.path.clone());
        } else {
            validate_logical(&fe.path)?;
            own.push(crate::bind::to_local(&game.bind, &fe.path).ok_or_else(|| {
                anyhow!(crate::tr!(
                    "「{}」的这个版本没有放 {} 的位置",
                    "This version of \"{}\" has no place for {}",
                    game.name,
                    fe.path
                ))
            })?);
        }
    }
    let adapter = game.adapter.as_deref().and_then(crate::adapters::find);
    if let Some(a) = adapter {
        // Saves an emulator numbers per PC may have to be created first.
        let mut used: Vec<String> = own.iter().filter_map(|p| p.split('/').next().map(str::to_string)).collect();
        used.dedup();
        crate::emulators::prepare_restore(a, &mut roots, &used)?;
    }
    // Resolve every destination first so nothing is written if a root is missing.
    let mut plan = Vec::with_capacity(files.len());
    for (fe, path) in files.iter().zip(&own) {
        plan.push((fe, resolve_logical(&roots, path)?));
    }
    let keep: HashSet<String> = files.iter().map(|f| f.path.to_lowercase()).collect();
    // One file per place. A scan never gives two names that differ in case
    // only, but a version something else wrote might, and on Windows they
    // are one file: the later one wins, as it always did.
    let mut places: HashSet<String> = HashSet::new();
    let mut plan: Vec<(&FileEntry, PathBuf)> =
        plan.into_iter().rev().filter(|(_, dest)| places.insert(dest.to_string_lossy().to_lowercase())).collect();
    plan.reverse();
    // 1. Written beside its place and read back: nothing is replaced yet.
    let mut written: Vec<PathBuf> = Vec::with_capacity(plan.len());
    let staged = (|| -> Result<()> {
        for (fe, dest) in &plan {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", parent.display()))?;
            }
            let tmp = beside(dest, WRITING);
            written.push(tmp.clone());
            {
                let mut out = File::create(&tmp)
                    .with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
                content(&fe.path, &mut out)?;
                out.flush()?;
            }
            // A version of 0.6 may not say what a file's content is.
            if !fe.hash.is_empty() && hash_file(&tmp)? != fe.hash {
                bail!(crate::tr!(
                    "{} 写到这台设备上以后读回来的内容不对",
                    "{} read back differently after it was written to this device",
                    fe.path
                ));
            }
        }
        Ok(())
    })();
    let unchanged = || crate::tr!("存档没有改动", "The saves were left as they were");
    if let Err(e) = staged {
        for tmp in &written {
            let _ = std::fs::remove_file(tmp);
        }
        return Err(e.context(unchanged()));
    }
    // 2. Put in place. What a file replaces stays beside it until all are.
    let mut placed: Vec<(&PathBuf, Option<PathBuf>)> = Vec::with_capacity(plan.len());
    let committed = (|| -> Result<()> {
        for ((_, dest), tmp) in plan.iter().zip(&written) {
            let old = beside(dest, REPLACED);
            let had = dest.is_file();
            if had {
                let _ = std::fs::remove_file(&old);
                std::fs::rename(dest, &old).with_context(|| crate::tr!("无法替换 {}", "Can't replace {}", dest.display()))?;
            }
            placed.push((dest, had.then_some(old)));
            std::fs::rename(tmp, dest).with_context(|| crate::tr!("无法替换 {}", "Can't replace {}", dest.display()))?;
        }
        Ok(())
    })();
    if let Err(e) = committed {
        // Back in the opposite order. A file that can't be put back keeps
        // its content beside its place, and the error says where.
        let mut stuck: Vec<String> = Vec::new();
        for (dest, old) in placed.iter().rev() {
            let _ = std::fs::remove_file(dest);
            if let Some(old) = old {
                if std::fs::rename(old, dest).is_err() {
                    stuck.push(old.display().to_string());
                }
            }
        }
        for tmp in &written {
            let _ = std::fs::remove_file(tmp);
        }
        return Err(match stuck.is_empty() {
            true => e.context(unchanged()),
            false => e.context(crate::tr!(
                "没能把原来的存档放回去，它们的内容在：{}",
                "The saves that were there couldn't be put back. Their content is in: {}",
                stuck.join(", ")
            )),
        });
    }
    for (_, old) in &placed {
        if let Some(old) = old {
            let _ = std::fs::remove_file(old);
        }
    }
    // What a write that was cut short left in these folders.
    let folders: HashSet<&Path> = plan.iter().filter_map(|(_, dest)| dest.parent()).collect();
    for folder in folders {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if is_leftover(&path) && path.is_file() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    for (fe, dest) in &plan {
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
