//! tablepresets <file.key>: the theme's table style presets (index, thumbnail object, network).
use kn_model::Document;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = Document::open(&a[1]).unwrap();
    for (pid, img, net) in d.table_style_presets() {
        let it = img.map(|i| d.file.type_name(i).to_string()).unwrap_or_default();
        println!("preset {pid} image {img:?} ({it}) network {net:?}");
        if let Some(i) = img {
            if let Some(m) = d.file.obj(i) {
                println!("   {:?}", m.msg("data").and_then(|x| x.u64("identifier")).or(m.u64("identifier")));
            }
        }
    }
}
