//! ES2's window, laid out as Logic's: the global parameters at the left, the three
//! oscillators with the mix Triangle, the round filter section (Filter 1 with its Lo / Hi /
//! Peak / BR / BP buttons, Filter 2 with its slope buttons and Fat, Filter Blend, Drive, Filter
//! FM, the Series / Parallel button, three chain symbols that drag two values at once), the
//! Planar Pad at the top right, Volume, Sine Level and the effects beside it; the dark strip
//! across the centre shared by the router and the Vector Envelope (buttons at its right end);
//! LFOs and envelopes under it; the gray strip at the bottom with the macro controls or the
//! controller assignments (Macro / MIDI / Macro only).
//!
//! Router: Target, via and Source pop-ups per slot, the Intensity slider (two arrows once a via
//! source is chosen; the part between drags both), the zero symbol, b/p; Control-click the
//! Router button for Copy / Paste / Clear Matrix. Vector Envelope: click a point to select it,
//! Shift-click between two points to add one, Control-click a point to delete it, click the
//! strip above a point for the Sustain point (S) and the strip below for the Loop point (L),
//! drag a time up or down (Control-drag: the next point makes up for it), Control-click for
//! the shortcut menu. Option-click the Triangle or the Planar Pad to centre its pointer.
//! Digiwaves: drag the Sine label up or down, Control-click it for the menu, Shift-click to
//! type a number.

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_synth::es2::{self, VecEdit, VEC_POINTS};

use crate::app::App;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x3b4250, 0x2d3340, 0x62c7e8);
const TURQ: Color32 = rgb(0x39b8b0);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let macro_only = crate::instwin::values(app, t).and_then(|(p, v)| p.iter().position(|x| x.id == 193).map(|i| v[i].round() as i32 == 2)).unwrap_or(false);
    let size = if macro_only { vec2(1100.0, 74.0) } else { vec2(1100.0, 650.0) };
    synthui::window(app, ctx, t, "ES2", size, SKIN, &[184, 185], move |p| if macro_only { bottom(p, 8.0) } else { body(p) });
}

fn body(p: &mut Pane) {
    global(p);
    oscillators(p);
    filters(p);
    pad_and_output(p);
    // the dark strip: router or Vector Envelope
    let strip = p.rect(10.0, 308.0, 1080.0, 140.0);
    p.ui.painter().rect_filled(strip, 7.0, rgb(0x1c2028));
    let vec_view = p.geti(146) == 1;
    let rb = p.rect(1036.0, 316.0, 48.0, 22.0);
    let resp = p.ui.interact(rb, egui::Id::unique(("es2-router-btn", p.t)), Sense::click());
    p.ui.painter().rect_filled(rb, 4.0, if !vec_view { p.skin.accent } else { Color32::from_gray(0x3c) });
    p.ui.painter().text(rb.center(), Align2::CENTER_CENTER, "Router", look::regular(10.5), if !vec_view { Color32::from_gray(0x10) } else { p.skin.text });
    if resp.clicked() && !p.ui.input(|i| i.modifiers.ctrl) {
        p.put(146, 0.0);
    }
    matrix_menu(p, &resp);
    let vb = p.rect(1036.0, 342.0, 48.0, 22.0);
    if p.button(vb, "Vector", vec_view, "vecbtn") {
        p.put(146, 1.0);
    }
    if vec_view {
        vector_envelope(p);
    } else {
        router(p);
    }
    modulators(p);
    bottom(p, 584.0);
}

fn row(p: &Pane, x: f32, y: f32, label: &str) {
    p.text(x, y + 10.0, Align2::LEFT_CENTER, label, 10.5, p.skin.dim);
}

fn global(p: &mut Pane) {
    p.group(10.0, 10.0, 142.0, 292.0, "GLOBAL");
    let (x, w) = (20.0, 60.0);
    row(p, x, 30.0, "Tune");
    p.field(x + 62.0, 30.0, w, 20.0, 2, 1.0);
    row(p, x, 54.0, "Analog");
    p.field(x + 62.0, 54.0, w, 20.0, 3, 0.01);
    row(p, x, 78.0, "CBD");
    p.menu(x + 62.0, 78.0, w, 20.0, 10);
    row(p, x, 102.0, "Glide");
    p.field(x + 62.0, 102.0, w, 20.0, 7, 0.01);
    row(p, x, 126.0, "Bend range");
    p.field(x + 62.0, 126.0, 28.0, 20.0, 167, 1.0);
    p.field(x + 94.0, 126.0, 28.0, 20.0, 6, 1.0);
    row(p, x, 150.0, "Voices");
    p.field(x + 62.0, 150.0, w, 20.0, 1, 1.0);
    row(p, x, 174.0, "Osc Start");
    p.menu(x + 62.0, 174.0, w, 20.0, 5);
    p.seg(x, 204.0, 124.0, 22.0, 8, es2::KB_MODES, false);
    p.toggle(x, 232.0, 60.0, 22.0, 9, "Unison");
    p.toggle(x + 64.0, 232.0, 60.0, 22.0, 4, "Flt Reset");
}

