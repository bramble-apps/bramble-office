//! Apple's drawings: every NO3 sample decodes to what Notes drew, and what we write decodes back the
//! same and has the shape of Notes' own bundles.

mod common;

use std::path::Path;

use nt_core::model::*;
use nt_core::paperkit;
use nt_core::store::Store;

fn bundle(dir: &Path, sample: &str) -> (Drawing, std::path::PathBuf, String) {
    let (a, id) = common::no3_sample(dir, sample);
    let b = a.join("Paper.bundle");
    (paperkit::read(&b).unwrap_or_else(|| panic!("{sample} decodes")), b, id)
}

/// The research decoder's float32 dump of each stroke record (x y t [width] per point), when the
/// points are all float32.
fn reference_points(attachment: &Path) -> Vec<Vec<f32>> {
    let j = common::json(&attachment.join("paper-decoded.json"));
    let mut out = Vec::new();
    for r in j["references"].as_array().unwrap() {
        for s in r["strokes"].as_array().into_iter().flatten() {
            if s["f4"].as_u64().is_some_and(|m| m == 3 || m == 7) {
                out.push(s["float32"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect());
            }
        }
    }
    out
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Every stroke's points match the research decoder (rounded to 3 places there).
fn matches_reference(sample: &str, dir: &Path, d: &Drawing) {
    let (a, _) = common::no3_sample(dir, sample);
    let refs = reference_points(&a);
    assert!(!refs.is_empty(), "{sample}: reference strokes");
    for r in &refs {
        let stride = if d.strokes.iter().any(|s| s.points.len() * 4 == r.len()) { 4 } else { 3 };
        let s = d.strokes.iter().find(|s| s.points.len() * stride == r.len() && near(s.points[0].x, r[0], 0.002) && near(s.points[0].y, r[1], 0.002));
        let s = s.unwrap_or_else(|| panic!("{sample}: a stroke starting at {},{}", r[0], r[1]));
        for (p, q) in s.points.iter().zip(r.chunks_exact(stride)) {
            assert!(near(p.x, q[0], 0.002) && near(p.y, q[1], 0.002) && near(p.t, q[2], 0.002), "{sample}: {p:?} vs {q:?}");
            if stride == 4 {
                assert!(near(p.size, paperkit::drawn_width(s.ink, q[3]), 0.002));
            }
        }
    }
}

#[test]
fn notes_drawings_decode() {
    let Some(dir) = common::no3_drawings() else { return eprintln!("skipped: NO3 drawings not found") };
    let black = [0.0, 0.0, 0.0, 1.0];

    let (d, ..) = bundle(&dir, "D1_");
    assert_eq!((d.width, d.height, d.strokes.len(), d.shapes.len()), (768.0, 187.0, 1, 0));
    let s = &d.strokes[0];
    assert_eq!((s.ink, s.color, s.points.len(), s.mask.is_none()), (InkKind::Pen, black, 61, true));
    // drawn widths (PencilKit's stored 3.173 draws 2.35 pt)
    assert!(near(s.points[0].size, 2.35, 0.001) && s.width == s.points.iter().map(|p| p.size).fold(0.0, f32::max));
    matches_reference("D1_", &dir, &d);

    let (d, ..) = bundle(&dir, "D2_");
    assert_eq!((d.strokes.len(), d.strokes[0].ink, d.strokes[0].points.len()), (1, InkKind::Marker, 61));
    assert!(near(d.strokes[0].width, 16.5, 1e-4));
    let (d, ..) = bundle(&dir, "D3_");
    assert_eq!((d.strokes.len(), d.strokes[0].ink), (1, InkKind::Pencil));
    assert!(near(d.strokes[0].width, 1.2, 1e-4));
    // per-point opacity (0x7fff = opaque): pencil is light, the pen opaque
    let mut o: Vec<f32> = d.strokes[0].points.iter().map(|p| p.opacity).collect();
    o.sort_by(f32::total_cmp);
    assert!(near(o[o.len() / 2], 5898.0 / 32767.0, 1e-4), "{o:?}");
    assert!(bundle(&dir, "D1_").0.strokes[0].points.iter().all(|p| p.opacity == 1.0));

    // 3 colours x 3 widths
    let (d, ..) = bundle(&dir, "D4_");
    assert_eq!(d.strokes.len(), 9);
    let red = [1.0, 0.149_131_42, 0.0, 1.0];
    let blue = [0.016_804_177, 0.198_351_03, 1.0, 1.0];
    for c in [red, blue, black] {
        assert_eq!(d.strokes.iter().filter(|s| s.color == c).count(), 3, "{c:?}");
    }
    let mut widths: Vec<f32> = d.strokes.iter().map(|s| (s.width * 100.0).round() / 100.0).collect();
    widths.sort_by(f32::total_cmp);
    widths.dedup();
    assert_eq!(widths, vec![2.2, 2.6, 4.0]);
    matches_reference("D4_", &dir, &d);

    // the pixel eraser: one stroke left in two pieces, each with its visible outline
    let (d, ..) = bundle(&dir, "D5_");
    assert_eq!(d.strokes.len(), 2);
    for s in &d.strokes {
        assert!(s.mask.as_ref().is_some_and(|m| m.len() == 55));
        assert!(s.points.len() < 61);
    }
    let xs = |s: &Stroke| (s.points.first().unwrap().x, s.points.last().unwrap().x);
    let mut pieces: Vec<(f32, f32)> = d.strokes.iter().map(xs).collect();
    pieces.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (a, b) = (pieces[0], pieces[1]);
    assert!(a.0 > 150.0 && near(a.1, 245.88, 0.01), "{a:?}"); // from index 36.7 to the end
    assert!(near(b.0, 30.37, 0.01) && b.1 < 120.0, "{b:?}"); // from the start to index 23.8

    // snapped shapes stay ink strokes
    let (d, ..) = bundle(&dir, "D6_");
    let mut n: Vec<usize> = d.strokes.iter().map(|s| s.points.len()).collect();
    n.sort();
    assert_eq!(n, vec![58, 128]);
    matches_reference("D6_", &dir, &d);

    // straight strokes + a line shape with an arrow end
    let (d, ..) = bundle(&dir, "D8_");
    assert_eq!((d.strokes.len(), d.shapes.len()), (2, 1));
    let l = &d.shapes[0];
    assert_eq!((l.kind, l.arrows, l.line_width, l.stroke), (ShapeKind::Arrow, 2, 5.0, black));
    let f = l.frame;
    assert_eq!(l.points, vec![[f[0], f[1] + f[3]], [f[0] + f[2], f[1]]]);
    matches_reference("D8_", &dir, &d);

    // Monoline strokes, a filled rectangle, a text box
    let (d, ..) = bundle(&dir, "33_");
    assert_eq!(d.strokes.len(), 3);
    assert!(d.strokes.iter().all(|s| s.ink == InkKind::Monoline && near(s.width, 1.0, 1e-6)));
    let rect = d.shapes.iter().find(|s| s.kind == ShapeKind::Rect).unwrap();
    assert_eq!((rect.line_width, rect.stroke, rect.frame[2], rect.frame[3]), (5.0, black, 150.0, 150.0));
    assert!(near(rect.fill[0], 0.76, 0.001) && rect.fill[3] == 1.0);
    let text = d.shapes.iter().find(|s| s.kind == ShapeKind::Text).unwrap();
    assert_eq!((text.text.as_str(), text.font.as_str(), text.font_size, text.text_color), ("Bramble text", "Helvetica", 24.0, black));
    matches_reference("33_", &dir, &d);
}

/// Strokes and shapes, ids aside (our bundle gets new record ids).
fn same(a: &Drawing, b: &Drawing) {
    assert_eq!((a.width, a.height, a.strokes.len(), a.shapes.len()), (b.width, b.height, b.strokes.len(), b.shapes.len()));
    for (x, y) in a.strokes.iter().zip(&b.strokes) {
        assert_eq!((x.ink, x.color, &x.mask), (y.ink, y.color, &y.mask));
        assert!(near(x.width, y.width, 1e-5), "{} vs {}", x.width, y.width);
        assert_eq!(x.points.len(), y.points.len());
        for (p, q) in x.points.iter().zip(&y.points) {
            assert!(near(p.x, q.x, 1e-4) && near(p.y, q.y, 1e-4) && near(p.t, q.t, 1e-6) && near(p.size, q.size, 1e-5), "{p:?} vs {q:?}");
        }
    }
    for (x, y) in a.shapes.iter().zip(&b.shapes) {
        let y = Shape { id: x.id.clone(), ..y.clone() };
        let close = |p: &[f32], q: &[f32]| p.len() == q.len() && p.iter().zip(q).all(|(a, b)| near(*a, *b, 1e-3));
        assert!(close(&x.frame, &y.frame) || matches!(x.kind, ShapeKind::Line | ShapeKind::Arrow), "{:?} vs {:?}", x.frame, y.frame);
        assert!(close(&x.points.concat(), &y.points.concat()), "{:?} vs {:?}", x.points, y.points);
        assert_eq!(Shape { frame: x.frame, points: x.points.clone(), ..y }, *x);
    }
}

/// Record kinds and keys, Notes' bundle vs ours.
fn layout(b: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(format!("file:{}?immutable=1", b.join("Database/data.sqlite3").display()), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI).unwrap();
    let mut st = c.prepare("SELECT Id, Data, RetainCount FROM Reference").unwrap();
    let mut v: Vec<String> = st
        .query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, i64>(2)?)))
        .unwrap()
        .flatten()
        .map(|(id, data, rc)| {
            let b = if data.starts_with(b"crdt") { &data[8..] } else { &data[..] };
            let m = kn_format::Msg::parse(b, None).unwrap();
            let top = m.fields.iter().find(|f| f.num == 1).and_then(|f| if let kn_format::Val::Bytes(b) = &f.val { kn_format::Msg::parse(b, None).ok() } else { None });
            let kinds: Vec<u32> = top.map(|t| t.fields.iter().map(|f| f.num).collect()).unwrap_or_default();
            let keys: Vec<String> = m
                .fields
                .iter()
                .find(|f| f.num == 6)
                .and_then(|f| if let kn_format::Val::Bytes(b) = &f.val { kn_format::Msg::parse(b, None).ok() } else { None })
                .map(|t| t.fields.iter().filter(|f| f.num == 2).filter_map(|f| if let kn_format::Val::Bytes(b) = &f.val { Some(String::from_utf8_lossy(b).into_owned()) } else { None }).collect())
                .unwrap_or_default();
            format!("{} {kinds:?} {keys:?} rc{rc}", if id.len() == 1 { "root" } else if id[0] == 1 { "canvas" } else { "obj" })
        })
        .collect();
    v.sort();
    v
}

