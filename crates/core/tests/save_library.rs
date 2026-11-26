//! The save library across devices and platforms (docs/plan-phase5.md §5):
//! PC A backs up Celeste (Steam version) with a cover; PC B sees it as a
//! game only the cloud has, with the same cover, and recognises its own
//! GOG and Xbox versions as the same game. A's save version is applied to
//! B's GOG version (files placed by name) and into B's Xbox containers
//! (save slots replaced the way the Xbox app writes them), and the Xbox
//! version's saves are read back out into the GOG version.

use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome};
use ludonimbus_core::{artsync, localart, savelib, transfer, wgs};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CELESTE_XBOX: &str = "MattMakesGamesInc.Celeste_79daxvg0dq3v6";
const XBOX_USER: &str = "000901F2A1B2C3D4_000000000000000000000000646E3F8B";

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// One device: its own data folder, connected to the shared cloud folder.
fn device(tmp: &Path, name: &str, cloud: &Path) {
    std::env::set_var("LUDONIMBUS_HOME", tmp.join(format!("home-{name}")));
    let mut cfg = AppConfig::default();
    cfg.device.id = format!("d-{name}");
    cfg.device.name = format!("PC-{name}");
    cfg.cloud = CloudConfig::LocalFolder { path: cloud.to_path_buf() };
    config::update(|c| {
        if c.device.id != cfg.device.id {
            *c = cfg.clone();
        }
        Ok(())
    })
    .unwrap();
}

