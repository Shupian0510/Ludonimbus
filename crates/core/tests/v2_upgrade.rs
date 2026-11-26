//! Bringing a cloud written by 0.6 to layout 2 (docs/plan-v2.md, section
//! 12), on the cloud the released 0.6.2 wrote (`tests/fixtures/cloud-0.6`):
//! what is added, that nothing of 0.6 changes, what the new layout makes of
//! it, doing it again after a break or from two devices, and how many
//! requests it takes.

use anyhow::{bail, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use ludonimbus_core::cloud::cached::Cached;
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::packs;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::upgrade::{self, Progress, Report};
use ludonimbus_core::cloud::v2::{self, FileRef, GameRecord, Line, Manifest, Pin};
use ludonimbus_core::vv::Vv;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

const DEVICE: &str = "d-01m9c0000000000000000000cc";
const DEVICE_NAME: &str = "PC-C";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 12, 1, 10, 0, 0).unwrap()
}

// ------------------------------------------------------------------ the sample

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cloud-0.6")
}

/// Every file under `root`, by its path relative to it.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel: Vec<String> = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        out.insert(rel.join("/"), std::fs::read(entry.path()).unwrap());
    }
    out
}

/// The sample as a cloud: every file under its path.
fn sample() -> Memory {
    let store = Memory::new();
    for (path, data) in tree(&fixture().join("cloud")) {
        store.put(&path, &data);
    }
    store
}

/// What 0.6.2 itself listed for each game when the sample was made.
fn expected() -> Value {
    serde_json::from_slice(&std::fs::read(fixture().join("expected.json")).unwrap()).unwrap()
}

fn keys() -> Vec<String> {
    expected().as_object().unwrap().keys().cloned().collect()
}

/// The 0.6 snapshots of `key` that the user kept: locked, or made by hand.
fn kept(key: &str) -> Vec<Value> {
    expected()[key].as_array().unwrap().iter().filter(|s| s["locked"] == true || s["trigger"] == "manual").cloned().collect()
}

fn old_manifest(store: &Memory, key: &str, id: &str) -> Manifest {
    serde_json::from_slice(&store.peek(&format!("games/{key}/snapshots/{id}.json")).unwrap()).unwrap()
}

/// When the oldest snapshot of `key` was made.
fn oldest(store: &Memory, key: &str) -> DateTime<Utc> {
    expected()[key].as_array().unwrap().iter().map(|s| old_manifest(store, key, s["id"].as_str().unwrap()).created_at).min().unwrap()
}

// ---------------------------------------------------------------------- tools

fn run(store: &dyn Store) -> Result<Report> {
    upgrade::run(store, DEVICE, DEVICE_NAME, now(), &mut |_| {})
}

fn files(store: &Memory) -> BTreeMap<String, Vec<u8>> {
    store.paths().into_iter().map(|p| (p.clone(), store.peek(&p).unwrap())).collect()
}

fn records(store: &Memory) -> Vec<GameRecord> {
    library::load(store, &mut LibraryCache::default()).unwrap().records
}

/// A line's folders: the one it writes to, then the ones it only reads.
fn folders(line: &Line) -> Vec<&str> {
    std::iter::once(line.path.as_str()).chain(line.also.iter().map(String::as_str)).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Forwards to a `Memory`, except that write number `fail_at` (counting
/// from 1) fails, as when the connection drops, and with `fail_removes`
/// every remove does. Remembers the paths it wrote and removed.
struct Flaky<'a> {
    inner: &'a Memory,
    fail_at: Option<u32>,
    fail_removes: bool,
    writes: AtomicU32,
    touched: Mutex<Vec<String>>,
}

impl<'a> Flaky<'a> {
    fn new(inner: &'a Memory) -> Self {
        Flaky { inner, fail_at: None, fail_removes: false, writes: AtomicU32::new(0), touched: Mutex::default() }
    }

    fn failing_at(inner: &'a Memory, n: u32) -> Self {
        Flaky { fail_at: Some(n), ..Flaky::new(inner) }
    }

    fn writes(&self) -> u32 {
        self.writes.load(Ordering::SeqCst)
    }

    fn touched(&self) -> Vec<String> {
        self.touched.lock().unwrap().clone()
    }

    fn write_or_fail(&self, path: &str) -> Result<()> {
        let n = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_at == Some(n) {
            bail!("connection lost writing {path}");
        }
        self.touched.lock().unwrap().push(path.to_string());
        Ok(())
    }
}

impl Store for Flaky<'_> {
    fn kind(&self) -> &'static str {
        "flaky"
    }
    fn label(&self) -> String {
        "flaky".into()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.write_or_fail(path)?;
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.write_or_fail(path)?;
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        if self.fail_removes {
            bail!("connection lost removing {path}");
        }
        self.touched.lock().unwrap().push(path.to_string());
        self.inner.remove(path)
    }
}

