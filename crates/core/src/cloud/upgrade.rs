//! Bringing a cloud written by 0.6 (layout 1) to layout 2
//! (docs/plan-v2.md, section 12). Done once per cloud, by the first device
//! that runs a build with layout 2 and wants to write.
//!
//! (Copying from one cloud to another when the user switches clouds is
//! [`super::migrate`]; this is not that.)
//!
//! **No file of 0.6 is moved, changed or deleted.** The upgrade only adds:
//! game records, pins, indexes and, last of all, `format.json`. So going
//! back is deleting what was added (section 12.4), and a device still on
//! 0.6 keeps working in its own folders.
//!
//! # What is added
//!
//! For every folder `games/<key>/` that has a `game.json` 0.6 can read or at
//! least one snapshot in `snapshots/`:
//!
//! 1. **A game record** `library/<key>.json`:
//!    - `id` and the one entry of `folders`: the key. `name`: from
//!      `game.json`, the key when there is none.
//!    - `createdAt`: when the oldest snapshot in the folder was made; none
//!      for a folder without snapshots. Every device that does the upgrade
//!      must write the same record, so nothing in it comes from this device
//!      or from the clock (`updatedAt` and `updatedBy`, which
//!      [`library::save`] sets, are the exception).
//!    - one line: `id` `main`, shared, `path` `games/<key>/saves/main`,
//!      `format` from the save rules of the game ([`line_format`]), and
//!      `also` `[games/<key>/snapshots]` ([`v2::legacy_dir`]) when that
//!      folder has snapshots. A line reads every folder it names each time
//!      it is looked at, and a game nobody uploaded saves of has nothing
//!      there. Should a device still on 0.6 upload there later, a device on
//!      layout 2 adds the folder ([`super::Cloud::adopt_old_folder`]).
//!      `def` says which files the saves are ([`line_def`]), so that a game
//!      added later under another key can tell whether it continues them.
//!    - `ids` from the game's definition ([`record_ids`]), `rule` when a
//!      cross-platform rule covers it.
//!
//!    The record is saved with [`library::save`], so one that is there
//!    already (an upgrade that broke off, another device at the same time)
//!    is merged with, not overwritten.
//! 2. **One card per game**: records that share a strong id become one
//!    ([`library::dedupe_with`]; survivor written first). Their save lines
//!    are *not* joined: each key keeps its own history, as in 0.6, under the
//!    one card. Whether two versions save the same files is not something
//!    the upgrade can know; joining lines comes with bindings and the
//!    question that goes with them (section 6).
//! 3. **Pins**: a snapshot of the old folder that is locked, or was made by
//!    hand (`manual`), gets `<id>.pin.json` in the line's new folder: `name`
//!    is its note (empty when it has none), `createdAt` and `deviceId` are
//!    the snapshot's. Skipped when the listing shows a pin for it already.
//!    From here on the pin says "kept"; the `locked` in the old file is
//!    history.
//! 4. **The index** of each line, in its new folder, covering the old
//!    folder's snapshots and the pins. A device that reads the line
//!    afterwards with nothing cached makes its two listings and one read.
//! 5. **`format.json`**, when everything else is there:
//!    `{layout: 2, minApp: "1.0.0", requires: [], migratedAt, migratedBy}`.
//!
//! A cloud with no games at all gets only step 5: that is how a new cloud
//! becomes one of layout 2.
//!
//! In what order: pins and the index are written game by game, as each
//! folder is read; the records once every folder has been read (making one
//! card per game needs them all); `format.json` last.
//!
//! # Doing it twice, or two at once
//!
//! Every step gives the same result when it is done again, by the same
//! device or by another: an upgrade that broke off is simply run again, and
//! two devices that both run it leave what one would have left. The lock
//! (`upgrade.lock.json` in the cloud's root, taken and released like the one
//! of [`crate::gamebackup::remote::Lock`], refreshed while it runs) only
//! saves the second device the work; nothing depends on it holding. A
//! device that finds its own lock (left behind by a run that broke off)
//! takes it over.
//!
//! # Requests
//!
//! | | requests |
//! |---|---|
//! | a cloud that is of layout 2 already | 1 read |
//! | per game | 1 list of `games/<key>/snapshots` and of the new folder, 1 read of `game.json`, 1 read per snapshot not cached on this device, 1 read + 1 write for its record, 1 write per pin, then 1 list of the new folder when pins were written (the index records their tags) and 1 write for the index (none for a folder without snapshots) |
//! | once | 1 read of `format.json`, 1 list of `games/`, the lock (2 reads + 1 write to take it, 1 read + 1 remove to give it back, 1 write every 10 minutes while it runs; none for a cloud without games), 1 write of `format.json` |
//!
//! Snapshots are read through the store the caller passes: with
//! [`super::cached::Cached`] around it, a device that used 0.6 has them
//! already. A run after one that broke off reads what is there already (an
//! index instead of the snapshots, the pins) instead of writing it again.

