//! Make one slide per layout in a deck and save it (for visual checks / Mac tests).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let mut h = kn_model::edit::History::default();
    let layouts = d.layouts();
    d.begin(&mut h, "t");
    for (i, (l, _)) in layouts.iter().enumerate() {
        d.new_slide_from_layout(&mut h, *l, i + 1).unwrap();
    }
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("{} layouts", layouts.len());
}
