//! Writing table cell values (text and numbers), byte-for-byte the way Keynote 15.4 stores them
//! (worked out from R2-table-values.key, made in Keynote on the Mac):
//!
//! cell record (storage_version 5): [05][type 3=text 2=number][00 00][00 00 00 00][flags u32]
//! then fields in flag-bit order: 0x1 decimal128 (16 bytes), 0x8 string id, 0x200 formula id,
//! 0x1000 "suggest" id (Keynote writes 5 for text, 1 for numbers), 0x2000 number format id,
//! 0x20000 text format id. Ids point into the table's STRING / FORMAT data lists (with
//! refcounts). Rows/columns keep a numberOfCells count in their header buckets.

use kn_format::{Msg, ObjId};

use crate::edit::History;
use crate::Document;

#[derive(Debug, Clone, PartialEq)]
pub enum CellInput {
    Empty,
    Text(String),
    Number(f64),
    /// A formula cell: its cached result and the key of its entry in the table's FORMULA list
    /// (the caller holds that entry's reference).
    Formula(f64, u32),
    /// A formula cell shown as a percentage (a formula ending in %; Keynote gives it a
    /// percent format, T61).
    FormulaPercent(f64, u32),
    /// A formula cell whose result is text (=B2&" items", IF(…,"yes","no")).
    FormulaText(String, u32),
    /// Rich text: the key of its entry in the table's RICH_TEXT_PAYLOAD list (the caller holds
    /// that entry's reference; see [`Document::set_cell_rich`]).
    RichText(u32),
    /// A whole storage_version 5 record holding the same references as the cell's current one
    /// (used to change flags such as conditional highlighting).
    Raw(Vec<u8>),
    /// A date: seconds since 2001-01-01 (as CellValue::Date). The cell keeps a date format it
    /// has; otherwise it gets Numbers' default one, as a typed date does (05-value-date).
    Date(f64),
}

impl CellInput {
    /// What the user typed: a number if it parses as one, otherwise text.
    pub fn parse(s: &str) -> CellInput {
        let t = s.trim();
        if t.is_empty() {
            return CellInput::Empty;
        }
        match t.replace(',', "").parse::<f64>() {
            Ok(v) if v.is_finite() => CellInput::Number(v),
            _ => CellInput::Text(s.to_string()),
        }
    }
}

const TEXT_FORMAT: i64 = 260;
const NUMBER_FORMAT: i64 = 256;
const PERCENT_FORMAT: i64 = 258;
const DATE_FORMAT: i64 = 261;
/// The date format Numbers gives a typed date (05-value-date: "1/5/26 1:00 PM").
const DEFAULT_DATE_PATTERN: &str = "M/d/yy h:mm\u{202f}a";

/// Keynote's decimal128: 112-bit mantissa (7 u16 LE), then u16 = 12352 + 2*exponent (+0x8000 if negative).
fn decimal128(v: f64) -> [u8; 16] {
    let neg = v < 0.0;
    let a = v.abs();
    let mut exp = 0i32;
    let mut m = a;
    for k in 0..=15 {
        let scaled = a * 10f64.powi(k);
        if (scaled - scaled.round()).abs() <= 1e-9 * scaled.abs().max(1.0) {
            exp = -k;
            m = scaled.round();
            break;
        }
    }
    let mant = m as u128;
    let mut out = [0u8; 16];
    for i in 0..7 {
        let w = ((mant >> (16 * i)) & 0xffff) as u16;
        out[2 * i..2 * i + 2].copy_from_slice(&w.to_le_bytes());
    }
    let mut e = (12352 + 2 * exp) as u16;
    if neg {
        e |= 0x8000;
    }
    out[14..16].copy_from_slice(&e.to_le_bytes());
    out
}

/// Ids a cell record refers to: (string id, number format id, text format id).
fn record_ids(rec: &[u8]) -> (Option<u32>, Option<u32>, Option<u32>) {
    let (s, nf, tf, _) = record_ids4(rec);
    (s, nf, tf)
}

/// Field sizes of a storage_version 5 cell record, in flag-bit order.
const FIELDS: [(u32, usize); 18] = [
    (0x1, 16),
    (0x2, 8),
    (0x4, 8),
    (0x8, 4),
    (0x10, 4),
    (0x20, 4),
    (0x40, 4),
    (0x80, 4),
    (0x100, 4),
    (0x200, 4),
    (0x400, 4),
    (0x800, 4),
    (0x1000, 4),
    (0x2000, 4),
    (0x4000, 4),
    (0x8000, 4),
    (0x10000, 4),
    (0x20000, 4),
];

