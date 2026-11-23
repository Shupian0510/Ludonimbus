//! Steam-Cloud-like sync decisions.
//!
//! | local since last sync | cloud since last sync | action                         |
//! |-----------------------|-----------------------|--------------------------------|
//! | unchanged             | unchanged             | nothing                        |
//! | unchanged             | new snapshot          | pull (protect local first)     |
//! | changed               | unchanged             | upload                         |
//! | changed               | new snapshot          | conflict, keep both            |
//! | no saves              | has snapshots         | pull (reinstalled game)        |
//! | unknown (new PC)      | has snapshots         | equal → synced, else conflict  |
//! | offline               | —                     | continue, queue uploads        |
//!
//! Nothing is ever deleted except by retention, which never touches manual,
//! locked, head or last-synced snapshots.

use crate::cloud::{self, CloudGame, CloudProvider};
use crate::config::{self, AppConfig, GameEntry};
use crate::snapshot::{self, LocalState, SnapshotMeta, SnapshotSummary, Trigger};
use crate::state::{self, ConflictInfo};
use crate::{logger, paths};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashSet;

pub struct Ctx {
    pub cfg: AppConfig,
    cloud: Option<Box<dyn CloudProvider>>,
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

    pub fn cloud(&self) -> Result<&dyn CloudProvider> {
        if let Some(c) = self.cloud.as_deref() {
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize)]
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

/// Newest snapshot that is part of the main history.
pub fn mainline_head(list: &[SnapshotSummary]) -> Option<SnapshotSummary> {
    list.iter()
        .filter(|s| s.trigger.is_mainline())
        .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
        .cloned()
}

fn log(msg: impl AsRef<str>) {
    logger::log("sync", msg);
}

fn mark_synced(key: &str, id: &str, hash: &str, device: &str) -> Result<()> {
    state::update_game(key, |g| {
        g.last_synced_id = Some(id.to_string());
        g.last_synced_hash = Some(hash.to_string());
        g.last_sync_at = Some(Utc::now());
        g.last_synced_device = Some(device.to_string());
        g.conflict = None;
        g.last_error = None;
    })
}

fn queue_pending(meta: &SnapshotMeta, zip: &std::path::Path) -> Result<()> {
    let dir = paths::pending_dir().join(&meta.game);
    std::fs::create_dir_all(&dir)?;
    std::fs::rename(zip, dir.join(format!("{}.zip", meta.id)))
        .or_else(|_| std::fs::copy(zip, dir.join(format!("{}.zip", meta.id))).map(|_| ()))?;
    std::fs::write(dir.join(format!("{}.json", meta.id)), serde_json::to_vec_pretty(meta)?)?;
    state::update_game(&meta.game, |g| {
        if !g.pending.contains(&meta.id) {
            g.pending.push(meta.id.clone());
        }
    })
}

/// Upload snapshots that were created while offline.
pub fn flush_pending(cloud: &dyn CloudProvider, key: &str) -> Result<usize> {
    let dir = paths::pending_dir().join(key);
    let mut metas = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                if let Ok(m) = serde_json::from_slice::<SnapshotMeta>(&std::fs::read(&p)?) {
                    metas.push(m);
                }
            }
        }
    }
    metas.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut n = 0;
    for m in metas {
        let zip = dir.join(format!("{}.zip", m.id));
        cloud.upload(&m, &zip)?;
        let _ = std::fs::remove_file(&zip);
        let _ = std::fs::remove_file(dir.join(format!("{}.json", m.id)));
        state::update_game(key, |g| g.pending.retain(|x| x != &m.id))?;
        log(format!("{key}: uploaded pending snapshot {}", m.id));
        n += 1;
    }
    Ok(n)
}

fn best_effort_housekeeping(ctx: &Ctx, cloud: &dyn CloudProvider, game: &GameEntry) {
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
}

