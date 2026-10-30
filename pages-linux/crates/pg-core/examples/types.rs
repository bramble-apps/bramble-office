//! cargo run -p pg-core --example types -- file.pages [prefix] : archives per .iwa file (with a type filter)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let mut by: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (f, ar) in p.file().archives() {
        if a.get(2).is_none_or(|x| f.contains(x.as_str())) {
            by.entry(f.to_string()).or_default().push(format!("{} {}", ar.id(), p.file().type_name(ar.id())));
        }
    }
    for (f, v) in by {
        println!("{f}: {}", v.join(", "));
    }
}
