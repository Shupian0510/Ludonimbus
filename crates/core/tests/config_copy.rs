//! The configuration an older version of Ludonimbus wrote is kept as
//! `config.v1.json` the first time this version changes it
//! (docs/plan-v2.md, 12.4).

use ludonimbus_core::config;

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn an_older_configuration_is_kept_once() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", home.path());
    let file = home.path().join("config.json");
    let copy = home.path().join("config.v1.json");

    // A configuration this version made: there is nothing older to keep.
    config::load().unwrap();
    config::update(|c| {
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    assert!(!copy.exists());

    // One an older version wrote.
    let mut v: serde_json::Value = serde_json::from_str(&read(&file)).unwrap();
    v["version"] = 1.into();
    let old = serde_json::to_string_pretty(&v).unwrap();
    std::fs::write(&file, &old).unwrap();

    assert!(config::load().unwrap().nx_lan);
    assert!(!copy.exists(), "reading it changes nothing");
    assert_eq!(read(&file), old);

    config::update(|c| {
        c.nx_lan = false;
        Ok(())
    })
    .unwrap();
    assert_eq!(read(&copy), old, "the file as the older version left it");
    let now: serde_json::Value = serde_json::from_str(&read(&file)).unwrap();
    assert_eq!(now["version"], config::VERSION);
    assert_eq!(now["nxLan"], false);

    // Kept once: what this version writes later never replaces the copy.
    config::update(|c| {
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    assert_eq!(read(&copy), old);
}
