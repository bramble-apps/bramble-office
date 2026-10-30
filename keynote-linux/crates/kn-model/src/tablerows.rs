//! Format > Table > Add Row Above / Below, Add Column Before / After, Delete Row / Column.
//!
//! No Keynote before/after pair exists for these yet; the structure is derived from the tables
//! Keynote made (05-table.key, R2-table-values.key, the feature catalog's six tables). A row or
//! column lives in all of these, each updated here:
//! * the table model (number_of_rows / columns, header / footer counts) and the drawable's
//!   frame, whose size is the sum of the row heights / column widths;
//! * the cell tile (rows by tile_row_index; columns by the 255-slot cell offset tables, in both
//!   the storage_version 5 and the pre-BNC buffers) and the header storage buckets (size,
//!   hidingState, numberOfCells per row / column);
//! * the column/row uid map: one uuid per row / column, kept sorted by (upper, lower) with the
//!   two index permutations;
//! * the stroke sidecar (row / column counts and per-edge stroke layers) and merges (the merge
//!   owner's colon-tract formulas, its range back dependency and range precedents tile);
//! * the calculation engine: the table owner's spanning ranges (whole table and body), and
//!   the header name manager: the uids of the header rows / columns, one tracked absolute
//!   reference per header cell (every header-row cell right of the header columns and every
//!   header-column cell below the header rows) and the uses of name fragments (by uid).
//!
//! Tables this can't handle yet are refused, see [`Document::table_edit_blocker`].

use std::collections::{HashMap, HashSet};

