//! Handhelds and controller-only screens (docs/plan-phase5.md §1–2): Windows
//! handhelds in the Xbox full screen experience, Steam Big Picture, Steam
//! Deck Game Mode. One full-screen window at a time, no tray, a controller
//! instead of mouse and keyboard.

use crate::config::{AppConfig, HandheldMode};

/// A controller interface that is up right now: what "Auto" goes by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shell {
    /// Steam started us from its controller interface (Big Picture on
    /// Windows, Game Mode on the Deck): it sets `SteamGamepadUI=1` (and on a
    /// Deck `SteamDeck=1`) for the programs it launches.
    pub steam_gamepad_ui: bool,
    /// The Xbox full screen experience is on ([`xbox_fse`]).
    pub xbox_fse: bool,
}

impl Shell {
    pub fn detect() -> Self {
        Self::from_inputs(|name| std::env::var(name).ok(), xbox_fse)
    }

    /// `var` reads an environment variable; `fse` says whether the Xbox full
    /// screen experience is on.
    fn from_inputs(var: impl Fn(&str) -> Option<String>, fse: impl FnOnce() -> bool) -> Self {
        let on = |name: &str| var(name).is_some_and(|v| v.trim() == "1");
        Shell { steam_gamepad_ui: on("SteamGamepadUI") || on("SteamDeck"), xbox_fse: fse() }
    }

    pub fn any(self) -> bool {
        self.steam_gamepad_ui || self.xbox_fse
    }
}

/// Steam started us from its controller interface (see [`Shell`]).
pub fn steam_gamepad_ui() -> bool {
    Shell::from_inputs(|name| std::env::var(name).ok(), || false).steam_gamepad_ui
}

/// A controller interface is up right now (see [`Shell`]).
pub fn controller_shell() -> bool {
    Shell::detect().any()
}

/// Whether the Xbox full screen experience is on (Windows 11 handhelds, and
/// PCs in preview): `IsGamingFullScreenExperienceActive` from the
/// `api-ms-win-gaming-experience-l1-1-0` API set (gamemode.dll), looked up
/// when first asked, so a Windows without it just says no.
#[cfg(windows)]
pub fn xbox_fse() -> bool {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
    type IsActive = unsafe extern "system" fn() -> i32;
    static FUNC: OnceLock<Option<IsActive>> = OnceLock::new();
    let func = FUNC.get_or_init(|| {
        let dll: Vec<u16> = "api-ms-win-gaming-experience-l1-1-0.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL-terminated names. The module is never freed, so the
        // function pointer stays valid for the life of the process.
        unsafe {
            let module = LoadLibraryExW(dll.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
            if module.is_null() {
                return None;
            }
            let f = GetProcAddress(module, c"IsGamingFullScreenExperienceActive".as_ptr().cast())?;
            Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, IsActive>(f))
        }
    });
    // SAFETY: it takes nothing and returns a BOOL. Only the low byte is
    // read, which is also right for a C++ `bool`.
    func.is_some_and(|f| unsafe { f() } & 0xff != 0)
}

#[cfg(not(windows))]
pub fn xbox_fse() -> bool {
    false
}

/// Whether the launch wrapper should ask in a full-screen window (a child
/// `ludonimbus-app --prompt`) instead of system dialogs.
pub fn wants_prompt_window(cfg: &AppConfig) -> bool {
    prompt_window(cfg.handheld, Shell::detect())
}

fn prompt_window(mode: HandheldMode, shell: Shell) -> bool {
    match mode {
        HandheldMode::On => true,
        HandheldMode::Off => false,
        HandheldMode::Auto => shell.any(),
    }
}

/// Whether Ludonimbus, opened without `--handheld`, shows the handheld view
/// full screen: only while a controller interface is up (opened from the
/// Xbox full screen experience, or by Steam Big Picture), and not when the
/// setting is "Off". "On" is about the launch questions and doesn't switch
/// the view, and neither does the device being a known handheld
/// ([`device`]): a handheld in a dock opens the desktop view.
pub fn handheld_view(cfg: &AppConfig) -> bool {
    auto_view(cfg.handheld, Shell::detect())
}

fn auto_view(mode: HandheldMode, shell: Shell) -> bool {
    mode != HandheldMode::Off && shell.any()
}

