//! Which parts a game backup has (docs/plan-game-backup.md, section 8):
//! the program (emulator, recompilation), the game's own files (disc image,
//! ROM, updates, DLC), its settings and mods, firmware, or for games the
//! rules don't describe, the whole folder.
//!
//! Every file gets a logical path starting with a base: `{program}` (the
//! program's folder), `{content}` (the folder holding the game's files),
//! the user folders (`{appdata}`, `{localappdata}`, `{documents}`, `{home}`)
//! or `{extraN}` for anything else. Restoring maps `{program}` and
//! `{content}` to the folders the user picks, and the others to this PC's.

use super::format::Kind;
use super::scan::{self, path_key, ScannedFile};
use super::version;
use crate::adapters::{self, Adapter, Packs, PathsFrom};
use crate::config::GameEntry;
use crate::emulators::{self, EmuGame};
use crate::{emugames, snapshot, spec, util};
use anyhow::{anyhow, Result};
use globset::{GlobBuilder, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Disc images and ROMs: never part of an emulator's program.
const ROMS: &str = "**/*.{iso,chd,cso,rvz,gcz,wia,wbfs,wad,nsp,nsz,xci,xcz,wua,wux,z64,n64,v64,sfc,smc,gba,gbc,nds,3ds,cia,cci,pbp,cue,zip,7z}";

const USER_BASES: [&str; 4] = ["appdata", "localappdata", "documents", "home"];

/// Folders the bases stand for.
#[derive(Debug, Clone, Default)]
pub struct Bases {
    map: BTreeMap<String, PathBuf>,
}

impl Bases {
    /// This PC's user folders plus `{program}` / `{content}` when given.
    pub fn new(program: Option<&Path>, content: Option<&Path>) -> Bases {
        let mut map = BTreeMap::new();
        for (k, v) in spec::builtin_roots(None) {
            if USER_BASES.contains(&k.as_str()) {
                map.insert(k, v);
            }
        }
        if let Some(p) = program {
            map.insert("program".into(), spec::normalize(p));
        }
        if let Some(c) = content {
            map.insert("content".into(), spec::normalize(c));
        }
        Bases { map }
    }

    pub fn set(&mut self, name: &str, p: PathBuf) {
        self.map.insert(name.to_string(), p);
    }

    pub fn get(&self, name: &str) -> Option<&PathBuf> {
        self.map.get(name)
    }

    /// `{base}/rel` for an absolute path: the deepest base that holds it; a
    /// new `{extraN}` (its parent folder) when none does.
    pub fn tokenize(&mut self, abs: &Path) -> String {
        let abs = spec::normalize(abs);
        let key = path_key(&abs);
        let best = self
            .map
            .iter()
            .filter_map(|(name, base)| {
                let b = path_key(base);
                (key == b || key.starts_with(&format!("{b}\\"))).then_some((name.clone(), base.clone(), b.len()))
            })
            .max_by_key(|(_, _, len)| *len);
        let (name, base) = match best {
            Some((n, b, _)) => (n, b),
            None => {
                let parent = abs.parent().map(Path::to_path_buf).unwrap_or_else(|| abs.clone());
                let name = format!("extra{}", self.map.keys().filter(|k| k.starts_with("extra")).count());
                self.map.insert(name.clone(), parent.clone());
                (name, parent)
            }
        };
        let rel = abs.strip_prefix(&base).map(slash).unwrap_or_default();
        if rel.is_empty() {
            format!("{{{name}}}")
        } else {
            format!("{{{name}}}/{rel}")
        }
    }

    /// The absolute path of `{base}/rel`.
    pub fn resolve(&self, logical: &str) -> Result<PathBuf> {
        let (name, rest) = split(logical).ok_or_else(|| anyhow!(crate::tr!("备份里有无效的路径：{logical}", "The backup has an invalid path: {logical}")))?;
        let base = self
            .map
            .get(name)
            .ok_or_else(|| anyhow!(crate::tr!("不知道恢复到哪里：{{{name}}}", "Don't know where to restore {{{name}}}")))?;
        if rest.is_empty() {
            Ok(base.clone())
        } else {
            super::restore::safe_join(base, rest)
        }
    }

    /// The bases (`program`, `content`, `extraN`) the paths use, with the
    /// folders they stand for, to record in the manifest.
    pub fn recorded<'a>(&self, paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for p in paths {
            if let Some((name, _)) = split(p) {
                if !USER_BASES.contains(&name) {
                    if let Some(b) = self.map.get(name) {
                        out.insert(name.to_string(), b.display().to_string());
                    }
                }
            }
        }
        out
    }
}

