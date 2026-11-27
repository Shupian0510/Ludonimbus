//! RetroAchievements Web API, read-only, with the user's own name and Web
//! API key (`retroachievements.org` → Settings → Keys). The key lives in the
//! Windows Credential Manager and is never logged or given to the Steam
//! plugin. Emulators with rcheevos report unlocks themselves; Ludonimbus
//! only reads them back. Achievement trigger logic (`MemAddr`) is never read
//! or stored: RA's terms reserve it.

use super::{Achievement, Binding, Scan, Schema, Settings, SourceKind, Unlock};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://retroachievements.org/API";
pub const SITE: &str = "https://retroachievements.org";
pub const MEDIA: &str = "https://media.retroachievements.org";
const KEYRING_USER: &str = "retroachievements";

pub fn api_key() -> Result<Option<String>> {
    crate::cloud::secrets::load(KEYRING_USER)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent(&format!("Ludonimbus/{}", crate::APP_VERSION)).build()
}

fn get(endpoint: &str, key: &str, params: &[(&str, &str)]) -> Result<Value> {
    let mut url = url::Url::parse(&format!("{API}/{endpoint}.php"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("y", key);
        for (k, v) in params {
            q.append_pair(k, v);
        }
    }
    match agent().get(url.as_str()).call() {
        Ok(r) => Ok(r.into_json()?),
        Err(ureq::Error::Status(401 | 403, _)) => bail!(crate::tr!(
            "RetroAchievements 拒绝了请求：用户名或 Web API Key 不对",
            "RetroAchievements refused the request: wrong user name or Web API key"
        )),
        Err(ureq::Error::Status(code, _)) => bail!(crate::tr!("RetroAchievements 返回错误 {code}", "RetroAchievements returned error {code}")),
        // The URL holds the key: never include it in the message.
        Err(ureq::Error::Transport(t)) => Err(anyhow!(crate::cloud::Unavailable(t.kind().to_string()))),
    }
}

/// RA times are UTC, `YYYY-MM-DD HH:MM:SS`.
pub fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|n| Utc.from_utc_datetime(&n))
        .or_else(|| DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.with_timezone(&Utc)))
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn badge_url(badge: &str) -> String {
    format!("{MEDIA}/Badge/{badge}.png")
}

fn media(path: &str) -> Option<String> {
    let p = path.trim();
    (!p.is_empty()).then(|| if p.starts_with("http") { p.to_string() } else { format!("{MEDIA}{p}") })
}

// ---------------------------------------------------------------------------
// Account

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub user: String,
    pub ulid: Option<String>,
    pub avatar: Option<String>,
    pub points: u64,
}

pub fn parse_profile(v: &Value) -> Result<Profile> {
    let user = text(&v["User"]);
    if user.is_empty() {
        bail!(crate::tr!("RetroAchievements 没有返回这个用户", "RetroAchievements returned no such user"));
    }
    Ok(Profile {
        user,
        ulid: v["ULID"].as_str().map(str::to_string),
        avatar: v["UserPic"].as_str().and_then(media),
        points: v["TotalPoints"].as_u64().unwrap_or(0),
    })
}

/// Check the name and key, then remember them.
pub fn connect(user: &str, key: &str) -> Result<Profile> {
    let (user, key) = (user.trim(), key.trim());
    if user.is_empty() || key.is_empty() {
        bail!(crate::tr!("请填写用户名和 Web API Key", "Enter the user name and the Web API key"));
    }
    let profile = parse_profile(&get("API_GetUserProfile", key, &[("u", user)])?)?;
    crate::cloud::secrets::store(KEYRING_USER, key)?;
    super::update_settings(|s| {
        s.ra_user = Some(profile.user.clone());
        s.ra_ulid = profile.ulid.clone();
    })?;
    Ok(profile)
}

pub fn disconnect() -> Result<()> {
    crate::cloud::secrets::clear(KEYRING_USER)?;
    super::update_settings(|s| {
        s.ra_user = None;
        s.ra_ulid = None;
    })
}

