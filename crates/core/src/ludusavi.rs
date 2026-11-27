//! Ludusavi as a save-location database.
//!
//! The bundled `ludusavi.exe` runs headless with its own configuration folder
//! (`%APPDATA%\Ludonimbus\ludusavi`), so it never touches the user's own
//! Ludusavi setup. We use it to search its manifest and to resolve the save
//! files of PC games; packing and syncing is done by Ludonimbus.

use crate::paths;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn exe_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LUDONIMBUS_LUDUSAVI") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let sidecar = paths::exe_dir().join("ludusavi.exe");
    if sidecar.is_file() {
        return Some(sidecar);
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join("ludusavi.exe")).find(|p| p.is_file()))
}

fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg("--config").arg(paths::ludusavi_config_dir()).arg("--try-manifest-update");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_json(mut cmd: Command) -> Result<Value> {
    let out = cmd.output().context(crate::tr!("无法运行 Ludusavi", "Couldn't run Ludusavi"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Ludusavi may print log lines before the JSON document.
    let start = stdout
        .find('{')
        .ok_or_else(|| anyhow!(crate::tr!("Ludusavi 没有返回结果：{}", "Ludusavi returned nothing: {}", String::from_utf8_lossy(&out.stderr).trim())))?;
    serde_json::from_str(&stdout[start..]).context(crate::tr!("无法解析 Ludusavi 的输出", "Couldn't read Ludusavi's output"))
}

pub fn version() -> Option<String> {
    let exe = exe_path()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Search game titles in Ludusavi's manifest.
pub fn search(query: &str) -> Result<Vec<String>> {
    let exe = exe_path().ok_or_else(|| anyhow!(crate::tr!("没有找到 ludusavi.exe", "ludusavi.exe not found")))?;
    let input = json!({
        "requests": [ { "findTitle": { "names": [query], "fuzzy": true, "multiple": true } } ]
    });
    let mut cmd = command(&exe);
    cmd.arg("api").arg(input.to_string());
    let v = run_json(cmd)?;
    let titles =
        v["responses"][0]["findTitle"]["titles"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok(titles)
}

/// Exact Ludusavi titles for several names in one call (`None` when a name
/// is not in the database). Matching ignores case and punctuation.
pub fn find_titles(names: &[String]) -> Result<Vec<Option<String>>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let exe = exe_path().ok_or_else(|| anyhow!(crate::tr!("没有找到 ludusavi.exe", "ludusavi.exe not found")))?;
    let requests: Vec<Value> = names.iter().map(|n| json!({ "findTitle": { "names": [n], "normalized": true } })).collect();
    let mut cmd = command(&exe);
    cmd.arg("api").arg(json!({ "requests": requests }).to_string());
    let v = run_json(cmd)?;
    Ok((0..names.len()).map(|i| v["responses"][i]["findTitle"]["titles"][0].as_str().map(str::to_string)).collect())
}

/// A game with saves on this PC, from a full scan.
#[derive(Debug, Clone)]
pub struct ScanGame {
    pub title: String,
    /// `(absolute path, bytes)` of each save file.
    pub files: Vec<(String, u64)>,
    pub has_registry: bool,
}

/// Every game in Ludusavi's database that has saves on this PC. Takes about a
/// minute (it checks tens of thousands of games).
pub fn scan_all() -> Result<Vec<ScanGame>> {
    let exe = exe_path().ok_or_else(|| anyhow!(crate::tr!("没有找到 ludusavi.exe", "ludusavi.exe not found")))?;
    let mut cmd = command(&exe);
    cmd.args(["backup", "--preview", "--api"]);
    parse_scan(&run_json(cmd)?)
}

pub fn parse_scan(v: &Value) -> Result<Vec<ScanGame>> {
    let games = v.get("games").and_then(Value::as_object).ok_or_else(|| anyhow!(crate::tr!("Ludusavi 没有返回游戏列表", "Ludusavi returned no game list")))?;
    let mut out = Vec::new();
    for (title, g) in games {
        let files: Vec<(String, u64)> = g
            .get("files")
            .and_then(Value::as_object)
            .map(|f| {
                f.iter()
                    .filter(|(_, i)| i.get("ignored").and_then(Value::as_bool) != Some(true))
                    .map(|(p, i)| (crate::util::native_sep(p), i.get("bytes").and_then(Value::as_u64).unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default();
        let has_registry = g.get("registry").and_then(Value::as_object).is_some_and(|r| !r.is_empty());
        if !files.is_empty() || has_registry {
            out.push(ScanGame { title: title.clone(), files, has_registry });
        }
    }
    Ok(out)
}

/// What the manifest says about a game.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManifestInfo {
    pub steam_id: Option<u32>,
    /// Other Steam app ids of the same game (editions, remasters).
    pub steam_extra: Vec<u32>,
    /// Steam Cloud keeps the saves of the Steam version in sync.
    pub steam_cloud: bool,
    /// Folder names the game is installed in (under a store's library).
    pub install_dirs: Vec<String>,
}

/// Read Steam ids, Steam Cloud support and install folder names for `titles`
/// from the manifest that Ludusavi downloaded (a large YAML file; only the
/// needed lines are read).
pub fn manifest_info(titles: &std::collections::HashSet<String>) -> std::collections::HashMap<String, ManifestInfo> {
    let text = std::fs::read_to_string(paths::ludusavi_config_dir().join("manifest.yaml")).unwrap_or_default();
    parse_manifest(&text, titles)
}

fn unquote_key(k: &str) -> String {
    if let Some(inner) = k.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else if let Some(inner) = k.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        k.to_string()
    }
}

pub fn parse_manifest(text: &str, titles: &std::collections::HashSet<String>) -> std::collections::HashMap<String, ManifestInfo> {
    let mut out = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    // Second-level section we are in ("steam", "cloud", ...) and the
    // third-level key under it ("steamExtra" under "id").
    let mut section = String::new();
    let mut sub = String::new();
    for line in text.lines() {
        if line.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            current = line.strip_suffix(':').map(unquote_key).filter(|t| titles.contains(t));
            section.clear();
            continue;
        }
        let Some(title) = &current else { continue };
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 2 {
            section = trimmed.trim_end_matches(':').to_string();
            sub.clear();
            continue;
        }
        let info = out.entry(title.clone()).or_insert_with(ManifestInfo::default);
        if indent == 4 {
            sub.clear();
            if section == "installDir" {
                // `Folder Name: {}`, the key may be quoted.
                let key = trimmed.strip_suffix(": {}").or_else(|| trimmed.strip_suffix(':')).unwrap_or(trimmed);
                info.install_dirs.push(unquote_key(key));
                continue;
            }
            match (section.as_str(), trimmed.split_once(':')) {
                ("steam", Some(("id", v))) => info.steam_id = v.trim().parse().ok(),
                ("cloud", Some(("steam", v))) => info.steam_cloud = v.trim() == "true",
                ("id", Some((k, ""))) => sub = k.to_string(),
                _ => {}
            }
        } else if indent >= 6 && section == "id" && sub == "steamExtra" {
            if let Some(id) = trimmed.strip_prefix("- ").and_then(|v| v.trim().parse().ok()) {
                info.steam_extra.push(id);
            }
        }
    }
    out
}

/// The titles of the games with these Steam app ids, from the manifest that
/// Ludusavi downloaded. A game's own id goes before another edition's
/// (`steamExtra`).
pub fn titles_by_steam_id(ids: &std::collections::HashSet<u32>) -> std::collections::HashMap<u32, String> {
    let text = std::fs::read_to_string(paths::ludusavi_config_dir().join("manifest.yaml")).unwrap_or_default();
    parse_steam_titles(&text, ids)
}

pub fn parse_steam_titles(text: &str, ids: &std::collections::HashSet<u32>) -> std::collections::HashMap<u32, String> {
    let mut out = std::collections::HashMap::new();
    let mut editions: Vec<(u32, String)> = Vec::new();
    let (mut title, mut section, mut sub) = (String::new(), String::new(), String::new());
    for line in text.lines() {
        if line.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            title = line.strip_suffix(':').map(unquote_key).unwrap_or_default();
            section.clear();
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 2 {
            section = trimmed.trim_end_matches(':').to_string();
            sub.clear();
        } else if indent == 4 {
            sub.clear();
            match (section.as_str(), trimmed.split_once(':')) {
                ("steam", Some(("id", v))) => {
                    if let Some(id) = v.trim().parse().ok().filter(|id| ids.contains(id)) {
                        out.entry(id).or_insert_with(|| title.clone());
                    }
                }
                ("id", Some((k, ""))) => sub = k.to_string(),
                _ => {}
            }
        } else if indent >= 6 && section == "id" && sub == "steamExtra" {
            if let Some(id) = trimmed.strip_prefix("- ").and_then(|v| v.trim().parse().ok()).filter(|id| ids.contains(id)) {
                editions.push((id, title.clone()));
            }
        }
    }
    for (id, t) in editions {
        out.entry(id).or_insert(t);
    }
    out
}

/// Absolute save files Ludusavi finds for a title on this PC.
pub fn resolve_files(title: &str) -> Result<Vec<PathBuf>> {
    resolve_files_in(title, None)
}

/// Like [`resolve_files`], looking inside a Proton / Wine prefix too (a
/// Windows game on Linux).
pub fn resolve_files_in(title: &str, wine_prefix: Option<&std::path::Path>) -> Result<Vec<PathBuf>> {
    let exe = exe_path().ok_or_else(|| anyhow!("没有找到 ludusavi.exe，无法查找「{title}」的存档"))?;
    let mut cmd = command(&exe);
    cmd.args(["backup", "--preview", "--api"]);
    if let Some(p) = wine_prefix.filter(|p| p.is_dir()) {
        cmd.arg("--wine-prefix").arg(p);
    }
    cmd.arg(title);
    let v = run_json(cmd)?;
    if let Some(errs) = v.get("errors").and_then(Value::as_object) {
        if let Some(unknown) = errs.get("unknownGames") {
            if unknown.as_array().is_some_and(|a| !a.is_empty()) {
                bail!("Ludusavi 数据库里没有「{title}」");
            }
        }
    }
    let mut out = Vec::new();
    if let Some(games) = v.get("games").and_then(Value::as_object) {
        for game in games.values() {
            if let Some(files) = game.get("files").and_then(Value::as_object) {
                for (path, info) in files {
                    if info.get("ignored").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    out.push(PathBuf::from(crate::util::native_sep(path)));
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Save locations from the manifest's rules
//
// A scan of a game Ludusavi knows asks Ludusavi which files are its saves.
// A restore has to know where the game's saves may be before writing
// anything, and the files found today are not enough (a save made later in
// the same folder is one too). So the manifest's rules for the title are
// turned into save patterns the way a scan names the files it finds:
// placeholders become this device's folders, and each folder is written
// under the most specific root (`spec::tokenize`), as the scan's files are.

/// When a rule of the manifest applies: on a system (`windows`, `linux`,
/// `mac`, `dos`), for a store (`steam`, `gog`, …). Either may be missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct When {
    pub os: Option<String>,
    pub store: Option<String>,
}

/// A `files` rule of a game in the manifest: a path with Ludusavi's
/// placeholders (`<base>/saves`, `<winAppData>/Game/*.sav`), and the
/// conditions it applies under (none: always).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileRule {
    pub path: String,
    pub when: Vec<When>,
}

/// Where the manifest says a game keeps its files. Its registry entries are
/// left out: Ludonimbus doesn't back up or restore the registry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameFiles {
    pub files: Vec<FileRule>,
    /// The names of the folders it is installed in (`installDir`).
    pub install_dirs: Vec<String>,
}

/// A key of the manifest as text (an install folder may be named `1979`).
fn key_text(k: &serde_yaml::Value) -> Option<String> {
    match k {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The entries of `titles` in the manifest's text, as YAML. Only their
/// lines are read as YAML: the whole file is tens of megabytes.
fn entries(text: &str, titles: &HashSet<String>) -> HashMap<String, serde_yaml::Value> {
    let mut out = HashMap::new();
    let mut take = |title: String, part: &str| {
        let value = serde_yaml::from_str::<serde_yaml::Value>(part).ok().and_then(|v| v.as_mapping().and_then(|m| m.values().next().cloned()));
        if let Some(v) = value {
            out.insert(title, v);
        }
    };
    let mut current: Option<(String, usize)> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let start = pos;
        pos += line.len();
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() || l.starts_with([' ', '\t']) || l.starts_with('#') {
            continue;
        }
        if let Some((title, from)) = current.take() {
            take(title, &text[from..start]);
        }
        current = l.strip_suffix(':').map(unquote_key).filter(|t| titles.contains(t)).map(|t| (t, start));
    }
    if let Some((title, from)) = current {
        take(title, &text[from..]);
    }
    out
}

fn game_files_of(v: &serde_yaml::Value) -> GameFiles {
    let text = |v: Option<&serde_yaml::Value>| v.and_then(serde_yaml::Value::as_str).map(str::to_string);
    let mut out = GameFiles::default();
    if let Some(files) = v.get("files").and_then(serde_yaml::Value::as_mapping) {
        for (path, info) in files {
            let Some(path) = key_text(path) else { continue };
            let when = info
                .get("when")
                .and_then(serde_yaml::Value::as_sequence)
                .map(|ws| ws.iter().map(|w| When { os: text(w.get("os")), store: text(w.get("store")) }).collect())
                .unwrap_or_default();
            out.files.push(FileRule { path, when });
        }
    }
    if let Some(dirs) = v.get("installDir").and_then(serde_yaml::Value::as_mapping) {
        out.install_dirs = dirs.keys().filter_map(key_text).collect();
    }
    out
}

/// Where the manifest Ludusavi downloaded says each of `titles` keeps its
/// files. A title that is another's alias gets the other's. A title the
/// manifest doesn't have is missing.
pub fn game_files(titles: &HashSet<String>) -> HashMap<String, GameFiles> {
    let text = std::fs::read_to_string(paths::ludusavi_config_dir().join("manifest.yaml")).unwrap_or_default();
    parse_game_files(&text, titles)
}

pub fn parse_game_files(text: &str, titles: &HashSet<String>) -> HashMap<String, GameFiles> {
    let mut out = HashMap::new();
    // The games aliases stand for, and the titles standing for each.
    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (title, v) in entries(text, titles) {
        match v.get("alias").and_then(serde_yaml::Value::as_str).filter(|a| !a.is_empty()) {
            Some(target) => aliases.entry(target.to_string()).or_default().push(title),
            None => {
                out.insert(title, game_files_of(&v));
            }
        }
    }
    if !aliases.is_empty() {
        let targets: HashSet<String> = aliases.keys().cloned().collect();
        for (target, v) in entries(text, &targets) {
            if v.get("alias").is_some() {
                continue;
            }
            let files = game_files_of(&v);
            for title in aliases.remove(&target).unwrap_or_default() {
                out.insert(title, files.clone());
            }
        }
    }
    out
}

/// Ludusavi's root folders and their stores (`steam`, `epic`, `other`…),
/// from its configuration (it looked for them the first time it ran).
pub fn stores() -> Vec<(String, PathBuf)> {
    let text = std::fs::read_to_string(paths::ludusavi_config_dir().join("config.yaml")).unwrap_or_default();
    parse_stores(&text)
}

pub fn parse_stores(text: &str) -> Vec<(String, PathBuf)> {
    let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(text) else { return Vec::new() };
    let Some(roots) = v.get("roots").and_then(serde_yaml::Value::as_sequence) else { return Vec::new() };
    roots
        .iter()
        .filter_map(|r| {
            let path = r.get("path")?.as_str()?.trim();
            let path = match path.strip_prefix('~') {
                Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
                    dirs::home_dir()?.join(crate::util::native_sep(rest.trim_start_matches(['/', '\\'])))
                }
                _ if path.is_empty() => return None,
                _ => PathBuf::from(crate::util::native_sep(path)),
            };
            let store = r.get("store").and_then(serde_yaml::Value::as_str).unwrap_or("other").to_lowercase();
            Some((store, path))
        })
        .collect()
}

/// The system Ludonimbus runs on, as the manifest names systems.
fn host_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

/// Where Ludusavi's placeholders are for one game on this device.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// The game's roots here ([`crate::spec::resolve_roots`]). The save
    /// locations are written against them, the way a scan names the files
    /// it finds.
    pub roots: BTreeMap<String, PathBuf>,
    /// The system this runs on: `windows`, `linux` or `mac`.
    pub os: &'static str,
    /// Windows' folders for the game, by placeholder (`home`, `winAppData`,
    /// …): the user's own on Windows, those in the game's Proton prefix
    /// elsewhere, none without one.
    pub windows: BTreeMap<&'static str, PathBuf>,
    /// The folders of Linux and macOS, by placeholder (`home`, `xdgData`,
    /// `xdgConfig`). None on Windows.
    pub native: BTreeMap<&'static str, PathBuf>,
    /// Ludusavi's root folders and their stores ([`stores`]).
    pub stores: Vec<(String, PathBuf)>,
}

impl Places {
    /// The places for `game`, whose roots here are `roots`.
    pub fn of(game: &crate::config::GameEntry, roots: BTreeMap<String, PathBuf>, stores: Vec<(String, PathBuf)>) -> Places {
        let os = host_os();
        let prefix = crate::spec::proton_prefix(game);
        let root = |k: &str| roots.get(k).cloned();
        let mut windows = BTreeMap::new();
        if os == "windows" || prefix.is_some() {
            // Under Proton these roots are already the prefix's.
            for (placeholder, name) in [("home", "home"), ("winAppData", "appdata"), ("winLocalAppData", "localappdata"), ("winDocuments", "documents")] {
                if let Some(p) = root(name) {
                    windows.insert(placeholder, p);
                }
            }
            if let Some(home) = root("home") {
                windows.insert("winLocalAppDataLow", home.join("AppData").join("LocalLow"));
            }
            match prefix.as_ref().filter(|_| os != "windows") {
                Some(pfx) => {
                    let c = pfx.join("drive_c");
                    windows.insert("winPublic", c.join("users").join("Public"));
                    windows.insert("winProgramData", c.join("ProgramData"));
                    windows.insert("winDir", c.join("windows"));
                }
                None => {
                    let env = |name: &str, or: &str| std::env::var_os(name).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(or));
                    windows.insert("winPublic", env("PUBLIC", r"C:\Users\Public"));
                    windows.insert("winProgramData", env("ProgramData", r"C:\ProgramData"));
                    windows.insert("winDir", env("windir", r"C:\Windows"));
                }
            }
        }
        let mut native = BTreeMap::new();
        if os != "windows" {
            // In a Proton prefix `{home}` is the prefix's user; a rule for
            // Linux itself means the user's own home folder.
            if let Some(home) = if prefix.is_some() { dirs::home_dir() } else { root("home") } {
                native.insert("home", home);
            }
            if let Some(p) = root("xdgdata") {
                native.insert("xdgData", p);
            }
            if let Some(p) = root("xdgconfig") {
                native.insert("xdgConfig", p);
            }
        }
        Places { roots, os, windows, native, stores }
    }

    /// The placeholders of the systems a rule for `os` applies to here.
    /// Windows' rules apply to a game in a Proton prefix too, and a rule
    /// for no system to every one.
    fn systems(&self, os: Option<&str>) -> Vec<&BTreeMap<&'static str, PathBuf>> {
        match os {
            None if self.os == "windows" => vec![&self.windows],
            None => vec![&self.windows, &self.native],
            Some("windows" | "dos") => vec![&self.windows],
            Some(o) if o == self.os => vec![&self.native],
            Some(_) => Vec::new(),
        }
    }
}

/// A name in a path from the manifest.
#[derive(Debug, Clone)]
enum Part {
    /// A file or folder name as it is.
    Name(String),
    /// A wildcard (`*.sav`, `profile[0-3].ojs`), as patterns write one.
    Wild(String),
}

/// `name` in a pattern, matching itself only.
fn escape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '[' | '{' | '}' | '*' | '?' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

/// One name of a path from the manifest, `<game>` being `game` (an install
/// folder's name). A store's user id and the system's user name match any
/// name. `None` for another placeholder: it can't stand inside a path.
///
/// Ludusavi reads `*`, `?` and `[…]` as wildcards and braces as they are,
/// so braces are written so they match themselves.
fn part(raw: &str, game: &str) -> Option<Part> {
    let wild = raw.contains(['*', '?', '[']) || raw.contains("<storeUserId>") || raw.contains("<osUserName>");
    let mut out = String::new();
    let push = |s: &str, out: &mut String| match wild {
        true => out.push_str(&s.replace('{', "[{]").replace('}', "[}]")),
        false => out.push_str(s),
    };
    let mut rest = raw;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start..].find('>') else { break };
        let token = &rest[start + 1..start + len];
        if token.is_empty() || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
            push(&rest[..=start], &mut out);
            rest = &rest[start + 1..];
            continue;
        }
        push(&rest[..start], &mut out);
        match token {
            "game" if wild => out.push_str(&escape_name(game)),
            "game" => out.push_str(game),
            "storeUserId" | "osUserName" => out.push('*'),
            _ => return None,
        }
        rest = &rest[start + len + 1..];
    }
    push(rest, &mut out);
    Some(if wild { Part::Wild(out) } else { Part::Name(out) })
}

