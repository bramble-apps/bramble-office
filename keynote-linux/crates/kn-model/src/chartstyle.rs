//! Any chart property (Format > Chart / Axis / Series in Keynote, Numbers and Pages, which
//! share the TSCH chart format), read and written by its tschchart* name.
//!
//! A chart keeps its properties in style objects (shared with every chart of its theme
//! preset) and non-style objects (its own). A property is written where its type has it: on
//! the non-style when that's where it lives, else on the chart's own variation of its style,
//! made the first time (as Keynote does), so other charts of the same preset keep their look.

use kn_format::{KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::format::{color_msg, fill_msg, stroke_msg, FillSpec, StrokeSpec};
use crate::{Color, Document};

/// The part of a chart a property belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartPart {
    Chart,
    Legend,
    /// Y axis (0), the second Y axis of 2-axis charts (1); the X axis of scatter / bubble
    /// charts is value axis 1 as well.
    ValueAxis(usize),
    CategoryAxis,
    Series(usize),
}

/// A property value.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartValue {
    Bool(bool),
    Float(f32),
    Int(i64),
    Str(String),
    /// A number that may be unset (axis min / max): TSCH.ChartsNSNumberDoubleArchive.
    Number(f64),
    Fill(FillSpec),
    /// None: no line.
    Stroke(Option<StrokeSpec>),
    Color(Color),
}

/// The style and non-style object types of a part, and the unity fields that hold them.
fn kinds(part: ChartPart) -> (&'static str, &'static str) {
    match part {
        ChartPart::Chart => ("TSCH.ChartStyleArchive", "TSCH.ChartNonStyleArchive"),
        ChartPart::Legend => ("TSCH.LegendStyleArchive", "TSCH.LegendNonStyleArchive"),
        ChartPart::ValueAxis(_) | ChartPart::CategoryAxis => ("TSCH.ChartAxisStyleArchive", "TSCH.ChartAxisNonStyleArchive"),
        ChartPart::Series(_) => ("TSCH.ChartSeriesStyleArchive", "TSCH.ChartSeriesNonStyleArchive"),
    }
}

/// Whether objects of `ty` carry property `name` (in their `current`).
fn carries(ty: &str, name: &str) -> bool {
    let m = Msg::new(ty);
    m.field_def("current").and_then(|d| d.type_idx).is_some_and(|t| {
        let cur = Msg { ty: Some(t), fields: Vec::new() };
        cur.has_field(name)
    })
}

fn sparse(m: Option<&Msg>, i: usize) -> Option<ObjId> {
    m?.msgs("entries").find(|e| e.u32("index") == Some(i as u32))?.ref_id("reference")
}

/// (style, non-style) of a part as the chart has them now.
fn objects(unity: &Msg, part: ChartPart) -> (Option<ObjId>, Option<ObjId>) {
    match part {
        ChartPart::Chart => (unity.ref_id("chart_style"), unity.ref_id("chart_non_style")),
        ChartPart::Legend => (unity.ref_id("legend_style"), unity.ref_id("legend_non_style")),
        ChartPart::ValueAxis(i) => (unity.ref_ids("value_axis_styles").get(i).copied(), unity.ref_ids("value_axis_nonstyles").get(i).copied()),
        ChartPart::CategoryAxis => (unity.ref_ids("category_axis_styles").first().copied(), unity.ref_ids("category_axis_nonstyles").first().copied()),
        ChartPart::Series(i) => {
            let theme = unity.ref_ids("series_theme_styles");
            let style = sparse(unity.msg("series_private_styles"), i).or_else(|| (!theme.is_empty()).then(|| theme[i % theme.len()]));
            (style, sparse(unity.msg("series_non_styles"), i))
        }
    }
}

