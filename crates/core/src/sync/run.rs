//! Running one sync of a game ([`sync`], [`sync_with`]): the game as it is
//! settled now ([`fresh`]), one sync at a time, what waits in the queue
//! first, then the decision ([`decide::decide`]) and what this device makes
//! of it. A sync nobody chose ([`Resolution::Auto`]) also goes by the game:
//! [`backup_only`], [`not_into_containers`], [`not_too_new`]. After playing
//! comes the bookkeeping. And [`check`]: the same decision, with nothing
//! changed.

use super::carry::{local_here, scope_of_platform, too_new, with_carried};
use super::gate::{is_gave_up, Gate, GaveUp, Phase};
use super::housekeeping::{best_effort_housekeeping, clean_if_due, tell_base};
use super::incomplete::repair_marked;
use super::lock::{lock_game, settle_owed};
use super::publish::flush;
use super::queue::{other_side, queued, stage_local, with_queue, Shot};
use super::session::Session;
use super::{holding, local_of, log, mark_synced, must_wait, remember_base, Ctx, Outcome, Resolution, Status, StatusReport};
use crate::cloud::decide::{self, Action, Local};
use crate::cloud::line::LineView;
use crate::cloud::saves::Place;
use crate::cloud::{self, line, v2};
use crate::config::{self, GameEntry};
use crate::snapshot::{self, LocalState, SnapshotSummary, Trigger};
use crate::state;
use anyhow::{bail, Result};
use chrono::Utc;

/// When the cloud is unreachable: after playing, keep the new saves in the
/// queue so they are uploaded later.
fn offline(ctx: &Ctx, game: &GameEntry, trigger: Trigger, err: anyhow::Error) -> Result<Outcome> {
    let message = format!("{err:#}");
    log(format!("{}: offline ({message})", game.key));
    let _ = state::update_game(&game.key, |g| g.last_error = Some(message.clone()));
    if matches!(trigger, Trigger::PreLaunch) {
        return Ok(Outcome::Offline { message });
    }
    let local = local_here(None, game, None, false)?;
    let st = state::game(&game.key);
    if local.is_empty() || st.last_synced_hash.as_deref() == Some(local.hash.as_str()) {
        return Ok(Outcome::Offline { message });
    }
    // Built on what this device synced last, and complete: what it would
    // share files with may be gone by the time this is uploaded.
    let place = match &st.line {
        Some(l) => Place { game: l.game.clone(), line: l.line.clone(), folders: l.folders.clone(), dirs: l.dirs.clone() },
        None => Place::of_key(&game.key),
    };
    let shot = Shot { bases: vec![st.last_synced_vv.clone()], parent: st.last_synced_id.clone(), trigger: Some(trigger), ..Shot::default() };
    let manifest = stage_local(ctx, game, &place, &LineView::default(), &local, &shot)?;
    Ok(Outcome::Queued { id: manifest.id, reason: message })
}

/// Sync one game. `trigger` labels a snapshot uploaded by this call.
pub fn sync(ctx: &Ctx, game: &GameEntry, resolution: Resolution, trigger: Trigger) -> Result<Outcome> {
    sync_with(ctx, game, resolution, trigger, &Gate::default())
}

/// [`sync`], for a caller that may stop waiting for it ([`Gate`]). Another
/// sync of the game on this device is waited for first ([`lock_game`]),
/// and that wait can be given up too.
pub fn sync_with(ctx: &Ctx, game: &GameEntry, resolution: Resolution, trigger: Trigger, gate: &Gate) -> Result<Outcome> {
    gate.set_phase(Phase::Checking);
    let result = gate.check().and_then(|_| {
        let _lock = lock_game(&game.key, gate)?;
        let outcome = sync_locked(ctx, game, resolution, trigger, gate)?;
        // A sync after playing that was owed is done by any sync that ends.
        if outcome.is_final() {
            settle_owed(&game.key);
        }
        Ok(outcome)
    });
    gate.set_phase(Phase::Done);
    match result {
        Err(e) if is_gave_up(&e) => {
            log(format!("{}: not waited for, stopped", game.key));
            Ok(Outcome::Offline { message: GaveUp.to_string() })
        }
        other => other,
    }
}

