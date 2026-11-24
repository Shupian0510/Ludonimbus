//! `ludoart://` protocol for artwork:
//! - `/<account>/<appid>/<kind>`: Steam's custom artwork in
//!   `userdata/<account>/config/grid`;
//! - `/local/<game>/<kind>`: artwork chosen in Ludonimbus for games that are
//!   not in Steam;
//! - `/shot/<game>/<device>/<t|f>/<name>`: a Steam screenshot (thumbnail or
//!   full image) from this PC's Steam folder or the screenshot cache.
//!
//! On Windows the webview reaches it as `http://ludoart.localhost/...`. URLs
//! carry the file's modification time so a replaced image is not cached.

use ludonimbus_core::{config, localart, screenshots, steam};
use std::path::Path;
use tauri::http::{Request, Response};
use tauri::{Runtime, UriSchemeContext};

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

pub fn handle<R: Runtime>(_ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = req.uri().path().trim_start_matches('/').to_string();
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
    Response::builder()
        .header("Content-Type", mime)
        .header("Cache-Control", "max-age=3600")
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .unwrap()
}
