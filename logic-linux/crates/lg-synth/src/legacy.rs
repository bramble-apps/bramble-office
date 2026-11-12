//! Logic's legacy instruments (the Option-click Legacy submenu): light synthesizers with a few
//! sliders — Analog Basic / Mono / Pad / Swirl / Sync, Digital Basic / Mono / Stepper, Hybrid
//! Basic / Morph, Church Organ and Sound Effects. One small engine of our own plays them all:
//! two oscillators (analog shapes, hard sync, additive "digital" tones or Studio's own two-layer
//! wave sets), a resonant filter with its envelope, the amplitude ADSR, glide, unison
//! richness, a chorus, a stepping sequencer, pipe ranks for the organ and noise textures for
//! the sound effects. The waves, registrations and effects are Studio's.

use lg_dsp::{Instrument, Param, Plugin, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    AnalogBasic,
    AnalogMono,
    AnalogPad,
    AnalogSwirl,
    AnalogSync,
    DigitalBasic,
    DigitalMono,
    DigitalStepper,
    HybridBasic,
    HybridMorph,
    ChurchOrgan,
    SoundEffects,
}

pub const VARIANTS: &[(&str, Variant)] = &[
    ("Analog Basic", Variant::AnalogBasic),
    ("Analog Mono", Variant::AnalogMono),
    ("Analog Pad", Variant::AnalogPad),
    ("Analog Swirl", Variant::AnalogSwirl),
    ("Analog Sync", Variant::AnalogSync),
    ("Digital Basic", Variant::DigitalBasic),
    ("Digital Mono", Variant::DigitalMono),
    ("Digital Stepper", Variant::DigitalStepper),
    ("Hybrid Basic", Variant::HybridBasic),
    ("Hybrid Morph", Variant::HybridMorph),
    ("Church Organ", Variant::ChurchOrgan),
    ("Sound Effects", Variant::SoundEffects),
];

pub fn variant_of(name: &str) -> Option<Variant> {
    VARIANTS.iter().find(|v| v.0 == name).map(|v| v.1)
}

// parameter ids (one numbering; each variant shows its own)
pub const VOLUME: u32 = 0;
pub const MIX: u32 = 1;
pub const TUNING: u32 = 2;
pub const CUTOFF: u32 = 3;
pub const RESONANCE: u32 = 4;
pub const ATTACK: u32 = 5;
pub const DECAY: u32 = 6;
pub const SUSTAIN: u32 = 7;
pub const RELEASE: u32 = 8;
pub const GLIDE: u32 = 9;
pub const RICHNESS: u32 = 10;
pub const MODULATION: u32 = 11;
pub const CHARACTER: u32 = 12;
pub const CUTOFF_ENV: u32 = 13;
pub const DURATION: u32 = 14;
pub const ANIMATION: u32 = 15;
pub const SYNC: u32 = 16;
pub const SYNC_MOD: u32 = 17;
pub const SYNC_ENV: u32 = 18;
pub const HARMONICS: u32 = 19;
pub const TIMBRE: u32 = 20;
pub const TIMBRE_ENV: u32 = 21;
pub const DISTORTION: u32 = 22;
pub const BALANCE: u32 = 23;
pub const HARM_STEPS: u32 = 24;
pub const CUTOFF_STEPS: u32 = 25;
pub const WAVEFORM: u32 = 26;
pub const WHEEL_VIB: u32 = 27;
pub const WHEEL_CUT: u32 = 28;
pub const CUTOFF_TYPE: u32 = 29;
pub const CUTOFF_ATTACK: u32 = 30;
pub const MORPH: u32 = 31;
pub const MORPH_ENV: u32 = 32;
pub const REGISTRATION: u32 = 33;
pub const BASS: u32 = 34;

pub const WAVES: &[&str] = &["Glass Bell", "Breath Pad", "Hollow Reed", "Bright Strings", "Vox Choir", "Metal Keys", "Soft Brass", "Plucked Wire"];
pub const CUTOFF_TYPES: &[&str] = &["Smooth Lowpass", "Steep Lowpass", "Bandpass", "Highpass", "Peaked Lowpass"];
pub const REGISTRATIONS: &[&str] = &["Soft Flute 8'", "Principal 8' 4'", "Bright Mixture", "Reed Chorus", "Celeste Strings", "Full Organ", "Solo Trumpet"];
/// Registrations that have no bass pipes (their Bass buttons are off).
const NO_BASS: &[usize] = &[4, 6];
pub const BASS_CHOICES: &[&str] = &["Off", "16'", "16' + 32'"];

