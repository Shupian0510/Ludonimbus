//! Google Drive (REST v3, `drive.file` scope) as a path-based [`Store`].
//!
//! Files live in a visible "Ludonimbus" folder in My Drive so users can
//! always download their saves from the Drive website, even without the app.
//! What goes into that folder is decided by [`super::repo::Repo`], as on
//! every other cloud; this file only turns paths into Drive's files.
//!
//! Drive addresses files by id, so a path is resolved folder by folder.
//! Sub-folder ids are cached on disk (keyed by the app folder's id) so that a
//! launch from Steam needs only a few requests.
//!
//! Drive also allows several files with one name in a folder, which happens
//! when two devices create the same file at the same moment.
//! [`DriveFile::beats`] says which of them counts and every operation
//! follows it, so all devices read the same one.

use super::oauth::{self, describe, GoogleClient};
use super::store::{Entry, Store};
use super::Unavailable;
use crate::{paths, util};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
/// What a listing asks Drive for about each file.
const FILE_FIELDS: &str = "id,name,size,mimeType,md5Checksum,createdTime,modifiedTime";
/// Largest content replaced with a simple upload, the kind Drive documents
/// for up to 5 MB. Anything bigger goes through an upload session.
const SIMPLE_UPLOAD_MAX: usize = 5 * 1024 * 1024;

pub struct GoogleDrive {
    client: GoogleClient,
    refresh_token: String,
    folder: String,
    /// Which sign-in and folder name this is, see [`sign_in_key`].
    owner: String,
    agent: ureq::Agent,
    bulk: ureq::Agent,
    token: Mutex<Option<(String, Instant)>>,
    /// Id of the app folder in My Drive, looked up once per process.
    root: Mutex<Option<String>>,
    /// Sub-folder path (e.g. `games/x/snapshots`) → id, below `root`.
    folders: Mutex<HashMap<String, String>>,
    /// The account's e-mail address (or name), once [`Store::quota`] has
    /// asked Drive for it.
    account: Mutex<Option<String>>,
}

#[derive(Serialize, Deserialize, Default)]
struct FolderCache {
    /// [`sign_in_key`] of the sign-in that looked `root` up. Empty in files
    /// written by older versions.
    #[serde(default)]
    owner: String,
    root: String,
    folders: HashMap<String, String>,
}

impl FolderCache {
    /// The app folder's id, when this cache was written for the same sign-in
    /// and folder name. A cache left by another Google account says nothing
    /// about this one.
    fn root_for(&self, owner: &str) -> Option<String> {
        (self.owner == owner && !self.root.is_empty()).then(|| self.root.clone())
    }
}

fn cache_file() -> std::path::PathBuf {
    paths::data_dir().join("gdrive-folders.json")
}

