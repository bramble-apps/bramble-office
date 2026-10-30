//! A new presentation from a theme file, with a few slides added from layouts and some text.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::new_from_theme(kn_format::KeyFile::open(&a[1]).unwrap()).unwrap();
    let mut h = kn_model::edit::History::default();
    let layouts = d.layouts();
    d.begin(&mut h, "t");
    for (i, (l, _)) in layouts.iter().enumerate().skip(1).take(4) {
        d.new_slide_from_layout(&mut h, *l, i).unwrap();
    }
    let s = d.slides();
    let title = d.file.obj(s[0].slide).and_then(|x| x.ref_id("titlePlaceholder")).and_then(|p| d.file.obj(p)).and_then(|m| m.find_super("TSWP.ShapeInfoArchive")).and_then(|i| i.ref_id("owned_storage")).unwrap();
    d.replace_text(&mut h, title, 0, 0, "Made on Linux");
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
