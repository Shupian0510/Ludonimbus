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
    #[cfg(windows)]
    {
        let fallback = PathBuf::from(r"C:\Program Files (x86)\Steam");
        fallback.is_dir().then_some(fallback)
    }
    #[cfg(not(windows))]
    {
        unix_roots().into_iter().find(|p| is_root(p))
    }
}

/// Where Steam keeps its files on Linux: the native install (and the
/// `~/.steam` links to it), the Flatpak and the Snap.
#[cfg(not(windows))]
fn unix_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let mut out = vec![
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join("snap/steam/common/.local/share/Steam"),
        home.join("Library/Application Support/Steam"),
    ];
    // The links point at one of the others: list each folder once.
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    out
}

/// A folder Steam is installed in (it holds `steam.exe`, or Steam's own
/// `config` and `steamapps` folders).
pub fn is_root(p: &Path) -> bool {
    p.join("steam.exe").is_file() || p.join("steam.sh").is_file() || (p.join("config").is_dir() && p.join("steamapps").is_dir())
}

/// The Steam install folder a picked folder is in: the folder itself or one
/// of its parents (someone may pick `Steam\steamapps` or `Steam\userdata`).
pub fn root_of(p: &Path) -> Option<PathBuf> {
    p.ancestors().find(|a| is_root(a)).map(Path::to_path_buf)
}

/// Every Steam install found here: the registry entries first, then the
/// usual folders on each drive.
pub fn find_roots() -> Vec<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let reg = [
            (HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath"),
            (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"),
            (HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam", "InstallPath"),
        ];
        for (hive, path, value) in reg {
            if let Ok(key) = winreg::RegKey::predef(hive).open_subkey(path) {
                if let Ok(p) = key.get_value::<String, _>(value) {
                    cands.push(PathBuf::from(p.replace('/', "\\")));
                }
            }
        }
    }
    for letter in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if !drive.is_dir() {
            continue;
        }
        for sub in [r"Program Files (x86)\Steam", r"Program Files\Steam", "Steam", r"Games\Steam", r"Program Files (x86)\Valve\Steam"] {
            cands.push(drive.join(sub));
        }
    }
    let mut seen = std::collections::HashSet::new();
    cands
        .into_iter()
        .filter(|p| is_root(p) && seen.insert(util::norm_path(&p.to_string_lossy())))
        .collect()
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
        self.wrapper().map(|(_, key)| key)
    }

    /// The wrapper this shortcut launches through and the game's key: the
    /// program itself (Windows), or `"<wrapper>" --game <key> --` before
    /// `%command%` in the launch options (Linux, so Proton keeps working;
    /// see [`wrapper_options`]).
    pub fn wrapper(&self) -> Option<(String, String)> {
        if is_runner(&self.exe) {
            let args = util::split_args(&self.launch_options);
            return args.iter().position(|a| a == "--game").and_then(|i| args.get(i + 1).cloned()).map(|k| (self.exe.clone(), k));
        }
        parse_wrapped(&self.launch_options).map(|w| (w.runner, w.key))
    }
}

