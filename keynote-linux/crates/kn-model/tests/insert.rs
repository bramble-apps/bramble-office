mod common;
use common::*;

use kn_model::edit::History;
use kn_model::insert::{BasicShape, NewObject};
use kn_model::{Document, DrawableKind};

fn png(w: u32, h: u32) -> Vec<u8> {
    // smallest valid PNG header is enough for size detection; the renderer may not decode it
    std::fs::read(testdata("../crates/kn-model/tests/data/test.png")).unwrap_or_else(|_| {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
        v
    })
}

fn insert_all(name: &str) {
    let mut doc = Document::open(testdata(name)).unwrap();
    let before = problems(&doc.file);
    let slides = doc.slides();
    let slide = slides[0].slide;
    let n0 = doc.scene(&slides, 0).unwrap().drawables.len();
    let mut h = History::default();
    let mut objs = vec![NewObject::TextBox, NewObject::Line { arrow: false }, NewObject::Line { arrow: true }, NewObject::Image(png(400, 200), "pic.png".into())];
    objs.extend(BasicShape::ALL.iter().map(|s| NewObject::Shape(*s)));
    let mut ids = Vec::new();
    for o in &objs {
        doc.begin(&mut h, "Insert");
        ids.push(doc.insert(&mut h, slide, o).unwrap_or_else(|e| panic!("{o:?}: {e}")));
        doc.end(&mut h);
    }
    let doc2 = roundtrip(&doc, &before);
    let sc = doc2.scene(&doc2.slides(), 0).unwrap();
    assert_eq!(sc.drawables.len(), n0 + objs.len());
    let size = doc2.slide_size();
    for id in &ids {
        let d = sc.drawables.iter().find(|d| d.id == *id).unwrap();
        assert!((d.geom.x + d.geom.w / 2.0 - size.w / 2.0).abs() < 2.0, "{:?} not centred", d.kind);
    }
    let text = sc.drawables.iter().find(|d| d.id == ids[0]).unwrap();
    let DrawableKind::Shape(s) = &text.kind else { panic!() };
    assert_eq!(s.text.as_ref().unwrap().text, "Text");
    let arrow = sc.drawables.iter().find(|d| d.id == ids[2]).unwrap();
    let DrawableKind::Shape(s) = &arrow.kind else { panic!() };
    assert!(s.is_line && s.head.is_some());
    let img = sc.drawables.iter().find(|d| d.id == ids[3]).unwrap();
    let DrawableKind::Image(i) = &img.kind else { panic!() };
    assert_eq!((i.natural.w, i.natural.h), (400.0, 200.0));
    assert!(doc2.data(i.data_id.unwrap()).is_some());

    // undo removes them all again, including the picture file
    let entries = doc.file.entries.len();
    while doc.undo(&mut h) {}
    assert_eq!(doc.scene(&slides, 0).unwrap().drawables.len(), n0);
    assert_eq!(doc.file.entries.len(), entries - 1);
    roundtrip(&doc, &before);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_objects_minimal() {
    insert_all("00-base.key");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_objects_other_theme() {
    insert_all("keywriter-template-White.key");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn copy_paste_duplicate() {
    let mut doc = Document::open(testdata("11-two-slides.key")).unwrap();
    let before = problems(&doc.file);
    let s = doc.slides();
    let mut h = History::default();
    doc.begin(&mut h, "Insert");
    let shape = doc.insert(&mut h, s[0].slide, &NewObject::Shape(BasicShape::Star)).unwrap();
    let pic = doc.insert(&mut h, s[0].slide, &NewObject::Image(png(10, 20), "p.png".into())).unwrap();
    doc.end(&mut h);
    let clip = doc.copy_objects(&[shape, pic]).unwrap();
    doc.begin(&mut h, "Paste");
    let dup = doc.paste_objects(&mut h, s[0].slide, &clip, 20.0).unwrap();
    let other = doc.paste_objects(&mut h, s[1].slide, &clip, 0.0).unwrap();
    doc.end(&mut h);
    let doc2 = roundtrip(&doc, &before);
    let s2 = doc2.slides();
    let a = doc2.scene(&s2, 0).unwrap();
    let g0 = a.drawables.iter().find(|d| d.id == shape).unwrap().geom;
    let g1 = a.drawables.iter().find(|d| d.id == dup[0]).unwrap().geom;
    assert_eq!((g1.x, g1.y), (g0.x + 20.0, g0.y + 20.0));
    let b = doc2.scene(&s2, 1).unwrap();
    assert!(b.drawables.iter().any(|d| d.id == other[0]));
    let p = b.drawables.iter().find(|d| d.id == other[1]).unwrap();
    let DrawableKind::Image(i) = &p.kind else { panic!() };
    assert!(doc2.data(i.data_id.unwrap()).is_some());

    // paste into a different document re-points styles
    let mut other_doc = Document::open(testdata("keywriter-template-White.key")).unwrap();
    let before2 = problems(&other_doc.file);
    let os = other_doc.slides();
    let clip = doc.copy_objects(&[shape, pic]).unwrap();
    other_doc.begin(&mut h, "Paste");
    let ids = other_doc.paste_objects(&mut h, os[0].slide, &clip, 0.0).unwrap();
    other_doc.end(&mut h);
    let d3 = roundtrip(&other_doc, &before2);
    let sc = d3.scene(&d3.slides(), 0).unwrap();
    assert!(sc.drawables.iter().any(|d| d.id == ids[0]));
    let p = sc.drawables.iter().find(|d| d.id == ids[1]).unwrap();
    let DrawableKind::Image(i) = &p.kind else { panic!() };
    assert_eq!(d3.data(i.data_id.unwrap()).map(|b| b.len()), Some(png(10, 20).len()));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn duplicate_with_placeholders_selected() {
    let mut doc = Document::open(testdata("keywriter-template-White.key")).unwrap();
    let before = problems(&doc.file);
    let s = doc.slides();
    let slide = s[4].slide;
    let mut h = History::default();
    doc.begin(&mut h, "Insert");
    for o in [NewObject::Shape(BasicShape::Star), NewObject::Shape(BasicShape::Oval), NewObject::Line { arrow: true }] {
        doc.insert(&mut h, slide, &o).unwrap();
    }
    doc.end(&mut h);
    let sc = doc.scene(&s, 4).unwrap();
    let all: Vec<u64> = sc.drawables.iter().filter(|d| !d.from_template).map(|d| d.id).collect();
    let n = all.len();
    let clip = doc.copy_objects(&all).unwrap();
    assert_eq!(clip.objects.len(), 3);
    doc.begin(&mut h, "Duplicate");
    let ids = doc.paste_objects(&mut h, slide, &clip, 20.0).unwrap();
    doc.end(&mut h);
    assert_eq!(ids.len(), 3);
    let d2 = roundtrip(&doc, &before);
    assert_eq!(d2.scene(&d2.slides(), 4).unwrap().drawables.iter().filter(|d| !d.from_template).count(), n + 3);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_charts() {
    for name in ["00-base.key", "keywriter-template-White.key", "06-chart-column.key"] {
        let mut doc = Document::open(testdata(name)).unwrap();
        let before = problems(&doc.file);
        let s = doc.slides();
        let mut h = History::default();
        doc.begin(&mut h, "Insert Chart");
        let a = doc.insert(&mut h, s[0].slide, &NewObject::Chart { pie: false }).unwrap();
        let b = doc.insert(&mut h, s[0].slide, &NewObject::Chart { pie: true }).unwrap();
        doc.end(&mut h);
        let d2 = roundtrip(&doc, &before);
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        // Keynote's default data: 2 regions x 4 months; the pie 6 wedges (series by column)
        for (id, rows, series) in [(a, 2, 2), (b, 1, 6)] {
            let c = sc.drawables.iter().find(|d| d.id == id).unwrap();
            let DrawableKind::Chart(c) = &c.kind else { panic!() };
            assert_eq!((c.values.len(), c.series.len()), (rows, series), "{name}");
        }
        let entries = doc.file.entries.len();
        while doc.undo(&mut h) {}
        assert!(doc.file.entries.len() <= entries);
        roundtrip(&doc, &before);
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn connection_lines_outside_a_slide() {
    // drawables that aren't in a slide's z-order (as in Pages): the line still follows its ends
    use kn_model::insert::Connection;
    let mut doc = Document::open(testdata("00-base.key")).unwrap();
    let slide = doc.slides()[0].slide;
    let mut h = History::default();
    doc.begin(&mut h, "Insert");
    let a = doc.insert(&mut h, slide, &NewObject::Shape(BasicShape::Rectangle)).unwrap();
    let b = doc.insert(&mut h, slide, &NewObject::Shape(BasicShape::Rectangle)).unwrap();
    let ga = doc.file.obj(a).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap();
    doc.set_frame(&mut h, b, kn_model::Geometry { x: ga.x + 400.0, ..ga });
    let line = doc.insert_connection_line(&mut h, slide, Connection::Straight, Some((a, b))).unwrap();
    let ids = [a, b, line];
    doc.touch(&mut h, slide);
    doc.file.obj_mut(slide).unwrap().retain_msgs("drawables_z_order", |d| !d.u64("identifier").is_some_and(|x| ids.contains(&x)));
    doc.end(&mut h);
    let frame = |doc: &Document| doc.file.obj(line).and_then(|o| o.find_super("TSD.DrawableArchive")).map(kn_model::scene::geometry).unwrap();
    let before = frame(&doc);
    assert!((before.w - 400.0 + ga.w).abs() < 1.0, "spans the gap between the shapes: {before:?}");

    doc.begin(&mut h, "Move");
    doc.move_by(&mut h, b, 100.0, 0.0);
    // the slide no longer lists them, so the slide-based call leaves the line alone
    doc.update_connections(&mut h, slide, &[b]);
    assert_eq!(frame(&doc).w, before.w);
    let all: Vec<_> = doc.file.objects_of("TSD.ConnectionLineArchive").map(|(i, _)| i).collect();
    // not attached to the moved object: untouched
    doc.update_connections_in(&mut h, &all, &[slide]);
    assert_eq!(frame(&doc).w, before.w);
    doc.update_connections_in(&mut h, &all, &[b]);
    doc.end(&mut h);
    let after = frame(&doc);
    assert!((after.w - before.w - 100.0).abs() < 1.0, "the end followed the shape: {before:?} -> {after:?}");
    assert!((after.x - before.x).abs() < 1.0);
}
