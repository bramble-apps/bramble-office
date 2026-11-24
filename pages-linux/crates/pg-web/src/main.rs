#[cfg(target_arch = "wasm32")]
fn main() {
    pg_app::start_web();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("pg-web targets wasm32-unknown-unknown; build with `trunk build --release`");
}
