//! cargo run -p pg-core --example whoref -- file.pages id : which archives refer to object id
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    for (f, ar) in p.file().archives() {
        for (i, pl) in ar.payloads.iter().enumerate() {
            let mut v = Vec::new();
            pl.collect_refs(&mut v);
            let declared = ar.object_references(i);
            if v.contains(&id) || declared.contains(&id) {
                println!("{f} {} {} payload_ref={} declared={}", ar.id(), p.file().type_name(ar.id()), v.contains(&id), declared.contains(&id));
            }
        }
    }
}
