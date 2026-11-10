//! Space Convolver (working name): the stand-in for Logic's Space Designer, a convolution
//! reverb whose impulse responses we synthesise ourselves (Logic's sampled IRs are Apple's
//! recordings and aren't used).
//!
//! The response is built the way Space Designer's "Synthesized IR" mode describes a space:
//! - Length and Size set its duration; Init / End Density set how sparse the first
//!   reflections are and how quickly they thicken into a smooth tail.
//! - The volume envelope (initial level, attack time, decay time, end level) shapes it; the
//!   filter envelope sweeps a low-pass over it (initial, maximum and end cutoff), giving the
//!   natural darkening of a real tail; Filter Mode and Resonance as Logic's.
//! - Stereo Spread decorrelates left and right (separately below and above ~300 Hz).
//! - Reverse plays it backwards; Predelay delays it.
//! - Output Dry / Wet in dB.
//! The responses are normalised to a fixed energy so Wet behaves alike for every length.
//! The convolution itself is zero-latency (see convolve.rs).
//!
//! Parameter positions follow the research map where it anchors them (Predelay, Reverse, the
//! envelopes, filter, densities, spreads, Size); Output Dry / Wet and Length are placed at the
//! unlabelled floats next to Predelay and are unconfirmed. Responses are regenerated when a
//! setting changes (in `set`, not in `process`).

use crate::convolve::Convolver;
use crate::util::{Delay, Noise, Values, db, plugin_impl};
use lg_dsp::{Param, Unit};

const DRY: u32 = 19;
const WET: u32 = 20;
const PREDELAY: u32 = 21;
const LENGTH: u32 = 22;
const REVERSE: u32 = 27;
const V_INIT: u32 = 28;
const V_ATTACK: u32 = 29;
const V_DECAY: u32 = 34;
const V_END: u32 = 39;
const F_INIT: u32 = 40;
const F_ATTACK: u32 = 41;
const F_MAX: u32 = 46;
const F_DECAY: u32 = 47;
const F_END: u32 = 52;
const FILTER: u32 = 54;
const F_MODE: u32 = 55;
const F_RES: u32 = 57;
const SPREAD_LO: u32 = 60;
const INIT_DENS: u32 = 63;
const END_DENS: u32 = 66;
const SIZE: u32 = 104;
const SPREAD_HI: u32 = 110;

const MODES: &[&str] = &["HP 12dB", "BP 6dB", "LP 6dB", "LP 12dB", "LP 24dB"];

