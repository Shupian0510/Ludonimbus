//! Xbox app (PC Game Pass) save containers.
//!
//! Games that save through the Xbox APIs keep their saves in
//! `%LOCALAPPDATA%\Packages\<package>\SystemAppData\wgs\<user>_<title>\`.
//! `containers.index` lists the containers (name → folder named by a GUID);
//! each container folder holds `container.N`, which lists the container's
//! files (name → data file named by a GUID), and the data files themselves.
//! Nothing is encrypted: a data file is the game's file as it is.
//!
//! The layout follows XGP-save-extractor (MIT, © 2021 Mark Mäkinen) and the
//! AbioticEditor Game Pass format reference, which also describes how to
//! write a container the way the Xbox app does ([`replace`]).

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const INDEX: &str = "containers.index";

/// Container states in the index.
pub const STATE_MODIFIED: u32 = 2;
pub const STATE_DELETED: u32 = 3;
pub const STATE_CREATED: u32 = 5;
/// Index flag: everything is uploaded to Xbox's cloud.
pub const FLAG_UPLOADED: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    pub version: u32,
    pub display_name: String,
    /// `<package family name>!<app id>`.
    pub package: String,
    /// What Xbox's sync compares: a newer local index wins.
    pub filetime: i64,
    pub flags: u32,
    pub root: String,
    pub reserved: [u8; 8],
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub name2: String,
    /// Issued by Xbox's cloud; never made up.
    pub etag: String,
    /// The `N` of `container.N`.
    pub number: u8,
    pub state: u32,
    pub guid: [u8; 16],
    pub filetime: i64,
    pub reserved: i64,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub header: u32,
    pub files: Vec<BlobRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlobRef {
    pub name: String,
    /// The data file as Xbox's cloud last knew it.
    pub cloud: [u8; 16],
    /// The data file on this PC (the same once a sync has finished).
    pub local: [u8; 16],
}

/// One container with its files' contents.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| anyhow!("truncated at byte {}", self.pos))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn guid(&mut self) -> Result<[u8; 16]> {
        Ok(self.take(16)?.try_into()?)
    }
    /// A string with its length in UTF-16 units first.
    fn wstr(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n > 4096 {
            bail!("string too long at byte {}", self.pos);
        }
        Ok(utf16(self.take(n * 2)?))
    }
}

fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
}

fn put_wstr(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.extend((units.len() as u32).to_le_bytes());
    for u in units {
        out.extend(u.to_le_bytes());
    }
}

pub fn parse_index(b: &[u8]) -> Result<Index> {
    let mut r = Reader { b, pos: 0 };
    let version = r.u32()?;
    let count = r.u32()? as usize;
    let display_name = r.wstr()?;
    let package = r.wstr()?;
    let filetime = r.i64()?;
    let flags = r.u32()?;
    let root = r.wstr()?;
    let reserved: [u8; 8] = r.take(8)?.try_into()?;
    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        entries.push(Entry {
            name: r.wstr()?,
            name2: r.wstr()?,
            etag: r.wstr()?,
            number: r.u8()?,
            state: r.u32()?,
            guid: r.guid()?,
            filetime: r.i64()?,
            reserved: r.i64()?,
            size: r.i64()?,
        });
    }
    Ok(Index { version, display_name, package, filetime, flags, root, reserved, entries })
}

pub fn write_index(i: &Index) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(i.version.to_le_bytes());
    out.extend((i.entries.len() as u32).to_le_bytes());
    put_wstr(&mut out, &i.display_name);
    put_wstr(&mut out, &i.package);
    out.extend(i.filetime.to_le_bytes());
    out.extend(i.flags.to_le_bytes());
    put_wstr(&mut out, &i.root);
    out.extend(i.reserved);
    for e in &i.entries {
        put_wstr(&mut out, &e.name);
        put_wstr(&mut out, &e.name2);
        put_wstr(&mut out, &e.etag);
        out.push(e.number);
        out.extend(e.state.to_le_bytes());
        out.extend(e.guid);
        out.extend(e.filetime.to_le_bytes());
        out.extend(e.reserved.to_le_bytes());
        out.extend(e.size.to_le_bytes());
    }
    out
}

pub fn parse_manifest(b: &[u8]) -> Result<Manifest> {
    let mut r = Reader { b, pos: 0 };
    let header = r.u32()?;
    let count = r.u32()? as usize;
    let mut files = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        files.push(BlobRef { name: utf16(r.take(128)?), cloud: r.guid()?, local: r.guid()? });
    }
    Ok(Manifest { header, files })
}

