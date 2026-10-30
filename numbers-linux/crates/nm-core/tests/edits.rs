//! Keydeck's table edits applied to Numbers files: each one saves, reopens and reads back.
//! The saved files (target/n-edits/) go to the Mac to be opened in real Numbers 15.4.

use nm_core::kn_model::edit::History;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::kn_model::{CellRange, Color};
use nm_core::Workbook;

fn testdata(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).join(name)
}

fn out(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Open `src`, run `f` on its first table, save as `name`, reopen and return the cells of that table.
fn edit(src: &str, name: &str, f: impl FnOnce(&mut Workbook, &mut History, nm_core::kn_format::ObjId)) -> (Workbook, nm_core::kn_model::TableData) {
    let mut wb = Workbook::open(testdata(src)).unwrap();
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    wb.doc.begin(&mut h, "edit");
    f(&mut wb, &mut h, t);
    wb.doc.end(&mut h);
    wb.doc.save_as(out(name)).unwrap();
    let back = Workbook::open(out(name)).unwrap();
    let s = back.sheets().remove(0);
    let t = back.tables(&s)[0].info;
    let td = back.table_data(t).unwrap();
    (back, td)
}

fn cell(t: &nm_core::kn_model::TableData, r: u32, c: u32) -> &str {
    &t.cells[(r * t.cols + c) as usize]
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n01_values() {
    let (_, t) = edit("00-empty.numbers", "N01-values.numbers", |wb, h, t| {
        wb.doc.set_cell(h, t, 1, 1, &CellInput::Number(42.5)).unwrap();
        wb.doc.set_cell(h, t, 2, 1, &CellInput::Number(-3.0)).unwrap();
        wb.doc.set_cell(h, t, 1, 2, &CellInput::Text("Grüße".into())).unwrap();
        wb.doc.set_cell(h, t, 0, 1, &CellInput::Text("Header".into())).unwrap();
    });
    assert_eq!((cell(&t, 1, 1), cell(&t, 2, 1), cell(&t, 1, 2), cell(&t, 0, 1)), ("42.5", "-3", "Grüße", "Header"));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n02_formula_live() {
    let (back, t) = edit("00-empty.numbers", "N02-formula.numbers", |wb, h, t| {
        wb.doc.set_cell(h, t, 1, 1, &CellInput::Number(2.0)).unwrap();
        wb.doc.set_cell(h, t, 2, 1, &CellInput::Number(3.0)).unwrap();
        wb.doc.set_formula_text(h, t, 3, 1, "=B2+B3").unwrap();
        wb.doc.set_formula_text(h, t, 4, 1, "=SUM(B2:B4)").unwrap();
    });
    assert_eq!((cell(&t, 3, 1), cell(&t, 4, 1)), ("5", "10"));
    let s = back.sheets().remove(0);
    let id = back.tables(&s)[0].info;
    assert_eq!(back.doc.formula_text(id, 4, 1).as_deref(), Some("=SUM(B2:B4)"));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n03_formula_existing_recalc() {
    // 07: B2=2, B3=3, B4 = B2+B3 made by Numbers; change B2 and the cached result follows
    let (_, t) = edit("07-formula-same-table.numbers", "N03-recalc.numbers", |wb, h, t| {
        wb.doc.set_cell(h, t, 1, 1, &CellInput::Number(10.0)).unwrap();
    });
    assert_eq!(cell(&t, 3, 1), "13");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n04_insert_delete_rows_cols() {
    let (_, t) = edit("29-base-data.numbers", "N04-rows-cols.numbers", |wb, h, t| {
        wb.doc.table_insert(h, t, Axis::Row, 2, 1).unwrap();
        wb.doc.table_insert(h, t, Axis::Column, 1, 1).unwrap();
        wb.doc.table_delete(h, t, Axis::Row, 5).unwrap();
    });
    assert!(t.rows > 0);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n05_merge() {
    let (_, t) = edit("29-base-data.numbers", "N05-merge.numbers", |wb, h, t| {
        wb.doc.merge_cells(h, t, CellRange { row: 1, col: 0, rows: 2, cols: 2 }).unwrap();
    });
    assert_eq!(t.merges.len(), 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n06_sort() {
    let (_, t) = edit("29-base-data.numbers", "N06-sort-desc.numbers", |wb, h, t| {
        wb.doc.sort_table_rows(h, t, 2, false).unwrap();
    });
    let _ = t;
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n07_fill_and_format() {
    let (_, t) = edit("29-base-data.numbers", "N07-fill-format.numbers", |wb, h, t| {
        let red = Color { r: 0.9, g: 0.2, b: 0.2, a: 1.0 };
        wb.doc.set_cells_fill(h, t, (1, 0, 2, 2), Some(&nm_core::kn_model::format::FillSpec::Color(red))).unwrap();
        let cur = nm_core::kn_model::tablefmt::CellFormat::Currency { code: "USD".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false };
        wb.doc.set_cells_format(h, t, (1, 2, 6, 2), &cur).unwrap();
    });
    assert!(cell(&t, 1, 2).starts_with('$'), "{}", cell(&t, 1, 2));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n08_headers() {
    let (_, t) = edit("00-empty.numbers", "N08-headers.numbers", |wb, h, t| {
        wb.doc.set_table_header_count(h, t, HeaderKind::Rows, 2).unwrap();
        wb.doc.set_table_header_count(h, t, HeaderKind::Footer, 1).unwrap();
    });
    assert_eq!((t.header_rows, t.footer_rows), (2, 1));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n09_highlight() {
    let (_, _t) = edit("29-base-data.numbers", "N09-highlight.numbers", |wb, h, t| {
        let look = &nm_core::kn_model::tablecond::LOOKS[0];
        wb.doc.add_highlight_rule(h, t, (1, 2, 6, 2), nm_core::kn_model::tablecond::Cmp::Greater, 20.0, look).unwrap();
    });
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n10_autofill() {
    let (_, t) = edit("29-base-data.numbers", "N10-autofill.numbers", |wb, h, t| {
        wb.doc.autofill(h, t, (1, 2, 2, 2), (1, 2, 4, 2)).unwrap();
    });
    let _ = t;
}

fn reopen(wb: &mut Workbook, name: &str) -> Workbook {
    wb.doc.save_as(out(name)).unwrap();
    Workbook::open(out(name)).unwrap()
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n11_sheets_rename_move_delete() {
    let mut wb = Workbook::open(testdata("11-formula-cross-sheet.numbers")).unwrap();
    let mut h = History::default();
    let ids = nm_core::sheets::sheet_ids(&wb.doc);
    wb.doc.begin(&mut h, "x");
    nm_core::sheets::rename_sheet(&mut wb.doc, &mut h, ids[1], "Renamed");
    assert!(nm_core::sheets::move_sheet(&mut wb.doc, &mut h, 1, 0));
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N11-sheet-rename-move.numbers");
    let names: Vec<String> = back.sheets().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Renamed", "Sheet 1"]);

    let mut wb = Workbook::open(testdata("01-second-sheet.numbers")).unwrap();
    wb.doc.begin(&mut h, "x");
    assert!(nm_core::sheets::delete_sheet(&mut wb.doc, &mut h, 1));
    assert!(!nm_core::sheets::delete_sheet(&mut wb.doc, &mut h, 0), "last sheet stays");
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N12-sheet-delete.numbers");
    assert_eq!(back.sheets().len(), 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n13_detach_table() {
    let mut wb = Workbook::open(testdata("02-second-table.numbers")).unwrap();
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t2 = wb.tables(&s)[1].info;
    wb.doc.begin(&mut h, "x");
    nm_core::sheets::detach_drawables(&mut wb.doc, &mut h, s.id, &[t2]);
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N13-table-delete.numbers");
    let s = back.sheets().remove(0);
    assert_eq!(back.tables(&s).len(), 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n14_add_empty_sheet() {
    let mut wb = Workbook::open(testdata("29-base-data.numbers")).unwrap();
    let mut h = History::default();
    let s0 = wb.sheets()[0].id;
    wb.doc.begin(&mut h, "x");
    nm_core::sheets::add_sheet(&mut wb.doc, &mut h, s0, "Sheet 2", 1).unwrap();
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N14-add-empty-sheet.numbers");
    let sheets = back.sheets();
    assert_eq!(sheets.len(), 2);
    assert_eq!(sheets[1].name, "Sheet 2");
    assert!(sheets[1].drawables.is_empty());
    assert_eq!(back.tables(&sheets[0]).len(), 1);
    // undo removes it again
    wb.doc.undo(&mut h);
    assert_eq!(wb.sheets().len(), 1);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n15_insert_table_and_sheet_with_table() {
    let mut wb = Workbook::open(testdata("29-base-data.numbers")).unwrap();
    let mut h = History::default();
    let s0 = wb.sheets()[0].id;
    let (donor, info) = nm_core::sheets::blank_donor().unwrap();
    wb.doc.begin(&mut h, "x");
    let t = nm_core::sheets::insert_table(&mut wb.doc, &mut h, s0, &donor, info, 0.0, 300.0).unwrap();
    wb.doc.set_cell(&mut h, t, 1, 1, &CellInput::Number(7.0)).unwrap();
    let s1 = nm_core::sheets::add_sheet(&mut wb.doc, &mut h, s0, "Sheet 2", 1).unwrap();
    let t2 = nm_core::sheets::insert_table(&mut wb.doc, &mut h, s1, &donor, info, 0.0, 0.0).unwrap();
    wb.doc.set_formula_text(&mut h, t2, 1, 1, "=1+2").unwrap();
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N15-insert-table-and-sheet.numbers");
    let sheets = back.sheets();
    assert_eq!(sheets.len(), 2);
    let tabs = back.tables(&sheets[0]);
    assert_eq!(tabs.len(), 2);
    assert_ne!(tabs[0].name, tabs[1].name, "new table needs its own name");
    let td = back.table_data(tabs[1].info).unwrap();
    assert_eq!(td.cells[(td.cols + 1) as usize], "7");
    let td2 = back.table_data(back.tables(&sheets[1])[0].info).unwrap();
    assert_eq!(td2.cells[(td2.cols + 1) as usize], "3");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n17_category_value_edit() {
    // Pen (row 1) moves from Office to Furniture by a plain edit of its Group cell
    let (back, _) = edit("30-category.numbers", "N17-category-edit.numbers", |wb, h, t| {
        wb.doc.set_cell(h, t, 1, 1, &CellInput::Text("Furniture".into())).unwrap();
    });
    let s = back.sheets().remove(0);
    let t = back.tables(&s)[0].info;
    assert!(nm_core::categories::groups(back.file(), t).is_some());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n19_table_style() {
    let (back, t) = edit("29-base-data.numbers", "N19-table-style-blue.numbers", |wb, h, t| {
        let presets = nm_core::styles::table_style_presets(&wb.doc);
        assert!(wb.doc.apply_table_style_preset(h, t, presets[1].0));
    });
    assert_eq!(t.cells[0], "Item");
    let _ = back;
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n20_borders() {
    let (_, t) = edit("29-base-data.numbers", "N20-border-outside.numbers", |wb, h, t| {
        let spec = nm_core::kn_model::format::StrokeSpec { color: Color { r: 0.8, g: 0.1, b: 0.1, a: 1.0 }, width: 2.0, dash: nm_core::kn_model::format::Dash::Solid };
        wb.doc.set_cells_border(h, t, (0, 0, 6, 2), nm_core::kn_model::tablefmt::BorderEdges::Outside, Some(&spec)).unwrap();
    });
    let top = t.h_strokes[0].as_ref().expect("top edge stroked");
    assert!((top.width - 2.0).abs() < 0.01);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n22_password() {
    let mut wb = Workbook::open(testdata("29-base-data.numbers")).unwrap();
    wb.doc.password = Some(("numdeck".into(), "the app".into()));
    wb.doc.save_as(out("N22-password-numdeck.numbers")).unwrap();
    match Workbook::open(out("N22-password-numdeck.numbers")) {
        Err(nm_core::kn_format::Error::NeedsPassword(hint)) => assert_eq!(hint, "the app"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("opened without a password"),
    }
    let doc = nm_core::kn_model::Document::open_with_password(out("N22-password-numdeck.numbers"), "numdeck", "the app").unwrap();
    let back = Workbook { doc };
    let s = back.sheets().remove(0);
    let td = back.table_data(back.tables(&s)[0].info).unwrap();
    assert_eq!(td.cells[td.cols as usize], "Pen");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n23_text_box() {
    let (back, _) = edit("29-base-data.numbers", "N23-text-box.numbers", |wb, h, _| {
        let s = wb.sheets()[0].id;
        let (donor, id) = nm_core::sheets::text_box_donor().unwrap();
        let t = nm_core::sheets::insert_object(&mut wb.doc, h, s, &donor, id, 520.0, 40.0).unwrap();
        nm_core::sheets::set_text_box_path(&mut wb.doc, h, t);
        let els: Vec<String> = wb.doc.file.obj(t).unwrap().find_super("TSD.ShapeArchive").unwrap().msg("pathsource").unwrap().msg("bezier_path_source").unwrap().msg("path").unwrap().msgs("elements").map(|e| e.enum_name("type").unwrap_or("?").to_string()).collect();
        assert_eq!(els, ["moveTo", "lineTo", "lineTo", "lineTo", "closeSubpath", "moveTo"]);
        let st = wb.doc.storage_of(t).unwrap();
        wb.doc.set_storage_text(h, st, "Hello from Linux");
    });
    let s = back.sheets().remove(0);
    assert_eq!(s.drawables.len(), 2);
    let t = s.drawables[1];
    assert_eq!(back.doc.storage_text(back.doc.storage_of(t).unwrap()), "Hello from Linux");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n24_duplicate_sheet() {
    let mut wb = Workbook::open(testdata("10-formula-cross-table.numbers")).unwrap();
    let mut h = History::default();
    wb.doc.begin(&mut h, "x");
    nm_core::sheets::duplicate_sheet(&mut wb.doc, &mut h, 0).unwrap();
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N24-duplicate-sheet.numbers");
    let sheets = back.sheets();
    assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Sheet 1", "Sheet 1 copy"]);
    let a = back.tables(&sheets[0]);
    let b = back.tables(&sheets[1]);
    assert_eq!(a.len(), b.len());
    let ta = back.table_data(a[1].info).unwrap();
    let tb = back.table_data(b[1].info).unwrap();
    assert_eq!(ta.cells, tb.cells);
    // the copy's =T2::B2×2 names the COPIED T2 (Numbers' Duplicate, N9): it follows the copy only
    assert_eq!(nm_core::formula::formula_text(back.file(), b[0].info, 1, 1).as_deref(), Some("=T2::B2×2"));
    // ... and its engine edge too: no record of the copies names an original table's owner (Mac N13)
    {
        use nm_core::kn_format::tables::{family, uuid_of_cf, uuid_of_str};
        let k = back.file();
        let fam = |info| k.obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)).and_then(|m| m.str("table_id")).and_then(uuid_of_str).and_then(|u| family(&u));
        let (orig, copies): (Vec<_>, Vec<_>) = (a.iter().map(|t| fam(t.info)).collect(), b.iter().map(|t| fam(t.info)).collect());
        let ce = k.first_of("TSCE.CalculationEngineArchive").unwrap().1;
        let owners: Vec<(u64, Option<[u8; 12]>)> = ce.msg("dependency_tracker").unwrap().msg("owner_id_map").unwrap().msgs("map_entry").map(|e| (e.u64("internal_owner_id").unwrap(), family(&uuid_of_cf(e.msg("owner_id").unwrap())))).collect();
        let of = |fams: &[Option<[u8; 12]>]| owners.iter().filter(|(_, f)| fams.contains(f)).map(|(i, _)| *i).collect::<Vec<u64>>();
        let (orig_ids, copy_ids) = (of(&orig), of(&copies));
        let mut edges = 0;
        for (_, t) in k.objects_of("TSCE.CellRecordTileArchive") {
            if !t.u64("internal_owner_id").is_some_and(|i| copy_ids.contains(&i)) {
                continue;
            }
            for r in t.msgs("cell_records") {
                for e in r.msg("expanded_edges").map(|e| e.u64s("internal_owner_id_for_edge")).unwrap_or_default() {
                    edges += 1;
                    assert!(!orig_ids.contains(&e), "copy's edge names original owner {e}");
                }
            }
        }
        assert!(edges > 0);
    }
    let mut wb = back;
    wb.doc.begin(&mut h, "z");
    wb.doc.set_cell(&mut h, b[1].info, 1, 1, &CellInput::Number(100.0)).unwrap();
    nm_core::formula::recalc_cross(&mut wb.doc, &mut h);
    wb.doc.end(&mut h);
    let copy = wb.table_data(b[0].info).unwrap();
    let orig = wb.table_data(a[0].info).unwrap();
    assert_eq!(copy.cells[(copy.cols + 1) as usize], "200");
    assert_eq!(orig.cells[(orig.cols + 1) as usize], "14");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n27_big_tables() {
    let name = |td: &nm_core::kn_model::TableData, r: u32| td.cells[(r * td.cols) as usize].clone();
    // Numbers' own 356-row table (two cell tiles): a cell in the second tile
    let mut wb = Workbook::open(testdata("N-functions.numbers")).unwrap();
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    wb.doc.begin(&mut h, "x");
    wb.doc.set_cell(&mut h, t, 300, 2, &CellInput::Text("far".into())).unwrap();
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N27-numbers-tile-2-cell.numbers");
    let td = back.table_data(t).unwrap();
    assert_eq!(td.cells[(300 * td.cols + 2) as usize], "far");
    assert_eq!(name(&td, 300), name(&wb.table_data(t).unwrap(), 300));

    // a 300-row table made here: rows inserted / deleted across the tile boundary
    let rows: Vec<Vec<String>> = (0..300).map(|r| vec![format!("r{r}"), r.to_string()]).collect();
    let mut wb = nm_core::import::workbook_from_rows(&rows, "Big").unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    wb.doc.begin(&mut h, "y");
    wb.doc.table_insert(&mut h, t, Axis::Row, 100, 99).unwrap();
    wb.doc.set_cell(&mut h, t, 100, 0, &CellInput::Text("new".into())).unwrap();
    wb.doc.table_delete(&mut h, t, Axis::Row, 270).unwrap();
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N28-big-table-300-rows.numbers");
    let td = back.table_data(back.tables(&back.sheets()[0])[0].info).unwrap();
    assert_eq!(td.rows, 300);
    assert_eq!((name(&td, 99), name(&td, 100), name(&td, 101)), ("r99".into(), "new".into(), "r100".into()));
    assert_eq!((name(&td, 256), name(&td, 257), name(&td, 269), name(&td, 270)), ("r255".into(), "r256".into(), "r268".into(), "r270".into()));
    assert_eq!(name(&td, 299), "r299");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n29_style_tables_and_rectangle() {
    let (back, _) = edit("29-base-data.numbers", "N29-styles-and-shape.numbers", |wb, h, _| {
        let s = wb.sheets()[0].id;
        for i in 0..6 {
            let (d, id) = nm_core::sheets::table_style_donor(i).unwrap();
            nm_core::sheets::insert_table(&mut wb.doc, h, s, &d, id, (i % 3) as f32 * 560.0, 500.0 + (i / 3) as f32 * 260.0).unwrap();
        }
        let (d, id) = nm_core::sheets::object_donor("shape").unwrap();
        nm_core::sheets::insert_object(&mut wb.doc, h, s, &d, id, 600.0, 40.0).unwrap();
    });
    let s = back.sheets().remove(0);
    assert_eq!(back.tables(&s).len(), 7);
    assert_eq!(s.drawables.len(), 8);
    let s2 = nm_core::sheets::new_sheet_table_donor().unwrap();
    assert!(s2.0.obj(s2.1).is_some());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n30_image() {
    // a 3 x 2 PNG made here
    let png: Vec<u8> = {
        let mut v = Vec::new();
        let img = image_png();
        v.extend(img);
        v
    };
    let (back, _) = edit("29-base-data.numbers", "N30-image.numbers", |wb, h, _| {
        let s = wb.sheets()[0].id;
        nm_core::sheets::insert_image(&mut wb.doc, h, s, png.clone(), "dot.png", 600.0, 100.0).unwrap();
    });
    let s = back.sheets().remove(0);
    let img = *s.drawables.last().unwrap();
    assert_eq!(back.file().type_name(img), "TSD.ImageArchive");
    let data = back.file().obj(img).unwrap().ref_id("data");
    assert!(data.is_some());
}

/// A tiny valid PNG (3 x 2, RGBA), written without an image crate.
fn image_png() -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    let (w, h) = (3u32, 2u32);
    let mut raw = Vec::new();
    for _ in 0..h {
        raw.push(0u8);
        for _ in 0..w {
            raw.extend([0x40, 0xa0, 0x40, 0xff]);
        }
    }
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend((raw.len() as u16).to_le_bytes());
    z.extend((!(raw.len() as u16)).to_le_bytes());
    z.extend(&raw);
    z.extend(adler(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    for (ty, data) in [(b"IHDR".as_slice(), [w.to_be_bytes().as_slice(), h.to_be_bytes().as_slice(), &[8, 6, 0, 0, 0]].concat()), (b"IDAT", z), (b"IEND", Vec::new())] {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend(&data);
        out.extend(&c);
        out.extend(crc(&c).to_be_bytes());
    }
    out
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n31_delete_cleans_engine() {
    let count = |wb: &Workbook| {
        let (_, ce) = wb.file().first_of("TSCE.CalculationEngineArchive").unwrap();
        let t = ce.msg("dependency_tracker").unwrap();
        (t.ref_ids("formula_owner_dependencies").len(), t.u64("number_of_formulas"))
    };
    // Numbers' own table delete of 02-second-table: 24 -> 14 owners, 5 formulas (Mac N3 reference)
    let mut wb = Workbook::open(testdata("02-second-table.numbers")).unwrap();
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t2 = wb.tables(&s)[1].info;
    wb.doc.begin(&mut h, "x");
    nm_core::sheets::detach_drawables(&mut wb.doc, &mut h, s.id, &[t2]);
    wb.doc.end(&mut h);
    assert_eq!(count(&wb), (14, Some(5)));
    // the table's kind-1 owner stays, emptied as in Numbers' own delete (N11)
    let kind1 = |wb: &Workbook| -> Vec<(bool, usize)> {
        let (_, ce) = wb.file().first_of("TSCE.CalculationEngineArchive").unwrap();
        ce.msg("dependency_tracker").unwrap().ref_ids("formula_owner_dependencies").into_iter().filter_map(|f| wb.file().obj(f)).filter(|m| m.u64("owner_kind") == Some(1)).map(|m| (m.has("formula_owner"), m.msg("tiled_cell_dependencies").map(|t| t.count("cell_record_tiles")).unwrap_or(0))).collect()
    };
    let reference = Workbook::open(format!("{}/CascadeProjects/numbers-research-src/out-N3/N13-reference-Numbers-own-table-delete.numbers", std::env::var("HOME").unwrap()));
    if let Ok(r) = reference {
        let mut a = kind1(&wb);
        let mut b = kind1(&r);
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    // the tracking owner (kind 7) keeps no tile the delete emptied (Numbers drops them, Mac N13;
    // Numbers' own files do keep some empty tiles of other owners)
    let empty_tiles = |wb: &Workbook| -> usize {
        let k = wb.file();
        let (_, ce) = k.first_of("TSCE.CalculationEngineArchive").unwrap();
        let fods = ce.msg("dependency_tracker").unwrap().ref_ids("formula_owner_dependencies");
        fods.into_iter().filter_map(|f| k.obj(f)).filter(|m| m.u64("owner_kind") == Some(7)).flat_map(|m| m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default()).filter(|t| k.obj(*t).is_some_and(|m| m.count("cell_records") == 0)).count()
    };
    assert_eq!(empty_tiles(&wb), 0);
    let back = reopen(&mut wb, "N31-table-delete-clean.numbers");
    assert_eq!(count(&back), (14, Some(5)));
    wb.doc.undo(&mut h);
    assert_eq!(count(&wb), (24, Some(24)));
    // sheet delete (01-second-sheet)
    let mut wb = Workbook::open(testdata("01-second-sheet.numbers")).unwrap();
    wb.doc.begin(&mut h, "y");
    nm_core::sheets::delete_sheet(&mut wb.doc, &mut h, 1);
    wb.doc.end(&mut h);
    let back = reopen(&mut wb, "N32-sheet-delete-clean.numbers");
    let (_, n) = count(&back);
    assert_eq!(n, Some(5));
    assert_eq!(empty_tiles(&back), 0);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n33_charts() {
    let (back, _) = edit("29-base-data.numbers", "N33-charts.numbers", |wb, h, t| {
        let s = wb.sheets()[0].id;
        let td = wb.table_data(t).unwrap();
        // A1:C7: names, groups, amounts -> one series "Amount" over six items
        let grid = nm_core::charts::grid_from_cells(&td, (0, 0, 6, 2));
        assert_eq!(grid.col_names.len(), 6);
        for k in 0..nm_core::charts::CHART_TYPES.len() {
            let g = if k == 0 { nm_core::charts::grid_from_cells(&td, (0, 0, 6, 2)) } else { grid.clone() };
            nm_core::charts::insert_chart(&mut wb.doc, h, s, k, &g, (k % 3) as f32 * 420.0, 500.0 + (k / 3) as f32 * 300.0).unwrap();
        }
    });
    let s = back.sheets().remove(0);
    let charts: Vec<_> = s.drawables.iter().filter(|d| back.file().type_name(**d) == "TSCH.ChartDrawableArchive").collect();
    assert_eq!(charts.len(), nm_core::charts::CHART_TYPES.len());
    let g = back.doc.chart_grid(*charts[0]).unwrap();
    assert_eq!(g.col_names[1], "Desk");
    assert_eq!(g.values[0][1], 120.0);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n34_shapes() {
    // the Shape popover's shapes from Numbers' own UI-made donors (Mac N13), in a row below the table
    let (back, _) = edit("29-base-data.numbers", "N34-shapes.numbers", |wb, h, _| {
        let s = wb.sheets()[0].id;
        for i in 0..nm_core::sheets::SHAPES.len() {
            let (d, id) = nm_core::sheets::shape_donor(i).unwrap();
            nm_core::sheets::insert_object(&mut wb.doc, h, s, &d, id, 40.0 + i as f32 * 160.0, 820.0).unwrap();
        }
    });
    let s = back.sheets().remove(0);
    assert_eq!(s.drawables.len(), 1 + nm_core::sheets::SHAPES.len());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n35_duplicate_and_reorder_objects() {
    // Edit > Duplicate Selection on a table, a text box and a shape (20 pt down-right), then
    // Arrange > Send to Back on the copied shape
    let (back, _) = edit("29-base-data.numbers", "N35-duplicate-reorder.numbers", |wb, h, t| {
        let s = wb.sheets()[0].id;
        let (d, id) = nm_core::sheets::text_box_donor().unwrap();
        let tb = nm_core::sheets::insert_object(&mut wb.doc, h, s, &d, id, 600.0, 40.0).unwrap();
        let (d, id) = nm_core::sheets::shape_donor(1).unwrap();
        let sh = nm_core::sheets::insert_object(&mut wb.doc, h, s, &d, id, 600.0, 200.0).unwrap();
        let clip = nm_core::sheets::copy_objects(&wb.doc, &[t, tb, sh]).unwrap();
        let new = nm_core::sheets::paste_objects(&mut wb.doc, h, s, &clip, 20.0).unwrap();
        assert_eq!(new.len(), 3);
        nm_core::sheets::reorder(&mut wb.doc, h, s, &[*new.last().unwrap()], nm_core::kn_model::edit::ZMove::Back);
    });
    let s = back.sheets().remove(0);
    assert_eq!(s.drawables.len(), 6);
    assert_eq!(back.tables(&s).len(), 2);
    // the copied shape is first (at the back)
    assert_eq!(back.file().type_name(s.drawables[0]), "TSWP.ShapeInfoArchive");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n36_hide_rows_and_columns() {
    // Table > Hide Row / Hide Column: by uid with user_hidden in the table's hidden states
    let (back, _) = edit("29-base-data.numbers", "N36-hide-row-column.numbers", |wb, h, t| {
        nm_core::set_hidden(&mut wb.doc, h, t, true, &[2, 4], true).unwrap();
        nm_core::set_hidden(&mut wb.doc, h, t, false, &[1], true).unwrap();
    });
    let s = back.sheets().remove(0);
    let t = back.tables(&s)[0].info;
    assert_eq!(nm_core::hidden(back.file(), t), (vec![2, 4], vec![1]));
    let mut wb = back;
    let mut h = History::default();
    wb.doc.begin(&mut h, "x");
    nm_core::set_hidden(&mut wb.doc, &mut h, t, true, &[2, 4], false).unwrap();
    nm_core::set_hidden(&mut wb.doc, &mut h, t, false, &[1], false).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(nm_core::hidden(wb.file(), t), (vec![], vec![]));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n37_rows_and_columns_around_merges() {
    use nm_core::kn_model::tablerows::Axis;
    use nm_core::kn_model::CellRange;
    let merges = |wb: &Workbook| {
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let mut m = wb.table_data(t).unwrap().merges;
        m.sort_by_key(|r| (r.row, r.col));
        m
    };
    let mut wb = Workbook::open(testdata("12-merged-cells.numbers")).unwrap();
    let mut h = History::default();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    assert_eq!(merges(&wb), vec![CellRange { row: 1, col: 1, rows: 2, cols: 2 }]);
    // a row inside the merge: it grows
    wb.doc.begin(&mut h, "a");
    wb.doc.table_insert(&mut h, t, Axis::Row, 2, 1).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(merges(&wb), vec![CellRange { row: 1, col: 1, rows: 3, cols: 2 }]);
    // a row above it: it moves down
    wb.doc.begin(&mut h, "b");
    wb.doc.table_insert(&mut h, t, Axis::Row, 0, 0).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(merges(&wb), vec![CellRange { row: 2, col: 1, rows: 3, cols: 2 }]);
    // its second column deleted: one column wide, still merged (3 rows)
    wb.doc.begin(&mut h, "c");
    wb.doc.table_delete(&mut h, t, Axis::Column, 2).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(merges(&wb), vec![CellRange { row: 2, col: 1, rows: 3, cols: 1 }]);
    let back = reopen(&mut wb, "N37-merge-rows-columns.numbers");
    assert_eq!(merges(&back), vec![CellRange { row: 2, col: 1, rows: 3, cols: 1 }]);
    // undo all: the merge as it was
    for _ in 0..3 {
        wb.doc.undo(&mut h);
    }
    assert_eq!(merges(&wb), vec![CellRange { row: 1, col: 1, rows: 2, cols: 2 }]);
    // its only column deleted: no merge left
    wb.doc.begin(&mut h, "d");
    wb.doc.table_delete(&mut h, t, Axis::Column, 1).unwrap();
    wb.doc.table_delete(&mut h, t, Axis::Column, 1).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(merges(&wb), vec![]);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n38_merges_match_numbers_n19() {
    // rows / columns added and removed around merged cells, against Numbers' own files (Mac N19)
    use nm_core::kn_model::tablerows::Axis;
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/research/out-N19/files");
    if !dir.join("N19-base.numbers").exists() {
        return;
    }
    let grid = |wb: &Workbook| {
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let d = wb.table_data(t).unwrap();
        let mut m = d.merges.clone();
        m.sort_by_key(|r| (r.row, r.col));
        (d.rows, d.cols, d.cells, m)
    };
    let cases: &[(&str, &[(Axis, bool, u32)])] = &[
        ("N19-row-above-2", &[(Axis::Row, true, 1)]),
        ("N19-row-below-3", &[(Axis::Row, true, 3)]),
        ("N19-row-above-4", &[(Axis::Row, true, 3)]),
        ("N19-delete-row-2", &[(Axis::Row, false, 1)]),
        ("N19-delete-rows-1-2", &[(Axis::Row, false, 0), (Axis::Row, false, 0)]),
        ("N19-col-before-B", &[(Axis::Column, true, 1)]),
        ("N19-delete-col-C", &[(Axis::Column, false, 2)]),
        ("N19-delete-col-B", &[(Axis::Column, false, 1)]),
        ("N19-extra-1x1-leftover", &[(Axis::Row, false, 1), (Axis::Column, false, 1)]),
    ];
    for (name, ops) in cases {
        let mut wb = Workbook::open(dir.join("N19-base.numbers")).unwrap();
        let mut h = History::default();
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        wb.doc.begin(&mut h, "x");
        for &(axis, ins, i) in ops.iter() {
            if ins {
                wb.doc.table_insert(&mut h, t, axis, i, i.saturating_sub(1)).unwrap();
            } else {
                wb.doc.table_delete(&mut h, t, axis, i).unwrap();
            }
        }
        wb.doc.end(&mut h);
        let back = reopen(&mut wb, &format!("N38-{name}.numbers"));
        let want = Workbook::open(dir.join(format!("{name}.numbers"))).unwrap();
        assert_eq!(grid(&back), grid(&want), "{name}");
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n39_preset_shapes_and_arrow_lines() {
    // Insert > Shape > Star etc.: a Numbers rectangle given a preset outline; undo restores it
    use nm_core::kn_model::insert::BasicShape;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/Blank.numbers");
    let mut wb = Workbook::open(&path).unwrap();
    let sheet = wb.sheets()[0].id;
    let (k, info) = nm_core::sheets::shape_donor(3).unwrap();
    let mut h = History::default();
    wb.doc.begin(&mut h, "Shape");
    let id = nm_core::sheets::insert_object(&mut wb.doc, &mut h, sheet, &k, info, 100.0, 100.0).unwrap();
    wb.doc.end(&mut h);
    let kind = |wb: &Workbook| wb.file().obj(id).and_then(|m| m.find_super("TSD.ShapeArchive")).and_then(|s| s.msg("pathsource")).map(|p| format!("{p:?}").len());
    let before = kind(&wb);
    wb.doc.begin(&mut h, "Star");
    assert!(nm_core::sheets::set_basic_shape(&mut wb.doc, &mut h, id, BasicShape::Star));
    wb.doc.end(&mut h);
    assert_ne!(kind(&wb), before, "the outline changed");
    // the frame is kept
    let g = |wb: &Workbook| wb.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry).map(|g| (g.x, g.y, g.w, g.h));
    let frame = g(&wb);
    assert_eq!(frame.map(|f| (f.0, f.1)), Some((100.0, 100.0)));
    let out = std::env::temp_dir().join("n39.numbers");
    wb.doc.save_as(&out).unwrap();
    let again = Workbook::open(&out).unwrap();
    assert_eq!(kind(&again), kind(&wb));
    assert!(wb.doc.undo(&mut h));
    assert_eq!(kind(&wb), before, "undo puts the rectangle back");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n40_excel_export_round_trip() {
    // File > Export To > Excel: values come back through our own reader; merges and names kept
    use nm_core::kn_model::CellRange;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/Blank.numbers");
    let mut wb = Workbook::open(&path).unwrap();
    let s = wb.sheets().remove(0);
    let t = wb.tables(&s)[0].info;
    let mut h = History::default();
    wb.doc.begin(&mut h, "Cells");
    wb.doc.set_cell(&mut h, t, 1, 1, &CellInput::Number(42.5)).unwrap();
    wb.doc.set_cell(&mut h, t, 2, 1, &CellInput::Text("Fish & <chips>".into())).unwrap();
    wb.doc.set_cell(&mut h, t, 3, 1, &CellInput::Text("two\nlines".into())).unwrap();
    wb.doc.merge_cells(&mut h, t, CellRange { row: 4, col: 0, rows: 1, cols: 2 }).unwrap();
    wb.doc.end(&mut h);
    let out = std::env::temp_dir().join("n40.xlsx");
    assert_eq!(nm_core::xlsx::write_xlsx(&wb, &out).unwrap(), 1);
    let bytes = std::fs::read(&out).unwrap();
    let sheets = nm_core::xlsx::read_xlsx(&bytes).unwrap();
    assert_eq!(sheets.len(), 1);
    assert_eq!(sheets[0].0, s.name);
    let text = format!("{:?}", sheets[0].1);
    assert!(text.contains("42.5"), "{text}");
    assert!(text.contains("Fish & <chips>"), "{text}");
    assert!(text.contains("two\\nlines"), "{text}");
    let z = String::from_utf8_lossy(&bytes);
    let _ = z;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
    let mut x = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut x).unwrap();
    assert!(x.contains(r#"<mergeCell ref="A5:B5"/>"#), "{x}");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n41_group_on_a_sheet_like_numbers_n22() {
    // Arrange > Group on a Numbers sheet: the group takes the shapes' place in drawable_infos,
    // its frame the union of theirs, the children relative to it with the group as parent
    // (Mac N22 G2); undo and ungroup put them back where they were
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/Blank.numbers");
    let mut wb = Workbook::open(&path).unwrap();
    let sheet = wb.sheets()[0].id;
    let (k, info) = nm_core::sheets::shape_donor(3).unwrap();
    let mut h = History::default();
    wb.doc.begin(&mut h, "Shapes");
    let a = nm_core::sheets::insert_object(&mut wb.doc, &mut h, sheet, &k, info, 100.0, 400.0).unwrap();
    let b = nm_core::sheets::insert_object(&mut wb.doc, &mut h, sheet, &k, info, 300.0, 450.0).unwrap();
    wb.doc.end(&mut h);
    let infos = |wb: &Workbook| wb.file().obj(sheet).unwrap().ref_ids("drawable_infos");
    let geom = |wb: &Workbook, id| wb.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry).map(|g| (g.x, g.y, g.w, g.h)).unwrap();
    let before = infos(&wb);
    wb.doc.begin(&mut h, "Group");
    let g = wb.doc.group(&mut h, sheet, &[a, b]).unwrap();
    wb.doc.end(&mut h);
    let after = infos(&wb);
    assert!(after.contains(&g) && !after.contains(&a) && !after.contains(&b), "{after:?}");
    assert_eq!(geom(&wb, g), (100.0, 400.0, 300.0, 150.0), "the union of the frames (N22: 100,400 300x150)");
    assert_eq!(geom(&wb, a), (0.0, 0.0, 100.0, 100.0));
    assert_eq!(geom(&wb, b), (200.0, 50.0, 100.0, 100.0));
    let parent = |wb: &Workbook, id| wb.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.ref_id("parent"));
    assert_eq!(parent(&wb, a), Some(g));
    assert_eq!(parent(&wb, g), Some(sheet));
    // saved and read back
    let out = std::env::temp_dir().join("n41.numbers");
    wb.doc.save_as(&out).unwrap();
    let again = Workbook::open(&out).unwrap();
    assert!(again.file().obj(sheet).unwrap().ref_ids("drawable_infos").contains(&g));
    // undo: the shapes are back on the sheet
    assert!(wb.doc.undo(&mut h));
    assert_eq!(infos(&wb), before);
    assert_eq!(geom(&wb, b), (300.0, 450.0, 100.0, 100.0));
    // group again, then ungroup
    assert!(wb.doc.redo(&mut h));
    wb.doc.begin(&mut h, "Ungroup");
    let kids = wb.doc.ungroup(&mut h, sheet, g).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(kids, vec![a, b]);
    assert!(infos(&wb).contains(&a) && !infos(&wb).contains(&g));
    assert_eq!(geom(&wb, b), (300.0, 450.0, 100.0, 100.0));
    assert_eq!(parent(&wb, b), Some(sheet));
    wb.doc.save_as(std::env::temp_dir().join("n41-ungrouped.numbers")).unwrap();
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n42_tab_color_like_numbers_n23() {
    // Format > Sheet > Tab Color: a variation of the sheet's style with only tab_color, the
    // sheet pointing at it (Mac N23 A1: red = 0.9216, 0.2941, 0.2745); changing it again edits
    // the same variation; None clears it; undo puts the plain tab back
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/Blank.numbers");
    let mut wb = Workbook::open(&path).unwrap();
    let sheet = wb.sheets()[0].id;
    let base = wb.file().obj(sheet).unwrap().ref_id("style").unwrap();
    assert!(nm_core::sheets::tab_color(wb.file(), sheet).is_none());
    let red = Color { r: 0.92156863, g: 0.29411766, b: 0.27450982, a: 1.0 };
    let mut h = History::default();
    wb.doc.begin(&mut h, "Change Tab Color");
    nm_core::sheets::set_tab_color(&mut wb.doc, &mut h, sheet, Some(red)).unwrap();
    wb.doc.end(&mut h);
    let v = wb.file().obj(sheet).unwrap().ref_id("style").unwrap();
    assert_ne!(v, base);
    let vm = wb.file().obj(v).unwrap();
    let tss = vm.find_super("TSS.StyleArchive").unwrap();
    assert_eq!(tss.ref_id("parent"), Some(base));
    assert_eq!(tss.bool("is_variation"), Some(true));
    assert_eq!(vm.u64("override_count"), Some(1));
    assert!(vm.msg("sheet_properties").unwrap().msg("fill").is_none(), "only the tab colour is overridden");
    assert_eq!(nm_core::sheets::tab_color(wb.file(), sheet), Some(red));
    // a second change edits the variation in place
    let blue = Color { r: 0.2, g: 0.4, b: 0.9, a: 1.0 };
    wb.doc.begin(&mut h, "Change Tab Color");
    nm_core::sheets::set_tab_color(&mut wb.doc, &mut h, sheet, Some(blue)).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(wb.file().obj(sheet).unwrap().ref_id("style"), Some(v));
    assert_eq!(nm_core::sheets::tab_color(wb.file(), sheet), Some(blue));
    let mut r = reopen(&mut wb, "n42");
    let s2 = r.sheets()[0].id;
    assert_eq!(nm_core::sheets::tab_color(r.file(), s2), Some(blue));
    // none
    let mut h2 = History::default();
    r.doc.begin(&mut h2, "Change Tab Color");
    nm_core::sheets::set_tab_color(&mut r.doc, &mut h2, s2, None).unwrap();
    r.doc.end(&mut h2);
    assert!(nm_core::sheets::tab_color(r.file(), s2).is_none());
    // undo back to the plain style
    assert!(wb.doc.undo(&mut h));
    assert!(wb.doc.undo(&mut h));
    assert_eq!(wb.file().obj(sheet).unwrap().ref_id("style"), Some(base));
    assert!(wb.file().obj(v).is_none());
    assert!(nm_core::sheets::tab_color(wb.file(), sheet).is_none());
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn n43_date_cell_like_numbers() {
    // a date written by CellInput::Date has the record the Mac wrote for a typed date
    // (05-value-date B2: cell type 5, the date, format kind 3, the default date format)
    let mut wb = Workbook::open(testdata("05-value-date.numbers")).unwrap();
    let t = wb.tables(&wb.sheets()[0])[0].info;
    let raw = |wb: &Workbook, rc: (u32, u32)| {
        let model = wb.file().obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| wb.file().obj(m)).unwrap();
        nm_core::kn_model::table::raw_cell_records(wb.file(), model).into_iter().find(|(x, _)| *x == rc).map(|x| x.1)
    };
    let mac = raw(&wb, (1, 1)).unwrap();
    let Some(nm_core::kn_model::table::CellValue::Date(secs)) = wb.doc.cell_values(t).get(&(1, 1)).cloned() else { panic!("B2 is a date") };
    let mut h = History::default();
    wb.doc.begin(&mut h, "Date");
    wb.doc.set_cell(&mut h, t, 2, 1, &CellInput::Date(secs)).unwrap();
    wb.doc.end(&mut h);
    assert_eq!(raw(&wb, (2, 1)).unwrap(), mac, "the same bytes as the Mac's date cell");
    let r = reopen(&mut wb, "n43");
    let t2 = r.tables(&r.sheets()[0])[0].info;
    assert_eq!(r.doc.cell_values(t2).get(&(2, 1)), Some(&nm_core::kn_model::table::CellValue::Date(secs)));
}
