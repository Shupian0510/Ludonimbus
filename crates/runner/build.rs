// Windows file properties for ludonimbus-run.exe. Code signing (SignPath)
// checks the product name and version of every program it signs, see
// docs/code-signing.md.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = tauri_winres::WindowsResource::new();
        res.set("ProductName", "Ludonimbus")
            .set("FileDescription", "Ludonimbus Steam launch wrapper");
        res.compile().expect("failed to embed the version resource");
    }
}
