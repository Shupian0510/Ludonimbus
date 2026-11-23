//! Google Drive (REST v3, `drive.file` scope).
//!
//! Files live in a visible "Ludonimbus" folder in My Drive so users can
//! always download their saves from the Drive website, even without the app.
//! Snapshot summaries are stored in `appProperties` / `description` of the
//! zip file, so one listing call returns the whole history.

use super::oauth::{self, describe, GoogleClient};
use super::{AccountInfo, CloudGame, CloudProvider, DeviceInfo};
use crate::snapshot::{SnapshotMeta, SnapshotSummary, Trigger};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

pub struct GoogleDrive {
    client: GoogleClient,
    refresh_token: String,
    folder: String,
    agent: ureq::Agent,
    bulk: ureq::Agent,
    token: Mutex<Option<(String, Instant)>>,
    folders: Mutex<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct DriveFile {
    id: String,
    name: String,
    app: HashMap<String, String>,
    description: String,
    size: u64,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

impl GoogleDrive {
    pub fn new(client: GoogleClient, refresh_token: String, folder: String, timeout: Duration) -> Self {
        GoogleDrive {
            client,
            refresh_token,
            folder: if folder.trim().is_empty() { "Ludonimbus".into() } else { folder },
            agent: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(timeout.max(Duration::from_secs(20)))
                .build(),
            bulk: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(Duration::from_secs(300))
                .timeout_write(Duration::from_secs(300))
                .build(),
            token: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
        }
    }

    fn bearer(&self) -> Result<String> {
        let mut guard = self.token.lock().unwrap();
        if let Some((tok, exp)) = guard.as_ref() {
            if Instant::now() + Duration::from_secs(60) < *exp {
                return Ok(format!("Bearer {tok}"));
            }
        }
        let (tok, secs) = oauth::refresh(&self.agent, &self.client, &self.refresh_token)?;
        *guard = Some((tok.clone(), Instant::now() + Duration::from_secs(secs)));
        Ok(format!("Bearer {tok}"))
    }

    fn list(&self, q: &str) -> Result<Vec<DriveFile>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut params = vec![
                ("q", q.to_string()),
                ("fields", "nextPageToken,files(id,name,size,appProperties,description)".to_string()),
                ("pageSize", "1000".to_string()),
                ("spaces", "drive".to_string()),
            ];
            if let Some(p) = &page {
                params.push(("pageToken", p.clone()));
            }
            let url = url::Url::parse_with_params(&format!("{API}/files"), &params)?;
            let v: Value = self
                .agent
                .get(url.as_str())
                .set("Authorization", &self.bearer()?)
                .call()
                .map_err(describe)?
                .into_json()?;
            for f in v["files"].as_array().into_iter().flatten() {
                out.push(DriveFile {
                    id: f["id"].as_str().unwrap_or_default().to_string(),
                    name: f["name"].as_str().unwrap_or_default().to_string(),
                    app: f["appProperties"]
                        .as_object()
                        .map(|o| {
                            o.iter()
                                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    description: f["description"].as_str().unwrap_or_default().to_string(),
                    size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
            match v["nextPageToken"].as_str() {
                Some(t) => page = Some(t.to_string()),
                None => break,
            }
        }
        Ok(out)
    }

    fn create_folder(&self, name: &str, parent: &str) -> Result<String> {
        let v: Value = self
            .agent
            .post(&format!("{API}/files?fields=id"))
            .set("Authorization", &self.bearer()?)
            .send_json(json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] }))
            .map_err(describe)?
            .into_json()?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("创建文件夹失败"))
    }

    /// Resolve (optionally creating) a folder path below the app folder.
    fn folder_id(&self, parts: &[&str], create: bool) -> Result<Option<String>> {
        let mut cache_key = String::new();
        let mut parent = "root".to_string();
        let mut names = vec![self.folder.as_str()];
        names.extend_from_slice(parts);
        for name in names {
            cache_key.push('/');
            cache_key.push_str(name);
            if let Some(id) = self.folders.lock().unwrap().get(&cache_key).cloned() {
                parent = id;
                continue;
            }
            let q = format!(
                "name='{}' and '{}' in parents and mimeType='{FOLDER_MIME}' and trashed=false",
                esc(name),
                esc(&parent)
            );
            let id = match self.list(&q)?.into_iter().next() {
                Some(f) => f.id,
                None if create => self.create_folder(name, &parent)?,
                None => return Ok(None),
            };
            self.folders.lock().unwrap().insert(cache_key.clone(), id.clone());
            parent = id;
        }
        Ok(Some(parent))
    }

    fn find_file(&self, parent: &str, name: &str) -> Result<Option<DriveFile>> {
        let q = format!("name='{}' and '{}' in parents and trashed=false", esc(name), esc(parent));
        Ok(self.list(&q)?.into_iter().next())
    }

    /// Create a file with a resumable upload (works for any size).
    fn create_file(&self, parent: &str, name: &str, mime: &str, meta_extra: Value, body: Vec<u8>) -> Result<String> {
        let mut meta = json!({ "name": name, "parents": [parent] });
        if let (Some(m), Some(extra)) = (meta.as_object_mut(), meta_extra.as_object()) {
            for (k, v) in extra {
                m.insert(k.clone(), v.clone());
            }
        }
        let resp = self
            .agent
            .post(&format!("{UPLOAD}/files?uploadType=resumable&fields=id"))
            .set("Authorization", &self.bearer()?)
            .set("X-Upload-Content-Type", mime)
            .set("X-Upload-Content-Length", &body.len().to_string())
            .send_json(meta)
            .map_err(describe)?;
        let location = resp
            .header("Location")
            .ok_or_else(|| anyhow!("Google Drive 没有返回上传地址"))?
            .to_string();
        let v: Value = self
            .bulk
            .put(&location)
            .set("Content-Type", mime)
            .send_bytes(&body)
            .map_err(describe)?
            .into_json()?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("上传失败：没有返回文件 id"))
    }

