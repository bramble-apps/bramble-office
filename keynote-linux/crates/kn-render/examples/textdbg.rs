use kn_model::{Document, DrawableKind};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let slides = doc.slides();
    let sc = doc.scene(&slides, a[2].parse::<usize>().unwrap() - 1).unwrap();
    let mut r = kn_render::Renderer::new();
    for d in &sc.drawables {
        if let DrawableKind::Shape(s) = &d.kind {
            if let Some(tb) = &s.text {
                let (tl, g) = r.text_layout(d).unwrap();
                println!("#{} {:?} pad {:?} shrink {} valign {:?} -> scale {} content {}x{} eff {:?}", d.id, tb.text.chars().take(30).collect::<String>(), tb.padding, tb.shrink_to_fit, tb.valign, tl.scale, tl.content_w, tl.content_h, (g.x,g.y,g.w,g.h));
                for p in &tb.paragraphs { println!("   para sb {} sa {} li {} list_indent {} label_indent {} spacing {:?} size {}", p.space_before, p.space_after, p.left_indent, p.list_indent, p.label_indent, p.line_spacing, p.runs[0].style.size); }
                for l in &tl.lines { println!("   line top {:.1} base {:.1} h {:.1} x {:.1} w {:.1}", l.top, l.baseline, l.height, l.x, l.width); }
                if let Some(g) = tl.glyphs.first() { println!("   first glyph size {} at {},{}", g.size, g.x, g.y); }
                for h in &tl.highlights { println!("   highlight x {:.1} y {:.1} w {:.1} h {:.1}", h.0, h.1, h.2, h.3); }
                println!("   sizes {:?}", tl.glyphs.iter().map(|g| g.size as i32).collect::<std::collections::BTreeSet<_>>());
            }
        }
    }
}
