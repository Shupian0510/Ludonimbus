//! Games installed through Steam (docs/plan-v2.md, section 8): backup only,
//! taking over from Steam Cloud, handing back. Every device has a fake
//! Steam folder of its own in a temporary folder, and the cloud is in
//! memory. One test, because the data directory is chosen through an
//! environment variable.

use anyhow::Result;
use ludonimbus_core::cloud::memory::Memory;
use ludonimbus_core::cloud::store::{Entry, Store};
use ludonimbus_core::cloud::Cloud;
use ludonimbus_core::config::{self, AppConfig, GameEntry, StoreMode};
use ludonimbus_core::games::{self, FoundChoice};
use ludonimbus_core::snapshot::{self, SnapshotSummary, Trigger};
use ludonimbus_core::steam::{self, WrapperHealth};
use ludonimbus_core::steamcloud::{self, Polled, State};
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::{state, vdf};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const ACCOUNT: &str = "111111111";
/// The game the tests play.
const APPID: u32 = 1000;
const KEY: &str = "fake-game";
/// Installed too; nothing knows where it keeps its saves, and Steam has
/// noted no cloud sync of it: Steam Cloud doesn't keep it.
const OTHER: u32 = 2000;
/// Installed too; Ludusavi's manifest knows it, and Steam Cloud keeps it.
const KNOWN: u32 = 3000;

/// What the user had in the launch options, as Steam writes it in the file.
const USER_OPTIONS: &str = r#"-config "C:\My Games\fake.cfg""#;
const USER_OPTIONS_IN_FILE: &str = r#""-config \"C:\\My Games\\fake.cfg\"""#;

/// `localconfig.vdf` as Steam writes it; `SYNC` is the game's last sync
/// state. A block elsewhere is named like the game.
const LOCALCONFIG: &str = r#""UserLocalConfigStore"
{
	"friends"
	{
		"1000"
		{
			"name"		"someone"
		}
	}
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"1000"
					{
						"LastPlayed"		"1700000000"
						"Playtime"		"42"
						"LaunchOptions"		"-config \"C:\\My Games\\fake.cfg\""
						"cloud"
						{
							"last_sync_state"		"SYNC"
							"quota_bytes"		"1000000"
						}
						"autocloud"
						{
							"lastlaunch"		"1700000000"
							"lastexit"		"1700000100"
						}
					}
					"3000"
					{
						"cloud"
						{
							"last_sync_state"		"changesincloud"
						}
					}
				}
			}
		}
	}
}
"#;

/// The game's own Steam Cloud switch, off (in `sharedconfig.vdf`).
const GAME_OFF: &str = "\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"1000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"CloudEnabled\"\t\t\"0\"\n\t\t\t\t\t}\n\t\t\t\t}\n";
/// The account's Steam Cloud switch, off.
const ALL_OFF: &str = "\t\t\t\t\"CloudEnabled\"\t\t\"0\"\n";

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A store in memory that writes down every request.
#[derive(Clone, Default)]
struct Logged {
    inner: Arc<Memory>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Logged {
    fn note(&self, what: &str, path: &str) {
        self.log.lock().unwrap().push(format!("{what} {path}"));
    }

    /// The requests since the last call.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

impl Store for Logged {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn label(&self) -> String {
        "memory".to_string()
    }
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        self.note("list", dir);
        self.inner.list(dir)
    }
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        self.note("read", path);
        self.inner.read(path)
    }
    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        self.note("download", path);
        self.inner.download(path, dest)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.note("write", path);
        self.inner.write(path, data)
    }
    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        self.note("upload", path);
        self.inner.upload(path, src)
    }
    fn remove(&self, path: &str) -> Result<()> {
        self.note("remove", path);
        self.inner.remove(path)
    }
    fn cache_scope(&self) -> Option<String> {
        Some("steam-cloud-test".to_string())
    }
}

/// Requests that change nothing in the cloud.
fn only_reads(requests: &[String]) -> bool {
    requests.iter().all(|r| r.starts_with("list ") || r.starts_with("read "))
}

