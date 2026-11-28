//! Adding games: where the save finder suggests a game keeps its saves when
//! Ludusavi's database doesn't know it (engine conventions, Steam's cloud
//! folder, Wine prefixes; all on made-up folders), and the library never
//! taking Ludonimbus itself for a game.

use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::games;
use ludonimbus_core::savefind::hints::Places;
use ludonimbus_core::savefind::{self, Candidate, Look};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The tests that change the process's environment: one at a time.
static ENV: Mutex<()> = Mutex::new(());

fn mk(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

fn mkdir(p: &Path) {
    fs::create_dir_all(p).unwrap();
}

/// A Windows user home and a games folder in a temporary folder.
struct Machine {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    games: PathBuf,
}

impl Machine {
    fn new() -> Machine {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("Users").join("me");
        for d in ["Documents", "Saved Games", "AppData/Roaming", "AppData/Local", "AppData/LocalLow"] {
            mkdir(&home.join(d));
        }
        let games = tmp.path().join("Games");
        mkdir(&games);
        Machine { _tmp: tmp, home, games }
    }

    fn look(&self) -> Look {
        Look { places: Places::windows(&self.home), steam_userdata: None, prefixes_home: None, drives: Vec::new() }
    }

    fn suggest(&self, name: &str, exe: &Path) -> Vec<Candidate> {
        savefind::suggest_in(&self.look(), name, Some(exe), exe.parent())
    }
}

fn same(a: &str, b: &Path) -> bool {
    Path::new(a) == b
}

#[test]
fn unity_saves_from_app_info() {
    let m = Machine::new();
    let dir = m.games.join("Some Game");
    let exe = dir.join("SomeGame.exe");
    mk(&exe);
    fs::create_dir_all(dir.join("SomeGame_Data")).unwrap();
    fs::write(dir.join("SomeGame_Data").join("app.info"), "Studio X\nSome Game").unwrap();

    // Not played yet: where the game will write, without files.
    let list = m.suggest("Whatever it is called", &exe);
    let save = m.home.join("AppData").join("LocalLow").join("Studio X").join("Some Game");
    assert!(same(&list[0].path, &save), "{list:#?}");
    assert!(list[0].likely && list[0].files == 0);
    assert!(list[0].reason.contains("Studio X"));

    // Played: the same folder, with its files.
    mk(&save.join("save0.dat"));
    let list = m.suggest("Whatever it is called", &exe);
    assert!(same(&list[0].path, &save));
    assert_eq!(list[0].files, 1);
    assert!(list[0].pattern.ends_with("Studio X/Some Game/**"), "{}", list[0].pattern);
}

#[test]
fn unreal_saves_for_shipping_programs_and_launchers() {
    let m = Machine::new();
    let dir = m.games.join("Fancy");
    let shipping = dir.join("FancyGame").join("Binaries").join("Win64").join("FancyGame-Win64-Shipping.exe");
    mk(&shipping);
    mkdir(&dir.join("FancyGame").join("Content").join("Paks"));
    mkdir(&dir.join("Engine").join("Binaries"));
    let launcher = dir.join("Fancy.exe");
    mk(&launcher);
    let local = m.home.join("AppData").join("Local").join("FancyGame").join("Saved").join("SaveGames");

    // Not played yet: the project's folder under Local.
    let list = m.suggest("Fancy", &shipping);
    assert!(same(&list[0].path, &local), "{list:#?}");
    assert!(list[0].likely && list[0].files == 0);

    // Played, and picked through the launcher next to the project.
    mk(&local.join("slot1.sav"));
    let list = savefind::suggest_in(&m.look(), "Fancy", Some(&launcher), Some(&dir));
    assert!(same(&list[0].path, &local), "{list:#?}");
    assert_eq!(list[0].files, 1);

    // A portable release keeps them next to the game.
    let beside = dir.join("FancyGame").join("Saved").join("SaveGames");
    mk(&beside.join("slot2.sav"));
    let list = m.suggest("Fancy", &shipping);
    assert!(list.iter().any(|c| same(&c.path, &beside) && c.likely), "{list:#?}");

    // A project folder named otherwise ("Phoenix"): the shipping program's name.
    let other = m.games.join("Hogwarts Legacy");
    let exe = other.join("Phoenix").join("Binaries").join("Win64").join("HogwartsLegacy-Win64-Shipping.exe");
    mk(&exe);
    let saves = m.home.join("AppData").join("Local").join("HogwartsLegacy").join("Saved").join("SaveGames");
    mk(&saves.join("HL-00-00.sav"));
    let list = m.suggest("Hogwarts Legacy", &exe);
    assert!(same(&list[0].path, &saves), "{list:#?}");
}

#[test]
fn rpg_maker_godot_renpy_and_gamemaker() {
    let m = Machine::new();

    // RPG Maker VX Ace: the save files next to Game.exe, not the whole folder.
    let ace = m.games.join("Ace Quest");
    mk(&ace.join("Game.exe"));
    mk(&ace.join("Game.rgss3a"));
    mk(&ace.join("Save01.rvdata2"));
    mk(&ace.join("Graphics").join("a.png"));
    let list = m.suggest("Ace Quest", &ace.join("Game.exe"));
    assert!(same(&list[0].path, &ace) && list[0].likely, "{list:#?}");
    assert_eq!(list[0].pattern, "{install}/Save*.rvdata2");
    assert_eq!(list[0].files, 1);

    // RPG Maker MV: www/save.
    let mv = m.games.join("MV Tale");
    mk(&mv.join("Game.exe"));
    mk(&mv.join("www").join("js").join("rpg_core.js"));
    mk(&mv.join("www").join("save").join("file1.rpgsave"));
    let list = m.suggest("MV Tale", &mv.join("Game.exe"));
    assert_eq!(list[0].pattern, "{install}/www/save/**", "{list:#?}");

    // Godot: a .pck beside the program, the project's folder in app_userdata.
    let gd = m.games.join("Cozy Farm");
    mk(&gd.join("CozyFarm.exe"));
    mk(&gd.join("CozyFarm.pck"));
    let userdata = m.home.join("AppData").join("Roaming").join("Godot").join("app_userdata").join("Cozy Farm");
    mk(&userdata.join("save.json"));
    let list = m.suggest("Cozy Farm", &gd.join("CozyFarm.exe"));
    assert!(same(&list[0].path, &userdata) && list[0].likely, "{list:#?}");
    // The shared Godot folders themselves are never offered.
    assert!(!list.iter().any(|c| same(&c.path, userdata.parent().unwrap())));

    // Ren'Py: config.save_directory names the folder under RenPy.
    let rp = m.games.join("Visual Novel");
    mk(&rp.join("VisualNovel.exe"));
    mkdir(&rp.join("renpy"));
    mk(&rp.join("game").join("options.rpy"));
    fs::write(rp.join("game").join("options.rpy"), "define config.save_directory = \"VisualNovel-1600000000\"\n").unwrap();
    let saves = m.home.join("AppData").join("Roaming").join("RenPy").join("VisualNovel-1600000000");
    mk(&saves.join("1-1-LT1.save"));
    let list = m.suggest("Visual Novel", &rp.join("VisualNovel.exe"));
    assert!(same(&list[0].path, &saves), "{list:#?}");

    // GameMaker: data.win beside the program, saves under Local/<program>.
    let gm = m.games.join("Pixel Hero");
    mk(&gm.join("PixelHero.exe"));
    mk(&gm.join("data.win"));
    let saves = m.home.join("AppData").join("Local").join("PixelHero");
    mk(&saves.join("save.ini"));
    let list = m.suggest("Pixel Hero", &gm.join("PixelHero.exe"));
    assert!(same(&list[0].path, &saves) && list[0].likely, "{list:#?}");
}

#[test]
fn steam_cloud_folders_game_folders_and_similar_names() {
    let m = Machine::new();
    let dir = m.games.join("Lonely Road");
    let exe = dir.join("LonelyRoad.exe");
    mk(&exe);
    fs::write(dir.join("steam_appid.txt"), "4242\n").unwrap();
    mk(&dir.join("savegames").join("77").join("1.save"));
    mk(&dir.join("ShaderCache").join("save").join("a.bin"));
    let steam = m.home.parent().unwrap().join("Steam").join("userdata");
    mk(&steam.join("1001").join("4242").join("remote").join("profile.sav"));
    mk(&steam.join("1001").join("4242").join("remotecache.vdf"));
    mk(&m.home.join("Documents").join("My Games").join("Lonely Road").join("settings.ini"));
    let look = Look { steam_userdata: Some(steam.clone()), ..m.look() };
    let list = savefind::suggest_in(&look, "Lonely Road", Some(&exe), Some(&dir));
    let at = |p: &Path| list.iter().position(|c| same(&c.path, p)).unwrap_or_else(|| panic!("{p:?} not in {list:#?}"));
    let remote = at(&steam.join("1001").join("4242").join("remote"));
    let in_game = at(&dir.join("savegames"));
    let named = at(&m.home.join("Documents").join("My Games").join("Lonely Road"));
    assert!(remote < named && in_game < named);
    assert!(!list.iter().any(|c| c.path.contains("ShaderCache")));
    // Never a whole root.
    assert!(!list.iter().any(|c| same(&c.path, &m.home.join("Documents").join("My Games"))));
}

#[test]
fn wine_prefixes() {
    let m = Machine::new();
    let home = m.home.parent().unwrap().join("deck");
    // A Unity game installed inside a Heroic prefix: its LocalLow is the prefix's.
    let prefix = home.join("Games").join("Heroic").join("Prefixes").join("Tiny Isle");
    let user = prefix.join("drive_c").join("users").join("deck");
    mkdir(&prefix.join("drive_c").join("users").join("Public"));
    mkdir(&user.join("AppData"));
    let dir = prefix.join("drive_c").join("Games").join("Tiny Isle");
    let exe = dir.join("TinyIsle.exe");
    mk(&exe);
    fs::create_dir_all(dir.join("TinyIsle_Data")).unwrap();
    fs::write(dir.join("TinyIsle_Data").join("app.info"), "Isle Co\nTiny Isle").unwrap();
    let list = m.suggest("Tiny Isle", &exe);
    assert!(same(&list[0].path, &user.join("AppData").join("LocalLow").join("Isle Co").join("Tiny Isle")), "{list:#?}");

    // A game installed outside its prefix: the launcher's prefix named like it.
    let other = home.join("Games").join("Heroic").join("Prefixes").join("Celeste");
    let saves = other.join("drive_c").join("users").join("deck").join("AppData").join("Local").join("Celeste");
    mk(&saves.join("0.celeste"));
    let game = home.join("Games").join("Heroic").join("Celeste").join("Celeste.exe");
    mk(&game);
    let look = Look { prefixes_home: Some(home.clone()), ..m.look() };
    let list = savefind::suggest_in(&look, "Celeste", Some(&game), game.parent());
    assert!(list.iter().any(|c| same(&c.path, &saves)), "{list:#?}");
}

#[test]
fn ludonimbus_is_never_a_game() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    config::update(|c| {
        *c = AppConfig::default();
        Ok(())
    })
    .unwrap();
    let app = tmp.path().join("Ludonimbus").join("ludonimbus-app.exe");
    mk(&app);
    let err = games::add_custom("Ludonimbus", Some(app.clone()), vec!["{home}/x/**".into()], None, None).unwrap_err();
    assert!(err.to_string().contains("Ludonimbus"), "{err}");
    let game = tmp.path().join("Games").join("Hades").join("Hades.exe");
    mk(&game);
    let g = games::add_custom("Hades", Some(game.clone()), vec!["{home}/Saved Games/Hades/**".into()], None, None).unwrap();
    assert!(games::set_exe(&g.key, &app).is_err());

    // An entry made before this: kept, but known as Ludonimbus (hidden from the library).
    let old = GameEntry { key: "ludonimbus".into(), name: "Ludonimbus".into(), exe: Some(app), ..Default::default() };
    assert!(games::is_ludonimbus(&old));
    assert!(!games::is_ludonimbus(&g));
}

/// The bug behind "添加 does nothing": a game added by its title in
/// Ludusavi's database without a name typed got an empty name. The title
/// names it now.
#[test]
fn a_game_added_by_its_ludusavi_title_is_named_after_it() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    config::update(|c| {
        *c = AppConfig::default();
        Ok(())
    })
    .unwrap();
    let g = games::add_custom("  ", None, Vec::new(), Some("Hollow Knight".into()), None).unwrap();
    assert_eq!(g.name, "Hollow Knight");
    assert_eq!(g.key, "hollow-knight");
    assert_eq!(g.ludusavi_title.as_deref(), Some("Hollow Knight"));
    // Without a save location or a title there is still nothing to add.
    assert!(games::add_custom("", None, Vec::new(), None, None).is_err());
}
