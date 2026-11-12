//! The metering plug-ins' windows, as Logic's: Level Meter (a bar per channel, blue, yellow
//! over the target, red over 0 dB; RMS dark under the light peak; peak-hold numbers coloured
//! by level, a falling peak line; the target handle, separate ones for C and LFE in surround;
//! the Mode pop-up), Loudness Meter (M / S / I bars, the draggable target line, LU Range and
//! Integrated, Start / Pause, Reset), Correlation Meter (the momentary white line, the recent
//! range bar, peak divergence below 0, Reaction behind the lower-left triangle), MultiMeter and
//! its surround version (Analyzer / Goniometer / Balance in the main display, Loudness and
//! Level meters beside it, the correlation meter under it, the mode's parameters, the Peak
//! parameters, and the View pop-up: Full, Meters, Display / Compact), BPM Counter (the LED,
//! :2 and x2) and Tuner. Each window scales with its size (drag a lower corner); the
//! Horizontal / Vertical choice is the window's Orientation pop-up.
//!
//! Logic's metering windows aren't in the research's screenshots: the look follows the guide's
//! descriptions and Logic's dark plug-in style.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_dsp::surround::Ch;
use lg_fx::meters::mm;

use crate::fxviews::Live;
use crate::look;

const BG: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x1e);
const DISPLAY: Color32 = Color32::from_rgb(0x0f, 0x11, 0x14);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x9c, 0xf6);
const DARK_BLUE: Color32 = Color32::from_rgb(0x1e, 0x5c, 0xb0);
const DIM_BLUE: Color32 = Color32::from_rgb(0x2a, 0x4c, 0x78);
const YELLOW: Color32 = Color32::from_rgb(0xf2, 0xc6, 0x1f);
const RED: Color32 = Color32::from_rgb(0xff, 0x3b, 0x30);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x8a, 0x1e);
const GREEN: Color32 = Color32::from_rgb(0x46, 0xd0, 0x5a);
const TEXT: Color32 = Color32::from_rgb(0xd0, 0xd2, 0xd6);
const DIM: Color32 = Color32::from_rgb(0x8c, 0x90, 0x96);
const GRID: Color32 = Color32::from_rgb(0x2a, 0x2e, 0x34);

type Set = Vec<(u32, f32)>;

struct Ctx<'a> {
    params: &'a [Param],
    values: &'a [f32],
    live: &'a Live,
    id: egui::Id,
}

impl Ctx<'_> {
    fn get(&self, set: &Set, id: u32) -> f32 {
        set.iter().rev().find(|x| x.0 == id).map_or_else(|| self.params.iter().position(|p| p.id == id).map_or(0.0, |i| self.values[i]), |x| x.1)
    }
    fn geti(&self, set: &Set, id: u32) -> usize {
        self.get(set, id).round().max(0.0) as usize
    }
    fn prm(&self, id: u32) -> Option<&Param> {
        self.params.iter().find(|p| p.id == id)
    }
    fn r(&self, n: &str) -> f32 {
        self.live.get(n).unwrap_or(-200.0)
    }
    fn rk(&self, kind: &str, k: usize) -> f32 {
        self.live.readouts.iter().find(|r| r.0.strip_prefix(kind).and_then(|x| x.strip_prefix(' ')).and_then(|x| x.parse::<usize>().ok()) == Some(k)).map_or(-200.0, |r| r.1)
    }
    fn channels(&self) -> Vec<Ch> {
        let n = self.live.get("Channels").unwrap_or(2.0) as usize;
        (0..n).map(|k| Ch::ALL[(self.rk("Ch", k).max(0.0) as usize).min(16)]).collect()
    }
}

/// The level scale: 0 at the top (+6 dB) to 1 at the bottom (-60 dB), Logic's meters' steps.
fn frac(db: f32) -> f32 {
    const M: [f32; 12] = [6.0, 3.0, 0.0, -3.0, -6.0, -10.0, -15.0, -20.0, -30.0, -40.0, -50.0, -60.0];
    if db >= M[0] {
        return 0.0;
    }
    for k in 0..M.len() - 1 {
        if db >= M[k + 1] {
            return (k as f32 + (M[k] - db) / (M[k] - M[k + 1])) / (M.len() - 1) as f32;
        }
    }
    1.0
}
const MARKS: [f32; 10] = [6.0, 0.0, -6.0, -10.0, -15.0, -20.0, -30.0, -40.0, -50.0, -60.0];

/// The colour a level shows in (blue, yellow over the target, red over 0 dB).
fn level_col(db: f32, target: f32) -> Color32 {
    if db > 0.0 {
        RED
    } else if db > target {
        YELLOW
    } else {
        BLUE
    }
}

fn db_txt(v: f32) -> String {
    if v <= -119.0 { "-∞".into() } else { format!("{v:.1}") }
}

/// A small push button in the metering style.
fn btn(ui: &mut egui::Ui, r: Rect, id: egui::Id, label: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 3.0, if on { Color32::from_rgb(0x2b, 0x6c, 0xc4) } else if resp.hovered() { Color32::from_gray(0x48) } else { Color32::from_gray(0x36) });
    p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(11.0), if on { Color32::WHITE } else { TEXT });
    resp
}

/// A row of buttons choosing a menu parameter.
fn choice(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, pid: u32, labels: &[&str]) {
    let cur = c.geti(set, pid);
    let w = r.width() / labels.len() as f32;
    for (k, l) in labels.iter().enumerate() {
        let rr = Rect::from_min_size(pos2(r.min.x + w * k as f32, r.min.y), vec2(w - 2.0, r.height()));
        if btn(ui, rr, c.id.with(("choice", pid, k)), l, k == cur).clicked() && k != cur {
            set.push((pid, k as f32));
        }
    }
}

/// A pop-up menu for a menu parameter, with a caption before it.
fn popup(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, pid: u32, caption: &str) {
    if c.prm(pid).is_none() {
        return;
    }
    let cur = c.geti(set, pid);
    let p = ui.painter();
    if !caption.is_empty() {
        p.text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, caption, look::regular(11.0), DIM);
    }
    let cw = if caption.is_empty() { 0.0 } else { caption.chars().count() as f32 * 6.0 + 6.0 };
    let br = Rect::from_min_max(pos2(r.min.x + cw, r.min.y), r.max);
    let resp = ui.interact(br, c.id.with(("popup", pid)), Sense::click());
    ui.painter().rect_filled(br, 3.0, if resp.hovered() { Color32::from_gray(0x44) } else { Color32::from_gray(0x30) });
    let choices = c.prm(pid).map(|p| p.choices).unwrap_or(&[]);
    ui.painter().text(br.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, choices.get(cur).copied().unwrap_or(""), look::regular(11.0), TEXT);
    ui.painter().text(br.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, "⌄", look::regular(10.0), DIM);
    egui::Popup::menu(&resp).show(|ui| {
        for (k, ch) in choices.iter().enumerate() {
            if ui.add(egui::Button::selectable(k == cur, *ch)).clicked() {
                set.push((pid, k as f32));
                ui.close();
            }
        }
    });
}