/// A Steam folder with three installed games and one account.
struct FakeSteam {
    root: PathBuf,
}

impl FakeSteam {
    fn new(dir: &Path) -> FakeSteam {
        let root = dir.join("Steam");
        fs::create_dir_all(root.join("config")).unwrap();
        for (appid, name) in [(APPID, "Fake Game"), (OTHER, "Other Game"), (KNOWN, "Known Game")] {
            let manifest = format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{name}\"\n}}\n");
            write(&root.join("steamapps").join(format!("appmanifest_{appid}.acf")), &manifest);
            write(&root.join("steamapps").join("common").join(name).join(format!("{}.exe", name.replace(' ', ""))), "exe");
        }
        let steam = FakeSteam { root };
        steam.set_sync_state("changesincloud");
        steam
    }

    fn userdata(&self) -> PathBuf {
        self.root.join("userdata").join(ACCOUNT)
    }

    fn set_sync_state(&self, state: &str) {
        write(&steam::localconfig_path(&self.root, ACCOUNT), &LOCALCONFIG.replace("SYNC", state));
    }

    /// `localconfig.vdf` as it is now.
    fn local(&self) -> String {
        fs::read_to_string(steam::localconfig_path(&self.root, ACCOUNT)).unwrap()
    }

    /// `sharedconfig.vdf` with `lines` in its Steam block.
    fn set_shared(&self, lines: &str) {
        let text = "\"UserRoamingConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\nLINES\t\t\t}\n\t\t}\n\t}\n}\n";
        write(&steam::sharedconfig_path(&self.root, ACCOUNT), &text.replace("LINES", lines));
    }

    fn launch_options(&self) -> Option<String> {
        steam::launch_options(&self.root, ACCOUNT, APPID).unwrap()
    }

    /// A save of the game in Steam's cloud folder.
    fn save(&self, file: &str, content: &str) {
        write(&self.userdata().join(APPID.to_string()).join("remote").join(file), content);
    }

    fn read_save(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.userdata().join(APPID.to_string()).join("remote").join(file)).ok()
    }
}

/// A device with a data folder and a Steam folder of its own.
struct Device {
    home: PathBuf,
    steam: FakeSteam,
    store: Logged,
}

impl Device {
    fn new(root: &Path, store: &Logged, name: &str) -> Device {
        let dev = Device { home: root.join(format!("home-{name}")), steam: FakeSteam::new(&root.join(format!("steam-{name}"))), store: store.clone() };
        fs::create_dir_all(&dev.home).unwrap();
        dev.activate();
        let mut cfg = AppConfig::default();
        cfg.device.name = name.to_string();
        cfg.language = "en".to_string();
        cfg.steam.root = Some(dev.steam.root.clone());
        cfg.steam.account = Some(ACCOUNT.to_string());
        config::update(|c| {
            *c = cfg.clone();
            Ok(())
        })
        .unwrap();
        dev
    }

    fn activate(&self) {
        std::env::set_var("LUDONIMBUS_HOME", &self.home);
    }

    fn cfg(&self) -> AppConfig {
        self.activate();
        config::load().unwrap()
    }

    /// This device, looking at the cloud anew.
    fn ctx(&self) -> Ctx {
        let cfg = self.cfg();
        let cloud = Cloud::new(Box::new(self.store.clone()), &cfg.device.id, &cfg.device.name);
        Ctx::with_cloud(cfg, cloud)
    }

    /// This device without a cloud.
    fn no_cloud(&self) -> Ctx {
        Ctx::from_config(self.cfg())
    }

    fn entry(&self) -> GameEntry {
        self.cfg().game(KEY).unwrap().clone()
    }

    fn mode(&self) -> StoreMode {
        self.entry().store.unwrap().mode
    }

    /// The game added as Steam installed it, its saves in Steam's own cloud
    /// folder.
    fn add(&self) -> GameEntry {
        let choice = FoundChoice { saves: vec![format!("{{steamuser}}/{APPID}/remote/**")], ..Default::default() };
        steamcloud::add(&self.cfg(), ACCOUNT, APPID, choice, false).unwrap()
    }

