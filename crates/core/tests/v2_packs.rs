//! Content packs of cloud layout 2: what is uploaded, what is read back, and
//! how many requests it takes (docs/plan-v2.md, sections 3.4 and 5.6).

use anyhow::{anyhow, Result};
use chrono::{Duration, TimeZone, Utc};
use ludonimbus_core::cloud::line::{self, LineCache};
use ludonimbus_core::cloud::memory::{Counts, Memory};
use ludonimbus_core::cloud::packs::{self, LocalFile, Plan, Policy};
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::{self, FileRef, Manifest};
use ludonimbus_core::cloud::{self, Unavailable};
use ludonimbus_core::vv::Vv;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const GAME: &str = "stardew-7k3f";
const DIR: &str = "games/stardew-7k3f/saves/main";

/// Kilobytes where the real policy has megabytes: the rules are the same.
const POLICY: Policy = Policy { full_below: 4 * 1024, split_above: 8 * 1024 };

fn folders() -> Vec<String> {
    vec![GAME.to_string()]
}

/// A snapshot id made `ago` before now. Among ids of the same moment a
/// higher `n` sorts later.
fn id_made(ago: Duration, n: u64) -> String {
    let ms = (Utc::now() - ago).timestamp_millis() as u64;
    ulid::Ulid::from_parts(ms, n as u128).to_string()
}

/// Snapshot ids made a few minutes ago, in the order of `n`.
fn id(n: u64) -> String {
    id_made(Duration::minutes(10) - Duration::seconds(n as i64), n)
}

fn bytes(seed: usize, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 17) % 251) as u8).collect()
}

/// Every file below `dir`: `/`-separated relative path -> content.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel: Vec<String> =
                    p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Nothing in it, not even an empty folder (a folder that doesn't exist
/// counts as empty).
fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|d| d.count() == 0).unwrap_or(true)
}

/// One device: a folder with its saves and one for work in progress.
struct Device {
    tmp: TempDir,
}

impl Device {
    fn new() -> Self {
        Device { tmp: tempfile::tempdir().unwrap() }
    }

    /// The saves are below this folder under their canonical paths.
    fn root(&self) -> PathBuf {
        self.tmp.path().join("local")
    }

    fn staging(&self) -> PathBuf {
        self.tmp.path().join("staging")
    }

