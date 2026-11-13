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
//! Beyond the decoded block (our ids from 1000): Synthesized IR / Sampled IR mode, the sampled
//! impulse response (a WAV the user loads: kept in ~/.config/bramble-studio/impulse-responses,
//! also read from ~/Music/Impulse Responses; Apple's IRs are never shipped), IR Offset, Quality
//! (Lo-Fi / Low / Medium / High), the volume envelope's LIN / EXP decay, Density Ramp Time and
//! Reflection Shape, the six-band Output EQ (high-pass, low shelf, two peaks, high shelf,
//! low-pass), Volume Compensation, the Input mode (Stereo / Mono), Predelay Sync and X-Over.
//! Sampled IRs are shaped by the same envelopes, filter, length and size (size resamples them),
//! up to six seconds.
//!
//! Parameter positions follow the research map where it anchors them (Predelay, Reverse, the
//! envelopes, filter, densities, spreads, Size); Output Dry / Wet and Length are placed at the
//! unlabelled floats next to Predelay and are unconfirmed. Responses are regenerated when a
//! setting changes (in `set`, not in `process`).

use crate::convolve::Convolver;
use crate::util::{Biquad, Coefs, Delay, Noise, Values, db, plugin_impl};
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

pub const IR_MODE: u32 = 1000;
pub const SAMPLED_IR: u32 = 1001;
pub const QUALITY: u32 = 1002;
pub const IR_OFFSET: u32 = 1003;
pub const DECAY_EXP: u32 = 1004;
pub const RAMP_TIME: u32 = 1005;
pub const REFL_SHAPE: u32 = 1006;
pub const EQ_ON: u32 = 1010;
/// Output EQ bands 1..6 at 1011 + 3 (band − 1): frequency, gain (or order for 1 and 6), Q.
pub const EQ_BASE: u32 = 1011;
pub const VOL_COMP: u32 = 1031;
pub const INPUT_MODE: u32 = 1032;
pub const PREDELAY_SYNC: u32 = 1033;
pub const XOVER: u32 = 1034;
pub const LATENCY_COMP: u32 = 1035;
/// The display mode bar's Volume Env and Density Env On/Off (Filter Env On/Off is `FILTER`:
/// it switches the filter and its envelope together, as Logic's does).
pub const VOL_ENV_ON: u32 = 1036;
pub const DENS_ENV_ON: u32 = 1037;
pub const FILTER_ENV_ON: u32 = FILTER;

/// The display mode bar's views: Volume Env, Filter Env, Density Env, Output EQ.
pub const VIEWS: [&str; 4] = ["Volume Env", "Filter Env", "Density Env", "Output EQ"];

/// Which display mode bar view a parameter belongs to (None: always in the parameter bar).
pub fn view_of(id: u32) -> Option<usize> {
    match id {
        V_INIT | V_ATTACK | V_DECAY | V_END | DECAY_EXP | VOL_ENV_ON => Some(0),
        F_INIT | F_ATTACK | F_MAX | F_DECAY | F_END | FILTER | F_MODE | F_RES => Some(1),
        INIT_DENS | END_DENS | RAMP_TIME | REFL_SHAPE | DENS_ENV_ON => Some(2),
        EQ_ON => Some(3),
        _ if (EQ_BASE..EQ_BASE + 18).contains(&id) => Some(3),
        _ => None,
    }
}

/// The view's On/Off parameter.
pub fn view_switch(view: usize) -> u32 {
    [VOL_ENV_ON, FILTER, DENS_ENV_ON, EQ_ON][view.min(3)]
}

