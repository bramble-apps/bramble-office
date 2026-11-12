//! Sculpture's processing blocks: the LFO waves, the waveshaper, the stereo delay with its
//! groove pad, the body EQ models and the limiters. All allocation happens in `new`.

use crate::dsp::{self, Biquad, Delay, OnePole, Rng};
use std::f32::consts::PI;

/// Note values for tempo-synced rates and times: (name, length in beats).
pub const NOTE_VALUES: &[(&str, f32)] = &[
    ("1/64", 0.0625),
    ("1/64d", 0.09375),
    ("1/32t", 0.0833333),
    ("1/32", 0.125),
    ("1/32d", 0.1875),
    ("1/16t", 0.1666667),
    ("1/16", 0.25),
    ("1/16d", 0.375),
    ("1/8t", 0.3333333),
    ("1/8", 0.5),
    ("1/8d", 0.75),
    ("1/4t", 0.6666667),
    ("1/4", 1.0),
    ("1/4d", 1.5),
    ("1/2t", 1.3333333),
    ("1/2", 2.0),
    ("1/2d", 3.0),
    ("1 bar", 4.0),
    ("1 bar d", 6.0),
    ("2 bars", 8.0),
    ("3 bars", 12.0),
    ("4 bars", 16.0),
    ("6 bars", 24.0),
    ("8 bars", 32.0),
    ("12 bars", 48.0),
    ("16 bars", 64.0),
    ("24 bars", 96.0),
    ("32 bars", 128.0),
];

/// The index of the note value nearest `beats`.
pub fn nearest_note(beats: f32) -> usize {
    let mut best = 0;
    let mut bd = f32::MAX;
    for (i, (_, b)) in NOTE_VALUES.iter().enumerate() {
        let d = (b.ln() - beats.max(1e-3).ln()).abs();
        if d < bd {
            bd = d;
            best = i;
        }
    }
    best
}

pub const LFO_WAVES: &[&str] = &[
    "Sine",
    "Triangle",
    "Sawtooth",
    "Sawtooth Inv.",
    "Rect Unipolar",
    "Rect Bipolar",
    "S & H",
    "S & H Lag",
    "S & H from CtrlA",
    "Filtered Noise",
];

/// One LFO's running state (per voice, or global for monophonic modulation).
#[derive(Clone, Copy, Debug)]
pub struct LfoState {
    pub phase: f32,
    held: f32,
    lag: f32,
    noise: f32,
    last_trig: f32,
    pub rng: Rng,
}

impl LfoState {
    pub fn new(seed: u32) -> LfoState {
        LfoState { phase: 0.0, held: 0.0, lag: 0.0, noise: 0.0, last_trig: 0.0, rng: Rng::new(seed) }
    }
    /// Advances by `dt_cycles` and returns the wave (-1..1; Rect Unipolar is 0..1 scaled to
    /// -1..1 only by the caller's choice: it stays 0/1).
    pub fn step(&mut self, wave: usize, curve: f32, dt_cycles: f32, ctrl_a: f32) -> f32 {
        let old = self.phase;
        self.phase += dt_cycles;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= self.phase.floor();
        }
        let p = self.phase;
        let raw = match wave {
            0 => dsp::sin_cycles(p),
            1 => {
                if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                }
            }
            2 => 2.0 * p - 1.0,
            3 => 1.0 - 2.0 * p,
            4 => {
                if p < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            5 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            6 | 7 => {
                if wrapped || old > self.phase {
                    self.held = self.rng.bipolar();
                }
                if wave == 7 {
                    self.lag += (self.held - self.lag) * (dt_cycles * 6.0).min(1.0);
                    self.lag
                } else {
                    self.held
                }
            }
            8 => {
                // a new random value whenever Ctrl A moves up through its half way point
                if ctrl_a > 0.5 && self.last_trig <= 0.5 {
                    self.held = self.rng.bipolar();
                }
                self.last_trig = ctrl_a;
                self.held
            }
            _ => {
                // filtered noise: chaotic, a little different each time
                self.noise += (self.rng.bipolar() - self.noise) * (dt_cycles * 12.0).clamp(0.0, 1.0);
                (self.noise * 3.0).clamp(-1.0, 1.0)
            }
        };
        shape_curve(raw, curve, wave == 4)
    }
}

/// The Curve knob: above 0 the wave bends toward a rectangle, below 0 the zero crossing is
/// flattened into short soft pulses.
pub fn shape_curve(x: f32, curve: f32, unipolar: bool) -> f32 {
    if curve == 0.0 {
        return x;
    }
    let (b, off) = if unipolar { (x * 2.0 - 1.0, true) } else { (x, false) };
    let y = if curve > 0.0 {
        let k = 1.0 + 9.0 * curve;
        (b * k).tanh() / k.tanh()
    } else {
        let k = 1.0 + 3.0 * (-curve);
        b.signum() * b.abs().powf(k)
    };
    if off {
        y * 0.5 + 0.5
    } else {
        y
    }
}

