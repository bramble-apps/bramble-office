//! formulanew <in.key>: insert a Basic table on slide 1, then Insert > Formula > Sum into
//! several cells (checks that formulas work in a freshly inserted table).
use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::tableformula::Func;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides()[0].slide;
    let mut h = History::default();
    d.begin(&mut h, "Table");
    let t = d.insert(&mut h, s, &NewObject::BASIC_TABLE).unwrap();
    d.end(&mut h);
    println!("can insert formula: {}", d.can_insert_formula(t));
    for (r, c) in [(4u32, 1u32), (1, 3), (2, 2)] {
        d.begin(&mut h, "Formula");
        let res = d.insert_formula(&mut h, t, r, c, Func::from_menu("Sum").unwrap());
        d.end(&mut h);
        println!("({r},{c}): {res:?}");
    }
}
