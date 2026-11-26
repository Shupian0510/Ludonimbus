//! `ludonimbus-run`: what a Steam shortcut launches once auto-sync is on.
//!
//! ```text
//! ludonimbus-run --game <key>                 # exe/args come from Ludonimbus config
//! ludonimbus-run --game <key> -- <cmd...>     # explicit command (e.g. Steam %command%)
//! ```
//! 1. sync before launch (pull newer cloud saves, ask on conflict), and offer
//!    newer saves of another version of the game (save library)
//! 2. start the game and wait for the whole process tree to exit
//! 3. sync after exit (upload new saves); without the tray app, also finish
//!    the sessions of terminated wrappers and upload screenshots
//!
//! Questions are system dialogs on the desktop and a full-screen window on
//! handhelds (`ludonimbus-app --prompt`, see `core::prompt`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config::{self, GameEntry};
use ludonimbus_core::prompt::Request;
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::{catchup, games, handheld, logger, paths, playtime, process, prompt, relocate, savelib, spec, tr, transfer, util};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn log(msg: impl AsRef<str>) {
    logger::log("runner", msg);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    log(format!("args: {args:?}"));
    // Linux: Steam's "Exit game" signals the whole tree; finish uploading first.
    process::defer_termination();
    let code = match run(&args) {
        Ok(c) => c,
        Err(e) => {
            log(format!("fatal: {e:#}"));
            tell("Ludonimbus", &format!("{e:#}"), "error");
            1
        }
    };
    // OpenList (Quark) started for this launch stops again unless the app runs.
    ludonimbus_core::openlist::release_if_started_here();
    std::process::exit(code);
}

struct Launch {
    exe: PathBuf,
    args: Vec<String>,
    work_dir: PathBuf,
}

fn parse_args(args: &[String]) -> Result<(String, Option<Vec<String>>)> {
    let mut key = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game" => key = it.next().cloned(),
            "--" => return Ok((key.ok_or_else(|| anyhow!(ludonimbus_core::tr!("缺少 --game", "Missing --game")))?, Some(it.cloned().collect()))),
            _ => {}
        }
    }
    Ok((key.ok_or_else(|| anyhow!(ludonimbus_core::tr!("用法：ludonimbus-run --game <游戏 id>", "Usage: ludonimbus-run --game <game id>")))?, None))
}

/// Whether questions go to a full-screen window (handhelds) instead of
/// system dialogs.
fn windowed() -> bool {
    match config::load() {
        Ok(cfg) => handheld::wants_prompt_window(&cfg),
        Err(_) => handheld::steam_gamepad_ui(),
    }
}

/// Ask a question; returns the chosen button's id (`None`: closed).
fn ask(req: &Request) -> Option<String> {
    if windowed() {
        match prompt::ask_window(req) {
            Ok(choice) => return choice,
            Err(e) => log(format!("prompt window failed ({e:#}); using a system dialog")),
        }
    }
    system_dialog(req)
}

fn system_dialog(req: &Request) -> Option<String> {
    let level = match req.level.as_str() {
        "warn" => MessageLevel::Warning,
        "error" => MessageLevel::Error,
        _ => MessageLevel::Info,
    };
    let dialog = MessageDialog::new().set_title(&req.title).set_description(&req.text).set_level(level);
    let b = &req.buttons;
    let by_label = |l: &str| b.iter().find(|x| x.label == l).map(|x| x.id.clone());
    match b.len() {
        0 | 1 => {
            dialog.set_buttons(MessageButtons::Ok).show();
            b.first().map(|x| x.id.clone())
        }
        2 => match dialog.set_buttons(MessageButtons::OkCancelCustom(b[0].label.clone(), b[1].label.clone())).show() {
            MessageDialogResult::Custom(l) => by_label(&l),
            MessageDialogResult::Ok | MessageDialogResult::Yes => Some(b[0].id.clone()),
            _ => Some(b[1].id.clone()),
        },
        _ => match dialog.set_buttons(MessageButtons::YesNoCancelCustom(b[0].label.clone(), b[1].label.clone(), b[2].label.clone())).show() {
            MessageDialogResult::Custom(l) => by_label(&l),
            MessageDialogResult::Yes => Some(b[0].id.clone()),
            MessageDialogResult::No => Some(b[1].id.clone()),
            _ => Some(b[2].id.clone()),
        },
    }
}