/// A cell record with the u32 fields in `set` added / replaced and the ones in `clear` removed.
pub(crate) fn record_with(rec: &[u8], set: &[(u32, u32)], clear: &[u32]) -> Option<Vec<u8>> {
    let flags = u32::from_le_bytes(rec.get(8..12)?.try_into().ok()?);
    let mut at = 12;
    let mut fields: Vec<(u32, Vec<u8>)> = Vec::new();
    for (bit, size) in FIELDS {
        if flags & bit != 0 {
            fields.push((bit, rec.get(at..at + size)?.to_vec()));
            at += size;
        }
    }
    fields.retain(|(b, _)| !clear.contains(b) && !set.iter().any(|(s, _)| s == b));
    for (b, v) in set {
        fields.push((*b, v.to_le_bytes().to_vec()));
    }
    fields.sort_by_key(|f| f.0);
    let mut out = rec[..8].to_vec();
    out.extend(fields.iter().fold(0u32, |a, f| a | f.0).to_le_bytes());
    for (_, v) in fields {
        out.extend(v);
    }
    Some(out)
}

/// Text alignment of table cells (TSWP TATvalue0..4; Mac round 24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAlign {
    Left,
    Right,
    Center,
    Justify,
    /// Format > Text > Auto-Align Table Cell: numbers right, text left.
    Auto,
}

impl CellAlign {
    fn tat(self) -> &'static str {
        match self {
            CellAlign::Left => "TATvalue0",
            CellAlign::Right => "TATvalue1",
            CellAlign::Center => "TATvalue2",
            CellAlign::Justify => "TATvalue3",
            CellAlign::Auto => "TATvalue4",
        }
    }
}

impl Document {
    /// Set the text alignment of the cells (with values) in block (r0, c0, r1, c1), the way
    /// Keynote does: a variation of the cell's paragraph style carrying the alignment, in the
    /// table's STYLE list, referenced from the cell record (field 0x40). Returns the cells changed.
    pub fn set_cells_alignment(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), align: CellAlign) -> Result<usize, kn_format::Error> {
        self.set_cells_text_style(h, table, block, &|m| {
            m.msg_mut_or_create("char_properties");
            m.msg_mut_or_create("para_properties").set_enum("alignment", align.tat());
        })
    }

    /// Format > Font (Bold, size, colour...) on cells: each cell's own variation of its text
    /// style carries the change, as alignment does.
    pub fn set_cells_text_prop(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), prop: &crate::format::TextProp) -> Result<usize, kn_format::Error> {
        self.set_cells_text_style(h, table, block, &|m| {
            m.msg_mut_or_create("char_properties");
            m.msg_mut_or_create("para_properties");
            crate::format::apply_text_prop(m, prop);
        })
    }

    /// Give the cells (with values) in `block` a variation of their text style changed by
    /// `edit`, in the table's STYLE list, referenced from the cell record (field 0x40).
    fn set_cells_text_style(&mut self, h: &mut History, table: ObjId, block: (u32, u32, u32, u32), edit: &dyn Fn(&mut Msg)) -> Result<usize, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let model_id = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).ok_or(err("not a table"))?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?;
        let list = model.msg("base_data_store").and_then(|d| d.ref_id("styleTable")).ok_or(err("no style list"))?;
        let t = self.file.obj(table).and_then(|m| crate::table::table_shape(&self.file, m)).ok_or(err("not a table"))?;
        let raws: Vec<((u32, u32), Vec<u8>)> = crate::table::raw_cell_records(&self.file, model)
            .into_iter()
            .filter(|((r, c), rec)| (block.0..=block.2).contains(r) && (block.1..=block.3).contains(c) && rec.first() == Some(&5))
            .collect();
        if raws.is_empty() {
            return Err(err("select cells with values first"));
        }
        let styles: std::collections::HashMap<u32, ObjId> = self
            .file
            .obj(list)
            .map(|l| l.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.ref_id("reference")?))).collect())
            .unwrap_or_default();
        let base_of = |r: u32, c: u32| -> Option<ObjId> {
            let field = if r < t.header_rows {
                "header_row_text_style"
            } else if r >= t.rows.saturating_sub(t.footer_rows) {
                "footer_row_text_style"
            } else if c < t.header_cols {
                "header_column_text_style"
            } else {
                "body_text_style"
            };
            model.ref_id(field)
        };
        let plan: Vec<((u32, u32), Vec<u8>, Option<u32>, ObjId)> = raws
            .into_iter()
            .filter_map(|((r, c), rec)| {
                let old = record_field(&rec, 0x40);
                let base = old.and_then(|k| styles.get(&k).copied()).or_else(|| base_of(r, c))?;
                Some(((r, c), rec, old, base))
            })
            .collect();
        // one new variation per base style
        let mut made: Vec<(ObjId, u32)> = Vec::new();
        let sheet = self.file.stylesheet_id();
        let touch: Vec<ObjId> = sheet.into_iter().chain([list]).collect();
        let mut n = 0;
        for ((r, c), rec, old, base) in plan {
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
                        edit(m);
                        kn_format::objects::fix_override_count(m);
                    }
                    self.touch(h, list);
                    let l = self.file.obj_mut(list).ok_or(err("style list"))?;
                    let k = list_add(l, |_| false, |e| e.set_ref("reference", v));
                    self.file.refresh_refs(list);
                    made.push((base, k));
                    k
                }
            };
            if let Some(o) = old {
                if let Some(l) = self.file.obj_mut(list) {
                    list_release(l, o);
                }
            }
            let new = record_with(&rec, &[(0x40, key)], &[]).ok_or(err("cell record"))?;
            self.set_cell(h, table, r, c, &CellInput::Raw(new))?;
            n += 1;
        }
        self.file.refresh_refs(list);
        Ok(n)
    }

    /// Which rule of conditional style set `key` applies to a value: its index, or 15 for none.
    pub(crate) fn conditional_rule_applied(&self, table: ObjId, key: u32, v: &crate::table::CellValue) -> u32 {
        let set = self
            .file
            .obj(table)
            .and_then(|t| t.ref_id("tableModel"))
            .and_then(|m| self.file.obj(m))
            .and_then(|m| m.msg("base_data_store")?.ref_id("conditionalstyletable"))
            .and_then(|l| self.file.obj(l))
            .and_then(|l| l.msgs("entries").find(|e| e.u32("key") == Some(key))?.ref_id("reference"))
            .and_then(|s| self.file.obj(s));
        set.and_then(|s| s.msgs("rules_prepivot").position(|r| r.msg("predicate").and_then(|p| p.msg("formula")).is_some_and(|f| crate::table::predicate_holds(f, v))))
            .map(|i| i as u32)
            .unwrap_or(15)
    }
}

