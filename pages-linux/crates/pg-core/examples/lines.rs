//! cargo run -p pg-core --example lines -- file.pages : each line's arrowheads and style
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    for (_, id) in p.floating() {
        let Some(d) = pg_core::kn_model::scene::drawable(k, id, Default::default()) else { continue };
        if let pg_core::kn_model::DrawableKind::Shape(s) = &d.kind {
            let style = k.obj(id).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.ref_id("style"));
            println!("{id} line {} head {:?} tail {:?} style {:?} at {:.0},{:.0}", s.is_line, s.head.is_some(), s.tail.is_some(), style, d.geom.x, d.geom.y);
        }
    }
}
