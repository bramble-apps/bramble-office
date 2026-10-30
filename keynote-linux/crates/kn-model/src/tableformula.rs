//! Insert > Formula > Sum / Average / Minimum / Maximum / Count / Product, stored the way
//! Keynote 15.4 does it (Mac round 18, K-sum.key / K-avg.key):
//!
//! - the formula is an entry of the table's FORMULA data list: a colon-tract range relative to
//!   the formula cell (sticky bits false; a single row / column has no range_end) followed by a
//!   function node;
//! - the cell keeps the cached result, the formula key and a number format (flags 0x3201);
//! - the calculation engine: the formula count, and in the table's own formula owner a cell
//!   record and a back dependency at the cell's real coordinates, plus the same in its tiles.
//!
//! Keynote picks the range: the numbers above in the column, else the ones to the left in the
//! row. Results are recalculated whenever a cell of the table changes.

use std::collections::HashMap;

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::table::CellValue;
use crate::tablecells::{list_add, record_formula, CellInput};
use crate::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Sum,
    Average,
    Min,
    Max,
    Count,
    Product,
}

impl Func {
    pub const ALL: [Func; 6] = [Func::Sum, Func::Average, Func::Min, Func::Max, Func::Count, Func::Product];

    /// Keynote's function number (AST_function_node_index), where known.
    pub fn index(self) -> Option<u64> {
        match self {
            Func::Sum => Some(168),
            Func::Average => Some(15),
            Func::Min => Some(88),
            Func::Max => Some(84),
            Func::Count => Some(30),
            Func::Product => Some(113),
        }
    }

    fn from_index(i: u64) -> Option<Func> {
        Func::ALL.into_iter().find(|f| f.index() == Some(i))
    }

    /// The Insert > Formula menu item.
    pub fn menu_name(self) -> &'static str {
        match self {
            Func::Sum => "Sum",
            Func::Average => "Average",
            Func::Min => "Minimum",
            Func::Max => "Maximum",
            Func::Count => "Count",
            Func::Product => "Product",
        }
    }

    pub fn from_menu(name: &str) -> Option<Func> {
        Func::ALL.into_iter().find(|f| f.menu_name() == name)
    }
}

/// Inclusive cell block (row0, col0, row1, col1).
type Block = (u32, u32, u32, u32);

fn number(v: &CellValue) -> Option<f64> {
    match v {
        CellValue::Number(x) | CellValue::Duration(x) => Some(*x),
        _ => None,
    }
}

/// The formula's function and the block it reads, for a formula in (row, col).
fn parse(f: &Msg, row: u32, col: u32) -> Option<(Func, Block)> {
    let nodes: Vec<&Msg> = f.msg("AST_node_array")?.msgs("AST_node").collect();
    let func = nodes.iter().find(|n| n.enum_name("AST_node_type") == Some("FUNCTION_NODE"))?.u64("AST_function_node_index").and_then(Func::from_index)?;
    let ct = nodes.iter().find_map(|n| n.msg("AST_colon_tract"))?;
    let span = |abs: &str, rel: &str, at: u32| -> Option<(u32, u32)> {
        if let Some(r) = ct.msg(abs) {
            let b = r.u32("range_begin")?;
            return Some((b, r.u32("range_end").unwrap_or(b)));
        }
        let r = ct.msg(rel)?;
        let b = r.i64("range_begin")?;
        let e = r.i64("range_end").unwrap_or(b);
        let p = |o: i64| u32::try_from(i64::from(at) + o).ok();
        Some((p(b)?, p(e)?))
    };
    let (r0, r1) = span("absolute_row", "relative_row", row)?;
    let (c0, c1) = span("absolute_column", "relative_column", col)?;
    Some((func, (r0, c0, r1, c1)))
}

/// "B2" -> (row 1, col 1).
fn cell_name(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let col = letters.to_ascii_uppercase().bytes().fold(0u32, |a, c| a * 26 + u32::from(c - b'A' + 1)) - 1;
    let row = digits.parse::<u32>().ok()?.checked_sub(1)?;
    Some((row, col))
}

