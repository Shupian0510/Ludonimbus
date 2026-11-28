//! What the user does with the versions of a game, outside a sync: keep the
//! saves as they are now ([`backup_now`]), restore a version and make it the
//! newest ([`restore`], [`undo`]), change the saves from outside
//! ([`apply_external`]), keep and name a version, delete one into
//! 「最近删除」, put it back from there or delete it for good, and save one
//! as a zip.

use super::carry::with_carried;
use super::gate::Gate;
use super::housekeeping::{best_effort_housekeeping, tell_base};
use super::lock::lock_game;
use super::publish::{flush, ready_to_write};
use super::queue::{stage_local, Played, Shot};
use super::run::fresh;
use super::session::Session;
use super::{local_of, note_replaced, Ctx, Outcome};
use crate::cloud;
use crate::cloud::decide::{self, Action};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Place};
use crate::config::GameEntry;
use crate::snapshot::{self, SnapshotSummary, Trigger};
use crate::state;
use anyhow::{anyhow, bail, Result};
use chrono::Utc;

/// Keep the saves as they are now for good, under a name (a kept save).
///
/// When the cloud already has a version with this content, that one is
/// kept. Otherwise a version is uploaded: the newest of the line when the
/// saves here are what the line would get next anyway, and beside the
/// history when not (the cloud has something newer, or there is a conflict
/// to settle), so that keeping a save never decides anything for the user.
pub fn backup_now(ctx: &Ctx, game: &GameEntry, comment: &str) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let local = snapshot::local_state(game)?;
    if local.is_empty() {
        bail!(crate::tr!("没有找到「{}」的存档文件", "No save files of \"{}\" found", game.name));
    }
    let cloud = ctx.cloud()?;
    let mut s = match Session::open(ctx, cloud, game) {
        Ok(s) => s,
        Err(e) if cloud::is_unavailable(&e) || crate::pause::is_paused_error(&e) => {
            // Not reachable, or syncing is paused: it waits in the queue,
            // built on what this device synced last.
            let local = with_carried(None, game, None, false, local)?;
            let st = state::game(&game.key);
            let place = match &st.line {
                Some(l) => Place { game: l.game.clone(), line: l.line.clone(), folders: l.folders.clone(), dirs: l.dirs.clone() },
                None => Place::of_key(&game.key),
            };
            let shot = Shot {
                bases: vec![st.last_synced_vv.clone()],
                parent: st.last_synced_id.clone(),
                trigger: Some(Trigger::Manual),
                comment: comment.to_string(),
                ..Shot::default()
            };
            let manifest = stage_local(ctx, game, &place, &LineView::default(), &local, &shot)?;
            return Ok(Outcome::Queued { id: manifest.id, reason: format!("{e:#}") });
        }
        Err(e) => return Err(e),
    };
    flush(cloud, &mut s.line, Some(game), None)?;
    let local = with_carried(Some(cloud), game, Some((&s.line.place, &s.line.view)), true, local)?;
    let same = s.line.view.snapshots.iter().rev().find(|x| x.content_hash == local.hash).map(|x| x.id.clone());
    let outcome = if let Some(id) = same {
        ready_to_write(cloud, &mut s.line, Some(game))?;
        saves::keep(cloud.files(), &s.line.place, &s.line.view, &id, comment, cloud.device_id(), Utc::now())?;
        Outcome::Uploaded { id }
    } else {
        let loc = local_of(&local, &state::game(&game.key));
        let shot = match decide::decide(&loc, &s.line.view) {
            Action::Push { bases, parent } => Shot { bases, parent, ..Shot::default() },
            _ => Shot { parent: loc.base.as_ref().map(|b| b.id.clone()), side: true, ..Shot::default() },
        };
        s.push(&local, Shot { trigger: Some(Trigger::Manual), comment: comment.to_string(), ..shot }, false)?
    };
    best_effort_housekeeping(ctx, cloud, game);
    Ok(outcome)
}

/// Restore a specific snapshot and make it the newest version.
pub fn restore(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let cloud = ctx.cloud()?;
    let mut s = Session::open(ctx, cloud, game)?;
    flush(cloud, &mut s.line, Some(game), None)?;
    let local = s.local()?;
    let before = state::game(&game.key).last_synced_id;
    let pulled = s.pull(&local, id)?;
    // Make the restored content the head so the next sync keeps it.
    let outcome = s.make_current(id, Trigger::Restore, false)?.unwrap_or(pulled);
    tell_base(ctx, cloud, &game.key, before.as_deref(), &s.line.view, local.total_size);
    Ok(outcome)
}

/// What [`undo`] would take back: the last time saves of the cloud replaced
/// the ones here, while the device is still at what they were replaced with.
pub fn undoable(key: &str) -> Option<state::Replaced> {
    let st = state::game(key);
    st.replaced.filter(|r| st.last_synced_id.as_deref() == Some(r.now.as_str()))
}

/// Take back the last time saves of the cloud replaced the ones here (a
/// sync that brought another device's progress, "make current"): the saves
/// that were here are current again, as with [`restore`] of the version
/// that holds them. What is here now is kept the same way, so undoing can
/// itself be undone.
pub fn undo(ctx: &Ctx, game: &GameEntry) -> Result<Outcome> {
    let Some(r) = undoable(&game.key) else {
        bail!(crate::tr!("没有可以撤销的更改", "There is nothing to undo"));
    };
    restore(ctx, game, &r.back)
}

