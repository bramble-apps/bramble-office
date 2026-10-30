//! Pivot tables (Organize > Create Pivot Table): a source table's body rows summarized by the
//! values of some columns (Rows), split by others (Columns), with summaries of value columns
//! (Values: Sum, Average, Count, …) and grand totals, as Numbers lays them out.

use std::collections::BTreeMap;

use kn_model::TableData;

/// How a value column is summarized (Numbers' "Summarize by", in its menu's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Summary {
    Sum,
    Average,
    /// Non-empty cells (COUNTA).
    CountAll,
    Maximum,
    Minimum,
    /// Cells with numbers (COUNT).
    Count,
    CountUnique,
    Median,
    Product,
    Stdev,
    StdevP,
    Var,
    VarP,
}

impl Summary {
    pub const ALL: [Summary; 13] = [
        Summary::Sum,
        Summary::Average,
        Summary::CountAll,
        Summary::Maximum,
        Summary::Minimum,
        Summary::Count,
        Summary::CountUnique,
        Summary::Median,
        Summary::Product,
        Summary::Stdev,
        Summary::StdevP,
        Summary::Var,
        Summary::VarP,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Summary::Sum => "Sum",
            Summary::Average => "Average",
            Summary::CountAll => "Count (All)",
            Summary::Maximum => "Maximum",
            Summary::Minimum => "Minimum",
            Summary::Count => "Count",
            Summary::CountUnique => "Count (Unique)",
            Summary::Median => "Median",
            Summary::Product => "Product",
            Summary::Stdev => "STDEV",
            Summary::StdevP => "STDEVP",
            Summary::Var => "VAR",
            Summary::VarP => "VARP",
        }
    }

    /// The menu's separator falls after this one (Sum … Minimum | Count … VARP).
    pub fn ends_group(self) -> bool {
        self == Summary::Minimum
    }

    fn of(self, texts: &[String]) -> Option<f64> {
        let nums: Vec<f64> = texts.iter().filter_map(|t| t.replace([',', '$', '%'], "").trim().parse().ok()).collect();
        let n = nums.len() as f64;
        let mean = || nums.iter().sum::<f64>() / n;
        let ss = || {
            let m = mean();
            nums.iter().map(|x| (x - m) * (x - m)).sum::<f64>()
        };
        match self {
            Summary::CountAll => Some(texts.iter().filter(|t| !t.trim().is_empty()).count() as f64),
            Summary::Count => Some(n),
            Summary::CountUnique => {
                let mut u: Vec<&str> = texts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
                u.sort_unstable();
                u.dedup();
                Some(u.len() as f64)
            }
            _ if nums.is_empty() => None,
            Summary::Sum => Some(nums.iter().sum()),
            Summary::Average => Some(mean()),
            Summary::Minimum => nums.iter().copied().reduce(f64::min),
            Summary::Maximum => nums.iter().copied().reduce(f64::max),
            Summary::Product => Some(nums.iter().product()),
            Summary::Median => {
                let mut v = nums.clone();
                v.sort_by(|a, b| a.total_cmp(b));
                let k = v.len();
                Some(if k % 2 == 1 { v[k / 2] } else { (v[k / 2 - 1] + v[k / 2]) / 2.0 })
            }
            // sample statistics need two values (Numbers shows an error otherwise)
            Summary::Stdev => (n > 1.0).then(|| (ss() / (n - 1.0)).sqrt()),
            Summary::Var => (n > 1.0).then(|| ss() / (n - 1.0)),
            Summary::StdevP => Some((ss() / n).sqrt()),
            Summary::VarP => Some(ss() / n),
        }
    }
}

/// How a value is shown (Numbers' "Show as").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ShowAs {
    #[default]
    Base,
    PctGrandTotal,
    PctColumnTotal,
    PctRowTotal,
    PctParentColumnTotal,
    PctParentRowTotal,
    Index,
    /// Running total down the rows.
    RunningTotal,
    PctRunningTotal,
}

