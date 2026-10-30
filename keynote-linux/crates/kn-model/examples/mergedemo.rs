//! mergedemo <in.key> <out.key> row col rows cols [row col rows cols ...]: merge cell blocks
//! of the first table on slide 1 (checks).
use kn_model::edit::History;
use kn_model::{CellRange, Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let t = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let n: Vec<u32> = a[3..].iter().map(|x| x.parse().unwrap()).collect();
    let mut h = History::default();
    for b in n.chunks(4) {
        d.begin(&mut h, "Merge");
        d.merge_cells(&mut h, t, CellRange { row: b[0], col: b[1], rows: b[2], cols: b[3] }).unwrap();
        d.end(&mut h);
    }
    d.save_as(&a[2]).unwrap();
}
