//! The collection's shelves (`shelf.rs`): a tag on the game's card in the
//! cloud, set and taken off from any device, read back with the cards.

use anyhow::Result;
use ludonimbus_core::cloud::library::{self, LibraryCache};
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::Cloud;
use ludonimbus_core::config::{self, AppConfig, CloudConfig, GameEntry};
use ludonimbus_core::shelf::{self, ARCHIVED, FINISHED};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Resolution};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// One store shared by the test and the device.
#[derive(Clone, Default)]
struct Shared(Arc<Memory>);

impl Store for Shared {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".into()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.0.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.0.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.0.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.0.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.0.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.0.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("shelves-test".into())
    }
}

#[test]
fn a_game_goes_on_a_shelf_and_comes_off_it() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", dir.path().join("home"));
    let game = dir.path().join("game");
    fs::create_dir_all(game.join("save")).unwrap();
    fs::write(game.join("game.exe"), b"exe").unwrap();
    fs::write(game.join("save").join("slot1.sav"), b"day 1").unwrap();
    let mut cfg = AppConfig::default();
    cfg.device.name = "Desk".into();
    cfg.cloud = CloudConfig::None;
    cfg.games.push(GameEntry {
        key: "hollow".into(),
        name: "Hollow".into(),
        exe: Some(game.join("game.exe")),
        saves: vec!["{install}/save/**".into()],
        auto_sync: true,
        ..Default::default()
    });
    cfg.games.push(GameEntry { key: "never".into(), name: "Never Uploaded".into(), ..Default::default() });
    config::update(|c| {
        *c = cfg.clone();
        Ok(())
    })
    .unwrap();
    let store = Shared::default();
    let ctx = || {
        let cfg = config::load().unwrap();
        let cloud = Cloud::new(Box::new(store.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    };
    let record = |id: &str| library::get(&library::load(&store, &mut LibraryCache::default()).unwrap().records, id).cloned().unwrap();

    let c = ctx();
    sync::sync(&c, &c.game("hollow").unwrap(), Resolution::Auto, Trigger::SyncNow).unwrap();
    // A game that never uploaded has no card to put on a shelf.
    let c = ctx();
    let err = shelf::set(&c, &["never".into()], FINISHED, true).unwrap_err();
    assert!(format!("{err:#}").contains("record") || format!("{err:#}").contains("记录"), "{err:#}");

    // On both shelves, then off one: the card in the cloud says so.
    let c = ctx();
    let tags = shelf::set(&c, &["hollow".into()], FINISHED, true).unwrap();
    assert_eq!(tags["hollow"], vec![FINISHED.to_string()]);
    shelf::set(&c, &["hollow".into()], ARCHIVED, true).unwrap();
    assert_eq!(record("hollow").tags, vec![FINISHED.to_string(), ARCHIVED.to_string()]);
    // Setting it again changes nothing.
    shelf::set(&c, &["hollow".into()], ARCHIVED, true).unwrap();
    assert_eq!(record("hollow").tags.len(), 2);

    // Another look at the cloud reads them with the cards.
    let c = ctx();
    let by_record = c.cloud().unwrap().record_tags().unwrap();
    assert_eq!(shelf::of(&c.cfg, &by_record, "hollow"), vec![FINISHED.to_string(), ARCHIVED.to_string()]);
    assert!(shelf::of(&c.cfg, &by_record, "never").is_empty());

    // Taken off: gone from the card, even though it was read before.
    shelf::set(&c, &["hollow".into()], FINISHED, false).unwrap();
    assert_eq!(record("hollow").tags, vec![ARCHIVED.to_string()]);
    shelf::set(&c, &["hollow".into()], ARCHIVED, false).unwrap();
    assert!(record("hollow").tags.is_empty());
    assert!(ctx().cloud().unwrap().record_tags().unwrap().is_empty());

    // Only the two shelves there are.
    assert!(shelf::set(&c, &["hollow".into()], "favourite", true).is_err());
}