#[test]
fn our_bundles_round_trip_every_sample() {
    let Some(dir) = common::no3_drawings() else { return eprintln!("skipped: NO3 drawings not found") };
    let out = common::scratch("paperkit-round-trip");
    for sample in ["D1_", "D2_", "D3_", "D4_", "D5_", "D6_", "D8_", "33_"] {
        let (d, apple, id) = bundle(&dir, sample);
        let ours = out.join(format!("{sample}.bundle"));
        let root = paperkit::write(&ours, &id, &d).unwrap();
        assert_eq!(paperkit::fingerprint(&ours), Some(root));
        let back = paperkit::read(&ours).unwrap();
        same(&d, &back);
        // and again: what we wrote writes the same
        let again = out.join(format!("{sample}-2.bundle"));
        paperkit::write(&again, &id, &back).unwrap();
        same(&back, &paperkit::read(&again).unwrap());
        // laid out like Notes' (one replica, no edit history; eraser pieces become plain masked strokes,
        // and our text box keeps one style run, so those two compare by what they hold)
        if !matches!(sample, "D5_" | "33_") {
            assert_eq!(layout(&ours), layout(&apple), "{sample}");
        }
    }
}

#[test]
fn store_writes_the_bundle_notes_reads() {
    let Some(dir) = common::no3_drawings() else { return eprintln!("skipped: NO3 drawings not found") };
    let lib = common::scratch("paperkit-store");
    let mut st = Store::open(&lib).unwrap();
    let (mut d, ..) = bundle(&dir, "33_");
    // a kind Notes' bundle can't hold yet: kept in our copy
    d.shapes.push(Shape { id: "star".into(), kind: ShapeKind::Star, frame: [10.0, 10.0, 50.0, 50.0], fill: [1.0, 0.0, 0.0, 1.0], sides: 5, ..Default::default() });
    let mut body = Body::from_lines("Sketch", &[]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let a = Attachment { id: "D1D1D1D1-0000-4000-8000-000000000002".into(), uti: UTI_PAPER.into(), kind: AttachmentKind::Drawing, drawing: Some(d.clone()), width: d.width as f64, height: d.height as f64, ..Default::default() };
    note.attachments.push(a.clone());
    note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, UTI_PAPER)], ..Default::default() });
    st.save_note(&mut note).unwrap();

    let b = paperkit::bundle_dir(&lib.join("Accounts/LocalAccount"), &a.id);
    assert!(b.join("Database/data.sqlite3").is_file());
    // Notes sees the strokes and the shapes it knows; we see everything
    let from_bundle = paperkit::read(&b).unwrap();
    assert_eq!((from_bundle.strokes.len(), from_bundle.shapes.len()), (3, 2));
    assert_eq!(st.note(&note.id).unwrap().attachment(&a.id).unwrap().drawing.as_ref(), Some(&d));
    let meta: Vec<u8> = rusqlite::Connection::open(lib.join("NoteStore.sqlite"))
        .unwrap()
        .query_row("SELECT ZMETADATADATA FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1", [&a.id], |r| r.get(0))
        .unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&meta).unwrap();
    assert_eq!((meta["hasEnhancedCanvasKey"].as_bool(), meta["hasNewInksKey"].as_bool()), (Some(true), Some(true)));

    // edited on a Mac: the bundle changed, so it's what we read
    let mut mac = from_bundle.clone();
    mac.strokes.truncate(1);
    paperkit::write(&b, &a.id, &mac).unwrap();
    let seen = st.note(&note.id).unwrap().attachment(&a.id).unwrap().drawing.clone().unwrap();
    assert_eq!((seen.strokes.len(), seen.shapes.len()), (1, 2));

    // edited here again: the bundle follows
    let mut n2 = st.note(&note.id).unwrap();
    n2.attachments[0].drawing.as_mut().unwrap().strokes.clear();
    n2.attachments[0].drawing.as_mut().unwrap().shapes.truncate(1);
    st.save_note(&mut n2).unwrap();
    assert_eq!(paperkit::read(&b).unwrap().strokes.len(), 0);

    // locked: no plaintext bundle; unlocked: back
    st.set_password("pw", None).unwrap();
    st.lock_note(&note.id, "pw").unwrap();
    assert!(!b.exists());
    st.remove_lock(&note.id, "pw").unwrap();
    assert_eq!(paperkit::read(&b).unwrap().shapes.len(), 1);

    // deleting the note removes it
    st.delete_note(&note.id).unwrap();
    st.empty_trash().unwrap();
    assert!(!b.exists());
}