/// A program that is a Ludonimbus launch wrapper.
fn is_runner(p: &str) -> bool {
    util::file_name_lower(Path::new(&util::native_sep(&util::unquote(p)))).starts_with("ludonimbus-run")
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
    let bytes = std::fs::read(&path).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", path.display()))?;
    let doc = vdf::parse(&bytes).with_context(|| crate::tr!("无法解析 {}", "Can't parse {}", path.display()))?;
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

/// Steam library folders (the install folder plus `libraryfolders.vdf`).
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let file = root.join("steamapps").join("libraryfolders.vdf");
    if let Some(doc) = std::fs::read_to_string(file).ok().and_then(|s| vdf::parse_text(&s).ok()) {
        for (_, lib) in doc.get("libraryfolders").map(|l| l.entries()).unwrap_or_default() {
            if let Some(p) = lib.get("path").and_then(vdf::Text::as_str) {
                let p = PathBuf::from(p);
                if !out.iter().any(|o| util::same_path(&o.to_string_lossy(), &p.to_string_lossy())) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// A store game installed through Steam.
#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub appid: u32,
    pub name: String,
    pub install_dir: PathBuf,
}

/// Store games installed in any Steam library (`appmanifest_<id>.acf`).
pub fn installed_apps(root: &Path) -> Vec<InstalledApp> {
    let mut out = Vec::new();
    for lib in library_folders(root) {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Some(doc) = std::fs::read_to_string(e.path()).ok().and_then(|s| vdf::parse_text(&s).ok()) else {
                continue;
            };
            let Some(state) = doc.get("AppState") else { continue };
            let field = |k: &str| state.get(k).and_then(vdf::Text::as_str).unwrap_or("").to_string();
            let Ok(appid) = field("appid").parse::<u32>() else { continue };
            out.push(InstalledApp {
                appid,
                name: field("name"),
                install_dir: apps.join("common").join(field("installdir")),
            });
        }
    }
    out
}

/// The id Steam derives for a non-Steam shortcut from its `Exe` field (as
/// stored, quotes included) and name. Grid artwork files are named after it.
pub fn shortcut_appid(exe_field: &str, name: &str) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(exe_field.as_bytes());
    h.update(name.as_bytes());
    h.finalize() | 0x8000_0000
}

/// Fields of a shortcut to create.
#[derive(Debug, Clone)]
pub struct NewShortcut {
    pub name: String,
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub icon: String,
}

/// Keep a copy of one of Steam's files before it is changed, named
/// `<name>-<time>.vdf` in [`paths::steam_backups_dir`].
fn back_up(path: &Path, name: &str) -> Result<()> {
    let dir = paths::steam_backups_dir();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    // Two writes within a millisecond each keep their backup.
    let mut backup = dir.join(format!("{name}-{stamp}.vdf"));
    let mut n = 2;
    while backup.exists() {
        backup = dir.join(format!("{name}-{stamp}-{n}.vdf"));
        n += 1;
    }
    std::fs::copy(path, &backup)?;
    Ok(())
}

fn save_doc(root: &Path, account: &str, doc: &vdf::Value) -> Result<()> {
    let path = shortcuts_path(root, account);
    if path.exists() {
        back_up(&path, &format!("shortcuts-{account}"))?;
    }
    util::atomic_write(&path, &vdf::write(doc))
}

/// Add a non-Steam shortcut and return its appid.
pub fn add_shortcut(root: &Path, account: &str, sc: &NewShortcut) -> Result<u32> {
    ensure_steam_closed(root)?;
    if !root.join("userdata").join(account).is_dir() {
        bail!(crate::tr!("找不到 Steam 账号 {account}", "Steam account {account} not found"));
    }
    let (mut doc, existing) = read_shortcuts(root, account)?;
    let exe_field = util::quote(&sc.exe);
    let mut appid = shortcut_appid(&exe_field, &sc.name);
    while existing.iter().any(|s| s.appid == appid) {
        appid = appid.wrapping_add(1) | 0x8000_0000;
    }
    if doc.get("shortcuts").is_none() {
        if let Some(e) = doc.entries_mut() {
            e.push((b"shortcuts".to_vec(), vdf::Value::Map(vec![])));
        }
    }
    let list = doc
        .get_mut("shortcuts")
        .and_then(vdf::Value::entries_mut)
        .ok_or_else(|| anyhow!(crate::tr!("shortcuts.vdf 格式异常", "shortcuts.vdf is malformed")))?;
    let index = list
        .iter()
        .filter_map(|(k, _)| std::str::from_utf8(k).ok()?.parse::<usize>().ok())
        .max()
        .map_or(0, |m| m + 1);
    let s = |v: &str| vdf::Value::Str(v.as_bytes().to_vec());
    let quoted = |v: &str| if v.is_empty() { s("") } else { s(&util::quote(v)) };
    list.push((
        index.to_string().into_bytes(),
        vdf::Value::Map(vec![
            (b"appid".to_vec(), vdf::Value::Int(appid as i32)),
            (b"AppName".to_vec(), s(&sc.name)),
            (b"Exe".to_vec(), s(&exe_field)),
            (b"StartDir".to_vec(), quoted(&sc.start_dir)),
            (b"icon".to_vec(), quoted(&sc.icon)),
            (b"ShortcutPath".to_vec(), s("")),
            (b"LaunchOptions".to_vec(), s(&sc.launch_options)),
            (b"IsHidden".to_vec(), vdf::Value::Int(0)),
            (b"AllowDesktopConfig".to_vec(), vdf::Value::Int(1)),
            (b"AllowOverlay".to_vec(), vdf::Value::Int(1)),
            (b"OpenVR".to_vec(), vdf::Value::Int(0)),
            (b"Devkit".to_vec(), vdf::Value::Int(0)),
            (b"DevkitGameID".to_vec(), s("")),
            (b"DevkitOverrideAppID".to_vec(), vdf::Value::Int(0)),
            (b"LastPlayTime".to_vec(), vdf::Value::Int(0)),
            (b"FlatpakAppID".to_vec(), s("")),
            (b"tags".to_vec(), vdf::Value::Map(vec![])),
        ]),
    ));
    save_doc(root, account, &doc)?;
    Ok(appid)
}

/// Remove a shortcut (and renumber the rest). `Ok(false)` when it was not there.
pub fn remove_shortcut(root: &Path, account: &str, appid: u32) -> Result<bool> {
    ensure_steam_closed(root)?;
    let (mut doc, existing) = read_shortcuts(root, account)?;
    if !existing.iter().any(|s| s.appid == appid) {
        return Ok(false);
    }
    let list = doc
        .get_mut("shortcuts")
        .and_then(vdf::Value::entries_mut)
        .ok_or_else(|| anyhow!(crate::tr!("shortcuts.vdf 格式异常", "shortcuts.vdf is malformed")))?;
    list.retain(|(_, v)| v.get("appid").and_then(vdf::Value::int).map(|i| i as u32) != Some(appid));
    for (i, (k, _)) in list.iter_mut().enumerate() {
        *k = i.to_string().into_bytes();
    }
    save_doc(root, account, &doc)?;
    for kind in ART_KINDS {
        remove_art(root, account, appid, kind);
    }
    Ok(true)
}

/// Artwork kinds: portrait cover, horizontal capsule, hero banner, logo, icon.
pub const ART_KINDS: [&str; 5] = ["cover", "wide", "hero", "logo", "icon"];
const ART_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

fn art_suffix(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "cover" => "p",
        "wide" => "",
        "hero" => "_hero",
        "logo" => "_logo",
        "icon" => "_icon",
        _ => return None,
    })
}

pub fn grid_dir(root: &Path, account: &str) -> PathBuf {
    root.join("userdata").join(account).join("config").join("grid")
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    pub cover: Option<PathBuf>,
    pub wide: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

pub fn art(root: &Path, account: &str, appid: u32) -> Art {
    let pick = |kind: &str| art_file(root, account, appid, kind);
    Art {
        cover: pick("cover"),
        wide: pick("wide"),
        hero: pick("hero"),
        logo: pick("logo"),
        icon: pick("icon"),
    }
}

pub fn art_file(root: &Path, account: &str, appid: u32, kind: &str) -> Option<PathBuf> {
    let suffix = art_suffix(kind)?;
    let grid = grid_dir(root, account);
    util::first_existing(ART_EXTS.iter().map(|ext| grid.join(format!("{appid}{suffix}.{ext}"))))
}

fn remove_art(root: &Path, account: &str, appid: u32, kind: &str) {
    if let Some(suffix) = art_suffix(kind) {
        let grid = grid_dir(root, account);
        for ext in ART_EXTS {
            let _ = std::fs::remove_file(grid.join(format!("{appid}{suffix}.{ext}")));
        }
    }
}

/// Image extension of a file (by name), defaulting to png.
pub fn image_ext(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("jpg") | Some("jpeg") => "jpg",
        Some("webp") => "webp",
        _ => "png",
    }
}

