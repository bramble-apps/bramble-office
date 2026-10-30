//! Open an Excel workbook as Values does and write it back. usage: xlsxround in.xlsx out.xlsx
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let o = nm_core::xlsx::open_xlsx(std::path::Path::new(&a[1])).unwrap();
    println!("compat {} extras sheets {:?}", o.compat, o.extras.keys().collect::<Vec<_>>());
    let n = nm_core::xlsx::write_xlsx_full(&o.workbook, std::path::Path::new(&a[2]), o.compat, &o.extras).unwrap();
    println!("wrote {n}");
}
