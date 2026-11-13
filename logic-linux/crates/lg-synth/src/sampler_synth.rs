//! The Sampler's synthesizer, as Logic's Synth, Modulators and Mod Matrix panes describe it:
//! two filters (Series / Parallel, Filter Blend, Drive, the Creamy / Edgy / Gritty / Lush /
//! Sharp designs), Envelope 1 (amplitude, AHDSR or AR with Delay, Curve and Vel) and up to four
//! more envelopes, four LFOs (six waveforms, tempo sync, fade in / out, phase, mono / poly,
//! unipolar / bipolar, key trigger), and twenty Mod Matrix routings (Source, Via, Target,
//! Amount with Min / Max when a Via is set, Inv buttons). Everything here is our own code.
//!
//! The parameter ids extend the Sampler's first fifteen (sampler::PARAMS), so instruments saved
//! before keep their values.

use std::sync::OnceLock;

use crate::dsp::{self, Ladder, Svf, SvfMode};
use lg_dsp::{Param, Unit};

// ---------------------------------------------------------------------------------------------
// Parameter ids

pub const F1_TYPE: u32 = 21;
pub const F1_DRIVE: u32 = 22;
pub const F2_ON: u32 = 23;
pub const F2_TYPE: u32 = 24;
pub const F2_CUTOFF: u32 = 25;
pub const F2_RES: u32 = 26;
pub const F2_DRIVE: u32 = 27;
pub const F_ROUTING: u32 = 28;
pub const F_BLEND: u32 = 29;
pub const PAN: u32 = 30;
pub const GLIDE: u32 = 31;
pub const BEND_DOWN: u32 = 32;
pub const VEL_OFFSET: u32 = 33;
pub const VEL_CURVE: u32 = 34;
pub const KEY_SCALE: u32 = 35;
pub const MODE: u32 = 36;
pub const UNISON: u32 = 37;
pub const RANDOM_DETUNE: u32 = 38;
pub const IGNORE_REL_VEL: u32 = 39;
pub const SEL_RANDOM: u32 = 40;
pub const VEL_RANDOM: u32 = 41;
pub const COARSE_REMOTE: u32 = 42;
pub const MONO_MODE: u32 = 43;
pub const MONO_BEND: u32 = 44;
/// Synth Details' Transpose: shifts the incoming notes (the Tune knob shifts the pitch).
pub const TRANSPOSE_NOTES: u32 = 45;
/// Envelope 1 (amplitude): type, delay, attack, curve, hold, decay, sustain, release, vel.
pub const ENV1: u32 = 50;
/// Envelopes 2–5: `env_base(k)` + [on, type, delay, attack, curve, hold, decay, sustain, release, vel].
pub fn env_base(k: usize) -> u32 {
    60 + (k as u32 - 2) * 10
}
/// LFOs 1–4: `lfo_base(k)` + [on, wave, sync, rate, sync rate, fade, fade out, phase, poly, bipolar, key trigger].
pub fn lfo_base(k: usize) -> u32 {
    100 + (k as u32 - 1) * 12
}
/// Mod Matrix rows 0–19: `mod_base(r)` + [on, source, source inv, via, via inv, target, amount, max amount].
pub fn mod_base(r: usize) -> u32 {
    200 + r as u32 * 8
}
pub const ROUTINGS: usize = 20;
pub const ENVS: usize = 5;
pub const LFOS: usize = 4;

