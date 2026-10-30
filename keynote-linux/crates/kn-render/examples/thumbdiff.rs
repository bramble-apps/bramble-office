//! thumbdiff deck.key out_dir: every slide rendered next to Keynote's own saved thumbnail,
//! with how different they look (mean absolute difference per channel, 0-255, at 192 px).
//! Writes side-by-side PNGs of the slides that differ most. Prints "slide diff" lines.
use kn_model::Document;
use kn_render::Renderer;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = Document::open(&a[1]).unwrap();
    let out = std::path::PathBuf::from(&a[2]);
    std::fs::create_dir_all(&out).unwrap();
    let slides = d.slides();
    let mut r = Renderer::new();
    r.set_async_images(false);
    for (i, info) in slides.iter().enumerate() {
        let Some(node) = d.file.obj(info.node) else { continue };
        let thumb = ["thumbnails", "thumbnailData"].iter().flat_map(|f| node.msgs(f).filter_map(|m| m.u64("identifier")).collect::<Vec<_>>()).find_map(|id| d.file.data(id)).and_then(|(_, b)| image::load_from_memory(&b).ok());
        let Some(thumb) = thumb else {
            println!("{} - no thumbnail", i + 1);
            continue;
        };
        let Some(sc) = d.scene(&slides, i) else { continue };
        let Some(pm) = r.render_scene(&d, &sc, 384) else { continue };
        let ours = image::RgbaImage::from_raw(pm.width(), pm.height(), pm.data().to_vec()).unwrap();
        let w = 192u32;
        let h = (w as f32 * thumb.height() as f32 / thumb.width() as f32).round() as u32;
        let k = image::imageops::resize(&thumb.to_rgba8(), w, h, image::imageops::FilterType::Triangle);
        let o = image::imageops::resize(&ours, w, h, image::imageops::FilterType::Triangle);
        let mut sum = 0u64;
        for (p, q) in k.pixels().zip(o.pixels()) {
            for c in 0..3 {
                sum += (i32::from(p[c]) - i32::from(q[c])).unsigned_abs() as u64;
            }
        }
        let diff = sum as f64 / (w * h * 3) as f64;
        println!("{} {:.1}", i + 1, diff);
        if diff > 12.0 {
            let mut side = image::RgbaImage::from_pixel(w * 2 + 8, h, image::Rgba([255, 255, 255, 255]));
            image::imageops::overlay(&mut side, &k, 0, 0);
            image::imageops::overlay(&mut side, &o, i64::from(w + 8), 0);
            let side = image::imageops::resize(&side, (w * 2 + 8) * 2, h * 2, image::imageops::FilterType::Triangle);
            let _ = side.save(out.join(format!("slide-{:03}-{:.0}.png", i + 1, diff)));
        }
    }
}
