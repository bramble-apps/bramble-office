//! galleryins <in.key> <out.key> <a.png> <b.png> <c.png>: Insert > Image Gallery on slide 1
//! with three pictures, as the app does (gallery + Keynote's automatic Appear build).
use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let s = d.slides()[0].slide;
    let read = |i: usize| std::fs::read(&a[i]).unwrap();
    let mut h = History::default();
    d.begin(&mut h, "Insert Image");
    let id = d.insert(&mut h, s, &NewObject::Image(read(3), "one.png".into())).unwrap();
    d.make_gallery(&mut h, id, vec![(read(4), "two.png".into()), (read(5), "three.png".into())]).unwrap();
    d.sync_gallery_build(&mut h, s, id, 3).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("gallery {id}");
}
