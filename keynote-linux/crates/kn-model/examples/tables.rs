//! cargo run -p kn-model --example tables -- file.key : each table's slide, id and size
use kn_model::{Document, DrawableKind};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let slides = doc.slides();
    for i in 0..slides.len() {
        let sc = doc.scene(&slides, i).unwrap();
        for d in &sc.drawables {
            if let DrawableKind::Table(t) = &d.kind {
                println!("slide {} table {} {}x{} at ({:.0},{:.0})", i + 1, d.id, t.rows, t.cols, d.geom.x, d.geom.y);
            }
        }
    }
}
