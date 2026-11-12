//! Drum Synth, as Logic's guide describes it: an electronic drum synthesizer with four groups —
//! Kicks, Snares and Claps, Percussion, Hats and Cymbals — each with sound types that show up
//! to eight of their own controls next to Pitch, Decay and Volume; Key Tracking (chromatic, or
//! the root pitch on every key) and the Mono / Poly / Gate modes. The synthesis is Studio's
//! own: a swept sine kick with a beater click and saturation, two-mode snare shells with wire
//! noise and smeared claps, modal percussion whose partials follow Material, Tension and
//! Dissonance, and six-oscillator metallic hats and cymbals. The sound types and their values
//! are Studio's (not Logic's presets).

use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const GROUPS: &[&str] = &["Kicks", "Snares and Claps", "Percussion", "Hats and Cymbals"];
pub const MODES: &[&str] = &["Mono", "Poly", "Gate"];

pub const GROUP: u32 = 0;
pub const SOUND: u32 = 1;
pub const KEY_TRACK: u32 = 2;
pub const ROOT: u32 = 3;
pub const MODE: u32 = 4;
pub const PITCH: u32 = 5;
pub const DECAY: u32 = 6;
pub const TONE: u32 = 7;
pub const VOLUME: u32 = 8;
// the sound-specific controls
pub const ATTACK: u32 = 10;
pub const BODY: u32 = 11;
pub const COLOR: u32 = 12;
pub const CRUSH: u32 = 13;
pub const DIRT: u32 = 14;
pub const DISSONANCE: u32 = 15;
pub const HARMONIC: u32 = 16;
pub const MATERIAL: u32 = 17;
pub const NOISE: u32 = 18;
pub const SATURATION: u32 = 19;
pub const SNAP: u32 = 20;
pub const TENSION: u32 = 21;
pub const BELL: u32 = 22;
pub const CHARACTER: u32 = 23;
pub const METALLIC: u32 = 24;
pub const CYCLES: u32 = 25;
pub const POSITION: u32 = 26;
pub const SIZE: u32 = 27;
pub const SPEED: u32 = 28;
pub const PUNCH: u32 = 29;
pub const SHAPE: u32 = 30;
pub const SWEEP: u32 = 31;

/// How a sound is made within its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Kick,
    Snare,
    Rim,
    Clap,
    Tom,
    Perc,
    Metal,
    Hat,
    OpenHat,
    Cymbal,
    Shaker,
}

/// A sound type: its group, model, the controls it shows (besides Pitch, Decay, Volume) and
/// its starting values (id, value).
pub struct Sound {
    pub name: &'static str,
    pub group: usize,
    pub model: Model,
    pub controls: &'static [u32],
    pub values: &'static [(u32, f32)],
}

const KICK_C: &[u32] = &[TONE, BODY, PUNCH, SNAP, SWEEP, SHAPE, NOISE, SATURATION];
const SNARE_C: &[u32] = &[TONE, BODY, TENSION, SIZE, POSITION, NOISE, DIRT, CRUSH];
const CLAP_C: &[u32] = &[TONE, CYCLES, SPEED, BODY, NOISE, DIRT, CRUSH, TENSION];
const PERC_C: &[u32] = &[TONE, MATERIAL, TENSION, DISSONANCE, HARMONIC, BODY, ATTACK, COLOR];
const PERC2_C: &[u32] = &[TONE, MATERIAL, TENSION, SNAP, NOISE, SATURATION, CRUSH, DIRT];
const HAT_C: &[u32] = &[TONE, METALLIC, CHARACTER, DISSONANCE, BELL, ATTACK, NOISE, BODY];
const HAT2_C: &[u32] = &[TONE, METALLIC, CHARACTER, NOISE, ATTACK, CRUSH, DIRT, BODY];

