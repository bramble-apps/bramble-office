//! Delay Designer (its first tap) and the Mastering Assistant's processing chain.
//!
//! Delay Designer: Logic's multi-tap delay. Taps are variable-length records in the float
//! block whose layout beyond the first tap is not decoded yet, so this plays Tap A only: its
//! delay (ms, or snapped to the Grid when Sync is on), its low / high cut, the feedback from Tap A
//! (level in dB) and the Original / Effect mix — enough to sound right for the default and
//! single-tap settings.
//!
//! Mastering Assistant: the chain it builds — Auto EQ / Custom EQ (three bands: low shelf,
//! peak, high shelf), the Character stage (Clean: transparent; Valve: tube saturation; Punch:
//! a slow glue compressor that keeps transients; Transparent: none), Excite (high harmonics),
//! Stereo Width and a loudness gain into a true-peak-safe limiter at -1 dBTP. The Auto EQ curve
//! itself (a 512-point frequency grid follows in the block) is not decoded, so the Auto EQ
//! amount is not applied; the Custom EQ is.

use crate::limiter::Limiter;
use crate::util::{Biquad, Coefs, Delay, Follower, Smooth, Values, db, flush, plugin_impl};
use lg_dsp::{Effect, Param, Plugin, Unit};

// ---------------------------------------------------------------------------------------------
// Delay Designer