fn p(id: u32, n: &'static str, def: f32) -> Param {
    Param::new(id, n, 0.0, 1.0, def, Unit::Ratio)
}

/// The sliders and pop-ups of a variant, in the window's order.
pub fn params_of(v: Variant) -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Vec<Param>>> = std::sync::OnceLock::new();
    let all = P.get_or_init(|| {
        VARIANTS
            .iter()
            .map(|(_, v)| {
                let vol = Param::new(VOLUME, "Volume", -40.0, 6.0, -6.0, Unit::Db);
                use Variant::*;
                match v {
                    AnalogBasic => vec![vol, p(MIX, "Mix", 0.5), Param::new(TUNING, "Tuning", -12.0, 12.0, 0.0, Unit::Semitones), p(CUTOFF, "Cutoff", 0.7), p(RESONANCE, "Resonance", 0.2), p(ATTACK, "Attack", 0.0), p(DECAY, "Decay", 0.4), p(SUSTAIN, "Sustain", 0.7)],
                    AnalogMono => vec![vol, p(GLIDE, "Glide", 0.1), p(MIX, "Mix", 0.5), p(CUTOFF, "Cutoff", 0.6), p(RESONANCE, "Resonance", 0.3), p(ATTACK, "Attack", 0.0), p(DECAY, "Decay", 0.4), p(RICHNESS, "Richness", 0.3)],
                    AnalogPad => vec![vol, p(MODULATION, "Modulation", 0.4), p(CHARACTER, "Character", 0.5), p(CUTOFF, "Cutoff", 0.5), p(RESONANCE, "Resonance", 0.2), p(CUTOFF_ENV, "Cutoff Envelope", 0.4), p(DURATION, "Duration", 0.5), p(ANIMATION, "Animation", 0.5)],
                    AnalogSwirl => vec![vol, p(MODULATION, "Modulation", 0.4), p(CUTOFF, "Cutoff", 0.6), p(RESONANCE, "Resonance", 0.2), p(ATTACK, "Attack", 0.3), p(DECAY, "Decay", 0.5), p(SUSTAIN, "Sustain", 0.8), p(RELEASE, "Release", 0.5)],
                    AnalogSync => vec![vol, p(SYNC, "Sync", 0.4), p(SYNC_MOD, "Sync Modulation", 0.2), p(SYNC_ENV, "Sync Envelope", 0.4), p(CUTOFF, "Cutoff", 0.7), p(ATTACK, "Attack", 0.0), p(DECAY, "Decay", 0.4), p(SUSTAIN, "Sustain", 0.6)],
                    DigitalBasic => vec![vol, p(MIX, "Mix", 0.5), Param::new(TUNING, "Tuning", -12.0, 12.0, 0.0, Unit::Semitones), p(HARMONICS, "Harmonics", 0.5), p(TIMBRE, "Timbre", 0.5), p(ATTACK, "Attack", 0.0), p(DECAY, "Decay", 0.4), p(RELEASE, "Release", 0.3)],
                    DigitalMono => vec![vol, Param::new(TUNING, "Tuning", -12.0, 12.0, 0.0, Unit::Semitones), p(HARMONICS, "Harmonics", 0.5), p(TIMBRE, "Timbre", 0.5), p(TIMBRE_ENV, "Timbre Envelope", 0.4), p(DECAY, "Decay", 0.4), p(RICHNESS, "Richness", 0.3), p(DISTORTION, "Distortion", 0.0)],
                    DigitalStepper => vec![vol, p(BALANCE, "Balance", 0.5), p(MODULATION, "Modulation", 0.3), p(HARMONICS, "Harmonics", 0.5), p(HARM_STEPS, "Harmonic Steps", 0.5), p(CUTOFF, "Cutoff", 0.6), p(CUTOFF_STEPS, "Cutoff Steps", 0.5), p(DURATION, "Duration", 0.5)],
                    HybridBasic => vec![vol, Param::menu(WAVEFORM, "Waveform", WAVES, 0.0), p(GLIDE, "Glide", 0.0), p(WHEEL_VIB, "Wheel to Vibrato", 0.3), p(WHEEL_CUT, "Wheel to Cutoff", 0.0), p(CUTOFF, "Cutoff", 0.7), Param::menu(CUTOFF_TYPE, "Cutoff Type", CUTOFF_TYPES, 0.0), p(RESONANCE, "Resonance", 0.2), p(CUTOFF_ATTACK, "Cutoff Attack", 0.0), p(ATTACK, "Attack", 0.0), p(DECAY, "Decay", 0.5), p(SUSTAIN, "Sustain", 0.7), p(RELEASE, "Release", 0.3)],
                    HybridMorph => vec![vol, Param::menu(WAVEFORM, "Waveform", WAVES, 1.0), Param::new(MORPH, "Morph", 0.0, 1.0, 0.5, Unit::Ratio), Param::new(MORPH_ENV, "Morph Envelope", -1.0, 1.0, 0.0, Unit::None), p(CUTOFF, "Cutoff", 0.7), Param::menu(CUTOFF_TYPE, "Cutoff Type", CUTOFF_TYPES, 0.0), p(RESONANCE, "Resonance", 0.2), p(CUTOFF_ENV, "Cutoff Envelope", 0.3), p(ATTACK, "Attack", 0.1), p(DECAY, "Decay", 0.5), p(SUSTAIN, "Sustain", 0.7), p(RELEASE, "Release", 0.4)],
                    ChurchOrgan => vec![vol, Param::menu(REGISTRATION, "Registration", REGISTRATIONS, 1.0), Param::menu(BASS, "Bass", BASS_CHOICES, 0.0), p(CUTOFF, "Cutoff", 0.8), p(ATTACK, "Attack", 0.1), p(RELEASE, "Release", 0.3)],
                    SoundEffects => vec![vol, p(CUTOFF, "Filter Cutoff", 1.0), p(RELEASE, "Release", 0.3)],
                }
            })
            .collect()
    });
    let i = VARIANTS.iter().position(|x| x.1 == v).unwrap_or(0);
    &all[i]
}

