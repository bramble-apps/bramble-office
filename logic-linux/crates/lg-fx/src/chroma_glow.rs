//! Chroma Saturator (working name): the stand-in for Logic's ChromaGlow, an analogue-colour
//! saturator with five models, two styles each.
//!
//! - Retro Tube: a triode stage — asymmetric, grid current softly compressing the positive side,
//!   rich 2nd harmonic, slightly rounded highs.
//! - Modern Tube: a cleaner, symmetric tube stage (mostly 3rd harmonic), extended highs.
//! - Nostalgia Magnetic Tape: tape — pre-emphasis before a soft symmetric (hysteresis-like)
//!   saturation and de-emphasis after, a head bump near 60 Hz and gentle top-end loss.
//! - Squeeze: a compressing saturator; the drive also pushes a fast level-dependent gain
//!   reduction (Style: soft or hard press).
//! - Warm Tape: a darker, thicker tape with more low-mid weight.
//! Drive sets how hard the model is pushed (0-100 %; the loudness is compensated), Saturation the
//! share of the coloured signal, Level In / Out the gains. The optional filter (a high cut) and
//! "Bypass Below" (keeps lows under its frequency out of the saturation) are as Logic's.
//! The model runs 4x oversampled.
//!
//! Parameter positions come from the sample projects' ChromaGlow blocks and Logic's string
//! order; only some are pinned (see the report): treat them as provisional.

