//! Charts linked to table cells, as in Numbers: a chart's TN.ChartMediatorArchive (the chart's
//! `unity.mediator`) holds a formula per series (a range of one table), formulas for the
//! table's row labels and column labels, and the table by its table_id (Mac N29-H2). When the
//! cells change the chart's grid is rebuilt from them; a chart made from selected cells is
//! linked to them.

use kn_format::tables::{cf_msg, family, uuid_of_cf, uuid_of_str};
use kn_format::{Msg, ObjId};
use kn_model::chartdata::ChartGrid;
use kn_model::edit::History;
use kn_model::{Document, TableData};

/// A chart's link to table cells (0-based cells of the table).
#[derive(Debug, Clone, PartialEq)]
pub struct ChartLink {
    pub mediator: ObjId,
    /// The table (TST.TableInfoArchive), when it's in the document.
    pub table: Option<ObjId>,
    /// Each series' cells: (r0, c0, r1, c1).
    pub series: Vec<(u32, u32, u32, u32)>,
    /// Labels of the table rows / columns the series run across.
    pub row_labels: Vec<(u32, u32)>,
    pub col_labels: Vec<(u32, u32)>,
}

impl ChartLink {
    /// The series run down the table's columns (each series one column).
    pub fn columns_are_series(&self) -> bool {
        self.series.iter().all(|s| s.1 == s.3) && self.series.iter().any(|s| s.0 != s.2) || (self.series.iter().all(|s| s.0 == s.2 && s.1 == s.3) && self.col_labels.len() == self.series.len())
    }

    /// Every cell the link reads: (r0, c0, r1, c1), for Edit Data References.
    pub fn ranges(&self) -> Vec<(u32, u32, u32, u32)> {
        let one = |&(r, c): &(u32, u32)| (r, c, r, c);
        self.series.iter().copied().chain(self.row_labels.iter().map(one)).chain(self.col_labels.iter().map(one)).collect()
    }
}

/// The cells a formula refers to (relative references are from A1, as charts' are).
fn refs(f: &Msg) -> Vec<(u32, u32)> {
    let Some(arr) = f.msg("AST_node_array") else { return Vec::new() };
    arr.msgs("AST_node")
        .filter(|n| n.enum_name("AST_node_type") == Some("CELL_REFERENCE_NODE"))
        .filter_map(|n| Some((u32::try_from(n.msg("AST_row")?.i64("row")?).ok()?, u32::try_from(n.msg("AST_column")?.i64("column")?).ok()?)))
        .collect()
}

fn table_family(f: &Msg) -> Option<[u8; 12]> {
    let arr = f.msg("AST_node_array")?;
    arr.msgs("AST_node").find_map(|n| n.msg("AST_cross_table_reference_extra_info")?.msg("table_id").and_then(|u| family(&uuid_of_cf(u))))
}

fn table_by_family(doc: &Document, fam: [u8; 12]) -> Option<ObjId> {
    let k = &doc.file;
    k.objects_of("TST.TableInfoArchive").map(|(id, _)| id).find(|i| {
        k.obj(*i).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)).and_then(|m| m.str("table_id")).and_then(uuid_of_str).and_then(|u| family(&u)) == Some(fam)
    })
}

fn mediator_of(doc: &Document, chart: ObjId) -> Option<ObjId> {
    doc.file.obj(chart)?.msg("unity")?.ref_id("mediator").filter(|m| doc.file.type_name(*m) == "TN.ChartMediatorArchive")
}

