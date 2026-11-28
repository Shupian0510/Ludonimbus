//! A game's saves: check, sync and back up; the versions in the cloud
//! (restore, keep or name, delete, put back from 「最近删除」); saves as files
//! to keep or give away; and the save files on this PC.

use super::overview::{platform_marks, PlatformMark};
use super::{blocking, restoring_while_running, with_free_game, with_free_line, with_game, CmdResult};
use anyhow::{anyhow, bail};
use ludonimbus_core::cloud;
use ludonimbus_core::config;
use ludonimbus_core::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use ludonimbus_core::state;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, StatusReport};
use ludonimbus_core::{ludusavi, savelib, share, spec};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

pub(crate) fn cache_check(key: &str, report: &StatusReport) {
    let v = serde_json::to_value(report).ok();
    let _ = state::update_game(key, |g| g.last_check = v);
}

#[tauri::command]
pub async fn check_game(app: tauri::AppHandle, key: String) -> CmdResult<StatusReport> {
    blocking(move || {
        with_game(&key, |ctx, g| {
            let r = sync::check(ctx, g);
            cache_check(&key, &r);
            crate::notify::after_check(&app, ctx.cfg.cloud_newer_notifications, g, &r);
            Ok(r)
        })
    })
    .await
}

/// What 「全部同步」 did with one game: the saves here uploaded, nothing
/// written into them ([`sync::upload`]), and how the game stands now.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    outcome: Outcome,
    check: StatusReport,
}

/// One game of 「全部同步」: upload what changed here, then look at how it
/// stands (newer saves in the cloud and conflicts are for the user to
/// choose, game by game).
#[tauri::command]
pub async fn upload_game(key: String) -> CmdResult<UploadResult> {
    blocking(move || {
        let running = ludonimbus_core::tr!("游戏正在运行，退出游戏后会自动同步", "The game is running; saves sync after you quit");
        let outcome = with_free_game(&key, running, sync::upload)?;
        let check = with_game(&key, |ctx, g| Ok(sync::check(ctx, g)))?;
        cache_check(&key, &check);
        Ok(UploadResult { outcome, check })
    })
    .await
}

#[tauri::command]
pub async fn sync_game(key: String, resolution: String) -> CmdResult<Outcome> {
    blocking(move || {
        let res = match resolution.as_str() {
            "local" => Resolution::UseLocal,
            "cloud" => Resolution::UseCloud,
            _ => Resolution::Auto,
        };
        let running = ludonimbus_core::tr!("游戏正在运行，退出游戏后会自动同步", "The game is running; saves sync after you quit");
        with_free_game(&key, running, |ctx, g| sync::sync(ctx, g, res, Trigger::SyncNow))
    })
    .await
}

#[tauri::command]
pub async fn backup_game(key: String, comment: String) -> CmdResult<Outcome> {
    blocking(move || {
        let running = ludonimbus_core::tr!("游戏正在运行，请先退出游戏", "The game is running. Quit it first");
        with_free_game(&key, running, |ctx, g| sync::backup_now(ctx, g, comment.trim()))
    })
    .await
}

/// Take back the last time saves of the cloud replaced the ones here.
#[tauri::command]
pub async fn undo_replace(key: String) -> CmdResult<Outcome> {
    blocking(move || with_free_game(&key, restoring_while_running(), sync::undo)).await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotList {
    snapshots: Vec<SnapshotSummary>,
    head_id: Option<String>,
    last_synced_id: Option<String>,
    /// How to show each platform the versions name.
    platforms: BTreeMap<String, PlatformMark>,
    /// The versions saved on another platform by a newer game than the one
    /// that wrote the saves here, which the game's rule says an older game
    /// can't load ([`sync::too_new`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    too_new: Vec<String>,
    /// Versions made on this PC that couldn't be uploaded complete
    /// ([`sync::unsent`]): listed once, as this PC's, until dismissed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    unsent: Vec<state::Unsent>,
}

/// The ids among `list` that [`sync::too_new`] says this device's saves of
/// the game `key` can't take as they are. Nothing is read unless the game's
/// rule says an older game can't load a newer save.
pub(super) fn too_new_ids<'a>(cfg: &config::AppConfig, key: &str, list: impl IntoIterator<Item = &'a SnapshotSummary>) -> Vec<String> {
    let Some(g) = cfg.game(key) else { return Vec::new() };
    let Some(here) = sync::saves_version(g) else { return Vec::new() };
    list.into_iter().filter(|s| sync::too_new(g, Some(&here), &s.platform, s.game_version.as_deref())).map(|s| s.id.clone()).collect()
}

