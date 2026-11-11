//! ES E, ES M and ES P stand-ins: three small subtractive synths on one voice.
//!
//! - ES E ("Ensemble Synth"): eight voices, sawtooth or a pulse whose width the Wave knob sets,
//!   an LFO for vibrato (sawtooth) or pulse width modulation (pulse), a low-pass with an
//!   attack / release envelope, Chorus I / II / Ensemble.
//! - ES M ("Bass Mono"): one voice with fingered portamento, sawtooth crossfading to a
//!   rectangle an octave below, a 24 dB low-pass that keeps its bass at high resonance, decay
//!   envelopes for filter and level, overdrive.
//! - ES P ("Poly Six"): eight voices, six level sliders (triangle, sawtooth, rectangle, two
//!   sub-oscillators, noise), an LFO for vibrato or wah, a low-pass with key follow, one ADSR for
//!   filter and level (or a plain gate), chorus and overdrive.
//!
//! Parameter ids are Studio's own (Logic's float blocks for these aren't decoded).

use crate::dsp::{self, Adsr, Chorus, Fade, Osc, Rng, Wave, CHUNK};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::synthkit::{self as kit, Lfo, Lp, Shape, Stack};
use lg_dsp::{Instrument, Param, Plugin, Unit};

/// What the voices play, worked out from the front panel once per block.
#[derive(Clone, Copy, Default)]
pub(crate) struct Tone {
    rate: f32,
    tri: f32,
    saw: f32,
    pulse: f32,
    pw: f32,
    /// The pulse sounds an octave below the sawtooth (ES M).
    pulse_down: bool,
    sub1: f32,
    sub2: f32,
    noise: f32,
    /// Semitones added to every note (octave buttons, tune, vibrato).
    transpose: f32,
    cutoff: f32,
    res: f32,
    fat: bool,
    key: f32,
    env_int: f32,
    env_vel: f32,
    /// Filter envelope and level envelope: attack, decay, sustain, release.
    fenv: [f32; 4],
    aenv: [f32; 4],
    vel_vol: f32,
    glide: f32,
    last_pitch: f32,
}

pub(crate) struct V {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    osc: Osc,
    tri: Osc,
    pul: Osc,
    s1: Osc,
    s2: Osc,
    filt: Lp,
    fenv: Adsr,
    aenv: Adsr,
    rng: Rng,
    fade: Fade,
    killing: bool,
    level: f32,
}

impl V {
    fn new(seed: u32) -> V {
        V { on: false, note: 60, vel: 1.0, pitch: 60.0, target: 60.0, osc: Osc::default(), tri: Osc::default(), pul: Osc::default(), s1: Osc::default(), s2: Osc::default(), filt: Lp::default(), fenv: Adsr::default(), aenv: Adsr::default(), rng: Rng::new(seed), fade: Fade::default(), killing: false, level: 0.0 }
    }
    fn legato(&mut self, note: u8) {
        self.note = note;
        self.target = note as f32;
    }
}

