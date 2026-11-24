//! The cloud speed test against a local folder and a real WebDAV server
//! (`dav-server`, in memory): every phase moves data, and no test file is
//! left behind, also when the test is cancelled.

use ludonimbus_core::cloud::store::Store;
use ludonimbus_core::cloud::{self, local::LocalFolder};
use ludonimbus_core::config::CloudConfig;
use ludonimbus_core::gamebackup::speedtest::{self, SpeedOptions, Verdict};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn start_dav() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let handler = dav_server::DavHandler::builder()
                .filesystem(dav_server::memfs::MemFs::new())
                .locksystem(dav_server::fakels::FakeLs::new())
                .build_handler();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let handler = handler.clone();
                        async move { Ok::<_, Infallible>(handler.handle(req).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
    });
    rx.recv().unwrap()
}

fn small() -> SpeedOptions {
    SpeedOptions {
        total_bytes: 2 * 1024 * 1024,
        object_bytes: 256 * 1024,
        threads: 3,
        max_secs: 30,
    }
}

fn leftovers(store: &dyn Store) -> Vec<String> {
    store
        .list("")
        .unwrap()
        .into_iter()
        .filter(|e| e.name.starts_with(speedtest::PREFIX))
        .map(|e| e.name)
        .collect()
}

fn check_full_run(store: &dyn Store) {
    let cancel = AtomicBool::new(false);
    let r = speedtest::run(store, &small(), &cancel, &|_| {}).unwrap();
    for p in [&r.upload_serial, &r.upload_parallel, &r.download_serial, &r.download_parallel] {
        let p = p.as_ref().unwrap();
        assert_eq!(p.objects, 8, "{p:?}");
        assert_eq!(p.bytes, 2 * 1024 * 1024);
        assert!(!p.capped && p.error.is_none());
        assert!(p.bytes_per_sec > 0.0);
    }
    assert_eq!(r.upload_parallel.as_ref().unwrap().threads, 3);
    assert_ne!(r.verdict, Verdict::Failed);
    assert!(r.cleanup_error.is_none());
    assert!(leftovers(store).is_empty(), "test files left: {:?}", leftovers(store));
}

#[test]
fn speed_test_on_local_folder_and_webdav() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("LUDONIMBUS_HOME", tmp.path().join("home"));

    let local = LocalFolder::new(tmp.path().join("cloud"));
    std::fs::create_dir_all(tmp.path().join("cloud")).unwrap();
    // A file left by an earlier run that was killed is removed first.
    local.write(&format!("{}old-s0000", speedtest::PREFIX), b"x").unwrap();
    check_full_run(&local);
    // Nothing but the test files was ever written.
    assert_eq!(std::fs::read_dir(tmp.path().join("cloud")).unwrap().count(), 0);

    let port = start_dav();
    let dav = cloud::build_store(
        &CloudConfig::WebDav {
            url: format!("http://127.0.0.1:{port}/"),
            username: "player".into(),
            folder: "Ludonimbus".into(),
        },
        Some("secret"),
        Duration::from_secs(10),
    )
    .unwrap();
    check_full_run(dav.as_ref());

    // Cancelled during the first upload phase: stops, reports it, cleans up.
    let cancel = AtomicBool::new(false);
    let err = speedtest::run(&local, &small(), &cancel, &|p| {
        if p.phase == "uploadSerial" && p.done_bytes >= 512 * 1024 {
            cancel.store(true, Ordering::SeqCst);
        }
    })
    .unwrap_err();
    assert!(format!("{err:#}").contains("cancel") || format!("{err:#}").contains("取消"), "{err:#}");
    assert!(leftovers(&local).is_empty());

    // A phase that hits its time limit is marked as capped.
    let slow = SpeedOptions { max_secs: 1, total_bytes: 64 * 1024 * 1024, object_bytes: 64 * 1024, threads: 1 };
    struct Slow<'a>(&'a LocalFolder);
    impl Store for Slow<'_> {
        fn kind(&self) -> &'static str { "localFolder" }
        fn label(&self) -> String { self.0.label() }
        fn check(&self) -> anyhow::Result<()> { self.0.check() }
        fn list(&self, d: &str) -> anyhow::Result<Vec<ludonimbus_core::cloud::store::Entry>> { self.0.list(d) }
        fn read(&self, p: &str) -> anyhow::Result<Option<Vec<u8>>> { self.0.read(p) }
        fn download(&self, p: &str, d: &std::path::Path) -> anyhow::Result<()> { self.0.download(p, d) }
        fn write(&self, p: &str, b: &[u8]) -> anyhow::Result<()> {
            std::thread::sleep(Duration::from_millis(150));
            self.0.write(p, b)
        }
        fn upload(&self, p: &str, s: &std::path::Path) -> anyhow::Result<()> { self.0.upload(p, s) }
        fn remove(&self, p: &str) -> anyhow::Result<()> { self.0.remove(p) }
    }
    let r = speedtest::run(&Slow(&local), &slow, &AtomicBool::new(false), &|_| {}).unwrap();
    let us = r.upload_serial.unwrap();
    assert!(us.capped && us.objects > 0 && us.objects < 1024, "{us:?}");
    assert_eq!(r.download_serial.unwrap().objects, us.objects);
    assert!(leftovers(&local).is_empty());
}