/// Fields set from a clock: what `library::save` stamps on a record and
/// `line::write_index` on an index (`updatedAt`), and `migratedAt`.
const CLOCK: &[&str] = &["updatedAt", "migratedAt"];
/// The same, and which device wrote them.
const CLOCK_AND_DEVICE: &[&str] = &["updatedAt", "migratedAt", "updatedBy", "migratedBy"];

/// The cloud's files as two runs are compared. JSON files parsed, without
/// the top-level fields in `ignore`; in an index, the tag it keeps for a pin
/// replaced by whether it is the tag the pin file has in the listing
/// (`Memory` numbers its writes, so the tags differ between two runs
/// whatever is written; what matters is that the index has the current
/// one). Other files by their hash.
fn state(store: &Memory, ignore: &[&str]) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for path in store.paths() {
        let data = store.peek(&path).unwrap();
        if !path.ends_with(".json") {
            out.insert(path, Value::String(blake3::hash(&data).to_hex().to_string()));
            continue;
        }
        let mut value: Value = serde_json::from_slice(&data).unwrap();
        if let Value::Object(map) = &mut value {
            for field in ignore {
                map.remove(*field);
            }
        }
        if let Some(dir) = path.strip_suffix("/index.json") {
            let tags: BTreeMap<String, String> = store
                .list(dir)
                .unwrap()
                .into_iter()
                .filter_map(|e| Some((e.name.strip_suffix(v2::PIN_SUFFIX)?.to_string(), e.tag)))
                .collect();
            for entry in value["snapshots"].as_array_mut().unwrap() {
                let id = entry["id"].as_str().unwrap().to_string();
                if let Some(pin) = entry.get_mut("pin") {
                    let current = tags.get(&id).map(String::as_str) == pin["tag"].as_str();
                    pin["tag"] = json!(if current { "as listed" } else { "stale" });
                }
            }
        }
        out.insert(path, value);
    }
    out
}

/// What the table in the module documentation of `upgrade` promises for a
/// whole run on a cloud of layout 1, with nothing cached. `games` holds,
/// for each game, how many snapshots its old folder has and how many of
/// them get a pin.
fn budget(games: &[(u32, u32)]) -> Counts {
    // Once: format.json read; games/ listed; the lock taken (read, write,
    // read back) and given back (read, remove); format.json written.
    let mut c = Counts { read: 1 + 2 + 1, list: 1, write: 1 + 1, remove: 1, ..Default::default() };
    for &(snapshots, pins) in games {
        // Both folders listed, and the new one again after writing pins.
        c.list += 2 + u32::from(pins > 0);
        // game.json, every snapshot, the record before saving it.
        c.read += 1 + snapshots + 1;
        // The record, the pins, the index.
        c.write += 1 + pins + u32::from(snapshots > 0);
    }
    c
}

/// (snapshots, pins) of each game in the sample.
fn sample_budget() -> Counts {
    let games: Vec<(u32, u32)> =
        keys().iter().map(|k| (expected()[k].as_array().unwrap().len() as u32, kept(k).len() as u32)).collect();
    budget(&games)
}

// ---------------------------------------------------------------------- tests

