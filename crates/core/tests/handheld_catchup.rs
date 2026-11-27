//! Without the tray app (Xbox full screen experience, Deck Game Mode), the
//! next launch wrapper finishes the session of one that was terminated: its
//! playtime is recorded, its saves are uploaded and its marker is removed
//! (docs/plan-phase5.md §2.2). When the upload doesn't get through, the
//! game owes the sync after playing until it does.

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::sync::{self, Ctx};
use ludonimbus_core::{catchup, paths, playtime};
use std::collections::BTreeMap;
use std::fs;

#[test]
fn terminated_wrapper_is_finished_later() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let cloud = tmp.path().join("cloud");
    fs::create_dir_all(&cloud).unwrap();
    let saves = tmp.path().join("saves");
    fs::create_dir_all(&saves).unwrap();
    fs::write(saves.join("slot1.sav"), b"progress").unwrap();
    let game = GameEntry {
        key: "celeste".into(),
        name: "Celeste".into(),
        saves: vec!["{data}/**".into()],
        roots: BTreeMap::from([("data".to_string(), saves.to_string_lossy().into_owned())]),
        auto_sync: true,
        ..Default::default()
    };
    config::update(|c| {
        *c = AppConfig::default();
        c.cloud = CloudConfig::LocalFolder { path: cloud.clone() };
        c.games.push(game.clone());
        Ok(())
    })
    .unwrap();
    assert!(!catchup::app_running());

    // A wrapper that was killed 20 minutes into the session (its pid is gone).
    let started = chrono::Utc::now() - chrono::Duration::minutes(20);
    let marker = paths::running_dir().join("celeste.json");
    fs::write(&marker, serde_json::json!({ "pid": 4_000_000_000u32, "phase": "playing", "startedAt": started }).to_string()).unwrap();
    // The game being launched now is left alone.
    let other = paths::running_dir().join("other.json");
    fs::write(&other, serde_json::json!({ "pid": 4_000_000_001u32, "phase": "preLaunch" }).to_string()).unwrap();
    // A marker caught while it is written (empty for a moment, as older
    // versions wrote it in place) is a wrapper that is there, not a
    // terminated one.
    let writing = paths::running_dir().join("hollow.json");
    fs::write(&writing, "").unwrap();

    let cfg = config::load().unwrap();
    let done = catchup::stale_runners(&cfg, Some("other"), &|_| {});
    assert_eq!(done, vec!["celeste".to_string()]);
    assert!(!marker.exists());
    assert!(other.exists());
    assert!(writing.exists());
    assert_eq!(catchup::runner_phase("hollow").as_deref(), Some("playing"));
    assert!(playtime::summary(&cfg, "celeste").total_secs >= 19 * 60);
    let ctx = Ctx::load().unwrap();
    assert_eq!(ctx.cloud().unwrap().list_snapshots("celeste").unwrap().len(), 1);
    assert!(!sync::is_owed("celeste"));
    // Nothing left to do the second time.
    assert!(catchup::stale_runners(&cfg, Some("other"), &|_| {}).is_empty());
    fs::remove_file(&writing).unwrap();

    // Terminated again, and the cloud can't be reached this time: the sync
    // is still owed afterwards, and done once the cloud is back.
    fs::write(saves.join("slot1.sav"), b"more progress").unwrap();
    fs::write(&marker, serde_json::json!({ "pid": 4_000_000_000u32, "phase": "playing" }).to_string()).unwrap();
    let not_a_folder = tmp.path().join("not-a-folder");
    fs::write(&not_a_folder, b"x").unwrap();
    config::update(|c| {
        c.cloud = CloudConfig::LocalFolder { path: not_a_folder.clone() };
        Ok(())
    })
    .unwrap();
    let cfg = config::load().unwrap();
    assert_eq!(catchup::stale_runners(&cfg, Some("other"), &|_| {}), vec!["celeste".to_string()]);
    assert!(!marker.exists());
    assert!(sync::is_owed("celeste"));
    config::update(|c| {
        c.cloud = CloudConfig::LocalFolder { path: cloud.clone() };
        Ok(())
    })
    .unwrap();
    assert!(sync::finish_owed(&Ctx::load().unwrap(), "celeste").unwrap().is_some_and(|o| o.is_final()));
    assert!(!sync::is_owed("celeste"));
    assert_eq!(Ctx::load().unwrap().cloud().unwrap().list_snapshots("celeste").unwrap().len(), 2);
}
