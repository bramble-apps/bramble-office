//! listlayouts theme.key: the theme's layouts (id, name) and what each holds.
use kn_model::DrawableKind;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    for (id, name) in d.layouts() {
        let sc = d.layout_scene(id);
        let mut parts = Vec::new();
        if let Some(sc) = sc {
            for x in &sc.drawables {
                let k = match &x.kind {
                    DrawableKind::Shape(s) if s.text.is_some() => "text",
                    DrawableKind::Shape(_) => "shape",
                    DrawableKind::Image(_) => "image",
                    _ => "other",
                };
                parts.push(format!("{k}{}@{:.0},{:.0} {:.0}x{:.0}", x.placeholder.map(|p| format!("[{p:?}]")).unwrap_or_default(), x.geom.x, x.geom.y, x.geom.w, x.geom.h));
            }
        }
        println!("{id}\t{name}\t{}", parts.join("  "));
    }
}
