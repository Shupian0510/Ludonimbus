//! `ludonimbus-run`: what a Steam shortcut launches once auto-sync is on.
//!
//! ```text
//! ludonimbus-run --game <key>                 # exe/args come from Ludonimbus config
//! ludonimbus-run --game <key> -- <cmd...>     # explicit command (e.g. Steam %command%)
//! ludonimbus-run --finish <key>               # finish a sync after playing (no window)
//! ```
//! 1. sync before launch (pull newer cloud saves, ask on conflict), and offer
//!    newer saves of another version of the game (save library). Not for a
//!    game Steam installed whose saves Steam Cloud keeps (backup only; also
//!    a managed one found with Steam Cloud on again, see `core::steamcloud`)
//! 2. start the game and wait for the whole process tree to exit
//! 3. sync after exit (upload new saves); without the tray app, also finish
//!    the sessions of terminated wrappers and upload screenshots
//!
//! Playing doesn't wait long for the cloud (docs/plan-v2.md, section 5.5):
//! before launch the sync has a time limit, and only a newer version that
//! is being downloaded is waited for, with a way to stop waiting. After
//! exit the upload is waited for a few seconds; the rest goes on in the
//! tray app or in a process of its own (`--finish`) and the wrapper exits.
//!
//! Asked to stop (Steam's "Stop"; Linux: SIGTERM) before the game starts,
//! the wrapper doesn't start it. Once the game runs, the game ends by itself
//! and the saves are uploaded as usual.
//!
//! Questions are system dialogs on the desktop and a full-screen window on
//! handhelds (`ludonimbus-app --prompt`, see `core::prompt`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::config::{self, GameEntry};
use ludonimbus_core::prompt::Request;
use ludonimbus_core::snapshot::Trigger;
use ludonimbus_core::sync::{self, Ctx, Gate, Outcome, Phase, Resolution};
use ludonimbus_core::{catchup, games, handheld, logger, paths, playtime, process, prompt, relocate, savelib, spec, steamcloud, tr, transfer, util};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// `ludonimbus-run --finish <key>`.
const FINISH_ARG: &str = "--finish";

/// After a hand-over, the finishing process tries again after these pauses
/// (seconds, about 10 minutes in all); then the tray app or the game's next
/// sync takes it from there.
const FINISH_PAUSES: [u64; 8] = [5, 10, 20, 40, 60, 120, 150, 180];

fn log(msg: impl AsRef<str>) {
    logger::log("runner", msg);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    log(format!("args: {args:?}"));
    // Linux: Steam's "Exit game" signals the whole tree; finish uploading first.
    process::defer_termination();
    let code = match finishing(&args) {
        // Nobody looks at this process: no windows, only the log.
        Some(key) => finish(key),
        None => match run(&args) {
            Ok(c) => c,
            Err(e) => {
                log(format!("fatal: {e:#}"));
                // Asked to stop: nobody waits for a message.
                if !process::termination_requested() {
                    tell("Ludonimbus", &format!("{e:#}"), "error");
                }
                1
            }
        },
    };
    // OpenList (Quark) started for this launch stops again unless the app runs.
    ludonimbus_core::openlist::release_if_started_here();
    std::process::exit(code);
}

/// The game of `--finish <key>`. Only the wrapper's own arguments count,
/// the ones before `--`: what follows is the game's command, which may have
/// a `--finish` of its own.
fn finishing(args: &[String]) -> Option<&str> {
    let own = &args[..args.iter().position(|a| a == "--").unwrap_or(args.len())];
    own.iter().position(|a| a == FINISH_ARG).and_then(|i| own.get(i + 1)).map(String::as_str)
}

struct Launch {
    exe: PathBuf,
    args: Vec<String>,
    /// Set for the game: `NAME=value` words in front of the command.
    envs: Vec<(String, String)>,
    work_dir: PathBuf,
}

