//! Logic's Noise Gate editor: Gate / Ducker, the Threshold and Reduction knobs with Hysteresis
//! below, the Open / Close lights, the Attack / Hold / Release knobs with Lookahead below, and on
//! the right Monitor and the side-chain filter's High and Low Cutoff sliders.
//!
//! Positions were measured on Logic's own window (screenshots/ui-plugin-window-noise-gate.png, at
//! 75 %): the numbers in `at` calls are that picture's pixels, turned into points by `Geo`.
//! The Characteristics pop-up (Bandpass / Band Reject) and the filter's power button, which dims
//! the cutoff sliders when off, as Logic draws them.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;

use crate::fxviews::Live;
use crate::look;

/// The picture's body (below the header, above the footer), in its pixels.
const X0: f32 = 4.0;
const Y0: f32 = 206.0;
const BODY_W: f32 = 1536.0;
const BODY_H: f32 = 502.0;

/// Logic's window at 100 %: 1028 × 527 points with its header and footer; ours adds egui's title bar.
pub const SIZE: egui::Vec2 = egui::vec2(1014.0, 515.0);

pub(crate) const GREEN: Color32 = Color32::from_rgb(0xb1, 0xfc, 0xb2);
pub(crate) const LABEL: Color32 = Color32::from_rgb(0xb4, 0xbd, 0xca);
pub(crate) const SCALE: Color32 = Color32::from_rgb(0xa6, 0xae, 0xba);
const KNOB: Color32 = Color32::from_rgb(0x39, 0x44, 0x55);

/// A Logic screenshot's pixels → points in the window: `o` is the body's top left, (`x0`, `y0`)
/// the same spot in the picture, `s` points per picture pixel.
pub(crate) struct Geo {
    pub o: Pos2,
    pub s: f32,
    pub x0: f32,
    pub y0: f32,
}

impl Geo {
    pub fn at(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2((x - self.x0) * self.s, (y - self.y0) * self.s)
    }
    pub fn pt(&self, px: f32) -> f32 {
        px * self.s
    }
    pub fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.at(x0, y0), self.at(x1, y1))
    }
}

pub(crate) fn val(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).and_then(|i| values.get(i).copied()).unwrap_or(0.0)
}

pub(crate) fn prm(params: &[Param], id: u32) -> Param {
    params.iter().find(|p| p.id == id).cloned().unwrap_or_else(|| Param::new(id, "", 0.0, 1.0, 0.0, lg_dsp::Unit::None))
}

/// "-50", "3", "10.0", "20000": the number Logic shows for each control.
fn number(id: u32, v: f32) -> String {
    match id {
        1 | 5 | 8 => format!("{v:.1}"),
        _ => format!("{}", v.round() as i64),
    }
}

pub(crate) fn unit(u: lg_dsp::Unit) -> &'static str {
    match u {
        lg_dsp::Unit::Db => "dB",
        lg_dsp::Unit::Ms => "ms",
        lg_dsp::Unit::Hz => "Hz",
        _ => "",
    }
}

/// The value line: the number large, its unit smaller, as Logic sets it.
pub(crate) fn value_text(pt: &egui::Painter, g: &Geo, pos: Pos2, align: Align2, num: &str, unit: &str) {
    let big = pt.layout_no_wrap(num.to_string(), look::regular(g.pt(30.0)), GREEN);
    let small = pt.layout_no_wrap(format!(" {unit}"), look::regular(g.pt(24.0)), GREEN);
    let w = big.size().x + small.size().x;
    let x = match align.x() {
        egui::Align::Min => pos.x,
        egui::Align::Center => pos.x - w / 2.0,
        egui::Align::Max => pos.x - w,
    };
    let base = pos.y + big.size().y / 2.0;
    let (bw, bh, sh) = (big.size().x, big.size().y, small.size().y);
    pt.galley(pos2(x, base - bh), big, GREEN);
    pt.galley(pos2(x + bw, base - sh - g.pt(1.5)), small, GREEN);
}

