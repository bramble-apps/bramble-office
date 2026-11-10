//! A plucked/struck string as a digital waveguide (extended Karplus-Strong): fractional-delay
//! loop with a loss filter, dispersion-free; used by the clav, basses, guitars, harp and harpsichord.

use crate::dsp::{self, Delay, Rng};

#[derive(Clone, Debug)]
pub struct WaveString {
    pub dl: Delay,
    len: f32,
    /// Loop low-pass coefficient (0 = no damping of highs).
    a: f32,
    z: f32,
    /// Loop gain per period.
    g: f32,
    hz: f32,
    rate: f32,
    dc: dsp::DcBlock,
    /// Excitation queued into the loop (one sample per tick).
    exc: [f32; 2048],
    exc_len: usize,
    exc_pos: usize,
}

impl WaveString {
    /// `min_hz` sets the delay buffer size (allocated here, never while rendering).
    pub fn new(min_hz: f32, rate: f32) -> WaveString {
        WaveString {
            dl: Delay::new((rate / min_hz) as usize + 8),
            len: 100.0,
            a: 0.3,
            z: 0.0,
            g: 0.999,
            hz: 110.0,
            rate,
            dc: dsp::DcBlock::default(),
            exc: [0.0; 2048],
            exc_len: 0,
            exc_pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.dl.clear();
        self.z = 0.0;
        self.exc_len = 0;
        self.exc_pos = 0;
    }

    /// Tunes the loop: `t60` is the decay of the fundamental, `bright` 0..1 how slowly the
    /// upper partials die away compared with it.
    pub fn tune(&mut self, hz: f32, t60: f32, bright: f32) {
        let rate = self.rate;
        let hz = hz.clamp(10.0, rate * 0.3);
        self.hz = hz;
        self.a = (1.0 - bright.clamp(0.0, 1.0)) * 0.85;
        // one-pole low-pass phase delay at f0 ≈ a/(1-a) samples (low frequencies)
        let w = std::f32::consts::TAU * hz / rate;
        let a = self.a;
        let lp_delay = {
            // exact phase delay of y = (1-a)x + a y[-1]
            let ph = (a * w.sin()).atan2(1.0 - a * w.cos());
            ph / w
        };
        let max = (self.dl.len() - 8) as f32;
        self.len = (rate / hz - 1.0 - lp_delay).clamp(2.0, max);
        // loop gain for the wanted decay, compensating the filter's loss at f0
        let lp_mag = (1.0 - a) / (1.0 + a * a - 2.0 * a * w.cos()).sqrt();
        let per_period = dsp::t60_coef(t60.max(0.005), hz);
        self.g = (per_period / lp_mag.max(1e-3)).min(0.99995);
    }

    /// Only changes the pitch (pitch bend), keeping the loss settings.
    pub fn retune(&mut self, hz: f32) {
        let rate = self.rate;
        let hz = hz.clamp(10.0, rate * 0.3);
        let w = std::f32::consts::TAU * hz / rate;
        let a = self.a;
        let ph = (a * w.sin()).atan2(1.0 - a * w.cos()) / w;
        let max = (self.dl.len() - 8) as f32;
        self.len = (rate / hz - 1.0 - ph).clamp(2.0, max);
    }

    /// Sets a new decay (e.g. dampers) without touching tuning.
    pub fn set_decay(&mut self, t60: f32) {
        let w = std::f32::consts::TAU * self.hz / self.rate;
        let a = self.a;
        let lp_mag = (1.0 - a) / (1.0 + a * a - 2.0 * a * w.cos()).sqrt();
        self.g = (dsp::t60_coef(t60.max(0.002), self.hz) / lp_mag.max(1e-3)).min(0.99995);
    }

    pub fn period(&self) -> f32 {
        self.rate / self.hz
    }

    /// Queues a pluck: a burst shaped by `shape` (0 = soft/round, 1 = sharp), `pos` the
    /// plucking position (0..0.5 of the string; comb-filters the excitation), amplitude `amp`.
    pub fn pluck(&mut self, amp: f32, shape: f32, pos: f32, rng: &mut Rng) {
        let n = (self.rate / self.hz).min(2047.0) as usize;
        let n = n.max(2);
        // a smoothed noise/triangle mix: softer plucks are rounder
        let mut lp = 0.0;
        let k = 0.15 + 0.8 * shape.clamp(0.0, 1.0);
        let mut acc = [0.0f32; 2048];
        for (i, a) in acc.iter_mut().enumerate().take(n) {
            let t = i as f32 / n as f32;
            let tri = 1.0 - (2.0 * t - 1.0).abs();
            let src = tri * (1.0 - shape) + rng.bipolar() * shape * 0.8 + tri * 0.3 * shape;
            lp += (src - lp) * k;
            *a = lp;
        }
        // pluck position comb: subtract a delayed copy
        let d = ((pos.clamp(0.02, 0.5)) * n as f32) as usize;
        for i in 0..n {
            let delayed = if i >= d { acc[i - d] } else { 0.0 };
            self.exc[i] = (acc[i] - delayed) * amp;
        }
        // remove DC so the loop does not drift
        let mean: f32 = self.exc[..n].iter().sum::<f32>() / n as f32;
        for x in &mut self.exc[..n] {
            *x -= mean;
        }
        self.exc_len = n;
        self.exc_pos = 0;
    }

    /// Adds a single sample of external force into the loop (bowing, sympathetic drive).
    #[inline]
    pub fn tick(&mut self, input: f32) -> f32 {
        let mut x = input;
        if self.exc_pos < self.exc_len {
            x += self.exc[self.exc_pos];
            self.exc_pos += 1;
        }
        let y = self.dl.read_cubic(self.len);
        self.z = dsp::flush((1.0 - self.a) * y + self.a * self.z);
        self.dl.push(self.z * self.g + x);
        self.dc.run(y)
    }

    /// Output as seen by a pickup at `pos` (0..1 of the period) — a comb on the travelling wave.
    #[inline]
    pub fn pickup(&self, pos: f32) -> f32 {
        let d = pos * self.len;
        self.dl.tap(0) - self.dl.read(d.max(1.0))
    }
}
