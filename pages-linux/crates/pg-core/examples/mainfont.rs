//! cargo run -p pg-core --example mainfont -- file.pages : the font, size and line spacing of the
//! longest paragraph on page 1
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let Some(b) = p.body_storage().and_then(|s| pg_core::body::build(p.file(), s, None, Default::default())) else { return };
    let paras: std::collections::BTreeSet<usize> = l.pages.first().map(|pg| pg.lines.iter().map(|ln| ln.para).collect()).unwrap_or_default();
    let best = paras.iter().filter_map(|i| b.tb.paragraphs.get(*i)).filter(|q| q.runs.len() == 1 || q.runs.windows(2).all(|w| w[0].style.font_name == w[1].style.font_name && w[0].style.size == w[1].style.size)).max_by_key(|q| q.end - q.start);
    if let Some(q) = best {
        if let Some(r0) = q.runs.first() {
            let r = match q.line_spacing { kn_model::LineSpacing::Relative(v) => format!("rel {v}"), o => format!("{o:?}") };
            println!("{}\t{}\t{}\t{}", r0.style.font_name, r0.style.size, r, q.end - q.start);
        }
    }
}
