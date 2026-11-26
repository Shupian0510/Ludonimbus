//! Syncing one game: what this device does about what [`cloud::decide`]
//! says.
//!
//! The decision itself is one table, in [`cloud::decide`]; reading and
//! writing a save line is [`cloud::saves`]. This module is the part with
//! side effects on this device: scanning the saves, remembering what was
//! synced, replacing the saves with a version from the cloud, and the queue
//! of versions the cloud doesn't have yet.
//!
//! **The queue.** Every new version is first put on this device's disk,
//! complete ([`cloud::saves::stage`]), and uploaded from there. When the
//! cloud can't be reached it simply stays; the next sync uploads what waits
//! before it does anything else. The device counts as synced to a version
//! from the moment it is in the queue, so playing on while offline builds
//! one version on the other.
//!
//! **Nothing is lost by a sync.** Before saves on this device are replaced,
//! they go to the cloud as a snapshot kept beside the history, unless the
//! cloud already has the same content. Two devices that both played are a
//! conflict the user settles; the side that loses stays in the cloud.
//!
//! Versions the user deletes go to 「最近删除」 first and are purged after
//! `cloud::TRASH_DAYS` days. What else goes away, and when, is
//! [`cloud::retention`].
//!
//! **Which line.** A game syncs with the save line it follows on this
//! device: the one its devices share, or one a device keeps to itself
//! ([`lines`], [`use_line`]). Everything above is about that line.
//!
//! **Not waiting.** Playing doesn't wait for the cloud unless there really
//! is newer progress to get (docs/plan-v2.md, section 5.5). Whoever waits
//! for a sync can stop waiting ([`Gate`]); a sync given up never replaces
//! the save files afterwards. That is safe because of the version vectors:
//! what wasn't waited for is at worst a fork the user is asked about later,
//! never saves silently overwritten. One sync of a game runs at a time on
//! a device, across processes ([`lock_game`]), and a sync after playing the
//! launch wrapper didn't wait for is finished by someone else
//! ([`finish_owed`]).

use crate::cloud::decide::{self, Action, Local};
use crate::cloud::line::LineView;
use crate::cloud::saves::{self, Draft, Place};
use crate::cloud::store::{Entry, Store};
use crate::cloud::v2::{self, FileRef, IndexEntry, Manifest, Origin, Pin};
use crate::cloud::{self, library, line, packs, retention, Cloud, CloudGame, OpenLine};
use crate::config::{self, AppConfig, GameEntry};
use crate::lockfile::FileLock;
use crate::snapshot::{self, LocalState, SnapshotMeta, SnapshotSummary, Trigger};
use crate::state::{self, ConflictInfo, GameState};
use crate::vv::{self, Vv};
use crate::{logger, paths, util};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Old versions and unused packs of a game are cleaned up this often.
const CLEAN_EVERY_HOURS: i64 = 24;

/// A device that has not synced a game for this long uploads everything
/// again instead of building on what it synced last: the cloud no longer
/// keeps that version for it (docs/plan-v2.md, section 5.4).
const BASE_DAYS: i64 = 60;

/// The note a snapshot was given when it was made, in its manifest.
const COMMENT: &str = "comment";

pub struct Ctx {
    pub cfg: AppConfig,
    cloud: Option<Cloud>,
    cloud_error: Option<String>,
}

impl Ctx {
    pub fn load() -> Result<Ctx> {
        Ok(Self::from_config(config::load()?))
    }

