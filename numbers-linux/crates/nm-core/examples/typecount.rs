//! Object types in a document and how many of each. usage: typecount file.numbers [filter]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let k = wb.file();
    let mut counts = std::collections::BTreeMap::new();
    for (_, ar) in k.archives() {
        *counts.entry(k.type_name(ar.id()).to_string()).or_insert(0) += 1;
    }
    for (t, n) in counts {
        if a.get(2).is_none_or(|f| t.contains(f.as_str())) {
            println!("{n:4} {t}");
        }
    }
}
