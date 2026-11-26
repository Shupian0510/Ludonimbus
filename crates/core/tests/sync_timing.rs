//! Not waiting for the cloud (docs/plan-v2.md, section 5.5): a sync before
//! launch that is given up never changes the saves, whatever it was doing;
//! what wasn't waited for is a fork the user is asked about, not saves
//! lost; a sync after playing that a wrapper handed over is finished later;
//! and one sync of a game runs at a time. One test, because the data
//! directory is chosen through an environment variable.

use anyhow::Result;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::{Cloud, Unavailable};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::{SnapshotSummary, Trigger};
use ludonimbus_core::sync::{self, Ctx, Gate, Outcome, Phase, Resolution};
use ludonimbus_core::{paths, state};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const KEY: &str = "test-game";

/// A store in memory whose requests the test holds back when it wants: the
/// next request of one kind waits until the test lets it go. It can also be
/// out of reach, as a cloud without a connection.
#[derive(Clone, Default)]
struct Slow {
    inner: Arc<Memory>,
    ctl: Arc<(Mutex<Ctl>, Condvar)>,
}

#[derive(Default)]
struct Ctl {
    /// The kind of request to hold back next: "list", "download", ...
    hold: Option<&'static str>,
    /// One is held back right now.
    holding: bool,
    /// Every request fails as one to a cloud that can't be reached.
    down: bool,
}

impl Slow {
    fn hold(&self, what: &'static str) {
        self.ctl.0.lock().unwrap().hold = Some(what);
    }

    /// Wait until a request is held back.
    fn wait_held(&self) {
        let (m, c) = &*self.ctl;
        let start = Instant::now();
        let mut g = m.lock().unwrap();
        while !g.holding {
            assert!(start.elapsed() < Duration::from_secs(20), "no request was held back");
            g = c.wait_timeout(g, Duration::from_millis(50)).unwrap().0;
        }
    }

    fn release(&self) {
        let (m, c) = &*self.ctl;
        let mut g = m.lock().unwrap();
        g.hold = None;
        g.holding = false;
        c.notify_all();
    }

    fn down(&self, down: bool) {
        self.ctl.0.lock().unwrap().down = down;
    }

    fn pass(&self, what: &'static str) -> Result<()> {
        let (m, c) = &*self.ctl;
        let mut g = m.lock().unwrap();
        if g.down {
            return Err(Unavailable("the test took it away".into()).into());
        }
        if g.hold == Some(what) {
            g.holding = true;
            c.notify_all();
            while g.holding {
                g = c.wait(g).unwrap();
            }
        }
        Ok(())
    }

    /// Requests made so far.
    fn requests(&self) -> u32 {
        self.inner.counts().total()
    }
}

