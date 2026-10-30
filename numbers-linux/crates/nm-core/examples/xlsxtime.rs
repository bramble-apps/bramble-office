//! How long an .xlsx takes to open, and the size of each sheet. usage: xlsxtime file.xlsx
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let bytes = std::fs::read(&a[1]).unwrap();
    let sheets = nm_core::xlsx::read_xlsx(&bytes).unwrap();
    println!("read in {:?}", t.elapsed());
    for (n, rows) in &sheets {
        println!("  {n}: {} rows, {} columns", rows.len(), rows.iter().map(|r| r.len()).max().unwrap_or(0));
    }
    let t = std::time::Instant::now();
    let wb = nm_core::xlsx::import_xlsx(std::path::Path::new(&a[1])).unwrap();
    println!("imported in {:?}: {} sheets", t.elapsed(), wb.sheets().len());
    // what one full table read costs (edits that rebuild it pay this each time)
    let s = &wb.sheets()[0];
    let info = wb.tables(s)[0].info;
    let t = std::time::Instant::now();
    for _ in 0..10 {
        let m = wb.file().obj(info).unwrap();
        let _ = nm_core::kn_model::table::table(wb.file(), m);
    }
    println!("table(): {:?} each", t.elapsed() / 10);
    let t = std::time::Instant::now();
    let model = wb.file().obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|id| wb.file().obj(id)).unwrap();
    for _ in 0..10 {
        let _ = nm_core::kn_model::table::raw_cell_records(wb.file(), model);
    }
    println!("raw_cell_records(): {:?} each", t.elapsed() / 10);
}
