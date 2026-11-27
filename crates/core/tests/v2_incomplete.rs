//! Versions that miss a content pack (docs/plan-v2.md, section 3.4, "引用的包
//! 被删了怎么办"): the mark, finding them in the clean-up without extra
//! requests, putting a pack back from the saves on a device, from zips kept
//! on it and from another pack in the cloud, making a waiting version whole
//! before it goes up, and letting go of what nobody can repair.

use chrono::{DateTime, Duration, Utc};
use ludonimbus_core::cloud::decide::{decide, Action, Base, Local};
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::packs::{self, LocalFile, Policy};
use ludonimbus_core::cloud::retention;
use ludonimbus_core::cloud::saves::{self, Draft, Open, Place, Repair, Repairs, Sources};
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::v2::{self, GameRecord, Line, Manifest, Mark};
use ludonimbus_core::vv::Vv;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;
use tempfile::TempDir;

const GAME: &str = "stardew-7k3f";

/// Kilobytes where the real policy has megabytes: the rules are the same.
const POLICY: Policy = Policy { full_below: 4 * 1024, split_above: 64 * 1024 };

fn start() -> DateTime<Utc> {
    static START: OnceLock<DateTime<Utc>> = OnceLock::new();
    *START.get_or_init(|| Utc::now() - Duration::hours(2))
}

/// A moment later than every one handed out before.
fn tick() -> DateTime<Utc> {
    static TICKS: AtomicI64 = AtomicI64::new(0);
    start() + Duration::seconds(TICKS.fetch_add(1, Ordering::SeqCst))
}

fn place() -> Place {
    let r = GameRecord {
        id: GAME.into(),
        name: "Stardew Valley".into(),
        folders: vec![GAME.into()],
        lines: vec![Line { id: "main".into(), format: "pc".into(), scope: v2::SCOPE_SHARED.into(), path: v2::line_dir(GAME, "main"), ..Default::default() }],
        ..Default::default()
    };
    Place::of(&r, &r.lines[0])
}

fn bytes(seed: usize, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 17) % 251) as u8).collect()
}

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel: Vec<String> = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn counts(list: u32, read: u32, download: u32, write: u32, upload: u32, remove: u32) -> Counts {
    Counts { list, read, download, write, upload, remove }
}

/// One device: its saves, the line as it read it last, a folder for what
/// waits for the cloud.
struct Device {
    tmp: TempDir,
    id: &'static str,
    cache: LineCache,
    counter: u64,
}

impl Device {
    fn new(id: &'static str) -> Self {
        Device { tmp: tempfile::tempdir().unwrap(), id, cache: LineCache::default(), counter: 0 }
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().join("local")
    }

    fn queue(&self) -> PathBuf {
        self.tmp.path().join("queue")
    }

    fn staging(&self) -> PathBuf {
        self.tmp.path().join("staging")
    }

    fn save(&self, path: &str, data: &[u8]) {
        let p = self.root().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    /// Six files of 1 KiB: larger than `POLICY.full_below`, so a version
    /// built on another takes the unchanged files from its packs.
    fn large_save(&self, version: usize) {
        for n in 0..6 {
            self.save(&format!("saves/world/region{n}"), &bytes(n + 10 * version, 1024));
        }
    }

    fn files(&self) -> Vec<LocalFile> {
        let root = self.root();
        tree(&root).keys().map(|p| LocalFile::from_disk(p.clone(), root.join(p)).unwrap()).collect()
    }

    fn open(&mut self, store: &dyn Store, place: &Place) -> LineView {
        saves::open(store, place, &mut self.cache).unwrap()
    }

    /// A new version of the saves here, built on `base` (taking its unchanged
    /// files from its packs) or complete without one; staged, not uploaded.
    fn make(&mut self, store: &dyn Store, place: &Place, base: Option<&Manifest>) -> Manifest {
        let view = self.open(store, place);
        let bases: Vec<Vv> = view.head_entries().map(|h| h.vv.clone()).collect();
        let vv = ludonimbus_core::cloud::decide::next_vv(&bases, &view, self.id, self.counter);
        self.counter = vv.get(self.id);
        let d = Draft {
            vv,
            parent: base.map(|b| b.id.clone()),
            trigger: "auto-exit".into(),
            platform: "steam".into(),
            device_id: self.id.into(),
            device_name: self.id.into(),
            created_at: tick(),
            ..Default::default()
        };
        let m = saves::draft(place, &d, &self.files(), base, &POLICY);
        saves::stage(&m, &self.files(), &self.queue()).unwrap();
        m
    }

    /// [`Device::make`], then uploaded.
    fn push(&mut self, store: &dyn Store, place: &Place, base: Option<&Manifest>) -> Manifest {
        let m = self.make(store, place, base);
        self.open(store, place);
        let (queue, files) = (self.queue(), self.files());
        saves::publish(store, place, &mut self.cache, &m, &queue, &files, self.id).unwrap();
        saves::unstage(&queue, &m).unwrap();
        m
    }

    fn fetch(&self, store: &dyn Store, place: &Place, m: &Manifest, name: &str) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
        let view = saves::open(store, place, &mut LineCache::default())?;
        let dest = self.tmp.path().join(name);
        saves::fetch(store, place, &view, m, None, &dest, &self.staging())?;
        Ok(tree(&dest))
    }
}