/// How much to zoom the handheld view on a `width` × `height` pixel screen
/// at Windows' `scale`. It is laid out for 1280 × 720 CSS pixels, where its
/// text meets Xbox's handheld guidance (at least 12 pixels high at 720p, 18
/// at 1080p); zooming keeps it that way whatever scaling the device uses.
pub fn view_zoom(width: u32, height: u32, scale: f64) -> f64 {
    if width == 0 || height == 0 || scale <= 0.0 {
        return 1.0;
    }
    let fit = (f64::from(width) / 1280.0).min(f64::from(height) / 720.0) / scale;
    let zoom = fit.clamp(0.75, 2.0);
    if (zoom - 1.0).abs() < 0.1 {
        1.0
    } else {
        (zoom * 20.0).round() / 20.0
    }
}

/// Handhelds by the names their firmware reports (manufacturer, product,
/// family, version, board): `(all of these, name to show)`; the first match
/// wins. Short model codes go with the maker's name.
const KNOWN: &[(&[&str], &str)] = &[
    (&["rog xbox ally x"], "ROG Xbox Ally X"),
    (&["asus", "rc73x"], "ROG Xbox Ally X"),
    (&["rog xbox ally"], "ROG Xbox Ally"),
    (&["asus", "rc73"], "ROG Xbox Ally"),
    (&["rog ally x"], "ROG Ally X"),
    (&["asus", "rc72l"], "ROG Ally X"),
    (&["rog ally"], "ROG Ally"),
    (&["asus", "rc71l"], "ROG Ally"),
    (&["lenovo", "83l3"], "Legion Go S"),
    (&["lenovo", "83n6"], "Legion Go S"),
    (&["lenovo", "83q2"], "Legion Go S"),
    (&["lenovo", "83q3"], "Legion Go S"),
    (&["legion go s"], "Legion Go S"),
    (&["lenovo", "83n0"], "Legion Go 2"),
    (&["lenovo", "83n1"], "Legion Go 2"),
    (&["legion go 2"], "Legion Go 2"),
    (&["lenovo", "83e1"], "Legion Go"),
    (&["legion go"], "Legion Go"),
    (&["micro-star", "claw"], "MSI Claw"),
    (&["msi claw"], "MSI Claw"),
    (&["valve", "jupiter"], "Steam Deck"),
    (&["valve", "galileo"], "Steam Deck OLED"),
    (&["ayaneo"], "AYANEO"),
    (&["ayadevice"], "AYANEO"),
    (&["one-netbook"], "OneXPlayer"),
    (&["onexplayer"], "OneXPlayer"),
    (&["one xplayer"], "OneXPlayer"),
    (&["aokzoe"], "AOKZOE"),
    (&["zotac gaming zone"], "ZOTAC ZONE"),
    (&["gpd", "g1618"], "GPD WIN 4"),
    (&["gpd", "g1617"], "GPD WIN Mini"),
    (&["gpd", "g1619"], "GPD WIN Max 2"),
    (&["nitro blaze"], "Acer Nitro Blaze"),
    (&["ayn", "loki"], "AYN Loki"),
    (&["anbernic", "win600"], "ANBERNIC Win600"),
];

/// What kind of device this is, for its record in the cloud (the other
/// devices show it by an icon): "deck", "handheld", "laptop" or "desktop".
/// A Switch registers itself as "switch". Read once: a known handheld by
/// its firmware ([`device`]), a laptop by its battery or its chassis.
pub fn device_kind() -> &'static str {
    static K: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    K.get_or_init(|| kind_of(device().as_deref(), has_battery(), chassis_type()))
}

/// SMBIOS chassis types: hand held, and the portable ones (portable,
/// laptop, notebook, sub notebook, tablet, convertible, detachable).
const CHASSIS_HANDHELD: u32 = 11;
const CHASSIS_PORTABLE: &[u32] = &[8, 9, 10, 14, 30, 31, 32];

fn kind_of(handheld: Option<&str>, battery: bool, chassis: Option<u32>) -> &'static str {
    match handheld {
        Some(name) if name.starts_with("Steam Deck") => "deck",
        Some(_) => "handheld",
        None if chassis == Some(CHASSIS_HANDHELD) => "handheld",
        None if battery || chassis.is_some_and(|c| CHASSIS_PORTABLE.contains(&c)) => "laptop",
        None => "desktop",
    }
}

/// The machine runs on a battery of its own (not a controller's or a
/// mouse's).
fn has_battery() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: fills the struct it is given and touches nothing else.
        let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        // 128: no system battery; 255: unknown.
        unsafe { GetSystemPowerStatus(&mut s) != 0 && s.BatteryFlag != 128 && s.BatteryFlag != 255 }
    }
    #[cfg(not(windows))]
    {
        let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else {
            return false;
        };
        dir.flatten().any(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
            read("type") == "Battery" && read("scope") != "Device"
        })
    }
}

