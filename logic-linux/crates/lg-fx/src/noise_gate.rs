//! Noise Gate: the stand-in for Logic's Noise Gate.
//!
//! Project floats: Threshold, Hysteresis, Reduction, Attack, Hold, Release, High Cutoff, Low Cutoff,
//! Lookahead, Monitor. The key signal goes through the side-chain band (Low Cutoff high-pass,
//! High Cutoff low-pass) to a peak detector. The gate opens when the key passes Threshold and
//! closes only when it falls below Threshold + Hysteresis (hysteresis is negative), after Hold.
//! Opening ramps over Attack, closing over Release, down to Reduction (-100 dB = silence).
//! Lookahead delays the audio so the gate is open before a transient arrives; Monitor plays the
//! filtered key instead. Reduction runs up to +20 dB (a boost). The newer versions' Ducker mode
//! floats (10-14) aren't decoded; Mode, Filter and Characteristics use our own ids.

use crate::util::{Biquad, Coefs, Values, coef, db, plugin_impl};
use lg_dsp::{Param, Unit};

static PARAMS: [Param; 13] = [
    Param::new(0, "Threshold", -100.0, 0.0, -50.0, Unit::Db),
    Param::new(1, "Hysteresis", -20.0, 0.0, -3.0, Unit::Db),
    Param::new(2, "Reduction", -100.0, 20.0, -100.0, Unit::Db),
    Param::new(3, "Attack", 0.0, 200.0, 3.0, Unit::Ms),
    Param::new(4, "Hold", 0.0, 2000.0, 40.0, Unit::Ms),
    Param::new(5, "Release", 2.0, 10000.0, 10.0, Unit::Ms),
    Param::new(6, "High Cutoff", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(7, "Low Cutoff", 20.0, 20000.0, 20.0, Unit::Hz),
    Param::new(8, "Lookahead", 0.0, 10.0, 0.0, Unit::Ms),
    Param::new(9, "Monitor", 0.0, 1.0, 0.0, Unit::OnOff),
    // (our own id: Logic's Ducker-mode floats 10-14 aren't decoded)
    Param::menu(1000, "Mode", &["Noise Gate", "Ducker"], 0.0),
    // (our own ids: the side-chain filter's on/off button and its Characteristics pop-up)
    Param::new(1001, "Filter", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(1002, "Characteristics", &["Bandpass", "Band Reject"], 0.0),
];

pub struct NoiseGate {
    p: Values,
    rate: f32,
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    peak: f32,
    peak_rel: f32,
    open: bool,
    hold_left: usize,
    hold: usize,
    gain: f32,
    att_step: f32,
    rel_k: f32,
    floor: f32,
    thr_open: f32,
    thr_close: f32,
    delay: [Vec<f32>; 2],
    dpos: usize,
    look: usize,
    side: Option<(Vec<f32>, Vec<f32>)>,
}

impl NoiseGate {
    pub fn new(rate: f32) -> NoiseGate {
        let cap = (0.011 * rate) as usize + 4;
        let mut g = NoiseGate {
            p: Values::new(&PARAMS),
            rate,
            hp: [Biquad::default(); 2],
            lp: [Biquad::default(); 2],
            peak: 0.0,
            peak_rel: coef(rate, 5.0),
            open: false,
            hold_left: 0,
            hold: 0,
            gain: 0.0,
            att_step: 1.0,
            rel_k: 0.0,
            floor: 0.0,
            thr_open: 0.0,
            thr_close: 0.0,
            delay: [vec![0.0; cap], vec![0.0; cap]],
            dpos: 0,
            look: 0,
            side: None,
        };
        g.update();
        g.gain = g.floor;
        g
    }

    pub fn latency_samples(&self) -> usize {
        self.look
    }

    fn update(&mut self) {
        let p = &self.p;
        let r = self.rate;
        let hc = p.get(6);
        let lc = p.get(7);
        let lpc = if hc >= 19999.0 { Coefs::THRU } else { Coefs::lowpass(r, hc, 0.707) };
        let hpc = if lc <= 20.0 { Coefs::THRU } else { Coefs::highpass(r, lc, 0.707) };
        for i in 0..2 {
            self.lp[i].c = lpc;
            self.hp[i].c = hpc;
        }
        self.thr_open = db(p.get(0));
        self.thr_close = db(p.get(0) + p.get(1));
        self.floor = if p.get(2) <= -99.9 { 0.0 } else { db(p.get(2)) };
        // the attack is a linear ramp from the floor to unity
        self.att_step = 1.0 / (p.get(3).max(0.02) * 0.001 * r);
        // the release falls exponentially (about -60 dB over the release time)
        self.rel_k = (-6.9 / (p.get(5).max(1.0) * 0.001 * r)).exp();
        self.hold = (p.get(4) * 0.001 * r) as usize;
        let look = ((p.get(8) * 0.001 * r) as usize).min(self.delay[0].len() - 1);
        if look != self.look {
            self.look = look;
            for d in self.delay.iter_mut() {
                d.iter_mut().for_each(|x| *x = 0.0);
            }
        }
    }

    fn clear(&mut self) {
        for i in 0..2 {
            self.hp[i].clear();
            self.lp[i].clear();
        }
        self.peak = 0.0;
        self.open = false;
        self.hold_left = 0;
        self.gain = self.floor;
        for d in self.delay.iter_mut() {
            d.iter_mut().for_each(|x| *x = 0.0);
        }
    }

    fn tail_secs(&self) -> f32 {
        (self.p.get(4) + self.p.get(5)) * 0.001
    }
}

impl lg_dsp::Effect for NoiseGate {
    /// Whether the gate is open now (the window's Open / Close lights).
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Open", if self.open { 1.0 } else { 0.0 })]
    }

    fn latency(&self) -> usize {
        self.latency_samples()
    }

    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = Some((left.to_vec(), right.to_vec()));
    }
    fn has_side_chain(&self) -> bool {
        true
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let monitor = self.p.on(9);
        let cap = self.delay[0].len();
        let side = self.side.take();
        // Ducker: the key (a voice on the side chain) turns the signal down by Reduction, and
        // the side chain is mixed into the output after processing
        let ducker = self.p.idx(1000) == 1;
        // the side-chain filter: off = the key as it comes; Band Reject = what the band leaves out
        let (filter, reject) = (self.p.on(1001), self.p.idx(1002) == 1);
        for i in 0..n {
            let (xl, xr) = match &side {
                Some((a, b)) => (a.get(i).copied().unwrap_or(0.0), b.get(i).copied().unwrap_or(0.0)),
                None => (left[i], right[i]),
            };
            let (bl, br) = (self.lp[0].run(self.hp[0].run(xl)), self.lp[1].run(self.hp[1].run(xr)));
            let (kl, kr) = match (filter, reject) {
                (false, _) => (xl, xr),
                (true, false) => (bl, br),
                (true, true) => (xl - bl, xr - br),
            };
            let a = kl.abs().max(kr.abs());
            self.peak = if a > self.peak { a } else { a + (self.peak - a) * self.peak_rel };
            if self.peak >= self.thr_open {
                self.open = true;
                self.hold_left = self.hold;
            } else if self.open && self.peak < self.thr_close {
                if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            } else if self.open {
                self.hold_left = self.hold;
            }
            if self.open {
                // ramp to unity (from below, or down from a boosting Reduction above 0 dB)
                self.gain = if self.gain < 1.0 { (self.gain + self.att_step).min(1.0) } else { (self.gain - self.att_step * self.floor.max(1.0)).max(1.0) };
            } else {
                self.gain = self.floor + (self.gain - self.floor) * self.rel_k;
                if (self.gain - self.floor).abs() < 1e-7 {
                    self.gain = self.floor;
                }
            }
            if monitor {
                left[i] = kl;
                right[i] = kr;
                continue;
            }
            let (l, r) = if self.look > 0 {
                let rd = (self.dpos + cap - self.look) % cap;
                self.delay[0][self.dpos] = left[i];
                self.delay[1][self.dpos] = right[i];
                let v = (self.delay[0][rd], self.delay[1][rd]);
                self.dpos = (self.dpos + 1) % cap;
                v
            } else {
                (left[i], right[i])
            };
            if ducker {
                // (the gate's gain inverted: open key = the floor, closed = unity)
                let g = 1.0 + self.floor - self.gain;
                left[i] = l * g + if side.is_some() { xl } else { 0.0 };
                right[i] = r * g + if side.is_some() { xr } else { 0.0 };
                continue;
            }
            left[i] = l * self.gain;
            right[i] = r * self.gain;
        }
    }
}

