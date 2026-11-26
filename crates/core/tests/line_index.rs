//! Reading a save line of cloud layout 2: what is found, and how many
//! requests it takes (docs/plan-v2.md, sections 3.8 and 5.6).

use chrono::{TimeZone, Utc};
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::v2::{self, FileRef, Manifest, Pin};
use ludonimbus_core::vv::Vv;

const DIR: &str = "games/stardew-7k3f/saves/main";

fn id(n: u64) -> String {
    format!("01JTEST{n:019}")
}

fn manifest(n: u64, device: &str, vv: Vv) -> Manifest {
    Manifest {
        v: 2,
        id: id(n),
        game: "stardew-7k3f".into(),
        line: "main".into(),
        vv,
        device_id: device.into(),
        device_name: device.to_uppercase(),
        platform: "steam".into(),
        created_at: Utc.timestamp_opt(1_780_000_000 + n as i64 * 60, 0).unwrap(),
        trigger: "auto-exit".into(),
        content_hash: format!("c{n}"),
        total_size: 10,
        files: vec![FileRef { path: "saves/a".into(), size: 10, hash: format!("h{n}"), ..Default::default() }],
        app_version: "1.0.0".into(),
        ..Default::default()
    }
}

/// A device that syncs: writes snapshots and keeps the index up to date.
struct Writer<'a> {
    store: &'a Memory,
    cache: LineCache,
    last: Vv,
    next: u64,
}

impl<'a> Writer<'a> {
    fn new(store: &'a Memory) -> Self {
        Writer { store, cache: LineCache::default(), last: Vv::new(), next: 1 }
    }

    /// `count` snapshots, each made from the one before, by two devices in turn.
    fn play(&mut self, count: u64, index: bool) {
        for _ in 0..count {
            let device = if self.next % 2 == 0 { "d-a" } else { "d-b" };
            let m = manifest(self.next, device, Vv::next([&self.last], device, 0));
            line::write_manifest(self.store, DIR, &m).unwrap();
            self.cache.insert(&m);
            self.last = m.vv.clone();
            self.next += 1;
        }
        if index {
            self.index();
        }
    }

    fn index(&mut self) {
        // Reading first picks up pins written since.
        line::load(self.store, DIR, &mut self.cache).unwrap();
        line::write_index(self.store, DIR, &self.cache, "d-a").unwrap();
    }
}

fn read(store: &Memory, cache: &mut LineCache) -> (LineView, Counts) {
    store.take();
    let view = line::load(store, DIR, cache).unwrap();
    (view, store.take())
}

fn requests(c: Counts) -> (u32, u32) {
    assert_eq!(c.total(), c.list + c.read, "reading a line only lists and reads");
    (c.list, c.read)
}

#[test]
fn a_long_history_costs_two_requests() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(50, true);

    // A device that has never seen this line.
    let mut cache = LineCache::default();
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(view.snapshots.len(), 50);
    assert_eq!(view.head().unwrap().id, id(50));
    assert!(!view.needs_index && view.unreadable.is_empty());
    assert_eq!(view.snapshots[0].id, id(1));
    assert_eq!(view.snapshots[0].packs, vec![id(1)]);

    // Nothing new: the listing is enough.
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(view.head().unwrap().id, id(50));

    // Twenty more versions cost the same as one.
    w.play(20, true);
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(view.snapshots.len(), 70);
    assert_eq!(view.head().unwrap().id, id(70));
    assert!(!view.needs_index);
}

#[test]
fn what_the_index_lacks_is_read_from_the_manifests() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(10, true);
    let mut cache = LineCache::default();
    read(&store, &mut cache);

    // A device wrote a snapshot and was cut off before updating the index.
    w.play(1, false);
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 1), "only the one new manifest");
    assert_eq!(view.head().unwrap().id, id(11));
    assert!(view.needs_index);

    // A new device: the index, then the one manifest it lacks.
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 2));
    assert_eq!(view.snapshots.len(), 11);
    assert_eq!(view.head().unwrap().id, id(11));
    assert!(view.needs_index);

    // Once somebody rewrites the index, it is two requests again.
    w.index();
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 1));
    assert!(!view.needs_index);
}

