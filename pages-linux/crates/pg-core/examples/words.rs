//! cargo run -p pg-core --example words -- file.docx|file.pages page : each line of page `page` (0-based) as words with their x range in points; --tsv: a word per line (x0 x1 baseline text) (to compare with `pdftotext -bbox` of OnlyOffice's PDF)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let p = if path.extension().is_some_and(|e| e == "docx") { pg_core::import::import_file(path).unwrap() } else { pg_core::Pages::open(path).unwrap() };
    let page: usize = a[2].parse().unwrap();
    // --tsv: one word per line: x0, x1, baseline, text
    let tsv = a.iter().any(|x| x == "--tsv");
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    let b = pg_core::body::build(p.file(), p.body_storage().unwrap(), None, pg_core::body::BuildCtx { word_labels: p.word_layout, ..Default::default() }).unwrap();
    for ln in &l.pages[page].lines {
        let gl = &ln.piece.line_glyphs[ln.line];
        // --labels: each list label glyph's face and size
        if a.iter().any(|x| x == "--labels") {
            for &i in gl {
                let g = &ln.piece.tl.glyphs[i];
                if g.label {
                    let fam = r.fonts.fs.db().face(g.font_id).map(|f| f.families[0].0.clone()).unwrap_or_default();
                    println!("label at {:.2},{:.2}: {} {:.1}pt glyph {}", ln.dx + g.x, ln.dy + g.y, fam, g.size, g.glyph_id);
                }
            }
            continue;
        }
        let mut words: Vec<(f32, f32, String)> = Vec::new();
        let mut prev_end = usize::MAX;
        let mut pending: Option<f32> = None;
        for &i in gl {
            let g = &ln.piece.tl.glyphs[i];
            let t = b.tb.text.get(g.start..g.end).unwrap_or("");
            // (a joiner the layout added after a character shares its source: no word of its own)
            if g.start < prev_end && prev_end != usize::MAX { continue; }
            // a glyph with no text of its own (its character's source went to a neighbour): part
            // of the word it sits in
            if t.is_empty() {
                pending.get_or_insert(ln.dx + g.x);
                if prev_end != usize::MAX { if let Some(w) = words.last_mut() { w.1 = ln.dx + g.x + g.w; } }
                continue;
            }
            if t.trim().is_empty() { prev_end = usize::MAX; pending = None; continue; }
            if prev_end == g.start && !words.is_empty() {
                let w = words.last_mut().unwrap(); w.1 = ln.dx + g.x + g.w; w.2.push_str(t);
            } else { words.push((pending.unwrap_or(ln.dx + g.x), ln.dx + g.x + g.w, t.to_string())); }
            pending = None;
            prev_end = g.end;
        }
        let base = gl.iter().map(|&i| ln.piece.tl.glyphs[i].y).fold(f32::MIN, f32::max) + ln.dy;
        if tsv {
            for w in &words {
                println!("{:.2}\t{:.2}\t{:.2}\t{}", w.0, w.1, base, w.2);
            }
        } else {
            println!("{:.2} {}", ln.dy, words.iter().map(|w| format!("{}[{:.2}-{:.2}]", w.2, w.0, w.1)).collect::<Vec<_>>().join(" "));
        }
    }
}
