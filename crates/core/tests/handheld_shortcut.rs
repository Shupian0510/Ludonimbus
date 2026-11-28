//! The handheld screen's Steam shortcut is found again whatever Ludonimbus
//! program it points at (the Windows app, the Linux AppImage), so adding it
//! a second time replaces it instead of adding another one.

use ludonimbus_core::{handheld, steam};
use std::fs;

#[test]
fn the_handheld_shortcut_is_found_for_the_appimage_too() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let root = tmp.path().join("Steam");
    fs::create_dir_all(root.join("userdata").join("42").join("config")).unwrap();

    for app in ["ludonimbus-app.exe", "Ludonimbus_1.0.0_amd64.AppImage"] {
        let exe = tmp.path().join("apps").join(app);
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"x").unwrap();

        let first = handheld::add_to_steam(&root, "42", &exe).unwrap();
        let found = handheld::steam_shortcut(&root, "42").expect("the shortcut just added");
        assert_eq!(found.appid, first, "{app}");

        // Adding again replaces it: still one Ludonimbus shortcut.
        handheld::add_to_steam(&root, "42", &exe).unwrap();
        let (_, list) = steam::read_shortcuts(&root, "42").unwrap();
        assert_eq!(list.iter().filter(|s| s.launch_options.contains("--handheld")).count(), 1, "{app}");
    }
}