#[tauri::command]
pub async fn list_snapshots(key: String) -> CmdResult<SnapshotList> {
    // Also for games only the cloud has.
    blocking(move || {
        let ctx = Ctx::load()?;
        let mut list = ctx.cloud()?.list_snapshots(&key)?;
        let head = sync::mainline_head(&list).map(|h| h.id);
        list.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        let platforms = platform_marks(&ctx.cfg, list.iter().map(|s| s.platform.as_str()));
        let too_new = too_new_ids(&ctx.cfg, &key, &list);
        let unsent = sync::unsent(&key);
        Ok(SnapshotList { snapshots: list, head_id: head, last_synced_id: state::game(&key).last_synced_id, platforms, too_new, unsent })
    })
    .await
}

/// Save what a version that couldn't be uploaded complete brought itself
/// (`sync::export_unsent`) as a zip where the user picks. `None` when no
/// place was picked.
#[tauri::command]
pub async fn export_unsent(key: String, id: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let Some(dest) = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("导出存档", "Export a save"))
            .set_file_name(format!("{id}.zip"))
            .add_filter("zip", &["zip"])
            .save_file()
        else {
            return Ok(None);
        };
        sync::export_unsent(&key, &id, &dest)?;
        Ok(Some(dest.display().to_string()))
    })
    .await
}

/// Stop listing a version that couldn't be uploaded complete.
#[tauri::command]
pub async fn dismiss_unsent(key: String, id: String) -> CmdResult<()> {
    blocking(move || sync::dismiss_unsent(&key, &id)).await
}

#[tauri::command]
pub async fn get_snapshot(key: String, id: String) -> CmdResult<SnapshotMeta> {
    blocking(move || Ctx::load()?.cloud()?.get_meta(&key, &id)).await
}

#[tauri::command]
pub async fn restore_snapshot(key: String, id: String) -> CmdResult<Outcome> {
    blocking(move || with_free_game(&key, restoring_while_running(), |ctx, g| sync::restore(ctx, g, &id))).await
}

#[tauri::command]
pub async fn set_snapshot(key: String, id: String, locked: Option<bool>, comment: Option<String>) -> CmdResult<()> {
    blocking(move || with_free_line(&key, |ctx, g| sync::set_snapshot_flags(ctx, g, &id, locked, comment))).await
}

/// Move a version to 「最近删除」 (restorable for 30 days).
#[tauri::command]
pub async fn delete_snapshot(key: String, id: String) -> CmdResult<()> {
    blocking(move || with_free_line(&key, |ctx, g| sync::delete_snapshot(ctx, g, &id))).await
}

#[tauri::command]
pub async fn deleted_snapshots(key: String) -> CmdResult<Vec<cloud::TrashedSnapshot>> {
    blocking(move || with_game(&key, sync::deleted_snapshots)).await
}

/// Returns whether the version was locked so retention keeps it.
#[tauri::command]
pub async fn restore_deleted_snapshot(key: String, id: String) -> CmdResult<bool> {
    blocking(move || with_free_line(&key, |ctx, g| sync::restore_deleted(ctx, g, &id))).await
}

#[tauri::command]
pub async fn purge_deleted_snapshot(key: String, id: String) -> CmdResult<()> {
    blocking(move || with_free_line(&key, |ctx, g| sync::purge_deleted(ctx, g, &id))).await
}

