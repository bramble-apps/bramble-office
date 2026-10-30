//! cargo run -p pg-core --example pw -- in.pages password
fn main() {
    let a: Vec<String> = std::env::args().collect();
    println!("plain open: {}", pg_core::Pages::open(&a[1]).is_ok());
    match pg_core::Pages::open_with_password(&a[1], &a[2], "") {
        Ok(p) => println!("with password: {:?}", p.body_text()),
        Err(e) => println!("with password failed: {e}"),
    }
}
