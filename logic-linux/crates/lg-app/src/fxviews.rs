//! Logic's custom plug-in editors for the effects whose guide pages describe one (the
//! Controls view stays one click away): Remix FX (XY pads, Gater, Downsampler, Reverse /
//! Scratch / Tape Stop), Beat Breaker (edit modes, slices, patterns, Randomize), Step FX (three
//! step modulators, XY pad, effects order), Delay Designer (tap display, Tap pads, tap parameter
//! bar), Pedalboard (Pedal Browser, Pedal area with buses, Macro controls), Mastering Assistant
//! (analysis, meters, spectrum and EQ curves), Space Designer (IR modes, Load IR, the response),
//! Match EQ (learn, curves), Pitch Correction (the keyboard), and the meters (Level, Loudness,
//! Correlation, MultiMeter, BPM Counter, Tuner).
//!
//! Every view edits the plug-in through its parameters (the custom views' stored state is in
//! hidden parameters), so Compare, Copy / Paste, settings and undo work the same as in Controls.

use std::collections::HashMap;

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::app::App;
use crate::look;
use lg_dsp::Param;

/// The effect names Studio now has beyond the first set (the Audio FX menu's entries).
#[allow(dead_code)]
pub const COMMANDS: &[&str] = &[
    "Remix FX", "Beat Breaker", "Step FX", "Spring Box", "Tie Dye Delay", "Candy Fuzz", "Double Dragon", "Fuzz Machine", "Grinder", "Grit", "Happy Face Fuzz", "Hi-Drive", "Monster Fuzz", "OctaFuzz", "Rawk! Distortion", "Tube Burner", "Squash Compressor", "Auto-Funk", "Classic Wah", "Modern Wah", "Graphic EQ", "Flange Factory", "Heavenly Chorus", "Phase Tripper", "Phaze 2", "Retro Chorus", "Roswell Ringer", "Roto Phase", "Spin Box", "The Vibe", "Trem-O-Tone", "Dr. Octave", "Wham",
    "Sample Delay", "Distortion II", "Phase Distortion", "Expander", "Single Band EQ", "Linear Phase EQ", "Match EQ", "Fuzz-Wah", "Spectral Gate", "Direction Mixer", "EVOC 20 Filterbank", "EVOC 20 TrackOscillator", "Modulation Delay", "Ringshifter", "Rotor Cabinet", "Scanner Vibrato", "Spreader", "Pitch Correction", "Pitch Shifter", "Vocal Transformer", "EnVerb", "Quantec Room Simulator", "SilverVerb", "SubBass", "Test Oscillator",
    "BPM Counter", "Correlation Meter", "Level Meter", "Loudness Meter", "MultiMeter", "Tuner",
];

/// What the running effect shows: its readouts and the curves the view asked for.
#[derive(Default)]
pub struct Live {
    pub readouts: Vec<(&'static str, f32)>,
    pub curves: HashMap<String, Vec<f32>>,
}

impl Live {
    fn get(&self, name: &str) -> Option<f32> {
        self.readouts.iter().find(|r| r.0 == name).map(|r| r.1)
    }
    fn curve(&self, name: &str) -> &[f32] {
        self.curves.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Something a view asks the app to do (it needs more than parameters).
pub enum Action {
    /// Mastering Assistant: render the mix (the cycle, if set) and analyze it.
    Reanalyze,
    /// Space Designer: pick a WAV, copy it into the IR folder, select it.
    LoadIr,
    /// Match EQ: learn the current (false) or reference (true) spectrum from an audio file.
    LearnFromFile(bool),
    /// Beat Breaker: save the edited pattern under a name / load a saved one.
    SavePattern(String),
    LoadPattern(std::path::PathBuf),
}

const EDITORS: &[&str] = &["Channel EQ", "Linear Phase EQ", "Remix FX", "Beat Breaker", "Step FX", "Delay Designer", "Pedalboard", "Mastering Assistant", "Space Designer", "Match EQ", "Pitch Correction", "Level Meter", "Loudness Meter", "Correlation Meter", "MultiMeter", "BPM Counter", "Tuner"];

pub fn has_editor(name: &str) -> bool {
    EDITORS.contains(&name)
}

pub fn default_size(name: &str) -> egui::Vec2 {
    match name {
        "Remix FX" | "Beat Breaker" | "Step FX" | "Delay Designer" | "Pedalboard" | "Mastering Assistant" => vec2(760.0, 560.0),
        "Space Designer" | "Match EQ" | "MultiMeter" => vec2(680.0, 520.0),
        // (Logic's Channel EQ window at 100 %: 1028 × 552 points with its header and footer)
        "Channel EQ" | "Linear Phase EQ" => vec2(1014.0, 540.0),
        _ => vec2(520.0, 460.0),
    }
}

pub fn curves_for(name: &str) -> &'static [&'static str] {
    match name {
        "Mastering Assistant" => &["spectrum", "eq"],
        "Space Designer" => &["ir"],
        "Match EQ" => &["current", "reference", "eq"],
        "Channel EQ" | "Linear Phase EQ" => &["analyzer"],
        "MultiMeter" => &["spectrum", "goniometer"],
        _ => &[],
    }
}

fn v(params: &[Param], values: &[f32], id: u32) -> f32 {
    params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i])
}

fn pick<'a>(params: &'a [Param], values: &[f32], ids: &[u32]) -> (Vec<Param>, Vec<f32>) {
    let mut p = Vec::new();
    let mut vv = Vec::new();
    for id in ids {
        if let Some(i) = params.iter().position(|x| x.id == *id) {
            let mut q = params[i].clone();
            q.hidden = false;
            p.push(q);
            vv.push(values[i]);
        }
    }
    let _: &'a [Param] = params;
    (p, vv)
}

/// Rows (Controls style) for the given parameter ids.
fn rows(ui: &mut egui::Ui, params: &[Param], values: &[f32], ids: &[u32], salt: &str) -> Vec<(u32, f32)> {
    let (p, vv) = pick(params, values, ids);
    crate::pluginwin::control_rows(ui, &p, &vv, &[], salt)
}

fn button(ui: &mut egui::Ui, r: Rect, id: egui::Id, label: &str, on: bool) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click_and_drag());
    let p = ui.painter();
    let fill = if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() };
    p.rect_filled(r, 5.0, fill);
    p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if on { Color32::WHITE } else { look::text() });
    resp
}

fn panel(ui: &mut egui::Ui, h: f32) -> (Rect, egui::Painter) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, look::lcd_bg());
    (r, p)
}

/// A horizontal meter bar: value in dB from lo to hi.
fn meter(p: &egui::Painter, r: Rect, db: f32, lo: f32, hi: f32, label: &str) {
    p.rect_filled(r, 2.0, look::group_bg());
    let f = ((db - lo) / (hi - lo)).clamp(0.0, 1.0);
    let col = if db > -0.1 { look::record_red() } else if db > -3.0 { Color32::from_rgb(0xe8, 0xc0, 0x30) } else { Color32::from_rgb(0x40, 0xc0, 0x60) };
    p.rect_filled(Rect::from_min_size(r.min, vec2(r.width() * f, r.height())), 2.0, col);
    p.text(r.right_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, format!("{label} {}", if db <= -119.0 { "−∞".to_string() } else { format!("{db:.1}") }), look::regular(11.0), look::text());
}

fn curve_line(p: &egui::Painter, r: Rect, ys: &[f32], lo: f32, hi: f32, col: Color32, w: f32) {
    if ys.len() < 2 {
        return;
    }
    let pts: Vec<Pos2> = ys.iter().enumerate().map(|(i, y)| pos2(r.min.x + r.width() * i as f32 / (ys.len() - 1) as f32, r.max.y - r.height() * ((y - lo) / (hi - lo)).clamp(0.0, 1.0))).collect();
    p.add(egui::Shape::line(pts, Stroke::new(w, col)));
}

/// The editor for a plug-in; the parameters it changed and an action for the app.
pub fn editor(ui: &mut egui::Ui, name: &str, params: &[Param], values: &[f32], live: &Live, slot: (usize, usize)) -> (Vec<(u32, f32)>, Option<Action>) {
    let mut set = Vec::new();
    let mut act = None;
    egui::ScrollArea::vertical().id_salt(("fx-editor", slot)).auto_shrink([false, false]).show(ui, |ui| match name {
        "Channel EQ" | "Linear Phase EQ" => crate::eqview::view(ui, params, values, live.curve("analyzer"), slot, &mut set),
        "Remix FX" => remix(ui, params, values, &mut set),
        "Beat Breaker" => beat_breaker(ui, params, values, live, &mut set, &mut act),
        "Step FX" => step_fx(ui, params, values, live, &mut set),
        "Delay Designer" => delay_designer(ui, params, values, &mut set),
        "Pedalboard" => pedalboard(ui, params, values, &mut set),
        "Mastering Assistant" => mastering(ui, params, values, live, &mut set, &mut act),
        "Space Designer" => space(ui, params, values, live, &mut set, &mut act),
        "Match EQ" => match_eq(ui, params, values, live, &mut set, &mut act),
        "Pitch Correction" => pitch_correction(ui, params, values, live, &mut set),
        _ => meters(ui, name, params, values, live, &mut set),
    });
    (set, act)
}

// ---------------------------------------------------------------------------------------------
// Remix FX