fn account(st: &Settings) -> Result<(String, String)> {
    let user = st
        .ra_ulid
        .clone()
        .or_else(|| st.ra_user.clone())
        .ok_or_else(|| anyhow!(crate::tr!("还没有连接 RetroAchievements 账号", "No RetroAchievements account connected yet")))?;
    let key = api_key()?.ok_or_else(|| {
        anyhow!(crate::tr!(
            "找不到 RetroAchievements 的 Web API Key，请在设置里重新连接",
            "The RetroAchievements Web API key is missing. Reconnect in Settings"
        ))
    })?;
    Ok((user, key))
}

// ---------------------------------------------------------------------------
// Games

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaGame {
    pub id: String,
    pub title: String,
    pub console: String,
    pub icon: Option<String>,
    pub last_played: Option<DateTime<Utc>>,
    pub achieved: u32,
    pub total: u32,
}

pub fn parse_recently_played(v: &Value) -> Vec<RaGame> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let id = text(&g["GameID"]);
            (!id.is_empty()).then(|| RaGame {
                id,
                title: text(&g["Title"]),
                console: text(&g["ConsoleName"]),
                icon: g["ImageIcon"].as_str().and_then(media),
                last_played: g["LastPlayed"].as_str().and_then(parse_date),
                achieved: g["NumAchieved"].as_u64().unwrap_or(0) as u32,
                total: g["NumPossibleAchievements"].as_u64().or(g["AchievementsTotal"].as_u64()).unwrap_or(0) as u32,
            })
        })
        .collect()
}

/// The games the user played most recently on RA (for linking).
pub fn recently_played(st: &Settings) -> Result<Vec<RaGame>> {
    let (user, key) = account(st)?;
    Ok(parse_recently_played(&get("API_GetUserRecentlyPlayedGames", &key, &[("u", &user), ("c", "30")])?))
}

/// The RA game played in the last few minutes, to suggest linking it to the
/// game that is running.
pub fn playing_now(st: &Settings) -> Result<Option<RaGame>> {
    let games = recently_played(st)?;
    let cutoff = Utc::now() - chrono::Duration::minutes(10);
    Ok(games.into_iter().find(|g| g.last_played.is_some_and(|t| t > cutoff)))
}

pub fn parse_game(v: &Value, game_id: &str) -> Result<Scan> {
    let map = v["Achievements"].as_object();
    if v["ID"].is_null() && map.is_none() {
        bail!(crate::tr!("RetroAchievements 上没有这个游戏（{game_id}）", "No such game on RetroAchievements ({game_id})"));
    }
    let mut rows: Vec<&Value> = map.into_iter().flat_map(|m| m.values()).collect();
    rows.sort_by_key(|a| (a["DisplayOrder"].as_i64().unwrap_or(0), a["ID"].as_i64().unwrap_or(0)));
    let mut achievements = Vec::new();
    let mut unlocks = Vec::new();
    for a in rows {
        let id = text(&a["ID"]);
        if id.is_empty() {
            continue;
        }
        let badge = text(&a["BadgeName"]);
        achievements.push(Achievement {
            id: id.clone(),
            name: text(&a["Title"]),
            description: text(&a["Description"]),
            icon: (!badge.is_empty()).then(|| badge_url(&badge)),
            hidden: false,
            points: a["Points"].as_u64().map(|p| p as u32),
            grade: None,
        });
        let hard = a["DateEarnedHardcore"].as_str().and_then(parse_date);
        let soft = a["DateEarned"].as_str().and_then(parse_date);
        if let Some(at) = hard.or(soft) {
            unlocks.push(Unlock { id, unlocked_at: soft.map_or(at, |s| s.min(at)), hardcore: hard.is_some() });
        }
    }
    Ok(Scan {
        schema: Some(Schema {
            source: SourceKind::Retroachievements,
            source_id: game_id.to_string(),
            title: v["Title"].as_str().map(str::to_string),
            achievements,
        }),
        unlocks,
        quiet: Vec::new(),
    })
}

/// All achievements of the linked game, with the user's unlocks.
pub fn scan(st: &Settings, b: &Binding) -> Result<Scan> {
    if b.source_id.trim().is_empty() {
        bail!(crate::tr!("还没有选择 RetroAchievements 游戏", "No RetroAchievements game chosen yet"));
    }
    let (user, key) = account(st)?;
    let v = get("API_GetGameInfoAndUserProgress", &key, &[("u", &user), ("g", b.source_id.trim())])?;
    parse_game(&v, b.source_id.trim())
}

