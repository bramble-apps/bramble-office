//! whorefs file id: the objects that reference `id`, with their type.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let want: u64 = a[2].parse().unwrap();
    for (_, ar) in k.archives() {
        let id = ar.info.u64("identifier").unwrap_or(0);
        let mut refs = Vec::new();
        for p in &ar.payloads {
            p.collect_refs(&mut refs);
        }
        if refs.contains(&want) {
            println!("#{id} {} ({} refs)", k.type_name(id), refs.len());
        }
    }
}
