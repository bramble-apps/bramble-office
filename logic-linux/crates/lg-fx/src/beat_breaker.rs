//! Beat Breaker (Logic 12's Multi Effects ▸ Beat Breaker), as the Effects guide describes it:
//! the incoming audio is cut into slices across a pattern (1–16 beats, up to 32 slice markers)
//! that follows the song; each slice has six edit modes:
//! - Time: Input Beat (which part of the input buffer plays), Speed (negative = reverse) and
//!   Curve (speed changing through the slice);
//! - Repeat: 1–8 repetitions of the slice (stutter);
//! - Cutoff: LP or HP filter, Cutoff, Slope (a ramp through the slice) and Curve (its shape);
//! - Resonance: the filter's resonance;
//! - Volume and Pan: a value with its Slope and Curve.
//!
//! Global: Length, De-click (short fades where the read point jumps), Bypass Below (lows under
//! it pass unprocessed), Mix; ten Pattern slots and Pattern Off; the Randomize button (Current
//! Mode or Multiple Modes with Enabled Modes, each mode with Probability and Intensity per
//! parameter, Repeat's allowed values 1–8, Output Beat randomization), Clear All Values, the More
//! menu's snaps (Output Beat, Input Beat, Speed Time / Pitch / Off, and Off / Fine / Medium /
//! Coarse for Cutoff, Resonance, Volume, Pan).
//!
//! The input buffer keeps the last pattern length; an Input Beat later than the playing point
//! reads the previous pass (as the guide's input buffer shows). Ids are our own (Logic's block
//! isn't decoded): the globals from 1000, the selected slice's controls at 1100 (they edit the
//! selected slice of the pattern being edited), the ten patterns' stored slices from 2000.

use crate::kit::{Clock, Crossover, Rng, Svf, beat_samples, leak};
use crate::util::{Smooth, Values};
use lg_dsp::{Effect, Param, Plugin, Transport, Unit};

