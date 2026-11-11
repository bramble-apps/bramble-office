//! What the synthesizer windows share (ES1, ES2, ES E / M / P, EFM1, Retro Synth, Sculpture,
//! EVOC 20 PolySynth, Ultrabeat): the window frame with Logic's plug-in header (bypass, the
//! Setting menu with Recall Default, Save Setting As…, the saved settings, previous / next,
//! Copy, Paste, Compare), the fixed panel the controls are laid out on, the extended
//! parameters under a disclosure triangle, and the controls themselves: knobs, faders, the
//! two-arrow range sliders (drag an arrow, or the bar between them to move both), segmented
//! buttons, pop-up menus, value fields and XY pads. Logic's gestures: drag up or down, Shift
//! for fine steps, Option-click (or double-click) for the default value.
//!
//! Every change is a plug-in undo step (⌘Z with the pointer over the window).

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_dsp::Param;

use crate::app::App;
use crate::instwin;
use crate::look;

/// A panel's colours.
#[derive(Clone, Copy)]
pub struct Skin {
    pub bg: Color32,
    pub panel: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub dim: Color32,
}

impl Skin {
    pub const fn new(bg: u32, panel: u32, accent: u32) -> Skin {
        Skin { bg: rgb(bg), panel: rgb(panel), accent: rgb(accent), text: Color32::from_rgb(0xe6, 0xe6, 0xe8), dim: Color32::from_rgb(0x9c, 0x9c, 0xa2) }
    }
}

pub const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The panel being drawn: positions are in the panel's own points from its top left.
pub struct Pane<'a> {
    pub ui: &'a mut egui::Ui,
    pub o: Pos2,
    pub params: &'a [Param],
    pub vals: &'a [f32],
    pub texts: &'a [Option<String>],
    pub set: Vec<(u32, f32)>,
    pub skin: Skin,
    pub t: usize,
    /// Actions the body asks the frame for (by name; the windows define their own).
    pub actions: Vec<String>,
}

