//! cargo run -p pg-core --example shapes -- file.pages : each floating shape's outline kind and
//! each chart's type, in insertion order
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    let mut ids: Vec<_> = p.floating().into_iter().map(|(_, id)| id).collect();
    ids.extend(k.objects_of("TSCH.ChartDrawableArchive").map(|(i, _)| i));
    ids.sort();
    for id in ids {
        let m = k.obj(id).unwrap();
        if let Some(c) = m.msg("unity") {
            println!("{id} chart {}", c.enum_name("chart_type").unwrap_or("?"));
            continue;
        }
        let ps = m.find_super("TSD.ShapeArchive").and_then(|s| s.msg("pathsource"));
        let kind = ps.map(|ps| {
            for f in ["point_path_source", "scalar_path_source", "bezier_path_source", "callout_path_source", "connection_line_path_source", "editable_path_source"] {
                if let Some(x) = ps.msg(f) {
                    let t = x.enum_name("type").map(|t| t.to_string()).unwrap_or_default();
                    return format!("{f} {t}");
                }
            }
            "other".to_string()
        });
        println!("{id} {} {:?}", m.type_name(), kind);
    }
}
