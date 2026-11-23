//! Launching games and waiting for the whole process tree to exit.
//!
//! Launchers often start the real game and exit immediately, so the game is
//! started inside a Windows Job Object and we wait until the job has no
//! active processes. Process-name watching is the fallback for games that
//! escape the job.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Names (lowercase) of all running processes.
pub fn process_names() -> Vec<String> {
    #[cfg(windows)]
    {
        win::process_names()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn is_running(names: &[String]) -> bool {
    if names.is_empty() {
        return false;
    }
    let want: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    process_names().iter().any(|p| want.contains(p))
}

pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        win::pid_alive(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// Start the game and block until it (and everything it spawned) has exited.
/// Returns the exit code of the started process.
///
/// With `on_game_exit`, the `wait_for` processes (the real game) are watched
/// while the launcher is still open, and the callback runs every time the
/// game exits, so saves can be uploaded without waiting for the launcher.
pub fn launch_and_wait(
    exe: &Path,
    args: &[String],
    work_dir: &Path,
    wait_for: &[String],
    mut on_game_exit: Option<&mut dyn FnMut()>,
    log: &dyn Fn(&str),
) -> Result<i32> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).current_dir(work_dir);

    #[cfg(windows)]
    let job = win::Job::new().ok();

    let mut child = cmd
        .spawn()
        .with_context(|| format!("无法启动 {}", exe.display()))?;
    log(&format!("started pid {}", child.id()));

    #[cfg(windows)]
    let tracked = match &job {
        Some(j) => match j.assign(&child) {
            Ok(()) => true,
            Err(e) => {
                log(&format!("job assign failed: {e}; falling back to pid wait"));
                false
            }
        },
        None => false,
    };
    #[cfg(not(windows))]
    let tracked = false;

    let started = Instant::now();
    let mut exit: Option<(i32, Duration)> = None;
    // Poll the game processes (cheap) to know whether the game was seen, and
    // to report each game exit while the launcher stays open.
    let watch_game = !wait_for.is_empty();
    let mut game_seen = false;
    let mut game_ever_seen = false;
    let mut game_gone_since: Option<Instant> = None;
    loop {
        if exit.is_none() {
            if let Some(status) = child.try_wait()? {
                let code = status.code().unwrap_or(0);
                exit = Some((code, started.elapsed()));
                log(&format!("main process exited with {code} after {}s", started.elapsed().as_secs()));
            }
        }
        #[cfg(windows)]
        let active = match (&job, tracked) {
            (Some(j), true) => j.active_processes() > 0,
            _ => exit.is_none(),
        };
        #[cfg(not(windows))]
        let active = exit.is_none();

        if watch_game {
            if is_running(wait_for) {
                if !game_seen {
                    log("game process is running");
                }
                game_seen = true;
                game_ever_seen = true;
                game_gone_since = None;
            } else if game_seen
                && game_gone_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(3)
            {
                game_seen = false;
                game_gone_since = None;
                if active {
                    if let Some(cb) = on_game_exit.as_mut() {
                        log("game exited while the launcher is still open");
                        cb();
                    }
                }
            }
        }
        if !active {
            break;
        }
        std::thread::sleep(Duration::from_millis(if watch_game { 1000 } else { 500 }));
    }
    if tracked {
        log("job is empty");
    }
    let (code, main_lifetime) = match exit {
        Some(e) => e,
        None => (child.wait()?.code().unwrap_or(0), started.elapsed()),
    };

    // Fallback: the real game may live outside the job. Only wait for it to
    // appear when the launcher handed off quickly (or tracking failed);
    // otherwise a single check is enough and exiting stays instant.
    if !wait_for.is_empty() {
        // Already watched the game run and exit above: nothing to wait for.
        let handoff = !game_ever_seen && (!tracked || main_lifetime < Duration::from_secs(20));
        // The Xbox app may check the license or update before the game starts.
        let wait = if crate::xbox::is_launch_helper(exe) { 30 } else { 8 };
        let appear_window = if handoff { Duration::from_secs(wait) } else { Duration::ZERO };
        let appear_deadline = Instant::now() + appear_window;
        let mut seen = is_running(wait_for);
        while !seen && Instant::now() < appear_deadline {
            std::thread::sleep(Duration::from_millis(500));
            seen = is_running(wait_for);
        }
        if seen {
            log(&format!("waiting for {wait_for:?} to exit"));
            let mut quiet_since: Option<Instant> = None;
            loop {
                if is_running(wait_for) {
                    quiet_since = None;
                } else if quiet_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(3) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
            log("watched processes exited");
        }
    }
    Ok(code)
}

/// Wait (up to `max`) while any file matching the given absolute glob
/// patterns is still held open by another process, e.g. a lock file the game
/// keeps open while writing saves. Files that merely exist do not count:
/// some games leave their lock files behind after exiting.
pub fn wait_quiesce(patterns: &[std::path::PathBuf], max: Duration) {
    if patterns.is_empty() {
        return;
    }
    let start = Instant::now();
    loop {
        let busy = patterns
            .iter()
            .flat_map(|p| matching_files(p))
            .any(|f| file_in_use(&f));
        if !busy || start.elapsed() > max {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn matching_files(pattern: &Path) -> Vec<std::path::PathBuf> {
    let s = pattern.to_string_lossy().replace('\\', "/");
    let Some(idx) = s.find(['*', '?', '[']) else {
        return if pattern.is_file() { vec![pattern.to_path_buf()] } else { Vec::new() };
    };
    let dir_end = s[..idx].rfind('/').unwrap_or(0);
    let dir = std::path::PathBuf::from(&s[..dir_end]);
    let Ok(glob) = globset::GlobBuilder::new(&s)
        .case_insensitive(true)
        .literal_separator(true)
        .build()
    else {
        return Vec::new();
    };
    let m = glob.compile_matcher();
    walkdir::WalkDir::new(&dir)
        .max_depth(4)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && m.is_match(e.path().to_string_lossy().replace('\\', "/")))
        .map(|e| e.into_path())
        .collect()
}

/// Whether another process currently has the file open.
pub fn file_in_use(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        // Opening with no sharing fails while any other handle is open.
        match std::fs::OpenOptions::new().read(true).share_mode(0).open(path) {
            Ok(_) => false,
            Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn detects_open_lock_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("slot.lock");
        std::fs::write(&lock, b"").unwrap();
        let pattern = tmp.path().join("*.lock");
        // A leftover lock file nobody holds open does not block.
        assert!(!file_in_use(&lock));
        let start = Instant::now();
        wait_quiesce(&[pattern.clone()], Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_secs(1));
        // While a process holds it open, it counts as busy.
        let held = std::fs::File::open(&lock).unwrap();
        assert!(file_in_use(&lock));
        drop(held);
        assert!(!file_in_use(&lock));
    }

    #[test]
    fn reports_game_exit_while_launcher_stays_open() {
        let tmp = tempfile::tempdir().unwrap();
        // A uniquely named "game": a copy of ping that runs ~1 s.
        let game = tmp.path().join("ln-fake-game.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &game).unwrap();
        // The "launcher" starts the game, then stays open ~6 s.
        let script = format!(
            "start /b {} -n 2 127.0.0.1 >nul & ping -n 7 127.0.0.1 >nul",
            game.display()
        );
        let calls = std::cell::Cell::new(0);
        let launcher_done = std::cell::Cell::new(false);
        let mut cb = || {
            assert!(!launcher_done.get());
            calls.set(calls.get() + 1);
        };
        let start = Instant::now();
        let code = launch_and_wait(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &["/c".to_string(), script],
            tmp.path(),
            &["ln-fake-game.exe".to_string()],
            Some(&mut cb),
            &|_| {},
        )
        .unwrap();
        launcher_done.set(true);
        assert_eq!(code, 0);
        assert_eq!(calls.get(), 1, "callback should run once, before the launcher exits");
        assert!(start.elapsed() >= Duration::from_secs(5));
        // The game was seen running, so no extra "wait for it to appear" delay.
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub struct Job(HANDLE);

    // The handle is only used from the thread that owns the Job.
    unsafe impl Send for Job {}

    impl Job {
        pub fn new() -> std::io::Result<Self> {
            let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if h.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Job(h))
        }

        pub fn assign(&self, child: &std::process::Child) -> std::io::Result<()> {
            let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn active_processes(&self) -> u32 {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                0
            } else {
                info.ActiveProcesses
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn process_names() -> Vec<String> {
        let mut out = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                    out.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        out
    }

    pub fn pid_alive(pid: u32) -> bool {
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            ok != 0 && code == STILL_ACTIVE
        }
    }
}