/// Bass buttons unavailable for this registration?
pub fn bass_off(registration: usize) -> bool {
    NO_BASS.contains(&registration)
}

/// Harmonic amplitudes of a Hybrid wave set's two layers (A, B), 32 partials (Studio's own).
fn wave_layers(w: usize) -> ([f32; 32], [f32; 32]) {
    let mut a = [0.0f32; 32];
    let mut b = [0.0f32; 32];
    for h in 0..32 {
        let n = (h + 1) as f32;
        let (x, y) = match w {
            0 => (if [0, 2, 5, 9, 14].contains(&h) { 1.0 / n.sqrt() } else { 0.0 }, 1.0 / (n * n)),
            1 => (1.0 / n * (-(n / 6.0)).exp(), if h % 2 == 0 { 1.0 / n } else { 0.2 / n }),
            2 => (if h % 2 == 0 { 1.0 / n } else { 0.0 }, 1.0 / n.powf(1.4)),
            3 => (1.0 / n, 1.0 / n * if h > 4 { 1.2 } else { 0.6 }),
            4 => ((-((n - 3.0) * (n - 3.0)) / 6.0).exp() + (-((n - 9.0) * (n - 9.0)) / 8.0).exp() * 0.5, 1.0 / (n * n)),
            5 => (if h % 3 == 0 { 1.0 / n.sqrt() } else { 0.1 / n }, (-(n / 3.0)).exp()),
            6 => (1.0 / n * (-(n / 10.0)).exp(), 1.0 / n),
            _ => ((n * 0.7).sin().abs() / n, 1.0 / n.powf(0.8)),
        };
        a[h] = x;
        b[h] = y;
    }
    (a, b)
}

/// Pipe ranks of a registration: (footage multiplier, level).
fn ranks(r: usize) -> &'static [(f32, f32)] {
    match r {
        0 => &[(1.0, 1.0), (2.0, 0.15)],
        1 => &[(1.0, 0.8), (2.0, 0.6)],
        2 => &[(1.0, 0.6), (2.0, 0.5), (3.0, 0.4), (4.0, 0.4), (6.0, 0.3), (8.0, 0.25)],
        3 => &[(1.0, 0.7), (2.0, 0.7), (3.0, 0.5), (5.0, 0.4)],
        4 => &[(1.0, 0.6), (1.006, 0.6)],
        5 => &[(1.0, 0.7), (2.0, 0.6), (3.0, 0.5), (4.0, 0.5), (6.0, 0.4), (8.0, 0.35), (12.0, 0.2)],
        _ => &[(1.0, 1.0), (2.0, 0.8), (3.0, 0.7), (4.0, 0.5), (5.0, 0.4)],
    }
}