impl Store for Slow {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".to_string()
    }
    fn check(&self) -> Result<()> {
        self.pass("check")
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.pass("list")?;
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.pass("read")?;
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.pass("download")?;
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.pass("write")?;
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.pass("upload")?;
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.pass("remove")?;
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("sync-timing-test".to_string())
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

    fn write(&self, content: &str) {
        let file = self.game.join("save").join("slot1.sav");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn read(&self) -> String {
        fs::read_to_string(self.game.join("save").join("slot1.sav")).unwrap_or_default()
    }
}

fn ctx(store: &Slow) -> Ctx {
    let cfg = config::load().unwrap();
    let cloud = Cloud::new(Box::new(store.clone()), &cfg.device.id, &cfg.device.name);
    Ctx::with_cloud(cfg, cloud)
}

fn sync_as(store: &Slow, resolution: Resolution, trigger: Trigger) -> Outcome {
    let ctx = ctx(store);
    let game = ctx.game(KEY).unwrap();
    sync::sync(&ctx, &game, resolution, trigger).unwrap()
}

fn run(store: &Slow, trigger: Trigger) -> Outcome {
    sync_as(store, Resolution::Auto, trigger)
}

/// A sync before launch on a thread of its own, as the launch wrapper runs
/// it.
fn spawn(store: &Slow, gate: &Gate) -> JoinHandle<Outcome> {
    let (ctx, gate) = (ctx(store), gate.clone());
    std::thread::spawn(move || {
        let game = ctx.game(KEY).unwrap();
        sync::sync_with(&ctx, &game, Resolution::Auto, Trigger::PreLaunch, &gate).unwrap()
    })
}

fn versions(store: &Slow) -> Vec<SnapshotSummary> {
    ctx(store).cloud().unwrap().list_snapshots(KEY).unwrap()
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

fn not_waited(outcome: Outcome) {
    assert!(matches!(outcome, Outcome::Offline { .. }), "expected a sync that stopped, got {outcome:?}");
}

/// Nothing of a download is left on this device.
fn nothing_staged() {
    let left: Vec<String> = fs::read_dir(paths::staging_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("pull-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn not_waiting_for_the_cloud() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store = Slow::default();
    let a = Device::new(root, "PC-A");
    let b = Device::new(root, "PC-B");

    // Both share the game; A has played since B last synced.
    a.activate();
    a.write("v1");
    let v1 = uploaded(run(&store, Trigger::AutoExit));
    b.activate();
    assert_eq!(pulled(run(&store, Trigger::PreLaunch)), v1);
    a.activate();
    a.write("v2");
    let v2 = uploaded(run(&store, Trigger::AutoExit));
    b.activate();

    given_up_while_checking(&store, &b, &v1);
    given_up_while_downloading(&store, &b, &v1);
    applying_is_waited_for(&store, &b, &v2);
    a_fork_after_playing(&store, &a, &b);
    owed_after_playing(&store, &b);
    one_sync_at_a_time(&store);
}

/// Given up while the line is read: the saves stay as they are, also once
/// the cloud answers.
fn given_up_while_checking(store: &Slow, b: &Device, v1: &str) {
    store.hold("list");
    let gate = Gate::new();
    let sync = spawn(store, &gate);
    store.wait_held();
    assert_eq!(gate.phase(), Phase::Checking);
    assert!(gate.give_up());
    store.release();
    not_waited(sync.join().unwrap());
    assert_eq!(gate.phase(), Phase::Done);
    assert_eq!(b.read(), "v1");
    assert_eq!(state::game(KEY).last_synced_id.as_deref(), Some(v1));
}

/// Given up while the newer version comes in: the saves stay as they are,
/// and nothing of the download is left.
fn given_up_while_downloading(store: &Slow, b: &Device, v1: &str) {
    store.hold("download");
    let gate = Gate::new();
    let sync = spawn(store, &gate);
    store.wait_held();
    assert!(matches!(gate.phase(), Phase::Downloading { done: 0, total } if total > 0), "{:?}", gate.phase());
    assert!(gate.give_up());
    store.release();
    not_waited(sync.join().unwrap());
    assert_eq!(b.read(), "v1");
    assert_eq!(state::game(KEY).last_synced_id.as_deref(), Some(v1));
    nothing_staged();
}

/// Once the saves are being replaced it is too late to give up: that is
/// waited for, and the pull completes.
fn applying_is_waited_for(store: &Slow, b: &Device, v2: &str) {
    let gate = Gate::new();
    let (at, applying) = mpsc::channel::<()>();
    let (go, wait) = mpsc::channel::<()>();
    let (at, wait) = (Mutex::new(at), Mutex::new(wait));
    gate.watch(move |phase| {
        if phase == Phase::Applying {
            at.lock().unwrap().send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
        }
    });
    let sync = spawn(store, &gate);
    applying.recv_timeout(Duration::from_secs(20)).expect("the sync got to replacing the saves");
    assert_eq!(gate.phase(), Phase::Applying);
    assert!(!gate.give_up());
    go.send(()).unwrap();
    assert_eq!(pulled(sync.join().unwrap()), v2);
    assert!(!gate.given_up());
    assert_eq!(b.read(), "v2");
    nothing_staged();
}

/// B starts without waiting while A plays and uploads: after B's game the
/// two are a fork the user is asked about, and nothing is lost.
fn a_fork_after_playing(store: &Slow, a: &Device, b: &Device) {
    a.activate();
    a.write("v3a");
    let v3 = uploaded(run(store, Trigger::AutoExit));

    b.activate();
    store.hold("list");
    let gate = Gate::new();
    let sync = spawn(store, &gate);
    store.wait_held();
    assert!(gate.give_up());
    store.release();
    not_waited(sync.join().unwrap());
    assert_eq!(b.read(), "v2");

    // B plays on its own saves; after the game it is asked about A's.
    b.write("v3b");
    let other = match run(store, Trigger::AutoExit) {
        Outcome::Conflict { cloud, .. } => cloud,
        other => panic!("expected a conflict, got {other:?}"),
    };
    assert_eq!((other.id.as_str(), other.device_name.as_str()), (v3.as_str(), "PC-A"));
    assert_eq!(b.read(), "v3b");
    assert!(state::game(KEY).conflict.is_some());
    let list = versions(store);
    assert!(list.iter().any(|s| s.id == v3 && s.head), "{list:#?}");
    assert!(list.iter().any(|s| s.side && s.trigger == Trigger::ConflictKeep && s.device_name == "PC-B"), "{list:#?}");

    // The next launch asks again, and B keeps its own: one newest version,
    // and A's stays in the history.
    assert!(matches!(run(store, Trigger::PreLaunch), Outcome::Conflict { .. }));
    let settled = uploaded(sync_as(store, Resolution::UseLocal, Trigger::Manual));
    let list = versions(store);
    assert_eq!(list.iter().filter(|s| s.head).map(|s| s.id.clone()).collect::<Vec<_>>(), vec![settled]);
    assert!(list.iter().any(|s| s.id == v3));
    assert_eq!(b.read(), "v3b");
}

/// The sync after playing the wrapper handed over: noted, kept while the
/// cloud can't be reached, done once it can.
fn owed_after_playing(store: &Slow, b: &Device) {
    assert!(!sync::is_owed(KEY));
    sync::owe(KEY).unwrap();
    assert!(sync::is_owed(KEY));
    assert_eq!(sync::owed_keys(), vec![KEY.to_string()]);

    // Played without a cloud: the version waits in the queue, and the note
    // stays.
    store.down(true);
    b.write("v4");
    let q = queued(sync::finish_owed(&ctx(store), KEY).unwrap().expect("something owed"));
    assert!(sync::is_owed(KEY));
    assert_eq!(state::game(KEY).pending, vec![q.clone()]);
    assert!(!versions_offline_safe(store).iter().any(|s| s.id == q));

    // The cloud is back: the version that waited is uploaded, and the note
    // goes.
    store.down(false);
    assert!(matches!(sync::finish_owed(&ctx(store), KEY).unwrap(), Some(Outcome::UpToDate)));
    assert!(!sync::is_owed(KEY));
    assert!(state::game(KEY).pending.is_empty());
    assert!(versions(store).iter().any(|s| s.id == q && s.head));
    assert!(sync::finish_owed(&ctx(store), KEY).unwrap().is_none());

    // Any sync that ends settles it too.
    sync::owe(KEY).unwrap();
    assert!(matches!(run(store, Trigger::PreLaunch), Outcome::UpToDate));
    assert!(!sync::is_owed(KEY));
    assert_eq!(b.read(), "v4");
}

/// The versions in the cloud, looked at while the store is out of reach.
fn versions_offline_safe(store: &Slow) -> Vec<SnapshotSummary> {
    store.down(false);
    let list = versions(store);
    store.down(true);
    list
}

/// Another process syncs the game: a sync before launch waits for it, and
/// once nobody waits any more it ends without a single request.
fn one_sync_at_a_time(store: &Slow) {
    let held = sync::lock_game(KEY, &Gate::default()).unwrap();
    assert!(sync::sync_running(KEY));
    let before = store.requests();
    let gate = Gate::new();
    let sync = spawn(store, &gate);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(gate.phase(), Phase::Checking);
    assert!(gate.give_up());
    not_waited(sync.join().unwrap());
    assert_eq!(store.requests(), before);

    // One that waits goes on once the other is done.
    let gate = Gate::new();
    let sync = spawn(store, &gate);
    std::thread::sleep(Duration::from_millis(100));
    drop(held);
    assert!(matches!(sync.join().unwrap(), Outcome::UpToDate));
    assert!(!sync::sync_running(KEY));
}