/// Each folder a rule's path starts at on this device (its first
/// placeholder) with the names that follow it. `placeholders` are those of
/// the system the rule is for; `store` limits it to Ludusavi's roots of
/// one store; `games` are the names of the game's install folders.
fn expand(path: &str, placeholders: &BTreeMap<&'static str, PathBuf>, store: Option<&str>, games: &[&str], places: &Places) -> Vec<(PathBuf, Vec<Part>)> {
    let path = path.replace('\\', "/");
    let mut names = path.split('/').filter(|n| !n.is_empty());
    let Some(first) = names.next() else { return Vec::new() };
    let rest: Vec<&str> = names.collect();
    let stores = || places.stores.iter().filter(|(s, _)| store.is_none_or(|w| w.eq_ignore_ascii_case(s)));
    let folders: Vec<PathBuf> = match first {
        "<root>" => stores().map(|(_, p)| p.clone()).collect(),
        // The game's folder: the one Ludonimbus knows, and where Ludusavi
        // finds it in its roots.
        "<base>" => {
            let mut out: Vec<PathBuf> = places.roots.get("install").cloned().into_iter().collect();
            for (s, root) in stores() {
                for g in games {
                    let p = if s == "steam" { root.join("steamapps").join("common").join(g) } else { root.join(g) };
                    if p.is_dir() {
                        out.push(p);
                    }
                }
            }
            out
        }
        other => match other.strip_prefix('<').and_then(|p| p.strip_suffix('>')) {
            Some(placeholder) => placeholders.get(placeholder).cloned().into_iter().collect(),
            // The manifest's paths start with a placeholder.
            None => Vec::new(),
        },
    };
    if folders.is_empty() {
        return Vec::new();
    }
    let variants: Vec<&str> = if rest.iter().any(|n| n.contains("<game>")) { games.to_vec() } else { vec![""] };
    let mut out = Vec::new();
    for game in variants {
        let Some(parts) = rest.iter().map(|n| part(n, game)).collect::<Option<Vec<Part>>>() else { continue };
        out.extend(folders.iter().map(|f| (f.clone(), parts.clone())));
    }
    out
}

