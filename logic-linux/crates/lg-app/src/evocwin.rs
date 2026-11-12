//! EVOC 20 PolySynth's window, in Logic's six sections: Sidechain Analysis (Attack, Release,
//! Freeze, Bands), U/V Detection (Sensitivity, Mode, Level), Synthesis (voices and mode,
//! oscillators in dual or FM mode, noise, tuning, the low-pass filter, envelope), Formant Filter
//! (the band display split into analysis above and synthesis below, the frequency range bar with
//! its silver handles, Low / High, Lowest / Highest, Resonance, Stretch, Shift), Modulation (Pitch
//! and Shift LFOs, the Int via Whl range) and Output (Signal, Ensemble, Stereo Width, Level). The
//! Analysis Input choice is the Side Chain pop-up of the plug-in header.

use egui::{pos2, vec2, Align2, Color32, Sense, Stroke};
use lg_synth::evoc_ps::{self as e, EvocPs};

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x2f3a3f, 0x252d31, 0x6fc2d8);
const SILVER: Color32 = rgb(0xc8ccd0);
const BLUE: Color32 = rgb(0x4a8fe0);
const ORANGE: Color32 = rgb(0xe8a04a);

struct Live {
    disp: e::BandDisplay,
    voices: usize,
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let live = instwin::with_inst(app, t, |s: &mut EvocPs| Live { disp: s.display(), voices: s.voices_shown() });
    let Some(live) = live else { return };
    synthui::window(app, ctx, t, "EVOC 20 PolySynth", vec2(900.0, 470.0), SKIN, &[], |p| body(p, &live));
    // the band display moves with the sound
    ctx.request_repaint_after(std::time::Duration::from_millis(40));
}

fn cap(p: &Pane, x: f32, y: f32, s: &str) {
    p.text(x, y, Align2::LEFT_CENTER, s, 10.0, p.skin.dim);
}

fn body(p: &mut Pane, live: &Live) {
    sidechain(p);
    uv(p);
    synthesis(p, live);
    formant(p, live);
    modulation(p);
    output(p);
}

fn sidechain(p: &mut Pane) {
    p.group(10.0, 10.0, 240.0, 180.0, "SIDECHAIN ANALYSIS");
    p.knob(52.0, 36.0, 46.0, e::ATTACK, "Attack");
    p.knob(122.0, 36.0, 46.0, e::RELEASE, "Release");
    let frozen = p.on(e::FREEZE);
    let r = p.rect(176.0, 40.0, 62.0, 26.0);
    if p.button(r, "Freeze", frozen, "freeze") {
        p.put(e::FREEZE, if frozen { 0.0 } else { 1.0 });
    }
    cap(p, 178.0, 84.0, "Bands");
    p.field(176.0, 92.0, 62.0, 20.0, e::BANDS, 0.12);
    p.text(20.0, 138.0, Align2::LEFT_CENTER, "Analysis In: the Side Chain pop-up", 10.0, p.skin.dim);
    p.text(20.0, 153.0, Align2::LEFT_CENTER, "in the plug-in header (track, bus or input)", 10.0, p.skin.dim);
    if frozen {
        p.text(20.0, 172.0, Align2::LEFT_CENTER, "Frozen: Attack and Release have no effect", 10.0, Color32::from_rgb(0xf0, 0xc8, 0x70));
    }
}

fn uv(p: &mut Pane) {
    p.group(260.0, 10.0, 190.0, 180.0, "U/V DETECTION");
    let blend = p.geti(e::UV_MODE) == 2;
    p.knob(300.0, 36.0, 46.0, e::UV_SENS, if blend { "Sensitivity (off)" } else { "Sensitivity" });
    p.knob(396.0, 36.0, 46.0, e::UV_LEVEL, "Level");
    cap(p, 270.0, 128.0, "Mode");
    p.menu(270.0, 138.0, 170.0, 20.0, e::UV_MODE);
}

