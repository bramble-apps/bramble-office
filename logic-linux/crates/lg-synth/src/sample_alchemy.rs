//! Sample Alchemy: one sample resynthesized as a playable instrument. Four sources (A to D)
//! read the sample under the control of a handle (position and a second axis), in one of five
//! play modes (Classic, Loop, Scrub, Bow, Arp), through one of three engines (granular,
//! additive, spectral) with their effects, a filter module, amp and mod envelopes, four LFOs and
//! a mod matrix; Motion mode replays recorded handle movement in time with the tempo.
//!
//! Layout of the parameters: the global ones first, four LFOs from 40, the sources from 100 in
//! blocks of 64 (a fifth block, 356, is the global filter), the mod matrix routings from 400 and
//! the recorded motion from 600.

use std::sync::OnceLock;

use lg_dsp::{Instrument, Param, Plugin, Transport, Unit};

use crate::dsp::{fast_tanh, semis, sin_cycles, Adsr, Biquad, Delay, Ladder, Rng, Svf, SvfMode};
use crate::poly::{Channel, Params};
use crate::sample_alchemy_an::{Analysis, BANDS, PARTIALS};
use crate::synthkit::{bars_hz, cutoff_hz, env_time, Lfo, Shape, SYNC};

// ---------------------------------------------------------------------------------------------
// Parameters

pub const VOLUME: u32 = 0;
pub const POLY: u32 = 1;
pub const GLIDE: u32 = 2;
pub const PLAY_MODE: u32 = 3;
pub const BEND_UP: u32 = 4;
pub const BEND_DOWN: u32 = 5;
pub const LIMITER: u32 = 6;
pub const PREVIEW: u32 = 7;
pub const PREVIEW_KEY: u32 = 8;
pub const PITCH_LOCK: u32 = 9;
pub const REVERSE: u32 = 10;
pub const ROOT_KEY: u32 = 11;
pub const ROOT_CENTS: u32 = 12;
pub const TEMPO: u32 = 13;
pub const MIDI_MONO: u32 = 14;
pub const MPE_RANGE: u32 = 15;
pub const CTRL_A: u32 = 16;
pub const GLOBAL_FILTER: u32 = 20;
pub const SAMPLE_LOCK: u32 = 21;
pub const TRIM_START: u32 = 22;
pub const TRIM_END: u32 = 23;
pub const EDIT_MODE: u32 = 24;
pub const SNAP: u32 = 25;
pub const ZOOM_LO: u32 = 26;
pub const ZOOM_HI: u32 = 27;
pub const ENV_SEL: u32 = 28;
pub const MIXER_SHOW: u32 = 29;
pub const MATRIX_SHOW: u32 = 30;
pub const MOD_TARGET: u32 = 31;
pub const SRC_SEL: u32 = 32;
pub const MOTION_DUR: u32 = 33;
pub const MOTION_LS: u32 = 34;
pub const MOTION_LE: u32 = 35;
pub const MOTION_REC: u32 = 36;
pub const LFO_BASE: u32 = 40;
pub const SRC_BASE: u32 = 100;
pub const SRC_STRIDE: u32 = 64;
pub const ROUTE_BASE: u32 = 400;
pub const ROUTES: usize = 48;
pub const MOTION_BASE: u32 = 600;
pub const MOTION_STEPS: usize = 32;

/// Offsets inside a source's block.
pub mod o {
    pub const ON: u32 = 0;
    pub const MODE: u32 = 1;
    pub const EFFECT: u32 = 2;
    pub const K1: u32 = 3;
    pub const PLAY: u32 = 7;
    pub const X: u32 = 8;
    pub const Y: u32 = 9;
    pub const LOOP_END: u32 = 10;
    pub const SPEED: u32 = 11;
    pub const SPEED_SYNC: u32 = 12;
    pub const LOOP_SPEED: u32 = 13;
    pub const LOOP_SYNC: u32 = 14;
    pub const JITTER: u32 = 15;
    pub const BOW_RATE: u32 = 16;
    pub const BOW_SYNC: u32 = 17;
    pub const ARP_RATE: u32 = 18;
    pub const PAN: u32 = 19;
    pub const COARSE: u32 = 20;
    pub const FINE: u32 = 21;
    pub const VOL: u32 = 22;
    pub const F_ON: u32 = 23;
    pub const F_TYPE: u32 = 24;
    pub const F_VAR: u32 = 25;
    pub const F_CUT: u32 = 26;
    pub const F_RES: u32 = 27;
    pub const F_NOTE: u32 = 28;
    pub const F_FB: u32 = 29;
    pub const F_FREQ: u32 = 30;
    pub const F_MIX: u32 = 31;
    pub const F_MOD: u32 = 32;
    pub const AMP: u32 = 33; // A D S R
    pub const MODENV: u32 = 37; // A D S R
}

/// Id of a source's parameter (source 4 is the global filter's block).
pub const fn sid(src: usize, off: u32) -> u32 {
    SRC_BASE + src as u32 * SRC_STRIDE + off
}
pub const fn lfo_id(l: usize, k: u32) -> u32 {
    LFO_BASE + l as u32 * 3 + k
}
pub const fn route_id(i: usize, k: u32) -> u32 {
    ROUTE_BASE + i as u32 * 4 + k
}
pub const fn motion_id(src: usize, axis: usize, step: usize) -> u32 {
    MOTION_BASE + ((src * 2 + axis) * MOTION_STEPS + step) as u32
}

pub const MODES: &[&str] = &["Granular", "Additive", "Spectral"];
pub const PLAYS: &[&str] = &["Classic", "Loop", "Scrub", "Bow", "Arp"];
pub const PLAY_MODES: &[&str] = &["Always", "Retrigger", "Legato"];
pub const EDIT_MODES: &[&str] = &["Play", "Motion", "Trim"];
pub const SNAPS: &[&str] = &["Auto", "Off", "Transients", "Beat", "Fixed"];
pub const FILTER_TYPES: &[&str] = &["Lowpass", "Bandpass", "Highpass", "Comb PM", "Downsampler", "FM"];
pub const FILTER_VARS: &[&str] = &["Sharp", "Clean", "Gritty", "Edgy 2-pole", "Edgy 4-pole"];
pub const MONO_MODES: &[&str] = &["Off", "On (Common Base Channel 1)", "On (Common Base Channel 16)"];
pub const ENVS: &[&str] = &["Amp", "Mod"];
pub const SOURCES: &[&str] = &["A", "B", "C", "D"];
pub const LFO_SHAPES: &[&str] = &["Sine", "Triangle", "Saw Down", "Saw Up", "Square", "Sample & Hold", "Lag"];
pub const MOD_SOURCES: &[&str] = &[
    "None", "LFO 1", "LFO 2", "LFO 3", "LFO 4", "Amp Env", "Mod Env", "Waveform Y", "Mod Wheel", "Aftertouch", "Pitch Bend", "Velocity", "Key", "Ctrl A",
    "Ctrl B", "Ctrl C", "Ctrl D",
];
pub const ARP_RATES: &[&str] = &["1/1", "1/2", "1/2 d", "1/2 t", "1/4", "1/4 d", "1/4 t", "1/8", "1/8 d", "1/8 t", "1/16", "1/16 d", "1/16 t", "1/32", "1/32 d", "1/32 t"];
/// Length of each Arp rate in beats (quarter notes); the table is 1/1, then plain, dotted and triplet of each value down to 1/32.
pub fn arp_beats(i: usize) -> f32 {
    // table order: 1/1 then (plain, dotted, triplet) per value from 1/2 down
    if i == 0 {
        return 4.0;
    }
    let g = (i - 1) / 3;
    let base = [2.0, 1.0, 0.5, 0.25, 0.125][g.min(4)];
    match (i - 1) % 3 {
        0 => base,
        1 => base * 1.5,
        _ => base * 2.0 / 3.0,
    }
}

pub const GRAN_FX: &[&str] = &["Classic", "Sync"];
pub const ADD_FX: &[&str] = &["Harmonic", "Partials Lock", "Partials", "Formant", "Formant Synth"];
pub const SPEC_FX: &[&str] = &["Formant", "Low/High Cut", "Blur", "Cloud", "Metallize", "Noise"];

pub fn effects(mode: usize) -> &'static [&'static str] {
    match mode {
        0 => GRAN_FX,
        1 => ADD_FX,
        _ => SPEC_FX,
    }
}

/// The four knob names of an effect ("" for a knob the effect does not use).
pub fn knob_names(mode: usize, fx: usize) -> [&'static str; 4] {
    match (mode, fx) {
        (0, 0) => ["Size", "Density", "Random Pan", "Random Time"],
        (0, _) => ["Formant Shift", "Num Taps", "Tap Spacing", "Random Time"],
        (1, 0) => ["Odd/Even", "Fifths", "Octaves", "Fundamental"],
        (1, 1) => ["Symmetry", "Odd/Even", "Pitch Var", "Num Partials"],
        (1, 2) => ["Odd/Even", "Symmetry", "Pitch Var", "Num Partials"],
        (1, 3) => ["Shift", "Key Track", "Size", "Center"],
        (1, _) => ["Shift", "Size", "Center", "Vowel"],
        (_, 0) => ["Shift", "Key Track", "Size", "Center"],
        (_, 1) => ["Low Cut", "High Cut", "", ""],
        (_, 2) => ["Mix", "Length", "Variance", "Gate"],
        (_, 3) => ["Mix", "Threshold", "Attack", "Simplify"],
        (_, 4) => ["Mix", "Interval", "Simplify", "Feedback"],
        (_, _) => ["Low Cut", "High Cut", "Mix", ""],
    }
}

/// What the knobs are set to when an effect is chosen.
pub fn knob_defaults(mode: usize, fx: usize) -> [f32; 4] {
    match (mode, fx) {
        (0, 0) => [0.82, 0.11, 0.0, 0.03],
        (0, _) => [0.5, 0.0, 0.2, 0.03],
        (1, 0) => [0.5, 0.5, 0.5, 0.5],
        (1, 1) => [0.5, 0.5, 0.0, 1.0],
        (1, 2) => [0.5, 0.5, 1.0, 1.0],
        (1, 3) => [0.5, 1.0, 0.5, 0.5],
        (1, _) => [0.5, 0.5, 0.5, 0.0],
        (_, 0) => [0.5, 1.0, 0.5, 0.5],
        (_, 1) => [0.0, 1.0, 0.0, 0.0],
        (_, 2) => [0.5, 0.5, 0.0, 0.0],
        (_, 3) => [0.5, 0.5, 0.3, 0.0],
        (_, 4) => [0.5, 0.5, 0.0, 0.5],
        (_, _) => [0.0, 1.0, 1.0, 0.0],
    }
}

