//! autorows file.key: each table's default row height, stored row sizes and which rows fit their content.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for (_, info) in k.objects_of("TST.TableInfoArchive") {
        let Some(model) = info.ref_id("tableModel").and_then(|m| k.obj(m)) else { continue };
        let t = kn_model::table::table(&k, info).unwrap();
        println!("default {:?} heights {:?}\nauto {:?}", model.f64("default_row_height"), t.row_heights, t.auto_rows);
    }
}