impl Pane<'_> {
    pub fn idx(&self, id: u32) -> Option<usize> {
        self.params.iter().position(|p| p.id == id)
    }
    /// The value now (a change made this frame counts).
    pub fn get(&self, id: u32) -> f32 {
        if let Some(x) = self.set.iter().rev().find(|x| x.0 == id) {
            return x.1;
        }
        self.idx(id).map(|i| self.vals[i]).unwrap_or(0.0)
    }
    pub fn geti(&self, id: u32) -> usize {
        self.get(id).round().max(0.0) as usize
    }
    pub fn on(&self, id: u32) -> bool {
        self.get(id) >= 0.5
    }
    pub fn put(&mut self, id: u32, v: f32) {
        self.set.push((id, v));
    }
    pub fn prm(&self, id: u32) -> Param {
        self.idx(id).map(|i| self.params[i].clone()).unwrap_or(Param::new(id, "", 0.0, 1.0, 0.0, lg_dsp::Unit::None))
    }
    /// The value as text (the instrument's own wording when it has one).
    pub fn shown(&self, id: u32, v: f32) -> String {
        shown_of(self.params, self.vals, self.texts, id, v)
    }
    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.o + vec2(x, y), vec2(w, h))
    }
    pub fn id(&self, what: &str, id: u32) -> egui::Id {
        egui::Id::unique(("synth", self.t, what, id))
    }
    pub fn painter(&self) -> egui::Painter {
        self.ui.painter().clone()
    }
    pub fn text(&self, x: f32, y: f32, align: Align2, s: &str, size: f32, col: Color32) {
        self.ui.painter().text(self.o + vec2(x, y), align, s, look::regular(size), col);
    }
    pub fn label(&self, x: f32, y: f32, s: &str) {
        self.text(x, y, Align2::CENTER_CENTER, s, 10.5, self.skin.dim);
    }
    /// A section: a rounded box with its name at the top left.
    pub fn group(&self, x: f32, y: f32, w: f32, h: f32, title: &str) {
        let r = self.rect(x, y, w, h);
        self.ui.painter().rect_filled(r, 7.0, self.skin.panel);
        if !title.is_empty() {
            self.ui.painter().text(r.min + vec2(9.0, 10.0), Align2::LEFT_CENTER, title, look::semibold(10.5), self.skin.dim);
        }
    }
    /// A knob of diameter `d` centred at x, with its label under it.
    pub fn knob(&mut self, x: f32, y: f32, d: f32, id: u32, label: &str) -> bool {
        let p = self.prm(id);
        let mut v = self.get(id);
        let r = Rect::from_min_size(self.o + vec2(x - d / 2.0 - 2.0, y), vec2(d + 4.0, d + 18.0));
        let (ps, vs, ts) = (self.params, self.vals, self.texts);
        let kid = self.id("knob", id);
        let ch = crate::widgets::knob(self.ui, r, kid, &mut v, p.min, p.max, p.default, label, &|v| shown_of(ps, vs, ts, id, v));
        if ch {
            let v = if matches!(p.unit, lg_dsp::Unit::Menu | lg_dsp::Unit::OnOff) { v.round() } else { v };
            self.put(id, v);
        }
        ch
    }
    /// A knob that clicks through a menu parameter's choices (the choice shown under it).
    pub fn knob_menu(&mut self, x: f32, y: f32, d: f32, id: u32, label: &str) {
        let p = self.prm(id);
        let cur = self.geti(id);
        let r = Rect::from_min_size(self.o + vec2(x - d / 2.0 - 2.0, y), vec2(d + 4.0, d + 18.0));
        let acc_id = self.id("knobacc", id);
        let mut acc: f32 = self.ui.data(|dd| dd.get_temp(acc_id)).unwrap_or(cur as f32);
        let resp = self.ui.interact(r, self.id("knobm", id), Sense::click_and_drag());
        if resp.drag_started() {
            acc = cur as f32;
        }
        if resp.dragged() {
            acc = (acc - resp.drag_delta().y / 14.0).clamp(p.min, p.max);
            if acc.round() as usize != cur {
                self.put(id, acc.round());
            }
        }
        self.ui.data_mut(|dd| dd.insert_temp(acc_id, acc));
        if resp.double_clicked() || (resp.clicked() && self.ui.input(|i| i.modifiers.alt)) {
            self.put(id, p.default);
        }
        let c = pos2(r.center().x, r.min.y + d / 2.0 + 2.0);
        let pt = self.ui.painter();
        let n = (p.max - p.min).max(1.0);
        let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
        let sweep = std::f32::consts::PI * 5.0 / 3.0;
        for k in 0..=(n as usize) {
            let a = start + sweep * k as f32 / n;
            pt.circle_filled(c + vec2(a.cos(), a.sin()) * (d / 2.0 - 1.0), 1.5, if k == cur { self.skin.accent } else { Color32::from_gray(0x60) });
        }
        pt.circle_filled(c, d / 2.0 - 5.0, Color32::from_gray(if resp.hovered() || resp.dragged() { 0x5a } else { 0x4a }));
        let a = start + sweep * (cur as f32 - p.min) / n;
        pt.line_segment([c + vec2(a.cos(), a.sin()) * (d / 2.0 - 12.0).max(2.0), c + vec2(a.cos(), a.sin()) * (d / 2.0 - 6.0)], Stroke::new(2.0, Color32::WHITE));
        let text = if resp.hovered() || resp.dragged() { p.choices.get(cur).copied().unwrap_or("").to_string() } else { label.to_string() };
        pt.text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_CENTER, text, look::regular(10.5), self.skin.dim);
    }
    fn drag_to(&mut self, resp: &egui::Response, id: u32, p: &Param, frac: Option<f32>) {
        if resp.double_clicked() || (resp.clicked() && self.ui.input(|i| i.modifiers.alt)) {
            self.put(id, p.default);
        } else if let (true, Some(f)) = (resp.dragged() || resp.clicked(), frac) {
            self.put(id, p.min + f.clamp(0.0, 1.0) * (p.max - p.min));
        }
    }
    /// A vertical fader (the value rises upward), its label under it.
    pub fn vfader(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32, label: &str) {
        let p = self.prm(id);
        let v = self.get(id);
        let r = self.rect(x, y, w, h);
        let resp = self.ui.interact(r, self.id("vf", id), Sense::click_and_drag()).on_hover_text(format!("{}: {}", p.name, self.shown(id, v)));
        let f = (v - p.min) / (p.max - p.min).max(1e-9);
        let pt = self.ui.painter();
        let track = Rect::from_center_size(r.center(), vec2(4.0, r.height() - 8.0));
        pt.rect_filled(track, 2.0, Color32::from_gray(0x18));
        let cy = track.max.y - track.height() * f;
        // lit from the bottom, or from zero for values that go both ways
        let zy = if p.min < 0.0 && p.max > 0.0 { track.max.y - track.height() * (-p.min / (p.max - p.min)) } else { track.max.y };
        pt.rect_filled(Rect::from_min_max(pos2(track.min.x, cy.min(zy)), pos2(track.max.x, cy.max(zy))), 2.0, self.skin.accent.gamma_multiply(0.7));
        pt.rect_filled(Rect::from_center_size(pos2(r.center().x, cy), vec2(w.min(18.0), 8.0)), 2.0, Color32::from_gray(0xd8));
        if !label.is_empty() {
            pt.text(pos2(r.center().x, r.max.y + 8.0), Align2::CENTER_CENTER, label, look::regular(10.5), self.skin.dim);
        }
        let frac = resp.interact_pointer_pos().map(|pp| (track.max.y - pp.y) / track.height());
        self.drag_to(&resp, id, &p, frac);
    }
    /// A horizontal fader, its label at the left when given.
    pub fn hfader(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32) {
        let p = self.prm(id);
        let v = self.get(id);
        let r = self.rect(x, y, w, h);
        let resp = self.ui.interact(r, self.id("hf", id), Sense::click_and_drag()).on_hover_text(format!("{}: {}", p.name, self.shown(id, v)));
        let f = (v - p.min) / (p.max - p.min).max(1e-9);
        let pt = self.ui.painter();
        let track = Rect::from_center_size(r.center(), vec2(r.width() - 8.0, 4.0));
        pt.rect_filled(track, 2.0, Color32::from_gray(0x18));
        let cx = track.min.x + track.width() * f;
        let zero = if p.min < 0.0 && p.max > 0.0 { track.min.x + track.width() * (-p.min / (p.max - p.min)) } else { track.min.x };
        pt.rect_filled(Rect::from_min_max(pos2(zero.min(cx), track.min.y), pos2(zero.max(cx), track.max.y)), 2.0, self.skin.accent.gamma_multiply(0.7));
        pt.rect_filled(Rect::from_center_size(pos2(cx, r.center().y), vec2(8.0, h.min(16.0))), 2.0, Color32::from_gray(0xd8));
        let frac = resp.interact_pointer_pos().map(|pp| (pp.x - track.min.x) / track.width());
        self.drag_to(&resp, id, &p, frac);
    }
    /// Two arrows on one slider (a range: the value at the least and at the most of its
    /// source); the bar between them drags both and keeps their distance.
    pub fn range(&mut self, x: f32, y: f32, w: f32, h: f32, lo: u32, hi: u32, label: &str, col: Color32) {
        let p = self.prm(lo);
        let (a, b) = (self.get(lo), self.get(hi));
        let r = self.rect(x, y, w, h);
        let vertical = h > w;
        let span = (p.max - p.min).max(1e-9);
        let (t0, len) = if vertical { (r.max.y - 5.0, -(r.height() - 10.0)) } else { (r.min.x + 5.0, r.width() - 10.0) };
        let at = |v: f32| t0 + len * (v - p.min) / span;
        let (pa, pb) = (at(a), at(b));
        let resp = self.ui.interact(r, self.id("range", lo), Sense::click_and_drag()).on_hover_text(format!("{label}: {} … {}", self.shown(lo, a), self.shown(hi, b)));
        // which part the drag took: 0 the lower arrow, 1 the upper, 2 the bar
        let part_id = self.id("rangepart", lo);
        let mut part: u8 = self.ui.data(|d| d.get_temp(part_id)).unwrap_or(2);
        if resp.drag_started()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            let q = if vertical { pp.y } else { pp.x };
            let (da, db) = ((q - pa).abs(), (q - pb).abs());
            let between = (q - pa) * (q - pb) < 0.0;
            // the lower arrow is drawn on the left / under, the upper on the right / over
            let side = if vertical { pp.x - r.center().x } else { r.center().y - pp.y };
            part = if between && da > 6.0 && db > 6.0 {
                2
            } else if (da - db).abs() < 3.0 {
                if side < 0.0 { 0 } else { 1 }
            } else if da < db {
                0
            } else {
                1
            };
            self.ui.data_mut(|d| d.insert_temp(part_id, part));
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            let dv = (if vertical { d.y } else { d.x }) / len * span * if self.ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
            if dv != 0.0 {
                match part {
                    0 => self.put(lo, (a + dv).clamp(p.min, p.max)),
                    1 => self.put(hi, (b + dv).clamp(p.min, p.max)),
                    _ => {
                        let dv = dv.clamp(p.min - a.min(b), p.max - a.max(b));
                        self.put(lo, a + dv);
                        self.put(hi, b + dv);
                    }
                }
            }
        }
        if resp.double_clicked() || (resp.clicked() && self.ui.input(|i| i.modifiers.alt)) {
            let d = self.prm(hi).default;
            self.put(lo, p.default);
            self.put(hi, d);
        }
        let pt = self.ui.painter();
        let tri = |c: Pos2, dir: egui::Vec2, colr: Color32| {
            let n = vec2(-dir.y, dir.x);
            pt.add(egui::Shape::convex_polygon(vec![c, c + dir * 8.0 + n * 5.0, c + dir * 8.0 - n * 5.0], colr, Stroke::NONE));
        };
        if vertical {
            let track = Rect::from_center_size(r.center(), vec2(4.0, r.height() - 10.0));
            pt.rect_filled(track, 2.0, Color32::from_gray(0x18));
            pt.rect_filled(Rect::from_min_max(pos2(track.min.x - 1.0, pa.min(pb)), pos2(track.max.x + 1.0, pa.max(pb))), 2.0, col);
            tri(pos2(track.min.x - 1.0, pa), vec2(-1.0, 0.0), Color32::from_gray(0xd8));
            tri(pos2(track.max.x + 1.0, pb), vec2(1.0, 0.0), Color32::from_gray(0xd8));
            pt.text(pos2(r.center().x, r.max.y + 8.0), Align2::CENTER_CENTER, label, look::regular(10.5), self.skin.dim);
        } else {
            let track = Rect::from_center_size(r.center(), vec2(r.width() - 10.0, 4.0));
            pt.rect_filled(track, 2.0, Color32::from_gray(0x18));
            pt.rect_filled(Rect::from_min_max(pos2(pa.min(pb), track.min.y - 1.0), pos2(pa.max(pb), track.max.y + 1.0)), 2.0, col);
            tri(pos2(pa, track.max.y + 1.0), vec2(0.0, 1.0), Color32::from_gray(0xd8));
            tri(pos2(pb, track.min.y - 1.0), vec2(0.0, -1.0), Color32::from_gray(0xd8));
            pt.text(pos2(r.min.x, r.min.y - 7.0), Align2::LEFT_CENTER, label, look::regular(10.5), self.skin.dim);
        }
    }
    /// Segmented buttons for a choice (one lit); `labels` in the order of the values.
    pub fn seg(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32, labels: &[&str], vertical: bool) {
        let cur = self.geti(id);
        let n = labels.len().max(1) as f32;
        for (k, l) in labels.iter().enumerate() {
            let r = if vertical { self.rect(x, y + h / n * k as f32, w, h / n - 2.0) } else { self.rect(x + w / n * k as f32, y, w / n - 2.0, h) };
            if self.button(r, l, k == cur, ("seg", id, k)) && k != cur {
                self.put(id, k as f32);
            }
        }
    }
    /// A lit / unlit button; true when clicked.
    pub fn button(&mut self, r: Rect, label: &str, lit: bool, salt: impl std::hash::Hash + std::fmt::Debug) -> bool {
        let resp = self.ui.interact(r, egui::Id::unique(("synthbtn", self.t, salt)), Sense::click());
        let pt = self.ui.painter();
        let fill = if lit { self.skin.accent } else if resp.hovered() { Color32::from_gray(0x52) } else { Color32::from_gray(0x3c) };
        pt.rect_filled(r, 4.0, fill);
        pt.text(r.center(), Align2::CENTER_CENTER, label, look::regular(10.5), if lit { Color32::from_gray(0x10) } else { self.skin.text });
        resp.clicked()
    }
    /// An on / off button for a switch parameter.
    pub fn toggle(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32, label: &str) {
        let on = self.on(id);
        let r = self.rect(x, y, w, h);
        if self.button(r, label, on, ("tog", id)) {
            self.put(id, if on { 0.0 } else { 1.0 });
        }
    }
    /// A pop-up menu showing the parameter's choice.
    pub fn menu(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32) {
        let p = self.prm(id);
        let cur = self.geti(id);
        let r = self.rect(x, y, w, h);
        let resp = self.ui.interact(r, self.id("menu", id), Sense::click());
        let pt = self.ui.painter();
        pt.rect_filled(r, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
        pt.with_clip_rect(r).text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, p.choices.get(cur).copied().unwrap_or(""), look::regular(10.5), self.skin.text);
        crate::glyphs::paint(pt, Rect::from_center_size(pos2(r.max.x - 8.0, r.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, self.skin.dim);
        let mut pick = None;
        egui::Popup::menu(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (i, c) in p.choices.iter().enumerate() {
                    if *c == "---" {
                        ui.separator();
                    } else if ui.add(egui::Button::selectable(i == cur, *c)).clicked() {
                        pick = Some(i);
                        ui.close();
                    }
                }
            });
        });
        if let Some(i) = pick {
            self.put(id, i as f32);
        }
    }
    /// A value field: drag up or down, double-click to type.
    pub fn field(&mut self, x: f32, y: f32, w: f32, h: f32, id: u32, step: f32) {
        let p = self.prm(id);
        let mut v = self.get(id);
        let r = self.rect(x, y, w, h);
        self.ui.painter().rect_filled(r, 4.0, Color32::from_gray(0x22));
        let (ps, vs, ts) = (self.params, self.vals, self.texts);
        let fid = self.id("field", id);
        if crate::widgets::value_slot(self.ui, r, fid, &mut v, p.min, p.max, p.default, step, &|v| shown_of(ps, vs, ts, id, v)) {
            self.put(id, v);
        }
    }
    /// A square pad for two parameters (x to the right, y upward); returns its rectangle.
    pub fn xy(&mut self, x: f32, y: f32, w: f32, h: f32, idx: u32, idy: u32) -> Rect {
        let (px, py) = (self.prm(idx), self.prm(idy));
        let r = self.rect(x, y, w, h);
        let resp = self.ui.interact(r, self.id("xy", idx), Sense::click_and_drag());
        if (resp.dragged() || resp.clicked())
            && let Some(pp) = resp.interact_pointer_pos()
        {
            let fx = ((pp.x - r.min.x) / r.width()).clamp(0.0, 1.0);
            let fy = ((r.max.y - pp.y) / r.height()).clamp(0.0, 1.0);
            self.put(idx, px.min + fx * (px.max - px.min));
            self.put(idy, py.min + fy * (py.max - py.min));
        }
        let pt = self.ui.painter();
        pt.rect_filled(r, 5.0, Color32::from_gray(0x16));
        pt.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, Color32::from_gray(0x30)));
        pt.line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, Color32::from_gray(0x30)));
        let fx = (self.get(idx) - px.min) / (px.max - px.min).max(1e-9);
        let fy = (self.get(idy) - py.min) / (py.max - py.min).max(1e-9);
        pt.circle_filled(pos2(r.min.x + fx * r.width(), r.max.y - fy * r.height()), 5.0, self.skin.accent);
        r
    }
    /// A dark display area; returns its rectangle.
    pub fn display(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let r = self.rect(x, y, w, h);
        self.ui.painter().rect_filled(r, 5.0, Color32::from_gray(0x12));
        r
    }
    /// A curve through points given as fractions of the display (x right, y up).
    pub fn curve(&self, r: Rect, pts: &[(f32, f32)], col: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| pos2(r.min.x + x * r.width(), r.max.y - y.clamp(0.0, 1.0) * r.height())).collect();
        self.ui.painter().with_clip_rect(r).add(egui::Shape::line(v, Stroke::new(1.5, col)));
    }
}