#[test]
fn library_import_reads_apple_bundles() {
    let (Some(r), Some(dir)) = (common::research(), common::no3_drawings()) else { return eprintln!("skipped: research not found") };
    let g = common::group_container(&r, "paperkit-import");
    let (a, id) = common::no3_sample(&dir, "33_");
    let dst = paperkit::bundle_dir(&g.join("Accounts/LocalAccount"), &id);
    std::fs::create_dir_all(dst.join("Database")).unwrap();
    std::fs::copy(a.join("Paper.bundle/Database/data.sqlite3"), dst.join("Database/data.sqlite3")).unwrap();
    let before = std::fs::read(dst.join("Database/data.sqlite3")).unwrap();
    let lib = nt_core::import::read_library(&g).unwrap();
    let n = lib.notes.iter().find(|n| n.title == "33 Drawing").unwrap();
    let d = n.attachments.iter().find(|a| a.kind == AttachmentKind::Drawing).unwrap().drawing.clone().unwrap();
    assert_eq!((d.strokes.len(), d.shapes.len()), (3, 2));
    // the Apple bundle is untouched
    assert_eq!(std::fs::read(dst.join("Database/data.sqlite3")).unwrap(), before);
    assert!(!dst.join("Database/data.sqlite3-wal").exists());

    // imported into our library, it becomes ours with a bundle Notes can read
    let mine = common::scratch("paperkit-import-store");
    let mut st = Store::open(&mine).unwrap();
    st.import_library(&lib).unwrap();
    let b = paperkit::bundle_dir(&mine.join("Accounts/LocalAccount"), &id);
    assert_eq!(paperkit::read(&b).unwrap().strokes.len(), 3);
}