/// Show a message with an OK button.
fn tell(title: &str, text: &str, level: &str) {
    ask(&Request::new(title, text, level).button("ok", tr!("确定", "OK"), "a"));
}

/// Ask a yes/no question.
fn confirm(title: &str, text: &str, yes: &str, no: &str) -> bool {
    ask(&Request::new(title, text, "info").button("yes", yes, "a").button("no", no, "b")).as_deref() == Some("yes")
}

/// Make sure the game exe exists; offer the new location after an update.
fn ensure_exe(game: &mut GameEntry) -> Result<()> {
    if game.exe.as_ref().is_some_and(|e| e.is_file()) {
        return Ok(());
    }
    let old = game
        .exe
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ludonimbus_core::tr!("（未设置）", "(not set)"));
    log(format!("{}: exe missing at {old}", game.key));

    if let Some(candidate) = relocate::find_candidate(game) {
        let text = ludonimbus_core::tr!(
            "找不到「{}」原来的位置：\n{old}\n\n在附近找到了：\n{}\n\n是否改用这个位置？（如果新文件夹里没有存档，会自动从云端恢复）",
            "The old location of \"{}\" is gone:\n{old}\n\nFound nearby:\n{}\n\nUse this location? (If the new folder has no saves, they are restored from the cloud.)",
            game.name,
            candidate.display()
        );
        let title = tr!("Ludonimbus · 游戏位置变了", "Ludonimbus · The game has moved");
        if confirm(&title, &text, &tr!("改用这个位置", "Use this location"), &tr!("不用", "No")) {
            *game = games::set_exe(&game.key, &candidate)?;
            log(format!("{}: relocated to {}", game.key, candidate.display()));
            return Ok(());
        }
    }

    // No file picker in a controller-only screen.
    if windowed() {
        bail!(tr!(
            "找不到「{}」的游戏程序：{old}\n\n请在桌面模式里打开 Ludonimbus，在游戏页设置游戏位置。",
            "The program of \"{}\" wasn't found: {old}\n\nOpen Ludonimbus in desktop mode and set the game's location on its page.",
            game.name
        ));
    }
    let start_dir = game
        .exe
        .as_ref()
        .and_then(|e| e.parent())
        .and_then(|p| p.ancestors().find(|a| a.is_dir()))
        .map(Path::to_path_buf);
    let mut dialog = rfd::FileDialog::new()
        .set_title(ludonimbus_core::tr!("找不到「{}」，请选择新的游戏程序", "Can't find \"{}\". Choose its program again", game.name))
        .add_filter(ludonimbus_core::tr!("程序", "Programs"), &["exe", "bat", "cmd", "lnk"]);
    if let Some(d) = start_dir {
        dialog = dialog.set_directory(d);
    }
    match dialog.pick_file() {
        Some(p) => {
            *game = games::set_exe(&game.key, &p)?;
            log(format!("{}: user picked {}", game.key, p.display()));
            Ok(())
        }
        None => bail!(ludonimbus_core::tr!("已取消：找不到「{}」的游戏程序", "Cancelled: the program of \"{}\" wasn't found", game.name)),
    }
}

fn human(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1048576.0)
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

enum Choice {
    Cloud,
    Local,
    Cancel,
}

