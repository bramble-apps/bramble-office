//! cargo run -p pg-core --example cells -- in.pages
//! Prints every table's cells (and formulas) by page.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).expect("open");
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    for (pi, page) in l.pages.iter().enumerate() {
        for d in pg_core::flow::page_drawables(&p, page) {
            if let kn_model::DrawableKind::Table(t) = &d.kind {
                println!("page {} table {} {}x{} merges {:?} rows {:?} cols {:?}", pi + 1, d.id, t.rows, t.cols, t.merges.iter().map(|m| (m.row, m.col, m.rows, m.cols)).collect::<Vec<_>>(), t.row_heights, t.col_widths);
                for row in 0..t.rows {
                    let cells: Vec<String> = (0..t.cols)
                        .map(|c| {
                            let v = t.cells.get((row * t.cols + c) as usize).cloned().unwrap_or_default();
                            match p.doc.formula_text(d.id, row, c) {
                                Some(f) => format!("{v} (={f})"),
                                None => v,
                            }
                        })
                        .collect();
                    println!("  {}", cells.join(" | "));
                }
            }
        }
    }
}
