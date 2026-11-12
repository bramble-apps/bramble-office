//! EVOC 20 PolySynth stand-in: a vocoder whose carrier is a polyphonic synthesizer played from
//! MIDI, and whose analysis input is a side-chain source (the host hands each block over through
//! `Instrument::side_chain`).
//!
//! - Analysis: up to 20 band-pass filters spread evenly (on a log axis) between Low and High
//!   Frequency, each followed by an envelope follower (Attack / Release in ms, Freeze holds the
//!   spectrum). Lowest can be a high pass, Highest a low pass.
//! - Synthesis: the same number of bands over the synth's signal; Formant Stretch changes their
//!   spread and widths, Formant Shift moves them (the Shift LFO sweeps it). Each band's level
//!   follows its analysis partner, so the output only holds frequencies present in both signals.
//! - U/V detection: sibilant, noisy parts of the analysis are spotted by their high-frequency
//!   share (Sensitivity) and replaced by noise, noise plus synth, or the high-passed analysis
//!   (Blend), at the U/V Level.
//! - The synth: dual oscillators (50 digital waveforms, Semi, Detune, Balance) or FM (sine
//!   carrier, wave-2 modulator, ratio in semitones and cents, Int), a noise generator (Color white
//!   to blue), tuning (Tune, Analog drift per note, Glide, Bend Range), a low pass before the
//!   formant banks, Attack / Release, Poly / Mono / Legato, Voices, Unison.
//! - Modulation: Pitch LFO (vibrato, intensity by mod wheel between a minimum and a maximum) and
//!   Shift LFO, each free in Hz or synced to the tempo; seven waveforms.
//! - Output: Signal (Voc / Syn / Ana), Level, Ensemble I / II, Stereo Width (bands spread
//!   ascending or alternating across the field).
//!
//! Ids are in the order of the guide's sections; Logic's float block is not decoded.