static DD_PARAMS: [Param; 11] = [
    Param::new(0, "Grid", 0.015625, 1.0, 0.0625, Unit::None),
    Param::new(1, "Swing", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(3, "Feedback", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(4, "Feedback Level", -60.0, 0.0, -6.0, Unit::Db),
    Param::new(10, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(21, "Tap A Delay", 1.0, 10000.0, 1000.0, Unit::Ms),
    Param::new(26, "Mix Original", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(28, "Mix Effect", 0.0, 100.0, 90.0, Unit::Percent),
    Param::new(34, "Tap A Low Cut", 10.0, 20000.0, 19.711, Unit::Hz),
    Param::new(35, "Tap A High Cut", 10.0, 20480.0, 20290.0, Unit::Hz),
    Param::new(29, "Tap A Level", 0.0, 100.0, 70.0, Unit::Percent),
];

pub struct DelayDesigner {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: Smooth,
    lc: [Biquad; 2],
    hc: [Biquad; 2],
}

impl DelayDesigner {
    pub fn new(rate: f32) -> DelayDesigner {
        let mut d = DelayDesigner { p: Values::new(&DD_PARAMS), rate, bpm: 120.0, line: [Delay::new((10.1 * rate) as usize), Delay::new((10.1 * rate) as usize)], time: Smooth::new(rate, 50.0, 1.0), lc: [Biquad::default(); 2], hc: [Biquad::default(); 2] };
        d.update();
        d.time.snap();
        d
    }
    fn update(&mut self) {
        let p = &self.p;
        let mut ms = p.get(21);
        if p.on(10) {
            // snap to the grid (Grid is a share of a whole note)
            let grid_ms = p.get(0) * 4.0 * 60000.0 / self.bpm as f32;
            ms = ((ms / grid_ms).round().max(1.0)) * grid_ms;
        }
        self.time.target = (ms * 0.001 * self.rate).clamp(1.0, 10.0 * self.rate);
        let (lc, hc) = (p.get(34), p.get(35));
        for s in 0..2 {
            self.lc[s].c = if lc <= 20.0 { Coefs::THRU } else { Coefs::highpass(self.rate, lc, 0.707) };
            self.hc[s].c = if hc >= 20000.0 { Coefs::THRU } else { Coefs::lowpass(self.rate, hc.min(self.rate * 0.45), 0.707) };
        }
    }
    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.lc[s].clear();
            self.hc[s].clear();
        }
        self.time.snap();
    }
    fn tail_secs(&self) -> f32 {
        let t = self.time.target / self.rate;
        if self.p.on(3) { t * (1.0 + 60.0 / (-self.p.get(4)).max(0.5)).min(60.0) } else { t + 0.1 }
    }
}

impl Effect for DelayDesigner {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let fb = if self.p.on(3) { db(self.p.get(4)).min(0.98) } else { 0.0 };
        let (orig, eff, lvl) = (self.p.get(26) / 100.0, self.p.get(28) / 100.0, self.p.get(29) / 100.0);
        for i in 0..left.len().min(right.len()) {
            let t = self.time.next();
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let y = self.hc[s].run(self.lc[s].run(self.line[s].read(t)));
                self.line[s].push(*x + y * fb);
                *x = *x * orig + y * lvl * eff;
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
        self.update();
    }
}

plugin_impl!(DelayDesigner);

// ---------------------------------------------------------------------------------------------
// Mastering Assistant

const CHARACTERS: &[&str] = &["Clean", "Valve", "Punch", "Transparent"];

static MA_PARAMS: [Param; 13] = [
    Param::menu(1, "Character", CHARACTERS, 0.0),
    Param::new(13, "Custom EQ", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(14, "Low Gain", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(15, "Low Freq", 20.0, 500.0, 100.0, Unit::Hz),
    Param::new(16, "Mid Gain", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(17, "Mid Freq", 200.0, 8000.0, 998.9, Unit::Hz),
    Param::new(18, "High Gain", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(19, "High Freq", 2000.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(20, "Excite", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(21, "Stereo Width", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(24, "Loudness", -12.0, 18.0, -0.17, Unit::Db),
    Param::new(25, "Auto EQ", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(5, "Loudness Compensation", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct MasteringAssistant {
    p: Values,
    rate: f32,
    eq: [[Biquad; 3]; 2],
    exc_hp: [[Biquad; 2]; 2],
    glue: Follower,
    glue_gr: f32,
    lim: Limiter,
    gain: f32,
}

impl MasteringAssistant {
    pub fn new(rate: f32) -> MasteringAssistant {
        let mut lim = Limiter::new(rate);
        lim.set(4, -1.0);
        lim.set(3, 120.0);
        let mut m = MasteringAssistant { p: Values::new(&MA_PARAMS), rate, eq: [[Biquad::default(); 3]; 2], exc_hp: [[Biquad::default(); 2]; 2], glue: Follower::new(rate, 30.0, 300.0), glue_gr: 0.0, lim, gain: 1.0 };
        m.update();
        m
    }
    fn update(&mut self) {
        let (p, r) = (&self.p, self.rate);
        let on = p.on(13);
        let c = [
            if on { Coefs::low_shelf(r, p.get(15), 0.7, p.get(14)) } else { Coefs::THRU },
            if on { Coefs::peak(r, p.get(17).min(r * 0.45), 0.8, p.get(16)) } else { Coefs::THRU },
            if on { Coefs::high_shelf(r, p.get(19).min(r * 0.45), 0.7, p.get(18)) } else { Coefs::THRU },
        ];
        for s in 0..2 {
            for k in 0..3 {
                self.eq[s][k].c = c[k];
            }
            let h = Coefs::highpass(r, 4000.0, 0.707);
            self.exc_hp[s] = [Biquad::new(h), Biquad::new(h)];
        }
        self.gain = db(p.get(24));
    }
    fn clear(&mut self) {
        for b in self.eq.iter_mut().flatten().chain(self.exc_hp.iter_mut().flatten()) {
            b.clear();
        }
        self.glue_gr = 0.0;
        self.lim.reset();
    }
    fn tail_secs(&self) -> f32 {
        0.2
    }
}

impl Effect for MasteringAssistant {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let character = self.p.idx(1);
        let excite = self.p.on(20);
        let width = 1.0 + self.p.get(21) / 100.0;
        for i in 0..left.len().min(right.len()) {
            let (mut l, mut r) = (left[i], right[i]);
            for k in 0..3 {
                l = self.eq[0][k].run(l);
                r = self.eq[1][k].run(r);
            }
            if excite {
                for (s, v) in [&mut l, &mut r].into_iter().enumerate() {
                    let h0 = self.exc_hp[s][0].run(*v);
                    let h = (h0 * 3.0).tanh() / 3.0;
                    *v += self.exc_hp[s][1].run(h * h.abs() * 4.0 + h * 0.3) * 0.5;
                }
            }
            match character {
                1 => {
                    // Valve: gentle even-order tube saturation
                    l = (l + 0.08 * l * l).tanh() * 0.98;
                    r = (r + 0.08 * r * r).tanh() * 0.98;
                }
                2 => {
                    // Punch: slow 2:1 glue above -18 dBFS, transients pass the slow attack
                    let e = lg_dsp::gain_to_db(self.glue.run(l.abs().max(r.abs())).max(1e-6));
                    self.glue_gr = flush((e + 18.0).max(0.0) * 0.5);
                    let g = db(-self.glue_gr + 1.5);
                    l *= g;
                    r *= g;
                }
                _ => {}
            }
            let mid = 0.5 * (l + r);
            let side = 0.5 * (l - r) * width;
            left[i] = (mid + side) * self.gain;
            right[i] = (mid - side) * self.gain;
        }
        if character != 3 {
            self.lim.process(left, right);
        }
    }
    fn latency(&self) -> usize {
        if self.p.idx(1) != 3 { self.lim.latency_samples() } else { 0 }
    }
}

plugin_impl!(MasteringAssistant);
