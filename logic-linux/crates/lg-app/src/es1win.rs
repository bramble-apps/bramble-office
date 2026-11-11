//! ES1's window, laid out as Logic's: the oscillator at the upper left (Wave, the octave
//! buttons, Sub, Mix), the filter in the middle (Drive, the round Filter area whose word drags
//! cutoff up / down and resonance left / right, the slope buttons, Key, ADSR via Vel), the
//! amplifier at the upper right (Level via Vel, the envelope selector buttons); in the dark
//! lower area Glide, the LFO, the router's two button columns (LFO targets left, modulation
//! envelope targets right), the modulation envelope and the ADSR sliders; the global strip
//! along the lower edge (Tune, Analog, Bender Range, Out Level, Voices, Chorus); PB Range
//! Down, Filter Boost and MIDI Mode under Extended Parameters.

use egui::{pos2, vec2, Align2, Color32, Sense, Stroke};
use lg_synth::es1 as e;

use crate::app::App;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x4a4f4c, 0x3c403e, 0x7fd08a);
const BLUE: Color32 = rgb(0x5aa0e8);
const GREEN: Color32 = rgb(0x6fce7a);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "ES1", vec2(900.0, 470.0), SKIN, &[e::BEND_DOWN, e::BOOST, e::MIDI_MODE], body);
}

fn body(p: &mut Pane) {
    // ---- oscillator
    p.group(10.0, 10.0, 300.0, 222.0, "OSCILLATOR");
    for k in 0..5u32 {
        // 2' at the top, 32' at the bottom
        let r = p.rect(22.0, 34.0 + k as f32 * 36.0, 40.0, 30.0);
        let val = 4 - k;
        let lit = p.geti(e::OCTAVE) == val as usize;
        if p.button(r, e::OCTAVES[val as usize], lit, ("oct", k)) {
            p.put(e::OCTAVE, val as f32);
        }
    }
    p.knob(120.0, 60.0, 70.0, e::WAVE, "Wave");
    wave_marks(p, 120.0, 150.0);
    p.vfader(184.0, 40.0, 26.0, 150.0, e::MIX, "Mix");
    p.label(197.0, 32.0, "Sub");
    p.label(197.0, 213.0, "Osc");
    p.knob_menu(255.0, 60.0, 70.0, e::SUB, "Sub");
    let sub = e::SUBS[p.geti(e::SUB)];
    p.text(255.0, 160.0, Align2::CENTER_CENTER, sub, 10.0, p.skin.text);

    // ---- filter
    p.group(320.0, 10.0, 360.0, 222.0, "FILTER");
    p.vfader(334.0, 40.0, 26.0, 150.0, e::DRIVE, "Drive");
    filter_circle(p, 474.0, 100.0, 60.0);
    p.knob(388.0, 132.0, 40.0, e::CUTOFF, "Cutoff");
    p.knob(560.0, 132.0, 40.0, e::RESONANCE, "Reso");
    p.seg(368.0, 202.0, 216.0, 18.0, e::SLOPE, &["24 classic", "24 fat", "12 dB", "18 dB"], false);
    p.vfader(596.0, 40.0, 26.0, 140.0, e::KEY, "Key");
    p.range(640.0, 40.0, 30.0, 140.0, e::ADSR_VEL_LO, e::ADSR_VEL_HI, "ADSR via Vel", BLUE);

    // ---- amplifier
    p.group(690.0, 10.0, 200.0, 222.0, "AMPLIFIER");
    p.range(715.0, 40.0, 30.0, 150.0, e::LEVEL_VEL_LO, e::LEVEL_VEL_HI, "Level via Vel", BLUE);
    p.seg(780.0, 70.0, 90.0, 96.0, e::AMP_MODE, e::AMP_MODES, true);

    // ---- the dark lower area: modulation and envelope
    let dark = p.rect(10.0, 240.0, 880.0, 184.0);
    p.ui.painter().rect_filled(dark, 7.0, rgb(0x263029));
    p.knob(50.0, 290.0, 50.0, e::GLIDE, "Glide");
    p.text(22.0, 252.0, Align2::LEFT_CENTER, "LOW FREQUENCY OSCILLATOR", 10.5, p.skin.dim);
    p.knob_menu(130.0, 290.0, 50.0, e::LFO_WAVE, "Wave");
    p.knob(205.0, 290.0, 50.0, e::LFO_RATE, "Rate");
    let rate = p.shown(e::LFO_RATE, p.get(e::LFO_RATE));
    p.text(205.0, 372.0, Align2::CENTER_CENTER, &rate, 10.0, p.skin.text);
    p.text(130.0, 372.0, Align2::CENTER_CENTER, e::LFO_WAVES[p.geti(e::LFO_WAVE)], 10.0, p.skin.text);
    p.range(262.0, 262.0, 30.0, 136.0, e::LFO_INT_LO, e::LFO_INT_HI, "Int via Whl", GREEN);
    // the router: LFO targets in the left column, modulation envelope targets in the right
    p.text(410.0, 252.0, Align2::CENTER_CENTER, "ROUTER", 10.5, p.skin.dim);
    for (k, name) in e::ENV_TARGETS.iter().enumerate() {
        let y = 264.0 + k as f32 * 19.0;
        p.text(410.0, y + 8.0, Align2::CENTER_CENTER, name, 10.5, p.skin.text);
        if k < e::LFO_TARGETS.len() {
            let r = p.rect(322.0, y, 30.0, 16.0);
            let lit = p.geti(e::LFO_TARGET) == k;
            if p.button(r, "", lit, ("rl", k)) {
                p.put(e::LFO_TARGET, k as f32);
            }
        }
        let r = p.rect(468.0, y, 30.0, 16.0);
        let lit = p.geti(e::ENV_TARGET) == k;
        if p.button(r, "", lit, ("rr", k)) {
            p.put(e::ENV_TARGET, k as f32);
        }
    }
    p.text(540.0, 252.0, Align2::LEFT_CENTER, "MOD ENVELOPE", 10.5, p.skin.dim);
    p.knob(570.0, 290.0, 50.0, e::FORM, "Form");
    p.label(540.0, 372.0, "decay");
    p.label(570.0, 384.0, "full");
    p.label(602.0, 372.0, "attack");
    p.range(632.0, 262.0, 30.0, 136.0, e::ENV_INT_LO, e::ENV_INT_HI, "Int via Vel", GREEN);
    p.text(700.0, 252.0, Align2::LEFT_CENTER, "ADSR", 10.5, p.skin.dim);
    for (k, (id, l)) in [(e::ATTACK, "A"), (e::DECAY, "D"), (e::SUSTAIN, "S"), (e::RELEASE, "R")].into_iter().enumerate() {
        p.vfader(712.0 + k as f32 * 42.0, 262.0, 26.0, 136.0, id, l);
    }

    // ---- the global strip
    let strip = p.rect(10.0, 430.0, 880.0, 32.0);
    p.ui.painter().rect_filled(strip, 6.0, rgb(0x5a6b5e));
    let mut x = 20.0;
    for (id, label, w, step) in [(e::TUNE, "Tune", 70.0, 1.0), (e::ANALOG, "Analog", 60.0, 0.01), (e::BEND, "Bender Range", 60.0, 1.0), (e::OUT, "Out Level", 70.0, 0.5)] {
        let lw = label.len() as f32 * 6.2 + 8.0;
        p.text(x, 446.0, Align2::LEFT_CENTER, label, 10.5, Color32::from_gray(0xe8));
        p.field(x + lw, 436.0, w, 20.0, id, step);
        x += lw + w + 16.0;
    }
    p.text(x, 446.0, Align2::LEFT_CENTER, "Voices", 10.5, Color32::from_gray(0xe8));
    p.menu(x + 44.0, 436.0, 70.0, 20.0, e::VOICES_P);
    x += 130.0;
    p.text(x, 446.0, Align2::LEFT_CENTER, "Chorus", 10.5, Color32::from_gray(0xe8));
    p.menu(x + 46.0, 436.0, 60.0, 20.0, e::CHORUS_P);
}

