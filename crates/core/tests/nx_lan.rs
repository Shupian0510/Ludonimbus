//! The Switch client's side of the LAN protocol (docs/nx-protocol.md,
//! protocol 2), run against the PC's request handler: pairing, statuses
//! answered from what the PC knows (checked with the cloud in the
//! background), the first upload of a game only the Switch has (it gets a
//! cloud definition with its title id, so emulators find it), uploads
//! queued on the PC and sent to the cloud later, the conflict check when the
//! cloud moved on, backups, and downloads as background jobs.

use ludonimbus_core::config::{self, AppConfig, CloudConfig};
use ludonimbus_core::nxlan::{self, Req};
use ludonimbus_core::snapshot::{self, Trigger};
use ludonimbus_core::sync::Ctx;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::time::Duration;

const TITLE: &str = "0100770008dd8000";

fn call(method: &str, path: &str, query: &str, auth: Option<&str>, body: &[u8]) -> (u16, Vec<u8>) {
    let r = nxlan::handle(&Req { method, path, query, auth, body });
    (r.status, r.body)
}

fn json_of(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap()
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn status_once(auth: &str, last: Option<&str>, changed: bool) -> Value {
    let body = json!({ "games": [{ "title": TITLE, "lastSyncedId": last, "changed": changed }] }).to_string();
    let (s, b) = call("POST", "/nx/v1/status", "client=0.2.0", Some(auth), body.as_bytes());
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    json_of(&b)
}

/// Ask until the PC has checked everything with the cloud, as the Switch does.
fn status_full(auth: &str, last: Option<&str>, changed: bool) -> Value {
    for _ in 0..100 {
        let v = status_once(auth, last, changed);
        if v["fresh"].as_bool() == Some(true) {
            return v;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the PC never finished checking");
}

fn status(auth: &str, last: Option<&str>, changed: bool) -> String {
    status_full(auth, last, changed)["games"][0]["status"].as_str().unwrap().to_string()
}

fn upload(auth: &str, query: &str, zip: &[u8]) -> (u16, Value) {
    let (s, b) = call("POST", &format!("/nx/v1/games/{TITLE}/snapshots"), query, Some(auth), zip);
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// Download a version the way the Switch does: start a job, wait, fetch.
fn download(auth: &str, id: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    let (s, b) = call("POST", "/nx/v1/downloads", "", Some(auth), json!({ "title": TITLE, "id": id }).to_string().as_bytes());
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let job = json_of(&b)["job"].as_str().unwrap().to_string();
    for _ in 0..200 {
        let (s, b) = call("GET", &format!("/nx/v1/jobs/{job}"), "", Some(auth), b"");
        assert_eq!(s, 200);
        match json_of(&b)["state"].as_str().unwrap() {
            "working" => std::thread::sleep(Duration::from_millis(25)),
            "done" => {
                let (s, b) = call("GET", &format!("/nx/v1/jobs/{job}/file"), "", Some(auth), b"");
                assert_eq!(s, 200);
                return zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
            }
            other => panic!("download {other}: {}", String::from_utf8_lossy(&b)),
        }
    }
    panic!("download never finished");
}

fn read(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
    let mut data = String::new();
    z.by_name(name).unwrap().read_to_string(&mut data).unwrap();
    data
}

#[test]
fn switch_client_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let cloud = tmp.path().join("cloud");
    std::fs::create_dir_all(&cloud).unwrap();
    config::update(|c| {
        *c = AppConfig::default();
        c.cloud = CloudConfig::LocalFolder { path: cloud.clone() };
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    let key = "nx-0100770008dd8000";
    // Uploads to the cloud happen when the test says (else in the background).
    nxlan::hold_uploads(true);

    // Hello without a token; nothing else.
    let (s, b) = call("GET", "/nx/v1/hello", "", None, b"");
    assert_eq!((s, json_of(&b)["protocol"].as_u64()), (200, Some(2)));
    assert_eq!(call("POST", "/nx/v1/status", "", None, b"{}").0, 401);

    // Pairing only while the PC shows a code; a wrong code doesn't pair.
    let pair = |code: &str| call("POST", "/nx/v1/pair", "", None, json!({ "code": code, "id": "sw-123", "name": "Switch" }).to_string().as_bytes());
    assert_eq!(pair("000000").0, 403);
    let (code, _) = nxlan::start_pairing();
    let wrong = if code == "111111" { "222222" } else { "111111" };
    assert_eq!(pair(wrong).0, 403);
    let (s, b) = pair(&code);
    assert_eq!(s, 200);
    let token = json_of(&b)["token"].as_str().unwrap().to_string();
    let auth = format!("Bearer {token}");
    assert_eq!(nxlan::devices().len(), 1);
    assert!(nxlan::pairing_code().is_none(), "one Switch per code");

    // The first answer comes at once, before the cloud's game list was ever
    // fetched: "checking", ask again.
    let first_answer = status_once(&auth, None, true);
    assert_eq!(first_answer["games"][0]["status"], "checking");
    assert_eq!(first_answer["fresh"], false);

    // A game only the Switch has: upload it. It's queued on the PC and the
    // Switch is in sync right away.
    assert_eq!(status(&auth, None, true), "upload");
    let save = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"hunter rank 50"), ("nxsave-0100770008dd8000/sub/opts.bin", b"o")]);
    let (s, b) = upload(&auth, "name=MONSTER%20HUNTER%20GU", &save);
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["pending"], true);
    let first = b["id"].as_str().unwrap().to_string();
    let ctx = Ctx::load().unwrap();
    let games = ctx.cloud().unwrap().list_games().unwrap();
    let cg = games.iter().find(|g| g.key == key).expect("cloud definition");
    assert_eq!(cg.name, "MONSTER HUNTER GU");
    assert!(cg.ids.contains(&"nx:0100770008DD8000".to_string()));
    assert!(ctx.cloud().unwrap().list_snapshots(key).unwrap().is_empty(), "not uploaded yet");
    let v = status_full(&auth, Some(&first), false);
    assert_eq!((v["games"][0]["status"].as_str(), v["games"][0]["pending"].as_bool()), (Some("synced"), Some(true)));
    assert_eq!(nxlan::waiting_uploads(), 1);

    // A queued version can be downloaded already (from the PC's queue).
    assert_eq!(read(&mut download(&auth, &first), "nxsave-0100770008dd8000/save.bin"), "hunter rank 50");

    // The PC uploads it; the Switch is now registered as a device too.
    let _ = nxlan::upload_waiting().unwrap();
    assert_eq!(nxlan::waiting_uploads(), 0);
    assert_eq!(ctx.cloud().unwrap().list_snapshots(key).unwrap().len(), 1);
    let v = status_full(&auth, Some(&first), false);
    assert_eq!((v["games"][0]["status"].as_str(), v["games"][0]["pending"].as_bool()), (Some("synced"), Some(false)));
    assert_eq!(status(&auth, Some(&first), true), "localNewer");
    let mut registered = false;
    for _ in 0..100 {
        registered = ctx.cloud().unwrap().list_devices().unwrap().iter().any(|d| d.os == "switch");
        if registered {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(registered, "the Switch is a device in the cloud");

    // An emulator on another PC saves a newer version. The PC's answers may
    // not know yet, but writing checks the cloud: the Switch's upload on top
    // of the old version is refused as a conflict.
    let (meta, zip) = snapshot::build_from_files(
        key,
        &[("nxsave-0100770008dd8000/save.bin".to_string(), b"hunter rank 51".to_vec())],
        Trigger::AutoExit,
        Some(first.clone()),
        ("d-pc", "PC"),
        "",
        None,
    )
    .unwrap();
    ctx.cloud().unwrap().upload(&meta, &zip).unwrap();
    let mine = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"hunter rank 50, switch side")]);
    let (s, b) = upload(&auth, &format!("parent={first}"), &mine);
    assert_eq!((s, b["head"]["id"].as_str()), (409, Some(meta.id.as_str())));
    assert_eq!(upload(&auth, "", &mine).0, 409, "no parent while the cloud has versions: a conflict too");
    assert_eq!(status(&auth, Some(&first), false), "cloudNewer");
    assert_eq!(status(&auth, Some(&first), true), "conflict");

    // The user picks the cloud's: the Switch first keeps its own saves as a
    // backup version (not the newest), once.
    let (s, b) = upload(&auth, &format!("backup=1&parent={first}"), &mine);
    assert_eq!(s, 200, "{b}");
    let backup = b["id"].as_str().unwrap().to_string();
    let (_, b) = upload(&auth, "backup=1", &mine);
    assert_eq!((b["id"].as_str(), b["existing"].as_bool()), (Some(backup.as_str()), Some(true)));
    let _ = nxlan::upload_waiting().unwrap();
    let kept = ctx.cloud().unwrap().list_snapshots(key).unwrap();
    let k = kept.iter().find(|s| s.id == backup).unwrap();
    assert_eq!((k.trigger, k.device_name.as_str()), (Trigger::PreRestore, "Switch"));
    assert_eq!(ludonimbus_core::sync::mainline_head(&kept).unwrap().id, meta.id, "the backup doesn't become the newest version");

    // The list and the download hold only the Switch game's files.
    let (s, b) = call("GET", &format!("/nx/v1/games/{TITLE}/snapshots"), "", Some(&auth), b"");
    assert_eq!(s, 200);
    let list = json_of(&b);
    assert_eq!(list["headId"].as_str(), Some(meta.id.as_str()));
    assert_eq!(list["snapshots"].as_array().unwrap().len(), 3);
    let mut z = download(&auth, &meta.id);
    assert_eq!(z.len(), 1);
    assert_eq!(read(&mut z, "nxsave-0100770008dd8000/save.bin"), "hunter rank 51");

    // Or the user keeps the Switch's: on top of the newest version.
    let (s, b) = upload(&auth, "force=1", &mine);
    assert_eq!(s, 200, "{b}");
    let kept_switch = b["id"].as_str().unwrap().to_string();
    assert_eq!(status(&auth, Some(&kept_switch), false), "synced");
    let (_, b) = call("GET", &format!("/nx/v1/games/{TITLE}/snapshots"), "", Some(&auth), b"");
    let list = json_of(&b);
    assert_eq!(list["headId"].as_str(), Some(kept_switch.as_str()));
    let newest = &list["snapshots"][0];
    assert_eq!((newest["id"].as_str(), newest["parent"].as_str(), newest["pending"].as_bool()), (Some(kept_switch.as_str()), Some(meta.id.as_str()), Some(true)));
    let _ = nxlan::upload_waiting().unwrap();
    assert_eq!(ludonimbus_core::sync::mainline_head(&ctx.cloud().unwrap().list_snapshots(key).unwrap()).unwrap().id, kept_switch);

    // Files of another game are refused; unknown jobs aren't there.
    let bad = zip_of(&[("nxsave-0100000000010000/x", b"x")]);
    assert_eq!(upload(&auth, "", &bad).0, 500);
    assert_eq!(call("GET", "/nx/v1/jobs/0000", "", Some(&auth), b"").0, 404);

    // A removed Switch can't connect any more.
    nxlan::remove_device("sw-123").unwrap();
    assert_eq!(call("GET", &format!("/nx/v1/games/{TITLE}/snapshots"), "", Some(&auth), b"").0, 401);
}
