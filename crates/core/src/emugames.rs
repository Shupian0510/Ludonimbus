//! Games found in emulators' own data folders: every game that has saves
//! there, with or without a Steam shortcut, so each can be synced on its
//! own. Names come from the saves or the emulator's game list. Where the
//! emulator says where the game file is (Ryujinx's and Dolphin's game
//! folders, Cemu's game list), the launch options that start it are filled
//! in too.

use crate::adapters::Adapter;
use crate::emulators::{self, EmuGame};
use crate::{spec, switch};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SavedGame {
    pub game: EmuGame,
    /// Launch options that start this game (empty when its file wasn't found).
    pub args: String,
}

impl SavedGame {
    fn new(game: EmuGame) -> SavedGame {
        SavedGame { game, args: String::new() }
    }
}

/// An emulator rule's roots on this PC (`install`: the emulator's folder, if known).
pub fn roots(a: &Adapter, install: Option<&Path>) -> BTreeMap<String, PathBuf> {
    let mut known = spec::builtin_roots(install);
    for (name, tpl) in &a.roots {
        if let Ok(p) = spec::expand_root(tpl, &known) {
            known.insert(name.clone(), p);
        }
    }
    for (k, v) in emulators::config_roots(a, &known, &[]) {
        known.insert(k, v);
    }
    known
}

