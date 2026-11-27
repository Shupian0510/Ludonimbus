//! [Millennium](https://github.com/SteamClientHomebrew/Millennium) integration:
//! detect it, install it from its official GitHub release (the same steps as
//! the official installer: download, verify SHA-256, extract into the Steam
//! folder while Steam is closed), and install / enable the Ludonimbus plugin
//! that shows the cloud save status inside Steam.
//!
//! Windows layout (Millennium 3.6+):
//! ```text
//! <steam>/wsock32.dll                        bootstrap that Steam loads
//! <steam>/millennium/lib/millennium.dll
//! <steam>/millennium/plugins/<file>.star     plugins (starlight format)
//! <steam>/millennium/config/config.json      { "plugins": { "enabledPlugins": [ids] } }
//! ```
//! Millennium rewrites its config when Steam exits, so enabling the plugin
//! must happen while Steam is closed.

use crate::util;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Plugin id from `plugin-millennium/millennium.toml` (what `enabledPlugins` lists).
pub const PLUGIN_ID: &str = "io.github.shupian0510.ludonimbus";
/// File name of the packed plugin, bundled with the app and copied into Steam.
pub const PLUGIN_FILE: &str = "ludonimbus.star";

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/SteamClientHomebrew/Millennium/releases/latest";
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Install {
    Missing,
    /// Millennium 2.x (`user32.dll` + `ext/`): too old for `.star` plugins.
    Legacy,
    Installed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub millennium: Install,
    pub plugin_installed: bool,
    pub plugin_enabled: bool,
    /// The installed plugin is the one bundled with this app version.
    pub plugin_current: bool,
}

pub fn millennium_dir(root: &Path) -> PathBuf {
    root.join("millennium")
}
pub fn plugin_path(root: &Path) -> PathBuf {
    millennium_dir(root).join("plugins").join(PLUGIN_FILE)
}
pub fn config_path(root: &Path) -> PathBuf {
    millennium_dir(root).join("config").join("config.json")
}

pub fn detect(root: &Path) -> Install {
    if root.join("wsock32.dll").is_file() && millennium_dir(root).join("lib").join("millennium.dll").is_file() {
        Install::Installed
    } else if root.join("user32.dll").is_file() && (root.join("millennium.dll").is_file() || root.join("ext").is_dir()) {
        Install::Legacy
    } else {
        Install::Missing
    }
}

pub fn status(root: &Path, bundled: Option<&Path>) -> Status {
    let installed = plugin_path(root);
    let plugin_installed = installed.is_file();
    let plugin_current = plugin_installed && bundled.is_some_and(|b| matches!((std::fs::read(b), std::fs::read(&installed)), (Ok(a), Ok(c)) if a == c));
    Status { millennium: detect(root), plugin_installed, plugin_enabled: is_enabled(root).unwrap_or(false), plugin_current }
}

fn read_config(root: &Path) -> Result<Value> {
    let path = config_path(root);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let v: Value = serde_json::from_slice(&bytes)
                .with_context(|| crate::tr!("Millennium 配置文件格式异常：{}", "Millennium's configuration file is malformed: {}", path.display()))?;
            if !v.is_object() {
                bail!(crate::tr!("Millennium 配置文件格式异常：{}", "Millennium's configuration file is malformed: {}", path.display()));
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", path.display())),
    }
}

pub fn is_enabled(root: &Path) -> Result<bool> {
    let cfg = read_config(root)?;
    Ok(cfg["plugins"]["enabledPlugins"].as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some(PLUGIN_ID))))
}

/// Add or remove the plugin in `enabledPlugins`, keeping everything else.
/// Steam must be closed (Millennium overwrites the file when it exits).
pub fn set_enabled(root: &Path, enabled: bool) -> Result<()> {
    let mut cfg = read_config(root)?;
    let obj = cfg.as_object_mut().expect("checked by read_config");
    let plugins = obj.entry("plugins").or_insert_with(|| json!({}));
    if !plugins.is_object() {
        *plugins = json!({});
    }
    let list = plugins.as_object_mut().expect("just made an object").entry("enabledPlugins").or_insert_with(|| json!([]));
    if !list.is_array() {
        *list = json!([]);
    }
    let arr = list.as_array_mut().expect("just made an array");
    arr.retain(|v| v.as_str() != Some(PLUGIN_ID));
    if enabled {
        arr.push(json!(PLUGIN_ID));
    }
    util::atomic_write(&config_path(root), &serde_json::to_vec_pretty(&cfg)?)
}

/// Copy the bundled plugin into Steam. Safe while Steam runs: Millennium
/// reads the file when it loads plugins, so it takes effect on the next start.
pub fn install_plugin(root: &Path, bundled: &Path) -> Result<()> {
    let bytes = std::fs::read(bundled).with_context(|| crate::tr!("找不到插件文件 {}", "Plugin file not found: {}", bundled.display()))?;
    util::atomic_write(&plugin_path(root), &bytes)
}

