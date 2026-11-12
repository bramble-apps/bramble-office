//! Channel EQ: the stand-in for Logic's eight-band channel equaliser.
//!
//! Bands, in Logic's order: Low Cut, Low Shelf, Peak 1-4, High Shelf, High Cut, then Master Gain.
//! Each band stores four floats [on, frequency, gain (cuts: slope), Q] starting at index 0 (the
//! Low Cut's on switch is the first float of the block), so band k sits at 4k..4k+3 and Master
//! Gain at 32 — exactly as a project's Channel EQ block is laid out.
//!
//! - The cut slopes are Logic's values: the stored number times 6 dB/oct (1 = 6, 2 = 12, 3 = 18,
//!   4 = 24, 6 = 36, 8 = 48). Built from Butterworth sections (one first-order section for odd
//!   orders); the band's Q sets the resonance of the last section, as Logic's cut Q does
//!   (0.71 = no bump).
//! - Shelves and peaks are RBJ shelving / peaking biquads with Logic's Q.
//! - Coefficients glide over ~20 ms when a value changes, so sweeping a band doesn't click.
//!
//! Matched against Logic's renders of factory settings (tests/reference.rs): within 0.1 dB for a
//! mastering curve of shelves and peaks. Known difference: Logic's strongly resonant cuts
//! (Q well above 0.71) reach about 3 dB deeper into their stop band here, and their bump is
//! about 1 dB lower.
//!
//! Not modelled: the analyzer, Gain-Q coupling, the mid/side channel modes and the display
//! settings stored after Master Gain (not part of this block's parameters here).

use crate::util::{Biquad, Coefs, Smooth, Values, db, plugin_impl};
use lg_dsp::{Param, Unit};
use std::sync::LazyLock;

const SLOPES: &[&str] = &["Off", "6 dB/Oct", "12 dB/Oct", "18 dB/Oct", "24 dB/Oct", "30 dB/Oct", "36 dB/Oct", "42 dB/Oct", "48 dB/Oct"];

/// The eight bands' names, in Logic's order.
const BANDS: [&str; 8] = ["Low Cut", "Low Shelf", "Peak 1", "Peak 2", "Peak 3", "Peak 4", "High Shelf", "High Cut"];
/// Logic's default Channel EQ (a new instance): (on, frequency, gain or slope, Q).
const DEFAULTS: [(f32, f32, f32, f32); 8] = [
    (0.0, 20.0, 2.0, 0.71),
    (1.0, 75.0, 0.0, 1.0),
    (1.0, 100.0, 0.0, 0.6),
    (1.0, 250.0, 0.0, 0.3),
    (1.0, 1040.0, 0.0, 0.41),
    (1.0, 2500.0, 0.0, 0.2),
    (1.0, 7500.0, 0.0, 1.0),
    (0.0, 20000.0, 4.0, 0.71),
];