    pub fn from_config(cfg: AppConfig) -> Ctx {
        let (cloud, cloud_error) = match cloud::open(&cfg) {
            Ok(c) => (c, None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };
        Ctx {
            cfg,
            cloud,
            cloud_error,
        }
    }

    /// With a cloud the caller opened itself, whatever the config says.
    pub fn with_cloud(cfg: AppConfig, cloud: Cloud) -> Ctx {
        let cloud = cloud.for_config(&cfg);
        Ctx { cfg, cloud: Some(cloud), cloud_error: None }
    }

    pub fn cloud(&self) -> Result<&Cloud> {
        if let Some(c) = self.cloud.as_ref() {
            return Ok(c);
        }
        match &self.cloud_error {
            Some(e) => Err(anyhow!("{e}")),
            None => Err(anyhow!(cloud::NotConfigured)),
        }
    }

    pub fn has_cloud(&self) -> bool {
        self.cloud.is_some()
    }

    pub fn game(&self, key: &str) -> Result<GameEntry> {
        self.cfg.require_game(key).cloned()
    }

    fn device(&self) -> (&str, &str) {
        (&self.cfg.device.id, &self.cfg.device.name)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    NoCloud,
    /// No saves locally or in the cloud.
    Empty,
    /// Local saves exist, cloud has nothing yet.
    NotUploaded,
    Synced,
    LocalChanged,
    CloudNewer,
    /// No local saves, the cloud has some (e.g. reinstalled game).
    CanRestore,
    Conflict,
    Offline,
    Error,
}

impl From<decide::Status> for Status {
    fn from(s: decide::Status) -> Self {
        match s {
            decide::Status::Empty => Status::Empty,
            decide::Status::NotUploaded => Status::NotUploaded,
            decide::Status::Synced => Status::Synced,
            decide::Status::LocalChanged => Status::LocalChanged,
            decide::Status::CloudNewer => Status::CloudNewer,
            decide::Status::CanRestore => Status::CanRestore,
            decide::Status::Conflict => Status::Conflict,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub status: Status,
    pub message: Option<String>,
    pub local_files: usize,
    pub local_size: u64,
    pub local_hash: Option<String>,
    pub head: Option<SnapshotSummary>,
    pub snapshot_count: usize,
    pub last_synced_id: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub pending: usize,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Auto,
    UseLocal,
    /// The newest version in the cloud.
    UseCloud,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    UpToDate,
    Uploaded { id: String },
    /// Snapshot created but the cloud is unreachable; uploaded later.
    Queued { id: String, reason: String },
    Pulled { id: String, device: String, created_at: DateTime<Utc> },
    Conflict { local_files: usize, local_size: u64, cloud: SnapshotSummary },
    NothingToSave,
    NoCloud,
    Offline { message: String },
}

impl Outcome {
    /// Nothing is left for a later sync to do: the cloud has the saves, or
    /// the user is asked (a conflict). Not so for a version that waits in
    /// the queue, or a cloud that wasn't reached.
    pub fn is_final(&self) -> bool {
        !matches!(self, Outcome::Queued { .. } | Outcome::Offline { .. })
    }
}

/// The newest version in a list of a game's versions: the one its save line
/// says ([`SnapshotSummary::head`]; the last of several when the line has
/// forked). In a list that doesn't say, such as one a device on 0.6 made,
/// the newest snapshot that is part of the main history.
pub fn mainline_head(list: &[SnapshotSummary]) -> Option<SnapshotSummary> {
    if let Some(head) = list.iter().filter(|s| s.head).max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id))) {
        return Some(head.clone());
    }
    list.iter()
        .filter(|s| !s.side && s.trigger.is_mainline())
        .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
        .cloned()
}

fn log(msg: impl AsRef<str>) {
    logger::log("sync", msg);
}

/// The cloud can't take what this device wants to write right now, and
/// will later: it can't be reached, or another device is upgrading it.
fn must_wait(e: &anyhow::Error) -> bool {
    cloud::is_unavailable(e) || cloud::upgrade::is_busy(e)
}

// ------------------------------------------------------------ what was synced

/// Remember that this device is now at snapshot `id`.
fn mark_synced(key: &str, id: &str, vv: &Vv, hash: &str, device_name: &str, own_device: &str) -> Result<()> {
    state::update_game(key, |g| {
        g.last_synced_id = Some(id.to_string());
        g.last_synced_hash = Some(hash.to_string());
        g.last_synced_vv = vv.clone();
        g.counter = g.counter.max(vv.get(own_device));
        g.last_sync_at = Some(Utc::now());
        g.last_synced_device = Some(device_name.to_string());
        g.conflict = None;
        g.last_error = None;
    })
}

/// Remember what the saves here were (`back`) before the version `now`
/// replaced them, for [`undo`]. Nothing to remember when there were none.
fn note_replaced(key: &str, back: Option<String>, now: &str, device: &str) {
    let replaced = back.filter(|b| b != now).map(|back| state::Replaced { back, now: now.to_string(), device: device.to_string(), at: Utc::now() });
    let _ = state::update_game(key, |g| g.replaced = replaced);
}

fn base_file(key: &str) -> PathBuf {
    paths::data_dir().join("lines").join(format!("{key}.base.json"))
}

/// Keep the manifest of the snapshot this device is at: the next upload of
/// a large save sends only what changed since.
fn remember_base(key: &str, manifest: Option<&Manifest>) {
    let file = base_file(key);
    match manifest.and_then(|m| serde_json::to_vec(m).ok()) {
        Some(bytes) => {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = util::atomic_write(&file, &bytes);
        }
        None => {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// The manifest to build an upload of `size` bytes on, when there is a
/// point: small saves are always uploaded whole.
fn base_manifest(key: &str, st: &GameState, size: u64) -> Option<Manifest> {
    if size <= packs::Policy::default().full_below {
        return None;
    }
    if !st.last_sync_at.is_some_and(|t| Utc::now() - t < Duration::days(BASE_DAYS)) {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(base_file(key)).ok()?).ok()?;
    (Some(&manifest.id) == st.last_synced_id.as_ref()).then_some(manifest)
}

/// The vector of what this device synced last, as far as it can be known:
/// what the line says while the snapshot is still there (a snapshot of 0.6
/// has no other), else what this device remembers.
fn base_vv(st: &GameState, view: &LineView) -> Vv {
    st.last_synced_id
        .as_deref()
        .and_then(|id| view.get(id))
        .map(|s| s.vv.clone())
        .filter(|vv| !vv.is_empty())
        .unwrap_or_else(|| st.last_synced_vv.clone())
}

fn local_of(local: &LocalState, st: &GameState) -> Local {
    Local { content_hash: (!local.is_empty()).then(|| local.hash.clone()), base: st.base() }
}

/// Where the saves of a game come from, for the manifest's `platform`.
/// A PC game says which store's version it is where that is known
/// (`pc:xbox`, `pc:steam`): versions of several stores can share one line,
/// and the app shows which one each version was played on.
pub fn platform(game: &GameEntry) -> String {
    match game.adapter.as_deref().and_then(crate::adapters::find) {
        Some(a) if a.is_emulator() => format!("emu:{}", a.key),
        Some(a) => format!("recomp:{}", a.key),
        None if game.store.is_some() => "pc:steam".to_string(),
        None if crate::games::source(game).kind == crate::games::SourceKind::Xbox => "pc:xbox".to_string(),
        None => "pc".to_string(),
    }
}

// ------------------------------------------------------------------ the queue

fn queue_dir(key: &str) -> PathBuf {
    paths::pending_dir().join(key)
}

/// A version 0.6 left in the queue, as a manifest of layout 2. Its zip is
/// the snapshot's own pack as it is; the vector is given when it is
/// uploaded ([`settle_queued`]).
fn from_06(meta: SnapshotMeta) -> Manifest {
    let mut extra = v2::Extra::new();
    if !meta.roots.is_empty() {
        extra.insert(snapshot::ROOTS.to_string(), serde_json::json!(meta.roots));
    }
    if !meta.comment.trim().is_empty() {
        extra.insert(COMMENT.to_string(), meta.comment.clone().into());
    }
    Manifest {
        v: v2::LAYOUT,
        id: meta.id,
        game: meta.game,
        line: library::MAIN_LINE.to_string(),
        vv: Vv::new(),
        parent: meta.parent,
        device_id: meta.device_id,
        device_name: meta.device_name,
        platform: String::new(),
        created_at: meta.created_at,
        trigger: meta.trigger.as_str().to_string(),
        side: !meta.trigger.is_mainline(),
        content_hash: meta.content_hash,
        total_size: meta.total_size,
        files: meta
            .files
            .into_iter()
            .map(|f| FileRef { path: f.path, size: f.size, hash: f.hash, mtime: f.mtime, pack: None, extra: v2::Extra::new() })
            .collect(),
        game_version: meta.game_version,
        installed_version: meta.installed_version,
        info: None,
        origin: None,
        requires: Vec::new(),
        app_version: meta.app_version,
        extra,
    }
}

/// The versions of `key` waiting on this device for the cloud, oldest
/// first. The flag says the file in the queue is one 0.6 wrote.
fn queued(key: &str) -> Vec<(Manifest, bool)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(queue_dir(key)) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        if value.get("v").and_then(|v| v.as_u64()) == Some(u64::from(v2::LAYOUT)) {
            if let Ok(m) = serde_json::from_value::<Manifest>(value) {
                out.push((m, false));
            }
        } else if let Ok(meta) = serde_json::from_value::<SnapshotMeta>(value) {
            out.push((from_06(meta), true));
        }
    }
    out.sort_by(|a, b| a.0.created_at.cmp(&b.0.created_at).then_with(|| a.0.id.cmp(&b.0.id)));
    out
}

/// Versions of `key` waiting on this device for the cloud, oldest first.
pub fn pending_metas(key: &str) -> Vec<SnapshotMeta> {
    queued(key).iter().map(|(m, _)| SnapshotMeta::of(m, key, None)).collect()
}

/// The zips that hold the files of a version waiting in the queue (each
/// file as `files/<path>` in one of them). Empty when the version isn't
/// waiting.
pub fn pending_zips(key: &str, id: &str) -> Vec<PathBuf> {
    let Some((m, _)) = queued(key).into_iter().find(|(m, _)| m.id == id) else { return Vec::new() };
    let dir = queue_dir(key);
    let zips: Vec<PathBuf> = m.packs().iter().map(|p| dir.join(format!("{p}.zip"))).collect();
    if zips.iter().all(|z| z.is_file()) {
        zips
    } else {
        Vec::new()
    }
}

/// The games with versions waiting for the cloud.
pub fn pending_keys() -> Vec<String> {
    std::fs::read_dir(paths::pending_dir())
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// Add snapshots to a view of their line, as reading the line again would
/// show them.
fn add_to_view(view: &mut LineView, entries: impl IntoIterator<Item = IndexEntry>) {
    let mut all: Vec<(IndexEntry, usize)> = view.snapshots.drain(..).zip(view.places.drain(..)).collect();
    for e in entries {
        if !all.iter().any(|(s, _)| s.id == e.id) {
            all.push((e, 0));
        }
    }
    all.sort_by(|a, b| a.0.created_at.cmp(&b.0.created_at).then_with(|| a.0.id.cmp(&b.0.id)));
    let (snapshots, places): (Vec<IndexEntry>, Vec<usize>) = all.into_iter().unzip();
    let heads = {
        let main: Vec<(usize, &IndexEntry)> = snapshots.iter().enumerate().filter(|(_, s)| !s.is_side()).collect();
        vv::heads(&main, |(_, s)| &s.vv).into_iter().map(|k| main[k].0).collect()
    };
    view.snapshots = snapshots;
    view.places = places;
    view.heads = heads;
}

/// Make a waiting version fit the line it is about to join. Nobody has seen
/// it yet, so it may still change. Returns whether it did.
///
/// - It is labelled with the game and line as they are now (the cloud may
///   have been upgraded since it was made).
/// - A version without a vector (0.6 made it, or the line couldn't be read
///   when it was made) gets one: built on its parent.
/// - A version whose vector lacks its parent's changes gets them. That is a
///   version made offline by a device that came from 0.6 and didn't know
///   the vector of the 0.6 snapshot it was at.
fn settle_queued(m: &mut Manifest, place: &Place, view: &LineView) -> bool {
    let before = m.clone();
    m.game = place.game.clone();
    m.line = place.line.clone();
    let parent = m.parent.as_deref().and_then(|p| view.get(p)).map(|s| s.vv.clone()).filter(|vv| !vv.is_empty());
    if m.is_side() {
        if m.vv.is_empty() {
            m.vv = parent.unwrap_or_default();
        }
    } else if m.vv.is_empty() {
        let floor = vv::max_count(view.snapshots.iter().map(|s| &s.vv), &m.device_id);
        m.vv = Vv::next(parent.iter(), &m.device_id, floor);
    } else if let Some(parent) = parent {
        if !m.vv.contains(&parent) {
            m.vv.join(&parent);
        }
    }
    *m != before
}

/// The line as it will be once what waits in the queue is uploaded. This
/// device is at one of those versions, so this is the line it decides by.
fn with_queue(view: &LineView, place: &Place, key: &str) -> LineView {
    let mut seen = view.clone();
    for (mut m, _) in queued(key) {
        if seen.get(&m.id).is_none() {
            settle_queued(&mut m, place, &seen);
            add_to_view(&mut seen, [m.summary()]);
        }
    }
    seen
}

/// The version "the cloud's" stands for when the user chooses between it
/// and the saves here: the newest head that isn't those saves.
fn other_side(view: &LineView, local_hash: &str) -> Option<String> {
    let heads: Vec<&IndexEntry> = view.head_entries().collect();
    heads.iter().rev().find(|h| h.content_hash != local_hash).or(heads.last()).map(|h| h.id.clone())
}

fn unqueue(key: &str, m: &Manifest) -> Result<()> {
    saves::unstage(&queue_dir(key), m)?;
    state::update_game(key, |g| g.pending.retain(|x| x != &m.id))
}

/// Upload one waiting version and make it part of its line. It stays in the
/// queue ([`unqueue`]).
fn publish_queued(cloud: &Cloud, line: &mut OpenLine, m: &mut Manifest, from_06: bool) -> Result<()> {
    let key = line.key.clone();
    let dir = queue_dir(&key);
    // Another process of this device may have uploaded it meanwhile.
    if line.view.get(&m.id).is_none() {
        if settle_queued(m, &line.place, &line.view) || from_06 {
            util::atomic_write(&dir.join(format!("{}.json", m.id)), &serde_json::to_vec(m)?)?;
            // The device counts as synced to this version since it was made.
            let _ = state::update_game(&key, |g| {
                if g.last_synced_id.as_deref() == Some(m.id.as_str()) {
                    g.last_synced_vv = m.vv.clone();
                }
            });
        }
        saves::publish(cloud.files(), &line.place, &mut line.cache, m, &dir, &[], cloud.device_id())?;
        // A backup made by hand is a kept save from the start.
        if m.trigger == Trigger::Manual.as_str() {
            let name = m.extra.get(COMMENT).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let pin = Pin { name, created_at: Some(m.created_at), device_id: m.device_id.clone(), ..Pin::default() };
            line::write_pin(cloud.files(), line.place.dir(), &m.id, &pin)?;
        }
        add_to_view(&mut line.view, [m.summary()]);
        log(format!("{key}: uploaded {} ({})", m.id, m.trigger));
    }
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
fn ready_to_write(cloud: &Cloud, line: &mut OpenLine, game: Option<&GameEntry>) -> Result<()> {
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

/// Upload everything of this game that waits in the queue.
fn flush(cloud: &Cloud, line: &mut OpenLine, game: Option<&GameEntry>) -> Result<usize> {
    let waiting = queued(&line.key);
    if waiting.is_empty() {
        return Ok(0);
    }
    ready_to_write(cloud, line, game)?;
    let n = waiting.len();
    for (mut m, old) in waiting {
        let done = publish_queued(cloud, line, &mut m, old).and_then(|_| unqueue(&line.key, &m));
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
    let done = publish_queued(cloud, &mut line, &mut manifest, old);
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
    flush(cloud, &mut line, None)
}

/// What a new snapshot is, beyond its files.
#[derive(Default)]
struct Shot {
    /// The vectors a mainline snapshot is built on ([`decide::next_vv`]).
    bases: Vec<Vv>,
    parent: Option<String>,
    trigger: Option<Trigger>,
    /// Beside the history: never the newest version.
    side: bool,
    comment: String,
    origin: Option<Origin>,
    /// The manifest to share unchanged files with (large saves).
    base: Option<Manifest>,
}

/// Put a snapshot of `files` into the queue of `key`: complete on this
/// device, not yet in the cloud.
#[allow(clippy::too_many_arguments)]
fn enqueue(key: &str, place: &Place, draft: &Draft, files: &[packs::LocalFile], roots: &BTreeMap<String, String>, comment: &str, base: Option<&Manifest>) -> Result<Manifest> {
    let mut manifest = saves::draft(place, draft, files, base, &packs::Policy::default());
    if !roots.is_empty() {
        manifest.extra.insert(snapshot::ROOTS.to_string(), serde_json::json!(roots));
    }
    if !comment.trim().is_empty() {
        manifest.extra.insert(COMMENT.to_string(), comment.into());
    }
    saves::stage(&manifest, files, &queue_dir(key))?;
    state::update_game(key, |g| {
        if !g.pending.contains(&manifest.id) {
            g.pending.push(manifest.id.clone());
        }
    })?;
    Ok(manifest)
}

/// A game that follows a line of this device's own because that is the
/// device's default ([`cloud::wanted_line`]) gets that written into its
/// entry before its first version is made: from then on the choice is the
/// game's, and the default no longer decides.
fn note_line(ctx: &Ctx, game: &GameEntry, st: &GameState) -> Result<()> {
    if game.line.is_some() {
        return Ok(());
    }
    let line = cloud::wanted_line(&ctx.cfg, game, Some(st));
    if line == library::MAIN_LINE {
        return Ok(());
    }
    config::update(|c| {
        if let Some(g) = c.games.iter_mut().find(|g| g.key == game.key) {
            if g.line.is_none() {
                g.line = Some(line.clone());
            }
        }
        Ok(())
    })
}

/// Queue a snapshot of this device's saves.
fn stage_local(ctx: &Ctx, game: &GameEntry, place: &Place, view: &LineView, local: &LocalState, shot: &Shot) -> Result<Manifest> {
    let st = state::game(&game.key);
    note_line(ctx, game, &st)?;
    let device = ctx.device();
    let view = &with_queue(view, place, &game.key);
    let vv = if shot.side { base_vv(&st, view) } else { decide::next_vv(&shot.bases, view, device.0, st.counter) };
    let draft = Draft {
        vv,
        parent: shot.parent.clone(),
        side: shot.side,
        trigger: shot.trigger.unwrap_or(Trigger::SyncNow).as_str().to_string(),
        platform: platform(game),
        device_id: device.0.to_string(),
        device_name: device.1.to_string(),
        created_at: Utc::now(),
        game_version: crate::crosssaves::local_version(game, &local.files, &local.roots),
        installed_version: crate::gamebackup::version::installed(game),
        info: None,
        origin: shot.origin.clone(),
    };
    let manifest = enqueue(&game.key, place, &draft, &local.pack_files(), &local.used_roots(), &shot.comment, shot.base.as_ref())?;
    if !shot.side {
        mark_synced(&game.key, &manifest.id, &manifest.vv, &manifest.content_hash, device.1, device.0)?;
        remember_base(&game.key, Some(&manifest));
    }
    Ok(manifest)
}

/// Files that are not this device's saves (a Switch's, handed over the
/// local network; a save converted for a version that is only in the
/// cloud), to be made a version of the game `key`.
pub struct Foreign<'a> {
    pub key: &'a str,
    /// Canonical path and content of each file.
    pub files: &'a [(String, Vec<u8>)],
    pub trigger: Trigger,
    /// The version the files were made from.
    pub parent: Option<String>,
    /// On top of every newest version, not only of `parent`: the user chose
    /// these files over whatever the cloud has.
    pub over_all: bool,
    /// Id and name of the device the files are from.
    pub device: (&'a str, &'a str),
    pub platform: &'a str,
    pub comment: &'a str,
    pub game_version: Option<String>,
    pub origin: Option<Origin>,
}

/// Make a version from files handed to this device and put it in the queue
/// ([`upload_pending`] uploads it). `line` is the game's line as just read.
/// A version of a kind that never is the newest (`pre-restore`) goes beside
/// the history.
pub fn queue_files(line: &OpenLine, f: &Foreign) -> Result<SnapshotMeta> {
    let work = paths::staging_dir().join(format!("files-{}", ulid::Ulid::new()));
    let result = (|| -> Result<Manifest> {
        let mut files = Vec::with_capacity(f.files.len());
        for (path, data) in f.files {
            let dest = snapshot::resolve_in(&work, path)?;
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&dest, data)?;
            files.push(packs::LocalFile::from_disk(path.clone(), dest)?);
        }
        let view = with_queue(&line.view, &line.place, f.key);
        let side = !f.trigger.is_mainline();
        let parent = f.parent.as_deref().and_then(|p| view.get(p)).map(|s| s.vv.clone()).filter(|vv| !vv.is_empty());
        let vv = if side {
            parent.unwrap_or_default()
        } else {
            let bases: Vec<Vv> = match f.over_all {
                true => view.head_entries().map(|h| h.vv.clone()).collect(),
                false => parent.into_iter().collect(),
            };
            decide::next_vv(&bases, &view, f.device.0, 0)
        };
        let draft = Draft {
            vv,
            parent: f.parent.clone(),
            side,
            trigger: f.trigger.as_str().to_string(),
            platform: f.platform.to_string(),
            device_id: f.device.0.to_string(),
            device_name: f.device.1.to_string(),
            created_at: Utc::now(),
            game_version: f.game_version.clone(),
            installed_version: None,
            info: None,
            origin: f.origin.clone(),
        };
        enqueue(f.key, &line.place, &draft, &files, &BTreeMap::new(), f.comment, None)
    })();
    let _ = std::fs::remove_dir_all(&work);
    Ok(SnapshotMeta::of(&result?, f.key, None))
}

// ------------------------------------------------------------- housekeeping

fn best_effort_housekeeping(ctx: &Ctx, cloud: &Cloud, game: &GameEntry) {
    let st = state::game(&game.key);
    if !st.cloud_def_uploaded && cloud.put_game(&CloudGame::from(game)).is_ok() {
        let _ = state::update_game(&game.key, |g| g.cloud_def_uploaded = true);
    }
    let due = state::load()
        .device_seen_at
        .is_none_or(|t| Utc::now() - t > chrono::Duration::hours(6));
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

/// What each device synced last on the line in `dir`, this device included.
fn device_bases(ctx: &Ctx, devices: &[cloud::DeviceInfo], dir: &str, own: Option<&str>) -> Vec<retention::DeviceBase> {
    let mut out: Vec<retention::DeviceBase> = devices
        .iter()
        .filter(|d| d.id != ctx.cfg.device.id)
        .flat_map(|d| d.installs.iter().filter(move |i| i.dir == dir).filter_map(|i| i.synced.clone()).map(move |s| (d, s)))
        .map(|(d, snapshot)| retention::DeviceBase { device_id: d.id.clone(), snapshot, seen_at: d.last_seen })
        .collect();
    if let Some(id) = own {
        out.push(retention::DeviceBase { device_id: ctx.cfg.device.id.clone(), snapshot: id.to_string(), seen_at: Utc::now() });
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
    let devices = cloud.list_devices().unwrap_or_default();
    // This device's own place on each line: the games here that sync it.
    let state = state::load();
    let own = |dir: &str| state.games.values().find(|g| g.line.as_ref().is_some_and(|l| l.dir() == dir)).and_then(|g| g.last_synced_id.clone());
    let mut caches: Vec<cloud::line::LineCache> = places.iter().map(|_| Default::default()).collect();
    let mut lines: Vec<saves::Open<'_>> = places
        .iter()
        .zip(caches.iter_mut())
        .map(|(place, cache)| saves::Open { place, cache, bases: device_bases(ctx, &devices, place.dir(), own(place.dir()).as_deref()) })
        .collect();
    saves::clean(
        cloud.files(),
        &mut lines,
        &retention::Policy::from(&ctx.cfg.retention),
        Duration::days(packs::GRACE_DAYS),
        Utc::now(),
        cloud.device_id(),
    )
}

/// [`clean`], about once a day per game. Never fails a sync.
fn clean_if_due(ctx: &Ctx, cloud: &Cloud, key: &str) {
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
        Ok(c) if c.snapshots.is_empty() && c.purged.is_empty() && c.packs.is_empty() => {}
        Ok(c) => log(format!(
            "{key}: cleaned up {} old versions, {} deleted long ago, {} unused packs",
            c.snapshots.len(),
            c.purged.len(),
            c.packs.len()
        )),
        Err(e) => log(format!("{key}: cleaning up failed: {e:#}")),
    }
    let _ = state::update_game(key, |g| g.cleaned_at = Some(Utc::now()));
}

// -------------------------------------------------------------- not waiting

/// What a sync is doing, for whoever waits for it ([`Gate::phase`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Phase {
    /// Waiting for another sync of the game, reading its save line,
    /// deciding.
    #[default]
    Checking,
    /// Getting a newer version: `done` of `total` bytes of its files, as
    /// each content pack comes in.
    Downloading { done: u64, total: u64 },
    /// Replacing the save files with it, until the next step. Quick, on
    /// this device, and not to be given up.
    Applying,
    /// Uploading versions of this device, and what goes with them.
    Uploading,
    /// The sync has ended, or is about to.
    Done,
}

/// Inside a sync: whoever waited for it stopped. It ends as
/// [`Outcome::Offline`].
#[derive(Debug)]
struct GaveUp;

impl std::fmt::Display for GaveUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", crate::tr!("没有等云盘，存档留到下次同步", "Didn't wait for the cloud; the saves are synced next time"))
    }
}

impl std::error::Error for GaveUp {}

fn is_gave_up(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<GaveUp>().is_some())
}

#[derive(Default)]
struct GateState {
    phase: Phase,
    given_up: bool,
}

type Watcher = Arc<dyn Fn(Phase) + Send + Sync>;

#[derive(Default)]
struct GateInner {
    state: Mutex<GateState>,
    watcher: Mutex<Option<Watcher>>,
}

/// Shared by a sync and whoever waits for it: what the sync is doing, and
/// a way to stop waiting ([`Gate::give_up`]).
///
/// A sync that was given up doesn't replace the save files any more: the
/// check and the start of replacing them are one step
/// ([`Gate::start_applying`]). It also starts no further request and ends
/// as [`Outcome::Offline`]. A version it put in the queue stays there, and
/// an upload already under way may finish: packs go up first and the
/// manifest last, so an upload cut off is never half a version.
///
/// `Gate::default()` is one nobody gives up.
#[derive(Clone, Default)]
pub struct Gate(Arc<GateInner>);

impl Gate {
    pub fn new() -> Gate {
        Gate::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tell(&self, phase: Phase) {
        let watcher = self.0.watcher.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(w) = watcher {
            w(phase);
        }
    }

    pub fn phase(&self) -> Phase {
        self.state().phase
    }

    pub fn given_up(&self) -> bool {
        self.state().given_up
    }

    /// Stop waiting. From here on the sync changes no save files. `false`
    /// when it is replacing them right now: then wait for it to end (that
    /// is quick, and on this device).
    pub fn give_up(&self) -> bool {
        let mut s = self.state();
        if s.phase == Phase::Applying {
            return false;
        }
        s.given_up = true;
        true
    }

    /// Be told of every change of phase, on the thread of the sync.
    pub fn watch(&self, f: impl Fn(Phase) + Send + Sync + 'static) {
        *self.0.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
    }

    /// What the work the gate is about does now. The sync calls this; so may
    /// something that stands in for a sync.
    pub fn set_phase(&self, phase: Phase) {
        self.state().phase = phase;
        self.tell(phase);
    }

    /// Move on to replacing the save files, unless the waiting was given
    /// up: `false` then, and nothing may be replaced. One step with
    /// [`Gate::give_up`], so the two can't cross.
    pub fn start_applying(&self) -> bool {
        {
            let mut s = self.state();
            if s.given_up {
                return false;
            }
            s.phase = Phase::Applying;
        }
        self.tell(Phase::Applying);
        true
    }

    fn add_done(&self, bytes: u64) {
        let phase = {
            let mut s = self.state();
            match &mut s.phase {
                Phase::Downloading { done, total } => *done = (*done + bytes).min(*total),
                _ => return,
            }
            s.phase
        };
        self.tell(phase);
    }

    /// An error once the waiting was given up: the next step isn't taken.
    fn check(&self) -> Result<()> {
        if self.given_up() {
            return Err(GaveUp.into());
        }
        Ok(())
    }
}

/// The store as [`Session::pull`] downloads from it: no new request once
/// the waiting was given up, and each content pack counted when it is in.
struct Watched<'a> {
    inner: &'a dyn Store,
    gate: &'a Gate,
    /// Bytes of the wanted files in each pack, by pack id.
    packs: BTreeMap<String, u64>,
}

impl Store for Watched<'_> {
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }
    fn label(&self) -> String {
        self.inner.label()
    }
    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        self.inner.quota()
    }
    fn check(&self) -> Result<()> {
        self.gate.check()?;
        self.inner.check()
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.gate.check()?;
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.gate.check()?;
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.gate.check()?;
        self.inner.download(path, dest)?;
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some(bytes) = self.packs.get(name.strip_suffix(".zip").unwrap_or(name)) {
            self.gate.add_done(*bytes);
        }
        Ok(())
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.gate.check()?;
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.gate.check()?;
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.gate.check()?;
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        self.inner.cache_scope()
    }
}