use kn_format::tables::{family, uuid_of_cf, uuid_of_str, uuid_of_tsp, Uuid16};
use kn_format::{KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// What happens along the axis: a new row/column at this index, or one removed.
#[derive(Debug, Clone, Copy)]
enum Op {
    Insert(u32),
    Delete(u32),
}

impl Op {
    /// Where an existing row/column ends up (None: deleted).
    fn index(self, i: u32) -> Option<u32> {
        match self {
            Op::Insert(at) => Some(i + u32::from(i >= at)),
            Op::Delete(at) if i == at => None,
            Op::Delete(at) => Some(i - u32::from(i > at)),
        }
    }

    /// An inclusive span [b, e]: grows when a row is inserted inside it, shrinks on delete.
    fn span(self, b: u32, e: u32) -> Option<(u32, u32)> {
        match self {
            Op::Insert(at) => Some((b + u32::from(b >= at), e + u32::from(e >= at))),
            Op::Delete(at) if b == at && e == at => None,
            Op::Delete(at) => Some((b - u32::from(b > at), e - u32::from(e >= at))),
        }
    }
}

/// Coordinate field names of an axis in TSCE / TST messages.
struct Names {
    tl: &'static str,
    br: &'static str,
    coord: &'static str,
    count: &'static str,
    abs: &'static str,
}

fn names(axis: Axis) -> Names {
    match axis {
        Axis::Row => Names { tl: "top_left_row", br: "bottom_right_row", coord: "row", count: "num_rows", abs: "absolute_row" },
        Axis::Column => Names {
            tl: "top_left_column",
            br: "bottom_right_column",
            coord: "column",
            count: "num_columns",
            abs: "absolute_column",
        },
    }
}

const SENTINEL_COLUMN: u64 = 32767;
const MAX_COLUMNS: u32 = 255;
/// Numbers' row limit; past 256 rows the cells take more tiles (tabletiles.rs).
const MAX_ROWS: u32 = 1_000_000;

/// The objects one table's rows and columns live in.
struct Parts {
    info: ObjId,
    model: ObjId,
    tile: ObjId,
    tile_size: u32,
    row_bucket: Option<ObjId>,
    col_bucket: Option<ObjId>,
    strings: Option<ObjId>,
    styles: Option<ObjId>,
    formats: Option<ObjId>,
    uid_map: ObjId,
    sidecar: Option<ObjId>,
    layers: Vec<ObjId>,
    uuid: Uuid16,
    /// The table's formula owners (uuid family of the table).
    fods: Vec<ObjId>,
    /// Internal id of the table's cell owner (owner kind 1).
    cell_owner: Option<u64>,
    merge_fod: Option<ObjId>,
    merge_tiles: Vec<ObjId>,
    hnm: Option<ObjId>,
    frag_tiles: Vec<ObjId>,
    tracked: Option<ObjId>,
    nrm_fod: Option<ObjId>,
    nrm_tiles: Vec<ObjId>,
}

impl Parts {
    fn touched(&self) -> Vec<ObjId> {
        let mut v = vec![self.info, self.model, self.tile, self.uid_map];
        v.extend(self.row_bucket);
        v.extend(self.col_bucket);
        v.extend(self.strings);
        v.extend(self.styles);
        v.extend(self.formats);
        v.extend(self.sidecar);
        v.extend(&self.layers);
        v.extend(&self.fods);
        v.extend(&self.merge_tiles);
        v.extend(self.hnm);
        v.extend(&self.frag_tiles);
        v.extend(self.tracked);
        v.extend(self.nrm_fod);
        v.extend(&self.nrm_tiles);
        v
    }
}

fn list_entries(k: &KeyFile, id: Option<ObjId>) -> usize {
    id.and_then(|i| k.obj(i)).map(|m| m.count("entries") + m.count("segments")).unwrap_or(0)
}

fn uid_map_has_uids(k: &KeyFile, id: Option<ObjId>) -> bool {
    id.and_then(|i| k.obj(i)).is_some_and(|m| m.count("sorted_column_uids") + m.count("sorted_row_uids") > 0)
}

fn fod_tiles(m: &Msg) -> Vec<ObjId> {
    m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default()
}

fn fod_range_tiles(m: &Msg) -> Vec<ObjId> {
    m.msg("tiled_range_dependencies").map(|t| t.ref_ids("range_precedents_tile")).unwrap_or_default()
}

/// Cell records of a formula owner (expanded + tiled).
fn fod_records<'a>(k: &'a KeyFile, m: &'a Msg) -> Vec<&'a Msg> {
    let mut v: Vec<&Msg> = m.msg("cell_dependencies").map(|c| c.msgs("cell_record").collect()).unwrap_or_default();
    for t in fod_tiles(m) {
        if let Some(tm) = k.obj(t) {
            v.extend(tm.msgs("cell_records"));
        }
    }
    v
}

fn edges_to(rec: &Msg, owner: u64) -> bool {
    rec.msg("expanded_edges").is_some_and(|e| e.u64s("internal_owner_id_for_edge").contains(&owner))
}

/// The table's own tracked header-cell reference? (CELL_REFERENCE_NODE into this table.)
fn tracked_cell(r: &Msg, table: &Uuid16) -> Option<(u32, u32)> {
    let n = r.msg("ast")?.msgs("AST_node").next()?;
    let t = n.msg("AST_cross_table_reference_extra_info")?.msg("table_id").map(uuid_of_cf)?;
    if &t != table {
        return None;
    }
    let row = n.msg("AST_row")?.i64("row")?;
    let col = n.msg("AST_column")?.i64("column")?;
    Some((row.max(0) as u32, col.max(0) as u32))
}

fn parts(k: &KeyFile, table: ObjId) -> Result<Parts, String> {
    let info = k.obj(table).ok_or("no table")?;
    let model_id = info.ref_id("tableModel").ok_or("not a table")?;
    let model = k.obj(model_id).ok_or("no table model")?;
    let ds = model.msg("base_data_store").ok_or("no data store")?;
    let tiles = ds.msg("tiles").ok_or("no cell tiles")?;
    // row and column edits work on tile 0: the table's other tiles are gathered into it first
    // (Document::gather_tiles, table_rows_edit)
    let tile_list: Vec<&Msg> = tiles.msgs("tiles").collect();
    let first = tile_list.iter().find(|t| t.u32("tileid") == Some(0)).ok_or("no first cell tile")?;
    let tile = first.ref_id("tile").ok_or("no cell tile")?;
    if k.obj(tile).is_some_and(|t| t.msgs("rowInfos").any(|r| r.bool("has_wide_offsets") == Some(true))) {
        return Err("tables with very large rows aren't supported yet".into());
    }
    let row_buckets = ds.msg("rowHeaders").map(|r| r.ref_ids("buckets")).unwrap_or_default();
    if row_buckets.len() > 1 {
        return Err("tables with more than one row bucket aren't supported yet".into());
    }
    if list_entries(k, ds.ref_id("conditionalstyletable")) > 0 {
        return Err("tables with conditional highlighting aren't supported yet".into());
    }
    if ds.ref_id("merge_region_map").and_then(|i| k.obj(i)).is_some_and(|m| m.count("cell_range") > 0) {
        return Err("tables with old-style merged cells aren't supported yet".into());
    }
    let col_bucket = ds.ref_id("columnHeaders");
    let hidden = row_buckets.iter().copied().chain(col_bucket).filter_map(|b| k.obj(b)).any(|b| b.msgs("headers").any(|h| h.u32("hidingState").unwrap_or(0) != 0));
    let hidden_counts = ["number_of_hidden_rows", "number_of_hidden_columns", "number_of_user_hidden_rows", "number_of_user_hidden_columns", "number_of_filtered_rows"]
        .iter()
        .any(|f| model.u32(f).unwrap_or(0) > 0);
    if hidden || hidden_counts {
        return Err("tables with hidden rows or columns aren't supported yet".into());
    }
    let summary_uids = info.ref_id("summary_model").and_then(|s| k.obj(s)).and_then(|s| s.ref_id("column_row_uids"));
    let category_uids = info.ref_id("category_order").and_then(|s| k.obj(s)).and_then(|s| s.ref_id("uid_map"));
    if uid_map_has_uids(k, summary_uids) || uid_map_has_uids(k, category_uids) {
        return Err("tables with categories aren't supported yet".into());
    }
    let sort_refs = model.msg("sort_rule_reference_tracker").and_then(|s| s.ref_id("reference_tracker")).and_then(|i| k.obj(i));
    if sort_refs.is_some_and(|s| s.count("contained_tracked_reference") + s.count("tracked_reference") + s.count("contained_expanded_tracked_reference") > 0) {
        return Err("tables with sort rules aren't supported yet".into());
    }
    let uid_map = model.ref_id("base_column_row_uids").ok_or("table has no row/column uids")?;
    let um = k.obj(uid_map).ok_or("table has no row/column uids")?;
    if um.count("sorted_row_uids") != model.u32("number_of_rows").unwrap_or(0) as usize
        || um.count("sorted_column_uids") != model.u32("number_of_columns").unwrap_or(0) as usize
    {
        return Err("the table's row/column uids don't match its size".into());
    }
    let uuid = model.str("table_id").and_then(uuid_of_str).ok_or("table has no id")?;
    let fam = family(&uuid);

    let sidecar = model.ref_id("stroke_sidecar");
    let layers = sidecar
        .and_then(|s| k.obj(s))
        .map(|s| {
            ["left_column_stroke_layers", "right_column_stroke_layers", "top_row_stroke_layers", "bottom_row_stroke_layers"]
                .iter()
                .flat_map(|f| s.ref_ids(f))
                .collect()
        })
        .unwrap_or_default();

    // calculation engine
    let ce = k.first_of("TSCE.CalculationEngineArchive").map(|(i, _)| i);
    let cem = ce.and_then(|c| k.obj(c));
    let all_fods: Vec<ObjId> = cem.and_then(|c| c.msg("dependency_tracker")).map(|t| t.ref_ids("formula_owner_dependencies")).unwrap_or_default();
    let fods: Vec<ObjId> = all_fods
        .iter()
        .copied()
        .filter(|f| fam.is_some() && k.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(|u| family(&uuid_of_tsp(u))) == Some(fam))
        .collect();
    let cell_fod = fods.iter().copied().find(|f| k.obj(*f).and_then(|m| m.u64("owner_kind")) == Some(1));
    let cell_owner = cell_fod.and_then(|f| k.obj(f)?.u64("internal_formula_owner_id"));
    // (formula records are taken out before rows / columns change and made again after:
    // Document::table_rows_edit)
    // Keynote hangs reading cells of a table whose rows/columns we changed around a merged cell
    // (Mac round 8): refused until the merge update is fixed
    if model.msg("merge_owner").and_then(|m| m.msg("formula_store")).is_some_and(|fs| fs.count("formulas") > 0) {
        return Err("tables with merged cells can't have rows or columns added or removed yet".into());
    }
    let merge_uid = model.msg("merge_owner").and_then(|m| m.msg("owner_id")).map(uuid_of_cf);
    let merge_fod = fods.iter().copied().find(|f| merge_uid.is_some() && k.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(uuid_of_tsp) == merge_uid);
    let merge_tiles = merge_fod.and_then(|f| k.obj(f)).map(fod_range_tiles).unwrap_or_default();

    let hnm = cem.and_then(|c| c.ref_id("header_name_manager"));
    let hnmm = hnm.and_then(|h| k.obj(h));
    let frag_tiles = hnmm.map(|h| h.ref_ids("name_frag_tiles")).unwrap_or_default();
    let tracked = cem.and_then(|c| c.ref_id("named_reference_manager")).and_then(|n| k.obj(n)).and_then(|n| n.ref_id("reference_tracker"));
    let nrm_uid = hnmm.and_then(|h| h.msg("nrm_owner_uid")).map(uuid_of_tsp).filter(|u| *u != [0; 16]);
    let nrm_fod = all_fods.iter().copied().find(|f| nrm_uid.is_some() && k.obj(*f).and_then(|m| m.msg("formula_owner_uid")).map(uuid_of_tsp) == nrm_uid);
    let nrm_tiles = nrm_fod.and_then(|f| k.obj(f)).map(fod_tiles).unwrap_or_default();

    // formulas elsewhere that point into this table
    if let Some(owner) = cell_owner {
        for f in &all_fods {
            if fods.contains(f) || Some(*f) == nrm_fod {
                continue;
            }
            let Some(m) = k.obj(*f) else { continue };
            let into = fod_records(k, m).iter().any(|r| edges_to(r, owner))
                || m.msg("range_dependencies").is_some_and(|r| {
                    r.msgs("back_dependency").any(|b| b.msg("internal_range_reference").and_then(|i| i.u64("owner_id")) == Some(owner))
                })
                || fod_range_tiles(m).iter().any(|t| k.obj(*t).and_then(|t| t.u64("to_owner_id")) == Some(owner));
            if into {
                return Err("other formulas refer to this table; that isn't supported yet".into());
            }
        }
    }

    Ok(Parts {
        info: table,
        model: model_id,
        tile,
        tile_size: tiles.u32("tile_size").unwrap_or(256),
        row_bucket: row_buckets.first().copied(),
        col_bucket,
        strings: ds.ref_id("stringTable"),
        styles: ds.ref_id("styleTable"),
        formats: ds.ref_id("format_table"),
        uid_map,
        sidecar,
        layers,
        uuid,
        fods,
        cell_owner,
        merge_fod,
        merge_tiles,
        hnm,
        frag_tiles,
        tracked,
        nrm_fod,
        nrm_tiles,
    })
}

// ------------------------------------------------------------------------------ cell rows

/// The cell records of one tile row: (column, record bytes), in column order.
fn split_cells(buf: &[u8], offs: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let at: Vec<(u32, usize)> = offs
        .chunks_exact(2)
        .enumerate()
        .map(|(c, b)| (c as u32, u16::from_le_bytes([b[0], b[1]])))
        .filter(|(_, o)| *o != 0xffff)
        .map(|(c, o)| (c, o as usize))
        .collect();
    let mut out = Vec::new();
    for (n, (c, start)) in at.iter().enumerate() {
        let end = at.get(n + 1).map(|x| x.1).unwrap_or(buf.len());
        out.push((*c, buf.get(*start..end.max(*start)).unwrap_or_default().to_vec()));
    }
    out
}

/// Inverse of [`split_cells`] (records in column order, 255 offset slots).
fn join_cells(cells: &[(u32, Vec<u8>)]) -> (Vec<u8>, Vec<u8>) {
    let mut buf = Vec::new();
    let mut offs = vec![0xffu8; 2 * MAX_COLUMNS as usize];
    for (c, r) in cells {
        let c = *c as usize;
        offs[2 * c..2 * c + 2].copy_from_slice(&(buf.len() as u16).to_le_bytes());
        buf.extend(r);
    }
    (buf, offs)
}

fn row_cells(ri: &Msg) -> Vec<(u32, Vec<u8>)> {
    match (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets")) {
        (Some(b), Some(o)) => split_cells(b, o),
        _ => Vec::new(),
    }
}

/// Data list ids a storage_version 5 record refers to that we release on delete:
/// (list, key) with list 0 = strings, 1 = styles, 2 = formats.
fn record_refs(rec: &[u8]) -> Vec<(usize, u32)> {
    let rd = |at: usize| rec.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let Some(flags) = rd(8) else { return Vec::new() };
    let mut at = 12;
    let mut out = Vec::new();
    for i in 0..19 {
        let bit = 1u32 << i;
        if flags & bit == 0 {
            continue;
        }
        let size = match bit {
            0x1 => 16,
            0x2 | 0x4 => 8,
            _ => 4,
        };
        let list = match bit {
            0x8 => Some(0),
            0x20 | 0x40 => Some(1),
            0x2000..=0x40000 => Some(2),
            _ => None,
        };
        if let (Some(l), Some(key)) = (list, rd(at)) {
            out.push((l, key));
        }
        at += size;
    }
    out
}

fn list_release(list: &mut Msg, key: u32) {
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

// ------------------------------------------------------------------------------ uid map

fn uid_fields(axis: Axis) -> (&'static str, &'static str, &'static str) {
    match axis {
        Axis::Row => ("sorted_row_uids", "row_index_for_uid", "row_uid_for_index"),
        Axis::Column => ("sorted_column_uids", "column_index_for_uid", "column_uid_for_index"),
    }
}

/// Row / column uids in index order.
fn uids_by_index(m: &Msg, axis: Axis) -> Vec<Msg> {
    let (sorted, _, for_index) = uid_fields(axis);
    let s: Vec<&Msg> = m.msgs(sorted).collect();
    m.u64s(for_index).iter().filter_map(|i| s.get(*i as usize).map(|u| (*u).clone())).collect()
}

fn uid_key(u: &Msg) -> (u64, u64) {
    (u.u64("upper").unwrap_or(0), u.u64("lower").unwrap_or(0))
}

/// Rebuild a ColumnRowUIDMapArchive (fields in Keynote's order) from uids in index order.
fn uid_map(rows: &[Msg], cols: &[Msg]) -> Msg {
    let mut m = Msg::new("TST.ColumnRowUIDMapArchive");
    for (axis, list) in [(Axis::Column, cols), (Axis::Row, rows)] {
        let (sorted, index_for_uid, uid_for_index) = uid_fields(axis);
        let mut order: Vec<usize> = (0..list.len()).collect();
        order.sort_by_key(|i| uid_key(&list[*i]));
        let mut pos = vec![0u64; list.len()];
        for (s, i) in order.iter().enumerate() {
            pos[*i] = s as u64;
        }
        for i in &order {
            m.push_msg(sorted, list[*i].clone());
        }
        m.set_u64s(index_for_uid, &order.iter().map(|i| *i as u64).collect::<Vec<_>>());
        m.set_u64s(uid_for_index, &pos);
    }
    m
}

fn new_uid() -> Msg {
    let mut u = Msg::new("TSP.UUID");
    kn_format::objects::new_uuid(&mut u);
    u
}

// ------------------------------------------------------------------------------ helpers

fn header_size(k: &KeyFile, bucket: Option<ObjId>, index: u32) -> Option<f32> {
    let b = k.obj(bucket?)?;
    b.msgs("headers").find(|h| h.u32("index") == Some(index)).and_then(|h| h.f32("size")).filter(|s| *s > 0.0)
}

/// Re-number a header bucket's entries; a new row/column gets `new_size` (when it has one).
fn shift_headers(b: &mut Msg, op: Op, new_size: Option<f32>) {
    if let Op::Delete(at) = op {
        b.retain_msgs("headers", |h| h.u32("index") != Some(at));
    }
    for h in b.msgs_mut("headers") {
        if let Some(n) = h.u32("index").and_then(|i| op.index(i)) {
            h.set_u64("index", u64::from(n));
        }
    }
    if let (Op::Insert(at), Some(size)) = (op, new_size) {
        let mut hd = Msg::new("TST.HeaderStorageBucket.Header");
        hd.set_u64("index", u64::from(at));
        hd.set_f32("size", size);
        hd.set_u64("hidingState", 0);
        hd.set_u64("numberOfCells", 0);
        let pos = b.msgs("headers").take_while(|x| x.u32("index").unwrap_or(0) < at).count();
        b.insert_msg("headers", pos, hd);
    }
}

/// Set each header's numberOfCells to the records it now holds (adding headers for rows /
/// columns that hold cells but had none, like set_cell does).
fn recount(b: &mut Msg, counts: &HashMap<u32, u64>, default: f32) {
    for h in b.msgs_mut("headers") {
        let n = h.u32("index").and_then(|i| counts.get(&i)).copied().unwrap_or(0);
        h.set_u64("numberOfCells", n);
    }
    let have: HashSet<u32> = b.msgs("headers").filter_map(|h| h.u32("index")).collect();
    let mut missing: Vec<u32> = counts.iter().filter(|(i, n)| **n > 0 && !have.contains(i)).map(|(i, _)| *i).collect();
    missing.sort_unstable();
    for i in missing {
        let mut hd = Msg::new("TST.HeaderStorageBucket.Header");
        hd.set_u64("index", u64::from(i));
        hd.set_f32("size", default);
        hd.set_u64("hidingState", 0);
        hd.set_u64("numberOfCells", counts[&i]);
        let pos = b.msgs("headers").take_while(|x| x.u32("index").unwrap_or(0) < i).count();
        b.insert_msg("headers", pos, hd);
    }
}

/// Merge spans of the merge owner's formulas: (formula position, node position, row span,
/// column span).
type Span = (u32, u32);

fn tract_span(ct: &Msg, abs: &str) -> Option<Span> {
    let r = ct.msg(abs)?;
    let b = r.u32("range_begin")?;
    Some((b, r.u32("range_end").unwrap_or(b).max(b)))
}

fn set_tract_span(ct: &mut Msg, abs: &str, (b, e): Span) {
    let Some(r) = ct.msg_mut(abs) else { return };
    r.set_u64("range_begin", u64::from(b));
    if r.has("range_end") || e != b {
        r.set_u64("range_end", u64::from(e));
    }
}

/// Map a RangeCoordinateArchive along the axis (sentinel ranges are left alone).
fn map_range_coord(r: &mut Msg, n: &Names, op: Op) {
    let (Some(b), Some(e)) = (r.u32(n.tl), r.u32(n.br)) else { return };
    if r.u64("top_left_column") == Some(SENTINEL_COLUMN) {
        return;
    }
    if let Some((b, e)) = op.span(b, e) {
        r.set_u64(n.tl, u64::from(b));
        r.set_u64(n.br, u64::from(e));
    }
}

fn set_range(r: &mut Msg, cols: (u32, u32), rows: (u32, u32)) {
    r.set_u64("top_left_column", u64::from(cols.0));
    r.set_u64("top_left_row", u64::from(rows.0));
    r.set_u64("bottom_right_column", u64::from(cols.1));
    r.set_u64("bottom_right_row", u64::from(rows.1));
}

/// Header cells Keynote tracks for header names: every header-row cell right of the header
/// columns, every header-column cell below the header rows. (row, column)
fn header_cells(rows: u32, cols: u32, hr: u32, hc: u32) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    for r in 0..hr.min(rows) {
        for c in hc.min(cols)..cols {
            v.push((r, c));
        }
    }
    for c in 0..hc.min(cols) {
        for r in hr.min(rows)..rows {
            v.push((r, c));
        }
    }
    v
}

fn set_edge(rec: &mut Msg, owner: u64, (row, col): (u32, u32)) {
    let Some(e) = rec.msg_mut("expanded_edges") else { return };
    let owners = e.u64s("internal_owner_id_for_edge");
    let mut rows = e.u64s("edge_with_owner_rows");
    let mut cols = e.u64s("edge_with_owner_columns");
    for (i, o) in owners.iter().enumerate() {
        if *o == owner && i < rows.len() && i < cols.len() {
            rows[i] = u64::from(row);
            cols[i] = u64::from(col);
        }
    }
    e.set_u64s("edge_with_owner_rows", &rows);
    e.set_u64s("edge_with_owner_columns", &cols);
}

impl Document {
    /// Why rows / columns can't be added to or deleted from this table (None: they can).
    pub fn table_edit_blocker(&self, table: ObjId) -> Option<String> {
        if let Err(e) = parts(&self.file, table) {
            return Some(e);
        }
        // formulas move with their cells: every one must be one we can rewrite
        let unknown = self.formula_cells(table).into_iter().any(|(_, key)| self.formula_entry(table, key).and_then(crate::formula::read_nodes).is_none());
        unknown.then(|| "this table has formulas Keydeck can't move yet".to_string())
    }

    /// Format > Table > Header Rows / Header Columns / Footer Rows: set the count (cells take
    /// their region's style from the table style).
    pub fn set_table_header_count(&mut self, h: &mut History, table: ObjId, which: HeaderKind, n: u32) -> Result<(), kn_format::Error> {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")) else { return Err(kn_format::Error::Invalid("not a table".into())) };
        let m = self.file.obj(model).ok_or(kn_format::Error::Invalid("no table model".into()))?;
        let (rows, cols) = (m.u32("number_of_rows").unwrap_or(1), m.u32("number_of_columns").unwrap_or(1));
        let (hr, fr) = (m.u32("number_of_header_rows").unwrap_or(0), m.u32("number_of_footer_rows").unwrap_or(0));
        // at least one body row / column stays
        let (field, max) = match which {
            HeaderKind::Rows => ("number_of_header_rows", rows.saturating_sub(fr + 1)),
            HeaderKind::Columns => ("number_of_header_columns", cols.saturating_sub(1)),
            HeaderKind::Footer => ("number_of_footer_rows", rows.saturating_sub(hr + 1)),
        };
        self.touch(h, model);
        self.touch(h, table);
        if let Some(m) = self.file.obj_mut(model) {
            m.set_u64(field, u64::from(n.min(max).min(5)));
        }
        Ok(())
    }

    /// Format > Table > Table Name (the title above the table).
    pub fn set_table_name_shown(&mut self, h: &mut History, table: ObjId, on: bool) {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")) else { return };
        self.touch(h, model);
        if let Some(m) = self.file.obj_mut(model) {
            m.set_bool("table_name_enabled", on);
        }
    }

    pub fn table_name_shown(&self, table: ObjId) -> bool {
        self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).and_then(|m| m.bool("table_name_enabled")).unwrap_or(false)
    }

    /// Format > Table > Alternating Row Color: on the table's own variation of its style.
    pub fn set_table_banded(&mut self, h: &mut History, table: ObjId, on: bool) -> bool {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")) else { return false };
        let Some(base) = self.file.obj(model).and_then(|m| m.ref_id("table_style")) else { return false };
        let Some(v) = self.variation_of(h, base) else { return false };
        if v != base {
            self.touch(h, model);
            if let Some(m) = self.file.obj_mut(model) {
                m.set_ref("table_style", v);
            }
            self.file.refresh_refs(model);
            if let Some(f) = self.file.file_of(model).map(str::to_string) {
                self.file.ensure_external_refs(&f);
            }
        }
        self.touch(h, v);
        if let Some(m) = self.file.obj_mut(v) {
            m.msg_mut_or_create("table_properties").set_bool("banded_rows", on);
            kn_format::objects::fix_override_count(m);
        }
        self.file.refresh_refs(v);
        true
    }

    pub fn table_banded(&self, table: ObjId) -> bool {
        let Some(model) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)) else { return false };
        let ch = crate::style::chain(&self.file, model.ref_id("table_style"));
        ch.iter().find_map(|m| m.msg("table_properties").and_then(|p| p.bool("banded_rows"))).unwrap_or(false)
    }

    /// Format > Table > Distribute Rows / Columns Evenly: every row (column) gets the average
    /// size; the table keeps its total size.
    pub fn distribute_table(&mut self, h: &mut History, table: ObjId, rows: bool) -> bool {
        let Some(t) = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)) else { return false };
        let sizes = if rows { &t.row_heights } else { &t.col_widths };
        if sizes.is_empty() {
            return false;
        }
        let avg = sizes.iter().sum::<f32>() / sizes.len() as f32;
        let Some(model) = self.file.obj(table).and_then(|m| m.ref_id("tableModel")) else { return false };
        let ds = self.file.obj(model).and_then(|m| m.msg("base_data_store")).cloned();
        let buckets: Vec<ObjId> = if rows {
            ds.as_ref().and_then(|d| d.msg("rowHeaders")).map(|s| s.ref_ids("buckets")).unwrap_or_default()
        } else {
            ds.as_ref().and_then(|d| d.ref_id("columnHeaders")).into_iter().collect()
        };
        self.touch(h, model);
        if let Some(m) = self.file.obj_mut(model) {
            m.set_f64(if rows { "default_row_height" } else { "default_column_width" }, f64::from(avg));
        }
        for b in buckets {
            self.touch(h, b);
            if let Some(bm) = self.file.obj_mut(b) {
                for hd in bm.msgs_mut("headers") {
                    if hd.f32("size").is_some_and(|s| s > 0.0) {
                        hd.set_f32("size", avg);
                    }
                }
            }
        }
        true
    }

    /// Set row heights (or column widths) by index; the table's frame follows.
    pub fn set_table_sizes(&mut self, h: &mut History, table: ObjId, rows: bool, sizes: &[(u32, f32)]) -> bool {
        let Some(model) = self.file.obj(table).and_then(|m| m.ref_id("tableModel")) else { return false };
        let ds = self.file.obj(model).and_then(|m| m.msg("base_data_store")).cloned();
        let bucket = if rows {
            ds.as_ref().and_then(|d| d.msg("rowHeaders")).and_then(|s| s.ref_ids("buckets").first().copied())
        } else {
            ds.as_ref().and_then(|d| d.ref_id("columnHeaders"))
        };
        let Some(bucket) = bucket else { return false };
        let Some(old) = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)) else { return false };
        let before: f32 = if rows { old.row_heights.iter().sum() } else { old.col_widths.iter().sum() };
        let mut after_sizes = if rows { old.row_heights.clone() } else { old.col_widths.clone() };
        self.touch(h, bucket);
        let Some(bm) = self.file.obj_mut(bucket) else { return false };
        for &(i, size) in sizes {
            if let Some(slot) = after_sizes.get_mut(i as usize) {
                *slot = size;
            }
            let has = bm.msgs("headers").any(|e| e.u32("index") == Some(i));
            match has {
                true => {
                    if let Some(e) = bm.msgs_mut("headers").find(|e| e.u32("index") == Some(i)) {
                        e.set_f32("size", size);
                    }
                }
                false => {
                    let mut e = Msg::new("TST.HeaderStorageBucket.Header");
                    e.set_u64("index", u64::from(i));
                    e.set_f32("size", size);
                    e.set_u64("hidingState", 0);
                    e.set_u64("numberOfCells", 0);
                    bm.push_msg("headers", e);
                }
            }
        }
        let delta = after_sizes.iter().sum::<f32>() - before;
        self.touch(h, table);
        if let Some(sz) = self.file.obj_mut(table).and_then(|m| m.find_super_mut("TSD.DrawableArchive")).and_then(|d| d.msg_mut("geometry")).and_then(|g| g.msg_mut("size")) {
            let f = if rows { "height" } else { "width" };
            let v = sz.f32(f).unwrap_or(0.0);
            sz.set_f32(f, (v + delta).max(1.0));
        }
        true
    }

    /// Resize a table to `w` x `h` by scaling every row height and column width (Keynote draws
    /// tables at their row/column sizes, so the frame size alone doesn't change anything).
    pub fn scale_table_to(&mut self, h: &mut History, table: ObjId, w: f32, ht: f32) -> bool {
        let Some(t) = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)) else { return false };
        let mut ok = true;
        for (rows, sizes, total) in [(true, &t.row_heights, ht), (false, &t.col_widths, w)] {
            let sum: f32 = sizes.iter().sum();
            if sum <= 0.0 || (total - sum).abs() < 0.01 {
                continue;
            }
            let k = total.max(1.0) / sum;
            let scaled: Vec<(u32, f32)> = sizes.iter().enumerate().map(|(i, s)| (i as u32, s * k)).collect();
            ok &= self.set_table_sizes(h, table, rows, &scaled);
        }
        ok
    }

    /// Format > Table > Resize Row / Column to Fit Content (rows: the lines of text each cell
    /// needs; columns: the widest text), for `indices`.
    pub fn fit_table_to_content(&mut self, h: &mut History, table: ObjId, rows: bool, indices: &[u32]) -> bool {
        let Some(t) = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)) else { return false };
        let style = |r: u32, c: u32| t.cell_styles.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
        let text = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
        let mut sizes = Vec::new();
        for &i in indices {
            let size = if rows {
                (0..t.cols)
                    .map(|c| {
                        let st = style(i, c);
                        let lines = text(i, c).lines().count().max(1) as f32;
                        lines * st.text.size * 1.25 + st.padding[1] + st.padding[3] + 4.0
                    })
                    .fold(0.0f32, f32::max)
            } else {
                (0..t.rows)
                    .map(|r| {
                        let st = style(r, i);
                        let chars = text(r, i).lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
                        chars * st.text.size * 0.55 + st.padding[0] + st.padding[2] + 8.0
                    })
                    .fold(0.0f32, f32::max)
            };
            sizes.push((i, size.max(12.0)));
        }
        self.set_table_sizes(h, table, rows, &sizes)
    }

    pub fn table_header_count(&self, table: ObjId, which: HeaderKind) -> u32 {
        let field = match which {
            HeaderKind::Rows => "number_of_header_rows",
            HeaderKind::Columns => "number_of_header_columns",
            HeaderKind::Footer => "number_of_footer_rows",
        };
        self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).and_then(|m| m.u32(field)).unwrap_or(0)
    }

    /// Add a row / column at `at` (0..=count); it takes the height / width of `like`.
    pub fn table_insert(&mut self, h: &mut History, table: ObjId, axis: Axis, at: u32, like: u32) -> Result<(), kn_format::Error> {
        self.around_merges(h, table, axis, Op::Insert(at), |d, h| {
            d.table_rows_edit(h, table, axis, Op::Insert(at), like)?;
            d.sync_layout_columns(h, table, axis);
            Ok(())
        })
    }

    /// Delete row / column `index`.
    pub fn table_delete(&mut self, h: &mut History, table: ObjId, axis: Axis, index: u32) -> Result<(), kn_format::Error> {
        self.around_merges(h, table, axis, Op::Delete(index), |d, h| {
            d.table_rows_edit(h, table, axis, Op::Delete(index), index)?;
            d.sync_layout_columns(h, table, axis);
            Ok(())
        })
    }

    /// Rows / columns added or removed in a table with merged cells: the merges are taken
    /// out (their records and anchor flags with them), the rows / columns change as in a
    /// table without merges, and the merges are made again where they went — after the
    /// change point they move, one spanning it grows (insert) or shrinks (delete), one left
    /// a single cell or emptied goes. All in the caller's undo step, so undo puts them back.
    fn around_merges(&mut self, h: &mut History, table: ObjId, axis: Axis, op: Op, edit: impl FnOnce(&mut Document, &mut History) -> Result<(), kn_format::Error>) -> Result<(), kn_format::Error> {
        let merges: Vec<crate::CellRange> = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)).map(|t| t.merges).unwrap_or_default();
        if merges.is_empty() {
            return edit(self, h);
        }
        for m in &merges {
            self.unmerge_cells(h, table, m.row, m.col);
            self.set_merge_anchor_flag(h, table, m.row, m.col, false)?;
        }
        // a merge losing its first row / column keeps its text: it goes to the next cell,
        // the merge's first cell after the change (Numbers, Mac N19)
        if let Op::Delete(i) = op {
            for m in &merges {
                let (first, n) = if axis == Axis::Row { (m.row, m.rows) } else { (m.col, m.cols) };
                if i == first && n > 1 {
                    let to = if axis == Axis::Row { (m.row + 1, m.col) } else { (m.row, m.col + 1) };
                    self.move_cell(h, table, (m.row, m.col), to)?;
                }
            }
        }
        if let Err(e) = edit(self, h) {
            // nothing changed: the merges as they were
            for m in &merges {
                let _ = self.merge_cells(h, table, *m);
            }
            return Err(e);
        }
        for m in merges.iter().filter_map(|m| moved_merge(*m, axis, op)) {
            self.merge_cells(h, table, m)?;
        }
        Ok(())
    }

    /// The table's layout cache counts its columns: keep it right when they change (Numbers
    /// rebuilds a stale one on re-save, N2).
    fn sync_layout_columns(&mut self, h: &mut History, table: ObjId, axis: Axis) {
        if axis != Axis::Column {
            return;
        }
        let Some(cols) = self.file.obj(table).and_then(|t| t.ref_id("tableModel")).and_then(|m| self.file.obj(m)).and_then(|m| m.u64("number_of_columns")) else { return };
        let has = self.file.obj(table).and_then(|t| t.msg("layout_engine")).and_then(|l| l.msg("width_height_cache")).is_some_and(|c| c.has("columns_count"));
        if !has {
            return;
        }
        self.touch(h, table);
        if let Some(c) = self.file.obj_mut(table).and_then(|t| t.msg_mut("layout_engine")).and_then(|l| l.msg_mut("width_height_cache")) {
            c.set_u64("columns_count", cols);
        }
    }

    /// A row / column change in a table with formulas: their calculation records go first,
    /// the cells move, then each formula's references follow the cells they point at (a
    /// deleted one becomes an error) and the records are made again (as Keynote keeps them).
    fn table_rows_edit(&mut self, h: &mut History, table: ObjId, axis: Axis, op: Op, like: u32) -> Result<(), kn_format::Error> {
        use crate::formula::{Coord, Node};
        if let Some(e) = self.table_edit_blocker(table) {
            return Err(kn_format::Error::Invalid(e));
        }
        let formulas: Vec<((u32, u32), u32, Vec<Node>)> = self
            .formula_cells(table)
            .into_iter()
            .filter_map(|(rc, key)| Some((rc, key, self.formula_entry(table, key).and_then(crate::formula::read_nodes)?)))
            .collect();
        for ((r, c), ..) in &formulas {
            self.drop_formula_deps(h, table, *r, *c);
        }
        // cells in several tiles: one tile while rows / columns move, dealt out again after
        self.gather_tiles(h, table)?;
        let moved = self.table_rows_move(h, table, axis, op, like);
        self.scatter_tiles(h, table)?;
        moved?;
        if formulas.is_empty() {
            return Ok(());
        }
        let list = self.formula_list(table);
        let along = |r: u32, c: u32| if matches!(axis, Axis::Row) { r } else { c };
        for ((r, c), key, nodes) in formulas {
            let Some(new_at) = op.index(along(r, c)) else {
                // the formula's own cell is gone: let its entry go
                if let Some(l) = list.and_then(|l| self.file.obj_mut(l)) {
                    crate::tablecells::list_release(l, key);
                }
                continue;
            };
            let (nr, nc) = if matches!(axis, Axis::Row) { (new_at, c) } else { (r, new_at) };
            let (old_base, new_base) = (along(r, c), along(nr, nc));
            let moved = |co: Coord| -> Option<Coord> {
                let target = if co.abs { co.v } else { i64::from(old_base) + co.v };
                let t = op.index(u32::try_from(target).ok()?)?;
                Some(if co.abs { Coord { v: i64::from(t), abs: true } } else { Coord { v: i64::from(t) - i64::from(new_base), abs: false } })
            };
            let shifted = |co: Coord| -> Coord { if co.abs { co } else { Coord { v: co.v, abs: false } } };
            let bad = Node::Str("#REF!".into());
            let out: Vec<Node> = nodes
                .into_iter()
                .map(|n| match n {
                    Node::Ref { row, col } => {
                        let (row2, col2) = if matches!(axis, Axis::Row) { (moved(row), Some(shifted(col))) } else { (Some(shifted(row)), moved(col)) };
                        match (row2, col2) {
                            (Some(row), Some(col)) => Node::Ref { row, col },
                            _ => bad.clone(),
                        }
                    }
                    Node::Range { r0, c0, r1, c1 } => {
                        let span = |a: Coord, b: Coord| -> Option<(Coord, Coord)> {
                            let ta = if a.abs { a.v } else { i64::from(old_base) + a.v };
                            let tb = if b.abs { b.v } else { i64::from(old_base) + b.v };
                            let (lo, hi) = op.span(u32::try_from(ta.min(tb)).ok()?, u32::try_from(ta.max(tb)).ok()?)?;
                            let put = |t: u32, abs: bool| if abs { Coord { v: i64::from(t), abs } } else { Coord { v: i64::from(t) - i64::from(new_base), abs } };
                            Some((put(lo, a.abs), put(hi, b.abs)))
                        };
                        let res = if matches!(axis, Axis::Row) { span(r0, r1).map(|(a, b)| (a, c0, b, c1)) } else { span(c0, c1).map(|(a, b)| (r0, a, r1, b)) };
                        match res {
                            Some((r0, c0, r1, c1)) => Node::Range { r0, c0, r1, c1 },
                            None => bad.clone(),
                        }
                    }
                    other => other,
                })
                .collect();
            // the formula's entry gets the moved references
            if let Some(l) = list {
                self.touch(h, l);
                if let Some(e) = self.file.obj_mut(l).and_then(|l| l.msgs_mut("entries").find(|e| e.u32("key") == Some(key))) {
                    e.remove("formula");
                    crate::formula::write_nodes(&out, e.msg_mut_or_create("formula"));
                }
            }
            self.add_formula_deps(h, table, nr, nc, &out)?;
        }
        self.recalc_formulas(h, table);
        Ok(())
    }

    fn table_rows_move(&mut self, h: &mut History, table: ObjId, axis: Axis, op: Op, like: u32) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let p = parts(&self.file, table).map_err(|e| err(&e))?;
        let n = names(axis);
        let model = self.file.obj(p.model).ok_or(err("no table model"))?;
        let rows = model.u32("number_of_rows").unwrap_or(0);
        let cols = model.u32("number_of_columns").unwrap_or(0);
        let (hr, hc, fr) = (
            model.u32("number_of_header_rows").unwrap_or(0).min(rows),
            model.u32("number_of_header_columns").unwrap_or(0).min(cols),
            model.u32("number_of_footer_rows").unwrap_or(0).min(rows),
        );
        let default_row = model.f64("default_row_height").unwrap_or(40.0) as f32;
        let default_col = model.f64("default_column_width").unwrap_or(120.0) as f32;
        let (count, limit, bucket, default) = match axis {
            Axis::Row => (rows, MAX_ROWS.max(p.tile_size), p.row_bucket, default_row),
            Axis::Column => (cols, MAX_COLUMNS, p.col_bucket, default_col),
        };
        match op {
            Op::Insert(at) if at > count || like >= count => return Err(err("no such row or column")),
            Op::Insert(_) if count >= limit => return Err(err("the table can't get any bigger")),
            Op::Delete(at) if at >= count => return Err(err("no such row or column")),
            Op::Delete(_) if count <= 1 => return Err(err("a table needs at least one row and one column")),
            _ => {}
        }

        // merges: new spans, checked before anything changes
        let merges: Vec<(Span, Span)> = model
            .msg("merge_owner")
            .and_then(|m| m.msg("formula_store"))
            .map(|s| {
                s.msgs("formulas")
                    .flat_map(|f| f.msg("formula").and_then(|f| f.msg("AST_node_array")).map(|a| a.msgs("AST_node").collect::<Vec<_>>()).unwrap_or_default())
                    .filter_map(|nd| nd.msg("AST_colon_tract"))
                    .filter_map(|ct| Some((tract_span(ct, "absolute_row")?, tract_span(ct, "absolute_column")?)))
                    .collect()
            })
            .unwrap_or_default();
        for (r, c) in &merges {
            let (r2, c2) = match axis {
                Axis::Row => (op.span(r.0, r.1), Some(*c)),
                Axis::Column => (Some(*r), op.span(c.0, c.1)),
            };
            match (r2, c2) {
                (Some(r), Some(c)) if (r.1 - r.0 + 1) * (c.1 - c.0 + 1) > 1 => {}
                _ => return Err(err("this would delete a merged cell; unmerge it first")),
            }
        }

        let size = match op {
            Op::Insert(_) => header_size(&self.file, bucket, like),
            Op::Delete(at) => header_size(&self.file, bucket, at),
        };
        let extent = size.unwrap_or(default);
        let old_rows_uids = uids_by_index(self.file.obj(p.uid_map).unwrap(), Axis::Row);
        let old_cols_uids = uids_by_index(self.file.obj(p.uid_map).unwrap(), Axis::Column);

        for id in p.touched() {
            self.touch(h, id);
        }
        let k = &mut self.file;

        // 1. cells
        let mut released: Vec<(usize, u32)> = Vec::new();
        {
            let t = k.obj_mut(p.tile).ok_or(err("no cell tile"))?;
            match axis {
                Axis::Row => {
                    if let Op::Delete(at) = op {
                        for ri in t.msgs("rowInfos").filter(|r| r.u32("tile_row_index") == Some(at)) {
                            for (_, rec) in row_cells(ri) {
                                released.extend(record_refs(&rec));
                            }
                        }
                        t.retain_msgs("rowInfos", |r| r.u32("tile_row_index") != Some(at));
                    }
                    for ri in t.msgs_mut("rowInfos") {
                        if let Some(i) = ri.u32("tile_row_index").and_then(|i| op.index(i)) {
                            ri.set_u64("tile_row_index", u64::from(i));
                        }
                    }
                }
                Axis::Column => {
                    for ri in t.msgs_mut("rowInfos") {
                        let remap = |cells: Vec<(u32, Vec<u8>)>, released: &mut Vec<(usize, u32)>, new: bool| -> Vec<(u32, Vec<u8>)> {
                            cells
                                .into_iter()
                                .filter_map(|(c, rec)| match op.index(c) {
                                    Some(c) => Some((c, rec)),
                                    None => {
                                        if new {
                                            released.extend(record_refs(&rec));
                                        }
                                        None
                                    }
                                })
                                .collect()
                        };
                        if let (Some(b), Some(o)) = (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets")) {
                            let cells = remap(split_cells(b, o), &mut released, true);
                            let (buf, offs) = join_cells(&cells);
                            ri.set_bytes("cell_storage_buffer", &buf);
                            ri.set_bytes("cell_offsets", &offs);
                            ri.set_u64("cell_count", cells.len() as u64);
                        }
                        if let (Some(b), Some(o)) = (ri.bytes("cell_storage_buffer_pre_bnc"), ri.bytes("cell_offsets_pre_bnc")) {
                            let cells = remap(split_cells(b, o), &mut released, false);
                            let (buf, offs) = join_cells(&cells);
                            ri.set_bytes("cell_storage_buffer_pre_bnc", &buf);
                            ri.set_bytes("cell_offsets_pre_bnc", &offs);
                        }
                    }
                    t.retain_msgs("rowInfos", |r| r.u32("cell_count").unwrap_or(0) > 0 || r.bytes("cell_storage_buffer").is_none_or(|b| !b.is_empty()));
                }
            }
            // the number of rows holding cells (Keynote, T61), not the last row + 1
            let rows_with_cells = t.count("rowInfos");
            t.set_u64("numrows", rows_with_cells as u64);
        }
        for (list, key) in released {
            let id = [p.strings, p.styles, p.formats][list];
            if let Some(l) = id.and_then(|i| k.obj_mut(i)) {
                list_release(l, key);
            }
        }

        // 2. header buckets: sizes, then cell counts for both axes
        if let Some(b) = bucket.and_then(|b| k.obj_mut(b)) {
            shift_headers(b, op, size);
        }
        let (mut per_row, mut per_col) = (HashMap::<u32, u64>::new(), HashMap::<u32, u64>::new());
        for ri in k.obj(p.tile).map(|t| t.msgs("rowInfos").collect::<Vec<_>>()).unwrap_or_default() {
            let r = ri.u32("tile_row_index").unwrap_or(0);
            for (c, _) in row_cells(ri) {
                *per_row.entry(r).or_default() += 1;
                *per_col.entry(c).or_default() += 1;
            }
        }
        if let Some(b) = p.row_bucket.and_then(|b| k.obj_mut(b)) {
            recount(b, &per_row, default_row);
        }
        if let Some(b) = p.col_bucket.and_then(|b| k.obj_mut(b)) {
            recount(b, &per_col, default_col);
        }

        // 3. the model's counts
        let grow = |c: u32| match op {
            Op::Insert(_) => c + 1,
            Op::Delete(_) => c - 1,
        };
        let (mut nrows, mut ncols, mut nhr, mut nhc, mut nfr) = (rows, cols, hr, hc, fr);
        match (axis, op) {
            (Axis::Row, Op::Insert(at)) => {
                nhr += u32::from(at < hr);
                nfr += u32::from(fr > 0 && at > rows - fr);
            }
            (Axis::Row, Op::Delete(at)) => {
                nhr -= u32::from(at < hr);
                nfr -= u32::from(fr > 0 && at >= rows - fr);
            }
            (Axis::Column, Op::Insert(at)) => nhc += u32::from(at < hc),
            (Axis::Column, Op::Delete(at)) => nhc -= u32::from(at < hc),
        }
        match axis {
            Axis::Row => nrows = grow(rows),
            Axis::Column => ncols = grow(cols),
        }
        {
            let m = k.obj_mut(p.model).unwrap();
            m.set_u64("number_of_rows", u64::from(nrows));
            m.set_u64("number_of_columns", u64::from(ncols));
            for (f, old, new) in [("number_of_header_rows", hr, nhr), ("number_of_header_columns", hc, nhc), ("number_of_footer_rows", fr, nfr)] {
                if old != new {
                    m.set_u64(f, u64::from(new));
                }
            }
        }

        // 4. uids
        let (mut row_uids, mut col_uids) = (old_rows_uids, old_cols_uids);
        let list = match axis {
            Axis::Row => &mut row_uids,
            Axis::Column => &mut col_uids,
        };
        let deleted_uid = match op {
            Op::Insert(at) => {
                list.insert(at as usize, new_uid());
                None
            }
            Op::Delete(at) => Some(uid_key(&list.remove(at as usize))),
        };
        *k.obj_mut(p.uid_map).unwrap() = uid_map(&row_uids, &col_uids);

        // 5. the frame
        if let Some(sz) = k.obj_mut(p.info).and_then(|m| m.find_super_mut("TSD.DrawableArchive")).and_then(|d| d.msg_mut("geometry")).and_then(|g| g.msg_mut("size")) {
            let f = match axis {
                Axis::Row => "height",
                Axis::Column => "width",
            };
            let v = sz.f32(f).unwrap_or(0.0);
            let d = if matches!(op, Op::Insert(_)) { extent } else { -extent };
            sz.set_f32(f, (v + d).max(1.0));
        }

        // 6. strokes
        if let Some(s) = p.sidecar.and_then(|s| k.obj_mut(s)) {
            for (f, v) in [("row_count", nrows), ("column_count", ncols)] {
                if s.has(f) {
                    s.set_u64(f, u64::from(v));
                }
            }
        }
        let (index_layers, run_layers) = match axis {
            Axis::Row => (["top_row_stroke_layers", "bottom_row_stroke_layers"], ["left_column_stroke_layers", "right_column_stroke_layers"]),
            Axis::Column => (["left_column_stroke_layers", "right_column_stroke_layers"], ["top_row_stroke_layers", "bottom_row_stroke_layers"]),
        };
        if let Some(sc) = p.sidecar {
            let get = |k: &KeyFile, f: &str| k.obj(sc).map(|s| s.ref_ids(f)).unwrap_or_default();
            for f in index_layers {
                let mut gone = Vec::new();
                for l in get(k, f) {
                    let Some(lm) = k.obj_mut(l) else { continue };
                    match lm.u32("row_column_index").map(|i| op.index(i)) {
                        Some(Some(i)) => lm.set_u64("row_column_index", u64::from(i)),
                        Some(None) => gone.push(l),
                        None => {}
                    }
                }
                if !gone.is_empty() {
                    k.obj_mut(sc).unwrap().retain_msgs(f, |r| !r.u64("identifier").is_some_and(|i| gone.contains(&i)));
                }
            }
            for f in run_layers {
                for l in get(k, f) {
                    let Some(lm) = k.obj_mut(l) else { continue };
                    lm.retain_msgs("stroke_runs", |run| {
                        let o = run.i64("origin").unwrap_or(0).max(0) as u32;
                        let len = run.u32("length").unwrap_or(0);
                        len == 0 || op.span(o, o + len - 1).is_some()
                    });
                    for run in lm.msgs_mut("stroke_runs") {
                        let o = run.i64("origin").unwrap_or(0).max(0) as u32;
                        let len = run.u32("length").unwrap_or(0);
                        if let Some((b, e)) = (len > 0).then(|| op.span(o, o + len - 1)).flatten() {
                            run.set_i64("origin", i64::from(b));
                            run.set_u64("length", u64::from(e - b + 1));
                        }
                    }
                }
            }
        }

        // 7. merges: the merge owner's formulas, its range dependency and precedents tile
        if !merges.is_empty() {
            let m = k.obj_mut(p.model).unwrap();
            if let Some(store) = m.msg_mut("merge_owner").and_then(|m| m.msg_mut("formula_store")) {
                for f in store.msgs_mut("formulas") {
                    let Some(arr) = f.msg_mut("formula").and_then(|f| f.msg_mut("AST_node_array")) else { continue };
                    for nd in arr.msgs_mut("AST_node") {
                        let Some(ct) = nd.msg_mut("AST_colon_tract") else { continue };
                        if let Some(sp) = tract_span(ct, n.abs).and_then(|(b, e)| op.span(b, e)) {
                            set_tract_span(ct, n.abs, sp);
                        }
                    }
                }
            }
            if let (Some(f), Some(owner)) = (p.merge_fod.and_then(|f| k.obj_mut(f)), p.cell_owner) {
                if let Some(rd) = f.msg_mut("range_dependencies") {
                    for b in rd.msgs_mut("back_dependency") {
                        let Some(ir) = b.msg_mut("internal_range_reference") else { continue };
                        if ir.u64("owner_id") == Some(owner) {
                            if let Some(r) = ir.msg_mut("range") {
                                map_range_coord(r, &n, op);
                            }
                        }
                    }
                }
            }
            for t in &p.merge_tiles {
                let Some(tm) = k.obj_mut(*t) else { continue };
                if tm.u64("to_owner_id") != p.cell_owner {
                    continue;
                }
                for ftr in tm.msgs_mut("from_to_range") {
                    let Some(rect) = ftr.msg_mut("refers_to_rect") else { continue };
                    let o = rect.msg("origin").and_then(|o| o.u32(n.coord)).unwrap_or(0);
                    let len = rect.msg("size").and_then(|s| s.u32(n.count)).unwrap_or(1).max(1);
                    let Some((b, e)) = op.span(o, o + len - 1) else { continue };
                    rect.msg_mut_or_create("origin").set_u64(n.coord, u64::from(b));
                    let sz = rect.msg_mut_or_create("size");
                    if sz.has(n.count) || e - b + 1 != 1 {
                        sz.set_u64(n.count, u64::from(e - b + 1));
                    }
                }
            }
        }

        // 8. the table owner's spanning ranges (whole table, body)
        let total = ((0, ncols - 1), (0, nrows - 1));
        let body_rows = (nhr.min(nrows - 1), (nrows - nfr).saturating_sub(1).max(nhr.min(nrows - 1)));
        let body = ((nhc.min(ncols - 1), ncols - 1), body_rows);
        for f in &p.fods {
            let Some(m) = k.obj_mut(*f) else { continue };
            if m.u64("owner_kind") != Some(1) {
                continue;
            }
            for sp in ["spanning_column_dependencies", "spanning_row_dependencies"] {
                let Some(s) = m.msg_mut(sp) else { continue };
                for (fld, (c, r)) in [("total_range_for_table", total), ("body_range_for_table", body)] {
                    if let Some(rm) = s.msg_mut(fld) {
                        if rm.u64("top_left_column") != Some(SENTINEL_COLUMN) {
                            set_range(rm, c, r);
                        }
                    }
                }
            }
        }

        // 9. header names: header uids, tracked header cells, name fragment uses
        let table_tsp = |m: &Msg| m.msg("table_uid").map(uuid_of_tsp) == Some(p.uuid);
        if let Some(hm) = p.hnm.and_then(|i| k.obj_mut(i)) {
            for per in hm.msgs_mut("per_tables") {
                if !table_tsp(per) {
                    continue;
                }
                let (f, list, hcount) = match axis {
                    Axis::Row => ("header_row_uids", &row_uids, nhr),
                    Axis::Column => ("header_column_uids", &col_uids, nhc),
                };
                per.retain_msgs(f, |_| false);
                for u in list.iter().take(hcount as usize) {
                    per.push_msg(f, u.clone());
                }
            }
        }
        if let Some(gone) = deleted_uid {
            let table_uid = p.uuid;
            for t in &p.frag_tiles {
                let Some(tm) = k.obj_mut(*t) else { continue };
                for fe in tm.msgs_mut("name_frag_entries") {
                    let Some(uses) = fe.msg_mut("uses_of_name_fragment") else { continue };
                    for oe in uses.msgs_mut("owner_entries") {
                        if oe.msg("owner_uid").map(uuid_of_tsp) != Some(table_uid) {
                            continue;
                        }
                        let Some(cs) = oe.msg_mut("coord_set") else { continue };
                        match axis {
                            Axis::Column => cs.retain_msgs("column_entries", |ce| ce.msg("column").map(uid_key) != Some(gone)),
                            Axis::Row => {
                                for ce in cs.msgs_mut("column_entries") {
                                    ce.retain_msgs("row_set", |u| uid_key(u) != gone);
                                }
                                cs.retain_msgs("column_entries", |ce| ce.count("row_set") > 0);
                            }
                        }
                    }
                    uses.retain_msgs("owner_entries", |oe| oe.msg("coord_set").is_none_or(|cs| cs.count("column_entries") > 0));
                }
            }
        }
        self.sync_tracked_headers(&p, axis, op, (nrows, ncols, nhr, nhc));
        Ok(())
    }

    /// Keep one tracked absolute reference per header cell (see [`header_cells`]): move the
    /// existing ones, drop the ones whose cell went away or is no longer a header cell, add
    /// the new header cells (copying an existing reference of the table). The header-name
    /// owner's records of those references (when it has them) follow.
    fn sync_tracked_headers(&mut self, p: &Parts, axis: Axis, op: Op, (rows, cols, hr, hc): (u32, u32, u32, u32)) {
        let Some(store) = p.tracked else { return };
        let k = &mut self.file;
        let want: HashSet<(u32, u32)> = header_cells(rows, cols, hr, hc).into_iter().collect();
        let Some(sm) = k.obj(store) else { return };
        let mut template: Option<Msg> = None;
        let mut moved: HashMap<u64, Option<(u32, u32)>> = HashMap::new();
        let mut next_id = 0;
        for r in sm.msgs("contained_tracked_reference") {
            let id = r.u64("formula_id").unwrap_or(0);
            next_id = next_id.max(id + 1);
            let Some((row, col)) = tracked_cell(r, &p.uuid) else { continue };
            template.get_or_insert_with(|| r.clone());
            let new = match axis {
                Axis::Row => op.index(row).map(|r| (r, col)),
                Axis::Column => op.index(col).map(|c| (row, c)),
            };
            moved.insert(id, new.filter(|c| want.contains(c)));
        }
        let have: HashSet<(u32, u32)> = moved.values().flatten().copied().collect();
        let mut add: Vec<(u32, u32)> = want.iter().copied().filter(|c| !have.contains(c)).collect();
        add.sort_unstable_by_key(|(r, c)| (*r, *c));
        if template.is_none() {
            add.clear(); // nothing to copy the shape from
        }
        let mut added: Vec<(u64, (u32, u32))> = Vec::new();
        {
            let sm = k.obj_mut(store).unwrap();
            sm.retain_msgs("contained_tracked_reference", |r| !matches!(moved.get(&r.u64("formula_id").unwrap_or(0)), Some(None)));
            for r in sm.msgs_mut("contained_tracked_reference") {
                let Some(Some((row, col))) = moved.get(&r.u64("formula_id").unwrap_or(0)) else { continue };
                set_tracked_cell(r, *row, *col);
            }
            for (i, cell) in add.iter().enumerate() {
                let mut r = template.clone().unwrap();
                let id = next_id + i as u64;
                r.set_u64("formula_id", id);
                set_tracked_cell(&mut r, cell.0, cell.1);
                sm.push_msg("contained_tracked_reference", r);
                added.push((id, *cell));
            }
        }

        // the header-name owner's records (row 0, column = formula id, edge to the cell)
        let (Some(fod), Some(owner)) = (p.nrm_fod, p.cell_owner) else { return };
        let mut rec_template: Option<Msg> = None;
        let mut holders: Vec<ObjId> = vec![fod];
        holders.extend(&p.nrm_tiles);
        for hid in &holders {
            let Some(m) = k.obj_mut(*hid) else { continue };
            let list = if *hid == fod { m.msg_mut("cell_dependencies") } else { Some(m) };
            let Some(list) = list else { continue };
            let field = if *hid == fod { "cell_record" } else { "cell_records" };
            list.retain_msgs(field, |r| !(r.u64("row") == Some(0) && matches!(moved.get(&r.u64("column").unwrap_or(u64::MAX)), Some(None)) && edges_to(r, owner)));
            for r in list.msgs_mut(field) {
                if r.u64("row") != Some(0) || !edges_to(r, owner) {
                    continue;
                }
                let Some(Some(cell)) = moved.get(&r.u64("column").unwrap_or(u64::MAX)) else { continue };
                if rec_template.is_none() {
                    rec_template = Some(r.clone());
                }
                set_edge(r, owner, *cell);
            }
        }
        let Some(tpl) = rec_template else { return };
        for (id, cell) in added {
            let mut r = tpl.clone();
            r.set_u64("column", id);
            set_edge(&mut r, owner, cell);
            if let Some(cd) = k.obj_mut(fod).and_then(|m| m.msg_mut("cell_dependencies")) {
                cd.push_msg("cell_record", r.clone());
            }
            let tile = p.nrm_tiles.iter().copied().filter(|t| k.obj(*t).is_some_and(|m| m.u64("tile_row_begin") == Some(0) && m.u64("tile_column_begin").unwrap_or(0) <= id)).max_by_key(|t| k.obj(*t).and_then(|m| m.u64("tile_column_begin")).unwrap_or(0));
            if let Some(tm) = tile.and_then(|t| k.obj_mut(t)) {
                if id - tm.u64("tile_column_begin").unwrap_or(0) < 256 {
                    let mut r2 = Msg::new("TSCE.CellRecordExpandedArchive");
                    r2.fields = r.fields;
                    tm.push_msg("cell_records", r2);
                }
            }
        }
    }
}

