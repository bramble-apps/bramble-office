//! ES M's window, laid out as Logic's: Mix (sawtooth to rectangle), the octave buttons and
//! Glide at the left, the filter's Cutoff and Resonance to their right, the filter envelope
//! (Int, Decay, Velo) at the upper right, the level envelope and output (Decay, Velo, Vol) in
//! the angled area at the lower right with Overdrive near the right edge halfway up; Pitch Bend
//! Up / Down and Tune under Extended Parameters.

use egui::{vec2, Align2, Color32, Stroke};
use lg_synth::es_emp::{m, OCT_M};

use crate::app::App;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x4a4640, 0x3b3833, 0xf0b35a);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "ES M", vec2(720.0, 270.0), SKIN, &[m::BEND_UP, m::BEND_DOWN, m::TUNE], body);
}

fn body(p: &mut Pane) {
    p.group(10.0, 10.0, 220.0, 250.0, "OSCILLATOR");
    p.knob(80.0, 36.0, 64.0, m::MIX, "Mix");
    // sawtooth at the left of the knob, rectangle at the right
    let pt = p.painter();
    let o = p.o;
    let line = |pts: &[(f32, f32)]| {
        pt.add(egui::Shape::line(pts.iter().map(|&(a, b)| o + vec2(a, b)).collect(), Stroke::new(1.2, Color32::from_gray(0xa0))));
    };
    line(&[(30.0, 112.0), (42.0, 100.0), (42.0, 112.0)]);
    line(&[(118.0, 112.0), (118.0, 100.0), (124.0, 100.0), (124.0, 112.0), (130.0, 112.0)]);
    for k in 0..3u32 {
        // 8' at the top, 32' at the bottom
        let r = p.rect(160.0, 40.0 + k as f32 * 28.0, 50.0, 23.0);
        let val = 2 - k;
        let lit = p.geti(m::OCTAVE) == val as usize;
        if p.button(r, OCT_M[val as usize], lit, ("oct", k)) {
            p.put(m::OCTAVE, val as f32);
        }
    }
    p.knob(80.0, 160.0, 50.0, m::GLIDE, "Glide");

    p.group(240.0, 10.0, 200.0, 250.0, "FILTER");
    p.knob(340.0, 40.0, 80.0, m::CUTOFF, "Cutoff");
    p.knob(340.0, 160.0, 56.0, m::RESONANCE, "Resonance");

    p.group(450.0, 10.0, 260.0, 110.0, "FILTER ENVELOPE");
    p.knob(500.0, 34.0, 50.0, m::INT, "Int");
    p.knob(580.0, 34.0, 50.0, m::FILTER_DECAY, "Decay");
    p.knob(660.0, 34.0, 50.0, m::FILTER_VELO, "Velo");

    // the angled area: level envelope and output
    let pts = vec![o + vec2(450.0, 150.0), o + vec2(480.0, 128.0), o + vec2(710.0, 128.0), o + vec2(710.0, 260.0), o + vec2(450.0, 260.0)];
    pt.add(egui::Shape::convex_polygon(pts, rgb(0x2c2a26), Stroke::NONE));
    p.text(490.0, 140.0, Align2::LEFT_CENTER, "VOLUME", 10.5, p.skin.dim);
    p.knob(500.0, 168.0, 50.0, m::DECAY, "Decay");
    p.knob(580.0, 168.0, 50.0, m::VELO, "Velo");
    p.knob(660.0, 168.0, 50.0, m::VOL, "Vol");
    // Overdrive: near the right edge, halfway up
    p.knob(684.0, 104.0, 34.0, m::OVERDRIVE, "Overdrive");
}