pub static SOUNDS: &[Sound] = &[
    Sound { name: "Round Kick", group: 0, model: Model::Kick, controls: KICK_C, values: &[(PITCH, 0.0), (DECAY, 0.5), (TONE, 0.4), (BODY, 0.6), (PUNCH, 0.5), (SNAP, 0.2), (SWEEP, 0.5), (SHAPE, 0.0)] },
    Sound { name: "Boom Kick", group: 0, model: Model::Kick, controls: KICK_C, values: &[(PITCH, -3.0), (DECAY, 0.85), (TONE, 0.3), (BODY, 0.8), (PUNCH, 0.3), (SNAP, 0.1), (SWEEP, 0.35), (SATURATION, 0.2)] },
    Sound { name: "Tight Kick", group: 0, model: Model::Kick, controls: KICK_C, values: &[(PITCH, 4.0), (DECAY, 0.25), (TONE, 0.6), (BODY, 0.4), (PUNCH, 0.8), (SNAP, 0.6), (SWEEP, 0.7)] },
    Sound { name: "Dirty Kick", group: 0, model: Model::Kick, controls: KICK_C, values: &[(PITCH, 0.0), (DECAY, 0.5), (TONE, 0.6), (BODY, 0.5), (PUNCH, 0.6), (SNAP, 0.4), (SWEEP, 0.5), (SATURATION, 0.7), (NOISE, 0.15), (SHAPE, 0.4)] },
    Sound { name: "Analog Snare", group: 1, model: Model::Snare, controls: SNARE_C, values: &[(DECAY, 0.4), (TONE, 0.5), (BODY, 0.5), (TENSION, 0.5), (SIZE, 0.5), (POSITION, 0.3), (NOISE, 0.6)] },
    Sound { name: "Fat Snare", group: 1, model: Model::Snare, controls: SNARE_C, values: &[(PITCH, -3.0), (DECAY, 0.6), (TONE, 0.4), (BODY, 0.8), (TENSION, 0.4), (SIZE, 0.75), (POSITION, 0.2), (NOISE, 0.5), (DIRT, 0.2)] },
    Sound { name: "Rim", group: 1, model: Model::Rim, controls: SNARE_C, values: &[(DECAY, 0.15), (TONE, 0.7), (BODY, 0.3), (TENSION, 0.7), (SIZE, 0.3), (POSITION, 0.8), (NOISE, 0.1)] },
    Sound { name: "Hand Clap", group: 1, model: Model::Clap, controls: CLAP_C, values: &[(DECAY, 0.45), (TONE, 0.5), (CYCLES, 0.4), (SPEED, 0.5), (BODY, 0.3), (NOISE, 1.0), (TENSION, 0.5)] },
    Sound { name: "Big Clap", group: 1, model: Model::Clap, controls: CLAP_C, values: &[(DECAY, 0.7), (TONE, 0.4), (CYCLES, 0.8), (SPEED, 0.3), (BODY, 0.5), (NOISE, 1.0), (DIRT, 0.3), (TENSION, 0.4)] },
    Sound { name: "Floor Tom", group: 2, model: Model::Tom, controls: PERC_C, values: &[(PITCH, -7.0), (DECAY, 0.6), (TONE, 0.4), (MATERIAL, 0.2), (TENSION, 0.4), (BODY, 0.7), (ATTACK, 0.0)] },
    Sound { name: "High Tom", group: 2, model: Model::Tom, controls: PERC_C, values: &[(PITCH, 5.0), (DECAY, 0.45), (TONE, 0.5), (MATERIAL, 0.2), (TENSION, 0.5), (BODY, 0.5)] },
    Sound { name: "Wood Block", group: 2, model: Model::Perc, controls: PERC2_C, values: &[(PITCH, 12.0), (DECAY, 0.15), (TONE, 0.6), (MATERIAL, 0.1), (TENSION, 0.6), (SNAP, 0.5)] },
    Sound { name: "Conga", group: 2, model: Model::Perc, controls: PERC2_C, values: &[(PITCH, 2.0), (DECAY, 0.35), (TONE, 0.5), (MATERIAL, 0.4), (TENSION, 0.5), (SNAP, 0.3)] },
    Sound { name: "Cowbell", group: 2, model: Model::Metal, controls: PERC_C, values: &[(PITCH, 7.0), (DECAY, 0.3), (TONE, 0.6), (MATERIAL, 0.8), (TENSION, 0.5), (DISSONANCE, 0.3), (HARMONIC, 0.4)] },
    Sound { name: "Bell Tone", group: 2, model: Model::Metal, controls: PERC_C, values: &[(PITCH, 12.0), (DECAY, 0.75), (TONE, 0.6), (MATERIAL, 1.0), (TENSION, 0.6), (DISSONANCE, 0.2), (HARMONIC, 0.6), (COLOR, 0.6)] },
    Sound { name: "Closed Hat", group: 3, model: Model::Hat, controls: HAT_C, values: &[(DECAY, 0.15), (TONE, 0.7), (METALLIC, 0.6), (CHARACTER, 0.4), (NOISE, 0.5)] },
    Sound { name: "Open Hat", group: 3, model: Model::OpenHat, controls: HAT_C, values: &[(DECAY, 0.55), (TONE, 0.7), (METALLIC, 0.6), (CHARACTER, 0.4), (NOISE, 0.5)] },
    Sound { name: "Ride", group: 3, model: Model::Cymbal, controls: HAT_C, values: &[(PITCH, 3.0), (DECAY, 0.8), (TONE, 0.6), (METALLIC, 0.8), (BELL, 0.5), (CHARACTER, 0.3), (NOISE, 0.3)] },
    Sound { name: "Crash", group: 3, model: Model::Cymbal, controls: HAT_C, values: &[(DECAY, 0.9), (TONE, 0.75), (METALLIC, 0.5), (BELL, 0.1), (CHARACTER, 0.6), (NOISE, 0.8), (DISSONANCE, 0.5)] },
    Sound { name: "Shaker", group: 3, model: Model::Shaker, controls: HAT2_C, values: &[(DECAY, 0.2), (TONE, 0.8), (METALLIC, 0.0), (NOISE, 1.0), (ATTACK, 0.4)] },
];

