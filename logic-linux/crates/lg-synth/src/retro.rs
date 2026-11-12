//! "Retro Synth" stand-in: a four-engine polyphonic synth (Analog, Sync, Table, FM) with Logic's
//! parameter list (ids = the order of plugins/RETRO-SYNTH-PARAM-MAP.txt; Logic's float block for
//! this plug-in is not yet aligned, see the report). Unison per voice, glide/autobend, mono/legato,
//! ladder and state-variable filters, two envelopes, LFO, vibrato, a small mod matrix
//! (velocity, mod wheel, aftertouch, a free controller) and the chorus/flanger effect.
//! Also the engine behind most synth patches (bass, lead, pad, pluck, brass, bells…).

use std::sync::OnceLock;

use crate::dsp::{self, Adsr, CHUNK, Chorus, Fade, Flanger, Ladder, Osc, Rng, Svf, SvfMode, Wave};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const ENGINES: &[&str] = &["Analog", "Sync", "Table", "FM"];
pub const MIDI_MODE: &[&str] = &["Poly", "Mono", "Legato"];
pub const MOD_TARGETS: &[&str] = &[
    "Off",
    "Cutoff",
    "Resonance",
    "Volume",
    "Osc Shape",
    "Osc Mix",
    "Pitch",
    "LFO Amount",
    "Vibrato",
    "Filter FM",
    "Wave Variation",
];
pub const FILTERS: &[&str] = &[
    "LP 24dB Lush",
    "LP 24dB Edgy",
    "LP 12dB",
    "Band Pass",
    "High Pass",
    "Peak",
    "Band Reject",
];
pub const EFFECTS: &[&str] = &["Chorus", "Flanger"];
pub const LFO_WAVES: &[&str] = &[
    "Sine",
    "Triangle",
    "Saw Up",
    "Saw Down",
    "Square",
    "Sample & Hold",
    "Random",
];
pub const VIA: &[&str] = &[
    "Off",
    "Mod Wheel",
    "Aftertouch",
    "Velocity",
    "Ctrl X",
    "Mod Wheel + Aftertouch",
];
pub const GLIDE_TYPE: &[&str] = &["Glide", "Autobend"];
pub const GLIDE_MODE: &[&str] = &["Always", "Legato"];
pub const FILTER_MODES: &[&str] = &["Legacy", "Lowpass", "Highpass", "Bandpass", "Band Reject", "Peak"];
pub const FILTER_VARIANTS: &[&str] = &["Lush", "Creamy", "Edgy", "Gritty", "Lush (Fat)", "Sharp"];
pub const FILTER_SLOPES: &[&str] = &["6 dB/oct", "12 dB/oct", "18 dB/oct", "24 dB/oct"];
pub const FM_MODES: &[&str] = &["FM", "FM + Harmonic", "Harmonic"];
pub const TABLE_MODES: &[&str] = &["Shape", "Formant"];
pub const GLIDE_TARGETS: &[&str] = &["Oscillator 1 + Sine", "Oscillator 2", "All Oscillators", "Opposed", "Oscillators + Filter"];
pub const VOICE_MODES: &[&str] = &["Single", "Double"];
pub const SYNC_DIVS: &[&str] = &["4 Bars", "2 Bars", "1 Bar", "1/2", "1/4", "1/8", "1/16", "1/32"];
/// Length of each sync division in beats.
pub const SYNC_BEATS: [f32; 8] = [16.0, 8.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.125];
pub const MPE_MODES: &[&str] = &["Off", "On (Common Base Channel 1)", "On (Common Base Channel 16)"];
pub const APP_TABLES: &[&str] = &["Digiwaves", "Organ Drawbars", "Vowels", "Pulse Sweep", "Bell Stack", "Odd to Even"];
pub const TABLE_SOURCES: &[&str] = &["App", "Custom", "User"];

// ids of the parameters added after Logic's list (the first 66 keep their meaning)
pub const OSC1_NOISE: u32 = 66;
pub const OSC2_NOISE: u32 = 67;
pub const FILTER_MODE: u32 = 68;
pub const FILTER_VARIANT: u32 = 69;
pub const FILTER_SLOPE: u32 = 70;
pub const BR_AMOUNT: u32 = 71;
pub const PEAK_GAIN: u32 = 72;
pub const PEAK_BW: u32 = 73;
pub const FILTER_FM: u32 = 74;
pub const CUTOFF_LFO2: u32 = 75;
pub const FM_MODE: u32 = 76;
pub const FM_HARMONIC: u32 = 77;
pub const FM_INHARMONIC: u32 = 78;
pub const FM_SHAPE: u32 = 79;
pub const FM_MIX: u32 = 80;
pub const SYNC_AMOUNT: u32 = 81;
pub const TABLE_MODE: u32 = 82;
pub const GLIDE_ON: u32 = 83;
pub const GLIDE_TARGET: u32 = 84;
pub const POLY_LFO: u32 = 85;
pub const LFO_VIA_SPEED: u32 = 86;
pub const VIB_VIA_SPEED: u32 = 87;
pub const LFO_DIV: u32 = 88;
pub const VIB_DIV: u32 = 89;
pub const VOICE_MODE: u32 = 90;
pub const EFFECT_MIX: u32 = 91;
pub const MPE_MODE: u32 = 92;
pub const MPE_BEND: u32 = 93;
pub const BEND_DOWN: u32 = 94;
pub const ANALYSIS_SENS: u32 = 95;
pub const WAVETABLE: u32 = 96;
pub const TABLE_SOURCE: u32 = 97;
pub const TABLE_REVERSE: u32 = 98;
pub const USER_TABLE: u32 = 99;

