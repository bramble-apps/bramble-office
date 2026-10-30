//! Charts (TSCH.ChartDrawableArchive with the ChartArchive in extension field "unity").
//!
//! Chart, axis, series and legend looks live in style / non-style archives whose properties sit
//! in a generated `current` message (TSCHArchives.GEN.proto, e.g. `tschchartseriescolumnfill`).
//! A series takes its properties from its non-style, then its private style, then the theme
//! series style for its index; anything unset falls back to Keynote's built-in defaults.

use kn_format::{KeyFile, Msg, ObjId};

use crate::style::{self, chain};
use crate::types::*;

/// The generated property messages of a style (and its parents), most specific first.
fn currents(k: &KeyFile, id: Option<ObjId>) -> Vec<&Msg> {
    chain(k, id).into_iter().filter_map(|m| m.msg("current")).collect()
}

fn find<'a, T>(props: &[&'a Msg], get: impl Fn(&'a Msg) -> Option<T>) -> Option<T> {
    props.iter().find_map(|m| get(m))
}

/// Entry `i` of a TSP.SparseReferenceArray.
fn sparse(m: Option<&Msg>, i: usize) -> Option<ObjId> {
    m?.msgs("entries").find(|e| e.u32("index") == Some(i as u32))?.ref_id("reference")
}

fn fill_color(f: &Fill) -> Option<Color> {
    match f {
        Fill::Color(c) => Some(*c),
        Fill::Gradient { stops, .. } => stops.first().map(|s| s.1),
        Fill::Image { tint, .. } => *tint,
        Fill::None => None,
    }
}

fn is_mixed(chart_type: &str) -> bool {
    chart_type.starts_with("mixed") || chart_type.starts_with("twoAxis")
}

fn base_kind(chart_type: &str) -> SeriesKind {
    let t = chart_type.to_ascii_lowercase();
    if t.contains("pie") || t.contains("donut") {
        SeriesKind::Pie
    } else if t.contains("bubble") {
        SeriesKind::Bubble
    } else if t.contains("scatter") {
        SeriesKind::Scatter
    } else if t.contains("radar") {
        SeriesKind::Radar
    } else if t.contains("bar") {
        SeriesKind::Bar
    } else if t.contains("line") {
        SeriesKind::Line
    } else if t.contains("area") {
        SeriesKind::Area
    } else {
        SeriesKind::Column
    }
}

/// Property-name stem of a series kind in a chart type, e.g. "mixedcolumn", "3dbar", "pie".
pub(crate) fn stem(kind: SeriesKind, chart_type: &str) -> &'static str {
    let mixed = is_mixed(chart_type);
    let three_d = chart_type.ends_with("3D");
    match kind {
        SeriesKind::Column if mixed => "mixedcolumn",
        SeriesKind::Line if mixed => "mixedline",
        SeriesKind::Area if mixed => "mixedarea",
        SeriesKind::Column if three_d => "3dcolumn",
        SeriesKind::Bar if three_d => "3dbar",
        SeriesKind::Line if three_d => "3dline",
        SeriesKind::Area if three_d => "3darea",
        SeriesKind::Pie if three_d => "3dpie",
        SeriesKind::Column => "column",
        SeriesKind::Bar => "bar",
        SeriesKind::Line => "line",
        SeriesKind::Area => "area",
        SeriesKind::Pie => "pie",
        SeriesKind::Scatter => "scatter",
        SeriesKind::Bubble => "bubble",
        SeriesKind::Radar => "radararea",
    }
}

/// Stem used by the value label properties ("bar" covers columns too).
pub(crate) fn label_stem(kind: SeriesKind, chart_type: &str) -> &'static str {
    if is_mixed(chart_type) {
        return "mixed";
    }
    match kind {
        SeriesKind::Column | SeriesKind::Bar => "bar",
        SeriesKind::Line => "line",
        SeriesKind::Area => "area",
        SeriesKind::Pie => "pie",
        SeriesKind::Scatter => "scatter",
        SeriesKind::Bubble => "bubble",
        SeriesKind::Radar => "radar",
    }
}