pub fn write_manifest(m: &Manifest) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend(m.header.to_le_bytes());
    out.extend((m.files.len() as u32).to_le_bytes());
    for f in &m.files {
        let units: Vec<u16> = f.name.encode_utf16().collect();
        if units.len() > 64 {
            bail!("file name too long for a container: {}", f.name);
        }
        let mut name = [0u8; 128];
        for (i, u) in units.iter().enumerate() {
            name[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        out.extend(name);
        out.extend(f.cloud);
        out.extend(f.local);
    }
    Ok(out)
}

/// A GUID as stored (little-endian fields) → how folders and data files are
/// named: 32 uppercase hex digits in the usual GUID order.
pub fn hex(guid: &[u8; 16]) -> String {
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    order.iter().map(|&i| format!("{:02X}", guid[i])).collect()
}

/// A new random (version 4) GUID, as stored.
pub fn new_guid() -> [u8; 16] {
    let mut g: [u8; 16] = rand::random();
    // Stored order: byte 7 holds the version, byte 8 the variant.
    g[7] = (g[7] & 0x0f) | 0x40;
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

/// The data file a manifest entry points at, among the files that exist.
fn blob_of(f: &BlobRef, exists: impl Fn(&str) -> bool) -> Option<String> {
    let (local, cloud) = (hex(&f.local), hex(&f.cloud));
    if exists(&local) {
        Some(local)
    } else if exists(&cloud) {
        Some(cloud)
    } else {
        None
    }
}

/// Read every container (paths relative to the wgs user folder, `/`
/// separated). Deleted containers and files whose data is missing are
/// left out.
pub fn read_all(read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<(Index, Vec<Container>)> {
    let idx = parse_index(&read(INDEX).ok_or_else(|| anyhow!("{INDEX} missing"))?).context(INDEX)?;
    let mut out = Vec::new();
    for e in idx.entries.iter().filter(|e| e.state != STATE_DELETED) {
        let folder = hex(&e.guid);
        let Some(m) = read(&format!("{folder}/container.{}", e.number)) else { continue };
        let m = parse_manifest(&m).with_context(|| format!("{folder}/container.{}", e.number))?;
        let mut files = Vec::new();
        for f in &m.files {
            let found = blob_of(f, |h| read(&format!("{folder}/{h}")).is_some());
            if let Some(data) = found.and_then(|h| read(&format!("{folder}/{h}"))) {
                files.push((f.name.clone(), data));
            }
        }
        out.push(Container { name: e.name.clone(), files });
    }
    Ok((idx, out))
}

/// Read the containers in the wgs user folder `dir` on this PC.
pub fn read_dir(dir: &Path) -> Result<(Index, Vec<Container>)> {
    read_all(&|rel: &str| std::fs::read(rel.split('/').fold(dir.to_path_buf(), |p, c| p.join(c))).ok())
}

/// FILETIME (100 ns since 1601) of now, to the millisecond.
pub fn filetime_now() -> i64 {
    let ms = chrono::Utc::now().timestamp_millis();
    ms * 10_000 + 116_444_736_000_000_000
}

/// Seconds since 1970 of a FILETIME (0 for one before 1970).
pub fn unix_secs(filetime: i64) -> i64 {
    ((filetime - 116_444_736_000_000_000) / 10_000_000).max(0)
}

/// Whether a path (either slash) is a container index: a
/// `containers.index` under `wgs`, or in a `<user id>_<title id>` folder.
fn is_user_index(path: &str) -> bool {
    let l = path.replace('\\', "/").to_lowercase();
    let Some(dir) = l.strip_suffix(&format!("/{INDEX}")) else { return false };
    let folder = dir.rsplit('/').next().unwrap_or("");
    let hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    folder.split_once('_').is_some_and(|(a, b)| hex(a) && hex(b)) || l.contains("/wgs/")
}

/// The wgs user folders (the folders that hold a [`INDEX`]) among the paths
/// of a game's files (either slash, as they are written): a container
/// index known by its place, or one next to `<GUID>/container.N` files.
pub fn user_dirs<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let all: Vec<String> = paths.map(|p| p.replace('\\', "/")).collect();
    let lower: Vec<String> = all.iter().map(|p| p.to_lowercase()).collect();
    let suffix = format!("/{INDEX}");
    all.iter()
        .zip(&lower)
        .filter_map(|(p, l)| {
            let dir = l.strip_suffix(&suffix)?;
            let beside = || {
                lower.iter().any(|q| {
                    let rest = q.strip_prefix(dir).and_then(|r| r.strip_prefix('/'));
                    rest.and_then(|r| r.split_once('/'))
                        .is_some_and(|(folder, file)| folder.len() == 32 && folder.chars().all(|c| c.is_ascii_hexdigit()) && file.starts_with("container."))
                })
            };
            (is_user_index(p) || beside()).then(|| p[..p.len() - suffix.len()].to_string())
        })
        .collect()
}

fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    crate::util::atomic_write(path, data)
}

/// Files of a wgs user folder replaced by [`begin`], with what they replaced
/// still there: [`Replacement::keep`] makes it final, [`Replacement::undo`]
/// puts the folder back as it was.
///
/// The index is what says which manifest and which data files are the
/// containers' content, and it is written last, in one piece: until then
/// the folder reads as before, from then on as after. A write cut short
/// leaves either, plus files nothing points at.
#[must_use = "keep or undo the replacement"]
pub struct Replacement {
    dir: PathBuf,
    /// The index as it was, byte for byte.
    old_index: Vec<u8>,
    /// The data files and manifests written.
    written: Vec<PathBuf>,
    /// The ones the new index no longer points at.
    dropped: Vec<PathBuf>,
    /// How many files were replaced.
    pub replaced: usize,
}

/// Start replacing the contents of existing files in the wgs user folder
/// `dir` (`(container, file, data)`), the way the Xbox app writes them: a
/// new data file, `container.(N+1)` pointing at it, the index entry updated
/// (number, size, time, state) and the index time moved forward with
/// "uploaded" cleared, so Xbox's sync sees a local change. The old manifest
/// and data files stay until [`Replacement::keep`]. Only existing containers
/// and files are touched; nothing is written when none of them is named.
/// When a step fails, what was written is removed again and the folder is
/// as it was.
pub fn begin(dir: &Path, changes: &[(String, String, Vec<u8>)]) -> Result<Replacement> {
    let index_path = dir.join(INDEX);
    let old_index = std::fs::read(&index_path).with_context(|| index_path.display().to_string())?;
    let mut r = Replacement { dir: dir.to_path_buf(), old_index, written: Vec::new(), dropped: Vec::new(), replaced: 0 };
    match stage(&mut r, changes) {
        Ok(()) => Ok(r),
        Err(e) => {
            for f in &r.written {
                let _ = std::fs::remove_file(f);
            }
            Err(e)
        }
    }
}

fn stage(r: &mut Replacement, changes: &[(String, String, Vec<u8>)]) -> Result<()> {
    let dir = r.dir.clone();
    let mut idx = parse_index(&r.old_index)?;
    let now = filetime_now();
    let names: HashSet<&str> = changes.iter().map(|(c, _, _)| c.as_str()).collect();
    for e in idx.entries.iter_mut().filter(|e| e.state != STATE_DELETED && names.contains(e.name.as_str())) {
        let folder = dir.join(hex(&e.guid));
        let old_manifest = folder.join(format!("container.{}", e.number));
        let mut m = parse_manifest(&std::fs::read(&old_manifest).with_context(|| old_manifest.display().to_string())?)?;
        let next = e.number.checked_add(1).ok_or_else(|| anyhow!("container {} can't be numbered further", e.name))?;
        let mut touched = false;
        for f in m.files.iter_mut() {
            let Some((_, _, data)) = changes.iter().find(|(c, n, _)| c == &e.name && n == &f.name) else { continue };
            let g = new_guid();
            let blob = folder.join(hex(&g));
            r.written.push(blob.clone());
            write_new(&blob, data)?;
            for old in [f.local, f.cloud] {
                r.dropped.push(folder.join(hex(&old)));
            }
            f.local = g;
            f.cloud = g;
            touched = true;
            r.replaced += 1;
        }
        if !touched {
            continue;
        }
        let manifest = folder.join(format!("container.{next}"));
        r.written.push(manifest.clone());
        write_new(&manifest, &write_manifest(&m)?)?;
        r.dropped.push(old_manifest);
        let mut size = 0i64;
        for f in &m.files {
            if let Some(h) = blob_of(f, |h| folder.join(h).is_file()) {
                size += std::fs::metadata(folder.join(h)).map(|m| m.len() as i64).unwrap_or(0);
            }
        }
        e.number = next;
        e.size = size;
        e.filetime = now;
        e.state = if e.etag.is_empty() { STATE_CREATED } else { STATE_MODIFIED };
    }
    if r.replaced == 0 {
        return Ok(());
    }
    idx.filetime = now.max(idx.filetime + 1);
    idx.flags &= !FLAG_UPLOADED;
    write_new(&dir.join(INDEX), &write_index(&idx))
}

impl Replacement {
    /// Make it final: what was replaced is deleted, except data files a
    /// manifest still in use points at.
    pub fn keep(self) {
        if self.replaced == 0 {
            return;
        }
        let dropping: HashSet<&Path> = self.dropped.iter().map(|p| p.as_path()).collect();
        let in_use: HashSet<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|d| d.path().is_dir())
            .flat_map(|d| {
                let folder = d.path();
                std::fs::read_dir(&folder)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|f| f.file_name().to_string_lossy().starts_with("container.") && !dropping.contains(f.path().as_path()))
                    .filter_map(|f| std::fs::read(f.path()).ok().and_then(|b| parse_manifest(&b).ok()))
                    .flat_map(|m| m.files.into_iter().flat_map(|f| [hex(&f.local), hex(&f.cloud)]))
                    .collect::<Vec<_>>()
            })
            .collect();
        for f in &self.dropped {
            let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !name.starts_with("container.") && in_use.contains(&name) {
                continue;
            }
            let _ = std::fs::remove_file(f);
        }
    }

    /// Put the folder back as it was: the old index, byte for byte, and
    /// without the files written. When the index can't be written, the new
    /// content stays complete and readable, and the error says so.
    pub fn undo(self) -> Result<()> {
        if self.replaced == 0 {
            return Ok(());
        }
        write_new(&self.dir.join(INDEX), &self.old_index)?;
        for f in &self.written {
            let _ = std::fs::remove_file(f);
        }
        Ok(())
    }
}