    fn sync_with(&self, resolution: Resolution) -> Outcome {
        let ctx = self.ctx();
        sync::sync(&ctx, &ctx.game(KEY).unwrap(), resolution, Trigger::Passive).unwrap()
    }

    fn sync(&self) -> Outcome {
        self.sync_with(Resolution::Auto)
    }

    fn versions(&self) -> Vec<SnapshotSummary> {
        self.ctx().cloud().unwrap().list_snapshots(KEY).unwrap()
    }

    fn files_of(&self, id: &str) -> Vec<String> {
        let mut files: Vec<String> = self.ctx().cloud().unwrap().get_meta(KEY, id).unwrap().files.into_iter().map(|f| f.path).collect();
        files.sort();
        files
    }

    fn take_over(&self, wrapper: bool) -> Result<GameEntry> {
        steamcloud::take_over(&self.ctx(), KEY, wrapper)
    }

    /// Taking over is refused: the error, after checking that nothing
    /// changed (Steam's file, the entry, the cloud).
    fn refused(&self, ctx: Ctx, wrapper: bool) -> String {
        let (local, entry, versions) = (self.steam.local(), self.entry(), self.versions().len());
        self.store.take();
        let err = format!("{:#}", steamcloud::take_over(&ctx, KEY, wrapper).unwrap_err());
        let requests = self.store.take();
        assert!(only_reads(&requests), "{err}: {requests:?}");
        assert_eq!(self.steam.local(), local, "{err}");
        let now = self.entry();
        assert_eq!((now.store, now.auto_sync), (entry.store, entry.auto_sync), "{err}");
        assert_eq!(self.versions().len(), versions, "{err}");
        err
    }
}

fn uploaded(outcome: Outcome) -> String {
    match outcome {
        Outcome::Uploaded { id } => id,
        other => panic!("expected an upload, got {other:?}"),
    }
}

fn head(dev: &Device) -> SnapshotSummary {
    sync::mainline_head(&dev.versions()).unwrap()
}

#[test]
fn games_installed_through_steam() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let runner = root.join("bin").join("ludonimbus-run.exe");
    write(&runner, "exe");
    std::env::set_var("LUDONIMBUS_RUNNER", &runner);
    launch_options_change_alone(root);
    backup_only(root);
    taking_over(root, &runner);
    without_steam_cloud(root, &runner);
    batches(root);
}

/// Only the one value of Steam's file changes, and it goes back byte for
/// byte; a copy is kept before every change.
fn launch_options_change_alone(root: &Path) {
    let home = root.join("home-files");
    std::env::set_var("LUDONIMBUS_HOME", &home);
    for (i, eol) in ["\n", "\r\n"].into_iter().enumerate() {
        let steam = FakeSteam::new(&root.join(format!("steam-files-{i}")));
        let original = LOCALCONFIG.replace("SYNC", "synchronized").replace('\n', eol);
        write(&steam::localconfig_path(&steam.root, ACCOUNT), &original);
        let (r, a) = (steam.root.as_path(), ACCOUNT);
        assert_eq!(steam.launch_options().as_deref(), Some(USER_OPTIONS));
        let wrapped = steam::wrapper_options(Path::new(r"C:\Program Files\Ludonimbus\ludonimbus-run.exe"), "game", USER_OPTIONS);
        steam::can_set_launch_options(r, a, APPID).unwrap();
        steam::set_launch_options(r, a, APPID, Some(&wrapped)).unwrap();
        assert_eq!(steam.launch_options().as_deref(), Some(wrapped.as_str()));
        assert_eq!(steam.local(), original.replace(USER_OPTIONS_IN_FILE, &vdf::quote_text(&wrapped)));
        // Back: the file as Steam wrote it.
        steam::set_launch_options(r, a, APPID, Some(USER_OPTIONS)).unwrap();
        assert_eq!(steam.local(), original);
        // None at all removes the line, and a game without a block gets one.
        steam::set_launch_options(r, a, APPID, None).unwrap();
        assert_eq!(steam.launch_options(), None);
        assert_eq!(steam.local(), original.replace(&format!("\t\t\t\t\t\t\"LaunchOptions\"\t\t{USER_OPTIONS_IN_FILE}{eol}"), ""));
        steam::set_launch_options(r, a, OTHER, Some("-novid")).unwrap();
        assert_eq!(steam::launch_options(r, a, OTHER).unwrap().as_deref(), Some("-novid"));
        assert!(vdf::text_well_formed(&steam.local()));
    }
    // Every change kept a copy of the file before it.
    assert_eq!(fs::read_dir(home.join("steam-backups")).unwrap().count(), 8);
    // A file Ludonimbus doesn't understand is left alone.
    let steam = FakeSteam::new(&root.join("steam-files-bad"));
    let bad = "\"UserLocalConfigStore\" { \"Software\" {";
    write(&steam::localconfig_path(&steam.root, ACCOUNT), bad);
    assert!(steam::can_set_launch_options(&steam.root, ACCOUNT, APPID).is_err());
    assert!(steam::set_launch_options(&steam.root, ACCOUNT, APPID, Some("-x")).is_err());
    assert_eq!(steam.local(), bad);
    // An account Steam has no settings of.
    assert!(steam::set_launch_options(&steam.root, "222222222", APPID, Some("-x")).is_err());
}