/// The value of property `name` on `cur` (a style's `current`), by the field's kind.
fn value_of(cur: &Msg, name: &str) -> Option<ChartValue> {
    if !cur.has(name) {
        return None;
    }
    let def = cur.field_def(name)?;
    let sub = def.type_idx.map(|t| kn_format::schema::schema().types[t as usize].name.as_str()).unwrap_or("");
    use kn_format::schema::Kind;
    Some(match def.kind {
        Kind::Bool => ChartValue::Bool(cur.bool(name)?),
        Kind::Float | Kind::Double => ChartValue::Float(cur.f32(name)?),
        Kind::String => ChartValue::Str(cur.str(name)?.to_string()),
        Kind::Message => {
            let m = cur.msg(name)?;
            match sub.trim_start_matches('.') {
                "TSCH.ChartsNSNumberDoubleArchive" => ChartValue::Number(m.f64("number_archive")?),
                "TSD.FillArchive" => ChartValue::Fill(fill_spec(&crate::style::fill(m))),
                "TSD.StrokeArchive" => ChartValue::Stroke(crate::style::stroke(m).map(|s| StrokeSpec { color: s.color, width: s.width, dash: dash_of(&s.dash) })),
                "TSP.Color" => ChartValue::Color(crate::style::color(m)),
                _ => return None,
            }
        }
        _ => ChartValue::Int(cur.i64(name)?),
    })
}

/// A stroke's dash pattern (in stroke widths) as the inspector's choice.
fn dash_of(d: &[f32]) -> crate::format::Dash {
    match d.first() {
        None => crate::format::Dash::Solid,
        Some(x) if *x < 1.5 => crate::format::Dash::Dotted,
        Some(_) => crate::format::Dash::Dashed,
    }
}

fn fill_spec(f: &crate::Fill) -> FillSpec {
    match f {
        crate::Fill::Color(c) => FillSpec::Color(*c),
        crate::Fill::Gradient { kind: crate::GradientKind::Linear { angle }, stops, .. } if stops.len() >= 2 => FillSpec::Gradient { from: stops[0].1, to: stops[stops.len() - 1].1, angle: *angle },
        _ => FillSpec::None,
    }
}

/// Write `v` into `cur` as property `name`; false if the kinds don't match.
fn put(cur: &mut Msg, name: &str, v: &ChartValue) -> bool {
    let Some(def) = cur.field_def(name) else { return false };
    use kn_format::schema::Kind;
    match (v, def.kind) {
        (ChartValue::Bool(b), Kind::Bool) => cur.set_bool(name, *b),
        (ChartValue::Float(f), Kind::Float | Kind::Double) => cur.set_f32(name, *f),
        (ChartValue::Int(i), Kind::Int32 | Kind::Int64 | Kind::UInt32 | Kind::UInt64 | Kind::SInt32 | Kind::SInt64 | Kind::Enum) => cur.set_i64(name, *i),
        (ChartValue::Str(s), Kind::String) => cur.set_str(name, s),
        (ChartValue::Number(n), Kind::Message) => {
            cur.remove(name);
            cur.msg_mut_or_create(name).set_f64("number_archive", *n);
        }
        (ChartValue::Fill(f), Kind::Message) => {
            cur.remove(name);
            cur.push_msg(name, fill_msg(f));
        }
        (ChartValue::Stroke(s), Kind::Message) => {
            cur.remove(name);
            cur.push_msg(name, stroke_msg(s.as_ref()));
        }
        (ChartValue::Color(c), Kind::Message) => {
            cur.remove(name);
            cur.push_msg(name, color_msg(*c));
        }
        _ => return false,
    }
    true
}

/// The Value Labels pop-up's index for a format (N29: 1 Same as Source Data, 2 Number,
/// 3 Currency, 4 Percentage, 5 Fraction, 6 Scientific).
fn format_type_index(f: &Msg) -> i64 {
    match f.u32("format_type") {
        Some(256) if f.u32("decimal_places").is_none_or(|d| d >= 200) && !f.bool("show_thousands_separator").unwrap_or(false) => 1,
        Some(256) => 2,
        Some(257) => 3,
        Some(258) => 4,
        Some(262) => 5,
        Some(259) => 6,
        _ => 1,
    }
}

/// How many charts point at style `id` (any of their style fields).
fn users(k: &KeyFile, id: ObjId) -> usize {
    k.objects_of("TSCH.ChartDrawableArchive")
        .filter(|(_, m)| {
            let mut refs = Vec::new();
            if let Some(u) = m.msg("unity") {
                u.collect_refs(&mut refs);
            }
            refs.contains(&id)
        })
        .count()
}

