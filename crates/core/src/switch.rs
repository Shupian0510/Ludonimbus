//! Nintendo Switch games in Ryujinx and the yuzu family (Eden, Citron,
//! Sudachi, Suyu, yuzu).
//!
//! **Which game.** Most dump file names carry the title id
//! (`… [0100770008DD8000][v0].nsp`). Otherwise it is read from the NSP (its
//! `.cnmt.xml` or ticket name) or from the NCA headers inside an NSP / XCI,
//! which are encrypted with the `header_key` in the user's own `prod.keys`
//! (the emulator needs that file anyway; it is only read, never synced).
//!
//! **Where its saves are.** Ryujinx numbers saves in the order they are
//! created on each PC (`bis/user/save/0000000000000001`, …), keeps the
//! numbers in a save index (`bis/system/save/8000000000000000/0/imkvdb.arc`),
//! and an account save belongs to one user profile. Per-game rules use two
//! roots looked up on each PC: `{nxsave-<title id>}` is the current
//! profile's save and `{nxdevice-<title id>}` the game's device save (both
//! the save's committed folder `0`). Before a restore, a save this PC doesn't
//! have yet is created the way Ryujinx creates one: an index entry, the
//! numbered folder and its extra data.
//!
//! The yuzu family keeps saves at fixed paths instead (see
//! [`yuzu_save`]), so the same two roots work there too: a snapshot made
//! in Ryujinx restores into Eden and back, on the same PC or another one.

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// A user profile's save.
    Account,
    /// Shared by all profiles on the console.
    Device,
}

impl SaveKind {
    fn code(self) -> u8 {
        match self {
            SaveKind::Account => 1,
            SaveKind::Device => 3,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            SaveKind::Account => "nxsave-",
            SaveKind::Device => "nxdevice-",
        }
    }
}

/// The root name of one game's save, e.g. `nxsave-0100770008dd8000`.
pub fn root_name(kind: SaveKind, title: u64) -> String {
    format!("{}{title:016x}", kind.prefix())
}

/// `nxsave-<title id>` / `nxdevice-<title id>`.
pub fn parse_root(name: &str) -> Option<(SaveKind, u64)> {
    let (kind, hex) = match name.strip_prefix("nxsave-") {
        Some(h) => (SaveKind::Account, h),
        None => (SaveKind::Device, name.strip_prefix("nxdevice-")?),
    };
    if hex.len() != 16 {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(|t| (kind, t))
}

/// A root in a rule's template (`<id>` not filled in yet).
pub fn is_root_template(name: &str) -> bool {
    name.starts_with("nxsave-") || name.starts_with("nxdevice-")
}

/// Updates (`…800`) and DLC (`…1xxx`) use the base game's saves.
pub fn base_title(id: u64) -> u64 {
    id & !0x1FFF
}

/// A game's title id (system titles are below `0100000000010000`).
fn is_app_title(id: u64) -> bool {
    id >> 56 == 0x01 && id >= 0x0100_0000_0001_0000
}

/// A title id in text, e.g. a file name.
pub fn title_in_text(text: &str) -> Option<u64> {
    crate::emulators::hex_id_in(text, "01")
        .and_then(|h| u64::from_str_radix(&h, 16).ok())
        .filter(|&t| is_app_title(t))
        .map(base_title)
}

// ---------------------------------------------------------------------------
// Title id from a game file

fn read_at(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}
fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// The game's title id from an NSP / NSZ / XCI / XCZ (or its file name).
/// `key_files` are `prod.keys` files to try for encrypted NCA headers.
pub fn title_id(path: &Path, key_files: &[PathBuf]) -> Option<u64> {
    if let Some(t) = path.file_name().and_then(|n| title_in_text(&n.to_string_lossy())) {
        return Some(t);
    }
    if !path.is_file() {
        return None;
    }
    let files = pfs_entries(path, 0).or_else(|| xci_secure(path))?;
    for f in &files {
        let n = f.name.to_lowercase();
        if n.ends_with(".cnmt.xml") && f.size < 1 << 20 {
            if let Some(t) = read_at(path, f.offset, f.size as usize).and_then(|b| cnmt_xml_id(&String::from_utf8_lossy(&b))) {
                return Some(t);
            }
        }
        // A ticket is named after its rights id: the title id, then the key generation.
        if n.ends_with(".tik") && n.len() >= 36 && n[..32].bytes().all(|c| c.is_ascii_hexdigit()) {
            if let Some(t) = u64::from_str_radix(&n[..16], 16).ok().filter(|&t| is_app_title(t)) {
                return Some(base_title(t));
            }
        }
    }
    let key = header_key(key_files)?;
    nca_title(path, &files, &key)
}

/// Program ids in the NCA headers, biggest NCA (the game itself) first.
fn nca_title(path: &Path, files: &[Entry], key: &[u8; 32]) -> Option<u64> {
    let mut ncas: Vec<&Entry> = files
        .iter()
        .filter(|f| {
            let n = f.name.to_lowercase();
            n.ends_with(".nca") || n.ends_with(".ncz")
        })
        .collect();
    ncas.sort_by_key(|f| std::cmp::Reverse(f.size));
    ncas.iter()
        .take(8)
        .find_map(|f| nca_program_id(path, f.offset, key).filter(|&t| is_app_title(t)))
        .map(base_title)
}

fn cnmt_xml_id(xml: &str) -> Option<u64> {
    let start = xml.find("<Id>")? + 4;
    let end = start + xml[start..].find("</Id>")?;
    let v = xml[start..end].trim();
    let v = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")).unwrap_or(v);
    u64::from_str_radix(v, 16).ok().filter(|&t| is_app_title(t)).map(base_title)
}

#[derive(Debug)]
struct Entry {
    name: String,
    offset: u64,
    size: u64,
}

/// Files in a PFS0 (NSP) or HFS0 (XCI partition) at `at`.
fn pfs_entries(path: &Path, at: u64) -> Option<Vec<Entry>> {
    let h = read_at(path, at, 0x10)?;
    let entry_size = match &h[0..4] {
        b"PFS0" => 0x18,
        b"HFS0" => 0x40,
        _ => return None,
    };
    let (n, strings) = (u32_le(&h, 4) as usize, u32_le(&h, 8) as usize);
    if n == 0 || n > 4096 || strings > 1 << 20 {
        return None;
    }
    let table = read_at(path, at + 0x10, n * entry_size + strings)?;
    let names = &table[n * entry_size..];
    let data = at + 0x10 + (n * entry_size + strings) as u64;
    (0..n)
        .map(|i| {
            let e = i * entry_size;
            let name_at = u32_le(&table, e + 16) as usize;
            let raw = names.get(name_at..)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Some(Entry {
                name: String::from_utf8_lossy(&raw[..end]).into_owned(),
                offset: data + u64_le(&table, e),
                size: u64_le(&table, e + 8),
            })
        })
        .collect()
}

/// The files of an XCI's secure partition (where the game's NCAs are). Some
/// dumps start with a 4 KB key area before the card header.
fn xci_secure(path: &Path) -> Option<Vec<Entry>> {
    for base in [0u64, 0x1000] {
        let Some(h) = read_at(path, base + 0x100, 0x40) else { continue };
        if &h[0..4] != b"HEAD" {
            continue;
        }
        let parts = pfs_entries(path, base + u64_le(&h, 0x30))?;
        let secure = parts.iter().find(|p| p.name == "secure")?;
        return pfs_entries(path, secure.offset);
    }
    None
}

/// The program id in an NCA header (NCZ keeps the header as is).
fn nca_program_id(path: &Path, offset: u64, key: &[u8; 32]) -> Option<u64> {
    let mut b = read_at(path, offset + 0x200, 0x200)?;
    xts(key, 1, &mut b, false);
    (&b[0..3] == b"NCA").then(|| u64_le(&b, 0x10))
}

/// `header_key` from the first `prod.keys` that has it.
pub fn header_key(files: &[PathBuf]) -> Option<[u8; 32]> {
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).find_map(|text| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            if !k.trim().eq_ignore_ascii_case("header_key") {
                return None;
            }
            let v = v.trim();
            if v.len() != 64 {
                return None;
            }
            let mut out = [0u8; 32];
            for (i, b) in out.iter_mut().enumerate() {
                *b = u8::from_str_radix(v.get(i * 2..i * 2 + 2)?, 16).ok()?;
            }
            Some(out)
        })
    })
}