fn no3_pdf() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var("HOME").ok()?).join("CascadeProjects/notes-research-src/no3-pdf/data");
    p.join("Paper.bundle").is_dir().then_some(p)
}

#[test]
fn notes_pdf_document_round_trips() {
    let Some(data) = no3_pdf() else { return eprintln!("skipped: NO3 PDF sample not found") };
    let apple = data.join("Paper.bundle");
    let doc = paperkit::read_document(&apple).unwrap();
    assert_eq!(doc.pages.iter().map(|p| p.index).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
    assert!(doc.pages.iter().all(|p| (p.width, p.height, p.crop) == (612.0, 792.0, [0.0, 0.0, 612.0, 792.0])));
    let pdf = std::fs::read(doc.file.as_ref().unwrap()).unwrap();
    assert!(pdf.starts_with(b"%PDF") && pdf.len() == 2122);
    // View As: the sample was left at Large
    assert_eq!(paperkit::view_size_of(&std::fs::read(data.join("att-ZMERGEABLEPREFERREDVIEWSIZE.bin")).unwrap()), ViewSize::Large);
    for v in [ViewSize::Small, ViewSize::Medium, ViewSize::Large] {
        assert_eq!(paperkit::view_size_of(&paperkit::view_size_bytes(v)), v);
    }
    assert_eq!(paperkit::view_size_bytes(ViewSize::Small)[9..], [0x12, 0x02, 0x08, 0x02]);

    let out = common::scratch("paperkit-pdf").join("doc.bundle");
    let file = paperkit::write_document(&out, &pdf, &doc.pages).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), pdf);
    let back = paperkit::read_document(&out).unwrap();
    assert_eq!(back.pages, doc.pages);
    assert_eq!(back.file, Some(file));
    assert_eq!(layout(&out), layout(&apple));
    let assets = |b: &Path| -> Vec<(Vec<u8>, i64)> {
        let c = rusqlite::Connection::open_with_flags(format!("file:{}?immutable=1", b.join("Database/data.sqlite3").display()), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI).unwrap();
        let mut st = c.prepare("SELECT Id, RetainCount FROM Assets").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect()
    };
    assert_eq!(assets(&out), assets(&apple));
}

