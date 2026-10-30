//! Files to check Insert > Table in real Keynote.
//! cargo run -p kn-model --example table_t14 -- <testdata dir> <out dir>
use std::path::Path;

use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::tablecells::CellInput;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let td = Path::new(&a[1]);
    let out = Path::new(&a[2]);
    std::fs::create_dir_all(out).unwrap();

    // T14: 00-base, slide duplicated, a Basic table on each slide; slide 1's gets values
    let mut d = Document::open(td.join("00-base.key")).unwrap();
    let mut h = History::default();
    d.duplicate_slide(&mut h, 0).unwrap();
    let s = d.slides();
    d.begin(&mut h, "Insert Table");
    let t1 = d.insert(&mut h, s[0].slide, &NewObject::BASIC_TABLE).unwrap();
    d.end(&mut h);
    d.begin(&mut h, "Insert Table");
    d.insert(&mut h, s[1].slide, &NewObject::BASIC_TABLE).unwrap();
    d.end(&mut h);
    d.begin(&mut h, "Cells");
    for (r, c, v) in [(0, 0, "Fruit"), (0, 1, "Count"), (0, 2, "Price"), (1, 0, "Apples"), (1, 1, "42"), (1, 2, "1.25"), (2, 0, "Pears"), (2, 1, "7")] {
        d.set_cell(&mut h, t1, r, c, &CellInput::parse(v)).unwrap();
    }
    d.end(&mut h);
    d.save_as(out.join("T14-insert-table.key")).unwrap();
    println!("wrote T14-insert-table.key");

    // T14b: the feature catalog with a Basic table inserted on slide 2
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    let s = d.slides();
    d.begin(&mut h, "Insert Table");
    let t = d.insert(&mut h, s[1].slide, &NewObject::BASIC_TABLE).unwrap();
    d.end(&mut h);
    d.begin(&mut h, "Cells");
    d.set_cell(&mut h, t, 0, 0, &CellInput::parse("Inserted on Linux")).unwrap();
    d.set_cell(&mut h, t, 1, 0, &CellInput::parse("3.5")).unwrap();
    d.end(&mut h);
    d.save_as(out.join("T14b-catalog-insert-table.key")).unwrap();
    println!("wrote T14b-catalog-insert-table.key");
}
