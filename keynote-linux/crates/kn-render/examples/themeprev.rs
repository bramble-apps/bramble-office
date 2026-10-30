//! cargo run --release -p kn-render --example themeprev -- deck.key out.png
//! Preview PNG of a theme's first slide (dynamic backgrounds drawn at t = 0).
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let s = doc.slides();
    let sc = doc.scene(&s, 0).unwrap();
    let w = 480u32;
    let h = (w as f32 * sc.size.h / sc.size.w) as u32;
    let mut r = kn_render::Renderer::new();
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    if let Some(m) = &sc.motion {
        let px = kn_render::motion::render(m, 0.0, w, h);
        pm.data_mut().copy_from_slice(&px);
    }
    let k = w as f32 / sc.size.w;
    r.draw_scene(&doc, &sc, &mut pm, tiny_skia::Transform::from_scale(k, k), kn_render::Filter::All, sc.motion.is_none());
    pm.save_png(&a[2]).unwrap();
}