    fn update_content(&self, file_id: &str, mime: &str, body: &[u8]) -> Result<()> {
        self.bulk
            .request("PATCH", &format!("{UPLOAD}/files/{file_id}?uploadType=media"))
            .set("Authorization", &self.bearer()?)
            .set("Content-Type", mime)
            .send_bytes(body)
            .map_err(describe)?;
        Ok(())
    }

    fn patch_meta(&self, file_id: &str, body: Value) -> Result<()> {
        self.agent
            .request("PATCH", &format!("{API}/files/{file_id}"))
            .set("Authorization", &self.bearer()?)
            .send_json(body)
            .map_err(describe)?;
        Ok(())
    }

    fn download_bytes(&self, file_id: &str) -> Result<Vec<u8>> {
        let resp = self
            .bulk
            .get(&format!("{API}/files/{file_id}?alt=media"))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(describe)?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn delete_file(&self, file_id: &str) -> Result<()> {
        self.agent
            .delete(&format!("{API}/files/{file_id}"))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(describe)?;
        Ok(())
    }

    fn put_json(&self, parts: &[&str], name: &str, value: &Value) -> Result<()> {
        let folder = self.folder_id(parts, true)?.expect("created");
        let body = serde_json::to_vec_pretty(value)?;
        match self.find_file(&folder, name)? {
            Some(f) => self.update_content(&f.id, "application/json", &body),
            None => self
                .create_file(&folder, name, "application/json", json!({}), body)
                .map(|_| ()),
        }
    }

    fn summary_props(meta: &SnapshotMeta) -> (Value, String) {
        let props = json!({
            "lnId": meta.id,
            "lnParent": meta.parent.clone().unwrap_or_default(),
            "lnDevice": meta.device_id,
            "lnTrigger": serde_json::to_value(meta.trigger).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
            "lnHash": meta.content_hash,
            "lnCreated": meta.created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "lnSize": meta.total_size.to_string(),
            "lnCount": meta.files.len().to_string(),
            "lnLocked": if meta.locked { "1" } else { "0" },
        });
        let desc = json!({ "deviceName": meta.device_name, "comment": meta.comment }).to_string();
        (props, desc)
    }

    fn summary_from(f: &DriveFile) -> Option<SnapshotSummary> {
        let a = &f.app;
        let id = a.get("lnId")?.clone();
        let desc: Value = serde_json::from_str(&f.description).unwrap_or(Value::Null);
        let trigger: Trigger = serde_json::from_value(Value::String(a.get("lnTrigger")?.clone())).ok()?;
        Some(SnapshotSummary {
            id,
            parent: a.get("lnParent").filter(|s| !s.is_empty()).cloned(),
            device_id: a.get("lnDevice").cloned().unwrap_or_default(),
            device_name: desc["deviceName"].as_str().unwrap_or_default().to_string(),
            created_at: chrono::DateTime::parse_from_rfc3339(a.get("lnCreated")?)
                .ok()?
                .with_timezone(&chrono::Utc),
            trigger,
            content_hash: a.get("lnHash").cloned().unwrap_or_default(),
            total_size: a.get("lnSize").and_then(|s| s.parse().ok()).unwrap_or(f.size),
            file_count: a.get("lnCount").and_then(|s| s.parse().ok()).unwrap_or(0),
            comment: desc["comment"].as_str().unwrap_or_default().to_string(),
            locked: a.get("lnLocked").map(|s| s == "1").unwrap_or(false),
        })
    }
}

impl CloudProvider for GoogleDrive {
    fn kind(&self) -> &'static str {
        "googleDrive"
    }

    fn account(&self) -> Result<AccountInfo> {
        let v: Value = self
            .agent
            .get(&format!("{API}/about?fields=user(emailAddress,displayName),storageQuota(limit,usage)"))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(describe)?
            .into_json()?;
        let num = |p: &str| v["storageQuota"][p].as_str().and_then(|s| s.parse().ok());
        Ok(AccountInfo {
            label: v["user"]["emailAddress"]
                .as_str()
                .or(v["user"]["displayName"].as_str())
                .unwrap_or("Google Drive")
                .to_string(),
            used: num("usage"),
            total: num("limit"),
        })
    }

