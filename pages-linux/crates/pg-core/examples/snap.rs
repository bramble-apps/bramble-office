//! cargo run -p pg-core --example snap -- in.pages out-prefix [scale]
//! Renders every page to out-prefix-N.png.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let p = if path.extension().is_some_and(|e| e == "docx") { pg_core::import::import_file(path).expect("import") } else { pg_core::Pages::open(path).expect("open") };
    let scale: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut r = kn_render::Renderer::new();
    let mut cache = pg_core::flow::Cache::default();
    let t = std::time::Instant::now();
    let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
    eprintln!("layout {} pages in {:?}", l.pages.len(), t.elapsed());
    for i in 0..l.pages.len() {
        let pm = pg_core::render::render_page(&p, &l, i, &mut r, scale, Default::default()).unwrap();
        pm.save_png(format!("{}-{}.png", a[2], i + 1)).unwrap();
    }
}
