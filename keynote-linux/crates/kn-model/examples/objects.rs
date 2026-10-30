//! objects <file.key>: each slide's objects: slide, id, kind.
use kn_model::{Document, DrawableKind};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let slides = doc.slides();
    for i in 0..slides.len() {
        let sc = doc.scene(&slides, i).unwrap();
        for d in &sc.drawables {
            let k = match &d.kind {
                DrawableKind::Shape(s) if s.text.is_some() => "text-shape",
                DrawableKind::Shape(_) => "shape",
                DrawableKind::Image(_) => "image",
                DrawableKind::Table(_) => "table",
                DrawableKind::Chart(_) => "chart",
                DrawableKind::Movie(_) => "movie",
                _ => "other",
            };
            println!("slide {} {} {}", i + 1, d.id, k);
        }
    }
}
