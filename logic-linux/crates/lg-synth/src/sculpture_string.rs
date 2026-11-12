//! Sculpture's string: a two-way digital waveguide closed into one ring. Positions along the
//! string (0 = left end, 1 = right end) are slots of the ring, so objects can push, hold or
//! damp the string at a place and pickups can read it at another. Material (inner loss,
//! stiffness, media loss) and Resolution live in the loop filter.

use crate::dsp::{self, Allpass1};
use std::f32::consts::TAU;

pub const MAX_DISP: usize = 16;
const DC_R: f32 = 0.9998;

/// Phase delay in samples of one dispersion all-pass at angular frequency `w`.
fn allpass_delay(a: f32, w: f32) -> f32 {
    let c = -a;
    let ph = (-w.sin()).atan2(c + w.cos()) - (-c * w.sin()).atan2(1.0 + c * w.cos());
    -ph / w
}

/// Phase delay of y = (1-a)x + a y[-1].
fn lp_delay(a: f32, w: f32) -> f32 {
    (a * w.sin()).atan2(1.0 - a * w.cos()) / w
}

fn lp_mag(a: f32, w: f32) -> f32 {
    (1.0 - a) / (1.0 + a * a - 2.0 * a * w.cos()).sqrt().max(1e-3)
}

#[derive(Clone, Debug)]
pub struct Guide {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
    scratch: Vec<f32>,
    /// Ticks per second of the string (the output rate times the oversampling).
    pub rate: f32,
    /// Where the ring is read for the loop (samples).
    d_read: f32,
    a_il: f32,
    z1: f32,
    a_res: f32,
    res_on: bool,
    z2: f32,
    z3: f32,
    g: f32,
    ap: [Allpass1; MAX_DISP],
    ap_a: f32,
    ap_use: usize,
    dc_x: f32,
    dc_y: f32,
    /// Followed peak of the loop signal (tension modulation reads it).
    pub energy: f32,
    pub hz: f32,
}

