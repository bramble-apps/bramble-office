//! Ultrabeat stand-in: a drum machine of 25 independent drum synthesizers (notes C1 … B2 play
//! drum sounds 1–24, sound 25 plays chromatically from C3 up) with a built-in 32-step sequencer
//! (24 patterns, each holding a sequence for every sound).
//!
//! Every sound has its own oscillator 1 (phase oscillator, FM, side chain), oscillator 2
//! (phase oscillator, sample, modelled string), ring modulator, noise generator, filter with a
//! distortion circuit, two-band EQ, pan modulation or stereo spread, four envelopes, two LFOs,
//! mod / via routing and trigger settings (see `ultrabeat_dsp`); the mixer (volume, pan, mute,
//! solo, output) sits behind each sound. The sequencer follows the host's play position.
//!
//! Parameters: ids below 100 are global; sound `v` (0…24) has ids `vid(v, k)` with `k` below
//! 256 (the numbers below); the step data of every cell lives in ids from `SEQ_BASE` (see
//! `ultrabeat_seq::Cell`, 64 words per cell) and is only listed once a cell is touched.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::dsp;
use crate::synthkit::{self as kit, SideBuf};
pub use crate::ultrabeat_dsp::Sample;
use crate::ultrabeat_dsp::{Gl, Inst, CH};
pub use crate::ultrabeat_seq::Cell;
use lg_dsp::{Instrument, Param, Plugin, Transport, Unit};

// ---- global parameters
pub const G_MASTER: u32 = 0;
pub const G_SEL: u32 = 1;
pub const G_PAT: u32 = 2;
pub const G_SEQ_ON: u32 = 3;
pub const G_SWING: u32 = 6;
pub const G_ACCENT: u32 = 7;
pub const G_PMODE: u32 = 8;
pub const G_PBMODE: u32 = 9;
pub const G_MUTEMODE: u32 = 10;
pub const G_AUTOSEL: u32 = 11;
pub const G_EDIT: u32 = 12;
pub const G_FULL: u32 = 13;
/// 14…17: the controller numbers of Ctrl A…D.
pub const G_CTRL: u32 = 14;
/// 18…21: their values.
pub const G_CV: u32 = 18;
pub const G_RUN: u32 = 22;

pub const VBASE: u32 = 1000;
pub const VSTRIDE: u32 = 256;
pub const SEQ_BASE: u32 = 100_000;
/// Pattern length and resolution of pattern `p`: `PAT_BASE + 2p` and `+ 1`.
pub const PAT_BASE: u32 = SEQ_BASE + 24 * 25 * 64;

pub fn vid(v: usize, k: u32) -> u32 {
    VBASE + v as u32 * VSTRIDE + k
}
pub fn cell_id(pat: usize, v: usize, w: usize) -> u32 {
    SEQ_BASE + ((pat * 25 + v) * 64 + w) as u32
}
pub fn pat_len_id(p: usize) -> u32 {
    PAT_BASE + 2 * p as u32
}
pub fn pat_res_id(p: usize) -> u32 {
    PAT_BASE + 2 * p as u32 + 1
}

// ---- per-sound parameters (k)
pub const O1_ON: u32 = 0;
pub const O1_MODE: u32 = 1;
pub const O1_PITCH: u32 = 2;
pub const O1_VOL: u32 = 3;
pub const O1_SLOPE: u32 = 4;
pub const O1_SAT: u32 = 5;
pub const O1_ASYM: u32 = 6;
pub const O1_FM: u32 = 7;
pub const O1_FLOW: u32 = 8;
pub const O2_ON: u32 = 9;
pub const O2_MODE: u32 = 10;
pub const O2_PITCH: u32 = 11;
pub const O2_VOL: u32 = 12;
pub const O2_SLOPE: u32 = 13;
pub const O2_SAT: u32 = 14;
pub const O2_ASYM: u32 = 15;
pub const O2_FLOW: u32 = 16;
pub const S_MIN: u32 = 17;
pub const S_MAX: u32 = 18;
pub const S_REV: u32 = 19;
pub const M_EXC: u32 = 20;
pub const M_STIFF: u32 = 21;
pub const M_LOSS: u32 = 22;
pub const M_RES: u32 = 23;
pub const RM_ON: u32 = 24;
pub const RM_VOL: u32 = 25;
pub const RM_FLOW: u32 = 26;
pub const NZ_ON: u32 = 27;
pub const NZ_TYPE: u32 = 28;
pub const NZ_CUT: u32 = 29;
pub const NZ_RES: u32 = 30;
pub const NZ_DIRT: u32 = 31;
pub const NZ_VOL: u32 = 32;
pub const NZ_FLOW: u32 = 33;
pub const F_ON: u32 = 34;
pub const F_TYPE: u32 = 35;
pub const F_SLOPE: u32 = 36;
pub const F_CUT: u32 = 37;
pub const F_RES: u32 = 38;
pub const D_MODE: u32 = 39;
pub const D_DRIVE: u32 = 40;
pub const D_COLOR: u32 = 41;
pub const D_LEVEL: u32 = 42;
pub const D_POS: u32 = 43;
/// Five numbers a band: on, type, frequency, Q, gain.
pub const EQ1_ON: u32 = 44;
pub const EQ1_TYPE: u32 = 45;
pub const EQ1_FREQ: u32 = 46;
pub const EQ1_Q: u32 = 47;
pub const EQ1_GAIN: u32 = 48;
pub const EQ2_ON: u32 = 49;
pub const EQ2_TYPE: u32 = 50;
pub const EQ2_FREQ: u32 = 51;
pub const EQ2_Q: u32 = 52;
pub const EQ2_GAIN: u32 = 53;
pub const PAN_MODE: u32 = 54;
pub const SPREAD_LO: u32 = 55;
pub const SPREAD_HI: u32 = 56;
pub const VOICE_VOL: u32 = 57;
pub const TRIG: u32 = 58;
pub const GROUP: u32 = 59;
pub const GATE: u32 = 60;
pub const SWING_EN: u32 = 61;
pub const ACC_EN: u32 = 62;
pub const MIX_VOL: u32 = 63;
pub const MIX_PAN: u32 = 64;
pub const MUTE: u32 = 65;
pub const SOLO: u32 = 66;
pub const OUTPUT: u32 = 67;
/// 68…71: the name, three characters a number.
pub const NAME0: u32 = 68;
/// Mod / via blocks, four numbers a target (source, amount, via, via amount).
pub const MOD0: u32 = 72;
/// Eight numbers an envelope: A, A shape, D, D shape, sustain, sustain point, target, amount.
pub const ENV0: u32 = 160;
/// Six numbers an LFO: on, sync, rate, waveform, cycles, ramp.
pub const LFO0: u32 = 192;