/// AES-128-XTS on one 0x200-byte sector, with the sector number as a
/// big-endian tweak (Nintendo's variant).
fn xts(key: &[u8; 32], sector: u128, data: &mut [u8], encrypt: bool) {
    let k1 = Aes128::new(GenericArray::from_slice(&key[..16]));
    let k2 = Aes128::new(GenericArray::from_slice(&key[16..]));
    let mut t = GenericArray::clone_from_slice(&sector.to_be_bytes());
    k2.encrypt_block(&mut t);
    for block in data.as_chunks_mut::<16>().0 {
        block.iter_mut().zip(t.iter()).for_each(|(b, x)| *b ^= x);
        let b = GenericArray::from_mut_slice(block);
        if encrypt {
            k1.encrypt_block(b);
        } else {
            k1.decrypt_block(b);
        }
        block.iter_mut().zip(t.iter()).for_each(|(b, x)| *b ^= x);
        // t *= x in GF(2^128), little-endian bytes.
        let mut carry = 0u8;
        for byte in t.iter_mut() {
            let next = *byte >> 7;
            *byte = (*byte << 1) | carry;
            carry = next;
        }
        if carry != 0 {
            t[0] ^= 0x87;
        }
    }
}

// ---------------------------------------------------------------------------
// Ryujinx's save index

const INDEX_DIR: [&str; 4] = ["bis", "system", "save", "8000000000000000"];
/// Sizes recorded for a new save when no other save of the game shows them;
/// big enough that Ryujinx never needs to enlarge it.
const DEFAULT_SIZE: u64 = 0x4000_0000;

fn index_dir(emu: &Path) -> PathBuf {
    INDEX_DIR.iter().fold(emu.to_path_buf(), |p, c| p.join(c))
}

fn saves_dir(emu: &Path) -> PathBuf {
    emu.join("bis").join("user").join("save")
}

fn save_dir(emu: &Path, id: u64) -> PathBuf {
    saves_dir(emu).join(format!("{id:016x}"))
}

/// One entry of `imkvdb.arc`: the save's attribute and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexEntry {
    key: [u8; 0x40],
    value: [u8; 0x40],
}

impl IndexEntry {
    fn new(title: u64, kind: SaveKind, user: (u64, u64), id: u64) -> IndexEntry {
        let mut e = IndexEntry { key: [0; 0x40], value: [0; 0x40] };
        e.key[0..8].copy_from_slice(&title.to_le_bytes());
        e.key[8..16].copy_from_slice(&user.0.to_le_bytes());
        e.key[16..24].copy_from_slice(&user.1.to_le_bytes());
        e.key[0x20] = kind.code();
        e.value[0..8].copy_from_slice(&id.to_le_bytes());
        // Space: user storage.
        e.value[0x18] = 1;
        e
    }
    fn program(&self) -> u64 {
        u64_le(&self.key, 0)
    }
    fn user(&self) -> (u64, u64) {
        (u64_le(&self.key, 8), u64_le(&self.key, 0x10))
    }
    fn kind(&self) -> u8 {
        self.key[0x20]
    }
    /// No static save id, rank or index (an ordinary game save).
    fn plain(&self) -> bool {
        u64_le(&self.key, 0x18) == 0 && self.key[0x21..0x24].iter().all(|&b| b == 0)
    }
    fn save_id(&self) -> u64 {
        u64_le(&self.value, 0)
    }
    fn in_user_space(&self) -> bool {
        self.value[0x18] == 1
    }
}

