//! Steam integration without Millennium: install detection, accounts,
//! non-Steam shortcuts (`shortcuts.vdf`), custom artwork, and wrapping a
//! shortcut so it launches through `ludonimbus-run`.

use crate::config::{AppConfig, GameEntry, OriginalShortcut};
use crate::{paths, process, util, vdf};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const STEAMID64_BASE: u64 = 76561197960265728;

/// Steam install folder.
pub fn root(cfg: &AppConfig) -> Option<PathBuf> {
    if let Some(r) = &cfg.steam.root {
        if r.is_dir() {
            return Some(r.clone());
        }
    }
    detect_root()
}

pub fn detect_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        if let Ok(key) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = key.get_value::<String, _>("SteamPath") {
                let p = PathBuf::from(p.replace('/', "\\"));
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    let fallback = PathBuf::from(r"C:\Program Files (x86)\Steam");
    fallback.is_dir().then_some(fallback)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub persona: Option<String>,
    pub account_name: Option<String>,
    pub most_recent: bool,
    pub shortcut_count: usize,
}

pub fn accounts(root: &Path) -> Vec<Account> {
    let login = std::fs::read_to_string(root.join("config").join("loginusers.vdf"))
        .ok()
        .and_then(|s| vdf::parse_text(&s).ok());
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root.join("userdata")) else {
        return out;
    };
    for e in rd.flatten() {
        let id = e.file_name().to_string_lossy().into_owned();
        let Ok(num) = id.parse::<u64>() else { continue };
        if num == 0 {
            continue;
        }
        let user = login
            .as_ref()
            .and_then(|l| l.get("users"))
            .and_then(|u| u.get(&(num + STEAMID64_BASE).to_string()));
        let field = |k: &str| user.and_then(|u| u.get(k)).and_then(|v| v.as_str()).map(str::to_string);
        let shortcut_count = read_shortcuts(root, &id).map(|(_, s)| s.len()).unwrap_or(0);
        out.push(Account {
            id,
            persona: field("PersonaName"),
            account_name: field("AccountName"),
            most_recent: field("MostRecent").as_deref() == Some("1"),
            shortcut_count,
        });
    }
    out.sort_by(|a, b| {
        b.most_recent
            .cmp(&a.most_recent)
            .then(b.shortcut_count.cmp(&a.shortcut_count))
    });
    out
}