    /// A folder to read a snapshot into.
    fn dest(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    fn save(&self, path: &str, data: &[u8]) {
        let p = self.root().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn delete(&self, path: &str) {
        std::fs::remove_file(self.root().join(path)).unwrap();
    }

    /// The saves as they are now, hashed.
    fn local(&self) -> Vec<LocalFile> {
        let root = self.root();
        tree(&root).keys().map(|p| LocalFile::from_disk(p.clone(), root.join(p)).unwrap()).collect()
    }
}

fn manifest(id: &str, files: Vec<FileRef>) -> Manifest {
    Manifest {
        v: 2,
        id: id.into(),
        game: GAME.into(),
        line: "main".into(),
        device_id: "d-a".into(),
        device_name: "Desktop".into(),
        platform: "steam".into(),
        created_at: Utc.timestamp_opt(1_780_000_000, 0).unwrap(),
        trigger: "auto-exit".into(),
        content_hash: "c".into(),
        total_size: files.iter().map(|f| f.size).sum(),
        files,
        app_version: "1.0.0".into(),
        ..Default::default()
    }
}

/// What a device does to upload its saves: plan, upload the packs, write the
/// manifest (that is the commit), check the packs the manifest refers to.
fn push(store: &Memory, dev: &Device, id: &str, base: Option<&Manifest>) -> (Manifest, Plan) {
    let local = dev.local();
    let plan = packs::plan(id, &local, base, &POLICY);
    let m = manifest(id, plan.files.clone());
    packs::upload(store, GAME, &m, &local, &dev.staging()).unwrap();
    line::write_manifest(store, DIR, &m).unwrap();
    packs::verify(store, &folders(), &m, &local, &dev.staging()).unwrap();
    assert!(is_empty(&dev.staging()), "nothing is left in the staging folder");
    (m, plan)
}

/// Read a whole snapshot on a device that has never seen it. Returns what
/// arrived and the requests it took.
fn pull(store: &Memory, m: &Manifest) -> (BTreeMap<String, Vec<u8>>, Counts) {
    let dev = Device::new();
    let dest = dev.dest("restored");
    store.take();
    packs::read(store, &folders(), DIR, m, None, &dest, &dev.staging()).unwrap();
    let c = store.take();
    assert!(is_empty(&dev.staging()), "nothing is left in the staging folder");
    (tree(&dest), c)
}

/// `(path, in)` of every file of a manifest.
fn refs(m: &Manifest) -> Vec<(&str, Option<&str>)> {
    m.files.iter().map(|f| (f.path.as_str(), f.pack.as_deref())).collect()
}

/// The entries of a zip in the cloud: name -> content.
fn zip_entries(store: &Memory, path: &str) -> BTreeMap<String, Vec<u8>> {
    let bytes = store.peek(path).unwrap_or_else(|| panic!("{path} is not in the cloud"));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.insert(entry.name().to_string(), data);
    }
    out
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// A save too large to be uploaded whole every time: six files of 1 KiB.
fn large_save(dev: &Device) {
    for n in 0..6 {
        dev.save(&format!("saves/world/region{n}"), &bytes(n, 1024));
    }
}

/// A save whose first upload is split: `a` | `b`, `c` | `d` | `e`.
fn split_save(store: &Memory, dev: &Device) -> (Manifest, Plan) {
    dev.save("saves/a", &bytes(1, 5 * 1024));
    dev.save("saves/b", &bytes(2, 5 * 1024));
    dev.save("saves/c", &bytes(3, 3 * 1024));
    dev.save("saves/d", &bytes(4, 20 * 1024));
    dev.save("saves/e", &bytes(5, 1024));
    push(store, dev, &id(1), None)
}

#[test]
fn a_round_trip_restores_identical_files() {
    let store = Memory::new();
    let a = Device::new();
    a.save("saves/Farm_1/Farm_1", &bytes(1, 1500));
    a.save("saves/Farm_1/SaveGameInfo", &bytes(2, 300));
    a.save("saves/startup_preferences", b"");
    let played = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let farm = std::fs::OpenOptions::new().write(true).open(a.root().join("saves/Farm_1/Farm_1")).unwrap();
    farm.set_times(std::fs::FileTimes::new().set_modified(played)).unwrap();
    drop(farm);

    let s1 = id(1);
    let local = a.local();
    let plan = packs::plan(&s1, &local, None, &POLICY);
    assert_eq!(plan.packs.len(), 1);
    assert_eq!(plan.packs[0].id, s1);
    assert_eq!(plan.upload_size(), 1800);
    assert!(plan.files.iter().all(|f| f.pack.is_none()));
    let m = manifest(&s1, plan.files.clone());

    store.take();
    packs::upload(&store, GAME, &m, &local, &a.staging()).unwrap();
    assert_eq!(store.take(), Counts { upload: 1, ..Default::default() });
    assert_eq!(store.paths(), [format!("games/{GAME}/packs/{s1}.zip")]);
    assert!(is_empty(&a.staging()));

    // The pack is an ordinary zip that says what it is: usable by hand.
    let entries = zip_entries(&store, &v2::pack_path(GAME, &s1));
    let names: Vec<&str> = entries.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["files/saves/Farm_1/Farm_1", "files/saves/Farm_1/SaveGameInfo", "files/saves/startup_preferences", "snapshot.json"]
    );
    assert_eq!(entries["files/saves/Farm_1/Farm_1"], bytes(1, 1500));
    assert_eq!(serde_json::from_slice::<Manifest>(&entries["snapshot.json"]).unwrap(), m);

    // Its own pack, uploaded a moment ago: nothing to look for afterwards.
    line::write_manifest(&store, DIR, &m).unwrap();
    store.take();
    assert!(packs::verify(&store, &folders(), &m, &local, &a.staging()).unwrap().is_empty());
    assert_eq!(store.take().total(), 0);

    // Another device.
    let b = Device::new();
    let dest = b.dest("restored");
    packs::read(&store, &folders(), DIR, &m, None, &dest, &b.staging()).unwrap();
    assert_eq!(store.take(), Counts { download: 1, ..Default::default() }, "one download, no listing");
    assert_eq!(tree(&dest), tree(&a.root()));
    assert_eq!(std::fs::metadata(dest.join("saves/Farm_1/Farm_1")).unwrap().modified().unwrap(), played);
    assert!(is_empty(&b.staging()));
    // What the reader hashes is what the writer hashed.
    let again = LocalFile::from_disk("saves/Farm_1/Farm_1", dest.join("saves/Farm_1/Farm_1")).unwrap();
    assert_eq!((again.size, &again.hash, again.mtime), (m.files[0].size, &m.files[0].hash, 1_700_000_000));
}

#[test]
fn a_small_save_always_gets_one_complete_pack() {
    let store = Memory::new();
    let a = Device::new();
    a.save("saves/slot1", &bytes(1, 1000));
    a.save("saves/slot2", &bytes(2, 1000));
    let (m1, _) = push(&store, &a, &id(1), None);

    // One of two files changed, and there is a base: still everything.
    a.save("saves/slot2", &bytes(3, 1200));
    store.take();
    let (m2, plan) = push(&store, &a, &id(2), Some(&m1));
    assert_eq!(store.take(), Counts { upload: 1, write: 1, ..Default::default() }, "the pack and the manifest");
    assert_eq!(plan.packs.len(), 1);
    assert_eq!(refs(&m2), [("saves/slot1", None), ("saves/slot2", None)]);
    assert_eq!(m2.packs().into_iter().collect::<Vec<_>>(), [m2.id.as_str()]);

    // The newest zip in the cloud is the newest progress, all of it.
    let entries = zip_entries(&store, &v2::pack_path(GAME, &m2.id));
    assert_eq!(entries["files/saves/slot1"], bytes(1, 1000));
    assert_eq!(entries["files/saves/slot2"], bytes(3, 1200));

    // It needs nothing of the version before it.
    store.remove(&v2::manifest_path(DIR, &m1.id)).unwrap();
    store.remove(&v2::pack_path(GAME, &m1.id)).unwrap();
    let (files, c) = pull(&store, &m2);
    assert_eq!(c, Counts { download: 1, ..Default::default() });
    assert_eq!(files, tree(&a.root()));
}

#[test]
fn a_large_save_uploads_only_what_changed() {
    let store = Memory::new();
    let a = Device::new();
    large_save(&a);
    let (m1, p1) = push(&store, &a, &id(1), None);
    assert_eq!(p1.packs.len(), 1);
    assert_eq!(p1.upload_size(), 6 * 1024);
    let v1 = tree(&a.root());

    // One of six files changes.
    a.save("saves/world/region3", &bytes(33, 1024));
    store.take();
    let (m2, p2) = push(&store, &a, &id(2), Some(&m1));
    assert_eq!(
        store.take(),
        Counts { upload: 1, write: 1, list: 1, ..Default::default() },
        "the changed file's pack, the manifest, one look at packs/"
    );
    assert_eq!(p2.packs.len(), 1);
    assert_eq!(p2.packs[0].files, ["saves/world/region3"]);
    assert_eq!(p2.upload_size(), 1024);
    let entries = zip_entries(&store, &v2::pack_path(GAME, &m2.id));
    assert_eq!(entries.keys().map(String::as_str).collect::<Vec<_>>(), ["files/saves/world/region3", "snapshot.json"]);
    let old = Some(m1.id.as_str());
    assert_eq!(
        refs(&m2),
        [
            ("saves/world/region0", old),
            ("saves/world/region1", old),
            ("saves/world/region2", old),
            ("saves/world/region3", None),
            ("saves/world/region4", old),
            ("saves/world/region5", old),
        ]
    );
    let v2_files = tree(&a.root());
    let (files, c) = pull(&store, &m2);
    assert_eq!(c, Counts { download: 2, ..Default::default() });
    assert_eq!(files, v2_files);

    // A new file, one deleted: every other file stays where the base says
    // it is, which by now is two packs.
    a.save("saves/world/region6", &bytes(6, 1024));
    a.delete("saves/world/region0");
    store.take();
    let (m3, p3) = push(&store, &a, &id(3), Some(&m2));
    assert_eq!(store.take(), Counts { upload: 1, write: 1, list: 1, ..Default::default() });
    assert_eq!(p3.packs[0].files, ["saves/world/region6"]);
    assert_eq!(
        refs(&m3),
        [
            ("saves/world/region1", old),
            ("saves/world/region2", old),
            ("saves/world/region3", Some(m2.id.as_str())),
            ("saves/world/region4", old),
            ("saves/world/region5", old),
            ("saves/world/region6", None),
        ]
    );
    assert!(m3.packs().iter().all(|p| *p == m3.id || m2.packs().contains(p)), "only packs the base uses");
    let (files, c) = pull(&store, &m3);
    assert_eq!(c, Counts { download: 3, ..Default::default() });
    assert_eq!(files, tree(&a.root()));

    // Nothing changed since the base (what making an old version the newest
    // again comes to): a manifest, no content.
    store.take();
    let (m4, p4) = push(&store, &a, &id(4), Some(&m3));
    assert_eq!(store.take(), Counts { write: 1, list: 1, ..Default::default() });
    assert!(p4.packs.is_empty());
    assert!(m4.files.iter().all(|f| f.pack.is_some()));
    assert_eq!(pull(&store, &m4).0, tree(&a.root()));

    // The earlier versions are what they were.
    assert_eq!(pull(&store, &m1).0, v1);
    assert_eq!(pull(&store, &m2).0, v2_files);
}

#[test]
fn new_content_is_split_into_packs_of_whole_files() {
    let store = Memory::new();
    let a = Device::new();
    store.take();
    let (m, plan) = split_save(&store, &a);
    assert_eq!(store.take(), Counts { upload: 4, write: 1, ..Default::default() }, "one upload per pack, and the manifest");
    let s = &m.id;
    let ids: Vec<&str> = plan.packs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, [s.clone(), format!("{s}-2"), format!("{s}-3"), format!("{s}-4")]);
    let (second, third, fourth) = (format!("{s}-2"), format!("{s}-3"), format!("{s}-4"));
    assert_eq!(
        refs(&m),
        [
            ("saves/a", None),
            ("saves/b", Some(second.as_str())),
            ("saves/c", Some(second.as_str())),
            ("saves/d", Some(third.as_str())),
            ("saves/e", Some(fourth.as_str())),
        ]
    );

