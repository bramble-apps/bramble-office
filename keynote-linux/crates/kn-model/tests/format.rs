mod common;
use common::*;

use kn_model::edit::History;
use kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use kn_model::{Align, Color, Document, DrawableKind, Fill};

fn red() -> Color {
    Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
}

fn shape_id(doc: &Document) -> u64 {
    let s = doc.slides();
    let sc = doc.scene(&s, 0).unwrap();
    sc.drawables.iter().find(|d| matches!(d.kind, DrawableKind::Shape(_)) && d.placeholder.is_none() && !d.from_template).unwrap().id
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn shape_style_variation_roundtrip_and_undo() {
    let mut doc = Document::open(testdata("02-shape.key")).unwrap();
    let before = problems(&doc.file);
    let id = shape_id(&doc);
    let old_style = doc.file.obj(id).unwrap().find_super("TSD.ShapeArchive").unwrap().ref_id("style").unwrap();
    let mut h = History::default();
    doc.begin(&mut h, "Fill");
    assert!(doc.set_fill(&mut h, id, &FillSpec::Color(red())));
    doc.end(&mut h);
    // a slider drag: many frames, one undo step, one variation
    for i in 0..5 {
        doc.begin_merge(&mut h, "Opacity");
        assert!(doc.set_opacity(&mut h, id, 0.9 - i as f32 * 0.1));
        doc.end(&mut h);
    }
    doc.begin(&mut h, "Border");
    doc.set_stroke(&mut h, id, Some(&StrokeSpec { color: Color::BLACK, width: 4.0, dash: Dash::Dashed }));
    doc.set_shadow(&mut h, id, true);
    doc.set_reflection(&mut h, id, true);
    doc.end(&mut h);

    let doc2 = roundtrip(&doc, &before);
    let s = doc2.slides();
    let d = doc2.scene(&s, 0).unwrap().drawables.into_iter().find(|d| d.id == id).unwrap();
    let DrawableKind::Shape(sh) = &d.kind else { panic!() };
    assert_eq!(sh.fill, Fill::Color(red()));
    assert!((d.opacity - 0.5).abs() < 1e-5);
    let st = sh.stroke.as_ref().unwrap();
    assert_eq!((st.width, st.dash.clone()), (4.0, vec![4.0, 2.0]));
    assert!(d.shadow.is_some());
    assert_eq!(d.reflection, Some(0.5));
    // the theme style itself is untouched; the shape points at a variation of it
    let new_style = doc2.file.obj(id).unwrap().find_super("TSD.ShapeArchive").unwrap().ref_id("style").unwrap();
    assert_ne!(new_style, old_style);
    let v = doc2.file.obj(new_style).unwrap().find_super("TSS.StyleArchive").unwrap();
    assert_eq!(v.bool("is_variation"), Some(true));
    assert_eq!(v.ref_id("parent"), Some(old_style));
    let sheet = doc2.file.stylesheet_id().unwrap();
    assert!(doc2.file.obj(sheet).unwrap().ref_ids("styles").contains(&new_style));

    // undo everything: the shape is back to the theme style, the variations are gone
    let n_archives = doc.file.archives().count();
    while doc.undo(&mut h) {}
    let s = doc.slides();
    let d = doc.scene(&s, 0).unwrap().drawables.into_iter().find(|d| d.id == id).unwrap();
    assert!((d.opacity - 1.0).abs() < 1e-5);
    assert_eq!(doc.file.obj(id).unwrap().find_super("TSD.ShapeArchive").unwrap().ref_id("style"), Some(old_style));
    assert!(doc.file.archives().count() < n_archives);
    assert!(doc.file.obj(new_style).is_none());
    // and redo brings them back
    while doc.redo(&mut h) {}
    let d = doc.scene(&s, 0).unwrap().drawables.into_iter().find(|d| d.id == id).unwrap();
    assert!((d.opacity - 0.5).abs() < 1e-5);
    roundtrip(&doc, &before);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn text_attributes_whole_box() {
    let mut doc = Document::open(testdata("01-text-box.key")).unwrap();
    let before = problems(&doc.file);
    let s = doc.slides();
    let id = doc.scene(&s, 0).unwrap().drawables.iter().find(|d| !d.from_template && d.placeholder.is_none()).unwrap().id;
    let mut h = History::default();
    doc.begin(&mut h, "Text");
    for p in [
        TextProp::Size(50.0),
        TextProp::Bold(true),
        TextProp::Italic(true),
        TextProp::Underline(true),
        TextProp::Color(red()),
        TextProp::Font("Georgia".into()),
        TextProp::Align(Align::Center),
        TextProp::LineSpacing(1.5),
    ] {
        assert!(doc.set_text_prop(&mut h, id, &p), "{p:?}");
    }
    doc.end(&mut h);
    let doc2 = roundtrip(&doc, &before);
    let d = doc2.scene(&doc2.slides(), 0).unwrap().drawables.into_iter().find(|d| d.id == id).unwrap();
    let DrawableKind::Shape(sh) = &d.kind else { panic!() };
    let p = &sh.text.as_ref().unwrap().paragraphs[0];
    let r = &p.runs[0].style;
    assert_eq!((r.size, r.bold, r.italic, r.underline, r.font_name.as_str()), (50.0, true, true, true, "Georgia"));
    assert_eq!(r.color, red());
    assert_eq!(p.align, Align::Center);
    assert_eq!(p.line_spacing, kn_model::LineSpacing::Relative(1.5));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn slide_background() {
    let mut doc = Document::open(testdata("00-base.key")).unwrap();
    let before = problems(&doc.file);
    let s = doc.slides();
    let mut h = History::default();
    doc.begin(&mut h, "Background");
    assert!(doc.set_slide_background(&mut h, s[0].slide, &FillSpec::Color(red())));
    doc.end(&mut h);
    let doc2 = roundtrip(&doc, &before);
    assert_eq!(doc2.scene(&doc2.slides(), 0).unwrap().background, Fill::Color(red()));
    let g = FillSpec::Gradient { from: red(), to: Color::BLACK, angle: 90.0 };
    doc.begin(&mut h, "Background");
    assert!(doc.set_slide_background(&mut h, s[0].slide, &g));
    doc.end(&mut h);
    let doc3 = roundtrip(&doc, &before);
    assert!(matches!(doc3.scene(&doc3.slides(), 0).unwrap().background, Fill::Gradient { .. }));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn successive_text_actions_keep_earlier_attributes() {
    let mut doc = Document::open(testdata("01-text-box.key")).unwrap();
    let s = doc.slides();
    let id = doc.scene(&s, 0).unwrap().drawables.iter().find(|d| !d.from_template && d.placeholder.is_none()).unwrap().id;
    let mut h = History::default();
    for p in [TextProp::Bold(true), TextProp::Size(33.0), TextProp::Italic(true)] {
        doc.begin_merge(&mut h, &format!("{p:?}"));
        assert!(doc.set_text_prop(&mut h, id, &p));
        doc.end(&mut h);
    }
    let d = doc.scene(&s, 0).unwrap().drawables.into_iter().find(|d| d.id == id).unwrap();
    let DrawableKind::Shape(sh) = &d.kind else { panic!() };
    let r = &sh.text.as_ref().unwrap().paragraphs[0].runs[0].style;
    assert_eq!((r.bold, r.size, r.italic), (true, 33.0, true));
}

/// R37: applying the sixth image style changes only ImageArchive.style, as in Keynote's file.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn image_style_like_keynote() {
    use kn_model::{edit::History, Document, DrawableKind};
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r37");
    let theirs = Document::open(dir.join("R37-4-image-style6.key")).unwrap();
    let mut d = Document::open(dir.join("R37-3-image-before-style.key")).unwrap();
    let img = |d: &Document| d.scene(&d.slides(), 2).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Image(_)) && !x.from_template).unwrap().id;
    let i = img(&d);
    let p = d.image_style_presets()[5];
    let mut h = History::default();
    d.begin(&mut h, "Style");
    assert!(d.apply_shape_style(&mut h, i, p));
    d.end(&mut h);
    let style = |d: &Document, i| d.file.obj(i).unwrap().ref_id("style");
    assert_eq!(style(&d, i), style(&theirs, img(&theirs)));
}

/// R41: Advanced Image Fill (a tinted image fill) written as Keynote writes it.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn advanced_image_fill_like_keynote() {
    use kn_model::{edit::History, format::FillSpec, Color, Document, ImageFillTechnique};
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r41");
    let mut d = Document::open(dir.join("R41-3-shape-advimagefill.key")).unwrap();
    let shape = 35843;
    let fill_of = |d: &Document| {
        let st = d.file.obj(shape).unwrap().find_super("TSD.ShapeArchive").unwrap().ref_id("style").unwrap();
        let m = d.file.obj(st).unwrap();
        let m = m.find_super("TSD.ShapeStyleArchive").unwrap_or(m);
        m.msg("shape_properties").unwrap().msg("fill").unwrap().msg("image").unwrap().clone()
    };
    let theirs = fill_of(&d);
    let tint = Color { r: 0.932126, g: 0.134829, b: 0.046555, a: 0.5 };
    let reference = Color { r: 0.65098, g: 0.25098, b: 0.207843, a: 1.0 };
    let mut h = History::default();
    d.begin(&mut h, "Fill");
    assert!(d.set_fill(&mut h, shape, &FillSpec::Image { data_id: 15, technique: ImageFillTechnique::Tile, size: Some((300.0, 300.0)), reference: Some(reference), tint: Some(tint) }));
    d.end(&mut h);
    let ours = fill_of(&d);
    assert_eq!(ours.enum_name("technique"), theirs.enum_name("technique"));
    for f in ["r", "g", "b", "a"] {
        let (o, t) = (ours.msg("tint").unwrap().f32(f).unwrap(), theirs.msg("tint").unwrap().f32(f).unwrap());
        assert!((o - t).abs() < 1e-4, "tint {f}");
    }
    assert_eq!(ours.msg("imagedata").and_then(|m| m.u64("identifier")), Some(15));
    assert_eq!(ours.msg("fillsize").and_then(|m| m.f32("width")), Some(300.0));
    // field order as Keynote's: technique, tint, fillsize, imagedata, …
    let order = |m: &kn_format::Msg| m.fields.iter().map(|f| f.num).collect::<Vec<_>>();
    assert_eq!(order(&ours), order(&theirs));
}