/// The sound types of a group (indexes into SOUNDS).
pub fn sounds_of(group: usize) -> Vec<usize> {
    SOUNDS.iter().enumerate().filter(|(_, s)| s.group == group).map(|(i, _)| i).collect()
}

fn names() -> &'static [&'static str] {
    static N: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    N.get_or_init(|| SOUNDS.iter().map(|s| s.name).collect()).as_slice()
}

pub fn params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![
            Param::menu(GROUP, "Group", GROUPS, 0.0),
            Param::menu(SOUND, "Sound", names(), 0.0),
            Param::new(KEY_TRACK, "Key Tracking", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(ROOT, "Root Key", 0.0, 127.0, 60.0, Unit::None),
            Param::menu(MODE, "Mode", MODES, 1.0),
            Param::new(PITCH, "Pitch", -24.0, 24.0, 0.0, Unit::Semitones),
            Param::new(DECAY, "Decay", 0.0, 1.0, 0.5, Unit::Ratio),
            Param::new(TONE, "Tone", 0.0, 1.0, 0.5, Unit::Ratio),
            Param::new(VOLUME, "Volume", -40.0, 6.0, 0.0, Unit::Db),
        ];
        let knobs = [
            (ATTACK, "Attack"),
            (BODY, "Body"),
            (COLOR, "Color"),
            (CRUSH, "Crush"),
            (DIRT, "Dirt"),
            (DISSONANCE, "Dissonance"),
            (HARMONIC, "Harmonic"),
            (MATERIAL, "Material"),
            (NOISE, "Noise"),
            (SATURATION, "Saturation"),
            (SNAP, "Snap"),
            (TENSION, "Tension"),
            (BELL, "Bell"),
            (CHARACTER, "Character"),
            (METALLIC, "Metallic"),
            (CYCLES, "Cycles"),
            (POSITION, "Position"),
            (SIZE, "Size"),
            (SPEED, "Speed"),
            (PUNCH, "Punch"),
            (SHAPE, "Shape"),
            (SWEEP, "Sweep"),
        ];
        for (id, n) in knobs {
            v.push(Param::new(id, n, 0.0, 1.0, 0.0, Unit::Ratio));
        }
        v
    })
    .as_slice()
}