// ---- modulation targets
pub const T_O1_PITCH: usize = 0;
pub const T_O1_VOL: usize = 1;
pub const T_O1_FM: usize = 2;
pub const T_O1_ASYM: usize = 3;
pub const T_O1_SLOPE: usize = 4;
pub const T_O1_SAT: usize = 5;
pub const T_O2_PITCH: usize = 6;
pub const T_O2_VOL: usize = 7;
pub const T_O2_SLOPE: usize = 8;
pub const T_O2_SAT: usize = 9;
pub const T_O2_ASYM: usize = 10;
pub const T_RM_VOL: usize = 11;
pub const T_NZ_CUT: usize = 12;
pub const T_NZ_RES: usize = 13;
pub const T_NZ_DIRT: usize = 14;
pub const T_NZ_VOL: usize = 15;
pub const T_F_CUT: usize = 16;
pub const T_F_RES: usize = 17;
pub const T_D_DRIVE: usize = 18;
pub const T_PAN: usize = 19;
pub const T_VOICE_VOL: usize = 20;
pub const T_COUNT: usize = 21;
/// The parameter each target modulates.
pub const TARGET_K: [u32; T_COUNT] = [O1_PITCH, O1_VOL, O1_FM, O1_ASYM, O1_SLOPE, O1_SAT, O2_PITCH, O2_VOL, O2_SLOPE, O2_SAT, O2_ASYM, RM_VOL, NZ_CUT, NZ_RES, NZ_DIRT, NZ_VOL, F_CUT, F_RES, D_DRIVE, MIX_PAN, VOICE_VOL];
pub const TARGET_NAMES: [&str; T_COUNT] = [
    "Osc 1 Pitch",
    "Osc 1 Volume",
    "Osc 1 FM Amount",
    "Osc 1 Asym",
    "Osc 1 Slope",
    "Osc 1 Saturation",
    "Osc 2 Pitch",
    "Osc 2 Volume",
    "Osc 2 Slope",
    "Osc 2 Saturation",
    "Osc 2 Asym",
    "Ring Mod Level",
    "Noise Cutoff",
    "Noise Resonance",
    "Noise Dirt",
    "Noise Volume",
    "Filter Cutoff",
    "Filter Resonance",
    "Distortion Drive",
    "Pan Modulation",
    "Voice Volume",
];
/// How far a modulation of depth 1 reaches, in the parameter's own unit.
pub fn target_span(t: usize) -> f32 {
    match t {
        T_O1_PITCH | T_O2_PITCH => 96.0,
        T_O1_VOL | T_O2_VOL | T_RM_VOL | T_NZ_VOL => 60.0,
        T_O1_SLOPE | T_O2_SLOPE => 2.0,
        _ => 1.0,
    }
}
/// Part 0 source, 1 amount, 2 via, 3 via amount.
pub fn mod_k(t: usize, part: u32) -> u32 {
    MOD0 + 4 * t as u32 + part
}

pub const MOD_SRC: &[&str] = &["Off", "Lfo1", "Lfo2", "Env1", "Env2", "Env3", "Env4", "Max"];
pub const VIA_SRC: &[&str] = &["Off", "Vel", "CtrlA", "CtrlB", "CtrlC", "CtrlD"];
pub const O1_MODES: &[&str] = &["Phase Osc", "FM", "Side Chain"];
pub const O2_MODES: &[&str] = &["Phase Osc", "Sample", "Model"];
pub const NZ_TYPES: &[&str] = &["LP", "HP", "BP", "byp"];
pub const F_TYPES: &[&str] = &["LP", "HP", "BP", "BR"];
pub const F_SLOPES: &[&str] = &["12 dB", "24 dB"];
pub const D_MODES: &[&str] = &["Off", "Crush", "Distort"];
pub const EQ_TYPES: &[&str] = &["Shelf", "Peak"];
pub const PAN_MODES: &[&str] = &["Off", "Pan Mod", "Spread"];
pub const TRIGS: &[&str] = &["Multi", "Single"];
pub const GROUPS: &[&str] = &["Off", "1", "2", "3", "4", "5", "6", "7", "8"];
pub const EXCITERS: &[&str] = &["Exciter 1", "Exciter 2"];
pub const ENV_TARGETS: &[&str] = &["A Time", "A Shape", "D Time", "D Shape"];
pub const OUTPUTS: &[&str] = &["Main (1-2)", "3-4", "5-6", "7-8", "9-10", "11-12", "13-14", "15-16", "Mono 1", "Mono 2", "Mono 3", "Mono 4", "Mono 5", "Mono 6", "Mono 7", "Mono 8"];
pub const RESOLUTIONS: &[&str] = &["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"];
pub const RES_BEATS: [f64; 6] = [1.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125];
pub const PLAYBACK_MODES: &[&str] = &["One-Shot Trig", "Sustain", "Toggle", "Toggle on Step 1"];
pub const PATTERNS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24"];
pub const EDIT_MODES: &[&str] = &["Voice", "Step"];

/// The Logic command names this instrument really carries out (window menus).
pub const COMMANDS: &[&str] = &["Copy (Voice & Seq)", "Paste Voice", "Paste Sequence", "Swap with Clipboard", "Init", "Copy Pattern", "Paste Pattern", "Clear Pattern"];

/// The tempo-synced LFO lengths, slowest first: (name, beats).
pub fn sync_list() -> &'static [(String, f32)] {
    static L: OnceLock<Vec<(String, f32)>> = OnceLock::new();
    L.get_or_init(|| {
        let mut v = Vec::new();
        for (n, beats) in [("32 bars", 128.0f32), ("16 bars", 64.0), ("8 bars", 32.0), ("4 bars", 16.0), ("2 bars", 8.0), ("1 bar", 4.0), ("1/2", 2.0), ("1/4", 1.0), ("1/8", 0.5), ("1/16", 0.25), ("1/32", 0.125), ("1/64", 0.0625)] {
            if beats >= 4.0 {
                v.push((n.to_string(), beats));
            } else {
                v.push((format!("{n} d"), beats * 1.5));
                v.push((n.to_string(), beats));
                v.push((format!("{n} t"), beats * 2.0 / 3.0));
            }
        }
        v
    })
}

// ---- the parameter table

#[derive(Clone, Copy)]
pub struct VSpec {
    pub k: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub unit: Unit,
    pub choices: &'static [&'static str],
}

pub fn vspecs() -> &'static [VSpec] {
    static S: OnceLock<Vec<VSpec>> = OnceLock::new();
    S.get_or_init(build_specs)
}

