//! tschfields [filter]: the fields of the TSCH.Generated.* style / non-style archives, with kinds.
fn main() {
    let f = std::env::args().nth(1).unwrap_or_default();
    let s = kn_format::schema::schema();
    let mut tys: Vec<_> = s.types.iter().filter(|t| t.name.contains("TSCH") && (t.name.contains("Style") || t.name.contains("Generated"))).collect();
    tys.sort_by(|a, b| a.name.cmp(&b.name));
    for t in tys {
        let mut fs: Vec<_> = t.fields.values().filter(|d| d.name.contains(&f)).collect();
        fs.sort_by_key(|d| d.num);
        println!("{} ({})", t.name, fs.len());
        for d in fs {
            let ty = d.type_idx.map(|i| s.types[i as usize].name.as_str()).unwrap_or("");
            println!("  {} {} {:?} {}", d.num, d.name, d.kind, ty);
        }
    }
}