/// A parameter's value as text: the instrument's own wording for the current value when it
/// has one, percent for plain 0…1 amounts, else by the parameter's unit.
pub fn shown_of(params: &[Param], vals: &[f32], texts: &[Option<String>], id: u32, v: f32) -> String {
    let Some(i) = params.iter().position(|p| p.id == id) else { return String::new() };
    if (v - vals[i]).abs() < 1e-6
        && let Some(Some(t)) = texts.get(i)
    {
        return t.clone();
    }
    let p = &params[i];
    if p.unit == lg_dsp::Unit::None && p.min >= -1.0 && p.max <= 1.0 {
        return format!("{:.0} %", v * 100.0);
    }
    crate::pluginwin::fmt(p, v)
}

/// The settings file's current name by track (shown in the header).
fn setting_name(app: &App, t: usize) -> String {
    instwin::track_id(app, t).and_then(|id| app.inst.synth_setting.get(&id).cloned()).unwrap_or_else(|| "Factory Default".to_string())
}

fn apply_setting(app: &mut App, t: usize, name: &str, pairs: &[(u32, f32)]) {
    instwin::checkpoint(app, t, "Load Setting");
    instwin::set_params(app, t, pairs);
    if let Some(id) = instwin::track_id(app, t) {
        app.inst.synth_setting.insert(id, name.to_string());
    }
}

