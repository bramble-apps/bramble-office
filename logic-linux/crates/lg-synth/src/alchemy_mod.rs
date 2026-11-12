//! Alchemy's modulation section (Studio's own engine, after the guide's description): up to 16
//! each of LFOs, AHDSR envelopes, MSEGs, step sequencers, envelope followers and ModMaps, the
//! MIDI (pitch bend, mod wheel, Ctrl A–D), Note Property and Perform sources, and the per-target
//! modulation racks: any knob can take up to ten modulators, each with on / off, depth (−100 to
//! 100 %), a ModMap and the target's Smooth; a rack entry's depth can itself be modulated by up to
//! five more sources (each with one further modulator).
//!
//! Everything is stored as parameters (saved with the setting and undoable). Racks are a pool of
//! routing entries: (target parameter id, source, on, depth, smooth, ModMap). A target id of
//! `DEPTH_TGT + n` modulates routing n's depth.
//!
//! Modulators set to Trigger On (or Voice On / Effects Off) run per voice and restart with each
//! note; Trigger Off ones run once for the instrument (LFOs free-running from the song start, the
//! envelopes on the first held note, the sequencer locked to the song position).

use lg_dsp::{Param, Unit};

pub const N: usize = 16;
pub const LFO: u32 = 2000;
pub const AHD: u32 = 2256;
pub const MSEG: u32 = 2512;
pub const MSEG_STRIDE: u32 = 96;
pub const MSEG_PTS: usize = 24;
pub const SEQ: u32 = 4096;
pub const SEQ_STRIDE: u32 = 544;
pub const SEQ_STEPS: usize = 128;
pub const ENVF: u32 = 12800;
pub const MMAP: u32 = 12928;
pub const MMAP_STRIDE: u32 = 40;
pub const MMAP_PTS: usize = 12;
pub const ROUTE: u32 = 13568;
pub const ROUTES: usize = 128;
/// Ctrl A–D's controller numbers (extended parameters); +4: Ctrl 71–78 Bipolar.
pub const CTRL_CC: u32 = 14600;
/// A routing whose target is DEPTH_TGT + n modulates routing n's depth.
pub const DEPTH_TGT: u32 = 30000;
/// Slots per target rack, and depth modulators per rack entry.
pub const PER_TARGET: usize = 10;
pub const PER_DEPTH: usize = 5;

// source kinds (code = kind × 64 + instance)
pub const K_LFO: u32 = 1;
pub const K_AHD: u32 = 2;
pub const K_MSEG: u32 = 3;
pub const K_SEQ: u32 = 4;
pub const K_ENVF: u32 = 5;
pub const K_MIDI: u32 = 6;
pub const K_NOTE: u32 = 7;
pub const K_PERF: u32 = 8;
pub const K_MMAP: u32 = 9; // only for files / panels (ModMaps aren't sources)

// LFO fields
pub const LF_EXISTS: u32 = 0;
pub const LF_SHAPE: u32 = 1;
pub const LF_RATE: u32 = 2;
pub const LF_SYNC: u32 = 3;
pub const LF_DIV: u32 = 4;
pub const LF_DELAY: u32 = 5;
pub const LF_ATTACK: u32 = 6;
pub const LF_PHASE: u32 = 7;
pub const LF_TRIG: u32 = 8;
pub const LF_BIP: u32 = 9;
// AHDSR fields
pub const AH_EXISTS: u32 = 0;
pub const AH_A: u32 = 1;
pub const AH_H: u32 = 2;
pub const AH_D: u32 = 3;
pub const AH_S: u32 = 4;
pub const AH_R: u32 = 5;
pub const AH_CA: u32 = 6;
pub const AH_CD: u32 = 7;
pub const AH_CR: u32 = 8;
pub const AH_TRIG: u32 = 9;
pub const AH_SYNC: u32 = 10;
// MSEG fields (+16 + point × 3: time, level, curve)
pub const MS_EXISTS: u32 = 0;
pub const MS_TRIG: u32 = 1;
pub const MS_SYNC: u32 = 2;
pub const MS_SNAPY: u32 = 3;
pub const MS_LOOP: u32 = 4;
pub const MS_LS: u32 = 5;
pub const MS_LE: u32 = 6;
pub const MS_EDIT: u32 = 7;
pub const MS_N: u32 = 8;
pub const MS_PT: u32 = 16;
// sequencer fields (+16 value, +144 length, +272 swing, +400 tie, per step)
pub const SQ_EXISTS: u32 = 0;
pub const SQ_TRIG: u32 = 1;
pub const SQ_RATE: u32 = 2;
pub const SQ_SNAP: u32 = 3;
pub const SQ_EDIT: u32 = 4;
pub const SQ_SWING: u32 = 5;
pub const SQ_ATTACK: u32 = 6;
pub const SQ_GATE: u32 = 7;
pub const SQ_RELEASE: u32 = 8;
pub const SQ_LEN: u32 = 9;
pub const SQ_VAL: u32 = 16;
pub const SQ_SLEN: u32 = 144;
pub const SQ_SW: u32 = 272;
pub const SQ_TIE: u32 = 400;
// envelope follower fields
pub const EF_EXISTS: u32 = 0;
pub const EF_SRC: u32 = 1;
pub const EF_ATTACK: u32 = 2;
pub const EF_RELEASE: u32 = 3;
pub const EF_SCALE: u32 = 4;
// ModMap fields (+4 + point × 3: x, y, curve)
pub const MM_EXISTS: u32 = 0;
pub const MM_SNAPX: u32 = 1;
pub const MM_SNAPY: u32 = 2;
pub const MM_N: u32 = 3;
pub const MM_PT: u32 = 4;
// routing fields
pub const RT_TARGET: u32 = 0;
pub const RT_SRC: u32 = 1;
pub const RT_ON: u32 = 2;
pub const RT_DEPTH: u32 = 3;
pub const RT_SMOOTH: u32 = 4;
pub const RT_MMAP: u32 = 5;

/// LFO shapes by category (Studio's own waveforms; the categories are Logic's).
pub const LFO_SHAPES: &[&str] = &[
    "Ramp Up", "Ramp Down", "Sine", "Square", "Triangle", "Random Glide", "Random Hold",
    "Sine + Octave", "Double Hump", "Pulse 25%", "Pulse 10%", "Saw Bounce", "Exp Decay",
    "Pattern 1", "Pattern 2", "Pattern 3", "Pattern 4",
    "Steps Up 4", "Steps Down 4", "Steps 8", "Zigzag 3",
    "Smooth Steps 4", "Smooth Steps 8", "Wave Walk",
    "UHF Sine x4", "UHF Saw x8", "UHF Square x16",
];
pub const LFO_CATEGORIES: &[(&str, usize, usize)] = &[
    ("Basic", 0, 7),
    ("Complex", 7, 13),
    ("Random Patterns", 13, 17),
    ("Serial-Angular", 17, 21),
    ("Serial-Smooth", 21, 24),
    ("Ultra High Frequency", 24, 27),
];
/// How many pattern copies an Ultra High Frequency shape holds (the rate multiple).
pub fn shape_multiple(s: usize) -> u32 {
    match s {
        24 => 4,
        25 => 8,
        26 => 16,
        _ => 1,
    }
}
pub const SYNC_DIVS: &[&str] = &[
    "8 Bars", "4 Bars", "2 Bars", "1 Bar", "1/2", "1/2T", "1/4.", "1/4", "1/4T", "1/8.", "1/8", "1/8T", "1/16.", "1/16", "1/16T", "1/32", "1/64",
];
pub const SYNC_BEATS: &[f32] = &[32.0, 16.0, 8.0, 4.0, 2.0, 4.0 / 3.0, 1.5, 1.0, 2.0 / 3.0, 0.75, 0.5, 1.0 / 3.0, 0.375, 0.25, 1.0 / 6.0, 0.125, 0.0625];
pub const TRIGGERS: &[&str] = &["Off", "On", "Voice On/Effects Off"];
pub const SNAPS: &[&str] = &["Off", "1/2", "1/3", "1/4", "1/5", "1/6", "1/8", "1/10", "1/12", "1/16", "1/24", "1/32"];
pub const SNAP_N: &[u32] = &[0, 2, 3, 4, 5, 6, 8, 10, 12, 16, 24, 32];
pub const SNAPS_X: &[&str] = &["Off", "1/2", "1/3", "1/4", "1/5", "1/6", "1/8", "1/10", "1/12", "1/16", "1/24", "1/32", "Key"];
pub const LOOP_MODES: &[&str] = &["None", "Continuous", "Sustain", "Forward/Back"];
pub const MSEG_EDIT: &[&str] = &["Normal", "Slide", "Stretch"];
pub const SEQ_RATES: &[&str] = &["4", "2", "1", "1/2", "1/3", "1/4", "1/6", "1/8", "1/16"];
pub const SEQ_RATE_BEATS: &[f32] = &[4.0, 2.0, 1.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125, 0.0625];
pub const SEQ_EDIT: &[&str] = &["Value", "Length", "Swing"];
pub const ENVF_SOURCES: &[&str] = &["Source A", "Source B", "Source C", "Source D", "Main Filter 1", "Main Filter 2", "Output Pre FX", "Output Post FX", "Side Chain"];
pub const MIDI_SOURCES: &[&str] = &["Pitchbend", "ModWheel", "Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D"];
pub const NOTE_SOURCES: &[&str] = &[
    "Velocity", "Key Follow", "Key Follow Fixed", "Aftertouch", "Speed", "Held", "Flip Flop", "Flip Flop 2", "Stepped A2", "Stepped A3", "Stepped A4", "Stepped 4", "Stepped 8", "Stepped 16", "Random 1", "Random 2", "Random 3", "Random 4", "Max", "Polyphony", "ArpMod 1", "ArpMod 2", "Playhead",
];
pub const PERF_SOURCES: &[&str] = &[
    "Control 1", "Control 2", "Control 3", "Control 4", "Control 5", "Control 6", "Control 7", "Control 8", "XYPad 1X", "XYPad 1Y", "XYPad 2X", "XYPad 2Y", "Mod Attack", "Mod Decay", "Mod Sustain", "Mod Release",
];
pub const CTRL_DEFAULT_CC: [u8; 4] = [2, 4, 74, 75];

