//! Mastering Assistant, as the Effects guide describes it: an analysis of the whole mix (or
//! the cycle) sets the Auto EQ correction curve, the loudness and the width; then the chain:
//! Auto EQ (scaled by the Auto EQ slider) → Custom EQ (low shelf 20–200 Hz, mid 200 Hz–8 kHz,
//! high shelf 8–20 kHz, ±6 dB) → the Character (Clean: transparent yet punchy, Valve: tubes with
//! deep lows and refined highs, Punch: aggressive with a mid emphasis, Transparent: a modern
//! tight compressor) → Excite (upper-mid saturation) → Width → the Loudness gain (knob centre ≈
//! −14 LUFS-I) → a limiter at −1 dBTP. Bypass switches to the unprocessed audio; Loudness
//! Compensation matches the processed level to the original. Meters: true peak (with max),
//! LUFS M / S / I, LU range (Start / Pause, Reset), correlation, and a spectrum.
//!
//! `analyze` runs on the mix the app renders (Reanalyze / Analyze), and its result is stored in
//! the plug-in's own hidden parameters (the Auto EQ bands, the measured loudness), so it is saved
//! with the setting. Logic's own Auto EQ block (a 512-point curve in its floats) isn't decoded.

use crate::limiter::Limiter;
use crate::meters::{Correlation, Loudness, Ring, TruePeak, spectrum};
use crate::util::{Biquad, Coefs, Follower, Values, db, flush};
use lg_dsp::{Effect, Param, Plugin, Unit};

