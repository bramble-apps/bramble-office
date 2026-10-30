//! Apple Numbers (`.numbers`, Numbers 15.4) documents on top of Keydeck's iWork crates.
//!
//! The container, IWA framing and the shared TST/TSCE/TSCH/TSWP/TSD packages are the same as
//! in Keynote, so `kn-format` reads and writes the file once [`init`] has swapped in the Numbers
//! schema and registry (TN.* in place of KN.*), and `kn-model` builds tables, charts and shapes.
//! What is Numbers-specific lives here: sheets (TN.SheetArchive), each one canvas of drawables.

use kn_format::{KeyFile, ObjId};
use kn_model::{Document, Drawable, Geometry};

pub mod categories;
pub mod chartlink;
pub mod charts;
pub mod comments;
pub mod controls;
pub mod formula;
pub mod import;
pub mod organize;
pub mod pivot;
pub mod sheets;
pub mod styles;
pub mod typed;
pub mod xlsx;
pub mod xlstyle;

pub use kn_model;
pub use kn_format;

/// Numbers' Blank template (one sheet, one 22 × 7 table): new documents, and the donor of new
/// tables. It is Apple's artwork, for local use only (see keynote-linux licensing notes).
pub const BLANK: &[u8] = include_bytes!("../data/Blank.numbers");

/// Load the Numbers schema; call once before opening any file.
pub fn init() {
    kn_format::schema::use_schema(include_str!("../data/schema.json"), include_str!("../data/registry.json"));
}

/// One sheet: its name and the drawables on its canvas, back to front.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub id: ObjId,
    pub name: String,
    pub drawables: Vec<ObjId>,
    /// Format > Sheet > Hide: not shown in the tab bar (TN.SheetArchive.is_hidden).
    pub hidden: bool,
}

/// A table on a sheet: the TST.TableInfoArchive id and where it sits.
#[derive(Debug, Clone)]
pub struct TableRef {
    pub info: ObjId,
    pub name: String,
    pub geom: Geometry,
}

pub struct Workbook {
    pub doc: Document,
}

impl Workbook {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Workbook, kn_format::Error> {
        init();
        Ok(Workbook { doc: Document::open(path)? })
    }

    pub fn file(&self) -> &KeyFile {
        &self.doc.file
    }

    /// Sheets in tab order (TN.DocumentArchive.sheets).
    pub fn sheets(&self) -> Vec<Sheet> {
        let k = self.file();
        let Some((_, d)) = k.first_of("TN.DocumentArchive") else { return Vec::new() };
        d.ref_ids("sheets")
            .into_iter()
            .filter_map(|id| {
                let s = k.obj(id)?;
                Some(Sheet { id, name: s.str("name").unwrap_or("").to_string(), drawables: s.ref_ids("drawable_infos"), hidden: s.bool("is_hidden").unwrap_or(false) })
            })
            .collect()
    }

    /// The tables of a sheet, in canvas order.
    pub fn tables(&self, sheet: &Sheet) -> Vec<TableRef> {
        let k = self.file();
        sheet
            .drawables
            .iter()
            .filter(|id| k.type_name(**id) == "TST.TableInfoArchive")
            .filter_map(|&info| {
                let m = k.obj(info)?;
                let name = m.ref_id("tableModel").and_then(|t| k.obj(t)).and_then(|t| t.str("table_name")).unwrap_or("").to_string();
                let geom = m.find_super("TSD.DrawableArchive").map(kn_model::scene::geometry).unwrap_or_default();
                Some(TableRef { info, name, geom })
            })
            .collect()
    }

    /// Every drawable of a sheet as Keydeck's renderer understands it (tables, charts, shapes, text, images).
    pub fn drawables(&self, sheet: &Sheet) -> Vec<Drawable> {
        self.drawables_and_views(sheet).0
    }

    /// The sheet's drawables as shown (hidden rows and columns at size 0, categorized tables in
    /// their groups) and, for each categorized table, how its shown rows map to the model's.
    pub fn drawables_and_views(&self, sheet: &Sheet) -> (Vec<Drawable>, std::collections::HashMap<ObjId, categories::View>) {
        let ctx = kn_model::text::TextContext { slide_number: 1 };
        let mut views = std::collections::HashMap::new();
        let drawables = sheet
            .drawables
            .iter()
            .filter_map(|&id| {
                let mut d = kn_model::scene::drawable(self.file(), id, ctx)?;
                if let kn_model::DrawableKind::Table(t) = &mut d.kind {
                    self.hide(id, t);
                    if let Some(g) = categories::groups(self.file(), id) {
                        views.insert(id, categories::apply(t, &g));
                    }
                }
                Some(d)
            })
            .collect();
        (drawables, views)
    }

