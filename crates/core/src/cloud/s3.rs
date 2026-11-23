//! S3-compatible object storage: MinIO on a NAS, Aliyun OSS, Tencent COS,
//! Cloudflare R2, Backblaze B2 and others.
//!
//! Requests are signed with AWS Signature Version 4. Objects are written with
//! a single `PUT`, which S3 makes atomic, so no temporary names are needed.
//! "Folders" are key prefixes listed with `delimiter=/`.

use super::store::{Entry, Store};
use super::Unavailable;
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use url::Url;

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub struct S3 {
    scheme: String,
    /// `host[:port]` of the endpoint.
    host: String,
    region: String,
    bucket: String,
    /// Key prefix without slashes at either end (may be empty).
    prefix: String,
    key_id: String,
    secret: String,
    path_style: bool,
    agent: ureq::Agent,
    bulk: ureq::Agent,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(data);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// URI-encode per SigV4: everything except `A-Z a-z 0-9 - _ . ~` (and `/`
/// when `keep_slash`).
pub fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A request ready to be signed.
pub struct SignInput<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Already URI-encoded path, starting with `/`.
    pub path: &'a str,
    /// Unencoded query parameters.
    pub query: &'a [(&'a str, &'a str)],
    /// Extra headers to sign (lowercase names).
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
    /// `YYYYMMDDTHHMMSSZ`
    pub amz_date: &'a str,
    pub region: &'a str,
    pub key_id: &'a str,
    pub secret: &'a str,
}

/// Canonical query string (also used as the request's query string).
pub fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (uri_encode(k, false), uri_encode(v, false))).collect();
    q.sort();
    q.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

/// The `Authorization` header value for a request.
pub fn authorization(r: &SignInput) -> String {
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), r.host.to_string()),
        ("x-amz-content-sha256".into(), r.payload_hash.to_string()),
        ("x-amz-date".into(), r.amz_date.to_string()),
    ];
    headers.extend(r.headers.iter().map(|(k, v)| (k.to_lowercase(), v.trim().to_string())));
    headers.sort();
    let signed = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        r.method,
        r.path,
        canonical_query(r.query),
        canonical_headers,
        signed,
        r.payload_hash
    );
    let date = &r.amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", r.region);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{scope}\n{}",
        r.amz_date,
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let k_date = hmac_sha256(format!("AWS4{}", r.secret).as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, r.region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex(&hmac_sha256(&k_signing, to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope},SignedHeaders={signed},Signature={signature}",
        r.key_id
    )
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).with_context(|| format!("无法读取 {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// `(code, message)` of an S3 XML error body.
fn s3_error(body: &str) -> (String, String) {
    let Ok(doc) = roxmltree::Document::parse(body) else {
        return (String::new(), body.chars().take(200).collect());
    };
    let find = |name: &str| {
        doc.descendants()
            .find(|n| n.tag_name().name() == name)
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string()
    };
    (find("Code"), find("Message"))
}

fn fail(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            let (c, msg) = s3_error(&resp.into_string().unwrap_or_default());
            match c.as_str() {
                "InvalidAccessKeyId" => anyhow!("Access Key 不对（{c}）"),
                "SignatureDoesNotMatch" => anyhow!("Secret Key 不对（{c}）"),
                "NoSuchBucket" => anyhow!("Bucket 不存在（{c}）"),
                "AccessDenied" => anyhow!("没有权限访问这个 Bucket（{c}），请检查密钥的权限"),
                "AuthorizationHeaderMalformed" | "PermanentRedirect" | "IllegalLocationConstraintException" => {
                    anyhow!("区域（Region）或访问方式不对（{c}）：{msg}")
                }
                "SlowDown" | "ServiceUnavailable" | "InternalError" => anyhow!(Unavailable(format!("S3 服务繁忙（{c}）"))),
                _ if code >= 500 => anyhow!(Unavailable(format!("S3 服务器错误 {code}"))),
                _ if c.is_empty() => anyhow!("S3 返回错误 {code}：{msg}"),
                _ => anyhow!("S3 返回错误 {code}（{c}）：{msg}"),
            }
        }
        ureq::Error::Transport(t) => {
            let msg = t.to_string();
            if msg.to_lowercase().contains("certificate") {
                anyhow!("无法验证 S3 服务器的 HTTPS 证书：{msg}")
            } else {
                anyhow!(Unavailable(msg))
            }
        }
    }
}

