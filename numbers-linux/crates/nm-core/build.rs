// Tests that read the private test files (Apple-made samples and the Mac research, kept on the
// developer's computer and never published) run only where those files are; elsewhere they are
// reported as ignored.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(private_testdata)");
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let probe = dir.join("../../testdata");
    // (not rerun-if-changed on the folder: cargo would walk all of it on every build, through
    // testdata/research into the Drime mount, and hang whenever the mount does)
    println!("cargo::rerun-if-changed=build.rs");
    if probe.exists() {
        println!("cargo::rustc-cfg=private_testdata");
    }
}