/// A drag on a knob or slider: up / right raises; Shift is fine; double-click resets.
pub(crate) fn drag(ui: &egui::Ui, resp: &egui::Response, p: &Param, v: f32, span: f32, vertical: bool) -> Option<f32> {
    if resp.double_clicked() {
        return Some(p.default);
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        let mut t = if vertical { -d.y } else { d.x } / span;
        if ui.input(|i| i.modifiers.shift) {
            t *= 0.1;
        }
        let f = ((v - p.min) / (p.max - p.min) + t).clamp(0.0, 1.0);
        return Some(p.min + f * (p.max - p.min));
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn knob(ui: &mut egui::Ui, pt: &egui::Painter, g: &Geo, c: (f32, f32), r: f32, id: egui::Id, p: &Param, v: f32, marks: (&str, &str)) -> Option<f32> {
    const START: f32 = std::f32::consts::PI * 0.75;
    const SWEEP: f32 = std::f32::consts::PI * 1.5;
    let c = g.at(c.0, c.1);
    let rr = g.pt(r);
    let f = ((v - p.min) / (p.max - p.min)).clamp(0.0, 1.0);
    pt.circle_filled(c, rr, KNOB);
    pt.circle_stroke(c, rr, Stroke::new(g.pt(3.0), Color32::from_gray(0x0c)));
    // the value arc, from the low end
    let n = 48;
    let pts: Vec<Pos2> = (0..=n).map(|k| {
        let a = START + SWEEP * f * k as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * (rr - g.pt(1.5))
    }).collect();
    if f > 0.005 {
        pt.add(egui::Shape::line(pts, Stroke::new(g.pt(6.0), GREEN)));
    }
    let a = START + SWEEP * f;
    let dir = vec2(a.cos(), a.sin());
    pt.line_segment([c + dir * g.pt(2.0), c + dir * (rr - g.pt(6.0))], Stroke::new(g.pt(3.0), GREEN));
    // the ends of the scale
    let (lo, hi) = marks;
    pt.text(c + vec2(-rr * 0.52, rr + g.pt(16.0)), Align2::CENTER_CENTER, lo, look::regular(g.pt(16.0)), SCALE);
    pt.text(c + vec2(rr * 0.52, rr + g.pt(16.0)), Align2::CENTER_CENTER, hi, look::regular(g.pt(16.0)), SCALE);
    let resp = ui.interact(Rect::from_center_size(c, vec2(rr * 2.0, rr * 2.0)), id, Sense::click_and_drag()).on_hover_text(format!("{}: {}", p.name, number(p.id, v)));
    drag(ui, &resp, p, v, 220.0, true)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn slider(ui: &mut egui::Ui, pt: &egui::Painter, g: &Geo, x0: f32, x1: f32, y: f32, id: egui::Id, p: &Param, v: f32, log: bool) -> Option<f32> {
    let frac = |v: f32| if log { ((v.max(p.min) / p.min).ln() / (p.max / p.min).ln()).clamp(0.0, 1.0) } else { ((v - p.min) / (p.max - p.min)).clamp(0.0, 1.0) };
    let f = frac(v);
    let (a, b) = (g.at(x0, y), g.at(x1, y));
    pt.line_segment([a, b], Stroke::new(g.pt(4.0), Color32::from_gray(0x23)));
    let x = a.x + (b.x - a.x) * f;
    pt.line_segment([a, pos2(x, a.y)], Stroke::new(g.pt(6.0), GREEN.gamma_multiply(0.85)));
    let thumb = Rect::from_center_size(pos2(x, a.y), vec2(g.pt(14.0), g.pt(36.0)));
    pt.rect_filled(thumb, g.pt(2.0), Color32::from_rgb(0x46, 0x55, 0x6a));
    pt.line_segment([pos2(x, thumb.min.y + g.pt(4.0)), pos2(x, thumb.max.y - g.pt(4.0))], Stroke::new(g.pt(2.0), GREEN));
    let area = Rect::from_min_max(pos2(a.x - g.pt(8.0), a.y - g.pt(20.0)), pos2(b.x + g.pt(8.0), a.y + g.pt(20.0)));
    let resp = ui.interact(area, id, Sense::click_and_drag()).on_hover_text(format!("{}: {}", p.name, number(p.id, v)));
    if resp.double_clicked() {
        return Some(p.default);
    }
    if resp.dragged() || resp.clicked() {
        let px = resp.interact_pointer_pos()?.x;
        let f = ((px - a.x) / (b.x - a.x)).clamp(0.0, 1.0);
        return Some(if log { p.min * (p.max / p.min).powf(f) } else { p.min + f * (p.max - p.min) });
    }
    None
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, slot: (usize, usize), set: &mut Vec<(u32, f32)>) {
    let get = |id: u32, set: &Vec<(u32, f32)>| set.iter().rev().find(|x| x.0 == id).map_or(val(params, values, id), |x| x.1);
    let margin = ui.spacing().window_margin;
    let w = ui.available_width() + (margin.left + margin.right) as f32;
    let s = w / BODY_W;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), BODY_H * s), Sense::hover());
    let body = Rect::from_min_max(pos2(r.min.x - margin.left as f32, r.min.y), pos2(r.max.x + margin.right as f32, r.max.y));
    let g = Geo { o: body.min, s, x0: X0, y0: Y0 };
    let pt = ui.painter().with_clip_rect(body);
    let id = egui::Id::unique(("noise-gate-view", slot));
    // the panel: Logic's blue, darker at the bottom
    let mut mesh = egui::Mesh::default();
    let (top, bot) = (Color32::from_rgb(0x2e, 0x41, 0x59), Color32::from_rgb(0x21, 0x2c, 0x40));
    mesh.colored_vertex(body.left_top(), top);
    mesh.colored_vertex(body.right_top(), top);
    mesh.colored_vertex(body.right_bottom(), bot);
    mesh.colored_vertex(body.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    pt.add(mesh);
    for x in [473.0, 1089.0] {
        pt.line_segment([g.at(x, 230.0), g.at(x, 680.0)], Stroke::new(g.pt(2.0), Color32::from_white_alpha(10)));
    }
    let label = |t: &str, x: f32, y: f32, align: Align2| {
        pt.text(g.at(x, y), align, t, look::regular(g.pt(22.0)), LABEL);
    };

    // Gate | Ducker
    let ducker = get(1000, set) >= 0.5;
    let seg = g.rect(134.0, 230.0, 344.0, 270.0);
    pt.rect_filled(seg, g.pt(5.0), Color32::from_rgb(0x37, 0x47, 0x5e));
    for (k, (n, x0, x1)) in [("Gate", 134.0, 238.0), ("Ducker", 238.0, 344.0)].into_iter().enumerate() {
        let b = g.rect(x0, 230.0, x1, 270.0);
        let on = ducker == (k == 1);
        if on {
            pt.rect_filled(b, g.pt(5.0), Color32::from_rgb(0x1f, 0x2d, 0x3d));
        }
        pt.text(b.center(), Align2::CENTER_CENTER, n, look::regular(g.pt(22.0)), if on { GREEN } else { LABEL });
        if ui.interact(b, id.with(("mode", k)), Sense::click()).clicked() {
            set.push((1000, k as f32));
        }
    }

    // the knobs: Threshold, Reduction · Attack, Hold, Release
    let knobs: [(u32, &str, (f32, f32), f32, (f32, f32)); 5] = [
        (0, "Threshold", (130.0, 449.0), 62.0, (323.0, 355.0)),
        (2, "Reduction", (347.0, 449.0), 62.0, (323.0, 355.0)),
        (3, "Attack", (596.0, 437.0), 50.0, (321.0, 355.0)),
        (4, "Hold", (784.0, 437.0), 50.0, (321.0, 355.0)),
        (5, "Release", (970.0, 437.0), 50.0, (321.0, 355.0)),
    ];
    for (pid, name, c, rad, (ly, vy)) in knobs {
        let p = prm(params, pid);
        let v = get(pid, set);
        label(name, c.0, ly, Align2::CENTER_CENTER);
        value_text(&pt, &g, g.at(c.0, vy), Align2::CENTER_CENTER, &number(pid, v), unit(p.unit));
        let ends = (number(pid, p.min), if p.max >= 1000.0 { format!("{} k", (p.max / 1000.0).round()) } else { number(pid, p.max) });
        if let Some(nv) = knob(ui, &pt, &g, c, rad, id.with(pid), &p, v, (&ends.0, &ends.1)) {
            set.push((pid, nv));
        }
    }

    // Open / Close: whether the gate is open now
    let open = live.get("Open").unwrap_or(0.0) > 0.5;
    label("Open", 705.0, 251.0, Align2::CENTER_CENTER);
    label("Close", 830.0, 251.0, Align2::CENTER_CENTER);
    pt.circle_filled(g.at(750.0, 251.0), g.pt(6.0), if open { GREEN } else { Color32::from_gray(0x10) });
    pt.circle_filled(g.at(876.0, 251.0), g.pt(6.0), if open { Color32::from_gray(0x10) } else { GREEN });

    // the sliders: Hysteresis, Lookahead, High Cutoff, Low Cutoff
    let sliders: [(u32, &str, f32, f32, f32, bool); 4] = [
        (1, "Hysteresis", 68.0, 375.0, 596.0, false),
        (8, "Lookahead", 628.0, 936.0, 596.0, false),
        (6, "High Cutoff", 1179.0, 1486.0, 469.0, true),
        (7, "Low Cutoff", 1179.0, 1486.0, 596.0, true),
    ];
    for (pid, name, x0, x1, y, log) in sliders {
        let p = prm(params, pid);
        let v = get(pid, set);
        label(name, x0, y, Align2::LEFT_CENTER);
        value_text(&pt, &g, g.at(x1, y - 3.0), Align2::RIGHT_CENTER, &number(pid, v), unit(p.unit));
        if let Some(nv) = slider(ui, &pt, &g, x0 + 2.0, x1 - 3.0, y + 40.0, id.with(("sl", pid)), &p, v, log) {
            set.push((pid, nv));
        }
    }

    // Characteristics: Bandpass / Band Reject (a click toggles, as the two-item pop-up does)
    let reject = get(1002, set) >= 0.5;
    let filter_on = get(1001, set) >= 0.5;
    label("Characteristics", 1179.0, 322.0, Align2::LEFT_CENTER);
    let ch = if reject { "Band Reject ⇕" } else { "Bandpass ⇕" };
    let cr = Rect::from_min_size(g.at(1179.0, 336.0), vec2(g.pt(200.0), g.pt(40.0)));
    pt.text(pos2(cr.min.x, cr.center().y), Align2::LEFT_CENTER, ch, look::regular(g.pt(30.0)), if filter_on { GREEN } else { GREEN.gamma_multiply(0.5) });
    let cresp = ui.interact(cr, id.with("character"), Sense::click()).on_hover_text("Characteristics");
    egui::Popup::menu(&cresp).show(|ui| {
        for (k, n) in ["Bandpass", "Band Reject"].into_iter().enumerate() {
            if ui.add(egui::Button::selectable(reject == (k == 1), n)).clicked() {
                set.push((1002, k as f32));
                ui.close();
            }
        }
    });
    // (filter off: the cutoffs are dimmed, as Logic's)
    if !filter_on {
        pt.rect_filled(g.rect(1170.0, 445.0, 1500.0, 665.0), 0.0, Color32::from_rgba_unmultiplied(0x27, 0x36, 0x4c, 150));
    }
    // the filter's power button, with the bracket joining the two cutoffs
    let c = g.at(1139.0, 573.0);
    let col = if filter_on { GREEN } else { LABEL };
    pt.line_segment([g.at(1139.0, 510.0), g.at(1139.0, 548.0)], Stroke::new(g.pt(2.0), LABEL.gamma_multiply(0.5)));
    pt.line_segment([g.at(1139.0, 598.0), g.at(1139.0, 636.0)], Stroke::new(g.pt(2.0), LABEL.gamma_multiply(0.5)));
    pt.line_segment([g.at(1139.0, 510.0), g.at(1155.0, 510.0)], Stroke::new(g.pt(2.0), LABEL.gamma_multiply(0.5)));
    pt.line_segment([g.at(1139.0, 636.0), g.at(1155.0, 636.0)], Stroke::new(g.pt(2.0), LABEL.gamma_multiply(0.5)));
    crate::glyphs::paint(&pt, Rect::from_center_size(c, vec2(g.pt(30.0), g.pt(30.0))), crate::glyphs::ph::POWER, g.pt(26.0), col);
    if ui.interact(Rect::from_center_size(c, vec2(g.pt(36.0), g.pt(36.0))), id.with("filter"), Sense::click()).on_hover_text("Filter").clicked() {
        set.push((1001, if filter_on { 0.0 } else { 1.0 }));
    }

    // Monitor: the filtered key alone
    label("Monitor", 1454.0, 322.0, Align2::CENTER_CENTER);
    let mon = get(9, set) >= 0.5;
    let b = g.rect(1426.0, 343.0, 1480.0, 383.0);
    pt.rect_filled(b, g.pt(4.0), if mon { Color32::from_rgb(0x2a, 0x4a, 0x3a) } else { Color32::from_rgb(0x3a, 0x48, 0x5c) });
    pt.text(b.center(), Align2::CENTER_CENTER, if mon { "ON" } else { "OFF" }, look::regular(g.pt(20.0)), if mon { GREEN } else { LABEL });
    if ui.interact(b, id.with("monitor"), Sense::click()).clicked() {
        set.push((9, if mon { 0.0 } else { 1.0 }));
    }
}
