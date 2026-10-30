//! Outside border on a block (scratch check). usage: bordertest in.numbers out.numbers table r0 c0 r1 c1
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut wb = nm_core::Workbook::open(&a[1]).unwrap();
    let t: u64 = a[3].parse().unwrap();
    let n: Vec<u32> = a[4..8].iter().map(|x| x.parse().unwrap()).collect();
    let mut h = nm_core::kn_model::edit::History::default();
    let spec = nm_core::kn_model::format::StrokeSpec { color: nm_core::kn_model::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, width: 2.0, dash: nm_core::kn_model::format::Dash::Solid };
    wb.doc.begin(&mut h, "b");
    wb.doc.set_cells_border(&mut h, t, (n[0], n[1], n[2], n[3]), nm_core::kn_model::tablefmt::BorderEdges::Outside, Some(&spec)).unwrap();
    wb.doc.end(&mut h);
    wb.doc.save_as(&a[2]).unwrap();
}
