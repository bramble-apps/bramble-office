//! alldynamic <in.key> <out.key>: one new slide per dynamic background preset of the theme,
//! each with that background (checks: do backgrounds set by keynote-linux play in Keynote?).
use kn_model::edit::History;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let presets = d.motion_presets();
    let mut h = History::default();
    for (id, name) in presets {
        let at = d.slides().len();
        d.begin(&mut h, "New Slide");
        let i = d.new_slide_after(&mut h, at - 1).unwrap();
        d.end(&mut h);
        let slide = d.slides()[i].slide;
        d.begin(&mut h, "Dynamic Background");
        assert!(d.set_motion_background(&mut h, slide, Some(id), None), "{name}");
        d.end(&mut h);
        println!("slide {} {name}", i + 1);
    }
    d.save_as(&a[2]).unwrap();
}