fn synthesis(p: &mut Pane, live: &Live) {
    p.group(460.0, 10.0, 430.0, 336.0, "SYNTHESIS");
    // global: voice mode, voices, unison
    p.seg(470.0, 30.0, 150.0, 18.0, e::VOICE_MODE, e::VOICE_MODES, false);
    p.text(632.0, 39.0, Align2::LEFT_CENTER, "Voices", 10.5, p.skin.dim);
    let poly = p.geti(e::VOICE_MODE) == 0;
    // the field shows the stack in Unison mono; it can be set only in Poly mode
    if poly {
        p.field(676.0, 30.0, 40.0, 18.0, e::VOICES, 0.1);
    } else {
        let rc = p.rect(676.0, 30.0, 40.0, 18.0);
        p.ui.painter().rect_filled(rc, 4.0, Color32::from_gray(0x22));
        p.ui.painter().text(rc.center(), Align2::CENTER_CENTER, format!("{}", live.voices), look::regular(10.5), p.skin.dim);
    }
    if poly && p.on(e::UNISON) {
        p.text(722.0, 56.0, Align2::LEFT_CENTER, &format!("{} voices", live.voices), 9.5, p.skin.dim);
    }
    p.toggle(722.0, 30.0, 60.0, 18.0, e::UNISON, "Unison");
    // oscillators
    p.seg(470.0, 60.0, 90.0, 18.0, e::OSC_MODE, e::OSC_MODES, false);
    p.seg(570.0, 60.0, 90.0, 18.0, e::OCTAVE, e::OCTAVES, false);
    p.text(672.0, 69.0, Align2::LEFT_CENTER, "Osc 1 range", 10.0, p.skin.dim);
    if p.geti(e::OSC_MODE) == 0 {
        for (k, (id, label, step)) in [(e::WAVE1, "Wave 1", 0.2f32), (e::WAVE2, "Wave 2", 0.2), (e::SEMI, "Semi", 0.1), (e::DETUNE, "Detune", 0.2)].into_iter().enumerate() {
            let x = 470.0 + k as f32 * 76.0;
            cap(p, x, 92.0, label);
            p.field(x, 100.0, 68.0, 18.0, id, step);
        }
        cap(p, 470.0, 130.0, "Balance (osc 1 - osc 2)");
        p.hfader(470.0, 136.0, 290.0, 16.0, e::BALANCE);
    } else {
        for (k, (id, label, step)) in [(e::WAVE2, "Wave 2", 0.2f32), (e::FM_COARSE, "Ratio coarse", 0.1), (e::FM_FINE, "Ratio fine", 0.3)].into_iter().enumerate() {
            let x = 470.0 + k as f32 * 76.0;
            cap(p, x, 92.0, label);
            p.field(x, 100.0, 68.0, 18.0, id, step);
        }
        p.text(700.0, 109.0, Align2::LEFT_CENTER, "Osc 1 is a sine; Wave 1 is off", 9.5, p.skin.dim);
        cap(p, 470.0, 130.0, "FM Int");
        p.hfader(470.0, 136.0, 290.0, 16.0, e::FM_INT);
    }
    // noise and tuning
    p.knob(496.0, 164.0, 40.0, e::NOISE_LEVEL, "Noise Level");
    p.knob(560.0, 164.0, 40.0, e::NOISE_COLOR, "Noise Color");
    cap(p, 628.0, 170.0, "Tune");
    p.field(628.0, 178.0, 60.0, 18.0, e::TUNE, 0.5);
    p.knob(724.0, 164.0, 40.0, e::ANALOG, "Analog");
    p.knob(788.0, 164.0, 40.0, e::GLIDE, "Glide");
    cap(p, 840.0, 170.0, "Bend");
    p.field(840.0, 178.0, 40.0, 18.0, e::BEND, 0.1);
    // filter and envelope
    p.text(470.0, 238.0, Align2::LEFT_CENTER, "FILTER (before the formant filter)", 10.0, p.skin.dim);
    p.knob(496.0, 252.0, 44.0, e::CUTOFF, "Cutoff");
    p.knob(570.0, 252.0, 44.0, e::FILTER_RES, "Resonance");
    p.text(650.0, 238.0, Align2::LEFT_CENTER, "ENVELOPE", 10.0, p.skin.dim);
    p.hfader(650.0, 254.0, 220.0, 18.0, e::ENV_ATTACK);
    p.text(650.0, 282.0, Align2::LEFT_CENTER, "Attack", 10.0, p.skin.dim);
    p.hfader(650.0, 296.0, 220.0, 18.0, e::ENV_RELEASE);
    p.text(650.0, 324.0, Align2::LEFT_CENTER, "Release", 10.0, p.skin.dim);
}

/// Frequency (20 Hz - 20 kHz, log) to a fraction of the display's width.
fn fx(f: f32) -> f32 {
    ((f.max(20.0) / 20.0).ln() / 1000f32.ln()).clamp(0.0, 1.0)
}

