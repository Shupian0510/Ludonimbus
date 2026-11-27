//! One save line in the cloud, as the sync uses it (docs/plan-v2.md,
//! sections 3.4, 3.7, 5.4, 5.6 and 12): two devices playing in turn,
//! uploads that wait on the device, the cloud of 0.6 read before any
//! migration, kept saves, "recently deleted", clean-up, what an interrupted
//! operation leaves, and how many requests each operation takes.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use ludonimbus_core::cloud::decide::{decide, next_vv, settle, Action, Base, Local};
use ludonimbus_core::cloud::library;
use ludonimbus_core::cloud::line::{self, LineCache, LineView};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::packs::{self, LocalFile, Policy};
use ludonimbus_core::cloud::repo::Repo;
use ludonimbus_core::cloud::retention::{self, DeviceBase};
use ludonimbus_core::cloud::saves::{self, Draft, Open, Place};
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::upgrade;
use ludonimbus_core::cloud::v2::{self, GameRecord, Line, LineIndex, Manifest, Pin};
use ludonimbus_core::cloud::{CloudProvider, TRASH_DAYS};
use ludonimbus_core::vv::Vv;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::OnceLock;
use tempfile::TempDir;

const GAME: &str = "stardew-7k3f";
const OLD: &str = "test-game";

/// Kilobytes where the real policy has megabytes: the rules are the same.
const POLICY: Policy = Policy { full_below: 4 * 1024, split_above: 8 * 1024 };

// -------------------------------------------------------------------- helpers

/// Two hours ago: snapshots made from here on count as made "just now" for
/// the packs (less than a day old).
fn start() -> DateTime<Utc> {
    static START: OnceLock<DateTime<Utc>> = OnceLock::new();
    *START.get_or_init(|| Utc::now() - Duration::hours(2))
}

/// A moment later than every one handed out before.
fn tick() -> DateTime<Utc> {
    static TICKS: AtomicI64 = AtomicI64::new(0);
    start() + Duration::seconds(TICKS.fetch_add(1, Ordering::SeqCst))
}

fn record() -> GameRecord {
    GameRecord {
        id: GAME.into(),
        name: "Stardew Valley".into(),
        folders: vec![GAME.into()],
        lines: vec![Line { id: "main".into(), format: "pc".into(), scope: v2::SCOPE_SHARED.into(), path: v2::line_dir(GAME, "main"), ..Default::default() }],
        ..Default::default()
    }
}

/// A line of a game that is in layout 2 from the start: one folder.
fn place() -> Place {
    let r = record();
    Place::of(&r, &r.lines[0])
}

/// Only the folder of 0.6: what [`saves::trash`], [`saves::untrash`] and
/// [`saves::purge`] do there, without the line's other folder.
fn old_only(key: &str) -> Place {
    Place { dirs: vec![v2::legacy_dir(key)], ..Place::of_key(key) }
}

fn bytes(seed: usize, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 17) % 251) as u8).collect()
}

/// Every file below `dir`: `/`-separated relative path -> content.
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

fn names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
}

fn counts(list: u32, read: u32, download: u32, write: u32, upload: u32, remove: u32) -> Counts {
    Counts { list, read, download, write, upload, remove }
}

/// The vector of a device's first snapshot on an empty line.
fn first(device: &str) -> Vv {
    Vv::next(std::iter::empty::<&Vv>(), device, 0)
}

/// The cloud as files, with the moment each index was written left out (it
/// is the one thing two runs of the same operation write differently).
fn state(store: &Memory) -> BTreeMap<String, Vec<u8>> {
    store
        .paths()
        .into_iter()
        .map(|p| {
            let mut data = store.peek(&p).unwrap();
            if p.ends_with("/index.json") {
                let mut index: LineIndex = serde_json::from_slice(&data).unwrap();
                index.updated_at = None;
                data = serde_json::to_vec(&index).unwrap();
            }
            (p, data)
        })
        .collect()
}

fn copy(store: &Memory) -> Memory {
    let out = Memory::new();
    for p in store.paths() {
        out.put(&p, &store.peek(&p).unwrap());
    }
    out
}

/// The entries of a zip in the cloud: name -> content.
fn zip_entries(store: &Memory, path: &str) -> BTreeMap<String, Vec<u8>> {
    let bytes = store.peek(path).unwrap_or_else(|| panic!("{path} is not in the cloud"));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.insert(entry.name().to_string(), data);
    }
    out
}

/// Nothing refers to a pack that isn't there: every manifest in the line's
/// folders and every record of "recently deleted" of layout 2 has all its
/// packs (a snapshot of 0.6: its zip).
fn assert_complete(store: &Memory, place: &Place) {
    let paths: BTreeSet<String> = store.paths().into_iter().collect();
    let has_pack = |m: &Manifest, dir: &str, pack: &str| {
        if m.v < 2 {
            paths.contains(&format!("{dir}/{pack}.zip"))
        } else {
            place.folders.iter().any(|f| paths.contains(&v2::pack_path(f, pack)))
        }
    };
    for dir in &place.dirs {
        for p in paths.iter().filter(|p| p.starts_with(&format!("{dir}/"))) {
            let name = &p[dir.len() + 1..];
            let Some(id) = name.strip_suffix(".json") else { continue };
            if !v2::is_snapshot_id(id) {
                continue;
            }
            let Ok(m) = serde_json::from_slice::<Manifest>(&store.peek(p).unwrap()) else { continue };
            for pack in m.packs() {
                assert!(has_pack(&m, dir, &pack), "{p} refers to {pack}, which is gone");
            }
        }
        if v2::legacy_key(dir).is_some() {
            continue;
        }
        let trash = v2::trash_dir(dir);
        for p in paths.iter().filter(|p| p.starts_with(&format!("{trash}/"))) {
            let record: Value = serde_json::from_slice(&store.peek(p).unwrap()).unwrap();
            let m: Manifest = serde_json::from_value(record["meta"].clone()).unwrap();
            for pack in m.packs() {
                assert!(has_pack(&m, dir, &pack), "{p} refers to {pack}, which is gone");
            }
        }
    }
}

// ------------------------------------------------------------------- fixture

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cloud-0.6")
}

/// The cloud written by 0.6.2, every file under its path.
fn sample() -> Memory {
    let store = Memory::new();
    let root = fixture().join("cloud");
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel: Vec<String> = entry.path().strip_prefix(&root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        store.put(&rel.join("/"), &std::fs::read(entry.path()).unwrap());
    }
    store
}

/// What 0.6.2 itself listed for each game when the sample was made.
fn expected() -> Value {
    serde_json::from_slice(&std::fs::read(fixture().join("expected.json")).unwrap()).unwrap()
}

/// The same cloud seen by a device still on 0.6.
struct Shared<'a>(&'a Memory);

impl Store for Shared<'_> {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".into()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.0.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.0.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.0.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.0.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.0.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.0.remove(path)
    }
}

/// The ids 0.6 lists for a game: its history and its "recently deleted".
fn seen_by_0_6(store: &Memory, key: &str) -> (Vec<String>, Vec<String>) {
    let repo = Repo::new(Shared(store));
    let history = repo.list_snapshots(key).unwrap().into_iter().map(|s| s.id).collect();
    let trash = repo.list_trash(key).unwrap().into_iter().map(|t| t.snapshot.id).collect();
    (history, trash)
}

// ------------------------------------------------------------ odd stores

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Write,
    Upload,
    Remove,
}

/// `Memory` whose connection breaks at the `at`-th request of one kind.
struct Flaky<'a> {
    inner: &'a Memory,
    kind: Kind,
    at: u32,
    seen: AtomicU32,
}

impl<'a> Flaky<'a> {
    fn new(inner: &'a Memory, kind: Kind, at: u32) -> Self {
        Flaky { inner, kind, at, seen: AtomicU32::new(0) }
    }

    fn request(&self, kind: Kind) -> Result<()> {
        if kind == self.kind && self.seen.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
            return Err(anyhow!("the connection broke"));
        }
        Ok(())
    }
}

impl Store for Flaky<'_> {
    fn kind(&self) -> &'static str {
        "memory"
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
        self.request(Kind::Write)?;
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.request(Kind::Upload)?;
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.request(Kind::Remove)?;
        self.inner.remove(path)
    }
}

/// `Memory` where another device writes `files` right after the first
/// listing of `folder` (which doesn't show them yet).
struct Meanwhile<'a> {
    inner: &'a Memory,
    folder: String,
    files: Vec<(String, Vec<u8>)>,
    done: AtomicBool,
}

impl<'a> Meanwhile<'a> {
    fn new(inner: &'a Memory, folder: &str, files: Vec<(String, Vec<u8>)>) -> Self {
        Meanwhile { inner, folder: folder.into(), files, done: AtomicBool::new(false) }
    }
}

impl Store for Meanwhile<'_> {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "meanwhile".into()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let listing = self.inner.list(dir)?;
        if dir == self.folder && !self.done.swap(true, Ordering::SeqCst) {
            for (path, data) in &self.files {
                self.inner.put(path, data);
            }
        }
        Ok(listing)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.inner.remove(path)
    }
}

// -------------------------------------------------------------------- device

/// One device as the sync keeps it: its saves, the line as it last read it,
/// what it synced last, and a folder for uploads that wait.
struct Device {
    tmp: TempDir,
    id: &'static str,
    name: &'static str,
    cache: LineCache,
    base: Option<Base>,
    /// The manifest of `base`: what a large save's next upload builds on.
    synced: Option<Manifest>,
    counter: u64,
    pulls: u32,
}

impl Device {
    fn new(id: &'static str, name: &'static str) -> Self {
        Device { tmp: tempfile::tempdir().unwrap(), id, name, cache: LineCache::default(), base: None, synced: None, counter: 0, pulls: 0 }
    }

    /// The saves, under their canonical paths.
    fn root(&self) -> PathBuf {
        self.tmp.path().join("local")
    }

    /// What waits for the cloud.
    fn queue(&self) -> PathBuf {
        self.tmp.path().join("queue")
    }

    fn staging(&self) -> PathBuf {
        self.tmp.path().join("staging")
    }