/// Copy an image into Steam's grid folder as the given kind of artwork.
/// Steam picks up new files after a restart. Returns the written path.
pub fn write_art(root: &Path, account: &str, appid: u32, kind: &str, src: &Path) -> Result<PathBuf> {
    let suffix = art_suffix(kind).ok_or_else(|| anyhow!(crate::tr!("未知的图片类型 {kind}", "Unknown image type {kind}")))?;
    let grid = grid_dir(root, account);
    std::fs::create_dir_all(&grid)?;
    remove_art(root, account, appid, kind);
    let dest = grid.join(format!("{appid}{suffix}.{}", image_ext(src)));
    std::fs::copy(src, &dest).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", dest.display()))?;
    Ok(dest)
}

/// Point a shortcut's icon at an image or exe.
pub fn set_shortcut_icon(root: &Path, account: &str, appid: u32, icon: &Path) -> Result<()> {
    let icon = icon.to_string_lossy().into_owned();
    edit_shortcut(root, account, appid, |e| e.set_str("icon", &util::quote(&icon)))
}

/// Steam's process, as [`process::process_names`] lists it.
pub const PROCESS: &str = if cfg!(windows) { "steam.exe" } else { "steam" };

pub fn is_running() -> bool {
    process::is_running(&[PROCESS.to_string()])
}

/// The command that starts Steam: `steam.exe` in its folder, or on Linux
/// the `steam` command (falling back to the folder's `steam.sh`).
fn steam_command(root: &Path) -> std::process::Command {
    if cfg!(windows) {
        return std::process::Command::new(root.join("steam.exe"));
    }
    let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("steam").is_file()));
    if on_path || !root.join("steam.sh").is_file() {
        std::process::Command::new("steam")
    } else {
        std::process::Command::new(root.join("steam.sh"))
    }
}

/// Start Steam again (after closing it to change its shortcuts).
pub fn launch(root: &Path) -> Result<()> {
    steam_command(root).spawn().map(|_| ()).with_context(|| crate::tr!("无法启动 Steam", "Couldn't start Steam"))
}

