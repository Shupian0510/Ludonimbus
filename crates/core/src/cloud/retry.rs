//! Sending a request again when the server turns it away for the moment
//! (too many requests, or a server failing): at most twice more, after the
//! wait the server asks for in `Retry-After`, never longer than 10 s.
//!
//! Only for requests that do the same when they arrive twice: a lost answer
//! to a request that did go through is then harmless. Reads, listings,
//! deletes and replacing a whole file are; creating a folder, moving a file
//! or opening something that would exist twice are not.
//!
//! Google Drive and Baidu Netdisk have loops of their own (a new token on a
//! 401, Drive's rate-limit 403, Baidu's error numbers) built on
//! [`retry_wait`] and [`busy_status`]; WebDAV and S3 use [`send`].

use chrono::{DateTime, Utc};
use std::time::Duration;

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
/// longer than [`LONGEST_WAIT`].
pub(crate) fn retry_wait(attempt: u32, retry_after: Option<&str>, now: DateTime<Utc>) -> Option<Duration> {
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
/// or a server failing.
pub(crate) fn busy_status(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// Send the request `request` builds and sends, again while the server
/// answers with a [`busy_status`] and [`retry_wait`] allows another try.
/// `request` builds the whole request each time (a signature with the time
/// in it, a file opened again from the start). Only for requests that do
/// the same when they arrive twice. A connection that fails is not tried
/// again: a missing network shouldn't hold anything up. `what` names the
/// request in the log.
pub(crate) fn send(what: &str, mut request: impl FnMut() -> Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, ureq::Error> {
    let mut attempt = 0;
    loop {
        match request() {
            Err(ureq::Error::Status(code, resp)) if busy_status(code) => match retry_wait(attempt, resp.header("Retry-After"), Utc::now()) {
                Some(wait) => {
                    attempt += 1;
                    crate::logger::log("cloud", format!("{what}: HTTP {code}; trying again in {} ms", wait.as_millis()));
                    std::thread::sleep(wait);
                }
                None => return Err(ureq::Error::Status(code, resp)),
            },
            done => return done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_request_turned_away_for_the_moment_waits_as_asked() {
        let now = DateTime::parse_from_rfc3339("2026-11-27T10:00:00.000Z").unwrap().with_timezone(&Utc);
        assert_eq!(retry_wait(0, None, now), Some(secs(1)));
        assert_eq!(retry_wait(1, None, now), Some(secs(2)));
        assert_eq!(retry_wait(2, None, now), None);
        assert_eq!(retry_wait(0, Some("3"), now), Some(secs(3)));
        assert_eq!(retry_wait(1, Some(" 0 "), now), Some(secs(0)));
        assert_eq!(retry_wait(2, Some("0"), now), None);
        // Longer than is worth waiting for here: the sync tries later.
        assert_eq!(retry_wait(0, Some("120"), now), None);
        assert_eq!(retry_wait(0, Some("10"), now), Some(secs(10)));
        assert_eq!(retry_wait(0, Some("11"), now), None);
        // A date, also one already past; something unreadable is the usual wait.
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 10:00:05 GMT"), now), Some(secs(5)));
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 09:59:00 GMT"), now), Some(secs(0)));
        assert_eq!(retry_wait(0, Some("Fri, 27 Nov 2026 11:00:00 GMT"), now), None);
        assert_eq!(retry_wait(0, Some("soon"), now), Some(secs(1)));
    }

    #[test]
    fn busy_statuses() {
        for code in [429, 500, 502, 503, 504] {
            assert!(busy_status(code), "{code}");
        }
        for code in [400, 401, 403, 404, 405, 409, 501, 507] {
            assert!(!busy_status(code), "{code}");
        }
    }

    /// Answers prepared in advance, one per request; counts the requests.
    fn answers<'a>(list: &'a [(u16, Option<&'a str>)]) -> impl FnMut() -> Result<ureq::Response, ureq::Error> + 'a {
        let mut i = 0;
        move || {
            let (code, retry_after) = list[i];
            i += 1;
            let head = match retry_after {
                Some(v) => format!("HTTP/1.1 {code} X\r\nRetry-After: {v}\r\n\r\n"),
                None => format!("HTTP/1.1 {code} X\r\n\r\n"),
            };
            let resp: ureq::Response = head.parse().unwrap();
            if code < 400 {
                Ok(resp)
            } else {
                Err(ureq::Error::Status(code, resp))
            }
        }
    }

    #[test]
    fn sends_again_while_busy_and_gives_up_after_two_more() {
        // Each try again is logged.
        let tmp = tempfile::tempdir().unwrap();
        let mut env = crate::paths::TestEnv::new();
        env.set("LUDONIMBUS_HOME", tmp.path());

        let list = [(503, Some("0")), (200, None)];
        assert_eq!(send("test", answers(&list)).unwrap().status(), 200);

        let list = [(429, Some("0")), (502, Some("0")), (503, Some("0")), (200, None)];
        match send("test", answers(&list)) {
            Err(ureq::Error::Status(503, _)) => {}
            other => panic!("{other:?}"),
        }

        // Not for the moment: answered as it came, once.
        let list = [(404, None), (200, None)];
        match send("test", answers(&list)) {
            Err(ureq::Error::Status(404, _)) => {}
            other => panic!("{other:?}"),
        }
        // Asks for longer than is worth waiting for.
        let list = [(503, Some("60")), (200, None)];
        match send("test", answers(&list)) {
            Err(ureq::Error::Status(503, r)) => assert_eq!(r.header("Retry-After"), Some("60")),
            other => panic!("{other:?}"),
        }
    }
}