    fn dest(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn save(&self, path: &str, data: &[u8]) {
        let p = self.root().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    /// A save too large to be uploaded whole every time: six files of 1 KiB.
    fn large_save(&self, version: usize) {
        for n in 0..6 {
            self.save(&format!("saves/world/region{n}"), &bytes(n + 10 * version, 1024));
        }
    }

    fn files(&self) -> Vec<LocalFile> {
        let root = self.root();
        tree(&root).keys().map(|p| LocalFile::from_disk(p.clone(), root.join(p)).unwrap()).collect()
    }

    fn local(&self) -> Local {
        let files = self.files();
        Local { content_hash: (!files.is_empty()).then(|| saves::content_hash(&files)), base: self.base.clone() }
    }

    fn open(&mut self, store: &dyn Store, place: &Place) -> LineView {
        saves::open(store, place, &mut self.cache).unwrap()
    }

    fn draft(&self, place: &Place, vv: Vv, parent: Option<String>, side: bool, at: DateTime<Utc>, base: Option<&Manifest>) -> Manifest {
        let d = Draft {
            vv,
            parent,
            side,
            trigger: if side { "pre-restore".into() } else { "auto-exit".into() },
            platform: "steam".into(),
            device_id: self.id.into(),
            device_name: self.name.into(),
            created_at: at,
            ..Default::default()
        };
        saves::draft(place, &d, &self.files(), base, &POLICY)
    }

    /// The snapshot a push decided on `view` writes.
    fn make(&self, place: &Place, view: &LineView, bases: &[Vv], parent: Option<String>, at: DateTime<Utc>) -> Manifest {
        let vv = next_vv(bases, view, self.id, self.counter);
        self.draft(place, vv, parent, false, at, self.synced.as_ref())
    }

    fn synced_to(&mut self, m: &Manifest) {
        self.base = Some(Base { id: m.id.clone(), vv: m.vv.clone(), content_hash: m.content_hash.clone() });
        self.counter = self.counter.max(m.vv.get(self.id));
        self.synced = Some(m.clone());
    }

    /// Upload a snapshot the way the sync does: open, stage, publish, unstage.
    fn upload(&mut self, store: &dyn Store, place: &Place, m: &Manifest) {
        let (files, queue) = (self.files(), self.queue());
        saves::stage(m, &files, &queue).unwrap();
        self.open(store, place);
        saves::publish(store, place, &mut self.cache, m, &queue, &files, self.id).unwrap();
        saves::unstage(&queue, m).unwrap();
        if !m.side {
            self.synced_to(m);
        }
    }

    /// What `decide` said: push the saves here.
    fn push(&mut self, store: &dyn Store, place: &Place, action: Action) -> Manifest {
        let Action::Push { bases, parent } = action else { panic!("expected a push, got {action:?}") };
        let view = self.open(store, place);
        let m = self.make(place, &view, &bases, parent, tick());
        self.upload(store, place, &m);
        m
    }

    /// A push of a large save made at `at`, built on what this device synced
    /// last.
    fn push_at(&mut self, store: &dyn Store, place: &Place, at: DateTime<Utc>) -> Manifest {
        let view = self.open(store, place);
        let bases: Vec<Vv> = view.head_entries().map(|h| h.vv.clone()).collect();
        let parent = view.head().map(|h| h.id.clone());
        let m = self.make(place, &view, &bases, parent, at);
        self.upload(store, place, &m);
        m
    }

    /// Replace the saves here with a version of the line.
    fn pull(&mut self, store: &dyn Store, place: &Place, id: &str) {
        let view = self.open(store, place);
        let m = saves::manifest(store, place, &view, id).unwrap();
        self.pulls += 1;
        let dest = self.dest(&format!("pull-{}", self.pulls));
        saves::fetch(store, place, &view, &m, None, &dest, &self.staging()).unwrap();
        let _ = std::fs::remove_dir_all(self.root());
        std::fs::rename(&dest, self.root()).unwrap();
        let vv = view.get(id).unwrap().vv.clone();
        self.base = Some(Base { id: id.into(), vv, content_hash: m.content_hash.clone() });
        self.synced = Some(m);
    }
}

// ------------------------------------------------------------------- places

#[test]
fn a_place_is_where_the_record_says_and_a_0_6_key_is_what_the_upgrade_makes_of_it() {
    let mut r = record();
    r.lines[0].also = vec![v2::legacy_dir("stardew"), "games/stardew-other/saves/main".into()];
    r.folders.push("stardew".into());
    let p = Place::of(&r, &r.lines[0]);
    assert_eq!(p.game, GAME);
    assert_eq!(p.line, "main");
    assert_eq!(p.folders, [GAME, "stardew"]);
    assert_eq!(p.dirs, ["games/stardew-7k3f/saves/main", "games/stardew/snapshots", "games/stardew-other/saves/main"]);
    assert_eq!(p.dir(), "games/stardew-7k3f/saves/main");

    let old = Place::of_key("stardew");
    assert_eq!(
        old,
        Place {
            game: "stardew".into(),
            line: "main".into(),
            folders: vec!["stardew".into()],
            dirs: vec!["games/stardew/saves/main".into(), "games/stardew/snapshots".into()],
        }
    );
    assert_eq!(old.dir(), v2::line_dir("stardew", "main"));
}

#[test]
fn a_line_read_before_the_upgrade_is_the_line_it_makes() {
    let store = sample();
    let expected = expected();
    let keys: Vec<&String> = expected.as_object().unwrap().keys().collect();
    let mut before = BTreeMap::new();
    for key in &keys {
        // The record the upgrade writes for the key.
        let r = upgrade::record_for(key, None, None, true);
        assert_eq!(Place::of(&r, &r.lines[0]), Place::of_key(key), "{key}");
        let view = saves::open(&store, &Place::of_key(key), &mut LineCache::default()).unwrap();
        before.insert(key.to_string(), view);
    }

    upgrade::run(&store, "d-u", "Upgrader", Utc::now(), &mut |_| {}).unwrap();
    for key in &keys {
        let r = library::read(&store, key).unwrap().unwrap();
        let place = Place::of(&r, &r.lines[0]);
        let old = Place::of_key(key);
        // A record that another one was merged into reads the other's folder
        // too; it writes where the key's line wrote.
        assert_eq!((&place.game, &place.line, &place.dirs, &place.folders[0]), (&old.game, &old.line, &old.dirs, &old.folders[0]), "{key}");
        let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
        let was = &before[key.as_str()];
        let ids = |v: &LineView| v.snapshots.iter().map(|s| (s.id.clone(), s.vv.clone())).collect::<Vec<_>>();
        assert_eq!(ids(&view), ids(was), "{key}");
        assert_eq!(view.heads, was.heads, "{key}");
        assert_eq!(view.places, was.places, "{key}");
    }
}

#[test]
fn a_draft_has_a_new_id_of_its_moment_and_the_hash_of_0_6() {
    let dev = Device::new("d-a", "Desktop");
    dev.save("saves/Farm_1/Farm_1", &bytes(1, 1500));
    dev.save("saves/Farm_1/SaveGameInfo", &bytes(2, 300));
    let at = tick();
    let mut vv = Vv::new();
    vv = Vv::next([&vv], "d-a", 0);
    let m = dev.draft(&place(), vv.clone(), Some("01JD8X3K2M0000000000000001".into()), false, at, None);

    assert_eq!(m.v, v2::LAYOUT);
    assert!(v2::is_snapshot_id(&m.id));
    assert_eq!(ulid::Ulid::from_string(&m.id).unwrap().timestamp_ms(), at.timestamp_millis() as u64);
    assert_eq!((m.game.as_str(), m.line.as_str()), (GAME, "main"));
    assert_eq!(m.vv, vv);
    assert_eq!(m.parent.as_deref(), Some("01JD8X3K2M0000000000000001"));
    assert_eq!((m.device_id.as_str(), m.device_name.as_str(), m.platform.as_str()), ("d-a", "Desktop", "steam"));
    assert_eq!(m.created_at, at);
    assert_eq!(m.total_size, 1800);
    assert_eq!(m.app_version, ludonimbus_core::APP_VERSION);
    assert!(m.requires.is_empty());
    assert_eq!(m.files, packs::plan(&m.id, &dev.files(), None, &POLICY).files);

    // The hash 0.6 computes for the same files.
    let entries: Vec<ludonimbus_core::snapshot::FileEntry> =
        dev.files().iter().map(|f| ludonimbus_core::snapshot::FileEntry { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime }).collect();
    assert_eq!(m.content_hash, ludonimbus_core::snapshot::content_hash(&entries));
    assert_eq!(saves::content_hash(&[]), "empty");

    // Two drafts of the same moment are two snapshots.
    let again = dev.draft(&place(), vv, None, false, at, None);
    assert_ne!(again.id, m.id);
}

// ------------------------------------------------------------- two devices

#[test]
fn two_devices_play_in_turn_fork_and_settle() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    let mut b = Device::new("d-b", "Deck");

    // A uploads first.
    a.save("saves/slot1", b"a1");
    let view = a.open(&store, &place);
    let s1 = a.push(&store, &place, decide(&a.local(), &view));
    assert_eq!(s1.vv.get("d-a"), 1);

    // B has nothing: it is told to pull, and gets A's bytes.
    let view = b.open(&store, &place);
    assert_eq!(decide(&b.local(), &view), Action::Pull { id: s1.id.clone() });
    b.pull(&store, &place, &s1.id);
    assert_eq!(tree(&b.root()), tree(&a.root()));
    assert_eq!(decide(&b.local(), &b.open(&store, &place)), Action::Nothing);

    // B plays and uploads; A pulls.
    b.save("saves/slot1", b"b2");
    b.save("saves/slot2", b"new on b");
    let view = b.open(&store, &place);
    let s2 = b.push(&store, &place, decide(&b.local(), &view));
    assert!(s2.vv.contains(&s1.vv));
    let view = a.open(&store, &place);
    assert_eq!(view.head().unwrap().id, s2.id);
    assert_eq!(decide(&a.local(), &view), Action::Pull { id: s2.id.clone() });
    a.pull(&store, &place, &s2.id);
    assert_eq!(tree(&a.root()), tree(&b.root()));

    // Both play; B is offline and stages its upload.
    let b_saw = b.open(&store, &place);
    a.save("saves/slot1", b"a3");
    b.save("saves/slot1", b"b3");
    let view = a.open(&store, &place);
    let s3 = a.push(&store, &place, decide(&a.local(), &view));
    let Action::Push { bases, parent } = decide(&b.local(), &b_saw) else { panic!("B would push") };
    let vv = next_vv(&bases, &b_saw, b.id, b.counter);
    // Made without the cloud: built on no pack.
    let s4 = b.draft(&place, vv, parent, false, tick(), None);
    saves::stage(&s4, &b.files(), &b.queue()).unwrap();
    assert_eq!(saves::staged(&b.queue()).unwrap(), std::slice::from_ref(&s4));

    // Back online: the upload that waited goes up.
    b.open(&store, &place);
    let q = b.queue();
    for m in saves::staged(&q).unwrap() {
        saves::publish(&store, &place, &mut b.cache, &m, &q, &[], b.id).unwrap();
        saves::unstage(&q, &m).unwrap();
        b.synced_to(&m);
    }
    assert!(saves::staged(&b.queue()).unwrap().is_empty());

    // The fork is reported on both devices.
    let view = b.open(&store, &place);
    assert!(view.is_forked());
    let heads: Vec<String> = view.head_entries().map(|h| h.id.clone()).collect();
    assert_eq!(heads, [s3.id.clone(), s4.id.clone()]);
    let fork = Action::Conflict { candidates: heads.clone(), local_is_candidate: false };
    assert_eq!(decide(&b.local(), &view), fork);
    assert_eq!(decide(&a.local(), &a.open(&store, &place)), fork);

    // On A the user keeps A's saves: the push that ends it contains both.
    let view = a.open(&store, &place);
    let s5 = a.push(&store, &place, settle(&a.local(), &view));
    assert!(s5.vv.contains(&s3.vv) && s5.vv.contains(&s4.vv));
    let view = b.open(&store, &place);
    assert_eq!(view.head().unwrap().id, s5.id);

    // B keeps its saves on the side before it pulls: the head doesn't move.
    assert_eq!(decide(&b.local(), &view), Action::Pull { id: s5.id.clone() });
    let side = b.draft(&place, b.base.clone().unwrap().vv, Some(s4.id.clone()), true, tick(), None);
    b.upload(&store, &place, &side);
    let view = b.open(&store, &place);
    assert_eq!(view.head().unwrap().id, s5.id);
    assert!(!view.is_forked());
    assert!(view.get(&side.id).unwrap().is_side());
    b.pull(&store, &place, &s5.id);
    assert_eq!(tree(&b.root()), tree(&a.root()));
    assert_eq!(std::fs::read(b.root().join("saves/slot1")).unwrap(), b"a3");
    assert_eq!(decide(&b.local(), &b.open(&store, &place)), Action::Nothing);
}

// ------------------------------------------------------------------- packs

#[test]
fn a_small_save_is_one_complete_pack() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.save("saves/Farm_1/Farm_1", &bytes(1, 1500));
    a.save("saves/Farm_1/SaveGameInfo", &bytes(2, 300));
    a.save("saves/startup_preferences", b"");
    let m = a.push_at(&store, &place, tick());
    assert_eq!(m.packs().into_iter().collect::<Vec<_>>(), std::slice::from_ref(&m.id));

