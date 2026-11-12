//! Channel EQ's and Linear Phase EQ's editor view, as Logic's (measured on its screenshot at
//! 100 %): the band buttons (each band's type icon in its colour, click = on / off) above the
//! graphic display; the analyzer scale at the left (its top and range), the EQ scale at the
//! right (dB Scale Mode, offset by dragging), the master Gain slider; each band's curve in its
//! colour and the sum; under the pointer a band lights up with its Frequency / Gain / Q fields.
//!
//! Editing in the display: click a band to select it (the background deselects; while one is
//! selected, other points inside its area can't be picked); drag inside the band = frequency and
//! gain; its control point = frequency only (bands 1 and 8: frequency and Q); its horizontal line
//! = gain (the DSP couples Q when Q-Couple is on); its vertical lines = Q (bands 1 and 8: drag
//! them vertically for the slope); where they meet = gain and Q; Command (Ctrl here) constrains
//! a drag to one axis; Option-Command = Q and frequency; the wheel over a selected band = Q.
//! Fields drag vertically or take a typed value (double-click). Below: Analyzer (PRE / POST),
//! Q-Couple, HQ, Processing. Control-click (right-click here) the display, Analyzer or Q-Couple
//! for their options (dB Scale Mode, Visualize Master Gain, Analyzer Mode / Resolution, Gain-Q
//! Couple Strength). The settings live in the plug-in's parameters (lg_fx::channel_eq).

use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::{Param, Plugin};
use lg_fx::channel_eq::*;

use crate::look;

/// Logic's band colours (1 red … 8 pink); bands that are off are grey.
const COLORS: [(u8, u8, u8); 8] = [(214, 92, 84), (214, 138, 58), (206, 210, 84), (126, 214, 102), (108, 214, 162), (122, 176, 222), (172, 138, 222), (218, 112, 182)];
const OFF: Color32 = Color32::from_rgb(72, 87, 106);
const BODY_H: f32 = 407.0;

fn color(k: usize, on: bool) -> Color32 {
    if on { Color32::from_rgb(COLORS[k].0, COLORS[k].1, COLORS[k].2) } else { OFF }
}

fn fmt_freq(f: f32) -> String {
    if f < 100.0 { format!("{f:.1}") } else { format!("{f:.0}") }
}

/// The layout, from the body's rectangle (full window width).
struct Geo {
    body: Rect,
    disp: Rect,
}

impl Geo {
    fn new(body: Rect) -> Geo {
        let w = body.width();
        let disp = Rect::from_min_max(pos2(body.min.x + 38.0, body.min.y + 45.0), pos2(body.min.x + w - 87.0, body.min.y + 291.0));
        Geo { body, disp }
    }
    fn x(&self, f: f32) -> f32 {
        self.disp.min.x + (f.clamp(20.0, 20000.0) / 20.0).log10() / 3.0 * self.disp.width()
    }
    fn f(&self, x: f32) -> f32 {
        20.0 * 10f32.powf(((x - self.disp.min.x) / self.disp.width()).clamp(0.0, 1.0) * 3.0)
    }
    fn col(&self, k: usize) -> f32 {
        self.disp.min.x + 38.0 + k as f32 * (self.disp.width() - 76.0) / 7.0
    }
    fn row(&self, y: f32) -> f32 {
        self.body.min.y + y
    }
}

/// The EQ scale: (half span dB, warped).
fn eq_scale(mode: usize, offset: f32) -> (f32, bool) {
    let half = [6.0, 15.0, 30.0, 30.0][mode.min(3)] * 2f32.powf(-offset / 10.0);
    (half.clamp(1.0, 60.0), mode == 3)
}

fn gain_y(g: &Geo, db: f32, (half, warped): (f32, bool)) -> f32 {
    let t = (db / half).clamp(-1.2, 1.2);
    let t = if warped { t.signum() * t.abs().sqrt() } else { t };
    g.disp.center().y - t * g.disp.height() / 2.0
}

fn y_gain(g: &Geo, y: f32, (half, warped): (f32, bool)) -> f32 {
    let t = (g.disp.center().y - y) / (g.disp.height() / 2.0);
    let t = if warped { t.signum() * t * t } else { t };
    t * half
}

/// A band's area (Hz): peaks span their bandwidth, shelves and cuts reach the edge.
fn area(k: usize, f: f32, q: f32) -> (f32, f32) {
    let half = (1.0 / (2.0 * q.max(0.05))).asinh() / std::f32::consts::LN_2;
    match k {
        0 => (20.0, f * 2.0),
        7 => (f / 2.0, 20000.0),
        1 => (20.0, f * 2f32.powf(half.max(0.5))),
        6 => (f / 2f32.powf(half.max(0.5)), 20000.0),
        _ => (f / 2f32.powf(half), f * 2f32.powf(half)),
    }
}