#[test]
fn store_keeps_pdfs_as_documents() {
    let Some(data) = no3_pdf() else { return eprintln!("skipped: NO3 PDF sample not found") };
    let src = paperkit::read_document(&data.join("Paper.bundle")).unwrap().file.unwrap();
    let lib = common::scratch("paperkit-pdf-store");
    let pdf_in = lib.join("Five pages.pdf");
    std::fs::copy(&src, &pdf_in).unwrap();
    let mut st = Store::open(&lib.join("lib")).unwrap();
    let mut body = Body::from_lines("PDF", &[]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let a = Attachment { id: "D0C0D0C0-0000-4000-8000-000000000005".into(), file: Some(pdf_in.clone()), ..Default::default() };
    note.attachments.push(a.clone());
    note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, "com.adobe.pdf")], ..Default::default() });
    st.save_note(&mut note).unwrap();

    let acct = lib.join("lib/Accounts/LocalAccount");
    let b = paperkit::bundle_dir(&acct, &a.id);
    assert!(b.join("Database/data.sqlite3").is_file());
    assert!(!acct.join("Media").exists(), "no ICMedia copy");
    let got = st.note(&note.id).unwrap().attachment(&a.id).unwrap().clone();
    assert_eq!((got.uti.as_str(), got.kind, got.name.as_str(), got.width, got.height), (UTI_PAPER_PDF, AttachmentKind::Pdf, "Five pages.pdf", 612.0, 792.0));
    assert_eq!(std::fs::read(got.file.as_ref().unwrap()).unwrap(), std::fs::read(&pdf_in).unwrap());
    assert!(got.fallback.as_ref().is_some_and(|f| f.ends_with("FallbackPDF.pdf")));
    if std::process::Command::new("pdfinfo").arg("-v").output().is_ok() {
        assert_eq!(got.pages.len(), 5);
    }
    let row: (String, i64, i64) = rusqlite::Connection::open(lib.join("lib/NoteStore.sqlite"))
        .unwrap()
        .query_row("SELECT ZTITLE, ZMINIMUMSUPPORTEDNOTESVERSION, ZSECTION FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1", [&a.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!(row, ("Five pages".to_string(), 13, 6));

    // View As
    let mut n2 = st.note(&note.id).unwrap();
    n2.attachments[0].view_size = ViewSize::Medium;
    st.save_note(&mut n2).unwrap();
    assert_eq!(st.note(&note.id).unwrap().attachment(&a.id).unwrap().view_size, ViewSize::Medium);

    // locked: no plain bundle; unlocked: back with its pages
    st.set_password("pw", None).unwrap();
    st.lock_note(&note.id, "pw").unwrap();
    assert!(!b.exists());
    st.remove_lock(&note.id, "pw").unwrap();
    let again = st.note(&note.id).unwrap().attachment(&a.id).unwrap().clone();
    assert_eq!(std::fs::read(again.file.as_ref().unwrap()).unwrap(), std::fs::read(&pdf_in).unwrap());

    // removed from the note: everything goes
    let mut n3 = st.note(&note.id).unwrap();
    n3.attachments.clear();
    n3.body = Body::from_lines("PDF", &["gone"]);
    st.save_note(&mut n3).unwrap();
    assert!(!b.exists() && !acct.join("FallbackPDFs").join(&a.id).exists());
}

#[test]
fn widths_convert_both_ways() {
    for k in [InkKind::Pen, InkKind::Monoline, InkKind::Marker, InkKind::Pencil] {
        for w in [0.5f32, 1.0, 2.2, 2.35, 3.0, 4.0, 6.0, 20.0] {
            assert!(near(paperkit::drawn_width(k, paperkit::stored_width(k, w)), w, 1e-4), "{k:?} {w}");
        }
    }
    assert!(near(paperkit::drawn_width(InkKind::Pen, 3.29), 2.6, 1e-6));
    assert!(near(paperkit::stored_width(InkKind::Monoline, 1.0), 2.5, 1e-6));
}

/// A legacy PKDrawing (com.apple.drawing.2) built the way PencilKit writes it.
#[test]
fn legacy_pencilkit_drawings_decode() {
    use kn_format::{msg::Field, Msg, Val};
    let m = |fields: Vec<(u32, Val)>| Msg { fields: fields.into_iter().map(|(num, val)| Field { num, val }).collect(), ..Msg::untyped() };
    let b = |m: Msg| Val::Bytes(m.encode());
    let f = |x: f32| Val::I32(x.to_bits());
    let ink = m(vec![(1, b(m(vec![(1, f(1.0)), (2, f(0.0)), (3, f(0.0)), (4, f(1.0))]))), (2, Val::Bytes(b"com.apple.ink.marker".to_vec())), (3, Val::Varint(3))]);
    let pts: Vec<u8> = [(10.0f32, 20.0f32, 0.0f32), (30.0, 25.0, 0.1), (50.0, 30.0, 0.2)].iter().flat_map(|(x, y, t)| [x.to_le_bytes(), y.to_le_bytes(), t.to_le_bytes()].concat()).collect();
    let mut blob = 13.75f32.to_le_bytes().to_vec();
    blob.extend([0xe8, 0x03, 0, 0, 0, 0, 0x12, 0x34, 0, 0, 0xff, 0x7f, 0, 0, 0x80, 0x3f, 0, 0, 0, 0, 0, 0]);
    let data = m(vec![(3, Val::Varint(3)), (4, Val::Varint(3)), (5, Val::Varint(4092)), (6, Val::Bytes(blob)), (7, Val::Bytes(pts)), (8, Val::Varint(1))]);
    let tr = m(vec![(1, f(1.0)), (2, f(0.0)), (3, f(0.0)), (4, f(1.0)), (5, f(5.0)), (6, f(-2.0))]);
    let live = m(vec![(1, Val::Bytes(vec![7; 16])), (4, Val::Varint(1)), (5, b(data)), (7, b(tr))]);
    let erased = m(vec![(1, Val::Bytes(vec![8; 16])), (4, Val::Varint(0))]);
    let pen = m(vec![(1, b(m(vec![(1, f(0.0)), (2, f(0.0)), (3, f(0.0)), (4, f(1.0))]))), (2, Val::Bytes(b"com.apple.ink.pen".to_vec())), (3, Val::Varint(3))]);
    let bounds = m(vec![(1, f(0.0)), (2, f(0.0)), (3, f(320.0)), (4, f(240.0))]);
    let top = m(vec![(30, Val::Varint(0)), (4, b(pen)), (4, b(ink)), (5, b(erased)), (5, b(live)), (6, b(bounds))]);
    let mut raw = b"wrd".to_vec();
    raw.extend(top.encode());
    let d = paperkit::decode_legacy(&raw).unwrap();
    assert_eq!((d.width, d.height, d.strokes.len()), (320.0, 240.0, 1));
    let s = &d.strokes[0];
    assert_eq!((s.ink, s.color, s.transform, s.points.len()), (InkKind::Marker, [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0, 5.0, -2.0], 3));
    assert!(near(s.width, 13.75 * 1.2, 1e-4) && (s.points[2].x, s.points[2].y, s.points[2].t) == (50.0, 30.0, 0.2));
    assert_eq!(paperkit::decode_legacy(b"not a drawing"), None);
}
