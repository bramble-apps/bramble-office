fn main() {
    use kn_model::{Document, DrawableKind};
    let dir = std::path::PathBuf::from("testdata/ref");
    let first = |d: &Document| d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let raw = |d: &Document| {
        let m = d.file.obj(d.file.obj(first(d)).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
        kn_model::table::raw_cell_records(&d.file, &m).into_iter().filter(|((r, c), _)| (*r, *c) == (1, 1)).map(|(_, b)| b).next()
    };
    let k = Document::open(dir.join("R13-merge-1-merged.key")).unwrap();
    let mut d = Document::open(dir.join("R13-merge-0-before.key")).unwrap();
    let t = first(&d);
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "m");
    d.merge_cells(&mut h, t, kn_model::CellRange { row: 1, col: 1, rows: 2, cols: 2 }).unwrap();
    d.end(&mut h);
    println!("keynote {:02x?}\nours    {:02x?}", raw(&k), raw(&d));
    let ku = Document::open(dir.join("R13-merge-2-unmerged.key")).unwrap();
    d.begin(&mut h, "u");
    d.unmerge_cells(&mut h, t, 1, 1);
    d.end(&mut h);
    println!("keynote unmerged {:02x?}\nours unmerged    {:02x?}", raw(&ku), raw(&d));
}
