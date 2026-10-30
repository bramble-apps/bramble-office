//! Frozen headers and a cell's wrap flag (first table of sheet n).
//! usage: cellflags file.numbers sheet row col
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let n: usize = a[2].parse().unwrap();
    let (r, c): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let s = wb.sheets().remove(n - 1);
    let t = wb.tables(&s)[0].info;
    let d = wb.table_data(t).unwrap();
    println!("rows frozen {} cols frozen {}", nm_core::sheets::headers_frozen(&wb.doc, t, true), nm_core::sheets::headers_frozen(&wb.doc, t, false));
    println!("wrap at {r},{c}: {:?}", d.cell_styles.get((r * d.cols + c) as usize).map(|s| s.wrap));
}
