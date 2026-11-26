//! End-to-end sync scenarios with two simulated devices sharing a local-folder
//! "cloud". Runs as one test because the data directory is chosen through an
//! environment variable.

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use ludonimbus_core::{games, state};
use std::fs;
use std::path::{Path, PathBuf};

struct Device {
    home: PathBuf,
}

impl Device {
    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }
}

fn setup_device(root: &Path, name: &str, cloud: &Path, game_dir: &Path) -> Device {
    let home = root.join(format!("home-{name}"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(game_dir).unwrap();
    fs::write(game_dir.join("game.exe"), b"exe").unwrap();
    let dev = Device { home };
    dev.activate();
    let mut cfg = AppConfig::default();
    cfg.device.name = name.to_string();
    cfg.cloud = CloudConfig::LocalFolder { path: cloud.to_path_buf() };
    cfg.games.push(GameEntry {
        key: "test-game".into(),
        name: "Test Game".into(),
        exe: Some(game_dir.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        exclude: vec!["{install}/save/cache/**".into()],
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

fn write_save(game_dir: &Path, slot: &str, content: &str) {
    let file = game_dir.join("save").join(slot);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, content).unwrap();
}

fn read_save(game_dir: &Path, slot: &str) -> String {
    fs::read_to_string(game_dir.join("save").join(slot)).unwrap_or_default()
}

fn run(trigger: Trigger) -> Outcome {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game("test-game").unwrap();
    sync::sync(&ctx, &game, Resolution::Auto, trigger).unwrap()
}

fn status() -> Status {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game("test-game").unwrap();
    sync::check(&ctx, &game).status
}

#[test]
fn two_devices_full_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let cloud = root.join("cloud");
    fs::create_dir_all(&cloud).unwrap();
    let game_a = root.join("pc-a").join("fefw-recomp-0.2.6");
    let game_b = root.join("pc-b").join("Games").join("fefw");

    let b = setup_device(root, "PC-B", &cloud, &game_b);
    let a = setup_device(root, "PC-A", &cloud, &game_a);

    // 1. First upload from A; cache files are excluded.
    a.activate();
    write_save(&game_a, "slot1.sav", "v1");
    write_save(&game_a, "cache/tmp.bin", "junk");
    assert_eq!(status(), Status::NotUploaded);
    assert!(matches!(run(Trigger::AutoExit), Outcome::Uploaded { .. }));
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(status(), Status::Synced);

    // 2. B has no saves (fresh install) → pulls, into a different folder.
    b.activate();
    assert_eq!(status(), Status::CanRestore);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::Pulled { .. }));
    assert_eq!(read_save(&game_b, "slot1.sav"), "v1");
    assert!(!game_b.join("save/cache/tmp.bin").exists());

    // 3. B plays → uploads v2.
    write_save(&game_b, "slot1.sav", "v2");
    assert_eq!(status(), Status::LocalChanged);
    assert!(matches!(run(Trigger::AutoExit), Outcome::Uploaded { .. }));

    // 4. A launches → cloud newer → pulls v2 (cache file untouched by restore).
    a.activate();
    assert_eq!(status(), Status::CloudNewer);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::Pulled { .. }));
    assert_eq!(read_save(&game_a, "slot1.sav"), "v2");

    // 5. Conflict: both change. B uploads first, then A exits.
    b.activate();
    write_save(&game_b, "slot1.sav", "v3-from-b");
    assert!(matches!(run(Trigger::AutoExit), Outcome::Uploaded { .. }));
    a.activate();
    write_save(&game_a, "slot1.sav", "v3-from-a");
    assert!(matches!(run(Trigger::AutoExit), Outcome::Conflict { .. }));
    assert_eq!(status(), Status::Conflict);
    {
        // A's saves were preserved in the cloud as a conflict-keep snapshot.
        let ctx = Ctx::load().unwrap();
        let list = ctx.cloud().unwrap().list_snapshots("test-game").unwrap();
        assert!(list.iter().any(|s| s.trigger == Trigger::ConflictKeep));
        // Choose the cloud version.
        let game = ctx.game("test-game").unwrap();
        assert!(matches!(
            sync::sync(&ctx, &game, Resolution::UseCloud, Trigger::Manual).unwrap(),
            Outcome::Pulled { .. }
        ));
    }
    assert_eq!(read_save(&game_a, "slot1.sav"), "v3-from-b");
    assert_eq!(status(), Status::Synced);
    assert!(state::game("test-game").conflict.is_none());

    // 6. Emulator update into a new folder without saves → relocate → pull.
    let game_a2 = root.join("pc-a").join("fefw-recomp-0.2.7");
    fs::create_dir_all(&game_a2).unwrap();
    fs::write(game_a2.join("game.exe"), b"exe").unwrap();
    games::set_exe("test-game", &game_a2.join("game.exe")).unwrap();
    assert_eq!(status(), Status::CanRestore);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::Pulled { .. }));
    assert_eq!(read_save(&game_a2, "slot1.sav"), "v3-from-b");

    // 7. Restore the very first version; it becomes the newest.
    let first_id = {
        let ctx = Ctx::load().unwrap();
        let list = ctx.cloud().unwrap().list_snapshots("test-game").unwrap();
        list.iter().find(|s| s.device_name == "PC-A" && s.trigger == Trigger::AutoExit).unwrap().id.clone()
    };
    {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        assert!(matches!(sync::restore(&ctx, &game, &first_id).unwrap(), Outcome::Uploaded { .. }));
    }
    assert_eq!(read_save(&game_a2, "slot1.sav"), "v1");
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));

    // 8. Offline: cloud folder disappears → after playing, the snapshot is queued.
    let cloud_moved = root.join("cloud-offline");
    fs::rename(&cloud, &cloud_moved).unwrap();
    write_save(&game_a2, "slot1.sav", "v4-offline");
    assert!(matches!(run(Trigger::AutoExit), Outcome::Queued { .. }));
    assert_eq!(state::game("test-game").pending.len(), 1);
    fs::rename(&cloud_moved, &cloud).unwrap();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::UpToDate));
    assert!(state::game("test-game").pending.is_empty());
    b.activate();
    assert!(matches!(run(Trigger::PreLaunch), Outcome::Pulled { .. }));
    assert_eq!(read_save(&game_b, "slot1.sav"), "v4-offline");

    // 9. Keeping the saves as they are: the cloud has this content already
    // (B just pulled it), so that version is kept under the name; nothing
    // new is uploaded.
    config::update(|c| {
        c.retention.keep_recent = 2;
        c.retention.keep_daily_days = 0;
        Ok(())
    })
    .unwrap();
    let count = || Ctx::load().unwrap().cloud().unwrap().list_snapshots("test-game").unwrap().len();
    let before = count();
    let kept = {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        let Outcome::Uploaded { id } = sync::backup_now(&ctx, &game, "before boss").unwrap() else { panic!("kept") };
        id
    };
    assert_eq!(count(), before);
    assert_eq!(status(), Status::Synced);

    // Changed saves kept by hand are a new version, the newest, and kept
    // from the start.
    write_save(&game_b, "slot1.sav", "v5-by-hand");
    let by_hand = {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        let Outcome::Uploaded { id } = sync::backup_now(&ctx, &game, "by hand").unwrap() else { panic!("uploaded") };
        id
    };
    assert_eq!(count(), before + 1);
    assert_eq!(status(), Status::Synced);

    // Old automatic versions are let go about once a day; kept ones stay.
    for i in 0..4 {
        write_save(&game_b, "slot1.sav", &format!("v5-{i}"));
        assert!(matches!(run(Trigger::AutoExit), Outcome::Uploaded { .. }));
    }
    state::update_game("test-game", |g| g.cleaned_at = Some(chrono::Utc::now() - chrono::Duration::days(2))).unwrap();
    write_save(&game_b, "slot1.sav", "v6");
    assert!(matches!(run(Trigger::AutoExit), Outcome::Uploaded { .. }));
    let ctx = Ctx::load().unwrap();
    let list = ctx.cloud().unwrap().list_snapshots("test-game").unwrap();
    let named = |id: &str| list.iter().find(|s| s.id == id).map(|s| (s.locked, s.comment.clone(), s.trigger));
    assert_eq!(named(&kept), Some((true, "before boss".to_string(), Trigger::AutoExit)));
    assert_eq!(named(&by_hand), Some((true, "by hand".to_string(), Trigger::Manual)));
    let autos = list.iter().filter(|s| !s.locked).count();
    assert_eq!(autos, 2, "the newest two automatic versions stay: {list:#?}");
    assert!(list.last().is_some_and(|s| s.head && !s.locked));

    // A is still at the version it uploaded offline, which B kept: it pulls
    // the newest without being asked anything.
    a.activate();
    assert_eq!(status(), Status::CloudNewer);
    assert!(matches!(run(Trigger::PreLaunch), Outcome::Pulled { .. }));
    assert_eq!(read_save(&game_a2, "slot1.sav"), "v6");
}
