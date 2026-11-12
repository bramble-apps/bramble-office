//! Sculpture: component modelling. A string (a two-way waveguide of a chosen material) is
//! excited, disturbed and damped by up to three objects, read by two pickups, then shaped per
//! voice by an amplitude envelope, a waveshaper and a multimode filter; the summed voices go
//! through the body EQ, a stereo delay and a limiter. Two LFOs, a vibrato LFO, two jitter
//! generators, two note-on randomisers, two velocity modulators, two (recordable) control
//! envelopes and the morph pad with its (recordable) morph envelope modulate it; there is no
//! central router, each source has its own targets, as in Logic's.

use crate::dsp::{self, Adsr, Fade, Rng, Svf, SvfMode};
use crate::poly::{Channel, Params};
use crate::sculpture_fx::*;
pub use crate::sculpture_fx::{BODY_MODELS, LFO_WAVES, NOTE_VALUES, WS_TYPES};
use crate::sculpture_string::Guide;
use lg_dsp::{Instrument, Param, Plugin, Transport, Unit};
use std::sync::OnceLock;

/// Samples between control updates (modulation, pitch, filter coefficients).
pub const CTRL: usize = 32;
const VOICES: usize = 28;
pub const POLY_MAX: usize = 16;

macro_rules! ids {
    ($start:expr; $($n:ident),+ $(,)?) => { ids!(@ $start; $($n),+); };
    (@ $c:expr; $n:ident $(, $r:ident)*) => { pub const $n: u32 = $c; ids!(@ $c + 1; $($r),*); };
    (@ $c:expr;) => {};
}

ids!(0; GLIDE, TUNE, WARMTH, TRANSPOSE, KBMODE, BEND_UP, BEND_DOWN, BEND_LINK, POLY,
    INNER_LOSS, STIFFNESS, MEDIA_LOSS, TENS_MOD, RESOLUTION,
    IL_KS_LO, IL_KS_HI, ST_KS_LO, ST_KS_HI, ML_KS_LO, ML_KS_HI, TM_KS_LO, TM_KS_HI, RES_KS_LO, RES_KS_HI,
    IL_REL, ML_REL, KS_VIEW,
    PICK_A, PICK_B, PICK_INVERT, SPREAD_PICK, SPREAD_KEY,
    WS_ON, WS_TYPE, WS_INPUT, WS_VAR,
    F_ON, F_TYPE, CUTOFF, RESONANCE, F_KEY, F_VELO,
    A_SOFT, A_HARD, A_DECAY, A_SUSTAIN, A_RELEASE,
    D_ON, D_TIME, D_NOTE, D_SYNC, D_FB, D_XFEED, D_WET, D_LOCUT, D_HICUT, D_INBAL, D_WIDTH, D_SPREAD, D_GROOVE,
    B_ON, B_MODEL, B_P1, B_P2, B_P3, B_P4,
    LEVEL, LIMITER);
ids!(100; VIB_WAVE, VIB_RATE, VIB_SYNC, VIB_NOTE, VIB_CURVE, VIB_PHASE, VIB_AMT, VIB_VIA,
    CTRL_A, CTRL_B, CTRL_VIB, CTRL_ENV1, CTRL_ENV2, CTRL_MX, CTRL_MY, CTRL_MODE,
    MONO_MODE, MONO_BEND, RENDER_MODE, SURR_RANGE, SURR_DIV,
    MORPH_SEL, MORPH_X, MORPH_Y, MORPH_AUTO, MORPH_POINTS, MORPH_INT, MORPH_RNDSET,
    MENV_MODE, MENV_SUB, MENV_TRIG, MENV_REC, MENV_SCALE, MENV_SYNC, MENV_REFBPM, MENV_DEPTH,
    MENV_MOD, MENV_MODSRC, MENV_TRANS, MENV_SUSMODE, MENV_LOOP, MENV_SUS, MENV_NSEL, ENV_SEL);

// ---- objects: 3 groups of fields
pub const OBJ_BASE: u32 = 200;
pub const OBJ_STRIDE: u32 = 10;
pub const O_ON: u32 = 0;
pub const O_TYPE: u32 = 1;
pub const O_GATE: u32 = 2;
pub const O_STR: u32 = 3;
pub const O_TIM: u32 = 4;
pub const O_VAR: u32 = 5;
pub const O_VEL: u32 = 6;
pub const O_POS: u32 = 7;
pub const fn obj(o: usize, f: u32) -> u32 {
    OBJ_BASE + o as u32 * OBJ_STRIDE + f
}
// ---- LFOs (two)
pub const LFO_BASE: u32 = 240;
pub const LFO_STRIDE: u32 = 24;
pub const L_ON: u32 = 0;
pub const L_WAVE: u32 = 1;
pub const L_RATE: u32 = 2;
pub const L_SYNC: u32 = 3;
pub const L_NOTE: u32 = 4;
pub const L_CURVE: u32 = 5;
pub const L_ENV: u32 = 6;
pub const L_PHASE: u32 = 7;
pub const L_RMSRC: u32 = 8;
pub const L_RMAMT: u32 = 9;
pub const fn lfo(i: usize, f: u32) -> u32 {
    LFO_BASE + i as u32 * LFO_STRIDE + f
}
/// A routing's fields: 0 on, 1 target, 2 via source, 3 amount, 4 via amount.
pub const fn lfo_r(i: usize, r: usize, f: u32) -> u32 {
    lfo(i, 10 + r as u32 * 7 + f)
}
pub const R_ON: u32 = 0;
pub const R_TARGET: u32 = 1;
pub const R_VIA: u32 = 2;
pub const R_AMT: u32 = 3;
pub const R_VIAAMT: u32 = 4;
// ---- jitter (two)
pub const JIT_BASE: u32 = 300;
pub const J_ON: u32 = 0;
pub const J_RATE: u32 = 1;
pub const J_T1: u32 = 2;
pub const J_A1: u32 = 3;
pub const J_T2: u32 = 4;
pub const J_A2: u32 = 5;
pub const fn jit(i: usize, f: u32) -> u32 {
    JIT_BASE + i as u32 * 8 + f
}
// ---- note-on random (two) and velocity modulators (two)
pub const NR_BASE: u32 = 320;
pub const N_ON: u32 = 0;
pub const N_TARGET: u32 = 1;
pub const N_AMT: u32 = 2;
pub const fn nr(i: usize, f: u32) -> u32 {
    NR_BASE + i as u32 * 4 + f
}
pub const VM_BASE: u32 = 330;
pub const V_ON: u32 = 0;
pub const V_TARGET: u32 = 1;
pub const V_AMT: u32 = 2;
pub const V_CURVE: u32 = 3;
pub const fn vmod(i: usize, f: u32) -> u32 {
    VM_BASE + i as u32 * 5 + f
}
// ---- Controller A / B modulation (two targets)
pub const CAB_BASE: u32 = 340;
pub const C_ON: u32 = 0;
pub const C_TARGET: u32 = 1;
pub const C_SRC: u32 = 2;
pub const C_MODE: u32 = 3;
pub const C_AMT: u32 = 4;
pub const fn cab(i: usize, f: u32) -> u32 {
    CAB_BASE + i as u32 * 6 + f
}
pub const CAB_SOURCES: &[&str] = &["Ctrl A", "Ctrl B", "Ctrl A+B"];
pub const CAB_MODES: &[&str] = &["Continue", "Note On"];
// ---- control envelopes (two)
pub const ENV_BASE: u32 = 360;
pub const ENV_STRIDE: u32 = 64;
pub const E_CTRL: u32 = 0;
pub const E_ENV: u32 = 1;
pub const E_SUS: u32 = 2;
pub const E_SCALE: u32 = 3;
pub const E_SYNC: u32 = 4;
pub const E_REFBPM: u32 = 5;
pub const E_ATV: u32 = 6;
pub const E_VMSRC: u32 = 7;
pub const E_VMAMT: u32 = 8;
pub const E_TRIG: u32 = 9;
pub const E_REC: u32 = 10;
pub const E_HASREC: u32 = 11;
pub const E_USEREC: u32 = 12;
pub const E_RECLEN: u32 = 13;
pub const fn env(i: usize, f: u32) -> u32 {
    ENV_BASE + i as u32 * ENV_STRIDE + f
}
pub const fn env_r(i: usize, r: usize, f: u32) -> u32 {
    env(i, 14 + r as u32 * 5 + f)
}
/// Segment times (ms), four of them.
pub const fn env_t(i: usize, k: usize) -> u32 {
    env(i, 24 + k as u32)
}
/// Node levels, five of them (start, attack, loop, sustain, end).
pub const fn env_l(i: usize, k: usize) -> u32 {
    env(i, 28 + k as u32)
}
pub const REC_POINTS: usize = 24;
/// The recorded curve's points.
pub const fn env_rec(i: usize, k: usize) -> u32 {
    env(i, 33 + k as u32)
}
// ---- morph points: 5 points x 23 values; the morph envelope's 9 nodes
pub const MS_BASE: u32 = 500;
pub const MORPH_N: usize = 23;
pub const fn ms(p: usize, k: usize) -> u32 {
    MS_BASE + (p * MORPH_N + k) as u32
}
pub const MN_BASE: u32 = 620;
/// Morph envelope node `i`: field 0 segment time in ms (from the node before), 1 x, 2 y.
pub const fn mn(i: usize, f: u32) -> u32 {
    MN_BASE + i as u32 * 3 + f
}

/// The morphable parameters, in the order of a morph point's stored values.
pub const MORPH_IDS: [u32; MORPH_N] = [
    obj(0, O_STR), obj(0, O_TIM), obj(0, O_VAR), obj(0, O_POS),
    obj(1, O_STR), obj(1, O_TIM), obj(1, O_VAR), obj(1, O_POS),
    obj(2, O_STR), obj(2, O_TIM), obj(2, O_VAR), obj(2, O_POS),
    PICK_A, PICK_B, INNER_LOSS, STIFFNESS, MEDIA_LOSS, TENS_MOD, RESOLUTION,
    WS_INPUT, WS_VAR, CUTOFF, RESONANCE,
];

fn morph_index(id: u32) -> Option<usize> {
    MORPH_IDS.iter().position(|&m| m == id)
}

pub const MORPH_POINT_NAMES: [&str; 5] = ["A", "B", "C", "D", "Center"];

// ---- choices
pub const KB_MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const F_TYPES: &[&str] = &["HiPass", "LoPass", "Peak", "BandPass", "Notch"];
pub const GATES: &[&str] = &["KeyOn", "Always", "KeyOff"];
pub const OBJ1_TYPES: &[&str] = &["Impulse", "Strike", "GravStrike", "Pick", "Bow", "Bow wide", "Noise", "Blow"];
pub const OBJ2_TYPES: &[&str] = &[
    "Impulse", "Strike", "GravStrike", "Pick", "Bow", "Bow wide", "Noise", "Blow", "External", "Disturb",
    "Disturb 2-sided", "Bouncing", "Bound", "Mass", "Damp",
];
pub const OBJ3_TYPES: &[&str] = &["Disturb", "Disturb 2-sided", "Bouncing", "Bound", "Mass", "Damp"];
/// The object kind (1 Impulse … 15 Damp) behind a type menu entry of object `o`.
pub fn kind_of(o: usize, ty: usize) -> u8 {
    let k = if o == 2 { 10 + ty } else { 1 + ty };
    let max = if o == 0 { 8 } else { 15 };
    k.min(max) as u8
}
pub fn types_of(o: usize) -> &'static [&'static str] {
    match o {
        0 => OBJ1_TYPES,
        1 => OBJ2_TYPES,
        _ => OBJ3_TYPES,
    }
}
pub const TARGETS: &[&str] = &[
    "Off", "Object1 Strength", "Object1 Timbre", "Object1 Variation", "Object1 Position", "Object2 Strength",
    "Object2 Timbre", "Object2 Variation", "Object2 Position", "Object3 Strength", "Object3 Timbre",
    "Object3 Variation", "Object3 Position", "PickupA Position", "PickupB Position", "Pickup Pos A+B",
    "Pickup Pos A-B", "Pickup Pan A+B", "Pickup Pan A-B", "String Inner Loss", "String Stiffness",
    "String Media Loss", "String Tension Mod", "Pitch", "Waveshaper Input", "Waveshaper Variation",
    "Filter Cutoff", "Filter Resonance", "Level",
];
pub const NT: usize = 29;
pub const VIA_SOURCES: &[&str] = &[
    "Off", "Velocity Concave", "Velocity", "Velocity Convex", "KeyScale", "Ctrl A (1 ModWhl)", "Ctrl B (4 Foot)",
    "LFO 1", "LFO 2", "Env 1", "Env 2", "Aftertouch",
];
pub const ENV_SUS_MODES: &[&str] = &["Sustain", "Finish", "Loop Forward", "Loop Backward", "Loop Alternate"];
pub const MENV_SUS_MODES: &[&str] =
    &["Sustain", "Finish", "Loop Forward", "Loop Backward", "Loop Alternate", "Scan via CtrlB"];
pub const REC_TRIGGERS: &[&str] = &["NoteOn", "Note + Ctrl Movement", "Note + Sustain Pedal"];
pub const MORPH_TRIGGERS: &[&str] = &["NoteOn", "Note + Move Morph Point", "Note + Sustain Pedal"];
pub const MENV_MODES: &[&str] = &["Off", "Pad only", "Env only", "Env + Pad"];
pub const MENV_SUBS: &[&str] = &["Offset", "Point Set", "Point Solo"];
pub const MORPH_RND: &[&str] = &[
    "All morphable parameters", "All except TensMod", "String Material/Media", "Objects&Pickups", "Waveshaper&Filter",
];
pub const MORPH_POINTS_MODES: &[&str] = &["Selected point", "Four corners", "All five points"];
pub const VM_CURVES: &[&str] = &["Concave", "Linear", "Convex"];
pub const KS_VIEWS: &[&str] = &["Hide", "Keyscale", "Release"];
pub const MONO_MODES: &[&str] = &["Off", "On, common base channel 1", "On, common base channel 16"];
pub const RENDER_MODES: &[&str] = &["Basic", "Extended", "High Definition"];
pub const CTRL_MODES: &[&str] = &["Use Default", "Load From Setting"];
pub const LIMITERS: &[&str] = &["Off", "Mono", "Poly", "Both"];
pub const TRANSPOSES: &[&str] = &["-3 Oct.", "-2 Oct.", "-1 Oct.", "0", "+1 Oct.", "+2 Oct.", "+3 Oct."];

/// Controller number 128 is "Off".
pub const CC_OFF: u32 = 128;

