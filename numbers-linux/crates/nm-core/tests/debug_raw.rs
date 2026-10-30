#[test]
#[ignore]
fn raw() {
    for f in ["34-comment", "29-base-data"] {
        let wb = nm_core::Workbook::open(format!("{}/../../testdata/{f}.numbers", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let model = wb.file().obj(wb.file().obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
        for ((r, c), rec) in nm_core::kn_model::table::raw_cell_records(wb.file(), model) {
            if rec.first() == Some(&5) {
                eprintln!("{f} r{r}c{c}: {}", rec.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
            }
        }
    }
}
