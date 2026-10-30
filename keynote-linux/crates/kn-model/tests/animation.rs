//! Build / movie attributes the slideshow relies on, read from the feature catalog.
use kn_model::{Document, DrawableKind};

fn testdata(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn build_in_action_out_attributes() {
    let doc = Document::open(testdata("Keynote-Feature-Catalog.key")).unwrap();
    let slides = doc.slides();
    let sc = doc.scene(&slides, 130).unwrap();
    assert_eq!(sc.builds.len(), 3);
    let rot = sc.builds.iter().find(|b| b.kind == "Action").unwrap();
    assert_eq!(rot.effect, "apple:action-rotation");
    assert_eq!(rot.params.rotation_angle, Some(45.0));
    assert!(!rot.params.rotation_clockwise);
    assert_eq!(rot.params.acceleration, 3);
    let pop = sc.builds.iter().find(|b| b.kind == "In").unwrap();
    assert_eq!(pop.params.text_delivery, 1);
    assert_eq!(sc.build_order.len(), 3);
    assert!(sc.build_order.iter().all(|s| !s.automatic && s.chunk == 1));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn movie_attributes() {
    let doc = Document::open(testdata("Keynote-Feature-Catalog.key")).unwrap();
    let slides = doc.slides();
    let sc = doc.scene(&slides, 126).unwrap();
    let m = sc
        .drawables
        .iter()
        .find_map(|d| match &d.kind {
            DrawableKind::Movie(m) => Some(m),
            _ => None,
        })
        .unwrap();
    assert_eq!(m.movie_data, Some(660));
    assert_eq!((m.start, m.end, m.volume, m.loop_mode), (0.0, 4.0, 1.0, 0));
    assert!(sc.builds.iter().any(|b| b.effect == "apple:movie-start"));
}