/// Ask Steam to exit and wait for it.
pub fn shutdown(root: &Path) -> Result<()> {
    if !is_running() {
        return Ok(());
    }
    steam_command(root)
        .arg("-shutdown")
        .spawn()
        .with_context(|| crate::tr!("无法关闭 Steam", "Couldn't close Steam"))?;
    let start = Instant::now();
    while is_running() {
        if start.elapsed() > Duration::from_secs(45) {
            bail!(crate::tr!("Steam 没有在 45 秒内退出，请手动关闭 Steam 后重试", "Steam didn't exit within 45 seconds. Close Steam yourself and try again"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // Give Steam a moment to finish writing its files.
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

/// Steam runs from `root`, and overwrites its files there when it exits.
/// Other folders (tests, copies) are never in use.
fn running_in(root: &Path) -> bool {
    // A folder picked in Settings may differ from the registry entry; it is
    // still the real Steam when it holds steam.exe.
    let live = root.join("steam.exe").is_file()
        || root.join("steam.sh").is_file()
        || detect_root().is_some_and(|r| util::same_path(&r.to_string_lossy(), &root.to_string_lossy()));
    live && !cfg!(test) && is_running()
}

/// A running Steam overwrites `shortcuts.vdf` in its own install folder, so
/// edits there need Steam closed.
fn ensure_steam_closed(root: &Path) -> Result<()> {
    if running_in(root) {
        bail!(crate::tr!("STEAM_RUNNING: 修改 Steam 快捷方式前需要先关闭 Steam（Steam 运行时会覆盖修改）", "STEAM_RUNNING: Steam needs to be closed before changing its shortcuts (it overwrites changes while running)"));
    }
    Ok(())
}

/// Steam writes `localconfig.vdf` (launch options among it) when it exits,
/// so changing a game's launch options needs Steam closed too.
pub(crate) fn ensure_closed_for_options(root: &Path) -> Result<()> {
    if running_in(root) {
        bail!(crate::tr!(
            "STEAM_RUNNING: 修改游戏的启动选项前需要先关闭 Steam（Steam 退出时会覆盖修改）",
            "STEAM_RUNNING: Steam needs to be closed before changing a game's launch options (it overwrites changes when it exits)"
        ));
    }
    Ok(())
}

/// Modify one shortcut entry and save `shortcuts.vdf` (with a backup copy).
fn edit_shortcut(root: &Path, account: &str, appid: u32, f: impl FnOnce(&mut vdf::Value)) -> Result<()> {
    ensure_steam_closed(root)?;
    let (mut doc, _) = read_shortcuts(root, account)?;
    let list = doc
        .get_mut("shortcuts")
        .ok_or_else(|| anyhow!(crate::tr!("shortcuts.vdf 格式异常", "shortcuts.vdf is malformed")))?;
    let entry = list
        .entries_mut()
        .and_then(|e| {
            e.iter_mut()
                .map(|(_, v)| v)
                .find(|v| v.get("appid").and_then(vdf::Value::int).map(|i| i as u32) == Some(appid))
        })
        .ok_or_else(|| anyhow!(crate::tr!("在 Steam 中找不到这个快捷方式（appid {appid}）", "This shortcut wasn't found in Steam (appid {appid})")))?;
    f(entry);
    save_doc(root, account, &doc)
}

/// Point a shortcut at `ludonimbus-run --game <key>`. Returns the original fields.
pub fn wrap(root: &Path, account: &str, appid: u32, key: &str, runner: &Path) -> Result<OriginalShortcut> {
    if !runner.is_file() {
        bail!(crate::tr!("找不到启动包装器 {}", "The launch wrapper {} is missing", runner.display()));
    }
    let sc = find_shortcut(root, account, appid)?
        .ok_or_else(|| anyhow!(crate::tr!("在 Steam 中找不到这个快捷方式（appid {appid}）", "This shortcut wasn't found in Steam (appid {appid})")))?;
    let original = OriginalShortcut {
        exe: sc.exe.clone(),
        start_dir: sc.start_dir.clone(),
        launch_options: sc.launch_options.clone(),
        icon: sc.icon.clone(),
    };
    let runner_s = runner.to_string_lossy().into_owned();
    let icon = if sc.icon.trim().is_empty() { sc.exe.clone() } else { sc.icon.clone() };
    if cfg!(windows) {
        edit_shortcut(root, account, appid, |e| {
            e.set_str("Exe", &util::quote(&runner_s));
            e.set_str("LaunchOptions", &format!("--game {key}"));
            e.set_str("icon", &util::quote(&icon));
        })?;
    } else {
        // Proton runs `Exe`: it stays, the wrapper goes in front of %command%.
        let options = wrapper_options(runner, key, &sc.launch_options);
        edit_shortcut(root, account, appid, |e| e.set_str("LaunchOptions", &options))?;
    }
    Ok(original)
}

/// Launch options that run a game through the wrapper (a non-Steam
/// shortcut on Linux, a game Steam installed anywhere): Steam replaces
/// `%command%` with the whole command (Proton and the game's exe included).
///
/// The wrapper goes right before `%command%` and everything the user had
/// stays where it was: environment variables (`MANGOHUD=1 %command%`) and
/// programs that start the game (`gamemoderun %command%`) stay in front,
/// so Steam still reads them as such and they reach the game through the
/// wrapper. Put in front of them, the wrapper would get `MANGOHUD=1` as the
/// program to start. Options without `%command%` go after the command, as
/// Steam puts them. Where `%command%` isn't a word of its own (inside
/// quotes, say) the wrapper goes in front of everything, as older versions
/// put it; so it does on Windows, as it always has there, starting what the
/// user put before `%command%` inside the job it waits for. Options that go
/// through a wrapper already are taken apart first: never one wrapper
/// inside another.
pub fn wrapper_options(runner: &Path, key: &str, original: &str) -> String {
    let wrapper = format!("{} --game {key} --", util::quote(&runner.to_string_lossy()));
    let unwrapped = parse_wrapped(original).map(|w| w.original);
    let original = unwrapped.as_deref().unwrap_or(original).trim();
    let before_command = if cfg!(windows) { None } else { command_word(original) };
    if let Some(at) = before_command {
        format!("{}{wrapper} {}", &original[..at], &original[at..])
    } else if original.contains("%command%") {
        format!("{wrapper} {original}")
    } else if original.is_empty() {
        format!("{wrapper} %command%")
    } else {
        format!("{wrapper} %command% {original}")
    }
}

/// Where each word of launch options starts. A quoted part belongs to the
/// word it is in, spaces and all.
fn word_starts(s: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut quote = None;
    let mut after_space = true;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            after_space = false;
            continue;
        }
        if after_space && !c.is_whitespace() {
            starts.push(i);
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        }
        after_space = c.is_whitespace();
    }
    starts
}

/// Where `%command%` starts a word of launch options, outside quotes: the
/// place of the wrapper.
fn command_word(s: &str) -> Option<usize> {
    word_starts(s).into_iter().find(|&i| s[i..].starts_with("%command%"))
}

/// Launch options that start a game through a Ludonimbus wrapper
/// ([`wrapper_options`]), taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped {
    /// The wrapper, as written.
    pub runner: String,
    pub key: String,
    /// What the user had: [`wrapper_options`] of it gives the same launch
    /// options again. `-x` and `%command% -x` mean the same to Steam; the
    /// shorter is given.
    pub original: String,
}

/// Take launch options made by [`wrapper_options`] apart: which wrapper,
/// which game, and what the user had before. `None` for any other options.
///
/// The wrapper is the first word (what older versions wrote, and what
/// options with nothing before `%command%` look like), or the words right
/// before `%command%`. Either way the user's options are what is left
/// once the wrapper is taken out, byte for byte; with nothing before
/// `%command%` that is the shorter form ([`Wrapped::original`]).
pub fn parse_wrapped(options: &str) -> Option<Wrapped> {
    let s = options.trim();
    let (at, runner, key, rest) = word_starts(s)
        .into_iter()
        .take_while(|&i| !s[i..].starts_with("%command%"))
        .find_map(|i| wrapper_at(&s[i..]).map(|(runner, key, rest)| (i, runner, key, rest)))?;
    let original = if at > 0 {
        // The user's words before `%command%`, then `%command%` and on.
        format!("{}{}", &s[..at], rest.trim_start())
    } else {
        let command = rest.trim();
        match command.strip_prefix("%command%") {
            Some("") => String::new(),
            Some(more) if more.starts_with(char::is_whitespace) && !more.contains("%command%") => more.trim_start().to_string(),
            _ => command.to_string(),
        }
    };
    Some(Wrapped { runner, key, original })
}

/// `"<wrapper>" --game <key> --` at the start of `s`: the wrapper as
/// written, the key, and what follows.
fn wrapper_at(s: &str) -> Option<(String, String, &str)> {
    let (runner, rest) = match s.strip_prefix('"') {
        Some(r) => {
            let end = r.find('"')?;
            (&r[..end], &r[end + 1..])
        }
        None => s.split_at(s.find(char::is_whitespace).unwrap_or(s.len())),
    };
    if !is_runner(runner) {
        return None;
    }
    // `--game <key> --`, each apart from the next.
    fn word(s: &str) -> (&str, &str) {
        let s = s.trim_start();
        s.split_at(s.find(char::is_whitespace).unwrap_or(s.len()))
    }
    let (flag, rest) = word(rest);
    let (key, rest) = word(rest);
    let (dashes, rest) = word(rest);
    if flag != "--game" || key.is_empty() || dashes != "--" {
        return None;
    }
    Some((runner.to_string(), key.to_string(), rest))
}

/// Restore the shortcut fields saved by [`wrap`], pointing at the current exe.
pub fn unwrap(root: &Path, account: &str, appid: u32, game: &GameEntry, original: Option<&OriginalShortcut>) -> Result<()> {
    let exe = game
        .exe
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| original.map(|o| o.exe.clone()))
        .ok_or_else(|| anyhow!(crate::tr!("不知道原来的游戏程序路径", "The original program path is unknown")))?;
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

/// The game behind a Steam app id: the one linked to that shortcut, or the
/// one Steam installed under that id. The one of `account` when it has one,
/// else of any account.
pub fn game_of_app<'a>(cfg: &'a AppConfig, appid: u32, account: Option<&str>) -> Option<&'a GameEntry> {
    fn account_of(g: &GameEntry, appid: u32) -> Option<&str> {
        let shortcut = g.steam.as_ref().filter(|l| l.appid == appid).map(|l| l.account.as_str());
        shortcut.or_else(|| g.store.as_ref().filter(|l| l.appid == appid).map(|l| l.account.as_str()))
    }
    let mut any = None;
    for g in &cfg.games {
        match account_of(g, appid) {
            Some(a) if account.is_none_or(|want| want == a) => return Some(g),
            Some(_) if any.is_none() => any = Some(g),
            _ => {}
        }
    }
    any
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
    match sc.wrapper() {
        Some((runner, k)) if k == game.key => {
            if util::same_path(&runner, &paths::runner_exe().to_string_lossy()) {
                Health::Ok
            } else {
                Health::RunnerMoved { exe: runner }
            }
        }
        _ => Health::Changed {
            exe: sc.exe.clone(),
            launch_options: sc.launch_options.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// Games installed through Steam: launch options and Steam Cloud
// (docs/plan-v2.md, section 8)

/// Steam's settings of one account on this PC: among them each game's
/// launch options and how its last Steam Cloud sync went. Steam writes the
/// file when it exits, so it is changed only while Steam is closed.
pub fn localconfig_path(root: &Path, account: &str) -> PathBuf {
    root.join("userdata").join(account).join("config").join("localconfig.vdf")
}

/// The account's settings Steam keeps in its own cloud, the Steam Cloud
/// switches among them.
pub fn sharedconfig_path(root: &Path, account: &str) -> PathBuf {
    root.join("userdata").join(account).join("7").join("remote").join("sharedconfig.vdf")
}

/// Where the games' blocks are in `localconfig.vdf`.
const LOCAL_APPS: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];
/// Where Steam's own settings are in `sharedconfig.vdf`.
const SHARED_STEAM: [&str; 4] = ["UserRoamingConfigStore", "Software", "Valve", "Steam"];

fn read_text(path: &Path) -> Option<vdf::Text> {
    std::fs::read_to_string(path).ok().and_then(|s| vdf::parse_text(&s).ok())
}

/// What Steam's files say about Steam Cloud and one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCloud {
    /// How the last sync went (`last_sync_state`: `synchronized`,
    /// `changesincloud`, …); `None` when Steam has noted nothing.
    pub sync_state: Option<String>,
    /// Steam has synced the game with Steam Cloud on this device: its
    /// settings have a `cloud` block for the game, or it keeps the list of
    /// the game's files in the cloud (`remotecache.vdf`). A game without
    /// Steam Cloud has neither. Assumed when Steam's settings can't be read.
    pub used: bool,
    /// The game's own switch ("Keep game saves in the Steam Cloud").
    pub enabled: bool,
    /// The account's switch for every game.
    pub global: bool,
}

impl Default for AppCloud {
    /// What is assumed without proof: Steam Cloud is on.
    fn default() -> Self {
        AppCloud { sync_state: None, used: true, enabled: true, global: true }
    }
}

impl AppCloud {
    /// Steam's last sync of the game left nothing behind on either side.
    pub fn synchronized(&self) -> bool {
        self.sync_state.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("synchronized"))
    }

    /// Steam Cloud syncs the game's saves.
    pub fn on(&self) -> bool {
        self.used && self.enabled && self.global
    }
}

