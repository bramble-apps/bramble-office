//! cargo run -p pg-core --example hfbox -- file.pages [page] : margins vs where the header,
//! footer and body lines sit on a page
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let pi: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let s = &l.setup;
    println!("page {}x{} margins L{} R{} T{} B{} header_margin {} footer_margin {} facing {}", s.width, s.height, s.left, s.right, s.top, s.bottom, s.header_margin, s.footer_margin, s.facing_pages);
    let pg = &l.pages[pi];
    for hf in pg.headers.iter().chain(&pg.footers) {
        let lines = &hf.piece.tl.lines;
        let x0 = lines.iter().map(|ln| ln.x).fold(f32::MAX, f32::min);
        println!("{} field {} dx {} dy {} lines {} first x {:?} text h {}", if hf.footer { "footer" } else { "header" }, hf.index, hf.dx, hf.dy, lines.len(), (x0 < f32::MAX).then_some(x0), hf.piece.tl.content_h);
    }
    if let (Some(f), Some(last)) = (pg.lines.first(), pg.lines.last()) {
        println!("body first line y {} x {} / last line y {}", f.dy, f.dx, last.dy);
    }
}
