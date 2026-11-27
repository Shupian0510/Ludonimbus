//! Playtime, achievements, screenshots and artwork belong to the game, not
//! to a key (docs/plan-v2.md, section 3.6): a game whose record has two
//! folders, installs that joined a record made under another key, files a
//! device on 0.6 wrote, what it all costs, and artwork kept in another of
//! the game's folders. One test, because the data directory is chosen
//! through an environment variable.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use ludonimbus_core::achievements::{self, cloud as ach_cloud, Achievement, Schema, SourceKind, Unlock};
use ludonimbus_core::cloud::gamedata::Folders;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::{self, GameRecord, Line};
use ludonimbus_core::cloud::{library, Cloud};
use ludonimbus_core::config::{self, AppConfig, GameEntry};
use ludonimbus_core::playtime::{self, Via};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::state;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::{artsync, localart, screenshots};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Two keys of 0.6 that are one game. The upgrade made them one card,
/// `celeste` first; each key kept its folder.
const STEAM: &str = "celeste";
const XBOX: &str = "celeste-xbox";
/// Versions on device C that joined that card although neither key is a
/// folder of it.
const GOG: &str = "celeste-gog";
const EPIC: &str = "celeste-epic";
/// A game with one folder, as almost every game has.
const SOLO: &str = "solo";

/// What a device on 0.6 left in the Xbox version's folder: names and
/// contents as 0.6 writes them (sessions without `platform` and `line`).
const OLD_PLAYTIME: &str = r#"{"version":1,"device":"d-old","deviceName":"OLD","steamBaseline":null,"sessions":[{"start":"2026-11-01T10:00:00Z","end":"2026-11-01T11:00:00Z","secs":3600,"via":"direct"}],"updatedAt":"2026-11-01T11:00:00Z"}"#;
const OLD_UNLOCKS: &str = r#"{"device":"d-old","deviceName":"OLD","updatedAt":null,"unlocks":[{"id":"3","unlockedAt":"2026-11-01T10:30:00Z","hardcore":false}]}"#;

/// A store in memory that writes down every request. Small files it has
/// served are kept on the device, as with a cloud that is far away.
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

    /// The files under `dir`, by name relative to it.
    fn files(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        self.inner.paths().into_iter().filter_map(|p| p.strip_prefix(&prefix).map(str::to_string)).collect()
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
        Some("game-data-test".to_string())
    }
}

struct Device {
    id: &'static str,
    home: PathBuf,
}

impl Device {
    fn new(root: &Path, id: &'static str) -> Device {
        let dev = Device { id, home: root.join(id) };
        fs::create_dir_all(dev.home.join("Steam")).unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.id = id.to_string();
        cfg.device.name = id.to_uppercase();
        // An empty Steam folder of its own: never the real one.
        cfg.steam.root = Some(dev.home.join("Steam"));
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    /// Act as this device: Ludonimbus's own files live in its home.
    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    fn cfg(&self) -> AppConfig {
        config::load().unwrap()
    }

    fn cloud(&self, store: &Logged) -> Cloud {
        Cloud::new(Box::new(store.clone()), self.id, &self.id.to_uppercase()).for_config(&self.cfg())
    }

    /// Add a game with a save to this device.
    fn add(&self, key: &str) -> GameEntry {
        self.add_as(key, None)
    }

    /// Add a game that continues the saves of the card `record`, which the
    /// cloud has under another key: what [`Cloud`] settles for a version of
    /// a game another device added first.
    fn add_as(&self, key: &str, record: Option<&str>) -> GameEntry {
        let dir = self.home.join("games").join(key);
        fs::create_dir_all(dir.join("save")).unwrap();
        fs::write(dir.join("game.exe"), b"exe").unwrap();
        fs::write(dir.join("save").join("slot1.sav"), format!("{key} on {}", self.id)).unwrap();
        let game = GameEntry {
            key: key.into(),
            name: key.into(),
            exe: Some(dir.join("game.exe")),
            saves: vec!["{install}/save/**".into()],
            auto_sync: true,
            record: record.map(str::to_string),
            ..Default::default()
        };
        config::update(|c| {
            c.games.push(game.clone());
            Ok(())
        })
        .unwrap();
        game
    }
}

struct World {
    store: Logged,
    a: Device,
    b: Device,
    c: Device,
    t0: DateTime<Utc>,
}

fn record(key: &str, created: DateTime<Utc>) -> GameRecord {
    GameRecord {
        id: key.into(),
        name: "Celeste".into(),
        ids: vec!["steam:504230".into()],
        folders: vec![key.into()],
        lines: vec![Line { id: "main".into(), format: "pc".into(), scope: v2::SCOPE_SHARED.into(), path: v2::line_dir(key, "main"), ..Default::default() }],
        created_at: Some(created),
        ..Default::default()
    }
}

impl World {
    fn new(root: &Path) -> World {
        let store = Logged::default();
        let (b, c) = (Device::new(root, "d-b"), Device::new(root, "d-c"));
        let a = Device::new(root, "d-a");
        // A cloud of layout 2 with the card the upgrade makes of two keys
        // that are one game.
        a.cloud(&store).writable().unwrap();
        let t0 = Utc::now() - Duration::days(2);
        let dupes = library::dedupe_with(&[record(STEAM, t0), record(XBOX, t0 + Duration::minutes(1))], &|_, _| false);
        assert_eq!(dupes.len(), 1);
        for r in std::iter::once(&dupes[0].survivor).chain(&dupes[0].losers) {
            library::save(&store, r, "d-setup", t0).unwrap();
        }
        store.inner.put(&format!("games/{XBOX}/playtime/d-old.json"), OLD_PLAYTIME.as_bytes());
        store.inner.put(&format!("games/{XBOX}/achievements/d-old.json"), OLD_UNLOCKS.as_bytes());
        store.take();
        World { store, a, b, c, t0 }
    }