/// Nothing refers to a pack that isn't there.
fn assert_complete(store: &Memory, place: &Place) {
    let paths: BTreeSet<String> = store.paths().into_iter().collect();
    for p in paths.iter().filter(|p| p.starts_with(&format!("{}/", place.dir()))) {
        let Some(id) = p[place.dir().len() + 1..].strip_suffix(".json") else { continue };
        if !v2::is_snapshot_id(id) {
            continue;
        }
        let m: Manifest = serde_json::from_slice(&store.peek(p).unwrap()).unwrap();
        for pack in m.packs() {
            assert!(paths.contains(&v2::pack_path(GAME, &pack)), "{p} refers to {pack}, which is gone");
        }
    }
}

/// Every pack `m` names is there.
fn assert_whole(store: &Memory, m: &Manifest) {
    for pack in m.packs() {
        assert!(store.peek(&v2::pack_path(GAME, &pack)).is_some(), "{} refers to {pack}, which is gone", m.id);
    }
}

fn marks(store: &Memory) -> BTreeSet<String> {
    store.paths().iter().filter_map(|p| p.rsplit('/').next()?.strip_suffix(v2::MARK_SUFFIX).map(str::to_string)).collect()
}

/// A large save uploaded complete (`v1`), then a version built on it
/// (`v2`): `v2` takes all but one file from `v1`'s pack.
fn two_versions(store: &Memory, a: &mut Device, place: &Place) -> (Manifest, Manifest) {
    a.large_save(1);
    let v1 = a.push(store, place, None);
    a.save("saves/world/region0", &bytes(99, 1024));
    let v2 = a.push(store, place, Some(&v1));
    assert!(v2.packs().contains(&v1.id), "the second takes files from the first one's pack");
    (v1, v2)
}

fn clean(store: &Memory, place: &Place, cache: &mut LineCache, device: &str, repairs: Option<Repairs<'_>>) -> saves::Cleaned {
    let mut lines = [Open { place, cache, bases: vec![] }];
    saves::clean_with(store, &mut lines, &retention::Policy::default(), Duration::days(7), Utc::now(), device, repairs).unwrap()
}

#[test]
fn a_mark_holds_a_version_back_and_costs_one_read_to_learn() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    let (v1, v2) = two_versions(&store, &mut a, &place);
    let mut b = Device::new("d-b");
    b.open(&store, &place);

    let view = a.open(&store, &place);
    saves::mark(&store, &place, &mut a.cache, &v2.id, std::slice::from_ref(&v1.id), "d-a", Utc::now()).unwrap();
    assert!(a.cache.snapshots[&v2.id].is_incomplete());
    assert_eq!(view.head().unwrap().id, v2.id);

    // Another device learns of it from the listing it makes anyway, and
    // reads the mark once.
    store.take();
    let seen = b.open(&store, &place);
    assert_eq!(store.take(), counts(1, 1, 0, 0, 0, 0));
    let marked = seen.get(&v2.id).unwrap();
    assert_eq!(marked.incomplete.as_ref().unwrap().missing, std::slice::from_ref(&v1.id));
    assert!(seen.newest_is_incomplete());
    b.open(&store, &place);
    assert_eq!(store.take(), counts(1, 0, 0, 0, 0, 0), "and then it is quiet again");

    // It is no newest version for any decision: a device at the first one
    // stays there, one with nothing gets the first one.
    assert_eq!(seen.usable().head().unwrap().id, v1.id);
    let at_v1 =
        Local { content_hash: Some(v1.content_hash.clone()), base: Some(Base { id: v1.id.clone(), vv: v1.vv.clone(), content_hash: v1.content_hash.clone() }) };
    assert_eq!(decide(&at_v1, &seen), Action::Nothing);
    assert_eq!(decide(&Local::default(), &seen), Action::Pull { id: v1.id.clone() });
    // The device whose saves grew from it builds on it: its new version
    // contains it, and nothing is asked.
    let grew = Local { content_hash: Some("newer".into()), base: Some(Base { id: v2.id.clone(), vv: v2.vv.clone(), content_hash: v2.content_hash.clone() }) };
    match decide(&grew, &seen) {
        Action::Push { bases, .. } => assert!(bases.contains(&v2.vv)),
        other => panic!("expected a push, got {other:?}"),
    }

    // Taken away, the version counts again.
    saves::unmark(&store, &place, &mut b.cache, &v2.id).unwrap();
    let seen = b.open(&store, &place);
    assert!(!seen.get(&v2.id).unwrap().is_incomplete());
    assert_eq!(seen.usable().head().unwrap().id, v2.id);
}

