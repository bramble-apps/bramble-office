//! Match EQ, as the Effects guide describes it: learn the spectrum of the material (Current
//! Learn) and of a reference (Reference Learn — Studio has no side chains, so the reference is
//! learned from what plays through the plug-in while Reference Learn is on, or from an audio
//! file the window opens), and Match applies the difference as an EQ curve: Apply (−100..200 %),
//! Smoothing, Limit Boost / Limit Cut with their maxima, Ease-In (lows) and Ease-Out (highs)
//! ranges, Channel (L&R / L / R), and Phase (Linear, Minimal, Minimal Zero Latency).
//!
//! The spectra are 63 third-ish-octave bands (20 Hz–20 kHz) kept in the plug-in's hidden
//! parameters, so they're saved with the setting (Copy / Paste / Clear in the window). Linear
//! phase is a symmetric FIR (latency half its length); Minimal is a cascade of peaking filters
//! (no latency). Ids are our own from 1000.

use crate::convolve::{C, Convolver, Fft};
use crate::meters::Ring;
use crate::util::{Biquad, Coefs, Values, db};
use lg_dsp::{Effect, Param, Plugin, Unit};

const B: u32 = 1000;
pub const BANDS: usize = 63;
pub const CURRENT: u32 = 2000;
pub const REFERENCE: u32 = 2100;
pub const CURRENT_LEARN: u32 = B;
pub const REFERENCE_LEARN: u32 = B + 1;
pub const MATCH: u32 = B + 2;
pub const APPLY: u32 = B + 3;
pub const SMOOTHING: u32 = B + 4;
pub const PHASE: u32 = B + 5;
pub const LIMIT_BOOST: u32 = B + 6;
pub const BOOST_MAX: u32 = B + 7;
pub const LIMIT_CUT: u32 = B + 8;
pub const CUT_MAX: u32 = B + 9;
pub const EASE_IN: u32 = B + 10;
pub const EASE_IN_LO: u32 = B + 11;
pub const EASE_IN_HI: u32 = B + 12;
pub const EASE_OUT: u32 = B + 13;
pub const EASE_OUT_LO: u32 = B + 14;
pub const EASE_OUT_HI: u32 = B + 15;
pub const CHANNEL: u32 = B + 16;
pub const HAS_CURRENT: u32 = B + 17;
pub const HAS_REFERENCE: u32 = B + 18;

pub fn band_hz(k: usize) -> f32 {
    20.0 * 1000f32.powf(k as f32 / (BANDS - 1) as f32)
}