/// "=SUM(B2:B4)" -> (Sum, block). Also MIN / MAX / AVERAGE / COUNT / PRODUCT, one cell or range.
pub fn parse_formula_text(text: &str) -> Option<(Func, Block)> {
    let t = text.trim().strip_prefix('=')?.trim();
    let open = t.find('(')?;
    let name = t[..open].trim().to_ascii_uppercase();
    let func = match name.as_str() {
        "SUM" => Func::Sum,
        "AVERAGE" | "AVG" => Func::Average,
        "MIN" => Func::Min,
        "MAX" => Func::Max,
        "COUNT" => Func::Count,
        "PRODUCT" => Func::Product,
        _ => return None,
    };
    let inner = t[open + 1..].strip_suffix(')')?.trim();
    let (a, b) = inner.split_once(':').unwrap_or((inner, inner));
    let (r0, c0) = cell_name(a)?;
    let (r1, c1) = cell_name(b)?;
    Some((func, (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))))
}

/// Keynote keeps calculation-engine cell records in row-major order (row, then column; T61).
fn row_major(m: &mut Msg, field: &str) {
    let mut recs: Vec<Msg> = m.msgs(field).cloned().collect();
    recs.sort_by_key(|r| (r.u32("row").unwrap_or(0), r.u32("column").unwrap_or(0)));
    m.remove(field);
    for r in recs {
        m.push_msg(field, r);
    }
}

/// Rows and columns one calculation-engine cell record tile covers (Numbers 15.4, research
/// N-functions.numbers: tile_row_begin 0 / 128 / 256, tile_column_begin 0 / 32 / 64 …).
const RECORD_TILE_ROWS: u32 = 128;
const RECORD_TILE_COLUMNS: u32 = 32;

/// The cell record tile of `owner` that covers cell (row, col), made when missing.
pub(crate) fn cell_record_tile(k: &mut kn_format::KeyFile, deps: ObjId, file: &str, owner: u64, row: u32, col: u32) -> ObjId {
    let (rb, cb) = (u64::from(row / RECORD_TILE_ROWS * RECORD_TILE_ROWS), u64::from(col / RECORD_TILE_COLUMNS * RECORD_TILE_COLUMNS));
    let have = k.obj(deps).and_then(|d| d.msg("tiled_cell_dependencies")).map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default();
    if let Some(t) = have.into_iter().find(|t| {
        k.obj(*t).is_some_and(|m| m.u64("internal_owner_id") == Some(owner) && m.u64("tile_row_begin").unwrap_or(0) == rb && m.u64("tile_column_begin").unwrap_or(0) == cb)
    }) {
        return t;
    }
    let id = k.alloc_ids(1)[0];
    let mut m = Msg::new("TSCE.CellRecordTileArchive");
    m.set_u64("internal_owner_id", owner);
    m.set_u64("tile_column_begin", cb);
    m.set_u64("tile_row_begin", rb);
    k.insert_archive(file, Archive::new_object(id, m));
    if let Some(d) = k.obj_mut(deps) {
        d.msg_mut_or_create("tiled_cell_dependencies").push_ref("cell_record_tiles", id);
    }
    id
}

/// Add (or find) the calculation-engine tile of `owner` in the list `refs_field` of `tiled`.
fn tile(k: &mut kn_format::KeyFile, deps: ObjId, file: &str, cell: bool, owner: u64) -> ObjId {
    let (tiled, list, key) = if cell {
        ("tiled_cell_dependencies", "cell_record_tiles", "internal_owner_id")
    } else {
        ("tiled_range_dependencies", "range_precedents_tile", "to_owner_id")
    };
    let have = k.obj(deps).and_then(|d| d.msg(tiled)).map(|t| t.ref_ids(list)).unwrap_or_default();
    if let Some(t) = have.into_iter().find(|t| k.obj(*t).and_then(|m| m.u64(key)) == Some(owner)) {
        return t;
    }
    let id = k.alloc_ids(1)[0];
    let mut m = Msg::new(if cell { "TSCE.CellRecordTileArchive" } else { "TSCE.RangePrecedentsTileArchive" });
    m.set_u64(key, owner);
    if cell {
        m.set_u64("tile_column_begin", 0);
        m.set_u64("tile_row_begin", 0);
    }
    k.insert_archive(file, Archive::new_object(id, m));
    if let Some(d) = k.obj_mut(deps) {
        d.msg_mut_or_create(tiled).push_ref(list, id);
    }
    id
}

