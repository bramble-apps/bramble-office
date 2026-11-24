//! Keydeck in the browser (WebAssembly).
//!
//! Build with `trunk build --release` in `keynote-linux/crates/kn-web` (or `./tools/build-web.sh keydeck`).

#[cfg(target_arch = "wasm32")]
fn main() {
    kn_app::start_web();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("kn-web only runs in the browser: build it with `trunk build --release` in keynote-linux/crates/kn-web");
}
