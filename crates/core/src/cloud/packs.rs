//! Content packs: where the bytes of a snapshot are (docs/plan-v2.md,
//! section 3.4).
//!
//! A pack is an ordinary zip: one entry `files/<canonical path>` per file,
//! plus `snapshot.json`, a copy of the manifest it was written for. A
//! snapshot's manifest names, per file, the pack that holds it
//! ([`super::v2::FileRef::pack`]; none = the pack named like the snapshot).
//! Packs belong to the game, not to a save line:
//! `games/<folder>/packs/<pack id>.zip`. A snapshot of the 0.6 layout
//! (`v` < 2) has its one pack beside its manifest: `<line dir>/<id>.zip`.
//!
//! What is here, in the order a sync uses it:
//!
//! - [`plan`] (pure) decides which files of a new snapshot go into which new
//!   pack and which are referenced from the packs of the base, the snapshot
//!   this device synced last. Alongside an upload it also takes the files
//!   still in use out of a pack that is mostly dead weight ([`PACK_SIZES`]).
//! - [`upload`] builds the new packs as zips ([`build`]) and uploads them.
//!   The caller then writes the manifest ([`super::line::write_manifest`]):
//!   that is what makes the snapshot exist.
//! - [`verify`], once the manifest is written, looks whether the packs it
//!   refers to are still there and puts back what another device cleaned up
//!   in between.
//! - [`read`] fetches the packs a manifest needs (all files, or a chosen
//!   subset), checks every file against the manifest and only then puts the
//!   files into the destination. A pack that is gone is [`MissingPack`].
//! - [`unused`] (pure) names the packs no manifest refers to any more.
//!
//! | operation | requests |
//! |---|---|
//! | [`upload`] | 1 upload per new pack |
//! | [`verify`], a snapshot made just now with all its files in its own packs | none |
//! | [`verify`], otherwise | 1 list, and 1 upload per pack put back |
//! | [`read`] | 1 download per pack that holds a wanted file |
//!
//! Sizes and thresholds are the policy of whoever writes, not part of the
//! format: a reader only needs to follow `in`.

use super::store::{join, Entry, Store};
use super::v2::{self, Extra, FileRef, Manifest};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MIB: u64 = 1024 * 1024;

/// How long a pack no manifest uses is left alone unless the caller says
/// otherwise. It may be an upload in progress (the manifest is written
/// last), or one that broke off and will be continued.
pub const GRACE_DAYS: i64 = 7;

/// [`unused`] never names a pack younger than this, whatever grace period it
/// is given. [`verify`] relies on it: the packs a device has just uploaded
/// for a snapshot it has just made need no second look.
const FRESH_HOURS: i64 = 24;

/// The manifest field (an extension, docs/plan-v2.md section 3.9) that says
/// how many bytes each pack a snapshot uses holds, before compression and
/// with the files no snapshot uses any more: what [`plan`] goes by to see
/// that a pack is mostly dead weight. Written for large saves only, and
/// only for the packs whose size is known.
pub const PACK_SIZES: &str = "packSizes";

/// The packs a snapshot uses may hold this many times its content before
/// the next upload tidies up.
const REPACK_ABOVE: u64 = 2;

/// From this size on a zip entry is written with 64-bit sizes. A little
/// below 4 GiB, because a file that doesn't compress grows slightly.
const ZIP64_FROM: u64 = 0xF000_0000;

// --------------------------------------------------------------------- plan

/// When a writer uploads everything and when it splits what it uploads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// A save of at most this many bytes always gets one complete pack: the
    /// newest zip in the cloud is then the newest progress, and can be
    /// fetched by hand without any other file.
    pub full_below: u64,
    /// More new content than this is split into several packs, so that an
    /// upload that breaks off only loses the pack it was sending.
    pub split_above: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { full_below: 16 * MIB, split_above: 64 * MIB }
    }
}

/// A save file on this device, as it goes into a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    /// Canonical path, e.g. `saves/Farm_1/SaveGameInfo`.
    pub path: String,
    pub size: u64,
    /// blake3 of the content, in hex.
    pub hash: String,
    pub mtime: i64,
    /// Where the bytes are on this device.
    pub source: PathBuf,
}

impl LocalFile {
    /// Describe the file at `source` as `path` of a snapshot. The hash is
    /// the one 0.6 uses, so the same content compares equal across layouts.
    pub fn from_disk(path: impl Into<String>, source: impl Into<PathBuf>) -> Result<LocalFile> {
        let source = source.into();
        let mut file = File::open(&source).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", source.display()))?;
        let mtime = file.metadata().ok().and_then(|m| m.modified().ok()).map(|t| DateTime::<Utc>::from(t).timestamp()).unwrap_or(0);
        let (size, hash) = copy_hashed(&mut file, &mut std::io::sink(), u64::MAX)?;
        Ok(LocalFile { path: path.into(), size, hash, mtime, source })
    }
}

/// A pack to build and upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPack {
    pub id: String,
    /// Canonical paths of the files in it, in the order they are packed.
    pub files: Vec<String>,
    /// Their size together, before compression.
    pub size: u64,
}

/// What [`plan`] decided.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// The packs to upload, `<id>` first. None when nothing changed.
    pub packs: Vec<NewPack>,
    /// The manifest's `files`, sorted by path, each saying where its content
    /// is.
    pub files: Vec<FileRef>,
    /// For the manifest's [`PACK_SIZES`]: what every pack the snapshot uses
    /// holds, as far as known. Empty for a save uploaded whole.
    pub sizes: BTreeMap<String, u64>,
}

impl Plan {
    /// Bytes to upload, before compression.
    pub fn upload_size(&self) -> u64 {
        self.packs.iter().map(|p| p.size).sum()
    }
}

