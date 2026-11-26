//! Ludusavi as a save-location database.
//!
//! The bundled `ludusavi.exe` runs headless with its own configuration folder
//! (`%APPDATA%\Ludonimbus\ludusavi`), so it never touches the user's own
//! Ludusavi setup. We use it to search its manifest and to resolve the save
//! files of PC games; packing and syncing is done by Ludonimbus.

use crate::paths;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
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
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("ludusavi.exe"))
            .find(|p| p.is_file())
    })
}

fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.arg("--config")
        .arg(paths::ludusavi_config_dir())
        .arg("--try-manifest-update");
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
    let start = stdout.find('{').ok_or_else(|| {
        anyhow!(crate::tr!(
            "Ludusavi 没有返回结果：{}",
            "Ludusavi returned nothing: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    })?;
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
    let titles = v["responses"][0]["findTitle"]["titles"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(titles)
}

/// Exact Ludusavi titles for several names in one call (`None` when a name
/// is not in the database). Matching ignores case and punctuation.
pub fn find_titles(names: &[String]) -> Result<Vec<Option<String>>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let exe = exe_path().ok_or_else(|| anyhow!(crate::tr!("没有找到 ludusavi.exe", "ludusavi.exe not found")))?;
    let requests: Vec<Value> = names
        .iter()
        .map(|n| json!({ "findTitle": { "names": [n], "normalized": true } }))
        .collect();
    let mut cmd = command(&exe);
    cmd.arg("api").arg(json!({ "requests": requests }).to_string());
    let v = run_json(cmd)?;
    Ok((0..names.len())
        .map(|i| v["responses"][i]["findTitle"]["titles"][0].as_str().map(str::to_string))
        .collect())
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
        let has_registry = g
            .get("registry")
            .and_then(Value::as_object)
            .is_some_and(|r| !r.is_empty());
        if !files.is_empty() || has_registry {
            out.push(ScanGame {
                title: title.clone(),
                files,
                has_registry,
            });
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

pub fn parse_manifest(
    text: &str,
    titles: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ManifestInfo> {
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
            current = line
                .strip_suffix(':')
                .map(unquote_key)
                .filter(|t| titles.contains(t));
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let titles: std::collections::HashSet<String> =
            ["Hollow Knight", "If on a Winter's Night, Four Travelers"].iter().map(|s| s.to_string()).collect();
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
        assert_eq!(
            m["If on a Winter's Night, Four Travelers"],
            ManifestInfo { steam_id: Some(2173360), ..Default::default() }
        );
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