/// `(game id, achievement id)` of unlocks in the last `minutes` minutes: a
/// cheap check while a game runs, before reading the whole game again.
pub fn parse_recent(v: &Value) -> Vec<(String, String)> {
    v.as_array().into_iter().flatten().map(|a| (text(&a["GameID"]), text(&a["AchievementID"]))).filter(|(g, a)| !g.is_empty() && !a.is_empty()).collect()
}

pub fn recent_unlocks(st: &Settings, minutes: u32) -> Result<Vec<(String, String)>> {
    let (user, key) = account(st)?;
    Ok(parse_recent(&get("API_GetUserRecentAchievements", &key, &[("u", &user), ("m", &minutes.to_string())])?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn game_progress_from_the_docs_sample() {
        let v = json!({
          "ID": 1, "Title": "Sonic the Hedgehog", "ConsoleName": "Mega Drive", "NumAchievements": 2,
          "Achievements": {
            "9": { "ID": 9, "Title": "That Was Easy", "Description": "Complete the first act in Green Hill Zone",
                   "Points": 3, "BadgeName": "250336", "DisplayOrder": 1, "MemAddr": "secret",
                   "DateEarnedHardcore": "2016-03-12 17:47:29", "DateEarned": "2016-03-12 17:47:29" },
            "10": { "ID": 10, "Title": "Speed", "Description": "Go fast", "Points": 10, "BadgeName": "250337", "DisplayOrder": 2 }
          }
        });
        let scan = parse_game(&v, "1").unwrap();
        let schema = scan.schema.unwrap();
        assert_eq!(schema.title.as_deref(), Some("Sonic the Hedgehog"));
        assert_eq!(schema.achievements.len(), 2);
        assert_eq!(schema.achievements[0].id, "9");
        assert_eq!(schema.achievements[0].icon.as_deref(), Some("https://media.retroachievements.org/Badge/250336.png"));
        assert_eq!(schema.achievements[1].points, Some(10));
        assert_eq!(scan.unlocks.len(), 1);
        assert!(scan.unlocks[0].hardcore);
        assert_eq!(scan.unlocks[0].unlocked_at, Utc.with_ymd_and_hms(2016, 3, 12, 17, 47, 29).unwrap());
        // The trigger logic is never kept.
        assert!(!serde_json::to_string(&schema).unwrap().contains("secret"));
    }

    #[test]
    fn softcore_only_unlock() {
        let v = json!({ "ID": 2, "Title": "X", "Achievements": { "5": { "ID": 5, "Title": "A", "DateEarned": "2026-11-23 10:00:00" } } });
        let scan = parse_game(&v, "2").unwrap();
        assert!(!scan.unlocks[0].hardcore);
    }

    #[test]
    fn recently_played_and_recent_unlocks() {
        let v = json!([{ "GameID": 11332, "ConsoleName": "PlayStation", "Title": "Final Fantasy Origins",
                         "ImageIcon": "/Images/060249.png", "LastPlayed": "2023-10-27 00:30:04",
                         "NumPossibleAchievements": 119, "NumAchieved": 38 }]);
        let games = parse_recently_played(&v);
        assert_eq!(games[0].id, "11332");
        assert_eq!(games[0].icon.as_deref(), Some("https://media.retroachievements.org/Images/060249.png"));
        assert_eq!(games[0].total, 119);
        let r = json!([{ "Date": "2023-12-27 16:04:50", "HardcoreMode": 1, "AchievementID": 98012, "GameID": 14715 }]);
        assert_eq!(parse_recent(&r), vec![("14715".to_string(), "98012".to_string())]);
    }

    #[test]
    fn profile() {
        let p = parse_profile(&json!({ "User": "MaxMilyin", "ULID": "00003EMFWR7XB8SDPEHB3K56ZQ", "UserPic": "/UserPic/MaxMilyin.png", "TotalPoints": 5 }))
            .unwrap();
        assert_eq!(p.ulid.as_deref(), Some("00003EMFWR7XB8SDPEHB3K56ZQ"));
        assert!(parse_profile(&json!({})).is_err());
    }
}