/// `{name}/rest` → (`name`, `rest`).
pub fn split(logical: &str) -> Option<(&str, &str)> {
    let s = logical.strip_prefix('{')?;
    let end = s.find('}')?;
    let rest = s[end + 1..].trim_start_matches('/');
    Some((&s[..end], rest))
}

fn slash(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// What the user chose for this game's backups.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    /// The program's folder (or the whole folder); `None` = the game's folder.
    pub root: Option<PathBuf>,
    /// Exclude patterns relative to the program's folder.
    pub user_exclude: Vec<String>,
    /// Back up keys, firmware and BIOS.
    pub firmware: bool,
    /// Parts not to back up.
    pub skip: Vec<Kind>,
    /// More files or folders that are part of the game (a ROM the rules don't know).
    pub extra_content: Vec<PathBuf>,
}

/// A component ready to back up.
#[derive(Debug, Clone)]
pub struct Planned {
    pub kind: Kind,
    pub name: String,
    pub version: Option<String>,
    /// Sorted by logical path: the stream order.
    pub files: Vec<PlannedFile>,
    pub dirs: Vec<String>,
    pub exe: Option<String>,
    pub source: Option<String>,
    pub bases: BTreeMap<String, String>,
    /// Program: the launcher that builds the game from its files.
    pub build: Option<String>,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    pub skipped_links: u64,
}

#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub logical: String,
    pub abs: PathBuf,
    pub size: u64,
    pub mtime: i64,
    pub nanos: u32,
}