/// Whether the wildcard `wild` matches the one name `name`, the way a
/// restore compares names (any letter case).
fn matches_name(wild: &str, name: &str) -> bool {
    globset::GlobBuilder::new(wild).case_insensitive(true).literal_separator(true).build().is_ok_and(|g| g.compile_matcher().is_match(name))
}

/// The names of `path` below `folder` when it is inside it (and not the
/// folder itself).
fn below(path: &Path, folder: &Path) -> Option<Vec<String>> {
    let sep = std::path::MAIN_SEPARATOR;
    let p = crate::util::norm_path(&path.to_string_lossy());
    let f = crate::util::norm_path(&folder.to_string_lossy());
    let rest = p.strip_prefix(f.trim_end_matches(sep))?.strip_prefix(sep)?;
    let names: Vec<String> = rest.split(sep).filter(|n| !n.is_empty()).map(str::to_string).collect();
    (!names.is_empty()).then_some(names)
}

/// What is left of `wild` (what follows a folder, from its first wildcard
/// on) inside a root whose path below that folder is `below`. `None` when
/// it doesn't reach into the root.
fn reach(wild: &[String], below: &[String]) -> Option<Vec<String>> {
    for (i, name) in below.iter().enumerate() {
        // It ends at a folder above the root: all of the root is in it.
        let Some(w) = wild.get(i) else { return Some(Vec::new()) };
        if w == "**" {
            return Some(wild[i..].to_vec());
        }
        if !matches_name(w, name) {
            return None;
        }
    }
    Some(wild[below.len()..].to_vec())
}