fn upload_local(
    ctx: &Ctx,
    cloud: &dyn CloudProvider,
    game: &GameEntry,
    local: &LocalState,
    parent: Option<String>,
    trigger: Trigger,
    comment: &str,
) -> Result<Outcome> {
    let (meta, zip) = snapshot::build(game, local, trigger, parent, ctx.device(), comment)?;
    match cloud.upload(&meta, &zip) {
        Ok(()) => {
            let _ = std::fs::remove_file(&zip);
            mark_synced(&game.key, &meta.id, &meta.content_hash, &meta.device_name)?;
            log(format!("{}: uploaded {} ({:?})", game.key, meta.id, trigger));
            if let Err(e) = prune(ctx, cloud, game) {
                log(format!("{}: prune failed: {e:#}", game.key));
            }
            Ok(Outcome::Uploaded { id: meta.id })
        }
        Err(e) if cloud::is_unavailable(&e) => {
            queue_pending(&meta, &zip)?;
            mark_synced(&game.key, &meta.id, &meta.content_hash, &meta.device_name)?;
            log(format!("{}: queued {} ({e:#})", game.key, meta.id));
            Ok(Outcome::Queued {
                id: meta.id,
                reason: format!("{e:#}"),
            })
        }
        Err(e) => Err(e),
    }
}

/// Save the local saves as a side snapshot unless the cloud already has them.
fn protect_local(
    ctx: &Ctx,
    cloud: &dyn CloudProvider,
    game: &GameEntry,
    local: &LocalState,
    list: &[SnapshotSummary],
    trigger: Trigger,
) -> Result<()> {
    if local.is_empty() || list.iter().any(|s| s.content_hash == local.hash) {
        return Ok(());
    }
    let parent = state::game(&game.key).last_synced_id;
    let (meta, zip) = snapshot::build(game, local, trigger, parent, ctx.device(), "")?;
    snapshot::keep_local_copy(&meta, &zip)?;
    match cloud.upload(&meta, &zip) {
        Ok(()) => {
            let _ = std::fs::remove_file(&zip);
            log(format!("{}: protected local saves as {} ({:?})", game.key, meta.id, trigger));
        }
        Err(e) if cloud::is_unavailable(&e) => queue_pending(&meta, &zip)?,
        Err(e) => return Err(e),
    }
    Ok(())
}

fn pull(
    ctx: &Ctx,
    cloud: &dyn CloudProvider,
    game: &GameEntry,
    local: &LocalState,
    list: &[SnapshotSummary],
    target: &SnapshotSummary,
) -> Result<Outcome> {
    protect_local(ctx, cloud, game, local, list, Trigger::PreRestore)?;
    let dl = paths::staging_dir().join(format!("{}-download.zip", target.id));
    cloud.download(&game.key, &target.id, &dl)?;
    let meta = snapshot::read_meta(&dl)?;
    let result = snapshot::apply(&dl, &meta, game);
    let _ = std::fs::remove_file(&dl);
    result?;
    mark_synced(&game.key, &target.id, &target.content_hash, &target.device_name)?;
    log(format!("{}: pulled {} from {}", game.key, target.id, target.device_name));
    Ok(Outcome::Pulled {
        id: target.id.clone(),
        device: target.device_name.clone(),
        created_at: target.created_at,
    })
}

fn record_conflict(game: &GameEntry, local: &LocalState, head: &SnapshotSummary) -> Result<Outcome> {
    state::update_game(&game.key, |g| {
        g.conflict = Some(ConflictInfo {
            cloud_id: head.id.clone(),
            cloud_device: head.device_name.clone(),
            cloud_created_at: head.created_at,
            local_hash: local.hash.clone(),
            detected_at: Utc::now(),
        });
    })?;
    log(format!("{}: conflict with {} from {}", game.key, head.id, head.device_name));
    Ok(Outcome::Conflict {
        local_files: local.files.len(),
        local_size: local.total_size,
        cloud: head.clone(),
    })
}

/// When the cloud is unreachable: after playing, keep the new saves in the
/// pending queue so they are uploaded later.
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
    let (meta, zip) = snapshot::build(game, &local, trigger, st.last_synced_id, ctx.device(), "")?;
    queue_pending(&meta, &zip)?;
    mark_synced(&game.key, &meta.id, &meta.content_hash, &meta.device_name)?;
    Ok(Outcome::Queued { id: meta.id, reason: message })
}