impl Planned {
    pub fn total(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Every file a `{root}/pattern` matches (a folder matches what is inside).
fn glob_files(pattern: &str, roots: &BTreeMap<String, PathBuf>) -> Vec<PathBuf> {
    let Ok((root_name, rest)) = spec::split_pattern(pattern) else { return Vec::new() };
    let Some(root) = spec::resolve_root_name(&root_name, roots) else { return Vec::new() };
    let rest = rest.trim_matches('/');
    if rest.is_empty() {
        return files_under(&root);
    }
    // Walk only from the part before the first wildcard.
    let parts: Vec<&str> = rest.split('/').collect();
    let lit = parts.iter().take_while(|p| !p.contains(['*', '?', '[', '{'])).count();
    let start = parts[..lit].iter().fold(root.clone(), |acc, p| acc.join(p));
    if lit == parts.len() {
        return if start.is_file() { vec![start] } else { files_under(&start) };
    }
    let mut b = GlobSetBuilder::new();
    for v in [rest.to_string(), format!("{rest}/**")] {
        if let Ok(g) = GlobBuilder::new(&v).case_insensitive(true).literal_separator(true).build() {
            b.add(g);
        }
    }
    let Ok(set) = b.build() else { return Vec::new() };
    walkdir::WalkDir::new(&start)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().strip_prefix(&root).map(|r| set.is_match(slash(r))).unwrap_or(false))
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn files_under(p: &Path) -> Vec<PathBuf> {
    if p.is_file() {
        return vec![p.to_path_buf()];
    }
    if !p.is_dir() {
        return Vec::new();
    }
    walkdir::WalkDir::new(p)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// `{root}/rest` patterns as patterns relative to `dir`, for those whose
/// root lies inside it.
fn rel_patterns(dir: &Path, patterns: &[String], roots: &BTreeMap<String, PathBuf>) -> Vec<String> {
    let d = path_key(dir);
    let mut out = Vec::new();
    for pat in patterns {
        let Ok((name, rest)) = spec::split_pattern(pat) else { continue };
        let Some(r) = spec::resolve_root_name(&name, roots) else { continue };
        let rk = path_key(&r);
        let prefix = if rk == d {
            String::new()
        } else if let Some(sub) = rk.strip_prefix(&format!("{d}\\")) {
            globset::escape(&sub.replace('\\', "/"))
        } else {
            continue;
        };
        let rest = rest.trim_matches('/');
        let joined = match (prefix.is_empty(), rest.is_empty()) {
            (true, true) => "**".to_string(),
            (true, false) => rest.to_string(),
            (false, true) => prefix,
            (false, false) => format!("{prefix}/{rest}"),
        };
        out.push(joined);
    }
    out
}

fn fill(t: &str, g: Option<&EmuGame>) -> Option<String> {
    let mut out = t.to_string();
    if let Some(g) = g {
        for (k, v) in &g.vars {
            out = out.replace(&format!("<{k}>"), v);
        }
    }
    (!out.contains('<')).then_some(out)
}

fn by_kind(map: &BTreeMap<String, Vec<String>>, g: Option<&EmuGame>) -> Vec<String> {
    let list = g.and_then(|g| map.get(&g.variant)).or_else(|| map.get("default"));
    list.map(|l| l.iter().filter_map(|t| fill(t, g)).collect()).unwrap_or_default()
}

/// The whole game the launch options point into: an unpacked Wii U game
/// from its `code/*.rpx`, a PS3 game from `PS3_GAME/USRDIR/EBOOT.BIN`, a
/// PS4 / PSP game folder from its program; disc images and ROMs are the
/// file itself.
pub fn game_root(game_id: Option<&str>, source: &Path) -> PathBuf {
    let name_is = |p: &Path, n: &str| p.file_name().is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case(n));
    if source.is_dir() {
        return source.to_path_buf();
    }
    let parent = source.parent().unwrap_or(source);
    match game_id {
        Some("wiiu") if name_is(parent, "code") => parent.parent().unwrap_or(parent).to_path_buf(),
        Some("ps3") => {
            let mut d = parent;
            if name_is(d, "USRDIR") {
                d = d.parent().unwrap_or(d);
            }
            if name_is(d, "PS3_GAME") {
                d = d.parent().unwrap_or(d);
            }
            d.to_path_buf()
        }
        Some("ps4") => parent.to_path_buf(),
        Some("psp") if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("pbp")) => parent.to_path_buf(),
        _ => source.to_path_buf(),
    }
}

/// INI values of `keys` (in any section), as text.
fn ini_values(text: &str, keys: &[String]) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with([';', '#', '[']) {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            keys.iter().any(|x| x.eq_ignore_ascii_case(k.trim())).then(|| v.trim().trim_matches('"').to_string())
        })
        .collect()
}

fn json_values(v: &serde_json::Value, pointer: &[&str], out: &mut Vec<String>) {
    let Some((first, rest)) = pointer.split_first() else {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => out.extend(a.iter().filter_map(|x| x.as_str().map(str::to_string))),
            _ => {}
        }
        return;
    };
    match (*first, v) {
        ("*", serde_json::Value::Array(a)) => a.iter().for_each(|x| json_values(x, rest, out)),
        ("*", serde_json::Value::Object(o)) => o.values().for_each(|x| json_values(x, rest, out)),
        (k, serde_json::Value::Object(o)) => {
            if let Some(x) = o.get(k) {
                json_values(x, rest, out)
            }
        }
        _ => {}
    }
}

/// The settings file of `pf` on this PC.
pub fn settings_file(pf: &PathsFrom, roots: &BTreeMap<String, PathBuf>) -> Option<PathBuf> {
    spec::expand(&pf.file, roots).ok().filter(|p| p.is_file())
}

