//! What changed from A to B: objects added (by id, with type and file), objects whose payload
//! changed, objects removed, and package entries added/removed.
//! cargo run -p kn-format --example objdiff -- a.key b.key
fn main() {
    let a = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    let b = kn_format::KeyFile::open(std::env::args().nth(2).unwrap()).unwrap();
    let ids: std::collections::HashSet<u64> = a.archives().map(|(_, x)| x.id()).collect();
    for (f, x) in b.archives() {
        match a.archive(x.id()) {
            None => println!("+ {:>8} {:<45} {}", x.id(), x.obj().type_name(), f),
            Some(old) => {
                let enc = |ar: &kn_format::Archive| ar.payloads.iter().map(|p| p.encode()).collect::<Vec<_>>();
                if enc(old) != enc(x) {
                    println!("~ {:>8} {:<45} {}", x.id(), x.obj().type_name(), f);
                }
            }
        }
    }
    let bids: std::collections::HashSet<u64> = b.archives().map(|(_, x)| x.id()).collect();
    for (f, x) in a.archives() {
        if !bids.contains(&x.id()) {
            println!("- {:>8} {:<45} {}", x.id(), x.obj().type_name(), f);
        }
    }
    let _ = ids;
    let names: std::collections::HashSet<&str> = a.entries.iter().map(|e| e.name.as_str()).collect();
    let bnames: std::collections::HashSet<&str> = b.entries.iter().map(|e| e.name.as_str()).collect();
    for e in &b.entries {
        if !names.contains(e.name.as_str()) {
            println!("new entry {}", e.name);
        }
    }
    for e in &a.entries {
        if !bnames.contains(e.name.as_str()) {
            println!("removed entry {}", e.name);
        }
    }
}