fn formant(p: &mut Pane, live: &Live) {
    p.group(10.0, 196.0, 440.0, 156.0, "FORMANT FILTER");
    let r = p.display(20.0, 214.0, 420.0, 84.0);
    let mid = r.center().y + 4.0;
    let pt = p.painter().with_clip_rect(r);
    // the line dividing analysis (above) from synthesis (below)
    pt.line_segment([pos2(r.min.x, mid), pos2(r.max.x, mid)], Stroke::new(1.0, Color32::from_gray(0x60)));
    pt.text(r.min + vec2(4.0, 22.0), Align2::LEFT_CENTER, "Analysis", look::regular(9.0), Color32::from_gray(0x80));
    pt.text(pos2(r.min.x + 4.0, r.max.y - 8.0), Align2::LEFT_CENTER, "Synthesis", look::regular(9.0), Color32::from_gray(0x80));
    let n = live.disp.n.max(1);
    let up = mid - (r.min.y + 18.0);
    let down = r.max.y - 4.0 - mid;
    for k in 0..n {
        let (ca, cs) = (live.disp.analysis[k], live.disp.synthesis[k]);
        let lvl = (live.disp.levels[k] * 10.0).min(1.0).sqrt();
        // band widths from the spacing of the centres (each bank's own)
        let width = |c: &[f32; e::MAX_BANDS], i: usize| {
            let (a, b) = if i + 1 < n { (c[i], c[i + 1]) } else if i > 0 { (c[i - 1], c[i]) } else { (c[i] * 0.7, c[i] * 1.4) };
            ((fx(b) - fx(a)).abs() * r.width() * 0.8).max(2.0)
        };
        let (wa, ws) = (width(&live.disp.analysis, k), width(&live.disp.synthesis, k));
        let xa = r.min.x + fx(ca) * r.width();
        let xs = r.min.x + fx(cs) * r.width();
        pt.rect_filled(egui::Rect::from_min_max(pos2(xa - wa / 2.0, mid - 3.0 - up * lvl), pos2(xa + wa / 2.0, mid - 1.0)), 1.0, BLUE.gamma_multiply(0.35 + 0.65 * lvl));
        pt.rect_stroke(egui::Rect::from_min_max(pos2(xa - wa / 2.0, mid - up * 0.55), pos2(xa + wa / 2.0, mid - 1.0)), 1.0, Stroke::new(0.6, BLUE.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        pt.rect_filled(egui::Rect::from_min_max(pos2(xs - ws / 2.0, mid + 1.0), pos2(xs + ws / 2.0, mid + 3.0 + down * lvl)), 1.0, ORANGE.gamma_multiply(0.35 + 0.65 * lvl));
        pt.rect_stroke(egui::Rect::from_min_max(pos2(xs - ws / 2.0, mid + 1.0), pos2(xs + ws / 2.0, mid + down * 0.55)), 1.0, Stroke::new(0.6, ORANGE.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    }
    // the blue range bar at the top with a silver handle at each end
    let (lo, hi) = (p.get(e::LOW_FREQ), p.get(e::HIGH_FREQ));
    let (x0, x1) = (r.min.x + fx(lo) * r.width(), r.min.x + fx(hi) * r.width());
    let bar = egui::Rect::from_min_max(pos2(x0, r.min.y + 3.0), pos2(x1.max(x0 + 6.0), r.min.y + 13.0));
    let bid = p.id("fbar", 0);
    let bresp = p.ui.interact(bar.shrink2(vec2(6.0, 0.0)), bid, Sense::drag()).on_hover_text("Drag to move the whole range; the silver handles set Low and High Frequency");
    let hl_id = p.id("fbar-lo", 0);
    let hh_id = p.id("fbar-hi", 0);
    let hl = p.ui.interact(egui::Rect::from_center_size(pos2(x0, bar.center().y), vec2(12.0, 14.0)), hl_id, Sense::drag());
    let hh = p.ui.interact(egui::Rect::from_center_size(pos2(x1, bar.center().y), vec2(12.0, 14.0)), hh_id, Sense::drag());
    {
        let pt = p.painter().with_clip_rect(r.expand(2.0));
        pt.rect_filled(bar, 3.0, if bresp.hovered() || bresp.dragged() { BLUE } else { BLUE.gamma_multiply(0.8) });
        pt.rect_filled(egui::Rect::from_center_size(pos2(x0, bar.center().y), vec2(6.0, 12.0)), 2.0, SILVER);
        pt.rect_filled(egui::Rect::from_center_size(pos2(x1, bar.center().y), vec2(6.0, 12.0)), 2.0, SILVER);
    }
    let per_px = 1000f32.ln() / r.width();
    if hl.dragged() {
        p.put(e::LOW_FREQ, (lo * (hl.drag_delta().x * per_px).exp()).clamp(20.0, (hi / 1.2).min(2000.0)));
    } else if hh.dragged() {
        p.put(e::HIGH_FREQ, (hi * (hh.drag_delta().x * per_px).exp()).clamp((lo * 1.2).max(1000.0), 20000.0));
    } else if bresp.dragged() {
        let k = (bresp.drag_delta().x * per_px).exp();
        let k = k.clamp(20.0 / lo, 2000.0 / lo).clamp(1000.0 / hi, 20000.0 / hi);
        p.put(e::LOW_FREQ, lo * k);
        p.put(e::HIGH_FREQ, hi * k);
    }
    // below the display: Low / High, Lowest / Highest, Resonance, Stretch, Shift
    cap(p, 22.0, 306.0, "Low");
    p.field(22.0, 314.0, 62.0, 18.0, e::LOW_FREQ, 0.5);
    cap(p, 92.0, 306.0, "High");
    p.field(92.0, 314.0, 62.0, 18.0, e::HIGH_FREQ, 2.0);
    let lowhp = p.geti(e::LOWEST_MODE) == 1;
    let rb = p.rect(22.0, 336.0, 66.0, 14.0);
    if p.button(rb, if lowhp { "Lowest: HP" } else { "Lowest: BP" }, lowhp, "lowest") {
        p.put(e::LOWEST_MODE, if lowhp { 0.0 } else { 1.0 });
    }
    let highlp = p.geti(e::HIGHEST_MODE) == 1;
    let rb = p.rect(92.0, 336.0, 66.0, 14.0);
    if p.button(rb, if highlp { "Highest: LP" } else { "Highest: BP" }, highlp, "highest") {
        p.put(e::HIGHEST_MODE, if highlp { 0.0 } else { 1.0 });
    }
    p.knob(205.0, 302.0, 30.0, e::RESONANCE, "Resonance");
    p.knob(283.0, 302.0, 30.0, e::STRETCH, "Stretch");
    p.knob(361.0, 302.0, 30.0, e::SHIFT, "Shift");
}

fn modulation(p: &mut Pane) {
    p.group(10.0, 358.0, 440.0, 104.0, "MODULATION");
    // Pitch LFO (left column) and Shift LFO (right column): rate left of centre is tempo-synced
    p.text(20.0, 386.0, Align2::LEFT_CENTER, "Pitch LFO", 10.0, p.skin.dim);
    p.knob(52.0, 392.0, 30.0, e::PITCH_RATE, "Rate");
    p.text(52.0, 392.0 + 32.0, Align2::CENTER_CENTER, &e::rate_text(p.get(e::PITCH_RATE)), 9.0, p.skin.text);
    p.menu(86.0, 390.0, 96.0, 18.0, e::PITCH_WAVE);
    // Int via Whl: the left arrow is the intensity with the wheel at its minimum, the right with it at its maximum
    p.range(86.0, 420.0, 130.0, 24.0, e::PITCH_INT_MIN, e::PITCH_INT_MAX, "Int via Whl", Color32::from_rgb(0x5f, 0xb0, 0x6a));
    p.text(232.0, 386.0, Align2::LEFT_CENTER, "Shift LFO", 10.0, p.skin.dim);
    p.knob(266.0, 392.0, 30.0, e::SHIFT_RATE, "Rate");
    p.text(266.0, 392.0 + 32.0, Align2::CENTER_CENTER, &e::rate_text(p.get(e::SHIFT_RATE)), 9.0, p.skin.text);
    p.menu(300.0, 390.0, 100.0, 18.0, e::SHIFT_WAVE);
    p.text(300.0, 424.0, Align2::LEFT_CENTER, "Intensity", 10.0, p.skin.dim);
    p.hfader(300.0, 430.0, 130.0, 18.0, e::SHIFT_INT);
}

fn output(p: &mut Pane) {
    p.group(460.0, 358.0, 430.0, 104.0, "OUTPUT");
    cap(p, 470.0, 386.0, "Signal");
    p.seg(470.0, 394.0, 126.0, 20.0, e::SIGNAL, e::SIGNALS, false);
    cap(p, 610.0, 386.0, "Ensemble");
    p.seg(610.0, 394.0, 116.0, 20.0, e::ENSEMBLE, e::ENSEMBLES, false);
    p.knob(780.0, 380.0, 38.0, e::WIDTH, "Stereo Width");
    let w = p.get(e::WIDTH);
    p.text(836.0, 400.0, Align2::LEFT_CENTER, if w < 0.05 { "centre" } else if w < 0.5 { "narrow" } else if w < 0.95 { "ascending" } else { "alternating" }, 9.5, p.skin.dim);
    cap(p, 470.0, 428.0, "Level");
    p.hfader(510.0, 420.0, 220.0, 18.0, e::LEVEL);
    let t = p.shown(e::LEVEL, p.get(e::LEVEL));
    p.text(740.0, 429.0, Align2::LEFT_CENTER, &t, 10.0, p.skin.text);
}
