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
pub fn launch_and_wait(
    exe: &Path,
    args: &[String],
    work_dir: &Path,
    wait_for: &[String],
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

    let status = child.wait()?;
    let code = status.code().unwrap_or(0);
    log(&format!("main process exited with {code}"));

    #[cfg(windows)]
    if tracked {
        if let Some(j) = &job {
            while j.active_processes() > 0 {
                std::thread::sleep(Duration::from_millis(500));
            }
            log("job is empty");
        }
    }
    let _ = tracked;

    // Fallback: the real game may live outside the job (or appear late).
    if !wait_for.is_empty() {
        let appear_deadline = Instant::now() + Duration::from_secs(8);
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

/// Wait (up to `max`) until no file matches any of the given absolute glob
/// patterns, e.g. a lock file the game keeps while writing saves.
pub fn wait_quiesce(patterns: &[std::path::PathBuf], max: Duration) {
    if patterns.is_empty() {
        return;
    }
    let start = Instant::now();
    loop {
        let busy = patterns.iter().any(|p| any_match(p));
        if !busy || start.elapsed() > max {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn any_match(pattern: &Path) -> bool {
    let s = pattern.to_string_lossy().replace('\\', "/");
    let Some(idx) = s.find(['*', '?', '[']) else {
        return pattern.exists();
    };
    let dir_end = s[..idx].rfind('/').unwrap_or(0);
    let dir = std::path::PathBuf::from(&s[..dir_end]);
    let Ok(glob) = globset::GlobBuilder::new(&s)
        .case_insensitive(true)
        .literal_separator(true)
        .build()
    else {
        return false;
    };
    let m = glob.compile_matcher();
    walkdir::WalkDir::new(&dir)
        .max_depth(4)
        .into_iter()
        .flatten()
        .any(|e| m.is_match(e.path().to_string_lossy().replace('\\', "/")))
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