/// A value field changed by dragging vertically (Logic's numeric fields).
fn field(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, pid: u32, caption: &str, unit: &str, step: f32) {
    let Some(p) = c.prm(pid).cloned() else { return };
    let v = c.get(set, pid);
    ui.painter().text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, caption, look::regular(11.0), DIM);
    let fr = Rect::from_min_max(pos2(r.max.x - 58.0, r.min.y), r.max);
    let resp = ui.interact(fr, c.id.with(("field", pid)), Sense::click_and_drag());
    ui.painter().rect_filled(fr, 3.0, DISPLAY);
    ui.painter().text(fr.center(), Align2::CENTER_CENTER, format!("{v:.0} {unit}"), look::regular(11.0), TEXT);
    if resp.dragged() {
        let nv = (v - resp.drag_delta().y * step).clamp(p.min, p.max);
        if nv != v {
            set.push((pid, nv));
        }
    }
    if resp.double_clicked() {
        set.push((pid, p.default));
    }
}

fn bump(c: &Ctx, set: &mut Set, pid: u32) {
    let v = c.get(set, pid) + 1.0;
    set.push((pid, v));
}

/// Vertical or horizontal level bars for the given channels, the peak numbers above them
/// (click: reset), the targets (dragged); `rms_too` draws RMS dark under the peak.
#[allow(clippy::too_many_arguments)]
fn level_bars(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, chans: &[Ch], ch_kind: usize, targets: (u32, u32, u32), reset_id: u32, horizontal: bool) {
    let p = ui.painter().clone();
    p.rect_filled(r, 3.0, DISPLAY);
    let n = chans.len().max(1);
    let lab = 16.0;
    let num = 16.0;
    // the scale
    let area = if horizontal { Rect::from_min_max(pos2(r.min.x + 30.0, r.min.y + 6.0), pos2(r.max.x - 40.0, r.max.y - 18.0)) } else { Rect::from_min_max(pos2(r.min.x + 28.0, r.min.y + num + 4.0), pos2(r.max.x - 6.0, r.max.y - lab)) };
    let pos = |db: f32| -> f32 { if horizontal { area.max.x - area.width() * frac(db) } else { area.min.y + area.height() * frac(db) } };
    for m in MARKS {
        let t = if m > 0.0 { format!("+{m:.0}") } else { format!("{m:.0}") };
        if horizontal {
            p.text(pos2(pos(m), area.max.y + 9.0), Align2::CENTER_CENTER, t, look::regular(9.5), DIM);
            p.line_segment([pos2(pos(m), area.min.y), pos2(pos(m), area.max.y)], Stroke::new(0.5, GRID));
        } else {
            p.text(pos2(area.min.x - 4.0, pos(m)), Align2::RIGHT_CENTER, t, look::regular(9.5), DIM);
            p.line_segment([pos2(area.min.x, pos(m)), pos2(area.max.x, pos(m))], Stroke::new(0.5, GRID));
        }
    }
    let tgt_all = c.get(set, targets.0);
    let target_of = |ch: Ch| -> f32 {
        match ch {
            Ch::C if targets.1 != u32::MAX => c.get(set, targets.1),
            Ch::Lfe if targets.2 != u32::MAX => c.get(set, targets.2),
            _ => tgt_all,
        }
    };
    let span = if horizontal { area.height() } else { area.width() };
    let w = span / n as f32;
    let mut reset = false;
    for (k, ch) in chans.iter().enumerate() {
        let a = if horizontal { area.min.y } else { area.min.x } + w * k as f32 + w * 0.12;
        let b = a + w * 0.76;
        let tgt = target_of(*ch);
        let peak = if matches!(ch_kind, 3 | 4) { c.rk("TP", k) } else { c.rk("Peak", k) };
        let rms = c.rk("RMS", k);
        let bar = |from: f32, to: f32| -> Rect {
            if horizontal { Rect::from_min_max(pos2(pos(to), a), pos2(pos(from), b)) } else { Rect::from_min_max(pos2(a, pos(to)), pos2(b, pos(from))) }
        };
        // peak (light) over RMS (dark), each split at the target and 0 dB
        let draw = |v: f32, light: bool| {
            if v <= -60.0 {
                return;
            }
            let segs = [(-60.0, tgt.min(v), if light { BLUE } else { DARK_BLUE }), (tgt, 0f32.min(v), if light { YELLOW } else { Color32::from_rgb(0xa8, 0x8a, 0x16) }), (0.0, v, if light { RED } else { Color32::from_rgb(0xb0, 0x28, 0x22) })];
            for (lo, hi, col) in segs {
                if hi > lo {
                    p.rect_filled(bar(lo, hi.min(6.0)), 0.0, col);
                }
            }
        };
        match ch_kind {
            1 => draw(rms, true),
            0 | 3 => draw(peak, true),
            _ => {
                draw(peak, true);
                draw(rms, false);
            }
        }
        // the peak line: shows at the highest peak, then falls
        let seg = if ch_kind == 1 { c.rk("SegRMS", k) } else { c.rk("Seg", k) };
        if seg > -60.0 {
            let y = pos(seg.min(6.0));
            let s = if horizontal { [pos2(y, a), pos2(y, b)] } else { [pos2(a, y), pos2(b, y)] };
            p.line_segment(s, Stroke::new(2.0, level_col(seg, tgt)));
        }
        // the channel label and the peak-hold number (coloured by level)
        let mid = (a + b) / 2.0;
        let hold = c.rk("Hold", k);
        if horizontal {
            p.text(pos2(r.min.x + 4.0, mid), Align2::LEFT_CENTER, ch.label(), look::regular(9.5), DIM);
            let nr = Rect::from_center_size(pos2(r.max.x - 20.0, mid), vec2(38.0, (b - a).max(12.0)));
            p.text(nr.center(), Align2::CENTER_CENTER, db_txt(hold), look::regular(10.0), if hold > -119.0 { level_col(hold, tgt) } else { DIM });
            reset |= ui.interact(nr, c.id.with(("hold", k)), Sense::click()).clicked();
        } else {
            p.text(pos2(mid, r.max.y - lab / 2.0), Align2::CENTER_CENTER, ch.label(), look::regular(9.5), DIM);
            let nr = Rect::from_center_size(pos2(mid, r.min.y + num / 2.0 + 2.0), vec2(w.max(30.0), num));
            p.text(nr.center(), Align2::CENTER_CENTER, db_txt(hold), look::regular(10.0), if hold > -119.0 { level_col(hold, tgt) } else { DIM });
            reset |= ui.interact(nr, c.id.with(("hold", k)), Sense::click()).on_hover_text("Peak hold (click to reset)").clicked();
        }
    }
    // the target handles: one for every channel but C and LFE, which have their own
    let mut handles = vec![(targets.0, tgt_all, "")];
    if chans.contains(&Ch::C) && targets.1 != u32::MAX {
        handles.push((targets.1, c.get(set, targets.1), "C"));
    }
    if chans.contains(&Ch::Lfe) && targets.2 != u32::MAX {
        handles.push((targets.2, c.get(set, targets.2), "LFE"));
    }
    for (k, (pid, v, name)) in handles.iter().enumerate() {
        let at = pos(*v);
        let hr = if horizontal { Rect::from_center_size(pos2(at, area.min.y - 3.0 + 0.0 * k as f32), vec2(10.0, 8.0)) } else { Rect::from_center_size(pos2(area.max.x + 0.0 - 6.0 * k as f32, at), vec2(10.0, 10.0)) };
        let hr = if horizontal { hr } else { hr.translate(vec2(2.0, 0.0)) };
        let resp = ui.interact(hr.expand(3.0), c.id.with(("target", pid)), Sense::drag()).on_hover_text(format!("{} target: {v:.1} dB", if name.is_empty() { "Level" } else { name }));
        let tri = if horizontal { vec![pos2(at - 4.0, hr.min.y), pos2(at + 4.0, hr.min.y), pos2(at, hr.max.y)] } else { vec![pos2(hr.max.x, at - 4.0), pos2(hr.max.x, at + 4.0), pos2(hr.min.x, at)] };
        p.add(egui::Shape::convex_polygon(tri, YELLOW, Stroke::NONE));
        if resp.dragged() {
            let d = if horizontal { resp.drag_delta().x } else { -resp.drag_delta().y };
            let span = if horizontal { area.width() } else { area.height() };
            let nv = (v + d / span * 66.0).clamp(-30.0, 0.0);
            set.push((*pid, nv));
        }
    }
    if reset {
        bump(c, set, reset_id);
    }
}