pub fn vspec(k: u32) -> Option<&'static VSpec> {
    static IDX: OnceLock<Vec<Option<usize>>> = OnceLock::new();
    let idx = IDX.get_or_init(|| {
        let mut v = vec![None; 256];
        for (i, s) in vspecs().iter().enumerate() {
            v[s.k as usize] = Some(i);
        }
        v
    });
    idx.get(k as usize).copied().flatten().map(|i| &vspecs()[i])
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn build_specs() -> Vec<VSpec> {
    let mut v: Vec<VSpec> = Vec::new();
    macro_rules! n {
        ($k:expr, $name:expr, $min:expr, $max:expr, $def:expr, $u:expr) => {
            v.push(VSpec { k: $k, name: $name, min: $min, max: $max, def: $def, unit: $u, choices: &[] })
        };
    }
    macro_rules! m {
        ($k:expr, $name:expr, $ch:expr, $def:expr) => {
            v.push(VSpec { k: $k, name: $name, min: 0.0, max: ($ch.len() as f32) - 1.0, def: $def, unit: Unit::Menu, choices: $ch })
        };
    }
    n!(O1_ON, "Osc 1 On", 0.0, 1.0, 1.0, Unit::OnOff);
    m!(O1_MODE, "Osc 1 Mode", O1_MODES, 0.0);
    n!(O1_PITCH, "Osc 1 Pitch", -48.0, 48.0, 0.0, Unit::Semitones);
    n!(O1_VOL, "Osc 1 Volume", -60.0, 0.0, 0.0, Unit::Db);
    n!(O1_SLOPE, "Osc 1 Slope", -1.0, 1.0, 0.0, Unit::None);
    n!(O1_SAT, "Osc 1 Saturation", 0.0, 1.0, 0.0, Unit::None);
    n!(O1_ASYM, "Osc 1 Asym", 0.0, 1.0, 0.0, Unit::None);
    n!(O1_FM, "Osc 1 FM Amount", 0.0, 1.0, 0.0, Unit::None);
    n!(O1_FLOW, "Osc 1 Signal Flow", 0.0, 1.0, 1.0, Unit::OnOff);
    n!(O2_ON, "Osc 2 On", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(O2_MODE, "Osc 2 Mode", O2_MODES, 0.0);
    n!(O2_PITCH, "Osc 2 Pitch", -48.0, 48.0, 0.0, Unit::Semitones);
    n!(O2_VOL, "Osc 2 Volume", -60.0, 0.0, 0.0, Unit::Db);
    n!(O2_SLOPE, "Osc 2 Slope", -1.0, 1.0, 0.0, Unit::None);
    n!(O2_SAT, "Osc 2 Saturation", 0.0, 1.0, 0.0, Unit::None);
    n!(O2_ASYM, "Osc 2 Asym", 0.0, 1.0, 0.0, Unit::None);
    n!(O2_FLOW, "Osc 2 Signal Flow", 0.0, 1.0, 1.0, Unit::OnOff);
    n!(S_MIN, "Sample Start Min", 0.0, 1.0, 0.0, Unit::None);
    n!(S_MAX, "Sample Start Max", 0.0, 1.0, 0.0, Unit::None);
    n!(S_REV, "Sample Reverse", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(M_EXC, "Model Exciter", EXCITERS, 0.0);
    n!(M_STIFF, "Model Stiffness", 0.0, 1.0, 0.3, Unit::None);
    n!(M_LOSS, "Model Inner Loss", 0.0, 1.0, 0.3, Unit::None);
    n!(M_RES, "Model Resolution", 0.0, 1.0, 0.6, Unit::None);
    n!(RM_ON, "Ring Mod On", 0.0, 1.0, 0.0, Unit::OnOff);
    n!(RM_VOL, "Ring Mod Level", -60.0, 0.0, 0.0, Unit::Db);
    n!(RM_FLOW, "Ring Mod Signal Flow", 0.0, 1.0, 1.0, Unit::OnOff);
    n!(NZ_ON, "Noise On", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(NZ_TYPE, "Noise Filter Type", NZ_TYPES, 0.0);
    n!(NZ_CUT, "Noise Cutoff", 0.0, 1.0, 0.7, Unit::None);
    n!(NZ_RES, "Noise Resonance", 0.0, 1.0, 0.0, Unit::None);
    n!(NZ_DIRT, "Noise Dirt", 0.0, 1.0, 0.0, Unit::None);
    n!(NZ_VOL, "Noise Volume", -60.0, 0.0, 0.0, Unit::Db);
    n!(NZ_FLOW, "Noise Signal Flow", 0.0, 1.0, 1.0, Unit::OnOff);
    n!(F_ON, "Filter On", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(F_TYPE, "Filter Type", F_TYPES, 0.0);
    m!(F_SLOPE, "Filter Slope", F_SLOPES, 1.0);
    n!(F_CUT, "Filter Cutoff", 0.0, 1.0, 0.5, Unit::None);
    n!(F_RES, "Filter Resonance", 0.0, 1.0, 0.0, Unit::None);
    m!(D_MODE, "Distortion Mode", D_MODES, 0.0);
    n!(D_DRIVE, "Distortion Drive", 0.0, 1.0, 0.0, Unit::None);
    n!(D_COLOR, "Distortion Color", 0.0, 1.0, 0.5, Unit::None);
    n!(D_LEVEL, "Distortion Level", 0.0, 1.0, 0.5, Unit::None);
    n!(D_POS, "Distortion Before Filter", 0.0, 1.0, 0.0, Unit::OnOff);
    n!(EQ1_ON, "EQ Band 1 On", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(EQ1_TYPE, "EQ Band 1 Type", EQ_TYPES, 0.0);
    n!(EQ1_FREQ, "EQ Band 1 Frequency", 20.0, 20000.0, 200.0, Unit::Hz);
    n!(EQ1_Q, "EQ Band 1 Q", 0.1, 12.0, 1.0, Unit::None);
    n!(EQ1_GAIN, "EQ Band 1 Gain", -18.0, 18.0, 0.0, Unit::Db);
    n!(EQ2_ON, "EQ Band 2 On", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(EQ2_TYPE, "EQ Band 2 Type", EQ_TYPES, 0.0);
    n!(EQ2_FREQ, "EQ Band 2 Frequency", 20.0, 20000.0, 2000.0, Unit::Hz);
    n!(EQ2_Q, "EQ Band 2 Q", 0.1, 12.0, 1.0, Unit::None);
    n!(EQ2_GAIN, "EQ Band 2 Gain", -18.0, 18.0, 0.0, Unit::Db);
    m!(PAN_MODE, "Pan Mod / Spread", PAN_MODES, 0.0);
    n!(SPREAD_LO, "Spread Lo Frequency", 0.0, 1.0, 0.0, Unit::None);
    n!(SPREAD_HI, "Spread Hi Frequency", 0.0, 1.0, 0.5, Unit::None);
    n!(VOICE_VOL, "Voice Volume", -60.0, 6.0, 0.0, Unit::Db);
    m!(TRIG, "Trigger Mode", TRIGS, 0.0);
    m!(GROUP, "Group", GROUPS, 0.0);
    n!(GATE, "Gate", 0.0, 1.0, 0.0, Unit::OnOff);
    n!(SWING_EN, "Swing Enable", 0.0, 1.0, 0.0, Unit::OnOff);
    n!(ACC_EN, "Accent Enable", 0.0, 1.0, 1.0, Unit::OnOff);
    n!(MIX_VOL, "Mixer Volume", -60.0, 6.0, 0.0, Unit::Db);
    n!(MIX_PAN, "Mixer Pan", -1.0, 1.0, 0.0, Unit::Pan);
    n!(MUTE, "Mute", 0.0, 1.0, 0.0, Unit::OnOff);
    n!(SOLO, "Solo", 0.0, 1.0, 0.0, Unit::OnOff);
    m!(OUTPUT, "Output", OUTPUTS, 0.0);
    for k in NAME0..NAME0 + 4 {
        n!(k, "Name", 0.0, 2_097_151.0, 0.0, Unit::None);
    }
    for t in 0..T_COUNT {
        v.push(VSpec { k: mod_k(t, 0), name: leak(format!("{} Mod", TARGET_NAMES[t])), min: 0.0, max: 7.0, def: 0.0, unit: Unit::Menu, choices: MOD_SRC });
        v.push(VSpec { k: mod_k(t, 1), name: leak(format!("{} Mod Amount", TARGET_NAMES[t])), min: -1.0, max: 1.0, def: 0.5, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: mod_k(t, 2), name: leak(format!("{} Via", TARGET_NAMES[t])), min: 0.0, max: 5.0, def: 0.0, unit: Unit::Menu, choices: VIA_SRC });
        v.push(VSpec { k: mod_k(t, 3), name: leak(format!("{} Via Amount", TARGET_NAMES[t])), min: -1.0, max: 1.0, def: 1.0, unit: Unit::None, choices: &[] });
    }
    for e in 0..4u32 {
        let b = ENV0 + e * 8;
        let dd = if e == 3 { 0.55 } else { 0.5 };
        v.push(VSpec { k: b, name: leak(format!("Env {} Attack", e + 1)), min: 0.0, max: 1.0, def: 0.0, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 1, name: leak(format!("Env {} Attack Shape", e + 1)), min: -1.0, max: 1.0, def: 0.0, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 2, name: leak(format!("Env {} Decay", e + 1)), min: 0.0, max: 1.0, def: dd, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 3, name: leak(format!("Env {} Decay Shape", e + 1)), min: -1.0, max: 1.0, def: 0.25, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 4, name: leak(format!("Env {} Sustain", e + 1)), min: 0.0, max: 1.0, def: 0.0, unit: Unit::OnOff, choices: &[] });
        v.push(VSpec { k: b + 5, name: leak(format!("Env {} Sustain Point", e + 1)), min: 0.0, max: 1.0, def: 0.5, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 6, name: leak(format!("Env {} Mod Target", e + 1)), min: 0.0, max: 3.0, def: 2.0, unit: Unit::Menu, choices: ENV_TARGETS });
        v.push(VSpec { k: b + 7, name: leak(format!("Env {} Mod via Vel", e + 1)), min: -1.0, max: 1.0, def: 0.0, unit: Unit::None, choices: &[] });
    }
    for l in 0..2u32 {
        let b = LFO0 + l * 6;
        v.push(VSpec { k: b, name: leak(format!("LFO {} On", l + 1)), min: 0.0, max: 1.0, def: 1.0, unit: Unit::OnOff, choices: &[] });
        v.push(VSpec { k: b + 1, name: leak(format!("LFO {} Sync", l + 1)), min: 0.0, max: 1.0, def: 0.0, unit: Unit::OnOff, choices: &[] });
        v.push(VSpec { k: b + 2, name: leak(format!("LFO {} Rate", l + 1)), min: 0.0, max: 1.0, def: 0.5, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 3, name: leak(format!("LFO {} Waveform", l + 1)), min: 0.0, max: 1.0, def: 0.0, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 4, name: leak(format!("LFO {} Cycles", l + 1)), min: 1.0, max: 100.0, def: 100.0, unit: Unit::None, choices: &[] });
        v.push(VSpec { k: b + 5, name: leak(format!("LFO {} Ramp", l + 1)), min: -1.0, max: 1.0, def: 0.0, unit: Unit::None, choices: &[] });
    }
    v
}

/// Parameters the step automation can offset (no menus, buttons, names, pan or spread).
pub fn automatable(k: u32) -> bool {
    vspec(k).is_some_and(|s| !matches!(s.unit, Unit::Menu | Unit::OnOff | Unit::Pan) && !(NAME0..NAME0 + 4).contains(&k) && k != SPREAD_LO && k != SPREAD_HI)
}

fn global_specs() -> Vec<Param> {
    let mut v = vec![
        Param::new(G_MASTER, "Master Volume", -60.0, 6.0, 0.0, Unit::Db),
        Param::new(G_SEL, "Selected Sound", 0.0, 24.0, 0.0, Unit::None).hide(),
        Param::menu(G_PAT, "Pattern", PATTERNS, 0.0),
        Param::new(G_SEQ_ON, "Sequencer On", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(G_SWING, "Swing", 0.0, 1.0, 0.0, Unit::None),
        Param::new(G_ACCENT, "Accent", 0.0, 1.0, 0.5, Unit::None),
        Param::new(G_PMODE, "Pattern Mode", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(G_PBMODE, "Playback Mode", PLAYBACK_MODES, 0.0),
        Param::new(G_MUTEMODE, "Voice Mute Mode", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(G_AUTOSEL, "Voice Auto Select", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(G_EDIT, "Edit Mode", EDIT_MODES, 0.0),
        Param::new(G_FULL, "Full View", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
    ];
    for (i, n) in ["Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D"].into_iter().enumerate() {
        v.push(Param::menu(G_CTRL + i as u32, n, kit::CTRLS, [1.0, 2.0, 4.0, 7.0][i]));
    }
    for (i, n) in ["Ctrl A Value", "Ctrl B Value", "Ctrl C Value", "Ctrl D Value"].into_iter().enumerate() {
        v.push(Param::new(G_CV + i as u32, n, 0.0, 1.0, 0.0, Unit::None).hide());
    }
    v.push(Param::new(G_RUN, "Transport", 0.0, 1.0, 0.0, Unit::OnOff));
    v
}

fn base_params() -> &'static Vec<Param> {
    static P: OnceLock<Vec<Param>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = global_specs();
        for p in 0..24usize {
            v.push(Param::new(pat_len_id(p), leak(format!("Pattern {} Length", p + 1)), 1.0, 32.0, 16.0, Unit::None).hide());
            v.push(Param::menu(pat_res_id(p), leak(format!("Pattern {} Resolution", p + 1)), RESOLUTIONS, 3.0).hide());
        }
        for vo in 0..25usize {
            for s in vspecs() {
                let name: &'static str = leak(format!("Voice {} {}", vo + 1, s.name));
                let mut p = if s.choices.is_empty() { Param::new(vid(vo, s.k), name, s.min, s.max, s.def, s.unit) } else { Param::menu(vid(vo, s.k), name, s.choices, s.def) };
                if (NAME0..NAME0 + 4).contains(&s.k) {
                    p = p.hide();
                }
                v.push(p);
            }
        }
        v
    })
}

fn seq_param_name(id: u32) -> &'static str {
    static N: OnceLock<Mutex<HashMap<u32, &'static str>>> = OnceLock::new();
    let m = N.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = m.lock().unwrap();
    m.entry(id).or_insert_with(|| {
        let r = id - SEQ_BASE;
        let (cell, w) = (r / 64, r % 64);
        leak(format!("Pattern {} Sound {} Step Data {}", cell / 25 + 1, cell % 25 + 1, w))
    })
}

// ---- the instrument

#[derive(Clone, Copy)]
struct Ev {
    at: usize,
    v: usize,
    vel: f32,
    gain: f32,
    gate: i64,
    pitch: f32,
    note: i16,
    off: bool,
    lanes: [(u8, f32); 4],
}

#[derive(Clone, Copy, Default)]
struct PatRun {
    pat: Option<usize>,
    origin: f64,
    once: bool,
    held: Option<u8>,
    pending: Option<Option<usize>>,
}

struct DVoice {
    inst: [Inst; 4],
    free: [f32; 2],
}

pub struct Ultrabeat {
    rate: f32,
    bpm: f64,
    tr: Transport,
    g: [f32; 100],
    vp: Vec<[f32; 256]>,
    cells: Vec<Cell>,
    pat_len: [u8; 24],
    pat_res: [u8; 24],
    dv: Vec<DVoice>,
    plist: Vec<Param>,
    declared: Vec<u32>,
    side: SideBuf,
    free_beats: f64,
    last_end: f64,
    pm: PatRun,
    learn: Option<usize>,
    hits: [f32; 25],
    events: Vec<Ev>,
    samples: Vec<Option<Arc<Sample>>>,
    sample_names: Vec<String>,
    names: Vec<String>,
    seed: u32,
}

/// A sound's starting point (the Init menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Init {
    Standard,
    Sample,
    Kick,
    Snare,
    Hihat,
    Tom,
    Clap,
    Cymbal,
    Bell,
}

pub const INITS: &[(&str, Init)] = &[
    ("Standard Tut", Init::Standard),
    ("Sample Init", Init::Sample),
    ("Kick", Init::Kick),
    ("Snare", Init::Snare),
    ("Hihat", Init::Hihat),
    ("Tom", Init::Tom),
    ("Clap", Init::Clap),
    ("Cymbal", Init::Cymbal),
    ("Ring Bell", Init::Bell),
];

fn name_words(name: &str) -> [f32; 4] {
    let mut w = [0.0f32; 4];
    let b: Vec<u8> = name.bytes().filter(|c| (32..127).contains(c)).take(12).collect();
    for (i, c) in b.iter().enumerate() {
        w[i / 3] += ((*c as u32) << (7 * (i % 3))) as f32;
    }
    w
}

fn name_from(words: &[f32]) -> String {
    let mut s = String::new();
    for w in words {
        let x = *w as u32;
        for q in 0..3 {
            let c = ((x >> (7 * q)) & 127) as u8;
            if c >= 32 {
                s.push(c as char);
            }
        }
    }
    s.trim().to_string()
}

fn voice_default() -> [f32; 256] {
    let mut a = [0.0f32; 256];
    for s in vspecs() {
        a[s.k as usize] = s.def;
    }
    a
}

/// A sound recipe on a parameter array (the mixer and the name stay).
fn recipe(a: &mut [f32; 256], kind: Init) {
    let ks = [MIX_VOL, MIX_PAN, MUTE, SOLO, OUTPUT, NAME0, NAME0 + 1, NAME0 + 2, NAME0 + 3];
    let keep: Vec<f32> = ks.iter().map(|k| a[*k as usize]).collect();
    *a = voice_default();
    for (k, v) in ks.iter().zip(keep) {
        a[*k as usize] = v;
    }
    fn modset(a: &mut [f32; 256], t: usize, src: f32, amt: f32) {
        a[mod_k(t, 0) as usize] = src;
        a[mod_k(t, 1) as usize] = amt;
    }
    let env4_decay = ENV0 as usize + 24 + 2;
    let env1_decay = ENV0 as usize + 2;
    match kind {
        Init::Standard => {}
        Init::Sample => {
            a[O1_ON as usize] = 0.0;
            a[O2_ON as usize] = 1.0;
            a[O2_MODE as usize] = 1.0;
        }
        Init::Kick => {
            a[O1_PITCH as usize] = -24.0;
            a[O1_SAT as usize] = 0.15;
            a[env1_decay] = 0.38;
            a[env1_decay + 1] = 0.5;
            modset(a, T_O1_PITCH, 3.0, 0.3);
            a[env4_decay] = 0.62;
            a[EQ1_ON as usize] = 1.0;
            a[EQ1_GAIN as usize] = 4.0;
            a[D_MODE as usize] = 2.0;
            a[F_ON as usize] = 1.0;
            a[F_CUT as usize] = 0.8;
            a[D_DRIVE as usize] = 0.15;
        }
        Init::Snare => {
            a[O1_PITCH as usize] = -8.0;
            a[O1_VOL as usize] = -6.0;
            modset(a, T_O1_PITCH, 3.0, 0.12);
            a[env1_decay] = 0.3;
            a[NZ_ON as usize] = 1.0;
            a[NZ_TYPE as usize] = 1.0;
            a[NZ_CUT as usize] = 0.45;
            a[NZ_VOL as usize] = -4.0;
            a[env4_decay] = 0.5;
        }
        Init::Hihat => {
            a[O1_ON as usize] = 0.0;
            a[NZ_ON as usize] = 1.0;
            a[NZ_TYPE as usize] = 1.0;
            a[NZ_CUT as usize] = 0.78;
            a[NZ_DIRT as usize] = 0.2;
            a[env4_decay] = 0.3;
            a[F_ON as usize] = 1.0;
            a[F_TYPE as usize] = 1.0;
            a[F_CUT as usize] = 0.65;
        }
        Init::Tom => {
            a[O1_PITCH as usize] = -14.0;
            a[O1_SAT as usize] = 0.1;
            modset(a, T_O1_PITCH, 3.0, 0.08);
            a[env1_decay] = 0.45;
            a[env4_decay] = 0.6;
        }
        Init::Clap => {
            a[O1_ON as usize] = 0.0;
            a[NZ_ON as usize] = 1.0;
            a[NZ_TYPE as usize] = 2.0;
            a[NZ_CUT as usize] = 0.58;
            a[NZ_RES as usize] = 0.3;
            a[env4_decay] = 0.4;
        }
        Init::Cymbal => {
            a[O1_MODE as usize] = 1.0;
            a[O1_PITCH as usize] = 10.0;
            a[O1_FM as usize] = 0.8;
            a[O2_ON as usize] = 1.0;
            a[O2_PITCH as usize] = 17.3;
            a[O1_VOL as usize] = -8.0;
            a[O2_VOL as usize] = -12.0;
            a[NZ_ON as usize] = 1.0;
            a[NZ_TYPE as usize] = 1.0;
            a[NZ_CUT as usize] = 0.72;
            a[NZ_VOL as usize] = -6.0;
            a[env4_decay] = 0.72;
        }
        Init::Bell => {
            a[O1_MODE as usize] = 1.0;
            a[O1_PITCH as usize] = 5.0;
            a[O1_FM as usize] = 0.35;
            a[O2_ON as usize] = 1.0;
            a[O2_PITCH as usize] = 12.0;
            a[RM_ON as usize] = 1.0;
            a[RM_VOL as usize] = -6.0;
            a[env4_decay] = 0.75;
        }
    }
}

fn default_kit() -> Vec<(&'static str, Init)> {
    vec![
        ("Kick 1", Init::Kick),
        ("Kick 2", Init::Kick),
        ("Kick 3", Init::Kick),
        ("Kick 4", Init::Kick),
        ("Snare 1", Init::Snare),
        ("Snare 2", Init::Snare),
        ("Snare 3", Init::Snare),
        ("Clap", Init::Clap),
        ("Snare 5", Init::Snare),
        ("Tom 1", Init::Tom),
        ("Tom 2", Init::Tom),
        ("Tom 3", Init::Tom),
        ("Tom 4", Init::Tom),
        ("Tom 5", Init::Tom),
        ("Tom 6", Init::Tom),
        ("Rim", Init::Clap),
        ("Perc 1", Init::Standard),
        ("Hihat 1", Init::Hihat),
        ("Hihat 2", Init::Hihat),
        ("Cym 1", Init::Cymbal),
        ("Cym 2", Init::Cymbal),
        ("Ring Bell", Init::Bell),
        ("Perc 2", Init::Standard),
        ("Perc 3", Init::Standard),
        ("Chromatic", Init::Standard),
    ]
}

impl Ultrabeat {
    pub fn new(rate: f32) -> Ultrabeat {
        dsp::warm_tables();
        let mut g = [0.0f32; 100];
        for p in global_specs() {
            g[p.id as usize] = p.default;
        }
        let mut vp = vec![voice_default(); 25];
        let mut names = Vec::new();
        for (i, (n, kind)) in default_kit().into_iter().enumerate() {
            recipe(&mut vp[i], kind);
            // the sounds of one kind are tuned a little apart
            match kind {
                Init::Kick | Init::Tom => vp[i][O1_PITCH as usize] += (i % 6) as f32 * 2.0,
                Init::Hihat => vp[i][F_CUT as usize] += (i % 2) as f32 * 0.04,
                _ => {}
            }
            let w = name_words(n);
            for q in 0..4 {
                vp[i][NAME0 as usize + q] = w[q];
            }
            names.push(n.to_string());
        }
        let mut plist = base_params().clone();
        plist.reserve(2048);
        Ultrabeat {
            rate,
            bpm: 120.0,
            tr: Transport { beats: 0.0, bpm: 120.0, playing: false },
            g,
            vp,
            cells: vec![Cell::default(); 24 * 25],
            pat_len: [16; 24],
            pat_res: [3; 24],
            dv: (0..25).map(|_| DVoice { inst: [Inst::default(); 4], free: [0.0; 2] }).collect(),
            plist,
            declared: Vec::with_capacity(2048),
            side: SideBuf::new(),
            free_beats: 0.0,
            last_end: f64::NAN,
            pm: PatRun::default(),
            learn: None,
            hits: [0.0; 25],
            events: Vec::with_capacity(1024),
            samples: vec![None; 25],
            sample_names: vec![String::new(); 25],
            names,
            seed: 1,
        }
    }

    // ---- state the window reads and edits

    pub fn selected(&self) -> usize {
        (self.g[G_SEL as usize].round().max(0.0) as usize).min(24)
    }
    pub fn vparam(&self, v: usize, k: u32) -> f32 {
        self.vp[v.min(24)][k as usize]
    }
    pub fn voice_params(&self, v: usize) -> [f32; 256] {
        self.vp[v.min(24)]
    }
    pub fn voice_name(&self, v: usize) -> &str {
        &self.names[v.min(24)]
    }
    /// The parameter pairs that give sound `v` the name.
    pub fn name_pairs(v: usize, name: &str) -> Vec<(u32, f32)> {
        let w = name_words(name);
        (0..4).map(|q| (vid(v, NAME0 + q as u32), w[q])).collect()
    }
    /// Parameter pairs that make sound `v` a copy of `src` (another sound's parameter array);
    /// the mixer (volume, pan, mute, solo, output) stays when `keep_mixer`.
    pub fn assign_pairs(v: usize, src: &[f32; 256], keep_mixer: bool) -> Vec<(u32, f32)> {
        let mut out = Vec::new();
        for s in vspecs() {
            if keep_mixer && (MIX_VOL..=OUTPUT).contains(&s.k) {
                continue;
            }
            out.push((vid(v, s.k), src[s.k as usize]));
        }
        out
    }
    /// Parameter pairs that make sound `v` a fresh starting-point sound.
    pub fn init_pairs(&self, v: usize, kind: Init) -> Vec<(u32, f32)> {
        let mut a = self.vp[v.min(24)];
        recipe(&mut a, kind);
        let mut o = Ultrabeat::assign_pairs(v, &a, true);
        if kind != Init::Standard {
            let n = INITS.iter().find(|x| x.1 == kind).map(|x| x.0).unwrap_or("Init");
            o.extend(Ultrabeat::name_pairs(v, n));
        }
        o
    }
    pub fn cell(&self, pat: usize, v: usize) -> &Cell {
        &self.cells[pat.min(23) * 25 + v.min(24)]
    }
    /// Parameter pairs that make a cell equal `c` (only the words that differ).
    pub fn cell_pairs(&self, pat: usize, v: usize, c: &Cell) -> Vec<(u32, f32)> {
        let old = self.cell(pat, v);
        (0..64).filter(|&w| old.word(w) != c.word(w)).map(|w| (cell_id(pat, v, w), c.word(w))).collect()
    }
    pub fn pattern_len(&self, p: usize) -> usize {
        self.pat_len[p.min(23)] as usize
    }
    pub fn pattern_res(&self, p: usize) -> usize {
        self.pat_res[p.min(23)] as usize
    }
    /// Beats one step of the pattern lasts.
    pub fn step_beats(&self, p: usize) -> f64 {
        RES_BEATS[self.pattern_res(p).min(5)]
    }
    /// Swing works at 1/8 and 1/16 only.
    pub fn swing_active(&self, p: usize) -> bool {
        matches!(self.pattern_res(p), 1 | 3)
    }
    /// 0…1: how recently the sound was hit (the Assignment keyboard's light).
    pub fn hit_level(&self, v: usize) -> f32 {
        self.hits[v.min(24)]
    }
    /// The step the pattern is on now, when the sequencer runs.
    pub fn playing_step(&self) -> Option<usize> {
        let running = self.g[G_SEQ_ON as usize] >= 0.5 && (self.tr.playing || self.g[G_RUN as usize] >= 0.5);
        if !running {
            return None;
        }
        let (pat, origin) = if self.g[G_PMODE as usize] >= 0.5 { (self.pm.pat?, self.pm.origin) } else { ((self.g[G_PAT as usize].round() as usize).min(23), 0.0) };
        let beats = if self.tr.playing { self.tr.beats } else { self.free_beats };
        let n = ((beats - origin) / self.step_beats(pat)).floor() as i64;
        if n < 0 {
            return None;
        }
        Some((n as usize) % self.pattern_len(pat))
    }
    /// The pattern the MIDI pattern mode is playing (None: stopped).
    pub fn pattern_mode_playing(&self) -> Option<usize> {
        self.pm.pat
    }
    pub fn start_learn(&mut self, ctrl: usize) {
        self.learn = Some(ctrl.min(3));
    }
    pub fn cancel_learn(&mut self) {
        self.learn = None;
    }
    pub fn learning(&self) -> Option<usize> {
        self.learn
    }
    /// Gives oscillator 2 of sound `v` a sample (its Sample mode plays it).
    pub fn load_sample(&mut self, v: usize, data: Vec<f32>, rate: f32, name: &str) {
        self.samples[v.min(24)] = Some(Arc::new(Sample { data, rate }));
        self.sample_names[v.min(24)] = name.to_string();
    }
    pub fn unload_sample(&mut self, v: usize) {
        self.samples[v.min(24)] = None;
        self.sample_names[v.min(24)].clear();
    }
    pub fn sample_name(&self, v: usize) -> Option<&str> {
        self.samples[v.min(24)].as_ref().map(|_| self.sample_names[v.min(24)].as_str())
    }
    /// The sample's outline for the waveform display.
    pub fn sample_preview(&self, v: usize, points: usize) -> Vec<f32> {
        match &self.samples[v.min(24)] {
            Some(s) if !s.data.is_empty() => (0..points)
                .map(|i| {
                    let a = i * s.data.len() / points;
                    let b = ((i + 1) * s.data.len() / points).max(a + 1).min(s.data.len());
                    s.data[a..b].iter().fold(0.0f32, |m, x| if x.abs() > m.abs() { *x } else { m })
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The notes of a pattern as MIDI: (beat, note, velocity, length in beats), with swing
    /// applied; accents raise the velocity.
    pub fn pattern_notes(&self, pattern: usize) -> Vec<(f64, u8, u8, f64)> {
        let p = pattern.min(23);
        let (len, sl) = (self.pattern_len(p), self.step_beats(p));
        let mut out = Vec::new();
        for v in 0..25 {
            let c = self.cell(p, v);
            let swing = self.vp[v][SWING_EN as usize] >= 0.5 && self.swing_active(p);
            let acc = self.vp[v][ACC_EN as usize] >= 0.5;
            for i in 0..len {
                if !c.trig(i) {
                    continue;
                }
                let shift = if swing && i % 2 == 1 { self.g[G_SWING as usize] as f64 * sl * 0.5 } else { 0.0 };
                let mut vel = c.vel(i);
                if acc && c.accent(i) {
                    vel = (vel + (1.0 - vel) * 0.5 + self.g[G_ACCENT as usize] * 0.3).min(1.0);
                }
                let note = if v < 24 { 36 + v as u8 } else { 60 };
                out.push((i as f64 * sl + shift, note, (vel * 127.0).round().clamp(1.0, 127.0) as u8, sl * c.gate_len(i) as f64));
            }
        }
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        out
    }

    // ---- inside

    fn declare(&mut self, id: u32) {
        if let Err(pos) = self.declared.binary_search(&id) {
            self.declared.insert(pos, id);
            self.plist.push(Param::new(id, seq_param_name(id), 0.0, 16_777_215.0, 0.0, Unit::None).hide());
        }
    }

    fn refresh_name(&mut self, v: usize) {
        let w = [self.vp[v][NAME0 as usize], self.vp[v][NAME0 as usize + 1], self.vp[v][NAME0 as usize + 2], self.vp[v][NAME0 as usize + 3]];
        self.names[v] = name_from(&w);
    }

    fn voice_audible(&self) -> [bool; 25] {
        let solo = (0..25).any(|v| self.vp[v][SOLO as usize] >= 0.5);
        let mut a = [true; 25];
        for (v, x) in a.iter_mut().enumerate() {
            *x = self.vp[v][MUTE as usize] < 0.5 && (!solo || self.vp[v][SOLO as usize] >= 0.5);
        }
        a
    }

    fn choke_voice(&mut self, v: usize) {
        let rate = self.rate;
        for i in &mut self.dv[v].inst {
            i.choke(0.003, rate);
        }
    }

    fn trigger(&mut self, e: &Ev) {
        let v = e.v;
        let group = self.vp[v][GROUP as usize].round() as usize;
        if group > 0 {
            for o in 0..25 {
                if o != v && self.vp[o][GROUP as usize].round() as usize == group {
                    self.choke_voice(o);
                }
            }
        }
        if self.vp[v][TRIG as usize] >= 0.5 || group > 0 {
            self.choke_voice(v);
        }
        let rate = self.rate;
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let base = self.vp[v];
        let p = crate::ultrabeat_dsp::with_lanes(&base, &e.lanes);
        let d = &mut self.dv[v];
        // a free slot, else the oldest
        let slot = (0..4).find(|&i| !d.inst[i].active).unwrap_or_else(|| (0..4).max_by_key(|&i| d.inst[i].age).unwrap_or(0));
        d.inst[slot].start(&p, e.vel, e.gain, e.pitch, e.note, e.gate, e.lanes, rate, self.seed);
        self.hits[v] = 1.0;
    }

    fn release_note(&mut self, note: i16) {
        let rate = self.rate;
        for v in 0..25 {
            let cut = self.vp[v][GATE as usize] >= 0.5;
            for i in &mut self.dv[v].inst {
                if i.active && i.held && i.note == note {
                    i.release(cut, rate);
                }
            }
        }
    }

    fn step_event(&self, pat: usize, v: usize, i: usize, at: usize, spb: f64) -> Option<Ev> {
        let c = self.cell(pat, v);
        if !c.trig(i) {
            return None;
        }
        let sl = self.step_beats(pat);
        let mut gain = 1.0;
        if c.accent(i) && self.vp[v][ACC_EN as usize] >= 0.5 {
            gain = 1.0 + self.g[G_ACCENT as usize] * 0.8;
        }
        let mut lanes = [(0u8, 0.0f32); 4];
        for l in 0..4 {
            if c.lane_heard(l) {
                if let Some(s) = vspec(c.lane[l] as u32 - 1) {
                    lanes[l] = (c.lane[l], c.offs[l][i] as f32 / 31.0 * (s.max - s.min));
                }
            }
        }
        Some(Ev { at, v, vel: c.vel(i), gain, gate: (sl * c.gate_len(i) as f64 * spb).max(1.0) as i64, pitch: 0.0, note: -1, off: false, lanes })
    }

    /// Hits of the steps whose time falls in [b0, b1) beats (`blk0`: the block's first beat).
    #[allow(clippy::too_many_arguments)]
    fn gen_range(&mut self, pat: usize, origin: f64, once: bool, b0: f64, b1: f64, blk0: f64, spb: f64, n: usize) {
        let len = self.pattern_len(pat) as i64;
        let sl = self.step_beats(pat);
        let swing_on = self.swing_active(pat);
        let nlo = (((b0 - origin) / sl).floor() as i64 - 1).max(0);
        let nhi = ((b1 - origin) / sl).ceil() as i64 + 1;
        for s in nlo..=nhi {
            if once && s >= len {
                break;
            }
            let idx = (s % len) as usize;
            for v in 0..25 {
                if !self.cells[pat * 25 + v].trig(idx) {
                    continue;
                }
                let shift = if swing_on && idx % 2 == 1 && self.vp[v][SWING_EN as usize] >= 0.5 { self.g[G_SWING as usize] as f64 * sl * 0.5 } else { 0.0 };
                let t = origin + s as f64 * sl + shift;
                if t < b0 || t >= b1 {
                    continue;
                }
                let at = (((t - blk0) * spb).max(0.0) as usize).min(n.saturating_sub(1));
                if let Some(e) = self.step_event(pat, v, idx, at, spb) {
                    if self.events.len() < self.events.capacity() {
                        self.events.push(e);
                    }
                }
            }
        }
    }

    fn gen_events(&mut self, n: usize) {
        let tr = self.tr;
        let bpm = if tr.bpm > 0.0 { tr.bpm } else { self.bpm };
        let spb = self.rate as f64 * 60.0 / bpm;
        let run = self.g[G_SEQ_ON as usize] >= 0.5 && (tr.playing || self.g[G_RUN as usize] >= 0.5);
        if !run {
            self.last_end = f64::NAN;
            return;
        }
        let mut b0 = if tr.playing { tr.beats } else { self.free_beats };
        if (b0 - self.last_end).abs() < 1e-3 {
            b0 = self.last_end;
        }
        let b1 = b0 + n as f64 / spb;
        self.last_end = b1;
        if !tr.playing {
            self.free_beats = b1;
        }
        if self.g[G_PMODE as usize] < 0.5 {
            let pat = (self.g[G_PAT as usize].round().max(0.0) as usize).min(23);
            self.gen_range(pat, 0.0, false, b0, b1, b0, spb, n);
            return;
        }
        let Some(pat) = self.pm.pat else { return };
        let (origin, once) = (self.pm.origin, self.pm.once);
        if let Some(pend) = self.pm.pending {
            // Toggle on Step 1: switch or stop at the start of the next cycle
            let cyc = self.step_beats(pat) * self.pattern_len(pat) as f64;
            let next = origin + ((b0 - origin) / cyc).ceil().max(0.0) * cyc;
            if next < b1 {
                self.gen_range(pat, origin, once, b0, next.max(b0), b0, spb, n);
                self.pm.pending = None;
                match pend {
                    Some(p) => {
                        self.pm.pat = Some(p);
                        self.pm.origin = next;
                        self.gen_range(p, next, false, next.max(b0), b1, b0, spb, n);
                    }
                    None => self.pm.pat = None,
                }
                return;
            }
        }
        self.gen_range(pat, origin, once, b0, b1, b0, spb, n);
        if once && (b1 - origin) / self.step_beats(pat) >= self.pattern_len(pat) as f64 {
            self.pm.pat = None;
        }
    }

    fn pattern_note(&mut self, note: u8, on: bool) {
        let p = note as usize;
        let beats = if self.tr.playing { self.tr.beats } else { self.free_beats };
        let mode = self.g[G_PBMODE as usize].round() as i32;
        if !on {
            if mode == 1 && self.pm.held == Some(note) {
                self.pm.pat = None;
                self.pm.held = None;
            }
            return;
        }
        match mode {
            0 => self.pm = PatRun { pat: Some(p), origin: beats, once: true, held: None, pending: None },
            1 => self.pm = PatRun { pat: Some(p), origin: beats, once: false, held: Some(note), pending: None },
            _ => {
                if self.pm.pat == Some(p) {
                    if mode == 3 {
                        self.pm.pending = Some(None);
                    } else {
                        self.pm.pat = None;
                    }
                } else if self.pm.pat.is_some() {
                    if mode == 3 {
                        self.pm.pending = Some(Some(p));
                    } else {
                        self.pm.pat = Some(p);
                    }
                } else {
                    self.pm = PatRun { pat: Some(p), origin: beats, once: false, held: None, pending: None };
                }
            }
        }
    }

    fn render_span(&mut self, l: &mut [f32], r: &mut [f32], from: usize, to: usize, audible: &[bool; 25]) {
        let rate = self.rate;
        let bpm = if self.tr.bpm > 0.0 { self.tr.bpm as f32 } else { self.bpm as f32 };
        let ctrl = [self.g[G_CV as usize], self.g[G_CV as usize + 1], self.g[G_CV as usize + 2], self.g[G_CV as usize + 3]];
        let master = if self.g[G_MASTER as usize] <= -59.9 { 0.0 } else { 10f32.powf(self.g[G_MASTER as usize] / 20.0) };
        let mut pos = from;
        while pos < to {
            let n = (to - pos).min(CH);
            let mut side = [0.0f32; CH];
            if self.side.n >= pos + n {
                for (k, s) in side.iter_mut().enumerate().take(n) {
                    *s = self.side.at(pos + k);
                }
            }
            for v in 0..25 {
                if !self.dv[v].inst.iter().any(|i| i.active) {
                    continue;
                }
                let gain = if audible[v] {
                    let d = self.vp[v][MIX_VOL as usize];
                    if d <= -59.9 { 0.0 } else { 10f32.powf(d / 20.0) * master }
                } else {
                    0.0
                };
                let base = self.vp[v];
                let incs = [0, 1].map(|i| {
                    let b = LFO0 as usize + i * 6;
                    crate::ultrabeat_dsp::lfo_hz(base[b + 1] >= 0.5, base[b + 2], bpm) * n as f32 / rate
                });
                let gl = Gl { rate, bpm, ctrl, side: &side[..n], sample: self.samples[v].as_deref(), free: self.dv[v].free, gain };
                for i in &mut self.dv[v].inst {
                    if i.active {
                        i.render(&base, &gl, n, &mut l[pos..pos + n], &mut r[pos..pos + n]);
                    }
                }
                for k in 0..2 {
                    self.dv[v].free[k] = (self.dv[v].free[k] + incs[k]).fract();
                }
            }
            pos += n;
        }
    }
}

impl Plugin for Ultrabeat {
    fn params(&self) -> &[Param] {
        &self.plist
    }
    fn get(&self, id: u32) -> f32 {
        if id < 100 {
            return self.g[id as usize];
        }
        if (VBASE..VBASE + 25 * VSTRIDE).contains(&id) {
            let v = ((id - VBASE) / VSTRIDE) as usize;
            let k = ((id - VBASE) % VSTRIDE) as usize;
            return self.vp[v][k];
        }
        if (SEQ_BASE..PAT_BASE).contains(&id) {
            let r = (id - SEQ_BASE) as usize;
            return self.cells[r / 64].word(r % 64);
        }
        if (PAT_BASE..PAT_BASE + 48).contains(&id) {
            let r = (id - PAT_BASE) as usize;
            return if r % 2 == 0 { self.pat_len[r / 2] as f32 } else { self.pat_res[r / 2] as f32 };
        }
        0.0
    }
    fn set(&mut self, id: u32, v: f32) {
        if id < 100 {
            self.g[id as usize] = v;
        } else if (VBASE..VBASE + 25 * VSTRIDE).contains(&id) {
            let vo = ((id - VBASE) / VSTRIDE) as usize;
            let k = (id - VBASE) % VSTRIDE;
            if let Some(s) = vspec(k) {
                let x = v.clamp(s.min, s.max);
                self.vp[vo][k as usize] = if matches!(s.unit, Unit::Menu | Unit::OnOff) { x.round() } else { x };
                if (NAME0..NAME0 + 4).contains(&k) {
                    self.refresh_name(vo);
                }
            }
        } else if (SEQ_BASE..PAT_BASE).contains(&id) {
            let r = (id - SEQ_BASE) as usize;
            self.cells[r / 64].set_word(r % 64, v);
            self.declare(id);
        } else if (PAT_BASE..PAT_BASE + 48).contains(&id) {
            let r = (id - PAT_BASE) as usize;
            if r % 2 == 0 {
                self.pat_len[r / 2] = v.round().clamp(1.0, 32.0) as u8;
            } else {
                self.pat_res[r / 2] = v.round().clamp(0.0, 5.0) as u8;
            }
        }
    }
    fn reset(&mut self) {
        for d in &mut self.dv {
            for i in &mut d.inst {
                i.active = false;
            }
        }
        self.events.clear();
        self.pm = PatRun::default();
        self.last_end = f64::NAN;
    }
    fn tail(&self) -> f32 {
        4.0
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        // a pattern's length is a whole number of steps
        if (PAT_BASE..PAT_BASE + 2 * PATTERNS.len() as u32).contains(&id) && (id - PAT_BASE) % 2 == 0 {
            return Some(format!("{}", v.round()));
        }
        if !(VBASE..VBASE + 25 * VSTRIDE).contains(&id) {
            return None;
        }
        let vo = ((id - VBASE) / VSTRIDE) as usize;
        if vo != self.selected() {
            return None;
        }
        let k = (id - VBASE) % VSTRIDE;
        let time = |x: f32| {
            let t = kit::env_time(x, 10.0);
            if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.2} s") }
        };
        match k {
            F_CUT | NZ_CUT | F_RES | NZ_RES | NZ_DIRT | D_DRIVE | D_COLOR | D_LEVEL | O1_SAT | O1_ASYM | O2_SAT | O2_ASYM | O1_FM | M_STIFF | M_LOSS | M_RES | O1_SLOPE | O2_SLOPE => Some(format!("{v:.2}")),
            _ if (ENV0..ENV0 + 32).contains(&k) && matches!((k - ENV0) % 8, 0 | 2) => Some(time(v)),
            _ if (LFO0..LFO0 + 12).contains(&k) && (k - LFO0) % 6 == 2 => {
                let b = k - (k - LFO0) % 6;
                Some(if self.vp[vo][b as usize + 1] >= 0.5 {
                    let list = sync_list();
                    list[((v.clamp(0.0, 1.0) * (list.len() - 1) as f32).round()) as usize].0.clone()
                } else {
                    format!("{:.2} Hz", crate::ultrabeat_dsp::lfo_hz(false, v, 120.0))
                })
            }
            _ if (LFO0..LFO0 + 12).contains(&k) && (k - LFO0) % 6 == 5 => Some(format!("{:.0} ms", v * 2000.0)),
            _ => None,
        }
    }
    /// Logic's own parameter block for Ultrabeat isn't decoded: a project's Ultrabeat keeps
    /// the defaults.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for Ultrabeat {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.g[G_PMODE as usize] >= 0.5 && pitch < 24 {
            return self.pattern_note(pitch, true);
        }
        if self.g[G_MUTEMODE as usize] >= 0.5 && pitch >= 36 {
            let v = ((pitch - 36) as usize).min(24);
            self.vp[v][MUTE as usize] = if self.vp[v][MUTE as usize] >= 0.5 { 0.0 } else { 1.0 };
            return;
        }
        let (v, off) = if (36..60).contains(&pitch) {
            ((pitch - 36) as usize, 0.0)
        } else if pitch >= 60 {
            (24, (pitch - 60) as f32)
        } else {
            return;
        };
        if self.g[G_AUTOSEL as usize] >= 0.5 {
            self.g[G_SEL as usize] = v as f32;
        }
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev { at: 0, v, vel: velocity as f32 / 127.0, gain: 1.0, gate: -1, pitch: off, note: pitch as i16, off: false, lanes: [(0, 0.0); 4] });
        }
    }
    fn note_off(&mut self, pitch: u8) {
        if self.g[G_PMODE as usize] >= 0.5 && pitch < 24 {
            return self.pattern_note(pitch, false);
        }
        if self.events.len() < self.events.capacity() {
            self.events.push(Ev { at: 0, v: 0, vel: 0.0, gain: 1.0, gate: 0, pitch: 0.0, note: pitch as i16, off: true, lanes: [(0, 0.0); 4] });
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if let Some(i) = self.learn {
            if cc != 0 && cc < 120 {
                self.g[G_CTRL as usize + i] = cc as f32;
                self.learn = None;
            }
        }
        for i in 0..4 {
            if cc != 0 && self.g[G_CTRL as usize + i].round() as u8 == cc {
                self.g[G_CV as usize + i] = value as f32 / 127.0;
            }
        }
    }
    fn pressure(&mut self, value: u8) {
        for i in 0..4 {
            if self.g[G_CTRL as usize + i].round() as usize == kit::CTRL_AT {
                self.g[G_CV as usize + i] = value as f32 / 127.0;
            }
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let audible = self.voice_audible();
        self.gen_events(n);
        // events in time order (the list is short: insertion sort)
        for i in 1..self.events.len() {
            let mut j = i;
            while j > 0 && self.events[j - 1].at > self.events[j].at {
                self.events.swap(j - 1, j);
                j -= 1;
            }
        }
        let mut pos = 0;
        let mut k = 0;
        while k < self.events.len() {
            let e = self.events[k];
            let at = e.at.min(n);
            if at > pos {
                self.render_span(left, right, pos, at, &audible);
                pos = at;
            }
            if e.off {
                self.release_note(e.note);
            } else {
                self.trigger(&e);
            }
            k += 1;
        }
        self.events.clear();
        self.render_span(left, right, pos, n, &audible);
        self.side.done();
        for h in &mut self.hits {
            *h *= 0.97;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }
    fn set_transport(&mut self, t: Transport) {
        self.tr = t;
    }
    fn side_chain(&mut self, l: &[f32], r: &[f32]) {
        self.side.fill(l, r);
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
