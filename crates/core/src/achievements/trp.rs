//! PS4 trophy files: `sce_sys/trophy/trophy00.trp` in the game's folder.
//!
//! A TRP file is a header (0x60 bytes) and a table of 0x40-byte entries
//! (name, offset, length, flags). Icons (`ICON0.PNG`, `TROPnnn.PNG`) are
//! stored as is. The definitions (`TROP.ESFM`, one `TROP_XX.ESFM` per
//! language) are AES-128-CBC encrypted: the file key is the trophy key
//! encrypting the game's NP communication id (`NPWR…_00`, zero-padded to 16
//! bytes), and each entry starts with its 16-byte IV. Same as shadPS4's
//! `src/core/file_format/trp.cpp`.
//!
//! Ludonimbus does not ship a trophy key. It uses the one the user already
//! set up: shadPS4's `TrophyKey`, or the "PS4 trophy key" in Ludonimbus's
//! achievement settings (kept on this PC only). Without a key only the icons
//! come out, and trophies show as numbers.
//!
//! Decrypted definitions and icons are cached under
//! `%APPDATA%\Ludonimbus\achievements\trophies\<NPWR>\` (never uploaded).

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub pos: usize,
    pub len: usize,
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn be64(d: &[u8], o: usize) -> Option<u64> {
    d.get(o..o + 8).map(|b| u64::from_be_bytes(b.try_into().unwrap()))
}

/// The entry table of a TRP file.
pub fn entries(d: &[u8]) -> Result<Vec<Entry>> {
    if be32(d, 0) != Some(0xDCA2_4D00) {
        bail!("not a trophy file");
    }
    let count = be32(d, 0x10).unwrap_or(0) as usize;
    let size = be32(d, 0x14).unwrap_or(0x40) as usize;
    if count > 4096 || size < 0x30 {
        bail!("bad trophy file header");
    }
    let mut out = Vec::new();
    for i in 0..count {
        let e = 0x60 + i * size;
        let Some(raw) = d.get(e..e + 32) else { break };
        let name: String = raw.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        let (Some(pos), Some(len)) = (be64(d, e + 32), be64(d, e + 40)) else { continue };
        let (pos, len) = (pos as usize, len as usize);
        if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") || pos.checked_add(len).is_none_or(|end| end > d.len()) {
            continue;
        }
        out.push(Entry { name, pos, len });
    }
    Ok(out)
}

/// A 32-character hexadecimal key.
pub fn parse_hex_key(s: &str) -> Option<[u8; 16]> {
    let s = s.trim();
    if s.len() != 32 {
        return None;
    }
    let mut k = [0u8; 16];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(k)
}

/// shadPS4's `config.toml` (installed version).
fn shadps4_configs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(a) = dirs::config_dir() {
        out.push(a.join("shadPS4").join("config.toml"));
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

/// A TOML string value (`key = "…"`), or the strings of an array value.
fn toml_strings(text: &str, key: &str) -> Vec<String> {
    let Some(line) = text.lines().find(|l| l.trim_start().starts_with(key) && l.contains('=')) else { return Vec::new() };
    let value = line.split_once('=').map(|(_, v)| v).unwrap_or("");
    value.split('"').skip(1).step_by(2).map(|s| s.replace("\\\\", "\\")).filter(|s| !s.is_empty()).collect()
}

/// Where the trophy key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeySource {
    Shadps4,
    Settings,
}

/// shadPS4's `TrophyKey`, when set.
fn shadps4_key() -> Option<[u8; 16]> {
    shadps4_configs().into_iter().find_map(|cfg| {
        let text = std::fs::read_to_string(cfg).ok()?;
        toml_strings(&text, "TrophyKey").first().and_then(|s| parse_hex_key(s))
    })
}

/// The trophy key the user set up: shadPS4's `TrophyKey`, then the one in
/// Ludonimbus's achievement settings. `None` when neither is set.
pub fn trophy_key() -> Option<([u8; 16], KeySource)> {
    if let Some(k) = shadps4_key() {
        return Some((k, KeySource::Shadps4));
    }
    super::settings().ps4_trophy_key.as_deref().and_then(parse_hex_key).map(|k| (k, KeySource::Settings))
}

fn file_key(key: &[u8; 16], npwr: &str) -> [u8; 16] {
    let mut block = [0u8; 16];
    let n = npwr.len().min(16);
    block[..n].copy_from_slice(&npwr.as_bytes()[..n]);
    // CBC with a zero IV over one block is a plain block encryption.
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut b = GenericArray::clone_from_slice(&block);
    cipher.encrypt_block(&mut b);
    b.into()
}