    /// When the `n`th session of the test began.
    fn at(&self, n: i64) -> DateTime<Utc> {
        self.t0 + Duration::hours(n)
    }
}

fn unlock(key: &str, id: &str, device: &str, at: DateTime<Utc>) {
    let u = Unlock { id: id.into(), unlocked_at: at, hardcore: false };
    achievements::update_db(key, |db| {
        db.mine.insert(u.id.clone(), u.clone());
        db.merge(&u, device, &device.to_uppercase());
    })
    .unwrap();
}

fn unlocked(key: &str) -> Vec<String> {
    achievements::load_db(key).unlocks.keys().cloned().collect()
}

fn secs_of(summary: &playtime::Summary, device: &str) -> u64 {
    summary.devices.iter().find(|d| d.device == device).map(|d| d.secs).unwrap_or(0)
}

#[test]
fn game_data_belongs_to_the_game() {
    let tmp = tempfile::tempdir().unwrap();
    let w = World::new(tmp.path());
    two_folders(&w);
    joined_another_card(&w);
    costs(&w);
    artwork(&w);
}

/// Each version of a merged game writes where it always did, under the
/// names 0.6 uses; the game's totals and unlocks are what both folders hold.
fn two_folders(w: &World) {
    // PC A plays the Steam version.
    w.a.activate();
    let game_a = w.a.add(STEAM);
    let cloud_a = w.a.cloud(&w.store);
    let at_a = Folders::of(&cloud_a, STEAM).unwrap();
    assert_eq!(at_a.all(), [STEAM, XBOX]);
    assert_eq!((at_a.own(), at_a.own_name("d-a").as_str()), (STEAM, "d-a"));
    assert!(playtime::record(&w.a.cfg(), &game_a, w.at(0), w.at(0) + Duration::seconds(600), Via::Direct).unwrap());
    // A session says where it was played.
    let session = playtime::load_local(STEAM).unwrap().sessions[0].clone();
    assert_eq!((session.platform.as_deref(), session.line.as_deref()), (Some("pc"), Some("main")));
    // The hour the device on 0.6 played the Xbox version counts.
    let sum = playtime::sync(cloud_a.files(), &w.a.cfg(), &at_a).unwrap();
    assert_eq!(sum.total_secs, 600 + 3600);

    // PC B plays the Xbox version: its own folder, as on 0.6.
    w.b.activate();
    let game_b = w.b.add(XBOX);
    let cloud_b = w.b.cloud(&w.store);
    let at_b = Folders::of(&cloud_b, XBOX).unwrap();
    assert_eq!(at_b.all(), [STEAM, XBOX]);
    assert_eq!((at_b.own(), at_b.own_name("d-b").as_str()), (XBOX, "d-b"));
    assert!(playtime::record(&w.b.cfg(), &game_b, w.at(1), w.at(1) + Duration::seconds(1200), Via::Steam).unwrap());
    let sum = playtime::sync(cloud_b.files(), &w.b.cfg(), &at_b).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600);
    assert!(sum.devices[0].this_device);

