//! cargo run --release -p kn-render --example motionbg -- deck.key out.png [t]
//! Contact sheet of every slide's dynamic background at time t (3 per row, 480x270 each).
use kn_model::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let t: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let slides = doc.slides();
    let bgs: Vec<_> = slides.iter().filter_map(|s| doc.motion_background(s.slide)).collect();
    let (w, h) = (480u32, 270u32);
    let rows = bgs.len().div_ceil(3) as u32;
    let mut pm = tiny_skia::Pixmap::new(w * 3, h * rows.max(1)).unwrap();
    for (i, bg) in bgs.iter().enumerate() {
        let t0 = std::time::Instant::now();
        let px = kn_render::motion::render(bg, t, w, h);
        eprintln!("{:2} {:14} {:28} {:?}", i + 1, bg.name, bg.family, t0.elapsed());
        let (ox, oy) = ((i as u32 % 3) * w, (i as u32 / 3) * h);
        let data = pm.data_mut();
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + x) * 4) as usize;
                let d = (((oy + y) * w * 3 + ox + x) * 4) as usize;
                data[d..d + 4].copy_from_slice(&px[s..s + 4]);
            }
        }
    }
    pm.save_png(&a[2]).unwrap();
}
