//! The sync flow on layout 2: what a sync costs in requests, versions made
//! on two devices while one was offline, a device that comes from 0.6, and
//! managing versions. One test, because the data directory is chosen through
//! an environment variable.

use anyhow::Result;
use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::repo::Repo;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::{Cloud, CloudGame, CloudProvider};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::{self, SnapshotMeta, SnapshotSummary, Trigger};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
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
