//! One game that devices added under different keys: one card, and one
//! history where the versions keep the same files (docs/plan-v2.md,
//! section 6). One test, because the data directory is chosen through an
//! environment variable.

use anyhow::Result;
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::GameRecord;
use ludonimbus_core::cloud::{own_line_id, Cloud};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::state;
use ludonimbus_core::sync::{self, Ctx, LineChoice, Outcome, Resolution, Status};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

    /// The game records, read past the log.
    fn records(&self) -> Vec<GameRecord> {
        library::load(self.inner.as_ref(), &mut LibraryCache::default()).unwrap().records
    }

    fn record(&self, id: &str) -> GameRecord {
        library::get(&self.records(), id).cloned().unwrap_or_else(|| panic!("no record {id}"))
    }

    fn has(&self, path: &str) -> bool {
        self.inner.read(path).unwrap().is_some()
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
        Some("sync-join-test".to_string())
    }
}

/// A device with one game: its key and name there, the link that says
/// which game it is (`None`: only its name does), and the folder its saves
/// are in.
struct Device {
    home: PathBuf,
    game: PathBuf,
    key: String,
    dir: String,
    store: Logged,
}

impl Device {
    fn new(root: &Path, store: &Logged, name: &str, key: &str, title: &str, link: Option<&str>, dir: &str) -> Device {
        let dev = Device {
            home: root.join(format!("home-{name}")),
            game: root.join(format!("game-{name}")),
            key: key.to_string(),
            dir: dir.to_string(),
            store: store.clone(),
        };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        cfg.games.push(GameEntry {
            key: key.into(),
            name: title.into(),
            exe: Some(dev.game.join("game.exe")),
            saves: vec![format!("{{install}}/{dir}/**")],
            game: link.map(str::to_string),
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

    /// This device, looking at the cloud anew.
    fn ctx(&self) -> Ctx {
        self.activate();
        let cfg = config::load().unwrap();
        let cloud = Cloud::new(Box::new(self.store.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    }

    fn id(&self) -> String {
        self.activate();
        config::load().unwrap().device.id
    }

    /// The game's entry on this device.
    fn entry(&self) -> GameEntry {
        self.activate();
        config::load().unwrap().game(&self.key).unwrap().clone()
    }

    fn write(&self, content: &str) {
        let file = self.game.join(&self.dir).join("slot1.sav");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn read(&self) -> String {
        fs::read_to_string(self.game.join(&self.dir).join("slot1.sav")).unwrap_or_default()
    }

    fn sync_with(&self, ctx: &Ctx, resolution: Resolution, trigger: Trigger) -> Outcome {
        self.activate();
        sync::sync(ctx, &ctx.game(&self.key).unwrap(), resolution, trigger).unwrap()
    }

    fn sync(&self, trigger: Trigger) -> Outcome {
        self.sync_with(&self.ctx(), Resolution::Auto, trigger)
    }

    fn status_with(&self, ctx: &Ctx) -> Status {
        self.activate();
        sync::check(ctx, &ctx.game(&self.key).unwrap()).status
    }

    fn status(&self) -> Status {
        self.status_with(&self.ctx())
    }

    /// The ids of the versions on the line the game follows, oldest first.
    fn versions(&self) -> Vec<String> {
        self.ctx().cloud().unwrap().list_snapshots(&self.key).unwrap().into_iter().map(|s| s.id).collect()
    }

    /// The newest versions of that line.
    fn heads(&self) -> Vec<String> {
        self.ctx().cloud().unwrap().list_snapshots(&self.key).unwrap().into_iter().filter(|s| s.head).map(|s| s.id).collect()
    }

    /// Where the device remembers the game's line: record, line, folders.
    fn line(&self) -> (String, String, Vec<String>) {
        self.activate();
        let line = state::game(&self.key).line.unwrap();
        (line.game, line.line, line.dirs)
    }

    /// As when the device last looked at the game's record two days ago.
    fn two_days_later(&self) {
        self.activate();
        state::update_game(&self.key, |g| {
            if let Some(line) = g.line.as_mut() {
                line.checked_at = Some(chrono::Utc::now() - chrono::Duration::days(2));
            }
        })
        .unwrap();
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

fn line_ids(record: &GameRecord) -> Vec<&str> {
    record.lines.iter().map(|l| l.id.as_str()).collect()
}

#[test]
fn one_game_under_different_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    one_card(root);
    made_at_once(root);
}

/// Devices that add a game the cloud has, each under a key of its own.
fn one_card(root: &Path) {
    let store = Logged::default();
    const MAIN: &str = "games/tale/saves/main";

    let a = Device::new(root, &store, "J-A", "tale", "Tale", Some("tale"), "save");
    a.write("v1");
    let v1 = uploaded(a.sync(Trigger::AutoExit));
    let record = store.record("tale");
    assert!(record.ids.contains(&"link:tale".to_string()), "{:?}", record.ids);
    assert!(record.lines[0].def.is_some());
    store.take();

    // ---- The same game, the same files, another name and key: it finds
    // the card by what it is and continues its line. What that costs is
    // what any device new to a game pays.
    let b = Device::new(root, &store, "J-B", "game-0b7", "传说之旅", Some("tale"), "save");
    assert_eq!(b.status(), Status::CanRestore);
    assert_eq!(
        store.take(),
        strings(&["read format.json", "list library", "read library/tale.json", &format!("list {MAIN}"), &format!("read {MAIN}/index.json")])
    );
    assert_eq!(b.entry().record.as_deref(), Some("tale"));
    assert_eq!(b.line(), ("tale".to_string(), "main".to_string(), strings(&[MAIN])));
    assert_eq!(pulled(b.sync(Trigger::PreLaunch)), v1);
    assert_eq!(b.read(), "v1");
    store.take();
    // From here on a check is one listing, as for any game.
    assert!(matches!(b.sync(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(store.take(), strings(&[&format!("list {MAIN}")]));

    // What it plays goes to the card's line and packs. Its own key gets no
    // record, no definition and no saves.
    b.write("v2");
    let v2 = uploaded(b.sync(Trigger::AutoExit));
    let log = store.take();
    assert!(log.contains(&format!("upload games/tale/packs/{v2}.zip")), "{log:#?}");
    assert!(log.contains(&format!("write {MAIN}/{v2}.json")), "{log:#?}");
    assert!(!log.iter().any(|l| l.contains("library/")), "{log:#?}");
    assert!(!store.has("library/game-0b7.json") && !store.has("games/game-0b7/game.json"));
    assert!(store.inner.list("games/game-0b7/saves").unwrap().is_empty());
    assert_eq!(pulled(a.sync(Trigger::PreLaunch)), v2);
    assert_eq!(a.read(), "v2");
    assert_eq!(a.versions(), b.versions());
    assert_eq!(store.records().len(), 1);

    // The card learns the name this version has, the next time the device
    // looks at the record: a third version may know only that.
    assert!(!store.record("tale").ids.contains(&"name:传说之旅".to_string()));
    b.two_days_later();
    b.write("v3");
    let v3 = uploaded(b.sync(Trigger::AutoExit));
    assert_eq!(store.record("tale").ids, strings(&["name:tale", "link:tale", "name:传说之旅"]));
    assert_eq!(store.records().len(), 1);

    // A line of its own is a line of the card.
    b.write("v4");
    let v4 = uploaded(sync::use_line(&b.ctx(), &b.entry(), LineChoice::Independent { name: None }).unwrap());
    let own = own_line_id(&b.id(), "game-0b7");
    let card = store.record("tale");
    assert_eq!(line_ids(&card), ["main", own.as_str()]);
    assert_eq!(card.lines[1].path, format!("games/tale/saves/{own}"));
    assert_eq!((card.lines[1].format.as_str(), &card.lines[1].def), (card.lines[0].format.as_str(), &card.lines[0].def));
    assert_eq!(b.versions(), vec![v4.clone()]);
    let lines = sync::lines(&a.ctx(), &a.entry()).unwrap();
    assert_eq!(lines.iter().map(|l| (l.name.as_str(), l.shared, l.current)).collect::<Vec<_>>(), [("", true, true), ("J-B", false, false)]);
    // And back, with the saves the shared line has: the card's line again.
    b.write("v3");
    assert!(matches!(sync::use_line(&b.ctx(), &b.entry(), LineChoice::Shared).unwrap(), Outcome::UpToDate));
    assert_eq!(b.versions(), vec![v1.clone(), v2.clone(), v3.clone()]);
    assert_eq!(b.entry().record.as_deref(), Some("tale"));

    // ---- The same game with other files (another store's version keeps
    // its saves elsewhere): the same card, a line of its own. Nothing of
    // one version is ever written into the other's folders.
    let c = Device::new(root, &store, "J-C", "tale-gog", "Tale", Some("tale"), "profile");
    assert_eq!(c.status(), Status::Empty);
    c.write("c1");
    assert_eq!(c.status(), Status::NotUploaded);
    assert_eq!(c.entry().record, None);
    let c1 = uploaded(c.sync(Trigger::AutoExit));
    let card = store.record("tale");
    assert_eq!(line_ids(&card), ["main", own.as_str(), "main-tale-gog"]);
    assert_eq!(card.folders, strings(&["tale", "tale-gog"]));
    assert_ne!(card.lines[2].def, card.lines[0].def);
    assert_eq!(store.record("tale-gog").merged_into.as_deref(), Some("tale"));
    assert_eq!(library::live(&store.records()).count(), 1);
    assert_eq!(c.line(), ("tale".to_string(), "main-tale-gog".to_string(), strings(&["games/tale-gog/saves/main"])));
    assert_eq!(c.versions(), vec![c1.clone()]);
    assert_eq!(c.status(), Status::Synced);
    assert_eq!(c.read(), "c1");
    assert!(!c.game.join("save").exists());
    // The first version's devices go on as before, and are not offered a
    // line whose files are not theirs.
    assert_eq!(a.versions(), vec![v1.clone(), v2.clone(), v3.clone()]);
    assert_eq!(pulled(a.sync(Trigger::PreLaunch)), v3);
    assert!(!a.game.join("profile").exists());
    let lines = sync::lines(&a.ctx(), &a.entry()).unwrap();
    assert_eq!(lines.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["", "J-B"]);
    let lines = sync::lines(&c.ctx(), &c.entry()).unwrap();
    assert_eq!(lines.iter().map(|l| (l.shared, l.current)).collect::<Vec<_>>(), [(true, true)]);

    // A fourth device has that other version too, under yet another key:
    // it continues that line.
    let d = Device::new(root, &store, "J-D", "tale-gog-deck", "Tale (GOG)", Some("tale"), "profile");
    assert_eq!(pulled(d.sync(Trigger::PreLaunch)), c1);
    assert_eq!(d.entry().record.as_deref(), Some("tale-gog"));
    assert_eq!(d.line(), c.line());
    d.write("d1");
    let d1 = uploaded(d.sync(Trigger::AutoExit));
    assert_eq!(pulled(c.sync(Trigger::PreLaunch)), d1);
    assert_eq!(c.read(), "d1");
    assert_eq!(a.versions(), vec![v1.clone(), v2.clone(), v3.clone()]);
    assert_eq!(library::live(&store.records()).count(), 1);

    // ---- Only the name is the same: another game as far as anyone can
    // tell. A card of its own.
    let e = Device::new(root, &store, "J-E", "tale-2", "Tale", None, "save");
    assert_eq!(e.status(), Status::Empty);
    e.write("e1");
    let e1 = uploaded(e.sync(Trigger::AutoExit));
    assert_eq!(e.entry().record, None);
    assert_eq!(e.versions(), vec![e1]);
    assert_eq!(e.line().0, "tale-2");
    assert_eq!(store.record("tale-2").merged_into, None);
    // So is a version the user keeps apart from every other.
    let f = Device::new(root, &store, "J-F", "tale-mine", "Tale", Some("solo:tale-mine"), "save");
    f.write("f1");
    uploaded(f.sync(Trigger::AutoExit));
    assert_eq!(f.line().0, "tale-mine");
    assert_eq!(library::live(&store.records()).map(|r| r.id.as_str()).collect::<Vec<_>>(), ["tale", "tale-2", "tale-mine"]);
    assert_eq!(a.versions(), vec![v1, v2, v3]);
}

/// Two devices add the same game at the same moment, neither seeing the
/// other's record: two records, which become one card the next time a
/// device looks at the library to write. The versions keep the same files,
/// so their histories become one; each has a version, and the user is asked
/// which to continue, as after any two devices played at once.
fn made_at_once(root: &Path) {
    let store = Logged::default();
    // The cloud is in use already.
    let first = Device::new(root, &store, "M-0", "other", "Other", None, "save");
    first.write("o1");
    uploaded(first.sync(Trigger::AutoExit));

    let f = Device::new(root, &store, "M-F", "duo", "Duo", Some("duo"), "save");
    let g = Device::new(root, &store, "M-G", "duo-deck", "Duo", Some("duo"), "save");
    // G looks at the library, F uploads, G uploads by what it saw.
    g.write("g1");
    let early = g.ctx();
    assert_eq!(g.status_with(&early), Status::NotUploaded);
    f.write("f1");
    let f1 = uploaded(f.sync(Trigger::AutoExit));
    let g1 = uploaded(g.sync_with(&early, Resolution::Auto, Trigger::AutoExit));
    assert_eq!(library::live(&store.records()).map(|r| r.id.as_str()).collect::<Vec<_>>(), ["duo", "duo-deck", "other"]);
    assert_eq!(g.entry().record, None);

    // Until a device looks again, each goes on in its own folder.
    assert!(matches!(f.sync(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(f.versions(), vec![f1.clone()]);

    // F looks at the record again when it next uploads.
    f.two_days_later();
    f.write("f2");
    store.take();
    let f2 = uploaded(f.sync(Trigger::AutoExit));
    let log = store.take();
    assert!(log.contains(&"write library/duo.json".to_string()) && log.contains(&"write library/duo-deck.json".to_string()), "{log:#?}");
    let card = store.record("duo");
    assert_eq!(line_ids(&card), ["main"]);
    assert_eq!(card.lines[0].also, strings(&["games/duo-deck/saves/main"]));
    assert_eq!(card.folders, strings(&["duo", "duo-deck"]));
    assert_eq!(store.record("duo-deck").merged_into.as_deref(), Some("duo"));
    assert_eq!(library::live(&store.records()).count(), 2);
    // One history with two newest versions.
    assert_eq!(f.versions(), vec![f1.clone(), g1.clone(), f2.clone()]);
    assert_eq!(f.heads(), vec![g1.clone(), f2.clone()]);
    match f.sync(Trigger::PreLaunch) {
        Outcome::Conflict { cloud, .. } => assert_eq!(cloud.id, g1),
        other => panic!("expected to be asked, got {other:?}"),
    }
    assert_eq!(f.read(), "f2");

    // G finds out the same way; what it remembered of its line was still
    // good until then.
    assert!(matches!(g.sync(Trigger::PreLaunch), Outcome::UpToDate));
    g.two_days_later();
    assert!(matches!(g.sync(Trigger::AutoExit), Outcome::UpToDate));
    assert_eq!(g.line(), ("duo".to_string(), "main".to_string(), strings(&["games/duo/saves/main", "games/duo-deck/saves/main"])));
    match g.sync(Trigger::PreLaunch) {
        Outcome::Conflict { cloud, .. } => assert_eq!(cloud.id, f2),
        other => panic!("expected to be asked, got {other:?}"),
    }
    // It takes the other device's: one newest version again, on the card's
    // own folder, and nothing was lost on the way.
    assert_eq!(pulled(g.sync_with(&g.ctx(), Resolution::UseCloud, Trigger::Manual)), f2);
    assert_eq!(g.read(), "f2");
    assert_eq!(g.heads().len(), 1);
    assert!(g.versions().contains(&g1) && g.versions().contains(&f1));
    assert_eq!(g.status(), Status::Synced);
    // F has those very saves: nothing left to ask there either.
    assert!(matches!(f.sync(Trigger::PreLaunch), Outcome::UpToDate));
    assert_eq!(f.status(), Status::Synced);
    assert_eq!(f.heads(), g.heads());
}
