//! Quark through OpenList with a real account, as two devices: two OpenList
//! servers (own data folder, port and listing cache each) signed in with the
//! same cookie. What one device writes, overwrites or deletes must show up
//! on the other. Ignored by default; `scripts/quark-e0.ps1 -TwoDevices`
//! asks for the cookie and runs it:
//!
//! ```text
//! LUDONIMBUS_QUARK_COOKIE=… LUDONIMBUS_OPENLIST_ZIP=…\openlist-windows-amd64.zip \
//!   cargo test -p ludonimbus-core --test quark_live -- --ignored --nocapture
//! ```
//!
//! Everything goes in a "Ludonimbus-E1" folder in the drive, which is deleted
//! at the end (also when a check fails), together with both OpenList servers
//! and their data. Like the app, every step opens the cloud anew.

use ludonimbus_core::cloud::{self, repo::Repo, store::Store, CloudProvider};
use ludonimbus_core::config::CloudConfig;
use ludonimbus_core::openlist;
use ludonimbus_core::snapshot::{SnapshotMeta, Trigger};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TEST_FOLDER: &str = "Ludonimbus-E1";

fn meta(id: &str) -> SnapshotMeta {
    SnapshotMeta {
        id: id.into(),
        game: "fe".into(),
        parent: None,
        device_id: "d-a".into(),
        device_name: "DEVICE-A".into(),
        created_at: chrono::Utc::now(),
        trigger: Trigger::AutoExit,
        roots: BTreeMap::new(),
        content_hash: format!("hash-{id}"),
        files: Vec::new(),
        total_size: 3,
        app_version: "e1".into(),
        comment: String::new(),
        locked: false,
        game_version: None,
        installed_version: None,
    }
}

/// One "device": its own LUDONIMBUS_HOME, hence its own OpenList.
struct Device {
    home: PathBuf,
    cfg: CloudConfig,
}

impl Device {
    fn enter(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }
    fn store(&self) -> Box<dyn Store> {
        self.enter();
        cloud::build_store(&self.cfg, None, Duration::from_secs(30)).unwrap()
    }
    fn repo(&self) -> Repo<Box<dyn Store>> {
        Repo::new(self.store())
    }
}

/// Deletes the test folder in the drive and both servers, even after a failed check.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let mut removed = false;
        for home in &self.0 {
            std::env::set_var("LUDONIMBUS_HOME", home);
            if !removed {
                if let Ok(run) = openlist::ensure_running() {
                    if let Ok(api) = openlist::Api::login(run.port) {
                        removed = api.remove_path(&format!("{}/{TEST_FOLDER}", openlist::QUARK_MOUNT)).is_ok();
                    }
                }
            }
            openlist::forget();
        }
        println!(
            "cleanup: drive folder {}, OpenList servers stopped and their data deleted",
            if removed { "deleted" } else { "NOT deleted (delete Ludonimbus-E1 in Quark yourself)" }
        );
    }
}

#[test]
#[ignore]
fn quark_two_devices() {
    let cookie = std::env::var("LUDONIMBUS_QUARK_COOKIE").expect("set LUDONIMBUS_QUARK_COOKIE");
    let zip = std::env::var("LUDONIMBUS_OPENLIST_ZIP").expect("set LUDONIMBUS_OPENLIST_ZIP");
    let tmp = tempfile::tempdir().unwrap();
    let folder = format!("{TEST_FOLDER}/run-{}", ulid::Ulid::new().to_string().to_lowercase());
    let cfg = CloudConfig::Quark { folder: folder.clone() };
    let a = Device { home: tmp.path().join("a"), cfg: cfg.clone() };
    let b = Device { home: tmp.path().join("b"), cfg };
    let _cleanup = Cleanup(vec![a.home.clone(), b.home.clone()]);
    let t = Instant::now();
    let timed = |what: &str, since: Instant| println!("[{:>6.1}s] OK  {what} ({:.1} s)", t.elapsed().as_secs_f64(), since.elapsed().as_secs_f64());

    for d in [&a, &b] {
        d.enter();
        openlist::install(Some(Path::new(&zip)), &|_, _| {}).unwrap();
        let s0 = Instant::now();
        openlist::connect_quark(&cookie, &folder).unwrap();
        timed("device signed in (OpenList started, Quark mounted)", s0);
    }

    let s0 = Instant::now();
    a.store().check().unwrap();
    b.store().check().unwrap();
    timed("both see the folder", s0);

    let s0 = Instant::now();
    a.store().write("games/fe/game.json", b"{\"v\":1}").unwrap();
    timed("A: write a small file", s0);
    let s0 = Instant::now();
    assert_eq!(b.store().read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":1}"[..]), "B reads A's new file");
    timed("B: read it", s0);

    let s0 = Instant::now();
    a.store().write("games/fe/game.json", b"{\"v\":22}").unwrap();
    timed("A: overwrite it", s0);
    let s0 = Instant::now();
    assert_eq!(b.store().read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":22}"[..]), "B reads the new content, not a cached link");
    timed("B: read the new content", s0);
    let names: Vec<String> = a.store().list("games/fe").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["game.json"], "one file, no leftovers from the overwrite: {names:?}");

    // A full upload as after a play session, in a folder B hasn't seen yet;
    // then B checks before launching.
    let zip_file = tmp.path().join("snap.zip");
    let data: Vec<u8> = (0..1_000_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    std::fs::write(&zip_file, &data).unwrap();
    let s0 = Instant::now();
    a.repo().upload(&meta("01E1SNAP"), &zip_file).unwrap();
    timed("A: upload a 1 MB save version (zip + metadata)", s0);
    let s0 = Instant::now();
    let list = b.repo().list_snapshots("fe").unwrap();
    assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["01E1SNAP"], "B sees A's new version in a new folder");
    timed("B: list versions before launching", s0);
    let s0 = Instant::now();
    let out = tmp.path().join("back.zip");
    b.repo().download("fe", "01E1SNAP", &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), data);
    timed("B: download it", s0);

    let s0 = Instant::now();
    b.store().remove("games/fe/game.json").unwrap();
    assert!(a.store().list("games/fe").unwrap().iter().all(|e| e.name != "game.json"), "A sees B's delete");
    timed("B deletes, A sees it", s0);

    let s0 = Instant::now();
    assert!(b.repo().list_snapshots("zelda").unwrap().is_empty());
    timed("B: list versions of a game nobody uploaded", s0);
    println!("ALL CHECKS PASSED in {:.1} s", t.elapsed().as_secs_f64());
}
