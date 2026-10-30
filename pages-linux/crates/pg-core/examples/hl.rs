fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).expect("open");
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    println!("body highlights {:?}", l.body.as_ref().map(|b| &b.tb.highlights));
    for line in &l.pages[0].lines {
        println!("line para {} hl {:?}", line.para, line.piece.tl.highlights);
    }
}
