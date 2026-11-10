//! The smaller Logic synths as front panels over our engines: each keeps its own short parameter
//! list (as on its window) and voices the ES2 or Retro engine underneath.
//! ES1 (one oscillator + sub), ES M (mono bass), ES P (polysynth with six level sliders),
//! ES E (string-ensemble machine), EFM1 (two-operator FM).

use crate::poly::Params;
use lg_dsp::{Instrument, Param, Plugin, Unit};

/// A front panel: its own parameters, mapped onto an inner instrument's.
pub struct Facade {
    inner: Box<dyn Instrument>,
    params: Params,
    map: fn(&Params) -> Vec<(u32, f32)>,
}

impl Facade {
    pub fn new(
        inner: Box<dyn Instrument>,
        defs: &'static [Param],
        map: fn(&Params) -> Vec<(u32, f32)>,
    ) -> Facade {
        let mut f = Facade {
            inner,
            params: Params::new(defs),
            map,
        };
        f.apply();
        f
    }
    fn apply(&mut self) {
        for (id, v) in (self.map)(&self.params) {
            self.inner.set(id, v);
        }
    }
}

impl Plugin for Facade {
    fn params(&self) -> &[Param] {
        self.params.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.apply();
        }
    }
    fn reset(&mut self) {
        self.inner.reset();
        self.apply();
    }
    fn tail(&self) -> f32 {
        self.inner.tail()
    }
}

impl Instrument for Facade {
    fn note_on(&mut self, p: u8, v: u8) {
        self.inner.note_on(p, v)
    }
    fn note_off(&mut self, p: u8) {
        self.inner.note_off(p)
    }
    fn controller(&mut self, cc: u8, v: u8) {
        self.inner.controller(cc, v)
    }
    fn pitch_bend(&mut self, v: i16) {
        self.inner.pitch_bend(v)
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.inner.render(l, r)
    }
}

const SUBS: &[&str] = &["Off", "Square -1", "Square -2", "Pulse -2", "Noise"];
const LFO_W: &[&str] = &[
    "Sine",
    "Triangle",
    "Saw Up",
    "Saw Down",
    "Square",
    "Sample & Hold",
    "Random",
];
const CHORUS: &[&str] = &["Off", "Chorus", "Ensemble"];
const MONO: &[&str] = &["Poly", "Mono", "Legato"];

// ES2 ids used below: 1 voices, 7 glide, 8 kb mode, 12/17/22 semis, 13/18/23 detune, 14/19/24 waves,
// 26/27 mixer, 28 drive, 31 F1 mode, 32/33 F1 cut/res, 34 F2 mode, 36/37 F2 cut/res, 30 blend,
// 41.. router slot 2 (46..50), 91/92 LFO1, 101..106 env2, 108..114 env3, 119..121 FX mod, 40 volume

fn exp_t(x: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(x.clamp(0.0, 1.0))
}

