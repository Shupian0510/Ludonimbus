//! Creating / removing non-Steam shortcuts, grid artwork and Steam libraries
//! on a fake Steam folder.

use ludonimbus_core::steam::{self, NewShortcut};
use std::fs;

#[test]
fn shortcuts_art_and_libraries() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let root = tmp.path().join("Steam");
    fs::create_dir_all(root.join("userdata").join("123").join("config")).unwrap();

    // The id Steam derives from the quoted exe and the name.
    assert_eq!(steam::shortcut_appid("\"C:\\Games\\demo.exe\"", "Demo"), 3193970896);

    let sc = NewShortcut {
        name: "示例游戏".into(),
        exe: r"C:\Tools\ludonimbus-run.exe".into(),
        start_dir: r"C:\Games\Demo".into(),
        launch_options: "--game demo".into(),
        icon: r"C:\Games\Demo\demo.exe".into(),
    };
    // No shortcuts.vdf yet: it is created.
    let a = steam::add_shortcut(&root, "123", &sc).unwrap();
    assert!(a & 0x8000_0000 != 0);
    assert_eq!(a, steam::shortcut_appid("\"C:\\Tools\\ludonimbus-run.exe\"", "示例游戏"));
    // Same exe and name again: a different id is chosen.
    let b = steam::add_shortcut(&root, "123", &NewShortcut { launch_options: "--game demo-2".into(), ..sc.clone() }).unwrap();
    assert_ne!(a, b);

    let (_, list) = steam::read_shortcuts(&root, "123").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].index, "0");
    assert_eq!(list[1].index, "1");
    assert_eq!(list[0].name, "示例游戏");
    assert_eq!(list[0].exe, r"C:\Tools\ludonimbus-run.exe");
    assert_eq!(list[0].start_dir, r"C:\Games\Demo");
    assert_eq!(list[0].icon, r"C:\Games\Demo\demo.exe");
    assert_eq!(list[0].wrapped_key().as_deref(), Some("demo"));
    assert_eq!(list[1].wrapped_key().as_deref(), Some("demo-2"));

    // Artwork: writing a new format replaces the old file of the same kind.
    let png = tmp.path().join("cover.png");
    fs::write(&png, b"png").unwrap();
    let jpg = tmp.path().join("cover.JPEG");
    fs::write(&jpg, b"jpg").unwrap();
    let p = steam::write_art(&root, "123", a, "cover", &png).unwrap();
    assert!(p.ends_with(format!("{a}p.png")));
    steam::write_art(&root, "123", a, "cover", &jpg).unwrap();
    let art = steam::art(&root, "123", a);
    assert!(art.cover.as_ref().unwrap().ends_with(format!("{a}p.jpg")));
    assert!(!steam::grid_dir(&root, "123").join(format!("{a}p.png")).exists());
    steam::write_art(&root, "123", a, "wide", &png).unwrap();
    steam::write_art(&root, "123", a, "icon", &png).unwrap();
    let icon = steam::art(&root, "123", a).icon.unwrap();
    steam::set_shortcut_icon(&root, "123", a, &icon).unwrap();
    assert!(steam::grid_dir(&root, "123").join(format!("{a}.png")).exists());
    assert!(steam::write_art(&root, "123", a, "poster", &png).is_err());

    // Removing renumbers the rest and deletes its artwork.
    assert!(steam::remove_shortcut(&root, "123", a).unwrap());
    let (_, list) = steam::read_shortcuts(&root, "123").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].index, "0");
    assert_eq!(list[0].appid, b);
    let art = steam::art(&root, "123", a);
    assert!(art.cover.is_none() && art.wide.is_none() && art.icon.is_none());
    assert!(!steam::remove_shortcut(&root, "123", a).unwrap());
    // Every write kept a backup of the previous file.
    assert!(fs::read_dir(tmp.path().join("home").join("steam-backups")).unwrap().count() >= 3);

    // Libraries and installed store games.
    let lib2 = tmp.path().join("SteamLibrary");
    fs::create_dir_all(lib2.join("steamapps")).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::write(
        root.join("steamapps").join("libraryfolders.vdf"),
        format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            root.display().to_string().replace('\\', "\\\\"),
            lib2.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    fs::write(
        lib2.join("steamapps").join("appmanifest_367520.acf"),
        "\"AppState\"\n{\n\t\"appid\"\t\t\"367520\"\n\t\"name\"\t\t\"Hollow Knight\"\n\t\"installdir\"\t\t\"Hollow Knight\"\n}\n",
    )
    .unwrap();
    let libs = steam::library_folders(&root);
    assert_eq!(libs.len(), 2);
    let apps = steam::installed_apps(&root);
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].appid, 367520);
    assert_eq!(apps[0].name, "Hollow Knight");
    assert!(apps[0].install_dir.ends_with("steamapps/common/Hollow Knight"));
}
