//! Alchemy's multimode filters (Studio's own designs after the guide's list): low / band / high
//! pass at 6, 12, 24 and 60 dB in Clean (biquad, no Drive), Edgy and Rich (analog-style ladders),
//! Gritty (two-pole, saturating hard at high Resonance and Drive), Sharp (two-pole, high Q) and
//! Smooth (state-variable) designs; Notch, Peaking and Formant; three comb filters (Comb Pos,
//! Comb Neg, Comb PM) with Note / Feedback / Damp; RingMod (Note / Mix / Weird); the distortions
//! Bitcrusher, Downsample, Mech and Tube (Amount / Mix / PreGain, Downsample's Frequency);
//! Compressor (Threshold / Ratio / Gain); FM (Note / Mod / Feedback) and Allpass. The same filters
//! serve the sources (three per source, series or parallel) and the two main filters, per voice.

use crate::dsp::{self, Allpass1, Biquad, Ladder, OnePole, Svf, SvfMode};

pub const FILTER_TYPES: &[&str] = &[
    "LP 12dB Edgy", "LP 24dB Edgy", "LP 6dB Clean", "LP 12dB Clean", "LP 24dB Clean", "LP 60dB Clean", "LP 12dB Gritty", "LP 12dB Rich", "LP 24dB Rich", "LP 12dB Sharp", "LP 12dB Smooth", "LP 24dB Smooth",
    "BP 6dB Clean", "BP 12dB Edgy", "BP 12dB Gritty", "BP 12dB Smooth", "BP 24dB Rich",
    "HP 6dB Clean", "HP 12dB Clean", "HP 12dB Edgy", "HP 24dB Edgy", "HP 12dB Gritty", "HP 24dB Smooth", "HP 60dB Clean",
    "Notch", "Peaking", "Formant", "Comb Pos", "Comb Neg", "Comb PM", "RingMod", "Bitcrusher", "Downsample", "Mech", "Tube", "Compressor", "FM", "Allpass", "BP 24dB Clean",
];
pub const LP12_SMOOTH: usize = 10;
pub const COMB_LEN: usize = 2048;

/// The three knobs' names for a filter type (they change with the type).
pub fn knob_names(t: usize) -> [&'static str; 3] {
    match FILTER_TYPES.get(t).copied().unwrap_or("") {
        "Notch" => ["Frequency", "Amount", "Drive"],
        "Peaking" => ["Frequency", "Gain", "BW"],
        "Formant" => ["Cutoff", "Amount", "Drive"],
        "Comb Pos" | "Comb Neg" | "Comb PM" => ["Note", "Feedback", "Damp"],
        "RingMod" => ["Note", "Mix", "Weird"],
        "Bitcrusher" | "Mech" | "Tube" => ["Amount", "Mix", "PreGain"],
        "Downsample" => ["Frequency", "Mix", "PreGain"],
        "Compressor" => ["Threshold", "Ratio", "Gain"],
        "FM" => ["Note", "Mod", "Feedback"],
        _ => ["Cutoff", "Res", "Drive"],
    }
}

/// Clean (biquad) designs have no Drive.
pub fn has_drive(t: usize) -> bool {
    !FILTER_TYPES.get(t).is_some_and(|n| n.ends_with("Clean"))
}

/// The cutoff / note knob (0..1) in Hz.
pub fn cutoff_hz(x: f32) -> f32 {
    20.0 * 1000f32.powf(x.clamp(0.0, 1.0))
}

/// Is this a low pass that, wide open, changes nothing (so the voice can skip it)?
pub fn transparent(t: usize, cut: f32) -> bool {
    FILTER_TYPES.get(t).is_some_and(|n| n.starts_with("LP")) && cut >= 0.999
}

/// One channel of one filter.
#[derive(Clone)]
pub struct AFilt {
    ty: usize,
    bq: [Biquad; 5],
    nbq: usize,
    svf: [Svf; 2],
    lad: Ladder,
    op: OnePole,
    ap: [Allpass1; 4],
    ap_a: f32,
    comb: Box<[f32]>,
    pos: usize,
    delay: f32,
    damp: f32,
    lp: f32,
    phase: f32,
    inc: f32,
    hold: f32,
    count: f32,
    env: f32,
    prev: f32,
    // the knobs (0..1), the rate
    a: f32,
    b: f32,
    c: f32,
    rate: f32,
}

impl Default for AFilt {
    fn default() -> Self {
        AFilt {
            ty: LP12_SMOOTH,
            bq: [Biquad::default(); 5],
            nbq: 1,
            svf: [Svf::default(); 2],
            lad: Ladder::new(),
            op: OnePole::default(),
            ap: [Allpass1::default(); 4],
            ap_a: 0.0,
            comb: vec![0.0; COMB_LEN].into_boxed_slice(),
            pos: 0,
            delay: 100.0,
            damp: 0.0,
            lp: 0.0,
            phase: 0.0,
            inc: 0.0,
            hold: 0.0,
            count: 0.0,
            env: 0.0,
            prev: 0.0,
            a: 1.0,
            b: 0.0,
            c: 0.0,
            rate: 48000.0,
        }
    }
}

