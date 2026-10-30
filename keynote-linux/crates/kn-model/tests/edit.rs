use kn_model::edit::History;
use kn_model::{Document, DrawableKind};

fn testdata(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn move_resize_notes_survive_save_and_undo() {
    let mut doc = Document::open(testdata("02-shape.key")).unwrap();
    let slides = doc.slides();
    let sc = doc.scene(&slides, 0).unwrap();
    let shape = sc.drawables.iter().find(|d| matches!(d.kind, DrawableKind::Shape(_)) && d.placeholder.is_none()).unwrap();
    let (id, g0) = (shape.id, shape.geom);
    let mut h = History::default();

    doc.begin(&mut h, "Move");
    doc.move_by(&mut h, id, 50.0, 25.0);
    doc.end(&mut h);
    doc.begin(&mut h, "Notes");
    assert!(doc.set_notes(&mut h, slides[0].slide, "Speaker notes ✓"));
    doc.end(&mut h);

    let bytes = doc.file.to_zip().unwrap();
    let doc2 = Document::from_file(kn_format::KeyFile::from_zip(&bytes).unwrap());
    let sc2 = doc2.scene(&doc2.slides(), 0).unwrap();
    let g1 = sc2.drawables.iter().find(|d| d.id == id).unwrap().geom;
    assert_eq!((g1.x, g1.y), (g0.x + 50.0, g0.y + 25.0));
    assert_eq!(doc2.notes(slides[0].slide), "Speaker notes ✓");

    assert!(doc.undo(&mut h));
    assert!(doc.undo(&mut h));
    let sc3 = doc.scene(&slides, 0).unwrap();
    assert_eq!(sc3.drawables.iter().find(|d| d.id == id).unwrap().geom, g0);
    assert!(doc.redo(&mut h));
    let g4 = doc.scene(&slides, 0).unwrap().drawables.iter().find(|d| d.id == id).unwrap().geom;
    assert_eq!(g4.x, g0.x + 50.0);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn duplicate_delete_reorder_slides_roundtrip() {
    let mut doc = Document::open(testdata("11-two-slides.key")).unwrap();
    let n0 = doc.slides().len();
    let mut h = History::default();
    doc.begin(&mut h, "Duplicate");
    doc.duplicate_slide(&mut h, 0).unwrap();
    doc.end(&mut h);
    assert_eq!(doc.slides().len(), n0 + 1);
    // the copy renders the same content under new ids
    let s = doc.slides();
    let a = doc.scene(&s, 0).unwrap();
    let b = doc.scene(&s, 1).unwrap();
    assert_eq!(a.drawables.len(), b.drawables.len());
    assert!(a.drawables.iter().zip(&b.drawables).all(|(x, y)| x.id != y.id && x.geom == y.geom));
    // survives save/load
    let k = kn_format::KeyFile::from_zip(&doc.file.to_zip().unwrap()).unwrap();
    assert_eq!(Document::from_file(k).slides().len(), n0 + 1);
    doc.begin(&mut h, "Delete");
    doc.delete_slides(&mut h, &[0]).unwrap();
    doc.end(&mut h);
    assert_eq!(doc.slides().len(), n0);
    assert!(doc.undo(&mut h));
    assert!(doc.undo(&mut h));
    assert_eq!(doc.slides().len(), n0);
    assert_eq!(doc.slides()[0].slide, s[0].slide);
}

/// Keynote draws shapes from the path source: resizing must scale the path and natural size,
/// and flips must be stored on the path source (verified in Keynote 15.4 by the Mac session).
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn resize_scales_path_and_flip_goes_on_path_source() {
    let mut doc = Document::open(testdata("02-shape.key")).unwrap();
    let slides = doc.slides();
    let d = doc.scene(&slides, 0).unwrap().drawables.into_iter().find(|d| d.placeholder.is_none() && !d.from_template).unwrap();
    let mut h = History::default();
    doc.begin(&mut h, "t");
    doc.set_frame(&mut h, d.id, kn_model::Geometry { w: 500.0, h: 250.0, hflip: true, ..d.geom });
    doc.end(&mut h);
    let k = kn_format::KeyFile::from_zip(&doc.file.to_zip().unwrap()).unwrap();
    let shape = k.obj(d.id).unwrap().find_super("TSD.ShapeArchive").unwrap();
    let ps = shape.msg("pathsource").unwrap();
    assert_eq!(ps.bool("horizontalFlip"), Some(true));
    let b = ps.msg("bezier_path_source").unwrap();
    assert_eq!(b.msg("naturalSize").unwrap().f32("width"), Some(500.0));
    let max_x = b.msg("path").unwrap().msgs("elements").flat_map(|e| e.msgs("points").map(|p| p.f32("x").unwrap()).collect::<Vec<_>>()).fold(0.0f32, f32::max);
    assert!((max_x - 500.0).abs() < 0.01, "path scaled to 500, got {max_x}");
    let d2 = Document::from_file(k);
    let g = d2.scene(&d2.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == d.id).unwrap().geom;
    assert!(g.hflip && g.w == 500.0);
}

/// Duplicating a slide with a table gives the copy its own table: editing it leaves the
/// original alone (they used to share one; found by the deck stress test).
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn duplicated_slide_has_its_own_table() {
    use kn_model::{edit::History, tablecells::CellInput, Document, DrawableKind};
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-merge-0-before.key");
    let mut d = Document::open(path).unwrap();
    let mut h = History::default();
    d.begin(&mut h, "dup");
    d.duplicate_slide(&mut h, 0).unwrap();
    d.end(&mut h);
    let table = |d: &Document, i: usize| {
        let s = d.slides();
        d.scene(&s, i).unwrap().drawables.iter().find_map(|x| match &x.kind {
            DrawableKind::Table(t) => Some((x.id, t.cells.clone())),
            _ => None,
        }).unwrap()
    };
    let (a, cells_a) = table(&d, 0);
    let (b, _) = table(&d, 1);
    assert_ne!(a, b);
    d.begin(&mut h, "edit");
    d.set_cell(&mut h, b, 1, 1, &CellInput::Text("only on the copy".into())).unwrap();
    d.end(&mut h);
    assert_eq!(table(&d, 0).1, cells_a);
    assert!(table(&d, 1).1.iter().any(|c| c == "only on the copy"));
    let p = std::env::temp_dir().join(format!("kd-dup-{}.key", std::process::id()));
    d.save_as(&p).unwrap();
    let back = Document::open(&p).unwrap();
    assert_ne!(table(&back, 0).0, table(&back, 1).0);
    let _ = std::fs::remove_file(p);
}
