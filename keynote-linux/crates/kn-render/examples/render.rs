//! cargo run -p kn-render --example render -- file.key out_dir [width] [slide numbers...]
use kn_model::Document;
use kn_render::Renderer;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).expect("open");
    let out = std::path::Path::new(&a[2]);
    std::fs::create_dir_all(out).unwrap();
    let width: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(960);
    let slides = doc.slides();
    let pick: Vec<usize> = if a.len() > 4 { a[4..].iter().map(|s| s.parse::<usize>().unwrap() - 1).collect() } else { (0..slides.len()).collect() };
    let mut r = Renderer::new();
    let total = std::time::Instant::now();
    for i in pick {
        let t = std::time::Instant::now();
        let sc = doc.scene(&slides, i).unwrap();
        let pm = r.render_scene(&doc, &sc, width).unwrap();
        pm.save_png(out.join(format!("slide-{:03}.png", i + 1))).unwrap();
        let ms = t.elapsed().as_millis();
        if ms > 500 { println!("SLOW slide {} took {ms} ms", i + 1); }
    }
    println!("rendered in {:?}", total.elapsed());
}
