use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Write a file atomically: write a temporary file beside it, flush it to
/// the disk, then rename it over the target. Readers see the old content or
/// the new, never part of it, and a power cut doesn't leave an empty file.
/// Each write has a temporary file of its own, so writers that don't lock
/// (two processes, or an older version during an update) don't write into
/// each other's.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    write_replacing(path, bytes, false)
}

/// [`atomic_write`] for a file with secrets in it: on Linux and macOS only
/// this user can read it (mode 0600) from the moment it is created, the
/// temporary file included. Windows keeps a user's app data private already.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    write_replacing(path, bytes, true)
}

fn write_replacing(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| crate::tr!("无法创建目录 {}", "Can't create the folder {}", parent.display()))?;
    let (tmp, mut file) = create_temp(path, private).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", path.display()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow::Error::new(e).context(crate::tr!("无法写入 {}", "Can't write {}", tmp.display())));
    }
    if let Err(e) = rename_retrying(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow::Error::new(e).context(crate::tr!("无法替换 {}", "Can't replace {}", path.display())));
    }
    // The new name is in the folder's entry only once the folder is flushed
    // too (not needed, nor possible, on Windows).
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// A new temporary file beside `path`: `<name>.<process>-<n>.lntmp`, ending
/// in `.lntmp` like the one older versions use (`<name>.lntmp`), which
/// readers of folders already skip.
fn create_temp(path: &Path, private: bool) -> std::io::Result<(PathBuf, std::fs::File)> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    loop {
        let tmp = path.with_file_name(format!("{name}.{:x}-{:x}.lntmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        match opts.open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            // Left by a process that had the same id and was cut short.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Rename `from` over `to`. On Windows a program that has `to` open (a
/// virus scanner, the search indexer, another process reading it) makes the
/// rename fail for a moment; it is tried again for up to a second.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            // Access denied, sharing violation, lock violation.
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33)) && tries < 20 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            res => return res,
        }
    }
}

/// Path equality the way this system's file names compare, ignoring slash
/// style and surrounding quotes: any case on Windows and macOS, exact on Linux.
pub fn same_path(a: &str, b: &str) -> bool {
    norm_path(a) == norm_path(b)
}

/// A key for comparing paths (see [`same_path`]).
pub fn norm_path(p: &str) -> String {
    let p = native_sep(p.trim().trim_matches('"'));
    let trimmed = p.trim_end_matches(std::path::MAIN_SEPARATOR);
    // Keep the root folder `/` itself.
    let p = if trimmed.is_empty() { &p[..p.len().min(1)] } else { trimmed };
    if cfg!(any(windows, target_os = "macos")) {
        p.to_lowercase()
    } else {
        p.to_string()
    }
}

/// Tests: a Windows path (`D:\Emu\x`) as is on Windows, or as an absolute
/// path on Linux and macOS (`/d/Emu/x`).
#[cfg(test)]
pub fn test_path(win: &str) -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(win);
    }
    match win.split_once(":\\") {
        Some((drive, rest)) => PathBuf::from(format!("/{}/{}", drive.to_lowercase(), rest.replace('\\', "/"))),
        None => PathBuf::from(win.replace('\\', "/")),
    }
}

/// A path written with either slash, in this system's separators. Windows
/// takes both; elsewhere a `\` from a Windows-style path (a launcher's
/// manifest, an emulator's settings) is read as a separator too.
pub fn native_sep(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\")
    } else {
        p.replace('\\', "/")
    }
}

/// Remove one level of surrounding double quotes.
pub fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

/// A path in double quotes, the way Steam writes the `Exe`, `StartDir` and
/// `icon` of a shortcut (Steam takes the quotes off itself; a `StartDir`
/// ending in `\` is how Steam writes it too), and the wrapper in front of
/// launch options. Not for arguments in general: in a command line a `\`
/// before the closing quote escapes it, and a shell reads `$`, `` ` `` and
/// `\` inside double quotes.
pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.trim_matches('"'))
}

