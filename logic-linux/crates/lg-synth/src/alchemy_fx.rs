//! Alchemy's effects section (Studio's own effects after the guide's list): five racks (Main, A, B,
//! C, D) of up to eight slots, each slot an effect with its On switch: Acoustic Reverb, Classic
//! Reverb, Convolution Reverb (Studio's synthesized spaces or the user's impulse responses),
//! Delay, Modulation FX, Phaser, Amp, Panner, Phat Compressor, Vintage Compressor, Bandpass
//! Filter, Band Reject (blends back what the Bandpass before it took out), Multimode Filter (the
//! 39 filter types), Three Band EQ, Bass Enhancer, Distortion (Bitcrusher, Mech, Tube and XCite at
//! once), Waveshaper (a three-handle transfer curve). Knob values are stored 0..1 and scaled by
//! each effect's knob table.

use lg_dsp::{Param, Unit};

use crate::alchemy_filter::{self as af, AFilt};
use crate::dsp::{self, Biquad, OnePole};

pub const FXR: u32 = 16000;
pub const RACKS: usize = 5;
pub const SLOTS: usize = 8;
pub const KNOBS: usize = 20;
pub const RACK_NAMES: &[&str] = &["Main", "A", "B", "C", "D"];

/// A slot's parameter: FXR + rack × 256 + slot × 24 + (0 type, 1 on, 2.. knobs).
pub fn id(rack: usize, slot: usize, f: u32) -> u32 {
    FXR + rack as u32 * 256 + slot as u32 * 24 + f
}

pub const TYPES: &[&str] = &[
    "None", "Acoustic Reverb", "Classic Reverb", "Convolution Reverb", "Delay", "Modulation FX", "Phaser", "Amp", "Panner", "Phat Compressor", "Vintage Compressor", "Bandpass Filter", "Band Reject", "Multimode Filter", "Three Band EQ", "Bass Enhancer", "Distortion", "Waveshaper",
];

/// Studio's synthesized spaces for the Convolution Reverb (the user's IRs follow them).
pub const SPACES: &[&str] = &["Small Room", "Studio", "Chamber", "Plate", "Hall", "Cathedral", "Spring", "Nonlinear"];

/// A knob: name, range, default, kind (0 linear, 1 logarithmic, 2 switch, 3 menu of `choices`).
#[derive(Clone, Copy)]
pub struct Ks {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub kind: u8,
    pub choices: &'static [&'static str],
}