/// Split `NAME=value` words off the front of a command: the environment
/// a shell would give the program. Launch options written by older versions
/// put the wrapper in front of them (`<wrapper> -- MANGOHUD=1 %command%`),
/// so they reach the wrapper as part of the command.
fn split_envs(cmd: Vec<String>) -> (Vec<(String, String)>, Vec<String>) {
    let is_name = |n: &str| n.chars().next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic()) && n.chars().all(|c| c == '_' || c.is_ascii_alphanumeric());
    let n = cmd.iter().take_while(|w| w.split_once('=').is_some_and(|(name, _)| is_name(name))).count();
    let envs = cmd[..n].iter().filter_map(|w| w.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    (envs, cmd[n..].to_vec())
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
    let mut text = ludonimbus_core::tr!(
        "「{}」的存档在本机和云端都有新的改动。\n\n云端：{} 于 {} 保存（{} 个文件，{}）\n本机：{} 个文件，{}\n\n请选择这次要使用哪一份。没选的那份也会留在「存档」里，随时可以设回去。",
        "Both this PC and the cloud have new changes to the saves of \"{}\".\n\nCloud: saved by {} at {} ({} files, {})\nThis PC: {} files, {}\n\nChoose which one to use this time. The other one stays under Saves and can be made current again at any time.",
        game.name,
        cloud.device_name,
        cloud.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
        cloud.file_count,
        human(cloud.total_size),
        local_files,
        human(local_size),
    );
    // The cloud's go into the Xbox app's containers: experimental, so said
    // before the user chooses. The sync before may have just bound the game.
    let containers = ludonimbus_core::config::load().ok().and_then(|c| c.game(&game.key).map(|g| ludonimbus_core::bind::has_wgs(&g.bind)));
    if containers.unwrap_or_else(|| ludonimbus_core::bind::has_wgs(&game.bind)) {
        text.push_str("\n\n");
        text.push_str(&tr!(
            "使用云端存档会写入 Xbox 版，这是实验性的（Xbox 应用的云同步可能把存档换回去），本机现在的存档会先保留一份。",
            "Using the cloud saves writes into the Xbox version, which is experimental (the Xbox app's own cloud sync may put its saves back); the saves on this PC are kept first."
        ));
    }
    // Saved on another platform by a newer game than the one here: said
    // before the user chooses it.
    let here = ludonimbus_core::sync::saves_version(game);
    if ludonimbus_core::sync::too_new(game, here.as_deref(), &cloud.platform, cloud.game_version.as_deref()) {
        let v = cloud.game_version.as_deref().unwrap_or_default();
        text.push_str("\n\n");
        text.push_str(&tr!(
            "云端这份是游戏 {} 存的，比这里的新。旧版本的游戏读不了新版本的存档：用它之前先把这里的游戏更新到 {}。",
            "The cloud's saves were made by version {} of the game, newer than here. An older game can't read a newer save: update the game here to {} before using them.",
            v,
            v
        ));
    }
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
/// back. The tries are part of the time the wrapper waits.
fn sync_retrying(ctx: &Ctx, game: &GameEntry, res: Resolution, trigger: Trigger, gate: &Gate) -> Result<Outcome> {
    let waits: &[u64] = if matches!(trigger, Trigger::PreLaunch) { &[2, 4] } else { &[3, 6] };
    let waits: Vec<Duration> = waits.iter().map(|&s| Duration::from_secs(s)).collect();
    retrying(&waits, trigger == Trigger::PreLaunch, gate, || sync::sync_with(ctx, game, res, trigger, gate), &|w| {
        log(format!("{}: cloud unreachable, trying again in {} s", game.key, w.as_secs()))
    })
}

/// Run `attempt` again after each of `waits` while the cloud wasn't
/// reached, until the waiting is given up. Before playing (`launching`) a
/// version kept for after the game ([`Outcome::Queued`]) is no reason to try
/// again: it is a cloud that needs upgrading, or a sync nobody waits for.
fn retrying(
    waits: &[Duration],
    launching: bool,
    gate: &Gate,
    mut attempt: impl FnMut() -> Result<Outcome>,
    note: &dyn Fn(Duration),
) -> Result<Outcome> {
    let again = |out: &Outcome| match out {
        Outcome::Offline { .. } => true,
        Outcome::Queued { .. } => !launching,
        _ => false,
    };
    let mut out = attempt()?;
    for &w in waits {
        if !again(&out) || gate.given_up() {
            break;
        }
        note(w);
        // Still checking: the wrapper may stop waiting during the pause.
        gate.set_phase(Phase::Checking);
        if !pause(w, gate) {
            break;
        }
        out = attempt()?;
    }
    Ok(out)
}

/// Sleep for `d`, unless this process is asked to stop meanwhile (Linux:
/// SIGTERM): `false` then.
fn sleep_unless_stopped(d: Duration) -> bool {
    let end = Instant::now() + d;
    while !process::termination_requested() {
        let now = Instant::now();
        if now >= end {
            return true;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(100)));
    }
    false
}

/// Sleep for `d`, unless the waiting is given up meanwhile: `false` then.
fn pause(d: Duration, gate: &Gate) -> bool {
    let end = Instant::now() + d;
    loop {
        if gate.given_up() {
            return false;
        }
        let now = Instant::now();
        if now >= end {
            return true;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(50)));
    }
}

// --------------------------------------------------------------- waiting

/// How often the wrapper looks at a sync it waits for past its time.
const POLL: Duration = Duration::from_millis(50);

/// Work on a thread of its own, and the gate to stop waiting for it.
struct Worker<T> {
    gate: Gate,
    rx: mpsc::Receiver<T>,
    thread: Option<std::thread::JoinHandle<()>>,
}

enum Poll<T> {
    Ready(T),
    Pending,
    /// It ended without a result (it panicked).
    Lost,
}

impl<T: Send + 'static> Worker<T> {
    fn start(f: impl FnOnce(&Gate) -> T + Send + 'static) -> Worker<T> {
        let gate = Gate::new();
        let (tx, rx) = mpsc::channel();
        let g = gate.clone();
        let thread = std::thread::spawn(move || {
            let _ = tx.send(f(&g));
        });
        Worker { gate, rx, thread: Some(thread) }
    }

    fn wait(&self, max: Duration) -> Poll<T> {
        match self.rx.recv_timeout(max) {
            Ok(v) => Poll::Ready(v),
            Err(mpsc::RecvTimeoutError::Timeout) => Poll::Pending,
            Err(mpsc::RecvTimeoutError::Disconnected) => Poll::Lost,
        }
    }

    /// Wait until it has ended, and its result.
    fn join(mut self) -> Option<T> {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.rx.try_recv().ok()
    }
}

/// How waiting for the sync before launch ended.
enum Waited<T> {
    /// It ended: in time, or the user waited for it.
    Finished(T),
    Lost,
    /// The wrapper stopped waiting. The sync stops at its next step without
    /// touching the saves; join it before the game's next sync.
    GaveUp(Worker<T>),
}

/// What the user did with the window shown while a newer version is being
/// downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Open,
    /// "Play with this PC's saves".
    PlayLocal,
    /// The window is gone without an answer (it couldn't show).
    Gone,
}

/// That window. Dropping it closes it.
trait Notice {
    fn answer(&mut self) -> Answer;
}

/// The button of the download window.
const PLAY_LOCAL: &str = "local";