impl Guide {
    pub fn new(max_samples: usize, rate: f32) -> Guide {
        let n = (max_samples + 16).next_power_of_two();
        Guide {
            buf: vec![0.0; n],
            mask: n - 1,
            w: 0,
            scratch: vec![0.0; n / 2 + 8],
            rate,
            d_read: 100.0,
            a_il: 0.0,
            z1: 0.0,
            a_res: 0.0,
            res_on: false,
            z2: 0.0,
            z3: 0.0,
            g: 0.999,
            ap: [Allpass1::default(); MAX_DISP],
            ap_a: 0.0,
            ap_use: 0,
            dc_x: 0.0,
            dc_y: 0.0,
            energy: 0.0,
            hz: 220.0,
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
        self.z1 = 0.0;
        self.z2 = 0.0;
        self.z3 = 0.0;
        self.ap = [Allpass1::default(); MAX_DISP];
        self.dc_x = 0.0;
        self.dc_y = 0.0;
        self.energy = 0.0;
    }

    /// Sets pitch and material. `t60` is the decay of the fundamental in seconds (media loss),
    /// `il` 0..1 inner loss, `stiff` 0..1, `overtones` the number of overtones the string can
    /// carry (Resolution), `extra_inh` extra inharmonicity from a very low Resolution.
    pub fn set(&mut self, hz: f32, t60: f32, il: f32, stiff: f32, overtones: f32, extra_inh: f32) {
        let rate = self.rate;
        let hz = hz.clamp(8.0, rate * 0.25);
        self.hz = hz;
        let w = TAU * hz / rate;
        self.a_il = (il.clamp(0.0, 1.0) * 0.85).min(0.9);
        let fc = overtones.max(1.5) * hz;
        let mut gain_comp = lp_mag(self.a_il, w);
        let mut delay = lp_delay(self.a_il, w);
        if fc < rate * 0.42 {
            self.a_res = (-TAU * fc / rate).exp();
            self.res_on = true;
            gain_comp *= lp_mag(self.a_res, w).powi(2);
            delay += 2.0 * lp_delay(self.a_res, w);
        } else {
            self.res_on = false;
        }
        // stiffness: how strongly (and from how low) the dispersion stretches the overtones
        self.ap_a = (stiff.clamp(0.0, 1.0) * 0.93 + extra_inh).min(0.95);
        let per = allpass_delay(self.ap_a, w).max(0.01);
        let want = if self.ap_a > 0.001 { MAX_DISP } else { 0 };
        let budget = (rate / hz * 0.6 / per) as usize;
        self.ap_use = want.min(budget);
        delay += per * self.ap_use as f32;
        // the DC blocker leads in phase a little (shortens the loop): count it
        let sw = w.sin();
        let lead = sw.atan2(1.0 - w.cos()) - (DC_R * sw).atan2(1.0 - DC_R * w.cos());
        delay -= lead / w;
        let per_period = dsp::t60_coef(t60.max(0.005), hz);
        self.g = (per_period / gain_comp.max(1e-3)).min(0.99996);
        let max = (self.buf.len() - 8) as f32;
        self.d_read = (rate / hz - 1.0 - delay).clamp(1.0, max);
    }

    /// Length of the usable ring in samples.
    #[inline]
    pub fn len_ring(&self) -> f32 {
        self.d_read
    }

    #[inline]
    fn slot(&self, k: usize) -> usize {
        self.w.wrapping_sub(k) & self.mask
    }

    #[inline]
    fn tap_lin(&self, d: f32) -> f32 {
        let d = d.max(0.0);
        let i = d as usize;
        let f = d - i as f32;
        let a = self.buf[self.slot(i)];
        let b = self.buf[self.slot(i + 1)];
        a + (b - a) * f
    }

    #[inline]
    fn tap_cubic(&self, d: f32) -> f32 {
        let d = d.max(1.0);
        let i = d as usize;
        let f = d - i as f32;
        dsp::hermite(
            self.buf[self.slot(i - 1)],
            self.buf[self.slot(i)],
            self.buf[self.slot(i + 1)],
            self.buf[self.slot(i + 2)],
            f,
        )
    }

    /// The string's displacement at `x` (0..1).
    #[inline]
    pub fn disp(&self, x: f32) -> f32 {
        let l = self.d_read;
        let k1 = x.clamp(0.0, 1.0) * l * 0.5;
        self.tap_lin(k1) - self.tap_lin(l - k1)
    }

    /// Pushes the string by `v` at `x` (a displacement added to both travelling waves).
    #[inline]
    pub fn inject(&mut self, x: f32, v: f32) {
        let l = self.d_read;
        let k1 = x.clamp(0.0, 1.0) * l * 0.5;
        self.add_frac(k1, v * 0.5);
        self.add_frac(l - k1, -v * 0.5);
    }

    #[inline]
    fn add_frac(&mut self, k: f32, v: f32) {
        let i = k as usize;
        let f = k - i as f32;
        let a = self.slot(i);
        let b = self.slot(i + 1);
        self.buf[a] += v * (1.0 - f);
        self.buf[b] += v * f;
    }

    /// Takes `frac` of what both waves carry at `x` (a damper).
    #[inline]
    pub fn damp(&mut self, x: f32, frac: f32) {
        let l = self.d_read;
        let k1 = x.clamp(0.0, 1.0) * l * 0.5;
        for k in [k1, l - k1] {
            let v = self.tap_lin(k);
            self.add_frac(k, -v * frac);
        }
    }

    /// Adds an initial displacement shape: a triangle peaking at `pos` of height `amp`, its
    /// corner rounded over `round` (0..1 of the string).
    pub fn pluck(&mut self, pos: f32, amp: f32, round: f32) {
        let l = self.d_read;
        let n = (l * 0.5) as usize + 1;
        let pos = pos.clamp(0.01, 0.99);
        for j in 0..n.min(self.scratch.len()) {
            let x = (j as f32 / (l * 0.5)).min(1.0);
            self.scratch[j] = if x < pos { x / pos } else { (1.0 - x) / (1.0 - pos) };
        }
        // round the corner: smooth with a one-pole forward and back, as wide as `round`
        let a = (-1.0 / (round * n as f32 * 0.5).max(0.5)).exp();
        if a > 0.05 {
            let mut z = 0.0;
            for j in 0..n {
                z = self.scratch[j] * (1.0 - a) + z * a;
                self.scratch[j] = z;
            }
            let mut z = 0.0;
            for j in (0..n).rev() {
                z = self.scratch[j] * (1.0 - a) + z * a;
                self.scratch[j] = z;
            }
        }
        let lk = l.round() as usize;
        for j in 0..n {
            let v = self.scratch[j] * amp * 0.5;
            let a = self.slot(j);
            self.buf[a] += v;
            let b = self.slot(lk.saturating_sub(j));
            self.buf[b] -= v;
        }
    }

    /// One tick of the loop.
    #[inline]
    pub fn tick(&mut self) {
        let x = self.tap_cubic(self.d_read);
        self.z1 = dsp::flush(x * (1.0 - self.a_il) + self.z1 * self.a_il);
        let mut y = self.z1 * self.g;
        if self.res_on {
            self.z2 = dsp::flush(y * (1.0 - self.a_res) + self.z2 * self.a_res);
            self.z3 = dsp::flush(self.z2 * (1.0 - self.a_res) + self.z3 * self.a_res);
            y = self.z3;
        }
        for i in 0..self.ap_use {
            y = self.ap[i].run(y, self.ap_a);
        }
        let o = y - self.dc_x + DC_R * self.dc_y;
        self.dc_x = y;
        self.dc_y = dsp::flush(o);
        y = o;
        if !y.is_finite() {
            y = 0.0;
        }
        y = y.clamp(-4.0, 4.0);
        self.energy = (self.energy * 0.9995).max(y.abs());
        self.w = (self.w + 1) & self.mask;
        let w = self.w;
        self.buf[w] = y;
    }
}
