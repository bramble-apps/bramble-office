//! ES P's window, laid out as Logic's: the six oscillator level sliders (triangle, sawtooth,
//! rectangle, sub-oscillator −1 and −2 octaves, noise) and the octave buttons at the left, the
//! LFO (Vib/Wah, Speed) to their right, the filter column in the centre (Frequency, Resonance,
//! the 1/3 2/3 3/3 key follow buttons — none lit = no key follow —, ADSR Int, Velo Filter), the
//! envelope sliders and Volume / Velo Volume right of the filter, Chorus and Overdrive at the
//! far right. VCA Mode (ADSR / Gate), Bend Up / Down and Tune under Extended Parameters.

use egui::{vec2, Align2};
use lg_synth::es_emp::{p as q, OCT_P};

use crate::app::App;
use crate::synthui::{self, Pane, Skin};

const SKIN: Skin = Skin::new(0x40444e, 0x32353d, 0xff8f6b);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "ES P", vec2(900.0, 262.0), SKIN, &[q::VCA_MODE, q::BEND_UP, q::BEND_DOWN, q::TUNE], body);
}

fn body(p: &mut Pane) {
    p.group(10.0, 10.0, 300.0, 242.0, "OSCILLATOR");
    for (k, (id, l)) in [(q::TRIANGLE, "Tri"), (q::SAWTOOTH, "Saw"), (q::RECTANGLE, "Rect"), (q::SUB1, "−1 Oct"), (q::SUB2, "−2 Oct"), (q::NOISE, "Noise")].into_iter().enumerate() {
        p.vfader(22.0 + k as f32 * 37.0, 34.0, 26.0, 186.0, id, l);
    }
    for k in 0..3u32 {
        // 4' at the top, 16' at the bottom
        let r = p.rect(252.0, 60.0 + k as f32 * 30.0, 46.0, 24.0);
        let val = 2 - k;
        let lit = p.geti(q::OCTAVE) == val as usize;
        if p.button(r, OCT_P[val as usize], lit, ("oct", k)) {
            p.put(q::OCTAVE, val as f32);
        }
    }

    p.group(320.0, 10.0, 100.0, 242.0, "LFO");
    p.knob(370.0, 36.0, 56.0, q::VIB_WAH, "Vib/Wah");
    p.label(338.0, 122.0, "Vib");
    p.label(402.0, 122.0, "Wah");
    p.knob(370.0, 146.0, 56.0, q::SPEED, "Speed");

    p.group(430.0, 10.0, 150.0, 242.0, "FILTER");
    p.knob(470.0, 30.0, 50.0, q::FREQUENCY, "Frequency");
    p.knob(540.0, 30.0, 50.0, q::RESONANCE, "Resonance");
    // key follow: three buttons, clicking the lit one turns key follow off
    let cur = p.geti(q::KEY);
    p.text(442.0, 116.0, Align2::LEFT_CENTER, "Key Follow", 10.5, p.skin.dim);
    for (k, name) in ["1/3", "2/3", "3/3"].iter().enumerate() {
        let r = p.rect(442.0 + k as f32 * 44.0, 126.0, 40.0, 20.0);
        if p.button(r, name, cur == k + 1, ("key", k)) {
            p.put(q::KEY, if cur == k + 1 { 0.0 } else { (k + 1) as f32 });
        }
    }
    p.knob(470.0, 162.0, 50.0, q::ADSR_INT, "ADSR Int");
    p.knob(540.0, 162.0, 50.0, q::VELO_FILTER, "Velo Filter");

    p.group(590.0, 10.0, 210.0, 242.0, "ENVELOPE");
    for (k, (id, l)) in [(q::ATTACK, "A"), (q::DECAY, "D"), (q::SUSTAIN, "S"), (q::RELEASE, "R")].into_iter().enumerate() {
        p.vfader(602.0 + k as f32 * 30.0, 34.0, 24.0, 186.0, id, l);
    }
    p.knob(762.0, 36.0, 50.0, q::VOLUME, "Volume");
    p.knob(762.0, 146.0, 50.0, q::VELO_VOLUME, "Velo Volume");

    p.group(810.0, 10.0, 80.0, 242.0, "EFFECTS");
    p.knob(850.0, 36.0, 50.0, q::CHORUS, "Chorus");
    p.knob(850.0, 146.0, 50.0, q::OVERDRIVE, "Overdrive");
}
