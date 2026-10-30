//! t43media <in.key> <out.key> <red.png> <green.png> <blue.png> <poster.png>: a 3-picture image
//! gallery and a web video on slide 1 (checks).
use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides()[0].slide;
    let read = |i: usize| std::fs::read(&a[i]).unwrap();
    let mut h = History::default();
    d.begin(&mut h, "Media");
    let g = d.insert(&mut h, s, &NewObject::Image(read(3), "red.png".into())).unwrap();
    d.make_gallery(&mut h, g, vec![(read(4), "green.png".into()), (read(5), "blue.png".into())]).unwrap();
    d.move_by(&mut h, g, -400.0, 0.0);
    let w = d.insert(&mut h, s, &NewObject::Image(read(6), "web-video-poster.png".into())).unwrap();
    assert!(d.make_web_video(&mut h, w, "https://www.youtube.com/watch?v=aqz-KE-bpKQ", "Big Buck Bunny", "Blender"));
    d.move_by(&mut h, w, 400.0, 0.0);
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
