//! Message type id of each object's first payload, for given ids.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for id in &a[2..] {
        let id: u64 = id.parse().unwrap();
        if let Some(ar) = k.archive(id) {
            println!("{id}: type {} ({}) in {}", ar.type_id(0), ar.obj().type_name(), k.file_of(id).unwrap_or(""));
        }
    }
}
