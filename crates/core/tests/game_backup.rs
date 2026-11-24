//! Game backups end to end against a local folder and a real WebDAV server
//! (`dav-server`, in memory): backup, nothing-changed, incremental backup,
//! restore, resuming both after a stop, keeping two versions, deleting and
//! cleaning up, the lock, damaged chunks, and fixing the game's paths.
//! One test, because the data folder is chosen through an environment variable.

use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::config::{self, CloudConfig, DeviceConfig, GameEntry};
use ludonimbus_core::gamebackup::backup::{self, BackupOptions, Outcome};
use ludonimbus_core::gamebackup::chunker::Params;
use ludonimbus_core::gamebackup::{install, is_cancelled, remote, restore};
use rand::{Rng, RngCore, SeedableRng};
use std::collections::HashSet;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn start_dav() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let handler = dav_server::DavHandler::builder()
                .filesystem(dav_server::memfs::MemFs::new())
                .locksystem(dav_server::fakels::FakeLs::new())
                .build_handler();
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
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    });
    rx.recv().unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn random(len: usize, rng: &mut rand::rngs::StdRng) -> Vec<u8> {
    let mut v = vec![0u8; len];
    rng.fill_bytes(&mut v);
    v
}

/// A game folder: a program, a big archive, many small text files, an
/// empty file, an empty folder, saves and a log.
fn make_game(root: &Path) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    write(&root.join("game.exe"), &random(300_000, &mut rng));
    write(&root.join("data/big.pak"), &random(1_500_000, &mut rng));
    for i in 0..200 {
        let n = rng.gen_range(100..2000);
        let text: String = (0..n).map(|_| (b'a' + rng.gen_range(0..26)) as char).collect();
        write(&root.join(format!("data/small/file{i:03}.txt")), text.as_bytes());
    }
    write(&root.join("empty.cfg"), b"");
    std::fs::create_dir_all(root.join("mods")).unwrap();
    write(&root.join("saves/slot1.sav"), b"my progress");
    write(&root.join("logs/run.log"), b"log line");
}

fn game(root: &Path) -> GameEntry {
    GameEntry {
        key: "zelda".into(),
        name: "Zelda".into(),
        exe: Some(root.join("game.exe")),
        saves: vec!["{install}/saves/**".into()],
        auto_sync: true,
        ..Default::default()
    }
}

fn device(id: &str) -> DeviceConfig {
    DeviceConfig { id: id.into(), name: format!("PC-{id}") }
}

fn opts(root: &Path, keep: Option<usize>) -> BackupOptions {
    BackupOptions {
        root: root.to_path_buf(),
        user_exclude: Vec::new(),
        threads: 3,
        params: Params::tiny(),
        keep,
        comment: String::new(),
    }
}

fn run(store: &dyn Store, g: &GameEntry, o: &BackupOptions) -> Outcome {
    backup::backup(store, g, &device("a"), o, &AtomicBool::new(false), &|_| {}).unwrap()
}

fn created(o: Outcome) -> (ludonimbus_core::gamebackup::format::Summary, usize, usize) {
    match o {
        Outcome::Created { version, new_chunks, reused_chunks, .. } => (version, new_chunks, reused_chunks),
        Outcome::Unchanged { .. } => panic!("expected a new version"),
    }
}

fn restore_to(store: &dyn Store, s: &ludonimbus_core::gamebackup::format::Summary, target: &Path) -> restore::Report {
    restore::restore(store, "zelda", s, target, 3, &AtomicBool::new(false), &|_| {}).unwrap().1
}

/// Every backed-up file is restored byte for byte; saves and logs are not.
fn same_tree(src: &Path, dst: &Path) {
    let m = |p: &Path| -> Vec<PathBuf> {
        walkdir::WalkDir::new(p)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().strip_prefix(p).unwrap().to_path_buf())
            .collect()
    };
    let mut want: Vec<PathBuf> = m(src).into_iter().filter(|r| !r.starts_with("saves") && !r.starts_with("logs")).collect();
    let mut got = m(dst);
    want.sort();
    got.sort();
    assert_eq!(got, want);
    for r in &want {
        assert_eq!(std::fs::read(src.join(r)).unwrap(), std::fs::read(dst.join(r)).unwrap(), "{}", r.display());
    }
    for d in ["mods", "saves", "logs"] {
        assert!(dst.join(d).is_dir(), "{d} recreated");
    }
    assert!(!dst.join(restore::JOURNAL).exists());
}