pub const MODES: &[&str] = &["Time", "Repeat", "Cutoff", "Resonance", "Volume", "Pan"];
pub const LENGTHS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "12", "16"];
pub const LENGTH_BEATS: [f32; 10] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 12.0, 16.0];
const PATTERNS: &[&str] = &["Off", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
const SNAP_BEATS: &[&str] = &["Off", "1/4", "1/8", "1/16", "1/32"];
const SNAP_SPEED: &[&str] = &["Off", "Time", "Pitch"];
const SNAP_FINE: &[&str] = &["Off", "Fine", "Medium", "Coarse"];
pub const ACTIONS: &[&str] = &["—", "Recall Default", "Copy Pattern", "Paste Pattern", "Previous Pattern", "Next Pattern"];

pub const EDIT_MODE: u32 = 1000;
pub const LENGTH: u32 = 1001;
pub const DECLICK: u32 = 1002;
pub const BYPASS_BELOW: u32 = 1003;
pub const MIX: u32 = 1004;
pub const PATTERN: u32 = 1005;
pub const RANDOMIZE: u32 = 1006;
pub const RANDOM_MODE: u32 = 1007;
/// Enabled Modes for Multiple Modes: Time, Repeat, Cutoff, Resonance, Volume, Pan, Output Beat.
pub const ENABLED: u32 = 1008;
pub const SNAP_OUT: u32 = 1015;
pub const SNAP_IN: u32 = 1016;
pub const SNAP_SPD: u32 = 1017;
pub const SNAP_CUT: u32 = 1018;
pub const SNAP_RES: u32 = 1019;
pub const SNAP_VOL: u32 = 1020;
pub const SNAP_PAN: u32 = 1021;
pub const SELECTED: u32 = 1022;
/// Clear All Values: set to a mode + 1 (7 = Output Beat) to reset that mode on every slice.
pub const CLEAR: u32 = 1023;
/// The pattern being edited (1..10), kept when Pattern Off is chosen.
pub const EDITING: u32 = 1024;
pub const ACTION: u32 = 1025;
/// Randomization: probability / intensity pairs, in the guide's order (see RAND_NAMES).
pub const RAND: u32 = 1030;
/// The selected slice's controls (FIELDS order) at 1100 + field.
pub const SEL: u32 = 1100;

/// A slice's stored values.
pub const FIELDS: &[(&str, f32, f32, f32)] = &[
    ("Output Beat", 0.0, 16.0, 0.0),
    ("Input Beat", 0.0, 16.0, 0.0),
    ("Speed", -400.0, 400.0, 100.0),
    ("Time Curve", -1.0, 1.0, 0.0),
    ("Repeats", 1.0, 8.0, 1.0),
    ("Filter Type", 0.0, 1.0, 0.0),
    ("Cutoff", 0.0, 1.0, 1.0),
    ("Cutoff Slope", -1.0, 1.0, 0.0),
    ("Cutoff Curve", -1.0, 1.0, 0.0),
    ("Resonance", 0.0, 1.0, 0.0),
    ("Volume", 0.0, 1.0, 1.0),
    ("Volume Slope", -1.0, 1.0, 0.0),
    ("Volume Curve", -1.0, 1.0, 0.0),
    ("Pan", -1.0, 1.0, 0.0),
    ("Pan Slope", -1.0, 1.0, 0.0),
    ("Pan Curve", -1.0, 1.0, 0.0),
];
pub const F_OUT: usize = 0;
pub const F_IN: usize = 1;
pub const F_SPEED: usize = 2;
pub const F_TCURVE: usize = 3;
pub const F_REP: usize = 4;
pub const F_FTYPE: usize = 5;
pub const F_CUT: usize = 6;
pub const F_CSLOPE: usize = 7;
pub const F_CCURVE: usize = 8;
pub const F_RES: usize = 9;
pub const F_VOL: usize = 10;
pub const F_VSLOPE: usize = 11;
pub const F_VCURVE: usize = 12;
pub const F_PAN: usize = 13;
pub const F_PSLOPE: usize = 14;
pub const F_PCURVE: usize = 15;
pub const NF: usize = 16;
pub const MAX_SLICES: usize = 32;
pub const PAT_BASE: u32 = 2000;
pub const PAT_STRIDE: u32 = 600;

/// Pattern p's (0..10) length menu index and slice count ids, and a slice field's id.
pub const fn pat_len(p: usize) -> u32 {
    PAT_BASE + PAT_STRIDE * p as u32
}
pub const fn pat_count(p: usize) -> u32 {
    PAT_BASE + PAT_STRIDE * p as u32 + 1
}
pub const fn slice_id(p: usize, s: usize, f: usize) -> u32 {
    PAT_BASE + PAT_STRIDE * p as u32 + 8 + (s * NF + f) as u32
}

/// (name, mode it belongs to: 0..5 the edit modes, 6 Output Beat, the fields it randomizes).
pub const RAND_NAMES: &[(&str, usize, usize)] = &[
    ("Input Beat Probability", 0, F_IN),
    ("Input Beat Intensity", 0, F_IN),
    ("Speed Probability", 0, F_SPEED),
    ("Speed Intensity", 0, F_SPEED),
    ("Time Curve Probability", 0, F_TCURVE),
    ("Time Curve Intensity", 0, F_TCURVE),
    ("Repeat Probability", 1, F_REP),
    ("Filter Mode Probability", 2, F_FTYPE),
    ("Cutoff Probability", 2, F_CUT),
    ("Cutoff Intensity", 2, F_CUT),
    ("Cutoff Slope Probability", 2, F_CSLOPE),
    ("Cutoff Slope Intensity", 2, F_CSLOPE),
    ("Cutoff Curve Probability", 2, F_CCURVE),
    ("Cutoff Curve Intensity", 2, F_CCURVE),
    ("Resonance Probability", 3, F_RES),
    ("Resonance Intensity", 3, F_RES),
    ("Volume Probability", 4, F_VOL),
    ("Volume Intensity", 4, F_VOL),
    ("Volume Slope Probability", 4, F_VSLOPE),
    ("Volume Slope Intensity", 4, F_VSLOPE),
    ("Volume Curve Probability", 4, F_VCURVE),
    ("Volume Curve Intensity", 4, F_VCURVE),
    ("Pan Probability", 5, F_PAN),
    ("Pan Intensity", 5, F_PAN),
    ("Pan Slope Probability", 5, F_PSLOPE),
    ("Pan Slope Intensity", 5, F_PSLOPE),
    ("Pan Curve Probability", 5, F_PCURVE),
    ("Pan Curve Intensity", 5, F_PCURVE),
    ("Output Beat Probability", 6, F_OUT),
    ("Output Beat Intensity", 6, F_OUT),
];
/// Repeat values 1–8 allowed in randomization: 1060..1067.
pub const RAND_REPEATS: u32 = 1060;

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::menu(EDIT_MODE, "Edit Mode", MODES, 0.0),
        Param::menu(LENGTH, "Length", LENGTHS, 3.0),
        Param::new(DECLICK, "De-click", 0.0, 20.0, 3.0, Unit::Ms),
        Param::new(BYPASS_BELOW, "Bypass Below", 20.0, 1000.0, 20.0, Unit::Hz),
        Param::new(MIX, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
        Param::menu(PATTERN, "Pattern", PATTERNS, 1.0),
        Param::new(RANDOMIZE, "Randomize", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(RANDOM_MODE, "Randomize Mode", &["Current Mode", "Multiple Modes"], 0.0),
    ];
    for (k, n) in ["Time", "Repeat", "Cutoff", "Resonance", "Volume", "Pan", "Output Beat"].iter().enumerate() {
        v.push(Param::new(ENABLED + k as u32, leak(format!("Randomize {n}")), 0.0, 1.0, 1.0, Unit::OnOff));
    }
    v.extend([
        Param::menu(SNAP_OUT, "Snap Output Beat", SNAP_BEATS, 3.0),
        Param::menu(SNAP_IN, "Snap Input Beat", SNAP_BEATS, 3.0),
        Param::menu(SNAP_SPD, "Snap Speed", SNAP_SPEED, 1.0),
        Param::menu(SNAP_CUT, "Snap Cutoff", SNAP_FINE, 0.0),
        Param::menu(SNAP_RES, "Snap Resonance", SNAP_FINE, 0.0),
        Param::menu(SNAP_VOL, "Snap Volume", SNAP_FINE, 0.0),
        Param::menu(SNAP_PAN, "Snap Pan", SNAP_FINE, 0.0),
        Param::new(SELECTED, "Selected Slice", 0.0, (MAX_SLICES - 1) as f32, 0.0, Unit::None),
        Param::new(CLEAR, "Clear All Values", 0.0, 7.0, 0.0, Unit::None),
        Param::new(EDITING, "Editing Pattern", 1.0, 10.0, 1.0, Unit::None).hide(),
        Param::menu(ACTION, "Pattern Action", ACTIONS, 0.0),
    ]);
    for (k, (n, ..)) in RAND_NAMES.iter().enumerate() {
        v.push(Param::new(RAND + k as u32, n, 0.0, 100.0, if n.ends_with("Probability") { 50.0 } else { 50.0 }, Unit::Percent));
    }
    for r in 0..8 {
        v.push(Param::new(RAND_REPEATS + r, leak(format!("Random Repeats {}", r + 1)), 0.0, 1.0, if r < 4 { 1.0 } else { 0.0 }, Unit::OnOff));
    }
    for (f, (n, lo, hi, d)) in FIELDS.iter().enumerate() {
        let u = match f {
            F_FTYPE => Param::menu(SEL + f as u32, "Slice Filter Type", &["LP", "HP"], 0.0),
            F_SPEED => Param::new(SEL + f as u32, "Slice Speed", *lo, *hi, *d, Unit::Percent),
            _ => Param::new(SEL + f as u32, leak(format!("Slice {n}")), *lo, *hi, *d, Unit::None),
        };
        v.push(u);
    }
    for p in 0..10 {
        v.push(Param::menu(pat_len(p), leak(format!("Pattern {} Length", p + 1)), LENGTHS, 3.0).hide());
        v.push(Param::new(pat_count(p), leak(format!("Pattern {} Slices", p + 1)), 1.0, MAX_SLICES as f32, 8.0, Unit::None).hide());
        for s in 0..MAX_SLICES {
            for (f, (n, lo, hi, d)) in FIELDS.iter().enumerate() {
                // the default pattern: eight even slices over four beats, each playing itself
                let d = if f == F_OUT || f == F_IN { if s < 8 { s as f32 * 0.5 } else { 4.0 } } else { *d };
                v.push(Param::new(slice_id(p, s, f), leak(format!("P{} S{} {n}", p + 1, s + 1)), *lo, *hi, d, Unit::None).hide());
            }
        }
    }
    v
});

