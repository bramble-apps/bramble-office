//! prints the body's spans and paragraph styles
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let su = p.setup();
    println!("setup {}x{} margins l{} r{} t{} b{} landscape {} facing {} body {} paper {:?}", su.width, su.height, su.left, su.right, su.top, su.bottom, su.landscape, su.facing_pages, su.body, su.paper_id);
    for (pg, id) in p.floating() {
        println!("floating page {pg}: {id} {:?}", p.file().obj(id).map(|m| m.type_name()));
        if let Some(d) = p.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")) {
            println!("  geom {:?} desc {:?}", pg_core::kn_model::scene::geometry(d), d.str("accessibility_description"));
            if let Some(ps) = p.file().obj(id).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.msg("pathsource")) {
                println!("  pathsource flips h {:?} v {:?}", ps.bool("horizontalFlip"), ps.bool("verticalFlip"));
            }
        }
        if let Some(m) = p.file().obj(id).and_then(|m| m.ref_id("mask")) {
            if let Some(d) = p.file().obj(m).and_then(|m| m.find_super("TSD.DrawableArchive")) {
                println!("  mask geom {:?}", pg_core::kn_model::scene::geometry(d));
            }
        }
    }
    let b = pg_core::body::build(p.file(), p.body_storage().unwrap(), None, Default::default()).unwrap();
    println!("{:?}", b.tb.text);
    for s in &b.spans { println!("{:?}", s); }
    if std::env::var_os("RUNS").is_some() {
        for (i, pa) in b.tb.paragraphs.iter().enumerate() {
            println!("para {i}: align {:?} spacing {:?} before {} after {} indents {} {} {}", pa.align, pa.line_spacing, pa.space_before, pa.space_after, pa.first_line_indent, pa.left_indent, pa.right_indent);
            for r in &pa.runs {
                let c = &r.style;
                println!("  {:?}: {} {} b{} i{} u{} s{} script{} shift{} caps {:?} track {} color {:?} bg {:?}", &b.tb.text[r.start..r.end], c.font_name, c.size, c.bold as u8, c.italic as u8, c.underline as u8, c.strike as u8, c.script, c.baseline_shift, c.caps, c.tracking, (c.color.r, c.color.g, c.color.b), c.background.map(|b| (b.r, b.g, b.b)));
            }
        }
    }
    for (i, e) in b.extra.iter().enumerate() { println!("para {i}: style {:?} list {:?} dropcap {:?} newpage {} keepnext {} together {}", e.style.map(|s| pg_core::edit::named_style(&p.doc, Some(s)).1), e.list, e.drop_cap, e.page_break_before, e.keep_with_next, e.keep_together); }
    for sec in p.sections() {
        let Some(m) = sec.master(1, 1) else { continue };
        for f in [false, true] {
            for st in p.header_footer_storages(m, f) {
                let t = p.doc.storage_text(st);
                if !t.is_empty() { println!("{} {st}: {t:?}", if f { "footer" } else { "header" }); }
            }
        }
    }
    for (_, a) in p.file().archives() {
        let m = a.obj();
        if m.type_name() == "TSWP.TOCAttachmentArchive" || m.type_name() == "TSWP.TOCInfoArchive" {
            let st = m.find_super("TSWP.ShapeInfoArchive").and_then(|i| i.ref_id("owned_storage"));
            let styles: Vec<String> = st.and_then(|s| p.file().obj(s)).map(|s| s.msg("table_para_style").map(|t| t.msgs("entries").filter_map(|e| e.ref_id("object")).map(|ps| format!("{:?}", pg_core::edit::named_style(&p.doc, Some(ps)))).collect()).unwrap_or_default()).unwrap_or_default();
            println!("{} {}: storage {:?} styles {:?}", m.type_name(), a.id(), st, styles);
            if let Some(ps) = st.and_then(|s| p.file().obj(s)).and_then(|s| s.msg("table_para_style").and_then(|t| t.msgs("entries").find_map(|e| e.ref_id("object")))) {
                let cs = pg_core::kn_model::style::char_style(p.file(), Some(ps), None);
                println!("  size {} font {:?} bold {}", cs.size, cs.font_name, cs.bold);
                for m in pg_core::kn_model::style::chain(p.file(), Some(ps)) {
                    println!("  chain {:?} cp {:?} size {:?}", m.type_name(), m.msg("char_properties").is_some(), m.msg("char_properties").and_then(|c| c.f32("font_size")));
                }
            }
        }
    }
    if std::env::var_os("ALL").is_some() {
        for (_, a) in p.file().archives() {
            if a.obj().type_name() == "TSWP.StorageArchive" {
                let t = p.doc.storage_text(a.id());
                if !t.is_empty() { println!("storage {}: {t:?}", a.id()); }
            }
        }
    }
    println!("char styles: {:?}", p.doc.character_styles());
}
// (debug) TOC storages' paragraph styles
#[allow(dead_code)]
fn _toc() {}