#[test]
fn clean_up_finds_a_missing_pack_with_no_extra_request() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    let (v1, v2) = two_versions(&store, &mut a, &place);
    a.open(&store, &place);

    // Nothing missing: the line, its "recently deleted", the game's packs.
    store.take();
    let cleaned = clean(&store, &place, &mut a.cache, "d-x", None);
    assert_eq!(cleaned, saves::Cleaned::default());
    assert_eq!(store.take(), counts(3, 0, 0, 0, 0, 0));

    // A pack gone (deleted by hand, by an older build): the same three
    // listings find it; the marks of the two versions that use it are the
    // only requests more.
    store.remove(&v2::pack_path(GAME, &v1.id)).unwrap();
    store.take();
    let cleaned = clean(&store, &place, &mut a.cache, "d-x", None);
    assert_eq!(store.take(), counts(3, 0, 0, 2, 0, 0));
    assert_eq!(cleaned.marked.iter().collect::<BTreeSet<_>>(), BTreeSet::from([&v1.id, &v2.id]));
    assert_eq!(marks(&store), BTreeSet::from([v1.id.clone(), v2.id.clone()]));
    assert!(cleaned.retired.is_empty(), "marked today: nothing is let go yet");
}

#[test]
fn a_pack_is_put_back_from_the_saves_on_a_device() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    let (v1, v2) = two_versions(&store, &mut a, &place);
    store.remove(&v2::pack_path(GAME, &v1.id)).unwrap();
    let mut b = Device::new("d-b");
    assert!(packs::missing_pack(&b.fetch(&store, &place, &v2, "early").unwrap_err()).is_some());

    // A device that still has the first version's files: its clean-up puts
    // the pack back, whole for both versions.
    let mut c = Device::new("d-c");
    c.large_save(1);
    c.open(&store, &place);
    let local = c.files();
    let staging = c.staging();
    let read = || local.clone();
    let repairs = Repairs { local: &read, zips: &[], staging: &staging, device_name: "C" };
    let cleaned = clean(&store, &place, &mut c.cache, "d-c", Some(repairs));
    assert_eq!(cleaned.repaired.iter().collect::<BTreeSet<_>>(), BTreeSet::from([&v1.id, &v2.id]));
    assert!(cleaned.marked.is_empty());
    assert_complete(&store, &place);
    assert_eq!(b.fetch(&store, &place, &v2, "after").unwrap(), tree(&a.root()));
    b.open(&store, &place);
}

#[test]
fn a_pack_is_put_back_from_a_zip_kept_on_a_device() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    let (v1, v2) = two_versions(&store, &mut a, &place);
    // The first version's pack, as a copy kept on this device.
    let kept = a.tmp.path().join("local-backups");
    std::fs::create_dir_all(&kept).unwrap();
    let zip = kept.join(format!("{}.zip", v1.id));
    std::fs::write(&zip, store.peek(&v2::pack_path(GAME, &v1.id)).unwrap()).unwrap();
    store.remove(&v2::pack_path(GAME, &v1.id)).unwrap();
    // The saves here have moved on: they hold none of it.
    a.large_save(7);

    let view = a.open(&store, &place);
    let zips = [zip];
    let sources = Sources { local: &a.files(), zips: &zips };
    let lines = [(&place, &view)];
    let repaired = saves::put_back(&store, &lines, &place, &v2, None, sources, &a.staging(), "d-a", Utc::now()).unwrap();
    assert_eq!(repaired, Repair::Repaired(vec![v1.id.clone()]));
    assert_complete(&store, &place);
    let b = Device::new("d-b");
    b.fetch(&store, &place, &v1, "v1").unwrap();
    b.fetch(&store, &place, &v2, "v2").unwrap();
}

