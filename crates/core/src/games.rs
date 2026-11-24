//! Library operations shared by the CLI, the Steam wrapper and the app.

use crate::adapters::{self, Adapter};
use crate::cloud::CloudGame;
use crate::config::{self, AppConfig, GameEntry, OriginalShortcut, SteamLink};
use crate::discover::Found;
use crate::emulators::{self, EmuGame};
use crate::{localart, paths, steam, util};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub fn slugify(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        format!("game-{}", &ulid::Ulid::new().to_string().to_lowercase()[..8])
    } else {
        s
    }
}

pub fn unique_key(cfg: &AppConfig, base: &str) -> String {
    if cfg.game(base).is_none() {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|k| cfg.game(k).is_none())
        .expect("infinite iterator")
}

/// Names that Steam stored broken (e.g. U+FFFD from a bad encoding).
pub fn looks_corrupted(name: &str) -> bool {
    name.contains('\u{FFFD}') || name.trim().is_empty()
}

/// What the built-in rules know about a game started by an exe.
#[derive(Debug, Clone)]
pub struct Recognized {
    pub adapter: &'static Adapter,
    /// What `{install}` stands for when it isn't the exe's folder.
    pub install: Option<PathBuf>,
    /// Started through RecompLauncher.
    pub via_recomplauncher: bool,
    /// The game an emulator starts, when it could be identified.
    pub game: Option<EmuGame>,
}

impl Recognized {
    /// "Dolphin（RMGE01）", "Dolphin（整个模拟器的存档）", or the game's name.
    pub fn label(&self) -> String {
        adapters::label(self.adapter, self.game.as_ref())
    }

    /// Save patterns for the library entry: one emulator game's saves, or
    /// empty for the rule's own.
    pub fn saves(&self) -> Vec<String> {
        self.game.as_ref().map(|g| g.saves(self.adapter)).unwrap_or_default()
    }

    /// A name when the shortcut has none: for emulator games the name the
    /// emulator knows, else the disc / ROM name; else the rule's game name.
    pub fn default_name(&self) -> String {
        match &self.game {
            Some(g) if self.adapter.is_emulator() => g
                .name
                .clone()
                .or_else(|| g.source.as_deref().and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| g.id.clone()),
            _ => self.adapter.name.clone(),
        }
    }
}

/// Recognise a game from its exe and launch options. `forced` is a rule the
/// caller already chose (e.g. from the cloud).
pub fn recognize(exe: &Path, args: &str, forced: Option<&str>) -> Option<Recognized> {
    let detected = adapters::detect_with(exe, args);
    let (adapter, detected) = match forced.and_then(adapters::find) {
        Some(a) => (a, detected.filter(|d| d.adapter.key == a.key)),
        None => {
            let d = detected?;
            (d.adapter, Some(d))
        }
    };
    Some(Recognized {
        adapter,
        install: detected.as_ref().and_then(|d| d.install.clone()),
        via_recomplauncher: detected.is_some_and(|d| d.via_recomplauncher),
        game: emulators::identify(adapter, exe, args),
    })
}

