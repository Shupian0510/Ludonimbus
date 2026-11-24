//! What goes into a game backup: the files of one folder in a fixed order,
//! without saves (they sync on their own), logs, dumps and shader caches.

use crate::config::GameEntry;
use crate::{snapshot, spec, util};
use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Excluded from every backup: only things games rebuild themselves.
/// Folders named "cache" are kept: some games keep needed data in them.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/*.log",
    "**/*.dmp",
    "**/*.tmp",
    "**/crashdumps/**",
    "**/shadercache/**",
    "**/shader_cache/**",
    "**/__pycache__/**",
    "**/Thumbs.db",
    "**/desktop.ini",
    ".ludonimbus-restore.json",
];

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    /// Modification time: Unix seconds and nanoseconds.
    pub mtime: i64,
    pub nanos: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// Sorted by `rel`: the stream order.
    pub files: Vec<ScannedFile>,
    /// Folders no included file creates (empty or only excluded content).
    pub dirs: Vec<String>,
    pub total: u64,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    /// Symbolic links and junctions, not followed.
    pub skipped_links: u64,
    /// Patterns in effect.
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub root: String,
    pub files: usize,
    pub bytes: u64,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    pub skipped_links: u64,
}

impl Scan {
    pub fn summary(&self, root: &Path) -> ScanSummary {
        ScanSummary {
            root: root.display().to_string(),
            files: self.files.len(),
            bytes: self.total,
            excluded_files: self.excluded_files,
            excluded_bytes: self.excluded_bytes,
            skipped_links: self.skipped_links,
        }
    }
}

fn globset(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim().trim_start_matches("./").trim_start_matches('/');
        if p.is_empty() {
            continue;
        }
        // A folder pattern ("mods/old") also covers what is inside it.
        for v in [p.to_string(), format!("{}/**", p.trim_end_matches('/'))] {
            b.add(
                GlobBuilder::new(&v)
                    .case_insensitive(true)
                    .literal_separator(true)
                    .build()
                    .with_context(|| crate::tr!("无效的排除规则：{v}", "Invalid exclude pattern: {v}"))?,
            );
        }
    }
    Ok(b.build()?)
}

fn rel_slash(base: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(base).ok()?;
    let s = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    (!s.is_empty()).then_some(s)
}

fn mtime(meta: &std::fs::Metadata) -> (i64, u32) {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
        .unwrap_or((0, 0))
}