/// The existing places a settings file names.
pub fn paths_from(pf: &PathsFrom, roots: &BTreeMap<String, PathBuf>) -> Vec<PathBuf> {
    let Some(file) = settings_file(pf, roots) else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&file) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let mut values = ini_values(&text, &pf.ini);
    if !pf.json.is_empty() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) {
            for ptr in &pf.json {
                json_values(&v, &ptr.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>(), &mut values);
            }
        }
    }
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    values
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            let p = PathBuf::from(v.trim());
            if p.is_absolute() {
                p
            } else {
                base.join(p)
            }
        })
        .flat_map(|p| {
            let also: Vec<PathBuf> = pf.also.iter().map(|a| join_rel(&p, a)).collect();
            std::iter::once(p).chain(also)
        })
        .filter(|p| p.exists())
        .collect()
}

/// `base` joined with a relative path (`..` goes up a folder).
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if part == ".." {
            p.pop();
        } else {
            p.push(part);
        }
    }
    p
}

/// What a launcher builds on this PC from the game files, as `{root}/…`
/// patterns, and the program that builds it: from the rule, and from the
/// `prepare` step of a RecompLauncher package's `recomplauncher.json`.
pub fn build_of(adapter: Option<&Adapter>, root: &Path, roots: &BTreeMap<String, PathBuf>) -> (Option<PathBuf>, Vec<String>) {
    let rule = adapter.and_then(|a| a.build.as_ref());
    let mut launcher = rule.and_then(|b| b.launcher.as_deref()).and_then(|l| spec::expand(l, roots).ok()).filter(|p| p.is_file());
    let mut output: Vec<String> = rule.map(|b| b.output.clone()).unwrap_or_default();
    let manifest = std::fs::read(root.join("recomplauncher.json")).ok().and_then(|b| serde_json::from_str::<serde_json::Value>(String::from_utf8_lossy(&b).trim_start_matches('\u{feff}')).ok());
    if let Some(prepare) = manifest.as_ref().map(|v| &v["prepare"]) {
        let strings = |v: &serde_json::Value| v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>();
        if launcher.is_none() {
            launcher = strings(&prepare["program"]).iter().map(|p| root.join(p)).find(|p| p.is_file());
        }
        for d in strings(&prepare["done"]) {
            if !output.contains(&d) {
                output.push(d);
            }
        }
    }
    (launcher, output)
}

/// Add-on folders whose description file lists this game's id.
fn pack_dirs(packs: &Packs, roots: &BTreeMap<String, PathBuf>, g: Option<&EmuGame>) -> Vec<PathBuf> {
    let (Ok(dir), Some(id)) = (spec::expand(&packs.dir, roots), fill(&packs.id, g)) else { return Vec::new() };
    walkdir::WalkDir::new(&dir)
        .max_depth(4)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(&packs.file))
        .filter(|e| {
            // Not always UTF-8 (comments in other encodings): read leniently.
            let text = String::from_utf8_lossy(&std::fs::read(e.path()).unwrap_or_default()).into_owned();
            ini_values(&text, std::slice::from_ref(&packs.key))
                .iter()
                .any(|v| v.split(',').any(|x| x.trim().eq_ignore_ascii_case(&id)))
        })
        .filter_map(|e| e.path().parent().map(Path::to_path_buf))
        .collect()
}

/// The program's name: an emulator rule's name ("Cemu"); for games, the
/// platform without the console in brackets ("Zelda 64: Recompiled").
fn program_name(a: &Adapter) -> String {
    if a.is_emulator() {
        return a.name.clone();
    }
    let p = a.platform_label().unwrap_or_else(|| a.name.clone());
    let cut = p.find(['（', '(']).unwrap_or(p.len());
    let n = p[..cut].trim();
    if n.is_empty() {
        a.name.clone()
    } else {
        n.to_string()
    }
}

struct Builder<'a> {
    bases: Bases,
    taken: HashSet<String>,
    saves: &'a HashSet<String>,
    out: Vec<Planned>,
}

