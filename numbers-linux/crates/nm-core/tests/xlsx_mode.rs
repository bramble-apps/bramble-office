//! Excel mode: a workbook saved as .xlsx and opened again keeps its look, values and dates;
//! with the Values copy (compatibility mode) it comes back whole, unless another app changed it.
use nm_core::kn_model::edit::History;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::table::CellValue;
use nm_core::Workbook;

fn out(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-xlsx"));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn sample() -> Workbook {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/Blank.numbers");
    let mut wb = Workbook::open(&p).unwrap();
    let t = wb.tables(&wb.sheets()[0])[0].info;
    let mut h = History::default();
    let red = nm_core::kn_model::Color { r: 0.9, g: 0.1, b: 0.1, a: 1.0 };
    wb.doc.begin(&mut h, "Fill");
    wb.doc.set_cell(&mut h, t, 1, 1, &CellInput::Text("Budget".into())).unwrap();
    wb.doc.set_cell(&mut h, t, 2, 1, &CellInput::Number(12.5)).unwrap();
    wb.doc.set_cell(&mut h, t, 3, 1, &CellInput::Date(9500.0 * 86400.0)).unwrap();
    wb.doc.set_cells_fill(&mut h, t, (1, 1, 1, 1), Some(&nm_core::kn_model::format::FillSpec::Color(red))).unwrap();
    wb.doc.set_cells_text_prop(&mut h, t, (1, 1, 1, 1), &nm_core::kn_model::format::TextProp::Bold(true)).unwrap();
    wb.doc.set_formula_text(&mut h, t, 4, 1, "=B3*2").unwrap();
    wb.doc.end(&mut h);
    wb
}

#[test]
fn excel_round_trip_keeps_values_and_look() {
    let wb = sample();
    let p = out("plain.xlsx");
    nm_core::xlsx::write_xlsx(&wb, &p).unwrap();
    let o = nm_core::xlsx::open_xlsx(&p).unwrap();
    assert!(!o.compat && !o.changed_elsewhere);
    let w = o.workbook;
    let t = w.tables(&w.sheets()[0])[0].info;
    let v = w.doc.cell_values(t);
    assert_eq!(v.get(&(1, 1)), Some(&CellValue::Text("Budget".into())));
    assert_eq!(v.get(&(2, 1)), Some(&CellValue::Number(12.5)));
    assert_eq!(v.get(&(3, 1)), Some(&CellValue::Date(9500.0 * 86400.0)), "a date stays a date");
    assert_eq!(v.get(&(4, 1)), Some(&CellValue::Number(25.0)));
    assert_eq!(w.doc.formula_text(t, 4, 1).as_deref(), Some("=B3×2"), "the formula is kept (shown as Numbers shows it)");
    let td = w.table_data(t).unwrap();
    let st = &td.cell_styles[(td.cols + 1) as usize];
    assert!(st.text.bold, "bold kept");
    assert!(matches!(st.fill, nm_core::kn_model::Fill::Color(c) if c.r > 0.8 && c.g < 0.2), "fill kept: {:?}", st.fill);
}

#[test]
fn compatibility_copy_comes_back_whole() {
    let wb = sample();
    let p = out("compat.xlsx");
    nm_core::xlsx::write_xlsx_with(&wb, &p, true).unwrap();
    let o = nm_core::xlsx::open_xlsx(&p).unwrap();
    assert!(o.compat && !o.changed_elsewhere);
    // the very document (same objects), not a re-import
    assert_eq!(o.workbook.file().objects_of("TST.TableInfoArchive").count(), wb.file().objects_of("TST.TableInfoArchive").count());
    let t = o.workbook.tables(&o.workbook.sheets()[0])[0].info;
    assert_eq!(o.workbook.doc.cell_values(t).get(&(1, 1)), Some(&CellValue::Text("Budget".into())));
    // another app saves the workbook (a worksheet changes): the Excel parts win
    let bytes = std::fs::read(&p).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let p2 = out("compat-edited.xlsx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&p2).unwrap());
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().map(|n| n.to_string()).unwrap();
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut b).unwrap();
        if name == "xl/worksheets/sheet1.xml" {
            b = String::from_utf8(b).unwrap().replace("12.5", "13.5").into_bytes();
        }
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut w, &b).unwrap();
    }
    w.finish().unwrap();
    let o = nm_core::xlsx::open_xlsx(&p2).unwrap();
    assert!(o.changed_elsewhere);
    let t = o.workbook.tables(&o.workbook.sheets()[0])[0].info;
    assert_eq!(o.workbook.doc.cell_values(t).get(&(2, 1)), Some(&CellValue::Number(13.5)));
}