fn remix(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Vec<(u32, f32)>) {
    use lg_fx::remix_fx::*;
    let g = |id| v(params, values, id);
    let w = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(w, 300.0), Sense::hover());
    let pad = 260.0f32.min((w - 140.0) / 2.0);
    for side in 0..2 {
        let right = side == 1;
        let x0 = if right { area.max.x - pad } else { area.min.x };
        let r = Rect::from_min_size(pos2(x0, area.min.y + 30.0), vec2(pad, pad));
        let fx_id = if right { RIGHT_FX } else { LEFT_FX };
        let lock_id = if right { RIGHT_LOCK } else { LEFT_LOCK };
        let e = g(fx_id).round() as usize;
        // the pad's pop-up and FX Lock
        let menu_r = Rect::from_min_size(pos2(x0, area.min.y), vec2(pad - 60.0, 24.0));
        if let Some(k) = crate::widgets::choice_slot(ui, menu_r, ui.scope_id().with(("rfx-menu", side)), e, XY_EFFECTS) {
            set.push((fx_id, k as f32));
        }
        let lock = g(lock_id) >= 0.5;
        let lr = Rect::from_min_size(pos2(x0 + pad - 54.0, area.min.y), vec2(54.0, 24.0));
        if button(ui, lr, ui.scope_id().with(("rfx-lock", side)), "FX Lock", lock).clicked() {
            set.push((lock_id, if lock { 0.0 } else { 1.0 }));
        }
        let p = ui.painter().clone();
        let on = g(fx_on(e)) >= 0.5;
        p.rect_filled(r, 8.0, if on { look::mix(look::lcd_bg(), look::accent(), 0.25) } else { look::lcd_bg() });
        p.rect_stroke(r, 8.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        let axes = [("Cutoff", "Resonance"), ("Rate", "Depth"), ("Rate", "Depth"), ("Rate", "Mix"), ("Time", "Mix"), ("Time", "Feedback")][e.min(5)];
        p.text(r.center_bottom() + vec2(0.0, -6.0), Align2::CENTER_BOTTOM, axes.0, look::regular(11.0), look::text_dim());
        p.text(r.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, axes.1, look::regular(11.0), look::text_dim());
        // every effect this pad has running (a locked one keeps its dot)
        for k in 0..XY_EFFECTS.len() {
            if g(fx_on(k)) >= 0.5 {
                let c = pos2(r.min.x + r.width() * g(fx_x(k)), r.max.y - r.height() * g(fx_y(k)));
                p.circle_filled(c, if k == e { 9.0 } else { 6.0 }, if k == e { look::accent() } else { look::text_dim() });
            }
        }
        let resp = ui.interact(r, ui.scope_id().with(("rfx-pad", side)), Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
            let x = ((pp.x - r.min.x) / r.width()).clamp(0.0, 1.0);
            let y = ((r.max.y - pp.y) / r.height()).clamp(0.0, 1.0);
            set.push((fx_x(e), x));
            set.push((fx_y(e), y));
            if !on {
                set.push((fx_on(e), 1.0));
            }
        } else if (resp.drag_stopped() || resp.clicked()) && !lock {
            set.push((fx_on(e), 0.0));
        }
    }
    // Gater and Downsampler sliders, Reset, between the pads
    let mid = Rect::from_min_max(pos2(area.min.x + pad + 10.0, area.min.y + 30.0), pos2(area.max.x - pad - 10.0, area.min.y + 30.0 + pad));
    let p = ui.painter().clone();
    for (k, (id, label)) in [(GATER, "Gater"), (DOWNSAMPLER, "Downsampler")].iter().enumerate() {
        let col = Rect::from_min_size(pos2(mid.min.x + k as f32 * mid.width() / 2.0 + 8.0, mid.min.y + 20.0), vec2(mid.width() / 2.0 - 16.0, mid.height() - 60.0));
        p.rect_filled(col, 4.0, look::group_bg());
        let val = g(*id);
        p.rect_filled(Rect::from_min_max(pos2(col.min.x, col.max.y - col.height() * val), col.max), 4.0, look::accent());
        p.text(col.center_top() - vec2(0.0, 4.0), Align2::CENTER_BOTTOM, *label, look::regular(11.0), look::text());
        let resp = ui.interact(col, ui.scope_id().with(("rfx-sl", k)), Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
            set.push((*id, ((col.max.y - pp.y) / col.height()).clamp(0.0, 1.0)));
        }
        if resp.double_clicked() {
            set.push((*id, 0.0));
        }
    }
    let rr = Rect::from_center_size(pos2(mid.center().x, mid.max.y - 14.0), vec2(70.0, 24.0));
    if button(ui, rr, ui.scope_id().with("rfx-reset"), "Reset", false).clicked() {
        set.push((RESET, 1.0));
    }
    // Reverse, Scratch, Tape Stop: held buttons; where pressed sets the variation
    ui.add_space(8.0);
    let (br, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    for (k, (id, var, label)) in [(REVERSE, REVERSE_VAR, "Reverse"), (SCRATCH, SCRATCH_VAR, "Scratch"), (TAPE_STOP, TAPE_STOP_VAR, "Tape Stop")].iter().enumerate() {
        let r = Rect::from_min_size(pos2(br.min.x + k as f32 * br.width() / 3.0 + 6.0, br.min.y), vec2(br.width() / 3.0 - 12.0, 40.0));
        let held = g(*id) >= 0.5;
        let resp = button(ui, r, ui.scope_id().with(("rfx-btn", k)), label, held);
        if resp.is_pointer_button_down_on() {
            if !held {
                if let Some(pp) = resp.interact_pointer_pos() {
                    let side = ((pp.x - r.center().x) / (r.width() / 2.0)).clamp(-1.0, 1.0);
                    // the outer thirds pick the variation, the middle the plain effect
                    set.push((*var, if side.abs() < 0.33 { 0.0 } else { side.signum() }));
                }
                set.push((*id, 1.0));
            }
        } else if held {
            set.push((*id, 0.0));
        }
    }
    ui.add_space(6.0);
    let le = g(LEFT_FX).round() as u32;
    let re = g(RIGHT_FX).round() as u32;
    let setting = |e: u32| [FILTER_MODE, WOBBLE_TIME, ORBIT_MODE, REPEATER_TIME, REVERB_COLOR, DELAY_TIME][e.min(5) as usize];
    ui.label(egui::RichText::new("Settings").color(look::text_dim()));
    set.extend(rows(ui, params, values, &[setting(le), setting(re), GATER_TIME, NOISE, DS_MODE, REVERSE_TIME, SCRATCH_TIME, TAPE_STOP_TIME], "rfx-set"));
}

// ---------------------------------------------------------------------------------------------
// Beat Breaker

const MODE_COLORS: [Color32; 6] = [Color32::from_rgb(0xf0, 0x90, 0x30), Color32::from_rgb(0xe0, 0x40, 0xc0), Color32::from_rgb(0x30, 0xd0, 0xe0), Color32::from_rgb(0x90, 0x60, 0xf0), Color32::from_rgb(0xf0, 0xd0, 0x40), Color32::from_rgb(0x50, 0xd0, 0x70)];

pub fn patterns_dir() -> Option<std::path::PathBuf> {
    crate::pluginwin::settings_dir("Beat Breaker").map(|d| d.join("patterns"))
}

fn beat_breaker(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>, act: &mut Option<Action>) {
    use lg_fx::beat_breaker::*;
    let g = |id| v(params, values, id);
    let mode = g(EDIT_MODE).round() as usize;
    let pat = (g(EDITING).round() as usize).clamp(1, 10) - 1;
    let len = LENGTH_BEATS[(g(pat_len(pat)).round() as usize).min(LENGTH_BEATS.len() - 1)];
    let count = (g(pat_count(pat)).round() as usize).clamp(1, MAX_SLICES);
    let sel = (g(SELECTED).round() as usize).min(count - 1);
    // the edit mode buttons, Randomize, the global controls
    ui.horizontal(|ui| {
        for (k, m) in MODES.iter().enumerate() {
            let b = egui::Button::new(egui::RichText::new(*m).color(if k == mode { Color32::BLACK } else { MODE_COLORS[k] })).fill(if k == mode { MODE_COLORS[k] } else { look::group_bg() });
            if ui.add(b).clicked() {
                set.push((EDIT_MODE, k as f32));
            }
        }
        ui.separator();
        let multi = g(RANDOM_MODE) >= 0.5;
        let r = ui.add(egui::Button::new(if multi { "⚄⚄ Randomize" } else { "⚄ Randomize" }));
        if r.clicked() {
            set.push((RANDOMIZE, 1.0));
        }
        r.context_menu(|ui| {
            if ui.add(egui::Button::selectable(!multi, "Current Mode")).clicked() {
                set.push((RANDOM_MODE, 0.0));
            }
            if ui.add(egui::Button::selectable(multi, "Multiple Modes")).clicked() {
                set.push((RANDOM_MODE, 1.0));
            }
            ui.separator();
            for (k, n) in ["Time", "Repeat", "Cutoff", "Resonance", "Volume", "Pan", "Output Beat"].iter().enumerate() {
                let on = g(ENABLED + k as u32) >= 0.5;
                if ui.add(egui::Button::selectable(on, *n)).clicked() {
                    set.push((ENABLED + k as u32, if on { 0.0 } else { 1.0 }));
                }
            }
            if ui.button("Select All Modes").clicked() {
                for k in 0..7 {
                    set.push((ENABLED + k, 1.0));
                }
            }
            ui.separator();
            if ui.button("Clear All Values").clicked() {
                set.push((CLEAR, mode as f32 + 1.0));
                ui.close();
            }
        });
    });
    // the Slice Editor strip and the slices
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 180.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(strip, 3.0, look::group_bg());
    p.rect_filled(area, 6.0, look::lcd_bg());
    let bx = |b: f32| area.min.x + area.width() * (b / len).clamp(0.0, 1.0);
    let fields: Vec<[f32; NF]> = (0..count).map(|s| std::array::from_fn(|f| g(slice_id(pat, s, f)))).collect();
    let main_field = [F_IN, F_REP, F_CUT, F_RES, F_VOL, F_PAN][mode];
    let (lo, hi) = (FIELDS[main_field].1, if main_field == F_IN { len } else { FIELDS[main_field].2 });
    // beat grid
    for b in 0..=(len as usize * 4) {
        let x = bx(b as f32 / 4.0);
        p.vline(x, area.y_range(), Stroke::new(if b % 4 == 0 { 1.0 } else { 0.5 }, look::grid_line()));
    }
    let playing = live.get("Playing Slice").map(|v| v as usize);
    for (s, f) in fields.iter().enumerate() {
        let x0 = bx(f[F_OUT]);
        let x1 = fields.get(s + 1).map_or(area.max.x, |n| bx(n[F_OUT]));
        let col = MODE_COLORS[mode];
        let r = Rect::from_min_max(pos2(x0 + 1.0, area.min.y + 2.0), pos2(x1 - 1.0, area.max.y - 2.0));
        if s == sel {
            p.rect_filled(r, 3.0, look::mix(look::lcd_bg(), col, 0.18));
        }
        if playing == Some(s) {
            p.rect_stroke(r, 3.0, Stroke::new(1.5, look::text()), egui::StrokeKind::Inside);
        }
        let t = ((f[main_field] - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
        let y = r.max.y - r.height() * t;
        if mode == 0 {
            // Time: the mapping line from the input beat, slope = speed
            let speed = f[F_SPEED] / 100.0;
            let seg = (x1 - x0) / f[F_REP].max(1.0);
            let yy = |b: f32| r.max.y - r.height() * (b / len).clamp(0.0, 1.0);
            let dur = (x1 - x0) / area.width() * len;
            let b0 = if speed < 0.0 { f[F_IN] + dur } else { f[F_IN] };
            p.line_segment([pos2(x0, yy(b0)), pos2(x0 + seg, yy(b0 + speed * dur / f[F_REP].max(1.0)))], Stroke::new(2.0, col));
        } else if mode == 1 {
            let reps = f[F_REP].round().max(1.0) as usize;
            for k in 0..reps {
                let w = (x1 - x0) / reps as f32;
                p.rect_filled(Rect::from_min_max(pos2(x0 + k as f32 * w + 2.0, r.max.y - 30.0), pos2(x0 + (k + 1) as f32 * w - 2.0, r.max.y - 4.0)), 2.0, col);
            }
        } else {
            // the value with its slope through the slice
            let slope = match mode {
                2 => f[F_CSLOPE],
                4 => f[F_VSLOPE],
                5 => f[F_PSLOPE] / 2.0,
                _ => 0.0,
            };
            let y1 = r.max.y - r.height() * (t + slope).clamp(0.0, 1.0);
            p.line_segment([pos2(x0, y), pos2(x1, y1)], Stroke::new(3.0, col));
        }
        // the slice marker
        let mr = Rect::from_center_size(pos2(x0, strip.center().y), vec2(10.0, strip.height()));
        p.rect_filled(mr, 2.0, if s == sel { col } else { look::text_dim() });
        if s > 0 {
            let resp = ui.interact(mr, ui.scope_id().with(("bb-mark", s)), Sense::click_and_drag());
            if resp.double_clicked() {
                // delete the marker: the slices after it move down
                let mut rest: Vec<[f32; NF]> = fields.clone();
                rest.remove(s);
                set.push((pat_count(pat), rest.len() as f32));
                for (k, ff) in rest.iter().enumerate() {
                    for (fi, val) in ff.iter().enumerate() {
                        set.push((slice_id(pat, k, fi), *val));
                    }
                }
                return;
            }
            if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                set.push((SELECTED, s as f32));
                let b = ((pp.x - area.min.x) / area.width() * len).clamp(fields[s - 1][F_OUT] + 0.0625, fields.get(s + 1).map_or(len, |n| n[F_OUT]) - 0.0625);
                set.push((SEL + F_OUT as u32, b));
            }
        }
        // click selects, vertical drag sets the main value, horizontal the second, ⌘-drag Curve
        let resp = ui.interact(r, ui.scope_id().with(("bb-slice", s)), Sense::click_and_drag());
        if resp.clicked() {
            set.push((SELECTED, s as f32));
        }
        if resp.dragged() {
            set.push((SELECTED, s as f32));
            let d = resp.drag_delta();
            let cmd = ui.input(|i| i.modifiers.command);
            let (vf, hf, cf) = match mode {
                0 => (F_IN, Some(F_SPEED), Some(F_TCURVE)),
                1 => (F_REP, None, None),
                2 => (F_CUT, Some(F_CSLOPE), Some(F_CCURVE)),
                3 => (F_RES, None, None),
                4 => (F_VOL, Some(F_VSLOPE), Some(F_VCURVE)),
                _ => (F_PAN, Some(F_PSLOPE), Some(F_PCURVE)),
            };
            if cmd {
                if let Some(cf) = cf {
                    set.push((SEL + cf as u32, (f[cf] - d.y / 100.0).clamp(-1.0, 1.0)));
                }
            } else if d.x.abs() > d.y.abs() {
                if let Some(hf) = hf {
                    let (a, b) = (FIELDS[hf].1, FIELDS[hf].2);
                    set.push((SEL + hf as u32, (f[hf] + d.x / 200.0 * (b - a)).clamp(a, b)));
                }
            } else {
                let (a, b) = (lo, hi);
                set.push((SEL + vf as u32, (f[vf] - d.y / r.height() * (b - a)).clamp(a, b)));
            }
        }
    }
    // a click on an empty part of the strip adds a slice marker
    let sresp = ui.interact(strip, ui.scope_id().with("bb-strip"), Sense::click());
    if sresp.clicked() && count < MAX_SLICES {
        if let Some(pp) = sresp.interact_pointer_pos() {
            let b = (pp.x - area.min.x) / area.width() * len;
            let snap = [0.0, 1.0, 0.5, 0.25, 0.125][(g(SNAP_OUT).round() as usize).min(4)];
            let b = if snap > 0.0 { (b / snap).round() * snap } else { b }.clamp(0.0, len - 0.0625);
            if !fields.iter().any(|f| (f[F_OUT] - b).abs() < 1e-3) {
                let mut all = fields.clone();
                let mut nf: [f32; NF] = std::array::from_fn(|k| FIELDS[k].3);
                nf[F_OUT] = b;
                nf[F_IN] = b;
                all.push(nf);
                all.sort_by(|a, c| a[F_OUT].partial_cmp(&c[F_OUT]).unwrap());
                set.push((pat_count(pat), all.len() as f32));
                for (k, ff) in all.iter().enumerate() {
                    for (fi, val) in ff.iter().enumerate() {
                        set.push((slice_id(pat, k, fi), *val));
                    }
                }
            }
        }
    }
    // the selected slice's parameters for this mode
    let sel_ids: Vec<u32> = match mode {
        0 => vec![SEL + F_OUT as u32, SEL + F_IN as u32, SEL + F_SPEED as u32, SEL + F_TCURVE as u32],
        1 => vec![SEL + F_OUT as u32, SEL + F_REP as u32],
        2 => vec![SEL + F_OUT as u32, SEL + F_FTYPE as u32, SEL + F_CUT as u32, SEL + F_CSLOPE as u32, SEL + F_CCURVE as u32],
        3 => vec![SEL + F_OUT as u32, SEL + F_RES as u32],
        4 => vec![SEL + F_OUT as u32, SEL + F_VOL as u32, SEL + F_VSLOPE as u32, SEL + F_VCURVE as u32],
        _ => vec![SEL + F_OUT as u32, SEL + F_PAN as u32, SEL + F_PSLOPE as u32, SEL + F_PCURVE as u32],
    };
    ui.label(egui::RichText::new(format!("Slice {}", sel + 1)).color(MODE_COLORS[mode]));
    set.extend(rows(ui, params, values, &sel_ids, "bb-sel"));
    set.extend(rows(ui, params, values, &[LENGTH, DECLICK, BYPASS_BELOW, MIX], "bb-glob"));
    // the pattern buttons and the Pattern Slot menu
    ui.horizontal(|ui| {
        let cur = g(PATTERN).round() as usize;
        if ui.add(egui::Button::selectable(cur == 0, "Off")).clicked() {
            set.push((PATTERN, 0.0));
        }
        for k in 1..=10 {
            if ui.add(egui::Button::selectable(cur == k, k.to_string())).clicked() {
                set.push((PATTERN, k as f32));
            }
        }
        ui.menu_button("Pattern ⌄", |ui| {
            for (k, a) in ACTIONS.iter().enumerate().skip(1) {
                if ui.button(*a).clicked() {
                    set.push((ACTION, k as f32));
                    ui.close();
                }
            }
            ui.separator();
            let id = ui.scope_id().with("bb-save-name");
            let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut name).hint_text("Pattern name").desired_width(120.0));
                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save Pattern As…")).clicked() {
                    *act = Some(Action::SavePattern(name.trim().to_string()));
                    ui.close();
                }
            });
            ui.data_mut(|d| d.insert_temp(id, name));
            let user: Vec<std::path::PathBuf> = patterns_dir().and_then(|d| std::fs::read_dir(d).ok()).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect()).unwrap_or_default();
            if !user.is_empty() {
                ui.separator();
                ui.label("User");
                for p in user {
                    let n = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                    if ui.button(&n).clicked() {
                        *act = Some(Action::LoadPattern(p.clone()));
                        ui.close();
                    }
                }
            }
        });
    });
    // randomization settings of the current mode
    egui::CollapsingHeader::new("Randomization").id_salt("bb-rand").show(ui, |ui| {
        let ids: Vec<u32> = RAND_NAMES.iter().enumerate().filter(|(_, r)| r.1 == mode || (mode == 0 && r.1 == 6)).map(|(k, _)| RAND + k as u32).collect();
        set.extend(rows(ui, params, values, &ids, "bb-rnd"));
        if mode == 1 {
            ui.horizontal(|ui| {
                for r in 0..8u32 {
                    let on = g(RAND_REPEATS + r) >= 0.5;
                    if ui.add(egui::Button::selectable(on, (r + 1).to_string())).clicked() {
                        set.push((RAND_REPEATS + r, if on { 0.0 } else { 1.0 }));
                    }
                }
            });
        }
    });
    egui::CollapsingHeader::new("More").id_salt("bb-more").show(ui, |ui| {
        set.extend(rows(ui, params, values, &[SNAP_OUT, SNAP_IN, SNAP_SPD, SNAP_CUT, SNAP_RES, SNAP_VOL, SNAP_PAN], "bb-snap"));
    });
}