impl Builder<'_> {
    /// A component made of single files (pattern matches), skipping files
    /// another component already took.
    fn add_files(&mut self, kind: Kind, name: String, version: Option<String>, files: Vec<PathBuf>, source: Option<&Path>) {
        let mut list = Vec::new();
        for f in files {
            let k = path_key(&f);
            if self.saves.contains(&k) || !self.taken.insert(k) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&f) else { continue };
            let (mtime, nanos) = mtime(&meta);
            list.push(PlannedFile { logical: self.bases.tokenize(&f), abs: f, size: meta.len(), mtime, nanos });
        }
        let source = source.map(|s| self.bases.tokenize(s));
        self.push(kind, name, version, list, Vec::new(), None, source, (0, 0, 0));
    }

    /// A component made of one folder (minus excludes and taken files).
    fn add_dir(&mut self, kind: Kind, name: String, version: Option<String>, dir: &Path, exclude: &[String], exe: Option<&Path>) -> Result<()> {
        let mut skip: HashSet<String> = self.taken.clone();
        skip.extend(self.saves.iter().cloned());
        let sc = scan::scan_skip(dir, exclude, &skip)?;
        let mut list = Vec::new();
        for ScannedFile { abs, size, mtime, nanos, .. } in sc.files {
            self.taken.insert(path_key(&abs));
            list.push(PlannedFile { logical: self.bases.tokenize(&abs), abs, size, mtime, nanos });
        }
        let dirs = sc.dirs.iter().map(|d| self.bases.tokenize(&dir.join(d))).collect();
        let exe = exe.filter(|e| e.starts_with(dir)).map(|e| self.bases.tokenize(e));
        self.push(kind, name, version, list, dirs, exe, None, (sc.excluded_files, sc.excluded_bytes, sc.skipped_links));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        kind: Kind,
        name: String,
        version: Option<String>,
        mut files: Vec<PlannedFile>,
        dirs: Vec<String>,
        exe: Option<String>,
        source: Option<String>,
        (excluded_files, excluded_bytes, skipped_links): (u64, u64, u64),
    ) {
        if files.is_empty() && kind != Kind::Program && kind != Kind::Folder {
            return;
        }
        files.sort_by(|a, b| a.logical.cmp(&b.logical));
        let bases = self
            .bases
            .recorded(files.iter().map(|f| f.logical.as_str()).chain(dirs.iter().map(String::as_str)).chain(source.iter().map(String::as_str)));
        self.out.push(Planned { kind, name, version, files, dirs, exe, source, bases, build: None, excluded_files, excluded_bytes, skipped_links });
    }
}

fn mtime(meta: &std::fs::Metadata) -> (i64, u32) {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
        .unwrap_or((0, 0))
}