/// Sync one game. `trigger` labels a snapshot uploaded by this call.
pub fn sync(ctx: &Ctx, game: &GameEntry, resolution: Resolution, trigger: Trigger) -> Result<Outcome> {
    let cloud = match ctx.cloud() {
        Ok(c) => c,
        Err(e) if cloud::is_not_configured(&e) => return Ok(Outcome::NoCloud),
        Err(e) => return offline(ctx, game, trigger, e),
    };
    if let Err(e) = flush_pending(cloud, &game.key) {
        if cloud::is_unavailable(&e) {
            return offline(ctx, game, trigger, e);
        }
        return Err(e);
    }
    let list = match cloud.list_snapshots(&game.key) {
        Ok(l) => l,
        Err(e) if cloud::is_unavailable(&e) => return offline(ctx, game, trigger, e),
        Err(e) => return Err(e),
    };
    // Keep launching fast: bookkeeping happens after playing, not before.
    if trigger != Trigger::PreLaunch {
        best_effort_housekeeping(ctx, cloud, game);
    }

    let local = snapshot::local_state(game)?;
    let st = state::game(&game.key);
    let head = mainline_head(&list);

    let Some(head) = head else {
        if local.is_empty() {
            return Ok(Outcome::NothingToSave);
        }
        return upload_local(ctx, cloud, game, &local, None, trigger, "");
    };

    if local.hash == head.content_hash {
        mark_synced(&game.key, &head.id, &head.content_hash, &head.device_name)?;
        return Ok(Outcome::UpToDate);
    }

    match resolution {
        Resolution::UseCloud => return pull(ctx, cloud, game, &local, &list, &head),
        Resolution::UseLocal => {
            if local.is_empty() {
                bail!("本地没有存档，不能以本地为准");
            }
            return upload_local(ctx, cloud, game, &local, Some(head.id.clone()), Trigger::Manual, "冲突时选择保留本地存档");
        }
        Resolution::Auto => {}
    }

    if local.is_empty() {
        return pull(ctx, cloud, game, &local, &list, &head);
    }

    let known_local = list.iter().any(|s| s.content_hash == local.hash);
    let synced_hash = st.last_synced_hash.as_deref();
    let local_changed = synced_hash != Some(local.hash.as_str());
    let cloud_changed = st.last_synced_id.as_deref() != Some(head.id.as_str())
        && synced_hash != Some(head.content_hash.as_str());

    if synced_hash.is_none() {
        if known_local {
            return pull(ctx, cloud, game, &local, &list, &head);
        }
        return conflict_with_backup(ctx, cloud, game, &local, &list, &head, trigger);
    }

    match (local_changed, cloud_changed) {
        (false, true) => pull(ctx, cloud, game, &local, &list, &head),
        (true, false) => upload_local(ctx, cloud, game, &local, Some(head.id.clone()), trigger, ""),
        (true, true) if known_local => pull(ctx, cloud, game, &local, &list, &head),
        _ => conflict_with_backup(ctx, cloud, game, &local, &list, &head, trigger),
    }
}

fn conflict_with_backup(
    ctx: &Ctx,
    cloud: &dyn CloudProvider,
    game: &GameEntry,
    local: &LocalState,
    list: &[SnapshotSummary],
    head: &SnapshotSummary,
    trigger: Trigger,
) -> Result<Outcome> {
    // After playing, make sure the new local saves reach the cloud right away
    // even though the user still has to choose.
    if matches!(trigger, Trigger::AutoExit | Trigger::Passive) {
        protect_local(ctx, cloud, game, local, list, Trigger::ConflictKeep)?;
    }
    record_conflict(game, local, head)
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
    let list = match cloud.list_snapshots(&game.key) {
        Ok(l) => l,
        Err(e) => {
            report.status = if cloud::is_unavailable(&e) { Status::Offline } else { Status::Error };
            report.message = Some(format!("{e:#}"));
            return report;
        }
    };
    report.snapshot_count = list.len();
    let head = mainline_head(&list);
    report.head = head.clone();
    report.status = match &head {
        None if local.is_empty() => Status::Empty,
        None => Status::NotUploaded,
        Some(h) if h.content_hash == local.hash => Status::Synced,
        Some(_) if local.is_empty() => Status::CanRestore,
        Some(_) if st.conflict.is_some() => Status::Conflict,
        Some(h) => {
            let known_local = list.iter().any(|s| s.content_hash == local.hash);
            let synced = st.last_synced_hash.as_deref();
            let local_changed = synced != Some(local.hash.as_str());
            let cloud_changed =
                st.last_synced_id.as_deref() != Some(h.id.as_str()) && synced != Some(h.content_hash.as_str());
            match (synced.is_some(), local_changed, cloud_changed) {
                (false, _, _) if known_local => Status::CloudNewer,
                (false, _, _) => Status::Conflict,
                (true, false, true) => Status::CloudNewer,
                (true, true, false) => Status::LocalChanged,
                (true, true, true) if known_local => Status::CloudNewer,
                _ => Status::Conflict,
            }
        }
    };
    report
}

