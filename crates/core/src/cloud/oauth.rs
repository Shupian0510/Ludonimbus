//! Google OAuth for desktop apps: loopback redirect + PKCE (S256).
//!
//! The project publisher registers one "Desktop app" OAuth client and ships
//! its id with releases (`LUDONIMBUS_GOOGLE_CLIENT_ID` /
//! `LUDONIMBUS_GOOGLE_CLIENT_SECRET` at build time). Users only sign in.
//! Desktop client secrets are not confidential by design.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use rand::distributions::{Alphanumeric, DistString};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const SCOPES: &str = "openid email https://www.googleapis.com/auth/drive.file";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const KEYRING_SERVICE: &str = "Ludonimbus";
const KEYRING_USER: &str = "google-drive-refresh-token";

#[derive(Debug, Clone)]
pub struct GoogleClient {
    pub id: String,
    pub secret: String,
}

impl GoogleClient {
    /// Configured values win; otherwise fall back to the ids baked in at build time.
    pub fn resolve(id: &str, secret: &str) -> Option<Self> {
        let id = if id.trim().is_empty() {
            option_env!("LUDONIMBUS_GOOGLE_CLIENT_ID").unwrap_or("").to_string()
        } else {
            id.trim().to_string()
        };
        let secret = if secret.trim().is_empty() {
            option_env!("LUDONIMBUS_GOOGLE_CLIENT_SECRET").unwrap_or("").to_string()
        } else {
            secret.trim().to_string()
        };
        (!id.is_empty()).then_some(GoogleClient { id, secret })
    }

    pub fn has_builtin() -> bool {
        option_env!("LUDONIMBUS_GOOGLE_CLIENT_ID").is_some_and(|s| !s.is_empty())
    }
}

pub struct Authorized {
    pub refresh_token: String,
    pub email: Option<String>,
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Run the browser sign-in. Blocks until the user finishes or `timeout`.
pub fn authorize(client: &GoogleClient, timeout: Duration) -> Result<Authorized> {
    let listener = TcpListener::bind("127.0.0.1:0").context(crate::tr!("无法启动本地回调端口", "Couldn't open the local sign-in callback port"))?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = Alphanumeric.sample_string(&mut rand::thread_rng(), 64);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let state = Alphanumeric.sample_string(&mut rand::thread_rng(), 24);

    let url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client.id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )?;
    opener::open_browser(url.as_str()).context(crate::tr!("无法打开浏览器", "Couldn't open the browser"))?;

    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    let code = loop {
        if Instant::now() > deadline {
            bail!(crate::tr!("等待 Google 登录超时", "Timed out waiting for the Google sign-in"));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut reader = BufReader::new(stream.try_clone()?);
                let mut line = String::new();
                reader.read_line(&mut line)?;
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let parsed = url::Url::parse(&format!("http://127.0.0.1{path}"))?;
                let get = |k: &str| {
                    parsed
                        .query_pairs()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| v.into_owned())
                };
                let (ok, msg) = if let Some(err) = get("error") {
                    (false, crate::tr!("登录被取消或失败：{err}", "Sign-in was cancelled or failed: {err}"))
                } else if get("state").as_deref() != Some(state.as_str()) {
                    (false, crate::tr!("回调校验失败，请重试", "The sign-in callback didn't match. Please try again"))
                } else if get("code").is_some() {
                    (true, crate::tr!("Google Drive 已连接，可以关闭此页面并回到 Ludonimbus。", "Google Drive is connected. You can close this page and go back to Ludonimbus."))
                } else {
                    // e.g. /favicon.ico
                    let mut s = stream;
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                };
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>Ludonimbus</title>\
                     <body style=\"font-family:system-ui;background:#171a21;color:#c7d5e0;display:grid;place-items:center;height:100vh;margin:0\">\
                     <div style=\"text-align:center\"><h2>{}</h2><p>{}</p></div>",
                    if ok { crate::tr!("✅ 完成", "✅ Done") } else { crate::tr!("⚠️ 未完成", "⚠️ Not finished") },
                    msg
                );
                let mut s = stream;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                if !ok {
                    bail!(msg);
                }
                break get("code").unwrap_or_default();
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    };

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
    let resp: serde_json::Value = agent
        .post(TOKEN_URL)
        .send_form(&[
            ("code", code.as_str()),
            ("client_id", client.id.as_str()),
            ("client_secret", client.secret.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ])
        .map_err(describe)?
        .into_json()?;
    if !grants_drive(&resp) {
        return Err(no_drive_scope());
    }
    let refresh_token = resp["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow!(crate::tr!("Google 没有返回 refresh token", "Google returned no refresh token")))?
        .to_string();
    let email = resp["id_token"].as_str().and_then(email_from_id_token);
    Ok(Authorized { refresh_token, email })
}

fn email_from_id_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v["email"].as_str().map(str::to_string)
}

