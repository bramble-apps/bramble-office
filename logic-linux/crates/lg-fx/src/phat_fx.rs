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
];
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

static ALL: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| PARAMS.iter().chain(PARAMS2.iter()).cloned().collect());

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
}

impl PhatFx {
    pub fn new(rate: f32) -> PhatFx {
        let mut f = PhatFx {
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
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let v = |id: u32| self.p.get(id);
        let (bp_on, dist_on, filt_on, mod_on, bass_on, comp_on) = (self.p.on(1), self.p.on(13), self.p.on(26), self.p.on(37), self.p.on(46), self.p.on(55));
        let (bp_lo, bp_hi, bp_rlo, bp_rhi, reject) = (v(2), v(3), v(4), v(5), v(11));
        let dists = [(self.p.idx(14), v(15)), (self.p.idx(16), v(17)), (self.p.idx(18), v(19))];
        let (ftype, fcut, fres, fdrive, fmix) = (self.p.idx(27), v(28), v(29), v(30), v(31));
        let (mmix, mrate, mmode) = (v(38), v(39), self.p.idx(45));
        let (bamt, bmode) = (v(47), self.p.idx(54));
        let camt = v(56);
        let (mix, out_v, lim) = (v(65), v(66), self.p.idx(91));
        let out = if out_v >= 0.731 { db((out_v - 0.731) / 0.269 * 12.0) } else { db((out_v - 0.731) / 0.731 * 48.0) };
        let mods = [(self.p.on(100), self.p.idx(101), self.p.idx(102)), (self.p.on(103), self.p.idx(104), self.p.idx(105))];
        let (env_on, env_t) = (self.p.on(98), self.p.idx(99));
        let lfo_hz = beats_hz(2.0, self.bpm);
        let comb_ms = 0.5 + 15.0 * (1.0 - fcut);
        let ds_step = 1.0 + 40.0 * dists.iter().filter(|d| d.0 == 1).map(|d| d.1).fold(0.0, f32::max);
        for i in 0..left.len().min(right.len()) {
            // modulation, in units of each target's 0..1 range
            let mut m = [0.0f32; 7];
            for (w, &(on, t, sh)) in mods.iter().enumerate() {
                if on {
                    let l = self.lfo(w, sh, lfo_hz);
                    m[t.min(6)] += 0.35 * l;
                }
            }
            if env_on {
                let e = self.env.run(left[i].abs().max(right[i].abs()));
                m[env_t.min(6)] += (e * 3.0).min(1.0) * 0.5;
            }
            self.hold_n += 1.0;
            let wrap = if self.hold_n >= ds_step {
                self.hold_n -= ds_step;
                1.0
            } else {
                0.0
            };
            let gr = if comp_on {
                let lvl = lg_dsp::gain_to_db(self.comp_env.run(left[i].abs().max(right[i].abs())).max(1e-6));
                let thr = -10.0 - 30.0 * camt;
                let over = (lvl - thr).max(0.0) * (0.5 + 0.4 * camt);
                self.comp_gr = over;
                db(-over + 10.0 * camt * 0.6)
            } else {
                1.0
            };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                let mut y = dry;
                if bp_on {
                    let k_lo = 2.0 - 1.9 * bp_rlo;
                    let k_hi = 2.0 - 1.9 * bp_rhi;
                    let (_, _, hp) = self.bp[s][0].run(y, svf_g(rate, hz(bp_lo + m[2])), k_lo);
                    let (lp, _, _) = self.bp[s][1].run(hp, svf_g(rate, hz(bp_hi + m[3])), k_hi);
                    y = lp + reject * (y - lp);
                }
                if dist_on {
                    for &(t, d) in dists.iter() {
                        y = dist(t, y, (d + m[4]).clamp(0.0, 1.0), &mut self.hold[s], wrap);
                    }
                }
                if filt_on {
                    let fx_in = if fdrive > 0.0 { (y * db(fdrive * 24.0)).tanh() / db(fdrive * 12.0) } else { y };
                    let g = svf_g(rate, hz(fcut + m[0]));
                    let k = 2.0 - 1.95 * (fres + m[1]).clamp(0.0, 1.0);
                    let edgy = ftype % 5 == 1 || ftype == 6 || ftype == 11;
                    let f = if ftype <= 4 || (5..=14).contains(&ftype) {
                        let (lp, bp, hp) = self.filt[s][0].run(if edgy { fx_in.tanh() } else { fx_in }, g, k);
                        let first = match ftype {
                            0..=4 => lp,
                            5..=9 => bp,
                            _ => hp,
                        };
                        if ftype == 4 || ftype == 9 || ftype == 14 {
                            let (lp2, bp2, hp2) = self.filt[s][1].run(first, g, k.max(1.0));
                            match ftype {
                                4 => lp2,
                                9 => bp2,
                                _ => hp2,
                            }
                        } else {
                            first
                        }
                    } else if ftype <= 17 {
                        let d = self.comb[s].read(comb_ms * 0.001 * rate);
                        let fb = if ftype == 15 { -0.6 } else { 0.6 } * (0.3 + 0.65 * fres);
                        let w = fx_in + d * fb;
                        self.comb[s].push(w);
                        w
                    } else {
                        let (_, bp, _) = self.filt[s][0].run(fx_in, g, 0.7);
                        fx_in + bp * (fres * 4.0)
                    };
                    y = y * (1.0 - fmix) + f * fmix;
                }
                if mod_on {
                    let hzm = 0.05 * 200f32.powf((mrate + m[5]).clamp(0.0, 1.0));
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
                    if s == 1 {
                        self.mod_ph.step(rate, hzm);
                    }
                    y = y * (1.0 - mmix * 0.5) + w * mmix;
                }
                if bass_on {
                    let lo = self.bass_lp[s].run(y);
                    let gen_ = if bmode == 0 { (lo * 4.0).tanh() * 0.5 } else { (lo * 8.0).clamp(-0.5, 0.5) };
                    y += self.bass_lp2[s].run(gen_) * bamt * 1.5;
                }
                y *= gr;
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

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
}

plugin_impl!(PhatFx);
