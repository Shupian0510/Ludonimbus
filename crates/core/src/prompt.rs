//! Questions the launch wrapper asks in a full-screen window on handhelds
//! (docs/plan-phase5.md §1.2): it writes a request file, starts
//! `ludonimbus-app --prompt <file>` and waits; the app shows the question,
//! writes the chosen button to `<file>.answer` and exits. System dialogs can
//! hide behind Steam Big Picture, need a mouse and may not show at all in
//! Deck Game Mode; a child window of the game's process does.

use crate::paths;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Request {
    pub title: String,
    pub text: String,
    /// "info", "warn" or "error".
    pub level: String,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Button {
    pub id: String,
    pub label: String,
    /// The controller button: "a", "b", "x" or "y". "b" also answers when
    /// the window is closed.
    pub key: String,
}

impl Request {
    pub fn new(title: impl Into<String>, text: impl Into<String>, level: &str) -> Self {
        Request { title: title.into(), text: text.into(), level: level.into(), buttons: Vec::new() }
    }

    pub fn button(mut self, id: &str, label: impl Into<String>, key: &str) -> Self {
        self.buttons.push(Button { id: id.into(), label: label.into(), key: key.into() });
        self
    }

    /// What closing the window means: the "b" button's answer.
    pub fn cancel_id(&self) -> Option<&str> {
        self.buttons.iter().find(|b| b.key == "b").map(|b| b.id.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Answer {
    pub choice: Option<String>,
}

/// The app next to this program (the wrapper is installed beside it).
pub fn app_exe() -> PathBuf {
    if let Ok(p) = std::env::var("LUDONIMBUS_APP") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let beside = paths::exe_dir().join(if cfg!(windows) { "ludonimbus-app.exe" } else { "ludonimbus-app" });
    // Linux: the wrapper runs from `<data>/bin`; the app noted where it is.
    if !beside.is_file() {
        if let Ok(p) = std::fs::read_to_string(paths::data_dir().join("bin").join("app-path")) {
            let p = PathBuf::from(p.trim());
            if p.is_file() {
                return p;
            }
        }
    }
    beside
}

pub fn answer_path(request: &Path) -> PathBuf {
    let mut s = request.as_os_str().to_owned();
    s.push(".answer");
    PathBuf::from(s)
}

pub fn read_request(path: &Path) -> Result<Request> {
    Ok(serde_json::from_slice(&std::fs::read(path).with_context(|| path.display().to_string())?)?)
}

/// Write the chosen button for a request (called by the app).
pub fn write_answer(request: &Path, choice: Option<&str>) -> Result<()> {
    crate::util::atomic_write(&answer_path(request), &serde_json::to_vec(&Answer { choice: choice.map(str::to_string) })?)
}

/// Ask in a full-screen window and wait for the answer. `Ok(None)`: closed
/// without choosing. `Err`: the window couldn't be shown (the caller falls
/// back to a system dialog).
pub fn ask_window(req: &Request) -> Result<Option<String>> {
    let app = app_exe();
    if !app.is_file() {
        return Err(anyhow!("{} not found", app.display()));
    }
    let file = paths::staging_dir().join(format!("prompt-{}.json", ulid::Ulid::new()));
    std::fs::write(&file, serde_json::to_vec(req)?)?;
    let answer = answer_path(&file);
    let status = std::process::Command::new(&app).arg("--prompt").arg(&file).status();
    let result = (|| {
        let status = status.with_context(|| app.display().to_string())?;
        match std::fs::read(&answer) {
            Ok(b) => Ok(serde_json::from_slice::<Answer>(&b)?.choice.or_else(|| req.cancel_id().map(str::to_string))),
            // Exited without writing an answer: it never showed (old version, crashed).
            Err(_) if !status.success() => Err(anyhow!("the prompt window exited with {status}")),
            Err(_) => Ok(req.cancel_id().map(str::to_string)),
        }
    })();
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&answer);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let req = Request::new("Ludonimbus", "Both changed", "warn").button("cloud", "Use cloud", "a").button("cancel", "Don't start", "b");
        assert_eq!(req.cancel_id(), Some("cancel"));
        let file = tmp.path().join("p.json");
        std::fs::write(&file, serde_json::to_vec(&req).unwrap()).unwrap();
        assert_eq!(read_request(&file).unwrap(), req);
        write_answer(&file, Some("cloud")).unwrap();
        let a: Answer = serde_json::from_slice(&std::fs::read(answer_path(&file)).unwrap()).unwrap();
        assert_eq!(a.choice.as_deref(), Some("cloud"));
    }
}