#[derive(Clone, Copy, Default)]
struct Svf {
    lp: f32,
    bp: f32,
}
impl Svf {
    fn tick(&mut self, x: f32, f: f32, q: f32) -> (f32, f32, f32) {
        let hp = x - self.lp - q * self.bp;
        self.bp += f * hp;
        self.lp += f * self.bp;
        (self.lp, self.bp, hp)
    }
}

#[derive(Clone, Copy)]
struct Voice {
    on: bool,
    key: u8,
    vel: f32,
    hz: f32,
    target_hz: f32,
    ph: [f32; 4],
    env: f32,
    stage: u8,
    fenv: f32,
    t: f32,
    f: [Svf; 2],
    rng: u32,
    age: u32,
}

impl Voice {
    fn new() -> Voice {
        Voice { on: false, key: 0, vel: 1.0, hz: 440.0, target_hz: 440.0, ph: [0.0; 4], env: 0.0, stage: 0, fenv: 0.0, t: 0.0, f: [Svf::default(); 2], rng: 1, age: 0 }
    }
}

pub struct Legacy {
    pub variant: Variant,
    params: crate::poly::Params,
    voices: Vec<Voice>,
    rate: f32,
    wheel: f32,
    lfo: f32,
    step: (f32, usize),
    chorus: Vec<f32>,
    chorus_at: usize,
    age: u32,
}

fn adv(ph: &mut [f32; 4], dt: f32, k: usize, f: f32) -> f32 {
    ph[k] = (ph[k] + f * dt).fract();
    ph[k]
}
fn saw(ph: f32) -> f32 {
    2.0 * ph - 1.0
}
fn square(ph: f32, pw: f32) -> f32 {
    if ph < pw { 1.0 } else { -1.0 }
}

