//! gallerystages <file.key> <out_dir> [width]: every image of every gallery, as the slide shows
//! it at that build step (compare with Keynote's "each stage of builds" PDF).
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let w: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let slides = doc.slides();
    let mut r = kn_render::Renderer::new();
    let mut n = 0;
    for i in 0..slides.len() {
        let Some(sc) = doc.scene(&slides, i) else { continue };
        let galleries: Vec<(u64, usize)> = sc.drawables.iter().filter_map(|d| match &d.kind {
            DrawableKind::Image(im) if !im.gallery.is_empty() => Some((d.id, im.gallery.len())),
            _ => None,
        }).collect();
        let count = galleries.first().map(|g| g.1).unwrap_or(1);
        for k in 0..count {
            let mut s = sc.clone();
            for (id, _) in &galleries {
                s.show_gallery_image(*id, k);
            }
            n += 1;
            if let Some(pm) = r.render_scene(&doc, &s, w) {
                pm.save_png(format!("{}/stage-{n:02}.png", a[2])).unwrap();
            }
        }
    }
    println!("{n} stages");
}
