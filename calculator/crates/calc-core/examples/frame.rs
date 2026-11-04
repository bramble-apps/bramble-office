//! Prints the engine's JSON for a request (dev aid): cargo run -p calc-core --example frame -- '{"cmd":"frame","w":393,"h":852}'
fn main() {
    let mut s = calc_core::session::Session::new(None);
    for a in std::env::args().skip(1) {
        let v: serde_json::Value = serde_json::from_str(&a).expect("json");
        println!("{}", serde_json::to_string_pretty(&calc_core::api::call(&mut s, &v)).unwrap());
    }
}