impl Document {
    pub fn cell_values(&self, table: ObjId) -> HashMap<(u32, u32), CellValue> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return HashMap::new() };
        crate::table::cell_records(&self.file, model).into_iter().map(|(rc, r)| (rc, r.value)).collect()
    }

    /// Raw cell records of a table with their FORMULA keys: ((row, col), key).
    pub(crate) fn formula_cells(&self, table: ObjId) -> Vec<((u32, u32), u32)> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return Vec::new() };
        crate::table::raw_cell_records(&self.file, model).into_iter().filter_map(|(rc, rec)| Some((rc, record_formula(&rec)?))).collect()
    }

    pub(crate) fn formula_list(&self, table: ObjId) -> Option<ObjId> {
        let model = self.file.obj(self.file.obj(table)?.ref_id("tableModel")?)?;
        model.msg("base_data_store")?.ref_id("formula_table")
    }

    pub(crate) fn formula_entry(&self, table: ObjId, key: u32) -> Option<&Msg> {
        let list = self.file.obj(self.formula_list(table)?)?;
        list.msgs("entries").find(|e| e.u32("key") == Some(key))?.msg("formula")
    }

    /// The block Keynote's Insert > Formula uses for a formula in (row, col): the body cells
    /// above in the column when any holds a number, else the ones to the left in the row.
    pub fn formula_auto_range(&self, table: ObjId, row: u32, col: u32) -> Option<(u32, u32, u32, u32)> {
        let t = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m))?;
        let values = self.cell_values(table);
        let num = |r: u32, c: u32| values.get(&(r, c)).and_then(number).is_some();
        let top = t.header_rows;
        if row > top && (top..row).any(|r| num(r, col)) {
            return Some((top, col, row - 1, col));
        }
        let left = t.header_cols;
        (col > left).then(|| (row, left, row, col - 1))
    }

    /// The formula in (row, col) as text, e.g. "=SUM(B2:B4)", for the ones we understand.
    pub fn formula_text(&self, table: ObjId, row: u32, col: u32) -> Option<String> {
        let key = self.formula_cells(table).into_iter().find(|(rc, _)| *rc == (row, col))?.1;
        if let Some(t) = self.formula_entry(table, key).and_then(crate::formula::read_nodes).and_then(|n| crate::formula::to_text(&n, row, col)) {
            return Some(t);
        }
        let (func, b) = parse(self.formula_entry(table, key)?, row, col)?;
        let name = |r: u32, c: u32| {
            let mut letters = String::new();
            let mut n = c + 1;
            while n > 0 {
                letters.insert(0, char::from(b'A' + ((n - 1) % 26) as u8));
                n = (n - 1) / 26;
            }
            format!("{letters}{}", r + 1)
        };
        let f = match func {
            Func::Sum => "SUM",
            Func::Average => "AVERAGE",
            Func::Min => "MIN",
            Func::Max => "MAX",
            Func::Count => "COUNT",
            Func::Product => "PRODUCT",
        };
        let range = if (b.0, b.1) == (b.2, b.3) { name(b.0, b.1) } else { format!("{}:{}", name(b.0, b.1), name(b.2, b.3)) };
        Some(format!("={f}({range})"))
    }

    /// Whether Insert > Formula can put a formula in this table.
    pub fn can_insert_formula(&self, table: ObjId) -> bool {
        self.merge_ctx(table).is_some() && self.formula_list(table).is_some()
    }

    /// Insert > Formula > `func` into cell (row, col), over the block Keynote would pick.
    pub fn insert_formula(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, func: Func) -> Result<(), kn_format::Error> {
        let b = self.formula_auto_range(table, row, col).ok_or(kn_format::Error::Invalid("there are no cells above or to the left to use".into()))?;
        self.insert_formula_over(h, table, row, col, func, b)
    }

    /// A typed formula such as "=SUM(B2:B4)", "=average(a5:c5)" or "=MAX(C2)" into (row, col).
    pub fn set_formula_text(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, text: &str) -> Result<(), kn_format::Error> {
        let nodes = crate::formula::parse(text, row, col).map_err(kn_format::Error::Invalid)?;
        let (cells, ranges) = crate::formula::precedents(&nodes, row, col);
        if cells.contains(&(row, col)) || ranges.iter().any(|b| b.0 <= row && row <= b.2 && b.1 <= col && col <= b.3) {
            return Err(kn_format::Error::Invalid("the formula can't refer to its own cell".into()));
        }
        self.insert_formula_nodes(h, table, row, col, &nodes)
    }

    /// The formula `func` over block `b` into cell (row, col).
    pub fn insert_formula_over(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, func: Func, b: (u32, u32, u32, u32)) -> Result<(), kn_format::Error> {
        use crate::formula::{Coord, Node};
        let fi = func.index().ok_or(kn_format::Error::Invalid("this function isn't available yet".into()))?;
        let rel = |v: u32, at: u32| Coord { v: i64::from(v) - i64::from(at), abs: false };
        let nodes = vec![Node::Range { r0: rel(b.0, row), c0: rel(b.1, col), r1: rel(b.2, row), c1: rel(b.3, col) }, Node::Func { index: fi, args: 1 }];
        self.insert_formula_nodes(h, table, row, col, &nodes)
    }

    /// A formula (Keynote's node array) into cell (row, col): its entry, the cell with its
    /// result, and the calculation engine's records (single cells as the cell record's
    /// expanded edges, ranges as range precedents; rounds 18 and 30).
    pub fn insert_formula_nodes(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, nodes: &[crate::formula::Node]) -> Result<(), kn_format::Error> {
        use crate::formula::Value;
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        self.merge_ctx(table).ok_or(err("this table's formulas can't be edited"))?;
        let list = self.formula_list(table).ok_or(err("this table has no formula list"))?;
        // (any row: records past row 128 / column 32 go in the engine tiles covering them,
        // cell_record_tile)
        let value = crate::formula::eval_with(nodes, row, col, &self.cell_values(table), &self.remote_values());

        // the formula entry
        self.touch(h, list);
        let l = self.file.obj_mut(list).ok_or(err("formula list"))?;
        let key = list_add(l, |_| false, |e| crate::formula::write_nodes(nodes, e.msg_mut_or_create("formula")));

        // the cell (drops a formula that was there before)
        let percent = matches!(nodes.last(), Some(crate::formula::Node::Percent));
        let input = match value {
            Value::Num(v) if percent => CellInput::FormulaPercent(v, key),
            Value::Num(v) => CellInput::Formula(v, key),
            Value::Text(t) => CellInput::FormulaText(t, key),
            Value::Bool(b) => CellInput::FormulaText(if b { "TRUE" } else { "FALSE" }.into(), key),
            Value::Cells(_) | Value::Err(_) => CellInput::Formula(0.0, key),
        };
        self.set_cell(h, table, row, col, &input)?;
        self.add_formula_deps(h, table, row, col, nodes)
    }

    /// The calculation engine's records of the formula in (row, col) (the formula count, its
    /// cell record with single-cell precedents as expanded edges, and its range precedents).
    pub(crate) fn add_formula_deps(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, nodes: &[crate::formula::Node]) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let cx = self.merge_ctx(table).ok_or(err("this table's formulas can't be edited"))?;
        let (cells, ranges) = crate::formula::precedents(nodes, row, col);
        let file = self.file.file_of(cx.table_deps).ok_or(err("no calculation engine file"))?.to_string();
        let owner = cx.table_owner;
        self.structural(h, &[cx.engine, cx.table_deps], |k| {
            if let Some(dt) = k.obj_mut(cx.engine).map(|ce| ce.msg_mut_or_create("dependency_tracker")) {
                let n = dt.u64("number_of_formulas").unwrap_or(0);
                dt.set_u64("number_of_formulas", n + 1);
            }
            let ct = cell_record_tile(k, cx.table_deps, &file, owner, row, col);
            let edges = |m: &mut Msg| {
                let e = m.msg_mut_or_create("expanded_edges");
                if !cells.is_empty() {
                    e.set_u64s("edge_without_owner_rows", &cells.iter().map(|c| u64::from(c.0)).collect::<Vec<_>>());
                    e.set_u64s("edge_without_owner_columns", &cells.iter().map(|c| u64::from(c.1)).collect::<Vec<_>>());
                }
            };
            if let Some(m) = k.obj_mut(ct) {
                let rec = m.push_new("cell_records");
                rec.set_u64("column", u64::from(col));
                rec.set_u64("row", u64::from(row));
                edges(rec);
                row_major(m, "cell_records");
            }
            if !ranges.is_empty() {
                let rt = tile(k, cx.table_deps, &file, false, owner);
                if let Some(m) = k.obj_mut(rt) {
                    for b in &ranges {
                        let ft = m.push_new("from_to_range");
                        let from = ft.msg_mut_or_create("from_coord");
                        from.set_u64("column", u64::from(col));
                        from.set_u64("row", u64::from(row));
                        let rr = ft.msg_mut_or_create("refers_to_rect");
                        let o = rr.msg_mut_or_create("origin");
                        o.set_u64("column", u64::from(b.1));
                        o.set_u64("row", u64::from(b.0));
                        let s = rr.msg_mut_or_create("size");
                        if b.3 > b.1 {
                            s.set_u64("num_columns", u64::from(b.3 - b.1 + 1));
                        }
                        if b.2 > b.0 {
                            s.set_u64("num_rows", u64::from(b.2 - b.0 + 1));
                        }
                    }
                }
            }
            if let Some(d) = k.obj_mut(cx.table_deps) {
                let deps = d.msg_mut_or_create("cell_dependencies");
                let rec = deps.push_new("cell_record");
                rec.set_u64("column", u64::from(col));
                rec.set_u64("row", u64::from(row));
                edges(rec);
                row_major(deps, "cell_record");
                for b in &ranges {
                    let back = d.msg_mut_or_create("range_dependencies").push_new("back_dependency");
                    back.set_u64("cell_coord_row", u64::from(row));
                    back.set_u64("cell_coord_column", u64::from(col));
                    let irr = back.msg_mut_or_create("internal_range_reference");
                    irr.set_u64("owner_id", owner);
                    let rg = irr.msg_mut_or_create("range");
                    rg.set_u64("top_left_column", u64::from(b.1));
                    rg.set_u64("top_left_row", u64::from(b.0));
                    rg.set_u64("bottom_right_column", u64::from(b.3));
                    rg.set_u64("bottom_right_row", u64::from(b.2));
                }
            }
            k.refresh_refs(cx.table_deps);
            k.ensure_external_refs(&file);
        });
        Ok(())
    }

    /// Remove the calculation-engine records of the formula that was in (row, col).
    pub(crate) fn drop_formula_deps(&mut self, h: &mut History, table: ObjId, row: u32, col: u32) {
        let Some(cx) = self.merge_ctx(table) else { return };
        let at = |m: &Msg, c: &str, r: &str| m.u32(c) == Some(col) && m.u32(r) == Some(row);
        // the tiles too: one left empty is removed, and undo must bring it back
        let mut touched = vec![cx.engine, cx.table_deps];
        if let Some(d) = self.file.obj(cx.table_deps) {
            touched.extend(d.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default());
            touched.extend(d.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default());
        }
        self.structural(h, &touched, |k| {
            if let Some(dt) = k.obj_mut(cx.engine).map(|ce| ce.msg_mut_or_create("dependency_tracker")) {
                let n = dt.u64("number_of_formulas").unwrap_or(1);
                dt.set_u64("number_of_formulas", n.saturating_sub(1));
            }
            let (cells, ranges) = k
                .obj(cx.table_deps)
                .map(|d| {
                    (
                        d.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default(),
                        d.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let mut gone = Vec::new();
            for t in cells {
                if let Some(m) = k.obj_mut(t) {
                    m.retain_msgs("cell_records", |c| !at(c, "column", "row"));
                    if m.count("cell_records") == 0 {
                        gone.push(t);
                    }
                }
            }
            for t in ranges {
                if let Some(m) = k.obj_mut(t) {
                    m.retain_msgs("from_to_range", |f| !f.msg("from_coord").is_some_and(|c| at(c, "column", "row")));
                    if m.count("from_to_range") == 0 {
                        gone.push(t);
                    }
                }
            }
            if let Some(d) = k.obj_mut(cx.table_deps) {
                if let Some(c) = d.msg_mut("cell_dependencies") {
                    c.retain_msgs("cell_record", |c| !at(c, "column", "row"));
                }
                if let Some(r) = d.msg_mut("range_dependencies") {
                    r.retain_msgs("back_dependency", |b| !at(b, "cell_coord_column", "cell_coord_row"));
                }
                if let Some(t) = d.msg_mut("tiled_cell_dependencies") {
                    t.retain_msgs("cell_record_tiles", |r| !r.u64("identifier").is_some_and(|i| gone.contains(&i)));
                }
                if let Some(t) = d.msg_mut("tiled_range_dependencies") {
                    t.retain_msgs("range_precedents_tile", |r| !r.u64("identifier").is_some_and(|i| gone.contains(&i)));
                }
            }
            for t in &gone {
                k.remove_archive(*t);
            }
            k.refresh_refs(cx.table_deps);
        });
    }

    /// Recalculate the table's formulas we understand and store the new results.
    pub fn recalc_formulas(&mut self, h: &mut History, table: ObjId) {
        // (the document's stored quotes, once)
        let remote = self.remote_values();
        // a few passes so formulas reading other formulas settle
        for _ in 0..4 {
            let values = self.cell_values(table);
            let mut changed = false;
            for ((row, col), key) in self.formula_cells(table) {
                let Some(nodes) = self.formula_entry(table, key).and_then(crate::formula::read_nodes) else { continue };
                let percent = matches!(nodes.last(), Some(crate::formula::Node::Percent));
                let input = match crate::formula::eval_with(&nodes, row, col, &values, &remote) {
                    crate::formula::Value::Num(v) if percent => CellInput::FormulaPercent(v, key),
                    crate::formula::Value::Num(v) => CellInput::Formula(v, key),
                    crate::formula::Value::Text(t) => CellInput::FormulaText(t, key),
                    crate::formula::Value::Bool(b) => CellInput::FormulaText(if b { "TRUE" } else { "FALSE" }.into(), key),
                    _ => continue,
                };
                let same = match (&input, values.get(&(row, col))) {
                    (CellInput::Formula(v, _) | CellInput::FormulaPercent(v, _), Some(CellValue::Number(c))) => (c - v).abs() <= 1e-9 * v.abs().max(1.0),
                    (CellInput::FormulaText(t, _), Some(CellValue::Text(c))) => c == t,
                    _ => false,
                };
                if !same && self.set_cell(h, table, row, col, &input).is_ok() {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    fn first_table(d: &Document) -> ObjId {
        let s = d.slides();
        d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id
    }

    fn refs() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref")
    }

    /// Sum in B5 then Average in C5 give Keynote's formulas, cells and engine records.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn sum_and_average_match_keynote() {
        let dir = refs();
        let mut d = Document::open(dir.join("K-formula-0-before.key")).unwrap();
        let k = Document::open(dir.join("K-avg.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        for (c, f) in [(1, Func::Sum), (2, Func::Average)] {
            d.begin(&mut h, "Formula");
            d.insert_formula(&mut h, t, 4, c, f).unwrap();
            d.end(&mut h);
        }
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let shape = |doc: &Document| {
            let t = first_table(doc);
            let cx = doc.merge_ctx(t).unwrap();
            let list = doc.file.obj(doc.formula_list(t).unwrap()).unwrap();
            let mut out: Vec<String> = list.msgs("entries").map(|e| format!("{:?} {:?}", e.u32("key"), e.msg("formula"))).collect();
            let dm = doc.file.obj(cx.table_deps).unwrap();
            out.push(format!("{:?}", dm.msg("cell_dependencies")));
            out.push(format!("{:?}", dm.msg("range_dependencies")));
            for id in dm.msg("tiled_cell_dependencies").unwrap().ref_ids("cell_record_tiles").into_iter().chain(dm.msg("tiled_range_dependencies").unwrap().ref_ids("range_precedents_tile")) {
                out.push(format!("{:?}", doc.file.obj(id).unwrap()));
            }
            out.push(format!("{:?}", doc.file.obj(cx.engine).unwrap().msg("dependency_tracker").unwrap().u64("number_of_formulas")));
            let mut cells: Vec<_> = doc.formula_cells(t);
            cells.sort();
            out.push(format!("{cells:?}"));
            let sc = doc.scene(&doc.slides(), 0).unwrap();
            if let DrawableKind::Table(tb) = &sc.drawables.iter().find(|x| x.id == t).unwrap().kind {
                out.push(format!("{:?}", tb.cells));
            }
            out
        };
        assert_eq!(shape(&d), shape(&k));
    }

    /// Changing a summed cell updates the result; overwriting the formula removes its records.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn recalc_and_overwrite() {
        let mut d = Document::open(refs().join("K-sum.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Edit");
        d.set_cell(&mut h, t, 1, 1, &CellInput::Number(10.0)).unwrap();
        d.end(&mut h);
        assert_eq!(d.cell_values(t).get(&(4, 1)).and_then(number), Some(15.0));
        let cx = d.merge_ctx(t).unwrap();
        let n0 = d.file.obj(cx.engine).unwrap().msg("dependency_tracker").unwrap().u64("number_of_formulas");
        d.begin(&mut h, "Edit");
        d.set_cell(&mut h, t, 4, 1, &CellInput::Number(1.0)).unwrap();
        d.end(&mut h);
        assert!(d.formula_cells(t).is_empty());
        let dm = d.file.obj(cx.table_deps).unwrap();
        assert_eq!(dm.msg("cell_dependencies").map(|c| c.count("cell_record")), Some(0));
        assert_eq!(d.file.obj(cx.engine).unwrap().msg("dependency_tracker").unwrap().u64("number_of_formulas"), n0.map(|n| n - 1));
        assert!(d.file.obj(d.formula_list(t).unwrap()).unwrap().count("entries") == 0);
    }

    /// The Mac's 12 typed formulas (R30) give Keynote's entries, cell records and engine edges.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn r30_typed_formulas_match_keynote() {
        let dir = refs();
        let mut d = Document::open(dir.join("R30-formulas-0-before.key")).unwrap();
        let k = Document::open(dir.join("R30-formulas.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        let typed = [
            "=B2+B3", "=B2*B3-B4", "=(B2+B3)/B4", "=B2^2", "=SUM(B2:B4)*2", "=ROUND(B2/B3,2)", "=IF(B2>B3,\"yes\",\"no\")", "=B2&\" items\"", "=ABS(-5)", "=AVERAGE(B2,B4)", "=B2%", "=COUNTA(B2:B4)",
        ];
        for (i, f) in typed.iter().enumerate() {
            d.begin(&mut h, "Formula");
            d.set_formula_text(&mut h, t, 1 + i as u32, 2, f).unwrap_or_else(|e| panic!("{f}: {e}"));
            d.end(&mut h);
        }
        d.begin(&mut h, "Formula");
        d.set_formula_text(&mut h, t, 1, 3, "=B2+B3").unwrap();
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let kt = first_table(&k);
        // the same node arrays and results, cell by cell
        for row in 1..=12u32 {
            let ours = d.formula_cells(t).into_iter().find(|(rc, _)| *rc == (row, 2)).map(|(_, key)| d.formula_entry(t, key).and_then(crate::formula::read_nodes)).unwrap();
            let theirs = k.formula_cells(kt).into_iter().find(|(rc, _)| *rc == (row, 2)).map(|(_, key)| k.formula_entry(kt, key).and_then(crate::formula::read_nodes)).unwrap();
            assert_eq!(ours, theirs, "row {row}");
            assert_eq!(d.cell_values(t).get(&(row, 2)), k.cell_values(kt).get(&(row, 2)), "value row {row}");
            assert_eq!(d.formula_text(t, row, 2), k.formula_text(kt, row, 2), "text row {row}");
        }
        // single-cell precedents as the cell records' expanded edges
        let edges = |doc: &Document, tb: ObjId| {
            let cx = doc.merge_ctx(tb).unwrap();
            let dm = doc.file.obj(cx.table_deps).unwrap();
            let mut v: Vec<String> = dm.msg("cell_dependencies").unwrap().msgs("cell_record").map(|c| format!("{:?} {:?} {:?} {:?}", c.u32("row"), c.u32("column"), c.msg("expanded_edges").map(|e| e.u64s("edge_without_owner_rows")), c.msg("expanded_edges").map(|e| e.u64s("edge_without_owner_columns")))).collect();
            v.sort();
            v
        };
        assert_eq!(edges(&d, t), edges(&k, kt));
    }

    /// The Mac's R31 functions: our parse gives Keynote's stored nodes, and the same results.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn r31_functions_match_keynote() {
        let k = Document::open(refs().join("R31-functions.key")).unwrap();
        let t = first_table(&k);
        let values = k.cell_values(t);
        let cases: &[(&str, &str)] = &[
            ("=AND(1,1)", "TRUE"), ("=OR(1,0)", "TRUE"), ("=NOT(1)", "FALSE"), ("=MEDIAN(B2:B4)", "3"), ("=MOD(7,3)", "1"), ("=INT(2.5)", "2"),
            ("=SQRT(4)", "2"), ("=POWER(2,3)", "8"), ("=ROUNDUP(2.1,0)", "3"), ("=ROUNDDOWN(2.9,0)", "2"), ("=CONCATENATE(\"a\",\"b\")", "ab"),
            ("=LEN(\"ab\")", "2"), ("=LEFT(\"abc\",1)", "a"), ("=RIGHT(\"abc\",1)", "c"), ("=MID(\"abc\",2,1)", "b"), ("=UPPER(\"a\")", "A"),
            ("=LOWER(\"A\")", "a"), ("=TRIM(\" a \")", "a"), ("=TODAY()", ""), ("=NOW()", ""), ("=DATE(2026,1,2)", "1/2/26"), ("=YEAR(B2)", ""),
            ("=MONTH(B2)", ""), ("=DAY(B2)", ""), ("=IFERROR(1/0,0)", "0"), ("=SUMIF(B2:B4,\">2\")", "8"), ("=COUNTIF(B2:B4,\">2\")", "2"),
            ("=AVERAGEIF(B2:B4,\">2\")", "4"), ("=VLOOKUP(2,B2:B4,1,FALSE)", "2"), ("=PI()", "3.14159265358979"), ("=RAND()", ""),
            ("=CEILING(2.1,1)", "3"), ("=FLOOR(2.9,1)", "2"), ("=TEXTJOIN(\",\",TRUE,B2:B4)", "2,3,5"), ("=ISBLANK(B2)", "FALSE"),
            ("=SUMPRODUCT(B2:B4,B2:B4)", "38"), ("=STDEV(B2:B4)", "1.52752523165195"), ("=INDEX(B2:B4,2)", "3"), ("=MATCH(3,B2:B4,0)", "2"),
            ("=COUNT(B2:B4)", "3"), ("=MAX(B2:B4)", "5"), ("=MIN(B2:B4)", "2"), ("=PRODUCT(B2:B4)", "30"),
        ];
        for (i, (f, want)) in cases.iter().enumerate() {
            let row = 1 + i as u32;
            let ours = crate::formula::parse(f, row, 2).unwrap_or_else(|e| panic!("{f}: {e}"));
            let key = k.formula_cells(t).into_iter().find(|(rc, _)| *rc == (row, 2)).map(|(_, key)| key).unwrap();
            let theirs = k.formula_entry(t, key).and_then(crate::formula::read_nodes).unwrap();
            assert_eq!(ours, theirs, "{f}");
            if !want.is_empty() {
                let got = match crate::formula::eval(&ours, row, 2, &values) {
                    crate::formula::Value::Num(x) => {
                        let s = format!("{x:.14}");
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                    crate::formula::Value::Text(t) => t,
                    crate::formula::Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
                    other => format!("{other:?}"),
                };
                assert_eq!(&got, want, "{f}");
            }
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn typed_formulas() {
        assert_eq!(parse_formula_text("=SUM(B2:B4)"), Some((Func::Sum, (1, 1, 3, 1))));
        assert_eq!(parse_formula_text(" =average( a5 : c5 ) "), Some((Func::Average, (4, 0, 4, 2))));
        assert_eq!(parse_formula_text("=MAX($C$2)"), Some((Func::Max, (1, 2, 1, 2))));
        assert_eq!(parse_formula_text("=SUM(AA1:AB2)"), Some((Func::Sum, (0, 26, 1, 27))));
        assert_eq!(parse_formula_text("=FOO(A1)"), None);
        let mut d = Document::open(refs().join("K-formula-0-before.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Formula");
        d.set_formula_text(&mut h, t, 4, 3, "=SUM(B2:B4)").unwrap();
        d.end(&mut h);
        assert_eq!(d.cell_values(t).get(&(4, 3)).and_then(number), Some(6.0));
        assert!(d.set_formula_text(&mut h, t, 2, 1, "=SUM(B2:B4)").is_err());
        assert_eq!(d.formula_text(t, 4, 3).as_deref(), Some("=SUM(B2:B4)"));
        let k = Document::open(refs().join("K-avg.key")).unwrap();
        assert_eq!(k.formula_text(first_table(&k), 4, 2).as_deref(), Some("=AVERAGE(A5:B5)"));
    }
}
