//! Game backups end to end against a local folder and a real WebDAV server
//! (`dav-server`, in memory). Two parts in one test, because the data folder
//! is chosen through an environment variable:
//!
//! - a PC game backed up as a whole folder: backup, nothing-changed,
//!   incremental backup, restore, resuming both after a stop, keeping two
//!   versions, deleting and cleaning up, locks, damaged chunks, fixing paths,
//!   the cache of big files;
//! - two Wii U games sharing one Cemu: program, game files (with the update
//!   in Cemu's MLC), settings and firmware as separate components, the
//!   emulator stored once, a renamed big file not uploaded again, restoring
//!   to new folders, reusing an installed emulator, rewriting the launch
//!   options, cleaning up across games;
//! - a recompilation that its launcher builds: what it builds is left out,
//!   the game files are where its `.ini` says, which names their new place
//!   after a restore.

use ludonimbus_core::cloud::local::LocalFolder;
use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::config::{self, CloudConfig, DeviceConfig, GameEntry};
use ludonimbus_core::gamebackup::backup::{self, BackupOptions, Outcome};
use ludonimbus_core::gamebackup::chunker::Params;
use ludonimbus_core::gamebackup::format::{Kind, Summary};
use ludonimbus_core::gamebackup::restore::Targets;
use ludonimbus_core::gamebackup::{components, install, is_cancelled, remote, restore};
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

fn random(len: usize, seed: u64) -> Vec<u8> {
    let mut v = vec![0u8; len];
    rand::rngs::StdRng::seed_from_u64(seed).fill_bytes(&mut v);
    v
}

fn device(id: &str) -> DeviceConfig {
    DeviceConfig { id: id.into(), name: format!("PC-{id}") }
}

fn opts(keep: Option<usize>) -> BackupOptions {
    BackupOptions { threads: 3, params: Params::tiny(), keep, ..Default::default() }
}

fn run(store: &dyn Store, g: &GameEntry, o: &BackupOptions) -> Outcome {
    backup::backup(store, g, &device("a"), o, &AtomicBool::new(false), &|_| {}).unwrap()
}

fn created(o: Outcome) -> (Summary, usize, usize, u64) {
    match o {
        Outcome::Created { version, new_chunks, reused_chunks, uploaded, .. } => (version, new_chunks, reused_chunks, uploaded),
        Outcome::Unchanged { .. } => panic!("expected a new version"),
    }
}

fn to(program: &Path, content: Option<&Path>) -> Targets {
    Targets { program: program.to_path_buf(), restore_program: true, content: content.map(Path::to_path_buf) }
}

fn restore_to(store: &dyn Store, key: &str, s: &Summary, t: &Targets) -> restore::Report {
    restore::restore(store, key, s, t, 3, &AtomicBool::new(false), &|_| {}).unwrap().1
}

fn files_in(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = walkdir::WalkDir::new(p)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(p).unwrap().to_path_buf())
        .collect();
    v.sort();
    v
}

fn same_file(a: &Path, b: &Path) {
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap(), "{} vs {}", a.display(), b.display());
}

/// Every chunk in the cloud belongs to a committed version of some game,
/// and every version's chunks are there.
fn chunks_match_versions(store: &dyn Store) {
    let mut used = HashSet::new();
    for g in remote::list_games(store).unwrap() {
        for s in remote::list_versions(store, &g).unwrap() {
            for c in remote::read_manifest(store, &g, &s).unwrap().components {
                used.extend(c.chunks.into_iter().map(|c| c.h));
            }
        }
    }
    let present: HashSet<String> = remote::list_chunks(store).unwrap().into_keys().collect();
    assert_eq!(present, used);
}

// ---------------------------------------------------------------------------
// A PC game: the whole folder

fn make_pc_game(root: &Path) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    write(&root.join("game.exe"), &random(300_000, 1));
    write(&root.join("data/big.pak"), &random(1_500_000, 2));
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

fn pc_game(root: &Path) -> GameEntry {
    GameEntry {
        key: "zelda".into(),
        name: "Zelda".into(),
        exe: Some(root.join("game.exe")),
        saves: vec!["{install}/saves/**".into()],
        auto_sync: true,
        ..Default::default()
    }
}

