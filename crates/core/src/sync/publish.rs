//! Uploading what waits in the queue ([`flush`]). A version leaves the
//! queue once its manifest and every pack it names are in the cloud
//! ([`publish_queued`]). One that borrows a pack another device cleaned up
//! while it waited is made whole first, from wherever the content still is;
//! one that can never be complete any more is set aside on this device
//! ([`set_aside`]), so that it doesn't hold up the versions after it.

use super::gate::Gate;
use super::incomplete::set_aside;
use super::lock::lock_game;
use super::queue::{add_to_view, queue_dir, queued, repair_zips, settle_queued, unqueue};
use super::{log, BASE_DAYS, COMMENT};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Place};
use crate::cloud::v2::{IndexEntry, Manifest, Pin};
use crate::cloud::{library, line, packs, Cloud, CloudGame, OpenLine};
use crate::config::{self, GameEntry};
use crate::snapshot::{self, SnapshotMeta, Trigger};
use crate::{paths, state, util};
use anyhow::Result;
use chrono::{Duration, Utc};

/// The save files of a game on this device, for putting back a pack that a
/// waiting version borrows and another device cleaned up while it waited
/// ([`saves::publish`]). Read when a version first needs them, then kept
/// for the rest of the queue.
struct FilesHere<'a> {
    key: &'a str,
    game: Option<&'a GameEntry>,
    /// `None` until read; `Some(None)` when there is nothing to go by: the
    /// game isn't on this device, its saves can't be scanned right now, or
    /// none are found (a drive that isn't there, a game installed anew).
    read: Option<Option<Vec<packs::LocalFile>>>,
}

impl<'a> FilesHere<'a> {
    fn new(key: &'a str, game: Option<&'a GameEntry>) -> Self {
        FilesHere { key, game, read: None }
    }

    /// Files scanned a moment ago.
    fn scanned(key: &'a str, files: Vec<packs::LocalFile>) -> Self {
        FilesHere { key, game: None, read: Some((!files.is_empty()).then_some(files)) }
    }

    fn get(&mut self) -> Option<&[packs::LocalFile]> {
        if self.read.is_none() {
            let game = match self.game {
                Some(g) => Some(g.clone()),
                None => config::load().ok().and_then(|c| c.game(self.key).cloned()),
            };
            let files = game.and_then(|g| match snapshot::local_state(&g) {
                Ok(local) => Some(local.pack_files()).filter(|f| !f.is_empty()),
                Err(e) => {
                    log(format!("{}: the saves here can't be read to check what waits for the cloud: {e:#}", self.key));
                    None
                }
            });
            self.read = Some(files);
        }
        self.read.as_ref().and_then(|f| f.as_deref())
    }

    /// Whether the saves here were read and are what the error says lacks:
    /// a borrowed pack that is gone, with its content not on this device
    /// either. Uploading the version again can't help then.
    fn lost(&self, e: &anyhow::Error) -> bool {
        packs::missing_pack(e).is_some() && matches!(self.read, Some(Some(_)))
    }
}

/// Whether a waiting version that borrows packs may find one gone: it was
/// made longer ago than the cloud keeps what it builds on for this device
/// ([`BASE_DAYS`]; the device synced its base before it made the version),
/// or the version it was built on is no longer in the line, or is marked as
/// missing a pack.
fn waited_too_long(m: &Manifest, view: &LineView) -> bool {
    let base = m.parent.as_deref().and_then(|p| view.get(p));
    Utc::now() - m.created_at > Duration::days(BASE_DAYS) || base.is_none_or(IndexEntry::is_incomplete)
}

/// The game's other save lines besides `here`, each read. Packs belong to
/// the game, so a pack put back has to be whole for the snapshots of every
/// line. Only on the rare ways that put packs back: one listing per line,
/// and its index.
pub(super) fn other_lines_of(cloud: &Cloud, here: &Place) -> Result<Vec<(Place, LineView)>> {
    let library = cloud.library()?;
    let Some(record) = library::resolve(&library.records, &here.game) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for l in &record.lines {
        let place = Place::of(record, l);
        if place.dir() == here.dir() {
            continue;
        }
        let view = cloud.peek(&place)?;
        out.push((place, view));
    }
    Ok(out)
}

