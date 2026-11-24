//! Which game an emulator shortcut starts, and where that game's saves are.
//!
//! Emulator rules (`adapters/emulators.yaml`) say how a game is identified
//! (`gameId`) and list one game's saves with `<placeholders>`. We read the
//! id from the launch options and the disc image or game folder they point
//! at. When the game can't be identified (or the emulator has no per-game
//! saves), the whole emulator's saves are used instead.
//!
//! | `gameId` | read from | placeholders |
//! |---|---|---|
//! | `gc-wii` | ISO / GCM / RVZ / WIA / WBFS / CISO header | `<id4>` `<maker>` `<id4hex>`; kind `gc` or `wii` |
//! | `wiiu` | `meta/meta.xml` next to the game's `code` folder | `<high>` `<low>` |
//! | `3ds` | .3ds / .cci / .cxi / .cia header | `<high>` `<low>` |
//! | `ps3` | serial in the path, or `PARAM.SFO` | `<id>` |
//! | `ps4` | serial in the path, or `sce_sys/param.sfo` | `<id>` |
//! | `psp` | ISO `UMD_DATA.BIN`, EBOOT.PBP / folder `PARAM.SFO`, serial in the path | `<id>` |
//! | `vita` | `-r <id>`, or serial in the launch options | `<id>` |
//! | `rom` | the content file's name (RetroArch) | `<stem>` |
//! | `switch` | title id in the file name, or the NSP / XCI ([`crate::switch`]) | `<id>` |

use crate::adapters::Adapter;
use crate::switch;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct EmuGame {
    /// What identifies the game, e.g. `GALE01`, `CUSA00207`, `0005000010101C00`.
    pub id: String,
    /// Which `gameSaves` list applies (`default`, `gc`, `wii`).
    pub variant: String,
    /// Placeholder values (already escaped for save patterns).
    pub vars: BTreeMap<String, String>,
    /// The disc image / game folder / file the id came from.
    pub source: Option<PathBuf>,
    /// The game's name, when the emulator or the save tells it.
    pub name: Option<String>,
}

impl EmuGame {
    pub(crate) fn new(id: impl Into<String>, variant: &str, vars: &[(&str, String)], source: Option<&Path>) -> EmuGame {
        EmuGame {
            id: id.into(),
            variant: variant.into(),
            vars: vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            source: source.map(Path::to_path_buf),
            name: None,
        }
    }

    pub fn named(mut self, name: Option<String>) -> EmuGame {
        self.name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        self
    }

    /// This game's save patterns from the rule (empty when the rule has
    /// none for this kind of game).
    pub fn saves(&self, a: &Adapter) -> Vec<String> {
        let list = a.game_saves.get(&self.variant).or_else(|| a.game_saves.get("default"));
        list.map(|l| l.iter().map(|t| fill(t, &self.vars)).collect()).unwrap_or_default()
    }
}

fn fill(template: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("<{k}>"), v);
    }
    out
}

/// Escape glob characters in a value put into a save pattern.
pub fn escape_glob(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '*' | '?' | '[' | ']' | '{' | '}' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

/// The game an emulator shortcut starts, when the rule has per-game saves
/// and the game can be identified.
pub fn identify(a: &Adapter, exe: &Path, args: &str) -> Option<EmuGame> {
    if a.game_saves.is_empty() {
        return None;
    }
    identify_any(a, exe, args).filter(|g| !g.saves(a).is_empty())
}

/// The game an emulator shortcut starts, whether or not the rule has
/// per-game saves (game backups need the disc image either way).
pub fn identify_any(a: &Adapter, exe: &Path, args: &str) -> Option<EmuGame> {
    if !a.is_emulator() {
        return None;
    }
    let toks = tokens(args);
    let paths = existing_paths(exe, &toks);
    let g = match a.game_id.as_deref().unwrap_or_default() {
        "gc-wii" => paths.iter().find_map(|p| gc_wii(p)),
        "wiiu" => paths.iter().find_map(|p| wiiu(p)).or_else(|| hex_id_in(args, "0005").map(|id| wiiu_game(&id, None))),
        "3ds" => paths.iter().find_map(|p| n3ds(p)).or_else(|| hex_id_in(args, "0004").map(|id| n3ds_game(&id, None))),
        "ps3" => paths.iter().find_map(|p| ps3(p)).or_else(|| serial_in(args).map(|s| serial_game(&s, None))),
        "ps4" => paths.iter().find_map(|p| ps4(p)).or_else(|| serial_in(args).map(|s| serial_game(&s, None))),
        "psp" => paths.iter().find_map(|p| psp(p)).or_else(|| serial_in(args).map(|s| serial_game(&s, None))),
        "vita" => vita(&toks),
        "rom" => rom(&toks, &paths),
        "switch" => switch_game(exe, args, &paths),
        _ => None,
    };
    // No way to identify it: the file the launch options name is still the game.
    g.or_else(|| {
        let p = paths.iter().find(|p| !["dll", "cfg", "ini", "json", "exe", "yml", "yaml"].contains(&ext_lower(p).as_str()))?;
        let stem = p.file_stem()?.to_string_lossy().into_owned();
        Some(EmuGame::new(stem.clone(), "default", &[("stem", escape_glob(&stem))], Some(p)))
    })
}

/// Launch options split like a command line; `--opt=value` also yields `value`.
fn tokens(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in crate::util::split_args(args) {
        if let Some((k, v)) = t.split_once('=') {
            if k.starts_with('-') && !v.is_empty() {
                out.push(t.clone());
                out.push(v.to_string());
                continue;
            }
        }
        out.push(t);
    }
    out
}

/// Files and folders named in the launch options, last first (the game is
/// usually the last argument). Relative paths are taken from the exe folder.
fn existing_paths(exe: &Path, toks: &[String]) -> Vec<PathBuf> {
    let dir = exe.parent();
    let mut out = Vec::new();
    for t in toks.iter().rev() {
        if t.starts_with('-') || t.is_empty() {
            continue;
        }
        let p = PathBuf::from(t);
        let p = if p.is_absolute() { p } else { dir.map(|d| d.join(&p)).unwrap_or(p) };
        if p.exists() && !crate::util::same_path(&p.to_string_lossy(), &exe.to_string_lossy()) {
            out.push(p);
        }
    }
    out
}

fn read_at(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    let mut got = 0;
    while got < len {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => return None,
        }
    }
    (got == len).then_some(buf)
}

