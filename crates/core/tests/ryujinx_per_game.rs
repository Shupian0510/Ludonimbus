//! Ryujinx per game, end to end: a shortcut that starts a game syncs only
//! that game's save, found through the save index of the PC it runs on; on
//! a PC without that save (another profile, other save numbers), restoring
//! creates it the way Ryujinx does and fills both copies of the save. The
//! game is also found in Ryujinx's data folder without a shortcut.

use ludonimbus_core::config::{self, AppConfig};
use ludonimbus_core::discover;
use ludonimbus_core::games::{self, FoundChoice, NewGame};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::{steam, switch};
use std::fs;
use std::path::{Path, PathBuf};

const MHGU: u64 = 0x0100_7700_08DD_8000;

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn entry(title: u64, user: (u64, u64), kind: u8, id: u64) -> Vec<u8> {
    let mut b = b"IMEN\x40\0\0\0\x40\0\0\0".to_vec();
    let mut key = [0u8; 0x40];
    key[0..8].copy_from_slice(&title.to_le_bytes());
    key[8..16].copy_from_slice(&user.0.to_le_bytes());
    key[16..24].copy_from_slice(&user.1.to_le_bytes());
    key[0x20] = kind;
    let mut val = [0u8; 0x40];
    val[0..8].copy_from_slice(&id.to_le_bytes());
    val[0x18] = 1;
    b.extend_from_slice(&key);
    b.extend_from_slice(&val);
    b
}

