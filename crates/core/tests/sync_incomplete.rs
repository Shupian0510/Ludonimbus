//! Versions that miss a content pack, through the whole sync
//! (docs/plan-v2.md, sections 3.4, 3.5 and 5.4): the device record told at
//! once when a large save's base changes, a waiting version made whole
//! before it goes up, one that can't be (listed once on this device), a
//! broken newest version found, never taken, repaired by a device that has
//! the files, and let go by its writer when nobody can. One test, because
//! the data directory is chosen through an environment variable.

use anyhow::Result;
use chrono::{Duration, Utc};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::{v2, Cloud, DeviceInfo, Unavailable};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use ludonimbus_core::{paths, state};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const KEY: &str = "test-game";
const MIB: usize = 1024 * 1024;

/// A cloud in memory that writes down every request and can lose its
/// connection for uploads.
#[derive(Clone, Default)]
struct Net {
    inner: Arc<Memory>,
    log: Arc<Mutex<Vec<String>>>,
    cut: Arc<AtomicBool>,
}

impl Net {
    fn note(&self, what: &str, path: &str) {
        self.log.lock().unwrap().push(format!("{what} {path}"));
    }

    /// The requests since the last call.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    /// Delete every file of that name, as a hand in the cloud folder does.
    fn remove_all(&self, name: &str) {
        for p in self.inner.paths().into_iter().filter(|p| p.ends_with(&format!("/{name}"))) {
            self.inner.remove(&p).unwrap();
        }
    }

    fn has(&self, name: &str) -> bool {
        self.inner.paths().iter().any(|p| p.ends_with(&format!("/{name}")))
    }

    /// The ids of the versions marked as missing a pack.
    fn marks(&self) -> BTreeSet<String> {
        self.inner.paths().iter().filter_map(|p| p.rsplit('/').next()?.strip_suffix(v2::MARK_SUFFIX).map(str::to_string)).collect()
    }
}

impl Store for Net {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".to_string()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.note("list", dir);
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.note("read", path);
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.note("download", path);
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.note("write", path);
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        if self.cut.load(Ordering::SeqCst) {
            return Err(Unavailable("the test took it away".into()).into());
        }
        self.note("upload", path);
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.note("remove", path);
        self.inner.remove(path)
    }
}

struct Device {
    home: PathBuf,
    game: PathBuf,
}

impl Device {
    fn new(root: &Path, name: &str) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), game: root.join(format!("game-{name}")) };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        cfg.games.push(GameEntry {
            key: KEY.into(),
            name: "Test Game".into(),
            exe: Some(dev.game.join("game.exe")),
            saves: vec!["{install}/save/**".into()],
            auto_sync: true,
            ..Default::default()
        });
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    fn put(&self, name: &str, data: &[u8]) {
        let file = self.game.join("save").join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, data).unwrap();
    }

    /// A save too large to be uploaded whole every time (over 16 MiB).
    fn large(&self, version: u8) {
        for n in 0..4u8 {
            self.put(&format!("world{n}.dat"), &vec![version * 10 + n; 4 * MIB]);
        }
    }

    fn small(&self, content: &str) {
        self.put("slot1.sav", content.as_bytes());
    }

    fn saves(&self) -> BTreeMap<String, Vec<u8>> {
        let dir = self.game.join("save");
        let Ok(rd) = fs::read_dir(&dir) else { return BTreeMap::new() };
        rd.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap())).collect()
    }
}

fn ctx(store: &Net) -> Ctx {
    let cfg = config::load().unwrap();
    let cloud = Cloud::new(Box::new(store.clone()), &cfg.device.id, &cfg.device.name);
    Ctx::with_cloud(cfg, cloud)
}

fn run(store: &Net, trigger: Trigger) -> Outcome {
    let ctx = ctx(store);
    let game = ctx.game(KEY).unwrap();
    sync::sync(&ctx, &game, Resolution::Auto, trigger).unwrap()
}

fn flush(store: &Net) -> usize {
    sync::flush_pending(ctx(store).cloud().unwrap(), KEY).unwrap()
}

