//! cargo run -p pg-core --example fonts -- list.txt : fonts the documents' body text uses and whether each is installed
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fonts = kn_render::fonts::Fonts::new();
    let mut use_count: std::collections::BTreeMap<String, (usize, std::collections::BTreeSet<usize>)> = Default::default();
    for (n, line) in std::fs::read_to_string(&a[1]).unwrap().lines().enumerate() {
        let Ok(p) = pg_core::Pages::open(line) else { continue };
        let Some(st) = p.body_storage() else { continue };
        let Some(b) = pg_core::body::build(p.file(), st, None, Default::default()) else { continue };
        for pa in &b.tb.paragraphs {
            for r in &pa.runs {
                let e = use_count.entry(r.style.font_name.clone()).or_default();
                e.0 += r.end - r.start;
                e.1.insert(n);
            }
        }
    }
    for (f, (chars, docs)) in use_count {
        println!("{} {:>8} chars in {:>3} docs  {}", if fonts.is_installed(&f) { "ok     " } else { "MISSING" }, chars, docs.len(), f);
    }
}
