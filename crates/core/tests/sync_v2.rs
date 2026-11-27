//! The sync flow on layout 2: what a sync costs in requests, versions made
//! on two devices while one was offline, a device that comes from 0.6,
//! managing versions, and devices that keep a game's saves to themselves.
//! One test, because the data directory is chosen through an environment
//! variable.

use anyhow::Result;
use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::repo::Repo;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::{own_line_id, Cloud, CloudGame, CloudProvider};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use ludonimbus_core::sync::{self, Ctx, LineChoice, Outcome, Resolution, Status};
use ludonimbus_core::{paths, state};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const KEY: &str = "test-game";
/// The game's save line, and the folder 0.6 keeps its snapshots in.
const LINE: &str = "games/test-game/saves/main";
const OLD: &str = "games/test-game/snapshots";

/// A store in memory that writes down every request. Small files it has
/// served are kept on the device, as with a cloud that is far away.
#[derive(Clone, Default)]
struct Logged {
    inner: Arc<Memory>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Logged {
    fn note(&self, what: &str, path: &str) {
        self.log.lock().unwrap().push(format!("{what} {path}"));
    }

    /// The requests since the last call.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

impl Store for Logged {
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
        self.note("upload", path);
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.note("remove", path);
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("sync-v2-test".to_string())
    }
}

struct Device {
    home: PathBuf,
    game: PathBuf,
}

impl Device {
    fn new(root: &Path, name: &str, cloud: CloudConfig) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), game: root.join(format!("game-{name}")) };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = cloud;
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

fn sync_with(ctx: &Ctx, resolution: Resolution, trigger: Trigger) -> Outcome {
    let game = ctx.game(KEY).unwrap();
    sync::sync(ctx, &game, resolution, trigger).unwrap()
}

fn run(trigger: Trigger) -> Outcome {
    sync_with(&Ctx::load().unwrap(), Resolution::Auto, trigger)
}

fn status_with(ctx: &Ctx) -> Status {
    let game = ctx.game(KEY).unwrap();
    sync::check(ctx, &game).status
}

