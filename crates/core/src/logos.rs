//! Logos for the library's source groups: the icon built into the program a
//! game runs on (the emulator, the recompilation) and, for Xbox app games,
//! the Xbox app's own icon. Nothing is drawn or shipped by Ludonimbus: the
//! image comes from the program installed on this PC, so a game whose
//! program is missing here has no logo.
//!
//! Icons are read from the exe's resources and cached as PNG files named by
//! their content (`%APPDATA%\Ludonimbus\logos\<hash>.png`), so the same
//! emulator installed in two folders gives the same file.

use crate::adapters::Adapter;
use crate::config::GameEntry;
use crate::{paths, recomplauncher, spec, xbox};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub fn dir() -> PathBuf {
    paths::data_dir().join("logos")
}

/// The cached logo `name` (`<hash>.png`), for the art protocol.
pub fn file(name: &str) -> Option<PathBuf> {
    let stem = name.strip_suffix(".png")?;
    let valid = (8..=64).contains(&stem.len()) && stem.chars().all(|c| c.is_ascii_hexdigit());
    let f = dir().join(name);
    (valid && f.is_file()).then_some(f)
}

/// The logo of what `game` runs on: its emulator or recompilation (the
/// program itself, not RecompLauncher when it can be found), the Xbox app
/// for Xbox games, or Steam for the games it installed. `None` for other PC
/// games and games added by hand.
pub fn for_game(game: &GameEntry, adapter: Option<&Adapter>) -> Option<PathBuf> {
    let exe = game.exe.as_deref();
    if adapter.is_none() && game.store.is_some() {
        return spec::steam_root().and_then(|r| of_exe(&r.join("steam.exe")));
    }
    if adapter.is_none() {
        let is_xbox = exe.is_some_and(|e| xbox::is_launch_helper(e) || xbox::content_dir(e).is_some());
        return if is_xbox { xbox_app().and_then(|e| of_exe(&e)) } else { None };
    }
    program_exes(game, adapter?).into_iter().find_map(|e| of_exe(&e))
}

/// Where the program can be: the shortcut's exe, unless it is RecompLauncher,
/// then the rule's exe names in the game folder, then RecompLauncher.
fn program_exes(game: &GameEntry, adapter: &Adapter) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let launcher = game.exe.as_deref().is_some_and(recomplauncher::is_launcher);
    if let (Some(e), false) = (&game.exe, launcher) {
        out.push(e.clone());
    }
    if let (Some(dir), Some(m)) = (spec::install_dir(game), &adapter.matcher) {
        out.extend(m.exe.iter().filter(|n| !n.contains('*')).map(|n| dir.join(n)));
    }
    if let (Some(e), true) = (&game.exe, launcher) {
        out.push(e.clone());
    }
    out
}

/// The Xbox app's program (`XboxPcApp.exe` in its package folder).
fn xbox_app() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        let packages = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages")
            .ok()?;
        let mut names: Vec<String> = packages.enum_keys().flatten().filter(|k| k.starts_with("Microsoft.GamingApp_")).collect();
        names.sort();
        names.iter().rev().find_map(|n| {
            let root: String = packages.open_subkey(n).ok()?.get_value("PackageRootFolder").ok()?;
            let exe = Path::new(&root).join("XboxPcApp.exe");
            exe.is_file().then_some(exe)
        })
    }
    #[cfg(not(windows))]
    None
}

type Memo = HashMap<(PathBuf, u64, u64), Option<PathBuf>>;

/// The cached PNG of `exe`'s icon, made on first use (remembered per exe
/// version while the program runs).
pub fn of_exe(exe: &Path) -> Option<PathBuf> {
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    let meta = std::fs::metadata(exe).ok().filter(|m| m.is_file())?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let id = (exe.to_path_buf(), mtime, meta.len());
    let memo = MEMO.get_or_init(Default::default);
    if let Some(hit) = memo.lock().ok()?.get(&id) {
        return hit.clone().filter(|f| f.is_file());
    }
    let made = exe_icon_png(exe).and_then(|png| save(&png));
    memo.lock().ok()?.insert(id, made.clone());
    made
}

fn save(png: &[u8]) -> Option<PathBuf> {
    let hash = blake3::hash(png).to_hex();
    let f = dir().join(format!("{}.png", &hash[..16]));
    if !f.is_file() {
        crate::util::atomic_write(&f, png).ok()?;
    }
    Some(f)
}

/// The exe's main icon (the first icon group, as Explorer shows it) as PNG:
/// the smallest image at least 64 pixels wide, else the largest.
pub fn exe_icon_png(exe: &Path) -> Option<Vec<u8>> {
    let ico = exe_icon(exe)?;
    let dir = ico::IconDir::read(std::io::Cursor::new(ico)).ok()?;
    let size = |e: &ico::IconDirEntry| (e.width().max(e.height()), e.bits_per_pixel());
    let entries = dir.entries();
    let pick = entries
        .iter()
        .filter(|e| size(e).0 >= 64)
        .min_by_key(|e| (size(e).0, std::cmp::Reverse(size(e).1)))
        .or_else(|| entries.iter().max_by_key(|e| size(e)))?;
    let image = pick.decode().ok()?;
    let mut out = Vec::new();
    image.write_png(&mut out).ok()?;
    Some(out)
}

