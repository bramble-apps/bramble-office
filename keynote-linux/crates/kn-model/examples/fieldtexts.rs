//! fieldtexts <file.key>: text objects on slides whose text holds fields (U+FFFC).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let Ok(d) = kn_model::Document::open(&a[1]) else { return };
    let s = d.slides();
    for i in 0..s.len() {
        let Some(sc) = d.scene(&s, i) else { continue };
        for x in &sc.drawables {
            if let kn_model::DrawableKind::Shape(sh) = &x.kind {
                if let Some(t) = sh.text.as_ref().filter(|t| !t.fields.is_empty() && !x.from_template) {
                    println!("{} slide {} #{} {:?} fields {:?}", a[1], i + 1, x.id, t.text, t.fields);
                }
            }
        }
    }
}