/// Delete the plugin and remove it from `enabledPlugins`. Steam must be closed.
pub fn remove_plugin(root: &Path) -> Result<()> {
    let p = plugin_path(root);
    if p.exists() {
        std::fs::remove_file(&p).with_context(|| crate::tr!("无法删除 {}", "Can't delete {}", p.display()))?;
    }
    set_enabled(root, false)
}

// ---------------------------------------------------------------------------
// Installing Millennium from its official release

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub zip_name: String,
    pub zip_url: String,
    pub sha256_url: String,
    pub size: u64,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("Ludonimbus/{}", crate::APP_VERSION))
        .build()
}

/// Newest stable (non-prerelease) Windows build on GitHub.
pub fn latest_release() -> Result<Release> {
    let v: Value = agent()
        .get(LATEST_RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow!(crate::tr!("无法获取 Millennium 的最新版本：{e}", "Couldn't get the latest Millennium version: {e}")))?
        .into_json()?;
    parse_release(&v)
}

fn parse_release(v: &Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().ok_or_else(|| anyhow!(crate::tr!("GitHub 返回的版本信息不完整", "GitHub returned incomplete release information")))?;
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |suffix: &str| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(suffix))).cloned();
    let zip =
        find("-windows-x86_64.zip").ok_or_else(|| anyhow!(crate::tr!("Millennium {tag} 没有 Windows 安装包", "Millennium {tag} has no Windows package")))?;
    let sha = find("-windows-x86_64.sha256").ok_or_else(|| anyhow!(crate::tr!("Millennium {tag} 没有校验文件", "Millennium {tag} has no checksum file")))?;
    let url = |a: &Value| a["browser_download_url"].as_str().map(str::to_string);
    Ok(Release {
        tag: tag.to_string(),
        zip_name: zip["name"].as_str().unwrap_or_default().to_string(),
        zip_url: url(&zip).ok_or_else(|| anyhow!(crate::tr!("缺少下载地址", "Missing download address")))?,
        sha256_url: url(&sha).ok_or_else(|| anyhow!(crate::tr!("缺少校验文件地址", "Missing checksum file address")))?,
        size: zip["size"].as_u64().unwrap_or(0),
    })
}

/// First token of a `sha256sum`-style line, validated as 64 hex chars.
pub fn parse_sha256(text: &str) -> Result<String> {
    let hex = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(crate::tr!("Millennium 校验文件格式异常", "Millennium's checksum file is malformed"));
    }
    Ok(hex)
}

/// Download the release zip into `dir` and verify its SHA-256.
/// `progress(downloaded, total)` is called as data arrives.
pub fn download(release: &Release, dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    let agent = agent();
    let expected = parse_sha256(
        &agent
            .get(&release.sha256_url)
            .call()
            .map_err(|e| anyhow!(crate::tr!("无法下载 Millennium 校验文件：{e}", "Couldn't download Millennium's checksum file: {e}")))?
            .into_string()?,
    )?;
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(&release.zip_name);
    let resp = agent.get(&release.zip_url).call().map_err(|e| anyhow!(crate::tr!("无法下载 Millennium：{e}", "Couldn't download Millennium: {e}")))?;
    let total = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(release.size);
    let mut reader = resp.into_reader().take(MAX_DOWNLOAD + 1);
    let mut file = std::fs::File::create(&dest).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf).context(crate::tr!("下载 Millennium 时连接中断", "The connection dropped while downloading Millennium"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > MAX_DOWNLOAD {
            bail!(crate::tr!("Millennium 安装包大小异常", "The Millennium package has an unexpected size"));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        progress(done, total);
    }
    file.flush()?;
    drop(file);
    let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        let _ = std::fs::remove_file(&dest);
        bail!(crate::tr!(
            "Millennium 安装包校验失败（SHA-256 不一致），请稍后重试",
            "The Millennium package failed its check (SHA-256 mismatch). Try again later"
        ));
    }
    Ok(dest)
}

