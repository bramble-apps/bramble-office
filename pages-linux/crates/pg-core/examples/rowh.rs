//! cargo run -p pg-core --example rowh -- file.pages|file.docx : each table's stored and laid-out row heights
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let path = std::path::Path::new(&f);
    let p = if path.extension().is_some_and(|e| e == "docx") { pg_core::import::import_file(path).unwrap() } else { pg_core::Pages::open(path).unwrap() };
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    for pg in &l.pages {
        for o in pg.objects.iter().filter(|o| p.file().type_name(o.id) == "TST.TableInfoArchive") {
            let t = p.file().obj(o.id).and_then(|m| kn_model::table::table(p.file(), m)).unwrap();
            println!("page {} table {}: frame h {:.1}, stored {:?}", pg.index + 1, o.id, o.h, t.row_heights.iter().take(6).map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>());
        }
        for (id, rows) in &pg.fitted_rows {
            let t = p.file().obj(*id).and_then(|m| kn_model::table::table(p.file(), m)).unwrap();
            println!("page {} table {id}: stored {:?}\n   laid out {:?}\n   padding {:?} font {:?}", pg.index + 1, t.row_heights.iter().take(8).map(|v| v.round()).collect::<Vec<_>>(), rows.iter().take(8).map(|v| v.round()).collect::<Vec<_>>(), t.cell_styles.get(2).map(|s| s.padding), t.cell_styles.get(2).map(|s| s.text.size));
        }
    }
}