/// One slice, read from the pattern's values.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slice {
    pub f: [f32; NF],
}

/// A value ramped through the slice: `base + slope × shaped progress` (curve bends the ramp).
#[inline]
pub fn ramp(base: f32, slope: f32, curve: f32, u: f32) -> f32 {
    let k = 2f32.powf(-2.0 * curve);
    base + slope * u.clamp(0.0, 1.0).powf(k)
}

pub struct BeatBreaker {
    p: Values,
    rate: f32,
    clock: Clock,
    buf: [Vec<f32>; 2],
    w: usize,
    xo: [Crossover; 2],
    xo_wet: [Crossover; 2],
    filt: [Svf; 2],
    mix: Smooth,
    rng: Rng,
    clip: Option<Vec<f32>>,
    slices: Vec<Slice>,
    pat_beats: f32,
    pub playing_slice: usize,
    input_pos: f32,
}

const BUF_SECS: f32 = 26.0;

impl BeatBreaker {
    pub fn new(rate: f32) -> BeatBreaker {
        let n = (BUF_SECS * rate) as usize;
        let mut b = BeatBreaker {
            p: Values::new(PARAMS.as_slice()),
            rate,
            clock: Clock::default(),
            buf: [vec![0.0; n], vec![0.0; n]],
            w: 0,
            xo: [Crossover::new(rate, 100.0); 2],
            xo_wet: [Crossover::new(rate, 100.0); 2],
            filt: [Svf::default(); 2],
            mix: Smooth::new(rate, 20.0, 1.0),
            rng: Rng(0x9e37_79b9_7f4a_7c15),
            clip: None,
            slices: Vec::new(),
            pat_beats: 4.0,
            playing_slice: 0,
            input_pos: 0.0,
        };
        b.update();
        b
    }