    // Each pack holds its files whole, and says which snapshot it is for.
    let holds = |pack: &str| -> Vec<String> { zip_entries(&store, &v2::pack_path(GAME, pack)).into_keys().collect() };
    assert_eq!(holds(s), ["files/saves/a", "snapshot.json"]);
    assert_eq!(holds(&second), ["files/saves/b", "files/saves/c", "snapshot.json"]);
    assert_eq!(holds(&third), ["files/saves/d", "snapshot.json"]);
    assert_eq!(holds(&fourth), ["files/saves/e", "snapshot.json"]);
    assert_eq!(zip_entries(&store, &v2::pack_path(GAME, &third))["files/saves/d"], bytes(4, 20 * 1024));

    let (files, c) = pull(&store, &m);
    assert_eq!(c, Counts { download: 4, ..Default::default() });
    assert_eq!(files, tree(&a.root()));

    // The next version refers into the parts.
    a.save("saves/c", &bytes(30, 3 * 1024));
    let (m2, p2) = push(&store, &a, &id(2), Some(&m));
    assert_eq!(p2.packs.len(), 1);
    assert_eq!(
        refs(&m2),
        [
            ("saves/a", Some(s.as_str())),
            ("saves/b", Some(second.as_str())),
            ("saves/c", None),
            ("saves/d", Some(third.as_str())),
            ("saves/e", Some(fourth.as_str())),
        ]
    );
    assert_eq!(pull(&store, &m2).0, tree(&a.root()));
}

