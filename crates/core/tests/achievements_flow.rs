//! Achievements with two simulated devices sharing a local-folder "cloud":
//! reading RPCS3 trophy files, announcing only new unlocks, and the
//! append-only device files. One test, because the data directory is chosen
//! through an environment variable.

use ludonimbus_core::achievements::{self, cloud, Binding, SourceKind};
use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::config::{AppConfig, GameEntry};
use std::fs;
use std::path::Path;

const CONF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!--Sce-Np-Trophy-Signature: abc-->
<trophyconf version="1.0">
<title-name>Test Game</title-name>
<trophy id="000" hidden="no" ttype="P" pid="-1"><name>All</name><detail>Get them all</detail></trophy>
<trophy id="001" hidden="no" ttype="B" pid="000"><name>First</name><detail>Start</detail></trophy>
<trophy id="002" hidden="yes" ttype="G" pid="000"><name>Boss</name><detail>Beat the boss</detail></trophy>
</trophyconf>"#;

/// A TROPUSR.DAT with three trophies; `unlocked` = (id, unix seconds).
fn tropusr(unlocked: &[(u32, i64)]) -> Vec<u8> {
    const RTC: i64 = 62_135_596_800;
    let n = 3usize;
    let mut d = vec![0u8; 0x30];
    d[0..4].copy_from_slice(&0x818F54ADu32.to_be_bytes());
    d[8..12].copy_from_slice(&1u32.to_be_bytes());
    let off = 0x30 + 0x20;
    let mut h = vec![0u8; 0x20];
    h[0..4].copy_from_slice(&6u32.to_be_bytes());
    h[4..8].copy_from_slice(&0x60u32.to_be_bytes());
    h[12..16].copy_from_slice(&(n as u32).to_be_bytes());
    h[16..24].copy_from_slice(&(off as u64).to_be_bytes());
    d.extend(h);
    for id in 0..n as u32 {
        let mut e = vec![0u8; 0x60];
        e[0..4].copy_from_slice(&6u32.to_be_bytes());
        e[16..20].copy_from_slice(&id.to_be_bytes());
        if let Some((_, t)) = unlocked.iter().find(|(i, _)| *i == id) {
            e[20..24].copy_from_slice(&1u32.to_be_bytes());
            let tick = ((t + RTC) as u64) * 1_000_000;
            e[32..40].copy_from_slice(&tick.to_be_bytes());
        }
        d.extend(e);
    }
    d
}

fn device(home: &Path, id: &str, name: &str) -> AppConfig {
    std::env::set_var("LUDONIMBUS_HOME", home);
    let mut cfg = AppConfig::default();
    cfg.device.id = id.into();
    cfg.device.name = name.into();
    cfg
}