use crate::distortion::Over4;
use crate::util::{Biquad, Coefs, Follower, OnePole, Values, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

pub const MODELS: &[&str] = &["Retro Tube", "Modern Tube", "Nostalgia Magnetic Tape", "Squeeze", "Warm Tape"];

static PARAMS: [Param; 13] = [
    Param::menu(1, "Model", MODELS, 0.0),
    Param::menu(2, "Style", &["Style 1", "Style 2"], 0.0),
    Param::new(7, "Level In", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(8, "Level Out", -24.0, 24.0, -1.0, Unit::Db),
    Param::new(9, "Drive", 0.0, 100.0, 35.0, Unit::Percent),
    Param::new(10, "Saturation", 0.0, 100.0, 10.0, Unit::Percent),
    Param::new(12, "Filter On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(15, "Filter Frequency", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(16, "Filter Q", 0.1, 10.0, 0.71, Unit::Ratio),
    Param::new(17, "Bypass Below On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(20, "Bypass Below", 20.0, 2000.0, 500.0, Unit::Hz),
    Param::new(21, "Bypass Below Q", 0.1, 10.0, 0.71, Unit::Ratio),
    Param::new(6, "Mix Mode", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct ChromaGlow {
    p: Values,
    rate: f32,
    os: [Over4; 2],
    pre: [Biquad; 2],
    post: [Biquad; 2],
    bump: [Biquad; 2],
    tone: [OnePole; 2],
    filt: [Biquad; 2],
    split: [Biquad; 2],
    env: Follower,
    hyst: [f32; 2],
    dc: [f32; 2],
    dck: f32,
}

impl ChromaGlow {
    pub fn new(rate: f32) -> ChromaGlow {
        let mut c = ChromaGlow {
            p: Values::new(&PARAMS),
            rate,
            os: [Over4::new(rate); 2],
            pre: [Biquad::default(); 2],
            post: [Biquad::default(); 2],
            bump: [Biquad::default(); 2],
            tone: [OnePole::default(); 2],
            filt: [Biquad::default(); 2],
            split: [Biquad::default(); 2],
            env: Follower::new(rate, 2.0, 80.0),
            hyst: [0.0; 2],
            dc: [0.0; 2],
            dck: 1.0 - (-crate::util::TAU * 8.0 / rate).exp(),
        };
        c.update();
        c
    }

    fn update(&mut self) {
        let (r, p) = (self.rate, &self.p);
        let model = p.idx(1);
        let tape = model == 2 || model == 4;
        // tape emphasis: lift the highs into the saturation, take them back after
        let (pre, post, bump, tone): (Coefs, Coefs, Coefs, f32) = if tape {
            let e = if model == 2 { 6.0 } else { 3.0 };
            (Coefs::high_shelf(r, 3000.0, 0.7, e), Coefs::high_shelf(r, 3000.0, 0.7, -e), Coefs::peak(r, if model == 2 { 62.0 } else { 90.0 }, 1.1, if model == 2 { 2.0 } else { 2.8 }), if model == 2 { 15000.0 } else { 9000.0 })
        } else {
            (Coefs::THRU, Coefs::THRU, Coefs::THRU, if model == 0 { 14000.0 } else { 20000.0 })
        };
        for s in 0..2 {
            self.pre[s].c = pre;
            self.post[s].c = post;
            self.bump[s].c = bump;
            self.tone[s].set(r, tone.min(r * 0.45));
            self.filt[s].c = if p.on(12) { Coefs::lowpass(r, p.get(15).min(r * 0.45), p.get(16)) } else { Coefs::THRU };
            self.split[s].c = Coefs::lowpass(r, p.get(20), p.get(21));
        }
    }

    fn clear(&mut self) {
        for s in 0..2 {
            for b in [&mut self.pre[s], &mut self.post[s], &mut self.bump[s], &mut self.filt[s], &mut self.split[s]] {
                b.clear();
            }
            self.os[s].clear();
            self.tone[s].y = 0.0;
        }
        self.hyst = [0.0; 2];
        self.dc = [0.0; 2];
        self.env.env = 0.0;
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }
}

/// The models' transfer curves (input already driven).
#[inline]
fn curve(model: usize, style: usize, u: f32, hyst: &mut f32) -> f32 {
    match model {
        0 => {
            // triode: the positive side squashes earlier (grid current)
            let a = if style == 0 { 0.35 } else { 0.55 };
            if u >= 0.0 { (u * (1.0 + a)).tanh() / (1.0 + a) } else { u / (1.0 + (u * 0.7).powi(2)).sqrt() }
        }
        1 => {
            let k = if style == 0 { 1.0 } else { 1.6 };
            (u * k).tanh() / k + 0.03 * (u * k).tanh().powi(3)
        }
        2 | 4 => {
            // tape: soft symmetric curve with a little lag (hysteresis-like memory)
            let v = u + 0.12 * (*hyst - u);
            *hyst = flush(v);
            let k = if style == 0 { 1.0 } else { 1.4 };
            (v * k).tanh() / k
        }
        _ => {
            let hard = style == 1;
            if hard { u.clamp(-1.2, 1.2) * 0.85 + 0.15 * u.tanh() } else { u / (1.0 + u.abs()) * 1.3 }
        }
    }
}

impl lg_dsp::Effect for ChromaGlow {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let model = p.idx(1).min(4);
        let style = p.idx(2);
        let drive = db(p.get(9) / 100.0 * 24.0);
        let sat = (p.get(10) / 100.0 * 0.85 + 0.15).min(1.0);
        let (lin, lout) = (db(p.get(7)), db(p.get(8)));
        let bypass_below = p.on(17);
        // loudness compensation for Drive (most of it)
        let comp = 1.0 / drive.powf(0.85);
        for i in 0..left.len().min(right.len()) {
            if model == 3 {
                let a = (left[i].abs().max(right[i].abs())) * lin * drive;
                self.env.run(a);
            }
            let squeeze = if model == 3 { 1.0 / (1.0 + 0.8 * self.env.env.max(0.0)) } else { 1.0 };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let input = *x * lin;
                let (low, body) = if bypass_below {
                    let lo = self.split[s].run(input);
                    (lo, input - lo)
                } else {
                    (0.0, input)
                };
                let v = self.pre[s].run(body) * drive * squeeze;
                let h = &mut self.hyst[s];
                let y = self.os[s].run(v, |u| curve(model, style, u, h));
                let y = self.tone[s].lp(self.bump[s].run(self.post[s].run(y))) * comp / squeeze;
                self.dc[s] = flush(self.dc[s] + self.dck * (y - self.dc[s]));
                let wet = y - self.dc[s];
                let mixed = body * (1.0 - sat) + wet * sat + low;
                *x = self.filt[s].run(mixed) * lout;
            }
        }
    }
}

plugin_impl!(ChromaGlow);
