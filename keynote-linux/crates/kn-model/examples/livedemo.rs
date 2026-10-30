//! livedemo <in.key> <out.key>: Insert > Live Video on slide 1 (checks).
use kn_model::edit::History;
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides()[0].slide;
    let mut h = History::default();
    d.begin(&mut h, "Insert Live Video");
    let id = d.insert_live_video(&mut h, s).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    let d2 = Document::open(&a[2]).unwrap();
    let sc = d2.scene(&d2.slides(), 0).unwrap();
    let x = sc.drawables.iter().find(|x| x.id == id).unwrap();
    match &x.kind {
        DrawableKind::Movie(m) => println!("live {} poster {:?} geom {:?}", m.live, m.poster_data, (x.geom.x, x.geom.y, x.geom.w, x.geom.h)),
        _ => println!("not a movie"),
    }
}
