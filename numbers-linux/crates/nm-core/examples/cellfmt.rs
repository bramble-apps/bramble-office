//! The format of cells showing `text` on sheet `n`. usage: cellfmt file.numbers n text
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let s = &wb.sheets()[a[2].parse::<usize>().unwrap() - 1];
    for t in wb.tables(s) {
        let td = wb.table_data(t.info).unwrap();
        for (i, c) in td.cells.iter().enumerate() {
            if c == &a[3] {
                let (r, col) = (i as u32 / td.cols, i as u32 % td.cols);
                println!("table {} r{r} c{col}: {:?}", t.info, wb.doc.cell_format(t.info, r, col));
                return;
            }
        }
    }
}