#[test]
fn a_subset_downloads_only_the_packs_it_needs() {
    let store = Memory::new();
    let a = Device::new();
    let (m, _) = split_save(&store, &a);
    let all = tree(&a.root());
    let b = Device::new();
    let read = |only: &[&str], dest: &Path| -> (Result<()>, Counts) {
        let only: Vec<String> = only.iter().map(|p| p.to_string()).collect();
        store.take();
        let r = packs::read(&store, &folders(), DIR, &m, Some(&only), dest, &b.staging());
        (r, store.take())
    };

    let dest = b.dest("one");
    let (r, c) = read(&["saves/c"], &dest);
    r.unwrap();
    assert_eq!(c, Counts { download: 1, ..Default::default() });
    assert_eq!(tree(&dest), BTreeMap::from([("saves/c".to_string(), all["saves/c"].clone())]));

    // Two files of one pack: still one download. Asked for twice: once.
    let dest = b.dest("pair");
    let (r, c) = read(&["saves/c", "saves/b", "saves/c"], &dest);
    r.unwrap();
    assert_eq!(c, Counts { download: 1, ..Default::default() });
    assert_eq!(tree(&dest).keys().map(String::as_str).collect::<Vec<_>>(), ["saves/b", "saves/c"]);

    let dest = b.dest("ends");
    let (r, c) = read(&["saves/e", "saves/a"], &dest);
    r.unwrap();
    assert_eq!(c, Counts { download: 2, ..Default::default() });
    assert_eq!(tree(&dest).keys().map(String::as_str).collect::<Vec<_>>(), ["saves/a", "saves/e"]);

    // A file the snapshot doesn't have: an error before anything is fetched.
    let dest = b.dest("none");
    let (r, c) = read(&["saves/a", "saves/zzz"], &dest);
    assert!(r.is_err());
    assert_eq!(c.total(), 0);
    assert!(is_empty(&dest));

    let (r, c) = read(&[], &dest);
    r.unwrap();
    assert_eq!(c.total(), 0);
    assert!(is_empty(&dest) && is_empty(&b.staging()));
}

/// The cloud the released 0.6.2 wrote (`tests/fixtures/cloud-0.6`), and what
/// 0.6.2 itself listed in it: per game, every snapshot with the text of its
/// files.
fn sample_of_0_6() -> (Memory, serde_json::Value) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cloud-0.6");
    let store = Memory::new();
    for (path, data) in tree(&fixture.join("cloud")) {
        store.put(&path, &data);
    }
    let expected = serde_json::from_slice(&std::fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    (store, expected)
}

#[test]
fn a_snapshot_of_the_old_layout_reads_through_the_same_code() {
    let (store, expected) = sample_of_0_6();
    let b = Device::new();

    // Every version 0.6.2 uploaded comes back as the files it had: the zip
    // is beside its manifest, one download.
    let mut restored = 0;
    for (key, snapshots) in expected.as_object().unwrap() {
        let dir = format!("games/{key}/snapshots");
        let game = vec![key.clone()];
        for was in snapshots.as_array().unwrap() {
            let id = was["id"].as_str().unwrap();
            let m = line::read_manifest(&store, &dir, id).unwrap().unwrap();
            assert_eq!(m.v, 1);
            let dest = b.dest(id);
            store.take();
            packs::read(&store, &game, &dir, &m, None, &dest, &b.staging()).unwrap();
            assert_eq!(store.take(), Counts { download: 1, ..Default::default() }, "{key} {id}");
            let files: BTreeMap<String, Vec<u8>> =
                was["files"].as_object().unwrap().iter().map(|(p, text)| (p.clone(), text.as_str().unwrap().as_bytes().to_vec())).collect();
            assert_eq!(tree(&dest), files, "{key} {id}");
            restored += 1;
        }
    }
    assert_eq!(restored, 16);
    assert!(is_empty(&b.staging()));

    // One of them, with two files.
    let (dir, game) = ("games/abs-game/snapshots", vec!["abs-game".to_string()]);
    let id = expected["abs-game"][1]["id"].as_str().unwrap();
    let m = line::read_manifest(&store, dir, id).unwrap().unwrap();
    let (profile, extra) = ("drive-c/lnfix/abs-saves/profile.dat", "drive-c/lnfix/abs-saves/sub/extra.dat");
    assert_eq!(m.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), [profile, extra]);

    // Part of it.
    let one = b.dest("one");
    packs::read(&store, &game, dir, &m, Some(&[extra.to_string()]), &one, &b.staging()).unwrap();
    assert_eq!(tree(&one), BTreeMap::from([(extra.to_string(), b"x".to_vec())]));

    // The first upload of the new layout doesn't point into the old zip,
    // however large the save is: `in` only reaches into `packs/`.
    let large = Policy { full_below: 0, ..POLICY };
    let local: Vec<LocalFile> = [profile, extra].iter().map(|p| LocalFile::from_disk(*p, b.dest(id).join(p)).unwrap()).collect();
    assert_eq!(local[0].hash, m.files[0].hash, "the same hash in both layouts");
    let plan = packs::plan("S1", &local, Some(&m), &large);
    assert_eq!(plan.packs.len(), 1);
    assert!(plan.files.iter().all(|f| f.pack.is_none()));
    // (The same files in a snapshot of the new layout would be referenced.)
    let new_layout = Manifest { v: 2, ..m.clone() };
    assert!(packs::plan("S1", &local, Some(&new_layout), &large).packs.is_empty());

    // The zip is gone: the same error as for a pack.
    store.remove(&format!("{dir}/{id}.zip")).unwrap();
    let dest = b.dest("gone");
    let err = packs::read(&store, &game, dir, &m, None, &dest, &b.staging()).unwrap_err();
    assert_eq!(packs::missing_pack(&err).unwrap().pack, id);
    assert!(is_empty(&dest));
}

