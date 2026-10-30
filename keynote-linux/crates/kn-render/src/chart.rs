//! Chart drawing from the resolved [`ChartData`]: column/bar (clustered, stacked), line, area,
//! mixed and 2-axis, interactive (one data set), pie/donut, scatter, bubble and radar charts.
//! 3D variants get a simple oblique extrusion (box bars, ribbon lines, slab areas, thick pies).
//!
//! Like Keynote, the chart frame is the plot area: axis labels sit outside it.

use std::f32::consts::{FRAC_PI_2, TAU};

use kn_model::*;
use tiny_skia as sk;

use crate::paint::{frame_transform, rect_path, sk_stroke, solid};
use crate::{simple_text_box, text, Renderer};

/// Default series colours of the White theme (used when a chart carries no series styles).
pub const PALETTE: [Color; 6] = [
    Color { r: 0.0, g: 0.63, b: 1.0, a: 1.0 },
    Color { r: 0.38, g: 0.85, b: 0.21, a: 1.0 },
    Color { r: 0.97, g: 0.73, b: 0.0, a: 1.0 },
    Color { r: 1.0, g: 0.15, b: 0.0, a: 1.0 },
    Color { r: 0.76, g: 0.28, b: 0.52, a: 1.0 },
    Color { r: 0.37, g: 0.37, b: 0.37, a: 1.0 },
];

const AXIS_LINE: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.85 };
const GRID_LINE: Color = Color { r: 0.72, g: 0.72, b: 0.72, a: 1.0 };

/// Drawing target: renderer (fonts), pixmap and the chart frame's transform.
struct Cx<'a> {
    r: &'a mut Renderer,
    pm: &'a mut sk::Pixmap,
    t: sk::Transform,
}

impl Cx<'_> {
    fn fill(&mut self, p: Option<sk::Path>, c: Color) {
        if let Some(p) = p {
            self.pm.fill_path(&p, &solid(c), sk::FillRule::Winding, self.t, None);
        }
    }

    fn stroke(&mut self, p: Option<sk::Path>, c: Color, width: f32) {
        if let Some(p) = p {
            let st = sk::Stroke { width, line_join: sk::LineJoin::Round, line_cap: sk::LineCap::Round, ..Default::default() };
            self.pm.stroke_path(&p, &solid(c), &st, self.t, None);
        }
    }

    fn stroke_with(&mut self, p: Option<sk::Path>, s: &Stroke) {
        if let Some(p) = p {
            self.pm.stroke_path(&p, &solid(s.color), &sk_stroke(s), self.t, None);
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, s: &Stroke) {
        let mut b = sk::PathBuilder::new();
        b.move_to(x0, y0);
        b.line_to(x1, y1);
        self.stroke_with(b.finish(), s);
    }

    /// One line of text in the box (x, y, w, h).
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, s: &str, style: &CharStyle, x: f32, y: f32, w: f32, h: f32, align: Align, valign: VAlign) {
        if s.is_empty() {
            return;
        }
        let mut tb = simple_text_box(s, style.clone(), align, valign);
        tb.padding = [0.0; 4];
        let tl = text::layout(&mut self.r.fonts, &tb, w.max(1.0), h.max(1.0), false, false);
        self.r.draw_text(&tl, self.pm, self.t.pre_translate(x, y));
    }

    /// One line of text ending at (x, y) (its right end there, centred vertically), turned
    /// `deg` degrees counter-clockwise about that point: angled axis labels.
    fn text_turned(&mut self, s: &str, style: &CharStyle, x: f32, y: f32, deg: f32) {
        if s.is_empty() {
            return;
        }
        let h = style.size * 1.6;
        let mut tb = simple_text_box(s, style.clone(), Align::Right, VAlign::Middle);
        tb.padding = [0.0; 4];
        let tl = text::layout(&mut self.r.fonts, &tb, 400.0, h, false, false);
        let t = self.t.pre_translate(x, y).pre_concat(sk::Transform::from_rotate(-deg)).pre_translate(-400.0, -h / 2.0);
        self.r.draw_text(&tl, self.pm, t);
    }

    /// Text centred on (x, y), at most `w` wide.
    fn text_at(&mut self, s: &str, style: &CharStyle, x: f32, y: f32, w: f32) {
        let h = style.size * 1.6;
        self.text(s, style, x - w / 2.0, y - h / 2.0, w, h, Align::Center, VAlign::Middle);
    }
}

fn poly(pts: &[(f32, f32)], close: bool) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        if i == 0 {
            b.move_to(*x, *y);
        } else {
            b.line_to(*x, *y);
        }
    }
    if close {
        b.close();
    }
    b.finish()
}

fn shade(c: Color, f: f32) -> Color {
    if f >= 1.0 {
        let k = f - 1.0;
        Color::rgba(c.r + (1.0 - c.r) * k, c.g + (1.0 - c.g) * k, c.b + (1.0 - c.b) * k, c.a)
    } else {
        Color::rgba(c.r * f, c.g * f, c.b * f, c.a)
    }
}

fn plain_stroke(c: Color, width: f32) -> Stroke {
    Stroke {
        color: c,
        width,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        miter_limit: 4.0,
        dash: Vec::new(),
        phase: 0.0,
        brush: None,
    }
}

// ---------------------------------------------------------------------------------- scales

/// A value axis range split into `n` equal intervals.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Scale {
    lo: f64,
    hi: f64,
    n: u32,
    /// Logarithmic: `lo` and `hi` are powers of ten, one interval per decade.
    log: bool,
}

impl Scale {
    fn frac(&self, v: f64) -> f32 {
        if self.log {
            let (a, b) = (self.lo.log10(), self.hi.log10());
            return if b > a && v > 0.0 { ((v.log10() - a) / (b - a)) as f32 } else { 0.0 };
        }
        if self.hi > self.lo { ((v - self.lo) / (self.hi - self.lo)) as f32 } else { 0.0 }
    }

    fn ticks(&self) -> impl Iterator<Item = f64> + '_ {
        (0..=self.n).map(move |i| if self.log { self.lo * 10f64.powi(i as i32) } else { self.lo + (self.hi - self.lo) * f64::from(i) / f64::from(self.n.max(1)) })
    }

    /// Minor tick values between the major ones: `k` intervals in each.
    fn minor_ticks(&self, k: u32) -> Vec<f64> {
        let ticks: Vec<f64> = self.ticks().collect();
        let mut out = Vec::new();
        for w in ticks.windows(2) {
            for j in 1..k.max(1) {
                let f = f64::from(j) / f64::from(k);
                out.push(if self.log { w[0] * (1.0 + 9.0 * f) } else { w[0] + (w[1] - w[0]) * f });
            }
        }
        out
    }
}