use crate::dsp::{self, Adsr, CHUNK, Chorus, Fade, Rng, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::retro::Lfo;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MAX_BANDS: usize = 20;

pub const UV_MODES: &[&str] = &["Noise", "Noise + Synth", "Blend"];
pub const OSC_MODES: &[&str] = &["Dual", "FM"];
pub const OCTAVES: &[&str] = &["16'", "8'", "4'"];
pub const VOICE_MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const LOWEST: &[&str] = &["Bandpass", "Highpass"];
pub const HIGHEST: &[&str] = &["Bandpass", "Lowpass"];
pub const LFO_WAVES: &[&str] = &["Triangle", "Saw Down", "Saw Up", "Square", "Square (Unipolar)", "Sample & Hold", "Smooth Random"];
pub const SIGNALS: &[&str] = &["Voc", "Syn", "Ana"];
pub const ENSEMBLES: &[&str] = &["Off", "I", "II"];
/// Tempo divisions of a synced LFO (left half of the Rate control) and their length in beats.
pub const SYNC_DIVS: &[&str] = &["8 Bars", "4 Bars", "2 Bars", "1 Bar", "1/2", "1/4", "1/8", "1/16", "1/32", "1/4 T", "1/8 T", "1/16 T"];
pub const SYNC_BEATS: [f32; 12] = [32.0, 16.0, 8.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.125, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0];
/// The number of digital waveforms for the oscillators.
pub const WAVES: usize = 50;

pub const ATTACK: u32 = 0;
pub const RELEASE: u32 = 1;
pub const FREEZE: u32 = 2;
pub const BANDS: u32 = 3;
pub const UV_SENS: u32 = 4;
pub const UV_MODE: u32 = 5;
pub const UV_LEVEL: u32 = 6;
pub const OSC_MODE: u32 = 7;
pub const OCTAVE: u32 = 8;
pub const WAVE1: u32 = 9;
pub const WAVE2: u32 = 10;
pub const SEMI: u32 = 11;
pub const DETUNE: u32 = 12;
pub const BALANCE: u32 = 13;
pub const FM_COARSE: u32 = 14;
pub const FM_FINE: u32 = 15;
pub const FM_INT: u32 = 16;
pub const NOISE_LEVEL: u32 = 17;
pub const NOISE_COLOR: u32 = 18;
pub const TUNE: u32 = 19;
pub const ANALOG: u32 = 20;
pub const GLIDE: u32 = 21;
pub const BEND: u32 = 22;
pub const CUTOFF: u32 = 23;
pub const FILTER_RES: u32 = 24;
pub const ENV_ATTACK: u32 = 25;
pub const ENV_RELEASE: u32 = 26;
pub const VOICE_MODE: u32 = 27;
pub const VOICES: u32 = 28;
pub const UNISON: u32 = 29;
pub const LOW_FREQ: u32 = 30;
pub const HIGH_FREQ: u32 = 31;
pub const LOWEST_MODE: u32 = 32;
pub const HIGHEST_MODE: u32 = 33;
pub const RESONANCE: u32 = 34;
pub const STRETCH: u32 = 35;
pub const SHIFT: u32 = 36;
pub const PITCH_RATE: u32 = 37;
pub const SHIFT_RATE: u32 = 38;
pub const PITCH_WAVE: u32 = 39;
pub const SHIFT_WAVE: u32 = 40;
pub const PITCH_INT_MIN: u32 = 41;
pub const PITCH_INT_MAX: u32 = 42;
pub const SHIFT_INT: u32 = 43;
pub const SIGNAL: u32 = 44;
pub const LEVEL: u32 = 45;
pub const ENSEMBLE: u32 = 46;
pub const WIDTH: u32 = 47;

pub static PARAMS: &[Param] = &[
    Param::new(ATTACK, "Analysis Attack", 0.1, 100.0, 5.0, Unit::Ms),
    Param::new(RELEASE, "Analysis Release", 1.0, 1000.0, 40.0, Unit::Ms),
    Param::new(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(BANDS, "Bands", 1.0, 20.0, 20.0, Unit::None),
    Param::new(UV_SENS, "U/V Sensitivity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(UV_MODE, "U/V Mode", UV_MODES, 0.0),
    Param::new(UV_LEVEL, "U/V Level", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(OSC_MODE, "Oscillator Mode", OSC_MODES, 0.0),
    Param::menu(OCTAVE, "Octave", OCTAVES, 1.0),
    Param::new(WAVE1, "Wave 1", 0.0, 49.0, 1.0, Unit::None),
    Param::new(WAVE2, "Wave 2", 0.0, 49.0, 30.0, Unit::None),
    Param::new(SEMI, "Semi", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(DETUNE, "Detune", -100.0, 100.0, 8.0, Unit::Cents),
    Param::new(BALANCE, "Balance", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(FM_COARSE, "FM Ratio Coarse", -24.0, 48.0, 12.0, Unit::Semitones),
    Param::new(FM_FINE, "FM Ratio Fine", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(FM_INT, "FM Int", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(NOISE_LEVEL, "Noise Level", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(NOISE_COLOR, "Noise Color", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(TUNE, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(ANALOG, "Analog", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(GLIDE, "Glide", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::new(BEND, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(CUTOFF, "Cutoff", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(FILTER_RES, "Filter Resonance", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(ENV_ATTACK, "Attack", 0.0, 5.0, 0.005, Unit::Seconds),
    Param::new(ENV_RELEASE, "Release", 0.0, 5.0, 0.3, Unit::Seconds),
    Param::menu(VOICE_MODE, "Voice Mode", VOICE_MODES, 0.0),
    Param::new(VOICES, "Voices", 1.0, 16.0, 8.0, Unit::None),
    Param::new(UNISON, "Unison", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(LOW_FREQ, "Low Frequency", 20.0, 2000.0, 100.0, Unit::Hz),
    Param::new(HIGH_FREQ, "High Frequency", 1000.0, 20000.0, 8000.0, Unit::Hz),
    Param::menu(LOWEST_MODE, "Lowest", LOWEST, 0.0),
    Param::menu(HIGHEST_MODE, "Highest", HIGHEST, 0.0),
    Param::new(RESONANCE, "Resonance", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(STRETCH, "Formant Stretch", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(SHIFT, "Formant Shift", -3.0, 3.0, 0.0, Unit::Octaves),
    Param::new(PITCH_RATE, "Pitch LFO Rate", 0.0, 1.0, 0.7, Unit::None),
    Param::new(SHIFT_RATE, "Shift LFO Rate", 0.0, 1.0, 0.7, Unit::None),
    Param::menu(PITCH_WAVE, "Pitch LFO Wave", LFO_WAVES, 0.0),
    Param::menu(SHIFT_WAVE, "Shift LFO Wave", LFO_WAVES, 0.0),
    Param::new(PITCH_INT_MIN, "Pitch LFO Int (Wheel Min)", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(PITCH_INT_MAX, "Pitch LFO Int (Wheel Max)", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(SHIFT_INT, "Shift LFO Int", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(SIGNAL, "Signal", SIGNALS, 0.0),
    Param::new(LEVEL, "Output Level", -40.0, 12.0, 0.0, Unit::Db),
    Param::menu(ENSEMBLE, "Ensemble", ENSEMBLES, 0.0),
    Param::new(WIDTH, "Stereo Width", 0.0, 1.0, 0.5, Unit::Ratio),
];

/// A free LFO rate in Hz for a position 0.5..1 of the Rate control.
pub fn free_hz(pos: f32) -> f32 {
    0.05 * 400f32.powf(((pos - 0.5) * 2.0).clamp(0.0, 1.0))
}

/// What a Rate control position means: Some(division index) when synced (left of centre).
pub fn sync_division(pos: f32) -> Option<usize> {
    (pos < 0.5).then(|| ((pos / 0.5 * SYNC_DIVS.len() as f32) as usize).min(SYNC_DIVS.len() - 1))
}

/// The Rate control's text: a tempo division or Hz.
pub fn rate_text(pos: f32) -> String {
    match sync_division(pos) {
        Some(i) => SYNC_DIVS[i].to_string(),
        None => format!("{:.2} Hz", free_hz(pos)),
    }
}

/// Centres (Hz) of `n` bands spaced evenly on a log axis between `lo` and `hi`.
pub fn band_centres(n: usize, lo: f32, hi: f32) -> Vec<f32> {
    let n = n.clamp(1, MAX_BANDS);
    let (lo, hi) = (lo.max(20.0), hi.max(lo.max(20.0) * 1.2));
    (0..n)
        .map(|k| {
            let t = if n == 1 { 0.5 } else { k as f32 / (n - 1) as f32 };
            lo * (hi / lo).powf(t)
        })
        .collect()
}

/// The synthesis bands' centres after Formant Stretch (exponent about the middle of the range:
/// 0 leaves them where the analysis bands are) and Formant Shift (octaves).
pub fn synthesis_centres(ana: &[f32], stretch: f32, shift_oct: f32) -> Vec<f32> {
    if ana.is_empty() {
        return Vec::new();
    }
    let mid = (ana[0] * ana[ana.len() - 1]).sqrt();
    let e = (1.0 + stretch).max(0.05);
    ana.iter().map(|&f| (mid * (f / mid).powf(e) * 2f32.powf(shift_oct)).clamp(20.0, 20000.0)).collect()
}

/// What the Formant Filter display shows: both banks' centres and the analysis levels.
#[derive(Clone, Copy, Debug)]
pub struct BandDisplay {
    pub n: usize,
    pub analysis: [f32; MAX_BANDS],
    pub synthesis: [f32; MAX_BANDS],
    pub levels: [f32; MAX_BANDS],
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub(crate) struct EvShared {
    rate: f32,
    fm: bool,
    oct_semi: f32,
    w1: f32,
    w2: f32,
    semi: f32,
    detune: f32,
    balance: f32,
    fm_ratio: f32,
    fm_int: f32,
    noise: f32,
    color: f32,
    tune: f32,
    analog: f32,
    glide: f32,
    glide_from: f32,
    cutoff: f32,
    res: f32,
    env: [f32; 2],
    stack: usize,
    /// Semitones of pitch LFO this chunk.
    lfo_pitch: f32,
}

impl Default for EvShared {
    fn default() -> Self {
        EvShared {
            rate: 48000.0,
            fm: false,
            oct_semi: 0.0,
            w1: 0.0,
            w2: 0.0,
            semi: 0.0,
            detune: 0.0,
            balance: 0.5,
            fm_ratio: 1.0,
            fm_int: 0.3,
            noise: 0.0,
            color: 0.0,
            tune: 0.0,
            analog: 0.0,
            glide: 0.0,
            glide_from: f32::NAN,
            cutoff: 1.0,
            res: 0.0,
            env: [0.005, 0.3],
            stack: 1,
            lfo_pitch: 0.0,
        }
    }
}

const MAX_STACK: usize = 16;

struct EvVoice {
    on: bool,
    note: u8,
    pitch: f32,
    target: f32,
    glide_coef: f32,
    drift: f32,
    vel: f32,
    o1: [dsp::Osc; MAX_STACK],
    o2: [dsp::Osc; MAX_STACK],
    fm_ph: [f32; MAX_STACK],
    car_ph: [f32; MAX_STACK],
    lp: Svf,
    aenv: Adsr,
    fade: Fade,
    killing: bool,
    rng: Rng,
    last_noise: f32,
    level: f32,
}

impl EvVoice {
    fn new(seed: u32) -> EvVoice {
        EvVoice {
            on: false,
            note: 60,
            pitch: 60.0,
            target: 60.0,
            glide_coef: 0.0,
            drift: 0.0,
            vel: 1.0,
            o1: [dsp::Osc::default(); MAX_STACK],
            o2: [dsp::Osc::default(); MAX_STACK],
            fm_ph: [0.0; MAX_STACK],
            car_ph: [0.0; MAX_STACK],
            lp: Svf::default(),
            aenv: Adsr::default(),
            fade: Fade::default(),
            killing: false,
            rng: Rng::new(seed),
            last_noise: 0.0,
            level: 0.0,
        }
    }
    /// Legato: move to a new note without retriggering.
    fn glide_to(&mut self, note: u8, sh: &EvShared) {
        self.note = note;
        self.target = note as f32;
        if sh.glide <= 0.0 {
            self.pitch = self.target;
        }
    }
}

impl Voice<EvShared> for EvVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &EvShared, _ctl: &Ctl) {
        let rate = sh.rate;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.vel = velocity.max(1) as f32 / 127.0;
        self.target = note as f32;
        self.pitch = if sh.glide > 0.0 && sh.glide_from.is_finite() { sh.glide_from } else { self.target };
        self.glide_coef = if sh.glide > 0.0 { dsp::tau_coef(sh.glide * 0.3, rate / CHUNK as f32) } else { 0.0 };
        // Analog: a random pitch error per note
        self.drift = self.rng.bipolar() * sh.analog * 0.3;
        for u in 0..MAX_STACK {
            self.o1[u].phase = self.rng.unit();
            self.o2[u].phase = self.rng.unit();
            self.fm_ph[u] = 0.0;
            self.car_ph[u] = 0.0;
        }
        self.lp.reset();
        self.last_noise = 0.0;
        self.aenv.set(sh.env[0], 0.01, 1.0, sh.env[1], rate);
        self.aenv.trigger();
    }
    fn release(&mut self, sh: &EvShared, _ctl: &Ctl) {
        self.aenv.set(sh.env[0], 0.01, 1.0, sh.env[1], sh.rate);
        self.aenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.003, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &EvShared, ctl: &Ctl) {
        let rate = sh.rate;
        let n = l.len();
        self.pitch = self.target + (self.pitch - self.target) * self.glide_coef;
        let base = self.pitch + sh.tune / 100.0 + self.drift + ctl.bend + sh.lfo_pitch;
        let uni = sh.stack.clamp(1, MAX_STACK);
        let hz1 = lg_dsp::midi_hz(base + sh.oct_semi);
        let det_c = sh.analog * 30.0;
        // stacked voices add up louder (about a quarter-power law)
        let gain = 0.45 / (uni as f32).powf(0.25);
        let fc = 60.0 * 330f32.powf(sh.cutoff.clamp(0.0, 1.0));
        self.lp.mode = SvfMode::Low;
        self.lp.set(fc.min(rate * 0.45), 0.6 + sh.res * 8.0, rate);
        let filt_on = sh.cutoff < 0.999;
        let mut peak = 0.0f32;
        let nz_k = sh.color * 0.97;
        for i in 0..n {
            let mut s = 0.0;
            for u in 0..uni {
                let off = if uni > 1 { (u as f32 / (uni - 1) as f32 - 0.5) * det_c } else { 0.0 };
                if sh.fm {
                    // oscillator 2 modulates oscillator 1's sine; Detune and Wave 1 do not apply
                    let f1 = hz1 * dsp::semis(off / 100.0);
                    let inc1 = f1 / rate;
                    self.fm_ph[u] = (self.fm_ph[u] + inc1 * sh.fm_ratio).fract();
                    self.car_ph[u] = (self.car_ph[u] + inc1).fract();
                    let m = crate::retro::table_read(sh.w2, self.fm_ph[u], f1 * sh.fm_ratio);
                    s += dsp::sin_cycles(self.car_ph[u] + m * sh.fm_int * 1.2);
                } else {
                    // Detune fine-tunes both oscillators (apart), Semi tunes oscillator 2
                    let f1 = hz1 * dsp::semis(off / 100.0 + sh.detune * 0.01);
                    let f2 = hz1 * dsp::semis(off / 100.0 + sh.semi - sh.detune * 0.01);
                    self.o1[u].advance(f1 / rate);
                    self.o2[u].advance(f2 / rate);
                    let a = crate::retro::table_read(sh.w1, self.o1[u].phase, f1);
                    let b = crate::retro::table_read(sh.w2, self.o2[u].phase, f2);
                    s += a * (1.0 - sh.balance) + b * sh.balance;
                }
            }
            s *= gain;
            if sh.noise > 0.0 {
                let w = self.rng.bipolar();
                // blue-ish: subtract the last sample, which tilts the spectrum upward
                let nz = (w - nz_k * self.last_noise) / (1.0 + nz_k * 0.5);
                self.last_noise = w;
                s += nz * sh.noise;
            }
            if filt_on {
                s = self.lp.run(s);
            }
            let a = self.aenv.next();
            let mut g = a * (0.6 + 0.4 * self.vel);
            if self.killing {
                g *= self.fade.next();
            }
            s *= g;
            peak = peak.max(s.abs());
            l[i] += s;
            r[i] += s;
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

// ---------------------------------------------------------------------------------------------

/// How much of the side chain one block may hold (longer blocks are analysed in part).
const SC_CAP: usize = 16384;

pub struct EvocPs {
    params: Params,
    rate: f32,
    poly: Poly<EvVoice>,
    ch: Channel,
    sh: EvShared,
    // side chain for this block
    sc_l: Vec<f32>,
    sc_r: Vec<f32>,
    sc_len: usize,
    // vocoder
    nb: usize,
    ana: [[Svf; 2]; MAX_BANDS],
    syn: [[Svf; 2]; MAX_BANDS],
    qa: [f32; MAX_BANDS],
    qs: [f32; MAX_BANDS],
    env: [f32; MAX_BANDS],
    att_c: f32,
    rel_c: f32,
    pan: [(f32, f32); MAX_BANDS],
    centres_a: [f32; MAX_BANDS],
    centres_s: [f32; MAX_BANDS],
    shift_extra: f32,
    // U/V
    hp: Svf,
    hf_env: f32,
    full_env: f32,
    unvoiced: f32,
    noise: Rng,
    // modulation
    pitch_lfo: Lfo,
    shift_lfo: Lfo,
    lfo_rng: Rng,
    bpm: f64,
    // voices
    stack: [u8; 16],
    nstack: usize,
    last_pitch: f32,
    mono_note: Option<u8>,
    ens_a: Chorus,
    ens_b: Chorus,
    out: f32,
}

impl EvocPs {
    pub fn new(rate: f32) -> EvocPs {
        dsp::warm_tables();
        let _ = crate::retro::tables();
        let mut seed = 5;
        let mut s = EvocPs {
            params: Params::new(PARAMS),
            rate,
            poly: Poly::new(16, || {
                seed += 13;
                EvVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: EvShared { rate, ..Default::default() },
            sc_l: vec![0.0; SC_CAP],
            sc_r: vec![0.0; SC_CAP],
            sc_len: 0,
            nb: MAX_BANDS,
            ana: [[Svf::default(); 2]; MAX_BANDS],
            syn: [[Svf::default(); 2]; MAX_BANDS],
            qa: [1.0; MAX_BANDS],
            qs: [1.0; MAX_BANDS],
            env: [0.0; MAX_BANDS],
            att_c: 0.1,
            rel_c: 0.01,
            pan: [(1.0, 1.0); MAX_BANDS],
            centres_a: [0.0; MAX_BANDS],
            centres_s: [0.0; MAX_BANDS],
            shift_extra: 0.0,
            hp: Svf::default(),
            hf_env: 0.0,
            full_env: 0.0,
            unvoiced: 0.0,
            noise: Rng::new(77),
            pitch_lfo: Lfo::default(),
            shift_lfo: Lfo::default(),
            lfo_rng: Rng::new(31),
            bpm: 120.0,
            stack: [0; 16],
            nstack: 0,
            last_pitch: f32::NAN,
            mono_note: None,
            ens_a: Chorus::new(rate),
            ens_b: Chorus::new(rate),
            out: 1.0,
        };
        s.update();
        s
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn mode(&self) -> u8 {
        self.p(VOICE_MODE).round() as u8
    }

    /// Both banks' centres and the analysis levels, for the Formant Filter display.
    pub fn display(&self) -> BandDisplay {
        BandDisplay { n: self.nb, analysis: self.centres_a, synthesis: self.centres_s, levels: self.env }
    }
    /// The voices field's value: Poly voices (halved by Unison), or the stack in Unison mono.
    pub fn voices_shown(&self) -> usize {
        let uni = self.p(UNISON) >= 0.5;
        match (self.mode(), uni) {
            (0, false) => self.p(VOICES).round() as usize,
            (0, true) => (self.p(VOICES).round() as usize / 2).clamp(1, 8),
            (_, true) => MAX_STACK,
            _ => 1,
        }
    }

    fn update(&mut self) {
        let mode = self.mode();
        let uni = self.p(UNISON) >= 0.5;
        let voices = self.p(VOICES).round() as usize;
        self.poly.set_max(match (mode, uni) {
            (0, false) => voices,
            (0, true) => (voices / 2).clamp(1, 8),
            _ => 1,
        });
        self.poly.retrigger_kills = true;
        let sh = &mut self.sh;
        sh.fm = self.params.get(OSC_MODE) >= 0.5;
        sh.oct_semi = (self.params.get(OCTAVE).round() - 1.0) * 12.0;
        sh.w1 = self.params.get(WAVE1) / (WAVES - 1) as f32;
        sh.w2 = self.params.get(WAVE2) / (WAVES - 1) as f32;
        sh.semi = self.params.get(SEMI);
        sh.detune = self.params.get(DETUNE);
        sh.balance = self.params.get(BALANCE);
        sh.fm_ratio = dsp::semis(self.params.get(FM_COARSE) + self.params.get(FM_FINE) / 100.0);
        sh.fm_int = self.params.get(FM_INT);
        sh.noise = self.params.get(NOISE_LEVEL);
        sh.color = self.params.get(NOISE_COLOR);
        sh.tune = self.params.get(TUNE);
        sh.analog = self.params.get(ANALOG);
        sh.glide = self.params.get(GLIDE);
        sh.cutoff = self.params.get(CUTOFF);
        sh.res = self.params.get(FILTER_RES);
        sh.env = [self.params.get(ENV_ATTACK), self.params.get(ENV_RELEASE)];
        // Unison: poly doubles each voice; mono / legato stack sixteen
        sh.stack = match (mode, uni) {
            (_, false) => 1,
            (0, true) => 2,
            _ => MAX_STACK,
        };
        self.ch.bend_up = self.params.get(BEND);
        self.ch.bend_down = self.params.get(BEND);
        self.ch.update_bend();
        self.out = lg_dsp::db_to_gain(self.params.get(LEVEL));
        // follower times
        let rate = self.rate;
        self.att_c = 1.0 - (-1.0 / (self.params.get(ATTACK) * 0.001 * rate)).exp();
        self.rel_c = 1.0 - (-1.0 / (self.params.get(RELEASE) * 0.001 * rate)).exp();
        self.nb = (self.params.get(BANDS).round() as usize).clamp(1, MAX_BANDS);
        self.tune_banks(0.0);
        self.hp.mode = SvfMode::High;
        self.hp.set(4000.0, 0.7, rate);
        self.pan_bands();
    }

    fn pan_bands(&mut self) {
        let w = self.params.get(WIDTH);
        let n = self.nb;
        for k in 0..n {
            let asc = if n > 1 { k as f32 / (n - 1) as f32 * 2.0 - 1.0 } else { 0.0 };
            let alt = if k % 2 == 0 { -1.0 } else { 1.0 };
            // 0: all centred; middle: ascending left to right; full: alternating
            let p = if w <= 0.5 { asc * (w / 0.5) } else { asc + (alt - asc) * ((w - 0.5) / 0.5) };
            let a = (p + 1.0) * 0.25 * std::f32::consts::PI;
            self.pan[k] = (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2);
        }
    }

    /// Sets both banks' filters. The analysis bank is fixed by Low / High; the synthesis bank
    /// adds Stretch, Shift and `shift_extra` (the Shift LFO, in octaves).
    fn tune_banks(&mut self, shift_extra: f32) {
        let rate = self.rate;
        let (lo, hi) = (self.params.get(LOW_FREQ), self.params.get(HIGH_FREQ));
        let ca = band_centres(self.nb, lo, hi);
        let cs = synthesis_centres(&ca, self.params.get(STRETCH), self.params.get(SHIFT) + shift_extra);
        let res = self.params.get(RESONANCE);
        let lowhp = self.params.get(LOWEST_MODE) >= 0.5;
        let highlp = self.params.get(HIGHEST_MODE) >= 0.5;
        let n = self.nb;
        let q_for = |c: &[f32], k: usize| {
            // width from the local spacing of the centres; Resonance narrows each band
            let r = if n == 1 {
                2.0
            } else if k + 1 < n {
                (c[k + 1] / c[k]).max(1.01)
            } else {
                (c[k] / c[k - 1]).max(1.01)
            };
            let nat = 1.0 / (r.sqrt() - 1.0 / r.sqrt());
            (nat * (0.5 + res * 3.0)).clamp(0.7, 60.0)
        };
        for k in 0..n {
            let mode = if k == 0 && lowhp {
                SvfMode::High
            } else if k == n - 1 && highlp {
                SvfMode::Low
            } else {
                SvfMode::Band
            };
            let qa = q_for(&ca, k);
            let qs = q_for(&cs, k);
            self.qa[k] = qa;
            self.qs[k] = qs;
            for st in 0..2 {
                self.ana[k][st].mode = mode;
                self.ana[k][st].set(ca[k].min(rate * 0.45), qa, rate);
                self.syn[k][st].mode = mode;
                self.syn[k][st].set(cs[k].min(rate * 0.45), qs, rate);
            }
            self.centres_a[k] = ca[k];
            self.centres_s[k] = cs[k];
        }
        self.shift_extra = shift_extra;
    }

    fn lfo_hz(&self, pos: f32) -> f32 {
        match sync_division(pos) {
            Some(i) => (self.bpm as f32 / 60.0) / SYNC_BEATS[i],
            None => free_hz(pos),
        }
    }

    fn lfo_value(wave: u8, lfo: &mut Lfo, inc: f32, rng: &mut Rng) -> f32 {
        // our order: triangle, saw down, saw up, bipolar square, unipolar square, S&H, smooth random
        let (code, uni) = match wave {
            0 => (1, false),
            1 => (3, false),
            2 => (2, false),
            3 => (4, false),
            4 => (4, true),
            5 => (5, false),
            _ => (6, false),
        };
        let v = lfo.step(code, inc, rng);
        if uni { (v + 1.0) * 0.5 } else { v }
    }
}

impl Plugin for EvocPs {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.update();
        }
    }
    /// Whole-number settings show without decimals (voices, bands, wave numbers).
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        [VOICES, BANDS, WAVE1, WAVE2].contains(&id).then(|| format!("{}", v.round()))
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.nstack = 0;
        self.mono_note = None;
        self.last_pitch = f32::NAN;
        self.env = [0.0; MAX_BANDS];
        for k in 0..MAX_BANDS {
            for st in 0..2 {
                self.ana[k][st].reset();
                self.syn[k][st].reset();
            }
        }
        self.ens_a.reset();
        self.ens_b.reset();
        self.ch.reset();
        self.sc_len = 0;
        self.update();
    }
    fn tail(&self) -> f32 {
        self.p(ENV_RELEASE) + self.p(RELEASE) * 0.003 + 0.2
    }
}

impl Instrument for EvocPs {
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
    fn pressure(&mut self, _value: u8) {}
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len()).min(SC_CAP);
        self.sc_l[..n].copy_from_slice(&left[..n]);
        self.sc_r[..n].copy_from_slice(&right[..n]);
        self.sc_len = n;
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let mode = self.mode();
        if mode == 0 {
            // poly: glide starts from the last note played
            self.sh.glide_from = self.last_pitch;
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
            self.last_pitch = pitch as f32;
            return;
        }
        if self.nstack < self.stack.len() {
            self.stack[self.nstack] = pitch;
            self.nstack += 1;
        }
        let legato = mode == 2;
        let sh = self.sh;
        if let (true, Some(_)) = (legato && self.nstack > 1, self.mono_note) {
            // tied note: glide, no retrigger
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(pitch, &sh);
                self.last_pitch = pitch as f32;
                return;
            }
        }
        // mono retriggers; Glide is always active, starting from where the voice was
        if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
            let mut sh2 = sh;
            sh2.glide_from = v.pitch;
            v.start(pitch, velocity, &sh2, &self.ch.ctl);
            self.mono_note = Some(pitch);
            self.last_pitch = pitch as f32;
            return;
        }
        let mut sh2 = sh;
        sh2.glide_from = if mode == 1 { self.last_pitch } else { f32::NAN };
        self.mono_note = Some(pitch);
        self.poly.note_on(pitch, velocity, &sh2, &self.ch.ctl);
        self.last_pitch = pitch as f32;
    }
    fn note_off(&mut self, pitch: u8) {
        let mode = self.mode();
        if mode == 0 {
            self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
            return;
        }
        if let Some(i) = self.stack[..self.nstack].iter().position(|&n| n == pitch) {
            for j in i..self.nstack - 1 {
                self.stack[j] = self.stack[j + 1];
            }
            self.nstack -= 1;
        }
        let sh = self.sh;
        if self.nstack > 0 {
            let top = self.stack[self.nstack - 1];
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(top, &sh);
            }
        } else if self.mono_note.take().is_some() {
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
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut car = [0.0f32; CHUNK];
        let mut scratch = [0.0f32; CHUNK];
        let mut outl = [0.0f32; CHUNK];
        let mut outr = [0.0f32; CHUNK];
        let signal = self.p(SIGNAL).round() as u8;
        let freeze = self.p(FREEZE) >= 0.5;
        let uv_mode = self.p(UV_MODE).round() as u8;
        let uv_level = self.p(UV_LEVEL);
        let sens = self.p(UV_SENS);
        let (gl, gr) = self.ch.out_gains();
        let lowhp = self.p(LOWEST_MODE) >= 0.5;
        let highlp = self.p(HIGHEST_MODE) >= 0.5;
        let shift_int = self.p(SHIFT_INT);
        let pitch_wave = self.p(PITCH_WAVE).round() as u8;
        let shift_wave = self.p(SHIFT_WAVE).round() as u8;
        let ens = self.p(ENSEMBLE).round() as u8;
        let nb = self.nb;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            // LFOs
            let pinc = self.lfo_hz(self.p(PITCH_RATE)) * m as f32 / self.rate;
            let wheel = self.ch.ctl.mod_wheel;
            let depth = self.p(PITCH_INT_MIN) + (self.p(PITCH_INT_MAX) - self.p(PITCH_INT_MIN)) * wheel;
            let pl = Self::lfo_value(pitch_wave, &mut self.pitch_lfo, pinc, &mut self.lfo_rng);
            self.sh.lfo_pitch = pl * depth * 2.0;
            if shift_int > 0.0 {
                let sinc = self.lfo_hz(self.p(SHIFT_RATE)) * m as f32 / self.rate;
                let sv = Self::lfo_value(shift_wave, &mut self.shift_lfo, sinc, &mut self.lfo_rng);
                let extra = sv * shift_int;
                if (extra - self.shift_extra).abs() > 0.002 {
                    self.tune_banks(extra);
                }
            } else if self.shift_extra != 0.0 {
                self.tune_banks(0.0);
            }
            // the synth
            car[..m].fill(0.0);
            scratch[..m].fill(0.0);
            self.poly.render(&mut car[..m], &mut scratch[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                let idx = pos + i;
                let x = if idx < self.sc_len { 0.5 * (self.sc_l[idx] + self.sc_r[idx]) } else { 0.0 };
                let synth = car[i];
                // unvoiced detection: the high-frequency share of the analysis signal
                let hi = self.hp.run(x);
                self.hf_env += (hi.abs() - self.hf_env) * 0.01;
                self.full_env += (x.abs() - self.full_env) * 0.01;
                let ratio = self.hf_env / self.full_env.max(1e-5);
                let thr = 0.95 - 0.9 * sens;
                let uv_now = if self.full_env > 1e-4 && ratio > thr { 1.0 } else { 0.0 };
                self.unvoiced += (uv_now - self.unvoiced) * 0.01;
                let noise = self.noise.bipolar();
                let u = if uv_mode == 2 { 0.0 } else { self.unvoiced };
                let carrier = match uv_mode {
                    0 => synth * (1.0 - u) + noise * 0.6 * uv_level * u,
                    1 => synth + noise * 0.6 * uv_level * u,
                    _ => synth,
                };
                let mut sum_l = 0.0f32;
                let mut sum_r = 0.0f32;
                for k in 0..nb {
                    let edge = (k == 0 && lowhp) || (k == nb - 1 && highlp);
                    // analysis
                    let a1 = self.ana[k][0].run(x);
                    let a1 = if edge { a1 } else { a1 / self.qa[k] };
                    let a2 = self.ana[k][1].run(a1);
                    let a2 = if edge { a2 } else { a2 / self.qa[k] };
                    if !freeze {
                        let v = a2.abs();
                        let c = if v > self.env[k] { self.att_c } else { self.rel_c };
                        self.env[k] += (v - self.env[k]) * c;
                    }
                    // synthesis
                    let s1 = self.syn[k][0].run(carrier);
                    let s1 = if edge { s1 } else { s1 / self.qs[k] };
                    let s2 = self.syn[k][1].run(s1);
                    let s2 = if edge { s2 } else { s2 / self.qs[k] };
                    let y = s2 * self.env[k] * 90.0;
                    sum_l += y * self.pan[k].0;
                    sum_r += y * self.pan[k].1;
                }
                // thinning the bank keeps the level
                let nrm = (MAX_BANDS as f32 / nb as f32).sqrt() * 0.35;
                let (mut yl, mut yr) = (sum_l * nrm, sum_r * nrm);
                if uv_mode == 2 {
                    // Blend: the high-passed analysis joins the output
                    yl += hi * uv_level * 1.5;
                    yr += hi * uv_level * 1.5;
                }
                let (ol, or) = match signal {
                    1 => (synth * 2.0, synth * 2.0),
                    2 => (x, x),
                    _ => (yl, yr),
                };
                outl[i] = ol;
                outr[i] = or;
            }
            let (el, er) = (&mut outl[..m], &mut outr[..m]);
            match ens {
                1 => self.ens_a.process(el, er, 0.6, 0.5, 0.5),
                2 => {
                    self.ens_a.process(el, er, 0.45, 0.9, 0.6);
                    self.ens_b.process(el, er, 0.9, 0.6, 0.5);
                }
                _ => {}
            }
            for i in 0..m {
                left[pos + i] += outl[i] * gl * self.out;
                right[pos + i] += outr[i] * gr * self.out;
            }
            pos += m;
        }
        self.sc_len = 0;
    }
}
