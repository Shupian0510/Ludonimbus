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
    assert_eq!(head.vv, Vv::legacy(2));

    // The first snapshot of the new kind is made from it and becomes the
    // newest.
    let next = manifest(4, "d-b", Vv::next([&head.vv], "d-b", 0));
    line::write_manifest(&store, DIR, &next).unwrap();
    let (view, _) = read(&store, &mut cache);
    assert_eq!(view.head().unwrap().id, id(4));
}
