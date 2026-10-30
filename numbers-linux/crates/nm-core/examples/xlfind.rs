//! Where text sits in an imported .xlsx (sheet 1-based), and those rows' heights and borders.
//! usage: xlfind file.xlsx sheet text
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::xlsx::import_xlsx(std::path::Path::new(&a[1])).unwrap();
    let s = wb.sheets().remove(a[2].parse::<usize>().unwrap() - 1);
    let d = wb.table_data(wb.tables(&s)[0].info).unwrap();
    for r in 0..d.rows {
        for c in 0..d.cols {
            if d.cells[(r * d.cols + c) as usize].contains(&a[3]) {
                println!("({r},{c}) {:?}", d.cells[(r * d.cols + c) as usize]);
                for rr in r.saturating_sub(2)..(r + 3).min(d.rows) {
                    let line: Vec<String> = (0..d.cols.min(12)).map(|cc| d.h_strokes.get((rr * d.cols + cc) as usize).cloned().flatten().map(|s| format!("{:.2}", s.width)).unwrap_or("-".into())).collect();
                    println!("  row {rr} h {:.1} top: {}", d.row_heights[rr as usize], line.join(" "));
                }
            }
        }
    }
}