/// Decide where the content of a new snapshot `id` goes.
///
/// `base` is the snapshot the local saves were last in sync with (the one
/// this device uploaded or pulled last), of the same game. Pass `None` when
/// there is none, or when it is so old that the cloud may no longer keep it
/// for this device (section 5.4).
///
/// - A save of at most `full_below` bytes, or one without a usable base:
///   every file goes into the snapshot's own packs.
/// - Otherwise a file with the path, size and hash of a file of the base is
///   referenced from the pack the base says it is in, and only the rest is
///   packed. A renamed file counts as new.
/// - New content is filled into `<id>`, `<id>-2`, ... in the order of the
///   paths, a pack being closed when the next file would take it past
///   `split_above`. Every file is whole in one pack; a file larger than the
///   limit is alone in its pack.
/// - Tidying up: when the packs the snapshot would use hold more than twice
///   its content (the base says what each holds, [`PACK_SIZES`]), the
///   files still taken from the pack with the most dead weight are packed
///   anew, so that the pack can go once the older snapshots do. Only
///   alongside an upload (a snapshot that brings nothing new stays a
///   manifest alone), and never more than the upload itself, or
///   `full_below` when that is more.
///
/// The result depends on nothing but the arguments, not even on the order of
/// `local` (which holds each path once).
pub fn plan(id: &str, local: &[LocalFile], base: Option<&Manifest>, policy: &Policy) -> Plan {
    let mut sorted: Vec<&LocalFile> = local.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let total = sorted.iter().fold(0u64, |sum, f| sum.saturating_add(f.size));
    let large = total > policy.full_below;

    // What may be referenced: only what the base itself uses, because that is
    // all the cloud is known to keep for this device.
    let base = base.filter(|b| large && can_reference(b));
    let kept: BTreeMap<&str, (&FileRef, &str)> = match base {
        Some(b) => b.files.iter().map(|f| (f.path.as_str(), (f, pack_of(b, f)))).collect(),
        None => BTreeMap::new(),
    };
    // The files that are as the base has them, and the pack each is in.
    let mut borrowed: BTreeMap<&str, &str> = sorted
        .iter()
        .filter_map(|f| {
            let (b, pack) = kept.get(f.path.as_str())?;
            (b.size == f.size && b.hash == f.hash).then_some((f.path.as_str(), *pack))
        })
        .collect();
    let known = base.map(pack_sizes).unwrap_or_default();
    repack(&sorted, &mut borrowed, &known, total, policy);

    let mut plan = Plan::default();
    for f in sorted {
        let pack = match borrowed.get(f.path.as_str()) {
            Some(pack) => Some(pack.to_string()),
            None => {
                let full = plan.packs.last().is_none_or(|p| p.size.saturating_add(f.size) > policy.split_above);
                if full {
                    let n = plan.packs.len() + 1;
                    let id = if n == 1 { id.to_string() } else { format!("{id}-{n}") };
                    plan.packs.push(NewPack { id, files: Vec::new(), size: 0 });
                }
                let first = plan.packs.len() == 1;
                let p = plan.packs.last_mut().expect("a pack was just opened");
                p.files.push(f.path.clone());
                p.size = p.size.saturating_add(f.size);
                // No `in` = the pack named like the snapshot.
                (!first).then(|| p.id.clone())
            }
        };
        plan.files.push(FileRef { path: f.path.clone(), size: f.size, hash: f.hash.clone(), mtime: f.mtime, pack, extra: Extra::new() });
    }
    if large {
        plan.sizes = plan.packs.iter().map(|p| (p.id.clone(), p.size)).collect();
        for pack in borrowed.values() {
            if let Some(size) = known.get(*pack) {
                plan.sizes.insert(pack.to_string(), *size);
            }
        }
    }
    plan
}

/// What a manifest says its packs hold ([`PACK_SIZES`]).
fn pack_sizes(manifest: &Manifest) -> BTreeMap<String, u64> {
    let sizes = manifest.extra.get(PACK_SIZES).and_then(|v| v.as_object());
    sizes.map(|m| m.iter().filter_map(|(pack, size)| Some((pack.clone(), size.as_u64()?))).collect()).unwrap_or_default()
}

/// The tidying up of [`plan`]: while the packs the snapshot would use hold
/// more than [`REPACK_ABOVE`] times its content, stop taking files from the
/// pack with the most dead weight (they are packed anew with the rest). A
/// pack is only given up when it holds more than that many times what is
/// still taken from it: sending 3 MB again to get rid of 1 is no gain.
/// `known` says what a pack holds; one it doesn't name counts as holding
/// just what is taken from it.
fn repack<'a>(sorted: &[&'a LocalFile], borrowed: &mut BTreeMap<&'a str, &'a str>, known: &BTreeMap<String, u64>, total: u64, policy: &Policy) {
    let size_of: BTreeMap<&str, u64> = sorted.iter().map(|f| (f.path.as_str(), f.size)).collect();
    let taken = |borrowed: &BTreeMap<&'a str, &'a str>| {
        let mut per_pack: BTreeMap<&'a str, u64> = BTreeMap::new();
        for (path, pack) in borrowed {
            let n = per_pack.entry(*pack).or_default();
            *n = n.saturating_add(size_of.get(path).copied().unwrap_or(0));
        }
        per_pack
    };
    let fresh = total.saturating_sub(taken(borrowed).values().sum());
    if fresh == 0 {
        return;
    }
    let mut budget = fresh.max(policy.full_below);
    loop {
        let per_pack = taken(borrowed);
        let live: u64 = per_pack.values().sum();
        let held =
            per_pack.iter().fold(total.saturating_sub(live), |sum, (pack, used)| sum.saturating_add(known.get(*pack).copied().unwrap_or(*used).max(*used)));
        if held <= total.saturating_mul(REPACK_ABOVE) {
            return;
        }
        // Of the packs that are themselves mostly dead weight, the one with
        // the most first; among equals the same one every time.
        let worst = per_pack
            .iter()
            .filter(|(_, used)| **used <= budget)
            .filter_map(|(pack, used)| {
                let holds = *known.get(*pack)?;
                (holds > used.saturating_mul(REPACK_ABOVE)).then(|| (holds - *used, *pack, *used))
            })
            .max();
        let Some((_, pack, used)) = worst else { return };
        borrowed.retain(|_, p| *p != pack);
        budget -= used;
    }
}

/// Whether a new snapshot may point into the packs of `base`. A 0.6 snapshot
/// keeps its zip beside its manifest, where `in` can't point. A snapshot
/// with features this build doesn't know may store its files in a way this
/// build can't see.
fn can_reference(base: &Manifest) -> bool {
    base.v >= 2 && base.unsupported().is_empty()
}

