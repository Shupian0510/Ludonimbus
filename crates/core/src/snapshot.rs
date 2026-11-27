//! Scanning local saves, hashing them and packing / applying zip snapshots.
//!
//! A snapshot zip contains `files/<root>/<relative path>` entries plus
//! `snapshot.json` (the [`SnapshotMeta`]).

use crate::bind::{self, Codec};
use crate::config::GameEntry;
use crate::{ludusavi, paths, spec, wgs, xboxsaves};
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
    /// A save someone shared, taken in beside the history ([`crate::share`]).
    Shared,
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
            Trigger::Shared => "shared",
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
            "shared" => Trigger::Shared,
            _ => Trigger::Unknown,
        }
    }

    /// Mainline snapshots form the history; side snapshots never become head.
    pub fn is_mainline(self) -> bool {
        !matches!(self, Trigger::PreRestore | Trigger::ConflictKeep | Trigger::Shared)
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
    /// rule says how (`rules/games/`), e.g. `1.6.15`.
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
    /// The content hash of the saves as a version of the line would hold
    /// them: the files here, and those [`LocalState::carried`].
    pub hash: String,
    pub total_size: u64,
    pub roots: BTreeMap<String, PathBuf>,
    /// Files of the line that are not this version's (another platform's,
    /// for a version that shares its line through a rule) and go with the
    /// saves here unchanged, each naming the pack that holds it. Empty for
    /// every other version.
    pub carried: Vec<crate::cloud::v2::FileRef>,
    /// The same, for files that have to be packed anew: their content is on
    /// this device (they come from a version no new one may point into).
    pub carried_files: Vec<crate::cloud::packs::LocalFile>,
}

