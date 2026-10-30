//! Format > Chart, Axis and Series panes, shared by Keydeck, Values and Letters (Keynote,
//! Numbers and Pages have the same chart inspector). Each pane edits the document itself (one
//! undo step per change) and says what happened; the app redraws and opens its own data editor.

use egui::{Color32, RichText};
use kn_format::ObjId;
use kn_model::chartdata::ChartFlag;
use kn_model::chartstyle::{ChartPart, ChartValue};
use kn_model::edit::History;
use kn_model::format::{Dash, FillSpec, StrokeSpec};
use kn_model::tablefmt::CellFormat;
use kn_model::{ChartData, Color, Document, SeriesKind};

use crate::theme;

/// Which axis and series the panes show (kept by the app between frames).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartPanes {
    /// 0 value (Y), 1 category (X), 2 second value axis.
    pub axis: usize,
    pub series: usize,
}

/// What a pane did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneAction {
    None,
    /// The document changed: redraw the chart.
    Changed,
    /// Edit Chart Data was clicked: the app opens its data editor.
    EditData,
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8)
}

fn kcol(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::rgba(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, f32::from(a) / 255.0)
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new(title).strong());
}

/// One undo step around `f`; Changed when it changed anything. Repeats of the same change in
/// quick succession (a colour or stepper drag) merge into that one step.
fn edit(doc: &mut Document, h: &mut History, label: &str, f: impl FnOnce(&mut Document, &mut History) -> bool) -> PaneAction {
    doc.begin_merge(h, label);
    let ok = f(doc, h);
    doc.end(h);
    if ok { PaneAction::Changed } else { PaneAction::None }
}

fn merge(a: &mut PaneAction, b: PaneAction) {
    if b != PaneAction::None {
        *a = b;
    }
}

/// Number formats offered for labels: (name, format for `decimals`).
const FORMATS: [&str; 4] = ["Automatic", "Number", "Percentage", "Currency"];

fn format_of(kind: usize, decimals: u32) -> CellFormat {
    match kind {
        1 => CellFormat::Number { decimals: Some(decimals), thousands: true, negative: 0 },
        2 => CellFormat::Percent { decimals: Some(decimals), thousands: false, negative: 0 },
        3 => CellFormat::Currency { code: "USD".into(), decimals: Some(decimals), thousands: true, negative: 0, accounting: false },
        _ => CellFormat::Automatic,
    }
}

/// (kind index, decimals) of a stored format.
fn read_format(f: Option<&kn_format::Msg>) -> (usize, u32) {
    let Some(f) = f else { return (0, 0) };
    let dec = f.u32("decimal_places").filter(|d| *d < 200).unwrap_or(0);
    let kind = match f.u32("format_type") {
        Some(257) => 3,
        Some(258) => 2,
        Some(256) if f.u32("decimal_places").is_some_and(|d| d < 200) => 1,
        _ => 0,
    };
    (kind, dec)
}

/// The format popup and decimals stepper; Some(new format) when changed.
fn format_controls(ui: &mut egui::Ui, id: &str, cur: Option<&kn_format::Msg>) -> Option<CellFormat> {
    let (mut kind, decimals) = read_format(cur);
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label("Format");
        let before = kind;
        theme::combo(id).selected_text(FORMATS[kind]).show_ui(ui, |ui| {
            for (i, n) in FORMATS.iter().enumerate() {
                ui.selectable_value(&mut kind, i, *n);
            }
        });
        let mut d = f64::from(decimals);
        let dec_changed = kind != 0 && theme::stepper(ui, &mut d, 1.0, 0.0..=10.0, "", 0);
        if kind != before || dec_changed {
            out = Some(format_of(kind, d as u32));
        }
    });
    out
}

/// A colour well for an optional colour; Some(new) when changed.
fn well(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, cur: Color) -> Option<Color> {
    let mut c = c32(cur);
    crate::colors::color_well(ui, id, &mut c, 110.0).then(|| kcol(c))
}

/// Format > Chart: data, type, options (legend, title, border), background, corners, gaps.
pub fn chart_tab(ui: &mut egui::Ui, doc: &mut Document, h: &mut History, chart: ObjId, c: &ChartData) -> PaneAction {
    chart_tab_labeled(ui, doc, h, chart, c, "Edit Chart Data")
}

