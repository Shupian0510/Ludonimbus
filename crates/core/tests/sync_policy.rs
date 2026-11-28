//! Who writes into a game's saves, and who shares a line
//! (docs/plan-positioning.md, sections 2 and 3).
//!
//! - Uploading is always automatic. A newer version of the game's own line
//!   is written into its saves by itself only for a game started through
//!   Steam (the launch wrapper's syncs). A sync in the background for a
//!   game that ran any other way only uploads: the game shows that the
//!   cloud has newer saves, and the user takes them (「取回」).
//! - Only the same version of a game shares a line by itself: the same kind
//!   of install on another device. Another store's version gets a line of
//!   its own on the same card, and shares one only when the user says so
//!   ("共用进度"); then the next device with that kind of install does too.
//!
//! One test, because the data directory is chosen through an environment
//! variable.

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry, SteamLink};
use ludonimbus_core::join::{self, Fit};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use std::fs;
use std::path::{Path, PathBuf};

/// A device with one game installed in `install`.
struct Device {
    home: PathBuf,
    install: PathBuf,
    key: String,
}

impl Device {
    fn new(root: &Path, cloud: &Path, name: &str, key: &str, install: PathBuf) -> Device {
        let home = root.join(format!("home-{name}"));
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

    fn write(&self, content: &str) {
        let file = self.install.join("save").join("slot1.sav");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn read(&self) -> String {
        fs::read_to_string(self.install.join("save").join("slot1.sav")).unwrap_or_default()
    }

    fn sync_as(&self, resolution: Resolution, trigger: Trigger) -> Outcome {
        let ctx = self.ctx();
        sync::sync(&ctx, &ctx.game(&self.key).unwrap(), resolution, trigger).unwrap()
    }

    fn sync(&self, trigger: Trigger) -> Outcome {
        self.sync_as(Resolution::Auto, trigger)
    }

    fn upload(&self) -> Outcome {
        let ctx = self.ctx();
        sync::upload(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    fn status(&self) -> Status {
        let ctx = self.ctx();
        sync::check(&ctx, &ctx.game(&self.key).unwrap()).status
    }

    /// The contents of the versions on the line the game follows.
    fn versions(&self) -> usize {
        let ctx = self.ctx();
        ctx.cloud().unwrap().list_snapshots(&self.key).unwrap().len()
    }
}

fn uploaded(o: Outcome) {
    assert!(matches!(o, Outcome::Uploaded { .. }), "expected an upload, got {o:?}");
}

fn pulled(o: Outcome) {
    assert!(matches!(o, Outcome::Pulled { .. }), "expected a pull, got {o:?}");
}

fn nothing(o: Outcome) {
    assert!(matches!(o, Outcome::UpToDate | Outcome::NothingToSave), "expected nothing to happen, got {o:?}");
}

#[test]
fn who_writes_and_who_shares() {
    let tmp = tempfile::tempdir().unwrap();
    background_syncs_only_upload(tmp.path());
    only_the_same_version_shares_by_itself(tmp.path());
}

fn background_syncs_only_upload(root: &Path) {
    let cloud = root.join("cloud-policy");
    fs::create_dir_all(&cloud).unwrap();
    let a = Device::new(root, &cloud, "P-A", "tale", root.join("p-a").join("Tale"));
    let b = Device::new(root, &cloud, "P-B", "tale", root.join("p-b").join("Games").join("Tale"));

    a.write("v1");
    uploaded(a.sync(Trigger::AutoExit));
    pulled(b.sync(Trigger::PreLaunch));
    assert_eq!(b.read(), "v1");

    // Another device played on. B's game ended outside Steam: the sync in
    // the background leaves the newer version in the cloud.
    a.write("v2");
    uploaded(a.sync(Trigger::AutoExit));
    nothing(b.sync(Trigger::Passive));
    assert_eq!(b.read(), "v1");
    assert_eq!(b.status(), Status::CloudNewer);
    // So does "全部同步", and a background sync that was owed.
    nothing(b.upload());
    assert_eq!(b.read(), "v1");
    b.activate();
    sync::owe_passive(&b.key).unwrap();
    assert!(sync::is_owed(&b.key));
    nothing(sync::finish_owed(&b.ctx(), &b.key).unwrap().expect("owed"));
    assert!(!sync::is_owed(&b.key), "settled");
    assert_eq!(b.read(), "v1");
    assert_eq!(b.status(), Status::CloudNewer);

    // The user takes it (「取回」, "立即同步").
    pulled(b.sync(Trigger::SyncNow));
    assert_eq!(b.read(), "v2");
    assert_eq!(b.status(), Status::Synced);

    // Uploading still happens by itself.
    b.write("v3");
    uploaded(b.sync(Trigger::Passive));
    assert_eq!(a.status(), Status::CloudNewer);

    // A game added to Steam that was started without Steam: the same. Its
    // next launch through Steam takes the newer version before it starts.
    a.change(|g| g.steam = Some(SteamLink { account: "1".into(), appid: 1, enabled: true, original: None, created: false }));
    assert!(sync::pulls_by_itself(&a.entry()));
    nothing(a.sync(Trigger::Passive));
    assert_eq!(a.read(), "v2");
    pulled(a.sync(Trigger::PreLaunch));
    assert_eq!(a.read(), "v3");
    // A sync after playing the wrapper owes takes it too.
    b.write("v4");
    uploaded(b.sync(Trigger::Passive));
    a.activate();
    sync::owe(&a.key).unwrap();
    pulled(sync::finish_owed(&a.ctx(), &a.key).unwrap().expect("owed"));
    assert_eq!(a.read(), "v4");

    // Both played: a conflict for the user, and nothing written.
    a.write("a5");
    uploaded(a.sync(Trigger::AutoExit));
    b.write("b5");
    assert!(matches!(b.sync(Trigger::Passive), Outcome::Conflict { .. }));
    assert_eq!(b.read(), "b5");
    assert_eq!(b.status(), Status::Conflict);
    assert!(!sync::pulls_by_itself(&b.entry()));
}

fn only_the_same_version_shares_by_itself(root: &Path) {
    let cloud = root.join("cloud-kinds");
    fs::create_dir_all(&cloud).unwrap();
    let steam1 = Device::new(root, &cloud, "K-S1", "tale-s1", root.join("k-s1").join("steamapps").join("common").join("Tale"));
    steam1.write("s1");
    uploaded(steam1.sync(Trigger::AutoExit));

    // The Steam version on another device, under another key: the same
    // version, so it continues that line by itself.
    let steam2 = Device::new(root, &cloud, "K-S2", "tale-s2", root.join("k-s2").join("SteamLibrary").join("steamapps").join("common").join("Tale"));
    pulled(steam2.sync(Trigger::PreLaunch));
    assert_eq!(steam2.read(), "s1");
    assert_eq!(steam2.entry().record.as_deref(), Some("tale-s1"));

    // The Epic version keeps the same files in the same place, and is
    // another version: a line of its own, and nothing taken from Steam's.
    let epic = Device::new(root, &cloud, "K-E", "tale-e", root.join("k-e").join("Epic Games").join("Tale"));
    nothing(epic.sync(Trigger::PreLaunch));
    assert_eq!(epic.read(), "");
    assert_ne!(epic.entry().record.as_deref(), Some("tale-s1"));
    epic.write("e1");
    uploaded(epic.sync(Trigger::AutoExit));
    assert_eq!(epic.versions(), 1);
    assert_eq!(steam2.status(), Status::Synced);
    assert_eq!(steam1.versions(), 1);

    // "共用进度": offered, because the files are the same, and only taken
    // when the user says so.
    let ctx = epic.ctx();
    let offer = join::suggestion(&ctx, &epic.entry()).unwrap().expect("Steam's line is offered");
    assert_eq!(offer.fit, Fit::Ready);
    assert!(matches!(join::accept(&ctx, &epic.entry(), &offer).unwrap(), Outcome::Conflict { .. }));
    pulled(epic.sync_as(Resolution::UseCloud, Trigger::SyncNow));
    assert_eq!(epic.read(), "s1");

    // The next device with the Epic version shares it too: the user chose
    // that once. The GOG version wasn't chosen, and keeps its own.
    let epic2 = Device::new(root, &cloud, "K-E2", "tale-e2", root.join("k-e2").join("Epic Games").join("Tale"));
    pulled(epic2.sync(Trigger::PreLaunch));
    assert_eq!(epic2.read(), "s1");
    let gog = Device::new(root, &cloud, "K-G", "tale-g", root.join("k-g").join("GOG Games").join("Tale"));
    nothing(gog.sync(Trigger::PreLaunch));
    assert_eq!(gog.read(), "");
}