#[test]
fn nothing_of_0_6_changes_and_everything_else_is_an_addition() {
    let store = sample();
    let before = files(&store);
    let flaky = Flaky::new(&store);
    run(&flaky).unwrap();
    let after = files(&store);

    // Every file of 0.6 is there, byte for byte, and none was even
    // rewritten or removed on the way.
    for (path, data) in &before {
        assert_eq!(after.get(path), Some(data), "{path}");
    }
    for path in flaky.touched() {
        assert!(!before.contains_key(&path), "{path} was written or removed");
    }

    // What was added is in the places the upgrade adds to, and nothing else.
    let added: BTreeSet<String> = after.keys().filter(|p| !before.contains_key(*p)).cloned().collect();
    let mut places = BTreeSet::from([v2::FORMAT_FILE.to_string()]);
    for key in keys() {
        places.insert(v2::game_record_path(&key));
        places.insert(v2::index_path(&v2::line_dir(&key, "main")));
        for s in kept(&key) {
            places.insert(v2::pin_path(&v2::line_dir(&key, "main"), s["id"].as_str().unwrap()));
        }
    }
    assert_eq!(added, places);
    assert_eq!(added.len(), 1 + 4 + 4 + 2);
    assert!(store.peek(upgrade::LOCK_FILE).is_none(), "the lock is gone");

    // format.json last, saying who did it and when.
    let format: Value = serde_json::from_slice(&store.peek(v2::FORMAT_FILE).unwrap()).unwrap();
    assert_eq!(
        format,
        json!({ "layout": 2, "minApp": "1.0.0", "requires": [], "migratedAt": now(), "migratedBy": DEVICE })
    );
    assert_eq!(flaky.touched().iter().filter(|p| !p.as_str().eq(upgrade::LOCK_FILE)).last().unwrap(), v2::FORMAT_FILE);
    assert_eq!(v2::access(v2::read_format(&store).unwrap().as_ref()), v2::Access::ReadWrite);
}

#[test]
fn one_record_per_key_and_one_card_per_game() {
    let store = sample();
    let mut seen = Vec::new();
    let report =
        upgrade::run(&store, DEVICE, DEVICE_NAME, now(), &mut |p: &Progress| seen.push((p.game.clone(), p.done, p.total))).unwrap();
    assert_eq!(report, Report { already: false, games: 4, merged: 1, snapshots: 16, pins: 2 });
    let fe_name = "火焰纹章：万缕千丝";
    assert_eq!(
        seen,
        [("Abs Game", 0), (fe_name, 1), (fe_name, 2), ("Test Game", 3)].map(|(name, done)| (name.to_string(), done, 4))
    );

    let records = records(&store);
    assert_eq!(records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), keys());
    let line = |id: &str, format: &str, key: &str| Line {
        id: id.into(),
        format: format.into(),
        scope: "shared".into(),
        path: format!("games/{key}/saves/main"),
        also: vec![format!("games/{key}/snapshots")],
        ..Default::default()
    };
    // Saved once: what comes from this device and the clock is what
    // `library::save` stamps, nothing else.
    let saved = |record: GameRecord| GameRecord { rev: 1, updated_at: Some(now()), updated_by: DEVICE.into(), ..record };

    for (key, name) in [("abs-game", "Abs Game"), ("test-game", "Test Game")] {
        assert_eq!(
            library::get(&records, key).unwrap(),
            &saved(GameRecord {
                id: key.into(),
                name: name.into(),
                ids: vec![library::name_id(name).unwrap()],
                folders: strings(&[key]),
                lines: vec![line("main", "pc", key)],
                created_at: Some(oldest(&store, key)),
                ..Default::default()
            }),
            "{key}"
        );
    }

    // One game added on two devices under two keys, the same built-in rule:
    // one card. Made first, `fe-fortunes-weave` stays; each key keeps its own
    // line and history under it.
    let fe_ids = vec![library::name_id(fe_name).unwrap(), "adapter:fe-fortunes-weave".to_string()];
    let format = "adapter:fe-fortunes-weave";
    let second = saved(GameRecord {
        id: "fe-fortunes-weave-2".into(),
        name: fe_name.into(),
        ids: fe_ids.clone(),
        folders: strings(&["fe-fortunes-weave-2"]),
        lines: vec![line("main", format, "fe-fortunes-weave-2")],
        created_at: Some(oldest(&store, "fe-fortunes-weave-2")),
        ..Default::default()
    });
    let first = saved(GameRecord {
        id: "fe-fortunes-weave".into(),
        name: fe_name.into(),
        ids: fe_ids,
        folders: strings(&["fe-fortunes-weave", "fe-fortunes-weave-2"]),
        lines: vec![
            line("main", format, "fe-fortunes-weave"),
            Line { id: "main-fe-fortunes-weave-2".into(), ..line("main", format, "fe-fortunes-weave-2") },
        ],
        created_at: Some(oldest(&store, "fe-fortunes-weave")),
        ..Default::default()
    });
    assert!(first.created_at < second.created_at);
    assert_eq!(library::get(&records, "fe-fortunes-weave").unwrap(), &first);
    assert_eq!(
        library::get(&records, "fe-fortunes-weave-2").unwrap(),
        &GameRecord { merged_into: Some("fe-fortunes-weave".into()), ..second }
    );
    assert_eq!(library::live(&records).count(), 3);

    // What a device remembers of the old key finds the card and the line.
    assert_eq!(library::resolve(&records, "fe-fortunes-weave-2").unwrap().id, "fe-fortunes-weave");
    let (card, moved) = library::resolve_line(&records, "fe-fortunes-weave-2", "main").unwrap();
    assert_eq!((card.id.as_str(), moved.id.as_str()), ("fe-fortunes-weave", "main-fe-fortunes-weave-2"));
    assert_eq!(folders(moved), ["games/fe-fortunes-weave-2/saves/main", "games/fe-fortunes-weave-2/snapshots"]);
    let (_, own) = library::resolve_line(&records, "fe-fortunes-weave", "main").unwrap();
    assert_eq!(folders(own), ["games/fe-fortunes-weave/saves/main", "games/fe-fortunes-weave/snapshots"]);
    assert!(library::dedupe(&records).len() <= 1, "only lines could still be joined, never cards");

    // Another device, another day, a cloud of its own: the same records but
    // for the stamp.
    let elsewhere = sample();
    upgrade::run(&elsewhere, "d-other", "Deck", now() + Duration::days(3), &mut |_| {}).unwrap();
    let unstamped = |records: Vec<GameRecord>| -> Vec<GameRecord> {
        records.into_iter().map(|r| GameRecord { updated_at: None, updated_by: String::new(), ..r }).collect()
    };
    assert_eq!(unstamped(self::records(&elsewhere)), unstamped(records));
}

