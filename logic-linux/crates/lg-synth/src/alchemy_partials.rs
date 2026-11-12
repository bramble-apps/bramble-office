//! Alchemy's additive and spectral elements as Studio's own partial engine: each block a
//! source's partials (frequency, level, pan) go through PVar, the additive element's three effect
//! units (1 amplitude: None / Harmonic / Pulse/Saw / Saw+Noise; 2 pitch: Beating / Stretch / Shift /
//! Magnet / Pitch Noise; 3 output: Auto Pan / Comb / EQ / Filter / Spread / Strum / Ripples), the
//! synthesized formant filter (four units morphed by Select: combs, metal combs, classic and
//! parallel low / band / high pass, peak, notch, vowels) which scales partials rather than
//! filtering audio, then — for the spectral element — Pitch / Noise filling, Low / High Cut and two
//! spectral effects (Bloom, Blur, Cloud, Freq Shift, Freq Stretch, Gate, Glide, Metallize,
//! Shimmer, Shuffle, Smear) with Mix. The voice then plays the partials as sines (Additive Sine) or
//! a chosen shape (Complex), with Sym.

use lg_dsp::{Param, Unit};

pub const NP: usize = 96;
/// Per source s: ADD + s × 64 + field.
pub const ADD: u32 = 14700;
pub const A_COMPLEX: u32 = 0;
pub const A_SHAPE: u32 = 1;
pub const A_VOL: u32 = 2;
pub const A_NUM: u32 = 3;
pub const A_PVAR: u32 = 4;
pub const A_SYM: u32 = 5;
/// Additive effect units 1–3: +0 on, +1 type, +2..+5 four knobs.
pub const U1: u32 = 8;
pub const U2: u32 = 16;
pub const U3: u32 = 24;
pub const SP_MODE: u32 = 32;
pub const SP_VOL: u32 = 33;
pub const SP_LO: u32 = 34;
pub const SP_HI: u32 = 35;
/// Spectral effects 1–2: +0 on, +1 type, +2 mix, +3..+5 three knobs.
pub const SFX1: u32 = 36;
pub const SFX2: u32 = 42;
pub const FM_ON: u32 = 48;
pub const FM_SHIFT: u32 = 49;
pub const FM_SIZE: u32 = 50;
pub const FM_CENTER: u32 = 51;
pub const FM_SELECT: u32 = 52;
pub const FM_TYPE: u32 = 53;

pub fn id(s: usize, f: u32) -> u32 {
    ADD + s as u32 * 64 + f
}

pub const SHAPES: &[&str] = &["Sine", "Triangle", "Square", "Saw", "Pulse 25%", "Half Sine"];
pub const U1_TYPES: &[&str] = &["None", "Harmonic", "Pulse/Saw", "Saw+Noise"];
pub const U2_TYPES: &[&str] = &["None", "Beating", "Stretch", "Shift", "Magnet", "Pitch Noise"];
pub const U3_TYPES: &[&str] = &["None", "Auto Pan", "Comb", "EQ", "Filter", "Spread", "Strum", "Ripples"];
pub const SPEC_MODES: &[&str] = &["Pitch", "Noise"];
pub const SPEC_FX: &[&str] = &["None", "Bloom", "Blur", "Cloud", "Freq Shift", "Freq Stretch", "Gate", "Glide", "Metallize", "Shimmer", "Shuffle", "Smear"];
pub const FORMANT_TYPES: &[&str] = &[
    "Off", "Comb Neg", "Comb Pos", "Brass", "Iron", "Steel", "Tin", "Low Pass Classic", "Low Pass Parallel", "Band Pass Classic", "Band Pass Parallel", "High Pass Classic", "High Pass Parallel", "Peak", "Notch",
    "Vowel A Bright", "Vowel E Bright", "Vowel I Bright", "Vowel O Bright", "Vowel U Bright", "Vowel A Classic", "Vowel E Classic", "Vowel I Classic", "Vowel O Classic", "Vowel U Classic", "Vowel A Smooth", "Vowel E Smooth", "Vowel I Smooth", "Vowel O Smooth", "Vowel U Smooth",
];

