//! cargo run -p pg-core --example shapefill -- file.pages : each floating shape's fill and stroke
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    for (_, id) in p.floating() {
        let Some(d) = pg_core::kn_model::scene::drawable(k, id, Default::default()) else { continue };
        if let pg_core::kn_model::DrawableKind::Shape(s) = &d.kind {
            let st = k.obj(id).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.ref_id("style"));
            let sm = st.and_then(|s| k.obj(s));
            let parent = sm.and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.ref_id("parent"));
            let name = sm.and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.str("name").map(|x| x.to_string()));
            println!("{id} fill {:?} stroke {:?} style {:?} {:?} parent {:?} type {:?}", s.fill, s.stroke.as_ref().map(|x| (x.color, x.width)), st, name, parent, sm.map(|m| m.type_name()));
        }
    }
}