// ------------------------------------------------------ one sync at a time

thread_local! {
    /// Games whose lock this thread holds: an operation that holds it may
    /// run another one (choosing a line runs a sync).
    static HELD: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

fn lock_file(key: &str) -> PathBuf {
    paths::locks_dir().join(format!("sync-{key}.lock"))
}

/// The right to read, decide about and write a game's save line on this
/// device, until dropped ([`lock_game`]).
pub struct GameLock {
    key: Option<String>,
    _file: Option<FileLock>,
}

impl Drop for GameLock {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            let _ = HELD.try_with(|h| h.borrow_mut().remove(key));
        }
    }
}

/// One sync of a game at a time on this device: the launch wrapper, the
/// process that finishes its upload, the tray app and the command line all
/// take this lock first. Waits for whoever has it, however long that takes,
/// unless `gate` is given up meanwhile (an error that ends a sync as
/// [`Outcome::Offline`]). A holder that is gone doesn't keep it
/// ([`FileLock::hold`]).
pub fn lock_game(key: &str, gate: &Gate) -> Result<GameLock> {
    if HELD.with(|h| h.borrow().contains(key)) {
        return Ok(GameLock { key: None, _file: None });
    }
    let mut told = false;
    let file = FileLock::hold(&lock_file(key), &mut || {
        if !told {
            log(format!("{key}: waiting for another sync of this game to finish"));
            told = true;
        }
        !gate.given_up()
    })?;
    let Some(file) = file else { return Err(GaveUp.into()) };
    HELD.with(|h| h.borrow_mut().insert(key.to_string()));
    Ok(GameLock { key: Some(key.to_string()), _file: Some(file) })
}