/// The four knobs' names of an additive effect unit's type.
pub fn unit_knobs(unit: usize, ty: usize) -> [&'static str; 4] {
    match (unit, ty) {
        (0, 1) => ["Fundamental", "Octaves", "Odd/Even", "Fifths"],
        (0, 2) => ["Pulse/Saw", "Sync", "Odd/Even", "Tone"],
        (0, 3) => ["Noise", "Rate", "Smooth", "Tone"],
        (1, 1) => ["Amount", "Partial", "Tuned", ""],
        (1, 2) => ["Amount", "String", "", ""],
        (1, 3) => ["Pitch", "Frequency", "", ""],
        (1, 4) => ["Amount", "Pitch", "", ""],
        (1, 5) => ["Amount", "Rate", "Smooth", "Min. Partial"],
        (2, 1) | (2, 5) => ["Amount", "Rate", "Ramp", "Cycles"],
        (2, 2) => ["Amount", "Damp", "Frequency", "Feedback"],
        (2, 3) => ["Low", "Low Mid", "High Mid", "High"],
        (2, 4) => ["Amount", "LP - HP", "Cutoff", "Resonance"],
        (2, 6) => ["Partial", "Release", "Width", "Amount"],
        (2, 7) => ["Group", "Period", "Phase", "Amount"],
        _ => ["", "", "", ""],
    }
}

/// A unit type's knob values when it's chosen (neutral where that means anything).
pub fn unit_defaults(unit: usize, ty: usize) -> [f32; 4] {
    match (unit, ty) {
        (0, 1) => [0.5, 0.5, 0.5, 0.0],
        (0, 2) => [1.0, 0.0, 0.5, 0.0],
        (0, 3) => [0.3, 0.3, 0.5, 0.0],
        (1, 1) => [0.2, 0.0, 0.0, 0.0],
        (1, 2) | (1, 3) | (1, 4) => [0.5, 0.0, 0.5, 0.0],
        (1, 5) => [0.2, 0.3, 0.5, 0.0],
        (2, 3) => [0.5, 0.5, 0.5, 0.5],
        (2, 4) => [1.0, 0.0, 0.7, 0.2],
        (2, 6) => [0.1, 0.4, 0.2, 0.8],
        (2, 7) => [0.0, 0.3, 0.0, 0.5],
        _ => [0.5, 0.3, 0.3, 0.3],
    }
}