/// Series kind of series `i` in a mixed / 2-axis chart: the series type when set
/// (0 column, 1 line, 2 area; inferred), else the default alternation seen in Keynote's
/// output (line, column, line, ...).
fn mixed_kind(props: &[&Msg], i: usize) -> SeriesKind {
    match find(props, |p| p.i64("tschchartseriesdefaultseriestype")) {
        Some(1) => SeriesKind::Line,
        Some(2) => SeriesKind::Area,
        Some(_) => SeriesKind::Column,
        None if i % 2 == 0 => SeriesKind::Line,
        None => SeriesKind::Column,
    }
}

/// Raw series (name, values) and category names for the grid and series direction.
fn grid_series(c: &ChartData) -> (Vec<(String, Vec<f64>)>, Vec<String>) {
    if c.series_by_row {
        let s = c.values.iter().enumerate().map(|(i, v)| (c.row_names.get(i).cloned().unwrap_or_default(), v.clone()));
        (s.collect(), c.col_names.clone())
    } else {
        let ncols = c.values.iter().map(Vec::len).max().unwrap_or(0).max(c.col_names.len());
        let s = (0..ncols).map(|j| {
            let vals = c.values.iter().map(|r| r.get(j).copied().unwrap_or(f64::NAN)).collect();
            (c.col_names.get(j).cloned().unwrap_or_default(), vals)
        });
        (s.collect(), c.row_names.clone())
    }
}

/// Split raw series into scatter / bubble series (name, xs, ys, sizes): shared X (the first
/// series is X, then Y or Y + size) or separate X (X, Y[, size] per series).
type XySeries = (String, Vec<f64>, Vec<f64>, Vec<f64>);
fn xy_series(raw: Vec<(String, Vec<f64>)>, bubble: bool, shared_x: bool) -> Vec<XySeries> {
    let mut out = Vec::new();
    let per = if bubble { 2 } else { 1 };
    if shared_x {
        let Some((_, xs)) = raw.first().cloned() else { return out };
        for group in raw[1..].chunks(per) {
            let sizes = if bubble { group.get(1).map(|g| g.1.clone()).unwrap_or_default() } else { Vec::new() };
            out.push((group[0].0.clone(), xs.clone(), group[0].1.clone(), sizes));
        }
    } else {
        for group in raw.chunks(per + 1) {
            let (Some(x), Some(y)) = (group.first(), group.get(1)) else { continue };
            let sizes = if bubble { group.get(2).map(|g| g.1.clone()).unwrap_or_default() } else { Vec::new() };
            out.push((y.0.clone(), x.1.clone(), y.1.clone(), sizes));
        }
    }
    out
}

struct Ctx<'a> {
    k: &'a KeyFile,
    unity: &'a Msg,
    chart_type: &'a str,
    paragraph_styles: Vec<ObjId>,
}