/// Every backed-up file is restored byte for byte; saves and logs are not.
fn same_pc_tree(src: &Path, dst: &Path) {
    let want: Vec<PathBuf> = files_in(src).into_iter().filter(|r| !r.starts_with("saves") && !r.starts_with("logs")).collect();
    assert_eq!(files_in(dst), want);
    for r in &want {
        same_file(&src.join(r), &dst.join(r));
    }
    for d in ["mods", "saves", "logs"] {
        assert!(dst.join(d).is_dir(), "{d} recreated");
    }
}

fn pc_game_part(tmp: &Path, store: &dyn Store) {
    let root = tmp.join("games/Zelda");
    make_pc_game(&root);
    let g = pc_game(&root);

    // First backup: one "whole folder" component without saves and logs.
    let (v1, new1, reused1, _) = created(run(store, &g, &opts(None)));
    assert_eq!(v1.components.len(), 1);
    assert_eq!(v1.components[0].kind, Kind::Folder);
    assert_eq!(v1.file_count, 1 + 1 + 200 + 1);
    assert!(new1 > 50 && reused1 == 0, "{new1} new, {reused1} reused");
    let m1 = remote::read_manifest(store, "zelda", &v1).unwrap();
    let folder = &m1.components[0];
    assert!(folder.files.iter().all(|f| f.p.starts_with("{program}/") && !f.p.contains("/saves/") && !f.p.contains("/logs/")));
    assert_eq!(folder.exe.as_deref(), Some("{program}/game.exe"));
    assert!(folder.dirs.contains(&"{program}/mods".to_string()) && folder.dirs.contains(&"{program}/saves".to_string()));
    assert!(remote::lock_holder(store, "zelda").unwrap().is_none(), "lock released");

    // Nothing changed.
    assert!(matches!(run(store, &g, &opts(None)), Outcome::Unchanged { .. }));

    // An update: 100 bytes changed inside the archive, one new file.
    let mut big = std::fs::read(root.join("data/big.pak")).unwrap();
    for b in &mut big[700_000..700_100] {
        *b ^= 0x5a;
    }
    write(&root.join("data/big.pak"), &big);
    write(&root.join("data/patch.txt"), b"version 1.1");
    let (v2, new2, reused2, _) = created(run(store, &g, &opts(None)));
    assert!(new2 <= 6 && reused2 >= new1 - 6, "update uploaded {new2} chunks, reused {reused2}");

    // Restore the newest version into a new folder.
    let r1 = tmp.join("restored1");
    let rep = restore_to(store, "zelda", &v2, &to(&r1, None));
    assert_eq!(rep.files, v2.file_count);
    same_pc_tree(&root, &r1);
    let mt = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(mt(&r1.join("data/patch.txt")), mt(&root.join("data/patch.txt")), "modification time kept");
    // A backup of the restored folder finds nothing new.
    let mut from_restore = g.clone();
    from_restore.exe = Some(r1.join("game.exe"));
    assert!(matches!(run(store, &from_restore, &opts(None)), Outcome::Unchanged { .. }));
    // Touched but not changed: no new version.
    let same = std::fs::read(root.join("data/patch.txt")).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    write(&root.join("data/patch.txt"), &same);
    assert!(matches!(run(store, &g, &opts(None)), Outcome::Unchanged { .. }));
    assert_eq!(remote::list_versions(store, "zelda").unwrap().len(), 2);
    // Restoring again over a complete copy writes nothing.
    let again = restore_to(store, "zelda", &v2, &to(&r1, None));
    assert_eq!((again.files, again.kept), (0, v2.file_count));

    // A stopped restore continues where it stopped.
    let r2 = tmp.join("restored2");
    let cancel = AtomicBool::new(false);
    let err = restore::restore(store, "zelda", &v2, &to(&r2, None), 2, &cancel, &|p| {
        if p.phase == "downloading" && p.done > 600_000 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(is_cancelled(&err), "{err:#}");
    assert_eq!(restore::pending("zelda").map(|(v, _)| v), Some(v2.id.clone()));
    let rep = restore_to(store, "zelda", &v2, &to(&r2, None));
    assert!(rep.resumed_chunks > 0, "{rep:?}");
    assert!(restore::pending("zelda").is_none());
    same_pc_tree(&root, &r2);

    // A stopped backup keeps what it uploaded and only uploads the rest.
    write(&root.join("data/dlc.pak"), &random(800_000, 9));
    let before = remote::list_chunks(store).unwrap().len();
    let cancel = AtomicBool::new(false);
    let err = backup::backup(store, &g, &device("a"), &opts(None), &cancel, &|p| {
        if p.new_chunks >= 5 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(is_cancelled(&err), "{err:#}");
    assert!(remote::lock_holder(store, "zelda").unwrap().is_none(), "lock released after a stop");
    let partial = remote::list_chunks(store).unwrap().len();
    assert!(partial >= before + 5);
    assert_eq!(remote::list_versions(store, "zelda").unwrap().len(), 2, "nothing committed");
    let (v3, new3, _, _) = created(run(store, &g, &opts(None)));
    let after = remote::list_chunks(store).unwrap().len();
    assert_eq!(new3, after - partial, "only the missing chunks were uploaded");

    // A big file (8 MB or more) is chunked on its own and remembered: when
    // its size and time are unchanged it is not read again (the cached hash
    // is used even though the bytes were swapped behind its back).
    let big_path = root.join("data/movie.bik");
    write(&big_path, &random(9 * 1024 * 1024, 11));
    let (v4, _, _, _) = created(run(store, &g, &opts(None)));
    let hash_of = |s: &Summary| {
        let m = remote::read_manifest(store, "zelda", s).unwrap();
        m.components[0].files.iter().find(|f| f.p.ends_with("movie.bik")).unwrap().h.clone()
    };
    let t = std::fs::metadata(&big_path).unwrap().modified().unwrap();
    let mut swapped = random(9 * 1024 * 1024, 12);
    swapped.truncate(9 * 1024 * 1024);
    write(&big_path, &swapped);
    std::fs::OpenOptions::new().write(true).open(&big_path).unwrap().set_modified(t).unwrap();
    write(&root.join("data/patch.txt"), b"version 1.2");
    let (v5, _, _, _) = created(run(store, &g, &opts(None)));
    assert_eq!(hash_of(&v5), hash_of(&v4), "the unchanged-looking big file was not read");
    write(&big_path, &random(9 * 1024 * 1024, 11));
    std::fs::OpenOptions::new().write(true).open(&big_path).unwrap().set_modified(t).unwrap();

    // Keep 2: older versions and the chunks only they used go away.
    write(&root.join("data/patch.txt"), b"version 1.3");
    let Outcome::Created { version: v6, pruned, .. } = run(store, &g, &opts(Some(2))) else { panic!() };
    assert_eq!(pruned, vec![v1.id.clone(), v2.id.clone(), v3.id.clone(), v4.id.clone()]);
    let ids: Vec<String> = remote::list_versions(store, "zelda").unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec![v5.id.clone(), v6.id.clone()]);
    chunks_match_versions(store);

    // Deleting a version, then cleaning up.
    remote::delete_version(store, "zelda", &v5.id).unwrap();
    remote::collect_garbage(store, ("d-a", "PC-a"), None, &|_, _| {}).unwrap();
    chunks_match_versions(store);
    assert_eq!(remote::list_versions(store, "zelda").unwrap().len(), 1);

    // Another device holds the lock.
    write(&root.join("data/patch.txt"), b"version 1.4");
    let other = remote::Lock::acquire(store, "zelda", "d-b", "LAPTOP", "backup").unwrap();
    let err = backup::backup(store, &g, &device("a"), &opts(None), &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(format!("{err:#}").contains("LAPTOP"), "{err:#}");
    // ...and cleaning up waits while it backs up.
    assert!(remote::collect_garbage(store, ("d-a", "PC-a"), None, &|_, _| {}).unwrap().postponed);
    other.release().unwrap();
    let (v7, _, _, _) = created(run(store, &g, &opts(None)));

    // A damaged chunk is caught when restoring.
    let m7 = remote::read_manifest(store, "zelda", &v7).unwrap();
    let victim_id = m7.components[0].chunks[0].h.clone();
    let cloud = tmp.join("cloud");
    let victim = cloud.join(remote::chunk_path(&victim_id).replace('/', "\\"));
    let good = std::fs::read(&victim).unwrap();
    let mut bad = good.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0xff;
    std::fs::write(&victim, bad).unwrap();
    let err = restore::restore(store, "zelda", &v7, &to(&tmp.join("restored4"), None), 2, &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(format!("{err:#}").contains(&victim_id), "{err:#}");
    std::fs::write(&victim, good).unwrap();

    // After a restore, the game points at the new folder.
    config::update(|c| {
        c.cloud = CloudConfig::LocalFolder { path: cloud.clone() };
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();
    let fix = install::fix_paths("zelda", &m1, &to(&r1, None)).unwrap();
    assert_eq!(fix.shortcut, install::ShortcutFix::NotLinked);
    assert_eq!(config::load().unwrap().game("zelda").unwrap().exe.as_deref(), Some(r1.join("game.exe").as_path()));
}

// ---------------------------------------------------------------------------
// Two Wii U games sharing one Cemu

/// An unpacked Wii U game: code, content, meta/meta.xml.
fn make_wiiu_game(dir: &Path, title: &str, version: u32, seed: u64) {
    write(&dir.join("code/game.rpx"), &random(200_000, seed));
    write(&dir.join("content/a.bin"), &random(300_000, seed + 1));
    write(&dir.join("content/big.bin"), &random(9 * 1024 * 1024, seed + 2));
    write(
        &dir.join("meta/meta.xml"),
        format!("<?xml version=\"1.0\"?><menu><title_id type=\"hexBinary\" length=\"8\">{title}</title_id><title_version type=\"unsignedInt\" length=\"4\">{version}</title_version></menu>").as_bytes(),
    );
}

fn make_cemu(emu: &Path) {
    write(&emu.join("Cemu.exe"), &random(400_000, 30));
    write(&emu.join("graphicPacks/pack/rules.txt"), b"[Definition]");
    write(&emu.join("settings.xml"), b"<content></content>");
    write(&emu.join("keys.txt"), b"secret keys");
    write(&emu.join("shaderCache/transferable/x.bin"), b"cache");
    write(&emu.join("games/Other.wua"), &random(50_000, 31));
    let mlc = emu.join("portable/mlc01");
    // Updates of the two games and of a third one; a save.
    write(&mlc.join("usr/title/0005000e/10101c00/content/up.bin"), &random(100_000, 32));
    write(&mlc.join("usr/title/0005000e/1010ec00/content/up.bin"), &random(100_000, 33));
    write(&mlc.join("usr/title/0005000e/aaaaaaaa/content/up.bin"), &random(100_000, 34));
    write(&mlc.join("usr/save/00050000/10101c00/user/80000001/save.dat"), b"save");
    write(&emu.join("gameProfiles/0005000010101c00.ini"), b"[Graphics]\naccurateShaderMul = true");
    write(&emu.join("gameProfiles/default/x.ini"), b"default profile");
}

fn cemu_game(key: &str, emu: &Path, rpx: &Path) -> GameEntry {
    GameEntry {
        key: key.into(),
        name: key.to_uppercase(),
        adapter: Some("cemu".into()),
        exe: Some(emu.join("Cemu.exe")),
        args: format!("-g \"{}\"", rpx.display()),
        auto_sync: true,
        ..Default::default()
    }
}

fn emulator_part(tmp: &Path, store: &dyn Store) {
    let emu = tmp.join("emu/Cemu 2.6");
    make_cemu(&emu);
    let roms = tmp.join("roms/WiiU");
    make_wiiu_game(&roms.join("Zelda"), "0005000010101C00", 208, 40);
    make_wiiu_game(&roms.join("Kart"), "000500001010EC00", 64, 50);
    let botw = cemu_game("botw", &emu, &roms.join("Zelda/code/game.rpx"));
    let mk8 = cemu_game("mk8", &emu, &roms.join("Kart/code/game.rpx"));

    // The parts, before backing up.
    let plan = components::plan(&botw, &components::Options::default()).unwrap();
    let kinds: Vec<Kind> = plan.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, vec![Kind::Program, Kind::Content, Kind::Config], "firmware only when ticked");
    let content = plan.iter().find(|c| c.kind == Kind::Content).unwrap();
    let logical: Vec<&str> = content.files.iter().map(|f| f.logical.as_str()).collect();
    assert_eq!(
        logical,
        vec![
            "{content}/Zelda/code/game.rpx",
            "{content}/Zelda/content/a.bin",
            "{content}/Zelda/content/big.bin",
            "{content}/Zelda/meta/meta.xml",
            "{program}/portable/mlc01/usr/title/0005000e/10101c00/content/up.bin",
        ]
    );
    assert_eq!(content.version.as_deref(), Some("v208"));
    let program = plan.iter().find(|c| c.kind == Kind::Program).unwrap();
    assert_eq!(program.name, "Cemu");
    assert_eq!(program.version.as_deref(), Some("2.6"));
    let prog: Vec<&str> = program.files.iter().map(|f| f.logical.as_str()).collect();
    assert_eq!(
        prog,
        vec!["{program}/Cemu.exe", "{program}/gameProfiles/default/x.ini", "{program}/graphicPacks/pack/rules.txt", "{program}/settings.xml"],
        "no saves, other games' files, ROMs, caches, keys or this game's settings"
    );
    let config = plan.iter().find(|c| c.kind == Kind::Config).unwrap();
    assert_eq!(config.files.iter().map(|f| f.logical.as_str()).collect::<Vec<_>>(), vec!["{program}/gameProfiles/0005000010101c00.ini"]);
    let with_fw = components::plan(&botw, &components::Options { firmware: true, ..Default::default() }).unwrap();
    let fw = with_fw.iter().find(|c| c.kind == Kind::Firmware).unwrap();
    assert_eq!(fw.files.iter().map(|f| f.logical.as_str()).collect::<Vec<_>>(), vec!["{program}/keys.txt"]);

    // Back up both games: the emulator is stored once.
    let (b1, _, _, _) = created(run(store, &botw, &opts(Some(2))));
    let chunks_after_botw = remote::list_chunks(store).unwrap().len();
    let (k1, _, _, _) = created(run(store, &mk8, &opts(Some(2))));
    let mb = remote::read_manifest(store, "botw", &b1).unwrap();
    let mk = remote::read_manifest(store, "mk8", &k1).unwrap();
    assert_eq!(mb.component(Kind::Program).unwrap().chunks, mk.component(Kind::Program).unwrap().chunks);
    let own: HashSet<&String> = mk
        .components
        .iter()
        .filter(|c| c.kind != Kind::Program)
        .flat_map(|c| c.chunks.iter().map(|c| &c.h))
        .collect();
    assert_eq!(remote::list_chunks(store).unwrap().len(), chunks_after_botw + own.len(), "mk8 added only its own chunks");
    assert_eq!(b1.components.iter().map(|c| c.version.clone()).collect::<Vec<_>>(), vec![Some("2.6".into()), Some("v208".into()), None]);

    // A renamed big file is not uploaded again.
    std::fs::rename(roms.join("Zelda/content/big.bin"), roms.join("Zelda/content/big2.bin")).unwrap();
    let (b2, new, _, uploaded) = created(run(store, &botw, &opts(Some(2))));
    assert_eq!((new, uploaded), (0, 0), "only the manifest changed");

    // A new PC: everything to new folders.
    let new_emu = tmp.join("new/Cemu 2.6");
    let new_roms = tmp.join("new/ROMs");
    let rep = restore_to(store, "botw", &b2, &to(&new_emu, Some(&new_roms)));
    assert_eq!(rep.kept, 0);
    same_file(&emu.join("Cemu.exe"), &new_emu.join("Cemu.exe"));
    same_file(&roms.join("Zelda/content/big2.bin"), &new_roms.join("Zelda/content/big2.bin"));
    same_file(
        &emu.join("portable/mlc01/usr/title/0005000e/10101c00/content/up.bin"),
        &new_emu.join("portable/mlc01/usr/title/0005000e/10101c00/content/up.bin"),
    );
    same_file(&emu.join("gameProfiles/0005000010101c00.ini"), &new_emu.join("gameProfiles/0005000010101c00.ini"));
    // Left out (their folders come back empty, like in the original).
    for f in ["keys.txt", "games/Other.wua", "shaderCache/transferable/x.bin", "portable/mlc01/usr/save/00050000/10101c00/user/80000001/save.dat", "portable/mlc01/usr/title/0005000e/aaaaaaaa/content/up.bin"] {
        assert!(!new_emu.join(f).exists(), "{f}");
    }

    // The second game on that PC: the emulator is already there and is kept.
    let rep = restore_to(store, "mk8", &k1, &to(&new_emu, Some(&new_roms)));
    assert_eq!(rep.kept, mk.component(Kind::Program).unwrap().files.len());
    same_file(&roms.join("Kart/content/big.bin"), &new_roms.join("Kart/content/big.bin"));
    // Keeping a different program that is installed: only the game's own parts.
    let other_emu = tmp.join("other/Cemu");
    write(&other_emu.join("Cemu.exe"), b"another version");
    let rep = restore_to(store, "mk8", &k1, &Targets { program: other_emu.clone(), restore_program: false, content: Some(tmp.join("other/ROMs")) });
    assert_eq!(std::fs::read(other_emu.join("Cemu.exe")).unwrap(), b"another version");
    assert!(other_emu.join("portable/mlc01/usr/title/0005000e/1010ec00/content/up.bin").is_file());
    assert!(rep.files > 0);

    // The game now starts the restored emulator with the restored disc.
    config::update(|c| {
        c.games.push(botw.clone());
        Ok(())
    })
    .unwrap();
    let mb2 = remote::read_manifest(store, "botw", &b2).unwrap();
    let fix = install::fix_paths("botw", &mb2, &to(&new_emu, Some(&new_roms))).unwrap();
    assert!(fix.args_changed);
    let g = config::load().unwrap().game("botw").unwrap().clone();
    assert_eq!(g.exe.as_deref(), Some(new_emu.join("Cemu.exe").as_path()));
    let unify = |s: String| s.replace('/', "\\").to_lowercase();
    assert!(unify(g.args.clone()).contains(&unify(new_roms.join("Zelda").join("code").display().to_string())), "{}", g.args);
    assert_eq!(ludonimbus_core::gamebackup::content_dir_for("cemu"), Some(new_roms.clone()));

    // Deleting every version of one game keeps the chunks the other uses.
    for s in remote::list_versions(store, "botw").unwrap() {
        remote::delete_version(store, "botw", &s.id).unwrap();
    }
    remote::collect_garbage(store, ("d-a", "PC-a"), None, &|_, _| {}).unwrap();
    chunks_match_versions(store);
    restore_to(store, "mk8", &k1, &to(&tmp.join("third/Cemu"), Some(&tmp.join("third/ROMs"))));
    same_file(&emu.join("Cemu.exe"), &tmp.join("third/Cemu/Cemu.exe"));
}

// ---------------------------------------------------------------------------
// A recompilation that its launcher builds from the game files, which are
// where its settings file says

fn settings_file_part(tmp: &Path, store: &dyn Store) {
    let prog = tmp.join("recomp/fefw-recomp-0.2.7");
    let romfs = tmp.join("dumps/romfs");
    write(&prog.join("fefw-launcher.exe"), &random(100_000, 30));
    // Built from the game: the program, an unpacked copy of exefs, the build state.
    write(&prog.join("fefw.exe"), &random(200_000, 31));
    write(&prog.join("exefs/main"), &random(60_000, 32));
    write(&prog.join("build/state.json"), b"{}");
    write(&prog.join("save/slot.sav"), b"progress");
    write(&tmp.join("dumps/exefs/main"), &random(50_000, 34));
    write(&romfs.join("data.bin"), &random(400_000, 33));
    write(&romfs.join("sub/a.txt"), b"text");
    let ini = format!("[fs]\nexefs = {}\nromfs = {}\nfullscreen = 1\n", prog.join("exefs").display(), romfs.display());
    write(&prog.join("fefw.ini"), ini.as_bytes());
    let g = GameEntry {
        key: "fefw".into(),
        name: "FE".into(),
        exe: Some(prog.join("fefw.exe")),
        adapter: Some("fe-fortunes-weave".into()),
        saves: vec!["{install}/save/**".into()],
        auto_sync: true,
        ..Default::default()
    };
    config::update(|c| {
        c.games.push(g.clone());
        Ok(())
    })
    .unwrap();

    let (v, _, _, _) = created(run(store, &g, &opts(None)));
    let m = remote::read_manifest(store, "fefw", &v).unwrap();
    let paths = |k: Kind| m.component(k).map(|c| c.files.iter().map(|f| f.p.clone()).collect::<Vec<_>>()).unwrap_or_default();
    assert_eq!(paths(Kind::Content), ["{content}/exefs/main", "{content}/romfs/data.bin", "{content}/romfs/sub/a.txt"]);
    assert_eq!(paths(Kind::Config), ["{program}/fefw.ini"]);
    assert_eq!(paths(Kind::Program), ["{program}/fefw-launcher.exe"]);
    let program = m.component(Kind::Program).unwrap();
    assert_eq!(program.build.as_deref(), Some("{program}/fefw-launcher.exe"));
    assert_eq!(program.exe.as_deref(), Some("{program}/fefw.exe"));

    // Restored elsewhere: the ini names the new places (the old one is kept),
    // the game points at where the launcher will build it.
    let (new_prog, new_dumps) = (tmp.join("new/fefw"), tmp.join("new/dumps"));
    restore_to(store, "fefw", &v, &to(&new_prog, Some(&new_dumps)));
    same_file(&romfs.join("data.bin"), &new_dumps.join("romfs/data.bin"));
    same_file(&tmp.join("dumps/exefs/main"), &new_dumps.join("exefs/main"));
    assert!(!new_prog.join("fefw.exe").exists() && !new_prog.join("exefs").exists());
    let fix = install::fix_paths("fefw", &m, &to(&new_prog, Some(&new_dumps))).unwrap();
    assert_eq!(fix.settings.len(), 1, "{:?}", fix.settings);
    let unify = |s: String| s.replace('/', "\\").to_lowercase();
    assert_eq!(unify(fix.build.clone().unwrap()), unify(new_prog.join("fefw-launcher.exe").display().to_string()));
    let exe = config::load().unwrap().game("fefw").unwrap().exe.clone().unwrap();
    assert_eq!(unify(exe.display().to_string()), unify(new_prog.join("fefw.exe").display().to_string()));
    let text = std::fs::read_to_string(new_prog.join("fefw.ini")).unwrap();
    let want = format!("[fs]\nexefs = {}\nromfs = {}\nfullscreen = 1\n", new_prog.join("exefs").display(), new_dumps.join("romfs").display());
    assert_eq!(unify(text), unify(want));
    assert_eq!(std::fs::read_to_string(new_prog.join("fefw.ini.ludonimbus-bak")).unwrap(), ini);
    // Nothing more to change the second time.
    assert!(install::fix_paths("fefw", &m, &to(&new_prog, Some(&new_dumps))).unwrap().settings.is_empty());
}

#[test]
fn game_backup_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    std::fs::create_dir_all(tmp.path().join("cloud")).unwrap();
    let store = LocalFolder::new(tmp.path().join("cloud"));
    pc_game_part(tmp.path(), &store);
    emulator_part(tmp.path(), &store);
    settings_file_part(tmp.path(), &store);

    // The same against WebDAV.
    let port = start_dav();
    let dav = ludonimbus_core::cloud::build_store(
        &CloudConfig::WebDav { url: format!("http://127.0.0.1:{port}/"), username: "u".into(), folder: "Ludonimbus".into() },
        Some("p"),
        Duration::from_secs(10),
    )
    .unwrap();
    let root = tmp.path().join("games/Zelda");
    let g = pc_game(&root);
    let (dv, _, _, _) = created(run(dav.as_ref(), &g, &opts(Some(2))));
    assert!(matches!(run(dav.as_ref(), &g, &opts(Some(2))), Outcome::Unchanged { .. }));
    let r = tmp.path().join("restored-dav");
    restore_to(dav.as_ref(), "zelda", &dv, &to(&r, None));
    same_pc_tree(&root, &r);
    chunks_match_versions(dav.as_ref());
}
