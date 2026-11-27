//! The sample cloud written by Ludonimbus 0.6.2 (`tests/fixtures/cloud-0.6`):
//! what the reader of cloud layout 2 makes of it, before any migration.

use ludonimbus_core::cloud::line::{self, LineCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::v2;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cloud-0.6")
}

/// The sample as a cloud: every file under its path.
fn sample() -> Memory {
    let store = Memory::new();
    let root = fixture().join("cloud");
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel: Vec<String> = entry.path().strip_prefix(&root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        store.put(&rel.join("/"), &std::fs::read(entry.path()).unwrap());
    }
    store
}

/// What 0.6.2 itself listed for each game when the sample was made.
fn expected() -> Value {
    serde_json::from_slice(&std::fs::read(fixture().join("expected.json")).unwrap()).unwrap()
}

fn old_folder(key: &str) -> String {
    format!("games/{key}/snapshots")
}

#[test]
fn every_old_snapshot_is_found_in_the_order_it_was_made() {
    let store = sample();
    let expected = expected();
    for (key, snapshots) in expected.as_object().unwrap() {
        let snapshots = snapshots.as_array().unwrap();
        let view = line::load(&store, &old_folder(key), &mut LineCache::default()).unwrap();
        assert!(view.unreadable.is_empty(), "{key}");
        assert_eq!(view.snapshots.len(), snapshots.len(), "{key}");
        for (found, was) in view.snapshots.iter().zip(snapshots) {
            assert_eq!(found.id, was["id"].as_str().unwrap());
            assert_eq!(found.content_hash, was["contentHash"].as_str().unwrap());
            assert_eq!(found.trigger, was["trigger"].as_str().unwrap());
            assert_eq!(found.device_name, was["deviceName"].as_str().unwrap());
            assert_eq!(found.file_count, was["files"].as_object().unwrap().len());
            let side = matches!(found.trigger.as_str(), "conflict-keep" | "pre-restore");
            assert_eq!(found.is_side(), side);
        }
        // One history, never a fork: 0.6 had a single newest version.
        assert!(!view.is_forked(), "{key}");
    }
}

#[test]
fn the_newest_version_is_the_one_0_6_showed() {
    let store = sample();
    let expected = expected();
    let snapshots = expected["test-game"].as_array().unwrap();
    let view = line::load(&store, &old_folder("test-game"), &mut LineCache::default()).unwrap();

    // The last upload; the copies kept beside the history don't count.
    let head = view.head().unwrap();
    assert_eq!(head.id, snapshots.last().unwrap()["id"].as_str().unwrap());
    let mainline = snapshots.iter().filter(|s| !matches!(s["trigger"].as_str().unwrap(), "conflict-keep" | "pre-restore")).count();
    assert_eq!(mainline, 9);
    assert_eq!(head.vv, line::legacy_vv(&old_folder("test-game"), head.created_at));
    assert!(view.snapshots.iter().filter(|s| s.is_side()).all(|s| s.vv.is_empty()));
}

#[test]
fn what_0_6_kept_in_a_snapshot_is_still_readable() {
    let store = sample();
    let expected = expected();
    let dir = old_folder("test-game");
    let snapshots = expected["test-game"].as_array().unwrap();
    let locked = snapshots.iter().find(|s| s["locked"] == true).unwrap();
    let manual = snapshots.iter().find(|s| s["trigger"] == "manual").unwrap();

    // The lock and the notes lived in the snapshot's own file; the migration
    // turns them into kept saves.
    let m = line::read_manifest(&store, &dir, locked["id"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(m.v, 1);
    assert_eq!(m.line, "");
    assert_eq!(m.game, "test-game");
    assert_eq!(m.extra["locked"], true);
    assert_eq!(m.extra["comment"], "keep this");
    assert_eq!(m.app_version, "0.6.2");
    // All of its content is in the zip beside it.
    assert_eq!(m.packs().into_iter().collect::<Vec<_>>(), std::slice::from_ref(&m.id));
    assert!(store.peek(&format!("{dir}/{}.zip", m.id)).is_some());

    let m = line::read_manifest(&store, &dir, manual["id"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(m.extra["comment"], "before boss");

    // A save folder no root covered is recorded by drive letter; the
    // migration gives it a named mount point (docs/plan-v2.md, 12.2).
    let view = line::load(&store, &old_folder("abs-game"), &mut LineCache::default()).unwrap();
    let m = line::read_manifest(&store, &old_folder("abs-game"), &view.head().unwrap().id).unwrap().unwrap();
    assert!(m.files.iter().all(|f| f.path.starts_with("drive-c/lnfix/abs-saves/")));
}

#[test]
fn the_sample_has_no_marker_of_the_new_layout() {
    let store = sample();
    assert_eq!(v2::access(v2::read_format(&store).unwrap().as_ref()), v2::Access::Legacy);
}
