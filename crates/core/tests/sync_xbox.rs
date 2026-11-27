//! The Xbox app's version of a game shares a save line with the game's other
//! PC versions through its save containers (docs/plan-v2.md, sections 4.2
//! and 6): its saves are read as the files the other versions keep, and a
//! version of the line is written back into the containers only when the
//! user asks, all or nothing. Every device has its own data folder and
//! fake containers in a temporary folder; the cloud is in memory. One test,
//! because the data directory is chosen through an environment variable.

use anyhow::Result;
use ludonimbus_core::bind::{Bind, Codec};
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::GameRecord;
use ludonimbus_core::cloud::Cloud;
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::join::{self, Fit};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use ludonimbus_core::{wgs, xboxsaves};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Celeste's package: one container per file, named like the file.
const PACKAGE: &str = "MattMakesGamesInc.Celeste_79daxvg0dq3v6";
/// Two Xbox accounts: each has a container folder of its own.
const USER_A: &str = "000901F2A1B2C3D4_000000000000000000000000646E3F8B";
const USER_B: &str = "000901F2A1B2C3D5_000000000000000000000000646E3F8B";

/// The cloud: one store in memory that every device talks to.
#[derive(Clone, Default)]
struct Shared(Arc<Memory>);

impl Shared {
    fn records(&self) -> Vec<GameRecord> {
        library::load(self.0.as_ref(), &mut LibraryCache::default()).unwrap().records
    }

    fn record(&self, id: &str) -> GameRecord {
        library::get(&self.records(), id).cloned().unwrap_or_else(|| panic!("no record {id}"))
    }
}