    w.a.activate();
    let sum = playtime::sync(cloud_a.files(), &w.a.cfg(), &at_a).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600);
    assert_eq!((secs_of(&sum, "d-a"), secs_of(&sum, "d-b"), secs_of(&sum, "d-old")), (600, 1200, 3600));
    // Only sessions that say where they were played are split by platform.
    assert_eq!(sum.platforms.iter().map(|p| (p.platform.as_str(), p.secs)).collect::<Vec<_>>(), vec![("pc", 1800)]);

    // Achievements: A has trophy 1 and the definitions, B trophy 2, the
    // device on 0.6 trophy 3.
    unlock(STEAM, "1", "d-a", w.at(0));
    achievements::update_db(STEAM, |db| {
        db.schema = Some(Schema {
            source: SourceKind::Rules,
            source_id: "celeste".into(),
            title: None,
            achievements: ["1", "2", "3", "4"]
                .iter()
                .map(|id| Achievement { id: id.to_string(), name: format!("A{id}"), description: String::new(), icon: None, hidden: false, points: None, grade: None })
                .collect(),
        })
    })
    .unwrap();
    ach_cloud::sync(cloud_a.files(), &at_a, "d-a", "D-A").unwrap();
    w.b.activate();
    unlock(XBOX, "2", "d-b", w.at(1));
    ach_cloud::sync(cloud_b.files(), &at_b, "d-b", "D-B").unwrap();
    assert_eq!(unlocked(XBOX), ["1", "2", "3"]);
    // The definitions come from the first folder that has them.
    assert_eq!(achievements::load_db(XBOX).schema.map(|s| s.achievements.len()), Some(4));
    w.a.activate();
    ach_cloud::pull(cloud_a.files(), &at_a, "d-a").unwrap();
    assert_eq!(unlocked(STEAM), ["1", "2", "3"]);
    assert_eq!(achievements::load_db(STEAM).unlocks["3"].device_name, "OLD");

    // Each wrote into its own key's folder, nothing into the other one.
    assert_eq!(w.store.files(&format!("games/{STEAM}/playtime")), ["d-a.json"]);
    assert_eq!(w.store.files(&format!("games/{XBOX}/playtime")), ["d-b.json", "d-old.json"]);
    assert_eq!(w.store.files(&format!("games/{STEAM}/achievements")), ["d-a.json", "schema.json"]);
    assert_eq!(w.store.files(&format!("games/{XBOX}/achievements")), ["d-b.json", "d-old.json"]);
    // What 0.6 wrote is still as it was.
    assert_eq!(w.store.inner.peek(&format!("games/{XBOX}/playtime/d-old.json")).unwrap(), OLD_PLAYTIME.as_bytes());
}

/// Two versions on one device that joined the card made under another key:
/// the record's first folder, a file each with the key in its name, and
/// every reader counts each once.
fn joined_another_card(w: &World) {
    w.c.activate();
    let gog = w.c.add_as(GOG, Some(STEAM));
    let epic = w.c.add_as(EPIC, Some(STEAM));
    let cloud_c = w.c.cloud(&w.store);
    let at_gog = Folders::of(&cloud_c, GOG).unwrap();
    let at_epic = Folders::of(&cloud_c, EPIC).unwrap();
    assert_eq!((at_gog.own(), at_gog.own_name("d-c").as_str()), (STEAM, "d-c~celeste-gog"));
    assert_eq!((at_epic.own(), at_epic.own_name("d-c").as_str()), (STEAM, "d-c~celeste-epic"));

    assert!(playtime::record(&w.c.cfg(), &gog, w.at(2), w.at(2) + Duration::seconds(300), Via::Direct).unwrap());
    let sum = playtime::sync(cloud_c.files(), &w.c.cfg(), &at_gog).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600 + 300);
    assert!(playtime::record(&w.c.cfg(), &epic, w.at(3), w.at(3) + Duration::seconds(900), Via::Direct).unwrap());
    let sum = playtime::sync(cloud_c.files(), &w.c.cfg(), &at_epic).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600 + 300 + 900);
    assert_eq!(w.store.files(&format!("games/{STEAM}/playtime")), ["d-a.json", "d-c~celeste-epic.json", "d-c~celeste-gog.json"]);
    // The second version didn't write over the first one's file.
    let file: playtime::DeviceLog = serde_json::from_slice(&w.store.inner.peek(&format!("games/{STEAM}/playtime/d-c~celeste-gog.json")).unwrap()).unwrap();
    assert_eq!(file.total_secs(), 300);
    // Both are this device's time, each counted once.
    let sum = playtime::sync(cloud_c.files(), &w.c.cfg(), &at_gog).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600 + 300 + 900);
    assert_eq!(sum.devices.iter().filter(|d| d.device == "d-c").count(), 1);
    assert!(sum.devices[0].this_device && sum.devices[0].secs == 1200);

    // Achievements likewise.
    unlock(GOG, "4", "d-c", w.at(2));
    ach_cloud::sync(cloud_c.files(), &at_gog, "d-c", "D-C").unwrap();
    assert_eq!(unlocked(GOG), ["1", "2", "3", "4"]);
    assert_eq!(w.store.files(&format!("games/{STEAM}/achievements")), ["d-a.json", "d-c~celeste-gog.json", "schema.json"]);

    // A sees both versions of C as C's, once each; nothing moved anywhere.
    w.a.activate();
    let cloud_a = w.a.cloud(&w.store);
    let at_a = Folders::of(&cloud_a, STEAM).unwrap();
    let sum = playtime::sync(cloud_a.files(), &w.a.cfg(), &at_a).unwrap();
    assert_eq!(sum.total_secs, 600 + 1200 + 3600 + 300 + 900);
    assert_eq!((secs_of(&sum, "d-a"), secs_of(&sum, "d-c"), sum.devices.len()), (600, 1200, 4));
    ach_cloud::pull(cloud_a.files(), &at_a, "d-a").unwrap();
    assert_eq!(unlocked(STEAM), ["1", "2", "3", "4"]);
    assert_eq!(w.store.files(&format!("games/{XBOX}/playtime")), ["d-b.json", "d-old.json"]);
    assert!(w.store.inner.paths().iter().all(|p| !p.starts_with(&format!("games/{GOG}")) && !p.starts_with(&format!("games/{EPIC}"))));

    // Where the cloud can't be asked and nothing is remembered, such a
    // version still writes into the card's folder, never under its own key.
    w.c.activate();
    state::update_game(GOG, |g| g.line = None).unwrap();
    let at = Folders::remembered(&gog);
    assert_eq!((at.own(), at.own_name("d-c").as_str()), (STEAM, "d-c~celeste-gog"));
}

