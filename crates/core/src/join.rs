//! Sharing a save line with a version of the game that keeps the same saves
//! in other folders: asked once (docs/plan-v2.md, section 6).
//!
//! A game whose saves are where another version's are joins that version's
//! line by itself ([`crate::cloud::library::line_to_join`]). One whose saves
//! are elsewhere (another store's version, a copy in another place) gets a
//! line of its own on the same card, and this:
//!
//! - [`suggestion`]: the line it could share, and which of its folders is
//!   which folder there ([`crate::bind::propose`]), worked out from the
//!   line's newest version and the saves on this device. Nothing changes.
//! - [`accept`]: the version reads and writes that line through the
//!   bindings from here on. The sync that follows is a first sync with the
//!   line: the same saves on both sides are simply in sync, an older version
//!   of the line here is replaced by the newest, and anything else is a
//!   conflict the user settles ([`Outcome::Conflict`]). That question is the
//!   one time the user is asked. The bindings are noted on the line, so the
//!   next device with such a version joins without asking ([`auto`]).
//! - [`decline`]: not suggested again; the version keeps its own line.
//! - [`leave`]: back to the line of its own version; that line is synced
//!   with like any line the game comes to, and the line it leaves is not
//!   suggested again.
//!
//! Files of the version outside the bound folders stay where they are and
//! are no longer uploaded; the suggestion lists them. A line that has a file
//! this version has no folder for is not joined, and a version that meets
//! one later stops before anything is written ([`NoPlace`]).
//!
//! The Xbox app's version takes part the same way through its save
//! containers: its files are the containers' files under the names the
//! other versions use, and its bindings say so ([`bind::Codec::Wgs`]).
//! Writing into the containers is experimental and only done when the user
//! asks for it (`sync`'s rule for syncs nobody chose).

use crate::bind::{self, Bind, Proposal};
use crate::cloud::saves;
use crate::cloud::{library, upgrade, Cloud, CloudGame};
use crate::config::{self, GameEntry};
use crate::snapshot::{self, Trigger};
use crate::sync::{self, Ctx, Gate, Outcome, Resolution};
use crate::{logger, state, wgs};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether a version can share a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fit {
    /// Every file of the line has a folder here.
    Ready,
    /// This device has no saves of the game yet to compare with: play once.
    NeedsSaves,
    /// The files on the two sides don't correspond: the versions keep their
    /// lines.
    NoMatch,
}

/// A save line a game on this device could share ([`suggestion`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The game record to belong to.
    pub record: String,
    /// Which files the line holds ([`crate::cloud::v2::Line::def`]).
    pub def: String,
    /// The line's newest version: who made it, on what, and when.
    pub device: String,
    pub platform: String,
    pub created_at: DateTime<Utc>,
    /// A user confirmed these bindings on another device.
    pub confirmed: bool,
    pub fit: Fit,
    pub binds: Vec<Bind>,
    /// Files both sides have.
    pub matched: usize,
    /// Files only the line has, and only this device has, in the bound
    /// folders.
    pub only_in_line: usize,
    pub only_here: usize,
    /// Files of the line with no folder here (none when `fit` is ready).
    pub uncovered: Vec<String>,
    /// Files here outside the bound folders: they stay, and are no longer
    /// uploaded once the line is shared.
    pub outside: Vec<String>,
}

/// A version of a save line has files this version of the game has no
/// folder for: nothing was written. The user confirms the folders again
/// ([`suggestion`], [`accept`]) or keeps the versions apart ([`leave`]).
#[derive(Debug)]
pub struct NoPlace {
    pub game: String,
    pub files: Vec<String>,
}

impl std::fmt::Display for NoPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let first = self.files.first().map(String::as_str).unwrap_or_default();
        write!(
            f,
            "{}",
            crate::tr!(
                "「{}」共用的存档线里有 {} 个文件在这个版本里没有对应的位置（例如 {}）。请重新确认共用，或者改成分开存",
                "The save line \"{}\" shares has {} file(s) this version has no place for (such as {}). Confirm sharing again, or keep the versions apart",
                self.game,
                self.files.len(),
                first
            )
        )
    }
}

impl std::error::Error for NoPlace {}

pub fn is_no_place(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<NoPlace>().is_some())
}

fn log(msg: impl AsRef<str>) {
    logger::log("join", msg);
}

fn def_of(game: &GameEntry) -> String {
    upgrade::line_def(&CloudGame::from(game))
}


fn suggest(s: Proposal, record: &str, def: &str, head: &crate::cloud::v2::IndexEntry, confirmed: bool, fit: Fit) -> Suggestion {
    Suggestion {
        record: record.to_string(),
        def: def.to_string(),
        device: head.device_name.clone(),
        platform: head.platform.clone(),
        created_at: head.created_at,
        confirmed,
        fit,
        binds: s.binds,
        matched: s.matched,
        only_in_line: s.only_in_line,
        only_here: s.only_here,
        uncovered: s.uncovered,
        outside: s.outside,
    }
}

