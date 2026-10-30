//! Cells with controls (Format > Checkbox / Star Rating; research 15-format-*): their value is
//! changed in place in the cell record, keeping its format and control spec.
//!   checkbox: cell type 6, flag 0x2, value a double at byte 12 (1.0 = checked)
//!   star rating: cell type 2, flag 0x1, value a decimal128 at byte 12 (0..5)

use kn_format::ObjId;
use kn_model::edit::History;
use kn_model::tablecells::CellInput;
use kn_model::Document;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    Checkbox(bool),
    Stars(u32),
}

fn record(doc: &Document, t: ObjId, r: u32, c: u32) -> Option<Vec<u8>> {
    let model = doc.file.obj(t)?.ref_id("tableModel").and_then(|m| doc.file.obj(m))?;
    kn_model::table::raw_cell_records(&doc.file, model).into_iter().find(|(rc, _)| *rc == (r, c)).map(|x| x.1)
}

/// The control of cell (r, c) (model row), judged by its record and what it shows.
pub fn control(doc: &Document, t: ObjId, r: u32, c: u32, shown: &str) -> Option<Control> {
    let rec = record(doc, t, r, c)?;
    if rec.first() != Some(&5) || rec.len() < 20 {
        return None;
    }
    let flags = u32::from_le_bytes(rec[8..12].try_into().ok()?);
    if rec[1] == 6 && flags & 2 != 0 && flags & 1 == 0 {
        let v = f64::from_le_bytes(rec[12..20].try_into().ok()?);
        return Some(Control::Checkbox(v != 0.0));
    }
    if flags & 1 != 0 && !shown.is_empty() && shown.chars().all(|ch| ch == '★' || ch == '☆' || ch == '•') {
        return Some(Control::Stars(shown.chars().filter(|ch| *ch == '★').count() as u32));
    }
    None
}

/// Set a checkbox / star rating cell's value.
pub fn set(doc: &mut Document, h: &mut History, t: ObjId, r: u32, c: u32, value: Control) -> Result<(), kn_format::Error> {
    let mut rec = record(doc, t, r, c).ok_or(kn_format::Error::Invalid("no such cell".into()))?;
    match value {
        Control::Checkbox(on) => rec[12..20].copy_from_slice(&(if on { 1.0f64 } else { 0.0 }).to_le_bytes()),
        Control::Stars(n) => {
            // decimal128 with exponent 0: mantissa n, then 0x3040
            let mut d = [0u8; 16];
            d[0] = n.min(5) as u8;
            d[14] = 0x40;
            d[15] = 0x30;
            rec[12..28].copy_from_slice(&d);
        }
    }
    doc.set_cell(h, t, r, c, &CellInput::Raw(rec))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str) -> crate::Workbook {
        crate::Workbook::open(format!("{}/../../testdata/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn checkbox_and_stars() {
        let mut wb = open("15-format-checkbox.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let shown = wb.table_data(t).unwrap().cells[8].clone();
        assert_eq!(control(&wb.doc, t, 1, 1, &shown), Some(Control::Checkbox(true)));
        let mut h = History::default();
        wb.doc.begin(&mut h, "x");
        set(&mut wb.doc, &mut h, t, 1, 1, Control::Checkbox(false)).unwrap();
        wb.doc.end(&mut h);
        let td = wb.table_data(t).unwrap();
        assert_eq!(control(&wb.doc, t, 1, 1, &td.cells[8]), Some(Control::Checkbox(false)));
        assert_ne!(td.cells[8], shown, "shows unchecked");
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
        std::fs::create_dir_all(&dir).unwrap();
        wb.doc.save_as(dir.join("N21-checkbox-unchecked.numbers")).unwrap();

        let mut wb = open("15-format-star-rating.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let shown = wb.table_data(t).unwrap().cells[8].clone();
        assert_eq!(control(&wb.doc, t, 1, 1, &shown), Some(Control::Stars(3)));
        wb.doc.begin(&mut h, "y");
        set(&mut wb.doc, &mut h, t, 1, 1, Control::Stars(5)).unwrap();
        wb.doc.end(&mut h);
        let td = wb.table_data(t).unwrap();
        assert_eq!(control(&wb.doc, t, 1, 1, &td.cells[8]), Some(Control::Stars(5)));
        let dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/n-edits"));
        wb.doc.save_as(dir.join("N18-stars-5.numbers")).unwrap();
    }
}