impl ShowAs {
    pub const ALL: [ShowAs; 9] = [
        ShowAs::Base,
        ShowAs::PctGrandTotal,
        ShowAs::PctColumnTotal,
        ShowAs::PctRowTotal,
        ShowAs::PctParentColumnTotal,
        ShowAs::PctParentRowTotal,
        ShowAs::Index,
        ShowAs::RunningTotal,
        ShowAs::PctRunningTotal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShowAs::Base => "Base Value",
            ShowAs::PctGrandTotal => "% of Grand Total",
            ShowAs::PctColumnTotal => "% of Column Total",
            ShowAs::PctRowTotal => "% of Row Total",
            ShowAs::PctParentColumnTotal => "% of Parent Column Total",
            ShowAs::PctParentRowTotal => "% of Parent Row Total",
            ShowAs::Index => "Index",
            ShowAs::RunningTotal => "Running Total In…",
            ShowAs::PctRunningTotal => "% of Running Total In…",
        }
    }

    /// Shown as a percentage.
    pub fn percent(self) -> bool {
        !matches!(self, ShowAs::Base | ShowAs::Index | ShowAs::RunningTotal)
    }
}

/// A pivot table's fields: source columns.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PivotSpec {
    pub rows: Vec<u32>,
    pub cols: Vec<u32>,
    pub values: Vec<(u32, Summary)>,
    /// Only these source rows (For Selected Cells); None = the whole body.
    pub only: Option<(u32, u32)>,
    /// "Show as" of each value field (missing: Base Value).
    pub show_as: Vec<ShowAs>,
    /// Rows (…) > Hide Grand Total Row, Columns (…) > Hide Grand Total Column.
    pub hide_total_row: bool,
    pub hide_total_column: bool,
}

/// The pivot's cells: header rows first; numbers as numbers. Also how many header rows and
/// header columns it has.
pub struct Pivot {
    pub cells: Vec<Vec<Cell>>,
    pub header_rows: u32,
    pub header_cols: u32,
    /// Cells shown as percentages (row, column).
    pub percent: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
    Empty,
}

fn label(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() { "(blank)".into() } else { t.to_string() }
}

/// Key order: numbers by size, text alphabetically, blank last.
fn key_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let k = |s: &str| (s == "(blank)", s.replace([',', '$', '%'], "").parse::<f64>().ok(), s.to_lowercase());
    let (ka, kb) = (k(a), k(b));
    ka.0.cmp(&kb.0).then_with(|| match (ka.1, kb.1) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => ka.2.cmp(&kb.2),
    })
}

/// A source column's name: its header cell, else "Column B".
pub fn field_name(t: &TableData, c: u32) -> String {
    let h = (t.header_rows > 0).then(|| t.cells.get(c as usize).map(|s| s.trim().replace('\n', " "))).flatten().filter(|s| !s.is_empty());
    h.unwrap_or_else(|| {
        let mut n = c;
        let mut s = String::new();
        loop {
            s.insert(0, (b'A' + (n % 26) as u8) as char);
            if n < 26 {
                break;
            }
            n = n / 26 - 1;
        }
        format!("Column {s}")
    })
}