    /// The pattern (0..10) the window edits.
    pub fn editing(&self) -> usize {
        (self.p.get(EDITING).round() as usize).clamp(1, 10) - 1
    }

    fn sel(&self) -> usize {
        (self.p.get(SELECTED).round() as usize).min(self.count(self.editing()) - 1)
    }

    fn count(&self, p: usize) -> usize {
        (self.p.get(pat_count(p)).round() as usize).clamp(1, MAX_SLICES)
    }

    /// Slices of a pattern, ordered by Output Beat.
    pub fn slices_of(&self, p: usize) -> Vec<Slice> {
        (0..self.count(p)).map(|s| Slice { f: std::array::from_fn(|f| self.p.get(slice_id(p, s, f))) }).collect()
    }

    fn write_slices(&mut self, p: usize, sl: &[Slice]) {
        let mut sl = sl.to_vec();
        sl.sort_by(|a, b| a.f[F_OUT].partial_cmp(&b.f[F_OUT]).unwrap_or(std::cmp::Ordering::Equal));
        self.p.set(pat_count(p), sl.len() as f32);
        for (s, x) in sl.iter().enumerate() {
            for f in 0..NF {
                self.p.set(slice_id(p, s, f), x.f[f]);
            }
        }
    }

    fn default_slices() -> Vec<Slice> {
        (0..8).map(|s| {
            let mut f: [f32; NF] = std::array::from_fn(|k| FIELDS[k].3);
            f[F_OUT] = s as f32 * 0.5;
            f[F_IN] = s as f32 * 0.5;
            Slice { f }
        }).collect()
    }

    fn snap_value(&self, f: usize, v: f32) -> f32 {
        let beats = |idx: usize| [0.0, 1.0, 0.5, 0.25, 0.125][idx.min(4)];
        let fine = |idx: usize, range: f32| [0.0, 0.01, 0.05, 0.125][idx.min(3)] * range;
        let q = |v: f32, step: f32| if step > 0.0 { (v / step).round() * step } else { v };
        match f {
            F_OUT => q(v, beats(self.p.idx(SNAP_OUT))),
            F_IN => q(v, beats(self.p.idx(SNAP_IN))),
            F_SPEED => match self.p.idx(SNAP_SPD) {
                // Time: musical ratios (×¼ … ×4, both directions); Pitch: semitone ratios
                1 => {
                    let r = [25.0f32, 50.0, 100.0, 200.0, 400.0];
                    let a = r.iter().copied().min_by(|x, y| (x - v.abs()).abs().partial_cmp(&(y - v.abs()).abs()).unwrap()).unwrap();
                    a * v.signum()
                }
                2 => {
                    if v.abs() < 1.0 {
                        v
                    } else {
                        let st = (12.0 * (v.abs() / 100.0).log2()).round();
                        100.0 * 2f32.powf(st / 12.0) * v.signum()
                    }
                }
                _ => v,
            },
            F_CUT => q(v, fine(self.p.idx(SNAP_CUT), 1.0)),
            F_RES => q(v, fine(self.p.idx(SNAP_RES), 1.0)),
            F_VOL => q(v, fine(self.p.idx(SNAP_VOL), 1.0)),
            F_PAN => q(v, fine(self.p.idx(SNAP_PAN), 2.0)),
            F_REP => v.round(),
            F_FTYPE => v.round(),
            _ => v,
        }
    }

