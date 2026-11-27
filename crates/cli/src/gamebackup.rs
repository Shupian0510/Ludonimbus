//! `ludonimbus speedtest` and `ludonimbus game-backup …`.

use anyhow::Result;
use clap::Args;
use ludonimbus_core::config;
use ludonimbus_core::gamebackup::{self, speedtest};
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use crate::human;

#[derive(Args)]
pub struct SpeedArgs {
    /// Megabytes each phase moves
    #[arg(long, default_value_t = 200)]
    mb: u64,
    /// Size of one test object in MB (game backups use 8)
    #[arg(long, default_value_t = 8)]
    object_mb: u64,
    /// Workers in the parallel phases
    #[arg(long, default_value_t = 4)]
    threads: usize,
    /// Each phase starts no new object after this many seconds
    #[arg(long, default_value_t = 90)]
    max_secs: u64,
    /// Print the report as JSON
    #[arg(long)]
    json: bool,
}

fn rate(bps: f64) -> String {
    format!("{}/s", human(bps as u64))
}

fn phase_line(name: &str, p: &Option<speedtest::PhaseResult>) -> String {
    match p {
        None => format!("{name:<14} -"),
        Some(p) => format!(
            "{name:<14} {:>12}  {} 个对象 {:>9} 用时 {:.1} 秒，{} 线程{}{}",
            rate(p.bytes_per_sec),
            p.objects,
            human(p.bytes),
            p.secs,
            p.threads,
            if p.capped { "（到时间上限，提前结束）" } else { "" },
            p.error.as_ref().map(|e| format!("  错误：{e}")).unwrap_or_default()
        ),
    }
}

pub fn speed(a: SpeedArgs) -> Result<()> {
    let cfg = config::load()?;
    let provider = gamebackup::open(&cfg)?;
    let store = provider.files();
    const MB: u64 = 1024 * 1024;
    let opts = speedtest::SpeedOptions { total_bytes: a.mb * MB, object_bytes: a.object_mb * MB, threads: a.threads, max_secs: a.max_secs };
    eprintln!("测速：{}（{}），每轮 {} MB，对象 {} MB，并行 {} 线程，每轮最多 {} 秒", store.label(), store.kind(), a.mb, a.object_mb, a.threads, a.max_secs);
    let last = Mutex::new(String::new());
    let progress = |p: &speedtest::SpeedProgress| {
        let line = if p.phase == "cleanup" {
            format!("cleanup {}/{}", p.done_bytes, p.total_bytes)
        } else if p.total_bytes == 0 {
            p.phase.to_string()
        } else {
            format!("{:<16} {:>9} / {:<9} {:>5.1}s", p.phase, human(p.done_bytes), human(p.total_bytes), p.elapsed_secs)
        };
        let mut l = last.lock().unwrap();
        if *l != line {
            eprint!("\r{line:<60}");
            let _ = std::io::stderr().flush();
            *l = line;
        }
    };
    let cancel = AtomicBool::new(false);
    let result = speedtest::run(store, &opts, &cancel, &progress);
    eprintln!();
    let r = result?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&r)?);
        return Ok(());
    }
    println!("云盘          {} ({})", r.label, r.kind);
    println!("小文件        写入 {:.0} ms，读取 {:.0} ms（每次请求的固定开销）", r.small_write_ms, r.small_read_ms);
    println!("{}", phase_line("上传 单线程", &r.upload_serial));
    println!("{}", phase_line("上传 并行", &r.upload_parallel));
    println!("{}", phase_line("下载 单线程", &r.download_serial));
    println!("{}", phase_line("下载 并行", &r.download_parallel));
    let eta = |bytes: u64, bps: f64| speedtest::eta_secs(bytes, bps).map(|s| format!("{:.1} 小时", s as f64 / 3600.0)).unwrap_or_else(|| "-".into());
    println!(
        "估算          10 GB 上传 {}，下载 {}；50 GB 上传 {}，下载 {}",
        eta(10 << 30, r.upload_bps),
        eta(10 << 30, r.download_bps),
        eta(50 << 30, r.upload_bps),
        eta(50 << 30, r.download_bps)
    );
    println!("结论          {:?}", r.verdict);
    for n in &r.notes {
        println!("说明          {n}");
    }
    if let Some(e) = &r.cleanup_error {
        println!("清理失败      {e}");
    }
    Ok(())
}

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Back up a game: its program, game files, settings (and firmware with --firmware)
    Backup {
        key: String,
        /// The program's folder, or the whole folder (default: the game's folder)
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        #[arg(long)]
        threads: Option<usize>,
        /// Versions to keep afterwards (0 = all; default 2)
        #[arg(long)]
        keep: Option<usize>,
        /// Extra exclude pattern, relative to the program's folder (repeatable)
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        /// Also back up keys, firmware and BIOS
        #[arg(long)]
        firmware: bool,
        /// Show the parts and sizes without backing up
        #[arg(long)]
        dry_run: bool,
    },
    /// List the backed-up versions of a game
    List { key: String },
    /// Restore a version, then fix the game's paths and pull its saves
    Restore {
        key: String,
        /// Folder for the program (or the whole folder); a subfolder is used when it isn't empty
        #[arg(long)]
        to: std::path::PathBuf,
        /// Folder for the game's files (disc image, ROM); default: the same as --to's parent
        #[arg(long)]
        content: Option<std::path::PathBuf>,
        /// Keep the program already in --to (only restore the game's own parts)
        #[arg(long)]
        keep_program: bool,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        threads: Option<usize>,
        /// Don't point the game and its Steam shortcut at the restored folders
        #[arg(long)]
        no_fix: bool,
        /// Don't pull the saves afterwards
        #[arg(long)]
        no_saves: bool,
    },
    /// Delete a version and the chunks no version of any game uses
    Delete { key: String, version: String },
    /// Check that every chunk of a version is in the cloud
    Check {
        key: String,
        #[arg(long)]
        version: Option<String>,
    },
}