/// Chart `chart`'s link to cells, if it has one.
pub fn link(doc: &Document, chart: ObjId) -> Option<ChartLink> {
    let mediator = mediator_of(doc, chart)?;
    let f = doc.file.obj(mediator)?.msg("formulas")?;
    let data: Vec<&Msg> = f.msgs("data_formulae").collect();
    if data.is_empty() {
        return None;
    }
    let table = data.iter().find_map(|m| table_family(m)).and_then(|fam| table_by_family(doc, fam));
    let series = data
        .iter()
        .filter_map(|m| {
            let r = refs(m);
            let (a, b) = (*r.first()?, *r.last()?);
            Some((a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)))
        })
        .collect();
    let one = |name: &str| f.msgs(name).filter_map(|m| refs(m).first().copied()).collect::<Vec<_>>();
    Some(ChartLink { mediator, table, series, row_labels: one("row_label_formulae"), col_labels: one("col_label_formulae") })
}

fn num(s: &str) -> f64 {
    let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
    if s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e') {
        return f64::NAN;
    }
    clean.parse::<f64>().unwrap_or(f64::NAN)
}

/// The grid a linked chart shows now: the table's cells, laid out the way the chart reads its
/// grid (`by_row`: series are grid rows).
pub fn grid_of(link: &ChartLink, t: &TableData, by_row: bool) -> ChartGrid {
    let cell = |r: u32, c: u32| if r < t.rows && c < t.cols { t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default() } else { String::new() };
    let cols_series = link.columns_are_series();
    let series: Vec<Vec<f64>> = link
        .series
        .iter()
        .map(|&(r0, c0, r1, c1)| if cols_series || c0 == c1 { (r0..=r1).map(|r| num(&cell(r, c0))).collect() } else { (c0..=c1).map(|c| num(&cell(r0, c))).collect() })
        .collect();
    let (names, cats) = if cols_series { (&link.col_labels, &link.row_labels) } else { (&link.row_labels, &link.col_labels) };
    let n = series.iter().map(Vec::len).max().unwrap_or(0);
    let names: Vec<String> = (0..series.len()).map(|i| names.get(i).map(|&(r, c)| cell(r, c)).unwrap_or_else(|| format!("Series {}", i + 1))).collect();
    let cats: Vec<String> = (0..n).map(|j| cats.get(j).map(|&(r, c)| cell(r, c)).unwrap_or_else(|| (j + 1).to_string())).collect();
    if by_row {
        ChartGrid { row_names: names, col_names: cats, values: series.iter().map(|s| (0..n).map(|j| s.get(j).copied().unwrap_or(f64::NAN)).collect()).collect() }
    } else {
        ChartGrid { row_names: cats, col_names: names, values: (0..n).map(|j| series.iter().map(|s| s.get(j).copied().unwrap_or(f64::NAN)).collect()).collect() }
    }
}

fn same(a: &ChartGrid, b: &ChartGrid) -> bool {
    a.row_names == b.row_names
        && a.col_names == b.col_names
        && a.values.len() == b.values.len()
        && a.values.iter().zip(&b.values).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p.is_nan() && q.is_nan()) || p == q))
}

/// Rebuild every linked chart whose cells changed; how many were.
pub fn refresh(doc: &mut Document, h: &mut History) -> usize {
    let charts: Vec<ObjId> = doc.file.objects_of("TSCH.ChartDrawableArchive").map(|(id, _)| id).collect();
    let mut n = 0;
    for chart in charts {
        let Some(l) = link(doc, chart) else { continue };
        let Some(t) = l.table.and_then(|t| doc.file.obj(t).and_then(|m| kn_model::table::table(&doc.file, m))) else { continue };
        let by_row = doc.chart_layout(chart).map(|x| x.by_row).unwrap_or(true);
        let g = grid_of(&l, &t, by_row);
        if g.values.is_empty() || doc.chart_grid(chart).is_some_and(|cur| same(&cur, &g)) {
            continue;
        }
        if doc.set_chart_grid(h, chart, &g) {
            n += 1;
        }
    }
    n
}