fn ask_conflict(game: &GameEntry, local_files: usize, local_size: u64, cloud: &ludonimbus_core::snapshot::SnapshotSummary) -> Choice {
    let text = ludonimbus_core::tr!(
        "「{}」的存档在本机和云端都有新的改动。\n\n云端：{} 于 {} 保存（{} 个文件，{}）\n本机：{} 个文件，{}\n\n请选择这次要使用哪一份。没选的那份也会保存在版本历史里，随时可以恢复。",
        "Both this PC and the cloud have new changes to the saves of \"{}\".\n\nCloud: saved by {} at {} ({} files, {})\nThis PC: {} files, {}\n\nChoose which one to use this time. The other one is kept in the version history and can be restored at any time.",
        game.name,
        cloud.device_name,
        cloud.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
        cloud.file_count,
        human(cloud.total_size),
        local_files,
        human(local_size),
    );
    let req = Request::new(tr!("Ludonimbus · 存档冲突", "Ludonimbus · Save conflict"), text, "warn")
        .button("cloud", tr!("使用云端存档", "Use cloud saves"), "a")
        .button("local", tr!("使用本机存档", "Use this PC's saves"), "x")
        .button("cancel", tr!("暂不启动", "Don't start"), "b");
    match ask(&req).as_deref() {
        Some("cloud") => Choice::Cloud,
        Some("local") => Choice::Local,
        _ => Choice::Cancel,
    }
}

/// Sync, trying again a couple of times while the cloud is unreachable: a
/// handheld just woken from sleep needs a few seconds to get its network
/// back.
fn sync_retrying(ctx: &Ctx, game: &GameEntry, res: Resolution, trigger: Trigger) -> Result<Outcome> {
    let waits: &[u64] = if matches!(trigger, Trigger::PreLaunch) { &[2, 4] } else { &[3, 6] };
    let mut out = sync::sync(ctx, game, res, trigger)?;
    for &w in waits {
        if !matches!(out, Outcome::Offline { .. } | Outcome::Queued { .. }) {
            break;
        }
        log(format!("{}: cloud unreachable, trying again in {w} s", game.key));
        std::thread::sleep(Duration::from_secs(w));
        out = sync::sync(ctx, game, res, trigger)?;
    }
    Ok(out)
}

/// Another version of the game (say the Xbox version, played on the PC) has
/// newer saves than this one: offer to apply them before starting.
fn offer_newer_version(ctx: &Ctx, game: &GameEntry) {
    let newer = match savelib::newer_version(ctx, game) {
        Ok(Some(n)) => n,
        Ok(None) => return,
        Err(e) => {
            log(format!("{}: checking other versions failed: {e:#}", game.key));
            return;
        }
    };
    let when = newer.snapshot.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M");
    let text = tr!(
        "「{}」的另一个版本（{}）有更新的存档：{} 在 {} 保存。\n\n要先把它应用到这个版本再开始吗？这个版本现在的存档会先保存到云端，可以在 Ludonimbus 里撤回。",
        "Another version of \"{}\" ({}) has newer saves: saved by {} at {}.\n\nApply them to this version before starting? This version's current saves are kept in the cloud first, so you can undo it in Ludonimbus.",
        game.name,
        newer.label,
        newer.snapshot.device_name,
        when
    );
    let req = Request::new(tr!("Ludonimbus · 另一个版本有更新的存档", "Ludonimbus · Newer saves in another version"), text, "info")
        .button("apply", tr!("应用后开始", "Apply and start"), "a")
        .button("skip", tr!("不用", "No thanks"), "b");
    if ask(&req).as_deref() != Some("apply") {
        let _ = savelib::dismiss_newer(&game.key, &newer.snapshot.id);
        return;
    }
    match transfer::apply(ctx, &newer.key, &newer.snapshot.id, &game.key, None) {
        Ok(out) => log(format!("{}: applied {} of {}: {out:?}", game.key, newer.snapshot.id, newer.key)),
        Err(e) => {
            log(format!("{}: applying {} failed: {e:#}", game.key, newer.key));
            tell(
                "Ludonimbus",
                &tr!(
                    "没能应用另一个版本的存档：\n{e:#}\n\n游戏会用现在的存档启动。",
                    "The other version's saves couldn't be applied:\n{e:#}\n\nThe game starts with its current saves."
                ),
                "warn",
            );
        }
    }
}

