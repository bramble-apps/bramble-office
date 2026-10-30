//! List charts in a deck: slide, id, type, legend/title flags.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    for i in 0..s.len() {
        for x in d.scene(&s, i).unwrap().drawables {
            if let kn_model::DrawableKind::Chart(c) = &x.kind {
                println!("slide {} #{} {} legend={} rows={:?} cols={:?} vals={:?} by_row={} series={}", i + 1, x.id, c.chart_type, c.show_legend, c.row_names, c.col_names, c.values, c.series_by_row, c.series.len());
            }
        }
    }
}
