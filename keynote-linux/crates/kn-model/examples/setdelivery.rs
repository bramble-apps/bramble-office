//! setdelivery <in.key> <out.key> <delivery name>: set the first build on slide 1 (checks).
use kn_model::buildedit::Delivery;
use kn_model::edit::History;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let build = d.file.obj(s[0].slide).unwrap().ref_ids("builds")[0];
    let dv = [Delivery::ByParagraph, Delivery::ByParagraphGroup, Delivery::ByHighlightedParagraph].into_iter().find(|x| x.name() == a[3]).expect("delivery");
    let mut h = History::default();
    d.begin(&mut h, "Delivery");
    assert!(d.set_build_delivery(&mut h, s[0].slide, build, dv));
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