/// Parse `imkvdb.arc` ("IMKV" header, then "IMEN" entries).
pub fn parse_index(b: &[u8]) -> Result<Vec<IndexEntry>> {
    if b.len() < 12 || &b[0..4] != b"IMKV" {
        bail!(crate::tr!("Ryujinx 的存档索引格式不认识", "Ryujinx's save index format isn't recognized"));
    }
    let n = u32_le(b, 8) as usize;
    // No more than the file has room for, whatever its header says.
    let mut out = Vec::with_capacity(n.min((b.len() - 12) / (12 + 0x80)));
    let mut at = 12;
    for _ in 0..n {
        let ok = b.len() >= at + 12 + 0x80
            && &b[at..at + 4] == b"IMEN"
            && u32_le(b, at + 4) == 0x40
            && u32_le(b, at + 8) == 0x40;
        if !ok {
            bail!(crate::tr!("Ryujinx 的存档索引格式不认识", "Ryujinx's save index format isn't recognized"));
        }
        let mut e = IndexEntry { key: [0; 0x40], value: [0; 0x40] };
        e.key.copy_from_slice(&b[at + 12..at + 12 + 0x40]);
        e.value.copy_from_slice(&b[at + 12 + 0x40..at + 12 + 0x80]);
        out.push(e);
        at += 12 + 0x80;
    }
    Ok(out)
}

pub fn write_index(entries: &[IndexEntry]) -> Vec<u8> {
    let mut out = b"IMKV".to_vec();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(b"IMEN");
        out.extend_from_slice(&0x40u32.to_le_bytes());
        out.extend_from_slice(&0x40u32.to_le_bytes());
        out.extend_from_slice(&e.key);
        out.extend_from_slice(&e.value);
    }
    out
}

fn read_index(emu: &Path) -> Result<Vec<IndexEntry>> {
    let p = index_dir(emu).join("0").join("imkvdb.arc");
    parse_index(&std::fs::read(&p).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", p.display()))?)
}

/// The profile Ryujinx opened last (`system/Profiles.json`); the default
/// profile when there is none.
pub fn current_user(emu: &Path) -> (u64, u64) {
    let v = std::fs::read_to_string(emu.join("system").join("Profiles.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let id = v.as_ref().and_then(|v| {
        v["last_opened"]
            .as_str()
            .filter(|s| parse_user(s).is_some())
            .or_else(|| v["profiles"][0]["user_id"].as_str())
            .map(str::to_string)
    });
    id.as_deref().and_then(parse_user).unwrap_or((1, 0))
}

/// Ryujinx writes user ids as 32 hex digits: the high half, then the low half.
fn parse_user(s: &str) -> Option<(u64, u64)> {
    if s.len() != 32 {
        return None;
    }
    Some((u64::from_str_radix(&s[..16], 16).ok()?, u64::from_str_radix(&s[16..], 16).ok()?))
}

fn wanted_user(emu: &Path, kind: SaveKind) -> (u64, u64) {
    match kind {
        SaveKind::Account => current_user(emu),
        SaveKind::Device => (0, 0),
    }
}

fn find_entry(entries: &[IndexEntry], title: u64, kind: SaveKind, user: (u64, u64)) -> Option<&IndexEntry> {
    entries
        .iter()
        .find(|e| e.in_user_space() && e.plain() && e.kind() == kind.code() && e.program() == title && e.user() == user)
}

/// The committed folder of a game's save on this PC, if Ryujinx has made it.
pub fn find_save(emu: &Path, kind: SaveKind, title: u64) -> Option<PathBuf> {
    let entries = read_index(emu).ok()?;
    let e = find_entry(&entries, title, kind, wanted_user(emu, kind))?;
    let dir = save_dir(emu, e.save_id());
    dir.is_dir().then(|| dir.join("0"))
}

/// Stands for a save this PC doesn't have yet: scans find nothing there, and
/// a restore creates the real save first ([`create_save`]).
pub fn pending_dir(emu: &Path, kind: SaveKind, title: u64) -> PathBuf {
    saves_dir(emu).join(format!("ludonimbus-new-{}", root_name(kind, title))).join("0")
}

pub fn is_pending(p: &Path) -> bool {
    p.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n.to_string_lossy().starts_with("ludonimbus-new-"))
}

/// Create a game's save the way Ryujinx does (index entry, numbered folder,
/// extra data) and return its committed folder. Ryujinx must be closed: it
/// keeps the index in memory while it runs. The old index is copied to
/// Ludonimbus's local backups first.
pub fn create_save(emu: &Path, kind: SaveKind, title: u64) -> Result<PathBuf> {
    create_save_with_backup(emu, kind, title, &crate::paths::local_backups_dir().join("ryujinx-save-index"))
}