#[test]
fn locked_snapshots_and_manual_backups_get_a_pin() {
    let store = sample();
    run(&store).unwrap();

    let found: BTreeMap<String, Pin> = store
        .paths()
        .into_iter()
        .filter(|p| p.ends_with(v2::PIN_SUFFIX))
        .map(|p| {
            let pin = serde_json::from_slice(&store.peek(&p).unwrap()).unwrap();
            (p, pin)
        })
        .collect();

    // The sample has two: a locked one with a note, a manual one with a note.
    let kept = kept("test-game");
    assert_eq!(kept.len(), 2);
    let names: BTreeSet<&str> = kept.iter().map(|s| s["comment"].as_str().unwrap()).collect();
    assert_eq!(names, BTreeSet::from(["keep this", "before boss"]));
    assert!(keys().iter().filter(|k| *k != "test-game").all(|k| self::kept(k).is_empty()));

    let wanted: BTreeMap<String, Pin> = kept
        .iter()
        .map(|s| {
            let id = s["id"].as_str().unwrap();
            let m = old_manifest(&store, "test-game", id);
            let pin = Pin {
                name: s["comment"].as_str().unwrap().into(),
                created_at: Some(m.created_at),
                device_id: m.device_id.clone(),
                ..Default::default()
            };
            (v2::pin_path("games/test-game/saves/main", id), pin)
        })
        .collect();
    assert_eq!(found, wanted, "these two, and nothing else");

    // A note on a version that isn't kept stays a note (the restore has
    // one): it follows the snapshot into the index, it gets no pin.
    let index = line::read_index(&store, "games/test-game/saves/main").unwrap().unwrap();
    let restore = index.snapshots.iter().find(|s| s.trigger == "restore").unwrap();
    assert!(restore.pin.is_none() && restore.old_note().is_some());
    let pinned: BTreeMap<&str, &str> =
        index.snapshots.iter().filter_map(|s| Some((s.id.as_str(), s.pin.as_ref()?.name.as_str()))).collect();
    assert_eq!(pinned.values().copied().collect::<BTreeSet<_>>(), names);
}

