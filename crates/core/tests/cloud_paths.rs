//! Paths at the edge of a store: parts of them come from files in the cloud
//! (a game record's folders, a manifest's packs), so a path that would lead
//! out of the store's root folder is refused, not followed. And a folder
//! that can't be listed is an error, never an empty cloud.

use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::webdav::WebDav;
use std::fs;
use std::time::Duration;

const LEADING_OUT: [&str; 6] = ["..", "../outside.txt", "games/../../outside.txt", r"games\..\..\outside.txt", "C:/outside.txt", "games/C:outside.txt"];

#[test]
fn a_local_folder_stays_inside_its_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("cloud");
    fs::create_dir_all(&root).unwrap();
    let outside = tmp.path().join("outside.txt");
    fs::write(&outside, "mine").unwrap();
    let src = tmp.path().join("src.bin");
    fs::write(&src, "data").unwrap();
    let store = LocalFolder::new(root.clone());

    for path in LEADING_OUT {
        assert!(store.write(path, b"x").is_err(), "{path}");
        assert!(store.upload(path, &src).is_err(), "{path}");
        assert!(store.read(path).is_err(), "{path}");
        assert!(store.download(path, &tmp.path().join("got.bin")).is_err(), "{path}");
        assert!(store.remove(path).is_err(), "{path}");
        assert!(store.list(path).is_err(), "{path}");
    }
    assert_eq!(fs::read_to_string(&outside).unwrap(), "mine");
    assert!(!tmp.path().join("got.bin").exists());

    // Paths as Ludonimbus makes them work as ever; empty segments lead
    // nowhere else and are skipped as before.
    store.write("games/a/saves/main/x.json", b"{}").unwrap();
    store.write("/games/a//y.json", b"{}").unwrap();
    assert!(root.join("games/a/y.json").is_file());
    let mut names: Vec<String> = store.list("games/a").unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["saves", "y.json"]);
}

#[test]
fn a_local_folder_that_cant_be_listed_is_no_empty_cloud() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("cloud");
    fs::create_dir_all(root.join("games")).unwrap();
    fs::write(root.join("games/file.json"), "{}").unwrap();
    let store = LocalFolder::new(root.clone());

    // A folder that isn't there is empty, and so is one that is a file.
    assert!(store.list("games/nothing/here").unwrap().is_empty());
    assert!(store.list("games/file.json").unwrap().is_empty());
    assert_eq!(store.list("games").unwrap().len(), 1);

    // A folder that can't be read (no permission here) is an error.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = root.join("games/locked");
        fs::create_dir_all(locked.join("inside")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // Someone who may read anything (root) reads it anyway.
        if fs::read_dir(&locked).is_err() {
            assert!(store.list("games/locked").is_err());
        }
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        // Names no path may have are no files of ours.
        fs::write(root.join("games/a:b"), "x").unwrap();
        fs::write(root.join(r"games/a\b"), "x").unwrap();
        let mut names: Vec<String> = store.list("games").unwrap().into_iter().map(|e| e.name).collect();
        names.sort();
        assert_eq!(names, ["file.json", "locked"]);
    }

    // The folder went away with the share it was on: an error too.
    fs::remove_dir_all(&root).unwrap();
    assert!(store.list("games").is_err());
}

#[test]
fn webdav_refuses_a_path_that_leads_out() {
    // Nothing listens there: a path that got as far as a request would fail
    // with a connection error instead, which doesn't name the path.
    let dav = WebDav::new("http://127.0.0.1:9/dav/", "u", "p", "Ludonimbus", Duration::from_secs(2)).unwrap();
    for path in LEADING_OUT {
        let e = dav.read(path).unwrap_err();
        assert!(format!("{e:#}").contains(path), "{path}: {e:#}");
        assert!(format!("{:#}", dav.write(path, b"x").unwrap_err()).contains(path), "{path}");
        assert!(format!("{:#}", dav.remove(path).unwrap_err()).contains(path), "{path}");
        assert!(format!("{:#}", dav.list(path).unwrap_err()).contains(path), "{path}");
    }
}