fn sync_once(cloud: Cloud, key: &str) -> Outcome {
    let ctx = Ctx::with_cloud(config::load().unwrap(), cloud);
    let game = ctx.game(key).unwrap();
    sync::sync(&ctx, &game, Resolution::Auto, Trigger::AutoExit).unwrap()
}

/// A game with one folder costs what it did on 0.6; each further folder one
/// listing (one read for artwork) more per kind of data, nothing else.
fn costs(w: &World) {
    w.a.activate();
    w.a.add(SOLO);
    assert!(matches!(sync_once(w.a.cloud(&w.store), STEAM), Outcome::Uploaded { .. }));
    assert!(matches!(sync_once(w.a.cloud(&w.store), SOLO), Outcome::Uploaded { .. }));
    assert_eq!(w.store.files(&format!("games/{SOLO}/playtime")), Vec::<String>::new());
    // After playing with nothing new: the line, and the bookkeeping. The
    // second of two such syncs is measured, when nothing is left over from
    // the upload.
    let after = |key: &str| {
        sync_once(w.a.cloud(&w.store), key);
        w.store.take();
        assert!(matches!(sync_once(w.a.cloud(&w.store), key), Outcome::UpToDate));
        w.store.take()
    };
    assert_eq!(
        after(SOLO),
        [format!("list games/{SOLO}/saves/main"), format!("list games/{SOLO}/playtime"), format!("read games/{SOLO}/art/art.json")]
    );
    assert_eq!(
        after(STEAM),
        [
            format!("list games/{STEAM}/saves/main"),
            format!("list games/{STEAM}/playtime"),
            format!("list games/{XBOX}/playtime"),
            format!("read games/{STEAM}/art/art.json"),
            format!("read games/{XBOX}/art/art.json"),
        ]
    );

    // Achievements and screenshots: one listing per folder as well.
    let cloud = w.a.cloud(&w.store);
    let cfg = w.a.cfg();
    let (solo, steam) = (cfg.game(SOLO).unwrap().clone(), cfg.game(STEAM).unwrap().clone());
    let (at_solo, at_steam) = (Folders::of(&cloud, SOLO).unwrap(), Folders::of(&cloud, STEAM).unwrap());
    assert_eq!(at_solo.all(), [SOLO]);
    w.store.take();
    ach_cloud::pull(cloud.files(), &at_solo, "d-a").unwrap();
    assert_eq!(w.store.take(), [format!("list games/{SOLO}/achievements")]);
    ach_cloud::pull(cloud.files(), &at_steam, "d-a").unwrap();
    assert_eq!(w.store.take(), [format!("list games/{STEAM}/achievements"), format!("list games/{XBOX}/achievements")]);
    screenshots::gallery(Some(cloud.files()), &cfg, &at_solo, &solo).unwrap();
    assert_eq!(w.store.take(), [format!("list games/{SOLO}/screenshots")]);
    screenshots::gallery(Some(cloud.files()), &cfg, &at_steam, &steam).unwrap();
    assert_eq!(w.store.take(), [format!("list games/{STEAM}/screenshots"), format!("list games/{XBOX}/screenshots")]);
}