    fn list_snapshots(&self, game: &str) -> Result<Vec<SnapshotSummary>> {
        let Some(folder) = self.folder_id(&["games", game, "snapshots"], false)? else {
            return Ok(Vec::new());
        };
        let q = format!("'{}' in parents and trashed=false", esc(&folder));
        let mut out: Vec<SnapshotSummary> = self
            .list(&q)?
            .iter()
            .filter(|f| f.name.ends_with(".zip"))
            .filter_map(Self::summary_from)
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    fn get_meta(&self, game: &str, id: &str) -> Result<SnapshotMeta> {
        let folder = self
            .folder_id(&["games", game, "snapshots"], false)?
            .ok_or_else(|| anyhow!("云端没有这个游戏的快照"))?;
        let f = self
            .find_file(&folder, &format!("{id}.json"))?
            .ok_or_else(|| anyhow!("找不到快照 {id}"))?;
        Ok(serde_json::from_slice(&self.download_bytes(&f.id)?)?)
    }

    fn upload(&self, meta: &SnapshotMeta, zip: &Path) -> Result<()> {
        let folder = self
            .folder_id(&["games", &meta.game, "snapshots"], true)?
            .expect("created");
        let body = std::fs::read(zip).with_context(|| format!("无法读取 {}", zip.display()))?;
        let (props, desc) = Self::summary_props(meta);
        // Full metadata first, then the zip (its appProperties make it visible).
        self.create_file(
            &folder,
            &format!("{}.json", meta.id),
            "application/json",
            json!({}),
            serde_json::to_vec_pretty(meta)?,
        )?;
        self.create_file(
            &folder,
            &format!("{}.zip", meta.id),
            "application/zip",
            json!({ "appProperties": props, "description": desc }),
            body,
        )?;
        Ok(())
    }

    fn download(&self, game: &str, id: &str, dest: &Path) -> Result<()> {
        let folder = self
            .folder_id(&["games", game, "snapshots"], false)?
            .ok_or_else(|| anyhow!("云端没有这个游戏的快照"))?;
        let f = self
            .find_file(&folder, &format!("{id}.zip"))?
            .ok_or_else(|| anyhow!("找不到快照文件 {id}"))?;
        let resp = self
            .bulk
            .get(&format!("{API}/files/{}?alt=media", f.id))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(describe)?;
        let mut out = std::fs::File::create(dest)?;
        std::io::copy(&mut resp.into_reader(), &mut out)?;
        Ok(())
    }

    fn delete(&self, game: &str, id: &str) -> Result<()> {
        let Some(folder) = self.folder_id(&["games", game, "snapshots"], false)? else {
            return Ok(());
        };
        for name in [format!("{id}.zip"), format!("{id}.json")] {
            if let Some(f) = self.find_file(&folder, &name)? {
                self.delete_file(&f.id)?;
            }
        }
        Ok(())
    }

    fn update_meta(&self, meta: &SnapshotMeta) -> Result<()> {
        let folder = self
            .folder_id(&["games", &meta.game, "snapshots"], false)?
            .ok_or_else(|| anyhow!("云端没有这个游戏的快照"))?;
        let (props, desc) = Self::summary_props(meta);
        if let Some(f) = self.find_file(&folder, &format!("{}.zip", meta.id))? {
            self.patch_meta(&f.id, json!({ "appProperties": props, "description": desc }))?;
        }
        if let Some(f) = self.find_file(&folder, &format!("{}.json", meta.id))? {
            self.update_content(&f.id, "application/json", &serde_json::to_vec_pretty(meta)?)?;
        }
        Ok(())
    }

    fn put_device(&self, device: &DeviceInfo) -> Result<()> {
        self.put_json(&["devices"], &format!("{}.json", device.id), &serde_json::to_value(device)?)
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let Some(folder) = self.folder_id(&["devices"], false)? else {
            return Ok(Vec::new());
        };
        let q = format!("'{}' in parents and trashed=false", esc(&folder));
        let mut out = Vec::new();
        for f in self.list(&q)? {
            if let Ok(bytes) = self.download_bytes(&f.id) {
                if let Ok(d) = serde_json::from_slice(&bytes) {
                    out.push(d);
                }
            }
        }
        Ok(out)
    }

    fn put_game(&self, game: &CloudGame) -> Result<()> {
        self.put_json(&["games", &game.key], "game.json", &serde_json::to_value(game)?)
    }

    fn list_games(&self) -> Result<Vec<CloudGame>> {
        let Some(games) = self.folder_id(&["games"], false)? else {
            return Ok(Vec::new());
        };
        let q = format!(
            "'{}' in parents and mimeType='{FOLDER_MIME}' and trashed=false",
            esc(&games)
        );
        let mut out = Vec::new();
        for dir in self.list(&q)? {
            if let Some(f) = self.find_file(&dir.id, "game.json")? {
                if let Ok(bytes) = self.download_bytes(&f.id) {
                    if let Ok(g) = serde_json::from_slice(&bytes) {
                        out.push(g);
                    }
                }
            }
        }
        Ok(out)
    }
}
