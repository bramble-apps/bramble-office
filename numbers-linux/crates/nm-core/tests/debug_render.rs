#[test]
#[ignore]
fn show() {
    let wb = nm_core::Workbook::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/N-functions.numbers")).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let td = wb.table_data(t).unwrap();
    let mut n = 0;
    for r in 1..td.rows {
        let f = nm_core::formula::formula_text(wb.file(), t, r, 1);
        if f.is_none() { n += 1; eprintln!("NONE {}", td.cells[(r * td.cols) as usize]); }
        else if r % 23 == 0 { eprintln!("{} | {}", td.cells[(r * td.cols) as usize], f.unwrap()); }
    }
    eprintln!("declined {n}");
}
