//! shadows <out.key>: three shapes with a drop, a contact and a curved shadow; prints the ids.
use kn_model::edit::History;
use kn_model::insert::{BasicShape, NewObject};
use kn_model::{Color, Document, Shadow};

fn main() {
    let out = std::env::args().nth(1).expect("out.key");
    let mut d = Document::open("testdata/ref/R10-standard.key").unwrap();
    let s = d.slides()[0].slide;
    let mut h = History::default();
    d.begin(&mut h, "Shadows");
    for (i, kind) in [0u8, 1, 2].into_iter().enumerate() {
        let id = d.insert(&mut h, s, &NewObject::Shape(BasicShape::ALL[0])).unwrap();
        let mut g = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| x.id == id).unwrap().geom;
        g.x = 200.0 + i as f32 * 560.0;
        g.y = 300.0;
        g.w = 400.0;
        g.h = 300.0;
        d.set_frame(&mut h, id, g);
        let sh = Shadow { color: Color::BLACK, angle: 315.0, offset: if kind == 0 { 20.0 } else { 10.0 }, blur: 16.0, opacity: 0.7, kind, height: 0.2, curve: 0.6 };
        d.set_shadow_spec(&mut h, id, Some(&sh));
        println!("{kind} {id}");
    }
    d.end(&mut h);
    d.save_as(&out).unwrap();
}