/// Steam Cloud keeps the saves: devices store versions, and nothing is
/// written into a game's saves by itself.
fn backup_only(root: &Path) {
    let store = Logged::default();

    // A: the game is added as Steam installed it.
    let a = Device::new(root, &store, "A");
    let entry = a.add();
    let link = entry.store.clone().unwrap();
    assert_eq!(entry.key, KEY);
    assert_eq!((link.account.as_str(), link.appid, link.mode, link.wrapper), (ACCOUNT, APPID, StoreMode::Backup, false));
    assert!(entry.backup_only() && entry.auto_sync);
    assert!(entry.exe.as_ref().unwrap().ends_with("FakeGame.exe"));
    assert_eq!(entry.install.as_deref(), Some(a.steam.root.join("steamapps").join("common").join("Fake Game").as_path()));
    assert!(steamcloud::add(&a.cfg(), ACCOUNT, APPID, FoundChoice { saves: vec!["{steamuser}/x/**".into()], ..Default::default() }, false).is_err());
    let apps = steamcloud::apps(&a.cfg());
    assert_eq!(apps.iter().map(|x| x.appid).collect::<Vec<_>>(), [APPID, KNOWN, OTHER]);
    assert_eq!(
        (apps[0].key.as_deref(), apps[0].state, apps[0].sync_state.as_deref(), apps[0].cloud_enabled),
        (Some(KEY), State::Backup, Some("changesincloud"), true)
    );
    assert_eq!((apps[1].key.as_deref(), apps[1].state), (None, State::Steam));
    // `{steamuser}` is this device's Steam folder.
    assert_eq!(snapshot::local_state(&entry).unwrap().roots.get("steamuser"), Some(&a.steam.userdata()));

    a.steam.save("slot1.sav", "a1");
    let first = uploaded(a.sync());
    assert_eq!(a.files_of(&first), ["steamuser/1000/remote/slot1.sav"]);

    // B: Steam in another place, the same account. Its saves differ from
    // the cloud's (Steam Cloud brought it further): uploaded on top without
    // a question, and none of its files changes.
    let b = Device::new(root, &store, "B");
    b.add();
    b.steam.save("slot1.sav", "b1");
    b.steam.save("slot2.sav", "b2");
    let roots = snapshot::local_state(&b.entry()).unwrap().roots;
    assert_eq!(roots.get("steamuser"), Some(&b.steam.userdata()));
    assert_ne!(b.steam.userdata(), a.steam.userdata());
    let from_b = uploaded(b.sync());
    assert_eq!(b.files_of(&from_b), ["steamuser/1000/remote/slot1.sav", "steamuser/1000/remote/slot2.sav"]);
    assert_eq!(head(&b).id, from_b);
    // The versions say they were made on the Steam version of the game.
    assert_eq!(head(&b).platform, "pc:steam");
    assert!(state::game(KEY).conflict.is_none());
    assert_eq!((b.steam.read_save("slot1.sav").as_deref(), b.steam.read_save("slot2.sav").as_deref()), (Some("b1"), Some("b2")));

    // A has older saves than the cloud: left as they are, nothing fetched.
    store.take();
    assert!(matches!(a.sync(), Outcome::UpToDate));
    assert!(store.take().iter().all(|r| !r.starts_with("download ")));
    assert_eq!(a.steam.read_save("slot1.sav").as_deref(), Some("a1"));
    assert_eq!(a.steam.read_save("slot2.sav"), None);
    // Until the user asks for the cloud's.
    assert!(matches!(a.sync_with(Resolution::UseCloud), Outcome::Pulled { ref id, .. } if *id == from_b));
    assert_eq!((a.steam.read_save("slot1.sav").as_deref(), a.steam.read_save("slot2.sav").as_deref()), (Some("b1"), Some("b2")));

    // B plays on; Steam Cloud brings the same saves to A, which then counts
    // as synced to B's version without a transfer.
    b.steam.save("slot1.sav", "b1-next");
    let next = uploaded(b.sync());
    a.steam.save("slot1.sav", "b1-next");
    let versions = a.versions().len();
    store.take();
    assert!(matches!(a.sync(), Outcome::UpToDate));
    assert!(store.take().iter().all(|r| !r.starts_with("download ")));
    assert_eq!(a.versions().len(), versions);
    assert_eq!(state::game(KEY).last_synced_id.as_deref(), Some(next.as_str()));

    // C has no saves yet: nothing is written into Steam's folder either.
    let c = Device::new(root, &store, "C");
    c.add();
    assert!(matches!(c.sync(), Outcome::NothingToSave));
    assert!(!c.steam.userdata().join(APPID.to_string()).exists());
}