    // The zip alone is the whole save: unpacked, it is exactly the files.
    let entries = zip_entries(&store, &v2::pack_path(GAME, &m.id));
    let files: BTreeMap<String, Vec<u8>> =
        entries.iter().filter_map(|(name, data)| name.strip_prefix("files/").map(|p| (p.to_string(), data.clone()))).collect();
    assert_eq!(files, tree(&a.root()));
    assert_eq!(entries.len(), files.len() + 1);
    assert_eq!(serde_json::from_slice::<Manifest>(&entries["snapshot.json"]).unwrap(), m);
    assert_eq!(store.paths(), [v2::pack_path(GAME, &m.id), v2::manifest_path(place.dir(), &m.id), v2::index_path(place.dir()),]);
}

#[test]
fn a_large_save_uploads_what_changed_and_fetches_whole() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.large_save(1);
    let first = a.push_at(&store, &place, tick());
    assert_eq!(first.packs().len(), 1);

    // One file changes: only it is uploaded, the rest is borrowed.
    a.save("saves/world/region2", &bytes(99, 1024));
    let second = a.push_at(&store, &place, tick());
    let refs: Vec<(&str, Option<&str>)> = second.files.iter().map(|f| (f.path.as_str(), f.pack.as_deref())).collect();
    for (path, pack) in &refs {
        if *path == "saves/world/region2" {
            assert_eq!(*pack, None);
        } else {
            assert_eq!(*pack, Some(first.id.as_str()), "{path}");
        }
    }
    let own = zip_entries(&store, &v2::pack_path(GAME, &second.id));
    assert_eq!(own.keys().cloned().collect::<Vec<_>>(), ["files/saves/world/region2", "snapshot.json"]);

    // Another device fetches it whole, or only some files.
    let b = Device::new("d-b", "Deck");
    let mut cache = LineCache::default();
    let view = saves::open(&store, &place, &mut cache).unwrap();
    let m = saves::manifest(&store, &place, &view, &second.id).unwrap();
    assert_eq!(m, second);
    let dest = b.dest("all");
    saves::fetch(&store, &place, &view, &m, None, &dest, &b.staging()).unwrap();
    assert_eq!(tree(&dest), tree(&a.root()));

    let some = b.dest("some");
    let only = ["saves/world/region0".to_string()];
    store.take();
    saves::fetch(&store, &place, &view, &m, Some(&only), &some, &b.staging()).unwrap();
    assert_eq!(store.take(), counts(0, 0, 1, 0, 0, 0));
    assert_eq!(tree(&some).keys().cloned().collect::<Vec<_>>(), ["saves/world/region0"]);
    assert_eq!(tree(&some)["saves/world/region0"], bytes(10, 1024));

    // The pack it borrows from is gone: the version is incomplete, and the
    // destination is left as it was.
    store.remove(&v2::pack_path(GAME, &first.id)).unwrap();
    let e = saves::fetch(&store, &place, &view, &m, None, &some, &b.staging()).unwrap_err();
    assert_eq!(packs::missing_pack(&e), Some(&packs::MissingPack { pack: first.id.clone() }));
    assert_eq!(tree(&some).keys().cloned().collect::<Vec<_>>(), ["saves/world/region0"]);
    // What it brings itself still comes.
    let mine = b.dest("mine");
    saves::fetch(&store, &place, &view, &m, Some(&["saves/world/region2".to_string()]), &mine, &b.staging()).unwrap();
    assert_eq!(tree(&mine)["saves/world/region2"], bytes(99, 1024));
}

#[test]
fn a_version_the_view_does_not_have_is_an_error() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.save("saves/a", b"1");
    let m = a.push_at(&store, &place, tick());
    let view = a.open(&store, &place);
    assert!(saves::manifest(&store, &place, &LineView::default(), &m.id).is_err());
    assert!(saves::fetch(&store, &place, &LineView::default(), &m, None, &a.dest("x"), &a.staging()).is_err());
    // Deleted since the view was read.
    store.remove(&v2::manifest_path(place.dir(), &m.id)).unwrap();
    assert!(saves::manifest(&store, &place, &view, &m.id).is_err());
}

// ----------------------------------------------------------------- staging

#[test]
fn staged_snapshots_wait_on_the_device_and_go_up_as_they_were() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    let q = a.queue();
    assert!(saves::staged(&q).unwrap().is_empty(), "a missing folder is empty");

    // Three snapshots, staged as they are made; the clock says `w` was made
    // first.
    let (t1, t2, t3) = (tick(), tick(), tick());
    a.save("saves/slot", b"one");
    let x = a.draft(&place, first("d-a"), None, false, t2, None);
    saves::stage(&x, &a.files(), &q).unwrap();
    let x_files = tree(&a.root());
    a.save("saves/slot", b"two, a split one");
    a.save("saves/big", &bytes(3, 9 * 1024));
    let y = a.draft(&place, Vv::next([&x.vv], "d-a", 0), Some(x.id.clone()), false, t3, None);
    assert_eq!(y.packs().len(), 2);
    saves::stage(&y, &a.files(), &q).unwrap();
    let y_files = tree(&a.root());
    std::fs::remove_file(a.root().join("saves/big")).unwrap();
    a.save("saves/slot", b"zero");
    let w = a.draft(&place, first("d-a"), None, false, t1, None);
    saves::stage(&w, &a.files(), &q).unwrap();
    let w_files = tree(&a.root());
    assert_eq!(saves::staged(&q).unwrap(), [w.clone(), x.clone(), y.clone()], "oldest first");
    assert!(!names(&q).iter().any(|n| n.ends_with(".lntmp")));

    // The saves changed between the draft and the staging: nothing is left.
    let z = a.draft(&place, first("d-a"), None, false, tick(), None);
    a.save("saves/slot", b"changed meanwhile");
    assert!(saves::stage(&z, &a.files(), &q).is_err());
    assert!(!names(&q).iter().any(|n| n.starts_with(&z.id)));

    // `v` was not staged completely: its zip is there, its manifest isn't.
    let v = a.draft(&place, first("d-a"), None, false, tick(), None);
    saves::stage(&v, &a.files(), &q).unwrap();
    std::fs::remove_file(q.join(format!("{}.json", v.id))).unwrap();
    assert!(names(&q).contains(&format!("{}.zip", v.id)));
    // Not snapshots at all.
    std::fs::write(q.join("01JZZZZZZZZZZZZZZZZZZZZZZZ.json"), b"{ not json").unwrap();
    std::fs::write(q.join("notes.json"), serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(saves::staged(&q).unwrap(), [w.clone(), x.clone(), y.clone()]);

    // The saves change before the upload: what goes up is what was staged.
    a.save("saves/slot", b"four");
    for m in saves::staged(&q).unwrap() {
        a.open(&store, &place);
        saves::publish(&store, &place, &mut a.cache, &m, &q, &[], a.id).unwrap();
    }
    let b = Device::new("d-b", "Deck");
    let mut cache = LineCache::default();
    let view = saves::open(&store, &place, &mut cache).unwrap();
    for (m, files) in [(&w, &w_files), (&x, &x_files), (&y, &y_files)] {
        let dest = b.dest(&m.id);
        saves::fetch(&store, &place, &view, m, None, &dest, &b.staging()).unwrap();
        assert_eq!(&tree(&dest), files);
    }
    assert_eq!(y_files["saves/big"], bytes(3, 9 * 1024));

    // Published twice: the same cloud.
    let once = state(&store);
    a.open(&store, &place);
    saves::publish(&store, &place, &mut a.cache, &y, &q, &[], a.id).unwrap();
    assert_eq!(state(&store), once);
    // Publishing removes nothing from the folder.
    assert!(names(&q).contains(&format!("{}.json", y.id)));

    // Unstaging takes exactly that snapshot's files: its manifest, its
    // packs and the notes that they were sent and that its manifest was in
    // the cloud.
    let before = names(&q);
    saves::unstage(&q, &y).unwrap();
    let gone: BTreeSet<String> = before.difference(&names(&q)).cloned().collect();
    let of_y = [".json", ".zip", "-2.zip", ".sent", "-2.sent", ".shown"].map(|end| format!("{}{end}", y.id));
    assert_eq!(gone, BTreeSet::from(of_y));
    assert_eq!(saves::staged(&q).unwrap(), [w.clone(), x.clone()]);
    saves::unstage(&q, &y).unwrap();
}

#[test]
fn an_interrupted_publish_leaves_nothing_to_undo() {
    let a = Device::new("d-a", "Desktop");
    let place = place();
    a.save("saves/slot", b"split");
    a.save("saves/big", &bytes(3, 9 * 1024));
    let m = a.draft(&place, first("d-a"), None, false, tick(), None);
    assert_eq!(m.packs().len(), 2);
    let q = a.queue();
    saves::stage(&m, &a.files(), &q).unwrap();

    let reference = Memory::new();
    let mut cache = LineCache::default();
    saves::open(&reference, &place, &mut cache).unwrap();
    saves::publish(&reference, &place, &mut cache, &m, &q, &[], a.id).unwrap();

    for (kind, at) in [(Kind::Upload, 1), (Kind::Upload, 2), (Kind::Write, 1), (Kind::Write, 2)] {
        let store = Memory::new();
        let mut cache = LineCache::default();
        saves::open(&store, &place, &mut cache).unwrap();
        let flaky = Flaky::new(&store, kind, at);
        assert!(saves::publish(&flaky, &place, &mut cache, &m, &q, &[], a.id).is_err(), "{kind:?} {at}");
        // A manifest is never there without its packs.
        assert_complete(&store, &place);
        let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
        assert_eq!(view.get(&m.id).is_some(), kind == Kind::Write && at == 2, "{kind:?} {at}");
        // Again: the state an uninterrupted upload leaves.
        saves::publish(&store, &place, &mut cache, &m, &q, &[], a.id).unwrap();
        assert_eq!(state(&store), state(&reference), "{kind:?} {at}");
    }
}

