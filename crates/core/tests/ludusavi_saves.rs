//! Games whose saves Ludusavi finds get save patterns from the rules of
//! their title in Ludusavi's database, kept with the game: filled in for a
//! game that has none yet, once, and again when the database changes, with
//! the patterns the user added kept. A restore of such a game writes only
//! files under them (or files a scan finds now), as for any other game.
//!
//! Everything is in a temporary folder: the data directory, Ludusavi's
//! database (a few lines standing in for its manifest) and the game's
//! roots. Ludusavi itself is a file that can't run, so a scan of these games
//! fails and a restore goes by the patterns alone.

use ludonimbus_core::config::{self, GameEntry};
use ludonimbus_core::games;
use ludonimbus_core::snapshot::{self, FileEntry};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The tests change the process's environment: one at a time.
static ENV: Mutex<()> = Mutex::new(());

const MANIFEST: &str = r#"Fake Game:
  files:
    "<base>/save":
      tags:
        - save
    "<home>/Saved Games/Fake Game":
      when:
        - os: windows
        - os: linux
    "<home>/Library/Fake Game":
      when:
        - os: mac
  installDir:
    Fake Game: {}
Other Game:
  files:
    "<home>/Other":
      tags:
        - save
"#;

fn cache(etag: &str) -> String {
    format!("---\nversion:\n  - 0\n  - 31\n  - 0\nmanifests:\n  \"https://example.invalid/manifest.yaml\":\n    etag: \"{etag}\"\n")
}

struct Setup {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    install: PathBuf,
    /// Where every root a version names is: nothing lands elsewhere.
    elsewhere: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Setup {
    fn new() -> Setup {
        let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        std::env::set_var("LUDONIMBUS_HOME", base.join("data"));
        // Never the real Ludusavi: it would download its database over ours.
        let fake = base.join("ludusavi.exe");
        fs::write(&fake, b"not a program").unwrap();
        std::env::set_var("LUDONIMBUS_LUDUSAVI", &fake);
        let db = base.join("data").join("ludusavi");
        fs::create_dir_all(&db).unwrap();
        fs::write(db.join("manifest.yaml"), MANIFEST).unwrap();
        fs::write(db.join("cache.yaml"), cache("one")).unwrap();
        fs::write(db.join("config.yaml"), "---\nroots: []\n").unwrap();
        let install = base.join("game");
        fs::create_dir_all(install.join("save")).unwrap();
        fs::write(install.join("game.exe"), b"exe").unwrap();
        let elsewhere = base.join("roots");
        Setup { _tmp: tmp, install, elsewhere, base, _guard: guard }
    }

    fn roots(&self) -> BTreeMap<String, String> {
        ["home", "appdata", "localappdata", "documents", "xdgconfig", "xdgdata", "drive-c", "abs"]
            .iter()
            .map(|r| (r.to_string(), self.elsewhere.join(r).display().to_string()))
            .collect()
    }

    fn game(&self, key: &str, title: &str) -> GameEntry {
        GameEntry {
            key: key.into(),
            name: title.into(),
            exe: Some(self.install.join("game.exe")),
            ludusavi_title: Some(title.into()),
            roots: self.roots(),
            auto_sync: true,
            ..Default::default()
        }
    }

    fn manifest(&self, text: &str, etag: &str) {
        let db = self.base.join("data").join("ludusavi");
        fs::write(db.join("manifest.yaml"), text).unwrap();
        fs::write(db.join("cache.yaml"), cache(etag)).unwrap();
    }
}

fn add(game: GameEntry) {
    config::update(|cfg| {
        cfg.games.push(game);
        Ok(())
    })
    .unwrap();
}

fn listed(key: &str) -> GameEntry {
    config::load().unwrap().game(key).unwrap().clone()
}

fn entry(path: &str, content: &str) -> FileEntry {
    FileEntry { path: path.into(), size: content.len() as u64, hash: blake3::hash(content.as_bytes()).to_hex().to_string(), mtime: 0 }
}

/// A version's files as a download leaves them, `<dir>/<path>`.
fn version(dir: &Path, files: &[(&str, &str)]) -> Vec<FileEntry> {
    let _ = fs::remove_dir_all(dir);
    files
        .iter()
        .map(|(path, content)| {
            let p = path.split('/').fold(dir.to_path_buf(), |p, c| p.join(c));
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, content).unwrap();
            entry(path, content)
        })
        .collect()
}

/// The files below `dir`, `/`-separated and sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    out.sort();
    out
}

#[test]
fn an_existing_game_gets_its_save_patterns_once() {
    let s = Setup::new();
    // Added before there were save patterns from Ludusavi's database.
    add(s.game("fake", "Fake Game"));
    add(GameEntry { key: "plain".into(), name: "Plain".into(), saves: vec!["{install}/save/**".into()], ..Default::default() });
    assert!(listed("fake").ludusavi_saves.is_empty());

    assert_eq!(games::refresh_ludusavi_saves(None).unwrap(), 1);
    let g = listed("fake");
    assert_eq!(g.ludusavi_saves, ["{home}/Saved Games/Fake Game", "{install}/save"]);
    assert!(g.ludusavi_from.is_some());
    // The one without a title is left alone.
    assert!(listed("plain").ludusavi_saves.is_empty() && listed("plain").ludusavi_from.is_none());

    // Once: nothing changed, nothing is worked out again.
    let before = fs::read_to_string(s.base.join("data").join("config.json")).unwrap();
    assert_eq!(games::refresh_ludusavi_saves(None).unwrap(), 0);
    assert_eq!(fs::read_to_string(s.base.join("data").join("config.json")).unwrap(), before);
}

