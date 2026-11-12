//! Smaller dynamics and utility effects: DeEsser 2, Gain and Enveloper — stand-ins for Logic's,
//! on a project's float positions.
//!
//! - DeEsser 2 (Max Reduction, Threshold, Filter Solo, Detector Frequency, Range Split / Wide,
//!   Filter Mode, Threshold Mode Relative / Absolute): a band around the detector frequency is
//!   watched; Relative mode compares it with the whole signal (so loud and quiet passages are
//!   treated alike), Absolute with the threshold directly. Split turns down only that band,
//!   Wide the whole signal, by up to Max Reduction. Filter Solo plays the detector band.
//! - Gain (Gain, phase invert left / right, swap, mono): the gain sits at float 4 (confirmed);
//!   the switches' positions are unconfirmed.
//! - Enveloper (Attack Time / Gain, Release Time / Gain, Threshold, Lookahead, Out Level): a
//!   transient designer — Attack Gain boosts or cuts the rising part of each note, Release Gain
//!   its decay, above the threshold.

use crate::util::{Biquad, Coefs, Follower, Values, coef, db, flush, plugin_impl};
use lg_dsp::{Param, Unit, gain_to_db};

// ---------------------------------------------------------------------------------------------
// DeEsser 2

static DEESSER_PARAMS: [Param; 7] = [
    Param::new(0, "Max Reduction", 0.0, 40.0, 20.0, Unit::Db),
    Param::new(1, "Threshold", -60.0, 0.0, -9.5, Unit::Db),
    Param::new(2, "Filter Solo", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(3, "Detector Frequency", 800.0, 20000.0, 7001.34, Unit::Hz),
    Param::menu(4, "Range", &["Split", "Wide"], 0.0),
    Param::menu(5, "Filter Mode", &["Bandpass", "Highpass"], 0.0),
    Param::menu(6, "Threshold Mode", &["Relative", "Absolute"], 0.0),
];

pub struct DeEsser {
    p: Values,
    rate: f32,
    band: [Biquad; 2],
    band_env: Follower,
    full_env: Follower,
    gr: f32,
    att: f32,
    rel: f32,
}

impl DeEsser {
    pub fn new(rate: f32) -> DeEsser {
        let mut d = DeEsser { p: Values::new(&DEESSER_PARAMS), rate, band: [Biquad::default(); 2], band_env: Follower::new(rate, 0.5, 30.0), full_env: Follower::new(rate, 0.5, 30.0), gr: 0.0, att: coef(rate, 1.0), rel: coef(rate, 50.0) };
        d.update();
        d
    }
    fn update(&mut self) {
        let f = self.p.get(3).min(self.rate * 0.45);
        let c = if self.p.idx(5) == 1 { Coefs::highpass(self.rate, f, 0.707) } else { Coefs::bandpass(self.rate, f, 1.4) };
        self.band = [Biquad::new(c); 2];
    }
    fn clear(&mut self) {
        self.band[0].clear();
        self.band[1].clear();
        self.band_env.env = 0.0;
        self.full_env.env = 0.0;
        self.gr = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for DeEsser {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let max = p.get(0);
        let thr = p.get(1);
        let solo = p.on(2);
        let wide = p.idx(4) == 1;
        let relative = p.idx(6) == 0;
        for i in 0..left.len().min(right.len()) {
            let (l, r) = (left[i], right[i]);
            let (bl, br) = (self.band[0].run(l), self.band[1].run(r));
            let be = self.band_env.run(bl * bl + br * br);
            let fe = self.full_env.run(l * l + r * r);
            let band_db = 10.0 * be.max(1e-12).log10();
            // relative: the band's level against the whole signal's (0 dB = all sibilance)
            let level = if relative { band_db - 10.0 * fe.max(1e-12).log10() } else { band_db };
            let over = level - thr;
            let target = (over * 1.5).clamp(0.0, max);
            let k = if target > self.gr { self.att } else { self.rel };
            self.gr = flush(target + (self.gr - target) * k);
            let g = db(-self.gr);
            if solo {
                left[i] = bl;
                right[i] = br;
            } else if wide {
                left[i] = l * g;
                right[i] = r * g;
            } else {
                // split: only the detector band is turned down
                left[i] = l - bl * (1.0 - g);
                right[i] = r - br * (1.0 - g);
            }
        }
    }
}

plugin_impl!(DeEsser);

// ---------------------------------------------------------------------------------------------
// Gain

static GAIN_PARAMS: [Param; 6] = [
    Param::new(1, "Phase Invert Left", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(2, "Phase Invert Right", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(3, "Swap Left/Right", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(4, "Gain", -96.0, 24.0, 0.0, Unit::Db),
    Param::new(5, "Mono", 0.0, 1.0, 0.0, Unit::OnOff),
    // (our own id) left/right balance of the input: turning one side up turns the other down
    Param::new(1000, "Balance", -100.0, 100.0, 0.0, Unit::Percent),
];

pub struct Gain {
    p: Values,
    g: crate::util::Smooth,
}

impl Gain {
    pub fn new(rate: f32) -> Gain {
        let mut g = Gain { p: Values::new(&GAIN_PARAMS), g: crate::util::Smooth::new(rate, 10.0, 1.0) };
        g.update();
        g.g.snap();
        g
    }
    fn update(&mut self) {
        self.g.target = if self.p.get(4) <= -95.9 { 0.0 } else { db(self.p.get(4)) };
    }
    fn clear(&mut self) {
        self.g.snap();
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for Gain {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let (il, ir) = (if p.on(1) { -1.0 } else { 1.0 }, if p.on(2) { -1.0 } else { 1.0 });
        let (swap, mono) = (p.on(3), p.on(5));
        let b = p.get(1000) / 100.0;
        let (bl, br) = ((1.0 - b).min(1.0), (1.0 + b).min(1.0));
        for i in 0..left.len().min(right.len()) {
            let g = self.g.next();
            let (mut l, mut r) = (left[i] * il * bl, right[i] * ir * br);
            if swap {
                std::mem::swap(&mut l, &mut r);
            }
            if mono {
                let m = 0.5 * (l + r);
                l = m;
                r = m;
            }
            left[i] = l * g;
            right[i] = r * g;
        }
    }
}

impl lg_dsp::Plugin for Gain {
    fn params(&self) -> &[lg_dsp::Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        // like Logic's buttons: turning Mono on turns Swap Left/Right off
        if id == 5 && value > 0.5 {
            self.p.set(3, 0.0);
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
    fn reset(&mut self) {
        self.clear();
    }
    fn tail(&self) -> f32 {
        self.tail_secs()
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

// ---------------------------------------------------------------------------------------------
// Enveloper

static ENV_PARAMS: [Param; 7] = [
    Param::new(0, "Attack Time", 0.0, 200.0, 20.0, Unit::Ms),
    Param::new(1, "Attack Gain", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(2, "Release Time", 1.0, 5000.0, 200.0, Unit::Ms),
    Param::new(3, "Release Gain", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(4, "Threshold", -100.0, 0.0, -100.0, Unit::Db),
    Param::new(5, "Lookahead", 0.0, 20.0, 1.0, Unit::Ms),
    Param::new(6, "Out Level", -40.0, 20.0, 0.0, Unit::Db),
];

pub struct Enveloper {
    p: Values,
    rate: f32,
    fast: Follower,
    attack: Follower,
    release: Follower,
    delay: [Vec<f32>; 2],
    pos: usize,
    look: usize,
}

impl Enveloper {
    pub fn new(rate: f32) -> Enveloper {
        let cap = (0.021 * rate) as usize + 2;
        let mut e = Enveloper { p: Values::new(&ENV_PARAMS), rate, fast: Follower::default(), attack: Follower::default(), release: Follower::default(), delay: [vec![0.0; cap], vec![0.0; cap]], pos: 0, look: 0 };
        e.update();
        e
    }
    pub fn latency_samples(&self) -> usize {
        self.look
    }
    fn update(&mut self) {
        let (r, p) = (self.rate, &self.p);
        // the "now" follower; the attack follower lags by the attack time (their difference is
        // the rising part); the release follower holds on for the release time (its excess is
        // the decaying part)
        self.fast.set(r, 0.3, 10.0);
        self.attack.set(r, p.get(0).max(0.5), 10.0);
        self.release.set(r, 0.3, p.get(2).max(1.0));
        self.look = ((p.get(5) * 0.001 * r) as usize).min(self.delay[0].len() - 1);
    }
    fn clear(&mut self) {
        self.fast.env = 0.0;
        self.attack.env = 0.0;
        self.release.env = 0.0;
        for d in self.delay.iter_mut() {
            d.iter_mut().for_each(|x| *x = 0.0);
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for Enveloper {
    fn latency(&self) -> usize {
        self.latency_samples()
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let (ag, rg) = (p.get(1) / 100.0, p.get(3) / 100.0);
        let thr = db(p.get(4));
        let out = db(p.get(6));
        let cap = self.delay[0].len();
        for i in 0..left.len().min(right.len()) {
            let a = left[i].abs().max(right[i].abs());
            let f = self.fast.run(a);
            let at = self.attack.run(a);
            let re = self.release.run(a);
            let mut g_db = 0.0;
            if f > thr {
                // up to ±24 dB on the attack and the release parts
                let rise = (gain_to_db(f.max(1e-9)) - gain_to_db(at.max(1e-9))).max(0.0);
                g_db += ag * rise.min(24.0);
            }
            if re > thr {
                let fall = (gain_to_db(re.max(1e-9)) - gain_to_db(f.max(1e-9))).max(0.0);
                g_db += rg * fall.min(24.0);
            }
            let g = db(g_db.clamp(-60.0, 24.0)) * out;
            let rd = (self.pos + cap - self.look) % cap;
            self.delay[0][self.pos] = left[i];
            self.delay[1][self.pos] = right[i];
            let (l, r) = (self.delay[0][rd], self.delay[1][rd]);
            self.pos = (self.pos + 1) % cap;
            left[i] = l * g;
            right[i] = r * g;
        }
    }
}

plugin_impl!(Enveloper);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    #[test]
    fn deesser_cuts_sibilance_not_voice() {
        let rate = 48000.0;
        let tone = |f: f32, a: f32| -> Vec<f32> { (0..24000).map(|i| a * (crate::util::TAU * f * i as f32 / rate).sin()).collect() };
        let rms = |v: &[f32]| (v[12000..].iter().map(|x| x * x).sum::<f32>() / 12000.0).sqrt();
        let mut d = DeEsser::new(rate);
        let mut s = tone(7000.0, 0.3);
        let s0 = rms(&s);
        let mut s2 = s.clone();
        d.process(&mut s, &mut s2);
        assert!(rms(&s) < s0 * 0.5, "sibilance {} vs {}", rms(&s), s0);
        let mut d = DeEsser::new(rate);
        let mut v = tone(300.0, 0.3);
        let v0 = rms(&v);
        let mut v2 = v.clone();
        d.process(&mut v, &mut v2);
        assert!((rms(&v) / v0 - 1.0).abs() < 0.05);
    }

    #[test]
    fn gain_is_exact() {
        let mut g = Gain::new(48000.0);
        g.set(4, -6.0);
        g.reset();
        let mut l = vec![1.0; 10];
        let mut r = vec![1.0; 10];
        g.process(&mut l, &mut r);
        assert!((l[5] - db(-6.0)).abs() < 1e-6);
    }

    #[test]
    fn gain_balance_and_mono_turns_swap_off() {
        let mut g = Gain::new(48000.0);
        g.set(1000, 50.0);
        let mut l = vec![1.0; 10];
        let mut r = vec![1.0; 10];
        g.process(&mut l, &mut r);
        assert!((l[5] - 0.5).abs() < 1e-6 && (r[5] - 1.0).abs() < 1e-6);
        g.set(1000, -100.0);
        let (mut l, mut r) = (vec![1.0; 10], vec![1.0; 10]);
        g.process(&mut l, &mut r);
        assert!((l[5] - 1.0).abs() < 1e-6 && r[5].abs() < 1e-6);
        g.set(3, 1.0);
        assert!(g.p.on(3));
        g.set(5, 1.0);
        assert!(!g.p.on(3) && g.p.on(5));
    }
}