use super::library::{self, MAIN_LINE};
use super::line::{self, LineCache};
use super::store::{join, Store};
use super::v2::{self, Access, FormatFile, GameRecord, Line, Pin, PinRef, SCOPE_SHARED};
use super::CloudGame;
use crate::{adapters, games, savelib};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

/// The first version that writes layout 2.
pub const MIN_APP: &str = "1.0.0";
pub const LOCK_FILE: &str = "upgrade.lock.json";

/// A lock not refreshed for this long belongs to a device that stopped (the
/// same as a game backup's).
const LOCK_MINUTES: i64 = 30;
/// How often a running upgrade pushes the expiry of its lock forward.
const REFRESH_EVERY: Duration = Duration::from_secs(10 * 60);
/// The trigger of a backup the user made by hand.
const MANUAL: &str = "manual";
const LINK: &str = "link:";

/// Another device is upgrading the cloud right now. Nothing is wrong: what
/// this device wanted to write waits until that has finished.
#[derive(Debug)]
pub struct Busy(pub String);

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for Busy {}

pub fn is_busy(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Busy>().is_some())
}

/// How far the upgrade is, for a progress bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Name of the game being worked on.
    pub game: String,
    pub done: usize,
    pub total: usize,
}

/// What an upgrade did. A run that finishes one that broke off reports the
/// same numbers as a run that never broke off: what the earlier run wrote
/// already counts too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The cloud was of layout 2 already; nothing was done.
    pub already: bool,
    /// Game records saved (one per key).
    pub games: usize,
    /// Records that turned out to be a game another record stands for.
    pub merged: usize,
    /// Snapshots of 0.6 now reachable through a line.
    pub snapshots: usize,
    /// Pins for locked snapshots and manual backups.
    pub pins: usize,
}

/// The ids a game record gets from a game as 0.6 defines it.
///
/// From what `savelib::cloud_ids` works out for it:
///
/// | 0.6 | record |
/// |---|---|
/// | `nx:…`, `ludusavi:…`, `xbox:…` (and any other kind a development build wrote) | the same |
/// | `fmt:<rule>` (a cross-platform rule) | `rule:<rule>` |
/// | `name:<scope>:<key>` | dropped; [`library::name_id`] of the game's name instead (a weak id: a suggestion, never a merge) |
///
/// and from the definition itself:
///
/// - a built-in rule for one game (`adapter` that is not an emulator's):
///   `adapter:<adapter key>`. Two keys with the same such rule are the same
///   game.
/// - the link a development build let the user set (`game`): `link:<value>`
///   for entries linked together (a value that is `link:…` already, as that
///   build wrote them, is taken as it is); an entry the user kept apart
///   (`solo:…`) gets no strong id at all, only its name, so nothing merges
///   it.
///
/// Each id once, whatever its letter case.
pub fn record_ids(game: &CloudGame) -> Vec<String> {
    let mut out = Vec::new();
    for id in savelib::cloud_ids(game) {
        if let Some(rule) = id.strip_prefix("fmt:") {
            add_id(&mut out, format!("rule:{rule}"));
        } else if id.starts_with(library::NAME_PREFIX) {
            // The scope in it was 0.6's way of keeping platforms apart; a
            // record's name id is only the name.
            if let Some(name) = library::name_id(&game.name) {
                add_id(&mut out, name);
            }
        } else {
            add_id(&mut out, id);
        }
    }
    if let Some(rule) = game.adapter.as_deref().and_then(adapters::find).filter(|a| !a.is_emulator()) {
        add_id(&mut out, format!("adapter:{}", rule.key));
    }
    match game.game.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        Some(link) if link.starts_with(savelib::SOLO) => {
            // Kept apart by the user: the name may suggest, nothing merges.
            out.retain(|id| !library::is_strong(id));
        }
        Some(link) if link.starts_with(LINK) => add_id(&mut out, link.to_string()),
        Some(link) => add_id(&mut out, format!("{LINK}{link}")),
        None => {}
    }
    out
}

fn add_id(ids: &mut Vec<String>, id: String) {
    if !ids.iter().any(|have| have.to_lowercase() == id.to_lowercase()) {
        ids.push(id);
    }
}

