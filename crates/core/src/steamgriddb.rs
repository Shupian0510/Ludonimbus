//! SteamGridDB (https://www.steamgriddb.com): community covers, banners, logos
//! and icons. Needs the user's own free API key (Preferences → API), kept in
//! the Windows Credential Manager.

use crate::cloud::secrets;
use crate::paths;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const API: &str = "https://www.steamgriddb.com/api/v2";
const KEYRING_USER: &str = "steamgriddb-api-key";
pub const KEY_PAGE: &str = "https://www.steamgriddb.com/profile/preferences/api";

pub fn load_key() -> Result<Option<String>> {
    secrets::load(KEYRING_USER)
}

pub fn store_key(key: &str) -> Result<()> {
    secrets::store(KEYRING_USER, key.trim())
}

pub fn clear_key() -> Result<()> {
    secrets::clear(KEYRING_USER)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgdbGame {
    pub id: u64,
    pub name: String,
    pub year: Option<i32>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgdbImage {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    pub width: u32,
    pub height: u32,
    pub score: i64,
    pub style: String,
    pub author: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(30)).build()
}

fn get(key: &str, path: &str, query: &[(&str, &str)]) -> Result<Value> {
    let url = url::Url::parse_with_params(&format!("{API}{path}"), query)?;
    match agent().get(url.as_str()).set("Authorization", &format!("Bearer {key}")).call() {
        Ok(r) => {
            let v: Value = r.into_json().context(crate::tr!("SteamGridDB 返回了无法识别的内容", "SteamGridDB returned something unexpected"))?;
            if v["success"].as_bool() == Some(false) {
                bail!(crate::tr!("SteamGridDB：{}", "SteamGridDB: {}", v["errors"]));
            }
            Ok(v)
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            bail!(crate::tr!("SteamGridDB API Key 无效，请在设置里重新填写", "The SteamGridDB API key is invalid. Enter it again in Settings"))
        }
        Err(ureq::Error::Status(404, _)) => Ok(serde_json::json!({ "data": [] })),
        Err(ureq::Error::Status(code, _)) => bail!(crate::tr!("SteamGridDB 返回错误 {code}", "SteamGridDB returned error {code}")),
        Err(ureq::Error::Transport(t)) => bail!(crate::tr!("连不上 SteamGridDB：{t}", "Can't reach SteamGridDB: {t}")),
    }
}

/// Search games by name.
pub fn search(key: &str, term: &str) -> Result<Vec<SgdbGame>> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let encoded: String = url::form_urlencoded::byte_serialize(term.as_bytes()).collect();
    let v = get(key, &format!("/search/autocomplete/{}", encoded.replace('+', "%20")), &[])?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| {
                    Some(SgdbGame {
                        id: g["id"].as_u64()?,
                        name: g["name"].as_str()?.to_string(),
                        year: g["release_date"]
                            .as_i64()
                            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                            .map(|d| d.format("%Y").to_string().parse().unwrap_or(0)),
                        verified: g["verified"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Images of one kind (`cover`, `wide`, `hero`, `logo`, `icon`) for a game.
pub fn images(key: &str, game_id: u64, kind: &str) -> Result<Vec<SgdbImage>> {
    let (path, dims) = match kind {
        "cover" => ("grids", Some("600x900,342x482,660x930")),
        "wide" => ("grids", Some("920x430,460x215")),
        "hero" => ("heroes", None),
        "logo" => ("logos", None),
        "icon" => ("icons", None),
        _ => bail!(crate::tr!("未知的图片类型 {kind}", "Unknown image type {kind}")),
    };
    let mut query = vec![("types", "static"), ("nsfw", "false"), ("humor", "false")];
    if let Some(d) = dims {
        query.push(("dimensions", d));
    }
    let v = get(key, &format!("/{path}/game/{game_id}"), &query)?;
    let mut out: Vec<SgdbImage> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    Some(SgdbImage {
                        id: i["id"].as_u64()?,
                        url: i["url"].as_str()?.to_string(),
                        thumb: i["thumb"].as_str().unwrap_or(i["url"].as_str()?).to_string(),
                        width: i["width"].as_u64().unwrap_or(0) as u32,
                        height: i["height"].as_u64().unwrap_or(0) as u32,
                        score: i["score"].as_i64().unwrap_or(0),
                        style: i["style"].as_str().unwrap_or("").to_string(),
                        author: i["author"]["name"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(out)
}

/// Download an image (from SteamGridDB's CDN) to a temporary file.
pub fn download(url: &str) -> Result<PathBuf> {
    let parsed = url::Url::parse(url).map_err(|_| anyhow!(crate::tr!("图片地址无效", "Invalid image address")))?;
    let host = parsed.host_str().unwrap_or("");
    if parsed.scheme() != "https" || !(host == "steamgriddb.com" || host.ends_with(".steamgriddb.com")) {
        bail!(crate::tr!("只能从 SteamGridDB 下载图片", "Images can only be downloaded from SteamGridDB"));
    }
    let resp = agent().get(url).call().map_err(|e| anyhow!(crate::tr!("下载图片失败：{e}", "Couldn't download the image: {e}")))?;
    let ext = match resp.content_type() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    let mut buf = Vec::new();
    resp.into_reader().take(30 * 1024 * 1024).read_to_end(&mut buf)?;
    let dest = paths::staging_dir().join(format!("sgdb-{}.{ext}", ulid::Ulid::new()));
    std::fs::write(&dest, &buf)?;
    Ok(dest)
}
