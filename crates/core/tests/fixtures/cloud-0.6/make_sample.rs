//! Writes a sample cloud with the 0.6.2 code: the input of the migration
//! tests of later versions. Works in the folder named by `LN_SAMPLE_ROOT`
//! (a short neutral path, because absolute paths end up in the snapshots).

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

fn activate(home: &Path) {
    std::env::set_var("LUDONIMBUS_HOME", home);
}

fn device(root: &Path, name: &str, games: Vec<GameEntry>) -> PathBuf {
    let home = root.join(format!("home-{name}"));
    fs::create_dir_all(&home).unwrap();
    activate(&home);
    let mut cfg = AppConfig::default();
    cfg.device.name = name.to_string();
    cfg.cloud = CloudConfig::LocalFolder { path: root.join("cloud") };
    cfg.games = games;
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
    home
}

fn game(key: &str, name: &str, dir: &Path, exe: &str, saves: &[&str], adapter: Option<&str>) -> GameEntry {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(exe), b"exe").unwrap();
    GameEntry {
        key: key.into(),
        name: name.into(),
        exe: Some(dir.join(exe)),
        saves: saves.iter().map(|s| s.to_string()).collect(),
        adapter: adapter.map(str::to_string),
        auto_sync: true,
        ..Default::default()
    }
}

fn write(dir: &Path, rel: &str, content: &str) {
    let file = dir.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, content).unwrap();
}

fn run(key: &str, trigger: Trigger) -> Outcome {
    let ctx = Ctx::load().unwrap();
    let game = ctx.game(key).unwrap();
    // Snapshots a few milliseconds apart sort the same way everywhere.
    std::thread::sleep(std::time::Duration::from_millis(20));
    sync::sync(&ctx, &game, Resolution::Auto, trigger).unwrap()
}

