//! Step FX (Multi Effects ▸ Step FX), as the Effects guide describes it:
//! - five processors in a reorderable series chain (the Effects order strip): Mod FX (Delay,
//!   Rate, Depth, Feedback, Mix), Delay (filter type LP / BP / HP / LP+LoCut, Feedback, Cutoff,
//!   Mix, Sync, Time L / R), Filter (the Smooth / Edgy / Rich / Sharp / Clean / Gritty LP, BP
//!   and HP designs, Notch, Peaking, three combs, RingMod and four distortions; Cutoff, Res, a
//!   third control (Drive / Amount / Gain / Damp) and Mix), Distortion (Exciter, Dirt) and
//!   Reverb (Type, Time, Mix);
//! - three step modulators, each with its own target (any processor parameter, Gate Mix, Gate
//!   Add or Pan), Depth (negative inverts), Rate, Swing, an Attack / Hold / Release envelope as
//!   shares of the step (with attack and release curves), up to 128 steps with level, length
//!   (ties beyond 100 %) and on / off, and the Action menu (Random Full, Random Deviation, Shift
//!   Left / Right, Double Note Length, Reverse, Invert, Append Duplicate, Append Reverse
//!   Duplicate) and pattern commands (Recall Default, Copy, Paste);
//! - the XY pad with up to two targets per axis and their depths; Input, Mix, Output.
//!
//! Gate Mix multiplies (Depth 0 % changes nothing, 100 % silences the lowest steps); Gate Add
//! adds (one Gate Add at 0 % is silence, at 100 % sounds like Gate Mix). Ids are our own
//! (Logic's block isn't decoded): globals from 1000, steps hidden from 3000.

use crate::kit::{Clock, Fdn, NOTE_BEATS, NOTE_NAMES, Rng, Svf, beat_samples, leak};
use crate::util::{Biquad, Coefs, Delay, Phase, Smooth, Values, db, flush, sine01};
use lg_dsp::{Effect, Param, Plugin, Transport, Unit};

pub const PROCESSORS: &[&str] = &["Mod FX", "Delay", "Filter", "Distortion", "Reverb"];
pub const FILTER_TYPES: &[&str] = &[
    "LP 12dB Smooth", "LP 24dB Smooth", "LP 12dB Edgy", "LP 24dB Edgy", "LP 12dB Rich", "LP 24dB Rich", "LP 12dB Sharp", "LP 12dB Clean", "LP 12dB Gritty",
    "BP 6dB Smooth", "BP 12dB Smooth", "BP 6dB Edgy", "BP 12dB Rich", "BP 6dB Clean", "BP 6dB Gritty",
    "HP 12dB Smooth", "HP 24dB Smooth", "HP 12dB Edgy", "HP 24dB Edgy", "HP 12dB Rich", "HP 24dB Rich", "HP 12dB Sharp", "HP 12dB Clean", "HP 12dB Gritty",
    "Notch", "Peaking", "Comb Pos", "Comb Neg", "Comb PM", "RingMod", "Bitcrusher", "Dirt", "Downsampler", "Grit",
];
const DELAY_FILTERS: &[&str] = &["LP", "BP", "HP", "LP+LoCut"];
const REVERB_TYPES: &[&str] = &["Room", "Hall", "Plate", "Bright Room", "Dark Hall"];
/// What a step modulator or an XY axis can move.
pub const TARGETS: &[&str] = &[
    "Off", "Gate Mix", "Gate Add", "Pan", "Mod FX Delay", "Mod FX Rate", "Mod FX Depth", "Mod FX Feedback", "Mod FX Mix", "Delay Feedback", "Delay Cutoff", "Delay Mix",
    "Filter Cutoff", "Filter Res", "Filter Drive", "Filter Mix", "Distortion Exciter", "Distortion Dirt", "Reverb Time", "Reverb Mix", "Input", "Mix", "Output",
];
pub const ACTIONS: &[&str] = &["—", "Random Full", "Random Deviation", "Shift Left", "Shift Right", "Double Note Length", "Reverse", "Invert", "Append Duplicate", "Append Reverse Duplicate", "Recall Default", "Copy", "Paste"];

