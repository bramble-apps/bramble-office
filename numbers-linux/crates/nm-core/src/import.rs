//! Opening CSV / TSV files: a new document (Blank template) whose table holds the file, the
//! first line as the header row, named after the file (as Numbers does).

use kn_model::edit::History;
use kn_model::tablecells::CellInput;
use kn_model::tablerows::{Axis, HeaderKind};
use kn_model::Document;

use crate::Workbook;

/// Split delimited text into rows of fields ("…" quoting with "" for a quote, as RFC 4180).
pub fn parse_delimited(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(ch),
            }
        } else if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch == sep {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(ch);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// One imported cell: what it shows, and its formula ("=…") if it has one. A formula Keydeck
/// can't parse keeps the shown value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field {
    pub shown: String,
    pub formula: Option<String>,
    /// A date (seconds since 2001-01-01) with its format (an ICU pattern): set as a date cell.
    pub date: Option<(f64, String)>,
}

impl Field {
    /// A CSV field: "=…" is a formula (its text shows if it can't be read).
    pub fn from_text(s: &str) -> Field {
        if s.trim_start().starts_with('=') && s.trim().len() > 1 {
            Field { shown: s.to_string(), formula: Some(s.trim().to_string()), date: None }
        } else {
            Field { shown: s.to_string(), formula: None, date: None }
        }
    }
}

/// Numbers' row limit.
pub const MAX_ROWS: usize = 1_000_000;

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What the last import had to leave out (cleared by the call).
pub fn take_warnings() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

/// Size table `info` to the fields (at least 2 × 1) and fill it; the first row is the header.
pub fn fill_table(doc: &mut Document, h: &mut History, info: kn_format::ObjId, rows: &[Vec<Field>], name: &str) -> Result<(), kn_format::Error> {
    if rows.len() > MAX_ROWS {
        WARNINGS.with(|w| w.borrow_mut().push(format!("{name}: only the first {MAX_ROWS} of {} rows were imported", rows.len())));
    }
    let rows = &rows[..rows.len().min(MAX_ROWS)];
    let want_rows = (rows.len() as u32).max(2);
    let want_cols = (rows.iter().map(Vec::len).max().unwrap_or(1) as u32).clamp(1, 255);
    doc.set_table_header_count(h, info, HeaderKind::Columns, 0)?;
    let size = |d: &Document| {
        let m = d.file.obj(info).and_then(|i| i.ref_id("tableModel")).and_then(|t| d.file.obj(t));
        (m.and_then(|m| m.u32("number_of_rows")).unwrap_or(0), m.and_then(|m| m.u32("number_of_columns")).unwrap_or(0))
    };
    let (mut r, mut c) = size(doc);
    while r < want_rows {
        doc.table_insert(h, info, Axis::Row, r, r - 1)?;
        r += 1;
    }
    while r > want_rows {
        doc.table_delete(h, info, Axis::Row, r - 1)?;
        r -= 1;
    }
    while c < want_cols {
        doc.table_insert(h, info, Axis::Column, c, c - 1)?;
        c += 1;
    }
    while c > want_cols {
        doc.table_delete(h, info, Axis::Column, c - 1)?;
        c -= 1;
    }
    // values first, then formulas (they read the values)
    let mut formulas = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        for (ci, f) in row.iter().enumerate().take(want_cols as usize) {
            if let Some((secs, pattern)) = &f.date {
                doc.set_cell(h, info, ri as u32, ci as u32, &CellInput::Date(*secs))?;
                let fmt = kn_model::tablefmt::CellFormat::DateTime { pattern: pattern.clone() };
                let _ = doc.set_cells_format(h, info, (ri as u32, ci as u32, ri as u32, ci as u32), &fmt);
                continue;
            }
            match &f.formula {
                Some(src) if ri > 0 => formulas.push((ri as u32, ci as u32, src, &f.shown)),
                _ if !f.shown.is_empty() => doc.set_cell(h, info, ri as u32, ci as u32, &CellInput::parse(&f.shown))?,
                _ => {}
            }
        }
    }
    for (r, c, src, shown) in formulas {
        if doc.set_formula_text(h, info, r, c, src).is_err() && !shown.is_empty() {
            doc.set_cell(h, info, r, c, &CellInput::parse(shown))?;
        }
    }
    if let Some(model) = doc.file.obj(info).and_then(|m| m.ref_id("tableModel")) {
        doc.touch(h, model);
        if let Some(m) = doc.file.obj_mut(model) {
            m.set_str("table_name", name);
        }
    }
    Ok(())
}

