//! cargo run -p pg-core --example paras -- file.pages [needle] : body paragraphs with style, list, level, label (around `needle`)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let b = pg_core::body::build(p.file(), p.body_storage().unwrap(), None, Default::default()).unwrap();
    let hit = a.get(2).and_then(|n| b.tb.paragraphs.iter().position(|pa| b.tb.text[pa.start..pa.end].contains(n.as_str())));
    let (from, to) = match hit { Some(h) => (h.saturating_sub(4), h + 12), None => (0, b.tb.paragraphs.len()) };
    for (i, (pa, ex)) in b.tb.paragraphs.iter().zip(&b.extra).enumerate().skip(from).take(to - from) {
        let lname = ex.list.and_then(|l| p.file().obj(l)).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.str("name")).unwrap_or("").to_string();
        println!("p{i} style {:?} list {:?} {lname:?} level {} label {:?} | {:?}", ex.style.map(|s| pg_core::edit::named_style(&p.doc, Some(s)).1), ex.list, pa.level, pa.label, b.tb.text[pa.start..pa.end].chars().take(50).collect::<String>());
    }
}