fn oscillators(p: &mut Pane) {
    p.group(160.0, 10.0, 440.0, 292.0, "OSCILLATORS");
    for o in 0..3u32 {
        let b = 11 + o * 5;
        let y = 34.0 + o as f32 * 88.0;
        // fine frequency: semitones (s) and cents (c)
        p.field(172.0, y + 8.0, 52.0, 20.0, b + 1, 1.0);
        p.field(172.0, y + 32.0, 52.0, 20.0, b + 2, 1.0);
        p.knob_menu(270.0, y, 54.0, b + 3, "Wave");
        let wave = p.geti(b + 3);
        let names = [es2::OSC1_WAVES, es2::OSC2_WAVES, es2::OSC3_WAVES][o as usize];
        p.text(270.0, y + 76.0, Align2::CENTER_CENTER, names.get(wave).copied().unwrap_or(""), 10.0, p.skin.text);
        // the Sine label: the Digiwave
        if wave == 0 {
            digiwave(p, 310.0, y + 20.0, b + 4);
        }
        // the ring round the Wave knob: FM amount (oscillator 1), base pulse width (2 and 3)
        if o == 0 && wave == 4 {
            p.knob(390.0, y + 4.0, 40.0, 190, "FM");
        } else if o > 0 && (wave == 3 || wave == 4 || wave == 6 && o == 1) {
            p.knob(390.0, y + 4.0, 40.0, 190 + o, "Width");
        }
        // the numbered on / off button, green when on
        let on = p.on(b);
        let r = p.rect(430.0, y + 16.0, 24.0, 24.0);
        let resp = p.ui.interact(r, egui::Id::unique(("es2-osc-on", p.t, o)), Sense::click());
        p.ui.painter().rect_filled(r, 5.0, if on { rgb(0x4fc46a) } else { Color32::from_gray(0x50) });
        p.ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("{}", o + 1), look::semibold(12.0), Color32::from_gray(0x10));
        if resp.clicked() {
            p.put(b, if on { 0.0 } else { 1.0 });
        }
    }
    triangle(p, 470.0, 70.0, 120.0);
}