const fn k(name: &'static str, min: f32, max: f32, def: f32) -> Ks {
    Ks { name, min, max, def, kind: 0, choices: &[] }
}
const fn kl(name: &'static str, min: f32, max: f32, def: f32) -> Ks {
    Ks { name, min, max, def, kind: 1, choices: &[] }
}
const fn sw(name: &'static str, def: f32) -> Ks {
    Ks { name, min: 0.0, max: 1.0, def, kind: 2, choices: &[] }
}
const fn mn(name: &'static str, choices: &'static [&'static str], def: f32) -> Ks {
    Ks { name, min: 0.0, max: (choices.len() - 1) as f32, def, kind: 3, choices }
}

const DIVS: &[&str] = crate::alchemy_mod::SYNC_DIVS;
const FT: &[&str] = af::FILTER_TYPES;

pub fn knobs(ty: usize) -> &'static [Ks] {
    match ty {
        1 => {
            const T: &[Ks] = &[kl("Time", 0.1, 20.0, 2.5), k("Predelay", 0.0, 250.0, 20.0), k("Size", 0.0, 1.0, 0.6), k("Width", 0.0, 1.0, 1.0), k("Diffusion", 0.0, 1.0, 0.7), k("Gate", 0.0, 1.0, 0.0), k("Attack", 0.0, 1.0, 0.05), k("Decay", 0.0, 2.0, 0.3), kl("EQ Lo Freq", 20.0, 1000.0, 200.0), k("EQ Lo Gain", -18.0, 18.0, 0.0), kl("EQ Hi Freq", 1000.0, 20000.0, 6000.0), k("EQ Hi Gain", -18.0, 18.0, 0.0), kl("Damp Lo Freq", 20.0, 1000.0, 150.0), k("Lo Damp", 0.0, 1.0, 0.0), kl("Damp Hi Freq", 1000.0, 20000.0, 5000.0), k("Hi Damp", 0.0, 1.0, 0.4), k("Variation", 0.0, 1.0, 0.5), k("Quality", 0.0, 1.0, 1.0), k("Mix", 0.0, 1.0, 0.3)];
            T
        }
        2 => {
            const T: &[Ks] = &[k("Predelay", 0.0, 250.0, 10.0), k("Damping", 0.0, 1.0, 0.4), kl("Hi Cut", 1000.0, 20000.0, 12000.0), kl("Lo Cut", 20.0, 1000.0, 40.0), k("Size", 0.0, 1.0, 0.5), k("Mix", 0.0, 1.0, 0.3)];
            T
        }
        3 => {
            const T: &[Ks] = &[mn("IR", SPACES, 4.0), k("Dry", 0.0, 1.0, 1.0), k("Wet", 0.0, 1.0, 0.4), k("Pre-Delay", 0.0, 250.0, 0.0), k("Size", 0.25, 2.0, 1.0), k("Stereo", 0.0, 1.0, 1.0), k("Start", 0.0, 1.0, 0.0), k("End", 0.0, 1.0, 1.0), sw("Reverse", 0.0), sw("Filter", 0.0), mn("Filter Type", FT, 10.0), k("Cutoff", 0.0, 1.0, 0.8), k("Resonance", 0.0, 1.0, 0.1)];
            T
        }
        4 => {
            const T: &[Ks] = &[sw("Sync", 1.0), kl("Rate L", 1.0, 2000.0, 375.0), kl("Rate R", 1.0, 2000.0, 250.0), k("Offset L", -50.0, 50.0, 0.0), k("Offset R", -50.0, 50.0, 0.0), k("Feedback L", 0.0, 1.0, 0.35), k("Feedback R", 0.0, 1.0, 0.35), sw("Filter A", 0.0), mn("A Type", FT, 10.0), k("A Cutoff", 0.0, 1.0, 0.8), k("A Res", 0.0, 1.0, 0.1), sw("Filter B", 0.0), mn("B Type", FT, 18.0), k("B Cutoff", 0.0, 1.0, 0.2), k("B Res", 0.0, 1.0, 0.1), sw("Mono", 0.0), k("Crossover", 0.0, 1.0, 0.0), k("Initial Pan", -1.0, 1.0, 0.0), k("Mix", 0.0, 1.0, 0.3)];
            T
        }
        5 => {
            const T: &[Ks] = &[kl("Delay", 0.1, 50.0, 15.0), kl("Rate", 0.05, 10.0, 0.6), k("Mix", 0.0, 1.0, 0.5), k("Depth", 0.0, 1.0, 0.4), k("Feedback", 0.0, 0.95, 0.2), k("Stereo", 0.0, 1.0, 0.8)];
            T
        }
        6 => {
            const T: &[Ks] = &[sw("Sync", 0.0), kl("Rate", 0.05, 10.0, 0.5), k("Depth", 0.0, 1.0, 0.6), k("Spread", 0.0, 1.0, 0.5), kl("Frequency", 100.0, 8000.0, 1000.0), k("Poles", 2.0, 12.0, 6.0), k("Feedback", 0.0, 0.95, 0.4), k("Mix", 0.0, 1.0, 0.5)];
            T
        }
        7 => {
            const T: &[Ks] = &[k("Gain", 0.0, 1.0, 1.0)];
            T
        }
        8 => {
            const T: &[Ks] = &[k("Pan", 0.0, 1.0, 0.5)];
            T
        }
        9 => {
            const T: &[Ks] = &[k("Amount", 0.0, 1.0, 0.4), kl("Release", 0.01, 1.0, 0.15), sw("Phat", 0.0)];
            T
        }
        10 => {
            const T: &[Ks] = &[kl("Attack", 0.1, 100.0, 10.0), kl("Release", 10.0, 1000.0, 150.0), k("Threshold", -60.0, 0.0, -18.0), k("Input Gain", -12.0, 24.0, 0.0), kl("Ratio", 1.0, 20.0, 4.0), k("Makeup", 0.0, 24.0, 4.0)];
            T
        }
        11 => {
            const T: &[Ks] = &[kl("Low Cut", 16.0, 20000.0, 100.0), kl("High Cut", 16.0, 20000.0, 8000.0), k("Low Res", 0.0, 1.0, 0.0), k("High Res", 0.0, 1.0, 0.0)];
            T
        }
        12 => {
            const T: &[Ks] = &[k("Mix", 0.0, 1.0, 1.0)];
            T
        }
        13 => {
            const T: &[Ks] = &[mn("Type", FT, 1.0), k("Cutoff", 0.0, 1.0, 0.6), k("Resonance", 0.0, 1.0, 0.2), k("Drive", 0.0, 1.0, 0.0), k("Mix", 0.0, 1.0, 1.0)];
            T
        }
        14 => {
            const T: &[Ks] = &[k("F1 Gain", -18.0, 18.0, 0.0), kl("F1 Freq", 16.0, 20000.0, 200.0), kl("F1 Q", 0.2, 10.0, 0.7), k("F2 Gain", -18.0, 18.0, 0.0), kl("F2 Freq", 16.0, 20000.0, 1000.0), kl("F2 Q", 0.2, 10.0, 0.7), k("F3 Gain", -18.0, 18.0, 0.0), kl("F3 Freq", 16.0, 20000.0, 5000.0), kl("F3 Q", 0.2, 10.0, 0.7)];
            T
        }
        15 => {
            const T: &[Ks] = &[k("Amount", 0.0, 1.0, 0.4), kl("Tune", 20.0, 200.0, 60.0), sw("Phat", 0.0)];
            T
        }
        16 => {
            const T: &[Ks] = &[k("Bitcrusher", 0.0, 1.0, 0.0), k("Mech", 0.0, 1.0, 0.0), k("Tube", 0.0, 1.0, 0.3), k("XCite", 0.0, 1.0, 0.0), k("Pre Gain", -12.0, 24.0, 0.0), k("Post Gain", -24.0, 12.0, 0.0)];
            T
        }
        17 => {
            const T: &[Ks] = &[k("Mix", 0.0, 1.0, 1.0), k("Pre Gain", -12.0, 24.0, 0.0), k("Post Gain", -24.0, 12.0, 0.0), k("Shape", 0.0, 1.0, 0.5), kl("Frequency", 100.0, 10000.0, 1000.0), sw("Bipolar", 1.0), k("Start", -1.0, 1.0, -1.0), k("Mid X", 0.05, 0.95, 0.5), k("Mid Y", -1.0, 1.0, 0.0), k("End", -1.0, 1.0, 1.0), k("Curve 1", -1.0, 1.0, 0.0), k("Curve 2", -1.0, 1.0, 0.0)];
            T
        }
        _ => &[],
    }
}

/// A knob's real value from its stored 0..1.
pub fn scale(s: &Ks, n: f32) -> f32 {
    let n = n.clamp(0.0, 1.0);
    match s.kind {
        1 => s.min * (s.max / s.min).powf(n),
        2 => (n >= 0.5) as u8 as f32,
        3 => (s.min + n * (s.max - s.min)).round(),
        _ => s.min + n * (s.max - s.min),
    }
}