/// The pack that holds a file of `manifest`.
fn pack_of<'a>(manifest: &'a Manifest, file: &'a FileRef) -> &'a str {
    file.pack.as_deref().unwrap_or(&manifest.id)
}

// -------------------------------------------------------------------- write

/// Write the pack `pack` of `manifest` as `<staging>/<pack>.zip`: the files
/// the manifest says are in it, taken from this device, and a copy of the
/// manifest. Returns the zip's path.
///
/// A file whose content is no longer what the manifest says is an error:
/// readers check against the manifest, and a pack that doesn't match it
/// would make the snapshot unreadable for good.
pub fn build(manifest: &Manifest, pack: &str, local: &[LocalFile], staging: &Path) -> Result<PathBuf> {
    if !is_file_name(pack) {
        bail!(crate::tr!("内容包的名字无效：{pack}", "Invalid content pack name: {pack}"));
    }
    let on_disk: BTreeMap<&str, &LocalFile> = local.iter().map(|l| (l.path.as_str(), l)).collect();
    let mut members: Vec<(&FileRef, &LocalFile)> = Vec::new();
    for f in manifest.files.iter().filter(|f| pack_of(manifest, f) == pack) {
        check_path(&f.path)?;
        let l = on_disk.get(f.path.as_str()).filter(|l| l.size == f.size && l.hash == f.hash).ok_or_else(|| {
            anyhow!(crate::tr!("本机没有内容包 {pack} 需要的文件 {}", "This device doesn't have the file {} that the content pack {pack} needs", f.path))
        })?;
        members.push((f, l));
    }
    if members.is_empty() {
        bail!(crate::tr!("这个版本没有文件在内容包 {pack} 里", "This version has no files in the content pack {pack}"));
    }
    std::fs::create_dir_all(staging).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", staging.display()))?;
    let zip_path = staging.join(format!("{pack}.zip"));
    let written = write_zip(&zip_path, manifest, &members);
    if written.is_err() {
        let _ = std::fs::remove_file(&zip_path);
    }
    written?;
    Ok(zip_path)
}

fn write_zip(zip_path: &Path, manifest: &Manifest, members: &[(&FileRef, &LocalFile)]) -> Result<()> {
    let file = File::create(zip_path).with_context(|| crate::tr!("无法创建 {}", "Can't create {}", zip_path.display()))?;
    let mut zw = zip::ZipWriter::new(file);
    // What 0.6 writes its snapshots with (snapshot.rs): a pack opens
    // wherever one of those does.
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (f, l) in members {
        zw.start_file(format!("files/{}", f.path), opts.large_file(f.size >= ZIP64_FROM))?;
        let mut src = File::open(&l.source).with_context(|| crate::tr!("无法读取存档文件 {}", "Can't read the save file {}", l.source.display()))?;
        // Hashed on the way in: the file may have been written to since it
        // was scanned.
        let (size, hash) = copy_hashed(&mut src, &mut zw, f.size.saturating_add(1))?;
        if size != f.size || hash != f.hash {
            bail!(crate::tr!("存档文件 {} 在打包时发生了变化", "The save file {} changed while it was being packed", l.source.display()));
        }
    }
    zw.start_file("snapshot.json", opts)?;
    zw.write_all(&serde_json::to_vec_pretty(manifest)?)?;
    zw.finish()?;
    Ok(())
}

/// Build and upload the packs `manifest` brings itself (`<id>`, `<id>-2`,
/// ...) into `folder`, the game's first folder. One upload per pack, one zip
/// in `staging` at a time. The manifest is written by the caller afterwards.
pub fn upload(store: &dyn Store, folder: &str, manifest: &Manifest, local: &[LocalFile], staging: &Path) -> Result<()> {
    let mut own: Vec<String> = manifest.packs().into_iter().filter(|p| is_own(p, &manifest.id)).collect();
    // `<id>`, `<id>-2`, ... `<id>-10`: shorter names first puts them in the
    // order they were planned.
    own.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    for pack in &own {
        put(store, folder, manifest, pack, local, staging)?;
    }
    Ok(())
}

fn put(store: &dyn Store, folder: &str, manifest: &Manifest, pack: &str, local: &[LocalFile], staging: &Path) -> Result<()> {
    let zip = build(manifest, pack, local, staging)?;
    let sent = store.upload(&v2::pack_path(folder, pack), &zip);
    let _ = std::fs::remove_file(&zip);
    sent
}

/// Call after the manifest is written: make sure the packs it refers to are
/// in the cloud. Another device may have cleaned one up between the moment
/// it was planned to be used and the moment the manifest appeared. A pack
/// that is gone is uploaded again under its own id, with the files this
/// manifest takes from it (manifests never change, so the pack is what gets
/// repaired). Returns the packs put back.
///
/// `folders` are the game's folders; what is put back goes into the first.
/// One listing normally. None for a snapshot made within the last day whose
/// files are all in its own packs: those were uploaded a moment ago and no
/// clean-up touches a pack that young.
///
/// A pack it borrows that is gone while `local` doesn't have every file the
/// manifest takes from it, as the manifest has it, can't be put back: the
/// content is nowhere any more. That is a [`MissingPack`], and nothing is
/// uploaded. One of its own packs that `local` can't rebuild is an ordinary
/// error: the caller still has it as a zip and can upload it again.
pub fn verify(store: &dyn Store, folders: &[String], manifest: &Manifest, local: &[LocalFile], staging: &Path) -> Result<Vec<String>> {
    // A snapshot that waited in the queue has an old id: its own packs look
    // old to a clean-up from the moment they are uploaded, and are checked
    // like the ones it borrows.
    let fresh = pack_time(&manifest.id).is_some_and(|made| Utc::now().signed_duration_since(made) < chrono::Duration::hours(FRESH_HOURS));
    let mut missing: BTreeSet<String> = manifest.packs().into_iter().filter(|p| !(fresh && is_own(p, &manifest.id))).collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let Some(home) = folders.first() else {
        bail!(crate::tr!("这个游戏在云端没有文件夹", "This game has no folder in the cloud"));
    };
    for folder in folders {
        let present = present(store, folder)?;
        missing.retain(|p| !present.contains(p));
        if missing.is_empty() {
            return Ok(Vec::new());
        }
    }
    let at_hand = |pack: &str| {
        manifest.files.iter().filter(|f| pack_of(manifest, f) == pack).all(|f| local.iter().any(|l| l.path == f.path && l.size == f.size && l.hash == f.hash))
    };
    if let Some(gone) = missing.iter().find(|p| !is_own(p.as_str(), &manifest.id) && !at_hand(p.as_str())) {
        return Err(MissingPack { pack: gone.clone() }.into());
    }
    for pack in &missing {
        put(store, home, manifest, pack, local, staging)?;
    }
    Ok(missing.into_iter().collect())
}