impl Notice for prompt::Window {
    fn answer(&mut self) -> Answer {
        match self.poll() {
            Ok(None) => Answer::Open,
            Ok(Some(choice)) if choice.as_deref() == Some(PLAY_LOCAL) => Answer::PlayLocal,
            Ok(Some(_)) => Answer::Gone,
            Err(e) => {
                log(format!("the download window didn't show ({e:#})"));
                Answer::Gone
            }
        }
    }
}

/// Wait for the sync before launch up to `budget`. Past it, a sync that
/// checks or uploads is given up, and one that downloads a newer version
/// is waited for while `notice` (when it shows one) offers to stop waiting.
/// A wrapper asked to stop gives it up too. Replacing the saves is always
/// waited for; it is quick.
fn wait_before_launch<T: Send + 'static>(
    worker: Worker<T>,
    budget: Duration,
    notice: &mut dyn FnMut(Phase) -> Option<Box<dyn Notice>>,
) -> Waited<T> {
    match worker.wait(budget) {
        Poll::Ready(v) => return Waited::Finished(v),
        Poll::Lost => return Waited::Lost,
        Poll::Pending => {}
    }
    let mut shown: Option<Box<dyn Notice>> = None;
    let mut asked = false;
    loop {
        // Asked to stop (Steam's "Stop"): no game to wait for.
        if process::termination_requested() {
            break;
        }
        match worker.gate.phase() {
            phase @ Phase::Downloading { .. } => {
                if !asked {
                    asked = true;
                    shown = notice(phase);
                }
            }
            // The saves are being replaced, or the sync is about to end.
            Phase::Applying | Phase::Done => {}
            Phase::Checking | Phase::Uploading => break,
        }
        if let Some(n) = shown.as_mut() {
            match n.answer() {
                Answer::Open => {}
                Answer::PlayLocal => break,
                Answer::Gone => shown = None,
            }
        }
        match worker.wait(POLL) {
            Poll::Ready(v) => return Waited::Finished(v),
            Poll::Lost => return Waited::Lost,
            Poll::Pending => {}
        }
    }
    drop(shown);
    loop {
        match worker.gate.phase() {
            // Replacing the saves takes a moment and can't be given up; the
            // sync may end right after.
            Phase::Applying | Phase::Done => {}
            // Not past an upload that may follow.
            _ => {
                if worker.gate.give_up() {
                    return Waited::GaveUp(worker);
                }
            }
        }
        match worker.wait(POLL) {
            Poll::Ready(v) => return Waited::Finished(v),
            Poll::Lost => return Waited::Lost,
            Poll::Pending => {}
        }
    }
}

/// How waiting for the sync after exit ended.
enum AfterExit<T> {
    Finished(T),
    Lost,
    /// Still running when the time was up.
    Running(Worker<T>),
}

/// Wait for the sync after exit up to `budget`; `None`: until it ends.
fn wait_after_exit<T: Send + 'static>(worker: Worker<T>, budget: Option<Duration>) -> AfterExit<T> {
    let Some(budget) = budget else {
        return match worker.join() {
            Some(v) => AfterExit::Finished(v),
            None => AfterExit::Lost,
        };
    };
    match worker.wait(budget) {
        Poll::Ready(v) => AfterExit::Finished(v),
        Poll::Lost => AfterExit::Lost,
        Poll::Pending => AfterExit::Running(worker),
    }
}

/// Sync `game` on a thread of its own.
fn start_sync(ctx: &Arc<Ctx>, game: &GameEntry, res: Resolution, trigger: Trigger, retry: bool) -> Worker<Result<Outcome>> {
    let (ctx, game) = (Arc::clone(ctx), game.clone());
    Worker::start(move |gate| match retry {
        true => sync_retrying(&ctx, &game, res, trigger, gate),
        false => sync::sync_with(&ctx, &game, res, trigger, gate),
    })
}

/// The window shown while a newer version of the saves is downloaded past
/// the time the wrapper waits. `None` when it can't be shown: the download
/// is waited for then.
fn download_notice(game: &GameEntry, phase: Phase) -> Option<Box<dyn Notice>> {
    let text = match phase {
        Phase::Downloading { total, .. } if total > 0 => {
            tr!("正在下载「{}」更新的存档（{}）。", "Downloading newer saves of \"{}\" ({}).", game.name, human(total))
        }
        _ => tr!("正在下载「{}」更新的存档。", "Downloading newer saves of \"{}\".", game.name),
    };
    let text = text
        + &tr!(
            "\n\n也可以先玩本机的：两份都会留着，之后会问你用哪一份。",
            "\n\nYou can also play with this PC's saves now: both are kept, and you'll be asked which one to use."
        );
    // Closing the window means the same as the button.
    let req = Request::new(tr!("Ludonimbus · 正在下载存档", "Ludonimbus · Downloading saves"), text, "info").button(
        PLAY_LOCAL,
        tr!("先玩本机的", "Play with this PC's saves"),
        "b",
    );
    match prompt::open_window(&req) {
        Ok(w) => {
            log(format!("{}: still downloading; showing the window", game.key));
            Some(Box::new(w))
        }
        Err(e) => {
            log(format!("{}: still downloading, and no window can be shown ({e:#}); waiting", game.key));
            None
        }
    }
}

/// The wrapper didn't wait for the sync before launch.
fn not_waited(key: &str, budget: Duration) {
    // It was asked to stop: the game isn't started ([`stopped_before_launch`]).
    if process::termination_requested() {
        return;
    }
    log(format!("{key}: no answer from the cloud within {} s; starting the game", budget.as_secs()));
    let reason = tr!(
        "启动前 {} 秒内没等到云盘，这一局结束后再同步",
        "The cloud didn't answer within {} s before launch; the saves are synced after this game",
        budget.as_secs()
    );
    let _ = ludonimbus_core::state::update_game(key, |g| g.last_error = Some(reason));
}