#[test]
fn an_upload_of_several_packs_goes_on_where_it_broke_off() {
    let a = Device::new("d-a", "Desktop");
    let place = place();
    a.save("saves/slot", b"split");
    a.save("saves/big", &bytes(3, 9 * 1024));
    let m = a.draft(&place, first("d-a"), None, false, tick(), None);
    let packs: Vec<String> = m.packs().into_iter().collect();
    assert_eq!(packs.len(), 2);
    let q = a.queue();
    saves::stage(&m, &a.files(), &q).unwrap();
    let folder = place.folders.first().unwrap().clone();
    let marks = |q: &Path| names(q).into_iter().filter(|n| n.ends_with(".sent")).count();

    let reference = Memory::new();
    let mut cache = LineCache::default();
    saves::open(&reference, &place, &mut cache).unwrap();
    saves::publish(&reference, &place, &mut cache, &m, &q, &[], a.id).unwrap();
    saves::stage(&m, &a.files(), &q).unwrap();
    assert_eq!(marks(&q), 0, "staged anew: nothing counts as sent");

    // What happened to the pack that got through before the connection
    // broke, and how many packs the next try uploads.
    type Meanwhile = fn(&Memory, &str);
    let cases: [(&str, Meanwhile, u32); 3] = [
        ("still there", |_, _| {}, 1),
        ("cleaned up by another device", |store, path| store.remove(path).unwrap(), 2),
        ("cut short", |store, path| store.put(path, b"PK"), 2),
    ];
    for (what, meanwhile, uploads) in cases {
        let store = Memory::new();
        let mut cache = LineCache::default();
        saves::open(&store, &place, &mut cache).unwrap();
        let flaky = Flaky::new(&store, Kind::Upload, 2);
        assert!(saves::publish(&flaky, &place, &mut cache, &m, &q, &[], a.id).is_err(), "{what}");
        assert_eq!(marks(&q), 1, "{what}");
        meanwhile(&store, &v2::pack_path(&folder, &packs[0]));
        store.take();
        saves::publish(&store, &place, &mut cache, &m, &q, &[], a.id).unwrap();
        // One listing to see what arrived, then only what is missing.
        assert_eq!(store.take(), counts(1, 0, 0, 2, uploads, 0), "{what}");
        assert_eq!(state(&store), state(&reference), "{what}");
        // Uploaded: nothing of it stays in the queue.
        saves::unstage(&q, &m).unwrap();
        assert_eq!(names(&q), BTreeSet::new(), "{what}");
        saves::stage(&m, &a.files(), &q).unwrap();
    }
}

#[test]
fn packs_that_are_mostly_dead_weight_are_tidied_up() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    let region = |a: &Device, n: usize, version: usize| a.save(&format!("saves/world/region{n}"), &bytes(n + 10 * version, 1024));
    let sizes = |m: &Manifest| -> BTreeMap<String, u64> { serde_json::from_value(m.extra["packSizes"].clone()).unwrap() };
    let mut made: Vec<(Manifest, BTreeMap<String, Vec<u8>>)> = Vec::new();

    // Six files of 1 KiB: one pack.
    a.large_save(1);
    let one = a.push_at(&store, &place, tick());
    assert_eq!(sizes(&one), BTreeMap::from([(one.id.clone(), 6 * 1024)]));
    made.push((one.clone(), tree(&a.root())));
    // Five of them change: the first pack still gives one file.
    (0..5).for_each(|n| region(&a, n, 2));
    let two = a.push_at(&store, &place, tick());
    assert_eq!(two.packs(), BTreeSet::from([one.id.clone(), two.id.clone()]));
    assert_eq!(sizes(&two), BTreeMap::from([(one.id.clone(), 6 * 1024), (two.id.clone(), 5 * 1024)]));
    made.push((two.clone(), tree(&a.root())));
    // Four change again. 4 KiB new, and 5 + 6 KiB of packs for 2 KiB: the
    // file still taken from the first pack goes along, and that pack is
    // used no more.
    (0..4).for_each(|n| region(&a, n, 3));
    let three = a.push_at(&store, &place, tick());
    assert_eq!(three.packs(), BTreeSet::from([two.id.clone(), three.id.clone()]));
    assert_eq!(sizes(&three), BTreeMap::from([(two.id.clone(), 5 * 1024), (three.id.clone(), 5 * 1024)]));
    made.push((three.clone(), tree(&a.root())));
    assert_complete(&store, &place);

    // Every version is still what it was.
    let mut b = Device::new("d-b", "Deck");
    for (m, files) in &made {
        b.pull(&store, &place, &m.id);
        assert_eq!(&tree(&b.root()), files, "{}", m.id);
    }
}

#[test]
fn a_borrowed_pack_cleaned_up_meanwhile_is_put_back() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.large_save(1);
    let first = a.push_at(&store, &place, tick());
    a.save("saves/world/region0", &bytes(77, 1024));
    let view = a.open(&store, &place);
    let second = a.make(&place, &view, std::slice::from_ref(&first.vv), Some(first.id.clone()), tick());
    let (files, q) = (a.files(), a.queue());
    saves::stage(&second, &files, &q).unwrap();
    // The pack goes while this one uploads (deleted outside Ludonimbus: the
    // first version still uses it).
    store.remove(&v2::pack_path(GAME, &first.id)).unwrap();
    a.open(&store, &place);
    store.take();
    saves::publish(&store, &place, &mut a.cache, &second, &q, &files, a.id).unwrap();
    // The look at the packs; the manifest of the first version, which uses
    // the pack too; the pack put back with what is found of it; the first
    // version marked, because the file it had and this one doesn't is
    // nowhere; manifest and index.
    assert_eq!(store.take(), counts(1, 1, 0, 3, 2, 0));
    assert_complete(&store, &place);
    let b = Device::new("d-b", "Deck");
    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    assert!(view.get(&first.id).unwrap().is_incomplete());
    assert!(!view.get(&second.id).unwrap().is_incomplete());
    let dest = b.dest("x");
    saves::fetch(&store, &place, &view, &second, None, &dest, &b.staging()).unwrap();
    assert_eq!(tree(&dest), tree(&a.root()));
    // Its scratch folder is gone; the staged files are all still there.
    assert_eq!(names(&q), BTreeSet::from([format!("{}.json", second.id), format!("{}.zip", second.id), format!("{}.shown", second.id)]));
}

#[test]
fn a_borrowed_pack_that_cannot_be_put_back_takes_the_manifest_with_it() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.large_save(1);
    let first = a.push_at(&store, &place, tick());
    a.save("saves/world/region0", &bytes(77, 1024));
    let view = a.open(&store, &place);
    let second = a.make(&place, &view, std::slice::from_ref(&first.vv), Some(first.id.clone()), tick());
    let q = a.queue();
    saves::stage(&second, &a.files(), &q).unwrap();
    // While it waits, another device cleans up the first version and its
    // pack, and a file it takes from that pack changes here.
    store.remove(&v2::manifest_path(place.dir(), &first.id)).unwrap();
    store.remove(&v2::pack_path(GAME, &first.id)).unwrap();
    a.save("saves/world/region1", &bytes(78, 1024));
    a.open(&store, &place);
    let index = store.peek(&v2::index_path(place.dir()));
    let files = a.files();
    store.take();
    let err = saves::publish(&store, &place, &mut a.cache, &second, &q, &files, a.id).unwrap_err();
    assert_eq!(packs::missing_pack(&err), Some(&packs::MissingPack { pack: first.id.clone() }));
    // Its pack, its manifest, a look at the packs, the manifest taken away
    // again; nothing uploaded in vain, the index as it was.
    assert_eq!(store.take(), counts(1, 0, 0, 1, 1, 1));
    assert!(store.peek(&v2::manifest_path(place.dir(), &second.id)).is_none());
    assert_eq!(store.peek(&v2::index_path(place.dir())), index);
    assert_complete(&store, &place);
    assert!(names(&q).contains(&format!("{}.json", second.id)));

    // With the file as it was, the pack is put back and the version is
    // published after all.
    a.save("saves/world/region1", &bytes(11, 1024));
    a.open(&store, &place);
    let files = a.files();
    saves::publish(&store, &place, &mut a.cache, &second, &q, &files, a.id).unwrap();
    assert_complete(&store, &place);
    let b = Device::new("d-b", "Deck");
    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    let dest = b.dest("x");
    saves::fetch(&store, &place, &view, &second, None, &dest, &b.staging()).unwrap();
    assert_eq!(tree(&dest), tree(&a.root()));
}

// ---------------------------------------------------------------- cloud of 0.6

#[test]
fn every_version_of_0_6_is_read_through_the_key_before_any_migration() {
    let store = sample();
    let expected = expected();
    let dev = Device::new("d-r", "Reader");
    for (key, snapshots) in expected.as_object().unwrap() {
        let place = Place::of_key(key);
        let mut cache = LineCache::default();
        let view = saves::open(&store, &place, &mut cache).unwrap();
        assert!(view.unreadable.is_empty(), "{key}");
        assert_eq!(view.snapshots.len(), snapshots.as_array().unwrap().len(), "{key}");
        for was in snapshots.as_array().unwrap() {
            let id = was["id"].as_str().unwrap();
            assert_eq!(place.dir_of(&view, id), Some(v2::legacy_dir(key).as_str()));
            let m = saves::manifest(&store, &place, &view, id).unwrap();
            let dest = dev.dest(&format!("{key}-{id}"));
            saves::fetch(&store, &place, &view, &m, None, &dest, &dev.staging()).unwrap();
            let got = tree(&dest);
            let want: BTreeMap<String, Vec<u8>> =
                was["files"].as_object().unwrap().iter().map(|(path, text)| (path.clone(), text.as_str().unwrap().as_bytes().to_vec())).collect();
            assert_eq!(got, want, "{key} {id}");
            for f in &m.files {
                assert_eq!(blake3::hash(&want[&f.path]).to_hex().to_string(), f.hash, "{key} {id} {}", f.path);
            }
            // The same saves have the same hash in both layouts.
            let local: Vec<LocalFile> = got.keys().map(|p| LocalFile::from_disk(p.clone(), dest.join(p)).unwrap()).collect();
            assert_eq!(saves::content_hash(&local), was["contentHash"].as_str().unwrap(), "{key} {id}");
        }
    }
}

#[test]
fn an_upload_to_a_line_of_0_6_writes_only_into_the_new_folders() {
    let store = sample();
    let place = Place::of_key(OLD);
    let before = state(&store);
    let history = seen_by_0_6(&store, OLD);

    let mut a = Device::new("d-a", "Desktop");
    let view = a.open(&store, &place);
    let head = view.head().unwrap().clone();
    a.pull(&store, &place, &head.id);
    assert_eq!(a.synced.as_ref().unwrap().v, 1);
    a.save("install/save/slot1.sav", b"after the migration");
    let view = a.open(&store, &place);
    let action = decide(&a.local(), &view);
    assert_eq!(action, Action::Push { bases: vec![head.vv.clone()], parent: Some(head.id.clone()) });
    let m = a.push(&store, &place, action);
    // The base is of 0.6: nothing is borrowed from its zip.
    assert!(m.files.iter().all(|f| f.pack.is_none()));

    let after = state(&store);
    for (path, data) in &before {
        assert_eq!(after.get(path), Some(data), "{path} changed");
    }
    for path in after.keys().filter(|p| !before.contains_key(*p)) {
        assert!(path.starts_with(&format!("games/{OLD}/saves/main/")) || path.starts_with(&format!("games/{OLD}/packs/")), "{path} was written");
    }
    assert_eq!(seen_by_0_6(&store, OLD), history);

    // The new version is the head; every old one is still there.
    let view = a.open(&store, &place);
    assert_eq!(view.head().unwrap().id, m.id);
    assert_eq!(place.dir_of(&view, &m.id), Some(place.dir()));
    assert_eq!(view.snapshots.len(), history.0.len() + 1);
}

// ------------------------------------------------------------- kept saves

