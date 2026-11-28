//! The Switch client's side of the LAN protocol (docs/nx-protocol.md), run
//! against the PC's request handler.
//!
//! Protocol 2 (`switch_client_round_trip`, a Switch app that hasn't been
//! updated): pairing, statuses answered from what the PC knows (checked
//! with the cloud in the background), the first upload of a game only the
//! Switch has (it gets a cloud definition with its title id, so emulators
//! find it), uploads queued on the PC and sent to the cloud later, the
//! conflict check when the cloud moved on, backups, and downloads as
//! background jobs.
//!
//! Protocol 3 (`several_switches_and_users`): two Switches, users mapped to
//! save lines by nickname, "back up" that never refuses, the PC's version
//! of the game as a hint and a source, taking a version with a kept copy
//! and undoing it, and what the owner sets on the PC (names, separate
//! users, users left out, unpairing).

use ludonimbus_core::cloud::client::line_key;
use ludonimbus_core::cloud::CloudGame;
use ludonimbus_core::config::{self, AppConfig, CloudConfig};
use ludonimbus_core::nxlan::{self, Req};
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::Duration;

const TITLE: &str = "0100770008dd8000";

/// The tests share the PC's state (paired Switches, the pairing code, the
/// data folder): one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

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
    upload_to(TITLE, auth, query, zip)
}

