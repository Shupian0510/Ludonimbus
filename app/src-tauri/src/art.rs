//! `ludoart://` protocol: serves the custom artwork Steam keeps in
//! `userdata/<account>/config/grid` (cover, hero, logo, icon).
//! On Windows the webview reaches it as `http://ludoart.localhost/<account>/<appid>/<kind>`.

use ludonimbus_core::{config, steam};
use tauri::http::{Request, Response};
use tauri::{Runtime, UriSchemeContext};

pub fn url(account: &str, appid: u32, kind: &str) -> String {
    format!("http://ludoart.localhost/{account}/{appid}/{kind}")
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder().status(404).body(Vec::new()).unwrap()
}

pub fn handle<R: Runtime>(_ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = req.uri().path().trim_start_matches('/').to_string();
    let parts: Vec<&str> = path.split('/').collect();
    let [account, appid, kind] = parts.as_slice() else {
        return not_found();
    };
    if !account.chars().all(|c| c.is_ascii_digit()) {
        return not_found();
    }
    let Ok(appid) = appid.parse::<u32>() else {
        return not_found();
    };
    let Ok(cfg) = config::load() else {
        return not_found();
    };
    let Some(root) = steam::root(&cfg) else {
        return not_found();
    };
    let Some(file) = steam::art_file(&root, account, appid, kind) else {
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
        .header("Cache-Control", "max-age=60")
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .unwrap()
}