pub const FILTER_TYPES: &[&str] = &[
    "Lowpass 24 dB Creamy",
    "Lowpass 12 dB Creamy",
    "Lowpass 24 dB Edgy",
    "Lowpass 12 dB Edgy",
    "Lowpass 12 dB Gritty",
    "Lowpass 24 dB Lush",
    "Lowpass 24 dB Lush (Fat)",
    "Lowpass 12 dB Sharp",
    "Highpass 24 dB Creamy",
    "Highpass 12 dB Creamy",
    "Highpass 24 dB Edgy",
    "Highpass 12 dB Gritty",
    "Highpass 12 dB Sharp",
    "Bandpass 12 dB Creamy",
    "Bandpass 12 dB Edgy",
    "Bandpass 12 dB Gritty",
    "Bandpass 12 dB Sharp",
    "Band Reject",
    "Peaking",
];
pub const ROUTING_CHOICES: &[&str] = &["Series", "Parallel"];
pub const MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const UNISON_CHOICES: &[&str] = &["Off", "2", "3", "4", "5", "6", "7", "8"];
pub const ENV_TYPES: &[&str] = &["AHDSR", "AR"];
pub const LFO_WAVES: &[&str] = &["Sine", "Triangle", "Sawtooth", "Square", "Random", "Smoothed Random"];
pub const MONO_MODES: &[&str] = &["Off", "On (Common Base Channel 1)", "On (Common Base Channel 16)"];
/// Synced LFO rates, as note values (and their length in beats): 32 bars to 1/128 triplets.
pub const SYNC_RATES: &[(&str, f64)] = &[
    ("32 bars", 128.0),
    ("16 bars", 64.0),
    ("8 bars", 32.0),
    ("4 bars", 16.0),
    ("2 bars", 8.0),
    ("1 bar", 4.0),
    ("1/2.", 3.0),
    ("1/2", 2.0),
    ("1/2T", 4.0 / 3.0),
    ("1/4.", 1.5),
    ("1/4", 1.0),
    ("1/4T", 2.0 / 3.0),
    ("1/8.", 0.75),
    ("1/8", 0.5),
    ("1/8T", 1.0 / 3.0),
    ("1/16.", 0.375),
    ("1/16", 0.25),
    ("1/16T", 1.0 / 6.0),
    ("1/32.", 0.1875),
    ("1/32", 0.125),
    ("1/32T", 1.0 / 12.0),
    ("1/64.", 0.09375),
    ("1/64", 0.0625),
    ("1/64T", 1.0 / 24.0),
    ("1/128", 0.03125),
    ("1/128T", 1.0 / 48.0),
];

/// Mod Matrix sources (also the Via list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    None,
    SideChain,
    Maximum,
    Random,
    Lfo(u8),
    Env(u8),
    Key,
    Velocity,
    ReleaseVelocity,
    PitchBend,
    Aftertouch,
    ModWheel,
    Cc(u8),
}

pub fn sources() -> &'static [&'static str] {
    static S: OnceLock<Vec<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        let mut v: Vec<&'static str> = vec!["None", "Side Chain", "Maximum", "Random"];
        for k in 1..=4 {
            v.push(Box::leak(format!("LFO {k}").into_boxed_str()));
        }
        for k in 1..=5 {
            v.push(Box::leak(format!("Envelope {k}").into_boxed_str()));
        }
        v.extend(["Key", "Velocity", "Release Velocity", "Pitch Bend", "Aftertouch", "Mod Wheel"]);
        for cc in 2..=119 {
            // Logic labels controller 11 Expression (7 and 10 drive the channel strip volume / pan)
            let name = if cc == 11 { "MIDI CC 11 (Expression)".to_string() } else { format!("MIDI CC {cc}") };
            v.push(Box::leak(name.into_boxed_str()));
        }
        v
    })
}

pub fn source_of(i: usize) -> Source {
    match i {
        0 => Source::None,
        1 => Source::SideChain,
        2 => Source::Maximum,
        3 => Source::Random,
        4..=7 => Source::Lfo((i - 4) as u8),
        8..=12 => Source::Env((i - 8) as u8),
        13 => Source::Key,
        14 => Source::Velocity,
        15 => Source::ReleaseVelocity,
        16 => Source::PitchBend,
        17 => Source::Aftertouch,
        18 => Source::ModWheel,
        n if n >= 19 && n < 19 + 118 => Source::Cc((n - 19 + 2) as u8),
        _ => Source::None,
    }
}

pub fn source_index(s: Source) -> usize {
    match s {
        Source::None => 0,
        Source::SideChain => 1,
        Source::Maximum => 2,
        Source::Random => 3,
        Source::Lfo(k) => 4 + k as usize,
        Source::Env(k) => 8 + k as usize,
        Source::Key => 13,
        Source::Velocity => 14,
        Source::ReleaseVelocity => 15,
        Source::PitchBend => 16,
        Source::Aftertouch => 17,
        Source::ModWheel => 18,
        Source::Cc(c) => 19 + c as usize - 2,
    }
}