/// Decrypt one `.ESFM` entry (16-byte IV, then AES-128-CBC data) to XML text.
pub fn decrypt_esfm(entry: &[u8], npwr: &str, key: &[u8; 16]) -> Result<String> {
    if entry.len() <= 16 || !(entry.len() - 16).is_multiple_of(16) {
        bail!("bad encrypted trophy entry");
    }
    let cipher = Aes128::new(GenericArray::from_slice(&file_key(key, npwr)));
    let mut prev: [u8; 16] = entry[..16].try_into().unwrap();
    let mut out = Vec::with_capacity(entry.len() - 16);
    for chunk in entry[16..].as_chunks::<16>().0 {
        let mut b = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut b);
        for i in 0..16 {
            out.push(b[i] ^ prev[i]);
        }
        prev.copy_from_slice(chunk);
    }
    let text = String::from_utf8_lossy(&out).to_string();
    let Some(end) = text.find("</trophyconf>") else {
        bail!(crate::tr!("解不开奖杯定义：密钥或奖杯组编号不对", "Couldn't decrypt the trophy definitions: wrong key or trophy set id"));
    };
    let start = text.find('<').unwrap_or(0);
    Ok(text[start..end + "</trophyconf>".len()].to_string())
}

/// Copy the icons and the decrypted definitions of a TRP file into `out`
/// (`TROPnnn.PNG`, `TROP.XML`, `TROP_XX.XML`) with the user's trophy key.
/// Returns `(icons, definitions)`.
pub fn extract(trp: &Path, npwr: Option<&str>, out: &Path) -> Result<(usize, usize)> {
    extract_with(trp, npwr, out, trophy_key().map(|(k, _)| k).as_ref())
}

/// [`extract`] with an explicit key (`None`: icons only).
pub fn extract_with(trp: &Path, npwr: Option<&str>, out: &Path, key: Option<&[u8; 16]>) -> Result<(usize, usize)> {
    let data = std::fs::read(trp).with_context(|| trp.display().to_string())?;
    std::fs::create_dir_all(out)?;
    let (mut icons, mut defs) = (0, 0);
    for e in entries(&data)? {
        let upper = e.name.to_ascii_uppercase();
        let body = &data[e.pos..e.pos + e.len];
        if upper.ends_with(".PNG") {
            if body.starts_with(&[0x89, b'P', b'N', b'G']) {
                crate::util::atomic_write(&out.join(&upper), body)?;
                icons += 1;
            }
        } else if let Some(stem) = upper.strip_suffix(".ESFM") {
            let (Some(npwr), Some(key)) = (npwr.filter(|n| super::trophy::is_npwr(n)), key) else { continue };
            if !stem.starts_with("TROP") || stem.starts_with("TROPCONF") {
                continue; // TROPCONF has no names
            }
            let xml = decrypt_esfm(body, npwr, key)?;
            crate::util::atomic_write(&out.join(format!("{stem}.XML")), xml.as_bytes())?;
            defs += 1;
        } else if let Some(stem) = upper.strip_suffix(".SFM") {
            // Unencrypted (PS3 style).
            if stem.starts_with("TROP") && !stem.starts_with("TROPCONF") {
                crate::util::atomic_write(&out.join(format!("{stem}.XML")), body)?;
                defs += 1;
            }
        }
    }
    Ok((icons, defs))
}

/// Icons and decrypted definitions of a trophy set, extracted once from the
/// game's TRP file and kept in the local cache.
pub fn cached(trp: &Path, npwr: &str) -> Result<PathBuf> {
    let out = super::dir().join("trophies").join(npwr);
    let stamp = out.join(".source");
    let key = trophy_key().map(|(k, _)| k);
    // Extract again when the file changes or a key is set or changed later.
    let key_tag = key.map(|k| blake3::hash(&k).to_hex()[..8].to_string()).unwrap_or_else(|| "none".into());
    let source = format!("{}|{}|{key_tag}", trp.display(), std::fs::metadata(trp).map(|m| m.len()).unwrap_or(0));
    if std::fs::read_to_string(&stamp).ok().as_deref() == Some(source.as_str()) {
        return Ok(out);
    }
    if key.is_none() {
        // No names without a key: drop definitions decrypted with an earlier one.
        for e in std::fs::read_dir(&out).into_iter().flatten().flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    extract_with(trp, Some(npwr), &out, key.as_ref())?;
    crate::util::atomic_write(&stamp, source.as_bytes())?;
    Ok(out)
}

/// Game folders that may hold this trophy set: Lantern's game folder and
/// shadPS4's install folders (one level deep).
fn game_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = super::trophy::lantern_game_dir() {
        out.push(d);
    }
    for cfg in shadps4_configs() {
        let Ok(text) = std::fs::read_to_string(cfg) else { continue };
        for dir in toml_strings(&text, "installDirs") {
            let dir = PathBuf::from(dir);
            out.push(dir.clone());
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    out.push(e.path());
                }
            }
        }
    }
    out
}

