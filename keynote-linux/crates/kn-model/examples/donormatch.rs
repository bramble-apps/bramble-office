//! For a deck, report which theme files share its layouts (same id and name): candidates for Add Slide donors.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let ls = d.layouts();
    println!("{} layouts in deck", ls.len());
    for e in std::fs::read_dir(&a[2]).unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("key") { continue }
        let Ok(t) = kn_model::Document::open(&p) else { continue };
        let tl = t.layouts();
        let same = ls.iter().filter(|l| tl.contains(l)).count();
        let slides = t.slides().iter().filter_map(|s| t.file.obj(s.slide).and_then(|m| m.ref_id("template_slide"))).filter(|x| ls.iter().any(|l| l.0 == *x)).count();
        if same > 0 { println!("{:?}: {same}/{} layouts match, {slides} donor slides", p.file_name().unwrap(), tl.len()); }
    }
}