fn uploaded(outcome: Outcome) -> String {
    match outcome {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

fn queued(outcome: Outcome) -> String {
    match outcome {
        Outcome::Queued { id, .. } => id,
        other => panic!("expected a queued version, got {other:?}"),
    }
}

fn pulled(outcome: Outcome) -> String {
    match outcome {
        Outcome::Pulled { id, .. } => id,
        other => panic!("expected a pull, got {other:?}"),
    }
}

fn heads(store: &Net) -> Vec<String> {
    ctx(store).cloud().unwrap().list_snapshots(KEY).unwrap().into_iter().filter(|s| s.head).map(|s| s.id).collect()
}

/// The manifest of version `id` in the cloud, or as "recently deleted"
/// keeps it.
fn manifest(store: &Net, id: &str) -> v2::Manifest {
    let paths = store.inner.paths();
    let p = paths.iter().find(|p| p.ends_with(&format!("/{id}.json"))).unwrap_or_else(|| panic!("{id} is nowhere"));
    let value: serde_json::Value = serde_json::from_slice(&store.inner.peek(p).unwrap()).unwrap();
    let meta = if p.contains("/trash/") { value["meta"].clone() } else { value };
    serde_json::from_value(meta).unwrap()
}

/// Every version in the cloud has all its packs.
fn assert_complete(store: &Net) {
    let paths: BTreeSet<String> = store.inner.paths().into_iter().collect();
    for p in paths.iter().filter(|p| p.contains("/saves/") && !p.contains("/trash/")) {
        let Some(id) = p.rsplit('/').next().and_then(|n| n.strip_suffix(".json")) else { continue };
        if !v2::is_snapshot_id(id) {
            continue;
        }
        let m: v2::Manifest = serde_json::from_slice(&store.inner.peek(p).unwrap()).unwrap();
        for pack in m.packs() {
            assert!(paths.iter().any(|q| q.ends_with(&format!("/packs/{pack}.zip"))), "{p} refers to {pack}, which is gone");
        }
    }
}

/// The staged manifest of a version waiting on the active device.
fn staged(id: &str) -> PathBuf {
    paths::pending_dir().join(KEY).join(format!("{id}.json"))
}

#[test]
fn versions_that_miss_a_pack() {
    let tmp = tempfile::tempdir().unwrap();
    device_record(&tmp.path().join("record"));
    too_old_to_borrow(&tmp.path().join("old"));
    from_local_backups(&tmp.path().join("backups"));
    nowhere(&tmp.path().join("nowhere"));
    broken_newest(&tmp.path().join("broken"));
    let_go(&tmp.path().join("let-go"));
}

/// A device tells the others at once which version it builds on, when that
/// changed and the next upload could take files from it; not for small
/// saves, which are always uploaded whole (section 3.5).
fn device_record(root: &Path) {
    let store = Net::default();
    let records = |log: &[String]| log.iter().filter(|l| l.starts_with("write devices/")).count();
    let a = Device::new(root, "PC-A");
    let id_a = config::load().unwrap().device.id;
    a.small("v1");
    uploaded(run(&store, Trigger::AutoExit));
    store.take();
    a.small("v2");
    uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(records(&store.take()), 0, "small saves: the record waits for its usual time");

    a.large(1);
    let big = uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(records(&store.take()), 1);
    a.large(2);
    let next = uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(records(&store.take()), 1);
    let record: DeviceInfo = serde_json::from_slice(&store.inner.peek(&format!("devices/{id_a}.json")).unwrap()).unwrap();
    assert_eq!(record.installs[0].synced.as_deref(), Some(next.as_str()));
    assert_ne!(big, next);

    // Another device, getting the large save before playing: told at once
    // too, though nothing else is done before playing.
    let _b = Device::new(root, "PC-B");
    let id_b = config::load().unwrap().device.id;
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), next);
    let log = store.take();
    assert_eq!(records(&log), 1, "{log:#?}");
    let record: DeviceInfo = serde_json::from_slice(&store.inner.peek(&format!("devices/{id_b}.json")).unwrap()).unwrap();
    assert_eq!(record.installs[0].synced.as_deref(), Some(next.as_str()));
    // Nothing new: nothing written.
    assert!(matches!(run(&store, Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(records(&store.take()), 0);
}

/// A large save uploaded, then a version built on it whose upload had to
/// wait: it takes the unchanged files from the first version's pack. Returns
/// the cloud, the device, the first version, the waiting one and the saves
/// it holds.
fn waiting(root: &Path) -> (Net, Device, String, String, BTreeMap<String, Vec<u8>>) {
    let store = Net::default();
    let a = Device::new(root, "PC-A");
    a.large(1);
    a.small("one");
    let first = uploaded(run(&store, Trigger::AutoExit));
    a.small("two");
    store.cut.store(true, Ordering::SeqCst);
    let waiting = queued(run(&store, Trigger::AutoExit));
    store.cut.store(false, Ordering::SeqCst);
    let m: v2::Manifest = serde_json::from_slice(&fs::read(staged(&waiting)).unwrap()).unwrap();
    assert!(m.packs().contains(&first), "it borrows from the first version's pack");
    let saves = a.saves();
    (store, a, first, waiting, saves)
}

/// A version that waited longer than the cloud keeps what it builds on is
/// made whole before it goes up (section 5.4): the files it took from a pack
/// that is gone are packed anew from the saves here, and its manifest is
/// written once, complete.
fn too_old_to_borrow(root: &Path) {
    let (store, a, first, waiting, held) = waiting(root);
    // It waited 61 days...
    let mut m: v2::Manifest = serde_json::from_slice(&fs::read(staged(&waiting)).unwrap()).unwrap();
    m.created_at = Utc::now() - Duration::days(61);
    fs::write(staged(&waiting), serde_json::to_vec(&m).unwrap()).unwrap();
    // ...and the cloud no longer kept the first version's pack for it.
    store.remove_all(&format!("{first}.zip"));
    store.take();
    assert_eq!(flush(&store), 1);
    let log = store.take();
    let looked = log.iter().position(|l| l.starts_with("list ") && l.ends_with("/packs")).expect("a look at the packs");
    let written = log.iter().position(|l| l == &format!("write games/{KEY}/saves/main/{waiting}.json")).expect("its manifest");
    assert!(looked < written, "{log:#?}");
    assert!(!log.iter().any(|l| l.starts_with("remove") && l.contains(&waiting)), "never written and taken back: {log:#?}");
    assert!(!manifest(&store, &waiting).packs().contains(&first));
    assert!(sync::pending_metas(KEY).is_empty());
    let b = Device::new(root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), waiting);
    assert_eq!(b.saves(), held);
    drop(a);
}

/// The same with the saves here moved on: what the version took from the
/// gone pack is found in a copy this device keeps in local-backups.
fn from_local_backups(root: &Path) {
    let (store, a, first, waiting, held) = waiting(root);
    // A copy of the first version's pack kept on this device.
    let kept = paths::local_backups_dir().join(KEY);
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join(format!("{first}.zip")), store.inner.peek(&format!("games/{KEY}/packs/{first}.zip")).unwrap()).unwrap();
    store.remove_all(&format!("{first}.json"));
    store.remove_all(&format!("{first}.zip"));
    a.large(2);
    assert_eq!(flush(&store), 1);
    assert!(sync::pending_metas(KEY).is_empty());
    assert!(sync::unsent(KEY).is_empty());
    assert_complete(&store);
    let b = Device::new(root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), waiting);
    assert_eq!(b.saves(), held);
}

