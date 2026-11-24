//! Achievements without an icon of their own show the game's icon: Steam's
//! grid icon, the icon chosen in Ludonimbus, the Steam shortcut's icon, in
//! that order. On a fake Steam folder; one test, because the data directory
//! is chosen through an environment variable.

use ludonimbus_core::achievements::{self, Achievement, Schema, SourceKind};
use ludonimbus_core::config::{self, GameEntry, SteamLink};
use ludonimbus_core::{localart, steam};
use std::fs;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

#[test]
fn game_icon_for_achievements_without_one() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let root = tmp.path().join("Steam");
    fs::create_dir_all(root.join("userdata/123/config")).unwrap();
    let image = |name: &str| {
        let p = tmp.path().join(name);
        fs::write(&p, PNG).unwrap();
        p
    };

    let shortcut_icon = image("shortcut.png");
    let appid = steam::add_shortcut(
        &root,
        "123",
        &steam::NewShortcut {
            name: "Demo".into(),
            exe: r"C:\Tools\ludonimbus-run.exe".into(),
            start_dir: String::new(),
            launch_options: "--game demo".into(),
            icon: shortcut_icon.display().to_string(),
        },
    )
    .unwrap();
    let game = GameEntry {
        key: "demo".into(),
        name: "Demo".into(),
        steam: Some(SteamLink { account: "123".into(), appid, enabled: true, ..Default::default() }),
        ..Default::default()
    };
    config::update(|c| {
        c.steam.root = Some(root.clone());
        c.games.push(game.clone());
        Ok(())
    })
    .unwrap();
    let icon = || achievements::game_icon_path("demo");

    // Only the shortcut has an icon.
    assert_eq!(icon(), Some(shortcut_icon.clone()));

    // An icon chosen in Ludonimbus comes before it.
    let chosen = localart::set("demo", "icon", &image("chosen.png")).unwrap();
    assert_eq!(icon(), Some(chosen.clone()));

    // Steam's grid icon comes first of all.
    let grid = steam::write_art(&root, "123", appid, "icon", &image("grid.png")).unwrap();
    assert_eq!(icon(), Some(grid.clone()));

    // Too large to send as a data URL: the next one is used.
    fs::write(&grid, vec![0u8; 3 * 1024 * 1024]).unwrap();
    assert_eq!(icon(), Some(chosen));

    // The view says so when achievements have no icon of their own.
    achievements::update_db("demo", |db| {
        db.schema = Some(Schema {
            source: SourceKind::Rules,
            source_id: "demo".into(),
            title: None,
            achievements: vec![Achievement {
                id: "first".into(),
                name: "First".into(),
                description: String::new(),
                icon: None,
                hidden: false,
                points: None,
                grade: None,
            }],
        });
    })
    .unwrap();
    let v = achievements::view("demo");
    assert!(!v.items[0].has_icon && v.items[0].icon_url.is_none());
    assert!(v.game_icon);
    assert!(achievements::icon_data_url(&icon().unwrap()).unwrap().starts_with("data:image/png;base64,"));

    // Every achievement has an icon of its own: no game icon needed.
    let own = image("own.png");
    achievements::update_db("demo", |db| db.schema.as_mut().unwrap().achievements[0].icon = Some(own.display().to_string())).unwrap();
    assert!(!achievements::view("demo").game_icon);

    // The Ludonimbus wrapper is never taken for the game's program.
    localart::clear("demo", "icon");
    fs::remove_file(&shortcut_icon).unwrap();
    steam::set_shortcut_icon(&root, "123", appid, std::path::Path::new(r"C:\Tools\ludonimbus-run.exe")).unwrap();
    assert_eq!(icon(), None);

    // Other image types get their own type in the data URL.
    let webp = tmp.path().join("i.webp");
    fs::write(&webp, b"RIFF\0\0\0\0WEBPVP8 ").unwrap();
    assert!(achievements::icon_data_url(&webp).unwrap().starts_with("data:image/webp;"));
    let ico = tmp.path().join("i.ico");
    fs::write(&ico, [0u8, 0, 1, 0, 1, 0]).unwrap();
    assert!(achievements::icon_data_url(&ico).unwrap().starts_with("data:image/x-icon;"));
}