/// Loudness bars (M, S, I or just M for the MultiMeters) with the target line.
fn loudness_bars(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, which: &[(&str, &str)], target_id: u32) {
    let p = ui.painter().clone();
    p.rect_filled(r, 3.0, DISPLAY);
    let area = Rect::from_min_max(pos2(r.min.x + 26.0, r.min.y + 8.0), pos2(r.max.x - 6.0, r.max.y - 16.0));
    // LUFS from -60 to 0, linear (1 LU = 1 dB)
    let y_of = |v: f32| area.max.y - area.height() * ((v + 60.0) / 60.0).clamp(0.0, 1.0);
    for m in (0..=6).map(|k| -10.0 * k as f32) {
        p.text(pos2(area.min.x - 4.0, y_of(m)), Align2::RIGHT_CENTER, format!("{m:.0}"), look::regular(9.5), DIM);
        p.line_segment([pos2(area.min.x, y_of(m)), pos2(area.max.x, y_of(m))], Stroke::new(0.5, GRID));
    }
    let target = c.get(set, target_id);
    let w = area.width() / which.len() as f32;
    for (k, (name, label)) in which.iter().enumerate() {
        let v = c.r(name);
        let x0 = area.min.x + w * k as f32 + w * 0.15;
        let x1 = x0 + w * 0.7;
        if v > -60.0 {
            p.rect_filled(Rect::from_min_max(pos2(x0, y_of(target.min(v))), pos2(x1, area.max.y)), 0.0, BLUE);
            if v > target {
                p.rect_filled(Rect::from_min_max(pos2(x0, y_of(v)), pos2(x1, y_of(target))), 0.0, YELLOW);
            }
        }
        p.text(pos2((x0 + x1) / 2.0, r.max.y - 8.0), Align2::CENTER_CENTER, *label, look::regular(9.5), DIM);
    }
    // the yellow target line (drag it)
    let y = y_of(target);
    p.line_segment([pos2(area.min.x, y), pos2(area.max.x, y)], Stroke::new(1.5, YELLOW));
    let resp = ui.interact(Rect::from_min_max(pos2(area.min.x, y - 4.0), pos2(area.max.x, y + 4.0)), c.id.with(("loud-target", target_id)), Sense::drag()).on_hover_text(format!("Target: {target:.1} LUFS"));
    if resp.dragged() {
        set.push((target_id, (target - resp.drag_delta().y / area.height() * 60.0).clamp(-30.0, 0.0)));
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
}

/// The correlation meter: -1 (red) … 0 … +1 (green), the momentary white line, the recent
/// range (yellow to green) and the peak divergence below 0 (orange to red).
fn correlation(ui: &mut egui::Ui, c: &Ctx, r: Rect, value: &str, min: &str, range: bool, vertical: bool) -> bool {
    let p = ui.painter().clone();
    p.rect_filled(r, 3.0, DISPLAY);
    let inner = r.shrink2(if vertical { vec2(6.0, 16.0) } else { vec2(16.0, 6.0) });
    let at = |v: f32| -> f32 {
        let f = (v.clamp(-1.0, 1.0) + 1.0) / 2.0;
        if vertical { inner.max.y - inner.height() * f } else { inner.min.x + inner.width() * f }
    };
    // the coloured track
    let n = 40;
    for k in 0..n {
        let v0 = -1.0 + 2.0 * k as f32 / n as f32;
        let v1 = -1.0 + 2.0 * (k + 1) as f32 / n as f32;
        let col = if v0 >= 0.0 { GREEN.gamma_multiply(0.25 + 0.2 * v0) } else if v0 > -0.5 { ORANGE.gamma_multiply(0.3) } else { RED.gamma_multiply(0.3) };
        let rr = if vertical { Rect::from_min_max(pos2(inner.min.x, at(v1)), pos2(inner.max.x, at(v0))) } else { Rect::from_min_max(pos2(at(v0), inner.min.y), pos2(at(v1), inner.max.y)) };
        p.rect_filled(rr, 0.0, col);
    }
    for (v, t) in [(-1.0, "-1"), (0.0, "0"), (1.0, "+1")] {
        let pt = if vertical { pos2(inner.center().x, at(v)) } else { pos2(at(v), r.max.y - 1.0) };
        if vertical {
            p.text(pt + vec2(0.0, if v > 0.0 { -9.0 } else if v < 0.0 { 9.0 } else { 0.0 }), Align2::CENTER_CENTER, t, look::regular(9.0), DIM);
        } else {
            p.line_segment([pos2(at(v), inner.min.y), pos2(at(v), inner.max.y)], Stroke::new(0.5, GRID));
            p.text(pos2(at(v) + if v > 0.0 { 9.0 } else if v < 0.0 { -9.0 } else { 0.0 }, r.center().y), Align2::CENTER_CENTER, t, look::regular(9.0), DIM);
        }
    }
    let span = |a: f32, b: f32, col: Color32, w: f32| {
        let rr = if vertical { Rect::from_min_max(pos2(inner.center().x - w, at(b)), pos2(inner.center().x + w, at(a))) } else { Rect::from_min_max(pos2(at(a), inner.center().y - w), pos2(at(b), inner.center().y + w)) };
        p.rect_filled(rr, 1.0, col);
    };
    if range {
        let (lo, hi) = (c.r("Corr Lo"), c.r("Corr Hi"));
        if hi >= lo && lo > -1.5 {
            span(lo, hi, if lo < 0.0 { YELLOW } else { GREEN }, inner.width().min(inner.height()) * 0.18);
        }
    }
    let m = c.r(min);
    if m < 0.0 && m > -1.5 {
        // the peak divergence: a line at the lowest value reached
        let col = if m < -0.5 { RED } else { ORANGE };
        let s = if vertical { [pos2(inner.min.x, at(m)), pos2(inner.max.x, at(m))] } else { [pos2(at(m), inner.min.y), pos2(at(m), inner.max.y)] };
        p.line_segment(s, Stroke::new(2.0, col));
    }
    let v = c.live.get(value).unwrap_or(0.0);
    let s = if vertical { [pos2(inner.min.x, at(v)), pos2(inner.max.x, at(v))] } else { [pos2(at(v), inner.min.y), pos2(at(v), inner.max.y)] };
    p.line_segment(s, Stroke::new(2.0, Color32::WHITE));
    ui.interact(r, c.id.with(("corr", value)), Sense::click()).on_hover_text(format!("Correlation {v:+.2} (click: reset the divergence line)")).clicked()
}

/// The Analyzer display: band bars (blue, hold segments), the scale (Top and Range; drag the
/// scale to change Range, the display to change Top).
fn analyzer(ui: &mut egui::Ui, c: &Ctx, set: &mut Set, r: Rect, hold: bool) {
    let p = ui.painter().clone();
    let top = c.get(set, mm::TOP);
    let range = c.get(set, mm::RANGE);
    let area = Rect::from_min_max(pos2(r.min.x + 30.0, r.min.y + 8.0), pos2(r.max.x - 8.0, r.max.y - 18.0));
    let y_of = |db: f32| area.min.y + area.height() * ((top - db) / range).clamp(0.0, 1.0);
    let step = if range > 60.0 { 20.0 } else { 10.0 };
    let mut db = top;
    while db >= top - range - 0.1 {
        p.line_segment([pos2(area.min.x, y_of(db)), pos2(area.max.x, y_of(db))], Stroke::new(0.5, GRID));
        p.text(pos2(area.min.x - 4.0, y_of(db)), Align2::RIGHT_CENTER, format!("{db:.0}"), look::regular(9.5), DIM);
        db -= step;
    }
    let spec = c.live.curve("spectrum");
    let held = c.live.curve("spectrum_hold");
    let n = spec.len().max(1);
    let bw = area.width() / n as f32;
    for (k, v) in spec.iter().enumerate() {
        let x0 = area.min.x + bw * k as f32 + bw * 0.12;
        let x1 = x0 + bw * 0.76;
        let y = y_of(*v);
        if y < area.max.y {
            p.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, area.max.y)), 0.0, if *v > top { RED } else { BLUE });
        }
        if hold && let Some(h) = held.get(k) {
            let yh = y_of(*h);
            if yh < area.max.y - 1.0 {
                p.line_segment([pos2(x0, yh), pos2(x1, yh)], Stroke::new(2.0, YELLOW));
            }
        }
    }
    for (f, t) in [(20.0, "20"), (100.0, "100"), (1000.0, "1k"), (10000.0, "10k"), (20000.0, "20k")] {
        let x = area.min.x + area.width() * ((f / 20.0f32).ln() / 1000f32.ln()).clamp(0.0, 1.0);
        p.text(pos2(x, area.max.y + 9.0), Align2::CENTER_CENTER, t, look::regular(9.0), DIM);
    }
    // dragging the scale changes Range; dragging the display, Top
    let scale = Rect::from_min_max(pos2(r.min.x, area.min.y), pos2(area.min.x, area.max.y));
    let rs = ui.interact(scale, c.id.with("an-scale"), Sense::drag()).on_hover_text(format!("Range {range:.0} dB"));
    if rs.dragged() {
        set.push((mm::RANGE, (range + rs.drag_delta().y * 0.5).clamp(20.0, 100.0)));
    }
    let rd = ui.interact(area, c.id.with("an-disp"), Sense::drag()).on_hover_text(format!("Top {top:.0} dB"));
    if rd.dragged() {
        set.push((mm::TOP, (top + rd.drag_delta().y * 0.3).clamp(-40.0, 20.0)));
    }
}