/// A waiting version whose borrowed files are nowhere any more: it leaves
/// the queue for local-backups (#60), and the saves tab lists it once, to
/// export or dismiss.
fn nowhere(root: &Path) {
    let (store, a, first, waiting, held) = waiting(root);
    store.remove_all(&format!("{first}.json"));
    store.remove_all(&format!("{first}.zip"));
    a.large(2);
    assert_eq!(flush(&store), 1);
    assert!(sync::pending_metas(KEY).is_empty());
    assert!(!store.has(&format!("{waiting}.json")));
    let unsent = sync::unsent(KEY);
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].id, waiting);
    assert_eq!(state::game(KEY).unsent.len(), 1);
    // What it brought itself (the small file) can be saved; nothing more.
    let out = root.join("exported.zip");
    assert_eq!(sync::export_unsent(KEY, &waiting, &out).unwrap(), 1);
    let mut zip = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    let mut slot = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("files/install/save/slot1.sav").unwrap(), &mut slot).unwrap();
    assert_eq!(slot, held["slot1.sav"]);
    sync::dismiss_unsent(KEY, &waiting).unwrap();
    assert!(sync::unsent(KEY).is_empty());
    assert!(paths::local_backups_dir().join(KEY).join(format!("{waiting}.json")).is_file(), "its copy stays");
    // The saves here go up complete with the next sync, built on the one
    // set aside: nothing to ask.
    let next = uploaded(run(&store, Trigger::AutoExit));
    assert_complete(&store);
    assert_eq!(heads(&store), [next]);
}

