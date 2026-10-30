#[test]
#[ignore]
fn chart_extract() {
    let p = format!("{}/CascadeProjects/numbers-research-src/out-N2/B/files/N-donor-chart-2d-pie-chart.numbers", std::env::var("HOME").unwrap());
    let wb = nm_core::Workbook::open(&p).unwrap();
    let k = wb.file();
    let (chart, _) = k.first_of("TSCH.ChartDrawableArchive").unwrap();
    let ex = k.extract(chart).unwrap();
    for a in &ex.archives {
        eprintln!("owned #{} {} in {}", a.id(), k.type_name(a.id()), k.file_of(a.id()).unwrap());
    }
    for e in &ex.external {
        eprintln!("external #{} {} in {}", e, k.type_name(*e), k.file_of(*e).unwrap_or("?"));
    }
    let grid = wb.doc.chart_grid(chart);
    eprintln!("grid {:?}", grid);
}