/// Whether the wrapper was asked to stop before the game started (Steam's
/// "Stop" while it syncs or asks; Linux: SIGTERM). The game isn't started
/// then. A sync before launch that wasn't waited for was given up: it
/// changes no save file any more, and an upload it may still be at is never
/// half a version when it is cut off, so the process may exit.
fn stopped_before_launch(key: &str) -> bool {
    let stop = process::termination_requested();
    if stop {
        log(format!("{key}: asked to stop before the game started; not starting it"));
    }
    stop
}

// ------------------------------------------------------------ hand-over

/// Leave the rest of the sync after playing to someone else, so the wrapper
/// can exit (and Steam stops counting the game as running): the tray app
/// when it runs, else a process of its own (also beside the headless agent
/// of Deck Game Mode, which doesn't do it). `false` when neither can take
/// it; the wrapper waits for the upload then.
fn hand_over(key: &str) -> bool {
    if let Err(e) = sync::owe(key) {
        log(format!("{key}: can't note the upload for later ({e:#})"));
        return false;
    }
    if catchup::desktop_app_running() {
        log(format!("{key}: the tray app finishes the upload"));
        return true;
    }
    spawn_finisher(key)
}

/// How long a sync handed over gets to stop before the wrapper exits anyway.
const STOP_AFTER_HAND_OVER: Duration = Duration::from_secs(5);

/// Stop the sync after playing of this process once it is handed over, so
/// the process exits at a safe point. Given up, it doesn't start replacing
/// save files and takes no step after the one under way; replacing them is
/// waited for, however long (it is quick and on this device, and cut off it
/// would leave the saves half old, half new). Then it has up to `max` to
/// end. An upload still under way when the process exits is never half a
/// version: packs go up first and the manifest last, the version stays in
/// the queue (written there manifest last, in one step), and whoever took
/// over waits for this process's lock before going on. [`Poll::Pending`]
/// when it didn't end in time.
fn stop_after_hand_over<T: Send + 'static>(worker: Worker<T>, max: Duration) -> Poll<T> {
    let end = loop {
        if worker.gate.give_up() {
            break worker.wait(max);
        }
        match worker.wait(POLL) {
            Poll::Pending => {}
            ended => break ended,
        }
    };
    if !matches!(end, Poll::Pending) {
        worker.join();
    }
    end
}

/// Start `ludonimbus-run --finish <key>` as a process that outlives this
/// one and that Steam doesn't count as part of the game, where that can be
/// done. Whether Steam really lets go is for a real device to show.
fn spawn_finisher(key: &str) -> bool {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            log(format!("{key}: can't start the background upload ({e})"));
            return false;
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let plain = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Out of a job Steam may have put the wrapper in, when it allows that.
        for flags in [plain | CREATE_BREAKAWAY_FROM_JOB, plain] {
            let spawned = Command::new(&exe)
                .arg(FINISH_ARG)
                .arg(key)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(flags)
                .spawn();
            match spawned {
                Ok(child) => {
                    let how = if flags == plain { " (it couldn't leave the wrapper's job)" } else { "" };
                    log(format!("{key}: process {} finishes the upload{how}", child.id()));
                    return true;
                }
                Err(e) => log(format!("{key}: starting the background upload failed ({e})")),
            }
        }
        false
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        // A unit of the user's systemd is no child of Steam's: its reaper
        // doesn't wait for it. Without one, Steam would keep the game
        // "running" anyway, so the wrapper waits itself.
        let mut cmd = Command::new("systemd-run");
        cmd.args(["--user", "--collect", "--quiet"]);
        for var in ["LUDONIMBUS_HOME", "LUDONIMBUS_APP", "XDG_CONFIG_HOME"] {
            if let Some(v) = std::env::var_os(var) {
                let mut arg = std::ffi::OsString::from(format!("--setenv={var}="));
                arg.push(v);
                cmd.arg(arg);
            }
        }
        cmd.arg(&exe).arg(FINISH_ARG).arg(key).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log(format!("{key}: no systemd-run ({e}); waiting for the upload"));
                return false;
            }
        };
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    log(format!("{key}: a systemd unit finishes the upload"));
                    return true;
                }
                Ok(Some(status)) => {
                    log(format!("{key}: systemd-run failed ({status}); waiting for the upload"));
                    return false;
                }
                Ok(None) if start.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    log(format!("{key}: systemd-run didn't answer; waiting for the upload"));
                    return false;
                }
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = exe;
        log(format!("{key}: no way to upload in the background here; waiting"));
        false
    }
}

