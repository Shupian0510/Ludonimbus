//! A game whose saves are the same files on PC and on the Switch
//! (`adapters/cross-saves.yaml`: Stardew Valley) shares one save line
//! between its PC version and its Switch version (an emulator here), with
//! the bindings the rule gives, asked once (docs/plan-v2.md, section 6).
//! Every device has its own data folder and save folders in a temporary
//! folder; the cloud is in memory. One test, because the data directory is
//! chosen through an environment variable.

use anyhow::Result;
use ludonimbus_core::bind::Bind;
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::GameRecord;
use ludonimbus_core::cloud::{Cloud, CloudGame};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::join::{self, Fit};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::switch::{self, SaveKind};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TITLE: u64 = 0x0100_E650_02BB_8000;
/// The Switch game's save data, as its versions write it.
const NX: &str = "nxsave-0100e65002bb8000";
const NX_DEVICE: &str = "nxdevice-0100e65002bb8000";
/// The rule's folder on PC, as the PC versions here write it.
const PC: &str = "data/StardewValley/Saves";
/// The Eden profile's UUID bytes 01..10.
const EDEN_USER: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// A store in memory that writes down every request.
#[derive(Clone, Default)]
struct Logged {
    inner: Arc<Memory>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Logged {
    fn note(&self, what: &str, path: &str) {
        self.log.lock().unwrap().push(format!("{what} {path}"));
    }

    /// The requests since the last call.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    fn records(&self) -> Vec<GameRecord> {
        library::load(self.inner.as_ref(), &mut LibraryCache::default()).unwrap().records
    }

    fn record(&self, id: &str) -> GameRecord {
        library::get(&self.records(), id).cloned().unwrap_or_else(|| panic!("no record {id}"))
    }

    fn has_record(&self, id: &str) -> bool {
        library::get(&self.records(), id).is_some()
    }
}

impl Store for Logged {
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
        self.note("list", dir);
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.note("read", path);
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.note("download", path);
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.note("write", path);
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.note("upload", path);
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.note("remove", path);
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("sync-rule-test".to_string())
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A farm's `SaveGameInfo`, saved by game version `version`.
fn info(version: &str, farm: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\" encoding=\"utf-8\"?><Farmer><name>{farm}</name><gameVersion>{version}</gameVersion></Farmer>").into_bytes()
}

/// A portable Eden with one profile: the Switch game's saves are in its
/// `user` folder.
fn eden(dir: &Path) -> PathBuf {
    let exe = dir.join("eden.exe");
    write(&exe, b"exe");
    let user = dir.join("user");
    let mut profiles = vec![0u8; 0x650];
    profiles[0x10..0x20].copy_from_slice(&EDEN_USER);
    profiles[0x20..0x30].copy_from_slice(&EDEN_USER);
    write(&user.join("nand/system/save/8000000000000010/su/avators/profiles.dat"), &profiles);
    write(&user.join("config/qt-config.ini"), b"[System]\ncurrent_user=0\n");
    exe
}

/// A device with Stardew Valley: its PC version (farms in
/// `StardewValley/Saves`, settings beside them) or its Switch version in
/// Eden.
struct Device {
    home: PathBuf,
    dir: PathBuf,
    key: String,
    store: Logged,
    switch: bool,
}