    pub fn table_data(&self, info: ObjId) -> Option<kn_model::TableData> {
        let mut t = kn_model::table::table(self.file(), self.file().obj(info)?)?;
        self.hide(info, &mut t);
        Some(t)
    }

    /// Rows and columns hidden by the user or by a filter get size 0.
    fn hide(&self, info: ObjId, t: &mut kn_model::TableData) {
        let (rows, cols) = hidden(self.file(), info);
        for r in rows {
            if let Some(h) = t.row_heights.get_mut(r as usize) {
                *h = 0.0;
            }
        }
        for c in cols {
            if let Some(w) = t.col_widths.get_mut(c as usize) {
                *w = 0.0;
            }
        }
    }
}

/// Hidden rows and columns of a table: TableModel.hidden_states_owner lists, by row / column
/// UID, the ones the user hid and the ones a filter hides (research 31-filter); the UIDs map to
/// indices through base_column_row_uids. Header buckets' hidingState (Keynote) counts too.
pub fn hidden(k: &KeyFile, info: ObjId) -> (Vec<u32>, Vec<u32>) {
    use kn_format::tables::uuid_of_tsp;
    let Some(model) = k.obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)) else { return Default::default() };
    let map = model.ref_id("base_column_row_uids").and_then(|m| k.obj(m));
    let index_of = |field_uids: &str, field_idx: &str| -> std::collections::HashMap<[u8; 16], u32> {
        let Some(m) = map else { return Default::default() };
        let idx = m.u64s(field_idx);
        m.msgs(field_uids).zip(idx).map(|(u, i)| (uuid_of_tsp(u), i as u32)).collect()
    };
    let row_ix = index_of("sorted_row_uids", "row_index_for_uid");
    let col_ix = index_of("sorted_column_uids", "column_index_for_uid");
    let (mut rows, mut cols) = (Vec::new(), Vec::new());
    for hs in model.msg("hidden_states_owner").into_iter().flat_map(|o| o.msgs("hidden_states")) {
        for (extent, ix, out) in [("row_hidden_state_extent", &row_ix, &mut rows), ("column_hidden_state_extent", &col_ix, &mut cols)] {
            for st in hs.msg(extent).into_iter().flat_map(|e| e.msgs("base_hidden_states")) {
                if st.bool("user_hidden") == Some(true) || st.bool("filtered") == Some(true) {
                    if let Some(i) = st.msg("row_or_column_uid").and_then(|u| ix.get(&uuid_of_tsp(u))) {
                        out.push(*i);
                    }
                }
            }
        }
    }
    // Keynote-style hiding in the header buckets
    if let Some(ds) = model.msg("base_data_store") {
        let mut buckets: Vec<(ObjId, bool)> = ds.msg("rowHeaders").map(|r| r.ref_ids("buckets")).unwrap_or_default().into_iter().map(|b| (b, true)).collect();
        buckets.extend(ds.ref_id("columnHeaders").map(|b| (b, false)));
        for (b, is_row) in buckets {
            for h in k.obj(b).into_iter().flat_map(|m| m.msgs("headers")) {
                if h.u32("hidingState").unwrap_or(0) != 0 {
                    if let Some(i) = h.u32("index") {
                        if is_row { rows.push(i) } else { cols.push(i) }
                    }
                }
            }
        }
    }
    rows.sort_unstable();
    rows.dedup();
    cols.sort_unstable();
    cols.dedup();
    (rows, cols)
}

/// The rows a filter hides (their hidden state's `filtered` flag), in order.
pub fn filtered(k: &KeyFile, info: ObjId) -> Vec<u32> {
    use kn_format::tables::uuid_of_tsp;
    let Some(model) = k.obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)) else { return Vec::new() };
    let ix = categories::row_index_map(k, model);
    let mut out: Vec<u32> = model
        .msg("hidden_states_owner")
        .into_iter()
        .flat_map(|o| o.msgs("hidden_states"))
        .flat_map(|hs| hs.msg("row_hidden_state_extent").into_iter().flat_map(|e| e.msgs("base_hidden_states")))
        .filter(|st| st.bool("filtered") == Some(true))
        .filter_map(|st| st.msg("row_or_column_uid").and_then(|u| ix.get(&uuid_of_tsp(u))).copied())
        .collect();
    out.sort_unstable();
    out
}

