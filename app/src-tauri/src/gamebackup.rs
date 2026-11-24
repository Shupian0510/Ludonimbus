//! Game backups and the cloud speed test (docs/plan-game-backup.md).
//!
//! Backups, restores and deletions run as background jobs in the tray
//! process (closing the window doesn't stop them). Progress goes out as
//! `game-backup-progress` events, the end as `game-backup-finished`.

use crate::commands::{blocking, CmdResult};
use crate::{show_main, watcher};
use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config::{self, GameEntry};
use ludonimbus_core::gamebackup::format::{Kind, Manifest, Summary};
use ludonimbus_core::gamebackup::{self, backup, components, install, remote, restore, scan, speedtest};
use ludonimbus_core::{logger, process, tr};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

fn log(msg: impl AsRef<str>) {
    logger::log("gamebackup", msg);
}

// ---------------------------------------------------------------------------
// Speed test

static SPEED_CANCEL: AtomicBool = AtomicBool::new(false);
static SPEED_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn speed_test(app: AppHandle, mb: Option<u64>, threads: Option<usize>) -> CmdResult<speedtest::SpeedReport> {
    if SPEED_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(tr!("测速已经在进行中", "A speed test is already running"));
    }
    SPEED_CANCEL.store(false, Ordering::SeqCst);
    let result = blocking(move || {
        let cfg = config::load()?;
        let provider = gamebackup::open(&cfg)?;
        let mut opts = speedtest::SpeedOptions::default();
        if let Some(mb) = mb {
            opts.total_bytes = mb.clamp(16, 2048) * 1024 * 1024;
        }
        if let Some(t) = threads {
            opts.threads = t.clamp(1, 16);
        }
        let throttle = Throttle::new();
        let progress = |p: &speedtest::SpeedProgress| {
            if throttle.ready(p.done_bytes == 0 || p.done_bytes == p.total_bytes) {
                let _ = app.emit("speed-test-progress", p);
            }
        };
        speedtest::run(provider.files(), &opts, &SPEED_CANCEL, &progress)
    })
    .await;
    SPEED_RUNNING.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub async fn speed_test_cancel() -> CmdResult<()> {
    SPEED_CANCEL.store(true, Ordering::SeqCst);
    Ok(())
}

/// At most a few progress events per second.
struct Throttle(Mutex<Instant>);

impl Throttle {
    fn new() -> Self {
        Throttle(Mutex::new(Instant::now() - Duration::from_secs(1)))
    }
    fn ready(&self, force: bool) -> bool {
        let mut last = self.0.lock().unwrap();
        if force || last.elapsed() >= Duration::from_millis(250) {
            *last = Instant::now();
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Jobs

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    /// backup | restore | delete
    pub kind: &'static str,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub uploaded: u64,
    pub elapsed_secs: f64,
    /// Where a restore goes.
    pub target: Option<String>,
}

struct Job {
    view: JobView,
    cancel: Arc<AtomicBool>,
}

fn jobs() -> &'static Mutex<HashMap<String, Job>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn job(key: &str) -> Option<JobView> {
    jobs().lock().unwrap().get(key).map(|j| j.view.clone())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Finished {
    key: String,
    kind: &'static str,
    ok: bool,
    cancelled: bool,
    message: String,
    outcome: Option<Value>,
}

/// Run `work` as the job of `key` on a background thread.
fn start_job(
    app: &AppHandle,
    key: &str,
    kind: &'static str,
    target: Option<String>,
    work: impl FnOnce(&(dyn Fn(&str, u64, u64, u64, f64) + Sync), &AtomicBool) -> Result<(String, Option<Value>)> + Send + 'static,
) -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut all = jobs().lock().unwrap();
        if all.contains_key(key) {
            bail!(tr!("这个游戏已经有备份或恢复在进行", "A backup or restore of this game is already running"));
        }
        let view = JobView { kind, phase: "starting".into(), done: 0, total: 0, uploaded: 0, elapsed_secs: 0.0, target };
        all.insert(key.to_string(), Job { view, cancel: cancel.clone() });
    }
    log(format!("{key}: {kind} started"));
    let app = app.clone();
    let key = key.to_string();
    std::thread::spawn(move || {
        let throttle = Throttle::new();
        let progress = |phase: &str, done: u64, total: u64, uploaded: u64, elapsed: f64| {
            let view = {
                let mut all = jobs().lock().unwrap();
                let Some(j) = all.get_mut(&key) else { return };
                let changed = j.view.phase != phase;
                j.view.phase = phase.to_string();
                j.view.done = done;
                j.view.total = total;
                j.view.uploaded = uploaded;
                j.view.elapsed_secs = elapsed;
                if !throttle.ready(changed) {
                    return;
                }
                j.view.clone()
            };
            let _ = app.emit("game-backup-progress", json!({ "key": key, "job": view }));
        };
        let result = work(&progress, &cancel);
        jobs().lock().unwrap().remove(&key);
        let finished = match result {
            Ok((message, outcome)) => Finished { key: key.clone(), kind, ok: true, cancelled: false, message, outcome },
            Err(e) => Finished {
                key: key.clone(),
                kind,
                ok: false,
                cancelled: gamebackup::is_cancelled(&e),
                message: format!("{e:#}"),
                outcome: None,
            },
        };
        log(format!("{key}: {kind} finished: ok={} {}", finished.ok, finished.message));
        let _ = app.emit("game-backup-finished", finished);
        let _ = app.emit("library-changed", ());
    });
    Ok(())
}