/// A portable Ryujinx with a save index (`saves`: title, user, save number).
fn ryujinx(dir: &Path, last_opened: &str, saves: &[(u64, (u64, u64), u64)]) -> PathBuf {
    let exe = dir.join("Ryujinx.exe");
    write(&exe, b"exe");
    let emu = dir.join("portable");
    let mut idx = b"IMKV\0\0\0\0".to_vec();
    idx.extend_from_slice(&(saves.len() as u32).to_le_bytes());
    for &(t, u, id) in saves {
        idx.extend(entry(t, u, 1, id));
        write(&emu.join(format!(r"bis\user\save\{id:016x}\ExtraData0")), &[0u8; 0x200]);
        fs::create_dir_all(emu.join(format!(r"bis\user\save\{id:016x}\0"))).unwrap();
    }
    let last = saves.iter().map(|s| s.2).max().unwrap_or(0);
    for d in ["0", "1"] {
        write(&emu.join(format!(r"bis\system\save\8000000000000000\{d}\imkvdb.arc")), &idx);
        write(&emu.join(format!(r"bis\system\save\8000000000000000\{d}\lastPublishedId")), &last.to_le_bytes());
    }
    write(&emu.join(r"system\Profiles.json"), format!(r#"{{"profiles":[],"last_opened":"{last_opened}"}}"#).as_bytes());
    exe
}

fn logical(key: &str) -> Vec<String> {
    let cfg = config::load().unwrap();
    let (files, _) = snapshot::scan(cfg.game(key).unwrap()).unwrap();
    let mut out: Vec<String> = files.into_iter().map(|f| f.logical).collect();
    out.sort();
    out
}

#[test]
fn ryujinx_per_game() {
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

    // --- PC A: the default profile, the game is save 1, another game save 2 --
    let default_user = "00000000000000010000000000000000";
    let exe_a = ryujinx(&tmp.path().join("A"), default_user, &[(MHGU, (1, 0), 1), (0x0100_7EF0_0011_E000, (1, 0), 2)]);
    let emu_a = exe_a.parent().unwrap().join("portable");
    write(&emu_a.join(r"bis\user\save\0000000000000001\0\save.bin"), b"hunter rank 50");
    write(&emu_a.join(r"bis\user\save\0000000000000001\0\sub\opts.bin"), b"options");
    write(&emu_a.join(r"bis\user\save\0000000000000001\1\save.bin"), b"working copy");
    write(&emu_a.join(r"bis\user\save\0000000000000002\0\zelda.bin"), b"other game");
    write(&emu_a.join(r"games\0100770008dd8000\gui\metadata.json"), br#"{"title":"MONSTER HUNTER GENERATIONS ULTIMATE","favorite":false}"#);
    let roms = tmp.path().join("ROMs");
    let nsp = roms.join("MHGU [0100770008dd8000][v0].nsp");
    write(&nsp, b"PFS0");
    write(&emu_a.join("Config.json"), format!(r#"{{"version":70,"game_dirs":[{}]}}"#, serde_json::to_string(&roms).unwrap()).as_bytes());

    let appid = steam::add_shortcut(
        &root,
        "42",
        &steam::NewShortcut {
            name: String::new(),
            exe: exe_a.to_string_lossy().into_owned(),
            start_dir: exe_a.parent().unwrap().to_string_lossy().into_owned(),
            launch_options: format!("\"{}\"", nsp.display()),
            icon: String::new(),
        },
    )
    .unwrap();
    let g = games::add_from_shortcut("42", appid, NewGame { name: None, adapter: None, saves: vec![], key: None, ludusavi_title: None }).unwrap();
    assert_eq!(g.name, "MONSTER HUNTER GENERATIONS ULTIMATE", "named as Ryujinx shows it");
    assert_eq!(g.adapter.as_deref(), Some("ryujinx"));
    assert_eq!(g.saves, vec!["{nxsave-0100770008dd8000}/**".to_string(), "{nxdevice-0100770008dd8000}/**".to_string()]);
    // Only the committed copy of this game's save.
    assert_eq!(logical(&g.key), vec!["nxsave-0100770008dd8000/save.bin".to_string(), "nxsave-0100770008dd8000/sub/opts.bin".to_string()]);

    // The same game in the data folder scan is already added; it knows the game file.
    let mut found = discover::emulator_games(std::slice::from_ref(&exe_a), false);
    discover::mark_added(&config::load().unwrap(), &mut found);
    let f = found.iter().find(|f| f.saves == g.saves).expect("found in the data folder");
    assert_eq!(f.added_key.as_deref(), Some(g.key.as_str()));
    assert_eq!(f.args, format!("\"{}\"", nsp.display()));
    let other = found.iter().find(|f| f.saves != g.saves).unwrap();
    assert!(other.name == "Ryujinx（01007EF00011E000）" || other.name == "Ryujinx (01007EF00011E000)", "no name known: {}", other.name);
    let err = games::add_found(f, FoundChoice::default()).unwrap_err().to_string();
    assert!(err.contains("MONSTER HUNTER"), "{err}");

    // Snapshot on PC A.
    let local = snapshot::local_state(&g).unwrap();
    let (meta, zip) = snapshot::build(&g, &local, Trigger::Manual, None, ("a", "PC A"), "").unwrap();

    // --- PC B: another profile is current and has no save of the game -------
    let me = "4b04a3eee61e4f808c8c15d3d0afb19d";
    let exe_b = ryujinx(&tmp.path().join("B"), me, &[(0x0100_0000_0001_0000 + 0x2000, (0x4b04a3eee61e4f80, 0x8c8c15d3d0afb19d), 7)]);
    let emu_b = exe_b.parent().unwrap().join("portable");
    let g = games::set_exe(&g.key, &exe_b).unwrap();
    assert!(logical(&g.key).is_empty(), "no save on PC B yet");
    snapshot::apply(&zip, &meta, &g).unwrap();
    let save = emu_b.join(r"bis\user\save\0000000000000008");
    assert_eq!(fs::read(save.join(r"0\save.bin")).unwrap(), b"hunter rank 50");
    assert_eq!(fs::read(save.join(r"1\sub\opts.bin")).unwrap(), b"options", "working copy filled too");
    assert_eq!(switch::find_save(&emu_b, switch::SaveKind::Account, MHGU), Some(save.join("0")));
    let extra = fs::read(save.join("ExtraData0")).unwrap();
    assert_eq!(&extra[0x08..0x18], &[0x80, 0x4f, 0x1e, 0xe6, 0xee, 0xa3, 0x04, 0x4b, 0x9d, 0xb1, 0xaf, 0xd0, 0xd3, 0x15, 0x8c, 0x8c]);
    let idx = fs::read(emu_b.join(r"bis\system\save\8000000000000000\0\imkvdb.arc")).unwrap();
    assert_eq!(switch::parse_index(&idx).unwrap().len(), 2);
    assert_eq!(logical(&g.key), vec!["nxsave-0100770008dd8000/save.bin".to_string(), "nxsave-0100770008dd8000/sub/opts.bin".to_string()]);
    // The old index was kept in Ludonimbus's local backups.
    assert!(tmp.path().join(r"home\local-backups\ryujinx-save-index").read_dir().unwrap().next().is_some());

    // Restoring again uses that save; nothing new in the index.
    write(&save.join(r"0\stray.bin"), b"left over");
    snapshot::apply(&zip, &meta, &g).unwrap();
    assert!(!save.join(r"0\stray.bin").exists());
    assert!(!save.join(r"1\stray.bin").exists());
    let idx = fs::read(emu_b.join(r"bis\system\save\8000000000000000\1\imkvdb.arc")).unwrap();
    assert_eq!(switch::parse_index(&idx).unwrap().len(), 2);

    // The whole emulator can't be added next to a game synced on its own.
    let plain = steam::add_shortcut(
        &root,
        "42",
        &steam::NewShortcut {
            name: "Ryujinx".into(),
            exe: exe_b.to_string_lossy().into_owned(),
            start_dir: String::new(),
            launch_options: String::new(),
            icon: String::new(),
        },
    )
    .unwrap();
    let err = games::add_from_shortcut("42", plain, NewGame { name: None, adapter: None, saves: vec![], key: None, ludusavi_title: None })
        .unwrap_err()
        .to_string();
    assert!(err.contains("单独同步") || err.contains("on their own"), "{err}");
}
