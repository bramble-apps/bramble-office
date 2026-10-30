//! Group and ungroup shapes on the feature catalog; print a hash of each saved file (checks
//! that group.rs changes leave Keynote output byte-identical).
use std::hash::{Hash, Hasher};
fn main() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
    let mut d = kn_model::Document::open(p).unwrap();
    let s = d.slides();
    let (i, sc) = (0..s.len()).map(|i| (i, d.scene(&s, i).unwrap())).find(|(_, sc)| sc.drawables.iter().filter(|x| x.placeholder.is_none() && !x.from_template && matches!(x.kind, kn_model::DrawableKind::Shape(_))).count() >= 3).unwrap();
    let ids: Vec<_> = sc.drawables.iter().filter(|x| x.placeholder.is_none() && !x.from_template).take(2).map(|x| x.id).collect();
    let mut h = kn_model::edit::History::default();
    // the objects group / ungroup write (the slide, the children, the group): their content,
    // the saved zip itself isn't byte-stable between runs
    let slide = s[i].slide;
    let hash = |d: &kn_model::Document| {
        let mut st = std::collections::hash_map::DefaultHasher::new();
        let mut ids2 = vec![slide];
        ids2.extend(&ids);
        for id in ids2 {
            format!("{:?}", d.file.obj(id)).hash(&mut st);
        }
        st.finish()
    };
    d.begin(&mut h, "Group");
    let g = d.group(&mut h, s[i].slide, &ids).unwrap();
    d.end(&mut h);
    println!("grouped {:x} group {:x}", hash(&d), {
        let mut st = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", d.file.obj(g)).hash(&mut st);
        st.finish()
    });
    d.begin(&mut h, "Ungroup");
    d.ungroup(&mut h, s[i].slide, g).unwrap();
    d.end(&mut h);
    println!("ungrouped {:x}", hash(&d));
}