impl S3 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: &str,
        region: &str,
        bucket: &str,
        prefix: &str,
        key_id: &str,
        secret: &str,
        path_style: bool,
        timeout: Duration,
    ) -> Result<Self> {
        let raw = endpoint.trim();
        let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
        let u = Url::parse(&with_scheme).map_err(|_| anyhow!("Endpoint 格式不对，例如 https://oss-cn-hangzhou.aliyuncs.com"))?;
        if !matches!(u.scheme(), "http" | "https") {
            bail!("Endpoint 需要以 http:// 或 https:// 开头");
        }
        let host = u.host_str().ok_or_else(|| anyhow!("Endpoint 缺少服务器名"))?;
        let host = match u.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_string(),
        };
        let bucket = bucket.trim();
        if bucket.is_empty() {
            bail!("请填写 Bucket 名称");
        }
        if key_id.trim().is_empty() {
            bail!("请填写 Access Key");
        }
        Ok(S3 {
            scheme: u.scheme().to_string(),
            host,
            region: if region.trim().is_empty() { "us-east-1".into() } else { region.trim().to_string() },
            bucket: bucket.to_string(),
            prefix: prefix.trim().trim_matches('/').to_string(),
            key_id: key_id.trim().to_string(),
            secret: secret.to_string(),
            path_style,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(timeout.max(Duration::from_secs(20)))
                .timeout_write(timeout.max(Duration::from_secs(20)))
                .build(),
            bulk: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout_read(Duration::from_secs(300))
                .timeout_write(Duration::from_secs(300))
                .build(),
        })
    }

    fn key(&self, path: &str) -> String {
        super::store::join(&[&self.prefix, path])
    }

    /// `(host, encoded path)` of an object key (`""` = the bucket itself).
    fn target(&self, key: &str) -> (String, String) {
        if self.path_style {
            let p = if key.is_empty() {
                format!("/{}", uri_encode(&self.bucket, false))
            } else {
                format!("/{}/{}", uri_encode(&self.bucket, false), uri_encode(key, true))
            };
            (self.host.clone(), p)
        } else {
            (format!("{}.{}", self.bucket, self.host), format!("/{}", uri_encode(key, true)))
        }
    }

    fn request(
        &self,
        agent: &ureq::Agent,
        method: &str,
        key: &str,
        query: &[(&str, &str)],
        payload_hash: &str,
    ) -> ureq::Request {
        let (host, path) = self.target(key);
        let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let auth = authorization(&SignInput {
            method,
            host: &host,
            path: &path,
            query,
            headers: &[],
            payload_hash,
            amz_date: &amz_date,
            region: &self.region,
            key_id: &self.key_id,
            secret: &self.secret,
        });
        let q = canonical_query(query);
        let url = if q.is_empty() {
            format!("{}://{host}{path}", self.scheme)
        } else {
            format!("{}://{host}{path}?{q}", self.scheme)
        };
        agent
            .request(method, &url)
            .set("x-amz-date", &amz_date)
            .set("x-amz-content-sha256", payload_hash)
            .set("Authorization", &auth)
    }

    fn get(&self, agent: &ureq::Agent, path: &str) -> Result<Option<ureq::Response>> {
        match self.request(agent, "GET", &self.key(path), &[], EMPTY_SHA256).call() {
            Ok(r) => Ok(Some(r)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(fail(e)),
        }
    }

    fn list_page(&self, prefix: &str, token: Option<&str>, max: Option<&str>) -> Result<String> {
        let mut q: Vec<(&str, &str)> = vec![("list-type", "2"), ("delimiter", "/"), ("prefix", prefix)];
        if let Some(t) = token {
            q.push(("continuation-token", t));
        }
        if let Some(m) = max {
            q.push(("max-keys", m));
        }
        self.request(&self.agent, "GET", "", &q, EMPTY_SHA256)
            .call()
            .map_err(fail)?
            .into_string()
            .map_err(|e| anyhow!(Unavailable(e.to_string())))
    }
}