/// The Sine label of an oscillator: its Digiwave (Sine = none). Drag up or down to step
/// through them, Control-click (or right-click) for the menu, Shift-click to type a number.
fn digiwave(p: &mut Pane, x: f32, y: f32, id: u32) {
    p.field(x, y, 46.0, 20.0, id, 1.0);
    let r = p.rect(x, y, 46.0, 20.0);
    let resp = p.ui.interact(r, egui::Id::unique(("es2-digi", p.t, id)), Sense::click());
    if resp.clicked() && p.ui.input(|i| i.modifiers.shift) {
        let fid = p.id("field", id).with("typing");
        let cur = format!("{:.0}", p.get(id));
        p.ui.data_mut(|d| d.insert_temp(fid, cur));
    }
    let mut pick = None;
    let cur = p.geti(id);
    resp.context_menu(|ui| {
        if ui.add(egui::Button::selectable(cur == 0, "Sine")).clicked() {
            pick = Some(0);
            ui.close();
        }
        for g in 0..5 {
            ui.menu_button(format!("Digiwaves {}–{}", g * 20 + 1, g * 20 + 20), |ui| {
                for k in g * 20 + 1..=g * 20 + 20 {
                    if ui.add(egui::Button::selectable(cur == k, format!("{k}"))).clicked() {
                        pick = Some(k);
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some(k) = pick {
        p.put(id, k as f32);
    }
}

/// Whether the Vector Envelope owns the Triangle / the Planar Pad, and the selected point's
/// parameter base.
fn vec_state(p: &Pane) -> (bool, bool, u32) {
    let mode = p.geti(142);
    (mode == 1 || mode == 3, mode == 2 || mode == 3, VEC_POINTS + p.geti(133) as u32 * 5)
}

/// The mix Triangle: oscillator 1 at the top, 2 at the lower left, 3 at the lower right.
fn triangle(p: &mut Pane, x: f32, y: f32, w: f32) {
    let h = w * 0.9;
    let (a, b, c) = (p.o + vec2(x + w / 2.0, y), p.o + vec2(x, y + h), p.o + vec2(x + w, y + h));
    let (vmix, _, pb) = vec_state(p);
    let (ix, iy) = if vmix { (pb + 1, pb + 2) } else { (26, 27) };
    let (mx, my) = (p.get(ix), p.get(iy));
    // weights: osc 1 = y, osc 2 = (1 − y)(1 − x), osc 3 = (1 − y) x
    let (w1, w2, w3) = (my, (1.0 - my) * (1.0 - mx), (1.0 - my) * mx);
    let at = pos2(a.x * w1 + b.x * w2 + c.x * w3, a.y * w1 + b.y * w2 + c.y * w3);
    let r = Rect::from_min_max(pos2(b.x, a.y), c).expand(6.0);
    let resp = p.ui.interact(r, egui::Id::unique(("es2-triangle", p.t)), Sense::click_and_drag());
    let pt = p.painter();
    pt.add(egui::Shape::convex_polygon(vec![a, b, c], Color32::from_gray(0x16), Stroke::new(1.0, Color32::from_gray(0x50))));
    for (pos, n, off) in [(a, "1", vec2(0.0, -9.0)), (b, "2", vec2(-8.0, 6.0)), (c, "3", vec2(8.0, 6.0))] {
        pt.text(pos + off, Align2::CENTER_CENTER, n, look::regular(10.5), p.skin.dim);
    }
    pt.circle_filled(at, 5.0, if vmix { TURQ } else { p.skin.accent });
    if resp.clicked() && p.ui.input(|i| i.modifiers.alt) {
        // Option-click: the centre, all three oscillators at the same level
        p.put(ix, 0.5);
        p.put(iy, 1.0 / 3.0);
        if vmix {
            p.put(26, 0.5);
            p.put(27, 1.0 / 3.0);
        }
    } else if (resp.dragged() || resp.clicked())
        && let Some(pp) = resp.interact_pointer_pos()
    {
        // barycentric coordinates of the pointer, kept inside the triangle
        let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        let mut l1 = ((b.y - c.y) * (pp.x - c.x) + (c.x - b.x) * (pp.y - c.y)) / det;
        let mut l2 = ((c.y - a.y) * (pp.x - c.x) + (a.x - c.x) * (pp.y - c.y)) / det;
        let mut l3 = 1.0 - l1 - l2;
        l1 = l1.max(0.0);
        l2 = l2.max(0.0);
        l3 = l3.max(0.0);
        let s = (l1 + l2 + l3).max(1e-6);
        let (l1, l2, l3) = (l1 / s, l2 / s, l3 / s);
        let ny = l1;
        let nx = if l2 + l3 > 1e-6 { l3 / (l2 + l3) } else { 0.5 };
        p.put(ix, nx);
        p.put(iy, ny);
        if vmix {
            p.put(26, nx);
            p.put(27, ny);
        }
    }
}

/// A chain symbol: drag it to change two values at once (`v` up / down, `h` left / right).
fn chain(p: &mut Pane, x: f32, y: f32, v: u32, h: u32, salt: u32) {
    let r = Rect::from_center_size(p.o + vec2(x, y), vec2(18.0, 14.0));
    let resp = p.ui.interact(r, egui::Id::unique(("es2-chain", p.t, salt)), Sense::drag());
    crate::glyphs::paint(p.ui.painter(), r, crate::glyphs::ph::LINK, 13.0, if resp.hovered() || resp.dragged() { Color32::WHITE } else { p.skin.dim });
    if resp.dragged() {
        let d = resp.drag_delta();
        if d.y != 0.0 {
            let c = (p.get(v) - d.y / 150.0).clamp(0.0, 1.0);
            p.put(v, c);
        }
        if d.x != 0.0 {
            let c = (p.get(h) + d.x / 150.0).clamp(0.0, 1.0);
            p.put(h, c);
        }
        p.ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
}

fn filters(p: &mut Pane) {
    p.group(608.0, 10.0, 250.0, 292.0, "");
    // the Filter button (the whole section on or off) and the configuration button
    p.toggle(616.0, 16.0, 50.0, 20.0, 115, "Filter");
    let parallel = p.geti(29) == 1;
    let r = p.rect(790.0, 16.0, 60.0, 20.0);
    if p.button(r, if parallel { "Parallel" } else { "Series" }, false, "cfg") {
        p.put(29, if parallel { 0.0 } else { 1.0 });
    }
    // the round element, turned to show the signal flow: left to right in series, top to bottom in parallel
    let c = p.o + vec2(733.0, 160.0);
    let pt = p.painter();
    pt.circle_filled(c, 112.0, Color32::from_gray(0x15));
    pt.circle_stroke(c, 112.0, Stroke::new(1.0, Color32::from_gray(0x44)));
    let arrow = |from: Pos2, to: Pos2| {
        pt.line_segment([from, to], Stroke::new(1.5, TURQ));
        let d = (to - from).normalized();
        let n = vec2(-d.y, d.x);
        pt.add(egui::Shape::convex_polygon(vec![to, to - d * 7.0 + n * 4.0, to - d * 7.0 - n * 4.0], TURQ, Stroke::NONE));
    };
    if parallel {
        arrow(c + vec2(-45.0, -104.0), c + vec2(-45.0, -84.0));
        arrow(c + vec2(45.0, -104.0), c + vec2(45.0, -84.0));
    } else {
        arrow(c + vec2(-18.0, -92.0), c + vec2(18.0, -92.0));
    }
    p.text(733.0, 44.0, Align2::CENTER_CENTER, "Blend", 10.5, p.skin.dim);
    p.hfader(668.0, 52.0, 130.0, 16.0, 30);
    // Filter 1
    p.text(690.0, 86.0, Align2::CENTER_CENTER, "FILTER 1", 10.0, p.skin.dim);
    p.knob(690.0, 96.0, 46.0, 32, "Cut");
    p.knob(690.0, 176.0, 40.0, 33, "Res");
    chain(p, 690.0, 166.0, 32, 33, 1);
    p.seg(622.0, 100.0, 34.0, 110.0, 31, es2::F1_MODES, true);
    // Filter 2
    p.text(776.0, 86.0, Align2::CENTER_CENTER, "FILTER 2", 10.0, p.skin.dim);
    p.knob(776.0, 96.0, 46.0, 36, "Cut");
    p.knob(776.0, 176.0, 40.0, 37, "Res");
    chain(p, 776.0, 166.0, 36, 37, 2);
    p.seg(812.0, 100.0, 40.0, 66.0, 34, &["12dB", "18dB", "24dB"], true);
    p.toggle(812.0, 170.0, 38.0, 20.0, 35, "fat");
    // the chain between the two cutoffs: Filter 1 up / down, Filter 2 left / right
    chain(p, 733.0, 118.0, 32, 36, 3);
    p.knob(660.0, 244.0, 38.0, 28, "Drive");
    p.knob(806.0, 244.0, 38.0, 38, "FM");
}

fn pad_and_output(p: &mut Pane) {
    p.group(866.0, 10.0, 124.0, 292.0, "");
    // the Planar Pad
    let (_, vxy, pb) = vec_state(p);
    let (ix, iy) = if vxy { (pb + 3, pb + 4) } else { (122, 123) };
    let r = p.xy(874.0, 18.0, 108.0, 108.0, ix, iy);
    let resp = p.ui.interact(r, egui::Id::unique(("es2-pad-alt", p.t)), Sense::click());
    if resp.clicked() && p.ui.input(|i| i.modifiers.alt) {
        // Option-click: both axes to zero
        p.put(ix, 0.0);
        p.put(iy, 0.0);
    }
    if vxy {
        let (x, y) = (p.get(ix), p.get(iy));
        p.put(122, x);
        p.put(123, y);
        p.set.retain(|s| !(s.0 == 122 || s.0 == 123) || true);
    }
    row(p, 874.0, 132.0, "X");
    p.menu(888.0, 132.0, 58.0, 20.0, 124);
    p.field(948.0, 132.0, 36.0, 20.0, 125, 0.01);
    row(p, 874.0, 156.0, "Y");
    p.menu(888.0, 156.0, 58.0, 20.0, 126);
    p.field(948.0, 156.0, 36.0, 20.0, 127, 0.01);
    row(p, 874.0, 184.0, "Vector Mode");
    p.menu(874.0, 204.0, 108.0, 20.0, 142);
    // Randomize
    let r = p.rect(874.0, 234.0, 40.0, 22.0);
    if p.button(r, "RND", false, "rnd") {
        let seed = (p.ui.input(|i| i.time) * 1000.0) as u32;
        let (int, dest) = (p.get(144), p.geti(145));
        let new = es2::randomize(&|id| p.get(id), int, dest, seed);
        for (id, v) in new {
            p.put(id, v);
        }
    }
    p.hfader(918.0, 236.0, 66.0, 18.0, 144);
    p.menu(874.0, 262.0, 108.0, 20.0, 145);

    p.group(998.0, 10.0, 92.0, 292.0, "");
    p.knob(1022.0, 16.0, 36.0, 40, "Volume");
    p.knob(1066.0, 16.0, 36.0, 39, "Sine Lvl");
    p.text(1044.0, 82.0, Align2::CENTER_CENTER, "DISTORTION", 10.0, p.skin.dim);
    p.seg(1004.0, 92.0, 80.0, 18.0, 116, es2::DIST_TYPES, false);
    p.knob(1022.0, 114.0, 34.0, 117, "Dist");
    p.knob(1066.0, 114.0, 34.0, 118, "Tone");
    p.seg(1004.0, 176.0, 80.0, 54.0, 119, es2::FXMOD_TYPES, true);
    p.knob(1022.0, 236.0, 34.0, 121, "Intensity");
    p.knob(1066.0, 236.0, 34.0, 120, "Speed");
}

/// Control-click on the Router button: Copy Matrix, Paste Matrix, Clear Matrix.
fn matrix_menu(p: &mut Pane, resp: &egui::Response) {
    let clip_id = egui::Id::new("es2-matrix-clip");
    let ids: Vec<u32> = (41..=90).chain(147..=166).collect();
    let mut act = 0;
    resp.context_menu(|ui| {
        if ui.button("Copy Matrix").clicked() {
            act = 1;
            ui.close();
        }
        if ui.button("Paste Matrix").clicked() {
            act = 2;
            ui.close();
        }
        if ui.button("Clear Matrix").clicked() {
            act = 3;
            ui.close();
        }
    });
    match act {
        1 => {
            let v: Vec<f32> = ids.iter().map(|id| p.get(*id)).collect();
            p.ui.ctx().data_mut(|d| d.insert_temp(clip_id, v));
        }
        2 => {
            if let Some(v) = p.ui.ctx().data(|d| d.get_temp::<Vec<f32>>(clip_id)) {
                for (id, x) in ids.iter().zip(v) {
                    p.put(*id, x);
                }
            }
        }
        3 => {
            // every routing initialised and turned off
            for k in 0..10u32 {
                for (o, v) in [(0, 0.0), (1, 0.0), (2, 0.0), (3, 0.0), (4, 0.0)] {
                    p.put(41 + k * 5 + o, v);
                }
                p.put(147 + k * 2, 0.0);
                p.put(148 + k * 2, 0.0);
            }
        }
        _ => {}
    }
}

fn router(p: &mut Pane) {
    for k in 0..10u32 {
        let b = 41 + k * 5;
        let x = 16.0 + k as f32 * 101.5;
        let active = p.on(147 + k * 2);
        p.text(x, 320.0, Align2::LEFT_CENTER, &format!("{}", k + 1), 10.0, p.skin.dim);
        // b/p: off keeps the routing
        let r = p.rect(x + 58.0, 312.0, 22.0, 15.0);
        if p.button(r, "b/p", !active, ("bp", k)) {
            p.put(147 + k * 2, if active { 0.0 } else { 1.0 });
        }
        p.menu(x, 332.0, 80.0, 19.0, b);
        p.menu(x, 356.0, 62.0, 19.0, b + 2);
        // the zero symbol beside via: intensity to zero
        let z = p.rect(x + 64.0, 356.0, 16.0, 19.0);
        if p.button(z, "0", false, ("zero", k)) {
            p.put(b + 3, 0.0);
            p.put(b + 4, 0.0);
        }
        p.menu(x, 380.0, 80.0, 19.0, b + 1);
        p.text(x - 1.0, 408.0, Align2::LEFT_CENTER, "Target / via / Source", 8.5, Color32::from_gray(0x60));
        // Intensity: one value, or two halves once a via source is chosen
        if p.geti(b + 2) == 0 {
            p.vfader(x + 82.0, 314.0, 16.0, 116.0, b + 3, "");
        } else {
            p.range(x + 80.0, 314.0, 20.0, 116.0, b + 3, b + 4, "", rgb(0xe8a23a));
        }
        let amt = p.shown(b + 3, p.get(b + 3));
        p.text(x, 426.0, Align2::LEFT_CENTER, &amt, 10.0, if active { p.skin.text } else { p.skin.dim });
    }
}

fn vector_envelope(p: &mut Pane) {
    let get = |p: &Pane, id: u32| p.get(id);
    let pts = es2::vec_points(&|id| p.get(id));
    let n = pts.len();
    let (sel, sus, lp) = (p.geti(133).min(n - 1), p.geti(136).min(n - 1), p.geti(140).min(n - 1));
    let looping = p.geti(138) != 0;
    let total: f32 = pts.iter().skip(1).map(|q| q[0]).sum::<f32>().max(0.05);
    let area = p.rect(18.0, 332.0, 730.0, 62.0);
    let x_of = |k: usize| -> f32 {
        let acc: f32 = pts.iter().take(k + 1).skip(1).map(|q| q[0]).sum();
        area.min.x + 14.0 + (area.width() - 28.0) * acc / total
    };
    let pt = p.painter();
    pt.rect_filled(area, 4.0, Color32::from_gray(0x10));
    let top = Rect::from_min_max(pos2(area.min.x, area.min.y - 14.0), pos2(area.max.x, area.min.y));
    let bot = Rect::from_min_max(pos2(area.min.x, area.max.y), pos2(area.max.x, area.max.y + 14.0));
    pt.rect_filled(top, 0.0, TURQ.gamma_multiply(0.35));
    pt.rect_filled(bot, 0.0, TURQ.gamma_multiply(0.35));
    // the looped region between the Sustain and Loop points
    if looping && sus != lp {
        let (a, b) = (x_of(sus.min(lp)), x_of(sus.max(lp)));
        pt.rect_filled(Rect::from_min_max(pos2(a, area.min.y), pos2(b, area.max.y)), 0.0, TURQ.gamma_multiply(0.15));
    }
    let line: Vec<Pos2> = (0..n).map(|k| pos2(x_of(k), area.center().y)).collect();
    pt.add(egui::Shape::line(line, Stroke::new(1.5, Color32::from_gray(0x70))));
    let ctrl = p.ui.input(|i| i.modifiers.ctrl);
    let shift = p.ui.input(|i| i.modifiers.shift);
    let mut edit: Option<VecEdit> = None;
    let mut select: Option<usize> = None;
    for k in 0..n {
        let c = pos2(x_of(k), area.center().y);
        let r = Rect::from_center_size(c, vec2(18.0, 18.0));
        let resp = p.ui.interact(r, egui::Id::unique(("es2-vpoint", p.t, k)), Sense::click());
        pt.circle_filled(c, 8.0, if k == sel { TURQ } else { Color32::from_gray(0x48) });
        pt.text(c, Align2::CENTER_CENTER, format!("{}", k + 1), look::regular(10.0), if k == sel { Color32::from_gray(0x10) } else { Color32::WHITE });
        if k == sus {
            pt.text(pos2(c.x, top.center().y), Align2::CENTER_CENTER, "S", look::semibold(10.5), Color32::WHITE);
        }
        if k == lp && looping {
            pt.text(pos2(c.x, bot.center().y), Align2::CENTER_CENTER, "L", look::semibold(10.5), Color32::WHITE);
        }
        if resp.clicked() {
            if ctrl {
                edit = Some(VecEdit::Delete(k));
            } else {
                select = Some(k);
            }
        }
        // the strips: above = Sustain point, below = Loop point
        let rs = p.ui.interact(Rect::from_center_size(pos2(c.x, top.center().y), vec2(18.0, 14.0)), egui::Id::unique(("es2-vsus", p.t, k)), Sense::click());
        if rs.clicked() {
            p.put(136, k as f32);
        }
        let rl = p.ui.interact(Rect::from_center_size(pos2(c.x, bot.center().y), vec2(18.0, 14.0)), egui::Id::unique(("es2-vloop", p.t, k)), Sense::click());
        if rl.clicked() {
            p.put(140, k as f32);
        }
        // the time from the point before (milliseconds, or a share of the loop when its rate is set)
        if k > 0 {
            let tr = Rect::from_center_size(pos2((x_of(k - 1) + c.x) / 2.0, bot.max.y + 12.0), vec2(48.0, 16.0));
            let rt = p.ui.interact(tr, egui::Id::unique(("es2-vtime", p.t, k)), Sense::drag());
            let in_loop = looping && k > sus.min(lp) && k <= sus.max(lp);
            let text = if in_loop && get(p, 132).abs() > 0.001 {
                let sum: f32 = (sus.min(lp) + 1..=sus.max(lp)).map(|j| pts[j][0]).sum();
                format!("{:.0} %", pts[k][0] / sum.max(1e-6) * 100.0)
            } else {
                format!("{:.0} ms", pts[k][0] * 1000.0)
            };
            pt.text(tr.center(), Align2::CENTER_CENTER, text, look::regular(10.0), if rt.hovered() || rt.dragged() { Color32::WHITE } else { p.skin.text });
            if rt.dragged() && rt.drag_delta().y != 0.0 {
                let t = (pts[k][0] - rt.drag_delta().y * 0.005 * (1.0 + pts[k][0])).clamp(0.0, 10.0);
                edit = Some(VecEdit::SetTime(k, t, ctrl));
            }
        }
    }
    // Shift-click between two points: a new point there
    let bg = p.ui.interact(area, egui::Id::unique(("es2-varea", p.t)), Sense::click());
    if bg.clicked() && shift
        && let Some(pp) = bg.interact_pointer_pos()
    {
        let t = ((pp.x - area.min.x - 14.0) / (area.width() - 28.0)).clamp(0.0, 1.0) * total;
        edit = Some(VecEdit::InsertAt(t));
    }
    // the shortcut menu
    let clip_id = egui::Id::new("es2-vpoint-clip");
    let env_clip = egui::Id::new("es2-venv-clip");
    let mut act = 0;
    bg.context_menu(|ui| {
        for (k, name) in ["Delete Selected Point", "Insert Point to the Right of Selected Point", "Copy Point", "Paste Point", "Paste to All Points", "Set Segments to Average Time", "Init to 8/8 Loop", "Init to 16/16 Loop", "Clear Envelope", "Copy Envelope", "Paste Envelope"].iter().enumerate() {
            if k == 2 || k == 5 || k == 9 {
                ui.separator();
            }
            if ui.button(*name).clicked() {
                act = k + 1;
                ui.close();
            }
        }
    });
    match act {
        1 => edit = Some(VecEdit::Delete(sel)),
        2 => edit = Some(VecEdit::InsertAfter(sel)),
        3 => {
            let q = pts[sel];
            p.ui.ctx().data_mut(|d| d.insert_temp(clip_id, q));
        }
        4 | 5 => {
            if let Some(q) = p.ui.ctx().data(|d| d.get_temp::<es2::VPoint>(clip_id)) {
                edit = Some(if act == 4 { VecEdit::Paste(sel, q) } else { VecEdit::PasteAll(q) });
            }
        }
        6 => edit = Some(VecEdit::AverageTime),
        7 => edit = Some(VecEdit::InitLoop(8)),
        8 => edit = Some(VecEdit::InitLoop(16)),
        9 => edit = Some(VecEdit::Clear),
        10 => {
            let ids: Vec<u32> = [128, 131, 132, 135, 136, 137, 138, 139, 140, 168].into_iter().chain(VEC_POINTS..VEC_POINTS + 80).collect();
            let v: Vec<(u32, f32)> = ids.iter().map(|id| (*id, p.get(*id))).collect();
            p.ui.ctx().data_mut(|d| d.insert_temp(env_clip, v));
        }
        11 => {
            if let Some(v) = p.ui.ctx().data(|d| d.get_temp::<Vec<(u32, f32)>>(env_clip)) {
                for (id, x) in v {
                    p.put(id, x);
                }
            }
        }
        _ => {}
    }
    if let Some(e) = edit {
        let new = es2::vec_edit(&|id| p.get(id), &e);
        for (id, v) in new {
            p.put(id, v);
        }
    }
    if let Some(k) = select {
        // the Triangle and Planar Pad pointers show the selected point (with Solo Point on it
        // is the point that sounds)
        p.put(133, k as f32);
    }
    // ---- the controls at the right
    let x = 760.0;
    p.toggle(x, 314.0, 76.0, 20.0, 134, "Solo Point");
    row(p, x + 84.0, 314.0, "Env Mode");
    p.menu(x + 140.0, 314.0, 70.0, 20.0, 137);
    row(p, x, 340.0, "Curve");
    p.field(x + 40.0, 340.0, 86.0, 20.0, 168, 1.0);
    row(p, x + 134.0, 340.0, "Loop Mode");
    p.menu(x + 194.0, 340.0, 72.0, 20.0, 138);
    row(p, x, 366.0, "Loop Rate");
    p.field(x + 60.0, 366.0, 66.0, 20.0, 132, 0.02);
    let r = p.rect(x + 130.0, 366.0, 44.0, 20.0);
    if p.button(r, "as set", p.get(132).abs() < 0.001, "asset") {
        p.put(132, 0.0);
    }
    row(p, x + 180.0, 366.0, "Count");
    p.field(x + 216.0, 366.0, 50.0, 20.0, 139, 1.0);
    row(p, x, 392.0, "Loop Smooth");
    p.field(x + 70.0, 392.0, 56.0, 20.0, 131, 0.01);
    row(p, x + 134.0, 392.0, "Time Scaling");
    p.field(x + 204.0, 392.0, 62.0, 20.0, 128, 0.05);
    let r = p.rect(x, 418.0, 76.0, 20.0);
    if p.button(r, "Fix Timing", false, "fixt") {
        let new = es2::vec_edit(&|id| p.get(id), &VecEdit::FixTiming);
        for (id, v) in new {
            p.put(id, v);
        }
    }
}

fn modulators(p: &mut Pane) {
    let y = 454.0;
    let lfo_names = ["Tri", "Saw", "Rect", "Rect+", "S&H", "Rnd"];
    p.group(10.0, y, 190.0, 124.0, "LFO 1");
    p.vfader(22.0, y + 22.0, 20.0, 70.0, 93, "EG");
    // the zero under LFO 1 EG turns its envelope off
    let z = p.rect(22.0, y + 106.0, 20.0, 14.0);
    if p.button(z, "0", false, "eg0") {
        p.put(93, 0.0);
    }
    let eg = p.shown(93, p.get(93));
    p.text(48.0, y + 113.0, Align2::LEFT_CENTER, &eg, 9.5, p.skin.text);
    p.knob(82.0, y + 22.0, 44.0, 92, "Rate");
    p.seg(120.0, y + 20.0, 74.0, 96.0, 91, &lfo_names, true);
    p.group(206.0, y, 150.0, 124.0, "LFO 2");
    p.knob(240.0, y + 22.0, 44.0, 95, "Rate");
    let r2 = p.shown(95, p.get(95));
    p.text(240.0, y + 96.0, Align2::CENTER_CENTER, &r2, 10.0, p.skin.text);
    p.seg(276.0, y + 20.0, 74.0, 96.0, 94, &lfo_names, true);

    p.group(362.0, y, 150.0, 124.0, "ENV 1");
    p.menu(410.0, y + 4.0, 62.0, 18.0, 97);
    p.range(376.0, y + 26.0, 26.0, 78.0, 99, 98, "A", rgb(0xe8a23a));
    p.vfader(424.0, y + 26.0, 22.0, 78.0, 100, "");
    // the D / R button: Attack / Decay or Attack / Release
    let rel = p.geti(96) == 1;
    let r = p.rect(420.0, y + 106.0, 30.0, 16.0);
    if p.button(r, if rel { "R" } else { "D" }, false, "e1dr") {
        p.put(96, if rel { 0.0 } else { 1.0 });
    }
    for (e, name, x0) in [(101u32, "ENV 2", 518.0), (108, "ENV 3", 742.0)] {
        p.group(x0, y, 218.0, 124.0, name);
        p.range(x0 + 12.0, y + 26.0, 26.0, 78.0, e + 1, e, "A", rgb(0xe8a23a));
        p.vfader(x0 + 50.0, y + 26.0, 22.0, 78.0, e + 2, "D");
        p.vfader(x0 + 80.0, y + 26.0, 22.0, 78.0, e + 4, "S");
        p.vfader(x0 + 110.0, y + 26.0, 22.0, 78.0, e + 3, "Time");
        p.vfader(x0 + 140.0, y + 26.0, 22.0, 78.0, e + 5, "R");
        p.knob(x0 + 190.0, y + 30.0, 36.0, e + 6, "Vel");
    }
    p.group(966.0, y, 124.0, 124.0, "");
    p.text(976.0, y + 14.0, Align2::LEFT_CENTER, "Selected point", 10.5, p.skin.dim);
    p.field(976.0, y + 26.0, 50.0, 20.0, 133, 1.0);
    p.text(976.0, y + 62.0, Align2::LEFT_CENTER, "Points", 10.5, p.skin.dim);
    p.field(976.0, y + 74.0, 50.0, 20.0, 135, 1.0);
}

/// The gray strip: the macro controls or the controller assignments, and the three view
/// buttons.
fn bottom(p: &mut Pane, y: f32) {
    let strip = p.rect(10.0, y, 1080.0, 58.0);
    p.ui.painter().rect_filled(strip, 6.0, rgb(0x5a6068));
    let view = p.geti(193);
    for (k, name) in es2::BOTTOM_VIEWS.iter().enumerate() {
        let r = p.rect(18.0 + k as f32 * 70.0, y + 18.0, 66.0, 22.0);
        if p.button(r, name, view == k, ("bview", k)) {
            p.put(193, k as f32);
        }
    }
    if view == 1 {
        // the controller assignments: Ctrl A to Ctrl F
        for k in 0..6u32 {
            let x = 250.0 + k as f32 * 138.0;
            p.text(x, y + 29.0, Align2::LEFT_CENTER, &format!("Ctrl {}", ['A', 'B', 'C', 'D', 'E', 'F'][k as usize]), 10.5, Color32::from_gray(0xe8));
            p.menu(x + 40.0, y + 19.0, 84.0, 20.0, 169 + k);
        }
    } else {
        for (k, name) in ["Cutoff", "Reso", "Mod", "Detune", "Wave", "Attack", "Decay", "Sustain", "Release"].iter().enumerate() {
            p.knob(280.0 + k as f32 * 90.0, y + 4.0, 34.0, 175 + k as u32, name);
        }
    }
}
