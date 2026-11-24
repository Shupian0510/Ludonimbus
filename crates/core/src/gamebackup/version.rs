//! Version labels for game backup components: what a person would call the
//! version of an emulator, a recompilation or a game's disc.

use crate::emulators::{self, EmuGame};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The first version-like token of a name: `fefw-recomp-0.2.7` → `0.2.7`,
/// `Zelda64Recompiled-v1.2.2-Windows` → `1.2.2`, `Cemu 2.6` → `2.6`.
pub fn in_name(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == 'v' || chars[i - 1] == 'V') {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect::<String>().trim_end_matches('.').to_string();
            let parts: Vec<&str> = tok.split('.').collect();
            if parts.len() >= 2 && parts.len() <= 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 5) {
                return Some(tok);
            }
        }
        i += 1;
    }
    None
}

/// `a.b.c.d` with trailing zero parts dropped (at least `a.b`).
fn tidy(parts: [u32; 4]) -> String {
    let mut n = 4;
    while n > 2 && parts[n - 1] == 0 {
        n -= 1;
    }
    parts[..n].iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}

/// The file version an exe declares (Windows version resource).
#[cfg(windows)]
pub fn exe_version(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(buf.as_ptr().cast(), root.as_ptr(), &mut ptr, &mut len) == 0 || ptr.is_null() {
            return None;
        }
        if (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let info = &*(ptr as *const VS_FIXEDFILEINFO);
        let parts = [
            info.dwFileVersionMS >> 16,
            info.dwFileVersionMS & 0xffff,
            info.dwFileVersionLS >> 16,
            info.dwFileVersionLS & 0xffff,
        ];
        (parts != [0; 4]).then(|| tidy(parts))
    }
}

#[cfg(not(windows))]
pub fn exe_version(_: &Path) -> Option<String> {
    None
}

/// `version` of a `package.json` in the folder (Lantern Engine's release).
fn package_json(folder: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("package.json")).ok()?).ok()?;
    v["version"].as_str().filter(|s| in_name(s).as_deref() == Some(*s)).map(str::to_string)
}

/// Version of a program: recompilations are usually unpacked into a folder
/// named after the release (or say it in a `package.json`); emulators
/// declare it in their exe.
pub fn program(folder: &Path, exe: Option<&Path>, emulator: bool) -> Option<String> {
    let from_name = || folder.file_name().and_then(|n| in_name(&n.to_string_lossy()));
    let from_exe = || exe.and_then(exe_version);
    if emulator {
        from_exe().or_else(from_name)
    } else {
        from_name().or_else(|| package_json(folder)).or_else(from_exe)
    }
}