fn create_save_with_backup(emu: &Path, kind: SaveKind, title: u64, backups: &Path) -> Result<PathBuf> {
    let idx = index_dir(emu);
    let arc = idx.join("0").join("imkvdb.arc");
    if !arc.is_file() {
        bail!(crate::tr!("Ryujinx 还没有建好存档索引（{}）。请先打开一次 Ryujinx，再同步", "Ryujinx hasn't created its save index yet ({}). Open Ryujinx once, then sync", arc.display()));
    }
    let mut entries = parse_index(&std::fs::read(&arc)?)?;
    let user = wanted_user(emu, kind);
    let id = match find_entry(&entries, title, kind, user) {
        Some(e) => e.save_id(),
        None => {
            let last = std::fs::read(idx.join("0").join("lastPublishedId"))
                .ok()
                .filter(|b| b.len() >= 8)
                .map(|b| u64_le(&b, 0))
                .unwrap_or(0);
            let max_dir = std::fs::read_dir(saves_dir(emu))
                .map(|rd| {
                    rd.flatten()
                        .filter_map(|e| {
                            let n = e.file_name().to_string_lossy().into_owned();
                            (n.len() == 16).then(|| u64::from_str_radix(&n, 16).ok()).flatten()
                        })
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let max_index = entries.iter().filter(|e| e.in_user_space()).map(IndexEntry::save_id).max().unwrap_or(0);
            let id = last.max(max_dir).max(max_index) + 1;
            backup_index(&idx, backups)?;
            entries.push(IndexEntry::new(title, kind, user, id));
            let bytes = write_index(&entries);
            // Both copies of the index save: the committed one and the working one.
            for d in [idx.join("0"), idx.join("1")] {
                if d.is_dir() {
                    write_atomic(&d.join("imkvdb.arc"), &bytes)?;
                    write_atomic(&d.join("lastPublishedId"), &id.to_le_bytes())?;
                }
            }
            id
        }
    };
    let dir = save_dir(emu, id);
    std::fs::create_dir_all(dir.join("0"))?;
    std::fs::create_dir_all(dir.join("1"))?;
    if !dir.join("ExtraData0").is_file() {
        let (data, journal) = sizes_like(emu, &entries, title).unwrap_or((DEFAULT_SIZE, DEFAULT_SIZE));
        let extra = extra_data(&IndexEntry::new(title, kind, user, id), data, journal);
        write_atomic(&dir.join("ExtraData0"), &extra)?;
        write_atomic(&dir.join("ExtraData1"), &extra)?;
    }
    Ok(dir.join("0"))
}

/// The save sizes Ryujinx recorded for another save of the same game.
fn sizes_like(emu: &Path, entries: &[IndexEntry], title: u64) -> Option<(u64, u64)> {
    entries.iter().filter(|e| e.in_user_space() && e.program() == title).find_map(|e| {
        let b = std::fs::read(save_dir(emu, e.save_id()).join("ExtraData0")).ok()?;
        (b.len() >= 0x70).then(|| (u64_le(&b, 0x58), u64_le(&b, 0x60))).filter(|&(d, j)| d > 0 && j > 0)
    })
}

/// A save's extra data (0x200 bytes): its attribute, owner, sizes and a
/// commit id.
fn extra_data(e: &IndexEntry, data: u64, journal: u64) -> Vec<u8> {
    let mut b = vec![0u8; 0x200];
    b[0..0x40].copy_from_slice(&e.key);
    b[0x40..0x48].copy_from_slice(&e.program().to_le_bytes());
    b[0x58..0x60].copy_from_slice(&data.to_le_bytes());
    b[0x60..0x68].copy_from_slice(&journal.to_le_bytes());
    b[0x68..0x70].copy_from_slice(&rand::random::<u64>().to_le_bytes());
    b
}

fn backup_index(idx: &Path, backups: &Path) -> Result<()> {
    let dir = backups.join(chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string());
    std::fs::create_dir_all(&dir)?;
    for f in ["imkvdb.arc", "lastPublishedId"] {
        let src = idx.join("0").join(f);
        if src.is_file() {
            std::fs::copy(&src, dir.join(f)).with_context(|| crate::tr!("无法备份 {}", "Can't back up {}", src.display()))?;
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("lntmp");
    std::fs::write(&tmp, bytes).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", path.display()))?;
    Ok(())
}

/// After a restore into a save's committed folder (`0`), make its working
/// copy (`1`) the same, so Ryujinx sees the restored files whichever copy
/// it opens from.
pub fn mirror_committed(committed: &Path) -> Result<()> {
    let Some(save) = committed.parent() else { return Ok(()) };
    let numbered = save
        .file_name()
        .is_some_and(|n| n.len() == 16 && n.to_string_lossy().bytes().all(|c| c.is_ascii_hexdigit()));
    if !numbered || committed.file_name().is_none_or(|n| n != "0") || !committed.is_dir() {
        return Ok(());
    }
    let work = save.join("1");
    if work.exists() {
        std::fs::remove_dir_all(&work).with_context(|| crate::tr!("无法更新 {}", "Can't update {}", work.display()))?;
    }
    for e in walkdir::WalkDir::new(committed).into_iter().flatten() {
        let Ok(rel) = e.path().strip_prefix(committed) else { continue };
        let dest = work.join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if e.file_type().is_file() {
            std::fs::copy(e.path(), &dest).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", dest.display()))?;
        }
    }
    Ok(())
}

/// A game save Ryujinx has on this PC (the current profile's, or a device save).
#[derive(Debug, Clone, PartialEq)]
pub struct NxSave {
    pub title: u64,
    pub kind: SaveKind,
    pub dir: PathBuf,
}

/// The game saves on this PC that per-game sync would use.
pub fn saves(emu: &Path) -> Vec<NxSave> {
    let Ok(entries) = read_index(emu) else { return Vec::new() };
    let user = current_user(emu);
    entries
        .iter()
        .filter(|e| e.in_user_space() && e.plain() && is_app_title(e.program()))
        .filter_map(|e| {
            let kind = match e.kind() {
                1 if e.user() == user => SaveKind::Account,
                3 => SaveKind::Device,
                _ => return None,
            };
            let dir = save_dir(emu, e.save_id()).join("0");
            dir.is_dir().then(|| NxSave { title: e.program(), kind, dir })
        })
        .collect()
}

/// The game's name Ryujinx shows (`games/<title id>/gui/metadata.json`).
pub fn game_name(emu: &Path, title: u64) -> Option<String> {
    let p = emu.join("games").join(format!("{title:016x}")).join("gui").join("metadata.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    v["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

// ---------------------------------------------------------------------------
// The yuzu family (Eden, Citron, Sudachi, Suyu, yuzu)
//
// No index: a save is `nand/user/save/0000000000000000/<user id>/<title id>/`
// under the emulator's `user` folder, both in upper-case hex, with the files
// right in it (yuzu's `SaveDataFactory::GetFullPath`). The user id is the
// profile's 16-byte UUID read as two little-endian halves, high half
// written first; device saves use an all-zero user id. The profiles are in
// `profiles.dat`, and the one in use is `current_user` (an index) in the
// emulator's Qt settings.

/// Size bookkeeping yuzu writes into each save folder. It isn't part of the
/// game's save and would show up as a file in Ryujinx, so it isn't synced.
pub const YUZU_SIZE_FILE: &str = ".yuzu_save_size";

fn yuzu_saves_dir(user_dir: &Path) -> PathBuf {
    user_dir.join("nand").join("user").join("save").join("0000000000000000")
}

fn yuzu_user_folder(user: (u64, u64)) -> String {
    format!("{:016X}{:016X}", user.0, user.1)
}

/// The profiles in `profiles.dat`, in the emulator's order.
pub fn yuzu_profiles(user_dir: &Path) -> Vec<(u64, u64)> {
    let p = user_dir.join("nand").join("system").join("save").join("8000000000000010").join("su").join("avators").join("profiles.dat");
    std::fs::read(p).map(|b| parse_yuzu_profiles(&b)).unwrap_or_default()
}

/// `profiles.dat`: 0x10 bytes of padding, then up to 8 records of 0xC8
/// bytes that start with the profile's UUID.
fn parse_yuzu_profiles(b: &[u8]) -> Vec<(u64, u64)> {
    const HEADER: usize = 0x10;
    const RECORD: usize = 0xC8;
    (0..8)
        .map(|i| HEADER + i * RECORD)
        .take_while(|&at| at + 16 <= b.len())
        .map(|at| (u64_le(b, at + 8), u64_le(b, at)))
        .filter(|&u| u != (0, 0))
        .collect()
}

/// Where a yuzu-family emulator keeps its settings: `<user>/config` (Windows
/// and portable installs), or on Linux `~/.config/<name>` while its data
/// is in `~/.local/share/<name>`.
fn yuzu_config_dir(user_dir: &Path) -> PathBuf {
    let inside = user_dir.join("config");
    if inside.is_dir() || !cfg!(unix) {
        return inside;
    }
    user_dir
        .file_name()
        .and_then(|n| dirs::config_dir().map(|c| c.join(n)))
        .filter(|d| d.is_dir())
        .unwrap_or(inside)
}

/// `current_user=<n>` in the `[System]` section of the emulator's settings
/// (`config/qt-config.ini`, or another `.ini` there in renamed forks).
fn yuzu_current_index(user_dir: &Path) -> Option<usize> {
    let dir = yuzu_config_dir(user_dir);
    let mut files = vec![dir.join("qt-config.ini")];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut more: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")) && !files.contains(p))
            .collect();
        more.sort();
        files.extend(more);
    }
    files
        .iter()
        .find_map(|f| ini_value(&std::fs::read_to_string(f).ok()?, "System", "current_user")?.parse().ok())
}

/// A value in a Qt-style `.ini` file.
pub(crate) fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case(section);
        } else if inside {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// The profile the emulator uses: `current_user` in its settings (the first
/// profile when that is missing). Without `profiles.dat`, the user folder
/// with saves that changed last.
pub fn yuzu_current_user(user_dir: &Path) -> Option<(u64, u64)> {
    let profiles = yuzu_profiles(user_dir);
    if !profiles.is_empty() {
        let i = yuzu_current_index(user_dir).filter(|&i| i < profiles.len()).unwrap_or(0);
        return Some(profiles[i]);
    }
    let mut best: Option<((u64, u64), std::time::SystemTime)> = None;
    for e in std::fs::read_dir(yuzu_saves_dir(user_dir)).into_iter().flatten().flatten() {
        let Some(u) = parse_user(&e.file_name().to_string_lossy()).filter(|&u| u != (0, 0)) else { continue };
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        if best.is_none_or(|(_, bt)| t > bt) {
            best = Some((u, t));
        }
    }
    best.map(|(u, _)| u)
}

/// Where a game's save is (or goes, when the game hasn't made one yet) for
/// the profile the emulator uses. `None` for an account save while the
/// emulator has no profile yet.
pub fn yuzu_save(user_dir: &Path, kind: SaveKind, title: u64) -> Option<PathBuf> {
    let user = match kind {
        SaveKind::Account => yuzu_current_user(user_dir)?,
        SaveKind::Device => (0, 0),
    };
    Some(yuzu_saves_dir(user_dir).join(yuzu_user_folder(user)).join(format!("{title:016X}")))
}

/// Stands for an account save while the emulator has no profile yet: scans
/// find nothing there, and a restore asks to open the emulator once.
pub fn yuzu_pending_dir(user_dir: &Path, kind: SaveKind, title: u64) -> PathBuf {
    user_dir.join("nand").join("user").join("save").join(format!("ludonimbus-new-{}", root_name(kind, title))).join("0")
}

/// The game saves per-game sync would use: the current profile's and the
/// device saves.
pub fn yuzu_saves(user_dir: &Path) -> Vec<NxSave> {
    let base = yuzu_saves_dir(user_dir);
    let mut owners = Vec::new();
    if let Some(u) = yuzu_current_user(user_dir) {
        owners.push((SaveKind::Account, u));
    }
    owners.push((SaveKind::Device, (0, 0)));
    let mut out = Vec::new();
    for (kind, user) in owners {
        let Ok(rd) = std::fs::read_dir(base.join(yuzu_user_folder(user))) else { continue };
        let mut found: Vec<NxSave> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let title = (name.len() == 16).then(|| u64::from_str_radix(&name, 16).ok()).flatten()?;
                is_app_title(title).then(|| NxSave { title, kind, dir: e.path() })
            })
            .collect();
        found.sort_by_key(|s| s.title);
        out.extend(found);
    }
    out
}

/// The game folders set in the emulator (`Paths\gamedirs\<n>\path` in its
/// Qt settings), for finding the file that starts each game.
pub fn yuzu_game_dirs(user_dir: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(yuzu_config_dir(user_dir).join("qt-config.ini")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.eq_ignore_ascii_case("UI");
            continue;
        }
        let Some((k, v)) = line.split_once('=').filter(|_| inside) else { continue };
        let k = k.trim().replace('/', "\\").to_lowercase();
        if k.starts_with("paths\\gamedirs\\") && k.ends_with("\\path") {
            let v = v.trim().trim_matches('"');
            if !["SDMC", "UserNAND", "SysNAND"].contains(&v) && !v.is_empty() {
                // Qt writes `/` on every system.
                out.push(PathBuf::from(v.replace('/', std::path::MAIN_SEPARATOR_STR)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MHGU: u64 = 0x0100_7700_08DD_8000;

    /// The first entries of a real Ryujinx index: two system saves, then
    /// an account save of the default profile.
    fn real_index() -> Vec<u8> {
        let mut b = b"IMKV\0\0\0\0\x03\0\0\0".to_vec();
        for (key, val) in [
            ({
                let mut k = [0u8; 0x40];
                k[0x18..0x20].copy_from_slice(&0x8000_0000_0000_0030u64.to_le_bytes());
                k
            }, {
                let mut v = [0u8; 0x40];
                v[0..8].copy_from_slice(&0x8000_0000_0000_0030u64.to_le_bytes());
                v
            }),
            ({
                let mut k = [0u8; 0x40];
                k[0x18..0x20].copy_from_slice(&0x8000_0000_0000_1040u64.to_le_bytes());
                k
            }, {
                let mut v = [0u8; 0x40];
                v[0..8].copy_from_slice(&0x8000_0000_0000_1040u64.to_le_bytes());
                v
            }),
            (IndexEntry::new(MHGU, SaveKind::Account, (1, 0), 1).key, IndexEntry::new(MHGU, SaveKind::Account, (1, 0), 1).value),
        ] {
            b.extend_from_slice(b"IMEN\x40\0\0\0\x40\0\0\0");
            b.extend_from_slice(&key);
            b.extend_from_slice(&val);
        }
        b
    }

    #[test]
    fn index_round_trip() {
        let b = real_index();
        // Bytes as Ryujinx wrote them for the account save.
        assert_eq!(&b[0x130..0x140], &[0x00, 0x80, 0xdd, 0x08, 0x00, 0x77, 0x00, 0x01, 1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(b[0x150], 1);
        assert_eq!(b[0x188], 1);
        let e = parse_index(&b).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!((e[2].program(), e[2].user(), e[2].save_id()), (MHGU, (1, 0), 1));
        assert!(!e[0].plain() && e[2].plain());
        assert_eq!(write_index(&e), b);
        assert!(parse_index(b"IMKV\0\0\0\0\x01\0\0\0IMEN").is_err());
        // A damaged header claiming four billion entries: an error, not an
        // attempt to make room for half a terabyte.
        assert!(parse_index(b"IMKV\0\0\0\0\xff\xff\xff\xff").is_err());
        let mut more = b.clone();
        more[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_index(&more).is_err());
    }

    #[test]
    fn roots_and_titles() {
        assert_eq!(root_name(SaveKind::Account, MHGU), "nxsave-0100770008dd8000");
        assert_eq!(parse_root("nxsave-0100770008dd8000"), Some((SaveKind::Account, MHGU)));
        assert_eq!(parse_root("nxdevice-0100770008DD8000"), Some((SaveKind::Device, MHGU)));
        assert_eq!(parse_root("emu"), None);
        assert_eq!(title_in_text("MONSTER HUNTER [0100770008dd8000][v0].nsp"), Some(MHGU));
        // An update's id: the base game's saves.
        assert_eq!(title_in_text("[UPD][v1.4.0][0100770008DD8800][v262144].nsp"), Some(MHGU));
        assert_eq!(title_in_text("0100000000001009"), None);
        assert_eq!(parse_user("4b04a3eee61e4f808c8c15d3d0afb19d"), Some((0x4b04a3eee61e4f80, 0x8c8c15d3d0afb19d)));
    }

    fn pfs0(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut table = Vec::new();
        let mut data = Vec::new();
        for (n, d) in files {
            table.extend_from_slice(&(data.len() as u64).to_le_bytes());
            table.extend_from_slice(&(d.len() as u64).to_le_bytes());
            table.extend_from_slice(&(names.len() as u32).to_le_bytes());
            table.extend_from_slice(&0u32.to_le_bytes());
            names.extend_from_slice(n.as_bytes());
            names.push(0);
            data.extend_from_slice(d);
        }
        let mut out = b"PFS0".to_vec();
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(table);
        out.extend(names);
        out.extend(data);
        out
    }

    fn hfs0(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut table = Vec::new();
        let mut data = Vec::new();
        for (n, d) in files {
            let mut e = vec![0u8; 0x40];
            e[0..8].copy_from_slice(&(data.len() as u64).to_le_bytes());
            e[8..16].copy_from_slice(&(d.len() as u64).to_le_bytes());
            e[16..20].copy_from_slice(&(names.len() as u32).to_le_bytes());
            table.extend(e);
            names.extend_from_slice(n.as_bytes());
            names.push(0);
            data.extend_from_slice(d);
        }
        let mut out = b"HFS0".to_vec();
        out.extend_from_slice(&(files.len() as u32).to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(table);
        out.extend(names);
        out.extend(data);
        out
    }

    const KEY: [u8; 32] = [7; 32];

    /// An NCA whose (encrypted) header carries a program id.
    fn nca(program: u64, len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len.max(0x400)];
        let mut h = vec![0u8; 0x200];
        h[0..4].copy_from_slice(b"NCA3");
        h[0x10..0x18].copy_from_slice(&program.to_le_bytes());
        xts(&KEY, 1, &mut h, true);
        b[0x200..0x400].copy_from_slice(&h);
        b
    }

    #[test]
    fn titles_from_game_files() {
        let tmp = tempfile::tempdir().unwrap();
        let keys = tmp.path().join("prod.keys");
        let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&keys, format!("aes_kek_generation_source = 00\nheader_key = {hex}\n")).unwrap();
        assert_eq!(header_key(std::slice::from_ref(&keys)), Some(KEY));
        // XTS round trip.
        let mut s = vec![3u8; 0x200];
        xts(&KEY, 5, &mut s, true);
        assert_ne!(s, vec![3u8; 0x200]);
        xts(&KEY, 5, &mut s, false);
        assert_eq!(s, vec![3u8; 0x200]);

        // NSP with a .cnmt.xml (scene dumps).
        let xml = b"<?xml version=\"1.0\"?><ContentMeta><Type>Application</Type><Id>0x0100770008dd8000</Id></ContentMeta>".to_vec();
        let nsp = tmp.path().join("a.nsp");
        std::fs::write(&nsp, pfs0(&[("x.nca", vec![0; 64]), ("a721.cnmt.xml", xml)])).unwrap();
        assert_eq!(title_id(&nsp, &[]), Some(MHGU));
        // NSP with a ticket.
        std::fs::write(&nsp, pfs0(&[("01007ef00011e0000000000000000004.tik", vec![0; 16])])).unwrap();
        assert_eq!(title_id(&nsp, &[]), Some(0x0100_7EF0_0011_E000));
        // NSP with only NCAs: needs the key.
        std::fs::write(&nsp, pfs0(&[("small.cnmt.nca", nca(0x0100_7EF0_0011_E800, 0x400)), ("big.nca", nca(MHGU, 0x800))])).unwrap();
        assert_eq!(title_id(&nsp, &[]), None);
        assert_eq!(title_id(&nsp, std::slice::from_ref(&keys)), Some(MHGU));

        // XCI: card header, root partition table, secure partition.
        let secure = hfs0(&[("a.nca", nca(MHGU, 0x400))]);
        let root = hfs0(&[("update", hfs0(&[("u.nca", vec![0; 16])])), ("secure", secure)]);
        let mut xci = vec![0u8; 0x200];
        xci[0x100..0x104].copy_from_slice(b"HEAD");
        xci[0x130..0x138].copy_from_slice(&0x200u64.to_le_bytes());
        xci.extend(root);
        let path = tmp.path().join("game.xci");
        std::fs::write(&path, &xci).unwrap();
        assert_eq!(title_id(&path, std::slice::from_ref(&keys)), Some(MHGU));
        // With the 4 KB key area in front.
        let mut with_key_area = vec![0u8; 0x1000];
        with_key_area.extend(xci);
        std::fs::write(&path, &with_key_area).unwrap();
        assert_eq!(title_id(&path, std::slice::from_ref(&keys)), Some(MHGU));
        assert_eq!(title_id(&tmp.path().join("missing.xci"), &[]), None);
    }

    /// A Ryujinx data folder like a real one: profiles, the index and saves.
    fn ryujinx(dir: &Path, last_opened: &str) -> PathBuf {
        let emu = dir.join("portable");
        let idx = index_dir(&emu);
        for d in ["0", "1"] {
            std::fs::create_dir_all(idx.join(d)).unwrap();
            std::fs::write(idx.join(d).join("imkvdb.arc"), real_index()).unwrap();
            std::fs::write(idx.join(d).join("lastPublishedId"), 1u64.to_le_bytes()).unwrap();
        }
        let s = save_dir(&emu, 1);
        std::fs::create_dir_all(s.join("0")).unwrap();
        std::fs::write(s.join("0").join("save.bin"), b"default profile").unwrap();
        let mut extra = extra_data(&IndexEntry::new(MHGU, SaveKind::Account, (1, 0), 1), 0xa0_0000, 0x50_0000);
        extra[0x68..0x70].fill(0);
        std::fs::write(s.join("ExtraData0"), &extra).unwrap();
        std::fs::create_dir_all(emu.join("system")).unwrap();
        std::fs::write(
            emu.join("system").join("Profiles.json"),
            format!(r#"{{"profiles":[{{"user_id":"00000000000000010000000000000000","name":"RyuPlayer"}}],"last_opened":"{last_opened}"}}"#),
        )
        .unwrap();
        emu
    }

    #[test]
    fn finds_and_creates_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let emu = ryujinx(tmp.path(), "00000000000000010000000000000000");
        assert_eq!(find_save(&emu, SaveKind::Account, MHGU), Some(save_dir(&emu, 1).join("0")));
        assert_eq!(find_save(&emu, SaveKind::Device, MHGU), None);
        assert_eq!(saves(&emu).len(), 1);

        // Another profile is current: its save doesn't exist yet.
        let me = "4b04a3eee61e4f808c8c15d3d0afb19d";
        std::fs::write(
            emu.join("system").join("Profiles.json"),
            format!(r#"{{"profiles":[],"last_opened":"{me}"}}"#),
        )
        .unwrap();
        assert_eq!(find_save(&emu, SaveKind::Account, MHGU), None);
        assert!(saves(&emu).is_empty());
        let backups = tmp.path().join("backups");
        let create = |kind| create_save_with_backup(&emu, kind, MHGU, &backups);
        let p = create(SaveKind::Account).unwrap();
        assert_eq!(p, save_dir(&emu, 2).join("0"));
        // The old index was kept.
        let kept: Vec<_> = std::fs::read_dir(&backups).unwrap().flatten().collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path().join("imkvdb.arc")).unwrap(), real_index());
        assert_eq!(find_save(&emu, SaveKind::Account, MHGU), Some(p.clone()));
        // Both index copies updated, next number recorded.
        for d in ["0", "1"] {
            let e = parse_index(&std::fs::read(index_dir(&emu).join(d).join("imkvdb.arc")).unwrap()).unwrap();
            assert_eq!(e.len(), 4);
            assert_eq!((e[3].user(), e[3].save_id()), ((0x4b04a3eee61e4f80, 0x8c8c15d3d0afb19d), 2));
            assert_eq!(std::fs::read(index_dir(&emu).join(d).join("lastPublishedId")).unwrap(), 2u64.to_le_bytes());
        }
        // Extra data: attribute, owner, sizes taken from the other save of the game.
        let x = std::fs::read(save_dir(&emu, 2).join("ExtraData0")).unwrap();
        assert_eq!(x.len(), 0x200);
        assert_eq!(&x[0..0x40], &IndexEntry::new(MHGU, SaveKind::Account, parse_user(me).unwrap(), 2).key);
        assert_eq!((u64_le(&x, 0x40), u64_le(&x, 0x58), u64_le(&x, 0x60)), (MHGU, 0xa0_0000, 0x50_0000));
        assert_eq!(std::fs::read(save_dir(&emu, 2).join("ExtraData1")).unwrap(), x);
        // Creating again finds it; a device save gets the next number.
        assert_eq!(create(SaveKind::Account).unwrap(), p);
        assert_eq!(create(SaveKind::Device).unwrap(), save_dir(&emu, 3).join("0"));

        // Restored files reach the working copy too.
        std::fs::write(p.join("data.bin"), b"restored").unwrap();
        std::fs::create_dir_all(save_dir(&emu, 2).join("1").join("old")).unwrap();
        mirror_committed(&p).unwrap();
        assert_eq!(std::fs::read(save_dir(&emu, 2).join("1").join("data.bin")).unwrap(), b"restored");
        assert!(!save_dir(&emu, 2).join("1").join("old").exists());
        // Only numbered save folders are touched.
        let other = tmp.path().join("x").join("0");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(tmp.path().join("x").join("1")).unwrap();
        mirror_committed(&other).unwrap();
        assert!(tmp.path().join("x").join("1").exists());

        assert!(is_pending(&pending_dir(&emu, SaveKind::Account, MHGU)));
        assert!(!is_pending(&p));
    }

    #[test]
    fn no_index_no_save() {
        let tmp = tempfile::tempdir().unwrap();
        let err = create_save_with_backup(tmp.path(), SaveKind::Account, MHGU, &tmp.path().join("b")).unwrap_err();
        assert!(err.to_string().contains("先打开一次 Ryujinx") || err.to_string().contains("Open Ryujinx once"), "{err}");
    }

    /// A `profiles.dat` with these UUIDs (16 raw bytes each).
    fn profiles_dat(uuids: &[[u8; 16]]) -> Vec<u8> {
        let mut b = vec![0u8; 0x650];
        for (i, u) in uuids.iter().enumerate() {
            let at = 0x10 + i * 0xC8;
            b[at..at + 16].copy_from_slice(u);
            b[at + 16..at + 32].copy_from_slice(u);
            b[at + 0x28..at + 0x2e].copy_from_slice(b"Player");
        }
        b
    }

    /// An Eden `user` folder with two profiles.
    fn eden(dir: &Path, current: Option<usize>) -> PathBuf {
        let user = dir.join("user");
        let avatars = user.join("nand/system/save/8000000000000010/su/avators");
        std::fs::create_dir_all(&avatars).unwrap();
        let first: [u8; 16] = [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let second: [u8; 16] = [0x11; 16];
        std::fs::write(avatars.join("profiles.dat"), profiles_dat(&[first, second])).unwrap();
        if let Some(i) = current {
            std::fs::create_dir_all(user.join("config")).unwrap();
            std::fs::write(
                user.join("config/qt-config.ini"),
                format!("[UI]\nPaths\\gamedirs\\size=2\nPaths\\gamedirs\\1\\path=SDMC\nPaths\\gamedirs\\2\\path=D:/Games/Switch\n\n[System]\ncurrent_user\\default=false\ncurrent_user={i}\n"),
            )
            .unwrap();
        }
        user
    }

    #[test]
    fn yuzu_family_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let user = eden(tmp.path(), None);
        // UUID bytes 0..8 are the low half, 8..16 the high half; the high half is written first.
        let first = (0xefcd_ab89_6745_2301, 0xfedc_ba98_7654_3210);
        assert_eq!(yuzu_profiles(&user), vec![first, (0x1111_1111_1111_1111, 0x1111_1111_1111_1111)]);
        assert_eq!(yuzu_current_user(&user), Some(first));
        let saves = user.join("nand/user/save/0000000000000000");
        assert_eq!(yuzu_save(&user, SaveKind::Account, MHGU), Some(saves.join("EFCDAB8967452301FEDCBA9876543210").join("0100770008DD8000")));
        assert_eq!(yuzu_save(&user, SaveKind::Device, MHGU), Some(saves.join("00000000000000000000000000000000").join("0100770008DD8000")));

        // The second profile is the current one.
        let user = eden(&tmp.path().join("b"), Some(1));
        assert_eq!(yuzu_current_user(&user), Some((0x1111_1111_1111_1111, 0x1111_1111_1111_1111)));
        assert_eq!(yuzu_game_dirs(&user), vec![PathBuf::from("D:/Games/Switch".replace('/', std::path::MAIN_SEPARATOR_STR))]);
        // An index past the profiles falls back to the first.
        let user = eden(&tmp.path().join("c"), Some(5));
        assert_eq!(yuzu_current_user(&user), Some(first));
    }

    #[test]
    fn yuzu_family_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let user = eden(tmp.path(), Some(0));
        let mine = yuzu_save(&user, SaveKind::Account, MHGU).unwrap();
        let device = yuzu_save(&user, SaveKind::Device, 0x0100_0000_0001_2000).unwrap();
        let other = user.join("nand/user/save/0000000000000000/11111111111111111111111111111111/0100A00000001000");
        for d in [&mine, &device, &other] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("data.bin"), b"x").unwrap();
        }
        // A system save in the same place is not a game's.
        std::fs::create_dir_all(user.join("nand/user/save/0000000000000000/00000000000000000000000000000000/0100000000001000")).unwrap();
        let found = yuzu_saves(&user);
        assert_eq!(
            found,
            vec![
                NxSave { title: MHGU, kind: SaveKind::Account, dir: mine },
                NxSave { title: 0x0100_0000_0001_2000, kind: SaveKind::Device, dir: device },
            ]
        );

        // No profiles.dat: the user folder that changed last.
        std::fs::remove_file(user.join("nand/system/save/8000000000000010/su/avators/profiles.dat")).unwrap();
        assert!(yuzu_current_user(&user).is_some());
        let fresh = tempfile::tempdir().unwrap();
        assert_eq!(yuzu_current_user(fresh.path()), None);
        assert_eq!(yuzu_save(fresh.path(), SaveKind::Account, MHGU), None);
        assert!(yuzu_save(fresh.path(), SaveKind::Device, MHGU).is_some());
        assert!(is_pending(&yuzu_pending_dir(fresh.path(), SaveKind::Account, MHGU)));
    }

    /// Reads the title id from the NCA headers of a real dump with the
    /// user's own keys: `LN_NSP=<file> LN_KEYS=<prod.keys> cargo test -- --ignored real_nca`.
    #[test]
    #[ignore]
    fn real_nca_headers() {
        let (Ok(f), Ok(k)) = (std::env::var("LN_NSP"), std::env::var("LN_KEYS")) else { return };
        let path = PathBuf::from(f);
        let files = pfs_entries(&path, 0).or_else(|| xci_secure(&path)).expect("not an NSP / XCI");
        let key = header_key(&[PathBuf::from(k)]).expect("no header_key");
        let t = nca_title(&path, &files, &key).expect("no program id");
        println!("title id from NCA headers: {t:016X}");
        assert_eq!(Some(t), title_in_text(&path.file_name().unwrap().to_string_lossy()));
    }
}
