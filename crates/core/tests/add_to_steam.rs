//! A game found outside Steam: add it, give it artwork, add it to Steam (a
//! wrapped shortcut with the artwork), toggle auto-sync, remove it from Steam.

use ludonimbus_core::config::{self, AppConfig};
use ludonimbus_core::discover::Found;
use ludonimbus_core::games::{self, FoundChoice};
use ludonimbus_core::{localart, steam};
use std::fs;

#[test]
fn add_found_game_to_steam() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let runner = tmp.path().join("app").join("ludonimbus-run.exe");
    fs::create_dir_all(runner.parent().unwrap()).unwrap();
    fs::write(&runner, b"x").unwrap();
    std::env::set_var("LUDONIMBUS_RUNNER", &runner);
    let root = tmp.path().join("Steam");
    fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();
    let mut cfg = AppConfig::default();
    cfg.steam.root = Some(root.clone());
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();

    // An Epic-style game with a known save folder.
    let game_dir = tmp.path().join("Games").join("Celeste");
    fs::create_dir_all(game_dir.join("Saves")).unwrap();
    let exe = game_dir.join("Celeste.exe");
    fs::write(&exe, b"exe").unwrap();
    let found = Found {
        id: "epic:Salt".into(),
        name: "Celeste".into(),
        source: "epic".into(),
        exe: Some(exe.to_string_lossy().into_owned()),
        install: Some(game_dir.to_string_lossy().into_owned()),
        ..Default::default()
    };
    // Without any save rule it refuses.
    assert!(games::add_found(&found, FoundChoice::default()).is_err());
    let g = games::add_found(
        &found,
        FoundChoice {
            saves: vec!["{install}/Saves/**".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(g.key, "celeste");
    assert_eq!(g.exe.as_deref(), Some(exe.as_path()));
    assert!(g.steam.is_none());

    // Artwork chosen before it is in Steam.
    let img = tmp.path().join("cover.jpg");
    fs::write(&img, b"jpg").unwrap();
    games::set_art("celeste", "cover", &img).unwrap();
    games::set_art("celeste", "icon", &img).unwrap();
    assert!(localart::file("celeste", "cover").unwrap().ends_with("cover.jpg"));

    // Add to Steam: a shortcut that launches through the wrapper.
    let appid = games::add_to_steam("celeste", "42").unwrap();
    let sc = steam::find_shortcut(&root, "42", appid).unwrap().unwrap();
    assert_eq!(sc.name, "Celeste");
    assert_eq!(sc.wrapped_key().as_deref(), Some("celeste"));
    assert!(sc.exe.ends_with("ludonimbus-run.exe"));
    assert!(sc.icon.ends_with(&format!("{appid}_icon.jpg")), "{}", sc.icon);
    assert!(steam::art(&root, "42", appid).cover.unwrap().ends_with(format!("{appid}p.jpg")));
    let cfg = config::load().unwrap();
    let g = cfg.game("celeste").unwrap();
    let link = g.steam.as_ref().unwrap();
    assert!(link.enabled && link.created);
    assert_eq!(steam::health(&cfg, g), steam::Health::Ok);
    assert!(games::add_to_steam("celeste", "42").is_err(), "only once");

    // Turning auto-sync off points the shortcut at the game itself.
    games::steam_disable("celeste").unwrap();
    let sc = steam::find_shortcut(&root, "42", appid).unwrap().unwrap();
    assert!(sc.exe.ends_with("Celeste.exe"));
    games::steam_enable("celeste").unwrap();
    assert_eq!(
        steam::find_shortcut(&root, "42", appid).unwrap().unwrap().wrapped_key().as_deref(),
        Some("celeste")
    );

    // Removing it from Steam deletes the shortcut and its artwork.
    games::remove_from_steam("celeste").unwrap();
    assert!(steam::find_shortcut(&root, "42", appid).unwrap().is_none());
    assert!(steam::art(&root, "42", appid).cover.is_none());
    assert!(config::load().unwrap().game("celeste").unwrap().steam.is_none());

    // Removing the game deletes its artwork in Ludonimbus.
    games::remove("celeste").unwrap();
    assert!(localart::file("celeste", "cover").is_none());

    // A game without a program cannot be added to Steam.
    let g = games::add_custom("No Exe", None, vec!["{appdata}/NoExe/**".into()], None, None).unwrap();
    assert!(games::add_to_steam(&g.key, "42").unwrap_err().to_string().contains("游戏程序"));
}
