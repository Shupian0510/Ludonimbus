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
use crate::snapshot::{SnapshotMeta, SnapshotSummary};
use crate::state::{self, CloudSeen, LineRef};
use crate::{logger, paths, util};
use anyhow::{anyhow, bail, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
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

fn remember_place(key: &str, place: &Place) {
    let line = LineRef {
        game: place.game.clone(),
        line: place.line.clone(),
        folders: place.folders.clone(),
        dirs: place.dirs.clone(),
        checked_at: Some(Utc::now()),
    };
    let _ = state::update_game(key, |g| g.line = Some(line));
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
        Cloud { store: Cached::new(store), device_id: device_id.to_string(), device_name: device_name.to_string(), library: Mutex::default() }
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

    /// [`Cloud::place`], and whether the cloud is known to have something
    /// for the game: `false` for a game without a record in a cloud of
    /// layout 2.
    fn find(&self, key: &str) -> Result<(Place, bool)> {
        if let Some(line) = state::game(key).line {
            return Ok((place_of(&line), true));
        }
        if self.access()? == Access::Legacy {
            // Not remembered: the upgrade may put the line on another card.
            return Ok((Place::of_key(key), true));
        }
        let library = self.library()?;
        match library::resolve_line(&library.records, key, library::MAIN_LINE) {
            Some((record, line)) => {
                let place = Place::of(record, line);
                remember_place(key, &place);
                Ok((place, true))
            }
            // No record yet: there is nothing to read but what a device on
            // 0.6 may have left under this key.
            None => Ok((Place::of_key(key), false)),
        }
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
        let library = self.library()?;
        if let Some((record, line)) = library::resolve_line(&library.records, key, library::MAIN_LINE) {
            remember_place(key, &Place::of(record, line));
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
    pub fn place_to_write(&self, key: &str, def: Option<&CloudGame>, has_old: bool) -> Result<Place> {
        self.writable()?;
        let known = state::game(key).line;
        if let Some(line) = &known {
            if line.checked_at.is_some_and(|t| Utc::now() - t < Duration::hours(LINE_FRESH_HOURS)) {
                return Ok(place_of(line));
            }
        }
        let library = self.library()?;
        if let Some((record, line)) = library::resolve_line(&library.records, key, library::MAIN_LINE) {
            let place = Place::of(record, line);
            remember_place(key, &place);
            return Ok(place);
        }
        // The same record the upgrade would have made for this key, so two
        // devices that add the game at the same time write the same thing.
        let now = Utc::now();
        let stored = match def {
            Some(_) => None,
            None => self.game_def(key)?,
        };
        let mut record = upgrade::record_for(key, def.or(stored.as_ref()), None, has_old);
        record.created_at = Some(now);
        let saved = library::save(&self.store, &record, &self.device_id, now)?;
        self.forget_library();
        let line = saved
            .lines
            .iter()
            .find(|l| l.id == library::MAIN_LINE)
            .or_else(|| saved.lines.first())
            .ok_or_else(|| anyhow!(crate::tr!("云端的游戏记录 {key} 没有存档线", "The game record {key} in the cloud has no save line")))?;
        let place = Place::of(&saved, line);
        remember_place(key, &place);
        Ok(place)
    }

    /// Make the line of the game `key` read the folder 0.6 keeps the key's
    /// snapshots in, when the line doesn't yet and a device still on 0.6 has
    /// put something there. One listing; returns whether the line changed.
    pub fn adopt_old_folder(&self, key: &str) -> Result<bool> {
        let Some(known) = state::game(key).line else { return Ok(false) };
        let old = v2::legacy_dir(key);
        if known.dirs.contains(&old) || self.access()? != Access::ReadWrite {
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
        let (place, known) = self.find(key)?;
        if !known {
            let folder = self.store.list(&join(&["games", key]))?;
            if !folder.iter().any(|e| e.is_dir && matches!(e.name.as_str(), "saves" | "snapshots")) {
                return Ok(OpenLine { key: key.to_string(), place, view: LineView::default(), cache: LineCache::default() });
            }
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
    // A version made by restoring another says so where 0.6 kept a note.
    for i in 0..out.len() {
        if !out[i].comment.is_empty() {
            continue;
        }
        let Some(origin) = view.snapshots[i].origin.as_ref().filter(|o| o.how == "restore") else { continue };
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