pub fn cc_name(cc: u32) -> String {
    match cc {
        1 => "1 ModWhl".into(),
        2 => "2 Breath".into(),
        4 => "4 Foot".into(),
        7 => "7 Volume".into(),
        10 => "10 Pan".into(),
        11 => "11 Express.".into(),
        64 => "64 Sustain".into(),
        74 => "74 Bright".into(),
        128 => "Off".into(),
        n => format!("{n}"),
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The parameter table (built once, names leaked).
pub fn params_table() -> &'static [Param] {
    static T: OnceLock<Vec<Param>> = OnceLock::new();
    T.get_or_init(build_params)
}

fn build_params() -> Vec<Param> {
    let mut v: Vec<Param> = Vec::new();
    let n = Param::new;
    let o = Unit::OnOff;
    let r = Unit::Ratio;
    // global
    v.push(n(GLIDE, "Glide Time", 0.0, 2.0, 0.0, Unit::Seconds));
    v.push(n(TUNE, "Tune", -100.0, 100.0, 0.0, Unit::Cents));
    v.push(n(WARMTH, "Warmth", 0.0, 1.0, 0.0, r));
    v.push(n(TRANSPOSE, "Transpose", -3.0, 3.0, 0.0, Unit::None));
    v.push(Param::menu(KBMODE, "Keyboard Mode", KB_MODES, 0.0));
    v.push(n(BEND_UP, "Bender Range Up", 0.0, 24.0, 2.0, Unit::Semitones));
    v.push(n(BEND_DOWN, "Bender Range Down", 0.0, 24.0, 2.0, Unit::Semitones));
    v.push(n(BEND_LINK, "Bender Range Linked", 0.0, 1.0, 1.0, o));
    v.push(n(POLY, "Polyphony", 1.0, POLY_MAX as f32, 16.0, Unit::None));
    // string
    v.push(n(INNER_LOSS, "Inner Loss", 0.0, 1.0, 0.3, r));
    v.push(n(STIFFNESS, "Stiffness", 0.0, 1.0, 0.2, r));
    v.push(n(MEDIA_LOSS, "Media Loss", 0.0, 1.0, 0.3, r));
    v.push(n(TENS_MOD, "Tension Mod", 0.0, 1.0, 0.0, r));
    v.push(n(RESOLUTION, "Resolution", 0.0, 1.0, 0.6, r));
    for (id, name) in [
        (IL_KS_LO, "Inner Loss Keyscale Low"), (IL_KS_HI, "Inner Loss Keyscale High"),
        (ST_KS_LO, "Stiffness Keyscale Low"), (ST_KS_HI, "Stiffness Keyscale High"),
        (ML_KS_LO, "Media Loss Keyscale Low"), (ML_KS_HI, "Media Loss Keyscale High"),
        (TM_KS_LO, "Tension Mod Keyscale Low"), (TM_KS_HI, "Tension Mod Keyscale High"),
        (RES_KS_LO, "Resolution Keyscale Low"), (RES_KS_HI, "Resolution Keyscale High"),
    ] {
        v.push(n(id, name, -1.0, 1.0, 0.0, r));
    }
    v.push(n(IL_REL, "Inner Loss Release", 0.0, 2.0, 1.0, Unit::None));
    v.push(n(ML_REL, "Media Loss Release", 0.0, 2.0, 1.0, Unit::None));
    v.push(Param::menu(KS_VIEW, "String View", KS_VIEWS, 1.0).hide());
    // pickups
    v.push(n(PICK_A, "Pickup A", 0.0, 1.0, 0.2, r));
    v.push(n(PICK_B, "Pickup B", 0.0, 1.0, 0.55, r));
    v.push(n(PICK_INVERT, "Pickup Invert", 0.0, 1.0, 0.0, o));
    v.push(n(SPREAD_PICK, "Spread Pickup", 0.0, 1.0, 0.0, r));
    v.push(n(SPREAD_KEY, "Spread Key", -1.0, 1.0, 0.0, r));
    // waveshaper, filter
    v.push(n(WS_ON, "Waveshaper On", 0.0, 1.0, 0.0, o));
    v.push(Param::menu(WS_TYPE, "Waveshaper Type", WS_TYPES, 0.0));
    v.push(n(WS_INPUT, "Waveshaper Input Scale", -1.0, 1.0, 0.0, r));
    v.push(n(WS_VAR, "Waveshaper Variation", -1.0, 1.0, 0.0, r));
    v.push(n(F_ON, "Filter On", 0.0, 1.0, 1.0, o));
    v.push(Param::menu(F_TYPE, "Filter Type", F_TYPES, 1.0));
    v.push(n(CUTOFF, "Filter Cutoff", 0.0, 1.0, 1.0, r));
    v.push(n(RESONANCE, "Filter Resonance", 0.0, 1.0, 0.1, r));
    v.push(n(F_KEY, "Filter Key", 0.0, 1.0, 0.0, r));
    v.push(n(F_VELO, "Filter Velo Sens", 0.0, 1.0, 0.0, r));
    // amplitude envelope
    v.push(n(A_SOFT, "Attack Soft", 0.0, 3.0, 0.0, Unit::Seconds));
    v.push(n(A_HARD, "Attack Hard", 0.0, 3.0, 0.0, Unit::Seconds));
    v.push(n(A_DECAY, "Decay", 0.005, 10.0, 1.0, Unit::Seconds));
    v.push(n(A_SUSTAIN, "Sustain", 0.0, 1.0, 1.0, r));
    v.push(n(A_RELEASE, "Release", 0.005, 10.0, 0.3, Unit::Seconds));
    // delay
    v.push(n(D_ON, "Delay On", 0.0, 1.0, 0.0, o));
    v.push(n(D_TIME, "Delay Time", 1.0, 2000.0, 300.0, Unit::Ms));
    v.push(Param::new(D_NOTE, "Delay Note", 0.0, (NOTE_VALUES.len() - 1) as f32, 12.0, Unit::None));
    v.push(n(D_SYNC, "Delay Sync", 0.0, 1.0, 0.0, o));
    v.push(n(D_FB, "Delay Feedback", -1.0, 1.0, 0.3, r));
    v.push(n(D_XFEED, "Delay Xfeed", -1.0, 1.0, 0.0, r));
    v.push(n(D_WET, "Delay Wet Level", 0.0, 1.0, 0.3, r));
    v.push(n(D_LOCUT, "Delay LoCut", 20.0, 2000.0, 40.0, Unit::Hz));
    v.push(n(D_HICUT, "Delay HiCut", 200.0, 20000.0, 12000.0, Unit::Hz));
    v.push(n(D_INBAL, "Delay Input Balance", -1.0, 1.0, 0.0, r));
    v.push(n(D_WIDTH, "Delay Output Width", 0.0, 1.0, 1.0, r));
    v.push(n(D_SPREAD, "Delay Spread", -1.0, 1.0, 0.0, r));
    v.push(n(D_GROOVE, "Delay Groove", -1.0, 1.0, 0.0, r));
    // body EQ
    v.push(n(B_ON, "Body EQ On", 0.0, 1.0, 0.0, o));
    v.push(Param::menu(B_MODEL, "Body EQ Model", BODY_MODELS, 0.0));
    v.push(n(B_P1, "Body EQ Low / Intensity", -1.0, 1.0, 0.0, r));
    v.push(n(B_P2, "Body EQ Mid / Shift", -1.0, 1.0, 0.0, r));
    v.push(n(B_P3, "Body EQ High / Stretch", -1.0, 1.0, 0.0, r));
    v.push(n(B_P4, "Body EQ Mid Frequency / Fine Structure", 0.0, 1.0, 0.5, r));
    v.push(n(LEVEL, "Level", -60.0, 12.0, 0.0, Unit::Db));
    v.push(Param::menu(LIMITER, "Limiter", LIMITERS, 1.0));
    // vibrato
    v.push(Param::menu(VIB_WAVE, "Vibrato Waveform", LFO_WAVES, 0.0));
    v.push(n(VIB_RATE, "Vibrato Rate", 0.1, 20.0, 5.5, Unit::Hz));
    v.push(n(VIB_SYNC, "Vibrato Sync", 0.0, 1.0, 0.0, o));
    v.push(Param::new(VIB_NOTE, "Vibrato Note", 0.0, (NOTE_VALUES.len() - 1) as f32, 9.0, Unit::None));
    v.push(n(VIB_CURVE, "Vibrato Curve", -1.0, 1.0, 0.0, r));
    v.push(n(VIB_PHASE, "Vibrato Phase", 0.0, 1.0, 0.0, r));
    v.push(n(VIB_AMT, "Vibrato Amount", 0.0, 1.0, 0.5, r));
    v.push(n(VIB_VIA, "Vibrato Via Ctrl", 0.0, 1.0, 1.0, r));
    // controllers
    for (id, name, d) in [
        (CTRL_A, "Ctrl A", 1.0), (CTRL_B, "Ctrl B", 4.0), (CTRL_VIB, "Vib Depth Ctrl", 1.0),
        (CTRL_ENV1, "CtrlEnv 1", 1.0), (CTRL_ENV2, "CtrlEnv 2", 4.0), (CTRL_MX, "Morph X", 128.0),
        (CTRL_MY, "Morph Y", 128.0),
    ] {
        v.push(n(id, name, 0.0, 128.0, d, Unit::None));
    }
    v.push(Param::menu(CTRL_MODE, "Controller Mode", CTRL_MODES, 0.0));
    v.push(Param::menu(MONO_MODE, "MIDI Mono Mode", MONO_MODES, 0.0));
    v.push(n(MONO_BEND, "Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones));
    v.push(Param::menu(RENDER_MODE, "Render Mode", RENDER_MODES, 1.0));
    v.push(n(SURR_RANGE, "Surround Range", 0.0, 360.0, 360.0, Unit::None));
    v.push(n(SURR_DIV, "Surround Diversity", 0.0, 1.0, 0.5, r));
    // morph
    v.push(Param::new(MORPH_SEL, "Morph Point", 0.0, 4.0, 4.0, Unit::None).hide());
    v.push(n(MORPH_X, "Morph X", 0.0, 1.0, 0.5, r));
    v.push(n(MORPH_Y, "Morph Y", 0.0, 1.0, 0.5, r));
    v.push(n(MORPH_AUTO, "Morph Auto Select", 0.0, 1.0, 1.0, o));
    v.push(Param::menu(MORPH_POINTS, "Morph Random Points", MORPH_POINTS_MODES, 2.0));
    v.push(n(MORPH_INT, "Morph Random Intensity", 0.01, 1.0, 0.3, Unit::Percent));
    v.push(Param::menu(MORPH_RNDSET, "Morph Random Set", MORPH_RND, 0.0));
    v.push(Param::menu(MENV_MODE, "Morph Env Mode", MENV_MODES, 1.0));
    v.push(Param::menu(MENV_SUB, "Morph Env Pad Mode", MENV_SUBS, 0.0));
    v.push(Param::menu(MENV_TRIG, "Morph Env Trigger", MORPH_TRIGGERS, 0.0));
    v.push(n(MENV_REC, "Morph Env Record", 0.0, 1.0, 0.0, o));
    v.push(n(MENV_SCALE, "Morph Env Timescale", 10.0, 1000.0, 100.0, Unit::Percent));
    v.push(n(MENV_SYNC, "Morph Env Sync", 0.0, 1.0, 0.0, o));
    v.push(n(MENV_REFBPM, "Morph Env Reference Tempo", 20.0, 400.0, 120.0, Unit::None).hide());
    v.push(n(MENV_DEPTH, "Morph Env Depth", 0.0, 2.0, 1.0, r));
    v.push(n(MENV_MOD, "Morph Env Modulation", 0.0, 1.0, 0.0, r));
    v.push(Param::menu(MENV_MODSRC, "Morph Env Mod Source", VIA_SOURCES, 0.0));
    v.push(n(MENV_TRANS, "Morph Env Transition", 0.0, 1.0, 0.5, r));
    v.push(Param::menu(MENV_SUSMODE, "Morph Env Sustain Mode", MENV_SUS_MODES, 0.0));
    v.push(n(MENV_LOOP, "Morph Env Loop Node", 0.0, 8.0, 3.0, Unit::None));
    v.push(n(MENV_SUS, "Morph Env Sustain Node", 0.0, 8.0, 6.0, Unit::None));
    v.push(n(MENV_NSEL, "Morph Env Selected Node", 0.0, 8.0, 0.0, Unit::None).hide());
    v.push(n(ENV_SEL, "Envelope Shown", 0.0, 1.0, 0.0, Unit::None).hide());
    // objects
    for k in 0..3usize {
        let nm = |s: &str| leak(format!("Object {} {s}", k + 1));
        v.push(n(obj(k, O_ON), nm("On"), 0.0, 1.0, if k == 0 { 1.0 } else { 0.0 }, o));
        let ty = if k == 2 { 5.0 } else { 0.0 };
        v.push(Param::menu(obj(k, O_TYPE), nm("Type"), types_of(k), ty));
        v.push(Param::menu(obj(k, O_GATE), nm("Gate"), GATES, if k == 2 { 2.0 } else { 0.0 }));
        v.push(n(obj(k, O_STR), nm("Strength"), 0.0, 1.0, [0.8, 0.5, 0.5][k], r));
        v.push(n(obj(k, O_TIM), nm("Timbre"), -1.0, 1.0, 0.0, r));
        v.push(n(obj(k, O_VAR), nm("Variation"), -1.0, 1.0, 0.0, r));
        v.push(n(obj(k, O_VEL), nm("VeloSens"), 0.0, 1.0, 0.7, r));
        v.push(n(obj(k, O_POS), nm("Position"), 0.0, 1.0, [0.15, 0.35, 0.6][k], r));
    }
    // LFOs
    for i in 0..2usize {
        let nm = |s: &str| leak(format!("LFO {} {s}", i + 1));
        v.push(n(lfo(i, L_ON), nm("On"), 0.0, 1.0, 0.0, o));
        v.push(Param::menu(lfo(i, L_WAVE), nm("Waveform"), LFO_WAVES, 0.0));
        v.push(n(lfo(i, L_RATE), nm("Rate"), 0.0, 60.0, 5.0, Unit::Hz));
        v.push(n(lfo(i, L_SYNC), nm("Sync"), 0.0, 1.0, 0.0, o));
        v.push(Param::new(lfo(i, L_NOTE), nm("Note"), 0.0, (NOTE_VALUES.len() - 1) as f32, 9.0, Unit::None));
        v.push(n(lfo(i, L_CURVE), nm("Curve"), -1.0, 1.0, 0.0, r));
        v.push(n(lfo(i, L_ENV), nm("Envelope"), -1.0, 1.0, 0.0, r));
        v.push(n(lfo(i, L_PHASE), nm("Phase"), 0.0, 1.0, 0.0, r));
        v.push(Param::menu(lfo(i, L_RMSRC), nm("RateMod Source"), VIA_SOURCES, 0.0));
        v.push(n(lfo(i, L_RMAMT), nm("RateMod Amount"), 0.0, 1.0, 0.0, r));
        for rt in 0..2usize {
            let nr_ = |s: &str| leak(format!("LFO {} Mod {} {s}", i + 1, rt + 1));
            v.push(n(lfo_r(i, rt, R_ON), nr_("On"), 0.0, 1.0, 0.0, o));
            v.push(Param::menu(lfo_r(i, rt, R_TARGET), nr_("Target"), TARGETS, 0.0));
            v.push(Param::menu(lfo_r(i, rt, R_VIA), nr_("Via"), VIA_SOURCES, 0.0));
            v.push(n(lfo_r(i, rt, R_AMT), nr_("Amount"), -1.0, 1.0, 0.0, r));
            v.push(n(lfo_r(i, rt, R_VIAAMT), nr_("Via Amount"), -1.0, 1.0, 0.0, r));
        }
    }
    for i in 0..2usize {
        let nm = |s: &str| leak(format!("Controller {} {s}", i + 1));
        v.push(n(cab(i, C_ON), nm("On"), 0.0, 1.0, 0.0, o));
        v.push(Param::menu(cab(i, C_TARGET), nm("Target"), TARGETS, 0.0));
        v.push(Param::menu(cab(i, C_SRC), nm("Source"), CAB_SOURCES, i as f32));
        v.push(Param::menu(cab(i, C_MODE), nm("Mode"), CAB_MODES, 0.0));
        v.push(n(cab(i, C_AMT), nm("Amount"), -1.0, 1.0, 0.0, r));
    }
    // jitter
    for i in 0..2usize {
        let nm = |s: &str| leak(format!("Jitter {} {s}", i + 1));
        v.push(n(jit(i, J_ON), nm("On"), 0.0, 1.0, 0.0, o));
        v.push(n(jit(i, J_RATE), nm("Rate"), 0.05, 100.0, 8.0, Unit::Hz));
        v.push(Param::menu(jit(i, J_T1), nm("Target 1"), TARGETS, 0.0));
        v.push(n(jit(i, J_A1), nm("Amount 1"), -1.0, 1.0, 0.0, r));
        v.push(Param::menu(jit(i, J_T2), nm("Target 2"), TARGETS, 0.0));
        v.push(n(jit(i, J_A2), nm("Amount 2"), -1.0, 1.0, 0.0, r));
    }
    for i in 0..2usize {
        let nm = |s: &str| leak(format!("Note-on Random {} {s}", i + 1));
        v.push(n(nr(i, N_ON), nm("On"), 0.0, 1.0, 0.0, o));
        v.push(Param::menu(nr(i, N_TARGET), nm("Target"), TARGETS, 0.0));
        v.push(n(nr(i, N_AMT), nm("Amount"), -1.0, 1.0, 0.0, r));
        let nm = |s: &str| leak(format!("Velocity Mod {} {s}", i + 1));
        v.push(n(vmod(i, V_ON), nm("On"), 0.0, 1.0, 0.0, o));
        v.push(Param::menu(vmod(i, V_TARGET), nm("Target"), TARGETS, 0.0));
        v.push(n(vmod(i, V_AMT), nm("Amount"), -1.0, 1.0, 0.0, r));
        v.push(Param::menu(vmod(i, V_CURVE), nm("Curve"), VM_CURVES, 1.0));
    }
    // control envelopes
    for i in 0..2usize {
        let nm = |s: &str| leak(format!("Envelope {} {s}", i + 1));
        v.push(n(env(i, E_CTRL), nm("Ctrl Mode"), 0.0, 1.0, 0.0, o));
        v.push(n(env(i, E_ENV), nm("Env Mode"), 0.0, 1.0, 1.0, o));
        v.push(Param::menu(env(i, E_SUS), nm("Sustain Mode"), ENV_SUS_MODES, 0.0));
        v.push(n(env(i, E_SCALE), nm("Timescale"), 10.0, 1000.0, 100.0, Unit::Percent));
        v.push(n(env(i, E_SYNC), nm("Sync"), 0.0, 1.0, 0.0, o));
        v.push(n(env(i, E_REFBPM), nm("Reference Tempo"), 20.0, 400.0, 120.0, Unit::None).hide());
        v.push(n(env(i, E_ATV), nm("A-Time Velosens"), -1.0, 1.0, 0.0, r));
        v.push(Param::menu(env(i, E_VMSRC), nm("VariMod Source"), VARIMOD_SOURCES, 0.0));
        v.push(n(env(i, E_VMAMT), nm("VariMod Amount"), -1.0, 1.0, 0.0, r));
        v.push(Param::menu(env(i, E_TRIG), nm("Record Trigger"), REC_TRIGGERS, 0.0));
        v.push(n(env(i, E_REC), nm("Record"), 0.0, 1.0, 0.0, o));
        v.push(n(env(i, E_HASREC), nm("Has Recording"), 0.0, 1.0, 0.0, o).hide());
        v.push(n(env(i, E_USEREC), nm("Use Recording"), 0.0, 1.0, 0.0, o).hide());
        v.push(n(env(i, E_RECLEN), nm("Recorded Length"), 10.0, 40000.0, 1000.0, Unit::Ms).hide());
        for rt in 0..2usize {
            let nr_ = |s: &str| leak(format!("Envelope {} Mod {} {s}", i + 1, rt + 1));
            v.push(n(env_r(i, rt, R_ON), nr_("On"), 0.0, 1.0, 0.0, o));
            v.push(Param::menu(env_r(i, rt, R_TARGET), nr_("Target"), TARGETS, 0.0));
            v.push(Param::menu(env_r(i, rt, R_VIA), nr_("Via"), VIA_SOURCES, 0.0));
            v.push(n(env_r(i, rt, R_AMT), nr_("Amount"), -1.0, 1.0, 0.0, r));
            v.push(n(env_r(i, rt, R_VIAAMT), nr_("Via Amount"), -1.0, 1.0, 0.0, r));
        }
        let dt = [20.0, 250.0, 400.0, 600.0];
        let dl = [0.0, 1.0, 0.6, 0.4, 0.0];
        for k in 0..4 {
            v.push(n(env_t(i, k), leak(format!("Envelope {} Segment {} Time", i + 1, k + 1)), 1.0, 40000.0, dt[k], Unit::Ms).hide());
        }
        for k in 0..5 {
            v.push(n(env_l(i, k), leak(format!("Envelope {} Node {} Level", i + 1, k + 1)), 0.0, 1.0, dl[k], r).hide());
        }
        for k in 0..REC_POINTS {
            v.push(n(env_rec(i, k), leak(format!("Envelope {} Recorded {}", i + 1, k + 1)), 0.0, 1.0, 0.0, r).hide());
        }
    }
    // morph points: all five start as the defaults of the morphable parameters
    let defaults: Vec<f32> = MORPH_IDS.iter().map(|&id| v.iter().find(|p| p.id == id).map(|p| p.default).unwrap_or(0.0)).collect();
    let ranges: Vec<(f32, f32)> = MORPH_IDS.iter().map(|&id| v.iter().find(|p| p.id == id).map(|p| (p.min, p.max)).unwrap_or((0.0, 1.0))).collect();
    for p in 0..5usize {
        for k in 0..MORPH_N {
            let nm = leak(format!("Morph Point {} {}", MORPH_POINT_NAMES[p], v.iter().find(|q| q.id == MORPH_IDS[k]).map(|q| q.name).unwrap_or("")));
            v.push(n(ms(p, k), nm, ranges[k].0, ranges[k].1, defaults[k], Unit::None).hide());
        }
    }
    for i in 0..9usize {
        v.push(n(mn(i, 0), leak(format!("Morph Env Node {} Time", i + 1)), 1.0, 40000.0, 250.0, Unit::Ms).hide());
        v.push(n(mn(i, 1), leak(format!("Morph Env Node {} X", i + 1)), 0.0, 1.0, 0.5, Unit::None).hide());
        v.push(n(mn(i, 2), leak(format!("Morph Env Node {} Y", i + 1)), 0.0, 1.0, 0.5, Unit::None).hide());
    }
    v
}

pub const VARIMOD_SOURCES: &[&str] =
    &["Off", "Velocity Concave", "Velocity", "Velocity Convex", "KeyScale", "Ctrl A", "Ctrl B"];

/// The via-source index (into `VIA_SOURCES`) a VariMod menu entry stands for.
fn varimod_via(i: usize) -> usize {
    i.min(6)
}

// =============================================================================================
// Derived settings (what the voices read)

#[derive(Clone, Copy, Default)]
struct Route {
    on: bool,
    target: usize,
    via: usize,
    amt: f32,
    viaamt: f32,
}

impl Route {
    fn load(p: &Params, f: impl Fn(u32) -> u32) -> Route {
        Route {
            on: p.get(f(R_ON)) >= 0.5,
            target: p.get(f(R_TARGET)).round() as usize,
            via: p.get(f(R_VIA)).round() as usize,
            amt: p.get(f(R_AMT)),
            viaamt: p.get(f(R_VIAAMT)),
        }
    }
    /// Amt with the via signal at zero, Via amount with it at its maximum.
    fn amount(&self, via: f32) -> f32 {
        if self.via == 0 {
            self.amt
        } else {
            self.amt + (self.viaamt - self.amt) * via
        }
    }
}

#[derive(Clone, Copy, Default)]
struct LfoCfg {
    on: bool,
    wave: usize,
    rate: f32,
    sync: bool,
    note: usize,
    curve: f32,
    env: f32,
    phase: f32,
    rmsrc: usize,
    rmamt: f32,
    r: [Route; 2],
}

#[derive(Clone, Copy, Default)]
struct JitCfg {
    on: bool,
    rate: f32,
    t: [usize; 2],
    a: [f32; 2],
}

#[derive(Clone, Copy, Default)]
struct EnvCfg {
    ctrl: bool,
    env: bool,
    sus: usize,
    scale: f32,
    sync: bool,
    refbpm: f32,
    atv: f32,
    vmsrc: usize,
    vmamt: f32,
    has_rec: bool,
    use_rec: bool,
    rec_len: f32,
    t: [f32; 4],
    l: [f32; 5],
    rec: [f32; REC_POINTS],
    r: [Route; 2],
}

#[derive(Clone, Copy, Default)]
struct MenvCfg {
    mode: usize,
    sub: usize,
    scale: f32,
    sync: bool,
    refbpm: f32,
    depth: f32,
    modamt: f32,
    modsrc: usize,
    trans: f32,
    sus: usize,
    loop_i: usize,
    sus_i: usize,
    nsel: usize,
    t: [f32; 9],
    x: [f32; 9],
    y: [f32; 9],
}

#[derive(Clone, Copy, Default)]
struct ObjCfg {
    on: bool,
    kind: u8,
    gate: u8,
    vel: f32,
}

#[derive(Clone)]
struct Sh {
    rate: f32,
    ovs: usize,
    glide: f32,
    tune: f32,
    warmth: f32,
    transpose: f32,
    kb: u8,
    poly: usize,
    morph: [[f32; MORPH_N]; 5],
    sel: usize,
    px: f32,
    py: f32,
    ks: [f32; 10],
    il_rel: f32,
    ml_rel: f32,
    invert: bool,
    spread_pick: f32,
    spread_key: f32,
    ws_on: bool,
    ws_ty: usize,
    f_on: bool,
    f_ty: usize,
    f_key: f32,
    f_velo: f32,
    a_soft: f32,
    a_hard: f32,
    a_decay: f32,
    a_sus: f32,
    a_rel: f32,
    objs: [ObjCfg; 3],
    lfo: [LfoCfg; 2],
    vib_wave: usize,
    vib_rate: f32,
    vib_sync: bool,
    vib_note: usize,
    vib_curve: f32,
    vib_phase: f32,
    vib_amt: f32,
    vib_via: f32,
    jit: [JitCfg; 2],
    nr: [(bool, usize, f32); 2],
    vm: [(bool, usize, f32, usize); 2],
    cab: [(bool, usize, usize, usize, f32); 2],
    env: [EnvCfg; 2],
    menv: MenvCfg,
    cc_a: u32,
    cc_b: u32,
    cc_vib: u32,
    cc_env: [u32; 2],
    cc_mx: u32,
    cc_my: u32,
    mono_mode: usize,
    mono_bend: f32,
    render_mode: usize,
    limiter: usize,
}

impl Sh {
    fn load(p: &Params, rate: f32) -> Sh {
        let g = |id: u32| p.get(id);
        let b = |id: u32| p.get(id) >= 0.5;
        let u = |id: u32| p.get(id).round().max(0.0) as usize;
        let mut morph = [[0.0; MORPH_N]; 5];
        for (pi, row) in morph.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                *v = g(ms(pi, k));
            }
        }
        let mut objs = [ObjCfg::default(); 3];
        for (o, c) in objs.iter_mut().enumerate() {
            c.on = b(obj(o, O_ON));
            c.kind = if c.on { kind_of(o, u(obj(o, O_TYPE))) } else { 0 };
            c.gate = u(obj(o, O_GATE)).min(2) as u8;
            c.vel = g(obj(o, O_VEL));
        }
        let mut lfos = [LfoCfg::default(); 2];
        for (i, c) in lfos.iter_mut().enumerate() {
            *c = LfoCfg {
                on: b(lfo(i, L_ON)),
                wave: u(lfo(i, L_WAVE)),
                rate: g(lfo(i, L_RATE)),
                sync: b(lfo(i, L_SYNC)),
                note: u(lfo(i, L_NOTE)),
                curve: g(lfo(i, L_CURVE)),
                env: g(lfo(i, L_ENV)),
                phase: g(lfo(i, L_PHASE)),
                rmsrc: u(lfo(i, L_RMSRC)),
                rmamt: g(lfo(i, L_RMAMT)),
                r: [Route::load(p, |f| lfo_r(i, 0, f)), Route::load(p, |f| lfo_r(i, 1, f))],
            };
        }
        let mut jits = [JitCfg::default(); 2];
        for (i, c) in jits.iter_mut().enumerate() {
            *c = JitCfg {
                on: b(jit(i, J_ON)),
                rate: g(jit(i, J_RATE)),
                t: [u(jit(i, J_T1)), u(jit(i, J_T2))],
                a: [g(jit(i, J_A1)), g(jit(i, J_A2))],
            };
        }
        let mut envs = [EnvCfg::default(); 2];
        for (i, c) in envs.iter_mut().enumerate() {
            c.ctrl = b(env(i, E_CTRL));
            c.env = b(env(i, E_ENV));
            c.sus = u(env(i, E_SUS)).min(4);
            c.scale = g(env(i, E_SCALE));
            c.sync = b(env(i, E_SYNC));
            c.refbpm = g(env(i, E_REFBPM));
            c.atv = g(env(i, E_ATV));
            c.vmsrc = u(env(i, E_VMSRC));
            c.vmamt = g(env(i, E_VMAMT));
            c.has_rec = b(env(i, E_HASREC));
            c.use_rec = b(env(i, E_USEREC)) && c.has_rec;
            c.rec_len = g(env(i, E_RECLEN));
            for k in 0..4 {
                c.t[k] = g(env_t(i, k));
            }
            for k in 0..5 {
                c.l[k] = g(env_l(i, k));
            }
            for k in 0..REC_POINTS {
                c.rec[k] = g(env_rec(i, k));
            }
            c.r = [Route::load(p, |f| env_r(i, 0, f)), Route::load(p, |f| env_r(i, 1, f))];
        }
        let mut menv = MenvCfg {
            mode: u(MENV_MODE),
            sub: u(MENV_SUB),
            scale: g(MENV_SCALE),
            sync: b(MENV_SYNC),
            refbpm: g(MENV_REFBPM),
            depth: g(MENV_DEPTH),
            modamt: g(MENV_MOD),
            modsrc: u(MENV_MODSRC),
            trans: g(MENV_TRANS),
            sus: u(MENV_SUSMODE).min(5),
            loop_i: u(MENV_LOOP).min(8),
            sus_i: u(MENV_SUS).min(8),
            nsel: u(MENV_NSEL).min(8),
            ..Default::default()
        };
        for i in 0..9 {
            menv.t[i] = g(mn(i, 0));
            menv.x[i] = g(mn(i, 1));
            menv.y[i] = g(mn(i, 2));
        }
        let mut ks = [0.0; 10];
        for (k, id) in [IL_KS_LO, IL_KS_HI, ST_KS_LO, ST_KS_HI, ML_KS_LO, ML_KS_HI, TM_KS_LO, TM_KS_HI, RES_KS_LO, RES_KS_HI].iter().enumerate() {
            ks[k] = g(*id);
        }
        let render_mode = u(RENDER_MODE).min(2);
        Sh {
            rate,
            ovs: if render_mode == 2 { 2 } else { 1 },
            glide: g(GLIDE),
            tune: g(TUNE),
            warmth: g(WARMTH),
            transpose: g(TRANSPOSE).round(),
            kb: u(KBMODE).min(2) as u8,
            poly: u(POLY).clamp(1, POLY_MAX),
            morph,
            sel: u(MORPH_SEL).min(4),
            px: g(MORPH_X),
            py: g(MORPH_Y),
            ks,
            il_rel: g(IL_REL),
            ml_rel: g(ML_REL),
            invert: b(PICK_INVERT),
            spread_pick: g(SPREAD_PICK),
            spread_key: g(SPREAD_KEY),
            ws_on: b(WS_ON),
            ws_ty: u(WS_TYPE).min(3),
            f_on: b(F_ON),
            f_ty: u(F_TYPE).min(4),
            f_key: g(F_KEY),
            f_velo: g(F_VELO),
            a_soft: g(A_SOFT),
            a_hard: g(A_HARD),
            a_decay: g(A_DECAY),
            a_sus: g(A_SUSTAIN),
            a_rel: g(A_RELEASE),
            objs,
            lfo: lfos,
            vib_wave: u(VIB_WAVE),
            vib_rate: g(VIB_RATE),
            vib_sync: b(VIB_SYNC),
            vib_note: u(VIB_NOTE),
            vib_curve: g(VIB_CURVE),
            vib_phase: g(VIB_PHASE),
            vib_amt: g(VIB_AMT),
            vib_via: g(VIB_VIA),
            jit: jits,
            nr: [
                (b(nr(0, N_ON)), u(nr(0, N_TARGET)), g(nr(0, N_AMT))),
                (b(nr(1, N_ON)), u(nr(1, N_TARGET)), g(nr(1, N_AMT))),
            ],
            vm: [
                (b(vmod(0, V_ON)), u(vmod(0, V_TARGET)), g(vmod(0, V_AMT)), u(vmod(0, V_CURVE))),
                (b(vmod(1, V_ON)), u(vmod(1, V_TARGET)), g(vmod(1, V_AMT)), u(vmod(1, V_CURVE))),
            ],
            cab: [
                (b(cab(0, C_ON)), u(cab(0, C_TARGET)), u(cab(0, C_SRC)), u(cab(0, C_MODE)), g(cab(0, C_AMT))),
                (b(cab(1, C_ON)), u(cab(1, C_TARGET)), u(cab(1, C_SRC)), u(cab(1, C_MODE)), g(cab(1, C_AMT))),
            ],
            env: envs,
            menv,
            cc_a: u(CTRL_A) as u32,
            cc_b: u(CTRL_B) as u32,
            cc_vib: u(CTRL_VIB) as u32,
            cc_env: [u(CTRL_ENV1) as u32, u(CTRL_ENV2) as u32],
            cc_mx: u(CTRL_MX) as u32,
            cc_my: u(CTRL_MY) as u32,
            mono_mode: u(MONO_MODE).min(2),
            mono_bend: g(MONO_BEND),
            render_mode,
            limiter: u(LIMITER).min(3),
        }
    }
}

/// Live state every voice reads.
struct Glob {
    ctrl: [f32; 128],
    aftertouch: f32,
    bend: f32,
    bpm: f32,
    lfo_mono: [LfoState; 3],
    lfo_out: [f32; 3],
    sustain: bool,
    ext: Vec<f32>,
    ext_n: usize,
    ext_pos: usize,
}

/// A note value in seconds at the tempo.
fn note_secs(i: usize, bpm: f32) -> f32 {
    NOTE_VALUES[i.min(NOTE_VALUES.len() - 1)].1 * 60.0 / bpm.max(1.0)
}

// =============================================================================================
// Envelope playback (control envelopes and the morph envelope)

#[derive(Clone, Copy, Debug)]
struct EnvRun {
    t: f32,
    dir: f32,
    rel: bool,
}

impl EnvRun {
    const fn new() -> EnvRun {
        EnvRun { t: 0.0, dir: 1.0, rel: false }
    }
    /// Moves on by `dt`. `mode`: 0 sustain, 1 finish, 2 loop forward, 3 loop backward,
    /// 4 loop alternate; the loop runs between `loop_t` and `sus_t`.
    fn advance(&mut self, dt: f32, total: f32, loop_t: f32, sus_t: f32, mode: usize) {
        if mode == 1 {
            self.t = (self.t + dt).min(total);
            return;
        }
        let looping = mode >= 2;
        let loop_before = loop_t < sus_t;
        let (a, b, active) = if !looping {
            (0.0, 0.0, false)
        } else if loop_before {
            (loop_t, sus_t, !self.rel)
        } else {
            (sus_t, loop_t, self.rel)
        };
        let mut nt = self.t + dt * self.dir;
        if active && b > a + 1e-4 {
            let span = b - a;
            match mode {
                2 => {
                    if nt >= b {
                        nt = a + (nt - b) % span;
                    }
                }
                3 => {
                    if self.dir > 0.0 {
                        if nt >= b {
                            self.dir = -1.0;
                            nt = b - (nt - b).min(span);
                        }
                    } else if nt <= a {
                        nt = b - (a - nt) % span;
                    }
                }
                _ => {
                    if self.dir > 0.0 && nt >= b {
                        self.dir = -1.0;
                        nt = b - (nt - b).min(span);
                    } else if self.dir < 0.0 && nt <= a {
                        self.dir = 1.0;
                        nt = a + (a - nt).min(span);
                    }
                }
            }
        } else if self.rel {
            self.dir = 1.0;
        } else if nt >= sus_t && self.dir > 0.0 {
            nt = sus_t;
        }
        self.t = nt.clamp(0.0, total);
    }
}

/// Piecewise-linear value of nodes (cumulative times `cum`, values `val`) at time `t`.
fn pl(cum: &[f32], val: &[f32], t: f32) -> f32 {
    let n = cum.len();
    if t <= cum[0] {
        return val[0];
    }
    for k in 1..n {
        if t <= cum[k] {
            let d = (cum[k] - cum[k - 1]).max(1e-6);
            return val[k - 1] + (val[k] - val[k - 1]) * ((t - cum[k - 1]) / d);
        }
    }
    val[n - 1]
}

/// How strongly a velocity-like source scales, by a curve (0 concave, 1 linear, 2 convex).
fn curve_of(vel: f32, c: usize) -> f32 {
    match c {
        0 => vel * vel,
        2 => vel.sqrt(),
        _ => vel,
    }
}

// =============================================================================================
// Objects

#[derive(Clone, Copy, Default)]
struct ObjP {
    kind: u8,
    st: f32,
    tim: f32,
    var: f32,
    pos: f32,
    vel: f32,
    active: bool,
}

#[derive(Clone)]
struct ObjRun {
    pending: bool,
    rng: Rng,
    last_kind: u8,
    imp_n: u32,
    imp_i: u32,
    imp_amp: f32,
    imp_x: f32,
    ham_on: bool,
    h: f32,
    hv: f32,
    contact: bool,
    k: f32,
    inv_m: f32,
    grav: f32,
    ramp: f32,
    prev: f32,
    lp: Svf,
    nz: f32,
    bh: f32,
    bv: f32,
    click: u32,
    y1: f32,
    y2: f32,
    acc: f32,
    vlp: f32,
}

impl ObjRun {
    fn new(seed: u32) -> ObjRun {
        ObjRun {
            pending: false,
            rng: Rng::new(seed),
            last_kind: 0,
            imp_n: 0,
            imp_i: 0,
            imp_amp: 0.0,
            imp_x: 0.0,
            ham_on: false,
            h: 0.0,
            hv: 0.0,
            contact: false,
            k: 0.1,
            inv_m: 0.3,
            grav: 0.0,
            ramp: 0.0,
            prev: 0.0,
            lp: Svf::default(),
            nz: 0.0,
            bh: 0.02,
            bv: 0.0,
            click: 0,
            y1: 0.0,
            y2: 0.0,
            acc: 0.0,
            vlp: 0.0,
        }
    }
    fn reset_state(&mut self) {
        self.imp_i = self.imp_n;
        self.ham_on = false;
        self.contact = false;
        self.ramp = 0.0;
        self.prev = 0.0;
        self.nz = 0.0;
        self.bv = 0.0;
        self.bh = 0.02;
        self.click = 0;
        self.y1 = 0.0;
        self.y2 = 0.0;
        self.acc = 0.0;
        self.vlp = 0.0;
        self.lp.reset();
    }

    /// The start of a one-shot excitation.
    fn begin(&mut self, g: &mut Guide, p: &ObjP) {
        let vel_amt = p.vel;
        match p.kind {
            1 => {
                // impulse: Timbre sets the width, Variation how much a hard note narrows it
                let bright = (p.tim * 0.5 + 0.5).clamp(0.0, 1.0);
                let width = (0.3 * (0.02f32 / 0.3).powf(bright)) * (1.0 - 0.6 * p.var * (p.vel - 0.5));
                self.imp_n = ((g.len_ring() * 0.5 * width) as u32).clamp(2, 4000);
                self.imp_i = 0;
                self.imp_amp = p.st * 0.5 * vel_amt;
                self.imp_x = p.pos;
            }
            2 | 3 => {
                // hammer: Timbre = mass (up = lighter), Variation = felt stiffness (GravStrike:
                // gravitation)
                let v0 = p.st * vel_amt * 0.024;
                self.ham_on = true;
                self.contact = false;
                self.inv_m = 0.0006 * 4f32.powf(p.tim);
                if p.kind == 2 {
                    self.k = (0.3 * 4f32.powf(p.var)).min(0.9);
                    self.h = 0.0;
                    self.hv = -v0;
                    self.grav = 0.0;
                } else {
                    self.k = (0.3 * 4f32.powf(p.tim)).min(0.9);
                    self.inv_m = 0.0006;
                    self.h = 0.01;
                    self.hv = -v0;
                    self.grav = 4e-6 * 4f32.powf(p.var);
                }
            }
            4 => {
                // pick: Strength force/speed, Timbre force/speed ratio, Variation stiffness
                let sharp = (0.5 + p.tim * 0.5).clamp(0.0, 1.0) * (0.5 + 0.5 * (p.var * 0.5 + 0.5));
                let round = 0.15 * (0.01f32 / 0.15).powf(sharp);
                g.pluck(p.pos, p.st * 0.24 * vel_amt, round);
                self.click = if p.var > 0.2 { (80.0 * p.var) as u32 } else { 0 };
            }
            _ => {}
        }
    }

    fn step(&mut self, g: &mut Guide, p: &ObjP, ext: f32, held_ticks: usize) {
        if p.kind != self.last_kind {
            self.last_kind = p.kind;
            self.reset_state();
        }
        if self.pending {
            self.pending = false;
            if p.st > 0.0 {
                self.begin(g, p);
            }
        }
        let st = p.st;
        match p.kind {
            1 => {
                if self.imp_i < self.imp_n {
                    let ph = (self.imp_i as f32 + 0.5) / self.imp_n as f32;
                    let w = 0.5 - 0.5 * (std::f32::consts::TAU * ph).cos();
                    g.inject(self.imp_x, self.imp_amp * w * 1.4);
                    self.imp_i += 1;
                }
            }
            2 | 3 => {
                if self.ham_on {
                    let y = g.disp(p.pos);
                    let c = y - self.h;
                    if c > 0.0 {
                        let f = (self.k * c).min(0.5);
                        self.hv += f * self.inv_m;
                        g.inject(p.pos, -f);
                        self.contact = true;
                    } else if self.contact {
                        // separated: a hammer that fell keeps bouncing, a struck one is gone
                        self.contact = false;
                        if p.kind == 2 {
                            self.ham_on = false;
                        } else {
                            self.hv *= 0.55;
                        }
                    }
                    self.hv -= self.grav;
                    self.h += self.hv;
                    if self.h > 0.5 {
                        self.ham_on = false;
                    }
                }
            }
            4 => {
                if self.click > 0 {
                    self.click -= 1;
                    let a = self.rng.bipolar() * p.st * 0.03 * p.var;
                    g.inject(p.pos, a);
                }
            }
            5 | 6 => {
                let y = g.disp(p.pos);
                let v_s = y - self.prev;
                self.prev = y;
                if !p.active {
                    self.ramp = 0.0;
                    return;
                }
                self.ramp = (self.ramp + 1.0 / (g.rate * 0.04)).min(1.0);
                let vb = 0.0010 * st * p.vel * self.ramp;
                let vref = 0.0005 * (1.0 + 0.6 * p.var);
                let r = (vb - v_s) / vref;
                // stick-slip friction: grips near zero relative speed, slides on less beyond
                let decay = 1.3 + 0.9 * p.var;
                let mu = (r * 8.0).tanh() * (0.35 + 0.65 * (-r.abs() * decay).exp());
                let pressure = 6e-4 * 2f32.powf(p.tim * 1.2);
                let wide = if p.kind == 6 { 0.5 } else { 1.0 };
                let f = pressure * mu * wide;
                // a force makes velocity waves: what the string is given is its running sum
                self.acc = self.acc * 0.998 + f + self.rng.bipolar() * 2e-6 * st * held_ticks as f32;
                if p.kind == 6 {
                    let w = 0.012;
                    g.inject(p.pos - w, self.acc * 0.3);
                    g.inject(p.pos, self.acc * 0.4);
                    g.inject(p.pos + w, self.acc * 0.3);
                } else {
                    g.inject(p.pos, self.acc);
                }
            }
            7 => {
                if !p.active {
                    return;
                }
                let hz = 150.0 * 70f32.powf(p.tim * 0.5 + 0.5);
                let q = 0.6 + (p.var * 0.5 + 0.5) * 6.0;
                self.lp.set(hz.min(g.rate * 0.4), q, g.rate);
                self.lp.mode = SvfMode::Low;
                let x = self.lp.run(self.rng.bipolar());
                g.inject(p.pos, x * st * 0.04 * p.vel);
            }
            8 => {
                let y = g.disp(p.pos);
                let v_s = y - self.prev;
                self.prev = y;
                if !p.active {
                    self.ramp = 0.0;
                    return;
                }
                self.ramp = (self.ramp + 1.0 / (g.rate * 0.06)).min(1.0);
                // a jet: negative resistance on the string velocity (self-sustaining), plus breath
                // the jet answers the lower overtones: the velocity is low-passed near the pitch
                let a = (-std::f32::consts::TAU * 2.2 * g.hz / g.rate).exp();
                self.vlp = self.vlp * a + v_s * (1.0 - a);
                let u = self.vlp / 0.0016;
                let gain = BLOW_GAIN * st * p.vel * 2f32.powf(p.tim * 0.5) * self.ramp;
                let jet = (u - u * u * u / 3.0) * 0.0016;
                let noise = self.rng.bipolar() * (0.6 + p.var * 0.5) * 0.00004 * st;
                self.acc = self.acc * 0.998 + gain * jet + noise * self.ramp;
                g.inject(p.pos, self.acc);
            }
            9 => {
                if !p.active {
                    return;
                }
                let hz = 200.0 * 90f32.powf(p.tim * 0.5 + 0.5);
                self.lp.set(hz.min(g.rate * 0.4), 0.7, g.rate);
                self.lp.mode = SvfMode::Low;
                let x = self.lp.run(ext);
                let w = 0.004 + (p.var * 0.5 + 0.5) * 0.04;
                g.inject(p.pos - w, x * st * 0.03);
                g.inject(p.pos + w, x * st * 0.03);
            }
            10 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                let y = g.disp(p.pos);
                let barrier = p.tim * 0.08;
                let over = y - barrier;
                if over > 0.0 {
                    let k = (0.1 + st * 0.8).min(0.9);
                    let w = 0.005 + (p.var * 0.5 + 0.5) * 0.04;
                    let d = -over * k / 3.0;
                    g.inject(p.pos - w, d);
                    g.inject(p.pos, d);
                    g.inject(p.pos + w, d);
                }
            }
            11 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                let y = g.disp(p.pos);
                let clear = if p.tim >= 0.0 { p.tim * 0.08 } else { 0.0 };
                let over = if y > clear {
                    y - clear
                } else if y < -clear {
                    y + clear
                } else {
                    0.0
                };
                if over != 0.0 {
                    let k = (0.1 + st * 0.8).min(0.9);
                    g.inject(p.pos, -over * k);
                }
            }
            12 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                let gr = 1e-6 + st * 5e-6;
                let kc = 0.1 * 4f32.powf(p.tim);
                let damp = 0.002 + (p.var * 0.5 + 0.5) * 0.04;
                let y = g.disp(p.pos);
                self.bv -= gr;
                self.bh += self.bv;
                if self.bh < y {
                    let f = ((y - self.bh) * kc).min(0.2);
                    self.bv += f * 0.8;
                    g.inject(p.pos, -f);
                    self.bv *= 1.0 - damp;
                    self.bv += self.rng.bipolar() * 1e-4;
                }
                if self.bh > 0.15 {
                    self.bh = 0.15;
                    self.bv = -self.bv.abs() * 0.5;
                }
            }
            13 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                let dist = (1.15 - st) * 0.07;
                let refl = if p.var >= 0.0 { 0.25 + 0.6 * p.var } else { 0.3 };
                for j in -2i32..=2 {
                    let x = p.pos + j as f32 * 0.02;
                    let b = dist + p.tim * 0.03 * j as f32;
                    let y = g.disp(x);
                    if y > b {
                        g.inject(x, -(y - b) * refl * 0.4);
                        if p.var < 0.0 {
                            g.inject(x, -y * -p.var * 0.05);
                        }
                    }
                }
            }
            14 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                let y = g.disp(p.pos);
                let a = y - 2.0 * self.y1 + self.y2;
                self.y2 = self.y1;
                self.y1 = y;
                g.inject(p.pos, -a * st * 0.4);
            }
            15 => {
                if !p.active || st <= 0.0 {
                    return;
                }
                // Timbre: below 0 a narrow, sharp damper; above 0 a broad, soft one
                // a damper takes the motion of the string where it sits (a node is left alone)
                let n = ((p.var * 0.5 + 0.5) * 2.0 + p.tim.max(0.0) * 2.0).round() as i32;
                let k = (1.0 - (-st * st * 0.12).exp()) * (1.0 - 0.4 * p.tim.max(0.0)) * (1.0 + 0.5 * (-p.tim).max(0.0));
                let k = (k / (2 * n + 1) as f32 * 2.0).min(0.5);
                for j in -n..=n {
                    let x = p.pos + j as f32 * 0.012;
                    let y = g.disp(x);
                    g.inject(x, -k * y);
                }
            }
            _ => {}
        }
    }
}

