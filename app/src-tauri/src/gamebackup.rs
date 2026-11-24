//! Commands for game backups and the cloud speed test (docs/plan-game-backup.md).

use crate::commands::{blocking, CmdResult};
use ludonimbus_core::config;
use ludonimbus_core::gamebackup::{self, speedtest};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

static SPEED_CANCEL: AtomicBool = AtomicBool::new(false);
static SPEED_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn speed_test(app: AppHandle, mb: Option<u64>, threads: Option<usize>) -> CmdResult<speedtest::SpeedReport> {
    if SPEED_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(ludonimbus_core::tr!("测速已经在进行中", "A speed test is already running"));
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
        // At most a few events per second.
        let last = Mutex::new(Instant::now() - std::time::Duration::from_secs(1));
        let progress = |p: &speedtest::SpeedProgress| {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_millis() >= 200 || p.done_bytes == 0 || p.done_bytes == p.total_bytes {
                *l = Instant::now();
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