/// Taking a game over from Steam Cloud: refused until it is safe, with
/// nothing changed; then managed; back to backup only by itself when Steam
/// Cloud reads on again; handed back with Steam's file as it was.
fn taking_over(root: &Path, runner: &Path) {
    let store = Logged::default();
    let t = Device::new(root, &store, "T");
    t.add();
    t.steam.save("slot1.sav", "t1");
    let before = uploaded(t.sync());

    // Steam's last sync wasn't complete.
    assert!(t.refused(t.ctx(), true).starts_with("STEAM_NOT_SYNCED:"));
    // Complete, but Steam Cloud is still on.
    t.steam.set_sync_state("synchronized");
    let original = t.steam.local();
    let err = t.refused(t.ctx(), true);
    assert!(err.starts_with("STEAM_CLOUD_ON:") && err.contains("Properties"), "{err}");
    // Off for the game. Still refused: without the wrapper program, while
    // the game is played, without a cloud to keep the saves in.
    t.steam.set_shared(GAME_OFF);
    std::env::set_var("LUDONIMBUS_RUNNER", root.join("gone").join("ludonimbus-run.exe"));
    assert!(t.refused(t.ctx(), true).contains("wrapper"));
    std::env::set_var("LUDONIMBUS_RUNNER", runner);
    let marker = t.home.join("running").join(format!("{KEY}.json"));
    write(&marker, &serde_json::json!({ "pid": std::process::id(), "phase": "playing" }).to_string());
    assert!(t.refused(t.ctx(), true).contains("running"));
    fs::remove_file(&marker).unwrap();
    t.refused(t.no_cloud(), true);

    // Taken over: the saves as they were are kept (the cloud's newest
    // version has them already), the launch options go through the wrapper
    // with the user's own behind it, and nothing else in Steam's file moved.
    let entry = t.take_over(true).unwrap();
    let link = entry.store.clone().unwrap();
    assert_eq!((link.mode, link.wrapper, link.original_options.as_deref()), (StoreMode::Managed, true, Some(USER_OPTIONS)));
    assert!(entry.auto_sync && !entry.backup_only());
    let versions = t.versions();
    assert_eq!(versions.len(), 1);
    assert!(versions[0].id == before && versions[0].locked && versions[0].comment == "Before taking over from Steam Cloud", "{versions:?}");
    let wrapped = steam::wrapper_options(runner, KEY, USER_OPTIONS);
    assert_eq!(t.steam.launch_options().as_deref(), Some(wrapped.as_str()));
    let w = steam::parse_wrapped(&wrapped).unwrap();
    assert_eq!((w.key.as_str(), w.original.as_str()), (KEY, USER_OPTIONS));
    assert_eq!(t.steam.local(), original.replace(USER_OPTIONS_IN_FILE, &vdf::quote_text(&wrapped)));
    let status = steamcloud::status(&t.cfg(), &t.entry()).unwrap();
    assert_eq!((status.mode, status.synchronized, status.cloud_enabled, status.global_enabled), (State::Managed, true, false, true));
    assert_eq!(status.wrapper, Some(WrapperHealth::Ok));

    // Managed, the game syncs both ways like any other.
    let u = Device::new(root, &store, "U");
    u.add();
    u.steam.save("slot1.sav", "u1");
    let from_u = uploaded(u.sync());
    assert!(matches!(t.sync(), Outcome::Pulled { ref id, .. } if *id == from_u));
    assert_eq!(t.steam.read_save("slot1.sav").as_deref(), Some("u1"));

    // Steam Cloud is turned on again in Steam. The next sync goes back to
    // backup only by itself and writes nothing; the wrapper stays.
    t.steam.set_shared("");
    u.steam.save("slot1.sav", "u2");
    uploaded(u.sync());
    assert!(matches!(t.sync(), Outcome::UpToDate));
    assert_eq!(t.steam.read_save("slot1.sav").as_deref(), Some("u1"));
    assert_eq!(t.mode(), StoreMode::Backup);
    assert_eq!(t.steam.launch_options().as_deref(), Some(wrapped.as_str()));

    // Taken over again (the options aren't wrapped twice); the wrapper's
    // check before launching finds Steam Cloud on.
    t.steam.set_shared(GAME_OFF);
    t.steam.save("slot1.sav", "t2");
    t.take_over(true).unwrap();
    assert_eq!(t.steam.launch_options().as_deref(), Some(wrapped.as_str()));
    let kept = t.versions().into_iter().filter(|v| v.locked).count();
    assert_eq!(kept, 2, "the saves before taking over again are kept too");
    assert_eq!(steamcloud::recheck(&t.cfg(), &t.entry()).unwrap().map(|e| e.store.unwrap().mode), None);
    t.steam.set_shared("");
    let demoted = steamcloud::recheck(&t.cfg(), &t.entry()).unwrap().expect("Steam Cloud reads on");
    assert_eq!(demoted.store.clone().unwrap().mode, StoreMode::Backup);
    assert_eq!(t.mode(), StoreMode::Backup);
    assert!(steamcloud::recheck(&t.cfg(), &t.entry()).unwrap().is_none());

    // Handed back: Steam's file exactly as it was.
    let entry = steamcloud::hand_back(&t.ctx(), KEY).unwrap();
    let link = entry.store.unwrap();
    assert_eq!((link.mode, link.wrapper, link.original_options), (StoreMode::Backup, false, None));
    assert_eq!(t.steam.local(), original);

    // Removing a game gives the launch options back too, also when what
    // the user had is only known from the wrapped options themselves.
    t.steam.set_shared(GAME_OFF);
    t.take_over(true).unwrap();
    config::update(|c| {
        c.game_mut(KEY).unwrap().store.as_mut().unwrap().original_options = None;
        Ok(())
    })
    .unwrap();
    games::remove(KEY).unwrap();
    assert!(t.cfg().game(KEY).is_none());
    assert_eq!(t.steam.local(), original);

    // Without the wrapper: the launch options stay, and the background keeps
    // the game up to date while it isn't running. Added anew, with the
    // saves Steam Cloud brought (the line's newest version).
    t.add();
    t.steam.save("slot1.sav", "u2");
    let entry = t.take_over(false).unwrap();
    let link = entry.store.unwrap();
    assert_eq!((link.mode, link.wrapper), (StoreMode::Managed, false));
    assert_eq!(t.steam.local(), original);
    u.steam.save("slot1.sav", "u3");
    let latest = uploaded(u.sync());
    match steamcloud::poll(&t.ctx(), &t.entry()).unwrap() {
        Polled::Synced(Outcome::Pulled { id, .. }) => assert_eq!(id, latest),
        other => panic!("expected a pull, got {other:?}"),
    }
    assert_eq!(t.steam.read_save("slot1.sav").as_deref(), Some("u3"));
    let ctx = t.ctx();
    store.take();
    assert!(matches!(steamcloud::poll(&ctx, &t.entry()).unwrap(), Polled::Nothing));
    let requests = store.take();
    assert!(only_reads(&requests) && requests.iter().filter(|r| r.starts_with("list ")).count() == 1, "{requests:?}");
    t.steam.set_shared("");
    assert!(matches!(steamcloud::poll(&t.ctx(), &t.entry()).unwrap(), Polled::Demoted(_)));
    assert_eq!(t.mode(), StoreMode::Backup);
}