// =============================================================================================
// A voice

const PICK_GAIN: f32 = 9.0;
const BLOW_GAIN: f32 = 0.08;
const OUT_GAIN: f32 = 0.35;

#[derive(Clone, Copy)]
struct MorphOut {
    v: [f32; MORPH_N],
    x: f32,
    y: f32,
}

/// The weights of the points A, B, C, D and Center at a pad position.
pub fn morph_weights(x: f32, y: f32) -> [f32; 5] {
    let u = (2.0 * x - 1.0).abs();
    let w = (2.0 * y - 1.0).abs();
    let r = u.max(w).clamp(0.0, 1.0);
    let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    [(1.0 - x) * y * r, x * y * r, (1.0 - x) * (1.0 - y) * r, x * (1.0 - y) * r, 1.0 - r]
}

/// The morph values (as stored per point) at a pad position.
pub fn morph_values(points: &[[f32; MORPH_N]; 5], x: f32, y: f32) -> [f32; MORPH_N] {
    let w = morph_weights(x, y);
    let mut v = [0.0f32; MORPH_N];
    for (i, wi) in w.iter().enumerate() {
        for k in 0..MORPH_N {
            v[k] += wi * points[i][k];
        }
    }
    v
}

/// The pad position of morph point `p` (A top left, B top right, C bottom left, D bottom
/// right, Center).
pub fn point_pos(p: usize) -> (f32, f32) {
    match p {
        0 => (0.0, 1.0),
        1 => (1.0, 1.0),
        2 => (0.0, 0.0),
        3 => (1.0, 0.0),
        _ => (0.5, 0.5),
    }
}

