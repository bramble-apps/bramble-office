//! Try a sync server the way Settings' check does (`dav_check <url> <user>`, password in
//! NT_DAV_PASSWORD; add `webpki` to use the built-in certificate list instead of the system's).
fn main() {
    let mut a = std::env::args().skip(1);
    let (url, user) = (a.next().expect("url"), a.next().unwrap_or_default());
    if a.next().as_deref() == Some("webpki") {
        nt_core::sync::use_system_certificates(false);
    }
    let s = nt_core::sync::Settings { on: true, url, user, password: std::env::var("NT_DAV_PASSWORD").unwrap_or_default(), folder: "Bramble Notes".into() };
    match nt_core::sync::test_connection(&s) {
        None => println!("ok"),
        Some(e) => println!("{e}"),
    }
}