/// The Goniometer: the trace fades (Decay), Auto Gain lifts quiet signals, Hold keeps a
/// cumulative dim figure.
fn goniometer(ui: &mut egui::Ui, c: &Ctx, set: &Set, r: Rect, hold: bool, pair: (&str, &str)) {
    let p = ui.painter().clone();
    let side = r.width().min(r.height()) - 24.0;
    let g = Rect::from_center_size(r.center() + vec2(0.0, 6.0), vec2(side, side));
    let cc = g.center();
    // the axes: M (vertical), S (horizontal), L and R diagonals
    p.line_segment([pos2(cc.x, g.min.y), pos2(cc.x, g.max.y)], Stroke::new(0.5, GRID));
    p.line_segment([pos2(g.min.x, cc.y), pos2(g.max.x, cc.y)], Stroke::new(0.5, GRID));
    p.line_segment([g.left_top(), g.right_bottom()], Stroke::new(0.5, GRID));
    p.line_segment([g.right_top(), g.left_bottom()], Stroke::new(0.5, GRID));
    p.text(g.left_top() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, pair.0, look::regular(10.0), DIM);
    p.text(g.right_top() + vec2(-4.0, -2.0), Align2::RIGHT_BOTTOM, pair.1, look::regular(10.0), DIM);
    let pts = c.live.curve("goniometer");
    let peak = pts.iter().fold(1e-6f32, |m, v| m.max(v.abs()));
    // Auto Gain: quiet signals shown larger, in 10 % steps (0 = off)
    let ag = (c.get(set, mm::AUTO_GAIN) / 10.0).round() * 10.0;
    let gain = if ag > 0.0 { (1.0 / peak).min(1.0 + ag / 10.0).max(1.0) } else { 1.0 };
    let to = |s: f32, m: f32| pos2(cc.x + (s * gain).clamp(-1.0, 1.0) * side / 2.0, cc.y - (m * gain).clamp(-1.0, 1.0) * side / 2.0);
    // the fading trace: earlier frames kept and dimmed by Decay
    let decay = c.get(set, mm::DECAY) / 100.0;
    let trail_id = c.id.with("gonio-trail");
    let mut trail: Vec<Vec<Pos2>> = ui.data(|d| d.get_temp(trail_id)).unwrap_or_default();
    let now: Vec<Pos2> = pts.chunks(2).step_by(2).filter_map(|x| if let [s, m] = x { Some(to(*s, *m)) } else { None }).collect();
    let keep = (1.0 + decay * 14.0) as usize;
    trail.push(now);
    while trail.len() > keep {
        trail.remove(0);
    }
    let held_id = c.id.with("gonio-held");
    let mut held: Vec<Pos2> = if hold { ui.data(|d| d.get_temp(held_id)).unwrap_or_default() } else { Vec::new() };
    // Reset (and playback starting, which clears the plug-in's holds) clears the dim figure
    let reset_id = c.id.with("gonio-reset");
    let resets = (c.get(set, mm::RESET), c.live.get("True Peak Max").unwrap_or(0.0) < -150.0);
    let before: (f32, bool) = ui.data(|d| d.get_temp(reset_id)).unwrap_or(resets);
    if resets.0 != before.0 || (resets.1 && !before.1) {
        held.clear();
    }
    ui.data_mut(|d| d.insert_temp(reset_id, resets));
    if hold {
        if let Some(last) = trail.last() {
            held.extend(last.iter().step_by(4));
        }
        if held.len() > 40000 {
            held.drain(..held.len() - 40000);
        }
        for q in &held {
            p.circle_filled(*q, 0.7, DIM_BLUE);
        }
    }
    let n = trail.len();
    for (k, frame) in trail.iter().enumerate() {
        let a = (k + 1) as f32 / n as f32;
        let col = Color32::from_rgb(0x50, 0xb8, 0xff).gamma_multiply(a);
        for q in frame {
            p.circle_filled(*q, 0.9, col);
        }
    }
    ui.data_mut(|d| {
        d.insert_temp(trail_id, trail);
        d.insert_temp(held_id, held);
    });
}

