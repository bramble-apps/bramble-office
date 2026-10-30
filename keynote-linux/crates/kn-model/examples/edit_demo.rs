//! Exercise the editing API on a file and save the result (for rendering checks):
//! cargo run -p kn-model --example edit_demo -- in.key out.key [picture.png]
use kn_model::edit::History;
use kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use kn_model::insert::{BasicShape, NewObject};
use kn_model::{Align, Color, Document};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut doc = Document::open(&a[1]).unwrap();
    let mut h = History::default();
    let slides = doc.slides();
    let slide = slides[0].slide;
    let size = doc.slide_size();
    doc.begin(&mut h, "demo");
    for (i, s) in BasicShape::ALL.iter().enumerate() {
        let id = doc.insert(&mut h, slide, &NewObject::Shape(*s)).unwrap();
        let mut g = kn_model::scene::geometry(doc.file.obj(id).unwrap().find_super("TSD.DrawableArchive").unwrap());
        g.x = 40.0 + (i % 6) as f32 * (size.w - 80.0) / 6.0;
        g.y = 300.0 + (i / 6) as f32 * 300.0;
        g.w = 260.0;
        g.h = 200.0;
        doc.set_frame(&mut h, id, g);
        let c = Color { r: (i as f32) / 12.0, g: 0.4, b: 1.0 - (i as f32) / 12.0, a: 1.0 };
        match i % 4 {
            0 => doc.set_fill(&mut h, id, &FillSpec::Color(c)),
            1 => doc.set_fill(&mut h, id, &FillSpec::Gradient { from: c, to: Color::WHITE, angle: 90.0 }),
            2 => doc.set_stroke(&mut h, id, Some(&StrokeSpec { color: Color::BLACK, width: 6.0, dash: Dash::Dashed })),
            _ => doc.set_shadow(&mut h, id, true),
        };
    }
    let t = doc.insert(&mut h, slide, &NewObject::TextBox).unwrap();
    for p in [TextProp::Size(80.0), TextProp::Bold(true), TextProp::Color(Color { r: 0.8, g: 0.1, b: 0.1, a: 1.0 }), TextProp::Align(Align::Center)] {
        doc.set_text_prop(&mut h, t, &p);
    }
    doc.insert(&mut h, slide, &NewObject::Line { arrow: true }).unwrap();
    if let Some(p) = a.get(3) {
        let bytes = std::fs::read(p).unwrap();
        doc.insert(&mut h, slide, &NewObject::Image(bytes, "picture.png".into())).unwrap();
    }
    if slides.len() > 2 {
        let c = doc.insert(&mut h, slides[2].slide, &NewObject::Chart { pie: false }).unwrap();
        doc.move_by(&mut h, c, -300.0, 0.0);
        let p = doc.insert(&mut h, slides[2].slide, &NewObject::Chart { pie: true }).unwrap();
        doc.move_by(&mut h, p, 500.0, 100.0);
    }
    doc.set_slide_background(&mut h, slide, &FillSpec::Color(Color { r: 0.95, g: 0.95, b: 0.85, a: 1.0 }));
    doc.end(&mut h);
    doc.save_as(&a[2]).unwrap();
}