#[test]
fn a_pack_is_put_back_from_another_pack_in_the_cloud() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    // The same saves uploaded complete twice (two devices that each started
    // from nothing), then a version built on the second.
    a.large_save(1);
    let v1 = a.push(&store, &place, None);
    let v2 = a.push(&store, &place, None);
    a.save("saves/world/region5", &bytes(55, 1024));
    let v3 = a.push(&store, &place, Some(&v2));
    assert!(v3.packs().contains(&v2.id));
    store.remove(&v2::pack_path(GAME, &v2.id)).unwrap();
    a.large_save(8);

    // Nothing on this device has it any more; the first version's pack
    // does. Downloaded, and the pack put back from it.
    let view = a.open(&store, &place);
    let lines = [(&place, &view)];
    let sources = Sources { local: &a.files(), zips: &[] };
    store.take();
    let repaired = saves::put_back(&store, &lines, &place, &v3, None, sources, &a.staging(), "d-a", Utc::now()).unwrap();
    assert_eq!(repaired, Repair::Repaired(vec![v2.id.clone()]));
    let c = store.take();
    assert_eq!((c.download, c.upload), (1, 1), "{c:?}");
    assert_complete(&store, &place);
    let b = Device::new("d-b");
    assert_eq!(b.fetch(&store, &place, &v2, "v2").unwrap(), b.fetch(&store, &place, &v1, "v1").unwrap());
    b.fetch(&store, &place, &v3, "v3").unwrap();
}

#[test]
fn a_waiting_version_takes_what_it_misses_from_elsewhere_before_it_goes_up() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    a.large_save(1);
    let v1 = a.push(&store, &place, None);
    let v2 = a.push(&store, &place, None);
    a.save("saves/world/region5", &bytes(55, 1024));
    let waiting = a.make(&store, &place, Some(&v2));
    assert!(waiting.packs().contains(&v2.id));
    store.remove(&v2::pack_path(GAME, &v2.id)).unwrap();

    // Files found with the same path in a pack that is there are taken from
    // it: nothing is uploaded for them.
    let view = a.open(&store, &place);
    let lines = [(&place, &view)];
    let files = a.files();
    let sources = Sources { local: &files, zips: &[] };
    let whole = saves::repair_queued(&store, &lines, &place, &waiting, sources, &a.queue(), &a.staging()).unwrap().unwrap();
    assert!(!whole.packs().contains(&v2.id));
    assert!(whole.packs().contains(&v1.id));
    assert_eq!(whole.id, waiting.id);
    assert_eq!(whole.content_hash, waiting.content_hash);
    let (queue, id) = (a.queue(), a.id);
    store.take();
    saves::publish(&store, &place, &mut a.cache, &whole, &queue, &files, id).unwrap();
    // Its own pack, a look at the packs, manifest and index: what it took
    // from the gone pack went nowhere.
    assert_eq!(store.take(), counts(1, 0, 0, 2, 1, 0));
    assert_whole(&store, &whole);
    let b = Device::new("d-b");
    assert_eq!(b.fetch(&store, &place, &whole, "w").unwrap(), tree(&a.root()));

    // Nothing in the cloud has them: from the saves here, into a pack of its
    // own built beside the others.
    let mut a = Device::new("d-a2");
    let store = Memory::new();
    a.large_save(2);
    let base = a.push(&store, &place, None);
    a.save("saves/world/region5", &bytes(66, 1024));
    let waiting = a.make(&store, &place, Some(&base));
    store.remove(&v2::pack_path(GAME, &base.id)).unwrap();
    let view = a.open(&store, &place);
    let lines = [(&place, &view)];
    let files = a.files();
    let sources = Sources { local: &files, zips: &[] };
    let whole = saves::repair_queued(&store, &lines, &place, &waiting, sources, &a.queue(), &a.staging()).unwrap().unwrap();
    assert_eq!(whole.packs(), BTreeSet::from([waiting.id.clone(), format!("{}-2", waiting.id)]));
    assert!(a.queue().join(format!("{}-2.zip", waiting.id)).is_file());
    let (queue, id) = (a.queue(), a.id);
    saves::publish(&store, &place, &mut a.cache, &whole, &queue, &files, id).unwrap();
    assert_whole(&store, &whole);
    assert_eq!(Device::new("d-b").fetch(&store, &place, &whole, "w").unwrap(), tree(&a.root()));

    // Found nowhere: a missing pack, and nothing changes.
    let mut a = Device::new("d-a3");
    let store = Memory::new();
    a.large_save(3);
    let base = a.push(&store, &place, None);
    a.save("saves/world/region5", &bytes(77, 1024));
    let waiting = a.make(&store, &place, Some(&base));
    store.remove(&v2::pack_path(GAME, &base.id)).unwrap();
    a.large_save(4);
    let view = a.open(&store, &place);
    let lines = [(&place, &view)];
    let files = a.files();
    let err = saves::repair_queued(&store, &lines, &place, &waiting, Sources { local: &files, zips: &[] }, &a.queue(), &a.staging()).unwrap_err();
    assert_eq!(packs::missing_pack(&err).unwrap().pack, base.id);
}

