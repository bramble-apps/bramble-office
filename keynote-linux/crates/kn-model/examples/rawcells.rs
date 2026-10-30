//! cargo run -p kn-model --example rawcells -- file : raw records of rich text cells (hex).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for (_, info) in k.objects_of("TST.TableInfoArchive") {
        let Some(model) = info.ref_id("tableModel").and_then(|m| k.obj(m)) else { continue };
        let recs = kn_model::table::cell_records(&k, model);
        let raw: std::collections::HashMap<_, _> = kn_model::table::raw_cell_records(&k, model).into_iter().collect();
        for (rc, r) in recs.iter().filter(|r| r.1.rich.is_some()).take(3) {
            let hex: Vec<String> = raw[rc].iter().map(|b| format!("{b:02x}")).collect();
            println!("{rc:?} text_style {:?} cell_style {:?}: {}", r.text_style, r.cell_style, hex.join(" "));
        }
    }
}