impl Legacy {
    pub fn new(variant: Variant, rate: f32) -> Legacy {
        Legacy { variant, params: crate::poly::Params::new(params_of(variant)), voices: vec![Voice::new(); 12], rate, wheel: 0.0, lfo: 0.0, step: (0.0, 0), chorus: vec![0.0; (rate * 0.05) as usize], chorus_at: 0, age: 0 }
    }
    fn mono(&self) -> bool {
        matches!(self.variant, Variant::AnalogMono | Variant::DigitalMono)
    }
    fn g(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    /// Seconds for a 0..1 time slider.
    fn secs(x: f32, max: f32) -> f32 {
        0.002 + x * x * max
    }
}

impl Plugin for Legacy {
    fn params(&self) -> &[Param] {
        params_of(self.variant)
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.params.set(id, v);
    }
    fn reset(&mut self) {
        for v in &mut self.voices {
            v.on = false;
        }
    }
    fn tail(&self) -> f32 {
        3.0
    }
}

impl Instrument for Legacy {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.age += 1;
        let hz = 440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0);
        if self.mono() {
            let v = &mut self.voices[0];
            let legato = v.on && v.stage < 3;
            v.on = true;
            v.key = pitch;
            v.vel = velocity as f32 / 127.0;
            v.target_hz = hz;
            if !legato || self.params.get(GLIDE) <= 0.0 {
                v.hz = hz;
            }
            if !legato {
                v.stage = 0;
                v.t = 0.0;
            }
            return;
        }
        let i = self.voices.iter().position(|v| !v.on).unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0));
        let mut v = Voice::new();
        v.on = true;
        v.key = pitch;
        v.vel = velocity as f32 / 127.0;
        v.hz = hz;
        v.target_hz = hz;
        v.rng = 0x9e37_79b9 ^ (self.age.wrapping_mul(2_654_435_761));
        v.age = self.age;
        // Hybrid Basic glides from the last note
        if self.variant == Variant::HybridBasic && self.g(GLIDE) > 0.0
            && let Some(last) = self.voices.iter().filter(|x| x.on).max_by_key(|x| x.age)
        {
            v.hz = last.hz;
        }
        self.voices[i] = v;
    }
    fn note_off(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.on && v.key == pitch && v.stage < 3 {
                v.stage = 3;
            }
        }
    }
    fn controller(&mut self, cc: u8, v: u8) {
        if cc == 1 {
            self.wheel = v as f32 / 127.0;
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        use Variant::*;
        let n = left.len().min(right.len());
        let dt = 1.0 / self.rate;
        let var = self.variant;
        let rate = self.rate;
        let pv: Vec<f32> = (0..=BASS).map(|i| self.params.get(i)).collect();
        let g = |id: u32| pv[id as usize];
        let vol = lg_dsp::db_to_gain(g(VOLUME));
        let (att, dec, sus, rel) = match var {
            AnalogMono | DigitalMono => (Legacy::secs(g(ATTACK), 3.0), Legacy::secs(g(DECAY), 4.0), 0.6, 0.15),
            AnalogPad => (0.4 + g(DURATION) * 2.0, 2.0, 0.85, 1.5),
            DigitalStepper => (0.005, 3.0, 0.8, 0.3),
            ChurchOrgan => (Legacy::secs(g(ATTACK), 1.0), 0.1, 1.0, Legacy::secs(g(RELEASE), 3.0)),
            SoundEffects => (0.01, 0.1, 1.0, Legacy::secs(g(RELEASE), 4.0)),
            AnalogSync | AnalogBasic => (Legacy::secs(g(ATTACK), 3.0), Legacy::secs(g(DECAY), 4.0), g(SUSTAIN), 0.25),
            _ => (Legacy::secs(g(ATTACK), 3.0), Legacy::secs(g(DECAY), 4.0), if matches!(var, DigitalBasic) { 0.6 } else { g(SUSTAIN) }, Legacy::secs(g(RELEASE), 4.0)),
        };
        let tune = 2f32.powf(g(TUNING) / 12.0);
        let glide = Legacy::secs(g(GLIDE), 1.5);
        let reso = g(RESONANCE);
        let wheel = self.wheel;
        // the stepper's steps
        let step_len = 0.06 + g(DURATION) * 0.5;
        let (layers_a, layers_b) = wave_layers(g(WAVEFORM).round() as usize);
        let reg = g(REGISTRATION).round() as usize;
        let bass = if bass_off(reg) { 0 } else { g(BASS).round() as usize };
        let lfo_hz = 0.1 + g(MODULATION) * 6.0;
        for i in 0..n {
            self.lfo = (self.lfo + lfo_hz * dt).fract();
            let lfo = (self.lfo * std::f32::consts::TAU).sin();
            self.step.0 += dt;
            if self.step.0 >= step_len {
                self.step.0 -= step_len;
                self.step.1 = (self.step.1 + 1) % 8;
            }
            let pattern = [0.0, 0.7, 0.3, 1.0, 0.5, 0.2, 0.9, 0.4][self.step.1];
            let mut out = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.on) {
                // envelopes
                v.t += dt;
                match v.stage {
                    0 => {
                        v.env += dt / att;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = 1;
                        }
                    }
                    1 => {
                        v.env += (sus - v.env) * (dt / dec * 3.0).min(1.0);
                    }
                    3 => {
                        v.env -= v.env * (dt / rel * 5.0).min(1.0) + 1e-6;
                        if v.env <= 1e-4 {
                            v.on = false;
                            continue;
                        }
                    }
                    _ => {}
                }
                v.fenv = (-v.t / (0.05 + g(DURATION).max(g(DECAY)) * 2.0)).exp();
                if v.hz != v.target_hz {
                    let k = (dt / glide).min(1.0);
                    v.hz += (v.target_hz - v.hz) * k;
                }
                let vib = 1.0 + if var == HybridBasic { wheel * g(WHEEL_VIB) * 0.02 * lfo } else { 0.0 };
                let hz = v.hz * tune * vib;
                let rnd = {
                    v.rng ^= v.rng << 13;
                    v.rng ^= v.rng >> 17;
                    v.rng ^= v.rng << 5;
                    v.rng as f32 / u32::MAX as f32 * 2.0 - 1.0
                };
                // the source
                let s = match var {
                    AnalogBasic | AnalogSwirl => {
                        let a = saw(adv(&mut v.ph, dt, 0, hz));
                        let b = square(adv(&mut v.ph, dt, 1, hz * 1.005), 0.5);
                        a * (1.0 - g(MIX)) + b * g(MIX)
                    }
                    AnalogMono => {
                        let a = saw(adv(&mut v.ph, dt, 0, hz));
                        let b = square(adv(&mut v.ph, dt, 1, hz * 0.5), 0.5);
                        let r = g(RICHNESS);
                        let c = saw(adv(&mut v.ph, dt, 2, hz * (1.0 + r * 0.012))) * r + saw(adv(&mut v.ph, dt, 3, hz * (1.0 - r * 0.012))) * r;
                        a * (1.0 - g(MIX)) + b * g(MIX) + c * 0.5
                    }
                    AnalogPad => {
                        let ch = g(CHARACTER);
                        let a = saw(adv(&mut v.ph, dt, 0, hz * (1.0 + 0.004 * lfo)));
                        let b = saw(adv(&mut v.ph, dt, 1, hz * (1.0 - 0.004 * lfo)));
                        let sq = square(adv(&mut v.ph, dt, 2, hz), 0.5 + 0.3 * lfo * g(ANIMATION));
                        (a + b) * 0.5 * ch + sq * (1.0 - ch) * 0.6
                    }
                    AnalogSync => {
                        let ratio = 1.0 + g(SYNC) * 6.0 + g(SYNC_MOD) * 2.0 * lfo + g(SYNC_ENV) * 6.0 * v.fenv;
                        let m = adv(&mut v.ph, dt, 0, hz);
                        if m < hz * dt {
                            v.ph[1] = 0.0;
                        }
                        saw(adv(&mut v.ph, dt, 1, hz * ratio.max(1.0)))
                    }
                    DigitalBasic | DigitalMono | DigitalStepper => {
                        let steps = if var == DigitalStepper { pattern * g(HARM_STEPS) } else { 0.0 };
                        let harm = (g(HARMONICS) + steps).clamp(0.0, 1.0);
                        let timbre = (g(TIMBRE) + if var == DigitalMono { g(TIMBRE_ENV) * v.vel * v.fenv } else { 0.0 }).clamp(0.0, 1.0);
                        let nh = 1 + (harm * 24.0) as usize;
                        let ph = adv(&mut v.ph, dt, 0, hz);
                        let mut s = 0.0;
                        for h in 1..=nh {
                            let amp = (1.0 / h as f32).powf(1.5 - timbre);
                            s += (ph * h as f32 * std::f32::consts::TAU).sin() * amp;
                        }
                        let tone2 = square(adv(&mut v.ph, dt, 1, hz * if var == DigitalBasic { 2.0 } else { 1.0 }), 0.3);
                        let r = if var == DigitalMono { g(RICHNESS) } else { 0.0 };
                        let thick = saw(adv(&mut v.ph, dt, 2, hz * (1.0 + r * 0.01))) * r * 0.5;
                        let mut s = match var {
                            DigitalBasic => s * (1.0 - g(MIX)) * 0.6 + tone2 * g(MIX) * 0.5,
                            DigitalStepper => s * 0.6 * (1.0 - g(BALANCE)) + saw(adv(&mut v.ph, dt, 3, hz)) * g(BALANCE) * 0.5,
                            _ => s * 0.6 + thick,
                        };
                        if var == DigitalMono && g(DISTORTION) > 0.0 {
                            s = (s * (1.0 + g(DISTORTION) * 12.0)).tanh();
                        }
                        if var == DigitalStepper {
                            s *= 1.0 + g(MODULATION) * 0.4 * lfo;
                        }
                        s
                    }
                    HybridBasic | HybridMorph => {
                        let morph = if var == HybridMorph {
                            let me = g(MORPH_ENV);
                            let env_m = if me >= 0.0 { me * (1.0 - v.fenv) } else { -me * v.fenv - 0.0 };
                            (g(MORPH) + env_m + wheel * 0.5).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let ph = adv(&mut v.ph, dt, 0, hz);
                        let mut s = 0.0;
                        for h in 0..32 {
                            if hz * (h + 1) as f32 > rate * 0.45 {
                                break;
                            }
                            let amp = layers_a[h] * (1.0 - morph) + layers_b[h] * morph;
                            if amp > 1e-4 {
                                s += (ph * (h + 1) as f32 * std::f32::consts::TAU).sin() * amp;
                            }
                        }
                        s * 0.5
                    }
                    ChurchOrgan => {
                        let mut s = 0.0;
                        for (k, &(mult, lv)) in ranks(reg).iter().enumerate().take(3) {
                            s += (adv(&mut v.ph, dt, k, hz * mult) * std::f32::consts::TAU).sin() * lv;
                        }
                        for &(mult, lv) in ranks(reg).iter().skip(3) {
                            s += (v.ph[0] * mult * std::f32::consts::TAU).sin() * lv;
                        }
                        // the bass pipes: an octave (and two) below, under C3
                        if bass > 0 && v.key < 60 {
                            s += (adv(&mut v.ph, dt, 3, hz * 0.5) * std::f32::consts::TAU).sin() * 0.6;
                            if bass > 1 {
                                s += (v.ph[3] * 0.5 * std::f32::consts::TAU).sin() * 0.4;
                            }
                        }
                        s * 0.35 + rnd * 0.004
                    }
                    SoundEffects => {
                        // the key picks the texture: wind, rain, surf, crowd, thunder, birds
                        match (v.key / 12) % 6 {
                            0 => rnd * (0.4 + 0.3 * lfo),
                            1 => if rnd > 0.96 { rnd * 2.0 } else { rnd * 0.1 },
                            2 => rnd * (0.5 + 0.5 * (v.t * 0.3).sin().abs()),
                            3 => rnd * (0.3 + 0.7 * ((v.t * 9.0 + (v.key as f32)).sin() * 0.5 + 0.5)),
                            4 => rnd * (-v.t * 0.8).exp() * 1.5,
                            _ => (adv(&mut v.ph, dt, 0, 2400.0 + 900.0 * (v.t * 18.0).sin()) * std::f32::consts::TAU).sin() * if (v.t * 6.0).fract() < 0.3 { 0.6 } else { 0.0 },
                        }
                    }
                };
                // the filter
                let mut cut = g(CUTOFF);
                cut += match var {
                    AnalogPad => g(CUTOFF_ENV) * v.fenv * 0.5 + 0.1 * lfo * g(ANIMATION),
                    AnalogSwirl => 0.1 * lfo,
                    HybridMorph => g(CUTOFF_ENV) * v.fenv * 0.5,
                    HybridBasic => {
                        let ca = (v.t / (0.01 + g(CUTOFF_ATTACK) * 2.0)).min(1.0);
                        (ca - 1.0) * 0.4 + wheel * g(WHEEL_CUT) * 0.4
                    }
                    DigitalStepper => (pattern - 0.5) * g(CUTOFF_STEPS) * 0.6,
                    _ => 0.0,
                };
                let filtered = if matches!(var, DigitalBasic | DigitalMono) {
                    s
                } else {
                    let hz_c = 40.0 * 2f32.powf(cut.clamp(0.0, 1.0) * 9.0);
                    let q = 1.0 - reso.min(0.95);
                    let f = (2.0 * (std::f32::consts::PI * hz_c.min(rate * 0.2) / rate).sin()).min(1.85 - q * 1.4).max(0.001);
                    let (lp, bp, hp) = v.f[0].tick(s, f, q * 1.4);
                    match g(CUTOFF_TYPE).round() as i32 {
                        1 if matches!(var, HybridBasic | HybridMorph) => v.f[1].tick(lp, f, q * 1.4).0,
                        2 if matches!(var, HybridBasic | HybridMorph) => bp,
                        3 if matches!(var, HybridBasic | HybridMorph) => hp,
                        4 if matches!(var, HybridBasic | HybridMorph) => lp + bp * 0.8,
                        _ => lp,
                    }
                };
                out += filtered * v.env * (0.4 + 0.6 * v.vel);
            }
            // Analog Swirl's chorus
            if var == AnalogSwirl {
                let len = self.chorus.len();
                self.chorus[self.chorus_at] = out;
                let d = (self.rate * (0.012 + 0.006 * lfo)) as usize;
                let tap = self.chorus[(self.chorus_at + len - d.min(len - 1)) % len];
                self.chorus_at = (self.chorus_at + 1) % len;
                left[i] += (out + tap) * 0.5 * vol;
                right[i] += (out - tap * 0.6) * 0.5 * vol;
            } else {
                left[i] += out * vol * 0.5;
                right[i] += out * vol * 0.5;
            }
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