impl Device {
    fn new(root: &Path, store: &Logged, name: &str, key: &str, entry: impl FnOnce(&Path) -> GameEntry, switch: bool) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), dir: root.join(format!("dev-{name}")), key: key.to_string(), store: store.clone(), switch };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.dir).unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        cfg.games.push(GameEntry { key: key.to_string(), auto_sync: true, ..entry(&dev.dir) });
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    /// The PC version, its saves found by the pattern `saves` below the
    /// root `data` (`{data}/StardewValley/**` as a rule would have it).
    fn pc_with(root: &Path, store: &Logged, name: &str, key: &str, saves: &str) -> Device {
        let saves = saves.to_string();
        Device::new(
            root,
            store,
            name,
            key,
            |dir| GameEntry {
                name: "Stardew Valley".into(),
                saves: vec![saves],
                roots: BTreeMap::from([("data".to_string(), dir.join("Roaming").to_string_lossy().into_owned())]),
                ..Default::default()
            },
            false,
        )
    }

    fn pc(root: &Path, store: &Logged, name: &str, key: &str) -> Device {
        Device::pc_with(root, store, name, key, "{data}/StardewValley/**")
    }

    /// The Switch version in Eden.
    fn switch(root: &Path, store: &Logged, name: &str, key: &str) -> Device {
        Device::new(
            root,
            store,
            name,
            key,
            |dir| GameEntry {
                name: "星露谷物语".into(),
                adapter: Some("eden".into()),
                exe: Some(eden(&dir.join("eden"))),
                saves: vec![format!("{{{NX}}}/**"), format!("{{{NX_DEVICE}}}/**")],
                ..Default::default()
            },
            true,
        )
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

    /// The folder the rule's saves are in on this device.
    fn saves(&self) -> PathBuf {
        match self.switch {
            true => switch::yuzu_save(&self.dir.join("eden").join("user"), SaveKind::Account, TITLE).unwrap(),
            false => self.dir.join("Roaming").join("StardewValley").join("Saves"),
        }
    }

    /// Where the Switch version keeps its device save; the PC version's
    /// `StardewValley` folder.
    fn other(&self) -> PathBuf {
        match self.switch {
            true => switch::yuzu_save(&self.dir.join("eden").join("user"), SaveKind::Device, TITLE).unwrap(),
            false => self.dir.join("Roaming").join("StardewValley"),
        }
    }

    fn farm(&self, farm: &str, save: &str, version: &str) {
        write(&self.saves().join(farm).join(farm), save.as_bytes());
        write(&self.saves().join(farm).join("SaveGameInfo"), &info(version, farm));
    }

    fn read_farm(&self, farm: &str) -> Option<String> {
        fs::read_to_string(self.saves().join(farm).join(farm)).ok()
    }

    fn read(&self, path: &Path) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    fn try_sync(&self, resolution: Resolution) -> Result<Outcome> {
        let ctx = self.ctx();
        sync::sync(&ctx, &ctx.game(&self.key).unwrap(), resolution, Trigger::SyncNow)
    }

    fn sync_with(&self, resolution: Resolution) -> Outcome {
        self.try_sync(resolution).unwrap()
    }

    fn sync(&self) -> Outcome {
        self.sync_with(Resolution::Auto)
    }

    fn check(&self) -> sync::StatusReport {
        let ctx = self.ctx();
        sync::check(&ctx, &ctx.game(&self.key).unwrap())
    }

    fn status(&self) -> Status {
        self.check().status
    }

    fn suggestion(&self) -> Option<join::Suggestion> {
        let ctx = self.ctx();
        join::suggestion(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    fn accept(&self, s: &join::Suggestion) -> Outcome {
        let ctx = self.ctx();
        join::accept(&ctx, &ctx.game(&self.key).unwrap(), s).unwrap()
    }

    /// The paths of the files of version `id` on the line the game follows.
    fn files_of(&self, id: &str) -> Vec<String> {
        let mut files: Vec<String> = self.ctx().cloud().unwrap().get_meta(&self.key, id).unwrap().files.into_iter().map(|f| f.path).collect();
        files.sort();
        files
    }

    fn newest(&self) -> ludonimbus_core::snapshot::SnapshotSummary {
        sync::mainline_head(&self.ctx().cloud().unwrap().list_snapshots(&self.key).unwrap()).unwrap()
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
    let mut out: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    out.sort();
    out
}

#[test]
fn stardew_valley_shares_one_line_between_pc_and_switch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    pc_first(root);
    switch_first(root);
    from_06(root);
}

/// A PC line whose newest version is one Ludonimbus 0.6 uploaded: its files
/// are in a zip beside its manifest, which no new version may point into.
/// The Switch version's first version fetches the PC's settings from there
/// and packs them anew.
fn from_06(root: &Path) {
    let store = Logged::default();
    let p7 = Device::pc(root, &store, "P7", "stardew");
    p7.farm("Farm_1", "<SaveGame>pc farm, 0.6</SaveGame>", "1.6.15");
    write(&p7.other().join("default_options"), b"pc settings, 0.6");
    // As 0.6 left it: the game's definition, and a snapshot with its zip.
    let entry = p7.entry();
    let local = snapshot::local_state(&entry).unwrap();
    let (meta, zip) = snapshot::build(&entry, &local, Trigger::AutoExit, None, ("d-06", "Old PC"), "").unwrap();
    store.inner.write("games/stardew/game.json", &serde_json::to_vec(&CloudGame::from(&entry)).unwrap()).unwrap();
    store.inner.write(&format!("games/stardew/snapshots/{}.json", meta.id), &serde_json::to_vec(&meta).unwrap()).unwrap();
    store.inner.upload(&format!("games/stardew/snapshots/{}.zip", meta.id), &zip).unwrap();
    assert!(matches!(p7.sync(), Outcome::UpToDate));

    // The Switch version had a line of its own (its first upload brought
    // the cloud to the new layout); it is offered the PC's.
    let s7 = Device::switch(root, &store, "S7", "stardew-eden");
    s7.farm("Farm_1", "<SaveGame>switch farm</SaveGame>", "1.6.15");
    uploaded(s7.sync());
    let s = s7.suggestion().expect("offered");
    assert_eq!((s.fit, s.binds.clone(), s.others), (Fit::Ready, vec![Bind::some(PC, NX, "*/*")], 1));
    assert!(matches!(s7.accept(&s), Outcome::Conflict { .. }));
    // The Switch's farm, over the PC's: the PC's settings come along.
    let from_switch = uploaded(s7.sync_with(Resolution::UseLocal));
    assert!(s7.files_of(&from_switch).contains(&"data/StardewValley/default_options".to_string()));
    assert_eq!(pulled(p7.sync()), from_switch);
    assert_eq!(p7.read_farm("Farm_1").as_deref(), Some("<SaveGame>switch farm</SaveGame>"));
    assert_eq!(p7.read(&p7.other().join("default_options")).as_deref(), Some("pc settings, 0.6"));
    // From then on they go by reference to that version's pack.
    s7.farm("Farm_1", "<SaveGame>switch farm 2</SaveGame>", "1.6.15");
    store.take();
    let next = uploaded(s7.sync());
    assert!(store.take().iter().all(|r| !r.contains("/snapshots/")), "the 0.6 zip isn't fetched again");
    assert_eq!(pulled(p7.sync()), next);
    assert_eq!(p7.read(&p7.other().join("default_options")).as_deref(), Some("pc settings, 0.6"));
}

/// The PC version uploads first; the Switch version on another device gets
/// the rule's bindings, is asked once, and from then on both get each
/// other's progress, each in its own folder.
fn pc_first(root: &Path) {
    let store = Logged::default();

    // P1: the PC version. One farm, saved by 1.6.15, a file the game keeps
    // beside the farms and its settings above them.
    let p1 = Device::pc(root, &store, "P1", "stardew");
    p1.farm("Farm_1", "<SaveGame>pc farm</SaveGame>", "1.6.15");
    write(&p1.saves().join("steam_autocloud.vdf"), b"pc cloud");
    write(&p1.other().join("default_options"), b"pc settings");
    let first = uploaded(p1.sync());
    assert_eq!(
        p1.files_of(&first),
        strings(&[
            "data/StardewValley/Saves/Farm_1/Farm_1",
            "data/StardewValley/Saves/Farm_1/SaveGameInfo",
            "data/StardewValley/Saves/steam_autocloud.vdf",
            "data/StardewValley/default_options",
        ])
    );

    // S1: the Switch version in Eden on another device, played on its own:
    // another farm state (saved by an older game), the game's own settings
    // file in the save data and a device save.
    let s1 = Device::switch(root, &store, "S1", "stardew-eden");
    s1.farm("Farm_1", "<SaveGame>switch farm</SaveGame>", "1.6.9");
    write(&s1.saves().join("startup_preferences"), b"switch settings");
    write(&s1.other().join("device.bin"), b"switch device");

    // The rule's bindings: the rule's folder in the line, the save data
    // here, only the farms' files. Nothing was written to find that out.
    store.take();
    let s = s1.suggestion().expect("the PC version's line is offered");
    assert!(store.take().iter().all(|r| r.starts_with("list ") || r.starts_with("read ")));
    assert_eq!((s.record.as_str(), s.fit, s.confirmed), ("stardew", Fit::Ready, false));
    assert_eq!(s.binds, vec![Bind::some(PC, NX, "*/*")]);
    assert_eq!((s.rule.as_deref(), s.from.as_str(), s.device.as_str()), (Some("Stardew Valley"), "PC", "P1"));
    assert_eq!(s.matched, 2);
    // The PC's settings and the file beside the farms are the PC's, not
    // files this version lacks; this version's own other files stay here.
    assert!(s.uncovered.is_empty());
    assert_eq!(s.others, 2);
    assert_eq!(s.outside, strings(&[&format!("{NX}/startup_preferences"), &format!("{NX_DEVICE}/device.bin")]));
    assert!(s1.entry().bind.is_empty());

    // Yes: the saves differ, so the one question is which to continue.
    assert!(matches!(s1.accept(&s), Outcome::Conflict { .. }));
    let entry = s1.entry();
    assert_eq!((entry.record.as_deref(), entry.bind.clone()), (Some("stardew"), vec![Bind::some(PC, NX, "*/*")]));
    assert_eq!(join::by_rule(&entry).map(|r| r.id.as_str()), Some("stardew-valley"));
    assert_eq!(s1.status(), Status::Conflict);
    assert_eq!(s1.read_farm("Farm_1").as_deref(), Some("<SaveGame>switch farm</SaveGame>"));
    // The line says how such versions take part in it, and it is asked no more.
    let line = store.record("stardew").lines.into_iter().find(|l| l.id == "main").unwrap();
    assert_eq!(line.binds.len(), 1);
    assert_eq!(line.binds[0].map, vec![Bind::some(PC, NX, "*/*")]);
    assert!(s1.suggestion().is_none());

    // The cloud's: the PC's farm arrives in the save data; the PC's other
    // files don't, and the Switch's own other files stay as they are.
    assert_eq!(pulled(s1.sync_with(Resolution::UseCloud)), first);
    assert_eq!(s1.read_farm("Farm_1").as_deref(), Some("<SaveGame>pc farm</SaveGame>"));
    assert_eq!(s1.read(&s1.saves().join("startup_preferences")).as_deref(), Some("switch settings"));
    assert_eq!(s1.read(&s1.other().join("device.bin")).as_deref(), Some("switch device"));
    assert!(!s1.saves().join("steam_autocloud.vdf").exists() && !s1.saves().join("default_options").exists());
    // What was here is kept beside the history, as a whole version of the
    // line (the PC's files carried along), and can be gone back to.
    let r = sync::undoable(&s1.key).expect("undoable");
    assert_eq!(r.now, first);
    assert_eq!(
        s1.files_of(&r.back),
        strings(&[
            "data/StardewValley/Saves/Farm_1/Farm_1",
            "data/StardewValley/Saves/Farm_1/SaveGameInfo",
            "data/StardewValley/Saves/steam_autocloud.vdf",
            "data/StardewValley/default_options",
        ])
    );
    // In sync, although the line's version has files this version doesn't.
    assert_eq!(s1.status(), Status::Synced);
    assert!(matches!(s1.sync(), Outcome::UpToDate));
    assert!(matches!(p1.sync(), Outcome::UpToDate));
    // Nothing to do costs what it costs any game.
    store.take();
    assert!(matches!(s1.sync(), Outcome::UpToDate));
    let quiet_switch = store.take();
    assert!(matches!(p1.sync(), Outcome::UpToDate));
    let quiet_pc = store.take();
    assert_eq!(quiet_switch.len(), quiet_pc.len(), "{quiet_switch:?} {quiet_pc:?}");

    // S1 plays on: its version holds its farm under the line's paths, and
    // the PC's files carried over from the version it was at, by reference.
    s1.farm("Farm_1", "<SaveGame>switch day 2</SaveGame>", "1.6.15");
    store.take();
    let from_switch = uploaded(s1.sync());
    let switch_upload = store.take();
    assert_eq!(switch_upload.iter().filter(|r| r.starts_with("upload ")).count(), 1, "{switch_upload:?}");
    assert_eq!(p1.files_of(&from_switch), p1.files_of(&first));
    assert_eq!(s1.newest().platform, "emu:eden");
    assert_eq!(s1.newest().game_version.as_deref(), Some("1.6.15"));

    // P1 gets it as its own files; its settings and the file beside the
    // farms are as they were.
    assert_eq!(pulled(p1.sync()), from_switch);
    assert_eq!(p1.read_farm("Farm_1").as_deref(), Some("<SaveGame>switch day 2</SaveGame>"));
    assert_eq!(p1.read(&p1.other().join("default_options")).as_deref(), Some("pc settings"));
    assert_eq!(p1.read(&p1.saves().join("steam_autocloud.vdf")).as_deref(), Some("pc cloud"));
    assert_eq!(p1.status(), Status::Synced);

    // And back: a new farm on PC, and the PC's settings change.
    p1.farm("Farm_2", "<SaveGame>second farm</SaveGame>", "1.6.15");
    write(&p1.other().join("default_options"), b"pc settings 2");
    store.take();
    let from_pc = uploaded(p1.sync());
    // What carrying the PC's files over costs: one listing of the packs, to
    // see that the ones it points into are still there.
    let pc_upload = store.take();
    assert_eq!(switch_upload.len(), pc_upload.len() + 1, "{switch_upload:?} {pc_upload:?}");
    assert!(switch_upload.contains(&"list games/stardew/packs".to_string()) && !pc_upload.contains(&"list games/stardew/packs".to_string()));
    assert_eq!(pulled(s1.sync()), from_pc);
    assert_eq!(s1.read_farm("Farm_2").as_deref(), Some("<SaveGame>second farm</SaveGame>"));
    assert_eq!(s1.read(&s1.saves().join("startup_preferences")).as_deref(), Some("switch settings"));
    assert!(!s1.saves().join("default_options").exists());
    assert_eq!(s1.status(), Status::Synced);
    // The Switch's next version carries the PC's new settings.
    s1.farm("Farm_2", "<SaveGame>second farm, switch</SaveGame>", "1.6.15");
    let again = uploaded(s1.sync());
    assert_eq!(pulled(p1.sync()), again);
    assert_eq!(p1.read(&p1.other().join("default_options")).as_deref(), Some("pc settings 2"));
    assert_eq!(p1.read_farm("Farm_2").as_deref(), Some("<SaveGame>second farm, switch</SaveGame>"));

    // S2: the same kind of version on a third device, nothing played yet.
    // The folders were confirmed once: it shares the line at its first
    // sync, without a question, a record or a folder of its own.
    let s2 = Device::switch(root, &store, "S2", "stardew-eden-2");
    store.take();
    assert_eq!(pulled(s2.sync()), again);
    let requests = store.take();
    assert_eq!(requests.iter().filter(|r| *r == "list library").count(), 1, "{requests:?}");
    assert!(requests.iter().all(|r| !r.starts_with("write library/")), "{requests:?}");
    assert_eq!(s2.entry().bind, vec![Bind::some(PC, NX, "*/*")]);
    assert_eq!(s2.read_farm("Farm_2").as_deref(), Some("<SaveGame>second farm, switch</SaveGame>"));
    assert!(!store.has_record("stardew-eden-2"));

    version_guard(&p1, &s1);
    leave_and_decline(root, &store, &s1);
}

/// A version written by a newer game on the other platform: not taken by a
/// sync nobody chose when the rule says an older game can't load it, taken
/// when chosen. Older and unknown versions go as ever.
fn version_guard(p1: &Device, s1: &Device) {
    // The Switch is at 1.6.9 again (its game is older), the PC at 1.6.16.
    s1.farm("Farm_1", "<SaveGame>switch, old game</SaveGame>", "1.6.9");
    s1.farm("Farm_2", "<SaveGame>second farm, old game</SaveGame>", "1.6.9");
    let older = uploaded(s1.sync());
    // Older than the PC's own: taken as ever.
    assert_eq!(pulled(p1.sync()), older);
    p1.farm("Farm_1", "<SaveGame>pc, new game</SaveGame>", "1.6.16");
    let newer = uploaded(p1.sync());
    assert_eq!(p1.newest().game_version.as_deref(), Some("1.6.16"));

    // Not by itself: nothing is written, the cloud shows as newer, and the
    // check says which game version saved it.
    assert!(matches!(s1.sync(), Outcome::UpToDate));
    assert_eq!(s1.read_farm("Farm_1").as_deref(), Some("<SaveGame>switch, old game</SaveGame>"));
    let report = s1.check();
    assert_eq!((report.status, report.too_new.as_deref()), (Status::CloudNewer, Some("1.6.16")));
    let ctx = s1.ctx();
    let game = ctx.game(&s1.key).unwrap();
    assert_eq!(sync::saves_version(&game).as_deref(), Some("1.6.9"));
    assert!(sync::too_new(&game, Some("1.6.9"), "pc:steam", Some("1.6.16")));
    assert!(!sync::too_new(&game, Some("1.6.9"), "console:switch", Some("1.6.16")), "the same platform's versions aren't compared");
    assert!(!sync::too_new(&game, None, "pc", Some("1.6.16")) && !sync::too_new(&game, Some("1.6.9"), "pc", None));
    // Chosen: taken.
    assert_eq!(pulled(s1.sync_with(Resolution::UseCloud)), newer);
    assert_eq!(s1.read_farm("Farm_1").as_deref(), Some("<SaveGame>pc, new game</SaveGame>"));
    assert_eq!(s1.status(), Status::Synced);

    // A version that doesn't say which game wrote it: taken as ever.
    fs::remove_file(p1.saves().join("Farm_1").join("SaveGameInfo")).unwrap();
    fs::remove_file(p1.saves().join("Farm_2").join("SaveGameInfo")).unwrap();
    let unknown = uploaded(p1.sync());
    assert_eq!(p1.newest().game_version, None);
    assert_eq!(pulled(s1.sync()), unknown);
    assert!(!s1.saves().join("Farm_1").join("SaveGameInfo").exists());
}

/// Taking a version back to its own line, and saying no.
fn leave_and_decline(root: &Path, store: &Logged, s1: &Device) {
    // S1 goes back to a line of its own version: the PC's line is not
    // offered again, and the files here are this version's own again.
    let ctx = s1.ctx();
    let outcome = join::leave(&ctx, &ctx.game(&s1.key).unwrap()).unwrap();
    let own = uploaded(outcome);
    let entry = s1.entry();
    assert!(entry.bind.is_empty() && entry.record.is_none() && entry.declined.len() == 1);
    assert!(s1.files_of(&own).iter().all(|f| f.starts_with(NX) || f.starts_with(NX_DEVICE)), "{:?}", s1.files_of(&own));
    assert!(s1.files_of(&own).contains(&format!("{NX}/startup_preferences")));
    assert!(s1.suggestion().is_none());
    assert!(matches!(s1.sync(), Outcome::UpToDate));

    // The line S1 has now is on the card too. Switch versions take part in
    // the PC's line (S2 still does), so it is the line they left: a PC
    // version is not offered it.
    assert_eq!(store.record("stardew").lines.len(), 2, "{:?}", store.record("stardew").lines);
    let p5 = Device::pc_with(root, store, "P5", "stardew-p5", "{data}/StardewValley/Saves/**");
    p5.farm("Farm_9", "<SaveGame>p5</SaveGame>", "1.6.15");
    assert!(p5.suggestion().is_none_or(|s| s.rule.is_none()));
}

/// The Switch version uploads first, with files that are no PC version's;
/// the PC version joins with the rule's bindings and carries those files
/// on in every version it uploads.
fn switch_first(root: &Path) {
    let store = Logged::default();

    // S3: the Switch version in Eden. A farm, the game's settings in the
    // save data (a file at its top, outside the farms) and a device save.
    let s3 = Device::switch(root, &store, "S3", "stardew-eden");
    s3.farm("Farm_1", "<SaveGame>farm</SaveGame>", "1.6.15");
    write(&s3.saves().join("startup_preferences"), b"switch settings");
    write(&s3.other().join("device.bin"), b"switch device");
    let first = uploaded(s3.sync());
    assert_eq!(
        s3.files_of(&first),
        strings(&[&format!("{NX}/Farm_1/Farm_1"), &format!("{NX}/Farm_1/SaveGameInfo"), &format!("{NX}/startup_preferences"), &format!("{NX_DEVICE}/device.bin")])
    );

    // P3: the PC version, the same farm, and settings of its own.
    let p3 = Device::pc(root, &store, "P3", "stardew");
    p3.farm("Farm_1", "<SaveGame>farm</SaveGame>", "1.6.15");
    write(&p3.other().join("default_options"), b"pc settings");
    let s = p3.suggestion().expect("the Switch version's line is offered");
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![Bind::some(NX, PC, "*/*")]));
    assert_eq!((s.from.as_str(), s.others, s.matched), ("Switch", 2, 2));
    assert_eq!(s.outside, strings(&["data/StardewValley/default_options"]));
    // The same farm: the files the line has and this version doesn't are
    // the Switch's, so the saves are the same. Shared without a question.
    assert!(matches!(p3.accept(&s), Outcome::UpToDate));
    assert_eq!(p3.status(), Status::Synced);
    assert!(!store.has_record("stardew"));

    // P3 plays: its version carries the Switch's files on.
    p3.farm("Farm_1", "<SaveGame>farm, pc day 2</SaveGame>", "1.6.15");
    let from_pc = uploaded(p3.sync());
    assert_eq!(s3.files_of(&from_pc), s3.files_of(&first));
    let carried = s3.ctx().cloud().unwrap().get_meta(&s3.key, &from_pc).unwrap();
    assert!(!carried.files.iter().any(|f| f.path.contains("default_options")));
    // S3 gets it, and still has its settings and its device save.
    assert_eq!(pulled(s3.sync()), from_pc);
    assert_eq!(s3.read_farm("Farm_1").as_deref(), Some("<SaveGame>farm, pc day 2</SaveGame>"));
    assert_eq!(s3.read(&s3.saves().join("startup_preferences")).as_deref(), Some("switch settings"));
    assert_eq!(s3.read(&s3.other().join("device.bin")).as_deref(), Some("switch device"));
    assert!(!s3.saves().join("default_options").exists());

    // The Switch changes only its own settings: the PC version's saves are
    // the same as before, and the next version is taken without a question.
    write(&s3.saves().join("startup_preferences"), b"switch settings 2");
    let settings = uploaded(s3.sync());
    assert_eq!(p3.status(), Status::CloudNewer);
    assert_eq!(pulled(p3.sync()), settings);
    assert_eq!(p3.read(&p3.other().join("default_options")).as_deref(), Some("pc settings"));
    p3.farm("Farm_1", "<SaveGame>farm, pc day 3</SaveGame>", "1.6.15");
    let next = uploaded(p3.sync());
    assert_eq!(pulled(s3.sync()), next);
    assert_eq!(s3.read(&s3.saves().join("startup_preferences")).as_deref(), Some("switch settings 2"));
    assert_eq!(s3.read_farm("Farm_1").as_deref(), Some("<SaveGame>farm, pc day 3</SaveGame>"));

    // P4: the same kind of PC version on another device (the same key, as a
    // game added from the cloud has), with saves of its own already. It
    // shares the line at its first sync; which saves to continue is asked.
    let p4 = Device::pc(root, &store, "P4", "stardew");
    p4.farm("Farm_1", "<SaveGame>p4 farm</SaveGame>", "1.6.15");
    assert!(matches!(p4.sync(), Outcome::Conflict { .. }));
    assert_eq!(p4.entry().bind, vec![Bind::some(NX, PC, "*/*")]);
    assert_eq!(pulled(p4.sync_with(Resolution::UseCloud)), next);
    assert_eq!(p4.read_farm("Farm_1").as_deref(), Some("<SaveGame>farm, pc day 3</SaveGame>"));

    // P4's save location is about to be changed by hand: it keeps its saves
    // apart again, nothing here changes, nothing is declined, and the line
    // is offered again.
    let ctx = p4.ctx();
    assert!(join::forget(&ctx, &ctx.game("stardew").unwrap()).unwrap());
    let entry = p4.entry();
    assert!(entry.bind.is_empty() && entry.record.is_none() && entry.declined.is_empty(), "{entry:#?}");
    assert_eq!(p4.read_farm("Farm_1").as_deref(), Some("<SaveGame>farm, pc day 3</SaveGame>"));
    let s = p4.suggestion().unwrap();
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![Bind::some(NX, PC, "*/*")]));

    // A PC version that says no: not offered again, and it keeps a line of
    // its own (its own files, under its own paths).
    let p6 = Device::pc_with(root, &store, "P6", "stardew-p6", "{data}/StardewValley/Saves/**");
    p6.farm("Farm_1", "<SaveGame>p6</SaveGame>", "1.6.15");
    let s = p6.suggestion().expect("offered");
    assert_eq!(s.binds, vec![Bind::some(NX, PC, "*/*")]);
    p6.activate();
    join::decline(&p6.key, &s.def).unwrap();
    assert!(p6.suggestion().is_none_or(|o| o.def != s.def));
    let own = uploaded(p6.sync());
    assert_eq!(p6.files_of(&own), strings(&["data/StardewValley/Saves/Farm_1/Farm_1", "data/StardewValley/Saves/Farm_1/SaveGameInfo"]));
    assert!(p6.entry().bind.is_empty());
}
