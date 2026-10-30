//! Outline a block, then remove it ("No Border"); print the block's edge strokes each time.
//! usage: borderoff file.numbers r0 c0 r1 c1
fn main() {
    use nm_core::kn_model::tablefmt::BorderEdges;
    let a: Vec<String> = std::env::args().collect();
    let mut wb = nm_core::Workbook::open(&a[1]).unwrap();
    let n: Vec<u32> = a[2..6].iter().map(|x| x.parse().unwrap()).collect();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let show = |wb: &nm_core::Workbook, label: &str| {
        let d = wb.table_data(t).unwrap();
        let top: Vec<bool> = (n[1]..=n[3]).map(|c| d.h_strokes.get((n[0] * d.cols + c) as usize).cloned().flatten().is_some()).collect();
        let bottom: Vec<bool> = (n[1]..=n[3]).map(|c| d.h_strokes.get(((n[2] + 1) * d.cols + c) as usize).cloned().flatten().is_some()).collect();
        println!("{label}: top {top:?} bottom {bottom:?}");
    };
    show(&wb, "before");
    let mut h = nm_core::kn_model::edit::History::default();
    let spec = nm_core::kn_model::format::StrokeSpec { color: nm_core::kn_model::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, width: 2.0, dash: nm_core::kn_model::format::Dash::Solid };
    wb.doc.begin(&mut h, "b");
    println!("{:?}", wb.doc.set_cells_border(&mut h, t, (n[0], n[1], n[2], n[3]), BorderEdges::Outside, Some(&spec)));
    wb.doc.end(&mut h);
    show(&wb, "outlined");
    wb.doc.begin(&mut h, "c");
    println!("{:?}", wb.doc.set_cells_border(&mut h, t, (n[0], n[1], n[2], n[3]), BorderEdges::Outside, None));
    wb.doc.end(&mut h);
    show(&wb, "no border");
}