    /// Fields belonging to a mode (6 = Output Beat).
    fn mode_fields(mode: usize) -> &'static [usize] {
        match mode {
            0 => &[F_IN, F_SPEED, F_TCURVE],
            1 => &[F_REP],
            2 => &[F_FTYPE, F_CUT, F_CSLOPE, F_CCURVE],
            3 => &[F_RES],
            4 => &[F_VOL, F_VSLOPE, F_VCURVE],
            5 => &[F_PAN, F_PSLOPE, F_PCURVE],
            _ => &[F_OUT],
        }
    }

    fn randomize(&mut self) {
        let modes: Vec<usize> = if self.p.idx(RANDOM_MODE) == 0 { vec![self.p.idx(EDIT_MODE)] } else { (0..7).filter(|m| self.p.on(ENABLED + *m as u32)).collect() };
        let pat = self.editing();
        let mut sl = self.slices_of(pat);
        let len = self.pat_len_beats(pat);
        let allowed: Vec<f32> = (0..8).filter(|r| self.p.on(RAND_REPEATS + r)).map(|r| (r + 1) as f32).collect();
        let allowed = if allowed.is_empty() { vec![1.0] } else { allowed };
        for m in modes {
            for (k, (_, mode, f)) in RAND_NAMES.iter().enumerate() {
                if *mode != m {
                    continue;
                }
                let name = RAND_NAMES[k].0;
                if !name.ends_with("Probability") {
                    continue;
                }
                let prob = self.p.get(RAND + k as u32) / 100.0;
                let inten = RAND_NAMES.get(k + 1).filter(|n| n.0.ends_with("Intensity") && n.2 == *f).map(|_| self.p.get(RAND + k as u32 + 1) / 100.0).unwrap_or(1.0);
                let (_, lo, hi, _) = FIELDS[*f];
                let (lo, hi) = if *f == F_IN || *f == F_OUT { (0.0, len) } else { (lo, hi) };
                for (si, s) in sl.iter_mut().enumerate() {
                    if self.rng.next() >= prob {
                        continue;
                    }
                    let v = match *f {
                        F_REP => allowed[(self.rng.next() * allowed.len() as f32) as usize % allowed.len()],
                        F_FTYPE => (self.rng.next() >= 0.5) as u8 as f32,
                        F_OUT if si == 0 => 0.0,
                        _ => {
                            // within ±intensity/2 of the full range around the current value
                            let dev = (self.rng.next() * 2.0 - 1.0) * inten * 0.5 * (hi - lo);
                            (s.f[*f] + dev).clamp(lo, hi)
                        }
                    };
                    s.f[*f] = self.snap_value(*f, v);
                }
            }
        }
        self.write_slices(pat, &sl);
    }

    fn pat_len_beats(&self, p: usize) -> f32 {
        LENGTH_BEATS[self.p.idx(pat_len(p)).min(LENGTH_BEATS.len() - 1)]
    }

    fn clear_mode(&mut self, mode: usize) {
        let pat = self.editing();
        let mut sl = self.slices_of(pat);
        let n = sl.len().max(1) as f32;
        let len = self.pat_len_beats(pat);
        for (i, s) in sl.iter_mut().enumerate() {
            for f in BeatBreaker::mode_fields(mode) {
                s.f[*f] = match *f {
                    F_IN => s.f[F_OUT],
                    F_OUT => i as f32 * len / n,
                    _ => FIELDS[*f].3,
                };
            }
            if mode == 6 {
                s.f[F_IN] = s.f[F_OUT];
            }
        }
        self.write_slices(pat, &sl);
    }

    fn update(&mut self) {
        // one-shot buttons
        if self.p.on(RANDOMIZE) {
            self.p.set(RANDOMIZE, 0.0);
            self.randomize();
        }
        let c = self.p.get(CLEAR).round() as usize;
        if c > 0 {
            self.p.set(CLEAR, 0.0);
            self.clear_mode(c - 1);
        }
        let a = self.p.idx(ACTION);
        if a > 0 {
            self.p.set(ACTION, 0.0);
            let pat = self.editing();
            match a {
                1 => {
                    self.p.set(pat_len(pat), 3.0);
                    self.write_slices(pat, &BeatBreaker::default_slices());
                }
                2 => {
                    let mut v = vec![self.p.get(pat_len(pat)), self.p.get(pat_count(pat))];
                    v.extend((0..MAX_SLICES * NF).map(|k| self.p.get(slice_id(pat, k / NF, k % NF))));
                    self.clip = Some(v);
                }
                3 => {
                    if let Some(v) = self.clip.clone() {
                        self.p.set(pat_len(pat), v[0]);
                        self.p.set(pat_count(pat), v[1]);
                        for k in 0..MAX_SLICES * NF {
                            self.p.set(slice_id(pat, k / NF, k % NF), v[2 + k]);
                        }
                    }
                }
                4 | 5 => {
                    let to = if a == 4 { (pat + 9) % 10 } else { (pat + 1) % 10 };
                    self.p.set(EDITING, (to + 1) as f32);
                    self.p.set(PATTERN, (to + 1) as f32);
                }
                _ => {}
            }
        }
        let pat = self.p.idx(PATTERN);
        if pat > 0 && self.p.get(EDITING).round() as usize != pat {
            self.p.set(EDITING, pat as f32);
        }
        // what plays
        let play = if pat == 0 { None } else { Some(pat - 1) };
        if let Some(pp) = play {
            self.slices = self.slices_of(pp);
            self.pat_beats = self.pat_len_beats(pp);
            // keep the Length menu showing the playing pattern's
            let li = self.p.get(pat_len(pp));
            if (self.p.get(LENGTH) - li).abs() > 0.5 {
                self.p.set(LENGTH, li);
            }
        } else {
            self.slices.clear();
        }
        self.mix.target = self.p.get(MIX) / 100.0;
        let bb = self.p.get(BYPASS_BELOW);
        for x in self.xo.iter_mut().chain(self.xo_wet.iter_mut()) {
            x.set(self.rate, bb.max(20.0));
        }
    }

    fn clear(&mut self) {
        for b in self.buf.iter_mut() {
            b.iter_mut().for_each(|x| *x = 0.0);
        }
        for x in self.xo.iter_mut().chain(self.xo_wet.iter_mut()) {
            x.clear();
        }
        for f in self.filt.iter_mut() {
            f.clear();
        }
    }

    fn tail_secs(&self) -> f32 {
        0.1
    }

    /// Where in the input the pattern is reading now (beats into the pattern), for the window.
    pub fn input_position(&self) -> f32 {
        self.input_pos
    }

    #[inline]
    fn read(&self, s: usize, back: f64) -> f32 {
        let n = self.buf[s].len();
        let back = back.clamp(0.0, (n - 2) as f64);
        let i = back as usize;
        let t = (back - i as f64) as f32;
        let a = self.buf[s][(self.w + n - 1 - i) % n];
        let b = self.buf[s][(self.w + n - 2 - i) % n];
        a * (1.0 - t) + b * t
    }
}