impl Document {
    /// Property `name` of a chart part, as drawn: the part's non-style, then its style and the
    /// styles it varies (None: not set anywhere, the app's default applies).
    pub fn chart_prop(&self, chart: ObjId, part: ChartPart, name: &str) -> Option<ChartValue> {
        let unity = self.file.obj(chart)?.msg("unity")?;
        let (style, non) = objects(unity, part);
        let mut chain: Vec<&Msg> = crate::style::chain(&self.file, non).into_iter().filter_map(|m| m.msg("current")).collect();
        chain.extend(crate::style::chain(&self.file, style).into_iter().filter_map(|m| m.msg("current")));
        if let ChartPart::Series(i) = part {
            // under a series' private style, its theme style
            let theme = unity.ref_ids("series_theme_styles");
            if !theme.is_empty() {
                chain.extend(crate::style::chain(&self.file, Some(theme[i % theme.len()])).into_iter().filter_map(|m| m.msg("current")));
            }
        }
        chain.into_iter().find_map(|c| value_of(c, name))
    }

    /// Set property `name` of a chart part (Format > Chart / Axis / Series). False when the
    /// part has no such property or the value is of the wrong kind.
    pub fn set_chart_prop(&mut self, h: &mut History, chart: ObjId, part: ChartPart, name: &str, value: &ChartValue) -> bool {
        let (style_ty, non_ty) = kinds(part);
        let target = if carries(non_ty, name) {
            self.chart_non_style(h, chart, part)
        } else if carries(style_ty, name) {
            self.chart_own_style(h, chart, part)
        } else {
            None
        };
        let Some(id) = target else { return false };
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return false };
        let ok = put(m.msg_mut_or_create("current"), name, value);
        kn_format::objects::fix_override_count(m);
        if let ChartValue::Fill(f) = value {
            crate::format::add_data_ref(&mut self.file, id, crate::format::fill_data(f));
        }
        self.file.refresh_refs(id);
        ok
    }

    /// The name of series `i`'s property `what` for the chart's type as it is now ("fill" ->
    /// tschchartseriescolumnfill on a column chart, tschchartseries3dpiefill on a 3D pie...);
    /// value-label properties ("showvaluelabels", "valuelabelposition"...) take the label stem.
    pub fn series_prop_name(&self, chart: ObjId, i: usize, what: &str) -> Option<String> {
        let c = crate::chart::chart(&self.file, self.file.obj(chart)?)?;
        let kind = c.series.get(i)?.kind;
        let stem = if what.contains("valuelabel") { crate::chart::label_stem(kind, &c.chart_type) } else { crate::chart::stem(kind, &c.chart_type) };
        Some(format!("tschchartseries{stem}{what}"))
    }

    /// Format > Series: one series' fill, for the chart's type.
    pub fn set_series_fill(&mut self, h: &mut History, chart: ObjId, i: usize, fill: &FillSpec) -> bool {
        let Some(name) = self.series_prop_name(chart, i, "fill") else { return false };
        self.set_chart_prop(h, chart, ChartPart::Series(i), &name, &ChartValue::Fill(fill.clone()))
    }

    /// Format > Series: one series' line / outline (None: none).
    pub fn set_series_stroke(&mut self, h: &mut History, chart: ObjId, i: usize, stroke: Option<&StrokeSpec>) -> bool {
        let Some(name) = self.series_prop_name(chart, i, "stroke") else { return false };
        self.set_chart_prop(h, chart, ChartPart::Series(i), &name, &ChartValue::Stroke(stroke.cloned()))
    }

    /// Format > Series > Value Labels > format: the number format of series `i`'s labels,
    /// written as Numbers does (N29): the format, its 1.0 copy and the pop-up's format type.
    pub fn set_series_number_format(&mut self, h: &mut History, chart: ObjId, i: usize, format: &Msg) -> bool {
        let pie = crate::chart::chart(&self.file, self.file.obj(chart).unwrap_or(&Msg::default())).is_some_and(|c| c.series.get(i).is_some_and(|s| s.kind == crate::SeriesKind::Pie));
        let (name, old, ty) = if pie {
            ("tschchartseriespienumberformat", "tschchartseriespie1_0numberformat", "tschchartseriespienumberformattype")
        } else {
            ("tschchartseriesdefaultnumberformat", "tschchartseriesdefault1_0numberformat", "tschchartseriesdefaultnumberformattype")
        };
        let Some(id) = self.chart_non_style(h, chart, ChartPart::Series(i)) else { return false };
        self.write_format(h, id, name, old, ty, format)
    }

    /// Format > Axis > number format of a value axis (its labels), as Numbers writes it.
    pub fn set_axis_number_format(&mut self, h: &mut History, chart: ObjId, part: ChartPart, format: &Msg) -> bool {
        let Some(id) = self.chart_non_style(h, chart, part) else { return false };
        self.write_format(h, id, "tschchartaxisdefaultnumberformat", "tschchartaxisdefault1_0numberformat", "tschchartaxisdefaultnumberformattype", format)
    }

    fn write_format(&mut self, h: &mut History, id: ObjId, name: &str, old: &str, ty: &str, format: &Msg) -> bool {
        self.touch(h, id);
        let Some(cur) = self.file.obj_mut(id).map(|m| m.msg_mut_or_create("current")) else { return false };
        for n in [name, old] {
            cur.remove(n);
            cur.push_msg(n, format.clone());
        }
        cur.set_i64(ty, format_type_index(format));
        true
    }

    /// Format > Chart > background: the plot area's fill (gridbackgroundfill, with an empty
    /// backgroundfill, as Numbers writes it; N29).
    pub fn set_chart_background(&mut self, h: &mut History, chart: ObjId, fill: &FillSpec) -> bool {
        let ok = self.set_chart_prop(h, chart, ChartPart::Chart, "tschchartinfodefaultgridbackgroundfill", &ChartValue::Fill(fill.clone()));
        if ok {
            if let Some(id) = self.file.obj(chart).and_then(|m| m.msg("unity")).and_then(|u| u.ref_id("chart_style")) {
                if let Some(cur) = self.file.obj_mut(id).map(|m| m.msg_mut_or_create("current")) {
                    cur.remove("tschchartinfodefaultbackgroundfill");
                    cur.push_msg("tschchartinfodefaultbackgroundfill", Msg::new("TSD.FillArchive"));
                }
            }
        }
        ok
    }

    /// The theme's chart style presets in order (the Chart pane's swatches).
    pub fn chart_presets(&self) -> Vec<ObjId> {
        // the theme's list (TSCH.ChartPresetsArchive.chart_presets) when it can be read, else
        // every preset in the order they were made
        let listed: Vec<ObjId> = self.file.objects_of("TSCH.ChartPresetsArchive").flat_map(|(_, m)| m.ref_ids("chart_presets")).collect();
        if !listed.is_empty() {
            return listed;
        }
        let mut v: Vec<ObjId> = self.file.objects_of("TSCH.ChartStylePreset").map(|(id, _)| id).collect();
        v.sort_unstable();
        v
    }

    /// A preset's series colours (its swatch): each series style's default / column fill.
    pub fn chart_preset_colors(&self, preset: ObjId) -> Vec<Color> {
        let Some(p) = self.file.obj(preset) else { return Vec::new() };
        p.ref_ids("series_styles")
            .into_iter()
            .filter_map(|id| {
                let cur = self.file.obj(id)?.msg("current")?;
                ["tschchartseriescolumnfill", "tschchartseriesdefaultfill"].iter().find_map(|n| cur.msg(n).map(crate::style::fill)).and_then(|f| match f {
                    crate::Fill::Color(c) => Some(c),
                    crate::Fill::Gradient { stops, .. } => stops.first().map(|s| s.1),
                    _ => None,
                })
            })
            .collect()
    }

    /// The preset a chart uses now.
    pub fn chart_preset(&self, chart: ObjId) -> Option<ObjId> {
        self.file.obj(chart)?.msg("unity")?.ref_id("preset")
    }

    /// Format > Chart > Chart Styles: the chart takes `preset`'s styles (its own changes to
    /// styles go, as in Keynote and Numbers; its data and non-style options stay).
    pub fn apply_chart_preset(&mut self, h: &mut History, chart: ObjId, preset: ObjId) -> bool {
        let Some(p) = self.file.obj(preset).cloned() else { return false };
        self.touch(h, chart);
        let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) else { return false };
        u.set_ref("preset", preset);
        for (from, to) in [("chart_style", "chart_style"), ("legend_style", "legend_style")] {
            if let Some(r) = p.ref_id(from) {
                u.set_ref(to, r);
            }
        }
        for (from, to) in [("value_axis_styles", "value_axis_styles"), ("category_axis_styles", "category_axis_styles"), ("series_styles", "series_theme_styles"), ("paragraph_styles", "paragraph_styles")] {
            let ids = p.ref_ids(from);
            if !ids.is_empty() {
                u.remove(to);
                for id in ids {
                    u.push_ref(to, id);
                }
            }
        }
        u.remove("series_private_styles");
        self.file.refresh_refs(chart);
        if let Some(f) = self.file.file_of(chart).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }

    /// Format > Chart > Chart Font: every text of the chart in `family` (None: as it is), its
    /// sizes times `scale` (the big / small A buttons step by 1.11, as Numbers' +2 steps gave
    /// 12 -> 14.8). Each paragraph style becomes the chart's own variation.
    pub fn set_chart_font(&mut self, h: &mut History, chart: ObjId, family: Option<&str>, scale: f32) -> bool {
        let Some(styles) = self.file.obj(chart).and_then(|m| m.msg("unity")).map(|u| u.ref_ids("paragraph_styles")) else { return false };
        let mut out = Vec::with_capacity(styles.len());
        for base in styles {
            let size = crate::style::char_style(&self.file, Some(base), None).size;
            let shared = self.file.objects_of("TSCH.ChartDrawableArchive").filter(|(_, m)| m.msg("unity").is_some_and(|u| u.ref_ids("paragraph_styles").contains(&base))).count() > 1;
            let own = if shared || self.file.obj(base).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.bool("is_variation")) != Some(true) {
                self.variation_of(h, base)
            } else {
                Some(base)
            };
            let Some(v) = own else { return false };
            self.touch(h, v);
            if let Some(m) = self.file.obj_mut(v) {
                if let Some(f) = family {
                    crate::format::apply_text_prop(m, &crate::format::TextProp::Font(f.to_string()));
                }
                if (scale - 1.0).abs() > 1e-3 && size > 0.0 {
                    crate::format::apply_text_prop(m, &crate::format::TextProp::Size((size * scale * 10.0).round() / 10.0));
                }
                kn_format::objects::fix_override_count(m);
            }
            out.push(v);
        }
        self.touch(h, chart);
        if let Some(u) = self.file.obj_mut(chart).and_then(|m| m.msg_mut("unity")) {
            u.remove("paragraph_styles");
            for id in out {
                u.push_ref("paragraph_styles", id);
            }
        }
        self.file.refresh_refs(chart);
        if let Some(f) = self.file.file_of(chart).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        true
    }

    /// The part's non-style object (series non-styles are made when missing).
    fn chart_non_style(&mut self, h: &mut History, chart: ObjId, part: ChartPart) -> Option<ObjId> {
        if let ChartPart::Series(i) = part {
            self.ensure_series_non_styles(h, chart);
            return sparse(self.file.obj(chart)?.msg("unity")?.msg("series_non_styles"), i);
        }
        objects(self.file.obj(chart)?.msg("unity")?, part).1
    }

    /// The part's style, made this chart's own first: a variation of the shared one (pointed at
    /// from the chart), or for a series a private style varying its theme style.
    fn chart_own_style(&mut self, h: &mut History, chart: ObjId, part: ChartPart) -> Option<ObjId> {
        let unity = self.file.obj(chart)?.msg("unity")?.clone();
        let (style, _) = objects(&unity, part);
        let base = style?;
        let private_series = matches!(part, ChartPart::Series(i) if sparse(unity.msg("series_private_styles"), i).is_some());
        let is_variation = |k: &KeyFile| k.obj(base).and_then(|m| m.find_super("TSS.StyleArchive")).and_then(|s| s.bool("is_variation")) == Some(true);
        // already this chart's own: write to it
        let own = match part {
            ChartPart::Series(_) => private_series && users(&self.file, base) <= 1,
            _ => is_variation(&self.file) && users(&self.file, base) <= 1,
        };
        if own || h.created_now(base) {
            return Some(base);
        }
        let v = self.variation_of(h, base)?;
        self.touch(h, chart);
        let u = self.file.obj_mut(chart)?.msg_mut("unity")?;
        match part {
            ChartPart::Chart => u.set_ref("chart_style", v),
            ChartPart::Legend => u.set_ref("legend_style", v),
            ChartPart::ValueAxis(i) => {
                let mut ids = u.ref_ids("value_axis_styles");
                *ids.get_mut(i)? = v;
                u.remove("value_axis_styles");
                for id in ids {
                    u.push_ref("value_axis_styles", id);
                }
            }
            ChartPart::CategoryAxis => {
                let mut ids = u.ref_ids("category_axis_styles");
                *ids.first_mut()? = v;
                u.remove("category_axis_styles");
                for id in ids {
                    u.push_ref("category_axis_styles", id);
                }
            }
            ChartPart::Series(i) => {
                // a private style for series i (the sparse array of private styles)
                let n = crate::chart::chart(&self.file, self.file.obj(chart)?).map(|c| c.series.len()).unwrap_or(i + 1);
                let u = self.file.obj_mut(chart)?.msg_mut("unity")?;
                let arr = u.msg_mut_or_create("series_private_styles");
                arr.retain_msgs("entries", |e| e.u32("index") != Some(i as u32));
                let mut e = Msg::new("TSP.SparseReferenceArray.Entry");
                e.set_u64("index", i as u64);
                e.set_ref("reference", v);
                arr.push_msg("entries", e);
                let count = arr.u64("count").unwrap_or(0).max(n as u64);
                arr.set_u64("count", count);
            }
        }
        self.file.refresh_refs(chart);
        if let Some(f) = self.file.file_of(chart).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insert::NewObject;

    fn two_charts() -> (Document, ObjId, ObjId) {
        let mut d = Document::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/ref/R10-standard.key")).unwrap();
        let s = d.slides()[0].slide;
        let mut h = History::default();
        d.begin(&mut h, "c");
        let a = d.insert(&mut h, s, &NewObject::Chart { pie: false }).unwrap();
        let b = d.insert(&mut h, s, &NewObject::Chart { pie: false }).unwrap();
        d.end(&mut h);
        (d, a, b)
    }

    /// A style property changes on its own chart only, survives a save, and reads back.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn style_props_stay_on_their_chart() {
        let (mut d, a, b) = two_charts();
        let mut h = History::default();
        d.begin(&mut h, "Style");
        let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
        assert!(d.set_chart_prop(&mut h, a, ChartPart::Chart, "tschchartinfodefaultbackgroundfill", &ChartValue::Fill(FillSpec::Color(red))));
        assert!(d.set_chart_prop(&mut h, a, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius", &ChartValue::Float(10.0)));
        assert!(d.set_chart_prop(&mut h, a, ChartPart::ValueAxis(0), "tschchartaxisdefaultusermax", &ChartValue::Number(100.0)));
        assert!(d.set_series_fill(&mut h, a, 0, &FillSpec::Color(red)));
        let fill_name = d.series_prop_name(a, 0, "fill").unwrap();
        assert!(!d.set_chart_prop(&mut h, a, ChartPart::Chart, "tschchartnosuchthing", &ChartValue::Bool(true)));
        d.end(&mut h);
        let d = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d.chart_prop(a, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius"), Some(ChartValue::Float(10.0)));
        assert_ne!(d.chart_prop(b, ChartPart::Chart, "tschchartinfodefaultroundedcornerradius"), Some(ChartValue::Float(10.0)));
        assert_eq!(d.chart_prop(a, ChartPart::ValueAxis(0), "tschchartaxisdefaultusermax"), Some(ChartValue::Number(100.0)));
        assert_eq!(d.chart_prop(b, ChartPart::ValueAxis(0), "tschchartaxisdefaultusermax"), None);
        assert_eq!(d.chart_prop(a, ChartPart::Series(0), &fill_name), Some(ChartValue::Fill(FillSpec::Color(red))));
        assert_ne!(d.chart_prop(a, ChartPart::Series(1), &fill_name), Some(ChartValue::Fill(FillSpec::Color(red))));
        assert_ne!(d.chart_prop(b, ChartPart::Series(0), &fill_name), Some(ChartValue::Fill(FillSpec::Color(red))));
        // and the scene draws it
        let sc = d.scene(&d.slides(), 0).unwrap();
        let c = sc.drawables.iter().find_map(|x| match &x.kind {
            crate::DrawableKind::Chart(c) if x.id == a => Some(c.clone()),
            _ => None,
        });
        assert_eq!(c.unwrap().series[0].fill, crate::Fill::Color(red));
    }

    /// Show Border and the axis gridlines toggle on one chart only (they used to change the
    /// shared preset style, or nothing for gridlines).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn flags_stay_on_their_chart() {
        use crate::chartdata::ChartFlag;
        let (mut d, a, b) = two_charts();
        let mut h = History::default();
        d.begin(&mut h, "Flags");
        assert!(d.set_chart_flag(&mut h, a, ChartFlag::Border(true)));
        assert!(d.set_chart_flag(&mut h, a, ChartFlag::Axis(Some(0), "showmajorgridlines", false)));
        d.end(&mut h);
        let get = |d: &Document, id| {
            let m = d.file.obj(id).unwrap();
            crate::chart::chart(&d.file, m).unwrap()
        };
        let (ca, cb) = (get(&d, a), get(&d, b));
        assert!(ca.show_border && !cb.show_border);
        assert!(!ca.value_axes[0].show_gridlines && cb.value_axes[0].show_gridlines);
    }

    /// Presets, chart font, background and label formats as Numbers writes them (N29).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn presets_font_background_formats() {
        let (mut d, a, b) = two_charts();
        let presets = d.chart_presets();
        assert!(presets.len() >= 3, "{presets:?}");
        assert!(!d.chart_preset_colors(presets[2]).is_empty());
        let mut h = History::default();
        d.begin(&mut h, "Style");
        assert!(d.apply_chart_preset(&mut h, a, presets[2]));
        assert!(d.set_chart_font(&mut h, a, Some("Avenir-Book"), 1.11 * 1.11));
        let grey = Color::rgba(0.84, 0.84, 0.84, 1.0);
        assert!(d.set_chart_background(&mut h, a, &FillSpec::Color(grey)));
        let two = crate::tablefmt::CellFormat::Number { decimals: Some(2), thousands: true, negative: 0 }.msg();
        assert!(d.set_series_number_format(&mut h, a, 0, &two));
        d.end(&mut h);
        let d = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(d.chart_preset(a), Some(presets[2]));
        assert_ne!(d.chart_preset(b), Some(presets[2]));
        let get = |id| crate::chart::chart(&d.file, d.file.obj(id).unwrap()).unwrap();
        let (ca, cb) = (get(a), get(b));
        assert_eq!(ca.background, Some(crate::Fill::Color(grey)));
        assert_eq!(cb.background, None);
        assert_eq!(ca.value_label_style.font_name, "Avenir-Book");
        assert_ne!(cb.value_label_style.font_name, "Avenir-Book");
        assert!(ca.value_label_style.size > cb.value_label_style.size * 1.2);
        assert_eq!(ca.series[0].value_format.as_ref().and_then(|f| f.u32("decimal_places")), Some(2));
        let non = d.file.obj(a).unwrap().msg("unity").unwrap().msg("series_non_styles").unwrap().msgs("entries").next().unwrap().ref_id("reference").unwrap();
        let cur = d.file.obj(non).unwrap().msg("current").unwrap();
        assert_eq!(cur.i64("tschchartseriesdefaultnumberformattype"), Some(2));
        assert!(cur.msg("tschchartseriesdefault1_0numberformat").is_some());
    }
}
