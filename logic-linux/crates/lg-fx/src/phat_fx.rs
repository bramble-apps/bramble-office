//! Phat FX: the stand-in for Logic's Phat FX multi-effect — a fixed chain of Bandpass,
//! Distortion (three stages), Filter, Mod FX, Bass Enhancer and Compressor, then the master
//! section (Mix, Output, Limiter), with LFO 1 / LFO 2 / Envelope modulation.
//!
//! Most stored values are normalised 0..1 as Logic keeps them (the window shows them scaled):
//! cutoffs map 20 Hz-20 kHz on a log scale, drives and amounts are shares of their range.
//! Positions are from the research map (value-anchored rows); the modulation sources' rates and
//! depths sit in rows the map could not pin, so the LFOs run at a fixed musical rate (one cycle
//! per 2 beats) at full depth when switched on.

use crate::util::{Biquad, Coefs, Delay, Follower, OnePole, Phase, Values, beats_hz, db, flush, plugin_impl, sine01, tri};
use lg_dsp::{Param, Unit};

const BP_TYPES: &[&str] = &["Classic", "Edgy", "Rich", "Gritty"];
const DIST_TYPES: &[&str] = &["Bit Crusher", "Downsampler", "Vari Drive", "Grit", "Scream", "Exciter", "Hard Saturation", "Tube", "Soft Saturation"];
const FILTER_TYPES: &[&str] = &[
    "LP 12dB Smooth", "LP 12dB Edgy", "LP 12dB Rich", "LP 12dB Clean", "LP 24dB Rich", "BP 6dB Smooth", "BP 6dB Edgy", "BP 6dB Rich", "BP 6dB Clean", "BP 12dB Rich", "HP 12dB Smooth", "HP 12dB Edgy", "HP 12dB Rich", "HP 12dB Clean", "HP 24dB Rich", "Comb Neg", "Comb PM", "Comb Pos", "Peaking",
    // (ours after Logic's decoded list: the rest of the guide's types)
    "LP 12dB Sharp", "LP 12dB Gritty", "LP 24dB Edgy", "BP 6dB Sharp", "BP 6dB Gritty", "HP 12dB Sharp", "HP 12dB Gritty", "HP 24dB Edgy", "Notch", "RingMod", "Bitcrusher", "Dirt", "Downsampler", "Grit",
];
pub const PROCESSORS: [&str; 6] = ["Bandpass", "Distortion", "Filter", "Mod FX", "Bass Enhancer", "Compressor"];
pub const SYNC_RATES: &[&str] = &["4/1", "2/1", "1/1", "1/2", "1/4", "1/8", "1/16", "1/32"];
const SYNC_BEATS: [f32; 8] = [16.0, 8.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.125];
/// Our own ids (Logic's rows for these aren't decoded).
pub const F_AMOUNT: u32 = 200;
pub const F_GAIN: u32 = 201;
pub const F_BW: u32 = 202;
pub const F_FBACK: u32 = 203;
pub const F_DAMP: u32 = 204;
pub const F_FREQ: u32 = 205;
pub const F_MIX2: u32 = 206;
pub const ENV_ATTACK: u32 = 210;
pub const ENV_RELEASE: u32 = 211;
pub const ENV_DEPTH: u32 = 212;
/// LFO n (0, 1): rate, sync, sync rate, depth at LFO_BASE + 4n.
pub const LFO_BASE: u32 = 213;
pub const INPUT: u32 = 221;
/// The effects order: the processor in each of six places.
pub const ORDER: u32 = 223;
/// XY pad: X targets A / B, Y targets A / B, X on, Y on, X depth, Y depth, X, Y.
pub const XY: u32 = 230;
const MOD_MODES: &[&str] = &["Soft", "Heavy", "Doubler", "Ring Mod"];
const COMP_MODES: &[&str] = &["Vintage VCA", "Vintage FET", "Vintage Opto", "Platinum", "Studio VCA", "Studio FET", "Classic VCA"];
const LIMITS: &[&str] = &["Soft Limiting", "Hard Clipping", "Limiter"];
const TARGETS: &[&str] = &["Filter Cutoff", "Filter Resonance", "Bandpass Low Cutoff", "Bandpass High Cutoff", "Distortion Drive", "Mod FX Rate", "Master Output"];
const SHAPES: &[&str] = &["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "Random Hold", "Random Glide"];

static PARAMS: [Param; 40] = [
    Param::new(1, "Bandpass On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Bandpass Low Cutoff", 0.0, 1.0, 0.0, Unit::None),
    Param::new(3, "Bandpass High Cutoff", 0.0, 1.0, 1.0, Unit::None),
    Param::new(4, "Bandpass Low Resonance", 0.0, 1.0, 0.35, Unit::None),
    Param::new(5, "Bandpass High Resonance", 0.0, 1.0, 0.35, Unit::None),
    Param::new(11, "Bandpass Reject Mix", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(12, "Bandpass Type", BP_TYPES, 0.0),
    Param::new(13, "Distortion On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(14, "Distortion Type 1", DIST_TYPES, 0.0),
    Param::new(15, "Distortion Drive 1", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(16, "Distortion Type 2", DIST_TYPES, 8.0),
    Param::new(17, "Distortion Drive 2", 0.0, 1.0, 0.15, Unit::None),
    Param::menu(18, "Distortion Type 3", DIST_TYPES, 2.0),
    Param::new(19, "Distortion Drive 3", 0.0, 1.0, 0.05, Unit::None),
    Param::new(26, "Filter On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(27, "Filter Type", FILTER_TYPES, 1.0),
    Param::new(28, "Filter Cutoff", 0.0, 1.0, 0.5, Unit::None),
    Param::new(29, "Filter Resonance", 0.0, 1.0, 0.0, Unit::None),
    Param::new(30, "Filter Drive", 0.0, 1.0, 0.0, Unit::None),
    Param::new(31, "Filter Mix", 0.0, 1.0, 1.0, Unit::None),
    Param::new(37, "Mod FX On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(38, "Mod FX Mix", 0.0, 1.0, 0.5, Unit::None),
    Param::new(39, "Mod FX Rate", 0.0, 1.0, 0.099, Unit::None),
    Param::menu(45, "Mod FX Mode", MOD_MODES, 0.0),
    Param::new(46, "Bass Enhancer On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(47, "Bass Enhancer Amount", 0.0, 1.0, 0.5, Unit::None),
    Param::new(48, "Bass Enhancer Tune", 0.0, 1.0, 0.5, Unit::None),
    Param::menu(54, "Bass Enhancer Mode", &["Warm", "Clip"], 0.0),
    Param::new(55, "Compressor On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(56, "Compressor Amount", 0.0, 1.0, 0.4999, Unit::None),
    Param::new(57, "Compressor Release", 0.0, 1.0, 0.492, Unit::None),
    Param::menu(64, "Compressor Mode", COMP_MODES, 0.0),
    Param::new(65, "Master Mix", 0.0, 1.0, 1.0, Unit::None),
    Param::new(66, "Master Output", 0.0, 1.0, 0.731, Unit::None),
    Param::menu(91, "Master Limiter", LIMITS, 0.0),
    Param::new(98, "Envelope On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(99, "Envelope Target", TARGETS, 0.0),
    Param::new(100, "LFO 1 On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(101, "LFO 1 Target", TARGETS, 0.0),
    Param::menu(102, "LFO 1 Shape", SHAPES, 0.0),
];

static PARAMS2: [Param; 3] = [
    Param::new(103, "LFO 2 On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(104, "LFO 2 Target", TARGETS, 1.0),
    Param::menu(105, "LFO 2 Shape", SHAPES, 0.0),
];

static ALL: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v: Vec<Param> = PARAMS.iter().chain(PARAMS2.iter()).cloned().collect();
    v.extend([
        Param::new(F_AMOUNT, "Filter Amount", 0.0, 1.0, 0.5, Unit::None),
        Param::new(F_GAIN, "Filter Gain", -24.0, 24.0, 6.0, Unit::Db),
        Param::new(F_BW, "Filter BW", 0.1, 4.0, 1.0, Unit::Octaves),
        Param::new(F_FBACK, "Filter F-Back", 0.0, 1.0, 0.5, Unit::None),
        Param::new(F_DAMP, "Filter Damp", 0.0, 1.0, 0.3, Unit::None),
        Param::new(F_FREQ, "Filter Frequency", 1.0, 8000.0, 440.0, Unit::Hz),
        Param::new(F_MIX2, "Filter Type Mix", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(ENV_ATTACK, "Envelope Attack", 0.1, 500.0, 3.0, Unit::Ms),
        Param::new(ENV_RELEASE, "Envelope Release", 5.0, 5000.0, 200.0, Unit::Ms),
        Param::new(ENV_DEPTH, "Envelope Depth", 0.0, 1000.0, 100.0, Unit::Percent),
        Param::new(INPUT, "Input", -24.0, 24.0, 0.0, Unit::Db),
    ]);
    for n in 0..2u32 {
        let b = LFO_BASE + 4 * n;
        let l = n + 1;
        v.push(Param::new(b, Box::leak(format!("LFO {l} Rate").into_boxed_str()), 0.01, 50.0, 1.0, Unit::Hz));
        v.push(Param::new(b + 1, Box::leak(format!("LFO {l} Sync").into_boxed_str()), 0.0, 1.0, 1.0, Unit::OnOff));
        v.push(Param::menu(b + 2, Box::leak(format!("LFO {l} Sync Rate").into_boxed_str()), SYNC_RATES, 3.0));
        v.push(Param::new(b + 3, Box::leak(format!("LFO {l} Depth").into_boxed_str()), 0.0, 100.0, 100.0, Unit::Percent));
    }
    for k in 0..6u32 {
        v.push(Param::menu(ORDER + k, Box::leak(format!("Order {}", k + 1).into_boxed_str()), &PROCESSORS, k as f32).hide());
    }
    for (k, n) in ["X Target A", "X Target B", "Y Target A", "Y Target B"].iter().enumerate() {
        v.push(Param::menu(XY + k as u32, n, XY_TARGETS, if k % 2 == 0 { (k / 2 + 1) as f32 } else { 0.0 }));
    }
    v.extend([
        Param::new(XY + 4, "X On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(XY + 5, "Y On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(XY + 6, "X Depth", -100.0, 100.0, 50.0, Unit::Percent),
        Param::new(XY + 7, "Y Depth", -100.0, 100.0, 50.0, Unit::Percent),
        Param::new(XY + 8, "X", 0.0, 1.0, 0.5, Unit::None),
        Param::new(XY + 9, "Y", 0.0, 1.0, 0.5, Unit::None),
    ]);
    v
});

/// What the XY pad can move ("Off", then the modulation targets).
pub const XY_TARGETS: &[&str] = &["Off", "Filter Cutoff", "Filter Resonance", "Bandpass Low Cutoff", "Bandpass High Cutoff", "Distortion Drive", "Mod FX Rate", "Master Output"];

/// 0..1 to Hz on a log scale (20 Hz-20 kHz).
#[inline]
fn hz(v: f32) -> f32 {
    20.0 * 1000f32.powf(v.clamp(0.0, 1.0))
}

/// A two-pole state-variable filter (resonant), giving low, band and high outputs.
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    #[inline]
    fn run(&mut self, x: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + g * a1 * v3;
        let v2 = self.ic2 + g * v1;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, v1, x - k * v1 - v2)
    }
}

#[inline]
fn svf_g(rate: f32, f: f32) -> f32 {
    (std::f32::consts::PI * f.clamp(10.0, rate * 0.45) / rate).tan()
}

pub struct PhatFx {
    /// The side chain's block: the envelope follower tracks it when one is chosen.
    side: Option<(Vec<f32>, Vec<f32>)>,
    p: Values,
    rate: f32,
    bpm: f64,
    bp: [[Svf; 2]; 2],
    filt: [[Svf; 2]; 2],
    comb: [Delay; 2],
    hold: [f32; 2],
    hold_n: f32,
    modd: [Delay; 2],
    mod_ph: Phase,
    bass_lp: [Biquad; 2],
    bass_lp2: [Biquad; 2],
    comp_env: Follower,
    comp_gr: f32,
    env: Follower,
    lfo1: Phase,
    lfo2: Phase,
    rnd: [crate::util::Noise; 2],
    rnd_v: [f32; 2],
    smooth: [OnePole; 2],
    comb_damp: [f32; 2],
    fhold: [f32; 2],
    fhold_n: f32,
    ring_ph: Phase,
}

impl PhatFx {
    pub fn new(rate: f32) -> PhatFx {
        let mut f = PhatFx {
            side: None,
            p: Values::new(&ALL),
            rate,
            bpm: 120.0,
            bp: [[Svf::default(); 2]; 2],
            filt: [[Svf::default(); 2]; 2],
            comb: [Delay::new((0.03 * rate) as usize), Delay::new((0.03 * rate) as usize)],
            hold: [0.0; 2],
            hold_n: 0.0,
            modd: [Delay::new((0.05 * rate) as usize), Delay::new((0.05 * rate) as usize)],
            mod_ph: Phase::default(),
            bass_lp: [Biquad::default(); 2],
            bass_lp2: [Biquad::default(); 2],
            comp_env: Follower::new(rate, 5.0, 150.0),
            comp_gr: 0.0,
            env: Follower::new(rate, 3.0, 200.0),
            lfo1: Phase::default(),
            lfo2: Phase::default(),
            rnd: [crate::util::Noise(3), crate::util::Noise(9)],
            rnd_v: [0.0; 2],
            smooth: [OnePole::new(rate, 30.0); 2],
            comb_damp: [0.0; 2],
            fhold: [0.0; 2],
            fhold_n: 0.0,
            ring_ph: Phase::default(),
        };
        f.update();
        f
    }

    fn update(&mut self) {
        let f = 30.0 + 120.0 * self.p.get(48);
        for s in 0..2 {
            self.bass_lp[s].c = Coefs::lowpass(self.rate, f, 0.707);
            self.bass_lp2[s].c = Coefs::lowpass(self.rate, f * 1.5, 0.707);
        }
        let rel = 30.0 + 1000.0 * self.p.get(57);
        self.comp_env.set(self.rate, 5.0, rel);
        self.env.set(self.rate, self.p.get(ENV_ATTACK), self.p.get(ENV_RELEASE));
    }

    fn clear(&mut self) {
        self.bp = [[Svf::default(); 2]; 2];
        self.filt = [[Svf::default(); 2]; 2];
        for d in self.comb.iter_mut().chain(self.modd.iter_mut()) {
            d.clear();
        }
        for b in self.bass_lp.iter_mut().chain(self.bass_lp2.iter_mut()) {
            b.clear();
        }
        self.comp_gr = 0.0;
    }

    fn tail_secs(&self) -> f32 {
        0.1
    }

    fn lfo(&mut self, which: usize, shape: usize, hz: f32) -> f32 {
        let ph = if which == 0 { &mut self.lfo1 } else { &mut self.lfo2 };
        let before = ph.p;
        let q = ph.step(self.rate, hz);
        if q < before {
            self.rnd_v[which] = self.rnd[which].next();
        }
        match shape {
            0 => sine01(q),
            1 => tri(q),
            2 => 2.0 * q - 1.0,
            3 => 1.0 - 2.0 * q,
            4 => if q < 0.5 { 1.0 } else { -1.0 },
            5 => self.rnd_v[which],
            _ => self.smooth[which].lp(self.rnd_v[which]),
        }
    }
}

/// One distortion stage of the given type at drive 0..1.
#[inline]
fn dist(kind: usize, x: f32, d: f32, hold: &mut f32, n: f32) -> f32 {
    if d <= 0.0 {
        return x;
    }
    let g = db(d * 36.0);
    match kind {
        0 => {
            let bits = 16.0 - 14.0 * d;
            let q = 2f32.powf(bits);
            (x * q).round() / q
        }
        1 => {
            // downsampler: n is the running counter's wrap flag
            if n >= 1.0 {
                *hold = x;
            }
            *hold
        }
        2 => ((x * g).tanh() + 0.2 * (x * g * 0.5).tanh().powi(2)) / (1.0 + d),
        3 => {
            let u = x * g;
            (u.clamp(-1.0, 1.0) * 0.7 + 0.3 * u.signum() * (u.abs().min(1.0)).sqrt()) / (1.0 + d * 0.5)
        }
        4 => (x * g * 2.0).clamp(-0.8, 0.8) / (1.0 + d * 0.3),
        5 => x + d * (x * g).tanh().abs() * 0.3,
        6 => (x * g).clamp(-1.0, 1.0) / g.sqrt(),
        7 => {
            let u = x * g;
            (if u > 0.0 { u.tanh() } else { (u * 0.6).tanh() / 0.6 }) / g.sqrt()
        }
        _ => (x * g).tanh() / g.powf(0.6),
    }
}

impl lg_dsp::Effect for PhatFx {
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = Some((left.to_vec(), right.to_vec()));
    }
    fn has_side_chain(&self) -> bool {
        true
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let side = self.side.take();
        let rate = self.rate;
        let v = |id: u32| self.p.get(id);
        let on = [self.p.on(1), self.p.on(13), self.p.on(26), self.p.on(37), self.p.on(46), self.p.on(55)];
        let (bp_lo, bp_hi, bp_rlo, bp_rhi, reject) = (v(2), v(3), v(4), v(5), v(11));
        let dists = [(self.p.idx(14), v(15)), (self.p.idx(16), v(17)), (self.p.idx(18), v(19))];
        let (ftype, fcut, fres, fdrive, fmix) = (self.p.idx(27), v(28), v(29), v(30), v(31));
        let (famt, fgain, fbw, ffb, fdamp, ffreq, fmix2) = (v(F_AMOUNT), v(F_GAIN), v(F_BW), v(F_FBACK), v(F_DAMP), v(F_FREQ), v(F_MIX2) / 100.0);
        let (mmix, mrate, mmode) = (v(38), v(39), self.p.idx(45));
        let (bamt, bmode) = (v(47), self.p.idx(54));
        let camt = v(56);
        let (mix, out_v, lim) = (v(65), v(66), self.p.idx(91));
        let input = db(v(INPUT));
        let out = if out_v >= 0.731 { db((out_v - 0.731) / 0.269 * 12.0) } else { db((out_v - 0.731) / 0.731 * 48.0) };
        let mods = [(self.p.on(100), self.p.idx(101), self.p.idx(102)), (self.p.on(103), self.p.idx(104), self.p.idx(105))];
        let lfo_set: Vec<(f32, f32)> = (0..2u32).map(|n| {
            let b = LFO_BASE + 4 * n;
            let hz = if self.p.on(b + 1) { beats_hz(SYNC_BEATS[self.p.idx(b + 2).min(7)], self.bpm) } else { self.p.get(b) };
            (hz, self.p.get(b + 3) / 100.0)
        }).collect();
        let (env_on, env_t, env_depth) = (self.p.on(98), self.p.idx(99), v(ENV_DEPTH) / 100.0);
        // the XY pad: each axis moves its one or two targets by its depth
        let mut xy = [0.0f32; 7];
        for (axis, on_id) in [(0u32, XY + 4), (1, XY + 5)] {
            if self.p.on(on_id) {
                let pos = self.p.get(XY + 8 + axis) * 2.0 - 1.0;
                let depth = self.p.get(XY + 6 + axis) / 100.0;
                for t in [self.p.idx(XY + 2 * axis), self.p.idx(XY + 2 * axis + 1)] {
                    if t > 0 {
                        xy[(t - 1).min(6)] += 0.5 * pos * depth;
                    }
                }
            }
        }
        let mut order: Vec<usize> = (0..6).map(|k| self.p.idx(ORDER + k as u32).min(5)).collect();
        // (a broken order falls back to Logic's)
        let mut seen = [false; 6];
        if order.iter().any(|o| std::mem::replace(&mut seen[*o], true)) {
            order = (0..6).collect();
        }
        let comb_ms = 0.5 + 15.0 * (1.0 - fcut);
        let ds_step = 1.0 + 40.0 * dists.iter().filter(|d| d.0 == 1).map(|d| d.1).fold(0.0, f32::max);
        let fds_step = (rate / ffreq.max(50.0)).max(1.0);
        for i in 0..left.len().min(right.len()) {
            // modulation, in units of each target's 0..1 range
            let mut m = xy;
            for (w, &(lon, t, sh)) in mods.iter().enumerate() {
                if lon {
                    let l = self.lfo(w, sh, lfo_set[w].0);
                    m[t.min(6)] += 0.35 * l * lfo_set[w].1;
                }
            }
            if env_on {
                let x = match &side {
                    Some((a, b)) => a.get(i).copied().unwrap_or(0.0).abs().max(b.get(i).copied().unwrap_or(0.0).abs()),
                    None => left[i].abs().max(right[i].abs()),
                };
                let e = self.env.run(x);
                // (depths over 100 % lift quiet signals)
                m[env_t.min(6)] += (e * 3.0 * env_depth).min(1.0) * 0.5;
            }
            self.hold_n += 1.0;
            let wrap = if self.hold_n >= ds_step {
                self.hold_n -= ds_step;
                1.0
            } else {
                0.0
            };
            self.fhold_n += 1.0;
            let fwrap = self.fhold_n >= fds_step;
            if fwrap {
                self.fhold_n -= fds_step;
            }
            let cl = self.comp_env.run(left[i].abs().max(right[i].abs()) * input);
            let gr = if on[5] {
                let lvl = lg_dsp::gain_to_db(cl.max(1e-6));
                let thr = -10.0 - 30.0 * camt;
                let over = (lvl - thr).max(0.0) * (0.5 + 0.4 * camt);
                self.comp_gr = over;
                db(-over + 10.0 * camt * 0.6)
            } else {
                1.0
            };
            if on[3] {
                let hzm = 0.05 * 200f32.powf((mrate + m[5]).clamp(0.0, 1.0));
                self.mod_ph.step(rate, hzm);
            }
            self.ring_ph.step(rate, ffreq);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                let mut y = dry * input;
                for &proc_ in &order {
                    if !on[proc_] {
                        continue;
                    }
                    match proc_ {
                        0 => {
                            let k_lo = 2.0 - 1.9 * bp_rlo;
                            let k_hi = 2.0 - 1.9 * bp_rhi;
                            let (_, _, hp) = self.bp[s][0].run(y, svf_g(rate, hz(bp_lo + m[2])), k_lo);
                            let (lp, _, _) = self.bp[s][1].run(hp, svf_g(rate, hz(bp_hi + m[3])), k_hi);
                            y = lp + reject * (y - lp);
                        }
                        1 => {
                            for &(t, d) in dists.iter() {
                                y = dist(t, y, (d + m[4]).clamp(0.0, 1.0), &mut self.hold[s], wrap);
                            }
                        }
                        2 => {
                            let clean = matches!(ftype, 3 | 8 | 13);
                            let fx_in = if fdrive > 0.0 && !clean { (y * db(fdrive * 24.0)).tanh() / db(fdrive * 12.0) } else { y };
                            let cut = hz(fcut + m[0]);
                            let g = svf_g(rate, cut);
                            let res = (fres + m[1]).clamp(0.0, 1.0);
                            let k = 2.0 - 1.95 * res;
                            let f = match ftype {
                                0..=14 | 19..=26 => {
                                    // LP / BP / HP: Smooth (SVF), Edgy (driven SVF, 24 dB two stages),
                                    // Rich, Clean, Sharp (analog-modelled, a peakier two-pole),
                                    // Gritty (saturating with Res and Drive)
                                    let band = match ftype {
                                        0..=4 | 19..=21 => 0,
                                        5..=9 | 22 | 23 => 1,
                                        _ => 2,
                                    };
                                    let edgy = matches!(ftype, 1 | 6 | 11 | 21 | 26);
                                    let sharp = matches!(ftype, 19 | 22 | 24);
                                    let gritty = matches!(ftype, 20 | 23 | 25);
                                    let input_ = if gritty { (fx_in * (1.0 + 4.0 * fdrive + 3.0 * res)).tanh() } else if edgy || sharp { fx_in.tanh() } else { fx_in };
                                    let kk = if sharp { (k * 0.6).max(0.05) } else { k };
                                    let (lp, bp, hp) = self.filt[s][0].run(input_, g, kk);
                                    let first = [lp, bp, hp][band];
                                    let first = if gritty { (first * (1.0 + 2.0 * res)).tanh() } else { first };
                                    if matches!(ftype, 4 | 9 | 14 | 21 | 26) {
                                        let (lp2, bp2, hp2) = self.filt[s][1].run(if edgy { first.tanh() } else { first }, g, k.max(1.0));
                                        [lp2, bp2, hp2][band]
                                    } else {
                                        first
                                    }
                                }
                                15..=17 => {
                                    // combs: F-Back (feedback), Damp (softens the peaks)
                                    let d = self.comb[s].read(comb_ms * 0.001 * rate);
                                    self.comb_damp[s] += (1.0 - 0.9 * fdamp) * (d - self.comb_damp[s]);
                                    let fb = if ftype == 15 { -0.95 } else { 0.95 } * ffb;
                                    let w = fx_in + self.comb_damp[s] * fb;
                                    self.comb[s].push(w);
                                    w
                                }
                                18 => {
                                    // peaking: Gain at Cutoff, BW wide
                                    let kb = (2.0 * (fbw * std::f32::consts::LN_2 / 2.0).sinh()).max(0.05);
                                    let (_, bp, _) = self.filt[s][0].run(fx_in, g, kb);
                                    fx_in + bp * kb * (db(fgain) - 1.0)
                                }
                                27 => {
                                    // notch: Amount is how deep the cut goes
                                    let (lp, _, hp) = self.filt[s][0].run(fx_in, g, 0.7);
                                    fx_in * (1.0 - famt) + (lp + hp) * famt
                                }
                                28 => {
                                    // ring modulation, Mix towards amplitude modulation
                                    let c = sine01(self.ring_ph.p + if s == 1 { 0.0 } else { 0.0 });
                                    fx_in * ((1.0 - fmix2) * c + fmix2 * (0.5 + 0.5 * c))
                                }
                                _ => {
                                    // Bitcrusher, Dirt (tube), Downsampler (Freq), Grit (transistor)
                                    let a = famt;
                                    let wet = match ftype {
                                        29 => {
                                            let q = 2f32.powf(16.0 - 14.0 * a);
                                            (fx_in * q).round() / q
                                        }
                                        30 => {
                                            let u = fx_in * db(a * 30.0);
                                            (if u > 0.0 { u.tanh() } else { (u * 0.7).tanh() / 0.7 }) / db(a * 15.0)
                                        }
                                        31 => {
                                            if fwrap {
                                                self.fhold[s] = fx_in;
                                            }
                                            self.fhold[s]
                                        }
                                        _ => (fx_in * db(a * 36.0)).clamp(-1.0, 1.0) / db(a * 18.0),
                                    };
                                    fx_in * (1.0 - fmix2) + wet * fmix2
                                }
                            };
                            y = y * (1.0 - fmix) + f * fmix;
                        }
                        3 => {
                            let ph = self.mod_ph.p + if s == 1 { 0.25 } else { 0.0 };
                            self.modd[s].push(y);
                            let w = match mmode {
                                0 => self.modd[s].read_lin((0.007 + 0.003 * sine01(ph)) * rate),
                                1 => {
                                    let d = self.modd[s].read_lin((0.002 + 0.0018 * sine01(ph)) * rate);
                                    d + 0.5 * self.modd[s].read_lin((0.004 + 0.0036 * sine01(ph)) * rate)
                                }
                                2 => self.modd[s].read_lin((0.018 + if s == 1 { 0.009 } else { 0.0 }) * rate),
                                _ => y * sine01(ph * 40.0),
                            };
                            y = y * (1.0 - mmix * 0.5) + w * mmix;
                        }
                        4 => {
                            let lo = self.bass_lp[s].run(y);
                            let gen_ = if bmode == 0 { (lo * 4.0).tanh() * 0.5 } else { (lo * 8.0).clamp(-0.5, 0.5) };
                            y += self.bass_lp2[s].run(gen_) * bamt * 1.5;
                        }
                        _ => y *= gr,
                    }
                }
                let mut o = (dry * (1.0 - mix) + y * mix) * out * db(m[6] * 12.0);
                o = match lim {
                    0 => (o * 0.9).tanh() / 0.9,
                    1 => o.clamp(-1.0, 1.0),
                    _ => o.clamp(-0.98, 0.98),
                };
                *x = o;
            }
        }
    }

    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Compressor Reduction", self.comp_gr), ("LFO 1", self.lfo1.p), ("LFO 2", self.lfo2.p)]
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
}

plugin_impl!(PhatFx);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    fn level(fx: &mut PhatFx, f: f32) -> f32 {
        let mut l: Vec<f32> = (0..24000).map(|i| 0.3 * (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect();
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        (l[12000..].iter().map(|x| x * x).sum::<f32>() / 12000.0).sqrt()
    }

    fn bare() -> PhatFx {
        let mut fx = PhatFx::new(48000.0);
        for id in [1, 13, 46, 55] {
            fx.set(id, 0.0);
        }
        fx.set(91, 2.0);
        fx.set(26, 1.0);
        fx
    }

    /// Notch: a tone at the cutoff is cut, by Amount; peaking: Gain lifts it.
    #[test]
    fn notch_and_peak() {
        let mut fx = bare();
        fx.set(27, 27.0);
        fx.set(28, (1000f32 / 20.0).log10() / 3.0);
        fx.set(F_AMOUNT, 1.0);
        let at = level(&mut fx, 1000.0);
        let mut fx2 = bare();
        fx2.set(27, 27.0);
        fx2.set(28, (1000f32 / 20.0).log10() / 3.0);
        fx2.set(F_AMOUNT, 1.0);
        let off = level(&mut fx2, 100.0);
        assert!(at < off * 0.3, "{at} {off}");
        let mut fx3 = bare();
        fx3.set(27, 18.0);
        fx3.set(28, (1000f32 / 20.0).log10() / 3.0);
        fx3.set(F_GAIN, 12.0);
        assert!(level(&mut fx3, 1000.0) > off * 2.0);
    }

    /// RingMod at 0 % mix removes the input's own frequency's steady level (sidebands only);
    /// the effects order and the XY pad are parameters.
    #[test]
    fn ringmod_order_xy() {
        let mut fx = bare();
        fx.set(27, 28.0);
        fx.set(F_FREQ, 300.0);
        fx.set(F_MIX2, 0.0);
        let rm = level(&mut fx, 1000.0);
        assert!(rm > 0.05 && rm < 0.2);
        // swapping two places keeps a valid order; a duplicate falls back
        let mut fx2 = bare();
        fx2.set(ORDER, 2.0);
        fx2.set(ORDER + 2, 0.0);
        assert!(level(&mut fx2, 440.0) > 0.0);
        // the XY pad moves the cutoff of an LP filter: far left closes it
        let mut a = bare();
        a.set(27, 0.0);
        a.set(28, 0.6);
        let open = level(&mut a, 2000.0);
        let mut b = bare();
        b.set(27, 0.0);
        b.set(28, 0.6);
        b.set(XY + 4, 1.0);
        b.set(XY + 6, 100.0);
        b.set(XY + 8, 0.0);
        assert!(level(&mut b, 2000.0) < open * 0.5);
    }
}
