//! cargo run -p pg-core --example chartpreset -- file.pages : each chart's preset / owned_preset
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    println!("{} presets in the file", k.objects_of("TSCH.ChartStylePreset").count());
    for (id, c) in k.objects_of("TSCH.ChartDrawableArchive") {
        let u = c.msg("unity");
        println!("chart {id}: preset {:?} owned_preset {:?}", u.and_then(|u| u.ref_id("preset")), u.and_then(|u| u.ref_id("owned_preset")));
    }
}
