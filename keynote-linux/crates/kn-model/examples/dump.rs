//! cargo run -p kn-model --example dump -- file.key [slide-number]
use kn_model::{Document, DrawableKind};
use std::collections::BTreeMap;

fn kind(d: &kn_model::Drawable) -> String {
    match &d.kind {
        DrawableKind::Shape(s) => if s.is_line { "line".into() } else if s.text.is_some() { "shape+text".into() } else { "shape".into() },
        DrawableKind::Image(i) => if i.equation.is_some() { "equation".into() } else { "image".into() },
        DrawableKind::Movie(_) => "movie".into(),
        DrawableKind::Group(_) => "group".into(),
        DrawableKind::Table(_) => "table".into(),
        DrawableKind::Chart(_) => "chart".into(),
        DrawableKind::Unsupported(t) => format!("UNSUPPORTED {t}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let doc = Document::open(&args[1]).expect("open");
    let slides = doc.slides();
    println!("opened in {:?}: {} slides, size {:?}, theme {:?}", t.elapsed(), slides.len(), doc.slide_size(), doc.theme_name());
    let t = std::time::Instant::now();
    let mut hist = BTreeMap::new();
    for i in 0..slides.len() {
        let sc = doc.scene(&slides, i).expect("scene");
        for d in &sc.drawables {
            *hist.entry(kind(d)).or_insert(0) += 1;
        }
    }
    println!("built all scenes in {:?}", t.elapsed());
    for (k, v) in hist { println!("{v:5} {k}"); }
    if let Some(n) = args.get(2) {
        let i: usize = n.parse::<usize>().unwrap() - 1;
        let sc = doc.scene(&slides, i).unwrap();
        println!("\nslide {} layout {:?} bg {:?} transition {:?}", n, doc.layout_name(slides[i].slide), sc.background, sc.transition);
        if let Some(m) = &sc.motion {
            println!("  motion style {} {:?} family {}", m.style, m.name, m.family);
        }
        for d in &sc.drawables {
            print!("- #{} {} geom ({:.0},{:.0} {:.0}x{:.0} rot {:.0}) op {}", d.id, kind(d), d.geom.x, d.geom.y, d.geom.w, d.geom.h, d.geom.angle, d.opacity);
            match &d.kind {
                DrawableKind::Shape(s) => {
                    print!(" fill {:?} stroke {:?}", s.fill, s.stroke.as_ref().map(|s| (s.width, s.color)));
                    if let Some(t) = &s.text {
                        print!(" text {:?}", t.text);
                        for p in &t.paragraphs { print!(" [{:?} {} {:.0}pt {:?}]", p.align, p.runs[0].style.font_name, p.runs[0].style.size, p.label); }
                    }
                }
                DrawableKind::Table(t) => print!(" {}x{} cells {:?}", t.rows, t.cols, t.cells),
                DrawableKind::Chart(c) => print!(" {} {:?} {:?}", c.chart_type, c.col_names, c.values),
                DrawableKind::Image(i) => print!(" data {:?} mask {}", i.data_id, i.mask.is_some()),
                _ => {}
            }
            println!();
        }
        println!("builds {:?}", sc.builds);
        println!("notes {:?}", doc.notes(slides[i].slide));
    }
}