/// Upload one waiting version and make it part of its line. It stays in the
/// queue ([`unqueue`]).
///
/// It is part of the line once its manifest is in the cloud and every pack
/// it names is there too ([`saves::publish`]). A version whose manifest got
/// into the cloud earlier while it is still waiting here (an upload that
/// broke off after the manifest, or one whose packs couldn't be checked) is
/// published again: that ends in the same state and checks the packs, so it
/// never leaves the queue with a pack missing.
///
/// A version that borrows packs and waited longer than the cloud keeps what
/// it builds on for this device ([`BASE_DAYS`]), or whose base left the line
/// or is marked as missing a pack, is made whole before it goes up
/// ([`saves::repair_queued`]): what it takes from a pack that is gone is
/// taken from another pack that has it, or packed anew from wherever it
/// still is. A pack found gone after the manifest was written (another
/// device cleaned it up in that moment) is put back whole from the same
/// places ([`saves::put_back`]) and the version published again. Content
/// found nowhere is a [`packs::MissingPack`].
fn publish_queued(cloud: &Cloud, line: &mut OpenLine, m: &mut Manifest, from_06: bool, here: &mut FilesHere) -> Result<()> {
    let key = line.key.clone();
    let dir = queue_dir(&key);
    // Another process of this device uploaded it meanwhile and took it off
    // the queue.
    if !dir.join(format!("{}.json", m.id)).is_file() {
        return Ok(());
    }
    let seen = line.view.get(&m.id).cloned();
    // Once its manifest is in the cloud it doesn't change any more.
    if seen.is_none() && (settle_queued(m, &line.place, &line.view) || from_06) {
        util::atomic_write(&dir.join(format!("{}.json", m.id)), &serde_json::to_vec(m)?)?;
        // The device counts as synced to this version since it was made.
        let _ = state::update_game(&key, |g| {
            if g.last_synced_id.as_deref() == Some(m.id.as_str()) {
                g.last_synced_vv = m.vv.clone();
            }
        });
    }
    let borrows = packs::borrows(m);
    if borrows && seen.is_none() && !saves::was_shown(&dir, &m.id) && waited_too_long(m, &line.view) {
        let local = here.get().unwrap_or_default().to_vec();
        let zips = repair_zips(&key);
        let others = other_lines_of(cloud, &line.place)?;
        let mut lines: Vec<(&Place, &LineView)> = vec![(&line.place, &line.view)];
        lines.extend(others.iter().map(|(p, v)| (p, v)));
        let sources = saves::Sources { local: &local, zips: &zips };
        if let Some(whole) = saves::repair_queued(cloud.files(), &lines, &line.place, m, sources, &dir, &paths::staging_dir())? {
            log(format!("{key}: {} took files from packs that are gone; it takes them from elsewhere now", m.id));
            *m = whole;
            util::atomic_write(&dir.join(format!("{}.json", m.id)), &serde_json::to_vec(m)?)?;
        }
    }
    let files = match borrows {
        true => here.get().unwrap_or_default().to_vec(),
        false => Vec::new(),
    };
    match saves::publish(cloud.files(), &line.place, &mut line.cache, m, &dir, &files, cloud.device_id()) {
        Ok(()) => {}
        Err(e) if packs::missing_pack(&e).is_some() => {
            // The manifest was in the cloud for a moment, and stays as it is:
            // the packs are put back whole.
            let zips = repair_zips(&key);
            let others = other_lines_of(cloud, &line.place)?;
            let mut lines: Vec<(&Place, &LineView)> = vec![(&line.place, &line.view)];
            lines.extend(others.iter().map(|(p, v)| (p, v)));
            let sources = saves::Sources { local: &files, zips: &zips };
            match saves::put_back(cloud.files(), &lines, &line.place, m, None, sources, &paths::staging_dir(), cloud.device_id(), Utc::now())? {
                saves::Repair::Lost(_) => return Err(e),
                _ => saves::publish(cloud.files(), &line.place, &mut line.cache, m, &dir, &files, cloud.device_id())?,
            }
        }
        Err(e) => return Err(e),
    }
    // A backup made by hand is a kept save from the start.
    if m.trigger == Trigger::Manual.as_str() && seen.as_ref().is_none_or(|s| s.pin.is_none()) {
        let name = m.extra.get(COMMENT).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let pin = Pin { name, created_at: Some(m.created_at), device_id: m.device_id.clone(), ..Pin::default() };
        line::write_pin(cloud.files(), line.place.dir(), &m.id, &pin)?;
    }
    add_to_view(&mut line.view, [m.summary()]);
    log(format!("{key}: uploaded {} ({})", m.id, m.trigger));
    Ok(())
}