/// A game Steam Cloud doesn't keep (Steam has noted no cloud sync of it) is
/// managed from the start: nothing to take over from, no switch to turn
/// off. When Steam starts syncing it, it is backup only like any other.
fn without_steam_cloud(root: &Path, runner: &Path) {
    let store = Logged::default();
    let n = Device::new(root, &store, "N");
    let choice = || FoundChoice { saves: vec!["{install}/save/**".to_string()], ..Default::default() };
    let entry = steamcloud::add(&n.cfg(), ACCOUNT, OTHER, choice(), false).unwrap();
    let key = entry.key.clone();
    assert_eq!(entry.store.as_ref().unwrap().mode, StoreMode::Managed);
    let status = steamcloud::status(&n.cfg(), &entry).unwrap();
    assert_eq!((status.mode, status.cloud_used, status.sync_state.as_deref()), (State::Managed, false, None));
    let app = steamcloud::apps(&n.cfg()).into_iter().find(|a| a.appid == OTHER).unwrap();
    assert_eq!((app.state, app.cloud_used), (State::Managed, false));
    // It syncs both ways like a game that isn't Steam's.
    let save = n.steam.root.join("steamapps").join("common").join("Other Game").join("save").join("slot1.sav");
    write(&save, "n1");
    let sync = |d: &Device| {
        let ctx = d.ctx();
        sync::sync(&ctx, &ctx.game(&key).unwrap(), Resolution::Auto, Trigger::Passive).unwrap()
    };
    let first = uploaded(sync(&n));
    let m = Device::new(root, &store, "M");
    steamcloud::add(&m.cfg(), ACCOUNT, OTHER, choice(), false).unwrap();
    match sync(&m) {
        Outcome::Pulled { id, .. } => assert_eq!(id, first),
        other => panic!("expected a pull, got {other:?}"),
    }
    let theirs = m.steam.root.join("steamapps").join("common").join("Other Game").join("save").join("slot1.sav");
    assert_eq!(fs::read_to_string(&theirs).unwrap(), "n1");
    // What something else knows counts: a scan that found its saves in
    // Steam's cloud folder adds it backup only.
    games::remove(&key).unwrap();
    let kept = steamcloud::add(&m.cfg(), ACCOUNT, OTHER, choice(), true).unwrap();
    assert_eq!(kept.store.unwrap().mode, StoreMode::Backup);

    // Through the wrapper, without a switch to turn off or a sync of
    // Steam's to wait for.
    n.activate();
    let entry = steamcloud::take_over(&n.ctx(), &key, true).unwrap();
    let link = entry.store.clone().unwrap();
    assert_eq!((link.mode, link.wrapper, link.original_options), (StoreMode::Managed, true, None));
    let options = steam::launch_options(&n.steam.root, ACCOUNT, OTHER).unwrap().unwrap();
    assert_eq!(steam::wrapper_health(Some(&options), &key, runner), WrapperHealth::Ok);
    assert!(steamcloud::recheck(&n.cfg(), &entry).unwrap().is_none());
    // Steam notes a cloud sync of it (the game got Steam Cloud): backup only
    // from the next look on.
    let path: Vec<&str> = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", "2000", "cloud", "last_sync_state"].to_vec();
    let noted = vdf::set_text_value(&n.steam.local(), &path, Some("synchronized")).unwrap();
    write(&steam::localconfig_path(&n.steam.root, ACCOUNT), &noted);
    let demoted = steamcloud::recheck(&n.cfg(), &entry).unwrap().expect("backup only again");
    assert_eq!(demoted.store.unwrap().mode, StoreMode::Backup);
    // Handed back: no launch options again, as before the wrapper.
    steamcloud::hand_back(&n.ctx(), &key).unwrap();
    assert_eq!(steam::launch_options(&n.steam.root, ACCOUNT, OTHER).unwrap(), None);
}