/// `running/<key>.json`: tells the tray app (and through it the Steam plugin)
/// that this wrapper handles the game, what it is doing, and since when the
/// game runs (so the tray app can still count the playtime if this wrapper
/// is terminated).
struct RunningLock {
    path: PathBuf,
    started: std::cell::Cell<Option<chrono::DateTime<chrono::Utc>>>,
}

impl RunningLock {
    fn create(key: &str) -> Self {
        let lock = RunningLock {
            path: paths::running_dir().join(format!("{key}.json")),
            started: std::cell::Cell::new(None),
        };
        lock.phase("preLaunch");
        lock
    }

    /// "preLaunch", "playing" or "uploading". The first "playing" marks the
    /// start of the play session.
    fn phase(&self, phase: &str) {
        if phase == "playing" && self.started.get().is_none() {
            self.started.set(Some(chrono::Utc::now()));
        }
        let mut v = serde_json::json!({ "pid": std::process::id(), "phase": phase });
        if let Some(t) = self.started.get() {
            v["startedAt"] = serde_json::json!(t);
        }
        let _ = std::fs::write(&self.path, v.to_string());
    }

    /// Mark the session as recorded so the tray app doesn't count it again.
    fn session_recorded(&self) {
        self.started.set(None);
    }
}

impl Drop for RunningLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn run(args: &[String]) -> Result<i32> {
    let (key, explicit) = parse_args(args)?;
    let ctx = Ctx::load()?;
    let mut game = ctx.game(&key)?;
    let lock = RunningLock::create(&key);