/// The game as it is settled now. Which record it belongs to and through
/// which folders it reads that record's line ([`GameEntry::record`],
/// [`GameEntry::bind`]) may have changed since `game` was read: by an
/// earlier sync in this process (the launch wrapper reads the game once,
/// before the first), or by another process. Scanning or writing saves with
/// what was true before would put one version's files into the line of
/// another, so everything that does either starts here. The configuration
/// on this device is what counts. So it does for whether Steam Cloud keeps
/// the saves of a game Steam installed ([`GameEntry::store`]): Steam Cloud
/// may have been found on again since ([`crate::steamcloud::recheck`]).
pub(super) fn fresh(ctx: &Ctx, game: &GameEntry) -> GameEntry {
    let now = config::load().ok().and_then(|c| c.game(&game.key).cloned());
    let Some(now) = now else { return game.clone() };
    let game = GameEntry { record: now.record, bind: now.bind, declined: now.declined, store: now.store, ..game.clone() };
    if let Ok(cloud) = ctx.cloud() {
        cloud.know(&game);
    }
    game
}

fn sync_locked(ctx: &Ctx, game: &GameEntry, resolution: Resolution, trigger: Trigger, gate: &Gate) -> Result<Outcome> {
    // A game Steam installed is managed only while Steam Cloud stays off.
    let game = &crate::steamcloud::rechecked(&ctx.cfg, fresh(ctx, game));
    let cloud = match ctx.cloud() {
        Ok(c) => c,
        Err(e) if cloud::is_not_configured(&e) => return Ok(Outcome::NoCloud),
        Err(e) => return offline(ctx, game, trigger, e),
    };
    // A game new to this device may share a line through folders a user
    // confirmed on another device. Settled before anything is scanned.
    let joined = crate::join::auto(cloud, game);
    let game = joined.as_ref().unwrap_or(game);
    let launching = trigger == Trigger::PreLaunch;
    let mut s = match Session::open(ctx, cloud, game) {
        Ok(s) => s,
        Err(e) if cloud::is_unavailable(&e) => return offline(ctx, game, trigger, e),
        Err(e) => return Err(e),
    };
    s.gate = gate.clone();
    // Given up while the line was read: nothing more happens.
    gate.check()?;
    // What waits from earlier comes first: the decision below has to see it.
    // Before playing it waits on when the cloud needs upgrading first.
    if !(launching && cloud.is_legacy().unwrap_or(false)) {
        if !queued(&game.key).is_empty() {
            gate.set_phase(Phase::Uploading);
        }
        if let Err(e) = flush(cloud, &mut s.line, Some(game), None) {
            if must_wait(&e) {
                return offline(ctx, game, trigger, e);
            }
            return Err(e);
        }
        gate.check()?;
        gate.set_phase(Phase::Checking);
    }
    let local = s.local()?;
    // A version of the line that misses a pack, which this device can make
    // whole: after playing, before deciding, so it counts again.
    if !launching {
        if let Err(e) = repair_marked(cloud, &mut s.line, &local) {
            if must_wait(&e) {
                return offline(ctx, game, trigger, e);
            }
            log(format!("{}: putting back what a version misses failed: {e:#}", game.key));
        }
        gate.check()?;
    }
    let st = state::game(&game.key);
    let before = st.last_synced_id.clone();
    let local_size = local.total_size;
    let seen = with_queue(&s.line.view, &s.line.place, &game.key);
    let loc = local_of(&local, &st);
    let mut action = holding(decide::decide(&loc, &seen), &loc, &seen);
    if game.backup_only() && resolution == Resolution::Auto {
        action = backup_only(&game.key, action, &loc, &seen);
    }
    if crate::bind::has_wgs(&game.bind) && resolution == Resolution::Auto {
        action = not_into_containers(&game.key, action);
    }
    if resolution == Resolution::Auto {
        action = not_too_new(game, action, &seen, &local);
    }
    let device = ctx.device();

    let outcome = match (action, resolution) {
        (Action::Nothing, _) => {
            if local.is_empty() {
                Outcome::NothingToSave
            } else {
                Outcome::UpToDate
            }
        }
        (Action::Adopt { id }, _) => {
            if let Some(head) = seen.get(&id) {
                mark_synced(&game.key, &id, &head.vv, &head.content_hash, &head.device_name, device.0)?;
                // Its manifest isn't on this device; the next upload of a
                // large save is a complete one.
                remember_base(&game.key, None);
            }
            Outcome::UpToDate
        }
        (_, Resolution::UseCloud) => {
            // The version the user was shown as the cloud's, while it still
            // is a newest version.
            let usable = s.line.view.usable();
            let shown = st.conflict.as_ref().map(|c| c.cloud_id.clone()).filter(|id| usable.head_entries().any(|h| &h.id == id));
            let Some(target) = shown.or_else(|| other_side(&usable, &local.hash)) else {
                bail!(crate::tr!("云端还没有这个游戏的存档", "The cloud has no saves of this game yet"));
            };
            let pulled = s.pull(&local, &target)?;
            s.make_current(&target, Trigger::Restore, launching)?;
            pulled
        }
        (_, Resolution::UseLocal) => {
            if local.is_empty() {
                bail!(crate::tr!("本地没有存档，不能以本地为准", "There are no local saves to keep"));
            }
            let Action::Push { bases, parent } = decide::settle(&loc, &seen) else { return Ok(Outcome::NothingToSave) };
            let comment = crate::tr!("冲突时选择保留本地存档", "Kept this PC's saves in a conflict");
            let local = s.over_all(local)?;
            s.push(&local, Shot { bases, parent, trigger: Some(Trigger::SyncNow), comment, ..Shot::default() }, launching)?
        }
        (Action::Pull { id }, Resolution::Auto) => s.pull_or_hold(&local, &id, &loc)?,
        (Action::Push { bases, parent }, Resolution::Auto) => s.push(&local, Shot { bases, parent, trigger: Some(trigger), ..Shot::default() }, launching)?,
        (Action::Conflict { candidates, local_is_candidate }, Resolution::Auto) => s.conflict(&local, &candidates, local_is_candidate, trigger)?,
    };
    // The version this device builds on changed: for a large save the other
    // devices are told now, not hours later.
    if !gate.given_up() {
        tell_base(ctx, cloud, &game.key, before.as_deref(), &s.line.view, local_size);
    }
    // Keep launching fast: bookkeeping happens after playing, not before.
    // And after the saves: what this device tells the others about itself
    // (which version it is at) is then true, and a cloud nobody has used
    // yet gets its layout before anything else is written to it.
    if !launching {
        gate.set_phase(Phase::Uploading);
        best_effort_housekeeping(ctx, cloud, game);
        // An index that didn't cover the line is written again for the next
        // device (an upload has done that already).
        if s.line.view.needs_index && !matches!(outcome, Outcome::Uploaded { .. }) && cloud.access().is_ok_and(|a| a == v2::Access::ReadWrite) {
            let _ = line::write_index(cloud.files(), s.line.place.dir(), &s.line.cache, cloud.device_id());
        }
        if matches!(outcome, Outcome::Uploaded { .. }) {
            clean_if_due(ctx, cloud, &game.key);
        }
        if let Err(e) = cloud.look_again(&game.key) {
            log(format!("{}: looking at the game's record failed: {e:#}", game.key));
        }
    }
    Ok(outcome)
}