/// A cloud whose downloads fail, in one of the two ways a store fails.
struct Failing<'a> {
    inner: &'a Memory,
    offline: bool,
}

impl Store for Failing<'_> {
    fn kind(&self) -> &'static str {
        "failing"
    }
    fn label(&self) -> String {
        "failing".to_string()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.inner.read(path)
    }
    fn download(&self, path: &str, _dest: &Path) -> Result<()> {
        if self.offline {
            Err(anyhow!(Unavailable("timed out".to_string())))
        } else {
            Err(anyhow!("HTTP 500 for {path}"))
        }
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.inner.remove(path)
    }
}

#[test]
fn a_missing_pack_is_its_own_error_and_leaves_the_destination_alone() {
    let store = Memory::new();
    let a = Device::new();
    large_save(&a);
    let (m1, _) = push(&store, &a, &id(1), None);
    a.save("saves/world/region3", &bytes(33, 1024));
    let (m2, _) = push(&store, &a, &id(2), Some(&m1));

    // A device with an older state of the saves, and a file of its own.
    let b = Device::new();
    let dest = b.dest("saves-here");
    std::fs::create_dir_all(dest.join("saves/world")).unwrap();
    std::fs::write(dest.join("saves/world/region0"), b"what was here before").unwrap();
    std::fs::write(dest.join("notes.txt"), b"mine").unwrap();
    let before = tree(&dest);

    // The second of the two packs is gone: the first one was fetched and
    // found good, and still nothing of it may arrive.
    let newer = v2::pack_path(GAME, &m2.id);
    let kept = store.peek(&newer).unwrap();
    store.remove(&newer).unwrap();
    store.take();
    let err = packs::read(&store, &folders(), DIR, &m2, None, &dest, &b.staging()).unwrap_err();
    assert_eq!(store.take(), Counts { download: 2, list: 1, ..Default::default() }, "the listing only confirms that it is gone");
    assert_eq!(packs::missing_pack(&err), Some(&packs::MissingPack { pack: m2.id.clone() }));
    assert!(!cloud::is_unavailable(&err));
    assert_eq!(tree(&dest), before);
    assert!(is_empty(&b.staging()));
    // Still recognisable after the caller has added to it.
    let wrapped = err.context("pulling the newest version");
    assert_eq!(packs::missing_pack(&wrapped).unwrap().pack, m2.id);

    // The first pack gone: found out at the first download.
    store.put(&newer, &kept);
    let older = v2::pack_path(GAME, &m1.id);
    let kept = store.peek(&older).unwrap();
    store.remove(&older).unwrap();
    store.take();
    let err = packs::read(&store, &folders(), DIR, &m2, None, &dest, &b.staging()).unwrap_err();
    assert_eq!(store.take(), Counts { download: 1, list: 1, ..Default::default() });
    assert_eq!(packs::missing_pack(&err).unwrap().pack, m1.id);
    assert_eq!(tree(&dest), before);
    // What doesn't need that pack still reads.
    let only = vec!["saves/world/region3".to_string()];
    packs::read(&store, &folders(), DIR, &m2, Some(&only), &b.dest("part"), &b.staging()).unwrap();
    store.put(&older, &kept);

    // No connection is not a missing pack: nothing says the version is
    // incomplete.
    let offline = Failing { inner: &store, offline: true };
    store.take();
    let err = packs::read(&offline, &folders(), DIR, &m2, None, &dest, &b.staging()).unwrap_err();
    assert!(packs::missing_pack(&err).is_none());
    assert!(cloud::is_unavailable(&err));
    assert_eq!(store.take().total(), 0, "no listing when the cloud can't be reached");
    assert_eq!(tree(&dest), before);

    // Neither is a download that fails while the pack is there.
    let broken = Failing { inner: &store, offline: false };
    let err = packs::read(&broken, &folders(), DIR, &m2, None, &dest, &b.staging()).unwrap_err();
    assert!(packs::missing_pack(&err).is_none());
    assert!(!cloud::is_unavailable(&err));
    assert_eq!(store.take(), Counts { list: 1, ..Default::default() });
    assert_eq!(tree(&dest), before);
    assert!(is_empty(&b.staging()));

    // With everything back in place the files arrive, replacing the old
    // state and leaving alone what the snapshot doesn't have.
    packs::read(&store, &folders(), DIR, &m2, None, &dest, &b.staging()).unwrap();
    let mut expect = tree(&a.root());
    expect.insert("notes.txt".to_string(), b"mine".to_vec());
    assert_eq!(tree(&dest), expect);
}

