//! After a restore: point the game (and its Steam shortcut) at the restored
//! program and game files, then pull its saves from the cloud.

use super::components::{split, Bases};
use super::format::{Kind, Manifest};
use super::restore::{bases_for, Targets};
use crate::config::{self, GameEntry};
use crate::snapshot::Trigger;
use crate::sync::{self, Ctx, Outcome, Resolution};
use crate::{games, util};
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutFix {
    /// No Steam shortcut linked.
    NotLinked,
    /// It launches through `ludonimbus-run`, which reads the new path itself.
    Wrapped,
    /// The shortcut now points at the new folder.
    Updated,
    /// Steam is running; the shortcut can be updated once it is closed.
    NeedsSteamClosed,
    /// Updating failed (message).
    Failed(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixReport {
    /// The game's program after the fix.
    pub exe: Option<String>,
    /// The launch options changed (the game's files moved).
    pub args_changed: bool,
    pub shortcut: ShortcutFix,
}

/// Where a logical path was when backing up (`None` for user folders,
/// which mean the same place on every PC).
fn original(m: &Manifest, logical: &str) -> Option<PathBuf> {
    let (name, rest) = split(logical)?;
    let base = m.components.iter().find_map(|c| c.bases.get(name))?;
    let mut p = PathBuf::from(base);
    for part in rest.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    Some(p)
}

/// Replace `old` with `new` in launch options, ignoring case and the
/// direction of slashes (each match keeps the slash style it had).
fn replace_path(args: &str, old: &Path, new: &Path) -> String {
    let fold = |s: &str| s.replace('/', "\\").to_lowercase();
    let (hay, needle) = (fold(args), fold(&old.display().to_string()));
    // Lower-casing changed some lengths (rare letters): exact matches only.
    if needle.is_empty() || hay.len() != args.len() || needle.len() != old.display().to_string().len() {
        return args.replace(&old.display().to_string(), &new.display().to_string());
    }
    let mut out = String::with_capacity(args.len());
    let mut at = 0;
    while let Some(i) = hay[at..].find(&needle) {
        let found = &args[at + i..at + i + needle.len()];
        let mut n = new.display().to_string();
        if found.contains('/') && !found.contains('\\') {
            n = n.replace('\\', "/");
        }
        out.push_str(&args[at..at + i]);
        out.push_str(&n);
        at += i + needle.len();
    }
    out.push_str(&args[at..]);
    out
}

fn same(a: &Path, b: &Path) -> bool {
    util::same_path(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Point game `key` at what was restored to `targets`.
pub fn fix_paths(key: &str, m: &Manifest, targets: &Targets) -> Result<FixReport> {
    let bases: Bases = bases_for(m, targets);
    let program = m.components.iter().find(|c| matches!(c.kind, Kind::Program | Kind::Folder));
    if let Some(exe) = program.and_then(|c| c.exe.as_deref()) {
        let new = bases.resolve(exe)?;
        if new.is_file() {
            games::set_exe(key, &new)?;
        }
    }
    let old_program = program.and_then(|c| c.bases.get("program")).map(PathBuf::from);
    let moved_source = m
        .component(Kind::Content)
        .filter(|_| targets.content.is_some())
        .and_then(|c| c.source.as_deref())
        .and_then(|s| Some((original(m, s)?, bases.resolve(s).ok()?)));
    let mut args_changed = false;
    let g = games::update_game(key, |g| {
        if let (Some(old), Some(i)) = (&old_program, &g.install) {
            if same(i, old) {
                g.install = Some(targets.program.clone());
            }
        }
        if let (Some(old), Some(w)) = (&old_program, &g.work_dir) {
            if same(w, old) {
                g.work_dir = Some(targets.program.clone());
            }
        }
        if let Some((old, new)) = &moved_source {
            if !same(old, new) {
                let args = replace_path(&g.args, old, new);
                args_changed = args != g.args;
                g.args = args;
            }
        }
        Ok(())
    })?;
    if let (Some(a), Some(c)) = (&g.adapter, &targets.content) {
        super::remember_content_dir(a, c);
    }
    let shortcut = fix_shortcut(&g);
    Ok(FixReport { exe: g.exe.as_ref().map(|e| e.display().to_string()), args_changed, shortcut })
}

fn fix_shortcut(g: &GameEntry) -> ShortcutFix {
    let Some(link) = &g.steam else { return ShortcutFix::NotLinked };
    if link.enabled {
        return ShortcutFix::Wrapped;
    }
    // Pointing a shortcut that launches the game directly at the new place
    // is what "turn off Steam auto-sync" does.
    match games::steam_disable(&g.key) {
        Ok(()) => ShortcutFix::Updated,
        Err(e) if format!("{e:#}").contains("STEAM_RUNNING") => ShortcutFix::NeedsSteamClosed,
        Err(e) => ShortcutFix::Failed(format!("{e:#}")),
    }
}

/// Pull the newest saves of `key` (like "sync now"; a conflict is reported,
/// not resolved).
pub fn pull_saves(key: &str) -> Result<Outcome> {
    let ctx = Ctx::from_config(config::load()?);
    let g = ctx.game(key)?;
    sync::sync(&ctx, &g, Resolution::Auto, Trigger::SyncNow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_the_game_path_in_launch_options() {
        let old = Path::new("D:\\ROMs\\WiiU\\Zelda");
        let new = Path::new("E:\\Games\\Zelda");
        assert_eq!(replace_path("-g \"d:\\roms\\wiiu\\zelda\\code\\x.rpx\" -f", old, new), "-g \"E:\\Games\\Zelda\\code\\x.rpx\" -f");
        assert_eq!(replace_path("-g D:/ROMs/WiiU/Zelda/code/x.rpx", old, new), "-g E:/Games/Zelda/code/x.rpx");
        assert_eq!(replace_path("--fullscreen", old, new), "--fullscreen");
        // Mixed slashes: the match has a backslash, so the new path keeps them.
        assert_eq!(replace_path("-g \"D:\\ROMs/WiiU\\Zelda/code/x.rpx\"", old, new), "-g \"E:\\Games\\Zelda/code/x.rpx\"");
    }
}