impl AFilt {
    pub fn reset(&mut self) {
        for b in &mut self.bq {
            b.reset();
        }
        for s in &mut self.svf {
            s.reset();
        }
        self.lad.reset();
        self.comb.fill(0.0);
        self.lp = 0.0;
        self.hold = 0.0;
        self.env = 0.0;
        self.prev = 0.0;
    }

    /// The type and the knobs for the next block: `hz` is the first knob as a frequency (the
    /// caller adds envelope / key tracking), b and c the other two (0..1).
    pub fn set(&mut self, ty: usize, hz: f32, b: f32, c: f32, rate: f32) {
        let ty = ty.min(FILTER_TYPES.len() - 1);
        if ty != self.ty {
            self.reset();
            self.ty = ty;
        }
        self.rate = rate;
        let hz = hz.clamp(10.0, rate * 0.45);
        self.a = ((hz / 20.0).ln() / 1000f32.ln()).clamp(0.0, 1.0);
        self.b = b;
        self.c = c;
        let name = FILTER_TYPES[ty];
        let q = 0.707 + b * 9.0;
        let poles = |n: &str| if n.contains("60dB") { 5 } else if n.contains("24dB") { 2 } else { 1 };
        match name {
            n if n.ends_with("Clean") => {
                self.nbq = poles(n);
                for k in 0..self.nbq {
                    // resonance on the last stage only
                    let qk = if k + 1 == self.nbq { q } else { 0.707 };
                    if n.starts_with("LP") {
                        self.bq[k].lowpass(hz, qk, rate);
                    } else if n.starts_with("HP") {
                        self.bq[k].highpass(hz, qk, rate);
                    } else {
                        self.bq[k].bandpass(hz, qk, rate);
                    }
                }
                self.op.lowpass_hz(hz, rate);
            }
            n if n.ends_with("Edgy") || n.ends_with("Rich") => {
                self.lad.set(hz, b * 0.97, rate);
                self.lad.poles = if n.contains("24dB") { 4 } else { 2 };
                self.lad.edgy = n.ends_with("Edgy");
                self.lad.drive = 1.0 + c * 4.0;
                let mode = if n.starts_with("HP") { SvfMode::High } else { SvfMode::Band };
                for s in &mut self.svf {
                    s.mode = mode;
                    s.set(hz, 0.6 + b * 6.0, rate);
                }
            }
            n if n.ends_with("Gritty") || n.ends_with("Sharp") || n.ends_with("Smooth") => {
                let mode = if n.starts_with("LP") { SvfMode::Low } else if n.starts_with("HP") { SvfMode::High } else { SvfMode::Band };
                let qq = if n.ends_with("Sharp") { 0.8 + b * 25.0 } else { 0.6 + b * 10.0 };
                for s in &mut self.svf {
                    s.mode = mode;
                    s.set(hz, qq, rate);
                }
            }
            "Notch" => {
                self.svf[0].mode = SvfMode::Notch;
                // Amount widens the cut
                self.svf[0].set(hz, 4.0 - b * 3.6, rate);
            }
            "Peaking" => self.bq[0].peaking(hz, 0.3 + (1.0 - c) * 8.0, (b - 0.5) * 36.0, rate),
            "Formant" => {
                self.svf[0].mode = SvfMode::Band;
                self.svf[0].set(hz, 1.0 + (1.0 - b) * 15.0, rate);
                self.svf[1].mode = SvfMode::Band;
                self.svf[1].set((hz * 1.9).min(rate * 0.45), 1.0 + (1.0 - b) * 12.0, rate);
            }
            "Comb Pos" | "Comb Neg" | "Comb PM" => {
                self.delay = (rate / hz).clamp(2.0, (COMB_LEN - 4) as f32);
                self.damp = c;
            }
            "RingMod" | "FM" => self.inc = hz / rate,
            "Downsample" => self.inc = hz / rate,
            "Allpass" => {
                let t = (std::f32::consts::PI * hz / rate).tan();
                self.ap_a = (1.0 - t) / (1.0 + t);
            }
            _ => {}
        }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let (b, c) = (self.b, self.c);
        let name = FILTER_TYPES[self.ty];
        match self.ty {
            2 | 12 | 17 => {
                // 6 dB: one pole (band = low pass of the high pass)
                let lo = self.op.lp(x);
                match self.ty {
                    2 => lo,
                    17 => x - lo,
                    _ => self.bq[0].run(x),
                }
            }
            _ if name.ends_with("Clean") => {
                let mut y = x;
                for k in 0..self.nbq {
                    y = self.bq[k].run(y);
                }
                y
            }
            _ if name.ends_with("Edgy") || name.ends_with("Rich") => {
                if name.starts_with("LP") {
                    self.lad.run(x)
                } else {
                    let d = 1.0 + c * 4.0;
                    let y = self.svf[0].run(dsp::soft_clip(x * d) / d.sqrt());
                    if name.contains("24dB") { self.svf[1].run(y) } else { y }
                }
            }
            _ if name.ends_with("Gritty") => {
                // saturates inside the loop, more with Resonance and Drive
                let g = 1.0 + c * 6.0 + b * 3.0;
                let y = self.svf[0].run(dsp::soft_clip(x * g));
                dsp::soft_clip(y * (1.0 + b * 2.0)) / g.sqrt()
            }
            _ if name.ends_with("Sharp") => {
                let g = 1.0 + c * 3.0;
                dsp::soft_clip(self.svf[0].run(x * g)) / g.sqrt()
            }
            _ if name.ends_with("Smooth") => {
                let g = 1.0 + c * 3.0;
                let y = self.svf[0].run(if c > 0.0 { dsp::soft_clip(x * g) / g.sqrt() } else { x });
                if name.contains("24dB") { self.svf[1].run(y) } else { y }
            }
            _ => match name {
                "Notch" => self.svf[0].run(x),
                "Peaking" => self.bq[0].run(x),
                "Formant" => {
                    let a = self.svf[0].run_bp_norm(x);
                    a + 0.5 * self.svf[1].run_bp_norm(x)
                }
                "Comb Pos" | "Comb Neg" | "Comb PM" => {
                    let d = if name == "Comb PM" { (self.delay * (1.0 + 0.5 * x.clamp(-1.0, 1.0))).clamp(2.0, (COMB_LEN - 4) as f32) } else { self.delay };
                    let rp = self.pos as f32 + COMB_LEN as f32 - d;
                    let i0 = rp as usize;
                    let fr = rp - i0 as f32;
                    let a = self.comb[i0 % COMB_LEN];
                    let bb = self.comb[(i0 + 1) % COMB_LEN];
                    let tap = a + (bb - a) * fr;
                    // Damp: a low pass in the loop
                    self.lp += (tap - self.lp) * (1.0 - c * 0.9);
                    let fb = b * 0.97 * if name == "Comb Neg" { -1.0 } else { 1.0 };
                    let y = x + fb * self.lp;
                    self.comb[self.pos] = dsp::flush(y.clamp(-4.0, 4.0));
                    self.pos = (self.pos + 1) % COMB_LEN;
                    y * (1.0 - b * 0.5)
                }
                "RingMod" => {
                    self.phase = (self.phase + self.inc).fract();
                    let mut car = dsp::sin_cycles(self.phase);
                    // Weird: from a pure sine toward clipped, noisier waves
                    if c > 0.0 {
                        let k = 1.0 + c * 8.0;
                        car = (car * k).clamp(-1.0, 1.0) * (1.0 - 0.15 * c) + 0.15 * c * dsp::sin_cycles(self.phase * 7.13);
                    }
                    // Mix: 0 % ring (−1..1), 100 % amplitude modulation (0..1)
                    let carrier = car * (1.0 - b) + (car * 0.5 + 0.5) * b;
                    x * carrier
                }
                "Bitcrusher" => {
                    let g = 1.0 + c * 8.0;
                    let bits = 16.0 - self.a * 14.0;
                    let q = 2f32.powf(bits - 1.0);
                    let w = ((x * g).clamp(-1.0, 1.0) * q).round() / q;
                    x * (1.0 - b) + w * b
                }
                "Downsample" => {
                    let g = 1.0 + c * 8.0;
                    self.count += self.inc;
                    if self.count >= 1.0 {
                        self.count -= self.count.floor();
                        self.hold = dsp::soft_clip(x * g);
                    }
                    x * (1.0 - b) + self.hold * b
                }
                "Mech" => {
                    let g = (1.0 + c * 8.0) * (1.0 + self.a * 6.0);
                    let w = (x * g * std::f32::consts::FRAC_PI_2).sin();
                    x * (1.0 - b) + w * b
                }
                "Tube" => {
                    let g = (1.0 + c * 8.0) * (1.0 + self.a * 6.0);
                    let bias = 0.3 * self.a;
                    let w = dsp::fast_tanh(x * g + bias) - dsp::fast_tanh(bias);
                    x * (1.0 - b) + w * b
                }
                "Compressor" => {
                    let thr = lg_dsp::db_to_gain(-60.0 + self.a * 60.0);
                    let ratio = 1.0 + b * 19.0;
                    let lvl = x.abs();
                    let k = if lvl > self.env { 0.01 } else { 0.0005 };
                    self.env += (lvl - self.env) * k;
                    let gr = if self.env > thr { (thr / self.env).powf(1.0 - 1.0 / ratio) } else { 1.0 };
                    x * gr * lg_dsp::db_to_gain(c * 24.0)
                }
                "FM" => {
                    self.phase = (self.phase + self.inc).fract();
                    let y = dsp::sin_cycles(self.phase + x * b * 2.0 + self.prev * c * 0.5);
                    self.prev = y;
                    y * 0.7
                }
                "Allpass" => {
                    let mut y = x;
                    for k in 0..4 {
                        y = self.ap[k].run(y, self.ap_a);
                    }
                    y
                }
                _ => x,
            },
        }
    }
}