/// Draws the band type icon (cut, shelf, bell) in `r`.
fn band_icon(p: &egui::Painter, k: usize, c: Pos2, col: Color32) {
    let s = Stroke::new(1.3, col);
    let (w, h) = (20.0, 7.0);
    let pts: Vec<Pos2> = match k {
        0 => vec![pos2(c.x - w, c.y + h), pos2(c.x - w * 0.3, c.y - h * 0.6), pos2(c.x + w * 0.1, c.y - h)],
        7 => vec![pos2(c.x - w * 0.1, c.y - h), pos2(c.x + w * 0.3, c.y - h * 0.6), pos2(c.x + w, c.y + h)],
        1 => vec![pos2(c.x - w, c.y - h), pos2(c.x - w * 0.2, c.y - h), pos2(c.x + w * 0.2, c.y), pos2(c.x + w, c.y)],
        6 => vec![pos2(c.x - w, c.y), pos2(c.x - w * 0.2, c.y), pos2(c.x + w * 0.2, c.y - h), pos2(c.x + w, c.y - h)],
        _ => vec![pos2(c.x - w, c.y), pos2(c.x - w * 0.45, c.y), pos2(c.x, c.y - h), pos2(c.x + w * 0.45, c.y), pos2(c.x + w, c.y)],
    };
    p.add(egui::Shape::line(pts.clone(), s));
    if (1..=6).contains(&k) {
        // the shelves' and bells' lower half (Logic draws them as a diamond / step outline)
        let low: Vec<Pos2> = pts.iter().map(|q| pos2(q.x, c.y + (c.y - q.y))).collect();
        if (2..=5).contains(&k) {
            p.add(egui::Shape::line(low, s));
        }
    }
}

/// What a display drag changes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    FreqGain,
    Freq,
    FreqQ,
    Gain,
    Q(f32),
    GainQ(f32),
    Slope,
}

#[derive(Clone, Copy, Debug)]
struct DragState {
    band: usize,
    mode: Drag,
    start: Pos2,
    f: f32,
    g: f32,
    q: f32,
}

fn val(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).map(|i| values[i]).unwrap_or(0.0)
}