/// Steam Cloud and the game `appid`, as Steam's files say. A switch counts
/// as off only when it is written down as off: a missing file or key is on.
/// A game counts as one Steam Cloud doesn't keep only when the account's
/// settings were read and say nothing of a cloud sync of it.
pub fn app_cloud(root: &Path, account: &str, appid: u32) -> AppCloud {
    let id = appid.to_string();
    let local = read_text(&localconfig_path(root, account));
    let block = local.as_ref().and_then(|d| LOCAL_APPS.iter().copied().chain([id.as_str(), "cloud"]).try_fold(d, |t, k| t.get(k)));
    let sync_state = block.and_then(|c| c.get("last_sync_state")).and_then(vdf::Text::as_str).map(str::to_string);
    let used = local.is_none() || block.is_some() || root.join("userdata").join(account).join(&id).join("remotecache.vdf").is_file();
    let shared = read_text(&sharedconfig_path(root, account));
    let steam = shared.as_ref().and_then(|d| SHARED_STEAM.iter().try_fold(d, |t, k| t.get(k)));
    AppCloud { sync_state, used, enabled: !game_cloud_off(steam, appid), global: !global_cloud_off(steam) }
}

/// Whether Steam Cloud is on for the account as a whole (its switch for
/// every game). On unless written down as off.
pub fn cloud_on_for_account(root: &Path, account: &str) -> bool {
    let shared = read_text(&sharedconfig_path(root, account));
    !global_cloud_off(shared.as_ref().and_then(|d| SHARED_STEAM.iter().try_fold(d, |t, k| t.get(k))))
}

fn global_cloud_off(shared_steam: Option<&vdf::Text>) -> bool {
    shared_steam.and_then(|s| s.get("CloudEnabled")).and_then(vdf::Text::as_str) == Some("0")
}

/// Whether the game's own Steam Cloud switch is written down as off.
///
/// Not verified: where Steam keeps this switch (docs/plan-v2.md, section
/// 15). The one place known so far is `apps > <appid> > CloudEnabled` in
/// the Steam block of `sharedconfig.vdf`; another place goes here.
fn game_cloud_off(shared_steam: Option<&vdf::Text>, appid: u32) -> bool {
    shared_steam.and_then(|s| vdf::text_value(s, &["apps", &appid.to_string(), "CloudEnabled"])) == Some("0")
}

fn launch_options_path(appid: &str) -> Vec<&str> {
    LOCAL_APPS.iter().copied().chain([appid, "LaunchOptions"]).collect()
}

/// The launch options of a game installed through Steam; `None` when it has
/// none.
pub fn launch_options(root: &Path, account: &str, appid: u32) -> Result<Option<String>> {
    let path = localconfig_path(root, account);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", path.display()))?;
    let id = appid.to_string();
    Ok(vdf::text_value(&vdf::parse_text(&text)?, &launch_options_path(&id)).map(str::to_string))
}