/// Modulation targets per source: the four knobs, the first source-module parameter, pan,
/// coarse and fine tune, volume, and the filter's cutoff, resonance and note/frequency.
pub const TARGETS_PER_SRC: usize = 12;
const TARGET_NAMES: [&str; TARGETS_PER_SRC] = ["Knob 1", "Knob 2", "Knob 3", "Knob 4", "Source Speed", "Pan", "Tune Coarse", "Tune Fine", "Volume", "Filter Cutoff", "Filter Res", "Filter Note"];
pub fn target_name(t: usize) -> String {
    format!("{} {}", SOURCES[(t / TARGETS_PER_SRC).min(3)], TARGET_NAMES[t % TARGETS_PER_SRC])
}
/// The parameter a mod target adjusts, given the source's play mode.
pub fn target_param(t: usize, play: usize) -> u32 {
    let s = t / TARGETS_PER_SRC;
    let off = match t % TARGETS_PER_SRC {
        0 => o::K1,
        1 => o::K1 + 1,
        2 => o::K1 + 2,
        3 => o::K1 + 3,
        4 => match play {
            0 => o::SPEED,
            1 => o::LOOP_SPEED,
            2 => o::JITTER,
            3 => o::BOW_RATE,
            _ => o::SPEED,
        },
        5 => o::PAN,
        6 => o::COARSE,
        7 => o::FINE,
        8 => o::VOL,
        9 => o::F_CUT,
        10 => o::F_RES,
        _ => o::F_NOTE,
    };
    sid(s, off)
}
/// The mod target (0..48) a parameter is, if it is one.
pub fn target_of(id: u32) -> Option<usize> {
    if !(SRC_BASE..SRC_BASE + 4 * SRC_STRIDE).contains(&id) {
        return None;
    }
    let s = ((id - SRC_BASE) / SRC_STRIDE) as usize;
    let off = (id - SRC_BASE) % SRC_STRIDE;
    let t = match off {
        3..=6 => off as usize - 3,
        o::SPEED | o::LOOP_SPEED | o::JITTER | o::BOW_RATE => 4,
        o::PAN => 5,
        o::COARSE => 6,
        o::FINE => 7,
        o::VOL => 8,
        o::F_CUT => 9,
        o::F_RES => 10,
        o::F_NOTE => 11,
        _ => return None,
    };
    Some(s * TARGETS_PER_SRC + t)
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn leak_list(v: Vec<String>) -> &'static [&'static str] {
    let v: Vec<&'static str> = v.into_iter().map(leak).collect();
    Box::leak(v.into_boxed_slice())
}

pub fn table() -> &'static [Param] {
    static T: OnceLock<Vec<Param>> = OnceLock::new();
    T.get_or_init(build_params)
}

fn build_params() -> Vec<Param> {
    use Unit::*;
    let mut p: Vec<Param> = Vec::new();
    p.push(Param::new(VOLUME, "Volume", -36.0, 6.0, 0.0, Db));
    p.push(Param::new(POLY, "Polyphony", 1.0, 16.0, 16.0, None));
    p.push(Param::new(GLIDE, "Glide", 0.0, 5.0, 0.0, Seconds));
    p.push(Param::menu(PLAY_MODE, "Play Mode", PLAY_MODES, 0.0));
    p.push(Param::new(BEND_UP, "Bend Up", 0.0, 24.0, 2.0, Semitones));
    p.push(Param::new(BEND_DOWN, "Bend Down", 0.0, 24.0, 2.0, Semitones));
    p.push(Param::new(LIMITER, "Output Limiter", 0.0, 1.0, 0.0, OnOff));
    p.push(Param::new(PREVIEW, "Preview Mode", 0.0, 1.0, 1.0, OnOff));
    p.push(Param::new(PREVIEW_KEY, "Preview Key", 0.0, 127.0, 60.0, None));
    p.push(Param::new(PITCH_LOCK, "Pitch Lock", 0.0, 1.0, 0.0, OnOff));
    p.push(Param::new(REVERSE, "Reverse", 0.0, 1.0, 0.0, OnOff));
    p.push(Param::new(ROOT_KEY, "Root Key", 0.0, 127.0, 60.0, None));
    p.push(Param::new(ROOT_CENTS, "Root Cents", -100.0, 100.0, 0.0, Cents));
    p.push(Param::new(TEMPO, "Tempo", 0.0, 300.0, 0.0, None));
    p.push(Param::menu(MIDI_MONO, "MIDI Mono Mode", MONO_MODES, 0.0));
    p.push(Param::new(MPE_RANGE, "MPE Pitch Bend Range", 0.0, 96.0, 48.0, Semitones));
    for (i, n) in ["Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D"].into_iter().enumerate() {
        p.push(Param::new(CTRL_A + i as u32, n, 0.0, 127.0, 16.0 + i as f32, None));
    }
    p.push(Param::new(GLOBAL_FILTER, "Global Filter", 0.0, 1.0, 0.0, OnOff));
    p.push(Param::new(SAMPLE_LOCK, "Sample Lock", 0.0, 1.0, 0.0, OnOff));
    p.push(Param::new(TRIM_START, "Trim Start", 0.0, 1.0, 0.0, None).hide());
    p.push(Param::new(TRIM_END, "Trim End", 0.0, 1.0, 1.0, None).hide());
    p.push(Param::menu(EDIT_MODE, "Edit Mode", EDIT_MODES, 0.0).hide());
    p.push(Param::menu(SNAP, "Snap", SNAPS, 0.0));
    p.push(Param::new(ZOOM_LO, "Zoom Start", 0.0, 1.0, 0.0, None).hide());
    p.push(Param::new(ZOOM_HI, "Zoom End", 0.0, 1.0, 1.0, None).hide());
    p.push(Param::menu(ENV_SEL, "Envelope", ENVS, 0.0).hide());
    p.push(Param::new(MIXER_SHOW, "Mixer", 0.0, 1.0, 0.0, OnOff).hide());
    p.push(Param::new(MATRIX_SHOW, "Mod Matrix", 0.0, 1.0, 0.0, OnOff).hide());
    p.push(Param::new(MOD_TARGET, "Mod Target", 0.0, ROUTES as f32, 0.0, None).hide());
    p.push(Param::new(SRC_SEL, "Source", 0.0, 4.0, 0.0, None).hide());
    p.push(Param::new(MOTION_DUR, "Motion Duration", 1.0, 64.0, 4.0, None));
    p.push(Param::new(MOTION_LS, "Motion Loop Start", 0.0, 64.0, 0.0, None));
    p.push(Param::new(MOTION_LE, "Motion Loop End", 0.0, 64.0, 4.0, None));
    p.push(Param::new(MOTION_REC, "Motion Record", 0.0, 1.0, 0.0, OnOff).hide());
    // LFOs
    let sync_names: Vec<String> = std::iter::once("Free".to_string()).chain(SYNC.iter().map(|s| s.0.to_string())).collect();
    let sync_list = leak_list(sync_names);
    for l in 0..4 {
        p.push(Param::menu(lfo_id(l, 0), leak(format!("LFO {} Wave", l + 1)), LFO_SHAPES, 0.0));
        p.push(Param::new(lfo_id(l, 1), leak(format!("LFO {} Rate", l + 1)), 0.05, 30.0, 2.0, Hz));
        p.push(Param::menu(lfo_id(l, 2), leak(format!("LFO {} Sync", l + 1)), sync_list, 0.0));
    }
    // sources (and the global filter block)
    let bow_list = {
        let v: Vec<String> = std::iter::once("Hz".to_string()).chain(SYNC.iter().map(|s| s.0.to_string())).collect();
        leak_list(v)
    };
    for s in 0..5usize {
        let tag = if s < 4 { SOURCES[s].to_string() } else { "Global".to_string() };
        let n = |x: &str| leak(format!("{tag} {x}"));
        if s < 4 {
            p.push(Param::new(sid(s, o::ON), n("On"), 0.0, 1.0, if s == 0 { 1.0 } else { 0.0 }, OnOff));
            p.push(Param::menu(sid(s, o::MODE), n("Synthesis"), MODES, 0.0));
            p.push(Param::new(sid(s, o::EFFECT), n("Effect"), 0.0, 5.0, 0.0, None));
            let d = knob_defaults(0, 0);
            for k in 0..4 {
                p.push(Param::new(sid(s, o::K1 + k as u32), n(&format!("Knob {}", k + 1)), 0.0, 1.0, d[k], None));
            }
            p.push(Param::menu(sid(s, o::PLAY), n("Play Mode"), PLAYS, 0.0));
            p.push(Param::new(sid(s, o::X), n("Handle X"), 0.0, 1.0, 0.0, None));
            p.push(Param::new(sid(s, o::Y), n("Handle Y"), 0.0, 1.0, 0.5, None));
            p.push(Param::new(sid(s, o::LOOP_END), n("Loop End"), 0.0, 1.0, 1.0, None));
            p.push(Param::new(sid(s, o::SPEED), n("Playback Speed"), -4.0, 4.0, 1.0, Ratio));
            p.push(Param::new(sid(s, o::SPEED_SYNC), n("Speed Sync"), 0.0, 1.0, 0.0, OnOff));
            p.push(Param::new(sid(s, o::LOOP_SPEED), n("Loop Speed"), -4.0, 4.0, 1.0, Ratio));
            p.push(Param::new(sid(s, o::LOOP_SYNC), n("Loop Speed Sync"), 0.0, 1.0, 0.0, OnOff));
            p.push(Param::new(sid(s, o::JITTER), n("Scrub Jitter"), 0.0, 1.0, 0.0, None));
            p.push(Param::new(sid(s, o::BOW_RATE), n("Bow Rate"), 0.1, 20.0, 2.0, Hz));
            p.push(Param::menu(sid(s, o::BOW_SYNC), n("Bow Rate Sync"), bow_list, 0.0));
            p.push(Param::menu(sid(s, o::ARP_RATE), n("Arp Rate"), ARP_RATES, 7.0));
            p.push(Param::new(sid(s, o::PAN), n("Pan"), -1.0, 1.0, 0.0, Pan));
            p.push(Param::new(sid(s, o::COARSE), n("Tune Coarse"), -36.0, 36.0, 0.0, Semitones));
            p.push(Param::new(sid(s, o::FINE), n("Tune Fine"), -100.0, 100.0, 0.0, Cents));
            p.push(Param::new(sid(s, o::VOL), n("Volume"), 0.0, 1.0, 0.8, None));
        }
        p.push(Param::new(sid(s, o::F_ON), n("Filter On"), 0.0, 1.0, 0.0, OnOff));
        p.push(Param::menu(sid(s, o::F_TYPE), n("Filter Type"), FILTER_TYPES, 0.0));
        p.push(Param::menu(sid(s, o::F_VAR), n("Filter Variant"), FILTER_VARS, 0.0));
        p.push(Param::new(sid(s, o::F_CUT), n("Filter Cutoff"), 0.0, 1.0, 1.0, None));
        p.push(Param::new(sid(s, o::F_RES), n("Filter Res"), 0.0, 1.0, 0.1, None));
        p.push(Param::new(sid(s, o::F_NOTE), n("Filter Note"), 0.0, 127.0, 60.0, None));
        p.push(Param::new(sid(s, o::F_FB), n("Filter Feedback"), 0.0, 1.0, 0.5, None));
        p.push(Param::new(sid(s, o::F_FREQ), n("Downsample Freq"), 0.0, 1.0, 0.6, None));
        p.push(Param::new(sid(s, o::F_MIX), n("Downsample Mix"), 0.0, 1.0, 1.0, None));
        p.push(Param::new(sid(s, o::F_MOD), n("FM Mod"), 0.0, 1.0, 0.3, None));
        if s < 4 {
            let defs = [[0.0, 0.7, 1.0, 0.5], [0.0, 0.5, 0.0, 0.5]];
            for (e, name) in ["Amp", "Mod"].into_iter().enumerate() {
                for (k, l) in ["Attack", "Decay", "Sustain", "Release"].into_iter().enumerate() {
                    let base = if e == 0 { o::AMP } else { o::MODENV };
                    p.push(Param::new(sid(s, base + k as u32), n(&format!("{name} {l}")), 0.0, 1.0, defs[e][k], None));
                }
            }
        }
    }
    // the mod matrix
    let targets: Vec<String> = (0..ROUTES).map(target_name).chain(std::iter::once("None".to_string())).collect();
    let _ = targets;
    for i in 0..ROUTES {
        p.push(Param::new(route_id(i, 0), leak(format!("Route {} Target", i + 1)), 0.0, ROUTES as f32, ROUTES as f32, None).hide());
        p.push(Param::menu(route_id(i, 1), leak(format!("Route {} Source", i + 1)), MOD_SOURCES, 0.0).hide());
        p.push(Param::new(route_id(i, 2), leak(format!("Route {} Depth", i + 1)), -1.0, 1.0, 0.0, None).hide());
        p.push(Param::new(route_id(i, 3), leak(format!("Route {} On", i + 1)), 0.0, 1.0, 1.0, Unit::OnOff).hide());
    }
    // recorded motion (-1: nothing recorded at that step)
    for s in 0..4 {
        for a in 0..2 {
            for k in 0..MOTION_STEPS {
                p.push(Param::new(motion_id(s, a, k), leak(format!("Motion {} {} {}", SOURCES[s], if a == 0 { "X" } else { "Y" }, k + 1)), -1.0, 1.0, -1.0, None).hide());
            }
        }
    }
    p
}

