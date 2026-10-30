//! cellraw <file.key> [slide]: the raw storage_version 5 records of the first table's cells.
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = Document::open(&a[1]).unwrap();
    let i: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1) - 1;
    let s = d.slides();
    let t = d.scene(&s, i).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
    let mut recs = kn_model::table::raw_cell_records(&d.file, model);
    recs.sort();
    for ((r, c), rec) in recs {
        let hex: String = rec.iter().map(|b| format!("{b:02x}")).collect();
        println!("r{r} c{c}: {hex}");
    }
}