fn read_cache() -> FolderCache {
    std::fs::read(cache_file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Names one sign-in and folder name without giving either away, so the
/// folder cache on disk can say whom it belongs to. Signing in again, also to
/// the same account, gives a new key: the cache is then filled again by the
/// first request.
fn sign_in_key(client_id: &str, refresh_token: &str, folder: &str) -> String {
    let text = format!("gdrive\n{client_id}\n{folder}\n{refresh_token}");
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// [`Store::cache_scope`] of an app folder. Its id belongs to one folder of
/// one account, and stays the same for as long as that folder exists.
fn scope_of(root_id: &str) -> String {
    format!("gdrive|{root_id}")
}

#[derive(Debug, Clone, PartialEq)]
struct DriveFile {
    id: String,
    name: String,
    size: u64,
    is_folder: bool,
    created: Option<DateTime<Utc>>,
    modified: Option<DateTime<Utc>>,
    /// Changes whenever the content changes, see [`change_tag`].
    tag: String,
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

/// What tells one content of a file from another: Drive's MD5 of the
/// content. Folders (and Google's own document types, which Ludonimbus never
/// creates) have no checksum and fall back to the modification time.
fn change_tag(md5: Option<&str>, modified: Option<&str>) -> String {
    md5.or(modified).unwrap_or_default().to_string()
}

impl DriveFile {
    /// One entry of a `files.list` answer.
    fn parse(f: &Value) -> DriveFile {
        DriveFile {
            id: f["id"].as_str().unwrap_or_default().to_string(),
            name: f["name"].as_str().unwrap_or_default().to_string(),
            // Drive sends 64-bit numbers as strings.
            size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            is_folder: f["mimeType"].as_str() == Some(FOLDER_MIME),
            created: time(&f["createdTime"]),
            modified: time(&f["modifiedTime"]),
            tag: change_tag(f["md5Checksum"].as_str(), f["modifiedTime"].as_str()),
        }
    }

    /// Whether this entry counts rather than `other` when both have the same
    /// name in one folder.
    ///
    /// Files: the most recently modified, which is the last write. Folders:
    /// the one created first, so every device settles on the same folder and
    /// a duplicate made later never hides one that already has files in it.
    /// The id settles ties, so the answer never depends on the order Drive
    /// lists them in.
    fn beats(&self, other: &DriveFile) -> bool {
        if self.is_folder {
            (self.created, &self.id) < (other.created, &other.id)
        } else {
            (self.modified, &self.id) > (other.modified, &other.id)
        }
    }
}

/// Split entries of one name and kind into the one that counts and the rest.
fn pick(mut same: Vec<DriveFile>) -> Option<(DriveFile, Vec<DriveFile>)> {
    let best = (0..same.len()).reduce(|a, b| if same[b].beats(&same[a]) { b } else { a })?;
    let current = same.swap_remove(best);
    Some((current, same))
}

/// A folder's entries with one entry per name, in the order Drive listed
/// them. A file and a folder may share a name; they stay two entries.
fn one_per_name(files: Vec<DriveFile>) -> Vec<DriveFile> {
    let mut out: Vec<DriveFile> = Vec::new();
    let mut seen: HashMap<(String, bool), usize> = HashMap::new();
    for f in files {
        match seen.get(&(f.name.clone(), f.is_folder)) {
            Some(&i) => {
                if f.beats(&out[i]) {
                    out[i] = f;
                }
            }
            None => {
                seen.insert((f.name.clone(), f.is_folder), out.len());
                out.push(f);
            }
        }
    }
    out
}

/// The content type stored with a file. Drive's website chooses the icon and
/// the preview by it.
fn mime_for(name: &str) -> &'static str {
    if name.ends_with(".json") {
        "application/json"
    } else if name.ends_with(".zip") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}

/// What `about` says: the account to show, bytes used and bytes in total
/// (no total on accounts without a limit).
fn parse_about(v: &Value) -> (Option<String>, Option<u64>, Option<u64>) {
    let num = |p: &str| v["storageQuota"][p].as_str().and_then(|s| s.parse().ok());
    let user = &v["user"];
    let account = user["emailAddress"]
        .as_str()
        .or(user["displayName"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    (account, num("usage"), num("limit"))
}

/// Read a JSON answer. A body that stops half-way is the connection failing,
/// so it is reported like any other connection problem.
fn body_json(resp: ureq::Response) -> Result<Value> {
    resp.into_json().map_err(|e| anyhow!(Unavailable(e.to_string())))
}

/// Copy a response body to `out`. A read that fails is the connection
/// dropping and becomes [`Unavailable`]; a write that fails (a full disk) is
/// not the cloud's fault and keeps its own error.
fn copy_body(mut body: impl Read, out: &mut impl Write) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(anyhow!(Unavailable(e.to_string()))),
        };
        out.write_all(&buf[..n])?;
    }
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
        let folder = if folder.trim().is_empty() { "Ludonimbus".to_string() } else { folder };
        GoogleDrive {
            owner: sign_in_key(&client.id, &refresh_token, &folder),
            client,
            refresh_token,
            folder,
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
            root: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
            account: Mutex::new(None),
        }
    }

    fn save_cache(&self) {
        let Some(root) = self.root.lock().unwrap().clone() else { return };
        let cache = FolderCache {
            owner: self.owner.clone(),
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
            Ok(r) => Ok(!body_json(r)?["trashed"].as_bool().unwrap_or(false)),
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

    /// Everything a Drive search query matches.
    fn query(&self, q: &str) -> Result<Vec<DriveFile>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut params = vec![
                ("q", q.to_string()),
                ("fields", format!("nextPageToken,files({FILE_FIELDS})")),
                ("pageSize", "1000".to_string()),
                ("spaces", "drive".to_string()),
            ];
            if let Some(p) = &page {
                params.push(("pageToken", p.clone()));
            }
            let url = url::Url::parse_with_params(&format!("{API}/files"), &params)?;
            let v = body_json(
                self.agent
                    .get(url.as_str())
                    .set("Authorization", &self.bearer()?)
                    .call()
                    .map_err(drive_error)?,
            )?;
            out.extend(v["files"].as_array().into_iter().flatten().map(DriveFile::parse));
            match v["nextPageToken"].as_str() {
                Some(t) => page = Some(t.to_string()),
                None => break,
            }
        }
        Ok(out)
    }

    fn create_folder(&self, name: &str, parent: &str) -> Result<String> {
        let v = body_json(
            self.agent
                .post(&format!("{API}/files?fields=id"))
                .set("Authorization", &self.bearer()?)
                .send_json(json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] }))
                .map_err(drive_error)?,
        )?;
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
        let id = match pick(self.query(&q)?) {
            Some((f, _)) => f.id,
            None if create => self.create_folder(&self.folder, "root")?,
            None => return Ok(None),
        };
        let disk = read_cache();
        let same_root = disk.root == id;
        {
            let mut folders = self.folders.lock().unwrap();
            folders.clear();
            if same_root {
                folders.extend(disk.folders);
            }
        }
        *self.root.lock().unwrap() = Some(id.clone());
        // Also before any sub-folder is known: the next process takes the
        // app folder's id from this file for `cache_scope`.
        if !same_root || disk.owner != self.owner {
            self.save_cache();
        }
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
            let id = match pick(self.query(&q)?) {
                Some((f, _)) => f.id,
                None if create => self.create_folder(name, &parent)?,
                None => return Ok(None),
            };
            self.folders.lock().unwrap().insert(path.clone(), id.clone());
            self.save_cache();
            parent = id;
        }
        Ok(Some(parent))
    }

    /// Every file (not folder) called `name` directly in a folder: more than
    /// one when devices created it at the same moment.
    fn named(&self, parent: &str, name: &str) -> Result<Vec<DriveFile>> {
        let q = format!("name='{}' and '{}' in parents and trashed=false", esc(name), esc(parent));
        Ok(self.query(&q)?.into_iter().filter(|f| !f.is_folder).collect())
    }

    /// The file a path stands for, if there is one.
    fn file_at(&self, path: &str) -> Result<Option<DriveFile>> {
        let (parts, name) = split_path(path);
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(None);
        };
        Ok(pick(self.named(&folder, name)?).map(|(current, _)| current))
    }

    /// Send `body` through an upload session: `start` opens it and one more
    /// request carries the bytes. The file appears (or changes) only once the
    /// last byte has arrived, at any size.
    fn upload_session(&self, start: ureq::Request, meta: Value, mime: &str, body: &[u8]) -> Result<Value> {
        let resp = start
            .set("Authorization", &self.bearer()?)
            .set("X-Upload-Content-Type", mime)
            .set("X-Upload-Content-Length", &body.len().to_string())
            .send_json(meta)
            .map_err(drive_error)?;
        let location = resp
            .header("Location")
            .ok_or_else(|| anyhow!(crate::tr!("Google Drive 没有返回上传地址", "Google Drive returned no upload address")))?
            .to_string();
        body_json(
            self.bulk
                .put(&location)
                .set("Content-Type", mime)
                .send_bytes(body)
                .map_err(drive_error)?,
        )
    }

    fn create_file(&self, parent: &str, name: &str, mime: &str, body: &[u8]) -> Result<()> {
        let start = self.agent.post(&format!("{UPLOAD}/files?uploadType=resumable&fields=id"));
        let v = self.upload_session(start, json!({ "name": name, "parents": [parent] }), mime, body)?;
        if v["id"].as_str().is_none() {
            return Err(anyhow!(crate::tr!("上传失败：没有返回文件 id", "Upload failed: no file id returned")));
        }
        Ok(())
    }

    fn update_content(&self, file_id: &str, mime: &str, body: &[u8]) -> Result<()> {
        if body.len() > SIMPLE_UPLOAD_MAX {
            let start = self
                .agent
                .request("PATCH", &format!("{UPLOAD}/files/{file_id}?uploadType=resumable&fields=id"));
            return self.upload_session(start, json!({}), mime, body).map(|_| ());
        }
        self.bulk
            .request("PATCH", &format!("{UPLOAD}/files/{file_id}?uploadType=media"))
            .set("Authorization", &self.bearer()?)
            .set("Content-Type", mime)
            .send_bytes(body)
            .map_err(drive_error)?;
        Ok(())
    }

    /// Start downloading a file's content. `None` when the file is gone.
    fn media(&self, file_id: &str) -> Result<Option<ureq::Response>> {
        match self
            .bulk
            .get(&format!("{API}/files/{file_id}?alt=media"))
            .set("Authorization", &self.bearer()?)
            .call()
        {
            Ok(r) => Ok(Some(r)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(drive_error(e)),
        }
    }

    fn delete_file(&self, file_id: &str) -> Result<()> {
        match self
            .agent
            .delete(&format!("{API}/files/{file_id}"))
            .set("Authorization", &self.bearer()?)
            .call()
        {
            // Already gone (another device deleted it): that is what was asked for.
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(drive_error(e)),
        }
    }

    /// Create the file a path stands for, or replace its content.
    fn put(&self, path: &str, data: &[u8]) -> Result<()> {
        let (parts, name) = split_path(path);
        let folder = self.writable_folder(&parts)?;
        let mime = mime_for(name);
        let Some((current, others)) = pick(self.named(&folder, name)?) else {
            return self.create_file(&folder, name, mime, data);
        };
        self.update_content(&current.id, mime, data)?;
        // The file just written is now the newest of its name, so readers
        // already get it; a duplicate that can't be deleted now goes with
        // the next write.
        for other in others {
            let _ = self.delete_file(&other.id);
        }
        Ok(())
    }
}

