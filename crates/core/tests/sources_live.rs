//! Check E1 of docs/plan-game-backup.md, section 10: what Quark (through the
//! OpenList server Ludonimbus runs) and Baidu Netdisk let Ludonimbus do with
//! files that are already in the drive, outside its own folder:
//!
//! - read folders elsewhere in the drive, and which hash each file comes with;
//! - copy a file inside the drive (on the server) into the Ludonimbus folder;
//! - (Baidu) whether writing outside the app folder is refused.
//!
//! Ignored by default. `scripts/cloud-sources-e1.ps1` runs it with your real
//! Ludonimbus settings and sign-ins:
//!
//! ```text
//! LUDONIMBUS_E1_QUARK=1 [LUDONIMBUS_E1_QUARK_FOLDER=/游戏] [LUDONIMBUS_E1_QUARK_FILE=/游戏/big.iso]
//! LUDONIMBUS_E1_BAIDU=1 [LUDONIMBUS_E1_BAIDU_FOLDER=/游戏] [LUDONIMBUS_E1_BAIDU_FILE=/游戏/small.bin]
//!   cargo test -p ludonimbus-core --test sources_live -- --ignored --nocapture
//! ```
//!
//! Your own folders and files are only read (and one file copied); the test
//! writes only to folders named `Ludonimbus-E1-…` / `e1-copy-…`, which it
//! deletes at the end, also when a step fails.

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::cloud::baidu::{self, BaiduClient, BaiduPan, Endpoints, TokenSink};
use ludonimbus_core::cloud::{self, store::Store};
use ludonimbus_core::config::{self, CloudConfig};
use ludonimbus_core::openlist;
use rand::RngCore;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

struct Log(Instant);

impl Log {
    fn step(&self, what: impl AsRef<str>) {
        println!("[E1 {:>6.1}s] {}", self.0.elapsed().as_secs_f64(), what.as_ref());
    }
}

fn test_data() -> Vec<u8> {
    let mut v = vec![0u8; 256 * 1024];
    rand::thread_rng().fill_bytes(&mut v);
    v
}