pub const WS_TYPES: &[&str] = &["VariDrive", "SoftSat", "Tube Dist.", "Scream"];

#[inline]
fn ws_curve(ty: usize, x: f32) -> f32 {
    match ty {
        0 => dsp::fast_tanh(x),
        1 => {
            let c = x.clamp(-1.5, 1.5);
            c - c * c * c / 6.75
        }
        2 => {
            if x >= 0.0 {
                dsp::fast_tanh(x * 1.2)
            } else {
                dsp::fast_tanh(x * 0.7) * 1.15
            }
        }
        _ => {
            // fold-back: the hot, screaming kind
            let y = dsp::fast_tanh(x * 2.0);
            y + 0.35 * (x * 3.0).sin() * y.abs()
        }
    }
}

/// The waveshaper: input scale (-1..1 is -24..+24 dB), a type-dependent variation, level
/// compensated so the sound only changes in character.
#[inline]
pub fn waveshape(ty: usize, input: f32, var: f32, x: f32) -> f32 {
    let g = (input * 4.0 * std::f32::consts::LN_2).exp();
    // compensation: a signal of 0.2 keeps its level whatever the input scale
    let comp = (ws_curve(ty, g * 0.2).abs() / 0.2).max(1e-3);
    if ty == 0 {
        let shaped = ws_curve(0, x * g) / comp;
        if var < 0.0 {
            shaped + (-var) * x
        } else {
            shaped - var * x
        }
    } else {
        let bias = var * 0.7;
        (ws_curve(ty, x * g + bias) - ws_curve(ty, bias)) / comp
    }
}

// ---------------------------------------------------------------------------------------------
// Delay

