//! cargo run -p pg-core --example rows -- file.docx|pages page : each table's row heights on a page.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let p = if path.extension().is_some_and(|e| e == "docx") { pg_core::import::import_file(path).unwrap() } else { pg_core::Pages::open(path).unwrap() };
    let page: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let pg = &l.pages[page - 1];
    for d in pg_core::flow::page_drawables(&p, pg) {
        if let kn_model::DrawableKind::Table(t) = &d.kind {
            println!("table at {:.2},{:.2} {:.2}x{:.2}: rows {:?}", d.geom.x, d.geom.y, d.geom.w, d.geom.h, t.row_heights.iter().map(|h| (h * 100.0).round() / 100.0).collect::<Vec<_>>());
            println!("   cols {:?} padding {:?}", t.col_widths, t.cell_styles.get(t.cols as usize).map(|s| s.padding));
        }
    }
}
