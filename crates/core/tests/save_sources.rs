//! Saves shown by where they come from, and newer progress elsewhere
//! (docs/plan-positioning.md, section 7: S1 and S2).
//!
//! PC A plays Celeste; PC B has another PC version of it (other folders, so
//! saves are placed by file name); PC C plays A's version but keeps its saves
//! to itself. Each snapshot says how long the game had been played; B sees
//! A's version as a source with newer progress, takes it (kept first, can be
//! undone), and is offered A's next save until it says no. C's own line is
//! listed for A, never offered. One test, because the data directory is
//! chosen through an environment variable.

use chrono::{DateTime, Duration, Utc};
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::elsewhere::{self, Kind, Source, Take};
use ludonimbus_core::playtime::{self, Via};
use ludonimbus_core::snapshot::{SnapshotSummary, Trigger};
use ludonimbus_core::sync::{self, Ctx, LineChoice, Outcome};
use ludonimbus_core::{savelib, state};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// One device: its own data folder, connected to the shared cloud folder.
fn device(tmp: &Path, name: &str, cloud: &Path) {
    std::env::set_var("LUDONIMBUS_HOME", tmp.join(format!("home-{name}")));
    let mut cfg = AppConfig::default();
    cfg.device.id = format!("d-{name}");
    cfg.device.name = format!("PC-{name}");
    cfg.cloud = CloudConfig::LocalFolder { path: cloud.to_path_buf() };
    config::update(|c| {
        if c.device.id != cfg.device.id {
            *c = cfg.clone();
        }
        Ok(())
    })
    .unwrap();
}

