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
//!
//! Finding a file by name is a request of its own, and so is asking whether
//! a folder is still there. What Drive said a moment ago is kept in a
//! [`Memo`], so that listing a folder and then reading or writing a few files
//! in it needs neither.

use super::oauth::{self, describe, GoogleClient};
use super::store::{Entry, Store};
use super::Unavailable;
use crate::{paths, util};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
/// What a listing asks Drive for about each file.
const FILE_FIELDS: &str = "id,name,size,mimeType,md5Checksum,createdTime,modifiedTime";
/// Largest content replaced with a simple upload, the kind Drive documents
/// for up to 5 MB. Anything bigger goes through an upload session.
const SIMPLE_UPLOAD_MAX: usize = 5 * 1024 * 1024;
/// What a create or an update asks Drive to answer with.
const WRITTEN_FIELDS: &str = "id,trashed";

/// How many more times a request turned away for the moment is sent.
const RETRIES: u32 = 2;
/// The longest wait before sending a request again. A server that asks for
/// longer is not kept waiting on: the error goes up, and the sync that made
/// the request tries again later.
const LONGEST_WAIT: Duration = Duration::from_secs(10);

/// How long to wait before sending a request again after try number
/// `attempt` (from 0) was turned away for the moment: what the server's
/// `Retry-After` asks for (seconds or a date), else 1 s and then 2 s. `None`
/// when it is not sent again: the tries are used up, or the server asks for
/// longer than [`LONGEST_WAIT`]. Shared with Baidu Netdisk.
pub(super) fn retry_wait(attempt: u32, retry_after: Option<&str>, now: DateTime<Utc>) -> Option<Duration> {
    if attempt >= RETRIES {
        return None;
    }
    let asked = retry_after.map(str::trim).and_then(|v| match v.parse::<u64>() {
        Ok(secs) => Some(Duration::from_secs(secs)),
        Err(_) => DateTime::parse_from_rfc2822(v).ok().map(|at| (at.with_timezone(&Utc) - now).to_std().unwrap_or_default()),
    });
    let wait = asked.unwrap_or(Duration::from_secs(1 << attempt));
    (wait <= LONGEST_WAIT).then_some(wait)
}

