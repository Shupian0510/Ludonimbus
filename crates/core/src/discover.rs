//! Finding games on this PC: Steam non-Steam shortcuts (all accounts), Epic,
//! GOG and Xbox app installs, games installed with RecompLauncher, games
//! with saves in emulators' data folders, games Ludusavi finds saves for,
//! and games in a folder the user picks. Results from different sources can
//! describe the same game; `match_key`, `exe` and (for emulator games) the
//! rule and save patterns let the caller merge them.

use crate::adapters::Adapter;
use crate::config::{AppConfig, GameEntry};
use crate::{adapters, emugames, games, ludusavi, paths, recomplauncher, snapshot, steam, util, xbox};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Found {
    /// Unique within one scan, e.g. `steam:123:4000000000`.
    pub id: String,
    pub name: String,
    /// `steam`, `epic`, `gog`, `xbox`, `recomplauncher`, `emulator` (saves in
    /// an emulator's data folder), `ludusavi` or `folder`.
    #[ts(type = r#""steam" | "epic" | "gog" | "xbox" | "recomplauncher" | "emulator" | "ludusavi" | "folder""#)]
    pub source: String,
    pub exe: Option<String>,
    pub install: Option<String>,
    pub args: String,
    pub steam_account: Option<String>,
    pub steam_appid: Option<u32>,
    pub adapter: Option<String>,
    pub adapter_name: Option<String>,
    /// Save patterns the scan worked out: one emulator game's saves, or a
    /// save folder picked in RecompLauncher (empty: the rule's or unknown).
    pub saves: Vec<String>,
    pub ludusavi_title: Option<String>,
    /// Save files found on this PC (0 when unknown).
    pub save_files: usize,
    pub save_bytes: u64,
    pub save_sample: Vec<String>,
    /// Steam Cloud already keeps these saves in sync.
    pub steam_cloud: bool,
    /// A store game installed through Steam.
    pub store_game: bool,
    /// Its app id in Steam, when it is known which installed game it is:
    /// such a game is added as one Steam installed ([`crate::steamcloud`]).
    pub store_appid: Option<u32>,
    /// Key of the library entry for this game, when already added.
    pub added_key: Option<String>,
    /// Saves only in the registry (not supported yet).
    pub registry_only: bool,
    /// Where the install was found for a game known by its saves ("epic",
    /// "gog", "xbox", "registry", "folder").
    pub install_store: Option<String>,
    /// Normalized name for matching results from different sources.
    pub match_key: String,
}