#[test]
fn keep_rename_and_unkeep() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.save("saves/a", b"1");
    let s1 = a.push_at(&store, &place, tick());
    a.save("saves/a", b"2");
    a.push_at(&store, &place, tick());

    let now = Utc::now();
    let view = a.open(&store, &place);
    saves::keep(&store, &place, &view, &s1.id, "before the boss", "d-a", now).unwrap();
    let pin: Pin = serde_json::from_slice(&store.peek(&v2::pin_path(place.dir(), &s1.id)).unwrap()).unwrap();
    assert_eq!(pin, Pin { name: "before the boss".into(), note: String::new(), created_at: Some(now), device_id: "d-a".into(), ..Default::default() });
    let view = a.open(&store, &place);
    assert_eq!(view.get(&s1.id).unwrap().pin.as_ref().unwrap().name, "before the boss");

    // A later version wrote more into the pin: renaming keeps all of it.
    let written = serde_json::json!({
        "name": "before the boss", "note": "the second try", "createdAt": "2026-01-02T03:04:05Z",
        "deviceId": "d-x", "colour": "red", "order": 3
    });
    store.write(&v2::pin_path(place.dir(), &s1.id), &serde_json::to_vec(&written).unwrap()).unwrap();
    let view = a.open(&store, &place);
    saves::keep(&store, &place, &view, &s1.id, "after the boss", "d-b", Utc::now()).unwrap();
    let mut renamed: Value = serde_json::from_slice(&store.peek(&v2::pin_path(place.dir(), &s1.id)).unwrap()).unwrap();
    assert_eq!(renamed["name"], "after the boss");
    renamed["name"] = "before the boss".into();
    assert_eq!(renamed, written);
    assert_eq!(a.open(&store, &place).get(&s1.id).unwrap().pin.as_ref().unwrap().name, "after the boss");

    // Not a version of this line.
    assert!(saves::keep(&store, &place, &view, "01JZZZZZZZZZZZZZZZZZZZZZZZ", "x", "d-a", now).is_err());

    saves::unkeep(&store, &place, &s1.id).unwrap();
    assert!(store.peek(&v2::pin_path(place.dir(), &s1.id)).is_none());
    assert!(a.open(&store, &place).get(&s1.id).unwrap().pin.is_none());
    // Again: nothing to do.
    saves::unkeep(&store, &place, &s1.id).unwrap();
}

#[test]
fn a_version_of_0_6_is_kept_with_a_pin_in_the_new_folder() {
    let store = sample();
    let place = Place::of_key(OLD);
    let before = state(&store);
    let mut a = Device::new("d-a", "Desktop");
    let view = a.open(&store, &place);
    let old = view.snapshots.iter().find(|s| !s.is_side() && !s.was_locked() && s.trigger != "manual").unwrap().id.clone();
    let now = Utc::now();
    saves::keep(&store, &place, &view, &old, "first farm", "d-a", now).unwrap();
    let pin_path = v2::pin_path(&v2::line_dir(OLD, "main"), &old);
    let pin: Pin = serde_json::from_slice(&store.peek(&pin_path).unwrap()).unwrap();
    assert_eq!(pin.name, "first farm");
    assert_eq!(state(&store).len(), before.len() + 1);
    let view = a.open(&store, &place);
    assert_eq!(view.get(&old).unwrap().pin.as_ref().unwrap().name, "first farm");
    assert_eq!(place.dir_of(&view, &old), Some(v2::legacy_dir(OLD).as_str()));

    saves::keep(&store, &place, &view, &old, "renamed", "d-a", now).unwrap();
    assert_eq!(serde_json::from_slice::<Pin>(&store.peek(&pin_path).unwrap()).unwrap().name, "renamed");
    saves::unkeep(&store, &place, &old).unwrap();
    assert_eq!(state(&store), before);
}

// ----------------------------------------------------------- recently deleted

/// A line in layout 2 with three versions, the oldest kept.
fn three_versions(store: &Memory, a: &mut Device) -> (Place, Vec<Manifest>) {
    let place = place();
    let mut out = Vec::new();
    for n in 1..=3 {
        a.save("saves/a", format!("v{n}").as_bytes());
        out.push(a.push_at(store, &place, tick()));
    }
    let view = a.open(store, &place);
    saves::keep(store, &place, &view, &out[0].id, "", a.id, Utc::now()).unwrap();
    (place, out)
}

#[test]
fn deleting_and_putting_back_in_layout_2() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = three_versions(&store, &mut a);
    let view = a.open(&store, &place);
    let now = Utc::now();

    // Refused: the newest version, a kept one. Nothing happens.
    store.take();
    let e = saves::trash(&store, &place, &view, &s[2].id, "Desktop", now).unwrap_err();
    assert!(e.to_string().contains("latest version") || e.to_string().contains("最新版本"), "{e}");
    let e = saves::trash(&store, &place, &view, &s[0].id, "Desktop", now).unwrap_err();
    assert!(e.to_string().contains("kept") || e.to_string().contains("保留"), "{e}");
    assert!(saves::trash(&store, &place, &view, "01JZZZZZZZZZZZZZZZZZZZZZZZ", "Desktop", now).is_err());
    assert_eq!(store.take().total(), 0);

    let original = store.peek(&v2::manifest_path(place.dir(), &s[1].id)).unwrap();
    saves::trash(&store, &place, &view, &s[1].id, "Desktop", now).unwrap();
    let record_path = v2::trash_path(place.dir(), &s[1].id);
    let record: Value = serde_json::from_slice(&store.peek(&record_path).unwrap()).unwrap();
    assert_eq!(record["deletedBy"], "Desktop");
    assert_eq!(serde_json::from_value::<DateTime<Utc>>(record["deletedAt"].clone()).unwrap(), now);
    assert_eq!(record["meta"], serde_json::from_slice::<Value>(&original).unwrap());
    assert!(store.peek(&v2::manifest_path(place.dir(), &s[1].id)).is_none());
    // Its pack stays.
    assert!(store.peek(&v2::pack_path(GAME, &s[1].id)).is_some());
    let view = a.open(&store, &place);
    assert!(view.get(&s[1].id).is_none());

    let trashed = saves::trashed(&store, &place).unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(trashed[0].snapshot, serde_json::from_slice::<Manifest>(&original).unwrap().summary());
    assert_eq!((trashed[0].deleted_at, trashed[0].deleted_by.as_str(), trashed[0].dir.as_str()), (now, "Desktop", place.dir()));

    // Back, byte for byte.
    saves::untrash(&store, &place, &s[1].id).unwrap();
    assert_eq!(store.peek(&v2::manifest_path(place.dir(), &s[1].id)).unwrap(), original);
    assert!(store.peek(&record_path).is_none());
    assert!(saves::trashed(&store, &place).unwrap().is_empty());
    assert!(a.open(&store, &place).get(&s[1].id).is_some());
    assert!(saves::untrash(&store, &place, &s[1].id).is_err(), "no longer in recently deleted");

    // Deleted for good: the record goes, the packs wait for the clean-up.
    let view = a.open(&store, &place);
    saves::trash(&store, &place, &view, &s[1].id, "Desktop", now).unwrap();
    saves::purge(&store, &place, &s[1].id).unwrap();
    assert!(store.peek(&record_path).is_none());
    assert!(store.peek(&v2::pack_path(GAME, &s[1].id)).is_some());
    assert!(saves::trashed(&store, &place).unwrap().is_empty());
    assert!(saves::untrash(&store, &place, &s[1].id).is_err());
    saves::purge(&store, &place, &s[1].id).unwrap();
}

#[test]
fn recently_deleted_lists_newest_first_and_skips_what_is_back() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let place = place();
    let mut s = Vec::new();
    for n in 1..=4 {
        a.save("saves/a", format!("v{n}").as_bytes());
        s.push(a.push_at(&store, &place, tick()));
    }
    let view = a.open(&store, &place);
    let t0 = Utc::now();
    saves::trash(&store, &place, &view, &s[0].id, "Desktop", t0).unwrap();
    saves::trash(&store, &place, &view, &s[2].id, "Deck", t0 + Duration::minutes(5)).unwrap();
    let order: Vec<String> = saves::trashed(&store, &place).unwrap().into_iter().map(|t| t.snapshot.id).collect();
    assert_eq!(order, [s[2].id.clone(), s[0].id.clone()]);

    // Another device put s0 back and hasn't removed the record yet: it is in
    // the history, not in "recently deleted", and can't be purged.
    let meta: Value = serde_json::from_slice(&store.peek(&v2::trash_path(place.dir(), &s[0].id)).unwrap()).unwrap();
    store.put(&v2::manifest_path(place.dir(), &s[0].id), &serde_json::to_vec(&meta["meta"]).unwrap());
    let order: Vec<String> = saves::trashed(&store, &place).unwrap().into_iter().map(|t| t.snapshot.id).collect();
    assert_eq!(order, [s[2].id.clone()]);
    assert!(saves::purge(&store, &place, &s[0].id).is_err());
    assert!(store.peek(&v2::trash_path(place.dir(), &s[0].id)).is_some());

    // A record that can't be understood is skipped.
    store.put(&v2::trash_path(place.dir(), "01JZZZZZZZZZZZZZZZZZZZZZZZ"), b"{ not json");
    assert_eq!(saves::trashed(&store, &place).unwrap().len(), 1);
}

#[test]
fn an_interrupted_deletion_leaves_the_version_in_the_history() {
    let base = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = three_versions(&base, &mut a);
    let now = Utc::now();
    let view = saves::open(&base, &place, &mut LineCache::default()).unwrap();

    let reference = copy(&base);
    saves::trash(&reference, &place, &view, &s[1].id, "Desktop", now).unwrap();
    for (kind, at) in [(Kind::Write, 1), (Kind::Remove, 1)] {
        let store = copy(&base);
        assert!(saves::trash(&Flaky::new(&store, kind, at), &place, &view, &s[1].id, "Desktop", now).is_err());
        let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
        assert!(view.get(&s[1].id).is_some(), "{kind:?}: still in the history");
        assert!(saves::trashed(&store, &place).unwrap().is_empty(), "{kind:?}: not in recently deleted");
        saves::trash(&store, &place, &view, &s[1].id, "Desktop", now).unwrap();
        assert_eq!(state(&store), state(&reference), "{kind:?}");
    }

    // Putting back, interrupted.
    let trashed = copy(&reference);
    saves::untrash(&reference, &place, &s[1].id).unwrap();
    for (kind, at) in [(Kind::Write, 1), (Kind::Remove, 1)] {
        let store = copy(&trashed);
        assert!(saves::untrash(&Flaky::new(&store, kind, at), &place, &s[1].id).is_err());
        assert_complete(&store, &place);
        saves::untrash(&store, &place, &s[1].id).unwrap();
        assert_eq!(state(&store), state(&reference), "{kind:?}");
    }
}