/// The account to use: configured, else most recent login, else the one with
/// the most shortcuts.
pub fn default_account(cfg: &AppConfig, root: &Path) -> Option<String> {
    if let Some(a) = &cfg.steam.account {
        if root.join("userdata").join(a).is_dir() {
            return Some(a.clone());
        }
    }
    let all = accounts(root);
    all.iter()
        .find(|a| a.most_recent && a.shortcut_count > 0)
        .or_else(|| all.iter().max_by_key(|a| a.shortcut_count))
        .map(|a| a.id.clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    pub index: String,
    pub appid: u32,
    pub name: String,
    pub exe: String,
    pub start_dir: String,
    pub icon: String,
    pub launch_options: String,
    pub last_play_time: u32,
}

impl Shortcut {
    /// Whether this shortcut already launches through a Ludonimbus wrapper.
    pub fn wrapped_key(&self) -> Option<String> {
        if !util::file_name_lower(Path::new(&self.exe)).eq("ludonimbus-run.exe") {
            return None;
        }
        let args = util::split_args(&self.launch_options);
        args.iter()
            .position(|a| a == "--game")
            .and_then(|i| args.get(i + 1).cloned())
    }
}

pub fn shortcuts_path(root: &Path, account: &str) -> PathBuf {
    root.join("userdata").join(account).join("config").join("shortcuts.vdf")
}

fn to_shortcut(index: &[u8], v: &vdf::Value) -> Shortcut {
    Shortcut {
        index: String::from_utf8_lossy(index).into_owned(),
        appid: v.get("appid").and_then(vdf::Value::int).unwrap_or(0) as u32,
        name: v.get_str("AppName"),
        exe: util::unquote(&v.get_str("Exe")),
        start_dir: util::unquote(&v.get_str("StartDir")),
        icon: util::unquote(&v.get_str("icon")),
        launch_options: v.get_str("LaunchOptions"),
        last_play_time: v.get("LastPlayTime").and_then(vdf::Value::int).unwrap_or(0) as u32,
    }
}

pub fn read_shortcuts(root: &Path, account: &str) -> Result<(vdf::Value, Vec<Shortcut>)> {
    let path = shortcuts_path(root, account);
    if !path.exists() {
        return Ok((
            vdf::Value::Map(vec![(b"shortcuts".to_vec(), vdf::Value::Map(vec![]))]),
            Vec::new(),
        ));
    }
    let bytes = std::fs::read(&path).with_context(|| format!("无法读取 {}", path.display()))?;
    let doc = vdf::parse(&bytes).with_context(|| format!("无法解析 {}", path.display()))?;
    let list = doc
        .get("shortcuts")
        .map(|s| s.entries().iter().map(|(k, v)| to_shortcut(k, v)).collect())
        .unwrap_or_default();
    Ok((doc, list))
}

pub fn find_shortcut(root: &Path, account: &str, appid: u32) -> Result<Option<Shortcut>> {
    Ok(read_shortcuts(root, account)?
        .1
        .into_iter()
        .find(|s| s.appid == appid))
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

pub fn art(root: &Path, account: &str, appid: u32) -> Art {
    let grid = root.join("userdata").join(account).join("config").join("grid");
    let pick = |suffix: &str| {
        util::first_existing(
            ["png", "jpg", "jpeg", "webp"]
                .iter()
                .map(|ext| grid.join(format!("{appid}{suffix}.{ext}"))),
        )
    };
    Art {
        cover: pick("p"),
        hero: pick("_hero"),
        logo: pick("_logo"),
        icon: pick("_icon"),
    }
}

pub fn art_file(root: &Path, account: &str, appid: u32, kind: &str) -> Option<PathBuf> {
    let a = art(root, account, appid);
    match kind {
        "cover" => a.cover,
        "hero" => a.hero,
        "logo" => a.logo,
        "icon" => a.icon,
        _ => None,
    }
}

pub fn is_running() -> bool {
    process::is_running(&["steam.exe".to_string()])
}

/// Ask Steam to exit and wait for it.
pub fn shutdown(root: &Path) -> Result<()> {
    if !is_running() {
        return Ok(());
    }
    let exe = root.join("steam.exe");
    std::process::Command::new(&exe)
        .arg("-shutdown")
        .spawn()
        .with_context(|| format!("无法启动 {}", exe.display()))?;
    let start = Instant::now();
    while is_running() {
        if start.elapsed() > Duration::from_secs(45) {
            bail!("Steam 没有在 45 秒内退出，请手动关闭 Steam 后重试");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // Give Steam a moment to finish writing its files.
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

fn ensure_steam_closed() -> Result<()> {
    if !cfg!(test) && is_running() {
        bail!("STEAM_RUNNING: 修改 Steam 快捷方式前需要先关闭 Steam（Steam 运行时会覆盖修改）");
    }
    Ok(())
}

/// Modify one shortcut entry and save `shortcuts.vdf` (with a backup copy).
fn edit_shortcut(root: &Path, account: &str, appid: u32, f: impl FnOnce(&mut vdf::Value)) -> Result<()> {
    ensure_steam_closed()?;
    let path = shortcuts_path(root, account);
    let (mut doc, _) = read_shortcuts(root, account)?;
    let list = doc
        .get_mut("shortcuts")
        .ok_or_else(|| anyhow!("shortcuts.vdf 格式异常"))?;
    let entry = list
        .entries_mut()
        .and_then(|e| {
            e.iter_mut()
                .map(|(_, v)| v)
                .find(|v| v.get("appid").and_then(vdf::Value::int).map(|i| i as u32) == Some(appid))
        })
        .ok_or_else(|| anyhow!("在 Steam 中找不到这个快捷方式（appid {appid}）"))?;
    f(entry);
    if path.exists() {
        let backup = paths::steam_backups_dir().join(format!(
            "shortcuts-{account}-{}.vdf",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::copy(&path, &backup)?;
    }
    util::atomic_write(&path, &vdf::write(&doc))?;
    Ok(())
}

/// Point a shortcut at `ludonimbus-run --game <key>`. Returns the original fields.
pub fn wrap(root: &Path, account: &str, appid: u32, key: &str, runner: &Path) -> Result<OriginalShortcut> {
    if !runner.is_file() {
        bail!("找不到启动包装器 {}", runner.display());
    }
    let sc = find_shortcut(root, account, appid)?
        .ok_or_else(|| anyhow!("在 Steam 中找不到这个快捷方式（appid {appid}）"))?;
    let original = OriginalShortcut {
        exe: sc.exe.clone(),
        start_dir: sc.start_dir.clone(),
        launch_options: sc.launch_options.clone(),
        icon: sc.icon.clone(),
    };
    let runner_s = runner.to_string_lossy().into_owned();
    let icon = if sc.icon.trim().is_empty() { sc.exe.clone() } else { sc.icon.clone() };
    edit_shortcut(root, account, appid, |e| {
        e.set_str("Exe", &util::quote(&runner_s));
        e.set_str("LaunchOptions", &format!("--game {key}"));
        e.set_str("icon", &util::quote(&icon));
    })?;
    Ok(original)
}

/// Restore the shortcut fields saved by [`wrap`], pointing at the current exe.
pub fn unwrap(root: &Path, account: &str, appid: u32, game: &GameEntry, original: Option<&OriginalShortcut>) -> Result<()> {
    let exe = game
        .exe
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| original.map(|o| o.exe.clone()))
        .ok_or_else(|| anyhow!("不知道原来的游戏程序路径"))?;
    let start_dir = game
        .work_dir
        .clone()
        .or_else(|| game.exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf)))
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| original.map(|o| o.start_dir.clone()))
        .unwrap_or_default();
    let icon = original.map(|o| o.icon.clone()).unwrap_or_default();
    let args = game.args.clone();
    edit_shortcut(root, account, appid, |e| {
        e.set_str("Exe", &util::quote(&exe));
        e.set_str("StartDir", &util::quote(&start_dir));
        e.set_str("LaunchOptions", &args);
        e.set_str("icon", &if icon.is_empty() { String::new() } else { util::quote(&icon) });
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Health {
    /// Not linked to a Steam shortcut.
    NotLinked,
    /// Linked, auto-sync through Steam not enabled.
    Disabled,
    Ok,
    /// The shortcut no longer exists (maybe re-added with a new appid).
    ShortcutMissing { candidate_appid: Option<u32> },
    /// The shortcut was edited / recreated and no longer goes through the wrapper.
    Changed { exe: String, launch_options: String },
    /// It goes through a wrapper at another location (app moved/reinstalled).
    RunnerMoved { exe: String },
    /// Steam folder not found.
    NoSteam,
}

pub fn health(cfg: &AppConfig, game: &GameEntry) -> Health {
    let Some(link) = &game.steam else {
        return Health::NotLinked;
    };
    let Some(root) = root(cfg) else {
        return Health::NoSteam;
    };
    let shortcuts = match read_shortcuts(&root, &link.account) {
        Ok((_, s)) => s,
        Err(_) => return Health::NoSteam,
    };
    let Some(sc) = shortcuts.iter().find(|s| s.appid == link.appid) else {
        // Look for a shortcut pointing at the same game.
        let candidate = shortcuts
            .iter()
            .find(|s| {
                s.wrapped_key().as_deref() == Some(game.key.as_str())
                    || game
                        .exe
                        .as_ref()
                        .is_some_and(|e| util::same_path(&s.exe, &e.to_string_lossy()))
            })
            .map(|s| s.appid);
        return Health::ShortcutMissing { candidate_appid: candidate };
    };
    if !link.enabled {
        return Health::Disabled;
    }
    match sc.wrapped_key() {
        Some(k) if k == game.key => {
            if util::same_path(&sc.exe, &paths::runner_exe().to_string_lossy()) {
                Health::Ok
            } else {
                Health::RunnerMoved { exe: sc.exe.clone() }
            }
        }
        _ => Health::Changed {
            exe: sc.exe.clone(),
            launch_options: sc.launch_options.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SteamLink;

    const APPID: i32 = -1633758279;
    const EXE: &str = r"C:\Games\fefw\fefw.exe";

    fn s(v: &str) -> vdf::Value {
        vdf::Value::Str(v.as_bytes().to_vec())
    }

    fn shortcut_doc() -> vdf::Value {
        vdf::Value::Map(vec![(
            b"shortcuts".to_vec(),
            vdf::Value::Map(vec![(
                b"0".to_vec(),
                vdf::Value::Map(vec![
                    (b"appid".to_vec(), vdf::Value::Int(APPID)),
                    (b"AppName".to_vec(), s("火焰纹章")),
                    (b"Exe".to_vec(), s(&util::quote(EXE))),
                    (b"StartDir".to_vec(), s(&util::quote(r"C:\Games\fefw"))),
                    (b"icon".to_vec(), s("")),
                    (b"LaunchOptions".to_vec(), s("--fullscreen")),
                    (b"tags".to_vec(), vdf::Value::Map(vec![])),
                ]),
            )]),
        )])
    }

    #[test]
    fn wrap_health_unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
        let root = tmp.path().join("Steam");
        let cfgdir = root.join("userdata").join("123").join("config");
        std::fs::create_dir_all(&cfgdir).unwrap();
        std::fs::write(cfgdir.join("shortcuts.vdf"), vdf::write(&shortcut_doc())).unwrap();
        let runner = tmp.path().join("ludonimbus-run.exe");
        std::fs::write(&runner, b"x").unwrap();
        std::env::set_var("LUDONIMBUS_RUNNER", &runner);
        let appid = APPID as u32;

        let original = wrap(&root, "123", appid, "fe", &runner).unwrap();
        assert_eq!(original.exe, EXE);
        assert_eq!(original.launch_options, "--fullscreen");
        let sc = find_shortcut(&root, "123", appid).unwrap().unwrap();
        assert!(util::same_path(&sc.exe, &runner.to_string_lossy()));
        assert_eq!(sc.launch_options, "--game fe");
        assert_eq!(sc.wrapped_key().as_deref(), Some("fe"));
        assert_eq!(sc.name, "火焰纹章");
        assert!(util::same_path(&sc.icon, EXE));

        let mut cfg = AppConfig::default();
        cfg.steam.root = Some(root.clone());
        let mut game = GameEntry {
            key: "fe".into(),
            exe: Some(PathBuf::from(EXE)),
            args: "--fullscreen".into(),
            steam: Some(SteamLink {
                account: "123".into(),
                appid,
                enabled: true,
                original: Some(original.clone()),
            }),
            ..Default::default()
        };
        assert_eq!(health(&cfg, &game), Health::Ok);

        // The user points the shortcut at a new emulator build in Steam.
        edit_shortcut(&root, "123", appid, |e| e.set_str("Exe", &util::quote(r"D:\New\fefw.exe"))).unwrap();
        assert!(matches!(health(&cfg, &game), Health::Changed { ref exe, .. } if exe == r"D:\New\fefw.exe"));

        unwrap(&root, "123", appid, &game, Some(&original)).unwrap();
        let sc = find_shortcut(&root, "123", appid).unwrap().unwrap();
        assert_eq!(sc.exe, EXE);
        assert_eq!(sc.launch_options, "--fullscreen");
        game.steam.as_mut().unwrap().enabled = false;
        assert_eq!(health(&cfg, &game), Health::Disabled);
    }
}