/// An envelope's shape over the IR (0..1 in time) for the main display, `points` long, from
/// the parameter values `g`: 0 volume, 1 filter (cutoff level), 2 density.
pub fn envelope_curve(g: &dyn Fn(u32) -> f32, which: usize, points: usize) -> Vec<f32> {
    (0..points)
        .map(|i| {
            let t = i as f32 / (points.max(2) - 1) as f32;
            match which {
                0 => {
                    let (vi, va, vd, ve) = (g(V_INIT) / 100.0, g(V_ATTACK) * 0.5, g(V_DECAY).max(0.02), g(V_END) / 100.0);
                    if t < va {
                        vi + (1.0 - vi) * t / va.max(1e-4)
                    } else {
                        let k = ((t - va) / (vd * (1.0 - va)).max(1e-4)).min(1.0);
                        if g(DECAY_EXP) >= 0.5 { ve.max(1e-3).powf(k) } else { 1.0 + (ve - 1.0) * k }
                    }
                }
                1 => {
                    let (fi, fa, fm, fd, fe) = (g(F_INIT) / 100.0, g(F_ATTACK) * 0.5, g(F_MAX) / 100.0, g(F_DECAY), g(F_END) / 100.0);
                    if t < fa { fi + (fm - fi) * t / fa.max(1e-4) } else { fm + (fe - fm) * ((t - fa) / (fd.max(0.02) * (1.0 - fa)).max(1e-4)).min(1.0) }
                }
                _ => {
                    let (d0, d1, ramp) = (g(INIT_DENS) / 100.0, g(END_DENS).clamp(0.0, 1.0), g(RAMP_TIME).clamp(0.01, 1.0));
                    d0 + (d1 - d0) * (t / ramp).min(1.0)
                }
            }
        })
        .collect()
}

static BASE_PARAMS: [Param; 22] = [
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
    Param::new(SPREAD_LO, "Stereo Spread Low", 0.0, 1.0, 0.38, Unit::None),
    Param::new(INIT_DENS, "Init Density", 0.0, 100.0, 88.0, Unit::Percent),
    Param::new(END_DENS, "End Density", 0.0, 1.0, 1.0, Unit::None),
    Param::new(SIZE, "Size", 0.1, 2.0, 1.0, Unit::None),
    Param::new(SPREAD_HI, "Stereo Spread High", 0.0, 1.0, 1.0, Unit::None),
];