fn upload_to(title: &str, auth: &str, query: &str, zip: &[u8]) -> (u16, Value) {
    let (s, b) = call("POST", &format!("/nx/v1/games/{title}/snapshots"), query, Some(auth), zip);
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// Download a version the way the Switch does: start a job, wait, fetch.
fn download(auth: &str, id: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    download_with(auth, json!({ "title": TITLE, "id": id }))
}

fn download_with(auth: &str, body: Value) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    let (s, b) = call("POST", "/nx/v1/downloads", "", Some(auth), body.to_string().as_bytes());
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
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
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
    assert_eq!((s, json_of(&b)["protocol"].as_u64()), (200, Some(3)));
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
    let cloud = ctx.cloud().unwrap();
    let meta = sync::queue_files(
        &cloud.open(key).unwrap(),
        &sync::Foreign {
            key,
            files: &[("nxsave-0100770008dd8000/save.bin".to_string(), b"hunter rank 51".to_vec())],
            trigger: Trigger::AutoExit,
            parent: Some(first.clone()),
            over_all: false,
            device: ("d-pc", "PC"),
            platform: "emu:ryujinx",
            comment: "",
            game_version: None,
            origin: None,
        },
    )
    .unwrap();
    sync::upload_pending(cloud, &meta).unwrap();
    sync::drop_pending(&meta).unwrap();
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
    assert_eq!(
        (newest["id"].as_str(), newest["parent"].as_str(), newest["pending"].as_bool()),
        (Some(kept_switch.as_str()), Some(meta.id.as_str()), Some(true))
    );
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

const U1: &str = "0123456789abcdef0123456789abcde1";
const U2: &str = "0123456789abcdef0123456789abcde2";
const V1: &str = "fedcba9876543210fedcba9876543210";

/// A status of protocol 3: the Switch's users, and its saves of the game
/// (user, version last synced, changed since); asked until the PC finished
/// checking, as the Switch does.
fn status3(auth: &str, users: &[(&str, &str)], games: &[(&str, Option<&str>, bool)]) -> Value {
    status3_of(TITLE, auth, users, games)
}

fn status3_of(title: &str, auth: &str, users: &[(&str, &str)], games: &[(&str, Option<&str>, bool)]) -> Value {
    let body = json!({
        "users": users.iter().map(|(uid, nick)| json!({ "uid": uid, "nickname": nick })).collect::<Vec<_>>(),
        "games": games.iter().map(|(user, last, changed)| json!({ "title": title, "user": user, "lastSyncedId": last, "changed": changed })).collect::<Vec<_>>(),
    })
    .to_string();
    for _ in 0..200 {
        let (s, b) = call("POST", "/nx/v1/status", "client=0.2.0", Some(auth), body.as_bytes());
        assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
        let v = json_of(&b);
        if v["fresh"].as_bool() == Some(true) {
            return v;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the PC never finished checking");
}

fn game_for(v: &Value, user: &str) -> Value {
    v["games"].as_array().unwrap().iter().find(|g| g["user"] == user).cloned().unwrap_or(Value::Null)
}

fn sources(auth: &str, user: &str) -> Value {
    sources_of(TITLE, auth, user)
}

fn sources_of(title: &str, auth: &str, user: &str) -> Value {
    let (s, b) = call("GET", &format!("/nx/v1/games/{title}/sources"), &format!("user={user}"), Some(auth), b"");
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    json_of(&b)
}

fn item<'a>(src: &'a Value, group: &str) -> &'a Value {
    src["items"].as_array().unwrap().iter().find(|i| i["group"] == group).unwrap_or_else(|| panic!("no {group} item in {src}"))
}

fn id_of(r: &Value) -> String {
    r["id"].as_str().unwrap_or_else(|| panic!("no id in {r}")).to_string()
}

#[test]
fn several_switches_and_users() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let cloud_dir = tmp.path().join("cloud");
    std::fs::create_dir_all(&cloud_dir).unwrap();
    config::update(|c| {
        *c = AppConfig::default();
        c.cloud = CloudConfig::LocalFolder { path: cloud_dir.clone() };
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    nxlan::hold_uploads(true);
    let card = "nx-0100770008dd8000";

    // Two Switches pair; the owner names the second one on the PC.
    let pair = |id: &str| {
        let (code, _) = nxlan::start_pairing();
        let (s, b) = call("POST", "/nx/v1/pair", "", None, json!({ "code": code, "id": id, "name": "Switch" }).to_string().as_bytes());
        assert_eq!(s, 200);
        format!("Bearer {}", json_of(&b)["token"].as_str().unwrap())
    };
    let a = pair("sw-a");
    let b = pair("sw-b");
    nxlan::rename_device("sw-b", "Bedroom").unwrap();
    assert_eq!(nxlan::devices().len(), 2);

    // A reports its two users: both are backed up, each player on a line of
    // their own.
    let users_a = [(U1, "Shupian"), (U2, "Kid")];
    let v = status3(&a, &users_a, &[(U1, None, true), (U2, None, true)]);
    assert_eq!(v["device"]["name"], "Switch");
    let users = v["users"].as_array().unwrap();
    assert!(users.len() == 2 && users.iter().all(|u| u["sync"] == true && u["separate"] == false), "{v}");
    assert_eq!((users[0]["line"].as_str(), users[1]["line"].as_str()), (Some("Switch · Shupian"), Some("Switch · Kid")));
    assert_eq!((game_for(&v, U1)["status"].as_str(), game_for(&v, U2)["status"].as_str()), (Some("upload"), Some("upload")));

    // "Back up": each user's saves become a version of their player's line.
    let save_a = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"shupian rank 50"), ("nxdevice-0100770008dd8000/dev.bin", b"device")]);
    let (s, r) = upload(&a, &format!("user={U1}&fork=1&name=MHGU"), &save_a);
    assert_eq!((s, r["forked"].as_bool()), (200, Some(false)), "{r}");
    let a1 = id_of(&r);
    let save_kid = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"kid rank 3")]);
    let (s, r) = upload(&a, &format!("user={U2}&fork=1"), &save_kid);
    assert_eq!(s, 200, "{r}");
    let k1 = id_of(&r);
    let _ = nxlan::upload_waiting().unwrap();
    let ctx = Ctx::load().unwrap();
    let cloud = ctx.cloud().unwrap();
    let lines = cloud.card(card).unwrap().expect("a card").lines;
    let line_named = |n: &str| lines.iter().find(|l| l.name.as_deref() == Some(n)).unwrap_or_else(|| panic!("no line {n}: {lines:?}")).id.clone();
    let (line_shupian, line_kid) = (line_named("Switch · Shupian"), line_named("Switch · Kid"));
    assert_ne!(line_shupian, line_kid);
    // Lines of the Switch's kind: only Switches continue them by themselves
    // (an emulator of the same title is another version), and so is the
    // card's line the Switch began with it.
    assert!(lines.iter().all(|l| l.kind.as_deref() == Some("switch")), "{lines:?}");
    assert_eq!(cloud.list_snapshots(&line_key(card, &line_shupian)).unwrap().len(), 1);
    assert_eq!(cloud.list_snapshots(&line_key(card, &line_kid)).unwrap().len(), 1);
    assert!(cloud.list_snapshots(card).unwrap().is_empty(), "the line the PC's versions share has none of it");

    // An emulator on a PC saves the game on that shared line. The player who
    // first synced hears of it, as a version the Switch can take as it is;
    // the other player doesn't.
    let emu = sync::queue_files(
        &cloud.open(card).unwrap(),
        &sync::Foreign {
            key: card,
            files: &[("nxsave-0100770008dd8000/save.bin".to_string(), b"ryujinx rank 60".to_vec())],
            trigger: Trigger::AutoExit,
            parent: None,
            over_all: false,
            device: ("d-pc", "DESKTOP"),
            platform: "emu:ryujinx",
            comment: "",
            game_version: None,
            origin: None,
        },
    )
    .unwrap();
    sync::upload_pending(cloud, &emu).unwrap();
    sync::drop_pending(&emu).unwrap();
    let v = status3(&a, &users_a, &[(U1, Some(&a1), false), (U2, Some(&k1), false)]);
    let g = game_for(&v, U1);
    assert_eq!(g["status"], "synced");
    assert_eq!(
        (g["newer"]["label"].as_str(), g["newer"]["deviceName"].as_str(), g["newer"]["convert"].as_bool()),
        (Some("Ryujinx"), Some("DESKTOP"), Some(false)),
        "{g}"
    );
    assert_eq!(game_for(&v, U2)["status"], "synced");
    assert!(game_for(&v, U2)["newer"].is_null(), "not the other player's");
    let src = sources(&a, U1);
    let pc = item(&src, "platform");
    assert_eq!((pc["id"].as_str(), pc["line"].as_str(), pc["label"].as_str()), (Some(emu.id.as_str()), Some("main"), Some("Ryujinx")));
    assert_eq!(item(&src, "this")["id"].as_str(), Some(a1.as_str()));

    // A takes it: its own saves are kept first (the cloud has them: nothing
    // new is stored), then the PC's are written and become the newest of the
    // player's line.
    let (_, r) = upload(&a, &format!("user={U1}&backup=1&parent={a1}"), &save_a);
    assert_eq!((r["id"].as_str(), r["existing"].as_bool()), (Some(a1.as_str()), Some(true)));
    let mut z = download_with(&a, json!({ "title": TITLE, "id": emu.id, "user": U1, "line": "main", "base": a1 }));
    assert_eq!(read(&mut z, "nxsave-0100770008dd8000/save.bin"), "ryujinx rank 60");
    let taken = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"ryujinx rank 60"), ("nxdevice-0100770008dd8000/dev.bin", b"device")]);
    let (s, r) = upload(&a, &format!("user={U1}&force=1&from={}&fromLine=main", emu.id), &taken);
    assert_eq!(s, 200, "{r}");
    let a2 = id_of(&r);
    let _ = nxlan::upload_waiting().unwrap();
    let list = cloud.list_snapshots(&line_key(card, &line_shupian)).unwrap();
    assert_eq!(list.iter().find(|s| s.id == a2).unwrap().trigger, Trigger::Restore);
    assert_eq!(sync::mainline_head(&list).unwrap().id, a2);

    // Undo: the kept copy goes back and is the newest again.
    let mut z = download_with(&a, json!({ "title": TITLE, "id": a1, "user": U1 }));
    assert_eq!(read(&mut z, "nxsave-0100770008dd8000/save.bin"), "shupian rank 50");
    assert_eq!(read(&mut z, "nxdevice-0100770008dd8000/dev.bin"), "device");
    let (s, r) = upload(&a, &format!("user={U1}&force=1&from={a1}"), &save_a);
    assert_eq!(s, 200, "{r}");
    let a3 = id_of(&r);
    let v = status3(&a, &users_a, &[(U1, Some(&a3), false), (U2, Some(&k1), false)]);
    assert_eq!(game_for(&v, U1)["status"], "synced");

    // The second Switch's user of the same nickname (in another case) is the
    // same player: it sees A's progress as another Switch's.
    let users_b = [(V1, "shupian ")];
    let v = status3(&b, &users_b, &[(V1, None, true)]);
    assert_eq!(v["device"]["name"], "Bedroom");
    let g = game_for(&v, V1);
    assert_eq!(
        (g["status"].as_str(), g["from"].as_str(), g["head"]["id"].as_str(), g["head"]["deviceName"].as_str()),
        (Some("conflict"), Some("switch"), Some(a3.as_str()), Some("Switch")),
        "{g}"
    );
    let src = sources(&b, V1);
    assert_eq!((item(&src, "switch")["id"].as_str(), src["headId"].as_str(), src["forked"].as_bool()), (Some(a3.as_str()), Some(a3.as_str()), Some(false)));
    // B keeps its own saves and takes A's: the newest of the line, so B is
    // in sync without another upload.
    let save_b = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"bedroom rank 1")]);
    let (s, r) = upload(&b, &format!("user={V1}&backup=1"), &save_b);
    assert_eq!(s, 200, "{r}");
    let mut z = download_with(&b, json!({ "title": TITLE, "id": a3, "user": V1 }));
    assert_eq!(read(&mut z, "nxsave-0100770008dd8000/save.bin"), "shupian rank 50");
    let v = status3(&b, &users_b, &[(V1, Some(&a3), false)]);
    assert_eq!(game_for(&v, V1)["status"], "synced");

    // Both play on. A backs up first. B's back up then isn't refused: the
    // line forks, and each Switch hears of the other's progress.
    let (_, r) = upload(&a, &format!("user={U1}&fork=1&parent={a3}"), &zip_of(&[("nxsave-0100770008dd8000/save.bin", b"shupian rank 51")]));
    assert_eq!(r["forked"], false);
    let a4 = id_of(&r);
    let save_b2 = zip_of(&[("nxsave-0100770008dd8000/save.bin", b"shupian rank 52, bedroom")]);
    assert_eq!(upload(&b, &format!("user={V1}&parent={a3}"), &save_b2).0, 409, "without fork=1 the conflict check stays");
    let (s, r) = upload(&b, &format!("user={V1}&fork=1&parent={a3}"), &save_b2);
    assert_eq!((s, r["forked"].as_bool()), (200, Some(true)), "{r}");
    let b1 = id_of(&r);
    let v = status3(&a, &users_a, &[(U1, Some(&a4), false), (U2, Some(&k1), false)]);
    let g = game_for(&v, U1);
    assert_eq!((g["status"].as_str(), g["from"].as_str(), g["head"]["deviceName"].as_str()), (Some("cloudNewer"), Some("switch"), Some("Bedroom")), "{g}");
    assert_eq!(sources(&a, U1)["forked"], true);
    let v = status3(&b, &users_b, &[(V1, Some(&b1), false)]);
    assert_eq!(game_for(&v, V1)["status"], "synced");

    // On the PC the owner separates B's user: it keeps a line of its own
    // from now on.
    nxlan::set_user("sw-b", V1, true, true).unwrap();
    let v = status3(&b, &users_b, &[(V1, Some(&b1), false)]);
    assert_eq!((v["users"][0]["separate"].as_bool(), v["users"][0]["line"].as_str()), (Some(true), Some("Switch · shupian · Bedroom")));
    assert_eq!(game_for(&v, V1)["status"], "upload");
    // And leaves the kid out: its saves are neither answered for nor taken.
    nxlan::set_user("sw-a", U2, false, false).unwrap();
    let v = status3(&a, &users_a, &[(U1, Some(&a4), false), (U2, Some(&k1), false)]);
    assert!(game_for(&v, U2).is_null(), "{v}");
    assert_eq!(v["users"][1]["sync"], false);
    assert_eq!(upload(&a, &format!("user={U2}&fork=1"), &save_kid).0, 403);

    // A Switch the owner unpairs can't connect any more.
    nxlan::remove_device("sw-a").unwrap();
    assert_eq!(call("POST", "/nx/v1/status", "", Some(&a), b"{\"games\":[]}").0, 401);
}