/// What a sync that nobody chose for does with a game whose saves Steam
/// Cloud keeps ([`GameEntry::backup_only`], docs/plan-v2.md, section 8.2):
/// Steam decides which saves are current, Ludonimbus keeps their history,
/// and writes nothing into the game's saves by itself.
///
/// - A newer version in the cloud is left there: Steam Cloud brings that
///   progress, and once the saves here are the same, [`Action::Adopt`]
///   marks them synced.
/// - Saves here that differ from what the cloud has are uploaded as the
///   newest version, on top of every newest one, without a question.
///
/// What the user asks for (a version from the cloud, a restore) still is
/// done: that is not decided here.
fn backup_only(key: &str, action: Action, local: &Local, view: &LineView) -> Action {
    match action {
        Action::Pull { id } => {
            log(format!("{key}: {id} in the cloud is newer; Steam Cloud keeps this game's saves, so nothing is written"));
            Action::Nothing
        }
        Action::Conflict { .. } => {
            log(format!("{key}: these saves differ from the cloud's; Steam Cloud decided they are current, so they are uploaded"));
            decide::settle(local, view)
        }
        other => other,
    }
}

/// What a sync that nobody chose for does with a version that keeps its
/// saves in the Xbox app's containers ([`crate::bind::Codec::Wgs`],
/// docs/plan-v2.md, section 6): writing into them is experimental (the Xbox
/// app's own cloud sync may put its saves back), so it never happens by
/// itself.
///
/// - A newer version in the cloud is left there; the game shows that the
///   cloud has newer saves ([`check`]), and the user makes it current.
/// - Everything else as for any game: an upload goes ahead, and saves here
///   that went another way than the cloud's are a conflict the user settles.
///
/// What the user asks for (a version from the cloud, a restore, another
/// line's save) is written; that is not decided here.
fn not_into_containers(key: &str, action: Action) -> Action {
    match action {
        Action::Pull { id } => {
            log(format!("{key}: {id} in the cloud is newer; it goes into the Xbox app's containers only when the user says so"));
            Action::Nothing
        }
        other => other,
    }
}

