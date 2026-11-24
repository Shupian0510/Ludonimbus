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
    let opts = speedtest::SpeedOptions {
        total_bytes: a.mb * MB,
        object_bytes: a.object_mb * MB,
        threads: a.threads,
        max_secs: a.max_secs,
    };
    eprintln!(
        "测速：{}（{}），每轮 {} MB，对象 {} MB，并行 {} 线程，每轮最多 {} 秒",
        store.label(),
        store.kind(),
        a.mb,
        a.object_mb,
        a.threads,
        a.max_secs
    );
    let last = Mutex::new(String::new());
    let progress = |p: &speedtest::SpeedProgress| {
        let line = if p.phase == "cleanup" {
            format!("cleanup {}/{}", p.done_bytes, p.total_bytes)
        } else if p.total_bytes == 0 {
            p.phase.to_string()
        } else {
            format!(
                "{:<16} {:>9} / {:<9} {:>5.1}s",
                p.phase,
                human(p.done_bytes),
                human(p.total_bytes),
                p.elapsed_secs
            )
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
    let eta = |bytes: u64, bps: f64| {
        speedtest::eta_secs(bytes, bps)
            .map(|s| format!("{:.1} 小时", s as f64 / 3600.0))
            .unwrap_or_else(|| "-".into())
    };
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
    /// Back up a game's folder (saves, logs and caches are left out)
    Backup {
        key: String,
        /// Folder to back up (default: the game's folder)
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        #[arg(long)]
        threads: Option<usize>,
        /// Versions to keep afterwards (0 = all; default 2)
        #[arg(long)]
        keep: Option<usize>,
        /// Extra exclude pattern, relative to the folder (repeatable)
        #[arg(long = "exclude")]
        exclude: Vec<String>,
    },
    /// List the backed-up versions of a game
    List { key: String },
    /// Restore a version into a folder, then fix the game's paths and pull its saves
    Restore {
        key: String,
        /// Folder to restore into (a subfolder is used when it isn't empty)
        #[arg(long)]
        to: std::path::PathBuf,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        threads: Option<usize>,
        /// Don't point the game and its Steam shortcut at the restored folder
        #[arg(long)]
        no_fix: bool,
        /// Don't pull the saves afterwards
        #[arg(long)]
        no_saves: bool,
    },
    /// Delete a version and the chunks no other version uses
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

pub fn run(cmd: Cmd) -> Result<()> {
    use gamebackup::{backup, install, remote, restore};
    let cfg = config::load()?;
    let provider = gamebackup::open(&cfg)?;
    let store = provider.files();
    let cancel = AtomicBool::new(false);
    match cmd {
        Cmd::Backup { key, root, threads, keep, exclude } => {
            let g = cfg.require_game(&key)?.clone();
            let s = gamebackup::settings(&key);
            let root = root
                .or_else(|| gamebackup::backup_root(&g, &s))
                .ok_or_else(|| anyhow::anyhow!("不知道游戏文件夹，请用 --root 指定"))?;
            let latest = gamebackup::latest(store, &key).ok().flatten();
            let user_exclude =
                if exclude.is_empty() { gamebackup::user_excludes(&s, latest.as_ref().map(|(_, m)| m)) } else { exclude };
            if let Some(f) = gamebackup::scan::file_outside(&g, &root) {
                eprintln!("注意：{f} 不在 {} 里，不会被备份", root.display());
            }
            let opts = backup::BackupOptions {
                root,
                user_exclude,
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
                    "{} {} {:<14} {:>9} {} 个文件  新上传 {}",
                    s.id,
                    s.created_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
                    s.device_name,
                    human(s.total_size),
                    s.file_count,
                    human(s.new_bytes)
                );
            }
            println!("云端占用 {}", human(remote::stored_bytes(store, &key)?));
        }
        Cmd::Restore { key, to, version, threads, no_fix, no_saves } => {
            let s = pick(remote::list_versions(store, &key)?, version.as_deref())?;
            let target = restore::resolve_target(&to, &s.root, &s.id);
            eprintln!("恢复 {} 到 {}", s.id, target.display());
            let show = line_progress();
            let threads = threads.unwrap_or_else(|| gamebackup::default_threads(store.kind()));
            let result = restore::restore(store, &key, &s, &target, threads, &cancel, &|p| show(p.phase, p.done, p.total));
            eprintln!();
            let (m, report) = result?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !no_fix && cfg.game(&key).is_some() {
                println!("{}", serde_json::to_string_pretty(&install::fix_paths(&key, &m, &target)?)?);
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
            let gc = remote::collect_garbage(store, &key, &|_, _| {})?;
            lock.release()?;
            println!("已删除 {version}，清理了 {} 个数据块（{}）", gc.deleted_chunks, human(gc.freed_bytes));
        }
        Cmd::Check { key, version } => {
            let s = pick(remote::list_versions(store, &key)?, version.as_deref())?;
            let m = remote::read_manifest(store, &key, &s)?;
            let present = remote::list_chunks(store, &key)?;
            let missing: Vec<&str> =
                m.chunks.iter().filter(|c| present.get(&c.h).is_none_or(|z| *z == 0)).map(|c| c.h.as_str()).collect();
            if missing.is_empty() {
                println!("版本 {} 完整：{} 个数据块都在", s.id, m.chunks.len());
            } else {
                anyhow::bail!("版本 {} 缺少 {} 个数据块，例如 {}", s.id, missing.len(), missing[0]);
            }
        }
    }
    Ok(())
}
