//! whatis file.key id...: each object's type, file and which objects refer to it (and by which field).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for id in a[2..].iter().map(|s| s.parse::<u64>().unwrap()) {
        println!("{id}: {} in {:?}", if k.obj(id).is_some() { k.type_name(id) } else { "(none)" }, k.file_of(id));
        if let Some(m) = k.obj(id) {
            let mut refs = Vec::new();
            m.collect_refs(&mut refs);
            println!("  refers to {refs:?}");
        }
    }
}
