//! cargo run -p kn-model --example inventory -- file.key : fonts, transitions, builds, media used
use kn_model::{Document, DrawableKind};
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let slides = doc.slides();
    let (mut fonts, mut trans, mut builds, mut media) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new(), Vec::new());
    for i in 0..slides.len() {
        let sc = doc.scene(&slides, i).unwrap();
        if let Some(t) = &sc.transition { *trans.entry(format!("{} {:.1}s auto={}", t.effect, t.duration, t.automatic)).or_insert(0) += 1; }
        for b in &sc.builds { *builds.entry(format!("{} {} [{}]", b.kind, b.effect, b.delivery)).or_insert(0) += 1; }
        for d in &sc.drawables {
            match &d.kind {
                DrawableKind::Shape(s) => if let Some(t) = &s.text { for p in &t.paragraphs { for r in &p.runs { *fonts.entry(r.style.font_name.clone()).or_insert(0) += 1; } } },
                DrawableKind::Movie(m) => media.push(format!("slide {} movie {:?} poster {:?} audio_only {} -> {:?}", i+1, m.movie_data, m.poster_data, m.audio_only, m.movie_data.and_then(|id| doc.data_name(id)))),
                _ => {}
            }
        }
    }
    println!("FONTS:"); for (k,v) in fonts { println!("  {v:4} {k}"); }
    println!("TRANSITIONS:"); for (k,v) in trans { println!("  {v:4} {k}"); }
    println!("BUILDS:"); for (k,v) in builds { println!("  {v:4} {k}"); }
    println!("MEDIA:"); for m in media { println!("  {m}"); }
}