/// The saved settings of a synth, stepped by `by` from the current one (‹ ›, Next / Previous
/// Plug-in Setting).
pub fn step_setting(app: &mut App, t: usize, plugin: &str, by: i32) {
    let list = crate::pluginwin::saved_settings(plugin);
    if list.is_empty() {
        return;
    }
    let cur = setting_name(app, t);
    let next = match list.iter().position(|(n, _)| *n == cur) {
        Some(i) => (i as i32 + by).rem_euclid(list.len() as i32) as usize,
        None if by > 0 => 0,
        None => list.len() - 1,
    };
    if let Some(pairs) = crate::pluginwin::read_setting(&list[next].1) {
        apply_setting(app, t, &list[next].0.clone(), &pairs);
    }
}

/// A synthesizer's window: the header, then `body` draws the panel of `size`; `ext` are the
/// extended parameters under the disclosure triangle at the lower left. Returns the actions
/// the body asked for.
pub fn window(app: &mut App, ctx: &egui::Context, t: usize, plugin: &str, size: egui::Vec2, skin: Skin, ext: &[u32], body: impl FnOnce(&mut Pane)) -> Vec<String> {
    let Some((params, vals)) = instwin::values(app, t) else { return Vec::new() };
    let texts: Vec<Option<String>> = app.engine.lock().ok().and_then(|e| e.units.get(t).and_then(|u| u.instrument.as_ref()).map(|ins| params.iter().zip(&vals).map(|(p, v)| ins.value_text(p.id, *v)).collect())).unwrap_or_default();
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let cur_setting = setting_name(app, t);
    let saved = crate::pluginwin::saved_settings(plugin);
    let ext_id = egui::Id::unique(("synth-ext", t));
    let mut ext_open: bool = ctx.data(|d| d.get_temp(ext_id)).unwrap_or(false);
    let name_id = egui::Id::unique(("synth-save-name", t));
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut actions: Vec<String> = Vec::new();
    // what the header asks for: 0 default, 1 save(name), 2 load(i), 3 prev, 4 next, 5 copy, 6 paste, 7 compare, 8 bypass
    let mut head: Option<(u8, String)> = None;
    let mut open = true;
    let bypassed = app.plugin_bypass.contains(&t);
    let comparing = app.plugin_compare.contains_key(&t);
    let resp = egui::Window::new(format!("{track} — {plugin}")).id(egui::Id::unique(("synth-window", t))).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
        let (hr, _) = ui.allocate_exact_size(vec2(size.x, 28.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(hr, 6.0, look::header_bg());
        let br = Rect::from_center_size(pos2(hr.min.x + 16.0, hr.center().y), vec2(22.0, 22.0));
        crate::glyphs::paint(&p, br, crate::glyphs::ph::POWER, 18.0, if bypassed { look::text_dim() } else { look::accent() });
        if ui.interact(br, egui::Id::unique(("synth-bypass", t)), Sense::click()).on_hover_text("Bypass").clicked() {
            head = Some((8, String::new()));
        }
        // the Setting pop-up
        let sr = Rect::from_min_size(pos2(hr.min.x + 34.0, hr.min.y + 4.0), vec2(190.0, 20.0));
        let sresp = ui.interact(sr, egui::Id::unique(("synth-setting", t)), Sense::click());
        p.rect_filled(sr, 5.0, if sresp.hovered() { look::button_on() } else { look::group_bg() });
        p.with_clip_rect(sr).text(pos2(sr.min.x + 8.0, sr.center().y), Align2::LEFT_CENTER, &cur_setting, look::regular(12.0), look::text());
        crate::glyphs::paint(&p, Rect::from_center_size(pos2(sr.max.x - 10.0, sr.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 10.0, look::text_dim());
        egui::Popup::menu(&sresp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            if ui.button("Recall Default").clicked() {
                head = Some((0, String::new()));
                ui.close();
            }
            ui.menu_button("Save Setting As…", |ui| {
                let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
                let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Setting name").desired_width(160.0));
                let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                    head = Some((1, name.trim().to_string()));
                    name.clear();
                    ui.close();
                }
                ui.data_mut(|d| d.insert_temp(name_id, name));
            });
            ui.separator();
            if ui.button("Next Setting").clicked() {
                head = Some((4, String::new()));
                ui.close();
            }
            if ui.button("Previous Setting").clicked() {
                head = Some((3, String::new()));
                ui.close();
            }
            if ui.button("Copy Setting").clicked() {
                head = Some((5, String::new()));
                ui.close();
            }
            if ui.button("Paste Setting").clicked() {
                head = Some((6, String::new()));
                ui.close();
            }
            if !saved.is_empty() {
                ui.separator();
            }
            for (i, (n, _)) in saved.iter().enumerate() {
                if ui.add(egui::Button::selectable(*n == cur_setting, n.as_str())).clicked() {
                    head = Some((2, i.to_string()));
                    ui.close();
                }
            }
        });
        let mut x = sr.max.x + 6.0;
        for (icon, act, tip) in [(crate::glyphs::ph::CARET_LEFT, 3u8, "Previous Setting"), (crate::glyphs::ph::CARET_RIGHT, 4, "Next Setting")] {
            let r = Rect::from_min_size(pos2(x, hr.min.y + 4.0), vec2(20.0, 20.0));
            let rs = ui.interact(r, egui::Id::unique(("synth-step", t, act)), Sense::click()).on_hover_text(tip);
            p.rect_filled(r, 5.0, if rs.hovered() { look::button_on() } else { look::group_bg() });
            crate::glyphs::paint(&p, r, icon, 11.0, look::text());
            if rs.clicked() {
                head = Some((act, String::new()));
            }
            x += 24.0;
        }
        let mut x = hr.max.x - 8.0;
        for (label, act) in [("Paste", 6u8), ("Copy", 5), ("Compare", 7)] {
            let w = p.layout_no_wrap(label.into(), look::regular(12.0), look::text()).size().x + 16.0;
            let r = Rect::from_min_max(pos2(x - w, hr.min.y + 4.0), pos2(x, hr.max.y - 4.0));
            x -= w + 6.0;
            let rs = ui.interact(r, egui::Id::unique(("synth-head", t, act)), Sense::click());
            p.rect_filled(r, 5.0, if rs.hovered() || (act == 7 && comparing) { look::button_on() } else { look::group_bg() });
            p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), look::text());
            if rs.clicked() {
                head = Some((act, String::new()));
            }
        }
        // the panel
        let (pr, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(pr, 8.0, skin.bg);
        let mut pane = Pane { ui, o: pr.min, params: &params, vals: &vals, texts: &texts, set: Vec::new(), skin, t, actions: Vec::new() };
        body(&mut pane);
        set = std::mem::take(&mut pane.set);
        actions = std::mem::take(&mut pane.actions);
        // extended parameters
        if !ext.is_empty() {
            let (dr, _) = ui.allocate_exact_size(vec2(size.x, 18.0), Sense::hover());
            let tr = Rect::from_min_size(dr.min, vec2(170.0, 18.0));
            let rs = ui.interact(tr, egui::Id::unique(("synth-ext-btn", t)), Sense::click());
            crate::glyphs::paint(ui.painter(), Rect::from_min_size(tr.min, vec2(16.0, 18.0)), if ext_open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 11.0, look::text_dim());
            ui.painter().text(pos2(tr.min.x + 18.0, tr.center().y), Align2::LEFT_CENTER, "Extended Parameters", look::regular(11.5), look::text_dim());
            if rs.clicked() {
                ext_open = !ext_open;
            }
            if ext_open {
                let list: Vec<Param> = ext.iter().filter_map(|id| params.iter().find(|p| p.id == *id).cloned()).collect();
                let lv: Vec<f32> = list.iter().map(|p| params.iter().position(|q| q.id == p.id).map(|i| vals[i]).unwrap_or(0.0)).collect();
                let lt: Vec<Option<String>> = list.iter().map(|p| params.iter().position(|q| q.id == p.id).and_then(|i| texts.get(i).cloned().flatten())).collect();
                ui.allocate_ui(vec2(size.x.min(460.0), 0.0), |ui| {
                    set.extend(crate::pluginwin::controls(ui, &list, &lv, &lt, "synth-ext"));
                });
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(ext_id, ext_open));
    app.inst.rect = resp.map(|r| r.response.rect);
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, &format!("{plugin} Setting"));
        instwin::set_params(app, t, &set);
    }
    match head {
        Some((0, _)) => {
            let d: Vec<(u32, f32)> = params.iter().map(|p| (p.id, p.default)).collect();
            apply_setting(app, t, "Factory Default", &d);
        }
        Some((1, name)) => {
            if crate::pluginwin::write_setting(plugin, &name, &params, &vals).is_some() {
                if let Some(id) = instwin::track_id(app, t) {
                    app.inst.synth_setting.insert(id, name.clone());
                }
                app.tell(format!("Saved the setting “{name}”"));
            } else {
                app.tell("Couldn't save the setting");
            }
        }
        Some((2, i)) => {
            if let Some((n, path)) = i.parse::<usize>().ok().and_then(|i| saved.get(i)).cloned()
                && let Some(pairs) = crate::pluginwin::read_setting(&path)
            {
                apply_setting(app, t, &n, &pairs);
            }
        }
        Some((3, _)) => step_setting(app, t, plugin, -1),
        Some((4, _)) => step_setting(app, t, plugin, 1),
        Some((5, _)) => app.plugin_clipboard = Some(vals.clone()),
        Some((6, _)) => {
            if let Some(v) = app.plugin_clipboard.clone().filter(|v| v.len() == params.len()) {
                let pairs: Vec<(u32, f32)> = params.iter().zip(v).map(|(p, v)| (p.id, v)).collect();
                instwin::checkpoint(app, t, "Paste Setting");
                instwin::set_params(app, t, &pairs);
            }
        }
        Some((7, _)) => {
            // swap the settings with the ones kept at the last Compare
            if let Some(v) = app.plugin_compare.insert(t, vals.clone()) {
                let pairs: Vec<(u32, f32)> = params.iter().zip(v).map(|(p, v)| (p.id, v)).collect();
                instwin::set_params(app, t, &pairs);
            }
        }
        Some((8, _)) => {
            if !app.plugin_bypass.remove(&t) {
                app.plugin_bypass.insert(t);
            }
        }
        _ => {}
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
    actions
}
