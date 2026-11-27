//! Save lines (docs/plan-v2.md, section 7): the lines a game on this device
//! can follow ([`lines`]), the newest version of each of the others
//! ([`other_saves`]), taking one of those as the progress here
//! ([`take_from`]), and following another line ([`use_line`]).

use super::carry::with_carried;
use super::gate::Gate;
use super::housekeeping::{best_effort_housekeeping, tell_base};
use super::incomplete::{incomplete_error, note_missing};
use super::lock::lock_game;
use super::publish::{flush, ready_to_write};
use super::queue::Shot;
use super::run::{fresh, sync};
use super::session::Session;
use super::{base_manifest, local_of, log, note_replaced, platform, remember_base, Ctx, Outcome, Resolution};
use crate::cloud::decide::{self, Action};
use crate::cloud::saves;
use crate::cloud::v2::{self, Manifest, Origin};
use crate::cloud::{self, library, packs, Cloud};
use crate::config::{self, GameEntry};
use crate::paths;
use crate::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use crate::state;
use crate::vv::Vv;
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde::Serialize;

/// Upload what waits for the cloud to the line the game follows now: the
/// first step of following another one.
pub(crate) fn flush_queue(cloud: &Cloud, game: &GameEntry) -> Result<()> {
    let mut line = cloud.open(&game.key)?;
    flush(cloud, &mut line, Some(game), None)?;
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
                .flat_map(|d| {
                    d.installs.iter().filter(|i| i.dir == l.path).map(|i| LineMember { device: d.name.clone(), platform: i.platform.clone(), here: false })
                })
                .collect();
            if current {
                members.push(me());
            }
            members.dedup();
            LineInfo {
                id: l.id.clone(),
                name: l.name.clone().unwrap_or_default(),
                shared: l.scope != v2::SCOPE_INDEPENDENT,
                current,
                devices: names(&members),
                members,
            }
        })
        .collect();
    out.sort_by_key(|l| !l.shared);
    if !out.iter().any(|l| l.shared) {
        let current = here.line == library::MAIN_LINE && !out.iter().any(|l| l.current);
        let members = if current { vec![me()] } else { Vec::new() };
        out.insert(0, LineInfo { id: library::MAIN_LINE.to_string(), name: String::new(), shared: true, current, devices: names(&members), members });
    }
    if !out.iter().any(|l| l.current) {
        out.push(LineInfo {
            id: here.line.clone(),
            name: own_name.to_string(),
            shared: false,
            current: true,
            devices: vec![own_name.to_string()],
            members: vec![me()],
        });
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
        // A newest version that misses a pack is nothing to take.
        let view = cloud.peek(&place)?.usable();
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
    flush(cloud, &mut s.line, Some(game), None)?;
    let Some((_, place)) = cloud.other_lines(&game.key)?.into_iter().find(|(l, _)| l.id == line) else {
        bail!(crate::tr!("云端没有这条存档线", "The cloud doesn't have this save line"));
    };
    let mut view = cloud.peek(&place)?;
    let target = view.get(id).cloned().ok_or_else(|| anyhow!(crate::tr!("云端找不到这个版本", "This version wasn't found in the cloud")))?;
    if !target.unsupported().is_empty() {
        bail!(crate::tr!(
            "这个版本是更新版本的 Ludonimbus 存的，这台设备上的 Ludonimbus 需要先更新",
            "This version was saved by a newer Ludonimbus. Update Ludonimbus on this device first"
        ));
    }
    if target.is_incomplete() {
        bail!(incomplete_error());
    }
    let local = s.local()?;
    let before = state::game(&game.key).last_synced_id;
    let back = s.protect(&local, Trigger::PreRestore)?;
    let manifest = saves::manifest(cloud.files(), &place, &view, id)?;
    let all = SnapshotMeta::of(&manifest, &game.key, None).files;
    crate::join::no_place(game, &all)?;
    let files = crate::join::held(game, &all);
    let only: Option<Vec<String>> = (files.len() != all.len()).then(|| files.iter().map(|f| f.path.clone()).collect());
    let work = paths::staging_dir().join(format!("take-{id}"));
    let _ = std::fs::remove_dir_all(&work);
    let dir = work.join("files");
    let result =
        saves::fetch(cloud.files(), &place, &view, &manifest, only.as_deref(), &dir, &work.join("packs")).and_then(|_| snapshot::apply_dir(&dir, &files, game));
    let _ = std::fs::remove_dir_all(&work);
    if let Some(missing) = result.as_ref().err().and_then(packs::missing_pack) {
        let pack = missing.pack.clone();
        let mut cache = cloud::line::LineCache { snapshots: view.snapshots.iter().map(|s| (s.id.clone(), s.clone())).collect(), ..Default::default() };
        note_missing(cloud, &place, &mut cache, Some(&mut view), id, &pack);
    }
    result?;
    log(format!("{}: took {id} of the line {line} ({})", game.key, target.device_name));
    let after = s.local()?;
    let loc = local_of(&after, &state::game(&game.key));
    let Action::Push { bases, parent } = decide::settle(&loc, &s.line.view) else { return Ok(Outcome::NothingToSave) };
    let origin = Origin { game: place.game.clone(), line: place.line.clone(), snapshot: id.to_string(), how: "copy".to_string(), extra: v2::Extra::new() };
    let outcome = s.push(&after, Shot { bases, parent, trigger: Some(Trigger::Restore), origin: Some(origin), ..Shot::default() }, false)?;
    if let Outcome::Uploaded { id: made } | Outcome::Queued { id: made, .. } = &outcome {
        note_replaced(&game.key, back, made, &target.device_name);
    }
    tell_base(ctx, cloud, &game.key, before.as_deref(), &s.line.view, after.total_size);
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
    let mut local = snapshot::local_state(game)?;
    // Where the saves here come from, and what they can share content with.
    let mut carried: (Option<Origin>, Option<Manifest>) = (None, None);
    if !staying {
        let mut old = cloud.open(key)?;
        flush(cloud, &mut old, Some(game), None)?;
        // Another platform's files go along to the line it continues on.
        local = with_carried(Some(cloud), game, Some((&old.place, &old.view)), true, local)?;
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
