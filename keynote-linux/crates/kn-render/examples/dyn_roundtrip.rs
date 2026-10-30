//! Make a deck for checking dynamic backgrounds in Keynote: from a Standard deck, slide 1 gets
//! Ocean Floor, then its Speed is changed to 3; written to the given path.
//! cargo run --release -p kn-render --example dyn_roundtrip -- in.key out.key
use kn_model::edit::History;
use kn_model::motionbg::{read, MotionProp};
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides();
    let presets = d.motion_presets();
    let (ocean, _) = presets.iter().find(|p| p.1 == "Ocean Floor").cloned().unwrap();
    let size = d.slide_size();
    let mut h = History::default();
    let mb = read(&d.file, ocean).unwrap();
    let poster = kn_render::motion::poster_jpeg(&mb, size.w as u32, size.h as u32);
    d.begin(&mut h, "Dynamic");
    d.set_motion_background(&mut h, s[0].slide, Some(ocean), poster);
    d.end(&mut h);
    let mut look = d.motion_background(s[0].slide).unwrap();
    look.nums.insert("noise_speed", 3.0);
    let poster = kn_render::motion::poster_jpeg(&look, size.w as u32, size.h as u32);
    d.begin(&mut h, "Speed");
    d.set_motion_property(&mut h, s[0].slide, MotionProp::Num("noise_speed", 3.0), poster);
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("wrote {}", a[2]);
}