/// `ludonimbus-run --finish <key>`: the sync after playing a wrapper handed
/// over, tried again for about 10 minutes, then what the wrapper would have
/// done after it. No windows: a conflict is only noted, and the game's next
/// launch asks. Asked to stop (Linux: SIGTERM, say when the session ends),
/// it stops after the try under way: what is still owed stays noted.
fn finish(key: &str) -> i32 {
    log(format!("{key}: finishing the sync after playing"));
    let mut pauses = FINISH_PAUSES.iter();
    loop {
        let done = match Ctx::load().and_then(|ctx| sync::finish_owed(&ctx, key)) {
            Ok(None) => {
                log(format!("{key}: nothing left to do"));
                true
            }
            Ok(Some(out)) => {
                log(format!("{key}: post-exit (finished later) {out:?}"));
                out.is_final()
            }
            Err(e) => {
                log(format!("{key}: finishing the sync failed: {e:#}"));
                false
            }
        };
        if done {
            break;
        }
        match pauses.next() {
            Some(&s) if sleep_unless_stopped(Duration::from_secs(s)) => {}
            Some(_) => {
                log(format!("{key}: asked to stop; the tray app or the game's next sync does the rest"));
                return 0;
            }
            None => {
                log(format!("{key}: not done yet; the tray app or the game's next sync does the rest"));
                break;
            }
        }
    }
    // The rest can wait for another time.
    if process::termination_requested() {
        return 0;
    }
    if let Ok(ctx) = Ctx::load() {
        if let Ok(game) = ctx.game(key) {
            ludonimbus_core::achievements::after_exit(&ctx, &game);
        }
        if !catchup::app_running() {
            catchup::stale_runners(&ctx.cfg, None, &|m| log(m));
            catchup::screenshots(&ctx.cfg, key, &|m| log(m));
        }
    }
    0
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
        "「{}」的另一个版本（{}）有更新的存档：{} 在 {} 保存。\n\n要先把它发到这个版本再开始吗？这个版本现在的存档会先保留，可以在 Ludonimbus 里撤销。",
        "Another version of \"{}\" ({}) has newer saves: saved by {} at {}.\n\nSend them to this version before starting? This version's current saves are kept first, so you can undo it in Ludonimbus.",
        game.name,
        newer.label,
        newer.snapshot.device_name,
        when
    );
    let req = Request::new(tr!("Ludonimbus · 另一个版本有更新的存档", "Ludonimbus · Newer saves in another version"), text, "info")
        .button("apply", tr!("发过来再开始", "Send and start"), "a")
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
    ///
    /// The marker is replaced in one step: written in place, it is empty for
    /// a moment, and a reader that catches that moment would take this
    /// wrapper for one that was terminated and sync the game while it runs.
    fn phase(&self, phase: &str) {
        if phase == "playing" && self.started.get().is_none() {
            self.started.set(Some(chrono::Utc::now()));
        }
        let mut v = serde_json::json!({ "pid": std::process::id(), "phase": phase });
        if let Some(t) = self.started.get() {
            v["startedAt"] = serde_json::json!(t);
        }
        if let Err(e) = util::atomic_write(&self.path, v.to_string().as_bytes()) {
            log(format!("can't write {} ({e:#})", self.path.display()));
        }
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

/// A game Steam installed that Ludonimbus manages: is Steam Cloud still off
/// for it? When it reads on again the game is backup only from now on, and
/// the user is told (once: the next launch finds it backup only already).
/// When that couldn't be written down, it is backup only for this launch.
fn recheck_steam_cloud(ctx: &Ctx, game: &mut GameEntry) {
    if game.store.is_none() {
        return;
    }
    let demoted = match steamcloud::recheck(&ctx.cfg, game) {
        Ok(None) => return,
        Ok(Some(changed)) => changed,
        Err(e) => {
            log(format!("{}: Steam Cloud is on again, but that couldn't be noted: {e:#}", game.key));
            let mut g = game.clone();
            if let Some(s) = g.store.as_mut() {
                s.mode = config::StoreMode::Backup;
            }
            g
        }
    };
    *game = demoted;
    tell(
        &tr!("Ludonimbus · Steam 云存档又打开了", "Ludonimbus · Steam Cloud is on again"),
        &tr!(
            "「{}」的 Steam 云存档又打开了。在关掉它、在 Ludonimbus 里重新接管之前，Ludonimbus 只备份这个游戏的存档，不会写入。",
            "Steam Cloud is on again for \"{}\". Until it is turned off and the game is taken over again in Ludonimbus, Ludonimbus only keeps backups of its saves and writes nothing into them.",
            game.name
        ),
        "warn",
    );
}

fn run(args: &[String]) -> Result<i32> {
    let (key, explicit) = parse_args(args)?;
    let ctx = Ctx::load()?;
    let mut game = ctx.game(&key)?;
    recheck_steam_cloud(&ctx, &mut game);
    let lock = RunningLock::create(&key);

    let launch = match explicit.map(split_envs) {
        // Steam's %command% (Linux): Steam already changed to the shortcut's
        // start folder, and the command may begin with Proton or a tool
        // such as gamemoderun, not with the game.
        Some((envs, cmd)) if !cmd.is_empty() => {
            let exe = PathBuf::from(&cmd[0]);
            let work_dir = std::env::current_dir().unwrap_or_else(|_| exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")));
            Launch { exe, args: cmd[1..].to_vec(), envs, work_dir }
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
            Launch { exe, args: util::split_args(&game.args), envs: Vec::new(), work_dir }
        }
    };

    // Reload the context in case ensure_exe changed the config.
    let ctx = Arc::new(Ctx::load()?);
    // Import the minutes Steam counted so far before this session adds to them.
    if let Err(e) = playtime::ensure(&ctx.cfg, &game) {
        log(format!("{key}: playtime: {e:#}"));
    }

    // The sync before launch when the wrapper stopped waiting for it: it
    // ends by itself, and is joined before the game's next sync.
    let before: std::cell::RefCell<Option<Worker<Result<Outcome>>>> = std::cell::RefCell::new(None);
    let join_before = || {
        if let Some(w) = before.borrow_mut().take() {
            match w.join() {
                Some(out) => log(format!("{key}: pre-launch (not waited for) {:?}", out.map_err(|e| format!("{e:#}")))),
                None => log(format!("{key}: pre-launch (not waited for) ended without a result")),
            }
        }
    };
    // Steam Cloud brings the saves of a game it keeps: nothing to do before
    // playing, and after it only a version is stored.
    if game.backup_only() {
        log(format!("{key}: Steam Cloud keeps this game's saves; no sync before launch"));
    }
    if game.auto_sync && !game.backup_only() {
        let budget = Duration::from_secs(ctx.cfg.launch_wait_secs);
        let mut notice = |phase| download_notice(&game, phase);
        let first = match wait_before_launch(start_sync(&ctx, &game, Resolution::Auto, Trigger::PreLaunch, true), budget, &mut notice) {
            Waited::Finished(out) => Some(out),
            Waited::Lost => Some(Err(anyhow!(tr!("启动前的同步意外中止了", "The sync before launch stopped unexpectedly")))),
            Waited::GaveUp(w) => {
                not_waited(&key, budget);
                *before.borrow_mut() = Some(w);
                None
            }
        };
        // Asked to stop meanwhile: no questions either.
        if stopped_before_launch(&key) {
            return Ok(0);
        }
        match first {
            None => {}
            Some(Ok(Outcome::Conflict { local_files, local_size, cloud })) => {
                let res = match ask_conflict(&game, local_files, local_size, &cloud) {
                    Choice::Cloud => Resolution::UseCloud,
                    Choice::Local => Resolution::UseLocal,
                    Choice::Cancel => return Ok(0),
                };
                match wait_before_launch(start_sync(&ctx, &game, res, Trigger::PreLaunch, false), budget, &mut notice) {
                    Waited::Finished(out) => log(format!("{key}: conflict resolved: {:?}", out?)),
                    Waited::Lost => bail!(tr!("启动前的同步意外中止了", "The sync before launch stopped unexpectedly")),
                    Waited::GaveUp(w) => {
                        not_waited(&key, budget);
                        *before.borrow_mut() = Some(w);
                    }
                }
            }
            Some(Ok(out)) => {
                log(format!("{key}: pre-launch {out:?}"));
                if !matches!(out, Outcome::Offline { .. }) {
                    offer_newer_version(&ctx, &game);
                }
            }
            Some(Err(e)) => {
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
        join_before();
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

    // Steam's "Stop" during the sync before launch, or the questions
    // (Linux: SIGTERM): the game isn't started.
    if stopped_before_launch(&key) {
        return Ok(0);
    }
    if launch.envs.is_empty() {
        log(format!("{key}: launching {} {:?}", launch.exe.display(), launch.args));
    } else {
        log(format!("{key}: launching {} {:?} with {:?}", launch.exe.display(), launch.args, launch.envs));
    }
    lock.phase("playing");
    let code = process::launch_and_wait(
        &launch.exe,
        &launch.args,
        &launch.envs,
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

    // Handed over: the rest of the sync after playing, and what follows it
    // here, is done by the tray app or the process started for it.
    let mut handed_over = false;
    if game.auto_sync {
        lock.phase("uploading");
        settle();
        // Never two syncs of one game at once.
        join_before();
        log(format!("{key}: uploading saves"));
        let budget = (!ctx.cfg.wait_for_upload).then(|| Duration::from_secs(ctx.cfg.exit_wait_secs));
        let result = match wait_after_exit(start_sync(&ctx, &game, Resolution::Auto, Trigger::AutoExit, true), budget) {
            AfterExit::Finished(out) => Some(out),
            AfterExit::Lost => Some(Err(anyhow!(tr!("退出后的同步意外中止了", "The sync after playing stopped unexpectedly")))),
            AfterExit::Running(w) if hand_over(&key) => {
                handed_over = true;
                match stop_after_hand_over(w, STOP_AFTER_HAND_OVER) {
                    Poll::Ready(out) => log(format!("{key}: stopped after the hand-over: {:?}", out.map_err(|e| format!("{e:#}")))),
                    Poll::Lost => log(format!("{key}: the sync after playing ended without a result")),
                    Poll::Pending => log(format!("{key}: still uploading; whoever took over goes on from what got through")),
                }
                None
            }
            AfterExit::Running(w) => {
                log(format!("{key}: waiting for the upload to finish"));
                Some(w.join().unwrap_or_else(|| Err(anyhow!(tr!("退出后的同步意外中止了", "The sync after playing stopped unexpectedly")))))
            }
        };
        match result {
            None => {}
            Some(Ok(Outcome::Conflict { cloud, .. })) => {
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
            Some(Ok(out)) => log(format!("{key}: post-exit {out:?}")),
            Some(Err(e)) => {
                log(format!("{key}: post-exit sync failed: {e:#}"));
                let _ = ludonimbus_core::state::update_game(&key, |g| g.last_error = Some(format!("{e:#}")));
            }
        }
    }
    if handed_over {
        return Ok(code);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A stand-in for a sync: goes through `phases`, each for so many
    /// milliseconds, and ends with "done"; once the waiting is given up it
    /// stops ("stopped").
    fn fake(phases: &[(Phase, u64)]) -> Worker<&'static str> {
        let phases = phases.to_vec();
        Worker::start(move |gate: &Gate| {
            for (phase, length) in phases {
                if phase == Phase::Applying {
                    if !gate.start_applying() {
                        return "stopped";
                    }
                } else {
                    gate.set_phase(phase);
                }
                let end = Instant::now() + ms(length);
                while Instant::now() < end {
                    if gate.given_up() {
                        return "stopped";
                    }
                    std::thread::sleep(ms(5));
                }
            }
            gate.set_phase(Phase::Done);
            "done"
        })
    }

    fn downloading() -> Phase {
        Phase::Downloading { done: 0, total: 10 }
    }

    /// The window shown while downloading, as far as the waiting sees it.
    struct FakeNotice {
        gate: Gate,
        /// What the user does, given what the sync does.
        user: fn(Phase) -> Answer,
        closed: Arc<AtomicBool>,
    }

    impl Notice for FakeNotice {
        fn answer(&mut self) -> Answer {
            (self.user)(self.gate.phase())
        }
    }

    impl Drop for FakeNotice {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    /// Wait like the wrapper before launch, with a window that does what
    /// `user` says (none: it can't be shown). Returns how it ended, how
    /// often a window was asked for, and whether it was closed again.
    fn before_launch(worker: Worker<&'static str>, budget: Duration, user: Option<fn(Phase) -> Answer>) -> (Waited<&'static str>, usize, bool) {
        let gate = worker.gate.clone();
        let asked = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicBool::new(false));
        let (a, c) = (asked.clone(), closed.clone());
        let waited = wait_before_launch(worker, budget, &mut |phase| {
            assert!(matches!(phase, Phase::Downloading { .. }), "a window only for a download, not {phase:?}");
            a.fetch_add(1, Ordering::SeqCst);
            user.map(|user| Box::new(FakeNotice { gate: gate.clone(), user, closed: c.clone() }) as Box<dyn Notice>)
        });
        (waited, asked.load(Ordering::SeqCst), closed.load(Ordering::SeqCst))
    }

    fn finished(w: Waited<&'static str>) -> &'static str {
        match w {
            Waited::Finished(v) => v,
            Waited::GaveUp(_) => panic!("expected it to be waited for, it was given up"),
            Waited::Lost => panic!("expected a result, it was lost"),
        }
    }

    fn gave_up(w: Waited<&'static str>) -> Worker<&'static str> {
        match w {
            Waited::GaveUp(worker) => worker,
            Waited::Finished(v) => panic!("expected it to be given up, it ended with {v}"),
            Waited::Lost => panic!("expected it to be given up, it was lost"),
        }
    }

    #[test]
    fn a_sync_that_ends_in_time_is_waited_for() {
        let (w, asked, _) = before_launch(fake(&[(Phase::Checking, 20), (downloading(), 20), (Phase::Applying, 20)]), ms(2000), None);
        assert_eq!(finished(w), "done");
        assert_eq!(asked, 0);
    }

    #[test]
    fn checking_or_uploading_past_the_time_is_given_up() {
        for phase in [Phase::Checking, Phase::Uploading] {
            let start = Instant::now();
            let (w, asked, _) = before_launch(fake(&[(phase, 5000)]), ms(100), None);
            let worker = gave_up(w);
            assert!(start.elapsed() < ms(1000));
            assert_eq!(asked, 0);
            assert!(worker.gate.given_up());
            // It stops by itself, and is joined before the next sync.
            assert_eq!(worker.join(), Some("stopped"));
        }
    }

    #[test]
    fn the_pause_before_trying_again_is_part_of_the_time() {
        let worker = Worker::start(|gate: &Gate| {
            let attempt = || {
                // As a sync that ends without reaching the cloud.
                gate.set_phase(Phase::Done);
                Ok(Outcome::Offline { message: "no".into() })
            };
            retrying(&[Duration::from_secs(5)], true, gate, attempt, &|_| {}).is_ok()
        });
        let start = Instant::now();
        match wait_before_launch(worker, ms(100), &mut |_| panic!("no window while checking")) {
            Waited::GaveUp(w) => assert_eq!(w.join(), Some(true)),
            _ => panic!("expected it to be given up"),
        }
        assert!(start.elapsed() < ms(1000));
    }

    #[test]
    fn a_download_past_the_time_is_waited_for_without_a_window() {
        let (w, asked, _) = before_launch(fake(&[(downloading(), 250), (Phase::Applying, 20)]), ms(50), None);
        assert_eq!(finished(w), "done");
        assert_eq!(asked, 1);
    }

    #[test]
    fn the_user_can_stop_waiting_for_a_download() {
        let (w, asked, closed) = before_launch(fake(&[(downloading(), 5000)]), ms(50), Some(|_| Answer::PlayLocal));
        let worker = gave_up(w);
        assert_eq!(asked, 1);
        assert!(closed, "the window is closed");
        assert_eq!(worker.join(), Some("stopped"));
    }

    #[test]
    fn a_download_that_ends_first_closes_the_window() {
        let (w, asked, closed) = before_launch(fake(&[(downloading(), 200), (Phase::Applying, 20)]), ms(50), Some(|_| Answer::Open));
        assert_eq!(finished(w), "done");
        assert_eq!(asked, 1);
        assert!(closed);
    }

    #[test]
    fn replacing_the_saves_is_waited_for_even_when_the_user_stops_waiting() {
        let user: fn(Phase) -> Answer = |p| if p == Phase::Applying { Answer::PlayLocal } else { Answer::Open };
        let worker = fake(&[(downloading(), 150), (Phase::Applying, 150)]);
        let gate = worker.gate.clone();
        let (w, _, closed) = before_launch(worker, ms(50), Some(user));
        assert_eq!(finished(w), "done");
        assert!(!gate.given_up());
        assert!(closed);
        // An upload after it (the version that settles a conflict) isn't
        // waited for.
        let worker = fake(&[(downloading(), 150), (Phase::Applying, 150), (Phase::Uploading, 5000)]);
        let start = Instant::now();
        let (w, _, _) = before_launch(worker, ms(50), Some(user));
        let worker = gave_up(w);
        assert!(start.elapsed() < ms(1000));
        assert_eq!(worker.join(), Some("stopped"));
    }

    #[test]
    fn a_window_that_doesnt_show_means_waiting_for_the_download() {
        let (w, asked, _) = before_launch(fake(&[(downloading(), 200), (Phase::Applying, 20)]), ms(50), Some(|_| Answer::Gone));
        assert_eq!(finished(w), "done");
        assert_eq!(asked, 1);
    }

    #[test]
    fn after_exit_the_upload_is_waited_for_a_while() {
        match wait_after_exit(fake(&[(Phase::Uploading, 20)]), Some(ms(2000))) {
            AfterExit::Finished(v) => assert_eq!(v, "done"),
            _ => panic!("expected it to end in time"),
        }
        // Past the time it goes on: it is handed over, not given up.
        match wait_after_exit(fake(&[(Phase::Uploading, 300)]), Some(ms(50))) {
            AfterExit::Running(w) => {
                assert!(!w.gate.given_up());
                assert_eq!(w.join(), Some("done"));
            }
            _ => panic!("expected it to be still running"),
        }
        // "Always wait for the upload".
        match wait_after_exit(fake(&[(Phase::Uploading, 200)]), None) {
            AfterExit::Finished(v) => assert_eq!(v, "done"),
            _ => panic!("expected it to be waited for"),
        }
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn only_the_wrappers_own_finish_counts() {
        assert_eq!(finishing(&words("--finish celeste")), Some("celeste"));
        assert_eq!(finishing(&words("--game celeste")), None);
        // The game's own `--finish`, after `--`.
        assert_eq!(finishing(&words("--game celeste -- /games/celeste --finish now")), None);
        assert_eq!(finishing(&words("--game celeste -- --finish")), None);
        assert_eq!(finishing(&words("--finish")), None);
    }

    #[test]
    fn variables_in_front_of_the_command_are_the_games_environment() {
        let (envs, cmd) = split_envs(words("MANGOHUD=1 DXVK_HUD=fps,frametimes gamemoderun /proton run game.exe A=B"));
        assert_eq!(envs, vec![("MANGOHUD".to_string(), "1".to_string()), ("DXVK_HUD".to_string(), "fps,frametimes".to_string())]);
        assert_eq!(cmd, words("gamemoderun /proton run game.exe A=B"));
        // A program is never taken for a variable.
        for cmd in ["/opt/a=b/game", r"C:\Games\a=b\game.exe", "=x game", "1X=2 game"] {
            assert_eq!(split_envs(words(cmd)), (Vec::new(), words(cmd)), "{cmd}");
        }
        assert_eq!(split_envs(words("FOO=")), (vec![("FOO".to_string(), String::new())], Vec::new()));
    }

    #[test]
    fn a_sync_handed_over_stops_at_a_safe_point() {
        // Uploading: it stops at its next step.
        let start = Instant::now();
        assert!(matches!(stop_after_hand_over(fake(&[(Phase::Uploading, 5000)]), ms(2000)), Poll::Ready("stopped")));
        assert!(start.elapsed() < ms(1000));
        // Replacing the saves is waited for, then it stops before the next step.
        let worker = fake(&[(downloading(), 20), (Phase::Applying, 600), (Phase::Uploading, 5000)]);
        let gate = worker.gate.clone();
        let start = Instant::now();
        while gate.phase() != Phase::Applying {
            assert!(start.elapsed() < ms(5000), "it never got to replacing the saves");
            std::thread::sleep(ms(2));
        }
        let start = Instant::now();
        assert!(matches!(stop_after_hand_over(worker, ms(2000)), Poll::Ready("stopped")));
        assert!(start.elapsed() >= ms(300), "the saves were being replaced");
        assert!(gate.given_up());
        // Work that doesn't stop (an upload under way) gets `max`, no more.
        let busy = Worker::start(|_: &Gate| {
            std::thread::sleep(ms(3000));
            "done"
        });
        let start = Instant::now();
        assert!(matches!(stop_after_hand_over(busy, ms(100)), Poll::Pending));
        assert!(start.elapsed() < ms(1000));
    }

    #[test]
    fn tries_again_only_while_the_cloud_isnt_reached() {
        let waits = [ms(10), ms(10)];
        let offline = || Outcome::Offline { message: "no".into() };
        let queued = || Outcome::Queued { id: "q".into(), reason: "later".into() };
        let tries = |launching: bool, gate: &Gate, out: &dyn Fn() -> Outcome| {
            let mut n = 0;
            let attempt = || {
                n += 1;
                Ok(out())
            };
            retrying(&waits, launching, gate, attempt, &|_| {}).unwrap();
            n
        };
        assert_eq!(tries(true, &Gate::new(), &offline), 3);
        // Before launch a version kept for after the game is no reason.
        assert_eq!(tries(true, &Gate::new(), &queued), 1);
        assert_eq!(tries(false, &Gate::new(), &queued), 3);
        assert_eq!(tries(true, &Gate::new(), &|| Outcome::UpToDate), 1);
        // Nobody waits any more: no more tries.
        let gate = Gate::new();
        gate.give_up();
        assert_eq!(tries(true, &gate, &offline), 1);
        // Given up during a pause: it ends right away.
        let gate = Gate::new();
        let other = gate.clone();
        let stop = std::thread::spawn(move || {
            std::thread::sleep(ms(50));
            other.give_up();
        });
        let start = Instant::now();
        let mut n = 0;
        let attempt = || {
            n += 1;
            Ok(offline())
        };
        retrying(&[Duration::from_secs(5), Duration::from_secs(5)], true, &gate, attempt, &|_| {}).unwrap();
        stop.join().unwrap();
        assert_eq!(n, 1);
        assert!(start.elapsed() < ms(1000));
    }
}
