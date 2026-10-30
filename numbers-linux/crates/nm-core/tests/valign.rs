#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn valign_reaches_cell_style() {
    use nm_core::kn_model::edit::History;
    let mut wb = nm_core::Workbook::open(format!("{}/../../testdata/29-base-data.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let mut h = History::default();
    wb.doc.begin(&mut h, "x");
    wb.doc.set_cells_text_prop(&mut h, t, (1, 0, 1, 0), &nm_core::kn_model::format::TextProp::VAlign(nm_core::kn_model::VAlign::Top)).unwrap();
    wb.doc.end(&mut h);
    let td = wb.table_data(t).unwrap();
    assert_eq!(td.cell_styles[td.cols as usize].valign, nm_core::kn_model::VAlign::Top);
}