fn set_tracked_cell(r: &mut Msg, row: u32, col: u32) {
    let Some(nd) = r.msg_mut("ast").and_then(|a| a.msg_mut("AST_node")) else { return };
    if let Some(m) = nd.msg_mut("AST_row") {
        m.set_i64("row", i64::from(row));
    }
    if let Some(m) = nd.msg_mut("AST_column") {
        m.set_i64("column", i64::from(col));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tablecells::CellInput;
    use crate::{DrawableKind, TableData};

    fn open(name: &str) -> Document {
        Document::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata").join(name)).unwrap()
    }

    /// Every table: (slide index, TableInfo id).
    fn tables(d: &Document) -> Vec<(usize, ObjId)> {
        let s = d.slides();
        let mut v = Vec::new();
        for i in 0..s.len() {
            for x in d.scene(&s, i).unwrap().drawables {
                if matches!(x.kind, DrawableKind::Table(_)) {
                    v.push((i, x.id));
                }
            }
        }
        v
    }

    fn data(d: &Document, i: usize, id: ObjId) -> TableData {
        let sc = d.scene(&d.slides(), i).unwrap();
        match sc.drawables.into_iter().find(|x| x.id == id).unwrap().kind {
            DrawableKind::Table(t) => t,
            _ => unreachable!(),
        }
    }

    fn reopen(d: &Document) -> Document {
        Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap())
    }

    /// References to objects that don't exist.
    fn dangling(k: &KeyFile) -> HashSet<(ObjId, ObjId)> {
        let mut out = HashSet::new();
        for (_, a) in k.archives() {
            for (i, p) in a.payloads.iter().enumerate() {
                let mut refs = Vec::new();
                p.collect_refs(&mut refs);
                refs.extend(a.object_references(i));
                for r in refs {
                    if k.archive(r).is_none() {
                        out.insert((a.id(), r));
                    }
                }
            }
        }
        out
    }

    fn size(d: &Document, id: ObjId) -> (f32, f32) {
        let g = d.file.obj(id).unwrap().find_super("TSD.DrawableArchive").unwrap().msg("geometry").unwrap().msg("size").unwrap();
        (g.f32("width").unwrap(), g.f32("height").unwrap())
    }

    fn tracked_count(d: &Document, id: ObjId) -> usize {
        let p = parts(&d.file, id).unwrap();
        d.file.obj(p.tracked.unwrap()).unwrap().msgs("contained_tracked_reference").filter(|r| tracked_cell(r, &p.uuid).is_some()).count()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn our_reading_of_keynote_tables_holds() {
        // numberOfCells = records per row / column; the uid map rebuilds byte for byte;
        // tracked header cells follow header_cells()
        for f in ["05-table.key", "Keynote-Feature-Catalog.key", "ref/R2-table-values.key"] {
            let d = open(f);
            for (_, id) in tables(&d) {
                let k = &d.file;
                let model = k.obj(k.obj(id).unwrap().ref_id("tableModel").unwrap()).unwrap();
                let ds = model.msg("base_data_store").unwrap();
                let tile = ds.msg("tiles").unwrap().msgs("tiles").next().unwrap().ref_id("tile").unwrap();
                let (mut per_row, mut per_col) = (HashMap::<u32, u64>::new(), HashMap::<u32, u64>::new());
                for ri in k.obj(tile).unwrap().msgs("rowInfos") {
                    for (c, _) in row_cells(ri) {
                        *per_row.entry(ri.u32("tile_row_index").unwrap()).or_default() += 1;
                        *per_col.entry(c).or_default() += 1;
                    }
                }
                let buckets = ds.msg("rowHeaders").unwrap().ref_ids("buckets").into_iter().map(|b| (b, &per_row)).chain([(ds.ref_id("columnHeaders").unwrap(), &per_col)]);
                for (b, counts) in buckets {
                    for hd in k.obj(b).unwrap().msgs("headers") {
                        let i = hd.u32("index").unwrap();
                        assert_eq!(hd.u64("numberOfCells").unwrap(), counts.get(&i).copied().unwrap_or(0), "{f} table {id} bucket {b} index {i}");
                    }
                }
                let um = k.obj(model.ref_id("base_column_row_uids").unwrap()).unwrap();
                let rebuilt = uid_map(&uids_by_index(um, Axis::Row), &uids_by_index(um, Axis::Column));
                assert_eq!(rebuilt.encode(), um.encode(), "{f} table {id} uid map");
                let p = parts(k, id);
                if let Ok(p) = p {
                    let (rows, cols) = (model.u32("number_of_rows").unwrap(), model.u32("number_of_columns").unwrap());
                    let (hr, hc) = (model.u32("number_of_header_rows").unwrap_or(0), model.u32("number_of_header_columns").unwrap_or(0));
                    let mut want = header_cells(rows, cols, hr, hc);
                    let mut got: Vec<(u32, u32)> = k.obj(p.tracked.unwrap()).unwrap().msgs("contained_tracked_reference").filter_map(|r| tracked_cell(r, &p.uuid)).collect();
                    want.sort_unstable();
                    got.sort_unstable();
                    assert_eq!(got, want, "{f} table {id} tracked header cells");
                }
            }
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn which_tables_are_refused() {
        let d = open("Keynote-Feature-Catalog.key");
        let refused: Vec<String> = tables(&d).iter().filter_map(|(_, id)| d.table_edit_blocker(*id)).collect();
        // the slide 27 table (merged cells: Keynote hung reading its cells after our edit, Mac
        // round 8); tables with formulas are fine now (their formulas move with the cells)
        assert_eq!(refused, vec!["tables with merged cells can't have rows or columns added or removed yet".to_string()]);
        let d = open("ref/R2-table-values.key");
        assert_eq!(tables(&d).iter().filter(|(_, id)| d.table_edit_blocker(*id).is_none()).count(), 2);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn scaling_a_table_scales_its_rows_and_columns() {
        let mut d = open("ref/R2-table-values.key");
        let (i, t) = *tables(&d).iter().find(|(_, id)| d.table_edit_blocker(*id).is_none() && d.formula_cells(*id).is_empty()).unwrap();
        let t0 = data(&d, i, t);
        let (w0, h0) = (t0.width(), t0.height());
        let mut h = History::default();
        d.begin(&mut h, "Resize");
        assert!(d.scale_table_to(&mut h, t, w0 * 1.5, h0 / 2.0));
        d.end(&mut h);
        let x = data(&reopen(&d), i, t);
        assert!((x.width() - w0 * 1.5).abs() < 0.01 && (x.height() - h0 / 2.0).abs() < 0.01);
        assert!((x.col_widths[0] - t0.col_widths[0] * 1.5).abs() < 0.01);
        assert!((x.row_heights[1] - t0.row_heights[1] / 2.0).abs() < 0.01);
        // the frame follows the rows / columns, whatever the stored size says
        let f = x.frame(&crate::Geometry { w: 1.0, h: 1.0, ..Default::default() });
        assert_eq!((f.w, f.h), (x.width(), x.height()));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn rows_and_columns_of_a_basic_table() {
        // the untouched Insert > Table > Basic table of R2 (5 x 4, one header row)
        let mut d = open("ref/R2-table-values.key");
        let (i, t) = *tables(&d).iter().find(|(_, id)| d.table_edit_blocker(*id).is_none()).unwrap();
        let before_zip = d.file.to_zip().unwrap();
        let base_dangling = dangling(&d.file);
        let mut h = History::default();
        d.begin(&mut h, "Cells");
        for r in 0..5 {
            for c in 0..4 {
                d.set_cell(&mut h, t, r, c, &CellInput::Text(format!("R{r}C{c}"))).unwrap();
            }
        }
        d.end(&mut h);
        let t0 = data(&d, i, t);
        let (w0, h0) = size(&d, t);

        d.begin(&mut h, "Add Row Below");
        d.table_insert(&mut h, t, Axis::Row, 3, 2).unwrap(); // below row 2
        d.end(&mut h);
        d.begin(&mut h, "Add Column After");
        d.table_insert(&mut h, t, Axis::Column, 1, 0).unwrap(); // after column 0
        d.end(&mut h);
        d.begin(&mut h, "Add Row Above");
        d.table_insert(&mut h, t, Axis::Row, 0, 0).unwrap(); // above the header row: a header row
        d.end(&mut h);
        d.begin(&mut h, "Delete Row");
        d.table_delete(&mut h, t, Axis::Row, 6).unwrap(); // the last row (old row 4)
        d.end(&mut h);

        let d2 = reopen(&d);
        let x = data(&d2, i, t);
        assert_eq!((x.rows, x.cols, x.header_rows), (6, 5, 2));
        let cell = |r: u32, c: u32| x.cells[(r * x.cols + c) as usize].as_str();
        let expect: [[&str; 5]; 6] = [
            ["", "", "", "", ""],
            ["R0C0", "", "R0C1", "R0C2", "R0C3"],
            ["R1C0", "", "R1C1", "R1C2", "R1C3"],
            ["R2C0", "", "R2C1", "R2C2", "R2C3"],
            ["", "", "", "", ""],
            ["R3C0", "", "R3C1", "R3C2", "R3C3"],
        ];
        for (r, row) in expect.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                assert_eq!(cell(r as u32, c as u32), *v, "cell {r},{c}");
            }
        }
        // sizes: the new column is as wide as column 0, the frame grew accordingly
        assert_eq!(x.col_widths[1], t0.col_widths[0]);
        let (w, hh) = size(&d2, t);
        assert!((w - (w0 + t0.col_widths[0])).abs() < 0.01);
        assert!((hh - (h0 + t0.row_heights[2] + t0.row_heights[0] - t0.row_heights[4])).abs() < 0.01);
        assert!((x.row_heights.iter().sum::<f32>() - hh).abs() < 0.01);
        assert!((x.col_widths.iter().sum::<f32>() - w).abs() < 0.01);
        // the reading rules still hold, nothing dangles, header names follow
        assert_eq!(tracked_count(&d2, t), 10); // 2 header rows x 5 columns
        assert_eq!(dangling(&d2.file), base_dangling);
        let k = &d2.file;
        let p = parts(k, t).unwrap();
        let um = k.obj(p.uid_map).unwrap();
        assert_eq!((um.count("sorted_row_uids"), um.count("sorted_column_uids")), (6, 5));
        let hnm = k.obj(p.hnm.unwrap()).unwrap();
        let per = hnm.msgs("per_tables").find(|x| x.msg("table_uid").map(uuid_of_tsp) == Some(p.uuid)).unwrap();
        let rows_by_index: Vec<(u64, u64)> = uids_by_index(um, Axis::Row).iter().map(uid_key).collect();
        assert_eq!(per.msgs("header_row_uids").map(uid_key).collect::<Vec<_>>(), rows_by_index[..2].to_vec());

        // delete a column and the new header row again
        d.begin(&mut h, "Delete Column");
        d.table_delete(&mut h, t, Axis::Column, 3).unwrap(); // old column 2
        d.end(&mut h);
        d.begin(&mut h, "Delete Row");
        d.table_delete(&mut h, t, Axis::Row, 0).unwrap();
        d.end(&mut h);
        let d3 = reopen(&d);
        let x = data(&d3, i, t);
        assert_eq!((x.rows, x.cols, x.header_rows), (5, 4, 1));
        assert_eq!(x.cells[..4], ["R0C0", "", "R0C1", "R0C3"]);
        assert_eq!(tracked_count(&d3, t), 4);
        assert_eq!(dangling(&d3.file), base_dangling);

        // undo everything
        while d.undo(&mut h) {}
        assert_eq!(d.file.to_zip().unwrap(), before_zip);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn tables_we_inserted() {
        let mut d = open("00-base.key");
        let mut h = History::default();
        let slide = d.slides()[0].slide;
        d.begin(&mut h, "Insert");
        let a = d.insert(&mut h, slide, &crate::insert::NewObject::BASIC_TABLE).unwrap();
        let b = d.insert(&mut h, slide, &crate::insert::NewObject::BASIC_TABLE).unwrap();
        d.end(&mut h);
        let base_dangling = dangling(&d.file);
        let before = d.file.to_zip().unwrap();
        d.begin(&mut h, "Cell");
        d.set_cell(&mut h, a, 1, 1, &CellInput::Number(42.0)).unwrap();
        d.end(&mut h);
        d.begin(&mut h, "Add Column Before");
        d.table_insert(&mut h, a, Axis::Column, 1, 1).unwrap();
        d.end(&mut h);
        d.begin(&mut h, "Add Row Above");
        d.table_insert(&mut h, a, Axis::Row, 1, 1).unwrap();
        d.end(&mut h);
        let d2 = reopen(&d);
        let x = data(&d2, 0, a);
        assert_eq!((x.rows, x.cols), (6, 5));
        assert_eq!(x.cells[(2 * 5 + 2) as usize], "42");
        assert_eq!((data(&d2, 0, b).rows, data(&d2, 0, b).cols), (5, 4));
        // paste_table doesn't give inserted tables tracked header cells, so none get added
        assert_eq!(tracked_count(&d2, a), 0);
        assert_eq!(dangling(&d2.file), base_dangling);
        while d.undo(&mut h) {
            if d.file.to_zip().unwrap() == before {
                break;
            }
        }
        assert_eq!(d.file.to_zip().unwrap(), before);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn plain_keynote_tables() {
        for f in ["Keynote-Feature-Catalog.key", "05-table.key"] {
            plain_tables_of(f);
        }
    }

    fn plain_tables_of(f: &str) {
        let mut d = open(f);
        let base_dangling = dangling(&d.file);
        let mut h = History::default();
        for (i, t) in tables(&d) {
            if d.table_edit_blocker(t).is_some() {
                continue;
            }
            let x0 = data(&d, i, t);
            d.begin(&mut h, "Add Column Before");
            d.table_insert(&mut h, t, Axis::Column, 0, 0).unwrap();
            d.end(&mut h);
            d.begin(&mut h, "Add Row Below");
            d.table_insert(&mut h, t, Axis::Row, x0.rows, x0.rows - 1).unwrap();
            d.end(&mut h);
            let x = data(&d, i, t);
            assert_eq!((x.rows, x.cols), (x0.rows + 1, x0.cols + 1));
            assert_eq!(x.header_cols, x0.header_cols + u32::from(x0.header_cols > 0));
            assert_eq!(x.footer_rows, x0.footer_rows + u32::from(x0.footer_rows > 0));
            for r in 0..x0.rows {
                for c in 0..x0.cols {
                    assert_eq!(x.cells[(r * x.cols + c + 1) as usize], x0.cells[(r * x0.cols + c) as usize]);
                }
            }
            d.begin(&mut h, "Delete Column");
            d.table_delete(&mut h, t, Axis::Column, 0).unwrap();
            d.end(&mut h);
            d.begin(&mut h, "Delete Row");
            d.table_delete(&mut h, t, Axis::Row, x0.rows).unwrap();
            d.end(&mut h);
            let x = data(&d, i, t);
            assert_eq!(x.cells, x0.cells);
            assert_eq!(x.merges, x0.merges);
        }
        let d2 = reopen(&d);
        assert_eq!(dangling(&d2.file), base_dangling);
    }

    #[test]
    #[ignore = "merged tables are refused until the merge update is fixed (Keynote hangs, Mac round 8)"]
    fn merged_cells_grow_and_shrink() {
        // catalog slide 27: rows 2..7 of column 2 are merged
        let mut d = open("Keynote-Feature-Catalog.key");
        let (i, t) = *tables(&d).iter().find(|(i, _)| *i == 26).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Add Row Above");
        d.table_insert(&mut h, t, Axis::Row, 4, 4).unwrap();
        d.end(&mut h);
        assert_eq!(data(&reopen(&d), i, t).merges, vec![crate::CellRange { row: 2, col: 2, rows: 7, cols: 1 }]);
        d.begin(&mut h, "Add Row Above");
        d.table_insert(&mut h, t, Axis::Row, 1, 1).unwrap();
        d.end(&mut h);
        d.begin(&mut h, "Delete Row");
        d.table_delete(&mut h, t, Axis::Row, 5).unwrap();
        d.end(&mut h);
        assert_eq!(data(&d, i, t).merges, vec![crate::CellRange { row: 3, col: 2, rows: 6, cols: 1 }]);
        // deleting the merged column would remove the merge
        d.begin(&mut h, "Delete Column");
        assert!(d.table_delete(&mut h, t, Axis::Column, 2).is_err());
        d.end(&mut h);
        let p = parts(&d.file, t).unwrap();
        let rt = d.file.obj(p.merge_tiles[0]).unwrap();
        let rect = rt.msgs("from_to_range").next().unwrap().msg("refers_to_rect").unwrap();
        assert_eq!((rect.msg("origin").unwrap().u32("row"), rect.msg("size").unwrap().u32("num_rows")), (Some(3), Some(6)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeaderKind {
    Rows,
    Columns,
    Footer,
}

#[cfg(test)]
mod formula_move_tests {
    use super::*;
    use crate::DrawableKind;

    /// Adding / deleting rows and columns in a table with formulas: they move with their
    /// cells and keep pointing at the same values (R30's 12 formulas).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn formulas_follow_their_cells() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R30-formulas.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        let before: Vec<String> = (1..=12).map(|r| format!("{:?}", d.cell_values(t).get(&(r, 2)))).collect();
        assert_eq!(d.table_edit_blocker(t), None);
        // a row above row 2: everything moves down one
        d.begin(&mut h, "Add Row");
        d.table_insert(&mut h, t, Axis::Row, 1, 1).unwrap();
        d.end(&mut h);
        assert_eq!(d.formula_text(t, 2, 2).as_deref(), Some("=B3+B4"));
        assert_eq!(d.formula_text(t, 6, 2).as_deref(), Some("=SUM(B3:B5)×2"));
        let after: Vec<String> = (2..=13).map(|r| format!("{:?}", d.cell_values(t).get(&(r, 2)))).collect();
        assert_eq!(before, after, "same results after the move");
        // a column before B: references move right
        d.begin(&mut h, "Add Column");
        d.table_insert(&mut h, t, Axis::Column, 1, 1).unwrap();
        d.end(&mut h);
        assert_eq!(d.formula_text(t, 2, 3).as_deref(), Some("=C3+C4"));
        // saved and reopened, still the same
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d2.formula_text(t, 2, 3).as_deref(), Some("=C3+C4"));
        let after2: Vec<String> = (2..=13).map(|r| format!("{:?}", d2.cell_values(t).get(&(r, 3)))).collect();
        assert_eq!(before, after2);
        // deleting a row a formula reads: that reference becomes an error, nothing breaks
        d.begin(&mut h, "Delete Row");
        d.table_delete(&mut h, t, Axis::Row, 3).unwrap();
        d.end(&mut h);
        assert!(d.formula_text(t, 2, 3).is_some_and(|f| f.contains("#REF!")));
    }
}

impl Document {
    /// Move cell `from` of `table` to `to` (an empty cell) as a whole: its value with its kind
    /// (number, date, duration, bool, text) or its formula (the references as they were), its
    /// data format (custom ones too) and its look (fill, text style, alignment).
    pub(crate) fn move_cell(&mut self, h: &mut History, table: ObjId, from: (u32, u32), to: (u32, u32)) -> Result<(), kn_format::Error> {
        use crate::format::{FillSpec, TextProp};
        use crate::table::CellValue;
        use crate::tablecells::{CellAlign, CellInput};
        use crate::tablefmt::CellFormat;
        let value = self.cell_values(table).get(&from).cloned().unwrap_or(CellValue::Empty);
        let formula = self.formula_text(table, from.0, from.1);
        let format = self.cell_format(table, from.0, from.1);
        let style = self.file.obj(table).and_then(|m| crate::table::table(&self.file, m)).and_then(|t| t.cell_styles.get((from.0 * t.cols + from.1) as usize).cloned());
        match (&formula, &value) {
            (Some(f), _) => self.set_formula_text(h, table, to.0, to.1, f)?,
            (None, CellValue::Empty) => {}
            (None, CellValue::Number(x)) => self.set_cell(h, table, to.0, to.1, &CellInput::Number(*x))?,
            (None, CellValue::Text(t)) => self.set_cell(h, table, to.0, to.1, &CellInput::Text(t.clone()))?,
            (None, v) => {
                if let Some(raw) = crate::tablefmt::typed_value_record(v) {
                    self.set_cell(h, table, to.0, to.1, &CellInput::Raw(raw))?;
                }
            }
        }
        let b = (to.0, to.1, to.0, to.1);
        if format != CellFormat::Automatic {
            self.set_cells_format(h, table, b, &format)?;
        }
        if let Some(st) = style {
            if let crate::Fill::Color(c) = st.fill {
                self.set_cells_fill(h, table, b, Some(&FillSpec::Color(c)))?;
            }
            // an empty cell has no text to style (kn-model refuses it): its fill is all it has
            if formula.is_none() && value == CellValue::Empty {
                return Ok(());
            }
            let t = &st.text;
            for p in [TextProp::Font(t.font_name.clone()), TextProp::Size(t.size), TextProp::Bold(t.bold), TextProp::Italic(t.italic), TextProp::Underline(t.underline), TextProp::Strike(t.strike), TextProp::Color(t.color), TextProp::VAlign(st.valign)] {
                self.set_cells_text_prop(h, table, b, &p)?;
            }
            let a = match st.align {
                crate::Align::Center => CellAlign::Center,
                crate::Align::Right => CellAlign::Right,
                crate::Align::Justify => CellAlign::Justify,
                crate::Align::Natural => CellAlign::Auto,
                crate::Align::Left => CellAlign::Left,
            };
            self.set_cells_alignment(h, table, b, a)?;
        }
        Ok(())
    }
}

/// Where a merge goes when a row / column is added or removed (None: it is no longer a merge).
fn moved_merge(m: crate::CellRange, axis: Axis, op: Op) -> Option<crate::CellRange> {
    let (lo, n) = if axis == Axis::Row { (m.row, m.rows) } else { (m.col, m.cols) };
    let (lo, n) = match op {
        Op::Insert(at) if at <= lo => (lo + 1, n),
        Op::Insert(at) if at < lo + n => (lo, n + 1),
        Op::Delete(i) if i < lo => (lo - 1, n),
        Op::Delete(i) if i < lo + n => (lo, n - 1),
        _ => (lo, n),
    };
    let out = if axis == Axis::Row { crate::CellRange { row: lo, rows: n, ..m } } else { crate::CellRange { col: lo, cols: n, ..m } };
    (n > 0 && out.rows * out.cols >= 2).then_some(out)
}

#[cfg(test)]
mod merge_anchor_tests {
    use super::*;
    use crate::format::FillSpec;
    use crate::table::CellValue;
    use crate::tablecells::CellInput;
    use crate::tablefmt::CellFormat;
    use crate::{CellRange, Color, Fill};

    fn first_table(d: &Document) -> ObjId {
        let s = d.slides();
        d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, crate::DrawableKind::Table(_))).unwrap().id
    }

    /// Deleting a merge's first row or first column moves its first cell as a whole: the
    /// value keeps its kind (number, date), its data format and its fill (Numbers, Mac N19).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn anchor_kind_format_and_fill_survive() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-merge-1-merged.key");
        let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let date_secs = 791_078_400.0; // 2026-01-25
        for (axis, kind) in [(Axis::Row, 0), (Axis::Column, 0), (Axis::Row, 1), (Axis::Column, 1)] {
            let mut d = Document::open(&path).unwrap();
            let t = first_table(&d);
            let mut h = History::default();
            d.begin(&mut h, "set");
            let fmt = if kind == 0 {
                d.set_cell(&mut h, t, 1, 1, &CellInput::Number(42.0)).unwrap();
                CellFormat::Currency { code: "EUR".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false }
            } else {
                d.set_cell(&mut h, t, 1, 1, &CellInput::Raw(crate::tablefmt::typed_value_record(&CellValue::Date(date_secs)).unwrap())).unwrap();
                CellFormat::DateTime { pattern: "d MMM yyyy".into() }
            };
            d.set_cells_format(&mut h, t, (1, 1, 1, 1), &fmt).unwrap();
            d.set_cells_fill(&mut h, t, (1, 1, 1, 1), Some(&FillSpec::Color(red))).unwrap();
            d.end(&mut h);
            let table = |d: &Document| crate::table::table(&d.file, d.file.obj(t).unwrap()).unwrap();
            assert_eq!(table(&d).merges, vec![CellRange { row: 1, col: 1, rows: 2, cols: 2 }]);
            // the merge's first row / column goes
            d.begin(&mut h, "delete");
            d.table_delete(&mut h, t, axis, 1).unwrap();
            d.end(&mut h);
            let td = table(&d);
            let want = if axis == Axis::Row { CellRange { row: 1, col: 1, rows: 1, cols: 2 } } else { CellRange { row: 1, col: 1, rows: 2, cols: 1 } };
            assert_eq!(td.merges, vec![want], "{axis:?} {kind}");
            let v = d.cell_values(t).get(&(1, 1)).cloned();
            assert_eq!(v, Some(if kind == 0 { CellValue::Number(42.0) } else { CellValue::Date(date_secs) }), "{axis:?} {kind}");
            assert_eq!(d.cell_format(t, 1, 1), fmt, "{axis:?} {kind}");
            assert_eq!(td.cell_styles[(1 * td.cols + 1) as usize].fill, Fill::Color(red), "{axis:?} {kind}");
        }
    }
}
