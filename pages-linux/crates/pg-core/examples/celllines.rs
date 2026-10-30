//! cargo run -p pg-core --example celllines -- file.docx page "text in the cell" : that cell's
//! lines as the table lays them out.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::import::import_file(std::path::Path::new(&a[1])).unwrap();
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let pg = &l.pages[a[2].parse::<usize>().unwrap() - 1];
    for d in pg_core::flow::page_drawables(&p, pg) {
        let kn_model::DrawableKind::Table(t) = &d.kind else { continue };
        for (i, s) in t.cells.iter().enumerate() {
            if !s.contains(&a[3]) {
                continue;
            }
            let c = i % t.cols as usize;
            let st = &t.cell_styles[i];
            let mut tb = kn_render::table::cell_box(t, i, s, st);
            tb.padding = st.padding;
            println!("rich {} width {} pad {:?}", t.rich.get(i).is_some_and(|x| x.is_some()), t.col_widths[c], st.padding);
            for p in &tb.paragraphs {
                println!("  para {}..{} first {} left {} right {} runs {:?}", p.start, p.end, p.first_line_indent, p.left_indent, p.right_indent, p.runs.iter().map(|r| (r.start, r.end, r.style.font_name.clone(), r.style.size, r.style.italic)).collect::<Vec<_>>());
            }
            let tl = kn_render::text::layout(&mut r.fonts, &tb, t.col_widths[c], 1000.0, !st.wrap, true);
            for ln in &tl.lines {
                println!("  {:7.2} {:?}", ln.width, &tb.text[ln.start..ln.end]);
            }
        }
    }
}