/// Mod Matrix targets.
pub const TARGETS: &[&str] = &[
    "None",
    "Sample Select",
    "Sample Start",
    "Bit Resolution",
    "Pitch",
    "Glide Time",
    "Filter 1 Cutoff",
    "Filter 1 Resonance",
    "Filter 1 Drive",
    "Filter 2 Cutoff",
    "Filter 2 Resonance",
    "Filter 2 Drive",
    "Filter Blend",
    "Volume",
    "Pan",
    "LFO 1 Rate",
    "LFO 2 Rate",
    "LFO 3 Rate",
    "LFO 4 Rate",
    "Env 1 Attack",
    "Env 1 Decay",
    "Env 1 Release",
    "All Env Time Stages",
    "Articulation ID",
    "Loop Start",
    "Loop End",
    "Loop Position",
    "LFO 1 Fade",
    "LFO 2 Fade",
    "LFO 3 Fade",
    "LFO 4 Fade",
    "LFO 1 Phase",
    "LFO 2 Phase",
    "LFO 3 Phase",
    "LFO 4 Phase",
];
pub mod target {
    pub const NONE: usize = 0;
    pub const SAMPLE_SELECT: usize = 1;
    pub const SAMPLE_START: usize = 2;
    pub const BITS: usize = 3;
    pub const PITCH: usize = 4;
    pub const GLIDE: usize = 5;
    pub const F1_CUT: usize = 6;
    pub const F1_RES: usize = 7;
    pub const F1_DRIVE: usize = 8;
    pub const F2_CUT: usize = 9;
    pub const F2_RES: usize = 10;
    pub const F2_DRIVE: usize = 11;
    pub const BLEND: usize = 12;
    pub const VOLUME: usize = 13;
    pub const PAN: usize = 14;
    pub const LFO_RATE: usize = 15;
    pub const ENV1_ATTACK: usize = 19;
    pub const ENV1_DECAY: usize = 20;
    pub const ENV1_RELEASE: usize = 21;
    pub const ENV_ALL: usize = 22;
    pub const ARTICULATION: usize = 23;
    /// Loop Start / End move one loop point, Loop Position the whole loop (its length kept);
    /// full depth = the zone's whole length.
    pub const LOOP_START: usize = 24;
    pub const LOOP_END: usize = 25;
    pub const LOOP_POS: usize = 26;
    /// LFO n Fade scales its fade (ramp) time ×2^(4·mod); LFO n Phase offsets its start phase
    /// (by the value at note on).
    pub const LFO_FADE: usize = 27;
    pub const LFO_PHASE: usize = 31;
    pub const COUNT: usize = 35;
}

