//! slidefonts deck.key [needle]: per slide, each text's fonts, sizes, highlights and tracking.
use kn_model::{Drawable, DrawableKind};
fn flat(v: &[Drawable], depth: usize, out: &mut Vec<(usize, Drawable)>) {
    for x in v {
        out.push((depth, x.clone()));
        if let DrawableKind::Group(c) = &x.kind {
            flat(c, depth + 1, out);
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let needle = a.get(2).map(|s| s.to_lowercase());
    let s = d.slides();
    for n in 0..s.len() {
        let Some(sc) = d.scene(&s, n) else { continue };
        let mut all = Vec::new();
        flat(&sc.drawables, 0, &mut all);
        let text: String = all.iter().filter_map(|(_, x)| if let DrawableKind::Shape(sh) = &x.kind { sh.text.as_ref().map(|t| t.text.clone()) } else { None }).collect::<Vec<_>>().join(" ");
        if needle.as_ref().is_some_and(|nd| !text.to_lowercase().contains(nd.as_str())) {
            continue;
        }
        println!("== slide {}", n + 1);
        for (dep, x) in &all {
            let ind = "  ".repeat(dep + 1);
            let g = &x.geom;
            match &x.kind {
                DrawableKind::Shape(sh) => {
                    println!("{ind}#{} shape tmpl={} geom=({:.0},{:.0},{:.0},{:.0}) fill={:?} op={:?}", x.id, x.from_template, g.x, g.y, g.w, g.h, sh.fill, x.opacity);
                    if let Some(t) = &sh.text {
                        for p in &t.paragraphs {
                            for r in &p.runs {
                                let st = &r.style;
                                println!("{ind}   {:?} font={} size={} bold={} track={} bg={:?} caps={:?} color={:?} align={:?} ls={:?}", &t.text[r.start..r.end], st.font_name, st.size, st.bold, st.tracking, st.background, st.caps, st.color, p.align, p.line_spacing);
                            }
                        }
                    }
                }
                k => println!("{ind}#{} {} geom=({:.0},{:.0},{:.0},{:.0}) op={:?}", x.id, match k { DrawableKind::Image(_) => "image", DrawableKind::Group(_) => "group", _ => "other" }, g.x, g.y, g.w, g.h, x.opacity),
            }
        }
    }
}