pub static ES1_PARAMS: &[Param] = &[
    Param::new(0, "Wave", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(1, "Sub Osc", SUBS, 1.0),
    Param::new(2, "Mix", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(3, "Octave", -2.0, 2.0, 0.0, Unit::None),
    Param::new(4, "Cutoff", 0.0, 1.0, 0.55, Unit::Ratio),
    Param::new(5, "Resonance", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(6, "Drive", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(7, "ADSR via Vel", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(8, "Filter Env", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(9, "Attack", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(10, "Decay", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(11, "Sustain", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(12, "Release", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(13, "LFO Wave", LFO_W, 1.0),
    Param::new(14, "LFO Rate", 0.05, 20.0, 5.0, Unit::Hz),
    Param::new(15, "LFO to Pitch (Wheel)", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(16, "Glide", 0.0, 2.0, 0.0, Unit::Seconds),
    Param::menu(17, "Chorus", CHORUS, 0.0),
    Param::new(18, "Voices", 1.0, 16.0, 8.0, Unit::None),
    Param::new(19, "Volume", -40.0, 12.0, 0.0, Unit::Db),
];

fn es1_map(p: &Params) -> Vec<(u32, f32)> {
    let w = p.get(0);
    let wave = if w < 0.25 {
        1.0
    } else if w < 0.5 {
        2.0
    } else if w < 0.75 {
        3.0
    } else {
        4.0
    };
    let sub = p.get(1).round() as i32;
    let oct = p.get(3).round() * 12.0;
    let (sub_semi, sub_wave) = match sub {
        1 => (-12.0, 3.0),
        2 => (-24.0, 3.0),
        3 => (-24.0, 4.0),
        4 => (0.0, 7.0),
        _ => (0.0, 0.0),
    };
    let mix = if sub == 0 { 0.0 } else { p.get(2) };
    vec![
        (14, wave),
        (12, oct),
        (16, 0.0),
        (21, if sub == 0 { 0.0 } else { 1.0 }),
        (24, sub_wave),
        (22, oct + sub_semi),
        (23, 0.0),
        (26, 1.0),
        (27, 1.0 - mix),
        (32, 1.0),
        (31, 0.0),
        (36, p.get(4)),
        (37, p.get(5)),
        (34, 2.0),
        (28, p.get(6)),
        (30, 1.0),
        (46, 14.0),
        (47, 4.0),
        (48, 10.0),
        (49, p.get(8) * 0.4),
        (50, p.get(8) * 0.4),
        (101, 0.002),
        (102, 0.002),
        (103, exp_t(p.get(10), 0.02, 8.0)),
        (105, p.get(11)),
        (106, exp_t(p.get(12), 0.01, 8.0)),
        (108, exp_t(p.get(9), 0.001, 5.0)),
        (109, exp_t(p.get(9), 0.001, 5.0)),
        (110, exp_t(p.get(10), 0.02, 8.0)),
        (112, p.get(11)),
        (113, exp_t(p.get(12), 0.01, 8.0)),
        (114, p.get(7)),
        (91, p.get(13)),
        (92, p.get(14)),
        (45, p.get(15) * 0.3),
        (7, p.get(16)),
        (119, if p.get(17) >= 0.5 { 0.0 } else { 3.0 }),
        (
            121,
            [0.0, 0.5, 0.9][p.get(17).round().clamp(0.0, 2.0) as usize],
        ),
        (120, if p.get(17) >= 1.5 { 0.8 } else { 0.5 }),
        (1, p.get(18)),
        (40, p.get(19) - 1.0),
    ]
}

pub static ESM_PARAMS: &[Param] = &[
    Param::new(0, "Mix", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(1, "Octave", -2.0, 1.0, 0.0, Unit::None),
    Param::new(2, "Glide", 0.0, 1.0, 0.05, Unit::Seconds),
    Param::new(3, "Cutoff", 0.0, 1.0, 0.35, Unit::Ratio),
    Param::new(4, "Resonance", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(5, "Filter Int", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(6, "Filter Decay", 0.0, 1.0, 0.35, Unit::Ratio),
    Param::new(7, "Velocity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(8, "Volume Decay", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(9, "Overdrive", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(10, "Volume", -40.0, 12.0, 0.0, Unit::Db),
];

fn esm_map(p: &Params) -> Vec<(u32, f32)> {
    let oct = p.get(1).round() * 12.0;
    vec![
        (8, 2.0),
        (7, p.get(2)),
        (14, 2.0),
        (19, 3.0),
        (12, oct),
        (17, oct),
        (18, 0.0),
        (21, 0.0),
        (26, 0.0),
        (27, 0.0),
        (16, 1.0),
        (11, 1.0),
        (26, 0.0),
        (27, 1.0 - p.get(0)),
        (32, 1.0),
        (36, p.get(3)),
        (37, p.get(4)),
        (34, 2.0),
        (30, 1.0),
        (28, p.get(9)),
        (46, 14.0),
        (47, 4.0),
        (48, 10.0),
        (49, p.get(5) * 0.3),
        (50, p.get(5) * p.get(7) * 0.5),
        (101, 0.001),
        (102, 0.001),
        (103, exp_t(p.get(6), 0.03, 4.0)),
        (105, 0.0),
        (106, 0.05),
        (108, 0.001),
        (109, 0.001),
        (110, exp_t(p.get(8), 0.1, 20.0)),
        (112, 0.0),
        (113, 0.04),
        (114, p.get(7)),
        (40, p.get(10) + 6.0),
    ]
}

pub static ESP_PARAMS: &[Param] = &[
    Param::new(0, "Triangle", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(1, "Sawtooth", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(2, "Rectangle", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(3, "Sub -1", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(4, "Sub -2", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(5, "Noise", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(6, "Cutoff", 0.0, 1.0, 0.55, Unit::Ratio),
    Param::new(7, "Resonance", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(8, "Filter ADSR Int", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(9, "Attack", 0.0, 1.0, 0.05, Unit::Ratio),
    Param::new(10, "Decay", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(11, "Sustain", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(12, "Release", 0.0, 1.0, 0.35, Unit::Ratio),
    Param::menu(13, "Chorus", CHORUS, 1.0),
    Param::new(14, "Overdrive", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(15, "Volume", -40.0, 12.0, 0.0, Unit::Db),
];

fn esp_map(p: &Params) -> Vec<(u32, f32)> {
    // osc 1 = main (dominant of tri/saw/rect), osc 2 = second shape, osc 3 = sub octaves / noise
    let lv = [p.get(0), p.get(1), p.get(2)];
    let mut order = [(lv[0], 1.0), (lv[1], 2.0), (lv[2], 3.0)];
    order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let sub = p.get(3).max(p.get(4)).max(p.get(5));
    let sub_wave = if p.get(5) > p.get(3).max(p.get(4)) {
        7.0
    } else {
        3.0
    };
    let total = (order[0].0 + order[1].0 + sub).max(1e-3);
    vec![
        (14, order[0].1),
        (19, order[1].1),
        (24, sub_wave),
        (22, if p.get(4) > p.get(3) { -24.0 } else { -12.0 }),
        (23, 0.0),
        (18, 3.0),
        (187, order[0].0 / total * 2.0),
        (188, order[1].0 / total * 2.0),
        (189, sub / total * 2.0),
        (26, 0.5),
        (27, 0.34),
        (32, 1.0),
        (36, p.get(6)),
        (37, p.get(7)),
        (34, 2.0),
        (30, 1.0),
        (28, p.get(14)),
        (46, 14.0),
        (47, 4.0),
        (48, 0.0),
        (49, p.get(8) * 0.4),
        (101, exp_t(p.get(9), 0.001, 5.0)),
        (102, exp_t(p.get(9), 0.001, 5.0)),
        (103, exp_t(p.get(10), 0.02, 8.0)),
        (105, p.get(11)),
        (106, exp_t(p.get(12), 0.01, 8.0)),
        (108, exp_t(p.get(9), 0.001, 5.0)),
        (109, exp_t(p.get(9), 0.001, 5.0)),
        (110, exp_t(p.get(10), 0.02, 8.0)),
        (112, p.get(11)),
        (113, exp_t(p.get(12), 0.01, 8.0)),
        (119, if p.get(13) >= 0.5 { 0.0 } else { 3.0 }),
        (
            121,
            [0.0, 0.5, 0.9][p.get(13).round().clamp(0.0, 2.0) as usize],
        ),
        (40, p.get(15) + 11.0),
    ]
}

pub static ESE_PARAMS: &[Param] = &[
    Param::new(0, "Wave", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(1, "Octave", -1.0, 1.0, 0.0, Unit::None),
    Param::new(2, "Cutoff", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(3, "Resonance", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(4, "Attack", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(5, "Release", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(6, "Velocity", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::menu(7, "Chorus", CHORUS, 2.0),
    Param::new(8, "Volume", -40.0, 12.0, 0.0, Unit::Db),
];

fn ese_map(p: &Params) -> Vec<(u32, f32)> {
    let oct = p.get(1).round() * 12.0;
    let pwm = p.get(0) > 0.5;
    vec![
        (14, if pwm { 4.0 } else { 2.0 }),
        (19, 2.0),
        (12, oct),
        (17, oct),
        (18, 9.0),
        (21, 0.0),
        (26, 0.5),
        (27, 0.5),
        (9, 1.0),
        (32, 1.0),
        (36, p.get(2)),
        (37, p.get(3)),
        (34, 0.0),
        (30, 1.0),
        (108, exp_t(p.get(4), 0.005, 4.0)),
        (109, exp_t(p.get(4), 0.005, 4.0)),
        (110, 1.0),
        (112, 1.0),
        (113, exp_t(p.get(5), 0.05, 8.0)),
        (114, p.get(6)),
        (46, 7.0),
        (47, 2.0),
        (48, 0.0),
        (49, if pwm { 0.3 } else { 0.0 }),
        (95, 0.8),
        (119, if p.get(7) >= 0.5 { 0.0 } else { 3.0 }),
        (
            121,
            [0.0, 0.6, 1.0][p.get(7).round().clamp(0.0, 2.0) as usize],
        ),
        (120, 0.6),
        (40, p.get(8) + 7.0),
    ]
}

pub static EFM1_PARAMS: &[Param] = &[
    Param::new(0, "Modulator Harmonic", 1.0, 16.0, 2.0, Unit::None),
    Param::new(1, "Modulator Fine", -0.5, 0.5, 0.0, Unit::Ratio),
    Param::new(2, "Carrier Harmonic", 1.0, 8.0, 1.0, Unit::None),
    Param::new(3, "FM Depth", 0.0, 1.0, 0.35, Unit::Ratio),
    Param::new(4, "FM Env Amount", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(5, "FM Decay", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(6, "Sub Osc Level", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(7, "Stereo Detune", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(8, "Attack", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(9, "Decay", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(10, "Sustain", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(11, "Release", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(12, "Velocity", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(13, "Volume", -40.0, 12.0, 0.0, Unit::Db),
];

fn efm1_map(p: &Params) -> Vec<(u32, f32)> {
    // Retro Synth FM engine: osc 1 waveform = carrier ratio − 1, osc 2 waveform = modulator ratio − 1,
    // mix = FM index, negative shape-mod = FM depth follows the filter envelope.
    vec![
        (19, 3.0),
        (28, (p.get(2) - 1.0).round().clamp(0.0, 3.0)),
        (30, (p.get(0) - 1.0).round().clamp(0.0, 3.0)),
        (
            32,
            if p.get(0) > 4.0 {
                12.0 * (p.get(0) / 4.0).log2()
            } else {
                0.0
            },
        ),
        (33, p.get(1) * 100.0),
        (26, p.get(3)),
        (25, -p.get(4)),
        (56, 0.001),
        (57, exp_t(p.get(5), 0.05, 6.0)),
        (58, 0.0),
        (59, 0.3),
        (27, p.get(6) * 0.5),
        (1, if p.get(7) > 0.05 { 2.0 } else { 1.0 }),
        (9, p.get(7) * 0.3),
        (8, 1.0),
        (34, 0.0),
        (42, 0.0),
        (61, exp_t(p.get(8), 0.001, 5.0)),
        (62, exp_t(p.get(9), 0.05, 10.0)),
        (63, p.get(10)),
        (64, exp_t(p.get(11), 0.01, 8.0)),
        (65, p.get(12)),
        (4, p.get(13) + 2.5),
    ]
}

pub fn es1(rate: f32) -> Facade {
    Facade::new(Box::new(crate::es2::Es2::new(rate)), ES1_PARAMS, es1_map)
}
pub fn es_m(rate: f32) -> Facade {
    Facade::new(Box::new(crate::es2::Es2::new(rate)), ESM_PARAMS, esm_map)
}
pub fn es_p(rate: f32) -> Facade {
    Facade::new(Box::new(crate::es2::Es2::new(rate)), ESP_PARAMS, esp_map)
}
pub fn es_e(rate: f32) -> Facade {
    Facade::new(Box::new(crate::es2::Es2::new(rate)), ESE_PARAMS, ese_map)
}
pub fn efm1(rate: f32) -> Facade {
    Facade::new(
        Box::new(crate::retro::RetroSynth::new(rate)),
        EFM1_PARAMS,
        efm1_map,
    )
}

#[allow(dead_code)]
const _UNUSED: &[&str] = MONO;
