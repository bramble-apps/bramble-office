//! List a slide's objects: id, kind, placeholder, first text. cargo run --example objs -- deck.key <slide>
use kn_model::DrawableKind;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let n: usize = a[2].parse().unwrap();
    for x in d.scene(&s, n - 1).unwrap().drawables {
        let what = match &x.kind {
            DrawableKind::Shape(sh) => format!("shape text={:?}", sh.text.as_ref().map(|t| t.text.chars().take(40).collect::<String>())),
            DrawableKind::Image(_) => "image".into(),
            DrawableKind::Movie(_) => "movie".into(),
            DrawableKind::Table(_) => "table".into(),
            DrawableKind::Chart(_) => "chart".into(),
            _ => "other".into(),
        };
        println!("#{} tmpl={} ph={:?} {what}", x.id, x.from_template, x.placeholder);
    }
}