/// Comments on cells of a table: (row, column) -> (text, author). The cell record's flag
/// 0x80000 holds a key into the table's COMMENT_STORAGE list, whose entries point to
/// TSD.CommentStorageArchive (research 34-comment).
pub fn cell_comments(k: &KeyFile, info: ObjId) -> std::collections::HashMap<(u32, u32), (String, String)> {
    let mut out = std::collections::HashMap::new();
    let Some(model) = k.obj(info).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)) else { return out };
    let Some(list) = model.msg("base_data_store").and_then(|d| d.ref_id("commentStorageTable")).and_then(|l| k.obj(l)) else { return out };
    let storages: std::collections::HashMap<u32, ObjId> = list.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.ref_id("comment_storage")?))).collect();
    if storages.is_empty() {
        return out;
    }
    for ((r, c), rec) in kn_model::table::raw_cell_records(k, model) {
        if rec.first() != Some(&5) || rec.len() < 12 {
            continue;
        }
        let flags = u32::from_le_bytes(rec[8..12].try_into().unwrap());
        if flags & 0x80000 == 0 {
            continue;
        }
        let mut at = 12;
        for bit in 0..19 {
            if flags & (1 << bit) != 0 {
                at += match bit {
                    0 => 16,
                    1 | 2 => 8,
                    _ => 4,
                };
            }
        }
        let Some(key) = rec.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { continue };
        let Some(cs) = storages.get(&key).and_then(|s| k.obj(*s)) else { continue };
        let author = cs.ref_id("author").and_then(|a| k.obj(a)).and_then(|a| a.str("name")).unwrap_or("").to_string();
        out.insert((r, c), (cs.str("text").unwrap_or("").to_string(), author));
    }
    out
}

/// How a sheet was last shown: its zoom and the scroll position (TN.SheetUIStateArchive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetView {
    pub zoom: f32,
    /// Top-left of the view; in screen points when `unscaled` is false (divide by the zoom
    /// for sheet points).
    pub scroll: (f32, f32),
    pub unscaled: bool,
}

/// The document's saved window state (TN.UIStateArchive): the open sheet, whether the
/// inspector was shown, and each sheet's zoom and scroll position (Numbers restores them).
#[derive(Clone, Debug, Default)]
pub struct UiState {
    pub active_sheet: usize,
    pub inspector: Option<bool>,
    pub sheets: std::collections::HashMap<ObjId, SheetView>,
}

pub fn ui_state(k: &KeyFile) -> Option<UiState> {
    let ui = k.objects_of("TN.UIStateArchive").next().map(|(_, m)| m).or_else(|| k.first_of("TN.DocumentArchive").and_then(|(_, d)| d.msg("uistate")))?;
    let mut out = UiState { active_sheet: ui.u32("active_sheet_index").unwrap_or(0) as usize, inspector: ui.bool("inspector_pane_visible"), ..Default::default() };
    // older files key the sheets by reference, newer ones by the sheet's uuid
    let by_uuid = ui.msgs("uuid_sheet_uistate_dictionary").filter_map(|e| {
        let b = kn_format::tables::uuid_of_tsp(e.msg("sheet_uuid")?);
        let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
        let u = format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]);
        Some((k.object_by_uuid(&u)?, e.msg("sheet_uistate")?))
    });
    let by_ref = ui.msgs("sheet_uistate_dictionary_entry").filter_map(|e| Some((e.ref_id("sheet")?, e.msg("sheet_uistate")?)));
    for (sheet, st) in by_ref.chain(by_uuid) {
        let p = st.msg("scroll_position");
        let scroll = (p.and_then(|p| p.f32("x")).unwrap_or(0.0), p.and_then(|p| p.f32("y")).unwrap_or(0.0));
        out.sheets.insert(sheet, SheetView { zoom: st.f32("view_scale").unwrap_or(1.0), scroll, unscaled: st.bool("scroll_position_is_unscaled").unwrap_or(false) });
    }
    Some(out)
}