// processors (ids) — the param a target moves is TARGET_PARAM[target]
pub const MOD_ON: u32 = 1000;
pub const MOD_DELAY: u32 = 1001;
pub const MOD_RATE: u32 = 1002;
pub const MOD_DEPTH: u32 = 1003;
pub const MOD_FB: u32 = 1004;
pub const MOD_MIX: u32 = 1005;
pub const DLY_ON: u32 = 1010;
pub const DLY_FTYPE: u32 = 1011;
pub const DLY_FB: u32 = 1012;
pub const DLY_CUT: u32 = 1013;
pub const DLY_MIX: u32 = 1014;
pub const DLY_SYNC: u32 = 1015;
pub const DLY_TL: u32 = 1016;
pub const DLY_TR: u32 = 1017;
pub const DLY_DL: u32 = 1018;
pub const DLY_DR: u32 = 1019;
pub const FLT_ON: u32 = 1020;
pub const FLT_TYPE: u32 = 1021;
pub const FLT_CUT: u32 = 1022;
pub const FLT_RES: u32 = 1023;
pub const FLT_DRIVE: u32 = 1024;
pub const FLT_MIX: u32 = 1025;
pub const DST_ON: u32 = 1030;
pub const DST_EXC: u32 = 1031;
pub const DST_DIRT: u32 = 1032;
pub const REV_ON: u32 = 1040;
pub const REV_TYPE: u32 = 1041;
pub const REV_TIME: u32 = 1042;
pub const REV_MIX: u32 = 1043;
pub const INPUT: u32 = 1050;
pub const MIX: u32 = 1051;
pub const OUTPUT: u32 = 1052;
/// The effects order: five slots holding a processor each.
pub const ORDER: u32 = 1060;
/// XY pad: X On, X Target 1, X Target 2, X Depth 1, X Depth 2, then Y the same, then X, Y.
pub const XY: u32 = 1070;
/// The step modulator being edited.
pub const SELECTED_MOD: u32 = 1099;
/// Step modulator m's controls at 1100 + 40 m (MOD_FIELDS order).
pub const fn mod_id(m: usize, f: u32) -> u32 {
    1100 + 40 * m as u32 + f
}
pub const M_ON: u32 = 0;
pub const M_TARGET: u32 = 1;
pub const M_DEPTH: u32 = 2;
pub const M_RATE: u32 = 3;
pub const M_SWING: u32 = 4;
pub const M_ATTACK: u32 = 5;
pub const M_ACURVE: u32 = 6;
pub const M_HOLD: u32 = 7;
pub const M_RELEASE: u32 = 8;
pub const M_RCURVE: u32 = 9;
pub const M_LENGTH: u32 = 10;
pub const M_ACTION: u32 = 11;
pub const MAX_STEPS: usize = 128;
/// Step s of modulator m: level, length (1 = full step, up to 8 = tied over 7 more), on.
pub const fn step_id(m: usize, s: usize, f: u32) -> u32 {
    3000 + 400 * m as u32 + 3 * s as u32 + f
}