/// The morph point nearest a pad position.
pub fn nearest_point(x: f32, y: f32) -> usize {
    let r = (2.0 * x - 1.0).abs().max((2.0 * y - 1.0).abs());
    if r < 0.4 {
        4
    } else {
        match (x >= 0.5, y >= 0.5) {
            (false, true) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        }
    }
}

struct SVoice {
    on: bool,
    note: u8,
    chan: u8,
    vel: f32,
    age: u64,
    held: bool,
    sustained: bool,
    released: bool,
    killing: bool,
    mono: bool,
    cur: f32,
    warm: f32,
    t: f32,
    guide: Guide,
    objs: [ObjRun; 3],
    op: [ObjP; 3],
    env: Adsr,
    lfo: [LfoState; 2],
    vib: LfoState,
    jit: [LfoState; 2],
    nr: [f32; 2],
    cab_latch: [f32; 2],
    envs: [EnvRun; 2],
    menv: EnvRun,
    src: [f32; 12],
    lfo_v: [f32; 2],
    filt: [Svf; 2],
    f_g: f32,
    fade: Fade,
    level: f32,
    silent: u32,
    pa: f32,
    pb: f32,
    gains: [f32; 4],
    ws: (f32, f32),
    out_gain: f32,
    poly_lim: Limiter,
    rng: Rng,
    chan_ctrl: [f32; 128],
    chan_bend: f32,
    pub morph_now: (f32, f32),
    pub env_t: [f32; 2],
    pub menv_t: f32,
    mods: [f32; NT],
    cut_hz: f32,
    tm: f32,
}

