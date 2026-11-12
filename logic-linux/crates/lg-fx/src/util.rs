//! The building blocks every effect here shares: parameter storage (by Logic's float index),
//! smoothing, biquads (RBJ cookbook, transposed direct form II), fractional delay lines, LFOs,
//! envelope followers, a small noise source and Logic's tempo-sync note values.
//!
//! Everything is allocation-free once built: buffers are sized in the constructors from the
//! sample rate, and `process` only reads and writes them.

use lg_dsp::{Param, Unit};

pub const PI: f32 = std::f32::consts::PI;
pub const TAU: f32 = std::f32::consts::TAU;

/// Flushes values too small to matter to zero, so feedback paths never fall into denormals.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

#[inline]
pub fn db(x: f32) -> f32 {
    lg_dsp::db_to_gain(x)
}

/// A one-pole coefficient for a time constant of `ms` at `rate` (0 = instant).
pub fn coef(rate: f32, ms: f32) -> f32 {
    if ms <= 0.0 { 0.0 } else { (-1.0 / (0.001 * ms * rate)).exp() }
}

/// The parameter values of one unit, stored by id (Logic's float index), clamped to range.
#[derive(Clone, Debug)]
pub struct Values {
    pub defs: &'static [Param],
    v: Vec<f32>,
}

impl Values {
    pub fn new(defs: &'static [Param]) -> Values {
        let n = defs.iter().map(|p| p.id as usize + 1).max().unwrap_or(0);
        let mut v = vec![0.0; n];
        for p in defs {
            v[p.id as usize] = p.default;
        }
        Values { defs, v }
    }

    #[inline]
    pub fn get(&self, id: u32) -> f32 {
        self.v.get(id as usize).copied().unwrap_or(0.0)
    }

    /// Whether a switch parameter is on.
    #[inline]
    pub fn on(&self, id: u32) -> bool {
        self.get(id) > 0.5
    }

    /// A menu parameter's choice.
    #[inline]
    pub fn idx(&self, id: u32) -> usize {
        self.get(id).round().max(0.0) as usize
    }

    /// Stores a value (clamped; menus and switches rounded). False for an unknown id or NaN.
    pub fn set(&mut self, id: u32, x: f32) -> bool {
        let Some(p) = self.defs.iter().find(|p| p.id == id) else { return false };
        if !x.is_finite() {
            return false;
        }
        let mut x = x.clamp(p.min.min(p.max), p.max.max(p.min));
        if matches!(p.unit, Unit::Menu | Unit::OnOff) {
            x = x.round();
        }
        self.v[id as usize] = x;
        true
    }

    pub fn defaults(&mut self) {
        for p in self.defs {
            self.v[p.id as usize] = p.default;
        }
    }
}

