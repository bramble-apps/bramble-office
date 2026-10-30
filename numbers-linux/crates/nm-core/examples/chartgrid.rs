//! Every chart's grid and cell link. usage: chartgrid file.numbers
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let ids: Vec<_> = wb.file().objects_of("TSCH.ChartDrawableArchive").map(|(id, _)| id).collect();
    for id in ids {
        println!("chart {id}: {:?}", wb.doc.chart_grid(id));
        println!("  link: {:?}", nm_core::chartlink::link(&wb.doc, id));
    }
}
