//! WebDAV (RFC 4918): NAS (Synology, QNAP…), Nextcloud / ownCloud,
//! Jianguoyun, OpenList / AList (which bridge many Chinese netdisks) and
//! `rclone serve webdav`.
//!
//! Only plain HTTP verbs are used: `PROPFIND` (depth 0/1), `MKCOL`, `PUT`,
//! `GET`, `MOVE` and `DELETE`, with Basic authentication. Uploads go to a
//! temporary name and are moved into place, so a half-uploaded file is never
//! visible under its real name. Servers that refuse `MOVE` get a direct `PUT`.

use super::store::{check_path, is_safe_name, temp_name, Entry, Store};
use super::Unavailable;
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

/// `LUDONIMBUS_DAV_TRACE=1` prints every request with its status and
/// duration to stderr, to see where a slow server spends its time.
fn trace<T>(method: &str, url: &Url, started: Instant, res: &Result<T, ureq::Error>) {
    if std::env::var_os("LUDONIMBUS_DAV_TRACE").is_none() {
        return;
    }
    let status = match res {
        Ok(_) => "ok".to_string(),
        Err(ureq::Error::Status(c, _)) => c.to_string(),
        Err(_) => "err".to_string(),
    };
    eprintln!("DAV {method} {status} {}ms {}", started.elapsed().as_millis(), url.path());
}

const PROPFIND_LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getetag/><d:getlastmodified/></d:prop></d:propfind>"#;
const PROPFIND_QUOTA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:quota-used-bytes/><d:quota-available-bytes/></d:prop></d:propfind>"#;

pub struct WebDav {
    /// Server URL as entered by the user (always ends with `/`).
    server: Url,
    /// Folder below `server` holding the Ludonimbus data.
    folder: Vec<String>,
    username: String,
    auth: String,
    agent: ureq::Agent,
    bulk: ureq::Agent,
    /// Folders known to exist (relative paths, `""` = root folder).
    known_dirs: Mutex<HashSet<String>>,
    /// Upload to a temporary name and rename it (see [`WebDav::direct_uploads`]).
    atomic: bool,
}

/// One `<response>` of a multistatus document.
#[derive(Debug, Clone, PartialEq)]
pub struct DavItem {
    /// Decoded URL path, e.g. `/dav/Ludonimbus/games/`.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub etag: String,
    pub modified: String,
    pub quota_used: Option<u64>,
    pub quota_available: Option<u64>,
}

