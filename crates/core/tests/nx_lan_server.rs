//! The PC's real HTTPS server for the Switch (docs/nx-protocol.md), spoken
//! to the way the Switch does (Ludonimbus-NX `net.cpp`): TLS 1.2 or 1.3 with
//! the certificate pinned by its fingerprint, one request per connection,
//! `Connection: close`, the answer read to the end. And what anyone else on
//! the network can't do: send a large body without a token, keep
//! connections open, or open many at once.

use ludonimbus_core::config::{self, AppConfig};
use ludonimbus_core::nxlan;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Trusts the one certificate whose fingerprint was pinned at pairing, as the Switch does.
#[derive(Debug)]
struct Pinned(String, Arc<CryptoProvider>);

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp: String = Sha256::digest(cert.as_ref()).iter().map(|b| format!("{b:02x}")).collect();
        if fp == self.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("not the paired PC".into()))
        }
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.1.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.1.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.1.signature_verification_algorithms.supported_schemes()
    }
}

fn client(fingerprint: &str, tls12_only: bool) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let versions: &[&rustls::SupportedProtocolVersion] = if tls12_only { &[&rustls::version::TLS12] } else { rustls::DEFAULT_VERSIONS };
    let cfg = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(versions)
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned(fingerprint.to_string(), provider)))
        .with_no_client_auth();
    Arc::new(cfg)
}

fn open(port: u16, tls: &Arc<ClientConfig>) -> StreamOwned<ClientConnection, TcpStream> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let conn = ClientConnection::new(tls.clone(), ServerName::try_from("ludonimbus.local").unwrap()).unwrap();
    StreamOwned::new(conn, tcp)
}

/// Send `head` (and `body`), read the answer to the end: (status, body).
fn exchange(port: u16, tls: &Arc<ClientConfig>, head: &str, body: &[u8]) -> std::io::Result<(u16, Vec<u8>)> {
    let mut s = open(port, tls);
    s.write_all(head.as_bytes())?;
    s.write_all(body)?;
    s.flush()?;
    let mut raw = Vec::new();
    // Some servers close without a close_notify: what came is the answer.
    if let Err(e) = s.read_to_end(&mut raw) {
        if raw.is_empty() {
            return Err(e);
        }
    }
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| std::io::Error::other("no answer"))?;
    let status = std::str::from_utf8(&raw[..end]).unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    Ok((status, raw[end + 4..].to_vec()))
}

/// A request as the Switch writes it.
fn request(port: u16, tls: &Arc<ClientConfig>, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> std::io::Result<(u16, Vec<u8>)> {
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}\r\n",
        body.len()
    );
    exchange(port, tls, &head, body)
}

fn json_of(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap()
}

/// Whether the server closes this connection before `within` passes.
fn closed_within(tcp: &mut TcpStream, within: Duration) -> bool {
    tcp.set_read_timeout(Some(within)).unwrap();
    let mut buf = [0u8; 64];
    loop {
        match tcp.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(e) => return !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    }
}

#[test]
fn switch_over_https() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));
    config::update(|c| {
        *c = AppConfig::default();
        c.nx_lan = true;
        Ok(())
    })
    .unwrap();
    nxlan::hold_uploads(true);
    let idle = Duration::from_secs(3);
    nxlan::set_io_timeout(idle);
    let server = nxlan::listen("127.0.0.1", 0, |_| {}).unwrap();
    let port = server.port;
    let (tls12, tls13) = (client(&server.fingerprint, true), client(&server.fingerprint, false));

    // Hello over TLS 1.2 (the Switch's mbedTLS) and 1.3.
    for tls in [&tls12, &tls13] {
        let (s, b) = request(port, tls, "GET", "/nx/v1/hello", None, b"").unwrap();
        assert_eq!((s, json_of(&b)["protocol"].as_u64()), (200, Some(2)));
    }
    // A Switch that pinned another PC's certificate doesn't get through.
    assert!(request(port, &client(&"00".repeat(32), false), "GET", "/nx/v1/hello", None, b"").is_err());

    // Pair, then the token works.
    let (code, _) = nxlan::start_pairing();
    let pair = json!({ "code": code, "id": "sw-1", "name": "Switch" }).to_string();
    let (s, b) = request(port, &tls12, "POST", "/nx/v1/pair", None, pair.as_bytes()).unwrap();
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let token = json_of(&b)["token"].as_str().unwrap().to_string();
    let (s, b) = request(port, &tls12, "POST", "/nx/v1/status", Some(&token), br#"{"games":[]}"#).unwrap();
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(nxlan::devices()[0].last_seen.is_some());

    // Refused from the headers alone, before a byte of the body: the
    // hundreds of megabytes these say never have to arrive.
    let upload = "/nx/v1/games/0100770008dd8000/snapshots";
    let big = |path: &str, token: Option<&str>, length: usize| {
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let head = format!("POST {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {length}\r\n{auth}\r\n");
        let started = Instant::now();
        let (s, _) = exchange(port, &tls13, &head, b"").unwrap();
        assert!(started.elapsed() < idle, "answered without waiting for the body");
        s
    };
    assert_eq!(big(upload, None, 500 << 20), 401);
    assert_eq!(big(upload, Some("not-a-token"), 500 << 20), 401);
    assert_eq!(big(upload, Some(&token), (512 << 20) + 1), 413);
    assert_eq!(big("/nx/v1/status", Some(&token), 2 << 20), 413);
    assert_eq!(big("/nx/v1/pair", None, 1 << 20), 413);
    let chunked = format!("POST {upload} HTTP/1.1\r\nTransfer-Encoding: chunked\r\nAuthorization: Bearer {token}\r\n\r\n");
    assert_eq!(exchange(port, &tls13, &chunked, b"").unwrap().0, 411);
    // Endless headers are cut off.
    let endless = format!("GET /nx/v1/hello HTTP/1.1\r\nX: {}", "a".repeat(64 * 1024));
    assert!(exchange(port, &tls13, &endless, b"").is_err());

    // A few connections from one address at a time; one more is closed at
    // once, without being read. Idle ones (no handshake) are closed after
    // the timeout, and the place is free again.
    let mut held: Vec<TcpStream> = (0..4).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(300));
    let mut extra = TcpStream::connect(("127.0.0.1", port)).unwrap();
    assert!(closed_within(&mut extra, Duration::from_millis(1500)), "over the limit: closed at once");
    assert!(request(port, &tls13, "GET", "/nx/v1/hello", None, b"").is_err(), "no place while four are held");
    for tcp in &mut held {
        assert!(closed_within(tcp, idle * 3), "an idle connection is closed");
    }
    let (s, _) = request(port, &tls13, "GET", "/nx/v1/hello", None, b"").unwrap();
    assert_eq!(s, 200);

    // Stopped: the port is let go.
    server.stop();
    let started = Instant::now();
    while TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).is_ok() {
        assert!(started.elapsed() < Duration::from_secs(10), "still listening after stop");
        std::thread::sleep(Duration::from_millis(100));
    }
}
