//! Questions the launch wrapper asks in a full-screen window on handhelds
//! (docs/plan-phase5.md §1.2): it writes a request file, starts
//! `ludonimbus-app --prompt <file>` and waits; the app shows the question,
//! writes the chosen button to `<file>.answer` and exits. System dialogs can
//! hide behind Steam Big Picture, need a mouse and may not show at all in
//! Deck Game Mode; a child window of the game's process does.
//!
//! [`open_window`] shows one without waiting, for a question that only
//! matters until something else happens (a download that finishes first
//! closes it).

use crate::paths;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "PromptRequest")]
pub struct Request {
    pub title: String,
    pub text: String,
    /// "info", "warn" or "error".
    pub level: String,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(rename = "PromptButton")]
pub struct Button {
    pub id: String,
    pub label: String,
    /// The controller button: "a", "b", "x" or "y". "b" also answers when
    /// the window is closed.
    #[ts(type = r#""a" | "b" | "x" | "y""#)]
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
    open_window(req)?.wait()
}

/// Show a question in a full-screen window and go on: [`Window::poll`] for
/// the answer. Dropping the [`Window`] closes it. `Err`: the app isn't
/// there to show it.
pub fn open_window(req: &Request) -> Result<Window> {
    let app = app_exe();
    if !app.is_file() {
        return Err(anyhow!("{} not found", app.display()));
    }
    let file = paths::staging_dir().join(format!("prompt-{}.json", ulid::Ulid::new()));
    std::fs::write(&file, serde_json::to_vec(req)?)?;
    let child = std::process::Command::new(&app).arg("--prompt").arg(&file).spawn();
    let child = match child.with_context(|| app.display().to_string()) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&file);
            return Err(e);
        }
    };
    Ok(Window { child, file, cancel: req.cancel_id().map(str::to_string), done: false })
}

/// A question showing in its own window ([`open_window`]).
pub struct Window {
    child: std::process::Child,
    file: PathBuf,
    /// What closing the window answers.
    cancel: Option<String>,
    /// The window is gone and its answer was read.
    done: bool,
}

impl Window {
    /// `Ok(None)` while the window is open; `Ok(Some(choice))` once it is
    /// gone, `choice` being what [`ask_window`] returns. `Err`: it exited
    /// without ever showing (old version, crashed).
    pub fn poll(&mut self) -> Result<Option<Option<String>>> {
        match self.child.try_wait()? {
            None => Ok(None),
            Some(status) => self.answer(status).map(Some),
        }
    }

    /// Wait until the window is gone, and its answer.
    pub fn wait(mut self) -> Result<Option<String>> {
        let status = self.child.wait()?;
        self.answer(status)
    }

    fn answer(&mut self, status: std::process::ExitStatus) -> Result<Option<String>> {
        self.done = true;
        match std::fs::read(answer_path(&self.file)) {
            Ok(b) => Ok(serde_json::from_slice::<Answer>(&b)?.choice.or_else(|| self.cancel.clone())),
            Err(_) if !status.success() => Err(anyhow!("the prompt window exited with {status}")),
            Err(_) => Ok(self.cancel.clone()),
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_file(answer_path(&self.file));
        let _ = std::fs::remove_file(&self.file);
    }
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