impl Ctx<'_> {
    /// Character style of chart paragraph style `index` (indices come from the style properties).
    fn para(&self, index: i64) -> CharStyle {
        let id = usize::try_from(index).ok().and_then(|i| self.paragraph_styles.get(i).copied());
        style::char_style(self.k, id, None)
    }

    /// Property messages for series `i`: non-style, private style, theme style.
    fn series_props(&self, i: usize) -> Vec<&Msg> {
        let mut out = currents(self.k, sparse(self.unity.msg("series_non_styles"), i));
        out.extend(currents(self.k, sparse(self.unity.msg("series_private_styles"), i)));
        let theme = self.unity.ref_ids("series_theme_styles");
        if !theme.is_empty() {
            out.extend(currents(self.k, Some(theme[i % theme.len()])));
        }
        out
    }

    fn series(&self, i: usize, name: String, values: Vec<f64>) -> ChartSeries {
        let props = self.series_props(i);
        let kind = if is_mixed(self.chart_type) { mixed_kind(&props, i) } else { base_kind(self.chart_type) };
        let st = stem(kind, self.chart_type);
        let fill_of = |name: &str| find(&props, |p| p.msg(name).map(style::fill)).filter(|f| *f != Fill::None);
        let stroke = find(&props, |p| p.msg(&format!("tschchartseries{st}stroke")).map(style::stroke)).flatten();
        let mut fill = match kind {
            SeriesKind::Bubble => fill_of("tschchartseriesbubblesymbolfill"),
            SeriesKind::Scatter => {
                let use_stroke = find(&props, |p| p.bool("tschchartseriesscattersymbolfilluseseriesstroke")).unwrap_or(true);
                if use_stroke { None } else { fill_of("tschchartseriesscattersymbolfill") }
            }
            SeriesKind::Line if st == "3dline" => fill_of("tschchartseries3dlinefill"),
            SeriesKind::Line | SeriesKind::Radar => None,
            _ => fill_of(&format!("tschchartseries{st}fill")),
        }
        .or_else(|| stroke.as_ref().map(|s| Fill::Color(s.color)))
        .unwrap_or(Fill::None);
        if kind == SeriesKind::Radar {
            let alpha = find(&props, |p| p.f32("tschchartseriesradarareafilluseseriesstrokealphamultiplier")).unwrap_or(0.15);
            if let Some(s) = &stroke {
                fill = Fill::Color(s.color.with_alpha(alpha));
            }
        }
        let color = match kind {
            SeriesKind::Line if st == "3dline" => None,
        SeriesKind::Line | SeriesKind::Radar | SeriesKind::Scatter => stroke.as_ref().map(|s| s.color),
            _ => None,
        }
        .or_else(|| fill_color(&fill))
        .unwrap_or(Color::rgba(0.0, 0.63, 1.0, 1.0));
        let ls = label_stem(kind, self.chart_type);
        let show_value_labels = find(&props, |p| p.bool(&format!("tschchartseries{ls}showvaluelabels")))
            .or_else(|| find(&props, |p| p.bool("tschchartseriesdefaultshowvaluelabels")))
            .unwrap_or(kind == SeriesKind::Pie);
        let show_series_names = match kind {
            SeriesKind::Pie => find(&props, |p| p.bool("tschchartseriespieshowserieslabels")),
            SeriesKind::Bubble => find(&props, |p| p.bool("tschchartseriesbubbleshowserieslabels")),
            _ => None,
        }
        .unwrap_or(false);
        let symbol_prop = match kind {
            SeriesKind::Scatter => Some("tschchartseriesscattershowsymbol"),
            SeriesKind::Line if st == "mixedline" => Some("tschchartseriesmixedlineshowsymbol"),
            SeriesKind::Line => Some("tschchartserieslineshowsymbol"),
            SeriesKind::Radar => Some("tschchartseriesradarareashowsymbol"),
            _ => None,
        };
        let show_symbols = symbol_prop.and_then(|p| find(&props, |m| m.bool(p))).unwrap_or(kind == SeriesKind::Scatter);
        let y2 = self.chart_type.starts_with("twoAxis")
            && find(&props, |p| p.i64("tschchartseriesdefaultyaxisordinal")).map(|o| o == 1).unwrap_or(i % 2 == 1);
        let fmt_name = if kind == SeriesKind::Pie { "tschchartseriespienumberformat" } else { "tschchartseriesdefaultnumberformat" };
        let value_format = find(&props, |p| p.msg(fmt_name).cloned());
        let explosion = find(&props, |p| p.f32("tschchartseriespiewedgeexplosion")).unwrap_or(0.0).clamp(0.0, 1.0);
        // Distance from Center, percent of the radius (outside the pie past 100)
        let label_distance = find(&props, |p| p.f32("tschchartseriespielabelexplosion")).filter(|v| *v > 0.0);
        let label_position = find(&props, |p| p.i64(&format!("tschchartseries{ls}valuelabelposition")))
            .or_else(|| find(&props, |p| p.i64("tschchartseriesdefaultvaluelabelposition")))
            .unwrap_or(0);
        ChartSeries { name, values, xs: Vec::new(), sizes: Vec::new(), kind, fill, color, stroke, y2, show_value_labels, show_series_names, show_symbols, value_format, explosion, label_distance, label_position }
    }

    /// An axis from its style and non-style; `which` is "value" or "category".
    fn axis(&self, style_id: Option<ObjId>, nonstyle_id: Option<ObjId>, which: &str) -> ChartAxis {
        let mut props = currents(self.k, nonstyle_id);
        props.extend(currents(self.k, style_id));
        let b = |name: &str| find(&props, |p| p.bool(&format!("tschchartaxis{which}{name}")));
        let user = |name: &str| find(&props, |p| p.msg(name)?.f64("number_archive"));
        let gridline_name = if self.chart_type.ends_with("3D") {
            format!("tschchartaxis{which}3dgridlinestroke")
        } else {
            format!("tschchartaxis{which}majorgridlinestroke")
        };
        let label_index = find(&props, |p| p.i64(&format!("tschchartaxis{which}labelparagraphstyleindex")))
            .or_else(|| find(&props, |p| p.i64("tschchartaxisdefaultlabelparagraphstyleindex")))
            .unwrap_or(3);
        ChartAxis {
            min: user("tschchartaxisdefaultusermin"),
            max: user("tschchartaxisdefaultusermax"),
            intervals: find(&props, |p| p.i64("tschchartaxisvaluenumberofmajorgridlines")).filter(|n| *n > 0).map(|n| n as u32),
            show_labels: b("showlabels").or_else(|| find(&props, |p| p.bool("tschchartaxisdefaultshowlabels"))).unwrap_or(true),
            show_line: b("showaxis").unwrap_or(true),
            show_gridlines: b("showmajorgridlines").unwrap_or(which == "value"),
            gridline: find(&props, |p| p.msg(&gridline_name).map(style::stroke)).flatten(),
            label_style: self.para(label_index),
            title: if b("showtitle").unwrap_or(false) {
                find(&props, |p| p.str(&format!("tschchartaxis{which}title"))).map(String::from)
            } else {
                None
            },
            label_angle: find(&props, |p| p.f32(&format!("tschchartaxis{which}labelsorientation")))
                .or_else(|| find(&props, |p| p.f32("tschchartaxisdefaultlabelsorientation")))
                .unwrap_or(0.0),
            number_format: find(&props, |p| p.msg("tschchartaxisdefaultnumberformat").cloned()),
            show_minor_gridlines: b("showminorgridlines").unwrap_or(false),
            minor_gridline: find(&props, |p| p.msg(&format!("tschchartaxis{which}minorgridlinestroke")).map(style::stroke)).flatten(),
            minor_intervals: find(&props, |p| p.i64("tschchartaxisvaluenumberofminorgridlines")).filter(|n| *n > 0).map(|n| n as u32),
            show_major_ticks: b("showmajortickmarks").unwrap_or(false),
            show_minor_ticks: b("showminortickmarks").unwrap_or(false),
            // 1 linear (Numbers' own linear charts store it), 2 logarithmic
            log: find(&props, |p| p.i64("tschchartaxisvaluescale")) == Some(2),
        }
    }
}

