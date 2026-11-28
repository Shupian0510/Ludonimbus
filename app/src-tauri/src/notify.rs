//! A system notification while the app sits in the tray
//! (docs/plan-positioning.md, section 6): a game that isn't brought up to
//! date by itself (it doesn't start through Steam, `overview::auto_pull`)
//! got newer saves on its own line in the cloud. One per game and cloud
//! version; clicking it opens the app on the game's page, where 「取回」
//! takes them. Turned off in Settings (`cloud_newer_notifications`).
//!
//! Nothing is asked of the cloud for this: it goes by the checks that
//! happen anyway (the library's checks after a game ends and every ten
//! minutes, `commands::saves::check_game`), within docs/plan-v2.md,
//! section 5.6. Checks close together (the library checks every game) are
//! told as one: a few games one by one, more as one notification that opens
//! the library.

use crate::commands::overview::auto_pull;
use chrono::{DateTime, Utc};
use ludonimbus_core::config::GameEntry;
use ludonimbus_core::sync::{Status, StatusReport};
use ludonimbus_core::{logger, paths, tr, util};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// A newer version of a game's line, to tell the user about.
#[derive(Clone)]
struct Newer {
    key: String,
    name: String,
    id: String,
    device: String,
    at: DateTime<Utc>,
}

/// What waits to be told, and whether a pass to tell it is on its way.
struct Queue {
    items: Vec<Newer>,
    flushing: bool,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { items: Vec::new(), flushing: false });
/// The versions told about (or seen in the open app), by game: kept on
/// this device so that a restart doesn't tell them again.
static SEEN: Mutex<()> = Mutex::new(());

/// How long checks are gathered before they are told.
const GATHER: Duration = Duration::from_secs(3);
/// More games than this at once are one notification.
const ONE_BY_ONE: usize = 3;

fn seen_file() -> PathBuf {
    paths::data_dir().join("notified.json")
}

fn seen() -> BTreeMap<String, String> {
    std::fs::read(seen_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Note the versions now known, game by game. Whether any is new.
fn note_seen(items: &[(String, String)]) -> Vec<bool> {
    let _guard = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = seen();
    let fresh: Vec<bool> = items.iter().map(|(k, id)| all.insert(k.clone(), id.clone()).as_ref() != Some(id)).collect();
    if fresh.iter().any(|f| *f) {
        if let Ok(bytes) = serde_json::to_vec(&all) {
            let _ = util::atomic_write(&seen_file(), &bytes);
        }
    }
    fresh
}

/// Whether the user is looking at the app: its window is shown and not
/// minimized. The game's status says it there.
fn looking(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// After a check of game `g`: tell the user when its own line has a newer
/// version that won't come by itself, once per version.
pub fn after_check(app: &AppHandle, enabled: bool, g: &GameEntry, report: &StatusReport) {
    if report.status != Status::CloudNewer || auto_pull(g) {
        return;
    }
    let Some(head) = &report.head else { return };
    let newer = Newer { key: g.key.clone(), name: g.name.clone(), id: head.id.clone(), device: head.device_name.clone(), at: head.created_at };
    if !enabled || looking(app) {
        // Shown in the app (or not wanted): not told again later.
        note_seen(&[(newer.key, newer.id)]);
        return;
    }
    if seen().get(&newer.key) == Some(&newer.id) {
        return;
    }
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    q.items.retain(|n| n.key != newer.key);
    q.items.push(newer);
    if q.flushing {
        return;
    }
    q.flushing = true;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(GATHER);
        let items = {
            let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
            q.flushing = false;
            std::mem::take(&mut q.items)
        };
        tell(&app, items);
    });
}

fn tell(app: &AppHandle, items: Vec<Newer>) {
    let fresh = note_seen(&items.iter().map(|n| (n.key.clone(), n.id.clone())).collect::<Vec<_>>());
    let items: Vec<Newer> = items.into_iter().zip(fresh).filter_map(|(n, f)| f.then_some(n)).collect();
    if items.is_empty() {
        return;
    }
    let shown = if items.len() <= ONE_BY_ONE {
        items
            .iter()
            .map(|n| {
                show(app, &tr!("《{}》云端有更新", "Newer saves of {} in the cloud", n.name), &format!("{} · {}", n.device, ago(n.at)), Some(n.key.clone()))
            })
            .collect()
    } else {
        let names: Vec<&str> = items.iter().take(ONE_BY_ONE).map(|n| n.name.as_str()).collect();
        vec![show(app, &tr!("{} 个游戏云端有更新", "{} games have newer saves in the cloud", items.len()), &format!("{}…", names.join("、")), None)]
    };
    for r in shown {
        if let Err(e) = r {
            logger::log("notify", format!("the notification wasn't shown: {e:#}"));
        }
    }
}

/// How long ago, in a few words.
fn ago(at: DateTime<Utc>) -> String {
    let mins = (Utc::now() - at).num_minutes().max(0);
    match mins {
        0 => tr!("刚刚", "just now"),
        1..=59 => tr!("{} 分钟前", "{} min ago", mins),
        60..=1439 => tr!("{} 小时前", "{} h ago", mins / 60),
        _ => tr!("{} 天前", "{} days ago", mins / 1440),
    }
}

/// Clicked: the app, on the game's page (or the library).
fn open(app: &AppHandle, key: Option<&str>) {
    crate::show_main(app);
    if let Some(key) = key {
        let _ = app.emit("open-game", key.to_string());
    }
}

/// A toast in the Action Center, under the app's name: the installer's
/// Start menu shortcut carries the app's id (Tauri's NSIS installer sets
/// it to the bundle identifier). A development build has no shortcut and
/// borrows PowerShell's, as Tauri's notification plugin does.
#[cfg(windows)]
fn show(app: &AppHandle, title: &str, body: &str, key: Option<String>) -> anyhow::Result<()> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    const POWERSHELL: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let xml = format!(
        r#"<toast launch="ludonimbus"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    let clicked = app.clone();
    toast.Activated(&TypedEventHandler::new(move |_, _| {
        open(&clicked, key.as_deref());
        Ok(())
    }))?;
    let id = if tauri::is_dev() { POWERSHELL.to_string() } else { app.config().identifier.clone() };
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&toast)?;
    Ok(())
}

/// Through `notify-send` (libnotify), which waits for a click on the
/// notification and says so. One that can't take an action shows the
/// notification without it.
#[cfg(target_os = "linux")]
fn show(app: &AppHandle, title: &str, body: &str, key: Option<String>) -> anyhow::Result<()> {
    use std::process::Command;
    let (app, title, body) = (app.clone(), title.to_string(), body.to_string());
    let action = format!("--action=default={}", tr!("打开", "Open"));
    std::thread::spawn(move || {
        let out = Command::new("notify-send").args(["--app-name=Ludonimbus", action.as_str(), "--wait", "--", title.as_str(), body.as_str()]).output();
        match out {
            Ok(o) if o.status.success() => {
                if String::from_utf8_lossy(&o.stdout).trim() == "default" {
                    open(&app, key.as_deref());
                }
            }
            _ => {
                let _ = Command::new("notify-send").args(["--app-name=Ludonimbus", "--", title.as_str(), body.as_str()]).status();
            }
        }
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn show(_app: &AppHandle, _title: &str, _body: &str, _key: Option<String>) -> anyhow::Result<()> {
    Ok(())
}
