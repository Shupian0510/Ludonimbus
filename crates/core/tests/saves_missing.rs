//! Every save file of a game deleted on this device (or on a drive that
//! isn't there now, such as a Steam Deck's SD card), and syncing paused on
//! this device (`ludonimbus_core::pause`).
//!
//! - Saves that vanished are never uploaded as the newest version: the game
//!   shows `savesMissing`; nothing is written back after playing or in the
//!   background; the user takes them back ("立即同步", 「取回」), and a game
//!   started through Steam gets them before it starts.
//! - While paused nothing goes to the cloud or comes from it: the wrapper
//!   starts the game without a sync and queues what was played, keeping a
//!   save queues it too, and what needs the cloud is refused. Once syncing
//!   goes on, what waits goes up in order.
//!
//! One test, because the data directory is chosen through an environment
//! variable. A local folder is the cloud; never real saves.

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, SteamLink};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use ludonimbus_core::{pause, state};
use std::fs;
use std::path::{Path, PathBuf};

struct Device {
    home: PathBuf,
    install: PathBuf,
    key: String,
}

impl Device {
    fn new(root: &Path, cloud: &Path, name: &str, key: &str) -> Device {
        let home = root.join(format!("home-{name}"));
        let install = root.join(name).join("Tale");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("game.exe"), b"exe").unwrap();
        let dev = Device { home, install, key: key.to_string() };
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::LocalFolder { path: cloud.to_path_buf() };
        cfg.games.push(GameEntry {
            key: key.into(),
            name: "Tale".into(),
            exe: Some(dev.install.join("game.exe")),
            saves: vec!["{install}/save/**".into()],
            game: Some("tale".into()),
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

    fn ctx(&self) -> Ctx {
        self.activate();
        Ctx::load().unwrap()
    }

    fn entry(&self) -> GameEntry {
        self.ctx().game(&self.key).unwrap()
    }

    fn change(&self, f: impl FnOnce(&mut GameEntry)) {
        self.activate();
        config::update(|c| {
            f(c.game_mut(&self.key).unwrap());
            Ok(())
        })
        .unwrap();
    }

    fn save_dir(&self) -> PathBuf {
        self.install.join("save")
    }

    fn write(&self, content: &str) {
        fs::create_dir_all(self.save_dir()).unwrap();
        fs::write(self.save_dir().join("slot1.sav"), content).unwrap();
    }

    fn read(&self) -> Option<String> {
        fs::read_to_string(self.save_dir().join("slot1.sav")).ok()
    }

    fn try_sync(&self, resolution: Resolution, trigger: Trigger) -> anyhow::Result<Outcome> {
        let ctx = self.ctx();
        sync::sync(&ctx, &self.entry(), resolution, trigger)
    }

    fn sync(&self, trigger: Trigger) -> Outcome {
        self.try_sync(Resolution::Auto, trigger).unwrap()
    }

    fn status(&self) -> Status {
        let ctx = self.ctx();
        sync::check(&ctx, &self.entry()).status
    }

    fn versions(&self) -> usize {
        let ctx = self.ctx();
        ctx.cloud().unwrap().list_snapshots(&self.key).unwrap().len()
    }

    fn pending(&self) -> usize {
        self.activate();
        state::game(&self.key).pending.len()
    }
}

fn uploaded(o: Outcome) {
    assert!(matches!(o, Outcome::Uploaded { .. }), "expected an upload, got {o:?}");
}

fn pulled(o: Outcome) {
    assert!(matches!(o, Outcome::Pulled { .. }), "expected a pull, got {o:?}");
}

fn queued(o: Outcome) {
    assert!(matches!(o, Outcome::Queued { .. }), "expected a version in the queue, got {o:?}");
}

#[test]
fn missing_saves_and_pausing() {
    let tmp = tempfile::tempdir().unwrap();
    missing_saves(tmp.path());
    pausing(tmp.path());
}

fn missing_saves(root: &Path) {
    let cloud = root.join("cloud-missing");
    fs::create_dir_all(&cloud).unwrap();
    let a = Device::new(root, &cloud, "M-A", "tale");
    a.write("v1");
    uploaded(a.sync(Trigger::AutoExit));
    a.write("v2");
    uploaded(a.sync(Trigger::AutoExit));
    assert_eq!(a.versions(), 2);

    // Every save file is gone here (deleted, or the SD card is out).
    fs::remove_dir_all(a.save_dir()).unwrap();
    assert_eq!(a.status(), Status::SavesMissing);

    // After playing, in the background, "全部同步", a sync that was owed:
    // nothing is uploaded and nothing is written back.
    for trigger in [Trigger::Passive, Trigger::AutoExit] {
        let o = a.sync(trigger);
        assert!(matches!(o, Outcome::NothingToSave), "{trigger:?}: {o:?}");
        assert_eq!(a.read(), None, "{trigger:?}");
    }
    let ctx = a.ctx();
    assert!(matches!(sync::upload(&ctx, &a.entry()).unwrap(), Outcome::NothingToSave));
    a.activate();
    sync::owe_passive(&a.key).unwrap();
    assert!(matches!(sync::finish_owed(&a.ctx(), &a.key).unwrap(), Some(Outcome::NothingToSave)));
    assert_eq!(a.read(), None);
    // Keeping "these" saves, or making them the newest, has nothing to keep.
    assert!(sync::backup_now(&a.ctx(), &a.entry(), "kept").is_err());
    assert!(a.try_sync(Resolution::UseLocal, Trigger::SyncNow).is_err());
    assert_eq!(a.versions(), 2, "nothing empty was uploaded");
    assert_eq!(a.status(), Status::SavesMissing);

    // The user takes them back: the newest version, with its files.
    pulled(a.sync(Trigger::SyncNow));
    assert_eq!(a.read().as_deref(), Some("v2"));
    assert_eq!(a.status(), Status::Synced);

    // A game started through Steam gets them before it starts, as it gets
    // any newer version; after playing it still writes nothing back.
    a.change(|g| g.steam = Some(SteamLink { account: "1".into(), appid: 1, enabled: true, original: None, created: false }));
    fs::remove_dir_all(a.save_dir()).unwrap();
    assert!(matches!(a.sync(Trigger::AutoExit), Outcome::NothingToSave));
    assert_eq!(a.read(), None);
    pulled(a.sync(Trigger::PreLaunch));
    assert_eq!(a.read().as_deref(), Some("v2"));
    assert_eq!(a.versions(), 2);

    // A device that never had the saves isn't missing them: it can restore.
    let b = Device::new(root, &cloud, "M-B", "tale");
    assert_eq!(b.status(), Status::CanRestore);
}

fn pausing(root: &Path) {
    let cloud = root.join("cloud-pause");
    fs::create_dir_all(&cloud).unwrap();
    let a = Device::new(root, &cloud, "P-A", "tale");
    a.write("v1");
    uploaded(a.sync(Trigger::AutoExit));
    let other = Device::new(root, &cloud, "P-B", "tale");
    pulled(other.sync(Trigger::PreLaunch));

    a.activate();
    assert!(!pause::is_paused());
    let p = pause::pause().unwrap();
    assert_eq!(pause::pause().unwrap(), p, "pausing again keeps the first time");
    assert!(pause::is_paused());

    // Before launch: no sync, the game starts.
    assert!(matches!(a.sync(Trigger::PreLaunch), Outcome::Offline { .. }));
    // After playing: the saves wait in the queue; nothing goes up.
    a.write("v2");
    queued(a.sync(Trigger::AutoExit));
    assert_eq!((a.versions(), a.pending()), (1, 1));
    // In the background: the same, once per change.
    a.write("v3");
    queued(a.sync(Trigger::Passive));
    assert!(matches!(a.sync(Trigger::Passive), Outcome::Offline { .. }), "nothing new to queue");
    // Keeping a save before a risky run: queued as well.
    a.write("v4");
    queued(sync::backup_now(&a.ctx(), &a.entry(), "before the boss").unwrap());
    assert_eq!((a.versions(), a.pending()), (1, 3));
    assert_eq!(state::game(&a.key).last_error, None, "pausing is no error");
    // What needs the cloud is refused, saying why.
    for trigger in [Trigger::SyncNow, Trigger::Manual] {
        let e = a.try_sync(Resolution::Auto, trigger).unwrap_err();
        assert!(pause::is_paused_error(&e), "{e:#}");
    }
    let e = a.try_sync(Resolution::UseCloud, Trigger::SyncNow).unwrap_err();
    assert!(pause::is_paused_error(&e));
    let ctx = a.ctx();
    let head = ctx.cloud().unwrap().list_snapshots(&a.key).unwrap()[0].id.clone();
    assert!(pause::is_paused_error(&sync::restore(&ctx, &a.entry(), &head).unwrap_err()));
    assert!(pause::is_paused_error(&sync::flush_pending(ctx.cloud().unwrap(), &a.key).unwrap_err()));
    assert_eq!(a.read().as_deref(), Some("v4"), "nothing was written");
    assert_eq!(a.versions(), 1);

    // Going on: what waits goes up in order, each on the one before, and
    // the saves here are the newest version.
    assert!(pause::resume().unwrap());
    assert!(!pause::resume().unwrap(), "not paused any more");
    let ctx = a.ctx();
    assert_eq!(sync::flush_pending(ctx.cloud().unwrap(), &a.key).unwrap(), 3);
    assert_eq!((a.versions(), a.pending()), (4, 0));
    assert_eq!(a.status(), Status::Synced);
    let list = ctx.cloud().unwrap().list_snapshots(&a.key).unwrap();
    let head = sync::mainline_head(&list).unwrap();
    assert_eq!(head.trigger, Trigger::Manual, "the kept save is the newest");
    // The other device gets the newest without a question.
    assert_eq!(other.status(), Status::CloudNewer);
    pulled(other.sync(Trigger::PreLaunch));
    assert_eq!(other.read().as_deref(), Some("v4"));
}