/// Whether `manifest` takes files from packs other than its own (`<id>`,
/// `<id>-2`, ...): a large save's snapshot built on an earlier one, or a
/// version that carries another platform's files. Those packs may be gone
/// by the time it is uploaded, and [`verify`] needs the save files to put
/// them back.
pub fn borrows(manifest: &Manifest) -> bool {
    manifest.packs().iter().any(|p| !is_own(p, &manifest.id))
}

/// The ids of the packs in one folder of the game.
fn present(store: &dyn Store, folder: &str) -> Result<BTreeSet<String>> {
    let listing = store.list(&v2::packs_dir(folder))?;
    Ok(listing.iter().filter(|e| !e.is_dir).filter_map(|e| e.name.strip_suffix(".zip")).map(str::to_string).collect())
}

// --------------------------------------------------------------------- read

/// A pack a snapshot needs is not in the cloud. The snapshot is incomplete
/// until the device that wrote it puts the pack back ([`verify`]); until
/// then the local saves are left as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPack {
    pub pack: String,
}

impl std::fmt::Display for MissingPack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = crate::tr!("这个版本不完整：云端缺少内容包 {}", "This version is incomplete: the content pack {} is missing from the cloud", self.pack);
        write!(f, "{text}")
    }
}
impl std::error::Error for MissingPack {}

/// The pack that is missing, when that is why reading failed.
pub fn missing_pack(e: &anyhow::Error) -> Option<&MissingPack> {
    e.chain().find_map(|c| c.downcast_ref::<MissingPack>())
}

/// Fetch files of a snapshot into `dest` (`<dest>/<canonical path>`): all of
/// them, or the paths in `only`.
///
/// `folders` are the game's folders, searched in order. `line_dir` is the
/// folder the manifest is in ([`super::line::LineView::place`]), which is
/// where a snapshot of the 0.6 layout keeps its zip. One download per pack
/// that holds a wanted file, no listing.
///
/// Every file is checked against the size and hash in the manifest. The
/// packs are fetched and checked in a folder of their own below `staging`,
/// and only when everything is right are the files moved into `dest`: after
/// an error (a [`MissingPack`], a damaged pack, no connection) `dest` is as
/// it was. All that can still fail once the moving has begun is this
/// device's own disk. Files already in `dest` under the same path are
/// replaced, others are left alone.
pub fn read(store: &dyn Store, folders: &[String], line_dir: &str, manifest: &Manifest, only: Option<&[String]>, dest: &Path, staging: &Path) -> Result<()> {
    let lacking = manifest.unsupported();
    if !lacking.is_empty() {
        bail!(crate::tr!("这个版本要用更新的 Ludonimbus 才能读取（{}）", "This version needs a newer Ludonimbus to be read ({})", lacking.join(", ")));
    }
    let by_path: BTreeMap<&str, &FileRef> = manifest.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let wanted: Vec<&FileRef> = match only {
        None => by_path.values().copied().collect(),
        Some(paths) => {
            let paths: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
            let mut wanted = Vec::with_capacity(paths.len());
            for p in paths {
                let f = by_path.get(p).ok_or_else(|| anyhow!(crate::tr!("这个版本里没有文件 {p}", "This version has no file {p}")))?;
                wanted.push(*f);
            }
            wanted
        }
    };
    let mut by_pack: BTreeMap<&str, Vec<&FileRef>> = BTreeMap::new();
    for f in &wanted {
        // The manifest comes from the cloud: nothing in it may lead outside
        // the destination.
        check_path(&f.path)?;
        by_pack.entry(pack_of(manifest, f)).or_default().push(f);
    }

    let scratch = Scratch::new(staging)?;
    let checked = scratch.0.join("files");
    for (n, (pack, files)) in by_pack.iter().enumerate() {
        // Named by position: a pack id from the cloud isn't trusted as a
        // local file name.
        let zip = scratch.0.join(format!("pack-{n}.zip"));
        fetch(store, &places(folders, line_dir, manifest, pack)?, pack, &zip)?;
        extract(&zip, pack, files, &checked)?;
        // One pack on disk at a time.
        let _ = std::fs::remove_file(&zip);
    }

    for f in &wanted {
        let to = local_path(dest, &f.path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", parent.display()))?;
        }
        place(&local_path(&checked, &f.path), &to)?;
        if f.mtime > 0 {
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&to) {
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(f.mtime as u64);
                let _ = file.set_times(std::fs::FileTimes::new().set_modified(t));
            }
        }
    }
    Ok(())
}

/// Where a pack of `manifest` may be, the likeliest place first.
fn places(folders: &[String], line_dir: &str, manifest: &Manifest, pack: &str) -> Result<Vec<String>> {
    // `join` would take a `/` in the name as a folder.
    if !is_file_name(pack) {
        bail!(crate::tr!("内容包的名字无效：{pack}", "Invalid content pack name: {pack}"));
    }
    if manifest.v < 2 {
        return Ok(vec![join(&[line_dir, &format!("{pack}.zip")])]);
    }
    // Nowhere to look is a mistake of the caller, not a pack that is gone.
    if folders.is_empty() {
        bail!(crate::tr!("这个游戏在云端没有文件夹", "This game has no folder in the cloud"));
    }
    Ok(folders.iter().map(|folder| v2::pack_path(folder, pack)).collect())
}