/// The save line the game could share although it keeps its saves
/// elsewhere, with the folders that correspond. `None`: there is none (no
/// other version of the game in the cloud, or all were declined), or the
/// game shares one through bindings already.
///
/// The first line that fits is the answer; when none does, the first one
/// looked at, with why not. Costs the game records (one listing, when not
/// read within the last minute) and, for each line looked at, its listing
/// and the manifest of its newest version.
///
/// An Xbox app version is compared by the files in its containers, under
/// the names the game's other versions use; the bindings into its container
/// folder read and write the containers ([`bind::Codec::Wgs`]). A line that
/// holds an Xbox app version's containers as they are (one that began with
/// such a version) is not suggested: those files are one PC's containers,
/// and no other version has them.
pub fn suggestion(ctx: &Ctx, game: &GameEntry) -> Result<Option<Suggestion>> {
    if !game.bind.is_empty() {
        return Ok(None);
    }
    let cloud = ctx.cloud()?;
    let (here, containers) = snapshot::scan_named(game).unwrap_or_default();
    let mut first: Option<Suggestion> = None;
    for (record, line, place, confirmed) in cloud.lines_to_ask(&game.key)? {
        let Some(def) = line.def.clone().filter(|d| !game.declined.contains(d)) else { continue };
        let view = cloud.peek(&place)?;
        let Some(head) = view.head_entries().last().cloned() else { continue };
        let manifest = saves::manifest(cloud.files(), &place, &view, &head.id)?;
        let canon: Vec<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
        if !wgs::user_dirs(canon.iter().map(String::as_str)).is_empty() {
            continue;
        }
        // Bindings confirmed elsewhere that this build can use, with the
        // container folders of this device.
        let confirmed = confirmed.filter(|map| bind::known(map)).map(|map| bind::rebase(&map, &containers));
        let s = match &confirmed {
            Some(map) => {
                let p = bind::measure(map, &canon, &here);
                let fit = if p.uncovered.is_empty() { Fit::Ready } else { Fit::NoMatch };
                suggest(p, &record, &def, &head, true, fit)
            }
            None => {
                let mut p = bind::propose(&canon, &here);
                let read = bind::with_containers(&p.binds, &containers);
                let fit = match (here.is_empty(), p.usable(), &read) {
                    (true, _, _) => Fit::NeedsSaves,
                    (false, true, Some(_)) => Fit::Ready,
                    _ => Fit::NoMatch,
                };
                if let Some(binds) = read {
                    p.binds = binds;
                }
                suggest(p, &record, &def, &head, false, fit)
            }
        };
        if s.fit == Fit::Ready {
            return Ok(Some(s));
        }
        first.get_or_insert(s);
    }
    Ok(first)
}

/// Change the game's entry in the configuration; the entry afterwards.
fn change(key: &str, f: impl FnOnce(&mut GameEntry)) -> Result<GameEntry> {
    config::update(|c| match c.game_mut(key) {
        Some(g) => {
            f(g);
            Ok(g.clone())
        }
        None => bail!(crate::tr!("没有找到游戏 {key}", "The game {key} wasn't found")),
    })
}

/// Share the line of `s` from here on, through its bindings, and sync with
/// it. What waits for the cloud is uploaded to the line the game leaves
/// first; when that can't be done now, nothing changes. The line it leaves
/// stays as it is.
pub fn accept(ctx: &Ctx, game: &GameEntry, s: &Suggestion) -> Result<Outcome> {
    if s.fit != Fit::Ready || s.binds.is_empty() || !game.bind.is_empty() {
        bail!(crate::tr!("这条存档线不能和这个版本共用", "This save line can't be shared with this version"));
    }
    let key = game.key.as_str();
    let _lock = sync::lock_game(key, &Gate::default())?;
    let cloud = ctx.cloud()?;
    sync::flush_queue(cloud, game)?;
    let entry = change(key, |g| {
        g.record = Some(s.record.clone());
        g.bind = s.binds.clone();
        // Chosen: the device's default (a line of its own) doesn't apply.
        g.line = Some(library::MAIN_LINE.to_string());
    })?;
    sync::start_over(key)?;
    cloud.know(&entry);
    cloud.follow(key, library::MAIN_LINE);
    log(format!("{key}: shares the saves of {} from here on, through {} folder(s)", s.record, s.binds.len()));
    if !s.confirmed {
        // Should this fail, the next device with such a version asks too.
        if let Err(e) = cloud.add_bind(&s.record, &def_of(game), &s.binds) {
            log(format!("{key}: could not note the folders on the line: {e:#}"));
        }
    }
    sync::sync(ctx, &entry, Resolution::Auto, Trigger::SyncNow)
}