/// File name of a path, lowercased.
pub fn file_name_lower(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Split launch options into arguments the way the program gets them when
/// Steam starts it: on Windows by the rules of the C runtime the program
/// parses its command line with (`windows_args`), elsewhere by the shell
/// Steam runs launch options with (`shell_words`).
pub fn split_args(s: &str) -> Vec<String> {
    if cfg!(windows) {
        windows_args(s)
    } else {
        shell_words(s)
    }
}

/// Collects the characters of one argument after the other.
#[derive(Default)]
struct Words {
    out: Vec<String>,
    cur: String,
    /// The current argument has begun (`""` is an empty one).
    begun: bool,
}

impl Words {
    fn push(&mut self, c: char) {
        self.cur.push(c);
        self.begun = true;
    }

    fn end(&mut self) {
        if self.begun {
            self.out.push(std::mem::take(&mut self.cur));
            self.begun = false;
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.end();
        self.out
    }
}

/// A command line split by the rules of the Microsoft C runtime (since
/// 2008), which Rust's `Command` quotes arguments for: spaces and tabs
/// outside quotes separate; backslashes are taken as they are unless a
/// quote follows them, then they are halved and an odd one out makes the
/// quote a plain character (so `"D:\Games\\"` is `D:\Games\`); inside
/// quotes `""` is a plain quote. A line break separates too, as in a
/// pasted list of arguments.
fn windows_args(s: &str) -> Vec<String> {
    let mut w = Words::default();
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut n = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    n += 1;
                }
                let before_quote = chars.peek() == Some(&'"');
                for _ in 0..if before_quote { n / 2 } else { n } {
                    w.push('\\');
                }
                if before_quote && n % 2 == 1 {
                    chars.next();
                    w.push('"');
                }
                w.begun = true;
            }
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                chars.next();
                w.push('"');
            }
            '"' => {
                in_quotes = !in_quotes;
                w.begun = true;
            }
            ' ' | '\t' | '\r' | '\n' if !in_quotes => w.end(),
            c => w.push(c),
        }
    }
    w.finish()
}

/// A command line split into words the way a POSIX shell does it, without
/// expanding anything: whitespace outside quotes separates; inside `'…'`
/// every character is plain; inside `"…"` a backslash escapes only a
/// backslash, a double quote, `$` and a backtick; outside quotes it escapes
/// any character. A quote left open runs to the end.
fn shell_words(s: &str) -> Vec<String> {
    let mut w = Words::default();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                w.begun = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    w.push(c);
                }
            }
            '"' => {
                w.begun = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('\\' | '"' | '$' | '`')) => w.push(chars.next().unwrap_or('\\')),
                        // A line continued.
                        '\\' if chars.peek() == Some(&'\n') => {
                            chars.next();
                        }
                        c => w.push(c),
                    }
                }
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(c) => w.push(c),
                None => w.push('\\'),
            },
            ' ' | '\t' | '\r' | '\n' => w.end(),
            c => w.push(c),
        }
    }
    w.finish()
}

/// Pick the first existing file among candidates.
pub fn first_existing(cands: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    cands.into_iter().find(|p| p.is_file())
}