#[test]
fn a_file_that_is_not_what_the_manifest_says_is_an_error() {
    let store = Memory::new();
    let a = Device::new();
    a.save("saves/slot1", &bytes(1, 1000));
    a.save("saves/slot2", &bytes(2, 1000));
    let (m, _) = push(&store, &a, &id(1), None);
    let path = v2::pack_path(GAME, &m.id);
    let good = store.peek(&path).unwrap();

    let b = Device::new();
    let dest = b.dest("saves-here");
    std::fs::create_dir_all(dest.join("saves")).unwrap();
    std::fs::write(dest.join("saves/slot1"), b"what was here before").unwrap();
    let before = tree(&dest);
    let (slot1, slot2) = (bytes(1, 1000), bytes(2, 1000));
    let damaged: Vec<(&str, Vec<u8>)> = vec![
        // The right size, other content. The first file is good and must
        // not arrive either.
        ("other content", zip_of(&[("files/saves/slot1", &slot1), ("files/saves/slot2", &bytes(99, 1000))])),
        ("longer", zip_of(&[("files/saves/slot1", &slot1), ("files/saves/slot2", &bytes(2, 5000))])),
        ("shorter", zip_of(&[("files/saves/slot1", &slot1), ("files/saves/slot2", &slot2[..999])])),
        ("a file missing", zip_of(&[("files/saves/slot1", &slot1)])),
        ("not a zip", b"<html>Sign in to continue</html>".to_vec()),
        ("cut off", good[..good.len() / 2].to_vec()),
    ];
    for (what, bytes) in &damaged {
        store.put(&path, bytes);
        let err = packs::read(&store, &folders(), DIR, &m, None, &dest, &b.staging()).unwrap_err();
        assert!(packs::missing_pack(&err).is_none(), "{what}: the pack is there");
        assert_eq!(tree(&dest), before, "{what}");
        assert!(is_empty(&b.staging()), "{what}");
    }

    store.put(&path, &good);
    packs::read(&store, &folders(), DIR, &m, None, &dest, &b.staging()).unwrap();
    assert_eq!(tree(&dest), tree(&a.root()));
}

#[test]
fn a_changed_save_file_never_gets_into_a_pack() {
    let store = Memory::new();
    let a = Device::new();
    a.save("saves/slot1", &bytes(1, 1000));
    a.save("saves/slot2", &bytes(2, 1000));
    let s1 = id(1);
    let local = a.local();
    let m = manifest(&s1, packs::plan(&s1, &local, None, &POLICY).files);

    // The game writes again between the scan and the upload: the manifest
    // would promise content the pack doesn't hold.
    a.save("saves/slot2", &bytes(3, 1000));
    store.take();
    assert!(packs::upload(&store, GAME, &m, &local, &a.staging()).is_err());
    assert_eq!(store.take().total(), 0);
    assert!(store.paths().is_empty());
    assert!(is_empty(&a.staging()));

    // A path that would leave the zip's `files/` folder is refused as well.
    let mut odd = local.clone();
    odd[0].path = "../outside".into();
    let m = manifest(&s1, packs::plan(&s1, &odd, None, &POLICY).files);
    assert!(packs::upload(&store, GAME, &m, &odd, &a.staging()).is_err());
    assert!(store.paths().is_empty());
}

#[test]
fn a_pack_cleaned_up_in_between_is_put_back() {
    let store = Memory::new();
    let a = Device::new();
    large_save(&a);
    let (m1, _) = push(&store, &a, &id(1), None);

    a.save("saves/world/region3", &bytes(33, 1024));
    let s2 = id(2);
    let local = a.local();
    let plan = packs::plan(&s2, &local, Some(&m1), &POLICY);
    let m2 = manifest(&s2, plan.files.clone());
    packs::upload(&store, GAME, &m2, &local, &a.staging()).unwrap();
    // Meanwhile another device removes the first version and, nothing else
    // referring to it yet, its pack.
    store.remove(&v2::manifest_path(DIR, &m1.id)).unwrap();
    store.remove(&v2::pack_path(GAME, &m1.id)).unwrap();
    line::write_manifest(&store, DIR, &m2).unwrap();

    // Until the writer has looked, the new version is incomplete.
    let b = Device::new();
    let err = packs::read(&store, &folders(), DIR, &m2, None, &b.dest("early"), &b.staging()).unwrap_err();
    assert_eq!(packs::missing_pack(&err).unwrap().pack, m1.id);

    store.take();
    let restored = packs::verify(&store, &folders(), &m2, &local, &a.staging()).unwrap();
    assert_eq!(store.take(), Counts { list: 1, upload: 1, ..Default::default() });
    assert_eq!(restored, [m1.id.as_str()]);
    assert!(is_empty(&a.staging()));

    // The pack is back under its own id with what this version takes from
    // it; the manifest was not touched.
    let entries = zip_entries(&store, &v2::pack_path(GAME, &m1.id));
    assert_eq!(
        entries.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "files/saves/world/region0",
            "files/saves/world/region1",
            "files/saves/world/region2",
            "files/saves/world/region4",
            "files/saves/world/region5",
            "snapshot.json",
        ]
    );
    assert_eq!(serde_json::from_slice::<Manifest>(&entries["snapshot.json"]).unwrap(), m2);
    assert_eq!(line::read_manifest(&store, DIR, &m2.id).unwrap().unwrap(), m2);

    let (files, c) = pull(&store, &m2);
    assert_eq!(c, Counts { download: 2, ..Default::default() });
    assert_eq!(files, tree(&a.root()));

    // Looking again finds everything in place.
    assert!(packs::verify(&store, &folders(), &m2, &local, &a.staging()).unwrap().is_empty());
    assert_eq!(store.take(), Counts { list: 1, ..Default::default() });
}