/// Lowercase letters and digits (any script) only: "Hollow Knight™" →
/// "hollowknight".
pub fn match_key(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// One of Ludonimbus's own programs, never a game, though Steam lists one
/// once the handheld screen is added to it: the app (`ludonimbus-app`, the
/// Linux `Ludonimbus*.AppImage`), the launch wrapper (`ludonimbus-run`), the
/// background service (`ludonimbus-agent`), the command line
/// (`ludonimbus`), and anything in the folder Linux keeps the wrapper and
/// service in (`~/.config/Ludonimbus/bin`).
pub fn is_own_program(path: &Path) -> bool {
    let p = PathBuf::from(util::native_sep(&util::unquote(&path.to_string_lossy())));
    if util::file_name_lower(&p).starts_with("ludonimbus") {
        return true;
    }
    let bin = crate::paths::data_dir().join("bin");
    p.parent().is_some_and(|d| util::same_path(&d.to_string_lossy(), &bin.to_string_lossy()))
}

fn found(source: &str, id: String, name: String) -> Found {
    Found { match_key: match_key(&name), id, name, source: source.into(), ..Default::default() }
}

/// Fast sources: Steam shortcuts, Epic, GOG, Xbox app, RecompLauncher, and
/// games with saves in emulators' data folders. Names are looked up in
/// Ludusavi's database for games without a built-in rule.
pub fn quick(cfg: &AppConfig) -> Vec<Found> {
    let mut out = steam_shortcuts(cfg);
    let mut seen: HashSet<String> = out.iter().filter_map(|f| f.exe.as_deref().map(util::norm_path)).collect();
    // Games RecompLauncher already put in Steam (`--play <id>`).
    let in_steam: HashSet<String> = out
        .iter()
        .filter(|f| f.exe.as_deref().is_some_and(|e| recomplauncher::is_launcher(Path::new(e))))
        .filter_map(|f| recomplauncher::play_id(&f.args))
        .collect();
    let mut more = epic_games(&epic_manifest_dir());
    more.extend(gog_games());
    more.extend(xbox_games());
    more.extend(recomplauncher_games(None).into_iter().filter(|f| !in_steam.contains(f.id.trim_start_matches("recomplauncher:"))));
    for f in more {
        // The same game may already be in Steam as a non-Steam shortcut.
        if f.exe.as_deref().is_some_and(|e| !seen.insert(util::norm_path(e))) {
            continue;
        }
        out.push(f);
    }
    // Emulator games: in the data folders of emulators in Steam or the
    // library, and in each emulator's usual folders.
    // And emulators kept on another drive (a Steam Deck's SD card).
    let programs: Vec<PathBuf> = out
        .iter()
        .filter_map(|f| f.exe.as_deref().map(PathBuf::from))
        .chain(cfg.games.iter().filter_map(|g| g.exe.clone()))
        .chain(drive_programs(&paths::extra_drives()))
        .collect();
    push_new_emulator_games(&mut out, emulator_games(&programs, true));
    attach_ludusavi_titles(&mut out);
    mark_added(cfg, &mut out);
    out
}

/// Add emulator games that no result (e.g. a Steam shortcut that starts the
/// game) already stands for.
fn push_new_emulator_games(out: &mut Vec<Found>, games: Vec<Found>) {
    for f in games {
        if !out.iter().any(|o| !o.saves.is_empty() && o.adapter == f.adapter && o.saves == f.saves) {
            out.push(f);
        }
    }
}

/// Games with saves in emulators' data folders (see [`emugames`]):
/// one result per game, with that game's save patterns. `programs` are
/// exes on this PC (only emulators are used); `usual_places` also looks in
/// each emulator's default data folders.
pub fn emulator_games(programs: &[PathBuf], usual_places: bool) -> Vec<Found> {
    let mut places: Vec<(&'static Adapter, Option<PathBuf>, Option<PathBuf>)> = Vec::new();
    for exe in programs {
        let Some(d) = adapters::detect_with(exe, "") else { continue };
        if !d.adapter.is_emulator() || d.adapter.game_saves.is_empty() || !exe.is_file() {
            continue;
        }
        let install = d.install.clone().or_else(|| exe.parent().map(Path::to_path_buf));
        if !places.iter().any(|(a, e, _)| a.key == d.adapter.key && e.as_deref().is_some_and(|e| util::same_path(&e.to_string_lossy(), &exe.to_string_lossy())))
        {
            places.push((d.adapter, Some(exe.clone()), install));
        }
    }
    if usual_places {
        for a in adapters::builtin().iter().filter(|a| a.is_emulator() && !a.game_saves.is_empty()) {
            places.push((a, None, None));
        }
    }
    let mut out: Vec<Found> = Vec::new();
    for (a, exe, install) in places {
        for g in emugames::games(a, install.as_deref()) {
            let saves = g.game.saves(a);
            if out.iter().any(|f| f.adapter.as_deref() == Some(a.key.as_str()) && f.saves == saves) {
                continue;
            }
            let probe = GameEntry {
                key: "scan".into(),
                name: a.name.clone(),
                adapter: Some(a.key.clone()),
                exe: exe.clone(),
                install: install.clone(),
                saves: saves.clone(),
                ..Default::default()
            };
            let Ok((files, _)) = snapshot::scan(&probe) else { continue };
            if files.is_empty() {
                continue;
            }
            let label = adapters::label(a, Some(&g.game));
            let name = g.game.name.clone().unwrap_or_else(|| label.clone());
            let mut f = found("emulator", format!("emulator:{}:{}:{}", a.key, g.game.variant, g.game.id), name);
            f.adapter = Some(a.key.clone());
            f.adapter_name = Some(label);
            f.saves = saves;
            if let Some(e) = &exe {
                f.exe = Some(e.to_string_lossy().into_owned());
                f.install = install.as_ref().map(|p| p.to_string_lossy().into_owned());
                f.args = g.args.clone();
            }
            f.save_files = files.len();
            f.save_bytes = files.iter().map(|x| x.size).sum();
            f.save_sample = files.iter().take(5).map(|x| x.abs.to_string_lossy().into_owned()).collect();
            out.push(f);
        }
    }
    out
}

fn steam_shortcuts(cfg: &AppConfig) -> Vec<Found> {
    let Some(root) = steam::root(cfg) else { return Vec::new() };
    // The selected account first, so a game added to several accounts is
    // offered from that one.
    let mut accounts: Vec<String> = steam::accounts(&root).into_iter().map(|a| a.id).collect();
    if let Some(def) = steam::default_account(cfg, &root) {
        accounts.retain(|a| *a != def);
        accounts.insert(0, def);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for account in accounts {
        let Ok(list) = games::shortcut_candidates(cfg, &account) else { continue };
        for c in list {
            if c.wrapped_key.is_some() && c.added_key.is_none() {
                // Wrapped by a Ludonimbus installation that is not this one.
                continue;
            }
            // Ludonimbus itself (the handheld screen's shortcut).
            if c.wrapped_key.is_none() && is_own_program(Path::new(&c.exe)) {
                continue;
            }
            // One emulator starts many games: the launch options tell them apart.
            if !seen.insert(format!("{}\0{}", util::norm_path(&c.exe), c.launch_options.trim())) {
                continue;
            }
            let mut f = found("steam", format!("steam:{account}:{}", c.appid), c.suggested_name.clone());
            f.exe = Some(c.exe.clone());
            f.install = (!c.start_dir.is_empty()).then(|| c.start_dir.clone());
            f.args = c.launch_options.clone();
            f.steam_account = Some(account.clone());
            f.steam_appid = Some(c.appid);
            f.adapter = c.adapter.clone();
            f.adapter_name = c.adapter_name.clone();
            f.saves = c.saves.clone();
            f.added_key = c.added_key.clone();
            // An added shortcut may point at the wrapper: describe it from
            // the library entry instead.
            if let Some(g) = c.added_key.as_deref().and_then(|k| cfg.game(k)) {
                if f.adapter != g.adapter {
                    f.adapter_name = None;
                }
                f.adapter = g.adapter.clone();
                f.ludusavi_title = g.ludusavi_title.clone();
                f.name = g.name.clone();
            }
            out.push(f);
        }
    }
    out
}

fn epic_manifest_dir() -> PathBuf {
    let base = std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests")
}

/// Games installed by the Epic Games Launcher (`*.item` manifests).
pub fn epic_games(dir: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        if e.path().extension().is_none_or(|x| !x.eq_ignore_ascii_case("item")) {
            continue;
        }
        let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else {
            continue;
        };
        if v["bIsIncompleteInstall"].as_bool() == Some(true) {
            continue;
        }
        // Skip add-ons / DLC that share the main game's folder.
        if v["bIsApplication"].as_bool() == Some(false) {
            continue;
        }
        let name = v["DisplayName"].as_str().unwrap_or("").trim().to_string();
        let install = v["InstallLocation"].as_str().unwrap_or("").to_string();
        let launch = v["LaunchExecutable"].as_str().unwrap_or("").to_string();
        if name.is_empty() || install.is_empty() || launch.is_empty() {
            continue;
        }
        let exe = Path::new(&install).join(util::native_sep(&launch));
        let mut f = found("epic", format!("epic:{}", v["AppName"].as_str().unwrap_or(&name)), name);
        f.adapter = adapters::detect(&exe).map(|a| a.key.clone());
        f.exe = Some(exe.to_string_lossy().into_owned());
        f.install = Some(install);
        f.args = v["LaunchCommand"].as_str().unwrap_or("").trim().to_string();
        out.push(f);
    }
    out
}

/// Games installed with RecompLauncher (its `library.json`). `launcher` is
/// a portable RecompLauncher's exe, if known.
pub fn recomplauncher_games(launcher: Option<&Path>) -> Vec<Found> {
    let mut out = Vec::new();
    for g in recomplauncher::installed(launcher) {
        let Some(exe) = g.exe.as_ref().map(|rel| g.path.join(rel)).filter(|e| e.is_file()) else { continue };
        let adapter = adapters::find_recomplauncher(&g.id).or_else(|| adapters::detect(&exe));
        let name = adapter.map(|a| a.name.clone()).or_else(|| (!g.game.trim().is_empty()).then(|| g.game.clone())).unwrap_or_else(|| g.name.clone());
        let mut f = found("recomplauncher", format!("recomplauncher:{}", g.id), name);
        f.adapter = adapter.map(|a| a.key.clone());
        if adapter.is_none() {
            f.saves = g.save_place.iter().chain(&g.more_save_places).filter_map(|t| recomplauncher::convert_place(t)).collect();
        }
        f.exe = Some(exe.to_string_lossy().into_owned());
        f.install = Some(g.path.to_string_lossy().into_owned());
        out.push(f);
    }
    out
}

/// Games installed by GOG Galaxy / GOG installers (registry).
pub fn gog_games() -> Vec<Found> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let Ok(games) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\WOW6432Node\\GOG.com\\Games") else {
            return out;
        };
        for id in games.enum_keys().flatten() {
            let Ok(k) = games.open_subkey(&id) else { continue };
            let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            let (name, exe) = (get("gameName"), get("exe"));
            if name.is_empty() || exe.is_empty() || !Path::new(&exe).is_file() {
                continue;
            }
            let mut f = found("gog", format!("gog:{id}"), name);
            f.adapter = adapters::detect(Path::new(&exe)).map(|a| a.key.clone());
            f.install = Some(get("path")).filter(|p| !p.is_empty());
            f.args = get("launchParam");
            f.exe = Some(exe);
            out.push(f);
        }
    }
    out
}