#[test]
fn the_listing_decides_what_exists() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(10, true);
    let mut seen = LineCache::default();
    read(&store, &mut seen);

    // Cleaned up by another device; the index still names it.
    store.remove(&v2::manifest_path(DIR, &id(3))).unwrap();
    for cache in [&mut seen, &mut LineCache::default()] {
        let (view, _) = read(&store, cache);
        assert_eq!(view.snapshots.len(), 9);
        assert!(view.get(&id(3)).is_none());
        assert_eq!(view.head().unwrap().id, id(10));
    }

    // Files of kinds this version doesn't know are never read.
    store.put(&format!("{DIR}/notes.txt"), b"hello");
    store.put(&format!("{DIR}/{}.thumb.json", id(4)), b"{}");
    store.put(&format!("{DIR}/summary.json"), b"{}");
    store.put(&format!("{DIR}/trash/{}.json", id(3)), b"{}");
    let (view, c) = read(&store, &mut seen);
    assert_eq!(requests(c), (1, 0));
    assert_eq!(view.snapshots.len(), 9);
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 1));
    assert_eq!(view.snapshots.len(), 9);
}

#[test]
fn a_lost_or_damaged_index_changes_nothing_but_the_cost() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(12, true);
    let (good, _) = read(&store, &mut LineCache::default());

    store.put(&v2::index_path(DIR), b"{ \"snapshots\": [ {");
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 13), "the index, then every manifest");
    assert_eq!(view.snapshots, good.snapshots);
    assert_eq!(view.heads, good.heads);
    assert!(view.needs_index);

    store.remove(&v2::index_path(DIR)).unwrap();
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 12));
    assert_eq!(view.snapshots, good.snapshots);
    assert!(view.needs_index);

    // A manifest that can't be understood is reported, the rest still works.
    store.put(&v2::manifest_path(DIR, &id(99)), b"not json");
    let (view, _) = read(&store, &mut LineCache::default());
    assert_eq!(view.unreadable, vec![id(99)]);
    assert_eq!(view.snapshots, good.snapshots);

    // An empty folder is an empty line, and needs no index.
    let empty = line::load(&store, "games/none/saves/main", &mut LineCache::default()).unwrap();
    assert!(empty.snapshots.is_empty() && empty.heads.is_empty() && !empty.needs_index);
}

#[test]
fn kept_saves_come_with_the_index() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(8, true);
    let pin = Pin { name: "Before the boss".into(), device_id: "d-a".into(), ..Default::default() };
    line::write_pin(&store, DIR, &id(5), &pin).unwrap();
    w.index();

    // The name arrives without reading the pin file.
    let mut cache = LineCache::default();
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(view.get(&id(5)).unwrap().pin.as_ref().unwrap().name, "Before the boss");
    assert!(view.get(&id(4)).unwrap().pin.is_none());
    assert!(!view.needs_index);

    // Renamed on another device, index not rewritten yet: one small read.
    line::write_pin(&store, DIR, &id(5), &Pin { name: "Chapter 3".into(), ..Default::default() }).unwrap();
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 1));
    assert_eq!(view.get(&id(5)).unwrap().pin.as_ref().unwrap().name, "Chapter 3");
    assert!(view.needs_index);
    let (_, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));

    // A new device with the index still naming the old pin: the old name is
    // not used.
    let (view, c) = read(&store, &mut LineCache::default());
    assert_eq!(requests(c), (1, 2));
    assert_eq!(view.get(&id(5)).unwrap().pin.as_ref().unwrap().name, "Chapter 3");

    // No longer kept: the listing says so.
    line::remove_pin(&store, DIR, &id(5)).unwrap();
    let (view, c) = read(&store, &mut cache);
    assert_eq!(requests(c), (1, 0));
    assert!(view.get(&id(5)).unwrap().pin.is_none());
    let (view, _) = read(&store, &mut LineCache::default());
    assert!(view.get(&id(5)).unwrap().pin.is_none());

    // A pin whose file is damaged still keeps its snapshot.
    store.put(&v2::pin_path(DIR, &id(2)), b"???");
    let (view, _) = read(&store, &mut cache);
    assert_eq!(view.get(&id(2)).unwrap().pin.as_ref().unwrap().name, "");
}

#[test]
fn playing_offline_on_two_devices_shows_as_a_fork() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(4, true);
    let base = w.last.clone();

    let a = manifest(5, "d-a", Vv::next([&base], "d-a", 0));
    let b = manifest(6, "d-b", Vv::next([&base], "d-b", 0));
    line::write_manifest(&store, DIR, &a).unwrap();
    line::write_manifest(&store, DIR, &b).unwrap();
    let mut cache = LineCache::default();
    let (view, _) = read(&store, &mut cache);
    assert!(view.is_forked());
    assert!(view.head().is_none());
    let heads: Vec<&str> = view.head_entries().map(|s| s.id.as_str()).collect();
    assert_eq!(heads, [id(5), id(6)]);

    // The user picks one side; the result contains both.
    let settled = manifest(7, "d-a", Vv::next([&a.vv, &b.vv], "d-a", 0));
    line::write_manifest(&store, DIR, &settled).unwrap();
    let (view, _) = read(&store, &mut cache);
    assert!(!view.is_forked());
    assert_eq!(view.head().unwrap().id, id(7));

    // Cleaning up the middle of the history doesn't change the answer.
    for n in 2..=6 {
        store.remove(&v2::manifest_path(DIR, &id(n))).unwrap();
    }
    let (view, _) = read(&store, &mut cache);
    assert_eq!(view.snapshots.len(), 2);
    assert_eq!(view.head().unwrap().id, id(7));
}