plugin_impl!(NoiseGate);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    #[test]
    fn ducker_mode_turns_down_under_the_key() {
        let mut g = NoiseGate::new(48000.0);
        g.set(0, -30.0);
        g.set(2, -20.0);
        g.set(1000, 1.0);
        let key: Vec<f32> = (0..4800).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let mut l = vec![0.2f32; 4800];
        let mut r = l.clone();
        g.side_chain(&key, &key);
        g.process(&mut l, &mut r);
        // music at −20 dB plus the voice
        assert!((l[4000] - key[4000] - 0.02).abs() < 0.005, "{}", l[4000] - key[4000]);
        // no key: the music passes
        let mut l = vec![0.2f32; 9600];
        let mut r = l.clone();
        g.side_chain(&vec![0.0; 9600], &vec![0.0; 9600]);
        g.process(&mut l, &mut r);
        assert!((l[9500] - 0.2).abs() < 0.01);
    }

    #[test]
    fn reduction_can_boost_and_filter_characteristics() {
        // Reduction +20 dB: the signal under the threshold is boosted, not cut
        let mut g = NoiseGate::new(48000.0);
        g.set(0, -30.0);
        g.set(2, 20.0);
        assert_eq!(g.get(2), 20.0);
        let mut q: Vec<f32> = (0..9600).map(|i| 0.005 * (i as f32 * 0.1).sin()).collect();
        let orig = q.clone();
        let mut q2 = q.clone();
        g.process(&mut q, &mut q2);
        let r = q[8000..].iter().map(|x| x.abs()).fold(0.0, f32::max) / orig[8000..].iter().map(|x| x.abs()).fold(0.0, f32::max);
        assert!((r - 10.0).abs() < 0.3, "{r}");
        // a loud key opens it back to unity
        let mut l: Vec<f32> = (0..9600).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let o = l.clone();
        let mut l2 = l.clone();
        g.process(&mut l, &mut l2);
        assert!((l[9000] - o[9000]).abs() < 1e-4);
        // key = 4 kHz tone; Bandpass 100-1000 Hz hides it (gate stays shut), Band Reject sees it,
        // Filter off sees it
        let tone: Vec<f32> = (0..9600).map(|i| 0.5 * (std::f32::consts::TAU * 4000.0 * i as f32 / 48000.0).sin()).collect();
        let level = |set: &[(u32, f32)]| {
            let mut g = NoiseGate::new(48000.0);
            g.set(0, -20.0);
            g.set(6, 1000.0);
            g.set(7, 100.0);
            for &(id, v) in set {
                g.set(id, v);
            }
            let mut a = tone.clone();
            let mut b = tone.clone();
            g.process(&mut a, &mut b);
            a[8000..].iter().map(|x| x.abs()).fold(0.0, f32::max)
        };
        assert!(level(&[]) < 0.01);
        assert!(level(&[(1002, 1.0)]) > 0.4);
        assert!(level(&[(1001, 0.0)]) > 0.4);
    }

    #[test]
    fn gates_quiet_passes_loud() {
        let mut g = NoiseGate::new(48000.0);
        g.set(0, -30.0);
        let mut quiet: Vec<f32> = (0..4800).map(|i| 0.005 * (i as f32 * 0.1).sin()).collect();
        let mut q2 = quiet.clone();
        g.process(&mut quiet, &mut q2);
        assert!(quiet[4000..].iter().all(|x| x.abs() < 1e-6));
        let mut loud: Vec<f32> = (0..4800).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let mut l2 = loud.clone();
        let orig = loud.clone();
        g.process(&mut loud, &mut l2);
        assert!((loud[3000] - orig[3000]).abs() < 1e-5);
    }

    #[test]
    fn the_side_chain_filter_band_or_its_reject() {
        // a 100 Hz key, above the threshold: the band 1–20 kHz keeps it out (gate shut),
        // Band Reject lets it in (gate open), the filter off lets it in
        let tone: Vec<f32> = (0..9600).map(|i| 0.5 * (i as f32 * std::f32::consts::TAU * 100.0 / 48000.0).sin()).collect();
        let run = |filter: f32, reject: f32| {
            let mut g = NoiseGate::new(48000.0);
            g.set(0, -30.0);
            g.set(7, 1000.0);
            g.set(1001, filter);
            g.set(1002, reject);
            let (mut l, mut r) = (tone.clone(), tone.clone());
            g.process(&mut l, &mut r);
            l[8000..].iter().fold(0f32, |m, x| m.max(x.abs()))
        };
        assert!(run(1.0, 0.0) < 0.01);
        assert!(run(1.0, 1.0) > 0.4);
        assert!(run(0.0, 0.0) > 0.4);
    }
}
