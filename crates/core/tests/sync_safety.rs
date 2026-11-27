//! What a sync must never leave in the cloud or take from it: a version
//! whose pack is gone, after an upload that waited while another device
//! cleaned up (docs/plan-v2.md, section 3.4); the versions other devices
//! build on, after a listing of the devices that failed (section 5.4). One
//! test, because the data directory is chosen through an environment
//! variable.

use anyhow::Result;
use chrono::{Duration, Utc};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::{v2, Cloud, Unavailable};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, Retention};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::{paths, state};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const KEY: &str = "test-game";
const MIB: usize = 1024 * 1024;

/// A cloud in memory that can lose its connection for uploads, or for the
/// listing of the devices, while everything else still works.
#[derive(Clone, Default)]
struct Cut {
    inner: Arc<Memory>,
    uploads: Arc<AtomicBool>,
    devices: Arc<AtomicBool>,
}

impl Cut {
    fn gone() -> anyhow::Error {
        Unavailable("the test took it away".into()).into()
    }

    /// Delete every file of that name, as a clean-up on another device does.
    fn remove_all(&self, name: &str) {
        for p in self.inner.paths().into_iter().filter(|p| p.ends_with(&format!("/{name}"))) {
            self.inner.remove(&p).unwrap();
        }
    }
}

impl Store for Cut {
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
        if dir == "devices" && self.devices.load(Ordering::SeqCst) {
            return Err(Self::gone());
        }
        self.inner.list(dir)
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
        if self.uploads.load(Ordering::SeqCst) {
            return Err(Self::gone());
        }
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.inner.remove(path)
    }
}

struct Device {
    home: PathBuf,
    game: PathBuf,
}

impl Device {
    fn new(root: &Path, name: &str) -> Device {
        Self::keeping(root, name, Retention::default())
    }

    /// A device that keeps `retention` of old versions when it cleans up.
    fn keeping(root: &Path, name: &str, retention: Retention) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), game: root.join(format!("game-{name}")) };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        cfg.retention = retention;
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

    /// A save too large to be uploaded whole every time (over 16 MiB): the
    /// next version only sends what changed and takes the rest from this
    /// one's pack.
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
        fs::read_dir(&dir).unwrap().flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap())).collect()
    }
}

fn ctx(store: &Cut) -> Ctx {
    let cfg = config::load().unwrap();
    let cloud = Cloud::new(Box::new(store.clone()), &cfg.device.id, &cfg.device.name);
    Ctx::with_cloud(cfg, cloud)
}

fn run(store: &Cut, trigger: Trigger) -> Outcome {
    let ctx = ctx(store);
    let game = ctx.game(KEY).unwrap();
    sync::sync(&ctx, &game, Resolution::Auto, trigger).unwrap()
}

fn flush(store: &Cut) -> usize {
    sync::flush_pending(ctx(store).cloud().unwrap(), KEY).unwrap()
}