/// The stored 0..1 for a real value.
pub fn unscale(s: &Ks, v: f32) -> f32 {
    match s.kind {
        1 => ((v.max(s.min) / s.min).ln() / (s.max / s.min).ln()).clamp(0.0, 1.0),
        _ => ((v - s.min) / (s.max - s.min).max(1e-9)).clamp(0.0, 1.0),
    }
}

/// A knob's value as text.
pub fn text(s: &Ks, n: f32) -> String {
    let v = scale(s, n);
    match s.kind {
        2 => if v >= 0.5 { "On" } else { "Off" }.to_string(),
        3 => s.choices.get(v as usize).copied().unwrap_or("").to_string(),
        _ => {
            let unit = match s.name {
                n if n.contains("Freq") || n.contains("Cut") || n == "Frequency" || n == "Tune" => " Hz",
                n if n.contains("Gain") || n == "Threshold" || n == "Makeup" => " dB",
                n if n.contains("Delay") || n.contains("Rate L") || n.contains("Rate R") || n.contains("Offset") || n == "Attack" && s.max > 10.0 || n == "Release" && s.max > 10.0 => " ms",
                "Time" | "Decay" => " s",
                _ => "",
            };
            if unit.is_empty() && s.min >= -1.0 && s.max <= 1.0 { format!("{:.0} %", v * 100.0) } else { format!("{v:.1}{unit}") }
        }
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn params() -> Vec<Param> {
    let mut v = Vec::new();
    for r in 0..RACKS {
        for s in 0..SLOTS {
            let n = format!("{} FX {}", RACK_NAMES[r], s + 1);
            v.push(Param::menu(id(r, s, 0), leak(format!("{n} Type")), TYPES, 0.0).hide());
            v.push(Param::new(id(r, s, 1), leak(format!("{n} On")), 0.0, 1.0, 1.0, Unit::OnOff).hide());
            for k in 0..KNOBS as u32 {
                v.push(Param::new(id(r, s, 2 + k), leak(format!("{n} Knob {}", k + 1)), 0.0, 1.0, 0.5, Unit::Ratio));
            }
        }
    }
    // where sources and main filters send to (F1/F2 or a main filter and an effects rack)
    for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
        v.push(Param::menu(SEND + s as u32, leak(format!("Send {l}")), SENDS, 0.0));
    }
    v.push(Param::menu(SEND + 4, "Filter 1 FX Rack", RACK_NAMES, 0.0));
    v.push(Param::menu(SEND + 5, "Filter 2 FX Rack", RACK_NAMES, 0.0));
    v
}

/// A source's Send pop-up (the F1/F2 knob balances its two destinations).
pub const SEND: u32 = 15020;
pub const SENDS: &[&str] = &["F1/F2", "F1/FX A", "F1/FX B", "F1/FX C", "F1/FX D", "F2/FX A", "F2/FX B", "F2/FX C", "F2/FX D"];

/// Studio's presets for an effect (File > Presets): name, real knob values by index.
pub fn presets(ty: usize) -> Vec<(&'static str, Vec<(usize, f32)>)> {
    let mut v: Vec<(&'static str, Vec<(usize, f32)>)> = vec![("Default", knobs(ty).iter().enumerate().map(|(i, s)| (i, s.def)).collect())];
    match ty {
        1 => v.extend([("Small Room", vec![(0, 0.6), (2, 0.25), (18, 0.25)]), ("Concert Hall", vec![(0, 2.8), (2, 0.8), (1, 30.0)]), ("Gated", vec![(0, 3.0), (5, 0.4), (7, 0.15)])]),
        2 => v.extend([("Room", vec![(4, 0.25)]), ("Big Space", vec![(4, 0.95), (0, 60.0)])]),
        3 => v.extend([("Plate Wash", vec![(0, 3.0), (2, 0.6)]), ("Cathedral", vec![(0, 5.0), (4, 1.5)])]),
        4 => v.extend([("Ping Pong", vec![(16, 1.0), (17, -1.0)]), ("Slapback", vec![(0, 0.0), (1, 110.0), (2, 110.0), (5, 0.1), (6, 0.1)])]),
        5 => v.extend([("Chorus", vec![(0, 20.0), (3, 0.3)]), ("Flanger", vec![(0, 2.0), (4, 0.7), (3, 0.6)])]),
        6 => v.extend([("Slow Sweep", vec![(1, 0.2), (5, 8.0)])]),
        16 => v.extend([("Crunch", vec![(2, 0.6), (4, 6.0)]), ("Lo-Fi", vec![(0, 0.7)])]),
        _ => {}
    }
    v
}

// ---------------------------------------------------------------- DSP

/// A delay line with fractional reads.
#[derive(Clone)]
struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    fn new(n: usize) -> Line {
        Line { buf: vec![0.0; n.max(4)], pos: 0 }
    }
    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = dsp::flush(x);
    }
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = d.clamp(1.0, (n - 2) as f32);
        let p = self.pos as f32 + n as f32 - d;
        let i = p as usize;
        let f = p - i as f32;
        let a = self.buf[i % n];
        let b = self.buf[(i + 1) % n];
        a + (b - a) * f
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

const FDN: usize = 8;
const FDN_LEN: [f32; FDN] = [0.0297, 0.0371, 0.0411, 0.0437, 0.0503, 0.0571, 0.0613, 0.0683];

/// One effect slot's running state (built for its type outside the audio thread).
pub struct Unit3 {
    pub ty: usize,
    lines: Vec<Line>,
    lp: [f32; FDN],
    hp: [f32; FDN],
    pre: [Line; 2],
    ap: Vec<Line>,
    bq: [[Biquad; 4]; 2],
    flt: [[AFilt; 2]; 2],
    op: [OnePole; 2],
    conv: Option<Box<[lg_fx::convolve::Convolver; 2]>>,
    conv_key: (usize, i32, i32, i32, bool, i32),
    phase: f32,
    env: f32,
    gate: f32,
    state: [f32; 8],
    aps: [[f32; 12]; 2],
    rate: f32,
}

impl Unit3 {
    pub fn new(rate: f32) -> Unit3 {
        Unit3 { ty: 0, lines: Vec::new(), lp: [0.0; FDN], hp: [0.0; FDN], pre: [Line::new(4), Line::new(4)], ap: Vec::new(), bq: [[Biquad::default(); 4]; 2], flt: Default::default(), op: [OnePole::default(); 2], conv: None, conv_key: (usize::MAX, 0, 0, 0, false, 0), phase: 0.0, env: 0.0, gate: 0.0, state: [0.0; 8], aps: [[0.0; 12]; 2], rate }
    }
    /// Gets ready for a type (allocates its delay lines); not for the audio thread.
    pub fn configure(&mut self, ty: usize) {
        if ty == self.ty {
            return;
        }
        self.ty = ty;
        let r = self.rate;
        self.lines.clear();
        self.ap.clear();
        self.conv = None;
        self.conv_key.0 = usize::MAX;
        match ty {
            1 | 2 => {
                for _ in 0..FDN {
                    self.lines.push(Line::new((r * 0.07 * 2.2) as usize + 8));
                }
                for n in [0.0047f32, 0.0036, 0.0127, 0.0093] {
                    self.ap.push(Line::new((r * n * 1.5) as usize + 4));
                }
                self.pre = [Line::new((r * 0.26) as usize), Line::new((r * 0.26) as usize)];
            }
            3 => {
                self.pre = [Line::new((r * 0.26) as usize), Line::new((r * 0.26) as usize)];
                self.conv = Some(Box::new([lg_fx::convolve::Convolver::new((r * 12.0) as usize), lg_fx::convolve::Convolver::new((r * 12.0) as usize)]));
            }
            4 => self.lines = vec![Line::new((r * 2.2) as usize), Line::new((r * 2.2) as usize)],
            5 => self.lines = vec![Line::new((r * 0.12) as usize), Line::new((r * 0.12) as usize)],
            _ => {}
        }
        self.reset();
    }
    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        for l in &mut self.ap {
            l.clear();
        }
        for l in &mut self.pre {
            l.clear();
        }
        if let Some(c) = &mut self.conv {
            c[0].clear();
            c[1].clear();
        }
        self.lp = [0.0; FDN];
        self.hp = [0.0; FDN];
        self.state = [0.0; 8];
        self.aps = [[0.0; 12]; 2];
        self.env = 0.0;
        self.gate = 0.0;
    }
    /// The Convolution Reverb's impulse responses from its knobs (not on the audio thread).
    pub fn load_ir(&mut self, v: &[f32], user: &[std::path::PathBuf]) {
        if self.ty != 3 {
            return;
        }
        let ks = knobs(3);
        let ir = scale(&ks[0], v[0]) as usize;
        let key = (ir, (scale(&ks[4], v[4]) * 100.0) as i32, (v[6] * 100.0) as i32, (v[7] * 100.0) as i32, scale(&ks[8], v[8]) >= 0.5, (v[5] * 100.0) as i32);
        if key == self.conv_key {
            return;
        }
        self.conv_key = key;
        let r = self.rate;
        let (mut l, mut rr) = if ir < SPACES.len() {
            synth_ir(ir, r)
        } else {
            match user.get(ir - SPACES.len()).and_then(|p| lg_fx::space_convolver::read_wav(p)) {
                Some((a, b, _)) => (a, b),
                None => synth_ir(4, r),
            }
        };
        // Size stretches it, Start / End trim it, Reverse flips it, Stereo narrows it
        let size = scale(&ks[4], v[4]);
        for ch in [&mut l, &mut rr] {
            if (size - 1.0).abs() > 0.01 {
                let n = ((ch.len() as f32 * size) as usize).max(2);
                let src = ch.clone();
                *ch = (0..n).map(|i| {
                    let p = i as f32 / size;
                    let k = (p as usize).min(src.len() - 2);
                    src[k] + (src[k + 1] - src[k]) * (p - k as f32)
                }).collect();
            }
            let n = ch.len();
            let a = ((v[6].clamp(0.0, 1.0)) * n as f32) as usize;
            let b = ((v[7].clamp(0.0, 1.0)) * n as f32).max(a as f32 + 2.0) as usize;
            *ch = ch[a.min(n - 1)..b.min(n)].to_vec();
            if key.4 {
                ch.reverse();
            }
        }
        let st = v[5].clamp(0.0, 1.0);
        let m: Vec<f32> = l.iter().zip(&rr).map(|(a, b)| (a + b) * 0.5).collect();
        for i in 0..l.len().min(rr.len()) {
            l[i] = m[i] + (l[i] - m[i]) * st;
            rr[i] = m[i] + (rr[i] - m[i]) * st;
        }
        if let Some(c) = &mut self.conv {
            c[0].load(&l);
            c[1].load(&rr);
        }
    }

    /// Processes a block in place; `v` are the slot's stored knob values (0..1).
    pub fn process(&mut self, v: &[f32; KNOBS], bpm: f32, l: &mut [f32], r: &mut [f32], reject: &mut ([f32; 64], [f32; 64], bool)) {
        let ks = knobs(self.ty);
        let mut q = [0.0f32; KNOBS];
        for (i, s) in ks.iter().enumerate() {
            q[i] = scale(s, v[i]);
        }
        let rate = self.rate;
        let n = l.len();
        match self.ty {
            1 | 2 => self.reverb(&q, l, r),
            3 => {
                let Some(c) = self.conv.as_mut() else { return };
                let pre = q[3] * 0.001 * rate;
                let filt = q[9] >= 0.5;
                for f in self.flt[0].iter_mut() {
                    f.set(q[10] as usize, af::cutoff_hz(q[11]), q[12], 0.0, rate);
                }
                for i in 0..n {
                    self.pre[0].push(l[i]);
                    self.pre[1].push(r[i]);
                    let (dl, dr) = if pre > 1.0 { (self.pre[0].read(pre), self.pre[1].read(pre)) } else { (l[i], r[i]) };
                    let mut wl = c[0].run(dl) * 0.3;
                    let mut wr = c[1].run(dr) * 0.3;
                    if filt {
                        wl = self.flt[0][0].run(wl);
                        wr = self.flt[0][1].run(wr);
                    }
                    l[i] = l[i] * q[1] + wl * q[2];
                    r[i] = r[i] * q[1] + wr * q[2];
                }
            }
            4 => {
                let sync = q[0] >= 0.5;
                let t = |k: usize, nrm: f32| -> f32 {
                    if sync {
                        let d = ((nrm * (DIVS.len() - 1) as f32).round() as usize).min(DIVS.len() - 1);
                        crate::alchemy_mod::SYNC_BEATS[d] * 60.0 / bpm.max(1.0)
                    } else {
                        q[k] * 0.001
                    }
                };
                let dl = ((t(1, v[1]) + q[3] * 0.001) * rate).max(1.0);
                let dr = ((t(2, v[2]) + q[4] * 0.001) * rate).max(1.0);
                for (j, (on, ty, c, res)) in [(7usize, 8usize, 9usize, 10usize), (11, 12, 13, 14)].iter().enumerate() {
                    if q[*on] >= 0.5 {
                        for f in self.flt[j].iter_mut() {
                            f.set(q[*ty] as usize, af::cutoff_hz(q[*c]), q[*res], 0.0, rate);
                        }
                    }
                }
                let (fa, fb) = (q[7] >= 0.5, q[11] >= 0.5);
                let x = q[16];
                let pan = q[17];
                let (pl, pr) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
                for i in 0..n {
                    let (il, ir) = if q[15] >= 0.5 { let m = (l[i] + r[i]) * 0.5; (m, m) } else { (l[i], r[i]) };
                    let mut yl = self.lines[0].read(dl);
                    let mut yr = self.lines[1].read(dr);
                    for (j, on) in [fa, fb].iter().enumerate() {
                        if *on {
                            yl = self.flt[j][0].run(yl);
                            yr = self.flt[j][1].run(yr);
                        }
                    }
                    // Crossover: 0 % same side, 100 % swapped
                    let fl = yl * (1.0 - x) + yr * x;
                    let fr = yr * (1.0 - x) + yl * x;
                    self.lines[0].push(il * pl + fl * q[5]);
                    self.lines[1].push(ir * pr + fr * q[6]);
                    l[i] = l[i] * (1.0 - q[18]) + yl * q[18];
                    r[i] = r[i] * (1.0 - q[18]) + yr * q[18];
                }
            }
            5 => {
                let base = q[0] * 0.001 * rate;
                for i in 0..n {
                    self.phase = (self.phase + q[1] / rate).fract();
                    let m0 = dsp::sin_cycles(self.phase);
                    let m1 = dsp::sin_cycles(self.phase + 0.25 * q[5]);
                    let d0 = base * (1.0 + q[3] * 0.9 * m0) + 1.0;
                    let d1 = base * (1.0 + q[3] * 0.9 * m1) + 1.0;
                    let yl = self.lines[0].read(d0);
                    let yr = self.lines[1].read(d1);
                    self.lines[0].push(l[i] + yl * q[4]);
                    self.lines[1].push(r[i] + yr * q[4]);
                    l[i] = l[i] * (1.0 - q[2] * 0.5) + yl * q[2];
                    r[i] = r[i] * (1.0 - q[2] * 0.5) + yr * q[2];
                }
            }
            6 => {
                let hz = if q[0] >= 0.5 { bpm / 60.0 / crate::alchemy_mod::SYNC_BEATS[((v[1] * (DIVS.len() - 1) as f32).round() as usize).min(DIVS.len() - 1)] } else { q[1] };
                let poles = (q[5].round() as usize).clamp(2, 12);
                for i in 0..n {
                    self.phase = (self.phase + hz / rate).fract();
                    for (c, x) in [&mut *l, &mut *r].into_iter().enumerate() {
                        let m = dsp::sin_cycles(self.phase + c as f32 * q[3] * 0.5);
                        let f = (q[4] * 2f32.powf(q[2] * 2.0 * m)).clamp(20.0, rate * 0.45);
                        let t = (std::f32::consts::PI * f / rate).tan();
                        let a = (1.0 - t) / (1.0 + t);
                        let mut y = x[i] + self.state[6 + c] * q[6];
                        for p in 0..poles {
                            // first-order allpasses (state per pole and channel)
                            let s = &mut self.aps[c][p];
                            let o = -a * y + *s;
                            *s = dsp::flush(y + a * o);
                            y = o;
                        }
                        self.state[6 + c] = y;
                        x[i] = x[i] * (1.0 - q[7] * 0.5) + y * q[7];
                    }
                }
            }
            7 => {
                for i in 0..n {
                    l[i] *= q[0];
                    r[i] *= q[0];
                }
            }
            8 => {
                let p = q[0] * 2.0 - 1.0;
                let (gl, gr) = ((1.0 - p).min(1.0), (1.0 + p).min(1.0));
                for i in 0..n {
                    l[i] *= gl;
                    r[i] *= gr;
                }
            }
            9 | 10 => {
                let (att, rel, thr, ing, ratio, make) = if self.ty == 9 {
                    (0.005, q[1], lg_dsp::db_to_gain(-6.0 - q[0] * 30.0), 1.0, 2.0 + q[0] * 6.0, lg_dsp::db_to_gain(q[0] * 14.0))
                } else {
                    (q[0] * 0.001, q[1] * 0.001, lg_dsp::db_to_gain(q[2]), lg_dsp::db_to_gain(q[3]), q[4], lg_dsp::db_to_gain(q[5]))
                };
                let (ka, kr) = (1.0 - (-1.0 / (att.max(1e-4) * rate)).exp(), 1.0 - (-1.0 / (rel.max(1e-3) * rate)).exp());
                let phat = self.ty == 9 && q[2] >= 0.5;
                for i in 0..n {
                    let (xl, xr) = (l[i] * ing, r[i] * ing);
                    let lv = xl.abs().max(xr.abs());
                    self.env += (lv - self.env) * if lv > self.env { ka } else { kr };
                    let g = if self.env > thr { (thr / self.env).powf(1.0 - 1.0 / ratio.max(1.0)) } else { 1.0 } * make;
                    let (mut yl, mut yr) = (xl * g, xr * g);
                    if phat {
                        yl = dsp::soft_clip(yl * 1.5) / 1.2;
                        yr = dsp::soft_clip(yr * 1.5) / 1.2;
                    }
                    l[i] = yl;
                    r[i] = yr;
                }
            }
            11 => {
                for c in 0..2 {
                    self.bq[c][0].highpass(q[0], 0.707 + q[2] * 6.0, rate);
                    self.bq[c][1].lowpass(q[1], 0.707 + q[3] * 6.0, rate);
                }
                for i in 0..n.min(64) {
                    let (ol, or) = (l[i], r[i]);
                    let t0 = self.bq[0][0].run(l[i]);
                    l[i] = self.bq[0][1].run(t0);
                    let t1 = self.bq[1][0].run(r[i]);
                    r[i] = self.bq[1][1].run(t1);
                    // what the band pass took out, for a Band Reject later in the rack
                    reject.0[i] = ol - l[i];
                    reject.1[i] = or - r[i];
                }
                reject.2 = true;
            }
            12 => {
                if reject.2 {
                    for i in 0..n.min(64) {
                        l[i] += reject.0[i] * q[0];
                        r[i] += reject.1[i] * q[0];
                    }
                }
            }
            13 => {
                for f in self.flt[0].iter_mut() {
                    f.set(q[0] as usize, af::cutoff_hz(q[1]), q[2], q[3], rate);
                }
                for i in 0..n {
                    let (a, b) = (self.flt[0][0].run(l[i]), self.flt[0][1].run(r[i]));
                    l[i] += (a - l[i]) * q[4];
                    r[i] += (b - r[i]) * q[4];
                }
            }
            14 => {
                for c in 0..2 {
                    for b in 0..3 {
                        self.bq[c][b].peaking(q[b * 3 + 1].min(rate * 0.45), q[b * 3 + 2], q[b * 3], rate);
                    }
                }
                for i in 0..n {
                    for b in 0..3 {
                        l[i] = self.bq[0][b].run(l[i]);
                        r[i] = self.bq[1][b].run(r[i]);
                    }
                }
            }
            15 => {
                for c in 0..2 {
                    self.bq[c][0].bandpass(q[1], 1.2, rate);
                }
                for i in 0..n {
                    for (c, x) in [&mut *l, &mut *r].into_iter().enumerate() {
                        let mut b = self.bq[c][0].run(x[i]) * q[0] * 3.0;
                        if q[2] >= 0.5 {
                            b = dsp::soft_clip(b * 2.0);
                        }
                        x[i] += b;
                    }
                }
            }
            16 => {
                let pre = lg_dsp::db_to_gain(q[4]);
                let post = lg_dsp::db_to_gain(q[5]);
                for x in [&mut *l, &mut *r] {
                    for s in x.iter_mut() {
                        let mut y = *s * pre;
                        // the four distortions together
                        if q[0] > 0.0 {
                            let bits = 16.0 - q[0] * 13.0;
                            let k = 2f32.powf(bits - 1.0);
                            y = (y.clamp(-1.0, 1.0) * k).round() / k;
                        }
                        if q[1] > 0.0 {
                            y = (y * (1.0 + q[1] * 8.0) * std::f32::consts::FRAC_PI_2).sin();
                        }
                        if q[2] > 0.0 {
                            let g = 1.0 + q[2] * 8.0;
                            y = dsp::fast_tanh(y * g + 0.2 * q[2]) - dsp::fast_tanh(0.2 * q[2]);
                        }
                        if q[3] > 0.0 {
                            // XCite: the highs driven hard and added
                            let hi = y - self.state[7];
                            self.state[7] += (y - self.state[7]) * 0.2;
                            y += dsp::soft_clip(hi * (1.0 + q[3] * 10.0)) * q[3] * 0.5;
                        }
                        *s = y * post;
                    }
                }
            }
            17 => {
                let pre = lg_dsp::db_to_gain(q[1]);
                let post = lg_dsp::db_to_gain(q[2]);
                let bip = q[5] >= 0.5;
                for c in 0..2 {
                    self.op[c].lowpass_hz(q[4], rate);
                }
                for (c, x) in [&mut *l, &mut *r].into_iter().enumerate() {
                    for s in x.iter_mut() {
                        // Shape: low pass (0) … high pass (1) before the curve
                        let lo = self.op[c].lp(*s);
                        let hi = *s - lo;
                        let tilt = lo * (1.0 - q[3]) * 2.0 + hi * q[3] * 2.0;
                        let inp = (tilt.min(1.0).max(-1.0) * pre).clamp(-1.0, 1.0);
                        let y = shaper_curve(&q, bip, inp);
                        *s = *s + (y * post - *s) * q[0];
                    }
                }
            }
            _ => {}
        }
    }

    fn reverb(&mut self, q: &[f32; KNOBS], l: &mut [f32], r: &mut [f32]) {
        let rate = self.rate;
        let classic = self.ty == 2;
        let (time, pre, size, width, diff, mix) = if classic {
            (0.4 + q[4] * 6.0, q[0], q[4], 1.0, 0.7, q[5])
        } else {
            (q[0], q[1], q[2], q[3], q[4], q[18])
        };
        let scale = 0.4 + size * 1.8;
        let var = if classic { 0.5 } else { q[16] };
        let lines = if !classic && q[17] < 0.5 { 4 } else { FDN };
        let mut lens = [0.0f32; FDN];
        let mut gains = [0.0f32; FDN];
        for k in 0..FDN {
            lens[k] = FDN_LEN[k] * scale * (1.0 + (var - 0.5) * 0.2 * ((k as f32 * 1.7).sin())) * rate;
            gains[k] = 10f32.powf(-3.0 * lens[k] / (time.max(0.05) * rate));
        }
        let (hi_k, lo_k) = if classic {
            (1.0 - (-std::f32::consts::TAU * q[2] / rate).exp(), 1.0 - (-std::f32::consts::TAU * q[3] / rate).exp())
        } else {
            (1.0 - (-std::f32::consts::TAU * q[14] / rate).exp(), 1.0 - (-std::f32::consts::TAU * q[12] / rate).exp())
        };
        let hi_damp = if classic { q[1] } else { q[15] };
        let lo_damp = if classic { 0.0 } else { q[13] };
        if !classic {
            for c in 0..2 {
                self.bq[c][0].peaking(q[8], 0.5, q[9], rate);
                self.bq[c][1].peaking(q[10], 0.5, q[11], rate);
            }
        } else {
            for c in 0..2 {
                self.bq[c][0].highpass(q[3], 0.707, rate);
                self.bq[c][1].lowpass(q[2], 0.707, rate);
            }
        }
        let pre_s = pre * 0.001 * rate;
        let ap_g = 0.3 + diff * 0.45;
        let ap_len = [0.0047f32, 0.0036, 0.0127, 0.0093].map(|x| x * rate * (0.7 + size * 0.6));
        let (gate_thr, g_att, g_dec) = (q[5] * 0.3, (1.0 / (q[6].max(0.001) * rate)).min(1.0), (1.0 / (q[7].max(0.001) * rate)).min(1.0));
        for i in 0..l.len() {
            self.pre[0].push(l[i]);
            self.pre[1].push(r[i]);
            let (mut xl, mut xr) = if pre_s > 1.0 { (self.pre[0].read(pre_s), self.pre[1].read(pre_s)) } else { (l[i], r[i]) };
            // diffusion: allpasses on the way in
            for (k, a) in self.ap.iter_mut().enumerate() {
                let x = if k % 2 == 0 { &mut xl } else { &mut xr };
                let d = a.read(ap_len[k]);
                let y = -ap_g * *x + d;
                a.push(*x + ap_g * y);
                *x = y;
            }
            // the feedback network (Householder mix)
            let mut outs = [0.0f32; FDN];
            let mut sum = 0.0;
            for k in 0..lines {
                outs[k] = self.lines[k].read(lens[k]);
                sum += outs[k];
            }
            let mix_k = 2.0 / lines as f32;
            let (mut wl, mut wr) = (0.0, 0.0);
            for k in 0..lines {
                let mut y = (outs[k] - sum * mix_k) * gains[k];
                // high and low damping in the loop
                self.lp[k] += (y - self.lp[k]) * (hi_k + (1.0 - hi_k) * (1.0 - hi_damp));
                y = self.lp[k];
                self.hp[k] += (y - self.hp[k]) * lo_k;
                y -= self.hp[k] * lo_damp;
                let inp = if k % 2 == 0 { xl } else { xr };
                self.lines[k].push(inp + y);
                if k % 2 == 0 { wl += outs[k] } else { wr += outs[k] }
            }
            let m = (wl + wr) * 0.5;
            wl = m + (wl - m) * width;
            wr = m + (wr - m) * width;
            let t0 = self.bq[0][0].run(wl);
            wl = self.bq[0][1].run(t0) * 0.35;
            let t1 = self.bq[1][0].run(wr);
            wr = self.bq[1][1].run(t1) * 0.35;
            if !classic && gate_thr > 0.0 {
                // Gate: the tail stops when the input falls under the threshold
                let lv = l[i].abs().max(r[i].abs());
                self.env += (lv - self.env) * 0.01;
                let open = (self.env > gate_thr) as u8 as f32;
                self.gate += (open - self.gate) * if open > self.gate { g_att } else { g_dec };
                wl *= self.gate;
                wr *= self.gate;
            }
            l[i] = l[i] * (1.0 - mix) + wl * mix;
            r[i] = r[i] * (1.0 - mix) + wr * mix;
        }
    }
}

