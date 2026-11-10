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
    v
});

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
}

impl ChannelEq {
    pub fn new(rate: f32) -> ChannelEq {
        let mut e = ChannelEq { p: Values::new(&*PARAMS), rate, bands: [BandState::default(); 8], master: Smooth::new(rate, 20.0, 1.0), countdown: 0 };
        e.update();
        e.snap();
        e
    }

    /// The band's processing values from the parameters.
    fn wanted(&self, k: usize) -> (bool, [f32; 3]) {
        let b = 4 * k as u32;
        let on = self.p.on(b);
        let (f, g, q) = (self.p.get(b + 1), self.p.get(b + 2), self.p.get(b + 3));
        let active = on && if k == 0 || k == 7 { g >= 0.5 } else { g.abs() > 0.001 };
        (active, [f, g, q])
    }

    fn update(&mut self) {
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
        b.n = design(self.rate, k, b.now[0], b.now[1], b.now[2], &mut c);
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
        let mut i = 0;
        while i < n {
            if self.countdown == 0 {
                self.glide();
                self.countdown = GLIDE_STEP;
            }
            let run = self.countdown.min(n - i);
            self.countdown -= run;
            for j in i..i + run {
                let (mut l, mut r) = (left[j], right[j]);
                for b in self.bands.iter_mut() {
                    for s in b.sect[..b.n].iter_mut() {
                        l = s[0].run(l);
                        r = s[1].run(r);
                    }
                }
                let m = self.master.next();
                left[j] = l * m;
                right[j] = r * m;
            }
            i += run;
        }
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