// ---------------------------------------------------------------------------------------------
// Step FX

fn step_fx(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>) {
    use lg_fx::step_fx::*;
    let g = |id| v(params, values, id);
    let m = (g(SELECTED_MOD).round() as usize).min(2);
    ui.horizontal(|ui| {
        for k in 0..3 {
            let on = g(mod_id(k, M_ON)) >= 0.5;
            if ui.add(egui::Button::selectable(on, if on { "⏻" } else { "○" })).clicked() {
                set.push((mod_id(k, M_ON), if on { 0.0 } else { 1.0 }));
            }
            let t = g(mod_id(k, M_TARGET)).round() as usize;
            if ui.add(egui::Button::selectable(k == m, format!("{} {}", k + 1, TARGETS[t.min(TARGETS.len() - 1)]))).clicked() {
                set.push((SELECTED_MOD, k as f32));
            }
        }
    });
    let n = (g(mod_id(m, M_LENGTH)).round() as usize).clamp(1, MAX_STEPS);
    let now = live.get(["Step 1", "Step 2", "Step 3"][m]).map(|v| v as usize);
    let shown = n.min(32);
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(area, 6.0, look::lcd_bg());
    let w = area.width() / shown as f32;
    let shift = ui.input(|i| i.modifiers.shift);
    for s in 0..shown {
        let (lvl, len, on) = (g(step_id(m, s, 0)), g(step_id(m, s, 1)), g(step_id(m, s, 2)) >= 0.5);
        let x0 = area.min.x + s as f32 * w;
        let col = if now == Some(s) { look::text() } else if on { look::accent() } else { look::text_dim() };
        let bw = (w - 3.0) * len.min(1.0);
        let r = Rect::from_min_max(pos2(x0 + 1.5, area.max.y - 18.0 - (area.height() - 22.0) * lvl), pos2(x0 + 1.5 + bw, area.max.y - 18.0));
        p.rect_filled(r, 2.0, col);
        if len > 1.0 {
            // a tie: the level carries over the next steps
            p.rect_filled(Rect::from_min_max(pos2(r.max.x, r.min.y + r.height() * 0.5), pos2(x0 + w * len.min((shown - s) as f32), r.min.y + r.height() * 0.5 + 2.0)), 0.0, look::mix(col, look::lcd_bg(), 0.5));
        }
        let num = Rect::from_min_max(pos2(x0, area.max.y - 16.0), pos2(x0 + w, area.max.y));
        p.text(num.center(), Align2::CENTER_CENTER, (s + 1).to_string(), look::regular(10.0), if on { look::text() } else { look::text_dim() });
        if ui.interact(num, ui.scope_id().with(("sfx-num", s)), Sense::click()).clicked() {
            set.push((step_id(m, s, 2), if on { 0.0 } else { 1.0 }));
        }
        let col_r = Rect::from_min_max(pos2(x0, area.min.y), pos2(x0 + w, area.max.y - 16.0));
        let resp = ui.interact(col_r, ui.scope_id().with(("sfx-step", s)), Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
            if ui.input(|i| i.modifiers.alt) {
                // length: snapped to 25 % steps (Shift: free), beyond one step a tie
                let l = ((pp.x - x0) / w).clamp(0.05, 8.0);
                let l = if shift { l } else { (l * 4.0).round().max(1.0) / 4.0 };
                set.push((step_id(m, s, 1), l));
                if l > 1.0 {
                    for k in 1..(l.ceil() as usize) {
                        if s + k < n {
                            set.push((step_id(m, s + k, 2), 1.0));
                        }
                    }
                }
            } else {
                let lvl = ((area.max.y - 18.0 - pp.y) / (area.height() - 22.0)).clamp(0.0, 1.0);
                set.push((step_id(m, s, 0), lvl));
            }
        }
    }
    ui.label(egui::RichText::new("Drag a step to set its level; Option-drag sideways for its length (past the step: a tie). Click a number to turn a step on or off.").size(11.0).color(look::text_dim()));
    set.extend(rows(ui, params, values, &[mod_id(m, M_TARGET), mod_id(m, M_DEPTH), mod_id(m, M_RATE), mod_id(m, M_SWING), mod_id(m, M_LENGTH), mod_id(m, M_ATTACK), mod_id(m, M_ACURVE), mod_id(m, M_HOLD), mod_id(m, M_RELEASE), mod_id(m, M_RCURVE), mod_id(m, M_ACTION)], "sfx-mod"));
    // XY pad
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(160.0, 160.0), Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(r, 6.0, look::lcd_bg());
        let c = pos2(r.min.x + r.width() * g(XY + 10), r.max.y - r.height() * g(XY + 11));
        p.circle_filled(c, 7.0, Color32::from_rgb(0x40, 0x90, 0xf0));
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
            set.push((XY + 10, ((pp.x - r.min.x) / r.width()).clamp(0.0, 1.0)));
            set.push((XY + 11, ((r.max.y - pp.y) / r.height()).clamp(0.0, 1.0)));
        }
        ui.vertical(|ui| {
            set.extend(rows(ui, params, values, &(XY..XY + 10).collect::<Vec<_>>(), "sfx-xy"));
        });
    });
    // the effects order strip
    ui.horizontal(|ui| {
        ui.label("Order:");
        let order: Vec<usize> = (0..5).map(|k| g(ORDER + k).round() as usize).collect();
        let ons = [MOD_ON, DLY_ON, FLT_ON, DST_ON, REV_ON];
        for (k, pr) in order.iter().enumerate() {
            let on = g(ons[*pr]) >= 0.5;
            let resp = ui.add(egui::Button::new(egui::RichText::new(PROCESSORS[*pr]).color(if on { look::text() } else { look::text_dim() })).sense(Sense::click_and_drag()));
            if resp.clicked() {
                set.push((ons[*pr], if on { 0.0 } else { 1.0 }));
            }
            // drag sideways: swap with the neighbour on that side
            if resp.drag_stopped() {
                let dx = resp.drag_delta().x + ui.input(|i| i.pointer.delta().x);
                let to = if dx < 0.0 && k > 0 { k - 1 } else if dx > 0.0 && k < 4 { k + 1 } else { k };
                if to != k {
                    set.push((ORDER + to as u32, *pr as f32));
                }
            }
            resp.on_hover_text("Click: on / off. Drag sideways to move it in the chain.");
        }
    });
    egui::CollapsingHeader::new("Effects").id_salt("sfx-proc").default_open(true).show(ui, |ui| {
        let ids: Vec<u32> = params.iter().map(|p| p.id).filter(|id| (MOD_ON..=OUTPUT).contains(id)).collect();
        set.extend(rows(ui, params, values, &ids, "sfx-p"));
    });
}

