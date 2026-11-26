//! Without the tray app (Xbox full screen experience, Deck Game Mode), the
//! next launch wrapper finishes the session of one that was terminated: its
//! playtime is recorded, its saves are uploaded and its marker is removed
//! (docs/plan-phase5.md §2.2).

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::sync::Ctx;
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

    let cfg = config::load().unwrap();
    let done = catchup::stale_runners(&cfg, Some("other"), &|_| {});
    assert_eq!(done, vec!["celeste".to_string()]);
    assert!(!marker.exists());
    assert!(other.exists());
    assert!(playtime::summary(&cfg, "celeste").total_secs >= 19 * 60);
    let ctx = Ctx::load().unwrap();
    assert_eq!(ctx.cloud().unwrap().list_snapshots("celeste").unwrap().len(), 1);
    // Nothing left to do the second time.
    assert!(catchup::stale_runners(&cfg, Some("other"), &|_| {}).is_empty());
}