/// Create a manual snapshot now (kept forever by retention).
pub fn backup_now(ctx: &Ctx, game: &GameEntry, comment: &str) -> Result<Outcome> {
    let local = snapshot::local_state(game)?;
    if local.is_empty() {
        bail!("没有找到「{}」的存档文件", game.name);
    }
    let cloud = ctx.cloud()?;
    let head = match cloud.list_snapshots(&game.key) {
        Ok(l) => mainline_head(&l).map(|h| h.id),
        Err(e) if cloud::is_unavailable(&e) => state::game(&game.key).last_synced_id,
        Err(e) => return Err(e),
    };
    best_effort_housekeeping(ctx, cloud, game);
    upload_local(ctx, cloud, game, &local, head, Trigger::Manual, comment)
}

/// Restore a specific snapshot and make it the newest version.
pub fn restore(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<Outcome> {
    let cloud = ctx.cloud()?;
    let list = cloud.list_snapshots(&game.key)?;
    let target = list
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| anyhow!("云端找不到这个版本"))?;
    let local = snapshot::local_state(game)?;
    let head = mainline_head(&list);
    let outcome = pull(ctx, cloud, game, &local, &list, &target)?;
    if head.as_ref().is_some_and(|h| h.id == target.id) {
        return Ok(outcome);
    }
    // Make the restored content the head so the next sync keeps it.
    let restored = snapshot::local_state(game)?;
    let comment = format!(
        "恢复自 {} 在 {} 的版本",
        target.device_name,
        target.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
    );
    upload_local(ctx, cloud, game, &restored, head.map(|h| h.id), Trigger::Restore, &comment)
}

/// Delete old automatic snapshots according to the retention settings.
pub fn prune(ctx: &Ctx, cloud: &dyn CloudProvider, game: &GameEntry) -> Result<usize> {
    let list = cloud.list_snapshots(&game.key)?;
    let st = state::game(&game.key);
    let mut keep: HashSet<String> = HashSet::new();
    if let Some(h) = mainline_head(&list) {
        keep.insert(h.id);
    }
    if let Some(id) = st.last_synced_id {
        keep.insert(id);
    }
    let mut sorted = list.clone();
    sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    for s in &sorted {
        if s.locked || s.trigger.is_pinned() {
            keep.insert(s.id.clone());
        }
    }
    let autos: Vec<&SnapshotSummary> = sorted
        .iter()
        .filter(|s| !s.locked && !s.trigger.is_pinned())
        .collect();
    for s in autos.iter().take(ctx.cfg.retention.keep_recent) {
        keep.insert(s.id.clone());
    }
    let now = Utc::now();
    let mut days = HashSet::new();
    for s in &autos {
        if (now - s.created_at).num_days() < ctx.cfg.retention.keep_daily_days
            && days.insert(s.created_at.with_timezone(&chrono::Local).date_naive())
        {
            keep.insert(s.id.clone());
        }
    }
    let mut deleted = 0;
    for s in autos {
        if !keep.contains(&s.id) {
            cloud.delete(&game.key, &s.id)?;
            deleted += 1;
        }
    }
    if deleted > 0 {
        log(format!("{}: retention removed {deleted} snapshots", game.key));
    }
    Ok(deleted)
}

pub fn set_snapshot_flags(ctx: &Ctx, game: &GameEntry, id: &str, locked: Option<bool>, comment: Option<String>) -> Result<()> {
    let cloud = ctx.cloud()?;
    let mut meta = cloud.get_meta(&game.key, id)?;
    if let Some(l) = locked {
        meta.locked = l;
    }
    if let Some(c) = comment {
        meta.comment = c;
    }
    cloud.update_meta(&meta)
}

pub fn delete_snapshot(ctx: &Ctx, game: &GameEntry, id: &str) -> Result<()> {
    let cloud = ctx.cloud()?;
    let list = cloud.list_snapshots(&game.key)?;
    let s = list.iter().find(|s| s.id == id).ok_or_else(|| anyhow!("找不到这个版本"))?;
    if s.locked {
        bail!("这个版本已锁定，请先解锁");
    }
    if mainline_head(&list).is_some_and(|h| h.id == id) {
        bail!("不能删除最新版本");
    }
    cloud.delete(&game.key, id)
}

/// Save a snapshot zip to a user-chosen location.
pub fn export_snapshot(ctx: &Ctx, game: &GameEntry, id: &str, dest: &std::path::Path) -> Result<()> {
    ctx.cloud()?.download(&game.key, id, dest)
}
