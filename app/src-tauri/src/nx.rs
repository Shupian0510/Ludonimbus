//! Settings → Switch: the LAN connection for the Switch client (see
//! `core::nxlan`). Runs while the app does, when the user turned it on.

use crate::commands::{blocking, CmdResult};
use ludonimbus_core::{config, logger, nxlan};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use ts_rs::TS;

static SERVER: Mutex<Option<nxlan::Server>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn try_start() -> bool {
    let mut slot = SERVER.lock().unwrap();
    if slot.is_some() {
        return true;
    }
    match nxlan::start(|m| logger::log("nx", m)) {
        Ok(s) => {
            logger::log("nx", format!("listening on port {}", s.port));
            *slot = Some(s);
            *LAST_ERROR.lock().unwrap() = None;
            true
        }
        Err(e) => {
            *LAST_ERROR.lock().unwrap() = Some(format!("{e:#}"));
            false
        }
    }
}

/// Keep the server running while it's turned on (the port may still be
/// held a few seconds by the background service handing over).
pub fn spawn() {
    std::thread::spawn(|| loop {
        let on = config::load().map(|c| c.nx_lan).unwrap_or(false);
        if on {
            try_start();
        } else {
            SERVER.lock().unwrap().take();
        }
        std::thread::sleep(Duration::from_secs(10));
    });
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "NxDevice")]
pub struct Device {
    id: String,
    name: String,
    paired_at: chrono::DateTime<chrono::Utc>,
    last_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// Its user accounts, as it last reported them (empty until a Switch
    /// app of protocol 3 connects).
    users: Vec<User>,
}

/// A user account on a paired Switch and how its saves are kept.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "NxUser")]
pub struct User {
    uid: String,
    nickname: String,
    sync: bool,
    /// Kept apart from users of the same nickname on other Switches.
    separate: bool,
    /// The name of the save line its saves are on.
    line: String,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NxStatus {
    enabled: bool,
    running: bool,
    address: Option<String>,
    port: u16,
    fingerprint: Option<String>,
    devices: Vec<Device>,
    pairing_code: Option<String>,
    error: Option<String>,
}

fn status() -> anyhow::Result<NxStatus> {
    let cfg = config::load()?;
    let running = SERVER.lock().unwrap().as_ref().map(|s| s.fingerprint.clone());
    Ok(NxStatus {
        enabled: cfg.nx_lan,
        running: running.is_some(),
        address: nxlan::local_address(),
        port: nxlan::PORT,
        fingerprint: running.or_else(|| nxlan::identity().ok().map(|(_, _, fp)| fp)),
        devices: nxlan::devices()
            .into_iter()
            .map(|d| Device {
                users: d
                    .users
                    .iter()
                    .map(|u| User { uid: u.uid.clone(), nickname: u.nickname.clone(), sync: u.sync, separate: u.separate, line: nxlan::user_line(&d, u).1 })
                    .collect(),
                id: d.id,
                name: d.name,
                paired_at: d.paired_at,
                last_seen: d.last_seen,
            })
            .collect(),
        pairing_code: nxlan::pairing_code(),
        error: if cfg.nx_lan { LAST_ERROR.lock().unwrap().clone() } else { None },
    })
}

#[tauri::command]
pub async fn nx_status() -> CmdResult<NxStatus> {
    blocking(status).await
}

#[tauri::command]
pub async fn nx_set_enabled(enabled: bool) -> CmdResult<NxStatus> {
    blocking(move || {
        config::update(|c| {
            c.nx_lan = enabled;
            Ok(())
        })?;
        if enabled {
            try_start();
        } else {
            SERVER.lock().unwrap().take();
            nxlan::cancel_pairing();
        }
        status()
    })
    .await
}

#[tauri::command]
pub async fn nx_start_pairing() -> CmdResult<NxStatus> {
    blocking(|| {
        nxlan::start_pairing();
        status()
    })
    .await
}

#[tauri::command]
pub async fn nx_cancel_pairing() -> CmdResult<NxStatus> {
    blocking(|| {
        nxlan::cancel_pairing();
        status()
    })
    .await
}

#[tauri::command]
pub async fn nx_remove_device(id: String) -> CmdResult<NxStatus> {
    blocking(move || {
        nxlan::remove_device(&id)?;
        status()
    })
    .await
}

#[tauri::command]
pub async fn nx_rename_device(id: String, name: String) -> CmdResult<NxStatus> {
    blocking(move || {
        nxlan::rename_device(&id, &name)?;
        status()
    })
    .await
}

/// Whether a user of a paired Switch is backed up, and whether it keeps its
/// saves apart from users of the same nickname on other Switches.
#[tauri::command]
pub async fn nx_set_user(id: String, uid: String, sync: bool, separate: bool) -> CmdResult<NxStatus> {
    blocking(move || {
        nxlan::set_user(&id, &uid, sync, separate)?;
        status()
    })
    .await
}
