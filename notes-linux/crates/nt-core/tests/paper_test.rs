//! Drawings: our strokes round-trip through the store; Apple's drawings come in with their picture.

mod common;

use nt_core::import;
use nt_core::model::*;
use nt_core::paper;
use nt_core::store::Store;

fn drawing() -> Drawing {
    let pt = |x: f32, y: f32, t: f32| InkPoint { x, y, t, size: 3.5, force: 0.6, azimuth: 0.3, altitude: 1.2, opacity: 1.0 };
    Drawing {
        width: 768.0,
        height: 358.0,
        strokes: vec![
            Stroke { id: "S1".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 3.5, points: vec![pt(10.0, 20.0, 0.0), pt(40.5, 22.25, 0.016)], transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask: None },
            Stroke { id: "S2".into(), ink: InkKind::Marker, color: [1.0, 0.8, 0.0, 0.5], width: 12.0, points: vec![pt(5.0, 5.0, 0.0)], transform: [1.0, 0.0, 0.0, 1.0, 30.0, -4.0], mask: Some(vec![[1.0, 2.0], [3.5, 4.0], [1.0, 2.0]]) },
        ],
        shapes: vec![
            Shape { id: "M1".into(), kind: ShapeKind::Arrow, frame: [10.0, 10.0, 100.0, 40.0], rotation: 0.25, stroke: [1.0, 0.0, 0.0, 1.0], line_width: 4.0, dash: 1, shadow: true, points: vec![[10.0, 10.0], [110.0, 50.0]], arrows: 2, ..Default::default() },
            Shape { id: "M2".into(), kind: ShapeKind::Text, frame: [200.0, 30.0, 150.0, 24.0], fill: [1.0, 1.0, 0.8, 1.0], text: "Café ✓".into(), font: "Helvetica".into(), font_size: 18.0, text_color: [0.1, 0.2, 0.3, 1.0], align: 1, ..Default::default() },
            Shape { id: "M3".into(), kind: ShapeKind::Star, frame: [5.0, 200.0, 60.0, 60.0], fill: [0.0, 0.5, 1.0, 0.5], sides: 5, ..Default::default() },
            Shape { id: "M4".into(), kind: ShapeKind::Loupe, frame: [400.0, 100.0, 80.0, 80.0], magnify: 2.5, ..Default::default() },
            Shape {
                id: "M5".into(),
                kind: ShapeKind::Signature,
                frame: [500.0, 250.0, 200.0, 60.0],
                ink: vec![Stroke { id: "G1".into(), color: [0.0, 0.0, 0.0, 1.0], width: 2.0, points: vec![pt(0.0, 0.5, 0.0), pt(1.0, 0.25, 0.4)], ink: InkKind::Reed, transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask: None }],
                ..Default::default()
            },
        ],
    }
}

#[test]
fn ink_round_trip() {
    let d = drawing();
    assert_eq!(paper::decode_ink(&paper::encode_ink(&d)), Some(d));
    // drawings saved before shapes existed read with none
    let plain = Drawing { shapes: Vec::new(), ..drawing() };
    assert_eq!(paper::decode_ink(&paper::encode_ink(&plain)).unwrap().shapes, vec![]);
    assert_eq!(paper::decode_ink(b"\x0a\x03abc"), None);
    assert_eq!(paper::decode_ink(&[0xff, 0xff]), None);
}

#[test]
fn drawing_saved_and_reopened_as_strokes() {
    let dir = common::scratch("paper-store");
    let mut st = Store::open(&dir).unwrap();
    let mut body = Body::from_lines("Sketch", &[]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let png = dir.join("render.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nfirst").unwrap();
    // the editor's new drawing: strokes + its render as the fallback picture
    let a = Attachment { id: "D1D1D1D1-0000-4000-8000-000000000001".into(), uti: UTI_PAPER.into(), kind: AttachmentKind::Drawing, drawing: Some(drawing()), fallback: Some(png.clone()), width: 768.0, height: 358.0, ..Default::default() };
    note.attachments.push(a.clone());
    note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, UTI_PAPER)], ..Default::default() });
    st.save_note(&mut note).unwrap();
    let back = st.note(&note.id).unwrap();
    let ba = back.attachment(&a.id).unwrap();
    assert_eq!(ba.drawing, Some(drawing()));
    assert!(ba.fallback.as_ref().unwrap().starts_with(&dir) && ba.fallback.as_ref().unwrap().ends_with("FallbackImage.png"));
    assert_eq!(back.thumbnail(), ba.fallback.as_deref());

    // edited: a stroke erased, a new render
    let mut n2 = back.clone();
    let d2 = n2.attachments.iter_mut().find(|x| x.id == a.id).unwrap();
    d2.drawing.as_mut().unwrap().strokes.remove(0);
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nsecond").unwrap();
    d2.fallback = Some(png.clone());
    st.save_note(&mut n2).unwrap();
    let again = st.note(&note.id).unwrap();
    let ga = again.attachment(&a.id).unwrap();
    assert_eq!(ga.drawing.as_ref().unwrap().strokes.len(), 1);
    assert_eq!(std::fs::read(ga.fallback.as_ref().unwrap()).unwrap(), b"\x89PNG\r\n\x1a\nsecond");
    assert!(ga.fallback.as_ref().unwrap().parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("2_"));

    // a Markup edit alone (text box retyped, shape removed) is saved too
    let mut n3 = again.clone();
    let d3 = n3.attachments.iter_mut().find(|x| x.id == a.id).unwrap().drawing.as_mut().unwrap();
    d3.shapes[1].text = "Edited".into();
    d3.shapes.remove(2);
    let want = d3.clone();
    st.save_note(&mut n3).unwrap();
    assert_eq!(st.note(&note.id).unwrap().attachment(&a.id).unwrap().drawing.as_ref(), Some(&want));

    // locked, the strokes are encrypted and come back with the password
    st.set_password("pw", None).unwrap();
    st.lock_note(&note.id, "pw").unwrap();
    assert!(st.note(&note.id).unwrap().attachment(&a.id).unwrap().drawing.is_none());
    let mut u = st.unlock_note(&note.id, "pw").unwrap();
    assert_eq!(u.attachment(&a.id).unwrap().drawing, Some(want));
    u.attachments[0].drawing.as_mut().unwrap().strokes.clear();
    st.save_note(&mut u).unwrap();
    assert_eq!(st.unlock_note(&note.id, "pw").unwrap().attachment(&a.id).unwrap().drawing.as_ref().unwrap().strokes.len(), 0);
}

#[test]
fn apple_drawing_comes_with_its_picture() {
    let Some(r) = common::research() else { return eprintln!("skipped: research not found") };
    let lib = import::read_library(&common::group_container(&r, "paper-apple")).unwrap();
    let n = lib.notes.iter().find(|n| n.title == "33 Drawing").unwrap();
    let a = n.attachments.iter().find(|a| a.kind == AttachmentKind::Drawing).unwrap();
    assert_eq!(a.uti, UTI_PAPER);
    // PaperKit strokes aren't decoded yet (research round NO3): the picture is what we have
    assert!(a.drawing.is_none());
    assert!(a.fallback.as_ref().is_some_and(|f| f.ends_with("FallbackImage.png")));
    assert_eq!((a.width, a.height), (768.0, 358.0));
}
