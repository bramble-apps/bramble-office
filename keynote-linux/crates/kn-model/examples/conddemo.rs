//! conddemo <in.key> <out.key> r0 c0 r1 c1 value [comparison look]: a highlighting rule
//! ("Greater than" / "Red Fill" by default) on a block of the first table on slide 1 (checks).
use kn_model::edit::History;
use kn_model::tablecond::{Cmp, LOOKS};
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let t = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let n: Vec<u32> = a[3..7].iter().map(|x| x.parse().unwrap()).collect();
    let v: f64 = a[7].parse().unwrap();
    let mut h = History::default();
    d.begin(&mut h, "Rule");
    let cmp = a.get(8).and_then(|c| Cmp::ALL.into_iter().find(|x| x.name() == c)).unwrap_or(Cmp::Greater);
    let look = a.get(9).and_then(|l| LOOKS.iter().find(|x| x.name == l)).unwrap_or(&LOOKS[0]);
    let cells = d.add_highlight_rule(&mut h, t, (n[0], n[1], n[2], n[3]), cmp, v, look).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("{cells} cells");
}
