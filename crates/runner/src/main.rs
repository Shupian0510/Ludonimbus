//! `ludonimbus-run`: what a Steam shortcut launches once auto-sync is on.
//!
//! ```text
//! ludonimbus-run --game <key>                 # exe/args come from Ludonimbus config
//! ludonimbus-run --game <key> -- <cmd...>     # explicit command (e.g. Steam %command%)
//! ```
//! 1. sync before launch (pull newer cloud saves, ask on conflict)
//! 2. start the game and wait for the whole process tree to exit
//! 3. sync after exit (upload new saves)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config::GameEntry;
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Outcome, Resolution};
use ludonimbus_core::{games, logger, paths, process, relocate, spec, util};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn log(msg: impl AsRef<str>) {
    logger::log("runner", msg);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    log(format!("args: {args:?}"));
    let code = match run(&args) {
        Ok(c) => c,
        Err(e) => {
            log(format!("fatal: {e:#}"));
            MessageDialog::new()
                .set_title("Ludonimbus")
                .set_description(format!("{e:#}"))
                .set_level(MessageLevel::Error)
                .show();
            1
        }
    };
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

/// Ask a yes/no question.
fn confirm(title: &str, text: &str) -> bool {
    matches!(
        MessageDialog::new()
            .set_title(title)
            .set_description(text)
            .set_level(MessageLevel::Info)
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    )
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
        if confirm(&ludonimbus_core::tr!("Ludonimbus · 游戏位置变了", "Ludonimbus · The game has moved"), &text) {
            *game = games::set_exe(&game.key, &candidate)?;
            log(format!("{}: relocated to {}", game.key, candidate.display()));
            return Ok(());
        }
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
    let cloud_label = ludonimbus_core::tr!("使用云端存档", "Use cloud saves");
    let local_label = ludonimbus_core::tr!("使用本机存档", "Use this PC's saves");
    let cancel_label = ludonimbus_core::tr!("暂不启动", "Don't start");
    let result = MessageDialog::new()
        .set_title(ludonimbus_core::tr!("Ludonimbus · 存档冲突", "Ludonimbus · Save conflict"))
        .set_description(text)
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::YesNoCancelCustom(
            cloud_label.clone(),
            local_label.clone(),
            cancel_label.clone(),
        ))
        .show();
    match result {
        MessageDialogResult::Custom(s) if s == cloud_label => Choice::Cloud,
        MessageDialogResult::Custom(s) if s == local_label => Choice::Local,
        MessageDialogResult::Yes => Choice::Cloud,
        MessageDialogResult::No => Choice::Local,
        _ => Choice::Cancel,
    }
}

/// `running/<key>.json`: tells the tray app (and through it the Steam plugin)
/// that this wrapper handles the game, and what it is doing.
struct RunningLock(PathBuf);

impl RunningLock {
    fn create(key: &str) -> Self {
        let lock = RunningLock(paths::running_dir().join(format!("{key}.json")));
        lock.phase("preLaunch");
        lock
    }

    /// "preLaunch", "playing" or "uploading".
    fn phase(&self, phase: &str) {
        let _ = std::fs::write(
            &self.0,
            serde_json::json!({ "pid": std::process::id(), "phase": phase }).to_string(),
        );
    }
}

impl Drop for RunningLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[String]) -> Result<i32> {
    let (key, explicit) = parse_args(args)?;
    let ctx = Ctx::load()?;
    let mut game = ctx.game(&key)?;
    let lock = RunningLock::create(&key);

    let launch = match explicit {
        Some(cmd) if !cmd.is_empty() => {
            let exe = PathBuf::from(&cmd[0]);
            let work_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
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

    if game.auto_sync {
        match sync::sync(&ctx, &game, Resolution::Auto, Trigger::PreLaunch) {
            Ok(Outcome::Conflict { local_files, local_size, cloud }) => {
                let res = match ask_conflict(&game, local_files, local_size, &cloud) {
                    Choice::Cloud => Resolution::UseCloud,
                    Choice::Local => Resolution::UseLocal,
                    Choice::Cancel => return Ok(0),
                };
                let out = sync::sync(&ctx, &game, res, Trigger::PreLaunch)?;
                log(format!("{key}: conflict resolved: {out:?}"));
            }
            Ok(out) => log(format!("{key}: pre-launch {out:?}")),
            Err(e) => {
                log(format!("{key}: pre-launch sync failed: {e:#}"));
                let text = ludonimbus_core::tr!(
                    "启动前同步存档失败：\n{e:#}\n\n仍然启动游戏吗？退出游戏后会再尝试上传。",
                    "Syncing saves before launch failed:\n{e:#}\n\nStart the game anyway? Ludonimbus will try to upload again after you quit."
                );
                if !confirm("Ludonimbus", &text) {
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

    if game.auto_sync {
        lock.phase("uploading");
        settle();
        log(format!("{key}: uploading saves"));
        match sync::sync(&ctx, &game, Resolution::Auto, Trigger::AutoExit) {
            Ok(Outcome::Conflict { cloud, .. }) => {
                log(format!("{key}: post-exit conflict"));
                MessageDialog::new()
                    .set_title(ludonimbus_core::tr!("Ludonimbus · 存档冲突", "Ludonimbus · Save conflict"))
                    .set_description(ludonimbus_core::tr!(
                        "玩游戏期间，{} 也上传了「{}」的新存档。\n\n两份存档都已保存到云端。下次启动时会让你选择使用哪一份，也可以在 Ludonimbus 里处理。",
                        "While you were playing, {} also uploaded new saves of \"{}\".\n\nBoth are kept in the cloud. You'll be asked which one to use the next time you start the game, or you can sort it out in Ludonimbus.",
                        cloud.device_name, game.name
                    ))
                    .set_level(MessageLevel::Warning)
                    .show();
            }
            Ok(out) => log(format!("{key}: post-exit {out:?}")),
            Err(e) => {
                log(format!("{key}: post-exit sync failed: {e:#}"));
                let _ = ludonimbus_core::state::update_game(&key, |g| g.last_error = Some(format!("{e:#}")));
            }
        }
    }
    Ok(code)
}
