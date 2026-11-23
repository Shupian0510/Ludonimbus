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
    let out = cmd.output().context("无法运行 Ludusavi")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Ludusavi may print log lines before the JSON document.
    let start = stdout.find('{').ok_or_else(|| {
        anyhow!(
            "Ludusavi 没有返回结果：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
    })?;
    serde_json::from_str(&stdout[start..]).context("无法解析 Ludusavi 的输出")
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
    let exe = exe_path().ok_or_else(|| anyhow!("没有找到 ludusavi.exe"))?;
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

/// Absolute save files Ludusavi finds for a title on this PC.
pub fn resolve_files(title: &str) -> Result<Vec<PathBuf>> {
    let exe = exe_path().ok_or_else(|| anyhow!("没有找到 ludusavi.exe，无法查找「{title}」的存档"))?;
    let mut cmd = command(&exe);
    cmd.args(["backup", "--preview", "--api", title]);
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
                    out.push(PathBuf::from(path.replace('/', "\\")));
                }
            }
        }
    }
    Ok(out)
}