/// Smallest "nice" step >= raw. Category value axes use 1-2-2.5-5 steps; scatter, bubble and
/// second axes also allow 1.5, 1.75, 3, 4 and 7.5 (as Keynote's own output does).
fn nice_step(raw: f64, fine: bool) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let coarse: &[f64] = &[1.0, 2.0, 2.5, 5.0, 7.5, 10.0];
    let finer: &[f64] = &[1.0, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0, 7.5, 10.0];
    let m = (if fine { finer } else { coarse }).iter().find(|m| raw <= *m * mag * (1.0 + 1e-9)).copied().unwrap_or(10.0);
    m * mag
}

/// Axis range for data in [min, max]: user min/max when set, else zero-based with 4 intervals.
fn scale_for(min: f64, max: f64, axis: &ChartAxis, fine: bool) -> Scale {
    if axis.log {
        // whole decades round the data (positive values only)
        let lo = axis.min.filter(|v| *v > 0.0).unwrap_or_else(|| 10f64.powf(min.max(1e-9).log10().floor()));
        let hi = axis.max.filter(|v| *v > lo).unwrap_or_else(|| 10f64.powf(max.max(lo * 10.0).log10().ceil()));
        let n = ((hi / lo).log10().round() as u32).clamp(1, 20);
        return Scale { lo, hi, n, log: true };
    }
    let n = axis.intervals.unwrap_or(4).clamp(1, 20);
    let (min, max) = (if min.is_finite() { min } else { 0.0 }, if max.is_finite() { max } else { 1.0 });
    let lo0 = axis.min.unwrap_or(min.min(0.0));
    if let (Some(lo), Some(hi)) = (axis.min, axis.max) {
        return Scale { lo, hi, n, log: false };
    }
    let step = nice_step((max.max(lo0 + 1e-9) - lo0) / f64::from(n), fine);
    let lo = axis.min.unwrap_or((lo0 / step).floor() * step);
    let hi = axis.max.unwrap_or_else(|| {
        let n_up = ((max - lo) / step).ceil().max(1.0);
        lo + step * n_up.max(f64::from(n))
    });
    let n = (((hi - lo) / step).round() as u32).clamp(1, 20);
    Scale { lo, hi, n, log: false }
}

/// An axis or value label: in its number format when it has one.
fn fmt_with(v: f64, f: Option<&kn_format::Msg>) -> String {
    match f {
        Some(f) => kn_model::table::format_value(&kn_model::table::CellValue::Number(v), Some(f)),
        None => fmt_value(v),
    }
}