#[derive(Clone)]
pub struct StereoDelay {
    l: Delay,
    r: Delay,
    hp: [OnePole; 2],
    lp: [OnePole; 2],
    ohp: [OnePole; 2],
    olp: [OnePole; 2],
    rate: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct DelaySet {
    pub on: bool,
    pub time_l: f32,
    pub time_r: f32,
    pub fb: f32,
    pub xfeed: f32,
    pub wet: f32,
    pub lo: f32,
    pub hi: f32,
    pub inbal: f32,
    pub width: f32,
}

impl StereoDelay {
    pub fn new(rate: f32) -> StereoDelay {
        let n = (rate * 5.0) as usize;
        StereoDelay {
            l: Delay::new(n),
            r: Delay::new(n),
            hp: [OnePole::default(); 2],
            lp: [OnePole::default(); 2],
            ohp: [OnePole::default(); 2],
            olp: [OnePole::default(); 2],
            rate,
        }
    }
    pub fn reset(&mut self) {
        self.l.clear();
        self.r.clear();
        self.hp = [OnePole::default(); 2];
        self.lp = [OnePole::default(); 2];
        self.ohp = [OnePole::default(); 2];
        self.olp = [OnePole::default(); 2];
    }
    pub fn set_filters(&mut self, lo: f32, hi: f32) {
        for c in 0..2 {
            self.hp[c].lowpass_hz(lo, self.rate);
            self.ohp[c].lowpass_hz(lo, self.rate);
            self.lp[c].lowpass_hz(hi, self.rate);
            self.olp[c].lowpass_hz(hi, self.rate);
        }
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], s: &DelaySet) {
        if !s.on {
            return;
        }
        let max = (self.l.len() - 8) as f32;
        let dl = (s.time_l * self.rate).clamp(2.0, max);
        let dr = (s.time_r * self.rate).clamp(2.0, max);
        let th = (s.inbal + 1.0) * 0.25 * PI;
        let (gl, gr) = (th.cos() * std::f32::consts::SQRT_2, th.sin() * std::f32::consts::SQRT_2);
        let x = s.xfeed;
        for i in 0..l.len().min(r.len()) {
            let ol = self.l.read_cubic(dl);
            let or = self.r.read_cubic(dr);
            // feedback path: crossfeed, then low cut and high cut
            let fl = (ol * (1.0 - x.abs()) + or * x) * s.fb;
            let fr = (or * (1.0 - x.abs()) + ol * x) * s.fb;
            let fl = self.lp[0].lp(self.hp[0].hp(fl));
            let fr = self.lp[1].lp(self.hp[1].hp(fr));
            self.l.push(dsp::soft_clip(l[i] * gl + fl));
            self.r.push(dsp::soft_clip(r[i] * gr + fr));
            let wl = self.olp[0].lp(self.ohp[0].hp(ol));
            let wr = self.olp[1].lp(self.ohp[1].hp(or));
            let m = (wl + wr) * 0.5;
            let sd = (wl - wr) * 0.5 * s.width;
            l[i] += (m + sd) * s.wet;
            r[i] += (m - sd) * s.wet;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Body EQ

pub const BODY_MODELS: &[&str] = &[
    "Lo Mid Hi",
    "Guitar 1",
    "Guitar 2",
    "Guitar 3",
    "Violin 1",
    "Violin 2",
    "Cello",
    "Upright Bass",
    "Harp",
    "Wood Box",
    "Metal Plate",
];

/// Formants (Hz, dB, Q) of each model, written for this app.
fn formants(model: usize) -> &'static [(f32, f32, f32)] {
    match model {
        1 => &[(98.0, 7.0, 4.0), (205.0, 6.0, 5.0), (410.0, -3.0, 3.0), (1250.0, 3.0, 2.5), (2600.0, 4.0, 2.0), (4800.0, -4.0, 1.5)],
        2 => &[(110.0, 6.0, 5.0), (230.0, 5.0, 4.0), (520.0, -4.0, 3.0), (1500.0, 4.0, 2.0), (3100.0, 5.0, 2.5), (6000.0, -3.0, 1.5)],
        3 => &[(85.0, 8.0, 3.0), (190.0, 4.0, 4.0), (700.0, -5.0, 2.0), (1800.0, 3.0, 2.0), (3600.0, 3.0, 2.0), (7000.0, -2.0, 1.2)],
        4 => &[(290.0, 7.0, 6.0), (440.0, 5.0, 5.0), (700.0, -4.0, 3.0), (1100.0, 3.0, 3.0), (2800.0, 6.0, 3.0), (3600.0, 4.0, 3.0), (6200.0, -3.0, 1.5)],
        5 => &[(270.0, 6.0, 5.0), (470.0, 6.0, 5.0), (820.0, -5.0, 3.0), (1300.0, 4.0, 3.0), (2500.0, 5.0, 3.0), (4100.0, 3.0, 2.0)],
        6 => &[(95.0, 7.0, 5.0), (190.0, 6.0, 4.0), (350.0, -4.0, 3.0), (780.0, 4.0, 3.0), (1500.0, 4.0, 2.5), (3000.0, 3.0, 2.0)],
        7 => &[(58.0, 8.0, 5.0), (105.0, 6.0, 4.0), (230.0, -3.0, 3.0), (520.0, 3.0, 2.5), (1100.0, 3.0, 2.0)],
        8 => &[(180.0, 5.0, 3.0), (420.0, 4.0, 3.0), (900.0, -3.0, 2.5), (2000.0, 4.0, 2.0), (4200.0, 3.0, 2.0)],
        9 => &[(150.0, 6.0, 4.0), (330.0, 5.0, 4.0), (680.0, -4.0, 3.0), (1700.0, 5.0, 3.0), (3900.0, -2.0, 2.0)],
        10 => &[(420.0, 6.0, 8.0), (1100.0, 6.0, 8.0), (2300.0, 6.0, 8.0), (4400.0, 5.0, 8.0), (7600.0, 4.0, 8.0)],
        _ => &[],
    }
}

pub const FINE_BANDS: usize = 12;
const MAX_FORMANTS: usize = 8;

#[derive(Clone)]
pub struct BodyEq {
    basic: [[Biquad; 3]; 2],
    form: [[Biquad; MAX_FORMANTS]; 2],
    fine: [[Biquad; FINE_BANDS]; 2],
    n_form: usize,
    n_fine: usize,
    basic_on: bool,
    pub on: bool,
    rate: f32,
}

impl BodyEq {
    pub fn new(rate: f32) -> BodyEq {
        BodyEq {
            basic: [[Biquad::default(); 3]; 2],
            form: [[Biquad::default(); MAX_FORMANTS]; 2],
            fine: [[Biquad::default(); FINE_BANDS]; 2],
            n_form: 0,
            n_fine: 0,
            basic_on: true,
            on: false,
            rate,
        }
    }
    pub fn reset(&mut self) {
        for c in 0..2 {
            for b in &mut self.basic[c] {
                b.reset();
            }
            for b in &mut self.form[c] {
                b.reset();
            }
            for b in &mut self.fine[c] {
                b.reset();
            }
        }
    }
    /// p1..p4: Low/Intensity, Mid/Shift, High/Stretch (all -1..1) and Mid frequency/Fine
    /// structure (0..1).
    pub fn set(&mut self, on: bool, model: usize, p1: f32, p2: f32, p3: f32, p4: f32) {
        self.on = on;
        let rate = self.rate;
        if model == 0 {
            self.basic_on = true;
            self.n_form = 0;
            self.n_fine = 0;
            let mid_hz = 100.0 * 100f32.powf(p4.clamp(0.0, 1.0));
            for c in 0..2 {
                self.basic[c][0].low_shelf(180.0, p1 * 18.0, rate);
                self.basic[c][1].peaking(mid_hz, 0.9, p2 * 18.0, rate);
                self.basic[c][2].high_shelf(5000.0, p3 * 18.0, rate);
            }
            return;
        }
        self.basic_on = false;
        let f = formants(model);
        self.n_form = f.len().min(MAX_FORMANTS);
        let shift = 10f32.powf(p2);
        for (i, &(hz, db, q)) in f.iter().enumerate().take(MAX_FORMANTS) {
            // stretch spreads the formants apart around 1 kHz (or pulls them together)
            let st = (hz / 1000.0).powf(1.0 + p3 * 0.8) * 1000.0;
            let fhz = (st * shift).clamp(30.0, rate * 0.45);
            for c in 0..2 {
                self.form[c][i].peaking(fhz, q, db * p1 * 1.5, rate);
            }
        }
        let fine = p4.clamp(0.0, 1.0);
        self.n_fine = if fine > 0.005 { FINE_BANDS } else { 0 };
        let mut rng = Rng::new(model as u32 * 977 + 5);
        for i in 0..FINE_BANDS {
            // a fixed irregular comb of narrow resonances: the fine structure of the body
            let hz = (90.0 * 1.52f32.powi(i as i32) * (0.9 + 0.2 * rng.unit()) * shift).clamp(40.0, rate * 0.45);
            let sign = if rng.unit() < 0.5 { 1.0 } else { -0.5 };
            for c in 0..2 {
                self.fine[c][i].peaking(hz, 24.0, 9.0 * fine * sign * p1.abs().max(0.2), rate);
            }
        }
    }
    #[inline]
    pub fn run(&mut self, c: usize, mut x: f32) -> f32 {
        if !self.on {
            return x;
        }
        if self.basic_on {
            for b in &mut self.basic[c] {
                x = b.run(x);
            }
            return x;
        }
        for b in self.form[c].iter_mut().take(self.n_form) {
            x = b.run(x);
        }
        for b in self.fine[c].iter_mut().take(self.n_fine) {
            x = b.run(x);
        }
        x
    }
}

// ---------------------------------------------------------------------------------------------
// Limiter

#[derive(Clone, Copy, Debug)]
pub struct Limiter {
    g: f32,
    rel: f32,
}

impl Limiter {
    pub fn new(rate: f32) -> Limiter {
        Limiter { g: 1.0, rel: dsp::tau_coef(0.12, rate) }
    }
    pub fn reset(&mut self) {
        self.g = 1.0;
    }
    /// The gain to apply for a signal of peak `p` against `thr`.
    #[inline]
    pub fn gain(&mut self, p: f32, thr: f32) -> f32 {
        let need = if p * self.g > thr { thr / p.max(1e-9) } else { 1.0 };
        if need < self.g {
            self.g = need;
        } else {
            self.g = 1.0 - (1.0 - self.g) * self.rel;
        }
        self.g
    }
}

/// The exponential frequency map used for cutoff-like values, 0..1 to 20 Hz..20 kHz.
pub fn cut_hz(c: f32) -> f32 {
    20.0 * 1000f32.powf(c.clamp(0.0, 1.0))
}

/// The body EQ's response in dB at `hz` for the drawing (the same model as `BodyEq::set`).
pub fn body_response_db(model: usize, p1: f32, p2: f32, p3: f32, p4: f32, hz: f32) -> f32 {
    let x = (hz / 1000.0).max(1e-3).ln();
    let bump = |f0: f32, db: f32, q: f32| db * (-(((hz / f0).max(1e-3).ln()) * q * 0.9).powi(2)).exp();
    if model == 0 {
        let mid = 100.0 * 100f32.powf(p4.clamp(0.0, 1.0));
        let low = p1 * 18.0 / (1.0 + (hz / 180.0).powi(2));
        let high = p3 * 18.0 * (hz / 5000.0).powi(2) / (1.0 + (hz / 5000.0).powi(2));
        return low + high + bump(mid, p2 * 18.0, 1.1);
    }
    let _ = x;
    let shift = 10f32.powf(p2);
    let mut db = 0.0;
    for &(f, g, q) in formants(model) {
        let st = (f / 1000.0).powf(1.0 + p3 * 0.8) * 1000.0;
        db += bump(st * shift, g * p1 * 1.5, q);
    }
    db + p4.clamp(0.0, 1.0) * 2.5 * (hz * 0.037).sin()
}