/// Whether a sync of `key` runs on this device right now, in any process.
pub fn sync_running(key: &str) -> bool {
    FileLock::is_held(&lock_file(key))
}

// ---------------------------------------------------- owed after playing

fn owed_file(key: &str) -> PathBuf {
    paths::owed_dir().join(format!("{key}.json"))
}

/// Note that the sync of `key` after playing is still owed: the launch
/// wrapper stopped waiting for it and exits. Whoever syncs the game next
/// with a final outcome settles it; [`finish_owed`] is there for just that.
pub fn owe(key: &str) -> Result<()> {
    let note = serde_json::json!({ "since": Utc::now(), "pid": std::process::id() });
    util::atomic_write(&owed_file(key), note.to_string().as_bytes())
}

pub fn is_owed(key: &str) -> bool {
    owed_file(key).is_file()
}

/// The games whose sync after playing is still owed.
pub fn owed_keys() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(paths::owed_dir()) else { return Vec::new() };
    let mut keys: Vec<String> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    keys.sort();
    keys
}

fn settle_owed(key: &str) {
    if std::fs::remove_file(owed_file(key)).is_ok() {
        log(format!("{key}: the sync after playing is done"));
    }
}

/// Do the sync of `key` after playing that is still owed ([`owe`]): the
/// usual sync, as right after the game. `None` when nothing is owed (any
/// more). The note goes once the outcome is final ([`Outcome::is_final`]),
/// and stays while the cloud can't be reached. A conflict is only noted:
/// the next launch of the game asks.
pub fn finish_owed(ctx: &Ctx, key: &str) -> Result<Option<Outcome>> {
    let gate = Gate::default();
    let _lock = lock_game(key, &gate)?;
    // Whoever had the lock may have done it meanwhile.
    if !is_owed(key) {
        return Ok(None);
    }
    // Being played again: no sync may change its saves now, and the one
    // after this session settles it.
    if matches!(crate::catchup::runner_phase(key).as_deref(), Some("preLaunch" | "playing")) {
        log(format!("{key}: being played again; the sync after this session does what was owed"));
        return Ok(None);
    }
    let Some(game) = ctx.cfg.game(key).filter(|g| g.auto_sync).cloned() else {
        // The game is gone from this device, or doesn't sync by itself now.
        settle_owed(key);
        return Ok(None);
    };
    sync_with(ctx, &game, Resolution::Auto, Trigger::AutoExit, &gate).map(Some)
}

