//! EFM1 stand-in ("FM Two"): a multiwave modulator and a sine carrier, each on one of the
//! first 32 harmonics with a fine detune, FM Intensity, a modulation envelope to FM depth and
//! modulator pitch, an LFO to FM or vibrato, a sine sub-oscillator, Stereo Detune (a second
//! detuned FM engine on the other side), the volume envelope, Unison, Glide, mono / legato /
//! up to 16 voices, and controller assignments for FM and vibrato.
//! Parameter ids are Studio's own (Logic's float block for EFM1 isn't decoded).

use crate::dsp::{self, sin_cycles, Adsr, Fade, Rng, CHUNK};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::synthkit::{self as kit, Lfo, Shape, Stack, CTRLS, CTRL_AT};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const TRANSPOSE: &[&str] = &["−2 Oct", "−1 Oct", "−11", "−10", "−9", "−8", "−7", "−6", "−5", "−4", "−3", "−2", "−1", "0", "+1", "+2", "+3", "+4", "+5", "+6", "+7", "+8", "+9", "+10", "+11", "+1 Oct", "+2 Oct"];
pub const VOICES: &[&str] = &["Mono", "Legato", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16"];
pub const MONO_MODES: &[&str] = &["Off", "On (with common base channel 1)", "On (with common base channel 16)"];

pub const TRANSPOSE_P: u32 = 0;
pub const TUNE: u32 = 1;
pub const VOICES_P: u32 = 2;
pub const UNISON: u32 = 3;
pub const GLIDE: u32 = 4;
pub const MOD_HARMONIC: u32 = 5;
pub const MOD_FINE: u32 = 6;
pub const MOD_WAVE: u32 = 7;
pub const CAR_HARMONIC: u32 = 8;
pub const CAR_FINE: u32 = 9;
pub const FIXED: u32 = 10;
pub const FM: u32 = 11;
pub const ME_A: u32 = 12;
pub const ME_D: u32 = 13;
pub const ME_S: u32 = 14;
pub const ME_R: u32 = 15;
pub const MOD_PITCH: u32 = 16;
pub const FM_DEPTH: u32 = 17;
pub const LFO: u32 = 18;
pub const RATE: u32 = 19;
pub const SUB: u32 = 20;
pub const STEREO: u32 = 21;
pub const VE_A: u32 = 22;
pub const VE_D: u32 = 23;
pub const VE_S: u32 = 24;
pub const VE_R: u32 = 25;
pub const LEVEL: u32 = 26;
pub const VELOCITY: u32 = 27;
pub const RANDOMIZE: u32 = 28;
pub const CTRL_FM: u32 = 29;
pub const CTRL_FM_AMT: u32 = 30;
pub const CTRL_VIB: u32 = 31;
pub const CTRL_VIB_AMT: u32 = 32;
pub const MONO_MODE: u32 = 33;
pub const MONO_BEND: u32 = 34;
pub const BEND: u32 = 35;
const N: usize = 36;

pub static PARAMS: &[Param] = &[
    Param::menu(TRANSPOSE_P, "Transpose", TRANSPOSE, 13.0),
    Param::new(TUNE, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
    Param::menu(VOICES_P, "Voices", VOICES, 16.0),
    Param::new(UNISON, "Unison", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(GLIDE, "Glide", 0.0, 1.0, 0.0, Unit::None),
    Param::new(MOD_HARMONIC, "Modulator Harmonic", 1.0, 32.0, 1.0, Unit::None),
    Param::new(MOD_FINE, "Modulator Fine", -0.5, 0.5, 0.0, Unit::None),
    Param::new(MOD_WAVE, "Modulator Wave", 0.0, 1.0, 0.0, Unit::None),
    Param::new(CAR_HARMONIC, "Carrier Harmonic", 1.0, 32.0, 1.0, Unit::None),
    Param::new(CAR_FINE, "Carrier Fine", -0.5, 0.5, 0.0, Unit::None),
    Param::new(FIXED, "Fixed Carrier", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(FM, "FM Intensity", 0.0, 1.0, 0.35, Unit::None),
    Param::new(ME_A, "Mod Env Attack", 0.0, 1.0, 0.0, Unit::None),
    Param::new(ME_D, "Mod Env Decay", 0.0, 1.0, 0.55, Unit::None),
    Param::new(ME_S, "Mod Env Sustain", 0.0, 1.0, 0.2, Unit::None),
    Param::new(ME_R, "Mod Env Release", 0.0, 1.0, 0.4, Unit::None),
    Param::new(MOD_PITCH, "Modulator Pitch", -1.0, 1.0, 0.0, Unit::None),
    Param::new(FM_DEPTH, "FM Depth", -1.0, 1.0, 0.4, Unit::None),
    Param::new(LFO, "LFO", -1.0, 1.0, 0.0, Unit::None),
    Param::new(RATE, "Rate", 0.1, 30.0, 5.0, Unit::Hz),
    Param::new(SUB, "Sub Osc Level", 0.0, 1.0, 0.0, Unit::None),
    Param::new(STEREO, "Stereo Detune", 0.0, 1.0, 0.0, Unit::None),
    Param::new(VE_A, "Vol Env Attack", 0.0, 1.0, 0.0, Unit::None),
    Param::new(VE_D, "Vol Env Decay", 0.0, 1.0, 0.6, Unit::None),
    Param::new(VE_S, "Vol Env Sustain", 0.0, 1.0, 0.7, Unit::None),
    Param::new(VE_R, "Vol Env Release", 0.0, 1.0, 0.4, Unit::None),
    Param::new(LEVEL, "Main Level", -40.0, 6.0, -6.0, Unit::Db),
    Param::new(VELOCITY, "Velocity", 0.0, 1.0, 0.5, Unit::None),
    Param::new(RANDOMIZE, "Randomize", 0.0, 1.0, 0.1, Unit::None),
    Param::menu(CTRL_FM, "Ctrl FM", CTRLS, 0.0),
    Param::new(CTRL_FM_AMT, "Ctrl FM Amount", 0.0, 1.0, 0.5, Unit::None),
    Param::menu(CTRL_VIB, "Ctrl Vibrato", CTRLS, 1.0),
    Param::new(CTRL_VIB_AMT, "Ctrl Vibrato Amount", 0.0, 1.0, 0.3, Unit::None),
    Param::menu(MONO_MODE, "MIDI Mono Mode", MONO_MODES, 0.0),
    Param::new(MONO_BEND, "Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones),
    Param::new(BEND, "Pitch Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
];

/// The parameters Randomize changes (the sound's character, not its level or tuning).
pub const RANDOMIZED: &[u32] = &[MOD_HARMONIC, MOD_FINE, MOD_WAVE, CAR_HARMONIC, CAR_FINE, FM, ME_A, ME_D, ME_S, ME_R, MOD_PITCH, FM_DEPTH, LFO, RATE, SUB, STEREO, VE_A, VE_D, VE_S, VE_R];

/// Randomize: each of the key parameters moved from where it is by up to `amount` of its
/// range (the harmonics stay whole numbers). `seed` makes each click a new variation.
pub fn randomized(values: &dyn Fn(u32) -> f32, amount: f32, seed: u32) -> Vec<(u32, f32)> {
    let mut rng = Rng::new(seed.wrapping_mul(2654435761).max(1));
    RANDOMIZED.iter().map(|&id| {
        let p = &PARAMS[id as usize];
        let v = values(id) + rng.bipolar() * amount * (p.max - p.min);
        let v = v.clamp(p.min, p.max);
        (id, if id == MOD_HARMONIC || id == CAR_HARMONIC { v.round() } else { v })
    }).collect()
}

/// The modulator's waves: a sine at the far left, then richer digital waves, as harmonics
/// (number, level).
const MOD_WAVES: [[(f32, f32); 4]; 8] = [
    [(1.0, 1.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)],
    [(1.0, 1.0), (2.0, 0.35), (3.0, 0.0), (4.0, 0.0)],
    [(1.0, 1.0), (3.0, 0.4), (5.0, 0.15), (7.0, 0.0)],
    [(1.0, 1.0), (2.0, 0.5), (3.0, 0.33), (4.0, 0.25)],
    [(1.0, 0.8), (4.0, 0.5), (7.0, 0.3), (9.0, 0.0)],
    [(1.0, 0.7), (3.0, 0.6), (6.0, 0.4), (11.0, 0.25)],
    [(2.0, 0.8), (5.0, 0.6), (8.0, 0.45), (13.0, 0.3)],
    [(1.0, 0.6), (5.0, 0.6), (9.0, 0.55), (15.0, 0.5)],
];

#[inline]
fn mod_wave(w: usize, ph: f32) -> f32 {
    if w == 0 {
        return sin_cycles(ph);
    }
    let mut s = 0.0;
    for &(h, a) in &MOD_WAVES[w.min(7)] {
        if a > 0.0 {
            s += sin_cycles((ph * h).fract()) * a;
        }
    }
    s
}

#[derive(Clone, Copy)]
pub(crate) struct Shared {
    rate: f32,
    p: [f32; N],
    lfo: f32,
    last_pitch: f32,
    /// The assigned controllers' values (FM, vibrato).
    cfm: f32,
    cvib: f32,
}

pub(crate) struct V {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    /// Phases: modulator and carrier of the left and the right engine, the sub-oscillator.
    mp: [f32; 2],
    cp: [f32; 2],
    sp: f32,
    menv: Adsr,
    venv: Adsr,
    fade: Fade,
    killing: bool,
    level: f32,
    /// Unison: this voice's detune in semitones.
    uni: f32,
    glide: bool,
}

impl V {
    fn new() -> V {
        V { on: false, note: 60, vel: 1.0, pitch: 60.0, target: 60.0, mp: [0.0; 2], cp: [0.0; 2], sp: 0.0, menv: Adsr::default(), venv: Adsr::default(), fade: Fade::default(), killing: false, level: 0.0, uni: 0.0, glide: true }
    }
    fn set_envs(&mut self, sh: &Shared) {
        let p = &sh.p;
        self.menv.set(kit::env_time(p[ME_A as usize], 10.0), kit::env_time(p[ME_D as usize], 10.0), p[ME_S as usize], kit::env_time(p[ME_R as usize], 10.0), sh.rate);
        self.venv.set(kit::env_time(p[VE_A as usize], 10.0), kit::env_time(p[VE_D as usize], 10.0), p[VE_S as usize], kit::env_time(p[VE_R as usize], 10.0), sh.rate);
    }
    fn legato(&mut self, note: u8) {
        self.note = note;
        self.target = note as f32;
    }
}

impl Voice<Shared> for V {
    fn start(&mut self, note: u8, velocity: u8, sh: &Shared, _ctl: &Ctl) {
        let was = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.target = note as f32;
        self.vel = (velocity.max(1) as f32 - 1.0) / 126.0;
        // glide from the note before (the voice's own pitch when it is still sounding)
        if sh.p[GLIDE as usize] > 0.0 && self.glide && sh.last_pitch > 0.0 {
            if !was {
                self.pitch = sh.last_pitch;
            }
        } else {
            self.pitch = self.target;
        }
        if !was {
            self.mp = [0.0; 2];
            self.cp = [0.0; 2];
            self.sp = 0.0;
        }
        self.set_envs(sh);
        self.menv.trigger();
        if was {
            self.venv.gate_on();
        } else {
            self.venv.trigger();
        }
    }
    fn release(&mut self, _sh: &Shared, _ctl: &Ctl) {
        self.menv.gate_off();
        self.venv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &Shared, ctl: &Ctl) {
        let rate = sh.rate;
        let p = &sh.p;
        let n = l.len();
        let glide = p[GLIDE as usize];
        if glide > 0.0 {
            self.pitch = self.target + (self.pitch - self.target) * dsp::tau_coef(glide * glide * 1.5 + 0.002, rate / n as f32);
        } else {
            self.pitch = self.target;
        }
        self.set_envs(sh);
        let tr = p[TRANSPOSE_P as usize].round() as i32;
        let transpose = match tr {
            0 => -24.0,
            1 => -12.0,
            25 => 12.0,
            26 => 24.0,
            k => (k - 13) as f32,
        };
        // the LFO knob: right of 0 it moves the FM intensity, left of 0 it is a vibrato
        let lfo_knob = p[LFO as usize];
        let vib = (if lfo_knob < 0.0 { lfo_knob * lfo_knob } else { 0.0 } + sh.cvib * p[CTRL_VIB_AMT as usize]) * sh.lfo * 1.5;
        let fm_lfo = if lfo_knob > 0.0 { lfo_knob * sh.lfo * 0.5 } else { 0.0 };
        let key = self.pitch + transpose + p[TUNE as usize] / 100.0 + ctl.bend + vib + self.uni;
        let base = lg_dsp::midi_hz(key);
        let car_ratio = p[CAR_HARMONIC as usize].round() + p[CAR_FINE as usize];
        let mod_ratio = p[MOD_HARMONIC as usize].round() + p[MOD_FINE as usize];
        // Fixed Carrier: the carrier leaves the keyboard, the bend and the vibrato behind
        let car_base = if p[FIXED as usize] >= 0.5 { lg_dsp::midi_hz(48.0 + transpose + p[TUNE as usize] / 100.0) } else { base };
        let stereo = p[STEREO as usize];
        let two = stereo > 0.0;
        let det = dsp::semis(stereo * stereo * 0.5);
        let wave = (p[MOD_WAVE as usize] * 7.999) as usize;
        let vsens = p[VELOCITY as usize];
        let velf = 1.0 - vsens + vsens * self.vel;
        let fm = (p[FM as usize] + fm_lfo + sh.cfm * p[CTRL_FM_AMT as usize]).clamp(0.0, 1.5);
        let (depth, mpitch) = (p[FM_DEPTH as usize], p[MOD_PITCH as usize]);
        let sub = p[SUB as usize];
        let sub_inc = base * 0.5 / rate;
        let gain = 0.35 * (1.0 - vsens * 0.8 + vsens * 0.8 * self.vel) / if two { 1.0 } else { 1.0 };
        let mut peak = 0.0f32;
        for i in 0..n {
            let me = self.menv.next();
            // modulation envelope: to the FM intensity (inverted left of 0) and the modulator's pitch
            let idx = ((fm + depth * me).clamp(0.0, 2.0)).powi(2) * 12.0 * (0.3 + 0.7 * velf);
            let mp_ratio = if mpitch != 0.0 { dsp::semis(mpitch * me * 24.0) } else { 1.0 };
            let mut out = [0.0f32; 2];
            let engines = if two { 2 } else { 1 };
            for e in 0..engines {
                let d = if !two {
                    1.0
                } else if e == 0 {
                    1.0 / det
                } else {
                    det
                };
                let m = mod_wave(wave, self.mp[e]);
                out[e] = sin_cycles((self.cp[e] + m * idx / std::f32::consts::TAU).rem_euclid(1.0));
                self.mp[e] = (self.mp[e] + (base * mod_ratio * mp_ratio * d / rate).min(0.49)).fract();
                self.cp[e] = (self.cp[e] + (car_base * car_ratio * d / rate).min(0.49)).fract();
            }
            if !two {
                out[1] = out[0];
            }
            let s = if sub > 0.0 {
                self.sp = (self.sp + sub_inc).fract();
                sin_cycles(self.sp) * sub
            } else {
                0.0
            };
            let mut a = self.venv.next() * gain;
            if self.killing {
                a *= self.fade.next();
            }
            let (yl, yr) = ((out[0] + s) * a, (out[1] + s) * a);
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.venv.idle() || (self.killing && self.fade.gain <= 0.0) {
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

pub struct Efm1 {
    params: Params,
    poly: Poly<V>,
    ch: Channel,
    sh: Shared,
    rate: f32,
    lfo: Lfo,
    rng: Rng,
    stack: Stack,
    cc: [f32; 121],
}

impl Efm1 {
    pub fn new(rate: f32) -> Efm1 {
        dsp::warm_tables();
        let mut s = Efm1 {
            params: Params::new(PARAMS),
            poly: Poly::new(16, V::new),
            ch: Channel::new(rate),
            sh: Shared { rate, p: [0.0; N], lfo: 0.0, last_pitch: 0.0, cfm: 0.0, cvib: 0.0 },
            rate,
            lfo: Lfo::default(),
            rng: Rng::new(21),
            stack: Stack::default(),
            cc: [0.0; 121],
        };
        s.update();
        s
    }
    fn update(&mut self) {
        for i in 0..N {
            self.sh.p[i] = self.params.get(i as u32);
        }
        let v = self.sh.p[VOICES_P as usize].round() as usize;
        // Unison layers two voices per note: at most eight notes
        self.poly.set_max(if v < 2 {
            if self.unison() { 2 } else { 1 }
        } else {
            v
        });
        self.ch.bend_up = self.sh.p[BEND as usize];
        self.ch.bend_down = self.sh.p[BEND as usize];
        self.ch.update_bend();
        self.sh.cfm = self.cc[self.sh.p[CTRL_FM as usize].round() as usize];
        self.sh.cvib = self.cc[self.sh.p[CTRL_VIB as usize].round() as usize];
    }
    fn unison(&self) -> bool {
        self.sh.p[UNISON as usize] >= 0.5
    }
    fn mono(&self) -> bool {
        self.sh.p[VOICES_P as usize].round() as i32 <= 1
    }
    fn start(&mut self, pitch: u8, velocity: u8, glide: bool) {
        for v in &mut self.poly.voices {
            v.glide = glide;
        }
        if self.unison() {
            self.poly.stacking = false;
            for v in &mut self.poly.voices {
                if !v.on {
                    v.uni = -0.09;
                }
            }
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
            self.poly.stacking = true;
            for v in &mut self.poly.voices {
                if !v.on {
                    v.uni = 0.09;
                }
            }
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
            self.poly.stacking = false;
        } else {
            for v in &mut self.poly.voices {
                if !v.on {
                    v.uni = 0.0;
                }
            }
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
        }
        self.sh.last_pitch = pitch as f32;
    }
}

impl Plugin for Efm1 {
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
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.stack.clear();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        kit::env_time(self.sh.p[VE_R as usize], 10.0) + 0.1
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        match id {
            ME_A | ME_D | ME_R | VE_A | VE_D | VE_R => {
                let t = kit::env_time(v, 10.0);
                Some(if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.2} s") })
            }
            MOD_HARMONIC | CAR_HARMONIC => Some(format!("{:.0}", v.round())),
            MOD_FINE | CAR_FINE => Some(format!("{v:+.3}")),
            LFO => Some(if v < -0.005 { format!("Vibrato {:.0} %", -v * 100.0) } else if v > 0.005 { format!("FM {:.0} %", v * 100.0) } else { "0".into() }),
            _ => None,
        }
    }
    /// Logic's own parameter block for EFM1 isn't decoded.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for Efm1 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.mono() {
            self.stack.push(pitch, velocity);
            let legato_mode = self.sh.p[VOICES_P as usize].round() as i32 == 1;
            if self.stack.len() > 1 && self.poly.voices.iter().any(|v| v.on && !v.killing) {
                // played legato: the envelopes carry on
                for v in self.poly.voices.iter_mut().filter(|v| v.on && !v.killing) {
                    v.legato(pitch);
                }
                self.sh.last_pitch = pitch as f32;
                return;
            }
            // staccato: a new trigger; Legato glides only between tied notes
            return self.start(pitch, velocity, !legato_mode);
        }
        self.start(pitch, velocity, true);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.mono() {
            self.stack.remove(pitch);
            if let Some((top, _)) = self.stack.last() {
                for v in self.poly.voices.iter_mut().filter(|v| v.on && !v.killing) {
                    v.legato(top);
                }
                self.sh.last_pitch = top as f32;
                return;
            }
            if !self.ch.ctl.sustain {
                let sh = self.sh;
                for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                    v.release(&sh, &self.ch.ctl);
                }
            }
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.stack.clear();
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        if (cc as usize) < CTRL_AT {
            self.cc[cc as usize] = value as f32 / 127.0;
        }
        self.cc[0] = 0.0;
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
        self.update();
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.sh.p[LEVEL as usize]);
        let hz = self.sh.p[RATE as usize];
        self.cc[CTRL_AT] = self.ch.ctl.aftertouch;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.lfo = self.lfo.step(Shape::Triangle, hz * m as f32 / self.rate, &mut self.rng);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly.render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
