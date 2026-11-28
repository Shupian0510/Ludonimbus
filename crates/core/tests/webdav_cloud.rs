//! The WebDAV store and the shared cloud layout against a real WebDAV server
//! (`dav-server`, in memory), plus copying a whole cloud from a local folder
//! to WebDAV. Runs as one test because the cache folder is chosen through an
//! environment variable.

use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::{self, migrate, repo::Repo, CloudGame, CloudProvider, DeviceInfo};
use ludonimbus_core::config::CloudConfig;
use ludonimbus_core::snapshot::{SnapshotMeta, Trigger};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::Path;
use std::time::Duration;

/// Start an in-memory WebDAV server on a free port.
fn start_server() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let handler =
                dav_server::DavHandler::builder().filesystem(dav_server::memfs::MemFs::new()).locksystem(dav_server::fakels::FakeLs::new()).build_handler();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let handler = handler.clone();
                        async move { Ok::<_, Infallible>(handler.handle(req).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(stream), service).await;
                });
            }
        });
    });
    rx.recv().unwrap()
}

fn dav_config(port: u16, folder: &str) -> CloudConfig {
    CloudConfig::WebDav { url: format!("http://127.0.0.1:{port}/"), username: "player".into(), folder: folder.into() }
}

fn dav_store(port: u16, folder: &str) -> Box<dyn Store> {
    cloud::build_store(&dav_config(port, folder), Some("secret"), Duration::from_secs(10)).unwrap()
}

