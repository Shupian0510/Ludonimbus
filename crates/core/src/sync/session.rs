//! One game's save line, opened for a sync ([`Session`]), and the steps a
//! sync is made of: upload the saves here ([`Session::push`]), keep them
//! beside the history before they are replaced ([`Session::protect`]),
//! replace them with a version from the cloud ([`Session::pull`]), or note
//! a conflict for the user ([`Session::conflict`]). Which step is taken is
//! up to the caller: a sync (`run`), or what the user does with versions
//! and save lines.

use super::carry::{carry, local_here};
use super::gate::{Gate, GaveUp, Phase, Watched};
use super::incomplete::{incomplete_error, keep_local_copy, note_missing};
use super::publish::flush;
use super::queue::{stage_local, with_queue, Shot};
use super::{base_manifest, holding, local_of, log, mark_synced, must_wait, note_replaced, remember_base, Ctx, Outcome};
use crate::cloud::decide::{self, Action, Local};
use crate::cloud::saves;
use crate::cloud::v2::{self, Manifest, Origin};
use crate::cloud::{packs, Cloud, OpenLine};
use crate::config::GameEntry;
use crate::paths;
use crate::snapshot::{self, LocalState, SnapshotMeta, SnapshotSummary, Trigger};
use crate::state::{self, ConflictInfo};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use std::collections::BTreeMap;

/// One game's save line, opened for a sync.
pub(super) struct Session<'a> {
    ctx: &'a Ctx,
    cloud: &'a Cloud,
    game: &'a GameEntry,
    pub(super) line: OpenLine,
    /// Whoever waits for this sync, and whether they still do.
    pub(super) gate: Gate,
}

