fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    let s = d.slides();
    for i in 0..s.len() {
        let Some(sc) = d.scene(&s, i) else { continue };
        if let Some(x) = sc.drawables.iter().find(|x| x.id == id) {
            if let kn_model::DrawableKind::Shape(sh) = &x.kind { println!("slide {} stroke {:?}", i + 1, sh.stroke); }
        }
    }
    // raw style stroke
    let st = d.file.obj(id).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.ref_id("style"));
    for m in kn_model::style::chain(&d.file, st) {
        if let Some(sp) = m.find_super("TSD.ShapeStyleArchive").and_then(|x| x.msg("shape_properties")).or(m.msg("shape_properties")) {
            if let Some(s) = sp.msg("stroke") { println!("raw stroke type {} fields {:?}", s.type_name(), s.fields.iter().map(|f| f.num).collect::<Vec<_>>()); println!("{s:?}"); break; }
        }
    }
}