/// Exchange the refresh token for a short-lived access token.
pub fn refresh(agent: &ureq::Agent, client: &GoogleClient, refresh_token: &str) -> Result<(String, u64)> {
    let resp: serde_json::Value = agent
        .post(TOKEN_URL)
        .send_form(&[
            ("client_id", client.id.as_str()),
            ("client_secret", client.secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .map_err(describe)?
        .into_json()?;
    let token = resp["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!(crate::tr!("Google 登录已失效，请在设置里重新连接 Google Drive", "The Google sign-in has expired. Reconnect Google Drive in Settings")))?
        .to_string();
    Ok((token, resp["expires_in"].as_u64().unwrap_or(3600)))
}

/// Map ureq errors: transport problems become [`super::Unavailable`].
pub fn describe(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => describe_status(code, &resp.into_string().unwrap_or_default()),
        ureq::Error::Transport(t) => anyhow!(super::Unavailable(t.to_string())),
    }
}

/// Map an HTTP error status and its body.
pub fn describe_status(code: u16, body: &str) -> anyhow::Error {
    if code == 400 && body.contains("invalid_grant") {
        anyhow!(crate::tr!("Google 登录已失效（{code}），请在设置里重新连接 Google Drive", "The Google sign-in has expired ({code}). Reconnect Google Drive in Settings"))
    } else if code >= 500 {
        anyhow!(super::Unavailable(crate::tr!("Google 服务器错误 {code}", "Google server error {code}")))
    } else {
        anyhow!(crate::tr!("Google 返回错误 {code}：{}", "Google returned error {code}: {}", body.chars().take(400).collect::<String>()))
    }
}

/// The Google Drive box on the consent page was left unticked.
pub fn no_drive_scope() -> anyhow::Error {
    anyhow!(crate::tr!(
        "Ludonimbus 没有拿到 Google Drive 的访问权限（Google 授权页上 Google Drive 那一项没有勾选）。请重新连接 Google Drive，并勾选这一项。",
        "Ludonimbus wasn't given access to Google Drive (the Google Drive box on Google's consent page was left unticked). Connect Google Drive again and tick that box."
    ))
}

/// Whether the token response grants Drive access. A response that doesn't
/// list its scopes is trusted.
fn grants_drive(token_response: &serde_json::Value) -> bool {
    token_response["scope"]
        .as_str()
        .is_none_or(|s| s.split_whitespace().any(|x| x == DRIVE_SCOPE))
}

pub fn store_refresh_token(token: &str) -> Result<()> {
    crate::credstore::set(KEYRING_SERVICE, KEYRING_USER, token)
}

pub fn load_refresh_token() -> Result<Option<String>> {
    crate::credstore::get(KEYRING_SERVICE, KEYRING_USER)
}

pub fn clear_refresh_token() -> Result<()> {
    crate::credstore::delete(KEYRING_SERVICE, KEYRING_USER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notices_unticked_drive_scope() {
        let granted = json!({ "scope": "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/drive.file" });
        assert!(grants_drive(&granted));
        let unticked = json!({ "scope": "openid https://www.googleapis.com/auth/userinfo.email" });
        assert!(!grants_drive(&unticked));
        assert!(grants_drive(&json!({ "access_token": "x" })));
    }
}