#[test]
fn a_snapshot_that_waited_to_be_uploaded_has_its_own_pack_checked() {
    let store = Memory::new();
    let a = Device::new();
    a.save("saves/slot1", &bytes(1, 1000));
    // Made ten days ago with no connection, uploaded now.
    let s1 = id_made(Duration::days(10), 1);
    let local = a.local();
    let m = manifest(&s1, packs::plan(&s1, &local, None, &POLICY).files);
    packs::upload(&store, GAME, &m, &local, &a.staging()).unwrap();

    // To a clean-up on another device this is an old pack without a
    // manifest, from the moment it appears.
    let listing = store.list(&v2::packs_dir(GAME)).unwrap();
    let doomed = packs::unused(&listing, &BTreeSet::new(), Utc::now(), Duration::days(packs::GRACE_DAYS));
    assert_eq!(doomed, [s1.as_str()]);
    store.remove(&v2::pack_path(GAME, &s1)).unwrap();

    line::write_manifest(&store, DIR, &m).unwrap();
    store.take();
    assert_eq!(packs::verify(&store, &folders(), &m, &local, &a.staging()).unwrap(), [s1.as_str()]);
    assert_eq!(store.take(), Counts { list: 1, upload: 1, ..Default::default() });
    assert_eq!(pull(&store, &m).0, tree(&a.root()));

    // With its manifest written, the pack is in use and stays.
    let listing = store.list(&v2::packs_dir(GAME)).unwrap();
    assert!(packs::unused(&listing, &m.packs(), Utc::now(), Duration::days(packs::GRACE_DAYS)).is_empty());
}

#[test]
fn packs_are_looked_for_in_every_folder_of_the_game() {
    // Two records of the same game were merged: the folder of the one that
    // stays comes first, the other's packs stay where they are.
    let store = Memory::new();
    let a = Device::new();
    large_save(&a);
    let (m1, _) = push(&store, &a, &id(1), None);
    let pack = store.peek(&v2::pack_path(GAME, &m1.id)).unwrap();
    store.remove(&v2::pack_path(GAME, &m1.id)).unwrap();
    store.put(&v2::pack_path("stardew-old", &m1.id), &pack);
    let both = vec![GAME.to_string(), "stardew-old".to_string()];

    let b = Device::new();
    let dest = b.dest("restored");
    store.take();
    packs::read(&store, &both, DIR, &m1, None, &dest, &b.staging()).unwrap();
    assert_eq!(store.take(), Counts { download: 2, ..Default::default() }, "the first folder doesn't have it; still no listing");
    assert_eq!(tree(&dest), tree(&a.root()));

    // A new version refers to the pack over there and finds it there:
    // nothing is uploaded a second time.
    a.save("saves/world/region3", &bytes(33, 1024));
    let s2 = id(2);
    let local = a.local();
    let m2 = manifest(&s2, packs::plan(&s2, &local, Some(&m1), &POLICY).files);
    packs::upload(&store, GAME, &m2, &local, &a.staging()).unwrap();
    line::write_manifest(&store, DIR, &m2).unwrap();
    store.take();
    assert!(packs::verify(&store, &both, &m2, &local, &a.staging()).unwrap().is_empty());
    assert_eq!(store.take(), Counts { list: 2, ..Default::default() });

    // Gone from both: put back into the folder that is written to.
    store.remove(&v2::pack_path("stardew-old", &m1.id)).unwrap();
    assert_eq!(packs::verify(&store, &both, &m2, &local, &a.staging()).unwrap(), [m1.id.as_str()]);
    assert!(store.peek(&v2::pack_path(GAME, &m1.id)).is_some());
    let dest = b.dest("again");
    store.take();
    packs::read(&store, &both, DIR, &m2, None, &dest, &b.staging()).unwrap();
    assert_eq!(store.take(), Counts { download: 2, ..Default::default() });
    assert_eq!(tree(&dest), tree(&a.root()));
}

