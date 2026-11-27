//! Upload / download speed test of a cloud store.
//!
//! Writes random (incompressible) objects the size game backups upload, one
//! at a time and then several in parallel, reads them back the same two
//! ways, and deletes everything. Every phase stops early after a time limit
//! so a slow cloud (Baidu Netdisk without membership downloads at about
//! 80 KB/s) still finishes in minutes; the speed is then measured on what was
//! moved. The test files sit in the root folder (`.ludonimbus-speed-…`), so
//! no folder is left behind.

use crate::cloud::store::Store;
use anyhow::{anyhow, bail, Result};
use rand::RngCore;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use ts_rs::TS;

pub const PREFIX: &str = ".ludonimbus-speed-";
const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SpeedOptions {
    /// Bytes each phase moves (when the time limit allows).
    pub total_bytes: u64,
    /// Size of one object; the default matches game backup chunks.
    pub object_bytes: u64,
    /// Workers in the parallel phases.
    pub threads: usize,
    /// Each phase starts no new object after this long.
    pub max_secs: u64,
}

impl Default for SpeedOptions {
    fn default() -> Self {
        SpeedOptions { total_bytes: 200 * MIB, object_bytes: 8 * MIB, threads: 4, max_secs: 90 }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SpeedPhase")]
pub struct PhaseResult {
    pub threads: usize,
    pub bytes: u64,
    pub objects: usize,
    pub secs: f64,
    pub bytes_per_sec: f64,
    /// Stopped at the time limit before all the data was moved.
    pub capped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Fine for game backups (both directions at least 5 MB/s).
    Good,
    /// Works, but large games take hours (0.5 to 5 MB/s).
    Slow,
    /// Too slow for games of a few GB (under 0.5 MB/s one way).
    TooSlow,
    /// Something failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpeedReport {
    pub kind: String,
    pub label: String,
    pub object_bytes: u64,
    pub total_bytes: u64,
    /// Average time of one small write and one small read.
    pub small_write_ms: f64,
    pub small_read_ms: f64,
    pub upload_serial: Option<PhaseResult>,
    pub upload_parallel: Option<PhaseResult>,
    pub download_serial: Option<PhaseResult>,
    pub download_parallel: Option<PhaseResult>,
    /// Best measured speeds, bytes per second.
    pub upload_bps: f64,
    pub download_bps: f64,
    pub verdict: Verdict,
    /// Test files that could not be deleted.
    pub cleanup_error: Option<String>,
    /// What is known about this kind of cloud (quotas, limits).
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpeedProgress {
    /// latency | uploadSerial | uploadParallel | downloadSerial | downloadParallel | cleanup
    #[ts(type = r#""latency" | "uploadSerial" | "uploadParallel" | "downloadSerial" | "downloadParallel" | "cleanup""#)]
    pub phase: &'static str,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub elapsed_secs: f64,
}

/// Known limits of a cloud kind (`Store::kind`), in the interface language.
pub fn known_limits(kind: &str) -> Vec<String> {
    match kind {
        "baiduPan" => vec![crate::tr!(
            "百度网盘非会员下载限速：实测 9 MB 用了约 116 秒（约 80 KB/s），10 GB 的游戏要下载一天半以上。上传按 4 MB 分片，每个文件要 3 次以上请求。",
            "Baidu Netdisk throttles downloads without membership: 9 MB took about 116 s in a test (about 80 KB/s), so a 10 GB game takes over a day and a half to download. Uploads go in 4 MB slices with 3 or more requests per file."
        )],
        "quark" => vec![crate::tr!(
            "夸克网盘经过本机的 OpenList 中转：每个请求有 1 秒以上的固定开销，大文件的速度取决于你的宽带。",
            "Quark goes through the OpenList server on this PC: each request has a fixed cost of a second or more; big files go as fast as your connection allows."
        )],
        "googleDrive" => vec![crate::tr!(
            "Google Drive 每个账号每天最多上传 750 GB，超过后要等第二天（备份会从断点继续）。免费空间 15 GB，和 Gmail、相册共用。",
            "Google Drive accepts at most 750 GB of uploads per account per day; beyond that, wait until the next day (backups resume where they stopped). Free accounts have 15 GB, shared with Gmail and Photos."
        )],
        "webDav" => vec![crate::tr!(
            "WebDAV 标准没有分片上传，每个对象用一次 PUT 上传（游戏备份的对象不超过 24 MB，不需要分片）。速度取决于服务器和网络。",
            "Standard WebDAV has no multipart upload; each object is sent in one PUT (game backup objects are at most 24 MB, so none is needed). Speed depends on the server and network."
        )],
        "s3" => vec![crate::tr!(
            "S3 单次 PUT 最大 5 GB，支持分片上传；游戏备份的对象不超过 24 MB，用单次 PUT。注意服务商的流量费（下载通常收费）。",
            "S3 takes up to 5 GB in one PUT and supports multipart uploads; game backup objects are at most 24 MB, so single PUTs are used. Mind the provider's traffic fees (downloads are usually billed)."
        )],
        "localFolder" => vec![crate::tr!(
            "本地或同步文件夹的速度取决于磁盘、网络共享或同步客户端；同步客户端（OneDrive 等）还要在后台再上传一遍。",
            "A local or synced folder is as fast as the disk, network share or sync client; a sync client (OneDrive and so on) uploads everything again in the background."
        )],
        _ => Vec::new(),
    }
}

fn verdict(up: f64, down: f64, failed: bool) -> Verdict {
    let slow = 0.5 * MIB as f64;
    let good = 5.0 * MIB as f64;
    if failed {
        Verdict::Failed
    } else if up < slow || down < slow {
        Verdict::TooSlow
    } else if up < good || down < good {
        Verdict::Slow
    } else {
        Verdict::Good
    }
}

type Progress<'a> = &'a (dyn Fn(&SpeedProgress) + Sync);

struct Run<'a> {
    store: &'a dyn Store,
    id: String,
    opts: SpeedOptions,
    cancel: &'a AtomicBool,
    progress: Progress<'a>,
    /// Every object written, for the cleanup.
    written: Mutex<Vec<String>>,
}

impl Run<'_> {
    fn name(&self, set: char, i: usize) -> String {
        format!("{PREFIX}{}-{set}{i:04}", self.id)
    }

    fn count(&self) -> usize {
        self.opts.total_bytes.div_ceil(self.opts.object_bytes.max(1)).max(1) as usize
    }

    /// Run `job(i)` for object `0..count` on `threads` workers until done,
    /// cancelled, failed or out of time. `job` returns the bytes it moved.
    fn phase(&self, phase: &'static str, threads: usize, count: usize, job: &(dyn Fn(usize) -> Result<u64> + Sync)) -> PhaseResult {
        let total = count as u64 * self.opts.object_bytes;
        let next = AtomicUsize::new(0);
        let done = AtomicU64::new(0);
        let objects = AtomicUsize::new(0);
        let error: Mutex<Option<String>> = Mutex::new(None);
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.opts.max_secs.max(1));
        let report =
            |bytes: u64| (self.progress)(&SpeedProgress { phase, done_bytes: bytes, total_bytes: total, elapsed_secs: started.elapsed().as_secs_f64() });
        report(0);
        let finished = Mutex::new(started);
        std::thread::scope(|s| {
            for _ in 0..threads.max(1) {
                s.spawn(|| loop {
                    if self.cancel.load(Ordering::Relaxed) || error.lock().unwrap().is_some() || Instant::now() >= deadline {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= count {
                        return;
                    }
                    match job(i) {
                        Ok(n) => {
                            let bytes = done.fetch_add(n, Ordering::SeqCst) + n;
                            objects.fetch_add(1, Ordering::SeqCst);
                            *finished.lock().unwrap() = Instant::now();
                            report(bytes);
                        }
                        Err(e) => {
                            error.lock().unwrap().get_or_insert(format!("{e:#}"));
                            return;
                        }
                    }
                });
            }
        });
        let bytes = done.load(Ordering::SeqCst);
        let objects = objects.load(Ordering::SeqCst);
        let secs = (*finished.lock().unwrap() - started).as_secs_f64().max(0.001);
        PhaseResult {
            threads: threads.max(1),
            bytes,
            objects,
            secs,
            bytes_per_sec: if objects > 0 { bytes as f64 / secs } else { 0.0 },
            capped: objects < count,
            error: error.into_inner().unwrap(),
        }
    }

    fn upload(&self, phase: &'static str, set: char, threads: usize) -> PhaseResult {
        let size = self.opts.object_bytes as usize;
        self.phase(phase, threads, self.count(), &|i| {
            let mut data = vec![0u8; size];
            rand::thread_rng().fill_bytes(&mut data);
            let name = self.name(set, i);
            self.written.lock().unwrap().push(name.clone());
            self.store.write(&name, &data)?;
            Ok(data.len() as u64)
        })
    }

    fn download(&self, phase: &'static str, set: char, threads: usize, uploaded: usize) -> PhaseResult {
        let size = self.opts.object_bytes;
        self.phase(phase, threads, uploaded, &|i| {
            let name = self.name(set, i);
            let data = self
                .store
                .read(&name)?
                .ok_or_else(|| anyhow!(crate::tr!("刚上传的测试文件 {name} 读不到", "The test file {name} that was just uploaded can't be read")))?;
            if data.len() as u64 != size {
                bail!(crate::tr!(
                    "测试文件 {name} 读回来大小不对（{} 字节，应为 {size}）",
                    "The test file {name} came back with the wrong size ({} bytes, expected {size})",
                    data.len()
                ));
            }
            Ok(size)
        })
    }

    /// Small write / read / delete round trips: the cost of one request.
    fn latency(&self) -> Result<(f64, f64)> {
        (self.progress)(&SpeedProgress { phase: "latency", done_bytes: 0, total_bytes: 0, elapsed_secs: 0.0 });
        let (mut w, mut r) = (0.0, 0.0);
        const ROUNDS: usize = 3;
        for i in 0..ROUNDS {
            let name = format!("{PREFIX}{}-l{i}", self.id);
            self.written.lock().unwrap().push(name.clone());
            let t = Instant::now();
            self.store.write(&name, format!("Ludonimbus speed test {i}").as_bytes())?;
            w += t.elapsed().as_secs_f64();
            let t = Instant::now();
            self.store.read(&name)?;
            r += t.elapsed().as_secs_f64();
        }
        Ok((w * 1000.0 / ROUNDS as f64, r * 1000.0 / ROUNDS as f64))
    }

    fn cleanup(&self, names: &[String]) -> Option<String> {
        let total = names.len() as u64;
        let done = AtomicU64::new(0);
        let failed: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let next = AtomicUsize::new(0);
        let started = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..self.opts.threads.clamp(1, 8) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(name) = names.get(i) else { return };
                    if let Err(e) = self.store.remove(name) {
                        failed.lock().unwrap().push(format!("{name}: {e:#}"));
                    }
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    (self.progress)(&SpeedProgress { phase: "cleanup", done_bytes: n, total_bytes: total, elapsed_secs: started.elapsed().as_secs_f64() });
                });
            }
        });
        let failed = failed.into_inner().unwrap();
        (!failed.is_empty()).then(|| failed.join("; "))
    }
}

