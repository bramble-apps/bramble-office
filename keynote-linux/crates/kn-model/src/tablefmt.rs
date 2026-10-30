//! Format > Cell (Keynote 15.4): data formats, cell fills and cell borders, written the way
//! Keynote writes them (Mac round R33, testdata/r33):
//!
//! * Data format: an entry in the table's FORMAT list ({format_type, decimal_places, ...}),
//!   referenced from the cell record's slot for that kind of format (0x2000 number / percentage
//!   / fraction / scientific, 0x4000 currency, 0x8000 date, 0x10000 duration, 0x20000 text);
//!   field 0x1000 says which kind applies (1..5) and header bytes 6..8 mark a format the user
//!   chose (0x0001 number, 0x0802 currency, 0x0008 date, 0x0004 duration, 0x0080 text).
//! * Fill: a variation of the region's cell style with only cell_properties.cell_fill, in the
//!   table's STYLE list, referenced from field 0x20 (empty cells get a record for it).
//! * Border: runs in the StrokeSidecar's top / bottom / left / right stroke layers, one layer
//!   per row / column (`row_column_index`), each run {origin, length, stroke, order}; every
//!   change takes order = max_order + 3.

use kn_format::iwa::Archive;
use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::format::{fill_msg, stroke_msg, FillSpec, StrokeSpec};
use crate::table::CellValue;
use crate::tablecells::{list_add, list_release, CellInput};
use crate::Document;

/// Format > Cell > Data Format.
#[derive(Clone, Debug, PartialEq)]
pub enum CellFormat {
    Automatic,
    /// `decimals` None = Auto.
    Number { decimals: Option<u32>, thousands: bool, negative: u32 },
    Currency { code: String, decimals: Option<u32>, thousands: bool, negative: u32, accounting: bool },
    Percent { decimals: Option<u32>, thousands: bool, negative: u32 },
    /// Accuracy: 2 halves, 4 quarters, 8 eighths, 16, 10 tenths, 100; u32::MAX - n + 1 = "up
    /// to n digits".
    Fraction { accuracy: u32 },
    Scientific { decimals: Option<u32> },
    /// Numeral System: base 2..36 with at least `places` digits.
    Base { base: u32, places: u32, minus: bool },
    Text,
    /// An ICU pattern such as "M/d/yy h:mm a".
    DateTime { pattern: String },
    /// style 0 "0:00:00", 1 "0h 0m", 2 "0 hours 0 minutes"; units 1 weeks .. 32 milliseconds.
    Duration { style: u32, largest: u32, smallest: u32, automatic: bool },
    /// A custom format (Format > Create Custom Format: format_type 270 number, 271 date / time,
    /// 272 text, naming its definition by custom_uid), kept as stored: written back unchanged,
    /// so it only means the same thing within its own document.
    Custom { format: Msg },
}

/// Fraction accuracies in the popup's order (R33 fraction-popup-items.txt).
pub const FRACTION_ACCURACIES: [(u32, &str); 9] = [
    (u32::MAX, "Up to 1 digit (7/8)"),
    (u32::MAX - 1, "Up to 2 digits (23/24)"),
    (u32::MAX - 2, "Up to 3 digits (445/553)"),
    (2, "Halves"),
    (4, "Quarters"),
    (8, "Eighths"),
    (16, "Sixteenths"),
    (10, "Tenths"),
    (100, "Hundredths"),
];

/// Date and time patterns of the Date & Time popups (R33 datetime-popup-*.txt), shown for
/// Monday, January 5, 2026, 7:08:09 PM.
pub const DATE_PATTERNS: [&str; 14] =
    ["EEEE, MMMM d, y", "EEE, MMM d, y", "MMMM d, y", "MMM d, y", "MMMM d", "MMMM y", "MMMM", "y", "M/d", "M/d/y", "MM/dd/y", "M/d/yy", "MM/dd/yy", "y-MM-dd"];
pub const TIME_PATTERNS: [&str; 5] = ["h a", "h:mm a", "h:mm:ss a", "HH:mm", "HH:mm:ss"];

/// Currencies offered first (ISO codes).
pub const CURRENCIES: [&str; 12] = ["USD", "EUR", "GBP", "JPY", "CAD", "AUD", "CHF", "CNY", "INR", "BRL", "MXN", "ZAR"];

/// Cell borders to set, as in the inspector's 3x3 grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderEdges {
    All,
    Outside,
    Inside,
    Left,
    /// The vertical lines inside the range.
    Center,
    Right,
    Top,
    /// The horizontal lines inside the range.
    Middle,
    Bottom,
}

const KIND_NUMBER: u32 = 1;
const KIND_CURRENCY: u32 = 2;
const KIND_DATE: u32 = 3;
const KIND_DURATION: u32 = 4;
const KIND_TEXT: u32 = 5;
const SLOTS: [u32; 5] = [0x2000, 0x4000, 0x8000, 0x10000, 0x20000];

impl CellFormat {
    /// (kind, slot bit, header marker).
    fn kind(&self) -> (u32, u32, u16) {
        match self {
            CellFormat::Currency { .. } => (KIND_CURRENCY, 0x4000, 0x0802),
            CellFormat::DateTime { .. } => (KIND_DATE, 0x8000, 0x0008),
            CellFormat::Duration { .. } => (KIND_DURATION, 0x10000, 0x0004),
            CellFormat::Text => (KIND_TEXT, 0x20000, 0x0080),
            CellFormat::Custom { format } => match format.u32("format_type") {
                Some(271) => (KIND_DATE, 0x8000, 0x0008),
                Some(272) => (KIND_TEXT, 0x20000, 0x0080),
                _ => (KIND_NUMBER, 0x2000, 0x0001),
            },
            _ => (KIND_NUMBER, 0x2000, 0x0001),
        }
    }

    /// The TSK.FormatStructArchive Keynote writes for it (charts' number formats too).
    pub fn msg(&self) -> Msg {
        if let CellFormat::Custom { format } = self {
            return format.clone();
        }
        let mut f = Msg::new("TSK.FormatStructArchive");
        let dec = |d: &Option<u32>| u64::from(d.unwrap_or(253));
        match self {
            CellFormat::Automatic | CellFormat::Number { .. } => {
                let (d, t, n) = match self {
                    CellFormat::Number { decimals, thousands, negative } => (*decimals, *thousands, *negative),
                    _ => (None, false, 0),
                };
                f.set_u64("format_type", 256);
                f.set_u64("decimal_places", dec(&d));
                f.set_u64("negative_style", u64::from(n));
                f.set_bool("show_thousands_separator", t);
            }
            CellFormat::Currency { code, decimals, thousands, negative, accounting } => {
                f.set_u64("format_type", 257);
                f.set_u64("decimal_places", dec(decimals));
                f.set_str("currency_code", code);
                f.set_u64("negative_style", u64::from(*negative));
                f.set_bool("show_thousands_separator", *thousands);
                f.set_bool("use_accounting_style", *accounting);
            }
            CellFormat::Percent { decimals, thousands, negative } => {
                f.set_u64("format_type", 258);
                f.set_u64("decimal_places", dec(decimals));
                f.set_u64("negative_style", u64::from(*negative));
                f.set_bool("show_thousands_separator", *thousands);
            }
            CellFormat::Scientific { decimals } => {
                f.set_u64("format_type", 259);
                f.set_u64("decimal_places", dec(decimals));
                f.set_u64("negative_style", 0);
                f.set_bool("show_thousands_separator", false);
            }
            CellFormat::Fraction { accuracy } => {
                f.set_u64("format_type", 262);
                f.set_u64("fraction_accuracy", u64::from(*accuracy));
            }
            CellFormat::Base { base, places, minus } => {
                f.set_u64("format_type", 269);
                f.set_u64("base", u64::from(*base));
                f.set_u64("base_places", u64::from(*places));
                f.set_bool("base_use_minus_sign", *minus);
            }
            CellFormat::Text => f.set_u64("format_type", 260),
            CellFormat::DateTime { pattern } => {
                f.set_u64("format_type", 261);
                f.set_str("date_time_format", pattern);
            }
            CellFormat::Custom { .. } => {}
            CellFormat::Duration { style, largest, smallest, automatic } => {
                f.set_u64("format_type", 268);
                f.set_u64("duration_style", u64::from(*style));
                f.set_u64("duration_unit_largest", u64::from(*largest));
                f.set_u64("duration_unit_smallest", u64::from(*smallest));
                f.set_bool("use_automatic_duration_units", *automatic);
            }
        }
        f
    }