/// A new workbook with one sheet per (sheet name, table name, rows).
pub fn workbook_from_sheets(sheets: &[(String, String, Vec<Vec<Field>>)]) -> Result<Workbook, kn_format::Error> {
    crate::init();
    let k = kn_format::KeyFile::from_zip(crate::BLANK)?;
    let mut doc = Document::from_file(k);
    let mut h = History::default();
    let (donor, dinfo) = crate::sheets::blank_donor()?;
    doc.begin(&mut h, "Import");
    let first_sheet = crate::sheets::sheet_ids(&doc)[0];
    for (i, (sname, tname, rows)) in sheets.iter().enumerate() {
        let (sheet, info) = if i == 0 {
            let info = doc.file.first_of("TST.TableInfoArchive").map(|(id, _)| id).ok_or(kn_format::Error::Invalid("Blank has no table".into()))?;
            (first_sheet, info)
        } else {
            let s = crate::sheets::add_sheet(&mut doc, &mut h, first_sheet, sname, i)?;
            let t = crate::sheets::insert_table(&mut doc, &mut h, s, &donor, dinfo, 0.0, 0.0)?;
            (s, t)
        };
        crate::sheets::rename_sheet(&mut doc, &mut h, sheet, sname);
        fill_table(&mut doc, &mut h, info, rows, tname)?;
    }
    doc.end(&mut h);
    doc.dirty = true;
    Ok(Workbook { doc })
}

/// A new workbook holding the rows (table named `name`).
pub fn workbook_from_rows(rows: &[Vec<String>], name: &str) -> Result<Workbook, kn_format::Error> {
    let fields: Vec<Vec<Field>> = rows.iter().map(|r| r.iter().map(|s| Field::from_text(s)).collect()).collect();
    workbook_from_sheets(&[("Sheet 1".into(), name.into(), fields)])
}

/// Open a .csv / .tsv (.txt = tab-separated if it has tabs) as a new workbook.
pub fn import_delimited(path: &std::path::Path) -> Result<Workbook, kn_format::Error> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let first = text.lines().next().unwrap_or("");
    let sep = if ext == "tsv" || (ext != "csv" && first.contains('\t')) {
        '\t'
    } else if ext != "csv" && first.matches(';').count() > first.matches(',').count() {
        ';'
    } else {
        ','
    };
    let rows = parse_delimited(&text, sep);
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Table 1");
    workbook_from_rows(&rows, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields() {
        let rows = parse_delimited("a,\"b,c\",\"say \"\"hi\"\"\"\r\n1,2\n", ',');
        assert_eq!(rows, vec![vec!["a", "b,c", "say \"hi\""], vec!["1", "2"]]);
    }

    #[test]
    fn import_roundtrip() {
        let rows = parse_delimited("Item,Amount\nPen,3\nDesk,120\nLamp,=B2+B3\n", ',');
        let mut wb = workbook_from_rows(&rows, "Stock").unwrap();
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
        std::fs::create_dir_all(&dir).unwrap();
        wb.doc.save_as(dir.join("N16-csv-import.numbers")).unwrap();
        let back = Workbook::open(dir.join("N16-csv-import.numbers")).unwrap();
        let s = back.sheets().remove(0);
        let t = back.tables(&s).remove(0);
        assert_eq!(t.name, "Stock");
        let td = back.table_data(t.info).unwrap();
        assert_eq!((td.rows, td.cols, td.header_rows, td.header_cols), (4, 2, 1, 0));
        assert_eq!(td.cells, ["Item", "Amount", "Pen", "3", "Desk", "120", "Lamp", "123"]);
    }
}
