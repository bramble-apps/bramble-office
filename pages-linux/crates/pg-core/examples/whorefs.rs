//! cargo run -p pg-core --example whorefs -- file.pages id : the objects referring to id
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    let k = p.file();
    for (_, ar) in k.archives() {
        for i in 0..ar.payloads.len() {
            if ar.object_references(i).contains(&id) {
                println!("{} ({})", ar.id(), k.type_name(ar.id()));
            }
        }
    }
}