#[test]
fn two_devices() {
    let tmp = tempfile::tempdir().unwrap();
    let cloud_dir = tmp.path().join("cloud");
    fs::create_dir_all(&cloud_dir).unwrap();
    let repo = LocalFolder::new(cloud_dir.clone());
    let game = GameEntry { key: "demo".into(), name: "Test Game".into(), ..Default::default() };

    // --- Device A: link an RPCS3 trophy set that already has one unlock.
    let set_a = tmp.path().join("a/dev_hdd0/home/00000001/trophy/NPWR00001_00");
    fs::create_dir_all(&set_a).unwrap();
    fs::write(set_a.join("TROPCONF.SFM"), CONF).unwrap();
    fs::write(set_a.join("TROP002.PNG"), b"\x89PNG").unwrap();
    fs::write(set_a.join("TROPUSR.DAT"), tropusr(&[(1, 1_763_800_000)])).unwrap();
    let cfg_a = device(&tmp.path().join("home-a"), "dev-a", "Desktop");
    let binding = Binding { source: SourceKind::Rpcs3, source_id: "NPWR00001_00".into(), path: Some(set_a.clone()), popup: None };
    let db = achievements::link(&cfg_a, &game, binding).unwrap();
    assert_eq!(db.mine.len(), 1);
    assert!(db.notified.contains("1"), "history is never announced");

    // Nothing new: nothing announced.
    assert!(achievements::refresh(&cfg_a, &game, true).unwrap().is_empty());

    // A new unlock while playing is announced once, with its name.
    fs::write(set_a.join("TROPUSR.DAT"), tropusr(&[(1, 1_763_800_000), (2, 1_763_900_000)])).unwrap();
    let new = achievements::refresh(&cfg_a, &game, true).unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].achievement.name, "Boss");
    assert!(achievements::refresh(&cfg_a, &game, true).unwrap().is_empty());

    let v = achievements::view("demo");
    assert_eq!((v.total, v.unlocked), (3, 2));
    assert_eq!(v.items[0].id, "2", "newest unlock first");
    assert!(v.items[0].has_icon);
    assert!(achievements::icon_data_url(&achievements::icon_path("demo", "2").unwrap()).unwrap().starts_with("data:image/png;base64,"));

    assert!(cloud::push(&repo, "demo", "dev-a", "Desktop").unwrap());
    assert!(!cloud::push(&repo, "demo", "dev-a", "Desktop").unwrap(), "unchanged: nothing written");
    let files: Vec<String> = repo.list("games/demo/achievements").unwrap().into_iter().map(|e| e.name).collect();
    assert!(files.contains(&"dev-a.json".to_string()) && files.contains(&"schema.json".to_string()));
    let schema = String::from_utf8(repo.read("games/demo/achievements/schema.json").unwrap().unwrap()).unwrap();
    assert!(!schema.contains("TROP002.PNG"), "local icon paths stay local");

    // --- Device B: no source of its own yet; sees A's records.
    let cfg_b = device(&tmp.path().join("home-b"), "dev-b", "Laptop");
    assert_eq!(cloud::pull(&repo, "demo", "dev-b").unwrap(), 2);
    let v = achievements::view("demo");
    assert_eq!((v.total, v.unlocked), (3, 2));
    assert_eq!(v.items[0].device_name.as_deref(), Some("Desktop"));

    // B earns trophy 0 (and trophy 1 earlier than A did).
    let set_b = tmp.path().join("b/dev_hdd0/home/00000001/trophy/NPWR00001_00");
    fs::create_dir_all(&set_b).unwrap();
    fs::write(set_b.join("TROPCONF.SFM"), CONF).unwrap();
    fs::write(set_b.join("TROPUSR.DAT"), tropusr(&[(0, 1_763_950_000), (1, 1_763_700_000)])).unwrap();
    achievements::link(&cfg_b, &game, Binding { source: SourceKind::Rpcs3, source_id: "NPWR00001_00".into(), path: Some(set_b), popup: None }).unwrap();
    cloud::sync(&repo, "demo", "dev-b", "Laptop").unwrap();
    let v = achievements::view("demo");
    assert_eq!(v.unlocked, 3);

    // --- Back on A: merge B's records; trophy 1 now belongs to B (earlier).
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home-a"));
    cloud::pull(&repo, "demo", "dev-a").unwrap();
    let v = achievements::view("demo");
    assert_eq!(v.unlocked, 3);
    let first = v.items.iter().find(|i| i.id == "1").unwrap();
    assert_eq!(first.device_name.as_deref(), Some("Laptop"));

    // --- A resets its trophies and loses its local database: the cloud file
    // keeps every unlock it ever uploaded.
    fs::write(set_a.join("TROPUSR.DAT"), tropusr(&[])).unwrap();
    achievements::unlink("demo").unwrap();
    achievements::link(&cfg_a, &game, Binding { source: SourceKind::Rpcs3, source_id: "NPWR00001_00".into(), path: Some(set_a.clone()), popup: None }).unwrap();
    fs::write(set_a.join("TROPUSR.DAT"), tropusr(&[(0, 1_764_000_000)])).unwrap();
    achievements::refresh(&cfg_a, &game, true).unwrap();
    cloud::push(&repo, "demo", "dev-a", "Desktop").unwrap();
    let file: cloud::DeviceFile = serde_json::from_slice(&repo.read("games/demo/achievements/dev-a.json").unwrap().unwrap()).unwrap();
    let mut ids: Vec<&str> = file.unlocks.iter().map(|u| u.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["0", "1", "2"]);
}