/// Look up Ludusavi titles for games without a built-in rule (one call).
fn attach_ludusavi_titles(list: &mut [Found]) {
    let idx: Vec<usize> = (0..list.len()).filter(|&i| list[i].adapter.is_none() && list[i].saves.is_empty() && list[i].ludusavi_title.is_none()).collect();
    let names: Vec<String> = idx.iter().map(|&i| list[i].name.clone()).collect();
    if let Ok(titles) = ludusavi::find_titles(&names) {
        for (i, t) in idx.into_iter().zip(titles) {
            list[i].ludusavi_title = t;
        }
    }
    for f in list.iter_mut() {
        if f.adapter_name.is_some() {
            continue;
        }
        if let Some(a) = f.adapter.as_deref().and_then(adapters::find) {
            // Without a game id, an emulator's saves go together.
            let whole = a.is_emulator() && f.saves.is_empty();
            f.adapter_name = Some(if whole { adapters::label(a, None) } else { a.name.clone() });
        }
    }
}

/// Mark results that are already in the library.
pub fn mark_added(cfg: &AppConfig, list: &mut [Found]) {
    for f in list.iter_mut() {
        if f.added_key.is_some() {
            continue;
        }
        f.added_key = cfg
            .games
            .iter()
            .find(|g| {
                let same_exe = match (&f.exe, &g.exe) {
                    (Some(a), Some(b)) => util::same_path(a, &b.to_string_lossy()),
                    _ => false,
                };
                let same_link = match (&f.steam_appid, &g.steam) {
                    (Some(id), Some(l)) => l.appid == *id && f.steam_account.as_deref() == Some(l.account.as_str()),
                    _ => false,
                };
                let same_title = match (&f.ludusavi_title, &g.ludusavi_title) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    _ => false,
                };
                // Same emulator rule and the same saves (one game, or the
                // whole emulator), or one game of an emulator whose saves
                // all go together: already synced by that entry.
                let same_rule = f.adapter.is_some() && f.adapter == g.adapter && (f.saves == g.saves || g.saves.is_empty()) && {
                    let a = f.adapter.as_deref().and_then(adapters::find);
                    a.is_some_and(|a| a.is_emulator())
                };
                // The same Switch game, added in another emulator.
                let same_game = !f.saves.is_empty() && f.saves == g.saves && {
                    let (a, b) = (f.adapter.as_deref().and_then(adapters::find), g.adapter.as_deref().and_then(adapters::find));
                    a.zip(b).is_some_and(|(a, b)| a.shares_game_saves(b))
                };
                same_exe || same_link || same_title || same_rule || same_game
            })
            .map(|g| g.key.clone());
    }
}

/// Games Ludusavi finds saves for (about a minute). Games whose saves Steam
/// Cloud already syncs are flagged so the caller can tuck them away; for the
/// others the install is looked up (Epic, GOG, Xbox, registered programs,
/// games folders) so they can be launched and added to Steam.
pub fn ludusavi_scan(cfg: &AppConfig) -> Result<Vec<Found>> {
    let games = ludusavi::scan_all()?;
    let titles: HashSet<String> = games.iter().map(|g| g.title.clone()).collect();
    let manifest = ludusavi::manifest_info(&titles);
    let mut installed = SteamInstalled::default();
    let apps = steam::root(cfg).map(|r| steam::installed_apps(&r)).unwrap_or_default();
    for a in &apps {
        installed.ids.insert(a.appid);
        installed.names.insert(match_key(&a.name));
    }
    let mut out = classify(&games, &manifest, &installed);
    // Installed through Steam by name: which installed game it is.
    for f in out.iter_mut().filter(|f| f.store_game && f.store_appid.is_none()) {
        f.store_appid = apps.iter().find(|a| match_key(&a.name) == f.match_key).map(|a| a.appid);
    }
    let all = installs(cfg);
    for f in out.iter_mut().filter(|f| !f.steam_cloud && !f.store_game && !f.registry_only) {
        let title = f.ludusavi_title.clone().unwrap_or_default();
        let dirs = manifest.get(&title).map(|m| m.install_dirs.clone()).unwrap_or_default();
        if let Some((install, exe)) = locate(&all, &title, &dirs) {
            f.exe = Some(exe.to_string_lossy().into_owned());
            f.install = Some(install.dir.to_string_lossy().into_owned());
            f.install_store = Some(install.store);
        }
    }
    mark_added(cfg, &mut out);
    Ok(out)
}

/// Store games installed through Steam on this PC.
#[derive(Debug, Default)]
pub struct SteamInstalled {
    pub ids: HashSet<u32>,
    /// `match_key` of their names.
    pub names: HashSet<String>,
}