/// The synth parameters (ids 21 and up), appended to the Sampler's own.
pub fn params(base: &[Param]) -> Vec<Param> {
    let mut v = base.to_vec();
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    v.extend([
        Param::menu(F1_TYPE, "Filter 1 Type", FILTER_TYPES, 0.0),
        Param::new(F1_DRIVE, "Filter 1 Drive", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(F2_ON, "Filter 2", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(F2_TYPE, "Filter 2 Type", FILTER_TYPES, 8.0),
        Param::new(F2_CUTOFF, "Filter 2 Cutoff", 20.0, 20000.0, 20.0, Unit::Hz),
        Param::new(F2_RES, "Filter 2 Resonance", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(F2_DRIVE, "Filter 2 Drive", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::menu(F_ROUTING, "Filter Routing", ROUTING_CHOICES, 0.0),
        Param::new(F_BLEND, "Filter Blend", -1.0, 1.0, 0.0, Unit::None),
        Param::new(PAN, "Pan", -1.0, 1.0, 0.0, Unit::Pan),
        Param::new(GLIDE, "Glide", 0.0, 5.0, 0.0, Unit::Seconds),
        Param::new(BEND_DOWN, "Pitch Bend Down", -1.0, 36.0, -1.0, Unit::Semitones),
        Param::new(VEL_OFFSET, "Velocity Offset", -127.0, 127.0, 0.0, Unit::None),
        Param::new(VEL_CURVE, "Amp Velocity Curve", -100.0, 100.0, 0.0, Unit::Percent),
        Param::new(KEY_SCALE, "Amp Key Scale", -100.0, 100.0, 0.0, Unit::Percent),
        Param::menu(MODE, "Mode", MODES, 0.0),
        Param::menu(UNISON, "Unison", UNISON_CHOICES, 0.0),
        Param::new(RANDOM_DETUNE, "Random Detune", 0.0, 100.0, 0.0, Unit::Cents),
        Param::new(IGNORE_REL_VEL, "Ignore Release Velocity", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(SEL_RANDOM, "Sample Select Random", 0.0, 127.0, 0.0, Unit::None),
        Param::new(VEL_RANDOM, "Velocity Random", 0.0, 127.0, 0.0, Unit::None),
        Param::new(COARSE_REMOTE, "Coarse Tune Remote", -1.0, 127.0, -1.0, Unit::None),
        Param::menu(MONO_MODE, "MIDI Mono Mode", MONO_MODES, 0.0),
        Param::new(MONO_BEND, "Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones),
        Param::new(TRANSPOSE_NOTES, "Transpose", -48.0, 48.0, 0.0, Unit::Semitones),
    ]);
    // Envelope 1
    v.extend([
        Param::menu(ENV1, "Env 1 Type", ENV_TYPES, 0.0),
        Param::new(ENV1 + 1, "Env 1 Delay", 0.0, 10.0, 0.0, Unit::Seconds),
        Param::new(ENV1 + 2, "Env 1 Attack", 0.0, 10.0, 0.0, Unit::Seconds),
        Param::new(ENV1 + 3, "Env 1 Curve", -1.0, 1.0, 0.0, Unit::None),
        Param::new(ENV1 + 4, "Env 1 Hold", 0.0, 10.0, 0.0, Unit::Seconds),
        Param::new(ENV1 + 5, "Env 1 Decay", 0.0, 20.0, 0.0, Unit::Seconds),
        Param::new(ENV1 + 6, "Env 1 Sustain", 0.0, 1.0, 1.0, Unit::Ratio),
        Param::new(ENV1 + 7, "Env 1 Release", 0.0, 20.0, 0.1, Unit::Seconds),
        Param::new(ENV1 + 8, "Env 1 Vel", 0.0, 1.0, 1.0, Unit::Ratio),
    ]);
    for k in 2..=ENVS {
        let b = env_base(k);
        let n = |s: &str| leak(format!("Env {k} {s}"));
        v.extend([
            Param::new(b, n("On"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(b + 1, n("Type"), ENV_TYPES, 0.0),
            Param::new(b + 2, n("Delay"), 0.0, 10.0, 0.0, Unit::Seconds),
            Param::new(b + 3, n("Attack"), 0.0, 10.0, 0.01, Unit::Seconds),
            Param::new(b + 4, n("Curve"), -1.0, 1.0, 0.0, Unit::None),
            Param::new(b + 5, n("Hold"), 0.0, 10.0, 0.0, Unit::Seconds),
            Param::new(b + 6, n("Decay"), 0.0, 20.0, 0.5, Unit::Seconds),
            Param::new(b + 7, n("Sustain"), 0.0, 1.0, 0.5, Unit::Ratio),
            Param::new(b + 8, n("Release"), 0.0, 20.0, 0.3, Unit::Seconds),
            Param::new(b + 9, n("Vel"), 0.0, 1.0, 0.0, Unit::Ratio),
        ]);
    }
    static RATE_NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let rates: &'static [&'static str] = RATE_NAMES.get_or_init(|| SYNC_RATES.iter().map(|r| r.0).collect()).as_slice();
    for k in 1..=LFOS {
        let b = lfo_base(k);
        let n = |s: &str| leak(format!("LFO {k} {s}"));
        v.extend([
            Param::new(b, n("On"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(b + 1, n("Waveform"), LFO_WAVES, 0.0),
            Param::new(b + 2, n("Sync"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(b + 3, n("Rate"), 0.01, 40.0, 5.0, Unit::Hz),
            Param::menu(b + 4, n("Sync Rate"), rates, 10.0),
            Param::new(b + 5, n("Fade"), 0.0, 10.0, 0.0, Unit::Seconds),
            Param::new(b + 6, n("Fade Out"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(b + 7, n("Phase"), 0.0, 360.0, 0.0, Unit::None),
            Param::new(b + 8, n("Poly"), 0.0, 1.0, 1.0, Unit::OnOff),
            Param::new(b + 9, n("Bipolar"), 0.0, 1.0, 1.0, Unit::OnOff),
            Param::new(b + 10, n("Key Trigger"), 0.0, 1.0, 1.0, Unit::OnOff),
        ]);
    }
    let src = sources();
    for r in 0..ROUTINGS {
        let b = mod_base(r);
        let n = |s: &str| leak(format!("Mod {} {s}", r + 1));
        // the default routing: Velocity → Sample Select (Logic's, choosing velocity layers)
        let (on, s, t, amt) = if r == 0 { (1.0, source_index(Source::Velocity) as f32, target::SAMPLE_SELECT as f32, 1.0) } else { (0.0, 0.0, 0.0, 0.0) };
        v.extend([
            Param::new(b, n("On"), 0.0, 1.0, on, Unit::OnOff),
            Param::menu(b + 1, n("Source"), src, s),
            Param::new(b + 2, n("Source Inv"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(b + 3, n("Via"), src, 0.0),
            Param::new(b + 4, n("Via Inv"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(b + 5, n("Target"), TARGETS, t),
            Param::new(b + 6, n("Amount"), -1.0, 1.0, amt, Unit::None),
            Param::new(b + 7, n("Max Amount"), -1.0, 1.0, 0.0, Unit::None),
        ]);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Settings as the voices read them

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvSet {
    pub on: bool,
    pub ar: bool,
    pub delay: f32,
    pub attack: f32,
    pub curve: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub vel: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LfoSet {
    pub on: bool,
    pub wave: u8,
    pub sync: bool,
    pub hz: f32,
    pub beats: f32,
    pub fade: f32,
    pub fade_out: bool,
    pub phase: f32,
    pub poly: bool,
    pub bipolar: bool,
    pub key_trigger: bool,
}

impl LfoSet {
    /// Cycles per second at this tempo.
    pub fn rate(&self, bpm: f32) -> f32 {
        if self.sync { bpm / 60.0 / self.beats.max(1e-3) } else { self.hz }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Route {
    pub on: bool,
    pub src: Source,
    pub src_inv: bool,
    pub via: Source,
    pub via_inv: bool,
    pub target: usize,
    pub amount: f32,
    pub max: f32,
}

impl Default for Source {
    fn default() -> Self {
        Source::None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FilterSet {
    pub on: bool,
    pub kind: u8,
    pub cutoff: f32,
    pub res: f32,
    pub drive: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Synth {
    pub f: [FilterSet; 2],
    pub parallel: bool,
    pub blend: f32,
    pub pan: f32,
    pub glide: f32,
    pub env: [EnvSet; ENVS],
    pub lfo: [LfoSet; LFOS],
    pub routes: [Route; ROUTINGS],
    pub nroutes: usize,
    pub vel_offset: f32,
    pub vel_curve: f32,
    pub key_scale: f32,
    pub mode: u8,
    pub unison: u8,
    pub detune: f32,
    pub sel_random: f32,
    pub vel_random: f32,
    pub coarse_remote: i32,
    pub bpm: f32,
    /// A routing to each target exists (for quick skips).
    pub has_target: [bool; target::COUNT],
}

impl Default for Synth {
    fn default() -> Self {
        Synth {
            f: [FilterSet { on: false, kind: 0, cutoff: 20000.0, res: 0.0, drive: 0.0 }; 2],
            parallel: false,
            blend: 0.0,
            pan: 0.0,
            glide: 0.0,
            env: [EnvSet::default(); ENVS],
            lfo: [LfoSet::default(); LFOS],
            routes: [Route::default(); ROUTINGS],
            nroutes: 0,
            vel_offset: 0.0,
            vel_curve: 0.0,
            key_scale: 0.0,
            mode: 0,
            unison: 1,
            detune: 0.0,
            sel_random: 0.0,
            vel_random: 0.0,
            coarse_remote: -1,
            bpm: 120.0,
            has_target: [false; target::COUNT],
        }
    }
}

impl Synth {
    /// Reads the settings from the parameter values (`p(id)`).
    pub fn read(&mut self, p: &dyn Fn(u32) -> f32) {
        self.f[0] = FilterSet { on: p(5) >= 0.5, kind: p(F1_TYPE).round() as u8, cutoff: p(6), res: p(7), drive: p(F1_DRIVE) };
        self.f[1] = FilterSet { on: p(F2_ON) >= 0.5, kind: p(F2_TYPE).round() as u8, cutoff: p(F2_CUTOFF), res: p(F2_RES), drive: p(F2_DRIVE) };
        self.parallel = p(F_ROUTING) >= 0.5;
        self.blend = p(F_BLEND);
        self.pan = p(PAN);
        self.glide = p(GLIDE);
        self.vel_offset = p(VEL_OFFSET);
        self.vel_curve = p(VEL_CURVE) / 100.0;
        self.key_scale = p(KEY_SCALE) / 100.0;
        self.mode = p(MODE).round() as u8;
        self.unison = if p(UNISON) < 0.5 { 1 } else { p(UNISON).round() as u8 + 1 };
        self.detune = p(RANDOM_DETUNE);
        self.sel_random = p(SEL_RANDOM);
        self.vel_random = p(VEL_RANDOM);
        self.coarse_remote = p(COARSE_REMOTE).round() as i32;
        let env = |b: u32, on: bool| EnvSet {
            on,
            ar: p(b).round() as i32 == 1,
            delay: p(b + 1),
            attack: p(b + 2),
            curve: p(b + 3),
            hold: p(b + 4),
            decay: p(b + 5),
            sustain: p(b + 6),
            release: p(b + 7),
            vel: p(b + 8),
        };
        self.env[0] = env(ENV1, true);
        for k in 2..=ENVS {
            let b = env_base(k);
            self.env[k - 1] = env(b + 1, p(b) >= 0.5);
        }
        for k in 1..=LFOS {
            let b = lfo_base(k);
            let si = (p(b + 4).round().max(0.0) as usize).min(SYNC_RATES.len() - 1);
            self.lfo[k - 1] = LfoSet {
                on: p(b) >= 0.5,
                wave: p(b + 1).round() as u8,
                sync: p(b + 2) >= 0.5,
                hz: p(b + 3),
                beats: SYNC_RATES[si].1 as f32,
                fade: p(b + 5),
                fade_out: p(b + 6) >= 0.5,
                phase: p(b + 7) / 360.0,
                poly: p(b + 8) >= 0.5,
                bipolar: p(b + 9) >= 0.5,
                key_trigger: p(b + 10) >= 0.5,
            };
        }
        self.nroutes = 0;
        self.has_target = [false; target::COUNT];
        for r in 0..ROUTINGS {
            let b = mod_base(r);
            let rt = Route {
                on: p(b) >= 0.5,
                src: source_of(p(b + 1).round().max(0.0) as usize),
                src_inv: p(b + 2) >= 0.5,
                via: source_of(p(b + 3).round().max(0.0) as usize),
                via_inv: p(b + 4) >= 0.5,
                target: (p(b + 5).round().max(0.0) as usize).min(target::COUNT - 1),
                amount: p(b + 6),
                max: p(b + 7),
            };
            if rt.on && rt.target != target::NONE && rt.src != Source::None {
                self.has_target[rt.target] = true;
                self.routes[self.nroutes] = rt;
                self.nroutes += 1;
            }
        }
    }
}

/// The value of a routing given its source and via values (via: None = no via).
#[inline]
pub fn route_value(rt: &Route, src: f32, via: Option<f32>) -> f32 {
    let s = if rt.src_inv { -src } else { src };
    match via {
        None => s * rt.amount,
        Some(v) => {
            let v = if rt.via_inv { 1.0 - v } else { v };
            s * (rt.amount + (rt.max - rt.amount) * v.clamp(0.0, 1.0))
        }
    }
}

/// Logic's XFade types: 0 Linear dB, 1 Linear Gain, 2 Equal Power. `t` 0..1 into the fade.
pub fn xfade_gain(kind: u8, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        0 => lg_dsp::db_to_gain(-60.0 * (1.0 - t)) * if t <= 0.0 { 0.0 } else { 1.0 },
        1 => t,
        _ => (t * std::f32::consts::FRAC_PI_2).sin(),
    }
}

// ---------------------------------------------------------------------------------------------
// Envelope

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EnvStage {
    #[default]
    Idle,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// Delay, attack (with a curve), hold, decay, sustain, release — or attack / release (AR).
#[derive(Clone, Copy, Debug, Default)]
pub struct Env {
    pub stage: EnvStage,
    pub out: f32,
    t: f32,
    delay: f32,
    attack_inc: f32,
    curve: f32,
    hold: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
    ar: bool,
}

impl Env {
    /// Starts the envelope; `scale` multiplies every time (Time-type modulation).
    pub fn start(&mut self, s: &EnvSet, times: [f32; 4], rate: f32) {
        // times: attack, decay, release, all (multipliers)
        let all = times[3];
        self.ar = s.ar;
        self.delay = s.delay * all * rate;
        self.attack_inc = 1.0 / ((s.attack * times[0] * all).max(0.0005) * rate);
        self.curve = s.curve;
        self.hold = s.hold * all * rate;
        self.decay_coef = dsp::t60_coef((s.decay * times[1] * all).max(0.001), rate);
        self.sustain = if s.ar { 1.0 } else { s.sustain.clamp(0.0, 1.0) };
        if !s.ar && s.decay <= 0.0 {
            // no decay: the level stays at full until release, as Logic's with Decay 0 and Sustain 100 %
            self.sustain = if s.sustain >= 0.999 { 1.0 } else { s.sustain };
            self.decay_coef = 0.0;
        }
        self.release_coef = dsp::t60_coef((s.release * times[2] * all).max(0.002), rate);
        self.t = 0.0;
        self.out = 0.0;
        self.stage = if self.delay > 0.0 { EnvStage::Delay } else { EnvStage::Attack };
    }
    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }
    /// A legato note: back to the attack from the current level (no restart).
    pub fn retrigger(&mut self) {
        if self.stage != EnvStage::Idle {
            self.t = self.out;
            self.stage = EnvStage::Attack;
        }
    }
    #[inline]
    fn shape(&self, t: f32) -> f32 {
        // curve > 0: slow start (exponential), < 0: fast start (logarithmic)
        let c = self.curve;
        if c.abs() < 1e-3 {
            t
        } else if c > 0.0 {
            t.powf(1.0 + 3.0 * c)
        } else {
            1.0 - (1.0 - t).powf(1.0 - 3.0 * c)
        }
    }
    /// Advances `n` samples (control rate) and returns the level.
    #[inline]
    pub fn advance(&mut self, n: usize) -> f32 {
        let n = n as f32;
        match self.stage {
            EnvStage::Idle => self.out = 0.0,
            EnvStage::Delay => {
                self.delay -= n;
                if self.delay <= 0.0 {
                    self.stage = EnvStage::Attack;
                }
            }
            EnvStage::Attack => {
                self.t += self.attack_inc * n;
                if self.t >= 1.0 {
                    self.t = 1.0;
                    self.stage = if self.ar { EnvStage::Sustain } else if self.hold > 0.0 { EnvStage::Hold } else { EnvStage::Decay };
                }
                self.out = self.shape(self.t);
            }
            EnvStage::Hold => {
                self.out = 1.0;
                self.hold -= n;
                if self.hold <= 0.0 {
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                let c = self.decay_coef.powf(n);
                self.out = self.sustain + (self.out - self.sustain) * c;
                if (self.out - self.sustain).abs() < 1e-5 {
                    self.out = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.out = self.sustain,
            EnvStage::Release => {
                self.out *= self.release_coef.powf(n);
                if self.out < 1e-5 {
                    self.out = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.out
    }
    pub fn idle(&self) -> bool {
        self.stage == EnvStage::Idle
    }
}

// ---------------------------------------------------------------------------------------------
// LFO

#[derive(Clone, Copy, Debug, Default)]
pub struct Lfo {
    pub phase: f32,
    pub held: f32,
    pub prev: f32,
    pub seed: u32,
    /// Seconds since the note began (for the fade).
    pub age: f32,
}

impl Lfo {
    pub fn restart(&mut self, s: &LfoSet, seed: u32) {
        self.phase = s.phase;
        self.age = 0.0;
        self.seed = seed | 1;
        self.held = self.rand();
        self.prev = self.held;
    }
    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    /// Advances `dt` seconds at `hz` and returns the value (−1..1 bipolar or 0..1 unipolar),
    /// with the fade applied.
    pub fn advance(&mut self, s: &LfoSet, hz: f32, dt: f32) -> f32 {
        let mut ph = self.phase + hz * dt;
        if ph >= 1.0 {
            ph = ph.fract();
            self.prev = self.held;
            self.held = self.rand();
        }
        self.phase = ph;
        self.age += dt;
        let x = match s.wave {
            0 => dsp::sin_cycles(ph),
            1 => 1.0 - 4.0 * (ph - 0.5).abs(),
            2 => 2.0 * ph - 1.0,
            3 => if ph < 0.5 { 1.0 } else { -1.0 },
            4 => self.held,
            _ => self.prev + (self.held - self.prev) * (0.5 - 0.5 * (std::f32::consts::PI * ph).cos()),
        };
        let x = if s.bipolar { x } else { x * 0.5 + 0.5 };
        let fade = if s.fade <= 0.0 {
            1.0
        } else if s.fade_out {
            (1.0 - self.age / s.fade).max(0.0)
        } else {
            (self.age / s.fade).min(1.0)
        };
        x * fade
    }
}

// ---------------------------------------------------------------------------------------------
// Filter

/// One filter of the Synth pane, per channel, for any of FILTER_TYPES.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilterUnit {
    svf: [[Svf; 2]; 2],
    ladder: [Ladder; 2],
    kind: u8,
}

impl FilterUnit {
    fn design(kind: u8) -> (SvfMode, u8, u8) {
        // (mode, poles, design: 0 creamy, 1 edgy, 2 gritty, 3 lush, 4 lush fat, 5 sharp)
        match kind {
            0 => (SvfMode::Low, 4, 0),
            1 => (SvfMode::Low, 2, 0),
            2 => (SvfMode::Low, 4, 1),
            3 => (SvfMode::Low, 2, 1),
            4 => (SvfMode::Low, 2, 2),
            5 => (SvfMode::Low, 4, 3),
            6 => (SvfMode::Low, 4, 4),
            7 => (SvfMode::Low, 2, 5),
            8 => (SvfMode::High, 4, 0),
            9 => (SvfMode::High, 2, 0),
            10 => (SvfMode::High, 4, 1),
            11 => (SvfMode::High, 2, 2),
            12 => (SvfMode::High, 2, 5),
            13 => (SvfMode::Band, 2, 0),
            14 => (SvfMode::Band, 2, 1),
            15 => (SvfMode::Band, 2, 2),
            16 => (SvfMode::Band, 2, 5),
            17 => (SvfMode::Notch, 2, 0),
            _ => (SvfMode::Peak, 2, 0),
        }
    }
    pub fn reset(&mut self) {
        for c in &mut self.svf {
            for s in c {
                s.reset();
            }
        }
        for l in &mut self.ladder {
            l.reset();
        }
    }
    /// Sets the type, cutoff (Hz) and resonance (0..1).
    pub fn set(&mut self, kind: u8, hz: f32, res: f32, rate: f32) {
        let (mode, poles, design) = Self::design(kind);
        if kind != self.kind {
            self.reset();
            self.kind = kind;
        }
        let hz = hz.clamp(20.0, rate * 0.45);
        let res = res.clamp(0.0, 1.0);
        if mode == SvfMode::Low && matches!(design, 1 | 3 | 4) {
            for l in &mut self.ladder {
                l.poles = poles;
                l.edgy = design == 1;
                l.set(hz, res * 1.02, rate);
            }
            return;
        }
        let q = match design {
            5 => 0.7 + res * res * 30.0,
            2 => 0.6 + res * 12.0,
            _ => 0.5 + res * res * 20.0,
        };
        // cascaded stages share the resonance (the first carries the peak)
        for c in &mut self.svf {
            c[0].mode = mode;
            c[1].mode = mode;
            c[0].set(hz, q, rate);
            c[1].set(hz, if poles == 4 { 0.707 } else { q }, rate);
        }
    }
    #[inline]
    pub fn run(&mut self, ch: usize, x: f32, drive: f32) -> f32 {
        let (mode, poles, design) = Self::design(self.kind);
        let g = 1.0 + drive * 6.0;
        let x = if drive > 0.0 { dsp::fast_tanh(x * g) / g.sqrt() } else { x };
        if mode == SvfMode::Low && matches!(design, 1 | 3 | 4) {
            let l = &mut self.ladder[ch];
            l.drive = if design == 4 { 1.4 } else { 1.0 };
            let y = l.run(x);
            return if design == 4 { y * 1.25 } else { y };
        }
        let s = &mut self.svf[ch];
        let mut y = s[0].run(x);
        if design == 2 {
            y = dsp::fast_tanh(y * 1.5) / 1.5;
        }
        if poles == 4 {
            y = s[1].run(y);
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_round_trip() {
        for i in 0..sources().len() {
            assert_eq!(source_index(source_of(i)), i, "{}", sources()[i]);
        }
        assert_eq!(sources()[source_index(Source::Cc(119))], "MIDI CC 119");
        assert_eq!(sources()[source_index(Source::Cc(11))], "MIDI CC 11 (Expression)");
    }

    #[test]
    fn envelope_stages() {
        let s = EnvSet { on: true, ar: false, delay: 0.01, attack: 0.01, curve: 0.0, hold: 0.01, decay: 0.1, sustain: 0.5, release: 0.1, vel: 0.0 };
        let mut e = Env::default();
        e.start(&s, [1.0; 4], 1000.0);
        assert_eq!(e.stage, EnvStage::Delay);
        for _ in 0..15 {
            e.advance(1);
        }
        assert_eq!(e.stage, EnvStage::Attack);
        for _ in 0..30 {
            e.advance(1);
        }
        assert!(e.out > 0.5);
        for _ in 0..2000 {
            e.advance(1);
        }
        assert!((e.out - 0.5).abs() < 0.01);
        e.gate_off();
        for _ in 0..2000 {
            e.advance(1);
        }
        assert!(e.idle());
    }

    #[test]
    fn lfo_waves_and_fade() {
        let mut s = LfoSet { on: true, wave: 3, sync: false, hz: 1.0, beats: 1.0, fade: 0.0, fade_out: false, phase: 0.0, poly: true, bipolar: false, key_trigger: true };
        let mut l = Lfo::default();
        l.restart(&s, 7);
        let a = l.advance(&s, 1.0, 0.1);
        let b = l.advance(&s, 1.0, 0.5);
        assert_eq!((a, b), (1.0, 0.0), "unipolar square: a positive value and 0");
        s.bipolar = true;
        s.fade = 1.0;
        l.restart(&s, 7);
        let early = l.advance(&s, 1.0, 0.1).abs();
        assert!(early <= 0.11, "fade in starts near zero: {early}");
        assert_eq!(SYNC_RATES.first().unwrap().0, "32 bars");
        assert_eq!(SYNC_RATES.last().unwrap().0, "1/128T");
    }

    #[test]
    fn via_scales_between_min_and_max() {
        let rt = Route { on: true, src: Source::Maximum, src_inv: false, via: Source::ModWheel, via_inv: false, target: target::PITCH, amount: 0.2, max: 0.8 };
        assert!((route_value(&rt, 1.0, Some(0.0)) - 0.2).abs() < 1e-6);
        assert!((route_value(&rt, 1.0, Some(1.0)) - 0.8).abs() < 1e-6);
        assert!((route_value(&rt, 1.0, None) - 0.2).abs() < 1e-6);
    }
}
