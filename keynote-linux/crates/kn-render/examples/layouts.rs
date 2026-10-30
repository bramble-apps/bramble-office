//! Render each layout of a deck (what the Add Slide picker shows): layouts deck.key out_dir
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    std::fs::create_dir_all(&a[2]).unwrap();
    let mut r = kn_render::Renderer::new();
    for (i, (id, name)) in d.layouts().iter().enumerate() {
        match d.layout_scene(*id) {
            Some(sc) => match r.render_scene(&d, &sc, 320) {
                Some(pm) => {
                    pm.save_png(format!("{}/{:02}.png", a[2], i)).unwrap();
                    println!("{i} {name}: {} objects", sc.drawables.len());
                }
                None => println!("{i} {name}: render failed"),
            },
            None => println!("{i} {name}: no scene"),
        }
    }
}
