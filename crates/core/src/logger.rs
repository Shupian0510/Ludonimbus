//! Tiny append-only file logger (`%APPDATA%\Ludonimbus\logs\<component>.log`).

use std::io::Write;

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

pub fn log(component: &str, msg: impl AsRef<str>) {
    let path = crate::paths::logs_dir().join(format!("{component}.log"));
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
    }
    let line = format!(
        "{} [{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        std::process::id(),
        msg.as_ref()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
    if cfg!(debug_assertions) {
        eprint!("[{component}] {line}");
    }
}