impl Effect for BeatBreaker {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let spb = beat_samples(rate, self.clock.bpm);
        let len = self.pat_beats as f64;
        let bypass_lows = self.p.get(BYPASS_BELOW) > 20.5;
        let declick = (self.p.get(DECLICK) * 0.001 * rate).max(1.0) as f64;
        let nb = self.buf[0].len();
        for i in 0..n {
            let beat = self.clock.tick();
            let (dl, dr) = (left[i], right[i]);
            self.buf[0][self.w] = dl;
            self.buf[1][self.w] = dr;
            self.w = (self.w + 1) % nb;
            if self.slices.is_empty() {
                continue;
            }
            let ph = beat.rem_euclid(len);
            // the slice playing
            let k = self.slices.iter().rposition(|s| s.f[F_OUT] as f64 <= ph).unwrap_or(self.slices.len() - 1);
            self.playing_slice = k;
            let sl = self.slices[k];
            let start = sl.f[F_OUT] as f64;
            let end = self.slices.get(k + 1).map_or(len, |s| s.f[F_OUT] as f64).max(start + 1e-3);
            let dur = end - start;
            let mut t = (ph - start).max(0.0);
            let reps = sl.f[F_REP].round().clamp(1.0, 8.0) as f64;
            let seg = dur / reps;
            t %= seg;
            let u = (t / seg) as f32;
            // Time: where in the input this sample comes from (beats into the pattern)
            let speed = sl.f[F_SPEED] as f64 / 100.0;
            let k_curve = 2f64.powf(-2.0 * sl.f[F_TCURVE] as f64);
            let warped = seg * (u as f64).powf(k_curve);
            let mut src = sl.f[F_IN] as f64 + speed * warped;
            if speed < 0.0 {
                // reverse: start from the slice's end
                src += seg;
            }
            self.input_pos = src as f32;
            // the input buffer holds the last pattern: a later point reads the previous pass
            let cycle_start = beat - ph;
            let mut abs = cycle_start + src;
            while abs > beat + 1e-6 {
                abs -= len;
            }
            let back = (beat - abs) * spb;
            let (mut wl, mut wr) = if back < (nb - 4) as f64 { (self.read(0, back), self.read(1, back)) } else { (0.0, 0.0) };
            // De-click: fades at segment edges where the read point jumps
            let neutral = (sl.f[F_IN] - sl.f[F_OUT]).abs() < 1e-4 && (speed - 1.0).abs() < 1e-4 && reps == 1.0 && sl.f[F_TCURVE].abs() < 1e-4;
            if !neutral {
                let ts = t * spb;
                let te = (seg - t) * spb;
                let g = (ts.min(te) / declick).min(1.0) as f32;
                wl *= g;
                wr *= g;
            }
            // Cutoff and Resonance
            let cut = ramp(sl.f[F_CUT], sl.f[F_CSLOPE], sl.f[F_CCURVE], u).clamp(0.0, 1.0);
            let hp = sl.f[F_FTYPE] >= 0.5;
            let res = sl.f[F_RES];
            let filtering = if hp { cut > 0.001 || res > 0.001 } else { cut < 0.999 || res > 0.001 };
            if filtering {
                let hz = 20.0 * 1000f32.powf(cut);
                let q = 0.6 + 14.0 * res * res;
                for (s, v) in [&mut wl, &mut wr].into_iter().enumerate() {
                    self.filt[s].set(rate, hz, q);
                    let o = self.filt[s].run(*v);
                    *v = if hp { o.high } else { o.low };
                }
            }
            // Volume and Pan
            let vol = ramp(sl.f[F_VOL], sl.f[F_VSLOPE], sl.f[F_VCURVE], u).clamp(0.0, 1.0);
            let pan = ramp(sl.f[F_PAN], sl.f[F_PSLOPE], sl.f[F_PCURVE], u).clamp(-1.0, 1.0);
            let (gl, gr) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
            wl *= vol * gl;
            wr *= vol * gr;
            let m = self.mix.next();
            if bypass_lows {
                // lows from the dry signal, highs processed
                let (lo_l, _) = self.xo[0].run(dl);
                let (lo_r, _) = self.xo[1].run(dr);
                let (_, hi_l) = self.xo_wet[0].run(wl);
                let (_, hi_r) = self.xo_wet[1].run(wr);
                left[i] = dl * (1.0 - m) + (lo_l + hi_l) * m;
                right[i] = dr * (1.0 - m) + (lo_r + hi_r) * m;
            } else {
                left[i] = dl * (1.0 - m) + wl * m;
                right[i] = dr * (1.0 - m) + wr * m;
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.clock.set_tempo(bpm, self.rate);
    }
    fn set_transport(&mut self, t: Transport) {
        self.clock.sync(t, self.rate);
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Playing Slice", self.playing_slice as f32), ("Input Beat", self.input_pos), ("Position", (self.clock.beats.rem_euclid(self.pat_beats as f64)) as f32)]
    }
}

impl Plugin for BeatBreaker {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        if (SEL..SEL + NF as u32).contains(&id) {
            return self.p.get(slice_id(self.editing(), self.sel(), (id - SEL) as usize));
        }
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        if (SEL..SEL + NF as u32).contains(&id) {
            let f = (id - SEL) as usize;
            let (pat, s) = (self.editing(), self.sel());
            let v = self.snap_value(f, value);
            self.p.set(slice_id(pat, s, f), v);
            if f == F_OUT {
                let sl = self.slices_of(pat);
                self.write_slices(pat, &sl);
            }
            self.update();
            return;
        }
        if id == LENGTH {
            let pat = self.editing();
            self.p.set(pat_len(pat), value);
        }
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

/// Slice editing for the window: add a slice marker at a beat, move one, delete one.
impl BeatBreaker {
    pub fn add_slice(&mut self, at: f32) {
        let pat = self.editing();
        let mut sl = self.slices_of(pat);
        if sl.len() >= MAX_SLICES {
            return;
        }
        let at = self.snap_value(F_OUT, at).clamp(0.0, self.pat_len_beats(pat) - 0.01);
        if sl.iter().any(|s| (s.f[F_OUT] - at).abs() < 1e-3) {
            return;
        }
        let mut f: [f32; NF] = std::array::from_fn(|k| FIELDS[k].3);
        f[F_OUT] = at;
        f[F_IN] = at;
        sl.push(Slice { f });
        self.write_slices(pat, &sl);
        self.update();
    }
    pub fn delete_slice(&mut self, k: usize) {
        let pat = self.editing();
        let mut sl = self.slices_of(pat);
        if sl.len() <= 1 || k >= sl.len() || k == 0 {
            return;
        }
        sl.remove(k);
        self.write_slices(pat, &sl);
        self.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_signal(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i % 48000) as f32 / 48000.0).collect()
    }