/// The `trophy00.trp` of the game whose `npbind.dat` names this trophy set.
pub fn find_trp(npwr: &str) -> Option<PathBuf> {
    game_dirs().into_iter().find_map(|d| {
        let trp = d.join("sce_sys").join("trophy").join("trophy00.trp");
        (trp.is_file() && super::trophy::npbind_id(&d).is_some_and(|id| id.eq_ignore_ascii_case(npwr))).then_some(trp)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up key: the format doesn't care which key encrypted it.
    const TEST_KEY: [u8; 16] = *b"ludonimbus-test!";

    /// AES-128-CBC encryption, the inverse of `decrypt_esfm` (test only).
    fn encrypt_esfm(xml: &str, npwr: &str, key: &[u8; 16], iv: [u8; 16]) -> Vec<u8> {
        let cipher = Aes128::new(GenericArray::from_slice(&file_key(key, npwr)));
        let mut data = xml.as_bytes().to_vec();
        while !data.len().is_multiple_of(16) {
            data.push(0);
        }
        let mut out = iv.to_vec();
        let mut prev = iv;
        for chunk in data.chunks(16) {
            let mut block = [0u8; 16];
            for i in 0..16 {
                block[i] = chunk[i] ^ prev[i];
            }
            let mut b = GenericArray::clone_from_slice(&block);
            cipher.encrypt_block(&mut b);
            prev = b.into();
            out.extend_from_slice(&prev);
        }
        out
    }

    fn trp(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut d = vec![0u8; 0x60];
        d[0..4].copy_from_slice(&0xDCA24D00u32.to_be_bytes());
        d[0x10..0x14].copy_from_slice(&(entries.len() as u32).to_be_bytes());
        d[0x14..0x18].copy_from_slice(&0x40u32.to_be_bytes());
        let mut pos = 0x60 + entries.len() * 0x40;
        let mut table = Vec::new();
        for (name, body) in entries {
            let mut e = vec![0u8; 0x40];
            e[..name.len()].copy_from_slice(name.as_bytes());
            e[32..40].copy_from_slice(&(pos as u64).to_be_bytes());
            e[40..48].copy_from_slice(&(body.len() as u64).to_be_bytes());
            table.extend(e);
            pos += body.len();
        }
        d.extend(table);
        for (_, body) in entries {
            d.extend(body);
        }
        d
    }

    const XML: &str = r#"<?xml version="1.0" encoding="utf-8"?><trophyconf version="1.0"><title-name>Test</title-name><trophy id="000" hidden="no" ttype="P" pid="-1"><name>Everything</name><detail>All of it</detail></trophy><trophy id="001" hidden="yes" ttype="B" pid="000"><name>首个</name><detail>说明</detail></trophy></trophyconf>"#;

    #[test]
    fn decrypts_what_it_encrypts() {
        let enc = encrypt_esfm(XML, "NPWR00001_00", &TEST_KEY, [7u8; 16]);
        assert_eq!(decrypt_esfm(&enc, "NPWR00001_00", &TEST_KEY).unwrap(), XML);
        // Another trophy set id gives garbage, reported as an error.
        assert!(decrypt_esfm(&enc, "NPWR00002_00", &TEST_KEY).is_err());
    }

    #[test]
    fn extracts_icons_and_definitions() {
        let png = b"\x89PNG\r\n\x1a\nIMG".to_vec();
        let file = trp(&[
            ("TROPCONF.ESFM", encrypt_esfm("<trophyconf></trophyconf>", "NPWR00001_00", &TEST_KEY, [1u8; 16])),
            ("TROP.ESFM", encrypt_esfm(XML, "NPWR00001_00", &TEST_KEY, [2u8; 16])),
            ("TROP_11.ESFM", encrypt_esfm(XML, "NPWR00001_00", &TEST_KEY, [3u8; 16])),
            ("TROP000.PNG", png.clone()),
            ("ICON0.PNG", png.clone()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trophy00.trp");
        std::fs::write(&path, file).unwrap();
        let out = dir.path().join("out");
        assert_eq!(extract_with(&path, Some("NPWR00001_00"), &out, Some(&TEST_KEY)).unwrap(), (2, 2));
        assert_eq!(std::fs::read_to_string(out.join("TROP_11.XML")).unwrap(), XML);
        assert!(!out.join("TROPCONF.XML").exists());
        assert_eq!(std::fs::read(out.join("TROP000.PNG")).unwrap(), png);
        // Without the trophy set id only the icons come out.
        let out2 = dir.path().join("out2");
        assert_eq!(extract_with(&path, None, &out2, Some(&TEST_KEY)).unwrap(), (2, 0));
        // Without a key only the icons come out too.
        let out3 = dir.path().join("out3");
        assert_eq!(extract_with(&path, Some("NPWR00001_00"), &out3, None).unwrap(), (2, 0));
    }

    #[test]
    fn toml_values() {
        let text = "[General]\nTrophyKey = \"\"\n[GUI]\ninstallDirs = [\"C:\\\\Games\\\\PS4\", \"D:\\\\PS4\"]\n";
        assert!(toml_strings(text, "TrophyKey").is_empty());
        assert_eq!(toml_strings(text, "installDirs"), vec!["C:\\Games\\PS4".to_string(), "D:\\PS4".to_string()]);
        assert_eq!(parse_hex_key("000102030405060708090a0b0c0d0e0f").unwrap()[15], 15);
        assert!(parse_hex_key("xyz").is_none());
    }
}