// ---------------------------------------------------------------------------------------------
// Delay Designer

fn delay_designer(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Vec<(u32, f32)>) {
    use lg_fx::delay_designer::*;
    let g = |id| v(params, values, id);
    let view_id = ui.scope_id().with("dd-view");
    let mut view = ui.data(|d| d.get_temp::<usize>(view_id)).unwrap_or(4);
    ui.horizontal(|ui| {
        for (k, n) in ["Cutoff", "Reso", "Transp", "Pan", "Level"].iter().enumerate() {
            if ui.add(egui::Button::selectable(view == k, *n)).clicked() {
                view = k;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(view_id, view));
    let taps: Vec<(usize, f32)> = (0..TAPS).filter(|k| g(tap_id(*k, T_ON)) >= 0.5).map(|k| (k, g(tap_id(k, T_DELAY)))).collect();
    let max_ms = taps.iter().map(|t| t.1).fold(500.0f32, f32::max) * 1.15;
    let sel = g(SELECTED).round() as usize;
    let (toggle, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
    let (idbar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(area, 6.0, look::lcd_bg());
    p.rect_filled(idbar, 3.0, look::group_bg());
    let x_of = |ms: f32| area.min.x + area.width() * (ms / max_ms);
    for (k, ms) in &taps {
        let x = x_of(*ms);
        let on_col = if *k == sel { Color32::WHITE } else { look::accent() };
        p.vline(x, area.y_range(), Stroke::new(1.0, look::mix(look::lcd_bg(), on_col, 0.4)));
        let fy = |t: f32| area.max.y - area.height() * t.clamp(0.0, 1.0);
        match view {
            0 => {
                let lo = (g(tap_id(*k, T_HP)).max(10.0) / 10.0).log10() / 3.31;
                let hi = (g(tap_id(*k, T_LP)).max(10.0) / 10.0).log10() / 3.31;
                p.line_segment([pos2(x, fy(lo)), pos2(x, fy(hi))], Stroke::new(4.0, on_col));
            }
            1 => {
                p.line_segment([pos2(x, area.max.y), pos2(x, fy(g(tap_id(*k, T_RESO)) / 100.0))], Stroke::new(4.0, on_col));
            }
            2 => {
                p.circle_filled(pos2(x, fy(0.5 + g(tap_id(*k, T_TRANSP)) / 48.0)), 4.0, on_col);
            }
            3 => {
                let pan = g(tap_id(*k, T_PAN)) / 100.0;
                let spread = g(tap_id(*k, T_SPREAD)) / 100.0;
                let c = fy(0.5 + pan * 0.5);
                p.line_segment([pos2(x, c - spread * 30.0), pos2(x, c + spread * 30.0)], Stroke::new(2.0, on_col));
                p.circle_filled(pos2(x, c), 4.0, on_col);
            }
            _ => {
                p.line_segment([pos2(x, area.max.y), pos2(x, fy(g(tap_id(*k, T_LEVEL)) / 100.0))], Stroke::new(4.0, on_col));
            }
        }
        // the letter on the identification bar: click selects, drag moves in time, drag down out
        // of the display deletes; the toggle above switches the view's parameter
        let lr = Rect::from_center_size(pos2(x, idbar.center().y), vec2(16.0, idbar.height()));
        p.text(lr.center(), Align2::CENTER_CENTER, tap_letter(*k).to_string(), look::semibold(12.0), on_col);
        let resp = ui.interact(lr, ui.scope_id().with(("dd-tap", *k)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            set.push((SELECTED, *k as f32));
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                if pp.y > idbar.max.y + 30.0 && taps.len() > 1 {
                    set.push((tap_id(*k, T_ON), 0.0));
                } else {
                    set.push((SELECTED, *k as f32));
                    set.push((SEL + T_DELAY as u32, ((pp.x - area.min.x) / area.width() * max_ms).clamp(1.0, 10000.0)));
                }
            }
        }
        resp.context_menu(|ui| {
            if ui.button("Delete Tap").clicked() && taps.len() > 1 {
                set.push((tap_id(*k, T_ON), 0.0));
                ui.close();
            }
        });
        let tr = Rect::from_center_size(pos2(x, toggle.center().y), vec2(14.0, 14.0));
        let tf = [T_FILTER, T_SLOPE, T_PITCH, T_FLIP, T_MUTE][view];
        let ton = g(tap_id(*k, tf)) >= 0.5;
        p.circle(tr.center(), 5.0, if ton { look::accent() } else { look::group_bg() }, Stroke::new(1.0, look::line()));
        if ui.interact(tr, ui.scope_id().with(("dd-tog", *k)), Sense::click()).clicked() {
            set.push((tap_id(*k, tf), if ton { 0.0 } else { 1.0 }));
        }
        // editing in the display: drag vertically on a tap's line
        let col = Rect::from_center_size(pos2(x, area.center().y), vec2(10.0, area.height()));
        let resp = ui.interact(col, ui.scope_id().with(("dd-edit", *k)), Sense::drag());
        if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            let t = ((area.max.y - pp.y) / area.height()).clamp(0.0, 1.0);
            set.push((SELECTED, *k as f32));
            match view {
                0 => set.push((SEL + T_LP as u32, 10.0 * 10f32.powf(t * 3.31))),
                1 => set.push((SEL + T_RESO as u32, t * 100.0)),
                2 => set.push((SEL + T_TRANSP as u32, ((t - 0.5) * 48.0).round())),
                3 => set.push((SEL + T_PAN as u32, (t - 0.5) * 200.0)),
                _ => set.push((SEL + T_LEVEL as u32, t * 100.0)),
            }
        }
    }
    // a click on the identification bar (not on a letter) adds a tap there
    let resp = ui.interact(idbar, ui.scope_id().with("dd-idbar"), Sense::click());
    if resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            if !taps.iter().any(|t| (x_of(t.1) - pp.x).abs() < 9.0) {
                if let Some(k) = (0..TAPS).find(|k| g(tap_id(*k, T_ON)) < 0.5) {
                    let ms = ((pp.x - area.min.x) / area.width() * max_ms).clamp(1.0, 10000.0);
                    for (f, (_, _, _, d, _)) in FIELDS.iter().enumerate() {
                        set.push((tap_id(k, f), *d));
                    }
                    set.push((tap_id(k, T_ON), 1.0));
                    set.push((tap_id(k, T_DELAY), ms));
                    set.push((SELECTED, k as f32));
                }
            }
        }
    }
    // the Tap pads
    let tp_id = ui.scope_id().with("dd-tappads");
    let mut rec: Option<(f64, Vec<f32>)> = ui.data(|d| d.get_temp(tp_id));
    let now = ui.input(|i| i.time);
    ui.horizontal(|ui| {
        match &mut rec {
            None => {
                if ui.button("Start").on_hover_text("Erases the taps and starts tap recording").clicked() {
                    rec = Some((now, Vec::new()));
                    set.push((FEEDBACK, 0.0));
                }
            }
            Some((t0, times)) => {
                if ui.button("Tap").clicked() && times.len() < TAPS {
                    times.push(((now - *t0) * 1000.0) as f32);
                }
                let last = ui.button("Last Tap").clicked();
                if last || now - *t0 > 10.0 || times.len() >= TAPS {
                    let mut ts = times.clone();
                    if last {
                        ts.push(((now - *t0) * 1000.0) as f32);
                    }
                    for k in 0..TAPS {
                        set.push((tap_id(k, T_ON), 0.0));
                    }
                    for (k, t) in ts.iter().enumerate().take(TAPS) {
                        for (f, (_, _, _, d, _)) in FIELDS.iter().enumerate() {
                            set.push((tap_id(k, f), *d));
                        }
                        set.push((tap_id(k, T_ON), 1.0));
                        set.push((tap_id(k, T_DELAY), t.max(1.0)));
                    }
                    if !ts.is_empty() {
                        set.push((FEEDBACK_TAP, (ts.len().min(TAPS) - 1) as f32));
                        set.push((FEEDBACK, 1.0));
                    }
                    rec = None;
                } else {
                    ui.label(egui::RichText::new(format!("recording taps… {}", times.len())).color(look::record_red()));
                    ui.ctx().request_repaint();
                }
            }
        }
    });
    ui.data_mut(|d| match rec {
        Some(r) => {
            d.insert_temp(tp_id, r);
        }
        None => {
            d.remove::<(f64, Vec<f32>)>(tp_id);
        }
    });
    ui.label(egui::RichText::new(format!("Tap {}", tap_letter(sel))).strong());
    let tap_ids: Vec<u32> = (1..FIELDS.len() as u32).map(|f| SEL + f).collect();
    set.extend(rows(ui, params, values, &tap_ids, "dd-bar"));
    set.extend(rows(ui, params, values, &[SYNC, GRID, SWING, FEEDBACK, FEEDBACK_TAP, FEEDBACK_LEVEL, MIX_DRY, MIX_WET], "dd-master"));
}

// ---------------------------------------------------------------------------------------------
// Pedalboard

fn pedalboard(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Vec<(u32, f32)>) {
    use lg_fx::pedalboard::*;
    let g = |id| v(params, values, id);
    let choices = pedal_choices();
    let kinds: Vec<usize> = (0..SLOTS).map(|k| g(BASE + STRIDE * k as u32).round() as usize).collect();
    let sel_id = ui.scope_id().with("pb-sel");
    let mut sel: Option<usize> = ui.data(|d| d.get_temp(sel_id));
    ui.columns(2, |cols| {
        // the Pedal Browser: a category, its pedals; double-click adds (or replaces the selected)
        let ui = &mut cols[1];
        let cat_id = ui.scope_id().with("pb-cat");
        let cats = ["All", "Delay", "Distortion", "Dynamics", "Filter", "Modulation", "Pitch", "Utility"];
        let mut cat = ui.data(|d| d.get_temp::<usize>(cat_id)).unwrap_or(0);
        egui::ComboBox::from_id_salt("pb-cat-menu").selected_text(cats[cat]).show_ui(ui, |ui| {
            for (k, c) in cats.iter().enumerate() {
                ui.selectable_value(&mut cat, k, *c);
            }
        });
        ui.data_mut(|d| d.insert_temp(cat_id, cat));
        egui::ScrollArea::vertical().id_salt("pb-browser").max_height(220.0).show(ui, |ui| {
            for (k, n) in choices.iter().enumerate().skip(1) {
                if cat != 0 && category(n) != cats[cat] {
                    continue;
                }
                let r = ui.add(egui::Button::selectable(false, *n));
                if r.double_clicked() {
                    let utility = *n == "Splitter" || *n == "Mixer";
                    let target = match sel {
                        Some(s) if !utility && kinds[s] != 0 && !matches!(choices[kinds[s]], "Splitter" | "Mixer") => Some(s),
                        _ => {
                            // after the last pedal (before a Mixer at the end)
                            let last = kinds.iter().rposition(|k| *k != 0);
                            (last.map_or(0, |l| l + 1)..SLOTS).find(|s| kinds[*s] == 0)
                        }
                    };
                    if let Some(t) = target {
                        set.push((BASE + STRIDE * t as u32, k as f32));
                    }
                }
            }
        });
        ui.label(egui::RichText::new("Double-click a pedal to add it (or to replace the selected one).").size(11.0).color(look::text_dim()));
        // the Pedal area: the router line above (Bus A / B), the pedals left to right
        let ui = &mut cols[0];
        let (router, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let p = ui.painter().clone();
        let used: Vec<usize> = (0..SLOTS).filter(|k| kinds[*k] != 0).collect();
        let w = router.width() / used.len().max(1) as f32;
        p.hline(router.x_range(), router.max.y - 8.0, Stroke::new(2.0, look::line()));
        if g(BASE + 1) >= 0.0 && used.iter().any(|k| g(BASE + STRIDE * *k as u32 + 1) >= 0.5) {
            p.hline(router.x_range(), router.min.y + 8.0, Stroke::new(2.0, look::line()));
        }
        for (i, k) in used.iter().enumerate() {
            let b = g(BASE + STRIDE * *k as u32 + 1) >= 0.5;
            let r = Rect::from_min_size(pos2(router.min.x + i as f32 * w + 2.0, if b { router.min.y } else { router.max.y - 16.0 }), vec2(w - 4.0, 16.0));
            p.rect_filled(r, 3.0, look::group_bg());
            p.text(r.center(), Align2::CENTER_CENTER, choices[kinds[*k]], look::regular(10.0), look::text());
            if ui.interact(r, ui.scope_id().with(("pb-route", *k)), Sense::click()).on_hover_text("Click: move between Bus A and Bus B").clicked() {
                set.push((BASE + STRIDE * *k as u32 + 1, if b { 0.0 } else { 1.0 }));
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (i, k) in used.iter().enumerate() {
                let r = ui.add(egui::Button::selectable(sel == Some(*k), format!("{} {}", i + 1, choices[kinds[*k]])));
                if r.clicked() {
                    sel = Some(*k);
                }
                r.context_menu(|ui| {
                    if ui.button("Remove").clicked() {
                        set.push((BASE + STRIDE * *k as u32, 0.0));
                        ui.close();
                    }
                    if i > 0 && ui.button("Move Left").clicked() {
                        swap_slots(params, values, *k, used[i - 1], set);
                        ui.close();
                    }
                    if i + 1 < used.len() && ui.button("Move Right").clicked() {
                        swap_slots(params, values, *k, used[i + 1], set);
                        ui.close();
                    }
                });
            }
        });
        if let Some(s) = sel.filter(|s| kinds[*s] != 0) {
            let ids: Vec<u32> = params.iter().map(|p| p.id).filter(|id| *id > BASE + STRIDE * s as u32 + 1 && *id < BASE + STRIDE * (s as u32 + 1)).collect();
            ui.label(egui::RichText::new(choices[kinds[s]]).strong());
            set.extend(rows(ui, params, values, &ids, "pb-pedal"));
            if ui.button("Remove Pedal").clicked() {
                set.push((BASE + STRIDE * s as u32, 0.0));
            }
        }
    });
    ui.data_mut(|d| match sel {
        Some(s) => {
            d.insert_temp(sel_id, s);
        }
        None => {
            d.remove::<usize>(sel_id);
        }
    });
    // Macro Controls: a target ("Slot n — Pedal — Parameter") and its value
    egui::CollapsingHeader::new("Macro Controls").id_salt("pb-macros").show(ui, |ui| {
        let targets: Vec<(u32, &str)> = params.iter().filter(|p| p.id >= BASE && p.id < MACROS && (p.id - BASE) % STRIDE >= 2).map(|p| (p.id - BASE + 1, p.name)).collect();
        for m in 0..8u32 {
            ui.horizontal(|ui| {
                let l = (b'A' + m as u8) as char;
                let cur = g(MACROS + 2 * m).round() as u32;
                let shown = targets.iter().find(|t| t.0 == cur).map_or("—", |t| t.1);
                egui::ComboBox::from_id_salt(("pb-mt", m)).selected_text(format!("Macro {l}: {shown}")).width(300.0).show_ui(ui, |ui| {
                    if ui.selectable_label(cur == 0, "—").clicked() {
                        set.push((MACROS + 2 * m, 0.0));
                    }
                    for (id, n) in &targets {
                        if ui.selectable_label(cur == *id, *n).clicked() {
                            set.push((MACROS + 2 * m, *id as f32));
                        }
                    }
                });
                let mut val = g(MACROS + 2 * m + 1);
                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0).show_value(true)).changed() {
                    set.push((MACROS + 2 * m + 1, val));
                }
            });
        }
    });
}

/// Swaps two Pedalboard slots (pedal, bus and the pedal's settings).
fn swap_slots(params: &[Param], values: &[f32], a: usize, b: usize, set: &mut Vec<(u32, f32)>) {
    use lg_fx::pedalboard::*;
    let slot = |k: usize| -> Vec<(u32, f32)> { params.iter().zip(values).filter(|(p, _)| p.id >= BASE + STRIDE * k as u32 && p.id < BASE + STRIDE * (k as u32 + 1)).map(|(p, v)| (p.id - BASE - STRIDE * k as u32, *v)).collect() };
    let (sa, sb) = (slot(a), slot(b));
    // the pedals first (so their parameters exist), then their values
    for (dst, src) in [(a, &sb), (b, &sa)] {
        if let Some(kind) = src.iter().find(|x| x.0 == 0) {
            set.push((BASE + STRIDE * dst as u32, kind.1));
        }
    }
    for (dst, src) in [(a, &sb), (b, &sa)] {
        for (off, val) in src.iter().filter(|x| x.0 != 0) {
            set.push((BASE + STRIDE * dst as u32 + off, *val));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mastering Assistant

fn mastering(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>, act: &mut Option<Action>) {
    use lg_fx::mastering::*;
    let g = |id| v(params, values, id);
    ui.horizontal(|ui| {
        let c = g(CHARACTER).round() as usize;
        egui::ComboBox::from_id_salt("ma-char").selected_text(CHARACTERS[c.min(3)]).show_ui(ui, |ui| {
            for (k, n) in CHARACTERS.iter().enumerate() {
                if ui.selectable_label(k == c, *n).clicked() {
                    set.push((CHARACTER, k as f32));
                }
            }
        });
        let analyzed = g(ANALYZED) >= 0.5;
        if ui.button(if analyzed { "Reanalyze" } else { "Analyze" }).clicked() {
            *act = Some(Action::Reanalyze);
        }
        for (id, n) in [(BYPASS, "Bypass"), (LOUDNESS_COMP, "Loudness Compensation")] {
            let on = g(id) >= 0.5;
            if ui.add(egui::Button::selectable(on, n)).clicked() {
                set.push((id, if on { 0.0 } else { 1.0 }));
            }
        }
    });
    // spectrum + composite curve (solid) + custom EQ (dashed, three draggable points)
    let (r, p) = panel(ui, 200.0);
    let spec = live.curve("spectrum");
    curve_line(&p, r, spec, -100.0, 0.0, look::mix(look::lcd_bg(), look::text_dim(), 0.6), 1.0);
    let eq = live.curve("eq");
    curve_line(&p, r, eq, -12.0, 12.0, look::accent(), 2.0);
    p.hline(r.x_range(), r.center().y, Stroke::new(0.5, look::grid_line()));
    let fx = |f: f32| r.min.x + r.width() * ((f / 20.0).log10() / 3.0).clamp(0.0, 1.0);
    let gy = |db: f32| r.center().y - r.height() / 2.0 * (db / 12.0);
    if g(CUSTOM_EQ) >= 0.5 {
        for (k, (fid, gid)) in [(LOW_FREQ, LOW_GAIN), (MID_FREQ, MID_GAIN), (HIGH_FREQ, HIGH_GAIN)].iter().enumerate() {
            let c = pos2(fx(g(*fid)), gy(g(*gid)));
            p.circle_filled(c, 6.0, Color32::from_rgb(0x40, 0x90, 0xf0));
            let resp = ui.interact(Rect::from_center_size(c, vec2(16.0, 16.0)), ui.scope_id().with(("ma-pt", k)), Sense::drag());
            if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                let f = 20.0 * 1000f32.powf(((pp.x - r.min.x) / r.width()).clamp(0.0, 1.0));
                let gn = ((r.center().y - pp.y) / (r.height() / 2.0) * 12.0).clamp(-6.0, 6.0);
                set.push((*fid, f));
                set.push((*gid, gn));
            }
        }
    }
    // meters
    let (mr, mp) = panel(ui, 110.0);
    let row = |k: usize| Rect::from_min_size(pos2(mr.min.x + 10.0, mr.min.y + 8.0 + 16.0 * k as f32), vec2(mr.width() * 0.55, 10.0));
    meter(&mp, row(0), live.get("True Peak").unwrap_or(-120.0), -40.0, 3.0, "True Peak (dBTP)");
    mp.text(row(0).right_center() + vec2(140.0, 0.0), Align2::LEFT_CENTER, format!("max {:.1}", live.get("True Peak Max").unwrap_or(-120.0)), look::regular(11.0), look::text_dim());
    meter(&mp, row(1), live.get("Momentary").unwrap_or(-120.0), -40.0, 0.0, "M (LUFS)");
    meter(&mp, row(2), live.get("Short-Term").unwrap_or(-120.0), -40.0, 0.0, "S (LUFS)");
    meter(&mp, row(3), live.get("Integrated").unwrap_or(-120.0), -40.0, 0.0, "I (LUFS)");
    mp.text(row(4).left_center(), Align2::LEFT_CENTER, format!("LU Range {:.1}   Loudness gain {:+.1} dB", live.get("LU Range").unwrap_or(0.0), live.get("Loudness Gain").unwrap_or(0.0)), look::regular(11.0), look::text());
    let corr = live.get("Correlation").unwrap_or(1.0);
    let cr = Rect::from_min_size(pos2(mr.min.x + 10.0, mr.max.y - 18.0), vec2(mr.width() * 0.55, 10.0));
    mp.rect_filled(cr, 2.0, look::group_bg());
    mp.circle_filled(pos2(cr.center().x + cr.width() / 2.0 * corr, cr.center().y), 5.0, if corr < 0.0 { look::record_red() } else { Color32::from_rgb(0x40, 0xc0, 0x60) });
    mp.text(cr.right_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, format!("Correlation {corr:+.2}"), look::regular(11.0), look::text());
    ui.horizontal(|ui| {
        let running = g(METER_RUNNING) >= 0.5;
        if ui.button(if running { "Pause" } else { "Start" }).clicked() {
            set.push((METER_RUNNING, if running { 0.0 } else { 1.0 }));
        }
        if ui.button("Reset").clicked() {
            set.push((METER_RESET, 1.0));
        }
    });
    set.extend(rows(ui, params, values, &[AUTO_EQ, LOUDNESS, EXCITE, WIDTH, CUSTOM_EQ, LOW_FREQ, LOW_GAIN, MID_FREQ, MID_GAIN, HIGH_FREQ, HIGH_GAIN], "ma-rows"));
}

// ---------------------------------------------------------------------------------------------
// Space Designer

fn space(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>, act: &mut Option<Action>) {
    use lg_fx::space_convolver::*;
    let g = |id| v(params, values, id);
    ui.horizontal(|ui| {
        let sampled = g(IR_MODE) >= 0.5;
        if ui.add(egui::Button::selectable(!sampled, "Synthesized IR")).clicked() {
            set.push((IR_MODE, 0.0));
        }
        if ui.add(egui::Button::selectable(sampled, "Sampled IR")).clicked() {
            set.push((IR_MODE, 1.0));
        }
        let irs = user_irs();
        let cur = g(SAMPLED_IR).round() as usize;
        let shown = if cur == 0 { "No impulse response".to_string() } else { irs.get(cur - 1).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "(missing)".into()) };
        egui::ComboBox::from_id_salt("sd-ir").selected_text(shown).width(220.0).show_ui(ui, |ui| {
            for (k, p) in irs.iter().enumerate() {
                let n = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                if ui.selectable_label(cur == k + 1, n).clicked() {
                    set.push((SAMPLED_IR, (k + 1) as f32));
                    set.push((IR_MODE, 1.0));
                }
            }
        });
        if ui.button("Load IR…").clicked() {
            *act = Some(Action::LoadIr);
        }
    });
    let (r, p) = panel(ui, 140.0);
    let ir = live.curve("ir");
    let peak = ir.iter().fold(1e-9f32, |m, v| m.max(*v));
    let pts: Vec<f32> = ir.iter().map(|v| v / peak).collect();
    curve_line(&p, r, &pts, 0.0, 1.0, look::accent(), 1.5);
    p.text(r.right_top() + vec2(-8.0, 6.0), Align2::RIGHT_TOP, format!("{:.2} s", live.get("IR Length").unwrap_or(0.0)), look::regular(11.0), look::text());
    let ids: Vec<u32> = params.iter().map(|p| p.id).filter(|id| *id != IR_MODE && *id != SAMPLED_IR).collect();
    set.extend(rows(ui, params, values, &ids, "sd-rows"));
}

// ---------------------------------------------------------------------------------------------
// Match EQ

fn match_eq(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>, act: &mut Option<Action>) {
    use lg_fx::match_eq::*;
    let g = |id| v(params, values, id);
    ui.horizontal(|ui| {
        for (id, n) in [(CURRENT_LEARN, "Current Learn"), (REFERENCE_LEARN, "Reference Learn"), (MATCH, "Match")] {
            let on = g(id) >= 0.5;
            if ui.add(egui::Button::selectable(on, n)).clicked() {
                set.push((id, if on { 0.0 } else { 1.0 }));
            }
        }
        ui.menu_button("Current ⌄", |ui| {
            if ui.button("Clear Current Spectrum").clicked() {
                set.push((HAS_CURRENT, 0.0));
                ui.close();
            }
            if ui.button("Generate Current Spectrum from Audio File…").clicked() {
                *act = Some(Action::LearnFromFile(false));
                ui.close();
            }
        });
        ui.menu_button("Reference ⌄", |ui| {
            if ui.button("Clear Reference Spectrum").clicked() {
                set.push((HAS_REFERENCE, 0.0));
                ui.close();
            }
            if ui.button("Generate Reference Spectrum from Audio File…").clicked() {
                *act = Some(Action::LearnFromFile(true));
                ui.close();
            }
        });
    });
    let (r, p) = panel(ui, 200.0);
    let cur = live.curve("current");
    let refc = live.curve("reference");
    if g(HAS_CURRENT) >= 0.5 {
        curve_line(&p, r, cur, -110.0, -10.0, Color32::from_rgb(0x50, 0xd0, 0x70), 1.5);
    }
    if g(HAS_REFERENCE) >= 0.5 {
        curve_line(&p, r, refc, -110.0, -10.0, Color32::from_rgb(0xb0, 0x70, 0xf0), 1.5);
    }
    curve_line(&p, r, live.curve("eq"), -24.0, 24.0, look::accent(), 2.0);
    let ids: Vec<u32> = params.iter().map(|p| p.id).filter(|id| (APPLY..=CHANNEL).contains(id)).collect();
    set.extend(rows(ui, params, values, &ids, "meq-rows"));
}

// ---------------------------------------------------------------------------------------------
// Pitch Correction

fn pitch_correction(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>) {
    use lg_fx::pitchfx::{PC_BYPASS, PC_EDIT};
    let g = |id| v(params, values, id);
    let kb_id = ui.scope_id().with("pc-kb");
    let mut bypass_mode = ui.data(|d| d.get_temp::<bool>(kb_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        if ui.add(egui::Button::selectable(!bypass_mode, "Edit Scale")).clicked() {
            bypass_mode = false;
        }
        if ui.add(egui::Button::selectable(bypass_mode, "Bypass Notes")).clicked() {
            bypass_mode = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(kb_id, bypass_mode));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 70.0), Sense::hover());
    let p = ui.painter().clone();
    let white = [0usize, 2, 4, 5, 7, 9, 11];
    let ww = r.width() / 7.0;
    let input = live.get("Input Pitch").map(|n| (n.round() as i32).rem_euclid(12) as usize);
    let mut keys: Vec<(usize, Rect)> = white.iter().enumerate().map(|(i, pc)| (*pc, Rect::from_min_size(pos2(r.min.x + i as f32 * ww, r.min.y), vec2(ww - 1.0, r.height())))).collect();
    for (i, pc) in [1usize, 3, 6, 8, 10].iter().enumerate() {
        let at = [1.0, 2.0, 4.0, 5.0, 6.0][i];
        keys.push((*pc, Rect::from_min_size(pos2(r.min.x + at * ww - ww * 0.3, r.min.y), vec2(ww * 0.6, r.height() * 0.6))));
    }
    for (pc, kr) in &keys {
        let black = [1, 3, 6, 8, 10].contains(pc);
        let excluded = g(PC_EDIT + *pc as u32) >= 0.5;
        let byp = g(PC_BYPASS + *pc as u32) >= 0.5;
        let mut fill = if black { Color32::from_gray(30) } else { Color32::from_gray(230) };
        if excluded {
            fill = look::mix(fill, Color32::from_gray(110), 0.6);
        }
        if byp {
            fill = look::mix(fill, look::record_red(), 0.5);
        }
        if input == Some(*pc) {
            fill = look::mix(fill, look::accent(), 0.6);
        }
        p.rect_filled(*kr, 2.0, fill);
        if ui.interact(*kr, ui.scope_id().with(("pc-key", *pc)), Sense::click()).clicked() {
            let id = if bypass_mode { PC_BYPASS } else { PC_EDIT } + *pc as u32;
            set.push((id, if g(id) >= 0.5 { 0.0 } else { 1.0 }));
        }
    }
    ui.label(format!("Correction: {:+.0} cents", live.get("Correction").unwrap_or(0.0)));
    let ids: Vec<u32> = params.iter().map(|p| p.id).filter(|id| *id < PC_EDIT).collect();
    set.extend(rows(ui, params, values, &ids, "pc-rows"));
}

// ---------------------------------------------------------------------------------------------
// Meters

fn meters(ui: &mut egui::Ui, name: &str, params: &[Param], values: &[f32], live: &Live, set: &mut Vec<(u32, f32)>) {
    let (r, p) = panel(ui, if name == "MultiMeter" { 240.0 } else { 120.0 });
    let row = |k: usize| Rect::from_min_size(pos2(r.min.x + 10.0, r.min.y + 10.0 + 18.0 * k as f32), vec2(r.width() * 0.5, 12.0));
    let gget = |n| live.get(n).unwrap_or(-120.0);
    match name {
        "Level Meter" => {
            meter(&p, row(0), gget("Peak L"), -60.0, 6.0, "Peak L");
            meter(&p, row(1), gget("Peak R"), -60.0, 6.0, "Peak R");
            meter(&p, row(2), gget("RMS L"), -60.0, 6.0, "RMS L");
            meter(&p, row(3), gget("RMS R"), -60.0, 6.0, "RMS R");
            p.text(row(4).left_center(), Align2::LEFT_CENTER, format!("Hold {:.1} / {:.1} dB", gget("Hold L"), gget("Hold R")), look::regular(11.0), look::text());
        }
        "Loudness Meter" => {
            meter(&p, row(0), gget("Momentary"), -50.0, 0.0, "M");
            meter(&p, row(1), gget("Short-Term"), -50.0, 0.0, "S");
            meter(&p, row(2), gget("Integrated"), -50.0, 0.0, "I");
            p.text(row(3).left_center(), Align2::LEFT_CENTER, format!("LU Range {:.1}   True Peak max {:.1} dBTP", live.get("LU Range").unwrap_or(0.0), gget("True Peak Max")), look::regular(11.0), look::text());
        }
        "Correlation Meter" => {
            let c = live.get("Correlation").unwrap_or(1.0);
            let br = Rect::from_center_size(r.center(), vec2(r.width() * 0.8, 14.0));
            p.rect_filled(br, 3.0, look::group_bg());
            p.circle_filled(pos2(br.center().x + br.width() / 2.0 * c, br.center().y), 7.0, if c < 0.0 { look::record_red() } else { Color32::from_rgb(0x40, 0xc0, 0x60) });
            p.text(br.center_bottom() + vec2(0.0, 12.0), Align2::CENTER_TOP, format!("{c:+.2}   (−1 … 0 … +1)"), look::regular(12.0), look::text());
        }
        "BPM Counter" => {
            let bpm = live.get("BPM").unwrap_or(0.0);
            p.text(r.center(), Align2::CENTER_CENTER, if bpm > 0.0 { format!("{bpm:.1} BPM") } else { "— BPM".into() }, look::bold(28.0), look::lcd_text());
        }
        "Tuner" => {
            let note = live.get("Note");
            let cents = live.get("Cents").unwrap_or(0.0);
            let names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
            let t = note.map(|n| format!("{}{}", names[(n as i32).rem_euclid(12) as usize], (n as i32) / 12 - 1)).unwrap_or_else(|| "—".into());
            p.text(r.center() - vec2(0.0, 18.0), Align2::CENTER_CENTER, t, look::bold(28.0), look::lcd_text());
            let br = Rect::from_center_size(r.center() + vec2(0.0, 24.0), vec2(r.width() * 0.7, 8.0));
            p.rect_filled(br, 2.0, look::group_bg());
            p.circle_filled(pos2(br.center().x + br.width() / 2.0 * (cents / 50.0).clamp(-1.0, 1.0), br.center().y), 6.0, if cents.abs() < 3.0 { Color32::from_rgb(0x40, 0xc0, 0x60) } else { look::accent() });
            p.text(br.center_bottom() + vec2(0.0, 6.0), Align2::CENTER_TOP, format!("{cents:+.0} ct   {:.1} Hz", live.get("Frequency").unwrap_or(0.0)), look::regular(11.0), look::text());
        }
        _ => {
            // MultiMeter: analyzer on the left, goniometer on the right, levels and correlation
            let ar = Rect::from_min_max(r.min + vec2(8.0, 8.0), pos2(r.center().x - 8.0, r.max.y - 40.0));
            let spec = live.curve("spectrum");
            let n = spec.len().max(1);
            for (k, db) in spec.iter().enumerate() {
                let x0 = ar.min.x + ar.width() * k as f32 / n as f32;
                let h = ar.height() * ((db + 100.0) / 100.0).clamp(0.0, 1.0);
                p.rect_filled(Rect::from_min_max(pos2(x0 + 0.5, ar.max.y - h), pos2(x0 + ar.width() / n as f32 - 0.5, ar.max.y)), 0.0, look::accent());
            }
            let gr = Rect::from_center_size(pos2(r.center().x + r.width() / 4.0, ar.center().y), vec2(ar.height(), ar.height()));
            p.rect_stroke(gr, 0.0, Stroke::new(0.5, look::grid_line()), egui::StrokeKind::Inside);
            let pts = live.curve("goniometer");
            let peak = pts.iter().fold(1e-6f32, |m, v| m.max(v.abs()));
            for c in pts.chunks(2).step_by(2) {
                if let [s, m] = c {
                    p.circle_filled(pos2(gr.center().x + s / peak * gr.width() / 2.0, gr.center().y - m / peak * gr.height() / 2.0), 0.8, Color32::from_rgb(0x50, 0xd0, 0x70));
                }
            }
            let br = Rect::from_min_size(pos2(r.min.x + 10.0, r.max.y - 30.0), vec2(r.width() * 0.4, 10.0));
            meter(&p, br, gget("Peak L").max(gget("Peak R")), -60.0, 6.0, "Peak");
            let c = live.get("Correlation").unwrap_or(1.0);
            p.text(pos2(r.center().x + 10.0, r.max.y - 25.0), Align2::LEFT_CENTER, format!("Correlation {c:+.2}   I {:.1} LUFS", gget("Integrated")), look::regular(11.0), look::text());
        }
    }
    set.extend(crate::pluginwin::control_rows(ui, params, values, &[], "meter-rows"));
}

// ---------------------------------------------------------------------------------------------
// actions

/// Runs a view's action; parameters to set on the effect.
pub fn run_action(app: &mut App, slot: (usize, usize), name: &str, a: Action) -> Vec<(u32, f32)> {
    match a {
        Action::Reanalyze => {
            // the mix as it plays (the cycle if set, else the whole project), without the
            // Stereo Out's own effects
            let (tracks, sf) = match app.engine.lock() {
                Ok(e) => (e.tracks.clone(), e.soundfont.clone()),
                Err(_) => return Vec::new(),
            };
            let Some(p) = app.project.as_ref() else { return Vec::new() };
            let (tempo, map) = (p.tempo, p.tempo_map.clone());
            let end = p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| r.start + r.length)).max().unwrap_or(0);
            let (from, to) = if app.cycle_on() { app.cycle } else { (0, end) };
            if to <= from {
                app.tell("Nothing to analyze: the project is empty");
                return Vec::new();
            }
            let x = crate::audio::bounce(tracks, tempo, map, sf.as_deref(), from, to);
            let l: Vec<f32> = x.iter().step_by(2).copied().collect();
            let r: Vec<f32> = x.iter().skip(1).step_by(2).copied().collect();
            let res = lg_fx::mastering::analyze(&l, &r, crate::audio::RATE as f32);
            app.tell(format!("Mastering Assistant analyzed {:.1} s: {:.1} LUFS-I", l.len() as f32 / crate::audio::RATE as f32, res.lufs));
            lg_fx::mastering::MasteringAssistant::analysis_floats(&res)
        }
        Action::LoadIr => {
            let Some(src) = kn_ui::files::FileDialog::new().set_title("Load an impulse response").add_filter("WAV", &["wav", "WAV"]).pick_file() else { return Vec::new() };
            let Some(dir) = lg_fx::space_convolver::ir_folders().into_iter().next() else { return Vec::new() };
            let _ = std::fs::create_dir_all(&dir);
            let dest = dir.join(src.file_name().unwrap_or_default());
            if dest != src && std::fs::copy(&src, &dest).is_err() {
                app.tell("Couldn't copy the impulse response");
                return Vec::new();
            }
            let list = lg_fx::space_convolver::user_irs();
            match list.iter().position(|p| p.file_name() == dest.file_name()) {
                Some(i) => vec![(lg_fx::space_convolver::IR_MODE, 1.0), (lg_fx::space_convolver::SAMPLED_IR, (i + 1) as f32)],
                None => Vec::new(),
            }
        }
        Action::LearnFromFile(reference) => {
            let Some(path) = kn_ui::files::FileDialog::new().set_title("Generate a spectrum from an audio file").add_filter("Audio", &["wav", "aif", "aiff", "flac", "mp3", "ogg"]).pick_file() else { return Vec::new() };
            let Some(x) = crate::audio::load_audio(&path) else {
                app.tell("Couldn't read that audio file");
                return Vec::new();
            };
            let l: Vec<f32> = x.iter().step_by(2).copied().collect();
            let r: Vec<f32> = x.iter().skip(1).step_by(2).copied().collect();
            let mut m = lg_fx::match_eq::MatchEq::new(crate::audio::RATE as f32);
            m.learn_from(reference, &l, &r);
            let base = if reference { lg_fx::match_eq::REFERENCE } else { lg_fx::match_eq::CURRENT };
            let mut out: Vec<(u32, f32)> = m.spectrum(reference).into_iter().enumerate().map(|(k, v)| (base + k as u32, v)).collect();
            out.push((if reference { lg_fx::match_eq::HAS_REFERENCE } else { lg_fx::match_eq::HAS_CURRENT }, 1.0));
            out
        }
        Action::SavePattern(n) => {
            use lg_fx::beat_breaker::*;
            let Some((_, params, values)) = crate::pluginwin::effect_state_pub(app, slot.0, slot.1) else { return Vec::new() };
            let pat = (v(&params, &values, EDITING).round() as usize).clamp(1, 10) - 1;
            let mut body = format!("len {}\ncount {}\n", v(&params, &values, pat_len(pat)), v(&params, &values, pat_count(pat)));
            for s in 0..MAX_SLICES {
                for f in 0..NF {
                    body += &format!("{s} {f} {}\n", v(&params, &values, slice_id(pat, s, f)));
                }
            }
            if let Some(d) = patterns_dir() {
                let _ = std::fs::create_dir_all(&d);
                if std::fs::write(d.join(format!("{}.txt", n.replace('/', "-"))), body).is_ok() {
                    app.tell(format!("Saved the pattern “{n}”"));
                }
            }
            let _ = name;
            Vec::new()
        }
        Action::LoadPattern(path) => {
            use lg_fx::beat_breaker::*;
            let Some((_, params, values)) = crate::pluginwin::effect_state_pub(app, slot.0, slot.1) else { return Vec::new() };
            let pat = (v(&params, &values, EDITING).round() as usize).clamp(1, 10) - 1;
            let Ok(t) = std::fs::read_to_string(&path) else { return Vec::new() };
            let mut out = Vec::new();
            for l in t.lines() {
                let w: Vec<&str> = l.split_whitespace().collect();
                match w.as_slice() {
                    ["len", x] => out.push((pat_len(pat), x.parse().unwrap_or(3.0))),
                    ["count", x] => out.push((pat_count(pat), x.parse().unwrap_or(8.0))),
                    [s, f, x] => {
                        if let (Ok(s), Ok(f), Ok(x)) = (s.parse::<usize>(), f.parse::<usize>(), x.parse::<f32>()) {
                            out.push((slice_id(pat, s.min(MAX_SLICES - 1), f.min(NF - 1)), x));
                        }
                    }
                    _ => {}
                }
            }
            out
        }
    }
}
