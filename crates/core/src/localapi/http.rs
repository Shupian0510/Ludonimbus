//! The little HTTP/1.1 the local API needs: one request per connection, no
//! body to read, `Connection: close`. Written here rather than taken from a
//! crate so that every limit is this module's: how long a client may take
//! and how much it may send.

use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

/// Largest request line and headers. The plugins send a few hundred bytes.
pub(crate) const MAX_HEAD: usize = 16 * 1024;
/// Most headers in one request.
const MAX_HEADERS: usize = 64;
/// Largest body. No endpoint takes one: a POST comes with an empty body.
pub(crate) const MAX_BODY: usize = 16 * 1024;

pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    headers: Vec<(String, String)>,
}

impl Request {
    /// The first header of that name (names are not case-sensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, v: &Value) -> Response {
        Response { status, body: v.to_string().into_bytes() }
    }

    pub fn error(status: u16, message: &str) -> Response {
        Response::json(status, &serde_json::json!({ "error": message }))
    }
}

/// Why no request came out of a connection.
pub(crate) enum Refused {
    /// The client went away, sent nothing in time, or broke the connection:
    /// nobody to answer.
    Gone,
    /// The request can't be served: tell the client why, then close.
    Answer(Response),
}

/// What [`parse_head`] takes from a request's head besides the request.
struct Head {
    request: Request,
    length: usize,
    chunked: bool,
    wants_continue: bool,
}

/// One read that ends by `deadline` at the latest.
fn read_some(tcp: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> io::Result<usize> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    tcp.set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
    tcp.read(buf)
}

/// Read one request: the head and the (empty) body, all within `timeout`.
pub(crate) fn read_request(tcp: &mut TcpStream, timeout: Duration) -> Result<Request, Refused> {
    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let end = loop {
        let from = buf.len().saturating_sub(3);
        let n = read_some(tcp, &mut chunk, deadline).map_err(|_| Refused::Gone)?;
        if n == 0 {
            return Err(Refused::Gone);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf[from..].windows(4).position(|w| w == b"\r\n\r\n") {
            break from + i;
        }
        if buf.len() > MAX_HEAD {
            return Err(Refused::Answer(Response::error(431, "request headers too large")));
        }
    };
    if end > MAX_HEAD {
        return Err(Refused::Answer(Response::error(431, "request headers too large")));
    }
    let head = parse_head(&buf[..end]).map_err(Refused::Answer)?;
    if head.chunked {
        return Err(Refused::Answer(Response::error(411, "send a Content-Length")));
    }
    if head.length > MAX_BODY {
        return Err(Refused::Answer(Response::error(413, "request body too large")));
    }
    // The body isn't used; it is read so that closing doesn't reset the
    // connection before the client has the answer.
    let mut have = buf.len() - (end + 4);
    if head.wants_continue && have < head.length {
        tcp.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").map_err(|_| Refused::Gone)?;
    }
    while have < head.length {
        let n = read_some(tcp, &mut chunk, deadline).map_err(|_| Refused::Gone)?;
        if n == 0 {
            return Err(Refused::Gone);
        }
        have += n;
    }
    Ok(head.request)
}

fn parse_head(raw: &[u8]) -> Result<Head, Response> {
    let bad = || Response::error(400, "bad request");
    let text = std::str::from_utf8(raw).map_err(|_| bad())?;
    let mut lines = text.split("\r\n");
    let first: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
    let &[method, target, version] = first.as_slice() else { return Err(bad()) };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) || !target.starts_with('/') || !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(bad());
    }
    let mut headers = Vec::new();
    let (mut length, mut chunked, mut wants_continue) = (None, false, false);
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(bad)?;
        // Also refuses folded lines (they start with a space).
        if name.is_empty() || name.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(bad());
        }
        let value = value.trim_matches(|c| c == ' ' || c == '\t');
        if name.eq_ignore_ascii_case("content-length") {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            let n = value.parse::<usize>().unwrap_or(usize::MAX);
            if length.is_some_and(|l| l != n) {
                return Err(bad());
            }
            length = Some(n);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = true;
        } else if name.eq_ignore_ascii_case("expect") {
            wants_continue = value.eq_ignore_ascii_case("100-continue");
        }
        if headers.len() >= MAX_HEADERS {
            return Err(Response::error(431, "too many request headers"));
        }
        headers.push((name.to_string(), value.to_string()));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let request = Request { method: method.to_string(), path: path.to_string(), query: query.to_string(), headers };
    Ok(Head { request, length: length.unwrap_or(0), chunked, wants_continue })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "",
    }
}