#[test]
fn deleting_in_a_folder_of_0_6_is_what_0_6_sees() {
    let store = sample();
    let place = Place::of_key(OLD);
    let old_dir = v2::legacy_dir(OLD);
    let (history, trash) = seen_by_0_6(&store, OLD);
    assert!(trash.is_empty());
    let mut a = Device::new("d-a", "Desktop");
    let view = a.open(&store, &place);
    let head = view.head().unwrap().id.clone();

    // The head of 0.6 is the head of the line.
    assert!(saves::trash(&store, &place, &view, &head, "Desktop", Utc::now()).is_err());

    let x = history[1].clone();
    let original = store.peek(&format!("{old_dir}/{x}.json")).unwrap();
    let now = Utc::now();
    saves::trash(&store, &place, &view, &x, "Desktop", now).unwrap();
    assert!(store.peek(&format!("games/{OLD}/trash/{x}.json")).is_some());
    assert!(store.peek(&format!("{old_dir}/{x}.zip")).is_some(), "the zip stays beside");
    let (h, t) = seen_by_0_6(&store, OLD);
    assert_eq!(h, history.iter().filter(|id| **id != x).cloned().collect::<Vec<_>>());
    assert_eq!(t, std::slice::from_ref(&x));
    let repo = Repo::new(Shared(&store));
    assert_eq!(repo.list_trash(OLD).unwrap()[0].deleted_by, "Desktop");
    let listed = saves::trashed(&store, &place).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].dir, old_dir);
    assert_eq!(listed[0].snapshot.id, x);
    assert_eq!(listed[0].snapshot.vv, view.get(&x).unwrap().vv);

    // Back: the file of 0.6, byte for byte.
    saves::untrash(&store, &place, &x).unwrap();
    assert_eq!(store.peek(&format!("{old_dir}/{x}.json")).unwrap(), original);
    assert_eq!(seen_by_0_6(&store, OLD), (history.clone(), vec![]));
    assert_eq!(a.open(&store, &place).get(&x), view.get(&x));

    // For good: the zip too, as 0.6 does it.
    let view = a.open(&store, &place);
    saves::trash(&store, &place, &view, &x, "Desktop", now).unwrap();
    saves::purge(&store, &place, &x).unwrap();
    assert!(store.peek(&format!("{old_dir}/{x}.zip")).is_none());
    assert!(store.peek(&format!("games/{OLD}/trash/{x}.json")).is_none());
    let (h, t) = seen_by_0_6(&store, OLD);
    assert_eq!(h, history.iter().filter(|id| **id != x).cloned().collect::<Vec<_>>());
    assert!(t.is_empty());
    assert!(saves::trashed(&store, &place).unwrap().is_empty());
}

#[test]
fn what_a_device_on_0_6_deleted_can_be_put_back_here() {
    let store = sample();
    let place = Place::of_key(OLD);
    let (history, _) = seen_by_0_6(&store, OLD);
    let y = history[2].clone();
    let repo = Repo::new(Shared(&store));
    repo.trash(OLD, &y, "PC-B").unwrap();

    let listed = saves::trashed(&store, &place).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].snapshot.id.as_str(), listed[0].deleted_by.as_str()), (y.as_str(), "PC-B"));
    saves::untrash(&store, &place, &y).unwrap();
    assert_eq!(seen_by_0_6(&store, OLD), (history.clone(), vec![]));
    let mut cache = LineCache::default();
    let view = saves::open(&store, &place, &mut cache).unwrap();
    let m = saves::manifest(&store, &place, &view, &y).unwrap();
    assert_eq!(m.id, y);
    assert_eq!(m.v, 1);
}

#[test]
fn a_half_purged_version_of_0_6_is_gone() {
    let store = sample();
    let place = Place::of_key(OLD);
    let old_dir = v2::legacy_dir(OLD);
    let (history, _) = seen_by_0_6(&store, OLD);
    let mut a = Device::new("d-a", "Desktop");
    let view = a.open(&store, &place);
    let (x, y) = (history[1].clone(), history[2].clone());
    saves::trash(&store, &place, &view, &x, "Desktop", Utc::now()).unwrap();
    saves::trash(&store, &place, &view, &y, "Desktop", Utc::now()).unwrap();

    // A purge on 0.6 broke off after the zip.
    store.remove(&format!("{old_dir}/{x}.zip")).unwrap();
    store.remove(&format!("{old_dir}/{y}.zip")).unwrap();
    assert!(saves::untrash(&store, &place, &x).is_err());
    assert!(store.peek(&format!("{old_dir}/{x}.json")).is_none(), "nothing is put back without its zip");
    assert!(saves::trashed(&store, &place).unwrap().is_empty());
    assert!(store.peek(&format!("games/{OLD}/trash/{x}.json")).is_none());
    assert!(store.peek(&format!("games/{OLD}/trash/{y}.json")).is_none());
    assert!(saves::untrash(&store, &place, &y).is_err());
}

#[test]
fn an_interrupted_purge_in_a_folder_of_0_6_carries_on() {
    let base = sample();
    let place = Place::of_key(OLD);
    let (history, _) = seen_by_0_6(&base, OLD);
    let view = saves::open(&base, &place, &mut LineCache::default()).unwrap();
    let x = history[1].clone();
    saves::trash(&base, &place, &view, &x, "Desktop", Utc::now()).unwrap();

    let reference = copy(&base);
    saves::purge(&reference, &place, &x).unwrap();
    for at in [1, 2] {
        let store = copy(&base);
        assert!(saves::purge(&Flaky::new(&store, Kind::Remove, at), &place, &x).is_err());
        saves::purge(&store, &place, &x).unwrap();
        assert_eq!(state(&store), state(&reference), "{at}");
        assert_eq!(seen_by_0_6(&store, OLD), seen_by_0_6(&reference, OLD));
    }
}

// ----------------------------------------------------------------- clean-up

/// A game that has a history in both layouts: the sample of 0.6 for
/// `test-game`, then five large uploads made 50 to 46 days ago, two of them
/// in "recently deleted" (one long enough to go), two old snapshots of 0.6
/// deleted the same way, and two packs no manifest refers to.
struct History {
    store: Memory,
    place: Place,
    now: DateTime<Utc>,
    s: Vec<Manifest>,
    old: Vec<String>,
    orphans: Vec<String>,
}

fn history() -> History {
    let store = sample();
    let place = Place::of_key(OLD);
    let now = Utc::now();
    let mut a = Device::new("d-a", "Desktop");

    // s1 all six files; s2 changes region0; s3 region1; s4 region0 and 1
    // again; s5 region2. So s3's own pack is used by s3 alone, and s2's by
    // s2 and s3.
    let mut s = Vec::new();
    a.large_save(1);
    s.push(a.push_at(&store, &place, now - Duration::days(50)));
    a.save("saves/world/region0", &bytes(200, 1024));
    s.push(a.push_at(&store, &place, now - Duration::days(49)));
    a.save("saves/world/region1", &bytes(201, 1024));
    s.push(a.push_at(&store, &place, now - Duration::days(48)));
    a.save("saves/world/region0", &bytes(202, 1024));
    a.save("saves/world/region1", &bytes(203, 1024));
    s.push(a.push_at(&store, &place, now - Duration::days(47)));
    a.save("saves/world/region2", &bytes(204, 1024));
    s.push(a.push_at(&store, &place, now - Duration::days(46)));
    assert_eq!(s[2].packs(), BTreeSet::from([s[0].id.clone(), s[1].id.clone(), s[2].id.clone()]));
    assert!(!s[3].packs().contains(&s[2].id) && !s[4].packs().contains(&s[2].id));
    assert!(s[4].packs().contains(&s[0].id));

    let view = a.open(&store, &place);
    let (old_history, _) = seen_by_0_6(&store, OLD);
    let old = vec![old_history[0].clone(), old_history[1].clone()];
    saves::trash(&store, &place, &view, &s[1].id, "Desktop", now - Duration::days(5)).unwrap();
    saves::trash(&store, &place, &view, &s[2].id, "Desktop", now - Duration::days(TRASH_DAYS + 1)).unwrap();
    saves::trash(&store, &place, &view, &old[0], "Desktop", now - Duration::days(TRASH_DAYS + 5)).unwrap();
    saves::trash(&store, &place, &view, &old[1], "Desktop", now - Duration::days(2)).unwrap();

    // Uploads that never got their manifest: one long ago, one three days ago.
    let orphan = |days: i64, n: u128| ulid::Ulid::from_parts((now - Duration::days(days)).timestamp_millis() as u64, n).to_string();
    let orphans = vec![orphan(30, 1), orphan(3, 2)];
    for o in &orphans {
        store.put(&v2::pack_path(OLD, o), b"PK\x05\x06 an upload that broke off");
    }
    History { store, place, now, s, old, orphans }
}

fn small_policy() -> retention::Policy {
    retention::Policy { keep_recent: 2, keep_daily_days: 0, device_days: retention::DEVICE_DAYS }
}

/// This device synced s5 last, another one s4.
fn bases(h: &History) -> Vec<DeviceBase> {
    vec![
        DeviceBase { device_id: "d-a".into(), snapshot: h.s[4].id.clone(), seen_at: h.now },
        DeviceBase { device_id: "d-b".into(), snapshot: h.s[3].id.clone(), seen_at: h.now },
    ]
}

fn clean(store: &dyn Store, h: &History, cache: &mut LineCache) -> Result<saves::Cleaned> {
    let mut lines = [Open { place: &h.place, cache, bases: bases(h) }];
    saves::clean(store, &mut lines, &small_policy(), Duration::days(packs::GRACE_DAYS), h.now, "d-a")
}

#[test]
fn clean_up_follows_retention_but_never_in_a_folder_of_0_6() {
    let h = history();
    let store = &h.store;
    let (old_history, old_trash) = seen_by_0_6(store, OLD);
    assert_eq!(old_trash, [h.old[1].clone(), h.old[0].clone()], "newest deletion first");
    let old_files: BTreeMap<String, Vec<u8>> = state(store).into_iter().filter(|(p, _)| p.starts_with(&format!("{}/", v2::legacy_dir(OLD)))).collect();

    let mut cache = LineCache::default();
    let view = saves::open(store, &h.place, &mut cache).unwrap();
    let plan = retention::plan(&view, &bases(&h), &small_policy(), h.now);
    let in_old: Vec<&String> = plan.delete.iter().filter(|id| h.place.dir_of(&view, id) == Some(v2::legacy_dir(OLD).as_str())).collect();
    assert!(!in_old.is_empty(), "retention would take old snapshots too");
    assert!(plan.delete.contains(&h.s[0].id));

    // Another device keeps s1 right after this one listed the line: a kept
    // save stays, whatever the listing said.
    let pin = v2::pin_path(h.place.dir(), &h.s[0].id);
    let meanwhile = Meanwhile::new(store, h.place.dir(), vec![(pin.clone(), b"{\"name\":\"late\"}".to_vec())]);
    let cleaned = clean(&meanwhile, &h, &mut cache).unwrap();
    assert!(meanwhile.done.load(Ordering::SeqCst), "the pin was written");
    assert!(cleaned.snapshots.is_empty());
    assert!(store.peek(&v2::manifest_path(h.place.dir(), &h.s[0].id)).is_some());
    assert!(store.peek(&pin).is_some());

    // "Recently deleted": what has been there 30 days goes, in both layouts.
    assert_eq!(cleaned.purged, [h.s[2].id.clone(), h.old[0].clone()]);
    assert!(store.peek(&v2::trash_path(h.place.dir(), &h.s[2].id)).is_none());
    assert!(store.peek(&v2::trash_path(h.place.dir(), &h.s[1].id)).is_some());
    let (history_now, trash_now) = seen_by_0_6(store, OLD);
    assert_eq!(history_now, old_history);
    assert_eq!(trash_now, [h.old[1].clone()]);
    for (path, data) in &old_files {
        if path.ends_with(&format!("{}.zip", h.old[0])) {
            assert!(store.peek(path).is_none(), "purged as 0.6 does: the zip too");
        } else {
            assert_eq!(store.peek(path).as_ref(), Some(data), "{path}");
        }
    }

    // Packs: s1's own pack is borrowed by every snapshot left, s2's by the
    // version in "recently deleted"; s3's own pack and the old upload that
    // broke off are used by nothing; the young one waits.
    assert_eq!(cleaned.packs, {
        let mut p = vec![h.s[2].id.clone(), h.orphans[0].clone()];
        p.sort();
        p
    });
    for kept in [&h.s[0].id, &h.s[1].id, &h.s[3].id, &h.s[4].id, &h.orphans[1]] {
        assert!(store.peek(&v2::pack_path(OLD, kept)).is_some(), "{kept}");
    }
    assert_complete(store, &h.place);

    // Kept no more: of what retention named, only what is in layout 2 goes.
    saves::unkeep(store, &h.place, &h.s[0].id).unwrap();
    let cleaned = clean(store, &h, &mut cache).unwrap();
    let expect: Vec<String> = plan.delete.iter().filter(|id| !in_old.contains(id)).cloned().collect();
    assert_eq!(cleaned.snapshots, expect);
    assert_eq!(cleaned.snapshots, [h.s[0].id.clone()]);
    assert!(store.peek(&v2::manifest_path(h.place.dir(), &h.s[0].id)).is_none());
    assert!(cleaned.purged.is_empty() && cleaned.packs.is_empty());
    assert_complete(store, &h.place);

    // Run again: nothing more to do.
    let again = clean(store, &h, &mut cache).unwrap();
    assert_eq!(again, saves::Cleaned::default());

    // Everything left can still be fetched, s2 once it is put back.
    saves::untrash(store, &h.place, &h.s[1].id).unwrap();
    let mut b = Device::new("d-b", "Deck");
    let view = b.open(store, &h.place);
    for m in [&h.s[1], &h.s[3], &h.s[4]] {
        let dest = b.dest(&m.id);
        saves::fetch(store, &h.place, &view, m, None, &dest, &b.staging()).unwrap();
        assert_eq!(tree(&dest).len(), 6);
    }
}