/// What a sync nobody chose ([`Resolution::Auto`]) does with a version
/// written on another platform by a newer game than the one that wrote the
/// saves here, when the game's rule says an older game can't load it
/// ([`too_new`]): it is left in the cloud, as [`not_into_containers`] leaves
/// what would go into the Xbox app's containers. Nothing is written, the
/// game shows that the cloud has newer saves ([`check`]), and the user can
/// still make it current; the app says first that this game may not load
/// it. Versions whose game versions aren't known are pulled as ever.
fn not_too_new(game: &GameEntry, action: Action, view: &LineView, local: &LocalState) -> Action {
    let Action::Pull { id } = &action else { return action };
    let Some(target) = view.get(id) else { return action };
    let Some(rule) = crate::crosssaves::rule_for(game).filter(|r| r.newer_only) else { return action };
    if target.game_version.is_none() || scope_of_platform(&target.platform).is_none_or(|s| s == crate::savelib::save_scope(game)) {
        return action;
    }
    let here = crate::crosssaves::local_version(game, &local.files, &local.roots);
    if !crate::crosssaves::too_new(rule, target.game_version.as_deref(), here.as_deref()) {
        return action;
    }
    log(format!(
        "{}: {id} in the cloud was saved by {} {}, newer than the saves here ({}); taken only when the user says so",
        game.key,
        target.platform,
        target.game_version.as_deref().unwrap_or_default(),
        here.as_deref().unwrap_or_default()
    ));
    Action::Nothing
}

/// Compare local and cloud without changing anything.
pub fn check(ctx: &Ctx, game: &GameEntry) -> StatusReport {
    let st = state::game(&game.key);
    let mut report = StatusReport {
        status: Status::Error,
        message: None,
        local_files: 0,
        local_size: 0,
        local_hash: None,
        head: None,
        snapshot_count: 0,
        last_synced_id: st.last_synced_id.clone(),
        last_sync_at: st.last_sync_at,
        pending: st.pending.len(),
        checked_at: Utc::now(),
        too_new: None,
        incomplete: false,
    };
    let local = match snapshot::local_state(game) {
        Ok(l) => l,
        Err(e) => {
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    report.local_files = local.files.len();
    report.local_size = local.total_size;
    report.local_hash = Some(local.hash.clone());

    let cloud = match ctx.cloud() {
        Ok(c) => c,
        Err(e) if cloud::is_not_configured(&e) => {
            report.status = Status::NoCloud;
            return report;
        }
        Err(e) => {
            report.status = Status::Offline;
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    let (line, view) = match cloud.open(&game.key) {
        Ok(l) => {
            let view = with_queue(&l.view, &l.place, &game.key);
            (l, view)
        }
        Err(e) => {
            report.status = if cloud::is_unavailable(&e) { Status::Offline } else { Status::Error };
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    // Another platform's files that go with the saves here, for a version
    // that shares that platform's line through a rule. Nothing is fetched
    // for a check.
    let local = match with_carried(Some(cloud), game, Some((&line.place, &line.view)), false, local) {
        Ok(l) => l,
        Err(e) => {
            report.status = if cloud::is_unavailable(&e) { Status::Offline } else { Status::Error };
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    report.local_hash = Some(local.hash.clone());
    report.snapshot_count = view.snapshots.len();
    report.incomplete = view.newest_is_incomplete();
    report.head = view.heads.last().and_then(|&i| view.snapshots.get(i)).map(|s| SnapshotSummary::of(s, true));
    if let Some(head) = &report.head {
        // Only read when the rule says an older game can't load a newer save.
        let here = crate::crosssaves::rule_for(game).filter(|r| r.newer_only).and_then(|_| crate::crosssaves::local_version(game, &local.files, &local.roots));
        if too_new(game, here.as_deref(), &head.platform, head.game_version.as_deref()) {
            report.too_new = head.game_version.clone();
        }
    }
    let loc = local_of(&local, &st);
    let action = holding(decide::decide(&loc, &view), &loc, &view);
    report.status = decide::Status::of(&action, &loc, &view).into();
    report
}