/// One set of artwork per game: each kind from the first folder that has
/// it; a device uploads into the folder it writes to.
fn artwork(w: &World) {
    let image = |name: &str, bytes: &[u8]| {
        let file = w.a.home.parent().unwrap().join(name);
        fs::write(&file, bytes).unwrap();
        file
    };
    let index = |items: &[(&str, &str, &[u8])]| {
        let files: serde_json::Map<String, serde_json::Value> = items
            .iter()
            .map(|(kind, file, bytes)| (kind.to_string(), serde_json::json!({ "file": file, "hash": blake3::hash(bytes).to_hex().to_string() })))
            .collect();
        serde_json::to_vec(&serde_json::json!({ "files": files })).unwrap()
    };
    // The Xbox version's folder has a cover; the Steam version's has none.
    let cover: &[u8] = b"PNG cover";
    w.store.inner.put(&format!("games/{XBOX}/art/cover.png"), cover);
    w.store.inner.put(&format!("games/{XBOX}/art/art.json"), &index(&[("cover", "cover.png", cover)]));

    // A, the Steam version, finds it in the second folder.
    w.a.activate();
    let cloud_a = w.a.cloud(&w.store);
    let at_a = Folders::of(&cloud_a, STEAM).unwrap();
    let game_a = w.a.cfg().game(STEAM).unwrap().clone();
    w.store.take();
    assert!(artsync::pull(cloud_a.files(), &w.a.cfg(), &at_a, Some(&game_a)).unwrap());
    assert_eq!(
        w.store.take(),
        [format!("read games/{STEAM}/art/art.json"), format!("read games/{XBOX}/art/art.json"), format!("download games/{XBOX}/art/cover.png")]
    );
    assert_eq!(fs::read(localart::file(STEAM, "cover").unwrap()).unwrap(), cover);

    // B, the Xbox version, shows a hero of its own: it goes into B's folder.
    w.b.activate();
    let hero: &[u8] = b"PNG hero";
    localart::set(XBOX, "hero", &image("hero.png", hero)).unwrap();
    let cloud_b = w.b.cloud(&w.store);
    let game_b = w.b.cfg().game(XBOX).unwrap().clone();
    assert!(artsync::push(cloud_b.files(), &w.b.cfg(), &Folders::of(&cloud_b, XBOX).unwrap(), &game_b).unwrap());
    assert_eq!(w.store.files(&format!("games/{XBOX}/art")), ["art.json", "cover.png", "hero.png"]);
    assert_eq!(w.store.files(&format!("games/{STEAM}/art")), Vec::<String>::new());

    // C's GOG version, which joined the card: its icon goes into the
    // record's first folder.
    w.c.activate();
    let icon: &[u8] = b"PNG icon";
    localart::set(GOG, "icon", &image("icon.png", icon)).unwrap();
    let cloud_c = w.c.cloud(&w.store);
    let gog = w.c.cfg().game(GOG).unwrap().clone();
    assert!(artsync::push(cloud_c.files(), &w.c.cfg(), &Folders::of(&cloud_c, GOG).unwrap(), &gog).unwrap());
    assert_eq!(w.store.files(&format!("games/{STEAM}/art")), ["art.json", "icon.png"]);

    // A gets the hero and the icon, each from the folder that has it, and
    // keeps the cover it has.
    w.a.activate();
    w.store.take();
    assert!(artsync::pull(cloud_a.files(), &w.a.cfg(), &at_a, Some(&game_a)).unwrap());
    assert_eq!(
        w.store.take(),
        [
            format!("read games/{STEAM}/art/art.json"),
            format!("read games/{XBOX}/art/art.json"),
            format!("download games/{XBOX}/art/hero.png"),
            format!("download games/{STEAM}/art/icon.png"),
        ]
    );
    assert_eq!(fs::read(localart::file(STEAM, "hero").unwrap()).unwrap(), hero);
    assert_eq!(fs::read(localart::file(STEAM, "icon").unwrap()).unwrap(), icon);
    assert_eq!(fs::read(localart::file(STEAM, "cover").unwrap()).unwrap(), cover);
}