/// The cross-platform rule that covers a game of 0.6, if one does.
fn rule_of(game: &CloudGame) -> Option<String> {
    savelib::cloud_ids(game).iter().find_map(|id| id.strip_prefix("fmt:").map(str::to_string))
}

/// The format of the line a game of 0.6 gets, from the scope its saves have
/// there (`savelib::save_scope`): `pc` → `pc`; `switch` → `nx`; anything
/// else (a built-in rule, an emulator) → `adapter:<scope>`.
pub fn line_format(game: &CloudGame) -> String {
    format_of_scope(&savelib::save_scope(&savelib::entry_of(game)))
}

/// The format of a line of versions with the save scope `scope`
/// ([`line_format`]).
pub fn format_of_scope(scope: &str) -> String {
    match scope {
        "pc" => "pc".to_string(),
        "switch" => "nx".to_string(),
        scope => format!("adapter:{scope}"),
    }
}

/// The save scope of the versions a line of format `format` is made by: the
/// other way round from [`format_of_scope`]. `None` for a format no scope
/// gives (`rule:…`).
pub fn scope_of_format(format: &str) -> Option<&str> {
    match format {
        "pc" => Some("pc"),
        "nx" => Some("switch"),
        f => f.strip_prefix("adapter:").filter(|s| !s.is_empty()),
    }
}

/// Which files a game's saves are, for [`Line::def`]: games with the same
/// answer (and the same [`line_format`]) read each other's saves as they
/// are, so they can share one history without anything being converted.
///
/// - A Switch game: its title id (`nx:0100E65002BB8000`). The console and
///   every emulator keep the same files for a title.
/// - Anything else: a fingerprint of where the saves are looked for. That
///   is the game's rule, its own save patterns (in any order, either kind of
///   slash, any letter case), its title in Ludusavi's database, and whether
///   it is the Xbox app's version (which keeps its saves in containers of
///   its own).
///
/// Left out on purpose: what a named folder stands for on a device
/// (`roots`: the name is in the patterns, the place is the device's own),
/// the files a game leaves out, and everything about how it is started.
///
/// Two versions of a game that keep the same saves in different places get
/// different answers and stay on lines of their own. That is the safe side:
/// a shared history would put one version's files where the other never
/// looks.
pub fn line_def(game: &CloudGame) -> String {
    if line_format(game) == "nx" {
        if let Some(title) = games::switch_title(&game.saves) {
            return format!("nx:{title:016X}");
        }
    }
    let tidy = |s: &str| s.trim().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let mut saves: Vec<String> = game.saves.iter().map(|p| tidy(p)).filter(|p| !p.is_empty()).collect();
    saves.sort();
    saves.dedup();
    let adapter = game.adapter.as_deref().map(tidy).unwrap_or_default();
    let ludusavi = game.ludusavi_title.as_deref().map(tidy).unwrap_or_default();
    let xbox = game.ids.iter().any(|id| id.to_lowercase().starts_with("xbox:"));
    // A character no pattern or title contains keeps the parts apart.
    let what = format!("{adapter}\0{}\0{ludusavi}\0{xbox}", saves.join("\n"));
    blake3::hash(what.as_bytes()).to_hex()[..16].to_string()
}

/// The record the upgrade writes for one key (before [`library::save`] and
/// before records are merged). `game` is `None` for a folder without a
/// `game.json`; `oldest` is when the oldest snapshot in its old folder was
/// made; `old` says that folder has snapshots, and the line reads it.
///
/// A folder without a `game.json` gets no ids and a line of format `pc`,
/// what 0.6 takes a game with no rule and no Switch saves for; which files
/// its saves are is then not known ([`Line::def`]).
pub fn record_for(key: &str, game: Option<&CloudGame>, oldest: Option<DateTime<Utc>>, old: bool) -> GameRecord {
    GameRecord {
        id: key.to_string(),
        name: name_of(key, game),
        ids: game.map(record_ids).unwrap_or_default(),
        rule: game.and_then(rule_of),
        folders: vec![key.to_string()],
        lines: vec![Line {
            id: MAIN_LINE.to_string(),
            format: game.map(line_format).unwrap_or_else(|| "pc".to_string()),
            scope: SCOPE_SHARED.to_string(),
            path: v2::line_dir(key, MAIN_LINE),
            also: if old { vec![v2::legacy_dir(key)] } else { Vec::new() },
            def: game.map(line_def),
            ..Default::default()
        }],
        created_at: oldest,
        ..Default::default()
    }
}

fn name_of(key: &str, game: Option<&CloudGame>) -> String {
    game.map(|g| g.name.trim()).filter(|n| !n.is_empty()).unwrap_or(key).to_string()
}