pub static PARAMS: &[Param] = &[
    Param::new(0, "Polyphony", 1.0, 32.0, 16.0, Unit::None),
    Param::new(1, "Unison", 1.0, 4.0, 1.0, Unit::None),
    Param::new(2, "Transpose", -36.0, 36.0, 0.0, Unit::Semitones),
    Param::new(3, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(4, "Output Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::menu(5, "MIDI Mono Mode", MIDI_MODE, 0.0),
    Param::new(6, "Pitch Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(7, "Mono Mode Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(8, "Stereo Spread", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(9, "Voice Detune", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(10, "Velocity To", MOD_TARGETS, 1.0),
    Param::new(11, "Velocity Mod Amount", -1.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(12, "Mod Wheel To", MOD_TARGETS, 8.0),
    Param::new(13, "Mod Wheel Mod Amount", -1.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(14, "Aftertouch To", MOD_TARGETS, 1.0),
    Param::new(15, "Aftertouch Mod Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(16, "Ctrl X Source", 1.0, 127.0, 74.0, Unit::None),
    Param::menu(17, "Ctrl X To", MOD_TARGETS, 1.0),
    Param::new(18, "Ctrl X Mod Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(19, "Oscillator Type", ENGINES, 0.0),
    Param::new(20, "Vibrato Depth", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(21, "Glide/Autobend", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(22, "Glide/AB Type", GLIDE_TYPE, 0.0),
    Param::menu(23, "Glide/AB Mode", GLIDE_MODE, 0.0),
    Param::new(24, "Glide/AB Time", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::new(
        25,
        "Oscillator Shape by LFO/Env",
        -1.0,
        1.0,
        0.0,
        Unit::Ratio,
    ),
    Param::new(26, "Oscillator Mix", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(27, "Sine Level", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(28, "Oscillator 1 Waveform", 0.0, 3.0, 1.0, Unit::None),
    Param::new(29, "Oscillator 1 Pulse Width", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(30, "Oscillator 2 Waveform", 0.0, 3.0, 1.0, Unit::None),
    Param::new(31, "Oscillator 2 Pulse Width", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(
        32,
        "Oscillator 2 Semitone",
        -24.0,
        24.0,
        0.0,
        Unit::Semitones,
    ),
    Param::new(33, "Oscillator 2 Detune", -50.0, 50.0, 8.0, Unit::Cents),
    Param::new(34, "Filter State", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(35, "Filter Type", FILTERS, 0.0),
    Param::new(36, "Cutoff", 0.0, 1.0, 0.52, Unit::Ratio),
    Param::new(37, "Resonance", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(38, "Filter Keyscale", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(39, "Cutoff by LFO", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(40, "Cutoff by Env", -1.0, 1.0, 0.5, Unit::Ratio),
    Param::new(41, "Filter FM/Drive", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(42, "Effect State", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(43, "Effect Type", EFFECTS, 1.0),
    Param::new(44, "Effect Intensity", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(45, "Effect Rate", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(46, "LFO Waveform", LFO_WAVES, 1.0),
    Param::new(47, "LFO Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(48, "LFO Rate", 0.05, 30.0, 4.0, Unit::Hz),
    Param::menu(49, "LFO Via Source", VIA, 0.0),
    Param::new(50, "LFO Via Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(51, "Vibrato Waveform", LFO_WAVES, 0.0),
    Param::new(52, "Vibrato Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(53, "Vibrato Rate", 0.05, 20.0, 5.5, Unit::Hz),
    Param::menu(54, "Vibrato Via Source", VIA, 1.0),
    Param::new(55, "Vibrato Via Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(56, "Filter Env Attack", 0.0, 10.0, 0.0, Unit::Seconds),
    Param::new(57, "Filter Env Decay", 0.0, 10.0, 1.2, Unit::Seconds),
    Param::new(58, "Filter Env Sustain", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(59, "Filter Env Release", 0.0, 10.0, 0.05, Unit::Seconds),
    Param::new(60, "Filter Env Velocity", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(61, "Amp Env Attack", 0.0, 10.0, 0.003, Unit::Seconds),
    Param::new(62, "Amp Env Decay", 0.0, 10.0, 4.0, Unit::Seconds),
    Param::new(63, "Amp Env Sustain", 0.0, 1.0, 0.75, Unit::Ratio),
    Param::new(64, "Amp Env Release", 0.0, 10.0, 0.2, Unit::Seconds),
    Param::new(65, "Amp Env Velocity", 0.0, 1.0, 0.35, Unit::Ratio),
    // ---- added after Logic's list
    Param::new(OSC1_NOISE, "Oscillator 1 Noise", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(OSC2_NOISE, "Oscillator 2 Noise", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(FILTER_MODE, "Filter Mode", FILTER_MODES, 0.0),
    Param::menu(FILTER_VARIANT, "Filter Variant", FILTER_VARIANTS, 0.0),
    Param::menu(FILTER_SLOPE, "Filter Slope", FILTER_SLOPES, 3.0),
    Param::new(BR_AMOUNT, "Band Reject Amount", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(PEAK_GAIN, "Peak Gain", 0.0, 24.0, 12.0, Unit::Db),
    Param::new(PEAK_BW, "Peak Bandwidth", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(FILTER_FM, "Filter FM", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(CUTOFF_LFO2, "Cutoff by LFO (Bipolar)", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(FM_MODE, "FM/Harmonic Switch", FM_MODES, 0.0),
    Param::new(FM_HARMONIC, "FM Harmonic", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(FM_INHARMONIC, "FM Inharmonic", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(FM_SHAPE, "FM Shape", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(FM_MIX, "FM Mix", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(SYNC_AMOUNT, "Sync Amount", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::menu(TABLE_MODE, "Table Shape/Formant", TABLE_MODES, 0.0),
    Param::new(GLIDE_ON, "Glide/Autobend On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(GLIDE_TARGET, "Glide/AB Target", GLIDE_TARGETS, 2.0),
    Param::new(POLY_LFO, "Polyphonic LFO", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(LFO_VIA_SPEED, "LFO Via Controls Speed", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(VIB_VIA_SPEED, "Vibrato Via Controls Speed", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(LFO_DIV, "LFO Sync Division", SYNC_DIVS, 4.0),
    Param::menu(VIB_DIV, "Vibrato Sync Division", SYNC_DIVS, 5.0),
    Param::menu(VOICE_MODE, "Voice Mode", VOICE_MODES, 0.0),
    Param::new(EFFECT_MIX, "Effect Mix", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::menu(MPE_MODE, "MIDI Mono Mode (MPE)", MPE_MODES, 0.0),
    Param::new(MPE_BEND, "MPE Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones),
    Param::new(BEND_DOWN, "Pitch Bend Range Down", -1.0, 24.0, -1.0, Unit::Semitones),
    Param::new(ANALYSIS_SENS, "Audio File Analysis Sensitivity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(WAVETABLE, "Wavetable", APP_TABLES, 0.0),
    Param::menu(TABLE_SOURCE, "Wavetable Source", TABLE_SOURCES, 0.0),
    Param::new(TABLE_REVERSE, "Reverse Wavetable Order", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(USER_TABLE, "User Wavetable", 0.0, 255.0, 0.0, Unit::None).hide(),
];

/// Which modulation targets a controller list offers for an engine (indexes into MOD_TARGETS).
pub fn targets_for(engine: u8) -> Vec<usize> {
    let mut v: Vec<usize> = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    if engine >= 2 {
        // Table: formant stretch / wave variation; FM: harmonic
        v.push(10);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Wavetables (our own: additive spectra morphing from sine to bright formant/organ/bell shapes,
// plus user tables made from audio files), mip-mapped per octave so they stay alias-free.

const TBL_LEN: usize = 2048;
const TBL_FRAMES: usize = 16;
const TBL_MIPS: usize = 11;

#[derive(Debug)]
pub(crate) struct Tables {
    data: Vec<f32>,
    frames: usize,
}

/// Harmonic amplitudes (index = harmonic number, 0 unused) of frame `f` of a generated table.
type Spectrum = fn(x: f32, k: usize) -> f32;

fn spec_default(x: f32, k: usize) -> f32 {
    let kf = k as f32;
    let saw = 1.0 / kf;
    let square = if k % 2 == 1 { 1.0 / kf } else { 0.0 };
    let formant = (-((kf - 1.0 - 14.0 * x) / (2.0 + 4.0 * x)).powi(2)).exp() / kf.sqrt();
    let organ = if [1usize, 2, 3, 4, 6, 8, 12, 16].contains(&k) { 0.6 / (kf.sqrt()) } else { 0.0 };
    let sine = if k == 1 { 1.0 } else { 0.0 };
    // walk: sine → organ → square → saw → formant sweep
    if x < 0.25 {
        let t = x / 0.25;
        sine * (1.0 - t) + organ * t
    } else if x < 0.5 {
        let t = (x - 0.25) / 0.25;
        organ * (1.0 - t) + square * t
    } else if x < 0.75 {
        let t = (x - 0.5) / 0.25;
        square * (1.0 - t) + saw * t
    } else {
        let t = (x - 0.75) / 0.25;
        saw * (1.0 - t) + formant * t * 1.5
    }
}

fn spec_organ(x: f32, k: usize) -> f32 {
    // drawbars at these harmonics fade in one after another
    const BARS: [usize; 9] = [1, 2, 3, 4, 6, 8, 10, 12, 16];
    match BARS.iter().position(|&b| b == k) {
        Some(j) => ((x * 9.0 + 1.0 - j as f32).clamp(0.0, 1.0)) / (k as f32).sqrt(),
        None => 0.0,
    }
}

fn spec_vowels(x: f32, k: usize) -> f32 {
    // five vowels (first two formants in Hz) on a 110 Hz fundamental, morphing along the table
    const V: [(f32, f32); 5] = [(800.0, 1150.0), (400.0, 1600.0), (350.0, 2300.0), (450.0, 800.0), (325.0, 700.0)];
    let p = x * 4.0;
    let i = (p as usize).min(3);
    let t = p - i as f32;
    let f1 = V[i].0 + (V[i + 1].0 - V[i].0) * t;
    let f2 = V[i].1 + (V[i + 1].1 - V[i].1) * t;
    let hz = k as f32 * 110.0;
    let g = |c: f32, w: f32| (-((hz - c) / w).powi(2)).exp();
    (g(f1, 110.0) + 0.7 * g(f2, 180.0) + 0.05) / (k as f32).sqrt()
}

fn spec_pulse(x: f32, k: usize) -> f32 {
    let w = 0.5 - 0.46 * x;
    (std::f32::consts::PI * k as f32 * w).sin() / k as f32
}

fn spec_bell(x: f32, k: usize) -> f32 {
    let kf = k as f32;
    let peaks = (1.0 + (kf * (0.4 + 2.2 * x)).cos()).max(0.0);
    peaks / kf.powf(0.8)
}

fn spec_odd_even(x: f32, k: usize) -> f32 {
    let kf = k as f32;
    if k % 2 == 1 { 1.0 / kf } else { x / kf }
}

const SPECS: [Spectrum; 6] = [spec_default, spec_organ, spec_vowels, spec_pulse, spec_bell, spec_odd_even];

/// Largest harmonic that stays below 20 kHz for mip level `mip` (fundamentals up to 20 Hz * 2^mip).
fn mip_harmonics(mip: usize) -> usize {
    ((20000.0 / (20.0 * 2f32.powi(mip as i32))) as usize).clamp(1, 512)
}

impl Tables {
    fn from_spectrum(spec: Spectrum) -> Tables {
        let mut data = vec![0.0f32; TBL_FRAMES * TBL_MIPS * TBL_LEN];
        for f in 0..TBL_FRAMES {
            let x = f as f32 / (TBL_FRAMES - 1) as f32;
            let amps: Vec<f32> = (0..=512).map(|k| if k == 0 { 0.0 } else { spec(x, k) }).collect();
            for mip in 0..TBL_MIPS {
                let max_h = mip_harmonics(mip);
                let base = (f * TBL_MIPS + mip) * TBL_LEN;
                let mut peak = 0.0f32;
                for i in 0..TBL_LEN {
                    let ph = i as f64 / TBL_LEN as f64 * std::f64::consts::TAU;
                    // sin(k·ph) by the Chebyshev recurrence
                    let c2 = 2.0 * ph.cos();
                    let (mut s1, mut s2) = (ph.sin(), 0.0f64);
                    let mut acc = 0.0f64;
                    for &a in &amps[1..=max_h] {
                        acc += a as f64 * s1;
                        let s0 = c2 * s1 - s2;
                        s2 = s1;
                        s1 = s0;
                    }
                    data[base + i] = acc as f32;
                    peak = peak.max(acc.abs() as f32);
                }
                if peak > 0.0 {
                    for v in &mut data[base..base + TBL_LEN] {
                        *v /= peak;
                    }
                }
            }
        }
        Tables { data, frames: TBL_FRAMES }
    }

    /// A table from single-cycle frames (any length, at least two frames): each is resampled to
    /// 2048 points and band-limited per mip level through an FFT.
    pub(crate) fn from_cycles(frames: &[Vec<f32>]) -> Tables {
        let nf = frames.len().max(2);
        let mut data = vec![0.0f32; nf * TBL_MIPS * TBL_LEN];
        for f in 0..nf {
            let src = &frames[f.min(frames.len() - 1)];
            let cyc: Vec<f32> = (0..TBL_LEN)
                .map(|i| {
                    let x = i as f32 / TBL_LEN as f32 * src.len() as f32;
                    let a = x as usize % src.len();
                    let b = (a + 1) % src.len();
                    let t = x - x.floor();
                    src[a] + (src[b] - src[a]) * t
                })
                .collect();
            let mut re = cyc;
            let mut im = vec![0.0f32; TBL_LEN];
            fft(&mut re, &mut im, false);
            for mip in 0..TBL_MIPS {
                let max_h = mip_harmonics(mip).min(TBL_LEN / 2 - 1);
                let mut r2 = re.clone();
                let mut i2 = im.clone();
                r2[0] = 0.0;
                i2[0] = 0.0;
                for k in (max_h + 1)..=(TBL_LEN - max_h - 1) {
                    r2[k] = 0.0;
                    i2[k] = 0.0;
                }
                fft(&mut r2, &mut i2, true);
                let peak = r2.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                let base = (f * TBL_MIPS + mip) * TBL_LEN;
                for i in 0..TBL_LEN {
                    data[base + i] = if peak > 0.0 { r2[i] / peak } else { 0.0 };
                }
            }
        }
        Tables { data, frames: nf }
    }
}

/// In-place radix-2 FFT (length a power of two); `inverse` also divides by the length.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = std::f64::consts::TAU / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos() as f32, ang.sin() as f32);
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f32;
        for v in re.iter_mut() {
            *v *= s;
        }
        for v in im.iter_mut() {
            *v *= s;
        }
    }
}

static APP_TABLE_SLOTS: [OnceLock<Tables>; 6] = [const { OnceLock::new() }; 6];

pub(crate) fn tables() -> &'static Tables {
    APP_TABLE_SLOTS[0].get_or_init(|| Tables::from_spectrum(SPECS[0]))
}

/// One of the supplied wavetables, building it on first use (call from outside the audio thread).
pub fn warm_app_table(i: usize) {
    let i = i.min(SPECS.len() - 1);
    APP_TABLE_SLOTS[i].get_or_init(|| Tables::from_spectrum(SPECS[i]));
}

/// A supplied wavetable if it is already built (the audio thread never builds one).
pub(crate) fn app_table(i: usize) -> Option<&'static Tables> {
    APP_TABLE_SLOTS.get(i).and_then(|c| c.get())
}

#[inline]
pub(crate) fn table_read(pos: f32, phase: f32, hz: f32) -> f32 {
    read_tab(tables(), pos, phase, hz)
}

#[inline]
pub(crate) fn read_tab(t: &Tables, pos: f32, phase: f32, hz: f32) -> f32 {
    let mip = ((hz / 20.0).max(1.0).log2().ceil() as usize).min(TBL_MIPS - 1);
    let fp = pos.clamp(0.0, 1.0) * (t.frames - 1) as f32;
    let f0 = (fp as usize).min(t.frames - 2);
    let ff = fp - f0 as f32;
    let x = phase * TBL_LEN as f32;
    let i = (x as usize) % TBL_LEN;
    let fr = x - x.floor();
    let j = (i + 1) % TBL_LEN;
    let a0 = (f0 * TBL_MIPS + mip) * TBL_LEN;
    let a1 = ((f0 + 1) * TBL_MIPS + mip) * TBL_LEN;
    let s0 = t.data[a0 + i] + (t.data[a0 + j] - t.data[a0 + i]) * fr;
    let s1 = t.data[a1 + i] + (t.data[a1 + j] - t.data[a1 + i]) * fr;
    s0 + (s1 - s0) * ff
}

/// Custom wavetables: made from an audio file, kept in memory, saved under
/// ~/.config/bramble-studio/wavetables.
pub mod wavetable {
    use super::{fft, TBL_LEN};
    use std::path::PathBuf;

    /// The most entries one custom wavetable holds.
    pub const MAX_FRAMES: usize = 32;

    /// Where user wavetables live.
    pub fn folder() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".config"))).unwrap_or_else(|| PathBuf::from("."));
        cfg.join("bramble-studio").join("wavetables")
    }

    /// The saved user wavetables, sorted by name.
    pub fn list() -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(folder()).into_iter().flatten().flatten().filter_map(|e| {
            let p = e.path();
            (p.extension().is_some_and(|x| x == "bwt")).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
        }).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    fn clean(name: &str) -> String {
        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect::<String>().trim().to_string()
    }

    /// Saves the frames under `name`.
    pub fn save(name: &str, frames: &[Vec<f32>]) -> std::io::Result<()> {
        let name = clean(name);
        if name.is_empty() || frames.is_empty() {
            return Err(std::io::Error::other("no name or no frames"));
        }
        std::fs::create_dir_all(folder())?;
        let mut b = Vec::with_capacity(12 + frames.len() * TBL_LEN * 4);
        b.extend_from_slice(b"BWT1");
        b.extend_from_slice(&(frames.len() as u32).to_le_bytes());
        b.extend_from_slice(&(TBL_LEN as u32).to_le_bytes());
        for f in frames {
            for i in 0..TBL_LEN {
                let x = i as f32 / TBL_LEN as f32 * f.len() as f32;
                let a = x as usize % f.len();
                let c = (a + 1) % f.len();
                let t = x - x.floor();
                b.extend_from_slice(&(f[a] + (f[c] - f[a]) * t).to_le_bytes());
            }
        }
        std::fs::write(folder().join(format!("{name}.bwt")), b)
    }

    /// Loads a saved wavetable's frames.
    pub fn load(name: &str) -> Option<Vec<Vec<f32>>> {
        let b = std::fs::read(folder().join(format!("{}.bwt", clean(name)))).ok()?;
        if b.len() < 12 || &b[..4] != b"BWT1" {
            return None;
        }
        let n = u32::from_le_bytes(b[4..8].try_into().ok()?) as usize;
        let len = u32::from_le_bytes(b[8..12].try_into().ok()?) as usize;
        if len == 0 || n == 0 || b.len() < 12 + n * len * 4 {
            return None;
        }
        Some((0..n).map(|f| (0..len).map(|i| f32::from_le_bytes(b[12 + (f * len + i) * 4..16 + (f * len + i) * 4].try_into().unwrap())).collect()).collect())
    }

    /// Deletes a saved wavetable.
    pub fn delete(name: &str) -> bool {
        std::fs::remove_file(folder().join(format!("{}.bwt", clean(name)))).is_ok()
    }

    /// One sounding stretch of the audio (sample range).
    fn sections(x: &[f32], rate: f32) -> Vec<(usize, usize)> {
        let win = ((rate * 0.01) as usize).max(16);
        let rms: Vec<f32> = x.chunks(win).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
        let peak = rms.iter().fold(0.0f32, |m, v| m.max(*v));
        if peak < 1e-4 {
            return Vec::new();
        }
        let thr = peak * 0.03;
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        let mut quiet = 0;
        for (i, r) in rms.iter().enumerate() {
            if *r > thr {
                if start.is_none() {
                    start = Some(i);
                }
                quiet = 0;
            } else if let Some(s) = start {
                quiet += 1;
                // silence of 30 ms ends a section
                if quiet >= 3 {
                    out.push((s * win, (i + 1 - quiet) * win));
                    start = None;
                }
            }
        }
        if let Some(s) = start {
            out.push((s * win, x.len()));
        }
        out.retain(|(a, b)| b - a > (rate * 0.04) as usize);
        out
    }

    /// The period in samples of a roughly constant-pitch stretch (a difference-function search).
    fn period(x: &[f32], rate: f32) -> Option<f32> {
        let n = x.len().min(4096);
        let mid = (x.len() - n) / 2;
        let seg = &x[mid..mid + n];
        let lo = (rate / 2000.0) as usize;
        let hi = ((rate / 40.0) as usize).min(n / 2 - 1);
        if lo + 2 >= hi {
            return None;
        }
        let mut d = vec![0.0f32; hi + 2];
        for t in lo..=hi + 1 {
            let mut s = 0.0;
            for i in 0..n - t - 1 {
                let v = seg[i] - seg[i + t];
                s += v * v;
            }
            d[t] = s;
        }
        // cumulative mean normalisation, then the first dip below the threshold
        let mut run = 0.0f32;
        let mut norm = vec![1.0f32; hi + 2];
        for t in lo..=hi + 1 {
            run += d[t];
            norm[t] = d[t] * (t - lo + 1) as f32 / run.max(1e-12);
        }
        let mut best = lo;
        let mut found = false;
        for t in lo + 1..=hi {
            if norm[t] < 0.15 && norm[t] <= norm[t + 1] && norm[t] <= norm[t - 1] {
                best = t;
                found = true;
                break;
            }
        }
        if !found {
            best = (lo..=hi).min_by(|&a, &b| norm[a].total_cmp(&norm[b]))?;
        }
        let (a, b, c) = (norm[best - 1], norm[best], norm[best + 1]);
        let den = a - 2.0 * b + c;
        let off = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
        Some(best as f32 + off.clamp(-1.0, 1.0))
    }

    fn read(x: &[f32], pos: f32) -> f32 {
        let i = pos as usize;
        let t = pos - i as f32;
        let a = x.get(i).copied().unwrap_or(0.0);
        let b = x.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * t
    }

    fn magnitudes(cycle: &[f32]) -> Vec<f32> {
        let mut re = cycle.to_vec();
        let mut im = vec![0.0f32; TBL_LEN];
        fft(&mut re, &mut im, false);
        (1..=48).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).collect()
    }

    /// Builds wavetable frames from audio: finds the sounding sections (separated by silence),
    /// takes each section's pitch period, cuts it into single cycles and starts a new entry
    /// whenever the timbre changes by more than the sensitivity allows (0 = rarely, 1 = often).
    pub fn from_audio(samples: &[f32], rate: f32, sensitivity: f32) -> Result<Vec<Vec<f32>>, String> {
        let secs = sections(samples, rate);
        if secs.is_empty() {
            return Err("no sound found in the file".into());
        }
        let thr = 0.6 - 0.55 * sensitivity.clamp(0.0, 1.0);
        let mut frames: Vec<Vec<f32>> = Vec::new();
        for (a, b) in secs {
            let seg = &samples[a..b];
            let Some(p) = period(seg, rate) else { continue };
            let mut last: Option<Vec<f32>> = None;
            let mut pos = 0.0f32;
            while pos + 2.0 * p < seg.len() as f32 {
                // start each cycle at the next rising zero crossing
                let mut s = pos;
                let lim = pos + p;
                while s < lim && !(read(seg, s) <= 0.0 && read(seg, s + 1.0) > 0.0) {
                    s += 1.0;
                }
                if s >= lim {
                    s = pos;
                }
                let cyc: Vec<f32> = (0..TBL_LEN).map(|i| read(seg, s + i as f32 / TBL_LEN as f32 * p)).collect();
                let mean = cyc.iter().sum::<f32>() / TBL_LEN as f32;
                let mut cyc: Vec<f32> = cyc.iter().map(|v| v - mean).collect();
                let peak = cyc.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                if peak > 1e-6 {
                    for v in &mut cyc {
                        *v /= peak;
                    }
                    let keep = match &last {
                        None => true,
                        Some(l) => {
                            let (m1, m2) = (magnitudes(&cyc), magnitudes(l));
                            let num: f32 = m1.iter().zip(&m2).map(|(x, y)| (x - y) * (x - y)).sum();
                            let den: f32 = m2.iter().map(|y| y * y).sum();
                            (num / den.max(1e-9)).sqrt() > thr
                        }
                    };
                    if keep {
                        last = Some(cyc.clone());
                        frames.push(cyc);
                    }
                }
                pos += p;
            }
        }
        if frames.is_empty() {
            return Err("no constant-pitch section found".into());
        }
        if frames.len() == 1 {
            frames.push(frames[0].clone());
        }
        if frames.len() > MAX_FRAMES {
            let n = frames.len();
            frames = (0..MAX_FRAMES).map(|i| frames[i * (n - 1) / (MAX_FRAMES - 1)].clone()).collect();
        }
        Ok(frames)
    }
}

// ---------------------------------------------------------------------------------------------
// Filters: the legacy types (old behaviour) and the Logic filter family (mode, variant, slope).

#[derive(Clone, Copy, Debug)]
pub(crate) struct FiltCfg {
    /// 0 = the legacy Filter Type parameter, 1 LP, 2 HP, 3 BP, 4 BR, 5 Peak.
    pub mode: u8,
    pub variant: u8,
    pub slope: u8,
    pub legacy: u8,
    pub br_amount: f32,
    pub peak_db: f32,
    pub peak_bw: f32,
    pub drive: f32,
}

impl Default for FiltCfg {
    fn default() -> Self {
        FiltCfg { mode: 0, variant: 0, slope: 3, legacy: 0, br_amount: 1.0, peak_db: 12.0, peak_bw: 0.5, drive: 0.0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Filt {
    ladder: Ladder,
    svf: [Svf; 3],
    op: [dsp::OnePole; 2],
    bq: [dsp::Biquad; 2],
    cfg: FiltCfg,
    res: f32,
}

impl Filt {
    fn new() -> Filt {
        Filt { ladder: Ladder::new(), svf: [Svf::default(); 3], op: [dsp::OnePole::default(); 2], bq: [dsp::Biquad::default(); 2], cfg: FiltCfg::default(), res: 0.0 }
    }
    fn reset(&mut self) {
        self.ladder.reset();
        for s in &mut self.svf {
            s.reset();
        }
        for o in &mut self.op {
            o.z = 0.0;
        }
        for b in &mut self.bq {
            b.reset();
        }
    }
    /// Cutoff `fc` Hz and resonance 0..1.
    fn set(&mut self, cfg: &FiltCfg, fc: f32, res: f32, rate: f32) {
        self.cfg = *cfg;
        self.res = res;
        let q = 0.5 + res * 12.0;
        if cfg.mode == 0 {
            self.ladder.set(fc, res * 0.98, rate);
            self.ladder.poles = if cfg.legacy == 2 { 2 } else { 4 };
            self.ladder.edgy = cfg.legacy == 1;
            self.ladder.drive = 1.0 + cfg.drive * 3.0;
            self.svf[0].mode = match cfg.legacy {
                3 => SvfMode::Band,
                4 => SvfMode::High,
                5 => SvfMode::Peak,
                _ => SvfMode::Notch,
            };
            self.svf[0].set(fc, q, rate);
            return;
        }
        let slope = cfg.slope.min(3);
        let ladder_variant = matches!(cfg.variant, 0 | 2 | 4);
        match cfg.mode {
            1 if ladder_variant => {
                self.ladder.set(fc, res * 0.98, rate);
                self.ladder.poles = if slope <= 1 { 2 } else { 4 };
                self.ladder.edgy = cfg.variant == 2;
                let fat = if cfg.variant == 4 { 1.8 } else { 1.0 };
                self.ladder.drive = fat * (1.0 + cfg.drive * 3.0);
                self.op[0].lowpass_hz(fc, rate);
            }
            1 | 2 => {
                // chain of 12 dB stages; Creamy keeps the Q low on later stages, Sharp is a biquad
                let sharp = cfg.variant == 5;
                let hp = cfg.mode == 2;
                let mode = if hp { SvfMode::High } else { SvfMode::Low };
                for k in 0..2 {
                    let qk = if k == 0 { q } else { 0.707 };
                    self.svf[k].mode = mode;
                    self.svf[k].set(fc, qk, rate);
                    if sharp {
                        let qs = 0.6 + res * 14.0;
                        if hp { self.bq[k].highpass(fc, qs, rate) } else { self.bq[k].lowpass(fc, qs, rate) }
                    }
                }
                self.op[0].lowpass_hz(fc, rate);
            }
            3 => {
                // two-pole band pass from a high pass below and a low pass above the cutoff; the
                // resonance narrows the band
                let bw = 1.0 + (1.0 - res) * 3.0;
                self.svf[0].mode = SvfMode::High;
                self.svf[0].set(fc / bw, 0.707, rate);
                self.svf[1].mode = SvfMode::Low;
                self.svf[1].set((fc * bw).min(rate * 0.45), 0.707, rate);
                self.op[0].lowpass_hz(fc / bw, rate);
                self.op[1].lowpass_hz((fc * bw).min(rate * 0.45), rate);
            }
            4 => {
                self.svf[0].mode = SvfMode::Notch;
                self.svf[0].set(fc, 0.5 + res * 14.0, rate);
            }
            _ => {
                let qp = 0.5 * 24f32.powf(1.0 - cfg.peak_bw);
                self.bq[0].peaking(fc, qp, cfg.peak_db, rate);
            }
        }
    }
    /// Runs the filter; a time-varying biquad driven hard can blow up, so a runaway state is
    /// cleared instead of passed on.
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.run_inner(x);
        if y.is_finite() && y.abs() < 1.0e3 {
            y
        } else {
            self.reset();
            0.0
        }
    }
    #[inline]
    fn run_inner(&mut self, x: f32) -> f32 {
        let c = self.cfg;
        if c.mode == 0 {
            return match c.legacy {
                0..=2 => self.ladder.run(x),
                _ => self.svf[0].run(x),
            };
        }
        let slope = c.slope.min(3);
        // saturation of the input: Gritty heavily (more with resonance and drive), Edgy lightly
        let sat = |x: f32, v: u8, res: f32, drive: f32| match v {
            3 => dsp::fast_tanh(x * (1.5 + res * 3.0 + drive * 3.0)) / (1.0 + res),
            2 => dsp::fast_tanh(x * 1.3) / 1.3,
            4 => dsp::fast_tanh(x * (1.0 + drive * 2.0)),
            _ => x,
        };
        match c.mode {
            1 if matches!(c.variant, 0 | 2 | 4) => {
                if slope == 0 {
                    self.op[0].lp(x)
                } else {
                    self.ladder.run(x)
                }
            }
            1 | 2 => {
                let hp = c.mode == 2;
                let x = sat(x, c.variant, self.res, c.drive);
                if slope == 0 {
                    return if hp { self.op[0].hp(x) } else { self.op[0].lp(x) };
                }
                let stage = |s: &mut Filt, k: usize, v: f32| if c.variant == 5 { s.bq[k].run(v) } else { s.svf[k].run(v) };
                let mut y = stage(self, 0, x);
                if slope == 3 {
                    y = stage(self, 1, y);
                } else if slope == 2 {
                    y = if hp { self.op[0].hp(y) } else { self.op[0].lp(y) };
                }
                if c.variant == 3 {
                    y = dsp::fast_tanh(y * 1.2);
                }
                y
            }
            3 => {
                let x = sat(x, c.variant, self.res, c.drive);
                if slope == 0 {
                    let h = self.op[0].hp(x);
                    self.op[1].lp(h) * (1.0 + self.res * 2.0)
                } else {
                    let h = self.svf[0].run(x);
                    self.svf[1].run(h) * (1.0 + self.res * 2.0)
                }
            }
            4 => {
                let n = self.svf[0].run(x);
                x + (n - x) * c.br_amount
            }
            _ => self.bq[0].run(x),
        }
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RetroShared {
    rate: f32,
    engine: u8,
    unison: usize,
    detune: f32,
    spread: f32,
    transpose: f32,
    tune: f32,
    mix: f32,
    sine: f32,
    w1: f32,
    pw1: f32,
    w2: f32,
    pw2: f32,
    semi2: f32,
    det2: f32,
    shape_mod: f32,
    filter_on: bool,
    cutoff: f32,
    res: f32,
    keyscale: f32,
    cut_lfo: f32,
    cut_env: f32,
    filt: FiltCfg,
    fenv: [f32; 4],
    fenv_vel: f32,
    aenv: [f32; 4],
    aenv_vel: f32,
    vib_depth: f32,
    glide: f32,
    glide_time: f32,
    autobend: bool,
    vel_to: u8,
    vel_amt: f32,
    // ---- added with the extended parameters
    noise1: bool,
    noise2: bool,
    filter_fm: f32,
    fm_mode: u8,
    fm_harm: f32,
    fm_inh: f32,
    fm_shape: f32,
    fm_mix: f32,
    sync_amt: f32,
    formant: bool,
    /// Glide/Autobend: how much of the glide offset each part follows (osc 1 + sine, osc 2, filter).
    glide_f: [f32; 3],
    poly_lfo: bool,
    lfo_wave: u8,
    lfo_hz: f32,
    lfo_amt: f32,
    vib_wave: u8,
    vib_hz: f32,
    vib_amt: f32,
    double: bool,
    synced_start: bool,
    reverse: bool,
    tab: Option<&'static Tables>,
    start_chan: u8,
    // per-chunk global modulation (filled by the instrument)
    lfo: f32,
    vib: f32,
    mods: [f32; 11],
}

pub(crate) struct RetroVoice {
    on: bool,
    note: u8,
    chan: u8,
    mpe_bend: f32,
    vel: f32,
    pitch: f32,
    target: f32,
    glide_coef: f32,
    bend_off: f32,
    bend_coef: f32,
    o1: [Osc; 4],
    o2: [Osc; 4],
    sine: Osc,
    fm_ph: [f32; 4],
    car_ph: [f32; 4],
    ffm_ph: f32,
    filt: [Filt; 2],
    fenv: Adsr,
    aenv: Adsr,
    fade: Fade,
    killing: bool,
    pans: [(f32, f32); 4],
    level: f32,
    rng: Rng,
    lfo: Lfo,
    vib: Lfo,
}

impl RetroVoice {
    pub fn new(seed: u32) -> RetroVoice {
        RetroVoice {
            on: false,
            note: 60,
            chan: 0,
            mpe_bend: 0.0,
            vel: 1.0,
            pitch: 60.0,
            target: 60.0,
            glide_coef: 0.0,
            bend_off: 0.0,
            bend_coef: 0.0,
            o1: [Osc::default(); 4],
            o2: [Osc::default(); 4],
            sine: Osc::default(),
            fm_ph: [0.0; 4],
            car_ph: [0.0; 4],
            ffm_ph: 0.0,
            filt: [Filt::new(); 2],
            fenv: Adsr::default(),
            aenv: Adsr::default(),
            fade: Fade::default(),
            killing: false,
            pans: [(1.0, 1.0); 4],
            level: 0.0,
            rng: Rng::new(seed),
            lfo: Lfo::default(),
            vib: Lfo::default(),
        }
    }

    /// Mono/legato: move to a new note without retriggering.
    pub fn glide_to(&mut self, note: u8, sh: &RetroShared) {
        self.note = note;
        self.target = note as f32;
        if sh.glide_time <= 0.0 || sh.autobend {
            self.pitch = self.target;
        }
    }

    fn wave_of(w: f32) -> Wave {
        match w.round() as i32 {
            0 => Wave::Triangle,
            1 => Wave::Saw,
            _ => Wave::Pulse,
        }
    }
}

impl Voice<RetroShared> for RetroVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &RetroShared, _ctl: &Ctl) {
        let rate = sh.rate;
        let was_on = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.chan = sh.start_chan;
        self.mpe_bend = 0.0;
        self.vel = velocity.max(1) as f32 / 127.0;
        self.target = note as f32;
        if !(was_on && sh.glide_time > 0.0 && !sh.autobend) {
            self.pitch = self.target;
        }
        self.glide_coef = if sh.glide_time > 0.0 {
            dsp::tau_coef(sh.glide_time * 0.3, rate / CHUNK as f32)
        } else {
            0.0
        };
        if sh.autobend && sh.glide_time > 0.0 {
            self.bend_off = sh.glide * 12.0;
            self.bend_coef = dsp::tau_coef(sh.glide_time * 0.3, rate / CHUNK as f32);
        } else {
            self.bend_off = 0.0;
        }
        for u in 0..4 {
            // free-running analog-style phases, or all in step when the attack is faster than
            // 0.5 ms (synced oscillators)
            if sh.synced_start {
                self.o1[u].phase = 0.0;
                self.o2[u].phase = 0.0;
            } else {
                self.o1[u].phase = self.rng.unit();
                self.o2[u].phase = if sh.engine == 1 { self.o1[u].phase } else { self.rng.unit() };
            }
            self.fm_ph[u] = 0.0;
            self.car_ph[u] = 0.0;
            let p = if sh.unison > 1 {
                (u as f32 / (sh.unison - 1) as f32 * 2.0 - 1.0) * sh.spread
            } else {
                0.0
            };
            self.pans[u] = ((1.0 - p).sqrt(), (1.0 + p).sqrt());
        }
        self.ffm_ph = 0.0;
        if sh.poly_lfo {
            self.lfo = Lfo::default();
            self.vib = Lfo::default();
        }
        for f in &mut self.filt {
            f.reset();
        }
        let [a, d, s, r] = sh.fenv;
        self.fenv.set(a, d, s, r, rate);
        let [a, d, s, r] = sh.aenv;
        self.aenv.set(a, d, s, r, rate);
        if was_on {
            self.fenv.gate_on();
            self.aenv.gate_on();
        } else {
            self.fenv.trigger();
            self.aenv.trigger();
        }
    }
    fn release(&mut self, sh: &RetroShared, _ctl: &Ctl) {
        let [a, d, s, r] = sh.aenv;
        self.aenv.set(a, d, s, r, sh.rate);
        self.fenv.gate_off();
        self.aenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.003, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &RetroShared, ctl: &Ctl) {
        let rate = sh.rate;
        let n = l.len();
        // control rate
        self.pitch = self.target + (self.pitch - self.target) * self.glide_coef;
        self.bend_off *= self.bend_coef;
        // this voice's own LFO and vibrato (polyphonic), or the shared ones
        let (lfo_v, vib_v) = if sh.poly_lfo {
            let li = sh.lfo_hz * n as f32 / rate;
            let vi = sh.vib_hz * n as f32 / rate;
            (self.lfo.step(sh.lfo_wave, li, &mut self.rng) * sh.lfo_amt, self.vib.step(sh.vib_wave, vi, &mut self.rng) * sh.vib_amt)
        } else {
            (sh.lfo, sh.vib)
        };
        let m = &sh.mods;
        let velm = |t: usize| {
            if sh.vel_to as usize == t {
                sh.vel_amt * (self.vel - 0.5) * 2.0
            } else {
                0.0
            }
        };
        let vib = vib_v * (sh.vib_depth + m[8].max(0.0)) * 0.5 + m[6] * 2.0;
        // the glide / autobend offset, shared out between the oscillators
        let goff = (self.pitch - self.target) + self.bend_off;
        let pitch = self.target + sh.transpose + sh.tune / 100.0 + vib + ctl.bend + self.mpe_bend + velm(6);
        let hz = lg_dsp::midi_hz(pitch);
        let hz1 = hz * dsp::semis(goff * sh.glide_f[0]);
        let hz2 = hz * dsp::semis(goff * sh.glide_f[1]);
        // positive = by LFO, negative = by the filter envelope (FM depth envelope for EFM1-style use)
        let shape_src = if sh.shape_mod >= 0.0 {
            lfo_v * sh.shape_mod
        } else {
            -sh.shape_mod * self.fenv.level - 0.5 * -sh.shape_mod
        };
        let shape = (shape_src + m[4] + velm(4)).clamp(-1.0, 1.0);
        let wvar = m[10];
        let mix = (sh.mix + m[5] + velm(5)).clamp(0.0, 1.0);
        let uni = sh.unison.clamp(1, 4);
        // stereo placement of the unison voices, from the current settings (Unison and Spread
        // may change while a note sounds)
        for u in 0..4 {
            let p = if uni > 1 { ((u.min(uni - 1) as f32 / (uni - 1) as f32) * 2.0 - 1.0) * sh.spread } else { 0.0 };
            let p = p.clamp(-1.0, 1.0);
            self.pans[u] = ((1.0 - p).sqrt(), (1.0 + p).sqrt());
        }
        // filter
        let fe = self.fenv.level;
        let vel_f = 1.0 - sh.fenv_vel + sh.fenv_vel * self.vel;
        let key = (pitch - 60.0) / 12.0 * sh.keyscale;
        let cut_oct = sh.cutoff * 10.0
            + key
            + sh.cut_env * fe * 7.0 * vel_f
            + sh.cut_lfo * lfo_v * 3.0
            + m[1] * 4.0
            + velm(1) * 3.0
            + (ctl.brightness - 0.5) * 2.0
            + goff * sh.glide_f[2] / 12.0;
        let res = (sh.res + m[2] * 0.5 + velm(2)).clamp(0.0, 1.0);
        let stereo_f = uni > 1 || sh.spread > 0.0;
        let fm_amt = sh.filter_fm.abs() + m[9].max(0.0) * 0.5;
        let fm_env = sh.filter_fm > 0.0;
        let per_sample_fm = fm_amt > 0.0;
        let fc0 = (20.0 * 2f32.powf(cut_oct)).clamp(20.0, rate * 0.45);
        for ch in 0..2 {
            self.filt[ch].set(&sh.filt, fc0, res, rate);
        }
        let amp_vel = 1.0 - sh.aenv_vel + sh.aenv_vel * self.vel;
        let vol = (1.0 + m[3] + velm(3)).max(0.0) * amp_vel * 0.38 / (uni as f32).sqrt();
        let det_c = sh.detune * 30.0;
        // FM engine: modulator/carrier tuning ratio and index
        let steps = (sh.fm_harm * 15.0).round();
        let mut ratio_h = 1.0 + sh.w2.round() + steps + sh.fm_inh + sh.fm_shape * 2.0;
        if sh.fm_mode >= 1 {
            ratio_h += shape * 4.0;
        }
        ratio_h = (ratio_h + wvar * 6.0).max(0.25);
        let fm_index = if sh.fm_mode == 2 { mix } else { (mix + 0.5 * shape).clamp(0.0, 1.0) } * 5.0;
        let shp_k = 1.0 + sh.fm_shape * 6.0;
        let shp_n = dsp::fast_tanh(shp_k).max(0.1);
        let sync_ratio = 1.0 + ((1.0 + (sh.w2 / 3.0 + 0.5 * shape).clamp(0.0, 1.0) * 6.0 + sh.semi2.max(0.0) / 12.0) - 1.0) * sh.sync_amt;
        // wavetable: Shape mode reads the frame, Formant mode stretches the cycle
        let tab = sh.tab.unwrap_or_else(tables);
        let tpos = |w: f32| {
            let p = (w / 3.0 + 0.5 * shape + wvar * 0.5).clamp(0.0, 1.0);
            if sh.reverse { 1.0 - p } else { p }
        };
        let fstretch = if sh.formant { 2f32.powf((shape + wvar) * 2.0) } else { 1.0 };
        let fwin_mix = ((fstretch.ln().abs()) * 3.0).clamp(0.0, 1.0);
        let mut peak = 0.0f32;
        for i in 0..n {
            let mut sl = 0.0;
            let mut sr = 0.0;
            for u in 0..uni {
                let ud = if uni > 1 {
                    (u as f32 / (uni - 1) as f32 - 0.5) * det_c
                } else {
                    0.0
                };
                let f1 = hz1 * dsp::semis(ud / 100.0);
                let f2 = hz2 * dsp::semis(ud / 100.0 + sh.semi2 + sh.det2 / 100.0);
                let inc1 = f1 / rate;
                let inc2 = f2 / rate;
                let s = match sh.engine {
                    0 => {
                        let pw1 = (sh.pw1 + 0.4 * shape).clamp(0.05, 0.95);
                        let pw2 = (sh.pw2 + 0.4 * shape).clamp(0.05, 0.95);
                        let a = self.o1[u].next(Self::wave_of(sh.w1), inc1, if sh.w1 >= 2.5 { 0.5 } else { pw1 });
                        let b = self.o2[u].next(Self::wave_of(sh.w2), inc2, if sh.w2 >= 2.5 { 0.5 } else { pw2 });
                        let a = if sh.noise1 { self.rng.bipolar() * 0.7 } else { a };
                        let b = if sh.noise2 { self.rng.bipolar() * 0.7 } else { b };
                        a * (1.0 - mix) + b * mix
                    }
                    1 => {
                        // osc 2 is hard-synced to osc 1; shape sweeps the sync ratio, the Sync knob
                        // scales it
                        let a = self.o1[u].next(Self::wave_of(sh.w1), inc1, sh.pw1);
                        let wrapped = self.o1[u].phase < inc1;
                        if wrapped {
                            self.o2[u].phase = self.o1[u].phase * sync_ratio;
                        }
                        let b = self.o2[u].next(Wave::Saw, inc1 * sync_ratio, 0.5);
                        let a = if sh.noise1 { self.rng.bipolar() * 0.7 } else { a };
                        let b = if sh.noise2 { self.rng.bipolar() * 0.7 } else { b };
                        a * (1.0 - mix) + b * mix
                    }
                    2 => {
                        let o1 = &mut self.o1[u];
                        let o2 = &mut self.o2[u];
                        o1.advance(inc1);
                        o2.advance(inc2);
                        let rd = |ph: f32, f: f32, w: f32| {
                            if sh.formant {
                                let q = (ph * fstretch).fract();
                                let win = 0.5 * (1.0 - dsp::sin_cycles(ph + 0.25));
                                let g = 1.0 + (win - 1.0) * fwin_mix;
                                read_tab(tab, if sh.reverse { 1.0 - (w / 3.0).clamp(0.0, 1.0) } else { (w / 3.0).clamp(0.0, 1.0) }, q, f * fstretch) * g
                            } else {
                                read_tab(tab, tpos(w), ph, f)
                            }
                        };
                        let a = rd(o1.phase, f1, sh.w1);
                        let b = rd(o2.phase, f2, sh.w2);
                        a * (1.0 - mix) + b * mix
                    }
                    _ => {
                        // 2-operator FM: osc 2 modulates osc 1; mix = FM amount, osc 1 waveform =
                        // carrier ratio, harmonic/inharmonic/shape set the modulator's tuning
                        let ratio_c = 1.0 + sh.w1.round();
                        self.fm_ph[u] = (self.fm_ph[u] + inc2 * ratio_h).fract();
                        self.car_ph[u] = (self.car_ph[u] + inc1 * ratio_c).fract();
                        let ms = dsp::sin_cycles(self.fm_ph[u]);
                        let shaped = if sh.fm_shape > 0.0 { dsp::fast_tanh(ms * shp_k) / shp_n } else { ms };
                        let modu = shaped * fm_index;
                        let car = dsp::sin_cycles(self.car_ph[u] + modu / std::f32::consts::TAU) * 0.5;
                        if sh.fm_mix > 0.0 {
                            car + (shaped * 0.5 - car) * sh.fm_mix
                        } else {
                            car
                        }
                    }
                };
                sl += s * self.pans[u].0;
                sr += s * self.pans[u].1;
            }
            if sh.sine > 0.0 {
                let s = self.sine.next(Wave::Sine, hz1 / rate, 0.5) * sh.sine;
                sl += s;
                sr += s;
            }
            let _ = self.fenv.next();
            let a = self.aenv.next();
            let (mut yl, mut yr) = (sl, sr);
            if sh.filter_on {
                if per_sample_fm {
                    // Osc 1's sine frequency-modulates the cutoff (static, or scaled by the
                    // filter envelope)
                    self.ffm_ph = (self.ffm_ph + hz1 / rate).fract();
                    let depth = fm_amt * 4.0 * if fm_env { fe } else { 1.0 };
                    let fc = (fc0 * 2f32.powf(dsp::sin_cycles(self.ffm_ph) * depth)).clamp(20.0, rate * 0.45);
                    self.filt[0].set(&sh.filt, fc, res, rate);
                    if stereo_f {
                        self.filt[1].set(&sh.filt, fc, res, rate);
                    }
                }
                yl = self.filt[0].run(yl);
                yr = if stereo_f { self.filt[1].run(yr) } else { yl };
            }
            let mut g = a * vol;
            if self.killing {
                g *= self.fade.next();
            }
            yl *= g;
            yr *= g;
            // a soft ceiling for extreme settings (peak boost with high drive and resonance)
            if yl.abs() > 2.0 {
                yl = dsp::fast_tanh(yl * 0.25) * 4.0;
            }
            if yr.abs() > 2.0 {
                yr = dsp::fast_tanh(yr * 0.25) * 4.0;
            }
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.aenv.idle() || (self.killing && self.fade.gain <= 0.0) {
            self.on = false;
        }
    }
    fn active(&self) -> bool {
        self.on
    }
    fn level(&self) -> f32 {
        self.level
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lfo {
    phase: f32,
    hold: f32,
    last: f32,
    target: f32,
}

impl Lfo {
    pub(crate) fn step(&mut self, wave: u8, inc: f32, rng: &mut Rng) -> f32 {
        let p = self.phase;
        let wrapped = {
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                true
            } else {
                false
            }
        };
        match wave {
            0 => dsp::sin_cycles(p),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 2.0 * p - 1.0,
            3 => 1.0 - 2.0 * p,
            4 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            5 => {
                if wrapped {
                    self.hold = rng.bipolar();
                }
                self.hold
            }
            _ => {
                if wrapped {
                    self.last = self.target;
                    self.target = rng.bipolar();
                }
                dsp::lerp(self.last, self.target, p)
            }
        }
    }
}

pub struct RetroSynth {
    params: Params,
    poly: Poly<RetroVoice>,
    ch: Channel,
    sh: RetroShared,
    rate: f32,
    lfo: Lfo,
    vib: Lfo,
    rng: Rng,
    chorus: Chorus,
    flanger: Flanger,
    stack: [u8; 16],
    nstack: usize,
    mono_note: Option<u8>,
    ctrl_x: f32,
    last_vel: f32,
    out: f32,
    bpm: f64,
    /// The custom wavetable in memory (made from an audio file or loaded from the user's folder).
    custom: Option<&'static Tables>,
    custom_frames: Option<Vec<Vec<f32>>>,
    pub custom_name: String,
    user: Option<&'static Tables>,
    user_key: Option<(usize, String)>,
}

/// Leaks a finished table (a few MB at most per wavetable the user loads; the audio thread
/// then holds a plain reference).
fn leak(t: Tables) -> &'static Tables {
    Box::leak(Box::new(t))
}

impl RetroSynth {
    pub fn new(rate: f32) -> RetroSynth {
        dsp::warm_tables();
        let _ = tables();
        let mut seed = 3;
        let mut s = RetroSynth {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 17;
                RetroVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: RetroShared {
                rate,
                ..Default::default()
            },
            rate,
            lfo: Lfo::default(),
            vib: Lfo::default(),
            rng: Rng::new(99),
            chorus: Chorus::new(rate),
            flanger: Flanger::new(rate),
            stack: [0; 16],
            nstack: 0,
            mono_note: None,
            ctrl_x: 0.0,
            last_vel: 0.8,
            out: 1.0,
            bpm: 120.0,
            custom: None,
            custom_frames: None,
            custom_name: String::new(),
            user: None,
            user_key: None,
        };
        s.update();
        s
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    /// Poly / Mono / Legato; the MPE mono mode plays one voice per channel, so it is poly here.
    fn mono_mode(&self) -> u8 {
        if self.p(MPE_MODE) >= 0.5 { 0 } else { self.p(5).round() as u8 }
    }
    /// The MIDI channel (0-based) that carries the messages for all voices in MPE mono mode.
    fn base_channel(&self) -> u8 {
        if self.p(MPE_MODE) >= 1.5 { 15 } else { 0 }
    }
    fn update(&mut self) {
        let mono = self.mono_mode();
        let unison = self.p(1).round().clamp(1.0, 4.0) as usize;
        let double = self.p(VOICE_MODE) >= 0.5;
        let poly = (self.p(0).round() as usize).clamp(1, 32);
        self.poly.set_max(if mono > 0 { 1 } else if double { (poly / 2).max(1) } else { poly });
        self.poly.retrigger_kills = true;
        let src = self.p(TABLE_SOURCE).round() as u8;
        let tab = match src {
            1 => self.custom,
            2 => self.user,
            _ => app_table(self.p(WAVETABLE).round() as usize),
        };
        let sh = &mut self.sh;
        sh.engine = self.params.get(19).round() as u8;
        sh.double = double;
        // Double mode plays a detuned pair per note: detune and spread apply to the pairs
        sh.unison = if double { (unison * 2).min(4) } else { unison };
        sh.detune = self.params.get(9);
        sh.spread = self.params.get(8);
        sh.transpose = self.params.get(2);
        sh.tune = self.params.get(3);
        sh.mix = self.params.get(26);
        sh.sine = self.params.get(27);
        sh.w1 = self.params.get(28);
        sh.pw1 = self.params.get(29);
        sh.w2 = self.params.get(30);
        sh.pw2 = self.params.get(31);
        sh.semi2 = self.params.get(32);
        sh.det2 = self.params.get(33);
        sh.shape_mod = self.params.get(25);
        sh.filter_on = self.params.get(34) >= 0.5;
        sh.filt = FiltCfg {
            mode: self.params.get(FILTER_MODE).round() as u8,
            variant: self.params.get(FILTER_VARIANT).round() as u8,
            slope: self.params.get(FILTER_SLOPE).round() as u8,
            legacy: self.params.get(35).round() as u8,
            br_amount: self.params.get(BR_AMOUNT),
            peak_db: self.params.get(PEAK_GAIN),
            peak_bw: self.params.get(PEAK_BW),
            drive: self.params.get(41),
        };
        sh.cutoff = self.params.get(36);
        sh.res = self.params.get(37);
        sh.keyscale = self.params.get(38);
        sh.cut_lfo = self.params.get(39) + self.params.get(CUTOFF_LFO2);
        sh.cut_env = self.params.get(40);
        sh.fenv = [self.params.get(56), self.params.get(57), self.params.get(58), self.params.get(59)];
        sh.fenv_vel = self.params.get(60);
        sh.aenv = [self.params.get(61), self.params.get(62), self.params.get(63), self.params.get(64)];
        sh.aenv_vel = self.params.get(65);
        // an attack faster than 0.5 ms starts the oscillators in step (synced mode)
        sh.synced_start = sh.aenv[0] < 0.0005;
        sh.vib_depth = self.params.get(20);
        let glide_on = self.params.get(GLIDE_ON) >= 0.5;
        sh.glide = if glide_on { self.params.get(21) } else { 0.0 };
        sh.glide_time = if glide_on {
            self.params.get(24) * self.params.get(21).abs().max(if self.params.get(22) < 0.5 { 1.0 } else { 0.0 })
        } else {
            0.0
        };
        sh.autobend = self.params.get(22) >= 0.5;
        sh.glide_f = match self.params.get(GLIDE_TARGET).round() as u8 {
            0 => [1.0, 0.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            3 => [1.0, -1.0, 0.0],
            4 => [1.0, 1.0, 1.0],
            _ => [1.0, 1.0, 0.0],
        };
        sh.vel_to = self.params.get(10).round() as u8;
        sh.vel_amt = self.params.get(11);
        sh.noise1 = self.params.get(OSC1_NOISE) >= 0.5;
        sh.noise2 = self.params.get(OSC2_NOISE) >= 0.5;
        sh.filter_fm = self.params.get(FILTER_FM);
        sh.fm_mode = self.params.get(FM_MODE).round() as u8;
        sh.fm_harm = self.params.get(FM_HARMONIC);
        sh.fm_inh = self.params.get(FM_INHARMONIC);
        sh.fm_shape = self.params.get(FM_SHAPE);
        sh.fm_mix = self.params.get(FM_MIX);
        sh.sync_amt = self.params.get(SYNC_AMOUNT);
        sh.formant = self.params.get(TABLE_MODE) >= 0.5;
        sh.poly_lfo = self.params.get(POLY_LFO) >= 0.5;
        sh.lfo_wave = self.params.get(46).round() as u8;
        sh.vib_wave = self.params.get(51).round() as u8;
        sh.reverse = self.params.get(TABLE_REVERSE) >= 0.5;
        sh.tab = tab;
        let bend = if mono > 0 { self.params.get(7) } else { self.params.get(6) };
        self.ch.bend_up = bend;
        let down = self.params.get(BEND_DOWN);
        self.ch.bend_down = if down >= 0.0 { down } else { bend };
        self.ch.update_bend();
        self.out = lg_dsp::db_to_gain(self.params.get(4));
    }

    fn via(&self, src: u8) -> f32 {
        match src {
            1 => self.ch.ctl.mod_wheel,
            2 => self.ch.ctl.aftertouch,
            3 => self.last_vel,
            4 => self.ctrl_x,
            5 => self.ch.ctl.mod_wheel.max(self.ch.ctl.aftertouch),
            _ => 0.0,
        }
    }

    fn compute_mods(&mut self) {
        let mut m = [0.0f32; 11];
        let route = |m: &mut [f32; 11], t: f32, v: f32| {
            let t = t.round() as usize;
            if t > 0 && t < 11 {
                m[t] += v;
            }
        };
        route(&mut m, self.p(12), self.ch.ctl.mod_wheel * self.p(13));
        route(&mut m, self.p(14), self.ch.ctl.aftertouch * self.p(15));
        route(&mut m, self.p(17), self.ctrl_x * self.p(18));
        self.sh.mods = m;
    }

    // ---- custom wavetables

    /// Makes the custom wavetable from audio samples (constant-pitch sections found by the
    /// analysis); returns the number of entries.
    pub fn wavetable_from_samples(&mut self, samples: &[f32], rate: f32) -> Result<usize, String> {
        let frames = wavetable::from_audio(samples, rate, self.p(ANALYSIS_SENS))?;
        let n = frames.len();
        self.set_custom(frames, "Custom");
        Ok(n)
    }
    /// Create Wavetable from Audiofile: reads the file and builds the custom wavetable.
    pub fn wavetable_from_file(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let a = crate::wav::read_audio(path).map_err(|e| e.to_string())?;
        let ch0 = a.channels.first().ok_or("empty file")?;
        let n = self.wavetable_from_samples(ch0, a.rate as f32)?;
        if let Some(stem) = path.file_stem() {
            self.custom_name = stem.to_string_lossy().into_owned();
        }
        Ok(n)
    }
    /// Puts frames in the custom slot and switches the Table oscillator to it.
    pub fn set_custom(&mut self, frames: Vec<Vec<f32>>, name: &str) {
        self.custom = Some(leak(Tables::from_cycles(&frames)));
        self.custom_frames = Some(frames);
        self.custom_name = name.to_string();
        self.params.set(TABLE_SOURCE, 1.0);
        self.update();
    }
    /// The custom wavetable's entries (single cycles), when there is one.
    pub fn custom_frames(&self) -> Option<&[Vec<f32>]> {
        self.custom_frames.as_deref()
    }
    /// Save Wavetable as…: stores the custom wavetable under a name.
    pub fn save_custom(&mut self, name: &str) -> Result<(), String> {
        let f = self.custom_frames.as_ref().ok_or("no custom wavetable")?;
        wavetable::save(name, f).map_err(|e| e.to_string())?;
        self.custom_name = name.to_string();
        Ok(())
    }
    /// Loads a saved user wavetable into the custom slot (Custom Wavetable / its name in the menu).
    pub fn load_user(&mut self, name: &str) -> Result<usize, String> {
        let f = wavetable::load(name).ok_or("wavetable not found")?;
        let n = f.len();
        self.set_custom(f, name);
        Ok(n)
    }
    /// Delete Selected User Wavetable: removes the file and drops the table when it is the one loaded.
    pub fn delete_user(&mut self, name: &str) -> bool {
        let ok = wavetable::delete(name);
        if ok && self.custom_name == name {
            self.custom = None;
            self.custom_frames = None;
            self.custom_name.clear();
            self.params.set(TABLE_SOURCE, 0.0);
            self.update();
        }
        ok
    }
    /// Re-reads the user wavetable the User Wavetable parameter names (for the saved state).
    fn refresh_user(&mut self) {
        if self.p(TABLE_SOURCE).round() as u8 != 2 {
            return;
        }
        let list = wavetable::list();
        let idx = self.p(USER_TABLE).round() as usize;
        let Some(name) = list.get(idx).cloned() else { return };
        if self.user_key.as_ref() == Some(&(idx, name.clone())) {
            return;
        }
        if let Some(f) = wavetable::load(&name) {
            self.user = Some(leak(Tables::from_cycles(&f)));
            self.user_key = Some((idx, name));
        }
    }

    /// One note's pitch bend on an MPE member channel (the base channel bends every voice).
    pub fn pitch_bend_ch(&mut self, channel: u8, value: i16) {
        if self.p(MPE_MODE) < 0.5 || channel == self.base_channel() {
            return self.pitch_bend(value);
        }
        let b = value as f32 / 8192.0 * self.p(MPE_BEND);
        for v in self.poly.voices.iter_mut().filter(|v| v.on && v.chan == channel) {
            v.mpe_bend = b;
        }
    }

    fn sync_hz(&self, div: u32) -> f32 {
        let beats = SYNC_BEATS[(self.p(div).round() as usize).min(SYNC_BEATS.len() - 1)];
        (self.bpm as f32 / 60.0) / beats
    }
}

impl Plugin for RetroSynth {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            if id == WAVETABLE {
                warm_app_table(self.params.get(WAVETABLE).round() as usize);
            }
            if id == TABLE_SOURCE || id == USER_TABLE {
                self.refresh_user();
            }
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.nstack = 0;
        self.mono_note = None;
        self.chorus.reset();
        self.flanger.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.p(64) + 0.3
    }
}

impl Instrument for RetroSynth {
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
    fn set_transport(&mut self, t: lg_dsp::Transport) {
        if t.bpm > 0.0 {
            self.bpm = t.bpm;
        }
    }
    fn pressure(&mut self, value: u8) {
        self.ch.ctl.aftertouch = value as f32 / 127.0;
    }
    fn note_on_ch(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.sh.start_chan = channel;
        self.note_on(pitch, velocity);
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.last_vel = velocity as f32 / 127.0;
        let mono = self.mono_mode();
        if mono == 0 {
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
            return;
        }
        // note stack for mono/legato
        if self.nstack < self.stack.len() {
            self.stack[self.nstack] = pitch;
            self.nstack += 1;
        }
        let legato = mono == 2 || self.p(23) >= 0.5;
        if let (true, Some(_)) = (legato && self.nstack > 1, self.mono_note) {
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(pitch, &self.sh);
                return;
            }
        }
        if let Some(n) = self.mono_note {
            // retrigger: keep the same voice so glide works from the last pitch
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                let sh = self.sh;
                v.start(pitch, velocity, &sh, &self.ch.ctl);
                v.note = pitch;
                let _ = n;
                return;
            }
        }
        self.mono_note = Some(pitch);
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off_ch(&mut self, _channel: u8, pitch: u8) {
        self.note_off(pitch);
    }
    fn note_off(&mut self, pitch: u8) {
        let mono = self.mono_mode();
        if mono == 0 {
            self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
            return;
        }
        if let Some(i) = self.stack[..self.nstack].iter().position(|&n| n == pitch) {
            for j in i..self.nstack - 1 {
                self.stack[j] = self.stack[j + 1];
            }
            self.nstack -= 1;
        }
        if self.nstack > 0 {
            let top = self.stack[self.nstack - 1];
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(top, &self.sh);
            }
        } else if let Some(n) = self.mono_note.take() {
            self.poly.note_off(n, &self.sh, &self.ch.ctl);
            // the voice may have moved notes; release whatever is sounding
            let sh = self.sh;
            for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                v.release(&sh, &self.ch.ctl);
            }
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.nstack = 0;
            self.mono_note = None;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        if cc == self.p(16).round() as u8 {
            self.ctrl_x = value as f32 / 127.0;
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let mut dl = [0.0f32; CHUNK];
        let mut dr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let fx_on = self.p(42) >= 0.5;
        let fx_type = self.p(43).round() as u8;
        let fx_int = self.p(44);
        let fx_mix = self.p(EFFECT_MIX);
        let fx_rate = 0.05 * 100f32.powf(self.p(45));
        let lfo_sync = self.p(47) >= 0.5;
        let vib_sync = self.p(52) >= 0.5;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.compute_mods();
            // LFO: tempo-synced or free; Via sets the level, or the speed when Sync is off
            let via_l = self.via(self.p(49).round() as u8);
            let lfo_speed_via = self.p(LFO_VIA_SPEED) >= 0.5 && !lfo_sync && self.p(49) >= 0.5;
            let mut lfo_hz = if lfo_sync { self.sync_hz(LFO_DIV) } else { self.p(48) };
            let lfo_amt = if lfo_speed_via {
                lfo_hz *= 2f32.powf(via_l * self.p(50) * 5.0);
                1.0
            } else if self.p(49) < 0.5 {
                1.0
            } else {
                (1.0 + self.p(50) * (via_l * 2.0 - 1.0)).clamp(0.0, 2.0) * 0.5
            } + self.sh.mods[7];
            let via_v = self.via(self.p(54).round() as u8);
            let vib_speed_via = self.p(VIB_VIA_SPEED) >= 0.5 && !vib_sync && self.p(54) >= 0.5;
            let mut vib_hz = if vib_sync { self.sync_hz(VIB_DIV) } else { self.p(53) };
            let vib_amt = if vib_speed_via {
                vib_hz *= 2f32.powf(via_v * self.p(55) * 5.0);
                1.0
            } else if self.p(54) < 0.5 {
                1.0
            } else {
                via_v * (0.5 + self.p(55).max(0.0))
            };
            self.sh.lfo_hz = lfo_hz;
            self.sh.lfo_amt = lfo_amt;
            self.sh.vib_hz = vib_hz;
            self.sh.vib_amt = vib_amt;
            if !self.sh.poly_lfo {
                let inc = lfo_hz * m as f32 / self.rate;
                self.sh.lfo = self.lfo.step(self.sh.lfo_wave, inc, &mut self.rng) * lfo_amt;
                let vinc = vib_hz * m as f32 / self.rate;
                self.sh.vib = self.vib.step(self.sh.vib_wave, vinc, &mut self.rng) * vib_amt;
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if fx_on && fx_int > 0.0 && fx_mix > 0.0 {
                dl[..m].copy_from_slice(&sl[..m]);
                dr[..m].copy_from_slice(&sr[..m]);
                if fx_type == 0 {
                    self.chorus
                        .process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int);
                } else {
                    self.flanger
                        .process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int * 0.3);
                }
                if fx_mix < 1.0 {
                    for i in 0..m {
                        sl[i] = dl[i] + (sl[i] - dl[i]) * fx_mix;
                        sr[i] = dr[i] + (sr[i] - dr[i]) * fx_mix;
                    }
                }
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * self.out;
                right[pos + i] += sr[i] * gr * self.out;
            }
            pos += m;
        }
    }
}