    let launch = match explicit {
        // Steam's %command% (Linux): Steam already changed to the shortcut's
        // start folder, and the command may begin with Proton or a tool
        // such as gamemoderun, not with the game.
        Some(cmd) if !cmd.is_empty() => {
            let exe = PathBuf::from(&cmd[0]);
            let work_dir = std::env::current_dir().unwrap_or_else(|_| exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")));
            Launch { exe, args: cmd[1..].to_vec(), work_dir }
        }
        _ => {
            ensure_exe(&mut game)?;
            let exe = game.exe.clone().expect("checked by ensure_exe");
            let work_dir = game
                .work_dir
                .clone()
                .filter(|d| d.is_dir())
                .or_else(|| exe.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."));
            Launch { exe, args: util::split_args(&game.args), work_dir }
        }
    };

    // Reload the context in case ensure_exe changed the config.
    let ctx = Ctx::load()?;
    // Import the minutes Steam counted so far before this session adds to them.
    if let Err(e) = playtime::ensure(&ctx.cfg, &game) {
        log(format!("{key}: playtime: {e:#}"));
    }

    if game.auto_sync {
        match sync_retrying(&ctx, &game, Resolution::Auto, Trigger::PreLaunch) {
            Ok(Outcome::Conflict { local_files, local_size, cloud }) => {
                let res = match ask_conflict(&game, local_files, local_size, &cloud) {
                    Choice::Cloud => Resolution::UseCloud,
                    Choice::Local => Resolution::UseLocal,
                    Choice::Cancel => return Ok(0),
                };
                let out = sync::sync(&ctx, &game, res, Trigger::PreLaunch)?;
                log(format!("{key}: conflict resolved: {out:?}"));
            }
            Ok(out) => {
                log(format!("{key}: pre-launch {out:?}"));
                if !matches!(out, Outcome::Offline { .. }) {
                    offer_newer_version(&ctx, &game);
                }
            }
            Err(e) => {
                log(format!("{key}: pre-launch sync failed: {e:#}"));
                let text = tr!(
                    "启动前同步存档失败：\n{e:#}\n\n仍然启动游戏吗？退出游戏后会再尝试上传。",
                    "Syncing saves before launch failed:\n{e:#}\n\nStart the game anyway? Ludonimbus will try to upload again after you quit."
                );
                if !confirm("Ludonimbus", &text, &tr!("仍然启动", "Start anyway"), &tr!("不启动", "Don't start")) {
                    return Ok(0);
                }
            }
        }
    }

    let spec = spec::effective(&game);
    let quiesce: Vec<PathBuf> = spec::resolve_roots(&game, &spec)
        .map(|roots| {
            spec.quiesce_absent
                .iter()
                .filter_map(|p| spec::split_pattern(p).ok())
                .filter_map(|(root, rest)| spec::resolve_root_name(&root, &roots).map(|b| b.join(rest)))
                .collect()
        })
        .unwrap_or_default();
    // Wait until the game has finished writing (e.g. lock files released).
    let settle = || {
        if !quiesce.is_empty() {
            log(format!("{key}: waiting for save files to be released"));
            process::wait_quiesce(&quiesce, Duration::from_secs(20));
        }
    };

    // Optionally upload as soon as the game process exits, while a launcher
    // that stays open keeps the Steam session alive.
    let mut early_upload = || {
        settle();
        log(format!("{key}: game exited, uploading before the launcher closes"));
        lock.phase("uploading");
        match sync::sync(&ctx, &game, Resolution::Auto, Trigger::AutoExit) {
            Ok(out) => log(format!("{key}: early upload {out:?}")),
            Err(e) => log(format!("{key}: early upload failed: {e:#}")),
        }
        lock.phase("playing");
    };
    let on_game_exit: Option<&mut dyn FnMut()> = if game.auto_sync && spec.upload_on_game_exit {
        Some(&mut early_upload)
    } else {
        None
    };

    log(format!("{key}: launching {} {:?}", launch.exe.display(), launch.args));
    lock.phase("playing");
    let code = process::launch_and_wait(
        &launch.exe,
        &launch.args,
        &launch.work_dir,
        &spec.wait_for,
        on_game_exit,
        &|m| log(format!("{key}: {m}")),
    )?;

    if let Some(start) = lock.started.get() {
        match playtime::record(&ctx.cfg, &game, start, chrono::Utc::now(), playtime::Via::Steam) {
            Ok(_) => lock.session_recorded(),
            Err(e) => log(format!("{key}: playtime: {e:#}")),
        }
    }
    // Without auto-sync there is no save sync to carry the playtime upload.
    if !game.auto_sync {
        playtime::upload_quietly(&ctx.cfg, &key, log);
    }

    if game.auto_sync {
        lock.phase("uploading");
        settle();
        log(format!("{key}: uploading saves"));
        match sync_retrying(&ctx, &game, Resolution::Auto, Trigger::AutoExit) {
            Ok(Outcome::Conflict { cloud, .. }) => {
                log(format!("{key}: post-exit conflict"));
                tell(
                    &tr!("Ludonimbus · 存档冲突", "Ludonimbus · Save conflict"),
                    &tr!(
                        "玩游戏期间，{} 也上传了「{}」的新存档。\n\n两份存档都已保存到云端。下次启动时会让你选择使用哪一份，也可以在 Ludonimbus 里处理。",
                        "While you were playing, {} also uploaded new saves of \"{}\".\n\nBoth are kept in the cloud. You'll be asked which one to use the next time you start the game, or you can sort it out in Ludonimbus.",
                        cloud.device_name, game.name
                    ),
                    "warn",
                );
            }
            Ok(out) => log(format!("{key}: post-exit {out:?}")),
            Err(e) => {
                log(format!("{key}: post-exit sync failed: {e:#}"));
                let _ = ludonimbus_core::state::update_game(&key, |g| g.last_error = Some(format!("{e:#}")));
            }
        }
    }
    // Achievements earned in this session are recorded and uploaded even
    // when the tray app isn't running (popups are the tray app's job).
    ludonimbus_core::achievements::after_exit(&ctx, &game);
    // No tray app (Xbox full screen experience, Deck Game Mode): do its work.
    if !catchup::app_running() {
        drop(lock);
        catchup::stale_runners(&ctx.cfg, Some(&key), &|m| log(m));
        catchup::screenshots(&ctx.cfg, &key, &|m| log(m));
    }
    Ok(code)
}