/// Surround Balance: the speakers from above with circled labels, lines to neighbours with the
/// balance dot and the decorrelation line (red when out of phase), the overall position as a
/// dim sphere.
fn balance(ui: &mut egui::Ui, c: &Ctx, r: Rect, chans: &[Ch], hold: bool) {
    let p = ui.painter().clone();
    let side = r.width().min(r.height()) - 30.0;
    let cc = r.center();
    let rad = side / 2.0 - 10.0;
    let fmt = lg_dsp::surround::Format::of_channels(chans).unwrap_or(lg_dsp::surround::Format::S51);
    let spot = |ch: Ch| {
        let (az, _) = lg_dsp::surround::position(fmt, ch);
        cc + vec2(az.to_radians().sin(), -az.to_radians().cos()) * rad
    };
    let pairs = c.live.get("Pairs").unwrap_or(0.0) as usize;
    let dots_id = c.id.with("bal-dots");
    let mut dots: Vec<Pos2> = ui.data(|d| d.get_temp(dots_id)).unwrap_or_default();
    dots.resize(pairs, cc);
    for k in 0..pairs {
        let (a, b) = (c.rk("PairA", k) as usize, c.rk("PairB", k) as usize);
        let (Some(ca), Some(cb)) = (chans.get(a), chans.get(b)) else { continue };
        let (pa, pb) = (spot(*ca), spot(*cb));
        p.line_segment([pa, pb], Stroke::new(1.0, GRID));
        let bal = c.rk("Bal", k).clamp(-1.0, 1.0);
        let at = pa + (pb - pa) * ((bal + 1.0) / 2.0);
        // with Hold on the dots stay at their latest position while quiet
        let quiet = c.rk("RMS", a) < -90.0 && c.rk("RMS", b) < -90.0;
        let at = if hold && quiet { dots[k] } else { at };
        dots[k] = at;
        let corr = c.rk("PCorr", k);
        // decorrelation: a line from the dot towards the room's centre, longer when less correlated
        let len = (1.0 - corr).clamp(0.0, 2.0) / 2.0 * rad * 0.5;
        let dir = (cc - at).normalized();
        p.line_segment([at, at + dir * len], Stroke::new(2.0, if corr < 0.0 { RED } else { Color32::from_rgb(0x7c, 0xc8, 0xff) }));
        if hold && c.rk("PCorrMin", k) < 0.0 {
            p.circle_stroke(at, 6.0, Stroke::new(1.0, RED));
        }
        p.circle_filled(at, 4.0, BLUE);
    }
    ui.data_mut(|d| d.insert_temp(dots_id, dots));
    for ch in chans {
        if *ch == Ch::Lfe {
            continue;
        }
        let s = spot(*ch);
        p.circle_filled(s, 11.0, BG);
        p.circle_stroke(s, 11.0, Stroke::new(1.0, DIM));
        p.text(s, Align2::CENTER_CENTER, ch.label(), look::regular(9.5), TEXT);
    }
    let (bx, by) = (c.r("Bal X").clamp(-1.0, 1.0), c.r("Bal Y").clamp(-1.0, 1.0));
    p.circle_filled(cc + vec2(bx, -by) * rad, 14.0, Color32::from_rgba_unmultiplied(0x60, 0x90, 0xd0, 60));
}