/// The values a sound type starts with (Back to Default).
pub fn defaults_of(sound: usize) -> Vec<(u32, f32)> {
    let s = &SOUNDS[sound.min(SOUNDS.len() - 1)];
    let mut v: Vec<(u32, f32)> = params().iter().filter(|p| p.id >= PITCH).map(|p| (p.id, p.default)).collect();
    for &(id, x) in s.values {
        if let Some(e) = v.iter_mut().find(|e| e.0 == id) {
            e.1 = x;
        }
    }
    v.push((GROUP, s.group as f32));
    v.push((SOUND, sound as f32));
    v
}

/// The control's value as the engine reads it (0 when the sound type doesn't show it).
#[derive(Clone, Copy, Default)]
struct K {
    v: [f32; 32],
}

impl K {
    fn g(&self, id: u32) -> f32 {
        self.v[id as usize]
    }
}

#[derive(Clone, Copy)]
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// A state-variable filter (lowpass / bandpass / highpass outputs).
#[derive(Clone, Copy, Default)]
struct Svf {
    lp: f32,
    bp: f32,
}
impl Svf {
    fn tick(&mut self, x: f32, f: f32, q: f32) -> (f32, f32, f32) {
        let f = f.clamp(0.0, 1.2);
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
    t: u32,
    ph: [f64; 6],
    rng: Rng,
    f1: Svf,
    f2: Svf,
    rel: f32,
    released: bool,
    hold: f32,
    crush_n: u32,
}

impl Voice {
    fn new() -> Voice {
        Voice { on: false, key: 60, vel: 1.0, t: 0, ph: [0.0; 6], rng: Rng(0x1234_5678), f1: Svf::default(), f2: Svf::default(), rel: 1.0, released: false, hold: 0.0, crush_n: 0 }
    }
}

pub struct DrumSynth {
    params: crate::poly::Params,
    k: K,
    voices: [Voice; 8],
    rate: f32,
    seed: u32,
}

impl DrumSynth {
    pub fn new(rate: f32) -> DrumSynth {
        let mut d = DrumSynth { params: crate::poly::Params::new(params()), k: K::default(), voices: [Voice::new(); 8], rate, seed: 1 };
        for (id, v) in defaults_of(0) {
            d.params.set(id, v);
        }
        d.update();
        d
    }
    /// A Drum Synth with a sound type chosen.
    pub fn with_sound(rate: f32, sound: usize) -> DrumSynth {
        let mut d = DrumSynth::new(rate);
        for (id, v) in defaults_of(sound) {
            d.params.set(id, v);
        }
        d.update();
        d
    }
    pub fn sound(&self) -> &'static Sound {
        &SOUNDS[(self.params.get(SOUND).round().max(0.0) as usize).min(SOUNDS.len() - 1)]
    }
    fn update(&mut self) {
        let s = self.sound();
        let mut k = K::default();
        for p in params() {
            // a control the sound type doesn't show stays out of its sound
            let shown = p.id < ATTACK || s.controls.contains(&p.id);
            k.v[p.id as usize] = if shown { self.params.get(p.id) } else { 0.0 };
        }
        // Tone is shown by every type here; keep its value
        k.v[TONE as usize] = self.params.get(TONE);
        self.k = k;
    }
    /// One sample of a voice.
    fn sample(k: &K, model: Model, v: &mut Voice, rate: f32, freq_ratio: f32) -> f32 {
        let t = v.t as f32 / rate;
        let dt = 1.0 / rate;
        let decay = k.g(DECAY);
        let tone = k.g(TONE);
        let n = v.rng.next();
        let tau = std::f32::consts::TAU;
        let osc = |ph: &mut f64, hz: f32| -> f32 {
            *ph = (*ph + (hz * dt) as f64).fract();
            (*ph as f32 * tau).sin()
        };
        let mut out = match model {
            Model::Kick => {
                let base = 50.0 * freq_ratio;
                let sweep = 1.0 + k.g(SWEEP) * 5.0 * (-t / (0.008 + 0.03 * k.g(SWEEP))).exp();
                let mut s = osc(&mut v.ph[0], base * sweep);
                // Shape: from a sine toward a rounded triangle / square
                let sh = k.g(SHAPE);
                if sh > 0.0 {
                    s = ((1.0 + sh * 6.0) * s).tanh() / (1.0 + sh * 6.0).tanh();
                }
                let body_env = (-t / (0.08 + decay * 1.2 * (0.5 + k.g(BODY)))).exp();
                let sub = osc(&mut v.ph[1], base * 0.5) * k.g(BODY) * 0.4 * body_env;
                let punch = (-t / 0.012).exp() * k.g(PUNCH) * 0.8;
                let click = {
                    let (_, bp, hp) = v.f1.tick(n, 0.6, 0.7);
                    (hp * 0.6 + bp * 0.4) * (-t / 0.004).exp() * k.g(SNAP)
                };
                let noise = n * k.g(NOISE) * 0.25 * (-t / (0.03 + decay * 0.2)).exp();
                s * body_env * (1.0 + punch) + sub + click + noise
            }
            Model::Snare | Model::Rim => {
                let size = k.g(SIZE);
                let base = (if model == Model::Rim { 420.0 } else { 185.0 }) * freq_ratio * (1.3 - size * 0.6);
                let pos = k.g(POSITION);
                let shell_env = (-t / ((0.03 + decay * 0.25) * (0.6 + size))).exp();
                let m1 = osc(&mut v.ph[0], base * (1.0 + 0.4 * (-t / 0.01).exp()));
                let m2 = osc(&mut v.ph[1], base * 1.59) * (0.3 + pos * 0.7);
                let shell = (m1 * (1.0 - pos * 0.5) + m2 * 0.6) * shell_env * (0.4 + k.g(BODY));
                // wires: noise, brighter and snappier with Tension
                let ten = k.g(TENSION);
                let (_, bp, hp) = v.f1.tick(n, 0.3 + ten * 0.6, 0.6);
                let wires = (hp * (0.4 + ten * 0.6) + bp * 0.5) * (-t / (0.04 + decay * 0.35 * (1.2 - ten * 0.5))).exp() * k.g(NOISE);
                shell + wires * if model == Model::Rim { 0.3 } else { 1.0 }
            }
            Model::Clap => {
                // smeared bursts: Cycles of them, Speed apart, then the tail
                let cycles = 1 + (k.g(CYCLES) * 6.0).round() as u32;
                let gap = 0.004 + (1.0 - k.g(SPEED)) * 0.02;
                let k_i = (t / gap) as u32;
                let burst = if k_i < cycles { (-(t - k_i as f32 * gap) / 0.003).exp() } else { 0.0 };
                let tail = (-(t - cycles as f32 * gap).max(0.0) / (0.05 + decay * 0.4)).exp() * if k_i >= cycles { 1.0 } else { 0.0 };
                let (_, bp, _) = v.f1.tick(n, 0.25 + k.g(TENSION) * 0.3 + 0.1 * k.g(BODY), 0.35);
                bp * (burst + tail * 0.7) * k.g(NOISE).max(0.2) * 2.0
            }
            Model::Tom | Model::Perc | Model::Metal => {
                // modal partials: Material sets the ratios (wood, skin, rubber, bell), Tension
                // stretches them, Dissonance detunes, Harmonic adds the upper ones
                let mat = k.g(MATERIAL);
                let ten = k.g(TENSION);
                let base = match model {
                    Model::Tom => 110.0,
                    Model::Perc => 260.0,
                    _ => 520.0,
                } * freq_ratio;
                let membrane = [1.0, 1.59, 2.14, 2.30, 2.65, 2.92];
                let bell = [1.0, 2.0, 2.4, 3.0, 4.07, 5.43];
                let mut s = 0.0;
                let attack = (t / (0.0005 + k.g(ATTACK) * 0.03)).min(1.0);
                for (i, (a, b)) in membrane.iter().zip(bell.iter()).enumerate() {
                    let r = a + (b - a) * mat;
                    let r = r.powf(1.0 + (ten - 0.5) * 0.3) * (1.0 + k.g(DISSONANCE) * 0.07 * (i as f32 * 1.7).sin());
                    let amp = if i == 0 { 1.0 + k.g(BODY) } else { (0.5 + k.g(HARMONIC)) / (i as f32 + 0.5) * (0.3 + mat) };
                    let d = (0.04 + decay * (0.3 + mat * 1.6)) / (1.0 + i as f32 * (1.0 - mat) * 0.8);
                    s += osc(&mut v.ph[i], base * r * (1.0 + 0.1 * (-t / 0.02).exp() * (1.0 - mat))) * amp * (-t / d).exp();
                }
                let snap = n * (-t / 0.003).exp() * k.g(SNAP);
                let noise = n * k.g(NOISE) * 0.3 * (-t / (0.02 + decay * 0.2)).exp();
                // Color: a tilt toward the upper partials
                let (lp, _, hp) = v.f2.tick(s, 0.3, 0.8);
                let colored = lp * (1.0 - k.g(COLOR)) + (hp + lp) * k.g(COLOR);
                (colored * 0.5 + s * 0.5) * attack * 0.6 + snap + noise
            }
            Model::Hat | Model::OpenHat | Model::Cymbal | Model::Shaker => {
                // six square oscillators at metallic ratios with the noise; Dissonance moves the
                // ratios, Bell adds a tuned ring, Character ring-modulates
                let ratios = [1.0, 1.304, 1.466, 1.787, 1.932, 2.536];
                let base = (if model == Model::Cymbal { 320.0 } else { 400.0 }) * freq_ratio;
                let mut sq = 0.0;
                for (i, r) in ratios.iter().enumerate() {
                    let r = r * (1.0 + k.g(DISSONANCE) * 0.12 * (i as f32 * 2.3).cos());
                    v.ph[i] = (v.ph[i] + (base * r * dt) as f64).fract();
                    sq += if v.ph[i] < 0.5 { 1.0 } else { -1.0 };
                }
                sq /= 6.0;
                let ring = sq * (v.ph[0] as f32 * tau * 3.0).sin();
                let metal = sq * (1.0 - k.g(CHARACTER)) + ring * k.g(CHARACTER);
                let mix = metal * k.g(METALLIC) + n * (k.g(NOISE) + if model == Model::Shaker { 0.6 } else { 0.0 }) * 0.8;
                let (_, bp, hp) = v.f1.tick(mix, 0.5 + tone * 0.6, 0.5);
                let body = bp * k.g(BODY);
                let bell = osc(&mut v.ph[5], base * 2.0) * k.g(BELL) * 0.3;
                let len = match model {
                    Model::Hat => 0.02 + decay * 0.12,
                    Model::OpenHat => 0.1 + decay * 0.7,
                    Model::Shaker => 0.03 + decay * 0.15,
                    _ => 0.3 + decay * 2.5,
                };
                let attack = (t / (0.0005 + k.g(ATTACK) * if model == Model::Shaker { 0.04 } else { 0.01 })).min(1.0);
                (hp + body + bell) * (-t / len).exp() * attack
            }
        };
        // Tone: an overall lowpass (gentle at the top)
        if !matches!(model, Model::Hat | Model::OpenHat | Model::Cymbal | Model::Shaker) {
            let f = 0.05 + tone * tone * 1.1;
            let (lp, _, _) = v.f2.tick(out, f, 1.0);
            out = lp * (1.0 - tone * 0.3) + out * tone * 0.3;
        }
        // Dirt: grit (noise riding the signal); Saturation: warm; Crush: lo-fi steps
        let dirt = k.g(DIRT);
        if dirt > 0.0 {
            out += out.abs() * n * dirt * 0.6;
        }
        let sat = k.g(SATURATION);
        if sat > 0.0 {
            out = (out * (1.0 + sat * 6.0)).tanh() / (1.0 + sat * 2.0).sqrt();
        }
        let crush = k.g(CRUSH);
        if crush > 0.0 {
            let hold = 1 + (crush * 12.0) as u32;
            if v.crush_n == 0 {
                let steps = 2f32.powf(12.0 - crush * 9.0);
                v.hold = (out * steps).round() / steps;
            }
            v.crush_n = (v.crush_n + 1) % hold;
            out = v.hold;
        }
        v.t += 1;
        out
    }
}

