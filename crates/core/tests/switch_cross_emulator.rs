//! One Switch game, two emulators: a save made in Ryujinx on PC A restores
//! into Eden on PC B (the game linked from the cloud follows PC B's
//! emulator), and Eden's save goes back to Ryujinx. yuzu's size file stays
//! out of the snapshots. A game added on PC C without linking takes over
//! the cloud's key when it was added in the same emulator (another
//! emulator's is another version), and the same game can't be added twice
//! on one PC.

use ludonimbus_core::cloud::CloudGame;
use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::discover;
use ludonimbus_core::games::{self, NewGame};
use ludonimbus_core::snapshot::{self, SnapshotMeta, Trigger};
use ludonimbus_core::{steam, switch};
use std::fs;
use std::path::{Path, PathBuf};

const MHGU: u64 = 0x0100_7700_08DD_8000;
/// The Eden profile's UUID bytes 01..10, as its save folder writes it.
const EDEN_USER: &str = "100F0E0D0C0B0A090807060504030201";

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// One device: its own Ludonimbus data folder and Steam.
fn device(tmp: &Path, name: &str) -> PathBuf {
    std::env::set_var("LUDONIMBUS_HOME", tmp.join(format!("home-{name}")));
    let root = tmp.join(format!("Steam-{name}"));
    if !root.exists() {
        fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();
        let mut cfg = AppConfig::default();
        cfg.steam.root = Some(root.clone());
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
    }
    root
}

fn shortcut(root: &Path, exe: &Path, game_file: &Path) -> u32 {
    steam::add_shortcut(
        root,
        "42",
        &steam::NewShortcut {
            name: String::new(),
            exe: exe.to_string_lossy().into_owned(),
            start_dir: exe.parent().unwrap().to_string_lossy().into_owned(),
            launch_options: format!("\"{}\"", game_file.display()),
            icon: String::new(),
        },
    )
    .unwrap()
}