/// Bring the cloud behind `store` to layout 2.
///
/// - Layout 2 already: `Report { already: true, .. }` after one read.
/// - Written by a newer Ludonimbus ([`Access::ReadOnly`]): an error that
///   tells the user to update; nothing is written.
/// - A `format.json` that can't be understood: an error; nothing is
///   written.
/// - Another device holds the lock and it hasn't expired: an error that
///   says which and since when ([`Busy`]); nothing is written. An expired
///   lock, or one this device left behind, is taken over.
/// - Otherwise the steps of the module documentation, `format.json` last.
///   An error on the way (no connection) leaves a cloud that is still of
///   layout 1 and that the next run finishes.
///
/// `now` is the caller's clock (`migratedAt`, the lock). `progress` is
/// called before each game, once its `game.json` has given its name.
pub fn run(store: &dyn Store, device_id: &str, device_name: &str, now: DateTime<Utc>, progress: &mut dyn FnMut(&Progress)) -> Result<Report> {
    match v2::access(v2::read_format(store)?.as_ref()) {
        Access::ReadWrite => return Ok(Report { already: true, ..Default::default() }),
        Access::ReadOnly { min_app, .. } if min_app.trim().is_empty() => bail!(crate::tr!(
            "这个云端是更新版本的 Ludonimbus 写的，请先更新 Ludonimbus",
            "This cloud was written by a newer Ludonimbus. Update Ludonimbus first"
        )),
        Access::ReadOnly { min_app, .. } => bail!(crate::tr!(
            "这个云端要用 Ludonimbus {} 或更新的版本，请先更新",
            "This cloud needs Ludonimbus {} or later. Update Ludonimbus first",
            min_app.trim()
        )),
        Access::Legacy => {}
    }

    let mut keys: Vec<String> = store.list(GAMES)?.into_iter().filter(|e| e.is_dir).map(|e| e.name).collect();
    keys.sort();
    let mut report = Report::default();
    if keys.is_empty() {
        write_format(store, device_id, now)?;
        return Ok(report);
    }

    let mut lock = Lock::take(store, device_id, device_name, now)?;
    let mut found = Vec::new();
    for (done, key) in keys.iter().enumerate() {
        lock.keep()?;
        let game = read_game(store, key)?;
        progress(&Progress { game: name_of(key, game.as_ref()), done, total: keys.len() });
        if let Some(f) = upgrade_line(store, key, game, device_id, &mut report)? {
            found.push(f);
        }
    }

    // One card per game. Every device that does this arrives at the same
    // records, so writing them again changes nothing.
    let records: Vec<GameRecord> = found.iter().map(|f| record_for(&f.key, f.game.as_ref(), f.oldest, f.old)).collect();
    let duplicates = library::dedupe_with(&records, &|_, _| false);
    let mut order: Vec<&GameRecord> = Vec::with_capacity(records.len());
    let mut grouped: HashSet<&str> = HashSet::new();
    for d in &duplicates {
        report.merged += d.losers.len();
        // The survivor first: a record pointing at one that hasn't taken
        // over its line yet would hide it.
        for r in std::iter::once(&d.survivor).chain(&d.losers) {
            grouped.insert(r.id.as_str());
            order.push(r);
        }
    }
    order.extend(records.iter().filter(|r| !grouped.contains(r.id.as_str())));
    for record in order {
        lock.keep()?;
        library::save(store, record, device_id, now)?;
    }
    report.games = records.len();

    write_format(store, device_id, now)?;
    // The upgrade is done whether or not this goes through: a lock left
    // behind expires, and nobody upgrades a cloud of layout 2.
    let _ = lock.release();
    Ok(report)
}

const GAMES: &str = "games";

/// What reading one folder of 0.6 found, for its record.
struct Found {
    key: String,
    game: Option<CloudGame>,
    oldest: Option<DateTime<Utc>>,
    /// The old folder has snapshots (some may be unreadable).
    old: bool,
}

