//! Categories (Organize > Categories): the table's rows shown in groups, each under a group row
//! with the group's value (research 30-category, screenshot N-category-by-group).
//!
//! TableModel.category_owner -> TST.CategoryOwnerRefArchive.group_by -> TST.GroupByArchive
//! {is_enabled, group_column, group_node_root.child[] (one per group: group_cell_value and
//! row_lookup_uids = an index set into row_uid_lookup.uuids)}; the row UIDs map to rows through
//! base_column_row_uids. Groups show in the order of their first row (Numbers' default).
//!
//! This is the view: the model keeps its rows; [`View`] maps the rows shown back to them.

use std::collections::HashMap;

use kn_format::{KeyFile, Msg, ObjId};
use kn_model::{Align, CellRange, Color, Fill, Stroke, TableCellStyle, TableData};

/// One group: its label ("Office", "(blank)") and its rows in the model, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub label: String,
    pub rows: Vec<u32>,
}

/// Rows as shown: `Some(model row)`, or `None` for a group row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub rows: Vec<Option<u32>>,
    /// The label of each group row (same index as `rows`).
    pub labels: HashMap<usize, String>,
}

impl View {
    /// The shown row of a model row.
    pub fn view_row(&self, model: u32) -> Option<u32> {
        self.rows.iter().position(|r| *r == Some(model)).map(|i| i as u32)
    }
}

pub(crate) fn row_index_map(k: &KeyFile, model: &Msg) -> HashMap<[u8; 16], u32> {
    use kn_format::tables::uuid_of_tsp;
    let Some(m) = model.ref_id("base_column_row_uids").and_then(|m| k.obj(m)) else { return HashMap::new() };
    m.msgs("sorted_row_uids").zip(m.u64s("row_index_for_uid")).map(|(u, i)| (uuid_of_tsp(u), i as u32)).collect()
}

fn label(v: Option<&Msg>) -> String {
    let Some(v) = v else { return "(blank)".into() };
    match v.enum_name("cell_value_type") {
        Some("STRING_TYPE") => v.msg("string_value").and_then(|s| s.str("value")).unwrap_or("").to_string(),
        Some("NUMBER_TYPE") => v.msg("number_value").and_then(|n| n.f64("value")).map(kn_model::table::fmt_num).unwrap_or_default(),
        Some("BOOLEAN_TYPE") => v.msg("boolean_value").and_then(|b| b.bool("value")).map(|b| if b { "TRUE" } else { "FALSE" }.to_string()).unwrap_or_default(),
        Some("DATE_TYPE") => v.msg("date_value").and_then(|d| d.f64("value")).map(|s| kn_model::table::format_date(s, "M/d/yy")).unwrap_or_default(),
        _ => "(blank)".into(),
    }
}

/// The groups of a categorized table (None when categories are off).
pub fn groups(k: &KeyFile, info: ObjId) -> Option<Vec<Group>> {
    use kn_format::tables::uuid_of_tsp;
    let model = k.obj(info)?.ref_id("tableModel").and_then(|t| k.obj(t))?;
    let owner = model.ref_id("category_owner").and_then(|c| k.obj(c))?;
    let gb = owner.ref_ids("group_by").into_iter().next().and_then(|g| k.obj(g))?;
    if gb.bool("is_enabled") != Some(true) || gb.count("group_column") == 0 {
        return None;
    }
    let lookup: Vec<[u8; 16]> = gb.msg("row_uid_lookup").map(|l| l.msgs("uuids").map(uuid_of_tsp).collect()).unwrap_or_default();
    let rows_of = row_index_map(k, model);
    let mut out = Vec::new();
    for g in gb.msg("group_node_root")?.msgs("child") {
        let mut rows = Vec::new();
        for e in g.msg("row_lookup_uids").into_iter().flat_map(|s| s.msgs("entries")) {
            let b = e.i64("range_begin").unwrap_or(0);
            let end = e.i64("range_end").unwrap_or(b);
            for i in b..=end {
                if let Some(r) = lookup.get(i as usize).and_then(|u| rows_of.get(u)) {
                    rows.push(*r);
                }
            }
        }
        rows.sort_unstable();
        out.push(Group { label: label(g.msg("group_cell_value")), rows });
    }
    out.retain(|g| !g.rows.is_empty());
    out.sort_by_key(|g| g.rows[0]);
    Some(out)
}

