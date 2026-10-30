//! Format > Table > Merge Cells / Unmerge Cells, stored the way Keynote 15.4 does it (Mac
//! references R13-merge-*):
//!
//! - the covered cells' text goes into the top-left cell (tabs between columns, newlines between
//!   rows) and the other cells are emptied; unmerging leaves the cells as they are;
//! - one formula per merge in the table model's merge owner: an absolute colon-tract range
//!   followed by function 168;
//! - the calculation engine's bookkeeping for that formula: the formula count, the UUID
//!   reference map, and the merge owner's cell / range dependencies, UUID references and tiles.

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::tablecells::CellInput;
use crate::{CellRange, Document};

const MERGE_FUNCTION: u64 = 168;

/// The calculation-engine cell (column, row) of the merge owner's formula `idx` (Mac round 17).
fn formula_coord(idx: u32) -> (u32, u32) {
    (idx, 0)
}

fn uuid_of(m: Option<&Msg>) -> (u64, u64) {
    m.map(|m| (m.u64("lower").unwrap_or(0), m.u64("upper").unwrap_or(0))).unwrap_or_default()
}

fn cf_uuid_of(m: Option<&Msg>) -> (u64, u64) {
    let w = |n: &str| m.and_then(|m| m.u64(n)).unwrap_or(0);
    (w("uuid_w0") | w("uuid_w1") << 32, w("uuid_w2") | w("uuid_w3") << 32)
}

fn set_uuid(m: &mut Msg, (lo, hi): (u64, u64)) {
    m.set_u64("lower", lo);
    m.set_u64("upper", hi);
}

fn set_cf_uuid(m: &mut Msg, (lo, hi): (u64, u64)) {
    m.set_u64("uuid_w0", lo & 0xffff_ffff);
    m.set_u64("uuid_w1", lo >> 32);
    m.set_u64("uuid_w2", hi & 0xffff_ffff);
    m.set_u64("uuid_w3", hi >> 32);
}

/// Add (column, row) to a TSCE.CellCoordSetArchive.
fn coord_add(set: &mut Msg, (c, r): (u32, u32)) {
    if !set.msgs("column_entries").any(|e| e.u32("column") == Some(c)) {
        set.push_new("column_entries").set_u64("column", c as u64);
    }
    let e = set.msgs_mut("column_entries").find(|e| e.u32("column") == Some(c)).unwrap();
    let rows = e.msg_mut_or_create("row_set");
    if !rows.msgs("entries").any(|x| x.u32("range_begin") == Some(r)) {
        rows.push_new("entries").set_u64("range_begin", r as u64);
    }
}

/// Remove (column, row) from a coordinate set; true when the set is now empty.
fn coord_remove(set: &mut Msg, (c, r): (u32, u32)) -> bool {
    for e in set.msgs_mut("column_entries").filter(|e| e.u32("column") == Some(c)) {
        if let Some(rows) = e.msg_mut("row_set") {
            rows.retain_msgs("entries", |x| x.u32("range_begin") != Some(r) || x.u32("range_end").is_some_and(|e| e != r));
        }
    }
    set.retain_msgs("column_entries", |e| e.msg("row_set").is_some_and(|r| r.count("entries") > 0));
    set.count("column_entries") == 0
}

fn is_coord(m: &Msg, col: &str, row: &str, (c, r): (u32, u32)) -> bool {
    m.u32(col) == Some(c) && m.u32(row) == Some(r)
}

/// What a merge edit needs to know about the table and the calculation engine.
pub(crate) struct MergeCtx {
    pub(crate) model: ObjId,
    pub(crate) engine: ObjId,
    /// The table's own TSCE.FormulaOwnerDependenciesArchive (its cell formulas).
    pub(crate) table_deps: ObjId,
    /// The merge owner's TSCE.FormulaOwnerDependenciesArchive.
    deps: ObjId,
    merge_owner: u64,
    pub(crate) table_owner: u64,
    table_uuid: (u64, u64),
}

