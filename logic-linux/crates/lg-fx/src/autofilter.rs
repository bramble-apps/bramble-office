//! AutoFilter: the stand-in for Logic's AutoFilter — a resonant low-pass whose cutoff an LFO
//! and an input-driven envelope move, with pre / post distortion.
//!
//! The filter is a four-pole transistor-ladder style low-pass (each pole a one-pole stage with
//! soft saturation, resonance fed back from the last pole), tapped after 1-4 poles for
//! 6 / 12 / 18 / 24 dB/oct; Fatness puts back the low end resonance takes away. Cutoff (0-100 %)
//! maps 20 Hz-20 kHz on a log scale; the LFO (free Hz, or a synced note when its rate is
//! stored negative, like Tremolo) and the envelope (threshold-gated follower with attack,
//! decay and sustain) move it in octaves; Spread offsets left and right.
//!
//! Positions confirmed from the research: Cutoff 2, Resonance 3, LFO intensity 12, Envelope
//! Threshold 15, LFO Rate 25, LFO Sync Phase 26, Spread 30. Fatness 4, slope 5, envelope
//! times 17-19, Stereo Phase 27 and the distortion levels 0 / 1 are the likely ones (unconfirmed).

use crate::util::{Phase, Values, beats_hz, coef, db, flush, plugin_impl, sine01, sync_index_beats};
use lg_dsp::{Param, Unit};

static PARAMS: [Param; 16] = [
    Param::new(0, "Distortion Input", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(1, "Distortion Output", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(2, "Cutoff", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(3, "Resonance", 0.0, 100.0, 10.0, Unit::Percent),
    Param::new(4, "Fatness", 0.0, 100.0, 0.0, Unit::Percent),
    Param::menu(5, "Slope", &["6 dB/Oct", "12 dB/Oct", "18 dB/Oct", "24 dB/Oct"], 2.0),
    Param::new(12, "LFO", -100.0, 100.0, 14.0, Unit::Percent),
    Param::new(15, "Threshold", -60.0, 0.0, -20.0, Unit::Db),
    Param::new(16, "Envelope", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(17, "Attack", 0.0, 2000.0, 52.0, Unit::Ms),
    Param::new(18, "Sustain", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(19, "Decay", 0.0, 5000.0, 52.0, Unit::Ms),
    Param::new(25, "LFO Rate", -26.0, 100.0, -11.0, Unit::None),
    Param::new(26, "LFO Sync Phase", 0.0, 360.0, 180.0, Unit::None),
    Param::new(27, "Stereo Phase", -180.0, 180.0, 0.0, Unit::None),
    Param::new(30, "Spread", 0.0, 1.0, 0.01, Unit::None),
];

pub struct AutoFilter {
    p: Values,
    rate: f32,
    bpm: f64,
    lfo: Phase,
    env: f32,
    stage: f32,
    pole: [[f32; 4]; 2],
    att: f32,
    dec: f32,
    rel: f32,
}

impl AutoFilter {
    pub fn new(rate: f32) -> AutoFilter {
        let mut a = AutoFilter { p: Values::new(&PARAMS), rate, bpm: 120.0, lfo: Phase::default(), env: 0.0, stage: 0.0, pole: [[0.0; 4]; 2], att: 0.0, dec: 0.0, rel: 0.0 };
        a.update();
        a
    }

    fn lfo_hz(&self) -> f32 {
        let r = self.p.get(25);
        if r < 0.0 { beats_hz(sync_index_beats(r), self.bpm) } else { r.max(0.01) }
    }

    fn update(&mut self) {
        self.att = coef(self.rate, self.p.get(17).max(0.1));
        self.dec = coef(self.rate, self.p.get(19).max(0.1));
        self.rel = coef(self.rate, 200.0);
    }

    fn clear(&mut self) {
        self.pole = [[0.0; 4]; 2];
        self.env = 0.0;
        self.stage = 0.0;
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl lg_dsp::Effect for AutoFilter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate;
        let base_oct = (p.get(2) / 100.0) * 1000f32.log2(); // octaves above 20 Hz
        let res = p.get(3) / 100.0;
        let k = res * 4.2;
        let fat = p.get(4) / 100.0;
        let poles = p.idx(5).min(3) + 1;
        let lfo_amt = p.get(12) / 100.0 * 5.0;
        let env_amt = p.get(16) / 100.0 * 6.0;
        let thr = db(p.get(15));
        let sustain = p.get(18) / 100.0;
        let spread = p.get(30) * 2.0;
        let (sync_ph, st_ph) = (p.get(26) / 360.0, p.get(27) / 360.0);
        let (din, dout) = (p.get(0) / 100.0, p.get(1) / 100.0);
        let hz = self.lfo_hz();
        for i in 0..left.len().min(right.len()) {
            let ph = self.lfo.step(rate, hz);
            // envelope: attack to 1 while the input is above threshold, then decay to sustain;
            // below threshold it releases to 0
            let lvl = left[i].abs().max(right[i].abs());
            if lvl > thr {
                if self.stage < 0.5 {
                    self.env = 1.0 + (self.env - 1.0) * self.att;
                    if self.env > 0.98 {
                        self.stage = 1.0;
                    }
                } else {
                    self.env = sustain + (self.env - sustain) * self.dec;
                }
            } else {
                self.stage = 0.0;
                self.env = flush(self.env * self.rel);
            }
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let lp = sine01(ph + sync_ph + if s == 1 { st_ph } else { 0.0 });
                let oct = base_oct + lfo_amt * lp + env_amt * self.env + if s == 1 { spread } else { -spread } * 0.5;
                let fc = (20.0 * 2f32.powf(oct)).clamp(20.0, rate * 0.45);
                let g = 1.0 - (-crate::util::TAU * fc / rate).exp();
                let mut v = *x;
                if din > 0.0 {
                    v = (v * (1.0 + 8.0 * din)).tanh() / (1.0 + 2.0 * din);
                }
                let st = &mut self.pole[s];
                // resonance feedback (with Fatness adding back the dry low end)
                let u = (v - k * (st[poles - 1] - fat * 0.5 * v)).tanh();
                st[0] = flush(st[0] + g * (u - st[0]));
                for j in 1..4 {
                    st[j] = flush(st[j] + g * (st[j - 1] - st[j]));
                }
                let mut y = st[poles - 1] * (1.0 + k * 0.5 * fat);
                if dout > 0.0 {
                    y = (y * (1.0 + 8.0 * dout)).tanh() / (1.0 + 2.0 * dout);
                }
                *x = y;
            }
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
}

plugin_impl!(AutoFilter);