fn pick(list: Vec<gamebackup::format::Summary>, version: Option<&str>) -> Result<gamebackup::format::Summary> {
    match version {
        Some(v) => list.into_iter().find(|s| s.id == v).ok_or_else(|| anyhow::anyhow!("没有版本 {v}")),
        None => list.into_iter().last().ok_or_else(|| anyhow::anyhow!("云端还没有这个游戏的本体备份")),
    }
}

fn line_progress() -> impl Fn(&str, u64, u64) + Sync {
    let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
    move |phase: &str, done: u64, total: u64| {
        let mut l = last.lock().unwrap();
        if l.elapsed().as_millis() < 300 && done < total {
            return;
        }
        *l = std::time::Instant::now();
        if total > 0 {
            eprint!("\r{phase:<12} {:>9} / {:<9}", human(done), human(total));
        } else {
            eprint!("\r{phase:<12} {:<22}", "");
        }
        let _ = std::io::stderr().flush();
    }
}

fn parts(list: &[gamebackup::format::ComponentSummary]) -> String {
    list.iter()
        .map(|c| match &c.version {
            Some(v) => format!("{} {v} ({})", c.name, human(c.total_size)),
            None => format!("{} ({})", c.name, human(c.total_size)),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The parts a backup of `key` would have, from this PC only (no cloud).
fn dry_run(cfg: &config::AppConfig, key: &str, root: Option<std::path::PathBuf>, exclude: Vec<String>, firmware: bool) -> Result<()> {
    let g = cfg.require_game(key)?.clone();
    let mut comp = gamebackup::options(&gamebackup::settings(key), None);
    if root.is_some() {
        comp.root = root;
    }
    if !exclude.is_empty() {
        comp.user_exclude = exclude;
    }
    comp.firmware |= firmware;
    for c in gamebackup::components::plan(&g, &comp)? {
        println!(
            "{:<9} {:<28} {:<12} {:>6} 个文件 {:>9}（排除 {} 个，{}）",
            format!("{:?}", c.kind),
            c.name,
            c.version.clone().unwrap_or_default(),
            c.files.len(),
            human(c.total()),
            c.excluded_files,
            human(c.excluded_bytes)
        );
        if let Some(l) = &c.build {
            println!("          {}", ludonimbus_core::tr!("由 {l} 现场编译，编译出来的文件不备份", "Built by {l} on each PC; what it builds isn't backed up"));
        }
        // How many files of each part to list (a number; any other value: 40).
        if let Some(v) = std::env::var_os("LUDONIMBUS_DRY_RUN_FILES") {
            let n = v.to_string_lossy().parse().unwrap_or(40);
            for f in c.files.iter().take(n) {
                println!("    {} {}", human(f.size), f.logical);
            }
            if c.files.len() > n {
                println!("    … 共 {} 个", c.files.len());
            }
        }
    }
    Ok(())
}

pub fn run(cmd: Cmd) -> Result<()> {
    use gamebackup::{backup, install, remote, restore};
    let cfg = config::load()?;
    if let Cmd::Backup { key, root, exclude, firmware, dry_run: true, .. } = cmd {
        return dry_run(&cfg, &key, root, exclude, firmware);
    }
    let provider = gamebackup::open(&cfg)?;
    let store = provider.files();
    let cancel = AtomicBool::new(false);
    match cmd {
        Cmd::Backup { key, root, threads, keep, exclude, firmware, .. } => {
            let g = cfg.require_game(&key)?.clone();
            let s = gamebackup::settings(&key);
            let latest = gamebackup::latest(store, &key).ok().flatten();
            let mut comp = gamebackup::options(&s, latest.as_ref().map(|(_, m)| m));
            if root.is_some() {
                comp.root = root;
            }
            if !exclude.is_empty() {
                comp.user_exclude = exclude;
            }
            comp.firmware |= firmware;
            let opts = backup::BackupOptions {
                components: comp,
                threads: threads.unwrap_or_else(|| gamebackup::default_threads(store.kind())),
                params: gamebackup::chunker::Params::default(),
                keep: match keep {
                    Some(0) => None,
                    Some(n) => Some(n),
                    None => s.keep_count(),
                },
                comment: String::new(),
            };
            let show = line_progress();
            let out = backup::backup(store, &g, &cfg.device, &opts, &cancel, &|p| show(p.phase, p.done, p.total));
            eprintln!();
            println!("{}", serde_json::to_string_pretty(&out?)?);
        }
        Cmd::List { key } => {
            for s in remote::list_versions(store, &key)?.iter().rev() {
                println!(
                    "{} {} {:<14} {:>9}  新上传 {:<9} {}",
                    s.id,
                    s.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
                    s.device_name,
                    human(s.total_size),
                    human(s.new_bytes),
                    parts(&s.components)
                );
            }
            let all: u64 = remote::list_chunks(store)?.values().sum();
            println!("云端所有游戏本体备份共占用 {}", human(all));
        }
        Cmd::Restore { key, to, content, keep_program, version, threads, no_fix, no_saves } => {
            let s = pick(remote::list_versions(store, &key)?, version.as_deref())?;
            let m = remote::read_manifest(store, &key, &s)?;
            let program_base = m.components.iter().find_map(|c| c.bases.get("program")).cloned().unwrap_or_default();
            let program = if keep_program { to.clone() } else { restore::resolve_target(&to, &program_base) };
            let content = m.component(gamebackup::format::Kind::Content).map(|_| content.unwrap_or_else(|| to.parent().unwrap_or(&to).to_path_buf()));
            let targets = restore::Targets { program, restore_program: !keep_program, content };
            eprintln!(
                "恢复 {}：程序到 {}，游戏文件到 {}",
                s.id,
                targets.program.display(),
                targets.content.as_ref().map(|c| c.display().to_string()).unwrap_or("-".into())
            );
            let show = line_progress();
            let threads = threads.unwrap_or_else(|| gamebackup::default_threads(store.kind()));
            let result = restore::restore(store, &key, &s, &targets, threads, &cancel, &|p| show(p.phase, p.done, p.total));
            eprintln!();
            let (m, report) = result?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !no_fix && cfg.game(&key).is_some() {
                println!("{}", serde_json::to_string_pretty(&install::fix_paths(&key, &m, &targets)?)?);
                if !no_saves {
                    match install::pull_saves(&key) {
                        Ok(o) => println!("存档：{}", serde_json::to_string(&o)?),
                        Err(e) => println!("存档没有拉取：{e:#}"),
                    }
                }
            }
        }
        Cmd::Delete { key, version } => {
            let lock = remote::Lock::acquire(store, &key, &cfg.device.id, &cfg.device.name, "cleanup")?;
            remote::delete_version(store, &key, &version)?;
            let gc = remote::collect_garbage(store, (&cfg.device.id, &cfg.device.name), Some(&key), &|_, _| {})?;
            lock.release()?;
            if let Some(reason) = gc.reason.as_deref().filter(|_| gc.postponed) {
                println!("已删除 {version}；不再使用的数据块下次再清理：{reason}");
            } else if gc.postponed {
                println!("已删除 {version}；有设备正在备份，不再使用的数据块下次再清理");
            } else {
                println!("已删除 {version}，清理了 {} 个数据块（{}）", gc.deleted_chunks, human(gc.freed_bytes));
                if gc.recent_chunks > 0 {
                    println!(
                        "{} 个不再使用的数据块（{}）是最近 {} 小时内上传的，下次清理时再删",
                        gc.recent_chunks,
                        human(gc.recent_bytes),
                        remote::CHUNK_GRACE_HOURS
                    );
                }
            }
        }
        Cmd::Check { key, version } => {
            let s = pick(remote::list_versions(store, &key)?, version.as_deref())?;
            let m = remote::read_manifest(store, &key, &s)?;
            let present = remote::list_chunks(store)?;
            let all: Vec<&String> = m.components.iter().flat_map(|c| c.chunks.iter().map(|c| &c.h)).collect();
            let missing: Vec<&&String> = all.iter().filter(|h| present.get(h.as_str()).is_none_or(|z| *z == 0)).collect();
            if missing.is_empty() {
                println!("版本 {} 完整：{} 个数据块都在（{}）", s.id, all.len(), parts(&s.components));
            } else {
                anyhow::bail!("版本 {} 缺少 {} 个数据块，例如 {}", s.id, missing.len(), missing[0]);
            }
        }
    }
    Ok(())
}