// ---------------------------------------------------------------- one sync

/// One game's save line, opened for a sync.
struct Session<'a> {
    ctx: &'a Ctx,
    cloud: &'a Cloud,
    game: &'a GameEntry,
    line: OpenLine,
    /// Whoever waits for this sync, and whether they still do.
    gate: Gate,
}

impl<'a> Session<'a> {
    fn open(ctx: &'a Ctx, cloud: &'a Cloud, game: &'a GameEntry) -> Result<Session<'a>> {
        Ok(Session { ctx, cloud, game, line: cloud.open(&game.key)?, gate: Gate::default() })
    }

    fn key(&self) -> &str {
        &self.game.key
    }

    fn summary(&self, id: &str) -> Option<SnapshotSummary> {
        let i = self.line.view.snapshots.iter().position(|s| s.id == id)?;
        Some(SnapshotSummary::of(&self.line.view.snapshots[i], self.line.view.heads.contains(&i)))
    }

    /// Queue a snapshot of the saves here and upload it. Uploading waits
    /// for later when the cloud can't be reached, and before playing when
    /// the cloud would have to be upgraded first: that can take a while,
    /// and nothing is lost by doing it after the game.
    fn push(&mut self, local: &LocalState, mut shot: Shot, launching: bool) -> Result<Outcome> {
        if shot.base.is_none() && !shot.side {
            shot.base = base_manifest(self.key(), &state::game(self.key()), local.total_size);
        }
        let manifest = stage_local(self.ctx, self.game, &self.line.place, &self.line.view, local, &shot)?;
        let id = manifest.id.clone();
        if launching && self.cloud.is_legacy().unwrap_or(false) {
            let reason = crate::tr!("云端要先升级到新的格式，这一局结束后再上传", "The cloud has to be upgraded first; this is uploaded after the game");
            log(format!("{}: queued {id} (the cloud is upgraded after playing)", self.key()));
            return Ok(Outcome::Queued { id, reason });
        }
        // Nobody waits any more: it waits in the queue, as one made offline.
        if self.gate.given_up() {
            log(format!("{}: queued {id} (not waited for)", self.key()));
            return Ok(Outcome::Queued { id, reason: GaveUp.to_string() });
        }
        self.gate.set_phase(Phase::Uploading);
        match flush(self.cloud, &mut self.line, Some(self.game)) {
            Ok(_) => Ok(Outcome::Uploaded { id }),
            Err(e) if must_wait(&e) => {
                log(format!("{}: queued {id} ({e:#})", self.key()));
                Ok(Outcome::Queued { id, reason: format!("{e:#}") })
            }
            Err(e) => Err(e),
        }
    }

    /// Save the saves here beside the history, unless the cloud already has
    /// the same content. Always a complete copy, and one stays on this
    /// device too. Returns the version that holds them (none when there are
    /// no saves here).
    fn protect(&mut self, local: &LocalState, trigger: Trigger) -> Result<Option<String>> {
        if local.is_empty() {
            return Ok(None);
        }
        if let Some(same) = self.line.view.snapshots.iter().find(|s| s.content_hash == local.hash) {
            return Ok(Some(same.id.clone()));
        }
        let shot = Shot { parent: state::game(self.key()).last_synced_id, trigger: Some(trigger), side: true, ..Shot::default() };
        let manifest = stage_local(self.ctx, self.game, &self.line.place, &self.line.view, local, &shot)?;
        keep_local_copy(self.key(), &manifest)?;
        // Nobody waits any more: it waits in the queue.
        if self.gate.given_up() {
            return Ok(Some(manifest.id));
        }
        self.gate.set_phase(Phase::Uploading);
        match flush(self.cloud, &mut self.line, Some(self.game)) {
            Ok(_) => {
                log(format!("{}: protected local saves as {} ({})", self.key(), manifest.id, manifest.trigger));
                Ok(Some(manifest.id))
            }
            // It waits in the queue.
            Err(e) if must_wait(&e) => Ok(Some(manifest.id)),
            Err(e) => Err(e),
        }
    }

    /// Replace the saves here with version `id`. Once the waiting is given
    /// up ([`Gate`]) the saves stay as they are, however far the download
    /// got.
    fn pull(&mut self, local: &LocalState, id: &str) -> Result<Outcome> {
        let target = self
            .line
            .view
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!(crate::tr!("云端找不到这个版本", "This version wasn't found in the cloud")))?;
        if !target.unsupported().is_empty() {
            bail!(crate::tr!(
                "这个版本是更新版本的 Ludonimbus 存的，这台设备上的 Ludonimbus 需要先更新",
                "This version was saved by a newer Ludonimbus. Update Ludonimbus on this device first"
            ));
        }
        let back = self.protect(local, Trigger::PreRestore)?;
        self.gate.check()?;
        let manifest = saves::manifest(self.cloud.files(), &self.line.place, &self.line.view, id)?;
        self.gate.check()?;
        // A version that shares the line through bindings needs a folder
        // for every file.
        crate::join::no_place(self.game, &SnapshotMeta::of(&manifest, self.key(), None).files)?;
        let mut packs: BTreeMap<String, u64> = BTreeMap::new();
        for f in &manifest.files {
            *packs.entry(f.pack.clone().unwrap_or_else(|| manifest.id.clone())).or_default() += f.size;
        }
        self.gate.set_phase(Phase::Downloading { done: 0, total: packs.values().sum() });
        let store = Watched { inner: self.cloud.files(), gate: &self.gate, packs };
        let work = paths::staging_dir().join(format!("pull-{id}"));
        let _ = std::fs::remove_dir_all(&work);
        let files = work.join("files");
        let result = saves::fetch(&store, &self.line.place, &self.line.view, &manifest, None, &files, &work.join("packs"))
            // Whoever waited may have stopped while it downloaded.
            .and_then(|_| if self.gate.start_applying() { Ok(()) } else { Err(GaveUp.into()) })
            .and_then(|_| snapshot::apply_dir(&files, &SnapshotMeta::of(&manifest, self.key(), None).files, self.game));
        let _ = std::fs::remove_dir_all(&work);
        result?;
        mark_synced(self.key(), id, &target.vv, &target.content_hash, &target.device_name, self.ctx.device().0)?;
        remember_base(self.key(), Some(&manifest));
        note_replaced(self.key(), back, id, &target.device_name);
        log(format!("{}: pulled {id} from {}", self.key(), target.device_name));
        Ok(Outcome::Pulled {
            id: id.to_string(),
            device: target.device_name.clone(),
            created_at: target.created_at,
        })
    }

