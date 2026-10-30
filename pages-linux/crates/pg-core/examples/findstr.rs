//! cargo run -p pg-core --example findstr -- needle file.pages… : objects whose decoded strings contain `needle`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let needle = &a[1];
    let bytes: Vec<String> = needle.bytes().map(|b| b.to_string()).collect();
    let as_list = bytes.join(", ");
    for f in &a[2..] {
        let Ok(p) = pg_core::Pages::open(f) else {
            println!("{f}: can't open (password?)");
            continue;
        };
        for (_, ar) in p.file().archives() {
            let s = format!("{:?}", ar.payloads);
            if s.contains(needle.as_str()) || s.contains(&as_list) {
                println!("{f}: {} {}", ar.id(), p.file().type_name(ar.id()));
            }
        }
    }
}