/// The parameter each target moves (by index into TARGETS; 0..3 are gate / pan).
const TARGET_PARAM: [u32; 23] = [0, 0, 0, 0, MOD_DELAY, MOD_RATE, MOD_DEPTH, MOD_FB, MOD_MIX, DLY_FB, DLY_CUT, DLY_MIX, FLT_CUT, FLT_RES, FLT_DRIVE, FLT_MIX, DST_EXC, DST_DIRT, REV_TIME, REV_MIX, INPUT, MIX, OUTPUT];
const RATES: [usize; 8] = [4, 7, 10, 13, 5, 8, 11, 9];

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::new(MOD_ON, "Mod FX On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(MOD_DELAY, "Mod FX Delay", 0.5, 30.0, 5.0, Unit::Ms),
        Param::new(MOD_RATE, "Mod FX Rate", 0.05, 10.0, 0.5, Unit::Hz),
        Param::new(MOD_DEPTH, "Mod FX Depth", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(MOD_FB, "Mod FX Feedback", -100.0, 100.0, 0.0, Unit::Percent),
        Param::new(MOD_MIX, "Mod FX Mix", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(DLY_ON, "Delay On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(DLY_FTYPE, "Delay Filter Type", DELAY_FILTERS, 0.0),
        Param::new(DLY_FB, "Delay Feedback", 0.0, 100.0, 35.0, Unit::Percent),
        Param::new(DLY_CUT, "Delay Cutoff", 100.0, 20000.0, 8000.0, Unit::Hz),
        Param::new(DLY_MIX, "Delay Mix", 0.0, 100.0, 30.0, Unit::Percent),
        Param::new(DLY_SYNC, "Delay Sync", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(DLY_TL, "Delay Time L", 1.0, 2000.0, 375.0, Unit::Ms),
        Param::new(DLY_TR, "Delay Time R", 1.0, 2000.0, 500.0, Unit::Ms),
        Param::menu(DLY_DL, "Delay Time L (Sync)", NOTE_NAMES, 9.0),
        Param::menu(DLY_DR, "Delay Time R (Sync)", NOTE_NAMES, 4.0),
        Param::new(FLT_ON, "Filter On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(FLT_TYPE, "Filter Type", FILTER_TYPES, 4.0),
        Param::new(FLT_CUT, "Filter Cutoff", 0.0, 1.0, 0.6, Unit::None),
        Param::new(FLT_RES, "Filter Res", 0.0, 1.0, 0.2, Unit::None),
        Param::new(FLT_DRIVE, "Filter Drive", 0.0, 1.0, 0.0, Unit::None),
        Param::new(FLT_MIX, "Filter Mix", 0.0, 100.0, 100.0, Unit::Percent),
        Param::new(DST_ON, "Distortion On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(DST_EXC, "Distortion Exciter", 0.0, 100.0, 0.0, Unit::Percent),
        Param::new(DST_DIRT, "Distortion Dirt", 0.0, 100.0, 30.0, Unit::Percent),
        Param::new(REV_ON, "Reverb On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(REV_TYPE, "Reverb Type", REVERB_TYPES, 0.0),
        Param::new(REV_TIME, "Reverb Time", 0.1, 10.0, 1.5, Unit::Seconds),
        Param::new(REV_MIX, "Reverb Mix", 0.0, 100.0, 25.0, Unit::Percent),
        Param::new(INPUT, "Input", -24.0, 12.0, 0.0, Unit::Db),
        Param::new(MIX, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
        Param::new(OUTPUT, "Output", -24.0, 12.0, 0.0, Unit::Db),
    ];
    for k in 0..5 {
        v.push(Param::menu(ORDER + k, leak(format!("Effect {}", k + 1)), PROCESSORS, [2.0, 0.0, 1.0, 3.0, 4.0][k as usize]));
    }
    for (a, axis) in ["X", "Y"].iter().enumerate() {
        let b = XY + 5 * a as u32;
        v.push(Param::new(b, leak(format!("{axis} On")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::menu(b + 1, leak(format!("{axis} Target 1")), TARGETS, if a == 0 { 12.0 } else { 13.0 }));
        v.push(Param::menu(b + 2, leak(format!("{axis} Target 2")), TARGETS, 0.0));
        v.push(Param::new(b + 3, leak(format!("{axis} Depth 1")), -100.0, 100.0, 50.0, Unit::Percent));
        v.push(Param::new(b + 4, leak(format!("{axis} Depth 2")), -100.0, 100.0, 50.0, Unit::Percent));
    }
    v.push(Param::new(XY + 10, "XY Pad X", 0.0, 1.0, 0.5, Unit::None));
    v.push(Param::new(XY + 11, "XY Pad Y", 0.0, 1.0, 0.5, Unit::None));
    v.push(Param::new(SELECTED_MOD, "Selected Modulator", 0.0, 2.0, 0.0, Unit::None));
    for m in 0..3 {
        let n = m + 1;
        v.extend([
            Param::new(mod_id(m, M_ON), leak(format!("Step Mod {n} On")), 0.0, 1.0, if m == 0 { 1.0 } else { 0.0 }, Unit::OnOff),
            Param::menu(mod_id(m, M_TARGET), leak(format!("Step Mod {n} Target")), TARGETS, [1.0, 12.0, 3.0][m]),
            Param::new(mod_id(m, M_DEPTH), leak(format!("Step Mod {n} Depth")), -100.0, 100.0, 100.0, Unit::Percent),
            Param::menu(mod_id(m, M_RATE), leak(format!("Step Mod {n} Rate")), &["1/4", "1/8", "1/16", "1/32", "1/4 T", "1/8 T", "1/16 T", "1/8 ."], 2.0),
            Param::new(mod_id(m, M_SWING), leak(format!("Step Mod {n} Swing")), 0.0, 100.0, 0.0, Unit::Percent),
            Param::new(mod_id(m, M_ATTACK), leak(format!("Step Mod {n} Attack")), 0.0, 100.0, 0.0, Unit::Percent),
            Param::new(mod_id(m, M_ACURVE), leak(format!("Step Mod {n} Attack Curve")), -1.0, 1.0, 0.0, Unit::None),
            Param::new(mod_id(m, M_HOLD), leak(format!("Step Mod {n} Hold")), 0.0, 100.0, 100.0, Unit::Percent),
            Param::new(mod_id(m, M_RELEASE), leak(format!("Step Mod {n} Release")), 0.0, 100.0, 0.0, Unit::Percent),
            Param::new(mod_id(m, M_RCURVE), leak(format!("Step Mod {n} Release Curve")), -1.0, 1.0, 0.0, Unit::None),
            Param::new(mod_id(m, M_LENGTH), leak(format!("Step Mod {n} Steps")), 1.0, MAX_STEPS as f32, 16.0, Unit::None),
            Param::menu(mod_id(m, M_ACTION), leak(format!("Step Mod {n} Action")), ACTIONS, 0.0),
        ]);
        for s in 0..MAX_STEPS {
            v.push(Param::new(step_id(m, s, 0), leak(format!("M{n} Step {} Level", s + 1)), 0.0, 1.0, 1.0, Unit::None).hide());
            v.push(Param::new(step_id(m, s, 1), leak(format!("M{n} Step {} Length", s + 1)), 0.0, 8.0, 1.0, Unit::None).hide());
            v.push(Param::new(step_id(m, s, 2), leak(format!("M{n} Step {} On", s + 1)), 0.0, 1.0, 1.0, Unit::OnOff).hide());
        }
    }
    v
});

/// Filter design of a type index: (kind, poles, character).
fn ftype(t: usize) -> (&'static str, u8, &'static str) {
    let n = FILTER_TYPES.get(t).copied().unwrap_or("LP 12dB Smooth");
    let kind = &n[..2];
    let poles = if n.contains("24dB") { 4 } else if n.contains("6dB") { 1 } else { 2 };
    let ch = n.rsplit(' ').next().unwrap_or("");
    (if ["LP", "BP", "HP"].contains(&kind) { kind } else { n }, poles, ch)
}

struct MultiFilter {
    svf: [Svf; 2],
    comb: Delay,
    hold: f32,
    hold_ph: f32,
    ring: Phase,
    peak: Biquad,
}

impl MultiFilter {
    fn new(rate: f32) -> MultiFilter {
        MultiFilter { svf: [Svf::default(); 2], comb: Delay::new((0.05 * rate) as usize), hold: 0.0, hold_ph: 0.0, ring: Phase::default(), peak: Biquad::default() }
    }
    fn clear(&mut self) {
        self.svf[0].clear();
        self.svf[1].clear();
        self.comb.clear();
        self.peak.clear();
    }
    #[inline]
    fn run(&mut self, rate: f32, t: usize, cut: f32, res: f32, drive: f32, x: f32) -> f32 {
        let hz = 20.0 * 1000f32.powf(cut.clamp(0.0, 1.0));
        let (kind, poles, ch) = ftype(t);
        let pre = |x: f32| if drive > 0.001 && ch != "Clean" { (x * (1.0 + 8.0 * drive)).tanh() / (1.0 + 2.0 * drive) } else { x };
        match kind {
            "LP" | "BP" | "HP" => {
                let q = 0.5 + res * res * if ch == "Sharp" { 25.0 } else { 14.0 };
                let x = pre(x);
                self.svf[0].set(rate, hz, q);
                let pick = |o: crate::kit::SvfOut| match kind {
                    "LP" => o.low,
                    "HP" => o.high,
                    _ => o.band,
                };
                let mut y = pick(self.svf[0].run(x));
                if ch == "Edgy" || ch == "Rich" {
                    y = (y * 1.3).tanh() / 1.3;
                }
                if poles == 4 {
                    self.svf[1].set(rate, hz, 0.7);
                    y = pick(self.svf[1].run(y));
                }
                if ch == "Gritty" {
                    y = (y * (2.0 + 6.0 * res + 6.0 * drive)).tanh() * 0.5;
                }
                y
            }
            "Notch" => {
                self.svf[0].set(rate, hz, 0.5 + 8.0 * (1.0 - res));
                let o = self.svf[0].run(x);
                x - o.band * (0.2 + 0.8 * drive.max(0.5))
            }
            "Peaking" => {
                self.peak.c = Coefs::peak(rate, hz.min(rate * 0.45), 0.3 + 6.0 * (1.0 - drive), 18.0 * res);
                self.peak.run(x)
            }
            "Comb Pos" | "Comb Neg" | "Comb PM" => {
                let d = (rate / hz.max(30.0)).min((0.049 * rate) as f32).max(2.0);
                let fb = match kind {
                    "Comb Pos" => res * 0.95,
                    "Comb Neg" => -res * 0.95,
                    _ => (res * 2.0 - 1.0) * 0.97,
                };
                let y = self.comb.read(d);
                // Damp: a gentle low-pass inside the loop
                let damped = y * (1.0 - 0.6 * drive) + self.hold * 0.6 * drive;
                self.hold = damped;
                self.comb.push(x + damped * fb);
                0.5 * (x + y)
            }
            "RingMod" => {
                let c = sine01(self.ring.step(rate, 20.0 * 250f32.powf(cut)));
                let off = drive;
                x * (c * (1.0 - off) + off * (0.5 + 0.5 * c))
            }
            "Bitcrusher" => {
                let bits = 16.0 - 14.0 * res.max(drive);
                let q = 2f32.powf(bits);
                (x * q).round() / q
            }
            "Dirt" => (x * (1.0 + 20.0 * res.max(drive))).tanh() * 0.7,
            "Downsampler" => {
                self.hold_ph += hz / rate;
                if self.hold_ph >= 1.0 {
                    self.hold_ph -= 1.0;
                    self.hold = x;
                }
                self.hold
            }
            _ => {
                // Grit: transistor-style hard clip with crossover
                let y = x * (1.0 + 30.0 * res.max(drive));
                let y = if y.abs() < 0.05 { y * 0.3 } else { y };
                y.clamp(-0.8, 0.8)
            }
        }
    }
}

pub struct StepFx {
    p: Values,
    rate: f32,
    clock: Clock,
    rng: Rng,
    clip: Option<Vec<f32>>,
    // modulated values this sample (target index → offset)
    filt: [MultiFilter; 2],
    mod_line: [Delay; 2],
    mod_ph: Phase,
    mod_fb: [f32; 2],
    dly: [Delay; 2],
    dly_f: [Svf; 2],
    dly_lc: [Biquad; 2],
    exc_hp: [Biquad; 2],
    verb: Fdn,
    gate: Smooth,
    pan: Smooth,
    pub step_now: [usize; 3],
    pub mod_value: [f32; 3],
    ranges: [(f32, f32); 23],
}

impl StepFx {
    pub fn new(rate: f32) -> StepFx {
        let mut s = StepFx {
            p: Values::new(PARAMS.as_slice()),
            rate,
            clock: Clock::default(),
            rng: Rng(0x5eed_1234_abcd_ef01),
            clip: None,
            filt: [MultiFilter::new(rate), MultiFilter::new(rate)],
            mod_line: [Delay::new((0.06 * rate) as usize), Delay::new((0.06 * rate) as usize)],
            mod_ph: Phase::default(),
            mod_fb: [0.0; 2],
            dly: [Delay::new((4.1 * rate) as usize), Delay::new((4.1 * rate) as usize)],
            dly_f: [Svf::default(); 2],
            dly_lc: [Biquad::new(Coefs::highpass(rate, 200.0, 0.7)); 2],
            exc_hp: [Biquad::new(Coefs::highpass(rate, 3000.0, 0.7)); 2],
            verb: Fdn::new(rate),
            gate: Smooth::new(rate, 1.0, 1.0),
            pan: Smooth::new(rate, 2.0, 0.0),
            step_now: [0; 3],
            mod_value: [0.0; 3],
            ranges: std::array::from_fn(|t| PARAMS.iter().find(|p| p.id == TARGET_PARAM[t]).map_or((0.0, 1.0), |p| (p.min, p.max))),
        };
        s.update();
        s
    }

    pub fn steps(&self, m: usize) -> usize {
        (self.p.get(mod_id(m, M_LENGTH)).round() as usize).clamp(1, MAX_STEPS)
    }

    fn step(&self, m: usize, s: usize) -> [f32; 3] {
        [self.p.get(step_id(m, s, 0)), self.p.get(step_id(m, s, 1)), self.p.get(step_id(m, s, 2))]
    }

    fn set_step(&mut self, m: usize, s: usize, v: [f32; 3]) {
        for f in 0..3 {
            self.p.set(step_id(m, s, f as u32), v[f]);
        }
    }

    /// The Action menu and the pattern commands on modulator m.
    pub fn action(&mut self, m: usize, a: usize) {
        let n = self.steps(m);
        let mut st: Vec<[f32; 3]> = (0..n).map(|s| self.step(m, s)).collect();
        match ACTIONS.get(a).copied().unwrap_or("") {
            "Random Full" => {
                for s in st.iter_mut() {
                    s[0] = self.rng.next();
                }
            }
            "Random Deviation" => {
                for s in st.iter_mut() {
                    s[0] = (s[0] + (self.rng.next() - 0.5) * 0.3).clamp(0.0, 1.0);
                }
            }
            "Shift Left" => st.rotate_left(1),
            "Shift Right" => st.rotate_right(1),
            "Double Note Length" => {
                for s in st.iter_mut() {
                    s[1] = (s[1] * 2.0).min(8.0);
                }
            }
            "Reverse" => st.reverse(),
            "Invert" => {
                for s in st.iter_mut() {
                    s[0] = 1.0 - s[0];
                }
            }
            "Append Duplicate" => {
                let c = st.clone();
                st.extend(c);
            }
            "Append Reverse Duplicate" => {
                let mut c = st.clone();
                c.reverse();
                st.extend(c);
            }
            "Recall Default" => {
                st = vec![[1.0, 1.0, 1.0]; 16];
                for f in [M_ATTACK, M_ACURVE, M_RELEASE, M_RCURVE, M_SWING] {
                    self.p.set(mod_id(m, f), 0.0);
                }
                self.p.set(mod_id(m, M_HOLD), 100.0);
            }
            "Copy" => {
                self.clip = Some(st.iter().flatten().copied().collect());
                return;
            }
            "Paste" => {
                if let Some(c) = &self.clip {
                    st = c.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
                }
            }
            _ => return,
        }
        st.truncate(MAX_STEPS);
        self.p.set(mod_id(m, M_LENGTH), st.len() as f32);
        for (s, v) in st.iter().enumerate() {
            self.set_step(m, s, *v);
        }
    }

    fn update(&mut self) {
        for m in 0..3 {
            let a = self.p.idx(mod_id(m, M_ACTION));
            if a > 0 {
                self.p.set(mod_id(m, M_ACTION), 0.0);
                self.action(m, a);
            }
        }
        // the order is a permutation: a slot set to a processor another slot holds swaps them
        let order: Vec<usize> = (0..5).map(|k| self.p.idx(ORDER + k)).collect();
        let mut seen = [false; 5];
        let mut fixed = order.clone();
        for v in fixed.iter_mut() {
            if seen[*v] {
                *v = (0..5).find(|x| !seen[*x] && !order.contains(x)).unwrap_or(*v);
            }
            seen[*v] = true;
        }
        for (k, v) in fixed.iter().enumerate() {
            self.p.set(ORDER + k as u32, *v as f32);
        }
        let r = self.rate;
        let t = self.p.idx(REV_TYPE);
        let size = [0.5, 1.4, 0.8, 0.6, 1.6][t.min(4)];
        let damp = [7000.0, 6000.0, 12000.0, 14000.0, 3000.0][t.min(4)];
        self.verb.set(self.p.get(REV_TIME), size, damp);
        let _ = r;
    }

    /// A step modulator's output now (0..1 before Depth), or None when off.
    fn modulator(&mut self, m: usize, beat: f64) -> Option<f32> {
        if !self.p.on(mod_id(m, M_ON)) || self.p.idx(mod_id(m, M_TARGET)) == 0 {
            return None;
        }
        let rate_beats = [1.0, 0.5, 0.25, 0.125, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0, 0.75][self.p.idx(mod_id(m, M_RATE)).min(7)] as f64;
        let n = self.steps(m);
        let pair = 2.0 * rate_beats;
        let pos = beat.rem_euclid(rate_beats * n as f64);
        // swing: odd steps longer, even shorter
        let sw = self.p.get(mod_id(m, M_SWING)) as f64 / 100.0 * 0.33;
        let pair_i = (pos / pair).floor();
        let in_pair = pos - pair_i * pair;
        let first = rate_beats * (1.0 + sw);
        let (s, t, dur) = if in_pair < first { (pair_i as usize * 2, in_pair, first) } else { (pair_i as usize * 2 + 1, in_pair - first, pair - first) };
        let s = s.min(n - 1);
        // a tied step continues the step it's tied from: find the start of this note
        let mut start = s;
        let mut t = t;
        while start > 0 {
            let prev = self.step(m, start - 1);
            if prev[2] >= 0.5 && prev[1] > (s - start + 1) as f32 {
                start -= 1;
                t += rate_beats;
            } else {
                break;
            }
        }
        self.step_now[m] = s;
        let st = self.step(m, start);
        if st[2] < 0.5 {
            return Some(0.0);
        }
        let len = (st[1] as f64 * rate_beats).max(1e-6) + if start == s { 0.0 } else { dur - rate_beats };
        if t >= len {
            return Some(0.0);
        }
        let u = (t / len) as f32;
        let (a, h, r) = (self.p.get(mod_id(m, M_ATTACK)) / 100.0, self.p.get(mod_id(m, M_HOLD)) / 100.0, self.p.get(mod_id(m, M_RELEASE)) / 100.0);
        let shape = |x: f32, c: f32| x.clamp(0.0, 1.0).powf(2f32.powf(-2.0 * c));
        let env = if u < a {
            shape(u / a.max(1e-4), self.p.get(mod_id(m, M_ACURVE)))
        } else if u < a + h {
            1.0
        } else if u < a + h + r {
            1.0 - shape((u - a - h) / r.max(1e-4), -self.p.get(mod_id(m, M_RCURVE)))
        } else if a + h + r >= 0.999 {
            1.0
        } else {
            0.0
        };
        Some(st[0] * env)
    }

    /// A target's parameter moved by a normalised offset.
    #[inline]
    fn moved(&self, id: u32, t: usize, off: f32) -> f32 {
        let (lo, hi) = self.ranges[t];
        let n = (self.p.get(id) - lo) / (hi - lo).max(1e-9);
        lo + (n + off).clamp(0.0, 1.0) * (hi - lo)
    }
}

impl Effect for StepFx {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let spb = beat_samples(rate, self.clock.bpm);
        let order: [usize; 5] = std::array::from_fn(|k| self.p.idx(ORDER + k as u32));
        let ftype_i = self.p.idx(FLT_TYPE);
        let dftype = self.p.idx(DLY_FTYPE);
        let ons = [self.p.on(MOD_ON), self.p.on(DLY_ON), self.p.on(FLT_ON), self.p.on(DST_ON), self.p.on(REV_ON)];
        // the XY pad's offsets (constant over the block)
        let mut xy_off = [0f32; 23];
        for a in 0..2 {
            let b = XY + 5 * a as u32;
            if !self.p.on(b) {
                continue;
            }
            let v = self.p.get(XY + 10 + a as u32) - 0.5;
            for k in 0..2 {
                let t = self.p.idx(b + 1 + k);
                xy_off[t] += v * 2.0 * self.p.get(b + 3 + k) / 100.0;
            }
        }
        let targets: [usize; 3] = std::array::from_fn(|m| self.p.idx(mod_id(m, M_TARGET)));
        let depths: [f32; 3] = std::array::from_fn(|m| self.p.get(mod_id(m, M_DEPTH)) / 100.0);
        for i in 0..left.len().min(right.len()) {
            let beat = self.clock.tick();
            let mut off = xy_off;
            let mut gate_mix = 1.0f32;
            let mut gate_add: Option<f32> = None;
            let mut pan = 0.0f32;
            for m in 0..3 {
                let Some(v) = self.modulator(m, beat) else { continue };
                self.mod_value[m] = v;
                let d = depths[m];
                let vv = if d < 0.0 { 1.0 - v } else { v };
                match targets[m] {
                    1 => gate_mix *= 1.0 - d.abs() * (1.0 - vv),
                    2 => *gate_add.get_or_insert(0.0) += d.abs() * vv,
                    3 => pan += d * (v - 0.5) * 2.0,
                    t => off[t] += d * (v - 0.5),
                }
            }
            let gate = gate_mix * gate_add.map_or(1.0, |g| g.clamp(0.0, 1.0));
            self.gate.target = gate;
            self.pan.target = pan.clamp(-1.0, 1.0);
            let g = self.gate.next();
            let pn = self.pan.next();
            let val = |s: &Self, id: u32, t: usize| if off[t] == 0.0 { s.p.get(id) } else { s.moved(id, t, off[t]) };
            let input = db(val(self, INPUT, 20));
            let (dl, dr) = (left[i], right[i]);
            let (mut l, mut r) = (dl * input, dr * input);
            for proc_ in order {
                if !ons[proc_] {
                    continue;
                }
                match proc_ {
                    0 => {
                        let d = val(self, MOD_DELAY, 4) * 0.001 * rate;
                        let hz = val(self, MOD_RATE, 5);
                        let depth = val(self, MOD_DEPTH, 6) / 100.0;
                        let fb = val(self, MOD_FB, 7) / 100.0 * 0.9;
                        let mix = val(self, MOD_MIX, 8) / 100.0;
                        let ph = self.mod_ph.step(rate, hz);
                        for (s, x) in [&mut l, &mut r].into_iter().enumerate() {
                            let m = sine01(ph + 0.25 * s as f32);
                            self.mod_line[s].push(*x + self.mod_fb[s] * fb);
                            let y = self.mod_line[s].read_lin((d * (1.0 + 0.9 * depth * m)).max(1.0));
                            self.mod_fb[s] = flush(y);
                            *x = *x * (1.0 - mix) + y * mix;
                        }
                    }
                    1 => {
                        let fb = val(self, DLY_FB, 9) / 100.0 * 0.95;
                        let cut = val(self, DLY_CUT, 10);
                        let mix = val(self, DLY_MIX, 11) / 100.0;
                        let (tl, tr) = if self.p.on(DLY_SYNC) {
                            (NOTE_BEATS[self.p.idx(DLY_DL)] as f64 * spb, NOTE_BEATS[self.p.idx(DLY_DR)] as f64 * spb)
                        } else {
                            (self.p.get(DLY_TL) as f64 * 0.001 * rate as f64, self.p.get(DLY_TR) as f64 * 0.001 * rate as f64)
                        };
                        for (s, x) in [&mut l, &mut r].into_iter().enumerate() {
                            let t = ([tl, tr][s] as f32).clamp(1.0, 4.0 * rate);
                            let y = self.dly[s].read(t);
                            self.dly_f[s].set(rate, cut, 0.7);
                            let o = self.dly_f[s].run(y);
                            let yf = match dftype {
                                0 => o.low,
                                1 => o.band,
                                2 => o.high,
                                _ => self.dly_lc[s].run(o.low),
                            };
                            self.dly[s].push(*x + yf * fb);
                            *x = *x * (1.0 - 0.5 * mix) + yf * mix;
                        }
                    }
                    2 => {
                        let (c, rs, dr_) = (val(self, FLT_CUT, 12), val(self, FLT_RES, 13), val(self, FLT_DRIVE, 14));
                        let mix = val(self, FLT_MIX, 15) / 100.0;
                        for (s, x) in [&mut l, &mut r].into_iter().enumerate() {
                            let y = self.filt[s].run(rate, ftype_i, c, rs, dr_, *x);
                            *x = *x * (1.0 - mix) + y * mix;
                        }
                    }
                    3 => {
                        let exc = val(self, DST_EXC, 16) / 100.0;
                        let dirt = val(self, DST_DIRT, 17) / 100.0;
                        for (s, x) in [&mut l, &mut r].into_iter().enumerate() {
                            let h = self.exc_hp[s].run(*x);
                            let e = (h * 4.0).tanh() * exc * 0.4;
                            let k = 1.0 + 15.0 * dirt;
                            *x = ((*x + e) * k).tanh() / k.sqrt().max(1.0);
                        }
                    }
                    _ => {
                        let mix = val(self, REV_MIX, 19) / 100.0;
                        let (a, b) = self.verb.run(l, r);
                        l = l * (1.0 - 0.5 * mix) + a * mix * 0.6;
                        r = r * (1.0 - 0.5 * mix) + b * mix * 0.6;
                    }
                }
            }
            // gate and pan (the step modulators), then Mix and Output
            l *= g;
            r *= g;
            if pn != 0.0 {
                l *= (1.0 - pn).min(1.0);
                r *= (1.0 + pn).min(1.0);
            }
            let mix = val(self, MIX, 21) / 100.0;
            let out = db(val(self, OUTPUT, 22));
            left[i] = (dl * (1.0 - mix) + l * mix) * out;
            right[i] = (dr * (1.0 - mix) + r * mix) * out;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.clock.set_tempo(bpm, self.rate);
    }
    fn set_transport(&mut self, t: Transport) {
        self.clock.sync(t, self.rate);
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Step 1", self.step_now[0] as f32), ("Step 2", self.step_now[1] as f32), ("Step 3", self.step_now[2] as f32)]
    }
}

impl Plugin for StepFx {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        // the order: putting a processor in a slot swaps it with the slot that had it
        if (ORDER..ORDER + 5).contains(&id) {
            let new = value.round().clamp(0.0, 4.0);
            if let Some(other) = (0..5).map(|k| ORDER + k).find(|k| *k != id && self.p.get(*k) == new) {
                let old = self.p.get(id);
                self.p.set(other, old);
            }
        }
        if self.p.set(id, value) && (id < 3000) {
            self.update();
        }
    }
    fn reset(&mut self) {
        for f in self.filt.iter_mut() {
            f.clear();
        }
        for d in self.mod_line.iter_mut().chain(self.dly.iter_mut()) {
            d.clear();
        }
        self.verb.clear();
    }
    fn tail(&self) -> f32 {
        if self.p.on(REV_ON) { self.p.get(REV_TIME) + 1.0 } else if self.p.on(DLY_ON) { 4.0 } else { 0.1 }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fx: &mut StepFx, x: &[f32]) -> Vec<f32> {
        let mut l = x.to_vec();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b);
        }
        l
    }

    #[test]
    fn default_is_transparent() {
        let mut fx = StepFx::new(48000.0);
        let x: Vec<f32> = (0..24000).map(|i| (i as f32 * 0.03).sin()).collect();
        let y = run(&mut fx, &x);
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-4));
    }

    #[test]
    fn gate_follows_the_steps() {
        let mut fx = StepFx::new(48000.0);
        fx.set_tempo(120.0);
        // 1/16 steps (6000 samples at 120): every other step off
        for s in (1..16).step_by(2) {
            fx.set(step_id(0, s, 2), 0.0);
        }
        let y = run(&mut fx, &vec![1.0; 48000]);
        assert!(y[3000] > 0.99 && y[9000] < 0.01, "{} {}", y[3000], y[9000]);
    }

    #[test]
    fn half_length_steps_gate_half() {
        let mut fx = StepFx::new(48000.0);
        fx.set_tempo(120.0);
        for s in 0..16 {
            fx.set(step_id(0, s, 1), 0.5);
        }
        let y = run(&mut fx, &vec![1.0; 12000]);
        assert!(y[1500] > 0.99 && y[4500] < 0.01);
    }

    #[test]
    fn tie_holds_over_the_next_step() {
        let mut fx = StepFx::new(48000.0);
        fx.set_tempo(120.0);
        fx.set(step_id(0, 0, 1), 2.0);
        fx.set(step_id(0, 1, 0), 0.0); // the tied step's own level is replaced by the first's
        let y = run(&mut fx, &vec![1.0; 12000]);
        assert!(y[9000] > 0.99, "{}", y[9000]);
    }

    #[test]
    fn gate_mix_depth_zero_changes_nothing_and_gate_add_zero_is_silence() {
        let mut fx = StepFx::new(48000.0);
        for s in 0..16 {
            fx.set(step_id(0, s, 0), 0.2);
        }
        fx.set(mod_id(0, M_DEPTH), 0.0);
        let y = run(&mut fx, &vec![1.0; 4800]);
        assert!(y[2400] > 0.99);
        fx.set(mod_id(0, M_TARGET), 2.0); // Gate Add
        let y = run(&mut fx, &vec![1.0; 4800]);
        assert!(y[2400] < 0.01);
    }

    #[test]
    fn actions() {
        let mut fx = StepFx::new(48000.0);
        fx.set(step_id(0, 0, 0), 0.3);
        fx.set(mod_id(0, M_ACTION), 3.0); // Shift Left: step 1 wraps to 16
        assert!((fx.get(step_id(0, 15, 0)) - 0.3).abs() < 1e-6);
        fx.set(mod_id(0, M_ACTION), 7.0); // Invert
        assert!((fx.get(step_id(0, 15, 0)) - 0.7).abs() < 1e-6);
        fx.set(mod_id(0, M_ACTION), 9.0); // Append Reverse Duplicate
        assert_eq!(fx.steps(0), 32);
        assert!((fx.get(step_id(0, 16, 0)) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn order_stays_a_permutation() {
        let mut fx = StepFx::new(48000.0);
        fx.set(ORDER, 4.0);
        let mut o: Vec<usize> = (0..5).map(|k| fx.get(ORDER + k).round() as usize).collect();
        o.sort();
        assert_eq!(o, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn filter_modulated_by_steps() {
        let mut fx = StepFx::new(48000.0);
        fx.set(FLT_ON, 1.0);
        fx.set(FLT_TYPE, 1.0); // LP 24 smooth
        fx.set(FLT_CUT, 0.3);
        fx.set(FLT_RES, 0.0);
        fx.set(mod_id(0, M_ON), 0.0);
        let x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 6000.0 * i as f32 / 48000.0).sin()).collect();
        let y = run(&mut fx, &x);
        let p = y[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(p < 0.01, "{p}");
        let _ = RATES;
    }
}