/// Summarize `t` as `spec` says.
pub fn build(t: &TableData, spec: &PivotSpec) -> Pivot {
    let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let (b0, b1) = (t.header_rows, t.rows.saturating_sub(t.footer_rows));
    let (b0, b1) = match spec.only {
        Some((a, b)) => (a.max(b0), (b + 1).min(b1)),
        None => (b0, b1),
    };
    let body: Vec<u32> = (b0..b1).filter(|&r| (0..t.cols).any(|c| !cell(r, c).trim().is_empty())).collect();
    let key = |r: u32, cols: &[u32]| cols.iter().map(|c| label(&cell(r, *c))).collect::<Vec<String>>();
    let mut row_keys: Vec<Vec<String>> = body.iter().map(|&r| key(r, &spec.rows)).collect();
    row_keys.sort_by(|a, b| a.iter().zip(b).map(|(x, y)| key_cmp(x, y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
    row_keys.dedup();
    let mut col_keys: Vec<Vec<String>> = body.iter().map(|&r| key(r, &spec.cols)).collect();
    col_keys.sort_by(|a, b| a.iter().zip(b).map(|(x, y)| key_cmp(x, y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
    col_keys.dedup();
    // the source rows of each (row key, column key)
    let mut groups: BTreeMap<(usize, usize), Vec<u32>> = BTreeMap::new();
    for &r in &body {
        let ri = row_keys.iter().position(|k| *k == key(r, &spec.rows)).unwrap_or(0);
        let ci = col_keys.iter().position(|k| *k == key(r, &spec.cols)).unwrap_or(0);
        groups.entry((ri, ci)).or_default().push(r);
    }
    let values: Vec<(u32, Summary)> = if spec.values.is_empty() { Vec::new() } else { spec.values.clone() };
    let nvals = values.len().max(1);
    let summarize = |rows: &[u32], v: (u32, Summary)| -> Cell {
        let texts: Vec<String> = rows.iter().map(|&r| cell(r, v.0)).collect();
        match v.1.of(&texts) {
            Some(x) => Cell::Number(x),
            None => Cell::Empty,
        }
    };
    let hc = spec.rows.len().max(1) as u32;
    let multi_cols = !spec.cols.is_empty();
    // Numbers' layout: a header row per column field (the field's name in the corner, then its
    // keys and Grand Total), then the values row (the row fields' names, then "Sales (Sum)")
    let _ = nvals;
    let hr = spec.cols.len() as u32 + 1;
    let mut out: Vec<Vec<Cell>> = Vec::new();
    let col_count = if multi_cols { col_keys.len() + 1 } else { 1 };
    let vn = values.len().max(1);
    for level in 0..spec.cols.len() {
        let mut row: Vec<Cell> = vec![Cell::Empty; hc as usize];
        row[hc as usize - 1] = Cell::Text(field_name(t, spec.cols[level]));
        for ci in 0..col_count {
            for vi in 0..vn {
                let text = if vi > 0 {
                    String::new()
                } else if ci < col_keys.len() {
                    // a key shows where it starts (the deeper levels repeat under it)
                    let starts = ci == 0 || col_keys[ci - 1][..=level] != col_keys[ci][..=level];
                    if starts { col_keys[ci][level].clone() } else { String::new() }
                } else if level == 0 {
                    "Grand Total".into()
                } else {
                    String::new()
                };
                row.push(if text.is_empty() { Cell::Empty } else { Cell::Text(text) });
            }
        }
        out.push(row);
    }
    let mut row: Vec<Cell> = (0..hc).map(|i| spec.rows.get(i as usize).map(|c| Cell::Text(field_name(t, *c))).unwrap_or(Cell::Empty)).collect();
    for ci in 0..col_count {
        for vi in 0..vn {
            // one value: its name once, over the keys (Numbers merges it across them); more:
            // each group names its values
            let text = match values.get(vi) {
                None => if ci == 0 { "Count".to_string() } else { String::new() },
                Some(_) if vn == 1 && ci > 0 => String::new(),
                Some(v) => format!("{} ({})", field_name(t, v.0), v.1.name()),
            };
            row.push(if text.is_empty() { Cell::Empty } else { Cell::Text(text) });
        }
    }
    out.push(row);
    // body: a row per row key, then Grand Total
    let all_rows: Vec<u32> = body.clone();
    for (ri, rk) in row_keys.iter().enumerate().map(|(i, k)| (Some(i), k.clone())).chain(std::iter::once((None, vec!["Grand Total".to_string()]))) {
        let mut row: Vec<Cell> = (0..hc as usize).map(|i| rk.get(i).map(|s| Cell::Text(s.clone())).unwrap_or(Cell::Empty)).collect();
        for ci in 0..col_count {
            let rows: Vec<u32> = match (ri, multi_cols && ci < col_keys.len()) {
                (Some(ri), true) => groups.get(&(ri, ci)).cloned().unwrap_or_default(),
                (Some(ri), false) => groups.iter().filter(|((r, _), _)| *r == ri).flat_map(|(_, v)| v.clone()).collect(),
                (None, true) => groups.iter().filter(|((_, c), _)| *c == ci).flat_map(|(_, v)| v.clone()).collect(),
                (None, false) => all_rows.clone(),
            };
            if values.is_empty() {
                row.push(Cell::Number(rows.len() as f64));
            } else {
                for v in &values {
                    row.push(if rows.is_empty() { Cell::Empty } else { summarize(&rows, *v) });
                }
            }
        }
        out.push(row);
    }
    let nv = values.len();
    let mut percent = Vec::new();
    // Show as: each value field's cells against its totals
    if nv > 0 {
        let total_r = out.len() - 1;
        let first_body = hr as usize;
        let groups = col_count;
        let grand_ci = groups - 1;
        let num = |c: &Cell| if let Cell::Number(x) = c { Some(*x) } else { None };
        let col_of = |ci: usize, vi: usize| hc as usize + ci * nv + vi;
        let snapshot = out.clone();
        for (vi, show) in spec.show_as.iter().enumerate().take(nv) {
            if *show == ShowAs::Base {
                continue;
            }
            let grand = snapshot[total_r].get(col_of(grand_ci, vi)).and_then(num).unwrap_or(f64::NAN);
            for ci in 0..groups {
                let c = col_of(ci, vi);
                let col_total = snapshot[total_r].get(c).and_then(num).unwrap_or(f64::NAN);
                let mut running = 0.0;
                for r in first_body..out.len() {
                    let Some(x) = snapshot[r].get(c).and_then(num) else { continue };
                    let row_total = snapshot[r].get(col_of(grand_ci, vi)).and_then(num).unwrap_or(f64::NAN);
                    let is_total_row = r == total_r;
                    if !is_total_row {
                        running += x;
                    }
                    let v = match show {
                        ShowAs::PctGrandTotal => x / grand,
                        ShowAs::PctColumnTotal | ShowAs::PctParentColumnTotal => x / col_total,
                        ShowAs::PctRowTotal | ShowAs::PctParentRowTotal => x / row_total,
                        ShowAs::Index => x * grand / (row_total * col_total),
                        ShowAs::RunningTotal => if is_total_row { x } else { running },
                        ShowAs::PctRunningTotal => if is_total_row { 1.0 } else { running / col_total },
                        ShowAs::Base => x,
                    };
                    out[r][c] = if v.is_finite() { Cell::Number(v) } else { Cell::Empty };
                    if show.percent() {
                        percent.push((r as u32, c as u32));
                    }
                }
            }
        }
    }
    // Hide Grand Total Row / Column
    if spec.hide_total_row && out.len() > hr as usize + 1 {
        out.pop();
        let last = out.len() as u32;
        percent.retain(|p| p.0 < last);
    }
    if spec.hide_total_column && multi_cols {
        let w = nv.max(1);
        let start = hc as usize + (col_count - 1) * w;
        for row in &mut out {
            row.truncate(start);
        }
        percent.retain(|p| (p.1 as usize) < start);
    }
    Pivot { cells: out, header_rows: hr, header_cols: hc, percent }
}

/// Show pivot `pv` in table `t` (a Numbers pivot table, whose Grand Total row and column live
/// in its summary model, not its cells): `t` grows to `pv`'s size, new rows and columns styled
/// like its last ones, totals in bold, numbers formatted.
pub fn show_in(t: &mut TableData, pv: &Pivot, spec: &PivotSpec) {
    if t.rows == 0 || t.cols == 0 {
        return;
    }
    let rows = pv.cells.len() as u32;
    let cols = pv.cells.iter().map(|r| r.len()).max().unwrap_or(0) as u32;
    if rows == 0 || cols == 0 {
        return;
    }
    let (or, oc) = (t.rows, t.cols);
    let sr = |r: u32| r.min(or - 1);
    let sc = |c: u32| c.min(oc - 1);
    let total_row = rows - 1;
    let total_col = cols - 1;
    let has_total_row = !spec.hide_total_row;
    let has_total_col = !spec.cols.is_empty() && !spec.hide_total_column;
    let mut cells = Vec::with_capacity((rows * cols) as usize);
    let mut styles = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let text = match pv.cells[r as usize].get(c as usize) {
                Some(Cell::Text(s)) => s.clone(),
                Some(Cell::Number(x)) if pv.percent.contains(&(r, c)) => format!("{}%", kn_model::table::fmt_num((x * 1000.0).round() / 10.0)),
                Some(Cell::Number(x)) => kn_model::table::fmt_num((x * 1e9).round() / 1e9),
                _ => String::new(),
            };
            cells.push(text);
            if !t.cell_styles.is_empty() {
                let mut st = t.cell_styles.get((sr(r) * oc + sc(c)) as usize).cloned().unwrap_or_default();
                if r >= pv.header_rows && ((has_total_row && r == total_row) || (has_total_col && c == total_col && c >= pv.header_cols)) {
                    st.text.bold = true;
                }
                styles.push(st);
            }
        }
    }
    let grow = |v: &[f32], n: u32| -> Vec<f32> { (0..n as usize).map(|i| v.get(i).or(v.last()).copied().unwrap_or(20.0)).collect() };
    t.row_heights = grow(&t.row_heights, rows);
    t.col_widths = grow(&t.col_widths, cols);
    let h = std::mem::take(&mut t.h_strokes);
    if !h.is_empty() {
        t.h_strokes = (0..=rows).flat_map(|b| (0..cols).map(move |c| (b, c))).map(|(b, c)| h.get((b.min(or) * oc + sc(c)) as usize).cloned().flatten()).collect();
    }
    let v = std::mem::take(&mut t.v_strokes);
    if !v.is_empty() {
        t.v_strokes = (0..rows).flat_map(|r| (0..=cols).map(move |b| (r, b))).map(|(r, b)| v.get((sr(r) * (oc + 1) + b.min(oc)) as usize).cloned().flatten()).collect();
    }
    if !t.auto_rows.is_empty() {
        let last = *t.auto_rows.last().unwrap();
        t.auto_rows.resize(rows as usize, last);
    }
    t.cells = cells;
    t.cell_styles = styles;
    t.rows = rows;
    t.cols = cols;
    t.header_rows = pv.header_rows.min(rows);
    t.header_cols = pv.header_cols.min(cols);
}

/// A pivot table made by Numbers: (the pivot table, its source table, its fields, when it was
/// last refreshed as seconds since 2001-01-01). TST.PivotOwnerArchive names the source by its
/// table_id and the fields by the source's column uids (Mac N29-C1).
pub fn native_pivots(k: &kn_format::KeyFile) -> Vec<(kn_format::ObjId, kn_format::ObjId, PivotSpec, Option<f64>)> {
    use kn_format::tables::{uuid_of_str, uuid_of_tsp};
    let infos: Vec<(kn_format::ObjId, kn_format::ObjId)> = k.objects_of("TST.TableInfoArchive").filter_map(|(i, m)| Some((i, m.ref_id("tableModel")?))).collect();
    let mut out = Vec::new();
    for (owner_id, owner) in k.objects_of("TST.PivotOwnerArchive") {
        let Some(&(pivot, _)) = infos.iter().find(|(_, model)| k.archive(*model).is_some_and(|a| a.object_references(0).contains(&owner_id))) else { continue };
        let Some(src_uid) = owner.msg("source_table_uid").map(uuid_of_tsp) else { continue };
        let Some(&(source, model)) = infos.iter().find(|(_, m)| k.obj(*m).and_then(|m| m.str("table_id")).and_then(uuid_of_str) == Some(src_uid)) else { continue };
        let col_of: std::collections::HashMap<[u8; 16], u32> = k
            .obj(model)
            .and_then(|m| m.ref_id("base_column_row_uids"))
            .and_then(|x| k.obj(x))
            .map(|m| m.msgs("sorted_column_uids").zip(m.u64s("column_index_for_uid")).map(|(u, i)| (uuid_of_tsp(u), i as u32)).collect())
            .unwrap_or_default();
        let group = |f: &str| -> Vec<u32> { owner.msg(f).into_iter().flat_map(|g| g.msgs("group_column")).filter_map(|g| g.msg("column_uid").map(uuid_of_tsp)).filter_map(|u| col_of.get(&u).copied()).collect() };
        let mut spec = PivotSpec { rows: group("grouping_columns_for_rows"), cols: group("grouping_columns_for_columns"), ..Default::default() };
        for a in owner.msg("aggregate_columns").into_iter().flat_map(|x| x.msgs("aggregates")) {
            let Some(c) = a.msg("column_uid").map(uuid_of_tsp).and_then(|u| col_of.get(&u).copied()) else { continue };
            spec.values.push((c, summary_of_agg(a.u64("agg_type").unwrap_or(2))));
            spec.show_as.push(show_of(a.u64("show_as_type").unwrap_or(0)));
        }
        spec.hide_total_row = owner.bool("hide_grand_total_rows").unwrap_or(false);
        spec.hide_total_column = owner.bool("hide_grand_total_columns").unwrap_or(false);
        out.push((pivot, source, spec, owner.f64("refresh_timestamp")));
    }
    out
}

/// TST.ColumnAggregateArchive.agg_type (N29-C1: 2 = Sum; the others are being confirmed on the
/// Mac, N30 C4: until then they read as Sum).
fn summary_of_agg(v: u64) -> Summary {
    match v {
        2 => Summary::Sum,
        _ => Summary::Sum,
    }
}

/// show_as_type (0 = Base Value; the others are being confirmed, N30 C5).
fn show_of(v: u64) -> ShowAs {
    match v {
        0 => ShowAs::Base,
        _ => ShowAs::Base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_numbers_pivot() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/research/out-N29/files/N29-C1-pivot.numbers");
        if !p.exists() {
            return;
        }
        let wb = crate::Workbook::open(&p).unwrap();
        let v = native_pivots(wb.file());
        assert_eq!(v.len(), 1);
        let (_, _, spec, ts) = &v[0];
        // Rows Region, Columns Product, Values Sum of Sales
        assert_eq!((spec.rows.clone(), spec.cols.clone(), spec.values.clone()), (vec![0], vec![1], vec![(2, Summary::Sum)]));
        assert!(ts.is_some());
    }

    fn source() -> TableData {
        let cells: Vec<String> = ["Region", "Product", "Sales", "East", "Pen", "3", "West", "Pen", "5", "East", "Ink", "10", "West", "Ink", "4", "East", "Pen", "2"].iter().map(|s| s.to_string()).collect();
        TableData { rows: 6, cols: 3, header_rows: 1, cells, ..Default::default() }
    }

    #[test]
    fn sums_by_rows_and_columns() {
        let p = build(&source(), &PivotSpec { rows: vec![0], cols: vec![1], values: vec![(2, Summary::Sum)], ..Default::default() });
        let n = |r: usize, c: usize| match &p.cells[r][c] {
            Cell::Number(x) => *x,
            other => panic!("{r},{c}: {other:?}"),
        };
        // as Numbers (Mac N29-C1): Product | Ink | Pen | Grand Total, then Region | Sales (Sum)
        assert_eq!(p.header_rows, 2);
        assert_eq!(p.cells[0][0], Cell::Text("Product".into()));
        assert_eq!(p.cells[0][1], Cell::Text("Ink".into()));
        assert_eq!(p.cells[0][3], Cell::Text("Grand Total".into()));
        assert_eq!(p.cells[1][0], Cell::Text("Region".into()));
        assert_eq!(p.cells[1][1], Cell::Text("Sales (Sum)".into()));
        assert_eq!(p.cells[1][2], Cell::Empty);
        // East: Ink 10, Pen 5, total 15; West: 4, 5, 9; Grand Total 14, 10, 24
        assert_eq!(p.cells[2][0], Cell::Text("East".into()));
        assert_eq!([n(2, 1), n(2, 2), n(2, 3)], [10.0, 5.0, 15.0]);
        assert_eq!([n(3, 1), n(3, 2), n(3, 3)], [4.0, 5.0, 9.0]);
        assert_eq!([n(4, 1), n(4, 2), n(4, 3)], [14.0, 10.0, 24.0]);
        let p = build(&source(), &PivotSpec { rows: vec![0], cols: vec![], values: vec![(2, Summary::Average)], ..Default::default() });
        assert_eq!(p.cells[0][1], Cell::Text("Sales (Average)".into()));
        assert_eq!(p.cells[1][1], Cell::Number(5.0));
    }

    #[test]
    fn summaries_show_as_and_hidden_totals() {
        let n = |p: &Pivot, r: usize, c: usize| match &p.cells[r][c] {
            Cell::Number(x) => *x,
            other => panic!("{r},{c}: {other:?}"),
        };
        // Sales by Region: East 3, 10, 2 (sum 15); West 5, 4 (sum 9)
        let spec = |s: Summary| PivotSpec { rows: vec![0], values: vec![(2, s)], ..Default::default() };
        let p = build(&source(), &spec(Summary::Stdev));
        assert!((n(&p, 1, 1) - (((3.0f64 - 5.0).powi(2) + 25.0 + 9.0) / 2.0).sqrt()).abs() < 1e-9);
        assert_eq!(n(&build(&source(), &spec(Summary::VarP)), 2, 1), 0.25);
        assert_eq!(n(&build(&source(), &spec(Summary::Count)), 3, 1), 5.0);
        // a West sale with text: Count leaves it out, Count (All) counts it
        let mut t = source();
        t.cells[14] = "n/a".into();
        assert_eq!(n(&build(&t, &spec(Summary::Count)), 2, 1), 1.0);
        assert_eq!(n(&build(&t, &spec(Summary::CountAll)), 2, 1), 2.0);
        // % of Grand Total, with columns: East Ink 10 of 24
        let mut sp = PivotSpec { rows: vec![0], cols: vec![1], values: vec![(2, Summary::Sum)], show_as: vec![ShowAs::PctGrandTotal], ..Default::default() };
        let p = build(&source(), &sp);
        assert!((n(&p, 2, 1) - 10.0 / 24.0).abs() < 1e-9);
        assert_eq!(n(&p, 4, 3), 1.0);
        assert!(p.percent.contains(&(2, 1)));
        sp.show_as = vec![ShowAs::PctRowTotal];
        assert!((n(&build(&source(), &sp), 3, 1) - 4.0 / 9.0).abs() < 1e-9);
        sp.show_as = vec![ShowAs::PctColumnTotal];
        assert!((n(&build(&source(), &sp), 3, 1) - 4.0 / 14.0).abs() < 1e-9);
        sp.show_as = vec![ShowAs::RunningTotal];
        let p = build(&source(), &sp);
        assert_eq!((n(&p, 2, 1), n(&p, 3, 1)), (10.0, 14.0));
        // Hide Grand Total Row / Column
        sp.show_as = vec![];
        sp.hide_total_row = true;
        sp.hide_total_column = true;
        let p = build(&source(), &sp);
        assert_eq!(p.cells.len(), 4);
        assert!(p.cells.iter().all(|r| r.len() == 3));
    }
}