impl Store for Shared {
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
        self.0.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.0.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.0.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.0.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.0.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.0.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("sync-xbox-test".to_string())
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A container folder of Celeste as the Xbox app writes it, one container
/// per file.
fn containers(dir: &Path, files: &[(&str, &[u8])]) {
    containers_of(dir, PACKAGE, files)
}

/// A container folder of the game `package`, one container per file, whose
/// one file is called `Data`.
fn containers_of(dir: &Path, package: &str, files: &[(&str, &[u8])]) {
    let mut entries = Vec::new();
    for (i, (name, data)) in files.iter().enumerate() {
        let guid = wgs::new_guid();
        let blob = wgs::new_guid();
        let folder = dir.join(wgs::hex(&guid));
        write(&folder.join(wgs::hex(&blob)), data);
        let m = wgs::Manifest { header: 4, files: vec![wgs::BlobRef { name: "Data".into(), cloud: blob, local: blob }] };
        write(&folder.join("container.1"), &wgs::write_manifest(&m).unwrap());
        entries.push(wgs::Entry {
            name: name.to_string(),
            name2: name.to_string(),
            etag: format!("\"0x8DEBCCC41BE96{i:02}\""),
            number: 1,
            state: 1,
            guid,
            filetime: 133_000_000_000_000_000,
            reserved: 0,
            size: data.len() as i64,
        });
    }
    let idx = wgs::Index {
        version: 14,
        display_name: String::new(),
        package: format!("{package}!Game"),
        filetime: 133_000_000_000_000_000,
        flags: 3,
        root: String::new(),
        reserved: [0, 0, 0, 0x10, 0, 0, 0, 0],
        entries,
    };
    write(&dir.join(wgs::INDEX), &wgs::write_index(&idx));
}

/// Every file below `dir` with its content: what "unchanged" is held to.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), fs::read(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

/// A device with one version of the game.
struct Device {
    home: PathBuf,
    /// The game's folder (a Steam version keeps its saves in `Saves` below).
    game: PathBuf,
    /// The container folder (an Xbox version).
    wgs: PathBuf,
    key: String,
    store: Shared,
}

impl Device {
    fn new(root: &Path, store: &Shared, name: &str, entry: GameEntry) -> Device {
        let dev = Device {
            home: root.join(name).join("home"),
            game: root.join(name).join("game"),
            wgs: PathBuf::new(),
            key: entry.key.clone(),
            store: store.clone(),
        };
        fs::create_dir_all(&dev.home).unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        // The errors are read below.
        cfg.language = "en".into();
        cfg.games.push(entry);
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    /// The Steam version: its saves in `<game>/Saves`.
    fn steam(root: &Path, store: &Shared, name: &str, key: &str, link: &str) -> Device {
        Device::steam_in(root, store, name, key, link, &["Saves"])
    }

    /// The Steam version, with its saves in these folders below the game's.
    fn steam_in(root: &Path, store: &Shared, name: &str, key: &str, link: &str, dirs: &[&str]) -> Device {
        let game = root.join(name).join("game");
        write(&game.join("Celeste.exe"), b"exe");
        let entry = GameEntry {
            key: key.into(),
            name: "Celeste".into(),
            exe: Some(game.join("Celeste.exe")),
            saves: dirs.iter().map(|d| format!("{{install}}/{d}/**")).collect(),
            game: Some(link.into()),
            auto_sync: true,
            ..Default::default()
        };
        Device::new(root, store, name, entry)
    }

    /// The Xbox version keeps its settings as a plain file too, in `cfg`
    /// next to the game's folder (the `game` folder is that one from here
    /// on).
    fn with_settings(mut self) -> Device {
        let cfg = self.home.parent().unwrap().join("cfg");
        fs::create_dir_all(&cfg).unwrap();
        self.activate();
        config::update(|c| {
            let g = c.game_mut(&self.key).unwrap();
            g.saves.push("{cfg}/**".into());
            g.roots.insert("cfg".into(), cfg.to_string_lossy().into_owned());
            Ok(())
        })
        .unwrap();
        self.game = cfg;
        self
    }

    /// The Xbox app's version, started through its launch helper, with its
    /// containers in the folder of the Xbox account `user`. Its saves are
    /// everything in its `wgs` folder, the way Ludusavi's database has them.
    fn xbox(root: &Path, store: &Shared, name: &str, key: &str, link: &str, user: &str, files: &[(&str, &[u8])]) -> Device {
        let content = root.join(name).join("XboxGames").join("Celeste").join("Content");
        write(&content.join("gamelaunchhelper.exe"), b"exe");
        write(&content.join("MicrosoftGame.config"), br#"<Game><ExecutableList><Executable Name="Celeste.exe" Id="Game" /></ExecutableList></Game>"#);
        let wgs_root = root.join(name).join("Packages").join(PACKAGE).join("SystemAppData").join("wgs");
        let entry = GameEntry {
            key: key.into(),
            name: "Celeste".into(),
            exe: Some(content.join("gamelaunchhelper.exe")),
            saves: vec!["{data}/**".into()],
            roots: [("data".to_string(), wgs_root.to_string_lossy().into_owned())].into(),
            game: Some(link.into()),
            auto_sync: true,
            ..Default::default()
        };
        let mut dev = Device::new(root, store, name, entry);
        dev.wgs = wgs_root.join(user);
        containers(&dev.wgs, files);
        dev
    }

    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    /// This device, looking at the cloud anew.
    fn ctx(&self) -> Ctx {
        self.activate();
        let cfg = config::load().unwrap();
        let cloud = Cloud::new(Box::new(self.store.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    }

    fn entry(&self) -> GameEntry {
        self.activate();
        config::load().unwrap().game(&self.key).unwrap().clone()
    }

    fn write(&self, file: &str, content: &str) {
        write(&self.game.join(file), content.as_bytes());
    }

    fn remove(&self, file: &str) {
        fs::remove_file(self.game.join(file)).unwrap();
    }

    fn read(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.game.join(file)).ok()
    }

    /// The Xbox app saves the game: one container file changes.
    fn play_xbox(&self, container: &str, data: &[u8]) {
        assert_eq!(wgs::replace(&self.wgs, &[(container.into(), "Data".into(), data.to_vec())]).unwrap(), 1);
    }

    /// The file in the container `name`.
    fn slot(&self, name: &str) -> String {
        let (_, list) = wgs::read_dir(&self.wgs).unwrap();
        let c = list.into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no container {name}"));
        String::from_utf8(c.files[0].1.clone()).unwrap()
    }

    fn index(&self) -> wgs::Index {
        wgs::parse_index(&fs::read(self.wgs.join(wgs::INDEX)).unwrap()).unwrap()
    }

    fn try_sync(&self, resolution: Resolution, trigger: Trigger) -> Result<Outcome> {
        let ctx = self.ctx();
        sync::sync(&ctx, &ctx.game(&self.key).unwrap(), resolution, trigger)
    }

    fn sync_with(&self, resolution: Resolution) -> Outcome {
        self.try_sync(resolution, Trigger::SyncNow).unwrap()
    }

    fn sync(&self) -> Outcome {
        self.sync_with(Resolution::Auto)
    }

    /// "Make current" in the app.
    fn make_current(&self, id: &str) -> Result<Outcome> {
        let ctx = self.ctx();
        sync::restore(&ctx, &ctx.game(&self.key).unwrap(), id)
    }

    fn undo(&self) -> Outcome {
        let ctx = self.ctx();
        sync::undo(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    fn status(&self) -> Status {
        let ctx = self.ctx();
        sync::check(&ctx, &ctx.game(&self.key).unwrap()).status
    }

    /// The content hash of the saves here, as a sync sees them.
    fn hash(&self) -> String {
        snapshot::local_state(&self.entry()).unwrap().hash
    }

    fn suggestion(&self) -> Option<join::Suggestion> {
        let ctx = self.ctx();
        join::suggestion(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    fn accept(&self, s: &join::Suggestion) -> Outcome {
        let ctx = self.ctx();
        join::accept(&ctx, &ctx.game(&self.key).unwrap(), s).unwrap()
    }

    /// The paths of the files of version `id` on the line the game follows,
    /// and its content hash.
    fn version(&self, id: &str) -> (Vec<String>, String) {
        let meta = self.ctx().cloud().unwrap().get_meta(&self.key, id).unwrap();
        let mut files: Vec<String> = meta.files.into_iter().map(|f| f.path).collect();
        files.sort();
        (files, meta.content_hash)
    }

    fn platform_of(&self, id: &str) -> String {
        let list = self.ctx().cloud().unwrap().list_snapshots(&self.key).unwrap();
        list.into_iter().find(|s| s.id == id).unwrap().platform
    }
}

fn uploaded(outcome: Outcome) -> String {
    match outcome {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

fn pulled(outcome: Outcome) -> String {
    match outcome {
        Outcome::Pulled { id, .. } => id,
        other => panic!("expected a pull, got {other:?}"),
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn wgs_bind(canon: &str, local: &str) -> Bind {
    Bind { codec: Codec::Wgs, ..Bind::files(canon, local) }
}

#[test]
fn the_xbox_version_shares_a_line_through_its_containers() {
    let tmp = tempfile::tempdir().unwrap();
    steam_first(tmp.path());
    xbox_first(tmp.path());
    containers_and_files(tmp.path());
    not_single_files(tmp.path());
}

/// Files with no single container file behind them are read, but never
/// written back: one made up from the containers, two containers read out
/// under one name.
fn not_single_files(root: &Path) {
    std::env::set_var("LUDONIMBUS_HOME", root.join("N").join("home"));
    ludonimbus_core::i18n::set("en");
    let wgs_root = root.join("N").join("wgs");
    let dir = wgs_root.join(USER_A);
    let game = GameEntry {
        key: "n".into(),
        name: "N".into(),
        saves: vec!["{data}/**".into()],
        roots: [("data".to_string(), wgs_root.to_string_lossy().into_owned())].into(),
        bind: vec![wgs_bind("install/Saves", &format!("data/{USER_A}"))],
        ..Default::default()
    };
    let version_of = |files: &[(&str, &str)]| -> (PathBuf, Vec<snapshot::FileEntry>) {
        let from = root.join("N").join("version");
        let _ = fs::remove_dir_all(&from);
        let entries = files
            .iter()
            .map(|(path, data)| {
                write(&snapshot::resolve_in(&from, path).unwrap(), data.as_bytes());
                snapshot::FileEntry { path: path.to_string(), size: data.len() as u64, hash: blake3::hash(data.as_bytes()).to_hex().to_string(), mtime: 0 }
            })
            .collect();
        (from, entries)
    };

    // Control: every container has a made-up file beside its own.
    containers_of(&dir, "505GAMESS.P.A.ControlPCGP_tefn33qh9azfc", &[("Slot1", b"one")]);
    let (files, _) = snapshot::scan(&game).unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f.logical.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["install/Saves/Slot1/--containerDisplayName.chunk", "install/Saves/Slot1/Data.chunk"]);
    let before = tree(&dir);
    let (from, version) = version_of(&[("install/Saves/Slot1/--containerDisplayName.chunk", "Slot1"), ("install/Saves/Slot1/Data.chunk", "ONE")]);
    let err = snapshot::apply_dir(&from, &version, &game).unwrap_err();
    assert!(format!("{err:#}").contains("--containerDisplayName.chunk"), "{err:#}");
    assert_eq!(tree(&dir), before);

    // Two containers whose names differ in case only: one file to the other
    // versions, two in the containers.
    fs::remove_dir_all(&dir).unwrap();
    containers(&dir, &[("Slot1", b"one"), ("slot1", b"two")]);
    assert_eq!(snapshot::scan(&game).unwrap().0.len(), 1);
    let before = tree(&dir);
    let (from, version) = version_of(&[("install/Saves/Slot1", "ONE")]);
    let err = snapshot::apply_dir(&from, &version, &game).unwrap_err();
    assert!(format!("{err:#}").contains("isn't a single file"), "{err:#}");
    assert_eq!(tree(&dir), before);
}

/// An Xbox version with one folder of the line in its containers and
/// another as plain files: both are written, all or nothing.
fn containers_and_files(root: &Path) {
    let store = Shared::default();
    let a = Device::steam_in(root, &store, "M-A", "saga", "saga", &["Saves", "Config"]);
    a.write("Saves/0.celeste", "s1");
    a.write("Config/options.ini", "o1");
    uploaded(a.sync());

    let b = Device::xbox(root, &store, "M-B", "saga-xbox", "saga", USER_A, &[("0.celeste", b"x1")]).with_settings();
    b.write("options.ini", "xo");
    let s = b.suggestion().unwrap();
    let mut binds = s.binds.clone();
    binds.sort_by(|x, y| x.canon.cmp(&y.canon));
    assert_eq!(binds, vec![Bind::files("install/Config", "cfg"), wgs_bind("install/Saves", &format!("data/{USER_A}"))]);
    assert_eq!(s.fit, Fit::Ready);
    assert!(matches!(b.accept(&s), Outcome::Conflict { .. }));
    // The cloud's, chosen: into the containers and the folder.
    pulled(b.sync_with(Resolution::UseCloud));
    assert_eq!((b.slot("0.celeste"), b.read("options.ini")), ("s1".to_string(), Some("o1".to_string())));
    assert_eq!(b.status(), Status::Synced);

    // The plain file can't be put in place (a folder is where it goes): the
    // containers are put back too.
    a.write("Saves/0.celeste", "s2");
    a.write("Config/options.ini", "o2");
    let next = uploaded(a.sync());
    b.remove("options.ini");
    fs::create_dir_all(b.game.join("options.ini")).unwrap();
    let before = tree(&b.wgs);
    assert!(b.make_current(&next).is_err());
    assert_eq!(tree(&b.wgs), before);
    assert_eq!(b.slot("0.celeste"), "s1");
    // Out of the way: both go in.
    fs::remove_dir(b.game.join("options.ini")).unwrap();
    assert_eq!(pulled(b.make_current(&next).unwrap()), next);
    assert_eq!((b.slot("0.celeste"), b.read("options.ini")), ("s2".to_string(), Some("o2".to_string())));
    assert_eq!(b.hash(), a.hash());
}

/// The Steam version's line exists; the Xbox version joins it, asked once.
fn steam_first(root: &Path) {
    let store = Shared::default();
    let saves = |user: &str| format!("data/{user}");

    // A: the Steam version uploads first.
    let a = Device::steam(root, &store, "S-A", "celeste", "celeste");
    a.write("Saves/0.celeste", "chapter 7");
    a.write("Saves/settings.celeste", "settings A");
    let first = uploaded(a.sync());
    assert_eq!(a.version(&first).0, strings(&["install/Saves/0.celeste", "install/Saves/settings.celeste"]));

    // B: the Xbox version on another device. Its containers hold the same
    // two files under the names the Steam version uses: ready to share.
    let b = Device::xbox(root, &store, "S-B", "celeste-xbox", "celeste", USER_A, &[("0.celeste", b"xbox chapter 2"), ("settings.celeste", b"xbox settings")]);
    let s = b.suggestion().expect("the card has a line to share");
    assert_eq!((s.record.as_str(), s.fit, s.confirmed), ("celeste", Fit::Ready, false));
    assert_eq!(s.binds, vec![wgs_bind("install/Saves", &saves(USER_A))]);
    assert_eq!((s.matched, s.only_in_line, s.only_here), (2, 0, 0));
    assert!(s.uncovered.is_empty() && s.outside.is_empty(), "{s:#?}");
    assert!(b.entry().bind.is_empty());

    // Yes: the saves differ, so the one question is which to continue. The
    // containers are as they were.
    let untouched = tree(&b.wgs);
    assert!(matches!(b.accept(&s), Outcome::Conflict { .. }));
    assert_eq!(tree(&b.wgs), untouched);
    let entry = b.entry();
    assert_eq!(entry.record.as_deref(), Some("celeste"));
    assert_eq!(entry.bind, vec![wgs_bind("install/Saves", &saves(USER_A))]);
    assert_eq!(b.status(), Status::Conflict);
    // The game record says so too, codec included.
    let line = store.record("celeste").lines.into_iter().find(|l| l.id == "main").unwrap();
    assert_eq!(line.binds.len(), 1);
    assert_eq!(line.binds[0].map, vec![wgs_bind("install/Saves", &saves(USER_A))]);
    assert!(b.suggestion().is_none());

    // B keeps its own: the containers' files go to the line under the Steam
    // version's paths, as an upload from the Xbox app's version.
    let mine = uploaded(b.sync_with(Resolution::UseLocal));
    let (files, hash) = b.version(&mine);
    assert_eq!(files, strings(&["install/Saves/0.celeste", "install/Saves/settings.celeste"]));
    assert_eq!(b.platform_of(&mine), "pc:xbox");
    assert_eq!(tree(&b.wgs), untouched);
    // The Steam version gets them as its own files, by itself; the same
    // progress has the same content hash on both versions.
    assert_eq!(pulled(a.sync()), mine);
    assert_eq!(a.read("Saves/0.celeste").as_deref(), Some("xbox chapter 2"));
    assert_eq!(a.read("Saves/settings.celeste").as_deref(), Some("xbox settings"));
    assert_eq!(a.hash(), hash);
    assert_eq!(b.hash(), hash);

    // Played on the Xbox version: the container changes, the upload has the
    // Steam version's names, and the Steam device takes it by itself.
    b.play_xbox("0.celeste", b"xbox chapter 3");
    let after_xbox = uploaded(b.sync());
    assert_eq!(b.version(&after_xbox).0, strings(&["install/Saves/0.celeste", "install/Saves/settings.celeste"]));
    assert_eq!(pulled(a.sync()), after_xbox);
    assert_eq!(a.read("Saves/0.celeste").as_deref(), Some("xbox chapter 3"));
    assert_eq!(a.hash(), b.hash());

    // Played on the Steam version. A sync nobody chose writes nothing into
    // the containers, before playing or after: the cloud is newer.
    a.write("Saves/0.celeste", "chapter 9");
    let steam = uploaded(a.sync());
    let before = tree(&b.wgs);
    assert!(matches!(b.sync(), Outcome::UpToDate));
    assert!(matches!(b.try_sync(Resolution::Auto, Trigger::PreLaunch).unwrap(), Outcome::UpToDate));
    assert_eq!(tree(&b.wgs), before);
    assert_eq!(b.status(), Status::CloudNewer);

    // The user makes it current: it is written, only into the slot that
    // changed, and the containers read as exactly that version.
    let settings_entry = b.index().entries.into_iter().find(|e| e.name == "settings.celeste").unwrap();
    assert_eq!(pulled(b.make_current(&steam).unwrap()), steam);
    assert_eq!(b.slot("0.celeste"), "chapter 9");
    assert_eq!(b.slot("settings.celeste"), "xbox settings");
    assert_eq!(b.index().entries.into_iter().find(|e| e.name == "settings.celeste").unwrap(), settings_entry);
    assert_eq!(b.hash(), b.version(&steam).1);
    assert_eq!(b.status(), Status::Synced);
    assert!(matches!(b.sync(), Outcome::UpToDate));
    // Nothing of a write is left beside the containers.
    let (_, list) = wgs::read_dir(&b.wgs).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(tree(&b.wgs).len(), 1 + 2 * 2, "the index, and a manifest and a data file per container");

    // Undo puts back what was there, and it is the newest version again.
    let back = uploaded(b.undo());
    assert_eq!(b.slot("0.celeste"), "xbox chapter 3");
    assert_eq!(b.slot("settings.celeste"), "xbox settings");
    assert_eq!(b.hash(), b.version(&after_xbox).1);
    assert_eq!(pulled(a.sync()), back);
    assert_eq!(a.read("Saves/0.celeste").as_deref(), Some("xbox chapter 3"));

    // A version with a file the Xbox version has no slot for: refused,
    // nothing written.
    a.write("Saves/1.celeste", "a new slot");
    let more = uploaded(a.sync());
    let before = tree(&b.wgs);
    let err = b.make_current(&more).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("1.celeste") && text.contains("left as they were"), "{text}");
    assert_eq!(tree(&b.wgs), before);
    assert_eq!(b.status(), Status::CloudNewer);

    // A version without a file the Xbox version has: refused too.
    a.remove("Saves/1.celeste");
    a.remove("Saves/settings.celeste");
    a.write("Saves/0.celeste", "chapter 10");
    let fewer = uploaded(a.sync());
    assert_eq!(a.version(&fewer).0, strings(&["install/Saves/0.celeste"]));
    let err = b.make_current(&fewer).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("settings.celeste") && text.contains("left as they were"), "{text}");
    assert_eq!(tree(&b.wgs), before);

    // The write reads back wrong: what it replaced is put back.
    a.write("Saves/0.celeste", "chapter 11");
    a.write("Saves/settings.celeste", "settings 11");
    let next = uploaded(a.sync());
    xboxsaves::spoil_next_read_back();
    let err = b.make_current(&next).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("read back") && text.contains("left as they were"), "{text}");
    assert_eq!(tree(&b.wgs), before);
    assert_eq!(b.status(), Status::CloudNewer);
    // Again, and it goes in.
    assert_eq!(pulled(b.make_current(&next).unwrap()), next);
    assert_eq!((b.slot("0.celeste"), b.slot("settings.celeste")), ("chapter 11".to_string(), "settings 11".to_string()));
    assert_eq!(b.status(), Status::Synced);
}

/// The Xbox version's line came first: it keeps its containers as they
/// are, as before. A Steam version that comes later isn't offered that line,
/// but the Xbox versions are offered the Steam version's line.
fn xbox_first(root: &Path) {
    let store = Shared::default();

    // X: the Xbox version alone. Its line holds the container files.
    let x = Device::xbox(root, &store, "X-A", "tale-xbox", "tale", USER_A, &[("0.celeste", b"x1")]);
    let raw = uploaded(x.sync());
    let (files, _) = x.version(&raw);
    assert!(files.contains(&format!("data/{USER_A}/containers.index")), "{files:?}");
    assert_eq!(files.len(), 3, "{files:?}");
    assert!(x.suggestion().is_none());
    let at_raw = tree(&x.wgs);
    x.play_xbox("0.celeste", b"x2");
    uploaded(x.sync());
    // An old version of it restores the container files as they were, and
    // is the newest version again.
    uploaded(x.make_current(&raw).unwrap());
    assert_eq!(tree(&x.wgs), at_raw);
    assert_eq!(x.slot("0.celeste"), "x1");

    // Y: the Steam version, later. Nothing of the Xbox version's line
    // corresponds to its files: no suggestion, a line of its own.
    let y = Device::steam(root, &store, "X-B", "tale", "tale");
    y.write("Saves/0.celeste", "steam 1");
    assert!(y.suggestion().is_none());
    let steam = uploaded(y.sync());
    assert_eq!(y.version(&steam).0, strings(&["install/Saves/0.celeste"]));

    // The Xbox version is offered the Steam version's line.
    let s = x.suggestion().expect("the Steam version's line");
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![wgs_bind("install/Saves", &format!("data/{USER_A}"))]));
    assert!(matches!(x.accept(&s), Outcome::Conflict { .. }));
    assert_eq!(x.slot("0.celeste"), "x1");
    // Its own line of container files is still there.
    let card = store.records().into_iter().find(|r| r.merged_into.is_none() && r.lines.len() > 1).expect("one card");
    assert_eq!(card.lines.len(), 2);
    // The cloud's, chosen by the user: written into the containers.
    assert_eq!(pulled(x.sync_with(Resolution::UseCloud)), steam);
    assert_eq!(x.slot("0.celeste"), "steam 1");
    assert_eq!(x.status(), Status::Synced);

    // Z: the Xbox version on a device signed in to another Xbox account,
    // whose containers have what the line has. The folders confirmed on X
    // are used with its own container folder; the same saves on both sides
    // need no question.
    let z = Device::xbox(root, &store, "X-C", "tale-xbox-2", "tale", USER_B, &[("0.celeste", b"steam 1")]);
    let s = z.suggestion().expect("the line, as confirmed on X");
    assert_eq!((s.fit, s.confirmed), (Fit::Ready, true));
    assert_eq!(s.binds, vec![wgs_bind("install/Saves", &format!("data/{USER_B}"))]);
    let untouched = tree(&z.wgs);
    assert!(matches!(z.accept(&s), Outcome::UpToDate));
    assert_eq!(z.status(), Status::Synced);
    assert_eq!(tree(&z.wgs), untouched);
    assert_eq!(z.entry().bind, vec![wgs_bind("install/Saves", &format!("data/{USER_B}"))]);
    // And plays: the Steam version gets it.
    z.play_xbox("0.celeste", b"z1");
    let from_z = uploaded(z.sync());
    assert_eq!(pulled(y.sync()), from_z);
    assert_eq!(y.read("Saves/0.celeste").as_deref(), Some("z1"));
}
