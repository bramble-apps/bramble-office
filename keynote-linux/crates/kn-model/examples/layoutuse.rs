//! Which layouts of a deck have a slide using them (so Add Slide can copy one).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    for (l, n) in d.layouts() {
        let c = s.iter().filter(|x| d.file.obj(x.slide).and_then(|m| m.ref_id("template_slide")) == Some(l)).count();
        println!("{c:4}  {n} ({l})");
    }
}