/// The Waveshaper's curve: start → mid → end handles, two bendable segments; unipolar folds the
/// negative half onto the positive.
pub fn shaper_curve(q: &[f32; KNOBS], bipolar: bool, x: f32) -> f32 {
    let (y0, mx, my, y1, c1, c2) = (q[6], q[7], q[8], q[9], q[10], q[11]);
    let u = if bipolar { (x + 1.0) * 0.5 } else { x.abs() };
    let y = if u < mx {
        crate::alchemy_mod::seg(y0, my, u / mx.max(1e-3), c1)
    } else {
        crate::alchemy_mod::seg(my, y1, (u - mx) / (1.0 - mx).max(1e-3), c2)
    };
    if bipolar { y } else { y * x.signum() }
}

/// A synthesized impulse response (left, right): decaying noise with the space's character.
pub fn synth_ir(space: usize, rate: f32) -> (Vec<f32>, Vec<f32>) {
    let (t60, bright, early, ring): (f32, f32, f32, f32) = match space {
        0 => (0.5, 0.6, 0.01, 0.0),
        1 => (0.8, 0.5, 0.012, 0.0),
        2 => (1.4, 0.45, 0.02, 0.0),
        3 => (2.2, 0.85, 0.0, 0.0),
        4 => (2.8, 0.4, 0.03, 0.0),
        5 => (5.5, 0.3, 0.05, 0.0),
        6 => (1.8, 0.7, 0.0, 1.0),
        _ => (0.4, 0.7, 0.0, 0.0),
    };
    let n = (rate * (t60 * 1.2 + 0.1)) as usize;
    let mut out = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut s = 0x1234_5678u32 + space as u32 * 977;
    let mut lp = [0.0f32; 2];
    for i in 0..n {
        let t = i as f32 / rate;
        let env = if space == 7 {
            // nonlinear: flat then cut
            if t < 0.25 { 1.0 } else { 0.0 }
        } else {
            10f32.powf(-3.0 * t / t60)
        };
        let k = 0.05 + bright * (1.0 - (t / t60).min(1.0)) * 0.9;
        for c in 0..2 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let w = (s & 0xFFFF) as f32 / 32768.0 - 1.0;
            lp[c] += (w - lp[c]) * k;
            let mut y = lp[c] * env;
            if ring > 0.0 {
                y *= 1.0 + 0.6 * (std::f32::consts::TAU * 3000.0 * t + c as f32).sin();
            }
            if early > 0.0 && t < early * 3.0 && (i % ((early * rate * 0.37) as usize + 7 + c * 3)) == 0 {
                y += 0.6 * env;
            }
            if c == 0 { out.0[i] = y } else { out.1[i] = y }
        }
    }
    // normalised to a sensible energy
    let e: f32 = out.0.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    for x in out.0.iter_mut().chain(out.1.iter_mut()) {
        *x /= e;
    }
    out
}

