//! A table's stored row headers (index, size, other fields) and the heights we use.
//! usage: cargo run -p nm-core --example rowsizes -- file.numbers <sheet no.>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let s = &wb.sheets()[a[2].parse::<usize>().unwrap() - 1];
    for t in wb.tables(s) {
        let k = wb.file();
        let model = k.obj(t.info).and_then(|m| m.ref_id("tableModel")).and_then(|m| k.obj(m)).unwrap();
        println!("table {} default_row_height {:?}", t.info, model.f64("default_row_height"));
        let ds = model.msg("base_data_store").unwrap();
        for b in ds.msg("rowHeaders").map(|r| r.ref_ids("buckets")).unwrap_or_default() {
            for h in k.obj(b).unwrap().msgs("headers") {
                println!("  {h:?}");
            }
        }
        println!("  used: {:?}", wb.table_data(t.info).unwrap().row_heights);
    }
}
