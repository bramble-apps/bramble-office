//! layoutswitch <file.key> <slide> <layout name> [out.key]: what changing a slide's layout does (checks).
use kn_model::edit::History;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let show = |d: &Document| {
        let sc = d.scene(&d.slides(), i).unwrap();
        println!("layout {:?} bg {:?}", d.layout_name(d.slides()[i].slide), sc.background);
        for x in &sc.drawables {
            println!("  #{} tmpl={} ph={:?} {:?}", x.id, x.from_template, x.placeholder, match &x.kind {
                kn_model::DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.text.chars().take(30).collect::<String>()),
                _ => None,
            });
        }
    };
    show(&d);
    println!("layouts: {:?}", d.layouts().iter().map(|l| &l.1).collect::<Vec<_>>());
    let l = d.layouts().into_iter().find(|l| l.1 == a[3]).expect("layout").0;
    let mut h = History::default();
    d.begin(&mut h, "Change Layout");
    println!("changed: {}", d.change_slide_layout(&mut h, s[i].slide, l));
    d.end(&mut h);
    show(&d);
    if let Some(out) = a.get(4) {
        d.save_as(out).unwrap();
    }
}
