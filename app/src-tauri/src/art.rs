//! `ludoart://` protocol for artwork:
//! - `/<account>/<appid>/<kind>`: Steam's custom artwork in
//!   `userdata/<account>/config/grid`;
//! - `/local/<game>/<kind>`: artwork chosen in Ludonimbus for games that are
//!   not in Steam;
//! - `/shot/<game>/<device>/<t|f>/<name>`: a Steam screenshot (thumbnail or
//!   full image) from this PC's Steam folder or the screenshot cache;
//! - `/logo/<hash>.png`: a program's icon, the logo of a library group (see
//!   `ludonimbus_core::logos`).
//!
//! On Windows the webview reaches it as `http://ludoart.localhost/...`. URLs
//! carry the file's modification time so a replaced image is not cached.
//!
//! Only the app's own windows get the images: every webview has the
//! protocol, the Quark sign-in window (a web page) too. Files are read off
//! the webview's thread.

use ludonimbus_core::{config, localart, logos, screenshots, steam};
use std::path::Path;
use tauri::http::{Request, Response};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

/// The windows that show the app itself (`main`, also in handheld mode, and
/// the achievement popup).
const APP_WINDOWS: [&str; 2] = ["main", crate::achievements::TOAST_LABEL];

fn version(file: &Path) -> u64 {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn url(account: &str, appid: u32, kind: &str, file: &Path) -> String {
    format!("http://ludoart.localhost/{account}/{appid}/{kind}?v={}", version(file))
}

pub fn local_url(key: &str, kind: &str, file: &Path) -> String {
    format!("http://ludoart.localhost/local/{key}/{kind}?v={}", version(file))
}

/// Logo files are named by their content, so they never change.
pub fn logo_url(file: &Path) -> Option<String> {
    let name = file.file_name()?.to_str()?;
    Some(format!("http://ludoart.localhost/logo/{name}"))
}

pub fn shot_url(key: &str, device: &str, name: &str, thumb: bool, file: &Path) -> String {
    let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
    format!(
        "http://ludoart.localhost/shot/{}/{}/{}/{}?v={}",
        enc(key),
        enc(device),
        if thumb { "t" } else { "f" },
        enc(name),
        version(file)
    )
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder().status(404).body(Vec::new()).unwrap()
}

fn resolve(parts: &[&str]) -> Option<std::path::PathBuf> {
    match parts {
        ["shot", key, device, size, name] => {
            let dec = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8().ok().map(|c| c.to_string());
            let (key, device, name) = (dec(key)?, dec(device)?, dec(name)?);
            let cfg = config::load().ok()?;
            let game = cfg.game(&key)?;
            screenshots::local_file(&cfg, game, &device, &name, *size == "t")
        }
        ["logo", name] => logos::file(name),
        ["local", key, kind] => {
            let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            valid.then(|| localart::file(key, kind)).flatten()
        }
        [account, appid, kind] => {
            if !account.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let appid = appid.parse::<u32>().ok()?;
            let cfg = config::load().ok()?;
            let root = steam::root(&cfg)?;
            steam::art_file(&root, account, appid, kind)
        }
        _ => None,
    }
}

fn respond(path: &str) -> Response<Vec<u8>> {
    let parts: Vec<&str> = path.split('/').collect();
    let Some(file) = resolve(&parts) else {
        return not_found();
    };
    let Ok(bytes) = std::fs::read(&file) else {
        return not_found();
    };
    let mime = match file.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    // No CORS header: the app shows these with <img> and CSS, which don't
    // need one, and no other page may read them.
    Response::builder()
        .header("Content-Type", mime)
        .header("Cache-Control", "max-age=3600")
        .body(bytes)
        .unwrap()
}

/// Registered with `register_asynchronous_uri_scheme_protocol`: reading the
/// configuration and the image would otherwise hold up the window.
pub fn handle<R: Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    if !APP_WINDOWS.contains(&ctx.webview_label()) {
        responder.respond(Response::builder().status(403).body(Vec::new()).unwrap());
        return;
    }
    let path = req.uri().path().trim_start_matches('/').to_string();
    tauri::async_runtime::spawn_blocking(move || responder.respond(respond(&path)));
}
