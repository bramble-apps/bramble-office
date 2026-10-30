//! Everything reachable from an object (payload references + archive object_references),
//! with type and file; objects in DocumentStylesheet.iwa / Document.iwa are listed, not followed.
//! cargo run -p kn-format --example graph -- file.key <id> [stop-id...]
use std::collections::BTreeMap;

use kn_format::KeyFile;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = KeyFile::open(&a[1]).unwrap();
    for (i, (n, s)) in k.slide_nodes().iter().enumerate() {
        println!("slide {} node {n} slide {s} in {}", i + 1, k.file_of(*s).unwrap_or("?"));
    }
    let root: u64 = a[2].parse().unwrap();
    let stop: Vec<u64> = a[3..].iter().filter_map(|s| s.parse().ok()).collect();
    let mut seen: BTreeMap<u64, (String, &str, bool)> = BTreeMap::new();
    let mut todo = vec![root];
    while let Some(id) = todo.pop() {
        if seen.contains_key(&id) {
            continue;
        }
        let Some(ar) = k.archive(id) else {
            seen.insert(id, ("MISSING".into(), "?", false));
            continue;
        };
        let file = k.file_of(id).unwrap().to_string();
        let shared = stop.contains(&id) || file.ends_with("DocumentStylesheet.iwa") || file.ends_with("/Document.iwa") || file.contains("Slide");
        seen.insert(id, (file, k.type_name(id), shared));
        if shared {
            continue;
        }
        let mut refs = Vec::new();
        for (i, p) in ar.payloads.iter().enumerate() {
            p.collect_refs(&mut refs);
            refs.extend(ar.object_references(i));
        }
        todo.extend(refs);
    }
    for (id, (f, t, shared)) in &seen {
        println!("{}{id:>7} {t:<45} {f}", if *shared { "  S" } else { "   " });
    }
    println!("{} objects, {} owned", seen.len(), seen.values().filter(|v| !v.2).count());
}