    /// After [`Session::pull`] of a version that is not the one newest
    /// version: upload the saves as they are now on top of every head, so
    /// the line has one newest version again and it is this one. Before
    /// playing (`launching`) a cloud that needs upgrading first gets the
    /// version after the game, as with any upload then.
    fn make_current(&mut self, from: &str, trigger: Trigger, launching: bool) -> Result<Option<Outcome>> {
        if self.line.view.head().is_some_and(|h| h.id == from) {
            return Ok(None);
        }
        let local = snapshot::local_state(self.game)?;
        let loc = local_of(&local, &state::game(self.key()));
        let Action::Push { bases, .. } = decide::settle(&loc, &self.line.view) else { return Ok(None) };
        let origin = Origin {
            game: self.line.place.game.clone(),
            line: self.line.place.line.clone(),
            snapshot: from.to_string(),
            how: "restore".to_string(),
            extra: v2::Extra::new(),
        };
        let shot = Shot { bases, parent: Some(from.to_string()), trigger: Some(trigger), origin: Some(origin), ..Shot::default() };
        let outcome = self.push(&local, shot, launching)?;
        if let Outcome::Uploaded { id } | Outcome::Queued { id, .. } = &outcome {
            let _ = state::update_game(self.key(), |g| {
                if let Some(r) = g.replaced.as_mut().filter(|r| r.now == from) {
                    r.now = id.clone();
                }
            });
        }
        Ok(Some(outcome))
    }

    fn conflict(&mut self, local: &LocalState, candidates: &[String], local_is_candidate: bool, trigger: Trigger) -> Result<Outcome> {
        // After playing, make sure the new local saves reach the cloud right
        // away even though the user still has to choose.
        if local_is_candidate && matches!(trigger, Trigger::AutoExit | Trigger::Passive) {
            self.protect(local, Trigger::ConflictKeep)?;
        }
        // The one the user is shown against the saves here: when those are
        // one side of a fork already, the other side.
        let newest = candidates
            .iter()
            .rev()
            .find(|id| self.line.view.get(id).is_some_and(|s| s.content_hash != local.hash))
            .or(candidates.last())
            .and_then(|id| self.summary(id))
            .ok_or_else(|| anyhow!(crate::tr!("云端找不到这个版本", "This version wasn't found in the cloud")))?;
        state::update_game(self.key(), |g| {
            g.conflict = Some(ConflictInfo {
                cloud_id: newest.id.clone(),
                cloud_device: newest.device_name.clone(),
                cloud_created_at: newest.created_at,
                local_hash: local.hash.clone(),
                detected_at: Utc::now(),
                candidates: candidates.to_vec(),
                local_is_candidate,
            });
        })?;
        log(format!("{}: conflict with {} from {}", self.key(), newest.id, newest.device_name));
        Ok(Outcome::Conflict {
            local_files: local.files.len(),
            local_size: local.total_size,
            cloud: newest,
        })
    }
}

/// Keep a copy of a queued snapshot on this PC (safety net for the saves a
/// pull is about to replace). The newest 10 of a game stay.
fn keep_local_copy(key: &str, manifest: &Manifest) -> Result<()> {
    let dir = paths::local_backups_dir().join(key);
    std::fs::create_dir_all(&dir)?;
    let queue = queue_dir(key);
    for pack in manifest.packs() {
        let zip = queue.join(format!("{pack}.zip"));
        if zip.is_file() {
            std::fs::copy(&zip, dir.join(format!("{pack}.zip")))?;
        }
    }
    std::fs::write(dir.join(format!("{}.json", manifest.id)), serde_json::to_vec_pretty(manifest)?)?;
    let mut kept: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    kept.sort();
    while kept.len() > 10 {
        let json = kept.remove(0);
        let id = json.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&id) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(())
}

