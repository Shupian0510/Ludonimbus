//! After a restore: point the game (and its Steam shortcut) at the new
//! folder, then pull its saves from the cloud.

use super::format::Manifest;
use super::restore::safe_join;
use crate::config::{self, GameEntry};
use crate::snapshot::Trigger;
use crate::sync::{self, Ctx, Outcome, Resolution};
use crate::{games, util};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

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
    pub shortcut: ShortcutFix,
}

fn same(a: &Path, b: &str) -> bool {
    util::same_path(&a.to_string_lossy(), b)
}

/// Point game `key` at its files restored into `target`.
pub fn fix_paths(key: &str, manifest: &Manifest, target: &Path) -> Result<FixReport> {
    if let Some(rel) = &manifest.exe {
        let exe = safe_join(target, rel)?;
        if exe.is_file() {
            games::set_exe(key, &exe)?;
        }
    }
    let g = games::update_game(key, |g| {
        // Folders that pointed at the backed-up folder follow it.
        if g.install.as_deref().is_some_and(|i| same(i, &manifest.root)) {
            g.install = Some(target.to_path_buf());
        }
        match &manifest.work_dir {
            Some(rel) => g.work_dir = Some(safe_join(target, rel)?),
            None if g.work_dir.as_deref().is_some_and(|w| same(w, &manifest.root)) => g.work_dir = Some(target.to_path_buf()),
            None => {}
        }
        Ok(())
    })?;
    let shortcut = fix_shortcut(&g);
    Ok(FixReport { exe: g.exe.as_ref().map(|e| e.display().to_string()), shortcut })
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
