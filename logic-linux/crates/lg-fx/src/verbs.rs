//! EnVerb, SilverVerb and Quantec Room Simulator, with the guide's controls.
//!
//! - EnVerb: a reverb whose tail follows an envelope you draw (Attack, Decay, Sustain, Hold,
//!   Release), with Dry Signal Delay, Predelay, Density, Spread, High Cut, Crossover and Low Freq
//!   Level: built as a synthesised impulse response with that envelope, convolved (zero latency).
//! - SilverVerb: Predelay, Reflectivity, Size, Density/Time, Low and High Cut, a modulation LFO
//!   (Rate, Phase, Intensity), Dry / Wet — a feedback delay network.
//! - Quantec Room Simulator: the QRS and YardStick models; Reverb Time with Room Size (1 m³ to
//!   1,000,000 m³), Low / High time multipliers (YardStick: with their crossovers), Reverb Delay,
//!   Dry and Reverb levels, Freeze (Add, Clear), Enhance (QRS), the AD/DA converter drive (QRS),
//!   the YardStick's Mode (Complex / Medium / Simple), Density, High Cut, Bass Gain and Crossover,
//!   Correlation, Dry Source (L R / R L) and Subsonic.
//!
//! Ids are our own from 1000 (the blocks aren't decoded).

use crate::convolve::Convolver;
use crate::kit::{Crossover, Fdn};
use crate::util::{Biquad, Coefs, Delay, Noise, OnePole, Phase, Values, db, plugin_impl, sine01};
use lg_dsp::{Effect, Param, Unit};

const B: u32 = 1000;

// ---------------------------------------------------------------------------------------------
// EnVerb

static EV_P: [Param; 14] = [
    Param::new(B, "Attack", 0.0, 2000.0, 20.0, Unit::Ms),
    Param::new(B + 1, "Decay", 0.0, 4000.0, 300.0, Unit::Ms),
    Param::new(B + 2, "Sustain", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 3, "Hold", 0.0, 4000.0, 300.0, Unit::Ms),
    Param::new(B + 4, "Release", 10.0, 4000.0, 800.0, Unit::Ms),
    Param::new(B + 5, "Dry Signal Delay", 0.0, 200.0, 0.0, Unit::Ms),
    Param::new(B + 6, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(B + 7, "Density", 0.0, 100.0, 80.0, Unit::Percent),
    Param::new(B + 8, "Spread", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(B + 9, "High Cut", 500.0, 20000.0, 9000.0, Unit::Hz),
    Param::new(B + 10, "Crossover", 50.0, 2000.0, 300.0, Unit::Hz),
    Param::new(B + 11, "Low Freq Level", -24.0, 6.0, -3.0, Unit::Db),
    Param::new(B + 12, "Dry", -96.0, 6.0, 0.0, Unit::Db),
    Param::new(B + 13, "Wet", -96.0, 6.0, -10.0, Unit::Db),
];

const EV_MAX: f32 = 6.0;

pub struct EnVerb {
    p: Values,
    rate: f32,
    conv: [Convolver; 2],
    ir: [Vec<f32>; 2],
    n: usize,
    dry_line: [Delay; 2],
}

impl EnVerb {
    pub fn new(rate: f32) -> EnVerb {
        let max = (EV_MAX * rate) as usize;
        let mut e = EnVerb { p: Values::new(&EV_P), rate, conv: [Convolver::new(max), Convolver::new(max)], ir: [vec![0.0; max], vec![0.0; max]], n: 0, dry_line: [Delay::new((0.21 * rate) as usize), Delay::new((0.21 * rate) as usize)] };
        e.update();
        e
    }

    /// The envelope's level at time t (s): Attack up, Decay to Sustain, Hold, Release to zero
    /// (after the Predelay).
    pub fn envelope(&self, t: f32) -> f32 {
        let g = |id| self.p.get(id) * 0.001;
        let (a, d, s, h, r) = (g(B), g(B + 1), self.p.get(B + 2) / 100.0, g(B + 3), g(B + 4));
        if t < a {
            t / a.max(1e-4)
        } else if t < a + d {
            1.0 - (1.0 - s) * (t - a) / d.max(1e-4)
        } else if t < a + d + h {
            s
        } else if t < a + d + h + r {
            s * (1.0 - (t - a - d - h) / r)
        } else {
            0.0
        }
    }

    fn update(&mut self) {
        let rate = self.rate;
        let total = (self.p.get(B) + self.p.get(B + 1) + self.p.get(B + 3) + self.p.get(B + 4)) * 0.001;
        let pre = (self.p.get(B + 6) * 0.001 * rate) as usize;
        let n = ((total * rate) as usize + pre).min(self.ir[0].len());
        self.n = n;
        let dens = self.p.get(B + 7) / 100.0;
        let spread = (self.p.get(B + 8) / 100.0).min(2.0);
        let mut nz = [Noise(0x1357_9bdf), Noise(0x2468_ace1), Noise(0x0f1e_2d3c)];
        let mut hc = [OnePole::new(rate, self.p.get(B + 9)); 2];
        let mut xo = [Crossover::new(rate, self.p.get(B + 10)); 2];
        let low = db(self.p.get(B + 11));
        let sparse = (dens * dens).max(0.01);
        for i in 0..n {
            let t = i.saturating_sub(pre) as f32 / rate;
            let env = if i < pre { 0.0 } else { self.envelope(t) };
            let keep = (nz[2].next() * 0.5 + 0.5) < sparse;
            let common = nz[2].next();
            for c in 0..2 {
                let own = nz[c].next();
                // Spread: 0 = mono, 1 = independent, 2 = anti-correlated widening
                let x = if keep { own * spread.min(1.0) + common * (1.0 - spread.min(1.0)) - if spread > 1.0 { (spread - 1.0) * common * if c == 0 { 1.0 } else { -1.0 } } else { 0.0 } } else { 0.0 } / sparse.sqrt();
                let x = hc[c].lp(x * env);
                let (l, h) = xo[c].run(x);
                self.ir[c][i] = l * low + h;
            }
        }
        for c in 0..2 {
            self.ir[c][n..].iter_mut().for_each(|v| *v = 0.0);
        }
        let e: f64 = self.ir.iter().map(|ch| ch[..n].iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum::<f64>() / 2.0;
        let g = (1.2 / e.max(1e-20).sqrt()) as f32;
        for c in 0..2 {
            self.ir[c][..n].iter_mut().for_each(|v| *v *= g);
            self.conv[c].load(&self.ir[c][..n.max(1)]);
        }
    }
    fn clear(&mut self) {
        for c in self.conv.iter_mut() {
            c.clear();
        }
        self.dry_line[0].clear();
        self.dry_line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        self.n as f32 / self.rate
    }
}

impl Effect for EnVerb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dd = self.p.get(B + 5) * 0.001 * self.rate;
        let dry = if self.p.get(B + 12) <= -95.9 { 0.0 } else { db(self.p.get(B + 12)) };
        let wet = if self.p.get(B + 13) <= -95.9 { 0.0 } else { db(self.p.get(B + 13)) };
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let w = self.conv[s].run(*x);
                self.dry_line[s].push(*x);
                let d = if dd >= 1.0 { self.dry_line[s].read_lin(dd) } else { *x };
                *x = d * dry + w * wet;
            }
        }
    }
}
plugin_impl!(EnVerb);