/// The hash fields a listing entry comes with, without download links.
fn hashes(v: &Value) -> String {
    let mut out = Vec::new();
    for k in ["hash_info", "hashinfo", "md5", "sha1", "content_hash", "hash"] {
        let x = &v[k];
        if !x.is_null() && x != "" && x != "null" {
            out.push(format!("{k}={x}"));
        }
    }
    if out.is_empty() {
        "no hash".into()
    } else {
        out.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Quark

fn quark(log: &Log) -> Result<()> {
    let cfg = config::load()?;
    let CloudConfig::Quark { folder } = &cfg.cloud else {
        bail!("the current cloud in Ludonimbus is not Quark: switch to it in Settings first");
    };
    let run = openlist::ensure_running()?;
    let api = openlist::Api::login(run.port)?;
    let root = openlist::QUARK_MOUNT;
    log.step(format!("Quark: OpenList on port {}", run.port));

    let list = |path: &str| -> Result<Vec<Value>> {
        let v = api.fs("list", json!({ "path": path, "refresh": true, "page": 1, "per_page": 0, "password": "" }))?;
        Ok(v["content"].as_array().cloned().unwrap_or_default())
    };

    // 1. The drive's root, outside the Ludonimbus folder.
    let t = Instant::now();
    let items = list(root)?;
    log.step(format!("Quark: read the drive's root folder: {} entries ({} ms)", items.len(), t.elapsed().as_millis()));

    // 2. A folder of yours, read only.
    let mine = env("LUDONIMBUS_E1_QUARK_FOLDER");
    if !mine.trim().is_empty() {
        let path = format!("{root}/{}", mine.trim().trim_matches('/'));
        let t = Instant::now();
        let items = list(&path)?;
        log.step(format!("Quark: read {mine}: {} entries ({} ms)", items.len(), t.elapsed().as_millis()));
        for e in items.iter().take(10) {
            log.step(format!(
                "  {} {} bytes{} modified {} — {}",
                e["name"].as_str().unwrap_or("?"),
                e["size"],
                if e["is_dir"].as_bool() == Some(true) { " (folder)" } else { "" },
                e["modified"].as_str().unwrap_or("?"),
                hashes(e)
            ));
        }
        if let Some(f) = items.iter().find(|e| e["is_dir"].as_bool() == Some(false)) {
            let name = f["name"].as_str().unwrap_or_default();
            let g = api.fs("get", json!({ "path": format!("{path}/{name}"), "password": "" }))?;
            log.step(format!(
                "Quark: details of {name}: {} — download link {}",
                hashes(&g),
                if g["raw_url"].as_str().is_some_and(|u| !u.is_empty()) { "yes" } else { "no" }
            ));
        }
    }

    // 3. A file outside the Ludonimbus folder, copied into it on the server.
    let id = ulid::Ulid::new().to_string().to_lowercase();
    let src_dir = format!("{root}/Ludonimbus-E1-{id}");
    let copy_rel = format!("e1-copy-{id}");
    let dst_dir = format!("{root}/{}/{copy_rel}", folder.trim_matches('/'));
    let data = test_data();
    let result = (|| -> Result<()> {
        api.mkdir(&src_dir)?;
        let t = Instant::now();
        api.put_file(&format!("{src_dir}/e1.bin"), &data)?;
        log.step(format!("Quark: uploaded a 256 KB test file outside the Ludonimbus folder ({} ms)", t.elapsed().as_millis()));
        let g = api.fs("get", json!({ "path": format!("{src_dir}/e1.bin"), "password": "" }))?;
        log.step(format!("Quark: the test file's details: {}", hashes(&g)));
        api.mkdir(&dst_dir)?;
        copy_and_wait(log, &api, &src_dir, "e1.bin", &dst_dir, data.len() as u64, 180)?;
        // Read it back the way Ludonimbus reads its files (WebDAV).
        let store = cloud::build_store(&cfg.cloud, None, Duration::from_secs(30))?;
        let t = Instant::now();
        let back = store.read(&format!("{copy_rel}/e1.bin"))?.ok_or_else(|| anyhow!("the copy can't be read through WebDAV"))?;
        if back != data {
            bail!("the copy read back differently ({} bytes)", back.len());
        }
        log.step(format!("Quark: the copy reads back identical through Ludonimbus's WebDAV ({} ms)", t.elapsed().as_millis()));
        // A big file of yours: near-instant means Quark copies on its servers;
        // otherwise OpenList downloads and uploads it again.
        let big = env("LUDONIMBUS_E1_QUARK_FILE");
        if !big.trim().is_empty() {
            let path = format!("{root}/{}", big.trim().trim_matches('/'));
            let (dir, name) = path.rsplit_once('/').ok_or_else(|| anyhow!("LUDONIMBUS_E1_QUARK_FILE needs a folder and a name"))?;
            let g = api.fs("get", json!({ "path": path, "password": "" }))?;
            let size = g["size"].as_u64().unwrap_or(0);
            log.step(format!("Quark: copying your file {big} ({} MB) into the Ludonimbus folder", size / 1_048_576));
            copy_and_wait(log, &api, dir, name, &dst_dir, size, 900)?;
        }
        Ok(())
    })();
    for d in [&src_dir, &dst_dir] {
        if let Err(e) = api.remove_path(d) {
            log.step(format!("Quark: cleanup of {d} failed: {e:#} — delete it by hand"));
        }
    }
    log.step("Quark: test folders deleted");
    openlist::release_if_started_here();
    result
}

/// Ask OpenList to copy `name` from `src_dir` to `dst_dir` and wait until the
/// copy has its full size (copies run as background tasks).
fn copy_and_wait(log: &Log, api: &openlist::Api, src_dir: &str, name: &str, dst_dir: &str, size: u64, max_secs: u64) -> Result<()> {
    let t = Instant::now();
    let reply = api.fs("copy", json!({ "src_dir": src_dir, "dst_dir": dst_dir, "names": [name] }))?;
    let task = reply["tasks"][0]["name"].as_str().unwrap_or("").chars().take(120).collect::<String>();
    log.step(format!("Quark: copy request answered in {} ms ({task})", t.elapsed().as_millis()));
    let deadline = Instant::now() + Duration::from_secs(max_secs);
    loop {
        let v = api.fs("list", json!({ "path": dst_dir, "refresh": true, "page": 1, "per_page": 0, "password": "" }))?;
        let got = v["content"].as_array().and_then(|l| l.iter().find(|e| e["name"] == name).and_then(|e| e["size"].as_u64()));
        if got == Some(size) {
            let secs = t.elapsed().as_secs_f64();
            log.step(format!("Quark: the copy of {name} is complete after {secs:.1} s ({:.1} MB/s)", size as f64 / 1_048_576.0 / secs.max(0.001)));
            return Ok(());
        }
        if Instant::now() > deadline {
            bail!("the copy of {name} wasn't complete after {max_secs} s ({got:?} of {size} bytes)");
        }
        let wait = if t.elapsed() < Duration::from_secs(10) { 300 } else { 2000 };
        std::thread::sleep(Duration::from_millis(wait));
    }
}

// ---------------------------------------------------------------------------
// Baidu Netdisk

const FILE: &str = "/rest/2.0/xpan/file";
/// Error numbers to report instead of failing on.
const SOFT: &[i64] = &[-7, -9, -8, -6, 2, 6, 12, 111, 31023, 31061, 31066, 31034];

fn errno(v: &Value) -> i64 {
    v["errno"].as_i64().unwrap_or(0)
}

fn baidu(log: &Log) -> Result<()> {
    let cfg = config::load()?;
    let (k, s, n) = match &cfg.cloud {
        CloudConfig::BaiduPan { app_key, secret_key, app_name, .. } => (app_key.clone(), secret_key.clone(), app_name.clone()),
        _ => bail!("the current cloud in Ludonimbus is not Baidu Netdisk: switch to it in Settings first"),
    };
    let pick = |own: &str, var: &str| if own.trim().is_empty() { env(var) } else { own.to_string() };
    let client = BaiduClient::resolve(&pick(&k, "LUDONIMBUS_BAIDU_APP_KEY"), &pick(&s, "LUDONIMBUS_BAIDU_SECRET_KEY"), &pick(&n, "LUDONIMBUS_BAIDU_APP_NAME"))
        .ok_or_else(|| anyhow!("no Baidu app keys: put them in secrets.local.env (the script reads it)"))?;
    let token = baidu::load_token()?.ok_or_else(|| anyhow!("sign in to Baidu Netdisk in Ludonimbus first"))?;
    let pan = BaiduPan::new(client, Endpoints::default(), token, TokenSink::Keyring, Duration::from_secs(20));
    let app = pan.app_root().to_string();

    let list = |dir: &str| pan.xpan(FILE, &[("method", "list"), ("dir", dir), ("limit", "100"), ("web", "0")], None, SOFT);

    // 1. The netdisk's root and the app folder.
    for dir in ["/", app.as_str()] {
        let t = Instant::now();
        let v = list(dir)?;
        log.step(format!("Baidu: list {dir}: errno {} — {} entries ({} ms)", errno(&v), v["list"].as_array().map_or(0, Vec::len), t.elapsed().as_millis()));
    }

    // 2. A folder of yours, read only.
    let mine = env("LUDONIMBUS_E1_BAIDU_FOLDER");
    if !mine.trim().is_empty() {
        let v = list(mine.trim())?;
        let items = v["list"].as_array().cloned().unwrap_or_default();
        log.step(format!("Baidu: list {mine}: errno {} — {} entries", errno(&v), items.len()));
        for e in items.iter().take(10) {
            log.step(format!(
                "  {} {} bytes{} mtime {} — {}",
                e["server_filename"].as_str().unwrap_or("?"),
                e["size"],
                if e["isdir"].as_i64() == Some(1) { " (folder)" } else { "" },
                e["server_mtime"],
                hashes(e)
            ));
        }
        if let Some(f) = items.iter().find(|e| e["isdir"].as_i64() == Some(0)) {
            let fsids = format!("[{}]", f["fs_id"]);
            let m = pan.xpan("/rest/2.0/xpan/multimedia", &[("method", "filemetas"), ("fsids", &fsids), ("dlink", "1")], None, SOFT)?;
            let meta = &m["list"][0];
            log.step(format!(
                "Baidu: details of {}: errno {} — {} — download link {}",
                meta["filename"].as_str().unwrap_or("?"),
                errno(&m),
                hashes(meta),
                if meta["dlink"].as_str().is_some_and(|u| !u.is_empty()) { "yes" } else { "no" }
            ));
        }
    }

    // 3. Writing outside the app folder.
    let id = ulid::Ulid::new().to_string().to_lowercase();
    let outside = format!("/Ludonimbus-E1-{id}");
    let v = pan.xpan(FILE, &[("method", "create")], Some(&[("path", &outside), ("isdir", "1"), ("rtype", "0")]), SOFT)?;
    if errno(&v) == 0 {
        log.step(format!("Baidu: creating a folder outside the app folder WORKED ({outside}); deleting it"));
        let list = format!("[\"{outside}\"]");
        let _ = pan.xpan(FILE, &[("method", "filemanager"), ("opera", "delete")], Some(&[("async", "0"), ("filelist", &list)]), SOFT);
    } else {
        log.step(format!("Baidu: creating a folder outside the app folder was refused (errno {})", errno(&v)));
    }

    // 4. Copying a file of yours into the app folder, on the server.
    let file = env("LUDONIMBUS_E1_BAIDU_FILE");
    if !file.trim().is_empty() {
        let dest = format!("{app}/e1-copy-{id}");
        let result = (|| -> Result<()> {
            let v = pan.xpan(FILE, &[("method", "create")], Some(&[("path", &dest), ("isdir", "1"), ("rtype", "0")]), SOFT)?;
            log.step(format!("Baidu: test folder in the app folder: errno {}", errno(&v)));
            let list = json!([{ "path": file.trim(), "dest": dest, "newname": "e1-copy.bin" }]).to_string();
            let t = Instant::now();
            let v = pan.xpan(FILE, &[("method", "filemanager"), ("opera", "copy")], Some(&[("async", "0"), ("ondup", "fail"), ("filelist", &list)]), SOFT)?;
            log.step(format!("Baidu: copy {} into the app folder: errno {} ({} ms) {}", file.trim(), errno(&v), t.elapsed().as_millis(), v["info"]));
            let after = pan.xpan(FILE, &[("method", "list"), ("dir", &dest), ("limit", "10"), ("web", "0")], None, SOFT)?;
            let found = after["list"].as_array().is_some_and(|l| l.iter().any(|e| e["server_filename"] == "e1-copy.bin"));
            log.step(format!("Baidu: the copy is {}", if found { "there" } else { "NOT there" }));
            Ok(())
        })();
        let list = format!("[\"{dest}\"]");
        let v = pan.xpan(FILE, &[("method", "filemanager"), ("opera", "delete")], Some(&[("async", "0"), ("filelist", &list)]), SOFT);
        log.step(format!("Baidu: test folder deleted (errno {})", v.map(|v| errno(&v)).unwrap_or(-1)));
        result?;
    }
    Ok(())
}

#[test]
#[ignore]
fn sources_live() {
    let log = Log(Instant::now());
    let mut failed = Vec::new();
    if env("LUDONIMBUS_E1_QUARK") == "1" {
        if let Err(e) = quark(&log) {
            log.step(format!("Quark: FAILED: {e:#}"));
            failed.push("Quark");
        }
    }
    if env("LUDONIMBUS_E1_BAIDU") == "1" {
        if let Err(e) = baidu(&log) {
            log.step(format!("Baidu: FAILED: {e:#}"));
            failed.push("Baidu");
        }
    }
    assert!(failed.is_empty(), "failed: {failed:?}");
}

/// The Quark part against OpenList's local storage instead of Quark (checks
/// the OpenList calls themselves). Needs the official zip:
/// `LUDONIMBUS_OPENLIST_ZIP=…\openlist-windows-amd64.zip cargo test -p ludonimbus-core --test sources_live sources_local_openlist -- --ignored --nocapture`
#[test]
#[ignore]
fn sources_local_openlist() {
    let zip = env("LUDONIMBUS_OPENLIST_ZIP");
    assert!(!zip.is_empty(), "set LUDONIMBUS_OPENLIST_ZIP");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    openlist::install(Some(std::path::Path::new(&zip)), &|_, _| {}).unwrap();
    let run = openlist::ensure_running().unwrap();
    let drive = tmp.path().join("drive");
    std::fs::create_dir_all(drive.join("Games/Zelda")).unwrap();
    std::fs::write(drive.join("Games/Zelda/game.iso"), vec![7u8; 100_000]).unwrap();
    std::fs::write(drive.join("Games/readme.txt"), b"hello").unwrap();
    let api = openlist::Api::login(run.port).unwrap();
    api.put_storage(openlist::QUARK_MOUNT, "Local", json!({ "root_folder_path": drive.display().to_string() })).unwrap();
    api.mkdir("/quark/Ludonimbus").unwrap();
    api.ensure_webdav_user("/quark/Ludonimbus").unwrap();
    config::update(|c| {
        c.cloud = CloudConfig::Quark { folder: "Ludonimbus".into() };
        Ok(())
    })
    .unwrap();
    std::env::set_var("LUDONIMBUS_E1_QUARK_FOLDER", "/Games");
    std::env::set_var("LUDONIMBUS_E1_QUARK_FILE", "/Games/Zelda/game.iso");
    let log = Log(Instant::now());
    quark(&log).unwrap();
    let left: Vec<String> = std::fs::read_dir(&drive).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.starts_with("Ludonimbus-E1")), "test folder removed: {left:?}");
    assert_eq!(std::fs::read_dir(drive.join("Ludonimbus")).unwrap().count(), 0, "copy removed");
    assert!(drive.join("Games/Zelda/game.iso").is_file(), "your files untouched");
    openlist::forget();
}