/// A newest version that misses a pack (an older build wrote it, or a pack
/// was deleted by hand): found when a device tries to take it, never taken,
/// put back by a device that has the files, then taken as usual.
fn broken_newest(root: &Path) {
    let store = Net::default();
    let a = Device::new(root, "PC-A");
    a.large(1);
    a.small("one");
    let v1 = uploaded(run(&store, Trigger::AutoExit));
    let b = Device::new(root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), v1);
    a.activate();
    a.small("two");
    let v2 = uploaded(run(&store, Trigger::AutoExit));
    assert!(manifest(&store, &v2).packs().contains(&v1));
    store.remove_all(&format!("{v1}.zip"));

    // A device new to the game: neither version can be taken. It keeps what
    // it has (nothing), is asked nothing, and both are marked.
    let c = Device::new(root, "PC-C");
    assert!(matches!(run(&store, Trigger::PreLaunch), Outcome::NothingToSave));
    assert!(c.saves().is_empty());
    assert_eq!(store.marks(), BTreeSet::from([v1.clone(), v2.clone()]));
    // From then on it doesn't try again, and says why it waits.
    store.take();
    assert!(matches!(run(&store, Trigger::PreLaunch), Outcome::NothingToSave));
    assert!(!store.take().iter().any(|l| l.starts_with("download")));
    let report = sync::check(&ctx(&store), &ctx(&store).game(KEY).unwrap());
    assert_eq!(report.status, Status::Empty);
    assert!(report.incomplete);
    let list = ctx(&store).cloud().unwrap().list_snapshots(KEY).unwrap();
    assert!(list.iter().all(|s| s.incomplete));

    // The device that has the first version's files puts the pack back after
    // playing; both versions are whole again and lose their marks.
    b.activate();
    assert_eq!(pulled(run(&store, Trigger::AutoExit)), v2);
    assert!(store.marks().is_empty());
    assert_complete(&store);

    c.activate();
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), v2);
    assert_eq!(c.saves(), a.saves());
}

/// A version nobody can repair: its writer lets it go to "recently deleted"
/// with the reason, and its next version builds on it, so nothing forks;
/// the other devices take that one.
fn let_go(root: &Path) {
    let store = Net::default();
    let a = Device::new(root, "PC-A");
    a.large(1);
    a.small("one");
    let v1 = uploaded(run(&store, Trigger::AutoExit));
    let c = Device::new(root, "PC-C");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), v1);
    a.activate();
    a.small("two");
    let v2 = uploaded(run(&store, Trigger::AutoExit));
    // Its own pack (the changed file) goes.
    store.remove_all(&format!("{v2}.zip"));

    // Never taken: C stays where it is, without a question.
    c.activate();
    assert!(matches!(run(&store, Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(store.marks(), BTreeSet::from([v2.clone()]));
    assert_eq!(c.saves()["slot1.sav"], b"one");

    // The writer played on: it can't put the pack back, and lets the version
    // go. What it plays now is built on it, so it contains it.
    a.activate();
    a.small("three");
    let v3 = uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(heads(&store), std::slice::from_ref(&v3));
    let deleted = sync::deleted_snapshots(&ctx(&store), &ctx(&store).game(KEY).unwrap()).unwrap();
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].snapshot.id, v2);
    assert_eq!(deleted[0].reason.as_deref(), Some("incomplete"));
    assert!(store.marks().is_empty());
    let (m2, m3) = (manifest(&store, &v2), manifest(&store, &v3));
    assert!(m3.vv.contains(&m2.vv), "{:?} doesn't contain {:?}", m3.vv, m2.vv);
    assert_complete(&store);

    c.activate();
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), v3);
    assert_eq!(c.saves(), a.saves());
}
