//! Store and cloud-layout operations against a real WebDAV server. Ignored by
//! default. Used to check servers that put their own logic between WebDAV and
//! the storage, such as OpenList with the Quark driver (`scripts/quark-e0.ps1`
//! sets everything up and runs this test).
//!
//! ```text
//! LUDONIMBUS_DAV_URL=http://127.0.0.1:5245/dav/ LUDONIMBUS_DAV_USER=… LUDONIMBUS_DAV_PASS=… \
//!   cargo test -p ludonimbus-core --test webdav_live -- --ignored --nocapture
//! ```
//!
//! Everything is written to a new `live-test-…` folder below
//! `LUDONIMBUS_DAV_FOLDER` (default: the server root) and deleted at the end.
//! Set `LUDONIMBUS_DAV_BIG_MB` to change the size of the large file (default 10).

use ludonimbus_core::cloud::{self, repo::Repo, CloudProvider, DeviceInfo};
use ludonimbus_core::config::CloudConfig;
use ludonimbus_core::snapshot::{SnapshotMeta, Trigger};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn meta(id: &str, parent: Option<&str>) -> SnapshotMeta {
    SnapshotMeta {
        id: id.into(),
        game: "e0".into(),
        parent: parent.map(str::to_string),
        device_id: "d-e0".into(),
        device_name: "E0-TEST".into(),
        created_at: chrono::Utc::now(),
        trigger: Trigger::AutoExit,
        roots: BTreeMap::new(),
        content_hash: format!("hash-{id}"),
        files: Vec::new(),
        total_size: 3,
        app_version: "e0".into(),
        comment: String::new(),
        locked: false,
        game_version: None,
        installed_version: None,
    }
}

#[test]
#[ignore]
fn webdav_live() {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let url = env("LUDONIMBUS_DAV_URL");
    assert!(!url.is_empty(), "set LUDONIMBUS_DAV_URL / LUDONIMBUS_DAV_USER / LUDONIMBUS_DAV_PASS");
    let password = env("LUDONIMBUS_DAV_PASS");
    let big_mb: usize = env("LUDONIMBUS_DAV_BIG_MB").parse().unwrap_or(10);
    let base = env("LUDONIMBUS_DAV_FOLDER");
    let dir = format!("live-test-{}", ulid::Ulid::new().to_string().to_lowercase());
    let folder = if base.is_empty() { dir.clone() } else { format!("{}/{dir}", base.trim_matches('/')) };
    let cfg = CloudConfig::WebDav { url, username: env("LUDONIMBUS_DAV_USER"), folder };
    let timeout = Duration::from_secs(20);

    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let t = Instant::now();
    let step = |what: &str| println!("[{:>6.1}s] OK  {what}", t.elapsed().as_secs_f64());

    // What "Test connection" in the settings page does (write, read back, delete).
    let account = cloud::probe(&cfg, Some(&password), timeout).unwrap();
    step(&format!("test connection: {} (used {:?} / total {:?})", account.label, account.used, account.total));

    let s = cloud::build_store(&cfg, Some(&password), timeout).unwrap();
    assert!(s.list("missing").unwrap().is_empty());
    assert_eq!(s.read("missing.json").unwrap(), None);
    s.remove("missing.json").unwrap();
    step("missing folder / file handled");

    // Small files: new nested folders, then overwrite in place.
    let json = "games/e0/game.json";
    s.write(json, b"{\"v\":1}").unwrap();
    step("small write into new nested folders (upload + rename)");
    assert_eq!(s.read(json).unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    step("small read");
    let before = s.list("games/e0").unwrap();
    assert_eq!(before.len(), 1, "no temporary files left behind: {before:?}");
    s.write(json, b"{\"v\":22}").unwrap();
    let after = s.list("games/e0").unwrap();
    assert_eq!(after.len(), 1, "overwritten in place: {after:?}");
    assert_eq!(s.read(json).unwrap().as_deref(), Some(&b"{\"v\":22}"[..]), "overwrite visible right away");
    step("overwrite an existing file");

    // A save-sized file both ways.
    let big: Vec<u8> = (0..(big_mb * 1_000_000) as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    let src = tmp.path().join("big.zip");
    std::fs::write(&src, &big).unwrap();
    let up = Instant::now();
    s.upload("games/e0/snapshots/big.zip", &src).unwrap();
    let secs = up.elapsed().as_secs_f64();
    step(&format!("{big_mb} MB upload ({secs:.1} s, {:.0} KB/s)", big.len() as f64 / 1000.0 / secs));
    let back = tmp.path().join("back.zip");
    let down = Instant::now();
    s.download("games/e0/snapshots/big.zip", &back).unwrap();
    let secs = down.elapsed().as_secs_f64();
    assert_eq!(std::fs::read(&back).unwrap(), big, "downloaded bytes match");
    step(&format!("{big_mb} MB download ({secs:.1} s, {:.0} KB/s)", big.len() as f64 / 1000.0 / secs));
    s.remove("games/e0/snapshots/big.zip").unwrap();
    assert!(s.list("games/e0/snapshots").unwrap().iter().all(|e| e.name != "big.zip"));
    step("delete a file");

    // The cloud layout on top: snapshots with metadata, notes, devices.
    let repo = Repo::new(cloud::build_store(&cfg, Some(&password), timeout).unwrap());
    let ids = ["01E0AAA", "01E0BBB", "01E0CCC"];
    for (i, id) in ids.iter().enumerate() {
        let zip = tmp.path().join(format!("{id}.zip"));
        std::fs::write(&zip, format!("zip-{id}")).unwrap();
        repo.upload(&meta(id, (i > 0).then(|| ids[i - 1])), &zip).unwrap();
    }
    let list = repo.list_snapshots("e0").unwrap();
    assert_eq!(list.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ids);
    step("upload 3 snapshots and list them");
    let mut m = repo.get_meta("e0", "01E0AAA").unwrap();
    m.comment = "通关前 / before the boss".into();
    m.locked = true;
    repo.update_meta(&m).unwrap();
    let edited = repo.list_snapshots("e0").unwrap();
    assert_eq!(edited[0].comment, "通关前 / before the boss");
    assert!(edited[0].locked);
    step("edit a note and lock (metadata overwrite)");
    let out = tmp.path().join("dl.zip");
    repo.download("e0", "01E0BBB", &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"zip-01E0BBB");
    step("download a snapshot");
    repo.delete("e0", "01E0BBB").unwrap();
    let left: Vec<String> = repo.list_snapshots("e0").unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(left, ["01E0AAA", "01E0CCC"]);
    step("delete a snapshot");
    repo.put_device(&DeviceInfo {
        id: "d-e0".into(),
        name: "E0-TEST".into(),
        os: "windows".into(),
        app_version: "e0".into(),
        last_seen: chrono::Utc::now(),
        layout: 0,
        installs: Vec::new(),
    })
    .unwrap();
    repo.put_device(&DeviceInfo {
        id: "d-e0".into(),
        name: "E0-TEST-2".into(),
        os: "windows".into(),
        app_version: "e0".into(),
        last_seen: chrono::Utc::now(),
        layout: 0,
        installs: Vec::new(),
    })
    .unwrap();
    let devices = repo.list_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "E0-TEST-2");
    step("write and rewrite a device");

    // Clean up: remove everything this test created.
    s.remove("").unwrap();
    step("cleaned up");
    println!("ALL CHECKS PASSED in {:.1} s", t.elapsed().as_secs_f64());
}