pub fn classify(games: &[ludusavi::ScanGame], manifest: &HashMap<String, ludusavi::ManifestInfo>, steam_installed: &SteamInstalled) -> Vec<Found> {
    games
        .iter()
        .map(|g| {
            let info = manifest.get(&g.title).cloned().unwrap_or_default();
            let store_appid = info.steam_id.iter().chain(&info.steam_extra).copied().find(|id| steam_installed.ids.contains(id));
            let store_game = store_appid.is_some() || steam_installed.names.contains(&match_key(&g.title));
            let lower: Vec<String> = g.files.iter().map(|(p, _)| p.to_lowercase()).collect();
            // Steam Auto-Cloud leaves this file in folders it syncs; saves under
            // Steam's userdata folder are Steam Cloud saves too. Both stay
            // behind after the Steam version is uninstalled, so they only
            // count while Steam has the game installed, unless every save is
            // in Steam's own cloud folder.
            let auto_cloud = lower.iter().any(|p| p.ends_with("steam_autocloud.vdf"));
            let in_userdata = lower.iter().any(|p| is_steam_userdata(p));
            let only_userdata = !lower.is_empty() && lower.iter().all(|p| is_steam_userdata(p));
            let mut f = found("ludusavi", format!("ludusavi:{}", g.title), g.title.clone());
            f.ludusavi_title = Some(g.title.clone());
            f.save_files = g.files.len();
            f.save_bytes = g.files.iter().map(|(_, b)| b).sum();
            f.save_sample = g.files.iter().take(5).map(|(p, _)| p.clone()).collect();
            f.registry_only = g.files.is_empty() && g.has_registry;
            f.store_game = store_game;
            f.store_appid = store_appid;
            f.steam_cloud = only_userdata || (store_game && (info.steam_cloud || auto_cloud || in_userdata));
            f
        })
        .collect()
}

fn is_steam_userdata(lower_path: &str) -> bool {
    let parts: Vec<&str> = lower_path.split(['\\', '/']).collect();
    parts.windows(3).any(|w| w[0] == "userdata" && w[1].chars().all(|c| c.is_ascii_digit()) && w[2].chars().all(|c| c.is_ascii_digit()))
}

const JUNK_EXE: &[&str] = &[
    "unins",
    "uninst",
    "crash",
    "redist",
    "vcredist",
    "vc_redist",
    "dxsetup",
    "dxwebsetup",
    "dotnet",
    "setup",
    "install",
    "update",
    "updater",
    "report",
    "helper",
    "cef",
    "unitycrashhandler",
    "ue4prereq",
    "easyanticheat",
    "battleye",
    "7z",
    "quicksfv",
    "gamelaunchhelper",
];
const JUNK_DIRS: [&str; 10] =
    ["redist", "_commonredist", "commonredist", "directx", "dotnet", "vcredist", "easyanticheat", "battleye", "__installer", "support"];

pub(crate) fn is_junk_exe(path: &Path) -> bool {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    JUNK_EXE.iter().any(|j| stem.starts_with(j) || stem.ends_with(j))
}

/// Pick the most likely game executable in a folder (up to 3 levels deep):
/// prefer exes whose name matches the folder, then shallow ones, then big ones.
pub fn pick_exe(dir: &Path) -> Option<PathBuf> {
    // Xbox app games: their own programs are protected.
    if let Some(helper) = xbox::launch_helper(dir) {
        return Some(helper);
    }
    let folder = match_key(&dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let mut best: Option<(i64, PathBuf)> = None;
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && JUNK_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str())))
        .flatten()
    {
        let p = entry.path();
        if !entry.file_type().is_file() || p.extension().is_none_or(|x| !x.eq_ignore_ascii_case("exe")) || is_junk_exe(p) || is_own_program(p) {
            continue;
        }
        let stem = match_key(&p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0) as i64;
        let mut score = (size / (1024 * 1024)).min(200);
        if !folder.is_empty() && (stem == folder || stem.contains(&folder) || folder.contains(&stem)) {
            score += 1000;
        }
        // Unreal games: <Game>-Win64-Shipping.exe is the real game when there
        // is no launcher next to the folder name.
        if stem.ends_with("shipping") {
            score += 200;
        }
        score -= entry.depth() as i64 * 100;
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, p.to_path_buf()));
        }
    }
    best.map(|(_, p)| p)
}

/// A folder name that says nothing about the game: a build output folder, or
/// the program's own name.
fn generic_folder(folder: &str, program: &str) -> bool {
    const GENERIC: [&str; 12] = ["publish", "bin", "build", "release", "dist", "x64", "win64", "windows", "portable", "emulator", "emulators", "emu"];
    let (f, p) = (match_key(folder), match_key(program));
    f.is_empty() || GENERIC.contains(&f.as_str()) || (!p.is_empty() && f.contains(&p))
}

/// The shallowest program in a folder (up to 3 levels deep) that a built-in
/// rule knows.
fn rule_exe(dir: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir)
        .max_depth(4)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && JUNK_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str())))
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .filter(|e| adapters::detect(e.path()).is_some())
        .min_by_key(|e| e.depth())
        .map(|e| e.path().to_path_buf())
}