fn u32_be(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Roots an emulator's own settings decide

/// Roots read from an emulator's settings and data on this PC, overriding
/// the rule's defaults: the MLC folder picked in Cemu's first-run wizard,
/// and Ryujinx's per-PC save folders for the `{nxsave-…}` / `{nxdevice-…}`
/// roots used by `saves`.
pub fn config_roots(a: &Adapter, known: &BTreeMap<String, PathBuf>, saves: &[String]) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    if a.key == "cemu" {
        if let Some(p) = cemu_mlc(known) {
            out.insert("mlc".to_string(), p);
        }
    }
    if a.game_id.as_deref() == Some("switch") {
        if let Some(emu) = known.get("emu") {
            for s in saves {
                let Ok((root, _)) = crate::spec::split_pattern(s) else { continue };
                if let Some((kind, title)) = switch::parse_root(&root) {
                    let p = switch::find_save(emu, kind, title).unwrap_or_else(|| switch::pending_dir(emu, kind, title));
                    out.insert(root, p);
                }
            }
        }
    }
    out
}

/// Before a restore writes to `used` roots: create the emulator saves this
/// PC doesn't have yet (Ryujinx numbers saves per PC). The emulator must be
/// closed for that.
pub fn prepare_restore(a: &Adapter, roots: &mut BTreeMap<String, PathBuf>, used: &[String]) -> anyhow::Result<()> {
    if a.game_id.as_deref() != Some("switch") {
        return Ok(());
    }
    let Some(emu) = roots.get("emu").cloned() else { return Ok(()) };
    for name in used {
        let Some((kind, title)) = switch::parse_root(name) else { continue };
        if roots.get(name).is_some_and(|p| !switch::is_pending(p)) {
            continue;
        }
        if is_running(a) {
            anyhow::bail!(crate::tr!("请先关闭 {}：这台电脑上还没有这个游戏的存档，要在 {} 的存档索引里新建", "Close {} first: this PC has no save for this game yet, and one has to be created in {}'s save index", a.name, a.name));
        }
        let p = switch::create_save(&emu, kind, title)?;
        roots.insert(name.clone(), p);
    }
    Ok(())
}

/// After a restore: Ryujinx keeps a working copy of each save next to the
/// committed one; make it match what was restored.
pub fn finish_restore(a: &Adapter, roots: &BTreeMap<String, PathBuf>) -> anyhow::Result<()> {
    if a.game_id.as_deref() != Some("switch") {
        return Ok(());
    }
    for (name, p) in roots {
        if switch::parse_root(name).is_some() && !switch::is_pending(p) {
            switch::mirror_committed(p)?;
        }
    }
    Ok(())
}