/// Table > Hide Row / Hide Column / Unhide All: rows (or columns) hidden by hand are listed
/// by uid in the table's hidden states with user_hidden (filtered ones keep their own flag).
pub fn set_hidden(doc: &mut kn_model::Document, h: &mut kn_model::edit::History, info: ObjId, rows: bool, indices: &[u32], hide: bool) -> Result<(), kn_format::Error> {
    set_hidden_flag(doc, h, info, rows, indices, hide, "user_hidden", false)
}

/// Organize > Filter: exactly the rows `indices` are the ones a filter hides (the `filtered`
/// flag of the table's hidden states; rows hidden by hand stay hidden).
pub fn set_filtered(doc: &mut kn_model::Document, h: &mut kn_model::edit::History, info: ObjId, indices: &[u32]) -> Result<(), kn_format::Error> {
    set_hidden_flag(doc, h, info, true, indices, true, "filtered", true)
}

/// Set (or clear) `flag` on the hidden states of `indices`; `only` first clears it everywhere
/// else.
#[allow(clippy::too_many_arguments)]
fn set_hidden_flag(doc: &mut kn_model::Document, h: &mut kn_model::edit::History, info: ObjId, rows: bool, indices: &[u32], hide: bool, flag: &str, only: bool) -> Result<(), kn_format::Error> {
    use kn_format::tables::uuid_of_tsp;
    let err = |s: &str| kn_format::Error::Invalid(s.into());
    let model_id = doc.file.obj(info).and_then(|m| m.ref_id("tableModel")).ok_or(err("no table model"))?;
    let model = doc.file.obj(model_id).ok_or(err("no table model"))?;
    let map = model.ref_id("base_column_row_uids").and_then(|m| doc.file.obj(m)).ok_or(err("no row / column uids"))?;
    let (uids_f, idx_f) = if rows { ("sorted_row_uids", "row_index_for_uid") } else { ("sorted_column_uids", "column_index_for_uid") };
    // index -> uid message
    let by_index: std::collections::HashMap<u32, kn_format::Msg> = map.msgs(uids_f).zip(map.u64s(idx_f)).map(|(u, i)| (i as u32, u.clone())).collect();
    // Numbers keeps the states in the uid map's sorted order (Mac N17)
    let order: Vec<[u8; 16]> = map.msgs(uids_f).map(uuid_of_tsp).collect();
    let want: Vec<kn_format::Msg> = indices.iter().filter_map(|i| by_index.get(i).cloned()).collect();
    if model.msg("hidden_states_owner").and_then(|o| o.msgs("hidden_states").next()).is_none() {
        return Err(err("this table keeps no hidden states"));
    }
    doc.touch(h, model_id);
    let m = doc.file.obj_mut(model_id).ok_or(err("no table model"))?;
    let hs = m.msg_mut("hidden_states_owner").and_then(|o| o.msgs_mut("hidden_states").next()).ok_or(err("no hidden states"))?;
    let extent = hs.msg_mut_or_create(if rows { "row_hidden_state_extent" } else { "column_hidden_state_extent" });
    if only {
        for st in extent.msgs_mut("base_hidden_states") {
            st.remove(flag);
        }
    }
    for u in want {
        let key = uuid_of_tsp(&u);
        let existing = extent.msgs_mut("base_hidden_states").find(|st| st.msg("row_or_column_uid").map(uuid_of_tsp) == Some(key));
        match (existing, hide) {
            (Some(st), true) => st.set_bool(flag, true),
            (Some(st), false) => st.remove(flag),
            (None, true) => {
                let st = extent.push_new("base_hidden_states");
                st.push_msg("row_or_column_uid", u);
                st.set_bool(flag, true);
            }
            (None, false) => {}
        }
    }
    // states that hide nothing any more go
    extent.retain_msgs("base_hidden_states", |st| st.bool("user_hidden") == Some(true) || st.bool("filtered") == Some(true) || st.bool("pivot_hidden") == Some(true));
    let mut states: Vec<kn_format::Msg> = extent.msgs("base_hidden_states").cloned().collect();
    states.sort_by_key(|st| st.msg("row_or_column_uid").map(uuid_of_tsp).and_then(|u| order.iter().position(|o| *o == u)).unwrap_or(usize::MAX));
    extent.remove("base_hidden_states");
    for st in states {
        extent.push_msg("base_hidden_states", st);
    }
    Ok(())
}
