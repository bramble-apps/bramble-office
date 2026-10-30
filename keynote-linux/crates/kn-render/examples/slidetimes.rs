//! slidetimes deck.key width: render time of every slide (slowest first).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let d = kn_model::Document::open(&a[1]).unwrap();
    println!("open {:?}", t.elapsed());
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1600);
    let s = d.slides();
    let mut r = kn_render::Renderer::new();
    r.set_async_images(false);
    let mut v = Vec::new();
    let all = std::time::Instant::now();
    for i in 0..s.len() {
        let t = std::time::Instant::now();
        let sc = d.scene(&s, i).unwrap();
        let ts = t.elapsed();
        let _ = r.render_scene(&d, &sc, w);
        v.push((t.elapsed(), ts, i + 1));
    }
    println!("all {} slides {:?}", s.len(), all.elapsed());
    v.sort_by(|a, b| b.0.cmp(&a.0));
    for (tt, ts, i) in v.iter().take(8) { println!("slide {i}: {tt:?} (scene {ts:?})"); }
}