/// MultiMeter (stereo and surround).
fn multimeter(ui: &mut egui::Ui, c: &Ctx, set: &mut Set) {
    let chans = c.channels();
    let surround = chans.len() > 2;
    let view = c.geti(set, mm::VIEW);
    let mode = c.geti(set, mm::MODE).min(if surround { 2 } else { 1 });
    let hold = c.get(set, mm::HOLD) >= 0.5;
    let w = ui.available_width();
    let h = (w * 0.62).max(300.0);
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 4.0, BG);
    // the View pop-up: Full, Meters, Display (Compact in surround)
    let vr = Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(150.0, 20.0));
    {
        let names = if surround { ["Full", "Meters", "Compact"] } else { ["Full", "Meters", "Display"] };
        p.text(vr.left_center(), Align2::LEFT_CENTER, "View:", look::regular(11.0), DIM);
        let br = Rect::from_min_max(vr.min + vec2(36.0, 0.0), vr.max);
        let resp = ui.interact(br, c.id.with("mm-view"), Sense::click());
        p.rect_filled(br, 3.0, Color32::from_gray(0x30));
        p.text(br.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, names[view.min(2)], look::regular(11.0), TEXT);
        egui::Popup::menu(&resp).show(|ui| {
            for (k, n) in names.iter().enumerate() {
                if ui.add(egui::Button::selectable(k == view, *n)).clicked() {
                    set.push((mm::VIEW, k as f32));
                    ui.close();
                }
            }
        });
    }
    let body = Rect::from_min_max(r.min + vec2(8.0, 32.0), r.max - vec2(8.0, 8.0));
    let controls_h = if view == 0 { 92.0 } else { 0.0 };
    let main_h = body.height() - controls_h;
    // Full: everything; Meters: the meters only; Display (stereo): the mode only; Compact
    // (surround): the mode and the meters, no controls
    let show_disp = view != 1;
    let show_meters = view != 2 || surround;
    let meters_w = if !show_meters { 0.0 } else if !show_disp { body.width() } else { (if surround { 30.0 * chans.len() as f32 + 40.0 } else { 100.0 }) + 60.0 };
    let disp = Rect::from_min_max(body.min, pos2(body.max.x - meters_w - if meters_w > 0.0 { 8.0 } else { 0.0 }, body.min.y + main_h - if surround { 0.0 } else { 32.0 }));
    if show_disp {
    p.rect_filled(disp, 3.0, DISPLAY);
    match mode {
        1 => {
            let pair = ["L-R", "Ls-Rs", "Lm-Rm", "Lc-Rc"][c.geti(set, mm::PAIR).min(3)];
            let (a, b) = pair.split_once('-').unwrap_or(("L", "R"));
            goniometer(ui, c, set, disp, hold, (a, b));
        }
        2 => balance(ui, c, disp, &chans, hold),
        _ => analyzer(ui, c, set, disp, hold),
    }
    // Control-click on the display (partial views) switches the mode
    let dresp = ui.interact(disp, c.id.with("mm-disp-ctx"), Sense::click());
    if view != 0 && (dresp.secondary_clicked() || (dresp.clicked() && ui.input(|i| i.modifiers.ctrl))) {
        set.push((mm::MODE, ((mode + 1) % if surround { 3 } else { 2 }) as f32));
    }
    if !surround {
        let cr = Rect::from_min_max(pos2(disp.min.x, disp.max.y + 6.0), pos2(disp.max.x, disp.max.y + 30.0));
        if correlation(ui, c, cr, "Correlation", "Corr Min", false, false) {
            bump(c, set, mm::RESET);
        }
    }
    }
    if meters_w > 0.0 {
        let mr = Rect::from_min_max(pos2(body.max.x - meters_w, body.min.y + 18.0), pos2(body.max.x, body.min.y + main_h));
        // LU-I and LU-S above the loudness meter
        let lr = Rect::from_min_size(mr.min, vec2(54.0, mr.height()));
        p.text(lr.center_top() - vec2(0.0, 12.0), Align2::CENTER_CENTER, format!("I {}  S {}", lu(c.r("Integrated")), lu(c.r("Short-Term"))), look::regular(9.5), TEXT);
        loudness_bars(ui, c, set, lr, &[("Momentary", "M")], mm::TARGET);
        let lv = Rect::from_min_max(pos2(lr.max.x + 6.0, mr.min.y), mr.max);
        level_bars(ui, c, set, lv, &chans, c.geti(set, mm::LEVEL), (mm::LEVEL_TARGET, if surround { mm::C_TARGET } else { u32::MAX }, if surround { mm::LFE_TARGET } else { u32::MAX }), mm::RESET, false);
    }
    if view != 0 {
        return;
    }
    // the parameters: mode buttons, the mode's own, the Peak parameters
    let row1 = body.max.y - controls_h + 8.0;
    let x0 = body.min.x;
    let modes: &[&str] = if surround { &["Analyzer", "Goniometer", "Balance"] } else { &["Analyzer", "Goniometer"] };
    choice(ui, c, set, Rect::from_min_size(pos2(x0, row1), vec2(80.0 * modes.len() as f32, 22.0)), mm::MODE, modes);
    let row2 = row1 + 30.0;
    let row3 = row2 + 28.0;
    match mode {
        0 => {
            field(ui, c, set, Rect::from_min_size(pos2(x0, row2), vec2(100.0, 20.0)), mm::TOP, "Top", "dB", 0.3);
            field(ui, c, set, Rect::from_min_size(pos2(x0 + 110.0, row2), vec2(110.0, 20.0)), mm::RANGE, "Range", "dB", 0.5);
            choice(ui, c, set, Rect::from_min_size(pos2(x0 + 230.0, row2), vec2(210.0, 20.0)), mm::AN_MODE, &["Peak", "Slow RMS", "Fast RMS"]);
            popup(ui, c, set, Rect::from_min_size(pos2(x0, row3), vec2(200.0, 20.0)), mm::BANDS, "Bands");
            if surround {
                // the channel buttons (height channels aren't analyzed) and Sum / Max
                let mask = c.get(set, mm::CHANNELS) as u32;
                let mut x = x0 + 210.0;
                for (k, ch) in chans.iter().enumerate() {
                    if lg_dsp::surround::position(lg_dsp::surround::Format::S714, *ch).1 > 0.0 {
                        continue;
                    }
                    let on = mask & (1 << k) != 0;
                    if btn(ui, Rect::from_min_size(pos2(x, row3), vec2(32.0, 20.0)), c.id.with(("anch", k)), ch.label(), on).clicked() {
                        set.push((mm::CHANNELS, (mask ^ (1 << k)) as f32));
                    }
                    x += 34.0;
                }
                choice(ui, c, set, Rect::from_min_size(pos2(x + 6.0, row3), vec2(90.0, 20.0)), mm::SUM_MAX, &["Sum", "Max"]);
            } else {
                choice(ui, c, set, Rect::from_min_size(pos2(x0 + 210.0, row3), vec2(230.0, 20.0)), mm::DETECTION, &["Left", "Right", "LRmax", "Mono"]);
            }
        }
        1 => {
            field(ui, c, set, Rect::from_min_size(pos2(x0, row2), vec2(130.0, 20.0)), mm::AUTO_GAIN, "Auto Gain", "%", 0.5);
            field(ui, c, set, Rect::from_min_size(pos2(x0 + 140.0, row2), vec2(120.0, 20.0)), mm::DECAY, "Decay", "%", 0.5);
            if surround {
                let fmt = lg_dsp::surround::Format::of_channels(&chans);
                let have = |a: Ch| chans.contains(&a);
                let mut labels = vec!["L-R"];
                if have(Ch::Ls) {
                    labels.push("Ls-Rs");
                }
                if have(Ch::Lm) {
                    labels.push("Lm-Rm");
                }
                if have(Ch::Lc) || fmt == Some(lg_dsp::surround::Format::Sdds) {
                    labels.push("Lc-Rc");
                }
                choice(ui, c, set, Rect::from_min_size(pos2(x0, row3), vec2(70.0 * labels.len() as f32, 20.0)), mm::PAIR, &labels);
            }
        }
        _ => {
            p.text(pos2(x0, row2 + 10.0), Align2::LEFT_CENTER, "LFE Correlation", look::regular(11.0), DIM);
            choice(ui, c, set, Rect::from_min_size(pos2(x0 + 100.0, row2), vec2(150.0, 20.0)), mm::LFE_SRC, &["C", "LCR", "Sum"]);
            let lc = Rect::from_min_size(pos2(x0 + 260.0, row2), vec2(220.0, 20.0));
            correlation(ui, c, lc, "LFE Corr", if hold { "LFE Corr Min" } else { "" }, false, false);
        }
    }
    // the Peak parameters and the Level pop-up
    let px = body.max.x - 300.0;
    let hold_on = c.get(set, mm::HOLD) >= 0.5;
    if btn(ui, Rect::from_min_size(pos2(px, row1), vec2(56.0, 22.0)), c.id.with("hold"), "Hold", hold_on).clicked() {
        set.push((mm::HOLD, if hold_on { 0.0 } else { 1.0 }));
    }
    if btn(ui, Rect::from_min_size(pos2(px + 62.0, row1), vec2(56.0, 22.0)), c.id.with("reset"), "Reset", false).clicked() {
        bump(c, set, mm::RESET);
    }
    popup(ui, c, set, Rect::from_min_size(pos2(px + 126.0, row1), vec2(174.0, 22.0)), mm::PEAK_TIME, "Peak");
    popup(ui, c, set, Rect::from_min_size(pos2(px, row2), vec2(300.0, 20.0)), mm::RETURN, "Return Rate");
    popup(ui, c, set, Rect::from_min_size(pos2(px, row3), vec2(300.0, 20.0)), mm::LEVEL, "Level");
}