/// Mark `id` as found missing `days` ago.
fn marked_since(store: &Memory, place: &Place, id: &str, pack: &str, days: i64) {
    let mark = Mark { missing: vec![pack.to_string()], since: Some(Utc::now() - Duration::days(days)), device_id: "d-x".into(), ..Default::default() };
    line::write_mark(store, place.dir(), id, &mark).unwrap();
}

#[test]
fn what_nobody_can_repair_is_let_go_after_fourteen_days() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    a.large_save(1);
    let v1 = a.push(&store, &place, None);
    a.save("saves/world/region0", &bytes(99, 1024));
    let v2 = a.push(&store, &place, Some(&v1));
    // Its own pack is gone; the first version is whole.
    store.remove(&v2::pack_path(GAME, &v2.id)).unwrap();
    let mut b = Device::new("d-b");
    let staging = b.staging();
    let nothing = Vec::new;
    let repairs = Repairs { local: &nothing, zips: &[], staging: &staging, device_name: "B" };

    marked_since(&store, &place, &v2.id, &v2.id, 3);
    b.open(&store, &place);
    let cleaned = clean(&store, &place, &mut b.cache, "d-b", Some(repairs));
    assert!(cleaned.retired.is_empty() && cleaned.repaired.is_empty(), "{cleaned:?}");

    marked_since(&store, &place, &v2.id, &v2.id, 15);
    let cleaned = clean(&store, &place, &mut b.cache, "d-b", Some(repairs));
    assert_eq!(cleaned.retired, std::slice::from_ref(&v2.id));
    // In "recently deleted", saying why; the line moves on.
    let trashed = saves::trashed(&store, &place).unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(trashed[0].snapshot.id, v2.id);
    assert_eq!(trashed[0].reason.as_deref(), Some(saves::RETIRED));
    assert!(marks(&store).is_empty());
    let view = b.open(&store, &place);
    assert_eq!(view.head().unwrap().id, v1.id);
}

#[test]
fn the_only_version_stays_marked_and_its_writer_lets_go_at_once() {
    // The only version of the line: kept, marked, however long.
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a");
    a.large_save(1);
    let only = a.push(&store, &place, None);
    store.remove(&v2::pack_path(GAME, &only.id)).unwrap();
    marked_since(&store, &place, &only.id, &only.id, 40);
    let staging = a.staging();
    let nothing = Vec::new;
    let repairs = Repairs { local: &nothing, zips: &[], staging: &staging, device_name: "A" };
    a.open(&store, &place);
    let cleaned = clean(&store, &place, &mut a.cache, "d-a", Some(repairs));
    assert!(cleaned.retired.is_empty());
    assert_eq!(marks(&store), BTreeSet::from([only.id.clone()]));

    // Its writer, finding one of its own it can't repair, lets it go the
    // first time: it had its chance.
    let store = Memory::new();
    let mut a = Device::new("d-a");
    a.large_save(1);
    let v1 = a.push(&store, &place, None);
    a.save("saves/world/region0", &bytes(99, 1024));
    let v2 = a.push(&store, &place, Some(&v1));
    store.remove(&v2::pack_path(GAME, &v2.id)).unwrap();
    a.large_save(5);
    let files = a.files();
    let read = || files.clone();
    let staging = a.staging();
    let repairs = Repairs { local: &read, zips: &[], staging: &staging, device_name: "A" };
    a.open(&store, &place);
    let cleaned = clean(&store, &place, &mut a.cache, "d-a", Some(repairs));
    assert_eq!(cleaned.marked, std::slice::from_ref(&v2.id));
    assert_eq!(cleaned.retired, std::slice::from_ref(&v2.id));
    assert_complete(&store, &place);
}