#[test]
fn afterwards_a_line_costs_its_listings_and_one_read() {
    let store = sample();
    run(&store).unwrap();
    let records = records(&store);
    let mut lines = 0;
    for card in library::live(&records) {
        for l in &card.lines {
            let dirs = folders(l);
            store.take();
            let view = line::load_all(&store, &dirs, &mut LineCache::default()).unwrap();
            assert_eq!(store.take(), Counts { list: dirs.len() as u32, read: 1, ..Default::default() }, "{}", l.path);
            assert!(!view.needs_index && view.unreadable.is_empty());

            // The same view as reading every manifest and pin.
            store.remove(&v2::index_path(&l.path)).unwrap();
            store.take();
            let slow = line::load_all(&store, &dirs, &mut LineCache::default()).unwrap();
            let c = store.take();
            assert_eq!(c.read as usize, slow.snapshots.len() + slow.snapshots.iter().filter(|s| s.pin.is_some()).count());
            same_view(&view, &slow, &l.path);
            assert!(!view.is_forked() && view.head().is_some());
            lines += 1;
        }
    }
    assert_eq!(lines, 4);
}

fn same_view(a: &LineView, b: &LineView, what: &str) {
    assert_eq!(a.snapshots, b.snapshots, "{what}");
    assert_eq!(a.heads, b.heads, "{what}");
    assert_eq!(a.places, b.places, "{what}");
    assert_eq!(a.unreadable, b.unreadable, "{what}");
}