#[test]
fn a_new_database_brings_new_patterns_and_keeps_the_users() {
    let s = Setup::new();
    // The user added a folder of their own.
    add(GameEntry { saves: vec!["{documents}/Fake Extra/**".into()], ..s.game("fake", "Fake Game") });
    games::refresh_ludusavi_saves(None).unwrap();
    assert_eq!(listed("fake").ludusavi_saves, ["{home}/Saved Games/Fake Game", "{install}/save"]);

    // Ludusavi got a newer database: the save folder moved to `profiles`.
    let newer = MANIFEST.replace("\"<base>/save\":", "\"<base>/profiles/*.dat\":");
    s.manifest(&newer, "two");
    assert_eq!(games::refresh_ludusavi_saves(Some("fake")).unwrap(), 1);
    let g = listed("fake");
    assert_eq!(g.ludusavi_saves, ["{home}/Saved Games/Fake Game", "{install}/profiles/*.dat"]);
    assert_eq!(g.saves, ["{documents}/Fake Extra/**"]);

    // A database without the title keeps what the game had.
    s.manifest("Other Game:\n  files:\n    \"<home>/Other\": {}\n", "three");
    games::refresh_ludusavi_saves(None).unwrap();
    assert_eq!(listed("fake").ludusavi_saves, ["{home}/Saved Games/Fake Game", "{install}/profiles/*.dat"]);
}

#[test]
fn a_restore_writes_only_under_the_save_patterns() {
    let s = Setup::new();
    let from = s.base.join("version");
    // In the library, without patterns yet: the restore works them out and
    // keeps them.
    add(GameEntry { saves: vec!["{documents}/Fake Extra/**".into()], ..s.game("fake", "Fake Game") });
    let game = listed("fake");
    let files = version(
        &from,
        &[
            ("install/save/slot1.sav", "progress"),
            ("home/Saved Games/Fake Game/profile.dat", "profile"),
            // A folder the user added.
            ("documents/Fake Extra/notes.txt", "notes"),
            // None of the game's saves.
            ("install/version.dll", "not a save"),
            ("appdata/Microsoft/Windows/Start Menu/Programs/Startup/run.bat", "not a save"),
            ("home/.bashrc", "not a save"),
            ("drive-c/Windows/System32/evil.dll", "not a save"),
        ],
    );
    snapshot::apply_dir(&from, &files, &game).unwrap();
    assert_eq!(listing(&s.install), ["game.exe", "save/slot1.sav"]);
    assert_eq!(listing(&s.elsewhere), ["documents/Fake Extra/notes.txt", "home/Saved Games/Fake Game/profile.dat"]);
    assert_eq!(listed("fake").ludusavi_saves, ["{home}/Saved Games/Fake Game", "{install}/save"]);

    // A save made later in the same folder is one too; nothing of the
    // version is a save: nothing is written.
    let files = version(&from, &[("install/save/slot2.sav", "later")]);
    snapshot::apply_dir(&from, &files, &listed("fake")).unwrap();
    assert!(s.install.join("save").join("slot2.sav").is_file());
    let outside = version(&from, &[("install/version.dll", "not a save"), ("home/.bashrc", "not a save")]);
    let err = snapshot::apply_dir(&from, &outside, &listed("fake")).unwrap_err();
    assert!(format!("{err:#}").contains("install/version.dll"), "{err:#}");
    assert!(!s.install.join("version.dll").exists());

    // What a shared save would bring: the same check.
    let paths = ["install/save/slot1.sav", "home/Saved Games/Fake Game/x.sav", "install/version.dll", "home/.bashrc"];
    let out = snapshot::outside_saves(&listed("fake"), paths.iter().copied()).unwrap().unwrap();
    assert_eq!(out, ["install/version.dll", "home/.bashrc"]);
}

#[test]
fn without_patterns_a_restore_is_as_before() {
    let s = Setup::new();
    let from = s.base.join("version");
    // Ludusavi's database doesn't have the title: no save locations to go
    // by, so the files go where they say (and the log says why).
    let unknown = s.game("unknown", "Unknown Game");
    let files = version(&from, &[("install/save/slot1.sav", "progress"), ("install/extra.cfg", "anything")]);
    snapshot::apply_dir(&from, &files, &unknown).unwrap();
    assert_eq!(listing(&s.install), ["extra.cfg", "game.exe", "save/slot1.sav"]);
    assert!(snapshot::outside_saves(&unknown, ["install/extra.cfg"]).unwrap().is_none());
    let log = fs::read_to_string(s.base.join("data").join("logs").join("sync.log")).unwrap();
    assert!(log.contains("unknown: where its files go is not checked"), "{log}");

    // No database on this device at all: the same.
    fs::remove_file(s.base.join("data").join("ludusavi").join("manifest.yaml")).unwrap();
    let files = version(&from, &[("install/other.cfg", "anything")]);
    snapshot::apply_dir(&from, &files, &s.game("fake", "Fake Game")).unwrap();
    assert!(s.install.join("other.cfg").is_file());
    assert_eq!(games::refresh_ludusavi_saves(None).unwrap(), 0);
}