enum Body<'a> {
    Bytes(&'a [u8]),
    File(&'a Path),
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

/// Parse a `207 Multi-Status` body.
pub fn parse_multistatus(xml: &str) -> Result<Vec<DavItem>> {
    let doc = roxmltree::Document::parse(xml).context(crate::tr!("WebDAV 服务器返回的不是有效的 XML", "The WebDAV server returned invalid XML"))?;
    let dav = |n: &roxmltree::Node, name: &str| n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some("DAV:");
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| dav(n, "response")) {
        let Some(href) = resp.children().find(|n| dav(n, "href")).and_then(|n| n.text()) else {
            continue;
        };
        let href = href.trim();
        let raw_path = match Url::parse(href) {
            Ok(u) => u.path().to_string(),
            Err(_) => href.to_string(),
        };
        let mut item = DavItem {
            path: decode(&raw_path),
            is_dir: raw_path.ends_with('/'),
            size: 0,
            etag: String::new(),
            modified: String::new(),
            quota_used: None,
            quota_available: None,
        };
        for ps in resp.children().filter(|n| dav(n, "propstat")) {
            let ok = ps.children().find(|n| dav(n, "status")).and_then(|n| n.text()).is_none_or(|s| s.contains(" 200"));
            if !ok {
                continue;
            }
            let Some(prop) = ps.children().find(|n| dav(n, "prop")) else { continue };
            for p in prop.children().filter(|n| n.is_element()) {
                let text = p.text().unwrap_or("").trim();
                match p.tag_name().name() {
                    "resourcetype" => item.is_dir = p.children().any(|c| dav(&c, "collection")),
                    "getcontentlength" => item.size = text.parse().unwrap_or(0),
                    "getetag" => item.etag = text.to_string(),
                    "getlastmodified" => item.modified = text.to_string(),
                    "quota-used-bytes" => item.quota_used = text.parse().ok(),
                    "quota-available-bytes" => item.quota_available = text.parse().ok(),
                    _ => {}
                }
            }
        }
        out.push(item);
    }
    Ok(out)
}

/// Turn a ureq error into a user-facing message; network problems and
/// server-side failures become [`Unavailable`].
fn fail(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let body: String = resp.into_string().unwrap_or_default().chars().take(300).collect();
            match code {
                401 => anyhow!(crate::tr!("WebDAV 用户名或密码不对（401）", "Wrong WebDAV username or password (401)")),
                403 => anyhow!(crate::tr!(
                    "WebDAV 服务器拒绝了操作（403），请检查账号是否有这个文件夹的读写权限",
                    "The WebDAV server refused (403). Check that the account can read and write this folder"
                )),
                404 => {
                    anyhow!(crate::tr!("WebDAV 地址不存在（404），请检查地址和文件夹", "The WebDAV address doesn't exist (404). Check the address and folder"))
                }
                405 => anyhow!(crate::tr!(
                    "WebDAV 服务器不支持这个操作（405），请确认地址指向的是 WebDAV 服务",
                    "The WebDAV server doesn't support this (405). Make sure the address points to a WebDAV service"
                )),
                507 => anyhow!(crate::tr!("WebDAV 空间不足（507）", "Not enough space on the WebDAV server (507)")),
                429 | 503 => {
                    anyhow!(Unavailable(crate::tr!("WebDAV 服务器繁忙或请求太频繁（{code}）", "The WebDAV server is busy or got too many requests ({code})")))
                }
                c if c >= 500 && body.trim().is_empty() => anyhow!(Unavailable(crate::tr!("WebDAV 服务器错误 {c}", "WebDAV server error {c}"))),
                c if c >= 500 => anyhow!(Unavailable(crate::tr!("WebDAV 服务器错误 {c}：{}", "WebDAV server error {c}: {}", body.trim()))),
                c => anyhow!(crate::tr!("WebDAV 返回错误 {c}：{body}", "WebDAV returned error {c}: {body}")),
            }
        }
        ureq::Error::Transport(t) => {
            let msg = t.to_string();
            let lower = msg.to_lowercase();
            if lower.contains("certificate") || lower.contains("unknownissuer") {
                anyhow!(crate::tr!("无法验证 WebDAV 服务器的 HTTPS 证书（可能是自签名证书）。请给服务器配置受信任的证书（例如 Let's Encrypt），或在局域网内改用 http:// 地址。详细信息：{msg}", "Couldn't verify the WebDAV server's HTTPS certificate (it may be self-signed). Give the server a trusted certificate (for example Let's Encrypt), or use an http:// address on your local network. Details: {msg}"))
            } else {
                anyhow!(Unavailable(msg))
            }
        }
    }
}

