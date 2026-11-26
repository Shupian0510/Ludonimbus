//! Versions of one game that keep the same saves in different folders
//! share one save line, asked once (docs/plan-v2.md, section 6). One test,
//! because the data directory is chosen through an environment variable.

use anyhow::Result;
use ludonimbus_core::bind::Bind;
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::v2::GameRecord;
use ludonimbus_core::cloud::Cloud;
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::join::{self, Fit};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution, Status};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
        Some("sync-bind-test".to_string())
    }
}

/// A device with one game: its key there, the link that says which game it
/// is, and the folders (below the game's folder) its saves are in.
struct Device {
    home: PathBuf,
    game: PathBuf,
    key: String,
    store: Logged,
}

impl Device {
    fn new(root: &Path, store: &Logged, name: &str, key: &str, link: &str, dirs: &[&str]) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), game: root.join(format!("game-{name}")), key: key.to_string(), store: store.clone() };
        fs::create_dir_all(&dev.home).unwrap();
        fs::create_dir_all(&dev.game).unwrap();
        fs::write(dev.game.join("game.exe"), b"exe").unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.cloud = CloudConfig::None;
        cfg.games.push(GameEntry {
            key: key.into(),
            name: format!("Game {key}"),
            exe: Some(dev.game.join("game.exe")),
            saves: dirs.iter().map(|d| format!("{{install}}/{d}/**")).collect(),
            game: Some(link.to_string()),
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

    fn entry(&self) -> GameEntry {
        self.activate();
        config::load().unwrap().game(&self.key).unwrap().clone()
    }

    fn write(&self, file: &str, content: &str) {
        let path = self.game.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// The content of a file below the game's folder; `None` when there is
    /// no such file.
    fn read(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.game.join(file)).ok()
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

    fn status(&self) -> Status {
        let ctx = self.ctx();
        sync::check(&ctx, &ctx.game(&self.key).unwrap()).status
    }

    fn suggestion(&self) -> Option<join::Suggestion> {
        let ctx = self.ctx();
        join::suggestion(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    fn accept(&self, s: &join::Suggestion) -> Outcome {
        let ctx = self.ctx();
        join::accept(&ctx, &ctx.game(&self.key).unwrap(), s).unwrap()
    }

    fn leave(&self) -> Outcome {
        let ctx = self.ctx();
        join::leave(&ctx, &ctx.game(&self.key).unwrap()).unwrap()
    }

    /// The paths of the files of version `id` on the line the game follows.
    fn files_of(&self, id: &str) -> Vec<String> {
        let mut files: Vec<String> = self.ctx().cloud().unwrap().get_meta(&self.key, id).unwrap().files.into_iter().map(|f| f.path).collect();
        files.sort();
        files
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

fn bind(canon: &str, local: &str) -> Bind {
    Bind::files(canon, local)
}

#[test]
fn versions_that_keep_their_saves_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    asked_once(root);
    a_file_with_no_folder(root);
}

/// One version uploads first; a version of another store joins its line
/// after one question, and the next device with that version without one.
fn asked_once(root: &Path) {
    let store = Logged::default();

    // A: the version the line begins with. Its saves are in `save`.
    let a = Device::new(root, &store, "B-A", "tale", "tale", &["save"]);
    a.write("save/slot1.sav", "a1");
    a.write("save/slot2.sav", "a2");
    let first = uploaded(a.sync());
    assert_eq!(a.files_of(&first), strings(&["install/save/slot1.sav", "install/save/slot2.sav"]));

    // B: another store's version of the same game. Its saves are in
    // `profile`, its settings in `options`. It hasn't been played here yet:
    // there is nothing to compare with.
    let b = Device::new(root, &store, "B-B", "tale-gog", "tale", &["profile", "options"]);
    let s = b.suggestion().expect("the card has a line to share");
    assert_eq!((s.record.as_str(), s.fit, s.confirmed), ("tale", Fit::NeedsSaves, false));
    assert!(s.binds.is_empty());
    assert_eq!(s.device, "B-A");

    // Played once: the files say which folder is which.
    b.write("profile/slot1.sav", "b1");
    b.write("profile/slot3.sav", "b3");
    b.write("options/video.ini", "fullscreen");
    store.take();
    let s = b.suggestion().unwrap();
    assert_eq!(s.fit, Fit::Ready);
    assert_eq!(s.binds, vec![bind("install/save", "install/profile")]);
    assert_eq!((s.matched, s.only_in_line, s.only_here), (1, 1, 1));
    assert!(s.uncovered.is_empty());
    assert_eq!(s.outside, strings(&["install/options/video.ini"]));
    // Nothing was written to find that out, and nothing is settled.
    assert!(store.take().iter().all(|r| r.starts_with("list ") || r.starts_with("read ")));
    assert!(b.entry().bind.is_empty() && b.entry().record.is_none());

    // Until the user answers, the version keeps a line of its own on the
    // same card, with its files as they are.
    let own = uploaded(b.sync());
    assert_eq!(b.files_of(&own), strings(&["install/options/video.ini", "install/profile/slot1.sav", "install/profile/slot3.sav"]));
    assert_eq!(store.record("tale-gog").merged_into.as_deref(), Some("tale"));
    assert_eq!(store.record("tale").lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["main", "main-tale-gog"]);

    // Yes: the saves differ, so the one question is which to continue.
    let s = b.suggestion().unwrap();
    assert_eq!(s.fit, Fit::Ready);
    assert!(matches!(b.accept(&s), Outcome::Conflict { .. }));
    let entry = b.entry();
    assert_eq!(entry.record.as_deref(), Some("tale"));
    assert_eq!(entry.bind, vec![bind("install/save", "install/profile")]);
    assert_eq!(entry.line.as_deref(), Some("main"));
    assert_eq!(b.status(), Status::Conflict);
    // Nothing on this device has changed yet.
    assert_eq!(b.read("profile/slot1.sav").as_deref(), Some("b1"));
    // The line says how such versions take part in it.
    let line = store.record("tale").lines.into_iter().find(|l| l.id == "main").unwrap();
    assert_eq!(line.binds.len(), 1);
    assert_eq!(line.binds[0].map, vec![bind("install/save", "install/profile")]);
    // It is asked no more.
    assert!(b.suggestion().is_none());

    // The cloud's: the line's files arrive in this version's folder. What
    // the line doesn't have goes from the bound folder (it is kept in the
    // cloud beside the history first); the settings outside it stay.
    assert_eq!(pulled(b.sync_with(Resolution::UseCloud)), first);
    assert_eq!(b.read("profile/slot1.sav").as_deref(), Some("a1"));
    assert_eq!(b.read("profile/slot2.sav").as_deref(), Some("a2"));
    assert_eq!(b.read("profile/slot3.sav"), None);
    assert_eq!(b.read("options/video.ini").as_deref(), Some("fullscreen"));
    assert!(!b.game.join("save").exists(), "nothing is written under the other version's paths");
    assert_eq!(b.status(), Status::Synced);
    assert!(matches!(b.sync(), Outcome::UpToDate));

    // B plays on: its version goes to the line under the line's paths, and
    // without the settings.
    b.write("profile/slot1.sav", "b-next");
    b.write("profile/slot4.sav", "b4");
    let next = uploaded(b.sync());
    assert_eq!(b.files_of(&next), strings(&["install/save/slot1.sav", "install/save/slot2.sav", "install/save/slot4.sav"]));

    // A gets it as its own files.
    assert_eq!(pulled(a.sync()), next);
    assert_eq!(a.read("save/slot1.sav").as_deref(), Some("b-next"));
    assert_eq!(a.read("save/slot4.sav").as_deref(), Some("b4"));
    assert!(!a.game.join("profile").exists());
    // And back.
    a.write("save/slot1.sav", "a-next");
    let back = uploaded(a.sync());
    assert_eq!(pulled(b.sync()), back);
    assert_eq!(b.read("profile/slot1.sav").as_deref(), Some("a-next"));
    assert_eq!(b.read("options/video.ini").as_deref(), Some("fullscreen"));

    // C: the same store's version as B on another device, nothing played
    // yet. The folders were confirmed once: it shares the line at its first
    // sync, without a question and without a record or a folder of its own.
    let c = Device::new(root, &store, "B-C", "tale-gog-deck", "tale", &["profile", "options"]);
    store.take();
    // As the launch wrapper does it: the game is read once, before the
    // first sync, and used for the one after playing too.
    let wrapper = c.ctx();
    let as_read = wrapper.game("tale-gog-deck").unwrap();
    assert!(as_read.bind.is_empty());
    assert_eq!(pulled(sync::sync(&wrapper, &as_read, Resolution::Auto, Trigger::PreLaunch).unwrap()), back);
    let requests = store.take();
    assert_eq!(requests.iter().filter(|r| *r == "list library").count(), 1, "{requests:?}");
    assert!(requests.iter().all(|r| !r.starts_with("write library/")), "{requests:?}");
    assert_eq!(c.read("profile/slot1.sav").as_deref(), Some("a-next"));
    assert_eq!(c.read("profile/slot4.sav").as_deref(), Some("b4"));
    assert_eq!(c.entry().record.as_deref(), Some("tale"));
    assert_eq!(c.entry().bind, vec![bind("install/save", "install/profile")]);
    assert!(!store.has_record("tale-gog-deck"));
    assert!(store.inner.list("games/tale-gog-deck").unwrap().is_empty());
    c.write("profile/slot1.sav", "c1");
    let from_c = uploaded(sync::sync(&wrapper, &as_read, Resolution::Auto, Trigger::AutoExit).unwrap());
    assert_eq!(c.files_of(&from_c), strings(&["install/save/slot1.sav", "install/save/slot2.sav", "install/save/slot4.sav"]));
    assert_eq!(pulled(a.sync()), from_c);
    assert_eq!(a.read("save/slot1.sav").as_deref(), Some("c1"));

    // D: that version again, with saves of its own already. It shares the
    // line too; which saves to continue is asked, as on any device that
    // comes to a line with other saves.
    let d = Device::new(root, &store, "B-D", "tale-gog-laptop", "tale", &["profile", "options"]);
    d.write("profile/slot1.sav", "d1");
    assert!(matches!(d.sync(), Outcome::Conflict { .. }));
    assert_eq!(d.entry().bind, vec![bind("install/save", "install/profile")]);
    assert_eq!(d.read("profile/slot1.sav").as_deref(), Some("d1"));
    let from_d = uploaded(d.sync_with(Resolution::UseLocal));
    assert_eq!(d.files_of(&from_d), strings(&["install/save/slot1.sav"]));
    assert_eq!(pulled(a.sync()), from_d);
    assert_eq!(a.read("save/slot1.sav").as_deref(), Some("d1"));
    assert_eq!(a.read("save/slot2.sav"), None);

    // E: a version whose files don't correspond to the line's.
    let e = Device::new(root, &store, "B-E", "tale-demo", "tale", &["demo"]);
    e.write("demo/progress.bin", "e");
    let s = e.suggestion().unwrap();
    assert_eq!(s.fit, Fit::NoMatch);
    let ctx = e.ctx();
    assert!(join::accept(&ctx, &ctx.game("tale-demo").unwrap(), &s).is_err());
    assert!(e.entry().bind.is_empty());

    // F: a version that could share, and the user says no: not suggested
    // again, and it keeps its own line.
    let f = Device::new(root, &store, "B-F", "tale-epic", "tale", &["data"]);
    f.write("data/slot1.sav", "f1");
    let s = f.suggestion().unwrap();
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![bind("install/save", "install/data")]));
    f.activate();
    join::decline("tale-epic", &s.def).unwrap();
    // The card's other line (the one B's version had before it shared) has
    // files this version has no folder for: nothing to accept.
    let other = f.suggestion();
    assert!(other.is_none_or(|o| o.def != s.def && o.fit == Fit::NoMatch));
    let own = uploaded(f.sync());
    assert_eq!(f.files_of(&own), strings(&["install/data/slot1.sav"]));
    assert!(f.entry().bind.is_empty());

    // D's save location is about to be changed by hand: the folders matched
    // for it no longer say where its files are. It keeps its saves apart
    // again, nothing on this device changes, and nothing was declined: with
    // the new location the line can be offered again.
    let ctx = d.ctx();
    assert!(join::forget(&ctx, &ctx.game("tale-gog-laptop").unwrap()).unwrap());
    let entry = d.entry();
    assert!(entry.bind.is_empty() && entry.record.is_none() && entry.declined.is_empty(), "{entry:#?}");
    assert_eq!(d.read("profile/slot1.sav").as_deref(), Some("d1"));
    assert!(!join::forget(&d.ctx(), &d.entry()).unwrap());
    let s = d.suggestion().unwrap();
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![bind("install/save", "install/profile")]));
    // What it syncs from here on is its own files under its own paths.
    let outcome = d.sync_with(Resolution::UseLocal);
    let own = uploaded(outcome);
    assert_eq!(d.files_of(&own), strings(&["install/profile/slot1.sav"]));
}

/// The line gets a file the bound version has no folder for: nothing is
/// written, and the version can go back to a line of its own.
fn a_file_with_no_folder(root: &Path) {
    let store = Logged::default();

    // A keeps saves in two folders; only one has files so far.
    let a = Device::new(root, &store, "N-A", "saga", "saga", &["save", "extra"]);
    a.write("save/slot1.sav", "a1");
    uploaded(a.sync());

    let b = Device::new(root, &store, "N-B", "saga-gog", "saga", &["profile"]);
    b.write("profile/slot1.sav", "a1");
    let s = b.suggestion().unwrap();
    assert_eq!((s.fit, s.binds.clone()), (Fit::Ready, vec![bind("install/save", "install/profile")]));
    // The same saves on both sides: sharing needs no further question.
    assert!(matches!(b.accept(&s), Outcome::UpToDate));
    assert!(!store.has_record("saga-gog"));

    // A's version now has a file in the other folder.
    a.write("extra/keys.cfg", "k");
    a.write("save/slot1.sav", "a2");
    uploaded(a.sync());
    let err = b.try_sync(Resolution::Auto, Trigger::SyncNow).unwrap_err();
    assert!(join::is_no_place(&err), "{err:#}");
    assert_eq!(b.read("profile/slot1.sav").as_deref(), Some("a1"));
    assert!(!b.game.join("extra").exists() && !b.game.join("save").exists());
    // Still so the next time.
    assert!(join::is_no_place(&b.try_sync(Resolution::Auto, Trigger::SyncNow).unwrap_err()));

    // Apart again: the version continues on a line of its own, with the
    // saves it has, and the line it left is not suggested again.
    let own = uploaded(b.leave());
    let entry = b.entry();
    assert!(entry.bind.is_empty() && entry.record.is_none());
    assert_eq!(entry.declined.len(), 1);
    assert_eq!(b.files_of(&own), strings(&["install/profile/slot1.sav"]));
    assert_eq!(store.record("saga-gog").merged_into.as_deref(), Some("saga"));
    assert!(b.suggestion().is_none());
    assert!(matches!(b.sync(), Outcome::UpToDate));
    // A's line is as it was.
    assert!(matches!(a.sync(), Outcome::UpToDate));
    assert_eq!(a.read("save/slot1.sav").as_deref(), Some("a2"));
}
