#[test]
#[ignore]
fn bulk_rows_and_cells() {
    use nm_core::kn_model::edit::History;
    use nm_core::kn_model::tablecells::CellInput;
    use nm_core::kn_model::tablerows::Axis;
    nm_core::init();
    let k = nm_core::kn_format::KeyFile::from_zip(nm_core::BLANK).unwrap();
    let mut wb = nm_core::Workbook { doc: nm_core::kn_model::Document::from_file(k) };
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let t0 = std::time::Instant::now();
    wb.doc.begin(&mut h, "x");
    for _ in 0..200 {
        wb.doc.table_insert(&mut h, t, Axis::Row, 22, 21).unwrap();
    }
    eprintln!("200 row inserts: {:?}", t0.elapsed());
    let t1 = std::time::Instant::now();
    for r in 1..201 {
        for c in 0..5 {
            wb.doc.set_cell(&mut h, t, r, c, &CellInput::Number((r * c) as f64)).unwrap();
        }
    }
    eprintln!("1000 set_cell: {:?}", t1.elapsed());
    wb.doc.end(&mut h);
}
