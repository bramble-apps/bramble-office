//! formulademo <in.key> <out.key> row col Func [row col Func ...]: Insert > Formula into the
//! first table on slide 1 (checks). Func is the menu name (Sum, Average, Minimum, ...).
use kn_model::edit::History;
use kn_model::tableformula::Func;
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let t = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let mut h = History::default();
    for c in a[3..].chunks(3) {
        d.begin(&mut h, "Formula");
        d.insert_formula(&mut h, t, c[0].parse().unwrap(), c[1].parse().unwrap(), Func::from_menu(&c[2]).expect("function")).unwrap();
        d.end(&mut h);
    }
    d.save_as(&a[2]).unwrap();
}