static P: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::new(CURRENT_LEARN, "Current Learn", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(REFERENCE_LEARN, "Reference Learn", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(MATCH, "Match", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(APPLY, "Apply", -100.0, 200.0, 100.0, Unit::Percent),
        Param::new(SMOOTHING, "Smoothing", 0.0, 100.0, 20.0, Unit::Percent),
        Param::menu(PHASE, "Phase", &["Linear", "Minimal", "Minimal, Zero Latency"], 2.0),
        Param::new(LIMIT_BOOST, "Limit Boost", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(BOOST_MAX, "Limit Boost Max", 0.0, 24.0, 12.0, Unit::Db),
        Param::new(LIMIT_CUT, "Limit Cut", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(CUT_MAX, "Limit Cut Max", 0.0, 24.0, 12.0, Unit::Db),
        Param::new(EASE_IN, "Ease-In", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(EASE_IN_LO, "Ease-In Low", 20.0, 2000.0, 30.0, Unit::Hz),
        Param::new(EASE_IN_HI, "Ease-In High", 20.0, 2000.0, 80.0, Unit::Hz),
        Param::new(EASE_OUT, "Ease-Out", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(EASE_OUT_LO, "Ease-Out Low", 2000.0, 20000.0, 10000.0, Unit::Hz),
        Param::new(EASE_OUT_HI, "Ease-Out High", 2000.0, 20000.0, 16000.0, Unit::Hz),
        Param::menu(CHANNEL, "Channel", &["L&R", "L", "R"], 0.0),
        Param::new(HAS_CURRENT, "Current Spectrum", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
        Param::new(HAS_REFERENCE, "Reference Spectrum", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
    ];
    for k in 0..BANDS {
        v.push(Param::new(CURRENT + k as u32, crate::kit::leak(format!("Current {k}")), -150.0, 50.0, -60.0, Unit::Db).hide());
        v.push(Param::new(REFERENCE + k as u32, crate::kit::leak(format!("Reference {k}")), -150.0, 50.0, -60.0, Unit::Db).hide());
    }
    v
});

const FIR: usize = 4096;

pub struct MatchEq {
    p: Values,
    rate: f32,
    ring: Ring,
    acc: [Vec<f64>; 2],
    frames: [u32; 2],
    since: usize,
    bq: [[Biquad; BANDS]; 2],
    conv: [Convolver; 2],
    curve: Vec<f32>,
}

impl MatchEq {
    pub fn new(rate: f32) -> MatchEq {
        let mut m = MatchEq { p: Values::new(P.as_slice()), rate, ring: Ring::new(4096), acc: [vec![0.0; BANDS], vec![0.0; BANDS]], frames: [0; 2], since: 0, bq: [[Biquad::default(); BANDS]; 2], conv: [Convolver::new(FIR), Convolver::new(FIR)], curve: vec![0.0; BANDS] };
        m.update();
        m
    }

    /// Learns a spectrum from audio given directly (Generate … Spectrum from audio file).
    pub fn learn_from(&mut self, which_reference: bool, l: &[f32], r: &[f32]) {
        let mut acc = vec![0f64; BANDS];
        let mut frames = 0;
        let mut at = 0;
        while at + 4096 <= l.len() {
            let mono: Vec<f32> = (at..at + 4096).map(|i| self.pick(l[i], r.get(i).copied().unwrap_or(l[i]))).collect();
            for (k, v) in crate::meters::spectrum(&mono, self.rate, BANDS).iter().enumerate() {
                acc[k] += 10f64.powf(*v as f64 / 10.0);
            }
            frames += 1;
            at += 2048;
        }
        if frames > 0 {
            let base = if which_reference { REFERENCE } else { CURRENT };
            for k in 0..BANDS {
                self.p.set(base + k as u32, (10.0 * (acc[k] / frames as f64).max(1e-15).log10()) as f32);
            }
            self.p.set(if which_reference { HAS_REFERENCE } else { HAS_CURRENT }, 1.0);
            self.update();
        }
    }

    fn pick(&self, l: f32, r: f32) -> f32 {
        match self.p.idx(CHANNEL) {
            1 => l,
            2 => r,
            _ => 0.5 * (l + r),
        }
    }

    /// The EQ curve (dB per band) Match applies: reference − current, smoothed, scaled by
    /// Apply, limited, eased in and out.
    pub fn compute_curve(&self) -> Vec<f32> {
        if !(self.p.on(HAS_CURRENT) && self.p.on(HAS_REFERENCE)) {
            return vec![0.0; BANDS];
        }
        let raw: Vec<f32> = (0..BANDS).map(|k| self.p.get(REFERENCE + k as u32) - self.p.get(CURRENT + k as u32)).collect();
        // remove the overall level difference (Match shapes the tone, not the level)
        let mean = raw.iter().sum::<f32>() / BANDS as f32;
        let raw: Vec<f32> = raw.iter().map(|v| v - mean).collect();
        let w = (self.p.get(SMOOTHING) / 100.0 * 8.0).round() as usize;
        let mut c: Vec<f32> = (0..BANDS).map(|k| {
            let (a, b) = (k.saturating_sub(w), (k + w + 1).min(BANDS));
            raw[a..b].iter().sum::<f32>() / (b - a) as f32
        }).collect();
        let apply = self.p.get(APPLY) / 100.0;
        for (k, v) in c.iter_mut().enumerate() {
            *v *= apply;
            if self.p.on(LIMIT_BOOST) {
                *v = v.min(self.p.get(BOOST_MAX));
            }
            if self.p.on(LIMIT_CUT) {
                *v = v.max(-self.p.get(CUT_MAX));
            }
            let f = band_hz(k);
            if self.p.on(EASE_IN) {
                let (lo, hi) = (self.p.get(EASE_IN_LO), self.p.get(EASE_IN_HI).max(self.p.get(EASE_IN_LO) + 1.0));
                *v *= ((f.ln() - lo.ln()) / (hi.ln() - lo.ln())).clamp(0.0, 1.0);
            }
            if self.p.on(EASE_OUT) {
                let (lo, hi) = (self.p.get(EASE_OUT_LO), self.p.get(EASE_OUT_HI).max(self.p.get(EASE_OUT_LO) + 1.0));
                *v *= 1.0 - ((f.ln() - lo.ln()) / (hi.ln() - lo.ln())).clamp(0.0, 1.0);
            }
            *v = v.clamp(-30.0, 30.0);
        }
        c
    }

    fn curve_at(&self, f: f32) -> f32 {
        let t = (f.max(20.0) / 20.0).ln() / 1000f32.ln() * (BANDS - 1) as f32;
        let k = (t.floor() as usize).min(BANDS - 2);
        let u = (t - k as f32).clamp(0.0, 1.0);
        self.curve[k] * (1.0 - u) + self.curve[k + 1] * u
    }

    fn update(&mut self) {
        self.curve = self.compute_curve();
        let on = self.p.on(MATCH);
        // minimal phase: one peaking filter per band at the curve's value
        for k in 0..BANDS {
            let g = if on { self.curve[k] } else { 0.0 };
            let c = if g.abs() < 0.05 || band_hz(k) >= self.rate * 0.45 { Coefs::THRU } else { Coefs::peak(self.rate, band_hz(k), 4.3, g * 0.9) };
            self.bq[0][k].c = c;
            self.bq[1][k].c = c;
        }
        // linear phase: frequency-sampled FIR
        if self.p.idx(PHASE) == 0 {
            let fft = Fft::new(FIR);
            let mut buf: Vec<C> = (0..FIR).map(|k| {
                let kk = if k <= FIR / 2 { k } else { FIR - k };
                let f = kk as f32 * self.rate / FIR as f32;
                C { re: if on { db(self.curve_at(f)) } else { 1.0 }, im: 0.0 }
            }).collect();
            fft.run(&mut buf, true);
            let fir: Vec<f32> = (0..FIR).map(|i| {
                let src = (i + FIR / 2) % FIR;
                let w = 0.5 - 0.5 * (crate::util::TAU * i as f32 / FIR as f32).cos();
                buf[src].re / FIR as f32 * w
            }).collect();
            for c in self.conv.iter_mut() {
                c.load(&fir);
            }
        }
        if !self.p.on(CURRENT_LEARN) && !self.p.on(REFERENCE_LEARN) {
            self.frames = [0; 2];
            self.acc.iter_mut().for_each(|a| a.iter_mut().for_each(|v| *v = 0.0));
        }
    }

    /// Clear / Copy / Paste of a spectrum: (reference?, the 63 values) or a clear.
    pub fn spectrum(&self, reference: bool) -> Vec<f32> {
        let base = if reference { REFERENCE } else { CURRENT };
        (0..BANDS).map(|k| self.p.get(base + k as u32)).collect()
    }
}

impl Effect for MatchEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let learning = [self.p.on(CURRENT_LEARN), self.p.on(REFERENCE_LEARN)];
        let linear = self.p.idx(PHASE) == 0;
        let on = self.p.on(MATCH);
        for i in 0..left.len().min(right.len()) {
            if learning[0] || learning[1] {
                let m = self.pick(left[i], right[i]);
                self.ring.push(m, m);
                self.since += 1;
                if self.since >= 2048 {
                    self.since = 0;
                    let (l, _) = self.ring.ordered();
                    let s = crate::meters::spectrum(&l, self.rate, BANDS);
                    let which = if learning[1] { 1 } else { 0 };
                    for (k, v) in s.iter().enumerate() {
                        self.acc[which][k] += 10f64.powf(*v as f64 / 10.0);
                    }
                    self.frames[which] += 1;
                    let base = if which == 1 { REFERENCE } else { CURRENT };
                    let n = self.frames[which] as f64;
                    for k in 0..BANDS {
                        self.p.set(base + k as u32, (10.0 * (self.acc[which][k] / n).max(1e-15).log10()) as f32);
                    }
                    self.p.set(if which == 1 { HAS_REFERENCE } else { HAS_CURRENT }, 1.0);
                }
            }
            if !on {
                continue;
            }
            if linear {
                left[i] = self.conv[0].run(left[i]);
                right[i] = self.conv[1].run(right[i]);
            } else {
                for k in 0..BANDS {
                    left[i] = self.bq[0][k].run(left[i]);
                    right[i] = self.bq[1][k].run(right[i]);
                }
            }
        }
    }
    fn latency(&self) -> usize {
        if self.p.idx(PHASE) == 0 && self.p.on(MATCH) { FIR / 2 } else { 0 }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        match name {
            "current" => self.spectrum(false),
            "reference" => self.spectrum(true),
            "eq" => self.compute_curve(),
            _ => Vec::new(),
        }
    }
}

impl Plugin for MatchEq {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        // only one Learn at a time
        if id == CURRENT_LEARN && v >= 0.5 {
            self.p.set(REFERENCE_LEARN, 0.0);
        }
        if id == REFERENCE_LEARN && v >= 0.5 {
            self.p.set(CURRENT_LEARN, 0.0);
        }
        // a learn finished: Match turns on with the new curve
        let ending = (id == CURRENT_LEARN || id == REFERENCE_LEARN) && v < 0.5 && self.p.on(id);
        if self.p.set(id, v) {
            if ending && self.p.on(HAS_CURRENT) && self.p.on(HAS_REFERENCE) {
                self.p.set(MATCH, 1.0);
            }
            self.update();
        }
    }
    fn reset(&mut self) {
        for c in self.conv.iter_mut() {
            c.clear();
        }
        for b in self.bq.iter_mut().flatten() {
            b.clear();
        }
    }
    fn load_floats(&mut self, f: &[f32]) {
        for d in self.p.defs {
            if let Some(v) = f.get(d.id as usize) {
                self.p.set(d.id, *v);
            }
        }
        self.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut z = crate::util::Noise(seed);
        (0..n).map(|_| z.next() * 0.3).collect()
    }

    #[test]
    fn matches_a_dull_signal_to_a_bright_reference() {
        let rate = 48000.0;
        let bright = noise(48000 * 2, 5);
        // the current material: the same noise low-passed at 2 kHz
        let mut lp = [Biquad::new(Coefs::lowpass(rate, 2000.0, 0.7)), Biquad::new(Coefs::lowpass(rate, 2000.0, 0.7))];
        let dull: Vec<f32> = bright.iter().map(|x| {
            let y = lp[0].run(*x);
            lp[1].run(y)
        }).collect();
        let mut m = MatchEq::new(rate);
        m.learn_from(false, &dull, &dull);
        m.learn_from(true, &bright, &bright);
        let c = m.compute_curve();
        // the curve lifts the highs relative to the lows
        assert!(c[55] - c[10] > 15.0, "{} {}", c[10], c[55]);
        // limits hold
        m.set(LIMIT_BOOST, 1.0);
        m.set(BOOST_MAX, 6.0);
        assert!(m.compute_curve().iter().all(|v| *v <= 6.0 + 1e-4));
    }

    #[test]
    fn learning_from_playback_fills_the_spectrum() {
        let mut m = MatchEq::new(48000.0);
        m.set(CURRENT_LEARN, 1.0);
        let mut l = noise(48000, 9);
        let mut r = l.clone();
        m.process(&mut l, &mut r);
        m.set(CURRENT_LEARN, 0.0);
        assert!(m.get(HAS_CURRENT) > 0.5);
        assert!(m.spectrum(false).iter().all(|v| *v > -100.0));
    }

    #[test]
    fn linear_phase_latency() {
        let mut m = MatchEq::new(48000.0);
        m.set(PHASE, 0.0);
        m.set(MATCH, 1.0);
        assert_eq!(m.latency(), FIR / 2);
        let mut l = vec![0.0; FIR];
        l[0] = 1.0;
        let mut r = l.clone();
        m.process(&mut l, &mut r);
        let at = (0..FIR).max_by(|a, b| l[*a].abs().partial_cmp(&l[*b].abs()).unwrap()).unwrap();
        assert_eq!(at, FIR / 2);
    }
}