    /// Read back from a FORMAT entry.
    pub fn from_msg(f: &Msg) -> CellFormat {
        let dec = || f.u32("decimal_places").filter(|d| *d != 253);
        let neg = f.u32("negative_style").unwrap_or(0);
        let sep = f.bool("show_thousands_separator").unwrap_or(false);
        match f.u32("format_type").unwrap_or(256) {
            270..=272 if f.has("custom_uid") => CellFormat::Custom { format: f.clone() },
            257 => CellFormat::Currency {
                code: f.str("currency_code").unwrap_or("USD").to_string(),
                decimals: dec(),
                thousands: sep,
                negative: neg,
                accounting: f.bool("use_accounting_style").unwrap_or(false),
            },
            258 => CellFormat::Percent { decimals: dec(), thousands: sep, negative: neg },
            259 => CellFormat::Scientific { decimals: dec() },
            260 => CellFormat::Text,
            261 => CellFormat::DateTime { pattern: f.str("date_time_format").unwrap_or("M/d/yy").to_string() },
            262 => CellFormat::Fraction { accuracy: f.u32("fraction_accuracy").unwrap_or(u32::MAX - 2) },
            268 => CellFormat::Duration {
                style: f.u32("duration_style").unwrap_or(1),
                largest: f.u32("duration_unit_largest").unwrap_or(1),
                smallest: f.u32("duration_unit_smallest").unwrap_or(32),
                automatic: f.bool("use_automatic_duration_units").unwrap_or(true),
            },
            269 => CellFormat::Base { base: f.u32("base").unwrap_or(2), places: f.u32("base_places").unwrap_or(0), minus: f.bool("base_use_minus_sign").unwrap_or(true) },
            _ => CellFormat::Number { decimals: dec(), thousands: sep, negative: neg },
        }
    }
}

/// The fields Keynote compares formats by.
fn format_key(f: &Msg) -> String {
    let mut s = String::new();
    for k in [
        "format_type",
        "decimal_places",
        "negative_style",
        "fraction_accuracy",
        "duration_style",
        "duration_unit_largest",
        "duration_unit_smallest",
        "base",
        "base_places",
    ] {
        s.push_str(&format!("{:?},", f.u64(k)));
    }
    for k in ["show_thousands_separator", "use_accounting_style", "use_automatic_duration_units", "base_use_minus_sign"] {
        s.push_str(&format!("{:?},", f.bool(k)));
    }
    for k in ["currency_code", "date_time_format", "custom_format_string"] {
        s.push_str(&format!("{:?},", f.str(k)));
    }
    // a custom format is told apart by the definition it names
    if let Some(u) = f.msg("custom_uid") {
        s.push_str(&format!("{:?},{:?}", u.u64("lower"), u.u64("upper")));
    }
    s
}

/// A new cell record holding a date, duration or bool value (they have no CellInput of their
/// own); None for the other kinds.
pub(crate) fn typed_value_record(v: &crate::table::CellValue) -> Option<Vec<u8>> {
    use crate::table::CellValue;
    let mut rec = Record::empty();
    match v {
        CellValue::Date(secs) => {
            rec.head[1] = 5;
            rec.set_bytes(0x4, secs.to_le_bytes().to_vec());
        }
        CellValue::Duration(secs) => {
            rec.head[1] = 7;
            rec.set_bytes(0x2, secs.to_le_bytes().to_vec());
        }
        CellValue::Bool(b) => {
            rec.head[1] = 6;
            rec.set_bytes(0x2, (if *b { 1.0f64 } else { 0.0 }).to_le_bytes().to_vec());
        }
        _ => return None,
    }
    Some(rec.bytes())
}

/// A storage_version 5 record taken apart: header bytes and fields in flag-bit order.
struct Record {
    head: [u8; 8],
    fields: Vec<(u32, Vec<u8>)>,
}

/// Field sizes in flag-bit order (as tablecells).
fn field_size(bit: u32) -> usize {
    match bit {
        0x1 => 16,
        0x2 | 0x4 => 8,
        _ => 4,
    }
}

impl Record {
    fn parse(rec: &[u8]) -> Option<Record> {
        let head: [u8; 8] = rec.get(..8)?.try_into().ok()?;
        let flags = u32::from_le_bytes(rec.get(8..12)?.try_into().ok()?);
        let mut at = 12;
        let mut fields = Vec::new();
        for i in 0..18 {
            let bit = 1u32 << i;
            if flags & bit != 0 {
                let n = field_size(bit);
                fields.push((bit, rec.get(at..at + n)?.to_vec()));
                at += n;
            }
        }
        Some(Record { head, fields })
    }

    fn empty() -> Record {
        Record { head: [5, 0, 0, 0, 0, 0, 0, 0], fields: Vec::new() }
    }

