//! cargo run -p pg-core --example dcfill : change a shaped drop cap's fill twice and print what the body reads
fn main() {
    use kn_model::edit::History;
    let mut p = pg_core::Pages::new_blank(false);
    let b = p.body_storage().unwrap();
    let mut h = History::default();
    p.doc.begin(&mut h, "t");
    pg_core::edit::replace(&mut p.doc, &mut h, b, 0, 0, "Hello world\nNext");
    p.doc.end(&mut h);
    let first = pg_core::edit::drop_cap_styles(&p.doc)[0].0;
    p.doc.begin(&mut h, "dc");
    pg_core::edit::set_drop_cap(&mut p.doc, &mut h, b, 0, 0, Some(first));
    p.doc.end(&mut h);
    for c in [(1.0, 0.0, 0.0), (0.0, 0.0, 1.0)] {
        p.doc.begin(&mut h, "fill");
        pg_core::edit::set_drop_cap_settings(&mut p.doc, &mut h, b, 0, 0, first, &|s| {
            s.shape = true;
            s.fill = Some(kn_model::Color { r: c.0, g: c.1, b: c.2, a: 1.0 });
        });
        p.doc.end(&mut h);
        let body = pg_core::body::build(p.file(), b, None, Default::default()).unwrap();
        let dc = pg_core::edit::drop_cap_at(&p.doc, b, 0);
        println!("set {c:?} -> shape {:?} settings fill {:?}", body.extra[0].drop_cap_shape.map(|s| s.0), dc.map(|d| pg_core::edit::drop_cap_settings(&p.doc, d).fill));
    }
}
