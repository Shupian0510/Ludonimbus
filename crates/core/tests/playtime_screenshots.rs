//! Two devices sharing one cloud folder: playtime adds up over devices (with
//! Steam's earlier minutes imported once), and Steam screenshots taken on one
//! PC end up in the other PC's Steam folder of the same shortcut.

use chrono::{Duration, Utc};
use ludonimbus_core::cloud::gamedata::Folders;
use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::config::{AppConfig, GameEntry, ScreenshotMode, SteamLink};
use ludonimbus_core::playtime::{self, Via};
use ludonimbus_core::screenshots::{self, shortcut_gameid};
use std::fs;
use std::path::{Path, PathBuf};

const KEY: &str = "zelda-oot";

struct Device {
    home: PathBuf,
    cfg: AppConfig,
    game: GameEntry,
    shots: PathBuf,
}

impl Device {
    fn new(tmp: &Path, id: &str, appid: u32) -> Device {
        let home = tmp.join(id).join("home");
        let root = tmp.join(id).join("Steam");
        fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();
        let mut cfg = AppConfig::default();
        cfg.device.id = id.into();
        cfg.device.name = id.to_uppercase();
        cfg.steam.root = Some(root.clone());
        cfg.screenshots = ScreenshotMode::Sync;
        let game = GameEntry {
            key: KEY.into(),
            name: "Zelda".into(),
            steam: Some(SteamLink {
                account: "42".into(),
                appid,
                enabled: true,
                original: None,
                created: false,
            }),
            ..Default::default()
        };
        let shots = root
            .join("userdata/42/760/remote")
            .join(shortcut_gameid(appid).to_string())
            .join("screenshots");
        fs::create_dir_all(shots.join("thumbnails")).unwrap();
        Device { home, cfg, game, shots }
    }

    /// Act as this device: Ludonimbus's own files live in its home.
    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    fn take_screenshot(&self, name: &str) {
        fs::write(self.shots.join(name), format!("jpeg {name}")).unwrap();
        fs::write(self.shots.join("thumbnails").join(name), format!("thumb {name}")).unwrap();
    }
}

#[test]
fn playtime_and_screenshots_across_devices() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("cloud")).unwrap();
    let store = LocalFolder::new(tmp.path().join("cloud"));
    store.check().unwrap();
    let store: &dyn Store = &store;
    let quiet = |_: String| {};

    // Device A played 60 minutes from Steam before using Ludonimbus.
    let a = Device::new(tmp.path(), "d-a", 4079546474);
    let localconfig = r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "-215420822" { "Playtime" "60" } } } } } }"#;
    fs::write(a.cfg.steam.root.as_ref().unwrap().join("userdata/42/config/localconfig.vdf"), localconfig).unwrap();
    a.activate();
    let log = playtime::ensure(&a.cfg, &a.game).unwrap();
    assert_eq!(log.steam_baseline.as_ref().map(|b| b.minutes), Some(60));
    let t0 = Utc::now() - Duration::hours(3);
    assert!(playtime::record(&a.cfg, &a.game, t0, t0 + Duration::seconds(600), Via::Steam).unwrap());
    // Too short to count, and the same session twice is ignored.
    assert!(!playtime::record(&a.cfg, &a.game, t0, t0 + Duration::seconds(3), Via::Steam).unwrap());
    assert!(!playtime::record(&a.cfg, &a.game, t0, t0 + Duration::seconds(600), Via::Steam).unwrap());
    let sum = playtime::sync(store, &a.cfg, &Folders::of_key(KEY)).unwrap();
    assert_eq!(sum.total_secs, 3600 + 600);

    a.take_screenshot("20261120161819_1.jpg");
    a.take_screenshot("20261120164625_1.jpg");
    let r = screenshots::sync(store, &a.cfg, &a.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded, r.failed), (2, 0, 0));

    // Device B: the same game under another shortcut app id, no Steam history.
    let b = Device::new(tmp.path(), "d-b", 2661209017);
    b.activate();
    let t1 = Utc::now() - Duration::hours(1);
    assert!(playtime::record(&b.cfg, &b.game, t1, t1 + Duration::seconds(1200), Via::Direct).unwrap());
    let sum = playtime::sync(store, &b.cfg, &Folders::of_key(KEY)).unwrap();
    assert_eq!(sum.total_secs, 3600 + 600 + 1200);
    assert_eq!(sum.devices.len(), 2);
    assert!(sum.devices[0].this_device);

    let r = screenshots::sync(store, &b.cfg, &b.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 2));
    assert_eq!(fs::read_to_string(b.shots.join("20261120161819_1.jpg")).unwrap(), "jpeg 20261120161819_1.jpg");
    assert_eq!(
        fs::read_to_string(b.shots.join("thumbnails/20261120161819_1.jpg")).unwrap(),
        "thumb 20261120161819_1.jpg"
    );
    // Nothing new: nothing moves.
    let r = screenshots::sync(store, &b.cfg, &b.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 0));
    b.take_screenshot("20261121090000_1.jpg");
    let r = screenshots::sync(store, &b.cfg, &b.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (1, 0));

    // Back on A: B's screenshot arrives, and A's playtime sees B's sessions.
    a.activate();
    let r = screenshots::sync(store, &a.cfg, &a.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 1));
    assert_eq!(playtime::sync(store, &a.cfg, &Folders::of_key(KEY)).unwrap().total_secs, 3600 + 600 + 1200);
    let shots = screenshots::gallery(Some(store), &a.cfg, &Folders::of_key(KEY), &a.game).unwrap();
    assert_eq!(shots.len(), 3);
    assert!(shots.iter().all(|s| s.on_this_pc && s.in_cloud));
    assert_eq!(shots[0].device, "d-b");

    // A screenshot deleted in Steam is not downloaded again.
    fs::remove_file(a.shots.join("20261121090000_1.jpg")).unwrap();
    let r = screenshots::sync(store, &a.cfg, &a.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 0));

    // A lost its local playtime file: the cloud copy brings it back.
    fs::remove_file(a.home.join("playtime").join(format!("{KEY}.json"))).unwrap();
    assert_eq!(playtime::sync(store, &a.cfg, &Folders::of_key(KEY)).unwrap().total_secs, 3600 + 600 + 1200);
    assert_eq!(playtime::load_local(KEY).unwrap().sessions.len(), 1);

    // "Upload only" never downloads.
    let mut c = Device::new(tmp.path(), "d-c", 123456789);
    c.cfg.screenshots = ScreenshotMode::Upload;
    c.activate();
    let r = screenshots::sync(store, &c.cfg, &c.game, &quiet).unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 0));
}
