// Tests that read the private test files (Apple-made samples and the Mac research, kept on the
// developer's computer and never published) run only where those files are; elsewhere they are
// reported as ignored.
fn main() {
    windows_resources();
    println!("cargo::rustc-check-cfg=cfg(private_testdata)");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let probe = dir.join("../../testdata");
    println!("cargo::rerun-if-changed={}", probe.display());
    if probe.exists() {
        println!("cargo::rustc-cfg=private_testdata");
    }
}

/// Windows: the program's icon and name (Explorer, the taskbar, Properties).
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("data/icons/keydeck.ico");
    res.set("ProductName", "Keydeck");
    res.set("FileDescription", "Keydeck presentations");
    res.set("CompanyName", "Bramble Office");
    res.set("LegalCopyright", "GPL-3.0-only");
    if let Err(e) = res.compile() {
        println!("cargo::warning=no Windows icon: {e}");
    }
}
