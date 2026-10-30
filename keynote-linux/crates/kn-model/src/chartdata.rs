//! Editing chart data (TSCH.ChartGridArchive inside the chart drawable's ChartArchive):
//! row names (series by row), column names (categories) and numeric values.

use kn_format::{Msg, ObjId};

use crate::chartstyle::{ChartPart, ChartValue};
use crate::edit::History;
use crate::Document;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartGrid {
    pub row_names: Vec<String>,
    pub col_names: Vec<String>,
    /// rows x cols; NaN = empty cell
    pub values: Vec<Vec<f64>>,
}

impl Document {
    pub fn chart_grid(&self, chart: ObjId) -> Option<ChartGrid> {
        let g = self.file.obj(chart)?.msg("unity")?.msg("grid")?;
        Some(ChartGrid {
            row_names: g.strs("row_name").into_iter().map(String::from).collect(),
            col_names: g.strs("column_name").into_iter().map(String::from).collect(),
            values: g.msgs("grid_row").map(|r| r.msgs("value").map(|v| v.f64("numeric_value").unwrap_or(f64::NAN)).collect()).collect(),
        })
    }

    /// Replace a chart's data. Rows/columns may be added or removed.
    pub fn set_chart_grid(&mut self, h: &mut History, chart: ObjId, grid: &ChartGrid) -> bool {
        self.touch(h, chart);
        let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) else { return false };
        let g = u.msg_mut_or_create("grid");
        let same_shape = g.count("row_name") == grid.row_names.len() && g.count("column_name") == grid.col_names.len();
        let rows: Vec<&str> = grid.row_names.iter().map(String::as_str).collect();
        let cols: Vec<&str> = grid.col_names.iter().map(String::as_str).collect();
        g.set_strs("row_name", &rows);
        g.set_strs("column_name", &cols);
        g.remove("grid_row");
        for r in 0..grid.row_names.len() {
            let mut gr = Msg::new("TSCH.GridRow");
            for c in 0..grid.col_names.len() {
                let mut v = Msg::new("TSCH.GridValue");
                let x = grid.values.get(r).and_then(|row| row.get(c)).copied().unwrap_or(f64::NAN);
                if x.is_finite() {
                    v.set_f64("numeric_value", x);
                }
                gr.push_msg("value", v);
            }
            g.push_msg("grid_row", gr);
        }
        if !same_shape {
            // row/column ids no longer line up; Keynote rebuilds the map
            g.remove("idMap");
        }
        u.set_bool("contains_default_data", false);
        true
    }
}

/// How a chart reads its grid (Edit Chart Data follows it): its type, whether series run
/// along the grid's rows (Plot Rows as Series) and, for scatter / bubble charts, whether all
/// series share the first X column.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartLayout {
    pub chart_type: String,
    pub by_row: bool,
    pub shared_x: bool,
}

/// What a series of the grid is to a chart type: the role of series `i` (in plot order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesRole {
    /// An ordinary series (columns, bars, lines, wedges…).
    Values,
    X,
    Y,
    Size,
}

impl ChartLayout {
    pub fn is_round(&self) -> bool {
        self.chart_type.starts_with("pie") || self.chart_type.starts_with("donut")
    }
    pub fn is_bubble(&self) -> bool {
        self.chart_type.contains("ubble")
    }
    pub fn is_xy(&self) -> bool {
        self.is_bubble() || self.chart_type.contains("catter")
    }

    /// Series `i`'s role: scatter series are X, Y (X shared by all when `shared_x`), bubble
    /// series X, Y, Size (as kn-render plots them).
    pub fn role(&self, i: usize) -> SeriesRole {
        if !self.is_xy() {
            return SeriesRole::Values;
        }
        let bubble = self.is_bubble();
        if self.shared_x {
            if i == 0 {
                return SeriesRole::X;
            }
            let per = if bubble { 2 } else { 1 };
            return if (i - 1) % per == 0 { SeriesRole::Y } else { SeriesRole::Size };
        }
        let per = if bubble { 3 } else { 2 };
        match i % per {
            0 => SeriesRole::X,
            1 => SeriesRole::Y,
            _ => SeriesRole::Size,
        }
    }

