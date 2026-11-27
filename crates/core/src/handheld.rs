//! Handhelds and controller-only screens (docs/plan-phase5.md §1–2): Windows
//! handhelds in the Xbox full screen experience, Steam Big Picture, Steam
//! Deck Game Mode. One full-screen window at a time, no tray, a controller
//! instead of mouse and keyboard.

use crate::config::{AppConfig, HandheldMode};

/// Steam started us from its controller interface (Big Picture on Windows,
/// Game Mode on the Deck): it sets these for the programs it launches.
pub fn steam_gamepad_ui() -> bool {
    let on = |name: &str| std::env::var(name).is_ok_and(|v| v == "1");
    on("SteamGamepadUI") || on("SteamDeck")
}

/// Whether the launch wrapper should ask in a full-screen window (a child
/// `ludonimbus-app --prompt`) instead of system dialogs.
pub fn wants_prompt_window(cfg: &AppConfig) -> bool {
    match cfg.handheld {
        HandheldMode::On => true,
        HandheldMode::Off => false,
        HandheldMode::Auto => steam_gamepad_ui(),
    }
}

/// Handhelds by the names their firmware reports (manufacturer, product,
/// family): `(match, name to show)`.
const KNOWN: &[(&str, &str)] = &[
    ("rog xbox ally", "ROG Xbox Ally"),
    ("rc73", "ROG Xbox Ally"),
    ("rog ally", "ROG Ally"),
    ("rc71l", "ROG Ally"),
    ("rc72l", "ROG Ally X"),
    ("legion go", "Legion Go"),
    ("83e1", "Legion Go"),
    ("83l3", "Legion Go S"),
    ("83n6", "Legion Go S"),
    ("claw", "MSI Claw"),
    ("jupiter", "Steam Deck"),
    ("galileo", "Steam Deck"),
    ("ayaneo", "AYANEO"),
    ("one-netbook", "OneXPlayer"),
    ("onexplayer", "OneXPlayer"),
    ("aokzoe", "AOKZOE"),
    ("zotac gaming zone", "ZOTAC ZONE"),
    ("g1618", "GPD WIN 4"),
    ("g1617", "GPD WIN Mini"),
];

/// The handheld this is, when its firmware names a known one.
pub fn device() -> Option<String> {
    let text = firmware_names().join(" ").to_lowercase();
    KNOWN.iter().find(|(k, _)| text.contains(k)).map(|(_, n)| n.to_string())
}

fn firmware_names() -> Vec<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\BIOS") else {
            return Vec::new();
        };
        ["SystemManufacturer", "SystemProductName", "SystemFamily", "BaseBoardProduct"].iter().filter_map(|n| k.get_value::<String, _>(n).ok()).collect()
    }
    #[cfg(not(windows))]
    {
        ["sys_vendor", "product_name", "product_family", "board_name"]
            .iter()
            .filter_map(|n| std::fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{n}")).ok())
            .map(|s| s.trim().to_string())
            .collect()
    }
}

/// What a Steam shortcut to Ludonimbus's handheld screen launches with.
pub const HANDHELD_ARG: &str = "--handheld";

/// The Steam shortcut that opens Ludonimbus's handheld screen, if any (so
/// it can be opened from the Steam library where there is no tray).
pub fn steam_shortcut(root: &std::path::Path, account: &str) -> Option<crate::steam::Shortcut> {
    let (_, list) = crate::steam::read_shortcuts(root, account).ok()?;
    list.into_iter().find(|s| {
        let exe = crate::util::file_name_lower(std::path::Path::new(&crate::util::native_sep(&crate::util::unquote(&s.exe))));
        exe.starts_with("ludonimbus-app") && s.launch_options.contains(HANDHELD_ARG)
    })
}

/// Add a Steam shortcut that opens Ludonimbus's handheld screen (replacing
/// an older one, e.g. after Ludonimbus moved). Steam must be closed.
pub fn add_to_steam(root: &std::path::Path, account: &str, app: &std::path::Path) -> anyhow::Result<u32> {
    if let Some(old) = steam_shortcut(root, account) {
        crate::steam::remove_shortcut(root, account, old.appid)?;
    }
    let exe = app.to_string_lossy().into_owned();
    crate::steam::add_shortcut(
        root,
        account,
        &crate::steam::NewShortcut {
            name: "Ludonimbus".into(),
            exe: exe.clone(),
            start_dir: app.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            launch_options: HANDHELD_ARG.into(),
            icon: exe,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names() {
        let find = |s: &str| KNOWN.iter().find(|(k, _)| s.to_lowercase().contains(k)).map(|(_, n)| *n);
        assert_eq!(find("ASUSTeK COMPUTER INC. ROG Ally RC71L_RC71L"), Some("ROG Ally"));
        assert_eq!(find("ASUSTeK COMPUTER INC. ROG Xbox Ally X RC73XA"), Some("ROG Xbox Ally"));
        assert_eq!(find("LENOVO 83E1 Legion Go 8APU1"), Some("Legion Go"));
        assert_eq!(find("Valve Jupiter"), Some("Steam Deck"));
        assert_eq!(find("Micro-Star International Co., Ltd. MS-7E01"), None);
    }
}