#[test]
fn an_interrupted_clean_up_carries_on() {
    let h = history();
    let reference = copy(&h.store);
    let cleaned = clean(&reference, &h, &mut LineCache::default()).unwrap();
    reference.take();
    clean(&reference, &h, &mut LineCache::default()).unwrap();
    assert_eq!(reference.take().remove, 0, "a second run has nothing to remove");

    // Every remove the clean-up made, broken off in turn.
    let removes = (cleaned.snapshots.len() + 2 + 1 + cleaned.packs.len()) as u32;
    for at in 1..=removes {
        let store = copy(&h.store);
        assert!(clean(&Flaky::new(&store, Kind::Remove, at), &h, &mut LineCache::default()).is_err(), "{at}");
        assert_complete(&store, &h.place);
        clean(&store, &h, &mut LineCache::default()).unwrap();
        assert_eq!(state(&store), state(&reference), "{at}");
    }
    // One more would have been none.
    let store = copy(&h.store);
    clean(&Flaky::new(&store, Kind::Remove, removes + 1), &h, &mut LineCache::default()).unwrap();
    assert_eq!(state(&store), state(&reference));
}

#[test]
fn a_line_with_a_manifest_this_build_cannot_read_loses_nothing() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    let now = Utc::now();
    for n in 0..4 {
        a.save("saves/a", format!("v{n}").as_bytes());
        a.push_at(&store, &place, now - Duration::days(40 - n));
    }
    let orphan = ulid::Ulid::from_parts((now - Duration::days(30)).timestamp_millis() as u64, 9).to_string();
    store.put(&v2::pack_path(GAME, &orphan), b"PK");
    let unknown = ulid::Ulid::from_parts((now - Duration::days(1)).timestamp_millis() as u64, 7).to_string();
    store.put(&v2::manifest_path(place.dir(), &unknown), b"{\"v\": 9, \"encrypted\": \"...\"}");
    let before = store.paths();

    let mut cache = LineCache::default();
    let bases = vec![];
    let mut lines = [Open { place: &place, cache: &mut cache, bases }];
    let policy = retention::Policy { keep_recent: 1, keep_daily_days: 0, device_days: 90 };
    let cleaned = saves::clean(&store, &mut lines, &policy, Duration::days(7), now, "d-a").unwrap();
    assert_eq!(cleaned, saves::Cleaned::default());
    for p in &before {
        assert!(store.peek(p).is_some(), "{p}");
    }

    // A record of "recently deleted" that can't be understood keeps the
    // packs as well.
    store.remove(&v2::manifest_path(place.dir(), &unknown)).unwrap();
    store.put(&v2::trash_path(place.dir(), &unknown), b"{\"deletedAt\": 5}");
    let mut lines = [Open { place: &place, cache: &mut cache, bases: vec![] }];
    let cleaned = saves::clean(&store, &mut lines, &policy, Duration::days(7), now, "d-a").unwrap();
    assert_eq!(cleaned.snapshots.len(), 3);
    assert!(cleaned.packs.is_empty());
    assert!(store.peek(&v2::pack_path(GAME, &orphan)).is_some());
}

#[test]
fn a_manifest_written_during_the_clean_up_keeps_its_pack() {
    let now = Utc::now();
    let setup = || {
        let store = Memory::new();
        let place = place();
        let mut a = Device::new("d-a", "Desktop");
        a.save("saves/a", b"v1");
        a.push_at(&store, &place, now - Duration::days(20));
        // Another device uploaded a snapshot it made 10 days ago while
        // offline; its manifest is on the way.
        let b = Device::new("d-b", "Deck");
        b.save("saves/a", b"from the deck");
        let late = b.draft(&place, Vv::next([], "d-b", 0), None, false, now - Duration::days(10), None);
        saves::stage(&late, &b.files(), &b.queue()).unwrap();
        store.put(&v2::pack_path(GAME, &late.id), &std::fs::read(b.queue().join(format!("{}.zip", late.id))).unwrap());
        (store, place, late)
    };

    let (store, place, late) = setup();
    let manifest = (v2::manifest_path(place.dir(), &late.id), serde_json::to_vec(&late).unwrap());
    let meanwhile = Meanwhile::new(&store, &v2::packs_dir(GAME), vec![manifest]);
    let mut cache = LineCache::default();
    let mut lines = [Open { place: &place, cache: &mut cache, bases: vec![] }];
    let cleaned = saves::clean(&meanwhile, &mut lines, &retention::Policy::default(), Duration::days(7), now, "d-a").unwrap();
    assert!(cleaned.packs.is_empty());
    assert!(store.peek(&v2::pack_path(GAME, &late.id)).is_some());
    assert_complete(&store, &place);
    // The listing again saw the new manifest: the index is written with it.
    let index: LineIndex = serde_json::from_slice(&store.peek(&v2::index_path(place.dir())).unwrap()).unwrap();
    assert!(index.snapshots.iter().any(|s| s.id == late.id));

    // Without the manifest, the pack would have gone.
    let (store, place, late) = setup();
    let mut cache = LineCache::default();
    let mut lines = [Open { place: &place, cache: &mut cache, bases: vec![] }];
    let cleaned = saves::clean(&store, &mut lines, &retention::Policy::default(), Duration::days(7), now, "d-a").unwrap();
    assert_eq!(cleaned.packs, std::slice::from_ref(&late.id));
}

#[test]
fn requests_of_a_quiet_clean_up() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    for n in 0..3 {
        a.save("saves/a", format!("v{n}").as_bytes());
        a.push_at(&store, &place, tick());
    }
    a.open(&store, &place);
    store.take();
    let mut lines = [Open { place: &place, cache: &mut a.cache, bases: vec![] }];
    let cleaned = saves::clean(&store, &mut lines, &retention::Policy::default(), Duration::days(7), Utc::now(), "d-a").unwrap();
    assert_eq!(cleaned, saves::Cleaned::default());
    // The line, its "recently deleted", the game's packs.
    assert_eq!(store.take(), counts(3, 0, 0, 0, 0, 0));

    // A manifest another device wrote without its index entry: the index
    // (changed since this device last read it) is read, then the manifest it
    // lacks, and the index is written again.
    let mut b = Device::new("d-b", "Deck");
    b.save("saves/a", b"deck");
    let view = b.open(&store, &place);
    let m = b.make(&place, &view, &[view.head().unwrap().vv.clone()], None, tick());
    saves::stage(&m, &b.files(), &b.queue()).unwrap();
    store.put(&v2::pack_path(GAME, &m.id), &std::fs::read(b.queue().join(format!("{}.zip", m.id))).unwrap());
    line::write_manifest(&store, place.dir(), &m).unwrap();
    store.take();
    let mut lines = [Open { place: &place, cache: &mut a.cache, bases: vec![] }];
    saves::clean(&store, &mut lines, &retention::Policy::default(), Duration::days(7), Utc::now(), "d-a").unwrap();
    assert_eq!(store.take(), counts(3, 2, 0, 1, 0, 0));
    let index: LineIndex = serde_json::from_slice(&store.peek(&v2::index_path(place.dir())).unwrap()).unwrap();
    assert!(index.snapshots.iter().any(|s| s.id == m.id));
    // And then it is quiet again.
    let mut lines = [Open { place: &place, cache: &mut a.cache, bases: vec![] }];
    saves::clean(&store, &mut lines, &retention::Policy::default(), Duration::days(7), Utc::now(), "d-a").unwrap();
    assert_eq!(store.take(), counts(3, 0, 0, 0, 0, 0));
}

// ------------------------------------------------------------- the requests
//
// One test per row of the table in `cloud::saves`.

/// A line in layout 2 with `n` small versions uploaded by `a`.
fn versions(store: &Memory, a: &mut Device, n: usize) -> (Place, Vec<Manifest>) {
    let place = place();
    let mut out = Vec::new();
    for k in 1..=n {
        a.save("saves/a", format!("v{k}").as_bytes());
        out.push(a.push_at(store, &place, tick()));
    }
    (place, out)
}

/// The sample of 0.6 with one upload of layout 2 on `test-game`.
fn sample_with_an_upload() -> Memory {
    let old = sample();
    let place = Place::of_key(OLD);
    let mut c = Device::new("d-c", "Laptop");
    c.save("install/save/slot1.sav", b"new");
    let view = c.open(&old, &place);
    let head = view.head().unwrap();
    c.push(&old, &place, Action::Push { bases: vec![head.vv.clone()], parent: Some(head.id.clone()) });
    old
}

#[test]
fn requests_open_with_nothing_new() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, _) = versions(&store, &mut a, 2);
    let mut b = Device::new("d-b", "Deck");
    b.open(&store, &place);
    store.take();
    b.open(&store, &place);
    assert_eq!(store.take(), counts(1, 0, 0, 0, 0, 0));

    // A line that reads the folder of 0.6 as well: one listing more.
    let old = sample_with_an_upload();
    let place = Place::of_key(OLD);
    let mut d = Device::new("d-d", "Deck");
    d.open(&old, &place);
    old.take();
    d.open(&old, &place);
    assert_eq!(old.take(), counts(2, 0, 0, 0, 0, 0));
}

#[test]
fn requests_open_with_new_snapshots() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, _) = versions(&store, &mut a, 1);
    let mut b = Device::new("d-b", "Deck");
    b.open(&store, &place);
    a.save("saves/a", b"2");
    a.push_at(&store, &place, tick());
    a.save("saves/a", b"3");
    a.push_at(&store, &place, tick());
    store.take();
    assert_eq!(b.open(&store, &place).snapshots.len(), 3);
    assert_eq!(store.take(), counts(1, 1, 0, 0, 0, 0));

    // Never seen, two folders: the listings and the index, which covers the
    // old folder too.
    let old = sample_with_an_upload();
    let place = Place::of_key(OLD);
    let mut d = Device::new("d-d", "Deck");
    old.take();
    assert_eq!(d.open(&old, &place).snapshots.len(), 12);
    assert_eq!(old.take(), counts(2, 1, 0, 0, 0, 0));
}

