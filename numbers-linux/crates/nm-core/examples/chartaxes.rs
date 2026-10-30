//! Every chart's axis scale settings (raw tschchartaxisvaluescale values). usage: chartaxes file.numbers
fn walk(m: &nm_core::kn_format::Msg, path: &str) {
    for f in &m.fields {
        if let nm_core::kn_format::Val::Msg(c) = &f.val {
            walk(c, &format!("{path}.{}", f.num));
        }
    }
    if let Some(v) = m.i64("tschchartaxisvaluescale") {
        println!("  {path} {} valuescale={v}", m.type_name());
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    for (id, m) in wb.file().objects_of("TSCH.ChartDrawableArchive") {
        println!("chart {id}");
        walk(m, "");
    }
    for t in ["TSCH.ChartAxisStyleArchive", "TSCH.ChartAxisNonStyleArchive", "TSCH.ChartStyleArchive"] {
        for (id, m) in wb.file().objects_of(t) {
            println!("{t} {id}");
            walk(m, "");
        }
    }
}
