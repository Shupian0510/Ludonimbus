//! Sharing one save with someone else (docs/plan-v2.md, section 9): the
//! file says nothing of whose it was; taken in, it goes beside the history
//! of the friend's line, kept, and changes nothing until they make it
//! current. One test, because the data directory is chosen through an
//! environment variable.

use anyhow::Result;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::Cloud;
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::{SnapshotSummary, Trigger};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use ludonimbus_core::{share, state};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One person's cloud, in memory.
#[derive(Clone)]
struct Drive {
    inner: Arc<Memory>,
    scope: &'static str,
}

impl Drive {
    fn new(scope: &'static str) -> Drive {
        Drive { inner: Arc::new(Memory::default()), scope }
    }
}

impl Store for Drive {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".to_string()
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
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.inner.download(path, dest)
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
    fn cache_scope(&self) -> Option<String> {
        Some(self.scope.to_string())
    }
}

/// A person's PC with one game.
struct Device {
    home: PathBuf,
    game: PathBuf,
    key: String,
    drive: Drive,
}

impl Device {
    fn new(root: &Path, drive: &Drive, who: &str, key: &str, name: &str) -> Device {
        let dev = Device { home: root.join(format!("home-{who}")), game: root.join(format!("game-{who}")), key: key.to_string(), drive: drive.clone() };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = format!("{who}-pc");
        cfg.cloud = CloudConfig::None;
        cfg.games.push(GameEntry {
            key: key.into(),
            name: name.into(),
            exe: Some(dev.game.join("game.exe")),
            saves: vec!["{install}/saves/**".to_string()],
            auto_sync: true,
            ..Default::default()
        });
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    fn ctx(&self) -> Ctx {
        self.activate();
        let cfg = config::load().unwrap();
        let cloud = Cloud::new(Box::new(self.drive.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    }

    fn entry(&self) -> GameEntry {
        self.activate();
        config::load().unwrap().game(&self.key).unwrap().clone()
    }

    fn write(&self, file: &str, content: &str) {
        let path = self.game.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.game.join(file)).ok()
    }

    fn sync(&self) -> Outcome {
        let ctx = self.ctx();
        sync::sync(&ctx, &self.entry(), Resolution::Auto, Trigger::SyncNow).unwrap()
    }

    fn status(&self) -> Status {
        let ctx = self.ctx();
        sync::check(&ctx, &self.entry()).status
    }

    fn versions(&self) -> Vec<SnapshotSummary> {
        self.ctx().cloud().unwrap().list_snapshots(&self.key).unwrap()
    }

    fn import(&self, file: &Path) -> Result<String> {
        let ctx = self.ctx();
        share::import(&ctx, &self.entry(), file).map(|m| m.id)
    }
}

fn uploaded(outcome: Outcome) -> String {
    match outcome {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

/// Every entry of a zip, by name.
fn entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut e = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        e.read_to_end(&mut data).unwrap();
        out.insert(e.name().to_string(), data);
    }
    out
}

fn write_zip(path: &Path, entries: &BTreeMap<String, Vec<u8>>) {
    let mut zw = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, data) in entries {
        zw.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap();
}

#[test]
fn a_shared_save_goes_beside_the_friends_history() {
    let dir = tempfile::tempdir().unwrap();
    let (anna_cloud, ben_cloud) = (Drive::new("share-anna"), Drive::new("share-ben"));
    let anna = Device::new(dir.path(), &anna_cloud, "anna", "farm-a", "Stardew Valley");
    let ben = Device::new(dir.path(), &ben_cloud, "ben", "farm-b", "Stardew Valley");

    // Each has their own progress in their own cloud.
    anna.write("saves/slot1.sav", "anna's farm, year 3");
    anna.write("saves/meta/info.xml", "<year>3</year>");
    let hers = uploaded(anna.sync());
    ben.write("saves/slot1.sav", "ben's farm, year 1");
    let his = uploaded(ben.sync());

    // Anna shares hers: the files and the game, nothing of her PC.
    let file = dir.path().join(format!("farm.{}", share::EXT));
    let shared = {
        let ctx = anna.ctx();
        share::export(&ctx, &anna.entry(), &hers, &file).unwrap()
    };
    assert_eq!(shared.files.len(), 2);
    assert_eq!(shared.name, "Stardew Valley");
    assert!(!shared.ids.is_empty());
    let inside = entries(&file);
    assert_eq!(inside.len(), 3, "{:?}", inside.keys().collect::<Vec<_>>());
    assert!(inside.keys().all(|n| n == "share.json" || n.starts_with("files/")));
    let said = String::from_utf8_lossy(&inside["share.json"]).to_lowercase();
    let anna_id = {
        anna.activate();
        config::load().unwrap().device.id
    };
    for private in ["anna-pc", anna_id.as_str(), "home-anna", "game-anna", "farm-a"] {
        assert!(!said.contains(&private.to_lowercase()), "the shared save names {private}: {said}");
    }
    assert_eq!(share::read(&file).unwrap().files.len(), 2);

    // Ben takes it in: his saves and his newest version stay as they are.
    let got = ben.import(&file).unwrap();
    assert_eq!(ben.read("saves/slot1.sav").as_deref(), Some("ben's farm, year 1"));
    assert_eq!(ben.read("saves/meta/info.xml"), None);
    let list = ben.versions();
    assert_eq!(list.iter().filter(|s| s.head).map(|s| s.id.clone()).collect::<Vec<_>>(), vec![his.clone()]);
    let row = list.iter().find(|s| s.id == got).expect("the shared save is in the line");
    assert!(row.side && row.locked, "{row:#?}");
    assert_eq!(row.trigger, Trigger::Shared);
    assert_eq!(row.platform, shared.platform);
    assert!(!row.comment.is_empty());
    assert!(!row.device_name.to_lowercase().contains("anna"));
    assert_eq!(ben.status(), Status::Synced);
    ben.activate();
    assert!(state::game("farm-b").pending.is_empty());

    // He makes it current when he wants it; what he had is still there.
    {
        let ctx = ben.ctx();
        sync::restore(&ctx, &ben.entry(), &got).unwrap();
    }
    assert_eq!(ben.read("saves/slot1.sav").as_deref(), Some("anna's farm, year 3"));
    assert_eq!(ben.read("saves/meta/info.xml").as_deref(), Some("<year>3</year>"));
    assert!(ben.versions().iter().any(|s| s.id == his));

    // Taken in twice is two saves, not an error.
    let again = ben.import(&file).unwrap();
    assert_ne!(again, got);

    // Another game doesn't take it.
    let carl_cloud = Drive::new("share-carl");
    let carl = Device::new(dir.path(), &carl_cloud, "carl", "celeste", "Celeste");
    carl.write("saves/0.celeste", "chapter 2");
    let own = uploaded(carl.sync());
    let refused = carl.import(&file).unwrap_err().to_string();
    assert!(refused.contains("Stardew Valley") && refused.contains("Celeste"), "{refused}");

    // A file that was changed on the way, one from a newer Ludonimbus, and
    // one that is no shared save at all: refused, and nothing is added.
    let before = ben.versions().len();
    let mut broken = inside.clone();
    let slot = inside.keys().find(|n| n.ends_with("slot1.sav")).unwrap().clone();
    broken.insert(slot, b"someone else's bytes".to_vec());
    let bad = dir.path().join("changed.lnsave");
    write_zip(&bad, &broken);
    assert!(ben.import(&bad).is_err());

    let mut newer = inside.clone();
    let mut info: serde_json::Value = serde_json::from_slice(&inside["share.json"]).unwrap();
    info["format"] = serde_json::json!(share::FORMAT + 1);
    newer.insert("share.json".to_string(), serde_json::to_vec(&info).unwrap());
    let future = dir.path().join("newer.lnsave");
    write_zip(&future, &newer);
    assert!(ben.import(&future).is_err());

    let junk = dir.path().join("junk.lnsave");
    fs::write(&junk, b"not a zip").unwrap();
    assert!(ben.import(&junk).is_err());

    // A file made to drop files where no save of the game is, whole and as
    // its hashes say: beside the game's program, in the Startup folder.
    let crafted = |name: &str, files: &[(&str, &str, Option<u64>)]| {
        let mut zip = BTreeMap::new();
        let mut info: serde_json::Value = serde_json::from_slice(&inside["share.json"]).unwrap();
        let mut list = Vec::new();
        for (path, data, size) in files {
            zip.insert(format!("files/{path}"), data.as_bytes().to_vec());
            let hash = blake3::hash(data.as_bytes()).to_hex().to_string();
            list.push(serde_json::json!({ "path": path, "size": size.unwrap_or(data.len() as u64), "hash": hash, "mtime": 0 }));
        }
        info["files"] = serde_json::Value::Array(list);
        zip.insert("share.json".to_string(), serde_json::to_vec(&info).unwrap());
        let path = dir.path().join(format!("{name}.lnsave"));
        write_zip(&path, &zip);
        path
    };
    let dropper = crafted(
        "dropper",
        &[
            ("install/saves/slot1.sav", "a farm", None),
            ("install/version.dll", "not a save", None),
            ("appdata/Microsoft/Windows/Start Menu/Programs/Startup/run.bat", "not a save", None),
        ],
    );
    let refused = ben.import(&dropper).unwrap_err().to_string();
    assert!(refused.contains("install/version.dll"), "{refused}");
    // Sizes that add up past what a number holds, and a file longer than
    // it says: refused, not a crash, and not read whole.
    let huge = crafted("huge", &[("install/saves/a.sav", "a", Some(1 << 63)), ("install/saves/b.sav", "b", Some(1 << 63))]);
    assert!(ben.import(&huge).is_err());
    let longer = crafted("longer", &[("install/saves/slot1.sav", "much longer than it says", Some(4))]);
    assert!(ben.import(&longer).is_err());
    assert_eq!(ben.versions().len(), before);
    assert_eq!(carl.versions().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), vec![own]);
}