    /// Pies and donuts draw one value per series (the first category); the other categories
    /// are kept but not drawn.
    pub fn category_used(&self, j: usize) -> bool {
        !self.is_round() || j == 0
    }
}

impl Document {
    pub fn chart_layout(&self, chart: ObjId) -> Option<ChartLayout> {
        let u = self.file.obj(chart)?.msg("unity")?;
        Some(ChartLayout {
            chart_type: u.enum_name("chart_type").unwrap_or("columnChartType2D").to_string(),
            by_row: u.enum_name("series_direction") != Some("series_direction_by_column"),
            shared_x: u.enum_name("scatter_format") != Some("scatter_format_separate_x"),
        })
    }

    /// Edit Chart Data > Plot Rows as Series / Plot Columns as Series.
    pub fn set_chart_series_direction(&mut self, h: &mut History, chart: ObjId, by_row: bool) -> bool {
        self.touch(h, chart);
        let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) else { return false };
        u.set_enum("series_direction", if by_row { "series_direction_by_row" } else { "series_direction_by_column" });
        true
    }

    /// Scatter / bubble charts: Share X Values (one X column for every series) or not.
    pub fn set_chart_shared_x(&mut self, h: &mut History, chart: ObjId, shared: bool) -> bool {
        self.touch(h, chart);
        let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) else { return false };
        u.set_enum("scatter_format", if shared { "scatter_format_shared_x" } else { "scatter_format_separate_x" });
        true
    }
}

/// Chart types offered by the Format > Chart inspector (Keynote's 2D and 3D types that share
/// the same data layout; the styles are the theme's and cover every type).
pub const CHART_TYPES: &[(&str, &str)] = &[
    ("columnChartType2D", "Column"),
    ("stackedColumnChartType2D", "Stacked Column"),
    ("barChartType2D", "Bar"),
    ("stackedBarChartType2D", "Stacked Bar"),
    ("lineChartType2D", "Line"),
    ("areaChartType2D", "Area"),
    ("stackedAreaChartType2D", "Stacked Area"),
    ("pieChartType2D", "Pie"),
    ("donutChartType2D", "Donut"),
    ("scatterChartType2D", "Scatter"),
    ("bubbleChartType2D", "Bubble"),
    ("radarChartType2D", "Radar"),
    ("mixedChartType2D", "Mixed"),
    ("twoAxisChartType2D", "2-Axis"),
    ("multiDataColumnChartType2D", "Interactive Column"),
    ("multiDataBarChartType2D", "Interactive Bar"),
    ("multiDataScatterChartType2D", "Interactive Scatter"),
    ("multiDataBubbleChartType2D", "Interactive Bubble"),
    ("columnChartType3D", "3D Column"),
    ("stackedColumnChartType3D", "3D Stacked Column"),
    ("barChartType3D", "3D Bar"),
    ("stackedBarChartType3D", "3D Stacked Bar"),
    ("lineChartType3D", "3D Line"),
    ("areaChartType3D", "3D Area"),
    ("stackedAreaChartType3D", "3D Stacked Area"),
    ("pieChartType3D", "3D Pie"),
];

