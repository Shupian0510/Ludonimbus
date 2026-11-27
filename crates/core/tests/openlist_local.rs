//! The OpenList manager and the Quark store end to end, with OpenList's
//! local storage in place of Quark: install from the release zip, start,
//! mount, the WebDAV-only user, uploads straight to the final name, fresh
//! listings after "another device" changed files, stop and forget.
//! Ignored by default; needs the official zip:
//!
//! ```text
//! LUDONIMBUS_OPENLIST_ZIP=…\openlist-windows-amd64.zip cargo test -p ludonimbus-core --test openlist_local -- --ignored --nocapture
//! ```

use ludonimbus_core::cloud::{self, repo::Repo, CloudProvider};
use ludonimbus_core::config::CloudConfig;
use ludonimbus_core::openlist::{self, Api};
use ludonimbus_core::snapshot::{SnapshotMeta, Trigger};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn meta(id: &str, parent: Option<&str>) -> SnapshotMeta {
    SnapshotMeta {
        id: id.into(),
        game: "e1".into(),
        parent: parent.map(str::to_string),
        device_id: "d-e1".into(),
        device_name: "E1".into(),
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

#[test]
#[ignore]
fn openlist_local() {
    let zip = std::env::var("LUDONIMBUS_OPENLIST_ZIP").expect("set LUDONIMBUS_OPENLIST_ZIP");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let t = Instant::now();
    let step = |what: &str| println!("[{:>5.1}s] OK  {what}", t.elapsed().as_secs_f64());

    assert!(!openlist::installed());
    openlist::install(Some(std::path::Path::new(&zip)), &|_, _| {}).unwrap();
    assert!(openlist::installed());
    step("installed from the zip (sha256 checked)");

    let run = openlist::ensure_running().unwrap();
    assert!(run.started);
    let again = openlist::ensure_running().unwrap();
    assert!(!again.started && again.port == run.port);
    step(&format!("started on 127.0.0.1:{} (second call reuses it)", run.port));

    // Local storage instead of Quark; the "drive" is a folder on disk.
    let drive = tmp.path().join("drive");
    std::fs::create_dir_all(&drive).unwrap();
    let api = Api::login(run.port).unwrap();
    api.put_storage(openlist::QUARK_MOUNT, "Local", serde_json::json!({ "root_folder_path": drive.display().to_string() })).unwrap();
    api.mkdir("/quark/Ludonimbus").unwrap();
    api.mkdir("/quark/Ludonimbus").unwrap(); // already exists: fine
    api.ensure_webdav_user("/quark/Ludonimbus").unwrap();
    step("mounted, folder and WebDAV user ready");

    let cfg = CloudConfig::Quark { folder: "Ludonimbus".into() };
    let s = cloud::build_store(&cfg, None, Duration::from_secs(20)).unwrap();
    s.check().unwrap();
    s.write("games/e1/game.json", b"{\"v\":1}").unwrap();
    assert_eq!(s.read("games/e1/game.json").unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    s.write("games/e1/game.json", b"{\"v\":22}").unwrap();
    assert_eq!(s.read("games/e1/game.json").unwrap().as_deref(), Some(&b"{\"v\":22}"[..]));
    let names: Vec<String> = s.list("games/e1").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["game.json"], "no temporary files: uploads go straight to the final name");
    step("write, overwrite, read, list");

    // Another device changes files behind this OpenList's cache.
    let on_disk = drive.join("Ludonimbus").join("games").join("e1");
    std::fs::write(on_disk.join("other.json"), b"from another device").unwrap();
    let names: Vec<String> = s.list("games/e1").unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"other.json".to_string()), "fresh listing: {names:?}");
    std::fs::write(on_disk.join("game.json"), b"{\"v\":333}").unwrap();
    assert_eq!(s.read("games/e1/game.json").unwrap().as_deref(), Some(&b"{\"v\":333}"[..]), "fresh read");
    // A folder the other device created (its parent's cached listing doesn't have it).
    std::fs::create_dir_all(on_disk.join("snapshots")).unwrap();
    std::fs::write(on_disk.join("snapshots").join("01NEW.json"), b"{}").unwrap();
    let names: Vec<String> = s.list("games/e1/snapshots").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["01NEW.json"], "new folder made elsewhere");
    std::fs::create_dir_all(drive.join("Ludonimbus").join("games").join("e2").join("snapshots")).unwrap();
    std::fs::write(drive.join("Ludonimbus").join("games").join("e2").join("snapshots").join("01X.json"), b"{}").unwrap();
    assert_eq!(s.list("games/e2/snapshots").unwrap().len(), 1, "several new levels made elsewhere");
    assert!(s.list("games/nothing/snapshots").unwrap().is_empty(), "a folder that really doesn't exist is empty");
    step("changes made elsewhere show up (refresh before list / read)");

    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmp.path().join("big.zip");
    std::fs::write(&src, &big).unwrap();
    s.upload("games/e1/snapshots/big.zip", &src).unwrap();
    let back = tmp.path().join("back.zip");
    s.download("games/e1/snapshots/big.zip", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big);
    s.remove("games/e1/snapshots/big.zip").unwrap();
    step("3 MB upload, download, delete");

    let repo = Repo::new(cloud::build_store(&cfg, None, Duration::from_secs(20)).unwrap());
    for (i, id) in ["01E1A", "01E1B"].iter().enumerate() {
        let z = tmp.path().join(format!("{id}.zip"));
        std::fs::write(&z, id).unwrap();
        repo.upload(&meta(id, (i > 0).then_some("01E1A")), &z).unwrap();
    }
    assert_eq!(repo.list_snapshots("e1").unwrap().len(), 2);
    step("snapshots through the cloud layout");

    // The WebDAV user can't leave its folder.
    assert!(s.read("../outside.json").unwrap_or(None).is_none());

    openlist::stop();
    std::thread::sleep(Duration::from_millis(800));
    assert!(openlist::ensure_running().unwrap().started, "stopped, so it starts again");
    step("stop, start again");

    openlist::forget();
    assert!(openlist::installed(), "the program stays");
    assert!(openlist::webdav_password().is_err(), "passwords are gone");
    step("forget (data and passwords removed)");
    println!("ALL CHECKS PASSED in {:.1} s", t.elapsed().as_secs_f64());
}