    fn get(&self, bit: u32) -> Option<u32> {
        self.fields.iter().find(|f| f.0 == bit).and_then(|f| f.1.get(..4)).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn set_bytes(&mut self, bit: u32, v: Vec<u8>) {
        self.fields.retain(|f| f.0 != bit);
        self.fields.push((bit, v));
        self.fields.sort_by_key(|f| f.0);
    }

    fn set(&mut self, bit: u32, v: u32) {
        self.set_bytes(bit, v.to_le_bytes().to_vec());
    }

    fn clear(&mut self, bit: u32) {
        self.fields.retain(|f| f.0 != bit);
    }

    fn marker(&self) -> u16 {
        u16::from_le_bytes([self.head[6], self.head[7]])
    }

    fn set_marker(&mut self, m: u16) {
        self.head[6..8].copy_from_slice(&m.to_le_bytes());
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = self.head.to_vec();
        out.extend(self.fields.iter().fold(0u32, |a, f| a | f.0).to_le_bytes());
        for (_, v) in &self.fields {
            out.extend(v);
        }
        out
    }
}

/// What a cell holds now, in the terms a format change needs.
fn value_of(values: &std::collections::HashMap<(u32, u32), CellValue>, rc: (u32, u32)) -> CellValue {
    values.get(&rc).cloned().unwrap_or(CellValue::Empty)
}

impl Document {
    fn table_lists(&self, table: ObjId) -> Result<(ObjId, ObjId, ObjId, ObjId), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let model_id = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).ok_or(err("not a table"))?;
        let ds = self.file.obj(model_id).and_then(|m| m.msg("base_data_store")).ok_or(err("no data store"))?;
        Ok((
            model_id,
            ds.ref_id("format_table").ok_or(err("no format list"))?,
            ds.ref_id("stringTable").ok_or(err("no string list"))?,
            ds.ref_id("styleTable").ok_or(err("no style list"))?,
        ))
    }

    /// The data format of cell (row, col): Automatic unless one was chosen.
    pub fn cell_format(&self, table: ObjId, row: u32, col: u32) -> CellFormat {
        let Ok((model_id, formats, ..)) = self.table_lists(table) else { return CellFormat::Automatic };
        let Some(model) = self.file.obj(model_id) else { return CellFormat::Automatic };
        let Some(rec) = crate::table::raw_cell_records(&self.file, model).into_iter().find(|(rc, _)| *rc == (row, col)).and_then(|(_, r)| Record::parse(&r)) else {
            return CellFormat::Automatic;
        };
        if rec.marker() == 0 {
            return CellFormat::Automatic;
        }
        let kind = rec.get(0x1000).unwrap_or(KIND_NUMBER).clamp(1, 5);
        let key = rec.get(SLOTS[(kind - 1) as usize]);
        let f = key.and_then(|k| self.file.obj(formats)?.msgs("entries").find(|e| e.u32("key") == Some(k))?.msg("format").cloned());
        f.map(|f| CellFormat::from_msg(&f)).unwrap_or(CellFormat::Automatic)
    }

    /// Give the cells with values in block (r0, c0, r1, c1) a data format. Numbers become text
    /// for Text and durations (in days) for Duration, as Keynote converts them; text that reads
    /// as a number becomes that number for the number formats. Returns the cells changed.
    pub fn set_cells_format(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), fmt: &CellFormat) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (model_id, formats, strings, _) = self.table_lists(table)?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?;
        let values = self.cell_values(table);
        let (rows, cols) = (model.u32("number_of_rows").unwrap_or(0), model.u32("number_of_columns").unwrap_or(0));
        let mut have: std::collections::HashMap<(u32, u32), Vec<u8>> = crate::table::raw_cell_records(&self.file, model).into_iter().collect();
        // every cell of the block: empty ones keep the format for what is typed later (R34)
        let mut cells: Vec<((u32, u32), Option<Vec<u8>>)> = Vec::new();
        for r in block.0..=block.2.min(rows.saturating_sub(1)) {
            for c in block.1..=block.3.min(cols.saturating_sub(1)) {
                cells.push(((r, c), have.remove(&(r, c))));
            }
        }
        self.touch(h, formats);
        self.touch(h, strings);
        let mut n = 0;
        for ((r, c), raw) in cells {
            let mut rec = match &raw {
                Some(raw) if raw.first() == Some(&5) => match Record::parse(raw) {
                    Some(rec) => rec,
                    None => continue,
                },
                Some(_) => continue,
                None if *fmt == CellFormat::Automatic => continue,
                None => Record::empty(),
            };
            if rec.get(0x200).is_some() && matches!(fmt, CellFormat::Text | CellFormat::Duration { .. } | CellFormat::DateTime { .. }) {
                // formula results keep their kind of value
                continue;
            }
            let mut value = value_of(&values, (r, c));
            // the value as the new format needs it
            match (fmt, &value) {
                (CellFormat::Text, CellValue::Number(x)) => {
                    let s = crate::table::format_value(&CellValue::Number(*x), None);
                    let sl = self.file.obj_mut(strings).ok_or(err("string list"))?;
                    let sid = list_add(sl, |e| e.str("string") == Some(s.as_str()), |e| e.set_str("string", &s));
                    rec.clear(0x1);
                    rec.clear(0x2);
                    rec.set(0x8, sid);
                    rec.head[1] = 3;
                    value = CellValue::Text(s);
                }
                (CellFormat::Duration { .. }, CellValue::Number(x)) => {
                    rec.clear(0x1);
                    rec.set_bytes(0x2, (x * 86_400.0).to_le_bytes().to_vec());
                    rec.head[1] = 7;
                    value = CellValue::Duration(x * 86_400.0);
                }
                (CellFormat::Number { .. } | CellFormat::Currency { .. } | CellFormat::Percent { .. } | CellFormat::Scientific { .. } | CellFormat::Fraction { .. } | CellFormat::Base { .. }, CellValue::Text(s)) => {
                    if let Ok(x) = s.trim().replace(',', "").parse::<f64>() {
                        if let Some(k) = rec.get(0x8) {
                            if let Some(l) = self.file.obj_mut(strings) {
                                list_release(l, k);
                            }
                        }
                        rec.clear(0x8);
                        rec.set_bytes(0x1, crate::tablecells::decimal128_bytes(x).to_vec());
                        rec.head[1] = 2;
                        value = CellValue::Number(x);
                    }
                }
                _ => {}
            }
            // currency numbers are their own cell type
            if let CellValue::Number(_) = value {
                rec.head[1] = if matches!(fmt, CellFormat::Currency { .. }) { 10 } else { 2 };
            }
            // let go of the format slots the cell had
            for bit in SLOTS {
                if let Some(k) = rec.get(bit) {
                    if let Some(l) = self.file.obj_mut(formats) {
                        list_release(l, k);
                    }
                    rec.clear(bit);
                }
            }
            if rec.head[1] == 0 && *fmt == CellFormat::Automatic {
                rec.clear(0x1000);
                rec.set_marker(0);
                let input = if rec.fields.is_empty() { CellInput::Empty } else { CellInput::Raw(rec.bytes()) };
                self.set_cell(h, table, r, c, &input)?;
                n += 1;
                continue;
            }
            let (kind, bit, marker) = match fmt {
                CellFormat::Automatic => {
                    let kind = match value {
                        CellValue::Text(_) => KIND_TEXT,
                        CellValue::Date(_) => KIND_DATE,
                        CellValue::Duration(_) => KIND_DURATION,
                        _ => KIND_NUMBER,
                    };
                    (kind, SLOTS[(kind - 1) as usize], 0)
                }
                f => f.kind(),
            };
            let f = match (fmt, kind) {
                (CellFormat::Automatic, KIND_TEXT) => CellFormat::Text.msg(),
                (CellFormat::Automatic, KIND_DATE) => CellFormat::DateTime { pattern: "M/d/yy h:mm a".into() }.msg(),
                (CellFormat::Automatic, KIND_DURATION) => CellFormat::Duration { style: 1, largest: 1, smallest: 32, automatic: true }.msg(),
                _ => fmt.msg(),
            };
            let want = format_key(&f);
            let fl = self.file.obj_mut(formats).ok_or(err("format list"))?;
            let key = list_add(fl, |e| e.msg("format").is_some_and(|x| format_key(x) == want), |e| e.push_msg("format", f));
            rec.set(0x1000, kind);
            rec.set(bit, key);
            rec.set_marker(marker);
            self.set_cell(h, table, r, c, &CellInput::Raw(rec.bytes()))?;
            n += 1;
        }
        self.file.refresh_refs(formats);
        Ok(n)
    }

    /// Fill the cells of block (r0, c0, r1, c1) (None: back to the table style's fill).
    pub fn set_cells_fill(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), fill: Option<&FillSpec>) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (model_id, _, _, list) = self.table_lists(table)?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?.clone();
        let t = self.file.obj(table).and_then(|m| crate::table::table_shape(&self.file, m)).ok_or(err("not a table"))?;
        let raws: std::collections::HashMap<(u32, u32), Vec<u8>> = crate::table::raw_cell_records(&self.file, &model).into_iter().collect();
        let styles: std::collections::HashMap<u32, ObjId> =
            self.file.obj(list).map(|l| l.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.ref_id("reference")?))).collect()).unwrap_or_default();
        let base_of = |r: u32, c: u32| {
            let field = if r < t.header_rows {
                "header_row_style"
            } else if r >= t.rows.saturating_sub(t.footer_rows) {
                "footer_row_style"
            } else if c < t.header_cols {
                "header_column_style"
            } else {
                "body_cell_style"
            };
            model.ref_id(field)
        };
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().chain([list]).collect();
        self.touch(h, list);
        let mut made: Vec<(ObjId, u32)> = Vec::new();
        let mut n = 0;
        for r in block.0..=block.2.min(t.rows.saturating_sub(1)) {
            for c in block.1..=block.3.min(t.cols.saturating_sub(1)) {
                // merged-away cells keep nothing of their own
                if t.merge_at(r, c).is_some_and(|m| (m.row, m.col) != (r, c)) {
                    continue;
                }
                let mut rec = match raws.get(&(r, c)) {
                    Some(raw) if raw.first() == Some(&5) => Record::parse(raw).ok_or(err("cell record"))?,
                    Some(_) => continue,
                    None if fill.is_none() => continue,
                    None => Record::empty(),
                };
                let old = rec.get(0x20);
                if let Some(o) = old {
                    if let Some(l) = self.file.obj_mut(list) {
                        list_release(l, o);
                    }
                    rec.clear(0x20);
                }
                if let Some(f) = fill {
                    // variations of the cell's own style keep what else it changed
                    let base = old.and_then(|k| styles.get(&k).copied()).or_else(|| base_of(r, c)).ok_or(err("no cell style"))?;
                    let key = match made.iter().find(|m| m.0 == base) {
                        Some(&(_, k)) => {
                            if let Some(e) = self.file.obj_mut(list).and_then(|l| l.msgs_mut("entries").find(|e| e.u32("key") == Some(k))) {
                                let rc = e.u32("refcount").unwrap_or(0);
                                e.set_u64("refcount", u64::from(rc + 1));
                            }
                            k
                        }
                        None => {
                            let v = self.structural(h, &touch, |k| k.new_style_variation(base))?;
                            if let Some(m) = self.file.obj_mut(v) {
                                m.msg_mut_or_create("cell_properties").remove("cell_fill");
                                m.msg_mut_or_create("cell_properties").push_msg("cell_fill", fill_msg(f));
                                kn_format::objects::fix_override_count(m);
                            }
                            crate::format::add_data_ref(&mut self.file, v, crate::format::fill_data(f));
                            let l = self.file.obj_mut(list).ok_or(err("style list"))?;
                            let k = list_add(l, |_| false, |e| e.set_ref("reference", v));
                            self.file.refresh_refs(list);
                            made.push((base, k));
                            k
                        }
                    };
                    rec.set(0x20, key);
                }
                // an empty cell left with nothing of its own goes away
                let input = if rec.head[1] == 0 && rec.fields.is_empty() { CellInput::Empty } else { CellInput::Raw(rec.bytes()) };
                self.set_cell(h, table, r, c, &input)?;
                n += 1;
            }
        }
        self.file.refresh_refs(list);
        Ok(n)
    }

    /// Format > Table > Wrap Text (Cell inspector > Wrap Text in Cell): a variation of each
    /// cell's style with cell_properties.text_wrap, as set_cells_fill makes for a fill.
    pub fn set_cells_wrap(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), wrap: bool) -> Result<usize, kn_format::Error> {
        self.set_cells_cell_props(h, table, block, &|p| p.set_bool("text_wrap", wrap))
    }

    /// The cells' vertical alignment (Text inspector > Top / Middle / Bottom for cells):
    /// cell_properties.vertical_alignment (0 top, 1 middle, 2 bottom), as table() reads it.
    pub fn set_cells_valign(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), v: crate::VAlign) -> Result<usize, kn_format::Error> {
        let n = match v {
            crate::VAlign::Top => 0,
            crate::VAlign::Middle => 1,
            crate::VAlign::Bottom => 2,
        };
        self.set_cells_cell_props(h, table, block, &|p| p.set_i64("vertical_alignment", n))
    }

    /// The cells' Text Inset (left, top, right, bottom, points): cell_properties.padding.
    pub fn set_cells_padding(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), pad: [f32; 4]) -> Result<usize, kn_format::Error> {
        self.set_cells_cell_props(h, table, block, &|p| {
            let pd = p.msg_mut_or_create("padding");
            for (f, v) in ["left", "top", "right", "bottom"].iter().zip(pad) {
                pd.set_f32(f, v.clamp(0.0, 200.0));
            }
        })
    }

    /// Give the cells in `block` (empty ones too; merged-away ones skipped) a variation of their
    /// cell style with `edit` applied to its cell_properties: one variation per base style.
    fn set_cells_cell_props(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), edit: &dyn Fn(&mut Msg)) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (model_id, _, _, list) = self.table_lists(table)?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?.clone();
        let t = self.file.obj(table).and_then(|m| crate::table::table_shape(&self.file, m)).ok_or(err("not a table"))?;
        let raws: std::collections::HashMap<(u32, u32), Vec<u8>> = crate::table::raw_cell_records(&self.file, &model).into_iter().collect();
        let styles: std::collections::HashMap<u32, ObjId> =
            self.file.obj(list).map(|l| l.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.ref_id("reference")?))).collect()).unwrap_or_default();
        let base_of = |r: u32, c: u32| {
            let field = if r < t.header_rows {
                "header_row_style"
            } else if r >= t.rows.saturating_sub(t.footer_rows) {
                "footer_row_style"
            } else if c < t.header_cols {
                "header_column_style"
            } else {
                "body_cell_style"
            };
            model.ref_id(field)
        };
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().chain([list]).collect();
        self.touch(h, list);
        // one variation per style varied
        let mut made: Vec<(ObjId, u32)> = Vec::new();
        let mut n = 0;
        for r in block.0..=block.2.min(t.rows.saturating_sub(1)) {
            for c in block.1..=block.3.min(t.cols.saturating_sub(1)) {
                if t.merge_at(r, c).is_some_and(|m| (m.row, m.col) != (r, c)) {
                    continue;
                }
                let mut rec = match raws.get(&(r, c)) {
                    Some(raw) if raw.first() == Some(&5) => Record::parse(raw).ok_or(err("cell record"))?,
                    Some(_) => continue,
                    None => Record::empty(),
                };
                let old = rec.get(0x20);
                let base = old.and_then(|k| styles.get(&k).copied()).or_else(|| base_of(r, c)).ok_or(err("no cell style"))?;
                if let Some(o) = old {
                    if let Some(l) = self.file.obj_mut(list) {
                        list_release(l, o);
                    }
                    rec.clear(0x20);
                }
                let key = match made.iter().find(|m| m.0 == base) {
                    Some(&(_, k)) => {
                        if let Some(e) = self.file.obj_mut(list).and_then(|l| l.msgs_mut("entries").find(|e| e.u32("key") == Some(k))) {
                            let rc = e.u32("refcount").unwrap_or(0);
                            e.set_u64("refcount", u64::from(rc + 1));
                        }
                        k
                    }
                    None => {
                        let v = self.structural(h, &touch, |k| k.new_style_variation(base))?;
                        if let Some(m) = self.file.obj_mut(v) {
                            edit(m.msg_mut_or_create("cell_properties"));
                            kn_format::objects::fix_override_count(m);
                        }
                        let l = self.file.obj_mut(list).ok_or(err("style list"))?;
                        let k = list_add(l, |_| false, |e| e.set_ref("reference", v));
                        self.file.refresh_refs(list);
                        made.push((base, k));
                        k
                    }
                };
                rec.set(0x20, key);
                self.set_cell(h, table, r, c, &CellInput::Raw(rec.bytes()))?;
                n += 1;
            }
        }
        self.file.refresh_refs(list);
        Ok(n)
    }

    /// Set (or, with None, take away) the borders `edges` of block (r0, c0, r1, c1).
    pub fn set_cells_border(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), edges: BorderEdges, stroke: Option<&StrokeSpec>) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (model_id, ..) = self.table_lists(table)?;
        let sidecar = self.file.obj(model_id).and_then(|m| m.ref_id("stroke_sidecar")).ok_or(err("this table can't take cell borders yet"))?;
        let (r0, c0, r1, c1) = block;
        // (layer field, cell row / column, first cell along the edge, cells). Layers are kept per
        // cell row / column and side, so a line between two rows of the block is written from
        // both sides: the bottom of one row and the top of the next (Keynote, R34)
        let mut segs: Vec<(&str, u32, u32, u32)> = Vec::new();
        let (rows_n, cols_n) = (r1 - r0 + 1, c1 - c0 + 1);
        let (outer_h, inner_h) = match edges {
            BorderEdges::All => (true, true),
            BorderEdges::Outside => (true, false),
            BorderEdges::Inside | BorderEdges::Middle => (false, true),
            _ => (false, false),
        };
        let (outer_v, inner_v) = match edges {
            BorderEdges::All => (true, true),
            BorderEdges::Outside => (true, false),
            BorderEdges::Inside | BorderEdges::Center => (false, true),
            _ => (false, false),
        };
        for r in r0..=r1 {
            if (r == r0 && (outer_h || edges == BorderEdges::Top)) || (r > r0 && inner_h) {
                segs.push(("top_row_stroke_layers", r, c0, cols_n));
            }
            if (r == r1 && (outer_h || edges == BorderEdges::Bottom)) || (r < r1 && inner_h) {
                segs.push(("bottom_row_stroke_layers", r, c0, cols_n));
            }
        }
        for c in c0..=c1 {
            if (c == c0 && (outer_v || edges == BorderEdges::Left)) || (c > c0 && inner_v) {
                segs.push(("left_column_stroke_layers", c, r0, rows_n));
            }
            if (c == c1 && (outer_v || edges == BorderEdges::Right)) || (c < c1 && inner_v) {
                segs.push(("right_column_stroke_layers", c, r0, rows_n));
            }
        }
        if segs.is_empty() {
            return Ok(());
        }
        let file = self.file.file_of(sidecar).unwrap_or("Index/CalculationEngine.iwa").to_string();
        // the new runs go on top of the old ones (Keynote: +3 for a line, +1 for No Border)
        let order = self.file.obj(sidecar).and_then(|s| s.u32("max_order")).unwrap_or(0) + if stroke.is_some() { 3 } else { 1 };
        let mut stroke_m = stroke_msg(stroke);
        if stroke.is_none() {
            stroke_m.set_f32("width", 0.0);
        }
        self.structural(h, &[sidecar], |k| {
            for (field, idx, origin, len) in segs {
                let have = k.obj(sidecar).map(|s| s.ref_ids(field)).unwrap_or_default();
                let layer = match have.iter().copied().find(|l| k.obj(*l).and_then(|m| m.u32("row_column_index")) == Some(idx)) {
                    Some(l) => l,
                    None => {
                        let id = k.alloc_ids(1)[0];
                        let mut m = Msg::new("TST.StrokeLayerArchive");
                        m.set_u64("row_column_index", u64::from(idx));
                        k.insert_archive(&file, Archive::new_object(id, m));
                        // layers stay in index order
                        let mut all: Vec<(u32, ObjId)> = have.iter().filter_map(|l| Some((k.obj(*l)?.u32("row_column_index")?, *l))).collect();
                        all.push((idx, id));
                        all.sort();
                        if let Some(s) = k.obj_mut(sidecar) {
                            s.remove(field);
                            for (_, l) in all {
                                s.push_ref(field, l);
                            }
                        }
                        id
                    }
                };
                let Some(m) = k.obj_mut(layer) else { continue };
                // the new run replaces what other runs had along its cells
                let end = origin + len;
                let mut runs: Vec<Msg> = Vec::new();
                for run in m.msgs("stroke_runs") {
                    let (o, l) = (run.i64("origin").unwrap_or(0).max(0) as u32, run.u32("length").unwrap_or(0));
                    let e = o + l;
                    if e <= origin || o >= end {
                        runs.push(run.clone());
                        continue;
                    }
                    if o < origin {
                        let mut a = run.clone();
                        a.set_u64("length", u64::from(origin - o));
                        runs.push(a);
                    }
                    if e > end {
                        let mut b = run.clone();
                        b.set_i64("origin", i64::from(end));
                        b.set_u64("length", u64::from(e - end));
                        runs.push(b);
                    }
                }
                let mut run = Msg::new("TST.StrokeLayerArchive.StrokeRunArchive");
                run.set_i64("origin", i64::from(origin));
                run.set_u64("length", u64::from(len));
                run.push_msg("stroke", stroke_m.clone());
                run.set_u64("order", u64::from(order));
                runs.push(run);
                runs.sort_by_key(|r| r.i64("origin").unwrap_or(0));
                m.remove("stroke_runs");
                for r in runs {
                    m.push_msg("stroke_runs", r);
                }
            }
            if let Some(s) = k.obj_mut(sidecar) {
                s.set_u64("max_order", u64::from(order));
            }
            k.refresh_refs(sidecar);
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, DrawableKind};

    /// A custom format (as in the user's Numbers calendar: format_type 271, custom_uid) reads
    /// back as Custom, is written unchanged, and two custom formats stay apart in the list.
    #[test]
    fn custom_format_kept_as_stored() {
        let custom = |lower: u64| {
            let mut f = Msg::new("TSK.FormatStructArchive");
            f.set_u64("format_type", 271);
            f.set_str("custom_format_string", " d");
            let u = f.msg_mut_or_create("custom_uid");
            u.set_u64("lower", lower);
            u.set_u64("upper", 7);
            f
        };
        let f = custom(1);
        let cf = CellFormat::from_msg(&f);
        assert_eq!(cf, CellFormat::Custom { format: f.clone() });
        assert_eq!(cf.msg(), f);
        assert_eq!(cf.kind().0, KIND_DATE);
        assert_ne!(format_key(&f), format_key(&custom(2)));
        // without a custom_uid it's no custom format
        let mut plain = f.clone();
        plain.remove("custom_uid");
        assert!(!matches!(CellFormat::from_msg(&plain), CellFormat::Custom { .. }));
    }

    fn r33(name: &str) -> Document {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r33").join(name);
        Document::open(p).unwrap()
    }

    fn table_of(d: &Document) -> (ObjId, crate::TableData) {
        let sc = d.scene(&d.slides(), 0).unwrap();
        sc.drawables.into_iter().find_map(|x| if let DrawableKind::Table(t) = x.kind { Some((x.id, t)) } else { None }).unwrap()
    }

    fn reopen(d: &Document) -> Document {
        Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap())
    }

    /// Each R33 step done here on R33-0-before.key shows what Keynote shows and stores the same
    /// format entry and cell record fields as Keynote's file.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn r33_formats_like_keynote() {
        let mut d = r33("R33-0-before.key");
        let (t, _) = table_of(&d);
        // the R33 values (Keynote set them by AppleScript in step 1)
        let mut h = History::default();
        d.begin(&mut h, "Values");
        for r in 1..=9 {
            d.set_cell(&mut h, t, r, 1, &CellInput::Number(1234.5678)).unwrap();
        }
        d.end(&mut h);
        let steps: Vec<(u32, CellFormat, &str)> = vec![
            (1, CellFormat::Number { decimals: Some(2), thousands: true, negative: 0 }, "1,234.57"),
            (2, CellFormat::Number { decimals: Some(0), thousands: false, negative: 3 }, "1235"),
            (3, CellFormat::Currency { code: "USD".into(), decimals: Some(2), thousands: false, negative: 0, accounting: false }, "$1234.57"),
            (5, CellFormat::Percent { decimals: Some(1), thousands: false, negative: 0 }, "123456.8%"),
            (6, CellFormat::Fraction { accuracy: 2 }, "1234 1/2"),
            (7, CellFormat::Scientific { decimals: Some(2) }, "1.23E+03"),
            (8, CellFormat::Text, "1234.5678"),
            (9, CellFormat::Duration { style: 1, largest: 1, smallest: 32, automatic: true }, "176w 2d 13h 37m 37s 920ms"),
        ];
        for (r, f, _) in &steps {
            d.begin(&mut h, "Format");
            d.set_cells_format(&mut h, t, (*r, 1, *r, 1), f).unwrap();
            d.end(&mut h);
        }
        let d = reopen(&d);
        let (_, tb) = table_of(&d);
        let theirs = r33("R33-formats.key");
        let (tt, _) = table_of(&theirs);
        for (r, f, shown) in &steps {
            assert_eq!(tb.cells[(r * tb.cols + 1) as usize], *shown, "row {r}");
            assert_eq!(&d.cell_format(t, *r, 1), f, "row {r} reads back");
            assert_eq!(&theirs.cell_format(tt, *r, 1), f, "Keynote's row {r} reads the same");
        }
        // the records match Keynote's field for field (values aside)
        let model = |d: &Document, t: ObjId| d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
        let ours: std::collections::HashMap<_, _> = crate::table::raw_cell_records(&d.file, &model(&d, t)).into_iter().collect();
        let keynote: std::collections::HashMap<_, _> = crate::table::raw_cell_records(&theirs.file, &model(&theirs, tt)).into_iter().collect();
        for (r, ..) in &steps {
            let (a, b) = (Record::parse(&ours[&(*r, 1)]).unwrap(), Record::parse(&keynote[&(*r, 1)]).unwrap());
            assert_eq!(a.head[..2], b.head[..2], "row {r} cell type");
            assert_eq!(a.marker(), b.marker(), "row {r} format marker");
            assert_eq!(a.fields.iter().map(|f| f.0).collect::<Vec<_>>(), b.fields.iter().map(|f| f.0).collect::<Vec<_>>(), "row {r} fields");
            assert_eq!(a.get(0x1000), b.get(0x1000), "row {r} format kind");
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn fills_and_borders_like_keynote() {
        let mut d = r33("R33-0-before.key");
        let (t, _) = table_of(&d);
        let mut h = History::default();
        let red = crate::Color { r: 1.0, g: 0.1491, b: 0.0, a: 1.0 };
        let blue = crate::Color { r: 0.000639, g: 0.633603, b: 1.0, a: 1.0 };
        d.begin(&mut h, "Fill");
        assert_eq!(d.set_cells_fill(&mut h, t, (1, 2, 1, 2), Some(&FillSpec::Color(red))).unwrap(), 1);
        d.end(&mut h);
        d.begin(&mut h, "Border");
        d.set_cells_border(&mut h, t, (3, 2, 3, 2), BorderEdges::All, Some(&StrokeSpec { color: blue, width: 2.0, dash: crate::format::Dash::Solid })).unwrap();
        d.set_cells_border(&mut h, t, (4, 2, 4, 2), BorderEdges::Bottom, Some(&StrokeSpec { color: crate::Color::BLACK, width: 1.0, dash: crate::format::Dash::Solid })).unwrap();
        d.end(&mut h);
        let d = reopen(&d);
        let (_, tb) = table_of(&d);
        let fill = &tb.cell_styles[(tb.cols + 2) as usize].fill;
        assert!(matches!(fill, crate::Fill::Color(c) if (c.r - 1.0).abs() < 1e-3 && c.g < 0.2), "C2 is red: {fill:?}");
        // C4's four sides (row 3, column 2) and C5's bottom
        let hs = |line: u32, col: u32| tb.h_strokes[(line * tb.cols + col) as usize].clone();
        let vs = |row: u32, line: u32| tb.v_strokes[(row * (tb.cols + 1) + line) as usize].clone();
        for s in [hs(3, 2), hs(4, 2), vs(3, 2), vs(3, 3)] {
            let s = s.expect("C4 border");
            assert!((s.width - 2.0).abs() < 1e-3 && s.color.b > 0.9);
        }
        let s = hs(5, 2).expect("C5 bottom");
        assert!((s.width - 1.0).abs() < 1e-3 && s.color.b < 0.1);
        // stored as Keynote stores them: one layer per line, order max_order + 3
        let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
        let sc = d.file.obj(model.ref_id("stroke_sidecar").unwrap()).unwrap();
        let idx = |f: &str| sc.ref_ids(f).iter().map(|l| d.file.obj(*l).unwrap().u32("row_column_index").unwrap()).collect::<Vec<_>>();
        assert_eq!(idx("top_row_stroke_layers"), [3]);
        assert_eq!(idx("bottom_row_stroke_layers"), [3, 4]);
        assert_eq!(idx("left_column_stroke_layers"), [2]);
        assert_eq!(idx("right_column_stroke_layers"), [2]);
        assert_eq!(sc.u32("max_order"), Some(8));
    }

    /// Wrap Text off over a block holding a filled cell and a merge: text_wrap reads back off,
    /// the fill stays, the merge's covered cell is untouched, cells that shared a style share one
    /// variation (refcount 2), and undo puts every cell back.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn valign_sets_cell_vertical_alignment() {
        // each cell's style variation carries cell_properties.vertical_alignment; a fill stays,
        // wrap and valign combine, and it survives a save; undo restores the old look
        let mut d = r33("R33-0-before.key");
        let (t, tb0) = table_of(&d);
        let style = |tb: &crate::TableData, r: u32, c: u32| tb.cell_styles[(r * tb.cols + c) as usize].clone();
        let mut h = History::default();
        let red = crate::Color { r: 1.0, g: 0.1491, b: 0.0, a: 1.0 };
        d.begin(&mut h, "Setup");
        d.set_cells_fill(&mut h, t, (1, 2, 1, 2), Some(&FillSpec::Color(red))).unwrap();
        d.end(&mut h);
        for (v, want) in [(crate::VAlign::Top, crate::VAlign::Top), (crate::VAlign::Bottom, crate::VAlign::Bottom), (crate::VAlign::Middle, crate::VAlign::Middle)] {
            d.begin(&mut h, "Align");
            assert!(d.set_cells_valign(&mut h, t, (1, 1, 2, 2), v).unwrap() >= 4);
            d.end(&mut h);
            let (_, tb) = table_of(&d);
            for (r, c) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
                assert_eq!(style(&tb, r, c).valign, want, "{r},{c}");
            }
            assert!(matches!(&style(&tb, 1, 2).fill, crate::Fill::Color(c) if c.g < 0.2), "C2 keeps its fill");
        }
        d.begin(&mut h, "Align");
        d.set_cells_valign(&mut h, t, (1, 1, 1, 1), crate::VAlign::Bottom).unwrap();
        d.set_cells_wrap(&mut h, t, (1, 1, 1, 1), false).unwrap();
        d.end(&mut h);
        let (_, tb) = table_of(&d);
        assert_eq!(style(&tb, 1, 1).valign, crate::VAlign::Bottom);
        assert!(!style(&tb, 1, 1).wrap);
        let d2 = reopen(&d);
        let (_, tb2) = table_of(&d2);
        assert_eq!(style(&tb2, 1, 1).valign, crate::VAlign::Bottom);
        assert_eq!(style(&tb2, 2, 2).valign, crate::VAlign::Middle);
        while d.undo(&mut h) {}
        let (_, tb) = table_of(&d);
        assert_eq!(style(&tb, 1, 1).valign, style(&tb0, 1, 1).valign);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn wrap_off_keeps_fill_and_merges() {
        let mut d = r33("R33-0-before.key");
        let (t, _) = table_of(&d);
        let mut h = History::default();
        let red = crate::Color { r: 1.0, g: 0.1491, b: 0.0, a: 1.0 };
        d.begin(&mut h, "Setup");
        d.set_cells_fill(&mut h, t, (1, 2, 1, 2), Some(&FillSpec::Color(red))).unwrap();
        d.merge_cells(&mut h, t, crate::CellRange { row: 3, col: 1, rows: 1, cols: 2 }).unwrap();
        d.end(&mut h);
        let raws = |d: &Document| -> std::collections::HashMap<(u32, u32), Vec<u8>> {
            let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
            crate::table::raw_cell_records(&d.file, &model).into_iter().collect()
        };
        let before = raws(&d);
        let (_, tb) = table_of(&d);
        let style = |tb: &crate::TableData, r: u32, c: u32| tb.cell_styles[(r * tb.cols + c) as usize].clone();
        assert!(style(&tb, 1, 1).wrap, "wraps to begin with");
        d.begin(&mut h, "Wrap Text");
        d.set_cells_wrap(&mut h, t, (1, 1, 3, 2), false).unwrap();
        d.end(&mut h);
        let (_, tb) = table_of(&d);
        for (r, c) in [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1)] {
            assert!(!style(&tb, r, c).wrap, "{r},{c} no longer wraps");
        }
        assert!(matches!(&style(&tb, 1, 2).fill, crate::Fill::Color(c) if (c.r - 1.0).abs() < 1e-3 && c.g < 0.2), "C2 keeps its fill");
        let after = raws(&d);
        assert_eq!(after.get(&(3, 2)), before.get(&(3, 2)), "the merge's covered cell is untouched");
        // cells that had the same (body) style share one variation; its refcount counts them
        let key = |r, c| after.get(&(r, c)).and_then(|raw| Record::parse(raw)).and_then(|rec| rec.get(0x20)).unwrap();
        assert_eq!(key(1, 1), key(2, 1));
        assert_ne!(key(1, 1), key(1, 2), "the filled cell has its own variation");
        let users = after.values().filter(|raw| Record::parse(raw).and_then(|rec| rec.get(0x20)) == Some(key(1, 1))).count() as u32;
        assert!(users >= 2);
        let (_, _, _, list) = d.table_lists(t).unwrap();
        let rc = d.file.obj(list).unwrap().msgs("entries").find(|e| e.u32("key") == Some(key(1, 1))).and_then(|e| e.u32("refcount"));
        assert_eq!(rc, Some(users));
        // saved and read back
        let d2 = reopen(&d);
        let (_, tb2) = table_of(&d2);
        assert!(!style(&tb2, 2, 2).wrap);
        // undo: every cell as it was
        assert!(d.undo(&mut h));
        assert_eq!(raws(&d), before);
        let (_, tb) = table_of(&d);
        assert!(style(&tb, 1, 1).wrap);
    }

    /// R34: a format set on an empty cell is kept, and a number typed into it uses it, as in
    /// Keynote; typing into a filled cell keeps its fill.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn empty_cells_keep_their_format_and_look() {
        let mut d = r33("R33-0-before.key");
        let (t, _) = table_of(&d);
        let mut h = History::default();
        let usd = CellFormat::Currency { code: "USD".into(), decimals: Some(2), thousands: false, negative: 0, accounting: false };
        d.begin(&mut h, "Format");
        assert_eq!(d.set_cells_format(&mut h, t, (5, 2, 5, 2), &usd).unwrap(), 1);
        d.set_cells_fill(&mut h, t, (6, 2, 6, 2), Some(&FillSpec::Color(crate::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }))).unwrap();
        d.end(&mut h);
        let rec = |d: &Document, r: u32, c: u32| {
            let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
            crate::table::raw_cell_records(&d.file, model).into_iter().find(|x| x.0 == (r, c)).map(|x| x.1)
        };
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        // Keynote: 0500000000000208005000000200000001000000 (key 1 there; ours may differ)
        let e = Record::parse(&rec(&d, 5, 2).unwrap()).unwrap();
        assert_eq!(hex(&e.head), "0500000000000208");
        assert_eq!(e.fields.iter().map(|f| f.0).collect::<Vec<_>>(), [0x1000, 0x4000]);
        assert_eq!(d.cell_format(t, 5, 2), usd);
        d.begin(&mut h, "Type");
        d.set_cell(&mut h, t, 5, 2, &CellInput::Number(5.0)).unwrap();
        d.set_cell(&mut h, t, 6, 2, &CellInput::Number(7.0)).unwrap();
        d.end(&mut h);
        let d = reopen(&d);
        let (_, tb) = table_of(&d);
        assert_eq!(tb.cells[(5 * tb.cols + 2) as usize], "$5.00");
        // the same record as Keynote's 050a...0208 01500000 <5> 02000000 <key>
        let typed = Record::parse(&rec(&d, 5, 2).unwrap()).unwrap();
        let keynote = r33_dir_file("../r34/KN-empty-cell-format-2-typed-5.key");
        let (kt, _) = table_of(&keynote);
        let kmodel = keynote.file.obj(keynote.file.obj(kt).unwrap().ref_id("tableModel").unwrap()).unwrap();
        let kr = Record::parse(&crate::table::raw_cell_records(&keynote.file, kmodel).into_iter().find(|x| x.0 == (5, 2)).unwrap().1).unwrap();
        assert_eq!(typed.head, kr.head);
        assert_eq!(typed.fields.iter().map(|f| f.0).collect::<Vec<_>>(), kr.fields.iter().map(|f| f.0).collect::<Vec<_>>());
        assert_eq!(typed.get(0x1000), kr.get(0x1000));
        // the filled cell kept its fill
        assert!(matches!(tb.cell_styles[(6 * tb.cols + 2) as usize].fill, crate::Fill::Color(c) if c.r > 0.9 && c.g < 0.1));
        assert_eq!(tb.cells[(6 * tb.cols + 2) as usize], "7");
    }

    /// R34: "All borders" on B2:C3 then No Border on C2's right edge, against Keynote's files.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn borders_from_both_sides_like_keynote() {
        let layers = |d: &Document, t: ObjId| {
            let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
            let sc = d.file.obj(model.ref_id("stroke_sidecar").unwrap()).unwrap();
            let mut out = Vec::new();
            for f in ["top_row_stroke_layers", "bottom_row_stroke_layers", "left_column_stroke_layers", "right_column_stroke_layers"] {
                for l in sc.ref_ids(f) {
                    let m = d.file.obj(l).unwrap();
                    let runs: Vec<(i64, u32, bool)> = m
                        .msgs("stroke_runs")
                        .map(|r| (r.i64("origin").unwrap_or(0), r.u32("length").unwrap_or(0), r.msg("stroke").and_then(crate::style::stroke).is_some()))
                        .collect();
                    out.push((f.to_string(), m.u32("row_column_index").unwrap(), runs));
                }
            }
            out
        };
        let mut d = r33("R33-0-before.key");
        let (t, _) = table_of(&d);
        let mut h = History::default();
        let black = StrokeSpec { color: crate::Color::BLACK, width: 1.0, dash: crate::format::Dash::Solid };
        d.begin(&mut h, "Border");
        d.set_cells_border(&mut h, t, (1, 1, 2, 2), BorderEdges::All, Some(&black)).unwrap();
        d.end(&mut h);
        let k1 = r33_dir_file("../r34/KN-borders-1-all-B2C3.key");
        let (kt, _) = table_of(&k1);
        assert_eq!(layers(&d, t), layers(&k1, kt), "All borders");
        d.begin(&mut h, "Border");
        d.set_cells_border(&mut h, t, (1, 2, 1, 2), BorderEdges::Right, None).unwrap();
        d.end(&mut h);
        let k2 = r33_dir_file("../r34/KN-borders-2-C2-right-none.key");
        let (kt2, _) = table_of(&k2);
        assert_eq!(layers(&d, t), layers(&k2, kt2), "C2 right: No Border");
        let d = reopen(&d);
        let (_, tb) = table_of(&d);
        assert!(tb.v_strokes[(tb.cols + 1) as usize + 3].is_none(), "C2's right edge is gone");
        assert!(tb.v_strokes[2 * (tb.cols + 1) as usize + 3].is_some(), "C3's right edge stays");
    }

    fn r33_dir_file(rel: &str) -> Document {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r33").join(rel);
        Document::open(p).unwrap()
    }

    /// R37: an Advanced Gradient and an Image Fill on cell B2, stored as Keynote does.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn advanced_and_image_fills_like_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r37");
        let fill_of = |d: &Document, image: bool| -> Msg {
            let field = if image { "image" } else { "gradient" };
            d.file.objects_of("TST.CellStyleArchive").filter_map(|(_, m)| m.msg("cell_properties").and_then(|p| p.msg("cell_fill")).filter(|f| f.msg(field).is_some()).cloned()).last().unwrap()
        };
        let mut d = Document::open(dir.join("R37-4-image-style6.key")).unwrap();
        let t = d.scene(&d.slides(), 1).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let theirs = Document::open(dir.join("R37-6-cell-imagefill.key")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Fill");
        let (c0, c1) = (Color { r: 0.337592, g: 0.757728, b: 1.0, a: 1.0 }, Color { r: 0.0, g: 0.462459, b: 0.729137, a: 1.0 });
        d.set_cells_fill(&mut h, t, (1, 1, 1, 1), Some(&FillSpec::Advanced { stops: vec![(0.0, c0, 0.5), (1.0, c1, 0.5)], radial: false, angle: 270.0 })).unwrap();
        let reference = Some(Color { r: 0.368627, g: 0.368627, b: 0.368627, a: 1.0 });
        d.set_cells_fill(&mut h, t, (1, 2, 1, 2), Some(&FillSpec::Image { data_id: 15, technique: crate::ImageFillTechnique::Tile, size: Some((300.0, 300.0)), reference, tint: None })).unwrap();
        d.end(&mut h);
        let (og, kg) = (fill_of(&d, false), fill_of(&theirs, false));
        let (og, kg) = (og.msg("gradient").unwrap(), kg.msg("gradient").unwrap());
        assert_eq!(og.bool("advancedGradient"), kg.bool("advancedGradient"));
        assert_eq!(og.enum_name("type"), kg.enum_name("type"));
        assert_eq!(og.msgs("stops").count(), kg.msgs("stops").count());
        let ang = |g: &Msg| g.msg("anglegradient").and_then(|a| a.f32("gradientangle")).unwrap();
        assert!((ang(og) - ang(kg)).abs() < 1e-4);
        let (oi, ki) = (fill_of(&d, true), fill_of(&theirs, true));
        let (oi, ki) = (oi.msg("image").unwrap(), ki.msg("image").unwrap());
        for f in ["technique"] {
            assert_eq!(oi.enum_name(f), ki.enum_name(f));
        }
        assert_eq!(oi.msg("imagedata").and_then(|x| x.u64("identifier")), ki.msg("imagedata").and_then(|x| x.u64("identifier")));
        assert_eq!(oi.msg("fillsize").and_then(|x| x.f32("width")), ki.msg("fillsize").and_then(|x| x.f32("width")));
        assert_eq!(oi.bool("interpretsUntaggedImageDataAsGeneric"), Some(true));
        // the style holding the picture lists it, as Keynote's does
        let holder = |d: &Document| d.file.objects_of("TST.CellStyleArchive").filter(|(_, m)| m.msg("cell_properties").and_then(|p| p.msg("cell_fill")).is_some_and(|f| f.msg("image").is_some())).map(|(id, _)| id).last().unwrap();
        assert!(d.file.archive(holder(&d)).unwrap().data_references(0).contains(&15));
        assert!(theirs.file.archive(holder(&theirs)).unwrap().data_references(0).contains(&15));
        // and it reads back as an advanced gradient / image fill
        let tbl = d.scene(&d.slides(), 1).unwrap().drawables.iter().find(|x| x.id == t).cloned().unwrap();
        let DrawableKind::Table(tb) = &tbl.kind else { panic!() };
        assert!(matches!(tb.cell_styles[(tb.cols + 1) as usize].fill, crate::Fill::Gradient { advanced: true, .. }));
        assert!(matches!(tb.cell_styles[(tb.cols + 2) as usize].fill, crate::Fill::Image { technique: crate::ImageFillTechnique::Tile, .. }));
    }

    /// Spacing and indents on a cell's text, and its Text Inset, read back after a save.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn cell_paragraph_and_inset_round_trip() {
        use crate::format::TextProp;
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R30-formulas-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Text");
        d.set_cell(&mut h, t, 1, 1, &crate::tablecells::CellInput::Text("one\ntwo".into())).unwrap();
        for p in [TextProp::LineSpacing(1.5), TextProp::SpaceBefore(4.0), TextProp::SpaceAfter(6.0), TextProp::FirstIndent(12.0), TextProp::LeftIndent(8.0), TextProp::RightIndent(3.0)] {
            d.set_cells_text_prop(&mut h, t, (1, 1, 1, 1), &p).unwrap();
        }
        d.set_cells_padding(&mut h, t, (1, 1, 1, 1), [10.0, 2.0, 6.0, 1.0]).unwrap();
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d.scene(&d.slides(), 0).unwrap();
        let DrawableKind::Table(tb) = &sc.drawables.iter().find(|x| x.id == t).unwrap().kind else { panic!() };
        let st = &tb.cell_styles[(tb.cols + 1) as usize];
        assert_eq!(
            st.para,
            crate::CellPara { line_spacing: crate::LineSpacing::Relative(1.5), space_before: 4.0, space_after: 6.0, left_indent: 8.0, right_indent: 3.0, first_line_indent: 12.0 }
        );
        assert_eq!(st.padding, [10.0, 2.0, 6.0, 1.0]);
        // a cell left alone keeps the defaults
        assert_eq!(tb.cell_styles[(tb.cols + 2) as usize].para, crate::CellPara::default());
    }
}