#[test]
fn clean_up_respects_references_and_the_grace_period() {
    let store = Memory::new();
    let a = Device::new();
    let grace = Duration::days(packs::GRACE_DAYS);
    let used = |manifests: &[&Manifest]| -> BTreeSet<String> { manifests.iter().flat_map(|m| m.packs()).collect() };
    let clean = |used: &BTreeSet<String>| -> Vec<String> {
        // As the design asks of whoever deletes: the manifests are read
        // first, the packs are listed, and only then is anything removed.
        let listing = store.list(&v2::packs_dir(GAME)).unwrap();
        let gone = packs::unused(&listing, used, Utc::now(), grace);
        for pack in &gone {
            store.remove(&v2::pack_path(GAME, pack)).unwrap();
        }
        gone
    };

    // Three versions of a large save over the last weeks. The second and
    // the third change the same file.
    large_save(&a);
    let (m1, _) = push(&store, &a, &id_made(Duration::days(30), 1), None);
    a.save("saves/world/region3", &bytes(33, 1024));
    let (m2, _) = push(&store, &a, &id_made(Duration::days(20), 2), Some(&m1));
    a.save("saves/world/region3", &bytes(34, 1024));
    let (m3, _) = push(&store, &a, &id_made(Duration::days(10), 3), Some(&m2));
    assert_eq!(m3.packs(), BTreeSet::from([m1.id.clone(), m3.id.clone()]));

    // An upload that broke off two weeks ago, one that is on its way right
    // now, and a file of a kind this version doesn't know.
    let abandoned = id_made(Duration::days(14), 4);
    let in_progress = id_made(Duration::minutes(1), 5);
    store.put(&v2::pack_path(GAME, &abandoned), b"half an upload");
    store.put(&v2::pack_path(GAME, &in_progress), b"a pack whose manifest comes next");
    store.put(&format!("{}/README.txt", v2::packs_dir(GAME)), b"hello");

    // Everything has its manifest: only the abandoned upload goes.
    assert_eq!(clean(&used(&[&m1, &m2, &m3])), [abandoned.as_str()]);

    // Retention drops the first version. The newest still takes five files
    // from its pack, so the pack stays.
    store.remove(&v2::manifest_path(DIR, &m1.id)).unwrap();
    assert!(clean(&used(&[&m2, &m3])).is_empty());

    // The user deletes the second version: it is in the trash, can come
    // back, and keeps its pack.
    store.remove(&v2::manifest_path(DIR, &m2.id)).unwrap();
    let trashed = [&m2];
    let remaining = [&m3];
    let with_trash: Vec<&Manifest> = remaining.iter().chain(trashed.iter()).copied().collect();
    assert!(clean(&used(&with_trash)).is_empty());
    assert_eq!(pull(&store, &m2).0.len(), 6, "a version in the trash can still be read");

    // The trash is emptied: now nothing uses that pack.
    assert_eq!(clean(&used(&remaining)), [m2.id.as_str()]);

    let mut left = store.paths();
    left.retain(|p| p.starts_with(&v2::packs_dir(GAME)));
    let mut expect = vec![
        format!("{}/README.txt", v2::packs_dir(GAME)),
        v2::pack_path(GAME, &m1.id),
        v2::pack_path(GAME, &m3.id),
        v2::pack_path(GAME, &in_progress),
    ];
    expect.sort();
    assert_eq!(left, expect);
    assert_eq!(pull(&store, &m3).0, tree(&a.root()));
    assert!(clean(&used(&remaining)).is_empty());
}

/// The numbers of docs/plan-v2.md section 5.6, with everything a sync does
/// around the packs.
#[test]
fn a_sync_costs_what_the_design_says() {
    let store = Memory::new();
    let a = Device::new();
    let mut cache = LineCache::default();
    let mut last = Vv::new();
    // After the game exits: look at the line, upload, update the index.
    let mut sync = |n: u64, base: Option<&Manifest>| -> (Manifest, Counts) {
        store.take();
        line::load(&store, DIR, &mut cache).unwrap();
        let local = a.local();
        let s = id(n);
        let mut m = manifest(&s, packs::plan(&s, &local, base, &POLICY).files);
        m.vv = Vv::next([&last], "d-a", 0);
        packs::upload(&store, GAME, &m, &local, &a.staging()).unwrap();
        line::write_manifest(&store, DIR, &m).unwrap();
        packs::verify(&store, &folders(), &m, &local, &a.staging()).unwrap();
        cache.insert(&m);
        line::write_index(&store, DIR, &cache, "d-a").unwrap();
        last = m.vv.clone();
        (m, store.take())
    };

    // A small save: list, pack, manifest, index.
    a.save("saves/world/region0", &bytes(0, 1024));
    let (m1, c) = sync(1, None);
    assert_eq!(c, Counts { list: 1, upload: 1, write: 2, ..Default::default() });
    assert_eq!(c.total(), 4);
    a.save("saves/world/region0", &bytes(10, 1024));
    let (m2, c) = sync(2, Some(&m1));
    assert_eq!(c.total(), 4);

    // The save grows past the limit: only the new files are uploaded, and
    // packs/ is listed once after the manifest is written.
    for n in 1..6 {
        a.save(&format!("saves/world/region{n}"), &bytes(n, 1024));
    }
    let (m3, c) = sync(3, Some(&m2));
    assert_eq!(c, Counts { list: 2, upload: 1, write: 2, ..Default::default() });
    assert_eq!(m3.files[0].pack.as_deref(), Some(m2.id.as_str()));
    // More new content than fits one pack: one more request per pack.
    a.save("saves/world/region6", &bytes(6, 7 * 1024));
    a.save("saves/world/region7", &bytes(7, 7 * 1024));
    let (m4, c) = sync(4, Some(&m3));
    assert_eq!(c, Counts { list: 2, upload: 2, write: 2, ..Default::default() });

    // Another device before the game starts: the listing and the index say
    // there is a new version; its manifest says which packs to fetch.
    let b = Device::new();
    let mut seen = LineCache::default();
    store.take();
    let view = line::load(&store, DIR, &mut seen).unwrap();
    assert_eq!(store.take(), Counts { list: 1, read: 1, ..Default::default() });
    let head = view.head().unwrap();
    assert_eq!(head.id, m4.id);
    assert_eq!(head.packs.len(), 4);
    let m = line::read_manifest(&store, DIR, &head.id).unwrap().unwrap();
    packs::read(&store, &folders(), DIR, &m, None, &b.dest("restored"), &b.staging()).unwrap();
    assert_eq!(store.take(), Counts { read: 1, download: 4, ..Default::default() }, "one download per pack");
    assert_eq!(tree(&b.dest("restored")), tree(&a.root()));

    // Nothing new: the listing is all it takes, and no pack is touched.
    line::load(&store, DIR, &mut seen).unwrap();
    assert_eq!(store.take(), Counts { list: 1, ..Default::default() });
}
