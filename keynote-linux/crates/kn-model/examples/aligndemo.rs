//! aligndemo <in.key> <out.key> r0 c0 r1 c1 left|right|center|justify|auto: align cells of the
//! first table on slide 1 (checks).
use kn_model::edit::History;
use kn_model::tablecells::CellAlign;
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let t = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let n: Vec<u32> = a[3..7].iter().map(|x| x.parse().unwrap()).collect();
    let align = match a[7].as_str() {
        "left" => CellAlign::Left,
        "right" => CellAlign::Right,
        "center" => CellAlign::Center,
        "justify" => CellAlign::Justify,
        _ => CellAlign::Auto,
    };
    let mut h = History::default();
    d.begin(&mut h, "Align");
    d.set_cells_alignment(&mut h, t, (n[0], n[1], n[2], n[3]), align).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