/// Run the speed test on `store`. Test files are deleted at the end, also
/// when a phase fails or the test is cancelled; leftovers of an earlier run
/// that was killed are deleted first.
pub fn run(store: &dyn Store, opts: &SpeedOptions, cancel: &AtomicBool, progress: Progress) -> Result<SpeedReport> {
    store.check()?;
    let opts = SpeedOptions {
        object_bytes: opts.object_bytes.clamp(64 * 1024, 64 * MIB),
        total_bytes: opts.total_bytes.max(opts.object_bytes),
        threads: opts.threads.clamp(1, 16),
        max_secs: opts.max_secs.max(1),
    };
    let run = Run { store, id: ulid::Ulid::new().to_string().to_lowercase(), opts: opts.clone(), cancel, progress, written: Mutex::new(Vec::new()) };
    if let Ok(old) = store.list("") {
        let leftovers: Vec<String> = old.into_iter().filter(|e| !e.is_dir && e.name.starts_with(PREFIX)).map(|e| e.name).collect();
        let _ = run.cleanup(&leftovers);
    }

    let mut report = SpeedReport {
        kind: store.kind().to_string(),
        label: store.label(),
        object_bytes: opts.object_bytes,
        total_bytes: opts.total_bytes,
        small_write_ms: 0.0,
        small_read_ms: 0.0,
        upload_serial: None,
        upload_parallel: None,
        download_serial: None,
        download_parallel: None,
        upload_bps: 0.0,
        download_bps: 0.0,
        verdict: Verdict::Failed,
        cleanup_error: None,
        notes: known_limits(store.kind()),
    };
    let result = (|| -> Result<()> {
        let (w, r) = run.latency()?;
        report.small_write_ms = w;
        report.small_read_ms = r;
        let stop = |p: &PhaseResult| -> Result<()> {
            if cancel.load(Ordering::Relaxed) {
                bail!(crate::tr!("测速已取消", "Speed test cancelled"));
            }
            match &p.error {
                Some(e) => bail!("{e}"),
                None => Ok(()),
            }
        };
        let us = run.upload("uploadSerial", 's', 1);
        report.upload_serial = Some(us.clone());
        stop(&us)?;
        let up = run.upload("uploadParallel", 'p', opts.threads);
        report.upload_parallel = Some(up.clone());
        stop(&up)?;
        let ds = run.download("downloadSerial", 's', 1, us.objects);
        report.download_serial = Some(ds.clone());
        stop(&ds)?;
        let dp = run.download("downloadParallel", 'p', opts.threads, up.objects);
        report.download_parallel = Some(dp.clone());
        stop(&dp)?;
        Ok(())
    })();
    let written = std::mem::take(&mut *run.written.lock().unwrap());
    report.cleanup_error = run.cleanup(&written);
    let best = |a: &Option<PhaseResult>, b: &Option<PhaseResult>| a.iter().chain(b.iter()).map(|p| p.bytes_per_sec).fold(0.0, f64::max);
    report.upload_bps = best(&report.upload_serial, &report.upload_parallel);
    report.download_bps = best(&report.download_serial, &report.download_parallel);
    report.verdict = verdict(report.upload_bps, report.download_bps, result.is_err());
    result?;
    Ok(report)
}

/// Time to move `bytes` at `bps`, in seconds (`None` when nothing was measured).
pub fn eta_secs(bytes: u64, bps: f64) -> Option<u64> {
    (bps > 0.0).then(|| (bytes as f64 / bps).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_thresholds() {
        let m = MIB as f64;
        assert_eq!(verdict(20.0 * m, 50.0 * m, false), Verdict::Good);
        assert_eq!(verdict(2.0 * m, 50.0 * m, false), Verdict::Slow);
        assert_eq!(verdict(20.0 * m, 0.08 * m, false), Verdict::TooSlow);
        assert_eq!(verdict(20.0 * m, 50.0 * m, true), Verdict::Failed);
    }

    #[test]
    fn eta() {
        assert_eq!(eta_secs(10 * MIB, MIB as f64), Some(10));
        assert_eq!(eta_secs(10, 0.0), None);
    }
}