fn fmt_value(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ---------------------------------------------------------------------------------- entry

pub fn draw_chart(r: &mut Renderer, d: &Drawable, c: &ChartData, pm: &mut sk::Pixmap, base: sk::Transform) {
    let g = d.geom;
    let mut cx = Cx { r, pm, t: base.pre_concat(frame_transform(&g, true)) };
    let series = displayed_series(c);
    if series.is_empty() || g.w <= 0.0 || g.h <= 0.0 {
        return;
    }
    // Format > Chart > background, under everything
    if let Some(f) = &c.background {
        if let (Some(paint), Some(p)) = (crate::paint::fill_paint(f, g.w, g.h), rect_path(0.0, 0.0, g.w, g.h)) {
            cx.pm.fill_path(&p, &paint, sk::FillRule::Winding, cx.t, None);
        }
    }
    match series[0].kind {
        SeriesKind::Pie => draw_pie(&mut cx, c, &series, g.w, g.h),
        SeriesKind::Radar => draw_radar(&mut cx, c, &series, g.w, g.h),
        SeriesKind::Scatter | SeriesKind::Bubble => draw_xy(&mut cx, c, &series, g.w, g.h),
        _ if c.data_set.is_some() => draw_data_set(&mut cx, c, &series, g.w, g.h),
        _ => draw_category(&mut cx, c, &series, g.w, g.h),
    }
    if c.show_border {
        let mut b = sk::PathBuilder::new();
        if let Some(r) = sk::Rect::from_xywh(0.0, 0.0, g.w, g.h) {
            b.push_rect(r);
        }
        match &c.border {
            Some(s) => cx.stroke_with(b.finish(), s),
            None => cx.stroke(b.finish(), Color { r: 0.6, g: 0.6, b: 0.6, a: 1.0 }, 1.0),
        }
    }
    if c.show_legend {
        draw_legend(&mut cx, c, &series, g.w, g.h);
    }
    if let Some(t) = &c.title {
        // above the plot (and above a legend in its default place)
        let st = &c.title_style;
        let lh = st.size * 1.6;
        let legend_default = c.show_legend && c.legend_frame.is_none_or(|f| f[2] <= 0.0);
        let y = if legend_default { -lh - c.legend_style.size * 1.6 * 1.5 } else { -lh * 1.3 };
        cx.text(t, st, 0.0, y, g.w, lh, Align::Center, VAlign::Middle);
    }
}

/// Series to draw; charts built without resolved series fall back to rows of the grid.
fn displayed_series(c: &ChartData) -> Vec<ChartSeries> {
    if !c.series.is_empty() {
        let mut s = c.series.clone();
        if let Some(i) = c.data_set {
            // interactive charts show one category (data set) at a time
            for x in &mut s {
                let pick = |v: &Vec<f64>| v.get(i as usize).copied().into_iter().collect::<Vec<_>>();
                x.values = pick(&x.values);
                if !x.xs.is_empty() {
                    x.xs = pick(&x.xs);
                }
                if !x.sizes.is_empty() {
                    x.sizes = pick(&x.sizes);
                }
            }
        }
        return s;
    }
    let ty = c.chart_type.to_ascii_lowercase();
    let kind = if ty.contains("pie") || ty.contains("donut") {
        SeriesKind::Pie
    } else if ty.contains("bar") {
        SeriesKind::Bar
    } else if ty.contains("line") {
        SeriesKind::Line
    } else if ty.contains("area") {
        SeriesKind::Area
    } else {
        SeriesKind::Column
    };
    c.values
        .iter()
        .enumerate()
        .map(|(i, v)| ChartSeries {
            name: c.row_names.get(i).cloned().unwrap_or_default(),
            values: v.clone(),
            xs: Vec::new(),
            sizes: Vec::new(),
            kind,
            fill: Fill::Color(PALETTE[i % PALETTE.len()]),
            color: PALETTE[i % PALETTE.len()],
            stroke: None,
            y2: false,
            show_value_labels: kind == SeriesKind::Pie,
            show_series_names: false,
            show_symbols: false,
            value_format: None,
            explosion: 0.0,
            label_distance: None,
            label_position: 0,
        })
        .collect()
}

fn line_width(s: &ChartSeries) -> f32 {
    s.stroke.as_ref().map(|st| st.width).unwrap_or(3.0).clamp(1.0, 12.0)
}

// ---------------------------------------------------------------------------------- category charts

/// Oblique extrusion offset for 3D charts.
#[derive(Clone, Copy)]
struct Depth {
    dx: f32,
    dy: f32,
}

/// Value gridlines, axis lines and labels around a plot of size w x h.
#[allow(clippy::too_many_arguments)]
fn draw_value_axis(cx: &mut Cx, axis: &ChartAxis, sc: &Scale, w: f32, h: f32, horizontal: bool, right: bool, depth: Option<Depth>) {
    let (dx, dy) = depth.map(|d| (d.dx, d.dy)).unwrap_or((0.0, 0.0));
    let grid = axis.gridline.clone().unwrap_or_else(|| plain_stroke(GRID_LINE, 1.0));
    let gap = axis.label_style.size * 0.4;
    let tick = 4.0;
    let axis_stroke = plain_stroke(AXIS_LINE, 1.0);
    // minor gridlines and tick marks between the major ones
    if axis.show_minor_gridlines || axis.show_minor_ticks {
        let minor = axis.minor_gridline.clone().unwrap_or_else(|| plain_stroke(GRID_LINE.with_alpha(0.5), 0.5));
        for v in sc.minor_ticks(axis.minor_intervals.unwrap_or(if sc.log { 9 } else { 2 })) {
            let f = sc.frac(v);
            if horizontal {
                let x = w * f;
                if axis.show_minor_gridlines {
                    cx.line(x + dx, dy, x + dx, h + dy, &minor);
                }
                if axis.show_minor_ticks {
                    cx.line(x, h, x, h + tick / 2.0, &axis_stroke);
                }
            } else {
                let y = h * (1.0 - f);
                if axis.show_minor_gridlines {
                    cx.line(dx, y + dy, w + dx, y + dy, &minor);
                }
                if axis.show_minor_ticks {
                    let x0 = if right { w } else { 0.0 };
                    cx.line(x0, y, x0 + if right { tick / 2.0 } else { -tick / 2.0 }, y, &axis_stroke);
                }
            }
        }
    }
    for v in sc.ticks() {
        let f = sc.frac(v);
        if horizontal {
            let x = w * f;
            if axis.show_gridlines && (f > 0.0 || depth.is_some()) {
                cx.line(x + dx, dy, x + dx, h + dy, &grid);
            }
            if axis.show_major_ticks {
                cx.line(x, h, x, h + tick, &axis_stroke);
            }
            if axis.show_labels {
                let txt = fmt_with(v, axis.number_format.as_ref());
                if axis.label_angle.abs() > 0.5 {
                    cx.text_turned(&txt, &axis.label_style, x + dx, h + gap, axis.label_angle);
                } else {
                    cx.text(&txt, &axis.label_style, x - 200.0 + dx, h + gap, 400.0, axis.label_style.size * 1.4, Align::Center, VAlign::Top);
                }
            }
        } else {
            let y = h * (1.0 - f);
            if axis.show_gridlines && (f > 0.0 || depth.is_some()) {
                cx.line(dx, y + dy, w + dx, y + dy, &grid);
            }
            if axis.show_major_ticks {
                let x0 = if right { w } else { 0.0 };
                cx.line(x0, y, x0 + if right { tick } else { -tick }, y, &axis_stroke);
            }
            if axis.show_labels {
                let lh = axis.label_style.size * 1.6;
                let txt = fmt_with(v, axis.number_format.as_ref());
                if right || depth.is_some() {
                    cx.text(&txt, &axis.label_style, w + dx + gap, y + dy - lh / 2.0, 400.0, lh, Align::Left, VAlign::Middle);
                } else if axis.label_angle.abs() > 0.5 {
                    cx.text_turned(&txt, &axis.label_style, -gap, y, axis.label_angle);
                } else {
                    cx.text(&txt, &axis.label_style, -400.0 - gap, y - lh / 2.0, 400.0, lh, Align::Right, VAlign::Middle);
                }
            }
        }
    }
    if axis.show_line && depth.is_none() && !right {
        let s = plain_stroke(AXIS_LINE, 1.0);
        if horizontal {
            cx.line(0.0, h, w, h, &s);
        } else {
            cx.line(0.0, 0.0, 0.0, h, &s);
        }
    }
}

/// Category labels along the category axis: `at[i]` is the label centre along the axis.
fn draw_category_labels(cx: &mut Cx, c: &ChartData, names: &[String], at: &[f32], slot: f32, w: f32, h: f32, horizontal: bool) {
    let axis = &c.category_axis;
    if !axis.show_labels {
        return;
    }
    let st = &axis.label_style;
    let gap = st.size * 0.4;
    for (name, p) in names.iter().zip(at) {
        if horizontal {
            let lh = st.size * 1.6;
            cx.text(name, st, -400.0 - gap, p - lh / 2.0, 400.0, lh, Align::Right, VAlign::Middle);
        } else if axis.label_angle.abs() > 0.5 {
            // angled labels end under their category
            cx.text_turned(name, st, *p, h + gap + st.size * 0.6, axis.label_angle);
        } else {
            let lw = slot.max(st.size * 6.0);
            cx.text(name, st, p - lw / 2.0, h + gap, lw, st.size * 1.4, Align::Center, VAlign::Top);
        }
    }
    let _ = w;
}

fn draw_category_axis_line(cx: &mut Cx, c: &ChartData, w: f32, h: f32, horizontal: bool, base: f32) {
    if !c.category_axis.show_line {
        return;
    }
    let s = plain_stroke(AXIS_LINE, 1.0);
    if horizontal {
        cx.line(base, 0.0, base, h, &s);
    } else {
        cx.line(0.0, base, w, base, &s);
    }
}

fn is_stacked(c: &ChartData) -> bool {
    c.chart_type.to_ascii_lowercase().contains("stacked")
}

/// Value range of a set of series (stacked sums when stacking).
fn value_range(series: &[&ChartSeries], ncat: usize, stacked: bool) -> (f64, f64) {
    if stacked {
        let (mut lo, mut hi) = (0f64, 0f64);
        for i in 0..ncat {
            let (mut pos, mut neg) = (0.0, 0.0);
            for s in series {
                let v = s.values.get(i).copied().unwrap_or(0.0);
                if v.is_finite() {
                    if v >= 0.0 { pos += v } else { neg += v }
                }
            }
            hi = hi.max(pos);
            lo = lo.min(neg);
        }
        (lo, hi)
    } else {
        let vals = series.iter().flat_map(|s| s.values.iter().copied()).filter(|v| v.is_finite());
        vals.fold((0f64, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    }
}

/// Column, bar, line, area, mixed and 2-axis charts (2D and 3D).
fn draw_category(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w0: f32, h0: f32) {
    let horizontal = series.iter().all(|s| s.kind == SeriesKind::Bar);
    let stacked = is_stacked(c);
    let ncat = series.iter().map(|s| s.values.len()).max().unwrap_or(0).max(c.categories.len()).max(1);
    let three_d = c.is_3d();
    // 3D: shrink the front plane so the extrusion stays inside the frame
    let (w, h, depth) = if three_d {
        let d = (if horizontal { h0 } else { w0 } / ncat as f32 * 0.35).min(w0.min(h0) * 0.12);
        let dp = Depth { dx: d * 0.9, dy: -d * 0.6 };
        cx.t = cx.t.pre_translate(0.0, -dp.dy);
        (w0 - dp.dx, h0 + dp.dy, Some(dp))
    } else {
        (w0, h0, None)
    };
    let main: Vec<&ChartSeries> = series.iter().filter(|s| !s.y2).collect();
    let second: Vec<&ChartSeries> = series.iter().filter(|s| s.y2).collect();
    let (lo, hi) = value_range(&main, ncat, stacked);
    let axis0 = c.value_axes.first().cloned().unwrap_or_default();
    let sc = scale_for(lo, hi, &axis0, false);
    let sc2 = (!second.is_empty()).then(|| {
        let (lo, hi) = value_range(&second, ncat, stacked);
        scale_for(lo, hi, c.value_axes.get(1).unwrap_or(&axis0), true)
    });

    draw_value_axis(cx, &axis0, &sc, w, h, horizontal, false, depth);
    if let (Some(sc2), Some(ax2)) = (sc2, c.value_axes.get(1)) {
        let mut ax2 = ax2.clone();
        ax2.show_gridlines = false;
        draw_value_axis(cx, &ax2, &sc2, w, h, horizontal, true, depth);
    }
    if let Some(dp) = depth {
        draw_floor(cx, w, h, horizontal, dp);
    }

    // line/area-only charts plot their points to the frame edges; anything with columns centres them
    let edges = series.iter().all(|s| matches!(s.kind, SeriesKind::Line | SeriesKind::Area)) && ncat > 1;
    let along = if horizontal { h } else { w };
    let slot = along / ncat as f32;
    let centre = |i: usize| if edges { along * i as f32 / (ncat - 1) as f32 } else { slot * (i as f32 + 0.5) };
    let at: Vec<f32> = (0..ncat).map(centre).collect();

    // columns / bars
    let bars: Vec<&ChartSeries> = series.iter().filter(|s| matches!(s.kind, SeriesKind::Column | SeriesKind::Bar)).collect();
    if !bars.is_empty() {
        draw_bars(cx, c, &bars, series, &sc, sc2.as_ref(), slot, w, h, horizontal, stacked, depth);
    }
    // areas behind lines; later series behind earlier ones
    let areas: Vec<(usize, &ChartSeries)> = series.iter().enumerate().filter(|(_, s)| s.kind == SeriesKind::Area).collect();
    if !areas.is_empty() {
        draw_areas(cx, &areas, &sc, sc2.as_ref(), &at, h, stacked, depth);
    }
    for (si, s) in series.iter().enumerate().filter(|(_, s)| s.kind == SeriesKind::Line) {
        let scale = if s.y2 { sc2.as_ref().unwrap_or(&sc) } else { &sc };
        let row = if depth.is_some() && !stacked { si } else { 0 };
        draw_line_series(cx, s, scale, &at, h, depth, row);
    }

    let base_line = if horizontal { w * sc.frac(0.0) } else { h * (1.0 - sc.frac(0.0)) };
    if depth.is_none() {
        draw_category_axis_line(cx, c, w, h, horizontal, base_line);
    }
    draw_category_labels(cx, c, &c.categories, &at, slot, w, h, horizontal);
}

/// Back wall shading of 3D charts: the floor edge from the front plane to the back.
fn draw_floor(cx: &mut Cx, w: f32, h: f32, horizontal: bool, dp: Depth) {
    let s = plain_stroke(GRID_LINE, 1.0);
    if horizontal {
        cx.line(0.0, 0.0, dp.dx, dp.dy, &s);
        cx.line(0.0, h, dp.dx, h + dp.dy, &s);
    } else {
        cx.line(0.0, h, dp.dx, h + dp.dy, &s);
        cx.line(w, h, w + dp.dx, h + dp.dy, &s);
    }
}

/// A box (front rectangle + top/side faces when extruded).
fn draw_box(cx: &mut Cx, x: f32, y: f32, bw: f32, bh: f32, color: Color, depth: Option<Depth>) {
    if let Some(dp) = depth {
        let (dx, dy) = (dp.dx, dp.dy);
        cx.fill(poly(&[(x + bw, y), (x + bw + dx, y + dy), (x + bw + dx, y + bh + dy), (x + bw, y + bh)], true), shade(color, 0.72));
        cx.fill(poly(&[(x, y), (x + dx, y + dy), (x + bw + dx, y + dy), (x + bw, y)], true), shade(color, 1.3));
    }
    cx.fill(rect_path(x, y, bw, bh), color);
}

/// Where a bar's rounded corners go: the end away from the axis (`outer`), or all four.
#[derive(Clone, Copy)]
enum Round {
    All,
    /// The outer end: top of a rising column, right of a bar to the right (and the other way
    /// for negative values).
    Outer { horizontal: bool, positive: bool },
}

/// A flat bar in its series' own look: fill (a gradient too), outline and rounded corners.
#[allow(clippy::too_many_arguments)]
fn draw_bar(cx: &mut Cx, x: f32, y: f32, bw: f32, bh: f32, s: &ChartSeries, radius: f32, round: Round) {
    // `radius`: a fraction of half the bar's thickness (its width across the value axis)
    let thick = match round {
        Round::Outer { horizontal: true, .. } => bh,
        _ => bw,
    };
    let r = (radius * thick / 2.0).min(bw / 2.0).min(bh / 2.0).max(0.0);
    let path = if r > 0.01 {
        // corners: top-left, top-right, bottom-right, bottom-left
        let [tl, tr, br, bl] = match round {
            Round::All => [r; 4],
            Round::Outer { horizontal: false, positive: true } => [r, r, 0.0, 0.0],
            Round::Outer { horizontal: false, positive: false } => [0.0, 0.0, r, r],
            Round::Outer { horizontal: true, positive: true } => [0.0, r, r, 0.0],
            Round::Outer { horizontal: true, positive: false } => [r, 0.0, 0.0, r],
        };
        let k = 0.552_284_8;
        let mut b = sk::PathBuilder::new();
        b.move_to(tl, 0.0);
        b.line_to(bw - tr, 0.0);
        b.cubic_to(bw - tr + tr * k, 0.0, bw, tr - tr * k, bw, tr);
        b.line_to(bw, bh - br);
        b.cubic_to(bw, bh - br + br * k, bw - br + br * k, bh, bw - br, bh);
        b.line_to(bl, bh);
        b.cubic_to(bl - bl * k, bh, 0.0, bh - bl + bl * k, 0.0, bh - bl);
        b.line_to(0.0, tl);
        b.cubic_to(0.0, tl - tl * k, tl - tl * k, 0.0, tl, 0.0);
        b.close();
        b.finish()
    } else {
        rect_path(0.0, 0.0, bw, bh)
    };
    let Some(path) = path else { return };
    let t = cx.t.pre_translate(x, y);
    match crate::paint::fill_paint(&s.fill, bw, bh) {
        Some(paint) => cx.pm.fill_path(&path, &paint, sk::FillRule::Winding, t, None),
        None => cx.pm.fill_path(&path, &solid(s.color), sk::FillRule::Winding, t, None),
    }
    if let Some(st) = &s.stroke {
        if st.width > 0.0 {
            cx.pm.stroke_path(&path, &solid(st.color), &sk_stroke(st), t, None);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bars(
    cx: &mut Cx,
    c: &ChartData,
    bars: &[&ChartSeries],
    all: &[ChartSeries],
    sc: &Scale,
    sc2: Option<&Scale>,
    slot: f32,
    w: f32,
    h: f32,
    horizontal: bool,
    stacked: bool,
    depth: Option<Depth>,
) {
    let n = bars.len() as f32;
    let set_gap = c.set_gap;
    let (bar, gap) = if stacked {
        (slot / (1.0 + set_gap), 0.0)
    } else {
        let bar = slot / (n + (n - 1.0) * c.bar_gap + set_gap);
        (bar, bar * c.bar_gap)
    };
    let group = if stacked { bar } else { n * bar + (n - 1.0) * gap };
    let ncat = bars.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let mut pos = vec![0f64; ncat];
    let mut neg = vec![0f64; ncat];
    let label_style = &c.outside_label_style;
    // left to right / top to bottom: back to front for the oblique extrusion
    for i in 0..ncat {
        for (k, s) in bars.iter().enumerate() {
            let scale = if s.y2 { sc2.unwrap_or(sc) } else { sc };
            let v = s.values.get(i).copied().unwrap_or(f64::NAN);
            if !v.is_finite() {
                continue;
            }
            let (v0, v1) = if stacked {
                let acc = if v >= 0.0 { &mut pos[i] } else { &mut neg[i] };
                let start = *acc;
                *acc += v;
                (start, *acc)
            } else {
                (0.0, v)
            };
            let off = slot * i as f32 + (slot - group) / 2.0 + if stacked { 0.0 } else { (bar + gap) * k as f32 };
            let (f0, f1) = (scale.frac(v0.min(v1)), scale.frac(v0.max(v1)));
            // horizontal bars run top to bottom in category order
            let (x, y, bw, bh) = if horizontal {
                (w * f0, off, w * (f1 - f0), bar)
            } else {
                (off, h * (1.0 - f1), bar, h * (f1 - f0))
            };
            if depth.is_some() {
                draw_box(cx, x, y, bw, bh, s.color, depth);
            } else {
                let round = if c.corner_outer_only { Round::Outer { horizontal, positive: v >= 0.0 } } else { Round::All };
                draw_bar(cx, x, y, bw, bh, s, c.corner_radius, round);
            }
            if s.show_value_labels {
                let txt = fmt_with(v, s.value_format.as_ref());
                let inside = &c.value_label_style;
                let fits = if horizontal { bw > inside.size * 3.0 } else { bh > inside.size * 1.6 };
                if s.label_position == 4 && depth.is_none() && fits {
                    // inside the bar's end (Numbers / Keynote's default for columns and bars)
                    let pad = inside.size * 0.4;
                    if horizontal {
                        let (tx, al) = if v >= 0.0 { (x + bw - pad - 400.0, Align::Right) } else { (x + pad, Align::Left) };
                        cx.text(&txt, inside, tx, y, 400.0, bh, al, VAlign::Middle);
                    } else {
                        let ty = if v >= 0.0 { y + pad + inside.size * 0.8 } else { y + bh - pad - inside.size * 0.8 };
                        cx.text_at(&txt, inside, x + bw / 2.0, ty, bw.max(inside.size * 4.0));
                    }
                } else if horizontal {
                    cx.text(&txt, label_style, x + bw + 4.0, y, 400.0, bh, Align::Left, VAlign::Middle);
                } else {
                    cx.text_at(&txt, label_style, x + bw / 2.0, y - label_style.size * 0.8, bw.max(label_style.size * 4.0));
                }
            }
        }
    }
    let _ = all;
}

#[allow(clippy::too_many_arguments)]
fn draw_areas(cx: &mut Cx, areas: &[(usize, &ChartSeries)], sc: &Scale, sc2: Option<&Scale>, at: &[f32], h: f32, stacked: bool, depth: Option<Depth>) {
    let ncat = at.len();
    // stacked tops per series, bottom-up
    let mut acc = vec![0f64; ncat];
    let mut layers: Vec<(Vec<(f32, f32)>, Vec<(f32, f32)>, &ChartSeries, usize)> = Vec::new();
    for (row, (_, s)) in areas.iter().enumerate() {
        let scale = if s.y2 { sc2.unwrap_or(sc) } else { sc };
        let base: Vec<f64> = if stacked { acc.clone() } else { vec![0.0; ncat] };
        let top: Vec<f64> = (0..ncat).map(|i| base[i] + s.values.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0)).collect();
        let pt = |i: usize, v: f64| (at[i], h * (1.0 - scale.frac(v)));
        layers.push(((0..ncat).map(|i| pt(i, top[i])).collect(), (0..ncat).map(|i| pt(i, base[i])).collect(), s, row));
        if stacked {
            acc = top;
        }
    }
    // non-stacked: first series in front (drawn last); 3D: each series in its own depth row
    let order: Vec<usize> = if stacked { (0..layers.len()).collect() } else { (0..layers.len()).rev().collect() };
    for li in order {
        let (top, bottom, s, row) = &layers[li];
        let shift = match depth {
            Some(dp) if !stacked => ((*row as f32) * dp.dx, (*row as f32) * dp.dy),
            _ => (0.0, 0.0),
        };
        let mv = |p: &(f32, f32)| (p.0 + shift.0, p.1 + shift.1);
        if let Some(dp) = depth {
            // top ribbon
            for i in 0..top.len().saturating_sub(1) {
                let (a, b) = (mv(&top[i]), mv(&top[i + 1]));
                let q = [a, b, (b.0 + dp.dx, b.1 + dp.dy), (a.0 + dp.dx, a.1 + dp.dy)];
                cx.fill(poly(&q, true), shade(s.color, 1.25));
            }
            let (e0, e1) = (mv(top.last().unwrap()), mv(bottom.last().unwrap()));
            cx.fill(poly(&[e0, (e0.0 + dp.dx, e0.1 + dp.dy), (e1.0 + dp.dx, e1.1 + dp.dy), e1], true), shade(s.color, 0.7));
        }
        let mut pts: Vec<(f32, f32)> = top.iter().map(mv).collect();
        pts.extend(bottom.iter().rev().map(mv));
        cx.fill(poly(&pts, true), s.color);
    }
}

fn draw_line_series(cx: &mut Cx, s: &ChartSeries, sc: &Scale, at: &[f32], h: f32, depth: Option<Depth>, row: usize) {
    let pts: Vec<(f32, f32)> = s
        .values
        .iter()
        .enumerate()
        .filter(|(i, v)| v.is_finite() && *i < at.len())
        .map(|(i, v)| (at[i], h * (1.0 - sc.frac(*v))))
        .collect();
    if pts.is_empty() {
        return;
    }
    if let Some(dp) = depth {
        // a ribbon in its own depth row: lit top face plus a darker front edge
        let (ox, oy) = (dp.dx * row as f32, dp.dy * row as f32);
        let p: Vec<(f32, f32)> = pts.iter().map(|(x, y)| (x + ox, y + oy)).collect();
        for i in 0..p.len().saturating_sub(1) {
            let (a, b) = (p[i], p[i + 1]);
            let q = [a, b, (b.0 + dp.dx * 0.8, b.1 + dp.dy * 0.8), (a.0 + dp.dx * 0.8, a.1 + dp.dy * 0.8)];
            cx.fill(poly(&q, true), shade(s.color, 1.2));
        }
        cx.stroke(poly(&p, false), shade(s.color, 0.7), (dp.dx * 0.25).max(2.0));
        return;
    }
    cx.stroke(poly(&pts, false), s.color, line_width(s));
    if s.show_symbols {
        for (x, y) in &pts {
            let mut b = sk::PathBuilder::new();
            b.push_circle(*x, *y, line_width(s) * 1.6);
            cx.fill(b.finish(), s.color);
        }
    }
}

/// Interactive (multi-data) column/bar charts: one bar per series for the current data set,
/// each with its own slot and label, the data set name below.
fn draw_data_set(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w: f32, h: f32) {
    let horizontal = series.iter().all(|s| s.kind == SeriesKind::Bar);
    let all: Vec<&ChartSeries> = series.iter().collect();
    let (lo, hi) = value_range(&all, 1, false);
    // the axis spans every data set so bars keep their scale while switching sets
    let (alo, ahi) = c.series.iter().flat_map(|s| s.values.iter().copied()).filter(|v| v.is_finite()).fold((lo, hi), |(a, b), v| (a.min(v), b.max(v)));
    let axis0 = c.value_axes.first().cloned().unwrap_or_default();
    let sc = scale_for(alo, ahi, &axis0, false);
    draw_value_axis(cx, &axis0, &sc, w, h, horizontal, false, None);
    let n = series.len();
    let along = if horizontal { h } else { w };
    let slot = along / n as f32;
    let bar = slot / (1.0 + c.set_gap);
    for (i, s) in series.iter().enumerate() {
        let Some(v) = s.values.first().copied().filter(|v| v.is_finite()) else { continue };
        let off = slot * i as f32 + (slot - bar) / 2.0;
        let (f0, f1) = (sc.frac(0f64.min(v)), sc.frac(0f64.max(v)));
        if horizontal {
            draw_box(cx, w * f0, off, w * (f1 - f0), bar, s.color, None);
        } else {
            draw_box(cx, off, h * (1.0 - f1), bar, h * (f1 - f0), s.color, None);
        }
    }
    let names: Vec<String> = series.iter().map(|s| s.name.clone()).collect();
    let at: Vec<f32> = (0..n).map(|i| slot * (i as f32 + 0.5)).collect();
    let base = if horizontal { w * sc.frac(0.0) } else { h * (1.0 - sc.frac(0.0)) };
    draw_category_axis_line(cx, c, w, h, horizontal, base);
    draw_category_labels(cx, c, &names, &at, slot, w, h, horizontal);
    let set = c.data_set.unwrap_or(0) as usize;
    if let Some(name) = c.categories.get(set) {
        let st = &c.category_axis.label_style;
        let y = h + st.size * (if horizontal { 1.8 } else { 2.2 }) + st.size * 0.7;
        cx.text_at(name, st, w / 2.0, y, w);
    }
}

// ---------------------------------------------------------------------------------- pie / donut

fn wedge(cx0: f32, cy0: f32, r_out: f32, r_in: f32, a0: f32, sweep: f32, sy: f32) -> Option<sk::Path> {
    let steps = ((sweep.abs() / TAU) * 128.0).ceil().max(2.0) as usize;
    let mut b = sk::PathBuilder::new();
    for k in 0..=steps {
        let a = a0 + sweep * k as f32 / steps as f32;
        let (x, y) = (cx0 + r_out * a.cos(), cy0 + r_out * a.sin() * sy);
        if k == 0 { b.move_to(x, y) } else { b.line_to(x, y) }
    }
    if r_in > 0.0 {
        for k in (0..=steps).rev() {
            let a = a0 + sweep * k as f32 / steps as f32;
            b.line_to(cx0 + r_in * a.cos(), cy0 + r_in * a.sin() * sy);
        }
    } else {
        b.line_to(cx0, cy0);
    }
    b.close();
    b.finish()
}

fn draw_pie(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w: f32, h: f32) {
    // one wedge per series: its value in the first category (or the shown data set)
    let vals: Vec<f64> = series.iter().map(|s| s.values.first().copied().filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0)).collect();
    let total: f64 = vals.iter().sum();
    if total <= 0.0 {
        return;
    }
    let donut = c.chart_type.to_ascii_lowercase().contains("donut");
    let three_d = c.is_3d();
    let (sy, thick) = if three_d { (0.9, w.min(h) * 0.06) } else { (1.0, 0.0) };
    // wedges pulled out (Format > Wedges > Explode) leave room for themselves
    let most = series.iter().map(|s| s.explosion).fold(0.0f32, f32::max);
    let rad = (w / 2.0).min((h - thick) / 2.0 / sy) / (1.0 + most);
    let (cx0, cy0) = (w / 2.0, h / 2.0 - thick / 2.0);
    let r_in = if donut { rad * c.inner_radius } else { 0.0 };
    let mut angles = Vec::new();
    let mut a0 = -FRAC_PI_2;
    for v in &vals {
        let sweep = (*v / total) as f32 * TAU;
        angles.push((a0, sweep));
        a0 += sweep;
    }
    // each wedge's offset along its middle
    let out: Vec<(f32, f32)> = series
        .iter()
        .zip(&angles)
        .map(|(s, (a, sw))| {
            let am = a + sw / 2.0;
            (s.explosion * rad * am.cos(), s.explosion * rad * am.sin() * sy)
        })
        .collect();
    if three_d {
        // side wall: the front-facing (lower) half of each wedge's rim, extruded downwards
        for (i, s) in series.iter().enumerate() {
            let (a, sw) = angles[i];
            let mut segs = Vec::new();
            for base in [0.0, TAU] {
                let (l, u) = ((a - base).max(0.0), (a + sw - base).min(std::f32::consts::PI));
                if u > l {
                    segs.push((l + base, u + base));
                }
            }
            for (l, u) in segs {
                let steps = (((u - l) / TAU) * 128.0).ceil().max(2.0) as usize;
                let mut top = Vec::new();
                for k in 0..=steps {
                    let ang = l + (u - l) * k as f32 / steps as f32;
                    top.push((cx0 + out[i].0 + rad * ang.cos(), cy0 + out[i].1 + rad * ang.sin() * sy));
                }
                let mut pts = top.clone();
                pts.extend(top.iter().rev().map(|(x, y)| (*x, y + thick)));
                cx.fill(poly(&pts, true), shade(s.color, 0.7));
            }
        }
    }
    for (i, s) in series.iter().enumerate() {
        let (a, sw) = angles[i];
        cx.fill(wedge(cx0 + out[i].0, cy0 + out[i].1, rad, r_in, a, sw, sy), s.color);
    }
    // percentage labels: inside pie wedges, outside donut rings
    for (i, s) in series.iter().enumerate() {
        if !(s.show_value_labels || s.show_series_names) || vals[i] <= 0.0 {
            continue;
        }
        let (a, sw) = angles[i];
        let am = a + sw / 2.0;
        let pct = match (s.show_series_names, s.show_value_labels) {
            (true, true) => format!("{} {:.0}%", s.name, vals[i] / total * 100.0),
            (true, false) => s.name.clone(),
            _ => format!("{:.0}%", vals[i] / total * 100.0),
        };
        if donut {
            // outside, in a column left or right of the ring, with a leader line
            let st = &c.outside_label_style;
            let right = am.cos() >= 0.0;
            let side = if right { 1.0 } else { -1.0 };
            let y = cy0 + rad * 1.05 * am.sin() * sy;
            let edge = cx0 + side * rad * 1.25;
            let (lw, lh) = (st.size * 4.0, st.size * 1.6);
            let from = (cx0 + rad * 1.02 * am.cos(), cy0 + rad * 1.02 * am.sin() * sy);
            let knee = (cx0 + side * (rad * 1.05 * am.cos().abs()).max(rad * 0.3), y);
            cx.stroke_with(poly(&[from, knee, (edge - side * st.size * 0.3, y)], false), &plain_stroke(GRID_LINE, 1.0));
            let (lx, align) = if right { (edge, Align::Left) } else { (edge - lw, Align::Right) };
            cx.text(&pct, st, lx, y - lh / 2.0, lw, lh, align, VAlign::Middle);
        } else {
            let rr = rad * s.label_distance.map(|d| d / 100.0).unwrap_or(0.62);
            let (lx, ly) = (cx0 + out[i].0 + rr * am.cos(), cy0 + out[i].1 + rr * am.sin() * sy);
            let st = &c.value_label_style;
            if s.show_series_names && s.show_value_labels {
                // Keynote stacks the series name over the percentage
                let half = st.size * 0.6;
                cx.text_at(&s.name, st, lx, ly - half, rad);
                cx.text_at(&format!("{:.0}%", vals[i] / total * 100.0), st, lx, ly + half, rad);
            } else {
                cx.text_at(&pct, st, lx, ly, rad);
            }
        }
    }
}

// ---------------------------------------------------------------------------------- radar

fn draw_radar(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w: f32, h: f32) {
    let ncat = series.iter().map(|s| s.values.len()).max().unwrap_or(0).max(c.categories.len());
    if ncat < 3 {
        return;
    }
    let all: Vec<&ChartSeries> = series.iter().collect();
    let (lo, hi) = value_range(&all, ncat, false);
    let axis = c.value_axes.first().cloned().unwrap_or_default();
    let sc = scale_for(lo, hi, &axis, false);
    let (cx0, cy0) = (w / 2.0, h / 2.0);
    let rad = w.min(h) / 2.0;
    let pt = |i: usize, f: f32| {
        let a = -FRAC_PI_2 + TAU * i as f32 / ncat as f32;
        (cx0 + rad * f * a.cos(), cy0 + rad * f * a.sin())
    };
    let grid = axis.gridline.clone().unwrap_or_else(|| plain_stroke(GRID_LINE, 1.0));
    for v in sc.ticks() {
        let f = sc.frac(v);
        let ring: Vec<(f32, f32)> = (0..ncat).map(|i| pt(i, f)).collect();
        cx.stroke_with(poly(&ring, true), &grid);
        if axis.show_labels {
            let st = &axis.label_style;
            let lh = st.size * 1.4;
            cx.text(&fmt_value(v), st, cx0 - 200.0 - st.size * 0.3, cy0 - rad * f - lh / 2.0, 200.0, lh, Align::Right, VAlign::Middle);
        }
    }
    for i in 0..ncat {
        let (x, y) = pt(i, 1.0);
        cx.line(cx0, cy0, x, y, &grid);
    }
    for s in series {
        let p: Vec<(f32, f32)> = (0..ncat).map(|i| pt(i, sc.frac(s.values.get(i).copied().unwrap_or(0.0)).clamp(0.0, 1.0))).collect();
        if let Fill::Color(fc) = s.fill {
            cx.fill(poly(&p, true), fc);
        }
        cx.stroke(poly(&p, true), s.color, line_width(s));
    }
    let st = &c.category_axis.label_style;
    for (i, name) in c.categories.iter().enumerate().take(ncat) {
        let a = -FRAC_PI_2 + TAU * i as f32 / ncat as f32;
        let (x, y) = pt(i, 1.0);
        let lw = st.size * 8.0;
        let lh = st.size * 1.6;
        let gap = st.size * 0.5;
        let (lx, align) = if a.cos() > 0.2 {
            (x + gap, Align::Left)
        } else if a.cos() < -0.2 {
            (x - gap - lw, Align::Right)
        } else {
            (x - lw / 2.0, Align::Center)
        };
        let ly = if a.sin() < -0.5 { y - lh - gap * 0.2 } else if a.sin() > 0.5 { y + gap * 0.2 } else { y - lh / 2.0 };
        cx.text(name, st, lx, ly, lw, lh, align, VAlign::Middle);
    }
}

// ---------------------------------------------------------------------------------- scatter / bubble

fn draw_xy(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w: f32, h: f32) {
    let bubble = series[0].kind == SeriesKind::Bubble;
    let finite = |v: &f64| v.is_finite();
    let all_x = || c.series.iter().flat_map(|s| s.xs.iter().copied()).filter(finite);
    let all_y = || c.series.iter().flat_map(|s| s.values.iter().copied()).filter(finite);
    let (xlo, xhi) = all_x().fold((0f64, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (ylo, yhi) = all_y().fold((0f64, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let yaxis = c.value_axes.first().cloned().unwrap_or_default();
    let mut xaxis = c.category_axis.clone();
    xaxis.show_gridlines = false;
    let scy = scale_for(ylo, yhi, &yaxis, true);
    let scx = scale_for(xlo, xhi, &xaxis, true);
    draw_value_axis(cx, &yaxis, &scy, w, h, false, false, None);
    draw_value_axis(cx, &xaxis, &scx, w, h, true, false, None);
    if let Some(name) = c.data_set.and_then(|i| c.row_names.get(i as usize).or(c.categories.get(i as usize))) {
        let st = &xaxis.label_style;
        cx.text_at(name, st, w / 2.0, h + st.size * 2.6, w);
    }
    let max_size = c.series.iter().flat_map(|s| s.sizes.iter().copied()).filter(finite).fold(0f64, f64::max);
    let max_r = w.min(h) * 0.07;
    for s in series {
        let pts: Vec<(f32, f32, f64)> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, y)| {
                let x = s.xs.get(i).copied()?;
                (x.is_finite() && y.is_finite()).then(|| (w * scx.frac(x), h * (1.0 - scy.frac(*y)), s.sizes.get(i).copied().unwrap_or(0.0)))
            })
            .collect();
        if bubble {
            for (x, y, size) in &pts {
                let rr = if max_size > 0.0 { max_r * (size.max(0.0) / max_size).sqrt() as f32 } else { max_r * 0.5 };
                let mut b = sk::PathBuilder::new();
                b.push_circle(*x, *y, rr.max(1.0));
                cx.fill(b.finish(), s.color);
                if s.show_series_names {
                    cx.text_at(&s.name, &c.value_label_style, *x, *y, (rr * 2.0).max(c.value_label_style.size * 6.0));
                }
            }
        } else if s.show_symbols {
            for (x, y, _) in &pts {
                let mut b = sk::PathBuilder::new();
                b.push_circle(*x, *y, 6.5);
                cx.fill(b.finish(), s.color);
            }
        }
    }
}

// ---------------------------------------------------------------------------------- legend

fn draw_legend(cx: &mut Cx, c: &ChartData, series: &[ChartSeries], w: f32, h: f32) {
    let st = &c.legend_style;
    let sw = st.size * 0.8;
    let item: Vec<f32> = series.iter().map(|s| sw * 1.6 + s.name.chars().count() as f32 * st.size * 0.55 + st.size).collect();
    let total: f32 = item.iter().sum();
    let lh = st.size * 1.6;
    // legend frames are stored relative to the chart frame's centre; the entries spread over a
    // frame wider than they need, the spare room shared evenly before, between and after them
    // (Numbers' N29-H2: entries at 73.6 / 213.9 pt of a 360 pt frame, measured 72 / 213)
    let (mut x, y, gap) = match c.legend_frame {
        Some([fx, fy, fw, fh]) if fw > 0.0 => {
            let spare = (fw - total).max(0.0) / (series.len() + 1) as f32;
            (w / 2.0 + fx + spare, h / 2.0 + fy + (fh - lh) / 2.0, spare)
        }
        _ => ((w - total) / 2.0, -lh * 1.5, 0.0),
    };
    for (s, iw) in series.iter().zip(item) {
        cx.fill(rect_path(x, y + (lh - sw) / 2.0, sw, sw), s.color);
        cx.text(&s.name, st, x + sw * 1.4, y, iw, lh, Align::Left, VAlign::Middle);
        x += iw + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis() -> ChartAxis {
        ChartAxis::default()
    }

    #[test]
    fn keynote_like_scales() {
        // (data max, fine steps) -> axis top as in Keynote's export of the feature catalog (3 -> 0.75 steps: Mac R5)
        for (max, fine, top) in [(40.0, false, 40.0), (96.0, false, 100.0), (154.0, false, 200.0), (95.0, false, 100.0), (200.0, false, 200.0), (3.0, false, 3.0)] {
            assert_eq!(scale_for(0.0, max, &axis(), fine).hi, top, "{max}");
        }
        for (max, top) in [(12.0, 12.0), (15.0, 16.0), (20.0, 20.0), (24.0, 30.0), (70.0, 70.0)] {
            let s = scale_for(0.0, max, &axis(), true);
            assert_eq!((s.hi, s.n), (top, 4), "{max}");
        }
    }
}