/// Games in a folder: each sub-folder (or the folder itself when it holds
/// executables directly) becomes one candidate.
pub fn folder_scan(cfg: &AppConfig, dir: &Path) -> Vec<Found> {
    let mut dirs = Vec::new();
    let has_exe_here = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                let p = e.path();
                p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) && !is_junk_exe(&p) && !is_own_program(&p)
            })
        })
        .unwrap_or(false);
    if has_exe_here {
        dirs.push(dir.to_path_buf());
    } else if let Ok(rd) = std::fs::read_dir(dir) {
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        dirs.extend(subs);
    }
    let mut out = Vec::new();
    for d in dirs {
        let Some(mut exe) = pick_exe(&d) else { continue };
        // A bundle (e.g. a launcher next to an emulator): the program a
        // built-in rule knows tells where the saves are.
        if adapters::detect(&exe).is_none() {
            if let Some(e) = rule_exe(&d) {
                exe = e;
            }
        }
        let detected = adapters::detect_with(&exe, "");
        let folder = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = match &detected {
            Some(x) if !x.adapter.is_emulator() => x.adapter.name.clone(),
            // An emulator in a folder named after a game ("ZeldaBW") keeps that
            // name; build folders ("publish", "Dolphin-x64") get the emulator's.
            Some(x) if generic_folder(&folder, &x.adapter.name) => x.adapter.name.clone(),
            _ => folder,
        };
        let mut f = found("folder", format!("folder:{}", exe.display()), name);
        f.adapter = detected.as_ref().map(|x| x.adapter.key.clone());
        // `{install}` of a rule is the exe's folder unless the rule says otherwise.
        let install = match &detected {
            Some(x) => x.install.clone().or_else(|| exe.parent().map(Path::to_path_buf)),
            None => Some(d.clone()),
        };
        f.install = install.map(|p| p.to_string_lossy().into_owned());
        f.exe = Some(exe.to_string_lossy().into_owned());
        out.push(f);
    }
    // The games of emulators found here that have saves in their data folders.
    let programs: Vec<PathBuf> = out.iter().filter_map(|f| f.exe.as_deref().map(PathBuf::from)).collect();
    push_new_emulator_games(&mut out, emulator_games(&programs, false));
    attach_ludusavi_titles(&mut out);
    mark_added(cfg, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Xbox app (PC Game Pass) installs

/// Folders the Xbox app installs games into: `<drive>:\.GamingRoot` names
/// them (usually `XboxGames`).
pub fn xbox_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for letter in b'C'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        let Ok(bytes) = std::fs::read(drive.join(".GamingRoot")) else { continue };
        for p in parse_gaming_root(&bytes) {
            let p = if p.contains(':') { PathBuf::from(p) } else { drive.join(p) };
            if p.is_dir() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// `.GamingRoot`: "RGBX", a little-endian count, then that many
/// NUL-terminated UTF-16 paths (relative to the drive root).
pub fn parse_gaming_root(b: &[u8]) -> Vec<String> {
    if b.len() < 8 || &b[..4] != b"RGBX" {
        return Vec::new();
    }
    let count = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let units: Vec<u16> = b[8..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    units.split(|&u| u == 0).filter(|s| !s.is_empty()).take(count.max(1)).map(String::from_utf16_lossy).collect()
}

/// Games installed by the Xbox app: `<root>\<game>\Content\MicrosoftGame.config`.
/// They start through the launch helper (see [`xbox`]).
pub fn xbox_games() -> Vec<Found> {
    xbox_games_in(&xbox_roots())
}

pub fn xbox_games_in(roots: &[PathBuf]) -> Vec<Found> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let content = e.path().join("Content");
            let Ok(text) = std::fs::read_to_string(content.join("MicrosoftGame.config")) else { continue };
            // Add-on packages have no executable of their own.
            let Some(exe) = xbox::xml_attr(&text, "Executable", "Name").map(|n| content.join(util::native_sep(&n))) else { continue };
            if !exe.is_file() {
                continue;
            }
            let folder = e.file_name().to_string_lossy().into_owned();
            let name = clean_store_name(&xbox::xml_attr(&text, "ShellVisuals", "DefaultDisplayName").unwrap_or_else(|| folder.clone()));
            let mut f = found("xbox", format!("xbox:{folder}"), name);
            f.adapter = adapters::detect(&exe).map(|a| a.key.clone());
            f.exe = Some(xbox::launchable(exe).to_string_lossy().into_owned());
            f.install = Some(content.to_string_lossy().into_owned());
            f.install_store = Some("xbox".into());
            out.push(f);
        }
    }
    out
}

/// Store names often carry a platform suffix ("Doom Eternal - PC").
pub fn clean_store_name(name: &str) -> String {
    const SUFFIXES: [&str; 9] =
        [" - pc", " (pc)", " pc", " for windows 10", " for windows", " - windows 10", " windows 10 edition", " windows edition", " (windows)"];
    let mut s = name.replace(['™', '®'], "").trim().to_string();
    loop {
        let lower = s.to_lowercase();
        let Some(suf) = SUFFIXES.iter().find(|suf| lower.ends_with(*suf) && lower.len() > suf.len()) else { break };
        s.truncate(s.len() - suf.len());
        s = s.trim_end_matches([' ', '-', ':']).to_string();
    }
    s
}

// ---------------------------------------------------------------------------
// Where games are installed

/// A folder games are commonly installed in, offered when picking a game exe.
#[derive(Debug, Clone, Serialize, TS)]
pub struct GameFolder {
    pub label: String,
    pub path: String,
}

/// Library folders of Steam, Xbox, Epic, GOG, EA and Ubisoft that exist here.
pub fn game_folders(cfg: &AppConfig) -> Vec<GameFolder> {
    let mut list: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(root) = steam::root(cfg) {
        for lib in steam::library_folders(&root) {
            list.push(("Steam", lib.join("steamapps").join("common")));
        }
    }
    for p in xbox_roots() {
        list.push(("Xbox", p));
    }
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    for f in epic_games(&epic_manifest_dir()) {
        if let Some(parent) = f.install.as_deref().and_then(|i| Path::new(i).parent()) {
            list.push(("Epic", parent.to_path_buf()));
        }
    }
    list.push(("Epic", pf.join("Epic Games")));
    for f in gog_games() {
        if let Some(parent) = f.install.as_deref().and_then(|i| Path::new(i).parent()) {
            list.push(("GOG", parent.to_path_buf()));
        }
    }
    list.push(("GOG", pf86.join("GOG Galaxy").join("Games")));
    list.push(("EA", pf.join("EA Games")));
    list.push(("EA", pf86.join("Origin Games")));
    list.push(("Ubisoft", pf86.join("Ubisoft").join("Ubisoft Game Launcher").join("games")));
    let games_folder = if crate::i18n::is_en() { "Games folder" } else { "游戏文件夹" };
    if cfg!(windows) {
        for letter in b'C'..=b'Z' {
            let drive = PathBuf::from(format!("{}:\\", letter as char));
            list.push(("GOG", drive.join("GOG Games")));
            list.push((games_folder, drive.join("Games")));
        }
    }
    let drives = paths::extra_drives();
    if !cfg!(windows) {
        if let Some(home) = dirs::home_dir() {
            list.extend(drive_game_folders(&home, games_folder));
        }
        for d in &drives {
            list.extend(drive_game_folders(d, games_folder));
        }
    }

    let mut seen = HashSet::new();
    let list: Vec<(&str, PathBuf)> = list.into_iter().filter(|(_, p)| p.is_dir() && seen.insert(util::norm_path(&p.to_string_lossy()))).collect();
    let count = |l: &str| list.iter().filter(|(x, _)| *x == l).count();
    // Where a folder is, when a label is there more than once: its drive
    // letter, or the name of the drive it is on (Linux).
    let place = |p: &Path| -> String {
        if cfg!(windows) {
            return p.to_string_lossy().chars().take(2).collect::<String>().to_uppercase();
        }
        drives.iter().find(|d| p.starts_with(d)).and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "~".to_string())
    };
    list.iter()
        .map(|(label, p)| GameFolder {
            label: if count(label) > 1 { format!("{label} {}", place(p)) } else { label.to_string() },
            path: p.to_string_lossy().into_owned(),
        })
        .collect()
}