/// Split a store path into its folder segments and file name.
fn split_path(path: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

/// Everything Ludonimbus keeps in Google Drive goes through these operations:
/// the save history ([`super::repo::Repo`]) and all other data
/// ([`super::CloudProvider::files`]).
///
/// Requests to Drive per call, once the folder ids are cached and the access
/// token is fresh: `list` one per 1000 entries; `read` and `download` two
/// (find the file by name, fetch it); `write` and `upload` three to replace a
/// file (is the folder still there, find, send) and four to create one or to
/// replace more than [`SIMPLE_UPLOAD_MAX`]; `remove` two; `check` and `quota`
/// one each.
impl Store for GoogleDrive {
    fn kind(&self) -> &'static str {
        "googleDrive"
    }

    fn label(&self) -> String {
        // The account once Drive has named it, so Settings shows who is
        // signed in: `Repo::account` asks for `quota` before the label.
        let account = self.account.lock().unwrap().clone();
        account.unwrap_or_else(|| format!("Google Drive · {}", self.folder))
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        let v = body_json(
            self.agent
                .get(&format!("{API}/about?fields=user(emailAddress,displayName),storageQuota(limit,usage)"))
                .set("Authorization", &self.bearer()?)
                .call()
                .map_err(drive_error)?,
        )?;
        let (account, used, total) = parse_about(&v);
        if account.is_some() {
            *self.account.lock().unwrap() = account;
        }
        Ok((used, total))
    }

    fn check(&self) -> Result<()> {
        // Ask Drive even when the folder is already known to this process:
        // this is the test of whether the cloud can be reached.
        let known = self.root.lock().unwrap().clone();
        if let Some(id) = known {
            if self.folder_alive(&id)? {
                return Ok(());
            }
            // Deleted in Drive meanwhile: look it up, or create it, again.
            *self.root.lock().unwrap() = None;
            self.folders.lock().unwrap().clear();
        }
        self.root_id(true).map(|_| ())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(Vec::new());
        };
        let q = format!("'{}' in parents and trashed=false", esc(&folder));
        Ok(one_per_name(self.query(&q)?)
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
        let Some(f) = self.file_at(path)? else {
            return Ok(None);
        };
        let Some(resp) = self.media(&f.id)? else {
            return Ok(None);
        };
        let mut buf = Vec::new();
        copy_body(resp.into_reader(), &mut buf)?;
        Ok(Some(buf))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let missing = || anyhow!(crate::tr!("云端找不到 {path}", "{path} wasn't found in the cloud"));
        let f = self.file_at(path)?.ok_or_else(missing)?;
        let resp = self.media(&f.id)?.ok_or_else(missing)?;
        let tmp = dest.with_extension("lntmp");
        let mut out = std::fs::File::create(&tmp)?;
        copy_body(resp.into_reader(), &mut out)?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.put(path, data)
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        // The whole file is held in memory while it is sent. That is fine
        // for saves of tens of megabytes and too much for very large files;
        // sending in pieces that can be resumed is planned separately.
        let data = std::fs::read(src).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", src.display()))?;
        self.put(path, &data)
    }

    fn remove(&self, path: &str) -> Result<()> {
        let (parts, name) = split_path(path);
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(());
        };
        // Every file of that name: a duplicate left behind would turn up as
        // the file again.
        for f in self.named(&folder, name)? {
            self.delete_file(&f.id)?;
        }
        Ok(())
    }

    fn cache_scope(&self) -> Option<String> {
        // `Repo` asks once, when it is built and before any request, so the
        // app folder's id has to be known on this PC already: from an
        // earlier request of this process, or from the folder cache a
        // previous one left. Right after signing in there is none yet, and
        // that `Repo` reads without its cache.
        let resolved = self.root.lock().unwrap().clone();
        let root = match resolved {
            Some(id) => id,
            None => read_cache().root_for(&self.owner)?,
        };
        Some(scope_of(&root))
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

    fn at(s: &str) -> Option<DateTime<Utc>> {
        time(&json!(s))
    }

    fn file(id: &str, name: &str, modified: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            size: 1,
            is_folder: false,
            created: at("2026-01-01T00:00:00.000Z"),
            modified: at(modified),
            tag: format!("md5-of-{id}"),
        }
    }

    fn folder(id: &str, name: &str, created: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            size: 0,
            is_folder: true,
            created: at(created),
            modified: at("2026-06-01T00:00:00.000Z"),
            tag: String::new(),
        }
    }

    fn ids(files: &[DriveFile]) -> Vec<&str> {
        files.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn the_most_recently_modified_file_counts() {
        let old = file("z-old", "index.json", "2026-11-26T10:00:00.000Z");
        let new = file("a-new", "index.json", "2026-11-26T10:00:00.250Z");
        let older = file("m-older", "index.json", "2026-11-25T23:59:59.999Z");
        // Whatever order Drive lists them in.
        for listed in [
            vec![old.clone(), new.clone(), older.clone()],
            vec![new.clone(), older.clone(), old.clone()],
            vec![older.clone(), old.clone(), new.clone()],
        ] {
            let (current, others) = pick(listed).unwrap();
            assert_eq!(current.id, "a-new");
            let mut rest = ids(&others);
            rest.sort();
            assert_eq!(rest, ["m-older", "z-old"]);
        }
    }

    #[test]
    fn files_modified_at_the_same_moment_are_settled_by_id() {
        let a = file("a", "index.json", "2026-11-26T10:00:00.000Z");
        let b = file("b", "index.json", "2026-11-26T10:00:00.000Z");
        assert_eq!(pick(vec![a.clone(), b.clone()]).unwrap().0.id, "b");
        assert_eq!(pick(vec![b, a]).unwrap().0.id, "b");
    }

    #[test]
    fn a_file_without_a_time_loses() {
        let mut blank = file("z", "index.json", "2026-11-26T10:00:00.000Z");
        blank.modified = None;
        let dated = file("a", "index.json", "2020-01-01T00:00:00.000Z");
        assert_eq!(pick(vec![blank.clone(), dated.clone()]).unwrap().0.id, "a");
        assert_eq!(pick(vec![dated, blank]).unwrap().0.id, "a");
    }

    #[test]
    fn the_folder_created_first_counts() {
        let first = folder("z-first", "games", "2026-11-01T08:00:00.000Z");
        let second = folder("a-second", "games", "2026-11-01T08:00:00.400Z");
        assert_eq!(pick(vec![second.clone(), first.clone()]).unwrap().0.id, "z-first");
        assert_eq!(pick(vec![first, second]).unwrap().0.id, "z-first");
    }

    #[test]
    fn one_file_or_none() {
        assert_eq!(pick(Vec::new()), None);
        let only = file("a", "game.json", "2026-11-26T10:00:00.000Z");
        assert_eq!(pick(vec![only.clone()]), Some((only, Vec::new())));
    }

    #[test]
    fn a_listing_has_one_entry_per_name() {
        let listed = vec![
            file("1", "01A.json", "2026-11-26T10:00:00.000Z"),
            file("2", "index.json", "2026-11-26T10:00:00.000Z"),
            folder("3", "thumbnails", "2026-11-02T00:00:00.000Z"),
            file("4", "index.json", "2026-11-26T10:00:05.000Z"),
            file("5", "01A.zip", "2026-11-26T09:00:00.000Z"),
            folder("6", "thumbnails", "2026-11-01T00:00:00.000Z"),
            // A file may be called like a folder; both are kept.
            file("7", "thumbnails", "2026-11-26T10:00:00.000Z"),
        ];
        let out = one_per_name(listed);
        assert_eq!(ids(&out), ["1", "4", "6", "5", "7"]);
        // The entry carries the tag of the file that reads return.
        assert_eq!(out[1].tag, "md5-of-4");
    }

    #[test]
    fn reads_a_listing_entry() {
        let zip = DriveFile::parse(&json!({
            "id": "1AbC", "name": "01JDV.zip", "mimeType": "application/zip", "size": "15728640",
            "md5Checksum": "9e107d9d372bb6826bd81d3542a419d6",
            "createdTime": "2026-11-20T08:00:00.000Z", "modifiedTime": "2026-11-20T08:00:05.123Z"
        }));
        assert_eq!(zip.id, "1AbC");
        assert_eq!(zip.name, "01JDV.zip");
        assert_eq!(zip.size, 15_728_640);
        assert!(!zip.is_folder);
        assert_eq!(zip.tag, "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(zip.modified, at("2026-11-20T08:00:05.123Z"));
        assert!(zip.created < zip.modified);

        let dir = DriveFile::parse(&json!({
            "id": "f1", "name": "snapshots", "mimeType": FOLDER_MIME,
            "createdTime": "2026-11-20T08:00:00.000Z", "modifiedTime": "2026-11-21T08:00:00.000Z"
        }));
        assert!(dir.is_folder);
        assert_eq!(dir.size, 0);
        assert_eq!(dir.tag, "2026-11-21T08:00:00.000Z");
    }

    #[test]
    fn the_tag_is_the_checksum_of_the_content() {
        let t = "2026-11-20T08:00:05.123Z";
        // Same content written again: same tag, although the time moved on.
        assert_eq!(change_tag(Some("abc"), Some(t)), change_tag(Some("abc"), Some("2026-11-21T00:00:00.000Z")));
        assert_ne!(change_tag(Some("abc"), Some(t)), change_tag(Some("abd"), Some(t)));
        assert_eq!(change_tag(None, Some(t)), t);
        assert_eq!(change_tag(None, None), "");
    }

    #[test]
    fn splits_paths() {
        assert_eq!(split_path("games/fe/snapshots/01J.zip"), (vec!["games", "fe", "snapshots"], "01J.zip"));
        assert_eq!(split_path("index.json"), (vec![], "index.json"));
        assert_eq!(split_path("/games//fe/game.json"), (vec!["games", "fe"], "game.json"));
        assert_eq!(split_path(""), (vec![], ""));
    }

    #[test]
    fn escapes_names_in_queries() {
        assert_eq!(esc("Baldur's Gate"), "Baldur\\'s Gate");
        assert_eq!(esc("a\\b"), "a\\\\b");
        assert_eq!(esc("血源"), "血源");
    }

    #[test]
    fn content_types() {
        assert_eq!(mime_for("01J.json"), "application/json");
        assert_eq!(mime_for("01J.zip"), "application/zip");
        assert_eq!(mime_for("shot.jpg"), "application/octet-stream");
    }

    #[test]
    fn about_names_the_account_and_the_space() {
        let v = json!({
            "user": { "emailAddress": "someone@example.com", "displayName": "Some One" },
            "storageQuota": { "limit": "16106127360", "usage": "1048576" }
        });
        assert_eq!(parse_about(&v), (Some("someone@example.com".into()), Some(1_048_576), Some(16_106_127_360)));
        // No address: the name. No limit: an account without one.
        let v = json!({ "user": { "displayName": "Some One" }, "storageQuota": { "usage": "5" } });
        assert_eq!(parse_about(&v), (Some("Some One".into()), Some(5), None));
        assert_eq!(parse_about(&json!({})), (None, None, None));
    }

    fn drive(refresh_token: &str, folder: &str) -> GoogleDrive {
        let client = GoogleClient {
            id: "client-id".into(),
            secret: String::new(),
        };
        GoogleDrive::new(client, refresh_token.into(), folder.into(), Duration::from_secs(5))
    }

    #[test]
    fn the_label_is_the_account_once_known() {
        let d = drive("token", "");
        assert_eq!(Store::kind(&d), "googleDrive");
        assert_eq!(d.label(), "Google Drive · Ludonimbus");
        *d.account.lock().unwrap() = Some("someone@example.com".into());
        assert_eq!(d.label(), "someone@example.com");
    }

    #[test]
    fn the_cache_scope_is_the_app_folder() {
        let d = drive("token", "Ludonimbus");
        *d.root.lock().unwrap() = Some("1RootFolderId".into());
        assert_eq!(d.cache_scope(), Some("gdrive|1RootFolderId".into()));
        assert_ne!(scope_of("1RootFolderId"), scope_of("1OtherFolderId"));
    }

    #[test]
    fn the_folder_cache_belongs_to_one_sign_in() {
        let mine = drive("token-a", "Ludonimbus");
        let cache = FolderCache {
            owner: mine.owner.clone(),
            root: "1RootFolderId".into(),
            folders: HashMap::new(),
        };
        assert_eq!(cache.root_for(&mine.owner), Some("1RootFolderId".into()));
        // The same sign-in opened again reads it; another account or another
        // folder name does not.
        assert_eq!(cache.root_for(&drive("token-a", "Ludonimbus").owner), Some("1RootFolderId".into()));
        assert_eq!(cache.root_for(&drive("token-b", "Ludonimbus").owner), None);
        assert_eq!(cache.root_for(&drive("token-a", "Other").owner), None);
        // A cache file written before the owner was recorded.
        let old: FolderCache = serde_json::from_str(r#"{ "root": "1RootFolderId", "folders": {} }"#).unwrap();
        assert_eq!(old.root_for(&mine.owner), None);
        assert_eq!(FolderCache::default().root_for(""), None);
    }

    #[test]
    fn the_sign_in_key_does_not_show_the_token() {
        let key = sign_in_key("client-id", "1//refresh-token-value", "Ludonimbus");
        assert_eq!(key.len(), 16);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(key, sign_in_key("client-id", "1//refresh-token-value", "Ludonimbus"));
        assert_ne!(key, sign_in_key("other-client", "1//refresh-token-value", "Ludonimbus"));
    }

    struct Dropped;
    impl Read for Dropped {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(ErrorKind::ConnectionReset, "connection reset"))
        }
    }

    struct Full;
    impl Write for Full {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_download_that_breaks_off_is_a_connection_problem() {
        let mut out = Vec::new();
        copy_body(&b"save data"[..], &mut out).unwrap();
        assert_eq!(out, b"save data");

        // Part of the body arrives, then the connection drops.
        let err = copy_body((&b"save"[..]).chain(Dropped), &mut Vec::new()).unwrap_err();
        assert!(crate::cloud::is_unavailable(&err));

        // The local disk failing is not the cloud being unreachable.
        let err = copy_body(&b"save data"[..], &mut Full).unwrap_err();
        assert!(!crate::cloud::is_unavailable(&err));
    }
}