/// `{root}/rest`. `None` when it would take in a whole folder of the
/// system (every name of it a wildcard alone, or none), unless that is the
/// game's own folder, or when it is no wildcard a restore can read.
fn pattern(root: &str, rest: &[String]) -> Option<String> {
    if root != "install" && rest.iter().all(|n| n.chars().all(|c| matches!(c, '*' | '?'))) {
        return None;
    }
    let rest = rest.join("/");
    if rest.is_empty() {
        return Some(format!("{{{root}}}"));
    }
    for variant in [rest.clone(), format!("{rest}/**")] {
        globset::GlobBuilder::new(&variant).case_insensitive(true).literal_separator(true).build().ok()?;
    }
    Some(format!("{{{root}}}/{rest}"))
}

/// `folder` followed by `parts`, as patterns the way a scan names files:
/// the folder under its most specific root, and every root inside the
/// folder the rest of the path reaches into (`<root>/userdata/<storeUserId>`
/// is in `{steamuser}`).
fn canonical(folder: &Path, parts: &[Part], roots: &BTreeMap<String, PathBuf>) -> Vec<String> {
    let first_wild = parts.iter().position(|p| matches!(p, Part::Wild(_))).unwrap_or(parts.len());
    let mut lit = folder.to_path_buf();
    for p in &parts[..first_wild] {
        if let Part::Name(n) = p {
            lit.push(n);
        }
    }
    let mut wild = Vec::new();
    for p in &parts[first_wild..] {
        match p {
            // Not a place once a wildcard comes before it.
            Part::Name(n) if n == "." || n == ".." => return Vec::new(),
            Part::Name(n) => wild.push(escape_name(n)),
            Part::Wild(w) => wild.push(w.clone()),
        }
    }
    let lit = crate::spec::normalize(&lit);
    if !lit.is_absolute() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let named = crate::spec::tokenize(&lit.join("x"), roots);
    let named = named.strip_suffix("/x").unwrap_or(&named);
    let (root, rel) = named.split_once('/').unwrap_or((named, ""));
    let mut rest: Vec<String> = rel.split('/').filter(|n| !n.is_empty()).map(escape_name).collect();
    rest.extend(wild.iter().cloned());
    out.extend(pattern(root, &rest));
    for (name, path) in roots {
        if let Some(rest) = below(path, &lit).and_then(|b| reach(&wild, &b)) {
            out.extend(pattern(name, &rest));
        }
    }
    out
}

