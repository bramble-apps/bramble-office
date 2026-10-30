//! cargo run -p pg-core --example presets -- file.pages : chart style presets and who lists them
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    let lists: Vec<_> = k.objects_of("TSCH.ChartPresetsArchive").map(|(id, m)| (id, m.ref_ids("chart_presets"))).collect();
    println!("preset lists {lists:?}");
    let all: Vec<_> = k.objects_of("TSCH.ChartStylePreset").map(|(id, m)| (id, k.file_of(id).unwrap_or("?").to_string(), m.str("uuid").map(|s| s.to_string()))).collect();
    println!("{} presets: {all:?}", all.len());
    println!("chart_presets() = {:?}", p.doc.chart_presets());
    for (id, c) in k.objects_of("TSCH.ChartDrawableArchive") {
        println!("chart {id} preset ref {:?}", c.ref_id("preset").or(c.msg("chart_info").and_then(|i| i.ref_id("preset"))));
    }
}
