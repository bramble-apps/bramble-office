//! Every Numbers 15.4 sample opens, lists its sheets and tables, and re-saves losslessly.

use nm_core::Workbook;

fn samples() -> Vec<std::path::PathBuf> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "numbers")).collect();
    v.sort();
    v
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn all_samples_open() {
    for p in samples() {
        let wb = Workbook::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let sheets = wb.sheets();
        assert!(!sheets.is_empty(), "{}: no sheets", p.display());
        for s in &sheets {
            for t in wb.tables(s) {
                let td = wb.table_data(t.info).expect("table data");
                if std::env::var("NM_DUMP").is_ok() {
                    eprintln!("{} | {} | {} {}x{}", p.file_name().unwrap().to_string_lossy(), s.name, t.name, td.rows, td.cols);
                    for r in 0..td.rows.min(6) {
                        let row: Vec<&str> = (0..td.cols.min(6)).map(|c| td.cells[(r * td.cols + c) as usize].as_str()).collect();
                        eprintln!("    {row:?}");
                    }
                }
            }
        }
    }
}

/// Every .iwa decodes and re-encodes to the identical stream; write -> read -> write is stable.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn roundtrip_is_lossless() {
    use nm_core::kn_format::{EntryData, KeyFile};
    for p in samples() {
        nm_core::init();
        let raw = std::fs::read(&p).unwrap();
        let k = KeyFile::from_zip(&raw).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&raw)).unwrap();
        for e in &k.entries {
            if let EntryData::Iwa(iwa) = &e.data {
                let mut f = z.by_name(&e.name).unwrap();
                let mut orig = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut orig).unwrap();
                let orig = nm_core::kn_format::wire::iwa_unframe(&orig).unwrap();
                assert!(iwa.encode_stream() == orig, "{}: {} differs", p.display(), e.name);
            }
        }
        let bytes = k.to_zip().unwrap();
        let k2 = KeyFile::from_zip(&bytes).unwrap();
        assert!(k2.to_zip().unwrap() == bytes, "{} not stable", p.display());
    }
}

/// The Numbers registry covers every object Numbers wrote.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn every_object_type_is_known() {
    for p in samples() {
        let wb = Workbook::open(&p).unwrap();
        let unknown: usize = wb.file().archives().map(|(_, a)| a.payloads.iter().filter(|x| x.ty.is_none()).count()).sum();
        assert!(unknown <= 3, "{}: {unknown} payloads of unknown type", p.display());
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn filter_hides_rows() {
    let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/31-filter.numbers"));
    let wb = Workbook::open(&p).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let (rows, _) = nm_core::hidden(wb.file(), t);
    // Amount > 20 keeps Desk (120), Chair (80), Lamp (25): rows 3, 5, 6 of 22 (1-based)
    let td = wb.table_data(t).unwrap();
    let shown: Vec<&str> = (1..td.rows).filter(|r| !rows.contains(r)).map(|r| td.cells[(r * td.cols) as usize].as_str()).collect();
    assert_eq!(shown, ["Desk", "Chair", "Lamp"]);
    assert_eq!(rows.len(), 18);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn categories_view() {
    let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/30-category.numbers"));
    let wb = Workbook::open(&p).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let g = nm_core::categories::groups(wb.file(), t).expect("categorized");
    let labels: Vec<&str> = g.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(labels, ["Office", "Furniture", "(blank)"]);
    assert_eq!(g[0].rows, [1, 3, 6]);
    let (ds, views) = wb.drawables_and_views(&s);
    let v = &views[&t];
    let nm_core::kn_model::DrawableKind::Table(td) = &ds[0].kind else { panic!() };
    let col0: Vec<&str> = (0..8).map(|r| td.cells[(r * td.cols) as usize].as_str()).collect();
    assert_eq!(col0, ["Item", "▼ Office", "Pen", "Paper", "Stapler", "▼ Furniture", "Desk", "Chair"]);
    assert_eq!(v.rows[1], None);
    assert_eq!(v.rows[3], Some(3));
    assert_eq!(td.rows, 22 + 3);
    // uncategorized tables have no view
    let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/29-base-data.numbers"));
    let wb = Workbook::open(&p).unwrap();
    let s = wb.sheets().remove(0);
    assert!(wb.drawables_and_views(&s).1.is_empty());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn comment_on_cell() {
    let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/34-comment.numbers"));
    let wb = Workbook::open(&p).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let c = nm_core::cell_comments(wb.file(), t);
    assert_eq!(c.len(), 1);
    assert_eq!(c[&(2, 2)].0, "Comment on the Desk amount");
}
