//! Edits on the user's real deck (skipped when testdata-private is absent; heavy, run with
//! `cargo test -p kn-model --test real_deck -- --ignored`).
mod common;
use common::*;

use kn_model::edit::History;
use kn_model::format::{FillSpec, TextProp};
use kn_model::insert::{BasicShape, NewObject};
use kn_model::{Color, Document, DrawableKind, Fill};

#[test]
#[ignore]
fn edit_real_deck() {
    let path = testdata("../testdata-private/real-deck.key");
    if !path.exists() {
        return;
    }
    let mut doc = Document::open(&path).unwrap();
    let before = problems(&doc.file);
    let slides = doc.slides();
    let mut h = History::default();
    let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    doc.begin(&mut h, "edits");
    assert!(doc.set_fill(&mut h, 220644, &FillSpec::Color(red)));
    assert!(doc.set_text_prop(&mut h, 220556, &TextProp::Size(120.0)));
    assert!(doc.set_text_prop(&mut h, 220556, &TextProp::Color(red)));
    assert!(doc.set_text_prop(&mut h, 220556, &TextProp::Bold(true)));
    assert!(doc.set_shadow(&mut h, 193356, true));
    assert!(doc.set_opacity(&mut h, 193356, 0.5));
    let star = doc.insert(&mut h, slides[4].slide, &NewObject::Shape(BasicShape::Star)).unwrap();
    let tb = doc.insert(&mut h, slides[4].slide, &NewObject::TextBox).unwrap();
    let clip = doc.copy_objects(&[193356, star]).unwrap();
    let pasted = doc.paste_objects(&mut h, slides[0].slide, &clip, 0.0).unwrap();
    doc.end(&mut h);
    let d2 = roundtrip(&doc, &before);
    let s2 = d2.slides();
    let sc = d2.scene(&s2, 4).unwrap();
    let sh = sc.drawables.iter().find(|d| d.id == 220644).unwrap();
    let DrawableKind::Shape(s) = &sh.kind else { panic!() };
    assert_eq!(s.fill, Fill::Color(red));
    let t = sc.drawables.iter().find(|d| d.id == 220556).unwrap();
    let DrawableKind::Shape(s) = &t.kind else { panic!() };
    assert_eq!(s.text.as_ref().unwrap().paragraphs[0].runs[0].style.size, 120.0);
    assert!(s.text.as_ref().unwrap().paragraphs[0].runs[0].style.bold);
    assert!(sc.drawables.iter().any(|d| d.id == star) && sc.drawables.iter().any(|d| d.id == tb));
    let sc0 = d2.scene(&s2, 0).unwrap();
    assert!(pasted.iter().all(|p| sc0.drawables.iter().any(|d| d.id == *p)));
    d2.file.save("/tmp/claude-1000/kn-edit/real-edited.key").ok();
}