/// A spectral effect's three knob names.
pub fn spec_knobs(ty: usize) -> [&'static str; 3] {
    match SPEC_FX.get(ty).copied().unwrap_or("") {
        "Bloom" => ["Time", "Spread", "Decay"],
        "Blur" => ["Length", "Variance", "Gate"],
        "Cloud" => ["Threshold", "Attack", "Simplify"],
        "Freq Shift" => ["Shift", "Feedback", "LP Cutoff"],
        "Freq Stretch" => ["Alpha", "Beta", "Variance"],
        "Gate" => ["Threshold", "Attack", "Decay"],
        "Glide" => ["Frequency", "Width", "Shift"],
        "Metallize" => ["Interval", "Simplify", "Feedback"],
        "Shimmer" => ["Rate", "Shimmer", "Feedback"],
        "Shuffle" => ["Factor", "Range", "Bins"],
        "Smear" => ["Smooth", "Bright", "Phase"],
        _ => ["", "", ""],
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn params() -> Vec<Param> {
    let mut v = Vec::new();
    for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
        v.push(Param::new(id(s, A_COMPLEX), leak(format!("Additive Complex {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::menu(id(s, A_SHAPE), leak(format!("Additive Shape {l}")), SHAPES, 0.0));
        v.push(Param::new(id(s, A_VOL), leak(format!("Additive Volume {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(id(s, A_NUM), leak(format!("Num Partials {l}")), 1.0, 600.0, 64.0, Unit::None));
        v.push(Param::new(id(s, A_PVAR), leak(format!("PVar {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(id(s, A_SYM), leak(format!("Sym {l}")), 0.0, 1.0, 0.5, Unit::Ratio));
        for (u, (b, types)) in [(U1, U1_TYPES), (U2, U2_TYPES), (U3, U3_TYPES)].iter().enumerate() {
            v.push(Param::new(id(s, b + 0), leak(format!("Add FX {} On {l}", u + 1)), 0.0, 1.0, 0.0, Unit::OnOff));
            v.push(Param::menu(id(s, b + 1), leak(format!("Add FX {} Type {l}", u + 1)), types, if u == 0 { 1.0 } else { 0.0 }));
            let d = unit_defaults(u, if u == 0 { 1 } else { 0 });
            for k in 0..4u32 {
                v.push(Param::new(id(s, b + 2 + k), leak(format!("Add FX {} Knob {} {l}", u + 1, k + 1)), 0.0, 1.0, d[k as usize], Unit::Ratio));
            }
        }
        v.push(Param::menu(id(s, SP_MODE), leak(format!("Spectral Mode {l}")), SPEC_MODES, 0.0));
        v.push(Param::new(id(s, SP_VOL), leak(format!("Spectral Volume {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(id(s, SP_LO), leak(format!("Spectral Low Cut {l}")), 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(id(s, SP_HI), leak(format!("Spectral High Cut {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        for (u, b) in [SFX1, SFX2].iter().enumerate() {
            v.push(Param::new(id(s, b + 0), leak(format!("Spec FX {} On {l}", u + 1)), 0.0, 1.0, 0.0, Unit::OnOff));
            v.push(Param::menu(id(s, b + 1), leak(format!("Spec FX {} Type {l}", u + 1)), SPEC_FX, 0.0));
            v.push(Param::new(id(s, b + 2), leak(format!("Spec FX {} Mix {l}", u + 1)), 0.0, 1.0, 1.0, Unit::Ratio));
            for k in 0..3u32 {
                v.push(Param::new(id(s, b + 3 + k), leak(format!("Spec FX {} Knob {} {l}", u + 1, k + 1)), 0.0, 1.0, 0.5, Unit::Ratio));
            }
        }
        v.push(Param::new(id(s, FM_ON), leak(format!("Formant On {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(id(s, FM_SHIFT), leak(format!("Formant Shift {l}")), -24.0, 24.0, 0.0, Unit::Semitones));
        v.push(Param::new(id(s, FM_SIZE), leak(format!("Formant Size {l}")), 0.25, 4.0, 1.0, Unit::None));
        v.push(Param::new(id(s, FM_CENTER), leak(format!("Formant Center {l}")), 0.0, 1.0, 0.5, Unit::Ratio));
        v.push(Param::new(id(s, FM_SELECT), leak(format!("Formant Select {l}")), 0.0, 3.0, 0.0, Unit::None));
        for k in 0..4u32 {
            // vowels A, E, I, O (Classic) by default
            v.push(Param::menu(id(s, FM_TYPE + k), leak(format!("Formant Unit {} {l}", k + 1)), FORMANT_TYPES, 20.0 + k as f32));
        }
    }
    v
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Unit3 {
    pub on: bool,
    pub ty: u8,
    pub k: [f32; 4],
}

#[derive(Clone, Copy, Default, Debug)]
pub struct SpecFx {
    pub on: bool,
    pub ty: u8,
    pub mix: f32,
    pub k: [f32; 3],
}

#[derive(Clone, Copy, Default, Debug)]
pub struct PartCfg {
    pub complex: bool,
    pub shape: u8,
    pub vol: f32,
    pub num: usize,
    pub pvar: f32,
    pub sym: f32,
    pub u: [Unit3; 3],
    pub noise_mode: bool,
    pub sp_vol: f32,
    pub lo: f32,
    pub hi: f32,
    pub sfx: [SpecFx; 2],
    pub fm_on: bool,
    pub fm_shift: f32,
    pub fm_size: f32,
    pub fm_center: f32,
    pub fm_select: f32,
    pub fm_types: [u8; 4],
}

impl PartCfg {
    pub fn read(p: &dyn Fn(u32) -> f32, s: usize) -> PartCfg {
        let q = |f: u32| p(id(s, f));
        let unit = |b: u32| Unit3 { on: q(b) >= 0.5, ty: q(b + 1).round().max(0.0) as u8, k: [q(b + 2), q(b + 3), q(b + 4), q(b + 5)] };
        let fx = |b: u32| SpecFx { on: q(b) >= 0.5, ty: q(b + 1).round().max(0.0) as u8, mix: q(b + 2), k: [q(b + 3), q(b + 4), q(b + 5)] };
        PartCfg {
            complex: q(A_COMPLEX) >= 0.5,
            shape: q(A_SHAPE).round().max(0.0) as u8,
            vol: q(A_VOL),
            num: q(A_NUM).round().clamp(1.0, 600.0) as usize,
            pvar: q(A_PVAR),
            sym: q(A_SYM),
            u: [unit(U1), unit(U2), unit(U3)],
            noise_mode: q(SP_MODE) >= 0.5,
            sp_vol: q(SP_VOL),
            lo: q(SP_LO),
            hi: q(SP_HI),
            sfx: [fx(SFX1), fx(SFX2)],
            fm_on: q(FM_ON) >= 0.5,
            fm_shift: q(FM_SHIFT),
            fm_size: q(FM_SIZE),
            fm_center: q(FM_CENTER),
            fm_select: q(FM_SELECT),
            fm_types: [0, 1, 2, 3].map(|k| q(FM_TYPE + k).round().max(0.0) as u8),
        }
    }
}

fn hash01(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x7F4A_7C15;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// A partial engine's running state (per source per voice).
#[derive(Clone, Copy)]
pub struct PartState {
    pub t: f32,
    pub seed: u32,
    pub anoise: [(f32, f32); NP],
    pub pnoise: [(f32, f32); NP],
    pub acc: f32,
    pub pacc: f32,
    pub hold: [[f32; NP]; 2],
    pub env: [[f32; NP]; 2],
    pub ph: [f32; 2],
    /// Smear with Phase off: the voice lines the partials' phases up.
    pub lock_phase: bool,
}

impl Default for PartState {
    fn default() -> Self {
        PartState { t: 0.0, seed: 1, anoise: [(1.0, 1.0); NP], pnoise: [(0.0, 0.0); NP], acc: 0.0, pacc: 0.0, hold: [[0.0; NP]; 2], env: [[0.0; NP]; 2], ph: [0.0; 2], lock_phase: false }
    }
}

impl PartState {
    pub fn start(&mut self, seed: u32) {
        *self = PartState { seed, ..PartState::default() };
    }
}

/// Gaussian bump in octaves.
fn bump(f: f32, center: f32, width_oct: f32) -> f32 {
    let d = (f.max(1.0) / center.max(1.0)).log2() / width_oct.max(0.01);
    (-d * d).exp()
}

/// Vowel formants (Hz) for A, E, I, O, U (common textbook values).
const VOWELS: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [350.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];

/// A synthesized formant unit's gain at f (Hz); `shift` in semitones, `size` stretch.
pub fn formant_unit(ty: usize, f: f32, shift: f32, size: f32, center_hz: f32) -> f32 {
    // Size stretches around Center: resonances below move up, above move down as it grows
    let f = center_hz * (f.max(1.0) / center_hz).powf(1.0 / size.max(0.05));
    let sh = 2f32.powf(shift / 12.0);
    let name = FORMANT_TYPES.get(ty).copied().unwrap_or("Off");
    match name {
        "Off" => 1.0,
        "Comb Neg" | "Comb Pos" => {
            let base = 220.0 * sh;
            let x = f / base;
            let c = if name == "Comb Neg" { (std::f32::consts::PI * x / 2.0).sin().abs() } else { (std::f32::consts::PI * x).cos().abs() };
            0.15 + 0.85 * c.powf(4.0)
        }
        "Brass" | "Iron" | "Steel" | "Tin" => {
            // teeth of varying thickness and spacing
            let (sp, th): (f32, f32) = match name {
                "Brass" => (1.0, 3.0),
                "Iron" => (1.37, 6.0),
                "Steel" => (1.73, 10.0),
                _ => (2.41, 16.0),
            };
            let x = (f / (180.0 * sh)).powf(1.0 / sp.max(1.0).sqrt()) * sp;
            0.1 + 0.9 * (std::f32::consts::PI * x).cos().abs().powf(th)
        }
        _ if name.starts_with("Low Pass") || name.starts_with("High Pass") || name.starts_with("Band Pass") => {
            let fc = 1000.0 * sh;
            let order = if name.ends_with("Parallel") { 4.0 } else { 2.0 };
            let r = f / fc;
            let lp = 1.0 / (1.0 + r.powf(order)).sqrt();
            let hp = 1.0 / (1.0 + (1.0 / r).powf(order)).sqrt();
            if name.starts_with("Low") {
                lp
            } else if name.starts_with("High") {
                hp
            } else {
                (lp * hp * 2.0).min(1.0)
            }
        }
        "Peak" => 0.3 + 1.5 * bump(f, 1000.0 * sh, 0.3),
        "Notch" => 1.0 - 0.95 * bump(f, 1000.0 * sh, 0.25),
        _ => {
            // vowels: three formants; Bright keeps more highs, Smooth widens and softens them
            let v = ["A", "E", "I", "O", "U"].iter().position(|x| name.starts_with(&format!("Vowel {x}"))).unwrap_or(0);
            let (w, tilt) = if name.ends_with("Bright") { (0.18, 0.0) } else if name.ends_with("Smooth") { (0.35, 0.6) } else { (0.22, 0.3) };
            let mut g = 0.02;
            for (k, &ff) in VOWELS[v].iter().enumerate() {
                g += bump(f, ff * sh, w) * (1.0 - tilt * k as f32 * 0.3);
            }
            g.min(1.5)
        }
    }
}

/// The partials for one block: from base levels and ratios (the source's content) to the
/// frequencies (Hz), levels and pans the voice plays. Returns how many partials are in use.
#[allow(clippy::too_many_arguments)]
pub fn process(cfg: &PartCfg, spectral: bool, base_amp: &[f32; NP], base_ratio: &[f32; NP], f0: f32, dt: f32, nyq: f32, st: &mut PartState, amp: &mut [f32; NP], freq: &mut [f32; NP], pan: &mut [f32; NP]) -> usize {
    st.t += dt;
    let n = cfg.num.min(NP);
    let base_sum: f32 = base_amp.iter().take(n).sum::<f32>().max(1e-6);
    for k in 0..NP {
        let h = (k + 1) as f32;
        let r = h + (base_ratio[k] - h) * cfg.pvar;
        freq[k] = f0 * r;
        amp[k] = if k < n { base_amp[k] } else { 0.0 };
        pan[k] = 0.0;
    }
    // unit 1: amplitude
    let u = cfg.u[0];
    if u.on {
        let [a, b, c, d] = u.k;
        match u.ty {
            0 => {
                for x in amp.iter_mut().take(n) {
                    *x = 1.0;
                }
            }
            1 => {
                for k in 0..n {
                    let h = k + 1;
                    let mut g = 1.0;
                    if h == 1 {
                        g *= a * 2.0;
                    } else if a > 0.5 {
                        g *= 1.0 - (a - 0.5) * 2.0;
                    }
                    if [2, 4, 8, 16].contains(&h) {
                        g *= b * 2.0;
                    }
                    if h > 1 {
                        g *= if h % 2 == 1 { (1.0 - c) * 2.0 } else { c * 2.0 };
                    }
                    if d > 0.0 {
                        g *= if [1, 3, 9, 27].contains(&h) { 1.0 + d * 2.0 } else { 1.0 - d };
                    }
                    amp[k] *= g.max(0.0);
                }
            }
            2 | 3 => {
                // a synthesized saw / pulse spectrum; Sync moves a peak up the harmonics
                for k in 0..n {
                    let h = (k + 1) as f32;
                    let saw = 1.0 / h;
                    let pulse = if (k + 1) % 2 == 1 { 1.0 / h } else { 0.0 };
                    let mut g = if u.ty == 2 { pulse + (saw - pulse) * a } else { saw };
                    if u.ty == 2 && b > 0.0 {
                        let peak = 1.0 + b * 12.0;
                        g *= 1.0 + 3.0 * (-((h - peak) / (1.0 + b * 3.0)).powi(2)).exp() * b;
                    }
                    if u.ty == 2 && k > 0 {
                        g *= if (k + 1) % 2 == 1 { (1.0 - c) * 2.0 } else { c * 2.0 };
                    }
                    if u.ty == 3 {
                        // noise on the harmonic levels at Rate, Smooth ramps between values
                        let (cur, tgt) = &mut st.anoise[k];
                        *cur += (*tgt - *cur) * if b > 0.0 { (1.0 - c * 0.97).max(0.02) } else { 1.0 };
                        g *= (1.0 - a) + a * *cur;
                    }
                    g *= h.powf(-d * 2.0);
                    amp[k] = g;
                }
                if u.ty == 3 {
                    st.acc += dt * (0.5 + b * 40.0);
                    if st.acc >= 1.0 {
                        st.acc = 0.0;
                        st.seed = st.seed.wrapping_add(1);
                        for k in 0..n {
                            st.anoise[k].1 = hash01(st.seed.wrapping_mul(131).wrapping_add(k as u32)) * 2.0;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // unit 2: pitch
    let u = cfg.u[1];
    if u.on {
        let [a, b, c, d] = u.k;
        match u.ty {
            1 => {
                let part = 2 + (b * 6.0).round() as usize;
                for k in 0..NP {
                    if (k + 1) % part == 0 {
                        let h = (k + 1) as f32;
                        let up = if c >= 0.5 { (std::f32::consts::PI * a).sin() * 0.06 } else { a / h };
                        freq[k] *= 1.0 + up;
                    }
                }
            }
            2 => {
                for k in 0..NP {
                    let h = (k + 1) as f32;
                    let s = h.powf((a - 0.5) * 0.2) * (1.0 + b * b * 0.002 * h * h).sqrt();
                    freq[k] *= s;
                }
            }
            3 => {
                let hz = (a - 0.5) * 2.0 * f0 + (b - 0.5) * 2000.0 * if b == 0.5 { 0.0 } else { 1.0 };
                for f in freq.iter_mut() {
                    *f = (*f + hz).max(0.0);
                }
            }
            4 => {
                let target = f0 * 2f32.powf(b * 4.0);
                for f in freq.iter_mut() {
                    *f += (target - *f) * a;
                }
            }
            5 => {
                st.pacc += dt * (0.5 + b * 40.0);
                if st.pacc >= 1.0 {
                    st.pacc = 0.0;
                    st.seed = st.seed.wrapping_add(7);
                    for k in 0..NP {
                        st.pnoise[k].1 = hash01(st.seed.wrapping_mul(977).wrapping_add(k as u32)) * 2.0 - 1.0;
                    }
                }
                let min = (d * 32.0) as usize;
                for k in 0..NP {
                    let (cur, tgt) = &mut st.pnoise[k];
                    *cur += (*tgt - *cur) * (1.0 - c * 0.97).max(0.02);
                    if k >= min {
                        freq[k] *= 1.0 + *cur * a * 0.06;
                    }
                }
            }
            _ => {}
        }
    }
    // the synthesized formant filter: scales partials by frequency
    if cfg.fm_on {
        let sel = cfg.fm_select.clamp(0.0, 3.0);
        let (i0, fr) = ((sel.floor() as usize).min(3), sel - sel.floor());
        let i1 = (i0 + 1).min(3);
        let center = 100.0 * 100f32.powf(cfg.fm_center);
        for k in 0..NP {
            let g0 = formant_unit(cfg.fm_types[i0] as usize, freq[k], cfg.fm_shift, cfg.fm_size, center);
            let g1 = formant_unit(cfg.fm_types[i1] as usize, freq[k], cfg.fm_shift, cfg.fm_size, center);
            amp[k] *= g0 + (g1 - g0) * fr;
        }
    }
    // unit 3: output
    let u = cfg.u[2];
    if u.on {
        let [a, b, c, d] = u.k;
        match u.ty {
            1 | 5 => {
                let rate = 0.05 + b * 8.0;
                for k in 0..NP {
                    let ramp = if (k as f32) < c * 16.0 { (k as f32 / (c * 16.0).max(1.0)).max(0.2) } else { 1.0 };
                    let ph = rate * st.t + d * 4.0 * k as f32 / NP as f32;
                    let mut p = a * ramp * (std::f32::consts::TAU * ph).sin();
                    if u.ty == 5 && k % 2 == 1 {
                        p = -p;
                    }
                    pan[k] = p;
                }
            }
            2 => {
                let fc = 16.0 * 1250f32.powf(c);
                for k in 0..NP {
                    let x = (std::f32::consts::TAU * freq[k] / fc).cos();
                    let comb = (1.0 + d * x) / (1.0 + d);
                    let damp = 1.0 / (1.0 + b * 4.0 * (freq[k] / fc).max(0.0) / 16.0);
                    amp[k] *= 1.0 - a + a * comb.max(0.0) * damp.max(0.05);
                }
            }
            3 => {
                for k in 0..NP {
                    let f = freq[k];
                    let w = |lo: f32, hi: f32| bump(f, (lo * hi).sqrt(), (hi / lo).log2() * 0.5 + 0.5);
                    let bands = [(20.0, 120.0), (120.0, 800.0), (800.0, 5000.0), (5000.0, 20000.0)];
                    let gains = [a, b, c, d];
                    let mut g = 0.0;
                    let mut ws = 0.0;
                    for (bd, gn) in bands.iter().zip(gains) {
                        let wt = w(bd.0, bd.1);
                        g += wt * lg_dsp::db_to_gain((gn - 0.5) * 36.0);
                        ws += wt;
                    }
                    amp[k] *= g / ws.max(1e-6);
                }
            }
            4 => {
                let fc = 16.0 * 1250f32.powf(c);
                for k in 0..NP {
                    let r = freq[k] / fc;
                    let lp = 1.0 / (1.0 + r.powi(4)).sqrt();
                    let hp = 1.0 / (1.0 + r.powi(-4)).sqrt();
                    let bp = (lp * hp * 2.0).min(1.0);
                    // LP - HP: low pass, band, high pass in turn
                    let shape = if b < 0.5 { lp + (bp - lp) * b * 2.0 } else { bp + (hp - bp) * (b - 0.5) * 2.0 };
                    let res = 1.0 + d * 4.0 * bump(freq[k], fc, 0.15);
                    amp[k] *= 1.0 - a + a * shape * res;
                }
            }
            6 => {
                let sel = (a * 31.0) as usize;
                let width = 1.0 + c * 12.0;
                let rel = 0.05 + b * 3.0;
                for k in 0..NP {
                    let near = (-((k as f32 - sel as f32) / width).powi(2)).exp();
                    let decay = (-st.t / rel).exp();
                    let g = (1.0 - d) + d * near * decay;
                    amp[k] *= g;
                }
            }
            7 => {
                let period = 2.0 + b * 30.0;
                for k in 0..NP {
                    let grp = (a * 2.99) as u32;
                    let take = match grp {
                        1 => k % 2 == 0,
                        2 => k % 2 == 1,
                        _ => true,
                    };
                    if take {
                        let s = 0.5 + 0.5 * (std::f32::consts::TAU * (k as f32 / period + c)).sin();
                        amp[k] *= 1.0 - d + d * s;
                    }
                }
            }
            _ => {}
        }
    }
    // the spectral element: fill mode, cuts, its two effects
    if spectral {
        let lo = 20.0 * 1000f32.powf(cfg.lo);
        let hi = 20.0 * 1000f32.powf(cfg.hi);
        for k in 0..NP {
            if freq[k] < lo || (cfg.hi < 0.999 && freq[k] > hi) {
                amp[k] = 0.0;
            }
            amp[k] *= cfg.sp_vol;
        }
        for (u, fx) in cfg.sfx.iter().enumerate() {
            if fx.on && fx.ty > 0 {
                spectral_fx(fx, u, f0, dt, st, amp, freq);
            }
        }
    } else {
        for a in amp.iter_mut() {
            *a *= cfg.vol;
        }
    }
    // a sane total (all-equal partials would be very loud)
    let sum: f32 = amp.iter().sum();
    if sum > base_sum.max(1.0) * 2.0 {
        let k = base_sum.max(1.0) * 2.0 / sum;
        for a in amp.iter_mut() {
            *a *= k;
        }
    }
    let mut used = 0;
    for k in 0..NP {
        if freq[k] >= nyq || freq[k] <= 0.0 {
            amp[k] = 0.0;
        }
        if amp[k] > 0.0 {
            used = k + 1;
        }
    }
    used
}

fn spectral_fx(fx: &SpecFx, u: usize, f0: f32, dt: f32, st: &mut PartState, amp: &mut [f32; NP], freq: &mut [f32; NP]) {
    let [a, b, c] = fx.k;
    let dry_a = *amp;
    let dry_f = *freq;
    let peak = amp.iter().fold(0.0f32, |m, &x| m.max(x)).max(1e-9);
    let t = st.t;
    st.lock_phase = false;
    match SPEC_FX.get(fx.ty as usize).copied().unwrap_or("") {
        "Bloom" => {
            // partials come in one after another after the note, low to high
            for k in 0..NP {
                let at = a * 0.5 * (k as f32 / NP as f32).powf(1.0 - b * 0.8);
                let g = if t < at { 0.0 } else { (-(t - at) / (0.1 + c * 3.0)).exp().max(0.3) };
                amp[k] *= g;
            }
        }
        "Blur" => {
            // frequencies sustained (Length), some more than others (Variance), Gate keeps the envelope
            let keep = (-dt / (0.02 + a * 3.0)).exp();
            for k in 0..NP {
                let pick = hash01(k as u32 * 31 + 7) >= b;
                let h = &mut st.hold[u][k];
                *h = if pick { (*h * keep).max(amp[k]) } else { amp[k] };
                amp[k] = *h * (1.0 - c) + amp[k].min(*h) * c;
            }
        }
        "Cloud" => {
            for k in 0..NP {
                let e = &mut st.env[u][k];
                let tgt = if amp[k] >= peak * a { 1.0 } else { 0.0 };
                let rate = dt / (0.005 + b * 2.0);
                *e += (tgt - *e) * rate.min(1.0);
                let simple = (amp[k] / peak).powf(1.0 + c * 3.0) * peak;
                amp[k] = simple * (0.4 + 0.6 * *e);
            }
        }
        "Freq Shift" => {
            let hz = (a - 0.5) * 2000.0;
            let cut = 20.0 * 1000f32.powf(c);
            for k in 0..NP {
                freq[k] = (freq[k] + hz).max(0.0);
                amp[k] *= 1.0 + b * 1.5 * (k % 3 == 0) as u8 as f32;
                if freq[k] > cut {
                    amp[k] *= (cut / freq[k]).powi(2);
                }
            }
        }
        "Freq Stretch" => {
            let alpha = 0.5 + a;
            let beta = 0.5 + b;
            for k in 0..NP {
                let r = freq[k] / f0.max(1.0);
                let v = 1.0 + (hash01(k as u32 * 17 + 3) - 0.5) * c * 0.1;
                freq[k] = f0 * alpha * r.powf(beta) * v;
            }
        }
        "Gate" => {
            for k in 0..NP {
                let e = &mut st.env[u][k];
                let open = amp[k] >= peak * a * a;
                let tau = if open { 0.002 + b * 1.0 } else { 0.005 + c * 2.0 };
                *e += ((open as u8 as f32) - *e) * (dt / tau).min(1.0);
                amp[k] *= *e;
            }
        }
        "Glide" => {
            st.ph[u] = (st.ph[u] + dt * 0.8).fract();
            let start = 100.0 * 50f32.powf(a);
            let center = start * 2f32.powf(c * 4.0 * st.ph[u]);
            for k in 0..NP {
                amp[k] *= 0.1 + bump(freq[k], center, 0.1 + b * 1.5);
            }
        }
        "Metallize" => {
            let interval = 50.0 + a * 1000.0;
            for k in 0..NP {
                let comb = 0.5 + 0.5 * (std::f32::consts::TAU * freq[k] / interval).cos();
                amp[k] *= (1.0 - c) + c * comb.powf(1.0 + c * 6.0) * 2.0;
                if amp[k] < peak * b * 0.3 {
                    amp[k] = 0.0;
                }
            }
        }
        "Shimmer" => {
            st.ph[u] += dt * (0.1 + a * 8.0);
            for k in 0..NP {
                freq[k] *= 1.0 + b * 0.03 * (std::f32::consts::TAU * (st.ph[u] + k as f32 * 0.37)).sin();
                freq[k] *= 1.0 + c * 0.0015 * k as f32;
            }
        }
        "Shuffle" => {
            let bins = 1 + (c * 7.0) as usize;
            let blocks = NP / bins;
            let range = 1 + (b * 8.0) as usize;
            let src = *amp;
            for blk in 0..blocks {
                if hash01(blk as u32 * 13 + 1) >= a {
                    continue;
                }
                let to = (blk + 1 + (hash01(blk as u32 * 7 + 5) * range as f32) as usize).min(blocks - 1);
                for j in 0..bins {
                    amp[blk * bins + j] = src[to * bins + j];
                    amp[to * bins + j] = src[blk * bins + j];
                }
            }
        }
        "Smear" => {
            let w = 1 + (a * 8.0) as usize;
            let src = *amp;
            for k in 0..NP {
                let lo = k.saturating_sub(w);
                let hi = (k + w).min(NP - 1);
                let avg: f32 = src[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32;
                amp[k] = avg * (1.0 + (b - 0.5) * 2.0 * k as f32 / NP as f32).max(0.0);
            }
            st.lock_phase = c < 0.5;
        }
        _ => {}
    }
    // Mix: dry against processed
    for k in 0..NP {
        amp[k] = dry_a[k] + (amp[k] - dry_a[k]) * fx.mix;
        freq[k] = dry_f[k] + (freq[k] - dry_f[k]) * fx.mix;
    }
}

/// The Complex element's shape for a partial at phase p (0..1), with Sym bending it.
#[inline]
pub fn shape_at(shape: u8, p: f32, sym: f32) -> f32 {
    // Sym: phase distortion, the same either way it is turned
    let d = (sym - 0.5).abs() * 1.8;
    let p = if d > 0.0 {
        let k = 0.5 - d * 0.45;
        if p < k { p * 0.5 / k } else { 0.5 + (p - k) * 0.5 / (1.0 - k) }
    } else {
        p
    };
    match shape {
        1 => 1.0 - 4.0 * (p - 0.5).abs(),
        2 => if p < 0.5 { 0.8 } else { -0.8 },
        3 => 1.0 - 2.0 * p,
        4 => if p < 0.25 { 0.8 } else { -0.27 },
        5 => crate::dsp::sin_cycles(p * 0.5) * 2.0 - 1.27,
        _ => crate::dsp::sin_cycles(p),
    }
}
