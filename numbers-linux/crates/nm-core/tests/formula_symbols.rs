//! A formula shown with Numbers' own operators (× ÷ − ≥ ≤ ≠) can be typed back unchanged.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn numbers_operators_parse() {
    use nm_core::kn_model::edit::History;
    let mut wb = nm_core::Workbook::open(format!("{}/../../testdata/29-base-data.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let mut h = History::default();
    for (src, want) in [("=C2×2", "6"), ("=C3÷4", "30"), ("=C3−C2", "117"), ("=IF(C2≥3,1,0)", "1"), ("=IF(C2≤2,1,0)", "0"), ("=IF(C2≠3,1,0)", "0")] {
        wb.doc.begin(&mut h, "x");
        let r = wb.doc.set_formula_text(&mut h, t, 9, 4, src);
        wb.doc.end(&mut h);
        assert!(r.is_ok(), "{src}: {r:?}");
        let td = wb.table_data(t).unwrap();
        assert_eq!(td.cells[(9 * td.cols + 4) as usize], want, "{src}");
    }
}
