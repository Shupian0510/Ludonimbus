//! Games installed by the Xbox app (PC Game Pass).
//!
//! Their own executables are encrypted: other programs cannot read or start
//! them. Steam and Ludonimbus start such a game through `gamelaunchhelper.exe`
//! next to its `MicrosoftGame.config`, and tell that the game is running by
//! the names of its real processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const LAUNCH_HELPER: &str = "gamelaunchhelper.exe";
const CONFIG: &str = "MicrosoftGame.config";

/// The package folder (the one holding `MicrosoftGame.config`, usually
/// `<root>\<game>\Content`) that `path` is in, is, or is just above.
pub fn content_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() && path.join("Content").join(CONFIG).is_file() {
        return Some(path.join("Content"));
    }
    path.ancestors().take(5).find(|d| d.join(CONFIG).is_file()).map(Path::to_path_buf)
}

pub fn is_launch_helper(exe: &Path) -> bool {
    exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case(LAUNCH_HELPER))
}

/// The program that starts the Xbox game `path` belongs to.
pub fn launch_helper(path: &Path) -> Option<PathBuf> {
    let helper = content_dir(path)?.join(LAUNCH_HELPER);
    helper.is_file().then_some(helper)
}

/// What to start instead of `exe`: the launch helper for an Xbox game's own
/// (protected) program, `exe` itself otherwise.
pub fn launchable(exe: PathBuf) -> PathBuf {
    if is_launch_helper(&exe) {
        return exe;
    }
    launch_helper(&exe).unwrap_or(exe)
}

/// Process names of the game itself: the executables `MicrosoftGame.config`
/// lists and the programs next to it (not the launch helper).
pub fn game_processes(content: &Path) -> Vec<String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(content).cloned()) {
        return v;
    }
    let v = read_game_processes(content);
    if let Ok(mut c) = cache.lock() {
        c.insert(content.to_path_buf(), v.clone());
    }
    v
}

fn read_game_processes(content: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut add = |n: String| {
        if !n.is_empty() && !n.eq_ignore_ascii_case(LAUNCH_HELPER) && !names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
            names.push(n);
        }
    };
    if let Ok(text) = std::fs::read_to_string(content.join(CONFIG)) {
        for n in xml_attrs(&text, "Executable", "Name") {
            add(n.rsplit(['/', '\\']).next().unwrap_or("").to_string());
        }
    }
    if let Ok(rd) = std::fs::read_dir(content) {
        let mut exes: Vec<String> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
            .filter(|p| !crate::discover::is_junk_exe(p))
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        exes.sort();
        exes.into_iter().for_each(add);
    }
    names
}

/// The value of `attr` in the first `<tag …>` element.
pub fn xml_attr(text: &str, tag: &str, attr: &str) -> Option<String> {
    xml_attrs(text, tag, attr).into_iter().next()
}

/// The value of `attr` in every `<tag …>` element that has it.
pub fn xml_attrs(text: &str, tag: &str, attr: &str) -> Vec<String> {
    let open = format!("<{tag} ");
    let pat = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let Some(end) = rest.find('>') else { break };
        let el = &rest[..end];
        // Match the attribute name exactly (not "DisplayName" for "Name").
        let mut at = 0;
        while let Some(i) = el[at..].find(&pat) {
            let i = at + i;
            if i == 0 || el.as_bytes()[i - 1].is_ascii_whitespace() {
                let v = &el[i + pat.len()..];
                if let Some(q) = v.find('"') {
                    out.push(unescape(&v[..q]));
                }
                break;
            }
            at = i + pat.len();
        }
        rest = &rest[end..];
    }
    out
}

fn unescape(v: &str) -> String {
    v.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_xml_attributes() {
        let t = r#"<Game><ExecutableList><Executable Name="launcher/a.exe" Id="Game" /><Executable Id="Tool" Name="b&amp;c.exe" /></ExecutableList><ShellVisuals DefaultDisplayName="X - PC" /></Game>"#;
        assert_eq!(xml_attrs(t, "Executable", "Name"), vec!["launcher/a.exe", "b&c.exe"]);
        assert_eq!(xml_attr(t, "ShellVisuals", "DefaultDisplayName").as_deref(), Some("X - PC"));
        // "Name" is not "DefaultDisplayName".
        assert_eq!(xml_attr(t, "ShellVisuals", "Name"), None);
    }

    #[test]
    fn starts_xbox_games_through_the_launch_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let content = tmp.path().join("Doom Eternal - PC").join("Content");
        std::fs::create_dir_all(content.join("launcher")).unwrap();
        for f in ["gamelaunchhelper.exe", "DOOMEternalx64vk.exe", "CrashReporter.exe", r"launcher\idTechLauncher.exe"] {
            std::fs::write(content.join(f), b"x").unwrap();
        }
        std::fs::write(
            content.join(CONFIG),
            r#"<Game><ExecutableList><Executable Name="launcher/idTechLauncher.exe" Id="Game" /></ExecutableList></Game>"#,
        )
        .unwrap();
        let helper = content.join("gamelaunchhelper.exe");
        let real = content.join("launcher").join("idTechLauncher.exe");
        assert_eq!(content_dir(&real), Some(content.clone()));
        assert_eq!(content_dir(content.parent().unwrap()), Some(content.clone()));
        assert_eq!(launchable(real.clone()), helper);
        assert_eq!(launchable(helper.clone()), helper);
        assert_eq!(game_processes(&content), vec!["idTechLauncher.exe", "DOOMEternalx64vk.exe"]);
        // The runner and the app wait for those, not the helper.
        let g = crate::config::GameEntry { exe: Some(helper.clone()), ..Default::default() };
        assert_eq!(crate::spec::effective(&g).wait_for, vec!["idTechLauncher.exe", "DOOMEternalx64vk.exe"]);

        // Not an Xbox game: unchanged.
        let other = tmp.path().join("Other").join("game.exe");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, b"x").unwrap();
        assert_eq!(launchable(other.clone()), other);
    }
}
