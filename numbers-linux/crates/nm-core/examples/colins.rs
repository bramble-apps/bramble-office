//! Insert a column into an imported .xlsx sheet and print each row's left stroke and fill of
//! the new column. usage: colins file.xlsx sheet(1-based) at like
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut wb = nm_core::xlsx::import_xlsx(std::path::Path::new(&a[1])).unwrap();
    let s = wb.sheets().remove(a[2].parse::<usize>().unwrap() - 1);
    let t = wb.tables(&s)[0].info;
    let (at, like): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let show = |wb: &nm_core::Workbook, c: u32| {
        let d = wb.table_data(t).unwrap();
        for r in 0..d.rows {
            let v = d.v_strokes.get((r * (d.cols + 1) + c) as usize).cloned().flatten().map(|s| s.width);
            let v2 = d.v_strokes.get((r * (d.cols + 1) + c + 1) as usize).cloned().flatten().map(|s| s.width);
            let fill = d.cell_styles.get((r * d.cols + c) as usize).map(|s| format!("{:?}", s.fill).chars().take(40).collect::<String>());
            let m = d.merge_at(r, c).map(|m| (m.row, m.col, m.rows, m.cols));
            println!("  r{r:>2} left {v:?} right {v2:?} fill {fill:?} merge {m:?}");
        }
    };
    println!("before (column {like}):");
    show(&wb, like);
    let mut h = nm_core::kn_model::edit::History::default();
    wb.doc.begin(&mut h, "Add Column");
    wb.doc.table_insert(&mut h, t, nm_core::kn_model::tablerows::Axis::Column, at, like).unwrap();
    wb.doc.end(&mut h);
    println!("after (new column {at}):");
    show(&wb, at);
}
