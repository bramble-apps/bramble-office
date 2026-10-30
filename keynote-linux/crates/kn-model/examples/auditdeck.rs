//! auditdeck <out.key>: one slide with a text box, a shape, a picture, a table and a chart
//! (for KN_AUDIT runs); prints each object's id.
use kn_model::edit::History;
use kn_model::insert::{BasicShape, NewObject};
use kn_model::Document;

fn main() {
    let out = std::env::args().nth(1).expect("out.key");
    let mut d = Document::open("testdata/ref/R10-standard.key").unwrap();
    let s = d.slides()[0].slide;
    let png = std::fs::read("crates/kn-app/data/icons/keydeck-256.png").unwrap();
    let mut h = History::default();
    d.begin(&mut h, "Audit deck");
    for (name, obj) in [
        ("text", NewObject::TextBox),
        ("shape", NewObject::Shape(BasicShape::ALL[0])),
        ("image", NewObject::Image(png, "picture.png".into())),
        ("table", NewObject::BASIC_TABLE),
        ("chart", NewObject::Chart { pie: false }),
    ] {
        let id = d.insert(&mut h, s, &obj).unwrap();
        println!("{name} {id}");
    }
    d.end(&mut h);
    d.save_as(&out).unwrap();
}