#[tauri::command]
pub async fn game_backup_cancel(key: String) -> CmdResult<()> {
    if let Some(j) = jobs().lock().unwrap().get(&key) {
        j.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn game_backup_job(key: String) -> CmdResult<Option<JobView>> {
    Ok(job(&key))
}

// ---------------------------------------------------------------------------
// Info

/// Where a restore goes by default and what is already on this PC.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreDefaults {
    /// The version has a program (or whole folder) part.
    has_program: bool,
    /// The version has the game's own files.
    has_content: bool,
    program_name: Option<String>,
    program_version: Option<String>,
    /// A folder on this PC with this game's program in it.
    installed_program: Option<String>,
    /// That program is the backed-up one (same exe).
    installed_same: bool,
    /// Where the program goes by default (the backed-up folder when it is free).
    program_dir: Option<String>,
    /// Where the game's files go by default: where they were, else where this
    /// emulator's games were last restored to.
    content_dir: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    version: String,
    targets: restore::Targets,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    has_cloud: bool,
    cloud_kind: String,
    /// Baidu Netdisk / Quark: works but slow (show the warning).
    slow: bool,
    /// The program's folder (or the whole folder) on this PC.
    root: Option<String>,
    root_exists: bool,
    root_custom: bool,
    exe_missing: bool,
    /// Newest first.
    versions: Vec<Summary>,
    /// About how much this game's backups take in the cloud.
    stored_bytes: u64,
    /// Versions kept after a backup; 0 = all.
    keep: usize,
    /// The user's own exclude patterns.
    exclude: Vec<String>,
    default_exclude: Vec<String>,
    /// The game's rule lists keys / firmware / BIOS files.
    firmware_available: bool,
    firmware: bool,
    skip: Vec<Kind>,
    extra_content: Vec<String>,
    /// The game's rule describes its parts (emulators, recompilations);
    /// otherwise the whole folder is backed up.
    by_parts: bool,
    job: Option<JobView>,
    pending_restore: Option<PendingView>,
    restore: Option<RestoreDefaults>,
    /// Problem reaching the cloud (the local part is still filled in).
    error: Option<String>,
}

fn game(key: &str) -> Result<GameEntry> {
    config::load()?.require_game(key).cloned()
}

/// A folder that is free for a restore: its parent exists and it is missing or empty.
fn free_dir(p: &str) -> Option<String> {
    let path = Path::new(p);
    let parent_ok = path.parent().is_some_and(|d| d.exists());
    let empty = std::fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(true);
    (parent_ok && empty).then(|| p.to_string())
}

fn file_hash(p: &Path) -> Option<String> {
    restore::hash_file(p).ok()
}

fn restore_defaults(cfg: &config::AppConfig, g: &GameEntry, m: &Manifest) -> RestoreDefaults {
    let program = m.components.iter().find(|c| matches!(c.kind, Kind::Program | Kind::Folder));
    let content = m.component(Kind::Content);
    // This game's program folder, or another game's with the same emulator.
    let installed: Option<PathBuf> = std::iter::once(g)
        .chain(cfg.games.iter().filter(|o| o.key != g.key && o.adapter.is_some() && o.adapter == g.adapter))
        .filter(|o| o.exe.as_ref().is_some_and(|e| e.is_file()))
        .find_map(ludonimbus_core::spec::install_dir);
    // The exe tells the versions apart; for a game its launcher builds, the launcher.
    let installed_same = match (program, &installed) {
        (Some(p), Some(dir)) => p.build.as_deref().or(p.exe.as_deref()).is_some_and(|exe| {
            let backed = p.files.iter().find(|f| f.p == exe).map(|f| f.h.clone());
            let local = components::split(exe).and_then(|(_, rest)| restore::safe_join(dir, rest).ok()).and_then(|f| file_hash(&f));
            backed.is_some() && backed == local
        }),
        _ => false,
    };
    let program_dir = installed
        .as_ref()
        .map(|d| d.display().to_string())
        .or_else(|| program.and_then(|p| p.bases.get("program")).and_then(|d| free_dir(d)));
    let content_dir = content.and_then(|c| c.bases.get("content")).filter(|d| Path::new(d).is_dir()).cloned().or_else(|| {
        g.adapter.as_deref().and_then(gamebackup::content_dir_for).map(|d| d.display().to_string())
    });
    RestoreDefaults {
        has_program: program.is_some(),
        has_content: content.is_some(),
        program_name: program.map(|p| p.name.clone()),
        program_version: program.and_then(|p| p.version.clone()),
        installed_program: installed.map(|d| d.display().to_string()),
        installed_same,
        program_dir,
        content_dir,
    }
}

fn info(key: &str) -> Result<Info> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    let s = gamebackup::settings(key);
    let root = gamebackup::backup_root(&g, &s);
    let adapter = g.adapter.as_deref().and_then(ludonimbus_core::adapters::find);
    let mut info = Info {
        has_cloud: cfg.cloud != config::CloudConfig::None,
        cloud_kind: ludonimbus_core::cloud::kind_id(&cfg.cloud).to_string(),
        slow: false,
        root_exists: root.as_ref().is_some_and(|r| r.is_dir()),
        root_custom: s.root.is_some(),
        exe_missing: !g.exe.as_ref().is_some_and(|e| e.exists()),
        root: root.map(|r| r.display().to_string()),
        versions: Vec::new(),
        stored_bytes: 0,
        keep: s.keep_count().unwrap_or(0),
        exclude: s.exclude.clone().unwrap_or_default(),
        default_exclude: scan::DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
        firmware_available: adapter.is_some_and(|a| !a.firmware.is_empty()),
        firmware: s.firmware,
        skip: s.skip.clone(),
        extra_content: s.extra_content.iter().map(|p| p.display().to_string()).collect(),
        by_parts: adapter.is_some(),
        job: job(key),
        pending_restore: restore::pending(key).map(|(version, targets)| PendingView { version, targets }),
        restore: None,
        error: None,
    };
    info.slow = gamebackup::is_slow_cloud(&info.cloud_kind);
    if !info.has_cloud {
        return Ok(info);
    }
    let cloud = (|| -> Result<(Vec<Summary>, Option<Manifest>)> {
        let provider = gamebackup::open(&cfg)?;
        let store = provider.files();
        let versions = remote::list_versions(store, key)?;
        let latest = match versions.last() {
            Some(v) => Some(remote::read_manifest(store, key, v)?),
            None => None,
        };
        Ok((versions, latest))
    })();
    match cloud {
        Ok((versions, latest)) => {
            if let Some(m) = &latest {
                info.exclude = gamebackup::user_excludes(&s, Some(m));
                info.restore = Some(restore_defaults(&cfg, &g, m));
            }
            if let Some(newest) = versions.last() {
                info.stored_bytes = newest.stored_size + versions[..versions.len() - 1].iter().map(|v| v.new_bytes).sum::<u64>();
            }
            info.versions = versions.into_iter().rev().collect();
        }
        Err(e) => info.error = Some(format!("{e:#}")),
    }
    Ok(info)
}

