//! Google Drive (REST v3, `drive.file` scope).
//!
//! Files live in a visible "Ludonimbus" folder in My Drive so users can
//! always download their saves from the Drive website, even without the app.
//! Snapshot summaries are stored in `appProperties` / `description` of the
//! zip file, so one listing call returns the whole history.
//!
//! Sub-folder ids are cached on disk (keyed by the app folder's id) so that a
//! launch from Steam needs only a few requests.

use super::oauth::{self, describe, GoogleClient};
use super::store::{Entry, Store};
use super::{AccountInfo, CloudGame, CloudProvider, DeviceInfo};
use crate::snapshot::{SnapshotMeta, SnapshotSummary, Trigger};
use crate::{paths, util};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
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
    /// Folders the path-based [`Store`] has checked are not in the trash.
    alive: Mutex<std::collections::HashSet<String>>,
    /// Id of the app folder in My Drive, looked up once per process.
    root: Mutex<Option<String>>,
    /// Sub-folder path (e.g. `games/x/snapshots`) → id, below `root`.
    folders: Mutex<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Default)]
struct FolderCache {
    root: String,
    folders: HashMap<String, String>,
}

fn cache_file() -> std::path::PathBuf {
    paths::data_dir().join("gdrive-folders.json")
}

#[derive(Debug, Clone)]
struct DriveFile {
    id: String,
    name: String,
    app: HashMap<String, String>,
    description: String,
    size: u64,
    is_folder: bool,
    /// Changes with the content (md5, or the modification time for folders).
    tag: String,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Why Drive refused a request, when the account itself is the cause.
#[derive(Debug, PartialEq)]
enum Refusal {
    /// The Drive box on Google's consent page was left unticked.
    NoScope,
    /// A company or school account whose admin blocks Drive apps.
    DomainPolicy,
    /// The account has no Google Drive yet.
    NoDrive,
    /// Any other 403 not about a file or a limit: most often an account
    /// that has never used Drive. Holds Google's message.
    Forbidden(String),
}

/// 403 reasons about a file, a quota or the app's own project, not the account.
const ORDINARY_403: &[&str] = &[
    "rateLimitExceeded",
    "userRateLimitExceeded",
    "dailyLimitExceeded",
    "sharingRateLimitExceeded",
    "storageQuotaExceeded",
    "activeItemCreationLimitExceeded",
    "numChildrenInNonRootLimitExceeded",
    "myDriveHierarchyDepthLimitExceeded",
    "insufficientFilePermissions",
    "appNotAuthorizedToFile",
    "accessNotConfigured",
    "SERVICE_DISABLED",
];

/// Read a Drive error body (`{"error": {"message", "errors": [{"reason"}],
/// "details": [{"reason"}]}}`).
fn refusal(code: u16, body: &str) -> Option<Refusal> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let err = &v["error"];
    let message = err["message"].as_str().unwrap_or_default();
    let reasons: Vec<&str> = ["errors", "details"]
        .iter()
        .flat_map(|k| err[*k].as_array().into_iter().flatten())
        .filter_map(|e| e["reason"].as_str())
        .collect();
    let has = |r: &str| reasons.iter().any(|x| x.eq_ignore_ascii_case(r));
    let lower = message.to_lowercase();
    if has("insufficientPermissions")
        || has("ACCESS_TOKEN_SCOPE_INSUFFICIENT")
        || lower.contains("insufficient authentication scopes")
    {
        return Some(Refusal::NoScope);
    }
    if has("domainPolicy") {
        return Some(Refusal::DomainPolicy);
    }
    let project_side = has("accessNotConfigured") || has("SERVICE_DISABLED");
    let no_drive_text = lower.contains("drive")
        && ["not enabled", "not activated", "signed up", "sign up"].iter().any(|p| lower.contains(p));
    if has("serviceNotAvailable") || (no_drive_text && !project_side) {
        return Some(Refusal::NoDrive);
    }
    if code == 403 && !reasons.iter().any(|r| ORDINARY_403.iter().any(|o| o.eq_ignore_ascii_case(r))) {
        let detail = if message.is_empty() { body.trim() } else { message };
        return Some(Refusal::Forbidden(detail.chars().take(300).collect()));
    }
    None
}