pub fn chart(k: &KeyFile, m: &Msg) -> Option<ChartData> {
    let c = m.msg("unity")?;
    let grid = c.msg("grid");
    let chart_type = c.enum_name("chart_type").unwrap_or("columnChartType2D").to_string();
    let mut data = ChartData {
        chart_type: chart_type.clone(),
        row_names: grid.map(|g| g.strs("row_name").into_iter().map(String::from).collect()).unwrap_or_default(),
        col_names: grid.map(|g| g.strs("column_name").into_iter().map(String::from).collect()).unwrap_or_default(),
        values: grid
            .map(|g| {
                g.msgs("grid_row")
                    .map(|r| r.msgs("value").map(|v| v.f64("numeric_value").unwrap_or(f64::NAN)).collect())
                    .collect()
            })
            .unwrap_or_default(),
        series_by_row: c.enum_name("series_direction") != Some("series_direction_by_column"),
        ..Default::default()
    };
    let ctx = Ctx { k, unity: c, chart_type: &chart_type, paragraph_styles: c.ref_ids("paragraph_styles") };
    let (raw, categories) = grid_series(&data);
    data.categories = categories;
    let kind = base_kind(&chart_type);
    let xy = matches!(kind, SeriesKind::Scatter | SeriesKind::Bubble);
    if xy {
        let shared = c.enum_name("scatter_format") != Some("scatter_format_separate_x");
        for (i, (name, xs, ys, sizes)) in xy_series(raw, kind == SeriesKind::Bubble, shared).into_iter().enumerate() {
            let mut s = ctx.series(i, name, ys);
            s.xs = xs;
            s.sizes = sizes;
            data.series.push(s);
        }
    } else {
        data.series = raw.into_iter().enumerate().map(|(i, (name, v))| ctx.series(i, name, v)).collect();
    }

    let vs = c.ref_ids("value_axis_styles");
    let vn = c.ref_ids("value_axis_nonstyles");
    data.value_axes =
        (0..vs.len().max(vn.len()).max(1)).map(|i| ctx.axis(vs.get(i).copied(), vn.get(i).copied(), "value")).collect();
    // scatter / bubble X axes are value axes too (the second value axis style)
    data.category_axis = if xy {
        ctx.axis(vs.get(1).copied(), vn.get(1).copied(), "value")
    } else {
        ctx.axis(c.ref_ids("category_axis_styles").first().copied(), c.ref_ids("category_axis_nonstyles").first().copied(), "category")
    };

    let mut info = currents(k, c.ref_id("chart_non_style"));
    info.extend(currents(k, c.ref_id("chart_style")));
    // unset: no legend in Keynote (its export of the feature catalog), a legend in Numbers
    // (N29-H2) and Pages
    let keynote = k.first_of("KN.DocumentArchive").is_some();
    data.show_legend = find(&info, |p| p.bool("tschchartinfodefaultshowlegend")).unwrap_or(!keynote);
    data.show_border = find(&info, |p| p.bool("tschchartinfodefaultshowborder")).unwrap_or(false);
    if find(&info, |p| p.bool("tschchartinfodefaultshowtitle")).unwrap_or(false) {
        // ticking Title without typing shows Keynote's default text
        data.title = Some(find(&info, |p| p.str("tschchartinfodefaulttitle")).unwrap_or("Title").to_string());
    }
    data.title_style = ctx.para(find(&info, |p| p.i64("tschchartinfodefaulttitleparagraphstyleindex")).unwrap_or(5));
    data.legend_frame = c.msg("legend_frame").and_then(|f| {
        let (o, s) = (f.msg("origin")?, f.msg("size")?);
        Some([o.f32("x")?, o.f32("y")?, s.f32("width")?, s.f32("height")?])
    });
    let legend = currents(k, c.ref_id("legend_style"));
    data.legend_style = ctx.para(find(&legend, |p| p.i64("tschlegendmodeldefaultlabelparagraphstyleindex")).unwrap_or(7));
    let first = ctx.series_props(0);
    let ls = label_stem(kind, &chart_type);
    let label_index = |name: &str, default: i64| {
        find(&first, |p| p.i64(&format!("tschchartseries{ls}{name}")))
            .or_else(|| find(&first, |p| p.i64(&format!("tschchartseriesdefault{name}"))))
            .unwrap_or(default)
    };
    data.value_label_style = ctx.para(label_index("labelparagraphstyleindex", 2));
    data.outside_label_style = ctx.para(label_index("outsidelabelparagraphstyleindex", 5));
    data.inner_radius = find(&info, |p| p.f32("tschchartinfodefaultinnerradius")).unwrap_or(0.75).clamp(0.0, 0.95);
    data.bar_gap = find(&info, |p| p.f32("tschchartinfodefaultinterbargap")).unwrap_or(10.0) / 100.0;
    data.set_gap = find(&info, |p| p.f32("tschchartinfodefaultintersetgap")).unwrap_or(40.0) / 100.0;
    // Format > Chart > background: the plot area's fill (Numbers writes gridbackgroundfill and an
    // empty backgroundfill, N29)
    data.background = find(&info, |p| p.msg("tschchartinfodefaultgridbackgroundfill").map(style::fill))
        .or_else(|| find(&info, |p| p.msg("tschchartinfodefaultbackgroundfill").map(style::fill)))
        .filter(|f| *f != Fill::None);
    data.border = find(&info, |p| p.msg("tschchartinfodefaultborderstroke").map(style::stroke)).flatten();
    // a fraction of half the bar's thickness (Rounded Corners 10 % = 0.1, N29)
    data.corner_radius = find(&info, |p| p.f32("tschchartinfodefaultroundedcornerradius")).unwrap_or(0.0).clamp(0.0, 1.0);
    data.corner_outer_only = find(&info, |p| p.bool("tschchartinfodefaultroundedcornerouterendonly")).unwrap_or(false);
    if chart_type.starts_with("multiData") {
        data.data_set = Some(c.u32("multidataset_index").unwrap_or(0));
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first chart on a catalog slide (1-based slide number).
    fn catalog_chart(slide: usize) -> ChartData {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let doc = crate::Document::open(path).unwrap();
        let slides = doc.slides();
        let scene = doc.scene(&slides, slide - 1).unwrap();
        scene
            .drawables
            .into_iter()
            .find_map(|d| if let DrawableKind::Chart(c) = d.kind { Some(c) } else { None })
            .expect("chart on slide")
    }

    fn close(a: Color, rgb: (f32, f32, f32)) -> bool {
        (a.r - rgb.0).abs() < 0.02 && (a.g - rgb.1).abs() < 0.02 && (a.b - rgb.2).abs() < 0.02
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn series_by_column_and_theme_colours() {
        let c = catalog_chart(45);
        assert!(!c.series_by_row);
        assert_eq!(c.categories, ["2024", "2025", "2026"]);
        let names: Vec<&str> = c.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Apples", "Pears", "Plums", "Figs"]);
        assert_eq!(c.series[3].values, [40.0, 35.0, 20.0]);
        assert!(close(c.series[0].color, (0.0, 0.63, 1.0)));
        assert!(close(c.series[3].color, (1.0, 0.15, 0.0)));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn mixed_two_axis_scatter_bubble_layouts() {
        let two = catalog_chart(104);
        assert_eq!(two.series.iter().map(|s| (s.kind, s.y2)).collect::<Vec<_>>(), [(SeriesKind::Line, false), (SeriesKind::Column, true)]);
        let sc = catalog_chart(101);
        assert_eq!(sc.series.len(), 3);
        assert_eq!(sc.series[0].xs[..3], [1.0, 3.0, 5.0]);
        let bubble = catalog_chart(102);
        assert_eq!(bubble.series.len(), 1);
        assert_eq!(bubble.series[0].sizes[0], 20.0);
        let pie = catalog_chart(99);
        assert_eq!(pie.series.len(), 6);
        assert!(pie.series.iter().all(|s| s.kind == SeriesKind::Pie && s.show_value_labels));
        assert_eq!(catalog_chart(113).data_set, Some(0));
    }
}