/// HTTP statuses that turn a request away for the moment: too many requests,
/// or a server failing. Shared with Baidu Netdisk.
pub(super) fn busy_status(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// Whether Drive turned a request away for the moment. Drive also says
/// "too many requests" with a 403 and a rate-limit reason; the daily limit
/// and the others in [`ORDINARY_403`] don't pass by waiting a few seconds.
fn busy(code: u16, body: &str) -> bool {
    if code != 403 {
        return busy_status(code);
    }
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    ["errors", "details"]
        .iter()
        .flat_map(|k| v["error"][*k].as_array().into_iter().flatten())
        .filter_map(|e| e["reason"].as_str())
        .any(|r| r.eq_ignore_ascii_case("rateLimitExceeded") || r.eq_ignore_ascii_case("userRateLimitExceeded"))
}

/// Gets a new access token and the seconds it lasts: from Google, or from
/// a stand-in in tests.
type Renew = Box<dyn Fn(&ureq::Agent) -> Result<(String, u64)> + Send + Sync>;

/// What a write sends: bytes already in memory, or a file that a big upload
/// reads as it goes, so a large content pack never sits in memory whole.
#[derive(Clone, Copy)]
enum Body<'a> {
    Bytes(&'a [u8]),
    /// The file and its length when the upload started.
    File(&'a Path, u64),
}

impl Body<'_> {
    fn len(&self) -> u64 {
        match self {
            Body::Bytes(b) => b.len() as u64,
            Body::File(_, len) => *len,
        }
    }

    /// The whole content, read from the file if need be (a small one).
    fn bytes(&self) -> Result<Cow<'_, [u8]>> {
        match self {
            Body::Bytes(b) => Ok(Cow::Borrowed(b)),
            Body::File(path, _) => Ok(Cow::Owned(std::fs::read(path).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", path.display()))?)),
        }
    }
}

/// How long the files Drive named in a folder are believed.
///
/// What this minute costs: two devices that create the same new name within
/// it make two files, because the second one does not look for the first.
/// That is what already happens when both look at the same moment.
/// [`DriveFile::beats`] says which of the two counts, and the next write
/// that looks for the name removes the other. Until its minute is over, a
/// device goes on reading and replacing the file it remembers.
const FILES_REMEMBERED: Duration = Duration::from_secs(60);

/// How long a folder that Drive showed alive is written into without asking
/// again.
///
/// What these ten minutes cost: a folder deleted or moved to the trash in
/// Drive within them is noticed by the write itself and not before it. Drive
/// answers 404 for a deleted folder, and says in its answer that a file
/// landed in the trash; either way the folder is looked up again and the
/// write is repeated once.
const ALIVE_REMEMBERED: Duration = Duration::from_secs(600);

/// What Drive told this store a moment ago, so that the next call about the
/// same folder need not ask again. It lives as long as the store (one sync,
/// one command of the app) and believes nothing for long. Every method is
/// given the time, so the rules are tested without waiting.
#[derive(Default)]
struct Memo {
    /// Folder id → when Drive last showed that the folder exists and is not
    /// in the trash.
    alive: HashMap<String, Instant>,
    /// Folder id → the files Drive named in it.
    folders: HashMap<String, Remembered>,
}

#[derive(Default)]
struct Remembered {
    /// When the whole folder was listed. Only such a listing can say that a
    /// name is not there.
    listed: Option<Instant>,
    /// File name → when Drive named it, and the ids of the files called
    /// that, the one that counts first.
    names: HashMap<String, (Instant, Vec<String>)>,
}

/// What a write does about the file it is to replace.
#[derive(Debug, PartialEq)]
enum Target {
    /// Replace this file. The others have the same name and are deleted.
    Update { current: String, others: Vec<String> },
    /// A listing of a moment ago has no file of this name: create it.
    Create,
    /// Nothing recent is known: look for the name first.
    Ask,
}

fn recent(at: Instant, now: Instant, within: Duration) -> bool {
    now.saturating_duration_since(at) < within
}

impl Memo {
    fn saw_alive(&mut self, folder: &str, now: Instant) {
        self.alive.insert(folder.to_string(), now);
    }

    fn is_alive(&self, folder: &str, now: Instant) -> bool {
        self.alive.get(folder).is_some_and(|at| recent(*at, now, ALIVE_REMEMBERED))
    }

    /// A complete listing of a folder. It replaces everything that was known
    /// about the folder's files.
    fn listed(&mut self, folder: &str, names: HashMap<String, Vec<String>>, now: Instant) {
        let names = names.into_iter().filter(|(_, ids)| !ids.is_empty()).map(|(name, ids)| (name, (now, ids))).collect();
        let known = Remembered { listed: Some(now), names };
        self.folders.insert(folder.to_string(), known);
    }

    /// The files called `name` as Drive just named them, or as a write left
    /// them: the one that counts first.
    fn named(&mut self, folder: &str, name: &str, ids: Vec<String>, now: Instant) {
        let known = self.folders.entry(folder.to_string()).or_default();
        if ids.is_empty() {
            known.names.remove(name);
        } else {
            known.names.insert(name.to_string(), (now, ids));
        }
    }

    /// The ids of the files called `name`, the one that counts first, when
    /// Drive named them within the last minute.
    ///
    /// `None` means "ask Drive" and never "there is no such file", also right
    /// after a complete listing: a caller that reads a record to merge it
    /// with what another device wrote a moment ago must get that record.
    fn ids(&self, folder: &str, name: &str, now: Instant) -> Option<Vec<String>> {
        let (at, ids) = self.folders.get(folder)?.names.get(name)?;
        recent(*at, now, FILES_REMEMBERED).then(|| ids.clone())
    }

    /// How to write `name`. Only a complete listing of the last minute
    /// decides it without asking, because only that can say the name is new.
    fn target(&self, folder: &str, name: &str, now: Instant) -> Target {
        let listed = self.folders.get(folder).and_then(|known| known.listed);
        if !listed.is_some_and(|at| recent(at, now, FILES_REMEMBERED)) {
            return Target::Ask;
        }
        match self.ids(folder, name, now) {
            Some(mut ids) => {
                let current = ids.remove(0);
                Target::Update { current, others: ids }
            }
            None => Target::Create,
        }
    }

    /// The files of this name were deleted.
    fn forget_name(&mut self, folder: &str, name: &str) {
        if let Some(known) = self.folders.get_mut(folder) {
            known.names.remove(name);
        }
    }

    /// Drive no longer has something that was remembered about this folder
    /// (404): nothing about it is believed any more.
    fn forget(&mut self, folder: &str) {
        self.alive.remove(folder);
        self.folders.remove(folder);
    }
}

pub struct GoogleDrive {
    folder: String,
    /// Which sign-in and folder name this is, see [`sign_in_key`].
    owner: String,
    /// Where requests go: [`API`] and [`UPLOAD`], a local server in tests.
    api: String,
    upload: String,
    renew: Renew,
    agent: ureq::Agent,
    bulk: ureq::Agent,
    /// The access token and when it runs out, by the wall clock: a
    /// monotonic clock stops while the PC sleeps (on Linux and macOS), and a
    /// store kept through a game session would go on using a dead token.
    token: Mutex<Option<(String, SystemTime)>>,
    /// Id of the app folder in My Drive, looked up once per process.
    root: Mutex<Option<String>>,
    /// Sub-folder path (e.g. `games/x/snapshots`) → id, below `root`.
    folders: Mutex<HashMap<String, String>>,
    /// The account's e-mail address (or name), once [`Store::quota`] has
    /// asked Drive for it.
    account: Mutex<Option<String>>,
    memo: Mutex<Memo>,
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
    std::fs::read(cache_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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

/// The ids of entries of one name and kind, the one that counts first.
fn current_first(mut same: Vec<&DriveFile>) -> Vec<String> {
    same.sort_by(|a, b| match (a.beats(b), b.beats(a)) {
        (true, _) => std::cmp::Ordering::Less,
        (_, true) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    same.into_iter().map(|f| f.id.clone()).collect()
}

/// The files of a listing for the [`Memo`]: name → ids, the one that counts
/// first. Folders are left out; their ids have a cache of their own.
fn ids_by_name(files: &[DriveFile]) -> HashMap<String, Vec<String>> {
    let mut by_name: HashMap<&str, Vec<&DriveFile>> = HashMap::new();
    for f in files.iter().filter(|f| !f.is_folder) {
        by_name.entry(f.name.as_str()).or_default().push(f);
    }
    by_name.into_iter().map(|(name, same)| (name.to_string(), current_first(same))).collect()
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
    let account = user["emailAddress"].as_str().or(user["displayName"].as_str()).filter(|s| !s.trim().is_empty()).map(str::to_string);
    (account, num("usage"), num("limit"))
}

/// Read a JSON answer. A body that stops half-way is the connection failing,
/// so it is reported like any other connection problem.
fn body_json(resp: ureq::Response) -> Result<Value> {
    resp.into_json().map_err(|e| anyhow!(Unavailable(e.to_string())))
}

/// A write did not reach a place readers find: Drive no longer has the file
/// or folder it named (404), or has the folder in its trash. It reads like
/// the error it stands for. `put` looks for it to ask Drive about the folder
/// again instead of trusting the [`Memo`].
#[derive(Debug)]
struct Gone(String);

impl std::fmt::Display for Gone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for Gone {}

fn is_gone(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Gone>().is_some())
}

/// [`drive_error`] for a create or an update: the same message, with a 404
/// marked as [`Gone`].
fn write_error(e: ureq::Error) -> anyhow::Error {
    let gone = matches!(e, ureq::Error::Status(404, _));
    let err = drive_error(e);
    if gone {
        anyhow!(Gone(err.to_string()))
    } else {
        err
    }
}

/// Whether a file just written is where readers find it. A folder in Drive's
/// trash still takes files, and they are in the trash with it; Drive says so
/// in its answer to the write.
///
/// The message is read only when the second try ends the same way, which
/// takes the app folder itself being in the trash. The next time the cloud is
/// opened it is looked up by name again, and a new one is made.
fn not_in_trash(written: &Value) -> Result<()> {
    if written["trashed"].as_bool() == Some(true) {
        return Err(anyhow!(Gone(crate::tr!(
            "Google Drive 上的文件夹在回收站里。请在 Google Drive 里恢复它，或者重试。",
            "The folder in Google Drive is in the trash. Restore it in Google Drive, or try again."
        ))));
    }
    Ok(())
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
    let reasons: Vec<&str> = ["errors", "details"].iter().flat_map(|k| err[*k].as_array().into_iter().flatten()).filter_map(|e| e["reason"].as_str()).collect();
    let has = |r: &str| reasons.iter().any(|x| x.eq_ignore_ascii_case(r));
    let lower = message.to_lowercase();
    if has("insufficientPermissions") || has("ACCESS_TOKEN_SCOPE_INSUFFICIENT") || lower.contains("insufficient authentication scopes") {
        return Some(Refusal::NoScope);
    }
    if has("domainPolicy") {
        return Some(Refusal::DomainPolicy);
    }
    let project_side = has("accessNotConfigured") || has("SERVICE_DISABLED");
    let no_drive_text = lower.contains("drive") && ["not enabled", "not activated", "signed up", "sign up"].iter().any(|p| lower.contains(p));
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
            folder,
            api: API.to_string(),
            upload: UPLOAD.to_string(),
            renew: Box::new(move |agent| oauth::refresh(agent, &client, &refresh_token)),
            agent: ureq::AgentBuilder::new().timeout_connect(timeout).timeout_read(timeout.max(Duration::from_secs(20))).build(),
            bulk: ureq::AgentBuilder::new().timeout_connect(timeout).timeout_read(Duration::from_secs(300)).timeout_write(Duration::from_secs(300)).build(),
            token: Mutex::new(None),
            root: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
            account: Mutex::new(None),
            memo: Mutex::new(Memo::default()),
        }
    }

    /// Send the requests to `api` and `upload` instead of Google's servers,
    /// and take access tokens from `renew` (tests).
    pub fn with_servers(mut self, api: &str, upload: &str, renew: impl Fn(&ureq::Agent) -> Result<(String, u64)> + Send + Sync + 'static) -> Self {
        self.api = api.trim_end_matches('/').to_string();
        self.upload = upload.trim_end_matches('/').to_string();
        self.renew = Box::new(renew);
        self
    }

    fn save_cache(&self) {
        let Some(root) = self.root.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        let cache = FolderCache { owner: self.owner.clone(), root, folders: self.folders.lock().unwrap_or_else(|e| e.into_inner()).clone() };
        if let Ok(bytes) = serde_json::to_vec_pretty(&cache) {
            let _ = util::atomic_write(&cache_file(), &bytes);
        }
    }

    /// Drop cached sub-folder ids (e.g. after the user deleted a folder).
    fn forget_folders(&self) {
        self.folders.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.save_cache();
    }

    /// Whether a folder still exists and is not in the trash.
    fn folder_alive(&self, id: &str) -> Result<bool> {
        let asked = Instant::now();
        let url = format!("{}/files/{id}?fields=trashed", self.api);
        let alive = match self.send(true, |auth| self.agent.get(&url).set("Authorization", auth).call())? {
            Ok(r) => !body_json(r)?["trashed"].as_bool().unwrap_or(false),
            Err(ureq::Error::Status(404, _)) => false,
            Err(e) => return Err(drive_error(e)),
        };
        let mut memo = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        if alive {
            memo.saw_alive(id, asked);
        } else {
            memo.forget(id);
        }
        Ok(alive)
    }

    /// Folder for writing: resolved (and created) with a liveness check so a
    /// trashed cached folder is never written into. The check is left out
    /// while the [`Memo`] has the folder as alive; `put` deals with a folder
    /// that went away since.
    fn writable_folder(&self, parts: &[&str]) -> Result<String> {
        let id = self.folder_id(parts, true)?.expect("created");
        let known_alive = self.memo.lock().unwrap_or_else(|e| e.into_inner()).is_alive(&id, Instant::now());
        if known_alive || self.folder_alive(&id)? {
            return Ok(id);
        }
        self.forget_folders();
        Ok(self.folder_id(parts, true)?.expect("created"))
    }

    /// The `Authorization` value, with a new access token when the one held
    /// runs out within a minute.
    fn bearer(&self) -> Result<String> {
        let held = self.token.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some((tok, until)) = held {
            if SystemTime::now() + Duration::from_secs(60) < until {
                return Ok(format!("Bearer {tok}"));
            }
        }
        // Not under the lock, so a slow answer from Google doesn't hold up
        // the requests of other threads that still have a token. Two threads
        // may renew at the same time; Google allows that.
        let (tok, secs) = (self.renew)(&self.agent)?;
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some((tok.clone(), SystemTime::now() + Duration::from_secs(secs)));
        Ok(format!("Bearer {tok}"))
    }

    /// Forget the access token behind `auth`, unless another thread has put
    /// a new one in its place already.
    fn drop_token(&self, auth: &str) {
        let mut held = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if held.as_ref().is_some_and(|(tok, _)| auth.strip_prefix("Bearer ") == Some(tok.as_str())) {
            *held = None;
        }
    }

    /// Send a request with the access token. `f` builds and sends it with
    /// the `Authorization` value it is given; the outer error is not getting
    /// a token at all.
    ///
    /// A 401 is a token that ran out or was revoked before this process
    /// noticed: a new one is taken and the request sent once more. Drive
    /// turns such a request away before doing anything, so that is safe for
    /// any request.
    ///
    /// With `again`, a request Drive turns away for the moment ([`busy`]) is
    /// also sent again, after the wait Drive asks for ([`retry_wait`]). Only
    /// for requests that do the same when they arrive twice: reads, deletes,
    /// replacing the content of a file, opening an upload session.
    fn send(&self, again: bool, f: impl Fn(&str) -> Result<ureq::Response, ureq::Error>) -> Result<Result<ureq::Response, ureq::Error>> {
        let mut renewed = false;
        let mut attempt = 0;
        loop {
            let auth = self.bearer()?;
            match f(&auth) {
                Err(ureq::Error::Status(401, _)) if !renewed => {
                    renewed = true;
                    self.drop_token(&auth);
                }
                Err(ureq::Error::Status(code, resp)) if again && (code == 403 || busy_status(code)) => {
                    let status = resp.status_text().to_string();
                    let retry_after = resp.header("Retry-After").map(str::to_string);
                    let text = resp.into_string().unwrap_or_default();
                    let wait = busy(code, &text).then(|| retry_wait(attempt, retry_after.as_deref(), Utc::now())).flatten();
                    match wait {
                        Some(wait) => {
                            attempt += 1;
                            std::thread::sleep(wait);
                        }
                        // The answer as it came, for the caller to read.
                        None => return Ok(ureq::Response::new(code, &status, &text).and_then(|r| Err(ureq::Error::Status(code, r)))),
                    }
                }
                done => return Ok(done),
            }
        }
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
            let url = url::Url::parse_with_params(&format!("{}/files", self.api), &params)?;
            let v = body_json(self.send(true, |auth| self.agent.get(url.as_str()).set("Authorization", auth).call())?.map_err(drive_error)?)?;
            out.extend(v["files"].as_array().into_iter().flatten().map(DriveFile::parse));
            match v["nextPageToken"].as_str() {
                Some(t) => page = Some(t.to_string()),
                None => break,
            }
        }
        Ok(out)
    }

    fn create_folder(&self, name: &str, parent: &str) -> Result<String> {
        // Not sent again when Drive is busy: a folder made by a request whose
        // answer was lost, and then made again, would be two folders, and
        // files written into the second one hide behind the first.
        let url = format!("{}/files?fields=id", self.api);
        let meta = json!({ "name": name, "mimeType": FOLDER_MIME, "parents": [parent] });
        let v = body_json(self.send(false, |auth| self.agent.post(&url).set("Authorization", auth).send_json(meta.clone()))?.map_err(drive_error)?)?;
        v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow!(crate::tr!("创建文件夹失败", "Couldn't create the folder")))
    }

    fn root_id(&self, create: bool) -> Result<Option<String>> {
        if let Some(r) = self.root.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(Some(r));
        }
        let q = format!("name='{}' and 'root' in parents and mimeType='{FOLDER_MIME}' and trashed=false", esc(&self.folder));
        let id = match pick(self.query(&q)?) {
            Some((f, _)) => f.id,
            None if create => self.create_folder(&self.folder, "root")?,
            None => return Ok(None),
        };
        let disk = read_cache();
        let same_root = disk.root == id;
        {
            let mut folders = self.folders.lock().unwrap_or_else(|e| e.into_inner());
            folders.clear();
            if same_root {
                folders.extend(disk.folders);
            }
        }
        *self.root.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
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
            if let Some(id) = self.folders.lock().unwrap_or_else(|e| e.into_inner()).get(&path).cloned() {
                parent = id;
                continue;
            }
            let q = format!("name='{}' and '{}' in parents and mimeType='{FOLDER_MIME}' and trashed=false", esc(name), esc(&parent));
            let id = match pick(self.query(&q)?) {
                Some((f, _)) => f.id,
                None if create => self.create_folder(name, &parent)?,
                None => return Ok(None),
            };
            self.folders.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), id.clone());
            self.save_cache();
            parent = id;
        }
        Ok(Some(parent))
    }

    /// Every file (not folder) called `name` directly in a folder: more than
    /// one when devices created it at the same moment.
    fn named(&self, parent: &str, name: &str) -> Result<Vec<DriveFile>> {
        let asked = Instant::now();
        let q = format!("name='{}' and '{}' in parents and trashed=false", esc(name), esc(parent));
        let files: Vec<DriveFile> = self.query(&q)?.into_iter().filter(|f| !f.is_folder).collect();
        let ids = current_first(files.iter().collect());
        let mut memo = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        // Only an answer with a file in it shows the folder alive: Drive
        // finds nothing in a folder that is in its trash.
        if !ids.is_empty() {
            memo.saw_alive(parent, asked);
        }
        memo.named(parent, name, ids, asked);
        Ok(files)
    }

    /// Start downloading the file a path stands for. `None` when there is no
    /// such file.
    fn fetch(&self, path: &str) -> Result<Option<ureq::Response>> {
        let (parts, name) = split_path(path);
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(None);
        };
        let known = self.memo.lock().unwrap_or_else(|e| e.into_inner()).ids(&folder, name, Instant::now());
        if let Some(id) = known.and_then(|ids| ids.into_iter().next()) {
            if let Some(resp) = self.media(&id)? {
                return Ok(Some(resp));
            }
            // Deleted since Drive named it. That says nothing about the
            // name, which may have a new file by now: ask.
            self.memo.lock().unwrap_or_else(|e| e.into_inner()).forget(&folder);
        }
        let Some((current, _)) = pick(self.named(&folder, name)?) else {
            return Ok(None);
        };
        self.media(&current.id)
    }

    /// Send `body` through an upload session: `start` opens it and one more
    /// request carries the bytes. The file appears (or changes) only once the
    /// last byte has arrived, at any size.
    ///
    /// A file is sent as it is read, with its length given up front. The
    /// request with the bytes is not sent again when it fails: the session
    /// may have taken part of them, and the whole write is repeated later.
    fn upload_session(&self, start: ureq::Request, meta: Value, mime: &str, body: Body) -> Result<Value> {
        let len = body.len().to_string();
        let resp = self
            .send(true, |auth| {
                start.clone().set("Authorization", auth).set("X-Upload-Content-Type", mime).set("X-Upload-Content-Length", &len).send_json(meta.clone())
            })?
            .map_err(write_error)?;
        let location =
            resp.header("Location").ok_or_else(|| anyhow!(crate::tr!("Google Drive 没有返回上传地址", "Google Drive returned no upload address")))?.to_string();
        let put = self.bulk.put(&location).set("Content-Type", mime);
        let sent = match body {
            Body::Bytes(b) => put.send_bytes(b),
            Body::File(path, size) => {
                let file = std::fs::File::open(path).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", path.display()))?;
                put.set("Content-Length", &len).send(file.take(size))
            }
        };
        body_json(sent.map_err(write_error)?)
    }

    /// Create a file and give its id.
    fn create_file(&self, parent: &str, name: &str, mime: &str, body: Body) -> Result<String> {
        let start = self.agent.post(&format!("{}/files?uploadType=resumable&fields={WRITTEN_FIELDS}", self.upload));
        let v = self.upload_session(start, json!({ "name": name, "parents": [parent] }), mime, body)?;
        not_in_trash(&v)?;
        v["id"].as_str().map(str::to_string).ok_or_else(|| anyhow!(crate::tr!("上传失败：没有返回文件 id", "Upload failed: no file id returned")))
    }

    fn update_content(&self, file_id: &str, mime: &str, body: Body) -> Result<()> {
        let written = if body.len() > SIMPLE_UPLOAD_MAX as u64 {
            let start = self.agent.request("PATCH", &format!("{}/files/{file_id}?uploadType=resumable&fields={WRITTEN_FIELDS}", self.upload));
            self.upload_session(start, json!({}), mime, body)?
        } else {
            // The same content sent twice is the same file: this one may be
            // sent again when Drive is busy.
            let bytes = body.bytes()?;
            let url = format!("{}/files/{file_id}?uploadType=media&fields={WRITTEN_FIELDS}", self.upload);
            body_json(
                self.send(true, |auth| self.bulk.request("PATCH", &url).set("Authorization", auth).set("Content-Type", mime).send_bytes(&bytes))?
                    .map_err(write_error)?,
            )?
        };
        not_in_trash(&written)
    }

    /// Start downloading a file's content. `None` when the file is gone.
    fn media(&self, file_id: &str) -> Result<Option<ureq::Response>> {
        let url = format!("{}/files/{file_id}?alt=media", self.api);
        match self.send(true, |auth| self.bulk.get(&url).set("Authorization", auth).call())? {
            Ok(r) => Ok(Some(r)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(drive_error(e)),
        }
    }

    /// Delete a file. `false` when Drive no longer had it.
    fn delete_file(&self, file_id: &str) -> Result<bool> {
        let url = format!("{}/files/{file_id}", self.api);
        match self.send(true, |auth| self.agent.delete(&url).set("Authorization", auth).call())? {
            Ok(_) => Ok(true),
            // Already gone (another device deleted it): that is what was asked for.
            Err(ureq::Error::Status(404, _)) => Ok(false),
            Err(e) => Err(drive_error(e)),
        }
    }

    /// Create the file a path stands for, or replace its content.
    fn put(&self, path: &str, body: Body) -> Result<()> {
        match self.put_once(path, body) {
            // Drive no longer has the folder or the file this write went to.
            // What was remembered about the folder is forgotten by now, so
            // this time Drive is asked about both, and the folder is made
            // again if need be.
            Err(e) if is_gone(&e) => self.put_once(path, body),
            done => done,
        }
    }

    fn put_once(&self, path: &str, body: Body) -> Result<()> {
        let (parts, name) = split_path(path);
        let folder = self.writable_folder(&parts)?;
        let mime = mime_for(name);
        let started = Instant::now();
        let target = self.memo.lock().unwrap_or_else(|e| e.into_inner()).target(&folder, name, started);
        let (current, others) = match target {
            Target::Update { current, others } => (Some(current), others),
            Target::Create => (None, Vec::new()),
            Target::Ask => match pick(self.named(&folder, name)?) {
                Some((current, others)) => (Some(current.id), others.into_iter().map(|f| f.id).collect()),
                None => (None, Vec::new()),
            },
        };
        let written = match current {
            Some(id) => self.update_content(&id, mime, body).map(|()| id),
            None => self.create_file(&folder, name, mime, body),
        };
        let id = match written {
            Ok(id) => id,
            Err(e) => {
                if is_gone(&e) {
                    self.memo.lock().unwrap_or_else(|e| e.into_inner()).forget(&folder);
                }
                return Err(e);
            }
        };
        // The file just written is now the newest of its name, so readers
        // already get it; a duplicate that can't be deleted now stays
        // remembered and goes with the next write.
        let mut ids = vec![id];
        for other in others {
            if self.delete_file(&other).is_err() {
                ids.push(other);
            }
        }
        let mut memo = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        memo.saw_alive(&folder, started);
        memo.named(&folder, name, ids, started);
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
/// token is fresh. The [`Memo`] decides which row applies: it knows a file's
/// id for a minute after its folder was listed, its name was looked for or it
/// was written, and it knows a folder as alive for ten minutes after a
/// listing or a search found something in it, a file was written into it, or
/// Drive was asked.
///
/// | Call | Requests |
/// |---|---|
/// | `list` | 1 per 1000 entries |
/// | `read`, `download`: id known | 1 |
/// | `read`, `download`: otherwise | 2 (find the file by name, fetch it) |
/// | `write`, `upload`: folder listed within the minute, name in the listing | 1 |
/// | `write`, `upload`: folder listed within the minute, name not in it | 2 (a create is always an upload session) |
/// | `write`, `upload`: no such listing | 2 to replace (find, send), 3 to create |
/// | `remove`: ids known | 1 per file of that name |
/// | `remove`: otherwise | 2 |
/// | `check`, `quota` | 1 each |
///
/// A write costs one more when the folder is not known as alive (ask Drive
/// first), and one more when it replaces a file with more than
/// [`SIMPLE_UPLOAD_MAX`]. When something that was remembered turns out to be
/// gone, the call starts again with nothing remembered: the request that
/// found out is the extra cost. A file sent into a folder that turned out to
/// be in the trash is sent a second time.
impl Store for GoogleDrive {
    fn kind(&self) -> &'static str {
        "googleDrive"
    }

    fn label(&self) -> String {
        // The account once Drive has named it, so Settings shows who is
        // signed in: `Repo::account` asks for `quota` before the label.
        let account = self.account.lock().unwrap_or_else(|e| e.into_inner()).clone();
        account.unwrap_or_else(|| format!("Google Drive · {}", self.folder))
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        let url = format!("{}/about?fields=user(emailAddress,displayName),storageQuota(limit,usage)", self.api);
        let v = body_json(self.send(true, |auth| self.agent.get(&url).set("Authorization", auth).call())?.map_err(drive_error)?)?;
        let (account, used, total) = parse_about(&v);
        if account.is_some() {
            *self.account.lock().unwrap_or_else(|e| e.into_inner()) = account;
        }
        Ok((used, total))
    }

    fn check(&self) -> Result<()> {
        // Ask Drive even when the folder is already known to this process:
        // this is the test of whether the cloud can be reached.
        let known = self.root.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(id) = known {
            if self.folder_alive(&id)? {
                return Ok(());
            }
            // Deleted in Drive meanwhile: look it up, or create it, again.
            *self.root.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.folders.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
        self.root_id(true).map(|_| ())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(Vec::new());
        };
        let asked = Instant::now();
        let q = format!("'{}' in parents and trashed=false", esc(&folder));
        let files = self.query(&q)?;
        {
            let mut memo = self.memo.lock().unwrap_or_else(|e| e.into_inner());
            // An empty listing does not show the folder alive: that is also
            // what Drive answers for a folder in its trash.
            if !files.is_empty() {
                memo.saw_alive(&folder, asked);
            }
            memo.listed(&folder, ids_by_name(&files), asked);
        }
        Ok(one_per_name(files).into_iter().map(|f| Entry { name: f.name, is_dir: f.is_folder, size: f.size, tag: f.tag }).collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(resp) = self.fetch(path)? else {
            return Ok(None);
        };
        let mut buf = Vec::new();
        copy_body(resp.into_reader(), &mut buf)?;
        Ok(Some(buf))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let resp = self.fetch(path)?.ok_or_else(|| anyhow!(crate::tr!("云端找不到 {path}", "{path} wasn't found in the cloud")))?;
        let tmp = dest.with_extension("lntmp");
        let mut out = std::fs::File::create(&tmp)?;
        copy_body(resp.into_reader(), &mut out)?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        self.put(path, Body::Bytes(data))
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        // A file that goes up through an upload session is read as it is
        // sent: a content pack can be as big as the biggest save file in it.
        // A small one is read whole, as a simple upload needs. An upload that
        // breaks off starts again from the beginning next time.
        let unreadable = || crate::tr!("无法读取 {}", "Can't read {}", src.display());
        let len = std::fs::metadata(src).with_context(unreadable)?.len();
        if len <= SIMPLE_UPLOAD_MAX as u64 {
            let data = std::fs::read(src).with_context(unreadable)?;
            return self.put(path, Body::Bytes(&data));
        }
        self.put(path, Body::File(src, len))
    }

    fn remove(&self, path: &str) -> Result<()> {
        let (parts, name) = split_path(path);
        let Some(folder) = self.folder_id(&parts, false)? else {
            return Ok(());
        };
        let known = self.memo.lock().unwrap_or_else(|e| e.into_inner()).ids(&folder, name, Instant::now());
        if let Some(ids) = known {
            let mut all_there = true;
            for id in &ids {
                all_there &= self.delete_file(id)?;
            }
            if all_there {
                self.memo.lock().unwrap_or_else(|e| e.into_inner()).forget_name(&folder, name);
                return Ok(());
            }
            // One of them was gone already, so something else has changed
            // this name since Drive named its files: ask what is there now.
            self.memo.lock().unwrap_or_else(|e| e.into_inner()).forget(&folder);
        }
        // Every file of that name: a duplicate left behind would turn up as
        // the file again.
        for f in self.named(&folder, name)? {
            self.delete_file(&f.id)?;
        }
        self.memo.lock().unwrap_or_else(|e| e.into_inner()).forget_name(&folder, name);
        Ok(())
    }

    fn cache_scope(&self) -> Option<String> {
        // The app folder's id, once this process has looked it up, or from
        // the folder cache a previous one left. Right after signing in there
        // is none until the first request; `Cached` asks again after each
        // listing until there is.
        let resolved = self.root.lock().unwrap_or_else(|e| e.into_inner()).clone();
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
        json!({ "error": { "code": 403, "message": message, "errors": [{ "domain": "global", "reason": reason, "message": message }] } }).to_string()
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
        for listed in
            [vec![old.clone(), new.clone(), older.clone()], vec![new.clone(), older.clone(), old.clone()], vec![older.clone(), old.clone(), new.clone()]]
        {
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
        let client = GoogleClient { id: "client-id".into(), secret: String::new() };
        GoogleDrive::new(client, refresh_token.into(), folder.into(), Duration::from_secs(5))
    }

    #[test]
    fn a_token_runs_out_by_the_wall_clock() {
        let renewed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = renewed.clone();
        // No request is made: only the token is looked at.
        let d = drive("token", "Ludonimbus").with_servers("http://127.0.0.1:9", "http://127.0.0.1:9", move |_| {
            let n = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Ok((format!("new-{n}"), 3600))
        });
        let renewals = || renewed.load(std::sync::atomic::Ordering::SeqCst);
        let hold = |tok: &str, until: SystemTime| *d.token.lock().unwrap() = Some((tok.to_string(), until));

        hold("old", SystemTime::now() + secs(600));
        assert_eq!(d.bearer().unwrap(), "Bearer old");
        assert_eq!(renewals(), 0);
        // It ran out while the PC slept: the wall clock says so, where a
        // monotonic clock would not have moved.
        hold("old", SystemTime::now() - secs(1));
        assert_eq!(d.bearer().unwrap(), "Bearer new-1");
        assert_eq!(d.bearer().unwrap(), "Bearer new-1");
        assert_eq!(renewals(), 1);
        // Within a minute of running out counts as run out.
        hold("old", SystemTime::now() + secs(30));
        assert_eq!(d.bearer().unwrap(), "Bearer new-2");

        // A 401 forgets the token it was sent with, not one renewed since.
        d.drop_token("Bearer old");
        assert_eq!(d.bearer().unwrap(), "Bearer new-2");
        d.drop_token("Bearer new-2");
        assert_eq!(d.bearer().unwrap(), "Bearer new-3");
        assert_eq!(renewals(), 3);
    }

    #[test]
    fn a_request_turned_away_for_the_moment_waits_as_asked() {
        let now = time(&json!("2026-11-27T10:00:00.000Z")).unwrap();
        assert_eq!(retry_wait(0, None, now), Some(secs(1)));
        assert_eq!(retry_wait(1, None, now), Some(secs(2)));
        assert_eq!(retry_wait(2, None, now), None);
        assert_eq!(retry_wait(0, Some("3"), now), Some(secs(3)));
        assert_eq!(retry_wait(1, Some(" 0 "), now), Some(secs(0)));
        assert_eq!(retry_wait(2, Some("0"), now), None);
        // Longer than is worth waiting for here: the sync tries later.
        assert_eq!(retry_wait(0, Some("120"), now), None);
        // A date, also one already past; something unreadable is the usual wait.
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 10:00:05 GMT"), now), Some(secs(5)));
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 09:59:00 GMT"), now), Some(secs(0)));
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 11:00:00 GMT"), now), None);
        assert_eq!(retry_wait(0, Some("soon"), now), Some(secs(1)));

        assert!(busy(429, ""));
        assert!(busy(503, "<html>"));
        assert!(busy(500, ""));
        assert!(!busy(404, ""));
        assert!(!busy(401, ""));
        assert!(busy(403, &body("userRateLimitExceeded", "User Rate Limit Exceeded")));
        assert!(busy(403, &body("rateLimitExceeded", "Rate Limit Exceeded")));
        assert!(!busy(403, &body("dailyLimitExceeded", "Daily Limit Exceeded")));
        assert!(!busy(403, &body("storageQuotaExceeded", "The user's Drive storage quota has been exceeded.")));
        assert!(!busy(403, "nope"));
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
        let cache = FolderCache { owner: mine.owner.clone(), root: "1RootFolderId".into(), folders: HashMap::new() };
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

    const LINE: &str = "1LineFolderId";
    const PACKS: &str = "1PacksFolderId";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn names(listing: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        listing.iter().map(|(name, ids)| (name.to_string(), ids.iter().map(|id| id.to_string()).collect())).collect()
    }

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn update(current: &str, others: &[&str]) -> Target {
        Target::Update { current: current.into(), others: strings(others) }
    }

    #[test]
    fn a_listing_gives_ids_in_the_order_that_counts() {
        let listed = vec![
            file("1", "index.json", "2026-11-26T10:00:00.000Z"),
            file("2", "01A.json", "2026-11-26T09:00:00.000Z"),
            file("3", "index.json", "2026-11-26T10:00:05.000Z"),
            folder("4", "thumbnails", "2026-11-01T00:00:00.000Z"),
            file("5", "index.json", "2026-11-25T10:00:00.000Z"),
        ];
        // The first id is the file `pick` chooses; folders are not files.
        let expected = names(&[("index.json", &["3", "1", "5"]), ("01A.json", &["2"])]);
        assert_eq!(ids_by_name(&listed), expected);
        let same: Vec<DriveFile> = listed.iter().filter(|f| f.name == "index.json").cloned().collect();
        assert_eq!(pick(same).unwrap().0.id, "3");
        assert!(ids_by_name(&[]).is_empty());
    }

    #[test]
    fn a_listed_file_is_known_for_a_minute() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.listed(LINE, names(&[("index.json", &["1"])]), t0);

        assert_eq!(memo.ids(LINE, "index.json", t0), Some(strings(&["1"])));
        assert_eq!(memo.ids(LINE, "index.json", t0 + secs(59)), Some(strings(&["1"])));
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(59)), update("1", &[]));
        assert_eq!(memo.target(LINE, "01B.json", t0 + secs(59)), Target::Create);

        // After the minute nothing is taken from it: Drive is asked.
        assert_eq!(memo.ids(LINE, "index.json", t0 + secs(60)), None);
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(60)), Target::Ask);
        assert_eq!(memo.target(LINE, "01B.json", t0 + secs(60)), Target::Ask);

        // Another folder was never listed.
        assert_eq!(memo.ids(PACKS, "index.json", t0), None);
        assert_eq!(memo.target(PACKS, "index.json", t0), Target::Ask);
    }

    #[test]
    fn a_listing_replaces_what_was_known() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.named(LINE, "01A.json", strings(&["1"]), t0);
        memo.named(LINE, "index.json", strings(&["2"]), t0);

        memo.listed(LINE, names(&[("index.json", &["3", "2"])]), t0 + secs(5));
        // Not in the listing any more, although it was named 5 seconds ago.
        assert_eq!(memo.ids(LINE, "01A.json", t0 + secs(6)), None);
        assert_eq!(memo.target(LINE, "01A.json", t0 + secs(6)), Target::Create);
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(6)), update("3", &["2"]));

        // The next listing counts from its own time, and an empty one says
        // that every name is new.
        memo.listed(LINE, HashMap::new(), t0 + secs(50));
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(100)), Target::Create);
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(110)), Target::Ask);
    }

    #[test]
    fn a_search_for_a_name_does_not_say_which_names_are_new() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.named(PACKS, "01A.zip", strings(&["1"]), t0);
        // The file can be read by id, but a write looks first: without a
        // listing nothing says what else is in the folder.
        assert_eq!(memo.ids(PACKS, "01A.zip", t0 + secs(1)), Some(strings(&["1"])));
        assert_eq!(memo.target(PACKS, "01A.zip", t0 + secs(1)), Target::Ask);
        assert_eq!(memo.target(PACKS, "01B.zip", t0 + secs(1)), Target::Ask);
        assert_eq!(memo.ids(PACKS, "01A.zip", t0 + secs(60)), None);

        // A search that finds nothing leaves no entry.
        memo.named(PACKS, "01A.zip", Vec::new(), t0 + secs(2));
        assert_eq!(memo.ids(PACKS, "01A.zip", t0 + secs(2)), None);
    }

    #[test]
    fn a_write_is_remembered() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.listed(LINE, names(&[("index.json", &["2", "1"])]), t0);

        // A new file: its id is known from the answer to the create.
        memo.named(LINE, "01B.json", strings(&["9"]), t0 + secs(1));
        assert_eq!(memo.ids(LINE, "01B.json", t0 + secs(2)), Some(strings(&["9"])));
        assert_eq!(memo.target(LINE, "01B.json", t0 + secs(2)), update("9", &[]));

        // A replaced file whose duplicate was deleted on the way.
        memo.named(LINE, "index.json", strings(&["2"]), t0 + secs(3));
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(4)), update("2", &[]));

        // The id is known for a minute after the write, also when the
        // listing is too old to say how to write.
        assert_eq!(memo.ids(LINE, "01B.json", t0 + secs(60)), Some(strings(&["9"])));
        assert_eq!(memo.target(LINE, "01B.json", t0 + secs(60)), Target::Ask);
        assert_eq!(memo.ids(LINE, "01B.json", t0 + secs(61)), None);
    }

    #[test]
    fn a_removed_name_is_new_again() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.listed(LINE, names(&[("index.json", &["1"]), ("01A.json", &["2"])]), t0);
        memo.forget_name(LINE, "01A.json");
        assert_eq!(memo.ids(LINE, "01A.json", t0 + secs(1)), None);
        assert_eq!(memo.target(LINE, "01A.json", t0 + secs(1)), Target::Create);
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(1)), update("1", &[]));
        // Nothing known about the folder: nothing to forget.
        memo.forget_name(PACKS, "01A.zip");
        assert_eq!(memo.target(PACKS, "01A.zip", t0 + secs(1)), Target::Ask);
    }

    #[test]
    fn a_404_forgets_the_folder() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        for folder in [LINE, PACKS] {
            memo.saw_alive(folder, t0);
            memo.listed(folder, names(&[("index.json", &["1"])]), t0);
        }
        memo.forget(LINE);
        assert!(!memo.is_alive(LINE, t0 + secs(1)));
        assert_eq!(memo.ids(LINE, "index.json", t0 + secs(1)), None);
        // Not "create": the listing that could say so is forgotten too.
        assert_eq!(memo.target(LINE, "index.json", t0 + secs(1)), Target::Ask);
        assert_eq!(memo.target(LINE, "01B.json", t0 + secs(1)), Target::Ask);
        // What is known about other folders stays.
        assert!(memo.is_alive(PACKS, t0 + secs(1)));
        assert_eq!(memo.target(PACKS, "index.json", t0 + secs(1)), update("1", &[]));
    }

    #[test]
    fn a_name_missing_from_a_listing_is_still_asked_for_on_a_read() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        memo.listed(LINE, names(&[("index.json", &["1"])]), t0);
        // A write may take the listing's word that the name is new. A read
        // gets no answer from it: another device may have written the file
        // since, so `fetch` asks Drive.
        assert_eq!(memo.target(LINE, "game.json", t0 + secs(1)), Target::Create);
        assert_eq!(memo.ids(LINE, "game.json", t0 + secs(1)), None);
        memo.listed(PACKS, HashMap::new(), t0);
        assert_eq!(memo.ids(PACKS, "01A.zip", t0), None);
    }

    #[test]
    fn a_folder_is_known_alive_for_ten_minutes() {
        let t0 = Instant::now();
        let mut memo = Memo::default();
        assert!(!memo.is_alive(LINE, t0));
        memo.saw_alive(LINE, t0);
        assert!(memo.is_alive(LINE, t0));
        assert!(memo.is_alive(LINE, t0 + secs(599)));
        assert!(!memo.is_alive(LINE, t0 + secs(600)));
        assert!(!memo.is_alive(PACKS, t0));

        // Seen again: ten minutes from then.
        memo.saw_alive(LINE, t0 + secs(500));
        assert!(memo.is_alive(LINE, t0 + secs(1099)));
        assert!(!memo.is_alive(LINE, t0 + secs(1100)));

        // Listing a folder does not by itself show it alive: `list` says so
        // only when Drive found something in it.
        memo.listed(PACKS, HashMap::new(), t0);
        assert!(!memo.is_alive(PACKS, t0));
    }

    /// An answer from Drive with an error status, built without a request.
    fn status(code: u16, reason: &str, message: &str) -> ureq::Error {
        let body = json!({ "error": { "code": code, "message": message, "errors": [{ "reason": reason }] } });
        ureq::Error::Status(code, ureq::Response::new(code, "Error", &body.to_string()).unwrap())
    }

    #[test]
    fn a_write_that_drive_answers_with_404_is_marked() {
        let gone = write_error(status(404, "notFound", "File not found: 1LineFolderId."));
        assert!(is_gone(&gone));
        // It reads like any other answer from Drive.
        assert!(gone.to_string().contains("404"));
        assert!(gone.to_string().contains("File not found: 1LineFolderId."));
        // Found also when a caller has said what it was doing.
        assert!(is_gone(&gone.context("upload")));

        let full = write_error(status(403, "storageQuotaExceeded", "The Drive storage quota has been exceeded."));
        assert!(!is_gone(&full));
        let down = write_error(status(503, "backendError", "Backend Error"));
        assert!(!is_gone(&down));
        assert!(crate::cloud::is_unavailable(&down));
        // The same answer to a request that is not a write is not marked.
        assert!(!is_gone(&drive_error(status(404, "notFound", "File not found: 1LineFolderId."))));
    }

    #[test]
    fn a_file_written_into_the_trash_is_not_written() {
        let err = not_in_trash(&json!({ "id": "1", "trashed": true })).unwrap_err();
        assert!(is_gone(&err));
        assert!(not_in_trash(&json!({ "id": "1", "trashed": false })).is_ok());
        assert!(not_in_trash(&json!({ "id": "1" })).is_ok());
    }
}