/// The routing every fresh instrument starts with: the handle's vertical movement drives each
/// source's first synthesis-module knob.
fn default_route(i: usize) -> Option<(usize, usize, f32)> {
    if i < 4 {
        Some((i * TARGETS_PER_SRC, 7, 1.0))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Value texts

fn note_name(n: f32) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let n = n.round() as i32;
    format!("{}{}", N[n.rem_euclid(12) as usize], n.div_euclid(12) - 2)
}

pub fn size_ms(k: f32) -> f32 {
    2.0 * 115f32.powf(k.clamp(0.0, 1.0))
}
pub fn centre_hz(k: f32) -> f32 {
    100.0 * 80f32.powf(k.clamp(0.0, 1.0))
}
fn size_ratio(k: f32) -> f32 {
    2f32.powf((k - 0.5) * 2.0)
}
fn shift_semis(k: f32, range: f32) -> f32 {
    (k - 0.5) * 2.0 * range
}

fn knob_text(mode: usize, fx: usize, i: usize, k: f32) -> String {
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    match (mode, fx, i) {
        (0, 0, 0) => format!("{:.0} ms", size_ms(k)),
        (0, 0, 1) => format!("{}", 1 + (k * 9.0).round() as i32),
        (0, 1, 0) => format!("{:+.1} st", shift_semis(k, 36.0)),
        (0, 1, 1) => format!("{}", 1 + (k * 7.0).round() as i32),
        (1, 0, 0) | (1, 1, 1) | (1, 2, 0) => {
            let d = (k - 0.5) * 200.0;
            if d.abs() < 0.5 { "Even = Odd".into() } else if d < 0.0 { format!("Odd {:.0}", -d) } else { format!("Even {:.0}", d) }
        }
        (1, 1, 3) | (1, 2, 3) => format!("{}", 1 + (k * 63.0).round() as i32),
        (1, 3, 0) | (1, 4, 0) | (2, 0, 0) => format!("{:+.1} st", shift_semis(k, 24.0)),
        (1, 3, 2) | (1, 4, 1) | (2, 0, 2) => pct(size_ratio(k)),
        (1, 3, 3) | (1, 4, 2) | (2, 0, 3) => format!("{:.0} Hz", centre_hz(k)),
        (1, 4, 3) => format!("{:.2}", k * 3.0 + 1.0),
        (2, 1, 0) | (2, 1, 1) | (2, 5, 0) | (2, 5, 1) => format!("{:.0} Hz", cutoff_hz(k)),
        (2, 2, 1) => format!("{:.2} s", blur_len(k)),
        (2, 4, 1) => format!("{:.0} st", k * 24.0),
        _ => pct(k),
    }
}

fn blur_len(k: f32) -> f32 {
    0.05 + 3.0 * k * k
}

fn path_of(id: u32) -> Option<(usize, u32)> {
    if (SRC_BASE..SRC_BASE + 5 * SRC_STRIDE).contains(&id) {
        Some((((id - SRC_BASE) / SRC_STRIDE) as usize, (id - SRC_BASE) % SRC_STRIDE))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// The oscillator bank shared by the additive and spectral engines

const NB: usize = BANDS; // 128, the widest bank
const CH: usize = 64;

struct Bank {
    phase: [f32; NB],
    amp: [f32; NB],
}

impl Bank {
    fn new() -> Bank {
        Bank { phase: [0.0; NB], amp: [0.0; NB] }
    }
    /// Renders `n` samples of sines (`inc` cycles per sample, amplitudes ramping from the last
    /// block's to `amp`) into `out`; `sym` bends each sine's shape (adds odd harmonics).
    fn run(&mut self, amp: &[f32; NB], inc: &[f32; NB], cnt: usize, n: usize, sym: f32, out: &mut [f32; CH]) {
        let inv = 1.0 / n as f32;
        for k in 0..cnt.min(NB) {
            let (a0, a1) = (self.amp[k], amp[k]);
            let step = inc[k];
            if a0.abs() < 1e-6 && a1.abs() < 1e-6 {
                self.phase[k] = (self.phase[k] + step * n as f32).fract();
                self.amp[k] = a1;
                continue;
            }
            let ph = self.phase[k];
            let (mut s, mut c) = (sin_cycles(ph), sin_cycles(ph + 0.25));
            let (sd, cd) = (sin_cycles(step), sin_cycles(step + 0.25));
            let da = (a1 - a0) * inv;
            let mut a = a0;
            if sym == 0.0 {
                for o in out.iter_mut().take(n) {
                    *o += a * s;
                    a += da;
                    let ns = s * cd + c * sd;
                    c = c * cd - s * sd;
                    s = ns;
                }
            } else {
                // the sine's time axis is squeezed in one half and stretched in the other (a
                // mirror image for the other sign, so both directions brighten the same)
                let w = (0.5 + sym * 0.55).clamp(0.05, 0.95);
                let dc = (2.0 * w - 1.0) * std::f32::consts::FRAC_2_PI;
                let mut p = ph;
                for o in out.iter_mut().take(n) {
                    let f = p - p.floor();
                    let t = if f < w { 0.5 * f / w } else { 0.5 + 0.5 * (f - w) / (1.0 - w) };
                    *o += a * (sin_cycles(t) - dc);
                    a += da;
                    p += step;
                }
            }
            self.phase[k] = (ph + step * n as f32).fract();
            self.amp[k] = a1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Filter module

struct Filt {
    svf: Svf,
    svf2: Svf,
    bq: Biquad,
    lad: Ladder,
    comb: Delay,
    comb_lp: f32,
    hold: f32,
    dphase: f32,
    cphase: f32,
    follow: f32,
    mode: u8,
}

impl Filt {
    fn new(rate: f32) -> Filt {
        Filt {
            svf: Svf::default(),
            svf2: Svf::default(),
            bq: Biquad::default(),
            lad: Ladder::new(),
            comb: Delay::new((rate / 20.0) as usize + 8),
            comb_lp: 0.0,
            hold: 0.0,
            dphase: 0.0,
            cphase: 0.0,
            follow: 0.0,
            mode: 255,
        }
    }
    fn reset(&mut self) {
        self.svf.reset();
        self.svf2.reset();
        self.bq.reset();
        self.lad.reset();
        self.comb.clear();
        self.comb_lp = 0.0;
        self.hold = 0.0;
        self.follow = 0.0;
    }
}

#[derive(Clone, Copy, Default)]
struct FCfg {
    on: bool,
    ty: usize,
    var: usize,
    cut: f32,
    res: f32,
    note: f32,
    fb: f32,
    freq: f32,
    mix: f32,
    modd: f32,
}

#[derive(Clone, Copy)]
struct FRun {
    ty: usize,
    var: usize,
    delay: f32,
    fb: f32,
    dinc: f32,
    mix: f32,
    cinc: f32,
    modd: f32,
    res: f32,
}

impl Filt {
    /// Sets the coefficients for the next block.
    fn config(&mut self, c: &FCfg, rate: f32) -> FRun {
        let hz = cutoff_hz(c.cut).min(rate * 0.45);
        let res = c.res.clamp(0.0, 1.0);
        let q = 0.55 + res * res * 14.0;
        let sig = (c.ty * 8 + c.var) as u8;
        if sig != self.mode {
            self.reset();
            self.mode = sig;
        }
        match c.ty {
            0 | 1 | 2 => {
                let mode = [SvfMode::Low, SvfMode::Band, SvfMode::High][c.ty];
                match c.var {
                    0 => {
                        self.svf.mode = mode;
                        self.svf.set(hz, q, rate);
                    }
                    1 => match c.ty {
                        0 => self.bq.lowpass(hz, q, rate),
                        1 => self.bq.bandpass(hz, q, rate),
                        _ => self.bq.highpass(hz, q, rate),
                    },
                    2 => {
                        self.svf.mode = mode;
                        self.svf.set(hz, q * 0.8, rate);
                    }
                    _ => {
                        self.lad.poles = if c.var == 3 { 2 } else { 4 };
                        self.lad.drive = 1.0;
                        self.lad.set(hz, res * 0.98, rate);
                        self.svf.mode = mode;
                        self.svf.set(hz, q, rate);
                        self.svf2.mode = mode;
                        self.svf2.set(hz, 0.7 + (q - 0.7) * 0.5, rate);
                    }
                }
            }
            _ => {}
        }
        let f = 440.0 * semis(c.note - 69.0);
        let ds = (rate / f.clamp(16.0, rate * 0.4)).min(self.comb.len() as f32 - 3.0);
        FRun {
            ty: c.ty,
            var: c.var,
            delay: ds.max(2.0),
            fb: c.fb.clamp(0.0, 0.985),
            dinc: (100.0 * (rate * 0.5 / 100.0).powf(c.freq.clamp(0.0, 1.0)) / rate).min(1.0),
            mix: c.mix.clamp(0.0, 1.0),
            cinc: f / rate,
            modd: c.modd.clamp(0.0, 1.0),
            res,
        }
    }

    #[inline]
    fn run(&mut self, x: f32, r: &FRun) -> f32 {
        match r.ty {
            0..=2 => match r.var {
                0 => {
                    if r.ty == 1 {
                        self.svf.run_bp_norm(x)
                    } else {
                        self.svf.run(x)
                    }
                }
                1 => self.bq.run(x),
                2 => {
                    // grit: the input is driven into a tanh the harder the resonance
                    let d = 1.0 + r.res * r.res * 7.0;
                    let xi = fast_tanh(x * d) / d.sqrt();
                    let y = if r.ty == 1 { self.svf.run_bp_norm(xi) } else { self.svf.run(xi) };
                    fast_tanh(y * (1.0 + r.res * 2.0))
                }
                _ => match r.ty {
                    0 => self.lad.run(x),
                    1 => self.svf2.run_bp_norm(self.svf.run_bp_norm(x)),
                    _ => self.svf2.run(self.svf.run(x)),
                },
            },
            3 => {
                // Comb PM: a feedback comb tuned to the note, its delay nudged by the signal
                let d = (r.delay * (1.0 + 0.004 * x)).clamp(2.0, self.comb.len() as f32 - 3.0);
                let del = self.comb.read(d);
                self.comb_lp += (del - self.comb_lp) * 0.6;
                let y = x + r.fb * self.comb_lp;
                self.comb.push(crate::dsp::flush(y));
                y * (1.0 - 0.5 * r.fb)
            }
            4 => {
                self.dphase += r.dinc;
                if self.dphase >= 1.0 {
                    self.dphase -= 1.0;
                    self.hold = x;
                }
                x + (self.hold - x) * r.mix
            }
            _ => {
                // FM: the source bends the frequency of a sine carrier
                self.cphase += r.cinc * (1.0 + r.modd * 3.0 * x);
                self.cphase -= self.cphase.floor();
                self.follow = (x.abs()).max(self.follow * 0.9995);
                sin_cycles(self.cphase) * (self.follow * 6.0).min(1.0) * 0.7
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Settings read once per block

#[derive(Clone, Copy, Default)]
struct SrcCfg {
    on: bool,
    mode: usize,
    fx: usize,
    k: [f32; 4],
    play: usize,
    x: f32,
    y: f32,
    loop_end: f32,
    speed: f32,
    speed_sync: bool,
    loop_speed: f32,
    loop_sync: bool,
    jitter: f32,
    bow_rate: f32,
    bow_sync: usize,
    arp_rate: usize,
    pan: f32,
    coarse: f32,
    fine: f32,
    vol: f32,
    f: FCfg,
    amp: [f32; 4],
    menv: [f32; 4],
}

struct Shared {
    rate: f32,
    srate: f32,
    sample: Vec<f32>,
    an: Analysis,
    cfg: [SrcCfg; 4],
    lfo: [(Shape, f32, usize); 4],
    routes: [(u8, u8, f32); ROUTES],
    motion: [[f32; MOTION_STEPS]; 8],
    motion_has: [bool; 8],
    motion_dur: f32,
    motion_ls: f32,
    motion_le: f32,
    trim: (f32, f32),
    reverse: bool,
    pitch_lock: bool,
    root: f32,
    root_cents: f32,
    tempo: f32,
    bpm: f32,
    ctrl: [f32; 4],
    glide: f32,
}

// ---------------------------------------------------------------------------------------------
// Voices

const MAXG: usize = 64;

#[derive(Clone, Copy, Default)]
struct Grain {
    on: bool,
    pos: f32,
    inc: f32,
    age: f32,
    len: f32,
    gl: f32,
    gr: f32,
}

struct SrcVoice {
    started: bool,
    ended: bool,
    pos: f32,
    dir: f32,
    bow_t: f32,
    arp_t: f32,
    arp_step: u32,
    fade_in: bool,
    jit: f32,
    grains: [Grain; MAXG],
    until: [f32; 8],
    bank: Bank,
    blur: [f32; NB],
    cloud: [f32; NB],
    nz: [f32; NB],
    hash: [f32; NB],
    amp_env: Adsr,
    mod_env: Adsr,
    filt: [Filt; 2],
    rng: Rng,
    lock_phase: bool,
    fresh: bool,
}

impl SrcVoice {
    fn new(rate: f32, seed: u32) -> SrcVoice {
        let mut rng = Rng::new(seed);
        let mut hash = [0.0; NB];
        for h in hash.iter_mut() {
            *h = rng.unit();
        }
        SrcVoice {
            started: false,
            ended: false,
            pos: 0.0,
            dir: 1.0,
            bow_t: 0.0,
            arp_t: 0.0,
            arp_step: 0,
            fade_in: false,
            jit: 0.0,
            grains: [Grain::default(); MAXG],
            until: [0.0; 8],
            bank: Bank::new(),
            blur: [0.0; NB],
            cloud: [0.0; NB],
            nz: [0.0; NB],
            hash,
            amp_env: Adsr::default(),
            mod_env: Adsr::default(),
            filt: [Filt::new(rate), Filt::new(rate)],
            rng,
            lock_phase: false,
            fresh: true,
        }
    }

    fn start(&mut self, cfg: &SrcCfg) {
        self.started = false;
        self.ended = false;
        self.dir = 1.0;
        self.bow_t = 0.0;
        self.arp_t = 0.0;
        self.arp_step = 0;
        self.fade_in = true;
        self.jit = 0.0;
        for g in self.grains.iter_mut() {
            g.on = false;
        }
        self.until = [0.0; 8];
        self.blur = [0.0; NB];
        self.cloud = [0.0; NB];
        self.nz = [0.0; NB];
        self.bank.amp = [0.0; NB];
        self.lock_phase = cfg.mode == 1 && cfg.fx == 1;
        self.fresh = true;
        for k in 0..NB {
            self.bank.phase[k] = if self.lock_phase { 0.0 } else { self.rng.unit() };
        }
        for f in self.filt.iter_mut() {
            f.reset();
        }
        self.amp_env.trigger();
        self.mod_env.trigger();
    }

    fn release(&mut self) {
        self.amp_env.gate_off();
        self.mod_env.gate_off();
    }

    fn idle(&self) -> bool {
        self.amp_env.idle()
    }
}

struct Voice {
    active: bool,
    held: bool,
    pedal: bool,
    note: u8,
    ch: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    glide: bool,
    born: u64,
    beats: f32,
    lfo: [Lfo; 4],
    src: [SrcVoice; 4],
}

impl Voice {
    fn new(rate: f32, i: usize) -> Voice {
        Voice {
            active: false,
            held: false,
            pedal: false,
            note: 0,
            ch: 0,
            vel: 0.0,
            pitch: 60.0,
            target: 60.0,
            glide: false,
            born: 0,
            beats: 0.0,
            lfo: [Lfo::default(); 4],
            src: std::array::from_fn(|s| SrcVoice::new(rate, 77 + (i * 4 + s) as u32 * 31)),
        }
    }
}

/// What the voice's render needs from the instrument beside the shared tables.
#[derive(Clone, Copy)]
struct Ctx {
    bend: f32,
    wheel: f32,
    at: f32,
    bend_norm: f32,
}

/// Where tap `t` reads: the position moved on by the tap spacing (a share of the sample), wrapped within the trimmed part.
fn tap_pos(pos: f32, t: usize, spacing: f32, ts: f32, te: f32) -> f32 {
    let p = pos + t as f32 * spacing;
    if t == 0 { p.clamp(ts, te) } else { ts + (p - ts).rem_euclid((te - ts).max(1e-4)) }
}

impl Shared {
    /// The handle of source `s` `beats` after the note started: the recorded motion (looping
    /// between its loop points) replaces it, the live handle offsets the motion.
    fn handle(&self, c: &SrcCfg, s: usize, beats: f32) -> (f32, f32) {
        let (mut hx, mut hy) = (c.x, c.y);
        let motion_at = |curve: usize| -> Option<f32> {
            if !self.motion_has[curve] {
                return None;
            }
            let (ls, le) = (self.motion_ls, self.motion_le);
            let t = if beats < le { beats } else { ls + (beats - ls) % (le - ls) };
            let f = (t / self.motion_dur * MOTION_STEPS as f32).clamp(0.0, MOTION_STEPS as f32 - 1.0);
            let i = f as usize;
            let j = (i + 1) % MOTION_STEPS;
            Some(lerp(self.motion[curve][i], self.motion[curve][j], f - i as f32))
        };
        if let Some(m) = motion_at(s * 2) {
            hx = (m + (c.x - self.motion[s * 2][0])).clamp(0.0, 1.0);
        }
        if let Some(m) = motion_at(s * 2 + 1) {
            hy = (m + (c.y - 0.5)).clamp(0.0, 1.0);
        }
        (hx, hy)
    }
}

/// A position moved by the Snap choice (index into `SNAPS`): transients, the beat grid of a
/// sample with a tempo, Auto picking the grid when there is a tempo and transients otherwise.
pub fn snap_to(x: f32, mode: usize, tempo: f32, duration: f32, transients: &[f32]) -> f32 {
    let by_transient = |x: f32| transients.iter().cloned().min_by(|a, b| (a - x).abs().partial_cmp(&(b - x).abs()).unwrap()).filter(|t| (t - x).abs() < 0.04).unwrap_or(x);
    let by_beat = |x: f32| {
        if tempo <= 0.0 || duration <= 0.0 {
            return x;
        }
        let b = 60.0 / tempo / duration;
        (x / b).round() * b
    };
    match mode {
        1 | 4 => x,
        2 => by_transient(x),
        3 => by_beat(x),
        _ => {
            if tempo > 0.0 {
                by_beat(x)
            } else {
                by_transient(x)
            }
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn env_at(a: &[f32; PARTIALS], idx: f32) -> f32 {
    if idx < 0.0 {
        return 0.0;
    }
    let p = idx - 1.0;
    if p < 0.0 {
        return a[0] * idx.min(1.0);
    }
    let i = p as usize;
    if i + 1 >= PARTIALS {
        return if i < PARTIALS { a[i] * (1.0 - (p - i as f32)) } else { 0.0 };
    }
    lerp(a[i], a[i + 1], p - i as f32)
}

fn band_at(a: &[f32; NB], p: f32) -> f32 {
    if !p.is_finite() || p < -1.0 || p > NB as f32 {
        return 0.0;
    }
    let p = p.clamp(0.0, NB as f32 - 1.001);
    let i = p as usize;
    lerp(a[i], a[i + 1], p - i as f32)
}

fn vowel_env(f: f32, x: f32) -> f32 {
    const V: [[f32; 3]; 4] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [350.0, 1700.0, 2700.0], [450.0, 800.0, 2830.0]];
    let p = x.clamp(0.0, 1.0) * 3.0;
    let i = (p as usize).min(2);
    let t = p - i as f32;
    let w = [1.0, 0.7, 0.4];
    let mut e = 0.02;
    for j in 0..3 {
        let fj = lerp(V[i][j], V[i + 1][j], t);
        let d = (f.max(20.0) / fj).ln() / 0.18;
        e += w[j] * (-0.5 * d * d).exp();
    }
    e
}

fn odd_even(k: usize, v: f32) -> f32 {
    if k % 2 == 1 {
        (2.0 * (1.0 - v)).clamp(0.0, 2.0)
    } else {
        (2.0 * v).clamp(0.0, 2.0)
    }
}

impl SrcVoice {
    /// One block of this source: its output (left, right) before the filter and amp envelope.
    #[allow(clippy::too_many_arguments)]
    fn synth(&mut self, sh: &Shared, c: &SrcCfg, k: [f32; 4], r: f32, pos: f32, n: usize, out: &mut [[f32; CH]; 2]) {
        let len = sh.sample.len();
        let dt = n as f32 / sh.rate;
        let rate = sh.rate;
        match c.mode {
            0 => {
                // granular
                let (ts, te) = sh.trim;
                let sync = c.fx == 1;
                let (size, dens, rpan, rtime, taps, spacing, ratio, interval) = if !sync {
                    let size = size_ms(k[0]) * 0.001 * rate;
                    let d = 1.0 + (k[1] * 9.0).round();
                    (size, d, k[2], k[3], 1usize, 0.0, r, size / d)
                } else {
                    // pitch-synchronous: a grain every output period, each two periods of the
                    // sample's own pitch long (shorter or longer by the formant ratio)
                    let f_out = (sh.an.f0_med.max(40.0) * r).clamp(30.0, 3000.0);
                    let per = rate / f_out;
                    let formant = semis(shift_semis(k[0], 36.0));
                    let taps = 1 + (k[1] * 7.0).round() as usize;
                    let own = 2.0 * rate / sh.an.f0_med.max(40.0) / formant;
                    (own.clamp(0.002 * rate, 0.1 * rate), 2.0, 0.0, k[3], taps, k[2], formant, per)
                };
                let gain = if sync { (2.0 * interval / size).min(1.5) / (taps as f32).sqrt() } else { 1.2 / dens.sqrt() };
                let period = sh.srate / sh.an.f0_med.max(40.0);
                let snap = |p: f32| if sync { (p / period).round() * period } else { p };
                let step = sh.srate / rate * ratio * if sh.reverse { -1.0 } else { 1.0 };
                if self.fresh && !self.ended {
                    // a new note starts with the grains already part way through, so the sound
                    // is at full level at once instead of fading in over the first grain
                    self.fresh = false;
                    let nd = dens.round().max(1.0) as usize;
                    for t in 0..taps.min(8) {
                        self.until[t] = interval.max(8.0) * 0.5;
                        for j in 0..nd {
                            let age = size * (0.5 + j as f32) / nd as f32;
                            let age = if nd == 1 { size * 0.5 } else { age.min(size * 0.95) };
                            if let Some(g) = self.grains.iter_mut().find(|g| !g.on) {
                                let a = if sync { 0.0 } else { self.rng.bipolar() * rpan };
                                let ang = (1.0 + a) * std::f32::consts::FRAC_PI_4;
                                let p = snap(tap_pos(pos, t, spacing, ts, te) * (len - 1) as f32) - step * age;
                                *g = Grain { on: true, pos: p.clamp(0.0, (len - 2) as f32), inc: step, age, len: size.max(16.0), gl: ang.cos() * gain, gr: ang.sin() * gain };
                            }
                        }
                    }
                }
                for i in 0..n {
                    for t in 0..taps.min(8) {
                        self.until[t] -= 1.0;
                        if self.until[t] <= 0.0 && !self.ended {
                            self.until[t] += interval.max(8.0);
                            if let Some(g) = self.grains.iter_mut().find(|g| !g.on) {
                                let rt = self.rng.bipolar() * rtime * 0.25;
                                let p = tap_pos(pos + rt, t, spacing, ts, te);
                                let a = if sync { 0.0 } else { self.rng.bipolar() * rpan };
                                let ang = (1.0 + a) * std::f32::consts::FRAC_PI_4;
                                *g = Grain { on: true, pos: snap(p * (len - 1) as f32), inc: step, age: 0.0, len: size.max(16.0), gl: ang.cos() * gain, gr: ang.sin() * gain };
                            } else {
                                self.until[t] += interval.max(8.0);
                            }
                        }
                    }
                    let (mut l, mut rr) = (0.0, 0.0);
                    for g in self.grains.iter_mut().filter(|g| g.on) {
                        let ph = g.age / g.len;
                        if ph >= 1.0 || g.pos < 0.0 || g.pos >= (len - 1) as f32 {
                            g.on = false;
                            continue;
                        }
                        let w = sin_cycles(ph * 0.5);
                        let w = w * w;
                        let ip = g.pos as usize;
                        let fr = g.pos - ip as f32;
                        let s = sh.sample[ip] + (sh.sample[ip + 1] - sh.sample[ip]) * fr;
                        l += s * w * g.gl;
                        rr += s * w * g.gr;
                        g.pos += g.inc;
                        g.age += 1.0;
                    }
                    out[0][i] = l;
                    out[1][i] = rr;
                }
            }
            1 => self.additive(sh, c, k, r, pos, n, out),
            _ => self.spectral(sh, c, k, r, pos, n, out),
        }
        let _ = dt;
    }

    fn frame(&self, sh: &Shared, pos: f32) -> (usize, usize, f32) {
        let ps = pos * (sh.sample.len().saturating_sub(1)) as f32 / sh.an.hop as f32;
        let i0 = (ps as usize).min(sh.an.frames.saturating_sub(1));
        let i1 = (i0 + 1).min(sh.an.frames.saturating_sub(1));
        (i0, i1, (ps - i0 as f32).clamp(0.0, 1.0))
    }

    #[allow(clippy::too_many_arguments)]
    fn additive(&mut self, sh: &Shared, c: &SrcCfg, k: [f32; 4], r: f32, pos: f32, n: usize, out: &mut [[f32; CH]; 2]) {
        let an = &sh.an;
        let (i0, i1, fr) = self.frame(sh, pos);
        let mut a = [0.0f32; PARTIALS];
        let mut dv = [1.0f32; PARTIALS];
        for p in 0..PARTIALS {
            a[p] = lerp(an.amp[i0 * PARTIALS + p], an.amp[i1 * PARTIALS + p], fr);
            dv[p] = lerp(an.dev[i0 * PARTIALS + p], an.dev[i1 * PARTIALS + p], fr);
        }
        let f0 = lerp(an.f0[i0], an.f0[i1], fr).max(20.0);
        let f0m = an.f0_med.max(20.0);
        let level = lerp(an.level[i0], an.level[i1], fr);
        let gate = if self.ended { 0.0 } else { 1.0 };
        let (mut pv, mut np, mut sym) = (1.0f32, PARTIALS, 0.0f32);
        let mut g = [1.0f32; PARTIALS];
        let mut amps = [0.0f32; NB];
        let mut incs = [0.0f32; NB];
        let mut formant: Option<(f32, f32, f32, f32)> = None; // shift ratio, key track, size, centre
        let mut vowel: Option<f32> = None;
        match c.fx {
            0 => {
                for p in 0..PARTIALS {
                    let kk = p + 1;
                    let mut x = odd_even(kk, k[0]);
                    if kk >= 3 && (kk % 3 == 0) && (kk / 3).is_power_of_two() {
                        x *= (2.0 * k[1]).min(2.0);
                    }
                    if kk.is_power_of_two() {
                        x *= (2.0 * k[2]).min(2.0);
                    }
                    if kk == 1 {
                        x *= if k[3] <= 0.5 { 2.0 * k[3] } else { 1.0 };
                    } else if k[3] > 0.5 {
                        x *= 2.0 * (1.0 - k[3]);
                    }
                    g[p] = x;
                }
            }
            1 | 2 => {
                let (sy, oe) = if c.fx == 1 { (k[0], k[1]) } else { (k[1], k[0]) };
                sym = (sy - 0.5) * 2.0 * 0.8;
                for p in 0..PARTIALS {
                    g[p] = odd_even(p + 1, oe);
                }
                pv = k[2];
                np = 1 + (k[3] * (PARTIALS as f32 - 1.0)).round() as usize;
            }
            3 => formant = Some((semis(shift_semis(k[0], 24.0)), k[1], size_ratio(k[2]), centre_hz(k[3]))),
            _ => {
                formant = Some((semis(shift_semis(k[0], 24.0)), 0.0, size_ratio(k[1]), centre_hz(k[2])));
                vowel = Some(k[3]);
                pv = 0.0;
            }
        }
        if sh.pitch_lock {
            pv = 0.0;
        }
        let mut cnt = 0;
        for p in 0..PARTIALS.min(np) {
            let kk = (p + 1) as f32;
            let dev = 1.0 + pv * (dv[p] * f0 / f0m - 1.0);
            let fo = kk * f0m * r * dev;
            let mut amp = a[p] * g[p];
            if let Some((sr, kt, sz, cen)) = formant {
                let fe = kk * f0m * r * r.powf(-kt) / sr.max(0.05);
                let f3 = cen + (fe - cen) * sz;
                let idx = f3 / f0;
                amp = match vowel {
                    Some(v) => level * 2.2 * vowel_env(f3, v) / kk.powf(0.6),
                    None => env_at(&a, idx),
                };
            }
            if fo >= rate_nyq(sh.rate) || fo <= 0.0 {
                amp = 0.0;
            }
            amps[p] = if amp.is_finite() { amp * gate } else { 0.0 };
            incs[p] = fo / sh.rate;
            cnt = p + 1;
        }
        self.bank.run(&amps, &incs, cnt, n, sym, &mut out[0]);
        let o0 = out[0];
        out[1][..n].copy_from_slice(&o0[..n]);
    }

    #[allow(clippy::too_many_arguments)]
    fn spectral(&mut self, sh: &Shared, c: &SrcCfg, k: [f32; 4], r: f32, pos: f32, n: usize, out: &mut [[f32; CH]; 2]) {
        let an = &sh.an;
        let (i0, i1, fr) = self.frame(sh, pos);
        let mut a = [0.0f32; NB];
        for b in 0..NB {
            a[b] = lerp(an.bands[i0 * NB + b], an.bands[i1 * NB + b], fr);
        }
        let dt = n as f32 / sh.rate;
        let mut p = a;
        let ratio = an.band_ratio().max(1.0001);
        let lr = ratio.ln();
        let lo = an.band_hz[0];
        let maxa = a.iter().cloned().fold(0.0, f32::max).max(1e-9);
        let mut noise_mix = [0.0f32; NB];
        match c.fx {
            0 => {
                let (sr, kt, sz, cen) = (semis(shift_semis(k[0], 24.0)), k[1], size_ratio(k[2]), centre_hz(k[3]));
                for b in 0..NB {
                    let fe = an.band_hz[b] * r.powf(1.0 - kt) / sr.max(0.05);
                    let f3 = cen + (fe - cen) * sz;
                    p[b] = if f3 > lo * 0.3 { band_at(&a, (f3 / lo).ln() / lr) } else { 0.0 };
                }
            }
            1 => {
                let (lc, hc) = (cutoff_hz(k[0]), cutoff_hz(k[1]));
                for b in 0..NB {
                    let f = an.band_hz[b];
                    let gl = ((f / lc).ln() / 0.35 + 0.5).clamp(0.0, 1.0);
                    let gh = ((hc / f).ln() / 0.35 + 0.5).clamp(0.0, 1.0);
                    p[b] = a[b] * gl * gh;
                }
            }
            2 => {
                let len = blur_len(k[1]);
                let thr = k[3] * k[3] * maxa * 0.5;
                for b in 0..NB {
                    let t = len * (1.0 + k[2] * (self.hash[b] - 0.5) * 1.8).max(0.05);
                    let tgt = a[b];
                    self.blur[b] = if tgt >= self.blur[b] { tgt } else { tgt + (self.blur[b] - tgt) * (-dt / t).exp() };
                    let blurred = if a[b] >= thr { self.blur[b] } else { 0.0 };
                    p[b] = lerp(a[b], blurred, k[0]);
                    if a[b] < thr {
                        p[b] = lerp(a[b], 0.0, if k[3] > 0.0 { 1.0 } else { 0.0 }) * (1.0 - k[0]);
                    }
                }
            }
            3 => {
                let cut = maxa * (1.0 - k[1]).powi(2) * 0.9;
                let att = 0.005 + k[2] * 2.0;
                let simp = k[3] * 2.5;
                for b in 0..NB {
                    let em = a[b] >= cut && a[b] > 1e-7;
                    let tgt = if em { 1.0 } else { 0.0 };
                    let tc = if em { att } else { 0.04 };
                    self.cloud[b] += (tgt - self.cloud[b]) * (1.0 - (-dt / tc).exp());
                    let shaped = a[b] * (a[b] / maxa).powf(simp);
                    let cloudy = shaped * (0.25 + 1.6 * self.cloud[b]);
                    p[b] = lerp(a[b], cloudy, k[0]);
                }
            }
            4 => {
                let bpo = std::f32::consts::LN_2 / lr;
                let sb = (k[1] * 24.0 / 12.0 * bpo).round() as usize;
                let fb = k[3] * 0.97;
                let dropf = k[2] * 0.3 * maxa;
                let mut cur = a;
                for _ in 0..4 {
                    let mut nx = a;
                    for b in sb..NB {
                        let v = cur[b - sb];
                        if v > dropf {
                            nx[b] += fb * v;
                        }
                    }
                    cur = nx;
                }
                for b in 0..NB {
                    p[b] = lerp(a[b], cur[b], k[0]);
                }
            }
            _ => {
                let (lc, hc) = (cutoff_hz(k[0]), cutoff_hz(k[1]));
                for b in 0..NB {
                    let f = an.band_hz[b];
                    let m = ((f / lc).ln() / 0.35 + 0.5).clamp(0.0, 1.0) * ((hc / f).ln() / 0.35 + 0.5).clamp(0.0, 1.0) * k[2];
                    noise_mix[b] = m;
                }
            }
        }
        let gate = if self.ended { 0.0 } else { 1.0 };
        let mut amps = [0.0f32; NB];
        let mut incs = [0.0f32; NB];
        for b in 0..NB {
            let fo = an.band_hz[b] * r;
            let mut amp = p[b];
            if c.fx == 5 {
                // a fresh random gain per block, smoothed to about the band's own width
                let bw = (an.band_hz[b] * (ratio - 1.0)).max(5.0);
                let kf = 1.0 - (-dt * bw * 3.0).exp();
                self.nz[b] += (self.rng.bipolar() - self.nz[b]) * kf;
                let sigma = 0.577 * (kf / (2.0 - kf)).sqrt();
                amp = a[b] * (1.0 - noise_mix[b]) + a[b] * self.nz[b] / sigma.max(0.02) * noise_mix[b];
            }
            if fo >= rate_nyq(sh.rate) || fo <= 0.0 {
                amp = 0.0;
            }
            amps[b] = if amp.is_finite() { amp * gate } else { 0.0 };
            incs[b] = fo / sh.rate;
        }
        self.bank.run(&amps, &incs, NB, n, 0.0, &mut out[0]);
        let o0 = out[0];
        out[1][..n].copy_from_slice(&o0[..n]);
    }
}

#[inline]
fn rate_nyq(rate: f32) -> f32 {
    rate * 0.46
}

// ---------------------------------------------------------------------------------------------
// The instrument

pub struct SampleAlchemy {
    pub rate: f32,
    p: Params,
    ch: Channel,
    sh: Shared,
    voices: Vec<Voice>,
    stack: Vec<(u8, u8, u8)>,
    last_pitch: f32,
    born: u64,
    chan_bend: [f32; 16],
    chan_press: [f32; 16],
    chan_wheel: [f32; 16],
    path: Option<String>,
    playing: bool,
}

const MAX_VOICES: usize = 16;

impl SampleAlchemy {
    pub fn new(rate: f32) -> SampleAlchemy {
        crate::dsp::warm_tables();
        let mut a = SampleAlchemy {
            rate,
            p: Params::new(table()),
            ch: Channel::new(rate),
            sh: Shared {
                rate,
                srate: rate,
                sample: Vec::new(),
                an: Analysis::default(),
                cfg: [SrcCfg::default(); 4],
                lfo: [(Shape::Sine, 1.0, 0); 4],
                routes: [(ROUTES as u8, 0, 0.0); ROUTES],
                motion: [[-1.0; MOTION_STEPS]; 8],
                motion_has: [false; 8],
                motion_dur: 4.0,
                motion_ls: 0.0,
                motion_le: 4.0,
                trim: (0.0, 1.0),
                reverse: false,
                pitch_lock: false,
                root: 60.0,
                root_cents: 0.0,
                tempo: 0.0,
                bpm: 120.0,
                ctrl: [0.0; 4],
                glide: 0.0,
            },
            voices: (0..MAX_VOICES).map(|i| Voice::new(rate, i)).collect(),
            stack: Vec::with_capacity(32),
            last_pitch: 60.0,
            born: 0,
            chan_bend: [0.0; 16],
            chan_press: [0.0; 16],
            chan_wheel: [0.0; 16],
            path: None,
            playing: false,
        };
        a.set_defaults();
        a
    }

    fn set_defaults(&mut self) {
        for i in 0..ROUTES {
            if let Some((t, s, d)) = default_route(i) {
                self.p.set(route_id(i, 0), t as f32);
                self.p.set(route_id(i, 1), s as f32);
                self.p.set(route_id(i, 2), d);
            }
        }
    }

    /// Loads the sample (mono) the instrument plays, recorded at `rate` Hz, and analyses it
    /// for the additive and spectral engines.
    pub fn set_sample(&mut self, mono: Vec<f32>, rate: f32) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
        self.sh.an = Analysis::new(&mono, rate.max(8000.0));
        self.sh.srate = rate.max(8000.0);
        self.sh.sample = mono;
        // room for the interpolation read of the last frame
        self.sh.sample.push(0.0);
    }

    /// The sample as loaded (mono).
    pub fn sample(&self) -> &[f32] {
        let n = self.sh.sample.len();
        &self.sh.sample[..n.saturating_sub(1)]
    }
    pub fn sample_rate(&self) -> f32 {
        self.sh.srate
    }
    pub fn has_sample(&self) -> bool {
        self.sh.sample.len() > 64
    }
    pub fn sample_path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    pub fn set_sample_path(&mut self, p: Option<String>) {
        self.path = p;
    }
    pub fn analysis(&self) -> &Analysis {
        &self.sh.an
    }
    /// Length of the sample in seconds.
    pub fn duration(&self) -> f32 {
        self.sample().len() as f32 / self.sh.srate
    }

    /// Sample positions of the transients (0…1).
    pub fn transients(&self) -> &[f32] {
        &self.sh.an.onsets
    }

    /// A position moved by the Snap choice (index into `SNAPS`).
    pub fn snap_position(&self, x: f32, mode: usize) -> f32 {
        snap_to(x, mode, self.p.get(TEMPO), self.duration(), self.transients())
    }

    /// The project tempo the instrument follows.
    pub fn bpm(&self) -> f32 {
        self.sh.bpm
    }

    /// Where the newest note is in its motion (beats since it started) and where each source's
    /// handle is then (None for a source that is off or without motion), for the moving display.
    pub fn motion_state(&mut self) -> Option<(f32, [Option<(f32, f32)>; 4])> {
        self.gcfg();
        let v = self.voices.iter().filter(|v| v.active).max_by_key(|v| v.born)?;
        let mut out = [None; 4];
        for s in 0..4 {
            let c = self.sh.cfg[s];
            if c.on && (self.sh.motion_has[s * 2] || self.sh.motion_has[s * 2 + 1]) {
                out[s] = Some(self.sh.handle(&c, s, v.beats));
            }
        }
        Some((v.beats, out))
    }

    /// The tempo that makes the length between the trim markers a whole number of beats (a
    /// power of two), nearest to 120 bpm.
    pub fn derive_tempo(&self) -> f32 {
        let d = (self.p.get(TRIM_END) - self.p.get(TRIM_START)).max(0.001) * self.duration();
        if d <= 0.0 {
            return 120.0;
        }
        let mut best = (f32::MAX, 120.0);
        for e in 0..8 {
            let beats = (1 << e) as f32;
            let t = 60.0 * beats / d;
            let dist = (t / 120.0).ln().abs();
            if dist < best.0 {
                best = (dist, t);
            }
        }
        best.1
    }

    /// Writes the source sample as a 24-bit WAV.
    pub fn export_wav(&self, path: &std::path::Path) -> std::io::Result<()> {
        let s = self.sample();
        crate::wav::write_wav_24(path, self.sh.srate as u32, s, s)
    }

    /// True when a motion has been recorded for a source.
    pub fn has_motion(&self) -> bool {
        (0..8).any(|c| (0..MOTION_STEPS).any(|k| self.p.get(MOTION_BASE + (c * MOTION_STEPS + k) as u32) >= 0.0))
    }

    /// Deletes every recorded motion.
    pub fn clear_motion(&mut self) {
        for c in 0..8 * MOTION_STEPS {
            self.p.set(MOTION_BASE + c as u32, -1.0);
        }
    }

    /// Pitch bend on a MIDI channel (-8192…8191), for MIDI Mono Mode and MPE.
    pub fn pitch_bend_ch(&mut self, ch: u8, v: i16) {
        let mono = self.p.get(MIDI_MONO).round() as usize;
        let base = if mono == 2 { 15 } else { 0 };
        if mono == 0 || (ch & 15) == base {
            // the common channel bends every voice by the Bend range
            self.pitch_bend(v);
        } else {
            self.chan_bend[(ch & 15) as usize] = v as f32 / 8192.0;
        }
    }
    pub fn pressure_ch(&mut self, ch: u8, v: u8) {
        self.chan_press[(ch & 15) as usize] = v as f32 / 127.0;
    }
    pub fn controller_ch(&mut self, ch: u8, cc: u8, v: u8) {
        if cc == 1 {
            self.chan_wheel[(ch & 15) as usize] = v as f32 / 127.0;
        } else {
            self.controller(cc, v);
        }
    }

    /// Voices sounding now.
    pub fn voices_active(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    fn gcfg(&mut self) {
        let sh = &mut self.sh;
        let p = &self.p;
        for s in 0..4 {
            let g = |off: u32| p.get(sid(s, off));
            let fs = if p.get(GLOBAL_FILTER) >= 0.5 { 4 } else { s };
            let f = |off: u32| p.get(sid(fs, off));
            let c = &mut sh.cfg[s];
            c.on = g(o::ON) >= 0.5;
            c.mode = (g(o::MODE).round() as usize).min(2);
            c.fx = (g(o::EFFECT).round() as usize).min(effects(c.mode).len() - 1);
            for i in 0..4 {
                c.k[i] = g(o::K1 + i as u32);
            }
            c.play = (g(o::PLAY).round() as usize).min(4);
            c.x = g(o::X);
            c.y = g(o::Y);
            c.loop_end = g(o::LOOP_END);
            c.speed = g(o::SPEED);
            c.speed_sync = g(o::SPEED_SYNC) >= 0.5;
            c.loop_speed = g(o::LOOP_SPEED);
            c.loop_sync = g(o::LOOP_SYNC) >= 0.5;
            c.jitter = g(o::JITTER);
            c.bow_rate = g(o::BOW_RATE);
            c.bow_sync = g(o::BOW_SYNC).round() as usize;
            c.arp_rate = g(o::ARP_RATE).round() as usize;
            c.pan = g(o::PAN);
            c.coarse = g(o::COARSE);
            c.fine = g(o::FINE);
            c.vol = g(o::VOL);
            c.f = FCfg {
                on: f(o::F_ON) >= 0.5,
                ty: (f(o::F_TYPE).round() as usize).min(5),
                var: (f(o::F_VAR).round() as usize).min(4),
                cut: f(o::F_CUT),
                res: f(o::F_RES),
                note: f(o::F_NOTE),
                fb: f(o::F_FB),
                freq: f(o::F_FREQ),
                mix: f(o::F_MIX),
                modd: f(o::F_MOD),
            };
            for i in 0..4 {
                c.amp[i] = g(o::AMP + i as u32);
                c.menv[i] = g(o::MODENV + i as u32);
            }
        }
        for l in 0..4 {
            let shape = [Shape::Sine, Shape::Triangle, Shape::SawDown, Shape::SawUp, Shape::Square, Shape::SampleHold, Shape::Lag][(p.get(lfo_id(l, 0)).round() as usize).min(6)];
            sh.lfo[l] = (shape, p.get(lfo_id(l, 1)), p.get(lfo_id(l, 2)).round() as usize);
        }
        for i in 0..ROUTES {
            sh.routes[i] = (p.get(route_id(i, 0)).round().clamp(0.0, ROUTES as f32) as u8, p.get(route_id(i, 1)).round() as u8, p.get(route_id(i, 2)) * if p.get(route_id(i, 3)) >= 0.5 { 1.0 } else { 0.0 });
        }
        sh.motion_dur = p.get(MOTION_DUR).max(1.0);
        sh.motion_ls = p.get(MOTION_LS).min(sh.motion_dur);
        sh.motion_le = p.get(MOTION_LE).clamp(sh.motion_ls + 0.01, sh.motion_dur.max(sh.motion_ls + 0.01));
        for c in 0..8 {
            let mut any = false;
            let mut last = -1.0;
            let mut first = -1.0;
            for k in 0..MOTION_STEPS {
                let v = p.get(MOTION_BASE + (c * MOTION_STEPS + k) as u32);
                if v >= 0.0 {
                    any = true;
                    last = v;
                    if first < 0.0 {
                        first = v;
                    }
                }
                sh.motion[c][k] = v;
            }
            // holes hold the last value (the first recorded value before the first one)
            if any {
                let mut hold = first;
                for k in 0..MOTION_STEPS {
                    if sh.motion[c][k] >= 0.0 {
                        hold = sh.motion[c][k];
                    } else {
                        sh.motion[c][k] = hold;
                    }
                }
            }
            let _ = last;
            sh.motion_has[c] = any;
        }
        let (a, b) = (p.get(TRIM_START), p.get(TRIM_END));
        sh.trim = (a.min(b - 0.001).max(0.0), b.max(a + 0.001).min(1.0));
        sh.reverse = p.get(REVERSE) >= 0.5;
        sh.pitch_lock = p.get(PITCH_LOCK) >= 0.5;
        sh.root = p.get(ROOT_KEY);
        sh.root_cents = p.get(ROOT_CENTS);
        sh.tempo = p.get(TEMPO);
        for i in 0..4 {
            let cc = p.get(CTRL_A + i as u32).round() as u8;
            let _ = cc;
        }
        sh.glide = p.get(GLIDE);
        self.ch.bend_up = p.get(BEND_UP);
        self.ch.bend_down = p.get(BEND_DOWN);
        self.ch.update_bend();
    }

    fn poly(&self) -> usize {
        (self.p.get(POLY).round() as usize).clamp(1, MAX_VOICES)
    }

    fn alloc(&mut self, poly: usize) -> usize {
        // a free voice within the polyphony, else steal: released voices first, oldest first
        let active = self.voices.iter().filter(|v| v.active).count();
        if active < poly
            && let Some(i) = self.voices.iter().position(|v| !v.active)
        {
            return i;
        }
        let mut best: Option<(usize, bool, u64)> = None;
        for (i, v) in self.voices.iter().enumerate().filter(|(_, v)| v.active) {
            let cand = (i, v.held, v.born);
            if best.is_none_or(|b| (cand.1, cand.2) < (b.1, b.2)) {
                best = Some(cand);
            }
        }
        best.map(|b| b.0).unwrap_or(0)
    }

    fn trigger(&mut self, i: usize, note: u8, vel: u8, ch: u8, from: Option<f32>) {
        self.gcfg();
        self.born += 1;
        let v = &mut self.voices[i];
        v.active = true;
        v.held = true;
        v.pedal = false;
        v.note = note;
        v.ch = ch;
        v.vel = vel as f32 / 127.0;
        v.target = note as f32;
        v.pitch = from.unwrap_or(note as f32);
        v.glide = from.is_some();
        v.born = self.born;
        v.beats = 0.0;
        for l in 0..4 {
            v.lfo[l].reset(0.0);
        }
        for s in 0..4 {
            let c = self.sh.cfg[s];
            v.src[s].start(&c);
        }
    }
}

impl Plugin for SampleAlchemy {
    fn params(&self) -> &[Param] {
        table()
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.p.set(id, v);
        if id == GLOBAL_FILTER || id == POLY {
            let poly = self.poly();
            let mut n = 0;
            for v in self.voices.iter_mut() {
                if v.active {
                    n += 1;
                    if n > poly {
                        for s in v.src.iter_mut() {
                            s.release();
                        }
                        v.held = false;
                    }
                }
            }
        }
    }
    fn reset(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
        self.stack.clear();
        self.ch.reset();
        self.chan_bend = [0.0; 16];
        self.chan_press = [0.0; 16];
        self.chan_wheel = [0.0; 16];
    }
    fn tail(&self) -> f32 {
        3.0
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        if let Some((s, off)) = path_of(id) {
            if s < 4 && (o::K1..o::K1 + 4).contains(&off) {
                let mode = (self.p.get(sid(s, o::MODE)).round() as usize).min(2);
                let fx = (self.p.get(sid(s, o::EFFECT)).round() as usize).min(effects(mode).len() - 1);
                return Some(knob_text(mode, fx, (off - o::K1) as usize, value));
            }
            return match off {
                o::SPEED | o::LOOP_SPEED => Some(format!("{:.2}x", value)),
                o::X | o::Y | o::LOOP_END => Some(format!("{:.1} %", value * 100.0)),
                o::VOL => Some(format!("{:.1} dB", 20.0 * (value / 0.8).max(1e-4).log10() * 2.0)),
                o::F_CUT => Some(format!("{:.0} Hz", cutoff_hz(value))),
                o::F_NOTE => Some(note_name(value)),
                o::F_FREQ => Some(format!("{:.0} Hz", 100.0 * (self.rate * 0.5 / 100.0).powf(value))),
                _ if (o::AMP..o::AMP + 8).contains(&off) => {
                    let i = (off - o::AMP) % 4;
                    if i == 2 {
                        Some(format!("{:.0} %", value * 100.0))
                    } else {
                        Some(format!("{:.0} ms", env_time(value, 10.0) * 1000.0))
                    }
                }
                _ => None,
            };
        }
        match id {
            ROOT_KEY | PREVIEW_KEY => Some(note_name(value)),
            TEMPO => Some(if value < 1.0 { "None".into() } else { format!("{:.1} bpm", value) }),
            POLY => Some(format!("{}", value.round() as i32)),
            _ => None,
        }
    }
    fn load_floats(&mut self, _floats: &[f32]) {}
}

fn lfo_rate(hz: f32, sync: usize, bpm: f32) -> f32 {
    if sync == 0 {
        hz
    } else {
        bars_hz(SYNC[(sync - 1).min(SYNC.len() - 1)].1, bpm)
    }
}

impl Voice {
    /// Modulation sources for source `s` (−1…1 or 0…1), indexed as `MOD_SOURCES`.
    #[inline]
    fn source(&self, i: usize, s: usize, ctx: &Ctx, sh: &Shared, wy: f32, lfov: &[f32; 4]) -> f32 {
        match i {
            1..=4 => lfov[i - 1],
            5 => self.src[s].amp_env.level,
            6 => self.src[s].mod_env.level,
            7 => (wy - 0.5) * 2.0,
            8 => ctx.wheel,
            9 => ctx.at,
            10 => ctx.bend_norm,
            11 => self.vel,
            12 => (self.note as f32 - 60.0) / 60.0,
            13..=16 => sh.ctrl[i - 13],
            _ => 0.0,
        }
    }

    fn chunk(&mut self, sh: &Shared, ctx: &Ctx, n: usize, out: &mut [[f32; CH]; 2]) {
        let dt = n as f32 / sh.rate;
        let bpm = sh.bpm.max(20.0);
        self.beats += dt * bpm / 60.0;
        // glide
        if (self.pitch - self.target).abs() > 1e-4 {
            if sh.glide > 0.001 && self.glide {
                let k = 1.0 - (-dt / (sh.glide / 3.0)).exp();
                self.pitch += (self.target - self.pitch) * k;
            } else {
                self.pitch = self.target;
            }
        }
        let mut lfov = [0.0f32; 4];
        for l in 0..4 {
            let (shape, hz, sync) = sh.lfo[l];
            let mut rng = self.src[0].rng;
            lfov[l] = self.lfo[l].step(shape, lfo_rate(hz, sync, bpm) * dt, &mut rng);
            self.src[0].rng = rng;
        }
        let mut alive = false;
        for s in 0..4 {
            let c = sh.cfg[s];
            if !c.on || sh.sample.len() < 64 {
                continue;
            }
            // the handle: recorded motion replaces it (the live handle offsets the motion)
            let (hx, hy) = sh.handle(&c, s, self.beats);
            // the mod matrix
            let mut mods = [0.0f32; TARGETS_PER_SRC];
            for &(t, src, depth) in sh.routes.iter() {
                if (t as usize) < ROUTES && src > 0 && depth != 0.0 && t as usize / TARGETS_PER_SRC == s {
                    mods[t as usize % TARGETS_PER_SRC] += depth * self.source(src as usize, s, ctx, sh, hy, &lfov);
                }
            }
            let mut k = c.k;
            for i in 0..4 {
                k[i] = (k[i] + mods[i]).clamp(0.0, 1.0);
            }
            let pan = (c.pan + mods[5] * 2.0).clamp(-1.0, 1.0);
            let coarse = c.coarse + mods[6] * 72.0;
            let fine = c.fine + mods[7] * 200.0;
            let vol = (c.vol + mods[8]).clamp(0.0, 1.0);
            let mut fcfg = c.f;
            fcfg.cut = (fcfg.cut + mods[9]).clamp(0.0, 1.0);
            fcfg.res = (fcfg.res + mods[10]).clamp(0.0, 1.0);
            fcfg.note = (fcfg.note + mods[11] * 127.0).clamp(0.0, 127.0);
            // pitch
            let p = self.pitch;
            let st = p - sh.root - sh.root_cents / 100.0 + coarse + fine / 100.0 + ctx.bend;
            let r = semis(st);
            // the source's position
            let (ts, te) = sh.trim;
            let dur = ((sh.sample.len() - 1) as f32 / sh.srate).max(0.01);
            let sv = &mut self.src[s];
            let rev = if sh.reverse { -1.0 } else { 1.0 };
            let sync_speed = |sync: bool, v: f32| if sync && sh.tempo > 0.0 { bpm / sh.tempo } else { v };
            if !sv.started {
                sv.pos = match c.play {
                    1 => hx.clamp(0.0, 1.0),
                    _ => hx,
                }
                .clamp(ts, te);
                sv.started = true;
            }
            match c.play {
                0 => {
                    let sp = sync_speed(c.speed_sync, c.speed + mods[4] * 8.0);
                    sv.pos += rev * sp * dt / dur;
                    let dirn = rev * sp.signum();
                    if sv.pos >= te && dirn > 0.0 || sv.pos <= ts && dirn < 0.0 {
                        sv.pos = sv.pos.clamp(ts, te);
                        sv.ended = true;
                    }
                    sv.pos = sv.pos.clamp(ts, te);
                }
                1 => {
                    let sp = sync_speed(c.loop_sync, c.loop_speed + mods[4] * 8.0);
                    let a = hx.clamp(ts, te);
                    let b = c.loop_end.clamp(ts, te);
                    let width = (b - a).abs();
                    if width < 1e-4 {
                        sv.pos = a;
                    } else {
                        let sign = (b - a).signum() * rev;
                        sv.pos += sign * sp * dt / dur;
                        let (lo, hi) = (a.min(b), a.max(b));
                        if sv.pos > hi {
                            sv.pos = lo + (sv.pos - hi) % width;
                        } else if sv.pos < lo {
                            sv.pos = hi - (lo - sv.pos) % width;
                        }
                        sv.pos = sv.pos.clamp(lo, hi);
                    }
                }
                2 => {
                    let jit = (c.jitter + mods[4]).clamp(0.0, 1.0);
                    sv.jit += (sv.rng.bipolar() - sv.jit) * 0.3;
                    let target = (hx + jit * 0.4 * sv.jit).clamp(ts, te);
                    sv.pos += (target - sv.pos) * (1.0 - (-dt / 0.03).exp());
                }
                3 => {
                    let rate_hz = if c.bow_sync == 0 { (c.bow_rate + mods[4] * 20.0).max(0.05) } else { bars_hz(SYNC[(c.bow_sync - 1).min(SYNC.len() - 1)].1, bpm) };
                    sv.bow_t += dt;
                    let half = 0.5 / rate_hz;
                    if sv.bow_t >= half {
                        sv.bow_t -= half;
                        sv.dir = -sv.dir;
                    }
                    sv.pos += sv.dir * rev * dt / dur;
                    if sv.pos >= te || sv.pos <= ts {
                        sv.pos = sv.pos.clamp(ts, te);
                        sv.dir = -sv.dir;
                    }
                }
                _ => {
                    let len = arp_beats(c.arp_rate);
                    sv.arp_t += dt * bpm / 60.0;
                    let first = sv.arp_step == 0 && sv.arp_t <= dt * bpm / 60.0 + 1e-6;
                    if sv.arp_t >= len || first {
                        if !first {
                            sv.arp_t -= len;
                            sv.arp_step += 1;
                        }
                        const PAT: [usize; 8] = [0, 2, 1, 3, 0, 4, 2, 5];
                        let a = hx.clamp(ts, te);
                        let sec = PAT[sv.arp_step as usize % 8] as f32 / 6.0;
                        sv.pos = (a + (te - a) * sec).clamp(ts, te);
                        sv.fade_in = true;
                        sv.ended = false;
                        for g in sv.grains.iter_mut() {
                            g.on = false;
                        }
                        sv.until = [0.0; 8];
                    } else {
                        sv.pos = (sv.pos + rev * dt / dur).clamp(ts, te);
                        if sv.pos >= te {
                            sv.ended = true;
                        }
                    }
                }
            }
            // the sound
            let mut buf = [[0.0f32; CH]; 2];
            let pos = sv.pos;
            sv.synth(sh, &c, k, r, pos, n, &mut buf);
            // filter module
            if c.f.on {
                for ch in 0..2 {
                    let run = sv.filt[ch].config(&fcfg, sh.rate);
                    let f = &mut sv.filt[ch];
                    for x in buf[ch].iter_mut().take(n) {
                        *x = f.run(*x, &run);
                    }
                }
            }
            // envelopes, pan and mixer level
            let a = c.amp;
            sv.amp_env.set(env_time(a[0], 10.0), env_time(a[1], 10.0), a[2], env_time(a[3], 10.0), sh.rate);
            let m = c.menv;
            sv.mod_env.set(env_time(m[0], 10.0), env_time(m[1], 10.0), m[2], env_time(m[3], 10.0), sh.rate);
            let ang = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let g = (vol / 0.8) * (vol / 0.8) * self.vel.max(0.05) * std::f32::consts::SQRT_2;
            let (gl, gr) = (ang.cos() * g, ang.sin() * g);
            let fade = if sv.fade_in {
                sv.fade_in = false;
                true
            } else {
                false
            };
            for i in 0..n {
                let e = sv.amp_env.next();
                sv.mod_env.next();
                let f = if fade { i as f32 / n as f32 } else { 1.0 };
                out[0][i] += buf[0][i] * e * gl * f;
                out[1][i] += buf[1][i] * e * gr * f;
            }
            if !sv.idle() {
                alive = true;
            }
        }
        if !alive {
            self.active = false;
        }
    }
}

impl Instrument for SampleAlchemy {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_on_ch(0, pitch, velocity)
    }
    fn note_on_ch(&mut self, channel: u8, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off_ch(channel, pitch);
        }
        let poly = self.poly();
        let mode = self.p.get(PLAY_MODE).round() as usize;
        let glide = self.p.get(GLIDE) > 0.001;
        let held_before = !self.stack.is_empty();
        self.stack.retain(|s| !(s.0 == pitch && s.2 == channel));
        self.stack.push((pitch, velocity, channel));
        let from = self.last_pitch;
        self.last_pitch = pitch as f32;
        if poly == 1 {
            // monophonic: voice 0 is the only one
            let alive = self.voices[0].active;
            let (retrigger, glide_on) = match mode {
                1 => (true, glide),
                0 => (!held_before || !alive, glide),
                _ => (!held_before || !alive, glide && (!held_before || !alive)),
            };
            for i in 1..MAX_VOICES {
                if self.voices[i].active {
                    for s in self.voices[i].src.iter_mut() {
                        s.release();
                    }
                    self.voices[i].held = false;
                }
            }
            if retrigger || !alive {
                let cur = if alive { self.voices[0].pitch } else { from };
                self.trigger(0, pitch, velocity, channel, if glide_on { Some(cur) } else { None });
            } else {
                let v = &mut self.voices[0];
                v.note = pitch;
                v.ch = channel;
                v.target = pitch as f32;
                v.glide = glide_on;
                v.vel = velocity as f32 / 127.0;
            }
        } else {
            let glide_on = match mode {
                2 => glide && held_before,
                _ => glide,
            };
            let i = self.alloc(poly);
            self.trigger(i, pitch, velocity, channel, if glide_on { Some(from) } else { None });
        }
    }
    fn note_off(&mut self, pitch: u8) {
        self.note_off_ch(0, pitch)
    }
    fn note_off_ch(&mut self, channel: u8, pitch: u8) {
        self.stack.retain(|s| !(s.0 == pitch && s.2 == channel));
        let poly = self.poly();
        let sustain = self.ch.ctl.sustain;
        if poly == 1 {
            let top = self.stack.last().copied();
            let v = &mut self.voices[0];
            if v.active && v.held && v.note == pitch {
                if let Some((n, _, ch)) = top {
                    v.note = n;
                    v.ch = ch;
                    v.target = n as f32;
                    v.glide = self.p.get(GLIDE) > 0.001;
                    self.last_pitch = n as f32;
                } else if sustain {
                    v.pedal = true;
                } else {
                    v.held = false;
                    for s in v.src.iter_mut() {
                        s.release();
                    }
                }
            }
        } else {
            for v in self.voices.iter_mut() {
                if v.active && v.held && v.note == pitch && v.ch == channel {
                    if sustain {
                        v.pedal = true;
                    } else {
                        v.held = false;
                        for s in v.src.iter_mut() {
                            s.release();
                        }
                    }
                }
            }
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        for i in 0..4 {
            if self.p.get(CTRL_A + i as u32).round() as u8 == cc {
                self.sh.ctrl[i] = value as f32 / 127.0;
            }
        }
        if let Some(down) = self.ch.cc(cc, value)
            && !down
        {
            for v in self.voices.iter_mut() {
                if v.active && v.pedal {
                    v.pedal = false;
                    v.held = false;
                    for s in v.src.iter_mut() {
                        s.release();
                    }
                }
            }
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
        self.chan_bend[match self.p.get(MIDI_MONO).round() as usize {
            2 => 15,
            _ => 0,
        }] = value as f32 / 8192.0;
    }
    fn pressure(&mut self, value: u8) {
        self.ch.ctl.aftertouch = value as f32 / 127.0;
    }
    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.sh.bpm = bpm as f32;
        }
    }
    fn set_transport(&mut self, t: Transport) {
        if t.bpm > 0.0 {
            self.sh.bpm = t.bpm as f32;
        }
        self.playing = t.playing;
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.gcfg();
        let n = left.len().min(right.len());
        let gain = 10f32.powf(self.p.get(VOLUME) / 20.0);
        let limit = self.p.get(LIMITER) >= 0.5;
        let mono = self.p.get(MIDI_MONO).round() as usize;
        let base = if mono == 2 { 15 } else { 0 };
        let mpe = self.p.get(MPE_RANGE);
        let (cg_l, cg_r) = self.ch.out_gains();
        let mut off = 0;
        while off < n {
            let m = (n - off).min(CH);
            let mut out = [[0.0f32; CH]; 2];
            for v in self.voices.iter_mut() {
                if !v.active {
                    continue;
                }
                let ch = (v.ch & 15) as usize;
                let (bend_norm, bend, wheel, at) = if mono != 0 && ch != base {
                    (self.chan_bend[ch], self.chan_bend[ch] * mpe + self.ch.ctl.bend, self.chan_wheel[ch].max(self.ch.ctl.mod_wheel), self.chan_press[ch])
                } else {
                    (self.ch.ctl.bend / self.ch.bend_up.max(1.0), self.ch.ctl.bend, self.ch.ctl.mod_wheel, self.ch.ctl.aftertouch)
                };
                let ctx = Ctx { bend, wheel, at, bend_norm };
                v.chunk(&self.sh, &ctx, m, &mut out);
            }
            for i in 0..m {
                let (mut l, mut r) = (out[0][i] * gain * cg_l * 0.7, out[1][i] * gain * cg_r * 0.7);
                if limit {
                    let pk = l.abs().max(r.abs());
                    if pk > 0.95 {
                        let g = 0.95 / pk;
                        l *= g;
                        r *= g;
                    }
                }
                left[off + i] += l;
                right[off + i] += r;
            }
            off += m;
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
