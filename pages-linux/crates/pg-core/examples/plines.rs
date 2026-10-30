//! cargo run -p pg-core --example plines -- file.pages : body text, inline objects and line boxes of page 1
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let b = p.body_storage().unwrap();
    println!("text {:?}", p.doc.storage_text(b));
    let pg = &l.pages[0];
    for ln in &pg.lines {
        let tl = &ln.piece.tl.lines[ln.line];
        println!("line para {} dx {} dy {} baseline {} w {:?}", ln.para, ln.dx, ln.dy, tl.baseline, tl.width);
    }
    for o in &pg.objects {
        println!("object {} {} at {},{} {}x{} inline {}", o.id, p.file().type_name(o.id), o.x, o.y, o.w, o.h, o.inline);
    }
}