/// Every chunk in the cloud belongs to a committed version, and every
/// version's chunks are there.
fn chunks_match_versions(store: &dyn Store) {
    let mut used = HashSet::new();
    for s in remote::list_versions(store, "zelda").unwrap() {
        used.extend(remote::read_manifest(store, "zelda", &s).unwrap().chunks.into_iter().map(|c| c.h));
    }
    let present: HashSet<String> = remote::list_chunks(store, "zelda").unwrap().into_keys().collect();
    assert_eq!(present, used);
}

#[test]
fn game_backup_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let root = tmp.path().join("games/Zelda");
    make_game(&root);
    let g = game(&root);
    std::fs::create_dir_all(tmp.path().join("cloud")).unwrap();
    let store = LocalFolder::new(tmp.path().join("cloud"));

    // First backup: everything but saves and logs.
    let (v1, new1, reused1) = created(run(&store, &g, &opts(&root, None)));
    assert_eq!(v1.file_count, 1 + 1 + 200 + 1);
    assert!(new1 > 50 && reused1 == 0, "{new1} new, {reused1} reused");
    let m1 = remote::read_manifest(&store, "zelda", &v1).unwrap();
    assert!(m1.files.iter().all(|f| !f.p.starts_with("saves") && !f.p.starts_with("logs")));
    assert_eq!(m1.exe.as_deref(), Some("game.exe"));
    assert!(m1.dirs.contains(&"mods".to_string()) && m1.dirs.contains(&"saves".to_string()));
    assert_eq!(remote::lock_holder(&store, "zelda").unwrap().map(|l| l.device_id), None, "lock released");

    // Nothing changed.
    assert!(matches!(run(&store, &g, &opts(&root, None)), Outcome::Unchanged { .. }));

    // An update: 100 bytes changed inside the big archive, one new file.
    let mut big = std::fs::read(root.join("data/big.pak")).unwrap();
    for b in &mut big[700_000..700_100] {
        *b ^= 0x5a;
    }
    write(&root.join("data/big.pak"), &big);
    write(&root.join("data/patch.txt"), b"version 1.1");
    let (v2, new2, reused2) = created(run(&store, &g, &opts(&root, None)));
    assert!(new2 <= 6 && reused2 >= new1 - 6, "update uploaded {new2} chunks, reused {reused2}");

    // Restore the newest version into a new folder.
    let r1 = tmp.path().join("restored1");
    let rep = restore_to(&store, &v2, &r1);
    assert_eq!(rep.files, v2.file_count);
    same_tree(&root, &r1);
    let mt = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    let secs = |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs(mt(&r1.join("data/patch.txt"))), secs(mt(&root.join("data/patch.txt"))));

    // A stopped restore continues where it stopped.
    let r2 = tmp.path().join("restored2");
    let cancel = AtomicBool::new(false);
    let err = restore::restore(&store, "zelda", &v2, &r2, 2, &cancel, &|p| {
        if p.phase == "downloading" && p.done > 600_000 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(is_cancelled(&err), "{err:#}");
    assert_eq!(restore::journal_version(&r2).as_deref(), Some(v2.id.as_str()));
    assert_eq!(restore::resolve_target(&r2, &v2.root, &v2.id), r2);
    let rep = restore_to(&store, &v2, &r2);
    assert!(rep.resumed_chunks > 0, "{rep:?}");
    same_tree(&root, &r2);

    // A stopped backup keeps what it uploaded and only uploads the rest.
    let mut rng = rand::rngs::StdRng::seed_from_u64(9);
    write(&root.join("data/dlc.pak"), &random(800_000, &mut rng));
    let before = remote::list_chunks(&store, "zelda").unwrap().len();
    let cancel = AtomicBool::new(false);
    let err = backup::backup(&store, &g, &device("a"), &opts(&root, None), &cancel, &|p| {
        if p.new_chunks >= 5 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(is_cancelled(&err), "{err:#}");
    assert!(remote::lock_holder(&store, "zelda").unwrap().is_none(), "lock released after a stop");
    let partial = remote::list_chunks(&store, "zelda").unwrap().len();
    assert!(partial >= before + 5);
    assert_eq!(remote::list_versions(&store, "zelda").unwrap().len(), 2, "nothing committed");
    let (v3, new3, _) = created(run(&store, &g, &opts(&root, None)));
    let after = remote::list_chunks(&store, "zelda").unwrap().len();
    assert_eq!(new3, after - partial, "only the missing chunks were uploaded");

    // Keep 2: the oldest version and the chunks only it used go away.
    write(&root.join("data/patch.txt"), b"version 1.2");
    let outcome = run(&store, &g, &opts(&root, Some(2)));
    let Outcome::Created { version: v4, pruned, .. } = outcome else { panic!() };
    assert_eq!(pruned, vec![v1.id.clone(), v2.id.clone()]);
    let ids: Vec<String> = remote::list_versions(&store, "zelda").unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec![v3.id.clone(), v4.id.clone()]);
    chunks_match_versions(&store);
    let r3 = tmp.path().join("restored3");
    restore_to(&store, &v3, &r3);
    assert_eq!(std::fs::read(r3.join("data/patch.txt")).unwrap(), b"version 1.1");

    // Deleting a version, then cleaning up.
    remote::delete_version(&store, "zelda", &v3.id).unwrap();
    remote::collect_garbage(&store, "zelda", &|_, _| {}).unwrap();
    chunks_match_versions(&store);
    assert_eq!(remote::list_versions(&store, "zelda").unwrap().len(), 1);

    // Another device holds the lock.
    write(&root.join("data/patch.txt"), b"version 1.3");
    let other = remote::Lock::acquire(&store, "zelda", "d-b", "LAPTOP", "backup").unwrap();
    let err = backup::backup(&store, &g, &device("a"), &opts(&root, None), &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(format!("{err:#}").contains("LAPTOP"), "{err:#}");
    other.release().unwrap();
    let (v5, _, _) = created(run(&store, &g, &opts(&root, None)));

    // A damaged chunk is caught when restoring.
    let m5 = remote::read_manifest(&store, "zelda", &v5).unwrap();
    let victim = tmp.path().join("cloud").join(remote::chunk_path("zelda", &m5.chunks[0].h).replace('/', "\\"));
    let mut obj = std::fs::read(&victim).unwrap();
    let last = obj.len() - 1;
    obj[last] ^= 0xff;
    std::fs::write(&victim, obj).unwrap();
    let err = restore::restore(&store, "zelda", &v5, &tmp.path().join("restored4"), 2, &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(format!("{err:#}").contains(&m5.chunks[0].h), "{err:#}");

    // After a restore, the game points at the new folder.
    config::update(|c| {
        c.cloud = CloudConfig::LocalFolder { path: tmp.path().join("cloud") };
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();
    let fix = install::fix_paths("zelda", &m1, &r1).unwrap();
    assert_eq!(fix.shortcut, install::ShortcutFix::NotLinked);
    let cfg = config::load().unwrap();
    assert_eq!(cfg.game("zelda").unwrap().exe.as_deref(), Some(r1.join("game.exe").as_path()));

    // The same against WebDAV.
    let port = start_dav();
    let dav = ludonimbus_core::cloud::build_store(
        &CloudConfig::WebDav { url: format!("http://127.0.0.1:{port}/"), username: "u".into(), folder: "Ludonimbus".into() },
        Some("p"),
        Duration::from_secs(10),
    )
    .unwrap();
    let (dv, _, _) = created(run(dav.as_ref(), &g, &opts(&root, Some(2))));
    assert!(matches!(run(dav.as_ref(), &g, &opts(&root, Some(2))), Outcome::Unchanged { .. }));
    let r5 = tmp.path().join("restored-dav");
    restore_to(dav.as_ref(), &dv, &r5);
    same_tree(&root, &r5);
    chunks_match_versions(dav.as_ref());
}