#[test]
fn every_old_version_still_restores_through_the_records() {
    let store = sample();
    run(&store).unwrap();
    let records = records(&store);
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    let mut restored = 0;
    for (key, versions) in expected().as_object().unwrap() {
        let versions = versions.as_array().unwrap();
        let (card, l) = library::resolve_line(&records, key, library::MAIN_LINE).unwrap();
        let dirs = folders(l);
        let view = line::load_all(&store, &dirs, &mut LineCache::default()).unwrap();
        // The order 0.6 showed them in.
        assert_eq!(
            view.snapshots.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            versions.iter().map(|v| v["id"].as_str().unwrap()).collect::<Vec<_>>(),
            "{key}"
        );
        for was in versions {
            let id = was["id"].as_str().unwrap();
            let dir = dirs[view.place(id).unwrap()];
            assert_eq!(dir, v2::legacy_dir(key));
            let m = line::read_manifest(&store, dir, id).unwrap().unwrap();
            assert_eq!(m.content_hash, was["contentHash"].as_str().unwrap(), "{key} {id}");
            let dest = tmp.path().join("restored").join(id);
            // Every file is checked against the size and hash in the manifest.
            packs::read(&store, &card.folders, dir, &m, None, &dest, &staging).unwrap();
            let files: BTreeMap<String, Vec<u8>> = was["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(p, text)| (p.clone(), text.as_str().unwrap().as_bytes().to_vec()))
                .collect();
            assert_eq!(tree(&dest), files, "{key} {id}");
            assert_eq!(m.files.iter().map(|f| f.path.clone()).collect::<BTreeSet<_>>(), files.keys().cloned().collect());
            restored += 1;
        }
    }
    assert_eq!(restored, 16);
}

#[test]
fn a_second_run_reads_one_file_and_writes_nothing() {
    let store = sample();
    run(&store).unwrap();
    let before = files(&store);
    store.take();
    assert_eq!(run(&store).unwrap(), Report { already: true, ..Default::default() });
    assert_eq!(store.take(), Counts { read: 1, ..Default::default() });
    assert_eq!(files(&store), before);
}

#[test]
fn a_run_that_broke_off_is_finished_by_the_next() {
    // The run without a break, to compare with.
    let whole = sample();
    let flaky = Flaky::new(&whole);
    let report = run(&flaky).unwrap();
    // The lock, a pin for each of two snapshots, an index and a record for
    // each of four games, format.json.
    let writes = flaky.writes();
    assert_eq!(writes, 1 + 2 + 4 + 4 + 1);
    let wanted = state(&whole, CLOCK);

    for n in 1..=writes {
        let store = sample();
        assert!(run(&Flaky::failing_at(&store, n)).is_err(), "write {n}");
        // Still of layout 1, so a device on 0.6 or a new build sees it as
        // before; the lock given back.
        assert_eq!(v2::access(v2::read_format(&store).unwrap().as_ref()), v2::Access::Legacy, "write {n}");
        assert!(store.peek(upgrade::LOCK_FILE).is_none(), "write {n}");

        assert_eq!(run(&store).unwrap(), report, "write {n}");
        assert_eq!(state(&store, CLOCK), wanted, "write {n}");
    }
}

#[test]
fn two_devices_leave_what_one_would_have_left() {
    let whole = sample();
    let flaky = Flaky::new(&whole);
    run(&flaky).unwrap();
    let writes = flaky.writes();
    let wanted = state(&whole, CLOCK_AND_DEVICE);

    for n in 1..=writes {
        // Device A breaks off at write `n`; device B upgrades the same cloud.
        let store = sample();
        assert!(upgrade::run(&Flaky::failing_at(&store, n), "d-a", "PC-A", now(), &mut |_| {}).is_err());
        upgrade::run(&store, "d-b", "PC-B", now() + Duration::minutes(1), &mut |_| {}).unwrap();
        assert_eq!(state(&store, CLOCK_AND_DEVICE), wanted, "A broke off at write {n}");

        // The same, but A lost its connection for good and its lock stayed.
        let store = sample();
        let a = Flaky { fail_removes: true, ..Flaky::failing_at(&store, n) };
        assert!(upgrade::run(&a, "d-a", "PC-A", now(), &mut |_| {}).is_err());
        if store.peek(upgrade::LOCK_FILE).is_some() {
            // B waits while it holds ...
            let before = files(&store);
            store.take();
            let err = upgrade::run(&store, "d-b", "PC-B", now() + Duration::minutes(5), &mut |_| {}).unwrap_err();
            assert!(err.to_string().contains("PC-A"), "{err}");
            let c = store.take();
            assert_eq!((c.write, c.remove), (0, 0));
            assert_eq!(files(&store), before);
        }
        // ... and takes over once it has expired.
        upgrade::run(&store, "d-b", "PC-B", now() + Duration::minutes(31), &mut |_| {}).unwrap();
        assert_eq!(state(&store, CLOCK_AND_DEVICE), wanted, "A left its lock at write {n}");
    }
}

fn lock_file(device_id: &str, device_name: &str, since: DateTime<Utc>, expires: DateTime<Utc>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "deviceId": device_id, "deviceName": device_name, "kind": "upgrade",
        "since": since, "expires": expires, "token": "01M9TOKEN"
    }))
    .unwrap()
}

/// Run, expecting an error that leaves the cloud as it was.
fn refused(store: &Memory) -> String {
    let before = files(store);
    store.take();
    let err = run(store).unwrap_err();
    let c = store.take();
    assert_eq!((c.write, c.upload, c.remove), (0, 0, 0), "{err}");
    assert_eq!(files(store), before);
    format!("{err:#}")
}

