//! The distortion family: Distortion, Overdrive, Bitcrusher, Clip Distortion and Exciter —
//! stand-ins for Logic's, on a project's float positions.
//!
//! - Distortion and Overdrive share Logic's layout (Drive dB, Tone Hz, Output dB, Level
//!   Compensation): Distortion clips hard and a little asymmetrically (a diode-like curve),
//!   Overdrive saturates softly with even harmonics (a driven transistor stage). Tone is a
//!   low-pass after the clipper; Level Compensation keeps the loudness about level as Drive
//!   rises. Both run 4x oversampled so high drive doesn't alias.
//! - Bitcrusher (Drive, Clip Level, Mode Fold / Clip / Wrap, Resolution bits, Downsampling,
//!   Mix): deliberately aliasing, as the original.
//! - Clip Distortion (Drive, Tone, Symmetry, Mix, Clip Filter, LP Filter, High Shelving gain and
//!   frequency, Input / Output Gain): high-pass "Tone" into an asymmetric hard clipper whose
//!   output is smoothed by the Clip Filter, mixed with the dry signal, then a low-pass and a
//!   high shelf.
//! - Exciter (Frequency, Color 1 / 2, Harmonics, Dry Signal): adds harmonics generated from
//!   the band above Frequency (Color 1 gentle, Color 2 brighter and harder).