impl Voice<Tone> for V {
    fn start(&mut self, note: u8, velocity: u8, sh: &Tone, _ctl: &Ctl) {
        let was = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.target = note as f32;
        self.pitch = self.target;
        self.vel = (velocity.max(1) as f32 - 1.0) / 126.0;
        if !was {
            self.filt.reset();
            let ph = self.rng.unit();
            self.osc.phase = ph;
            self.tri.phase = ph;
            self.pul.phase = ph;
        }
        self.fenv.set(sh.fenv[0], sh.fenv[1], sh.fenv[2], sh.fenv[3], sh.rate);
        self.aenv.set(sh.aenv[0], sh.aenv[1], sh.aenv[2], sh.aenv[3], sh.rate);
        self.fenv.trigger();
        if was {
            self.aenv.gate_on();
        } else {
            self.aenv.trigger();
        }
    }
    fn release(&mut self, _sh: &Tone, _ctl: &Ctl) {
        self.fenv.gate_off();
        self.aenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &Tone, ctl: &Ctl) {
        let rate = sh.rate;
        let n = l.len();
        if sh.glide > 0.0 {
            self.pitch = self.target + (self.pitch - self.target) * dsp::tau_coef(sh.glide * sh.glide * 1.5 + 0.002, rate / n as f32);
        } else {
            self.pitch = self.target;
        }
        self.fenv.set(sh.fenv[0], sh.fenv[1], sh.fenv[2], sh.fenv[3], rate);
        self.aenv.set(sh.aenv[0], sh.aenv[1], sh.aenv[2], sh.aenv[3], rate);
        let hz = lg_dsp::midi_hz(self.pitch + sh.transpose + ctl.bend);
        let inc = (hz / rate).min(0.45);
        self.filt.poles = 4;
        self.filt.fat = sh.fat;
        // velocity scales the envelope's hold on the cutoff: least at velocity 1, most at 127
        let env_amt = sh.env_int * (1.0 - sh.env_vel + sh.env_vel * self.vel);
        let cut_base = sh.cutoff + sh.key * (self.pitch + ctl.bend - 60.0) / 120.0 + (ctl.brightness - 0.5) * 0.2;
        let gain = 0.5 * (1.0 - sh.vel_vol + sh.vel_vol * self.vel);
        let pinc = if sh.pulse_down { inc * 0.5 } else { inc };
        let mut peak = 0.0f32;
        for i in 0..n {
            let mut x = 0.0;
            if sh.saw > 0.0 {
                x += self.osc.next(Wave::Saw, inc, 0.5) * sh.saw;
            }
            if sh.tri > 0.0 {
                x += self.tri.next(Wave::Triangle, inc, 0.5) * sh.tri;
            }
            if sh.pulse > 0.0 {
                x += self.pul.next(Wave::Pulse, pinc, sh.pw) * sh.pulse;
            }
            if sh.sub1 > 0.0 {
                x += self.s1.next(Wave::Pulse, inc * 0.5, 0.5) * sh.sub1;
            }
            if sh.sub2 > 0.0 {
                x += self.s2.next(Wave::Pulse, inc * 0.25, 0.5) * sh.sub2;
            }
            if sh.noise > 0.0 {
                x += self.rng.bipolar() * sh.noise;
            }
            let e = self.fenv.next();
            if i % 8 == 0 {
                self.filt.set(kit::cutoff_hz(cut_base + e * env_amt).min(rate * 0.45), sh.res, rate);
            }
            let y = self.filt.run(x * 0.5);
            let mut a = self.aenv.next() * gain;
            if self.killing {
                a *= self.fade.next();
            }
            let o = y * a;
            peak = peak.max(o.abs());
            l[i] += o;
            r[i] += o;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    E,
    M,
    P,
}

/// One of the three synths.
pub struct EsSmall {
    pub model: Model,
    params: Params,
    poly: Poly<V>,
    ch: Channel,
    tone: Tone,
    rate: f32,
    lfo: Lfo,
    rng: Rng,
    chorus: [Chorus; 2],
    stack: Stack,
    od_lp: [dsp::OnePole; 2],
}

// ---------------------------------------------------------------------------------------------
// ES E

pub mod e {
    pub const WAVE: u32 = 0;
    pub const OCTAVE: u32 = 1;
    pub const VIB_PWM: u32 = 2;
    pub const SPEED: u32 = 3;
    pub const CUTOFF: u32 = 4;
    pub const RESONANCE: u32 = 5;
    pub const AR_INT: u32 = 6;
    pub const VELO_FILTER: u32 = 7;
    pub const ATTACK: u32 = 8;
    pub const RELEASE: u32 = 9;
    pub const VOLUME: u32 = 10;
    pub const VELO_VOLUME: u32 = 11;
    pub const CHORUS: u32 = 12;
    pub const BEND_UP: u32 = 13;
    pub const BEND_DOWN: u32 = 14;
    pub const TUNE: u32 = 15;
}
pub const OCT_E: &[&str] = &["16'", "8'", "4'"];
pub const CHORUS_E: &[&str] = &["Off", "Chorus I", "Chorus II", "Ensemble"];
pub static PARAMS_E: &[Param] = &[
    Param::new(e::WAVE, "Wave", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(e::OCTAVE, "Octave", OCT_E, 1.0),
    Param::new(e::VIB_PWM, "Vib/PWM", 0.0, 1.0, 0.2, Unit::None),
    Param::new(e::SPEED, "Speed", 0.1, 20.0, 5.0, Unit::Hz),
    Param::new(e::CUTOFF, "Cutoff", 0.0, 1.0, 0.7, Unit::None),
    Param::new(e::RESONANCE, "Resonance", 0.0, 1.0, 0.1, Unit::None),
    Param::new(e::AR_INT, "AR Int", 0.0, 1.0, 0.2, Unit::None),
    Param::new(e::VELO_FILTER, "Velo Filter", 0.0, 1.0, 0.3, Unit::None),
    Param::new(e::ATTACK, "Attack", 0.0, 1.0, 0.3, Unit::None),
    Param::new(e::RELEASE, "Release", 0.0, 1.0, 0.5, Unit::None),
    Param::new(e::VOLUME, "Volume", -40.0, 6.0, -6.0, Unit::Db),
    Param::new(e::VELO_VOLUME, "Velo Volume", 0.0, 1.0, 0.4, Unit::None),
    Param::menu(e::CHORUS, "Chorus", CHORUS_E, 3.0),
    Param::new(e::BEND_UP, "Bend Up", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(e::BEND_DOWN, "Bend Down", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(e::TUNE, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
];

// ---------------------------------------------------------------------------------------------
// ES M

pub mod m {
    pub const MIX: u32 = 0;
    pub const OCTAVE: u32 = 1;
    pub const GLIDE: u32 = 2;
    pub const CUTOFF: u32 = 3;
    pub const RESONANCE: u32 = 4;
    pub const INT: u32 = 5;
    pub const FILTER_DECAY: u32 = 6;
    pub const FILTER_VELO: u32 = 7;
    pub const DECAY: u32 = 8;
    pub const VELO: u32 = 9;
    pub const VOL: u32 = 10;
    pub const OVERDRIVE: u32 = 11;
    pub const BEND_UP: u32 = 12;
    pub const BEND_DOWN: u32 = 13;
    pub const TUNE: u32 = 14;
}
pub const OCT_M: &[&str] = &["32'", "16'", "8'"];
pub static PARAMS_M: &[Param] = &[
    Param::new(m::MIX, "Mix", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(m::OCTAVE, "Octave", OCT_M, 1.0),
    Param::new(m::GLIDE, "Glide", 0.0, 1.0, 0.0, Unit::None),
    Param::new(m::CUTOFF, "Cutoff", 0.0, 1.0, 0.35, Unit::None),
    Param::new(m::RESONANCE, "Resonance", 0.0, 1.0, 0.3, Unit::None),
    Param::new(m::INT, "Filter Int", 0.0, 1.0, 0.5, Unit::None),
    Param::new(m::FILTER_DECAY, "Filter Decay", 0.0, 1.0, 0.5, Unit::None),
    Param::new(m::FILTER_VELO, "Filter Velo", 0.0, 1.0, 0.4, Unit::None),
    Param::new(m::DECAY, "Decay", 0.0, 1.0, 0.7, Unit::None),
    Param::new(m::VELO, "Velo", 0.0, 1.0, 0.4, Unit::None),
    Param::new(m::VOL, "Vol", -40.0, 6.0, -6.0, Unit::Db),
    Param::new(m::OVERDRIVE, "Overdrive", 0.0, 1.0, 0.0, Unit::None),
    Param::new(m::BEND_UP, "Pitch Bend Up", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(m::BEND_DOWN, "Pitch Bend Down", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(m::TUNE, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
];

// ---------------------------------------------------------------------------------------------
// ES P

pub mod p {
    pub const TRIANGLE: u32 = 0;
    pub const SAWTOOTH: u32 = 1;
    pub const RECTANGLE: u32 = 2;
    pub const SUB1: u32 = 3;
    pub const SUB2: u32 = 4;
    pub const NOISE: u32 = 5;
    pub const OCTAVE: u32 = 6;
    pub const VIB_WAH: u32 = 7;
    pub const SPEED: u32 = 8;
    pub const FREQUENCY: u32 = 9;
    pub const RESONANCE: u32 = 10;
    pub const KEY: u32 = 11;
    pub const ADSR_INT: u32 = 12;
    pub const VELO_FILTER: u32 = 13;
    pub const ATTACK: u32 = 14;
    pub const DECAY: u32 = 15;
    pub const SUSTAIN: u32 = 16;
    pub const RELEASE: u32 = 17;
    pub const VOLUME: u32 = 18;
    pub const VELO_VOLUME: u32 = 19;
    pub const VCA_MODE: u32 = 20;
    pub const CHORUS: u32 = 21;
    pub const OVERDRIVE: u32 = 22;
    pub const BEND_UP: u32 = 23;
    pub const BEND_DOWN: u32 = 24;
    pub const TUNE: u32 = 25;
}
pub const OCT_P: &[&str] = &["16'", "8'", "4'"];
pub const KEY_P: &[&str] = &["Off", "1/3", "2/3", "3/3"];
pub const VCA_P: &[&str] = &["ADSR", "Gate"];
pub static PARAMS_P: &[Param] = &[
    Param::new(p::TRIANGLE, "Triangle", 0.0, 1.0, 0.0, Unit::None),
    Param::new(p::SAWTOOTH, "Sawtooth", 0.0, 1.0, 1.0, Unit::None),
    Param::new(p::RECTANGLE, "Rectangle", 0.0, 1.0, 0.0, Unit::None),
    Param::new(p::SUB1, "Sub Osc −1 Oct", 0.0, 1.0, 0.4, Unit::None),
    Param::new(p::SUB2, "Sub Osc −2 Oct", 0.0, 1.0, 0.0, Unit::None),
    Param::new(p::NOISE, "Noise", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(p::OCTAVE, "Octave", OCT_P, 1.0),
    Param::new(p::VIB_WAH, "Vib/Wah", -1.0, 1.0, 0.0, Unit::None),
    Param::new(p::SPEED, "Speed", 0.1, 20.0, 5.0, Unit::Hz),
    Param::new(p::FREQUENCY, "Frequency", 0.0, 1.0, 0.55, Unit::None),
    Param::new(p::RESONANCE, "Resonance", 0.0, 1.0, 0.2, Unit::None),
    Param::menu(p::KEY, "Key Follow", KEY_P, 2.0),
    Param::new(p::ADSR_INT, "ADSR Int", 0.0, 1.0, 0.4, Unit::None),
    Param::new(p::VELO_FILTER, "Velo Filter", 0.0, 1.0, 0.3, Unit::None),
    Param::new(p::ATTACK, "Attack", 0.0, 1.0, 0.0, Unit::None),
    Param::new(p::DECAY, "Decay", 0.0, 1.0, 0.6, Unit::None),
    Param::new(p::SUSTAIN, "Sustain", 0.0, 1.0, 0.6, Unit::None),
    Param::new(p::RELEASE, "Release", 0.0, 1.0, 0.45, Unit::None),
    Param::new(p::VOLUME, "Volume", -40.0, 6.0, -6.0, Unit::Db),
    Param::new(p::VELO_VOLUME, "Velo Volume", 0.0, 1.0, 0.4, Unit::None),
    Param::menu(p::VCA_MODE, "VCA Mode", VCA_P, 0.0),
    Param::new(p::CHORUS, "Chorus", 0.0, 1.0, 0.3, Unit::None),
    Param::new(p::OVERDRIVE, "Overdrive", 0.0, 1.0, 0.0, Unit::None),
    Param::new(p::BEND_UP, "Bend Up", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(p::BEND_DOWN, "Bend Down", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(p::TUNE, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
];

impl EsSmall {
    pub fn new(model: Model, rate: f32) -> EsSmall {
        dsp::warm_tables();
        let mut seed = 301;
        let voices = if model == Model::M { 1 } else { 8 };
        let mut s = EsSmall {
            model,
            params: Params::new(match model {
                Model::E => PARAMS_E,
                Model::M => PARAMS_M,
                Model::P => PARAMS_P,
            }),
            poly: Poly::new(voices, || {
                seed += 17;
                V::new(seed)
            }),
            ch: Channel::new(rate),
            tone: Tone { rate, ..Tone::default() },
            rate,
            lfo: Lfo::default(),
            rng: Rng::new(3),
            chorus: [Chorus::new(rate), Chorus::new(rate)],
            stack: Stack::default(),
            od_lp: [dsp::OnePole::default(); 2],
        };
        s.poly.set_max(voices);
        for f in &mut s.od_lp {
            f.lowpass_hz(6000.0, rate);
        }
        s.update(0.0);
        s
    }
    /// The front panel as a tone; `lfo` is the LFO's value now.
    fn update(&mut self, lfo: f32) {
        let g = |id: u32| self.params.get(id);
        let mut t = Tone { rate: self.rate, last_pitch: self.tone.last_pitch, ..Tone::default() };
        let (up, down, tune);
        match self.model {
            Model::E => {
                let w = g(e::WAVE);
                let depth = g(e::VIB_PWM);
                t.transpose = (g(e::OCTAVE).round() - 1.0) * 12.0;
                if w <= 0.005 {
                    t.saw = 1.0;
                    // sawtooth: the LFO is a vibrato
                    t.transpose += lfo * depth * depth * 2.0;
                } else {
                    t.pulse = 1.0;
                    // pulse: the LFO moves the width (widest range from the 50 % rectangle)
                    t.pw = (1.0 - w + lfo * depth * 0.48).clamp(0.02, 0.98);
                }
                t.cutoff = g(e::CUTOFF);
                t.res = g(e::RESONANCE) * 0.95;
                t.env_int = g(e::AR_INT) * 0.8;
                t.env_vel = g(e::VELO_FILTER);
                let (a, r) = (kit::env_time(g(e::ATTACK), 6.0), kit::env_time(g(e::RELEASE), 8.0));
                t.fenv = [a, 0.01, 1.0, r];
                t.aenv = [a, 0.01, 1.0, r];
                t.vel_vol = g(e::VELO_VOLUME);
                (up, down, tune) = (g(e::BEND_UP), g(e::BEND_DOWN), g(e::TUNE));
            }
            Model::M => {
                let mix = g(m::MIX);
                t.saw = 1.0 - mix;
                t.pulse = mix;
                t.pw = 0.5;
                t.pulse_down = true;
                t.transpose = (g(m::OCTAVE).round() - 2.0) * 12.0;
                t.glide = g(m::GLIDE);
                t.cutoff = g(m::CUTOFF);
                t.res = g(m::RESONANCE);
                t.fat = true;
                t.key = 0.5;
                t.env_int = g(m::INT) * 0.8;
                t.env_vel = g(m::FILTER_VELO);
                t.fenv = [0.0005, kit::env_time(g(m::FILTER_DECAY), 12.0), 0.0, 0.01];
                // the level: no attack, no sustain, no release, only the decay
                t.aenv = [0.0005, kit::env_time(g(m::DECAY), 30.0), 0.0, 0.012];
                t.vel_vol = g(m::VELO);
                (up, down, tune) = (g(m::BEND_UP), g(m::BEND_DOWN), g(m::TUNE));
            }
            Model::P => {
                t.tri = g(p::TRIANGLE);
                t.saw = g(p::SAWTOOTH);
                t.pulse = g(p::RECTANGLE);
                t.pw = 0.5;
                t.sub1 = g(p::SUB1);
                t.sub2 = g(p::SUB2);
                t.noise = g(p::NOISE);
                t.transpose = (g(p::OCTAVE).round() - 1.0) * 12.0;
                let vw = g(p::VIB_WAH);
                t.cutoff = g(p::FREQUENCY);
                if vw < 0.0 {
                    t.transpose += lfo * vw * vw * 2.0;
                } else {
                    t.cutoff += lfo * vw * 0.3;
                }
                t.res = g(p::RESONANCE);
                t.fat = true;
                t.key = g(p::KEY).round() / 3.0;
                t.env_int = g(p::ADSR_INT) * 0.8;
                t.env_vel = g(p::VELO_FILTER);
                let adsr = [kit::env_time(g(p::ATTACK), 8.0), kit::env_time(g(p::DECAY), 10.0), g(p::SUSTAIN), kit::env_time(g(p::RELEASE), 10.0)];
                t.fenv = adsr;
                // Gate: a constant tone while the key is down, like an organ
                t.aenv = if g(p::VCA_MODE) >= 0.5 { [0.002, 0.01, 1.0, 0.01] } else { adsr };
                t.vel_vol = g(p::VELO_VOLUME);
                (up, down, tune) = (g(p::BEND_UP), g(p::BEND_DOWN), g(p::TUNE));
            }
        }
        t.transpose += tune / 100.0;
        self.tone = t;
        if self.ch.bend_up != up || self.ch.bend_down != down {
            self.ch.bend_up = up;
            self.ch.bend_down = down;
            self.ch.update_bend();
        }
    }
    fn lfo_hz(&self) -> f32 {
        match self.model {
            Model::E => self.params.get(e::SPEED),
            Model::P => self.params.get(p::SPEED),
            Model::M => 0.0,
        }
    }
    fn volume(&self) -> f32 {
        lg_dsp::db_to_gain(self.params.get(match self.model {
            Model::E => e::VOLUME,
            Model::M => m::VOL,
            Model::P => p::VOLUME,
        }))
    }
}

impl Plugin for EsSmall {
    fn params(&self) -> &[Param] {
        self.params.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.update(0.0);
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.stack.clear();
        for c in &mut self.chorus {
            c.reset();
        }
        self.ch.reset();
        self.update(0.0);
    }
    fn tail(&self) -> f32 {
        self.tone.aenv[3] + 0.1
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        let time = |max: f32| {
            let t = kit::env_time(v, max);
            Some(if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.2} s") })
        };
        match (self.model, id) {
            (Model::E, e::WAVE) => Some(if v <= 0.005 { "Sawtooth".into() } else { format!("Pulse {:.0} %", (1.0 - v).clamp(0.02, 0.98) * 100.0) }),
            (Model::E, e::ATTACK) => time(6.0),
            (Model::E, e::RELEASE) => time(8.0),
            (Model::M, m::FILTER_DECAY) => time(12.0),
            (Model::M, m::DECAY) => time(30.0),
            (Model::P, p::ATTACK) => time(8.0),
            (Model::P, p::DECAY) | (Model::P, p::RELEASE) => time(10.0),
            (Model::P, p::VIB_WAH) => Some(if v < -0.005 { format!("Vibrato {:.0} %", -v * 100.0) } else if v > 0.005 { format!("Wah {:.0} %", v * 100.0) } else { "Off".into() }),
            _ => None,
        }
    }
    /// Logic's own parameter blocks for these synths aren't decoded.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for EsSmall {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.model == Model::M {
            self.stack.push(pitch, velocity);
            if self.stack.len() > 1
                && let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing)
            {
                // fingered portamento: a tied note glides and keeps its envelopes
                v.legato(pitch);
                return;
            }
        }
        self.poly.note_on(pitch, velocity, &self.tone, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.model == Model::M {
            self.stack.remove(pitch);
            if let Some((top, _)) = self.stack.last() {
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                    v.legato(top);
                }
                return;
            }
            if !self.ch.ctl.sustain {
                let t = self.tone;
                for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                    v.release(&t, &self.ch.ctl);
                }
            }
        }
        self.poly.note_off(pitch, &self.tone, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.stack.clear();
            return self.poly.all_notes_off(&self.tone, &self.ch.ctl);
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.tone, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = self.volume();
        let hz = self.lfo_hz();
        let (chorus_mode, chorus_amt, drive) = match self.model {
            Model::E => (self.params.get(e::CHORUS).round() as i32, 1.0, 0.0),
            Model::M => (0, 0.0, self.params.get(m::OVERDRIVE)),
            Model::P => (if self.params.get(p::CHORUS) > 0.0 { 1 } else { 0 }, self.params.get(p::CHORUS), self.params.get(p::OVERDRIVE)),
        };
        let mut pos = 0;
        while pos < n {
            let k = (n - pos).min(CHUNK);
            // the mod wheel deepens the LFO a little, as on the originals' vibrato
            let lfo = if hz > 0.0 { self.lfo.step(Shape::Triangle, hz * k as f32 / self.rate, &mut self.rng) } else { 0.0 };
            self.update(lfo);
            sl[..k].fill(0.0);
            sr[..k].fill(0.0);
            self.poly.render(&mut sl[..k], &mut sr[..k], &self.tone, &self.ch.ctl);
            if drive > 0.0 {
                let g = 1.0 + drive * drive * 30.0;
                let make = 1.0 / (1.0 + drive * 2.5);
                for i in 0..k {
                    sl[i] = self.od_lp[0].lp(dsp::fast_tanh(sl[i] * g)) * make;
                    sr[i] = self.od_lp[1].lp(dsp::fast_tanh(sr[i] * g)) * make;
                }
            }
            match (self.model, chorus_mode) {
                (Model::E, 1) => self.chorus[0].process(&mut sl[..k], &mut sr[..k], 0.5, 0.5, 0.5),
                (Model::E, 2) => self.chorus[0].process(&mut sl[..k], &mut sr[..k], 0.85, 1.0, 0.7),
                (Model::E, 3) => {
                    self.chorus[0].process(&mut sl[..k], &mut sr[..k], 0.6, 0.8, 0.6);
                    self.chorus[1].process(&mut sl[..k], &mut sr[..k], 5.7, 0.25, 0.45);
                }
                (Model::P, 1) => self.chorus[0].process(&mut sl[..k], &mut sr[..k], 0.7, chorus_amt, chorus_amt * 0.8),
                _ => {}
            }
            for i in 0..k {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += k;
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
