//! What a sync does besides the saves: the game's record and this device's
//! (at once when the version it builds on changed, [`tell_base`]), playtime
//! and artwork, and about once a day the clean-up of old versions and of
//! packs nothing uses any more, in every save line of the game ([`clean`]).
//! None of it fails a sync.

use super::queue::{queued_bases, repair_zips};
use super::{log, Ctx};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Place};
use crate::cloud::{self, library, packs, retention, v2, Cloud, CloudGame};
use crate::config::GameEntry;
use crate::{logger, paths, snapshot, state};
use anyhow::Result;
use chrono::{Duration, Utc};

/// Old versions and unused packs of a game are cleaned up this often.
const CLEAN_EVERY_HOURS: i64 = 24;

pub(super) fn best_effort_housekeeping(ctx: &Ctx, cloud: &Cloud, game: &GameEntry) {
    let st = state::game(&game.key);
    if !st.cloud_def_uploaded && cloud.put_game(&CloudGame::from(game)).is_ok() {
        let _ = state::update_game(&game.key, |g| g.cloud_def_uploaded = true);
    }
    let due = state::load().device_seen_at.is_none_or(|t| Utc::now() - t > chrono::Duration::hours(6));
    if due && cloud.put_device(&cloud::device_info(&ctx.cfg)).is_ok() {
        let _ = state::update(|s| s.device_seen_at = Some(Utc::now()));
    }
    // Playtime and artwork are the game's: in its record's folders. Known
    // without a request once the game has synced.
    let at = match cloud::gamedata::Folders::of(cloud, &game.key) {
        Ok(at) => at,
        Err(e) => {
            logger::log("sync", format!("{}: the game's folders are unknown: {e:#}", game.key));
            return;
        }
    };
    // Upload this device's playtime, fetch the other devices'.
    if let Err(e) = crate::playtime::sync(cloud.files(), &ctx.cfg, &at) {
        logger::log("sync", format!("{}: playtime sync failed: {e:#}", game.key));
    }
    // Artwork: send what this device shows, fetch what it has none of.
    if let Err(e) = crate::artsync::push(cloud.files(), &ctx.cfg, &at, game) {
        logger::log("sync", format!("{}: artwork upload failed: {e:#}", game.key));
    }
    if crate::artsync::KINDS.iter().any(|k| crate::artsync::shown(&ctx.cfg, game, k).is_none()) {
        if let Err(e) = crate::artsync::pull(cloud.files(), &ctx.cfg, &at, Some(game)) {
            logger::log("sync", format!("{}: artwork download failed: {e:#}", game.key));
        }
    }
}

/// What each device synced last on the line in `dir`, this device included,
/// and what the versions waiting on each take files from.
fn device_bases(ctx: &Ctx, devices: &[cloud::DeviceInfo], dir: &str, own: &[String]) -> Vec<retention::DeviceBase> {
    let mut out: Vec<retention::DeviceBase> = devices
        .iter()
        .filter(|d| d.id != ctx.cfg.device.id)
        .flat_map(|d| d.installs.iter().filter(move |i| i.dir == dir).flat_map(|i| i.synced.iter().chain(&i.borrows).cloned()).map(move |s| (d, s)))
        .map(|(d, snapshot)| retention::DeviceBase { device_id: d.id.clone(), snapshot, seen_at: d.last_seen })
        .collect();
    for id in own {
        out.push(retention::DeviceBase { device_id: ctx.cfg.device.id.clone(), snapshot: id.clone(), seen_at: Utc::now() });
    }
    out
}