/// The merge's range from a merge-owner formula.
/// The cells a merge_owner formula merges: a colon tract, or (older files) two cell
/// references and a colon ("A1:C1"; row and column are sint32).
pub(crate) fn formula_range(f: &Msg) -> Option<CellRange> {
    let nodes: Vec<&Msg> = f.msg("formula")?.msg("AST_node_array")?.msgs("AST_node").collect();
    if let Some(ct) = nodes.iter().find_map(|n| n.msg("AST_colon_tract")) {
        let span = |name: &str| {
            let r = ct.msg(name)?;
            let b = r.u32("range_begin")?;
            Some((b, r.u32("range_end").unwrap_or(b).max(b)))
        };
        let ((r0, r1), (c0, c1)) = (span("absolute_row")?, span("absolute_column")?);
        return Some(CellRange { row: r0, col: c0, rows: r1 - r0 + 1, cols: c1 - c0 + 1 });
    }
    let kind = |n: &&Msg, t: &str| n.enum_name("AST_node_type") == Some(t);
    let refs: Vec<(u32, u32)> = nodes
        .iter()
        .filter(|n| kind(n, "CELL_REFERENCE_NODE"))
        .filter_map(|n| Some((n.msg("AST_row")?.i64("row")?.max(0) as u32, n.msg("AST_column")?.i64("column")?.max(0) as u32)))
        .collect();
    if refs.len() != 2 || !nodes.iter().any(|n| kind(n, "COLON_NODE")) {
        return None;
    }
    let (a, b) = (refs[0], refs[1]);
    Some(CellRange { row: a.0.min(b.0), col: a.1.min(b.1), rows: a.0.abs_diff(b.0) + 1, cols: a.1.abs_diff(b.1) + 1 })
}

impl Document {
    pub(crate) fn merge_ctx(&self, table: ObjId) -> Option<MergeCtx> {
        let model = self.file.obj(table)?.ref_id("tableModel")?;
        let merge_uuid = cf_uuid_of(self.file.obj(model)?.msg("merge_owner")?.msg("owner_id"));
        let (engine, ce) = self.file.first_of("TSCE.CalculationEngineArchive")?;
        let owners = ce.msg("dependency_tracker")?.ref_ids("formula_owner_dependencies");
        let owner = |uuid: (u64, u64)| owners.iter().copied().find(|d| self.file.obj(*d).is_some_and(|m| uuid_of(m.msg("formula_owner_uid")) == uuid));
        let deps = owner(merge_uuid)?;
        let dm = self.file.obj(deps)?;
        let table_uuid = uuid_of(dm.msg("base_owner_uid"));
        let table_deps = owner(table_uuid)?;
        let table_owner = self.file.obj(table_deps)?.u64("internal_formula_owner_id")?;
        Some(MergeCtx { model, engine, table_deps, deps, merge_owner: dm.u64("internal_formula_owner_id")?, table_owner, table_uuid })
    }

    /// Whether Format > Table > Merge Cells can work on this table.
    pub fn can_merge_cells(&self, table: ObjId) -> bool {
        self.merge_ctx(table).is_some()
    }