/// The game folders on a drive (Linux), or in the home folder: a Steam
/// library (SteamOS puts one at the root of an SD card), GOG and Heroic
/// installs, and a `Games` folder.
pub fn drive_game_folders<'a>(root: &Path, games_folder: &'a str) -> Vec<(&'a str, PathBuf)> {
    vec![
        ("Steam", root.join("steamapps").join("common")),
        ("Steam", root.join("SteamLibrary").join("steamapps").join("common")),
        ("GOG", root.join("GOG Games")),
        ("Heroic", root.join("Games").join("Heroic")),
        ("Heroic", root.join("Heroic")),
        (games_folder, root.join("Games")),
    ]
}

/// Emulators kept on another drive (Linux): portable programs and AppImages
/// on a Steam Deck's SD card, in `Applications`, `Emulators` and EmuDeck's
/// `Emulation/tools`, two folders deep at most. Only the ones a rule knows
/// are used ([`emulator_games`]).
pub fn drive_programs(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in drives {
        for dir in [d.join("Applications"), d.join("Emulators"), d.join("Emulation").join("tools")] {
            for e in walkdir::WalkDir::new(&dir).max_depth(3).into_iter().flatten() {
                if out.len() >= 300 {
                    return out;
                }
                if e.file_type().is_file() && adapters::detect_with(e.path(), "").is_some_and(|d| d.adapter.is_emulator()) {
                    out.push(e.path().to_path_buf());
                }
            }
        }
    }
    out
}

/// A game installed by a store, registered with Windows, or a subfolder of a
/// games folder.
#[derive(Debug, Clone)]
pub struct Install {
    /// "epic", "gog", "xbox", "registry" or "folder".
    pub store: String,
    pub name: String,
    pub dir: PathBuf,
    pub exe: Option<PathBuf>,
    /// Only match by the folder names in Ludusavi's manifest (generic folders
    /// such as Program Files hold much more than games).
    pub by_dir_only: bool,
}

/// Programs registered for uninstall (EA app, Ubisoft Connect, Battle.net
/// and standalone installers add their games here).
fn registered_programs() -> Vec<Install> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let places = [
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
            (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        ];
        for (hive, path) in places {
            let Ok(list) = winreg::RegKey::predef(hive).open_subkey(path) else { continue };
            for id in list.enum_keys().flatten() {
                let Ok(k) = list.open_subkey(&id) else { continue };
                let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
                let (name, dir) = (get("DisplayName"), get("InstallLocation"));
                let dir = PathBuf::from(dir.trim().trim_matches('"'));
                if name.trim().is_empty() || dir.parent().is_none() || !dir.is_dir() {
                    continue;
                }
                // DisplayIcon is often the game exe ("C:\...\game.exe,0").
                let icon = get("DisplayIcon");
                let icon = icon.split(',').next().unwrap_or("").trim().trim_matches('"');
                let exe = Some(PathBuf::from(icon)).filter(|p| {
                    p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) && p.starts_with(&dir) && p.is_file() && !is_junk_exe(p) && !is_own_program(p)
                });
                out.push(Install { store: "registry".into(), name: name.trim().to_string(), dir, exe, by_dir_only: false });
            }
        }
    }
    out
}

/// Every install Ludonimbus can see, stores first.
pub fn installs(cfg: &AppConfig) -> Vec<Install> {
    let mut out: Vec<Install> = epic_games(&epic_manifest_dir())
        .into_iter()
        .chain(gog_games())
        .chain(xbox_games())
        .filter_map(|f| {
            let exe = f.exe.map(PathBuf::from);
            let dir = f.install.map(PathBuf::from).or_else(|| exe.as_deref().and_then(Path::parent).map(Path::to_path_buf))?;
            Some(Install { store: f.source, name: f.name, dir, exe, by_dir_only: false })
        })
        .collect();
    out.extend(registered_programs());
    let mut roots: Vec<(PathBuf, bool)> = game_folders(cfg).into_iter().map(|g| (PathBuf::from(g.path), false)).collect();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = std::env::var_os(var) {
            roots.push((PathBuf::from(p), true));
        }
    }
    for (root, by_dir_only) in roots {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            let dir = e.path();
            if dir.is_dir() {
                let name = e.file_name().to_string_lossy().into_owned();
                out.push(Install { store: "folder".into(), name, dir, exe: None, by_dir_only });
            }
        }
    }
    out
}

/// Find where a game is installed by its name and the folder names
/// Ludusavi's manifest lists for it. Returns the install and its exe.
pub fn locate(installs: &[Install], title: &str, install_dirs: &[String]) -> Option<(Install, PathBuf)> {
    let key = match_key(&clean_store_name(title));
    if key.is_empty() {
        return None;
    }
    let matches = |i: &Install| {
        let folder = i.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if install_dirs.iter().any(|d| d.eq_ignore_ascii_case(&folder)) {
            return true;
        }
        !i.by_dir_only && (match_key(&clean_store_name(&i.name)) == key || match_key(&folder) == key)
    };
    let rank = |i: &Install| match i.store.as_str() {
        "epic" | "gog" | "xbox" => 0,
        "registry" => 1,
        _ => 2,
    };
    for pass in 0..3 {
        for i in installs.iter().filter(|i| rank(i) == pass && matches(i)) {
            if let Some(exe) = i.exe.clone().filter(|e| e.is_file()).or_else(|| pick_exe(&i.dir)) {
                return Some((i.clone(), exe));
            }
        }
    }
    None
}