/// Change a game's saves from outside a sync (the save library's "apply
/// to…"): keep the current saves in the cloud first (a pre-restore version),
/// run `change`, then upload the result as the newest version.
pub fn apply_external(ctx: &Ctx, game: &GameEntry, comment: &str, copy_of: Option<CopyOf>, change: impl FnOnce() -> Result<()>) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let cloud = ctx.cloud()?;
    let mut s = Session::open(ctx, cloud, game)?;
    flush(cloud, &mut s.line, Some(game), None)?;
    let back = match s.local() {
        Ok(local) => s.protect(&local, Trigger::PreRestore)?,
        Err(_) => None,
    };
    change()?;
    let after = s.local()?;
    if after.is_empty() {
        bail!(crate::tr!("应用后没有找到「{}」的存档文件", "No save files of \"{}\" found after applying", game.name));
    }
    let loc = local_of(&after, &state::game(&game.key));
    let Action::Push { bases, parent } = decide::settle(&loc, &s.line.view) else { return Ok(Outcome::NothingToSave) };
    let (origin, played, device) = match copy_of {
        Some(c) => (c.origin, Played::Was(c.playtime), c.device),
        None => (None, Played::Now, String::new()),
    };
    let shot = Shot { bases, parent, trigger: Some(Trigger::Restore), comment: comment.to_string(), origin, played, ..Shot::default() };
    let outcome = s.push(&after, shot, false)?;
    // Taken back like any other time saves from elsewhere replaced these.
    if let Outcome::Uploaded { id } | Outcome::Queued { id, .. } = &outcome {
        note_replaced(&game.key, back, id, &device);
    }
    best_effort_housekeeping(ctx, cloud, game);
    Ok(outcome)
}

/// The version of another game's line that saves [`apply_external`] writes
/// were made from: where it is (`origin`), how long the game had been
/// played then ([`crate::cloud::v2::PLAYTIME`]), and the device that made it.
/// Where the cloud couldn't say, the first two are `None`.
pub struct CopyOf {
    pub origin: Option<crate::cloud::v2::Origin>,
    pub playtime: Option<u64>,
    pub device: String,
}

/// Keep a version or stop keeping it (`locked`), and name it (`comment`).
/// A name given to a version that is not kept keeps it.
pub fn set_snapshot_flags(ctx: &Ctx, game: &GameEntry, id: &str, locked: Option<bool>, comment: Option<String>) -> Result<()> {
    let cloud = ctx.cloud()?;
    let mut line = cloud.open(&game.key)?;
    ready_to_write(cloud, &mut line, Some(game))?;
    let entry = line.view.get(id).cloned().ok_or_else(|| anyhow!(crate::tr!("找不到这个版本", "This version wasn't found")))?;
    let kept = entry.pin.is_some();
    if locked == Some(false) {
        if kept {
            saves::unkeep(cloud.files(), &line.place, id)?;
        }
        return Ok(());
    }
    let name = match (&comment, &entry.pin) {
        (Some(c), _) => c.clone(),
        (None, Some(pin)) => pin.name.clone(),
        (None, None) => entry.old_note().unwrap_or_default().to_string(),
    };
    let wanted = locked == Some(true) || comment.as_ref().is_some_and(|c| kept || !c.trim().is_empty());
    if wanted && (!kept || comment.is_some()) {
        saves::keep(cloud.files(), &line.place, &line.view, id, &name, cloud.device_id(), Utc::now())?;
    }
    Ok(())
}

pub fn delete_snapshot(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<()> {
    let cloud = ctx.cloud()?;
    let mut line = cloud.open(&game.key)?;
    ready_to_write(cloud, &mut line, Some(game))?;
    // Into 「最近删除」, restorable for TRASH_DAYS days.
    saves::trash(cloud.files(), &line.place, &line.view, id, &ctx.cfg.device.name, Utc::now())
}

/// Versions in 「最近删除」, newest deletion first.
pub fn deleted_snapshots(ctx: &Ctx, game: &GameEntry) -> Result<Vec<cloud::TrashedSnapshot>> {
    let cloud = ctx.cloud()?;
    let place = cloud.place(&game.key)?;
    Ok(saves::trashed(cloud.files(), &place)?
        .into_iter()
        .map(|t| cloud::TrashedSnapshot {
            snapshot: SnapshotSummary::of(&t.snapshot, false),
            deleted_at: t.deleted_at,
            deleted_by: t.deleted_by,
            reason: t.reason,
        })
        .collect())
}

/// Put a version from 「最近删除」 back into the history. An automatic one is
/// kept too, or retention could delete it again right away. Returns whether
/// it was.
pub fn restore_deleted(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<bool> {
    let cloud = ctx.cloud()?;
    let mut line = cloud.open(&game.key)?;
    ready_to_write(cloud, &mut line, Some(game))?;
    saves::untrash(cloud.files(), &line.place, id)?;
    cloud.reopen(&mut line)?;
    let Some(entry) = line.view.get(id).cloned() else { return Ok(false) };
    if entry.pin.is_some() {
        return Ok(false);
    }
    let name = entry.old_note().unwrap_or_default().to_string();
    saves::keep(cloud.files(), &line.place, &line.view, id, &name, cloud.device_id(), Utc::now())?;
    Ok(true)
}

/// Delete a version in 「最近删除」 for good.
pub fn purge_deleted(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<()> {
    let cloud = ctx.cloud()?;
    let mut line = cloud.open(&game.key)?;
    ready_to_write(cloud, &mut line, Some(game))?;
    saves::purge(cloud.files(), &line.place, id)
}

/// Save a snapshot zip to a user-chosen location.
pub fn export_snapshot(ctx: &Ctx, game: &GameEntry, id: &str, dest: &std::path::Path) -> Result<()> {
    ctx.cloud()?.download(&game.key, id, dest)
}