#[test]
fn when_the_upgrade_must_not_run() {
    // Another device is at it.
    let store = sample();
    let since = now() - Duration::minutes(5);
    store.put(upgrade::LOCK_FILE, &lock_file("d-other", "Living-room PC", since, since + Duration::minutes(30)));
    let err = refused(&store);
    assert!(err.contains("Living-room PC"), "{err}");
    assert!(err.contains(&since.with_timezone(&chrono::Local).format("%H:%M").to_string()), "{err}");

    // Its lock has expired: it stopped.
    let store = sample();
    let since = now() - Duration::hours(2);
    store.put(upgrade::LOCK_FILE, &lock_file("d-other", "Living-room PC", since, since + Duration::minutes(30)));
    assert_eq!(run(&store).unwrap().games, 4);
    assert!(store.peek(upgrade::LOCK_FILE).is_none());
    assert_eq!(v2::access(v2::read_format(&store).unwrap().as_ref()), v2::Access::ReadWrite);

    // This device's own lock, left by a run that broke off: taken over.
    let store = sample();
    store.put(upgrade::LOCK_FILE, &lock_file(DEVICE, DEVICE_NAME, now() - Duration::minutes(1), now() + Duration::minutes(29)));
    assert_eq!(run(&store).unwrap().games, 4);
    assert!(store.peek(upgrade::LOCK_FILE).is_none());

    // Written by a newer Ludonimbus: a newer layout, or a feature this build
    // doesn't know. The message says which version to get.
    let store = sample();
    store.put(v2::FORMAT_FILE, br#"{"layout": 3, "minApp": "2.0.0", "requires": []}"#);
    assert!(refused(&store).contains("2.0.0"));
    let store = sample();
    store.put(v2::FORMAT_FILE, br#"{"layout": 2, "minApp": "1.2.0", "requires": ["enc1"]}"#);
    assert!(refused(&store).contains("1.2.0"));

    // A marker that can't be understood is never taken for the old layout.
    let store = sample();
    store.put(v2::FORMAT_FILE, b"{\"layout\": 2,");
    assert!(refused(&store).contains("format.json"));
}

#[test]
fn a_cloud_without_games_only_gets_format_json() {
    for store in [Memory::new(), {
        // A device of 0.6 connected, nothing uploaded.
        let s = Memory::new();
        s.put("devices/d-01m8bf8vmmdmey8afh42vz71ba.json", br#"{"id": "d-01m8bf8vmmdmey8afh42vz71ba"}"#);
        s
    }] {
        let before = store.paths();
        assert_eq!(run(&store).unwrap(), Report::default());
        assert_eq!(store.take(), Counts { read: 1, list: 1, write: 1, ..Default::default() });
        let mut after = before.clone();
        after.push(v2::FORMAT_FILE.to_string());
        after.sort();
        assert_eq!(store.paths(), after);
        assert_eq!(v2::access(v2::read_format(&store).unwrap().as_ref()), v2::Access::ReadWrite);
    }
}

#[test]
fn folders_without_a_definition_or_without_snapshots() {
    let sample = sample();
    let store = Memory::new();
    // Snapshots, but no game.json.
    for (path, data) in files(&sample).into_iter().filter(|(p, _)| p.starts_with("games/abs-game/snapshots/")) {
        store.put(&path.replace("abs-game", "no-definition"), &data);
    }
    // A game.json and nothing else.
    store.put("games/defined-only/game.json", &sample.peek("games/test-game/game.json").unwrap());
    // Neither: only what other features keep there.
    store.put("games/playtime-only/playtime/d-01m8bf8vmmdmey8afh42vz71ba.json", b"{}");

    let report = run(&store).unwrap();
    assert_eq!(report, Report { already: false, games: 2, merged: 0, snapshots: 2, pins: 0 });
    let records = records(&store);
    assert_eq!(records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["defined-only", "no-definition"]);

    let bare = library::get(&records, "no-definition").unwrap();
    assert_eq!(bare.name, "no-definition");
    assert!(bare.ids.is_empty());
    assert_eq!(bare.lines[0].format, "pc");
    assert_eq!(bare.created_at, Some(old_manifest(&store, "no-definition", expected()["abs-game"][0]["id"].as_str().unwrap()).created_at));
    assert!(store.peek("games/no-definition/saves/main/index.json").is_some());

    // A game with nothing in the cloud yet: a record that says nothing about
    // when it was made, and no index to write.
    let defined = library::get(&records, "defined-only").unwrap();
    assert_eq!(defined.name, "Test Game");
    assert_eq!(defined.created_at, None);
    assert!(store.paths().iter().all(|p| !p.starts_with("games/defined-only/saves/")));
    assert!(store.paths().iter().all(|p| !p.starts_with("games/playtime-only/saves/")));
}

#[test]
fn a_snapshot_0_6_uploads_afterwards_is_a_fork_not_a_replacement() {
    let store = sample();
    run(&store).unwrap();
    let records = records(&store);
    let (_, l) = library::resolve_line(&records, "test-game", "main").unwrap();
    let dirs = folders(l);
    // What a device that had read the line before remembers.
    let mut known = LineCache::default();
    let before = line::load_all(&store, &dirs, &mut known).unwrap();
    let head = before.head().unwrap().clone();

    // A device of layout 2 plays on from the newest version.
    let v2_snapshot = Manifest {
        v: 2,
        id: format!("{:0<26}", "01M9V2"),
        game: "test-game".into(),
        line: "main".into(),
        vv: Vv::next([&head.vv], DEVICE, 0),
        parent: Some(head.id.clone()),
        device_id: DEVICE.into(),
        device_name: DEVICE_NAME.into(),
        platform: "steam".into(),
        created_at: head.created_at + Duration::hours(1),
        trigger: "auto-exit".into(),
        content_hash: "c-v2".into(),
        total_size: 2,
        files: vec![FileRef { path: "install/save/slot1.sav".into(), size: 2, hash: "h-v2".into(), ..Default::default() }],
        app_version: "1.0.0".into(),
        ..Default::default()
    };
    line::write_manifest(&store, &l.path, &v2_snapshot).unwrap();

    // A device still on 0.6 knows nothing of it and uploads its own next
    // version into the old folder, as 0.6 does: the zip, then the .json.
    let old = v2::legacy_dir("test-game");
    let mut meta: Value = serde_json::from_slice(&store.peek(&format!("{old}/{}.json", head.id)).unwrap()).unwrap();
    let id_06 = format!("{:0<26}", "01M9OLD");
    meta["id"] = json!(id_06);
    meta["parent"] = json!(head.id);
    meta["deviceId"] = json!("d-01m8bf8vmmdmey8afh42vz71ba");
    meta["deviceName"] = json!("PC-A");
    meta["createdAt"] = json!(head.created_at + Duration::hours(2));
    store.write(&format!("{old}/{id_06}.zip"), &store.peek(&format!("{old}/{}.zip", head.id)).unwrap()).unwrap();
    store.write(&format!("{old}/{id_06}.json"), &serde_json::to_vec_pretty(&meta).unwrap()).unwrap();

    // Two heads: the user is asked, nothing is silently replaced, though the
    // 0.6 version is the later one by the clock. For a device that has never
    // seen the line as for one that read it before.
    for mut cache in [LineCache::default(), known] {
        let view = line::load_all(&store, &dirs, &mut cache).unwrap();
        assert!(view.is_forked());
        let heads: BTreeSet<&str> = view.head_entries().map(|s| s.id.as_str()).collect();
        assert_eq!(heads, BTreeSet::from([v2_snapshot.id.as_str(), id_06.as_str()]));
        let (a, b) = (view.get(&v2_snapshot.id).unwrap(), view.get(&id_06).unwrap());
        assert!(!a.vv.contains(&b.vv) && !b.vv.contains(&a.vv));
        assert_eq!(view.place(&id_06), Some(1));
        assert_eq!(view.place(&v2_snapshot.id), Some(0));
    }
}

#[test]
fn a_whole_run_costs_what_the_table_says() {
    let store = sample();
    store.take();
    run(&store).unwrap();
    let c = store.take();
    assert_eq!(c, sample_budget());
    // Written out for the sample: 4 games, 16 snapshots, 2 pins in one game.
    assert_eq!(c, Counts { list: 10, read: 28, write: 12, remove: 1, ..Default::default() });

    // A device that used 0.6 has read the snapshots before: through a
    // `Cached` store none of them is read again.
    let store = sample();
    let tmp = tempfile::tempdir().unwrap();
    {
        let earlier = Cached::at(Flaky::new(&store), tmp.path());
        for key in keys() {
            line::load(&earlier, &v2::legacy_dir(&key), &mut LineCache::default()).unwrap();
        }
    }
    let cached = Cached::at(Flaky::new(&store), tmp.path());
    store.take();
    run(&cached).unwrap();
    let mut wanted = sample_budget();
    wanted.read -= 16;
    assert_eq!(store.take(), wanted);
    assert_eq!(state(&store, CLOCK), {
        let plain = sample();
        run(&plain).unwrap();
        state(&plain, CLOCK)
    });
}
