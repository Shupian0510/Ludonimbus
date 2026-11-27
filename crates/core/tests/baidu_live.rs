//! Store operations against the real Baidu Netdisk. Ignored by default.
//!
//! Sign in first (this saves the token in the Windows Credential Manager):
//!
//! ```text
//! cargo run -p ludonimbus-cli -- cloud baidu --app-key <AppKey> --secret-key <SecretKey> --app-name Ludonimbus
//! ```
//!
//! then run, with the same app values in the environment:
//!
//! ```text
//! LUDONIMBUS_BAIDU_APP_KEY=… LUDONIMBUS_BAIDU_SECRET_KEY=… cargo test -p ludonimbus-core --test baidu_live -- --ignored --nocapture
//! ```
//!
//! Everything is written to a temporary folder inside 「我的应用数据/Ludonimbus」
//! and deleted at the end.

use ludonimbus_core::cloud::baidu::{self, BaiduClient, BaiduPan, Endpoints, TokenSink};
use ludonimbus_core::cloud::store::Store;
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn baidu_live() {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let client = BaiduClient::resolve(&env("LUDONIMBUS_BAIDU_APP_KEY"), &env("LUDONIMBUS_BAIDU_SECRET_KEY"), &env("LUDONIMBUS_BAIDU_APP_NAME"))
        .expect("set LUDONIMBUS_BAIDU_APP_KEY / LUDONIMBUS_BAIDU_SECRET_KEY");
    let token = baidu::load_token().unwrap().expect("sign in with `ludonimbus cloud baidu` first");
    let s = BaiduPan::new(client, Endpoints::default(), token, TokenSink::Keyring, Duration::from_secs(20));
    let dir = format!("live-test-{}", ulid::Ulid::new().to_string().to_lowercase());
    let t = Instant::now();
    let step = |what: &str| println!("[{:>6.1}s] {what}", t.elapsed().as_secs_f64());

    s.check().unwrap();
    let (used, total) = s.quota().unwrap();
    step(&format!("check + quota: used {used:?} / total {total:?}"));

    assert!(s.list(&format!("{dir}/missing")).unwrap().is_empty());
    assert_eq!(s.read(&format!("{dir}/missing.json")).unwrap(), None);
    s.remove(&format!("{dir}/missing.json")).unwrap();
    step("missing folder / file handled");

    // Nested folders that do not exist yet.
    let json = format!("{dir}/games/fe/game.json");
    s.write(&json, b"{\"v\":1}").unwrap();
    step("small write into new nested folders");
    assert_eq!(s.read(&json).unwrap().as_deref(), Some(&b"{\"v\":1}"[..]));
    step("small read");
    let before = s.list(&format!("{dir}/games/fe")).unwrap();
    s.write(&json, b"{\"v\":22}").unwrap();
    let after = s.list(&format!("{dir}/games/fe")).unwrap();
    assert_eq!(after.len(), 1, "overwritten in place, not renamed: {after:?}");
    assert_ne!(before[0].tag, after[0].tag);
    assert_eq!(s.read(&json).unwrap().as_deref(), Some(&b"{\"v\":22}"[..]));
    step("overwrite");

    let tmp = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..9_000_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    let src = tmp.path().join("big.zip");
    std::fs::write(&src, &big).unwrap();
    let zip = format!("{dir}/games/fe/snapshots/big.zip");
    s.upload(&zip, &src).unwrap();
    step("9 MB upload (3 slices)");
    let back = tmp.path().join("back.zip");
    s.download(&zip, &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big);
    step("9 MB download");

    s.remove(&zip).unwrap();
    assert!(s.list(&format!("{dir}/games/fe/snapshots")).unwrap().is_empty());
    s.remove(&dir).unwrap();
    assert!(s.list(&dir).unwrap().is_empty());
    step("cleaned up");
}
