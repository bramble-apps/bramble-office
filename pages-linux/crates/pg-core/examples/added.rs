//! cargo run -p pg-core --example added -- file.pages : the floating objects (kind, arrows, outline)
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    let mut out = Vec::new();
    for (_, id) in p.floating() {
        let m = k.obj(id).unwrap();
        let d = pg_core::kn_model::scene::drawable(k, id, Default::default());
        let desc = match d.as_ref().map(|d| &d.kind) {
            Some(pg_core::kn_model::DrawableKind::Shape(s)) => {
                let ps = m.find_super("TSD.ShapeArchive").and_then(|s| s.msg("pathsource"));
                let kind = ps.and_then(|ps| ["point_path_source", "scalar_path_source", "bezier_path_source", "callout_path_source"].iter().find_map(|f| ps.msg(f).map(|x| format!("{} {}", f.trim_end_matches("_path_source"), x.enum_name("type").unwrap_or("")))));
                format!("{}{} {}", if s.is_text_box { "textbox" } else if s.is_line { "line" } else { "shape" }, if s.head.is_some() { "+arrow" } else { "" }, kind.unwrap_or_default())
            }
            Some(other) => format!("{:?}", std::mem::discriminant(other)),
            None => m.type_name().to_string(),
        };
        out.push(desc);
    }
    println!("{} object(s): {}", out.len(), out.join(" | "));
}