impl<'a> Session<'a> {
    pub(super) fn open(ctx: &'a Ctx, cloud: &'a Cloud, game: &'a GameEntry) -> Result<Session<'a>> {
        Ok(Session { ctx, cloud, game, line: cloud.open(&game.key)?, gate: Gate::default() })
    }

    fn key(&self) -> &str {
        &self.game.key
    }

    /// The saves here as the line compares them ([`local_here`]).
    pub(super) fn local(&self) -> Result<LocalState> {
        local_here(Some(self.cloud), self.game, Some((&self.line.place, &self.line.view)), true)
    }

    /// The saves here, about to become the newest version over every head
    /// (the user chose them in a conflict): another platform's files go
    /// along from the newest version, not from the one this device was at.
    pub(super) fn over_all(&self, local: LocalState) -> Result<LocalState> {
        if crate::join::by_rule(self.game).is_none() {
            return Ok(local);
        }
        let Some(head) = self.line.view.head_entries().last() else { return Ok(local) };
        let manifest = saves::manifest(self.cloud.files(), &self.line.place, &self.line.view, &head.id)?;
        let (refs, files) = carry(Some(self.cloud), self.game, Some((&self.line.place, &self.line.view)), &manifest, true)?;
        // In place of what went along before.
        Ok(local.carrying(refs, files))
    }

    fn summary(&self, id: &str) -> Option<SnapshotSummary> {
        let i = self.line.view.snapshots.iter().position(|s| s.id == id)?;
        Some(SnapshotSummary::of(&self.line.view.snapshots[i], self.line.view.heads.contains(&i)))
    }

    /// Queue a snapshot of the saves here and upload it. Uploading waits
    /// for later when the cloud can't be reached, and before playing when
    /// the cloud would have to be upgraded first: that can take a while,
    /// and nothing is lost by doing it after the game.
    pub(super) fn push(&mut self, local: &LocalState, mut shot: Shot, launching: bool) -> Result<Outcome> {
        if shot.base.is_none() && !shot.side {
            shot.base = base_manifest(self.key(), &state::game(self.key()), local.total_size);
        }
        // Nothing is taken from a version that misses a pack, nor from one
        // of this line that left it (let go because it missed one, or
        // deleted): what it took from others may be gone too. A version of
        // the line the game came from (choosing another line) is fine.
        let gone = |b: &Manifest| match self.line.view.get(&b.id) {
            Some(e) => e.is_incomplete(),
            None => b.line == self.line.place.line,
        };
        if shot.base.as_ref().is_some_and(gone) {
            shot.base = None;
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
        match flush(self.cloud, &mut self.line, Some(self.game), Some(local.pack_files())) {
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
    pub(super) fn protect(&mut self, local: &LocalState, trigger: Trigger) -> Result<Option<String>> {
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
        match flush(self.cloud, &mut self.line, Some(self.game), Some(local.pack_files())) {
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
    pub(super) fn pull(&mut self, local: &LocalState, id: &str) -> Result<Outcome> {
        let target = self.line.view.get(id).cloned().ok_or_else(|| anyhow!(crate::tr!("云端找不到这个版本", "This version wasn't found in the cloud")))?;
        if !target.unsupported().is_empty() {
            bail!(crate::tr!(
                "这个版本是更新版本的 Ludonimbus 存的，这台设备上的 Ludonimbus 需要先更新",
                "This version was saved by a newer Ludonimbus. Update Ludonimbus on this device first"
            ));
        }
        if target.is_incomplete() {
            bail!(incomplete_error());
        }
        let back = self.protect(local, Trigger::PreRestore)?;
        self.gate.check()?;
        let manifest = saves::manifest(self.cloud.files(), &self.line.place, &self.line.view, id)?;
        self.gate.check()?;
        // A version that shares the line through bindings needs a folder
        // for every file; one that shares another platform's line through a
        // rule gets the files its bindings name, the rest are that
        // platform's.
        let all = SnapshotMeta::of(&manifest, self.key(), None).files;
        crate::join::no_place(self.game, &all)?;
        let wanted = crate::join::held(self.game, &all);
        let only: Option<Vec<String>> = (wanted.len() != all.len()).then(|| wanted.iter().map(|f| f.path.clone()).collect());
        let mut packs: BTreeMap<String, u64> = BTreeMap::new();
        for f in manifest.files.iter().filter(|f| only.as_ref().is_none_or(|o| o.contains(&f.path))) {
            *packs.entry(f.pack.clone().unwrap_or_else(|| manifest.id.clone())).or_default() += f.size;
        }
        self.gate.set_phase(Phase::Downloading { done: 0, total: packs.values().sum() });
        let store = Watched { inner: self.cloud.files(), gate: &self.gate, packs };
        let work = paths::staging_dir().join(format!("pull-{id}"));
        let _ = std::fs::remove_dir_all(&work);
        let files = work.join("files");
        let result = saves::fetch(&store, &self.line.place, &self.line.view, &manifest, only.as_deref(), &files, &work.join("packs"))
            // Whoever waited may have stopped while it downloaded.
            .and_then(|_| if self.gate.start_applying() { Ok(()) } else { Err(GaveUp.into()) })
            .and_then(|_| snapshot::apply_dir(&files, &wanted, self.game));
        let _ = std::fs::remove_dir_all(&work);
        if let Some(missing) = result.as_ref().err().and_then(packs::missing_pack) {
            let pack = missing.pack.clone();
            note_missing(self.cloud, &self.line.place, &mut self.line.cache, Some(&mut self.line.view), id, &pack);
            self.line.remember();
        }
        result?;
        mark_synced(self.key(), id, &target.vv, &target.content_hash, &target.device_name, self.ctx.device().0)?;
        remember_base(self.key(), Some(&manifest));
        note_replaced(self.key(), back, id, &target.device_name);
        log(format!("{}: pulled {id} from {}", self.key(), target.device_name));
        Ok(Outcome::Pulled { id: id.to_string(), device: target.device_name.clone(), created_at: target.created_at })
    }

    /// [`Session::pull`] as a sync nobody chose for does it: a version that
    /// turns out to miss a pack is marked, and the decision is made again
    /// without it. That may be another version to pull (an older complete
    /// one this device doesn't have yet); anything else waits for the next
    /// sync, and the saves here stay as they are.
    pub(super) fn pull_or_hold(&mut self, local: &LocalState, id: &str, loc: &Local) -> Result<Outcome> {
        let mut id = id.to_string();
        let mut tried: Vec<String> = Vec::new();
        loop {
            match self.pull(local, &id) {
                Err(e) if packs::missing_pack(&e).is_some() && tried.len() < 3 => {
                    log(format!("{}: {id} can't be fetched ({e:#}); the saves here stay", self.key()));
                    tried.push(id.clone());
                    let seen = with_queue(&self.line.view, &self.line.place, self.key());
                    match holding(decide::decide(loc, &seen), loc, &seen) {
                        Action::Pull { id: next } if !tried.contains(&next) => id = next,
                        _ if local.is_empty() => return Ok(Outcome::NothingToSave),
                        _ => return Ok(Outcome::UpToDate),
                    }
                }
                other => return other,
            }
        }
    }

    /// After [`Session::pull`] of a version that is not the one newest
    /// version: upload the saves as they are now on top of every head, so
    /// the line has one newest version again and it is this one. Before
    /// playing (`launching`) a cloud that needs upgrading first gets the
    /// version after the game, as with any upload then.
    pub(super) fn make_current(&mut self, from: &str, trigger: Trigger, launching: bool) -> Result<Option<Outcome>> {
        if self.line.view.head().is_some_and(|h| h.id == from) {
            return Ok(None);
        }
        let local = self.local()?;
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

    pub(super) fn conflict(&mut self, local: &LocalState, candidates: &[String], local_is_candidate: bool, trigger: Trigger) -> Result<Outcome> {
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
        Ok(Outcome::Conflict { local_files: local.files.len(), local_size: local.total_size, cloud: newest })
    }
}