fn uploaded(o: Outcome) -> String {
    match o {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

fn pulled(o: Outcome) {
    assert!(matches!(o, Outcome::Pulled { .. }), "expected a pull, got {o:?}");
}

#[test]
#[ignore]
fn make_sample() {
    let root = PathBuf::from(std::env::var("LN_SAMPLE_ROOT").expect("set LN_SAMPLE_ROOT"));
    assert!(!root.join("cloud").exists(), "the folder already holds a sample");
    fs::create_dir_all(root.join("cloud")).unwrap();

    let test_a = root.join("pc-a").join("test-game");
    let test_b = root.join("pc-b").join("Games").join("test-game");
    let fe_a = root.join("pc-a").join("fefw-recomp-0.2.6");
    let fe_b = root.join("pc-b").join("fefw");
    let abs = root.join("abs-saves");
    // A save folder outside every known root: recorded as `drive-c/...`.
    let abs_pattern = format!(
        "{{drive-{}}}/{}/**",
        abs.to_string_lossy()[..1].to_lowercase(),
        abs.to_string_lossy()[3..].replace('\\', "/")
    );

    let home_b = device(
        &root,
        "PC-B",
        vec![
            game("test-game", "Test Game", &test_b, "game.exe", &["{install}/save/**"], None),
            // The same game as PC-A's `fe-fortunes-weave`, added while offline.
            game("fe-fortunes-weave-2", "火焰纹章：万缕千丝", &fe_b, "fefw.exe", &[], Some("fe-fortunes-weave")),
        ],
    );
    let home_a = device(
        &root,
        "PC-A",
        vec![
            game("test-game", "Test Game", &test_a, "game.exe", &["{install}/save/**"], None),
            game("abs-game", "Abs Game", &root.join("pc-a").join("abs-game"), "abs.exe", &[abs_pattern.as_str()], None),
            game("fe-fortunes-weave", "火焰纹章：万缕千丝", &fe_a, "fefw.exe", &[], Some("fe-fortunes-weave")),
        ],
    );
    for (home, key) in [(&home_a, "test-game"), (&home_b, "test-game")] {
        activate(home);
        config::update(|c| {
            c.game_mut(key).unwrap().exclude = vec!["{install}/save/cache/**".into()];
            Ok(())
        })
        .unwrap();
    }

    // ---- test-game: two devices, a conflict, a restore, kept versions ----
    activate(&home_a);
    write(&test_a, "save/slot1.sav", "v1");
    write(&test_a, "save/cache/tmp.bin", "junk");
    let s1 = uploaded(run("test-game", Trigger::AutoExit));

    activate(&home_b);
    pulled(run("test-game", Trigger::PreLaunch));
    write(&test_b, "save/slot1.sav", "v2");
    write(&test_b, "save/slot2.sav", "second");
    uploaded(run("test-game", Trigger::AutoExit));

    activate(&home_a);
    pulled(run("test-game", Trigger::PreLaunch));

    // Both play from the same version; B uploads first.
    activate(&home_b);
    write(&test_b, "save/slot1.sav", "v3-from-b");
    uploaded(run("test-game", Trigger::AutoExit));
    activate(&home_a);
    write(&test_a, "save/slot1.sav", "v3-from-a");
    assert!(matches!(run("test-game", Trigger::AutoExit), Outcome::Conflict { .. }));
    {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        pulled(sync::sync(&ctx, &game, Resolution::UseCloud, Trigger::Manual).unwrap());
    }

    // Unsynced changes, then back to the first version: a pre-restore copy
    // and a restore.
    write(&test_a, "save/slot1.sav", "unsaved");
    {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        uploaded(sync::restore(&ctx, &game, &s1).unwrap());
        uploaded(sync::backup_now(&ctx, &game, "before boss").unwrap());
    }
    write(&test_a, "save/slot1.sav", "v4");
    let s_locked = uploaded(run("test-game", Trigger::AutoExit));
    {
        let ctx = Ctx::load().unwrap();
        let game = ctx.game("test-game").unwrap();
        sync::set_snapshot_flags(&ctx, &game, &s_locked, Some(true), Some("keep this".into())).unwrap();
    }

    activate(&home_b);
    pulled(run("test-game", Trigger::PreLaunch));
    for i in 0..3 {
        write(&test_b, "save/slot1.sav", &format!("v5-{i}"));
        uploaded(run("test-game", Trigger::AutoExit));
    }

    // ---- abs-game: a save folder no root covers ----
    activate(&home_a);
    write(&abs, "profile.dat", "p1");
    uploaded(run("abs-game", Trigger::AutoExit));
    write(&abs, "profile.dat", "p2");
    write(&abs, "sub/extra.dat", "x");
    uploaded(run("abs-game", Trigger::AutoExit));

    // ---- one game under two keys ----
    write(&fe_a, "save/slot0.sav", "a1");
    uploaded(run("fe-fortunes-weave", Trigger::AutoExit));
    write(&fe_a, "save/slot0.sav", "a2");
    uploaded(run("fe-fortunes-weave", Trigger::AutoExit));
    activate(&home_b);
    write(&fe_b, "save/slot0.sav", "b1");
    uploaded(run("fe-fortunes-weave-2", Trigger::AutoExit));

    // ---- what every version must restore to ----
    let ctx = Ctx::load().unwrap();
    let cloud = ctx.cloud().unwrap();
    let mut expected = BTreeMap::new();
    for key in ["test-game", "abs-game", "fe-fortunes-weave", "fe-fortunes-weave-2"] {
        let mut list = Vec::new();
        for s in cloud.list_snapshots(key).unwrap() {
            let zip_path = root.join("check.zip");
            cloud.download(key, &s.id, &zip_path).unwrap();
            let mut archive = zip::ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap();
            let mut files = BTreeMap::new();
            for i in 0..archive.len() {
                let mut e = archive.by_index(i).unwrap();
                let Some(path) = e.name().strip_prefix("files/").map(str::to_string) else { continue };
                let mut text = String::new();
                e.read_to_string(&mut text).unwrap();
                files.insert(path, text);
            }
            drop(archive);
            fs::remove_file(&zip_path).unwrap();
            list.push(serde_json::json!({
                "id": s.id,
                "parent": s.parent,
                "trigger": s.trigger,
                "deviceName": s.device_name,
                "comment": s.comment,
                "locked": s.locked,
                "contentHash": s.content_hash,
                "files": files,
            }));
        }
        expected.insert(key, list);
    }
    fs::write(root.join("expected.json"), serde_json::to_vec_pretty(&expected).unwrap()).unwrap();
}