/// Find the install of a game in the library that has no exe yet.
pub fn locate_game(cfg: &AppConfig, g: &crate::config::GameEntry) -> Option<PathBuf> {
    let titles: Vec<String> = g.ludusavi_title.iter().cloned().chain([g.name.clone()]).collect();
    let manifest = ludusavi::manifest_info(&titles.iter().cloned().collect());
    let all = installs(cfg);
    titles.iter().find_map(|t| {
        let dirs = manifest.get(t).map(|m| m.install_dirs.clone()).unwrap_or_default();
        locate(&all, t, &dirs).map(|(_, exe)| exe)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_keys() {
        assert_eq!(match_key("Hollow Knight™"), "hollowknight");
        assert_eq!(match_key("火焰纹章：万缕千丝"), "火焰纹章万缕千丝");
        assert_eq!(match_key("The Witcher 3: Wild Hunt"), "thewitcher3wildhunt");
    }

    #[test]
    fn reads_epic_manifests() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Games").join("Celeste");
        std::fs::create_dir_all(&game).unwrap();
        let item = |file: &str, body: serde_json::Value| std::fs::write(tmp.path().join(file), body.to_string()).unwrap();
        item(
            "A.item",
            serde_json::json!({
                "DisplayName": "Celeste", "AppName": "Salt", "InstallLocation": game.to_string_lossy(),
                "LaunchExecutable": "Celeste.exe", "LaunchCommand": "", "bIsIncompleteInstall": false, "bIsApplication": true
            }),
        );
        item(
            "B.item",
            serde_json::json!({
                "DisplayName": "Half done", "AppName": "X", "InstallLocation": "D:\\X", "LaunchExecutable": "x.exe",
                "bIsIncompleteInstall": true
            }),
        );
        item("C.item", serde_json::json!({ "DisplayName": "DLC", "InstallLocation": "D:\\Y", "LaunchExecutable": "y.exe", "bIsApplication": false }));
        std::fs::write(tmp.path().join("notes.txt"), "x").unwrap();
        let list = epic_games(tmp.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Celeste");
        assert_eq!(list[0].id, "epic:Salt");
        assert!(Path::new(list[0].exe.as_ref().unwrap()).ends_with("Celeste/Celeste.exe"));
    }

    #[test]
    fn picks_game_exes_in_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let mk = |rel: &str, size: usize| {
            let p = tmp.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; size]).unwrap();
        };
        mk("Hades/Hades.exe", 10);
        mk("Hades/unins000.exe", 2_000_000);
        mk("Hades/_CommonRedist/vcredist_x64.exe", 9_000_000);
        mk("Fancy/Fancy.exe", 1);
        mk("Fancy/Fancy/Binaries/Win64/Fancy-Win64-Shipping.exe", 50);
        mk("NoExe/readme.txt", 1);
        assert!(pick_exe(&tmp.path().join("Hades")).unwrap().ends_with("Hades/Hades.exe"));
        // The launcher next to the folder name wins over the deep shipping exe.
        assert!(pick_exe(&tmp.path().join("Fancy")).unwrap().ends_with("Fancy/Fancy.exe"));
        assert!(pick_exe(&tmp.path().join("NoExe")).is_none());
    }

    #[test]
    fn classifies_ludusavi_results() {
        let g =
            |t: &str, files: &[&str]| ludusavi::ScanGame { title: t.into(), files: files.iter().map(|f| (f.to_string(), 10)).collect(), has_registry: false };
        let games = vec![
            g("Hollow Knight", &[r"C:\Users\me\AppData\LocalLow\Team Cherry\Hollow Knight\user1.dat"]),
            g("Ace Combat 8", &[r"C:\Users\me\AppData\Local\AC8\steam_autocloud.vdf", r"C:\Users\me\AppData\Local\AC8\save.sav"]),
            g(
                "Granblue Fantasy: Relink",
                &[r"C:\Users\me\AppData\Local\GBFR\Saved\SaveGames\steam_autocloud.vdf", r"C:\Users\me\AppData\Local\GBFR\Saved\SaveGames\SaveData1.dat"],
            ),
            g("Celeste", &[r"C:\Program Files (x86)\Steam\userdata\123\504230\remote\0.celeste"]),
            g(
                "Florence",
                &[
                    r"C:\Program Files (x86)\Steam\userdata\123\1102130\remote\FlorenceSave.dat",
                    r"C:\Users\me\AppData\LocalLow\Mountains\Florence\FlorenceSave.dat",
                ],
            ),
            g("Hades", &[r"C:\Users\me\Documents\Saved Games\Hades\Profile1.sav"]),
            g("The Witcher 3", &[r"C:\Users\me\Documents\The Witcher 3\gamesaves\a.sav"]),
        ];
        let mut manifest = HashMap::new();
        let info = |id: u32, extra: &[u32]| ludusavi::ManifestInfo { steam_id: Some(id), steam_extra: extra.to_vec(), steam_cloud: true, ..Default::default() };
        manifest.insert("Hollow Knight".to_string(), info(367520, &[]));
        manifest.insert("Hades".to_string(), info(1145360, &[]));
        manifest.insert("The Witcher 3".to_string(), info(292030, &[499450]));
        let installed = SteamInstalled { ids: [367520, 499450].into_iter().collect(), names: [match_key("Ace Combat 8™")].into_iter().collect() };
        let out = classify(&games, &manifest, &installed);
        let by = |t: &str| out.iter().find(|f| f.name == t).unwrap();
        // Installed through Steam (by id, another edition's id, or name) and synced by Steam Cloud.
        assert!(by("Hollow Knight").store_game && by("Hollow Knight").steam_cloud);
        assert!(by("The Witcher 3").store_game && by("The Witcher 3").steam_cloud);
        assert!(by("Ace Combat 8").store_game && by("Ace Combat 8").steam_cloud);
        // Which installed game it is, when the manifest's ids say.
        assert_eq!(by("Hollow Knight").store_appid, Some(367520));
        assert_eq!(by("The Witcher 3").store_appid, Some(499450));
        assert_eq!(by("Ace Combat 8").store_appid, None);
        assert_eq!(by("Hades").store_appid, None);
        // Every save is in Steam's cloud folder.
        assert!(by("Celeste").steam_cloud);
        // Steam leftovers (an old Auto-Cloud marker, a copy in userdata) of a
        // game that Steam no longer has installed: the other saves need us.
        assert!(!by("Granblue Fantasy: Relink").steam_cloud);
        assert!(!by("Florence").steam_cloud);
        // Supports Steam Cloud, but not installed through Steam here (e.g. Epic).
        assert!(!by("Hades").store_game && !by("Hades").steam_cloud);
        assert_eq!(by("Hades").save_files, 1);
        assert_eq!(by("Hades").ludusavi_title.as_deref(), Some("Hades"));
    }

    #[test]
    fn reads_gaming_root_and_xbox_games() {
        let mut b = b"RGBX".to_vec();
        b.extend(1u32.to_le_bytes());
        for u in "XboxGames".encode_utf16().chain([0]) {
            b.extend(u.to_le_bytes());
        }
        assert_eq!(parse_gaming_root(&b), vec!["XboxGames".to_string()]);
        assert!(parse_gaming_root(b"nope").is_empty());

        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Doom Eternal - PC").join("Content");
        std::fs::create_dir_all(game.join("launcher")).unwrap();
        std::fs::write(game.join("launcher").join("idTechLauncher.exe"), b"x").unwrap();
        std::fs::write(game.join("gamelaunchhelper.exe"), b"x").unwrap();
        std::fs::write(
            game.join("MicrosoftGame.config"),
            r#"<?xml version="1.0"?><Game><Identity Name="X" /><ExecutableList><Executable Name="launcher/idTechLauncher.exe" Id="Game" /></ExecutableList><ShellVisuals DefaultDisplayName="Doom Eternal - PC" /></Game>"#,
        )
        .unwrap();
        // An add-on: its executable is not in its own package.
        let dlc = tmp.path().join("DOOM Eternal- Campaign").join("Content");
        std::fs::create_dir_all(&dlc).unwrap();
        std::fs::write(dlc.join("MicrosoftGame.config"), r#"<Game><ExecutableList><Executable Name="game.exe" /></ExecutableList></Game>"#).unwrap();
        let list = xbox_games_in(&[tmp.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Doom Eternal");
        // Started through the launch helper: the game's own program is protected.
        assert!(Path::new(list[0].exe.as_ref().unwrap()).ends_with("Content/gamelaunchhelper.exe"));
        assert_eq!(list[0].source, "xbox");
        assert_eq!(pick_exe(&tmp.path().join("Doom Eternal - PC")), Some(game.join("gamelaunchhelper.exe")));
    }

    #[test]
    fn knows_its_own_programs() {
        let mut env = crate::paths::TestEnv::new();
        let tmp = tempfile::tempdir().unwrap();
        env.set("LUDONIMBUS_HOME", tmp.path());
        let own = [
            r"C:\Program Files\Ludonimbus\ludonimbus-app.exe",
            r#""C:\Program Files\Ludonimbus\ludonimbus-app.exe""#,
            r"C:\Program Files\Ludonimbus\ludonimbus-run.exe",
            "/home/deck/.local/share/ludonimbus/Ludonimbus.AppImage",
            "/home/deck/Downloads/Ludonimbus_1.0.0_amd64.AppImage",
            "/home/deck/.config/Ludonimbus/bin/ludonimbus-agent",
        ];
        for p in own {
            assert!(is_own_program(Path::new(p)), "{p}");
        }
        // Whatever is in the folder Linux keeps the wrapper in.
        assert!(is_own_program(&tmp.path().join("bin").join("run")));
        assert!(!is_own_program(Path::new(r"D:\Games\Hades\Hades.exe")));
        assert!(!is_own_program(Path::new(r"D:\Games\Nimbus\nimbus.exe")));
    }

    #[test]
    fn steam_scan_leaves_out_ludonimbus() {
        let mut env = crate::paths::TestEnv::new();
        let tmp = tempfile::tempdir().unwrap();
        env.set("LUDONIMBUS_HOME", tmp.path().join("home"));
        let root = tmp.path().join("Steam");
        std::fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();
        let game = tmp.path().join("Games").join("Hades").join("Hades.exe");
        let app = tmp.path().join("Ludonimbus").join("ludonimbus-app.exe");
        for f in [&game, &app] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, b"x").unwrap();
        }
        crate::handheld::add_to_steam(&root, "42", &app).unwrap();
        let shortcut = |exe: &Path, name: &str| steam::NewShortcut {
            name: name.into(),
            exe: exe.to_string_lossy().into_owned(),
            start_dir: exe.parent().unwrap().to_string_lossy().into_owned(),
            launch_options: String::new(),
            icon: String::new(),
        };
        steam::add_shortcut(&root, "42", &shortcut(&game, "Hades")).unwrap();
        let mut cfg = AppConfig::default();
        cfg.steam.root = Some(root);
        let names: Vec<String> = steam_shortcuts(&cfg).into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["Hades".to_string()]);
    }

    #[test]
    fn cleans_store_names() {
        assert_eq!(clean_store_name("Doom Eternal - PC"), "Doom Eternal");
        assert_eq!(clean_store_name("Forza Horizon 5 for Windows 10"), "Forza Horizon 5");
        assert_eq!(clean_store_name("Hollow Knight™"), "Hollow Knight");
        assert_eq!(clean_store_name("PC"), "PC");
    }

    #[test]
    fn locates_installs() {
        let tmp = tempfile::tempdir().unwrap();
        let mk = |rel: &str| {
            let p = tmp.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
            p
        };
        let titan = mk("EA Games/Titanfall2/Titanfall2.exe");
        let doom = mk("XboxGames/Doom/Content/DOOMEternalx64vk.exe");
        mk("Program Files/Control/Control.exe");
        let folder = |rel: &str, by_dir_only: bool| Install {
            store: "folder".into(),
            name: Path::new(rel).file_name().unwrap().to_string_lossy().into_owned(),
            dir: tmp.path().join(rel),
            exe: None,
            by_dir_only,
        };
        let installs = vec![
            Install {
                store: "xbox".into(),
                name: "Doom Eternal".into(),
                dir: tmp.path().join("XboxGames/Doom/Content"),
                exe: Some(doom.clone()),
                by_dir_only: false,
            },
            folder("EA Games/Titanfall2", false),
            folder("Program Files/Control", true),
        ];
        // By store name.
        assert_eq!(locate(&installs, "DOOM Eternal", &[]).unwrap().1, doom);
        // By the folder name in Ludusavi's manifest.
        let (i, exe) = locate(&installs, "Titanfall 2", &["Titanfall2".into()]).unwrap();
        assert_eq!((i.store.as_str(), exe), ("folder", titan));
        // Program Files only by manifest folder names.
        assert!(locate(&installs, "Control", &[]).is_none());
        assert!(locate(&installs, "Control", &["Control".into()]).is_some());
        assert!(locate(&installs, "Starfield", &["Starfield".into()]).is_none());
    }
}