fn status() -> Status {
    status_with(&Ctx::load().unwrap())
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

/// The version a conflict shows against the saves here.
fn conflict(outcome: Outcome) -> SnapshotSummary {
    match outcome {
        Outcome::Conflict { cloud, .. } => cloud,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

fn versions() -> Vec<SnapshotSummary> {
    Ctx::load().unwrap().cloud().unwrap().list_snapshots(KEY).unwrap()
}

fn ids(list: &[SnapshotSummary]) -> Vec<String> {
    list.iter().map(|s| s.id.clone()).collect()
}

/// Make the game on the active device follow another save line.
fn use_line(choice: LineChoice) -> Result<Outcome> {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(KEY).unwrap();
    sync::use_line(&ctx, &game, choice)
}

/// The game's save lines as the active device lists them: name, shared,
/// the one it follows, the devices on it.
fn lines() -> Vec<(String, bool, bool, Vec<String>)> {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(KEY).unwrap();
    sync::lines(&ctx, &game).unwrap().into_iter().map(|l| (l.name, l.shared, l.current, l.devices)).collect()
}

/// The line the game follows on the active device, as its entry says.
fn chosen() -> Option<String> {
    config::load().unwrap().game(KEY).unwrap().line.clone()
}

fn heads() -> Vec<String> {
    versions().into_iter().filter(|s| s.head).map(|s| s.id).collect()
}

/// Take a local-folder cloud away, as a share that isn't mounted.
fn offline<T>(cloud: &Path, during: impl FnOnce() -> T) -> T {
    let away = cloud.with_extension("away");
    fs::rename(cloud, &away).unwrap();
    let out = during();
    fs::rename(&away, cloud).unwrap();
    out
}

/// Every file under `dir` with its content, by path relative to `dir`.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A snapshot of the active device's saves as 0.6 makes one.
fn snapshot_06(parent: Option<String>, device: (&str, &str)) -> (SnapshotMeta, PathBuf) {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(KEY).unwrap();
    let local = snapshot::local_state(&game).unwrap();
    snapshot::build(&game, &local, Trigger::AutoExit, parent, device, "").unwrap()
}

#[test]
fn sync_on_layout_2() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    requests(root);
    offline_fork(root);
    from_06(root);
    managing_versions(root);
    own_lines(root);
    from_another_line(root);
    taking_back(root);
}

/// What a sync asks of the cloud: the number of requests is what a slow
/// cloud makes the user wait for (docs/plan-v2.md, section 5.6).
fn requests(root: &Path) {
    let store = Logged::default();
    let ctx = || {
        let cfg = config::load().unwrap();
        let cloud = Cloud::new(Box::new(store.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    };
    let saves = |log: &[String]| -> Vec<String> {
        log.iter().filter(|l| l.contains("/saves/") || l.contains("/packs/") || l.contains(OLD)).cloned().collect()
    };

    // The first upload sets everything up: the layout, the game's record.
    let a = Device::new(root, "REQ-A", CloudConfig::None);
    a.write("v1");
    assert_eq!(status_with(&ctx()), Status::NotUploaded);
    let first = uploaded(sync_with(&ctx(), Resolution::Auto, Trigger::AutoExit));
    let log = store.take();
    // A cloud nobody has used gets its layout without the lock and the
    // reading an upgrade needs.
    assert!(log.contains(&"write format.json".to_string()));
    assert!(log.contains(&format!("write library/{KEY}.json")));
    assert!(!log.iter().any(|l| l.contains("upgrade.lock")), "{log:#?}");
    // A game added on this version has nothing where 0.6 would have put it,
    // and its line doesn't look there.
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string()]);

    // Before playing with nothing new, and looking at the game in the app:
    // the line's folder is listed, nothing else.
    assert!(matches!(sync_with(&ctx(), Resolution::Auto, Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(store.take(), vec![format!("list {LINE}")]);
    assert_eq!(status_with(&ctx()), Status::Synced);
    assert_eq!(store.take(), vec![format!("list {LINE}")]);

    // After playing: one listing, the pack, the manifest, the index.
    a.write("v2");
    let second = uploaded(sync_with(&ctx(), Resolution::Auto, Trigger::AutoExit));
    let log = store.take();
    assert_eq!(
        saves(&log),
        vec![
            format!("list {LINE}"),
            format!("upload games/{KEY}/packs/{second}.zip"),
            format!("write {LINE}/{second}.json"),
            format!("write {LINE}/index.json"),
        ]
    );
    assert!(!log.iter().any(|l| l.contains("format.json") || l.contains("library")), "{log:#?}");

    // Another device, new to this cloud: the layout, the game's record and
    // the line's index, once. Then it gets the newest version.
    let b = Device::new(root, "REQ-B", CloudConfig::None);
    assert_eq!(status_with(&ctx()), Status::CanRestore);
    assert_eq!(
        store.take(),
        vec![
            "read format.json".to_string(),
            "list library".to_string(),
            format!("read library/{KEY}.json"),
            format!("list {LINE}"),
            format!("read {LINE}/index.json"),
        ]
    );
    assert_eq!(pulled(sync_with(&ctx(), Resolution::Auto, Trigger::PreLaunch)), second);
    assert_eq!(b.read(), "v2");
    assert_eq!(
        store.take(),
        vec![format!("list {LINE}"), format!("read {LINE}/{second}.json"), format!("download games/{KEY}/packs/{second}.zip")]
    );
    assert!(matches!(sync_with(&ctx(), Resolution::Auto, Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(store.take(), vec![format!("list {LINE}")]);
    assert_ne!(first, second);

    // A game the cloud knows nothing of: the records are listed (the one
    // this device has read isn't read again), then the game's folder, which
    // is not there.
    let other = b.game.join("other");
    fs::create_dir_all(other.join("save")).unwrap();
    fs::write(other.join("game.exe"), b"exe").unwrap();
    fs::write(other.join("save").join("slot1.sav"), "o1").unwrap();
    config::update(|c| {
        c.games.push(GameEntry {
            key: "other-game".into(),
            name: "Other Game".into(),
            exe: Some(other.join("game.exe")),
            saves: vec!["{install}/save/**".into()],
            auto_sync: true,
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
    let on_b = ctx();
    let game = on_b.game("other-game").unwrap();
    assert_eq!(sync::check(&on_b, &game).status, Status::NotUploaded);
    assert_eq!(store.take(), vec!["list library".to_string(), "list games/other-game".to_string()]);
    // Its first upload makes its record, with a line of one folder. The
    // record is looked for and written; nothing is cleaned up on the first
    // day.
    let third = uploaded(sync::sync(&ctx(), &game, Resolution::Auto, Trigger::AutoExit).unwrap());
    let log = store.take();
    let other_line = "games/other-game/saves/main";
    assert_eq!(
        log.iter().filter(|l| !l.contains("/playtime") && !l.contains("/art/") && !l.ends_with("/game.json") && !l.starts_with("write devices/")).collect::<Vec<_>>(),
        vec![
            "list library",
            "list games/other-game",
            "read library/other-game.json",
            "write library/other-game.json",
            &format!("list {other_line}"),
            &format!("upload games/other-game/packs/{third}.zip"),
            &format!("write {other_line}/{third}.json"),
            &format!("write {other_line}/index.json"),
        ],
        "{log:#?}"
    );
    assert_eq!(state::game("other-game").line.unwrap().dirs, vec!["games/other-game/saves/main".to_string()]);
    assert_eq!(sync::check(&ctx(), &game).status, Status::Synced);
    assert_eq!(store.take(), vec!["list games/other-game/saves/main".to_string()]);

    // A line of the device's own: the game's record is written once, when
    // the line begins, and a check costs what it costs on the shared line.
    b.write("v3");
    let on_b = ctx();
    let game = on_b.game(KEY).unwrap();
    let fourth = uploaded(sync::use_line(&on_b, &game, LineChoice::Independent { name: None }).unwrap());
    let own = format!("games/{KEY}/saves/{}", own_line_id(&on_b.cfg.device.id, KEY));
    let log = store.take();
    eprintln!("a line of its own: {log:#?}");
    // The game's record is read from what this device kept of it: the
    // listing says it hasn't changed.
    let record = format!("library/{KEY}.json");
    assert_eq!(
        log.iter().filter(|l| l.contains("/saves/") || l.contains("/packs/") || *l == "list library" || l.contains(&record)).collect::<Vec<_>>(),
        vec![
            &format!("list {LINE}"),
            "list library",
            &format!("write library/{KEY}.json"),
            &format!("upload games/{KEY}/packs/{fourth}.zip"),
            &format!("write {own}/{fourth}.json"),
            &format!("write {own}/index.json"),
        ],
        "{log:#?}"
    );
    assert_eq!(status_with(&ctx()), Status::Synced);
    assert_eq!(store.take(), vec![format!("list {own}")]);
    assert!(matches!(sync_with(&ctx(), Resolution::Auto, Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(store.take(), vec![format!("list {own}")]);
}

/// Two devices each make a version without seeing the other's: the line has
/// two newest versions until the user chooses.
fn offline_fork(root: &Path) {
    let cloud = root.join("fork-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let folder = || CloudConfig::LocalFolder { path: cloud.clone() };
    let a = Device::new(root, "PC-A", folder());
    let b = Device::new(root, "PC-B", folder());

    a.activate();
    a.write("v1");
    uploaded(run(Trigger::AutoExit));
    b.activate();
    pulled(run(Trigger::PreLaunch));

    // A plays without the cloud, B with it.
    a.activate();
    a.write("v2a");
    let from_a = offline(&cloud, || queued(run(Trigger::AutoExit)));
    assert_eq!(status(), Status::Synced);
    b.activate();
    b.write("v2b");
    let from_b = uploaded(run(Trigger::AutoExit));

    // A's version is uploaded as it was made. Neither is built on the other,
    // so the user is asked, on both devices, about the other one's.
    a.activate();
    let other = conflict(run(Trigger::PreLaunch));
    assert_eq!((other.id.as_str(), other.device_name.as_str()), (from_b.as_str(), "PC-B"));
    assert!(state::game(KEY).pending.is_empty());
    assert_eq!(heads(), vec![from_a.clone(), from_b.clone()]);
    let asked = state::game(KEY).conflict.unwrap();
    assert_eq!(asked.candidates, vec![from_a.clone(), from_b.clone()]);
    assert!(!asked.local_is_candidate);
    assert_eq!(status(), Status::Conflict);
    assert_eq!(a.read(), "v2a");
    b.activate();
    let other = conflict(run(Trigger::PreLaunch));
    assert_eq!((other.id.as_str(), other.device_name.as_str()), (from_a.as_str(), "PC-A"));
    assert_eq!(b.read(), "v2b");

    // A keeps its own: a version on top of both, and B follows.
    a.activate();
    let settled = uploaded(sync_with(&Ctx::load().unwrap(), Resolution::UseLocal, Trigger::Manual));
    assert_eq!(heads(), vec![settled.clone()]);
    assert_eq!(status(), Status::Synced);
    assert!(state::game(KEY).conflict.is_none());
    b.activate();
    assert_eq!(status(), Status::CloudNewer);
    assert_eq!(pulled(run(Trigger::PreLaunch)), settled);
    assert_eq!(b.read(), "v2a");

    // The same again, and this time A takes B's.
    a.activate();
    a.write("v3a");
    let from_a = offline(&cloud, || queued(run(Trigger::AutoExit)));
    b.activate();
    b.write("v3b");
    let from_b = uploaded(run(Trigger::AutoExit));
    a.activate();
    assert_eq!(conflict(run(Trigger::PreLaunch)).id, from_b);
    assert_eq!(pulled(sync_with(&Ctx::load().unwrap(), Resolution::UseCloud, Trigger::Manual)), from_b);
    assert_eq!(a.read(), "v3b");
    assert_eq!(status(), Status::Synced);
    // The fork is closed by a version that says where it came from; A's own
    // version stays in the history.
    let list = versions();
    let last = list.last().unwrap();
    assert!(last.head && last.trigger == Trigger::Restore && last.device_name == "PC-A", "{last:#?}");
    assert_eq!(list.iter().filter(|s| s.head).count(), 1);
    assert!(list.iter().any(|s| s.id == from_a));
    // B has that content already: nothing is transferred.
    b.activate();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(status(), Status::Synced);
    assert_eq!(b.read(), "v3b");
}

/// A device updated from 0.6, with a cloud nobody has upgraded yet and a
/// version still waiting from an evening without the cloud.
fn from_06(root: &Path) {
    let cloud = root.join("old-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let a = Device::new(root, "OLD-A", CloudConfig::LocalFolder { path: cloud.clone() });
    let me = config::load().unwrap().device;
    let repo = Repo::new(LocalFolder::new(cloud.clone()));

    // What 0.6 left: one snapshot in the cloud, one made offline after it.
    a.write("s1");
    let (s1, zip) = snapshot_06(None, (&me.id, &me.name));
    repo.upload(&s1, &zip).unwrap();
    repo.put_game(&CloudGame::from(&Ctx::load().unwrap().game(KEY).unwrap())).unwrap();
    a.write("s2");
    let (q, zip) = snapshot_06(Some(s1.id.clone()), (&me.id, &me.name));
    let pending = paths::pending_dir().join(KEY);
    fs::create_dir_all(&pending).unwrap();
    fs::rename(&zip, pending.join(format!("{}.zip", q.id))).unwrap();
    fs::write(pending.join(format!("{}.json", q.id)), serde_json::to_vec_pretty(&q).unwrap()).unwrap();
    state::update_game(KEY, |g| {
        g.last_synced_id = Some(q.id.clone());
        g.last_synced_hash = Some(q.content_hash.clone());
        g.last_sync_at = Some(q.created_at);
        g.pending = vec![q.id.clone()];
        g.cloud_def_uploaded = true;
    })
    .unwrap();
    let before = tree(&cloud);

    // Looking changes nothing, and the device is where it was.
    assert_eq!(status(), Status::Synced);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(tree(&cloud), before);

    // Saves changed before playing: kept on this device. Upgrading the
    // cloud can take a while, and the game doesn't wait for it.
    a.write("s3");
    let p = queued(run(Trigger::PreLaunch));
    assert_eq!(tree(&cloud), before);
    assert_eq!(state::game(KEY).pending, vec![q.id.clone(), p.clone()]);
    assert_eq!(status(), Status::Synced);

    // Another device is upgrading the cloud at this very moment: nothing
    // can be written, and what was played waits with the rest.
    let lock = cloud.join("upgrade.lock.json");
    let now = chrono::Utc::now();
    let held = serde_json::json!({
        "deviceId": "d-other",
        "deviceName": "Other PC",
        "kind": "upgrade",
        "since": now,
        "expires": now + chrono::Duration::minutes(10),
        "token": "t",
    });
    fs::write(&lock, serde_json::to_vec(&held).unwrap()).unwrap();
    a.write("s4");
    let r = queued(run(Trigger::AutoExit));
    assert!(!cloud.join("format.json").exists());
    assert_eq!(state::game(KEY).pending.len(), 3);
    assert_eq!(status(), Status::Synced);
    fs::remove_file(&lock).unwrap();

    // Then this device upgrades the cloud, what waited is uploaded under
    // the ids it had, in the order it was made, and the next saves follow.
    assert!(matches!(run(Trigger::AutoExit), Outcome::UpToDate));
    assert!(cloud.join("format.json").is_file());
    assert!(state::game(KEY).pending.is_empty());
    assert!(!pending.join(format!("{}.json", q.id)).exists());
    assert_eq!(heads(), vec![r.clone()]);
    a.write("s5");
    let head = uploaded(run(Trigger::AutoExit));
    let list = versions();
    assert_eq!(
        list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec![s1.id.as_str(), q.id.as_str(), p.as_str(), r.as_str(), head.as_str()]
    );
    assert_eq!(heads(), vec![head.clone()]);
    for id in [&q.id, &p, &r, &head] {
        assert!(cloud.join(LINE).join(format!("{id}.json")).is_file());
    }
    // What 0.6 wrote is where it was, as it was: a device still on 0.6
    // keeps reading it.
    let after = tree(&cloud);
    for (path, bytes) in before.iter().filter(|(p, _)| p.starts_with(OLD)) {
        assert_eq!(after.iter().find(|(p, _)| p == path).map(|(_, b)| b), Some(bytes), "{path}");
    }
    assert_eq!(status(), Status::Synced);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));

    // A device still on 0.6 uploads: it can't have seen what was uploaded
    // since the upgrade, so the user is asked.
    a.write("from the old device");
    let (old, zip) = snapshot_06(Some(s1.id.clone()), ("d-old", "OLD-PC"));
    repo.upload(&old, &zip).unwrap();
    a.write("s5");
    let other = conflict(run(Trigger::PreLaunch));
    assert_eq!((other.id.as_str(), other.device_name.as_str()), (old.id.as_str(), "OLD-PC"));
    assert_eq!(pulled(sync_with(&Ctx::load().unwrap(), Resolution::UseCloud, Trigger::Manual)), old.id);
    assert_eq!(a.read(), "from the old device");
    assert_eq!(heads().len(), 1);
    assert_eq!(status(), Status::Synced);
}

/// Keeping, deleting, putting back and exporting versions.
fn managing_versions(root: &Path) {
    let cloud = root.join("manage-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let a = Device::new(root, "PC-M", CloudConfig::LocalFolder { path: cloud.clone() });
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(KEY).unwrap();
    let find = |id: &str| versions().into_iter().find(|s| s.id == id);

    a.write("m1");
    let m1 = uploaded(run(Trigger::AutoExit));
    a.write("m2");
    let m2 = uploaded(run(Trigger::AutoExit));
    a.write("m3");
    let m3 = uploaded(run(Trigger::AutoExit));

    // The newest version and kept saves can't be deleted.
    assert!(sync::delete_snapshot(&ctx, &game, &m3).is_err());
    sync::set_snapshot_flags(&ctx, &game, &m1, Some(true), None).unwrap();
    assert!(find(&m1).unwrap().locked);
    assert!(sync::delete_snapshot(&ctx, &game, &m1).is_err());
    sync::set_snapshot_flags(&ctx, &game, &m1, Some(false), None).unwrap();
    assert!(!find(&m1).unwrap().locked);

    // Deleted versions wait in "recently deleted".
    sync::delete_snapshot(&ctx, &game, &m2).unwrap();
    assert!(find(&m2).is_none());
    let deleted = sync::deleted_snapshots(&ctx, &game).unwrap();
    assert_eq!(deleted.iter().map(|t| (t.snapshot.id.as_str(), t.deleted_by.as_str())).collect::<Vec<_>>(), vec![(m2.as_str(), "PC-M")]);

    // Put back, it is kept: otherwise the next clean-up could take it.
    assert!(sync::restore_deleted(&ctx, &game, &m2).unwrap());
    assert!(find(&m2).unwrap().locked);
    assert!(sync::deleted_snapshots(&ctx, &game).unwrap().is_empty());
    sync::set_snapshot_flags(&ctx, &game, &m2, None, Some("before the boss".to_string())).unwrap();
    let named = find(&m2).unwrap();
    assert_eq!((named.locked, named.comment.as_str()), (true, "before the boss"));

    // Deleted for good.
    sync::set_snapshot_flags(&ctx, &game, &m2, Some(false), None).unwrap();
    sync::delete_snapshot(&ctx, &game, &m2).unwrap();
    sync::purge_deleted(&ctx, &game, &m2).unwrap();
    assert!(sync::deleted_snapshots(&ctx, &game).unwrap().is_empty());
    assert!(find(&m2).is_none());
    assert_eq!(status(), Status::Synced);

    // A version leaves as one zip: the files and what is known about them.
    let zip = root.join("m1.zip");
    sync::export_snapshot(&ctx, &game, &m1, &zip).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    assert!(names.iter().any(|n| n == "snapshot.json"), "{names:?}");
    let save = names.iter().find(|n| n.starts_with("files/") && n.ends_with("slot1.sav")).expect("the save file").clone();
    let mut content = String::new();
    archive.by_name(&save).unwrap().read_to_string(&mut content).unwrap();
    assert_eq!(content, "m1");

    // Another device that will only read from here on.
    let n = Device::new(root, "PC-N", CloudConfig::LocalFolder { path: cloud.clone() });
    assert_eq!(pulled(run(Trigger::PreLaunch)), m3);
    a.activate();

    // A device still on 0.6 uploads this game where the line doesn't look.
    // Once a day this device checks, and from then on the line reads that
    // folder too.
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string()]);
    a.write("from the old device");
    let (old, old_zip) = snapshot_06(None, ("d-old", "OLD-PC"));
    Repo::new(LocalFolder::new(cloud.clone())).upload(&old, &old_zip).unwrap();
    a.write("m3");
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    state::update_game(KEY, |g| g.cleaned_at = Some(chrono::Utc::now() - chrono::Duration::days(2))).unwrap();
    a.write("m4");
    uploaded(run(Trigger::AutoExit));
    let other = conflict(run(Trigger::PreLaunch));
    assert_eq!((other.id.as_str(), other.device_name.as_str()), (old.id.as_str(), "OLD-PC"));
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string(), OLD.to_string()]);
    let settled = uploaded(sync_with(&Ctx::load().unwrap(), Resolution::UseLocal, Trigger::Manual));
    assert_eq!(heads(), vec![settled.clone()]);
    assert_eq!(a.read(), "m4");

    // The device that only reads learns of the folder when it looks at the
    // game's record again, a day later, after playing.
    n.activate();
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string()]);
    assert_eq!(pulled(run(Trigger::AutoExit)), settled);
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string()]);
    state::update_game(KEY, |g| {
        if let Some(line) = g.line.as_mut() {
            line.checked_at = Some(chrono::Utc::now() - chrono::Duration::days(2));
        }
    })
    .unwrap();
    assert!(matches!(run(Trigger::AutoExit), Outcome::UpToDate));
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![LINE.to_string(), OLD.to_string()]);
    assert_eq!(status(), Status::Synced);
    assert_eq!(n.read(), "m4");
}

/// Devices that keep a game's saves to themselves: a line of a device's
/// own, another device that follows it, the way back to the shared line,
/// and a device where that is the default (docs/plan-v2.md, section 7).
fn own_lines(root: &Path) {
    let cloud = root.join("lines-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let folder = || CloudConfig::LocalFolder { path: cloud.clone() };
    let dad = Device::new(root, "DAD-PC", folder());
    let kid = Device::new(root, "KID-PC", folder());

    // Both share the game.
    dad.activate();
    dad.write("d1");
    let d1 = uploaded(run(Trigger::AutoExit));
    kid.activate();
    assert_eq!(pulled(run(Trigger::PreLaunch)), d1);
    kid.write("k1");

    // The kid's PC gets a line of its own. It begins with the saves that
    // are there, and says which version of the shared line they were.
    let k1 = uploaded(use_line(LineChoice::Independent { name: Some("Kid".into()) }).unwrap());
    let own = own_line_id(&config::load().unwrap().device.id, KEY);
    let solo = format!("games/{KEY}/saves/{own}");
    assert_eq!(chosen(), Some(own.clone()));
    assert_eq!(state::game(KEY).line.unwrap().dirs, vec![solo.clone()]);
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(cloud.join(&solo).join(format!("{k1}.json"))).unwrap()).unwrap();
    assert_eq!(manifest["line"], own.as_str());
    assert_eq!(manifest["origin"]["how"], "copy");
    assert_eq!(manifest["origin"]["line"], "main");
    assert_eq!(manifest["origin"]["snapshot"], d1.as_str());
    let list = versions();
    assert_eq!(ids(&list), vec![k1.clone()]);
    assert!(list[0].head && !list[0].comment.is_empty(), "{list:#?}");
    assert_eq!(status(), Status::Synced);
    assert_eq!(kid.read(), "k1");

    // The shared line is as it was, and from here on nothing crosses.
    dad.activate();
    assert_eq!(ids(&versions()), vec![d1.clone()]);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    dad.write("d2");
    let d2 = uploaded(run(Trigger::AutoExit));
    kid.activate();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(kid.read(), "k1");
    kid.write("k2");
    let k2 = uploaded(run(Trigger::AutoExit));
    dad.activate();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(dad.read(), "d2");

    // Every device sees both lines, and who is on which.
    let shared = |current: bool| (String::new(), true, current, vec!["DAD-PC".to_string()]);
    assert_eq!(lines(), vec![shared(true), ("Kid".to_string(), false, false, vec!["KID-PC".to_string()])]);
    kid.activate();
    assert_eq!(lines(), vec![shared(false), ("Kid".to_string(), false, true, vec!["KID-PC".to_string()])]);

    // A third device follows the kid's line: it gets what is there, and
    // what it plays goes there.
    let deck = Device::new(root, "DECK", folder());
    assert!(use_line(LineChoice::Follow { line: "solo-none".into() }).is_err());
    assert_eq!(chosen(), None);
    assert_eq!(pulled(use_line(LineChoice::Follow { line: own.clone() }).unwrap()), k2);
    assert_eq!(chosen(), Some(own.clone()));
    assert_eq!(deck.read(), "k2");
    deck.write("k3");
    let k3 = uploaded(run(Trigger::AutoExit));
    kid.activate();
    assert_eq!(pulled(run(Trigger::PreLaunch)), k3);
    assert_eq!(lines()[1], ("Kid".to_string(), false, true, vec!["DECK".to_string(), "KID-PC".to_string()]));

    // Back to the shared line, whose newest version is another: the user
    // is asked, as on a device that joins. Taking the shared one keeps the
    // saves that were here beside its history.
    assert_eq!(conflict(use_line(LineChoice::Shared).unwrap()).id, d2);
    assert_eq!(chosen().as_deref(), Some("main"));
    assert_eq!(kid.read(), "k3");
    assert_eq!(status(), Status::Conflict);
    assert_eq!(pulled(sync_with(&Ctx::load().unwrap(), Resolution::UseCloud, Trigger::Manual)), d2);
    assert_eq!(kid.read(), "d2");
    let list = versions();
    assert_eq!(heads(), vec![d2.clone()]);
    assert!(list.iter().any(|s| s.side && s.trigger == Trigger::PreRestore && s.device_name == "KID-PC"), "{list:#?}");
    assert_eq!(status(), Status::Synced);
    // The line it left is as it was, for the device that still follows it.
    deck.activate();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(ids(&versions()), vec![k1.clone(), k2.clone(), k3.clone()]);
    assert_eq!(lines()[1], ("Kid".to_string(), false, true, vec!["DECK".to_string()]));

    // And to its own line again, which is the one it had: the line goes on
    // with the saves that are on the kid's PC now, under the name it has.
    kid.activate();
    kid.write("d3");
    let k4 = uploaded(use_line(LineChoice::Independent { name: None }).unwrap());
    assert_eq!(chosen(), Some(own.clone()));
    assert_eq!(ids(&versions()), vec![k1.clone(), k2.clone(), k3.clone(), k4.clone()]);
    assert_eq!(heads(), vec![k4.clone()]);
    assert_eq!(lines()[1].0, "Kid");
    // Choosing it again changes nothing, except the name when one is given.
    assert!(matches!(use_line(LineChoice::Independent { name: Some("Kid's PC".into()) }).unwrap(), Outcome::UpToDate));
    assert_eq!(heads(), vec![k4.clone()]);
    assert_eq!(lines()[1].0, "Kid's PC");
    deck.activate();
    assert_eq!(pulled(run(Trigger::PreLaunch)), k4);
    assert_eq!(deck.read(), "d3");
    dad.activate();
    assert_eq!(heads(), vec![d2.clone()]);

    // A device that keeps its games to itself by default takes nothing of
    // the shared line, and the game's first version here begins its line.
    let guest = Device::new(root, "GUEST", folder());
    config::update(|c| {
        c.independent_saves = true;
        Ok(())
    })
    .unwrap();
    assert_eq!(status(), Status::Empty);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::NothingToSave));
    assert_eq!(guest.read(), "");
    assert_eq!(chosen(), None);
    guest.write("g1");
    assert_eq!(status(), Status::NotUploaded);
    let g1 = uploaded(run(Trigger::AutoExit));
    let guests = own_line_id(&config::load().unwrap().device.id, KEY);
    assert_ne!(guests, own);
    assert_eq!(chosen(), Some(guests.clone()));
    assert_eq!(ids(&versions()), vec![g1.clone()]);
    assert_eq!(lines().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["", "Kid's PC", "GUEST"]);
    // The default is for games that are new to the device: this one stays
    // on its line when the default changes,
    config::update(|c| {
        c.independent_saves = false;
        Ok(())
    })
    .unwrap();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(status(), Status::Synced);
    // and a game that shares already goes on sharing when it is turned on.
    dad.activate();
    config::update(|c| {
        c.independent_saves = true;
        Ok(())
    })
    .unwrap();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    dad.write("d4");
    let d4 = uploaded(run(Trigger::AutoExit));
    assert_eq!(chosen(), None);
    assert!(cloud.join(LINE).join(format!("{d4}.json")).is_file());
    assert_eq!(heads(), vec![d4]);
}

/// A version of another save line becomes the progress here: the game page's
/// "set as current" on a save another device keeps to itself.
fn from_another_line(root: &Path) {
    let cloud = root.join("take-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let folder = || CloudConfig::LocalFolder { path: cloud.clone() };
    let dad = Device::new(root, "TAKE-DAD", folder());
    let kid = Device::new(root, "TAKE-KID", folder());
    let others = || {
        let ctx = Ctx::load().unwrap();
        sync::other_saves(&ctx, &ctx.game(KEY).unwrap()).unwrap()
    };
    let take = |line: &str, id: &str| {
        let ctx = Ctx::load().unwrap();
        sync::take_from(&ctx, &ctx.game(KEY).unwrap(), line, id)
    };

    dad.activate();
    dad.write("d1");
    let d1 = uploaded(run(Trigger::AutoExit));
    // Nothing else to take from while there is one line.
    assert!(others().is_empty());
    kid.activate();
    assert_eq!(pulled(run(Trigger::PreLaunch)), d1);
    kid.write("k1");
    let k1 = uploaded(use_line(LineChoice::Independent { name: Some("Kid".into()) }).unwrap());
    let own = own_line_id(&config::load().unwrap().device.id, KEY);
    kid.write("k2");
    let k2 = uploaded(run(Trigger::AutoExit));

    // The kid's PC sees the shared line's newest version, the other one the kid's.
    let seen = others();
    assert_eq!(seen.len(), 1, "{seen:#?}");
    assert!(seen[0].shared && seen[0].line == "main" && seen[0].snapshot.id == d1 && seen[0].snapshot.platform == "pc", "{seen:#?}");
    dad.activate();
    dad.write("d2");
    let d2 = uploaded(run(Trigger::AutoExit));
    let seen = others();
    assert_eq!(seen.len(), 1, "{seen:#?}");
    assert!(!seen[0].shared && seen[0].name == "Kid" && seen[0].line == own && seen[0].snapshot.id == k2, "{seen:#?}");
    // Who is on which line, and on what.
    let ctx = Ctx::load().unwrap();
    let all = sync::lines(&ctx, &ctx.game(KEY).unwrap()).unwrap();
    assert_eq!(all[0].members.iter().map(|m| (m.device.as_str(), m.platform.as_str(), m.here)).collect::<Vec<_>>(), vec![("TAKE-DAD", "pc", true)]);
    assert_eq!(all[1].members.iter().map(|m| (m.device.as_str(), m.platform.as_str(), m.here)).collect::<Vec<_>>(), vec![("TAKE-KID", "pc", false)]);

    // A line or a version that isn't there: nothing changes.
    assert!(take("solo-none", &k2).is_err());
    assert!(take(&own, "no-such-version").is_err());
    assert_eq!(dad.read(), "d2");
    assert_eq!(ids(&versions()), vec![d1.clone(), d2.clone()]);

    // The kid's older version comes over: the files here are its files, it
    // is the shared line's newest version and says where it came from. What
    // was here is in the history already.
    let t1 = uploaded(take(&own, &k1).unwrap());
    assert_eq!(dad.read(), "k1");
    let list = versions();
    assert_eq!(ids(&list), vec![d1.clone(), d2.clone(), t1.clone()]);
    assert_eq!(heads(), vec![t1.clone()]);
    assert_eq!(status(), Status::Synced);
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(cloud.join(LINE).join(format!("{t1}.json"))).unwrap()).unwrap();
    assert_eq!(manifest["origin"]["how"], "copy");
    assert_eq!(manifest["origin"]["line"], own.as_str());
    assert_eq!(manifest["origin"]["snapshot"], k1.as_str());

    // Saves that are nowhere in the cloud yet are kept beside the history
    // before they are replaced.
    dad.write("d3 unsaved");
    let t2 = uploaded(take(&own, &k2).unwrap());
    assert_eq!(dad.read(), "k2");
    let list = versions();
    let kept: Vec<_> = list.iter().filter(|s| s.side).collect();
    assert_eq!(kept.len(), 1, "{list:#?}");
    assert_eq!(heads(), vec![t2.clone()]);
    // That copy can be brought back.
    let ctx = Ctx::load().unwrap();
    sync::restore(&ctx, &ctx.game(KEY).unwrap(), &kept[0].id).unwrap();
    assert_eq!(dad.read(), "d3 unsaved");

    // The kid's line is as it was, and the kid's PC is untouched.
    kid.activate();
    assert_eq!(ids(&versions()), vec![k1.clone(), k2.clone()]);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(kid.read(), "k2");
}

/// The last time saves of the cloud replaced the ones on a device can be
/// taken back, until the device saves on top of them (docs/plan-v2.md,
/// section 6, "安全网").
fn taking_back(root: &Path) {
    let cloud = root.join("undo-cloud");
    fs::create_dir_all(&cloud).unwrap();
    let folder = || CloudConfig::LocalFolder { path: cloud.clone() };
    let desk = Device::new(root, "UNDO-DESK", folder());
    let deck = Device::new(root, "UNDO-DECK", folder());
    let undo = || {
        let ctx = Ctx::load().unwrap();
        sync::undo(&ctx, &ctx.game(KEY).unwrap())
    };

    desk.activate();
    desk.write("one");
    let v1 = uploaded(run(Trigger::AutoExit));
    assert!(sync::undoable(KEY).is_none() && undo().is_err());
    deck.activate();
    // A device that had no saves has nothing to go back to.
    assert_eq!(pulled(run(Trigger::PreLaunch)), v1);
    assert!(sync::undoable(KEY).is_none());
    deck.write("two");
    let v2 = uploaded(run(Trigger::AutoExit));

    // The sync before playing brings the other device's progress: that can be taken back.
    desk.activate();
    assert_eq!(pulled(run(Trigger::PreLaunch)), v2);
    assert_eq!(desk.read(), "two");
    let r = sync::undoable(KEY).unwrap();
    assert_eq!((r.back.as_str(), r.now.as_str(), r.device.as_str()), (v1.as_str(), v2.as_str(), "UNDO-DECK"));
    let v3 = uploaded(undo().unwrap());
    assert_eq!(desk.read(), "one");
    assert_eq!(status(), Status::Synced);
    assert_eq!(heads(), [v3.clone()]);
    // Nothing was lost: the other device's progress is a version like any other,
    // and taking it back can be taken back.
    let r = sync::undoable(KEY).unwrap();
    assert_eq!((r.back.as_str(), r.now.as_str()), (v2.as_str(), v3.as_str()));
    let v4 = uploaded(undo().unwrap());
    assert_eq!(desk.read(), "two");
    assert_eq!(heads(), [v4.clone()]);

    // Saves that were not in the cloud yet are kept first, and are what it goes back to.
    desk.write("unsaved");
    let ctx = Ctx::load().unwrap();
    sync::restore(&ctx, &ctx.game(KEY).unwrap(), &v1).unwrap();
    assert_eq!(desk.read(), "one");
    let r = sync::undoable(KEY).unwrap();
    let kept = versions().into_iter().find(|s| s.id == r.back).unwrap();
    assert!(kept.side && kept.trigger == Trigger::PreRestore, "{kept:#?}");
    undo().unwrap();
    assert_eq!(desk.read(), "unsaved");

    // Once the device saved on top, there is nothing to take back.
    assert!(sync::undoable(KEY).is_some());
    desk.write("three");
    uploaded(run(Trigger::AutoExit));
    assert!(sync::undoable(KEY).is_none());
    let before = versions().len();
    assert!(undo().is_err());
    assert_eq!((desk.read().as_str(), versions().len()), ("three", before));

    // The other device gets the newest progress as usual.
    deck.activate();
    run(Trigger::PreLaunch);
    assert_eq!(deck.read(), "three");
}
