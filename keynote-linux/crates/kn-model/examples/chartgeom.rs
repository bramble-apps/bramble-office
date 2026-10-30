//! chartgeom file: each chart's frame, legend frame and legend text size.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for (id, m) in k.objects_of("TSCH.ChartDrawableArchive") {
        let g = m.find_super("TSD.DrawableArchive").and_then(|d| d.msg("geometry"));
        let size = g.and_then(|g| g.msg("size")).map(|s| (s.f32("width"), s.f32("height")));
        let c = kn_model::chart::chart(&k, m).unwrap();
        println!("chart #{id} size {size:?} legend_frame {:?} legend {} {} series {:?}", c.legend_frame, c.legend_style.font_name, c.legend_style.size, c.series.iter().map(|s| &s.name).collect::<Vec<_>>());
    }
}