/// The Chart tab with its data button named `edit_label` (Numbers' charts linked to table
/// cells: "Edit Data References").
pub fn chart_tab_labeled(ui: &mut egui::Ui, doc: &mut Document, h: &mut History, chart: ObjId, c: &ChartData, edit_label: &str) -> PaneAction {
    let mut act = PaneAction::None;
    if ui.button(edit_label).clicked() {
        act = PaneAction::EditData;
    }
    ui.label(RichText::new(format!("{} series × {} categories", c.series.len(), c.categories.len())).color(theme::TEXT_DIM()));

    // the theme's chart styles, as swatches of their series colours (Keynote / Numbers'
    // six thumbnails)
    let presets = doc.chart_presets();
    if !presets.is_empty() {
        section(ui, "Chart Styles");
        let current = doc.chart_preset(chart);
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            for p in &presets {
                let cols = doc.chart_preset_colors(*p);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(44.0, 32.0), egui::Sense::click());
                let painter = ui.painter();
                painter.rect_filled(r, 4.0, theme::CONTROL_BG());
                let n = cols.len().clamp(1, 4);
                let bw = (r.width() - 12.0) / n as f32;
                for (k, c) in cols.iter().take(n).enumerate() {
                    let hgt = r.height() * (0.35 + 0.15 * k as f32);
                    let x = r.min.x + 6.0 + bw * k as f32;
                    painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x + 1.0, r.max.y - 4.0 - hgt), egui::pos2(x + bw - 1.0, r.max.y - 4.0)), 1.0, c32(*c));
                }
                let on = current == Some(*p);
                if on || resp.hovered() {
                    painter.rect_stroke(r, 4.0, egui::Stroke::new(if on { 2.0 } else { 1.0 }, theme::ACCENT()), egui::StrokeKind::Inside);
                }
                if resp.clicked() && !on {
                    pick = Some(*p);
                }
            }
        });
        if let Some(p) = pick {
            merge(&mut act, edit(doc, h, "Chart Style", |d, h| d.apply_chart_preset(h, chart, p)));
        }
    }

    section(ui, "Chart Font");
    let fonts = ["HelveticaNeue", "Avenir-Book", "AvenirNext-Regular", "Futura-Medium", "GillSans", "Georgia", "TimesNewRomanPSMT", "Menlo-Regular"];
    let cur_font = c.value_label_style.font_name.clone();
    let mut font_pick = None;
    let mut scale = 1.0;
    ui.horizontal(|ui| {
        theme::combo("chart-font").selected_text(cur_font.clone()).width(150.0).show_ui(ui, |ui| {
            for f in fonts {
                if ui.selectable_label(f == cur_font, f).clicked() {
                    font_pick = Some(f);
                }
            }
        });
        if ui.button(RichText::new("A").size(11.0)).on_hover_text("Smaller").clicked() {
            scale = 1.0 / 1.11;
        }
        if ui.button(RichText::new("A").size(16.0)).on_hover_text("Bigger").clicked() {
            scale = 1.11;
        }
    });
    if font_pick.is_some() || scale != 1.0 {
        merge(&mut act, edit(doc, h, "Chart Font", |d, h| d.set_chart_font(h, chart, font_pick, scale)));
    }

    section(ui, "Chart Type");
    let types = kn_model::chartdata::CHART_TYPES;
    let shown = types.iter().find(|t| t.0 == c.chart_type).map(|t| t.1.to_string()).unwrap_or_else(|| c.chart_type.clone());
    let mut pick = None;
    theme::combo("chart-type").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
        for (k, name) in types {
            if ui.selectable_label(*k == c.chart_type, *name).clicked() {
                pick = Some(*k);
            }
        }
    });
    if let Some(p) = pick.filter(|p| *p != c.chart_type) {
        merge(&mut act, edit(doc, h, "Chart Type", |d, h| d.set_chart_type(h, chart, p)));
    }

    section(ui, "Chart Options");
    let mut legend = c.show_legend;
    if theme::checkbox(ui, &mut legend, "Legend").changed() {
        merge(&mut act, edit(doc, h, "Legend", |d, h| d.set_chart_legend(h, chart, legend)));
    }
    let mut has_title = c.title.is_some();
    if theme::checkbox(ui, &mut has_title, "Title").changed() {
        let t = has_title.then(|| c.title.clone().unwrap_or_else(|| "Title".into()));
        merge(&mut act, edit(doc, h, "Chart Title", |d, h| d.set_chart_title(h, chart, t.as_deref())));
    }
    if let Some(title) = &c.title {
        let id = egui::Id::unique(("chart-title", chart));
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| title.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            if text != *title {
                merge(&mut act, edit(doc, h, "Chart Title", |d, h| d.set_chart_title(h, chart, Some(&text))));
            }
        }
    }
    let mut border = c.show_border;
    if theme::checkbox(ui, &mut border, "Border").changed() {
        merge(&mut act, edit(doc, h, "Border", |d, h| d.set_chart_flag(h, chart, ChartFlag::Border(border))));
    }
    if c.show_border {
        let cur = c.border.clone();
        let (col, width) = cur.as_ref().map(|s| (s.color, s.width)).unwrap_or((Color::rgba(0.6, 0.6, 0.6, 1.0), 1.0));
        let mut w = width;
        let mut new = None;
        ui.horizontal(|ui| {
            if let Some(nc) = well(ui, ("chart-border", chart), col) {
                new = Some((nc, w));
            }
            if theme::stepper32(ui, &mut w, 0.5, 0.0..=20.0, " pt", 1) {
                new = Some((col, w));
            }
        });
        if let Some((nc, nw)) = new {
            let s = StrokeSpec { color: nc, width: nw, dash: Dash::Solid };
            merge(&mut act, edit(doc, h, "Border", |d, h| d.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultborderstroke", &ChartValue::Stroke(Some(s)))));
        }
    }

    section(ui, "Background");
    let bg = c.background.as_ref().and_then(|f| match f {
        kn_model::Fill::Color(c) => Some(*c),
        kn_model::Fill::Gradient { stops, .. } => stops.first().map(|s| s.1),
        _ => None,
    });
    let mut on = bg.is_some();
    let mut fill: Option<FillSpec> = None;
    ui.horizontal(|ui| {
        if theme::checkbox(ui, &mut on, "Fill").changed() {
            fill = Some(if on { FillSpec::Color(Color::rgba(1.0, 1.0, 1.0, 1.0)) } else { FillSpec::None });
        }
        if let Some(cur) = bg {
            if let Some(nc) = well(ui, ("chart-bg", chart), cur) {
                fill = Some(FillSpec::Color(nc));
            }
        }
    });
    if let Some(f) = fill {
        merge(&mut act, edit(doc, h, "Chart Background", |d, h| d.set_chart_background(h, chart, &f)));
    }

    let bars = c.series.iter().any(|s| matches!(s.kind, SeriesKind::Column | SeriesKind::Bar));
    if bars {
        section(ui, "Bars");
        // a percentage, stored as a fraction (N29)
        let mut r = c.corner_radius * 100.0;
        let mut outer = c.corner_outer_only;
        ui.horizontal(|ui| {
            ui.label("Rounded Corners");
            if theme::stepper32(ui, &mut r, 5.0, 0.0..=100.0, "%", 0) {
                merge(&mut act, edit(doc, h, "Rounded Corners", |d, h| d.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius", &ChartValue::Float(r / 100.0))));
            }
        });
        if c.corner_radius > 0.0 && theme::checkbox(ui, &mut outer, "Outside Corners Only").changed() {
            merge(&mut act, edit(doc, h, "Rounded Corners", |d, h| d.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultroundedcornerouterendonly", &ChartValue::Bool(outer))));
        }
        for (label, name, cur) in [("Gap between bars", "tschchartinfodefaultinterbargap", c.bar_gap), ("Gap between sets", "tschchartinfodefaultintersetgap", c.set_gap)] {
            let mut pct = cur * 100.0;
            ui.horizontal(|ui| {
                ui.label(label);
                if theme::stepper32(ui, &mut pct, 5.0, 0.0..=500.0, "%", 0) {
                    merge(&mut act, edit(doc, h, label, |d, h| d.set_chart_prop(h, chart, ChartPart::Chart, name, &ChartValue::Float(pct))));
                }
            });
        }
    }
    if c.chart_type.to_ascii_lowercase().contains("donut") {
        section(ui, "Donut");
        let mut r = c.inner_radius * 100.0;
        ui.horizontal(|ui| {
            ui.label("Inner Radius");
            if theme::stepper32(ui, &mut r, 5.0, 0.0..=95.0, "%", 0) {
                merge(&mut act, edit(doc, h, "Inner Radius", |d, h| d.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultinnerradius", &ChartValue::Float(r / 100.0))));
            }
        });
    }
    act
}

/// Format > Axis: Value (Y) / Category (X) (and the second Y axis): options, scale, gridlines,
/// tick marks, label angle and number format.
pub fn axis_tab(ui: &mut egui::Ui, doc: &mut Document, h: &mut History, chart: ObjId, c: &ChartData, st: &mut ChartPanes) -> PaneAction {
    let mut act = PaneAction::None;
    let two = c.value_axes.len() > 1 && c.chart_type.starts_with("twoAxis");
    let mut names = vec!["Value (Y)", "Category (X)"];
    if two {
        names.push("Value (Y2)");
    }
    st.axis = st.axis.min(names.len() - 1);
    ui.horizontal(|ui| {
        for (i, n) in names.iter().enumerate() {
            if ui.selectable_label(st.axis == i, *n).clicked() {
                st.axis = i;
            }
        }
    });
    let xy = c.chart_type.starts_with("scatter") || c.chart_type.starts_with("bubble");
    let (axis, part, which, flag_idx) = match st.axis {
        1 if xy => (c.category_axis.clone(), ChartPart::ValueAxis(1), "value", None),
        1 => (c.category_axis.clone(), ChartPart::CategoryAxis, "category", None),
        2 => (c.value_axes.get(1).cloned().unwrap_or_default(), ChartPart::ValueAxis(1), "value", Some(1)),
        _ => (c.value_axes.first().cloned().unwrap_or_default(), ChartPart::ValueAxis(0), "value", Some(0)),
    };
    let is_value = which == "value";
    let set = |doc: &mut Document, h: &mut History, label: &str, prop: &str, v: ChartValue| edit(doc, h, label, |d, h| d.set_chart_prop(h, chart, part, &format!("tschchartaxis{which}{prop}"), &v));

    section(ui, "Axis Options");
    for (label, prop, on) in [("Axis Name", "showtitle", axis.title.is_some()), ("Labels", "showlabels", axis.show_labels), ("Axis Line", "showaxis", axis.show_line)] {
        let mut v = on;
        if theme::checkbox(ui, &mut v, label).changed() {
            let f = ChartFlag::Axis(if st.axis == 1 { None } else { flag_idx }, prop, v);
            merge(&mut act, edit(doc, h, label, |d, h| d.set_chart_flag(h, chart, f)));
        }
    }
    if axis.show_labels {
        let angles = [(0.0, "Horizontal"), (45.0, "45°"), (90.0, "Vertical"), (-45.0, "−45°")];
        let cur = angles.iter().find(|a| (a.0 - axis.label_angle).abs() < 0.5).map(|a| a.1).unwrap_or("Custom");
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label("Label Angle");
            theme::combo(("axis-angle", st.axis)).selected_text(cur).show_ui(ui, |ui| {
                for (a, n) in angles {
                    if ui.selectable_label(cur == n, n).clicked() {
                        pick = Some(a);
                    }
                }
            });
        });
        if let Some(a) = pick {
            merge(&mut act, set(doc, h, "Label Angle", "labelsorientation", ChartValue::Float(a)));
        }
        if is_value {
            if let Some(f) = format_controls(ui, "axis-format", axis.number_format.as_ref()) {
                let m = f.msg();
                merge(&mut act, edit(doc, h, "Number Format", |d, h| d.set_axis_number_format(h, chart, part, &m)));
            }
        }
    }

    if is_value {
        section(ui, "Axis Scale");
        let mut log = axis.log;
        if theme::checkbox(ui, &mut log, "Logarithmic").changed() {
            merge(&mut act, edit(doc, h, "Axis Scale", |d, h| d.set_chart_prop(h, chart, part, "tschchartaxisvaluescale", &ChartValue::Int(if log { 2 } else { 1 }))));
        }
        for (label, name, cur) in [("Min", "tschchartaxisdefaultusermin", axis.min), ("Max", "tschchartaxisdefaultusermax", axis.max)] {
            ui.horizontal(|ui| {
                let mut auto = cur.is_none();
                ui.label(label);
                if theme::checkbox(ui, &mut auto, "Auto").changed() && !auto {
                    let v = if label == "Min" { 0.0 } else { 100.0 };
                    merge(&mut act, edit(doc, h, label, |d, h| d.set_chart_prop(h, chart, part, name, &ChartValue::Number(v))));
                }
                if let Some(mut v) = cur {
                    if theme::stepper(ui, &mut v, 1.0, -1e12..=1e12, "", 2) {
                        merge(&mut act, edit(doc, h, label, |d, h| d.set_chart_prop(h, chart, part, name, &ChartValue::Number(v))));
                    }
                }
            });
        }
        let mut major = f64::from(axis.intervals.unwrap_or(4));
        ui.horizontal(|ui| {
            ui.label("Major Steps");
            if theme::stepper(ui, &mut major, 1.0, 1.0..=20.0, "", 0) {
                merge(&mut act, edit(doc, h, "Major Steps", |d, h| d.set_chart_prop(h, chart, part, "tschchartaxisvaluenumberofmajorgridlines", &ChartValue::Int(major as i64))));
            }
        });
        let mut minor = f64::from(axis.minor_intervals.unwrap_or(2));
        ui.horizontal(|ui| {
            ui.label("Minor Steps");
            if theme::stepper(ui, &mut minor, 1.0, 1.0..=20.0, "", 0) {
                merge(&mut act, edit(doc, h, "Minor Steps", |d, h| d.set_chart_prop(h, chart, part, "tschchartaxisvaluenumberofminorgridlines", &ChartValue::Int(minor as i64))));
            }
        });
    }

    section(ui, "Gridlines and Tick Marks");
    for (label, prop, on) in [
        ("Major Gridlines", "showmajorgridlines", axis.show_gridlines),
        ("Minor Gridlines", "showminorgridlines", axis.show_minor_gridlines),
        ("Major Tick Marks", "showmajortickmarks", axis.show_major_ticks),
        ("Minor Tick Marks", "showminortickmarks", axis.show_minor_ticks),
    ] {
        let mut v = on;
        if theme::checkbox(ui, &mut v, label).changed() {
            merge(&mut act, set(doc, h, label, prop, ChartValue::Bool(v)));
        }
    }
    if axis.show_gridlines {
        let g = axis.gridline.clone();
        let (col, width) = g.as_ref().map(|s| (s.color, s.width)).unwrap_or((Color::rgba(0.72, 0.72, 0.72, 1.0), 1.0));
        let mut w = width;
        let mut new = None;
        ui.horizontal(|ui| {
            ui.label("Gridlines");
            if let Some(nc) = well(ui, ("grid", chart, st.axis), col) {
                new = Some((nc, w));
            }
            if theme::stepper32(ui, &mut w, 0.5, 0.0..=10.0, " pt", 1) {
                new = Some((col, w));
            }
        });
        if let Some((nc, nw)) = new {
            let s = StrokeSpec { color: nc, width: nw, dash: Dash::Solid };
            merge(&mut act, set(doc, h, "Gridlines", "majorgridlinestroke", ChartValue::Stroke(Some(s))));
        }
    }
    act
}

/// Format > Series: one series' fill and outline, value labels and their format, pie wedge
/// explosion; value labels on or off for every series.
pub fn series_tab(ui: &mut egui::Ui, doc: &mut Document, h: &mut History, chart: ObjId, c: &ChartData, st: &mut ChartPanes) -> PaneAction {
    let mut act = PaneAction::None;
    if c.series.is_empty() {
        return act;
    }
    st.series = st.series.min(c.series.len() - 1);
    let i = st.series;
    let s = &c.series[i];

    section(ui, "Value Labels");
    let mut v = c.series.iter().any(|s| s.show_value_labels);
    if theme::checkbox(ui, &mut v, "Show value labels").changed() {
        merge(&mut act, edit(doc, h, "Value Labels", |d, h| d.set_chart_flag(h, chart, ChartFlag::ValueLabels(v))));
    }
    if matches!(s.kind, SeriesKind::Pie | SeriesKind::Bubble) {
        let mut n = c.series.iter().any(|s| s.show_series_names);
        if theme::checkbox(ui, &mut n, "Series names").changed() {
            merge(&mut act, edit(doc, h, "Series Names", |d, h| d.set_chart_flag(h, chart, ChartFlag::SeriesNames(n))));
        }
    }
    if v {
        if let Some(f) = format_controls(ui, "series-format", s.value_format.as_ref()) {
            let m = f.msg();
            let n = c.series.len();
            // Keynote formats every series' labels alike from this pane
            merge(&mut act, edit(doc, h, "Number Format", |d, h| (0..n).fold(false, |ok, k| d.set_series_number_format(h, chart, k, &m) || ok)));
        }
    }

    section(ui, "Series");
    theme::combo("chart-series").selected_text(s.name.clone()).width(ui.available_width()).show_ui(ui, |ui| {
        for (k, ser) in c.series.iter().enumerate() {
            ui.selectable_value(&mut st.series, k, ser.name.as_str());
        }
    });
    ui.horizontal(|ui| {
        ui.label("Fill");
        if let Some(nc) = well(ui, ("series-fill", chart, i), s.color) {
            merge(&mut act, edit(doc, h, "Series Fill", |d, h| d.set_series_fill(h, chart, i, &FillSpec::Color(nc))));
        }
    });
    let mut outline = s.stroke.as_ref().is_some_and(|k| k.width > 0.0);
    let line_kind = matches!(s.kind, SeriesKind::Line | SeriesKind::Scatter | SeriesKind::Radar);
    ui.horizontal(|ui| {
        let label = if line_kind { "Line" } else { "Outline" };
        let toggled = !line_kind && theme::checkbox(ui, &mut outline, label).changed();
        if line_kind {
            ui.label(label);
        }
        let (col, width) = s.stroke.as_ref().map(|k| (k.color, k.width)).unwrap_or((Color::rgba(0.0, 0.0, 0.0, 1.0), 1.0));
        let mut new: Option<Option<StrokeSpec>> = toggled.then(|| outline.then_some(StrokeSpec { color: col, width: width.max(1.0), dash: Dash::Solid }));
        if outline || line_kind {
            let mut w = width;
            if let Some(nc) = well(ui, ("series-stroke", chart, i), col) {
                new = Some(Some(StrokeSpec { color: nc, width: w, dash: Dash::Solid }));
            }
            if theme::stepper32(ui, &mut w, 0.5, 0.0..=20.0, " pt", 1) {
                new = Some(Some(StrokeSpec { color: col, width: w, dash: Dash::Solid }));
            }
        }
        if let Some(stroke) = new {
            merge(&mut act, edit(doc, h, "Series Line", |d, h| d.set_series_stroke(h, chart, i, stroke.as_ref())));
        }
    });
    if s.kind == SeriesKind::Pie {
        // label distance: every wedge alike, as the Wedges pane does (both fields Numbers writes)
        let mut dist = s.label_distance.unwrap_or(62.0);
        ui.horizontal(|ui| {
            ui.label("Distance from Center");
            if theme::stepper32(ui, &mut dist, 5.0, 0.0..=200.0, "%", 0) {
                let n = c.series.len();
                merge(&mut act, edit(doc, h, "Label Distance", |d, h| {
                    (0..n).fold(false, |ok, k| {
                        let a = d.set_chart_prop(h, chart, ChartPart::Series(k), "tschchartseriespielabelexplosion", &ChartValue::Float(dist));
                        d.set_chart_prop(h, chart, ChartPart::Series(k), "tschchartseriespie2_3labelexplosion", &ChartValue::Float(dist));
                        a || ok
                    })
                }));
            }
        });
        let mut e = s.explosion * 100.0;
        ui.horizontal(|ui| {
            ui.label("Wedge Offset");
            if theme::stepper32(ui, &mut e, 5.0, 0.0..=100.0, "%", 0) {
                merge(&mut act, edit(doc, h, "Wedge Offset", |d, h| d.set_chart_prop(h, chart, ChartPart::Series(i), "tschchartseriespiewedgeexplosion", &ChartValue::Float(e / 100.0))));
            }
        });
    }
    act
}