/// A game with a cross-platform rule (Stardew Valley, rules/games): the PC
/// version's newer save is a hint and a source marked as converted; taking
/// it brings the farm into the Switch's save data, keeps the Switch's other
/// files, and leaves the PC's settings out.
#[test]
fn a_switch_takes_another_platforms_save_through_the_rule() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    let cloud_dir = tmp.path().join("cloud");
    std::fs::create_dir_all(&cloud_dir).unwrap();
    config::update(|c| {
        *c = AppConfig::default();
        c.cloud = CloudConfig::LocalFolder { path: cloud_dir.clone() };
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    nxlan::hold_uploads(true);
    const SDV: &str = "0100e65002bb8000";
    let (code, _) = nxlan::start_pairing();
    let (_, b) = call("POST", "/nx/v1/pair", "", None, json!({ "code": code, "id": "sw-a", "name": "Switch" }).to_string().as_bytes());
    let a = format!("Bearer {}", json_of(&b)["token"].as_str().unwrap());
    let users = [(U1, "Farmer")];

    // The Switch backs up its farm first.
    let v = status3_of(SDV, &a, &users, &[(U1, None, true)]);
    assert_eq!(game_for(&v, U1)["status"], "upload");
    let mine = zip_of(&[
        ("nxsave-0100e65002bb8000/Farm_1/Farm_1", b"farm from the switch"),
        ("nxsave-0100e65002bb8000/Farm_1/SaveGameInfo", b"<switch/>"),
        ("nxsave-0100e65002bb8000/options.bin", b"switch options"),
        ("nxdevice-0100e65002bb8000/d.bin", b"device"),
    ]);
    let (s, r) = upload_to(SDV, &a, &format!("user={U1}&fork=1&name=Stardew"), &mine);
    assert_eq!(s, 200, "{r}");
    let s1 = id_of(&r);
    let _ = nxlan::upload_waiting().unwrap();

    // Then a PC plays the Steam version: one card with the Switch's (they
    // share the rule), a line of another format.
    let ctx = Ctx::load().unwrap();
    let cloud = ctx.cloud().unwrap();
    let pc = CloudGame {
        key: "stardew-valley".into(),
        name: "Stardew Valley".into(),
        adapter: None,
        saves: vec!["{appdata}/StardewValley/Saves/**".into()],
        exclude: vec![],
        wait_for: vec![],
        roots: Default::default(),
        ludusavi_title: Some("Stardew Valley".into()),
        source: None,
        ids: vec![],
        game: None,
    };
    cloud.put_game(&pc).unwrap();
    let steam = sync::queue_files(
        &cloud.open("stardew-valley").unwrap(),
        &sync::Foreign {
            key: "stardew-valley",
            files: &[
                ("appdata/StardewValley/Saves/Farm_1/Farm_1".to_string(), b"farm from the pc".to_vec()),
                ("appdata/StardewValley/Saves/Farm_1/SaveGameInfo".to_string(), b"<pc/>".to_vec()),
                ("appdata/StardewValley/startup_preferences".to_string(), b"pc settings".to_vec()),
            ],
            trigger: Trigger::AutoExit,
            parent: None,
            over_all: false,
            device: ("d-pc", "DESKTOP"),
            platform: "pc:steam",
            comment: "",
            game_version: None,
            origin: None,
        },
    )
    .unwrap();
    sync::upload_pending(cloud, &steam).unwrap();
    sync::drop_pending(&steam).unwrap();

    let v = status3_of(SDV, &a, &users, &[(U1, Some(&s1), false)]);
    let g = game_for(&v, U1);
    assert_eq!(g["status"], "synced");
    assert_eq!((g["newer"]["label"].as_str(), g["newer"]["convert"].as_bool()), (Some("Steam"), Some(true)), "{g}");
    let src = sources_of(SDV, &a, U1);
    let from = item(&src, "platform").clone();
    assert_eq!((from["id"].as_str(), from["convert"].as_bool()), (Some(steam.id.as_str()), Some(true)));

    // Taking it: the Switch keeps its own first (already in the cloud), then
    // gets the PC's farm where the Switch keeps it, with its own options and
    // device save; the PC's settings stay on the PC.
    let (_, r) = upload_to(SDV, &a, &format!("user={U1}&backup=1&parent={s1}"), &mine);
    assert_eq!(r["existing"], true);
    let mut z = download_with(&a, json!({ "title": SDV, "id": steam.id, "user": U1, "line": from["line"], "base": s1 }));
    assert_eq!(read(&mut z, "nxsave-0100e65002bb8000/Farm_1/Farm_1"), "farm from the pc");
    assert_eq!(read(&mut z, "nxsave-0100e65002bb8000/Farm_1/SaveGameInfo"), "<pc/>");
    assert_eq!(read(&mut z, "nxsave-0100e65002bb8000/options.bin"), "switch options");
    assert_eq!(read(&mut z, "nxdevice-0100e65002bb8000/d.bin"), "device");
    assert_eq!(z.len(), 4, "nothing else: not the PC's settings");
    // The Switch writes it and makes it the newest of its line.
    let taken = zip_of(&[
        ("nxsave-0100e65002bb8000/Farm_1/Farm_1", b"farm from the pc"),
        ("nxsave-0100e65002bb8000/Farm_1/SaveGameInfo", b"<pc/>"),
        ("nxsave-0100e65002bb8000/options.bin", b"switch options"),
        ("nxdevice-0100e65002bb8000/d.bin", b"device"),
    ]);
    let (s, r) = upload_to(SDV, &a, &format!("user={U1}&force=1&from={}&fromLine={}&how=convert", steam.id, from["line"].as_str().unwrap()), &taken);
    assert_eq!(s, 200, "{r}");
    let s2 = id_of(&r);
    let v = status3_of(SDV, &a, &users, &[(U1, Some(&s2), false)]);
    assert_eq!(game_for(&v, U1)["status"], "synced");
    assert!(game_for(&v, U1)["newer"].is_null(), "taken: no longer newer");
}