/// Download a pack from the first of `places` that has it.
fn fetch(store: &dyn Store, places: &[String], pack: &str, dest: &Path) -> Result<()> {
    let mut failed = Vec::new();
    for path in places {
        match store.download(path, dest) {
            Ok(()) => return Ok(()),
            // No connection says nothing about the pack: the caller tries
            // again later.
            Err(e) if super::is_unavailable(&e) => return Err(e),
            Err(e) => failed.push((path, e)),
        }
    }
    // A store reports a file that isn't there like any other failure. Only
    // now, when no place gave the pack, is a listing spent on telling "gone"
    // from "there, but the download failed".
    for (path, e) in failed {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        if store.list(dir)?.iter().any(|x| !x.is_dir && x.name == name) {
            return Err(e.context(crate::tr!("下载内容包 {pack} 失败", "Couldn't download the content pack {pack}")));
        }
    }
    Err(MissingPack { pack: pack.to_string() }.into())
}

/// Unpack `files` from a downloaded pack into `into`, checking each against
/// the manifest.
fn extract(zip: &Path, pack: &str, files: &[&FileRef], into: &Path) -> Result<()> {
    let damaged = || crate::tr!("内容包 {pack} 已损坏", "The content pack {pack} is damaged");
    let mut archive = zip::ZipArchive::new(File::open(zip)?).with_context(damaged)?;
    for f in files {
        let mut entry = archive
            .by_name(&format!("files/{}", f.path))
            .map_err(|_| anyhow!(crate::tr!("内容包 {pack} 里缺少文件 {}", "The content pack {pack} is missing the file {}", f.path)))?;
        let path = local_path(into, &f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", path.display()))?;
        // One byte past the size in the manifest is enough to see that an
        // entry is too long, without unpacking whatever it claims to hold.
        let (size, hash) = copy_hashed(&mut entry, &mut out, f.size.saturating_add(1)).with_context(damaged)?;
        out.flush()?;
        if size != f.size || hash != f.hash {
            bail!(crate::tr!(
                "内容包 {pack} 里的文件 {} 和这个版本记录的不一致",
                "The file {} in the content pack {pack} isn't what this version recorded",
                f.path
            ));
        }
    }
    Ok(())
}

/// Move a checked file to its place. A rename when both are on one drive;
/// otherwise a copy beside the target first, so the target is never half a
/// file.
fn place(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let tmp = to.with_file_name(format!("{}.lntmp", to.file_name().unwrap_or_default().to_string_lossy()));
    let moved = std::fs::copy(from, &tmp).and_then(|_| std::fs::rename(&tmp, to));
    if moved.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    moved.with_context(|| crate::tr!("无法写入 {}", "Can't write {}", to.display()))
}

/// A folder of its own below `staging`, removed again whatever happens.
struct Scratch(PathBuf);

impl Scratch {
    fn new(staging: &Path) -> Result<Scratch> {
        let dir = staging.join(format!("read-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", dir.display()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ----------------------------------------------------------------- clean up

/// The packs to delete: those in `listing` (the listing of one `packs/`
/// folder) that are not in `used` and were made more than `grace` before
/// `now`.
///
/// `used` must hold the packs of every manifest that still exists in any
/// save line of the game, the ones in the trash included
/// ([`Manifest::packs`], or `packs` of an index entry), read after the
/// manifests' folders were listed again.
///
/// A pack's age comes from the ULID its name starts with, not from the
/// cloud: a file whose name is not `<ULID>.zip` or `<ULID>-<n>.zip` is never
/// named (a kind of file a later version added, an upload's temporary file).
/// Neither is one made less than a day ago, however short `grace` is.
pub fn unused(listing: &[Entry], used: &BTreeSet<String>, now: DateTime<Utc>, grace: chrono::Duration) -> Vec<String> {
    let grace = grace.max(chrono::Duration::hours(FRESH_HOURS));
    let mut out: Vec<String> = listing
        .iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| e.name.strip_suffix(".zip"))
        .filter(|id| !used.contains(*id))
        .filter(|id| pack_time(id).is_some_and(|made| now.signed_duration_since(made) > grace))
        .map(str::to_string)
        .collect();
    out.sort();
    out
}

/// When a pack was made: its id starts with the id of the snapshot it was
/// first written for, a ULID. `None` for any other name.
fn pack_time(id: &str) -> Option<DateTime<Utc>> {
    let (ulid, rest) = (id.get(..26)?, id.get(26..)?);
    if !rest.is_empty() && !rest.strip_prefix('-').is_some_and(is_part_number) {
        return None;
    }
    // The first character of a ULID only goes up to 7; the parser lets more
    // through and drops the bits that don't fit.
    if !matches!(ulid.as_bytes()[0], b'0'..=b'7') {
        return None;
    }
    let ms = ulid::Ulid::from_string(ulid).ok()?.timestamp_ms();
    DateTime::from_timestamp_millis(i64::try_from(ms).ok()?)
}

/// `2`, `3`, ...: the numbers [`plan`] gives the packs after the first.
fn is_part_number(n: &str) -> bool {
    !n.is_empty() && n != "1" && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `pack` is one of the packs the snapshot `id` brought itself.
fn is_own(pack: &str, id: &str) -> bool {
    pack == id || pack.strip_prefix(id).and_then(|rest| rest.strip_prefix('-')).is_some_and(is_part_number)
}

// ------------------------------------------------------------------ helpers

/// Copy at most `limit` bytes, returning how many there were and their
/// blake3 hash (as 0.6 hashes save files, snapshot.rs).
fn copy_hashed(from: &mut dyn Read, to: &mut dyn Write, limit: u64) -> std::io::Result<(u64, String)> {
    let mut from = from.take(limit);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        total += n as u64;
    }
    Ok((total, hasher.finalize().to_hex().to_string()))
}

/// A canonical path becomes a zip entry and a path below a local folder: it
/// must stay inside both (the rules of 0.6, snapshot.rs).
fn check_path(p: &str) -> Result<()> {
    let ok = !p.is_empty() && !p.contains('\\') && !p.contains(':') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    if !ok {
        bail!(crate::tr!("快照中包含不安全的路径：{p}", "The snapshot contains an unsafe path: {p}"));
    }
    Ok(())
}

fn is_file_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':'])
}

fn local_path(base: &Path, canonical: &str) -> PathBuf {
    canonical.split('/').fold(base.to_path_buf(), |p, c| p.join(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;
    const POLICY: Policy = Policy { full_below: 4 * KIB, split_above: 8 * KIB };

    /// A snapshot id made `hours_ago`; `n` tells ids of the same hour apart.
    fn id(hours_ago: i64, n: u128) -> String {
        let made = Utc::now() - chrono::Duration::hours(hours_ago);
        ulid::Ulid::from_parts(made.timestamp_millis() as u64, n).to_string()
    }

    fn file(path: &str, size: u64, hash: &str) -> LocalFile {
        LocalFile { path: path.into(), size, hash: hash.into(), mtime: 1_780_000_000, source: PathBuf::from("unused") }
    }

    fn manifest(id: &str, files: Vec<FileRef>) -> Manifest {
        Manifest { v: 2, id: id.into(), game: "g".into(), line: "main".into(), files, ..Default::default() }
    }

    /// `(path, in)` of every file of a plan.
    fn refs(plan: &Plan) -> Vec<(&str, Option<&str>)> {
        plan.files.iter().map(|f| (f.path.as_str(), f.pack.as_deref())).collect()
    }

    #[test]
    fn default_policy_is_16_and_64_mib() {
        assert_eq!(Policy::default(), Policy { full_below: 16 * 1024 * 1024, split_above: 64 * 1024 * 1024 });
    }

    #[test]
    fn a_small_save_is_one_complete_pack_whatever_the_base() {
        let local = [file("saves/b", KIB, "hb"), file("saves/a", 3 * KIB, "ha")];
        let first = plan("S1", &local, None, &POLICY);
        assert_eq!(first.packs, vec![NewPack { id: "S1".into(), files: vec!["saves/a".into(), "saves/b".into()], size: 4 * KIB }]);
        assert_eq!(refs(&first), [("saves/a", None), ("saves/b", None)]);
        assert_eq!(first.files[0].size, 3 * KIB);
        assert_eq!(first.files[0].hash, "ha");
        assert_eq!(first.files[0].mtime, 1_780_000_000);
        assert_eq!(first.upload_size(), 4 * KIB);

        // Nothing changed, exactly at the limit: still everything, so the
        // newest zip in the cloud is the whole save.
        let base = manifest("S1", first.files.clone());
        let second = plan("S2", &local, Some(&base), &POLICY);
        assert_eq!(second.packs.len(), 1);
        assert_eq!(second.packs[0].id, "S2");
        assert_eq!(refs(&second), [("saves/a", None), ("saves/b", None)]);

        // No saves at all: nothing to pack.
        assert_eq!(plan("S3", &[], Some(&base), &POLICY), Plan::default());
    }

    #[test]
    fn a_large_save_references_what_the_base_already_has() {
        // The base has its own pack and a file it took from an older one.
        let base = manifest(
            "S2",
            vec![
                FileRef { path: "saves/a".into(), size: 2 * KIB, hash: "ha".into(), ..Default::default() },
                FileRef { path: "saves/b".into(), size: 2 * KIB, hash: "hb".into(), pack: Some("S1".into()), ..Default::default() },
                FileRef { path: "saves/c".into(), size: 2 * KIB, hash: "hc".into(), pack: Some("S1-2".into()), ..Default::default() },
                FileRef { path: "saves/d".into(), size: KIB, hash: "hd".into(), ..Default::default() },
                FileRef { path: "saves/gone".into(), size: KIB, hash: "hg".into(), ..Default::default() },
            ],
        );
        let local = [
            file("saves/a", 2 * KIB, "ha"),
            file("saves/b", 2 * KIB, "hb"),
            file("saves/c", 2 * KIB, "changed"),
            // The same content under another name is new content.
            file("saves/d2", KIB, "hd"),
            file("saves/new", KIB, "hn"),
        ];
        let p = plan("S3", &local, Some(&base), &POLICY);
        assert_eq!(refs(&p), [("saves/a", Some("S2")), ("saves/b", Some("S1")), ("saves/c", None), ("saves/d2", None), ("saves/new", None)]);
        assert_eq!(p.packs, vec![NewPack { id: "S3".into(), files: vec!["saves/c".into(), "saves/d2".into(), "saves/new".into()], size: 4 * KIB }]);
        // Only packs the base uses are referenced.
        let used: BTreeSet<String> = p.files.iter().filter_map(|f| f.pack.clone()).collect();
        assert!(used.is_subset(&base.packs()));

        // Nothing changed (also what restoring or copying a large save comes
        // to): a manifest and no pack.
        let same = [file("saves/a", 2 * KIB, "ha"), file("saves/b", 2 * KIB, "hb"), file("saves/c", 2 * KIB, "hc")];
        let p = plan("S4", &same, Some(&base), &POLICY);
        assert!(p.packs.is_empty());
        assert_eq!(refs(&p), [("saves/a", Some("S2")), ("saves/b", Some("S1")), ("saves/c", Some("S1-2"))]);

        // The same hash with another size is not the same file.
        let odd = [file("saves/a", 5 * KIB, "ha")];
        assert_eq!(refs(&plan("S5", &odd, Some(&base), &POLICY)), [("saves/a", None)]);
    }

    /// A base that takes 1 KiB from the pack `S1` (which holds `s1` bytes)
    /// and 3 + 1 KiB from its own, and says what its packs hold.
    fn wasteful_base(a: u64, s1: Option<u64>) -> Manifest {
        let mut base = manifest(
            "S2",
            vec![
                FileRef { path: "saves/a".into(), size: a, hash: "ha".into(), pack: Some("S1".into()), ..Default::default() },
                FileRef { path: "saves/b".into(), size: 3 * KIB, hash: "hb".into(), ..Default::default() },
                FileRef { path: "saves/c".into(), size: KIB, hash: "hc".into(), ..Default::default() },
            ],
        );
        let mut sizes = serde_json::json!({ "S2": 4 * KIB });
        if let Some(s1) = s1 {
            sizes["S1"] = s1.into();
        }
        base.extra.insert(PACK_SIZES.into(), sizes);
        base
    }

    #[test]
    fn a_pack_that_is_mostly_dead_weight_is_packed_anew_alongside_an_upload() {
        let base = wasteful_base(KIB, Some(10 * KIB));
        let changed = [file("saves/a", KIB, "ha"), file("saves/b", 3 * KIB, "hb"), file("saves/c", KIB, "new")];
        // 1 KiB new, 10 + 4 KiB of packs for 5 KiB of saves: the file still
        // taken from `S1` goes into the new pack, and `S1` is used no more.
        let p = plan("S3", &changed, Some(&base), &POLICY);
        assert_eq!(refs(&p), [("saves/a", None), ("saves/b", Some("S2")), ("saves/c", None)]);
        assert_eq!(p.packs, vec![NewPack { id: "S3".into(), files: vec!["saves/a".into(), "saves/c".into()], size: 2 * KIB }]);
        assert_eq!(p.sizes, BTreeMap::from([("S2".to_string(), 4 * KIB), ("S3".to_string(), 2 * KIB)]));
        // The same answer in whatever order the files come.
        let mut turned = changed.clone();
        turned.reverse();
        assert_eq!(plan("S3", &turned, Some(&base), &POLICY), p);

        // Within twice the content nothing is packed anew.
        let fine = wasteful_base(KIB, Some(5 * KIB));
        let p = plan("S3", &changed, Some(&fine), &POLICY);
        assert_eq!(refs(&p), [("saves/a", Some("S1")), ("saves/b", Some("S2")), ("saves/c", None)]);
        assert_eq!(p.sizes, BTreeMap::from([("S1".to_string(), 5 * KIB), ("S2".to_string(), 4 * KIB), ("S3".to_string(), KIB)]));

        // A pack nobody said the size of counts as holding what is taken
        // from it: left alone, and not named in the new manifest either.
        let unknown = wasteful_base(KIB, None);
        let p = plan("S3", &changed, Some(&unknown), &POLICY);
        assert_eq!(refs(&p), [("saves/a", Some("S1")), ("saves/b", Some("S2")), ("saves/c", None)]);
        assert_eq!(p.sizes, BTreeMap::from([("S2".to_string(), 4 * KIB), ("S3".to_string(), KIB)]));

        // Nothing new (a restore, a copy): a manifest alone, however much
        // dead weight there is.
        let same = [file("saves/a", KIB, "ha"), file("saves/b", 3 * KIB, "hb"), file("saves/c", KIB, "hc")];
        let p = plan("S3", &same, Some(&base), &POLICY);
        assert!(p.packs.is_empty());
        assert_eq!(refs(&p), [("saves/a", Some("S1")), ("saves/b", Some("S2")), ("saves/c", Some("S2"))]);
        assert_eq!(p.sizes, BTreeMap::from([("S1".to_string(), 10 * KIB), ("S2".to_string(), 4 * KIB)]));

        // Never more on the side than the upload itself, or than a small
        // save is: 6 KiB still taken from the wasteful pack stay there. And
        // the other pack, a quarter of it dead, is not worth sending again.
        let heavy = wasteful_base(6 * KIB, Some(40 * KIB));
        let changed = [file("saves/a", 6 * KIB, "ha"), file("saves/b", 3 * KIB, "hb"), file("saves/c", KIB, "new")];
        let p = plan("S3", &changed, Some(&heavy), &POLICY);
        assert_eq!(refs(&p), [("saves/a", Some("S1")), ("saves/b", Some("S2")), ("saves/c", None)]);
        // With an upload of that size it goes along.
        let changed = [file("saves/a", 6 * KIB, "ha"), file("saves/b", 3 * KIB, "new"), file("saves/c", 3 * KIB, "new")];
        let p = plan("S3", &changed, Some(&heavy), &POLICY);
        assert!(refs(&p).iter().all(|(_, pack)| pack.is_none_or(|p| p.starts_with("S3"))), "{:?}", refs(&p));
        let own: BTreeSet<&str> = p.packs.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(p.sizes.keys().map(String::as_str).collect::<BTreeSet<_>>(), own);

        // A save uploaded whole says nothing about its packs.
        assert!(plan("S1", &[file("saves/a", KIB, "ha")], None, &POLICY).sizes.is_empty());
    }

    #[test]
    fn a_base_that_cannot_be_pointed_into_is_no_base() {
        let files = vec![FileRef { path: "saves/a".into(), size: 6 * KIB, hash: "ha".into(), ..Default::default() }];
        let local = [file("saves/a", 6 * KIB, "ha")];

        // A 0.6 snapshot: its zip is beside its manifest, not in `packs/`.
        let mut old = manifest("S1", files.clone());
        old.v = 1;
        assert_eq!(refs(&plan("S2", &local, Some(&old), &POLICY)), [("saves/a", None)]);

        // Written with a feature this build doesn't know.
        let mut newer = manifest("S1", files.clone());
        newer.requires = vec!["chunks".into()];
        assert_eq!(refs(&plan("S2", &local, Some(&newer), &POLICY)), [("saves/a", None)]);

        assert_eq!(refs(&plan("S2", &local, Some(&manifest("S1", files)), &POLICY)), [("saves/a", Some("S1"))]);
    }

    #[test]
    fn new_content_is_split_with_every_file_whole() {
        let local = [
            file("saves/a", 5 * KIB, "a"),
            file("saves/b", 5 * KIB, "b"),
            file("saves/c", 3 * KIB, "c"),
            // Larger than a pack may be: alone in its pack.
            file("saves/d", 20 * KIB, "d"),
            file("saves/e", KIB, "e"),
            file("saves/f", 0, "f"),
        ];
        let p = plan("S1", &local, None, &POLICY);
        let packs: Vec<(&str, Vec<&str>, u64)> = p.packs.iter().map(|k| (k.id.as_str(), k.files.iter().map(String::as_str).collect(), k.size)).collect();
        assert_eq!(
            packs,
            [
                ("S1", vec!["saves/a"], 5 * KIB),
                ("S1-2", vec!["saves/b", "saves/c"], 8 * KIB),
                ("S1-3", vec!["saves/d"], 20 * KIB),
                ("S1-4", vec!["saves/e", "saves/f"], KIB),
            ]
        );
        assert_eq!(
            refs(&p),
            [
                ("saves/a", None),
                ("saves/b", Some("S1-2")),
                ("saves/c", Some("S1-2")),
                ("saves/d", Some("S1-3")),
                ("saves/e", Some("S1-4")),
                ("saves/f", Some("S1-4")),
            ]
        );
        // Every file is in exactly one pack, and the manifest agrees.
        for f in &p.files {
            let holders: Vec<&NewPack> = p.packs.iter().filter(|k| k.files.contains(&f.path)).collect();
            assert_eq!(holders.len(), 1);
            assert_eq!(holders[0].id, f.pack.clone().unwrap_or("S1".into()));
        }

        // Only what is new counts towards the limit.
        let base = manifest("S1", p.files.clone());
        let mut next = local.to_vec();
        next[1] = file("saves/b", 5 * KIB, "b2");
        next[4] = file("saves/e", 3 * KIB, "e2");
        let p = plan("S2", &next, Some(&base), &POLICY);
        assert_eq!(p.packs, vec![NewPack { id: "S2".into(), files: vec!["saves/b".into(), "saves/e".into()], size: 8 * KIB }]);
        assert_eq!(
            refs(&p),
            [("saves/a", Some("S1")), ("saves/b", None), ("saves/c", Some("S1-2")), ("saves/d", Some("S1-3")), ("saves/e", None), ("saves/f", Some("S1-4")),]
        );
    }

    #[test]
    fn the_plan_does_not_depend_on_the_order_of_the_files() {
        let mut local: Vec<LocalFile> = (0..12).map(|n| file(&format!("saves/slot{n:02}"), 3 * KIB, &format!("h{n}"))).collect();
        let sorted = plan("S1", &local, None, &POLICY);
        local.reverse();
        local.swap(2, 7);
        assert_eq!(plan("S1", &local, None, &POLICY), sorted);
        assert_eq!(sorted.packs.len(), 6);
        assert_eq!(sorted.packs[5].id, "S1-6");
    }

    #[test]
    fn only_the_names_plan_gives_are_a_snapshots_own_packs() {
        let s = "01JF0000000000000000000002";
        for own in [s.to_string(), format!("{s}-2"), format!("{s}-10"), format!("{s}-345")] {
            assert!(is_own(&own, s), "{own}");
            assert!(pack_time(&own).is_some(), "{own}");
        }
        let others = ["01JE0000000000000000000001".to_string(), format!("{s}2"), format!("{s}-")];
        let odd_numbers = ["1", "02", "2a", "+2"].map(|n| format!("{s}-{n}"));
        for other in others.iter().chain(&odd_numbers) {
            assert!(!is_own(other, s), "{other}");
        }
        // The time is the one in the ULID.
        let made = Utc::now() - chrono::Duration::hours(30);
        let old = ulid::Ulid::from_parts(made.timestamp_millis() as u64, 7).to_string();
        assert_eq!(pack_time(&old).unwrap().timestamp_millis(), made.timestamp_millis());
        assert_eq!(pack_time(&format!("{old}-3")), pack_time(&old));
        // Names that only look like one.
        let not_packs = [
            "",
            "index",
            // Too short, too long, a letter no ULID has, a time that doesn't fit.
            "01JF000000000000000000000",
            "01JF00000000000000000000022",
            "01JF000000000000000000000L",
            "81JF0000000000000000000002",
            "01JF0000000000000000000002.part",
            "01JF0000000000000000000002-x",
            "01JF0000000000000000000002-1",
            "存档存档存档存档存档存档存档存档存",
        ];
        for name in not_packs {
            assert_eq!(pack_time(name), None, "{name}");
        }
    }

    fn zip_entry(name: &str) -> Entry {
        Entry { name: name.into(), is_dir: false, size: 10, tag: "1".into(), modified: None }
    }

    #[test]
    fn clean_up_names_only_old_packs_nothing_uses() {
        let now = Utc::now();
        let grace = chrono::Duration::days(GRACE_DAYS);
        let (used_old, used_part, orphan_old, orphan_part, orphan_young, orphan_fresh) =
            (id(24 * 30, 1), format!("{}-2", id(24 * 30, 2)), id(24 * 30, 3), format!("{}-2", id(24 * 9, 4)), id(24 * 3, 5), id(1, 6));
        let mut listing: Vec<Entry> =
            [&used_old, &used_part, &orphan_old, &orphan_part, &orphan_young, &orphan_fresh].iter().map(|id| zip_entry(&format!("{id}.zip"))).collect();
        // Things that are not packs of the kind this version writes.
        listing.push(zip_entry("notes.txt"));
        listing.push(zip_entry(&format!("{}.zip.lntmp", id(24 * 30, 7))));
        listing.push(zip_entry(&format!("{}-merged.zip", id(24 * 30, 8))));
        listing.push(zip_entry("compacted.zip"));
        listing.push(Entry { name: format!("{}.zip", id(24 * 30, 9)), is_dir: true, size: 0, tag: String::new(), modified: None });

        let used: BTreeSet<String> = [used_old.clone(), used_part.clone()].into();
        let mut expect = vec![orphan_old.clone(), orphan_part.clone()];
        expect.sort();
        assert_eq!(unused(&listing, &used, now, grace), expect);

        // A shorter grace period reaches younger packs, but never one made
        // within the last day: its manifest may be on its way.
        let mut expect = vec![orphan_old.clone(), orphan_part.clone(), orphan_young.clone()];
        expect.sort();
        assert_eq!(unused(&listing, &used, now, chrono::Duration::zero()), expect);

        // Nothing refers to anything any more: the old ones go, all of them.
        assert_eq!(unused(&listing, &BTreeSet::new(), now, grace).len(), 4);
        // A device whose clock is behind sees packs "from the future": kept.
        assert!(unused(&listing, &BTreeSet::new(), now - chrono::Duration::days(60), grace).is_empty());
        assert!(unused(&[], &used, now, grace).is_empty());
    }
}
