//! Quark sign-in: the official Quark web page in a private window. Once the
//! user is signed in (QR code from the Quark app, or any method the page
//! offers), the cookies of that window are handed to OpenList and the window
//! closes. The private window keeps nothing after it closes, and Ludonimbus
//! doesn't store the cookie itself (OpenList keeps it in its data folder).

use anyhow::{anyhow, bail, Result};
use ludonimbus_core::tr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "quark-login";
const PAGE: &str = "https://pan.quark.cn/";

/// The cookie header of a signed-in Quark page. Errors when the window is
/// closed, the sign-in is cancelled or takes longer than 10 minutes.
pub async fn sign_in(app: &AppHandle, cancel: &AtomicBool) -> Result<String> {
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.destroy();
    }
    let url: tauri::Url = PAGE.parse()?;
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url.clone()))
        .title(tr!("登录夸克网盘 · Ludonimbus", "Sign in to Quark · Ludonimbus"))
        .inner_size(1100.0, 760.0)
        .incognito(true)
        .build()?;
    let started = Instant::now();
    let result = loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if cancel.load(Ordering::SeqCst) {
            break Err(anyhow!(tr!("已取消夸克网盘登录", "Quark sign-in cancelled")));
        }
        if app.get_webview_window(LABEL).is_none() {
            break Err(anyhow!(tr!("登录窗口已关闭，已取消夸克网盘登录", "The sign-in window was closed; Quark sign-in cancelled")));
        }
        if started.elapsed() > Duration::from_secs(600) {
            break Err(anyhow!(tr!("等待夸克网盘登录超时，请重新连接", "Timed out waiting for the Quark sign-in. Please try again")));
        }
        // Reading cookies must not happen on the main thread (it deadlocks on
        // Windows); this async task runs elsewhere.
        let cookies = match window.cookies_for_url(url.clone()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // __pus is set once the account is signed in.
        if cookies.iter().any(|c| c.name() == "__pus" && !c.value().is_empty()) {
            let header = cookies.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; ");
            break Ok(header);
        }
    };
    let _ = window.destroy();
    let header = result?;
    if header.is_empty() {
        bail!("no cookies");
    }
    Ok(header)
}