/// Implements `lg_dsp::Plugin` for an effect holding its `Values` in `p` and offering
/// `update()` (recompute coefficients after a change), `clear()` (drop audio state) and
/// `tail_secs()`. Loading a float block sets everything first and updates once.
macro_rules! plugin_impl {
    ($t:ty) => {
        impl lg_dsp::Plugin for $t {
            fn params(&self) -> &[lg_dsp::Param] {
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
    };
}
pub(crate) use plugin_impl;

/// Like `plugin_impl!`, but user edits go through the type's own `edit(id, value)` (for
/// Logic's linked buttons: momentary :2 / x2, Stereo Link …); loading floats stays direct.
macro_rules! plugin_impl_edit {
    ($t:ty) => {
        impl lg_dsp::Plugin for $t {
            fn params(&self) -> &[lg_dsp::Param] {
                self.p.defs
            }
            fn get(&self, id: u32) -> f32 {
                self.p.get(id)
            }
            fn set(&mut self, id: u32, value: f32) {
                self.edit(id, value);
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
    };
}
pub(crate) use plugin_impl_edit;

/// One-pole smoothing towards a target (for gains and times that must not click).
#[derive(Clone, Copy, Debug)]
pub struct Smooth {
    pub cur: f32,
    pub target: f32,
    k: f32,
}

impl Smooth {
    pub fn new(rate: f32, ms: f32, v: f32) -> Smooth {
        Smooth { cur: v, target: v, k: coef(rate, ms) }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.cur = self.target + (self.cur - self.target) * self.k;
        if (self.cur - self.target).abs() < 1e-7 {
            self.cur = self.target;
        }
        self.cur
    }
    pub fn snap(&mut self) {
        self.cur = self.target;
    }
    pub fn set_time(&mut self, rate: f32, ms: f32) {
        self.k = coef(rate, ms);
    }
}

/// Normalised biquad coefficients (a0 = 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for Coefs {
    fn default() -> Self {
        Coefs::THRU
    }
}

impl Coefs {
    pub const THRU: Coefs = Coefs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefs {
        Coefs { b0: (b0 / a0) as f32, b1: (b1 / a0) as f32, b2: (b2 / a0) as f32, a1: (a1 / a0) as f32, a2: (a2 / a0) as f32 }
    }

    /// (sin w, cos w) for `f`, kept safely below Nyquist. Computed in f64 so low
    /// frequencies at high rates stay exact.
    fn w(rate: f32, f: f32) -> (f64, f64) {
        let f = (f as f64).clamp(1.0, rate as f64 * 0.49);
        let w = std::f64::consts::TAU * f / rate as f64;
        (w.sin(), w.cos())
    }

    pub fn lowpass(rate: f32, f: f32, q: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn highpass(rate: f32, f: f32, q: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    /// Band-pass with 0 dB at the centre.
    pub fn bandpass(rate: f32, f: f32, q: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn notch(rate: f32, f: f32, q: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm(1.0, -2.0 * c, 1.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn allpass(rate: f32, f: f32, q: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm(1.0 - al, -2.0 * c, 1.0 + al, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    pub fn peak(rate: f32, f: f32, q: f32, gain_db: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let al = s / (2.0 * q.max(0.05) as f64);
        Self::norm(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a)
    }

    pub fn low_shelf(rate: f32, f: f32, q: f32, gain_db: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let al = s / (2.0 * q.max(0.05) as f64);
        let k = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - k),
            (a + 1.0) + (a - 1.0) * c + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - k,
        )
    }

    pub fn high_shelf(rate: f32, f: f32, q: f32, gain_db: f32) -> Coefs {
        let (s, c) = Self::w(rate, f);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let al = s / (2.0 * q.max(0.05) as f64);
        let k = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - k),
            (a + 1.0) - (a - 1.0) * c + k,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - k,
        )
    }

    /// First-order low-pass (bilinear, pre-warped).
    pub fn lp1(rate: f32, f: f32) -> Coefs {
        let k = (std::f64::consts::PI * (f as f64).clamp(1.0, rate as f64 * 0.49) / rate as f64).tan();
        Self::norm(k, k, 0.0, k + 1.0, k - 1.0, 0.0)
    }

    /// First-order high-pass (bilinear, pre-warped).
    pub fn hp1(rate: f32, f: f32) -> Coefs {
        let k = (std::f64::consts::PI * (f as f64).clamp(1.0, rate as f64 * 0.49) / rate as f64).tan();
        Self::norm(1.0, -1.0, 0.0, k + 1.0, k - 1.0, 0.0)
    }

    /// First-order all-pass with its 90° point at `f`.
    pub fn ap1(rate: f32, f: f32) -> Coefs {
        let k = (std::f64::consts::PI * (f as f64).clamp(1.0, rate as f64 * 0.49) / rate as f64).tan();
        let a = (k - 1.0) / (k + 1.0);
        Coefs { b0: a as f32, b1: 1.0, b2: 0.0, a1: a as f32, a2: 0.0 }
    }

    /// Linear magnitude at `f`.
    pub fn magnitude(&self, rate: f32, f: f32) -> f32 {
        let w = std::f64::consts::TAU * f as f64 / rate as f64;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (b0, b1, b2, a1, a2) = (self.b0 as f64, self.b1 as f64, self.b2 as f64, self.a1 as f64, self.a2 as f64);
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = -(b1 * s1 + b2 * s2);
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = -(a1 * s1 + a2 * s2);
        (((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-30)).sqrt()) as f32
    }
}

/// One biquad section with its state (mono; use one per channel).
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub c: Coefs,
    s1: f32,
    s2: f32,
}

impl Biquad {
    pub fn new(c: Coefs) -> Biquad {
        Biquad { c, s1: 0.0, s2: 0.0 }
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let c = &self.c;
        let y = c.b0 * x + self.s1;
        self.s1 = flush(c.b1 * x - c.a1 * y + self.s2);
        self.s2 = flush(c.b2 * x - c.a2 * y);
        y
    }
    pub fn clear(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

/// A simple one-pole low-pass (y += a (x - y)), for tone controls and damping.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    pub a: f32,
    pub y: f32,
}

impl OnePole {
    pub fn new(rate: f32, f: f32) -> OnePole {
        let mut o = OnePole { a: 0.0, y: 0.0 };
        o.set(rate, f);
        o
    }
    pub fn set(&mut self, rate: f32, f: f32) {
        self.a = 1.0 - (-TAU * f.clamp(1.0, rate * 0.49) / rate).exp();
    }
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.y = flush(self.y + self.a * (x - self.y));
        self.y
    }
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
}

/// A delay line with fractional reads (cubic Hermite), its length a power of two.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl Delay {
    /// Room for at least `max` samples of delay.
    pub fn new(max: usize) -> Delay {
        let n = (max + 4).next_power_of_two();
        Delay { buf: vec![0.0; n], mask: n - 1, pos: 0 }
    }
    pub fn capacity(&self) -> usize {
        self.buf.len() - 4
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = flush(x);
    }
    /// The sample written `n` pushes ago (0 = the latest).
    #[inline]
    pub fn tap(&self, n: usize) -> f32 {
        self.buf[(self.pos.wrapping_sub(n)) & self.mask]
    }
    /// The input from `d` samples ago (fractional, d >= 1), read before this sample's push.
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.capacity() - 2) as f32);
        let i = d as usize;
        let t = d - i as f32;
        // tap(k) is the input from k + 1 samples ago
        let y1 = self.tap(i - 1);
        let y0 = if i >= 2 { self.tap(i - 2) } else { y1 };
        let (y2, y3) = (self.tap(i), self.tap(i + 1));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
    /// Linear interpolation (cheaper), read AFTER this sample's push: `d` = 0 is the input just
    /// pushed.
    #[inline]
    pub fn read_lin(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.capacity() - 2) as f32);
        let i = d as usize;
        let t = d - i as f32;
        self.tap(i) * (1.0 - t) + self.tap(i + 1) * t
    }
    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// A phase from 0 to 1, advanced by `hz` per second.
#[derive(Clone, Copy, Debug, Default)]
pub struct Phase {
    pub p: f32,
}

impl Phase {
    #[inline]
    pub fn step(&mut self, rate: f32, hz: f32) -> f32 {
        self.p += hz / rate;
        self.p -= self.p.floor();
        self.p
    }
}

#[inline]
pub fn sine01(p: f32) -> f32 {
    (TAU * p).sin()
}

/// Triangle, -1..1, from a phase 0..1 (0 at phase 0, rising).
#[inline]
pub fn tri(p: f32) -> f32 {
    let p = p - p.floor();
    if p < 0.25 {
        4.0 * p
    } else if p < 0.75 {
        2.0 - 4.0 * p
    } else {
        4.0 * p - 4.0
    }
}

/// A small fast noise source (xorshift).
#[derive(Clone, Copy, Debug)]
pub struct Noise(pub u32);

impl Noise {
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Smoothed random modulation: a new random target every 1/hz seconds, glided to.
#[derive(Clone, Copy, Debug)]
pub struct Drift {
    noise: Noise,
    ph: Phase,
    from: f32,
    to: f32,
}

impl Drift {
    pub fn new(seed: u32) -> Drift {
        Drift { noise: Noise(seed | 1), ph: Phase::default(), from: 0.0, to: 0.0 }
    }
    #[inline]
    pub fn next(&mut self, rate: f32, hz: f32) -> f32 {
        let before = self.ph.p;
        let p = self.ph.step(rate, hz);
        if p < before {
            self.from = self.to;
            self.to = self.noise.next();
        }
        // cosine glide between the random points
        let t = 0.5 - 0.5 * (PI * p).cos();
        self.from + (self.to - self.from) * t
    }
}

/// Attack / release envelope follower on a rectified (or squared) input.
#[derive(Clone, Copy, Debug, Default)]
pub struct Follower {
    pub env: f32,
    att: f32,
    rel: f32,
}

impl Follower {
    pub fn new(rate: f32, att_ms: f32, rel_ms: f32) -> Follower {
        let mut f = Follower::default();
        f.set(rate, att_ms, rel_ms);
        f
    }
    pub fn set(&mut self, rate: f32, att_ms: f32, rel_ms: f32) {
        self.att = coef(rate, att_ms);
        self.rel = coef(rate, rel_ms);
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let k = if x > self.env { self.att } else { self.rel };
        self.env = flush(x + (self.env - x) * k);
        self.env
    }
}

/// Logic's tempo-sync "note" stored as the divisor of a whole note (4 = 1/4, 6 = 1/4 triplet,
/// 5.333 = dotted 1/8 …), as Stereo Delay and Tape Delay keep it: length in beats.
pub fn note_div_beats(div: f32) -> f32 {
    4.0 / div.max(0.01)
}

/// Logic's sync-rate index used by Tremolo and AutoFilter (a negative rate value means "synced",
/// its magnitude the index): ordered by length, triplet / straight / dotted in turn, index 11 =
/// 1/8, 14 = 1/4. Length in beats.
pub fn sync_index_beats(index: f32) -> f32 {
    let m = index.abs().round() as i32 - 2;
    let g = m.div_euclid(3);
    let r = m.rem_euclid(3);
    // 1/64 note = 1/16 beat at index 2; straight, triplet (x4/3), dotted (x3/2)
    let mult = [1.0, 4.0 / 3.0, 1.5][r as usize];
    (1.0 / 16.0) * 2f32.powi(g) * mult
}

/// Names for the sync indices above (for the plug-in window): 11 = "1/8", 12 = "1/4t",
/// 13 = "1/8.", 14 = "1/4".
pub fn sync_index_name(index: f32) -> String {
    let m = index.abs().round() as i32 - 2;
    let g = m.div_euclid(3);
    let r = m.rem_euclid(3);
    let base = |g: i32| if g <= 6 { format!("1/{}", 64 >> g.max(0)) } else { format!("{}/1", 1 << (g - 6)) };
    match r {
        0 => base(g),
        1 => format!("{}t", base(g + 1)),
        _ => format!("{}.", base(g)),
    }
}

/// A rate in Hz from a "beats per cycle" length at `bpm`.
pub fn beats_hz(beats: f32, bpm: f64) -> f32 {
    (bpm as f32 / 60.0) / beats.max(1e-3)
}

/// A free rate locked to the nearest synced note value (sync indices 2-26: 1/64 to four bars,
/// straight, triplet and dotted) at a tempo.
pub fn nearest_sync_hz(hz: f32, bpm: f64) -> f32 {
    let want = hz.max(1e-3);
    (2..=26).map(|k| beats_hz(sync_index_beats(-(k as f32)), bpm)).min_by(|a, b| (a / want).ln().abs().total_cmp(&(b / want).ln().abs())).unwrap_or(want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_indices() {
        assert!((sync_index_beats(-11.0) - 0.5).abs() < 1e-6);
        assert!((sync_index_beats(-14.0) - 1.0).abs() < 1e-6);
        assert!((sync_index_beats(-12.0) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(sync_index_name(-14.0), "1/4");
        assert_eq!(sync_index_name(-12.0), "1/4t");
        assert_eq!(sync_index_name(-13.0), "1/8.");
        assert!((note_div_beats(16.0 / 3.0) - 0.75).abs() < 1e-5);
    }

    #[test]
    fn delay_reads_back() {
        let mut d = Delay::new(100);
        for i in 0..50 {
            d.push(i as f32);
        }
        assert_eq!(d.tap(0), 49.0);
        // as if before pushing sample 50: 10 samples ago = 40
        assert!((d.read(10.0) - 40.0).abs() < 1e-4);
        assert!((d.read(10.5) - 39.5).abs() < 1e-4);
        assert!((d.read_lin(3.0) - 46.0).abs() < 1e-4);
    }

    #[test]
    fn peak_gain_at_centre() {
        let c = Coefs::peak(48000.0, 1000.0, 1.0, 6.0);
        assert!((lg_dsp::gain_to_db(c.magnitude(48000.0, 1000.0)) - 6.0).abs() < 0.01);
    }
}