/// `localconfig.vdf` before and after setting the launch options of a game.
fn edit_launch_options(root: &Path, account: &str, appid: u32, options: Option<&str>) -> Result<(PathBuf, String, String)> {
    let path = localconfig_path(root, account);
    let before = std::fs::read_to_string(&path).with_context(|| crate::tr!("找不到这个 Steam 账号的设置文件 {}", "The settings of this Steam account weren't found: {}", path.display()))?;
    let id = appid.to_string();
    let after = vdf::set_text_value(&before, &launch_options_path(&id), options).with_context(|| crate::tr!("无法修改 {}", "Can't change {}", path.display()))?;
    Ok((path, before, after))
}

/// Whether the launch options of the game can be changed: Steam's file is
/// there and Ludonimbus understands it. Changes nothing.
pub fn can_set_launch_options(root: &Path, account: &str, appid: u32) -> Result<()> {
    edit_launch_options(root, account, appid, Some("")).map(|_| ())
}

/// Set the launch options of a game installed through Steam (`None`: none).
/// Only that one value of `localconfig.vdf` changes; every other byte stays
/// as Steam wrote it. Needs Steam closed. A copy of the file is kept first,
/// and the file is read back afterwards: when it doesn't hold just what was
/// written, it is put back as it was and this fails.
pub fn set_launch_options(root: &Path, account: &str, appid: u32, options: Option<&str>) -> Result<()> {
    ensure_closed_for_options(root)?;
    let (path, before, after) = edit_launch_options(root, account, appid, options)?;
    if after == before {
        return Ok(());
    }
    back_up(&path, &format!("localconfig-{account}"))?;
    util::atomic_write(&path, after.as_bytes())?;
    let id = appid.to_string();
    let written = std::fs::read_to_string(&path).ok().filter(|t| vdf::text_well_formed(t)).and_then(|t| vdf::parse_text(&t).ok());
    let holds = written.as_ref().is_some_and(|d| vdf::text_value(d, &launch_options_path(&id)) == options);
    if !holds {
        let _ = util::atomic_write(&path, before.as_bytes());
        bail!(crate::tr!(
            "修改启动选项后读回的结果不对，已恢复原来的 {}",
            "The launch options didn't read back as written; {} was put back as it was",
            path.display()
        ));
    }
    Ok(())
}

/// Whether a game's launch options still go through this installation's
/// wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WrapperHealth {
    Ok,
    /// The game has no launch options any more.
    Missing,
    /// They were changed and no longer go through the wrapper for this game.
    Changed,
    /// They go through a wrapper at another place (app moved or reinstalled).
    RunnerMoved,
}