fn lu(v: f32) -> String {
    if v <= -119.0 { "--".into() } else { format!("{v:.1}") }
}

fn orientation(ui: &mut egui::Ui, c: &Ctx, set: &mut Set) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    popup(ui, c, set, Rect::from_min_size(r.min, vec2(180.0, 20.0)), 6, "View:");
    c.geti(set, 6) == 1
}

/// The editor for one of the metering plug-ins.
pub fn view(ui: &mut egui::Ui, name: &str, params: &[Param], values: &[f32], live: &Live, slot: (usize, usize), set: &mut Set) {
    let c = Ctx { params, values, live, id: egui::Id::unique(("meter-view", slot, name)) };
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    match name {
        "MultiMeter" => multimeter(ui, &c, set),
        "Level Meter" => {
            let horiz = orientation(ui, &c, set);
            let chans = c.channels();
            let w = ui.available_width();
            let h = if horiz { 30.0 + 22.0 * chans.len() as f32 } else { (ui.available_height() - 40.0).max(220.0) };
            let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            level_bars(ui, &c, set, r, &chans, c.geti(set, 0), (3, 4, 5), 1, horiz);
            let (rr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
            popup(ui, &c, set, Rect::from_min_size(rr.min + vec2(0.0, 4.0), vec2(200.0, 20.0)), 0, "Mode");
        }
        "Loudness Meter" => {
            let horiz = orientation(ui, &c, set);
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, if horiz { 120.0 } else { (ui.available_height() - 70.0).max(220.0) }), Sense::hover());
            let bars = Rect::from_min_size(r.min, vec2(if horiz { r.width() } else { 150.0 }, r.height()));
            loudness_bars(ui, &c, set, bars, &[("Momentary", "M"), ("Short-Term", "S"), ("Integrated", "I")], 2);
            let info = if horiz { Rect::from_min_size(pos2(r.min.x, r.max.y), vec2(w, 0.0)) } else { Rect::from_min_max(pos2(bars.max.x + 12.0, r.min.y), r.max) };
            let p = ui.painter();
            let at = if horiz { info.min } else { info.min + vec2(0.0, 10.0) };
            p.text(at, Align2::LEFT_TOP, format!("LU Range  {:.1} LU", c.live.get("LU Range").unwrap_or(0.0)), look::regular(13.0), TEXT);
            p.text(at + vec2(0.0, 22.0), Align2::LEFT_TOP, format!("Integrated  {} LUFS", lu(c.r("Integrated"))), look::regular(13.0), TEXT);
            p.text(at + vec2(0.0, 44.0), Align2::LEFT_TOP, format!("Target  {:.1} LUFS", c.get(set, 2)), look::regular(11.0), DIM);
            let (rr, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
            let running = c.get(set, 0) >= 0.5;
            if btn(ui, Rect::from_min_size(rr.min + vec2(0.0, 6.0), vec2(70.0, 22.0)), c.id.with("start"), if running { "Pause" } else { "Start" }, running).clicked() {
                set.push((0, if running { 0.0 } else { 1.0 }));
            }
            if btn(ui, Rect::from_min_size(rr.min + vec2(76.0, 6.0), vec2(70.0, 22.0)), c.id.with("reset"), "Reset", false).clicked() {
                bump(&c, set, 1);
            }
        }
        "Correlation Meter" => {
            let horiz = orientation(ui, &c, set);
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, if horiz { 40.0 } else { 220.0 }), Sense::hover());
            let rr = if horiz { r } else { Rect::from_center_size(r.center(), vec2(44.0, r.height())) };
            if correlation(ui, &c, rr, "Correlation", "Corr Min", true, !horiz) {
                bump(&c, set, 1);
            }
            // Reaction, behind the lower-left disclosure triangle
            let open_id = c.id.with("reaction-open");
            let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
            let (tr, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
            let tri = Rect::from_min_size(tr.min + vec2(0.0, 4.0), vec2(16.0, 16.0));
            if ui.interact(tri, c.id.with("tri"), Sense::click()).clicked() {
                open = !open;
            }
            ui.painter().text(tri.center(), Align2::CENTER_CENTER, if open { "▾" } else { "▸" }, look::regular(12.0), DIM);
            if open {
                popup(ui, &c, set, Rect::from_min_size(tri.right_top() + vec2(6.0, 0.0), vec2(170.0, 18.0)), 0, "Reaction");
            }
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        "BPM Counter" => {
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, 120.0), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(r, 4.0, DISPLAY);
            let locked = c.live.get("BPM Locked").unwrap_or(0.0);
            let bpm = c.live.get("BPM").unwrap_or(0.0);
            let shown = if locked > 0.0 { locked } else { bpm };
            let dr = Rect::from_center_size(r.center(), vec2(200.0, 60.0));
            p.text(dr.center(), Align2::CENTER_CENTER, if shown > 0.0 { format!("{shown:.1}") } else { "---.-".into() }, look::bold(40.0), look::lcd_text());
            if ui.interact(dr, c.id.with("bpm-disp"), Sense::click()).on_hover_text("Click to reset").clicked() {
                bump(&c, set, 0);
            }
            // the LED: flashing while measuring, steady once the tempo is found
            let t = ui.input(|i| i.time);
            let lit = if locked > 0.0 { true } else if bpm > 0.0 || c.r("Peak 0") > -60.0 { (t * 4.0).fract() < 0.5 } else { false };
            p.circle_filled(pos2(dr.min.x - 16.0, dr.center().y), 6.0, if lit { GREEN } else { Color32::from_gray(0x30) });
            let (br, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
            let f = c.geti(set, 2);
            if btn(ui, Rect::from_min_size(br.min + vec2(0.0, 6.0), vec2(44.0, 22.0)), c.id.with("half"), ":2", f == 0).clicked() {
                set.push((2, if f == 0 { 1.0 } else { 0.0 }));
            }
            if btn(ui, Rect::from_min_size(br.min + vec2(50.0, 6.0), vec2(44.0, 22.0)), c.id.with("double"), "x2", f == 2).clicked() {
                set.push((2, if f == 2 { 1.0 } else { 2.0 }));
            }
            popup(ui, &c, set, Rect::from_min_size(br.min + vec2(110.0, 6.0), vec2(160.0, 22.0)), 1, "Range");
        }
        _ => tuner(ui, &c, set),
    }
}

