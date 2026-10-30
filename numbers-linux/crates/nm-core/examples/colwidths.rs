//! Column widths and row heights of the first table on sheet n.
//! usage: colwidths file.numbers sheet
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let n: usize = a[2].parse().unwrap();
    let s = wb.sheets().remove(n - 1);
    let t = wb.tables(&s)[0].info;
    let d = wb.table_data(t).unwrap();
    println!("name {:?}", d.name);
    println!("cols {:?}", d.col_widths.iter().map(|w| w.round()).collect::<Vec<_>>());
    println!("rows {:?}", d.row_heights.iter().take(20).map(|w| w.round()).collect::<Vec<_>>());
}