/// The definition of the game `key`, for the record a game new to the
/// cloud gets.
fn definition(key: &str, game: Option<&GameEntry>) -> Option<CloudGame> {
    if let Some(g) = game {
        return Some(CloudGame::from(g));
    }
    let cfg = config::load().ok()?;
    if let Some(g) = cfg.game(key) {
        return Some(CloudGame::from(g));
    }
    crate::savelib::cached(&cfg).games.into_iter().find(|c| c.key == key)
}

/// Get the line ready for writing: the cloud is upgraded if it has to be,
/// the game gets its record if it has none, and the line is read again when
/// either changed what it looks like.
pub(super) fn ready_to_write(cloud: &Cloud, line: &mut OpenLine, game: Option<&GameEntry>) -> Result<()> {
    let upgraded = cloud.is_legacy()?;
    // Only asked about a game the cloud has no record of: its line was read
    // where 0.6 would have put it as well.
    let has_old = !line.view.snapshots.is_empty() || !line.view.unreadable.is_empty();
    let place = cloud.place_to_write(&line.key, definition(&line.key, game).as_ref(), has_old)?;
    if upgraded || place.dirs != line.place.dirs {
        *line = cloud.open_at(&line.key, place)?;
    } else {
        line.place = place;
    }
    Ok(())
}

/// Upload everything of this game that waits in the queue. `here` are the
/// save files on this device when the caller scanned them a moment ago
/// (they are read when needed otherwise). A version that can never be
/// complete is set aside ([`set_aside`]) and the rest go on.
pub(super) fn flush(cloud: &Cloud, line: &mut OpenLine, game: Option<&GameEntry>, here: Option<Vec<packs::LocalFile>>) -> Result<usize> {
    let waiting = queued(&line.key);
    if waiting.is_empty() {
        return Ok(0);
    }
    ready_to_write(cloud, line, game)?;
    let key = line.key.clone();
    let mut here = match here {
        Some(files) => FilesHere::scanned(&key, files),
        None => FilesHere::new(&key, game),
    };
    let n = waiting.len();
    for (mut m, old) in waiting {
        let done = match publish_queued(cloud, line, &mut m, old, &mut here) {
            Err(e) if here.lost(&e) => set_aside(line, &m, &e),
            other => other.and_then(|_| unqueue(&line.key, &m)),
        };
        if done.is_err() {
            line.remember();
        }
        done?;
    }
    line.remember();
    Ok(n)
}

/// Upload one waiting version (nothing when another process did already).
/// It stays in the queue until [`drop_pending`].
pub fn upload_pending(cloud: &Cloud, m: &SnapshotMeta) -> Result<()> {
    let key = m.game.as_str();
    let Some((mut manifest, old)) = queued(key).into_iter().find(|(q, _)| q.id == m.id) else { return Ok(()) };
    let _lock = lock_game(key, &Gate::default())?;
    let mut line = cloud.open(key)?;
    ready_to_write(cloud, &mut line, None)?;
    let done = publish_queued(cloud, &mut line, &mut manifest, old, &mut FilesHere::new(key, None));
    line.remember();
    done
}

/// Take a version off the queue without uploading it.
pub fn drop_pending(m: &SnapshotMeta) -> Result<()> {
    let key = m.game.as_str();
    match queued(key).into_iter().find(|(q, _)| q.id == m.id) {
        Some((manifest, _)) => unqueue(key, &manifest),
        None => state::update_game(key, |g| g.pending.retain(|x| x != &m.id)),
    }
}

/// Upload the versions of `key` that were made while the cloud couldn't be
/// reached.
pub fn flush_pending(cloud: &Cloud, key: &str) -> Result<usize> {
    if queued(key).is_empty() {
        return Ok(0);
    }
    let _lock = lock_game(key, &Gate::default())?;
    let mut line = cloud.open(key)?;
    flush(cloud, &mut line, None, None)
}