// --- Reading icons from a PE file --------------------------------------------

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;

/// The exe's first icon group as the bytes of an `.ico` file.
pub fn exe_icon(exe: &Path) -> Option<Vec<u8>> {
    let rsrc = Resources::read(exe)?;
    let group = rsrc.first(RT_GROUP_ICON)?;
    let count = u16_at(group, 4)? as usize;
    let mut images = Vec::new();
    for i in 0..count {
        let e = group.get(6 + i * 14..6 + i * 14 + 14)?;
        let id = u16::from_le_bytes([e[12], e[13]]) as u32;
        if let Some(data) = rsrc.find(RT_ICON, id) {
            images.push((e, data));
        }
    }
    if images.is_empty() {
        return None;
    }
    let mut ico = vec![0, 0, 1, 0];
    ico.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (e, data) in &images {
        ico.extend(&e[..8]);
        ico.extend((data.len() as u32).to_le_bytes());
        ico.extend((offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in &images {
        ico.extend(*data);
    }
    Some(ico)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The resource section of a PE file.
struct Resources {
    data: Vec<u8>,
    /// The section's address when loaded (data entries hold such addresses).
    rva: u32,
}

/// Larger resource sections are not read (icons sit in a few hundred KB).
const MAX_RSRC: u32 = 64 << 20;

impl Resources {
    fn read(exe: &Path) -> Option<Self> {
        let mut f = std::fs::File::open(exe).ok()?;
        let mut dos = [0u8; 64];
        f.read_exact(&mut dos).ok()?;
        if &dos[..2] != b"MZ" {
            return None;
        }
        let pe = u32_at(&dos, 0x3c)? as u64;
        f.seek(SeekFrom::Start(pe)).ok()?;
        let mut head = [0u8; 24];
        f.read_exact(&mut head).ok()?;
        if &head[..4] != b"PE\0\0" {
            return None;
        }
        let sections = u16_at(&head, 6)? as usize;
        let opt_size = u16_at(&head, 20)? as usize;
        let mut opt = vec![0u8; opt_size];
        f.read_exact(&mut opt).ok()?;
        // Data directory 2 (resources): after 96 (PE32) or 112 (PE32+) bytes.
        let dirs = match u16_at(&opt, 0)? {
            0x10b => 96,
            0x20b => 112,
            _ => return None,
        };
        let rva = u32_at(&opt, dirs + 16)?;
        if rva == 0 {
            return None;
        }
        let mut table = vec![0u8; sections * 40];
        f.read_exact(&mut table).ok()?;
        for s in table.chunks_exact(40) {
            let (vsize, vaddr) = (u32_at(s, 8)?, u32_at(s, 12)?);
            let (raw_size, raw_ptr) = (u32_at(s, 16)?, u32_at(s, 20)?);
            if rva < vaddr || rva >= vaddr + vsize.max(raw_size) {
                continue;
            }
            let start = raw_ptr + (rva - vaddr);
            let len = raw_size.checked_sub(rva - vaddr)?.min(MAX_RSRC);
            f.seek(SeekFrom::Start(start as u64)).ok()?;
            let mut data = vec![0u8; len as usize];
            f.read_exact(&mut data).ok()?;
            return Some(Resources { data, rva });
        }
        None
    }

    /// `(id or name flag, offset)` of a directory's entries; names first.
    fn entries(&self, at: usize) -> Option<Vec<(u32, u32)>> {
        let n = u16_at(&self.data, at + 12)? as usize + u16_at(&self.data, at + 14)? as usize;
        (0..n.min(4096)).map(|i| Some((u32_at(&self.data, at + 16 + i * 8)?, u32_at(&self.data, at + 20 + i * 8)?))).collect()
    }

    /// The directory under `type` (level 1), as an offset.
    fn type_dir(&self, kind: u32) -> Option<usize> {
        let (_, off) = self.entries(0)?.into_iter().find(|(id, off)| *id == kind && off & 0x8000_0000 != 0)?;
        Some((off & 0x7fff_ffff) as usize)
    }

    /// The data of the first language of a level-2 entry.
    fn data_of(&self, off: u32) -> Option<&[u8]> {
        let mut off = off;
        // Walk down through subdirectories (the language level).
        for _ in 0..2 {
            if off & 0x8000_0000 == 0 {
                break;
            }
            off = self.entries((off & 0x7fff_ffff) as usize)?.first()?.1;
        }
        if off & 0x8000_0000 != 0 {
            return None;
        }
        let at = off as usize;
        let (data_rva, size) = (u32_at(&self.data, at)?, u32_at(&self.data, at + 4)?);
        let start = data_rva.checked_sub(self.rva)? as usize;
        self.data.get(start..start + size as usize)
    }

    /// The first resource of a type (by Explorer's order: names, then ids).
    fn first(&self, kind: u32) -> Option<&[u8]> {
        let dir = self.type_dir(kind)?;
        self.entries(dir)?.into_iter().find_map(|(_, off)| self.data_of(off))
    }

    fn find(&self, kind: u32, id: u32) -> Option<&[u8]> {
        let dir = self.type_dir(kind)?;
        let (_, off) = self.entries(dir)?.into_iter().find(|(n, _)| *n == id)?;
        self.data_of(off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PE32+ file with one icon group holding one 1x1 PNG image.
    fn tiny_exe(png: &[u8]) -> Vec<u8> {
        const SECT_RVA: u32 = 0x1000;
        const RAW: u32 = 0x200;
        // Resource section: root -> type dirs -> name dirs -> lang dirs -> data entries.
        let mut r = vec![0u8; 0];
        let dir = |r: &mut Vec<u8>, entries: &[(u32, u32)]| {
            r.extend([0u8; 12]);
            r.extend(0u16.to_le_bytes());
            r.extend((entries.len() as u16).to_le_bytes());
            for (id, off) in entries {
                r.extend(id.to_le_bytes());
                r.extend(off.to_le_bytes());
            }
        };
        let sub = 0x8000_0000u32;
        // Layout (offsets): root 0, icon type 32, group type 56, icon name 80,
        // group name 104, icon lang 128, group lang 152, data entries 176/192, data 208.
        dir(&mut r, &[(RT_ICON, sub | 32), (RT_GROUP_ICON, sub | 56)]);
        dir(&mut r, &[(1, sub | 80)]);
        dir(&mut r, &[(1, sub | 104)]);
        dir(&mut r, &[(1033, sub | 128)]);
        dir(&mut r, &[(1033, sub | 152)]);
        dir(&mut r, &[(1033, 176)]);
        dir(&mut r, &[(1033, 192)]);
        let mut group = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
        group.extend((png.len() as u32).to_le_bytes());
        group.extend(1u16.to_le_bytes());
        let png_at = 208u32;
        let group_at = png_at + png.len() as u32;
        for (at, len) in [(png_at, png.len() as u32), (group_at, group.len() as u32)] {
            r.extend((SECT_RVA + at).to_le_bytes());
            r.extend(len.to_le_bytes());
            r.extend([0u8; 8]);
        }
        assert_eq!(r.len(), 208);
        r.extend(png);
        r.extend(&group);

        let mut f = vec![0u8; RAW as usize];
        f[..2].copy_from_slice(b"MZ");
        f[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        let mut pe = b"PE\0\0".to_vec();
        pe.extend([0x64, 0x86]); // machine
        pe.extend(1u16.to_le_bytes()); // sections
        pe.extend([0u8; 12]);
        pe.extend(240u16.to_le_bytes()); // optional header size
        pe.extend([0u8; 2]);
        let mut opt = vec![0u8; 240];
        opt[..2].copy_from_slice(&0x20bu16.to_le_bytes());
        opt[112 + 16..112 + 20].copy_from_slice(&SECT_RVA.to_le_bytes());
        opt[112 + 20..112 + 24].copy_from_slice(&(r.len() as u32).to_le_bytes());
        pe.extend(opt);
        let mut sect = b".rsrc\0\0\0".to_vec();
        sect.extend((r.len() as u32).to_le_bytes());
        sect.extend(SECT_RVA.to_le_bytes());
        sect.extend((r.len() as u32).to_le_bytes());
        sect.extend(RAW.to_le_bytes());
        sect.extend([0u8; 16]);
        pe.extend(sect);
        f[0x40..0x40 + pe.len()].copy_from_slice(&pe);
        f.extend(r);
        f
    }

    fn one_pixel_png() -> Vec<u8> {
        let image = ico::IconImage::from_rgba_data(1, 1, vec![255, 0, 0, 255]);
        let mut png = Vec::new();
        image.write_png(&mut png).unwrap();
        png
    }

    #[test]
    fn reads_the_icon_of_an_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let png = one_pixel_png();
        let exe = tmp.path().join("Emu.exe");
        std::fs::write(&exe, tiny_exe(&png)).unwrap();
        let ico = exe_icon(&exe).unwrap();
        let dir = ico::IconDir::read(std::io::Cursor::new(ico)).unwrap();
        assert_eq!(dir.entries().len(), 1);
        assert_eq!(dir.entries()[0].data(), &png[..]);
        let out = exe_icon_png(&exe).unwrap();
        let back = ico::IconImage::read_png(std::io::Cursor::new(out)).unwrap();
        assert_eq!(back.rgba_data(), &[255, 0, 0, 255]);
    }

    #[test]
    fn not_an_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("x.exe");
        std::fs::write(&f, b"hello").unwrap();
        assert!(exe_icon(&f).is_none());
        assert!(exe_icon(&tmp.path().join("missing.exe")).is_none());
    }

    #[test]
    fn logo_names() {
        assert!(file("../x.png").is_none());
        assert!(file("abcd.png").is_none());
        assert!(file("0123456789abcdef.jpg").is_none());
    }
}