#[tauri::command]
pub async fn game_backup_info(key: String) -> CmdResult<Info> {
    blocking(move || info(&key)).await
}

/// What a backup would contain, by part, from this PC only.
#[tauri::command]
pub async fn game_backup_preview(key: String) -> CmdResult<Vec<backup::PreviewComponent>> {
    blocking(move || {
        let g = game(&key)?;
        let s = gamebackup::settings(&key);
        backup::preview(&g, &gamebackup::options(&s, None))
    })
    .await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    /// `""` = back to the game's folder.
    root: Option<String>,
    exclude: Option<Vec<String>>,
    /// 0 = keep all.
    keep: Option<usize>,
    firmware: Option<bool>,
    skip: Option<Vec<Kind>>,
    extra_content: Option<Vec<String>>,
}

#[tauri::command]
pub async fn game_backup_set(key: String, patch: SettingsPatch) -> CmdResult<()> {
    blocking(move || {
        game(&key)?;
        gamebackup::update_settings(&key, |s| {
            if let Some(r) = patch.root {
                s.root = (!r.trim().is_empty()).then(|| PathBuf::from(r.trim()));
            }
            if let Some(e) = patch.exclude {
                s.exclude = Some(e.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
            }
            if let Some(k) = patch.keep {
                s.keep = Some(k.min(20));
            }
            if let Some(f) = patch.firmware {
                s.firmware = f;
            }
            if let Some(k) = patch.skip {
                s.skip = k;
            }
            if let Some(x) = patch.extra_content {
                s.extra_content = x.into_iter().filter(|p| !p.trim().is_empty()).map(PathBuf::from).collect();
            }
        })?;
        Ok(())
    })
    .await
}

/// Pick any file (a ROM the rules don't know).
#[tauri::command]
pub async fn game_backup_pick_file(start: Option<String>) -> CmdResult<Option<String>> {
    blocking(move || {
        let mut d = rfd::FileDialog::new().set_title(tr!("选择属于这个游戏的文件", "Choose a file that belongs to this game"));
        if let Some(s) = start.map(PathBuf::from).and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf)) {
            d = d.set_directory(s);
        }
        Ok(d.pick_file().map(|p| p.display().to_string()))
    })
    .await
}