/// Save one version as a file to keep or to give to someone
/// (`share::export`): a zip of its files that says which game it is of and
/// nothing of this PC. `None` when no place was picked.
#[tauri::command]
pub async fn export_snapshot(key: String, id: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let ctx = Ctx::load()?;
        // A game on this device, or one only the cloud has.
        let (name, ids) = match ctx.cfg.game(&key) {
            Some(g) => (g.name.clone(), savelib::ids(g)),
            None => {
                let cg = savelib::cached(&ctx.cfg).games.into_iter().find(|c| c.key == key);
                let cg = cg.ok_or_else(|| anyhow!(ludonimbus_core::tr!("找不到这个游戏", "This game wasn't found")))?;
                (cg.name.clone(), savelib::cloud_ids(&cg))
            }
        };
        let safe: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
        let Some(dest) = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("导出存档", "Export a save"))
            .set_file_name(format!("{}.{}", safe.trim(), share::EXT))
            .add_filter(ludonimbus_core::tr!("Ludonimbus 存档", "Ludonimbus save"), &[share::EXT])
            .save_file()
        else {
            return Ok(None);
        };
        share::export_as(&ctx, &key, &name, ids, &id, &dest)?;
        Ok(Some(dest.display().to_string()))
    })
    .await
}

/// Take in a save someone exported (a `.lnsave` file the user picks): it
/// goes beside the game's history, kept, and changes nothing until it is
/// made current. Returns its id; `None` when no file was picked.
#[tauri::command]
pub async fn import_shared_save(key: String) -> CmdResult<Option<String>> {
    blocking(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title(ludonimbus_core::tr!("导入存档", "Import a save"))
            .add_filter(ludonimbus_core::tr!("Ludonimbus 存档", "Ludonimbus save"), &[share::EXT])
            .pick_file()
        else {
            return Ok(None);
        };
        with_game(&key, |ctx, g| share::import(ctx, g, &path)).map(|m| Some(m.id))
    })
    .await
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalFiles {
    files: Vec<snapshot::LocalFile>,
    roots: BTreeMap<String, String>,
}

#[tauri::command]
pub async fn local_files(key: String) -> CmdResult<LocalFiles> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        let (files, roots) = snapshot::scan_own(g)?;
        let used: std::collections::HashSet<&str> = files.iter().filter_map(|f| f.logical.split('/').next()).collect();
        let mut shown: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in &roots {
            if used.contains(k.as_str()) || k == "install" {
                shown.insert(k.clone(), v.display().to_string());
            }
        }
        Ok(LocalFiles { files, roots: shown })
    })
    .await
}

/// Open a web page in the browser or a folder in the file manager, or show
/// a file in its folder. Nothing else: a program or a script is shown, never
/// run, and names the system would look up (`calc`, `ms-settings:`) are refused.
#[tauri::command]
pub async fn open_path(path: String) -> CmdResult<()> {
    blocking(move || {
        if let Ok(url) = tauri::Url::parse(&path) {
            if matches!(url.scheme(), "http" | "https") {
                opener::open_browser(url.as_str())?;
                return Ok(());
            }
        }
        let p = PathBuf::from(&path);
        if p.is_absolute() && p.is_dir() {
            opener::open(&p)?;
        } else if p.is_absolute() && p.is_file() {
            opener::reveal(&p)?;
        } else {
            bail!(ludonimbus_core::tr!("找不到「{path}」", "Can't find {path}"));
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn open_save_folder(key: String) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?;
        let eff = spec::effective(g);
        let roots = spec::resolve_roots(g, &eff)?;
        for pattern in &eff.saves {
            let (root, rest) = spec::split_pattern(pattern)?;
            let Some(base) = spec::resolve_root_name(&root, &roots) else { continue };
            let literal: PathBuf = rest.split('/').take_while(|c| !c.contains(['*', '?', '[', '{'])).fold(base, |p, c| p.join(c));
            let target = literal.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf);
            if let Some(t) = target {
                opener::open(&t)?;
                return Ok(());
            }
        }
        if let Some(title) = &eff.ludusavi_title {
            if let Some(f) = ludusavi::resolve_files(title)?.first() {
                opener::reveal(f)?;
                return Ok(());
            }
        }
        bail!(ludonimbus_core::tr!("还没有找到存档文件夹", "No save folder found yet"))
    })
    .await
}