/// Whether the emulator is running (its exe names, without wildcards).
fn is_running(a: &Adapter) -> bool {
    let names: Vec<String> = a
        .matcher
        .iter()
        .flat_map(|m| m.exe.iter())
        .filter(|e| !e.contains('*'))
        .filter_map(|e| Path::new(e).file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    crate::process::is_running(&names)
}

/// Cemu's settings file: in the portable folder, the exe folder (Cemu 1.x)
/// or %APPDATA%\Cemu, whichever exists first.
pub(crate) fn cemu_settings(known: &BTreeMap<String, PathBuf>) -> Option<PathBuf> {
    let mut files = Vec::new();
    if let Some(i) = known.get("install") {
        files.push(i.join("portable").join("settings.xml"));
        files.push(i.join("settings.xml"));
    }
    if let Some(a) = known.get("appdata") {
        files.push(a.join("Cemu").join("settings.xml"));
    }
    files.into_iter().find(|f| f.is_file())
}

/// Cemu's `<mlc_path>` from its settings file.
fn cemu_mlc(known: &BTreeMap<String, PathBuf>) -> Option<PathBuf> {
    let file = cemu_settings(known)?;
    // Not always UTF-8: Cemu writes paths in the local code page.
    let text = String::from_utf8_lossy(&std::fs::read(&file).ok()?).into_owned();
    let start = text.find("<mlc_path>")? + "<mlc_path>".len();
    let end = start + text[start..].find("</mlc_path>")?;
    let v = text[start..end].trim();
    if v.is_empty() {
        return None;
    }
    let p = PathBuf::from(v);
    let p = if p.is_relative() { file.parent()?.join(p) } else { p };
    p.is_dir().then_some(p)
}

// ---------------------------------------------------------------------------
// GameCube / Wii (Dolphin)

/// Game id and kind from a GameCube / Wii disc image.
pub fn gc_wii(path: &Path) -> Option<EmuGame> {
    if !path.is_file() {
        return None;
    }
    let head = read_at(path, 0, 0x60)?;
    let (header_at, forced) = match &head[0..4] {
        b"WBFS" => (0x200, Some("wii")),
        b"RVZ\x01" | b"WIA\x01" => {
            let kind = match u32_be(&head, 0x48) {
                1 => "gc",
                2 => "wii",
                _ => return None,
            };
            (0x58, Some(kind))
        }
        b"CISO" => (0x8000, None),
        _ => (0, None),
    };
    let disc = read_at(path, header_at, 0x20)?;
    let wii_magic = u32_be(&disc, 0x18) == 0x5D1C_9EA3;
    let gc_magic = u32_be(&disc, 0x1C) == 0xC233_9F3D;
    let kind = match forced {
        Some(k) => k,
        None if wii_magic => "wii",
        None if gc_magic => "gc",
        None => return None,
    };
    let id = &disc[0..6];
    if !id.iter().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let id = String::from_utf8_lossy(id).to_string();
    let id4hex: String = id.bytes().take(4).map(|b| format!("{b:02x}")).collect();
    Some(EmuGame::new(
        id.clone(),
        kind,
        &[("id4", id[0..4].to_string()), ("maker", id[4..6].to_string()), ("id4hex", id4hex), ("id", id.clone())],
        Some(path),
    ))
}

// ---------------------------------------------------------------------------
// Wii U (Cemu)

pub(crate) fn wiiu_game(title_id: &str, source: Option<&Path>) -> EmuGame {
    let t = title_id.to_lowercase();
    EmuGame::new(title_id.to_uppercase(), "default", &[("high", t[0..8].to_string()), ("low", t[8..16].to_string())], source)
}

/// Title id from the `meta/meta.xml` of an unpacked Wii U game (given its
/// `.rpx`, its `code` folder, or the game folder).
pub fn wiiu(path: &Path) -> Option<EmuGame> {
    let mut dirs = Vec::new();
    if path.is_file() {
        if let Some(code) = path.parent() {
            dirs.push(code.parent()?.to_path_buf());
        }
    } else {
        dirs.push(path.to_path_buf());
        if let Some(p) = path.parent() {
            dirs.push(p.to_path_buf());
        }
    }
    for d in dirs {
        let Some(meta) = crate::util::find_file_ci(&d.join("meta"), "meta.xml") else { continue };
        let text = String::from_utf8_lossy(&std::fs::read(meta).ok()?).into_owned();
        let start = text.find("<title_id")?;
        let open_end = start + text[start..].find('>')? + 1;
        let close = open_end + text[open_end..].find('<')?;
        let id = text[open_end..close].trim();
        if id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(wiiu_game(id, Some(path)));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 3DS (Azahar / Citra)

pub(crate) fn n3ds_game(title_id: &str, source: Option<&Path>) -> EmuGame {
    let t = title_id.to_lowercase();
    EmuGame::new(title_id.to_uppercase(), "default", &[("high", t[0..8].to_string()), ("low", t[8..16].to_string())], source)
}

/// Title id from a 3DS game file header (.3ds / .cci / .cxi / .cia).
pub fn n3ds(path: &Path) -> Option<EmuGame> {
    if !path.is_file() {
        return None;
    }
    let id = if ext_lower(path) == "cia" {
        cia_title_id(path)?
    } else {
        let h = read_at(path, 0x100, 0x20)?;
        match &h[0..4] {
            b"NCSD" => u64_le(&h, 0x08),
            b"NCCH" => u64_le(&h, 0x18),
            _ => return None,
        }
    };
    (id >> 32 == 0x0004_0000 || id >> 32 == 0x0004_0002).then(|| n3ds_game(&format!("{id:016x}"), Some(path)))
}

/// The title id in a CIA's title metadata.
fn cia_title_id(path: &Path) -> Option<u64> {
    let h = read_at(path, 0, 0x20)?;
    let align = |x: u64| x.div_ceil(64) * 64;
    let (hdr, cert, tik) = (u32_le(&h, 0) as u64, u32_le(&h, 8) as u64, u32_le(&h, 0x0C) as u64);
    let tmd = align(hdr) + align(cert) + align(tik);
    let sig_type = u32_be(&read_at(path, tmd, 4)?, 0);
    let sig_len = match sig_type {
        0x10000 | 0x10003 => 0x200 + 0x3C,
        0x10001 | 0x10004 => 0x100 + 0x3C,
        0x10002 | 0x10005 => 0x3C + 0x40,
        _ => return None,
    };
    let b = read_at(path, tmd + 4 + sig_len + 0x4C, 8)?;
    Some(u64::from_be_bytes(b.try_into().ok()?))
}

// ---------------------------------------------------------------------------
// PlayStation (RPCS3, shadPS4, PPSSPP, Vita3K)

/// `KEY` of a PARAM.SFO.
pub fn sfo_value(sfo: &[u8], key: &str) -> Option<String> {
    if sfo.len() < 0x14 || &sfo[0..4] != b"\0PSF" {
        return None;
    }
    let (keys, data, count) = (u32_le(sfo, 8) as usize, u32_le(sfo, 12) as usize, u32_le(sfo, 16) as usize);
    for i in 0..count.min(256) {
        let e = 0x14 + i * 16;
        if e + 16 > sfo.len() {
            break;
        }
        let k_off = keys + u16::from_le_bytes([sfo[e], sfo[e + 1]]) as usize;
        let len = u32_le(sfo, e + 4) as usize;
        let d_off = data + u32_le(sfo, e + 12) as usize;
        let k_end = sfo.get(k_off..)?.iter().position(|&b| b == 0)? + k_off;
        if &sfo[k_off..k_end] != key.as_bytes() {
            continue;
        }
        let v = sfo.get(d_off..d_off + len)?;
        let v = v.split(|&b| b == 0).next().unwrap_or(v);
        return Some(String::from_utf8_lossy(v).trim().to_string());
    }
    None
}

fn sfo_file_id(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    sfo_value(&bytes, "TITLE_ID").or_else(|| sfo_value(&bytes, "DISC_ID")).map(|s| s.replace('-', ""))
}

/// A serial like `CUSA00207`, `BLUS30443`, `ULUS-10041` in text (the last one).
pub fn serial_in(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let mut found = None;
    let mut i = 0;
    while i + 9 <= b.len() {
        let letters = b[i..i + 4].iter().all(u8::is_ascii_uppercase);
        let before_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if letters && before_ok {
            let mut j = i + 4;
            if j < b.len() && (b[j] == b'-' || b[j] == b'_') {
                j += 1;
            }
            if j + 5 <= b.len() && b[j..j + 5].iter().all(u8::is_ascii_digit) && (j + 5 == b.len() || !b[j + 5].is_ascii_digit()) {
                found = Some(format!("{}{}", &text[i..i + 4], &text[j..j + 5]));
                i = j + 5;
                continue;
            }
        }
        i += 1;
    }
    found
}

pub(crate) fn serial_game(id: &str, source: Option<&Path>) -> EmuGame {
    EmuGame::new(id, "default", &[("id", id.to_string())], source)
}

/// PS3 game: serial in the path, else the PARAM.SFO of the game folder.
pub fn ps3(path: &Path) -> Option<EmuGame> {
    if let Some(s) = serial_in(&path.to_string_lossy()) {
        return Some(serial_game(&s, Some(path)));
    }
    let dir = if path.is_file() { path.parent()? } else { path };
    // EBOOT.BIN lives in <game>/USRDIR; discs keep the game in PS3_GAME.
    let cands = [
        dir.join("PARAM.SFO"),
        dir.join("PS3_GAME").join("PARAM.SFO"),
        dir.parent().map(|p| p.join("PARAM.SFO")).unwrap_or_default(),
    ];
    cands.iter().filter(|p| p.is_file()).find_map(|p| sfo_file_id(p)).map(|id| serial_game(&id, Some(path)))
}

/// PS4 game: serial in the path, else `sce_sys/param.sfo` of the game folder.
pub fn ps4(path: &Path) -> Option<EmuGame> {
    if let Some(s) = serial_in(&path.to_string_lossy()) {
        return Some(serial_game(&s, Some(path)));
    }
    let dir = if path.is_file() { path.parent()? } else { path };
    let sfo = dir.join("sce_sys").join("param.sfo");
    sfo_file_id(&sfo).map(|id| serial_game(&id, Some(path)))
}

/// PSP game: `UMD_DATA.BIN` of an ISO, the PARAM.SFO in an EBOOT.PBP or
/// game folder, else a serial in the path.
pub fn psp(path: &Path) -> Option<EmuGame> {
    let id = match ext_lower(path).as_str() {
        "iso" => iso_root_file(path, "UMD_DATA.BIN")
            .and_then(|b| String::from_utf8_lossy(&b).split('|').next().map(|s| s.trim().replace('-', "")))
            .filter(|s| serial_in(s).is_some()),
        "pbp" => pbp_sfo(path).and_then(|s| sfo_value(&s, "DISC_ID")),
        _ if path.is_dir() => [path.join("PSP_GAME").join("PARAM.SFO"), path.join("PARAM.SFO")]
            .iter()
            .find_map(|p| sfo_file_id(p)),
        _ => None,
    };
    id.or_else(|| serial_in(&path.to_string_lossy())).map(|id| serial_game(&id, Some(path)))
}

fn pbp_sfo(path: &Path) -> Option<Vec<u8>> {
    let h = read_at(path, 0, 0x28)?;
    if &h[0..4] != b"\0PBP" {
        return None;
    }
    let (start, end) = (u32_le(&h, 0x08) as u64, u32_le(&h, 0x0C) as u64);
    (end > start && end - start < 64 * 1024).then(|| read_at(path, start, (end - start) as usize)).flatten()
}

/// A file in the root folder of an ISO 9660 image (first 4 KB).
pub fn iso_root_file(path: &Path, name: &str) -> Option<Vec<u8>> {
    let pvd = read_at(path, 16 * 2048, 2048)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let root = &pvd[156..156 + 34];
    let (lba, size) = (u32_le(root, 2) as u64, u32_le(root, 10) as usize);
    let dir = read_at(path, lba * 2048, size.min(64 * 1024))?;
    let mut i = 0;
    while i < dir.len() {
        let len = dir[i] as usize;
        if len == 0 {
            // Records don't cross sector boundaries.
            i = (i / 2048 + 1) * 2048;
            continue;
        }
        if i + len > dir.len() || len < 34 {
            break;
        }
        let r = &dir[i..i + len];
        let name_len = r[32] as usize;
        if 33 + name_len <= r.len() {
            let n = String::from_utf8_lossy(&r[33..33 + name_len]);
            let n = n.split(';').next().unwrap_or("");
            if n.eq_ignore_ascii_case(name) {
                let (f_lba, f_size) = (u32_le(r, 2) as u64, u32_le(r, 10) as usize);
                return read_at(path, f_lba * 2048, f_size.min(4096));
            }
        }
        i += len;
    }
    None
}

/// Vita3K: `-r <id>` / `--installed-path <id>`, or a serial in the options.
fn vita(toks: &[String]) -> Option<EmuGame> {
    let flag = toks.windows(2).find(|w| w[0] == "-r" || w[0] == "--installed-path").map(|w| w[1].clone());
    flag.filter(|s| serial_in(s).is_some())
        .or_else(|| serial_in(&toks.join(" ")))
        .map(|id| serial_game(&id, None))
}

// ---------------------------------------------------------------------------
// Switch (Ryujinx)

/// Ryujinx's data folders to look in for this exe: portable first.
pub fn ryujinx_dirs(exe: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = exe.parent() {
        let p = d.join("portable");
        if p.is_dir() {
            out.push(p);
        }
    }
    if let Some(a) = dirs::config_dir() {
        out.push(a.join("Ryujinx"));
    }
    out
}

fn switch_game(exe: &Path, args: &str, paths: &[PathBuf]) -> Option<EmuGame> {
    let dirs = ryujinx_dirs(exe);
    let mut keys: Vec<PathBuf> = dirs.iter().map(|d| d.join("system").join("prod.keys")).collect();
    if let Some(h) = dirs::home_dir() {
        keys.push(h.join(".switch").join("prod.keys"));
    }
    let (title, source) = paths
        .iter()
        .find_map(|p| switch::title_id(p, &keys).map(|t| (t, Some(p.as_path()))))
        .or_else(|| switch::title_in_text(args).map(|t| (t, None)))?;
    let name = dirs.iter().find_map(|d| switch::game_name(d, title));
    Some(switch_emu_game(title, source).named(name))
}

pub fn switch_emu_game(title: u64, source: Option<&Path>) -> EmuGame {
    EmuGame::new(format!("{title:016X}"), "default", &[("id", format!("{title:016x}"))], source)
}

// ---------------------------------------------------------------------------
// RetroArch

/// The content file (not the core or a config file) and its name.
fn rom(toks: &[String], paths: &[PathBuf]) -> Option<EmuGame> {
    let skip: Vec<&String> = toks
        .windows(2)
        .filter(|w| ["-L", "--libretro", "-c", "--config", "--appendconfig", "-s", "--save", "-S", "--savestate"].contains(&w[0].as_str()))
        .map(|w| &w[1])
        .collect();
    let content = paths.iter().find(|p| {
        p.is_file()
            && !["dll", "so", "cfg", "ini", "json", "exe"].contains(&ext_lower(p).as_str())
            && !skip.iter().any(|s| p.ends_with(s.as_str()) || crate::util::same_path(&p.to_string_lossy(), s))
    })?;
    let stem = content.file_stem()?.to_string_lossy().into_owned();
    Some(EmuGame::new(stem.clone(), "default", &[("stem", escape_glob(&stem))], Some(content)))
}

/// A 16-digit hex id starting with `prefix` in text (filenames often carry
/// the title id).
pub(crate) fn hex_id_in(text: &str, prefix: &str) -> Option<String> {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(15)).rev().find_map(|i| {
        let w = &b[i..i + 16];
        let edge_ok = (i == 0 || !b[i - 1].is_ascii_hexdigit()) && (i + 16 == b.len() || !b[i + 16].is_ascii_hexdigit());
        (edge_ok && w.iter().all(u8::is_ascii_hexdigit) && text[i..i + 16].to_lowercase().starts_with(prefix))
            .then(|| text[i..i + 16].to_string())
    })
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

    fn gc_header(id: &str, wii: bool) -> Vec<u8> {
        let mut b = vec![0u8; 0x440];
        b[0..6].copy_from_slice(id.as_bytes());
        if wii {
            b[0x18..0x1C].copy_from_slice(&0x5D1C_9EA3u32.to_be_bytes());
        } else {
            b[0x1C..0x20].copy_from_slice(&0xC233_9F3Du32.to_be_bytes());
        }
        b
    }

    fn sfo(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut keys = Vec::new();
        let mut data = Vec::new();
        let mut index = Vec::new();
        for (k, v) in pairs {
            let k_off = keys.len() as u16;
            keys.extend_from_slice(k.as_bytes());
            keys.push(0);
            let d_off = data.len() as u32;
            let mut val = v.as_bytes().to_vec();
            val.push(0);
            let len = val.len() as u32;
            data.extend_from_slice(&val);
            index.extend_from_slice(&k_off.to_le_bytes());
            index.extend_from_slice(&0x0204u16.to_le_bytes());
            index.extend_from_slice(&len.to_le_bytes());
            index.extend_from_slice(&len.to_le_bytes());
            index.extend_from_slice(&d_off.to_le_bytes());
        }
        let key_start = 0x14 + index.len() as u32;
        let data_start = key_start + keys.len() as u32;
        let mut out = b"\0PSF".to_vec();
        out.extend_from_slice(&0x0101u32.to_le_bytes());
        out.extend_from_slice(&key_start.to_le_bytes());
        out.extend_from_slice(&data_start.to_le_bytes());
        out.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        out.extend(index);
        out.extend(keys);
        out.extend(data);
        out
    }

    /// A tiny ISO 9660 image with one file in its root folder.
    fn iso_with(name: &str, content: &[u8]) -> Vec<u8> {
        let mut img = vec![0u8; 2048 * 20];
        let pvd = 16 * 2048;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let root = pvd + 156;
        img[root] = 34;
        img[root + 2..root + 6].copy_from_slice(&18u32.to_le_bytes());
        img[root + 10..root + 14].copy_from_slice(&2048u32.to_le_bytes());
        let dir = 18 * 2048;
        let fname = format!("{name};1");
        let rec_len = 33 + fname.len() + (33 + fname.len()) % 2;
        img[dir] = rec_len as u8;
        img[dir + 2..dir + 6].copy_from_slice(&19u32.to_le_bytes());
        img[dir + 10..dir + 14].copy_from_slice(&(content.len() as u32).to_le_bytes());
        img[dir + 32] = fname.len() as u8;
        img[dir + 33..dir + 33 + fname.len()].copy_from_slice(fname.as_bytes());
        img[19 * 2048..19 * 2048 + content.len()].copy_from_slice(content);
        img
    }

    #[test]
    fn reads_gamecube_and_wii_discs() {
        let tmp = tempfile::tempdir().unwrap();
        let gc = gc_wii(&write(tmp.path(), "Melee.iso", &gc_header("GALE01", false))).unwrap();
        assert_eq!((gc.id.as_str(), gc.variant.as_str()), ("GALE01", "gc"));
        assert_eq!(gc.vars["maker"], "01");
        let wii = gc_wii(&write(tmp.path(), "Galaxy.iso", &gc_header("RMGE01", true))).unwrap();
        assert_eq!(wii.variant, "wii");
        assert_eq!(wii.vars["id4hex"], "524d4745");
        // RVZ: the disc header is copied into the file header.
        let mut rvz = vec![0u8; 0x100];
        rvz[0..4].copy_from_slice(b"RVZ\x01");
        rvz[0x48..0x4C].copy_from_slice(&2u32.to_be_bytes());
        rvz[0x58..0x58 + 0x20].copy_from_slice(&gc_header("SOUE01", true)[0..0x20]);
        assert_eq!(gc_wii(&write(tmp.path(), "Skyward.rvz", &rvz)).unwrap().id, "SOUE01");
        // WBFS: the disc header is in the second sector.
        let mut wbfs = vec![0u8; 0x400];
        wbfs[0..4].copy_from_slice(b"WBFS");
        wbfs[0x200..0x220].copy_from_slice(&gc_header("RSBE01", true)[0..0x20]);
        assert_eq!(gc_wii(&write(tmp.path(), "Brawl.wbfs", &wbfs)).unwrap().id, "RSBE01");
        assert!(gc_wii(&write(tmp.path(), "random.bin", &[7u8; 0x400])).is_none());
    }

    #[test]
    fn dolphin_saves_per_game() {
        let tmp = tempfile::tempdir().unwrap();
        let disc = write(tmp.path(), "ROMs/Super Mario Galaxy.rvz", &{
            let mut r = vec![0u8; 0x100];
            r[0..4].copy_from_slice(b"RVZ\x01");
            r[0x48..0x4C].copy_from_slice(&2u32.to_be_bytes());
            r[0x58..0x78].copy_from_slice(&gc_header("RMGE01", true)[0..0x20]);
            r
        });
        let melee = write(tmp.path(), "ROMs/Melee.iso", &gc_header("GALE01", false));
        let a = adapters::find("dolphin").unwrap();
        let exe = tmp.path().join("Dolphin").join("Dolphin.exe");
        let g = identify(a, &exe, &format!("-b -e \"{}\"", disc.display())).unwrap();
        assert_eq!(g.saves(a), vec!["{emu}/Wii/title/0001000?/524d4745/data/**".to_string()]);
        let g = identify(a, &exe, &format!("--exec=\"{}\" --batch", melee.display())).unwrap();
        assert!(g.saves(a).iter().any(|s| s.ends_with("01-GALE-*.gci")), "{:?}", g.saves(a));
        // The emulator on its own: no game.
        assert!(identify(a, &exe, "").is_none());
    }

    #[test]
    fn reads_param_sfo_and_serials() {
        let s = sfo(&[("CATEGORY", "DG"), ("TITLE_ID", "BLUS30443"), ("TITLE", "Demon's Souls")]);
        assert_eq!(sfo_value(&s, "TITLE_ID").as_deref(), Some("BLUS30443"));
        assert_eq!(sfo_value(&s, "TITLE").as_deref(), Some("Demon's Souls"));
        assert_eq!(sfo_value(&s, "NOPE"), None);
        assert_eq!(serial_in(r"D:\PS4\CUSA00207\eboot.bin").as_deref(), Some("CUSA00207"));
        assert_eq!(serial_in("Crisis Core [ULUS-10336].iso").as_deref(), Some("ULUS10336"));
        assert_eq!(serial_in("XCUSA00207 CUSA0020"), None);

        let tmp = tempfile::tempdir().unwrap();
        // RPCS3: a disc game folder named by the player.
        write(tmp.path(), "Demons Souls/PS3_GAME/PARAM.SFO", &s);
        let eboot = write(tmp.path(), "Demons Souls/PS3_GAME/USRDIR/EBOOT.BIN", b"x");
        assert_eq!(ps3(&eboot).unwrap().id, "BLUS30443");
        // shadPS4: folder without the serial in its name.
        write(tmp.path(), "Bloodborne/sce_sys/param.sfo", &sfo(&[("TITLE_ID", "CUSA03173")]));
        let bb = write(tmp.path(), "Bloodborne/eboot.bin", b"x");
        assert_eq!(ps4(&bb).unwrap().id, "CUSA03173");
    }

    #[test]
    fn reads_psp_isos_and_eboots() {
        let tmp = tempfile::tempdir().unwrap();
        let iso = write(tmp.path(), "game.iso", &iso_with("UMD_DATA.BIN", b"ULUS-10041|0000000000000001|0001|G"));
        assert_eq!(psp(&iso).unwrap().id, "ULUS10041");
        let s = sfo(&[("DISC_ID", "NPUH10001")]);
        let mut pbp = b"\0PBP".to_vec();
        pbp.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        pbp.extend_from_slice(&0x28u32.to_le_bytes());
        pbp.extend_from_slice(&(0x28 + s.len() as u32).to_le_bytes());
        pbp.resize(0x28, 0);
        pbp.extend_from_slice(&s);
        let eboot = write(tmp.path(), "PSP/GAME/X/EBOOT.PBP", &pbp);
        assert_eq!(psp(&eboot).unwrap().id, "NPUH10001");
        let a = adapters::find("ppsspp").unwrap();
        let g = identify(a, Path::new(r"C:\PPSSPP\PPSSPPWindows64.exe"), &format!("\"{}\"", iso.display())).unwrap();
        assert_eq!(g.saves(a), vec!["{emu}/PSP/SAVEDATA/ULUS10041*/**".to_string()]);
    }

    #[test]
    fn reads_wiiu_and_3ds_ids() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "BotW/meta/meta.xml", br#"<?xml version="1.0"?><menu><title_id type="hexBinary" length="8">00050000101C9400</title_id></menu>"#);
        let rpx = write(tmp.path(), "BotW/code/U-King.rpx", b"x");
        let g = wiiu(&rpx).unwrap();
        assert_eq!((g.vars["high"].as_str(), g.vars["low"].as_str()), ("00050000", "101c9400"));
        let a = adapters::find("cemu").unwrap();
        let g = identify(a, &tmp.path().join("Cemu").join("Cemu.exe"), &format!("-f -g \"{}\"", rpx.display())).unwrap();
        assert_eq!(g.saves(a), vec!["{mlc}/usr/save/00050000/101c9400/**".to_string()]);

        let mut ncsd = vec![0u8; 0x200];
        ncsd[0x100..0x104].copy_from_slice(b"NCSD");
        ncsd[0x108..0x110].copy_from_slice(&0x0004_0000_0008_C300u64.to_le_bytes());
        let g = n3ds(&write(tmp.path(), "Smash.3ds", &ncsd)).unwrap();
        assert_eq!(g.vars["low"], "0008c300");
        assert!(n3ds(&write(tmp.path(), "junk.3ds", &[0u8; 0x200])).is_none());
    }

    #[test]
    fn cemu_mlc_from_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp.path().join("ZeldaBW").join("Cemu");
        let mlc = tmp.path().join("ZeldaBW").join("MLC");
        std::fs::create_dir_all(&mlc).unwrap();
        let mut known = BTreeMap::new();
        known.insert("install".to_string(), install.clone());
        known.insert("appdata".to_string(), tmp.path().join("Roaming"));
        let a = adapters::find("cemu").unwrap();
        assert!(config_roots(a, &known, &[]).is_empty());
        // With a non-UTF-8 game path elsewhere in the file (local code page).
        let mut xml = format!("<content><mlc_path>{}</mlc_path><path>", mlc.display()).into_bytes();
        xml.extend_from_slice(&[0xD5, 0xFB, 0xBA, 0xCF]);
        xml.extend_from_slice(b"</path></content>");
        write(&install, "settings.xml", &xml);
        assert_eq!(config_roots(a, &known, &[])["mlc"], mlc);
        // Empty or missing folders fall back to the rule's candidates.
        write(&install, "settings.xml", b"<content><mlc_path></mlc_path></content>");
        assert!(config_roots(a, &known, &[]).is_empty());
    }

    #[test]
    fn vita_and_retroarch() {
        let a = adapters::find("vita3k").unwrap();
        let g = identify(a, Path::new(r"C:\Vita3K\Vita3K.exe"), "-F -r PCSE00491").unwrap();
        assert_eq!(g.saves(a), vec!["{ux0}/user/00/savedata/PCSE00491/**".to_string()]);

        let tmp = tempfile::tempdir().unwrap();
        let core = write(tmp.path(), "RetroArch/cores/snes9x_libretro.dll", b"x");
        let rom = write(tmp.path(), "ROMs/Super Metroid (Japan, USA) [!].sfc", b"x");
        let a = adapters::find("retroarch").unwrap();
        let exe = tmp.path().join("RetroArch").join("retroarch.exe");
        let g = identify(a, &exe, &format!("-L \"{}\" \"{}\"", core.display(), rom.display())).unwrap();
        assert_eq!(g.id, "Super Metroid (Japan, USA) [!]");
        let pat = &g.saves(a)[0];
        assert!(pat.contains("Super Metroid (Japan, USA) [[]![]]"), "{pat}");
        // The escaped name matches the real save file.
        let (_, rest) = crate::spec::split_pattern(pat).unwrap();
        let glob = globset::GlobBuilder::new(&rest).literal_separator(true).build().unwrap().compile_matcher();
        assert!(glob.is_match("saves/Super Metroid (Japan, USA) [!].srm"));
        assert!(glob.is_match("saves/Snes9x/Super Metroid (Japan, USA) [!].srm"));
        assert!(!glob.is_match("saves/Super Metroid (Japan, USA) X.srm"));
    }

    #[test]
    fn hex_ids_in_names() {
        assert_eq!(hex_id_in("Game [0005000010101C00].wua", "0005").as_deref(), Some("0005000010101C00"));
        assert_eq!(hex_id_in("x 00040000000EDF00 y", "0004").as_deref(), Some("00040000000EDF00"));
        assert_eq!(hex_id_in("00050000101C94001", "0005"), None);
    }
}
