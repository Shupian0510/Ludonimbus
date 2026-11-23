//! Built-in rules end to end: emulator shortcuts get one game's saves (or the
//! whole emulator's, once, never both), games with saves in an emulator's
//! data folder are found without a shortcut, and a recomp whose exe sits
//! deeper in its release finds its saves from the release folder, without
//! programs or logs.

use ludonimbus_core::config::{self, AppConfig};
use ludonimbus_core::discover::{self, Found};
use ludonimbus_core::games::{self, FoundChoice, NewGame};
use ludonimbus_core::{adapters, snapshot, steam};
use std::fs;
use std::path::Path;

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn wii_disc(id: &str) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    b[0..6].copy_from_slice(id.as_bytes());
    b[0x18..0x1C].copy_from_slice(&0x5D1C_9EA3u32.to_be_bytes());
    b
}

fn new_game() -> NewGame {
    NewGame { name: None, adapter: None, saves: vec![], key: None, ludusavi_title: None }
}

fn logical(key: &str) -> Vec<String> {
    let cfg = config::load().unwrap();
    let (files, _) = snapshot::scan(cfg.game(key).unwrap()).unwrap();
    let mut out: Vec<String> = files.into_iter().map(|f| f.logical).collect();
    out.sort();
    out
}

#[test]
fn rules_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let root = tmp.path().join("Steam");
    fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();
    let mut cfg = AppConfig::default();
    cfg.steam.root = Some(root.clone());
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
    let shortcut = |name: &str, exe: &Path, args: &str| {
        steam::add_shortcut(
            &root,
            "42",
            &steam::NewShortcut {
                name: name.into(),
                exe: exe.to_string_lossy().into_owned(),
                start_dir: exe.parent().unwrap().to_string_lossy().into_owned(),
                launch_options: args.into(),
                icon: String::new(),
            },
        )
        .unwrap()
    };

    // --- Dolphin (portable): one Wii game per shortcut -----------------------
    let dolphin = tmp.path().join("Emu").join("Dolphin").join("Dolphin.exe");
    write(&dolphin, b"exe");
    let user = dolphin.parent().unwrap().join("User");
    write(&user.join(r"Wii\title\00010000\524d4745\data\GameData.bin"), b"galaxy");
    write(&user.join(r"Wii\title\00010000\534f5545\data\skyward.bin"), b"other game");
    let disc = tmp.path().join("ROMs").join("Super Mario Galaxy.iso");
    write(&disc, &wii_disc("RMGE01"));
    let galaxy = shortcut("Super Mario Galaxy", &dolphin, &format!("-b -e \"{}\"", disc.display()));
    let plain = shortcut("Dolphin", &dolphin, "");

    let cands = games::shortcut_candidates(&config::load().unwrap(), "42").unwrap();
    let c = cands.iter().find(|c| c.appid == galaxy).unwrap();
    assert_eq!(c.adapter.as_deref(), Some("dolphin"));
    assert_eq!(c.adapter_name.as_deref(), Some("Dolphin（RMGE01）"));
    assert_eq!(c.saves, vec!["{emu}/Wii/title/0001000?/524d4745/data/**".to_string()]);

    let g = games::add_from_shortcut("42", galaxy, new_game()).unwrap();
    assert_eq!(g.key, "super-mario-galaxy", "named after the game, not the emulator");
    assert_eq!(g.adapter.as_deref(), Some("dolphin"));
    assert_eq!(logical(&g.key), vec!["emu/Wii/title/00010000/524d4745/data/GameData.bin".to_string()]);

    // The emulator on its own would sync that game again: refused.
    let err = games::add_from_shortcut("42", plain, new_game()).unwrap_err().to_string();
    assert!(err.contains("Super Mario Galaxy") && err.contains("单独同步"), "{err}");

    // Its data folder has another game: found without a shortcut, with its
    // own saves; the game already added is marked.
    let cfg = config::load().unwrap();
    let mut emu_games = discover::emulator_games(std::slice::from_ref(&dolphin), false);
    discover::mark_added(&cfg, &mut emu_games);
    assert_eq!(emu_games.len(), 2, "{emu_games:?}");
    let added = emu_games.iter().find(|f| f.saves == g.saves).unwrap();
    assert_eq!(added.added_key.as_deref(), Some("super-mario-galaxy"));
    let skyward = emu_games.iter().find(|f| f.saves != g.saves).unwrap();
    assert_eq!((skyward.source.as_str(), skyward.adapter.as_deref()), ("emulator", Some("dolphin")));
    assert_eq!(skyward.adapter_name.as_deref(), Some("Dolphin（SOUE）"));
    assert_eq!(skyward.save_files, 1);
    let s = games::add_found(skyward, FoundChoice { name: "Skyward Sword".into(), ..Default::default() }).unwrap();
    assert_eq!((s.adapter.as_deref(), s.exe.as_deref()), (Some("dolphin"), Some(dolphin.as_path())));
    assert_eq!(logical(&s.key), vec!["emu/Wii/title/00010000/534f5545/data/skyward.bin".to_string()]);
    let err = games::add_found(skyward, FoundChoice::default()).unwrap_err().to_string();
    assert!(err.contains("Skyward Sword"), "{err}");

    // --- Ryujinx without a game it can identify: the whole emulator, once ----
    let ryujinx = tmp.path().join("Emu").join("Ryujinx").join("Ryujinx.exe");
    write(&ryujinx, b"exe");
    let portable = ryujinx.parent().unwrap().join("portable");
    write(&portable.join(r"bis\user\save\0000000000000001\0\save.dat"), b"zelda");
    write(&portable.join(r"bis\system\save\8000000000000000\0\imkvdb.arc"), b"index");
    write(&portable.join(r"bis\system\save\8000000000000000\.lock"), b"");
    write(&portable.join(r"system\Profiles.json"), b"{}");
    write(&portable.join(r"system\prod.keys"), b"secret");
    let botw = shortcut("Zelda BotW", &ryujinx, "\"D:\\Switch\\BotW.nsp\"");
    let mk8 = shortcut("Mario Kart 8", &ryujinx, "\"D:\\Switch\\MK8.nsp\"");
    let g = games::add_from_shortcut("42", botw, new_game()).unwrap();
    let files = logical(&g.key);
    assert_eq!(files.len(), 3, "{files:?}");
    assert!(!files.iter().any(|f| f.ends_with(".lock") || f.ends_with("prod.keys")), "{files:?}");
    let err = games::add_from_shortcut("42", mk8, new_game()).unwrap_err().to_string();
    assert!(err.contains("Zelda BotW"), "{err}");
    // A scan shows that shortcut as already synced by that entry.
    let mut found = vec![Found {
        id: "steam:42:mk8".into(),
        source: "steam".into(),
        adapter: Some("ryujinx".into()),
        steam_account: Some("42".into()),
        steam_appid: Some(mk8),
        ..Default::default()
    }];
    discover::mark_added(&config::load().unwrap(), &mut found);
    assert_eq!(found[0].added_key.as_deref(), Some(g.key.as_str()));

    // --- A recomp whose exe is in a subfolder of its release -----------------
    let rule = adapters::builtin()
        .iter()
        .find_map(|a| {
            let exe = a.matcher.as_ref()?.exe.iter().find(|e| e.contains('/') && !e.contains('*'))?.clone();
            let save = a.saves.iter().find(|s| {
                s.strip_prefix("{install}/")
                    .is_some_and(|rest| !rest.contains(['*', '?']) && !rest.rsplit('/').next().unwrap_or("").contains('.'))
            })?;
            a.skip_junk.then(|| (a, exe, save.strip_prefix("{install}/").unwrap().to_string()))
        })
        .expect("a generated rule with a nested exe and a save folder");
    let (adapter, rel_exe, save_dir) = rule;
    let release = tmp.path().join("Recomps").join("Release");
    let exe = release.join(rel_exe.replace('/', "\\"));
    write(&exe, b"exe");
    let saves = release.join(save_dir.replace('/', "\\"));
    write(&saves.join("slot1.sav"), b"progress");
    write(&saves.join("debug.log"), b"noise");
    write(&saves.join("crash.dmp"), b"noise");
    write(&saves.join("shadercache").join("a.bin"), b"noise");
    let f = Found {
        id: "folder:x".into(),
        name: "Some Recomp".into(),
        source: "folder".into(),
        exe: Some(exe.to_string_lossy().into_owned()),
        install: exe.parent().map(|p| p.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let g = games::add_found(&f, FoundChoice::default()).unwrap();
    assert_eq!(g.adapter.as_deref(), Some(adapter.key.as_str()));
    assert_eq!(g.install.as_deref(), Some(release.as_path()), "{{install}} is the release folder");
    assert_eq!(logical(&g.key), vec![format!("install/{save_dir}/slot1.sav")], "{}", adapter.key);
}