// ---------------------------------------------------------------------------
// Backup / restore / delete

fn running(g: &GameEntry) -> bool {
    watcher::runner_active(&g.key) || process::is_running(&watcher::watch_names(g))
}

#[tauri::command]
pub async fn game_backup_start(app: AppHandle, key: String) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?.clone();
        if running(&g) {
            bail!(tr!("游戏正在运行，请先退出游戏再备份", "The game is running. Quit it before backing up"));
        }
        let s = gamebackup::settings(&key);
        let device = cfg.device.clone();
        start_job(&app, &key.clone(), "backup", None, move |progress, cancel| {
            let provider = gamebackup::open(&cfg)?;
            let store = provider.files();
            let latest = if s.exclude.is_none() { gamebackup::latest(store, &key).ok().flatten() } else { None };
            let opts = backup::BackupOptions {
                components: gamebackup::options(&s, latest.as_ref().map(|(_, m)| m)),
                threads: gamebackup::default_threads(store.kind()),
                params: Default::default(),
                keep: s.keep_count(),
                comment: String::new(),
            };
            let out = backup::backup(store, &g, &device, &opts, cancel, &|p| {
                progress(p.phase, p.done, p.total, p.uploaded, p.elapsed_secs)
            })?;
            let message = match &out {
                backup::Outcome::Unchanged { .. } => tr!("游戏文件没有变化，不需要更新备份", "The game files haven't changed; the backup is up to date"),
                backup::Outcome::Created { uploaded, .. } => {
                    tr!("备份完成，上传了 {} MB", "Backup finished; {} MB uploaded", uploaded / 1_048_576)
                }
            };
            Ok((message, serde_json::to_value(&out).ok()))
        })
    })
    .await
}

