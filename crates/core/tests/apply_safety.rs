//! Writing a version's files to a device is all or nothing
//! (docs/plan-v2.md, section 6, "安全网"): a file that doesn't read back as
//! the version says, or one that can't be replaced, leaves the saves as
//! they were. One test, because the data directory is chosen through an
//! environment variable.

use ludonimbus_core::config::GameEntry;
use ludonimbus_core::snapshot::{self, FileEntry};
use std::fs;
use std::path::Path;

fn entry(name: &str, content: &str) -> FileEntry {
    FileEntry { path: format!("install/save/{name}"), size: content.len() as u64, hash: blake3::hash(content.as_bytes()).to_hex().to_string(), mtime: 0 }
}

/// The names and contents of what is in `dir`, folders as `name/`.
fn listing(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                (format!("{name}/"), String::new())
            } else {
                (name, fs::read_to_string(e.path()).unwrap())
            }
        })
        .collect();
    out.sort();
    out
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

#[test]
fn all_or_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::env::set_var("LUDONIMBUS_HOME", root.join("home"));
    let install = root.join("game");
    let saves = install.join("save");
    fs::create_dir_all(&saves).unwrap();
    fs::write(install.join("game.exe"), b"exe").unwrap();
    let game = GameEntry {
        key: "apply-test".into(),
        name: "Apply Test".into(),
        exe: Some(install.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        ..Default::default()
    };
    // The version's files, as a download leaves them.
    let from = root.join("version");
    fs::create_dir_all(from.join("install/save")).unwrap();
    fs::write(from.join("install/save/a.sav"), "A2").unwrap();
    fs::write(from.join("install/save/b.sav"), "B2").unwrap();
    let version = vec![entry("a.sav", "A2"), entry("b.sav", "B2")];

    let before = |extra: &[(&str, &str)]| {
        let _ = fs::remove_dir_all(&saves);
        fs::create_dir_all(&saves).unwrap();
        for (name, content) in [("a.sav", "A1"), ("b.sav", "B1"), ("c.sav", "C1")].iter().chain(extra) {
            fs::write(saves.join(name), content).unwrap();
        }
    };
    let here = pairs(&[("a.sav", "A1"), ("b.sav", "B1"), ("c.sav", "C1")]);

    // A file whose content is not what the version says: nothing changes.
    before(&[]);
    let wrong = vec![entry("a.sav", "A2"), entry("b.sav", "something else")];
    let err = snapshot::apply_dir(&from, &wrong, &game).unwrap_err();
    assert!(format!("{err:#}").contains("install/save/b.sav"), "{err:#}");
    assert_eq!(listing(&saves), here);

    // A file the version has that the download doesn't: nothing changes.
    let missing = vec![entry("a.sav", "A2"), entry("d.sav", "D2")];
    assert!(snapshot::apply_dir(&from, &missing, &game).is_err());
    assert_eq!(listing(&saves), here);

    // A file that can't be put in place (a folder is where it goes): the
    // one already replaced is put back.
    let _ = fs::remove_dir_all(&saves);
    fs::create_dir_all(saves.join("b.sav")).unwrap();
    fs::write(saves.join("a.sav"), "A1").unwrap();
    fs::write(saves.join("c.sav"), "C1").unwrap();
    assert!(snapshot::apply_dir(&from, &version, &game).is_err());
    assert_eq!(listing(&saves), pairs(&[("a.sav", "A1"), ("b.sav/", ""), ("c.sav", "C1")]));

    // What a write that was cut short left behind is not a save, and is
    // cleared away by the next one that goes through.
    before(&[("a.sav.lntmp", "half"), ("b.sav.lnold", "B0"), ("c.sav.lnold", "C0")]);
    let (found, _) = snapshot::scan(&game).unwrap();
    let mut names: Vec<&str> = found.iter().map(|f| f.logical.as_str()).collect();
    names.sort();
    assert_eq!(names, ["install/save/a.sav", "install/save/b.sav", "install/save/c.sav"]);

    snapshot::apply_dir(&from, &version, &game).unwrap();
    // The saves are exactly the version's: its files, and none of the others.
    assert_eq!(listing(&saves), pairs(&[("a.sav", "A2"), ("b.sav", "B2")]));
    assert_eq!(snapshot::local_state(&game).unwrap().hash, snapshot::content_hash(&version));

    // Two names that differ in case only are one file on Windows: the later
    // one is what the file holds, and a write that fails still changes nothing.
    fs::write(from.join("install/save/A.SAV"), "A3").unwrap();
    let twice = vec![entry("a.sav", "A2"), entry("b.sav", "B2"), entry("A.SAV", "A3")];
    if fs::read_to_string(from.join("install/save/a.sav")).unwrap() == "A3" {
        // The download folder is on a file system that ignores case too.
        before(&[]);
        let twice: Vec<FileEntry> = twice.iter().map(|f| FileEntry { hash: String::new(), ..f.clone() }).collect();
        snapshot::apply_dir(&from, &twice, &game).unwrap();
        assert_eq!(listing(&saves), pairs(&[("A.SAV", "A3"), ("b.sav", "B2")]));
        fs::write(from.join("install/save/a.sav"), "A2").unwrap();
    } else {
        fs::remove_file(from.join("install/save/A.SAV")).unwrap();
    }

    // A version of 0.6 that doesn't say what a file's content is still applies.
    before(&[]);
    let unhashed: Vec<FileEntry> = version.iter().map(|f| FileEntry { hash: String::new(), ..f.clone() }).collect();
    snapshot::apply_dir(&from, &unhashed, &game).unwrap();
    assert_eq!(listing(&saves), pairs(&[("a.sav", "A2"), ("b.sav", "B2")]));
}
