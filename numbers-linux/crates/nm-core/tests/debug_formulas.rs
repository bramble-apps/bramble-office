#[test]
#[ignore]
fn formula_texts() {
    for f in ["07-formula-same-table", "08-formula-absolute", "09-formula-whole-column", "10-formula-cross-table", "11-formula-cross-sheet", "12-merged-cells"] {
        let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).join(format!("{f}.numbers"));
        let wb = nm_core::Workbook::open(&p).unwrap();
        for s in wb.sheets() {
            for t in wb.tables(&s) {
                let td = wb.table_data(t.info).unwrap();
                for r in 0..td.rows {
                    for c in 0..td.cols {
                        if let Some(ft) = wb.doc.formula_text(t.info, r, c) {
                            eprintln!("{f} {}::{} r{r}c{c}: {ft} -> {}", s.name, t.name, td.cells[(r * td.cols + c) as usize]);
                        }
                    }
                }
            }
        }
    }
}