const EQ_DEFAULTS: [(f32, f32, f32); 6] = [(20.0, 0.0, 0.7), (200.0, 0.0, 0.7), (800.0, 0.0, 1.0), (3000.0, 0.0, 1.0), (8000.0, 0.0, 0.7), (20000.0, 0.0, 0.7)];
const EQ_NAMES: [&str; 6] = ["High Pass", "Low Shelf", "Parametric 1", "Parametric 2", "High Shelf", "Low Pass"];

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = BASE_PARAMS.to_vec();
    v.extend([
        Param::menu(IR_MODE, "IR Mode", &["Synthesized IR", "Sampled IR"], 0.0),
        Param::new(SAMPLED_IR, "Sampled IR", 0.0, 9999.0, 0.0, Unit::None),
        Param::menu(QUALITY, "Quality", &["Lo-Fi", "Low", "Medium", "High"], 3.0),
        Param::new(IR_OFFSET, "IR Offset", 0.0, 1000.0, 0.0, Unit::Ms),
        Param::menu(DECAY_EXP, "Volume Decay", &["LIN", "EXP"], 1.0),
        Param::new(RAMP_TIME, "Density Ramp Time", 0.0, 1.0, 0.17, Unit::None),
        Param::new(REFL_SHAPE, "Reflection Shape", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(EQ_ON, "Output EQ", 0.0, 1.0, 0.0, Unit::OnOff),
    ]);
    for (b, (f, g, q)) in EQ_DEFAULTS.iter().enumerate() {
        let id = EQ_BASE + 3 * b as u32;
        let n = EQ_NAMES[b];
        v.push(Param::new(id, crate::kit::leak(format!("EQ {n} Freq")), 20.0, 20000.0, *f, Unit::Hz));
        if b == 0 || b == 5 {
            v.push(Param::menu(id + 1, crate::kit::leak(format!("EQ {n} Order")), &["6 dB/Oct", "12 dB/Oct", "18 dB/Oct", "24 dB/Oct"], 1.0));
        } else {
            v.push(Param::new(id + 1, crate::kit::leak(format!("EQ {n} Gain")), -24.0, 24.0, *g, Unit::Db));
        }
        v.push(Param::new(id + 2, crate::kit::leak(format!("EQ {n} Q")), 0.1, 10.0, *q, Unit::None));
    }
    v.extend([
        Param::new(VOL_COMP, "Volume Compensation", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(INPUT_MODE, "Input", &["Stereo", "Mono"], 0.0),
        Param::new(PREDELAY_SYNC, "Predelay Sync", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(XOVER, "X-Over", 50.0, 2000.0, 300.0, Unit::Hz),
        Param::new(LATENCY_COMP, "Latency Compensation", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(VOL_ENV_ON, "Volume Env", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(DENS_ENV_ON, "Density Env", 0.0, 1.0, 1.0, Unit::OnOff),
    ]);
    v
});

/// Where user impulse responses live: Studio's own folder (where Load IR copies them) and
/// ~/Music/Impulse Responses.
pub fn ir_folders() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config")));
    let mut v = Vec::new();
    if let Some(c) = cfg {
        v.push(c.join("bramble-studio").join("impulse-responses"));
    }
    if let Some(h) = home {
        v.push(h.join("Music").join("Impulse Responses"));
    }
    v
}

/// The user's impulse responses (WAV), by name: the Sampled IR parameter is 1 + the index here.
pub fn user_irs() -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = ir_folders().into_iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"))).collect();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    v.dedup_by_key(|p| p.file_name().map(|n| n.to_owned()));
    v
}

/// Reads a WAV (PCM 8/16/24/32-bit or 32/64-bit float): (left, right, rate). Mono gives the
/// same channel twice; more channels keep the first two.
pub fn read_wav(path: &std::path::Path) -> Option<(Vec<f32>, Vec<f32>, f32)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut at = 12;
    let (mut fmt, mut ch, mut rate, mut bits) = (0u16, 0usize, 0u32, 0u16);
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = u32_at(at + 4) as usize;
        let body = at + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            fmt = u16_at(body);
            ch = u16_at(body + 2) as usize;
            rate = u32_at(body + 4);
            bits = u16_at(body + 14);
            if fmt == 0xfffe && body + 26 <= b.len() {
                fmt = u16_at(body + 24);
            }
        } else if id == b"data" && ch > 0 {
            let data = &b[body..(body + len).min(b.len())];
            let bps = (bits / 8) as usize;
            let frames = data.len() / (bps * ch);
            let sample = |k: usize| -> f32 {
                let s = &data[k * bps..k * bps + bps];
                match (fmt, bits) {
                    (1, 8) => (s[0] as f32 - 128.0) / 128.0,
                    (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                    (1, 24) => ((i32::from_le_bytes([0, s[0], s[1], s[2]])) >> 8) as f32 / 8_388_608.0,
                    (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                    (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    (3, 64) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32,
                    _ => 0.0,
                }
            };
            let l: Vec<f32> = (0..frames).map(|f| sample(f * ch)).collect();
            let r: Vec<f32> = if ch > 1 { (0..frames).map(|f| sample(f * ch + 1)).collect() } else { l.clone() };
            return Some((l, r, rate as f32));
        }
        at = body + len + (len & 1);
    }
    None
}

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
    bpm: f64,
    /// The loaded sample IR (at the engine rate) and which one it is.
    sampled: Option<(usize, [Vec<f32>; 2])>,
    eq: [[Biquad; 8]; 2],
    eq_used: bool,
}

impl SpaceConvolver {
    pub fn new(rate: f32) -> SpaceConvolver {
        let max = (MAX_SECS * rate) as usize;
        let mut s = SpaceConvolver {
            p: Values::new(PARAMS.as_slice()),
            rate,
            conv: [Convolver::new(max), Convolver::new(max)],
            ir: [vec![0.0; max], vec![0.0; max]],
            ir_len: 0,
            pre: [Delay::new((0.26 * rate) as usize), Delay::new((0.26 * rate) as usize)],
            pre_len: 1.0,
            dry: 1.0,
            wet: 0.3,
            bpm: 120.0,
            sampled: None,
            eq: [[Biquad::default(); 8]; 2],
            eq_used: false,
        };
        s.update();
        s
    }

