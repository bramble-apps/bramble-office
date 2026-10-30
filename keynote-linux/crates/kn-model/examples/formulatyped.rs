//! formulatyped <in.key> <out.key>: type the Mac's R30 formulas into C2:C13 and D2 of the first
//! table on slide 1, as a user would (checks).
use kn_model::edit::History;
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let t = d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let mut h = History::default();
    let typed = ["=B2+B3", "=B2*B3-B4", "=(B2+B3)/B4", "=B2^2", "=SUM(B2:B4)*2", "=ROUND(B2/B3,2)", "=IF(B2>B3,\"yes\",\"no\")", "=B2&\" items\"", "=ABS(-5)", "=AVERAGE(B2,B4)", "=B2%", "=COUNTA(B2:B4)"];
    for (i, f) in typed.iter().enumerate() {
        d.begin(&mut h, "Formula");
        d.set_formula_text(&mut h, t, 1 + i as u32, 2, f).unwrap();
        d.end(&mut h);
    }
    d.begin(&mut h, "Formula");
    d.set_formula_text(&mut h, t, 1, 3, "=B2+B3").unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