#[test]
fn requests_publish_with_all_files_in_own_packs() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.save("saves/a", b"1");
    a.save("saves/big", &bytes(3, 9 * 1024));
    let m = a.draft(&place, first("d-a"), None, false, tick(), None);
    assert_eq!(m.packs().len(), 2);
    let (files, q) = (a.files(), a.queue());
    saves::stage(&m, &files, &q).unwrap();
    a.open(&store, &place);
    store.take();
    saves::publish(&store, &place, &mut a.cache, &m, &q, &files, a.id).unwrap();
    assert_eq!(store.take(), counts(0, 0, 0, 2, 2, 0), "one upload per pack, manifest, index");
}

#[test]
fn requests_publish_with_files_from_older_packs() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.save("saves/a", b"1");
    a.save("saves/big", &bytes(3, 9 * 1024));
    let split = a.push_at(&store, &place, tick());
    let q = a.queue();

    a.save("saves/a", b"2");
    let view = a.open(&store, &place);
    let delta = a.make(&place, &view, std::slice::from_ref(&split.vv), Some(split.id.clone()), tick());
    assert_eq!(delta.packs(), BTreeSet::from([delta.id.clone(), format!("{}-2", split.id)]));
    let files = a.files();
    saves::stage(&delta, &files, &q).unwrap();
    store.take();
    saves::publish(&store, &place, &mut a.cache, &delta, &q, &files, a.id).unwrap();
    assert_eq!(store.take(), counts(1, 0, 0, 2, 1, 0));
    a.synced_to(&delta);

    // Nothing changed in a large save: a manifest and no pack.
    let view = a.open(&store, &place);
    let same = a.make(&place, &view, std::slice::from_ref(&delta.vv), Some(delta.id.clone()), tick());
    assert!(same.files.iter().all(|f| f.pack.is_some()));
    let files = a.files();
    saves::stage(&same, &files, &q).unwrap();
    store.take();
    saves::publish(&store, &place, &mut a.cache, &same, &q, &files, a.id).unwrap();
    assert_eq!(store.take(), counts(1, 0, 0, 2, 0, 0));
}

#[test]
fn requests_of_a_sync_after_playing_and_a_check_before() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 1);
    a.save("saves/a", b"played");
    let q = a.queue();
    let files = a.files();

    // After playing, a small save: list, pack, manifest, index.
    store.take();
    let view = a.open(&store, &place);
    let m = a.make(&place, &view, &[s[0].vv.clone()], Some(s[0].id.clone()), tick());
    saves::stage(&m, &files, &q).unwrap();
    saves::publish(&store, &place, &mut a.cache, &m, &q, &files, a.id).unwrap();
    let c = store.take();
    assert_eq!(c, counts(1, 0, 0, 2, 1, 0));
    assert_eq!(c.total(), 4);

    // Before playing, nothing new: one listing.
    a.open(&store, &place);
    assert_eq!(store.take(), counts(1, 0, 0, 0, 0, 0));
}

#[test]
fn requests_manifest() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    store.take();
    assert_eq!(saves::manifest(&store, &place, &view, &s[0].id).unwrap(), s[0]);
    assert_eq!(store.take(), counts(0, 1, 0, 0, 0, 0));
}

#[test]
fn requests_fetch() {
    let store = Memory::new();
    let place = place();
    let mut a = Device::new("d-a", "Desktop");
    a.large_save(1);
    let first = a.push_at(&store, &place, tick());
    a.save("saves/world/region3", &bytes(55, 1024));
    let second = a.push_at(&store, &place, tick());
    let view = a.open(&store, &place);

    store.take();
    saves::fetch(&store, &place, &view, &second, None, &a.dest("all"), &a.staging()).unwrap();
    assert_eq!(store.take(), counts(0, 0, 2, 0, 0, 0), "one download per pack");
    let only = ["saves/world/region3".to_string()];
    saves::fetch(&store, &place, &view, &second, Some(&only), &a.dest("one"), &a.staging()).unwrap();
    assert_eq!(store.take(), counts(0, 0, 1, 0, 0, 0), "only the pack that holds it");
    saves::fetch(&store, &place, &view, &first, None, &a.dest("first"), &a.staging()).unwrap();
    assert_eq!(store.take(), counts(0, 0, 1, 0, 0, 0));
}

#[test]
fn requests_keep() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    store.take();
    saves::keep(&store, &place, &view, &s[0].id, "one", "d-a", Utc::now()).unwrap();
    assert_eq!(store.take(), counts(0, 0, 0, 1, 0, 0));

    // Renaming: the pin is read first.
    let view = a.open(&store, &place);
    store.take();
    saves::keep(&store, &place, &view, &s[0].id, "two", "d-a", Utc::now()).unwrap();
    assert_eq!(store.take(), counts(0, 1, 0, 1, 0, 0));
}

#[test]
fn requests_unkeep() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    saves::keep(&store, &place, &view, &s[0].id, "one", "d-a", Utc::now()).unwrap();
    store.take();
    saves::unkeep(&store, &place, &s[0].id).unwrap();
    assert_eq!(store.take(), counts(0, 0, 0, 0, 0, 1));

    // One remove per folder of the line.
    let old = sample();
    let place = Place::of_key(OLD);
    let mut c = Device::new("d-c", "Laptop");
    let view = c.open(&old, &place);
    let id = view.snapshots[0].id.clone();
    saves::keep(&old, &place, &view, &id, "one", "d-c", Utc::now()).unwrap();
    old.take();
    saves::unkeep(&old, &place, &id).unwrap();
    assert_eq!(old.take(), counts(0, 0, 0, 0, 0, 2));
}

#[test]
fn requests_trash() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    store.take();
    saves::trash(&store, &place, &view, &s[0].id, "Desktop", Utc::now()).unwrap();
    assert_eq!(store.take(), counts(0, 1, 0, 1, 0, 1));

    // The same in a folder of 0.6.
    let old = sample();
    let place = Place::of_key(OLD);
    let (history, _) = seen_by_0_6(&old, OLD);
    let view = saves::open(&old, &place, &mut LineCache::default()).unwrap();
    old.take();
    saves::trash(&old, &place, &view, &history[1], "Desktop", Utc::now()).unwrap();
    assert_eq!(old.take(), counts(0, 1, 0, 1, 0, 1));
}

#[test]
fn requests_trashed() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 4);
    let view = a.open(&store, &place);
    store.take();
    assert!(saves::trashed(&store, &place).unwrap().is_empty());
    assert_eq!(store.take(), counts(1, 0, 0, 0, 0, 0), "nothing deleted: one listing per folder");

    saves::trash(&store, &place, &view, &s[0].id, "Desktop", Utc::now()).unwrap();
    saves::trash(&store, &place, &view, &s[1].id, "Desktop", Utc::now()).unwrap();
    store.take();
    assert_eq!(saves::trashed(&store, &place).unwrap().len(), 2);
    assert_eq!(store.take(), counts(2, 2, 0, 0, 0, 0), "and the folder itself, and each record");

    // Two folders, a version deleted from the old one.
    let old = sample();
    let place = Place::of_key(OLD);
    let (history, _) = seen_by_0_6(&old, OLD);
    old.take();
    assert!(saves::trashed(&old, &place).unwrap().is_empty());
    assert_eq!(old.take(), counts(2, 0, 0, 0, 0, 0));
    let view = saves::open(&old, &place, &mut LineCache::default()).unwrap();
    saves::trash(&old, &place, &view, &history[1], "Desktop", Utc::now()).unwrap();
    old.take();
    assert_eq!(saves::trashed(&old, &place).unwrap().len(), 1);
    assert_eq!(old.take(), counts(3, 1, 0, 0, 0, 0));
}

#[test]
fn requests_untrash() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    saves::trash(&store, &place, &view, &s[0].id, "Desktop", Utc::now()).unwrap();
    store.take();
    saves::untrash(&store, &place, &s[0].id).unwrap();
    assert_eq!(store.take(), counts(0, 1, 0, 1, 0, 1));

    // In a folder of 0.6: and the listing that finds the zip.
    let old = sample();
    let place = old_only(OLD);
    let (history, _) = seen_by_0_6(&old, OLD);
    let view = saves::open(&old, &place, &mut LineCache::default()).unwrap();
    saves::trash(&old, &place, &view, &history[1], "Desktop", Utc::now()).unwrap();
    old.take();
    saves::untrash(&old, &place, &history[1]).unwrap();
    assert_eq!(old.take(), counts(1, 1, 0, 1, 0, 1));
}

#[test]
fn requests_purge() {
    let store = Memory::new();
    let mut a = Device::new("d-a", "Desktop");
    let (place, s) = versions(&store, &mut a, 2);
    let view = a.open(&store, &place);
    saves::trash(&store, &place, &view, &s[0].id, "Desktop", Utc::now()).unwrap();
    store.take();
    saves::purge(&store, &place, &s[0].id).unwrap();
    assert_eq!(store.take(), counts(0, 1, 0, 0, 0, 1));

    // In a folder of 0.6: the zip first.
    let old = sample();
    let place = old_only(OLD);
    let (history, _) = seen_by_0_6(&old, OLD);
    let view = saves::open(&old, &place, &mut LineCache::default()).unwrap();
    saves::trash(&old, &place, &view, &history[1], "Desktop", Utc::now()).unwrap();
    old.take();
    saves::purge(&old, &place, &history[1]).unwrap();
    assert_eq!(old.take(), counts(0, 1, 0, 0, 0, 2));
}

#[test]
fn requests_in_a_line_of_several_folders() {
    // The record or pin is looked for in the line's folders in order.
    let store = sample();
    let place = Place::of_key(OLD);
    let (history, _) = seen_by_0_6(&store, OLD);
    let y = history[2].clone();
    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    saves::trash(&store, &place, &view, &y, "Desktop", Utc::now()).unwrap();
    store.take();
    saves::untrash(&store, &place, &y).unwrap();
    assert_eq!(store.take(), counts(1, 2, 0, 1, 0, 1), "the new folder's trash first");

    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    saves::trash(&store, &place, &view, &y, "Desktop", Utc::now()).unwrap();
    store.take();
    saves::purge(&store, &place, &y).unwrap();
    assert_eq!(store.take(), counts(0, 3, 0, 0, 0, 2), "both trashes, then the manifest");

    // The pins are in the first folder: renaming costs what the table says.
    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    let id = history[0].clone();
    saves::keep(&store, &place, &view, &id, "one", "d-a", Utc::now()).unwrap();
    let view = saves::open(&store, &place, &mut LineCache::default()).unwrap();
    store.take();
    saves::keep(&store, &place, &view, &id, "two", "d-a", Utc::now()).unwrap();
    assert_eq!(store.take(), counts(0, 1, 0, 1, 0, 0));
}

#[test]
fn nothing_is_written_into_a_folder_of_0_6() {
    let store = sample();
    let place = old_only(OLD);
    let mut cache = LineCache::default();
    let view = saves::open(&store, &place, &mut cache).unwrap();
    let id = view.snapshots[0].id.clone();
    let before = state(&store);
    assert!(saves::keep(&store, &place, &view, &id, "x", "d-a", Utc::now()).is_err());
    let a = Device::new("d-a", "Desktop");
    a.save("saves/a", b"1");
    let m = a.draft(&place, first("d-a"), None, false, tick(), None);
    saves::stage(&m, &a.files(), &a.queue()).unwrap();
    assert!(saves::publish(&store, &place, &mut cache, &m, &a.queue(), &[], "d-a").is_err());
    assert_eq!(state(&store), before);
}