impl WebDav {
    pub fn new(url: &str, username: &str, password: &str, folder: &str, timeout: Duration) -> Result<Self> {
        let mut server = Url::parse(url.trim()).map_err(|_| {
            anyhow!(crate::tr!(
                "WebDAV 地址格式不对，应类似 https://example.com/dav/",
                "The WebDAV address looks wrong; it should look like https://example.com/dav/"
            ))
        })?;
        if !matches!(server.scheme(), "http" | "https") {
            bail!(crate::tr!("WebDAV 地址需要以 http:// 或 https:// 开头", "The WebDAV address needs to start with http:// or https://"));
        }
        if server.host_str().is_none() {
            bail!(crate::tr!("WebDAV 地址缺少服务器名", "The WebDAV address has no server name"));
        }
        server.set_query(None);
        server.set_fragment(None);
        if !server.path().ends_with('/') {
            let p = format!("{}/", server.path());
            server.set_path(&p);
        }
        let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}")));
        Ok(WebDav {
            server,
            folder: segments(folder).map(str::to_string).collect(),
            username: username.to_string(),
            auth,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(timeout.max(Duration::from_secs(20)))
                .timeout_write(timeout.max(Duration::from_secs(20)))
                .build(),
            bulk: ureq::AgentBuilder::new().timeout_connect(timeout).timeout_read(Duration::from_secs(300)).timeout_write(Duration::from_secs(300)).build(),
            known_dirs: Mutex::new(HashSet::new()),
            atomic: true,
        })
    }

    /// Upload straight to the final name instead of a temporary file that
    /// is renamed afterwards. Only for servers whose uploads are atomic
    /// themselves (OpenList in front of Quark: a file only appears once its
    /// upload has finished), where the extra rename is slow.
    pub fn direct_uploads(mut self) -> Self {
        self.atomic = false;
        self
    }

    /// URL of a path below the data folder (`dir` adds a trailing slash).
    /// Each segment is escaped (`/`, `\`, `%` too) and `.` or `..` is
    /// dropped, so no URL leads out of the folder; the [`Store`] methods
    /// refuse such paths before they get here ([`check_path`]), rather than
    /// act on another file than the one asked for.
    fn url(&self, path: &str, dir: bool) -> Url {
        let mut u = self.server.clone();
        {
            let mut segs = u.path_segments_mut().expect("http url");
            segs.pop_if_empty();
            for s in self.folder.iter().map(String::as_str).chain(segments(path)) {
                segs.push(s);
            }
            if dir {
                segs.push("");
            }
        }
        u
    }

    fn req(&self, agent: &ureq::Agent, method: &str, url: &Url) -> ureq::Request {
        agent.request_url(method, url).set("Authorization", &self.auth)
    }

    fn propfind(&self, url: &Url, depth: &str, body: &str) -> Result<Option<Vec<DavItem>>> {
        let t = Instant::now();
        let res = self.req(&self.agent, "PROPFIND", url).set("Depth", depth).set("Content-Type", "application/xml; charset=utf-8").send_string(body);
        trace("PROPFIND", url, t, &res);
        match res {
            Ok(r) => Ok(Some(parse_multistatus(&r.into_string()?)?)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(fail(e)),
        }
    }

    fn mkcol(&self, url: &Url) -> Result<()> {
        let t = Instant::now();
        let res = self.req(&self.agent, "MKCOL", url).call();
        trace("MKCOL", url, t, &res);
        match res {
            Ok(_) => Ok(()),
            // 405: already exists. Some servers answer 301 (redirect to the
            // existing folder) or 409 when it exists as well.
            Err(ureq::Error::Status(301 | 405, _)) => Ok(()),
            Err(e) => Err(fail(e)),
        }
    }

    /// Create the data folder (and the folders above it inside the server
    /// URL) when missing.
    fn ensure_root(&self) -> Result<()> {
        if self.known_dirs.lock().unwrap().contains("") {
            return Ok(());
        }
        let root = self.url("", true);
        if self.propfind(&root, "0", PROPFIND_LIST)?.is_none() {
            let mut u = self.server.clone();
            for seg in &self.folder {
                u.path_segments_mut().expect("http url").pop_if_empty().push(seg).push("");
                self.mkcol(&u)?;
            }
            if self.propfind(&root, "0", PROPFIND_LIST)?.is_none() {
                bail!(crate::tr!("无法在 WebDAV 服务器上创建文件夹 {}", "Couldn't create the folder {} on the WebDAV server", self.folder.join("/")));
            }
        }
        self.known_dirs.lock().unwrap().insert(String::new());
        Ok(())
    }

    /// Create every folder above `path`.
    fn ensure_parents(&self, path: &str) -> Result<()> {
        self.ensure_root()?;
        let segs: Vec<&str> = segments(path).collect();
        for i in 1..segs.len() {
            let dir = segs[..i].join("/");
            if self.known_dirs.lock().unwrap().contains(&dir) {
                continue;
            }
            self.mkcol(&self.url(&dir, true))?;
            self.known_dirs.lock().unwrap().insert(dir);
        }
        Ok(())
    }

    /// Make sure the folder holding `path` exists: one `PROPFIND` the first
    /// time, `MKCOL`s only when it is missing.
    fn ensure_parent_exists(&self, path: &str) -> Result<()> {
        let segs: Vec<&str> = segments(path).collect();
        let parent = segs[..segs.len().saturating_sub(1)].join("/");
        if self.known_dirs.lock().unwrap().contains(&parent) {
            return Ok(());
        }
        if self.propfind(&self.url(&parent, true), "0", PROPFIND_LIST)?.is_some() {
            let mut known = self.known_dirs.lock().unwrap();
            for i in 0..segs.len() {
                known.insert(segs[..i].join("/"));
            }
            return Ok(());
        }
        self.ensure_parents(path)
    }

    fn put(&self, path: &str, body: &Body) -> Result<()> {
        let url = self.url(path, false);
        // Servers may reject a PUT into a missing folder before reading the
        // body and drop the connection mid-upload, so large uploads check the
        // folder first. Small writes just retry after creating it.
        if matches!(body, Body::File(_)) {
            self.ensure_parent_exists(path)?;
        }
        let mut created = false;
        loop {
            let t = Instant::now();
            let res = match body {
                Body::Bytes(b) => self.req(&self.bulk, "PUT", &url).set("Content-Type", "application/octet-stream").send_bytes(b),
                Body::File(p) => {
                    let f = std::fs::File::open(p).with_context(|| crate::tr!("无法读取 {}", "Can't read {}", p.display()))?;
                    let len = f.metadata()?.len();
                    // An explicit length avoids chunked uploads, which some
                    // WebDAV servers reject.
                    self.req(&self.bulk, "PUT", &url).set("Content-Type", "application/octet-stream").set("Content-Length", &len.to_string()).send(f)
                }
            };
            trace("PUT", &url, t, &res);
            match res {
                Ok(_) => return Ok(()),
                Err(ureq::Error::Status(404 | 409, _)) if !created => {
                    self.ensure_parents(path)?;
                    created = true;
                }
                Err(e) => return Err(fail(e)),
            }
        }
    }

    /// `Ok(false)` when the server does not support moving files.
    fn rename(&self, from: &str, to: &str) -> Result<bool> {
        let t = Instant::now();
        let url = self.url(from, false);
        let res = self.req(&self.agent, "MOVE", &url).set("Destination", self.url(to, false).as_str()).set("Overwrite", "T").call();
        trace("MOVE", &url, t, &res);
        match res {
            Ok(_) => Ok(true),
            Err(ureq::Error::Status(403 | 405 | 501 | 502, _)) => Ok(false),
            Err(e) => Err(fail(e)),
        }
    }

    fn put_atomic(&self, path: &str, body: Body) -> Result<()> {
        if !self.atomic {
            return self.put(path, &body);
        }
        let tmp = temp_name(path);
        self.put(&tmp, &body)?;
        match self.rename(&tmp, path) {
            Ok(true) => Ok(()),
            Ok(false) => {
                let _ = self.remove(&tmp);
                self.put(path, &body)
            }
            Err(e) => {
                let _ = self.remove(&tmp);
                Err(e)
            }
        }
    }

    fn get(&self, agent: &ureq::Agent, path: &str) -> Result<Option<ureq::Response>> {
        let t = Instant::now();
        let url = self.url(path, false);
        let res = self.req(agent, "GET", &url).call();
        trace("GET", &url, t, &res);
        match res {
            Ok(r) => Ok(Some(r)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(fail(e)),
        }
    }
}

impl Store for WebDav {
    fn kind(&self) -> &'static str {
        "webDav"
    }

    fn label(&self) -> String {
        let mut u = self.url("", false);
        let _ = u.set_username("");
        let _ = u.set_password(None);
        decode(u.as_str())
    }

    fn quota(&self) -> Result<(Option<u64>, Option<u64>)> {
        let items = self.propfind(&self.url("", true), "0", PROPFIND_QUOTA)?.unwrap_or_default();
        let Some(i) = items.first() else { return Ok((None, None)) };
        Ok((i.quota_used, i.quota_used.zip(i.quota_available).map(|(u, a)| u + a)))
    }

    fn check(&self) -> Result<()> {
        self.known_dirs.lock().unwrap().remove("");
        self.ensure_root()
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        check_path(dir)?;
        let url = self.url(dir, true);
        let Some(items) = self.propfind(&url, "1", PROPFIND_LIST)? else {
            return Ok(Vec::new());
        };
        let me = decode(url.path());
        let me = me.trim_end_matches('/');
        Ok(items
            .into_iter()
            .filter(|i| i.path.trim_end_matches('/') != me)
            .filter_map(|i| {
                let name = i.path.trim_end_matches('/').rsplit('/').next()?.to_string();
                // The server says what the names are: one no path may have
                // (`..`, a `\`) is no file of ours.
                is_safe_name(&name).then(|| Entry {
                    tag: if i.etag.is_empty() { format!("{}|{}", i.modified, i.size) } else { i.etag.clone() },
                    name,
                    is_dir: i.is_dir,
                    size: i.size,
                })
            })
            .collect())
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        check_path(path)?;
        let Some(r) = self.get(&self.agent, path)? else { return Ok(None) };
        let mut buf = Vec::new();
        r.into_reader().read_to_end(&mut buf).map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        Ok(Some(buf))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        check_path(path)?;
        let r = self.get(&self.bulk, path)?.ok_or_else(|| anyhow!(crate::tr!("云端找不到文件 {path}", "File not found in the cloud: {path}")))?;
        let tmp = dest.with_extension("download");
        let mut out = std::fs::File::create(&tmp).with_context(|| crate::tr!("无法写入 {}", "Can't write {}", tmp.display()))?;
        std::io::copy(&mut r.into_reader(), &mut out).map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        check_path(path)?;
        self.put_atomic(path, Body::Bytes(data))
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        check_path(path)?;
        self.put_atomic(path, Body::File(src))
    }

    fn remove(&self, path: &str) -> Result<()> {
        check_path(path)?;
        let t = Instant::now();
        let url = self.url(path, false);
        let res = self.req(&self.agent, "DELETE", &url).call();
        trace("DELETE", &url, t, &res);
        match res {
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(fail(e)),
        }
    }

    fn cache_scope(&self) -> Option<String> {
        Some(format!("webdav|{}|{}", self.username, self.url("", true)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multistatus_with_prefixes_and_encoding() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/Ludonimbus/games/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
  <D:response>
    <D:href>https://nas.local:5006/dav/Ludonimbus/games/%E8%A1%80%E6%BA%90.json</D:href>
    <D:propstat>
      <D:prop><D:resourcetype/><D:getcontentlength>42</D:getcontentlength><D:getetag>"abc"</D:getetag></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
    <D:propstat><D:prop><D:quota-used-bytes/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
  </D:response>
</D:multistatus>"#;
        let items = parse_multistatus(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_dir);
        assert_eq!(items[1].path, "/dav/Ludonimbus/games/血源.json");
        assert_eq!(items[1].size, 42);
        assert_eq!(items[1].etag, "\"abc\"");
        assert!(!items[1].is_dir);
        assert_eq!(items[1].quota_used, None);
    }

    #[test]
    fn builds_urls_below_the_folder() {
        let d = WebDav::new("https://dav.example.com/dav", "u", "p", "Ludo nimbus/sub", Duration::from_secs(5)).unwrap();
        assert_eq!(d.url("", true).as_str(), "https://dav.example.com/dav/Ludo%20nimbus/sub/");
        assert_eq!(d.url("games/x/snapshots/1.zip", false).as_str(), "https://dav.example.com/dav/Ludo%20nimbus/sub/games/x/snapshots/1.zip");
        assert_eq!(d.label(), "https://dav.example.com/dav/Ludo nimbus/sub");
        // No segment leads out of the folder, however it is written.
        for (path, url) in [("games/../../x", "games/x"), (r"games\..\x", "games%5C..%5Cx"), ("games/%2e%2e/x", "games/%252e%252e/x")] {
            assert_eq!(d.url(path, false).as_str(), format!("https://dav.example.com/dav/Ludo%20nimbus/sub/{url}"), "{path}");
        }
    }
}