/// `games/<key>/game.json`, when 0.6 can read it.
fn read_game(store: &dyn Store, key: &str) -> Result<Option<CloudGame>> {
    Ok(store.read(&join(&[GAMES, key, "game.json"]))?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// Read the old folder of `key` and give its line the pins and the index it
/// needs. `None` for a folder that is not a game (no `game.json`, no
/// snapshots).
fn upgrade_line(store: &dyn Store, key: &str, game: Option<CloudGame>, device_id: &str, report: &mut Report) -> Result<Option<Found>> {
    let dir = v2::line_dir(key, MAIN_LINE);
    let old = v2::legacy_dir(key);
    let mut cache = LineCache::default();
    let view = line::load_all(store, &[dir.as_str(), old.as_str()], &mut cache)?;
    let snapshots: Vec<_> = view.snapshots.iter().zip(&view.places).filter(|(_, place)| **place == 1).map(|(s, _)| s).collect();
    if game.is_none() && snapshots.is_empty() && view.unreadable.is_empty() {
        return Ok(None);
    }
    report.snapshots += snapshots.len();

    // Kept by the user in 0.6: from here on the pin says so.
    let mut written: Vec<(String, String)> = Vec::new();
    for s in snapshots.iter().filter(|s| s.was_locked() || s.trigger == MANUAL) {
        report.pins += 1;
        if s.pin.is_some() {
            continue;
        }
        let pin =
            Pin { name: s.old_note().unwrap_or_default().to_string(), created_at: Some(s.created_at), device_id: s.device_id.clone(), ..Default::default() };
        line::write_pin(store, &dir, &s.id, &pin)?;
        written.push((s.id.clone(), pin.name));
    }
    if !written.is_empty() {
        // The index remembers a pin together with the tag its file has in
        // the listing; without the tag a reader would read the pin again.
        let tags: BTreeMap<String, String> = store
            .list(&dir)?
            .into_iter()
            .filter(|e| !e.is_dir)
            .filter_map(|e| {
                let id = e.name.strip_suffix(v2::PIN_SUFFIX)?.to_string();
                Some((id, e.tag))
            })
            .collect();
        for (id, name) in &written {
            if let (Some(entry), Some(tag)) = (cache.snapshots.get_mut(id), tags.get(id)) {
                entry.pin = Some(PinRef { name: name.clone(), tag: tag.clone() });
            }
        }
    }
    if view.needs_index || !written.is_empty() {
        line::write_index(store, &dir, &cache, device_id)?;
    }

    let oldest = snapshots.iter().map(|s| s.created_at).min();
    let old = !snapshots.is_empty() || !view.unreadable.is_empty();
    Ok(Some(Found { key: key.to_string(), game, oldest, old }))
}

fn write_format(store: &dyn Store, device_id: &str, now: DateTime<Utc>) -> Result<()> {
    let format = FormatFile {
        layout: v2::LAYOUT,
        min_app: MIN_APP.to_string(),
        requires: Vec::new(),
        migrated_at: Some(now),
        migrated_by: Some(device_id.to_string()),
        extra: v2::Extra::new(),
    };
    store.write(v2::FORMAT_FILE, &serde_json::to_vec_pretty(&format)?)
}

// ----------------------------------------------------------------------- lock

/// `upgrade.lock.json`: the device upgrading the cloud. The same fields as
/// the lock of a game backup ([`crate::gamebackup::remote::LockInfo`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockInfo {
    device_id: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    kind: String,
    since: DateTime<Utc>,
    expires: DateTime<Utc>,
    /// Tells two takes of the same device apart.
    #[serde(default)]
    token: String,
}

/// The lock while this device holds it. Given back when dropped.
struct Lock<'a> {
    store: &'a dyn Store,
    info: LockInfo,
    /// The caller's clock when it was taken, and when that was here: the
    /// clock goes on from there.
    now: DateTime<Utc>,
    taken: Instant,
    written: Instant,
    released: bool,
}