#[test]
fn saves_kept_on_the_side_are_never_the_newest() {
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(3, false);

    // Made before a restore: later than everything, and beside the history.
    let mut before = manifest(4, "d-a", w.last.clone());
    before.trigger = "pre-restore".into();
    before.side = true;
    line::write_manifest(&store, DIR, &before).unwrap();
    let (view, _) = read(&store, &mut LineCache::default());
    assert_eq!(view.snapshots.len(), 4);
    assert_eq!(view.head().unwrap().id, id(3));
}

#[test]
fn snapshots_of_the_old_layout_keep_their_order() {
    let store = Memory::new();
    // As 0.6 wrote them: no vector, and the losing side of a conflict is
    // told by its trigger alone.
    for n in 1..=3 {
        let mut m = manifest(n, "d-a", Vv::new());
        m.v = 1;
        if n == 2 {
            m.trigger = "conflict-keep".into();
        }
        store.put(&v2::manifest_path(DIR, &id(n)), &serde_json::to_vec(&m).unwrap());
    }
    let mut cache = LineCache::default();
    let (view, _) = read(&store, &mut cache);
    let head = view.head().unwrap().clone();
    assert_eq!(head.id, id(3));
    assert_eq!(head.vv, line::legacy_vv(DIR, head.created_at));

    // The first snapshot of the new kind is made from it and becomes the
    // newest.
    let next = manifest(4, "d-b", Vv::next([&head.vv], "d-b", 0));
    line::write_manifest(&store, DIR, &next).unwrap();
    let (view, _) = read(&store, &mut cache);
    assert_eq!(view.head().unwrap().id, id(4));
}

const OLD: &str = "games/stardew/snapshots";

fn read_both(store: &Memory, cache: &mut LineCache) -> (LineView, Counts) {
    store.take();
    let view = line::load_all(store, &[DIR, OLD], cache).unwrap();
    (view, store.take())
}

/// Three snapshots as 0.6 left them in the folder from before the migration.
fn old_folder(store: &Memory) {
    for n in 1..=3 {
        let mut m = manifest(n, "d-a", Vv::new());
        m.v = 1;
        m.line = String::new();
        store.put(&v2::manifest_path(OLD, &id(n)), &serde_json::to_vec(&m).unwrap());
    }
}

#[test]
fn a_migrated_line_reads_the_old_folder_too() {
    let store = Memory::new();
    old_folder(&store);

    // Right after the migration nothing has been written to the new folder.
    let mut cache = LineCache::default();
    let (view, c) = read_both(&store, &mut cache);
    assert_eq!(requests(c), (2, 3));
    assert_eq!(view.snapshots.len(), 3);
    assert_eq!(view.places, [1, 1, 1]);
    let head = view.head().unwrap().clone();
    assert_eq!((head.id.as_str(), &head.vv), (id(3).as_str(), &line::legacy_vv(OLD, head.created_at)));
    assert!(view.needs_index);

    // The first snapshot of the new kind goes to the new folder, built on
    // the newest old one; the index there covers both folders.
    let next = manifest(4, "d-b", Vv::next([&head.vv], "d-b", 0));
    line::write_manifest(&store, DIR, &next).unwrap();
    cache.insert(&next);
    line::write_index(&store, DIR, &cache, "d-b").unwrap();
    let (view, c) = read_both(&store, &mut cache);
    assert_eq!(requests(c), (2, 0), "one listing per folder, nothing else");
    assert_eq!(view.head().unwrap().id, id(4));
    assert_eq!(view.place(&id(4)), Some(0));
    assert_eq!(view.place(&id(2)), Some(1));
    assert!(!view.needs_index);

    // Another device: both listings and the one index.
    let (view, c) = read_both(&store, &mut LineCache::default());
    assert_eq!(requests(c), (2, 1));
    assert_eq!(view.snapshots.len(), 4);
    assert_eq!(view.head().unwrap().id, id(4));

    // A device still on 0.6 keeps writing to the old folder: its snapshot
    // doesn't contain the new one and the new one doesn't contain it.
    let mut late = manifest(5, "d-a", Vv::new());
    late.v = 1;
    store.put(&v2::manifest_path(OLD, &id(5)), &serde_json::to_vec(&late).unwrap());
    let (view, c) = read_both(&store, &mut cache);
    // Its manifest, and the index once more: this device wrote the index
    // itself and only learns its tag from a listing.
    assert_eq!(requests(c), (2, 2));
    assert!(view.is_forked());
    let heads: Vec<&str> = view.head_entries().map(|s| s.id.as_str()).collect();
    assert_eq!(heads, [id(4), id(5)]);
    let (_, c) = read_both(&store, &mut cache);
    assert_eq!(requests(c), (2, 0));
}