/// The five racks and their running effects.
pub struct Racks {
    pub units: Vec<Unit3>,
    pub vals: [[f32; KNOBS]; RACKS * SLOTS],
    pub on: [bool; RACKS * SLOTS],
    pub used: [bool; RACKS],
    reject: ([f32; 64], [f32; 64], bool),
}

impl Racks {
    pub fn new(rate: f32) -> Racks {
        Racks { units: (0..RACKS * SLOTS).map(|_| Unit3::new(rate)).collect(), vals: [[0.5; KNOBS]; RACKS * SLOTS], on: [true; RACKS * SLOTS], used: [false; RACKS], reject: ([0.0; 64], [0.0; 64], false) }
    }
    /// Reads a slot (and builds its effect) after a parameter change: not for the audio thread.
    pub fn read(&mut self, p: &dyn Fn(u32) -> f32, rack: usize, slot: usize) {
        let i = rack * SLOTS + slot;
        let ty = (p(id(rack, slot, 0)).round().max(0.0) as usize).min(TYPES.len() - 1);
        self.units[i].configure(ty);
        self.on[i] = p(id(rack, slot, 1)) >= 0.5;
        for k in 0..KNOBS {
            self.vals[i][k] = p(id(rack, slot, 2 + k as u32));
        }
        if ty == 3 {
            let user = lg_fx::space_convolver::user_irs();
            let v = self.vals[i];
            self.units[i].load_ir(&v, &user);
        }
        self.used[rack] = (0..SLOTS).any(|s| self.units[rack * SLOTS + s].ty != 0);
    }
    pub fn read_all(&mut self, p: &dyn Fn(u32) -> f32) {
        for r in 0..RACKS {
            for s in 0..SLOTS {
                self.read(p, r, s);
            }
        }
    }
    pub fn read_changed(&mut self, id_: u32, p: &dyn Fn(u32) -> f32) -> bool {
        if !(FXR..FXR + RACKS as u32 * 256).contains(&id_) {
            return false;
        }
        let k = id_ - FXR;
        let (r, s) = ((k / 256) as usize, ((k % 256) / 24) as usize);
        if s < SLOTS {
            self.read(p, r, s);
        }
        true
    }
    /// A rack over a block (in place), top to bottom.
    pub fn process(&mut self, rack: usize, bpm: f32, l: &mut [f32], r: &mut [f32]) {
        self.reject.2 = false;
        for s in 0..SLOTS {
            let i = rack * SLOTS + s;
            if self.units[i].ty == 0 || !self.on[i] {
                continue;
            }
            let v = self.vals[i];
            self.units[i].process(&v, bpm, l, r, &mut self.reject);
        }
    }
    pub fn reset(&mut self) {
        for u in &mut self.units {
            u.reset();
        }
    }
    pub fn tail(&self) -> bool {
        self.units.iter().any(|u| matches!(u.ty, 1..=6))
    }
}