impl Plugin for DrumSynth {
    fn params(&self) -> &[Param] {
        params()
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if !self.params.set(id, v) {
            return;
        }
        if id == GROUP {
            // a new group: its first sound type with its values
            let g = self.params.get(GROUP).round() as usize;
            if self.sound().group != g
                && let Some(&s) = sounds_of(g).first()
            {
                for (pid, x) in defaults_of(s) {
                    self.params.set(pid, x);
                }
            }
        } else if id == SOUND {
            let g = self.sound().group;
            self.params.set(GROUP, g as f32);
        }
        self.update();
    }
    fn reset(&mut self) {
        for v in &mut self.voices {
            v.on = false;
        }
    }
    fn tail(&self) -> f32 {
        3.0
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        (id == ROOT).then(|| crate::sampler_doc::note_name(v.round().clamp(0.0, 127.0) as u8))
    }
}

impl Instrument for DrumSynth {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let mode = self.params.get(MODE).round() as i32;
        let slot = if mode != 1 {
            // Mono and Gate: one voice, the new note cuts the last
            for v in self.voices.iter_mut().skip(1) {
                v.on = false;
            }
            0
        } else {
            self.voices.iter().position(|v| !v.on).unwrap_or_else(|| self.voices.iter().enumerate().max_by_key(|(_, v)| v.t).map(|(i, _)| i).unwrap_or(0))
        };
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut v = Voice::new();
        v.on = true;
        v.key = pitch;
        v.vel = velocity as f32 / 127.0;
        v.rng = Rng(self.seed | 1);
        self.voices[slot] = v;
    }
    fn note_off(&mut self, pitch: u8) {
        if self.params.get(MODE).round() as i32 == 2 {
            for v in &mut self.voices {
                if v.on && v.key == pitch {
                    v.released = true;
                }
            }
        }
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let k = self.k;
        let model = self.sound().model;
        let gain = lg_dsp::db_to_gain(self.params.get(VOLUME)) * 0.8;
        let track = self.params.get(KEY_TRACK) >= 0.5;
        let root = self.params.get(ROOT);
        let pitch = self.params.get(PITCH);
        let rel_step = 1.0 / (0.01 * self.rate);
        for v in self.voices.iter_mut().filter(|v| v.on) {
            let semis = pitch + if track { v.key as f32 - root } else { 0.0 };
            let ratio = 2f32.powf(semis / 12.0);
            for i in 0..left.len().min(right.len()) {
                let s = DrumSynth::sample(&k, model, v, self.rate, ratio) * v.vel * gain * v.rel;
                left[i] += s;
                right[i] += s;
                if v.released {
                    v.rel -= rel_step;
                    if v.rel <= 0.0 {
                        v.on = false;
                        break;
                    }
                }
            }
            if v.t as f32 > self.rate * 6.0 {
                v.on = false;
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