    fn run(bb: &mut BeatBreaker, input: &[f32]) -> Vec<f32> {
        let mut l = input.to_vec();
        let mut r = l.clone();
        for (cl, cr) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            bb.process(cl, cr);
        }
        l
    }

    #[test]
    fn default_pattern_is_transparent() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        let x: Vec<f32> = (0..96000).map(|i| (i as f32 * 0.01).sin()).collect();
        let y = run(&mut bb, &x);
        let err = x.iter().zip(&y).skip(10).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "{err}");
    }

    #[test]
    fn time_mode_moves_a_slice() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        // slice 2 (beats 1.0..1.5) plays input beat 0.0
        bb.set(SELECTED, 2.0);
        bb.set(SEL + F_IN as u32, 0.0);
        bb.set(DECLICK, 0.0);
        // a counter as input: output value = which sample it came from
        let x: Vec<f32> = (0..96000).map(|i| i as f32).collect();
        let y = run(&mut bb, &x);
        // beat 1.25 of the second pass (pattern = 4 beats = 96000 samples at 120): sample 60000 + 96000
        // first pass only: at beat 1.25 (sample 30000) we hear beat 0.25 (sample 6000)
        assert!((y[30000] - 6000.0).abs() < 2.0, "{}", y[30000]);
        assert!((y[20000] - 20000.0).abs() < 1.0);
    }

    #[test]
    fn reverse_speed_plays_backwards() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        bb.set(SELECTED, 1.0);
        bb.set(SEL + F_SPEED as u32, -100.0);
        bb.set(DECLICK, 0.0);
        let x: Vec<f32> = (0..192000).map(|i| i as f32).collect();
        let y = run(&mut bb, &x);
        // slice 1 = beats 0.5..1.0 = samples 12000..24000 of each pass, read backwards from
        // 24000: the previous pass's (the input buffer only holds what has played)
        assert!(y[96000 + 13000] > y[96000 + 14000], "{} {}", y[109000], y[110000]);
        assert!((y[96000 + 13000] - (24000.0 - 1000.0)).abs() < 2.0, "{}", y[109000]);
    }

    #[test]
    fn repeats_stutter() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        bb.set(SELECTED, 0.0);
        bb.set(SEL + F_REP as u32, 2.0);
        bb.set(DECLICK, 0.0);
        let x: Vec<f32> = (0..48000).map(|i| i as f32).collect();
        let y = run(&mut bb, &x);
        // slice 0 = 0..12000: the first 6000 play twice
        assert!((y[7000] - 1000.0).abs() < 1.0, "{}", y[7000]);
    }

    #[test]
    fn volume_slope_fades_out() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        bb.set(SELECTED, 0.0);
        bb.set(SEL + F_VSLOPE as u32, -1.0);
        let y = run(&mut bb, &vec![1.0; 12000]);
        assert!(y[1000] > 0.85 && y[11900] < 0.05, "{} {}", y[1000], y[11900]);
    }

    #[test]
    fn cutoff_lowpass_darkens() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set_tempo(120.0);
        for s in 0..8 {
            bb.set(SELECTED, s as f32);
            bb.set(SEL + F_CUT as u32, 0.3);
        }
        let x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 8000.0 * i as f32 / 48000.0).sin()).collect();
        let y = run(&mut bb, &x);
        let peak = y[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.05, "{peak}");
    }

    #[test]
    fn pattern_off_is_dry() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set(SELECTED, 0.0);
        bb.set(SEL + F_VOL as u32, 0.0);
        bb.set(PATTERN, 0.0);
        let y = run(&mut bb, &vec![0.5; 2000]);
        assert!(y.iter().all(|v| (*v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn randomize_follows_probability() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set(EDIT_MODE, 1.0); // Repeat
        bb.set(RAND + 6, 0.0); // probability 0: nothing changes
        bb.set(RANDOMIZE, 1.0);
        assert!(bb.slices_of(0).iter().all(|s| s.f[F_REP] == 1.0));
        bb.set(RAND + 6, 100.0);
        for r in 0..8 {
            bb.set(RAND_REPEATS + r, if r == 3 { 1.0 } else { 0.0 });
        }
        bb.set(RANDOMIZE, 1.0);
        assert!(bb.slices_of(0).iter().all(|s| s.f[F_REP] == 4.0));
    }

    #[test]
    fn randomize_intensity_bounds() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.set(EDIT_MODE, 4.0); // Volume
        bb.set(SNAP_VOL, 0.0);
        // Volume 0.5 everywhere, Intensity 50 %: results within 0.25..0.75
        for s in 0..8 {
            bb.set(SELECTED, s as f32);
            bb.set(SEL + F_VOL as u32, 0.5);
        }
        bb.set(RAND + 16, 100.0);
        bb.set(RAND + 17, 50.0);
        bb.set(RAND + 18, 0.0);
        bb.set(RAND + 20, 0.0);
        bb.set(RANDOMIZE, 1.0);
        for s in bb.slices_of(0) {
            assert!((0.25..=0.75).contains(&s.f[F_VOL]), "{}", s.f[F_VOL]);
        }
        let _ = ramp_signal(1);
    }

    #[test]
    fn slices_add_and_delete() {
        let mut bb = BeatBreaker::new(48000.0);
        bb.add_slice(3.75);
        assert_eq!(bb.slices_of(0).len(), 9);
        bb.delete_slice(8);
        assert_eq!(bb.slices_of(0).len(), 8);
    }
}