fn read_at(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// GameCube / Wii disc revision (byte 7 of the disc header).
fn disc_revision(path: &Path) -> Option<String> {
    let head = read_at(path, 0, 0x60)?;
    let at = match &head[0..4] {
        b"WBFS" => 0x200,
        b"RVZ\x01" | b"WIA\x01" => 0x58,
        b"CISO" => 0x8000,
        _ => 0,
    };
    let disc = read_at(path, at, 8)?;
    Some(crate::tr!("修订版 {}", "revision {}", disc[7]))
}

/// `APP_VER` (or `VERSION`) of the PARAM.SFO next to a PS3 / PS4 game.
fn sfo_version(source: &Path) -> Option<String> {
    let dir = if source.is_file() { source.parent()? } else { source };
    let cands = [
        dir.join("PARAM.SFO"),
        dir.join("PS3_GAME").join("PARAM.SFO"),
        dir.join("sce_sys").join("param.sfo"),
        dir.parent().map(|p| p.join("PARAM.SFO")).unwrap_or_default(),
    ];
    cands.iter().filter(|p| p.is_file()).find_map(|p| {
        let b = std::fs::read(p).ok()?;
        emulators::sfo_value(&b, "APP_VER").or_else(|| emulators::sfo_value(&b, "VERSION")).filter(|v| !v.is_empty())
    })
    // "01.05" → "v1.05"
    .map(|v| {
        let t = v.strip_prefix('0').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(&v);
        format!("v{t}")
    })
}

/// `title_version` of an unpacked Wii U game's `meta/meta.xml`.
fn wiiu_version(source: &Path) -> Option<String> {
    let mut dirs = vec![];
    if source.is_file() {
        dirs.extend(source.parent().and_then(Path::parent).map(Path::to_path_buf));
    } else {
        dirs.push(source.to_path_buf());
    }
    dirs.iter().find_map(|d| {
        let meta = crate::util::find_file_ci(&d.join("meta"), "meta.xml")?;
        let text = String::from_utf8_lossy(&std::fs::read(meta).ok()?).into_owned();
        let start = text.find("<title_version")?;
        let open_end = start + text[start..].find('>')? + 1;
        let close = open_end + text[open_end..].find('<')?;
        let v: u32 = text[open_end..close].trim().parse().ok()?;
        Some(format!("v{v}"))
    })
}

/// `[v131072]` in a Switch file name (the version inside an NSP is encrypted).
fn switch_version(source: &Path) -> Option<String> {
    let name = source.file_name()?.to_string_lossy().into_owned();
    let start = name.find("[v")? + 2;
    let end = start + name[start..].find(']')?;
    let n: u64 = name[start..end].parse().ok()?;
    Some(format!("v{n}"))
}

/// `title_version` of a Wii U update installed among `files` (a game's
/// `meta/meta.xml` below `…/0005000e/<id>/`), which is the version that runs.
pub fn installed_update(files: &[std::path::PathBuf]) -> Option<String> {
    files.iter().find_map(|f| {
        let s = f.to_string_lossy().replace('\\', "/").to_lowercase();
        if !(s.contains("/0005000e/") && s.ends_with("/meta/meta.xml")) {
            return None;
        }
        wiiu_version(f.parent()?.parent()?)
    })
}

/// The version in an Xbox app game's `appxmanifest.xml` (next to its launch
/// helper), not the helper's own version (which is Windows').
pub fn xbox(exe: &Path) -> Option<String> {
    if !crate::xbox::is_launch_helper(exe) {
        return None;
    }
    let text = std::fs::read_to_string(exe.parent()?.join("appxmanifest.xml")).ok()?;
    let id = &text[text.find("<Identity")?..];
    let id = &id[..id.find('>')?];
    let start = id.find("Version=\"")? + 9;
    let end = start + id[start..].find('"')?;
    Some(id[start..end].to_string())
}

/// Version of the game an emulator shortcut starts.
pub fn content(game_id: Option<&str>, g: &EmuGame) -> Option<String> {
    let source = g.source.as_deref()?;
    match game_id? {
        "gc-wii" => disc_revision(source),
        "ps3" | "ps4" => sfo_version(source),
        "wiiu" => wiiu_version(source),
        "switch" => switch_version(source),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_in_names() {
        assert_eq!(in_name("fefw-recomp-0.2.7").as_deref(), Some("0.2.7"));
        assert_eq!(in_name("Zelda64Recompiled-v1.2.2-Windows").as_deref(), Some("1.2.2"));
        assert_eq!(in_name("Cemu 2.6").as_deref(), Some("2.6"));
        assert_eq!(in_name("rpcs3-v0.0.34-17000_win64").as_deref(), Some("0.0.34"));
        assert_eq!(in_name("BloodbornePC"), None);
        assert_eq!(in_name("x64"), None);
        assert_eq!(tidy([2, 6, 0, 0]), "2.6");
        assert_eq!(tidy([1, 0, 3, 0]), "1.0.3");
        let tmp = tempfile::tempdir().unwrap();
        let bb = tmp.path().join("BloodbornePC");
        std::fs::create_dir_all(&bb).unwrap();
        std::fs::write(bb.join("package.json"), r#"{ "version": "0.9.43", "engine": "x" }"#).unwrap();
        assert_eq!(program(&bb, None, false).as_deref(), Some("0.9.43"));
    }

    #[test]
    fn xbox_and_wiiu_update_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let content = tmp.path().join("Content");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(content.join("gamelaunchhelper.exe"), b"x").unwrap();
        std::fs::write(content.join("appxmanifest.xml"), r#"<Package><Identity Name="X" Publisher="CN=1" Version="1.0.56.0" ProcessorArchitecture="x64" /></Package>"#).unwrap();
        assert_eq!(xbox(&content.join("gamelaunchhelper.exe")).as_deref(), Some("1.0.56.0"));
        assert_eq!(xbox(&content.join("other.exe")), None);
        let meta = tmp.path().join("mlc01/usr/title/0005000e/101c9400/meta/meta.xml");
        std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
        std::fs::write(&meta, "<menu><title_version type=\"unsignedInt\">208</title_version></menu>").unwrap();
        let files = vec![tmp.path().join("Games/x/code/a.rpx"), meta];
        assert_eq!(installed_update(&files).as_deref(), Some("v208"));
    }

    #[test]
    fn switch_version_from_name() {
        assert_eq!(switch_version(Path::new("D:/x/Game [0100F2C0115B6800][v131072].nsp")).as_deref(), Some("v131072"));
        assert_eq!(switch_version(Path::new("D:/x/Game.nsp")), None);
    }
}