/// The pattern of the folder `dir` and everything in it, written against
/// `roots` the way a scan names files.
pub fn folder_pattern(dir: &Path, roots: &BTreeMap<String, PathBuf>) -> Option<String> {
    canonical(dir, &[], roots).into_iter().next()
}

/// The save locations the manifest's rules for a game give on this device,
/// as save patterns (`{appdata}/Game/*.sav`, `{install}/saves`), written
/// the way a scan names the files Ludusavi finds. A rule for another
/// system, or one whose places this device doesn't have (Windows' folders
/// on Linux without a Proton prefix, the game's folder when it isn't
/// known), gives none. Sorted, each once.
pub fn save_patterns(game: &GameFiles, title: &str, places: &Places) -> Vec<String> {
    let games: Vec<&str> = match game.install_dirs.is_empty() {
        true => vec![title],
        false => game.install_dirs.iter().map(String::as_str).collect(),
    };
    let always = [When::default()];
    let mut out = Vec::new();
    for rule in &game.files {
        let when: &[When] = if rule.when.is_empty() { &always } else { &rule.when };
        for w in when {
            for placeholders in places.systems(w.os.as_deref()) {
                for (folder, parts) in expand(&rule.path, placeholders, w.store.as_deref(), &games, places) {
                    out.extend(canonical(&folder, &parts, &places.roots));
                }
            }
        }
    }
    tidy(out)
}

