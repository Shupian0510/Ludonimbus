//! Stardew Valley across Switch and PC (rules/games/stardew-valley.yaml,
//! docs/plan-phase5.md §5.6): the Switch uploads its farm through the PC; the
//! PC's Steam-style version sees the Switch version as the same game, gets
//! the Switch farm applied into `StardewValley/Saves` (settings left out),
//! is warned that the farm was saved by a newer game version than its own
//! saves, and every save version records the game version that wrote it.
//! Then the PC's farms go back into a Switch version's save data, and into
//! the Switch itself, which is only in the cloud: on top of its newest
//! version, as its new newest version.

use ludonimbus_core::cloud::CloudGame;
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::sync::{self, Ctx, Outcome};
use ludonimbus_core::{crosssaves, savelib, transfer};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const TITLE: &str = "0100e65002bb8000";

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn info(version: &str, farm: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\" encoding=\"utf-8\"?><Farmer><name>{farm}</name><gameVersion>{version}</gameVersion></Farmer>").into_bytes()
}

fn add(g: GameEntry) -> GameEntry {
    config::update(|c| {
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();
    g
}

fn latest(ctx: &Ctx, key: &str) -> snapshot::SnapshotSummary {
    let mut list = ctx.cloud().unwrap().list_snapshots(key).unwrap();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    list.into_iter().find(|s| s.trigger.is_mainline()).unwrap()
}

#[test]
fn stardew_valley_between_switch_and_pc() {
    let tmp = tempfile::tempdir().unwrap();
    let cloud = tmp.path().join("cloud");
    fs::create_dir_all(&cloud).unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    config::update(|c| {
        *c = AppConfig::default();
        c.device.id = "d-pc".into();
        c.device.name = "PC".into();
        c.cloud = CloudConfig::LocalFolder { path: cloud.clone() };
        Ok(())
    })
    .unwrap();
    let ctx = Ctx::load().unwrap();

    // The Switch uploads its farm (saved by 1.6.15) through the PC, as the
    // Switch client does: the game is defined by its title id.
    let switch_key = format!("nx-{TITLE}");
    let nx = |p: &str| format!("nxsave-{TITLE}/{p}");
    let files = vec![
        (nx("Farm_111/Farm_111"), b"<SaveGame>switch farm</SaveGame>".to_vec()),
        (nx("Farm_111/SaveGameInfo"), info("1.6.15", "Switch")),
        (nx("startup_preferences"), b"switch settings".to_vec()),
    ];
    let version = crosssaves::files_version(&[format!("nx:{}", TITLE.to_uppercase())], "switch", &files, &BTreeMap::new());
    assert_eq!(version.as_deref(), Some("1.6.15"));
    let c = ctx.cloud().unwrap();
    c.put_game(&CloudGame {
        key: switch_key.clone(),
        name: "Stardew Valley".into(),
        adapter: None,
        saves: vec![format!("{{nxsave-{TITLE}}}/**"), format!("{{nxdevice-{TITLE}}}/**")],
        exclude: vec![],
        wait_for: vec![],
        roots: Default::default(),
        ludusavi_title: None,
        source: None,
        ids: vec![format!("nx:{}", TITLE.to_uppercase())],
        game: None,
    })
    .unwrap();
    let meta = sync::queue_files(
        &c.open(&switch_key).unwrap(),
        &sync::Foreign {
            key: &switch_key,
            files: &files,
            trigger: Trigger::SyncNow,
            parent: None,
            over_all: false,
            device: ("d-switch", "Switch"),
            platform: "console:switch",
            comment: "",
            game_version: version,
            origin: None,
        },
    )
    .unwrap();
    sync::upload_pending(c, &meta).unwrap();
    sync::drop_pending(&meta).unwrap();
    let from_switch = meta.id.clone();

    // The PC version: farms in StardewValley/Saves (one of them the same
    // farm, saved by 1.6.9), settings next to them.
    let appdata = tmp.path().join("Roaming");
    let pc_saves = appdata.join("StardewValley").join("Saves");
    write(&pc_saves.join("Farm_111").join("Farm_111"), b"<SaveGame>old pc farm</SaveGame>");
    write(&pc_saves.join("Farm_111").join("SaveGameInfo"), &info("1.6.9", "PC"));
    write(&pc_saves.join("Farm_222").join("Farm_222"), b"<SaveGame>second farm</SaveGame>");
    write(&pc_saves.join("Farm_222").join("SaveGameInfo"), &info("1.6.9", "PC2"));
    write(&appdata.join("StardewValley").join("default_options"), b"pc settings");
    let pc = add(GameEntry {
        key: "stardew".into(),
        name: "Stardew Valley".into(),
        saves: vec!["{data}/StardewValley/**".into()],
        roots: BTreeMap::from([("data".to_string(), appdata.to_string_lossy().into_owned())]),
        auto_sync: true,
        ..Default::default()
    });
    assert!(matches!(sync::backup_now(&ctx, &pc, "").unwrap(), Outcome::Uploaded { .. }));
    assert_eq!(latest(&ctx, "stardew").game_version.as_deref(), Some("1.6.9"), "each save version records its game version");

    // One game in the library, though on two platforms.
    let cfg = config::load().unwrap();
    let listed = savelib::refresh(&cfg, ctx.cloud().unwrap()).unwrap();
    let fam = savelib::index(&cfg, &listed.games);
    assert_eq!(fam[&switch_key], fam["stardew"]);
    let switch_summary = ctx.cloud().unwrap().list_snapshots(&switch_key).unwrap().remove(0);
    assert_eq!(switch_summary.game_version.as_deref(), Some("1.6.15"));

    // Switch → PC: the farm folder into StardewValley/Saves, the Switch's
    // settings left out, and a warning that 1.6.15 is newer than 1.6.9.
    let ctx = Ctx::load().unwrap();
    let plan = transfer::plan(&ctx, &switch_key, &from_switch, "stardew", None).unwrap();
    // The game's name in the language the app shows.
    assert_eq!(plan.format_rule.as_deref(), Some(ludonimbus_core::crosssaves::by_id("stardew-valley").unwrap().display_name()));
    assert!(!plan.cross_platform && !plan.same && !plan.needs_folder);
    assert_eq!(PathBuf::from(plan.base.clone().unwrap()), pc_saves);
    let mut to: Vec<(String, PathBuf)> = plan.items.iter().map(|i| (i.from.clone(), PathBuf::from(&i.to))).collect();
    to.sort();
    assert_eq!(
        to,
        vec![
            ("Farm_111/Farm_111".to_string(), pc_saves.join("Farm_111").join("Farm_111")),
            ("Farm_111/SaveGameInfo".to_string(), pc_saves.join("Farm_111").join("SaveGameInfo")),
        ]
    );
    assert_eq!(plan.matched, 2);
    assert_eq!(plan.skipped, vec![nx("startup_preferences")]);
    assert_eq!((plan.source_version.as_deref(), plan.target_version.as_deref(), plan.too_new), (Some("1.6.15"), Some("1.6.9"), true));
    assert!(matches!(transfer::apply(&ctx, &switch_key, &from_switch, "stardew", None).unwrap(), Outcome::Uploaded { .. }));
    assert_eq!(fs::read(pc_saves.join("Farm_111").join("Farm_111")).unwrap(), b"<SaveGame>switch farm</SaveGame>");
    assert_eq!(fs::read(pc_saves.join("Farm_222").join("Farm_222")).unwrap(), b"<SaveGame>second farm</SaveGame>", "other farms stay");
    assert_eq!(fs::read(appdata.join("StardewValley").join("default_options")).unwrap(), b"pc settings");
    assert_eq!(latest(&ctx, "stardew").game_version.as_deref(), Some("1.6.15"));

    // PC → the Switch itself, which is only in the cloud: the farms go on top
    // of its newest version (its settings file stays), uploaded as its new
    // newest version for the Switch client to download.
    let from_pc = latest(&ctx, "stardew").id;
    let plan = transfer::plan(&ctx, "stardew", &from_pc, &switch_key, None).unwrap();
    assert!(plan.cloud_target && !plan.cross_platform && !plan.needs_folder);
    assert_eq!(plan.base.as_deref(), Some(format!("nxsave-{TITLE}").as_str()));
    assert_eq!((plan.items.len(), plan.matched, plan.skipped.len()), (4, 2, 1));
    assert!(plan.items.iter().any(|i| i.from == "Farm_222/Farm_222" && i.to == nx("Farm_222/Farm_222") && !i.replaces));
    assert_eq!((plan.source_version.as_deref(), plan.target_version.as_deref(), plan.too_new), (Some("1.6.15"), Some("1.6.15"), false));
    let Outcome::Uploaded { id } = transfer::apply(&ctx, "stardew", &from_pc, &switch_key, None).unwrap() else { panic!("uploaded") };
    let to_switch = id.clone();
    let newest = latest(&ctx, &switch_key);
    assert_eq!((newest.id.as_str(), newest.trigger, newest.parent.as_deref()), (id.as_str(), Trigger::Restore, Some(from_switch.as_str())));
    assert_eq!(newest.game_version.as_deref(), Some("1.6.15"));
    let meta = ctx.cloud().unwrap().get_meta(&switch_key, &id).unwrap();
    let mut paths: Vec<String> = meta.files.iter().map(|f| f.path.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![nx("Farm_111/Farm_111"), nx("Farm_111/SaveGameInfo"), nx("Farm_222/Farm_222"), nx("Farm_222/SaveGameInfo"), nx("startup_preferences")]
    );

    // PC → a Switch version on this device: both farms into its save data;
    // the PC's settings stay out. Its own save data has the same farm.
    let nx_dir = tmp.path().join("ryujinx").join("bis").join("user").join("save").join("0000000000000003").join("0");
    write(&nx_dir.join("Farm_111").join("SaveGameInfo"), &info("1.6.15", "Switch"));
    let on_switch = add(GameEntry {
        key: "stardew-switch".into(),
        name: "星露谷物语".into(),
        saves: vec![format!("{{nxsave-{TITLE}}}/**")],
        roots: BTreeMap::from([(format!("nxsave-{TITLE}"), nx_dir.to_string_lossy().into_owned())]),
        auto_sync: true,
        ..Default::default()
    });
    let fam = savelib::index(&config::load().unwrap(), &savelib::cached(&config::load().unwrap()).games);
    assert_eq!(fam["stardew-switch"], fam["stardew"]);
    let ctx = Ctx::load().unwrap();
    let plan = transfer::plan(&ctx, "stardew", &from_pc, &on_switch.key, None).unwrap();
    assert_eq!(PathBuf::from(plan.base.clone().unwrap()), nx_dir);
    assert_eq!((plan.items.len(), plan.matched), (4, 1));
    assert_eq!(plan.skipped.len(), 1, "default_options: {:?}", plan.skipped);
    assert!(!plan.too_new, "1.6.15 onto 1.6.15");
    transfer::apply(&ctx, "stardew", &from_pc, &on_switch.key, None).unwrap();
    assert_eq!(fs::read(nx_dir.join("Farm_222").join("Farm_222")).unwrap(), b"<SaveGame>second farm</SaveGame>");
    assert!(!nx_dir.join("default_options").exists());
    // An emulator's version of a Switch title keeps the files the Switch
    // keeps: it continues the Switch's history instead of starting one.
    assert_eq!(config::load().unwrap().game("stardew-switch").unwrap().record.as_deref(), Some(switch_key.as_str()));
    assert_eq!(latest(&ctx, "stardew-switch").id, latest(&ctx, &switch_key).id);
    assert_eq!(latest(&ctx, &switch_key).parent.as_deref(), Some(to_switch.as_str()));

    // A Switch game only in the cloud that no rule covers can't be a target:
    // its saves aren't the PC's.
    ctx.cloud()
        .unwrap()
        .put_game(&CloudGame {
            key: "nx-0100770008dd8000".into(),
            name: "Monster Hunter".into(),
            adapter: None,
            saves: vec!["{nxsave-0100770008dd8000}/**".into()],
            exclude: vec![],
            wait_for: vec![],
            roots: Default::default(),
            ludusavi_title: None,
            source: None,
            ids: vec!["nx:0100770008DD8000".into()],
            game: None,
        })
        .unwrap();
    assert!(transfer::plan(&ctx, "stardew", &from_pc, "nx-0100770008dd8000", None).is_err());
}