/// What a backup of `game` contains: its components in a fixed order
/// (content, config, firmware, then the program or whole folder).
pub fn plan(game: &GameEntry, opts: &Options) -> Result<Vec<Planned>> {
    let root = opts
        .root
        .clone()
        .or_else(|| spec::install_dir(game))
        .ok_or_else(|| anyhow!(crate::tr!("不知道游戏文件夹，请先设置游戏位置", "The game's folder is unknown. Set the game location first")))?;
    let adapter = game.adapter.as_deref().and_then(adapters::find);
    let saves: HashSet<String> = snapshot::scan(game)
        .map(|(files, _)| files.iter().map(|f| path_key(&f.abs)).collect())
        .unwrap_or_default();
    let roots = match adapter {
        Some(a) => emugames::roots(a, Some(&root)),
        None => spec::builtin_roots(Some(&root)),
    };
    let emu = adapter.filter(|a| a.is_emulator());
    let emu_game = emu.and_then(|a| game.exe.as_deref().and_then(|exe| emulators::identify_any(a, exe, &game.args)));
    let game_dir = emu_game.as_ref().and_then(|g| g.source.as_deref()).map(|s| game_root(emu.and_then(|a| a.game_id.as_deref()), s));
    let from_settings: Vec<PathBuf> = adapter.map(|a| a.user_files_from.iter().flat_map(|pf| paths_from(pf, &roots)).collect()).unwrap_or_default();
    let content_base = game_dir
        .clone()
        .or_else(|| from_settings.first().cloned())
        .or_else(|| opts.extra_content.first().cloned())
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let mut b = Builder { bases: Bases::new(Some(&root), content_base.as_deref()), taken: HashSet::new(), saves: &saves, out: Vec::new() };
    let skip = |k: Kind| opts.skip.contains(&k);

    // Content: the disc image / game folder, the rule's game files, the
    // user's own additions.
    let mut content: Vec<PathBuf> = Vec::new();
    if let Some(dir) = &game_dir {
        content.extend(files_under(dir));
    }
    if let Some(a) = adapter {
        let patterns = if a.is_emulator() { by_kind(&a.game_files, emu_game.as_ref()) } else { a.user_files.clone() };
        for p in patterns {
            content.extend(glob_files(&p, &roots));
        }
    }
    for p in &from_settings {
        content.extend(files_under(p));
    }
    for p in &opts.extra_content {
        content.extend(files_under(p));
    }
    if !skip(Kind::Content) {
        let version = emu_game
            .as_ref()
            .and_then(|_| version::installed_update(&content))
            .or_else(|| emu_game.as_ref().and_then(|g| version::content(emu.and_then(|a| a.game_id.as_deref()), g)));
        let source = emu_game.as_ref().and_then(|g| g.source.clone());
        b.add_files(Kind::Content, game.name.clone(), version, content, source.as_deref());
    } else {
        b.taken.extend(content.iter().map(|p| path_key(p)));
    }

    // Settings and mods, including the settings files that name the game's
    // places (they are rewritten after a restore).
    if let Some(a) = adapter {
        let mut files: Vec<PathBuf> = by_kind(&a.game_config, emu_game.as_ref()).iter().flat_map(|p| glob_files(p, &roots)).collect();
        for pf in a.config_from.iter() {
            files.extend(paths_from(pf, &roots).iter().flat_map(|p| files_under(p)));
        }
        for pf in a.user_files_from.iter().chain(&a.config_from) {
            files.extend(settings_file(pf, &roots));
        }
        if let Some(packs) = &a.game_packs {
            files.extend(pack_dirs(packs, &roots, emu_game.as_ref()).iter().flat_map(|d| files_under(d)));
        }
        if skip(Kind::Config) {
            b.taken.extend(files.iter().map(|p| path_key(p)));
        } else {
            b.add_files(Kind::Config, crate::tr!("设置和 mods", "Settings and mods"), None, files, None);
        }
        // Firmware: only when ticked; never part of the program either way.
        let fw: Vec<PathBuf> = a.firmware.iter().flat_map(|p| glob_files(p, &roots)).collect();
        if opts.firmware && !skip(Kind::Firmware) {
            b.add_files(Kind::Firmware, crate::tr!("密钥和固件", "Keys and firmware"), None, fw, None);
        } else {
            b.taken.extend(fw.iter().map(|p| path_key(p)));
        }
    }

    // The program, or the whole folder.
    let mut exclude: Vec<String> = scan::DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
    exclude.extend(scan::save_excludes(game, &root));
    exclude.extend(opts.user_exclude.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    // What the launcher builds from the game files is built again after a
    // restore (its folders are not recreated empty either).
    let (launcher, built) = build_of(adapter, &root, &roots);
    let built = rel_patterns(&root, &built, &roots);
    let built_dirs: Vec<String> = built.iter().filter_map(|p| p.strip_suffix("/**")).map(|d| format!("{{program}}/{}", d.to_lowercase())).collect();
    exclude.extend(built);
    match adapter {
        Some(a) => {
            exclude.extend(rel_patterns(&root, &a.saves, &roots));
            exclude.extend(rel_patterns(&root, &a.exclude, &roots));
            if a.is_emulator() {
                exclude.extend(rel_patterns(&root, &a.program_exclude, &roots));
                exclude.push(ROMS.to_string());
            }
            if !skip(Kind::Program) {
                let version = version::program(&root, game.exe.as_deref(), a.is_emulator());
                b.add_dir(Kind::Program, program_name(a), version, &root, &exclude, game.exe.as_deref())?;
            }
        }
        None => {
            if !skip(Kind::Folder) {
                let version = game.exe.as_deref().and_then(|e| version::xbox(e).or_else(|| version::exe_version(e)));
                b.add_dir(Kind::Folder, crate::tr!("整个文件夹", "Whole folder"), version, &root, &exclude, game.exe.as_deref())?;
            }
        }
    }
    let token = launcher.map(|l| b.bases.tokenize(&l)).filter(|t| t.starts_with("{program}/"));
    if let Some(p) = b.out.iter_mut().find(|c| matches!(c.kind, Kind::Program | Kind::Folder)) {
        p.dirs.retain(|d| {
            let d = d.to_lowercase();
            !built_dirs.iter().any(|b| d == *b || d.starts_with(&format!("{b}/")))
        });
        p.build = token;
    }
    let mut out = b.out;
    out.sort_by_key(|c| c.kind);
    Ok(out)
}

/// The folder a game's files were in, for "use the folder this emulator used last".
pub fn content_dir_of(game: &GameEntry) -> Option<PathBuf> {
    let a = game.adapter.as_deref().and_then(adapters::find).filter(|a| a.is_emulator())?;
    let g = emulators::identify_any(a, game.exe.as_deref()?, &game.args)?;
    let root = game_root(a.game_id.as_deref(), g.source.as_deref()?);
    root.parent().map(Path::to_path_buf)
}

/// Same path, as Windows compares them.
pub fn same(a: &Path, b: &Path) -> bool {
    util::same_path(&a.to_string_lossy(), &b.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_take_the_deepest_folder() {
        let mut b = Bases::new(Some(Path::new("D:\\Emu\\Cemu")), Some(Path::new("D:\\Emu")));
        assert_eq!(b.tokenize(Path::new("D:\\Emu\\Cemu\\Cemu.exe")), "{program}/Cemu.exe");
        assert_eq!(b.tokenize(Path::new("D:\\Emu\\Game\\code\\a.rpx")), "{content}/Game/code/a.rpx");
        assert_eq!(b.tokenize(Path::new("E:\\Other\\x.bin")), "{extra0}/x.bin");
        assert_eq!(b.tokenize(Path::new("E:\\Other\\y.bin")), "{extra0}/y.bin");
        assert_eq!(b.recorded(["{program}/a", "{extra0}/x.bin", "{appdata}/z"].into_iter()).len(), 2);
        let mut r = Bases::new(Some(Path::new("C:\\New\\Cemu")), Some(Path::new("C:\\ROMs")));
        r.set("extra0", PathBuf::from("C:\\ROMs\\extra0"));
        assert_eq!(r.resolve("{content}/Game/code/a.rpx").unwrap(), PathBuf::from("C:\\ROMs\\Game\\code\\a.rpx"));
        assert_eq!(r.resolve("{extra0}/x.bin").unwrap(), PathBuf::from("C:\\ROMs\\extra0\\x.bin"));
        assert!(r.resolve("{nope}/x").is_err());
        assert!(r.resolve("{program}/../x").is_err());
    }

    #[test]
    fn whole_folder_counts_what_it_leaves_out() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Game");
        for (p, body) in [("game.exe", "x"), ("save/s.sav", "save"), ("a.log", "log")] {
            std::fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
            std::fs::write(root.join(p), body).unwrap();
        }
        let g = GameEntry { key: "g".into(), exe: Some(root.join("game.exe")), saves: vec!["{install}/save/**".into()], ..Default::default() };
        let plan = plan(&g, &Options::default()).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, Kind::Folder);
        assert_eq!(plan[0].files.len(), 1);
        assert_eq!((plan[0].excluded_files, plan[0].excluded_bytes), (2, 7));
    }

    #[test]
    fn relative_patterns_for_roots_inside_the_folder() {
        let roots = BTreeMap::from([("install".to_string(), PathBuf::from("D:\\Emu")), ("mlc".to_string(), PathBuf::from("D:\\Emu\\mlc01"))]);
        let pats = vec!["{mlc}/usr/title/**".to_string(), "{install}/log.txt".to_string(), "{mlc}".to_string()];
        assert_eq!(rel_patterns(Path::new("D:\\Emu"), &pats, &roots), vec!["mlc01/usr/title/**", "log.txt", "mlc01"]);
        assert!(rel_patterns(Path::new("D:\\Other"), &pats, &roots).is_empty());
    }
}
