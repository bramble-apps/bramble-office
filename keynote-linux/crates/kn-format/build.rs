// Tests that read the private test files (Apple-made samples and the Mac research, kept on the
// developer's computer and never published) run only where those files are; elsewhere they are
// reported as ignored.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(private_testdata)");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let probe = dir.join("../../testdata");
    println!("cargo::rerun-if-changed={}", probe.display());
    if probe.exists() {
        println!("cargo::rustc-cfg=private_testdata");
    }
}