fn meta(game: &str, id: &str, parent: Option<&str>, comment: &str) -> SnapshotMeta {
    SnapshotMeta {
        id: id.into(),
        game: game.into(),
        parent: parent.map(str::to_string),
        device_id: "d-a".into(),
        device_name: "PC-A".into(),
        created_at: chrono::Utc::now(),
        trigger: Trigger::AutoExit,
        roots: BTreeMap::new(),
        content_hash: format!("hash-{id}"),
        files: Vec::new(),
        total_size: 3,
        app_version: "test".into(),
        comment: comment.into(),
        locked: false,
        game_version: None,
        installed_version: None,
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn store_basics(port: u16, tmp: &Path) {
    let folder = "Ludonimbus/存档 test";
    // A full connection test, as the settings page runs it.
    let account = cloud::probe(&dav_config(port, folder), Some("secret"), Duration::from_secs(10)).unwrap();
    assert!(account.label.ends_with("/Ludonimbus/存档 test"), "{}", account.label);

    let s = dav_store(port, folder);
    s.check().unwrap();
    assert!(s.list("nothing/here").unwrap().is_empty());
    assert_eq!(s.read("missing.json").unwrap(), None);
    s.remove("missing.json").unwrap();

    // Parent folders are created on demand.
    s.write("games/fe/game.json", b"{\"v\":1}").unwrap();
    assert_eq!(s.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    let games = s.list("games").unwrap();
    assert_eq!(games.len(), 1);
    assert!(games[0].is_dir && games[0].name == "fe");
    let before = s.list("games/fe").unwrap();
    assert_eq!(before.len(), 1, "no temporary files are left behind: {before:?}");
    assert_eq!(before[0].size, 7);

    // Overwriting replaces the content and changes the tag.
    s.write("games/fe/game.json", b"{\"v\":22}").unwrap();
    let after = s.list("games/fe").unwrap();
    assert_eq!(after.len(), 1);
    assert_ne!(before[0].tag, after[0].tag);
    assert_eq!(s.read("games/fe/game.json").unwrap().as_deref(), Some(&b"{\"v\":22}"[..]));

    // Larger files stream both ways.
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let src = tmp.join("big.zip");
    write_file(&src, &big);
    s.upload("games/fe/snapshots/big.zip", &src).unwrap();
    let back = tmp.join("big-back.zip");
    s.download("games/fe/snapshots/big.zip", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big);
    assert!(s.download("games/fe/snapshots/nope.zip", &tmp.join("nope.zip")).is_err());

    s.remove("games/fe/snapshots/big.zip").unwrap();
    assert!(s.list("games/fe/snapshots").unwrap().is_empty());
}

fn layout_and_migration(port: u16, tmp: &Path) {
    // Source: a local-folder cloud with three snapshots, a device and a game.
    let local = Repo::new(ludonimbus_core::cloud::local::LocalFolder::new(tmp.join("local-cloud")));
    std::fs::create_dir_all(tmp.join("local-cloud")).unwrap();
    local
        .put_game(&CloudGame {
            key: "fe".into(),
            name: "火焰纹章".into(),
            adapter: Some("fe-fortunes-weave".into()),
            saves: vec![],
            exclude: vec![],
            wait_for: vec![],
            roots: BTreeMap::new(),
            ludusavi_title: None,
            source: None,
            ids: vec![],
            game: None,
        })
        .unwrap();
    local
        .put_device(&DeviceInfo {
            id: "d-a".into(),
            name: "PC-A".into(),
            os: "windows".into(),
            app_version: "test".into(),
            last_seen: chrono::Utc::now(),
            layout: 0,
            installs: Vec::new(),
            kind: None,
        })
        .unwrap();
    let ids = ["01AAA", "01BBB", "01CCC"];
    for (i, id) in ids.iter().enumerate() {
        let zip = tmp.join(format!("{id}.zip"));
        write_file(&zip, format!("zip-{id}").as_bytes());
        let parent = (i > 0).then(|| ids[i - 1]);
        local.upload(&meta("fe", id, parent, &format!("note {i}")), &zip).unwrap();
    }
    // A game whose definition never reached the cloud is copied all the same.
    let zip = tmp.join("orphan.zip");
    write_file(&zip, b"zip-orphan");
    local.upload(&meta("orphan", "01DDD", None, ""), &zip).unwrap();

    let dav = Repo::new(dav_store(port, "migrated"));
    let mut seen = Vec::new();
    let report = migrate::copy_all(local.store(), dav.store(), ("d-a", "PC-A"), |p| seen.push(p.done)).unwrap();
    assert_eq!((report.games, report.copied, report.skipped), (2, 4, 0));
    // Progress counts files: a zip and a manifest for each version.
    assert_eq!(seen.last(), Some(&8));

    let list = dav.list_snapshots("fe").unwrap();
    assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ids);
    assert_eq!(list[1].parent.as_deref(), Some("01AAA"));
    assert_eq!(list[2].comment, "note 2");
    assert_eq!(dav.list_snapshots("orphan").unwrap().len(), 1);
    let out = tmp.join("dl.zip");
    dav.download("fe", "01BBB", &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"zip-01BBB");
    assert_eq!(dav.get_meta("fe", "01CCC").unwrap().content_hash, "hash-01CCC");
    assert_eq!(dav.list_devices().unwrap().len(), 1);
    let games = dav.list_games().unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "火焰纹章");

    // Listing again is served from the metadata cache.
    let cache = std::path::PathBuf::from(std::env::var("LUDONIMBUS_HOME").unwrap()).join("cloud-cache");
    assert!(std::fs::read_dir(&cache).unwrap().next().is_some(), "metadata cache is used");
    assert_eq!(dav.list_snapshots("fe").unwrap(), list);

    // Running the copy again skips everything.
    let again = migrate::copy_all(local.store(), dav.store(), ("d-a", "PC-A"), |_| {}).unwrap();
    assert_eq!((again.copied, again.skipped), (0, 4));

    // Edited metadata is picked up even though older copies are cached.
    let mut m = dav.get_meta("fe", "01AAA").unwrap();
    m.comment = "通关前".into();
    m.locked = true;
    dav.update_meta(&m).unwrap();
    let edited = dav.list_snapshots("fe").unwrap();
    assert_eq!(edited[0].comment, "通关前");
    assert!(edited[0].locked);

    // Deleting removes the snapshot from the history.
    dav.delete("fe", "01BBB").unwrap();
    let ids_left: Vec<String> = dav.list_snapshots("fe").unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(ids_left, ["01AAA", "01CCC"]);

    // An unreachable server is reported as "offline", not as a hard error.
    let gone = cloud::build_store(
        &CloudConfig::WebDav { url: "http://127.0.0.1:9/".into(), username: "x".into(), folder: "x".into() },
        Some("x"),
        Duration::from_secs(3),
    )
    .unwrap();
    let err = Repo::new(gone).list_snapshots("fe").unwrap_err();
    assert!(cloud::is_unavailable(&err), "{err:#}");
}

#[test]
fn webdav_cloud() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let port = start_server();
    store_basics(port, tmp.path());
    layout_and_migration(port, tmp.path());
}