// ---------------------------------------------------------------------------------------------
// SilverVerb

static SV_P: [Param; 11] = [
    Param::new(B, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(B + 1, "Reflectivity", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(B + 2, "Room Size", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 3, "Density/Time", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Low Cut", 20.0, 2000.0, 100.0, Unit::Hz),
    Param::new(B + 5, "High Cut", 500.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(B + 6, "Modulation", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 7, "Rate", 0.01, 5.0, 0.4, Unit::Hz),
    Param::new(B + 8, "Phase", 0.0, 360.0, 90.0, Unit::None),
    Param::new(B + 9, "Intensity", 0.0, 100.0, 20.0, Unit::Percent),
    Param::new(B + 10, "Dry/Wet", 0.0, 100.0, 30.0, Unit::Percent),
];

pub struct SilverVerb {
    p: Values,
    rate: f32,
    fdn: Fdn,
    lc: [Biquad; 2],
    hc: [Biquad; 2],
    lfo: Phase,
    mod_line: [Delay; 2],
}

impl SilverVerb {
    pub fn new(rate: f32) -> SilverVerb {
        let mut s = SilverVerb { p: Values::new(&SV_P), rate, fdn: Fdn::new(rate), lc: [Biquad::default(); 2], hc: [Biquad::default(); 2], lfo: Phase::default(), mod_line: [Delay::new((0.02 * rate) as usize), Delay::new((0.02 * rate) as usize)] };
        s.update();
        s
    }
    fn update(&mut self) {
        let size = 0.4 + 1.4 * self.p.get(B + 2) / 100.0;
        let time = 0.3 + 6.0 * (self.p.get(B + 3) / 100.0).powi(2) * (0.4 + 0.6 * self.p.get(B + 1) / 100.0);
        let damp = 2000.0 + 14000.0 * self.p.get(B + 1) / 100.0;
        self.fdn.set(time, size, damp);
        self.fdn.predelay = self.p.get(B) * 0.001;
        for s in 0..2 {
            self.lc[s].c = Coefs::highpass(self.rate, self.p.get(B + 4), 0.7);
            self.hc[s].c = Coefs::lowpass(self.rate, self.p.get(B + 5).min(self.rate * 0.45), 0.7);
        }
    }
    fn clear(&mut self) {
        self.fdn.clear();
    }
    fn tail_secs(&self) -> f32 {
        6.0
    }
}

impl Effect for SilverVerb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mix = self.p.get(B + 10) / 100.0;
        let on = self.p.on(B + 6);
        let ph_off = self.p.get(B + 8) / 360.0;
        let int = self.p.get(B + 9) / 100.0;
        for i in 0..left.len().min(right.len()) {
            let (mut a, mut b) = self.fdn.run(left[i], right[i]);
            a = self.hc[0].run(self.lc[0].run(a));
            b = self.hc[1].run(self.lc[1].run(b));
            if on && int > 0.0 {
                let ph = self.lfo.step(self.rate, self.p.get(B + 7));
                for (s, v) in [&mut a, &mut b].into_iter().enumerate() {
                    let m = sine01(ph + ph_off * s as f32);
                    self.mod_line[s].push(*v);
                    *v = self.mod_line[s].read_lin(1.0 + 0.004 * self.rate * int * (0.5 + 0.5 * m));
                }
            }
            left[i] = left[i] * (1.0 - mix) + a * mix * 0.5;
            right[i] = right[i] * (1.0 - mix) + b * mix * 0.5;
        }
    }
}
plugin_impl!(SilverVerb);

// ---------------------------------------------------------------------------------------------
// Quantec Room Simulator
//
// One engine for QRS and YardStick, stereo and surround. The signal: per input channel the
// subsonic filter (and in surround the Room Input processing: Pre Level, Pre Delay, Pre High
// Cut, routed to one of eight virtual speakers), the AD/DA stage (QRS), three FDN tails for the
// bands below the low crossover, between, and above the high crossover (their times the Reverb
// Time times the Low / 1 / High multipliers; QRS crossovers fixed at 500 Hz and 1 kHz), or with
// Enhance a dense cluster of reflections sized by the room. The tail is picked up by virtual
// microphones (correlation patterns: each keeps 50 % correlation with the others; the same
// number twice gives the same signal, Off mutes that output), widened or narrowed by Reverb
// Correlation, delayed by Reverb Delay (+ the channel's offset), high-cut (YardStick and the
// channel's offset), and mixed per output channel with the first reflection (its source
// channel, delay, spread, level, phase) and the dry signal (its source channel). Macros move the
// front and rear (and top) channels' offsets against each other.

/// The per-channel block (surround details view) for channel slot `s`: base id.
pub fn qch(s: usize) -> u32 {
    B + 100 + 16 * s as u32
}
pub mod qrs {
    use super::B;
    pub const MODEL: u32 = B;
    pub const TIME: u32 = B + 1;
    pub const FREEZE: u32 = B + 2;
    pub const ADD: u32 = B + 3;
    pub const CLEAR: u32 = B + 4;
    pub const ENHANCE: u32 = B + 5;
    pub const LOW: u32 = B + 6;
    pub const HIGH: u32 = B + 7;
    pub const SIZE: u32 = B + 8;
    pub const REV_DELAY: u32 = B + 9;
    pub const DRY: u32 = B + 10;
    pub const REV_LEVEL: u32 = B + 11;
    pub const ADDA: u32 = B + 12;
    pub const DRIVE: u32 = B + 13;
    pub const MODE: u32 = B + 14;
    pub const LOW_XO: u32 = B + 15;
    pub const HIGH_XO: u32 = B + 16;
    pub const DENSITY: u32 = B + 17;
    pub const HIGH_CUT: u32 = B + 18;
    pub const BASS: u32 = B + 19;
    pub const BASS_XO: u32 = B + 20;
    pub const CORR: u32 = B + 21;
    pub const DRY_SRC: u32 = B + 22;
    pub const SUBSONIC: u32 = B + 23;
    pub const REFL_DELAY: u32 = B + 24;
    pub const REFL_SPREAD: u32 = B + 25;
    pub const REFL_LEVEL: u32 = B + 26;
    pub const REFL_SRC: u32 = B + 27;
    pub const REFL_PHASE: u32 = B + 28;
    pub const PATTERN_L: u32 = B + 29;
    pub const PATTERN_R: u32 = B + 30;
    pub const PAGE: u32 = B + 31;
    pub const DETAILS: u32 = B + 32;
    pub const INFO: u32 = B + 33;
    /// Surround macros: 1st Reflection Delay, Reverb Delay, Reverb High Cut, Dry Level, 1st
    /// Reflection Level (front / rear), Reverb Level (rear / front, bottom / top).
    pub const MACROS: [u32; 7] = [B + 40, B + 41, B + 42, B + 43, B + 44, B + 45, B + 46];
    /// Per-channel offsets in the details block (after the channel's base id).
    pub const PATTERN: u32 = 0;
    pub const ROOM_INPUT: u32 = 1;
    pub const PRE_LEVEL: u32 = 2;
    pub const PRE_DELAY: u32 = 3;
    pub const PRE_HIGH_CUT: u32 = 4;
    pub const DRY_SOURCE: u32 = 5;
    pub const REFL_SOURCE: u32 = 6;
    /// The seven offsets: 1st Refl. Delay, 1st Refl. Spread, Reverb Delay, Reverb High Cut, Dry
    /// Level, 1st Refl. Level, Reverb Level.
    pub const OFFSETS: [u32; 7] = [7, 8, 9, 10, 11, 12, 13];
}
pub use qrs::*;

const PATTERNS: &[&str] = &["Off", "1", "2", "3", "4", "5", "6", "7", "8"];
const SPEAKERS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8"];
/// "Mute" sits below this many dB.
const MUTE_DB: f32 = -30.5;

fn ch_labels() -> &'static [&'static str] {
    static L: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    L.get_or_init(|| lg_dsp::surround::Ch::ALL.iter().map(|c| c.label()).collect())
}

pub fn qrs_params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let mut v = vec![
            Param::menu(MODEL, "Model", &["QRS", "YardStick"], 1.0),
            Param::new(TIME, "Reverb Time", 0.1, 100.0, 2.0, Unit::Seconds),
            Param::new(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(ADD, "Add", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(CLEAR, "Clear", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(ENHANCE, "Enhance", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(LOW, "Low", 0.1, 10.0, 1.0, Unit::None),
            Param::new(HIGH, "High", 0.1, 10.0, 0.6, Unit::None),
            Param::new(SIZE, "Room Size", 1.0, 1_000_000.0, 2000.0, Unit::None),
            Param::new(REV_DELAY, "Reverb Delay", 0.0, 200.0, 20.0, Unit::Ms),
            Param::new(DRY, "Dry Level", -96.0, 6.0, 0.0, Unit::Db),
            Param::new(REV_LEVEL, "Reverb Level", -96.0, 6.0, -8.0, Unit::Db),
            Param::new(ADDA, "AD/DA Converters", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(DRIVE, "AD/DA Drive", 0.0, 30.0, 0.0, Unit::Db),
            Param::menu(MODE, "Mode", &["Complex", "Medium", "Simple"], 0.0),
            Param::new(LOW_XO, "Low Xover", 50.0, 1000.0, 300.0, Unit::Hz),
            Param::new(HIGH_XO, "High Xover", 1000.0, 16000.0, 5000.0, Unit::Hz),
            Param::new(DENSITY, "Reverb Density", 0.0, 200.0, 100.0, Unit::Percent),
            Param::new(HIGH_CUT, "Reverb High Cut", 1000.0, 20000.0, 16000.0, Unit::Hz),
            Param::new(BASS, "Bass Gain", -12.0, 12.0, 0.0, Unit::Db),
            Param::new(BASS_XO, "Bass Crossover", 50.0, 1000.0, 200.0, Unit::Hz),
            Param::new(CORR, "Reverb Correlation", -100.0, 100.0, 0.0, Unit::Percent),
            Param::menu(DRY_SRC, "Dry Source", &["L R", "R L"], 0.0),
            Param::new(SUBSONIC, "Subsonic", 0.0, 1.0, 1.0, Unit::OnOff),
            Param::new(REFL_DELAY, "1st Reflection Delay", 0.0, 200.0, 12.0, Unit::Ms),
            Param::new(REFL_SPREAD, "1st Reflection Spread", -50.0, 50.0, 4.0, Unit::Ms),
            Param::new(REFL_LEVEL, "1st Reflection Level", -96.0, 6.0, -12.0, Unit::Db),
            Param::menu(REFL_SRC, "1st Reflection Source", &["L R", "R L"], 0.0),
            Param::new(REFL_PHASE, "1st Phase Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(PATTERN_L, "L Correlation Pattern", PATTERNS, 1.0),
            Param::menu(PATTERN_R, "R Correlation Pattern", PATTERNS, 2.0),
            Param::menu(PAGE, "Page", &["Primary", "Secondary", "Extended"], 0.0).hide(),
            Param::new(DETAILS, "Details", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
            Param::new(INFO, "Info", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
        ];
        for (id, n) in MACROS.iter().zip(["Macro 1st Reflection Delay", "Macro Reverb Delay", "Macro Reverb High Cut", "Macro Dry Level", "Macro 1st Reflection Level", "Macro Reverb Level R/F", "Macro Reverb Level B/T"]) {
            v.push(Param::new(*id, n, -100.0, 100.0, 0.0, Unit::Percent));
        }
        for (s, c) in lg_dsp::surround::Ch::ALL.iter().enumerate() {
            let b = qch(s);
            let l = c.label();
            let pat = if *c == lg_dsp::surround::Ch::Lfe { 0.0 } else { (s % 8 + 1) as f32 };
            v.push(Param::menu(b + PATTERN, leak(format!("{l} Correlation Pattern")), PATTERNS, pat).hide());
            v.push(Param::menu(b + ROOM_INPUT, leak(format!("{l} Room Input")), SPEAKERS, (s % 8) as f32).hide());
            v.push(Param::new(b + PRE_LEVEL, leak(format!("{l} Pre Level")), -31.0, 0.0, 0.0, Unit::Db).hide());
            v.push(Param::new(b + PRE_DELAY, leak(format!("{l} Pre Delay")), 0.0, 250.0, 0.0, Unit::Ms).hide());
            v.push(Param::new(b + PRE_HIGH_CUT, leak(format!("{l} Pre High Cut")), 500.0, 20001.0, 20001.0, Unit::Hz).hide());
            v.push(Param::menu(b + DRY_SOURCE, leak(format!("{l} Dry Source")), ch_labels(), s as f32).hide());
            v.push(Param::menu(b + REFL_SOURCE, leak(format!("{l} 1st Refl. Source")), ch_labels(), s as f32).hide());
            for (k, n) in ["1st Refl. Delay", "1st Refl. Spread", "Reverb Delay", "Reverb High Cut", "Dry Level", "1st Refl. Level", "Reverb Level"].iter().enumerate() {
                let (lo, hi, unit) = match k {
                    0 | 2 => (-50.0, 50.0, Unit::Ms),
                    1 => (-20.0, 20.0, Unit::Ms),
                    3 => (-3.0, 3.0, Unit::Octaves),
                    _ => (-31.0, 12.0, Unit::Db),
                };
                v.push(Param::new(b + OFFSETS[k], leak(format!("{l} {n} Offset")), lo, hi, 0.0, unit).hide());
            }
        }
        v
    })
}

/// A decorrelating all-pass chain (one per virtual microphone).
#[derive(Clone)]
struct Mic {
    ap: [Delay; 3],
    len: [usize; 3],
}

impl Mic {
    fn new(rate: f32, k: usize) -> Mic {
        let base = [0.0031, 0.0047, 0.0071];
        let len = base.map(|b| ((b * (1.0 + 0.37 * k as f32)) * rate) as usize + 7 * k + 3);
        Mic { ap: len.map(|n| Delay::new(n + 4)), len }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let mut y = x;
        for i in 0..3 {
            let d = self.ap[i].tap(self.len[i] - 1);
            let v = y + 0.6 * d;
            self.ap[i].push(v);
            y = d - 0.6 * v;
        }
        y
    }
    fn clear(&mut self) {
        self.ap.iter_mut().for_each(|d| d.clear());
    }
}

/// One output channel's settings, worked out on update.
#[derive(Clone, Copy, Debug, Default)]
struct OutCfg {
    pattern: usize,
    dry_src: usize,
    refl_src: usize,
    refl_delay: f32,
    rev_delay: f32,
    high_cut: f32,
    dry: f32,
    refl: f32,
    rev: f32,
    // the room input side: level, delay (samples), high cut, virtual speaker (0..7)
    pre_level: f32,
    pre_delay: f32,
    pre_hc: f32,
    speaker: usize,
}

pub struct Quantec {
    p: Values,
    rate: f32,
    bands: [Fdn; 3],
    xo: [Crossover; 2],
    xo_hi: [Crossover; 2],
    hc: Vec<Biquad>,
    bass: [Biquad; 2],
    sub: Vec<Biquad>,
    pre_hc: Vec<Biquad>,
    pre: Vec<Delay>,
    /// Per output channel: the tail and the dry/first-reflection sources' delay lines.
    rev_line: Vec<Delay>,
    in_line: Vec<Delay>,
    mics: Vec<Mic>,
    cluster: [Delay; 2],
    taps: Vec<(usize, f32, usize)>,
    frozen: bool,
    layout: Vec<lg_dsp::surround::Ch>,
    cfg: Vec<OutCfg>,
    pub meter_in: Vec<f32>,
    pub meter_out: Vec<f32>,
    fall: f32,
}

impl Quantec {
    pub fn new(rate: f32) -> Quantec {
        let mut q = Quantec {
            p: Values::new(qrs_params()),
            rate,
            bands: [Fdn::new(rate), Fdn::new(rate), Fdn::new(rate)],
            xo: [Crossover::new(rate, 300.0); 2],
            xo_hi: [Crossover::new(rate, 5000.0); 2],
            hc: Vec::new(),
            bass: [Biquad::default(); 2],
            sub: Vec::new(),
            pre_hc: Vec::new(),
            pre: Vec::new(),
            rev_line: Vec::new(),
            in_line: Vec::new(),
            mics: (0..8).map(|k| Mic::new(rate, k)).collect(),
            cluster: [Delay::new((0.25 * rate) as usize), Delay::new((0.25 * rate) as usize)],
            taps: Vec::new(),
            frozen: false,
            layout: Vec::new(),
            cfg: Vec::new(),
            meter_in: Vec::new(),
            meter_out: Vec::new(),
            fall: db(-20.0 / rate),
        };
        q.configure(&[lg_dsp::surround::Ch::L, lg_dsp::surround::Ch::R]);
        q
    }
    /// The reverb time range a room size allows (s): bigger rooms ring longer.
    pub fn time_range(size_m3: f32) -> (f32, f32) {
        let s = size_m3.max(1.0).log10() / 6.0;
        (0.1 + 0.9 * s, 2.0 + 98.0 * s * s)
    }
    /// How many correlation patterns the YardStick's Mode offers (QRS: all eight).
    pub fn patterns_available(&self) -> usize {
        if self.p.idx(MODEL) == 0 { 8 } else { [8, 4, 2][self.p.idx(MODE).min(2)] }
    }
    pub fn layout(&self) -> &[lg_dsp::surround::Ch] {
        &self.layout
    }
    /// The channels processed: stereo L R, or a surround bus's layout. A new surround format
    /// resets the details view's per-channel settings.
    pub fn configure(&mut self, layout: &[lg_dsp::surround::Ch]) {
        if self.layout == layout {
            return;
        }
        let was_surround = self.layout.len() > 2;
        self.layout = layout.to_vec();
        if was_surround || layout.len() > 2 {
            for d in self.p.defs.iter().filter(|d| d.id >= B + 100) {
                self.p.set(d.id, d.default);
            }
        }
        let n = layout.len();
        let r = self.rate;
        self.hc = vec![Biquad::default(); n];
        self.sub = vec![Biquad::default(); n];
        self.pre_hc = vec![Biquad::default(); n];
        self.pre = (0..n).map(|_| Delay::new((0.26 * r) as usize)).collect();
        self.rev_line = (0..n).map(|_| Delay::new((0.5 * r) as usize)).collect();
        self.in_line = (0..n).map(|_| Delay::new((0.45 * r) as usize)).collect();
        self.meter_in = vec![0.0; n];
        self.meter_out = vec![0.0; n];
        self.update();
    }
    /// Whether a channel is in front (+1), behind (-1) or at the sides (0), and on top.
    fn place(c: lg_dsp::surround::Ch) -> (f32, bool) {
        let (az, el) = lg_dsp::surround::position(lg_dsp::surround::Format::S714, c);
        let fr = if az.abs() < 80.0 { 1.0 } else if az.abs() > 100.0 { -1.0 } else { 0.0 };
        (fr, el > 0.0)
    }
    fn level(dbv: f32) -> f32 {
        if dbv <= MUTE_DB { 0.0 } else { db(dbv) }
    }
    fn update(&mut self) {
        if self.p.on(CLEAR) {
            self.p.set(CLEAR, 0.0);
            self.bands.iter_mut().for_each(|f| f.clear());
            self.cluster.iter_mut().for_each(|d| d.clear());
            self.mics.iter_mut().for_each(|m| m.clear());
            self.rev_line.iter_mut().chain(self.pre.iter_mut()).for_each(|d| d.clear());
        }
        let (lo, hi) = Quantec::time_range(self.p.get(SIZE));
        let t = self.p.get(TIME).clamp(lo, hi);
        let size = (0.4 + 0.27 * self.p.get(SIZE).max(1.0).log10()).min(2.0);
        let (ml, mh) = (self.p.get(LOW), self.p.get(HIGH));
        let yard = self.p.idx(MODEL) == 1;
        self.frozen = self.p.on(FREEZE);
        let f = |x: f32| if self.frozen { 1e6 } else { x };
        self.bands[0].set(f(t * ml), size, 9000.0);
        self.bands[1].set(f(t), size, 12000.0);
        self.bands[2].set(f(t * mh), size, 16000.0);
        let m = if yard { [0.8, 0.4, 0.0][self.p.idx(MODE).min(2)] } else { 0.3 };
        for b in self.bands.iter_mut() {
            b.predelay = 0.0;
            b.modulation = m;
        }
        let r = self.rate;
        // QRS: the multipliers' bands are fixed (below 500 Hz, above 1 kHz)
        let (xl, xh) = if yard { (self.p.get(LOW_XO), self.p.get(HIGH_XO).max(self.p.get(LOW_XO) * 1.5)) } else { (500.0, 1000.0) };
        for s in 0..2 {
            self.xo[s].set(r, xl);
            self.xo_hi[s].set(r, xh);
            self.bass[s].c = if yard && self.p.get(BASS).abs() > 0.01 { Coefs::low_shelf(r, self.p.get(BASS_XO), 0.7, self.p.get(BASS)) } else { Coefs::THRU };
        }
        // Enhance: a dense cluster of reflections over a window the room size sets
        let win = (0.004 + 0.012 * self.p.get(SIZE).max(1.0).log10()).min(0.2);
        let mut seed = 0x2545F491u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32) / u32::MAX as f32
        };
        self.taps = (0..32).map(|k| {
            let d = ((0.0007 + win * (k as f32 + rnd()) / 32.0) * r) as usize;
            let g = (1.0 - k as f32 / 40.0) * if rnd() > 0.5 { 0.22 } else { -0.22 };
            (d.max(1), g, (rnd() * 2.0) as usize)
        }).collect();
        // per output channel
        let surround = self.layout.len() > 2;
        let n = self.layout.len();
        let mut cfg = vec![OutCfg::default(); n];
        let mac = |k: usize| self.p.get(MACROS[k]) / 100.0;
        let base_hc = if yard { self.p.get(HIGH_CUT) } else { 20000.0 };
        let layout = self.layout.clone();
        for (k, c) in layout.iter().enumerate() {
            let s = lg_dsp::surround::Ch::ALL.iter().position(|x| x == c).unwrap_or(0);
            let b = qch(s);
            let (fr, top) = Quantec::place(*c);
            let off = |i: usize| if surround { self.p.get(b + OFFSETS[i]) } else { 0.0 };
            let o = &mut cfg[k];
            // stereo: L / R patterns and the L R / R L sources
            if surround {
                o.pattern = self.p.idx(b + PATTERN);
                o.dry_src = self.layout.iter().position(|x| *x == lg_dsp::surround::Ch::ALL[self.p.idx(b + DRY_SOURCE).min(16)]).unwrap_or(k);
                o.refl_src = self.layout.iter().position(|x| *x == lg_dsp::surround::Ch::ALL[self.p.idx(b + REFL_SOURCE).min(16)]).unwrap_or(k);
                o.pre_level = Quantec::level(self.p.get(b + PRE_LEVEL));
                o.pre_delay = self.p.get(b + PRE_DELAY) * 0.001 * r;
                o.pre_hc = self.p.get(b + PRE_HIGH_CUT);
                o.speaker = self.p.idx(b + ROOM_INPUT).min(7);
            } else {
                o.pattern = self.p.idx(if k == 0 { PATTERN_L } else { PATTERN_R });
                o.dry_src = if self.p.idx(DRY_SRC) == 1 { 1 - k.min(1) } else { k };
                o.refl_src = if self.p.idx(REFL_SRC) == 1 { 1 - k.min(1) } else { k };
                o.pre_level = 1.0;
                o.pre_delay = 0.0;
                o.pre_hc = 20001.0;
                o.speaker = k.min(1);
            }
            let avail = self.patterns_available();
            if o.pattern > avail {
                o.pattern = (o.pattern - 1) % avail + 1;
            }
            // the first reflection: delay (+ offset, + macro front/rear), spread for pairs
            let side = match c {
                lg_dsp::surround::Ch::C | lg_dsp::surround::Ch::Lfe | lg_dsp::surround::Ch::S => 0.0,
                _ => if lg_dsp::surround::position(lg_dsp::surround::Format::S714, *c).0 < 0.0 { -0.5 } else { 0.5 },
            };
            let spread = self.p.get(REFL_SPREAD) + off(1);
            o.refl_delay = ((self.p.get(REFL_DELAY) + off(0) + 25.0 * mac(0) * fr + side * spread) * 0.001 * r).max(0.0);
            o.rev_delay = ((self.p.get(REV_DELAY) + off(2) + 25.0 * mac(1) * fr) * 0.001 * r).max(0.0);
            o.high_cut = (base_hc * 2f32.powf(off(3) + 2.0 * mac(2) * fr)).clamp(200.0, 20000.0);
            let lvl = |base: f32, offset: f32, m: f32| if offset <= MUTE_DB { 0.0 } else { Quantec::level(base + offset + m) };
            o.dry = lvl(self.p.get(DRY), off(4), 6.0 * mac(3) * fr);
            o.refl = lvl(self.p.get(REFL_LEVEL), off(5), 6.0 * mac(4) * fr) * if !yard && self.p.on(REFL_PHASE) { -1.0 } else { 1.0 };
            // (Reverb Level R/F: rear against front; B/T: bottom against top)
            o.rev = lvl(self.p.get(REV_LEVEL), off(6), -6.0 * mac(5) * fr + if top { 6.0 } else { -6.0 } * mac(6));
        }
        let subsonic = self.p.on(SUBSONIC);
        for (k, o) in cfg.iter().enumerate() {
            self.hc[k].c = if o.high_cut < 19900.0 { Coefs::lowpass(r, o.high_cut.min(r * 0.45), 0.7) } else { Coefs::THRU };
            self.pre_hc[k].c = if o.pre_hc <= 20000.0 { Coefs::lowpass(r, o.pre_hc.min(r * 0.45), 0.7) } else { Coefs::THRU };
            self.sub[k].c = if subsonic { Coefs::highpass(r, 25.0, 0.7) } else { Coefs::THRU };
        }
        self.cfg = cfg;
    }
    fn clear(&mut self) {
        if !self.frozen {
            self.bands.iter_mut().for_each(|f| f.clear());
            self.cluster.iter_mut().for_each(|d| d.clear());
            self.mics.iter_mut().for_each(|m| m.clear());
            self.rev_line.iter_mut().chain(self.pre.iter_mut()).chain(self.in_line.iter_mut()).for_each(|d| d.clear());
        }
    }
    fn tail_secs(&self) -> f32 {
        (self.p.get(TIME) * self.p.get(LOW).max(self.p.get(HIGH)).max(1.0)).min(30.0)
    }

    /// One frame: `x` the inputs, `y` the outputs (same channels).
    fn frame(&mut self, x: &[f32], y: &mut [f32]) {
        let yard = self.p.idx(MODEL) == 1;
        let enhance = !yard && self.p.on(ENHANCE);
        let feed = if self.frozen { if self.p.on(ADD) { 0.3 } else { 0.0 } } else { 1.0 };
        let adda = !yard && self.p.on(ADDA);
        let drive = db(self.p.get(DRIVE));
        let width = 1.0 - self.p.get(CORR) / 100.0;
        let density = (self.p.get(DENSITY) / 100.0).clamp(0.2, 2.0).sqrt();
        let n = x.len();
        // into the room: virtual speakers 1–8, odd on the left, even on the right
        let (mut il, mut ir) = (0f32, 0f32);
        for k in 0..n {
            let mut v = self.sub[k].run(x[k]);
            self.meter_in[k] = (self.meter_in[k] * self.fall).max(v.abs());
            self.in_line[k].push(v);
            let cfg = self.cfg[k];
            if cfg.pre_delay >= 1.0 {
                self.pre[k].push(v);
                v = self.pre[k].read_lin(cfg.pre_delay);
            }
            v = self.pre_hc[k].run(v) * cfg.pre_level;
            if adda {
                v = (v * drive).tanh() / drive.max(1.0).sqrt();
            }
            let sp = cfg.speaker;
            // the speakers further round the room reach both sides a little
            let pan = (sp / 2) as f32 / 4.0;
            if sp % 2 == 0 {
                il += v * (1.0 - 0.3 * pan);
                ir += v * 0.3 * pan;
            } else {
                ir += v * (1.0 - 0.3 * pan);
                il += v * 0.3 * pan;
            }
        }
        let (a, b) = if enhance {
            self.cluster[0].push(il * feed);
            self.cluster[1].push(ir * feed);
            let mut o = [0f32; 2];
            for (d, g, side) in &self.taps {
                // (each tap from one side, heard on alternating sides: mono becomes wide)
                o[*side] += g * self.cluster[1 - *side].tap(*d);
            }
            (o[0], o[1])
        } else {
            let (ll, lr) = self.xo[0].run(il * feed);
            let (rl, rr) = self.xo[1].run(ir * feed);
            let (ml, hl) = self.xo_hi[0].run(lr);
            let (mr, hr) = self.xo_hi[1].run(rr);
            let (a0, b0) = self.bands[0].run(ll, rl);
            let (a1, b1) = self.bands[1].run(ml, mr);
            let (a2, b2) = self.bands[2].run(hl, hr);
            let (mut a, mut b) = (a0 + a1 + a2, b0 + b1 + b2);
            if yard {
                a = self.bass[0].run(a) * density;
                b = self.bass[1].run(b) * density;
            }
            (a, b)
        };
        // the virtual microphones: a common part and each pattern's own (50 % correlated)
        let common = 0.5 * (a + b);
        let side = 0.5 * (a - b);
        let mut mics = [0f32; 9];
        for (k, m) in self.mics.iter_mut().enumerate() {
            let own = m.run(if k % 2 == 0 { side } else { -side } + 0.3 * common);
            mics[k + 1] = 0.7071 * common + 0.7071 * own * 1.2;
        }
        let mut tail = vec![0f32; n];
        let mut mean = 0f32;
        let mut live = 0;
        for k in 0..n {
            let p = self.cfg[k].pattern;
            if p > 0 {
                tail[k] = mics[p.min(8)];
                mean += tail[k];
                live += 1;
            }
        }
        if live > 1 {
            mean /= live as f32;
            for k in 0..n {
                if self.cfg[k].pattern > 0 {
                    tail[k] = mean + (tail[k] - mean) * width;
                }
            }
        }
        for k in 0..n {
            let c = self.cfg[k];
            self.rev_line[k].push(tail[k]);
            let t = if c.rev_delay >= 1.0 { self.rev_line[k].read_lin(c.rev_delay) } else { tail[k] };
            let t = self.hc[k].run(t);
            let refl = self.in_line[c.refl_src.min(n - 1)].read_lin(c.refl_delay);
            let dry = x[c.dry_src.min(n - 1)];
            y[k] = dry * c.dry + refl * c.refl + t * c.rev * 0.5;
            self.meter_out[k] = (self.meter_out[k] * self.fall).max(y[k].abs());
        }
    }
}

impl Effect for Quantec {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.layout.len() != 2 {
            self.configure(&[lg_dsp::surround::Ch::L, lg_dsp::surround::Ch::R]);
        }
        let mut y = [0f32; 2];
        for i in 0..left.len().min(right.len()) {
            self.frame(&[left[i], right[i]], &mut y);
            left[i] = y[0];
            right[i] = y[1];
        }
    }
    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<lg_dsp::surround::Ch>) -> bool {
        self.configure(layout);
        let n = chans.first().map_or(0, |c| c.len());
        let mut x = vec![0f32; chans.len()];
        let mut y = vec![0f32; chans.len()];
        for i in 0..n {
            for (k, c) in chans.iter().enumerate() {
                x[k] = c[i];
            }
            self.frame(&x, &mut y);
            for (k, c) in chans.iter_mut().enumerate() {
                c[i] = y[k];
            }
        }
        true
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        static N: std::sync::OnceLock<Vec<(&'static str, &'static str, &'static str)>> = std::sync::OnceLock::new();
        let names = N.get_or_init(|| (0..17).map(|k| -> (&'static str, &'static str, &'static str) { (Box::leak(format!("In {k}").into_boxed_str()), Box::leak(format!("Out {k}").into_boxed_str()), Box::leak(format!("Ch {k}").into_boxed_str())) }).collect());
        let (lo, hi) = Quantec::time_range(self.p.get(SIZE));
        let mut v = vec![("Channels", self.layout.len() as f32), ("Time Min", lo), ("Time Max", hi), ("Patterns", self.patterns_available() as f32)];
        for (k, c) in self.layout.iter().enumerate().take(17) {
            v.push((names[k].0, lg_dsp::gain_to_db(self.meter_in[k])));
            v.push((names[k].1, lg_dsp::gain_to_db(self.meter_out[k])));
            v.push((names[k].2, lg_dsp::surround::Ch::ALL.iter().position(|x| x == c).unwrap_or(0) as f32));
        }
        v
    }
}
plugin_impl!(Quantec);


pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "EnVerb" => Box::new(EnVerb::new(rate)),
        "SilverVerb" => Box::new(SilverVerb::new(rate)),
        "Quantec Room Simulator" | "Quantec" | "QRS" => Box::new(Quantec::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["EnVerb", "SilverVerb", "Quantec Room Simulator"];

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    fn impulse(fx: &mut dyn Effect, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        l
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum()
    }

    #[test]
    fn enverb_tail_follows_its_envelope() {
        let mut e = EnVerb::new(48000.0);
        e.set(B + 12, -96.0);
        e.set(B, 500.0); // a slow 0.5 s attack
        e.set(B + 6, 0.0);
        let y = impulse(&mut e, 96000);
        // the swell: more energy at 0.4–0.5 s than at 0–0.1 s
        assert!(energy(&y[19200..24000]) > 4.0 * energy(&y[0..4800]));
        // and nothing after the envelope ends
        let end = ((0.5 + 0.3 + 0.3 + 0.8) * 48000.0) as usize + 500;
        assert!(energy(&y[end..]) < 1e-9);
    }

    #[test]
    fn silververb_rings_then_decays() {
        let mut s = SilverVerb::new(48000.0);
        s.set(B + 10, 100.0);
        let y = impulse(&mut s, 96000 * 2);
        assert!(energy(&y[4800..14400]) > 10.0 * energy(&y[150000..170000]));
    }

    #[test]
    fn quantec_freeze_holds_the_tail() {
        let mut q = Quantec::new(48000.0);
        q.set(B + 10, -96.0);
        q.set(B + 1, 1.0);
        q.set(B + 8, 100.0);
        let mut l = vec![0.0; 24000];
        for (i, v) in l.iter_mut().enumerate().take(4800) {
            *v = ((i as f32) * 0.05).sin() * 0.5;
        }
        let mut r = l.clone();
        q.process(&mut l, &mut r);
        q.set(B + 2, 1.0);
        let mut l2 = vec![0.0; 96000];
        let mut r2 = l2.clone();
        q.process(&mut l2, &mut r2);
        // two seconds later it's still sounding at about the same level
        let a = energy(&l2[0..9600]);
        let b = energy(&l2[86400..96000]);
        assert!(b > a * 0.3, "{a} {b}");
        q.set(B + 4, 1.0); // Clear
        let mut l3 = vec![0.0; 9600];
        let mut r3 = l3.clone();
        q.process(&mut l3, &mut r3);
        assert!(energy(&l3[4800..]) < 1e-6);
    }

    #[test]
    fn room_size_sets_the_time_range() {
        let (a, b) = Quantec::time_range(1.0);
        let (c, d) = Quantec::time_range(1_000_000.0);
        assert!(a < c && b < d && d >= 99.0);
    }

    fn qrs_impulse(q: &mut Quantec, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        q.process(&mut l, &mut r);
        (l, r)
    }

    /// The same correlation pattern on both sides gives a mono tail; Off mutes that side only.
    #[test]
    fn quantec_patterns() {
        let mut q = Quantec::new(48000.0);
        q.set(DRY, -96.0);
        q.set(REFL_LEVEL, -96.0);
        q.set(PATTERN_L, 3.0);
        q.set(PATTERN_R, 3.0);
        let (l, r) = qrs_impulse(&mut q, 24000);
        assert!(energy(&l) > 1e-4);
        assert!(l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6));
        q.reset();
        q.set(PATTERN_R, 0.0);
        let (l, r) = qrs_impulse(&mut q, 24000);
        assert!(energy(&l) > 1e-4 && energy(&r) < 1e-6, "{} {}", energy(&l), energy(&r));
        // YardStick Simple offers two patterns
        q.set(MODE, 2.0);
        assert_eq!(q.patterns_available(), 2);
    }

    /// The first reflection comes at its delay, spread puts the right side later, phase reverse
    /// flips it (QRS).
    #[test]
    fn quantec_first_reflection() {
        let mut q = Quantec::new(48000.0);
        q.set(MODEL, 0.0);
        q.set(DRY, -96.0);
        q.set(REV_LEVEL, -96.0);
        q.set(REFL_LEVEL, 0.0);
        q.set(REFL_DELAY, 10.0);
        q.set(REFL_SPREAD, 4.0);
        q.set(SUBSONIC, 0.0);
        let (l, r) = qrs_impulse(&mut q, 4800);
        let peak = |x: &[f32]| (0..x.len()).max_by(|a, b| x[*a].abs().partial_cmp(&x[*b].abs()).unwrap()).unwrap();
        assert!((peak(&l) as i32 - 384).abs() <= 2, "{}", peak(&l));
        assert!((peak(&r) as i32 - 576).abs() <= 2, "{}", peak(&r));
        q.set(REFL_PHASE, 1.0);
        q.reset();
        let (l2, _) = qrs_impulse(&mut q, 4800);
        assert!(l2[peak(&l)] * l[peak(&l)] < 0.0);
    }

    /// Surround: each channel's details work; a new format resets them.
    #[test]
    fn quantec_surround_details() {
        use lg_dsp::surround::Ch;
        let mut q = Quantec::new(48000.0);
        let layout = vec![Ch::L, Ch::R, Ch::C, Ch::Lfe, Ch::Ls, Ch::Rs];
        q.configure(&layout);
        let c = Ch::ALL.iter().position(|x| *x == Ch::C).unwrap();
        q.set(qch(c) + OFFSETS[4], -31.0); // C dry muted
        let mut chans: Vec<Vec<f32>> = layout.iter().map(|_| vec![0.5; 480]).collect();
        let mut lay = layout.clone();
        assert!(q.process_surround(&mut chans, &mut lay));
        assert!(chans[2][0].abs() < 1e-6 && chans[0][0].abs() > 0.4);
        q.configure(&[Ch::L, Ch::R, Ch::C, Ch::Lfe, Ch::Ls, Ch::Rs, Ch::Lm, Ch::Rm]);
        assert_eq!(q.get(qch(c) + OFFSETS[4]), 0.0);
    }

    /// Enhance (QRS): a short dense cluster, no long tail.
    #[test]
    fn quantec_enhance_is_short() {
        let mut q = Quantec::new(48000.0);
        q.set(MODEL, 0.0);
        q.set(DRY, -96.0);
        q.set(REFL_LEVEL, -96.0);
        q.set(TIME, 5.0);
        q.set(SIZE, 100.0);
        q.set(ENHANCE, 1.0);
        let (l, _) = qrs_impulse(&mut q, 48000);
        assert!(energy(&l[..4800]) > 1e-3);
        assert!(energy(&l[24000..]) < 1e-9);
    }
}