    /// Merge the block `r` of `table` into one cell.
    /// Set or clear the merge-anchor word (bytes 4..8) of cell (row, col)'s record.
    pub(crate) fn set_merge_anchor_flag(&mut self, h: &mut History, table: ObjId, row: u32, col: u32, on: bool) -> Result<(), kn_format::Error> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).cloned() else { return Ok(()) };
        let raw = crate::table::raw_cell_records(&self.file, &model).into_iter().find(|(rc, _)| *rc == (row, col)).map(|(_, b)| b);
        // only byte 4: bytes 6..8 are the cell's format marker (a formatted anchor would lose
        // its format otherwise)
        if let Some(mut b) = raw.filter(|b| b.first() == Some(&5) && b.len() >= 8) {
            let want = u8::from(on);
            if b[4] != want {
                b[4] = want;
                self.set_cell(h, table, row, col, &CellInput::Raw(b))?;
            }
        }
        Ok(())
    }

    pub fn merge_cells(&mut self, h: &mut History, table: ObjId, r: CellRange) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        if r.rows * r.cols < 2 {
            return Err(err("select two or more cells to merge"));
        }
        let cx = self.merge_ctx(table).ok_or(err("this table's merges can't be edited"))?;
        let t = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)).ok_or(err("not a table"))?;
        if r.row + r.rows > t.rows || r.col + r.cols > t.cols {
            return Err(err("the cells are outside the table"));
        }
        let overlaps = |m: &CellRange| m.row < r.row + r.rows && r.row < m.row + m.rows && m.col < r.col + r.cols && r.col < m.col + m.cols;
        if t.merges.iter().any(overlaps) {
            return Err(err("unmerge the merged cells first"));
        }

        // the text: tabs between columns, newlines between rows (empty cells left out)
        let text = |row: u32, col: u32| t.cells.get((row * t.cols + col) as usize).cloned().unwrap_or_default();
        let lines: Vec<String> = (r.row..r.row + r.rows)
            .map(|row| (r.col..r.col + r.cols).map(|col| text(row, col)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\t"))
            .filter(|l| !l.is_empty())
            .collect();
        let joined = lines.join("\n");
        for row in r.row..r.row + r.rows {
            for col in r.col..r.col + r.cols {
                if (row, col) != (r.row, r.col) && !text(row, col).is_empty() {
                    self.set_cell(h, table, row, col, &CellInput::Empty)?;
                }
            }
        }
        if joined != text(r.row, r.col) {
            self.set_cell(h, table, r.row, r.col, &CellInput::Text(joined))?;
        }
        // the anchor's record marks it as the top-left of a merge (bytes 4..8 = 1), as Keynote's
        // and Numbers' own merges do (R13, N2)
        self.set_merge_anchor_flag(h, table, r.row, r.col, true)?;

        let file = self.file.file_of(cx.deps).ok_or(err("no calculation engine file"))?.to_string();
        self.structural(h, &[cx.model, cx.engine, cx.deps], |k| {
            // the formula: =168(absolute range)
            let Some(mo) = k.obj_mut(cx.model).and_then(|m| m.msg_mut("merge_owner")) else { return };
            let store = mo.msg_mut_or_create("formula_store");
            let idx = store.u32("next_formula_index").unwrap_or(0);
            store.set_u64("next_formula_index", idx as u64 + 1);
            let pair = store.push_new("formulas");
            pair.set_u64("formula_index", idx as u64);
            let nodes = pair.msg_mut_or_create("formula").msg_mut_or_create("AST_node_array");
            let tract = nodes.push_new("AST_node");
            tract.set_enum("AST_node_type", "COLON_TRACT_NODE");
            set_cf_uuid(tract.msg_mut_or_create("AST_cross_table_reference_extra_info").msg_mut_or_create("table_id"), cx.table_uuid);
            let sticky = tract.msg_mut_or_create("AST_sticky_bits");
            for b in ["begin_row_is_absolute", "begin_column_is_absolute", "end_row_is_absolute", "end_column_is_absolute"] {
                sticky.set_bool(b, true);
            }
            let ct = tract.msg_mut_or_create("AST_colon_tract");
            // a single row / column has no range_end
            let c = ct.push_new("absolute_column");
            c.set_u64("range_begin", r.col as u64);
            if r.cols > 1 {
                c.set_u64("range_end", (r.col + r.cols - 1) as u64);
            }
            let rr = ct.push_new("absolute_row");
            rr.set_u64("range_begin", r.row as u64);
            if r.rows > 1 {
                rr.set_u64("range_end", (r.row + r.rows - 1) as u64);
            }
            ct.set_bool("preserve_rectangular", true);
            let f = nodes.push_new("AST_node");
            f.set_enum("AST_node_type", "FUNCTION_NODE");
            f.set_u64("AST_function_node_index", MERGE_FUNCTION);
            f.set_u64("AST_function_node_numArgs", 1);
            let at = formula_coord(idx);

            // the engine: formula count and who refers to the table
            if let Some(ce) = k.obj_mut(cx.engine) {
                let dt = ce.msg_mut_or_create("dependency_tracker");
                let n = dt.u64("number_of_formulas").unwrap_or(0);
                dt.set_u64("number_of_formulas", n + 1);
                let map = ce.msg_mut_or_create("uuid_reference_map");
                if !map.msgs("cell_refs_for_uuid").any(|e| uuid_of(e.msg("uuid")) == cx.table_uuid) {
                    set_uuid(map.push_new("cell_refs_for_uuid").msg_mut_or_create("uuid"), cx.table_uuid);
                }
                let e = map.msgs_mut("cell_refs_for_uuid").find(|e| uuid_of(e.msg("uuid")) == cx.table_uuid).unwrap();
                let refs = e.msg_mut_or_create("cell_refs");
                if !refs.msgs("owner_entries").any(|o| o.u64("owner_id") == Some(cx.merge_owner)) {
                    refs.push_new("owner_entries").set_u64("owner_id", cx.merge_owner);
                }
                let o = refs.msgs_mut("owner_entries").find(|o| o.u64("owner_id") == Some(cx.merge_owner)).unwrap();
                coord_add(o.msg_mut_or_create("coord_set"), at);
            }

            // the merge owner's dependencies, plus its tiles
            let tiles = k.obj(cx.deps).map(|d| {
                let cell = d.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default();
                let range = d.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default();
                (cell.first().copied(), range.into_iter().find(|t| k.obj(*t).and_then(|m| m.u64("to_owner_id")) == Some(cx.table_owner)))
            });
            let (_, range_tile) = tiles.unwrap_or_default();
            // the record's tile covers its (row, column): past 32 merges it's the next one
            let cell_tile = crate::tableformula::cell_record_tile(k, cx.deps, &file, cx.merge_owner, at.1 as u32, at.0 as u32);
            let range_tile = range_tile.unwrap_or_else(|| {
                let id = k.alloc_ids(1)[0];
                let mut m = Msg::new("TSCE.RangePrecedentsTileArchive");
                m.set_u64("to_owner_id", cx.table_owner);
                k.insert_archive(&file, Archive::new_object(id, m));
                if let Some(d) = k.obj_mut(cx.deps) {
                    d.msg_mut_or_create("tiled_range_dependencies").push_ref("range_precedents_tile", id);
                }
                id
            });
            if let Some(m) = k.obj_mut(cell_tile) {
                let rec = m.push_new("cell_records");
                rec.set_u64("column", at.0 as u64);
                rec.set_u64("row", at.1 as u64);
                rec.msg_mut_or_create("expanded_edges");
            }
            if let Some(m) = k.obj_mut(range_tile) {
                let ft = m.push_new("from_to_range");
                let from = ft.msg_mut_or_create("from_coord");
                from.set_u64("column", at.0 as u64);
                from.set_u64("row", at.1 as u64);
                let rect = ft.msg_mut_or_create("refers_to_rect");
                let o = rect.msg_mut_or_create("origin");
                o.set_u64("column", r.col as u64);
                o.set_u64("row", r.row as u64);
                // sizes of 1 are left out
                let s = rect.msg_mut_or_create("size");
                if r.cols > 1 {
                    s.set_u64("num_columns", r.cols as u64);
                }
                if r.rows > 1 {
                    s.set_u64("num_rows", r.rows as u64);
                }
            }
            if let Some(d) = k.obj_mut(cx.deps) {
                let rec = d.msg_mut_or_create("cell_dependencies").push_new("cell_record");
                rec.set_u64("column", at.0 as u64);
                rec.set_u64("row", at.1 as u64);
                rec.msg_mut_or_create("expanded_edges");
                let back = d.msg_mut_or_create("range_dependencies").push_new("back_dependency");
                back.set_u64("cell_coord_row", at.1 as u64);
                back.set_u64("cell_coord_column", at.0 as u64);
                let irr = back.msg_mut_or_create("internal_range_reference");
                irr.set_u64("owner_id", cx.table_owner);
                let rg = irr.msg_mut_or_create("range");
                rg.set_u64("top_left_column", r.col as u64);
                rg.set_u64("top_left_row", r.row as u64);
                rg.set_u64("bottom_right_column", (r.col + r.cols - 1) as u64);
                rg.set_u64("bottom_right_row", (r.row + r.rows - 1) as u64);
                let refs = d.msg_mut_or_create("uuid_references");
                if !refs.msgs("table_refs").any(|t| uuid_of(t.msg("owner_uuid")) == cx.table_uuid) {
                    set_uuid(refs.push_new("table_refs").msg_mut_or_create("owner_uuid"), cx.table_uuid);
                }
                let tr = refs.msgs_mut("table_refs").find(|t| uuid_of(t.msg("owner_uuid")) == cx.table_uuid).unwrap();
                coord_add(tr.msg_mut_or_create("coord_set"), at);
            }
            k.refresh_refs(cx.deps);
            k.ensure_external_refs(&file);
        });
        Ok(())
    }

    /// Unmerge the merged cell covering (row, col). False when it isn't merged.
    pub fn unmerge_cells(&mut self, h: &mut History, table: ObjId, row: u32, col: u32) -> bool {
        let Some(cx) = self.merge_ctx(table) else { return false };
        let Some(idx) = self
            .file
            .obj(cx.model)
            .and_then(|m| m.msg("merge_owner")?.msg("formula_store"))
            .and_then(|s| s.msgs("formulas").find(|f| formula_range(f).is_some_and(|m| m.contains(row, col)))?.u32("formula_index"))
        else {
            return false;
        };
        let at = formula_coord(idx);
        let Some(file) = self.file.file_of(cx.deps).map(str::to_string) else { return false };
        self.structural(h, &[cx.model, cx.engine, cx.deps], |k| {
            if let Some(s) = k.obj_mut(cx.model).and_then(|m| m.msg_mut("merge_owner")).and_then(|m| m.msg_mut("formula_store")) {
                s.retain_msgs("formulas", |f| f.u32("formula_index") != Some(idx));
            }
            if let Some(ce) = k.obj_mut(cx.engine) {
                let dt = ce.msg_mut_or_create("dependency_tracker");
                let n = dt.u64("number_of_formulas").unwrap_or(1);
                dt.set_u64("number_of_formulas", n.saturating_sub(1));
                if let Some(map) = ce.msg_mut("uuid_reference_map") {
                    for e in map.msgs_mut("cell_refs_for_uuid").filter(|e| uuid_of(e.msg("uuid")) == cx.table_uuid) {
                        if let Some(refs) = e.msg_mut("cell_refs") {
                            for o in refs.msgs_mut("owner_entries").filter(|o| o.u64("owner_id") == Some(cx.merge_owner)) {
                                if let Some(set) = o.msg_mut("coord_set") {
                                    coord_remove(set, at);
                                }
                            }
                            refs.retain_msgs("owner_entries", |o| o.msg("coord_set").is_some_and(|s| s.count("column_entries") > 0));
                        }
                    }
                    map.retain_msgs("cell_refs_for_uuid", |e| e.msg("cell_refs").is_some_and(|r| r.count("owner_entries") > 0));
                }
            }
            let (cell_tiles, range_tiles) = k
                .obj(cx.deps)
                .map(|d| {
                    (
                        d.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default(),
                        d.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            let mut gone = Vec::new();
            for t in cell_tiles {
                if let Some(m) = k.obj_mut(t) {
                    m.retain_msgs("cell_records", |c| !is_coord(c, "column", "row", at));
                    if m.count("cell_records") == 0 {
                        gone.push(t);
                    }
                }
            }
            for t in range_tiles {
                if let Some(m) = k.obj_mut(t) {
                    m.retain_msgs("from_to_range", |f| !f.msg("from_coord").is_some_and(|c| is_coord(c, "column", "row", at)));
                    if m.count("from_to_range") == 0 {
                        gone.push(t);
                    }
                }
            }
            if let Some(d) = k.obj_mut(cx.deps) {
                if let Some(c) = d.msg_mut("cell_dependencies") {
                    c.retain_msgs("cell_record", |c| !is_coord(c, "column", "row", at));
                }
                if let Some(r) = d.msg_mut("range_dependencies") {
                    r.retain_msgs("back_dependency", |b| !is_coord(b, "cell_coord_column", "cell_coord_row", at));
                }
                if let Some(refs) = d.msg_mut("uuid_references") {
                    for t in refs.msgs_mut("table_refs") {
                        if let Some(set) = t.msg_mut("coord_set") {
                            coord_remove(set, at);
                        }
                    }
                    refs.retain_msgs("table_refs", |t| t.msg("coord_set").is_some_and(|s| s.count("column_entries") > 0));
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
            k.refresh_refs(cx.deps);
            let _ = file;
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref")
    }

    fn first_table(d: &Document) -> ObjId {
        let s = d.slides();
        d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, crate::DrawableKind::Table(_))).unwrap().id
    }

    /// Merging B2:C3 in Keynote's "before" file gives the same merge, text and engine records as
    /// Keynote's own merged file; unmerging gives Keynote's unmerged records.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn merge_matches_keynote() {
        let dir = refs();
        let mut d = Document::open(dir.join("R13-merge-0-before.key")).unwrap();
        let k = Document::open(dir.join("R13-merge-1-merged.key")).unwrap();
        let ku = Document::open(dir.join("R13-merge-2-unmerged.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        d.begin(&mut h, "Merge");
        d.merge_cells(&mut h, t, CellRange { row: 1, col: 1, rows: 2, cols: 2 }).unwrap();
        d.end(&mut h);
        let shape = |doc: &Document| {
            let s = doc.slides();
            let sc = doc.scene(&s, 0).unwrap();
            let crate::DrawableKind::Table(t) = &sc.drawables.iter().find(|x| x.id == first_table(doc)).unwrap().kind else { panic!() };
            let cx = doc.merge_ctx(first_table(doc)).unwrap();
            let dm = doc.file.obj(cx.deps).unwrap();
            let ce = doc.file.obj(cx.engine).unwrap();
            (
                t.merges.clone(),
                t.cells.clone(),
                ce.msg("dependency_tracker").unwrap().u64("number_of_formulas"),
                dm.msg("cell_dependencies").map(|c| c.count("cell_record")),
                dm.msg("range_dependencies").map(|c| c.count("back_dependency")),
                dm.msg("tiled_cell_dependencies").map(|c| c.ref_ids("cell_record_tiles").len()),
                dm.msg("tiled_range_dependencies").map(|c| c.ref_ids("range_precedents_tile").len()),
                ce.msg("uuid_reference_map").map(|m| m.count("cell_refs_for_uuid")),
            )
        };
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(shape(&d2), shape(&k));
        // the anchor cell's record byte for byte (its merge word, N2)
        let anchor = |doc: &Document| {
            let m = doc.file.obj(doc.file.obj(first_table(doc)).unwrap().ref_id("tableModel").unwrap()).unwrap().clone();
            crate::table::raw_cell_records(&doc.file, &m).into_iter().find(|(rc, _)| *rc == (1, 1)).map(|(_, b)| b)
        };
        assert_eq!(anchor(&d2), anchor(&k));
        d.begin(&mut h, "Unmerge");
        assert!(d.unmerge_cells(&mut h, t, 2, 2));
        d.end(&mut h);
        let mut a = shape(&d);
        let mut b = shape(&ku);
        // Keynote keeps the joined text after unmerging; so do we
        a.1.clear();
        b.1.clear();
        assert_eq!(a, b);
    }

    /// A second merge (A5:B5) next to the first, against Keynote's K-merge2 files (round 17).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn second_merge_matches_keynote() {
        let dir = refs();
        let mut d = Document::open(dir.join("K-merge2-0-before.key")).unwrap();
        let k = Document::open(dir.join("K-merge2-b.key")).unwrap();
        let t = first_table(&d);
        let mut h = History::default();
        for r in [CellRange { row: 1, col: 1, rows: 2, cols: 2 }, CellRange { row: 4, col: 0, rows: 1, cols: 2 }] {
            d.begin(&mut h, "Merge");
            d.merge_cells(&mut h, t, r).unwrap();
            d.end(&mut h);
        }
        let engine = |doc: &Document| {
            let cx = doc.merge_ctx(first_table(doc)).unwrap();
            let mut out = Vec::new();
            for id in [cx.deps, cx.model] {
                let m = doc.file.obj(id).unwrap();
                let pick = if id == cx.model { m.msg("merge_owner").unwrap().clone() } else {
                    let mut c = m.clone();
                    c.remove("tiled_cell_dependencies");
                    c.remove("tiled_range_dependencies");
                    c
                };
                out.push(format!("{pick:?}"));
            }
            let dm = doc.file.obj(cx.deps).unwrap();
            for t in dm.msg("tiled_cell_dependencies").unwrap().ref_ids("cell_record_tiles").into_iter().chain(dm.msg("tiled_range_dependencies").unwrap().ref_ids("range_precedents_tile")) {
                out.push(format!("{:?}", doc.file.obj(t).unwrap()));
            }
            out.push(format!("{:?}", doc.file.obj(cx.engine).unwrap().msg("uuid_reference_map")));
            out
        };
        assert_eq!(engine(&d), engine(&k));
    }
}