/// Sorted by name, each once (any letter case).
pub fn tidy(mut patterns: Vec<String>) -> Vec<String> {
    patterns.sort_by_key(|p| p.to_lowercase());
    patterns.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    patterns
}

/// Raised when the way save locations are worked out from the manifest
/// changes, so the ones kept are worked out again.
const PATTERNS_FROM: &str = "1";

/// Ludusavi's version and the manifest it downloaded (its etag), which the
/// save locations from it depend on. `None` when there is no manifest on
/// this device.
pub fn manifest_stamp() -> Option<String> {
    let dir = paths::ludusavi_config_dir();
    let manifest = std::fs::metadata(dir.join("manifest.yaml")).ok().filter(|m| m.is_file())?;
    if let Some(s) = std::fs::read_to_string(dir.join("cache.yaml")).ok().as_deref().and_then(cache_stamp) {
        return Some(s);
    }
    let mtime = manifest.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Some(format!("{} {mtime}", manifest.len()))
}

/// Ludusavi's version and the etags of its manifests, from its cache file.
pub fn cache_stamp(text: &str) -> Option<String> {
    let v: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
    let version: Vec<String> = v
        .get("version")
        .and_then(serde_yaml::Value::as_sequence)
        .map(|s| s.iter().filter_map(serde_yaml::Value::as_u64).map(|n| n.to_string()).collect())
        .unwrap_or_default();
    let etags: Vec<&str> = v.get("manifests")?.as_mapping()?.values().filter_map(|m| m.get("etag")?.as_str()).collect();
    if etags.is_empty() {
        return None;
    }
    Some(format!("{} {}", version.join("."), etags.join(" ")))
}