/// The waveform symbols round the Wave knob: triangle, sawtooth, square, pulse.
fn wave_marks(p: &Pane, cx: f32, y: f32) {
    let pt = p.painter();
    let col = p.skin.dim;
    let o = p.o;
    let line = |pts: &[(f32, f32)], x: f32| {
        let v: Vec<egui::Pos2> = pts.iter().map(|&(a, b)| o + vec2(x + a, y + b)).collect();
        pt.add(egui::Shape::line(v, Stroke::new(1.2, col)));
    };
    line(&[(0.0, 8.0), (5.0, 0.0), (10.0, 8.0)], cx - 52.0);
    line(&[(0.0, 8.0), (10.0, 0.0), (10.0, 8.0)], cx - 22.0);
    line(&[(0.0, 8.0), (0.0, 0.0), (5.0, 0.0), (5.0, 8.0), (10.0, 8.0)], cx + 10.0);
    line(&[(0.0, 8.0), (0.0, 0.0), (2.0, 0.0), (2.0, 8.0), (10.0, 8.0)], cx + 40.0);
}

/// The black circle: the cutoff as an arc round it, the word Filter in the middle (drag it up
/// and down for the cutoff, left and right for the resonance).
fn filter_circle(p: &mut Pane, cx: f32, cy: f32, rad: f32) {
    let c = p.o + vec2(cx, cy);
    let pt = p.painter();
    pt.circle_filled(c, rad, Color32::from_gray(0x0c));
    let (cut, res) = (p.get(e::CUTOFF), p.get(e::RESONANCE));
    let arc = |frac: f32, r: f32, col: Color32| {
        let a0 = std::f32::consts::PI * 0.75;
        let n = 40;
        let pts: Vec<egui::Pos2> = (0..=n).map(|k| {
            let a = a0 + std::f32::consts::PI * 1.5 * frac * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        }).collect();
        pt.add(egui::Shape::line(pts, Stroke::new(3.0, col)));
    };
    arc(1.0, rad - 6.0, Color32::from_gray(0x2a));
    arc(cut, rad - 6.0, GREEN);
    arc(1.0, rad - 14.0, Color32::from_gray(0x2a));
    arc(res, rad - 14.0, BLUE);
    let word = egui::Rect::from_center_size(c, vec2(56.0, 26.0));
    let resp = p.ui.interact(word, egui::Id::unique(("es1-filter", p.t)), Sense::drag()).on_hover_text("Drag up or down for the cutoff, left or right for the resonance");
    pt.text(c, Align2::CENTER_CENTER, "Filter", look::semibold(13.0), if resp.hovered() || resp.dragged() { Color32::WHITE } else { Color32::from_gray(0xb0) });
    if resp.dragged() {
        let d = resp.drag_delta();
        if d.y != 0.0 {
            p.put(e::CUTOFF, (cut - d.y / 150.0).clamp(0.0, 1.0));
        }
        if d.x != 0.0 {
            p.put(e::RESONANCE, (res + d.x / 150.0).clamp(0.0, 1.0));
        }
        p.ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    let _ = pos2(0.0, 0.0);
}