impl SVoice {
    fn new(rate: f32, seed: u32) -> SVoice {
        SVoice {
            on: false,
            note: 60,
            chan: 0,
            vel: 1.0,
            age: 0,
            held: false,
            sustained: false,
            released: false,
            killing: false,
            mono: false,
            cur: 60.0,
            warm: 0.0,
            t: 0.0,
            guide: Guide::new((rate * 2.0 / 14.0) as usize, rate),
            objs: [ObjRun::new(seed), ObjRun::new(seed + 1), ObjRun::new(seed + 2)],
            op: [ObjP::default(); 3],
            env: Adsr::default(),
            lfo: [LfoState::new(seed + 3), LfoState::new(seed + 4)],
            vib: LfoState::new(seed + 5),
            jit: [LfoState::new(seed + 6), LfoState::new(seed + 7)],
            nr: [0.0; 2],
            cab_latch: [0.0; 2],
            envs: [EnvRun::new(); 2],
            menv: EnvRun::new(),
            src: [0.0; 12],
            lfo_v: [0.0; 2],
            filt: [Svf::default(); 2],
            f_g: 1.0,
            fade: Fade::default(),
            level: 0.0,
            silent: 0,
            pa: 0.2,
            pb: 0.55,
            gains: [0.7; 4],
            ws: (0.0, 0.0),
            out_gain: 1.0,
            poly_lim: Limiter::new(rate),
            rng: Rng::new(seed + 11),
            chan_ctrl: [f32::NAN; 128],
            chan_bend: 0.0,
            morph_now: (0.5, 0.5),
            env_t: [0.0; 2],
            menv_t: 0.0,
            mods: [0.0; NT],
            cut_hz: 20000.0,
            tm: 0.0,
        }
    }

    fn cc(&self, g: &Glob, cc: u32) -> f32 {
        if cc >= 128 {
            return 0.0;
        }
        let l = self.chan_ctrl[cc as usize];
        if l.is_nan() {
            g.ctrl[cc as usize]
        } else {
            l
        }
    }

    /// A new note: `from` is the pitch the note glides from (its own pitch for no glide).
    fn start(&mut self, note: u8, vel: u8, chan: u8, sh: &Sh, g: &Glob, from: f32, clear: bool, age: u64) {
        self.note = note;
        self.chan = chan;
        self.vel = vel.max(1) as f32 / 127.0;
        self.age = age;
        self.on = true;
        self.held = true;
        self.sustained = false;
        self.released = false;
        self.killing = false;
        self.cur = from;
        self.warm = self.rng.bipolar() * sh.warmth * 25.0;
        self.chan_ctrl = [f32::NAN; 128];
        self.chan_bend = 0.0;
        self.guide.rate = sh.rate * sh.ovs as f32;
        if clear {
            self.guide.clear();
        }
        self.silent = 0;
        self.level = 1.0;
        self.restart(sh, g, true);
    }

    /// (Re)starts the modulation and fires the objects; the string keeps vibrating.
    fn restart(&mut self, sh: &Sh, g: &Glob, first: bool) {
        self.t = 0.0;
        self.held = true;
        self.released = false;
        for k in 0..2 {
            let ph = sh.lfo[k].phase;
            self.lfo[k].phase = if ph <= 0.0 {
                0.0
            } else if ph <= 0.33 {
                self.rng.unit() * ph * 3.0
            } else if ph <= 0.66 {
                self.rng.unit()
            } else {
                0.0
            };
            self.envs[k] = EnvRun::new();
        }
        let ph = sh.vib_phase;
        self.vib.phase = if ph <= 0.0 || ph > 0.66 {
            0.0
        } else if ph <= 0.33 {
            self.rng.unit() * ph * 3.0
        } else {
            self.rng.unit()
        };
        self.menv = EnvRun::new();
        for k in 0..2 {
            self.nr[k] = self.rng.bipolar();
        }
        // Controller A/B in Note On mode: the value at the start of the note
        let (ca, cb) = (self.cc(g, sh.cc_a), self.cc(g, sh.cc_b));
        for k in 0..2 {
            self.cab_latch[k] = match sh.cab[k].2 {
                0 => ca,
                1 => cb,
                _ => (ca + cb).min(1.0),
            };
        }
        let att = dsp::lerp(sh.a_soft, sh.a_hard, self.vel);
        self.env.set(att, sh.a_decay, sh.a_sus, sh.a_rel, sh.rate);
        self.env.trigger();
        self.control(sh, g);
        for o in 0..3 {
            if sh.objs[o].on && sh.objs[o].gate != 2 {
                self.objs[o].pending = true;
            }
        }
        let _ = first;
    }

    fn release(&mut self, sh: &Sh) {
        if self.released {
            return;
        }
        self.held = false;
        self.sustained = false;
        self.released = true;
        self.env.gate_off();
        self.env.set(dsp::lerp(sh.a_soft, sh.a_hard, self.vel), sh.a_decay, sh.a_sus, sh.a_rel, sh.rate);
        for k in 0..2 {
            self.envs[k].rel = true;
        }
        self.menv.rel = true;
        for o in 0..3 {
            let c = sh.objs[o];
            if c.on && (c.gate == 2 || (c.kind == 3 && c.gate == 0)) {
                self.objs[o].pending = true;
            }
        }
    }