/// When the cloud is unreachable: after playing, keep the new saves in the
/// queue so they are uploaded later.
fn offline(ctx: &Ctx, game: &GameEntry, trigger: Trigger, err: anyhow::Error) -> Result<Outcome> {
    let message = format!("{err:#}");
    log(format!("{}: offline ({message})", game.key));
    let _ = state::update_game(&game.key, |g| g.last_error = Some(message.clone()));
    if matches!(trigger, Trigger::PreLaunch) {
        return Ok(Outcome::Offline { message });
    }
    let local = snapshot::local_state(game)?;
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
fn fresh(ctx: &Ctx, game: &GameEntry) -> GameEntry {
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
        if let Err(e) = flush(cloud, &mut s.line, Some(game)) {
            if must_wait(&e) {
                return offline(ctx, game, trigger, e);
            }
            return Err(e);
        }
        gate.check()?;
        gate.set_phase(Phase::Checking);
    }
    let local = snapshot::local_state(game)?;
    let st = state::game(&game.key);
    let seen = with_queue(&s.line.view, &s.line.place, &game.key);
    let loc = local_of(&local, &st);
    let mut action = decide::decide(&loc, &seen);
    if game.backup_only() && resolution == Resolution::Auto {
        action = backup_only(&game.key, action, &loc, &seen);
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
            let shown = st.conflict.as_ref().map(|c| c.cloud_id.clone()).filter(|id| s.line.view.head_entries().any(|h| &h.id == id));
            let Some(target) = shown.or_else(|| other_side(&s.line.view, &local.hash)) else {
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
            s.push(&local, Shot { bases, parent, trigger: Some(Trigger::SyncNow), comment, ..Shot::default() }, launching)?
        }
        (Action::Pull { id }, Resolution::Auto) => s.pull(&local, &id)?,
        (Action::Push { bases, parent }, Resolution::Auto) => s.push(&local, Shot { bases, parent, trigger: Some(trigger), ..Shot::default() }, launching)?,
        (Action::Conflict { candidates, local_is_candidate }, Resolution::Auto) => s.conflict(&local, &candidates, local_is_candidate, trigger)?,
    };
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
    let view = match cloud.open(&game.key) {
        Ok(l) => with_queue(&l.view, &l.place, &game.key),
        Err(e) => {
            report.status = if cloud::is_unavailable(&e) { Status::Offline } else { Status::Error };
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    report.snapshot_count = view.snapshots.len();
    report.head = view.heads.last().and_then(|&i| view.snapshots.get(i)).map(|s| SnapshotSummary::of(s, true));
    let loc = local_of(&local, &st);
    let action = decide::decide(&loc, &view);
    report.status = decide::Status::of(&action, &loc, &view).into();
    report
}

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
        Err(e) if cloud::is_unavailable(&e) => {
            // Not reachable: it waits in the queue, built on what this
            // device synced last.
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
    flush(cloud, &mut s.line, Some(game))?;
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
    flush(cloud, &mut s.line, Some(game))?;
    let local = snapshot::local_state(game)?;
    let pulled = s.pull(&local, id)?;
    // Make the restored content the head so the next sync keeps it.
    Ok(s.make_current(id, Trigger::Restore, false)?.unwrap_or(pulled))
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
pub fn apply_external(ctx: &Ctx, game: &GameEntry, comment: &str, change: impl FnOnce() -> Result<()>) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let cloud = ctx.cloud()?;
    let mut s = Session::open(ctx, cloud, game)?;
    flush(cloud, &mut s.line, Some(game))?;
    if let Ok(local) = snapshot::local_state(game) {
        s.protect(&local, Trigger::PreRestore)?;
    }
    change()?;
    let after = snapshot::local_state(game)?;
    if after.is_empty() {
        bail!(crate::tr!("应用后没有找到「{}」的存档文件", "No save files of \"{}\" found after applying", game.name));
    }
    let loc = local_of(&after, &state::game(&game.key));
    let Action::Push { bases, parent } = decide::settle(&loc, &s.line.view) else { return Ok(Outcome::NothingToSave) };
    let outcome = s.push(&after, Shot { bases, parent, trigger: Some(Trigger::Restore), comment: comment.to_string(), ..Shot::default() }, false)?;
    best_effort_housekeeping(ctx, cloud, game);
    Ok(outcome)
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
        .map(|t| cloud::TrashedSnapshot { snapshot: SnapshotSummary::of(&t.snapshot, false), deleted_at: t.deleted_at, deleted_by: t.deleted_by })
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

// ----------------------------------------------------------------- save lines

/// Upload what waits for the cloud to the line the game follows now: the
/// first step of following another one.
pub(crate) fn flush_queue(cloud: &Cloud, game: &GameEntry) -> Result<()> {
    let mut line = cloud.open(&game.key)?;
    flush(cloud, &mut line, Some(game))?;
    Ok(())
}

/// Forget what this device synced of the game: on the line it follows from
/// here on it has synced nothing yet.
pub(crate) fn start_over(key: &str) -> Result<()> {
    state::update_game(key, |g| {
        g.last_synced_id = None;
        g.last_synced_hash = None;
        g.last_synced_vv = Vv::new();
        g.last_synced_device = None;
        g.last_sync_at = None;
        g.counter = 0;
        g.line = None;
        g.conflict = None;
        g.last_error = None;
        g.replaced = None;
    })?;
    remember_base(key, None);
    Ok(())
}

/// A save line a game on this device can follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineInfo {
    pub id: String,
    /// What a device called the line it keeps to itself. Empty for the
    /// shared line.
    pub name: String,
    /// The line the game's devices share.
    pub shared: bool,
    /// The line the game follows on this device.
    pub current: bool,
    /// The devices on it, by name, as far as each has told the cloud.
    pub devices: Vec<String>,
    /// The same, one for each version of the game a device has on it.
    pub members: Vec<LineMember>,
}

/// One version of a game on one device, on a save line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineMember {
    pub device: String,
    /// [`platform`]; empty when the device hasn't said (a record written
    /// before it did).
    pub platform: String,
    /// It is this device.
    pub here: bool,
}

/// Which save line a game follows on this device (docs/plan-v2.md,
/// section 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineChoice {
    /// The line the game's devices share.
    Shared,
    /// A line of this device's own. `name` is what other devices see it as;
    /// without one, the device's name.
    Independent { name: Option<String> },
    /// A line another device keeps to itself ([`LineInfo::id`]).
    Follow { line: String },
}

/// The save lines the game can follow on this device: the shared one first,
/// then each line a device keeps to itself. The line it follows now is
/// always among them, in the cloud yet or not.
pub fn lines(ctx: &Ctx, game: &GameEntry) -> Result<Vec<LineInfo>> {
    let cloud = ctx.cloud()?;
    let here = cloud.place(&game.key)?;
    let known = cloud.lines_of(&game.key)?;
    let devices = cloud.list_devices().unwrap_or_default();
    let (own_id, own_name) = ctx.device();
    let me = || LineMember { device: own_name.to_string(), platform: platform(game), here: true };
    // A device with two versions of the game on one line is named once.
    let names = |members: &[LineMember]| {
        let mut out: Vec<String> = Vec::new();
        for m in members {
            if !out.contains(&m.device) {
                out.push(m.device.clone());
            }
        }
        out
    };
    let mut out: Vec<LineInfo> = known
        .iter()
        .map(|l| {
            let current = l.path == here.dir();
            // This device's own record may be hours old: it is where the
            // game says it is.
            let mut members: Vec<LineMember> = devices
                .iter()
                .filter(|d| d.id != own_id)
                .flat_map(|d| d.installs.iter().filter(|i| i.dir == l.path).map(|i| LineMember { device: d.name.clone(), platform: i.platform.clone(), here: false }))
                .collect();
            if current {
                members.push(me());
            }
            members.dedup();
            LineInfo { id: l.id.clone(), name: l.name.clone().unwrap_or_default(), shared: l.scope != v2::SCOPE_INDEPENDENT, current, devices: names(&members), members }
        })
        .collect();
    out.sort_by_key(|l| !l.shared);
    if !out.iter().any(|l| l.shared) {
        let current = here.line == library::MAIN_LINE && !out.iter().any(|l| l.current);
        let members = if current { vec![me()] } else { Vec::new() };
        out.insert(0, LineInfo { id: library::MAIN_LINE.to_string(), name: String::new(), shared: true, current, devices: names(&members), members });
    }
    if !out.iter().any(|l| l.current) {
        out.push(LineInfo { id: here.line.clone(), name: own_name.to_string(), shared: false, current: true, devices: vec![own_name.to_string()], members: vec![me()] });
    }
    Ok(out)
}

/// The newest version of a save line of the game that this device does
/// not follow.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherSave {
    /// [`LineInfo::id`].
    pub line: String,
    /// What a device called the line it keeps to itself; empty for the
    /// shared line.
    pub name: String,
    pub shared: bool,
    pub snapshot: SnapshotSummary,
}

