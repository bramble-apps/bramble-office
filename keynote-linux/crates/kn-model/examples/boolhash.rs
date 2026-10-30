//! Unite two shapes on a slide, then Break Apart a two-part shape; print a hash of the objects
//! written (the slide and the results), to check that boolops / points changes leave Keynote's
//! output identical.
use std::hash::{Hash, Hasher};
fn main() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
    let mut d = kn_model::Document::open(p).unwrap();
    let s = d.slides();
    let (i, sc) = (0..s.len()).map(|i| (i, d.scene(&s, i).unwrap())).find(|(_, sc)| sc.drawables.iter().filter(|x| x.placeholder.is_none() && !x.from_template && matches!(&x.kind, kn_model::DrawableKind::Shape(sh) if !sh.is_line)).count() >= 3).unwrap();
    let _ = sc;
    let slide = s[i].slide;
    let mut h = kn_model::edit::History::default();
    // two rectangles of our own, apart (their union has two parts)
    d.begin(&mut h, "Shapes");
    let a = d.insert(&mut h, slide, &kn_model::insert::NewObject::Shape(kn_model::insert::BasicShape::Rectangle)).unwrap();
    let b = d.insert(&mut h, slide, &kn_model::insert::NewObject::Shape(kn_model::insert::BasicShape::Rectangle)).unwrap();
    d.move_by(&mut h, b, 400.0, 0.0);
    d.end(&mut h);
    let ids = vec![a, b];
    let hash = |d: &kn_model::Document, extra: &[kn_format::ObjId]| {
        let mut st = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", d.file.obj(slide)).hash(&mut st);
        for id in extra {
            format!("{:?}", d.file.obj(*id)).hash(&mut st);
        }
        st.finish()
    };
    d.begin(&mut h, "Unite");
    let u = d.shapes_boolean(&mut h, i, &ids, kn_model::boolops::BoolOp::Unite).unwrap();
    d.end(&mut h);
    println!("united {:x}", hash(&d, &[u]));
    // break apart the united shape (two parts when the shapes didn't touch, else one: report it)
    let sc = d.scene(&d.slides(), i).unwrap();
    let path = sc.drawables.iter().find(|x| x.id == u).and_then(|x| match &x.kind {
        kn_model::DrawableKind::Shape(sh) => Some(sh.path.clone()),
        _ => None,
    }).unwrap();
    d.begin(&mut h, "Break Apart");
    match d.break_apart(&mut h, slide, u, &path) {
        Ok(parts) => println!("broken {} {:x}", parts.len(), hash(&d, &parts)),
        Err(e) => println!("break apart: {e}"),
    }
    d.end(&mut h);
}