fn tuner(ui: &mut egui::Ui, c: &Ctx, set: &mut Set) {
    let horiz = orientation(ui, c, set);
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, if horiz { 150.0 } else { 260.0 }), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 4.0, DISPLAY);
    let note = c.live.get("Note");
    let cents = c.live.get("Cents").unwrap_or(0.0);
    let in_tune = note.is_some() && cents.abs() < 1.5;
    let col = if note.is_none() { DIM } else if in_tune { GREEN } else { ORANGE };
    let names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    let t = note.map(|n| format!("{}{}", names[(n as i32).rem_euclid(12) as usize], (n as i32) / 12 - 1)).unwrap_or_else(|| "—".into());
    // the segment arc (horizontal: a row of segments)
    let segs = 21;
    let lit = ((cents / 50.0).clamp(-1.0, 1.0) * 10.0).round() as i32;
    for k in -10..=10 {
        let on = note.is_some() && (k == lit || (k == 0 && in_tune));
        let cl = if !on { Color32::from_gray(0x2c) } else if k == 0 { GREEN } else { ORANGE };
        if horiz {
            let x = r.center().x + k as f32 * (r.width() - 40.0) / segs as f32;
            p.rect_filled(Rect::from_center_size(pos2(x, r.min.y + 30.0), vec2(8.0, if k == 0 { 30.0 } else { 20.0 })), 1.0, cl);
        } else {
            let a = (-90.0 + k as f32 * 7.0f32).to_radians();
            let cc = pos2(r.center().x, r.min.y + 170.0);
            let u = vec2(a.cos(), a.sin());
            p.line_segment([cc + u * 110.0, cc + u * if k == 0 { 145.0 } else { 135.0 }], Stroke::new(6.0, cl));
        }
    }
    let ny = if horiz { r.min.y + 85.0 } else { r.min.y + 150.0 };
    p.text(pos2(r.center().x, ny), Align2::CENTER_CENTER, t, look::bold(36.0), col);
    p.text(pos2(r.center().x, ny + 34.0), Align2::CENTER_CENTER, if note.is_some() { format!("{cents:+.0} cents") } else { String::new() }, look::regular(13.0), col);
    // the Reference Tuning field: drag vertically (410–470 Hz, 0.1 Hz steps)
    let (fr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    let refp = c.get(set, 0);
    let fld = Rect::from_min_size(fr.min + vec2(120.0, 4.0), vec2(80.0, 20.0));
    ui.painter().text(pos2(fr.min.x, fld.center().y), Align2::LEFT_CENTER, "Reference Tuning", look::regular(11.0), DIM);
    ui.painter().rect_filled(fld, 3.0, DISPLAY);
    ui.painter().text(fld.center(), Align2::CENTER_CENTER, format!("{refp:.1} Hz"), look::regular(11.0), TEXT);
    let resp = ui.interact(fld, c.id.with("ref"), Sense::click_and_drag());
    if resp.dragged() {
        let v = ((refp - resp.drag_delta().y * 0.1) * 10.0).round() / 10.0;
        set.push((0, v.clamp(410.0, 470.0)));
    }
    if resp.double_clicked() {
        set.push((0, 440.0));
    }
    let mute = c.get(set, 1) >= 0.5;
    if btn(ui, Rect::from_min_size(fr.min + vec2(220.0, 4.0), vec2(60.0, 20.0)), c.id.with("mute"), "Mute", mute).clicked() {
        set.push((1, if mute { 0.0 } else { 1.0 }));
    }
}