/// What a game's save locations from the manifest come from, as a short
/// text: the manifest ([`manifest_stamp`]), how this version reads it, the
/// title, and the places on this device.
pub fn saves_stamp(manifest: &str, title: &str, places: &Places) -> String {
    let mut h = blake3::Hasher::new();
    {
        let mut add = |s: &str| {
            h.update(s.as_bytes());
            h.update(&[0]);
        };
        add(PATTERNS_FROM);
        add(manifest);
        add(title);
        add(places.os);
        for (k, v) in &places.roots {
            add(k);
            add(&v.to_string_lossy());
        }
        for (k, v) in places.windows.iter().chain(&places.native) {
            add(k);
            add(&v.to_string_lossy());
        }
        for (s, p) in &places.stores {
            add(s);
            add(&p.to_string_lossy());
        }
    }
    h.finalize().to_hex()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_titles_by_steam_id() {
        let text = r#"Hollow Knight:
  steam:
    id: 367520
"The Witcher 3: Wild Hunt":
  id:
    steamExtra:
      - 499450
  steam:
    id: 292030
The Witcher 3 GOTY:
  steam:
    id: 499450
"#;
        let ids = [367520, 499450, 1].into_iter().collect();
        let m = parse_steam_titles(text, &ids);
        assert_eq!(m[&367520], "Hollow Knight");
        // The edition's own entry goes before the game that lists it.
        assert_eq!(m[&499450], "The Witcher 3 GOTY");
        assert!(!m.contains_key(&1) && !m.contains_key(&292030));
        let m = parse_steam_titles(text, &[292030, 499450].into_iter().collect());
        assert_eq!(m[&292030], "The Witcher 3: Wild Hunt");
    }

    #[test]
    fn reads_manifest_fields() {
        let text = r#"Hollow Knight:
  cloud:
    gog: true
    steam: true
  files:
    <winLocalAppData>/../LocalLow/Team Cherry/Hollow Knight/*.dat:
      tags:
        - save
  id:
    lutris: hollow-knight
    steamExtra:
      - 111
      - 222
  installDir:
    Hollow Knight: {}
    "Hollow Knight: Voidheart": {}
  steam:
    id: 367520
"If on a Winter's Night, Four Travelers":
  steam:
    id: 2173360
Other Game:
  steam:
    id: 1
"#;
        let titles: std::collections::HashSet<String> = ["Hollow Knight", "If on a Winter's Night, Four Travelers"].iter().map(|s| s.to_string()).collect();
        let m = parse_manifest(text, &titles);
        assert_eq!(
            m["Hollow Knight"],
            ManifestInfo {
                steam_id: Some(367520),
                steam_extra: vec![111, 222],
                steam_cloud: true,
                install_dirs: vec!["Hollow Knight".into(), "Hollow Knight: Voidheart".into()],
            }
        );
        assert_eq!(m["If on a Winter's Night, Four Travelers"], ManifestInfo { steam_id: Some(2173360), ..Default::default() });
        assert!(!m.contains_key("Other Game"));
    }

    #[test]
    fn parses_scan_output() {
        let v = json!({ "games": {
            "A": { "files": { "C:/x/a.sav": { "bytes": 5 }, "C:/x/b.log": { "bytes": 1, "ignored": true } }, "registry": {} },
            "B": { "files": {}, "registry": { "HKEY_CURRENT_USER/Software/B": {} } },
            "C": { "files": {}, "registry": {} }
        }});
        let mut g = parse_scan(&v).unwrap();
        g.sort_by(|a, b| a.title.cmp(&b.title));
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].files, vec![(crate::util::native_sep("C:/x/a.sav"), 5)]);
        assert!(g[1].has_registry && g[1].files.is_empty());
    }

    fn roots_of(list: &[(&str, &Path)]) -> BTreeMap<String, PathBuf> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_path_buf())).collect()
    }

    /// `{root}/rel/rest` for the folder `dir`, the way a scan names it with
    /// `roots` (outside them: `drive-c/…` on Windows, `abs/…` elsewhere).
    fn at(dir: &Path, rest: &str, roots: &BTreeMap<String, PathBuf>) -> String {
        let named = crate::spec::tokenize(&dir.join("x"), roots);
        let named = named.strip_suffix("/x").unwrap();
        let (root, rel) = named.split_once('/').unwrap_or((named, ""));
        [format!("{{{root}}}"), rel.to_string(), rest.to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/")
    }

    fn one(text: &str, title: &str) -> GameFiles {
        parse_game_files(text, &[title.to_string()].into_iter().collect()).remove(title).expect(title)
    }

    const HOLLOW_KNIGHT: &str = r#"---
Hollow Knight:
  files:
    "<base>/hollow_knight_Data/Config.ini":
      tags:
        - config
      when:
        - os: windows
        - os: linux
    "<home>/AppData/LocalLow/Team Cherry/Hollow Knight/*.dat":
      tags:
        - save
      when:
        - os: windows
    "<winLocalAppData>/../LocalLow/Team Cherry/Hollow Knight/*.bak":
      when:
        - os: windows
    "<home>/Library/Application Support/unity.Team Cherry.Hollow Knight":
      when:
        - os: mac
    "<xdgConfig>/unity3d/Team Cherry/Hollow Knight/*.dat":
      when:
        - os: linux
    "<root>/userdata/<storeUserId>/367520/remote":
      when:
        - store: steam
    "<root>/userdata/<storeUserId>/367520/epic":
      when:
        - store: epic
    "<winDocuments>/My Games/{random}/profile[0,1,3].sav": {}
    "<winAppData>/<game>/<osUserName>/*.sav": {}
    "<home>/**/remote.sav": {}
    "<home>/*": {}
    "<winDir>/hk.ini":
      when:
        - os: dos
    "<regHkcu>/Software/Team Cherry": {}
  installDir:
    Hollow Knight: {}
  registry:
    HKEY_CURRENT_USER/Software/Team Cherry/Hollow Knight:
      tags:
        - save
  steam:
    id: 367520
Other Game:
  files:
    "<base>/other": {}
"#;

    #[test]
    fn windows_rules_become_save_patterns() {
        let home = crate::util::test_path(r"C:\Users\me");
        let (appdata, local, docs) = (home.join("AppData").join("Roaming"), home.join("AppData").join("Local"), home.join("Documents"));
        let install = crate::util::test_path(r"D:\Games\Hollow Knight");
        let steam = crate::util::test_path(r"C:\Steam");
        let account = steam.join("userdata").join("42");
        let windir = crate::util::test_path(r"C:\Windows");
        let roots =
            roots_of(&[("home", &home), ("appdata", &appdata), ("localappdata", &local), ("documents", &docs), ("install", &install), ("steamuser", &account)]);
        let windows = BTreeMap::from([
            ("home", home.clone()),
            ("winAppData", appdata.clone()),
            ("winLocalAppData", local.clone()),
            ("winLocalAppDataLow", home.join("AppData").join("LocalLow")),
            ("winDocuments", docs.clone()),
            ("winDir", windir.clone()),
        ]);
        let stores = vec![("steam".to_string(), steam.clone()), ("other".to_string(), crate::util::test_path(r"E:\Games"))];
        let places = Places { roots: roots.clone(), os: "windows", windows, native: BTreeMap::new(), stores };
        let game = one(HOLLOW_KNIGHT, "Hollow Knight");
        assert_eq!(game.install_dirs, ["Hollow Knight"]);
        let expected = tidy(vec![
            "{install}/hollow_knight_Data/Config.ini".into(),
            // `<home>/AppData/LocalLow` and `<winLocalAppData>/..` are one folder.
            "{home}/AppData/LocalLow/Team Cherry/Hollow Knight/*.dat".into(),
            "{home}/AppData/LocalLow/Team Cherry/Hollow Knight/*.bak".into(),
            // Steam's folder for the account in use is `{steamuser}`; the
            // others' are where they are.
            "{steamuser}/367520/remote".into(),
            at(&steam.join("userdata"), "*/367520/remote", &roots),
            // Braces are names; `[…]` is a wildcard, as for Ludusavi.
            "{documents}/My Games/[{]random[}]/profile[0,1,3].sav".into(),
            "{appdata}/Hollow Knight/*/*.sav".into(),
            // `**` reaches into the roots inside the folder.
            "{home}/**/remote.sav".into(),
            "{appdata}/**/remote.sav".into(),
            "{localappdata}/**/remote.sav".into(),
            "{documents}/**/remote.sav".into(),
            at(&windir, "hk.ini", &roots),
        ]);
        // Not here: macOS's and Linux's rules, an epic root nobody has, all
        // of the home folder, the registry.
        assert_eq!(save_patterns(&game, "Hollow Knight", &places), expected);
        for p in &expected {
            crate::spec::split_pattern(p).unwrap();
        }
    }

    #[test]
    fn linux_and_proton_rules_become_save_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let pfx = t.join("steam").join("steamapps").join("compatdata").join("1").join("pfx");
        let user = pfx.join("drive_c").join("users").join("steamuser");
        let me = t.join("me");
        let steam = t.join("steam");
        let in_steam = steam.join("steamapps").join("common").join("Hades");
        std::fs::create_dir_all(&in_steam).unwrap();
        let install = t.join("games").join("Hades");
        let (appdata, local, docs) = (user.join("AppData").join("Roaming"), user.join("AppData").join("Local"), user.join("Documents"));
        let (xdgdata, xdgconfig) = (me.join(".local").join("share"), me.join(".config"));
        let text = r#"Hades:
  files:
    "<winDocuments>/Saved Games/Hades":
      when:
        - os: windows
    "<xdgData>/Supergiant Games/Hades":
      when:
        - os: linux
    "<home>/.hades/settings.ini": {}
    "<home>/Library/Application Support/Supergiant Games/Hades":
      when:
        - os: mac
    "<base>/Content/Saves/**": {}
    "<winPublic>/Hades/shared.dat":
      when:
        - os: windows
  installDir:
    Hades: {}
"#;
        let game = one(text, "Hades");
        // Under Proton the roots Windows' folders are named by are the prefix's.
        let roots = roots_of(&[
            ("home", &user),
            ("appdata", &appdata),
            ("localappdata", &local),
            ("documents", &docs),
            ("xdgdata", &xdgdata),
            ("xdgconfig", &xdgconfig),
            ("install", &install),
        ]);
        let windows = BTreeMap::from([
            ("home", user.clone()),
            ("winAppData", appdata.clone()),
            ("winLocalAppData", local.clone()),
            ("winDocuments", docs.clone()),
            ("winPublic", pfx.join("drive_c").join("users").join("Public")),
        ]);
        let native = BTreeMap::from([("home", me.clone()), ("xdgData", xdgdata.clone()), ("xdgConfig", xdgconfig.clone())]);
        let stores = vec![("steam".to_string(), steam.clone())];
        let proton = Places { roots: roots.clone(), os: "linux", windows, native: native.clone(), stores: stores.clone() };
        let common = [
            "{xdgdata}/Supergiant Games/Hades".to_string(),
            at(&me.join(".hades"), "settings.ini", &roots),
            "{install}/Content/Saves/**".to_string(),
            // Where Ludusavi finds the game in its Steam library.
            at(&in_steam.join("Content").join("Saves"), "**", &roots),
        ];
        let mut expected = common.to_vec();
        expected.extend(["{documents}/Saved Games/Hades".to_string(), "{home}/.hades/settings.ini".to_string()]);
        expected.push(at(&pfx.join("drive_c").join("users").join("Public").join("Hades"), "shared.dat", &roots));
        assert_eq!(save_patterns(&game, "Hades", &proton), tidy(expected));

        // A Linux game: no prefix, no Windows folders.
        let native_only = Places { roots, os: "linux", windows: BTreeMap::new(), native, stores };
        assert_eq!(save_patterns(&game, "Hades", &native_only), tidy(common.to_vec()));
    }

    #[test]
    fn reads_aliases_and_install_folders() {
        let text = r#""-KLAUS-":
  alias: Klaus
Klaus:
  files:
    "<base>/save":
      when:
        - os: windows
          store: steam
  installDir:
    1979: {}
Unrelated:
  files:
    "<base>/x": {}
"#;
        let m = parse_game_files(text, &["-KLAUS-".to_string(), "Missing".to_string()].into_iter().collect());
        assert_eq!(m.len(), 1);
        let klaus = &m["-KLAUS-"];
        assert_eq!(klaus.install_dirs, ["1979"]);
        assert_eq!(klaus.files, vec![FileRule { path: "<base>/save".into(), when: vec![When { os: Some("windows".into()), store: Some("steam".into()) }] }]);
    }

    #[test]
    fn reads_ludusavi_roots_and_stamps() {
        let config = "---\nroots:\n  - store: steam\n    path: \"C:/Program Files (x86)/Steam\"\n  - path: D:/Games\n    store: other\n  - store: epic\n    path: ''\nredirects: []\n";
        let sep = |p: &str| PathBuf::from(crate::util::native_sep(p));
        assert_eq!(parse_stores(config), vec![("steam".to_string(), sep("C:/Program Files (x86)/Steam")), ("other".to_string(), sep("D:/Games"))]);
        assert!(parse_stores("not: [yaml").is_empty());

        let cache = "---\nversion:\n  - 0\n  - 31\n  - 0\nmanifests:\n  \"https://example.invalid/manifest.yaml\":\n    etag: \"W/\\\"abc\\\"\"\n    checked: \"2026-11-27T16:33:15Z\"\n";
        assert_eq!(cache_stamp(cache).as_deref(), Some("0.31.0 W/\"abc\""));
        assert_eq!(cache_stamp("version: [0, 31, 0]\n"), None);

        let places = Places { roots: roots_of(&[("install", Path::new("/games/a"))]), os: "linux", ..Default::default() };
        let stamp = saves_stamp("0.31.0 a", "Game", &places);
        assert_eq!(stamp, saves_stamp("0.31.0 a", "Game", &places));
        assert_ne!(stamp, saves_stamp("0.31.0 b", "Game", &places));
        assert_ne!(stamp, saves_stamp("0.31.0 a", "Other", &places));
        let moved = Places { roots: roots_of(&[("install", Path::new("/games/b"))]), ..places.clone() };
        assert_ne!(stamp, saves_stamp("0.31.0 a", "Game", &moved));
    }
}