fn add(key: &str, saves: &str, root: &Path) -> GameEntry {
    let g = GameEntry {
        key: key.into(),
        name: "Celeste".into(),
        saves: vec![saves.into()],
        roots: BTreeMap::from([("data".to_string(), root.to_string_lossy().into_owned())]),
        auto_sync: true,
        ..Default::default()
    };
    config::update(|c| {
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();
    g
}

/// A session of `minutes` that ended a moment ago.
fn played(game: &GameEntry, minutes: i64) {
    let cfg = config::load().unwrap();
    let end = Utc::now() - Duration::seconds(5);
    assert!(playtime::record(&cfg, game, end - Duration::minutes(minutes), end, Via::Direct).unwrap());
}

fn newest(ctx: &Ctx, key: &str) -> SnapshotSummary {
    let list = ctx.cloud().unwrap().list_snapshots(key).unwrap();
    sync::mainline_head(&list).unwrap()
}

/// The texts in English, whatever the system says (both tests ask for the same).
fn english() {
    std::env::set_var("LUDONIMBUS_LANG", "en");
    ludonimbus_core::i18n::set("en");
}

/// Let the clock move on: saves made after this are newer.
fn later() {
    std::thread::sleep(std::time::Duration::from_millis(1100));
}

#[test]
fn saves_by_source_and_newer_progress_elsewhere() {
    english();
    let tmp = tempfile::tempdir().unwrap();
    let cloud = tmp.path().join("cloud");
    fs::create_dir_all(&cloud).unwrap();

    // PC B: its version keeps the saves one folder deeper; it played 10 min.
    device(tmp.path(), "b", &cloud);
    let b_root = tmp.path().join("b").join("Celeste");
    write(&b_root.join("Saves").join("0.celeste"), b"chapter 1");
    let gog = add("celeste-gog", "{data}/Saves/**", &b_root);
    played(&gog, 10);
    let ctx = Ctx::load().unwrap();
    assert!(matches!(sync::sync(&ctx, &gog, sync::Resolution::Auto, Trigger::SyncNow).unwrap(), Outcome::Uploaded { .. }));
    // Each snapshot says how long the game had been played, and on what.
    let b_first = newest(&ctx, "celeste-gog");
    assert_eq!(b_first.playtime, Some(600));
    assert_eq!((b_first.platform.as_str(), b_first.device_name.as_str()), ("pc", "PC-b"));
    later();

    // PC A: an hour, then a save.
    device(tmp.path(), "a", &cloud);
    let a_saves = tmp.path().join("a").join("Celeste").join("Saves");
    write(&a_saves.join("0.celeste"), b"chapter 7");
    let steam = add("celeste", "{data}/**", &a_saves);
    played(&steam, 60);
    let ctx = Ctx::load().unwrap();
    assert!(matches!(sync::sync(&ctx, &steam, sync::Resolution::Auto, Trigger::SyncNow).unwrap(), Outcome::Uploaded { .. }));
    let from_a = newest(&ctx, "celeste");
    assert_eq!(from_a.playtime, Some(3600));
    later();

    // On B, A's version is a source: another version, its saves placed by
    // file name, with newer progress than the save here.
    device(tmp.path(), "b", &cloud);
    let cfg = config::load().unwrap();
    let ctx = Ctx::load().unwrap();
    savelib::refresh(&cfg, ctx.cloud().unwrap()).unwrap();
    let ctx = Ctx::load().unwrap();
    let list = elsewhere::sources(&ctx, &gog).unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    let a = &list[0];
    assert_eq!((a.kind, a.key.as_deref(), a.line.as_deref()), (Kind::Version, Some("celeste"), None));
    assert_eq!(a.take, Take::Convert { how: "files".into() });
    assert_eq!((a.name.as_str(), a.platform.as_str()), ("PC version", "pc"));
    assert_eq!(a.devices, vec!["PC-a".to_string()]);
    assert_eq!(a.newest().map(|s| s.id.as_str()), Some(from_a.id.as_str()));

    let n = elsewhere::newer(&ctx, &gog).unwrap().expect("A's save is newer progress");
    assert_eq!(n.snapshot.id, from_a.id);
    assert_eq!(n.here.as_ref().map(|h| h.id.as_str()), Some(b_first.id.as_str()));
    let q = elsewhere::question(&gog, &n, false, &[], Utc::now());
    assert_eq!(q.title, "Celeste has newer progress in the PC version");
    assert!(q.text.starts_with("PC version · PC-a · today "), "{}", q.text);
    assert!(q.text.contains("50 min more play") && q.text.contains("PC version (here) · PC-b"), "{}", q.text);
    assert_eq!((q.take.as_str(), q.keep.as_str(), q.join.as_str()), ("Use the PC version's progress", "Keep the PC version's", ""));
    // The window shows the two saves side by side: there (the newer), here.
    let c = &q.compare;
    assert_eq!((c.there.name.as_str(), c.there.device.as_str(), c.there.playtime, c.there.newer), ("PC version", "PC-a", Some(3600), true));
    assert_eq!(c.there.at, Some(from_a.created_at));
    assert_eq!(c.here.as_ref().map(|h| (h.device.as_str(), h.playtime, h.newer)), Some(("PC-b", Some(600), false)));
    assert_eq!((c.convert.as_str(), c.notes.len()), ("files", 0));

    // Taken here: kept first, the copy says where it came from and how far
    // it was played, and it can be undone. Nothing newer then.
    let out = elsewhere::take(&ctx, &gog, &n.source, &n.snapshot.id).unwrap();
    assert!(matches!(out, Outcome::Uploaded { .. }), "{out:?}");
    assert_eq!(fs::read(b_root.join("Saves").join("0.celeste")).unwrap(), b"chapter 7");
    let copy = newest(&ctx, "celeste-gog");
    assert_eq!((copy.copied.as_str(), copy.playtime), ("convert:files", Some(3600)));
    let line = ctx.cloud().unwrap().list_snapshots("celeste-gog").unwrap();
    assert!(line.iter().any(|s| s.trigger == Trigger::PreRestore || s.id == b_first.id));
    assert!(sync::undoable("celeste-gog").is_some_and(|r| r.device == "PC-a"));
    assert!(elsewhere::newer(&ctx, &gog).unwrap().is_none());
    later();

    // A plays on: offered again, until B keeps its own.
    device(tmp.path(), "a", &cloud);
    write(&a_saves.join("0.celeste"), b"chapter 8");
    let ctx = Ctx::load().unwrap();
    sync::sync(&ctx, &steam, sync::Resolution::Auto, Trigger::SyncNow).unwrap();
    let again = newest(&ctx, "celeste");
    device(tmp.path(), "b", &cloud);
    let ctx = Ctx::load().unwrap();
    let n = elsewhere::newer(&ctx, &gog).unwrap().expect("offered again");
    assert_eq!(n.snapshot.id, again.id);
    elsewhere::dismiss("celeste-gog", &again.id).unwrap();
    assert_eq!(state::game("celeste-gog").newer_dismissed.as_deref(), Some(again.id.as_str()));
    assert!(elsewhere::newer(&ctx, &gog).unwrap().is_none());
    // The Steam plugins' view of it says the same.
    assert!(savelib::newer_version(&ctx, &gog).unwrap().is_none());
    later();

    // PC C plays A's version on a line of its own: A lists it, as C's,
    // taken as it is; it is never offered, however new.
    device(tmp.path(), "c", &cloud);
    let c_saves = tmp.path().join("c").join("Celeste").join("Saves");
    write(&c_saves.join("0.celeste"), b"chapter 3 on C");
    let c_game = add("celeste", "{data}/**", &c_saves);
    let ctx = Ctx::load().unwrap();
    sync::use_line(&ctx, &c_game, LineChoice::Independent { name: None }).unwrap();
    device(tmp.path(), "a", &cloud);
    let cfg = config::load().unwrap();
    let ctx = Ctx::load().unwrap();
    savelib::refresh(&cfg, ctx.cloud().unwrap()).unwrap();
    let ctx = Ctx::load().unwrap();
    let list = elsewhere::sources(&ctx, &steam).unwrap();
    let own = list.iter().find(|s| s.kind == Kind::Device).expect("C's own line is listed");
    assert_eq!((own.name.as_str(), own.take.clone(), own.key.as_deref()), ("PC-c", Take::Copy, None));
    assert!(own.line.as_deref().is_some_and(|l| l.starts_with("solo-")));
    assert_eq!(list.first().map(|s| s.kind), Some(Kind::Version), "other versions come first: {list:?}");
    assert!(elsewhere::newer(&ctx, &steam).unwrap().is_none());
}

fn snap(id: &str, at: DateTime<Utc>, hash: &str) -> SnapshotSummary {
    SnapshotSummary {
        id: id.into(),
        parent: None,
        device_id: "d".into(),
        device_name: "PC".into(),
        created_at: at,
        trigger: Trigger::AutoExit,
        content_hash: hash.into(),
        total_size: 1,
        file_count: 1,
        comment: String::new(),
        locked: false,
        game_version: None,
        installed_version: None,
        platform: "pc:xbox".into(),
        head: true,
        side: false,
        incomplete: false,
        playtime: None,
        copied: String::new(),
    }
}

fn source(id: &str, kind: Kind, take: Take, newest: SnapshotSummary) -> Source {
    Source {
        id: id.into(),
        kind,
        name: id.into(),
        platform: newest.platform.clone(),
        line: None,
        key: Some(id.into()),
        devices: vec![newest.device_name.clone()],
        snapshots: vec![newest],
        take,
        def: None,
        dir: String::new(),
    }
}

#[test]
fn which_progress_elsewhere_is_newer() {
    let t0 = Utc::now() - Duration::hours(5);
    let at = |h: i64| t0 + Duration::hours(h);
    let convert = || Take::Convert { how: "xbox".into() };
    let here = snap("here", at(1), "h-here");
    let xbox = source("xbox", Kind::Version, convert(), snap("x2", at(3), "h-x"));
    let switch = source("switch", Kind::Version, Take::Copy, snap("s1", at(2), "h-s"));
    let id = |s: Option<&Source>| s.map(|s| s.id.clone());

    // The newest of the versions newer than the save here.
    let list = vec![switch.clone(), xbox.clone()];
    assert_eq!(id(elsewhere::pick(Some(&here), None, &list, None)), Some("xbox".into()));
    // Nothing when the saves here changed after it, or the save here is newer.
    assert_eq!(id(elsewhere::pick(Some(&here), Some(at(4)), &list, None)), None);
    assert_eq!(id(elsewhere::pick(Some(&snap("later", at(4), "h-l")), None, &list, None)), None);
    // Without saves here, any.
    assert_eq!(id(elsewhere::pick(None, None, &list, None)), Some("xbox".into()));
    // The same saves are no newer progress.
    let same = vec![source("xbox", Kind::Version, convert(), snap("x2", at(3), "h-here"))];
    assert_eq!(id(elsewhere::pick(Some(&here), None, &same, None)), None);
    // Said no to: not offered, nor an older one of another version.
    assert_eq!(id(elsewhere::pick(Some(&here), None, &list, Some("x2"))), None);
    assert_eq!(id(elsewhere::pick(Some(&here), None, &list, Some("s1"))), Some("xbox".into()));
    // Lines kept apart, versions that can't be brought here, saves that
    // miss a pack: never offered.
    let apart = vec![
        source("kid", Kind::Device, Take::Copy, snap("k", at(4), "h-k")),
        source("shared", Kind::Shared, Take::Copy, snap("sh", at(4), "h-sh")),
        source("wiiu", Kind::Version, Take::Never { reason: "no rule".into() }, snap("w", at(4), "h-w")),
        source("broken", Kind::Version, Take::Copy, SnapshotSummary { incomplete: true, ..snap("b", at(4), "h-b") }),
        switch.clone(),
    ];
    assert_eq!(id(elsewhere::pick(Some(&here), None, &apart, None)), Some("switch".into()));
    // A source whose newest listed save is only a kept one has no progress to offer.
    let kept_only = vec![source("old", Kind::Version, Take::Copy, SnapshotSummary { head: false, ..snap("o", at(4), "h-o") })];
    assert_eq!(id(elsewhere::pick(Some(&here), None, &kept_only, None)), None);

    // How the difference in playtime is said.
    let there = SnapshotSummary { playtime: Some(7200 + 600), ..snap("t", at(3), "a") };
    let mine = SnapshotSummary { playtime: Some(600), ..snap("m", at(1), "b") };
    english();
    assert_eq!(elsewhere::played_more(&there, Some(&mine)).as_deref(), Some("2 h more play"));
    assert_eq!(elsewhere::played_more(&mine, Some(&there)).as_deref(), Some("2 h less play"));
    assert_eq!(elsewhere::played_more(&there, Some(&snap("u", at(1), "c"))), None);
    assert_eq!(elsewhere::played_more(&there, None), None);
}