pub(crate) fn decimal128_bytes(v: f64) -> [u8; 16] {
    decimal128(v)
}

/// The conditional style list key of a cell record (field 0x80), if any.
pub(crate) fn record_conditional(rec: &[u8]) -> Option<u32> {
    record_field(rec, 0x80)
}

/// A u32 field (0x8 and up) of a cell record, if present.
pub(crate) fn record_field(rec: &[u8], bit: u32) -> Option<u32> {
    let flags = u32::from_le_bytes(rec.get(8..12)?.try_into().ok()?);
    if flags & bit == 0 {
        return None;
    }
    let at = 12 + FIELDS.iter().take_while(|(b, _)| *b < bit).filter(|(b, _)| flags & b != 0).map(|(_, s)| s).sum::<usize>();
    rec.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// The FORMULA list key of a cell record, if it holds a formula.
pub(crate) fn record_formula(rec: &[u8]) -> Option<u32> {
    record_ids4(rec).3
}

/// (string id, number format id, text format id, formula id).
fn record_ids4(rec: &[u8]) -> (Option<u32>, Option<u32>, Option<u32>, Option<u32>) {
    let rd = |at: usize| rec.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let Some(flags) = rd(8) else { return (None, None, None, None) };
    let mut at = 12;
    let (mut s, mut nf, mut tf, mut fm) = (None, None, None, None);
    for (bit, size) in [
        (0x1, 16),
        (0x2, 8),
        (0x4, 8),
        (0x8, 4),
        (0x10, 4),
        (0x20, 4),
        (0x40, 4),
        (0x80, 4),
        (0x100, 4),
        (0x200, 4),
        (0x400, 4),
        (0x800, 4),
        (0x1000, 4),
        (0x2000, 4),
        (0x4000, 4),
        (0x8000, 4),
        (0x10000, 4),
        (0x20000, 4),
    ] {
        if flags & bit == 0 {
            continue;
        }
        match bit {
            0x8 => s = rd(at),
            0x200 => fm = rd(at),
            0x2000 => nf = rd(at),
            0x20000 => tf = rd(at),
            _ => {}
        }
        at += size;
    }
    (s, nf, tf, fm)
}

/// Add a reference to a list entry (existing with the same content, or new). Returns its key.
pub(crate) fn list_add(list: &mut Msg, matches: impl Fn(&Msg) -> bool, make: impl FnOnce(&mut Msg)) -> u32 {
    for e in list.msgs_mut("entries") {
        if matches(e) {
            let rc = e.u32("refcount").unwrap_or(0);
            e.set_u64("refcount", u64::from(rc + 1));
            return e.u32("key").unwrap_or(0);
        }
    }
    let key = list.u32("nextListID").unwrap_or(1).max(1);
    list.set_u64("nextListID", u64::from(key + 1));
    let mut e = Msg::new("TST.TableDataList.ListEntry");
    e.set_u64("key", u64::from(key));
    e.set_u64("refcount", 1);
    make(&mut e);
    list.push_msg("entries", e);
    key
}

/// Drop a reference; entries reaching zero are removed.
pub(crate) fn list_release(list: &mut Msg, key: u32) {
    let mut gone = false;
    for e in list.msgs_mut("entries") {
        if e.u32("key") == Some(key) {
            let rc = e.u32("refcount").unwrap_or(1).saturating_sub(1);
            e.set_u64("refcount", u64::from(rc));
            gone = rc == 0;
        }
    }
    if gone {
        list.retain_msgs("entries", |e| e.u32("key") != Some(key));
    }
}

fn format_is(e: &Msg, ty: i64) -> bool {
    e.msg("format").and_then(|f| f.i64("format_type")) == Some(ty) && (ty == TEXT_FORMAT || e.msg("format").and_then(|f| f.u32("decimal_places")) == Some(253))
}

impl Document {
    /// Set the value of cell (row, col) of a table (`table` = the TST.TableInfoArchive drawable).
    pub fn set_cell(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, value: &CellInput) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let model_id = self.file.obj(table).and_then(|m| m.ref_id("tableModel")).ok_or(err("not a table"))?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?;
        if row >= model.u32("number_of_rows").unwrap_or(0) || col >= model.u32("number_of_columns").unwrap_or(0) {
            return Err(err("cell outside the table"));
        }
        // tables over 256 rows keep their cells in several tiles (made as rows need them)
        let (tile, r_in_tile) = self.tile_for_row(h, table, row)?;
        let model = self.file.obj(model_id).ok_or(err("no table model"))?;
        let ds = model.msg("base_data_store").ok_or(err("no data store"))?;
        let strings = ds.ref_id("stringTable").ok_or(err("no string table"))?;
        let formats = ds.ref_id("format_table").ok_or(err("no format table"))?;
        let formula_list = ds.ref_id("formula_table");
        let col_bucket = ds.ref_id("columnHeaders");
        let row_buckets = ds.msg("rowHeaders").map(|r| r.ref_ids("buckets")).unwrap_or_default();
        let default_row_h = model.f64("default_row_height").unwrap_or(40.0) as f32;

        for id in [tile, strings, formats].into_iter().chain(col_bucket).chain(row_buckets.iter().copied()) {
            self.touch(h, id);
        }

        // current cells of the row
        let mut cells: Vec<(u32, Vec<u8>)> = Vec::new();
        if let Some(ri) = self.file.obj(tile).and_then(|t| t.msgs("rowInfos").find(|r| r.u32("tile_row_index") == Some(r_in_tile))) {
            if let (Some(buf), Some(offs)) = (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets")) {
                let offs: Vec<(u32, usize)> = offs
                    .chunks_exact(2)
                    .enumerate()
                    .map(|(c, b)| (c as u32, u16::from_le_bytes([b[0], b[1]])))
                    .filter(|(_, o)| *o != 0xffff)
                    .map(|(c, o)| (c, o as usize))
                    .collect();
                for (n, (c, start)) in offs.iter().enumerate() {
                    let end = offs.get(n + 1).map(|x| x.1).unwrap_or(buf.len());
                    cells.push((*c, buf[*start..end.max(*start)].to_vec()));
                }
            }
        }
        let old = cells.iter().position(|(c, _)| *c == col).map(|i| cells.remove(i));
        let had = old.is_some();
        let old_formula = old.as_ref().and_then(|(_, r)| record_formula(r));

        if let Some(l) = formula_list {
            self.touch(h, l);
        }
        // a format the user chose (R34: set on an empty cell too) stays for a typed number
        // and for an emptied cell: (kind, slot, key, marker)
        let chosen: Option<(u32, u32, u32, u16)> = old.as_ref().and_then(|(_, rec)| {
            let marker = u16::from_le_bytes([*rec.get(6)?, *rec.get(7)?]);
            let kind = record_field(rec, 0x1000)?;
            let bit = [0x2000, 0x4000, 0x8000, 0x10000, 0x20000].get(kind.checked_sub(1)? as usize).copied()?;
            (marker != 0).then_some((kind, bit, record_field(rec, bit)?, marker))
        });
        let keep_format = match value {
            CellInput::Number(_) => chosen.filter(|c| c.0 <= 2),
            // (a date keeps a date format: kind 3)
            CellInput::Date(_) => chosen.filter(|c| c.0 == 3),
            CellInput::Empty => chosen,
            _ => None,
        };
        // the cell's own look (fill, text style) stays whatever is typed
        let own_look: Vec<(u32, u32)> = old.as_ref().map(|(_, rec)| [0x20, 0x40].into_iter().filter_map(|b| Some((b, record_field(rec, b)?))).collect()).unwrap_or_default();
        // release what the old value referenced
        if let (Some((_, rec)), false) = (&old, matches!(value, CellInput::Raw(_))) {
            let (s, ..) = record_ids(rec);
            if let (Some(k), Some(l)) = (s, self.file.obj_mut(strings)) {
                list_release(l, k);
            }
            // a rich text cell's payload
            let rich = self.file.obj(model_id).and_then(|m| m.msg("base_data_store")?.ref_id("rich_text_table"));
            if let (Some(k), Some(l)) = (record_field(rec, 0x10), rich) {
                self.touch(h, l);
                if let Some(l) = self.file.obj_mut(l) {
                    list_release(l, k);
                }
            }
            let slots = [0x2000, 0x4000, 0x8000, 0x10000, 0x20000].into_iter().filter_map(|b| record_field(rec, b));
            for k in slots.filter(|k| keep_format.is_none_or(|f| f.2 != *k)) {
                if let Some(l) = self.file.obj_mut(formats) {
                    list_release(l, k);
                }
            }
        }

        // the new record
        let rec: Option<Vec<u8>> = match value {
            CellInput::Empty if keep_format.is_some() || !own_look.is_empty() => {
                let mut r = vec![5, 0, 0, 0, 0, 0];
                let (kind, bit, key, marker) = keep_format.unwrap_or((0, 0, 0, 0));
                r.extend(marker.to_le_bytes());
                r.extend(0u32.to_le_bytes());
                let mut set = Vec::new();
                if keep_format.is_some() {
                    set.extend([(0x1000, kind), (bit, key)]);
                }
                record_with(&r, &set, &[])
            }
            CellInput::Empty => None,
            CellInput::Raw(r) => Some(r.clone()),
            CellInput::Text(t) => {
                let sl = self.file.obj_mut(strings).ok_or(err("string table"))?;
                let sid = list_add(sl, |e| e.str("string") == Some(t.as_str()), |e| e.set_str("string", t));
                let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                let fid = list_add(fl, |e| format_is(e, TEXT_FORMAT), |e| {
                    let mut f = Msg::new("TSK.FormatStructArchive");
                    f.set_u64("format_type", TEXT_FORMAT as u64);
                    e.push_msg("format", f);
                });
                let mut r = vec![5, 3, 0, 0, 0, 0, 0, 0];
                r.extend(0x21008u32.to_le_bytes());
                r.extend(sid.to_le_bytes());
                r.extend(5u32.to_le_bytes());
                r.extend(fid.to_le_bytes());
                Some(r)
            }
            CellInput::RichText(key) => {
                // as Pages writes one: rich text key, text format kind and format
                let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                let fid = list_add(fl, |e| format_is(e, TEXT_FORMAT), |e| {
                    let mut f = Msg::new("TSK.FormatStructArchive");
                    f.set_u64("format_type", TEXT_FORMAT as u64);
                    e.push_msg("format", f);
                });
                let mut r = vec![5, 9, 0, 0, 0, 0, 0, 0];
                r.extend(0x21010u32.to_le_bytes());
                r.extend(key.to_le_bytes());
                r.extend(5u32.to_le_bytes());
                r.extend(fid.to_le_bytes());
                Some(r)
            }
            CellInput::FormulaText(t, key) => {
                // as Keynote 15.4 writes it (round 30): text, formula, text format kind and format
                let sl = self.file.obj_mut(strings).ok_or(err("string table"))?;
                let sid = list_add(sl, |e| e.str("string") == Some(t.as_str()), |e| e.set_str("string", t));
                let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                let fid = list_add(fl, |e| format_is(e, TEXT_FORMAT), |e| {
                    let mut f = Msg::new("TSK.FormatStructArchive");
                    f.set_u64("format_type", TEXT_FORMAT as u64);
                    e.push_msg("format", f);
                });
                let mut r = vec![5, 3, 0, 0, 0, 0, 0, 0];
                r.extend(0x21208u32.to_le_bytes());
                r.extend(sid.to_le_bytes());
                r.extend(key.to_le_bytes());
                r.extend(5u32.to_le_bytes());
                r.extend(fid.to_le_bytes());
                Some(r)
            }
            CellInput::Formula(v, key) | CellInput::FormulaPercent(v, key) => {
                let ty = if matches!(value, CellInput::FormulaPercent(..)) { PERCENT_FORMAT } else { NUMBER_FORMAT };
                let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                let fid = list_add(fl, |e| format_is(e, ty), |e| {
                    let mut f = Msg::new("TSK.FormatStructArchive");
                    f.set_u64("format_type", ty as u64);
                    f.set_u64("decimal_places", 253);
                    f.set_u64("negative_style", 0);
                    f.set_bool("show_thousands_separator", false);
                    e.push_msg("format", f);
                });
                // value, formula, format kind (number), number format (Keynote 15.4, round 18)
                let mut r = vec![5, 2, 0, 0, 0, 0, 0, 0];
                r.extend(0x3201u32.to_le_bytes());
                r.extend(decimal128(*v));
                r.extend(key.to_le_bytes());
                r.extend(1u32.to_le_bytes());
                r.extend(fid.to_le_bytes());
                Some(r)
            }
            CellInput::Number(v) if keep_format.is_some() => {
                let (kind, bit, key, marker) = keep_format.unwrap();
                // as Keynote writes a number typed into a formatted cell (R34: $5.00)
                let mut r = vec![5, if kind == 2 { 10 } else { 2 }, 0, 0, 0, 0];
                r.extend(marker.to_le_bytes());
                r.extend((0x1001u32 | bit).to_le_bytes());
                r.extend(decimal128(*v));
                r.extend(kind.to_le_bytes());
                r.extend(key.to_le_bytes());
                Some(r)
            }
            CellInput::Date(secs) => {
                // the value (field 0x4, cell type 5), format kind 3 and the format in slot
                // 0x8000: Numbers' record for a typed date (05-value-date)
                let base = crate::tablefmt::typed_value_record(&crate::table::CellValue::Date(*secs)).ok_or(err("date record"))?;
                match keep_format {
                    Some((kind, bit, key, marker)) => {
                        let mut r = base;
                        r[6..8].copy_from_slice(&marker.to_le_bytes());
                        record_with(&r, &[(0x1000, kind), (bit, key)], &[])
                    }
                    None => {
                        let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                        let fid = list_add(
                            fl,
                            |e| e.msg("format").is_some_and(|f| f.u32("format_type") == Some(DATE_FORMAT as u32) && f.str("date_time_format") == Some(DEFAULT_DATE_PATTERN)),
                            |e| {
                                let mut f = Msg::new("TSK.FormatStructArchive");
                                f.set_u64("format_type", DATE_FORMAT as u64);
                                f.set_str("date_time_format", DEFAULT_DATE_PATTERN);
                                e.push_msg("format", f);
                            },
                        );
                        record_with(&base, &[(0x1000, 3), (0x8000, fid)], &[])
                    }
                }
            }
            CellInput::Number(v) => {
                let fl = self.file.obj_mut(formats).ok_or(err("format table"))?;
                let fid = list_add(fl, |e| format_is(e, NUMBER_FORMAT), |e| {
                    let mut f = Msg::new("TSK.FormatStructArchive");
                    f.set_u64("format_type", NUMBER_FORMAT as u64);
                    f.set_u64("decimal_places", 253);
                    f.set_u64("negative_style", 0);
                    f.set_bool("show_thousands_separator", false);
                    e.push_msg("format", f);
                });
                let mut r = vec![5, 2, 0, 0, 0, 0, 0, 0];
                r.extend(0x3001u32.to_le_bytes());
                r.extend(decimal128(*v));
                r.extend(1u32.to_le_bytes());
                r.extend(fid.to_le_bytes());
                Some(r)
            }
        };
        let rec = match (rec, value) {
            (Some(r), v) if !own_look.is_empty() && !matches!(v, CellInput::Raw(_)) => record_with(&r, &own_look, &[]),
            (r, _) => r,
        };
        // a highlighted cell keeps its rules (and which one now applies); emptied, it lets go
        let old_cond = old.as_ref().and_then(|(_, r)| record_conditional(r));
        let rec = match (rec, old_cond, value) {
            (Some(r), Some(key), CellInput::Empty | CellInput::Text(_) | CellInput::Number(_) | CellInput::Date(_) | CellInput::Formula(..) | CellInput::FormulaPercent(..) | CellInput::FormulaText(..)) => {
                let v = match value {
                    CellInput::Date(x) => crate::table::CellValue::Date(*x),
                    CellInput::Text(t) | CellInput::FormulaText(t, _) => crate::table::CellValue::Text(t.clone()),
                    CellInput::Number(x) | CellInput::Formula(x, _) | CellInput::FormulaPercent(x, _) => crate::table::CellValue::Number(*x),
                    _ => crate::table::CellValue::Empty,
                };
                let applied = self.conditional_rule_applied(table, key, &v);
                record_with(&r, &[(0x80, key), (0x100, applied)], &[])
            }
            (None, Some(key), _) => {
                let list = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).and_then(|m| m.msg("base_data_store")?.ref_id("conditionalstyletable"));
                if let Some(l) = list {
                    self.touch(h, l);
                    if let Some(l) = self.file.obj_mut(l) {
                        list_release(l, key);
                    }
                }
                None
            }
            (r, ..) => r,
        };
        let has = rec.is_some();
        if let Some(r) = rec {
            cells.push((col, r));
        }
        cells.sort_by_key(|c| c.0);

        // write the row back (new format + Keynote's 12-byte legacy stubs)
        let t = self.file.obj_mut(tile).ok_or(err("tile"))?;
        t.retain_msgs("rowInfos", |r| r.u32("tile_row_index") != Some(r_in_tile));
        if !cells.is_empty() {
            let mut buf = Vec::new();
            let mut offs = vec![0xffu8; 510];
            let mut legacy = Vec::new();
            let mut loffs = vec![0xffu8; 510];
            for (c, r) in &cells {
                let c = *c as usize;
                offs[2 * c..2 * c + 2].copy_from_slice(&(buf.len() as u16).to_le_bytes());
                buf.extend(r);
                loffs[2 * c..2 * c + 2].copy_from_slice(&(legacy.len() as u16).to_le_bytes());
                legacy.extend([4u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            }
            let mut ri = Msg::new("TST.TileRowInfo");
            ri.set_u64("tile_row_index", u64::from(r_in_tile));
            ri.set_u64("cell_count", cells.len() as u64);
            ri.set_bytes("cell_storage_buffer_pre_bnc", &legacy);
            ri.set_bytes("cell_offsets_pre_bnc", &loffs);
            ri.set_u64("storage_version", 5);
            ri.set_bytes("cell_storage_buffer", &buf);
            ri.set_bytes("cell_offsets", &offs);
            let at = t.msgs("rowInfos").take_while(|r| r.u32("tile_row_index").unwrap_or(0) < r_in_tile).count();
            t.insert_msg("rowInfos", at, ri);
        }
        // the number of rows holding cells (Keynote, T61), not the last row + 1
        let rows_with_cells = t.count("rowInfos");
        t.set_u64("numrows", rows_with_cells as u64);
        t.set_u64("storage_version", 5);
        t.set_bool("last_saved_in_BNC", true);

        // per-row / per-column cell counts
        let delta: i64 = i64::from(has) - i64::from(had);
        if delta != 0 {
            let bump = |k: &mut kn_format::KeyFile, bucket: ObjId, index: u32| {
                let Some(b) = k.obj_mut(bucket) else { return false };
                if let Some(hd) = b.msgs_mut("headers").find(|x| x.u32("index") == Some(index)) {
                    let n = (i64::from(hd.u32("numberOfCells").unwrap_or(0)) + delta).max(0);
                    hd.set_u64("numberOfCells", n as u64);
                    return true;
                }
                false
            };
            if let Some(cb) = col_bucket {
                bump(&mut self.file, cb, col);
            }
            let found = row_buckets.iter().any(|b| bump(&mut self.file, *b, row));
            if !found && delta > 0 {
                if let Some(b) = row_buckets.first().and_then(|b| self.file.obj_mut(*b)) {
                    let mut hd = Msg::new("TST.HeaderStorageBucket.Header");
                    hd.set_u64("index", u64::from(row));
                    hd.set_f32("size", default_row_h);
                    hd.set_u64("hidingState", 0);
                    hd.set_u64("numberOfCells", 1);
                    let at = b.msgs("headers").take_while(|x| x.u32("index").unwrap_or(0) < row).count();
                    b.insert_msg("headers", at, hd);
                }
            }
        }
        // a formula replaced by something else: drop it and its calculation-engine records
        if let Some(fk) = old_formula {
            let kept = match value {
                CellInput::Formula(_, k) | CellInput::FormulaPercent(_, k) | CellInput::FormulaText(_, k) => *k == fk,
                CellInput::Raw(r) => record_formula(r) == Some(fk),
                _ => false,
            };
            if !kept {
                if let Some(l) = formula_list.and_then(|l| self.file.obj_mut(l)) {
                    list_release(l, fk);
                }
                self.drop_formula_deps(h, table, row, col);
            }
        }
        // results of formulas that read this cell
        if !matches!(value, CellInput::Formula(..) | CellInput::FormulaPercent(..) | CellInput::FormulaText(..) | CellInput::Raw(_)) {
            self.recalc_formulas(h, table);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    fn table_of(d: &Document) -> (usize, ObjId) {
        let s = d.slides();
        (0..s.len())
            .find_map(|i| d.scene(&s, i).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).map(|x| (i, x.id)))
            .unwrap()
    }

    fn cells(d: &Document, i: usize, id: ObjId) -> Vec<String> {
        let sc = d.scene(&d.slides(), i).unwrap();
        match &sc.drawables.iter().find(|x| x.id == id).unwrap().kind {
            DrawableKind::Table(t) => t.cells.clone(),
            _ => unreachable!(),
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn dates_round_trip() {
        // a date cell: CellValue::Date back, the default date format, saved and reopened; Text
        // format on it doesn't fail; a number typed over it, a date over a dated cell
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key");
        let mut d = Document::open(p).unwrap();
        let (i, t) = table_of(&d);
        let mut h = History::default();
        // 2026-01-05 13:00 (seconds since 2001)
        let secs = (9500.0 * 86400.0) + 13.0 * 3600.0;
        d.begin(&mut h, "Date");
        d.set_cell(&mut h, t, 1, 1, &CellInput::Date(secs)).unwrap();
        d.end(&mut h);
        assert_eq!(d.cell_values(t).get(&(1, 1)), Some(&crate::table::CellValue::Date(secs)));
        let raw = |d: &Document| {
            let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
            crate::table::raw_cell_records(&d.file, &model).into_iter().find(|(rc, _)| *rc == (1, 1)).map(|x| x.1).unwrap()
        };
        let r = raw(&d);
        assert_eq!(&r[..2], &[5, 5]);
        assert_eq!(record_field(&r, 0x1000), Some(3), "format kind: date");
        assert!(record_field(&r, 0x8000).is_some(), "the date format slot");
        let shown = cells(&d, i, t)[(1 * 2 + 1) as usize].clone();
        assert!(shown.starts_with("1/5/27") || shown.contains("1:00"), "{shown}");
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d2.cell_values(t).get(&(1, 1)), Some(&crate::table::CellValue::Date(secs)));
        // the same date format entry is shared by a second date
        d.begin(&mut h, "Date");
        d.set_cell(&mut h, t, 0, 1, &CellInput::Date(secs + 86400.0)).unwrap();
        d.end(&mut h);
        assert_eq!(record_field(&raw(&d), 0x8000), {
            let model = d.file.obj(d.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
            crate::table::raw_cell_records(&d.file, &model).into_iter().find(|(rc, _)| *rc == (0, 1)).and_then(|x| record_field(&x.1, 0x8000))
        });
        d.begin(&mut h, "Text");
        let _ = d.set_cells_format(&mut h, t, (1, 1, 1, 1), &crate::tablefmt::CellFormat::Text);
        d.end(&mut h);
        d.begin(&mut h, "Number");
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(4.0)).unwrap();
        d.end(&mut h);
        assert_eq!(d.cell_values(t).get(&(1, 1)), Some(&crate::table::CellValue::Number(4.0)));
        while d.undo(&mut h) {}
        assert!(d.cell_values(t).get(&(1, 1)).is_none_or(|v| *v != crate::table::CellValue::Date(secs)));
    }

    #[test]
    fn decimal128_matches_keynote() {
        // bytes Keynote wrote for 12 and 7.5 (R2-table-values.key)
        assert_eq!(&decimal128(12.0)[..], &[0x0c, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0x30]);
        assert_eq!(&decimal128(7.5)[..], &[0x4b, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3e, 0x30]);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn typing_into_a_fresh_table_matches_keynote() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/05-table.key");
        let mut d = Document::open(p).unwrap();
        let (i, t) = table_of(&d);
        let mut h = History::default();
        d.begin(&mut h, "Cells");
        d.set_cell(&mut h, t, 0, 0, &CellInput::Text("Name".into())).unwrap();
        d.set_cell(&mut h, t, 0, 1, &CellInput::Text("Score".into())).unwrap();
        d.set_cell(&mut h, t, 1, 0, &CellInput::Text("Ann".into())).unwrap();
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(12.0)).unwrap();
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let c = cells(&d2, i, t);
        let cols = 2;
        assert_eq!(c[0], "Name");
        assert_eq!(c[1], "Score");
        assert_eq!(c[cols], "Ann");
        assert_eq!(c[cols + 1], "12");
        // overwrite and clear
        d.begin(&mut h, "Cells");
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(7.5)).unwrap();
        d.set_cell(&mut h, t, 0, 1, &CellInput::Empty).unwrap();
        d.end(&mut h);
        let c = cells(&d, i, t);
        assert_eq!(c[cols + 1], "7.5");
        assert_eq!(c[1], "");
        assert!(d.undo(&mut h));
        assert_eq!(cells(&d, i, t)[cols + 1], "12");
    }

    /// Align Left then Auto-Align Table Cell on B2 matches Keynote (Mac round 24).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn cell_alignment_matches_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("K-align-0-before.key")).unwrap();
        let (_, t) = table_of(&d);
        let mut h = History::default();
        let shape = |doc: &Document| {
            let (_, t) = table_of(doc);
            let model = doc.file.obj(doc.file.obj(t).unwrap().ref_id("tableModel").unwrap()).unwrap();
            let rec = crate::table::raw_cell_records(&doc.file, model).into_iter().find(|(rc, _)| *rc == (1, 1)).unwrap().1;
            let list = doc.file.obj(model.msg("base_data_store").unwrap().ref_id("styleTable").unwrap()).unwrap();
            let entry = list.msgs("entries").find(|e| Some(e.u32("key").unwrap()) == record_field(&rec, 0x40)).unwrap();
            let st = doc.file.obj(entry.ref_id("reference").unwrap()).unwrap();
            let tss = st.find_super("TSS.StyleArchive").unwrap();
            (
                rec.len(),
                record_field(&rec, 0x40).is_some(),
                entry.u32("refcount"),
                st.msg("para_properties").and_then(|p| p.enum_name("alignment")).map(str::to_string),
                tss.bool("is_variation"),
                tss.ref_id("parent").and_then(|p| doc.file.obj(p)).and_then(|p| p.find_super("TSS.StyleArchive")?.str("name").map(str::to_string)),
            )
        };
        for (align, file) in [(CellAlign::Left, "K-align-left.key"), (CellAlign::Auto, "K-align-auto.key")] {
            d.begin(&mut h, "Align");
            assert_eq!(d.set_cells_alignment(&mut h, t, (1, 1, 1, 1), align).unwrap(), 1);
            d.end(&mut h);
            let k = Document::open(dir.join(file)).unwrap();
            assert_eq!(shape(&d), shape(&k), "{file}");
        }
    }
}