/// The newest version of every other save line the game could follow
/// ([`lines`]): the shared one when this device keeps its saves to itself,
/// and the lines other devices keep. What [`take_from`] can bring here.
/// Each line costs what opening a line costs (a listing, and its index
/// when the listing is new to this device). A line that has forked, or has
/// nothing yet, is left out.
pub fn other_saves(ctx: &Ctx, game: &GameEntry) -> Result<Vec<OtherSave>> {
    let cloud = ctx.cloud()?;
    let mut out = Vec::new();
    for (line, place) in cloud.other_lines(&game.key)? {
        let view = cloud.peek(&place)?;
        let Some(head) = view.head() else { continue };
        out.push(OtherSave {
            line: line.id.clone(),
            name: line.name.clone().unwrap_or_default(),
            shared: line.scope != v2::SCOPE_INDEPENDENT,
            snapshot: SnapshotSummary::of(head, true),
        });
    }
    Ok(out)
}

/// Make a version of another save line of the game the progress here
/// (docs/plan-v2.md, section 9: "copy"). The saves here are kept beside the
/// history first, then replaced by that version, and the result is the
/// newest version of the line this device follows; the line it came from
/// stays as it is. The new version says where it came from.
pub fn take_from(ctx: &Ctx, game: &GameEntry, line: &str, id: &str) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let cloud = ctx.cloud()?;
    let mut s = Session::open(ctx, cloud, game)?;
    flush(cloud, &mut s.line, Some(game))?;
    let Some((_, place)) = cloud.other_lines(&game.key)?.into_iter().find(|(l, _)| l.id == line) else {
        bail!(crate::tr!("云端没有这条存档线", "The cloud doesn't have this save line"));
    };
    let view = cloud.peek(&place)?;
    let target = view.get(id).cloned().ok_or_else(|| anyhow!(crate::tr!("云端找不到这个版本", "This version wasn't found in the cloud")))?;
    if !target.unsupported().is_empty() {
        bail!(crate::tr!(
            "这个版本是更新版本的 Ludonimbus 存的，这台设备上的 Ludonimbus 需要先更新",
            "This version was saved by a newer Ludonimbus. Update Ludonimbus on this device first"
        ));
    }
    let local = snapshot::local_state(game)?;
    let back = s.protect(&local, Trigger::PreRestore)?;
    let manifest = saves::manifest(cloud.files(), &place, &view, id)?;
    let files = SnapshotMeta::of(&manifest, &game.key, None).files;
    crate::join::no_place(game, &files)?;
    let work = paths::staging_dir().join(format!("take-{id}"));
    let _ = std::fs::remove_dir_all(&work);
    let dir = work.join("files");
    let result = saves::fetch(cloud.files(), &place, &view, &manifest, None, &dir, &work.join("packs")).and_then(|_| snapshot::apply_dir(&dir, &files, game));
    let _ = std::fs::remove_dir_all(&work);
    result?;
    log(format!("{}: took {id} of the line {line} ({})", game.key, target.device_name));
    let after = snapshot::local_state(game)?;
    let loc = local_of(&after, &state::game(&game.key));
    let Action::Push { bases, parent } = decide::settle(&loc, &s.line.view) else { return Ok(Outcome::NothingToSave) };
    let origin = Origin { game: place.game.clone(), line: place.line.clone(), snapshot: id.to_string(), how: "copy".to_string(), extra: v2::Extra::new() };
    let outcome = s.push(&after, Shot { bases, parent, trigger: Some(Trigger::Restore), origin: Some(origin), ..Shot::default() }, false)?;
    if let Outcome::Uploaded { id: made } | Outcome::Queued { id: made, .. } = &outcome {
        note_replaced(&game.key, back, made, &target.device_name);
    }
    best_effort_housekeeping(ctx, cloud, game);
    Ok(outcome)
}

fn set_line(key: &str, line: &str) -> Result<()> {
    config::update(|c| match c.games.iter_mut().find(|g| g.key == key) {
        Some(g) => {
            g.line = Some(line.to_string());
            Ok(())
        }
        None => bail!(crate::tr!("没有找到游戏 {key}", "The game {key} wasn't found")),
    })
}

/// Make the game follow another save line on this device.
///
/// What waits for the cloud is uploaded to the line the game leaves first;
/// when that can't be done now, nothing changes. The line it leaves stays
/// as it is.
///
/// - [`LineChoice::Independent`]: the line of this device's own continues
///   with the saves that are here now (a new line begins with them). The
///   version says which one of the line it left it was, and a large save
///   brings no content along that the cloud already has.
/// - [`LineChoice::Shared`], [`LineChoice::Follow`]: as on a device that
///   syncs the game for the first time. Saves that differ from the line's
///   newest version are a conflict the user settles ([`Outcome::Conflict`],
///   then [`sync`] with the choice); the side that loses stays in the cloud.
///
/// Choosing the line the game is on already changes nothing but its name
/// (`Independent` with one), and settles the choice: the device's default
/// no longer applies to this game.
pub fn use_line(ctx: &Ctx, game: &GameEntry, choice: LineChoice) -> Result<Outcome> {
    let _lock = lock_game(&game.key, &Gate::default())?;
    let game = &fresh(ctx, game);
    let cloud = ctx.cloud()?;
    let key = game.key.as_str();
    let from = cloud.following(key);
    let to = match &choice {
        LineChoice::Shared => library::MAIN_LINE.to_string(),
        LineChoice::Independent { .. } => cloud::own_line_id(cloud.device_id(), key),
        LineChoice::Follow { line } => line.clone(),
    };
    let staying = to == from;
    let local = snapshot::local_state(game)?;
    // Where the saves here come from, and what they can share content with.
    let mut carried: (Option<Origin>, Option<Manifest>) = (None, None);
    if !staying {
        let mut old = cloud.open(key)?;
        flush(cloud, &mut old, Some(game))?;
        if to != library::MAIN_LINE && !matches!(choice, LineChoice::Independent { .. }) && !cloud.lines_of(key)?.iter().any(|l| l.id == to) {
            bail!(crate::tr!("云端没有这条存档线", "The cloud doesn't have this save line"));
        }
        let st = state::game(key);
        let origin = st.last_synced_id.clone().map(|snapshot| Origin {
            game: old.place.game.clone(),
            line: old.place.line.clone(),
            snapshot,
            how: "copy".to_string(),
            extra: v2::Extra::new(),
        });
        carried = (origin, base_manifest(key, &st, local.total_size));
    }
    set_line(key, &to)?;
    cloud.follow(key, &to);
    if !staying {
        // On the other line this device has synced nothing yet.
        start_over(key)?;
        log(format!("{key}: follows the save line {to} now (was {from})"));
    }
    let game = &GameEntry { line: Some(to), ..game.clone() };
    let outcome = match choice {
        LineChoice::Independent { name } => {
            // The line is in the game's record from here on, where the
            // other devices see it and can follow it.
            let mut s = Session::open(ctx, cloud, game)?;
            ready_to_write(cloud, &mut s.line, Some(game))?;
            if let Some(name) = name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                cloud.name_line(key, name)?;
            }
            let loc = local_of(&local, &state::game(key));
            match decide::decide(&loc, &s.line.view) {
                Action::Push { .. } | Action::Conflict { .. } if !staying => match decide::settle(&loc, &s.line.view) {
                    Action::Push { bases, parent } => {
                        let (origin, base) = carried;
                        let pushed = s.push(&local, Shot { bases, parent, trigger: Some(Trigger::SyncNow), origin, base, ..Shot::default() }, false)?;
                        best_effort_housekeeping(ctx, cloud, game);
                        pushed
                    }
                    _ => Outcome::NothingToSave,
                },
                _ => sync(ctx, game, Resolution::Auto, Trigger::SyncNow)?,
            }
        }
        _ => sync(ctx, game, Resolution::Auto, Trigger::SyncNow)?,
    };
    // The other devices list who is on which line: tell them now rather
    // than at the next of this device's regular updates.
    if !staying && cloud.put_device(&cloud::device_info(&ctx.cfg)).is_ok() {
        let _ = state::update(|s| s.device_seen_at = Some(Utc::now()));
    }
    Ok(outcome)
}