fn versions(store: &Cut) -> BTreeSet<String> {
    ctx(store).cloud().unwrap().list_snapshots(KEY).unwrap().into_iter().map(|s| s.id).collect()
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

/// Whether the cloud has the manifest of version `id` in a save line.
fn has_manifest(store: &Cut, id: &str) -> bool {
    store.inner.paths().iter().any(|p| p.contains("/saves/") && p.ends_with(&format!("/{id}.json")))
}

/// Every version in the cloud has all its packs: no device is ever told
/// that one is incomplete.
fn assert_complete(store: &Cut) {
    let paths: BTreeSet<String> = store.inner.paths().into_iter().collect();
    for p in paths.iter().filter(|p| p.contains("/saves/")) {
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

/// A large save uploaded, then a version built on it whose upload had to
/// wait: it takes the unchanged files from the first version's pack.
fn waiting_on_a_pack(root: &Path) -> (Cut, Device, String, String) {
    let store = Cut::default();
    let a = Device::new(root, "PC-A");
    a.large(1);
    a.small("one");
    let first = uploaded(run(&store, Trigger::AutoExit));
    a.small("two");
    store.uploads.store(true, Ordering::SeqCst);
    let waiting = queued(run(&store, Trigger::AutoExit));
    store.uploads.store(false, Ordering::SeqCst);
    let staged: v2::Manifest = serde_json::from_slice(&fs::read(paths::pending_dir().join(KEY).join(format!("{waiting}.json"))).unwrap()).unwrap();
    assert!(staged.packs().contains(&first), "it borrows from the first version's pack");
    (store, a, first, waiting)
}

#[test]
fn nothing_incomplete_and_nothing_taken_blind() {
    let tmp = tempfile::tempdir().unwrap();

    // While the version waits, another device cleans up the first one and
    // its pack. The saves here are as they were when it was made: the pack
    // is put back from them, and the version is complete.
    let root = tmp.path().join("put-back");
    let (store, a, first, waiting) = waiting_on_a_pack(&root);
    store.remove_all(&format!("{first}.json"));
    store.remove_all(&format!("{first}.zip"));
    assert_eq!(flush(&store), 1);
    assert_eq!(flush(&store), 0);
    assert!(sync::pending_metas(KEY).is_empty());
    assert!(has_manifest(&store, &waiting));
    assert_complete(&store);
    let b = Device::new(&root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), waiting);
    assert_eq!(b.saves(), a.saves());

    // The same, but the saves changed again before the upload got through:
    // the files it took from that pack are nowhere any more. The version
    // never shows up in the cloud, it leaves the queue for this device's
    // kept copies instead of holding up every later sync, and the saves
    // here go up complete with the next one.
    let root = tmp.path().join("lost");
    let (store, a, first, waiting) = waiting_on_a_pack(&root);
    store.remove_all(&format!("{first}.json"));
    store.remove_all(&format!("{first}.zip"));
    a.large(2);
    assert_eq!(flush(&store), 1);
    assert_eq!(flush(&store), 0);
    assert!(sync::pending_metas(KEY).is_empty());
    assert!(!has_manifest(&store, &waiting));
    assert_complete(&store);
    assert!(paths::local_backups_dir().join(KEY).join(format!("{waiting}.json")).is_file());
    let next = uploaded(run(&store, Trigger::AutoExit));
    assert_complete(&store);
    let b = Device::new(&root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), next);
    assert_eq!(b.saves(), a.saves());

    // Clean-up keeps the version each device synced last. When the devices
    // can't be listed it doesn't know those, and cleans up nothing.
    let root = tmp.path().join("clean-up");
    let store = Cut::default();
    let a = Device::keeping(&root, "PC-A", Retention { keep_recent: 1, keep_daily_days: 0 });
    a.small("v1");
    let v1 = uploaded(run(&store, Trigger::AutoExit));
    let _b = Device::new(&root, "PC-B");
    assert_eq!(pulled(run(&store, Trigger::AutoExit)), v1);
    a.activate();
    a.small("v2");
    let v2 = uploaded(run(&store, Trigger::AutoExit));
    a.small("v3");
    let v3 = uploaded(run(&store, Trigger::AutoExit));
    let due = || state::update_game(KEY, |g| g.cleaned_at = Some(Utc::now() - Duration::days(2))).unwrap();
    due();
    store.devices.store(true, Ordering::SeqCst);
    a.small("v4");
    let v4 = uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(versions(&store), BTreeSet::from([v1.clone(), v2.clone(), v3.clone(), v4.clone()]));
    // With the devices listed: B's version stays, the rest of the old ones go.
    store.devices.store(false, Ordering::SeqCst);
    due();
    a.small("v5");
    let v5 = uploaded(run(&store, Trigger::AutoExit));
    assert_eq!(versions(&store), BTreeSet::from([v1, v5]));
    assert_complete(&store);
}
