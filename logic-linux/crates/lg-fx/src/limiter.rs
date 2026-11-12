//! Limiter and Adaptive Limiter: stand-ins for Logic's two look-ahead limiters.
//!
//! Limiter (project floats: Gain, Lookahead, Mode, Release, Output Level, -, True Peak):
//! - Precision mode never lets a peak through: the gain needed for each sample is held as a
//!   running minimum over the look-ahead window and then averaged over that same window, so the
//!   gain is already down when the delayed peak arrives. Release is exponential.
//! - Legacy mode follows peaks with a smooth one-pole attack the length of the look-ahead
//!   and catches what slips through with a soft clip at the ceiling (Logic's older, warmer one).
//! - True Peak Detection measures 4x-oversampled (inter-sample) peaks.
//!
//! Adaptive Limiter: a program-dependent limiter that rounds off peaks (a soft knee that starts
//! a few dB under the ceiling and adapts its release to the material), like an analogue
//! limiter driven hard; "NoOver" mode adds a final look-ahead stage so nothing exceeds the
//! ceiling. Its parameter positions are only partly confirmed (see the report in lib.rs).
//!
//! Both delay the signal by the look-ahead: `latency_samples()` tells the host how much to
//! compensate (lg-dsp's Effect has no latency call yet).

use crate::util::{Values, coef, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

const MAX_LOOKAHEAD_MS: f32 = 20.0;

static LIMITER_PARAMS: [Param; 7] = [
    Param::new(0, "Gain", 0.0, 20.0, 0.0, Unit::Db),
    Param::new(1, "Lookahead", 0.0, 20.0, 5.0, Unit::Ms),
    Param::menu(2, "Mode", &["Legacy", "Precision"], 1.0),
    Param::new(3, "Release", 0.01, 1000.0, 250.0, Unit::Ms),
    Param::new(4, "Output Level", -30.0, 0.0, 0.0, Unit::Db),
    // Legacy mode only: a gradual (nonlinear) move into full limiting over the last few dB
    Param::new(5, "Soft Knee", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(6, "True Peak Detection", 0.0, 1.0, 1.0, Unit::OnOff),
];

/// Running minimum over a sliding window (monotonic deque in fixed rings) and a running mean,
/// both allocation-free.
struct Window {
    /// deque of (index, value), increasing values from front
    dq: Vec<(u64, f32)>,
    head: usize,
    len: usize,
    ring: Vec<f32>,
    sum: f64,
    pos: usize,
    n: u64,
}

impl Window {
    fn new(cap: usize) -> Window {
        Window { dq: vec![(0, 0.0); cap + 1], head: 0, len: 0, ring: vec![1.0; cap + 1], sum: 0.0, pos: 0, n: 0 }
    }

    fn clear(&mut self, w: usize) {
        self.head = 0;
        self.len = 0;
        self.n = 0;
        self.pos = 0;
        self.ring.iter_mut().for_each(|x| *x = 1.0);
        self.sum = w as f64;
    }

    /// Pushes `v`; returns the minimum over the last `w` values.
    fn min(&mut self, v: f32, w: usize) -> f32 {
        let cap = self.dq.len();
        while self.len > 0 {
            let back = (self.head + self.len - 1) % cap;
            if self.dq[back].1 >= v {
                self.len -= 1;
            } else {
                break;
            }
        }
        let at = (self.head + self.len) % cap;
        self.dq[at] = (self.n, v);
        self.len += 1;
        while self.len > 0 && self.dq[self.head].0 + w as u64 <= self.n {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        self.n += 1;
        self.dq[self.head].1
    }

    /// Pushes `v`; returns the mean of the last `w` values.
    fn mean(&mut self, v: f32, w: usize) -> f32 {
        let cap = self.ring.len();
        let old = self.ring[(self.pos + cap - w % cap) % cap];
        self.ring[self.pos] = v;
        self.pos = (self.pos + 1) % cap;
        self.sum += v as f64 - old as f64;
        (self.sum / w as f64) as f32
    }
}

/// 4x-oversampled peak of the signal (a 32-tap windowed-sinc interpolator on the detector only).
struct TruePeak {
    hist: [[f32; 8]; 2],
    taps: [[f32; 8]; 3],
}

impl TruePeak {
    fn new() -> TruePeak {
        let mut taps = [[0.0; 8]; 3];
        for (ph, row) in taps.iter_mut().enumerate() {
            let frac = (ph + 1) as f32 / 4.0;
            for (k, t) in row.iter_mut().enumerate() {
                // tap k weighs the sample (k - 3) - frac away
                let x = k as f32 - 3.0 - frac;
                let sinc = if x.abs() < 1e-6 { 1.0 } else { (std::f32::consts::PI * x).sin() / (std::f32::consts::PI * x) };
                let win = 0.5 + 0.5 * (std::f32::consts::PI * x / 4.5).cos();
                *t = sinc * win;
            }
        }
        TruePeak { hist: [[0.0; 8]; 2], taps }
    }

    #[inline]
    fn peak(&mut self, ch: usize, x: f32) -> f32 {
        let h = &mut self.hist[ch];
        h.copy_within(1.., 0);
        h[7] = x;
        // the newest complete interpolation point lies between h[3] and h[4]
        let mut m = h[4].abs();
        for row in &self.taps {
            let mut s = 0.0;
            for k in 0..8 {
                s += row[k] * h[7 - k];
            }
            m = m.max(s.abs());
        }
        m
    }
}

/// Taps of the true-peak path's audio filter (Logic's oversampling filter shows as a gentle
/// roll-off just under Nyquist: about -1 dB on a single-sample click, -0.3 dB on white noise).
const FIR_LEN: usize = 31;

pub struct Limiter {
    p: Values,
    fir: [f32; FIR_LEN],
    fir_hist: [[f32; FIR_LEN]; 2],
    fir_pos: usize,
    rate: f32,
    delay: [Vec<f32>; 2],
    dpos: usize,
    look: usize,
    win_min: Window,
    win_mean: Window,
    tp: TruePeak,
    gain: f32,
    ceiling: f32,
    rel: f32,
    att: f32,
    env: f32,
    /// current reduction (dB, positive) for a meter
    pub reduction: f32,
    /// the last block's input and output peaks (linear), for the window's meters
    in_peak: f32,
    out_peak: f32,
}

impl Limiter {
    pub fn new(rate: f32) -> Limiter {
        let cap = (MAX_LOOKAHEAD_MS * 0.001 * rate) as usize + 8;
        let mut fir = [0.0; FIR_LEN];
        let fc = (21500.0 / rate).min(0.448);
        let mid = (FIR_LEN / 2) as f32;
        for (k, t) in fir.iter_mut().enumerate() {
            let x = k as f32 - mid;
            let sinc = if x == 0.0 { 2.0 * fc } else { (crate::util::TAU * fc * x).sin() / (std::f32::consts::PI * x) };
            let w = 0.42 + 0.5 * (std::f32::consts::PI * x / (mid + 1.0)).cos() + 0.08 * (crate::util::TAU * x / (mid + 1.0)).cos();
            *t = sinc * w;
        }
        let sum: f32 = fir.iter().sum();
        fir.iter_mut().for_each(|t| *t /= sum);
        let mut l = Limiter {
            p: Values::new(&LIMITER_PARAMS),
            fir,
            fir_hist: [[0.0; FIR_LEN]; 2],
            fir_pos: 0,
            rate,
            delay: [vec![0.0; cap + 8], vec![0.0; cap + 8]],
            dpos: 0,
            look: 1,
            win_min: Window::new(cap + 8),
            win_mean: Window::new(cap + 8),
            tp: TruePeak::new(),
            gain: 1.0,
            ceiling: 1.0,
            rel: 0.0,
            att: 0.0,
            env: 1.0,
            reduction: 0.0,
            in_peak: 0.0,
            out_peak: 0.0,
        };
        l.update();
        l.clear();
        l
    }

    /// The delay the look-ahead (and the true-peak filter) adds; the host should compensate it.
    pub fn latency_samples(&self) -> usize {
        self.look + if self.p.on(6) { FIR_LEN / 2 } else { 0 }
    }

    fn update(&mut self) {
        let look = ((self.p.get(1) * 0.001 * self.rate) as usize).max(1).min(self.delay[0].len() - 8);
        if look != self.look {
            self.look = look;
            self.clear();
        }
        self.gain = db(self.p.get(0));
        // a hair under the set level, so true-peak overs after conversion stay out
        self.ceiling = db(self.p.get(4)) * 0.9977;
        self.rel = coef(self.rate, self.p.get(3).max(0.01));
        self.att = coef(self.rate, (self.p.get(1) / 3.0).max(0.05));
    }

    fn clear(&mut self) {
        for d in self.delay.iter_mut() {
            d.iter_mut().for_each(|x| *x = 0.0);
        }
        self.win_min.clear(self.look + 1);
        self.win_mean.clear(self.look + 1);
        self.tp = TruePeak::new();
        self.fir_hist = [[0.0; FIR_LEN]; 2];
        self.env = 1.0;
    }

    fn tail_secs(&self) -> f32 {
        self.look as f32 / self.rate
    }
}

impl lg_dsp::Effect for Limiter {
    fn gain_reduction(&self) -> f32 {
        self.reduction
    }

    /// The window's Input / Reduction / Output meters (dB; reduction positive).
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Input", lg_dsp::gain_to_db(self.in_peak.max(1e-6))), ("Reduction", self.reduction), ("Output", lg_dsp::gain_to_db(self.out_peak.max(1e-6)))]
    }

    fn latency(&self) -> usize {
        self.latency_samples()
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let precision = self.p.idx(2) == 1;
        let true_peak = self.p.on(6);
        let soft = !precision && self.p.on(5);
        let cap = self.delay[0].len();
        let (mut pin, mut pout) = (0f32, 0f32);
        for i in 0..n {
            pin = pin.max(left[i].abs()).max(right[i].abs());
            let (l, r) = (left[i] * self.gain, right[i] * self.gain);
            let pk = if true_peak { self.tp.peak(0, l).max(self.tp.peak(1, r)) } else { l.abs().max(r.abs()) };
            let want = if soft {
                // a 12 dB wide soft knee centred on the ceiling (infinite ratio above it)
                const W: f32 = 6.0;
                let x = lg_dsp::gain_to_db(pk.max(1e-9) / self.ceiling);
                let gr = if x <= -W { 0.0 } else if x < W { -(x + W).powi(2) / (4.0 * W) } else { -x };
                db(gr)
            } else if pk > self.ceiling {
                self.ceiling / pk
            } else {
                1.0
            };
            let g = if precision {
                // hold the lowest gain over the look-ahead, then ramp into it over the same window
                let m = self.win_min.min(want, self.look + 1);
                let target = self.win_mean.mean(m, self.look + 1);
                self.env = if target < self.env { target } else { flush(target + (self.env - target) * self.rel) };
                self.env
            } else {
                let k = if want < self.env { self.att } else { self.rel };
                self.env = flush(want + (self.env - want) * k);
                self.env
            };
            self.reduction = -lg_dsp::gain_to_db(g);
            // the delayed (look-ahead) signal
            let rd = (self.dpos + cap - self.look) % cap;
            self.delay[0][self.dpos] = l;
            self.delay[1][self.dpos] = r;
            let (dl, dr) = (self.delay[0][rd], self.delay[1][rd]);
            self.dpos = (self.dpos + 1) % cap;
            let (mut yl, mut yr) = (dl * g, dr * g);
            if true_peak {
                self.fir_hist[0][self.fir_pos] = yl;
                self.fir_hist[1][self.fir_pos] = yr;
                let (mut al, mut ar) = (0.0, 0.0);
                for k in 0..FIR_LEN {
                    let j = (self.fir_pos + FIR_LEN - k) % FIR_LEN;
                    al += self.fir[k] * self.fir_hist[0][j];
                    ar += self.fir[k] * self.fir_hist[1][j];
                }
                self.fir_pos = (self.fir_pos + 1) % FIR_LEN;
                yl = al;
                yr = ar;
            }
            if !precision {
                yl = soft_ceiling(yl, self.ceiling);
                yr = soft_ceiling(yr, self.ceiling);
            } else {
                yl = yl.clamp(-self.ceiling, self.ceiling);
                yr = yr.clamp(-self.ceiling, self.ceiling);
            }
            left[i] = yl;
            right[i] = yr;
            pout = pout.max(yl.abs()).max(yr.abs());
        }
        self.in_peak = pin;
        self.out_peak = pout;
    }
}

/// A soft clip that is transparent below 90 % of the ceiling and never exceeds it.
#[inline]
fn soft_ceiling(x: f32, c: f32) -> f32 {
    let k = 0.9 * c;
    let a = x.abs();
    if a <= k {
        x
    } else {
        let over = (a - k) / (c - k);
        let y = k + (c - k) * over.tanh();
        y.copysign(x)
    }
}

plugin_impl!(Limiter);

// ---------------------------------------------------------------------------------------------

static ADAPTIVE_PARAMS: [Param; 7] = [
    Param::new(0, "Gain", 0.0, 24.0, 0.0, Unit::Db),
    Param::new(1, "Lookahead", 0.0, 20.0, 3.0, Unit::Ms),
    Param::new(2, "Out Ceiling", -20.0, 0.0, 0.0, Unit::Db),
    Param::menu(3, "Mode", &["OptFit", "NoOver"], 1.0),
    Param::new(4, "Input Scale", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(5, "Remove DC", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(8, "True Peak Detection", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct AdaptiveLimiter {
    p: Values,
    rate: f32,
    inner: Limiter,
    gain: f32,
    ceiling: f32,
    env: f32,
    slow: f32,
    att: f32,
    rel_fast: f32,
    rel_slow: f32,
    dc: [f32; 2],
    dc_k: f32,
}

impl AdaptiveLimiter {
    pub fn new(rate: f32) -> AdaptiveLimiter {
        let mut a = AdaptiveLimiter {
            p: Values::new(&ADAPTIVE_PARAMS),
            rate,
            inner: Limiter::new(rate),
            gain: 1.0,
            ceiling: 1.0,
            env: 0.0,
            slow: 0.0,
            att: coef(rate, 0.5),
            rel_fast: coef(rate, 60.0),
            rel_slow: coef(rate, 900.0),
            dc: [0.0; 2],
            dc_k: 1.0 - coef(rate, 1000.0 / (crate::util::TAU * 8.0)),
        };
        a.update();
        a
    }

    pub fn latency_samples(&self) -> usize {
        if self.p.idx(3) == 1 { self.inner.latency_samples() } else { 0 }
    }

    fn update(&mut self) {
        self.gain = db(self.p.get(0));
        self.ceiling = db(self.p.get(2));
        use lg_dsp::Plugin;
        self.inner.set(1, self.p.get(1));
        self.inner.set(4, self.p.get(2));
        self.inner.set(6, self.p.get(8));
        self.inner.set(3, 100.0);
        let _ = self.rate;
    }

    fn clear(&mut self) {
        self.env = 0.0;
        self.slow = 0.0;
        self.dc = [0.0; 2];
        self.inner.clear();
    }

    fn tail_secs(&self) -> f32 {
        self.inner.tail_secs()
    }
}

impl lg_dsp::Effect for AdaptiveLimiter {
    fn gain_reduction(&self) -> f32 {
        self.env.max(self.slow * 0.8) * 0.85 + if self.p.idx(3) == 1 { self.inner.reduction } else { 0.0 }
    }

    fn latency(&self) -> usize {
        self.latency_samples()
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let remove_dc = self.p.on(5);
        // how much of the work the adaptive (rounding) stage does: Input Scale
        let scale = (self.p.get(4) / 100.0).clamp(0.0, 1.0);
        let knee = 0.5 + 0.4 * (1.0 - scale);
        for i in 0..n {
            let (mut l, mut r) = (left[i] * self.gain, right[i] * self.gain);
            if remove_dc {
                self.dc[0] = flush(self.dc[0] + (l - self.dc[0]) * self.dc_k);
                self.dc[1] = flush(self.dc[1] + (r - self.dc[1]) * self.dc_k);
                l -= self.dc[0];
                r -= self.dc[1];
            }
            // program-dependent reduction: fast for peaks, slow for dense material
            let pk = l.abs().max(r.abs()) / self.ceiling;
            let over = if pk > knee { lg_dsp::gain_to_db(pk / knee) } else { 0.0 };
            self.env = flush(over + (self.env - over) * if over > self.env { self.att } else { self.rel_fast });
            self.slow = flush(over + (self.slow - over) * if over > self.slow { self.rel_fast } else { self.rel_slow });
            let gr = self.env.max(self.slow * 0.8) * 0.85;
            let g = db(-gr);
            // what still pokes over the knee is rounded off smoothly to the ceiling
            left[i] = round_off(l * g, self.ceiling, knee);
            right[i] = round_off(r * g, self.ceiling, knee);
        }
        if self.p.idx(3) == 1 {
            self.inner.process(left, right);
        }
    }
}

#[inline]
fn round_off(x: f32, c: f32, knee: f32) -> f32 {
    let k = knee * c;
    let a = x.abs();
    if a <= k {
        x
    } else {
        (k + (c - k) * ((a - k) / (c - k)).tanh()).copysign(x)
    }
}

plugin_impl!(AdaptiveLimiter);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    #[test]
    fn precision_never_exceeds_ceiling() {
        let mut l = Limiter::new(48000.0);
        l.set(0, 12.0);
        l.set(4, -1.0);
        let mut nz = crate::util::Noise(3);
        let mut a: Vec<f32> = (0..48000).map(|_| nz.next()).collect();
        let mut b = a.clone();
        l.process(&mut a, &mut b);
        let c = db(-1.0);
        assert!(a.iter().all(|x| x.abs() <= c + 1e-6));
    }

    #[test]
    fn legacy_soft_knee_eases_into_limiting() {
        // a steady tone 1.5 dB under the ceiling: untouched with a hard knee, turned down a
        // little with Soft Knee (Legacy mode only)
        let level = |mode: f32, soft: f32| {
            let mut l = Limiter::new(48000.0);
            l.set(2, mode);
            l.set(5, soft);
            l.set(6, 0.0);
            let mut a: Vec<f32> = (0..24000).map(|i| db(-1.5) * (i as f32 * 0.05).sin()).collect();
            let mut b = a.clone();
            l.process(&mut a, &mut b);
            a[12000..].iter().map(|x| x.abs()).fold(0.0, f32::max)
        };
        assert!((level(0.0, 0.0) - db(-1.5)).abs() < 0.01);
        assert!(level(0.0, 1.0) < db(-1.5) * 0.95, "{}", level(0.0, 1.0));
        assert!((level(1.0, 1.0) - db(-1.5)).abs() < 0.01);
        assert_eq!(LIMITER_PARAMS[2].choices, &["Legacy", "Precision"]);
    }

    #[test]
    fn quiet_signal_untouched_but_delayed() {
        let mut l = Limiter::new(48000.0);
        l.set(6, 0.0);
        let mut a = vec![0.0; 1000];
        a[0] = 0.25;
        let mut b = a.clone();
        l.process(&mut a, &mut b);
        let d = l.latency_samples();
        assert_eq!(d, 240);
        assert!((a[d] - 0.25).abs() < 1e-6, "{:?}", &a[d - 2..d + 3]);
    }

    #[test]
    fn adaptive_stays_under_ceiling_in_noover() {
        let mut l = AdaptiveLimiter::new(48000.0);
        l.set(0, 18.0);
        let mut nz = crate::util::Noise(5);
        let mut a: Vec<f32> = (0..48000).map(|_| nz.next() * 0.5).collect();
        let mut b = a.clone();
        l.process(&mut a, &mut b);
        assert!(a.iter().all(|x| x.is_finite() && x.abs() <= 1.0001));
    }
}
