//! EFM1's window, laid out as Logic's: the global row at the top (Transpose, Tune, Voices,
//! Unison, Glide); the FM engine with the modulator at the left (Harmonic, Fine, Wave), FM
//! Intensity in the centre and the carrier at the right (Harmonic, Fine, Fixed Carrier); the
//! modulation envelope (with Modulator Pitch and FM Depth) at the top of the central area and
//! the LFO (LFO, Rate) at its bottom; the output row (Sub Osc Level, Stereo Detune, the volume
//! envelope, Main Level, Velocity) with the Randomize field and button at the lower right.
//! Clicking a knob's 0 label centres it. Ctrl FM / Ctrl Vibrato with their amounts, MIDI Mono
//! Mode, Mono Mode Bend Range and Pitch Bend Range under Extended Parameters.

use egui::{vec2, Align2, Sense};
use lg_synth::efm1 as f;

use crate::app::App;
use crate::synthui::{self, Pane, Skin};

const SKIN: Skin = Skin::new(0x3d4a4a, 0x2e3939, 0x5fd6c8);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "EFM1", vec2(880.0, 480.0), SKIN, &[f::CTRL_FM, f::CTRL_FM_AMT, f::CTRL_VIB, f::CTRL_VIB_AMT, f::MONO_MODE, f::MONO_BEND, f::BEND], body);
}

/// The 0 mark over a centre-zero knob: a click puts the knob there.
fn zero(p: &mut Pane, x: f32, y: f32, id: u32) {
    let r = p.rect(x - 9.0, y - 7.0, 18.0, 14.0);
    let resp = p.ui.interact(r, egui::Id::unique(("efm1-zero", p.t, id)), Sense::click());
    p.text(x, y, Align2::CENTER_CENTER, "0", 10.5, if resp.hovered() { p.skin.text } else { p.skin.dim });
    if resp.clicked() {
        p.put(id, 0.0);
    }
}

fn body(p: &mut Pane) {
    // ---- global
    p.group(10.0, 10.0, 860.0, 40.0, "");
    let mut x = 22.0;
    p.text(x, 30.0, Align2::LEFT_CENTER, "Transpose", 10.5, p.skin.dim);
    p.menu(x + 62.0, 20.0, 80.0, 20.0, f::TRANSPOSE_P);
    x += 162.0;
    p.text(x, 30.0, Align2::LEFT_CENTER, "Tune", 10.5, p.skin.dim);
    p.field(x + 34.0, 20.0, 70.0, 20.0, f::TUNE, 1.0);
    x += 124.0;
    p.text(x, 30.0, Align2::LEFT_CENTER, "Voices", 10.5, p.skin.dim);
    p.menu(x + 44.0, 20.0, 80.0, 20.0, f::VOICES_P);
    x += 144.0;
    p.toggle(x, 20.0, 70.0, 20.0, f::UNISON, "Unison");
    x += 90.0;
    p.text(x, 30.0, Align2::LEFT_CENTER, "Glide", 10.5, p.skin.dim);
    p.field(x + 36.0, 20.0, 70.0, 20.0, f::GLIDE, 0.01);

    // ---- modulator, FM, carrier
    p.group(10.0, 58.0, 250.0, 300.0, "MODULATOR");
    p.knob(90.0, 84.0, 84.0, f::MOD_HARMONIC, "Harmonic");
    let h = p.shown(f::MOD_HARMONIC, p.get(f::MOD_HARMONIC));
    p.text(90.0, 194.0, Align2::CENTER_CENTER, &h, 12.0, p.skin.text);
    p.knob(200.0, 96.0, 50.0, f::MOD_FINE, "Fine");
    zero(p, 200.0, 88.0, f::MOD_FINE);
    p.knob(90.0, 250.0, 56.0, f::MOD_WAVE, "Wave");
    p.label(52.0, 326.0, "sine");
    p.label(130.0, 326.0, "digital");

    p.group(620.0, 58.0, 250.0, 300.0, "CARRIER");
    p.knob(790.0, 84.0, 84.0, f::CAR_HARMONIC, "Harmonic");
    let h = p.shown(f::CAR_HARMONIC, p.get(f::CAR_HARMONIC));
    p.text(790.0, 194.0, Align2::CENTER_CENTER, &h, 12.0, p.skin.text);
    p.knob(680.0, 96.0, 50.0, f::CAR_FINE, "Fine");
    zero(p, 680.0, 88.0, f::CAR_FINE);
    p.toggle(740.0, 262.0, 100.0, 24.0, f::FIXED, "Fixed Carrier");

    // the central area: modulation envelope at the top, FM in the middle, LFO at the bottom
    p.group(270.0, 58.0, 340.0, 300.0, "MODULATION ENV");
    for (k, (id, l)) in [(f::ME_A, "A"), (f::ME_D, "D"), (f::ME_S, "S"), (f::ME_R, "R")].into_iter().enumerate() {
        p.vfader(286.0 + k as f32 * 28.0, 82.0, 22.0, 84.0, id, l);
    }
    p.knob(450.0, 92.0, 44.0, f::MOD_PITCH, "Mod Pitch");
    zero(p, 450.0, 84.0, f::MOD_PITCH);
    p.knob(550.0, 92.0, 44.0, f::FM_DEPTH, "FM Depth");
    zero(p, 550.0, 84.0, f::FM_DEPTH);
    p.knob(440.0, 172.0, 90.0, f::FM, "FM");
    p.text(282.0, 286.0, Align2::LEFT_CENTER, "LFO", 10.5, p.skin.dim);
    p.knob(350.0, 290.0, 44.0, f::LFO, "LFO");
    zero(p, 350.0, 282.0, f::LFO);
    p.label(312.0, 344.0, "Vib");
    p.label(388.0, 344.0, "FM");
    p.knob(530.0, 290.0, 44.0, f::RATE, "Rate");

    // ---- output
    p.group(10.0, 366.0, 860.0, 104.0, "");
    p.knob(60.0, 384.0, 50.0, f::SUB, "Sub Osc");
    p.knob(160.0, 384.0, 50.0, f::STEREO, "Stereo");
    p.text(240.0, 378.0, Align2::LEFT_CENTER, "VOLUME ENV", 10.5, p.skin.dim);
    for (k, (id, l)) in [(f::VE_A, "A"), (f::VE_D, "D"), (f::VE_S, "S"), (f::VE_R, "R")].into_iter().enumerate() {
        p.vfader(250.0 + k as f32 * 30.0, 388.0, 22.0, 62.0, id, l);
    }
    p.knob(440.0, 384.0, 50.0, f::LEVEL, "Main Level");
    p.knob(540.0, 384.0, 50.0, f::VELOCITY, "Velocity");
    // Randomize: the amount field and the button
    p.text(640.0, 420.0, Align2::LEFT_CENTER, "Randomize", 10.5, p.skin.dim);
    p.field(706.0, 410.0, 60.0, 20.0, f::RANDOMIZE, 0.01);
    let r = p.rect(774.0, 408.0, 84.0, 24.0);
    if p.button(r, "Randomize", false, "rnd") {
        let seed = (p.ui.input(|i| i.time) * 1000.0) as u32;
        let amount = p.get(f::RANDOMIZE);
        let new = f::randomized(&|id| p.get(id), amount, seed);
        for (id, v) in new {
            p.put(id, v);
        }
    }
}