pub const CHARACTERS: &[&str] = &["Clean", "Valve", "Punch", "Transparent"];
pub const CHARACTER: u32 = 1;
pub const CUSTOM_EQ: u32 = 13;
pub const LOW_GAIN: u32 = 14;
pub const LOW_FREQ: u32 = 15;
pub const MID_GAIN: u32 = 16;
pub const MID_FREQ: u32 = 17;
pub const HIGH_GAIN: u32 = 18;
pub const HIGH_FREQ: u32 = 19;
pub const EXCITE: u32 = 20;
pub const WIDTH: u32 = 21;
pub const LOUDNESS: u32 = 24;
pub const AUTO_EQ: u32 = 25;
/// The Auto EQ correction: gains (dB) of ten octave bands, 31.5 Hz–16 kHz.
pub const AUTO_BANDS: u32 = 1200;
pub const ANALYZED_LUFS: u32 = 1210;
pub const LOUDNESS_COMP: u32 = 1211;
pub const BYPASS: u32 = 1212;
pub const METER_RUNNING: u32 = 1213;
pub const METER_RESET: u32 = 1214;
pub const ANALYZED: u32 = 1215;
pub const BAND_HZ: [f32; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// The tonal balance the analysis aims for: octave-band levels relative to 500 Hz–1 kHz (dB),
/// roughly the long-term spectrum of well-balanced masters (our own reference).
pub const TARGET: [f32; 10] = [1.5, 3.0, 2.5, 1.0, 0.0, 0.0, -2.0, -4.0, -6.5, -11.0];

static MA_PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::menu(CHARACTER, "Character", CHARACTERS, 0.0),
        Param::new(AUTO_EQ, "Auto EQ", 0.0, 100.0, 100.0, Unit::Percent),
        Param::new(CUSTOM_EQ, "Custom EQ", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(LOW_FREQ, "Low Freq", 20.0, 200.0, 100.0, Unit::Hz),
        Param::new(LOW_GAIN, "Low Gain", -6.0, 6.0, 0.0, Unit::Db),
        Param::new(MID_FREQ, "Mid Freq", 200.0, 8000.0, 998.9, Unit::Hz),
        Param::new(MID_GAIN, "Mid Gain", -6.0, 6.0, 0.0, Unit::Db),
        Param::new(HIGH_FREQ, "High Freq", 8000.0, 20000.0, 8000.0, Unit::Hz),
        Param::new(HIGH_GAIN, "High Gain", -6.0, 6.0, 0.0, Unit::Db),
        Param::new(LOUDNESS, "Loudness", -12.0, 12.0, -0.17, Unit::Db),
        Param::new(EXCITE, "Excite", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(WIDTH, "Width", -100.0, 100.0, 0.0, Unit::Percent),
        Param::new(LOUDNESS_COMP, "Loudness Compensation", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(BYPASS, "Bypass", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(METER_RUNNING, "Loudness Meter Running", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(METER_RESET, "Loudness Meter Reset", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(ANALYZED_LUFS, "Analyzed Loudness", -70.0, 0.0, -14.0, Unit::Db).hide(),
        Param::new(ANALYZED, "Analyzed", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
    ];
    for (k, f) in BAND_HZ.iter().enumerate() {
        let n = if *f >= 1000.0 { format!("Auto EQ {} kHz", f / 1000.0) } else { format!("Auto EQ {f} Hz") };
        v.push(Param::new(AUTO_BANDS + k as u32, crate::kit::leak(n), -6.0, 6.0, 0.0, Unit::Db).hide());
    }
    v
});

/// The analysis's result: parameter values to set.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub bands: [f32; 10],
    pub lufs: f32,
    pub width: f32,
    /// The measured octave-band levels relative to 500 Hz–1 kHz (for the window's curve).
    pub measured: [f32; 10],
}

/// Analyzes a rendered mix (stereo, `rate`): the octave-band balance against TARGET (the Auto
/// EQ correction, at most ±4 dB per band, neighbours smoothed), the integrated loudness, and
/// whether the image is too narrow or too wide (the Width setting).
pub fn analyze(l: &[f32], r: &[f32], rate: f32) -> Analysis {
    let mut energy = [0f64; 10];
    for (k, f) in BAND_HZ.iter().enumerate() {
        if *f >= rate * 0.45 {
            continue;
        }
        let c = Coefs::bandpass(rate, *f, 1.41);
        let (mut a, mut b) = (Biquad::new(c), Biquad::new(c));
        let (mut a2, mut b2) = (Biquad::new(c), Biquad::new(c));
        let mut e = 0f64;
        for (x, y) in l.iter().zip(r) {
            let u = a2.run(a.run(*x));
            let v = b2.run(b.run(*y));
            e += (u * u + v * v) as f64;
        }
        energy[k] = e / l.len().max(1) as f64;
    }
    let lvl: Vec<f32> = energy.iter().map(|e| 10.0 * (e.max(1e-20)).log10() as f32).collect();
    let refl = 0.5 * (lvl[4] + lvl[5]);
    let mut measured = [0f32; 10];
    let mut raw = [0f32; 10];
    for k in 0..10 {
        measured[k] = lvl[k] - refl;
        raw[k] = if energy[k] <= 1e-18 { 0.0 } else { (0.6 * (TARGET[k] - measured[k])).clamp(-4.0, 4.0) };
    }
    let mut bands = [0f32; 10];
    for k in 0..10usize {
        let a = raw[k.saturating_sub(1)];
        let c = raw[(k + 1).min(9)];
        bands[k] = (0.25 * a + 0.5 * raw[k] + 0.25 * c).clamp(-4.0, 4.0);
    }
    let mut m = Loudness::new(rate);
    let (mut mid, mut side) = (0f64, 0f64);
    for (x, y) in l.iter().zip(r) {
        m.push(*x, *y);
        mid += ((x + y) * (x + y)) as f64;
        side += ((x - y) * (x - y)) as f64;
    }
    let ratio = (side / mid.max(1e-20)) as f32;
    let width = if mid < 1e-12 { 0.0 } else if ratio < 0.03 { 20.0 } else if ratio > 0.5 { -25.0 } else { 0.0 };
    Analysis { bands, lufs: m.integrated().max(-70.0), width, measured }
}

pub struct MasteringAssistant {
    p: Values,
    rate: f32,
    auto: [[Biquad; 10]; 2],
    eq: [[Biquad; 3]; 2],
    color: [[Biquad; 2]; 2],
    exc_hp: [[Biquad; 2]; 2],
    comp: Follower,
    gr: f32,
    lim: Limiter,
    gain: f32,
    loud: Loudness,
    tp: TruePeak,
    corr: Correlation,
    ring: Ring,
    dry: Vec<[f32; 2]>,
}

impl MasteringAssistant {
    pub fn new(rate: f32) -> MasteringAssistant {
        let mut lim = Limiter::new(rate);
        lim.set(4, -1.0);
        lim.set(3, 120.0);
        let lat = lim.latency_samples();
        let mut m = MasteringAssistant {
            p: Values::new(MA_PARAMS.as_slice()),
            rate,
            auto: [[Biquad::default(); 10]; 2],
            eq: [[Biquad::default(); 3]; 2],
            color: [[Biquad::default(); 2]; 2],
            exc_hp: [[Biquad::default(); 2]; 2],
            comp: Follower::new(rate, 20.0, 200.0),
            gr: 0.0,
            lim,
            gain: 1.0,
            loud: Loudness::new(rate),
            tp: TruePeak::default(),
            corr: Correlation::new(rate),
            ring: Ring::new(8192),
            dry: vec![[0.0; 2]; lat.max(1)],
        };
        m.update();
        m
    }

    /// Stores an analysis's result in the plug-in (the Auto EQ bands, loudness, width).
    pub fn apply(&mut self, a: &Analysis) {
        for k in 0..10 {
            self.p.set(AUTO_BANDS + k as u32, a.bands[k]);
        }
        self.p.set(ANALYZED_LUFS, a.lufs);
        self.p.set(WIDTH, a.width);
        self.p.set(ANALYZED, 1.0);
        self.update();
    }

    /// The analysis as floats to set (id, value): for the app, which runs `analyze`.
    pub fn analysis_floats(a: &Analysis) -> Vec<(u32, f32)> {
        let mut v: Vec<(u32, f32)> = (0..10).map(|k| (AUTO_BANDS + k as u32, a.bands[k])).collect();
        v.extend([(ANALYZED_LUFS, a.lufs), (WIDTH, a.width), (ANALYZED, 1.0)]);
        v
    }

    /// The loudness gain (dB): the knob around −14 LUFS from the analyzed level.
    pub fn loudness_gain_db(&self) -> f32 {
        (-14.0 + self.p.get(LOUDNESS) - self.p.get(ANALYZED_LUFS)).clamp(-24.0, 24.0)
    }

    fn update(&mut self) {
        if self.p.on(METER_RESET) {
            self.p.set(METER_RESET, 0.0);
            self.loud.reset();
            self.tp.clear_max();
        }
        self.loud.running = self.p.on(METER_RUNNING);
        let (p, r) = (&self.p, self.rate);
        let amt = p.get(AUTO_EQ) / 100.0;
        for k in 0..10 {
            let g = p.get(AUTO_BANDS + k as u32) * amt;
            let c = if g.abs() < 0.01 || BAND_HZ[k] >= r * 0.45 {
                Coefs::THRU
            } else if k == 0 {
                Coefs::low_shelf(r, 45.0, 0.7, g)
            } else if k == 9 {
                Coefs::high_shelf(r, 11000.0, 0.7, g)
            } else {
                Coefs::peak(r, BAND_HZ[k], 1.41, g)
            };
            self.auto[0][k].c = c;
            self.auto[1][k].c = c;
        }
        let on = p.on(CUSTOM_EQ);
        let c = [
            if on && p.get(LOW_GAIN).abs() > 0.01 { Coefs::low_shelf(r, p.get(LOW_FREQ), 0.7, p.get(LOW_GAIN)) } else { Coefs::THRU },
            if on && p.get(MID_GAIN).abs() > 0.01 { Coefs::peak(r, p.get(MID_FREQ).min(r * 0.45), 0.8, p.get(MID_GAIN)) } else { Coefs::THRU },
            if on && p.get(HIGH_GAIN).abs() > 0.01 { Coefs::high_shelf(r, p.get(HIGH_FREQ).min(r * 0.45), 0.7, p.get(HIGH_GAIN)) } else { Coefs::THRU },
        ];
        let ch = p.idx(CHARACTER);
        let col = match ch {
            1 => [Coefs::low_shelf(r, 80.0, 0.7, 1.0), Coefs::high_shelf(r, 12000.0, 0.7, 0.6)],
            2 => [Coefs::peak(r, 1500.0, 0.7, 1.5), Coefs::THRU],
            _ => [Coefs::THRU, Coefs::THRU],
        };
        let (att, rel) = match ch {
            0 => (30.0, 300.0),
            1 => (25.0, 250.0),
            2 => (20.0, 150.0),
            _ => (5.0, 100.0),
        };
        self.comp.set(r, att, rel);
        for s in 0..2 {
            for k in 0..3 {
                self.eq[s][k].c = c[k];
            }
            self.color[s][0].c = col[0];
            self.color[s][1].c = col[1];
            let h = Coefs::highpass(r, 2500.0, 0.707);
            self.exc_hp[s][0].c = h;
            self.exc_hp[s][1].c = Coefs::lowpass(r, 9000.0, 0.707);
        }
        self.gain = db(self.loudness_gain_db());
    }
    fn clear(&mut self) {
        for b in self.auto.iter_mut().flatten().chain(self.eq.iter_mut().flatten()).chain(self.exc_hp.iter_mut().flatten()).chain(self.color.iter_mut().flatten()) {
            b.clear();
        }
        self.gr = 0.0;
        self.lim.reset();
        self.dry.iter_mut().for_each(|d| *d = [0.0; 2]);
    }
}

impl Effect for MasteringAssistant {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let character = self.p.idx(CHARACTER);
        let excite = self.p.on(EXCITE);
        let width = 1.0 + self.p.get(WIDTH) / 100.0;
        let bypass = self.p.on(BYPASS);
        let comp_out = if self.p.on(LOUDNESS_COMP) { 1.0 / self.gain.max(1e-6) } else { 1.0 };
        let (thr, ratio) = match character {
            0 => (-20.0, 1.5),
            1 => (-20.0, 1.6),
            2 => (-22.0, 2.5),
            _ => (-18.0, 2.0),
        };
        let n = left.len().min(right.len());
        let lat = self.dry.len();
        let mut di = 0usize;
        // the dry signal, delayed like the limiter, for Bypass
        let mut dry_out = Vec::new();
        if bypass {
            dry_out.reserve(n);
        }
        for i in 0..n {
            let (mut l, mut r) = (left[i], right[i]);
            if bypass {
                let slot = (di + i) % lat;
                dry_out.push(self.dry[slot]);
                self.dry[slot] = [l, r];
            }
            for k in 0..10 {
                l = self.auto[0][k].run(l);
                r = self.auto[1][k].run(r);
            }
            for k in 0..3 {
                l = self.eq[0][k].run(l);
                r = self.eq[1][k].run(r);
            }
            // the Character: its color EQ, then its compressor (Valve also saturates)
            for k in 0..2 {
                l = self.color[0][k].run(l);
                r = self.color[1][k].run(r);
            }
            let e = lg_dsp::gain_to_db(self.comp.run(l.abs().max(r.abs())).max(1e-6));
            let over = (e - thr).max(0.0);
            self.gr = flush(over * (1.0 - 1.0 / ratio));
            let g = db(-self.gr);
            l *= g;
            r *= g;
            if character == 1 {
                l = (l + 0.06 * l * l).tanh() * 0.99;
                r = (r + 0.06 * r * r).tanh() * 0.99;
            }
            if excite {
                for (s, v) in [&mut l, &mut r].into_iter().enumerate() {
                    let h0 = self.exc_hp[s][0].run(*v);
                    let h = (h0 * 4.0).tanh() / 4.0;
                    *v += self.exc_hp[s][1].run(h * h.abs() * 6.0 + h * 0.4) * 0.5;
                }
            }
            let mid = 0.5 * (l + r);
            let side = 0.5 * (l - r) * width;
            left[i] = (mid + side) * self.gain;
            right[i] = (mid - side) * self.gain;
        }
        di += n;
        let _ = di;
        self.lim.process(left, right);
        for i in 0..n {
            left[i] *= comp_out;
            right[i] *= comp_out;
            if bypass {
                left[i] = dry_out[i][0];
                right[i] = dry_out[i][1];
            }
            self.loud.push(left[i], right[i]);
            self.tp.push(left[i], right[i]);
            self.corr.push(left[i], right[i]);
            self.ring.push(left[i], right[i]);
        }
    }
    fn latency(&self) -> usize {
        self.lim.latency_samples()
    }
    fn gain_reduction(&self) -> f32 {
        self.gr + self.lim.gain_reduction()
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![
            ("True Peak", self.tp.now_db()),
            ("True Peak Max", self.tp.max_db()),
            ("Momentary", self.loud.momentary()),
            ("Short-Term", self.loud.short_term()),
            ("Integrated", self.loud.integrated()),
            ("LU Range", self.loud.range()),
            ("Correlation", self.corr.value()),
            ("Loudness Gain", self.loudness_gain_db()),
        ]
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        match name {
            "spectrum" => {
                let (l, r) = self.ring.ordered();
                let m: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
                spectrum(&m, self.rate, 63)
            }
            // the composite EQ curve (Auto EQ × amount, plus the Custom EQ) at 63 points
            "eq" => (0..63).map(|k| {
                let f = 20.0 * 1000f32.powf(k as f32 / 62.0);
                let a: f32 = self.auto[0].iter().map(|b| b.c.magnitude(self.rate, f)).product();
                let c: f32 = self.eq[0].iter().map(|b| b.c.magnitude(self.rate, f)).product();
                lg_dsp::gain_to_db(a * c)
            }).collect(),
            _ => Vec::new(),
        }
    }
}

impl Plugin for MasteringAssistant {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        if self.p.set(id, value) {
            self.update();
        }
    }
    fn reset(&mut self) {
        self.clear();
    }
    fn tail(&self) -> f32 {
        0.2
    }
    fn load_floats(&mut self, floats: &[f32]) {
        for d in self.p.defs {
            if let Some(v) = floats.get(d.id as usize) {
                self.p.set(d.id, *v);
            }
        }
        self.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, f: &[f32], a: f32) -> Vec<f32> {
        (0..n).map(|i| f.iter().map(|hz| (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin()).sum::<f32>() * a).collect()
    }

    #[test]
    fn analysis_corrects_a_dull_mix() {
        // lots of lows and mids, hardly any highs: the Auto EQ lifts the top and trims the bottom
        let x = tone(48000 * 4, &[60.0, 250.0, 700.0, 800.0], 0.1);
        let a = analyze(&x, &x, 48000.0);
        assert!(a.bands[8] > 1.0 && a.bands[9] > 1.0, "{:?}", a.bands);
        assert!(a.width > 0.0, "a mono mix is widened");
        assert!(a.lufs < -10.0 && a.lufs > -30.0, "{}", a.lufs);
    }

    #[test]
    fn loudness_knob_centre_reaches_about_minus_14() {
        let rate = 48000.0;
        let x: Vec<f32> = (0..48000 * 6).map(|i| {
            let t = i as f32 / rate;
            0.05 * ((std::f32::consts::TAU * 220.0 * t).sin() + 0.5 * (std::f32::consts::TAU * 1800.0 * t).sin() + 0.3 * (std::f32::consts::TAU * 5000.0 * t).sin())
        }).collect();
        let a = analyze(&x, &x, rate);
        let mut m = MasteringAssistant::new(rate);
        m.apply(&a);
        m.set(LOUDNESS, 0.0);
        m.set(AUTO_EQ, 0.0);
        m.set(WIDTH, 0.0);
        m.set(CHARACTER, 3.0);
        let mut l = x.clone();
        let mut r = x.clone();
        for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            m.process(cl, cr);
        }
        let mut meter = Loudness::new(rate);
        for (a, b) in l.iter().zip(&r).skip(48000) {
            meter.push(*a, *b);
        }
        let i = meter.integrated();
        assert!((i - (-14.0)).abs() < 2.0, "{i}");
        // and the limiter holds −1 dBTP
        let peak = l.iter().fold(0.0f32, |p, v| p.max(v.abs()));
        assert!(lg_dsp::gain_to_db(peak) < -0.8, "{}", lg_dsp::gain_to_db(peak));
    }

    #[test]
    fn bypass_passes_the_input() {
        let mut m = MasteringAssistant::new(48000.0);
        m.set(BYPASS, 1.0);
        let lat = m.latency();
        let x = tone(4800, &[440.0], 0.3);
        let mut l = x.clone();
        let mut r = x.clone();
        m.process(&mut l, &mut r);
        assert!((l[lat + 100] - x[100]).abs() < 1e-6);
    }

    #[test]
    fn auto_eq_amount_scales_the_curve() {
        let mut m = MasteringAssistant::new(48000.0);
        m.set(AUTO_BANDS + 7, 4.0);
        let full = m.curve("eq");
        m.set(AUTO_EQ, 50.0);
        let half = m.curve("eq");
        let k = (0..63).max_by(|a, b| full[*a].partial_cmp(&full[*b]).unwrap()).unwrap();
        assert!((full[k] - 4.0).abs() < 0.5 && (half[k] - 2.0).abs() < 0.4, "{} {}", full[k], half[k]);
    }
}