/// A cell-reference node (to `table_id`) from template `node`.
fn ref_node(node: &Msg, table: &Msg, r: u32, c: u32) -> Msg {
    let mut n = node.clone();
    if let Some(col) = n.msg_mut("AST_column") {
        col.set_i64("column", c as i64);
        col.set_bool("absolute", false);
    }
    if let Some(row) = n.msg_mut("AST_row") {
        row.set_i64("row", r as i64);
        row.set_bool("absolute", false);
    }
    n.remove("AST_cross_table_reference_extra_info");
    let mut x = Msg::new("TSCE.ASTNodeArrayArchive.ASTCrossTableReferenceExtraInfoArchive");
    x.push_msg("table_id", table.clone());
    n.push_msg("AST_cross_table_reference_extra_info", x);
    n
}

/// A formula like `template` (a range or one cell, wrapped as Numbers wraps chart references)
/// over cells (r0, c0)..(r1, c1).
fn formula(template: &Msg, table: &Msg, (r0, c0, r1, c1): (u32, u32, u32, u32)) -> Option<Msg> {
    let mut f = template.clone();
    let arr = f.msg_mut("AST_node_array")?;
    let nodes: Vec<Msg> = arr.msgs("AST_node").cloned().collect();
    let cell = nodes.iter().find(|n| n.enum_name("AST_node_type") == Some("CELL_REFERENCE_NODE"))?.clone();
    let colon = nodes.iter().find(|n| matches!(n.enum_name("AST_node_type"), Some("COLON_NODE") | Some("COLON_TRACT_NODE"))).cloned();
    let func = nodes.iter().find(|n| n.enum_name("AST_node_type") == Some("FUNCTION_NODE")).cloned();
    arr.remove("AST_node");
    arr.push_msg("AST_node", ref_node(&cell, table, r0, c0));
    if (r0, c0) != (r1, c1) {
        arr.push_msg("AST_node", ref_node(&cell, table, r1, c1));
        arr.push_msg("AST_node", colon?);
    }
    if let Some(func) = func {
        arr.push_msg("AST_node", func);
    }
    Some(f)
}

/// Link chart `chart` (inserted from a donor, so it has a mediator to reuse) to table `table`'s
/// cells: series `series` (each (r0, c0, r1, c1)) named by cells `names`, categories by
/// `cats`. Columns of the table are series when `columns_are_series`.
pub fn set_link(doc: &mut Document, h: &mut History, chart: ObjId, table: ObjId, series: &[(u32, u32, u32, u32)], names: &[(u32, u32)], cats: &[(u32, u32)], columns_are_series: bool) -> bool {
    let Some(mediator) = mediator_of(doc, chart) else { return false };
    let Some(tid) = doc.file.obj(table).and_then(|m| m.ref_id("tableModel")).and_then(|t| doc.file.obj(t)).and_then(|m| m.str("table_id")).and_then(uuid_of_str) else { return false };
    let table_msg = cf_msg(&tid);
    let Some(f) = doc.file.obj(mediator).and_then(|m| m.msg("formulas")).cloned() else { return false };
    // templates: the donor's own series and label formulas
    let Some(range_t) = f.msgs("data_formulae").next().cloned() else { return false };
    let Some(label_t) = f.msgs("row_label_formulae").chain(f.msgs("col_label_formulae")).next().cloned() else { return false };
    let mut nf = f.clone();
    for name in ["data_formulae", "row_label_formulae", "col_label_formulae"] {
        nf.remove(name);
    }
    for s in series {
        let Some(m) = formula(&range_t, &table_msg, *s) else { return false };
        nf.push_msg("data_formulae", m);
    }
    let (row_labels, col_labels) = if columns_are_series { (cats, names) } else { (names, cats) };
    for (name, cells) in [("row_label_formulae", row_labels), ("col_label_formulae", col_labels)] {
        for &(r, c) in cells {
            let Some(m) = formula(&label_t, &table_msg, (r, c, r, c)) else { return false };
            nf.push_msg(name, m);
        }
    }
    // Numbers: 2 = the table's columns are series (N29-H2)
    nf.set_i64("direction", if columns_are_series { 2 } else { 1 });
    doc.touch(h, mediator);
    let Some(m) = doc.file.obj_mut(mediator) else { return false };
    m.remove("formulas");
    m.push_msg("formulas", nf);
    m.set_bool("columns_are_series", columns_are_series);
    let n = series.len() as u64;
    if let Some(sup) = m.msg_mut("super") {
        sup.set_u64s("remote_series_indexes", &[n]);
    }
    true
}

