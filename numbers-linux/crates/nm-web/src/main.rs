#[cfg(target_arch = "wasm32")]
fn main() {
    nm_app::start_web();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("nm-web targets wasm32-unknown-unknown; build with `trunk build --release`");
}