/// Extract the release zip into the Steam folder, like the official installer.
/// Steam must be closed. Refuses archives that don't look like a Millennium
/// Windows build or that would write outside the Steam folder.
pub fn extract(zip_path: &Path, root: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).context(crate::tr!("Millennium 安装包已损坏", "The Millennium package is damaged"))?;
    let mut names = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!(crate::tr!("Millennium 安装包包含不安全的路径：{}", "The Millennium package contains an unsafe path: {}", entry.name())))?;
        names.push(rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase());
    }
    let has_bootstrap = names.iter().any(|n| n == "wsock32.dll");
    let has_core = names.iter().any(|n| n == "millennium/lib/millennium.dll");
    if !has_bootstrap || !has_core {
        bail!(crate::tr!("Millennium 安装包的内容和预期不一致，已取消安装", "The Millennium package doesn't contain what was expected; install cancelled"));
    }
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let rel = entry.enclosed_name().expect("checked above");
        let out = root.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        std::fs::write(&out, &bytes).with_context(|| {
            crate::tr!("无法写入 {}（Steam 或杀毒软件可能正在占用这个文件）", "Can't write {} (Steam or antivirus software may be using it)", out.display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steam() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn detects_installs() {
        let d = steam();
        assert_eq!(detect(d.path()), Install::Missing);
        std::fs::write(d.path().join("user32.dll"), b"x").unwrap();
        std::fs::create_dir_all(d.path().join("ext")).unwrap();
        assert_eq!(detect(d.path()), Install::Legacy);
        std::fs::write(d.path().join("wsock32.dll"), b"x").unwrap();
        std::fs::create_dir_all(d.path().join("millennium/lib")).unwrap();
        std::fs::write(d.path().join("millennium/lib/millennium.dll"), b"x").unwrap();
        assert_eq!(detect(d.path()), Install::Installed);
    }

    #[test]
    fn enabling_keeps_other_settings() {
        let d = steam();
        assert!(!is_enabled(d.path()).unwrap());
        // Fresh install: no config yet.
        set_enabled(d.path(), true).unwrap();
        assert!(is_enabled(d.path()).unwrap());

        let cfg = json!({
            "general": { "checkForUpdates": true },
            "plugins": { "enabledPlugins": ["other.plugin"], "extra": 1 }
        });
        util::atomic_write(&config_path(d.path()), cfg.to_string().as_bytes()).unwrap();
        set_enabled(d.path(), true).unwrap();
        set_enabled(d.path(), true).unwrap();
        let v = read_config(d.path()).unwrap();
        assert_eq!(v["general"]["checkForUpdates"], json!(true));
        assert_eq!(v["plugins"]["extra"], json!(1));
        assert_eq!(v["plugins"]["enabledPlugins"], json!(["other.plugin", PLUGIN_ID]));

        set_enabled(d.path(), false).unwrap();
        let v = read_config(d.path()).unwrap();
        assert_eq!(v["plugins"]["enabledPlugins"], json!(["other.plugin"]));
    }

    #[test]
    fn broken_config_is_not_overwritten() {
        let d = steam();
        std::fs::create_dir_all(config_path(d.path()).parent().unwrap()).unwrap();
        std::fs::write(config_path(d.path()), b"{ not json").unwrap();
        assert!(set_enabled(d.path(), true).is_err());
        assert_eq!(std::fs::read(config_path(d.path())).unwrap(), b"{ not json");
    }

    #[test]
    fn installs_and_removes_plugin() {
        let d = steam();
        let bundled = d.path().join("bundled.star");
        std::fs::write(&bundled, b"star-v1").unwrap();
        assert!(!status(d.path(), Some(&bundled)).plugin_installed);

        install_plugin(d.path(), &bundled).unwrap();
        set_enabled(d.path(), true).unwrap();
        let s = status(d.path(), Some(&bundled));
        assert!(s.plugin_installed && s.plugin_enabled && s.plugin_current);

        std::fs::write(&bundled, b"star-v2").unwrap();
        assert!(!status(d.path(), Some(&bundled)).plugin_current);

        remove_plugin(d.path()).unwrap();
        let s = status(d.path(), Some(&bundled));
        assert!(!s.plugin_installed && !s.plugin_enabled);
    }

    #[test]
    fn parses_checksums_and_releases() {
        let h = "A".repeat(64);
        assert_eq!(parse_sha256(&format!("{h}  millennium.zip\n")).unwrap(), "a".repeat(64));
        assert!(parse_sha256("nope").is_err());

        let v = json!({
            "tag_name": "v3.6.3",
            "assets": [
                { "name": "millennium-v3.6.3-linux-x86_64.tar.gz", "browser_download_url": "https://x/l", "size": 1 },
                { "name": "millennium-v3.6.3-windows-x86_64.sha256", "browser_download_url": "https://x/s", "size": 103 },
                { "name": "millennium-v3.6.3-windows-x86_64.zip", "browser_download_url": "https://x/z", "size": 4059400 }
            ]
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.tag, "v3.6.3");
        assert_eq!(r.zip_url, "https://x/z");
        assert_eq!(r.sha256_url, "https://x/s");
        assert_eq!(r.size, 4059400);
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn extracts_release_into_steam() {
        let d = steam();
        let zip = d.path().join("m.zip");
        make_zip(&zip, &[("wsock32.dll", b"boot"), ("millennium/lib/millennium.dll", b"core"), ("millennium/ext/data/assets/a.js", b"js")]);
        let root = d.path().join("Steam");
        std::fs::create_dir_all(&root).unwrap();
        extract(&zip, &root).unwrap();
        assert_eq!(detect(&root), Install::Installed);
        assert_eq!(std::fs::read(root.join("millennium/ext/data/assets/a.js")).unwrap(), b"js");
    }

    #[test]
    fn refuses_unexpected_archives() {
        let d = steam();
        let root = d.path().join("Steam");
        std::fs::create_dir_all(&root).unwrap();

        let other = d.path().join("other.zip");
        make_zip(&other, &[("readme.txt", b"hi")]);
        assert!(extract(&other, &root).is_err());

        let evil = d.path().join("evil.zip");
        make_zip(&evil, &[("wsock32.dll", b"boot"), ("millennium/lib/millennium.dll", b"core"), ("../outside.txt", b"x")]);
        assert!(extract(&evil, &root).is_err());
        assert!(!d.path().join("outside.txt").exists());
        assert!(!root.join("wsock32.dll").exists(), "nothing is written when the archive is rejected");
    }
}