/// A portable Ryujinx whose default profile has the game as save 1.
fn ryujinx(dir: &Path) -> PathBuf {
    let exe = dir.join("Ryujinx.exe");
    write(&exe, b"exe");
    let emu = dir.join("portable");
    let mut idx = b"IMKV\0\0\0\0\x01\0\0\0IMEN\x40\0\0\0\x40\0\0\0".to_vec();
    let mut key = [0u8; 0x40];
    key[0..8].copy_from_slice(&MHGU.to_le_bytes());
    key[8..16].copy_from_slice(&1u64.to_le_bytes());
    key[0x20] = 1;
    let mut val = [0u8; 0x40];
    val[0..8].copy_from_slice(&1u64.to_le_bytes());
    val[0x18] = 1;
    idx.extend_from_slice(&key);
    idx.extend_from_slice(&val);
    for d in ["0", "1"] {
        write(&emu.join(format!("bis/system/save/8000000000000000/{d}/imkvdb.arc")), &idx);
        write(&emu.join(format!("bis/system/save/8000000000000000/{d}/lastPublishedId")), &1u64.to_le_bytes());
    }
    write(&emu.join("bis/user/save/0000000000000001/ExtraData0"), &[0u8; 0x200]);
    write(&emu.join("bis/user/save/0000000000000001/0/save.bin"), b"hunter rank 50");
    write(&emu.join("bis/user/save/0000000000000001/0/sub/opts.bin"), b"options");
    write(&emu.join("system/Profiles.json"), br#"{"profiles":[],"last_opened":"00000000000000010000000000000000"}"#);
    write(&emu.join("games/0100770008dd8000/gui/metadata.json"), br#"{"title":"MONSTER HUNTER GENERATIONS ULTIMATE"}"#);
    exe
}

/// A portable Eden with one profile and `roms` as its game folder.
fn eden(dir: &Path, roms: &Path) -> PathBuf {
    let exe = dir.join("eden.exe");
    write(&exe, b"exe");
    let user = dir.join("user");
    let mut profiles = vec![0u8; 0x650];
    let uuid: Vec<u8> = (1..=16).collect();
    profiles[0x10..0x20].copy_from_slice(&uuid);
    profiles[0x20..0x30].copy_from_slice(&uuid);
    write(&user.join("nand/system/save/8000000000000010/su/avators/profiles.dat"), &profiles);
    let ini = format!("[UI]\nPaths\\gamedirs\\size=1\nPaths\\gamedirs\\1\\path={}\n\n[System]\ncurrent_user=0\n", roms.to_string_lossy().replace('\\', "/"));
    write(&user.join("config/qt-config.ini"), ini.as_bytes());
    exe
}

fn eden_save(exe: &Path) -> PathBuf {
    exe.parent().unwrap().join("user/nand/user/save/0000000000000000").join(EDEN_USER).join("0100770008DD8000")
}

fn logical(key: &str) -> Vec<String> {
    let cfg = config::load().unwrap();
    let (files, _) = snapshot::scan(cfg.game(key).unwrap()).unwrap();
    let mut out: Vec<String> = files.into_iter().map(|f| f.logical).collect();
    out.sort();
    out
}

fn snap(g: &GameEntry, device: &str) -> (SnapshotMeta, PathBuf) {
    let local = snapshot::local_state(g).unwrap();
    snapshot::build(g, &local, Trigger::Manual, None, (device, device), "").unwrap()
}

fn plain(key: Option<String>, adapter: Option<String>, saves: Vec<String>) -> NewGame {
    NewGame { name: None, adapter, saves, key, ludusavi_title: None }
}

#[test]
fn switch_game_moves_between_ryujinx_and_eden() {
    let tmp = tempfile::tempdir().unwrap();
    let roms = tmp.path().join("ROMs");
    let nsp = roms.join("MHGU [0100770008dd8000][v0].nsp");
    write(&nsp, b"PFS0");
    let saves = vec!["{nxsave-0100770008dd8000}/**".to_string(), "{nxdevice-0100770008dd8000}/**".to_string()];

    // --- PC A: Ryujinx ---------------------------------------------------
    let root_a = device(tmp.path(), "a");
    let ryu = ryujinx(&tmp.path().join("A"));
    let a = games::add_from_shortcut("42", shortcut(&root_a, &ryu, &nsp), plain(None, None, vec![])).unwrap();
    assert_eq!(a.adapter.as_deref(), Some("ryujinx"));
    assert_eq!(a.saves, saves);
    let (meta, zip) = snap(&a, "a");
    let cloud = CloudGame::from(&a);

    // --- PC B: Eden, the game linked from the cloud ------------------------
    let root_b = device(tmp.path(), "b");
    let ed = eden(&tmp.path().join("B"), &roms);
    let b = games::add_from_cloud(&cloud, Some("42"), Some(shortcut(&root_b, &ed, &nsp)), None).unwrap();
    assert_eq!(b.key, a.key, "same cloud entry");
    assert_eq!(b.adapter.as_deref(), Some("eden"), "follows this PC's emulator");
    assert_eq!(b.saves, saves);
    assert!(logical(&b.key).is_empty(), "no save in Eden yet");
    snapshot::apply(&zip, &meta, &b).unwrap();
    let save = eden_save(&ed);
    assert_eq!(fs::read(save.join("save.bin")).unwrap(), b"hunter rank 50");
    assert_eq!(fs::read(save.join("sub/opts.bin")).unwrap(), b"options");
    assert_eq!(switch::yuzu_save(&ed.parent().unwrap().join("user"), switch::SaveKind::Account, MHGU), Some(save.clone()));

    // Eden plays on and keeps its size file next to the save.
    write(&save.join("save.bin"), b"hunter rank 99");
    write(&save.join(switch::YUZU_SIZE_FILE), &[0u8; 16]);
    assert_eq!(logical(&b.key), vec!["nxsave-0100770008dd8000/save.bin".to_string(), "nxsave-0100770008dd8000/sub/opts.bin".to_string()]);
    let (meta_b, zip_b) = snap(&b, "b");

    // Restoring on Eden again keeps the size file (it isn't part of the save).
    snapshot::apply(&zip_b, &meta_b, &b).unwrap();
    assert!(save.join(switch::YUZU_SIZE_FILE).exists());

    // The game is found in Eden's data folder, already added, with its game file.
    let mut found = discover::emulator_games(std::slice::from_ref(&ed), false);
    discover::mark_added(&config::load().unwrap(), &mut found);
    let f = found.iter().find(|f| f.saves == saves).expect("found in Eden's save folder");
    assert_eq!(f.added_key.as_deref(), Some(b.key.as_str()));
    assert_eq!(f.args, format!("\"{}\"", nsp.display()));

    // --- Back on PC A: Eden's save restores into Ryujinx --------------------
    device(tmp.path(), "a");
    let a = config::load().unwrap().game(&a.key).unwrap().clone();
    snapshot::apply(&zip_b, &meta_b, &a).unwrap();
    let ryu_save = ryu.parent().unwrap().join("portable/bis/user/save/0000000000000001");
    assert_eq!(fs::read(ryu_save.join("0/save.bin")).unwrap(), b"hunter rank 99");
    assert_eq!(fs::read(ryu_save.join("1/save.bin")).unwrap(), b"hunter rank 99", "working copy too");
    assert!(!ryu_save.join("0").join(switch::YUZU_SIZE_FILE).exists());

    // The same game in Eden on PC A would sync the same saves twice.
    let ed_a = eden(&tmp.path().join("A-eden"), &roms);
    let err = games::add_from_shortcut("42", shortcut(&root_a, &ed_a, &nsp), plain(None, None, vec![])).unwrap_err().to_string();
    assert!(err.contains("MONSTER HUNTER"), "{err}");
    let mut found = discover::emulator_games(std::slice::from_ref(&ed_a), false);
    write(&eden_save(&ed_a).join("save.bin"), b"x");
    found.extend(discover::emulator_games(std::slice::from_ref(&ed_a), false));
    discover::mark_added(&config::load().unwrap(), &mut found);
    assert!(found.iter().filter(|f| f.saves == saves).all(|f| f.added_key.as_deref() == Some(a.key.as_str())));
    // Pointing the game at Eden instead (the game moved emulators) switches the rule.
    let moved = games::set_exe(&a.key, &ed_a).unwrap();
    assert_eq!(moved.adapter.as_deref(), Some("eden"));
    assert_eq!(logical(&a.key), vec!["nxsave-0100770008dd8000/save.bin".to_string()]);

    // --- PC C: Eden, the game added on its own, then linked by title id ----
    let root_c = device(tmp.path(), "c");
    let ed_c = eden(&tmp.path().join("C"), &roms);
    let c = games::add_from_shortcut("42", shortcut(&root_c, &ed_c, &nsp), plain(None, None, vec![])).unwrap();
    assert_ne!(c.key, cloud.key);
    // Ryujinx's is another version of the game: Eden keeps its own.
    assert_eq!(games::adopt_cloud_key(&c.key, std::slice::from_ref(&cloud)).unwrap(), None);
    let cloud = CloudGame { adapter: Some("eden".into()), ..cloud };
    let other = CloudGame { key: "zelda".into(), saves: vec!["{nxsave-0100000000010000}/**".into()], ..cloud.clone() };
    assert_eq!(games::adopt_cloud_key(&c.key, &[other.clone(), cloud.clone()]).unwrap(), Some(cloud.key.clone()));
    let cfg = config::load().unwrap();
    assert!(cfg.game(&c.key).is_none());
    assert_eq!(cfg.game(&cloud.key).unwrap().adapter.as_deref(), Some("eden"));
    // Nothing to take over once it has the key, or for a game the cloud doesn't have.
    assert_eq!(games::adopt_cloud_key(&cloud.key, std::slice::from_ref(&cloud)).unwrap(), None);
    assert_eq!(games::adopt_cloud_key(&cloud.key, &[other]).unwrap(), None);
}