/// Map Drive API errors, explaining the refusals the user can fix.
fn drive_error(e: ureq::Error) -> anyhow::Error {
    let (code, body) = match e {
        ureq::Error::Status(code, resp) => (code, resp.into_string().unwrap_or_default()),
        other => return describe(other),
    };
    let open_drive = crate::tr!(
        "请先用浏览器打开 drive.google.com，用同一个 Google 账号登录并完成开通，然后回到 Ludonimbus 重试。",
        "Open drive.google.com in a browser, sign in with the same Google account and finish setting it up, then try again in Ludonimbus."
    );
    match refusal(code, &body) {
        Some(Refusal::NoScope) => oauth::no_drive_scope(),
        Some(Refusal::DomainPolicy) => anyhow!(crate::tr!(
            "这个 Google 账号所属的组织（公司或学校）不允许第三方应用使用 Google Drive。请联系组织的管理员，或者换一个个人 Google 账号。",
            "The organization (company or school) that manages this Google account doesn't let third-party apps use Google Drive. Ask its administrator, or use a personal Google account."
        )),
        Some(Refusal::NoDrive) => anyhow!(crate::tr!(
            "这个 Google 账号还没有开通 Google Drive。{open_drive}",
            "This Google account doesn't have Google Drive yet. {open_drive}"
        )),
        Some(Refusal::Forbidden(detail)) => {
            let hint = crate::tr!(
                "无法访问 Google Drive（{code}）。如果这个 Google 账号从没用过 Google Drive，可能还没有开通：{open_drive}",
                "Can't access Google Drive ({code}). If this Google account has never used Google Drive, it may not be set up yet. {open_drive}"
            );
            if detail.is_empty() {
                anyhow!(hint)
            } else {
                anyhow!(crate::tr!("{hint}（Google 返回：{detail}）", "{hint} (Google said: {detail})"))
            }
        }
        None => oauth::describe_status(code, &body),
    }
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
            alive: Mutex::new(std::collections::HashSet::new()),
            root: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
        }
    }

    fn save_cache(&self) {
        let Some(root) = self.root.lock().unwrap().clone() else { return };
        let cache = FolderCache {
            root,
            folders: self.folders.lock().unwrap().clone(),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&cache) {
            let _ = util::atomic_write(&cache_file(), &bytes);
        }
    }

    /// Drop cached sub-folder ids (e.g. after the user deleted a folder).
    fn forget_folders(&self) {
        self.folders.lock().unwrap().clear();
        self.save_cache();
    }

    /// Whether a folder still exists and is not in the trash.
    fn folder_alive(&self, id: &str) -> Result<bool> {
        match self
            .agent
            .get(&format!("{API}/files/{id}?fields=trashed"))
            .set("Authorization", &self.bearer()?)
            .call()
        {
            Ok(r) => {
                let v: Value = r.into_json()?;
                Ok(!v["trashed"].as_bool().unwrap_or(false))
            }
            Err(ureq::Error::Status(404, _)) => Ok(false),
            Err(e) => Err(drive_error(e)),
        }
    }

    /// Folder for writing: resolved (and created) with a liveness check so a
    /// trashed cached folder is never written into.
    fn writable_folder(&self, parts: &[&str]) -> Result<String> {
        let id = self.folder_id(parts, true)?.expect("created");
        if self.folder_alive(&id)? {
            return Ok(id);
        }
        self.forget_folders();
        Ok(self.folder_id(parts, true)?.expect("created"))
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
                (
                    "fields",
                    "nextPageToken,files(id,name,size,appProperties,description,mimeType,md5Checksum,modifiedTime)".to_string(),
                ),
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
                .map_err(drive_error)?
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
                    is_folder: f["mimeType"].as_str() == Some(FOLDER_MIME),
                    tag: f["md5Checksum"]
                        .as_str()
                        .or(f["modifiedTime"].as_str())
                        .unwrap_or_default()
                        .to_string(),
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
            .map_err(drive_error)?
            .into_json()?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!(crate::tr!("创建文件夹失败", "Couldn't create the folder")))
    }

    fn root_id(&self, create: bool) -> Result<Option<String>> {
        if let Some(r) = self.root.lock().unwrap().clone() {
            return Ok(Some(r));
        }
        let q = format!(
            "name='{}' and 'root' in parents and mimeType='{FOLDER_MIME}' and trashed=false",
            esc(&self.folder)
        );
        let id = match self.list(&q)?.into_iter().next() {
            Some(f) => f.id,
            None if create => self.create_folder(&self.folder, "root")?,
            None => return Ok(None),
        };
        let disk: FolderCache = std::fs::read(cache_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        {
            let mut folders = self.folders.lock().unwrap();
            folders.clear();
            if disk.root == id {
                folders.extend(disk.folders);
            }
        }
        *self.root.lock().unwrap() = Some(id.clone());
        Ok(Some(id))
    }

    /// Resolve (optionally creating) a folder path below the app folder.
    fn folder_id(&self, parts: &[&str], create: bool) -> Result<Option<String>> {
        let Some(mut parent) = self.root_id(create)? else {
            return Ok(None);
        };
        let mut path = String::new();
        for name in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(name);
            if let Some(id) = self.folders.lock().unwrap().get(&path).cloned() {
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
            self.folders.lock().unwrap().insert(path.clone(), id.clone());
            self.save_cache();
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
            .map_err(drive_error)?;
        let location = resp
            .header("Location")
            .ok_or_else(|| anyhow!(crate::tr!("Google Drive 没有返回上传地址", "Google Drive returned no upload address")))?
            .to_string();
        let v: Value = self
            .bulk
            .put(&location)
            .set("Content-Type", mime)
            .send_bytes(&body)
            .map_err(drive_error)?
            .into_json()?;
        v["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!(crate::tr!("上传失败：没有返回文件 id", "Upload failed: no file id returned")))
    }

    fn update_content(&self, file_id: &str, mime: &str, body: &[u8]) -> Result<()> {
        self.bulk
            .request("PATCH", &format!("{UPLOAD}/files/{file_id}?uploadType=media"))
            .set("Authorization", &self.bearer()?)
            .set("Content-Type", mime)
            .send_bytes(body)
            .map_err(drive_error)?;
        Ok(())
    }

    fn patch_meta(&self, file_id: &str, body: Value) -> Result<()> {
        self.agent
            .request("PATCH", &format!("{API}/files/{file_id}"))
            .set("Authorization", &self.bearer()?)
            .send_json(body)
            .map_err(drive_error)?;
        Ok(())
    }

    fn download_bytes(&self, file_id: &str) -> Result<Vec<u8>> {
        let resp = self
            .bulk
            .get(&format!("{API}/files/{file_id}?alt=media"))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(drive_error)?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn delete_file(&self, file_id: &str) -> Result<()> {
        self.agent
            .delete(&format!("{API}/files/{file_id}"))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(drive_error)?;
        Ok(())
    }

    fn put_json(&self, parts: &[&str], name: &str, value: &Value) -> Result<()> {
        let folder = self.writable_folder(parts)?;
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
            .map_err(drive_error)?
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
            .ok_or_else(|| anyhow!(crate::tr!("云端没有这个游戏的快照", "The cloud has no snapshots of this game")))?;
        let f = self
            .find_file(&folder, &format!("{id}.json"))?
            .ok_or_else(|| anyhow!(crate::tr!("找不到快照 {id}", "Snapshot {id} not found")))?;
        Ok(serde_json::from_slice(&self.download_bytes(&f.id)?)?)
    }

    fn upload(&self, meta: &SnapshotMeta, zip: &Path) -> Result<()> {
        let folder = self.writable_folder(&["games", &meta.game, "snapshots"])?;
        let body = std::fs::read(zip).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", zip.display()))?;
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
            .ok_or_else(|| anyhow!(crate::tr!("云端没有这个游戏的快照", "The cloud has no snapshots of this game")))?;
        let f = self
            .find_file(&folder, &format!("{id}.zip"))?
            .ok_or_else(|| anyhow!(crate::tr!("找不到快照文件 {id}", "Snapshot file {id} not found")))?;
        let resp = self
            .bulk
            .get(&format!("{API}/files/{}?alt=media", f.id))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(drive_error)?;
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
            .ok_or_else(|| anyhow!(crate::tr!("云端没有这个游戏的快照", "The cloud has no snapshots of this game")))?;
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

/// Split a store path into its folder segments and file name.
fn split_path(path: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

impl GoogleDrive {
    /// Folder for the path-based [`Store`]: created when missing, and checked
    /// once per instance that it is not in the trash (instances are made per
    /// operation, unlike the long-lived sync provider).
    fn store_folder(&self, parts: &[&str]) -> Result<String> {
        let id = self.folder_id(parts, true)?.expect("created");
        if self.alive.lock().unwrap().contains(&id) {
            return Ok(id);
        }
        let id = if self.folder_alive(&id)? {
            id
        } else {
            self.forget_folders();
            self.folder_id(parts, true)?.expect("created")
        };
        self.alive.lock().unwrap().insert(id.clone());
        Ok(id)
    }

    fn file_at(&self, path: &str) -> Result<Option<DriveFile>> {
        let (dir, name) = split_path(path);
        match self.folder_id(&dir, false)? {
            Some(folder) => self.find_file(&folder, name),
            None => Ok(None),
        }
    }
}

/// Path-based access to the app folder, used by game backups and the speed
/// test (saves keep using [`CloudProvider`] and its file properties).
impl Store for GoogleDrive {
    fn kind(&self) -> &'static str {
        "googleDrive"
    }

    fn label(&self) -> String {
        format!("Google Drive / {}", self.folder)
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        let a = CloudProvider::account(self)?;
        Ok((a.used, a.total))
    }

    fn check(&self) -> Result<()> {
        self.root_id(true).map(|_| ())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(Vec::new());
        };
        let q = format!("'{}' in parents and trashed=false", esc(&folder));
        Ok(self
            .list(&q)?
            .into_iter()
            .map(|f| Entry {
                name: f.name,
                is_dir: f.is_folder,
                size: f.size,
                tag: f.tag,
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.file_at(path)? {
            Some(f) => Ok(Some(self.download_bytes(&f.id)?)),
            None => Ok(None),
        }
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let f = self
            .file_at(path)?
            .ok_or_else(|| anyhow!(crate::tr!("云端找不到 {path}", "{path} not found in the cloud")))?;
        let resp = self
            .bulk
            .get(&format!("{API}/files/{}?alt=media", f.id))
            .set("Authorization", &self.bearer()?)
            .call()
            .map_err(drive_error)?;
        let mut out = std::fs::File::create(dest)?;
        std::io::copy(&mut resp.into_reader(), &mut out)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        let (dir, name) = split_path(path);
        let folder = self.store_folder(&dir)?;
        // A resumable upload only creates the file once all of it arrived.
        match self.find_file(&folder, name)? {
            Some(f) => self.update_content(&f.id, "application/octet-stream", data),
            None => self
                .create_file(&folder, name, "application/octet-stream", json!({}), data.to_vec())
                .map(|_| ()),
        }
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        let data = std::fs::read(src).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", src.display()))?;
        Store::write(self, path, &data)
    }

    fn remove(&self, path: &str) -> Result<()> {
        match self.file_at(path)? {
            Some(f) => match self.delete_file(&f.id) {
                Ok(()) => Ok(()),
                // Someone else deleted it in the meantime.
                Err(_) if self.file_at(path)?.is_none() => Ok(()),
                Err(e) => Err(e),
            },
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(reason: &str, message: &str) -> String {
        json!({ "error": { "code": 403, "message": message, "errors": [{ "domain": "global", "reason": reason, "message": message }] } })
            .to_string()
    }

    #[test]
    fn unticked_drive_scope() {
        let b = body("insufficientPermissions", "Request had insufficient authentication scopes.");
        assert_eq!(refusal(403, &b), Some(Refusal::NoScope));
        let b = json!({ "error": { "code": 403, "message": "x", "details": [{ "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT" }] } }).to_string();
        assert_eq!(refusal(403, &b), Some(Refusal::NoScope));
    }

    #[test]
    fn organization_blocks_drive() {
        let b = body("domainPolicy", "The domain administrators have disabled Drive apps.");
        assert_eq!(refusal(403, &b), Some(Refusal::DomainPolicy));
    }

    #[test]
    fn account_without_drive() {
        assert_eq!(refusal(400, &body("serviceNotAvailable", "Drive service not enabled")), Some(Refusal::NoDrive));
        assert_eq!(refusal(403, &body("forbidden", "The user has not enabled Google Drive.")), Some(Refusal::NoDrive));
        // The app's own project having the API off is not the user's problem.
        let b = body("accessNotConfigured", "Google Drive API has not been used in project 1 before or it is not enabled.");
        assert_eq!(refusal(403, &b), None);
    }

    #[test]
    fn other_403_hints_at_drive_setup() {
        assert_eq!(refusal(403, &body("forbidden", "Forbidden")), Some(Refusal::Forbidden("Forbidden".into())));
        assert_eq!(refusal(403, "nope"), Some(Refusal::Forbidden("nope".into())));
    }

    #[test]
    fn ordinary_errors_keep_their_message() {
        assert_eq!(refusal(403, &body("userRateLimitExceeded", "User Rate Limit Exceeded")), None);
        assert_eq!(refusal(403, &body("storageQuotaExceeded", "The user's Drive storage quota has been exceeded.")), None);
        assert_eq!(refusal(404, &body("notFound", "File not found: x.")), None);
        assert_eq!(refusal(400, &body("invalid", "Invalid Value")), None);
    }
}
