//! Apply one row / column insert or delete to the first table and save.
//! usage: rowop in.numbers out.numbers (row|col) (ins|del) index
fn main() {
    use nm_core::kn_model::tablerows::Axis;
    let a: Vec<String> = std::env::args().collect();
    let mut wb = nm_core::Workbook::open(&a[1]).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let axis = if a[3] == "row" { Axis::Row } else { Axis::Column };
    let i: u32 = a[5].parse().unwrap();
    let mut h = nm_core::kn_model::edit::History::default();
    wb.doc.begin(&mut h, "op");
    let r = if a[4] == "ins" { wb.doc.table_insert(&mut h, t, axis, i, i.saturating_sub(1)) } else { wb.doc.table_delete(&mut h, t, axis, i) };
    wb.doc.end(&mut h);
    r.unwrap();
    wb.doc.save_as(&a[2]).unwrap();
}