    fn kill(&mut self, rate: f32) {
        if self.on && !self.killing {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }

    /// The modulation, morph and string settings for the next CTRL samples.
    fn control(&mut self, sh: &Sh, g: &Glob) {
        let rate = sh.rate;
        let dt = CTRL as f32 / rate;
        self.t += dt;
        let bpm = g.bpm;
        let vel = self.vel;
        let note = self.note as f32;
        // ---- sources
        self.src[1] = vel * vel;
        self.src[2] = vel;
        self.src[3] = vel.sqrt();
        self.src[4] = ((note - 36.0) / 60.0).clamp(0.0, 1.0);
        let ca = self.cc(g, sh.cc_a);
        let cb = self.cc(g, sh.cc_b);
        self.src[5] = ca;
        self.src[6] = cb;
        self.src[11] = g.aftertouch;
        let mut mods = [0.0f32; NT];
        // control envelopes
        let mut env_val = [0.0f32; 2];
        for k in 0..2 {
            let c = &sh.env[k];
            let ctrlv = self.cc(g, sh.cc_env[k]);
            let mut lvl = 0.0;
            if c.env {
                // segment times: scale, tempo sync, the attack time's velocity sensitivity
                let scale = c.scale * 0.01 * if c.sync { c.refbpm / bpm.max(1.0) } else { 1.0 };
                let mut cum = [0.0f32; 5];
                let atv = if c.atv >= 0.0 { 1.0 - c.atv * (1.0 - vel) } else { 1.0 - (-c.atv) * vel };
                for s in 0..4 {
                    let tt = c.t[s] * 0.001 * scale * if s == 0 { atv.max(0.02) } else { 1.0 };
                    cum[s + 1] = cum[s] + tt.max(0.0005);
                }
                let total = if c.use_rec { (c.rec_len * 0.001 * scale).max(0.01).max(cum[4]) } else { cum[4] };
                if c.use_rec {
                    let (lt, st) = (cum[2] / cum[4].max(1e-6) * total, cum[3] / cum[4].max(1e-6) * total);
                    self.envs[k].advance(dt, total, lt, st, c.sus);
                } else {
                    self.envs[k].advance(dt, cum[4], cum[2], cum[3], c.sus);
                }
                self.env_t[k] = self.envs[k].t;
                if c.use_rec {
                    let pos = (self.envs[k].t / total).clamp(0.0, 1.0) * (REC_POINTS - 1) as f32;
                    let i = (pos as usize).min(REC_POINTS - 2);
                    let f = pos - i as f32;
                    let raw = c.rec[i] + (c.rec[i + 1] - c.rec[i]) * f;
                    // VariMod: how much the recorded wiggle is exaggerated or flattened
                    let mean: f32 = c.rec.iter().sum::<f32>() / REC_POINTS as f32;
                    let vs = self.src[varimod_via(c.vmsrc)];
                    let fac = if c.vmsrc == 0 { 1.0 } else { (1.0 + c.vmamt * (vs * 2.0 - 1.0)).max(0.0) };
                    lvl = (mean + (raw - mean) * fac).clamp(0.0, 1.0);
                } else {
                    lvl = pl(&cum, &c.l, self.envs[k].t);
                }
            }
            let v = match (c.ctrl, c.env) {
                (true, true) => lvl + ctrlv,
                (true, false) => ctrlv,
                (false, true) => lvl,
                _ => 0.0,
            };
            env_val[k] = v;
            self.src[9 + k] = v.clamp(0.0, 1.0);
        }
        // LFOs
        for k in 0..2 {
            let c = &sh.lfo[k];
            self.lfo_v[k] = 0.0;
            if !c.on {
                continue;
            }
            let mut hz = if c.sync { 1.0 / note_secs(c.note, bpm) } else { c.rate };
            if c.rmsrc > 0 && c.rmamt > 0.0 {
                hz *= 2f32.powf((self.src[c.rmsrc.min(11)] * 2.0 - 1.0) * c.rmamt * 3.0);
            }
            let out = if c.phase <= 0.0 {
                g.lfo_out[k]
            } else {
                self.lfo[k].step(c.wave, c.curve, hz * dt, ca)
            };
            let fade = if c.env > 0.0 {
                (self.t / (c.env * 6.0)).clamp(0.0, 1.0)
            } else if c.env < 0.0 {
                1.0 - (self.t / (-c.env * 6.0)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let o = out * fade;
            self.lfo_v[k] = o;
            self.src[7 + k] = (o * 0.5 + 0.5).clamp(0.0, 1.0);
            for rt in &c.r {
                if rt.on && rt.target > 0 && rt.target < NT {
                    let via = self.src[rt.via.min(11)];
                    mods[rt.target] += o * rt.amount(via);
                }
            }
        }
        // jitter
        for k in 0..2 {
            let c = &sh.jit[k];
            if !c.on {
                continue;
            }
            let j = self.jit[k].step(9, 0.0, c.rate * dt, ca) * 1.3;
            for s in 0..2 {
                if c.t[s] > 0 && c.t[s] < NT {
                    mods[c.t[s]] += j * c.a[s];
                }
            }
        }
        // note-on randomisers and velocity modulators
        for k in 0..2 {
            let (on, tg, amt) = sh.nr[k];
            if on && tg > 0 && tg < NT {
                mods[tg] += self.nr[k] * amt;
            }
            let (on, tg, amt, cv) = sh.vm[k];
            if on && tg > 0 && tg < NT {
                mods[tg] += curve_of(vel, cv) * amt;
            }
        }
        // Controller A / B: two targets, continuous or only at note-on
        for k in 0..2 {
            let (on, tg, src, mode, amt) = sh.cab[k];
            if on && tg > 0 && tg < NT {
                let live = match src {
                    0 => ca,
                    1 => cb,
                    _ => (ca + cb).min(1.0),
                };
                mods[tg] += if mode == 1 { self.cab_latch[k] } else { live } * amt;
            }
        }
        // control envelope routes
        for k in 0..2 {
            for rt in &sh.env[k].r {
                if rt.on && rt.target > 0 && rt.target < NT && (sh.env[k].env || sh.env[k].ctrl) {
                    let via = self.src[rt.via.min(11)];
                    mods[rt.target] += env_val[k] * rt.amount(via);
                }
            }
        }
        // ---- the morph position
        let mm = self.morph(sh, g, cb);
        self.morph_now = (mm.x, mm.y);
        self.mods = mods;
        let mv = &mm.v;
        // ---- objects
        for o in 0..3 {
            let c = sh.objs[o];
            let b = 4 * o;
            let strength = (mv[b] + mods[1 + b]).clamp(0.0, 1.0);
            let kind = c.kind;
            // velocity sensitivity: excite types only (object 3 never)
            let vel_sens_ok = o < 2 && (1..=8).contains(&kind);
            let vel_amt = if vel_sens_ok { (1.0 - c.vel) + c.vel * vel } else { 1.0 };
            self.op[o] = ObjP {
                kind,
                st: strength,
                tim: (mv[b + 1] + mods[2 + b]).clamp(-1.0, 1.0),
                var: (mv[b + 2] + mods[3 + b]).clamp(-1.0, 1.0),
                pos: (mv[b + 3] + mods[4 + b] * 0.5).clamp(0.002, 0.998),
                vel: vel_amt,
                active: match c.gate {
                    0 => self.held,
                    1 => true,
                    _ => self.released,
                },
            };
        }
        // ---- pickups and pans
        self.pa = (mv[12] + 0.5 * (mods[13] + mods[15] + mods[16])).clamp(0.0, 1.0);
        self.pb = (mv[13] + 0.5 * (mods[14] + mods[15] - mods[16])).clamp(0.0, 1.0);
        let kp = sh.spread_key * ((note - 60.0) / 36.0).clamp(-1.0, 1.0);
        let pan_a = (sh.spread_pick + mods[17] + mods[18] + kp).clamp(-1.0, 1.0);
        let pan_b = (-sh.spread_pick + mods[17] - mods[18] + kp).clamp(-1.0, 1.0);
        let pg = |p: f32| {
            let a = (p + 1.0) * 0.25 * std::f32::consts::PI;
            (a.cos() * std::f32::consts::SQRT_2 * 0.5, a.sin() * std::f32::consts::SQRT_2 * 0.5)
        };
        let (al, ar) = pg(pan_a);
        let (bl, br) = pg(pan_b);
        let inv = if sh.invert { -1.0 } else { 1.0 };
        self.gains = [al, ar, bl * inv, br * inv];
        // ---- string material, pitch
        let ks = |lo: f32, hi: f32| {
            if note < 60.0 {
                lo * ((60.0 - note) / 48.0).min(1.0)
            } else {
                hi * ((note - 60.0) / 48.0).min(1.0)
            }
        };
        let mut il = mv[14] + ks(sh.ks[0], sh.ks[1]) + mods[19];
        let st = mv[15] + ks(sh.ks[2], sh.ks[3]) + mods[20];
        let mut ml = mv[16] + ks(sh.ks[4], sh.ks[5]) + mods[21];
        let tm = mv[17] + ks(sh.ks[6], sh.ks[7]) + mods[22];
        let res = mv[18] + ks(sh.ks[8], sh.ks[9]);
        if self.released {
            il *= sh.il_rel;
            ml = if sh.ml_rel >= 1.0 { ml + (sh.ml_rel - 1.0) * 0.6 } else { ml * sh.ml_rel };
        }
        let (il, st, ml, tm, res) = (il.clamp(0.0, 1.0), st.clamp(0.0, 1.0), ml.clamp(0.0, 1.0), tm.clamp(0.0, 1.0), res.clamp(0.0, 1.0));
        self.tm = tm;
        // glide, bend, vibrato, pitch
        let target = self.note as f32;
        if sh.glide > 0.0 {
            let tau = (sh.glide / 3.0).max(1e-3);
            self.cur += (target - self.cur) * (1.0 - (-dt / tau).exp());
            if (target - self.cur).abs() < 0.002 {
                self.cur = target;
            }
        } else {
            self.cur = target;
        }
        // vibrato: the dedicated LFO, depth from the Vib Depth controller
        let vctrl = self.cc(g, sh.cc_vib);
        let vdepth = sh.vib_amt * ((1.0 - sh.vib_via) + sh.vib_via * vctrl);
        let mut vib = 0.0;
        if vdepth > 0.0 {
            let hz = if sh.vib_sync { 1.0 / note_secs(sh.vib_note, bpm) } else { sh.vib_rate };
            let w = if sh.vib_phase <= 0.0 { g.lfo_out[2] } else { self.vib.step(sh.vib_wave, sh.vib_curve, hz * dt, ca) };
            vib = w * vdepth * 0.5;
        }
        let bend = g.bend + self.chan_bend;
        let semis = self.cur + sh.transpose * 12.0 + sh.tune * 0.01 + self.warm * 0.01 + bend + vib + mods[23] * 12.0;
        let mut hz = lg_dsp::midi_hz(semis);
        // tension modulation: a louder string runs sharper
        hz *= 1.0 + tm * self.guide.energy.min(0.3) * 0.6;
        let rate_s = self.guide.rate;
        let maxel = if sh.render_mode == 0 { 100.0 } else { 1000.0 };
        let elements = 3.0 + (maxel - 3.0) * res * res;
        let overtones = (elements - 1.0).min(rate_s * 0.5 / hz.max(1.0) - 1.0);
        let t60 = 14.0 * (1.0 - ml).powf(2.5) + 0.05;
        let extra = ((0.12 - res).max(0.0)) * 3.0;
        self.guide.set(hz, t60, il, st, overtones, extra);
        // ---- waveshaper, filter
        self.ws = ((mv[19] + mods[24]).clamp(-1.0, 1.0), (mv[20] + mods[25]).clamp(-1.0, 1.0));
        let cutn = (mv[21] + mods[26]).clamp(0.0, 1.0);
        let resn = (mv[22] + mods[27]).clamp(0.0, 1.0);
        let oct = sh.f_key * (note - 60.0) / 12.0 + sh.f_velo * (vel * 2.0 - 1.0) * 3.0;
        let fhz = (cut_hz(cutn) * 2f32.powf(oct)).clamp(20.0, rate * 0.45);
        self.cut_hz = fhz;
        let (mode, q) = match sh.f_ty {
            0 => (SvfMode::High, 0.6 + resn * resn * 10.0),
            1 => (SvfMode::Low, 0.6 + resn * resn * 10.0),
            2 => (SvfMode::Band, 1.0 + resn * 6.0),
            3 => (SvfMode::Band, 0.7 + resn * 10.0),
            _ => (SvfMode::Notch, 0.7 + resn * 8.0),
        };
        self.f_g = 1.0 + resn * 5.0;
        for f in &mut self.filt {
            f.mode = mode;
            f.set(fhz, q, rate);
        }
        self.out_gain = (1.0 + mods[28]).max(0.0);
    }

    /// The morph values for this voice now.
    fn morph(&mut self, sh: &Sh, g: &Glob, ctrl_b: f32) -> MorphOut {
        let m = &sh.menv;
        let dt = CTRL as f32 / sh.rate;
        let (px, py) = (sh.px, sh.py);
        let mut x = px;
        let mut y = py;
        let uses_env = m.mode >= 2;
        if m.mode == 0 {
            // morphing off: the selected point as it is
            let (sx, sy) = point_pos(sh.sel);
            x = sx;
            y = sy;
        } else if uses_env {
            let scale = m.scale * 0.01 * if m.sync { m.refbpm / g.bpm.max(1.0) } else { 1.0 };
            let mut cum = [0.0f32; 9];
            for i in 1..9 {
                cum[i] = cum[i - 1] + (m.t[i] * 0.001 * scale).max(0.0005);
            }
            let total = cum[8];
            if m.sus == 5 {
                // scan: Ctrl B is the time position
                self.menv.t = ctrl_b.clamp(0.0, 1.0) * total;
            } else {
                self.menv.advance(dt, total, cum[m.loop_i], cum[m.sus_i], m.sus);
            }
            self.menv_t = self.menv.t / total.max(1e-6);
            let (ex, ey) = if m.mode == 3 && m.sub == 2 {
                (m.x[m.nsel], m.y[m.nsel])
            } else {
                let t = self.menv.t;
                // find the segment, shape the transition
                let mut seg = 0;
                for i in 1..9 {
                    if t <= cum[i] || i == 8 {
                        seg = i - 1;
                        break;
                    }
                }
                let d = (cum[seg + 1] - cum[seg]).max(1e-6);
                let s = ((t - cum[seg]) / d).clamp(0.0, 1.0);
                let smooth = s * s * (3.0 - 2.0 * s);
                let f = if m.trans < 0.5 {
                    smooth + (s - smooth) * (m.trans * 2.0)
                } else {
                    let k = (m.trans - 0.5) * 2.0;
                    s * (1.0 - k)
                };
                (m.x[seg] + (m.x[seg + 1] - m.x[seg]) * f, m.y[seg] + (m.y[seg + 1] - m.y[seg]) * f)
            };
            let msrc = if m.modamt > 0.0 { self.src[m.modsrc.min(11)] } else { 1.0 };
            let depth = m.depth * ((1.0 - m.modamt) + m.modamt * msrc);
            x = 0.5 + (ex - 0.5) * depth;
            y = 0.5 + (ey - 0.5) * depth;
            if m.mode == 3 && m.sub == 0 {
                // the ball (or X/Y controllers) offsets the envelope
                x += px - 0.5;
                y += py - 0.5;
            }
            x = x.clamp(0.0, 1.0);
            y = y.clamp(0.0, 1.0);
        }
        let mut v = [0.0f32; MORPH_N];
        if m.mode == 0 {
            v = sh.morph[sh.sel];
        } else {
            let w = morph_weights(x, y);
            for (i, wi) in w.iter().enumerate() {
                if *wi > 0.0 {
                    for k in 0..MORPH_N {
                        v[k] += wi * sh.morph[i][k];
                    }
                }
            }
        }
        MorphOut { v, x, y }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &Sh, g: &Glob) {
        self.control(sh, g);
        let n = l.len();
        let ovs = sh.ovs;
        let inv_ovs = 1.0 / ovs as f32;
        let mut peak = 0.0f32;
        let ticks = if self.held { 1 } else { 0 };
        for i in 0..n {
            let ext = if g.ext_pos + i < g.ext_n { g.ext[g.ext_pos + i] } else { 0.0 };
            for o in 0..3 {
                if self.op[o].kind != 0 {
                    self.objs[o].step(&mut self.guide, &self.op[o], ext, ticks);
                } else {
                    self.objs[o].last_kind = 0;
                }
            }
            let (mut a, mut b) = (0.0, 0.0);
            for _ in 0..ovs {
                self.guide.tick();
                a += self.guide.disp(self.pa);
                b += self.guide.disp(self.pb);
            }
            a *= PICK_GAIN * inv_ovs;
            b *= PICK_GAIN * inv_ovs;
            let amp = self.env.next();
            let mut xl = (a * self.gains[0] + b * self.gains[2]) * amp;
            let mut xr = (a * self.gains[1] + b * self.gains[3]) * amp;
            if sh.ws_on {
                xl = waveshape(sh.ws_ty, self.ws.0, self.ws.1, xl);
                xr = waveshape(sh.ws_ty, self.ws.0, self.ws.1, xr);
            }
            if sh.f_on {
                xl = self.filter(sh, 0, xl);
                xr = self.filter(sh, 1, xr);
            }
            let mut gain = OUT_GAIN * self.out_gain;
            if self.killing {
                gain *= self.fade.next();
            }
            xl *= gain;
            xr *= gain;
            if sh.limiter == 2 || sh.limiter == 3 {
                let gl = self.poly_lim.gain(xl.abs().max(xr.abs()), 0.5);
                xl *= gl;
                xr *= gl;
            }
            peak = peak.max(xl.abs()).max(xr.abs());
            l[i] += xl;
            r[i] += xr;
        }
        self.level = peak;
        if peak < 1e-6 && !self.held {
            self.silent += n as u32;
        } else {
            self.silent = 0;
        }
        if (self.killing && self.fade.gain <= 0.0) || self.env.idle() || self.silent > (sh.rate * 0.25) as u32 {
            self.on = false;
        }
    }

    #[inline]
    fn filter(&mut self, sh: &Sh, c: usize, x: f32) -> f32 {
        match sh.f_ty {
            2 => x + (self.f_g - 1.0) * self.filt[c].run_bp_norm(x),
            3 => self.filt[c].run_bp_norm(x),
            _ => {
                if sh.f_ty == 1 && self.cut_hz >= sh.rate * 0.44 {
                    x
                } else {
                    self.filt[c].run(x)
                }
            }
        }
    }
}


// =============================================================================================
// The instrument

#[derive(Clone)]
struct RecState {
    armed: bool,
    running: bool,
    t: f32,
    since: f32,
    interval: f32,
    n: usize,
    pts: [[f32; 3]; 256],
}

impl RecState {
    fn new() -> RecState {
        RecState { armed: false, running: false, t: 0.0, since: 0.0, interval: 0.02, n: 0, pts: [[0.0; 3]; 256] }
    }
    fn start(&mut self) {
        self.running = true;
        self.t = 0.0;
        self.since = 1.0;
        self.interval = 0.02;
        self.n = 0;
    }
    fn push(&mut self, a: f32, b: f32) {
        if self.n == 256 {
            // keep every other point and sample half as often
            for i in 0..128 {
                self.pts[i] = self.pts[i * 2];
            }
            self.n = 128;
            self.interval *= 2.0;
        }
        self.pts[self.n] = [self.t, a, b];
        self.n += 1;
    }
    /// The value of column `c` at time `t` of the recording.
    fn at(&self, t: f32, c: usize) -> f32 {
        if self.n == 0 {
            return 0.0;
        }
        if t <= self.pts[0][0] {
            return self.pts[0][1 + c];
        }
        for i in 1..self.n {
            if t <= self.pts[i][0] {
                let (a, b) = (self.pts[i - 1], self.pts[i]);
                let d = (b[0] - a[0]).max(1e-6);
                return a[1 + c] + (b[1 + c] - a[1 + c]) * ((t - a[0]) / d);
            }
        }
        self.pts[self.n - 1][1 + c]
    }
}

pub struct Sculpture {
    params: Params,
    sh: Sh,
    glob: Glob,
    voices: Vec<SVoice>,
    ch: Channel,
    rate: f32,
    delay: StereoDelay,
    body: BodyEq,
    lim: Limiter,
    counter: u64,
    held: Vec<u8>,
    last_note: f32,
    learn: Option<(u32, f32)>,
    rec: [RecState; 2],
    mrec: RecState,
    delay_dirty: bool,
    dirty: bool,
    pub bpm: f32,
}


impl Sculpture {
    pub fn new(rate: f32) -> Sculpture {
        dsp::warm_tables();
        let params = Params::new(params_table());
        let sh = Sh::load(&params, rate);
        let mut seed = 701u32;
        let mut s = Sculpture {
            params,
            sh,
            glob: Glob {
                ctrl: [0.0; 128],
                aftertouch: 0.0,
                bend: 0.0,
                bpm: 120.0,
                lfo_mono: [LfoState::new(11), LfoState::new(12), LfoState::new(13)],
                lfo_out: [0.0; 3],
                sustain: false,
                ext: vec![0.0; 16384],
                ext_n: 0,
                ext_pos: 0,
            },
            voices: (0..VOICES)
                .map(|_| {
                    seed += 17;
                    SVoice::new(rate, seed)
                })
                .collect(),
            ch: Channel::new(rate),
            rate,
            delay: StereoDelay::new(rate),
            body: BodyEq::new(rate),
            lim: Limiter::new(rate),
            counter: 0,
            held: Vec::with_capacity(128),
            last_note: 60.0,
            learn: None,
            rec: [RecState::new(), RecState::new()],
            mrec: RecState::new(),
            delay_dirty: true,
            dirty: false,
            bpm: 120.0,
        };
        s.refresh();
        s
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }

    /// Re-derives everything the voices and effects read from the parameters.
    fn refresh(&mut self) {
        self.sh = Sh::load(&self.params, self.rate);
        let link = self.p(BEND_LINK) >= 0.5;
        self.ch.bend_up = self.p(BEND_UP);
        self.ch.bend_down = if link { self.p(BEND_UP) } else { self.p(BEND_DOWN) };
        self.ch.update_bend();
        self.glob.bend = self.ch.ctl.bend;
        self.body.set(self.p(B_ON) >= 0.5, self.p(B_MODEL).round() as usize, self.p(B_P1), self.p(B_P2), self.p(B_P3), self.p(B_P4));
        self.delay.set_filters(self.p(D_LOCUT), self.p(D_HICUT));
    }

    // ---- values the window and tests use

    /// The stored value `k` (an index into `MORPH_IDS`) of morph point `p`.
    pub fn point_value(&self, p: usize, k: usize) -> f32 {
        self.p(ms(p.min(4), k.min(MORPH_N - 1)))
    }
    pub fn set_point_value(&mut self, p: usize, k: usize, v: f32) {
        self.set(ms(p.min(4), k.min(MORPH_N - 1)), v);
    }
    /// Where the morph ball is for the newest voice (the pad position, or the envelope's).
    pub fn morph_now(&self) -> Option<(f32, f32)> {
        self.newest().map(|v| v.morph_now)
    }
    /// How far the newest voice is along its morph envelope / control envelope (0..1).
    pub fn menv_progress(&self) -> Option<f32> {
        self.newest().map(|v| v.menv_t)
    }
    pub fn env_progress(&self, k: usize) -> Option<f32> {
        self.newest().map(|v| {
            let c = &self.sh.env[k.min(1)];
            let scale = c.scale * 0.01 * if c.sync { c.refbpm / self.glob.bpm.max(1.0) } else { 1.0 };
            let total: f32 = c.t.iter().sum::<f32>() * 0.001 * scale;
            (v.env_t[k.min(1)] / total.max(1e-6)).clamp(0.0, 1.0)
        })
    }
    fn newest(&self) -> Option<&SVoice> {
        self.voices.iter().filter(|v| v.on && !v.killing).max_by_key(|v| v.age)
    }
    /// The string's displacement along its length for the newest voice (for the display).
    pub fn string_shape(&self, out: &mut [f32]) -> bool {
        match self.newest() {
            Some(v) => {
                let n = out.len().max(2);
                for (i, o) in out.iter_mut().enumerate() {
                    *o = v.guide.disp(i as f32 / (n - 1) as f32) * PICK_GAIN;
                }
                true
            }
            None => false,
        }
    }
    /// True once after the instrument changed its own parameters (a recording finished, a
    /// randomise): the window then copies them into the project's settings.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.on).count()
    }
    /// Starts listening for the next controller move to assign to `param` (one of the
    /// controller assignment parameters); it gives up after 20 seconds.
    pub fn learn(&mut self, param: u32) {
        self.learn = Some((param, 20.0));
    }
    pub fn learning(&self) -> Option<u32> {
        self.learn.map(|l| l.0)
    }
    /// Loads a setting's values; with Controller Mode "Use Default" the current controller
    /// assignments are kept.
    pub fn apply_setting(&mut self, vals: &[(u32, f32)]) {
        let keep = self.p(CTRL_MODE) < 0.5;
        for &(id, v) in vals {
            if id == CTRL_MODE || (keep && (CTRL_A..=CTRL_MY).contains(&id)) {
                continue;
            }
            self.set(id, v);
        }
    }

    /// Randomises morph points (`points`) in a group of parameters by an intensity (0..1).
    pub fn randomize(&mut self, points: &[usize], group: usize, intensity: f32, seed: u32) {
        let mut rng = Rng::new(seed);
        self.dirty = true;
        for &pt in points {
            for k in 0..MORPH_N {
                let take = match group {
                    0 => true,
                    1 => k != 17,
                    2 => (14..=18).contains(&k),
                    3 => k < 14 && k % 4 != 0 || (12..14).contains(&k),
                    _ => (19..=22).contains(&k),
                };
                if !take {
                    continue;
                }
                let id = ms(pt, k);
                let Some(d) = params_table().iter().find(|d| d.id == id) else { continue };
                let cur = self.p(id);
                let nv = (cur + rng.bipolar() * (d.max - d.min) * intensity.clamp(0.0, 1.0)).clamp(d.min, d.max);
                self.set(id, nv);
            }
        }
    }

    // ---- parameters and their side effects

    fn load_point(&mut self, p: usize) {
        for k in 0..MORPH_N {
            let v = self.p(ms(p, k));
            self.params.set(MORPH_IDS[k], v);
        }
    }

    fn after_set(&mut self, id: u32) {
        let val = self.p(id);
        if let Some(k) = morph_index(id) {
            let sel = self.p(MORPH_SEL).round() as usize;
            self.params.set(ms(sel.min(4), k), val);
        } else if id == MORPH_SEL {
            self.load_point(val.round() as usize);
        } else if (MS_BASE..MS_BASE + (5 * MORPH_N) as u32).contains(&id) {
            let rel = (id - MS_BASE) as usize;
            let (p, k) = (rel / MORPH_N, rel % MORPH_N);
            if p == self.p(MORPH_SEL).round() as usize {
                self.params.set(MORPH_IDS[k], val);
            }
        } else if id == env(0, E_SYNC) || id == env(1, E_SYNC) {
            let i = if id == env(0, E_SYNC) { 0 } else { 1 };
            self.requantize(i, val >= 0.5);
        } else if id == MENV_SYNC {
            self.requantize_morph(val >= 0.5);
        } else if id == env(0, E_REC) || id == env(1, E_REC) {
            let i = if id == env(0, E_REC) { 0 } else { 1 };
            if val >= 0.5 {
                self.rec[i].armed = true;
                self.rec[i].running = false;
                // arming makes the controller the source (Ctrl mode on)
            } else if self.rec[i].running {
                self.finish_env_rec(i);
            } else {
                self.rec[i].armed = false;
            }
        } else if id == MENV_REC {
            if val >= 0.5 {
                self.mrec.armed = true;
                self.mrec.running = false;
                // arming sets the mode to Pad so the ball is heard while recording
                self.params.set(MENV_MODE, 1.0);
            } else if self.mrec.running {
                self.finish_morph_rec();
            } else {
                self.mrec.armed = false;
            }
        }
    }

    fn requantize(&mut self, i: usize, sync_on: bool) {
        let bpm = self.bpm.max(20.0);
        if sync_on {
            for k in 0..4 {
                let ms_ = self.p(env_t(i, k));
                let beats = ms_ * 0.001 * bpm / 60.0;
                let q = NOTE_VALUES[nearest_note(beats)].1;
                self.params.set(env_t(i, k), q * 60.0 / bpm * 1000.0);
            }
            self.params.set(env(i, E_REFBPM), bpm);
        } else {
            let f = self.p(env(i, E_REFBPM)) / bpm;
            for k in 0..4 {
                let v = (self.p(env_t(i, k)) * f).round();
                self.params.set(env_t(i, k), v);
            }
        }
    }

    fn requantize_morph(&mut self, sync_on: bool) {
        let bpm = self.bpm.max(20.0);
        if sync_on {
            for k in 1..9 {
                let beats = self.p(mn(k, 0)) * 0.001 * bpm / 60.0;
                let q = NOTE_VALUES[nearest_note(beats)].1;
                self.params.set(mn(k, 0), q * 60.0 / bpm * 1000.0);
            }
            self.params.set(MENV_REFBPM, bpm);
        } else {
            let f = self.p(MENV_REFBPM) / bpm;
            for k in 1..9 {
                let v = (self.p(mn(k, 0)) * f).round();
                self.params.set(mn(k, 0), v);
            }
        }
    }

    // ---- recording

    fn rec_event_note(&mut self, was_idle: bool) {
        for i in 0..2 {
            if self.rec[i].running && was_idle {
                self.finish_env_rec(i);
            } else if self.rec[i].armed && !self.rec[i].running && self.p(env(i, E_TRIG)) < 0.5 {
                self.rec[i].start();
            }
        }
        if self.mrec.running && was_idle {
            self.finish_morph_rec();
        } else if self.mrec.armed && !self.mrec.running && self.p(MENV_TRIG) < 0.5 {
            self.mrec.start();
        }
    }

    fn rec_event_pedal(&mut self) {
        if self.held.is_empty() {
            return;
        }
        for i in 0..2 {
            if self.rec[i].armed && !self.rec[i].running && (self.p(env(i, E_TRIG)) - 2.0).abs() < 0.5 {
                self.rec[i].start();
            }
        }
        if self.mrec.armed && !self.mrec.running && (self.p(MENV_TRIG) - 2.0).abs() < 0.5 {
            self.mrec.start();
        }
    }

    fn finish_env_rec(&mut self, i: usize) {
        let r = std::mem::replace(&mut self.rec[i], RecState::new());
        self.params.set(env(i, E_REC), 0.0);
        if r.n < 2 {
            return;
        }
        let total = r.pts[r.n - 1][0].max(0.02);
        for j in 0..REC_POINTS {
            let v = r.at(total * j as f32 / (REC_POINTS - 1) as f32, 0).clamp(0.0, 1.0);
            self.params.set(env_rec(i, j), v);
        }
        self.params.set(env(i, E_RECLEN), total * 1000.0);
        self.params.set(env(i, E_HASREC), 1.0);
        self.params.set(env(i, E_USEREC), 1.0);
        self.params.set(env(i, E_ENV), 1.0);
        self.params.set(env(i, E_CTRL), 0.0);
        self.dirty = true;
        // the handles are placed for you: the peak, then a loop and a sustain point
        let mut peak = 0.0;
        let mut tp = 0.0;
        for j in 0..REC_POINTS {
            let v = self.p(env_rec(i, j));
            if v > peak {
                peak = v;
                tp = total * j as f32 / (REC_POINTS - 1) as f32;
            }
        }
        let t1 = tp.max(0.002);
        let rest = (total - t1).max(0.006);
        let ts = [t1, rest * 0.3, rest * 0.4, rest * 0.3];
        let at = |t: f32| r.at(t, 0).clamp(0.0, 1.0);
        let marks = [0.0, t1, t1 + ts[1], t1 + ts[1] + ts[2], total];
        for k in 0..4 {
            self.params.set(env_t(i, k), (ts[k] * 1000.0).max(1.0));
        }
        for k in 0..5 {
            self.params.set(env_l(i, k), at(marks[k]));
        }
        self.refresh();
    }

    fn finish_morph_rec(&mut self) {
        let r = std::mem::replace(&mut self.mrec, RecState::new());
        self.params.set(MENV_REC, 0.0);
        if r.n < 2 {
            return;
        }
        let total = r.pts[r.n - 1][0].max(0.02);
        for i in 0..9 {
            let t = total * i as f32 / 8.0;
            self.params.set(mn(i, 1), r.at(t, 0).clamp(0.0, 1.0));
            self.params.set(mn(i, 2), r.at(t, 1).clamp(0.0, 1.0));
            if i > 0 {
                self.params.set(mn(i, 0), (total / 8.0 * 1000.0).max(1.0));
            }
        }
        self.params.set(MENV_MODE, 2.0);
        self.dirty = true;
        self.refresh();
    }

    // ---- notes

    fn count_live(&self) -> usize {
        self.voices.iter().filter(|v| v.on && !v.killing).count()
    }

    fn free_slot(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.on) {
            return i;
        }
        // all busy: take the quietest
        let mut best = 0;
        let mut bl = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if v.level < bl {
                bl = v.level;
                best = i;
            }
        }
        self.voices[best].on = false;
        best
    }

