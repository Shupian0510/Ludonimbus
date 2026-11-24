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
    let store = gamebackup::open_store(&cfg)?;
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
    let result = speedtest::run(store.as_ref(), &opts, &cancel, &progress);
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
