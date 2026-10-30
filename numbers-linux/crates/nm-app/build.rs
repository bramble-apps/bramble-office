// The Windows program's icon and name.
fn main() {
    windows_resources();
}

/// Windows: the program's icon and name (Explorer, the taskbar, Properties).
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("data/icons/values.ico");
    res.set("ProductName", "Values");
    res.set("FileDescription", "Values spreadsheets");
    res.set("CompanyName", "Bramble Office");
    res.set("LegalCopyright", "GPL-3.0-only");
    if let Err(e) = res.compile() {
        println!("cargo::warning=no Windows icon: {e}");
    }
}
