//! cargo run -p pg-core --example charttypes -- file.pages : each chart's stored type
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    for (id, c) in p.file().objects_of("TSCH.ChartDrawableArchive") {
        let t = c.msg("unity").and_then(|u| u.enum_name("chart_type")).unwrap_or("?");
        println!("chart {id}: {t}");
    }
}