/// The games with saves in this emulator's data folder.
pub fn games(a: &Adapter, install: Option<&Path>) -> Vec<SavedGame> {
    if !a.is_emulator() || a.game_saves.is_empty() {
        return Vec::new();
    }
    let r = roots(a, install);
    let dir = |root: &str| r.get(root).filter(|p| p.is_dir()).cloned();
    // The folder the per-game saves are in.
    let save_root = a
        .game_saves
        .values()
        .flatten()
        .next()
        .and_then(|t| spec::split_pattern(t).ok())
        .map(|(root, _)| root)
        .unwrap_or_default();
    let mut out = match a.game_id.as_deref() {
        Some("switch") => match (dir("emu"), dir("user")) {
            (Some(emu), _) => ryujinx(&emu),
            (None, Some(user)) => yuzu(&user),
            (None, None) => Vec::new(),
        },
        Some(kind) => match dir(&save_root) {
            None => Vec::new(),
            Some(base) => match kind {
                "gc-wii" => dolphin(&base),
                "wiiu" => cemu(&base, &r),
                "3ds" => n3ds(&base),
                "ps3" => serial_saves(&base.join("dev_hdd0").join("home").join("00000001").join("savedata"), |d| d.join("PARAM.SFO")),
                "psp" => serial_saves(&base.join("PSP").join("SAVEDATA"), |d| d.join("PARAM.SFO")),
                "ps4" => ps4(&base.join("savedata").join("1")),
                "vita" => vita(&base),
                "rom" => retroarch(&base.join("saves")),
                _ => Vec::new(),
            },
        },
        None => Vec::new(),
    };
    out.retain(|g| !g.game.saves(a).is_empty());
    out
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn files_in(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn has_files(p: &Path) -> bool {
    walkdir::WalkDir::new(p).into_iter().flatten().any(|e| e.file_type().is_file())
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Printable ASCII up to the first NUL (save titles in old formats).
fn ascii(b: &[u8]) -> Option<String> {
    let b = b.split(|&c| c == 0).next().unwrap_or(b);
    let s = String::from_utf8_lossy(b).trim().to_string();
    (!s.is_empty() && s.chars().all(|c| c.is_ascii() && !c.is_ascii_control())).then_some(s)
}

fn read_at(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Game files in folders an emulator lists, a few levels deep.
fn game_files(dirs: &[PathBuf], exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs {
        for e in walkdir::WalkDir::new(d).max_depth(4).into_iter().flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if e.file_type().is_file() && exts.contains(&ext.as_str()) {
                out.push(p.to_path_buf());
                if out.len() >= 5000 {
                    return out;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Dolphin

fn dolphin(emu: &Path) -> Vec<SavedGame> {
    let mut out: Vec<SavedGame> = Vec::new();
    // GameCube: one GCI file per save in the "GCI folder" memory cards.
    for region in subdirs(&emu.join("GC")) {
        for card in subdirs(&region).into_iter().filter(|c| name_of(c).starts_with("Card ")) {
            for f in files_in(&card).into_iter().filter(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("gci"))) {
                let Some(h) = read_at(&f, 0, 0x40) else { continue };
                let (code, maker) = (&h[0..4], &h[4..6]);
                if !code.iter().chain(maker).all(u8::is_ascii_alphanumeric) {
                    continue;
                }
                let id4 = String::from_utf8_lossy(code).into_owned();
                let maker = String::from_utf8_lossy(maker).into_owned();
                let id = format!("{id4}{maker}");
                if out.iter().any(|g| g.game.variant == "gc" && g.game.id == id) {
                    continue;
                }
                // The save's comment: the game's name, then what the file is.
                let comment = u32::from_be_bytes([h[0x3C], h[0x3D], h[0x3E], h[0x3F]]) as u64;
                let name = read_at(&f, 0x40 + comment, 32).and_then(|b| ascii(&b));
                let id4hex: String = id4.bytes().map(|b| format!("{b:02x}")).collect();
                let vars = [("id4", id4.clone()), ("maker", maker), ("id4hex", id4hex), ("id", id.clone())];
                out.push(SavedGame::new(EmuGame::new(id, "gc", &vars, None).named(name)));
            }
        }
    }
    // Wii: title folders (discs, WiiWare / Virtual Console, disc channels) with saved data.
    for kind in ["00010000", "00010001", "00010004"] {
        for d in subdirs(&emu.join("Wii").join("title").join(kind)) {
            let hex = name_of(&d).to_lowercase();
            let Some(id4) = hex_ascii(&hex) else { continue };
            let data = d.join("data");
            if !has_files(&data) || out.iter().any(|g| g.game.variant == "wii" && g.game.id.starts_with(&id4)) {
                continue;
            }
            let name = wii_banner_title(&data.join("banner.bin"));
            let vars = [("id4", id4.clone()), ("id4hex", hex.clone()), ("id", id4.clone())];
            out.push(SavedGame::new(EmuGame::new(id4, "wii", &vars, None).named(name)));
        }
    }
    // Discs in Dolphin's game folders: launch options that start each game.
    let discs = game_files(&dolphin_iso_paths(emu), &["iso", "gcm", "rvz", "wia", "wbfs", "ciso"]);
    if !discs.is_empty() && !out.is_empty() {
        let ids: HashMap<String, PathBuf> = discs
            .iter()
            .filter_map(|p| emulators::gc_wii(p).map(|g| (format!("{}:{}", g.variant, g.id), p.clone())))
            .collect();
        for g in &mut out {
            let hit = match g.game.variant.as_str() {
                "gc" => ids.get(&format!("gc:{}", g.game.id)),
                _ => ids.iter().find(|(k, _)| k.starts_with(&format!("wii:{}", g.game.id))).map(|(_, p)| p),
            };
            if let Some(p) = hit {
                g.args = format!("-b -e \"{}\"", p.display());
                g.game.source = Some(p.clone());
            }
        }
    }
    out
}

/// `524d4745` → `RMGE`.
fn hex_ascii(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..4).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect();
    let bytes = bytes?;
    bytes.iter().all(u8::is_ascii_alphanumeric).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// The title in a Wii save's `banner.bin` (UTF-16BE).
fn wii_banner_title(p: &Path) -> Option<String> {
    let b = read_at(p, 0, 0x60)?;
    if &b[0..4] != b"WIBN" {
        return None;
    }
    let units: Vec<u16> = b[0x20..0x60].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    let s: String = char::decode_utf16(units).map(|c| c.unwrap_or('?')).collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `ISOPath0…` in Dolphin's `Config/Dolphin.ini`.
fn dolphin_iso_paths(emu: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(emu.join("Config").join("Dolphin.ini")) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            (k.starts_with("ISOPath") && k[7..].chars().all(|c| c.is_ascii_digit()) && k.len() > 7).then(|| PathBuf::from(v.trim()))
        })
        .filter(|p| p.is_dir())
        .collect()
}

// ---------------------------------------------------------------------------
// Cemu

struct CemuTitle {
    name: String,
    path: Option<PathBuf>,
}

fn cemu(mlc: &Path, roots: &BTreeMap<String, PathBuf>) -> Vec<SavedGame> {
    let cache = emulators::cemu_settings(roots).map(|f| cemu_game_cache(&f)).unwrap_or_default();
    let mut out = Vec::new();
    for d in subdirs(&mlc.join("usr").join("save").join("00050000")) {
        let low = name_of(&d).to_lowercase();
        if low.len() != 8 || !low.chars().all(|c| c.is_ascii_hexdigit()) || !has_files(&d.join("user")) {
            continue;
        }
        let id = format!("00050000{low}");
        let title = u64::from_str_radix(&id, 16).ok().and_then(|t| cache.get(&t));
        let name = title.map(|t| t.name.clone()).or_else(|| wiiu_meta_name(&d.join("meta").join("meta.xml")));
        let mut g = SavedGame::new(emulators::wiiu_game(&id, None).named(name));
        // The game list knows the game file (.rpx, .wud, .wux, .wua) or its code folder.
        let file = title.and_then(|t| t.path.as_ref()).and_then(|p| {
            if p.is_file() {
                Some(p.clone())
            } else {
                files_in(p).into_iter().find(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("rpx")))
            }
        });
        if let Some(f) = file {
            g.args = format!("-g \"{}\"", f.display());
            g.game.source = Some(f);
        }
        out.push(g);
    }
    out
}

fn xml_text(block: &str, tag: &str) -> Option<String> {
    let open = block.find(&format!("<{tag}"))?;
    let start = open + block[open..].find('>')? + 1;
    if block[open..start].ends_with("/>") {
        return None;
    }
    let end = start + block[start..].find(&format!("</{tag}>"))?;
    let v = block[start..end].trim();
    (!v.is_empty()).then(|| v.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">"))
}

/// Cemu's game list (`<GameCache>` in settings.xml): name and game file by title id.
fn cemu_game_cache(settings: &Path) -> HashMap<u64, CemuTitle> {
    let mut out = HashMap::new();
    let Ok(bytes) = std::fs::read(settings) else { return out };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (Some(s), Some(e)) = (text.find("<GameCache>"), text.find("</GameCache>")) else { return out };
    if e <= s {
        return out;
    }
    for entry in text[s..e].split("<Entry>").skip(1) {
        let Some(id) = xml_text(entry, "title_id").and_then(|v| v.parse::<u64>().ok()) else { continue };
        let Some(name) = xml_text(entry, "custom_name").or_else(|| xml_text(entry, "name")) else { continue };
        let path = xml_text(entry, "path").map(PathBuf::from).filter(|p| p.exists());
        out.insert(id, CemuTitle { name, path });
    }
    out
}

/// `longname_en` (else another language) in a Wii U `meta.xml`.
fn wiiu_meta_name(p: &Path) -> Option<String> {
    let text = String::from_utf8_lossy(&std::fs::read(p).ok()?).into_owned();
    ["longname_en", "shortname_en", "longname_ja", "longname_zhs", "longname_zht"]
        .iter()
        .find_map(|t| xml_text(&text, t))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
}

// ---------------------------------------------------------------------------
// 3DS

fn n3ds(user: &Path) -> Vec<SavedGame> {
    let mut out: Vec<SavedGame> = Vec::new();
    for id0 in subdirs(&user.join("sdmc").join("Nintendo 3DS")) {
        for id1 in subdirs(&id0) {
            for d in subdirs(&id1.join("title").join("00040000")) {
                let low = name_of(&d).to_lowercase();
                if low.len() != 8 || !low.chars().all(|c| c.is_ascii_hexdigit()) || !has_files(&d.join("data")) {
                    continue;
                }
                let id = format!("00040000{low}");
                if !out.iter().any(|g| g.game.id.eq_ignore_ascii_case(&id)) {
                    out.push(SavedGame::new(emulators::n3ds_game(&id, None)));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// PlayStation

fn sfo_title(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    emulators::sfo_value(&b, "TITLE").or_else(|| emulators::sfo_value(&b, "STITLE")).filter(|s| !s.is_empty())
}

/// Save folders named `<serial><suffix>` (RPCS3, PPSSPP): one game per serial.
fn serial_saves(dir: &Path, sfo: impl Fn(&Path) -> PathBuf) -> Vec<SavedGame> {
    let mut out: Vec<SavedGame> = Vec::new();
    for d in subdirs(dir) {
        let n = name_of(&d);
        let Some(serial) = n.get(..9).filter(|s| emulators::serial_in(s).as_deref() == Some(*s)) else { continue };
        if !has_files(&d) {
            continue;
        }
        match out.iter_mut().find(|g| g.game.id == serial) {
            Some(g) if g.game.name.is_none() => g.game.name = sfo_title(&sfo(&d)),
            Some(_) => {}
            None => out.push(SavedGame::new(emulators::serial_game(serial, None).named(sfo_title(&sfo(&d))))),
        }
    }
    out
}

/// shadPS4: `savedata/1/<CUSA…>/<save>/sce_sys/param.sfo`.
fn ps4(dir: &Path) -> Vec<SavedGame> {
    subdirs(dir)
        .into_iter()
        .filter(|d| emulators::serial_in(&name_of(d)).as_deref() == Some(name_of(d).as_str()) && has_files(d))
        .map(|d| {
            let name = subdirs(&d).iter().find_map(|s| sfo_title(&s.join("sce_sys").join("param.sfo")));
            SavedGame::new(emulators::serial_game(&name_of(&d), None).named(name))
        })
        .collect()
}

/// Vita3K: `ux0/user/00/savedata/<id>`, named from `ux0/app/<id>`.
fn vita(ux0: &Path) -> Vec<SavedGame> {
    subdirs(&ux0.join("user").join("00").join("savedata"))
        .into_iter()
        .filter(|d| emulators::serial_in(&name_of(d)).as_deref() == Some(name_of(d).as_str()) && has_files(d))
        .map(|d| {
            let id = name_of(&d);
            let name = sfo_title(&ux0.join("app").join(&id).join("sce_sys").join("param.sfo"));
            SavedGame::new(emulators::serial_game(&id, None).named(name))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// RetroArch

/// Save files are named after the content file (`<name>.srm`), possibly in
/// a folder per core.
fn retroarch(saves: &Path) -> Vec<SavedGame> {
    let mut stems: Vec<String> = Vec::new();
    for e in walkdir::WalkDir::new(saves).max_depth(2).into_iter().flatten() {
        if !e.file_type().is_file() {
            continue;
        }
        let Some(stem) = e.path().file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        if !stem.is_empty() && e.path().extension().is_some() && !stems.contains(&stem) {
            stems.push(stem);
        }
    }
    stems.sort();
    stems
        .into_iter()
        .map(|s| SavedGame::new(EmuGame::new(s.clone(), "default", &[("stem", emulators::escape_glob(&s))], None).named(Some(s))))
        .collect()
}

// ---------------------------------------------------------------------------
// Switch: Ryujinx and the yuzu family

fn ryujinx(emu: &Path) -> Vec<SavedGame> {
    let found = switch_saved(switch::saves(emu), |t| switch::game_name(emu, t));
    switch_launch_args(found, &[emu.join("system").join("prod.keys")], &ryujinx_game_dirs(emu))
}

/// Eden, Citron, Sudachi, Suyu, yuzu (`user`: the emulator's user folder).
fn yuzu(user: &Path) -> Vec<SavedGame> {
    let found = switch_saved(switch::yuzu_saves(user), |_| None);
    let dirs: Vec<PathBuf> = switch::yuzu_game_dirs(user).into_iter().filter(|d| d.is_dir()).collect();
    switch_launch_args(found, &[user.join("keys").join("prod.keys")], &dirs)
}

/// One entry per game with files in its save.
fn switch_saved(saves: Vec<switch::NxSave>, name: impl Fn(u64) -> Option<String>) -> Vec<SavedGame> {
    let mut out: Vec<SavedGame> = Vec::new();
    for s in saves {
        if out.iter().any(|g| g.game.id == format!("{:016X}", s.title)) || !has_files(&s.dir) {
            continue;
        }
        out.push(SavedGame::new(emulators::switch_emu_game(s.title, None).named(name(s.title))));
    }
    out
}

/// Game files in the emulator's game folders: launch options that start each game.
fn switch_launch_args(mut out: Vec<SavedGame>, keys: &[PathBuf], game_dirs: &[PathBuf]) -> Vec<SavedGame> {
    if out.is_empty() {
        return out;
    }
    let mut keys = keys.to_vec();
    if let Some(h) = dirs::home_dir() {
        keys.push(h.join(".switch").join("prod.keys"));
    }
    let files = game_files(game_dirs, &["nsp", "xci", "nsz", "xcz"]);
    for f in files {
        if out.iter().all(|g| !g.args.is_empty()) {
            break;
        }
        let Some(t) = switch::title_id(&f, &keys) else { continue };
        if let Some(g) = out.iter_mut().find(|g| g.args.is_empty() && g.game.id == format!("{t:016X}")) {
            g.args = format!("\"{}\"", f.display());
            g.game.source = Some(f);
        }
    }
    out
}

/// `game_dirs` in Ryujinx's `Config.json`.
fn ryujinx_game_dirs(emu: &Path) -> Vec<PathBuf> {
    let Some(v) = std::fs::read_to_string(emu.join("Config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return Vec::new();
    };
    v["game_dirs"]
        .as_array()
        .map(|a| a.iter().filter_map(|d| d.as_str()).map(PathBuf::from).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters;

    fn write(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn gci(code: &str, maker: &str, name: &str) -> Vec<u8> {
        let mut b = vec![0u8; 0x40 + 0x100];
        b[0..4].copy_from_slice(code.as_bytes());
        b[4..6].copy_from_slice(maker.as_bytes());
        b[0x3C..0x40].copy_from_slice(&0x20u32.to_be_bytes());
        b[0x60..0x60 + name.len()].copy_from_slice(name.as_bytes());
        b
    }

    #[test]
    fn dolphin_saves_and_discs() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("Dolphin-x64");
        let emu = install.join("User");
        write(&emu, "GC/USA/Card A/01-GALE-SuperSmashBros0110290334.gci", &gci("GALE", "01", "Super Smash Bros. Melee"));
        write(&emu, "GC/USA/Card A/8P-GSAE-Sonic.gci", &gci("GSAE", "8P", "Sonic Adventure"));
        let mut banner = b"WIBN".to_vec();
        banner.resize(0x20, 0);
        for u in "Super Mario Galaxy".encode_utf16() {
            banner.extend_from_slice(&u.to_be_bytes());
        }
        banner.resize(0x80, 0);
        write(&emu, "Wii/title/00010000/524d4745/data/banner.bin", &banner);
        write(&emu, "Wii/title/00010000/524d4745/data/GameData.bin", b"x");
        // System menu data is not a game.
        write(&emu, "Wii/title/00010002/48414141/data/x.bin", b"x");
        // A disc in the game folder Dolphin lists.
        let mut disc = vec![0u8; 0x440];
        disc[0..6].copy_from_slice(b"GALE01");
        disc[0x1C..0x20].copy_from_slice(&0xC233_9F3Du32.to_be_bytes());
        write(tmp.path(), "ROMs/Melee.iso", &disc);
        let iso = tmp.path().join("ROMs").join("Melee.iso");
        write(&emu, "Config/Dolphin.ini", format!("[General]\nISOPaths = 1\nISOPath0 = {}\n", tmp.path().join("ROMs").display()).as_bytes());

        let a = adapters::find("dolphin").unwrap();
        let list = games(a, Some(&install));
        let ids: Vec<(&str, &str)> = list.iter().map(|g| (g.game.variant.as_str(), g.game.id.as_str())).collect();
        assert_eq!(ids, vec![("gc", "GALE01"), ("gc", "GSAE8P"), ("wii", "RMGE")]);
        assert_eq!(list[0].game.name.as_deref(), Some("Super Smash Bros. Melee"));
        assert_eq!(list[0].args, format!("-b -e \"{}\"", iso.display()));
        assert!(list[0].game.saves(a)[0].ends_with("Card ?/01-GALE-*.gci"));
        assert_eq!(list[2].game.name.as_deref(), Some("Super Mario Galaxy"));
        assert_eq!(list[2].game.saves(a), vec!["{emu}/Wii/title/0001000?/524d4745/data/**".to_string()]);
        // The same patterns as a shortcut that starts the disc.
        let shortcut = emulators::identify(a, &install.join("Dolphin.exe"), &list[0].args).unwrap();
        assert_eq!(shortcut.saves(a), list[0].game.saves(a));
    }

    #[test]
    fn cemu_game_list() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ZeldaBW").join("Cemu");
        let mlc = tmp.path().join("ZeldaBW").join("MLC");
        write(&mlc, "usr/save/00050000/101c9300/user/80000001/game_data.sav", b"x");
        write(&mlc, "usr/save/00050000/101c9300/meta/meta.xml", b"<menu><longname_en type=\"string\">The Legend of Zelda\nBreath of the Wild</longname_en></menu>");
        write(&mlc, "usr/save/00050000/10101c00/meta/meta.xml", b"<menu/>");
        let code = tmp.path().join("Games").join("BotW").join("code");
        let rpx = write(&code, "U-King.rpx", b"x");
        let settings = format!(
            "<content><mlc_path>{}</mlc_path><GameCache><Entry><title_id>1407375153861376</title_id><name>Zelda BotW</name><custom_name></custom_name><path>{}</path></Entry></GameCache></content>",
            mlc.display(),
            rpx.display()
        );
        write(&install, "settings.xml", settings.as_bytes());
        let a = adapters::find("cemu").unwrap();
        let list = games(a, Some(&install));
        assert_eq!(list.len(), 1, "a save folder without user data is skipped");
        assert_eq!(list[0].game.id, "00050000101C9300");
        assert_eq!(list[0].game.name.as_deref(), Some("Zelda BotW"));
        assert_eq!(list[0].args, format!("-g \"{}\"", rpx.display()));
        assert_eq!(list[0].game.saves(a), vec!["{mlc}/usr/save/00050000/101c9300/**".to_string()]);
        // Without the game list: the name in the save's meta.xml.
        write(&install, "settings.xml", format!("<content><mlc_path>{}</mlc_path></content>", mlc.display()).as_bytes());
        let list = games(a, Some(&install));
        assert_eq!(list[0].game.name.as_deref(), Some("The Legend of Zelda Breath of the Wild"));
        assert!(list[0].args.is_empty());
    }

    fn sfo(title: &str) -> Vec<u8> {
        // Minimal PARAM.SFO with one TITLE entry.
        let key = b"TITLE\0";
        let mut val = title.as_bytes().to_vec();
        val.push(0);
        let mut b = b"\0PSF".to_vec();
        b.extend_from_slice(&0x0101u32.to_le_bytes());
        b.extend_from_slice(&(0x14u32 + 16).to_le_bytes());
        b.extend_from_slice(&(0x14u32 + 16 + key.len() as u32).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0x0204u16.to_le_bytes());
        b.extend_from_slice(&(val.len() as u32).to_le_bytes());
        b.extend_from_slice(&(val.len() as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(key);
        b.extend_from_slice(&val);
        b
    }

    #[test]
    fn playstation_and_retroarch() {
        let tmp = tempfile::tempdir().unwrap();
        // PPSSPP (portable memstick): two save folders of one game.
        let pp = tmp.path().join("PPSSPP");
        write(&pp, "memstick/PSP/SAVEDATA/ULUS10041DATA00/PARAM.SFO", &sfo("Lord of Arcana"));
        write(&pp, "memstick/PSP/SAVEDATA/ULUS10041DATA01/DATA.BIN", b"x");
        write(&pp, "memstick/PSP/SAVEDATA/SYSTEM/x.bin", b"x");
        let a = adapters::find("ppsspp").unwrap();
        let list = games(a, Some(&pp));
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].game.id.as_str(), list[0].game.name.as_deref()), ("ULUS10041", Some("Lord of Arcana")));
        assert_eq!(list[0].game.saves(a), vec!["{emu}/PSP/SAVEDATA/ULUS10041*/**".to_string()]);

        // RPCS3 is always portable on Windows (on Linux it uses ~/.config/rpcs3).
        if cfg!(windows) {
            let rp = tmp.path().join("rpcs3");
            write(&rp, "dev_hdd0/home/00000001/savedata/BLUS30443-AUTOSAVE/PARAM.SFO", &sfo("Demon's Souls"));
            let list = games(adapters::find("rpcs3").unwrap(), Some(&rp));
            assert_eq!(list[0].game.name.as_deref(), Some("Demon's Souls"));
            assert!(games(adapters::find("rpcs3").unwrap(), None).is_empty(), "no install, no RPCS3 folder");
        }

        // shadPS4 and Vita3K.
        let sh = tmp.path().join("shadPS4");
        write(&sh, "user/savedata/1/CUSA03173/SPRJ0005/sce_sys/param.sfo", &sfo("Bloodborne"));
        let list = games(adapters::find("shadps4").unwrap(), Some(&sh));
        assert_eq!((list[0].game.id.as_str(), list[0].game.name.as_deref()), ("CUSA03173", Some("Bloodborne")));
        let v = tmp.path().join("Vita3K");
        write(&v, "ux0/user/00/savedata/PCSE00491/save.dat", b"x");
        write(&v, "ux0/app/PCSE00491/sce_sys/param.sfo", &sfo("Persona 4 Golden"));
        let list = games(adapters::find("vita3k").unwrap(), Some(&v));
        assert_eq!(list[0].game.name.as_deref(), Some("Persona 4 Golden"));

        // RetroArch: one game per save file name, in core folders too
        // (portable on Windows; ~/.config/retroarch on Linux).
        if !cfg!(windows) {
            return;
        }
        let ra = tmp.path().join("RetroArch");
        write(&ra, "saves/Super Metroid (Japan, USA) [!].srm", b"x");
        write(&ra, "saves/mGBA/Pokemon Emerald.srm", b"x");
        write(&ra, "saves/mGBA/Pokemon Emerald.rtc", b"x");
        let a = adapters::find("retroarch").unwrap();
        let list = games(a, Some(&ra));
        let names: Vec<&str> = list.iter().map(|g| g.game.id.as_str()).collect();
        assert_eq!(names, vec!["Pokemon Emerald", "Super Metroid (Japan, USA) [!]"]);
        assert!(list[1].game.saves(a)[0].contains("[[]![]]"));
    }

    #[test]
    fn three_ds_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let az = tmp.path().join("Azahar");
        write(&az, "user/sdmc/Nintendo 3DS/0000/0000/title/00040000/00055d00/data/00000001/main", b"x");
        // A game whose save folder is empty.
        std::fs::create_dir_all(az.join("user/sdmc/Nintendo 3DS/0000/0000/title/00040000/00030700/data")).unwrap();
        let list = games(adapters::find("azahar").unwrap(), Some(&az));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].game.id, "0004000000055D00");
    }
}