static PARAMS: LazyLock<Vec<Param>> = LazyLock::new(|| {
    let mut v = Vec::new();
    let name = |b: &str, what: &str| -> &'static str { Box::leak(format!("{b} {what}").into_boxed_str()) };
    for (k, (b, d)) in BANDS.iter().zip(DEFAULTS).enumerate() {
        let id = 4 * k as u32;
        v.push(Param::new(id, name(b, "On/Off"), 0.0, 1.0, d.0, Unit::OnOff));
        v.push(Param::new(id + 1, name(b, "Frequency"), 20.0, 20000.0, d.1, Unit::Hz));
        if k == 0 || k == 7 {
            v.push(Param::menu(id + 2, name(b, "Slope"), SLOPES, d.2));
        } else {
            v.push(Param::new(id + 2, name(b, "Gain"), -24.0, 24.0, d.2, Unit::Db));
        }
        v.push(Param::new(id + 3, name(b, "Q-Factor"), 0.1, 100.0, d.3, Unit::Ratio));
    }
    v.push(Param::new(32, "Master Gain", -24.0, 24.0, 0.0, Unit::Db));
    // the window's controls and display options (our own ids: Logic stores them after the
    // block, not decoded)
    v.extend([
        Param::new(ANALYZER, "Analyzer", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(ANALYZER_POST, "Analyzer Pre/Post", &["Pre", "Post"], 1.0),
        Param::menu(ANALYZER_MODE, "Analyzer Mode", &["Peak", "RMS"], 0.0),
        Param::menu(ANALYZER_RES, "Analyzer Resolution", &["Low (2048)", "Medium (4096)", "High (8192)"], 1.0),
        Param::new(ANALYZER_DECAY, "Analyzer Decay", 1.0, 100.0, 20.0, Unit::None),
        Param::new(ANALYZER_TOP, "Analyzer Top", -80.0, 20.0, 0.0, Unit::Db),
        Param::menu(ANALYZER_RANGE, "Analyzer Range", &["60 dB", "80 dB", "100 dB"], 0.0),
        Param::new(Q_COUPLE, "Q-Couple", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(Q_STRENGTH, "Gain-Q Couple Strength", COUPLINGS, 2.0),
        Param::new(HQ, "HQ", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(PROCESSING, "Processing", PROCESSINGS, 0.0),
        Param::menu(SCALE_MODE, "EQ dB Scale Mode", SCALE_MODES, 1.0),
        Param::new(SCALE_OFFSET, "EQ Scale Offset", -30.0, 30.0, 0.0, Unit::Db),
        Param::new(VIS_MASTER, "Visualize Master Gain", 0.0, 1.0, 0.0, Unit::OnOff),
    ]);
    v
});

pub const ANALYZER: u32 = 1000;
pub const ANALYZER_POST: u32 = 1001;
pub const ANALYZER_MODE: u32 = 1002;
pub const ANALYZER_RES: u32 = 1003;
pub const ANALYZER_DECAY: u32 = 1004;
pub const ANALYZER_TOP: u32 = 1005;
pub const ANALYZER_RANGE: u32 = 1006;
pub const Q_COUPLE: u32 = 1007;
pub const Q_STRENGTH: u32 = 1008;
pub const HQ: u32 = 1009;
pub const PROCESSING: u32 = 1010;
pub const SCALE_MODE: u32 = 1011;
pub const SCALE_OFFSET: u32 = 1012;
pub const VIS_MASTER: u32 = 1013;
pub const COUPLINGS: &[&str] = &["Proportional", "Light", "Medium", "Strong", "Asymmetric Light", "Asymmetric Medium", "Asymmetric Strong"];
pub const PROCESSINGS: &[&str] = &["Stereo", "Left Only", "Right Only", "Mid Only", "Side Only"];
pub const SCALE_MODES: &[&str] = &["Linear 12 dB", "Linear 30 dB", "Linear 60 dB", "Warped"];

/// Gain-Q coupling: the Q a peak band plays with for its gain (the stored Q is kept; a change
/// of strength changes the played Q, as Logic's automation note says). Proportional widens
/// small boosts / cuts and narrows big ones; the others narrow with gain, the asymmetric ones
/// more for cuts than boosts.
pub fn coupled_q(q: f32, gain: f32, strength: usize) -> f32 {
    let g = gain.abs();
    let f = match strength {
        0 => (g / 6.0).max(0.25).sqrt(),
        1..=3 => 1.0 + [0.0, 0.25, 0.5, 1.0][strength] * g / 12.0,
        _ => {
            let s = [0.25, 0.5, 1.0][(strength - 4).min(2)];
            1.0 + if gain < 0.0 { s } else { s * 0.4 } * g / 12.0
        }
    };
    (q * f).clamp(0.1, 100.0)
}

/// The analyzer (an FFT of the input or output; Peak or RMS; 2048 / 4096 / 8192 points; a
/// falling display): 200 log-spaced bands from 20 Hz to 20 kHz, in dB.
#[derive(Clone)]
pub struct Analyzer {
    ring: Vec<f32>,
    at: usize,
    since: usize,
    pub bands: Vec<f32>,
}

pub const ANALYZER_BANDS: usize = 200;

impl Default for Analyzer {
    fn default() -> Analyzer {
        Analyzer { ring: vec![0.0; 8192], at: 0, since: 0, bands: vec![-120.0; ANALYZER_BANDS] }
    }
}

impl Analyzer {
    /// Band k's centre (Hz).
    pub fn band_hz(k: usize) -> f32 {
        20.0 * 1000f32.powf(k as f32 / (ANALYZER_BANDS - 1) as f32)
    }
    pub fn push(&mut self, l: &[f32], r: &[f32], rate: f32, size: usize, rms: bool, decay_db_s: f32) {
        let n = size.clamp(2048, 8192);
        for (a, b) in l.iter().zip(r) {
            self.ring[self.at] = 0.5 * (a + b);
            self.at = (self.at + 1) % self.ring.len();
            self.since += 1;
            if self.since >= n / 4 {
                self.since = 0;
                self.frame(n, rate, rms, decay_db_s * (n / 4) as f32 / rate);
            }
        }
    }
    fn frame(&mut self, n: usize, rate: f32, rms: bool, fall: f32) {
        let len = self.ring.len();
        let mut buf: Vec<crate::convolve::C> = (0..n).map(|i| {
            let x = self.ring[(self.at + len - n + i) % len];
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            crate::convolve::C { re: x * w, im: 0.0 }
        }).collect();
        crate::convolve::Fft::new(n).run(&mut buf, false);
        // (Hann's coherent gain is 0.5: a full-scale sine reads 0 dB)
        let mag = |k: usize| (buf[k].re * buf[k].re + buf[k].im * buf[k].im).sqrt() * 4.0 / n as f32;
        let bin = rate / n as f32;
        for b in 0..ANALYZER_BANDS {
            let (lo, hi) = (Self::band_hz(b) * 2f32.powf(-1.0 / 40.0), Self::band_hz(b) * 2f32.powf(1.0 / 40.0));
            let (k0, k1) = (((lo / bin).floor() as usize).max(1), ((hi / bin).ceil() as usize).min(n / 2));
            let ks = k0..=k1.max(k0);
            let v = if rms {
                let c = ks.clone().count() as f32;
                (ks.map(|k| mag(k).powi(2)).sum::<f32>() / c).sqrt()
            } else {
                ks.map(mag).fold(0.0, f32::max)
            };
            let d = 20.0 * v.max(1e-7).log10();
            // a falling display: rises at once, falls at the decay rate
            self.bands[b] = d.max(self.bands[b] - fall);
        }
    }
}

/// Two-times oversampling (HQ): a 31-tap half-band low-pass for the way up and down.
#[derive(Clone)]
struct Halfband {
    taps: Vec<f32>,
    z: Vec<f32>,
    at: usize,
}

impl Halfband {
    fn new() -> Halfband {
        let n = 31;
        let taps = (0..n).map(|i| {
            let m = i as f32 - 15.0;
            let sinc = if m == 0.0 { 0.5 } else { (std::f32::consts::PI * m * 0.5).sin() / (std::f32::consts::PI * m) };
            let w = 0.42 - 0.5 * (std::f32::consts::TAU * i as f32 / 30.0).cos() + 0.08 * (2.0 * std::f32::consts::TAU * i as f32 / 30.0).cos();
            sinc * w
        }).collect();
        Halfband { taps, z: vec![0.0; n], at: 0 }
    }
    fn run(&mut self, x: f32) -> f32 {
        let n = self.z.len();
        self.z[self.at] = x;
        let mut y = 0.0;
        for (i, t) in self.taps.iter().enumerate() {
            y += t * self.z[(self.at + n - i) % n];
        }
        self.at = (self.at + 1) % n;
        y
    }
    fn clear(&mut self) {
        self.z.iter_mut().for_each(|v| *v = 0.0);
    }
}

/// Splits a stereo pair for the Processing mode: the two channels the bands run on.
#[inline]
pub fn to_proc(mode: usize, l: f32, r: f32) -> (f32, f32) {
    if mode >= 3 { (0.5 * (l + r), 0.5 * (l - r)) } else { (l, r) }
}

/// Back to left / right: only the processed part of the pair is taken from the EQ (Left Only,
/// Right Only, Mid Only, Side Only).
#[inline]
pub fn from_proc(mode: usize, (a, b): (f32, f32), (ea, eb): (f32, f32)) -> (f32, f32) {
    let (x, y) = match mode {
        1 => (ea, b),
        2 => (a, eb),
        3 => (ea, b),
        4 => (a, eb),
        _ => (ea, eb),
    };
    if mode >= 3 { (x + y, x - y) } else { (x, y) }
}

/// Logic's shelf Q-Factor against the cookbook's shelf Q (matched to Logic's renders: a shelf at
/// Q 0.1 spreads over about two octaves, not five). Peaks use Logic's Q as is.
const Q_SCALE: f32 = 2.0;


/// Most sections one band can need (48 dB/oct = 4 biquads).
const MAX_SECT: usize = 4;
/// Samples between coefficient updates while gliding.
const GLIDE_STEP: usize = 16;

/// Butterworth section Qs for an order-`n` filter (ascending, so the most resonant section
/// comes last), and whether there's a first-order section.
fn butterworth(n: usize, out: &mut [f32; MAX_SECT]) -> (usize, bool) {
    let pairs = n / 2;
    for k in 0..pairs {
        let phi = if n % 2 == 0 { std::f32::consts::PI * (2 * k + 1) as f32 / (2 * n) as f32 } else { std::f32::consts::PI * (k + 1) as f32 / n as f32 };
        out[k] = 1.0 / (2.0 * phi.cos());
    }
    out[..pairs].sort_by(|a, b| a.partial_cmp(b).unwrap());
    (pairs, n % 2 == 1)
}

/// The sections for band `k` given its settings (`None` = not processing).
fn design(rate: f32, k: usize, f: f32, g: f32, q: f32, out: &mut [Coefs; MAX_SECT + 1]) -> usize {
    match k {
        0 | 7 => {
            let n = (g.round() as usize).min(8);
            if n == 0 {
                return 0;
            }
            let mut qs = [0.0; MAX_SECT];
            let (pairs, odd) = butterworth(n, &mut qs);
            let mut m = 0;
            if odd {
                out[m] = if k == 0 { Coefs::hp1(rate, f) } else { Coefs::lp1(rate, f) };
                m += 1;
            }
            for (i, &qb) in qs[..pairs].iter().enumerate() {
                // the last (most resonant) section takes the band's Q, relative to Butterworth
                let qq = if i + 1 == pairs { qb * q / std::f32::consts::FRAC_1_SQRT_2 } else { qb };
                out[m] = if k == 0 { Coefs::highpass(rate, f, qq) } else { Coefs::lowpass(rate, f, qq) };
                m += 1;
            }
            m
        }
        1 => {
            out[0] = Coefs::low_shelf(rate, f, q * Q_SCALE, g);
            1
        }
        6 => {
            out[0] = Coefs::high_shelf(rate, f, q * Q_SCALE, g);
            1
        }
        _ => {
            out[0] = Coefs::peak(rate, f, q, g);
            1
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BandState {
    n: usize,
    sect: [[Biquad; 2]; MAX_SECT + 1],
    /// Gliding: where the band's three values are now (freq, gain, q) and where they go.
    now: [f32; 3],
    target: [f32; 3],
    /// The band is active now (on, and a shelf / peak with gain, or a cut).
    active: bool,
}

pub struct ChannelEq {
    p: Values,
    rate: f32,
    bands: [BandState; 8],
    master: Smooth,
    countdown: usize,
    /// HQ: the filters run at twice the rate
    os: usize,
    up: [Halfband; 2],
    down: [Halfband; 2],
    pub analyzer: Analyzer,
}

impl ChannelEq {
    pub fn new(rate: f32) -> ChannelEq {
        let mut e = ChannelEq { p: Values::new(&*PARAMS), rate, bands: [BandState::default(); 8], master: Smooth::new(rate, 20.0, 1.0), countdown: 0, os: 1, up: [Halfband::new(), Halfband::new()], down: [Halfband::new(), Halfband::new()], analyzer: Analyzer::default() };
        e.update();
        e.snap();
        e
    }

    /// The band's processing values from the parameters.
    fn wanted(&self, k: usize) -> (bool, [f32; 3]) {
        let b = 4 * k as u32;
        let on = self.p.on(b);
        let (f, g, mut q) = (self.p.get(b + 1), self.p.get(b + 2), self.p.get(b + 3));
        if (2..=5).contains(&k) && self.p.on(Q_COUPLE) {
            q = coupled_q(q, g, self.p.idx(Q_STRENGTH));
        }
        let active = on && if k == 0 || k == 7 { g >= 0.5 } else { g.abs() > 0.001 };
        (active, [f, g, q])
    }

    fn update(&mut self) {
        let os = if self.p.on(HQ) { 2 } else { 1 };
        if os != self.os {
            self.os = os;
            for b in self.bands.iter_mut() {
                b.active = !b.active;
            }
            for h in self.up.iter_mut().chain(self.down.iter_mut()) {
                h.clear();
            }
        }
        for k in 0..8 {
            let (active, t) = self.wanted(k);
            let b = &mut self.bands[k];
            b.target = t;
            if active != b.active || ((k == 0 || k == 7) && t[1].round() != b.now[1].round()) {
                // switching on / off or changing the cut order: jump (and restart the filters)
                b.active = active;
                b.now = t;
                for s in b.sect.iter_mut() {
                    s[0].clear();
                    s[1].clear();
                }
                self.redesign(k);
            }
        }
        self.master.target = db(self.p.get(32));
    }

    fn snap(&mut self) {
        for k in 0..8 {
            self.bands[k].now = self.bands[k].target;
            self.redesign(k);
        }
        self.master.snap();
    }

    fn redesign(&mut self, k: usize) {
        let b = &mut self.bands[k];
        if !b.active {
            b.n = 0;
            return;
        }
        let mut c = [Coefs::THRU; MAX_SECT + 1];
        b.n = design(self.rate * self.os as f32, k, b.now[0], b.now[1], b.now[2], &mut c);
        for i in 0..b.n {
            b.sect[i][0].c = c[i];
            b.sect[i][1].c = c[i];
        }
    }

    /// Moves gliding bands a step towards their targets (frequency and Q on a log scale).
    fn glide(&mut self) {
        for k in 0..8 {
            let b = &mut self.bands[k];
            if !b.active || b.now == b.target {
                continue;
            }
            let a = 0.25;
            let fl = b.now[0].ln() + (b.target[0].ln() - b.now[0].ln()) * a;
            let ql = b.now[2].ln() + (b.target[2].ln() - b.now[2].ln()) * a;
            let g = b.now[1] + (b.target[1] - b.now[1]) * a;
            b.now = [fl.exp(), g, ql.exp()];
            if (b.now[0] / b.target[0] - 1.0).abs() < 1e-4 && (b.now[1] - b.target[1]).abs() < 1e-3 && (b.now[2] / b.target[2] - 1.0).abs() < 1e-4 {
                b.now = b.target;
            }
            self.redesign(k);
        }
    }

    fn clear(&mut self) {
        for b in self.bands.iter_mut() {
            for s in b.sect.iter_mut() {
                s[0].clear();
                s[1].clear();
            }
        }
        self.snap();
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }

    /// The EQ's response in dB at `f` with the current settings (for the window's curve and tests).
    pub fn response_db(&self, f: f32) -> f32 {
        let mut g = self.p.get(32);
        for k in 0..8 {
            let (active, t) = self.wanted(k);
            if !active {
                continue;
            }
            let mut c = [Coefs::THRU; MAX_SECT + 1];
            let n = design(self.rate, k, t[0], t[1], t[2], &mut c);
            for s in &c[..n] {
                g += lg_dsp::gain_to_db(s.magnitude(self.rate, f));
            }
        }
        g
    }
}

impl lg_dsp::Effect for ChannelEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mode = self.processing();
        let (analyze, post, size, rms, decay) = self.analyzer_settings();
        if analyze && !post {
            self.analyzer.push(left, right, self.rate, size, rms, decay);
        }
        let mut i = 0;
        while i < n {
            if self.countdown == 0 {
                self.glide();
                self.countdown = GLIDE_STEP;
            }
            let run = self.countdown.min(n - i);
            self.countdown -= run;
            for j in i..i + run {
                let (a, b) = to_proc(mode, left[j], right[j]);
                let (ea, eb) = if self.os == 2 {
                    // HQ: up (zero-stuffed, half-band), the bands at 2x, down
                    let mut out = (0.0, 0.0);
                    for phase in 0..2 {
                        let (xa, xb) = if phase == 0 { (a * 2.0, b * 2.0) } else { (0.0, 0.0) };
                        let (ua, ub) = (self.up[0].run(xa), self.up[1].run(xb));
                        let (ya, yb) = self.run_bands(ua, ub);
                        let (da, db_) = (self.down[0].run(ya), self.down[1].run(yb));
                        if phase == 0 {
                            out = (da, db_);
                        }
                    }
                    out
                } else {
                    self.run_bands(a, b)
                };
                let (l, r) = from_proc(mode, (a, b), (ea, eb));
                let m = self.master.next();
                left[j] = l * m;
                right[j] = r * m;
            }
            i += run;
        }
        if analyze && post {
            self.analyzer.push(left, right, self.rate, size, rms, decay);
        }
    }
    fn latency(&self) -> usize {
        // (the two half-bands at twice the rate)
        if self.os == 2 { 15 } else { 0 }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        if name == "analyzer" { self.analyzer.bands.clone() } else { Vec::new() }
    }
}

impl ChannelEq {
    #[inline]
    fn run_bands(&mut self, mut l: f32, mut r: f32) -> (f32, f32) {
        for b in self.bands.iter_mut() {
            for s in b.sect[..b.n].iter_mut() {
                l = s[0].run(l);
                r = s[1].run(r);
            }
        }
        (l, r)
    }

    /// The analyzer's settings: (on, post, points, rms, decay dB/s).
    pub fn analyzer_settings(&self) -> (bool, bool, usize, bool, f32) {
        (self.p.on(ANALYZER), self.p.idx(ANALYZER_POST) == 1, [2048, 4096, 8192][self.p.idx(ANALYZER_RES).min(2)], self.p.idx(ANALYZER_MODE) == 1, self.p.get(ANALYZER_DECAY))
    }

    /// One band's response in dB at `f` (its own curve in the window), None when it's inactive.
    pub fn band_db(&self, k: usize, f: f32) -> Option<f32> {
        let (active, t) = self.wanted(k);
        if !active {
            return None;
        }
        let mut c = [Coefs::THRU; MAX_SECT + 1];
        let n = design(self.rate, k, t[0], t[1], t[2], &mut c);
        Some(c[..n].iter().map(|s| lg_dsp::gain_to_db(s.magnitude(self.rate, f))).sum())
    }

    pub fn processing(&self) -> usize {
        self.p.idx(PROCESSING)
    }
}

plugin_impl!(ChannelEq);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    fn sine_gain(e: &mut ChannelEq, f: f32) -> f32 {
        let rate = 48000.0;
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| (crate::util::TAU * f * i as f32 / rate).sin() * 0.25).collect();
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        lg_dsp::gain_to_db(rms(&l[24000..]) / (0.25 / 2f32.sqrt()))
    }

    #[test]
    fn processing_modes_hq_analyzer_and_coupling() {
        let rate = 48000.0;
        let tone = |hz: f32, a: f32| -> Vec<f32> { (0..24000).map(|i| (crate::util::TAU * hz * i as f32 / rate).sin() * a).collect() };
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let mut e = ChannelEq::new(rate);
        e.set(Q_COUPLE, 0.0);
        e.set(17, 1000.0);
        e.set(18, -12.0);
        e.reset();
        // Left Only: the right side passes untouched
        e.set(PROCESSING, 1.0);
        let (mut l, mut r) = (tone(1000.0, 0.5), tone(1000.0, 0.5));
        e.process(&mut l, &mut r);
        assert!(rms(&l[12000..]) < 0.3 * 0.354 && (rms(&r[12000..]) - 0.354).abs() < 0.01);
        // Side Only: a mono (mid-only) signal passes; Mid Only cuts it
        e.set(PROCESSING, 4.0);
        e.reset();
        let (mut l, mut r) = (tone(1000.0, 0.5), tone(1000.0, 0.5));
        e.process(&mut l, &mut r);
        assert!((rms(&l[12000..]) - 0.354).abs() < 0.01);
        e.set(PROCESSING, 3.0);
        e.reset();
        let (mut l, mut r) = (tone(1000.0, 0.5), tone(1000.0, 0.5));
        e.process(&mut l, &mut r);
        assert!(rms(&l[12000..]) < 0.12);
        // HQ: the same response, 15 samples later
        e.set(PROCESSING, 0.0);
        e.set(HQ, 1.0);
        e.reset();
        assert_eq!(lg_dsp::Effect::latency(&e), 15);
        let (mut l, mut r) = (tone(1000.0, 0.5), tone(1000.0, 0.5));
        e.process(&mut l, &mut r);
        let g = lg_dsp::gain_to_db(rms(&l[12000..]) / 0.3536);
        assert!((g + 12.0).abs() < 0.3, "HQ {g}");
        // the analyzer reads a full-scale-ish sine near its level
        let mut a = ChannelEq::new(rate);
        let (mut l, mut r) = (tone(1000.0, 0.5), tone(1000.0, 0.5));
        a.process(&mut l, &mut r);
        let k = (0..ANALYZER_BANDS).min_by(|x, y| (Analyzer::band_hz(*x) - 1000.0).abs().total_cmp(&(Analyzer::band_hz(*y) - 1000.0).abs())).unwrap();
        let peak = a.curve("analyzer")[k];
        assert!((peak + 6.0).abs() < 1.5, "{peak}");
        // coupling: stronger narrows more; asymmetric narrows cuts more than boosts
        assert!(coupled_q(1.0, 12.0, 3) > coupled_q(1.0, 12.0, 1));
        assert!(coupled_q(1.0, -12.0, 6) > coupled_q(1.0, 12.0, 6));
        assert!(coupled_q(1.0, 1.0, 0) < 1.0 && coupled_q(1.0, 18.0, 0) > 1.0);
    }

    #[test]
    fn default_is_flat() {
        let mut e = ChannelEq::new(48000.0);
        for f in [50.0, 1000.0, 10000.0] {
            assert!(sine_gain(&mut e, f).abs() < 0.05);
        }
    }

    #[test]
    fn peak_boost_measured_at_its_frequency() {
        let mut e = ChannelEq::new(48000.0);
        e.set(17, 1000.0);
        e.set(18, 9.0);
        e.set(19, 2.0);
        e.reset();
        let g = sine_gain(&mut e, 1000.0);
        assert!((g - 9.0).abs() < 0.2, "{g}");
        assert!((e.response_db(1000.0) - 9.0).abs() < 0.05);
        assert!(sine_gain(&mut e, 100.0).abs() < 0.3);
    }

    #[test]
    fn cut_slopes() {
        let mut e = ChannelEq::new(48000.0);
        e.set(0, 1.0);
        e.set(1, 1000.0);
        for (s, dbo) in [(1.0, 6.0), (2.0, 12.0), (3.0, 18.0), (4.0, 24.0), (6.0, 36.0), (8.0, 48.0)] {
            e.set(2, s);
            // two and three octaves below the corner: the asymptotic slope
            let d = e.response_db(125.0) - e.response_db(250.0);
            assert!((d + dbo).abs() < 1.5, "slope {s}: {d}");
            assert!((e.response_db(1000.0) + 3.0).abs() < 0.3, "slope {s} at fc: {}", e.response_db(1000.0));
        }
        e.set(29, 30.0);
    }

    #[test]
    fn stable_on_noise_every_slope() {
        let mut e = ChannelEq::new(44100.0);
        e.load_floats(&[1.0, 30.0, 8.0, 5.0, 1.0, 20000.0, 24.0, 0.1, 1.0, 20.0, -24.0, 100.0]);
        for s in 0..=8 {
            e.set(2, s as f32);
            e.set(30, s as f32);
            e.set(28, 1.0);
            e.set(29, 300.0);
            let mut nz = crate::util::Noise(7);
            let mut l: Vec<f32> = (0..20000).map(|_| nz.next()).collect();
            let mut r = l.clone();
            e.process(&mut l, &mut r);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 100.0));
        }
    }
}
