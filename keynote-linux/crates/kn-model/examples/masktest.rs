//! Mac test: mask the first image of slide N with a shape. masktest in.key out.key <slide> <Shape name>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let n: usize = a[3].parse().unwrap();
    let img = d.scene(&s, n - 1).unwrap().drawables.into_iter().find(|x| matches!(x.kind, kn_model::DrawableKind::Image(_)) && !x.from_template).unwrap().id;
    let shape = kn_model::insert::BasicShape::ALL.into_iter().find(|b| b.name() == a[4]).unwrap();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "Mask");
    assert!(d.mask_with_shape(&mut h, img, shape));
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