/// Unlink a chart (its data stays as it is): Numbers' charts whose cells were deleted.
pub fn clear_link(doc: &mut Document, h: &mut History, chart: ObjId) -> bool {
    let Some(mediator) = mediator_of(doc, chart) else { return false };
    doc.touch(h, mediator);
    let Some(f) = doc.file.obj_mut(mediator).and_then(|m| m.msg_mut("formulas")) else { return false };
    for name in ["data_formulae", "row_label_formulae", "col_label_formulae"] {
        f.remove(name);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h2() -> Option<Document> {
        // (the Mac research files: testdata/research, kept on this computer)
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/research/out-N29/files/N29-H2-chart-styled.numbers");
        crate::init();
        p.exists().then(|| crate::Workbook::open(&p).ok()).flatten().map(|w| w.doc)
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn mac_link_reads_and_follows_cells() {
        let Some(mut d) = h2() else { return };
        let chart = d.file.objects_of("TSCH.ChartDrawableArchive").next().unwrap().0;
        let l = link(&d, chart).unwrap();
        // Series 1 = B2:B4, Series 2 = C2:C4; names B1, C1; categories A2..A4
        assert_eq!(l.series, vec![(1, 1, 3, 1), (1, 2, 3, 2)]);
        assert_eq!(l.col_labels, vec![(0, 1), (0, 2)]);
        assert_eq!(l.row_labels, vec![(1, 0), (2, 0), (3, 0)]);
        assert!(l.columns_are_series() && l.table.is_some());
        let t = l.table.unwrap();
        let g = grid_of(&l, &kn_model::table::table(&d.file, d.file.obj(t).unwrap()).unwrap(), true);
        assert_eq!(g, d.chart_grid(chart).unwrap());
        // a cell changes: the chart follows
        let mut h = History::default();
        d.begin(&mut h, "Typing");
        d.set_cell(&mut h, t, 2, 1, &kn_model::tablecells::CellInput::Number(99.0)).unwrap();
        assert_eq!(refresh(&mut d, &mut h), 1);
        d.end(&mut h);
        assert_eq!(d.chart_grid(chart).unwrap().values[0], vec![40.0, 99.0, 80.0]);
        assert_eq!(refresh(&mut d, &mut h), 0);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn new_link_round_trips() {
        let Some(mut d) = h2() else { return };
        let chart = d.file.objects_of("TSCH.ChartDrawableArchive").next().unwrap().0;
        let t = link(&d, chart).unwrap().table.unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Link");
        // only Series 2, rows 2-3
        assert!(set_link(&mut d, &mut h, chart, t, &[(2, 2, 3, 2)], &[(0, 2)], &[(2, 0), (3, 0)], true));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let l = link(&d2, chart).unwrap();
        assert_eq!(l.series, vec![(2, 2, 3, 2)]);
        assert_eq!((l.col_labels.clone(), l.row_labels.clone()), (vec![(0, 2)], vec![(2, 0), (3, 0)]));
        assert_eq!(l.table, Some(t));
        let mut d2 = d2;
        let mut h = History::default();
        d2.begin(&mut h, "Refresh");
        assert_eq!(refresh(&mut d2, &mut h), 1);
        d2.end(&mut h);
        let g = d2.chart_grid(chart).unwrap();
        assert_eq!((g.row_names, g.col_names, g.values), (vec!["Series 2".to_string()], vec!["Q2".to_string(), "Q3".to_string()], vec![vec![55.0, 70.0]]));
    }
}