/// Parse one `ListObjectsV2` page into entries below `prefix`.
pub fn parse_list(xml: &str, prefix: &str) -> Result<(Vec<Entry>, Option<String>)> {
    let doc = roxmltree::Document::parse(xml).context("S3 返回的不是有效的 XML")?;
    let text = |n: roxmltree::Node, name: &str| {
        n.children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|c| c.text())
            .unwrap_or("")
            .to_string()
    };
    let mut out = Vec::new();
    for n in doc.root_element().children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "Contents" => {
                let key = text(n, "Key");
                let Some(name) = key.strip_prefix(prefix) else { continue };
                if name.is_empty() || name.contains('/') {
                    continue;
                }
                let size = text(n, "Size").parse().unwrap_or(0);
                let etag = text(n, "ETag");
                out.push(Entry {
                    name: name.to_string(),
                    is_dir: false,
                    size,
                    tag: if etag.is_empty() { format!("{}|{size}", text(n, "LastModified")) } else { etag },
                });
            }
            "CommonPrefixes" => {
                let p = text(n, "Prefix");
                if let Some(name) = p.strip_prefix(prefix).map(|s| s.trim_end_matches('/')) {
                    if !name.is_empty() {
                        out.push(Entry {
                            name: name.to_string(),
                            is_dir: true,
                            size: 0,
                            tag: String::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    let truncated = text(doc.root_element(), "IsTruncated") == "true";
    let next = text(doc.root_element(), "NextContinuationToken");
    Ok((out, (truncated && !next.is_empty()).then_some(next)))
}

impl Store for S3 {
    fn kind(&self) -> &'static str {
        "s3"
    }

    fn label(&self) -> String {
        if self.prefix.is_empty() {
            format!("{} / {}", self.host, self.bucket)
        } else {
            format!("{} / {} / {}", self.host, self.bucket, self.prefix)
        }
    }

    fn check(&self) -> Result<()> {
        let prefix = if self.prefix.is_empty() { String::new() } else { format!("{}/", self.prefix) };
        self.list_page(&prefix, None, Some("1")).map(|_| ())
    }

    fn list(&self, dir: &str) -> Result<Vec<Entry>> {
        let key = self.key(dir);
        let prefix = if key.is_empty() { key } else { format!("{key}/") };
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let xml = self.list_page(&prefix, token.as_deref(), None)?;
            let (mut entries, next) = parse_list(&xml, &prefix)?;
            out.append(&mut entries);
            match next {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(r) = self.get(&self.agent, path)? else { return Ok(None) };
        let mut buf = Vec::new();
        r.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        Ok(Some(buf))
    }

    fn download(&self, path: &str, dest: &Path) -> Result<()> {
        let r = self
            .get(&self.bulk, path)?
            .ok_or_else(|| anyhow!("云端找不到文件 {path}"))?;
        let tmp = dest.with_extension("download");
        let mut out = std::fs::File::create(&tmp).with_context(|| format!("无法写入 {}", tmp.display()))?;
        std::io::copy(&mut r.into_reader(), &mut out).map_err(|e| anyhow!(Unavailable(e.to_string())))?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<()> {
        let hash = hex(&Sha256::digest(data));
        self.request(&self.bulk, "PUT", &self.key(path), &[], &hash)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(data)
            .map_err(fail)?;
        Ok(())
    }

    fn upload(&self, path: &str, src: &Path) -> Result<()> {
        let hash = file_sha256(src)?;
        let f = std::fs::File::open(src).with_context(|| format!("无法读取 {}", src.display()))?;
        let len = f.metadata()?.len();
        self.request(&self.bulk, "PUT", &self.key(path), &[], &hash)
            .set("Content-Type", "application/octet-stream")
            .set("Content-Length", &len.to_string())
            .send(f)
            .map_err(fail)?;
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<()> {
        match self.request(&self.agent, "DELETE", &self.key(path), &[], EMPTY_SHA256).call() {
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(fail(e)),
        }
    }

    fn cache_scope(&self) -> Option<String> {
        Some(format!("s3|{}|{}|{}", self.host, self.bucket, self.prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_ID: &str = "AKIAIOSFODNN7EXAMPLE";
    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    // Examples from the AWS documentation "Signature Calculations for the
    // Authorization Header: Transferring Payload in a Single Chunk".
    #[test]
    fn signs_get_object_like_aws_example() {
        let auth = authorization(&SignInput {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            query: &[],
            headers: &[("range", "bytes=0-9")],
            payload_hash: EMPTY_SHA256,
            amz_date: "20130524T000000Z",
            region: "us-east-1",
            key_id: KEY_ID,
            secret: SECRET,
        });
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,\
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,\
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn signs_list_objects_like_aws_example() {
        let auth = authorization(&SignInput {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/",
            query: &[("max-keys", "2"), ("prefix", "J")],
            headers: &[],
            payload_hash: EMPTY_SHA256,
            amz_date: "20130524T000000Z",
            region: "us-east-1",
            key_id: KEY_ID,
            secret: SECRET,
        });
        assert!(auth.ends_with("Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"), "{auth}");
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn parses_list_pages() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>b</Name><Prefix>Ludonimbus/games/</Prefix><IsTruncated>true</IsTruncated>
  <NextContinuationToken>tok</NextContinuationToken>
  <Contents><Key>Ludonimbus/games/x.json</Key><Size>12</Size><ETag>"e1"</ETag></Contents>
  <CommonPrefixes><Prefix>Ludonimbus/games/fe/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let (entries, next) = parse_list(xml, "Ludonimbus/games/").unwrap();
        assert_eq!(next.as_deref(), Some("tok"));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "x.json");
        assert_eq!(entries[0].tag, "\"e1\"");
        assert!(entries[1].is_dir);
        assert_eq!(entries[1].name, "fe");
    }

    #[test]
    fn addresses_objects() {
        let t = Duration::from_secs(5);
        let v = S3::new("https://oss-cn-hangzhou.aliyuncs.com", "", "my-bucket", "/Ludonimbus/", "k", "s", false, t).unwrap();
        assert_eq!(
            v.target(&v.key("games/a b.zip")),
            ("my-bucket.oss-cn-hangzhou.aliyuncs.com".to_string(), "/Ludonimbus/games/a%20b.zip".to_string())
        );
        let p = S3::new("http://192.168.1.5:9000", "", "saves", "", "k", "s", true, t).unwrap();
        assert_eq!(p.target(&p.key("devices/d.json")), ("192.168.1.5:9000".to_string(), "/saves/devices/d.json".to_string()));
        assert_eq!(p.target(""), ("192.168.1.5:9000".to_string(), "/saves".to_string()));
    }
}