/// Replace the contents of existing files in the wgs user folder `dir`
/// (`(container, file, data)`) the way the Xbox app does ([`begin`]), read
/// them back, and only then delete what they replaced. A file that doesn't
/// read back as given puts the folder back as it was. Returns the number of
/// files replaced.
pub fn replace(dir: &Path, changes: &[(String, String, Vec<u8>)]) -> Result<usize> {
    let r = begin(dir, changes)?;
    let check = (|| -> Result<()> {
        if r.replaced == 0 {
            return Ok(());
        }
        let (_, list) = read_dir(dir)?;
        for (c, f, data) in changes {
            let Some(container) = list.iter().find(|x| &x.name == c) else { continue };
            if container.files.iter().any(|(n, d)| n == f && d != data) {
                bail!("{c} / {f} read back differently after it was written");
            }
        }
        Ok(())
    })();
    match check {
        Ok(()) => {
            let n = r.replaced;
            r.keep();
            Ok(n)
        }
        Err(e) => match r.undo() {
            Ok(()) => Err(e),
            Err(u) => Err(e.context(format!("the folder couldn't be put back as it was ({u:#}); it has the new content"))),
        },
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A wgs user folder with the given containers, as the Xbox app writes it.
    pub fn make(dir: &Path, package: &str, containers: &[(&str, &[(&str, &[u8])])]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut entries = Vec::new();
        for (i, (name, files)) in containers.iter().enumerate() {
            let guid = new_guid();
            let folder = dir.join(hex(&guid));
            std::fs::create_dir_all(&folder).unwrap();
            let mut refs = Vec::new();
            let mut size = 0;
            for (fname, data) in files.iter() {
                let g = new_guid();
                std::fs::write(folder.join(hex(&g)), data).unwrap();
                size += data.len() as i64;
                refs.push(BlobRef { name: fname.to_string(), cloud: g, local: g });
            }
            let number = (i as u8) + 1;
            std::fs::write(folder.join(format!("container.{number}")), write_manifest(&Manifest { header: 4, files: refs }).unwrap()).unwrap();
            entries.push(Entry {
                name: name.to_string(),
                name2: name.to_string(),
                etag: "\"0x8DEBCCC41BE9635\"".into(),
                number,
                state: 1,
                guid,
                filetime: 133_000_000_000_000_000,
                reserved: 0,
                size,
            });
        }
        let idx = Index {
            version: 14,
            display_name: String::new(),
            package: format!("{package}!App"),
            filetime: 133_000_000_000_000_000,
            flags: 3,
            root: "{8F2A0000-0000-0000-0000-000000000000}".into(),
            reserved: [0, 0, 0, 0x10, 0, 0, 0, 0],
            entries,
        };
        std::fs::write(dir.join(INDEX), write_index(&idx)).unwrap();
    }

    #[test]
    fn guid_names() {
        // Stored little-endian: 33221100-5544-7766-8899-AABBCCDDEEFF.
        let g = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        assert_eq!(hex(&g), "33221100554477668899AABBCCDDEEFF");
        let n = new_guid();
        assert_eq!(hex(&n).as_bytes()[12], b'4');
    }

    #[test]
    fn reads_and_replaces_like_the_xbox_app() {
        let tmp = tempfile::tempdir().unwrap();
        let dir: PathBuf = tmp.path().join("wgs").join("000901F2A1B2C3D4_000000000000000000000000646E3F8B");
        make(&dir, "Pocketpair.Palworld_ad4psfrxyesvt", &[("Slot1", &[("Data", b"one")]), ("Slot2", &[("Data", b"two"), ("Meta", b"m")])]);
        let (idx, list) = read_dir(&dir).unwrap();
        assert_eq!(idx.package, "Pocketpair.Palworld_ad4psfrxyesvt!App");
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].files, vec![("Data".to_string(), b"two".to_vec()), ("Meta".to_string(), b"m".to_vec())]);
        // The index survives a write unchanged.
        let raw = std::fs::read(dir.join(INDEX)).unwrap();
        assert_eq!(write_index(&parse_index(&raw).unwrap()), raw);

        let n = replace(&dir, &[("Slot2".into(), "Data".into(), b"TWO!".to_vec()), ("Nope".into(), "Data".into(), b"x".to_vec())]).unwrap();
        assert_eq!(n, 1);
        let (after, list) = read_dir(&dir).unwrap();
        assert_eq!(list[1].files, vec![("Data".to_string(), b"TWO!".to_vec()), ("Meta".to_string(), b"m".to_vec())]);
        assert_eq!(list[0].files, vec![("Data".to_string(), b"one".to_vec())]);
        let e = &after.entries[1];
        assert_eq!(e.number, 3);
        assert_eq!(e.state, STATE_MODIFIED);
        assert_eq!(e.size, 5);
        assert!(after.filetime > idx.filetime);
        assert_eq!(after.flags & FLAG_UPLOADED, 0);
        assert_eq!(after.entries[0], idx.entries[0]);
        // Only the new manifest and data files are left in the container.
        let folder = dir.join(hex(&e.guid));
        let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"container.3".to_string()));
    }

    /// Every file below `dir` with its content.
    pub fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(dir)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(e.path()).unwrap()))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn a_replacement_is_undone_byte_for_byte_and_kept_only_when_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("0009_00AB");
        make(&dir, "Some.Game_x", &[("Slot1", &[("Data", b"one")]), ("Slot2", &[("Data", b"two")])]);
        let before = tree(&dir);
        let r = begin(&dir, &[("Slot1".into(), "Data".into(), b"ONE".to_vec())]).unwrap();
        assert_eq!(r.replaced, 1);
        // The new content is what the folder reads as, the old one is still there.
        let (_, list) = read_dir(&dir).unwrap();
        assert_eq!(list[0].files[0].1, b"ONE");
        assert_eq!(tree(&dir).len(), before.len() + 2);
        r.undo().unwrap();
        assert_eq!(tree(&dir), before);
        // Kept: what it replaced is gone.
        let r = begin(&dir, &[("Slot1".into(), "Data".into(), b"ONE".to_vec())]).unwrap();
        r.keep();
        assert_eq!(tree(&dir).len(), before.len());
        assert_eq!(read_dir(&dir).unwrap().1[0].files[0].1, b"ONE");
        // Nothing named that the folder has: nothing written.
        let after = tree(&dir);
        let r = begin(&dir, &[("Nope".into(), "Data".into(), b"x".to_vec())]).unwrap();
        assert_eq!(r.replaced, 0);
        r.undo().unwrap();
        assert_eq!(tree(&dir), after);
    }

    #[test]
    fn finds_container_folders() {
        let p = ["{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB/containers.index", "{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB/ABC/container.1"];
        assert_eq!(user_dirs(p.iter().copied()), vec!["{localappdata}/Packages/X_y/SystemAppData/wgs/0009_00AB".to_string()]);
        assert!(user_dirs(["{appdata}/Game/containers.index"].iter().copied()).is_empty());
        // Next to its containers, wherever it is.
        let p = ["data/containers.index", "data/0123456789ABCDEF0123456789ABCDEF/container.3", "data/other.txt"];
        assert_eq!(user_dirs(p.iter().copied()), vec!["data".to_string()]);
        assert!(is_user_index("{pk}/000901F2A1B2C3D4_000000000000000000000000646E3F8B/containers.index"));
        assert!(is_user_index(r"C:\Users\me\AppData\Local\Packages\X_y\SystemAppData\wgs\t\containers.index"));
        assert_eq!(unix_secs(116_444_736_000_000_000 + 10_000_000 * 60), 60);
    }
}
