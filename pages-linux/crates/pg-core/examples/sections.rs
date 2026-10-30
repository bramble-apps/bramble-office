//! cargo run -p pg-core --example sections -- file.pages : sections, their page templates and layout objects
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    let parent = |id: u64| k.obj(id).and_then(|x| x.find_super("TSD.DrawableArchive")).and_then(|x| x.ref_id("parent"));
    for s in p.sections() {
        println!("section {} start16 {} first_diff {} even_odd {} hides_first {} first {:?} odd {:?} even {:?}", s.id, s.start16, s.first_page_different, s.even_odd_different, s.first_page_hides_header_footer, s.first, s.odd, s.even);
        for t in [s.first, s.odd, s.even].into_iter().flatten() {
            let m = k.obj(t).unwrap();
            println!("  template {t}: drawables {:?} headers {:?} footers {:?}", m.ref_ids("section_template_drawables"), m.ref_ids("headers"), m.ref_ids("footers"));
            for d in m.ref_ids("section_template_drawables") {
                println!("    {d} {} parent {:?}", k.type_name(d), parent(d));
            }
        }
    }
    for (pg, id) in p.floating().into_iter().take(2) {
        println!("floating page {pg} {id} {} parent {:?}", k.type_name(id), parent(id));
    }
}