static PARAMS: [Param; 22] = [
    Param::new(DRY, "Output Dry", -96.0, 6.0, 0.0, Unit::Db),
    Param::new(WET, "Output Wet", -96.0, 6.0, -10.0, Unit::Db),
    Param::new(PREDELAY, "Predelay", 0.0, 250.0, 8.0, Unit::Ms),
    Param::new(LENGTH, "Length", 5.0, 100.0, 71.0, Unit::Percent),
    Param::new(REVERSE, "Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(V_INIT, "Volume Init Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(V_ATTACK, "Volume Attack Time", 0.0, 1.0, 0.059, Unit::None),
    Param::new(V_DECAY, "Volume Decay Time", 0.0, 1.0, 1.0, Unit::None),
    Param::new(V_END, "Volume End Level", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(F_INIT, "Filter Init Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(F_ATTACK, "Filter Attack Time", 0.0, 1.0, 0.219, Unit::None),
    Param::new(F_MAX, "Filter Max Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(F_DECAY, "Filter Decay Time", 0.0, 1.0, 1.0, Unit::None),
    Param::new(F_END, "Filter End Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(FILTER, "Filter", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(F_MODE, "Filter Mode", MODES, 2.0),
    Param::new(F_RES, "Filter Resonance", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(SPREAD_LO, "Stereo Spread Low", 0.0, 2.0, 0.38, Unit::None),
    Param::new(INIT_DENS, "Init Density", 0.0, 100.0, 88.0, Unit::Percent),
    Param::new(END_DENS, "End Density", 0.0, 1.0, 1.0, Unit::None),
    Param::new(SIZE, "Size", 0.1, 2.0, 1.0, Unit::None),
    Param::new(SPREAD_HI, "Stereo Spread High", 0.0, 2.0, 1.0, Unit::None),
];

/// The synthesised space's natural length (s) at Length 100 %, Size 100 %.
const BASE_SECS: f32 = 2.6;
const MAX_SECS: f32 = 6.0;
/// Impulse-response energy (dB): matched so Wet -10 dB gives Logic's default level.
const IR_ENERGY_DB: f32 = 2.5;

pub struct SpaceConvolver {
    p: Values,
    rate: f32,
    conv: [Convolver; 2],
    ir: [Vec<f32>; 2],
    ir_len: usize,
    pre: [Delay; 2],
    pre_len: f32,
    dry: f32,
    wet: f32,
}

impl SpaceConvolver {
    pub fn new(rate: f32) -> SpaceConvolver {
        let max = (MAX_SECS * rate) as usize;
        let mut s = SpaceConvolver {
            p: Values::new(&PARAMS),
            rate,
            conv: [Convolver::new(max), Convolver::new(max)],
            ir: [vec![0.0; max], vec![0.0; max]],
            ir_len: 0,
            pre: [Delay::new((0.26 * rate) as usize), Delay::new((0.26 * rate) as usize)],
            pre_len: 1.0,
            dry: 1.0,
            wet: 0.3,
        };
        s.update();
        s
    }

    /// Builds the stereo response into `self.ir` (no allocation: the buffers are full size).
    fn synthesise(&mut self) {
        let p = &self.p;
        let rate = self.rate;
        let secs = (BASE_SECS * p.get(LENGTH) / 100.0 * p.get(SIZE)).clamp(0.05, MAX_SECS);
        let n = ((secs * rate) as usize).min(self.ir[0].len());
        self.ir_len = n;
        let (vi, va, vd, ve) = (p.get(V_INIT) / 100.0, p.get(V_ATTACK), p.get(V_DECAY), p.get(V_END) / 100.0);
        let (fi, fa, fm, fd, fe) = (p.get(F_INIT) / 100.0, p.get(F_ATTACK), p.get(F_MAX) / 100.0, p.get(F_DECAY), p.get(F_END) / 100.0);
        let filter = p.on(FILTER);
        let mode = p.idx(F_MODE);
        let res = p.get(F_RES) / 100.0;
        let (d0, d1) = (p.get(INIT_DENS) / 100.0, p.get(END_DENS).clamp(0.0, 1.0));
        let (s_lo, s_hi) = (p.get(SPREAD_LO).clamp(0.0, 2.0) / 2.0, p.get(SPREAD_HI).clamp(0.0, 2.0) / 2.0);
        let att_n = (va * 0.5 * secs * rate) as usize;
        // decay: the volume falls by ~60 dB over (decay time x length), ending at End Level
        let dec_n = ((vd.max(0.02)) * n as f32).max(1.0);
        let mut nz = [Noise(0x1234_5678), Noise(0x9abc_def1), Noise(0x0f0f_1357)];
        // state of the per-channel time-varying filters and the low/high split for the spread
        let mut lp_state = [[0.0f32; 4]; 2];
        let mut split = [0.0f32; 3];
        let split_a = 1.0 - (-crate::util::TAU * 300.0 / rate).exp();
        for i in 0..n {
            let t = i as f32 / n as f32;
            // volume envelope: attack from the initial level, then an exponential fall that
            // reaches a tenth of End Level when the decay time is over (and keeps falling), with a
            // short fade at the very end
            let att = if i < att_n { vi + (1.0 - vi) * (i as f32 / att_n.max(1) as f32) } else { 1.0 };
            let k = (i.saturating_sub(att_n)) as f32 / dec_n;
            let floor = (ve * 0.1).max(1e-3);
            let tailfade = 1.0 - ((t - 0.85).max(0.0) / 0.15).min(1.0);
            let vol = att * floor.powf(k) * tailfade;
            // density: early on, only some samples carry reflections (energy kept the same)
            let dens = d0 + (d1 - d0) * (t * 6.0).min(1.0);
            let sparse = (dens * dens * dens).max(0.002);
            let keep = (nz[2].next() * 0.5 + 0.5) < sparse;
            let mut e = [0.0f32; 3];
            for (c, v) in e.iter_mut().enumerate() {
                let r = nz[c].next();
                *v = if keep { r / sparse.sqrt() } else { 0.0 };
            }
            // stereo: below 300 Hz mix towards the common (mono) noise by (1 - spread low),
            // above by (1 - spread high)
            let mut out = [0.0f32; 2];
            split[2] += split_a * (e[2] - split[2]);
            for c in 0..2 {
                split[c] += split_a * (e[c] - split[c]);
                let lo = split[c] * s_lo + split[2] * (1.0 - s_lo);
                let hi = (e[c] - split[c]) * s_hi + (e[2] - split[2]) * (1.0 - s_hi);
                out[c] = (lo + hi) * vol;
            }
            // natural air absorption: the tail darkens with time
            let mut cut = (7000.0 * (1.0 - 0.8 * t)).max(1200.0);
            if filter {
                let fa_n = fa * 0.5;
                let lvl = if t < fa_n { fi + (fm - fi) * t / fa_n.max(1e-4) } else { fm + (fe - fm) * ((t - fa_n) / (fd.max(0.02) * (1.0 - fa_n)).max(1e-4)).min(1.0) };
                cut = 40.0 * (20000.0f32 / 40.0).powf(lvl.clamp(0.0, 1.0));
            }
            let g = 1.0 - (-crate::util::TAU * cut.min(rate * 0.45) / rate).exp();
            for c in 0..2 {
                let s = &mut lp_state[c];
                let x = out[c];
                let y = if filter {
                    // resonance: feed back the band-pass difference
                    let fb = res * 3.5 * (s[1] - s[2]);
                    s[0] += g * (x - fb - s[0]);
                    s[1] += g * (s[0] - s[1]);
                    s[2] += g * (s[1] - s[2]);
                    s[3] += g * (s[2] - s[3]);
                    match mode {
                        0 => x - s[1],
                        1 => s[0] - s[1],
                        2 => s[0],
                        3 => s[1],
                        _ => s[3],
                    }
                } else {
                    s[0] += g * (x - s[0]);
                    s[0]
                };
                self.ir[c][i] = y;
            }
        }
        for c in 0..2 {
            for v in self.ir[c][n..].iter_mut() {
                *v = 0.0;
            }
            if p.on(REVERSE) {
                self.ir[c][..n].reverse();
            }
        }
        // normalise the energy (left and right together)
        let e: f64 = self.ir.iter().map(|ch| ch[..n].iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum::<f64>() / 2.0;
        let g = (db(IR_ENERGY_DB) as f64 / e.max(1e-20).sqrt()) as f32;
        for c in 0..2 {
            self.ir[c][..n].iter_mut().for_each(|v| *v *= g);
        }
    }

    fn update(&mut self) {
        self.synthesise();
        for c in 0..2 {
            let (conv, ir) = (&mut self.conv[c], &self.ir[c]);
            conv.load(&ir[..self.ir_len]);
        }
        self.pre_len = (self.p.get(PREDELAY) * 0.001 * self.rate).max(1.0);
        self.dry = if self.p.get(DRY) <= -95.9 { 0.0 } else { db(self.p.get(DRY)) };
        self.wet = if self.p.get(WET) <= -95.9 { 0.0 } else { db(self.p.get(WET)) };
    }

    fn clear(&mut self) {
        for c in self.conv.iter_mut() {
            c.clear();
        }
        for d in self.pre.iter_mut() {
            d.clear();
        }
    }

    fn tail_secs(&self) -> f32 {
        self.ir_len as f32 / self.rate + self.p.get(PREDELAY) * 0.001
    }
}

impl lg_dsp::Effect for SpaceConvolver {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            self.pre[0].push(left[i]);
            self.pre[1].push(right[i]);
            let a = self.pre[0].read(self.pre_len);
            let b = self.pre[1].read(self.pre_len);
            let wl = self.conv[0].run(a);
            let wr = self.conv[1].run(b);
            left[i] = left[i] * self.dry + wl * self.wet;
            right[i] = right[i] * self.dry + wr * self.wet;
        }
    }
}

plugin_impl!(SpaceConvolver);
