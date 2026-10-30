//! Every .xlsx Numbers exported in the research opens, and the result saves and reopens.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn research_xlsx_exports_import() {
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/numbers-research-src/Numbers-Research/exports");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "xlsx") {
            continue;
        }
        let mut wb = nm_core::xlsx::import_xlsx(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let out = std::env::temp_dir().join(format!("nm-xlsx-{}.numbers", std::process::id()));
        wb.doc.save_as(&out).unwrap();
        let back = nm_core::Workbook::open(&out).unwrap();
        let cells: usize = back.sheets().iter().flat_map(|s| back.tables(s)).map(|t| back.table_data(t.info).unwrap().cells.iter().filter(|c| !c.is_empty()).count()).sum();
        eprintln!("{}: {} sheets, {cells} filled cells", p.file_name().unwrap().to_string_lossy(), back.sheets().len());
        let _ = std::fs::remove_file(out);
        n += 1;
    }
    eprintln!("{n} workbooks");
}