    /// Builds the stereo response into `self.ir` (no allocation: the buffers are full size).
    /// Loads the chosen sample IR (resampled to the engine rate) when it changed.
    fn load_sampled(&mut self) {
        let want = self.p.get(SAMPLED_IR).round() as usize;
        if want == 0 {
            self.sampled = None;
            return;
        }
        if self.sampled.as_ref().is_some_and(|s| s.0 == want) {
            return;
        }
        let Some(path) = user_irs().get(want - 1).cloned() else {
            self.sampled = None;
            return;
        };
        self.sampled = read_wav(&path).map(|(l, r, sr)| {
            let ratio = sr / self.rate;
            let n = ((l.len() as f32 / ratio) as usize).min(self.ir[0].len());
            let rs = |x: &[f32]| (0..n).map(|i| {
                let t = i as f32 * ratio;
                let k = t as usize;
                let f = t - k as f32;
                x.get(k).copied().unwrap_or(0.0) * (1.0 - f) + x.get(k + 1).copied().unwrap_or(0.0) * f
            }).collect::<Vec<f32>>();
            (want, [rs(&l), rs(&r)])
        });
    }

    /// Shapes the loaded sample IR into `self.ir`: IR Offset, Length × Size (Size resamples),
    /// the volume envelope (LIN / EXP decay), the filter envelope, Reverse.
    fn shape_sampled(&mut self) -> bool {
        let Some((_, src)) = &self.sampled else { return false };
        let p = &self.p;
        let rate = self.rate;
        let off = (p.get(IR_OFFSET) * 0.001 * rate) as usize;
        let size = p.get(SIZE).clamp(0.1, 2.0);
        let avail = src[0].len().saturating_sub(off) as f32 / size;
        let n = ((avail * p.get(LENGTH) / 100.0) as usize).min(self.ir[0].len()).max(1);
        self.ir_len = n;
        let (vi, va, vd, ve) = (p.get(V_INIT) / 100.0, p.get(V_ATTACK), p.get(V_DECAY), p.get(V_END) / 100.0);
        let exp = p.idx(DECAY_EXP) == 1;
        let filter = p.on(FILTER);
        let vol_env = p.on(VOL_ENV_ON);
        let (fi, fa, fm, fd, fe) = (p.get(F_INIT) / 100.0, p.get(F_ATTACK), p.get(F_MAX) / 100.0, p.get(F_DECAY), p.get(F_END) / 100.0);
        let att_n = (va * 0.5 * n as f32) as usize;
        let dec_n = (vd.max(0.02) * n as f32).max(1.0);
        let mut lp = [[0f32; 2]; 2];
        for i in 0..n {
            let t = i as f32 / n as f32;
            let att = if i < att_n { vi + (1.0 - vi) * (i as f32 / att_n.max(1) as f32) } else { 1.0 };
            let k = ((i.saturating_sub(att_n)) as f32 / dec_n).min(1.0);
            let vol = if !vol_env { 1.0 } else { att * if exp { (ve.max(1e-3)).powf(k) } else { 1.0 + (ve - 1.0) * k } };
            let cut = if filter {
                let fa_n = fa * 0.5;
                let lvl = if t < fa_n { fi + (fm - fi) * t / fa_n.max(1e-4) } else { fm + (fe - fm) * ((t - fa_n) / (fd.max(0.02) * (1.0 - fa_n)).max(1e-4)).min(1.0) };
                40.0 * (20000.0f32 / 40.0).powf(lvl.clamp(0.0, 1.0))
            } else {
                rate * 0.49
            };
            let g = 1.0 - (-crate::util::TAU * cut.min(rate * 0.45) / rate).exp();
            for c in 0..2 {
                let pos = off as f32 + i as f32 * size;
                let k0 = pos as usize;
                let fr = pos - k0 as f32;
                let x = src[c].get(k0).copied().unwrap_or(0.0) * (1.0 - fr) + src[c].get(k0 + 1).copied().unwrap_or(0.0) * fr;
                let mut y = x * vol;
                if filter {
                    lp[c][0] += g * (y - lp[c][0]);
                    lp[c][1] += g * (lp[c][0] - lp[c][1]);
                    y = lp[c][1];
                }
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
        true
    }

    /// Quality: Lo-Fi (a quarter of the rate), Low (half), Medium / High (full).
    fn apply_quality(&mut self) {
        let q = self.p.idx(QUALITY);
        if q >= 2 {
            return;
        }
        let n = self.ir_len;
        let div = if q == 0 { 4 } else { 2 };
        let c = Coefs::lowpass(self.rate, self.rate * 0.45 / div as f32, 0.707);
        for ch in self.ir.iter_mut() {
            let mut b = Biquad::new(c);
            let mut b2 = Biquad::new(c);
            for v in ch[..n].iter_mut() {
                *v = b2.run(b.run(*v));
            }
            if q == 0 {
                // grainy: hold every fourth sample
                for k in (0..n).step_by(4) {
                    let h = ch[k];
                    for v in ch[k..(k + 4).min(n)].iter_mut() {
                        *v = h;
                    }
                }
            }
        }
    }

    fn normalise(&mut self) {
        let n = self.ir_len;
        let e: f64 = self.ir.iter().map(|ch| ch[..n].iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum::<f64>() / 2.0;
        let g = (db(IR_ENERGY_DB) as f64 / e.max(1e-20).sqrt()) as f32;
        for c in 0..2 {
            self.ir[c][..n].iter_mut().for_each(|v| *v *= g);
        }
    }

    fn update_eq(&mut self) {
        let p = &self.p;
        let r = self.rate;
        let on = p.on(EQ_ON);
        let mut c = [Coefs::THRU; 8];
        if on {
            let f = |b: u32| p.get(EQ_BASE + 3 * b).min(r * 0.45);
            let g = |b: u32| p.get(EQ_BASE + 3 * b + 1);
            let q = |b: u32| p.get(EQ_BASE + 3 * b + 2);
            let hp_order = p.idx(EQ_BASE + 1);
            let lp_order = p.idx(EQ_BASE + 16);
            if f(0) > 20.5 {
                c[0] = if hp_order == 0 { Coefs::hp1(r, f(0)) } else { Coefs::highpass(r, f(0), q(0)) };
                if hp_order >= 2 {
                    c[6] = if hp_order == 2 { Coefs::hp1(r, f(0)) } else { Coefs::highpass(r, f(0), 0.707) };
                }
            }
            c[1] = if g(1).abs() > 0.01 { Coefs::low_shelf(r, f(1), q(1), g(1)) } else { Coefs::THRU };
            c[2] = if g(2).abs() > 0.01 { Coefs::peak(r, f(2), q(2), g(2)) } else { Coefs::THRU };
            c[3] = if g(3).abs() > 0.01 { Coefs::peak(r, f(3), q(3), g(3)) } else { Coefs::THRU };
            c[4] = if g(4).abs() > 0.01 { Coefs::high_shelf(r, f(4), q(4), g(4)) } else { Coefs::THRU };
            if f(5) < 19999.0 {
                c[5] = if lp_order == 0 { Coefs::lp1(r, f(5)) } else { Coefs::lowpass(r, f(5), q(5)) };
                if lp_order >= 2 {
                    c[7] = if lp_order == 2 { Coefs::lp1(r, f(5)) } else { Coefs::lowpass(r, f(5), 0.707) };
                }
            }
        }
        self.eq_used = c.iter().any(|x| *x != Coefs::THRU);
        for ch in self.eq.iter_mut() {
            for (k, b) in ch.iter_mut().enumerate() {
                b.c = c[k];
            }
        }
    }

    fn synthesise(&mut self) {
        if self.p.idx(IR_MODE) == 1 {
            self.load_sampled();
            if self.shape_sampled() {
                self.apply_quality();
                if self.p.on(VOL_COMP) {
                    self.normalise();
                }
                return;
            }
        }
        self.synthesise_space();
        self.apply_quality();
    }

    fn synthesise_space(&mut self) {
        let p = &self.p;
        let rate = self.rate;
        let secs = (BASE_SECS * p.get(LENGTH) / 100.0 * p.get(SIZE)).clamp(0.05, MAX_SECS);
        let n = ((secs * rate) as usize).min(self.ir[0].len());
        self.ir_len = n;
        // Volume Env off: no attack, a plain fall over the whole length (the room's own decay)
        let (vi, va, vd, ve) = if p.on(VOL_ENV_ON) { (p.get(V_INIT) / 100.0, p.get(V_ATTACK), p.get(V_DECAY), p.get(V_END) / 100.0) } else { (1.0, 0.0, 1.0, 0.3) };
        let (fi, fa, fm, fd, fe) = (p.get(F_INIT) / 100.0, p.get(F_ATTACK), p.get(F_MAX) / 100.0, p.get(F_DECAY), p.get(F_END) / 100.0);
        let filter = p.on(FILTER);
        let mode = p.idx(F_MODE);
        let res = p.get(F_RES) / 100.0;
        // Density Env off: full density from the start
        let (d0, d1) = if p.on(DENS_ENV_ON) { (p.get(INIT_DENS) / 100.0, p.get(END_DENS).clamp(0.0, 1.0)) } else { (1.0, 1.0) };
        let (s_lo, s_hi) = (p.get(SPREAD_LO).clamp(0.0, 1.0), p.get(SPREAD_HI).clamp(0.0, 1.0));
        let att_n = (va * 0.5 * secs * rate) as usize;
        // decay: the volume falls by ~60 dB over (decay time x length), ending at End Level
        let dec_n = ((vd.max(0.02)) * n as f32).max(1.0);
        let mut nz = [Noise(0x1234_5678), Noise(0x9abc_def1), Noise(0x0f0f_1357)];
        // state of the per-channel time-varying filters and the low/high split for the spread
        let mut lp_state = [[0.0f32; 4]; 2];
        let mut split = [0.0f32; 3];
        let split_a = 1.0 - (-crate::util::TAU * p.get(XOVER) / rate).exp();
        let ramp = p.get(RAMP_TIME).clamp(0.01, 1.0);
        let exp = p.idx(DECAY_EXP) == 1;
        // Reflection Shape: steeper early clusters (sharper reflections) at higher values
        let shape = 0.5 + p.get(REFL_SHAPE) / 100.0;
        for i in 0..n {
            let t = i as f32 / n as f32;
            // volume envelope: attack from the initial level, then an exponential fall that
            // reaches a tenth of End Level when the decay time is over (and keeps falling), with a
            // short fade at the very end
            let att = if i < att_n { vi + (1.0 - vi) * (i as f32 / att_n.max(1) as f32) } else { 1.0 };
            let k = (i.saturating_sub(att_n)) as f32 / dec_n;
            let floor = (ve * 0.1).max(1e-3);
            let tailfade = 1.0 - ((t - 0.85).max(0.0) / 0.15).min(1.0);
            let vol = att * if exp { floor.powf(k) } else { (1.0 - k * (1.0 - floor)).max(0.0) } * tailfade;
            // density: early on, only some samples carry reflections (energy kept the same)
            let dens = d0 + (d1 - d0) * (t / ramp).min(1.0);
            let sparse = (dens * dens * dens).max(0.002);
            let keep = (nz[2].next() * 0.5 + 0.5) < sparse;
            let mut e = [0.0f32; 3];
            for (c, v) in e.iter_mut().enumerate() {
                let r = nz[c].next();
                *v = if keep { r.signum() * r.abs().powf(1.0 / shape) / sparse.sqrt() } else { 0.0 };
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
        self.update_eq();
        for c in 0..2 {
            let (conv, ir) = (&mut self.conv[c], &self.ir[c]);
            conv.load(&ir[..self.ir_len]);
        }
        let mut pre_ms = self.p.get(PREDELAY);
        if self.p.on(PREDELAY_SYNC) {
            // the nearest of 1/128 … 1/8 notes at the tempo
            let beat_ms = 60000.0 / self.bpm as f32;
            let notes = [beat_ms / 32.0, beat_ms / 16.0, beat_ms / 8.0, beat_ms / 4.0, beat_ms / 2.0];
            pre_ms = notes.iter().copied().filter(|v| *v <= 250.0).min_by(|a, b| (a - pre_ms).abs().partial_cmp(&(b - pre_ms).abs()).unwrap()).unwrap_or(pre_ms);
        }
        self.pre_len = (pre_ms * 0.001 * self.rate).max(1.0);
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
    fn set_tempo(&mut self, bpm: f64) {
        if (bpm - self.bpm).abs() > 1e-6 {
            self.bpm = bpm.max(1.0);
            if self.p.on(PREDELAY_SYNC) {
                self.update();
            }
        }
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("IR Length", self.ir_len as f32 / self.rate), ("Sampled IR Loaded", self.sampled.is_some() as u8 as f32)]
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        if name != "ir" {
            return Vec::new();
        }
        // the response's envelope at 400 points (peak per bin, both channels)
        let n = self.ir_len.max(1);
        (0..400).map(|k| {
            let (a, b) = (k * n / 400, ((k + 1) * n / 400).max(k * n / 400 + 1).min(n));
            self.ir[0][a..b].iter().chain(&self.ir[1][a..b]).fold(0.0f32, |m, v| m.max(v.abs()))
        }).collect()
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mono = self.p.idx(INPUT_MODE) == 1;
        for i in 0..n {
            let (il, ir) = if mono { let m = 0.5 * (left[i] + right[i]); (m, m) } else { (left[i], right[i]) };
            self.pre[0].push(il);
            self.pre[1].push(ir);
            let a = self.pre[0].read(self.pre_len);
            let b = self.pre[1].read(self.pre_len);
            let mut wl = self.conv[0].run(a);
            let mut wr = self.conv[1].run(b);
            if self.eq_used {
                for k in 0..8 {
                    wl = self.eq[0][k].run(wl);
                    wr = self.eq[1][k].run(wr);
                }
            }
            left[i] = left[i] * self.dry + wl * self.wet;
            right[i] = right[i] * self.dry + wr * self.wet;
        }
    }
}

plugin_impl!(SpaceConvolver);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    fn wav16(path: &std::path::Path, x: &[f32], rate: u32) {
        let mut b = Vec::new();
        let data = x.len() * 2;
        b.extend(b"RIFF");
        b.extend(((36 + data) as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((data as u32).to_le_bytes());
        for v in x {
            b.extend(((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    fn impulse_response(s: &mut SpaceConvolver, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        s.process(&mut l, &mut r);
        l
    }

    #[test]
    fn sampled_ir_is_used_with_offset_and_size() {
        let dir = std::env::temp_dir().join(format!("lgfx-ir-{}", std::process::id()));
        let folder = dir.join("bramble-studio").join("impulse-responses");
        std::fs::create_dir_all(&folder).unwrap();
        // a 24 kHz file with one spike at 0.1 s: resampled to 48 kHz it lands at 4800
        let mut x = vec![0.0; 12000];
        x[2400] = 0.9;
        wav16(&folder.join("spike.wav"), &x, 24000);
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        let mut s = SpaceConvolver::new(48000.0);
        for (id, v) in [(DRY, -96.0), (WET, 0.0), (PREDELAY, 0.0), (V_INIT, 100.0), (V_END, 100.0), (LENGTH, 100.0), (VOL_COMP, 0.0), (IR_MODE, 1.0), (SAMPLED_IR, 1.0)] {
            s.set(id, v);
        }
        assert_eq!(s.readouts()[1].1, 1.0);
        let y = impulse_response(&mut s, 20000);
        let at = (0..20000).max_by(|a, b| y[*a].abs().partial_cmp(&y[*b].abs()).unwrap()).unwrap();
        assert!((at as i64 - 4801).abs() <= 2, "{at}");
        // Size 200 %: reads the file twice as fast, so the spike comes at half the time
        s.set(SIZE, 2.0);
        s.reset();
        let y = impulse_response(&mut s, 20000);
        let at2 = (0..20000).max_by(|a, b| y[*a].abs().partial_cmp(&y[*b].abs()).unwrap()).unwrap();
        assert!((at2 as i64 - 2401).abs() <= 2, "{at2}");
        // IR Offset 50 ms moves it earlier by 2400 at Size 100 %
        s.set(SIZE, 1.0);
        s.set(IR_OFFSET, 50.0);
        s.reset();
        let y = impulse_response(&mut s, 20000);
        let at3 = (0..20000).max_by(|a, b| y[*a].abs().partial_cmp(&y[*b].abs()).unwrap()).unwrap();
        assert!((at3 as i64 - 2401).abs() <= 2, "{at3}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_mode_bar_switches_turn_envelopes_off() {
        // Volume Env off: a slow attack no longer holds back the start
        let mut s = SpaceConvolver::new(48000.0);
        s.set(DRY, -96.0);
        s.set(V_INIT, 0.0);
        s.set(V_ATTACK, 1.0);
        let early = |y: &[f32]| y[..2400].iter().map(|v| v * v).sum::<f32>() / y.iter().map(|v| v * v).sum::<f32>().max(1e-12);
        let a = early(&impulse_response(&mut s, 48000));
        s.set(VOL_ENV_ON, 0.0);
        s.reset();
        let b = early(&impulse_response(&mut s, 48000));
        assert!(b > a * 2.0, "{a} {b}");
        // Density Env off: the start is as dense as the rest
        let mut s = SpaceConvolver::new(48000.0);
        s.set(DRY, -96.0);
        s.set(INIT_DENS, 0.0);
        s.set(RAMP_TIME, 1.0);
        let zeros = |y: &[f32]| y[100..2500].iter().filter(|v| v.abs() < 1e-7).count();
        let za = zeros(&impulse_response(&mut s, 4800));
        s.set(DENS_ENV_ON, 0.0);
        s.reset();
        let zb = zeros(&impulse_response(&mut s, 4800));
        assert!(zb < za, "{za} {zb}");
        // the views own their parameters; Filter Env On/Off is the filter's switch
        assert_eq!(view_of(V_DECAY), Some(0));
        assert_eq!(view_of(F_RES), Some(1));
        assert_eq!(view_of(INIT_DENS), Some(2));
        assert_eq!(view_of(EQ_BASE + 4), Some(3));
        assert_eq!(view_of(WET), None);
        assert_eq!(view_switch(1), FILTER);
        let g = |id: u32| PARAMS.iter().find(|p| p.id == id).map_or(0.0, |p| p.default);
        let c = envelope_curve(&g, 0, 50);
        assert_eq!(c.len(), 50);
        assert!(c[49] < c[10]);
    }

    #[test]
    fn synthesized_length_follows_length_and_size() {
        let mut s = SpaceConvolver::new(48000.0);
        let a = s.readouts()[0].1;
        s.set(SIZE, 0.5);
        let b = s.readouts()[0].1;
        assert!((a / b - 2.0).abs() < 0.05, "{a} {b}");
    }

    #[test]
    fn output_eq_low_pass_darkens_the_tail() {
        let mut s = SpaceConvolver::new(48000.0);
        s.set(DRY, -96.0);
        let energy_hi = |y: &[f32]| y.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>();
        let a = energy_hi(&impulse_response(&mut s, 48000));
        s.set(EQ_ON, 1.0);
        s.set(EQ_BASE + 15, 1000.0);
        s.reset();
        let b = energy_hi(&impulse_response(&mut s, 48000));
        assert!(b < a * 0.2, "{a} {b}");
    }

    #[test]
    fn mono_input_sums() {
        let mut s = SpaceConvolver::new(48000.0);
        s.set(INPUT_MODE, 1.0);
        s.set(DRY, -96.0);
        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        l[0] = 1.0;
        s.process(&mut l, &mut r);
        assert!(r.iter().any(|v| v.abs() > 1e-4));
    }
}