impl LocalState {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// These saves with another platform's files going along (`carried`,
    /// `files`): the hash is then that of the version they make.
    pub fn carrying(mut self, carried: Vec<crate::cloud::v2::FileRef>, files: Vec<crate::cloud::packs::LocalFile>) -> LocalState {
        let mut all = self.entries.clone();
        all.extend(carried.iter().map(|f| FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }));
        all.extend(files.iter().map(|f| FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }));
        self.hash = content_hash(&all);
        self.carried = carried;
        self.carried_files = files;
        self
    }

    /// The saves as they go into a snapshot of layout 2: those here, and
    /// what is carried over and has to be packed anew.
    pub fn pack_files(&self) -> Vec<crate::cloud::packs::LocalFile> {
        let mut out: Vec<crate::cloud::packs::LocalFile> = self
            .files
            .iter()
            .zip(&self.entries)
            .map(|(f, e)| crate::cloud::packs::LocalFile {
                path: e.path.clone(),
                size: e.size,
                hash: e.hash.clone(),
                mtime: e.mtime,
                source: f.abs.clone(),
            })
            .collect();
        out.extend(self.carried_files.iter().cloned());
        out
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
///
/// A version bound through the Xbox app's containers ([`Codec::Wgs`]) has
/// the containers' files among them, under the names the game's other
/// versions use, and never the container files themselves. Their content is
/// kept in [`paths::decoded_dir`] while the sync reads it.
pub fn scan(game: &GameEntry) -> Result<(Vec<LocalFile>, BTreeMap<String, PathBuf>)> {
    let (files, roots) = scan_own(game)?;
    if game.bind.is_empty() {
        return Ok((files, roots));
    }
    from_newer(game)?;
    let found = match bind::has_wgs(&game.bind) {
        true => decode(files)?,
        false => Found { plain: files, containers: Vec::new(), decoded: Vec::new() },
    };
    let binds = bind::rebase(&game.bind, &found.folders());
    let mut out: BTreeMap<String, LocalFile> = BTreeMap::new();
    for mut f in found.plain {
        if let Some(canon) = bind::to_canon_as(&binds, &f.logical, &Codec::Files) {
            f.logical = canon;
            out.insert(f.logical.to_lowercase(), f);
        }
    }
    if !found.decoded.is_empty() {
        for d in found.decoded {
            let Some(canon) = bind::to_canon_as(&binds, &d.logical, &Codec::Wgs) else { continue };
            let abs = keep_decoded(&d.named.data)?;
            out.insert(canon.to_lowercase(), LocalFile { logical: canon, abs, size: d.named.data.len() as u64, mtime: d.mtime });
        }
        tidy_decoded();
    }
    Ok((out.into_values().collect(), roots))
}

/// A game whose bindings a newer Ludonimbus made (a codec this build
/// doesn't know) is neither read nor written here.
fn from_newer(game: &GameEntry) -> Result<()> {
    if bind::known(&game.bind) {
        return Ok(());
    }
    bail!(crate::tr!(
        "「{}」和别的版本共用存档的设置是更新版本的 Ludonimbus 做的，这台设备上的 Ludonimbus 需要先更新",
        "How \"{}\" shares its saves with another version was set up by a newer Ludonimbus. Update Ludonimbus on this device first",
        game.name
    ))
}

/// An Xbox app game's save containers among a version's own files.
struct Containers {
    /// The container folder among the version's own paths (`root/…`).
    logical: String,
    abs: PathBuf,
}

/// A file read out of a version's containers.
struct Decoded {
    /// `<container folder>/<the name the other versions use>`.
    logical: String,
    /// Which of [`Found::containers`] it is in.
    dir: usize,
    named: xboxsaves::Named,
    /// When its container was last written, in seconds since 1970.
    mtime: i64,
}

/// A version's own files with its containers read out ([`decode`]).
struct Found {
    /// The files outside every container folder.
    plain: Vec<LocalFile>,
    containers: Vec<Containers>,
    decoded: Vec<Decoded>,
}

impl Found {
    /// The container folders, among the version's own paths.
    fn folders(&self) -> Vec<String> {
        self.containers.iter().map(|c| c.logical.clone()).collect()
    }
}

/// The files of a version (`files`, a scan of its own) that lie in a folder
/// of the Xbox app's containers are not its saves as they are: in their
/// place come the containers' files, under the names the game's other
/// versions use ([`xboxsaves::named`]). Files a write was cut short in the
/// middle of left there point at nothing and are never read.
fn decode(files: Vec<LocalFile>) -> Result<Found> {
    let mut containers = Vec::new();
    for dir in wgs::user_dirs(files.iter().map(|f| f.logical.as_str())) {
        let index = format!("{dir}/{}", wgs::INDEX);
        let Some(abs) = files.iter().find(|f| f.logical.eq_ignore_ascii_case(&index)).and_then(|f| f.abs.parent()) else { continue };
        containers.push(Containers { logical: dir, abs: abs.to_path_buf() });
    }
    let plain: Vec<LocalFile> = files.into_iter().filter(|f| !containers.iter().any(|c| bind::is_within(&c.logical, &f.logical))).collect();
    let mut decoded = Vec::new();
    for (i, c) in containers.iter().enumerate() {
        let (idx, named) = xboxsaves::read_dir(&c.abs)
            .with_context(|| crate::tr!("无法读取 Xbox 应用的存档容器 {}", "Can't read the Xbox app's save containers in {}", c.abs.display()))?;
        for n in named {
            let logical = format!("{}/{}", c.logical, n.path);
            if validate_logical(&logical).is_err() {
                continue;
            }
            let when = n.origin.as_ref().and_then(|(name, _)| idx.entries.iter().find(|e| &e.name == name)).map_or(idx.filetime, |e| e.filetime);
            decoded.push(Decoded { logical, dir: i, named: n, mtime: wgs::unix_secs(when) });
        }
    }
    Ok(Found { plain, containers, decoded })
}

/// The paths of a version's own files, with the files in its Xbox app
/// containers under the names the game's other versions use instead of the
/// container files; and the container folders. What bindings are worked
/// out from ([`crate::join::suggestion`]).
pub fn scan_named(game: &GameEntry) -> Result<(Vec<String>, Vec<String>)> {
    let (files, _) = scan_own(game)?;
    let found = decode(files)?;
    let folders = found.folders();
    let mut paths: Vec<String> = found.plain.into_iter().map(|f| f.logical).collect();
    paths.extend(found.decoded.into_iter().map(|d| d.logical));
    Ok((paths, folders))
}

/// Where a file read out of containers is while a sync reads it: in
/// [`paths::decoded_dir`], named by its content, so that two scans at the
/// same time write the same file.
fn keep_decoded(data: &[u8]) -> Result<PathBuf> {
    let path = paths::decoded_dir().join(blake3::hash(data).to_hex().as_str());
    if std::fs::read(&path).is_ok_and(|d| d == data) {
        // In use again: not one to tidy away.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
    } else {
        crate::util::atomic_write(&path, data)?;
    }
    Ok(path)
}

/// Remove the files read out of containers that no scan used for a day.
fn tidy_decoded() {
    let day = std::time::Duration::from_secs(24 * 3600);
    for e in std::fs::read_dir(paths::decoded_dir()).into_iter().flatten().flatten() {
        let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The files of a line (`files`) this version of the game has no folder
/// for ([`GameEntry::bind`]). None for a version whose files are the line's
/// as they are, and none for one that shares another platform's line
/// through a rule ([`crate::join::by_rule`]): the files its bindings don't
/// name are that platform's, not files it lacks a place for. It leaves them
/// out ([`crate::join::held`]).
pub fn no_place_for(game: &GameEntry, files: &[FileEntry]) -> Vec<String> {
    if game.bind.is_empty() || crate::join::by_rule(game).is_some() {
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
        carried: Vec::new(),
        carried_files: Vec::new(),
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
    from_newer(game)?;
    if bind::has_wgs(&game.bind) {
        return apply_coded(files, game, content);
    }
    let (current, roots) = scan(game).unwrap_or_else(|_| {
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
            own.push(bind::to_local(&game.bind, &fe.path).ok_or_else(|| no_place_here(game, &fe.path))?);
        }
    }
    write_files(files, &own, current, roots, game, content)
}

fn no_place_here(game: &GameEntry, path: &str) -> anyhow::Error {
    anyhow!(crate::tr!("「{}」的这个版本没有放 {} 的位置", "This version of \"{}\" has no place for {}", game.name, path))
}

fn unchanged() -> String {
    crate::tr!("存档没有改动", "The saves were left as they were")
}

/// The content of the snapshot's file `path`, as `content` gives it.
fn content_of(content: &mut dyn FnMut(&str, &mut File) -> Result<()>, path: &str) -> Result<Vec<u8>> {
    let tmp = paths::staging_dir().join(format!("read-{}", ulid::Ulid::new()));
    let read = (|| -> Result<Vec<u8>> {
        {
            let mut out = File::create(&tmp).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
            content(path, &mut out)?;
            out.flush()?;
        }
        Ok(std::fs::read(&tmp)?)
    })();
    let _ = std::fs::remove_file(&tmp);
    read
}

/// After `e`, put back the containers of every replacement begun; then the
/// saves are as they were, which the error says when `say_unchanged`. When
/// one can't be put back, the error says that instead: those containers
/// have the new content.
fn put_back(e: anyhow::Error, begun: Vec<wgs::Replacement>, say_unchanged: bool) -> anyhow::Error {
    let mut failed = None;
    for r in begun.into_iter().rev() {
        if let Err(u) = r.undo() {
            failed.get_or_insert(u);
        }
    }
    match failed {
        None if say_unchanged => e.context(unchanged()),
        None => e,
        Some(u) => e.context(crate::tr!(
            "没能把 Xbox 版原来的存档放回去，它现在是新写入的存档：{:#}",
            "The Xbox version's saves couldn't be put back as they were; it has the ones just written: {:#}",
            u
        )),
    }
}

/// [`apply_with`] for a version that keeps files of the line in the Xbox
/// app's containers ([`Codec::Wgs`]; docs/plan-v2.md, sections 4.2 and 6).
/// Writing into them is experimental, so it is held to more than writing
/// plain files:
///
/// - Only save slots the containers have are replaced, each one exactly one
///   container file ([`xboxsaves::Named::origin`]). The version and the
///   bound files here must be the same files, one for one; when they are
///   not, nothing is written and the error names a file that differs.
/// - Only the files whose content differs are written.
/// - Written, then read back the same way they are read for a scan: the
///   containers have to give exactly the version's files. Until they do,
///   what was replaced stays beside them; when they don't, it is put back
///   ([`wgs::Replacement`]).
/// - Plain files the version has besides are written afterwards, all or
///   nothing as ever; when that fails, the containers are put back too.
fn apply_coded(files: &[FileEntry], game: &GameEntry, content: &mut dyn FnMut(&str, &mut File) -> Result<()>) -> Result<()> {
    let refuse = |msg: String| anyhow!(msg).context(unchanged());
    let (own, roots) = scan_own(game)?;
    let found = decode(own)?;
    let binds = bind::rebase(&game.bind, &found.folders());
    // The bound files here that are in containers, by their path in the
    // line: the path as written, and the files read out that have it (more
    // than one: no single file behind it).
    let mut here: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
    for (i, d) in found.decoded.iter().enumerate() {
        if let Some(canon) = bind::to_canon_as(&binds, &d.logical, &Codec::Wgs) {
            here.entry(canon.to_lowercase()).or_insert_with(|| (canon, Vec::new())).1.push(i);
        }
    }
    let not_one = |path: &str| {
        refuse(crate::tr!(
            "{} 在「{}」的 Xbox 版里不是单独的一个文件，不能写回去",
            "{} isn't a single file in the Xbox version of \"{}\", so it can't be written back",
            path,
            game.name
        ))
    };
    // The version's files: into the containers, or plain files.
    let mut coded: BTreeMap<String, (&FileEntry, usize)> = BTreeMap::new();
    let (mut plain, mut plain_own) = (Vec::new(), Vec::new());
    for fe in files {
        validate_logical(&fe.path)?;
        match bind::to_local_with(&binds, &fe.path) {
            Some((_, Codec::Wgs)) => {
                let key = fe.path.to_lowercase();
                let Some((_, at)) = here.get(&key) else {
                    return Err(refuse(crate::tr!(
                        "要写入的版本里有 {}，「{}」的 Xbox 版里没有这个存档位：写入 Xbox 版只能替换已有的存档位",
                        "The version to write has {}, a save slot the Xbox version of \"{}\" doesn't have: writing into the Xbox version can only replace save slots it has",
                        fe.path,
                        game.name
                    )));
                };
                let [i] = at[..] else { return Err(not_one(&fe.path)) };
                if found.decoded[i].named.origin.is_none() {
                    return Err(not_one(&fe.path));
                }
                coded.insert(key, (fe, i));
            }
            Some((local, _)) => {
                plain.push(fe.clone());
                plain_own.push(local);
            }
            None => return Err(no_place_here(game, &fe.path)),
        }
    }
    if let Some((_, (path, _))) = here.iter().find(|(k, _)| !coded.contains_key(*k)) {
        return Err(refuse(crate::tr!(
            "「{}」的 Xbox 版里有 {}，要写入的版本里没有：写入 Xbox 版不能删掉存档位",
            "The Xbox version of \"{}\" has {}, which the version to write doesn't have: writing into the Xbox version can't remove save slots",
            game.name,
            path
        )));
    }
    // What goes into each container folder, checked against the version as
    // plain files are; and what each folder has to read as afterwards.
    let mut writes: Vec<Vec<(String, String, Vec<u8>)>> = vec![Vec::new(); found.containers.len()];
    let mut expect: Vec<BTreeMap<String, String>> = vec![BTreeMap::new(); found.containers.len()];
    for (key, (fe, i)) in &coded {
        let data = content_of(content, &fe.path).map_err(|e| e.context(unchanged()))?;
        let hash = blake3::hash(&data).to_hex().to_string();
        if !fe.hash.is_empty() && hash != fe.hash {
            return Err(refuse(crate::tr!(
                "{} 写到这台设备上以后读回来的内容不对",
                "{} read back differently after it was written to this device",
                fe.path
            )));
        }
        let d = &found.decoded[*i];
        expect[d.dir].insert(key.clone(), hash);
        if d.named.data != data {
            if let Some((c, f)) = d.named.origin.clone() {
                writes[d.dir].push((c, f, data));
            }
        }
    }
    let mut begun: Vec<wgs::Replacement> = Vec::new();
    let written = (|| -> Result<()> {
        for (changes, c) in writes.iter().zip(&found.containers) {
            if !changes.is_empty() {
                begun.push(wgs::begin(&c.abs, changes)?);
            }
        }
        for ((changes, c), want) in writes.iter().zip(&found.containers).zip(&expect) {
            if changes.is_empty() {
                continue;
            }
            // Two files under one name read back are no single file either.
            let mut got: BTreeMap<String, String> = BTreeMap::new();
            let mut one_each = true;
            for n in xboxsaves::read_back(&c.abs)? {
                let Some(canon) = bind::to_canon_as(&binds, &format!("{}/{}", c.logical, n.path), &Codec::Wgs) else { continue };
                one_each &= got.insert(canon.to_lowercase(), blake3::hash(&n.data).to_hex().to_string()).is_none();
            }
            if !one_each || &got != want {
                bail!(crate::tr!(
                    "写进 Xbox 版以后读回来的存档和要写入的版本不一样",
                    "The saves read back from the Xbox version after writing differ from the version that was written"
                ));
            }
        }
        Ok(())
    })();
    if let Err(e) = written {
        return Err(put_back(e, begun, true));
    }
    // The plain files bound here, and the version's.
    let current: Vec<LocalFile> = found
        .plain
        .into_iter()
        .filter_map(|mut f| {
            f.logical = bind::to_canon_as(&binds, &f.logical, &Codec::Files)?;
            Some(f)
        })
        .collect();
    if !plain.is_empty() || !current.is_empty() {
        if let Err(e) = write_files(&plain, &plain_own, current, roots, game, content) {
            // The error says how the plain files were left.
            return Err(put_back(e, begun, false));
        }
    }
    for r in begun {
        r.keep();
    }
    Ok(())
}

/// Write the version's `files` (under the line's paths) at `own` (the same
/// files under this version's paths), all or nothing, and remove the files
/// of `current` (the bound files here, under the line's paths) the version
/// doesn't have.
fn write_files(
    files: &[FileEntry],
    own: &[String],
    current: Vec<LocalFile>,
    mut roots: BTreeMap<String, PathBuf>,
    game: &GameEntry,
    content: &mut dyn FnMut(&str, &mut File) -> Result<()>,
) -> Result<()> {
    let adapter = game.adapter.as_deref().and_then(crate::adapters::find);
    if let Some(a) = adapter {
        // Saves an emulator numbers per PC may have to be created first.
        let mut used: Vec<String> = own.iter().filter_map(|p| p.split('/').next().map(str::to_string)).collect();
        used.dedup();
        crate::emulators::prepare_restore(a, &mut roots, &used)?;
    }
    // Resolve every destination first so nothing is written if a root is missing.
    let mut plan = Vec::with_capacity(files.len());
    for (fe, path) in files.iter().zip(own) {
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