/// Library entries of one emulator must not sync the same files twice: the
/// whole emulator (empty `saves`) or games one by one, each game once.
fn check_emulator_overlap(cfg: &AppConfig, adapter: &Adapter, saves: &[String]) -> Result<()> {
    if !adapter.is_emulator() {
        return Ok(());
    }
    let same: Vec<&GameEntry> = cfg.games.iter().filter(|g| g.adapter.as_deref() == Some(adapter.key.as_str())).collect();
    if let Some(g) = same.iter().find(|g| g.saves.is_empty()) {
        bail!(crate::tr!("{} 里所有游戏的存档已经由「{}」一起同步了，不需要再添加", "The saves of every game in {} are already synced together by \"{}\"; no need to add this one", adapter.name, g.name));
    }
    if saves.is_empty() && !same.is_empty() {
        let names: Vec<String> = same.iter().take(3).map(|g| g.name.clone()).collect();
        let more = same.len() > 3;
        if crate::i18n::is_en() {
            let more = if more { " and others" } else { "" };
            bail!("{}{more} already sync on their own; syncing all of {} as well would duplicate them. Remove them first to sync the whole emulator", names.join(", "), adapter.name);
        }
        let names: String = names.iter().map(|n| format!("「{n}」")).collect();
        let more = if more { "等游戏" } else { "" };
        bail!("{names}{more}已经单独同步了，再同步整个 {} 会重复。要整体同步，请先移除它们", adapter.name);
    }
    if let Some(g) = same.iter().find(|g| g.saves == saves) {
        bail!(crate::tr!("这个游戏的存档已经由「{}」同步了", "This game's saves are already synced by \"{}\"", g.name));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCandidate {
    pub account: String,
    pub appid: u32,
    pub name: String,
    pub name_corrupted: bool,
    pub suggested_name: String,
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    pub adapter: Option<String>,
    pub adapter_name: Option<String>,
    /// One emulator game's saves (empty: the rule's own, or no rule).
    pub saves: Vec<String>,
    pub added_key: Option<String>,
    pub wrapped_key: Option<String>,
}

pub fn shortcut_candidates(cfg: &AppConfig, account: &str) -> Result<Vec<ShortcutCandidate>> {
    let root = steam::root(cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    let (_, list) = steam::read_shortcuts(&root, account)?;
    Ok(list
        .into_iter()
        .map(|s| {
            let rec = recognize(Path::new(&s.exe), &s.launch_options, None);
            let added_key = cfg
                .games
                .iter()
                .find(|g| g.steam.as_ref().is_some_and(|l| l.appid == s.appid && l.account == account))
                .map(|g| g.key.clone());
            let corrupted = looks_corrupted(&s.name);
            let suggested = match (corrupted, &rec) {
                (true, Some(r)) => r.default_name(),
                (true, None) => Path::new(&s.exe)
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| crate::tr!("未命名游戏", "Untitled game")),
                (false, _) => s.name.clone(),
            };
            ShortcutCandidate {
                account: account.to_string(),
                appid: s.appid,
                name_corrupted: corrupted,
                suggested_name: suggested,
                wrapped_key: s.wrapped_key(),
                exe: s.exe,
                start_dir: s.start_dir,
                launch_options: s.launch_options,
                adapter: rec.as_ref().map(|r| r.adapter.key.clone()),
                adapter_name: rec.as_ref().map(Recognized::label),
                saves: rec.as_ref().map(Recognized::saves).unwrap_or_default(),
                added_key,
                name: s.name,
            }
        })
        .collect())
}

pub struct NewGame {
    pub name: Option<String>,
    pub adapter: Option<String>,
    pub saves: Vec<String>,
    /// Reuse the key of a game that already exists in the cloud.
    pub key: Option<String>,
    /// Find saves through Ludusavi's database.
    pub ludusavi_title: Option<String>,
}

/// Add a game from a Steam shortcut.
pub fn add_from_shortcut(account: &str, appid: u32, opts: NewGame) -> Result<GameEntry> {
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    let sc = steam::find_shortcut(&root, account, appid)?
        .ok_or_else(|| anyhow!(crate::tr!("找不到这个 Steam 快捷方式", "This Steam shortcut wasn't found")))?;
    if sc.wrapped_key().is_some() {
        bail!(crate::tr!("这个快捷方式已经由 Ludonimbus 接管，但本机配置里没有它。请在 Steam 里把目标改回游戏程序后再添加。", "Ludonimbus already took over this shortcut, but it's missing from this PC's configuration. Point the shortcut back at the game's program in Steam, then add it again."));
    }
    let exe = PathBuf::from(&sc.exe);
    let rec = recognize(&exe, &sc.launch_options, opts.adapter.as_deref());
    let adapter_key = opts.adapter.clone().or_else(|| rec.as_ref().map(|r| r.adapter.key.clone()));
    let adapter = adapter_key.as_deref().and_then(adapters::find);
    // Saves the rule works out (one emulator game) unless the user chose.
    let from_rule = opts.saves.is_empty();
    let saves = if from_rule { rec.as_ref().map(Recognized::saves).unwrap_or_default() } else { opts.saves.clone() };
    if adapter.is_none() && saves.is_empty() && opts.ludusavi_title.is_none() {
        bail!(crate::tr!("无法自动识别这个游戏的存档位置，请手动指定存档文件夹", "Couldn't find this game's save location automatically. Choose the save folder yourself"));
    }
    let name = opts
        .name
        .filter(|n| !n.trim().is_empty())
        .or_else(|| (!looks_corrupted(&sc.name)).then(|| sc.name.clone()))
        .or_else(|| rec.as_ref().map(Recognized::default_name))
        .unwrap_or_else(|| crate::tr!("未命名游戏", "Untitled game"));
    let install = rec.as_ref().filter(|_| from_rule).and_then(|r| r.install.clone());
    let via_launcher = rec.as_ref().is_some_and(|r| r.via_recomplauncher);
    let work_dir = (!sc.start_dir.trim().is_empty()).then(|| PathBuf::from(&sc.start_dir));
    config::update(|cfg| {
        if let Some(g) = cfg
            .games
            .iter()
            .find(|g| g.steam.as_ref().is_some_and(|l| l.appid == appid && l.account == account))
        {
            bail!(crate::tr!("「{}」已经添加过了", "\"{}\" is already added", g.name));
        }
        if let Some(a) = adapter {
            check_emulator_overlap(cfg, a, &saves)?;
        }
        // An emulator rule is shared by many games: name the entry after the game.
        let base = opts
            .key
            .clone()
            .or_else(|| adapter.filter(|a| !a.is_emulator()).map(|a| a.key.clone()))
            .unwrap_or_else(|| slugify(&name));
        let key = if opts.key.is_some() { base } else { unique_key(cfg, &base) };
        if cfg.game(&key).is_some() {
            bail!(crate::tr!("本机已经有一个 id 为 {key} 的游戏", "This PC already has a game with id {key}"));
        }
        let entry = GameEntry {
            key,
            name,
            adapter: adapter_key.clone(),
            exe: Some(exe.clone()),
            args: sc.launch_options.clone(),
            work_dir,
            install: install.clone(),
            saves: saves.clone(),
            ludusavi_title: if adapter.is_none() && saves.is_empty() { opts.ludusavi_title.clone() } else { None },
            // RecompLauncher may stay open after the game: upload when the game exits.
            upload_on_game_exit: via_launcher.then_some(true),
            steam: Some(SteamLink {
                account: account.to_string(),
                appid,
                enabled: false,
                original: None,
                created: false,
            }),
            auto_sync: true,
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        Ok(entry)
    })
}

/// Add a game that is not in Steam (or linked later).
pub fn add_custom(name: &str, exe: Option<PathBuf>, saves: Vec<String>, ludusavi_title: Option<String>, key: Option<String>) -> Result<GameEntry> {
    add_custom_with(name, exe, "", None, saves, ludusavi_title, key)
}

/// [`add_custom`] for a game started with launch options (`args`), which
/// can tell which game an emulator starts. `rule` is a built-in rule the
/// caller already knows applies (e.g. from RecompLauncher's catalog id).
pub fn add_custom_with(
    name: &str,
    exe: Option<PathBuf>,
    args: &str,
    rule: Option<&str>,
    saves: Vec<String>,
    ludusavi_title: Option<String>,
    key: Option<String>,
) -> Result<GameEntry> {
    let exe = exe.map(crate::xbox::launchable);
    // The user's saves replace the rule (and are relative to the exe folder).
    let rec = exe.as_deref().and_then(|e| recognize(e, args, rule)).filter(|_| saves.is_empty());
    let saves = if saves.is_empty() { rec.as_ref().map(Recognized::saves).unwrap_or_default() } else { saves };
    if saves.is_empty() && ludusavi_title.is_none() && rec.is_none() {
        bail!(crate::tr!("请至少指定一个存档位置", "Choose at least one save location"));
    }
    config::update(|cfg| {
        if let Some(r) = &rec {
            check_emulator_overlap(cfg, r.adapter, &saves)?;
        }
        let key = match key {
            Some(k) if cfg.game(&k).is_none() => k,
            Some(k) => bail!(crate::tr!("本机已经有一个 id 为 {k} 的游戏", "This PC already has a game with id {k}")),
            None => unique_key(cfg, &slugify(name)),
        };
        let entry = GameEntry {
            key,
            name: name.to_string(),
            adapter: rec.as_ref().map(|r| r.adapter.key.clone()),
            exe,
            args: args.to_string(),
            install: rec.as_ref().and_then(|r| r.install.clone()),
            saves,
            ludusavi_title,
            upload_on_game_exit: rec.as_ref().is_some_and(|r| r.via_recomplauncher).then_some(true),
            auto_sync: true,
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        Ok(entry)
    })
}

/// Recreate a game known to the cloud on this device.
pub fn add_from_cloud(cg: &CloudGame, account: Option<&str>, appid: Option<u32>, exe: Option<PathBuf>) -> Result<GameEntry> {
    if let (Some(account), Some(appid)) = (account, appid) {
        let g = add_from_shortcut(
            account,
            appid,
            NewGame {
                name: Some(cg.name.clone()),
                adapter: cg.adapter.clone(),
                saves: cg.saves.clone(),
                key: Some(cg.key.clone()),
                ludusavi_title: cg.ludusavi_title.clone(),
            },
        )?;
        return update_game(&g.key, |e| {
            e.exclude = cg.exclude.clone();
            e.wait_for = cg.wait_for.clone();
            e.roots = cg.roots.clone();
            e.ludusavi_title = cg.ludusavi_title.clone();
            e.source = cg.source.clone();
            Ok(())
        });
    }
    config::update(|cfg| {
        if cfg.game(&cg.key).is_some() {
            bail!(crate::tr!("本机已经添加过「{}」", "\"{}\" is already added on this PC", cg.name));
        }
        let install = exe
            .as_deref()
            .filter(|_| cg.saves.is_empty() || cg.adapter.as_deref().and_then(adapters::find).is_some_and(Adapter::is_emulator))
            .and_then(|e| recognize(e, "", cg.adapter.as_deref()))
            .and_then(|r| r.install);
        let entry = GameEntry {
            key: cg.key.clone(),
            name: cg.name.clone(),
            adapter: cg.adapter.clone(),
            install,
            exe,
            saves: cg.saves.clone(),
            exclude: cg.exclude.clone(),
            wait_for: cg.wait_for.clone(),
            roots: cg.roots.clone(),
            ludusavi_title: cg.ludusavi_title.clone(),
            auto_sync: true,
            source: cg.source.clone(),
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        Ok(entry)
    })
}

/// What kind of program a game runs on, for the library's source badges
/// and groups.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// An emulator rule: many games share it.
    Emulator,
    /// A rule for one game: recompilations, ports, fan remakes.
    Recomp,
    /// A PC game whose saves Ludusavi's database knows.
    Pc,
    /// An Xbox app (Game Pass) game, started through `gamelaunchhelper.exe`.
    Xbox,
    /// Added by hand with its own save folders.
    Custom,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub kind: SourceKind,
    /// The emulator (`Dolphin`); `None` for the other kinds, whose group
    /// name the interface supplies.
    pub name: Option<String>,
    /// The rule's platform in the interface language
    /// (`GameCube / Wii 模拟器`, `Longshot（原生重制）`).
    pub detail: Option<String>,
    /// The group the user chose instead (see [`GameEntry::source`]).
    pub label: Option<String>,
}

/// Where a game comes from, worked out from its save rule and program.
pub fn source(g: &GameEntry) -> Source {
    let (kind, name, detail) = match g.adapter.as_deref().and_then(adapters::find) {
        Some(a) if a.is_emulator() => (SourceKind::Emulator, Some(a.name.clone()), a.platform_label()),
        Some(a) => (SourceKind::Recomp, None, a.platform_label()),
        None if g.exe.as_deref().is_some_and(|e| crate::xbox::is_launch_helper(e) || crate::xbox::content_dir(e).is_some()) => {
            (SourceKind::Xbox, None, None)
        }
        None if g.ludusavi_title.is_some() => (SourceKind::Pc, None, None),
        None => (SourceKind::Custom, None, None),
    };
    let label = g.source.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Source { kind, name, detail, label }
}

pub fn update_game(key: &str, f: impl FnOnce(&mut GameEntry) -> Result<()>) -> Result<GameEntry> {
    config::update(|cfg| {
        let g = cfg.game_mut(key).ok_or_else(|| anyhow!(crate::tr!("找不到游戏 {key}", "Game {key} not found")))?;
        f(g)?;
        Ok(g.clone())
    })
}

/// Point a game at a new executable (after an update / move).
pub fn set_exe(key: &str, exe: &Path) -> Result<GameEntry> {
    if !exe.is_file() {
        bail!(crate::tr!("文件不存在：{}", "The file doesn't exist: {}", exe.display()));
    }
    // An Xbox app game's own program cannot be started from outside.
    let exe = &crate::xbox::launchable(exe.to_path_buf());
    update_game(key, |g| {
        let old_dir = g.exe.as_ref().and_then(|e| e.parent().map(Path::to_path_buf));
        g.exe = Some(exe.to_path_buf());
        // Explicit folders that pointed at the old location follow the exe.
        let new_dir = exe.parent().map(Path::to_path_buf);
        if let (Some(old), Some(new)) = (&old_dir, &new_dir) {
            if g.work_dir.as_ref().is_some_and(|w| util::same_path(&w.to_string_lossy(), &old.to_string_lossy())) {
                g.work_dir = Some(new.clone());
            }
            if g.install.as_ref().is_some_and(|w| util::same_path(&w.to_string_lossy(), &old.to_string_lossy())) {
                g.install = Some(new.clone());
            }
        }
        // A rule whose exe sits deeper in the release knows the game folder.
        if g.saves.is_empty() || g.adapter.as_deref().and_then(adapters::find).is_some_and(Adapter::is_emulator) {
            if let Some(i) = recognize(exe, &g.args, g.adapter.as_deref()).and_then(|r| r.install) {
                g.install = Some(i);
            }
        }
        Ok(())
    })
}

pub fn remove(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?;
    if g.steam.as_ref().is_some_and(|l| l.enabled) {
        bail!(crate::tr!("请先关闭这个游戏的 Steam 自动同步（会把快捷方式恢复原样），再移除", "Turn off Steam auto-sync for this game first (this restores the shortcut), then remove it"));
    }
    config::update(|cfg| {
        cfg.games.retain(|g| g.key != key);
        Ok(())
    })?;
    localart::remove_all(key);
    crate::state::update(|s| {
        s.games.remove(key);
    })
}

/// What the user decided for a game found by a scan.
#[derive(Debug, Clone, Default)]
pub struct FoundChoice {
    pub name: String,
    /// Save patterns (empty: use the built-in rule or Ludusavi).
    pub saves: Vec<String>,
    pub ludusavi_title: Option<String>,
    pub exe: Option<PathBuf>,
}

/// Add a game found by [`crate::discover`].
pub fn add_found(f: &Found, c: FoundChoice) -> Result<GameEntry> {
    let name = if c.name.trim().is_empty() { f.name.clone() } else { c.name.trim().to_string() };
    let title = c.ludusavi_title.clone().or_else(|| f.ludusavi_title.clone());
    if let (Some(account), Some(appid)) = (&f.steam_account, f.steam_appid) {
        return add_from_shortcut(
            account,
            appid,
            NewGame {
                name: Some(name),
                adapter: f.adapter.clone(),
                saves: c.saves,
                key: None,
                ludusavi_title: title,
            },
        );
    }
    let exe = c.exe.or_else(|| f.exe.as_ref().map(PathBuf::from));
    if f.source == "emulator" && c.saves.is_empty() {
        return add_emulator_game(f, name, exe);
    }
    // The rule the scan found (by exe, or by RecompLauncher's catalog id).
    let rule = f.adapter.as_deref().filter(|k| adapters::find(k).is_some());
    let has_rule = exe.as_deref().and_then(|e| recognize(e, &f.args, rule)).is_some();
    // Save places the scan found (e.g. picked in RecompLauncher) when there is no rule.
    let saves = if c.saves.is_empty() && !has_rule { f.saves.clone() } else { c.saves };
    if !has_rule && saves.is_empty() && title.is_none() {
        bail!(crate::tr!("还不知道「{name}」的存档在哪里，请先指定存档位置", "It's not known yet where \"{name}\" keeps its saves. Choose the save location first"));
    }
    let title = if has_rule || !saves.is_empty() { None } else { title };
    let g = add_custom_with(&name, exe.clone(), &f.args, rule, saves, title, None)?;
    let install = f.install.as_ref().map(PathBuf::from).filter(|i| i.is_dir());
    let exe_dir = exe.as_deref().and_then(Path::parent).map(Path::to_path_buf);
    update_game(&g.key, |e| {
        if e.install.is_none() && install.is_some() && install != exe_dir {
            e.install = install.clone();
        }
        Ok(())
    })
}

/// Add one game found in an emulator's data folder: the emulator rule with
/// that game's saves, started by the emulator (with the game when its file
/// was found).
fn add_emulator_game(f: &Found, name: String, exe: Option<PathBuf>) -> Result<GameEntry> {
    let a = f
        .adapter
        .as_deref()
        .and_then(adapters::find)
        .filter(|a| a.is_emulator())
        .ok_or_else(|| anyhow!(crate::tr!("不认识「{name}」所在的模拟器", "The emulator of \"{name}\" isn't recognized")))?;
    if f.saves.is_empty() {
        bail!(crate::tr!("还不知道「{name}」的存档在哪里", "It's not known yet where \"{name}\" keeps its saves"));
    }
    let exe_dir = exe.as_deref().and_then(Path::parent).map(Path::to_path_buf);
    let install = f.install.as_ref().map(PathBuf::from).filter(|i| i.is_dir() && Some(i) != exe_dir.as_ref());
    config::update(|cfg| {
        check_emulator_overlap(cfg, a, &f.saves)?;
        let key = unique_key(cfg, &slugify(&name));
        let entry = GameEntry {
            key,
            name: name.clone(),
            adapter: Some(a.key.clone()),
            args: if exe.is_some() { f.args.clone() } else { String::new() },
            exe: exe.clone(),
            install: install.clone(),
            saves: f.saves.clone(),
            auto_sync: true,
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        Ok(entry)
    })
}

/// Create a Steam shortcut for a game that is not in Steam yet. It launches
/// through `ludonimbus-run`, so auto-sync is on right away, and gets the
/// artwork chosen in Ludonimbus. Steam must be closed.
pub fn add_to_steam(key: &str, account: &str) -> Result<u32> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    if g.steam.is_some() {
        bail!(crate::tr!("「{}」已经关联了 Steam 快捷方式", "\"{}\" is already linked to a Steam shortcut", g.name));
    }
    let exe = g
        .exe
        .clone()
        .filter(|e| e.is_file())
        .ok_or_else(|| anyhow!(crate::tr!("「{}」还没有设置游戏程序，Steam 需要知道启动哪个程序", "\"{}\" has no program set yet; Steam needs to know which program to start", g.name)))?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    let runner = paths::runner_exe();
    if !runner.is_file() {
        bail!(crate::tr!("找不到启动包装器 {}，请重新安装 Ludonimbus", "The launch wrapper {} is missing. Please reinstall Ludonimbus", runner.display()));
    }
    let exe_s = exe.to_string_lossy().into_owned();
    let work_dir = g
        .work_dir
        .clone()
        .or_else(|| exe.parent().map(Path::to_path_buf))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let appid = steam::add_shortcut(
        &root,
        account,
        &steam::NewShortcut {
            name: g.name.clone(),
            exe: runner.to_string_lossy().into_owned(),
            start_dir: work_dir.clone(),
            launch_options: format!("--game {key}"),
            icon: exe_s.clone(),
        },
    )?;
    for (kind, file) in localart::all(key) {
        let dest = steam::write_art(&root, account, appid, &kind, &file)?;
        if kind == "icon" {
            steam::set_shortcut_icon(&root, account, appid, &dest)?;
        }
    }
    let account = account.to_string();
    update_game(key, |e| {
        e.steam = Some(SteamLink {
            account: account.clone(),
            appid,
            enabled: true,
            original: Some(OriginalShortcut {
                exe: exe_s.clone(),
                start_dir: work_dir.clone(),
                launch_options: g.args.clone(),
                icon: exe_s.clone(),
            }),
            created: true,
        });
        Ok(())
    })?;
    Ok(appid)
}

/// Delete the Steam shortcut that [`add_to_steam`] created. Steam must be closed.
pub fn remove_from_steam(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    let link = g.steam.clone().ok_or_else(|| anyhow!(crate::tr!("这个游戏没有关联 Steam 快捷方式", "This game isn't linked to a Steam shortcut")))?;
    if !link.created {
        bail!(crate::tr!("这个 Steam 快捷方式不是 Ludonimbus 创建的，请在 Steam 里删除", "Ludonimbus didn't create this Steam shortcut. Delete it in Steam"));
    }
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    steam::remove_shortcut(&root, &link.account, link.appid)?;
    update_game(key, |e| {
        e.steam = None;
        Ok(())
    })?;
    Ok(())
}

/// Use an image as a game's artwork: kept in Ludonimbus and, when the game is
/// in Steam, written to Steam's grid folder (Steam shows it after a restart).
pub fn set_art(key: &str, kind: &str, src: &Path) -> Result<()> {
    let stored = localart::set(key, kind, src)?;
    let cfg = config::load()?;
    let g = cfg.require_game(key)?;
    if let (Some(link), Some(root)) = (&g.steam, steam::root(&cfg)) {
        let dest = steam::write_art(&root, &link.account, link.appid, kind, &stored)?;
        if kind == "icon" && !steam::is_running() {
            let _ = steam::set_shortcut_icon(&root, &link.account, link.appid, &dest);
        }
    }
    Ok(())
}


/// Make the Steam shortcut launch through `ludonimbus-run`.
pub fn steam_enable(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?;
    let link = g.steam.clone().ok_or_else(|| anyhow!(crate::tr!("这个游戏没有关联 Steam 快捷方式", "This game isn't linked to a Steam shortcut")))?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    let original = steam::wrap(&root, &link.account, link.appid, key, &paths::runner_exe())?;
    update_game(key, |e| {
        if let Some(l) = e.steam.as_mut() {
            l.enabled = true;
            if l.original.is_none() {
                l.original = Some(original);
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// Restore the Steam shortcut to launch the game directly.
pub fn steam_disable(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    let link = g.steam.clone().ok_or_else(|| anyhow!(crate::tr!("这个游戏没有关联 Steam 快捷方式", "This game isn't linked to a Steam shortcut")))?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    if steam::find_shortcut(&root, &link.account, link.appid)?.is_some() {
        steam::unwrap(&root, &link.account, link.appid, &g, link.original.as_ref())?;
    }
    update_game(key, |e| {
        if let Some(l) = e.steam.as_mut() {
            l.enabled = false;
            l.original = None;
        }
        Ok(())
    })?;
    Ok(())
}

/// Fix a shortcut that was edited / recreated / moved: adopt its new target
/// as the game exe and wrap it again.
pub fn steam_repair(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    let root = steam::root(&cfg).ok_or_else(|| anyhow!(crate::tr!("没有找到 Steam", "Steam not found")))?;
    match steam::health(&cfg, &g) {
        steam::Health::Ok => Ok(()),
        steam::Health::Changed { exe, launch_options } => {
            let sc_dir = steam::find_shortcut(&root, &g.steam.as_ref().unwrap().account, g.steam.as_ref().unwrap().appid)?
                .map(|s| s.start_dir)
                .unwrap_or_default();
            update_game(key, |e| {
                e.exe = Some(PathBuf::from(&exe));
                e.args = launch_options.clone();
                e.work_dir = (!sc_dir.trim().is_empty()).then(|| PathBuf::from(&sc_dir));
                if let Some(l) = e.steam.as_mut() {
                    l.original = None;
                }
                Ok(())
            })?;
            steam_enable(key)
        }
        steam::Health::RunnerMoved { .. } | steam::Health::Disabled => steam_enable(key),
        steam::Health::ShortcutMissing { candidate_appid: Some(appid) } => {
            update_game(key, |e| {
                if let Some(l) = e.steam.as_mut() {
                    l.appid = appid;
                    l.original = None;
                }
                Ok(())
            })?;
            steam_repair(key)
        }
        steam::Health::ShortcutMissing { candidate_appid: None } => {
            bail!(crate::tr!("Steam 里找不到这个游戏的快捷方式了，请在 Steam 中重新添加后再关联", "The game's shortcut is no longer in Steam. Add it in Steam again, then link it"))
        }
        steam::Health::NotLinked => bail!(crate::tr!("这个游戏没有关联 Steam 快捷方式", "This game isn't linked to a Steam shortcut")),
        steam::Health::NoSteam => bail!(crate::tr!("没有找到 Steam", "Steam not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(adapter: Option<&str>, exe: Option<&str>, ludusavi: Option<&str>) -> GameEntry {
        GameEntry {
            key: "g".into(),
            name: "G".into(),
            adapter: adapter.map(str::to_string),
            exe: exe.map(PathBuf::from),
            ludusavi_title: ludusavi.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn source_kinds() {
        let s = source(&game(Some("dolphin"), None, None));
        assert_eq!(s.kind, SourceKind::Emulator);
        assert_eq!(s.name.as_deref(), Some("Dolphin"));
        assert!(s.detail.is_some());

        let s = source(&game(Some("zelda-oot-longshot"), None, None));
        assert_eq!(s.kind, SourceKind::Recomp);
        assert_eq!(s.name, None);

        let xbox = r"C:\XboxGames\Some Game\Content\gamelaunchhelper.exe";
        assert_eq!(source(&game(None, Some(xbox), Some("Some Game"))).kind, SourceKind::Xbox);
        assert_eq!(source(&game(None, Some(r"C:\Games\x.exe"), Some("X"))).kind, SourceKind::Pc);
        assert_eq!(source(&game(None, None, None)).kind, SourceKind::Custom);
        // A rule that no longer exists.
        assert_eq!(source(&game(Some("gone"), None, None)).kind, SourceKind::Custom);
    }

    #[test]
    fn source_label() {
        let mut g = game(None, None, None);
        assert_eq!(source(&g).label, None);
        g.source = Some("  RetroArch ".into());
        assert_eq!(source(&g).label.as_deref(), Some("RetroArch"));
        g.source = Some("   ".into());
        assert_eq!(source(&g).label, None);
    }
}