#[test]
fn a_kept_save_in_the_old_folder_is_named_in_the_new_one() {
    let store = Memory::new();
    old_folder(&store);
    // The migration writes pins beside the new manifests; the old folder is
    // left as it was.
    line::write_pin(&store, DIR, &id(2), &Pin { name: "Before the boss".into(), ..Default::default() }).unwrap();
    let mut cache = LineCache::default();
    let (view, _) = read_both(&store, &mut cache);
    assert_eq!(view.get(&id(2)).unwrap().pin.as_ref().unwrap().name, "Before the boss");
    assert!(view.get(&id(1)).unwrap().pin.is_none());

    // A pin for a snapshot that exists nowhere is ignored.
    line::write_pin(&store, DIR, &id(9), &Pin::default()).unwrap();
    let (view, _) = read_both(&store, &mut cache);
    assert_eq!(view.snapshots.len(), 3);
}

#[test]
fn a_merged_line_brings_its_own_index() {
    const OTHER: &str = "games/stardew-x2/saves/main";
    let store = Memory::new();
    let mut w = Writer::new(&store);
    w.play(4, true);
    // The line that was merged in: 30 snapshots and an index of its own.
    let mut other = LineCache::default();
    let mut last = Vv::new();
    for n in 101..=130 {
        let m = manifest(n, "d-c", Vv::next([&last], "d-c", 0));
        line::write_manifest(&store, OTHER, &m).unwrap();
        other.insert(&m);
        last = m.vv.clone();
    }
    line::write_index(&store, OTHER, &other, "d-c").unwrap();

    // The first read after the merge: two listings, two indexes, no
    // manifest one by one.
    let mut cache = LineCache::default();
    store.take();
    let view = line::load_all(&store, &[DIR, OTHER], &mut cache).unwrap();
    assert_eq!(requests(store.take()), (2, 2));
    assert_eq!(view.snapshots.len(), 34);
    assert!(view.is_forked(), "both sides have progress: an ordinary fork");
    assert!(view.needs_index, "the line's own index doesn't cover the merged folder yet");

    // Once it does, a new device needs only that one.
    line::write_index(&store, DIR, &cache, "d-a").unwrap();
    store.take();
    let view = line::load_all(&store, &[DIR, OTHER], &mut LineCache::default()).unwrap();
    assert_eq!(requests(store.take()), (2, 1));
    assert_eq!(view.snapshots.len(), 34);
    assert!(!view.needs_index);

    // And nothing new costs the two listings.
    store.take();
    line::load_all(&store, &[DIR, OTHER], &mut cache).unwrap();
    assert_eq!(requests(store.take()), (2, 0));
}

/// Two games of 0.6 that turned out to be one: their folders were never one
/// history, so neither side's newest version contains the other's.
#[test]
fn two_old_folders_in_one_line_are_a_fork() {
    const OTHER: &str = "games/stardew-2/snapshots";
    let store = Memory::new();
    old_folder(&store);
    let mut m = manifest(7, "d-b", Vv::new());
    m.v = 1;
    m.line = String::new();
    store.put(&v2::manifest_path(OTHER, &id(7)), &serde_json::to_vec(&m).unwrap());

    let dirs = [DIR, OLD, OTHER];
    let view = line::load_all(&store, &dirs, &mut LineCache::default()).unwrap();
    let heads: Vec<&str> = view.head_entries().map(|s| s.id.as_str()).collect();
    assert_eq!(heads, [id(3), id(7)]);
    assert_eq!(view.get(&id(7)).unwrap().vv, line::legacy_vv(OTHER, m.created_at));
    // Each folder on its own still has one newest version.
    assert_eq!(line::load(&store, OLD, &mut LineCache::default()).unwrap().head().unwrap().id, id(3));

    // The snapshot that settles it is built on both, and is the newest.
    let both: Vec<Vv> = view.head_entries().map(|s| s.vv.clone()).collect();
    let settled = manifest(8, "d-a", Vv::next(&both, "d-a", 0));
    line::write_manifest(&store, DIR, &settled).unwrap();
    let view = line::load_all(&store, &dirs, &mut LineCache::default()).unwrap();
    assert_eq!(view.head().unwrap().id, id(8));
}