/// [`WrapperHealth`] of the launch options `options` of the game `key`.
pub fn wrapper_health(options: Option<&str>, key: &str, runner: &Path) -> WrapperHealth {
    let Some(options) = options.filter(|o| !o.trim().is_empty()) else { return WrapperHealth::Missing };
    match parse_wrapped(options) {
        Some(w) if w.key == key && util::same_path(&w.runner, &runner.to_string_lossy()) => WrapperHealth::Ok,
        Some(w) if w.key == key => WrapperHealth::RunnerMoved,
        _ => WrapperHealth::Changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SteamLink, StoreLink};

    const APPID: i32 = -1633758279;

    #[test]
    fn a_steam_app_id_finds_its_game() {
        let shortcut = |key: &str, account: &str, appid: u32| GameEntry {
            key: key.into(),
            steam: Some(SteamLink { account: account.into(), appid, ..Default::default() }),
            ..Default::default()
        };
        let installed = |key: &str, account: &str, appid: u32| GameEntry {
            key: key.into(),
            store: Some(StoreLink { account: account.into(), appid, ..Default::default() }),
            ..Default::default()
        };
        let mut cfg = AppConfig::default();
        cfg.games = vec![shortcut("a", "111", 0x8000_0001), shortcut("b", "222", 0x8000_0001), installed("c", "111", 646570), installed("d", "222", 646570)];
        let key = |appid, account| game_of_app(&cfg, appid, account).map(|g| g.key.as_str());
        // A shortcut's id, and the id of a game Steam installed.
        assert_eq!(key(0x8000_0001, Some("222")), Some("b"));
        assert_eq!(key(646570, Some("222")), Some("d"));
        // No account given, or one that has none: the first.
        assert_eq!(key(0x8000_0001, None), Some("a"));
        assert_eq!(key(646570, Some("333")), Some("c"));
        assert_eq!(key(413150, None), None);
    }
    const EXE: &str = r"C:\Games\fefw\fefw.exe";

    #[test]
    fn picked_folder_resolves_to_steam_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        std::fs::create_dir_all(root.join("steamapps").join("common")).unwrap();
        assert!(root_of(&root).is_none());
        std::fs::write(root.join("steam.exe"), b"").unwrap();
        assert_eq!(root_of(&root).as_deref(), Some(root.as_path()));
        assert_eq!(root_of(&root.join("steamapps").join("common")).as_deref(), Some(root.as_path()));
        assert!(root_of(dir.path()).is_none());
    }

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
    fn wrapper_options_keep_the_users() {
        let r = Path::new("/home/deck/.config/Ludonimbus/bin/ludonimbus-run");
        let w = "\"/home/deck/.config/Ludonimbus/bin/ludonimbus-run\" --game fe --";
        // What the user had, the launch options through the wrapper, and
        // what taking the wrapper out gives back.
        let table: &[(&str, String, &str)] = &[
            ("", format!("{w} %command%"), ""),
            ("-novid -windowed", format!("{w} %command% -novid -windowed"), "-novid -windowed"),
            ("%command% -novid", format!("{w} %command% -novid"), "-novid"),
            // Environment variables and programs that start the game stay in
            // front of the wrapper: Steam reads them, and they reach the game.
            ("MANGOHUD=1 %command%", format!("MANGOHUD=1 {w} %command%"), "MANGOHUD=1 %command%"),
            ("gamemoderun %command%", format!("gamemoderun {w} %command%"), "gamemoderun %command%"),
            ("gamemoderun %command% -x", format!("gamemoderun {w} %command% -x"), "gamemoderun %command% -x"),
            (
                "PROTON_LOG=1 DXVK_HUD=fps gamemoderun mangohud %command% -fullscreen",
                format!("PROTON_LOG=1 DXVK_HUD=fps gamemoderun mangohud {w} %command% -fullscreen"),
                "PROTON_LOG=1 DXVK_HUD=fps gamemoderun mangohud %command% -fullscreen",
            ),
            (r#"WINEDLLOVERRIDES="dxgi=n,b" %command%"#, format!(r#"WINEDLLOVERRIDES="dxgi=n,b" {w} %command%"#), r#"WINEDLLOVERRIDES="dxgi=n,b" %command%"#),
            // Spacing stays as it was; only the ends are trimmed.
            ("  MANGOHUD=1\t %command%  ", format!("MANGOHUD=1\t {w} %command%"), "MANGOHUD=1\t %command%"),
            // `%command%` inside quotes isn't a word of its own: the wrapper
            // goes in front of everything.
            (r#"bash -c "exec %command%""#, format!(r#"{w} bash -c "exec %command%""#), r#"bash -c "exec %command%""#),
        ];
        for (original, wrapped, back) in table {
            // Windows keeps the wrapper in front of everything.
            let made = if cfg!(windows) && original.contains("%command%") { format!("{w} {}", original.trim()) } else { wrapped.clone() };
            assert_eq!(wrapper_options(r, "fe", original), made, "{original}");
            // Either way it comes apart again, on every system.
            for options in [wrapped, &made] {
                let parsed = parse_wrapped(options).unwrap_or_else(|| panic!("{options}"));
                assert_eq!((parsed.runner.as_str(), parsed.key.as_str(), parsed.original.as_str()), (r.to_str().unwrap(), "fe", *back), "{options}");
                let sc = Shortcut {
                    index: "0".into(),
                    appid: 1,
                    name: "x".into(),
                    exe: "/games/fe.exe".into(),
                    start_dir: String::new(),
                    icon: String::new(),
                    launch_options: options.clone(),
                    last_play_time: 0,
                };
                assert_eq!(sc.wrapper(), Some((r.to_string_lossy().into_owned(), "fe".to_string())), "{options}");
            }
            // Wrapping what came back gives the same options again.
            assert_eq!(wrapper_options(r, "fe", back), made);
        }
    }

    #[test]
    fn launch_options_of_older_versions_are_recognised() {
        // Older versions put the wrapper in front of everything.
        let old = "\"/opt/ludonimbus/ludonimbus-run\" --game fe --";
        for (options, original) in [
            (format!("{old} %command%"), ""),
            (format!("{old} %command% -x"), "-x"),
            (format!("{old} gamemoderun %command% -x"), "gamemoderun %command% -x"),
            (format!("{old} MANGOHUD=1 %command%"), "MANGOHUD=1 %command%"),
            (format!("{old} PROTON_LOG=1 gamemoderun %command%"), "PROTON_LOG=1 gamemoderun %command%"),
        ] {
            let w = parse_wrapped(&options).unwrap_or_else(|| panic!("{options}"));
            assert_eq!((w.runner.as_str(), w.key.as_str(), w.original.as_str()), ("/opt/ludonimbus/ludonimbus-run", "fe", original), "{options}");
            // Wrapped again (a moved wrapper, taken over again): the wrapper
            // where it goes now, and never one inside another.
            let new = Path::new("/home/deck/.config/Ludonimbus/bin/ludonimbus-run");
            let again = wrapper_options(new, "fe", &options);
            assert_eq!(again, wrapper_options(new, "fe", original), "{options}");
            assert_eq!(again.matches("ludonimbus-run").count(), 1, "{again}");
        }
    }

    #[test]
    fn wrapped_launch_options_come_apart_again() {
        let r = Path::new(r"C:\Program Files\Ludonimbus\ludonimbus-run.exe");
        for original in ["", "-novid", "gamemoderun %command% -x", r#"-config "C:\My Games\a.cfg""#, "  -windowed  "] {
            let options = wrapper_options(r, "hollow-knight", original);
            let w = parse_wrapped(&options).unwrap_or_else(|| panic!("{options}"));
            assert_eq!(w.runner, r.to_string_lossy());
            assert_eq!(w.key, "hollow-knight");
            // What the user had, or what means the same to Steam.
            assert_eq!(wrapper_options(r, "hollow-knight", &w.original), options);
        }
        assert_eq!(parse_wrapped(&wrapper_options(r, "k", "-novid")).unwrap().original, "-novid");
        assert_eq!(parse_wrapped(&wrapper_options(r, "k", "%command% -novid")).unwrap().original, "-novid");
        assert_eq!(parse_wrapped("/opt/ludonimbus-run --game k -- %command%").unwrap().original, "");
        for other in ["", "-novid", "\"C:\\other.exe\" --game k -- %command%", "\"C:\\ludonimbus-run.exe\" --game", "\"C:\\ludonimbus-run.exe\" --game k %command%"] {
            assert_eq!(parse_wrapped(other), None, "{other}");
        }
        let health = |o: Option<&str>| wrapper_health(o, "k", r);
        assert_eq!(health(Some(&wrapper_options(r, "k", "-x"))), WrapperHealth::Ok);
        assert_eq!(health(Some(&wrapper_options(Path::new(r"D:\Old\ludonimbus-run.exe"), "k", ""))), WrapperHealth::RunnerMoved);
        assert_eq!(health(Some(&wrapper_options(r, "other", ""))), WrapperHealth::Changed);
        assert_eq!(health(Some("-novid")), WrapperHealth::Changed);
        assert_eq!(health(None), WrapperHealth::Missing);
        assert_eq!(health(Some(" ")), WrapperHealth::Missing);
    }

    /// A Steam folder with one account and the two settings files.
    fn steam_files(local: &str, shared: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Steam");
        let local_path = localconfig_path(&root, "111111111");
        std::fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        std::fs::write(&local_path, local).unwrap();
        if let Some(shared) = shared {
            let p = sharedconfig_path(&root, "111111111");
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, shared).unwrap();
        }
        (tmp, root)
    }

    fn local_app(app: &str, eol: &str) -> String {
        format!(
            "\"UserLocalConfigStore\"\n{{\n\t\"Software\"\n\t{{\n\t\t\"Valve\"\n\t\t{{\n\t\t\t\"Steam\"\n\t\t\t{{\n\t\t\t\t\"apps\"\n\t\t\t\t{{\n{app}\t\t\t\t}}\n\t\t\t}}\n\t\t}}\n\t}}\n}}\n"
        )
        .replace('\n', eol)
    }

    #[test]
    fn steam_cloud_switches_are_on_unless_written_down_as_off() {
        let app = "\t\t\t\t\t\"1000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"cloud\"\n\t\t\t\t\t\t{\n\t\t\t\t\t\t\t\"last_sync_state\"\t\t\"changesincloud\"\n\t\t\t\t\t\t}\n\t\t\t\t\t}\n";
        // No sharedconfig.vdf: on.
        let (_t, root) = steam_files(&local_app(app, "\r\n"), None);
        let c = app_cloud(&root, "111111111", 1000);
        assert_eq!(c, AppCloud { sync_state: Some("changesincloud".into()), used: true, enabled: true, global: true });
        assert!(!c.synchronized() && c.on());
        // A game Steam's settings say nothing of a cloud sync of: Steam
        // Cloud doesn't keep it. Until Steam keeps its list of cloud files.
        let other = app_cloud(&root, "111111111", 2000);
        assert_eq!(other, AppCloud { sync_state: None, used: false, enabled: true, global: true });
        assert!(!other.on());
        let cache = root.join("userdata").join("111111111").join("2000").join("remotecache.vdf");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, "\"2000\"\n{\n}\n").unwrap();
        assert!(app_cloud(&root, "111111111", 2000).on());
        // Another account, no Steam at all: on, nothing known.
        assert_eq!(app_cloud(&root, "222222222", 1000), AppCloud::default());
        assert!(AppCloud::default().on());

        let shared = |steam: &str| format!("\"UserRoamingConfigStore\"\n{{\n\t\"software\"\n\t{{\n\t\t\"valve\"\n\t\t{{\n\t\t\t\"Steam\"\n\t\t\t{{\n{steam}\t\t\t}}\n\t\t}}\n\t}}\n}}\n");
        let synced = app.replace("changesincloud", "synchronized");
        // The global switch, in other key case; the game's switch.
        let (_t, root) = steam_files(&local_app(&synced, "\n"), Some(&shared("\t\t\t\t\"CloudEnabled\"\t\t\"0\"\n")));
        let c = app_cloud(&root, "111111111", 1000);
        assert!(c.synchronized() && c.enabled && !c.global && !c.on());
        let (_t, root) = steam_files(&local_app(&synced, "\n"), Some(&shared("\t\t\t\t\"CloudEnabled\"\t\t\"1\"\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"1000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"cloudenabled\"\t\t\"0\"\n\t\t\t\t\t}\n\t\t\t\t}\n")));
        let c = app_cloud(&root, "111111111", 1000);
        assert!(!c.enabled && c.global && !c.on());
        assert!(!app_cloud(&root, "111111111", 2000).used);
    }

    #[test]
    fn wrap_health_unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = paths::TestEnv::new();
        env.set("LUDONIMBUS_HOME", tmp.path().join("home"));
        let root = tmp.path().join("Steam");
        let cfgdir = root.join("userdata").join("123").join("config");
        std::fs::create_dir_all(&cfgdir).unwrap();
        std::fs::write(cfgdir.join("shortcuts.vdf"), vdf::write(&shortcut_doc())).unwrap();
        let runner = tmp.path().join("ludonimbus-run.exe");
        std::fs::write(&runner, b"x").unwrap();
        env.set("LUDONIMBUS_RUNNER", &runner);
        let appid = APPID as u32;

        let original = wrap(&root, "123", appid, "fe", &runner).unwrap();
        assert_eq!(original.exe, EXE);
        assert_eq!(original.launch_options, "--fullscreen");
        let sc = find_shortcut(&root, "123", appid).unwrap().unwrap();
        if cfg!(windows) {
            assert!(util::same_path(&sc.exe, &runner.to_string_lossy()));
            assert_eq!(sc.launch_options, "--game fe");
        } else {
            // Linux: the program stays (Proton runs it); the wrapper goes in front of %command%.
            assert_eq!(sc.exe, EXE);
            assert_eq!(sc.launch_options, format!("{} --game fe -- %command% --fullscreen", util::quote(&runner.to_string_lossy())));
        }
        assert_eq!(sc.wrapped_key().as_deref(), Some("fe"));
        assert_eq!(sc.name, "火焰纹章");
        // Windows keeps the game's icon on the wrapper.
        if cfg!(windows) {
            assert!(util::same_path(&sc.icon, EXE));
        }

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
                created: false,
            }),
            ..Default::default()
        };
        assert_eq!(health(&cfg, &game), Health::Ok);

        // The user points the shortcut at a new emulator build in Steam
        // (on Linux: removes the wrapper from the launch options).
        if cfg!(windows) {
            edit_shortcut(&root, "123", appid, |e| e.set_str("Exe", &util::quote(r"D:\New\fefw.exe"))).unwrap();
            assert!(matches!(health(&cfg, &game), Health::Changed { ref exe, .. } if exe == r"D:\New\fefw.exe"));
        } else {
            edit_shortcut(&root, "123", appid, |e| e.set_str("LaunchOptions", "--fullscreen")).unwrap();
            assert!(matches!(health(&cfg, &game), Health::Changed { .. }));
        }

        unwrap(&root, "123", appid, &game, Some(&original)).unwrap();
        let sc = find_shortcut(&root, "123", appid).unwrap().unwrap();
        assert_eq!(sc.exe, EXE);
        assert_eq!(sc.launch_options, "--fullscreen");
        game.steam.as_mut().unwrap().enabled = false;
        assert_eq!(health(&cfg, &game), Health::Disabled);
    }
}