/// Rearrange `t` as Numbers shows a categorized table: header rows, then each group's row and
/// its rows, then the footer rows. Returns the view's row map.
pub fn apply(t: &mut TableData, groups: &[Group]) -> View {
    let body_end = t.rows - t.footer_rows;
    let mut rows: Vec<Option<u32>> = (0..t.header_rows).map(Some).collect();
    let mut labels = HashMap::new();
    for g in groups {
        labels.insert(rows.len(), g.label.clone());
        rows.push(None);
        rows.extend(g.rows.iter().copied().filter(|r| *r >= t.header_rows && *r < body_end).map(Some));
    }
    rows.extend((body_end..t.rows).map(Some));
    let cols = t.cols as usize;
    let old = t.clone();
    let line = Stroke { color: Color::BLACK, width: 1.0, cap: kn_model::LineCap::Butt, join: kn_model::LineJoin::Miter, miter_limit: 4.0, dash: Vec::new(), phase: 0.0, brush: None };
    let group_style = TableCellStyle {
        fill: Fill::Color(Color::rgba(0.85, 0.85, 0.85, 1.0)),
        text: kn_model::CharStyle { bold: true, ..old.cell_styles.get(cols).map(|s| s.text.clone()).unwrap_or_default() },
        align: Align::Left,
        ..Default::default()
    };
    t.rows = rows.len() as u32;
    t.row_heights.clear();
    t.cells.clear();
    t.cell_styles.clear();
    t.h_strokes.clear();
    t.v_strokes.clear();
    t.merges.clear();
    for (i, r) in rows.iter().enumerate() {
        match r {
            Some(r) => {
                let r = *r as usize;
                t.row_heights.push(old.row_heights.get(r).copied().unwrap_or(20.0));
                t.cells.extend_from_slice(&old.cells[r * cols..(r + 1) * cols]);
                if !old.cell_styles.is_empty() {
                    t.cell_styles.extend_from_slice(&old.cell_styles[r * cols..(r + 1) * cols]);
                }
                t.v_strokes.extend((0..=cols).map(|b| old.v_strokes.get(r * (cols + 1) + b).cloned().flatten()));
            }
            None => {
                t.row_heights.push(old.row_heights.get(t.header_rows as usize).copied().unwrap_or(20.0).max(20.0));
                // (a collapsed group comes labelled "▶ …" already)
                let l = &labels[&i];
                t.cells.push(if l.starts_with('▶') || l.starts_with('▼') { l.clone() } else { format!("▼ {l}") });
                t.cells.extend((1..cols).map(|_| String::new()));
                if !old.cell_styles.is_empty() {
                    t.cell_styles.extend((0..cols).map(|_| group_style.clone()));
                }
                t.v_strokes.extend((0..=cols).map(|b| if b == 0 || b == cols { Some(line.clone()) } else { None }));
                t.merges.push(CellRange { row: i as u32, col: 0, rows: 1, cols: cols as u32 });
            }
        }
    }
    // horizontal lines: a group row gets a heavier line above it; others keep the line above them
    for i in 0..=rows.len() {
        let above = i.checked_sub(1).and_then(|j| rows[j]);
        let here = rows.get(i).copied().flatten();
        for c in 0..cols {
            let s = if i < rows.len() && rows[i].is_none() && i > 0 {
                Some(Stroke { width: 2.0, ..line.clone() })
            } else if let Some(r) = here {
                old.h_strokes.get(r as usize * cols + c).cloned().flatten()
            } else if let Some(r) = above {
                old.h_strokes.get((r as usize + 1) * cols + c).cloned().flatten()
            } else {
                Some(line.clone())
            };
            t.h_strokes.push(s);
        }
    }
    // merges of the model rows that stay next to each other
    for m in &old.merges {
        if let Some(v) = rows.iter().position(|r| *r == Some(m.row)) {
            let contiguous = (0..m.rows).all(|k| rows.get(v + k as usize) == Some(&Some(m.row + k)));
            if contiguous {
                t.merges.push(CellRange { row: v as u32, ..*m });
            }
        }
    }
    View { rows, labels }
}