/// Find a file in `dir` whose name matches `name` case-insensitively.
pub fn find_file_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        if p.is_file() && e.file_name().to_string_lossy().to_lowercase() == want {
            Some(p)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_args() {
        assert_eq!(split_args(r#"--a "b c" d"#), vec!["--a", "b c", "d"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args(r#""""#), vec![""]);
        assert_eq!(split_args(r#"-e "/roms/My Game.iso" --batch"#), vec!["-e", "/roms/My Game.iso", "--batch"]);
    }

    #[test]
    fn splits_windows_command_lines() {
        let cases: &[(&str, &[&str])] = &[
            (r#"--a "b c" d"#, &["--a", "b c", "d"]),
            ("  a\t b  ", &["a", "b"]),
            (r#""""#, &[""]),
            (r#"a "" b"#, &["a", "", "b"]),
            // Backslashes are plain unless a quote follows them.
            (r"D:\Games\x.exe C:\a\\b", &[r"D:\Games\x.exe", r"C:\a\\b"]),
            (r"-path D:\Games\", &["-path", r"D:\Games\"]),
            // A folder ending in `\`, quoted the way the C runtime reads it.
            (r#"-path "D:\My Games\\" -x"#, &["-path", r"D:\My Games\", "-x"]),
            (r#""D:\Games\\\\" -x"#, &[r"D:\Games\\", "-x"]),
            // Written without the second `\`, the quote is escaped: the
            // program itself reads it so.
            (r#"-path "D:\Games\" -x"#, &["-path", r#"D:\Games" -x"#]),
            // Escaped quotes, and `""` inside quotes.
            (r#"a\"b "c\"d" e\\\"f"#, &[r#"a"b"#, r#"c"d"#, r#"e\"f"#]),
            (r#""a""b" c"#, &[r#"a"b"#, "c"]),
            (r#"--exec="D:\x y.iso" --batch"#, &[r"--exec=D:\x y.iso", "--batch"]),
            // Only spaces and tabs separate, not other blanks in a name.
            ("D:\\游戏\u{3000}合集\\x.iso -f", &["D:\\游戏\u{3000}合集\\x.iso", "-f"]),
            // Shell characters are plain.
            ("a&b $HOME 'c d'", &["a&b", "$HOME", "'c", "d'"]),
        ];
        for (line, want) in cases {
            assert_eq!(windows_args(line), *want, "{line}");
        }
    }

    #[test]
    fn splits_shell_words() {
        let cases: &[(&str, &[&str])] = &[
            (r#"--a "b c" d"#, &["--a", "b c", "d"]),
            ("  a\t b\n c ", &["a", "b", "c"]),
            ("''", &[""]),
            (r#"a "" b"#, &["a", "", "b"]),
            ("--name 'My Game' -x", &["--name", "My Game", "-x"]),
            (r#"'it'\''s' a\ b"#, &["it's", "a b"]),
            (r#""a\"b" "c\\d" "e\f" 'g\h'"#, &[r#"a"b"#, r"c\d", r"e\f", r"g\h"]),
            (r#""\$HOME" $HOME"#, &["$HOME", "$HOME"]),
            (r#"--exec="/x y.iso" --batch"#, &["--exec=/x y.iso", "--batch"]),
            // A Windows path for a game in Proton, quoted, as it reaches
            // the game through Steam's shell.
            (r#"-config "C:\My Games\a.cfg""#, &["-config", r"C:\My Games\a.cfg"]),
            (r#""/opt/ludonimbus-run" --game k -- %command% -x"#, &["/opt/ludonimbus-run", "--game", "k", "--", "%command%", "-x"]),
            ("a\\", &["a\\"]),
            ("a\\\nb", &["ab"]),
            ("\"open to the end", &["open to the end"]),
        ];
        for (line, want) in cases {
            assert_eq!(shell_words(line), *want, "{line}");
        }
    }

    #[test]
    fn writes_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub").join("config.json");
        atomic_write(&path, b"one").unwrap();
        atomic_write(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // A temporary file an older version left behind is no hindrance.
        std::fs::write(tmp.path().join("sub").join("config.json.lntmp"), b"old").unwrap();
        atomic_write(&path, b"three").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"three");
        // No temporary file of its own is left.
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        // Into a folder that can't be written: an error, and nothing left.
        let not_a_folder = tmp.path().join("file");
        std::fs::write(&not_a_folder, b"").unwrap();
        assert!(atomic_write(&not_a_folder.join("x.json"), b"x").is_err());
    }

    #[test]
    fn writers_at_once_dont_mix() {
        let tmp = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(tmp.path().join("state.json"));
        let threads: Vec<_> = (0..8u8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let body = vec![b'a' + i; 64 * 1024];
                    for _ in 0..20 {
                        atomic_write(&path, &body).unwrap();
                        let seen = std::fs::read(&*path).unwrap();
                        assert_eq!(seen.len(), body.len());
                        assert!(seen.iter().all(|&b| b == seen[0]), "two writes mixed");
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let left = std::fs::read_dir(tmp.path()).unwrap().count();
        assert_eq!(left, 1);
    }

    #[test]
    fn writes_private_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("credentials.json");
        write_private(&path, b"{}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            // Also when it replaces a file others could read.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            write_private(&path, b"{\"a\":1}").unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn compares_paths() {
        assert!(!same_path(r"C:\a", r"C:\b"));
        assert!(same_path(r#""C:\Games\x.exe""#, "C:/Games/x.exe/"));
        // File names: any case on Windows and macOS, exact on Linux.
        assert_eq!(same_path(r#""C:\Games\x.exe""#, "c:/games/X.EXE"), cfg!(any(windows, target_os = "macos")));
        assert!(same_path("/", "/"));
        assert!(!same_path("/", "/home"));
    }
}