/// Don't suggest the line that holds the files `def` to this game again.
pub fn decline(key: &str, def: &str) -> Result<()> {
    change(key, |g| {
        if !g.declined.iter().any(|d| d == def) {
            g.declined.push(def.to_string());
        }
    })?;
    Ok(())
}

/// Stop sharing through bindings: the game goes back to the line of its
/// own version (a new one when the cloud has none), and the line it leaves
/// is not suggested again. That line stays as it is. The sync that follows
/// is a first sync with the line it goes to: saves that differ from that
/// line's newest version are a conflict the user settles.
pub fn leave(ctx: &Ctx, game: &GameEntry) -> Result<Outcome> {
    if game.bind.is_empty() {
        bail!(crate::tr!("这个版本没有和别的版本共用存档", "This version doesn't share its saves with another"));
    }
    let key = game.key.as_str();
    let _lock = sync::lock_game(key, &Gate::default())?;
    let cloud = ctx.cloud()?;
    sync::flush_queue(cloud, game)?;
    let left = cloud.shared_def(key).unwrap_or_default();
    let entry = change(key, |g| {
        g.record = None;
        g.bind.clear();
        g.line = Some(library::MAIN_LINE.to_string());
        if let Some(def) = left.filter(|d| !g.declined.contains(d)) {
            g.declined.push(def);
        }
    })?;
    sync::start_over(key)?;
    cloud.know(&entry);
    cloud.follow(key, library::MAIN_LINE);
    log(format!("{key}: keeps its saves apart again"));
    sync::sync(ctx, &entry, Resolution::Auto, Trigger::SyncNow)
}

/// The user is about to change by hand where a game that shares a line
/// through bindings keeps its saves: the folders matched for it no longer
/// say where its files are. As with [`leave`] it goes back to the line of
/// its own version, but nothing is declined (with the new location it may
/// be offered the shared line again) and nothing is synced here: the next
/// sync is a first sync with the line it goes to. What waits for the cloud
/// is uploaded to the line it leaves first; when that can't be done now,
/// nothing changes. Returns whether there was anything to forget.
pub fn forget(ctx: &Ctx, game: &GameEntry) -> Result<bool> {
    if game.bind.is_empty() {
        return Ok(false);
    }
    let key = game.key.as_str();
    let _lock = sync::lock_game(key, &Gate::default())?;
    if !state::game(key).pending.is_empty() || !sync::pending_metas(key).is_empty() {
        sync::flush_queue(ctx.cloud()?, game).map_err(|e| {
            e.context(crate::tr!(
                "还有存档没传到云端，连上云盘以后再改存档位置",
                "Some saves haven't reached the cloud yet. Change the save location once the cloud can be reached"
            ))
        })?;
    }
    change(key, |g| {
        g.record = None;
        g.bind.clear();
        g.line = Some(library::MAIN_LINE.to_string());
    })?;
    sync::start_over(key)?;
    log(format!("{key}: its save location was changed; it keeps its saves apart again"));
    Ok(true)
}

/// For a game that has not synced on this device yet: when a user confirmed
/// on another device how versions like this one share a line of the game,
/// share it too. Returns the game's entry as it is from then on.
///
/// Done before the sync reads anything, so the whole sync scans and writes
/// through the bindings. No request of its own: a game that hasn't synced
/// has its record looked for anyway, and both read the game records once.
pub(crate) fn auto(cloud: &Cloud, game: &GameEntry) -> Option<GameEntry> {
    // Nothing was settled or chosen for it yet.
    if !game.bind.is_empty() || game.record.is_some() || game.line.is_some() || !game.declined.is_empty() {
        return None;
    }
    let key = game.key.as_str();
    let st = state::game(key);
    if st.line.is_some() || st.last_synced_id.is_some() || !st.pending.is_empty() || !sync::pending_metas(key).is_empty() {
        return None;
    }
    let (record, binds) = cloud.bound_line(key).ok()??;
    // A newer Ludonimbus confirmed them: this one can't use them.
    if !bind::known(&binds) {
        return None;
    }
    let entry = change(key, |g| {
        g.record = Some(record.clone());
        g.bind = binds.clone();
    })
    .map_err(|e| log(format!("{key}: could not note the game's record: {e:#}")))
    .ok()?;
    cloud.know(&entry);
    log(format!("{key}: shares the saves of {record}, through the folders confirmed on another device"));
    Some(entry)
}

/// What [`NoPlace`] is made of: the files of a version the game has no
/// folder for.
pub(crate) fn no_place(game: &GameEntry, files: &[snapshot::FileEntry]) -> Result<()> {
    let missing = snapshot::no_place_for(game, files);
    if missing.is_empty() {
        return Ok(());
    }
    Err(anyhow!(NoPlace { game: game.name.clone(), files: missing }))
}