/// The folder a restore into `chosen` would use (see `restore::resolve_target`).
#[tauri::command]
pub async fn game_backup_resolve_target(chosen: String, original: String) -> CmdResult<String> {
    Ok(restore::resolve_target(Path::new(&chosen), &original).display().to_string())
}

#[tauri::command]
pub async fn game_backup_restore(app: AppHandle, key: String, version: String, targets: restore::Targets, fix: bool) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        let g = cfg.require_game(&key)?.clone();
        if running(&g) {
            bail!(tr!("游戏正在运行，请先退出游戏再恢复", "The game is running. Quit it before restoring"));
        }
        let shown = targets.content.as_ref().unwrap_or(&targets.program).display().to_string();
        start_job(&app, &key.clone(), "restore", Some(shown), move |progress, cancel| {
            let provider = gamebackup::open(&cfg)?;
            let store = provider.files();
            let summary = remote::list_versions(store, &key)?
                .into_iter()
                .find(|s| s.id == version)
                .ok_or_else(|| anyhow!(tr!("云端已经没有这个版本了", "This version is no longer in the cloud")))?;
            let threads = gamebackup::default_threads(store.kind());
            let (m, report) = restore::restore(store, &key, &summary, &targets, threads, cancel, &|p| {
                progress(p.phase, p.done, p.total, 0, p.elapsed_secs)
            })?;
            let mut notes = vec![if report.kept > 0 {
                tr!("恢复完成：写入 {} 个文件，{} 个已经在本机，没有改动", "Restored: {} files written, {} were already here and left as they are", report.files, report.kept)
            } else {
                tr!("恢复完成：写入 {} 个文件", "Restored: {} files written", report.files)
            }];
            let mut fixed = None;
            if fix {
                progress("fixing", 0, 0, 0, 0.0);
                match install::fix_paths(&key, &m, &targets) {
                    Ok(f) => {
                        match f.shortcut {
                            install::ShortcutFix::NeedsSteamClosed => notes.push(tr!(
                                "Steam 正在运行，快捷方式还指向旧位置：关闭 Steam 后在这里点「更新 Steam 快捷方式」",
                                "Steam is running, so its shortcut still points at the old place: close Steam, then click \"Update the Steam shortcut\" here"
                            )),
                            install::ShortcutFix::Failed(ref e) => {
                                notes.push(tr!("没能更新 Steam 快捷方式：{e}", "Couldn't update the Steam shortcut: {e}"))
                            }
                            _ => {}
                        }
                        fixed = serde_json::to_value(&f).ok();
                    }
                    Err(e) => notes.push(tr!("没能修正游戏路径：{e:#}", "Couldn't fix the game's paths: {e:#}")),
                }
                progress("saves", 0, 0, 0, 0.0);
                match install::pull_saves(&key) {
                    Ok(_) => notes.push(tr!("已拉取云存档", "Saves pulled from the cloud")),
                    Err(e) => notes.push(tr!("存档没有拉取：{e:#}", "Saves weren't pulled: {e:#}")),
                }
            }
            Ok((notes.join("\n"), Some(json!({ "report": report, "fix": fixed }))))
        })
    })
    .await
}

