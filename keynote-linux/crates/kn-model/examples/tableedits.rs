//! tableedits <out.key>: R30's formula table with a row and a column added and a row deleted
//! (formulas move), plus R32's table sorted by Q2 and B filled down (checks).
use kn_model::edit::History;
use kn_model::tablerows::Axis;
use kn_model::{Document, DrawableKind};

fn table(d: &Document) -> u64 {
    d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
    let mut d = Document::open(root.join("R30-formulas.key")).unwrap();
    let t = table(&d);
    let mut h = History::default();
    d.begin(&mut h, "Edits");
    d.table_insert(&mut h, t, Axis::Row, 1, 1).unwrap();
    d.table_insert(&mut h, t, Axis::Column, 1, 1).unwrap();
    d.table_delete(&mut h, t, Axis::Row, 14).unwrap();
    d.end(&mut h);
    d.save_as(format!("{}-formulas.key", a[1])).unwrap();
    let mut d = Document::open(&a[2]).unwrap();
    let t = table(&d);
    d.begin(&mut h, "Sort");
    d.sort_table_rows(&mut h, t, 2, true).unwrap();
    d.autofill(&mut h, t, (1, 1, 2, 1), (1, 1, 4, 1)).unwrap();
    d.end(&mut h);
    d.save_as(format!("{}-sort-fill.key", a[1])).unwrap();
}