/// Patterns that exclude the game's saves inside `root`: save rules whose
/// root lies in it, plus every save file found there now.
pub fn save_excludes(game: &GameEntry, root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let root_n = spec::normalize(root);
    let inside = |p: &Path| -> Option<String> {
        let p = spec::normalize(p);
        if util::same_path(&p.to_string_lossy(), &root_n.to_string_lossy()) {
            return Some(String::new());
        }
        rel_slash(&root_n, &p)
    };
    let sp = spec::effective(game);
    if let Ok(roots) = spec::resolve_roots(game, &sp) {
        for pat in &sp.saves {
            let Ok((name, rest)) = spec::split_pattern(pat) else { continue };
            let Some(r) = spec::resolve_root_name(&name, &roots) else { continue };
            if let Some(prefix) = inside(&r) {
                let rest = rest.trim_matches('/');
                let joined = match (prefix.is_empty(), rest.is_empty()) {
                    (true, true) => continue, // the whole folder is "saves": nothing sensible to exclude
                    (true, false) => rest.to_string(),
                    (false, true) => globset::escape(&prefix),
                    (false, false) => format!("{}/{rest}", globset::escape(&prefix)),
                };
                out.push(joined);
            }
        }
    }
    if let Ok((files, _)) = snapshot::scan(game) {
        for f in files {
            if let Some(rel) = inside(&f.abs).filter(|r| !r.is_empty()) {
                out.push(globset::escape(&rel));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every pattern for a backup of `game` from `root`.
pub fn excludes_for(game: &GameEntry, root: &Path, user: &[String]) -> Vec<String> {
    let mut out: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
    out.extend(save_excludes(game, root));
    out.extend(user.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    out.dedup();
    out
}

/// List the files of `root` without the excluded ones.
pub fn scan(root: &Path, exclude: &[String]) -> Result<Scan> {
    scan_skip(root, exclude, &HashSet::new())
}

/// Lower-case form of a path for [`scan_skip`]'s set.
pub fn path_key(p: &Path) -> String {
    util::norm_path(&spec::normalize(p).to_string_lossy())
}

/// Like [`scan`], also leaving out the files in `skip` ([`path_key`]s):
/// files another component of the backup takes.
pub fn scan_skip(root: &Path, exclude: &[String], skip: &HashSet<String>) -> Result<Scan> {
    if !root.is_dir() {
        bail!(crate::tr!("文件夹不存在：{}", "The folder doesn't exist: {}", root.display()));
    }
    let set = globset(exclude)?;
    let mut s = Scan { exclude: exclude.to_vec(), ..Default::default() };
    let mut all_dirs = Vec::new();
    let mut walker = walkdir::WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = walker.next() {
        let e = entry.with_context(|| crate::tr!("无法读取 {}", "Can't read {}", root.display()))?;
        if e.depth() == 0 {
            continue;
        }
        let Some(rel) = rel_slash(root, e.path()) else { continue };
        let ft = e.file_type();
        if ft.is_symlink() || is_reparse_point(e.path()) {
            s.skipped_links += 1;
            if ft.is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if ft.is_dir() {
            all_dirs.push(rel);
            continue;
        }
        let meta = e.metadata().with_context(|| crate::tr!("无法读取 {}", "Can't read {}", e.path().display()))?;
        if set.is_match(&rel) || (!skip.is_empty() && skip.contains(&path_key(e.path()))) {
            s.excluded_files += 1;
            s.excluded_bytes += meta.len();
            continue;
        }
        s.total += meta.len();
        let (mtime, nanos) = mtime(&meta);
        s.files.push(ScannedFile { rel, abs: e.path().to_path_buf(), size: meta.len(), mtime, nanos });
    }
    s.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let mut created = HashSet::new();
    for f in &s.files {
        let mut p = f.rel.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            if !created.insert(parent.to_lowercase()) {
                break;
            }
            p = parent;
        }
    }
    let dirs: Vec<String> = all_dirs.into_iter().filter(|d| !created.contains(&d.to_lowercase())).collect();
    // Only the deepest ones: creating them creates their parents.
    let mut parents = HashSet::new();
    for d in &dirs {
        let mut p = d.as_str();
        while let Some((parent, _)) = p.rsplit_once('/') {
            parents.insert(parent.to_lowercase());
            p = parent;
        }
    }
    s.dirs = dirs.into_iter().filter(|d| !parents.contains(&d.to_lowercase())).collect();
    s.dirs.sort();
    Ok(s)
}

#[cfg(windows)]
fn is_reparse_point(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
fn is_reparse_point(_: &Path) -> bool {
    false
}

/// An emulator game whose launch arguments name a file outside `root` (the
/// ROM), which a backup of `root` would not include.
pub fn file_outside(game: &GameEntry, root: &Path) -> Option<String> {
    let is_emulator = game.adapter.as_deref().and_then(crate::adapters::find).is_some_and(|a| a.is_emulator());
    if !is_emulator {
        return None;
    }
    let root_n = spec::normalize(root);
    util::split_args(&game.args).into_iter().map(PathBuf::from).find_map(|p| {
        let abs = if p.is_absolute() { p } else { spec::install_dir(game)?.join(p) };
        (abs.is_file() && rel_slash(&root_n, &spec::normalize(&abs)).is_none()).then(|| abs.display().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_in_order_without_excluded_files() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        for (p, body) in [
            ("b.bin", "bb"),
            ("A/z.txt", "z"),
            ("A/y.log", "log"),
            ("Cache/data.pak", "keep"),
            ("ShaderCache/x.bin", "drop"),
            ("saves/slot1.sav", "save"),
        ] {
            let f = r.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, body).unwrap();
        }
        std::fs::create_dir_all(r.join("empty/inner")).unwrap();
        let ex: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).chain(["saves".to_string()]).collect();
        let s = scan(r, &ex).unwrap();
        let rels: Vec<&str> = s.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["A/z.txt", "Cache/data.pak", "b.bin"]);
        assert_eq!(s.total, 1 + 4 + 2);
        assert_eq!(s.excluded_files, 3);
        assert_eq!(s.dirs, vec!["ShaderCache", "empty/inner", "saves"]);
        assert!(scan(&r.join("missing"), &ex).is_err());
        assert!(globset(&["[".to_string()]).is_err());
    }
}
