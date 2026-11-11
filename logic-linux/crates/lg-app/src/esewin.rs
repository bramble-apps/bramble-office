//! ES E's window, laid out as Logic's: Wave and the octave buttons at the left with the LFO
//! (Vib/PWM, Speed) under Wave, the filter to their right (Cutoff, Resonance, AR Int, Velo
//! Filter), then the envelope (Attack, Release), then the effect buttons (Chorus I, Chorus II,
//! Ensemble: none lit = off) and Volume / Velo Volume at the far right; Bend Up, Bend Down and
//! Tune under Extended Parameters.

use egui::{vec2, Align2};
use lg_synth::es_emp::{e, OCT_E};

use crate::app::App;
use crate::synthui::{self, Pane, Skin};

const SKIN: Skin = Skin::new(0x3a4654, 0x2f3944, 0x8ec5ff);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "ES E", vec2(780.0, 236.0), SKIN, &[e::BEND_UP, e::BEND_DOWN, e::TUNE], body);
}

fn body(p: &mut Pane) {
    p.group(10.0, 10.0, 230.0, 216.0, "OSCILLATOR");
    p.knob(80.0, 30.0, 60.0, e::WAVE, "Wave");
    let w = p.shown(e::WAVE, p.get(e::WAVE));
    p.text(80.0, 112.0, Align2::CENTER_CENTER, &w, 10.0, p.skin.text);
    for k in 0..3u32 {
        // 4' at the top, 16' at the bottom
        let r = p.rect(170.0, 36.0 + k as f32 * 28.0, 50.0, 23.0);
        let val = 2 - k;
        let lit = p.geti(e::OCTAVE) == val as usize;
        if p.button(r, OCT_E[val as usize], lit, ("oct", k)) {
            p.put(e::OCTAVE, val as f32);
        }
    }
    p.text(22.0, 128.0, Align2::LEFT_CENTER, "LFO", 10.5, p.skin.dim);
    p.knob(60.0, 138.0, 50.0, e::VIB_PWM, "Vib/PWM");
    p.knob(140.0, 138.0, 50.0, e::SPEED, "Speed");

    p.group(250.0, 10.0, 220.0, 216.0, "FILTER");
    p.knob(310.0, 34.0, 60.0, e::CUTOFF, "Cutoff");
    p.knob(410.0, 34.0, 60.0, e::RESONANCE, "Resonance");
    p.knob(310.0, 130.0, 50.0, e::AR_INT, "AR Int");
    p.knob(410.0, 130.0, 50.0, e::VELO_FILTER, "Velo Filter");

    p.group(480.0, 10.0, 110.0, 216.0, "ENVELOPE");
    p.vfader(500.0, 36.0, 26.0, 160.0, e::ATTACK, "Attack");
    p.vfader(548.0, 36.0, 26.0, 160.0, e::RELEASE, "Release");

    p.group(600.0, 10.0, 170.0, 216.0, "OUTPUT");
    // the three effect buttons: clicking the lit one turns the effect off
    let cur = p.geti(e::CHORUS);
    for (k, name) in ["Chorus I", "Chorus II", "Ensemble"].iter().enumerate() {
        let r = p.rect(612.0, 34.0 + k as f32 * 28.0, 70.0, 23.0);
        if p.button(r, name, cur == k + 1, ("fx", k)) {
            p.put(e::CHORUS, if cur == k + 1 { 0.0 } else { (k + 1) as f32 });
        }
    }
    p.knob(730.0, 34.0, 50.0, e::VOLUME, "Volume");
    p.knob(730.0, 130.0, 50.0, e::VELO_VOLUME, "Velo Volume");
}
