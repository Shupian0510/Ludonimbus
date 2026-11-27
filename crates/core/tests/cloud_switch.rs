//! Moving to another cloud: what a device keeps of a game's sync state (what
//! names versions by id stays, because copying the old cloud over keeps the
//! ids; what was about the old cloud itself goes), and what the first upload
//! to the new cloud builds on. One test, because the data directory is
//! chosen through an environment variable.

use chrono::Utc;
use ludonimbus_core::cloud::{self, v2};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::state::{self, ConflictInfo, Replaced};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::vv::Vv;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const KEY: &str = "test-game";
const MIB: usize = 1024 * 1024;

/// A fresh data directory with `cloud` connected and one game whose saves
/// are in `<root>/game/save`.
fn device(root: &Path, cloud: &CloudConfig) {
    std::env::set_var("LUDONIMBUS_HOME", root.join("home"));
    let game = root.join("game");
    fs::create_dir_all(game.join("save")).unwrap();
    fs::write(game.join("game.exe"), b"exe").unwrap();
    let mut cfg = AppConfig { cloud: cloud.clone(), ..AppConfig::default() };
    cfg.games.push(GameEntry {
        key: KEY.into(),
        name: "Test Game".into(),
        exe: Some(game.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        auto_sync: true,
        ..Default::default()
    });
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
}

fn upload() -> String {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(KEY).unwrap();
    match sync::sync(&ctx, &game, Resolution::Auto, Trigger::AutoExit).unwrap() {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

/// Every file of that name below `dir`.
fn find(dir: &Path, name: &str) -> Vec<std::path::PathBuf> {
    walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_name().to_string_lossy() == name).map(|e| e.into_path()).collect()
}

#[test]
fn moving_to_another_cloud() {
    let tmp = tempfile::tempdir().unwrap();

    // What a device remembers of each game.
    let root = tmp.path().join("state");
    let old = CloudConfig::LocalFolder { path: root.join("cloud-a") };
    let new = CloudConfig::LocalFolder { path: root.join("cloud-b") };
    device(&root, &old);
    let vv = Vv::next(std::iter::empty::<&Vv>(), "d-a", 4);
    let now = Utc::now();
    state::update_game(KEY, |g| {
        g.last_synced_id = Some("01JA0000000000000000000000".into());
        g.last_synced_hash = Some("hash".into());
        g.last_synced_vv = vv.clone();
        g.counter = 5;
        g.pending = vec!["01JB0000000000000000000000".into()];
        g.replaced = Some(Replaced { back: "01J90000000000000000000000".into(), now: "01JA0000000000000000000000".into(), device: "Deck".into(), at: now });
        g.conflict = Some(ConflictInfo {
            cloud_id: "01JC0000000000000000000000".into(),
            cloud_device: "Deck".into(),
            cloud_created_at: now,
            local_hash: "hash".into(),
            detected_at: now,
            candidates: Vec::new(),
            local_is_candidate: true,
        });
        g.last_error = Some("The cloud can't be reached right now".into());
        g.art_pushed = BTreeMap::from([("cover".to_string(), "c1".to_string())]);
        g.art_pulled = BTreeMap::from([("hero".to_string(), "h1".to_string())]);
    })
    .unwrap();

    cloud::switch_to(new.clone(), None).unwrap();
    let cfg = config::load().unwrap();
    assert_eq!(cfg.cloud, new);
    assert_eq!(cfg.previous_cloud, Some(old), "kept to copy from");

    let g = state::game(KEY);
    // Versions by id: as true in the new cloud once the history is copied.
    assert_eq!(g.last_synced_id.as_deref(), Some("01JA0000000000000000000000"));
    assert_eq!(g.last_synced_hash.as_deref(), Some("hash"));
    assert_eq!(g.last_synced_vv, vv);
    assert_eq!(g.counter, 5, "counts are never handed out twice");
    assert!(g.replaced.is_some());
    // What waits is this device's saves: it goes to the new cloud.
    assert_eq!(g.pending, ["01JB0000000000000000000000"]);
    // An image that came from the old cloud is still not this device's own.
    assert_eq!(g.art_pulled.get("hero").map(String::as_str), Some("h1"));
    // About the old cloud: its newest versions, its error, what it holds.
    assert!(g.conflict.is_none());
    assert!(g.last_error.is_none());
    assert!(g.art_pushed.is_empty(), "the new cloud has none of this device's artwork");
    assert!(g.line.is_none() && g.cleaned_at.is_none() && !g.cloud_def_uploaded);

    // A large save, synced with the old cloud. The first upload to the new
    // one doesn't take files from the old cloud's packs (the manifest it
    // would build on is forgotten with the rest of what was cached): they
    // aren't there, and copying the history over afterwards brings them
    // whole.
    let root = tmp.path().join("large");
    let (a, b) = (root.join("cloud-a"), root.join("cloud-b"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    device(&root, &CloudConfig::LocalFolder { path: a.clone() });
    let save = root.join("game").join("save");
    for n in 0..4u8 {
        fs::write(save.join(format!("world{n}.dat")), vec![n; 4 * MIB]).unwrap();
    }
    fs::write(save.join("slot1.sav"), b"one").unwrap();
    let first = upload();
    cloud::switch_to(CloudConfig::LocalFolder { path: b.clone() }, None).unwrap();
    fs::write(save.join("slot1.sav"), b"two").unwrap();
    let second = upload();
    let manifest: v2::Manifest = serde_json::from_slice(&fs::read(&find(&b, &format!("{second}.json"))[0]).unwrap()).unwrap();
    assert_eq!(manifest.packs().into_iter().collect::<Vec<_>>(), std::slice::from_ref(&second));
    assert!(find(&b, &format!("{first}.zip")).is_empty());

    cloud::migrate_previous(|_| {}).unwrap();
    let pack = format!("{first}.zip");
    assert_eq!(fs::read(&find(&b, &pack)[0]).unwrap(), fs::read(&find(&a, &pack)[0]).unwrap(), "the first version's pack, whole");
    let versions: Vec<String> = Ctx::load().unwrap().cloud().unwrap().list_snapshots(KEY).unwrap().into_iter().map(|s| s.id).collect();
    assert!(versions.contains(&first) && versions.contains(&second));
}