/// All backup only; all taken over (only with Steam Cloud off for every
/// game); all handed back.
fn batches(root: &Path) {
    let store = Logged::default();
    let d = Device::new(root, &store, "D");
    write(&d.home.join("ludusavi").join("manifest.yaml"), "Known Game:\n  steam:\n    id: 3000\n");
    let done = steamcloud::backup_all(&d.cfg());
    let by = |appid: u32| done.iter().find(|x| x.appid == appid).unwrap_or_else(|| panic!("{appid}: {done:?}")).clone();
    assert_eq!(done.len(), 3);
    // Nothing knows where the other two keep their saves.
    for appid in [APPID, OTHER] {
        assert!(by(appid).key.is_none() && by(appid).error.as_deref().is_some_and(|e| e.contains("save location")), "{done:?}");
    }
    assert_eq!((by(KNOWN).key.as_deref(), by(KNOWN).error.as_deref()), (Some("known-game"), None));
    let known = d.cfg().game("known-game").unwrap().clone();
    assert_eq!((known.ludusavi_title.as_deref(), known.store.unwrap().mode), (Some("Known Game"), StoreMode::Backup));
    // Already in the library: not added again.
    assert!(steamcloud::backup_all(&d.cfg()).iter().all(|x| x.appid != KNOWN));

    d.add();
    d.steam.save("slot1.sav", "d1");
    d.steam.set_sync_state("synchronized");
    // The game's switch off is not enough for all at once.
    d.steam.set_shared(GAME_OFF);
    assert!(format!("{:#}", steamcloud::take_over_all(&d.ctx(), true).unwrap_err()).starts_with("STEAM_CLOUD_ON:"));
    assert_eq!(d.mode(), StoreMode::Backup);
    let original = d.steam.local();
    // The account's switch off: every game that is safe to take over is.
    d.steam.set_shared(ALL_OFF);
    let done = steamcloud::take_over_all(&d.ctx(), true).unwrap();
    let by = |appid: u32| done.iter().find(|x| x.appid == appid).unwrap_or_else(|| panic!("{appid}: {done:?}"));
    assert_eq!((by(APPID).key.as_deref(), by(APPID).error.as_deref()), (Some(KEY), None));
    assert!(by(KNOWN).error.as_deref().is_some_and(|e| e.starts_with("STEAM_NOT_SYNCED:")), "{done:?}");
    assert_eq!(d.mode(), StoreMode::Managed);
    assert_eq!(d.cfg().game("known-game").unwrap().store.as_ref().unwrap().mode, StoreMode::Backup);
    assert_ne!(d.steam.local(), original);

    let done = steamcloud::hand_back_all(&d.ctx());
    assert_eq!(done.len(), 1);
    assert!(done[0].error.is_none(), "{done:?}");
    assert_eq!(d.mode(), StoreMode::Backup);
    assert_eq!(d.steam.local(), original);
}