fn add(key: &str, name: &str, saves: &str, root: &Path) -> GameEntry {
    let g = GameEntry {
        key: key.into(),
        name: name.into(),
        saves: vec![saves.into()],
        roots: BTreeMap::from([("data".to_string(), root.to_string_lossy().into_owned())]),
        auto_sync: true,
        ..Default::default()
    };
    config::update(|c| {
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();
    g
}

/// An Xbox app save folder with one container per file, as Celeste's.
fn xbox_saves(dir: &Path, files: &[(&str, &[u8])]) {
    let mut entries = Vec::new();
    for (i, (name, data)) in files.iter().enumerate() {
        let guid = wgs::new_guid();
        let blob = wgs::new_guid();
        let folder = dir.join(wgs::hex(&guid));
        write(&folder.join(wgs::hex(&blob)), data);
        let m = wgs::Manifest { header: 4, files: vec![wgs::BlobRef { name: "Data".into(), cloud: blob, local: blob }] };
        write(&folder.join("container.1"), &wgs::write_manifest(&m).unwrap());
        entries.push(wgs::Entry {
            name: name.to_string(),
            name2: name.to_string(),
            etag: format!("\"0x8DEBCCC41BE96{i:02}\""),
            number: 1,
            state: 1,
            guid,
            filetime: 133_000_000_000_000_000,
            reserved: 0,
            size: data.len() as i64,
        });
    }
    let idx = wgs::Index {
        version: 14,
        display_name: String::new(),
        package: format!("{CELESTE_XBOX}!Game"),
        filetime: 133_000_000_000_000_000,
        flags: 3,
        root: String::new(),
        reserved: [0, 0, 0, 0x10, 0, 0, 0, 0],
        entries,
    };
    write(&dir.join(wgs::INDEX), &wgs::write_index(&idx));
}

fn xbox_file(dir: &Path, name: &str) -> Vec<u8> {
    let (_, list) = wgs::read_dir(dir).unwrap();
    list.into_iter().find(|c| c.name == name).unwrap().files.remove(0).1
}

fn latest(ctx: &Ctx, key: &str) -> String {
    let mut list = ctx.cloud().unwrap().list_snapshots(key).unwrap();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list.into_iter().find(|s| s.trigger.is_mainline()).unwrap().id
}

#[test]
fn save_library_across_devices_and_platforms() {
    let tmp = tempfile::tempdir().unwrap();
    let cloud = tmp.path().join("cloud");
    fs::create_dir_all(&cloud).unwrap();

    // PC A: the Steam version, with saves and a cover.
    device(tmp.path(), "a", &cloud);
    let a_saves = tmp.path().join("a").join("Celeste").join("Saves");
    write(&a_saves.join("0.celeste"), b"chapter 7");
    write(&a_saves.join("settings.celeste"), b"settings A");
    let steam = add("celeste", "Celeste", "{data}/**", &a_saves);
    let cover = tmp.path().join("cover.png");
    write(&cover, b"PNG cover");
    localart::set("celeste", "cover", &cover).unwrap();
    let ctx = Ctx::load().unwrap();
    assert!(matches!(sync::backup_now(&ctx, &steam, "").unwrap(), Outcome::Uploaded { .. }));
    let from_a = latest(&ctx, "celeste");

    // PC B: Celeste is only in the cloud, with A's cover.
    device(tmp.path(), "b", &cloud);
    let cfg = config::load().unwrap();
    let ctx = Ctx::load().unwrap();
    let listed = savelib::refresh(&cfg, ctx.cloud().unwrap()).unwrap();
    assert!(listed.games.iter().any(|g| g.key == "celeste" && g.ids.contains(&"name:pc:celeste".to_string())));
    assert_eq!(savelib::cached(&cfg).games.len(), 1);
    assert!(artsync::pull(ctx.cloud().unwrap().files(), &cfg, "celeste", None).unwrap());
    assert_eq!(fs::read(localart::file("celeste", "cover").unwrap()).unwrap(), b"PNG cover");

    // B's GOG version keeps its saves one folder deeper, and has an old one.
    let gog_root = tmp.path().join("b").join("GOG").join("Celeste");
    write(&gog_root.join("Saves").join("0.celeste"), b"chapter 1");
    add("celeste-gog", "Celeste", "{data}/Saves/**", &gog_root);
    // B's Xbox version: containers in its wgs folder.
    let xbox_dir = tmp.path().join("b").join("Packages").join(CELESTE_XBOX).join("SystemAppData").join("wgs").join(XBOX_USER);
    xbox_saves(&xbox_dir, &[("0.celeste", b"xbox chapter 2"), ("settings.celeste", b"xbox settings")]);
    add("celeste-xbox", "Celeste", "{data}/**", &xbox_dir);

    let cfg = config::load().unwrap();
    let fam = savelib::index(&cfg, &listed.games);
    assert_eq!(fam["celeste-gog"], "celeste");
    assert_eq!(fam["celeste-xbox"], "celeste");

    // A's save → B's GOG version: files placed next to the one it has.
    let ctx = Ctx::load().unwrap();
    let plan = transfer::plan(&ctx, "celeste", &from_a, "celeste-gog", None).unwrap();
    assert!(!plan.same && !plan.from_xbox && !plan.to_xbox);
    assert_eq!((plan.items.len(), plan.matched), (2, 1));
    assert_eq!(PathBuf::from(plan.base.clone().unwrap()), gog_root.join("Saves"));
    assert!(matches!(transfer::apply(&ctx, "celeste", &from_a, "celeste-gog", None).unwrap(), Outcome::Uploaded { .. }));
    assert_eq!(fs::read(gog_root.join("Saves").join("0.celeste")).unwrap(), b"chapter 7");
    assert_eq!(fs::read(gog_root.join("Saves").join("settings.celeste")).unwrap(), b"settings A");
    let kept = ctx.cloud().unwrap().list_snapshots("celeste-gog").unwrap();
    assert!(kept.iter().any(|s| s.trigger == Trigger::PreRestore));
    assert!(kept.iter().any(|s| s.trigger == Trigger::Restore));

    // A's save → B's Xbox version: the two save slots are replaced.
    let plan = transfer::plan(&ctx, "celeste", &from_a, "celeste-xbox", None).unwrap();
    assert!(plan.to_xbox);
    assert_eq!((plan.matched, plan.unmatched.len()), (2, 0));
    transfer::apply(&ctx, "celeste", &from_a, "celeste-xbox", None).unwrap();
    assert_eq!(xbox_file(&xbox_dir, "0.celeste"), b"chapter 7");
    assert_eq!(xbox_file(&xbox_dir, "settings.celeste"), b"settings A");
    let idx = wgs::parse_index(&fs::read(xbox_dir.join(wgs::INDEX)).unwrap()).unwrap();
    assert!(idx.entries.iter().all(|e| e.number == 2 && e.state == wgs::STATE_MODIFIED));
    assert_eq!(idx.flags & wgs::FLAG_UPLOADED, 0);

    // The Xbox version's saves, read out of the containers, → the GOG version.
    let from_xbox = latest(&ctx, "celeste-xbox");
    write(&gog_root.join("Saves").join("0.celeste"), b"chapter 1 again");
    let plan = transfer::plan(&ctx, "celeste-xbox", &from_xbox, "celeste-gog", None).unwrap();
    assert!(plan.from_xbox);
    assert_eq!(plan.xbox_rule.as_deref(), Some("Celeste"));
    assert_eq!(plan.matched, 2);
    transfer::apply(&ctx, "celeste-xbox", &from_xbox, "celeste-gog", None).unwrap();
    assert_eq!(fs::read(gog_root.join("Saves").join("0.celeste")).unwrap(), b"chapter 7");

    // Nothing newer elsewhere right after applying.
    let gog = config::load().unwrap().game("celeste-gog").unwrap().clone();
    let ctx = Ctx::load().unwrap();
    assert!(savelib::newer_version(&ctx, &gog).unwrap().is_none());
    // PC A plays on in the Steam version; PC B is offered that save for its
    // GOG version, until it waves that one off.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    device(tmp.path(), "a", &cloud);
    write(&a_saves.join("0.celeste"), b"chapter 8");
    let ctx = Ctx::load().unwrap();
    sync::backup_now(&ctx, &steam, "").unwrap();
    device(tmp.path(), "b", &cloud);
    let ctx = Ctx::load().unwrap();
    let newer = savelib::newer_version(&ctx, &gog).unwrap().expect("A's newer save is offered");
    assert_eq!((newer.key.as_str(), newer.snapshot.device_name.as_str()), ("celeste", "PC-a"));
    savelib::dismiss_newer("celeste-gog", &newer.snapshot.id).unwrap();
    assert!(savelib::newer_version(&ctx, &gog).unwrap().is_none());

    // Kept apart by hand, it's no longer shown with the others.
    savelib::unlink(Some(ctx.cloud().unwrap()), "celeste-xbox").unwrap();
    let cfg = config::load().unwrap();
    let fam = savelib::index(&cfg, &savelib::cached(&cfg).games);
    assert_eq!(fam["celeste-xbox"], "celeste-xbox");
    assert_eq!(fam["celeste-gog"], "celeste");
}