    fn steal_for_poly(&mut self) {
        while self.count_live() >= self.sh.poly {
            let mut best: Option<usize> = None;
            let mut score = f32::MAX;
            for (i, v) in self.voices.iter().enumerate() {
                if !v.on || v.killing {
                    continue;
                }
                let s = if v.released { 0.0 } else { 1e9 } + v.age as f32;
                if s < score {
                    score = s;
                    best = Some(i);
                }
            }
            match best {
                Some(i) => self.voices[i].kill(self.rate),
                None => break,
            }
        }
    }

    fn start_voice(&mut self, slot: usize, note: u8, vel: u8, chan: u8, from: f32, clear: bool) {
        self.counter += 1;
        let age = self.counter;
        let (sh, glob) = (&self.sh, &self.glob);
        let v = &mut self.voices[slot];
        v.mono = sh.kb != 0;
        v.start(note, vel, chan, sh, glob, from, clear, age);
    }

    fn do_note_on(&mut self, chan: u8, pitch: u8, velocity: u8) {
        let was_idle = self.held.is_empty();
        let held_before = self.held.len();
        if !self.held.contains(&pitch) && self.held.len() < 128 {
            self.held.push(pitch);
        }
        self.rec_event_note(was_idle);
        let note = pitch as f32;
        let true_retrigger = self.sh.a_soft <= 0.0005 && self.sh.a_hard <= 0.0005;
        match self.sh.kb {
            0 => {
                // a sounding voice of the same pitch is reused (true retrigger needs both
                // attack sliders at 0), otherwise it gives way to a new one
                if let Some(i) = self.voices.iter().position(|v| v.on && !v.killing && v.note == pitch && v.chan == chan) {
                    if true_retrigger {
                        let (sh, glob) = (&self.sh, &self.glob);
                        let v = &mut self.voices[i];
                        v.vel = velocity.max(1) as f32 / 127.0;
                        v.restart(sh, glob, false);
                        self.last_note = note;
                        return;
                    }
                    self.voices[i].kill(self.rate);
                }
                self.steal_for_poly();
                let slot = self.free_slot();
                let from = if self.sh.glide > 0.0 { self.last_note } else { note };
                self.start_voice(slot, pitch, velocity, chan, from, true);
                self.last_note = note;
            }
            _ => {
                // mono and legato: one voice, played legato it only changes pitch
                let legato = held_before > 0;
                if let Some(i) = self.voices.iter().position(|v| v.on && !v.killing && v.mono) {
                    let (sh, glob) = (&self.sh, &self.glob);
                    let v = &mut self.voices[i];
                    v.note = pitch;
                    v.vel = velocity.max(1) as f32 / 127.0;
                    if sh.kb == 2 && !(legato) && sh.glide > 0.0 {
                        // fingered portamento: a staccato note does not glide
                        v.cur = note;
                    }
                    if !legato {
                        if !true_retrigger {
                            v.guide.clear();
                        }
                        let keep = v.cur;
                        v.restart(sh, glob, false);
                        v.cur = keep;
                        if sh.glide <= 0.0 || sh.kb == 2 {
                            v.cur = note;
                        }
                    }
                } else {
                    let slot = self.free_slot();
                    let from = if self.sh.glide > 0.0 && self.sh.kb == 1 { self.last_note } else { note };
                    self.start_voice(slot, pitch, velocity, chan, from, true);
                    self.voices[slot].mono = true;
                }
                self.last_note = note;
            }
        }
    }