/// A file that can't be understood is no lock.
fn read_lock(store: &dyn Store) -> Result<Option<LockInfo>> {
    Ok(store.read(LOCK_FILE)?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

impl<'a> Lock<'a> {
    fn take(store: &'a dyn Store, device_id: &str, device_name: &str, now: DateTime<Utc>) -> Result<Lock<'a>> {
        if let Some(l) = read_lock(store)?.filter(|l| l.expires > now && l.device_id != device_id) {
            return Err(anyhow!(Busy(crate::tr!(
                "「{}」正在升级云端的存储格式（从 {} 开始），请等它完成再试",
                "\"{}\" is upgrading the cloud's storage layout (since {}). Try again when it has finished",
                l.device_name,
                l.since.with_timezone(&chrono::Local).format("%H:%M")
            ))));
        }
        let info = LockInfo {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            kind: "upgrade".to_string(),
            since: now,
            expires: now + chrono::Duration::minutes(LOCK_MINUTES),
            token: ulid::Ulid::new().to_string(),
        };
        store.write(LOCK_FILE, &serde_json::to_vec_pretty(&info)?)?;
        // Two devices writing at once: the last write wins, the other backs
        // off.
        if read_lock(store)?.map(|l| l.token) != Some(info.token.clone()) {
            return Err(anyhow!(Busy(crate::tr!("另一台设备刚开始升级云端，请稍后再试", "Another device just started upgrading the cloud. Try again later"))));
        }
        let at = Instant::now();
        Ok(Lock { store, info, now, taken: at, written: at, released: false })
    }

    /// Push the expiry forward when it is time (a long upgrade on a slow
    /// cloud).
    fn keep(&mut self) -> Result<()> {
        if self.written.elapsed() < REFRESH_EVERY {
            return Ok(());
        }
        let clock = self.now + chrono::Duration::from_std(self.taken.elapsed()).unwrap_or_default();
        self.info.expires = clock + chrono::Duration::minutes(LOCK_MINUTES);
        self.store.write(LOCK_FILE, &serde_json::to_vec_pretty(&self.info)?)?;
        self.written = Instant::now();
        Ok(())
    }

    fn release(mut self) -> Result<()> {
        self.released = true;
        self.remove()
    }

    fn remove(&self) -> Result<()> {
        // Only our own: one that expired may have been taken over.
        match read_lock(self.store)? {
            Some(l) if l.token != self.info.token => Ok(()),
            _ => self.store.remove(LOCK_FILE),
        }
    }
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::BTreeMap;

    fn game(key: &str, name: &str) -> CloudGame {
        CloudGame {
            key: key.into(),
            name: name.into(),
            adapter: None,
            saves: Vec::new(),
            exclude: Vec::new(),
            wait_for: Vec::new(),
            roots: BTreeMap::new(),
            ludusavi_title: None,
            source: None,
            ids: Vec::new(),
            game: None,
        }
    }

    fn with_adapter(key: &str, name: &str, adapter: &str, saves: &[&str]) -> CloudGame {
        CloudGame { adapter: Some(adapter.into()), saves: saves.iter().map(|s| s.to_string()).collect(), ..game(key, name) }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn store_ids_are_kept_and_a_name_loses_its_scope() {
        // A PC game Ludusavi knows, with the Xbox package its device read.
        let hades =
            CloudGame { ludusavi_title: Some("Hades".into()), ids: strings(&["xbox:supergiantgames.hades", "name:pc:hades"]), ..game("hades", "Hades™") };
        assert_eq!(savelib::cloud_ids(&hades), strings(&["xbox:supergiantgames.hades", "ludusavi:hades", "name:pc:hades"]));
        assert_eq!(record_ids(&hades), strings(&["xbox:supergiantgames.hades", "ludusavi:hades", "name:hades"]));
        assert!(!library::is_strong("name:hades"));

        // A Switch game: its title id.
        let mh = CloudGame { saves: strings(&["{nxsave-0100B04011742000}/**"]), ..game("mh", "Monster Hunter Rise") };
        assert_eq!(record_ids(&mh), strings(&["nx:0100B04011742000", "name:monsterhunterrise"]));

        // 0.6 gives a short name no id ("Ys" matches too much); nor does the
        // record.
        assert_eq!(record_ids(&game("ys", "Ys")), Vec::<String>::new());
        // The same id written twice in two letter cases is one.
        let twice = CloudGame { ids: strings(&["nx:0100B04011742000"]), ..mh.clone() };
        assert_eq!(record_ids(&twice), strings(&["nx:0100B04011742000", "name:monsterhunterrise"]));
    }

    #[test]
    fn a_cross_platform_rule_becomes_a_rule_id_and_the_records_rule() {
        let steam = CloudGame { ludusavi_title: Some("Stardew Valley".into()), ..game("sdv", "Stardew Valley") };
        assert_eq!(record_ids(&steam), strings(&["ludusavi:stardew valley", "name:stardewvalley", "rule:stardew-valley"]));
        let switch = CloudGame { saves: strings(&["{nxsave-0100e65002bb8000}/**"]), ..game("nx-sdv", "星露谷物语") };
        assert_eq!(record_ids(&switch), strings(&["nx:0100E65002BB8000", "name:星露谷物语", "rule:stardew-valley"]));
        // The two are one game through the rule, not through a name.
        assert_eq!(record_for("sdv", Some(&steam), None, true).rule.as_deref(), Some("stardew-valley"));
        assert_eq!(record_for("nx-sdv", Some(&switch), None, true).rule.as_deref(), Some("stardew-valley"));
        assert_eq!(record_for("hades", Some(&game("hades", "Hades")), None, true).rule, None);
    }

    #[test]
    fn a_rule_for_one_game_is_an_id_and_an_emulator_is_not() {
        let fe = with_adapter("fe", "火焰纹章：万缕千丝", "fe-fortunes-weave", &[]);
        assert_eq!(record_ids(&fe), strings(&["name:火焰纹章万缕千丝", "adapter:fe-fortunes-weave"]));
        let fe2 = with_adapter("fe-2", "Fire Emblem: Fortune's Weave", "fe-fortunes-weave", &[]);
        assert!(record_ids(&fe2).contains(&"adapter:fe-fortunes-weave".to_string()));

        // Every game of an emulator has the emulator's rule: it says nothing
        // about which game it is.
        let botw = with_adapter("botw", "Breath of the Wild", "cemu", &["{mlc}/usr/save/00050000/101c9300/**"]);
        assert_eq!(record_ids(&botw), strings(&["name:breathofthewild"]));
        let ryujinx = with_adapter("botw-nx", "Breath of the Wild", "ryujinx", &["{nxsave-01007ef00011e000}/**"]);
        assert_eq!(record_ids(&ryujinx), strings(&["nx:01007EF00011E000", "name:breathofthewild"]));
        let rom = with_adapter("oot-rom", "Ocarina of Time", "retroarch", &[]);
        assert_eq!(record_ids(&rom), strings(&["name:ocarinaoftime"]));
        // A rule this build doesn't know may be an emulator's: no id.
        let unknown = with_adapter("x", "Some Game", "rule-from-a-later-version", &[]);
        assert_eq!(record_ids(&unknown), strings(&["name:somegame"]));
    }

    #[test]
    fn links_join_and_solo_keeps_apart() {
        let linked = CloudGame { game: Some("link:oot-longshot".into()), ..with_adapter("oot", "时之笛", "zelda-oot-longshot", &[]) };
        assert_eq!(record_ids(&linked), strings(&["name:时之笛", "adapter:zelda-oot-longshot", "link:oot-longshot"]));
        let other = CloudGame { game: Some("link:oot-longshot".into()), ..game("oot-n64", "The Legend of Zelda: Ocarina of Time") };
        assert!(record_ids(&other).contains(&"link:oot-longshot".to_string()));
        // A value without the prefix gets it.
        let plain = CloudGame { game: Some("family".into()), ..game("g", "Game") };
        assert_eq!(record_ids(&plain), strings(&["name:game", "link:family"]));

        // Kept apart: only the name, which never merges.
        let solo = CloudGame {
            ludusavi_title: Some("Hades".into()),
            ids: strings(&["xbox:supergiantgames.hades"]),
            game: Some("solo:hades-xbox".into()),
            ..game("hades-xbox", "Hades")
        };
        assert_eq!(record_ids(&solo), strings(&["name:hades"]));
        let solo_rule = CloudGame { game: Some("solo:fe".into()), ..with_adapter("fe", "Fortune's Weave", "fe-fortunes-weave", &[]) };
        assert!(record_ids(&solo_rule).iter().all(|id| !library::is_strong(id)));
    }

    #[test]
    fn line_formats() {
        assert_eq!(line_format(&game("hades", "Hades")), "pc");
        assert_eq!(line_format(&CloudGame { saves: strings(&["{install}/save/**"]), ..game("t", "T") }), "pc");
        assert_eq!(line_format(&CloudGame { saves: strings(&["{nxsave-0100B04011742000}/**"]), ..game("mh", "MH") }), "nx");
        assert_eq!(line_format(&with_adapter("mh", "MH", "ryujinx", &["{nxsave-0100B04011742000}/**"])), "nx");
        assert_eq!(line_format(&with_adapter("botw", "BotW", "cemu", &[])), "adapter:wiiu");
        assert_eq!(line_format(&with_adapter("oot", "OoT", "retroarch", &[])), "adapter:retroarch");
        assert_eq!(line_format(&with_adapter("fe", "FE", "fe-fortunes-weave", &[])), "adapter:fe-fortunes-weave");
    }

    #[test]
    fn games_that_look_in_the_same_place_keep_the_same_saves() {
        let pc = |saves: &[&str]| CloudGame { saves: strings(saves), ..game("g", "Game") };
        let def = line_def(&pc(&["{install}/save/**", "{appdata}/Game/*.sav"]));
        assert_eq!(def.len(), 16);
        // Order, slashes, letter case, spaces and a slash at the end are not
        // part of where a game looks; neither are its key and its name.
        assert_eq!(line_def(&pc(&[" {AppData}\\Game\\*.sav ", "{install}/save/**"])), def);
        assert_eq!(line_def(&CloudGame { key: "other".into(), name: "Other".into(), ..pc(&["{install}/save/**", "{appdata}/Game/*.sav"]) }), def);
        // What a named folder stands for is the device's own business, and
        // so is what a game leaves out.
        let mut roots = BTreeMap::new();
        roots.insert("data".to_string(), "{drive-d}/Games/Data".to_string());
        let named = CloudGame { saves: strings(&["{data}/save/**"]), ..game("g", "Game") };
        assert_eq!(line_def(&CloudGame { roots, exclude: strings(&["*.log"]), ..named.clone() }), line_def(&named));

        // Another place, another rule, another title, the Xbox app's version.
        assert_ne!(line_def(&pc(&["{install}/save/**"])), def);
        assert_ne!(line_def(&pc(&[])), def);
        let ludusavi = |title: &str| CloudGame { ludusavi_title: Some(title.into()), ..game("g", "Game") };
        assert_eq!(line_def(&ludusavi("Hades")), line_def(&ludusavi(" hades ")));
        assert_ne!(line_def(&ludusavi("Hades")), line_def(&ludusavi("Hades II")));
        assert_ne!(line_def(&ludusavi("Hades")), line_def(&pc(&[])));
        let xbox = CloudGame { ids: strings(&["xbox:supergiantgames.hades"]), ..ludusavi("Hades") };
        assert_ne!(line_def(&xbox), line_def(&ludusavi("Hades")));
        let fe = with_adapter("fe", "FE", "fe-fortunes-weave", &[]);
        assert_eq!(line_def(&fe), line_def(&with_adapter("fe-2", "Fire Emblem", "FE-Fortunes-Weave", &[])));
        assert_ne!(line_def(&fe), line_def(&with_adapter("oot", "OoT", "zelda-oot-longshot", &[])));

        // A Switch title is the same saves on the console and in every
        // emulator, whatever else its rule says.
        let console = CloudGame { saves: strings(&["{nxsave-0100b04011742000}/**"]), ..game("mh", "MH") };
        let ryujinx = with_adapter("mh-emu", "Monster Hunter", "ryujinx", &["{nxsave-0100B04011742000}/**", "{nxdevice-0100B04011742000}/**"]);
        assert_eq!(line_def(&console), "nx:0100B04011742000");
        assert_eq!(line_def(&ryujinx), line_def(&console));
        assert_ne!(line_def(&CloudGame { saves: strings(&["{nxsave-01007ef00011e000}/**"]), ..game("z", "Z") }), line_def(&console));
    }

    #[test]
    fn the_record_of_one_key() {
        let oldest = Utc.timestamp_opt(1_780_000_000, 0).unwrap();
        let fe = with_adapter("fe-fortunes-weave", " 火焰纹章：万缕千丝 ", "fe-fortunes-weave", &[]);
        let r = record_for("fe-fortunes-weave", Some(&fe), Some(oldest), true);
        let expected = GameRecord {
            id: "fe-fortunes-weave".into(),
            name: "火焰纹章：万缕千丝".into(),
            ids: strings(&["name:火焰纹章万缕千丝", "adapter:fe-fortunes-weave"]),
            folders: strings(&["fe-fortunes-weave"]),
            lines: vec![Line {
                id: "main".into(),
                format: "adapter:fe-fortunes-weave".into(),
                scope: "shared".into(),
                path: "games/fe-fortunes-weave/saves/main".into(),
                also: strings(&["games/fe-fortunes-weave/snapshots"]),
                def: Some(line_def(&fe)),
                ..Default::default()
            }],
            created_at: Some(oldest),
            ..Default::default()
        };
        assert_eq!(r, expected);
        // Nothing from this device or the clock: `library::save` adds that.
        assert_eq!((r.rev, r.updated_at, r.updated_by.as_str(), r.merged_into.clone()), (0, None, "", None));

        // No `game.json`: the key for a name, no ids, a `pc` line.
        let bare = record_for("old-game", None, None, true);
        assert_eq!(bare.name, "old-game");
        assert!(bare.ids.is_empty() && bare.rule.is_none() && bare.created_at.is_none());
        assert_eq!(bare.lines[0].format, "pc");
        assert_eq!(bare.lines[0].def, None);
        assert_eq!(bare.lines[0].also, strings(&["games/old-game/snapshots"]));
        // A name that is only spaces is none.
        assert_eq!(record_for("k", Some(&game("k", "  ")), None, true).name, "k");
        // Nothing in the old folder: the line doesn't read it.
        assert!(record_for("new-game", None, None, false).lines[0].also.is_empty());
    }
}