impl Document {
    /// Change a chart's type. Keynote's charts of different types differ only in this field (and
    /// cached layout frames it recomputes); pies and donuts plot series by column.
    pub fn set_chart_type(&mut self, h: &mut History, chart: ObjId, ty: &str) -> bool {
        if !CHART_TYPES.iter().any(|t| t.0 == ty) {
            return false;
        }
        self.touch(h, chart);
        let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) else { return false };
        let was_round = u.enum_name("chart_type").is_some_and(|t| t.starts_with("pie") || t.starts_with("donut"));
        u.set_enum("chart_type", ty);
        let round = ty.starts_with("pie") || ty.starts_with("donut");
        if round != was_round {
            u.set_enum("series_direction", if round { "series_direction_by_column" } else { "series_direction_by_row" });
        }
        true
    }

    /// Chart > Title: show (with this text) or hide the chart title. Keynote keeps both in the
    /// chart's non-style properties (verified on the Mac, round 7).
    pub fn set_chart_title(&mut self, h: &mut History, chart: ObjId, title: Option<&str>) -> bool {
        let Some(ns) = self.file.obj(chart).and_then(|m| m.msg("unity")).and_then(|u| u.ref_id("chart_non_style")) else { return false };
        self.touch(h, ns);
        let Some(m) = self.file.obj_mut(ns) else { return false };
        let cur = m.msg_mut_or_create("current");
        cur.set_bool("tschchartinfodefaultshowtitle", title.is_some());
        if let Some(t) = title {
            cur.set_str("tschchartinfodefaulttitle", t);
        }
        true
    }

    /// Chart > Legend checkbox: the chart's own (non-style) show-legend property.
    pub fn set_chart_legend(&mut self, h: &mut History, chart: ObjId, show: bool) -> bool {
        let Some(ns) = self.file.obj(chart).and_then(|m| m.msg("unity")).and_then(|u| u.ref_id("chart_non_style")) else { return false };
        self.touch(h, ns);
        let Some(m) = self.file.obj_mut(ns) else { return false };
        m.msg_mut_or_create("current").set_bool("tschchartinfodefaultshowlegend", show);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    /// The column chart of 06-chart-column.key with `grid`, `ty` and shared / separate X,
    /// saved and reopened: the plotted series.
    fn plotted(grid: &ChartGrid, ty: &str, shared_x: bool) -> (ChartLayout, Vec<crate::ChartSeries>) {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/06-chart-column.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let chart = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(x.kind, DrawableKind::Chart(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Chart");
        assert!(d.set_chart_type(&mut h, chart, ty));
        assert!(d.set_chart_grid(&mut h, chart, grid));
        d.set_chart_shared_x(&mut h, chart, shared_x);
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s2 = d2.slides();
        let c = d2.scene(&s2, 0).unwrap().drawables.into_iter().find_map(|x| match x.kind {
            DrawableKind::Chart(c) if x.id == chart => Some(c),
            _ => None,
        });
        (d2.chart_layout(chart).unwrap(), c.unwrap().series)
    }

    fn grid(rows: &[(&str, &[f64])], cols: &[&str]) -> ChartGrid {
        ChartGrid { row_names: rows.iter().map(|r| r.0.to_string()).collect(), col_names: cols.iter().map(|c| c.to_string()).collect(), values: rows.iter().map(|r| r.1.to_vec()).collect() }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn pie_scatter_bubble_round_trip() {
        // pie: series are the columns (wedges); each wedge is its first value
        let g = grid(&[("Region 1", &[17.0, 26.0, 53.0]), ("Region 2", &[55.0, 43.0, 70.0])], &["April", "May", "June"]);
        let (lay, series) = plotted(&g, "pieChartType2D", true);
        assert!(lay.is_round() && !lay.by_row);
        assert_eq!(series.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["April", "May", "June"]);
        assert_eq!(series.iter().map(|s| s.values[0]).collect::<Vec<_>>(), [17.0, 26.0, 53.0]);
        assert!(lay.category_used(0) && !lay.category_used(1));

        // scatter, shared X: the first series is every series' X
        let g = grid(&[("X", &[1.0, 2.0, 3.0]), ("A", &[10.0, 20.0, 30.0]), ("B", &[5.0, 6.0, 7.0])], &["1", "2", "3"]);
        let (lay, series) = plotted(&g, "scatterChartType2D", true);
        assert!(lay.shared_x && lay.by_row);
        assert_eq!(series.len(), 2);
        assert_eq!((series[0].xs.clone(), series[1].xs.clone()), (vec![1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0]));
        assert_eq!(series[1].values, vec![5.0, 6.0, 7.0]);

        // scatter, separate X: X, Y pairs
        let g = grid(&[("X1", &[1.0, 2.0]), ("A", &[10.0, 20.0]), ("X2", &[7.0, 8.0]), ("B", &[3.0, 4.0])], &["1", "2"]);
        let (lay, series) = plotted(&g, "scatterChartType2D", false);
        assert!(!lay.shared_x);
        assert_eq!(series.len(), 2);
        assert_eq!((series[1].xs.clone(), series[1].values.clone()), (vec![7.0, 8.0], vec![3.0, 4.0]));
        assert_eq!([lay.role(2), lay.role(3)], [SeriesRole::X, SeriesRole::Y]);

        // bubble, shared X: X, then Y + Size per series
        let g = grid(&[("X", &[1.0, 2.0]), ("A", &[10.0, 20.0]), ("A size", &[3.0, 9.0])], &["1", "2"]);
        let (lay, series) = plotted(&g, "bubbleChartType2D", true);
        assert_eq!(series.len(), 1);
        assert_eq!((series[0].xs.clone(), series[0].values.clone(), series[0].sizes.clone()), (vec![1.0, 2.0], vec![10.0, 20.0], vec![3.0, 9.0]));
        assert_eq!(lay.role(2), SeriesRole::Size);

        // bubble, separate X: X, Y, Size per series
        let g = grid(&[("X1", &[1.0]), ("A", &[10.0]), ("S1", &[2.0]), ("X2", &[4.0]), ("B", &[11.0]), ("S2", &[6.0])], &["1"]);
        let (_, series) = plotted(&g, "bubbleChartType2D", false);
        assert_eq!(series.len(), 2);
        assert_eq!((series[1].xs.clone(), series[1].values.clone(), series[1].sizes.clone()), (vec![4.0], vec![11.0], vec![6.0]));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn layout_roles_and_direction() {
        let l = |t: &str, shared_x| ChartLayout { chart_type: t.into(), by_row: true, shared_x };
        let sc = l("scatterChartType2D", true);
        assert_eq!([sc.role(0), sc.role(1), sc.role(2)], [SeriesRole::X, SeriesRole::Y, SeriesRole::Y]);
        let sc = l("scatterChartType2D", false);
        assert_eq!([sc.role(0), sc.role(1), sc.role(2), sc.role(3)], [SeriesRole::X, SeriesRole::Y, SeriesRole::X, SeriesRole::Y]);
        let bu = l("bubbleChartType2D", true);
        assert_eq!([bu.role(0), bu.role(1), bu.role(2), bu.role(3)], [SeriesRole::X, SeriesRole::Y, SeriesRole::Size, SeriesRole::Y]);
        let bu = l("multiDataBubbleChartType2D", false);
        assert_eq!([bu.role(0), bu.role(1), bu.role(2), bu.role(3)], [SeriesRole::X, SeriesRole::Y, SeriesRole::Size, SeriesRole::X]);
        let pie = l("pieChartType2D", true);
        assert!(pie.category_used(0) && !pie.category_used(1));
        assert_eq!(l("columnChartType2D", true).role(5), SeriesRole::Values);

        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/06-chart-column.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let chart = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(x.kind, DrawableKind::Chart(_))).unwrap().id;
        assert!(d.chart_layout(chart).unwrap().by_row);
        let mut h = History::default();
        d.begin(&mut h, "Plot Columns as Series");
        d.set_chart_series_direction(&mut h, chart, false);
        d.set_chart_shared_x(&mut h, chart, false);
        d.end(&mut h);
        let lay = d.chart_layout(chart).unwrap();
        assert!(!lay.by_row && !lay.shared_x);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn edit_chart_values_and_add_series() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/06-chart-column.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let chart = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(x.kind, DrawableKind::Chart(_))).unwrap().id;
        let mut g = d.chart_grid(chart).unwrap();
        g.values[0][0] = 42.5;
        g.row_names.push("Linux".into());
        g.values.push(vec![1.0; g.col_names.len()]);
        let mut h = History::default();
        d.begin(&mut h, "Chart Data");
        assert!(d.set_chart_grid(&mut h, chart, &g));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let g2 = d2.chart_grid(chart).unwrap();
        assert_eq!(g2.values[0][0], 42.5);
        assert_eq!(g2.row_names.last().unwrap(), "Linux");
        assert_eq!(g2.values.len(), g.values.len());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let DrawableKind::Chart(c) = &sc.drawables.iter().find(|x| x.id == chart).unwrap().kind else { panic!() };
        assert_eq!(c.values[0][0], 42.5);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn change_type_and_legend() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/06-chart-column.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let chart = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| matches!(x.kind, DrawableKind::Chart(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Chart Type");
        assert!(d.set_chart_type(&mut h, chart, "barChartType2D"));
        assert!(d.set_chart_legend(&mut h, chart, true));
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let DrawableKind::Chart(c) = &sc.drawables.iter().find(|x| x.id == chart).unwrap().kind else { panic!() };
        assert_eq!((c.chart_type.as_str(), c.show_legend), ("barChartType2D", true));
    }
}

/// One on/off chart option (Format > Chart and the Axis / Series inspector tabs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartFlag {
    Border(bool),
    /// (value axis index or None for the category / X axis, property, on): property is one of
    /// "showlabels", "showtitle", "showmajorgridlines", "showaxis".
    Axis(Option<usize>, &'static str, bool),
    ValueLabels(bool),
    /// Pie wedge / bubble series names.
    SeriesNames(bool),
}

impl Document {
    /// The series non-style object of every series of `chart`, creating the missing ones the
    /// way Keynote does when a series property is first set: an empty style, the three support
    /// flags, kept in the ObjectContainer and listed in unity.series_non_styles.
    pub fn ensure_series_non_styles(&mut self, h: &mut History, chart: ObjId) -> Vec<ObjId> {
        let Some(m) = self.file.obj(chart) else { return Vec::new() };
        let n = crate::chart::chart(&self.file, m).map(|c| c.series.len()).unwrap_or(0);
        let have: Vec<(u64, ObjId)> = m
            .msg("unity")
            .and_then(|u| u.msg("series_non_styles"))
            .map(|a| a.msgs("entries").filter_map(|e| Some((e.u64("index")?, e.ref_id("reference")?))).collect())
            .unwrap_or_default();
        let missing: Vec<u64> = (0..n as u64).filter(|i| !have.iter().any(|(j, _)| j == i)).collect();
        let oc = self.file.first_of("TSP.ObjectContainer").map(|(i, _)| i);
        if let (false, Some(oc)) = (missing.is_empty(), oc) {
            self.structural(h, &[oc, chart], |k| {
                let file = k.file_of(oc).unwrap_or("Index/ObjectContainer.iwa").to_string();
                let ids = k.alloc_ids(missing.len() as u64);
                for (i, id) in missing.iter().zip(ids) {
                    let mut m = Msg::new("TSCH.ChartSeriesNonStyleArchive");
                    m.push_msg("super", Msg::new("TSS.StyleArchive"));
                    m.push_msg("current", Msg::new("TSCH.Generated.ChartSeriesNonStyleArchive"));
                    m.set_bool("series_supports_custom_number_format", true);
                    m.set_bool("series_supports_custom_date_format", true);
                    m.set_bool("series_supports_callout_lines", true);
                    k.insert_archive(&file, kn_format::Archive::new_object(id, m));
                    if let Some(o) = k.obj_mut(oc) {
                        o.push_ref("objects", id);
                    }
                    k.add_uuid_entry(&file, id);
                    let Some(u) = k.obj_mut(chart).and_then(|c| c.msg_mut("unity")) else { continue };
                    let arr = u.msg_mut_or_create("series_non_styles");
                    let mut e = Msg::new("TSP.SparseReferenceArray.Entry");
                    e.set_u64("index", *i);
                    e.set_ref("reference", id);
                    arr.push_msg("entries", e);
                    let count = arr.u64("count").unwrap_or(0).max(n as u64);
                    arr.set_u64("count", count);
                }
                k.refresh_refs(chart);
                // the slide references objects in the ObjectContainer component: declare them
                if let Some(f) = k.file_of(chart).map(str::to_string) {
                    k.ensure_external_refs(&f);
                }
            });
        }
        let m = self.file.obj(chart);
        let all: Vec<(u64, ObjId)> = m
            .and_then(|m| m.msg("unity"))
            .and_then(|u| u.msg("series_non_styles"))
            .map(|a| a.msgs("entries").filter_map(|e| Some((e.u64("index")?, e.ref_id("reference")?))).collect())
            .unwrap_or_default();
        all.into_iter().map(|(_, id)| id).collect()
    }

    pub fn set_chart_flag(&mut self, h: &mut History, chart: ObjId, flag: ChartFlag) -> bool {
        let Some(unity) = self.file.obj(chart).and_then(|m| m.msg("unity")).cloned() else { return false };
        let set = |doc: &mut Document, h: &mut History, id: Option<ObjId>, name: &str, v: bool| -> bool {
            let Some(id) = id else { return false };
            doc.touch(h, id);
            let Some(m) = doc.file.obj_mut(id) else { return false };
            let cur = m.msg_mut_or_create("current");
            // a property this message doesn't have (style vs non-style) is skipped, not forced
            if !cur.has_field(name) {
                return false;
            }
            cur.set_bool(name, v);
            true
        };
        match flag {
            // style properties go on the chart's own variation of its style (chartstyle.rs), so
            // charts sharing the preset keep theirs
            ChartFlag::Border(on) => self.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultshowborder", &ChartValue::Bool(on)),
            ChartFlag::Axis(idx, prop, on) => {
                let xy = matches!(unity.enum_name("chart_type"), Some(t) if t.starts_with("scatter") || t.starts_with("bubble"));
                let (which, part) = match idx {
                    Some(i) => ("value", ChartPart::ValueAxis(i)),
                    None if xy => ("value", ChartPart::ValueAxis(1)),
                    None => ("category", ChartPart::CategoryAxis),
                };
                let ok = self.set_chart_prop(h, chart, part, &format!("tschchartaxis{which}{prop}"), &ChartValue::Bool(on));
                // a title shown for the first time gets Keynote's default text
                if ok && prop == "showtitle" && on {
                    let key = format!("tschchartaxis{which}title");
                    if self.chart_prop(chart, part, &key).is_none() {
                        let default_title = if which == "value" { "Value Axis" } else { "Category Axis" };
                        self.set_chart_prop(h, chart, part, &key, &ChartValue::Str(default_title.into()));
                    }
                }
                ok
            }
            ChartFlag::ValueLabels(on) => {
                let ids = self.ensure_series_non_styles(h, chart);
                let mut any = false;
                for (i, id) in ids.into_iter().enumerate() {
                    any |= set(self, h, Some(id), "tschchartseriesdefaultshowvaluelabels", on);
                    // Numbers writes the chart type's own switch (barshowvaluelabels...; N29)
                    if let Some(name) = self.series_prop_name(chart, i, "showvaluelabels") {
                        set(self, h, Some(id), &name, on);
                    }
                }
                any
            }
            ChartFlag::SeriesNames(on) => {
                let ids = self.ensure_series_non_styles(h, chart);
                let mut any = false;
                for id in ids {
                    any |= set(self, h, Some(id), "tschchartseriespieshowserieslabels", on);
                    any |= set(self, h, Some(id), "tschchartseriesbubbleshowserieslabels", on);
                }
                any
            }
        }
    }
}

#[cfg(test)]
mod series_names_tests {
    use super::*;
    use crate::edit::History;
    use crate::insert::NewObject;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn pie_series_names_flag_reaches_scene() {
        let mut d = Document::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/ref/R10-standard.key")).unwrap();
        let s = d.slides();
        let mut h = History::default();
        d.begin(&mut h, "c");
        let id = d.insert(&mut h, s[0].slide, &NewObject::Chart { pie: true }).unwrap();
        assert!(d.set_chart_flag(&mut h, id, ChartFlag::SeriesNames(true)));
        d.end(&mut h);
        let sc = d.scene(&d.slides(), 0).unwrap();
        let c = sc.drawables.iter().find_map(|x| match &x.kind {
            crate::DrawableKind::Chart(c) if x.id == id => Some(c.clone()),
            _ => None,
        });
        assert!(c.unwrap().series.iter().all(|s| s.show_series_names));
    }
}