pub(crate) fn write_response<W: Write>(w: &mut W, r: &Response) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        r.status,
        reason(r.status),
        r.body.len()
    );
    w.write_all(head.as_bytes())?;
    w.write_all(&r.body)?;
    w.flush()
}

/// After answering a request that wasn't read to the end (a refusal): take
/// what the client still sends for a moment, so that closing doesn't reset
/// the connection before it has read the answer.
pub(crate) fn linger(tcp: &mut TcpStream) {
    let _ = tcp.shutdown(Shutdown::Write);
    let until = Instant::now() + Duration::from_secs(1);
    let mut left = 1024 * 1024usize;
    let mut buf = [0u8; 16 * 1024];
    while left > 0 {
        match read_some(tcp, &mut buf, until) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

/// `%XX` and `+` decoding of a query value.
pub(crate) fn unescape(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The raw value of a query parameter (`name` alone gives "").
pub(crate) fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').map(|kv| kv.split_once('=').unwrap_or((kv, ""))).find(|(k, _)| *k == name).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &str) -> Result<Head, u16> {
        parse_head(raw.as_bytes()).map_err(|r| r.status)
    }

    #[test]
    fn heads() {
        let h = head("POST /v1/games/a/sync?resolution=cloud HTTP/1.1\r\nHost: 127.0.0.1:1\r\nauthorization: Bearer x\r\nContent-Length: 0").ok().unwrap();
        assert_eq!((h.request.method.as_str(), h.request.path.as_str(), h.request.query.as_str()), ("POST", "/v1/games/a/sync", "resolution=cloud"));
        assert_eq!(h.request.header("Authorization"), Some("Bearer x"));
        assert_eq!((h.length, h.chunked, h.wants_continue), (0, false, false));
        assert!(head("GET /v1/ping HTTP/1.0").is_ok());

        assert_eq!(head("GET /v1/ping").err(), Some(400));
        assert_eq!(head("GET http://evil/v1/ping HTTP/1.1").err(), Some(400));
        assert_eq!(head("get /v1/ping HTTP/1.1").err(), Some(400));
        assert_eq!(head("GET /v1/ping HTTP/2").err(), Some(400));
        assert_eq!(head("GET  /v1/ping HTTP/1.1").err(), Some(400));
        assert_eq!(head("GET /v1/ping HTTP/1.1\r\nno colon").err(), Some(400));
        assert_eq!(head("GET /v1/ping HTTP/1.1\r\nX: a\r\n folded").err(), Some(400));
        assert_eq!(head("GET /v1/ping HTTP/1.1\r\nContent-Length: +5").err(), Some(400));
        assert_eq!(head("GET /v1/ping HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2").err(), Some(400));
        let many: String = (0..=MAX_HEADERS).map(|i| format!("\r\nX-{i}: y")).collect();
        assert_eq!(head(&format!("GET /v1/ping HTTP/1.1{many}")).err(), Some(431));

        let h = head("POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue").ok().unwrap();
        assert!(h.chunked && h.wants_continue);
        assert_eq!(head("POST /x HTTP/1.1\r\nContent-Length: 99999999999999999999999").ok().unwrap().length, usize::MAX);
    }

    #[test]
    fn query_values() {
        assert_eq!(query_param("a=1&refresh&b=", "refresh"), Some(""));
        assert_eq!(query_param("a=1&refresh&b=", "a"), Some("1"));
        assert_eq!(query_param("a=1&refresh&b=", "b"), Some(""));
        assert_eq!(query_param("a=1", "c"), None);
        assert_eq!(unescape("a%20b+c%E4%B8%AD%2"), "a b c中%2");
    }
}