/// Let go of old automatic versions, of versions deleted long ago and of
/// packs nothing uses any more, in every save line of the game.
fn clean(ctx: &Ctx, cloud: &Cloud, key: &str) -> Result<saves::Cleaned> {
    if cloud.access()? != v2::Access::ReadWrite {
        return Ok(saves::Cleaned::default());
    }
    let place = cloud.place(key)?;
    let library = cloud.library()?;
    let places: Vec<Place> = match library::resolve(&library.records, &place.game) {
        Some(record) => record.lines.iter().map(|l| Place::of(record, l)).collect(),
        None => return Ok(saves::Cleaned::default()),
    };
    // Without the other devices' records nobody knows what they synced last:
    // cleaning up then could take the version each of them builds on.
    let devices = cloud.list_devices()?;
    // This device's own place on each line: the games here that sync it,
    // and what their waiting versions take files from.
    let state = state::load();
    let own = |dir: &str| -> Vec<String> {
        let mut out = Vec::new();
        for (k, g) in state.games.iter().filter(|(_, g)| g.line.as_ref().is_some_and(|l| l.dir() == dir)) {
            out.extend(g.last_synced_id.clone());
            out.extend(queued_bases(k));
        }
        out
    };
    let mut caches: Vec<cloud::line::LineCache> = places.iter().map(|_| Default::default()).collect();
    let mut lines: Vec<saves::Open<'_>> = places
        .iter()
        .zip(caches.iter_mut())
        .map(|(place, cache)| saves::Open { place, cache, bases: device_bases(ctx, &devices, place.dir(), &own(place.dir())) })
        .collect();
    // What a version that misses a pack may get it back from: the saves
    // here (read only then), and the zips this device keeps.
    let local = || ctx.cfg.game(key).and_then(|g| snapshot::local_state(g).ok()).map(|l| l.pack_files()).unwrap_or_default();
    let zips = repair_zips(key);
    let staging = paths::staging_dir();
    let repairs = saves::Repairs { local: &local, zips: &zips, staging: &staging, device_name: ctx.device().1 };
    let policy = retention::Policy::from(&ctx.cfg.retention);
    let now = Utc::now();
    saves::clean_with(cloud.files(), &mut lines, &policy, Duration::days(packs::GRACE_DAYS), now, cloud.device_id(), Some(repairs))
}

/// [`clean`], about once a day per game. Never fails a sync.
pub(super) fn clean_if_due(ctx: &Ctx, cloud: &Cloud, key: &str) {
    let Some(last) = state::game(key).cleaned_at else {
        // The day this device first uploads a game there is nothing to let
        // go of: the first look is a day from now.
        let _ = state::update_game(key, |g| g.cleaned_at = Some(Utc::now()));
        return;
    };
    if Utc::now() - last < Duration::hours(CLEAN_EVERY_HOURS) {
        return;
    }
    // The same once a day: has a device still on 0.6 uploaded this game
    // where this device's line doesn't look?
    match cloud.adopt_old_folder(key) {
        Ok(true) => log(format!("{key}: a device on 0.6 uploaded saves; its folder is part of the line now")),
        Ok(false) => {}
        Err(e) => log(format!("{key}: looking at the folder of 0.6 failed: {e:#}")),
    }
    match clean(ctx, cloud, key) {
        Ok(c) => {
            if !(c.snapshots.is_empty() && c.purged.is_empty() && c.packs.is_empty()) {
                log(format!("{key}: cleaned up {} old versions, {} deleted long ago, {} unused packs", c.snapshots.len(), c.purged.len(), c.packs.len()));
            }
            if !(c.marked.is_empty() && c.repaired.is_empty() && c.retired.is_empty()) {
                log(format!("{key}: versions missing a pack: marked {:?}, whole again {:?}, let go {:?}", c.marked, c.repaired, c.retired));
            }
        }
        Err(e) => log(format!("{key}: cleaning up failed: {e:#}")),
    }
    let _ = state::update_game(key, |g| g.cleaned_at = Some(Utc::now()));
}

/// Tell the other devices at once which version this device builds on now
/// (its record, `devices/<id>.json`, section 3.5), when that changed and
/// the game's saves are large enough for the next upload to take files from
/// it: retention keeps that version for this device by what the record
/// says, and the record is otherwise rewritten every few hours only. Small
/// saves are always uploaded whole; for them nothing is written.
pub(super) fn tell_base(ctx: &Ctx, cloud: &Cloud, key: &str, before: Option<&str>, view: &LineView, local_size: u64) {
    let st = state::game(key);
    let Some(now) = st.last_synced_id.as_deref() else { return };
    if before == Some(now) {
        return;
    }
    let size = view.get(now).map_or(0, |e| e.total_size).max(local_size);
    if size <= packs::Policy::default().full_below {
        return;
    }
    if cloud.put_device(&cloud::device_info(&ctx.cfg)).is_ok() {
        let _ = state::update(|s| s.device_seen_at = Some(Utc::now()));
    }
}