pub fn lfo(i: usize, f: u32) -> u32 {
    if i < 7 {
        match f {
            LF_RATE => return 100 + i as u32,
            LF_DELAY => return 107 + i as u32,
            LF_ATTACK => return 114 + i as u32,
            LF_PHASE => return 121 + i as u32,
            _ => {}
        }
    }
    LFO + i as u32 * 16 + f
}
pub fn ahd(i: usize, f: u32) -> u32 {
    if i < 2 {
        match f {
            AH_A => return 128 + i as u32,
            AH_H => return 130 + i as u32,
            AH_D => return 132 + i as u32,
            AH_S => return 134 + i as u32,
            AH_R => return 136 + i as u32,
            _ => {}
        }
    }
    AHD + i as u32 * 16 + f
}
pub fn mseg(i: usize, f: u32) -> u32 {
    MSEG + i as u32 * MSEG_STRIDE + f
}
pub fn seq(i: usize, f: u32) -> u32 {
    SEQ + i as u32 * SEQ_STRIDE + f
}
pub fn envf(i: usize, f: u32) -> u32 {
    ENVF + i as u32 * 8 + f
}
pub fn mmap(i: usize, f: u32) -> u32 {
    MMAP + i as u32 * MMAP_STRIDE + f
}
pub fn route(n: usize, f: u32) -> u32 {
    ROUTE + n as u32 * 8 + f
}
/// A parameter id of a modulator instance by kind.
pub fn field(kind: u32, i: usize, f: u32) -> u32 {
    match kind {
        K_LFO => lfo(i, f),
        K_AHD => ahd(i, f),
        K_MSEG => mseg(i, f),
        K_SEQ => seq(i, f),
        K_ENVF => envf(i, f),
        _ => mmap(i, f),
    }
}
/// The fields a modulator kind has (Copy / Paste / Clear / Save).
pub fn fields(kind: u32) -> std::ops::Range<u32> {
    match kind {
        K_LFO => 0..10,
        K_AHD => 0..11,
        K_MSEG => 0..(MS_PT + MSEG_PTS as u32 * 3),
        K_SEQ => 0..(SQ_TIE + SEQ_STEPS as u32),
        K_ENVF => 0..5,
        _ => 0..(MM_PT + MMAP_PTS as u32 * 3),
    }
}
pub fn kind_name(kind: u32) -> &'static str {
    match kind {
        K_LFO => "LFO",
        K_AHD => "AHDSR",
        K_MSEG => "MSEG",
        K_SEQ => "Seq",
        K_ENVF => "Env Follower",
        K_MIDI => "MIDI",
        K_NOTE => "Note Property",
        K_PERF => "Perform",
        _ => "ModMap",
    }
}
/// The file extension a modulator kind's saved settings use.
pub fn file_ext(kind: u32) -> &'static str {
    match kind {
        K_LFO => "lfo",
        K_AHD => "ahd",
        K_MSEG => "mse",
        K_SEQ => "seq",
        K_ENVF => "ef",
        _ => "mma",
    }
}
pub fn code(kind: u32, i: usize) -> f32 {
    (kind * 64 + i as u32) as f32
}
pub fn decode(v: f32) -> (u32, usize) {
    let c = v.round().max(0.0) as u32;
    (c / 64, (c % 64) as usize)
}
/// A source's name as the rack shows it ("LFO 1 (Sin)", "Velocity", "Control 6").
pub fn source_name(c: f32, lfo_shape: impl Fn(usize) -> usize) -> String {
    let (k, i) = decode(c);
    match k {
        K_LFO => {
            let s = LFO_SHAPES.get(lfo_shape(i)).copied().unwrap_or("");
            format!("LFO {} ({})", i + 1, s.chars().take(3).collect::<String>())
        }
        K_AHD => format!("AHDSR {}", i + 1),
        K_MSEG => format!("MSEG {}", i + 1),
        K_SEQ => format!("Seq {}", i + 1),
        K_ENVF => format!("Env Follower {}", i + 1),
        K_MIDI => MIDI_SOURCES.get(i).copied().unwrap_or("").to_string(),
        K_NOTE => NOTE_SOURCES.get(i).copied().unwrap_or("").to_string(),
        K_PERF => PERF_SOURCES.get(i).copied().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The modulation section's parameters (LFO 1–7's rate / delay / attack / phase and AHDSR 1–2's
/// times are Logic's own ids in the main list).
pub fn params() -> Vec<Param> {
    let mut v = Vec::new();
    for i in 0..N {
        let n = i + 1;
        let ex = if i == 0 { 1.0 } else { 0.0 };
        v.push(Param::new(lfo(i, LF_EXISTS), leak(format!("LFO {n} Exists")), 0.0, 1.0, ex, Unit::OnOff).hide());
        v.push(Param::menu(lfo(i, LF_SHAPE), leak(format!("LFO {n} Shape")), LFO_SHAPES, 2.0));
        if i >= 7 {
            v.push(Param::new(lfo(i, LF_RATE), leak(format!("LFO {n} Rate")), 0.0, 220.0, 4.0, Unit::Hz));
            v.push(Param::new(lfo(i, LF_DELAY), leak(format!("LFO {n} Delay")), 0.0, 20.0, 0.0, Unit::Seconds));
            v.push(Param::new(lfo(i, LF_ATTACK), leak(format!("LFO {n} Attack")), 0.0, 20.0, 0.0, Unit::Seconds));
            v.push(Param::new(lfo(i, LF_PHASE), leak(format!("LFO {n} Phase")), 0.0, 1.0, 0.0, Unit::Ratio));
        }
        v.push(Param::new(lfo(i, LF_SYNC), leak(format!("LFO {n} Sync")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::menu(lfo(i, LF_DIV), leak(format!("LFO {n} Sync Rate")), SYNC_DIVS, 7.0));
        v.push(Param::menu(lfo(i, LF_TRIG), leak(format!("LFO {n} Trigger")), TRIGGERS, 1.0));
        v.push(Param::new(lfo(i, LF_BIP), leak(format!("LFO {n} Bipolar")), 0.0, 1.0, 1.0, Unit::OnOff));
    }
    for i in 0..N {
        let n = i + 1;
        let ex = if i == 0 { 1.0 } else { 0.0 };
        v.push(Param::new(ahd(i, AH_EXISTS), leak(format!("AHDSR {n} Exists")), 0.0, 1.0, ex, Unit::OnOff).hide());
        if i >= 2 {
            v.push(Param::new(ahd(i, AH_A), leak(format!("AHDSR {n} Attack")), 0.0, 20.0, 0.005, Unit::Seconds));
            v.push(Param::new(ahd(i, AH_H), leak(format!("AHDSR {n} Hold")), 0.0, 20.0, 0.0, Unit::Seconds));
            v.push(Param::new(ahd(i, AH_D), leak(format!("AHDSR {n} Decay")), 0.0, 20.0, 0.6, Unit::Seconds));
            v.push(Param::new(ahd(i, AH_S), leak(format!("AHDSR {n} Sustain")), 0.0, 1.0, 0.5, Unit::Ratio));
            v.push(Param::new(ahd(i, AH_R), leak(format!("AHDSR {n} Release")), 0.0, 20.0, 0.4, Unit::Seconds));
        }
        for (f, nm) in [(AH_CA, "Attack Curve"), (AH_CD, "Decay Curve"), (AH_CR, "Release Curve")] {
            v.push(Param::new(ahd(i, f), leak(format!("AHDSR {n} {nm}")), -1.0, 1.0, 0.0, Unit::None).hide());
        }
        v.push(Param::menu(ahd(i, AH_TRIG), leak(format!("AHDSR {n} Trigger")), TRIGGERS, 1.0));
        v.push(Param::new(ahd(i, AH_SYNC), leak(format!("AHDSR {n} Sync")), 0.0, 1.0, 0.0, Unit::OnOff));
    }
    const MSEG_DEF: [(f32, f32); 4] = [(0.0, 0.0), (0.1, 1.0), (0.5, 0.5), (1.0, 0.0)];
    for i in 0..N {
        let n = i + 1;
        let ex = if i == 0 { 1.0 } else { 0.0 };
        v.push(Param::new(mseg(i, MS_EXISTS), leak(format!("MSEG {n} Exists")), 0.0, 1.0, ex, Unit::OnOff).hide());
        v.push(Param::menu(mseg(i, MS_TRIG), leak(format!("MSEG {n} Trigger")), TRIGGERS, 1.0));
        v.push(Param::new(mseg(i, MS_SYNC), leak(format!("MSEG {n} Sync")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::menu(mseg(i, MS_SNAPY), leak(format!("MSEG {n} Snap Y")), SNAPS, 0.0).hide());
        v.push(Param::menu(mseg(i, MS_LOOP), leak(format!("MSEG {n} Loop Mode")), LOOP_MODES, 0.0));
        v.push(Param::new(mseg(i, MS_LS), leak(format!("MSEG {n} Loop Start")), 0.0, (MSEG_PTS - 1) as f32, 1.0, Unit::None).hide());
        v.push(Param::new(mseg(i, MS_LE), leak(format!("MSEG {n} Loop End")), 0.0, (MSEG_PTS - 1) as f32, 2.0, Unit::None).hide());
        v.push(Param::menu(mseg(i, MS_EDIT), leak(format!("MSEG {n} Edit Mode")), MSEG_EDIT, 0.0).hide());
        v.push(Param::new(mseg(i, MS_N), leak(format!("MSEG {n} Points")), 2.0, MSEG_PTS as f32, 4.0, Unit::None).hide());
        for k in 0..MSEG_PTS {
            let (t, l) = MSEG_DEF.get(k).copied().unwrap_or((1.0 + k as f32 * 0.25, 0.0));
            let b = MS_PT + k as u32 * 3;
            v.push(Param::new(mseg(i, b), leak(format!("MSEG {n} Point {} Time", k + 1)), 0.0, 64.0, t, Unit::Seconds).hide());
            v.push(Param::new(mseg(i, b + 1), leak(format!("MSEG {n} Point {} Level", k + 1)), 0.0, 1.0, l, Unit::Ratio).hide());
            v.push(Param::new(mseg(i, b + 2), leak(format!("MSEG {n} Point {} Curve", k + 1)), -1.0, 1.0, 0.0, Unit::None).hide());
        }
    }
    for i in 0..N {
        let n = i + 1;
        let ex = if i == 0 { 1.0 } else { 0.0 };
        v.push(Param::new(seq(i, SQ_EXISTS), leak(format!("Seq {n} Exists")), 0.0, 1.0, ex, Unit::OnOff).hide());
        v.push(Param::menu(seq(i, SQ_TRIG), leak(format!("Seq {n} Trigger")), TRIGGERS, 1.0));
        v.push(Param::menu(seq(i, SQ_RATE), leak(format!("Seq {n} Rate")), SEQ_RATES, 5.0));
        v.push(Param::menu(seq(i, SQ_SNAP), leak(format!("Seq {n} Value Snap")), SNAPS, 0.0).hide());
        v.push(Param::menu(seq(i, SQ_EDIT), leak(format!("Seq {n} Edit Mode")), SEQ_EDIT, 0.0).hide());
        v.push(Param::new(seq(i, SQ_SWING), leak(format!("Seq {n} Swing")), 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(seq(i, SQ_ATTACK), leak(format!("Seq {n} Attack")), 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(seq(i, SQ_GATE), leak(format!("Seq {n} Gate")), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(seq(i, SQ_RELEASE), leak(format!("Seq {n} Release")), 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(seq(i, SQ_LEN), leak(format!("Seq {n} Steps")), 1.0, SEQ_STEPS as f32, 16.0, Unit::None).hide());
        for k in 0..SEQ_STEPS {
            let val = 1.0 - (k % 4) as f32 * 0.25;
            v.push(Param::new(seq(i, SQ_VAL + k as u32), leak(format!("Seq {n} Step {} Value", k + 1)), 0.0, 1.0, val, Unit::Ratio).hide());
            v.push(Param::new(seq(i, SQ_SLEN + k as u32), leak(format!("Seq {n} Step {} Length", k + 1)), 0.0, 1.0, 1.0, Unit::Ratio).hide());
            v.push(Param::new(seq(i, SQ_SW + k as u32), leak(format!("Seq {n} Step {} Swing", k + 1)), 0.0, 2.0, 1.0, Unit::None).hide());
            v.push(Param::new(seq(i, SQ_TIE + k as u32), leak(format!("Seq {n} Step {} Tie", k + 1)), 0.0, 1.0, 0.0, Unit::OnOff).hide());
        }
    }
    for i in 0..N {
        let n = i + 1;
        let ex = if i == 0 { 1.0 } else { 0.0 };
        v.push(Param::new(envf(i, EF_EXISTS), leak(format!("Env Follower {n} Exists")), 0.0, 1.0, ex, Unit::OnOff).hide());
        v.push(Param::menu(envf(i, EF_SRC), leak(format!("Env Follower {n} Source")), ENVF_SOURCES, 7.0));
        v.push(Param::new(envf(i, EF_ATTACK), leak(format!("Env Follower {n} Attack")), 0.0, 2.0, 0.01, Unit::Seconds));
        v.push(Param::new(envf(i, EF_RELEASE), leak(format!("Env Follower {n} Release")), 0.0, 4.0, 0.2, Unit::Seconds));
        v.push(Param::new(envf(i, EF_SCALE), leak(format!("Env Follower {n} Scale")), 0.0, 4.0, 1.0, Unit::None));
    }
    for i in 0..N {
        let n = i + 1;
        v.push(Param::new(mmap(i, MM_EXISTS), leak(format!("ModMap {n} Exists")), 0.0, 1.0, 0.0, Unit::OnOff).hide());
        v.push(Param::menu(mmap(i, MM_SNAPX), leak(format!("ModMap {n} Snap X")), SNAPS_X, 0.0).hide());
        v.push(Param::menu(mmap(i, MM_SNAPY), leak(format!("ModMap {n} Snap Y")), SNAPS, 0.0).hide());
        v.push(Param::new(mmap(i, MM_N), leak(format!("ModMap {n} Points")), 2.0, MMAP_PTS as f32, 2.0, Unit::None).hide());
        for k in 0..MMAP_PTS {
            let b = MM_PT + k as u32 * 3;
            let d = if k == 0 { 0.0 } else { 1.0 };
            v.push(Param::new(mmap(i, b), leak(format!("ModMap {n} Point {} X", k + 1)), 0.0, 1.0, d, Unit::Ratio).hide());
            v.push(Param::new(mmap(i, b + 1), leak(format!("ModMap {n} Point {} Y", k + 1)), 0.0, 1.0, d, Unit::Ratio).hide());
            v.push(Param::new(mmap(i, b + 2), leak(format!("ModMap {n} Point {} Curve", k + 1)), -1.0, 1.0, 0.0, Unit::None).hide());
        }
    }
    for r in 0..ROUTES {
        let n = r + 1;
        v.push(Param::new(route(r, RT_TARGET), leak(format!("Route {n} Target")), 0.0, 40000.0, 0.0, Unit::None).hide());
        v.push(Param::new(route(r, RT_SRC), leak(format!("Route {n} Source")), 0.0, 1023.0, 0.0, Unit::None).hide());
        v.push(Param::new(route(r, RT_ON), leak(format!("Route {n} On")), 0.0, 1.0, 1.0, Unit::OnOff).hide());
        v.push(Param::new(route(r, RT_DEPTH), leak(format!("Route {n} Depth")), -1.0, 1.0, 0.0, Unit::None));
        v.push(Param::new(route(r, RT_SMOOTH), leak(format!("Route {n} Smooth")), 0.0, 1.0, 0.0, Unit::Ratio).hide());
        v.push(Param::new(route(r, RT_MMAP), leak(format!("Route {n} ModMap")), 0.0, 16.0, 0.0, Unit::None).hide());
    }
    for (k, nm) in ["Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D"].iter().enumerate() {
        v.push(Param::new(CTRL_CC + k as u32, nm, 0.0, 127.0, CTRL_DEFAULT_CC[k] as f32, Unit::None));
    }
    v.push(Param::new(CTRL_CC + 4, "Ctrl 71-78 Bipolar", 0.0, 1.0, 0.0, Unit::OnOff));
    v
}

// ---------------------------------------------------------------- curves and shapes

/// A segment from a to b at x (0..1); curve > 0 bulges it upward (convex), < 0 downward.
pub fn seg(a: f32, b: f32, x: f32, c: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let k = 1.0 + 4.0 * c.abs();
    let up = |x: f32| if c >= 0.0 { 1.0 - (1.0 - x).powf(k) } else { x.powf(k) };
    let down = |x: f32| if c >= 0.0 { x.powf(k) } else { 1.0 - (1.0 - x).powf(k) };
    if b >= a { a + (b - a) * up(x) } else { a + (b - a) * down(x) }
}

fn hash01(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// An LFO shape's value (0..1) at phase p; `rnd` = (previous, next) random values of this cycle.
pub fn shape_value(s: usize, p: f32, rnd: (f32, f32)) -> f32 {
    use std::f32::consts::TAU;
    let p = p.rem_euclid(1.0);
    let steps = |n: usize, f: &dyn Fn(usize) -> f32| f(((p * n as f32) as usize).min(n - 1));
    let smooth = |n: usize, f: &dyn Fn(usize) -> f32| {
        let x = p * n as f32;
        let k = (x as usize).min(n - 1);
        let t = x - k as f32;
        let w = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
        f(k) + (f((k + 1) % n) - f(k)) * w
    };
    match s {
        0 => p,
        1 => 1.0 - p,
        2 => 0.5 + 0.5 * (TAU * p).sin(),
        3 => (p < 0.5) as u8 as f32,
        4 => if p < 0.5 { 2.0 * p } else { 2.0 - 2.0 * p },
        5 => rnd.0 + (rnd.1 - rnd.0) * (p * p * (3.0 - 2.0 * p)),
        6 => rnd.0,
        7 => 0.5 + 0.25 * ((TAU * p).sin() + (2.0 * TAU * p).sin()),
        8 => (TAU * p).sin().abs(),
        9 => (p < 0.25) as u8 as f32,
        10 => (p < 0.1) as u8 as f32,
        11 => (p * 2.0).fract() * if p < 0.5 { 1.0 } else { 0.5 },
        12 => (-5.0 * p).exp(),
        13..=16 => steps(8, &|k| hash01((s as u32) * 131 + k as u32)),
        17 => steps(4, &|k| k as f32 / 3.0),
        18 => steps(4, &|k| 1.0 - k as f32 / 3.0),
        19 => steps(8, &|k| [0.0, 4.0, 2.0, 6.0, 1.0, 5.0, 3.0, 7.0][k] / 7.0),
        20 => {
            let x = (p * 3.0).fract();
            if x < 0.5 { 2.0 * x } else { 2.0 - 2.0 * x }
        }
        21 => smooth(4, &|k| [0.0, 0.66, 0.33, 1.0][k]),
        22 => smooth(8, &|k| hash01(977 + k as u32)),
        23 => 0.5 + 0.3 * (TAU * p).sin() + 0.2 * (3.0 * TAU * p).sin(),
        24 => 0.5 + 0.5 * (TAU * 4.0 * p).sin(),
        25 => (8.0 * p).fract(),
        26 => ((16.0 * p).fract() < 0.5) as u8 as f32,
        _ => 0.5,
    }
}

// ---------------------------------------------------------------- settings read from params

#[derive(Clone, Copy, Default)]
pub struct LfoCfg {
    pub shape: u8,
    pub hz: f32,
    pub sync: bool,
    pub beats: f32,
    pub delay: f32,
    pub attack: f32,
    pub phase: f32,
    pub trigger: u8,
    pub bipolar: bool,
}
#[derive(Clone, Copy, Default)]
pub struct AhdCfg {
    pub a: f32,
    pub h: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
    pub ca: f32,
    pub cd: f32,
    pub cr: f32,
    pub trigger: u8,
    pub sync: bool,
}
#[derive(Clone, Copy)]
pub struct MsegCfg {
    pub trigger: u8,
    pub sync: bool,
    pub loop_mode: u8,
    pub ls: usize,
    pub le: usize,
    pub n: usize,
    pub pts: [(f32, f32, f32); MSEG_PTS],
}
impl Default for MsegCfg {
    fn default() -> Self {
        MsegCfg { trigger: 1, sync: false, loop_mode: 0, ls: 1, le: 2, n: 2, pts: [(0.0, 0.0, 0.0); MSEG_PTS] }
    }
}
#[derive(Clone, Copy)]
pub struct SeqCfg {
    pub trigger: u8,
    pub step_beats: f32,
    pub swing: f32,
    pub attack: f32,
    pub gate: f32,
    pub release: f32,
    pub len: usize,
    pub val: [f32; SEQ_STEPS],
    pub slen: [f32; SEQ_STEPS],
    pub sw: [f32; SEQ_STEPS],
    pub tie: [bool; SEQ_STEPS],
}
impl Default for SeqCfg {
    fn default() -> Self {
        SeqCfg { trigger: 1, step_beats: 0.25, swing: 0.0, attack: 0.0, gate: 1.0, release: 0.0, len: 16, val: [0.0; SEQ_STEPS], slen: [1.0; SEQ_STEPS], sw: [1.0; SEQ_STEPS], tie: [false; SEQ_STEPS] }
    }
}
#[derive(Clone, Copy, Default)]
pub struct EnvfCfg {
    pub src: u8,
    pub attack: f32,
    pub release: f32,
    pub scale: f32,
}
#[derive(Clone, Copy)]
pub struct MmapCfg {
    pub on: bool,
    pub n: usize,
    pub pts: [(f32, f32, f32); MMAP_PTS],
}
impl Default for MmapCfg {
    fn default() -> Self {
        MmapCfg { on: false, n: 2, pts: [(0.0, 0.0, 0.0); MMAP_PTS] }
    }
}
impl MmapCfg {
    /// The map's output for x (0..1).
    pub fn map(&self, x: f32) -> f32 {
        if !self.on || self.n < 2 {
            return x;
        }
        let x = x.clamp(0.0, 1.0);
        let pts = &self.pts[..self.n];
        if x <= pts[0].0 {
            return pts[0].1;
        }
        for w in pts.windows(2) {
            if x <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return seg(w[0].1, w[1].1, (x - w[0].0) / span, w[0].2);
            }
        }
        pts[self.n - 1].1
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Route {
    pub n: usize,
    pub target: u32,
    pub kind: u32,
    pub idx: usize,
    pub depth: f32,
    pub smooth: f32,
    pub mmap: usize,
}

/// Is the parameter one whose modulation scales it (volumes) rather than adds to it?
pub fn is_volume(id: u32) -> bool {
    matches!(id, 19..=22 | 94 | 204..=207)
}

/// Every modulator's settings and the routings in use.
pub struct ModCfg {
    pub lfo: [LfoCfg; N],
    pub ahd: [AhdCfg; N],
    pub mseg: Box<[MsegCfg; N]>,
    pub seq: Box<[SeqCfg; N]>,
    pub envf: [EnvfCfg; N],
    pub mmap: Box<[MmapCfg; N]>,
    pub routes: [Route; ROUTES],
    pub nroutes: usize,
    pub ctrl_cc: [u8; 4],
    pub ctrl_bipolar: bool,
}

impl Default for ModCfg {
    fn default() -> Self {
        ModCfg {
            lfo: [LfoCfg::default(); N],
            ahd: [AhdCfg::default(); N],
            mseg: Box::new([MsegCfg::default(); N]),
            seq: Box::new([SeqCfg::default(); N]),
            envf: [EnvfCfg::default(); N],
            mmap: Box::new([MmapCfg::default(); N]),
            routes: [Route::default(); ROUTES],
            nroutes: 0,
            ctrl_cc: CTRL_DEFAULT_CC,
            ctrl_bipolar: false,
        }
    }
}

impl ModCfg {
    pub fn read_lfo(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        let div = (p(lfo(i, LF_DIV)).round().max(0.0) as usize).min(SYNC_BEATS.len() - 1);
        self.lfo[i] = LfoCfg {
            shape: p(lfo(i, LF_SHAPE)).round().max(0.0) as u8,
            hz: p(lfo(i, LF_RATE)),
            sync: p(lfo(i, LF_SYNC)) >= 0.5,
            beats: SYNC_BEATS[div],
            delay: p(lfo(i, LF_DELAY)),
            attack: p(lfo(i, LF_ATTACK)),
            phase: p(lfo(i, LF_PHASE)),
            trigger: p(lfo(i, LF_TRIG)).round() as u8,
            bipolar: p(lfo(i, LF_BIP)) >= 0.5,
        };
    }
    pub fn read_ahd(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        self.ahd[i] = AhdCfg {
            a: p(ahd(i, AH_A)),
            h: p(ahd(i, AH_H)),
            d: p(ahd(i, AH_D)),
            s: p(ahd(i, AH_S)),
            r: p(ahd(i, AH_R)),
            ca: p(ahd(i, AH_CA)),
            cd: p(ahd(i, AH_CD)),
            cr: p(ahd(i, AH_CR)),
            trigger: p(ahd(i, AH_TRIG)).round() as u8,
            sync: p(ahd(i, AH_SYNC)) >= 0.5,
        };
    }
    pub fn read_mseg(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        let m = &mut self.mseg[i];
        m.trigger = p(mseg(i, MS_TRIG)).round() as u8;
        m.sync = p(mseg(i, MS_SYNC)) >= 0.5;
        m.loop_mode = p(mseg(i, MS_LOOP)).round() as u8;
        m.n = (p(mseg(i, MS_N)).round() as usize).clamp(2, MSEG_PTS);
        m.ls = (p(mseg(i, MS_LS)).round() as usize).min(m.n - 1);
        m.le = (p(mseg(i, MS_LE)).round() as usize).clamp(m.ls, m.n - 1);
        let mut last = 0.0f32;
        for k in 0..MSEG_PTS {
            let b = MS_PT + k as u32 * 3;
            let t = if k == 0 { 0.0 } else { p(mseg(i, b)).max(last) };
            last = t;
            m.pts[k] = (t, p(mseg(i, b + 1)), p(mseg(i, b + 2)));
        }
    }
    pub fn read_seq(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        let s = &mut self.seq[i];
        s.trigger = p(seq(i, SQ_TRIG)).round() as u8;
        s.step_beats = SEQ_RATE_BEATS[(p(seq(i, SQ_RATE)).round().max(0.0) as usize).min(SEQ_RATE_BEATS.len() - 1)];
        s.swing = p(seq(i, SQ_SWING));
        s.attack = p(seq(i, SQ_ATTACK));
        s.gate = p(seq(i, SQ_GATE));
        s.release = p(seq(i, SQ_RELEASE));
        s.len = (p(seq(i, SQ_LEN)).round() as usize).clamp(1, SEQ_STEPS);
        for k in 0..SEQ_STEPS {
            s.val[k] = p(seq(i, SQ_VAL + k as u32));
            s.slen[k] = p(seq(i, SQ_SLEN + k as u32));
            s.sw[k] = p(seq(i, SQ_SW + k as u32));
            s.tie[k] = p(seq(i, SQ_TIE + k as u32)) >= 0.5;
        }
    }
    pub fn read_envf(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        self.envf[i] = EnvfCfg { src: p(envf(i, EF_SRC)).round() as u8, attack: p(envf(i, EF_ATTACK)), release: p(envf(i, EF_RELEASE)), scale: p(envf(i, EF_SCALE)) };
    }
    pub fn read_mmap(&mut self, i: usize, p: &dyn Fn(u32) -> f32) {
        let m = &mut self.mmap[i];
        m.on = p(mmap(i, MM_EXISTS)) >= 0.5;
        m.n = (p(mmap(i, MM_N)).round() as usize).clamp(2, MMAP_PTS);
        let mut last = 0.0f32;
        for k in 0..MMAP_PTS {
            let b = MM_PT + k as u32 * 3;
            let x = if k == 0 { 0.0 } else { p(mmap(i, b)).max(last) };
            last = x;
            m.pts[k] = (x, p(mmap(i, b + 1)), p(mmap(i, b + 2)));
        }
        if m.n >= 2 {
            m.pts[m.n - 1].0 = 1.0;
        }
    }
    pub fn read_routes(&mut self, p: &dyn Fn(u32) -> f32) {
        self.nroutes = 0;
        for r in 0..ROUTES {
            let target = p(route(r, RT_TARGET)).round() as u32;
            let (kind, idx) = decode(p(route(r, RT_SRC)));
            if target == 0 || kind == 0 || p(route(r, RT_ON)) < 0.5 {
                continue;
            }
            self.routes[self.nroutes] = Route { n: r, target, kind, idx, depth: p(route(r, RT_DEPTH)), smooth: p(route(r, RT_SMOOTH)), mmap: p(route(r, RT_MMAP)).round() as usize };
            self.nroutes += 1;
        }
        for k in 0..4 {
            self.ctrl_cc[k] = p(CTRL_CC + k as u32).round().clamp(0.0, 127.0) as u8;
        }
        self.ctrl_bipolar = p(CTRL_CC + 4) >= 0.5;
    }
    pub fn read_all(&mut self, p: &dyn Fn(u32) -> f32) {
        for i in 0..N {
            self.read_lfo(i, p);
            self.read_ahd(i, p);
            self.read_mseg(i, p);
            self.read_seq(i, p);
            self.read_envf(i, p);
            self.read_mmap(i, p);
        }
        self.read_routes(p);
    }
    /// Re-reads only what a changed parameter belongs to; false when it isn't a modulation one.
    pub fn read_changed(&mut self, id: u32, p: &dyn Fn(u32) -> f32) -> bool {
        match id {
            100..=127 => self.read_lfo(((id - 100) % 7) as usize, p),
            128..=137 => self.read_ahd(((id - 128) % 2) as usize, p),
            LFO..AHD => self.read_lfo(((id - LFO) / 16) as usize, p),
            AHD..MSEG => self.read_ahd(((id - AHD) / 16) as usize, p),
            MSEG..SEQ => self.read_mseg(((id - MSEG) / MSEG_STRIDE) as usize, p),
            SEQ..ENVF => self.read_seq(((id - SEQ) / SEQ_STRIDE) as usize, p),
            ENVF..MMAP => self.read_envf(((id - ENVF) / 8) as usize, p),
            MMAP..ROUTE => self.read_mmap(((id - MMAP) / MMAP_STRIDE) as usize, p),
            ROUTE..=14604 => self.read_routes(p),
            _ => return false,
        }
        true
    }
    /// Does this source run per voice (restarting with each note)?
    pub fn per_voice(&self, kind: u32, i: usize) -> bool {
        match kind {
            K_LFO => self.lfo[i].trigger != 0,
            K_AHD => self.ahd[i].trigger != 0,
            K_MSEG => self.mseg[i].trigger != 0,
            K_SEQ => self.seq[i].trigger != 0,
            K_NOTE => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------- running modulators

/// An AHDSR / MSEG run.
#[derive(Clone, Copy, Default)]
pub struct EnvRun {
    pub t: f32,
    pub gate: bool,
    pub active: bool,
    /// Level when the gate closed and the time since.
    pub rel_from: f32,
    pub rel_t: f32,
    pub level: f32,
    pub dir: f32,
}

impl EnvRun {
    pub fn start(&mut self, from_zero: bool) {
        self.t = 0.0;
        self.gate = true;
        self.active = true;
        self.dir = 1.0;
        if from_zero {
            self.level = 0.0;
        }
    }
    pub fn release(&mut self) {
        if self.gate {
            self.gate = false;
            self.rel_from = self.level;
            self.rel_t = 0.0;
        }
    }
}

/// One AHDSR step: advance by dt (seconds, or beats when synced), return the level.
pub fn ahd_step(c: &AhdCfg, e: &mut EnvRun, dt: f32) -> f32 {
    if !e.active {
        return 0.0;
    }
    let lv = if e.gate {
        e.t += dt;
        let t = e.t;
        if t < c.a {
            seg(0.0, 1.0, t / c.a.max(1e-5), c.ca)
        } else if t < c.a + c.h {
            1.0
        } else if t < c.a + c.h + c.d {
            seg(1.0, c.s, (t - c.a - c.h) / c.d.max(1e-5), c.cd)
        } else {
            c.s
        }
    } else {
        e.rel_t += dt;
        if e.rel_t >= c.r {
            e.active = false;
            0.0
        } else {
            seg(e.rel_from, 0.0, e.rel_t / c.r.max(1e-5), c.cr)
        }
    };
    e.level = lv;
    lv
}

/// The MSEG's level at t (no looping).
pub fn mseg_at(c: &MsegCfg, t: f32) -> f32 {
    let pts = &c.pts[..c.n];
    if t <= 0.0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        if t <= w[1].0 {
            let span = (w[1].0 - w[0].0).max(1e-6);
            return seg(w[0].1, w[1].1, (t - w[0].0) / span, w[0].2);
        }
    }
    pts[c.n - 1].1
}

/// One MSEG step with its loop mode: advance by dt, return the level.
pub fn mseg_step(c: &MsegCfg, e: &mut EnvRun, dt: f32) -> f32 {
    if !e.active {
        return e.level;
    }
    let (ls, le) = (c.pts[c.ls].0, c.pts[c.le].0);
    let span = le - ls;
    let looping = span > 1e-4
        && match c.loop_mode {
            1 | 3 => true,
            2 => e.gate,
            _ => false,
        };
    if c.loop_mode == 3 && looping {
        e.t += dt * if e.dir == 0.0 { 1.0 } else { e.dir };
        if e.t > le {
            e.t = le - (e.t - le).min(span);
            e.dir = -1.0;
        } else if e.t < ls && e.dir < 0.0 {
            e.t = ls + (ls - e.t).min(span);
            e.dir = 1.0;
        }
    } else {
        e.t += dt;
        if looping && e.t > le {
            e.t = ls + (e.t - le) % span;
        }
    }
    let end = c.pts[c.n - 1].0;
    if !e.gate && e.t >= end && c.loop_mode != 1 && c.loop_mode != 3 {
        e.active = false;
    }
    e.level = mseg_at(c, e.t);
    e.level
}

/// The sequencer's value at a position in steps (from the pattern start).
pub fn seq_at(c: &SeqCfg, pos: f32) -> f32 {
    let len = c.len.max(1);
    // swing: odd steps (1st, 3rd…) longer, even ones shorter; per-step swing shifts the start
    let pair = 2.0;
    let cycle = (pos / (len as f32)).floor();
    let p = pos - cycle * len as f32;
    let k0 = (p as usize).min(len - 1);
    let sw = c.swing * 0.5;
    // where step k starts inside its pair
    let start = |k: usize| -> f32 {
        let base = (k / 2) as f32 * pair + if k % 2 == 1 { 1.0 + sw } else { 0.0 };
        base + (c.sw[k] - 1.0) * 0.5
    };
    let mut k = k0;
    // the step whose start we're past (swing can move boundaries by up to a step)
    for cand in [k0.saturating_sub(1), k0, (k0 + 1).min(len - 1)] {
        if start(cand) <= p {
            k = cand;
        }
    }
    let st = start(k);
    let next = if k + 1 < len { start(k + 1) } else { len as f32 };
    let dur = (next - st).max(1e-3);
    let x = ((p - st) / dur).clamp(0.0, 1.0);
    // ties continue the previous step's level without a new envelope
    let mut src = k;
    while c.tie[src] && src > 0 {
        src -= 1;
    }
    let v = c.val[src];
    let gate = (c.gate * c.slen[k]).clamp(0.0, 1.0);
    let env = if c.tie[k] {
        1.0
    } else if c.attack > 0.0 && x < c.attack * gate {
        x / (c.attack * gate).max(1e-4)
    } else if x <= gate {
        1.0
    } else if c.release > 0.0 {
        (1.0 - (x - gate) / (c.release * (1.0 - gate)).max(1e-4)).max(0.0)
    } else {
        0.0
    };
    v * env
}

/// What one voice's modulators hold.
#[derive(Clone, Copy)]
pub struct ModVoice {
    pub fresh: bool,
    pub gate: bool,
    pub note: u8,
    pub vel: f32,
    pub index: u32,
    pub arp_index: u32,
    pub speed: f32,
    /// The arpeggiator step's Arp Mod 1–2 values and its place in the pattern (0..1).
    pub arp_mod: [f32; 2],
    pub playhead: f32,
    pub key: f32,
    pub t: f32,
    pub lfo_ph: [f32; N],
    pub lfo_rnd: [(f32, f32); N],
    pub lfo_seed: [u32; N],
    pub ahd: [EnvRun; N],
    pub mseg: [EnvRun; N],
    pub seq_pos: [f32; N],
    pub rnd: [f32; 4],
    pub smooth: [f32; ROUTES],
    pub out: Outs,
}

impl Default for ModVoice {
    fn default() -> Self {
        ModVoice {
            fresh: false,
            gate: false,
            note: 60,
            vel: 1.0,
            index: 0,
            arp_index: 0,
            speed: 0.0,
            arp_mod: [0.0; 2],
            playhead: 0.0,
            key: 60.0,
            t: 0.0,
            lfo_ph: [0.0; N],
            lfo_rnd: [(0.5, 0.5); N],
            lfo_seed: [0; N],
            ahd: [EnvRun::default(); N],
            mseg: [EnvRun::default(); N],
            seq_pos: [0.0; N],
            rnd: [0.0; 4],
            smooth: [0.0; ROUTES],
            out: Outs::default(),
        }
    }
}

/// Modulator outputs this block.
#[derive(Clone, Copy)]
pub struct Outs {
    pub lfo: [f32; N],
    pub ahd: [f32; N],
    pub mseg: [f32; N],
    pub seq: [f32; N],
}
impl Default for Outs {
    fn default() -> Self {
        Outs { lfo: [0.0; N], ahd: [0.0; N], mseg: [0.0; N], seq: [0.0; N] }
    }
}

/// Overlay of modulated parameters: (id, added amount, volume factor).
#[derive(Clone, Copy)]
pub struct Overlay {
    pub n: usize,
    pub ids: [u32; ROUTES],
    pub add: [f32; ROUTES],
    pub mul: [f32; ROUTES],
}
impl Default for Overlay {
    fn default() -> Self {
        Overlay { n: 0, ids: [0; ROUTES], add: [0.0; ROUTES], mul: [1.0; ROUTES] }
    }
}
impl Overlay {
    pub fn clear(&mut self) {
        self.n = 0;
    }
    fn slot(&mut self, id: u32) -> usize {
        if let Some(k) = self.ids[..self.n].iter().position(|&x| x == id) {
            return k;
        }
        let k = self.n.min(ROUTES - 1);
        self.ids[k] = id;
        self.add[k] = 0.0;
        self.mul[k] = 1.0;
        self.n = (self.n + 1).min(ROUTES);
        k
    }
    /// The parameter as modulated (base: its value; lo / hi its range).
    pub fn apply(&self, id: u32, base: f32, lo: f32, hi: f32) -> f32 {
        match self.ids[..self.n].iter().position(|&x| x == id) {
            Some(k) => ((base + self.add[k]) * self.mul[k]).clamp(lo, hi),
            None => base,
        }
    }
    pub fn has(&self, id: u32) -> bool {
        self.ids[..self.n].contains(&id)
    }
}

/// The instrument-wide side: free-running modulators, MIDI and Perform values, followers.
pub struct ModGlobal {
    pub lfo_ph: [f32; N],
    pub lfo_rnd: [(f32, f32); N],
    pub lfo_seed: [u32; N],
    pub ahd: [EnvRun; N],
    pub mseg: [EnvRun; N],
    pub envf: [f32; N],
    pub out: Outs,
    pub cc: [f32; 128],
    pub perform: [f32; 16],
    pub beats: f64,
    pub levels: [f32; 9],
    pub smooth: [f32; ROUTES],
    pub ov: Overlay,
    pub note_count: u32,
    pub arp_count: u32,
    pub last_note_t: f32,
    pub time: f32,
    pub held: usize,
    pub voices: usize,
    pub max_voices: usize,
}

impl Default for ModGlobal {
    fn default() -> Self {
        ModGlobal {
            lfo_ph: [0.0; N],
            lfo_rnd: [(0.5, 0.5); N],
            lfo_seed: [0; N],
            ahd: [EnvRun::default(); N],
            mseg: [EnvRun::default(); N],
            envf: [0.0; N],
            out: Outs::default(),
            cc: [0.0; 128],
            perform: [0.0; 16],
            beats: 0.0,
            levels: [0.0; 9],
            smooth: [f32::NAN; ROUTES],
            ov: Overlay::default(),
            note_count: 0,
            arp_count: 0,
            last_note_t: -10.0,
            time: 0.0,
            held: 0,
            voices: 0,
            max_voices: 16,
        }
    }
}

fn lfo_advance(c: &LfoCfg, ph: &mut f32, rnd: &mut (f32, f32), seed: &mut u32, dt: f32, bpm: f32) {
    let cps = if c.sync { bpm / 60.0 / c.beats.max(1e-3) } else { c.hz };
    let mult = shape_multiple(c.shape as usize) as f32;
    // the UHF shapes hold several copies, so the base rate runs slower per copy
    let inc = cps * dt / mult;
    *ph += inc;
    if *ph >= 1.0 {
        *ph -= ph.floor();
        *seed = seed.wrapping_add(1);
        rnd.0 = rnd.1;
        rnd.1 = hash01(seed.wrapping_mul(7919));
    }
}

fn lfo_value(c: &LfoCfg, ph: f32, rnd: (f32, f32), fade: f32) -> f32 {
    let v = shape_value(c.shape as usize, ph, rnd);
    let v = if c.bipolar { v - 0.5 } else { v };
    v * fade
}

/// Context for one block.
#[derive(Clone, Copy)]
pub struct Block {
    pub dt: f32,
    pub bpm: f32,
    pub bend: f32,
    pub wheel: f32,
    pub aftertouch: f32,
    pub glide: f32,
}

impl ModGlobal {
    /// The instrument-wide modulators over one block.
    pub fn step(&mut self, cfg: &ModCfg, b: &Block, used: &Used) {
        self.time += b.dt;
        self.beats += (b.dt * b.bpm / 60.0) as f64;
        let beat_dt = b.dt * b.bpm / 60.0;
        for i in 0..N {
            if used.lfo_g & (1 << i) != 0 {
                let c = &cfg.lfo[i];
                if c.sync {
                    // locked to the song position
                    let cyc = self.beats as f32 / c.beats.max(1e-3) / shape_multiple(c.shape as usize) as f32;
                    let ph = (cyc + c.phase).fract();
                    if ph < self.lfo_ph[i] {
                        self.lfo_seed[i] = self.lfo_seed[i].wrapping_add(1);
                        self.lfo_rnd[i] = (self.lfo_rnd[i].1, hash01(self.lfo_seed[i].wrapping_mul(7919)));
                    }
                    self.lfo_ph[i] = ph;
                } else {
                    lfo_advance(c, &mut self.lfo_ph[i], &mut self.lfo_rnd[i], &mut self.lfo_seed[i], b.dt, b.bpm);
                }
                self.out.lfo[i] = lfo_value(c, self.lfo_ph[i], self.lfo_rnd[i], 1.0);
            }
            if used.ahd_g & (1 << i) != 0 {
                let c = &cfg.ahd[i];
                self.out.ahd[i] = ahd_step(c, &mut self.ahd[i], if c.sync { beat_dt } else { b.dt });
            }
            if used.mseg_g & (1 << i) != 0 {
                let c = &cfg.mseg[i];
                self.out.mseg[i] = mseg_step(c, &mut self.mseg[i], if c.sync { beat_dt } else { b.dt });
            }
            if used.seq_g & (1 << i) != 0 {
                let c = &cfg.seq[i];
                let pos = self.beats as f32 / c.step_beats.max(1e-3);
                self.out.seq[i] = seq_at(c, pos % c.len.max(1) as f32);
            }
            if used.envf & (1 << i) != 0 {
                let c = &cfg.envf[i];
                let x = self.levels[(c.src as usize).min(8)];
                let k = if x > self.envf[i] { c.attack } else { c.release };
                let a = if k <= 0.0 { 1.0 } else { 1.0 - (-b.dt / k).exp() };
                self.envf[i] += (x - self.envf[i]) * a;
            }
        }
    }
    /// The first note after all were let go (Trigger Off envelopes start then).
    pub fn first_note(&mut self, cfg: &ModCfg) {
        for i in 0..N {
            self.ahd[i].start(cfg.ahd[i].trigger == 0);
            self.mseg[i].start(true);
        }
    }
    pub fn all_released(&mut self) {
        for i in 0..N {
            self.ahd[i].release();
            self.mseg[i].release();
        }
    }
}

/// Which modulators are in use (bit masks by instance), global and per voice.
#[derive(Clone, Copy, Default, Debug)]
pub struct Used {
    pub lfo_g: u16,
    pub ahd_g: u16,
    pub mseg_g: u16,
    pub seq_g: u16,
    pub envf: u16,
    pub lfo_v: u16,
    pub ahd_v: u16,
    pub mseg_v: u16,
    pub seq_v: u16,
    pub any_voice: bool,
    pub any: bool,
}

pub fn used(cfg: &ModCfg) -> Used {
    let mut u = Used::default();
    for r in &cfg.routes[..cfg.nroutes] {
        let bit = 1u16 << (r.idx.min(15));
        let pv = cfg.per_voice(r.kind, r.idx);
        match (r.kind, pv) {
            (K_LFO, false) => u.lfo_g |= bit,
            (K_LFO, true) => u.lfo_v |= bit,
            (K_AHD, false) => u.ahd_g |= bit,
            (K_AHD, true) => u.ahd_v |= bit,
            (K_MSEG, false) => u.mseg_g |= bit,
            (K_MSEG, true) => u.mseg_v |= bit,
            (K_SEQ, false) => u.seq_g |= bit,
            (K_SEQ, true) => u.seq_v |= bit,
            (K_ENVF, _) => u.envf |= bit,
            _ => {}
        }
        if pv {
            u.any_voice = true;
        }
        u.any = true;
    }
    u
}

impl ModVoice {
    /// A new note: restart what restarts with each note.
    pub fn init(&mut self, cfg: &ModCfg, g: &ModGlobal, legato: bool) {
        self.fresh = false;
        self.gate = true;
        self.t = 0.0;
        if !legato {
            self.smooth = [f32::NAN; ROUTES];
        }
        for i in 0..N {
            if !legato {
                self.lfo_ph[i] = cfg.lfo[i].phase;
                self.lfo_seed[i] = self.index.wrapping_mul(31).wrapping_add(i as u32);
                let a = hash01(self.lfo_seed[i]);
                self.lfo_rnd[i] = (a, hash01(self.lfo_seed[i] ^ 0x5555));
                self.ahd[i].start(true);
                self.mseg[i].start(true);
                self.seq_pos[i] = 0.0;
            } else {
                self.ahd[i].gate = true;
                self.mseg[i].gate = true;
            }
        }
        for k in 0..4 {
            self.rnd[k] = hash01(self.index.wrapping_mul(97).wrapping_add(k as u32 * 1013) ^ g.note_count);
        }
    }
    pub fn release(&mut self) {
        self.gate = false;
        for i in 0..N {
            self.ahd[i].release();
            self.mseg[i].release();
        }
    }
    /// The per-voice modulators over one block.
    pub fn step(&mut self, cfg: &ModCfg, b: &Block, used: &Used) {
        self.t += b.dt;
        let beat_dt = b.dt * b.bpm / 60.0;
        // Key Follow glides toward the note at the master Glide time
        let target = self.note as f32;
        self.key = if b.glide > 0.0 { target + (self.key - target) * (-b.dt / (b.glide * 0.3)).exp() } else { target };
        for i in 0..N {
            let bit = 1u16 << i;
            if used.lfo_v & bit != 0 {
                let c = &cfg.lfo[i];
                let (delay, attack) = if c.sync { (c.delay * 60.0 / b.bpm.max(1.0), c.attack * 60.0 / b.bpm.max(1.0)) } else { (c.delay, c.attack) };
                if self.t >= delay {
                    lfo_advance(c, &mut self.lfo_ph[i], &mut self.lfo_rnd[i], &mut self.lfo_seed[i], b.dt, b.bpm);
                    let fade = if attack > 0.0 { ((self.t - delay) / attack).min(1.0) } else { 1.0 };
                    self.out.lfo[i] = lfo_value(c, self.lfo_ph[i], self.lfo_rnd[i], fade);
                } else {
                    self.out.lfo[i] = 0.0;
                }
            }
            if used.ahd_v & bit != 0 {
                let c = &cfg.ahd[i];
                self.out.ahd[i] = ahd_step(c, &mut self.ahd[i], if c.sync { beat_dt } else { b.dt });
            }
            if used.mseg_v & bit != 0 {
                let c = &cfg.mseg[i];
                self.out.mseg[i] = mseg_step(c, &mut self.mseg[i], if c.sync { beat_dt } else { b.dt });
            }
            if used.seq_v & bit != 0 {
                let c = &cfg.seq[i];
                self.seq_pos[i] += beat_dt / c.step_beats.max(1e-3);
                self.out.seq[i] = seq_at(c, self.seq_pos[i] % c.len.max(1) as f32);
            }
        }
    }
    /// A Note Property source's value for this voice.
    pub fn note_prop(&self, k: usize, b: &Block, g: &ModGlobal) -> f32 {
        let i = self.index;
        let a = self.arp_index;
        match k {
            0 => self.vel,
            1 => self.key / 127.0,
            2 => ((self.key - 60.0) / 120.0).clamp(-0.5, 0.5),
            3 => b.aftertouch,
            4 => self.speed,
            5 => self.gate as u8 as f32,
            6 => (i % 2) as f32,
            7 => ((i / 2) % 2) as f32,
            8 => (a % 2) as f32,
            9 => (a % 3) as f32 / 2.0,
            10 => (a % 4) as f32 / 3.0,
            11 => (i % 4) as f32 / 3.0,
            12 => (i % 8) as f32 / 7.0,
            13 => (i % 16) as f32 / 15.0,
            14..=17 => self.rnd[k - 14],
            18 => 1.0,
            19 => g.voices as f32 / g.max_voices.max(1) as f32,
            20 => self.arp_mod[0],
            21 => self.arp_mod[1],
            _ => self.playhead,
        }
    }
}

/// A source's value (voice: the voice's modulators and note; None: instrument-wide).
pub fn source_value(cfg: &ModCfg, kind: u32, i: usize, v: Option<&ModVoice>, g: &ModGlobal, b: &Block) -> f32 {
    let j = i;
    let i = i.min(N - 1);
    let pv = cfg.per_voice(kind, i);
    match (kind, v) {
        (K_LFO, Some(v)) if pv => v.out.lfo[i],
        (K_AHD, Some(v)) if pv => v.out.ahd[i],
        (K_MSEG, Some(v)) if pv => v.out.mseg[i],
        (K_SEQ, Some(v)) if pv => v.out.seq[i],
        (K_LFO, _) => g.out.lfo[i],
        (K_AHD, _) => g.out.ahd[i],
        (K_MSEG, _) => g.out.mseg[i],
        (K_SEQ, _) => g.out.seq[i],
        (K_ENVF, _) => (g.envf[i] * cfg.envf[i].scale).min(1.0),
        (K_MIDI, _) => match j {
            0 => b.bend * 0.5,
            1 => b.wheel,
            k => {
                let cc = cfg.ctrl_cc[(k - 2).min(3)] as usize;
                let x = g.cc[cc.min(127)];
                if cfg.ctrl_bipolar && (71..=78).contains(&cc) { x - 0.5 } else { x }
            }
        },
        (K_NOTE, Some(v)) => v.note_prop(j, b, g),
        (K_NOTE, None) => 0.0,
        (K_PERF, _) => g.perform[j.min(15)],
        _ => 0.0,
    }
}

/// Fills `ov` with the routes' effect for a voice (or the instrument when `v` is None).
/// `range(id)` gives a target's (min, max); `voice_side` selects which routes apply.
pub fn build_overlay(cfg: &ModCfg, v: Option<&ModVoice>, g: &ModGlobal, b: &Block, smooth: &mut [f32; ROUTES], ov: &mut Overlay, range: &dyn Fn(u32) -> (f32, f32)) {
    ov.clear();
    let rs = &cfg.routes[..cfg.nroutes];
    let val = |r: &Route| -> f32 {
        let m = source_value(cfg, r.kind, r.idx, v, g, b);
        if r.mmap > 0 {
            let mm = &cfg.mmap[(r.mmap - 1).min(N - 1)];
            let bip = m < 0.0 || (r.kind == K_LFO && cfg.lfo[r.idx.min(N - 1)].bipolar);
            if bip { mm.map(m + 0.5) - 0.5 } else { mm.map(m) }
        } else {
            m
        }
    };
    // a route's depth with its depth modulators (two levels)
    let depth = |r: &Route| -> f32 {
        let mut d = r.depth;
        for q in rs.iter().filter(|q| q.target == DEPTH_TGT + r.n as u32) {
            let mut dq = q.depth;
            for q2 in rs.iter().filter(|q2| q2.target == DEPTH_TGT + q.n as u32) {
                dq += q2.depth * val(q2);
            }
            d += dq.clamp(-1.0, 1.0) * val(q);
        }
        d.clamp(-1.0, 1.0)
    };
    for r in rs {
        if r.target >= DEPTH_TGT {
            continue;
        }
        // instrument-wide overlays take only instrument-wide sources; per-voice ones take all
        if v.is_none() && cfg.per_voice(r.kind, r.idx) {
            continue;
        }
        let m = val(r);
        let d = depth(r);
        let (lo, hi) = range(r.target);
        let raw = if is_volume(r.target) {
            // volumes: depth 100 % makes the source scale the knob's value from 0 to all of it
            if d >= 0.0 { 1.0 + d * (m - 1.0) } else { 1.0 + d * m }
        } else {
            d * m * (hi - lo)
        };
        // Smooth: the target follows more gradually
        let s = &mut smooth[r.n];
        if r.smooth > 0.0 && !s.is_nan() {
            let k = 1.0 - (-b.dt / (r.smooth * r.smooth * 0.5)).exp();
            *s += (raw - *s) * k;
        } else {
            *s = raw;
        }
        let k = ov.slot(r.target);
        if is_volume(r.target) {
            ov.mul[k] *= s.max(0.0);
        } else {
            ov.add[k] += *s;
        }
    }
}

// ---------------------------------------------------------------- presets (Studio's own)

/// The Preset submenu's entries for a modulator kind: (name, field values).
pub fn presets(kind: u32) -> Vec<(&'static str, Vec<(u32, f32)>)> {
    match kind {
        K_LFO => vec![
            ("Slow Sine", vec![(LF_SHAPE, 2.0), (LF_RATE, 0.5), (LF_SYNC, 0.0), (LF_BIP, 1.0)]),
            ("Vibrato", vec![(LF_SHAPE, 2.0), (LF_RATE, 5.5), (LF_SYNC, 0.0), (LF_DELAY, 0.3), (LF_ATTACK, 0.6), (LF_BIP, 1.0)]),
            ("Eighth Gate", vec![(LF_SHAPE, 3.0), (LF_SYNC, 1.0), (LF_DIV, 10.0), (LF_BIP, 0.0)]),
            ("Sample & Hold 1/16", vec![(LF_SHAPE, 6.0), (LF_SYNC, 1.0), (LF_DIV, 13.0), (LF_BIP, 1.0)]),
            ("Ramp Bar", vec![(LF_SHAPE, 0.0), (LF_SYNC, 1.0), (LF_DIV, 3.0), (LF_BIP, 0.0)]),
        ],
        K_AHD => vec![
            ("Organ", vec![(AH_A, 0.002), (AH_H, 0.0), (AH_D, 0.0), (AH_S, 1.0), (AH_R, 0.05)]),
            ("Pluck", vec![(AH_A, 0.001), (AH_H, 0.0), (AH_D, 0.35), (AH_S, 0.0), (AH_R, 0.3), (AH_CD, 0.6)]),
            ("Pad", vec![(AH_A, 1.2), (AH_H, 0.0), (AH_D, 2.0), (AH_S, 0.8), (AH_R, 2.5)]),
            ("Swell", vec![(AH_A, 3.0), (AH_H, 0.5), (AH_D, 1.0), (AH_S, 0.6), (AH_R, 1.5), (AH_CA, -0.6)]),
        ],
        K_MSEG => vec![
            ("Ramp Down", mseg_pts(&[(0.0, 1.0), (1.0, 0.0)], 0)),
            ("Triangle Loop", mseg_pts(&[(0.0, 0.0), (0.25, 1.0), (0.5, 0.0)], 1)),
            ("Stairs", mseg_pts(&[(0.0, 0.0), (0.25, 0.0), (0.25, 0.33), (0.5, 0.33), (0.5, 0.66), (0.75, 0.66), (0.75, 1.0), (1.0, 1.0)], 0)),
        ],
        K_SEQ => vec![
            ("Ramp 16", (0..16).map(|k| (SQ_VAL + k, k as f32 / 15.0)).chain([(SQ_LEN, 16.0)]).collect()),
            ("Pulse 8", (0..8).map(|k| (SQ_VAL + k, if k % 2 == 0 { 1.0 } else { 0.0 })).chain([(SQ_LEN, 8.0)]).collect()),
            ("Off-beat", (0..16).map(|k| (SQ_VAL + k, if k % 4 == 2 { 1.0 } else { 0.2 })).chain([(SQ_LEN, 16.0)]).collect()),
        ],
        K_ENVF => vec![
            ("Fast", vec![(EF_ATTACK, 0.002), (EF_RELEASE, 0.05), (EF_SCALE, 1.0)]),
            ("Smooth", vec![(EF_ATTACK, 0.08), (EF_RELEASE, 0.6), (EF_SCALE, 1.5)]),
        ],
        _ => vec![
            ("Linear", mmap_pts(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)])),
            ("Convex", mmap_pts(&[(0.0, 0.0, 0.7), (1.0, 1.0, 0.0)])),
            ("Concave", mmap_pts(&[(0.0, 0.0, -0.7), (1.0, 1.0, 0.0)])),
            ("Invert", mmap_pts(&[(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)])),
            ("Dead Zone", mmap_pts(&[(0.0, 0.0, 0.0), (0.3, 0.0, 0.0), (1.0, 1.0, 0.0)])),
        ],
    }
}

fn mseg_pts(pts: &[(f32, f32)], loop_mode: u32) -> Vec<(u32, f32)> {
    let mut v = vec![(MS_N, pts.len() as f32), (MS_LOOP, loop_mode as f32), (MS_LS, 0.0), (MS_LE, (pts.len() - 1) as f32)];
    for (k, (t, l)) in pts.iter().enumerate() {
        let b = MS_PT + k as u32 * 3;
        v.extend([(b, *t), (b + 1, *l), (b + 2, 0.0)]);
    }
    v
}

fn mmap_pts(pts: &[(f32, f32, f32)]) -> Vec<(u32, f32)> {
    let mut v = vec![(MM_N, pts.len() as f32), (MM_EXISTS, 1.0)];
    for (k, (x, y, c)) in pts.iter().enumerate() {
        let b = MM_PT + k as u32 * 3;
        v.extend([(b, *x), (b + 1, *y), (b + 2, *c)]);
    }
    v
}

/// Randomize (AHDSR / MSEG / sequencer): field values, from a seed.
pub fn randomize(kind: u32, seed: u32, cur: &dyn Fn(u32) -> f32) -> Vec<(u32, f32)> {
    let r = |k: u32| hash01(seed.wrapping_mul(2654435761).wrapping_add(k * 7919));
    match kind {
        K_AHD => vec![(AH_A, r(1) * r(1) * 2.0), (AH_H, r(2) * 0.3), (AH_D, r(3) * 3.0), (AH_S, r(4)), (AH_R, r(5) * 3.0), (AH_CA, r(6) * 2.0 - 1.0), (AH_CD, r(7) * 2.0 - 1.0), (AH_CR, r(8) * 2.0 - 1.0)],
        K_MSEG => {
            let n = 3 + (r(1) * 6.0) as u32;
            let mut v = vec![(MS_N, n as f32)];
            let mut t = 0.0;
            for k in 0..n {
                let b = MS_PT + k * 3;
                if k > 0 {
                    t += 0.05 + r(10 + k) * 0.4;
                }
                v.extend([(b, t), (b + 1, r(40 + k)), (b + 2, r(70 + k) * 1.6 - 0.8)]);
            }
            v
        }
        _ => {
            // sequencer: random offsets to Swing, Attack, Gate and Release
            let off = |f: u32, k: u32| (f, (cur(f) + (r(k) - 0.5) * 0.5).clamp(0.0, 1.0));
            vec![off(SQ_SWING, 1), off(SQ_ATTACK, 2), off(SQ_GATE, 3), off(SQ_RELEASE, 4)]
        }
    }
}

/// The sequencer's Import Velocity (mode 0), Import Note (1) or Import Groove (2) from a short
/// MIDI file's notes (start tick, pitch, velocity) at `ppq` ticks per beat: step values from the
/// velocities or the pitches (two octaves up from the lowest C = 0…100 %), per-step swing from
/// how early or late each note is; gaps become zero steps.
pub fn seq_import(notes: &[(u32, u8, u8)], ppq: u32, step_beats: f32, mode: u8) -> Vec<(u32, f32)> {
    let step = (step_beats * ppq as f32).max(1.0);
    let Some(last) = notes.iter().map(|n| n.0).max() else { return Vec::new() };
    let len = (((last as f32 / step).round() as usize) + 1).min(SEQ_STEPS);
    let low_c = notes.iter().map(|n| n.1).min().unwrap_or(48) / 12 * 12;
    let mut v = Vec::new();
    if mode != 2 {
        v.push((SQ_LEN, len as f32));
    }
    for k in 0..len {
        let at = k as f32 * step;
        let near = notes.iter().filter(|n| (n.0 as f32 - at).abs() <= step * 0.5).min_by(|a, b| (a.0 as f32 - at).abs().total_cmp(&(b.0 as f32 - at).abs()));
        let (val, sw) = match near {
            Some(n) => {
                let val = if mode == 1 { ((n.1.saturating_sub(low_c)) as f32 / 24.0).min(1.0) } else { n.2 as f32 / 127.0 };
                (val, (1.0 + (n.0 as f32 - at) / step * 2.0).clamp(0.0, 2.0))
            }
            None => (0.0, 1.0),
        };
        if mode != 2 {
            v.push((SQ_VAL + k as u32, val));
            v.push((SQ_TIE + k as u32, 0.0));
        }
        v.push((SQ_SW + k as u32, sw));
    }
    v
}
