#[test]
#[ignore]
fn donors() {
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("CascadeProjects/numbers-research-src/out-N2/B/files");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    for p in names {
        let wb = nm_core::Workbook::open(&p).unwrap();
        for s in wb.sheets() {
            let kinds: Vec<String> = s.drawables.iter().map(|d| wb.file().type_name(*d).to_string()).collect();
            let tabs: Vec<String> = wb.tables(&s).iter().map(|t| { let td = wb.table_data(t.info).unwrap(); format!("{} {}x{} h{}/{}/{}", t.name, td.rows, td.cols, td.header_rows, td.header_cols, td.footer_rows) }).collect();
            eprintln!("{} | {} | {:?} | {:?}", p.file_name().unwrap().to_string_lossy(), s.name, kinds, tabs);
        }
    }
}
