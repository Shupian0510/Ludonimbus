//! Every game rule in `rules/games/` passes `ludonimbus rules check`
//! (`crosssaves::check_file`): its fields, and its sample saves for at least
//! two platforms. And the check says what is wrong with a file that isn't
//! right.

use ludonimbus_core::crosssaves::{check_file, rules};
use std::path::{Path, PathBuf};

fn rules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/games")
}

#[test]
fn every_rule_file_passes_the_check() {
    let mut n = 0;
    for path in std::fs::read_dir(rules_dir()).unwrap().flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|x| x != "yaml") {
            continue;
        }
        let c = check_file(&path).unwrap();
        assert!(c.problems.is_empty(), "{}: {:#?}", path.display(), c.problems);
        assert!(c.notes.len() >= 2, "{}: samples for two platforms: {:?}", path.display(), c.notes);
        n += 1;
    }
    // The program has the same ones built in.
    assert_eq!(n, rules().len());
}

#[test]
fn the_check_says_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let p = dir.path().join(name);
        std::fs::write(&p, text).unwrap();
        p
    };

    // A misspelt field.
    let p = write("game.yaml", "id: game\nname: Game\nids: [name:game]\nlines:\n  - id: main\n    platfroms: {}\n");
    let c = check_file(&p).unwrap();
    assert!(c.problems.iter().any(|x| x.contains("platfroms")), "{:?}", c.problems);

    // Fields that read but aren't right, and no samples.
    let p = write(
        "other.yaml",
        "id: Some_Game\nname: Game\nids: [nx:123, steam:1]\nlines:\n  - id: main\n    files: '*/*'\n    platforms:\n      pc: { saves: '{appdata}/Game' }\n      ps9: { saves: '{nxsave}' }\n    checked: { pc-xbox: tried }\n",
    );
    let c = check_file(&p).unwrap();
    let has = |s: &str| c.problems.iter().any(|x| x.contains(s));
    assert!(has("lowercase letters and digits"), "{:?}", c.problems);
    assert!(has("the file's name"), "{:?}", c.problems);
    assert!(has("\"nx:123\""), "{:?}", c.problems);
    assert!(has("\"steam:1\""), "{:?}", c.problems);
    assert!(has("\"ps9\""), "{:?}", c.problems);
    assert!(has("only for switch"), "{:?}", c.problems);
    assert!(has("platforms.pc.saves"), "{:?}", c.problems);
    assert!(has("pc-xbox"), "{:?}", c.problems);
    assert!(has("at least two platforms"), "{:?}", c.problems);

    // Samples that don't agree, and a version that can't be read.
    let p = write(
        "game.yaml",
        "id: game\nname: Game\nids: [name:game]\nlines:\n  - id: main\n    files: '*.sav'\n    platforms:\n      pc: { saves: Game/Saves }\n      switch: { saves: '{nxsave}' }\n    version: { file: '*.sav', xml: v }\n",
    );
    for (scope, file) in [("pc", "a.sav"), ("switch", "b.sav")] {
        let d = dir.path().join("game").join(scope);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(file), b"no version here").unwrap();
    }
    let c = check_file(&p).unwrap();
    assert!(c.problems.iter().any(|x| x.contains("hold different saves")), "{:?}", c.problems);
    assert!(c.problems.iter().any(|x| x.contains("no game version")), "{:?}", c.problems);
}