/// The view; returns the parameter changes.
pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], analyzer: &[f32], slot: (usize, usize), set: &mut Vec<(u32, f32)>) {
    let g = |id| val(params, values, id);
    let margin = ui.spacing().window_margin;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), BODY_H), Sense::hover());
    let body = Rect::from_min_max(pos2(r.min.x - margin.left as f32, r.min.y), pos2(r.max.x + margin.right as f32, r.max.y));
    let geo = Geo::new(body);
    let p = ui.painter().with_clip_rect(body);
    // the blue-grey body: lighter around the display, dark under it
    let mut mesh = egui::Mesh::default();
    let stops = [(0.0, Color32::from_rgb(20, 31, 45)), (0.45, Color32::from_rgb(30, 44, 63)), (0.72, Color32::from_rgb(22, 33, 48)), (1.0, Color32::from_rgb(10, 17, 27))];
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        let rr = Rect::from_min_max(pos2(body.min.x, body.min.y + a.0 * body.height()), pos2(body.max.x, body.min.y + b.0 * body.height()));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(rr.left_top(), a.1);
        mesh.colored_vertex(rr.right_top(), a.1);
        mesh.colored_vertex(rr.right_bottom(), b.1);
        mesh.colored_vertex(rr.left_bottom(), b.1);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    p.add(mesh);
    let disp = geo.disp;
    let scale = eq_scale(g(SCALE_MODE) as usize, g(SCALE_OFFSET));
    let id = egui::Id::unique(("chan-eq", slot));
    // the grid: decades and their steps, the scale lines
    let grid = Color32::from_rgb(41, 55, 76);
    for dec in [10.0f32, 100.0, 1000.0, 10000.0] {
        for m in 1..10 {
            let f = dec * m as f32;
            if (20.0..=20000.0).contains(&f) {
                let x = geo.x(f);
                p.line_segment([pos2(x, disp.min.y - 8.0), pos2(x, disp.max.y + 8.0)], Stroke::new(if m == 1 { 1.0 } else { 0.6 }, grid));
            }
        }
    }
    for i in 0..=12 {
        let y = disp.min.y + disp.height() * i as f32 / 12.0;
        p.line_segment([pos2(disp.min.x, y), pos2(disp.max.x, y)], Stroke::new(0.6, grid));
    }
    let zero = gain_y(&geo, 0.0, scale);
    p.line_segment([pos2(disp.min.x, zero), pos2(disp.max.x, zero)], Stroke::new(1.2, Color32::from_rgb(117, 124, 133)));
    let lab = Color32::from_rgb(140, 152, 168);
    for f in [20.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 200.0, 300.0, 400.0, 500.0, 800.0, 1000.0, 2000.0, 3000.0, 4000.0, 6000.0, 8000.0, 10000.0, 20000.0] {
        let t = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
        p.text(pos2(geo.x(f), zero + 4.0), Align2::CENTER_TOP, t, look::regular(11.0), lab);
    }
    // left: the analyzer scale (top and range); right: the EQ scale
    let analyzer_on = g(ANALYZER) >= 0.5;
    let top = g(ANALYZER_TOP);
    let range = [60.0, 80.0, 100.0][(g(ANALYZER_RANGE) as usize).min(2)];
    for i in 0..=12 {
        let y = disp.min.y + disp.height() * i as f32 / 12.0;
        // (Logic's left scale counts down from the analyzer's top: 0, 5 … 60)
        let v = range * i as f32 / 12.0 - top;
        let v = if v.abs() < 0.5 { 0.0 } else { v };
        p.text(pos2(body.min.x + 20.0, y), Align2::CENTER_CENTER, format!("{v:.0}"), look::regular(11.0), if analyzer_on { lab } else { lab.gamma_multiply(0.5) });
    }
    let (half, warped) = scale;
    for i in 0..=6 {
        let gdb = half - half * i as f32 / 3.0;
        let y = gain_y(&geo, gdb, scale);
        p.text(pos2(disp.max.x + 24.0, y), Align2::CENTER_CENTER, format!("{:.0}", gdb.abs()), look::regular(11.0), lab);
    }
    let _ = warped;
    for (x, sym) in [(body.min.x + 20.0, "+"), (disp.max.x + 24.0, "+")] {
        p.text(pos2(x, disp.min.y - 21.0), Align2::CENTER_CENTER, sym, look::regular(14.0), lab);
        p.text(pos2(x, disp.max.y + 10.0), Align2::CENTER_CENTER, "−", look::regular(14.0), lab);
    }
    // the scales: drag vertically to offset; right-click for their options
    let left_r = Rect::from_min_max(pos2(body.min.x + 4.0, disp.min.y - 30.0), pos2(disp.min.x - 2.0, disp.max.y + 16.0));
    let right_r = Rect::from_min_max(pos2(disp.max.x + 6.0, disp.min.y - 30.0), pos2(disp.max.x + 40.0, disp.max.y + 16.0));
    let lresp = ui.interact(left_r, id.with("lscale"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    let rresp = ui.interact(right_r, id.with("rscale"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    if lresp.dragged() {
        let dy = lresp.drag_delta().y;
        if analyzer_on {
            set.push((ANALYZER_TOP, (top + dy * 0.25).clamp(-80.0, 20.0)));
        } else {
            set.push((SCALE_OFFSET, (g(SCALE_OFFSET) - dy * 0.1).clamp(-30.0, 30.0)));
        }
    }
    if rresp.dragged() {
        set.push((SCALE_OFFSET, (g(SCALE_OFFSET) - rresp.drag_delta().y * 0.1).clamp(-30.0, 30.0)));
    }
    lresp.context_menu(|ui| {
        for (i, n) in ["60 dB", "80 dB", "100 dB"].iter().enumerate() {
            if ui.add(egui::Button::selectable(g(ANALYZER_RANGE) as usize == i, *n)).clicked() {
                set.push((ANALYZER_RANGE, i as f32));
                ui.close();
            }
        }
    });
    let scale_menu = |ui: &mut egui::Ui, set: &mut Vec<(u32, f32)>| {
        for (i, n) in SCALE_MODES.iter().enumerate() {
            if ui.add(egui::Button::selectable(g(SCALE_MODE) as usize == i, *n)).clicked() {
                set.push((SCALE_MODE, i as f32));
                set.push((SCALE_OFFSET, 0.0));
                ui.close();
            }
        }
    };
    rresp.context_menu(|ui| scale_menu(ui, set));
    // the analyzer curve (same display, dB-linear, its own top / range)
    if analyzer_on && !analyzer.is_empty() {
        let n = analyzer.len();
        let mut pts = Vec::with_capacity(n + 2);
        for (k, v) in analyzer.iter().enumerate() {
            let f = 20.0 * 1000f32.powf(k as f32 / (n - 1) as f32);
            let t = ((top - v) / range).clamp(0.0, 1.0);
            pts.push(pos2(geo.x(f), disp.min.y + t * disp.height()));
        }
        let fill = Color32::from_rgba_unmultiplied(120, 170, 230, 40);
        for w in pts.windows(2) {
            let quad = vec![w[0], w[1], pos2(w[1].x, disp.max.y), pos2(w[0].x, disp.max.y)];
            p.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
        }
        p.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgba_unmultiplied(170, 205, 240, 150))));
        ui.ctx().request_repaint();
    }
    // the bands: their curves and the sum
    let mut eq = lg_fx::channel_eq::ChannelEq::new(crate::audio::RATE as f32);
    for (pp, v) in params.iter().zip(values) {
        eq.set(pp.id, *v);
    }
    let band = |k: usize| (g(4 * k as u32) >= 0.5, g(4 * k as u32 + 1), g(4 * k as u32 + 2), g(4 * k as u32 + 3));
    let state_id = id.with("sel");
    let (mut sel, mut drag): (Option<usize>, Option<DragState>) = ui.data(|d| d.get_temp(state_id)).unwrap_or((None, None));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    // the band under the pointer: the selected one inside its area, else the nearest point
    let point_of = |k: usize| -> Pos2 {
        let (_, f, gn, _) = band(k);
        let gy = if k == 0 || k == 7 { eq.band_db(k, f).unwrap_or(-3.0) } else { gn };
        pos2(geo.x(f), gain_y(&geo, gy, scale))
    };
    let in_area = |k: usize, x: f32| {
        let (_, f, _, q) = band(k);
        let (a, b) = area(k, f, q);
        x >= geo.x(a) && x <= geo.x(b)
    };
    let hovered = pointer.filter(|pt| disp.expand(8.0).contains(*pt)).and_then(|pt| {
        if let Some(s) = sel
            && in_area(s, pt.x)
        {
            return Some(s);
        }
        (0..8).filter(|k| (band(*k).0 && in_area(*k, pt.x)) || point_of(*k).distance(pt) < 8.0).min_by(|a, b| point_of(*a).distance(pt).total_cmp(&point_of(*b).distance(pt)))
    });
    let lit = drag.map(|d| d.band).or(hovered).or(sel);
    let steps = 300;
    let mut sum = Vec::with_capacity(steps);
    let vis_master = g(VIS_MASTER) >= 0.5;
    for i in 0..steps {
        let x = disp.min.x + disp.width() * i as f32 / (steps - 1) as f32;
        let f = geo.f(x);
        let mut tot = if vis_master { g(32) } else { 0.0 };
        for k in 0..8 {
            if let Some(d) = eq.band_db(k, f) {
                tot += d;
            }
        }
        sum.push(pos2(x, gain_y(&geo, tot, scale)));
    }
    for k in 0..8 {
        let (on, _, _, _) = band(k);
        if !on {
            continue;
        }
        let c = color(k, true);
        let pts: Vec<Pos2> = (0..steps).map(|i| {
            let x = disp.min.x + disp.width() * i as f32 / (steps - 1) as f32;
            pos2(x, gain_y(&geo, eq.band_db(k, geo.f(x)).unwrap_or(0.0), scale))
        }).collect();
        let a = if lit == Some(k) { 70 } else { 28 };
        for w in pts.windows(2) {
            if (w[0].y - zero).abs() > 0.3 || (w[1].y - zero).abs() > 0.3 {
                let quad = vec![w[0], w[1], pos2(w[1].x, zero), pos2(w[0].x, zero)];
                p.add(egui::Shape::convex_polygon(quad, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a), Stroke::NONE));
            }
        }
        if lit == Some(k) {
            p.add(egui::Shape::line(pts, Stroke::new(1.2, c)));
        }
    }
    p.add(egui::Shape::line(sum, Stroke::new(1.6, Color32::from_rgb(232, 236, 242))));
    // control points; the selected band's lines
    for k in 0..8 {
        let (on, _, _, _) = band(k);
        if !on && (k == 0 || k == 7) && lit != Some(k) {
            // (the cuts' points show only while they're on)
            continue;
        }
        let c = color(k, on);
        let pt = point_of(k);
        let big = lit == Some(k);
        p.circle_filled(pt, if big { 6.0 } else { 5.0 }, c.gamma_multiply(if on { 0.55 } else { 0.35 }));
        if big {
            p.circle_stroke(pt, 6.0, Stroke::new(1.0, c));
        }
    }
    if let Some(s) = sel {
        let (on, f, _, q) = band(s);
        let c = color(s, on);
        let (a, b) = area(s, f, q);
        for fx in [a, b] {
            if (21.0..19999.0).contains(&fx) {
                p.line_segment([pos2(geo.x(fx), disp.min.y), pos2(geo.x(fx), disp.max.y)], Stroke::new(1.0, c.gamma_multiply(0.8)));
            }
        }
        let pt = point_of(s);
        p.line_segment([pos2(geo.x(a).max(disp.min.x), pt.y), pos2(geo.x(b).min(disp.max.x), pt.y)], Stroke::new(1.0, c.gamma_multiply(0.8)));
    }
    // display interaction
    let dresp = ui.interact(disp.expand(6.0), id.with("disp"), Sense::click_and_drag());
    let mods = ui.input(|i| i.modifiers);
    let mode_at = |k: usize, pt: Pos2, sel: Option<usize>| -> Drag {
        let (_, f, _, q) = band(k);
        let cp = point_of(k);
        let (a, b) = area(k, f, q);
        let near_v = [a, b].iter().map(|fx| geo.x(*fx)).filter(|x| (x - pt.x).abs() < 5.0).map(|x| if x < cp.x { -1.0 } else { 1.0 }).next();
        let near_h = (pt.y - cp.y).abs() < 5.0;
        if mods.alt && mods.command {
            Drag::FreqQ
        } else if cp.distance(pt) < 7.0 {
            if k == 0 || k == 7 { Drag::FreqQ } else { Drag::Freq }
        } else if sel == Some(k) && near_v.is_some() && (k == 0 || k == 7) {
            Drag::Slope
        } else if sel == Some(k) && near_v.is_some() && near_h {
            Drag::GainQ(near_v.unwrap_or(1.0))
        } else if sel == Some(k) && near_v.is_some() {
            Drag::Q(near_v.unwrap_or(1.0))
        } else if sel == Some(k) && near_h && (1..=6).contains(&k) {
            Drag::Gain
        } else {
            Drag::FreqGain
        }
    };
    if let (Some(k), Some(pt)) = (hovered, pointer) {
        let icon = match drag.map(|d| d.mode).unwrap_or_else(|| mode_at(k, pt, sel)) {
            Drag::Q(_) | Drag::Freq => CursorIcon::ResizeHorizontal,
            Drag::Gain | Drag::Slope => CursorIcon::ResizeVertical,
            Drag::GainQ(_) | Drag::FreqGain => CursorIcon::Move,
            Drag::FreqQ => CursorIcon::Crosshair,
        };
        ui.ctx().set_cursor_icon(icon);
    }
    if dresp.clicked() {
        sel = hovered;
    }
    if dresp.drag_started()
        && let (Some(k), Some(pt)) = (hovered, dresp.interact_pointer_pos())
    {
        let mode = mode_at(k, pt, sel);
        sel = Some(k);
        let (_, f, gn, q) = band(k);
        drag = Some(DragState { band: k, mode, start: pt, f, g: gn, q });
    }
    if let (Some(d), Some(pt)) = (drag, dresp.interact_pointer_pos())
        && dresp.dragged()
    {
        let mut dv = pt - d.start;
        if mods.command && !mods.alt {
            // Command: one axis
            if dv.x.abs() > dv.y.abs() { dv.y = 0.0 } else { dv.x = 0.0 }
        }
        let b = 4 * d.band as u32;
        let px_oct = disp.width() / (1000f32.log2());
        let freq = (d.f * 2f32.powf(dv.x / px_oct)).clamp(20.0, 20000.0);
        let gain = (y_gain(&geo, gain_y(&geo, d.g, scale) + dv.y, scale)).clamp(-24.0, 24.0);
        let qmul = |s: f32| 2f32.powf(-dv.x * s / 40.0);
        let cut = d.band == 0 || d.band == 7;
        match d.mode {
            Drag::FreqGain => {
                set.push((b + 1, freq));
                if !cut {
                    set.push((b + 2, gain));
                }
            }
            Drag::Freq => set.push((b + 1, freq)),
            Drag::FreqQ => {
                set.push((b + 1, freq));
                set.push((b + 3, (d.q * 2f32.powf(-dv.y / 40.0)).clamp(0.1, 100.0)));
            }
            Drag::Gain => set.push((b + 2, gain)),
            Drag::Q(side) => set.push((b + 3, (d.q * qmul(side)).clamp(0.1, 100.0))),
            Drag::GainQ(side) => {
                set.push((b + 2, gain));
                set.push((b + 3, (d.q * qmul(side)).clamp(0.1, 100.0)));
            }
            Drag::Slope => set.push((b + 2, (d.g - dv.y / 12.0).round().clamp(1.0, 8.0))),
        }
    }
    if dresp.drag_stopped() {
        drag = None;
    }
    // the wheel over the selected band: Q
    if let (Some(s), true) = (sel, dresp.hovered()) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let q = band(s).3;
            set.push((4 * s as u32 + 3, (q * 2f32.powf(dy / 120.0)).clamp(0.1, 100.0)));
        }
    }
    dresp.context_menu(|ui| {
        ui.menu_button("EQ dB Scale Mode", |ui| scale_menu(ui, set));
        if ui.add(egui::Button::selectable(vis_master, "Visualize Master Gain")).clicked() {
            set.push((VIS_MASTER, if vis_master { 0.0 } else { 1.0 }));
            ui.close();
        }
    });
    ui.data_mut(|d| d.insert_temp(state_id, (sel, drag)));
    // the band buttons (type icons): on / off
    for k in 0..8 {
        let (on, ..) = band(k);
        let c = pos2(geo.col(k), geo.row(18.0));
        let r = Rect::from_center_size(c, vec2(60.0, 22.0));
        let resp = ui.interact(r, id.with(("bandbtn", k)), Sense::click()).on_hover_text(if on { "Turn the band off" } else { "Turn the band on" });
        band_icon(&p, k, c, if resp.hovered() { color(k, true) } else { color(k, on) });
        if resp.clicked() {
            set.push((4 * k as u32, if on { 0.0 } else { 1.0 }));
        }
    }
    // the fields: Frequency, Gain (Slope), Q under each band
    for k in 0..8 {
        let (on, f, gn, q) = band(k);
        let hot = lit == Some(k);
        let c = if hot { color(k, true) } else { color(k, on) };
        let x = geo.col(k);
        let b = 4 * k as u32;
        let cut = k == 0 || k == 7;
        let rows: [(f32, String, &str, u32); 3] = [
            (314.0, fmt_freq(f), "Hz", b + 1),
            (335.0, if cut { format!("{:.0}", gn * 6.0) } else { format!("{gn:.1}") }, if cut { "dB/Oct" } else { "dB" }, b + 2),
            (356.0, format!("{q:.2}"), "", b + 3),
        ];
        for (y, text, unit, pid) in rows {
            let r = Rect::from_center_size(pos2(x, geo.row(y)), vec2(100.0, 20.0));
            field(ui, &p, r, id.with(("field", pid)), &text, unit, c, pid, k, f, gn, q, set);
        }
    }
    // the master Gain slider at the right
    let mx = body.max.x - 32.0;
    let mg = g(32);
    p.line_segment([pos2(mx, disp.min.y), pos2(mx, disp.max.y)], Stroke::new(1.0, lab.gamma_multiply(0.6)));
    for t in [disp.min.y, disp.max.y] {
        p.line_segment([pos2(mx - 13.0, t), pos2(mx - 8.0, t)], Stroke::new(1.0, lab));
        p.line_segment([pos2(mx + 8.0, t), pos2(mx + 13.0, t)], Stroke::new(1.0, lab));
    }
    let ky = disp.center().y - mg / 24.0 * disp.height() / 2.0;
    p.circle_filled(pos2(mx, ky), 8.0, Color32::from_rgb(40, 58, 82));
    p.circle_stroke(pos2(mx, ky), 8.0, Stroke::new(1.5, Color32::from_rgb(170, 190, 215)));
    p.circle_filled(pos2(mx, ky), 1.5, Color32::from_rgb(170, 190, 215));
    let gresp = ui.interact(Rect::from_center_size(pos2(mx, disp.center().y), vec2(26.0, disp.height() + 16.0)), id.with("master"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    if gresp.dragged() {
        set.push((32, (mg - gresp.drag_delta().y / (disp.height() / 2.0) * 24.0).clamp(-24.0, 24.0)));
    }
    if gresp.double_clicked() || (gresp.clicked() && mods.alt) {
        set.push((32, 0.0));
    }
    p.text(pos2(mx, geo.row(311.0)), Align2::CENTER_CENTER, "Gain", look::regular(13.0), Color32::from_rgb(170, 182, 198));
    p.text(pos2(mx, geo.row(335.0)), Align2::CENTER_CENTER, format!("{mg:.1} dB"), look::regular(12.0), Color32::from_rgb(170, 182, 198));
    // the buttons: Analyzer (PRE / POST), Q-Couple, HQ, Processing
    let y = geo.row(386.0);
    let btn = |x0: f32, w: f32| Rect::from_min_size(pos2(body.min.x + x0, y - 13.0), vec2(w, 26.0));
    let on_bg = Color32::from_rgb(72, 92, 120);
    let off_bg = Color32::from_rgb(38, 52, 70);
    let txt = Color32::from_rgb(200, 210, 222);
    let ar = btn(37.0, 100.0);
    let aresp = ui.interact(ar, id.with("analyzer"), Sense::click());
    p.rect_filled(ar, 4.0, if analyzer_on { on_bg } else { off_bg });
    p.text(pos2(ar.min.x + 10.0, ar.center().y), Align2::LEFT_CENTER, "Analyzer", look::regular(13.0), txt);
    let post = g(ANALYZER_POST) >= 0.5;
    let pr = Rect::from_min_max(pos2(ar.max.x - 32.0, ar.min.y), pos2(ar.max.x, ar.center().y + 2.0));
    p.text(pr.center(), Align2::CENTER_CENTER, if post { "POST" } else { "PRE" }, look::regular(8.5), txt);
    let presp = ui.interact(pr, id.with("prepost"), Sense::click()).on_hover_text("Analyzer Pre / Post EQ");
    if presp.clicked() {
        set.push((ANALYZER_POST, if post { 0.0 } else { 1.0 }));
    } else if aresp.clicked() {
        set.push((ANALYZER, if analyzer_on { 0.0 } else { 1.0 }));
    }
    aresp.context_menu(|ui| {
        ui.label(egui::RichText::new("Analyzer Mode").color(look::text_dim()));
        for (i, n) in ["Peak", "RMS"].iter().enumerate() {
            if ui.add(egui::Button::selectable(g(ANALYZER_MODE) as usize == i, *n)).clicked() {
                set.push((ANALYZER_MODE, i as f32));
                ui.close();
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Analyzer Resolution").color(look::text_dim()));
        for (i, n) in ["Low (2048 points)", "Medium (4096 points)", "High (8192 points)"].iter().enumerate() {
            if ui.add(egui::Button::selectable(g(ANALYZER_RES) as usize == i, *n)).clicked() {
                set.push((ANALYZER_RES, i as f32));
                ui.close();
            }
        }
    });
    let qr = btn(143.0, 77.0);
    let qc = g(Q_COUPLE) >= 0.5;
    let qresp = ui.interact(qr, id.with("qcouple"), Sense::click());
    p.rect_filled(qr, 4.0, if qc { on_bg } else { off_bg });
    p.text(qr.center(), Align2::CENTER_CENTER, "Q-Couple", look::regular(13.0), txt);
    if qresp.clicked() {
        set.push((Q_COUPLE, if qc { 0.0 } else { 1.0 }));
    }
    qresp.context_menu(|ui| {
        ui.label(egui::RichText::new("Gain-Q Couple Strength").color(look::text_dim()));
        for (i, n) in COUPLINGS.iter().enumerate() {
            if ui.add(egui::Button::selectable(g(Q_STRENGTH) as usize == i, *n)).clicked() {
                set.push((Q_STRENGTH, i as f32));
                ui.close();
            }
        }
    });
    if params.iter().any(|pp| pp.id == HQ) {
        let hr = btn(226.0, 37.0);
        let hq = g(HQ) >= 0.5;
        let hresp = ui.interact(hr, id.with("hq"), Sense::click()).on_hover_text("HQ: two-times oversampling");
        p.rect_filled(hr, 4.0, if hq { on_bg } else { off_bg });
        p.text(hr.center(), Align2::CENTER_CENTER, "HQ", look::regular(13.0), txt);
        if hresp.clicked() {
            set.push((HQ, if hq { 0.0 } else { 1.0 }));
        }
    }
    p.text(pos2(body.min.x + 276.0, y), Align2::LEFT_CENTER, "Processing:", look::regular(13.0), Color32::from_rgb(140, 152, 168));
    let procr = btn(356.0, 108.0);
    let procresp = ui.interact(procr, id.with("proc"), Sense::click());
    p.rect_filled(procr, 4.0, Color32::from_rgb(24, 37, 54));
    let pm = (g(PROCESSING) as usize).min(PROCESSINGS.len() - 1);
    p.text(pos2(procr.min.x + 8.0, y), Align2::LEFT_CENTER, PROCESSINGS[pm], look::regular(13.0), txt);
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(procr.max.x - 12.0, y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_UP_DOWN, 10.0, txt);
    egui::Popup::menu(&procresp).show(|ui| {
        for (i, n) in PROCESSINGS.iter().enumerate() {
            if ui.add(egui::Button::selectable(pm == i, *n)).clicked() {
                set.push((PROCESSING, i as f32));
                ui.close();
            }
        }
    });
}

/// A value field: drag vertically, double-click to type, Option-click for the default.
#[allow(clippy::too_many_arguments)]
fn field(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, text: &str, unit: &str, c: Color32, pid: u32, k: usize, f: f32, g: f32, q: f32, set: &mut Vec<(u32, f32)>) {
    let edit_id = id.with("edit");
    let mut editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
    if let Some(s) = editing.as_mut() {
        let te = ui.put(r, egui::TextEdit::singleline(s).align(egui::Align2::CENTER_CENTER).font(look::regular(13.0)));
        te.request_focus();
        if te.lost_focus() {
            if let Ok(v) = s.trim().trim_end_matches(unit).trim().parse::<f32>() {
                let v = if pid % 4 == 2 && (k == 0 || k == 7) { (v / 6.0).round() } else { v };
                set.push((pid, v));
            }
            editing = None;
        }
        ui.data_mut(|d| match &editing {
            Some(s) => {
                d.insert_temp(edit_id, s.clone());
            }
            None => d.remove::<String>(edit_id),
        });
        return;
    }
    let resp = ui.interact(r, id, Sense::click_and_drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    let w = p.layout_no_wrap(text.to_string(), look::regular(13.5), c).size().x;
    let uw = if unit.is_empty() { 0.0 } else { p.layout_no_wrap(unit.to_string(), look::regular(10.0), c).size().x + 3.0 };
    let x0 = r.center().x - (w + uw) / 2.0;
    p.text(pos2(x0, r.center().y), Align2::LEFT_CENTER, text, look::regular(13.5), c);
    if !unit.is_empty() {
        p.text(pos2(x0 + w + 3.0, r.center().y + 1.5), Align2::LEFT_CENTER, unit, look::regular(10.0), c.gamma_multiply(0.85));
    }
    if resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(edit_id, text.to_string()));
    }
    if resp.dragged() {
        let dy = -resp.drag_delta().y;
        let fine = if ui.input(|i| i.modifiers.shift) { 0.2 } else { 1.0 };
        let v = match pid % 4 {
            1 => (f * 2f32.powf(dy * fine / 60.0)).clamp(20.0, 20000.0),
            2 if k == 0 || k == 7 => (g + dy * fine / 12.0).clamp(0.0, 8.0),
            2 => (g + dy * fine * 0.1).clamp(-24.0, 24.0),
            _ => (q * 2f32.powf(dy * fine / 80.0)).clamp(0.1, 100.0),
        };
        set.push((pid, if pid % 4 == 2 && (k == 0 || k == 7) { v.round() } else { v }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_and_areas() {
        let g = Geo::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(1028.0, BODY_H)));
        // Logic's display: 20 Hz at the left edge, 20 kHz at the right, 1 kHz two thirds in
        assert!((g.x(20.0) - 38.0).abs() < 0.1 && (g.x(20000.0) - 941.0).abs() < 0.1);
        assert!((g.f(g.x(1000.0)) - 1000.0).abs() < 0.5);
        let s = eq_scale(1, 0.0);
        assert_eq!(s.0, 15.0);
        assert!((gain_y(&g, 15.0, s) - g.disp.min.y).abs() < 0.1);
        assert!((y_gain(&g, gain_y(&g, -7.0, s), s) + 7.0).abs() < 1e-3);
        let w = eq_scale(3, 0.0);
        assert!((y_gain(&g, gain_y(&g, 12.0, w), w) - 12.0).abs() < 1e-3);
        // higher Q, narrower area; cuts reach the edge
        let (a1, b1) = area(3, 1000.0, 1.0);
        let (a2, b2) = area(3, 1000.0, 4.0);
        assert!(b2 / a2 < b1 / a1);
        assert_eq!(area(0, 100.0, 0.7).0, 20.0);
        // the band columns: Logic's 76 … 903
        assert!((g.col(0) - 76.0).abs() < 0.5 && (g.col(7) - 903.0).abs() < 0.5);
    }
}
