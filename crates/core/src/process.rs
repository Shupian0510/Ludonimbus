//! Launching games and waiting for the whole process tree to exit.
//!
//! Launchers often start the real game and exit immediately, so the game is
//! started inside a Windows Job Object and we wait until the job has no
//! active processes. On Linux the wrapper becomes a "child subreaper":
//! processes whose parent exits are handed to it instead of init, so it can
//! wait until none of its descendants is left (Proton's wine processes
//! included). Process-name watching is the fallback for games that escape.

use anyhow::{Context, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Names (lowercase) of all running processes.
pub fn process_names() -> Vec<String> {
    #[cfg(windows)]
    {
        win::process_names()
    }
    #[cfg(unix)]
    {
        unix::process_names()
    }
    #[cfg(not(any(windows, unix)))]
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
    #[cfg(unix)]
    {
        unix::pid_alive(pid)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}

/// When the process `pid` started, as a number that only means something
/// compared with another one from here: a process id is reused once its
/// process is gone, its start time tells the two apart. `None` when it
/// can't be told.
pub fn started(pid: u32) -> Option<u64> {
    #[cfg(windows)]
    {
        win::started(pid)
    }
    #[cfg(target_os = "linux")]
    {
        unix::started(pid)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

/// Linux: let the wrapper finish its work when Steam ends the game (its
/// "Exit game" sends SIGTERM to the whole process tree): the signal is
/// noted, the game exits, and the saves are still uploaded. Where stopping
/// is safe, the wrapper asks [`termination_requested`]. The game's own
/// handling of the signal is its own: a caught signal is back to the
/// default in a program that is started, and nothing is blocked.
pub fn defer_termination() {
    #[cfg(unix)]
    unix::defer_sigterm();
}

/// Whether this process was asked to stop since [`defer_termination`]
/// (Linux: SIGTERM). Always `false` where there is no such signal.
pub fn termination_requested() -> bool {
    #[cfg(unix)]
    {
        unix::terminated()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Start the game and block until it (and everything it spawned) has exited.
/// Returns the exit code of the started process. `envs` are set for the
/// game on top of this process's environment.
///
/// With `on_game_exit`, the `wait_for` processes (the real game) are watched
/// while the launcher is still open, and the callback runs every time the
/// game exits, so saves can be uploaded without waiting for the launcher.
pub fn launch_and_wait(
    exe: &Path,
    args: &[String],
    envs: &[(String, String)],
    work_dir: &Path,
    wait_for: &[String],
    mut on_game_exit: Option<&mut dyn FnMut()>,
    log: &dyn Fn(&str),
) -> Result<i32> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).envs(envs.iter().map(|(k, v)| (k, v))).current_dir(work_dir);

    #[cfg(windows)]
    let job = win::Job::new().ok();
    #[cfg(target_os = "linux")]
    let subreaper = unix::become_subreaper();

    let mut child = cmd
        .spawn()
        .with_context(|| crate::tr!("无法启动 {}", "Couldn't start {}", exe.display()))?;
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
    #[cfg(target_os = "linux")]
    let tracked = subreaper;
    #[cfg(not(any(windows, target_os = "linux")))]
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
        #[cfg(target_os = "linux")]
        let active = if tracked {
            unix::reap_orphans(child.id());
            exit.is_none() || unix::live_descendants(std::process::id(), child.id()) > 0
        } else {
            exit.is_none()
        };
        #[cfg(not(any(windows, target_os = "linux")))]
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
        log("all started processes have exited");
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
            &[],
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

    #[test]
    fn the_game_gets_the_variables_given_for_it() {
        let tmp = tempfile::tempdir().unwrap();
        let env = [("LUDONIMBUS_TEST_CODE".to_string(), "7".to_string())];
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        let code = launch_and_wait(cmd, &["/c".to_string(), "exit %LUDONIMBUS_TEST_CODE%".to_string()], &env, tmp.path(), &[], None, &|_| {}).unwrap();
        assert_eq!(code, 7);
    }

    #[test]
    fn a_process_it_may_not_look_at_is_still_running() {
        assert!(pid_alive(std::process::id()));
        // "System" (pid 4) is always there, and a process that isn't an
        // administrator's may not open it.
        assert!(pid_alive(4));
        assert!(!pid_alive(u32::MAX - 3));
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
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
                // Denied: the process is there, it just isn't ours to look at
                // (another user's, or one guarded by an anti-cheat). A process
                // that is gone gives "invalid parameter".
                return GetLastError() == ERROR_ACCESS_DENIED;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            ok != 0 && code == STILL_ACTIVE
        }
    }

    /// The creation time of the process, in 100 ns units.
    pub fn started(pid: u32) -> Option<u64> {
        use windows_sys::Win32::Foundation::FILETIME;
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let zero = || FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut created, mut exited, mut kernel, mut user) = (zero(), zero(), zero(), zero());
            let ok = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user);
            CloseHandle(h);
            (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
    }
}

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;

    /// `(pid, parent pid, state)` of every process, from `/proc/<pid>/stat`.
    fn table() -> Vec<(u32, u32, char)> {
        let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
        rd.flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                // `pid (name) S ppid …`; the name may hold spaces and parentheses.
                let rest = &stat[stat.rfind(')')? + 2..];
                let mut it = rest.split_whitespace();
                let state = it.next()?.chars().next()?;
                let ppid = it.next()?.parse().ok()?;
                Some((pid, ppid, state))
            })
            .collect()
    }

    /// Names of the running processes, lowercase: the short name, the full
    /// program name, and for Windows games under Proton / Wine the `.exe`
    /// on their command line (that's what the game's rules list).
    pub fn process_names() -> Vec<String> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        for e in rd.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
                out.push(comm.trim().to_lowercase());
            }
            let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
            for (i, arg) in cmd.split(|&b| b == 0).filter(|a| !a.is_empty()).enumerate() {
                let arg = String::from_utf8_lossy(arg);
                let name = arg.rsplit(['/', '\\']).next().unwrap_or(&arg).to_lowercase();
                if i == 0 || name.ends_with(".exe") {
                    out.push(name);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn pid_alive(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat.rfind(')').and_then(|i| stat[i + 2..].chars().next()).is_some_and(|s| s != 'Z'),
            Err(_) => false,
        }
    }

    /// When the process started, in clock ticks since boot (`starttime`,
    /// the 22nd field of `/proc/<pid>/stat`).
    #[cfg(target_os = "linux")]
    pub fn started(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // After `pid (name) ` come the fields from the 3rd on.
        stat[stat.rfind(')')? + 2..].split_whitespace().nth(19)?.parse().ok()
    }

    /// Become the reaper of our orphaned descendants (Linux 3.4+).
    #[cfg(target_os = "linux")]
    pub fn become_subreaper() -> bool {
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1 as libc::c_ulong, 0, 0, 0) == 0 }
    }

    /// Live (not zombie) processes below `root`, other than ones we don't
    /// wait for. `main` is the started process (std waits for it itself).
    pub fn live_descendants(root: u32, main: u32) -> usize {
        let t = table();
        let parent: HashMap<u32, (u32, char)> = t.iter().map(|&(p, pp, s)| (p, (pp, s))).collect();
        t.iter()
            .filter(|&&(pid, _, state)| state != 'Z' && pid != root)
            .filter(|&&(pid, _, _)| {
                let mut cur = pid;
                for _ in 0..64 {
                    match parent.get(&cur) {
                        Some(&(pp, _)) if pp == root => return true,
                        Some(&(pp, _)) if pp > 1 => cur = pp,
                        _ => return false,
                    }
                }
                false
            })
            .filter(|&&(pid, _, _)| pid != main || pid_alive(main))
            .count()
    }

    /// Collect the exit status of orphans handed to us (not the started
    /// process: `Child` collects that one).
    pub fn reap_orphans(main: u32) {
        let me = std::process::id();
        for (pid, ppid, state) in table() {
            if ppid == me && state == 'Z' && pid != main {
                let mut status = 0;
                unsafe {
                    libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG);
                }
            }
        }
    }

    static TERM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    extern "C" fn on_term(_: libc::c_int) {
        TERM.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// A handler, not "ignore": an ignored signal would stay ignored in the
    /// game started from here, a handled one is back to the default there.
    pub fn defer_sigterm() {
        unsafe {
            libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        }
    }

    pub fn terminated() -> bool {
        TERM.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sees_itself() {
            assert!(pid_alive(std::process::id()));
            assert!(!pid_alive(u32::MAX - 1));
            let names = process_names();
            assert!(!names.is_empty());
        }

        #[test]
        fn sigterm_is_noted_and_the_process_goes_on() {
            defer_sigterm();
            unsafe {
                libc::raise(libc::SIGTERM);
            }
            assert!(terminated());
        }
    }
}
