//! Random edit sequences on the research samples (seeded, repeatable): every step must succeed
//! or fail cleanly, and the result must save, reopen with the same sheets/tables and pass
//! key_validate.py (run by the shell script that calls this with NM_FUZZ_OUT).

use nm_core::kn_model::edit::History;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::kn_model::CellRange;
use nm_core::Workbook;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { (self.next() % n as u64) as u32 }
    }
}

fn dims(wb: &Workbook, t: nm_core::kn_format::ObjId) -> (u32, u32) {
    let td = wb.table_data(t).unwrap();
    (td.rows, td.cols)
}

#[test]
#[ignore]
fn fuzz_edits() {
    let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata"));
    let out = std::path::PathBuf::from(std::env::var("NM_FUZZ_OUT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/fuzz").into()));
    std::fs::create_dir_all(&out).unwrap();
    let seeds: u64 = std::env::var("NM_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let samples = ["00-empty", "07-formula-same-table", "12-merged-cells", "29-base-data", "30-category", "31-filter", "32-conditional-highlight", "02-second-table", "01-second-sheet"];
    let (donor, dinfo) = nm_core::sheets::blank_donor().unwrap();
    let mut failures = Vec::new();
    for seed in 1..=seeds {
        let name = samples[(seed as usize) % samples.len()];
        let mut rng = Rng(seed * 0x9E37_79B9_7F4A_7C15 + 1);
        let mut wb = Workbook::open(dir.join(format!("{name}.numbers"))).unwrap();
        let mut h = History::default();
        let mut log = Vec::new();
        for step in 0..40 {
            let sheets = wb.sheets();
            if sheets.is_empty() {
                failures.push(format!("seed {seed} {name}: no sheets after step {step}:\n{}", log.join("\n")));
                break;
            }
            let s = &sheets[rng.below(sheets.len() as u32) as usize];
            let tables = wb.tables(s);
            let op = rng.below(16);
            if tables.is_empty() && op < 13 {
                continue;
            }
            let t = tables.get(rng.below(tables.len() as u32) as usize).map(|t| t.info);
            let (rows, cols) = t.map(|t| dims(&wb, t)).unwrap_or((0, 0));
            let (r, c) = (rng.below(rows), rng.below(cols));
            wb.doc.begin(&mut h, "fuzz");
            let res: Result<(), String> = match (op, t) {
                (0..=3, Some(t)) => {
                    let v = match rng.below(4) {
                        0 => CellInput::Number(rng.below(1000) as f64 / 4.0),
                        1 => CellInput::Text(format!("t{}", rng.below(99))),
                        2 => CellInput::Empty,
                        _ => CellInput::Number(-(rng.below(50) as f64)),
                    };
                    log.push(format!("set {r},{c} {v:?}"));
                    wb.doc.set_cell(&mut h, t, r, c, &v).map_err(|e| e.to_string())
                }
                (4, Some(t)) if rows > 2 => {
                    let src = format!("=SUM(A1:{}{})+1", nm_core_col(c), rng.below(rows) + 1);
                    log.push(format!("formula {r},{c} {src}"));
                    wb.doc.set_formula_text(&mut h, t, r, c, &src).map_err(|e| e.to_string())
                }
                (5, Some(t)) => {
                    log.push(format!("insert row {r}"));
                    wb.doc.table_insert(&mut h, t, Axis::Row, r, r).map_err(|e| e.to_string())
                }
                (6, Some(t)) if rows > 3 => {
                    log.push(format!("delete row {r}"));
                    wb.doc.table_delete(&mut h, t, Axis::Row, r).map_err(|e| e.to_string())
                }
                (7, Some(t)) => {
                    log.push(format!("insert col {c}"));
                    wb.doc.table_insert(&mut h, t, Axis::Column, c, c).map_err(|e| e.to_string())
                }
                (8, Some(t)) if cols > 2 => {
                    log.push(format!("delete col {c}"));
                    wb.doc.table_delete(&mut h, t, Axis::Column, c).map_err(|e| e.to_string())
                }
                (9, Some(t)) if r + 1 < rows && c + 1 < cols => {
                    log.push(format!("merge {r},{c} 2x2"));
                    wb.doc.merge_cells(&mut h, t, CellRange { row: r, col: c, rows: 2, cols: 2 }).map_err(|e| e.to_string())
                }
                (10, Some(t)) => {
                    log.push(format!("sort col {c}"));
                    wb.doc.sort_table_rows(&mut h, t, c, rng.below(2) == 0).map_err(|e| e.to_string())
                }
                (11, Some(t)) => {
                    let n = rng.below(3);
                    log.push(format!("header rows {n}"));
                    wb.doc.set_table_header_count(&mut h, t, HeaderKind::Rows, n).map_err(|e| e.to_string())
                }
                (12, Some(t)) => {
                    let f = nm_core::kn_model::tablefmt::CellFormat::Percent { decimals: Some(1), thousands: false, negative: 0 };
                    log.push(format!("percent {r},{c}"));
                    wb.doc.set_cells_format(&mut h, t, (r, c, r, c), &f).map(|_| ()).map_err(|e| e.to_string())
                }
                (13, _) => {
                    log.push("add sheet".into());
                    nm_core::sheets::add_sheet(&mut wb.doc, &mut h, s.id, &format!("S{step}"), 0).map(|_| ()).map_err(|e| e.to_string())
                }
                (14, _) => {
                    log.push("insert table".into());
                    nm_core::sheets::insert_table(&mut wb.doc, &mut h, s.id, &donor, dinfo, (step % 4) as f32 * 120.0, 600.0 + step as f32 * 40.0).map(|_| ()).map_err(|e| e.to_string())
                }
                (15, _) if sheets.len() > 1 => {
                    log.push("delete sheet".into());
                    let i = rng.below(sheets.len() as u32) as usize;
                    nm_core::sheets::delete_sheet(&mut wb.doc, &mut h, i);
                    Ok(())
                }
                _ => Ok(()),
            };
            wb.doc.end(&mut h);
            if let Err(e) = res {
                log.push(format!("  -> error: {e}"));
            }
            // undo / redo now and then
            if rng.below(8) == 0 {
                wb.doc.undo(&mut h);
                log.push("undo".into());
                if rng.below(2) == 0 {
                    wb.doc.redo(&mut h);
                    log.push("redo".into());
                }
            }
        }
        let path = out.join(format!("fuzz-{seed:03}-{name}.numbers"));
        let saved = wb.doc.save_as(&path).map_err(|e| e.to_string()).and_then(|_| Workbook::open(&path).map_err(|e| e.to_string()));
        match saved {
            Ok(back) => {
                let a: Vec<(String, usize)> = wb.sheets().iter().map(|s| (s.name.clone(), wb.tables(s).len())).collect();
                let b: Vec<(String, usize)> = back.sheets().iter().map(|s| (s.name.clone(), back.tables(s).len())).collect();
                if a != b {
                    failures.push(format!("seed {seed} {name}: sheets differ after reopen {a:?} vs {b:?}"));
                }
                for s in back.sheets() {
                    for t in back.tables(&s) {
                        if back.table_data(t.info).is_none() {
                            failures.push(format!("seed {seed} {name}: table {} unreadable", t.name));
                        }
                    }
                }
            }
            Err(e) => failures.push(format!("seed {seed} {name}: save/reopen failed: {e}")),
        }
        std::fs::write(out.join(format!("fuzz-{seed:03}-{name}.log")), log.join("\n")).unwrap();
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn nm_core_col(c: u32) -> String {
    let mut c = c;
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}