use crate::util::{Biquad, Coefs, Values, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

/// 4x oversampling around a memoryless shaper: zero-stuffed up, two cascaded 4th-order
/// low-passes up and down (at ~0.45 of the original rate).
#[derive(Clone, Copy, Default)]
struct Over4 {
    up: [Biquad; 2],
    down: [Biquad; 2],
}

impl Over4 {
    fn new(rate: f32) -> Over4 {
        let r4 = rate * 4.0;
        let f = rate * 0.45;
        let a = Coefs::lowpass(r4, f, 0.541);
        let b = Coefs::lowpass(r4, f, 1.307);
        Over4 { up: [Biquad::new(a), Biquad::new(b)], down: [Biquad::new(a), Biquad::new(b)] }
    }

    #[inline]
    fn run(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        let mut out = 0.0;
        for k in 0..4 {
            let v = if k == 0 { x * 4.0 } else { 0.0 };
            let u0 = self.up[0].run(v);
            let u = self.up[1].run(u0);
            let y = f(u);
            let d0 = self.down[0].run(y);
            let d = self.down[1].run(d0);
            if k == 3 {
                out = d;
            }
        }
        out
    }

    fn clear(&mut self) {
        for b in self.up.iter_mut().chain(self.down.iter_mut()) {
            b.clear();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Distortion / Overdrive

static DIST_PARAMS: [Param; 4] = [
    Param::new(0, "Drive", 0.0, 50.0, 10.0, Unit::Db),
    Param::new(1, "Tone", 20.0, 20000.0, 1100.0, Unit::Hz),
    Param::new(2, "Output", -40.0, 20.0, -0.5, Unit::Db),
    Param::new(3, "Level Compensation", 0.0, 1.0, 1.0, Unit::OnOff),
];

static OVERDRIVE_PARAMS: [Param; 4] = [
    Param::new(0, "Drive", 0.0, 24.0, 6.0, Unit::Db),
    Param::new(1, "Tone", 20.0, 20000.0, 980.0, Unit::Hz),
    Param::new(2, "Output", -40.0, 20.0, 0.0, Unit::Db),
    Param::new(3, "Level Compensation", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct Drive {
    p: Values,
    rate: f32,
    soft: bool,
    os: [Over4; 2],
    tone: [Biquad; 2],
    pre: f32,
    post: f32,
    dc: [f32; 2],
}

impl Drive {
    pub fn distortion(rate: f32) -> Drive {
        Drive::make(rate, &DIST_PARAMS, false)
    }
    pub fn overdrive(rate: f32) -> Drive {
        Drive::make(rate, &OVERDRIVE_PARAMS, true)
    }
    fn make(rate: f32, defs: &'static [Param], soft: bool) -> Drive {
        let mut d = Drive { p: Values::new(defs), rate, soft, os: [Over4::new(rate); 2], tone: [Biquad::default(); 2], pre: 1.0, post: 1.0, dc: [0.0; 2] };
        d.update();
        d
    }
    fn update(&mut self) {
        let drive = self.p.get(0);
        self.pre = db(drive);
        // level compensation: roughly undo the drive's loudness gain (it grows ~half as fast)
        let comp = if self.p.on(3) { db(-drive * 0.5) } else { 1.0 };
        self.post = db(self.p.get(2)) * comp;
        let c = Coefs::lowpass(self.rate, self.p.get(1).min(self.rate * 0.45), 0.6);
        self.tone = [Biquad::new(c); 2];
    }
    fn clear(&mut self) {
        for s in 0..2 {
            self.os[s].clear();
            self.tone[s].clear();
        }
        self.dc = [0.0; 2];
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

/// Overdrive: a smooth asymmetric saturation (even harmonics).
#[inline]
fn soft_curve(x: f32) -> f32 {
    let y = if x >= 0.0 { x / (1.0 + x) } else { (x * 1.25).tanh() * 0.8 };
    y * 1.15
}

/// Distortion: a harder, diode-like clip with a sharper knee.
#[inline]
fn hard_curve(x: f32) -> f32 {
    let k = 2.5;
    let y = x / (1.0 + (x.abs() * 1.0).powf(k)).powf(1.0 / k);
    if x < 0.0 { y * 0.92 } else { y }
}

impl lg_dsp::Effect for Drive {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (pre, post, soft) = (self.pre, self.post, self.soft);
        let dck = 1.0 - (-crate::util::TAU * 10.0 / self.rate).exp();
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let v = *x * pre;
                let y = self.os[s].run(v, |u| if soft { soft_curve(u) } else { hard_curve(u) });
                // remove the asymmetry's DC
                self.dc[s] = flush(self.dc[s] + dck * (y - self.dc[s]));
                *x = self.tone[s].run(y - self.dc[s]) * post;
            }
        }
    }
}

plugin_impl!(Drive);

// ---------------------------------------------------------------------------------------------
// Bitcrusher

static CRUSH_PARAMS: [Param; 6] = [
    Param::new(0, "Drive", 0.0, 50.0, 3.0, Unit::Db),
    Param::new(1, "Clip Level", -50.0, 0.0, 0.0, Unit::Db),
    Param::menu(2, "Mode", &["Fold", "Clip", "Wrap"], 1.0),
    Param::new(3, "Resolution", 1.0, 24.0, 8.0, Unit::None),
    Param::new(4, "Downsampling", 1.0, 50.0, 1.0, Unit::None),
    Param::new(5, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
];

pub struct Bitcrusher {
    p: Values,
    hold: [f32; 2],
    count: f32,
}

impl Bitcrusher {
    pub fn new(_rate: f32) -> Bitcrusher {
        Bitcrusher { p: Values::new(&CRUSH_PARAMS), hold: [0.0; 2], count: 0.0 }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.hold = [0.0; 2];
        self.count = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for Bitcrusher {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let drive = db(p.get(0));
        let clip = db(p.get(1));
        let mode = p.idx(2);
        let levels = 2f32.powf(p.get(3) - 1.0).max(0.5);
        let ds = p.get(4).max(1.0);
        let mix = p.get(5) / 100.0;
        for i in 0..left.len().min(right.len()) {
            self.count += 1.0;
            let take = self.count >= ds;
            if take {
                self.count -= ds;
            }
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                if take {
                    let mut v = *x * drive;
                    // over the clip level: fold back, clip, or wrap around
                    v = match mode {
                        0 => {
                            let mut f = v;
                            for _ in 0..8 {
                                if f > clip {
                                    f = 2.0 * clip - f;
                                } else if f < -clip {
                                    f = -2.0 * clip - f;
                                } else {
                                    break;
                                }
                            }
                            f.clamp(-clip, clip)
                        }
                        2 => {
                            let w = 2.0 * clip;
                            (v + clip).rem_euclid(w) - clip
                        }
                        _ => v.clamp(-clip, clip),
                    };
                    // quantise (mid-tread) to the resolution
                    self.hold[s] = (v * levels).round() / levels;
                }
                *x = *x * (1.0 - mix) + self.hold[s] * mix;
            }
        }
    }
}

plugin_impl!(Bitcrusher);

// ---------------------------------------------------------------------------------------------
// Clip Distortion

static CLIP_PARAMS: [Param; 10] = [
    Param::new(0, "Drive", 0.0, 72.0, 15.0, Unit::Db),
    Param::new(1, "Tone", 20.0, 20000.0, 170.0, Unit::Hz),
    Param::new(2, "Symmetry", -100.0, 100.0, -16.0, Unit::Percent),
    Param::new(3, "Mix", 0.0, 100.0, 45.0, Unit::Percent),
    Param::new(4, "Clip Filter", 20.0, 20000.0, 4400.0, Unit::Hz),
    Param::new(5, "LP Filter", 20.0, 20000.0, 5300.0, Unit::Hz),
    Param::new(6, "High Shelving Gain", -30.0, 30.0, 0.0, Unit::Db),
    Param::new(7, "High Shelving Frequency", 20.0, 20000.0, 980.0, Unit::Hz),
    Param::new(8, "Input Gain", -30.0, 30.0, 0.0, Unit::Db),
    Param::new(9, "Output Gain", -30.0, 30.0, 5.0, Unit::Db),
];

pub struct ClipDistortion {
    p: Values,
    rate: f32,
    tone: [Biquad; 2],
    clipf: [Biquad; 2],
    lp: [Biquad; 2],
    shelf: [Biquad; 2],
    os: [Over4; 2],
}

impl ClipDistortion {
    pub fn new(rate: f32) -> ClipDistortion {
        let mut c = ClipDistortion { p: Values::new(&CLIP_PARAMS), rate, tone: [Biquad::default(); 2], clipf: [Biquad::default(); 2], lp: [Biquad::default(); 2], shelf: [Biquad::default(); 2], os: [Over4::new(rate); 2] };
        c.update();
        c
    }
    fn update(&mut self) {
        let (r, p) = (self.rate, &self.p);
        let n = |f: f32| f.min(r * 0.45);
        for s in 0..2 {
            self.tone[s].c = Coefs::hp1(r, n(p.get(1)));
            self.clipf[s].c = Coefs::lp1(r, n(p.get(4)));
            self.lp[s].c = Coefs::lowpass(r, n(p.get(5)), 0.707);
            self.shelf[s].c = Coefs::high_shelf(r, n(p.get(7)), 0.707, p.get(6));
        }
    }
    fn clear(&mut self) {
        for s in 0..2 {
            for b in [&mut self.tone[s], &mut self.clipf[s], &mut self.lp[s], &mut self.shelf[s]] {
                b.clear();
            }
            self.os[s].clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for ClipDistortion {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let (gin, drive, gout) = (db(p.get(8)), db(p.get(0)), db(p.get(9)));
        let sym = p.get(2) / 100.0;
        let mix = p.get(3) / 100.0;
        // Symmetry moves the clip points: positive raises the upper one, lowers the lower one
        let (hi, lo) = ((1.0 + sym).max(0.05), (1.0 - sym).max(0.05));
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x * gin;
                let v = self.tone[s].run(dry) * drive;
                let c = self.os[s].run(v, |u| u.clamp(-lo, hi));
                let c = self.clipf[s].run(c) / drive.sqrt().max(1.0);
                let m = dry * (1.0 - mix) + c * mix;
                *x = self.shelf[s].run(self.lp[s].run(m)) * gout;
            }
        }
    }
}

plugin_impl!(ClipDistortion);

// ---------------------------------------------------------------------------------------------
// Exciter

static EXCITER_PARAMS: [Param; 4] = [
    Param::new(0, "Frequency", 1000.0, 12000.0, 2000.0, Unit::Hz),
    Param::menu(1, "Color", &["Color 1", "Color 2"], 0.0),
    Param::new(2, "Harmonics", 0.0, 100.0, 10.6, Unit::Percent),
    Param::new(3, "Dry Signal", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct Exciter {
    p: Values,
    rate: f32,
    hp_in: [[Biquad; 2]; 2],
    hp_out: [[Biquad; 2]; 2],
}

impl Exciter {
    pub fn new(rate: f32) -> Exciter {
        let mut e = Exciter { p: Values::new(&EXCITER_PARAMS), rate, hp_in: [[Biquad::default(); 2]; 2], hp_out: [[Biquad::default(); 2]; 2] };
        e.update();
        e
    }
    fn update(&mut self) {
        let f = self.p.get(0).min(self.rate * 0.4);
        let a = Coefs::highpass(self.rate, f, 0.541);
        let b = Coefs::highpass(self.rate, f, 1.307);
        for s in 0..2 {
            self.hp_in[s] = [Biquad::new(a), Biquad::new(b)];
            self.hp_out[s] = [Biquad::new(a), Biquad::new(b)];
        }
    }
    fn clear(&mut self) {
        for s in 0..2 {
            for b in self.hp_in[s].iter_mut().chain(self.hp_out[s].iter_mut()) {
                b.clear();
            }
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for Exciter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let amt = self.p.get(2) / 100.0 * 2.0;
        let color2 = self.p.idx(1) == 1;
        let dry = if self.p.on(3) { 1.0 } else { 0.0 };
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let h0 = self.hp_in[s][0].run(*x);
                let h = self.hp_in[s][1].run(h0);
                // harmonic generator: the band's distortion minus the band itself
                let d = if color2 { (h * 4.0).clamp(-1.0, 1.0) * 0.5 + h.abs() * 0.5 - h * 0.5 } else { (h * 2.5).tanh() * 0.4 + h * h * 0.6 - h * 0.4 };
                let g0 = self.hp_out[s][0].run(d);
                let g = self.hp_out[s][1].run(g0);
                *x = *x * dry + g * amt;
            }
        }
    }
}

plugin_impl!(Exciter);
