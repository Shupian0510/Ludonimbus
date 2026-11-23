//! Library operations shared by the CLI, the Steam wrapper and the app.

use crate::adapters;
use crate::cloud::CloudGame;
use crate::config::{self, AppConfig, GameEntry, SteamLink};
use crate::{paths, steam, util};
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
    pub added_key: Option<String>,
    pub wrapped_key: Option<String>,
}

pub fn shortcut_candidates(cfg: &AppConfig, account: &str) -> Result<Vec<ShortcutCandidate>> {
    let root = steam::root(cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
    let (_, list) = steam::read_shortcuts(&root, account)?;
    Ok(list
        .into_iter()
        .map(|s| {
            let adapter = adapters::detect(Path::new(&s.exe));
            let added_key = cfg
                .games
                .iter()
                .find(|g| g.steam.as_ref().is_some_and(|l| l.appid == s.appid && l.account == account))
                .map(|g| g.key.clone());
            let corrupted = looks_corrupted(&s.name);
            let suggested = match (corrupted, adapter) {
                (true, Some(a)) => a.name.clone(),
                (true, None) => Path::new(&s.exe)
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "未命名游戏".into()),
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
                adapter: adapter.map(|a| a.key.clone()),
                adapter_name: adapter.map(|a| a.name.clone()),
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
}

/// Add a game from a Steam shortcut.
pub fn add_from_shortcut(account: &str, appid: u32, opts: NewGame) -> Result<GameEntry> {
    let cfg = config::load()?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
    let sc = steam::find_shortcut(&root, account, appid)?
        .ok_or_else(|| anyhow!("找不到这个 Steam 快捷方式"))?;
    if sc.wrapped_key().is_some() {
        bail!("这个快捷方式已经由 Ludonimbus 接管，但本机配置里没有它。请在 Steam 里把目标改回游戏程序后再添加。");
    }
    let exe = PathBuf::from(&sc.exe);
    let adapter_key = opts
        .adapter
        .clone()
        .or_else(|| adapters::detect(&exe).map(|a| a.key.clone()));
    let adapter = adapter_key.as_deref().and_then(adapters::find);
    if adapter.is_none() && opts.saves.is_empty() {
        bail!("无法自动识别这个游戏的存档位置，请手动指定存档文件夹");
    }
    let name = opts
        .name
        .filter(|n| !n.trim().is_empty())
        .or_else(|| (!looks_corrupted(&sc.name)).then(|| sc.name.clone()))
        .or_else(|| adapter.map(|a| a.name.clone()))
        .unwrap_or_else(|| "未命名游戏".to_string());
    let work_dir = (!sc.start_dir.trim().is_empty()).then(|| PathBuf::from(&sc.start_dir));
    config::update(|cfg| {
        if let Some(g) = cfg
            .games
            .iter()
            .find(|g| g.steam.as_ref().is_some_and(|l| l.appid == appid && l.account == account))
        {
            bail!("「{}」已经添加过了", g.name);
        }
        let base = opts
            .key
            .clone()
            .or_else(|| adapter.map(|a| a.key.clone()))
            .unwrap_or_else(|| slugify(&name));
        let key = if opts.key.is_some() { base } else { unique_key(cfg, &base) };
        if cfg.game(&key).is_some() {
            bail!("本机已经有一个 id 为 {key} 的游戏");
        }
        let entry = GameEntry {
            key,
            name,
            adapter: adapter_key.clone(),
            exe: Some(exe.clone()),
            args: sc.launch_options.clone(),
            work_dir,
            saves: opts.saves.clone(),
            steam: Some(SteamLink {
                account: account.to_string(),
                appid,
                enabled: false,
                original: None,
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
    if saves.is_empty() && ludusavi_title.is_none() {
        bail!("请至少指定一个存档位置");
    }
    let adapter = exe.as_deref().and_then(adapters::detect).map(|a| a.key.clone());
    config::update(|cfg| {
        let key = match key {
            Some(k) if cfg.game(&k).is_none() => k,
            Some(k) => bail!("本机已经有一个 id 为 {k} 的游戏"),
            None => unique_key(cfg, &slugify(name)),
        };
        let entry = GameEntry {
            key,
            name: name.to_string(),
            adapter: if saves.is_empty() { adapter } else { None },
            exe,
            saves,
            ludusavi_title,
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
            },
        )?;
        return update_game(&g.key, |e| {
            e.exclude = cg.exclude.clone();
            e.wait_for = cg.wait_for.clone();
            e.roots = cg.roots.clone();
            e.ludusavi_title = cg.ludusavi_title.clone();
            Ok(())
        });
    }
    config::update(|cfg| {
        if cfg.game(&cg.key).is_some() {
            bail!("本机已经添加过「{}」", cg.name);
        }
        let entry = GameEntry {
            key: cg.key.clone(),
            name: cg.name.clone(),
            adapter: cg.adapter.clone(),
            exe,
            saves: cg.saves.clone(),
            exclude: cg.exclude.clone(),
            wait_for: cg.wait_for.clone(),
            roots: cg.roots.clone(),
            ludusavi_title: cg.ludusavi_title.clone(),
            auto_sync: true,
            ..Default::default()
        };
        cfg.games.push(entry.clone());
        Ok(entry)
    })
}

pub fn update_game(key: &str, f: impl FnOnce(&mut GameEntry) -> Result<()>) -> Result<GameEntry> {
    config::update(|cfg| {
        let g = cfg.game_mut(key).ok_or_else(|| anyhow!("找不到游戏 {key}"))?;
        f(g)?;
        Ok(g.clone())
    })
}

/// Point a game at a new executable (after an update / move).
pub fn set_exe(key: &str, exe: &Path) -> Result<GameEntry> {
    if !exe.is_file() {
        bail!("文件不存在：{}", exe.display());
    }
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
        Ok(())
    })
}

pub fn remove(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?;
    if g.steam.as_ref().is_some_and(|l| l.enabled) {
        bail!("请先关闭这个游戏的 Steam 自动同步（会把快捷方式恢复原样），再移除");
    }
    config::update(|cfg| {
        cfg.games.retain(|g| g.key != key);
        Ok(())
    })?;
    crate::state::update(|s| {
        s.games.remove(key);
    })
}

/// Make the Steam shortcut launch through `ludonimbus-run`.
pub fn steam_enable(key: &str) -> Result<()> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?;
    let link = g.steam.clone().ok_or_else(|| anyhow!("这个游戏没有关联 Steam 快捷方式"))?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
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
    let link = g.steam.clone().ok_or_else(|| anyhow!("这个游戏没有关联 Steam 快捷方式"))?;
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
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
    let root = steam::root(&cfg).ok_or_else(|| anyhow!("没有找到 Steam"))?;
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
            bail!("Steam 里找不到这个游戏的快捷方式了，请在 Steam 中重新添加后再关联")
        }
        steam::Health::NotLinked => bail!("这个游戏没有关联 Steam 快捷方式"),
        steam::Health::NoSteam => bail!("没有找到 Steam"),
    }
}
