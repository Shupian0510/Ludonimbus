//! The connected cloud as the rest of the program uses it.
//!
//! [`Cloud`] is a [`Store`] with what this device remembers about it: which
//! layout it has, where each game's save line is, and what each line held
//! the last time it was read. That memory is what makes a check before
//! playing one request (docs/plan-v2.md, section 5.6).
//!
//! Games are still named by the key they have on this device and had in the
//! cloud of 0.6. A key leads to a save line through the game's record:
//! record `<key>`, line `main`, wherever those are now
//! ([`library::resolve_line`]). On a cloud that 0.6 wrote and nobody has
//! upgraded yet, the line is read the way the upgrade will leave it
//! ([`Place::of_key`]), so reading never waits for the upgrade; the first
//! write does it ([`Cloud::writable`]).
//!
//! `main` is the line a game's devices share. A game on this device can
//! follow another line of its record instead ([`wanted_line`]): one of the
//! device's own, or one another device keeps (docs/plan-v2.md, section 7).
//! Everything here that takes a key works on the line that key follows.
//!
//! The functions with the names of 0.6 ([`Cloud::list_snapshots`],
//! [`Cloud::get_meta`], [`Cloud::download`]) give snapshots in the shape the
//! app has always shown them in, whichever layout they are stored in.

use super::cached::Cached;
use super::library::{self, Library, LibraryCache};
use super::line::{LineCache, LineView};
use super::saves::{self, Place};
use super::store::{join, Store};
use super::v2::{self, Access};
use super::{upgrade, AccountInfo, CloudGame, DeviceInfo};
use crate::config::{AppConfig, GameEntry};
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use crate::state::{self, CloudSeen, GameState, LineRef};
use crate::{logger, paths, util};
use anyhow::{anyhow, bail, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How long the layout this device saw is taken for granted. A cloud never
/// goes back to an older layout, so this only delays noticing that a newer
/// Ludonimbus started using a feature this build doesn't know.
const LAYOUT_FRESH_HOURS: i64 = 12;

/// How long a remembered save line is used for writing before the game's
/// record is looked at again (a line can gain folders to read when games
/// are merged).
const LINE_FRESH_HOURS: i64 = 24;

/// How long the game records one [`Cloud`] read are used again: looking at
/// many games in a row lists them once.
const LIBRARY_FRESH: std::time::Duration = std::time::Duration::from_secs(60);

pub struct Cloud {
    store: Cached<Box<dyn Store>>,
    device_id: String,
    device_name: String,
    library: Mutex<Option<(Instant, Arc<Library>)>>,
    /// The save line each game of this device follows, by key. A game that
    /// isn't here follows the shared one.
    lines: Mutex<HashMap<String, String>>,
}

/// A save line that has just been read.
pub struct OpenLine {
    /// The game's key on this device.
    pub key: String,
    pub place: Place,
    pub view: LineView,
    /// What this device remembers of the line; kept on disk between runs.
    pub cache: LineCache,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct KeptLine {
    /// The folders the cache is about: a line read from other folders
    /// starts again.
    dirs: Vec<String>,
    cache: LineCache,
}

fn lines_dir() -> PathBuf {
    paths::data_dir().join("lines")
}

fn line_file(key: &str) -> PathBuf {
    lines_dir().join(format!("{key}.json"))
}

/// Forget everything remembered about the cloud (another one was connected).
pub fn forget() {
    let _ = std::fs::remove_dir_all(lines_dir());
    let _ = state::update(|s| {
        s.cloud = CloudSeen::default();
        for g in s.games.values_mut() {
            g.line = None;
            g.cleaned_at = None;
        }
    });
}

fn place_of(line: &LineRef) -> Place {
    Place { game: line.game.clone(), line: line.line.clone(), folders: line.folders.clone(), dirs: line.dirs.clone() }
}

/// Whether what is remembered is about the line `want`.
fn is_for(known: &LineRef, want: &str) -> bool {
    match known.asked.as_str() {
        "" => want == library::MAIN_LINE,
        asked => asked == want,
    }
}

fn remember_place(key: &str, want: &str, place: &Place) {
    let line = LineRef {
        game: place.game.clone(),
        line: place.line.clone(),
        asked: if want == library::MAIN_LINE { String::new() } else { want.to_string() },
        folders: place.folders.clone(),
        dirs: place.dirs.clone(),
        checked_at: Some(Utc::now()),
    };
    let _ = state::update_game(key, |g| g.line = Some(line));
}

/// The id of the save line a device keeps to itself for the game it calls
/// `key`: `solo-` and four characters. It follows from the two, so the
/// device needs no look at the cloud to know it, comes back to the same
/// line when it leaves it and returns, and two devices don't arrive at the
/// same one.
pub fn own_line_id(device_id: &str, key: &str) -> String {
    const LETTERS: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let hash = blake3::hash(format!("{device_id}\0{key}").as_bytes());
    let bytes = hash.as_bytes();
    let n = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let tail: String = (0..4).map(|i| LETTERS[((n >> (i * 5)) & 31) as usize] as char).collect();
    format!("solo-{tail}")
}

/// The save line a game on this device follows: the one chosen for it
/// ([`GameEntry::line`]). With nothing chosen it is the shared line, unless
/// the device keeps its games to itself ([`AppConfig::independent_saves`])
/// and the game has not synced here yet: then a line of the device's own.
/// `st` is what the device remembers of the game.
pub fn wanted_line(cfg: &AppConfig, game: &GameEntry, st: Option<&GameState>) -> String {
    if let Some(line) = game.line.as_ref().filter(|l| !l.is_empty()) {
        return line.clone();
    }
    let synced = st.is_some_and(|st| st.last_synced_id.is_some() || !st.pending.is_empty());
    if cfg.independent_saves && !synced {
        own_line_id(&cfg.device.id, &game.key)
    } else {
        library::MAIN_LINE.to_string()
    }
}

/// Line `want` of the game `key`, wherever it is now. A line the game had
/// before it became part of another card is found by its folder; one added
/// since is on the card.
fn line_of<'a>(records: &'a [v2::GameRecord], key: &str, want: &str) -> Option<(&'a v2::GameRecord, &'a v2::Line)> {
    library::resolve_line(records, key, want).or_else(|| {
        let record = library::resolve(records, key)?;
        record.lines.iter().find(|l| l.id == want).map(|l| (record, l))
    })
}

/// What the cloud has of a line this device looked for.
enum Seen {
    /// The line, in the game's record or where 0.6 keeps it.
    Line,
    /// The game has no record: only a device on 0.6 can have left
    /// something, under the key.
    NoRecord,
    /// A line no record has yet (one of this device's own): nothing.
    NewLine,
}

impl OpenLine {
    /// Keep the cache for the next run.
    pub fn remember(&self) {
        let kept = KeptLine { dirs: self.place.dirs.clone(), cache: self.cache.clone() };
        let _ = std::fs::create_dir_all(lines_dir());
        if let Ok(bytes) = serde_json::to_vec(&kept) {
            let _ = util::atomic_write(&line_file(&self.key), &bytes);
        }
    }

    /// The newest version: the last head when the line has forked.
    pub fn newest(&self) -> Option<&v2::IndexEntry> {
        self.view.head_entries().last()
    }
}

impl Cloud {
    pub fn new(store: Box<dyn Store>, device_id: &str, device_name: &str) -> Cloud {
        Cloud {
            store: Cached::new(store),
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            library: Mutex::default(),
            lines: Mutex::default(),
        }
    }

    /// With the save line each game in `cfg` follows ([`wanted_line`]).
    pub fn for_config(self, cfg: &AppConfig) -> Cloud {
        let state = state::load();
        for g in &cfg.games {
            let line = wanted_line(cfg, g, state.games.get(&g.key));
            if line != library::MAIN_LINE {
                self.follow(&g.key, &line);
            }
        }
        self
    }

    /// From here on the game `key` follows the save line `line`.
    pub fn follow(&self, key: &str, line: &str) {
        self.lines.lock().unwrap().insert(key.to_string(), line.to_string());
    }

    /// The id of the save line the game `key` follows.
    pub fn following(&self, key: &str) -> String {
        self.lines.lock().unwrap().get(key).cloned().unwrap_or_else(|| library::MAIN_LINE.to_string())
    }

    pub fn kind(&self) -> &'static str {
        self.store.kind()
    }

    pub fn account(&self) -> Result<AccountInfo> {
        self.store.check()?;
        let (used, total) = self.store.quota().unwrap_or((None, None));
        Ok(AccountInfo { label: self.store.label(), used, total })
    }

    /// The cloud folder as path-based storage, for everything that is not a
    /// save snapshot (playtime, screenshots, ...) and for the modules that
    /// work on the layout itself. Small files read through it are kept on
    /// this device while the folder listing says they have not changed.
    pub fn files(&self) -> &dyn Store {
        &self.store
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    // ------------------------------------------------------------ layout

    /// What this build may do with the cloud. One read of `format.json`,
    /// unless this device looked recently.
    pub fn access(&self) -> Result<Access> {
        let seen = state::load().cloud;
        let fresh = seen.checked_at.is_some_and(|t| Utc::now() - t < Duration::hours(LAYOUT_FRESH_HOURS));
        if fresh && !seen.read_only {
            match seen.layout {
                1 => return Ok(Access::Legacy),
                v2::LAYOUT => return Ok(Access::ReadWrite),
                _ => {}
            }
        }
        self.look_at_layout()
    }

    fn look_at_layout(&self) -> Result<Access> {
        let access = v2::access(v2::read_format(&self.store)?.as_ref());
        let seen = CloudSeen {
            layout: match &access {
                Access::Legacy => 1,
                Access::ReadWrite => v2::LAYOUT,
                Access::ReadOnly { layout, .. } => *layout,
            },
            read_only: matches!(access, Access::ReadOnly { .. }),
            checked_at: Some(Utc::now()),
        };
        let _ = state::update(|s| s.cloud = seen);
        Ok(access)
    }

    /// Whether the cloud still has the layout of 0.6, as far as this device
    /// knows without asking again.
    pub fn is_legacy(&self) -> Result<bool> {
        Ok(self.access()? == Access::Legacy)
    }

    /// Make sure this build may write to the cloud; a cloud that 0.6 wrote
    /// is upgraded first ([`upgrade::run`]). Every function that writes
    /// save data calls this.
    pub fn writable(&self) -> Result<()> {
        match self.access()? {
            Access::ReadWrite => Ok(()),
            Access::ReadOnly { min_app, .. } => bail!(crate::tr!(
                "云端的数据是更新版本的 Ludonimbus 写的（需要 {} 或更高）。请先更新这台设备上的 Ludonimbus",
                "The data in the cloud was written by a newer Ludonimbus ({} or later is needed). Update Ludonimbus on this device first",
                if min_app.is_empty() { "?".to_string() } else { min_app }
            )),
            Access::Legacy => {
                logger::log("cloud", "upgrading the cloud to layout 2");
                let report = upgrade::run(&self.store, &self.device_id, &self.device_name, Utc::now(), &mut |p| {
                    logger::log("cloud", format!("upgrade: {} ({}/{})", p.game, p.done + 1, p.total));
                })?;
                logger::log(
                    "cloud",
                    format!("upgraded: {} games, {} merged, {} snapshots, {} kept saves", report.games, report.merged, report.snapshots, report.pins),
                );
                // Games may have become one card: every line is looked up again.
                self.forget_library();
                let _ = state::update(|s| {
                    s.cloud = CloudSeen { layout: v2::LAYOUT, read_only: false, checked_at: Some(Utc::now()) };
                    for g in s.games.values_mut() {
                        g.line = None;
                    }
                });
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------- lines

    /// Every game record: one listing, and a read for each record that
    /// changed since this device read it. Not asked again for a minute.
    pub fn library(&self) -> Result<Arc<Library>> {
        let mut kept = self.library.lock().unwrap();
        if let Some((at, library)) = kept.as_ref() {
            if at.elapsed() < LIBRARY_FRESH {
                return Ok(library.clone());
            }
        }
        let library = Arc::new(library::load(&self.store, &mut LibraryCache::default())?);
        *kept = Some((Instant::now(), library.clone()));
        Ok(library)
    }

    /// After a record was written: the next look reads them again.
    fn forget_library(&self) {
        *self.library.lock().unwrap() = None;
    }

    /// Where the save line of the game `key` is, for reading. No request
    /// when this device has seen the game's record before.
    pub fn place(&self, key: &str) -> Result<Place> {
        Ok(self.find(key)?.0)
    }

    /// [`Cloud::place`], and what the cloud is known to have there.
    fn find(&self, key: &str) -> Result<(Place, Seen)> {
        let want = self.following(key);
        if let Some(line) = state::game(key).line.filter(|l| is_for(l, &want)) {
            return Ok((place_of(&line), Seen::Line));
        }
        let shared = want == library::MAIN_LINE;
        // Where a line that no record has yet will be.
        let new_line = |record: Option<&v2::GameRecord>| {
            let (game, folders) = match record {
                Some(r) if !r.folders.is_empty() => (r.id.clone(), r.folders.clone()),
                _ => (key.to_string(), vec![key.to_string()]),
            };
            let dirs = vec![v2::line_dir(&folders[0], &want)];
            Place { game, line: want.clone(), folders, dirs }
        };
        if self.access()? == Access::Legacy {
            // Not remembered: the upgrade may put the line on another card.
            return Ok(if shared { (Place::of_key(key), Seen::Line) } else { (new_line(None), Seen::NewLine) });
        }
        let library = self.library()?;
        if let Some((record, line)) = line_of(&library.records, key, &want) {
            let place = Place::of(record, line);
            remember_place(key, &want, &place);
            return Ok((place, Seen::Line));
        }
        if shared {
            // No record yet: there is nothing to read but what a device on
            // 0.6 may have left under this key.
            return Ok((Place::of_key(key), Seen::NoRecord));
        }
        Ok((new_line(library::resolve(&library.records, key)), Seen::NewLine))
    }

    /// Look at the record of the game `key` again when this device hasn't
    /// for a day: its line may read more folders now (games that became one
    /// card, a device on 0.6 that uploaded). A device that writes notices
    /// in [`Cloud::place_to_write`]; this is for one that only reads. One
    /// listing, and a read when the record changed. For after playing,
    /// never before.
    pub fn look_again(&self, key: &str) -> Result<()> {
        let Some(known) = state::game(key).line else { return Ok(()) };
        if known.checked_at.is_some_and(|t| Utc::now() - t < Duration::hours(LINE_FRESH_HOURS)) || self.access()? != Access::ReadWrite {
            return Ok(());
        }
        let want = self.following(key);
        let library = self.library()?;
        if let Some((record, line)) = line_of(&library.records, key, &want) {
            remember_place(key, &want, &Place::of(record, line));
        }
        Ok(())
    }

    /// Where the save line of the game `key` is, for writing: the cloud is
    /// upgraded when it has to be, and a game the cloud doesn't know gets its
    /// record. The record is made from `def`, the game's definition; without
    /// one, from the `game.json` under the key in the cloud.
    ///
    /// `has_old`: the folder 0.6 keeps this key's snapshots in has some. The
    /// line of a new game reads that folder only then, as after an upgrade
    /// ([`upgrade::record_for`]).
    ///
    /// A line of the device's own that the record doesn't have is added to
    /// it, named after the device, in the format the game's shared line has.
    pub fn place_to_write(&self, key: &str, def: Option<&CloudGame>, has_old: bool) -> Result<Place> {
        self.writable()?;
        let want = self.following(key);
        let known = state::game(key).line.filter(|l| is_for(l, &want));
        if let Some(line) = &known {
            if line.checked_at.is_some_and(|t| Utc::now() - t < Duration::hours(LINE_FRESH_HOURS)) {
                return Ok(place_of(line));
            }
        }
        let library = self.library()?;
        if let Some((record, line)) = line_of(&library.records, key, &want) {
            let place = Place::of(record, line);
            remember_place(key, &want, &place);
            return Ok(place);
        }
        let now = Utc::now();
        let mut record = match library::resolve(&library.records, key) {
            // The game is known, the line is new.
            Some(record) => record.clone(),
            // The same record the upgrade would have made for this key, so
            // two devices that add the game at the same time write the same
            // thing.
            None => {
                let stored = match def {
                    Some(_) => None,
                    None => self.game_def(key)?,
                };
                // The caller read the line this game follows. When that is
                // not the shared one, what 0.6 left is one more listing.
                let has_old = match want == library::MAIN_LINE {
                    true => has_old,
                    false => self.store.list(&v2::legacy_dir(key))?.iter().any(|e| !e.is_dir && e.name.ends_with(".json")),
                };
                let mut record = upgrade::record_for(key, def.or(stored.as_ref()), None, has_old);
                record.created_at = Some(now);
                record
            }
        };
        if !record.lines.iter().any(|l| l.id == want) {
            // A line of this device's own, in the format the game's shared
            // line has.
            let like = record.lines.iter().find(|l| l.id == library::MAIN_LINE).or_else(|| record.lines.first());
            let folder = record.folders.first().cloned().unwrap_or_else(|| key.to_string());
            record.lines.push(v2::Line {
                id: want.clone(),
                format: like.map(|l| l.format.clone()).unwrap_or_else(|| "pc".to_string()),
                scope: v2::SCOPE_INDEPENDENT.to_string(),
                name: Some(self.device_name.clone()),
                path: v2::line_dir(&folder, &want),
                ..Default::default()
            });
        }
        let saved = library::save(&self.store, &record, &self.device_id, now)?;
        self.forget_library();
        let line = saved
            .lines
            .iter()
            .find(|l| l.id == want)
            .ok_or_else(|| anyhow!(crate::tr!("云端的游戏记录 {key} 没有这条存档线", "The game record {key} in the cloud doesn't have this save line")))?;
        let place = Place::of(&saved, line);
        remember_place(key, &want, &place);
        Ok(place)
    }

    /// Give the save line the game `key` follows a name (a line of its own:
    /// "the kid's PC"). The game's record is read and written.
    pub fn name_line(&self, key: &str, name: &str) -> Result<()> {
        self.writable()?;
        let want = self.following(key);
        self.forget_library();
        let library = self.library()?;
        let Some((record, line)) = line_of(&library.records, key, &want) else {
            bail!(crate::tr!("云端还没有这条存档线", "The cloud doesn't have this save line yet"));
        };
        let name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
        if line.name == name {
            return Ok(());
        }
        let mut record = record.clone();
        let id = line.id.clone();
        if let Some(l) = record.lines.iter_mut().find(|l| l.id == id) {
            l.name = name;
        }
        library::save(&self.store, &record, &self.device_id, Utc::now())?;
        self.forget_library();
        Ok(())
    }

    /// The save lines the game `key` could follow: those of its card with
    /// saves in the format its shared line has. None when the cloud has no
    /// record of the game.
    pub fn lines_of(&self, key: &str) -> Result<Vec<v2::Line>> {
        if self.access()? == Access::Legacy {
            return Ok(Vec::new());
        }
        let library = self.library()?;
        let Some(record) = library::resolve(&library.records, key) else { return Ok(Vec::new()) };
        let format = line_of(&library.records, key, library::MAIN_LINE).map(|(_, l)| l.format.clone());
        Ok(record.lines.iter().filter(|l| format.as_ref().is_none_or(|f| &l.format == f)).cloned().collect())
    }

    /// Make the line of the game `key` read the folder 0.6 keeps the key's
    /// snapshots in, when the line doesn't yet and a device still on 0.6 has
    /// put something there. One listing; returns whether the line changed.
    pub fn adopt_old_folder(&self, key: &str) -> Result<bool> {
        let Some(known) = state::game(key).line else { return Ok(false) };
        let old = v2::legacy_dir(key);
        // Only the shared line: what a device on 0.6 uploads is the game's
        // progress as the other devices share it.
        if !known.asked.is_empty() || known.dirs.contains(&old) || self.access()? != Access::ReadWrite {
            return Ok(false);
        }
        if !self.store.list(&old)?.iter().any(|e| !e.is_dir && e.name.ends_with(".json")) {
            return Ok(false);
        }
        let Some(mut record) = library::read(&self.store, &known.game)? else { return Ok(false) };
        let Some(line) = record.lines.iter_mut().find(|l| l.id == known.line) else { return Ok(false) };
        if line.path != old && !line.also.contains(&old) {
            line.also.push(old);
            library::save(&self.store, &record, &self.device_id, Utc::now())?;
            self.forget_library();
        }
        // Looked up again the next time the line is opened.
        let _ = state::update_game(key, |g| g.line = None);
        Ok(true)
    }

    fn kept_cache(key: &str, place: &Place) -> LineCache {
        std::fs::read(line_file(key))
            .ok()
            .and_then(|b| serde_json::from_slice::<KeptLine>(&b).ok())
            .filter(|k| k.dirs == place.dirs)
            .map(|k| k.cache)
            .unwrap_or_default()
    }

    /// Read the save line of the game `key`.
    ///
    /// A game the cloud has no record of has no saves there, unless a device
    /// on 0.6 uploaded some: one listing of the game's folder tells, and
    /// the line's folders are only read when there is something.
    pub fn open(&self, key: &str) -> Result<OpenLine> {
        let (place, seen) = self.find(key)?;
        let empty = match seen {
            Seen::Line => false,
            Seen::NewLine => true,
            Seen::NoRecord => {
                let folder = self.store.list(&join(&["games", key]))?;
                !folder.iter().any(|e| e.is_dir && matches!(e.name.as_str(), "saves" | "snapshots"))
            }
        };
        if empty {
            return Ok(OpenLine { key: key.to_string(), place, view: LineView::default(), cache: LineCache::default() });
        }
        self.open_at(key, place)
    }

    /// Read a save line whose place the caller knows.
    pub fn open_at(&self, key: &str, place: Place) -> Result<OpenLine> {
        let mut cache = Self::kept_cache(key, &place);
        let before = cache.clone();
        let view = saves::open(&self.store, &place, &mut cache)?;
        let line = OpenLine { key: key.to_string(), place, view, cache };
        if line.cache != before {
            line.remember();
        }
        Ok(line)
    }

    /// Read the line again after it changed, with the same cache.
    pub fn reopen(&self, line: &mut OpenLine) -> Result<()> {
        line.view = saves::open(&self.store, &line.place, &mut line.cache)?;
        line.remember();
        Ok(())
    }

    // ------------------------------------- snapshots, in the shape of 0.6

    /// Every version of the game `key`, oldest first.
    pub fn list_snapshots(&self, key: &str) -> Result<Vec<SnapshotSummary>> {
        Ok(summaries(&self.open(key)?.view))
    }

    pub fn get_meta(&self, key: &str, id: &str) -> Result<SnapshotMeta> {
        let line = self.open(key)?;
        let manifest = saves::manifest(&self.store, &line.place, &line.view, id)?;
        Ok(SnapshotMeta::of(&manifest, key, line.view.get(id)))
    }

    /// One version as a zip: `files/<path>` and `snapshot.json`.
    pub fn download(&self, key: &str, id: &str, dest: &Path) -> Result<()> {
        let line = self.open(key)?;
        let manifest = saves::manifest(&self.store, &line.place, &line.view, id)?;
        let meta = SnapshotMeta::of(&manifest, key, line.view.get(id));
        let work = paths::staging_dir().join(format!("download-{}", ulid::Ulid::new()));
        let files = work.join("files");
        let result = saves::fetch(&self.store, &line.place, &line.view, &manifest, None, &files, &work.join("packs"))
            .and_then(|_| crate::snapshot::zip_dir(&files, &meta, dest));
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    // --------------------------------------------- devices, definitions

    pub fn put_device(&self, device: &DeviceInfo) -> Result<()> {
        self.store.write(&join(&["devices", &format!("{}.json", device.id)]), &serde_json::to_vec_pretty(device)?)
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut out = Vec::new();
        for e in self.store.list("devices")? {
            if e.is_dir || !e.name.ends_with(".json") {
                continue;
            }
            if let Some(bytes) = self.store.read(&join(&["devices", &e.name]))? {
                if let Ok(d) = serde_json::from_slice(&bytes) {
                    out.push(d);
                }
            }
        }
        Ok(out)
    }

    /// A game's definition as 0.6 keeps it (`games/<key>/game.json`): what
    /// other devices need to find its saves. Game records don't hold that
    /// yet, so this file is still written.
    pub fn put_game(&self, game: &CloudGame) -> Result<()> {
        self.store.write(&join(&["games", &game.key, "game.json"]), &serde_json::to_vec_pretty(game)?)
    }

    /// The definition of one game, if the cloud has it.
    pub fn game_def(&self, key: &str) -> Result<Option<CloudGame>> {
        Ok(self.store.read(&join(&["games", key, "game.json"]))?.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    pub fn list_games(&self) -> Result<Vec<CloudGame>> {
        let mut out = Vec::new();
        for e in self.store.list("games")? {
            if !e.is_dir {
                continue;
            }
            if let Some(bytes) = self.store.read(&join(&["games", &e.name, "game.json"]))? {
                if let Ok(g) = serde_json::from_slice(&bytes) {
                    out.push(g);
                }
            }
        }
        Ok(out)
    }
}

/// The versions of a line as the app lists them, oldest first.
pub fn summaries(view: &LineView) -> Vec<SnapshotSummary> {
    let mut out: Vec<SnapshotSummary> =
        view.snapshots.iter().enumerate().map(|(i, s)| SnapshotSummary::of(s, view.heads.contains(&i))).collect();
    // A version made by restoring another says so where 0.6 kept a note; so
    // does one a device brought along from another save line.
    for i in 0..out.len() {
        if !out[i].comment.is_empty() {
            continue;
        }
        let Some(origin) = view.snapshots[i].origin.as_ref() else { continue };
        if origin.how == "copy" {
            out[i].comment = match origin.line == library::MAIN_LINE {
                true => crate::tr!("来自共用的存档", "From the shared saves"),
                false => crate::tr!("来自另一条存档线", "From another save line"),
            };
            continue;
        }
        if origin.how != "restore" {
            continue;
        }
        if let Some(from) = view.get(&origin.snapshot) {
            out[i].comment = crate::tr!(
                "恢复自 {} 在 {} 的版本",
                "Restored from the version {} saved at {}",
                from.device_name,
                from.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceConfig;

    fn config(device: &str, independent: bool) -> AppConfig {
        AppConfig { device: DeviceConfig { id: device.to_string(), name: "PC".to_string() }, independent_saves: independent, ..AppConfig::default() }
    }

    fn game(key: &str, line: Option<&str>) -> GameEntry {
        GameEntry { key: key.to_string(), line: line.map(str::to_string), ..GameEntry::default() }
    }

    #[test]
    fn a_device_has_one_line_of_its_own_for_a_game() {
        let id = own_line_id("d-01", "stardew");
        assert_eq!(id, own_line_id("d-01", "stardew"));
        assert_ne!(id, own_line_id("d-02", "stardew"));
        assert_ne!(id, own_line_id("d-01", "celeste"));
        assert_ne!(id, library::MAIN_LINE);
        let tail = id.strip_prefix("solo-").unwrap();
        assert_eq!(tail.len(), 4);
        assert!(tail.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()), "{id}");
    }

    #[test]
    fn the_line_a_game_follows_is_the_one_chosen_or_the_devices_default() {
        let synced = GameState { last_synced_id: Some("01A".to_string()), ..GameState::default() };
        let waiting = GameState { pending: vec!["01A".to_string()], ..GameState::default() };
        let new = GameState::default();

        // Nothing chosen, no default: shared.
        let cfg = config("d-01", false);
        assert_eq!(wanted_line(&cfg, &game("g", None), None), "main");
        assert_eq!(wanted_line(&cfg, &game("g", None), Some(&synced)), "main");
        // What was chosen, whatever the default.
        assert_eq!(wanted_line(&cfg, &game("g", Some("solo-abcd")), Some(&synced)), "solo-abcd");

        // The default is for games that have not synced here.
        let cfg = config("d-01", true);
        let own = own_line_id("d-01", "g");
        assert_eq!(wanted_line(&cfg, &game("g", None), None), own);
        assert_eq!(wanted_line(&cfg, &game("g", None), Some(&new)), own);
        assert_eq!(wanted_line(&cfg, &game("g", None), Some(&synced)), "main");
        assert_eq!(wanted_line(&cfg, &game("g", None), Some(&waiting)), "main");
        assert_eq!(wanted_line(&cfg, &game("g", Some("main")), None), "main");
        assert_eq!(wanted_line(&cfg, &game("g", Some("")), Some(&synced)), "main");
    }
}