#[tauri::command]
pub async fn game_backup_delete(app: AppHandle, key: String, version: String) -> CmdResult<()> {
    blocking(move || {
        let cfg = config::load()?;
        cfg.require_game(&key)?;
        let device = cfg.device.clone();
        start_job(&app, &key.clone(), "delete", None, move |progress, _cancel| {
            let provider = gamebackup::open(&cfg)?;
            let store = provider.files();
            let lock = remote::Lock::acquire(store, &key, &device.id, &device.name, "cleanup")?;
            remote::delete_version(store, &key, &version)?;
            let gc = remote::collect_garbage(store, (&device.id, &device.name), Some(&key), &|d, t| progress("cleaning", d as u64, t as u64, 0, 0.0))?;
            lock.release()?;
            let message = if gc.postponed {
                tr!("已删除这个版本；有设备正在备份，不再使用的数据以后再清理", "Version deleted; a device is backing up, so unused data is cleaned up later")
            } else {
                tr!("已删除这个版本，释放了 {} MB", "Version deleted; {} MB freed", gc.freed_bytes / 1_048_576)
            };
            Ok((message, serde_json::to_value(&gc).ok()))
        })
    })
    .await
}

// ---------------------------------------------------------------------------
// Steam plugin (through the local API)

fn pending_install() -> &'static Mutex<Option<String>> {
    static P: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(None))
}

/// The plugin asked to install this game: the game page opens the restore dialog.
#[tauri::command]
pub async fn game_backup_take_install(key: String) -> CmdResult<bool> {
    let mut p = pending_install().lock().unwrap();
    if p.as_deref() == Some(key.as_str()) {
        *p = None;
        return Ok(true);
    }
    Ok(false)
}

struct Cached {
    at: Instant,
    latest: Option<Summary>,
}

fn cache() -> &'static Mutex<HashMap<String, Cached>> {
    static C: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refreshing() -> &'static Mutex<std::collections::HashSet<String>> {
    static R: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Default::default()))
}

/// `GET /v1/games/{key}/gamebackup`: whether the game's files are missing,
/// the newest backup (looked up in the background, cached 5 minutes) and
/// the running job. Never waits for the cloud.
pub fn agent_status(key: &str) -> Result<Value> {
    let cfg = config::load()?;
    let g = cfg.require_game(key)?.clone();
    let exe_missing = !g.exe.as_ref().is_some_and(|e| e.exists());
    let cached = cache().lock().unwrap().get(key).map(|c| (c.at, c.latest.clone()));
    let stale = cached.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(300));
    if stale && exe_missing && cfg.cloud != config::CloudConfig::None && refreshing().lock().unwrap().insert(key.to_string()) {
        let key = key.to_string();
        std::thread::spawn(move || {
            let latest = gamebackup::open(&cfg).and_then(|p| remote::list_versions(p.files(), &key)).map(|mut v| v.pop());
            if let Ok(latest) = latest {
                cache().lock().unwrap().insert(key.clone(), Cached { at: Instant::now(), latest });
            }
            refreshing().lock().unwrap().remove(&key);
        });
    }
    let latest = cached.and_then(|(_, l)| l);
    Ok(json!({
        "exeMissing": exe_missing,
        "checked": cache().lock().unwrap().contains_key(key),
        "latest": latest.map(|s| json!({ "versionId": s.id, "createdAt": s.created_at, "totalSize": s.total_size, "deviceName": s.device_name })),
        "job": job(key),
        "lang": ludonimbus_core::i18n::current().code(),
    }))
}

/// `POST /v1/games/{key}/gamebackup/install`: show the game in Ludonimbus
/// with the restore dialog open (the folder is chosen there).
pub fn agent_install(app: &AppHandle, key: &str) -> Result<Value> {
    game(key)?;
    *pending_install().lock().unwrap() = Some(key.to_string());
    show_main(app);
    let _ = app.emit("open-game", key.to_string());
    let _ = app.emit("game-backup-install", key.to_string());
    Ok(json!({ "ok": true }))
}
