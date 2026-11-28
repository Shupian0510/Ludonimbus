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
//! one version on the other. A version leaves the queue once its manifest
//! and every pack it names are in the cloud; one that can never be complete
//! any more is set aside on this device ([`set_aside`]).
//!
//! **Versions that miss a pack** (docs/plan-v2.md, section 3.4). One found
//! when it is fetched is marked in the cloud ([`cloud::saves::mark`]) and
//! the decision made again without it; no device takes it while it is
//! marked. After playing, this device puts back what the marked versions it
//! wrote, or whose saves it has, lack ([`repair_marked`]); one it wrote and
//! can't repair goes to 「最近删除」. A device whose base changed tells the
//! others at once when the save is large ([`tell_base`]).
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
//!
//! **Where things are.** This file has the types callers use and what this
//! device remembers of what it synced. The rest is in parts, each with its
//! own notes: `run` (one sync, and [`check`]), `session` (the steps a sync
//! is made of), `queue` and `publish` (the versions waiting on this device,
//! and uploading them), `carry` (another platform's files on a line),
//! `incomplete` (versions that miss a pack), `housekeeping` (what a sync
//! does besides the saves), `gate` and `lock` (not waiting, one sync at a
//! time), `versions` (what the user does with versions) and `savelines`.

mod carry;
mod gate;
mod housekeeping;
mod incomplete;
mod lock;
mod publish;
mod queue;
mod run;
mod savelines;
mod session;
mod versions;

pub use carry::{carried_for, saves_version, too_new};
pub use gate::{Gate, Phase};
pub use incomplete::{dismiss_unsent, export_unsent, unsent};
pub use lock::{backlog, finish_owed, is_owed, lock_game, owe, owe_passive, owed_keys, sync_running, Backlog, BacklogState, GameLock};
pub use publish::{drop_pending, flush_pending, upload_pending};
pub use queue::{pending_keys, pending_metas, pending_zips, queue_files, queue_files_carrying, queued_bases, Foreign};
pub use run::{check, pulls_by_itself, sync, sync_with, upload};
pub(crate) use savelines::{flush_queue, start_over};
pub use savelines::{lines, other_saves, take_from, use_line, LineChoice, LineInfo, LineMember, OtherSave};
pub use versions::{
    apply_external, backup_now, delete_snapshot, deleted_snapshots, export_snapshot, purge_deleted, restore, restore_deleted, set_snapshot_flags, undo,
    undoable, CopyOf,
};

use crate::cloud::decide::{self, Action, Local};
use crate::cloud::line::LineView;
use crate::cloud::v2::{IndexEntry, Manifest};
use crate::cloud::{self, packs, Cloud};
use crate::config::{self, AppConfig, GameEntry};
use crate::snapshot::{LocalState, SnapshotSummary};
use crate::state::{self, GameState};
use crate::vv::Vv;
use crate::{logger, paths, util};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

// The notes in this file name these; only the documentation needs them here.
#[cfg(doc)]
use housekeeping::tell_base;
#[cfg(doc)]
use incomplete::{repair_marked, set_aside};

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
        Ctx { cfg, cloud, cloud_error }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
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
    /// No local saves, while this device synced some before: they were
    /// deleted, or the drive they are on isn't there. Nothing empty is
    /// uploaded, and nothing is written back by itself after playing or in
    /// the background: the user takes them back (「取回」), or a game
    /// started through Steam gets them before it starts.
    SavesMissing,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    /// The game version that saved `head` on another platform, when that is
    /// newer than the one that wrote the saves here and the game's rule says
    /// an older game can't load it ([`too_new`]): a sync nobody chose
    /// doesn't take it, and the app says so before the user does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub too_new: Option<String>,
    /// The line's newest version is marked as missing a content pack: no
    /// device takes it until the pack is back ([`cloud::saves::mark`]), and
    /// `status` is worked out without it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub incomplete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Auto,
    UseLocal,
    /// The newest version in the cloud.
    UseCloud,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    UpToDate,
    Uploaded {
        id: String,
    },
    /// Snapshot created but the cloud is unreachable; uploaded later.
    Queued {
        id: String,
        reason: String,
    },
    Pulled {
        id: String,
        device: String,
        created_at: DateTime<Utc>,
    },
    Conflict {
        local_files: usize,
        local_size: u64,
        cloud: SnapshotSummary,
    },
    NothingToSave,
    NoCloud,
    Offline {
        message: String,
    },
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
    list.iter().filter(|s| !s.side && s.trigger.is_mainline()).max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id))).cloned()
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
    st.last_synced_id.as_deref().and_then(|id| view.get(id)).map(|s| s.vv.clone()).filter(|vv| !vv.is_empty()).unwrap_or_else(|| st.last_synced_vv.clone())
}

fn local_of(local: &LocalState, st: &GameState) -> Local {
    Local { content_hash: (!local.is_empty()).then(|| local.hash.clone()), base: st.base() }
}

/// A device whose saves are still the version it synced last, while that
/// version is marked as missing a pack, holds on to them: it has what the
/// cloud lacks, and puts the pack back after playing ([`repair_marked`]).
/// [`decide`] leaves marked versions out and would see a base that left the
/// line, so it says to upload the same saves again; nothing happens instead.
fn holding(action: Action, local: &Local, view: &LineView) -> Action {
    let Some(base) = &local.base else { return action };
    let marked = view.get(&base.id).is_some_and(IndexEntry::is_incomplete);
    if matches!(action, Action::Push { .. }) && marked && local.content_hash.as_deref() == Some(base.content_hash.as_str()) {
        return Action::Nothing;
    }
    action
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
