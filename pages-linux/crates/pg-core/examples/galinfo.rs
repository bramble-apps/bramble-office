//! cargo run -p pg-core --example galinfo -- file.pages : galleries and web videos it holds
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    for pg in &l.pages {
        for d in pg_core::flow::page_drawables(&p, pg) {
            if let pg_core::kn_model::DrawableKind::Image(im) = &d.kind {
                println!("image {} gallery {} caption_mode {} web {:?}", d.id, im.gallery.len(), im.gallery_caption_mode, im.web_video.as_ref().map(|w| &w.url));
            }
        }
    }
}
