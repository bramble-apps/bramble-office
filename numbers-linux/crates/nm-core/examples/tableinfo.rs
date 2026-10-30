//! Each table's size, header counts and first row. usage: tableinfo file.numbers
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let ids: Vec<_> = wb.file().objects_of("TST.TableInfoArchive").map(|(id, _)| id).collect();
    for id in ids {
        if let Some(t) = wb.table_data(id) {
            println!("{id} {:?} {}x{} header rows {} cols {} first row {:?}", t.name, t.rows, t.cols, t.header_rows, t.header_cols, &t.cells[..t.cols as usize]);
        }
    }
}
