//! Multipressor: the stand-in for Logic's four-band compressor / expander.
//!
//! The signal splits at three crossovers (Linkwitz-Riley 24 dB/oct, the lower bands phase-
//! matched with all-passes so the bands sum flat). Each band has a downward expander (Exp.
//! Threshold, Exp. Ratio, its depth limited by Reduction) and a compressor (Threshold, Ratio,
//! Attack, Release, Make Up), a level detector with its own Response time, and Bypass / Solo.
//! Then Output Gain, Auto Gain and a peak limiter.
//!
//! Positions (best effort from the research map, which pins only some rows): float 2 and the
//! eleven-float band records that follow are stored from the TOP band down — [Response,
//! Reduction, Exp. Threshold, Exp. Ratio, Threshold, Ratio, Make Up, Attack, Release, On], the
//! crossover between two bands stored between their records (2 = 3/4, 13 = 2/3, 24 = 1/2);
//! Auto Gain 49, Bypass 50-53 and Solo 54-57 (bands 4..1) are value-confirmed. Lookahead (0),
//! Output Gain (46), Peak Limit (47/48) are unconfirmed.

use crate::util::{Biquad, Coefs, Values, coef, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

/// Record start of band k (0 = lowest), and its crossover above it.
const BAND_AT: [u32; 4] = [36, 25, 14, 3];
const XOVER_AT: [u32; 3] = [24, 13, 2];

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![Param::new(0, "Lookahead", 0.0, 20.0, 3.0, Unit::Ms)];
    let names = ["Band 1", "Band 2", "Band 3", "Band 4"];
    let leak = |b: &str, w: &str| -> &'static str { Box::leak(format!("{b} {w}").into_boxed_str()) };
    let defaults = [(200.0, -18.0, 2.5111, 61.0), (100.0, -15.0, 2.0222, 49.0), (100.0, -15.0, 2.0222, 49.0), (50.0, -12.0, 2.0222, 49.0)];
    for k in 0..4 {
        let at = BAND_AT[k];
        let (resp, thr, ratio, rel) = defaults[k];
        let b = names[k];
        v.extend([
            Param::new(at, leak(b, "Response"), 1.0, 1000.0, resp, Unit::Ms),
            Param::new(at + 1, leak(b, "Reduction"), -100.0, 0.0, 0.0, Unit::Db),
            Param::new(at + 2, leak(b, "Exp. Threshold"), -100.0, 0.0, -50.0, Unit::Db),
            Param::new(at + 3, leak(b, "Exp. Ratio"), 1.0, 10.0, 1.0, Unit::Ratio),
            Param::new(at + 4, leak(b, "Comp. Threshold"), -60.0, 0.0, thr, Unit::Db),
            Param::new(at + 5, leak(b, "Comp. Ratio"), 1.0, 30.0, ratio, Unit::Ratio),
            Param::new(at + 6, leak(b, "Make Up"), -24.0, 24.0, 0.0, Unit::Db),
            Param::new(at + 7, leak(b, "Attack"), 0.0, 200.0, 2.0, Unit::Ms),
            Param::new(at + 8, leak(b, "Release"), 2.0, 5000.0, rel, Unit::Ms),
            Param::new(at + 9, leak(b, "On"), 0.0, 1.0, 1.0, Unit::OnOff),
        ]);
    }
    let xd = [160.0, 1100.0, 7500.0];
    let xn = ["Xover Frequency 1/2", "Xover Frequency 2/3", "Xover Frequency 3/4"];
    for k in 0..3 {
        v.push(Param::new(XOVER_AT[k], xn[k], 20.0, 20000.0, xd[k], Unit::Hz));
    }
    v.extend([
        Param::new(46, "Output Gain", -30.0, 30.0, 0.0, Unit::Db),
        Param::new(47, "Peak Limit", -30.0, 0.0, 0.0, Unit::Db),
        Param::new(48, "Peak Limit On", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(49, "Auto Gain", 0.0, 1.0, 0.0, Unit::OnOff),
    ]);
    for k in 0..4 {
        // stored band 4 first
        v.push(Param::new(50 + (3 - k) as u32, leak(names[k], "Bypass"), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(54 + (3 - k) as u32, leak(names[k], "Solo"), 0.0, 1.0, 0.0, Unit::OnOff));
    }
    v
});

#[derive(Clone, Copy, Default)]
struct Band {
    ms: f32,
    gr: f32,
    resp: f32,
    att: f32,
    rel: f32,
    thr: f32,
    slope: f32,
    ethr: f32,
    eslope: f32,
    depth: f32,
    makeup: f32,
}

pub struct Multipressor {
    p: Values,
    rate: f32,
    /// per channel: [lp, hp] × 2 sections for each crossover, and the all-passes
    lp: [[[Biquad; 2]; 3]; 2],
    hp: [[[Biquad; 2]; 3]; 2],
    ap: [[Biquad; 3]; 2],
    bands: [Band; 4],
    out: f32,
    lim: f32,
    lim_g: f32,
    lim_rel: f32,
}

impl Multipressor {
    pub fn new(rate: f32) -> Multipressor {
        let mut m = Multipressor {
            p: Values::new(&PARAMS),
            rate,
            lp: [[[Biquad::default(); 2]; 3]; 2],
            hp: [[[Biquad::default(); 2]; 3]; 2],
            ap: [[Biquad::default(); 3]; 2],
            bands: [Band::default(); 4],
            out: 1.0,
            lim: 1.0,
            lim_g: 1.0,
            lim_rel: coef(rate, 80.0),
        };
        m.update();
        m
    }

    fn update(&mut self) {
        let (r, p) = (self.rate, &self.p);
        let mut xf = [p.get(XOVER_AT[0]), p.get(XOVER_AT[1]), p.get(XOVER_AT[2])];
        // keep the crossovers in order
        xf[1] = xf[1].max(xf[0] * 1.05);
        xf[2] = xf[2].max(xf[1] * 1.05).min(r * 0.45);
        for s in 0..2 {
            for k in 0..3 {
                let (l, h) = (Coefs::lowpass(r, xf[k], 0.7071), Coefs::highpass(r, xf[k], 0.7071));
                for j in 0..2 {
                    self.lp[s][k][j].c = l;
                    self.hp[s][k][j].c = h;
                }
            }
            // band 1 needs the 2/3 and 3/4 all-passes, band 2 the 3/4 one
            self.ap[s][0].c = Coefs::allpass(r, xf[1], 0.7071);
            self.ap[s][1].c = Coefs::allpass(r, xf[2], 0.7071);
            self.ap[s][2].c = Coefs::allpass(r, xf[2], 0.7071);
        }
        let mut auto = 0.0f32;
        for k in 0..4 {
            let at = BAND_AT[k];
            let b = &mut self.bands[k];
            b.resp = coef(r, p.get(at).max(1.0) * 0.2);
            b.att = coef(r, p.get(at + 7).max(0.05));
            b.rel = coef(r, p.get(at + 8));
            b.ethr = p.get(at + 2);
            b.eslope = p.get(at + 3) - 1.0;
            b.depth = -p.get(at + 1);
            b.thr = p.get(at + 4);
            b.slope = 1.0 - 1.0 / p.get(at + 5).max(1.0);
            b.makeup = p.get(at + 6);
            // auto gain: make up a quarter of what a full-scale band would lose
            auto = auto.max(b.slope * (-b.thr) * 0.25);
        }
        let ag = if p.on(49) { auto } else { 0.0 };
        self.out = db(p.get(46) + ag);
        self.lim = if p.on(48) { db(p.get(47)) } else { f32::INFINITY };
    }

    fn clear(&mut self) {
        for s in 0..2 {
            for k in 0..3 {
                for b in self.lp[s][k].iter_mut().chain(self.hp[s][k].iter_mut()) {
                    b.clear();
                }
                self.ap[s][k].clear();
            }
        }
        for b in self.bands.iter_mut() {
            b.ms = 0.0;
            b.gr = 0.0;
        }
        self.lim_g = 1.0;
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }

    /// Splits one channel's sample into four bands (low to high).
    #[inline]
    fn split(&mut self, s: usize, x: f32) -> [f32; 4] {
        let lr = |f: &mut [Biquad; 2], v: f32| -> f32 {
            let a = f[0].run(v);
            f[1].run(a)
        };
        let b1 = lr(&mut self.lp[s][0], x);
        let rest = lr(&mut self.hp[s][0], x);
        let b2 = lr(&mut self.lp[s][1], rest);
        let rest = lr(&mut self.hp[s][1], rest);
        let b3 = lr(&mut self.lp[s][2], rest);
        let b4 = lr(&mut self.hp[s][2], rest);
        // phase-match the lower bands to the later crossovers (so the four sum to an all-pass)
        let a = self.ap[s][0].run(b1);
        let b1 = self.ap[s][1].run(a);
        let b2 = self.ap[s][2].run(b2);
        [b1, b2, b3, b4]
    }
}

impl lg_dsp::Effect for Multipressor {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let on: [bool; 4] = std::array::from_fn(|k| p.on(BAND_AT[k] + 9) && !p.on(50 + (3 - k) as u32));
        let solo: [bool; 4] = std::array::from_fn(|k| p.on(54 + (3 - k) as u32));
        let any_solo = solo.iter().any(|s| *s);
        for i in 0..left.len().min(right.len()) {
            let bl = self.split(0, left[i]);
            let br = self.split(1, right[i]);
            let (mut yl, mut yr) = (0.0, 0.0);
            for k in 0..4 {
                if any_solo && !solo[k] {
                    continue;
                }
                let b = &mut self.bands[k];
                let pw = bl[k] * bl[k] + br[k] * br[k];
                b.ms = flush(pw * 0.5 + (b.ms - pw * 0.5) * b.resp);
                let lvl = 10.0 * b.ms.max(1e-12).log10() + 3.0;
                let mut target = if lvl > b.thr { b.slope * (lvl - b.thr) } else { 0.0 };
                if lvl < b.ethr && b.depth > 0.0 {
                    target += ((b.ethr - lvl) * b.eslope).min(b.depth);
                }
                let k2 = if target > b.gr { b.att } else { b.rel };
                b.gr = flush(target + (b.gr - target) * k2);
                let g = if on[k] { db(b.makeup - b.gr) } else { 1.0 };
                yl += bl[k] * g;
                yr += br[k] * g;
            }
            yl *= self.out;
            yr *= self.out;
            let pk = yl.abs().max(yr.abs());
            if pk * self.lim_g > self.lim {
                self.lim_g = self.lim / pk;
            } else {
                self.lim_g = 1.0 + (self.lim_g - 1.0) * self.lim_rel;
            }
            left[i] = yl * self.lim_g;
            right[i] = yr * self.lim_g;
        }
    }
}

plugin_impl!(Multipressor);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    /// With every band neutral the bands sum back to the input's level at any frequency.
    #[test]
    fn neutral_bands_sum_flat() {
        let mut m = Multipressor::new(48000.0);
        for k in 0..4 {
            m.set(BAND_AT[k] + 5, 1.0);
        }
        for f in [60.0, 400.0, 3000.0, 12000.0] {
            let x: Vec<f32> = (0..24000).map(|i| 0.2 * (crate::util::TAU * f * i as f32 / 48000.0).sin()).collect();
            let (mut l, mut r) = (x.clone(), x.clone());
            m.reset();
            m.process(&mut l, &mut r);
            let rms = |v: &[f32]| (v[12000..].iter().map(|a| a * a).sum::<f32>() / 12000.0).sqrt();
            let g = lg_dsp::gain_to_db(rms(&l) / rms(&x));
            assert!(g.abs() < 0.3, "{f} Hz: {g}");
        }
    }
}
