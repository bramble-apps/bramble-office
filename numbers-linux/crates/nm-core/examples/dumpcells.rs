//! Cells r0..=r1, c0..=c1 of the first table on sheet n (text | formula) and its merges there.
//! usage: dumpcells file.numbers sheet r0 c0 r1 c1
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let s = &wb.sheets()[a[2].parse::<usize>().unwrap() - 1];
    let n: Vec<u32> = a[3..7].iter().map(|x| x.parse().unwrap()).collect();
    let t = wb.tables(s)[0].info;
    let td = wb.table_data(t).unwrap();
    println!("table {} x {}", td.rows, td.cols);
    for r in n[0]..=n[2].min(td.rows.saturating_sub(1)) {
        let row: Vec<String> = (n[1]..=n[3].min(td.cols.saturating_sub(1)))
            .map(|c| {
                let v = td.cells.get((r * td.cols + c) as usize).cloned().unwrap_or_default().replace('\n', "⏎");
                let f = nm_core::formula::formula_text(wb.file(), t, r, c);
                let st = if std::env::var_os("STYLE").is_some() {
                    td.cell_styles.get((r * td.cols + c) as usize).map(|st| format!(" <{}{} {:.0}pt {:?}>", st.text.font_name, if st.text.bold { " bold" } else { "" }, st.text.size, st.fill)).unwrap_or_default()
                } else {
                    String::new()
                };
                match f {
                    Some(f) => format!("{v} [{f}]{st}"),
                    None => format!("{v}{st}"),
                }
            })
            .collect();
        println!("r{r}: {}", row.join(" | "));
    }
    for m in &td.merges {
        if m.row <= n[2] && m.row + m.rows > n[0] && m.col <= n[3] && m.col + m.cols > n[1] {
            println!("merge r{} c{} {}x{}", m.row, m.col, m.rows, m.cols);
        }
    }
}
