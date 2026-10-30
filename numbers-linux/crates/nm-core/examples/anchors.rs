//! The header bytes (0..8) of each merge's first cell. usage: anchors file.numbers sheet
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let s = &wb.sheets()[a[2].parse::<usize>().unwrap() - 1];
    for t in wb.tables(s) {
        let td = wb.table_data(t.info).unwrap();
        let k = wb.file();
        let model = k.obj(t.info).and_then(|m| m.ref_id("tableModel")).and_then(|m| k.obj(m)).unwrap();
        let recs = nm_core::kn_model::table::raw_cell_records(k, model);
        for m in td.merges.iter().take(12) {
            let raw = recs.iter().find(|(rc, _)| *rc == (m.row, m.col)).map(|(_, b)| b[..12.min(b.len())].to_vec());
            println!("merge r{} c{} {}x{} head {:02x?} fmt {:?}", m.row, m.col, m.rows, m.cols, raw, wb.doc.cell_format(t.info, m.row, m.col));
        }
    }
}