/// The SMBIOS chassis type, where the system gives it cheaply (Linux).
fn chassis_type() -> Option<u32> {
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/sys/class/dmi/id/chassis_type").ok()?.trim().parse().ok()
    }
    #[cfg(windows)]
    {
        None
    }
}

/// The handheld this is, when its firmware names a known one. Only a hint
/// for defaults (the first-run guide's handheld step, Settings): it never
/// switches the view by itself, since a handheld can sit in a dock.
pub fn device() -> Option<String> {
    match_device(&firmware_names()).map(str::to_string)
}

fn match_device(names: &[String]) -> Option<&'static str> {
    let text = names.join(" ").to_lowercase();
    KNOWN.iter().find(|(keys, _)| keys.iter().all(|k| text.contains(k))).map(|(_, name)| *name)
}

/// The machine's names from its firmware (SMBIOS): Windows keeps a copy in
/// the registry, Linux in sysfs. Both are cheap to read.
fn firmware_names() -> Vec<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\BIOS") else {
            return Vec::new();
        };
        ["SystemManufacturer", "SystemProductName", "SystemFamily", "SystemVersion", "BaseBoardManufacturer", "BaseBoardProduct"]
            .iter()
            .filter_map(|n| k.get_value::<String, _>(n).ok())
            .collect()
    }
    #[cfg(not(windows))]
    {
        ["sys_vendor", "product_name", "product_family", "product_version", "board_vendor", "board_name"]
            .iter()
            .filter_map(|n| std::fs::read_to_string(format!("/sys/class/dmi/id/{n}")).ok())
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
    // Any of Ludonimbus's own programs: `ludonimbus-app.exe` on Windows, the
    // AppImage (`Ludonimbus….AppImage`) or `ludonimbus-app` on Linux.
    list.into_iter().find(|s| crate::discover::is_own_program(std::path::Path::new(&s.exe)) && s.launch_options.contains(HANDHELD_ARG))
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
    fn device_kinds() {
        assert_eq!(kind_of(Some("Steam Deck OLED"), true, Some(1)), "deck");
        assert_eq!(kind_of(Some("ROG Ally"), true, None), "handheld");
        // A handheld the list doesn't know, by its chassis.
        assert_eq!(kind_of(None, true, Some(11)), "handheld");
        assert_eq!(kind_of(None, true, None), "laptop");
        assert_eq!(kind_of(None, false, Some(10)), "laptop");
        assert_eq!(kind_of(None, false, Some(3)), "desktop");
        assert_eq!(kind_of(None, false, None), "desktop");
        assert!(["deck", "handheld", "laptop", "desktop"].contains(&device_kind()));
    }

    #[test]
    fn known_names() {
        // What the firmware fields read on these machines (manufacturer,
        // product, family, version, board).
        let find = |fields: &[&str]| match_device(&fields.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(find(&["ASUSTeK COMPUTER INC.", "ROG Ally RC71L_RC71L", "ROG Ally", "1.0", "RC71L"]), Some("ROG Ally"));
        assert_eq!(find(&["ASUSTeK COMPUTER INC.", "ROG Ally X RC72LA_RC72LA", "ROG Ally X", "1.0", "RC72LA"]), Some("ROG Ally X"));
        assert_eq!(find(&["ASUSTeK COMPUTER INC.", "ROG Xbox Ally X RC73XA_RC73XA", "ROG Xbox Ally X", "", "RC73XA"]), Some("ROG Xbox Ally X"));
        assert_eq!(find(&["ASUSTeK COMPUTER INC.", "ROG Xbox Ally RC73YA_RC73YA", "ROG Xbox Ally", "", "RC73YA"]), Some("ROG Xbox Ally"));
        assert_eq!(find(&["ASUSTeK COMPUTER INC.", "", "", "", "RC73YA"]), Some("ROG Xbox Ally"));
        assert_eq!(find(&["LENOVO", "83E1", "Legion Go 8APU1", "Legion Go 8APU1", "LNVNB161216"]), Some("Legion Go"));
        assert_eq!(find(&["LENOVO", "83L3", "Legion Go S 8APU1", "Legion Go S 8APU1", "LNVNB161216"]), Some("Legion Go S"));
        assert_eq!(find(&["LENOVO", "83N0", "Legion Go 8AHP2", "Legion Go 8AHP2", "LNVNB161216"]), Some("Legion Go 2"));
        assert_eq!(find(&["Micro-Star International Co., Ltd.", "Claw A1M", "Claw", "REV:1.0", "MS-1T41"]), Some("MSI Claw"));
        assert_eq!(find(&["Valve", "Jupiter", "Aerith", "1", "Jupiter"]), Some("Steam Deck"));
        assert_eq!(find(&["Valve", "Galileo", "Sephiroth", "1", "Galileo"]), Some("Steam Deck OLED"));
        assert_eq!(find(&["AYANEO", "AYANEO 2S", "", "", "AB05-R7"]), Some("AYANEO"));
        assert_eq!(find(&["ONE-NETBOOK", "ONEXPLAYER X1 i", "", "", "ONEXPLAYER"]), Some("OneXPlayer"));
        assert_eq!(find(&["GPD", "G1618-04", "", "", "Default string"]), Some("GPD WIN 4"));
        // Desktops, laptops, and model codes without the maker.
        assert_eq!(find(&["Micro-Star International Co., Ltd.", "MS-7E01", "Default string", "1.0", "MAG B650"]), None);
        assert_eq!(
            find(&["ASUS", "System Product Name", "To be filled by O.E.M.", "System Version", "ASUSTeK COMPUTER INC.", "ROG STRIX B860-I GAMING WIFI"]),
            None
        );
        assert_eq!(find(&["Gigabyte Technology Co., Ltd.", "Jupiter Lake", "", "", "RC71LX"]), None);
        assert_eq!(find(&["LENOVO", "82JW", "Legion 5 15ACH6H", "Legion 5 15ACH6H", "LNVNB161216"]), None);
        assert_eq!(find(&[]), None);
    }

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    }

    #[test]
    fn controller_interfaces() {
        let shell = |vars: &'static [(&'static str, &'static str)], fse: bool| Shell::from_inputs(env(vars), || fse);
        assert_eq!(shell(&[], false), Shell::default());
        assert!(!shell(&[], false).any());
        assert!(shell(&[("SteamGamepadUI", "1")], false).steam_gamepad_ui);
        assert!(shell(&[("SteamDeck", "1")], false).steam_gamepad_ui);
        assert!(!shell(&[("SteamGamepadUI", "0"), ("SteamDeck", "")], false).any());
        let fse = shell(&[], true);
        assert!(fse.xbox_fse && !fse.steam_gamepad_ui && fse.any());
    }

    #[test]
    fn auto_on_off() {
        let desk = Shell::default();
        let fse = Shell { xbox_fse: true, ..Shell::default() };
        let big_picture = Shell { steam_gamepad_ui: true, ..Shell::default() };
        // The launch questions.
        assert!(!prompt_window(HandheldMode::Auto, desk));
        assert!(prompt_window(HandheldMode::Auto, fse));
        assert!(prompt_window(HandheldMode::Auto, big_picture));
        assert!(prompt_window(HandheldMode::On, desk));
        assert!(!prompt_window(HandheldMode::Off, fse));
        // The view Ludonimbus opens in: only while a controller interface is up.
        assert!(!auto_view(HandheldMode::Auto, desk));
        assert!(!auto_view(HandheldMode::On, desk));
        assert!(auto_view(HandheldMode::Auto, fse));
        assert!(auto_view(HandheldMode::On, big_picture));
        assert!(!auto_view(HandheldMode::Off, fse));
    }

    #[test]
    fn fse_lookup_never_fails() {
        // Whatever this machine is, asking twice gives an answer (false where
        // the API set doesn't exist, e.g. Linux or an older Windows).
        assert_eq!(xbox_fse(), xbox_fse());
        #[cfg(not(windows))]
        assert!(!xbox_fse());
    }

    #[test]
    fn zoom_for_the_screen() {
        // The usual scaling of 7–8" handhelds needs nothing.
        assert_eq!(view_zoom(1920, 1080, 1.5), 1.0); // ROG Ally, MSI Claw
        assert_eq!(view_zoom(1920, 1200, 1.5), 1.0); // Legion Go S
        assert_eq!(view_zoom(2560, 1600, 2.0), 1.0); // Legion Go
                                                     // Other scaling: the text stays as large on the screen.
        assert_eq!(view_zoom(1920, 1080, 1.0), 1.5);
        assert_eq!(view_zoom(1920, 1080, 1.25), 1.2);
        assert_eq!(view_zoom(1280, 800, 1.5), 0.75); // Steam Deck at 150 %
        assert_eq!(view_zoom(3840, 2160, 1.0), 2.0); // a TV
        assert_eq!(view_zoom(0, 0, 1.0), 1.0);
        assert_eq!(view_zoom(1920, 1080, 0.0), 1.0);
    }
}
