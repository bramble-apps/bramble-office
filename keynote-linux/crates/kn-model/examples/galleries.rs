//! galleries <file.key>: list image galleries and web videos.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    for i in 0..s.len() {
        for x in d.scene(&s, i).unwrap().drawables {
            if let kn_model::DrawableKind::Image(im) = &x.kind {
                if !im.gallery.is_empty() || im.web_video.is_some() {
                    println!("slide {} #{} gallery {:?} web {:?}", i + 1, x.id, im.gallery, im.web_video);
                }
            }
        }
    }
}