    fn do_note_off(&mut self, chan: u8, pitch: u8) {
        self.held.retain(|&n| n != pitch);
        let sh = self.sh.clone();
        if sh.kb != 0 {
            if let Some(&prev) = self.held.last() {
                // back to the key still held, without a new attack
                if let Some(v) = self.voices.iter_mut().find(|v| v.on && v.mono && v.held) {
                    v.note = prev;
                }
                return;
            }
        }
        let sustain = self.glob.sustain;
        let mono_ch = sh.mono_mode != 0;
        for v in &mut self.voices {
            if v.on && !v.killing && v.note == pitch && v.held && (!mono_ch || v.chan == chan) {
                if sustain {
                    v.held = false;
                    v.sustained = true;
                } else {
                    v.release(&sh);
                }
                if sh.kb == 0 {
                    break;
                }
            }
        }
    }

    /// Per-note pitch bend in MIDI Mono Mode: the bend of one channel (the base channel's
    /// bend moves every voice).
    pub fn pitch_bend_ch(&mut self, channel: u8, value: i16) {
        let base = if self.sh.mono_mode == 2 { 15 } else { 0 };
        if self.sh.mono_mode == 0 || channel == base {
            self.pitch_bend(value);
            return;
        }
        let semis = value as f32 / 8192.0 * self.sh.mono_bend;
        for v in &mut self.voices {
            if v.on && v.chan == channel {
                v.chan_bend = semis;
            }
        }
    }

    /// A controller on one channel in MIDI Mono Mode.
    pub fn controller_ch(&mut self, channel: u8, cc: u8, value: u8) {
        let base = if self.sh.mono_mode == 2 { 15 } else { 0 };
        if self.sh.mono_mode == 0 || channel == base {
            self.controller(cc, value);
            return;
        }
        for v in &mut self.voices {
            if v.on && v.chan == channel && cc < 128 {
                v.chan_ctrl[cc as usize] = value as f32 / 127.0;
            }
        }
    }

    // ---- block processing

    fn block_start(&mut self, m: usize) {
        let dt = m as f32 / self.rate;
        let bpm = self.glob.bpm;
        let ca = self.glob.ctrl[(self.sh.cc_a as usize).min(127)];
        let cb = self.glob.ctrl[(self.sh.cc_b as usize).min(127)];
        for k in 0..2 {
            let c = self.sh.lfo[k];
            if c.on && c.phase <= 0.0 {
                let mut hz = if c.sync { 1.0 / note_secs(c.note, bpm) } else { c.rate };
                if c.rmamt > 0.0 && (c.rmsrc == 5 || c.rmsrc == 6) {
                    let s = if c.rmsrc == 5 { ca } else { cb };
                    hz *= 2f32.powf((s * 2.0 - 1.0) * c.rmamt * 3.0);
                }
                self.glob.lfo_out[k] = self.glob.lfo_mono[k].step(c.wave, c.curve, hz * dt, ca);
            }
        }
        if self.sh.vib_phase <= 0.0 && self.sh.vib_amt > 0.0 {
            let hz = if self.sh.vib_sync { 1.0 / note_secs(self.sh.vib_note, bpm) } else { self.sh.vib_rate };
            self.glob.lfo_out[2] = self.glob.lfo_mono[2].step(self.sh.vib_wave, self.sh.vib_curve, hz * dt, ca);
        }
        if let Some((_, left)) = &mut self.learn {
            *left -= dt;
            if *left <= 0.0 {
                self.learn = None;
            }
        }
        // recording
        let any_on = self.voices.iter().any(|v| v.on);
        for i in 0..2 {
            if self.rec[i].running {
                let cc = self.sh.cc_env[i];
                let val = self.glob.ctrl[(cc as usize).min(127)];
                let r = &mut self.rec[i];
                r.t += dt;
                r.since += dt;
                if r.since >= r.interval {
                    r.since = 0.0;
                    r.push(val, 0.0);
                }
                if !any_on && r.t > 0.05 {
                    self.finish_env_rec(i);
                }
            }
        }
        if self.mrec.running {
            let (x, y) = (self.p(MORPH_X), self.p(MORPH_Y));
            let r = &mut self.mrec;
            r.t += dt;
            r.since += dt;
            if r.since >= r.interval {
                r.since = 0.0;
                r.push(x, y);
            }
            if !any_on && r.t > 0.05 {
                self.finish_morph_rec();
            }
        }
    }

    fn delay_set(&self) -> DelaySet {
        let bpm = self.glob.bpm;
        let base = if self.p(D_SYNC) >= 0.5 { note_secs(self.p(D_NOTE).round() as usize, bpm) } else { self.p(D_TIME) * 0.001 };
        let groove = self.p(D_GROOVE);
        let spread = self.p(D_SPREAD);
        let mut tl = base;
        let mut tr = base;
        if groove > 0.0 {
            tr *= 1.0 - groove.min(0.95);
        } else {
            tl *= 1.0 - (-groove).min(0.95);
        }
        tr *= 1.0 + spread * 0.5;
        tl *= 1.0 - spread * 0.5;
        DelaySet {
            on: self.p(D_ON) >= 0.5,
            time_l: tl,
            time_r: tr,
            fb: self.p(D_FB).clamp(-0.97, 0.97),
            xfeed: self.p(D_XFEED),
            wet: self.p(D_WET),
            lo: self.p(D_LOCUT),
            hi: self.p(D_HICUT),
            inbal: self.p(D_INBAL),
            width: self.p(D_WIDTH),
        }
    }
}

impl Plugin for Sculpture {
    fn params(&self) -> &[Param] {
        params_table()
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.after_set(id);
            self.refresh();
            if (D_ON..=D_GROOVE).contains(&id) {
                self.delay_dirty = true;
            }
        }
    }
    fn reset(&mut self) {
        for v in &mut self.voices {
            v.on = false;
            v.guide.clear();
            v.env = Adsr::default();
        }
        self.held.clear();
        self.delay.reset();
        self.body.reset();
        self.lim.reset();
        self.ch.reset();
        self.glob.sustain = false;
        self.refresh();
    }
    fn tail(&self) -> f32 {
        let d = if self.p(D_ON) >= 0.5 { 4.0 } else { 0.0 };
        self.p(A_RELEASE) + 2.0 + d
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        if (CTRL_A..=CTRL_MY).contains(&id) {
            return Some(cc_name(v.round() as u32));
        }
        if id == D_NOTE || id == VIB_NOTE || (0..2).any(|i| id == lfo(i, L_NOTE)) {
            return Some(NOTE_VALUES[(v.round().max(0.0) as usize).min(NOTE_VALUES.len() - 1)].0.to_string());
        }
        if id == TRANSPOSE {
            return Some(TRANSPOSES[((v.round() + 3.0).clamp(0.0, 6.0)) as usize].to_string());
        }
        None
    }
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for Sculpture {
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.do_note_on(0, pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        self.do_note_off(0, pitch);
    }
    fn note_on_ch(&mut self, channel: u8, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off_ch(channel, pitch);
        }
        let ch = if self.sh.mono_mode == 0 { 0 } else { channel };
        self.do_note_on(ch, pitch, velocity);
    }
    fn note_off_ch(&mut self, channel: u8, pitch: u8) {
        let ch = if self.sh.mono_mode == 0 { 0 } else { channel };
        self.do_note_off(ch, pitch);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            let sh = self.sh.clone();
            self.held.clear();
            for v in &mut self.voices {
                if v.on && !v.killing {
                    v.release(&sh);
                }
            }
            return;
        }
        if cc >= 120 {
            return;
        }
        if let Some((id, _)) = self.learn.take() {
            // the next moved controller becomes the assignment
            self.set(id, cc as f32);
            return;
        }
        let f = value as f32 / 127.0;
        self.glob.ctrl[cc as usize] = f;
        if let Some(down) = self.ch.cc(cc, value) {
            self.glob.sustain = down;
            if down {
                self.rec_event_pedal();
            } else {
                let sh = self.sh.clone();
                for v in &mut self.voices {
                    if v.on && v.sustained {
                        v.release(&sh);
                    }
                }
            }
        }
        // controller-driven recording and the morph pad
        if !self.held.is_empty() {
            for i in 0..2 {
                if self.rec[i].armed && !self.rec[i].running && cc as u32 == self.sh.cc_env[i] && (self.p(env(i, E_TRIG)) - 1.0).abs() < 0.5 {
                    self.rec[i].start();
                }
            }
        }
        let (mx, my) = (self.sh.cc_mx, self.sh.cc_my);
        if cc as u32 == mx || cc as u32 == my {
            if cc as u32 == mx {
                self.set(MORPH_X, f);
            } else {
                self.set(MORPH_Y, f);
            }
            if self.p(MORPH_AUTO) >= 0.5 {
                let n = nearest_point(self.p(MORPH_X), self.p(MORPH_Y)) as f32;
                if (n - self.p(MORPH_SEL)).abs() > 0.5 {
                    self.set(MORPH_SEL, n);
                }
            }
            if !self.held.is_empty() && self.mrec.armed && !self.mrec.running && (self.p(MENV_TRIG) - 1.0).abs() < 0.5 {
                self.mrec.start();
            }
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
        self.glob.bend = self.ch.ctl.bend;
    }
    fn pressure(&mut self, value: u8) {
        self.glob.aftertouch = value as f32 / 127.0;
    }
    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 1.0 {
            self.bpm = bpm as f32;
            self.glob.bpm = bpm as f32;
        }
    }
    fn set_transport(&mut self, t: Transport) {
        self.set_tempo(t.bpm);
    }
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len()).min(self.glob.ext.len());
        for i in 0..n {
            self.glob.ext[i] = (left[i] + right[i]) * 0.5;
        }
        self.glob.ext_n = n;
        self.glob.ext_pos = 0;
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CTRL];
        let mut sr = [0.0f32; CTRL];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.p(LEVEL));
        let lim_mono = self.sh.limiter == 1 || self.sh.limiter == 3;
        let ds = self.delay_set();
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CTRL);
            self.block_start(m);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            for v in &mut self.voices {
                if v.on {
                    v.render(&mut sl[..m], &mut sr[..m], &self.sh, &self.glob);
                }
            }
            for i in 0..m {
                sl[i] = self.body.run(0, sl[i]);
                sr[i] = self.body.run(1, sr[i]);
            }
            self.delay.process(&mut sl[..m], &mut sr[..m], &ds);
            for i in 0..m {
                let (mut a, mut b) = (sl[i] * gl * vol, sr[i] * gr * vol);
                if lim_mono {
                    let g = self.lim.gain(a.abs().max(b.abs()), 0.95);
                    a *= g;
                    b *= g;
                }
                left[pos + i] += a;
                right[pos + i] += b;
            }
            self.glob.ext_pos += m;
            pos += m;
        }
        if self.glob.ext_pos >= self.glob.ext_n {
            self.glob.ext_n = 0;
            self.glob.ext_pos = 0;
        }
    }
}
