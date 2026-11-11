//! ES1 stand-in ("Mono One"): one oscillator (triangle, sawtooth, square to pulse) and a
//! sub-oscillator (squares and pulse one or two octaves down, noise, the side chain) through
//! Drive into a ladder low-pass (24 dB classic / fat, 12 dB, 18 dB) with key follow, one ADSR
//! for the filter and (AGateR / ADSR / GateR) the level, velocity ranges, an LFO (free, synced
//! or constant) and a modulation envelope each routed to one target, Glide, Analog, Chorus.
//! Parameter ids are Studio's own (Logic's float block for ES1 isn't decoded).

use crate::dsp::{self, Adsr, Chorus, Fade, Osc, Rng, Wave, CHUNK};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::synthkit::{self as kit, Lfo, Lp, Shape, SideBuf, Stack};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const OCTAVES: &[&str] = &["32'", "16'", "8'", "4'", "2'"];
pub const SUBS: &[&str] = &["Off", "Square −1 Oct", "Square −2 Oct", "Pulse −2 Oct", "Square −1 + −2 Oct", "Square −1 − −2 Oct", "Noise", "EXT"];
pub const SLOPES: &[&str] = &["24 dB classic", "24 dB fat", "12 dB", "18 dB"];
pub const AMP_MODES: &[&str] = &["AGateR", "ADSR", "GateR"];
pub const LFO_WAVES: &[&str] = &["Triangle", "Saw Down", "Saw Up", "Square", "Sample & Hold", "Lagged Random", "EXT"];
pub const LFO_TARGETS: &[&str] = &["Pitch", "Pulse Width", "Mix", "Cutoff", "Resonance", "Volume"];
pub const ENV_TARGETS: &[&str] = &["Pitch", "Pulse Width", "Mix", "Cutoff", "Resonance", "Volume", "Filter FM", "LFO Amp"];
pub const PB_DOWN: &[&str] = &["Linked", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24"];
pub const VOICES: &[&str] = &["Legato", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16"];
pub const CHORUS: &[&str] = &["Off", "C1", "C2", "Ens"];
pub const MIDI_MODES: &[&str] = &["Off", "Full Remote"];

pub const WAVE: u32 = 0;
pub const OCTAVE: u32 = 1;
pub const SUB: u32 = 2;
pub const MIX: u32 = 3;
pub const CUTOFF: u32 = 4;
pub const RESONANCE: u32 = 5;
pub const SLOPE: u32 = 6;
pub const DRIVE: u32 = 7;
pub const KEY: u32 = 8;
pub const ADSR_VEL_LO: u32 = 9;
pub const ADSR_VEL_HI: u32 = 10;
pub const LEVEL_VEL_LO: u32 = 11;
pub const LEVEL_VEL_HI: u32 = 12;
pub const AMP_MODE: u32 = 13;
pub const ATTACK: u32 = 14;
pub const DECAY: u32 = 15;
pub const SUSTAIN: u32 = 16;
pub const RELEASE: u32 = 17;
pub const GLIDE: u32 = 18;
pub const LFO_WAVE: u32 = 19;
pub const LFO_RATE: u32 = 20;
pub const LFO_INT_LO: u32 = 21;
pub const LFO_INT_HI: u32 = 22;
pub const LFO_TARGET: u32 = 23;
pub const ENV_TARGET: u32 = 24;
pub const FORM: u32 = 25;
pub const ENV_INT_LO: u32 = 26;
pub const ENV_INT_HI: u32 = 27;
pub const TUNE: u32 = 28;
pub const ANALOG: u32 = 29;
pub const BEND: u32 = 30;
pub const BEND_DOWN: u32 = 31;
pub const OUT: u32 = 32;
pub const VOICES_P: u32 = 33;
pub const CHORUS_P: u32 = 34;
pub const BOOST: u32 = 35;
pub const MIDI_MODE: u32 = 36;
const N: usize = 37;

pub static PARAMS: &[Param] = &[
    Param::new(WAVE, "Wave", 0.0, 3.0, 1.0, Unit::None),
    Param::menu(OCTAVE, "Octave", OCTAVES, 2.0),
    Param::menu(SUB, "Sub", SUBS, 1.0),
    Param::new(MIX, "Mix", 0.0, 1.0, 0.0, Unit::None),
    Param::new(CUTOFF, "Cutoff", 0.0, 1.0, 0.6, Unit::None),
    Param::new(RESONANCE, "Resonance", 0.0, 1.0, 0.2, Unit::None),
    Param::menu(SLOPE, "Filter Slope", SLOPES, 1.0),
    Param::new(DRIVE, "Drive", 0.0, 1.0, 0.2, Unit::None),
    Param::new(KEY, "Key", 0.0, 1.0, 0.5, Unit::None),
    Param::new(ADSR_VEL_LO, "ADSR via Vel (min)", 0.0, 1.0, 0.2, Unit::None),
    Param::new(ADSR_VEL_HI, "ADSR via Vel (max)", 0.0, 1.0, 0.5, Unit::None),
    Param::new(LEVEL_VEL_LO, "Level via Vel (min)", 0.0, 1.0, 0.5, Unit::None),
    Param::new(LEVEL_VEL_HI, "Level via Vel (max)", 0.0, 1.0, 1.0, Unit::None),
    Param::menu(AMP_MODE, "Amplifier Envelope", AMP_MODES, 1.0),
    Param::new(ATTACK, "Attack", 0.0, 1.0, 0.0, Unit::None),
    Param::new(DECAY, "Decay", 0.0, 1.0, 0.55, Unit::None),
    Param::new(SUSTAIN, "Sustain", 0.0, 1.0, 0.6, Unit::None),
    Param::new(RELEASE, "Release", 0.0, 1.0, 0.4, Unit::None),
    Param::new(GLIDE, "Glide", 0.0, 1.0, 0.0, Unit::None),
    Param::menu(LFO_WAVE, "LFO Wave", LFO_WAVES, 0.0),
    Param::new(LFO_RATE, "LFO Rate", -1.0, 1.0, 0.75, Unit::None),
    Param::new(LFO_INT_LO, "Int via Whl (min)", 0.0, 1.0, 0.0, Unit::None),
    Param::new(LFO_INT_HI, "Int via Whl (max)", 0.0, 1.0, 0.3, Unit::None),
    Param::menu(LFO_TARGET, "LFO Target", LFO_TARGETS, 0.0),
    Param::menu(ENV_TARGET, "Mod Envelope Target", ENV_TARGETS, 3.0),
    Param::new(FORM, "Form", -1.0, 1.0, 0.0, Unit::None),
    Param::new(ENV_INT_LO, "Int via Vel (min)", 0.0, 1.0, 0.0, Unit::None),
    Param::new(ENV_INT_HI, "Int via Vel (max)", 0.0, 1.0, 0.0, Unit::None),
    Param::new(TUNE, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
    Param::new(ANALOG, "Analog", 0.0, 1.0, 0.1, Unit::None),
    Param::new(BEND, "Bender Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::menu(BEND_DOWN, "PB Range Down", PB_DOWN, 0.0),
    Param::new(OUT, "Out Level", -40.0, 6.0, -6.0, Unit::Db),
    Param::menu(VOICES_P, "Voices", VOICES, 8.0),
    Param::menu(CHORUS_P, "Chorus", CHORUS, 0.0),
    Param::new(BOOST, "Filter Boost", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(MIDI_MODE, "MIDI Mode", MIDI_MODES, 0.0),
];

/// Logic's controller numbers for ES1 (MIDI Mode: Full Remote).
pub const REMOTE: &[(u8, u32)] = &[
    (12, OCTAVE),
    (13, WAVE),
    (14, MIX),
    (15, SUB),
    (16, DRIVE),
    (17, CUTOFF),
    (18, RESONANCE),
    (19, SLOPE),
    (20, ADSR_VEL_LO),
    (21, ADSR_VEL_HI),
    (22, ATTACK),
    (23, DECAY),
    (24, SUSTAIN),
    (25, RELEASE),
    (26, KEY),
    (27, AMP_MODE),
    (28, LEVEL_VEL_LO),
    (29, LEVEL_VEL_HI),
    (30, CHORUS_P),
    (31, ENV_TARGET),
    (102, FORM),
    (103, ENV_INT_LO),
    (104, ENV_INT_HI),
    (105, LFO_RATE),
    (106, LFO_WAVE),
    (107, LFO_TARGET),
    (108, LFO_INT_LO),
    (109, LFO_INT_HI),
    (110, GLIDE),
    (111, TUNE),
    (112, ANALOG),
    (113, BEND),
    (114, OUT),
    (115, VOICES_P),
];

#[derive(Clone, Copy)]
pub(crate) struct Shared {
    rate: f32,
    p: [f32; N],
    /// The LFO's value now (1 when the rate is 0: the wheel sets the amount by hand).
    lfo: f32,
    /// Where a new voice's glide starts.
    last_pitch: f32,
    /// The side chain block (Sub: EXT).
    ext: [f32; CHUNK],
}

pub(crate) struct V {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    osc: Osc,
    sub1: Osc,
    sub2: Osc,
    filt: Lp,
    env: Adsr,
    amp: Adsr,
    /// Seconds since the note began (the modulation envelope).
    age: f32,
    rng: Rng,
    det: f32,
    cut_off: f32,
    fade: Fade,
    killing: bool,
    level: f32,
    glide_always: bool,
    /// A tick into the filter at the note's start (wakes a self-oscillating filter).
    kick: f32,
}

impl V {
    fn new(seed: u32) -> V {
        V { on: false, note: 60, vel: 1.0, pitch: 60.0, target: 60.0, osc: Osc::default(), sub1: Osc::default(), sub2: Osc::default(), filt: Lp::default(), env: Adsr::default(), amp: Adsr::default(), age: 0.0, rng: Rng::new(seed), det: 0.0, cut_off: 0.0, fade: Fade::default(), killing: false, level: 0.0, glide_always: false, kick: 0.0 }
    }
    fn set_envs(&mut self, sh: &Shared) {
        let (a, d, s, r) = (kit::env_time(sh.p[ATTACK as usize], 10.0), kit::env_time(sh.p[DECAY as usize], 10.0), sh.p[SUSTAIN as usize], kit::env_time(sh.p[RELEASE as usize], 10.0));
        self.env.set(a, d, s, r, sh.rate);
        match sh.p[AMP_MODE as usize].round() as i32 {
            0 => self.amp.set(a, 0.01, 1.0, r, sh.rate),
            2 => self.amp.set(0.001, 0.01, 1.0, r, sh.rate),
            _ => self.amp.set(a, d, s, r, sh.rate),
        }
    }
    /// A legato note: the pitch moves, the envelopes carry on.
    fn legato(&mut self, note: u8) {
        self.note = note;
        self.target = note as f32;
    }
}

impl Voice<Shared> for V {
    fn start(&mut self, note: u8, velocity: u8, sh: &Shared, _ctl: &Ctl) {
        let was = self.on && !self.killing;
        let voices = sh.p[VOICES_P as usize].round() as i32;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.target = note as f32;
        self.vel = (velocity.max(1) as f32 - 1.0) / 126.0;
        // Legato: glide only between tied notes; otherwise from the note before
        self.glide_always = voices != 0;
        self.pitch = if sh.p[GLIDE as usize] > 0.0 && voices != 0 && sh.last_pitch > 0.0 { if was { self.pitch } else { sh.last_pitch } } else { self.target };
        let analog = sh.p[ANALOG as usize];
        self.det = self.rng.bipolar() * analog * 0.2;
        self.cut_off = self.rng.bipolar() * analog * 0.04;
        if !was {
            // Analog at 0: every voice starts its cycle at the same point
            let ph = if analog <= 0.0 { 0.0 } else { self.rng.unit() };
            self.osc.phase = ph;
            self.sub1.phase = ph * 0.5;
            self.sub2.phase = ph * 0.25;
            self.filt.reset();
            self.kick = 0.05;
        }
        self.age = 0.0;
        self.set_envs(sh);
        if was {
            self.env.gate_on();
            self.amp.gate_on();
        } else {
            self.env.trigger();
            self.amp.trigger();
        }
    }
    fn release(&mut self, _sh: &Shared, _ctl: &Ctl) {
        self.env.gate_off();
        self.amp.gate_off();
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
        // the modulation envelope: Form 0 = full (static), right = fade in, left = fade out
        let form = p[FORM as usize];
        let me = if form > 0.001 {
            (self.age / (form * form * 8.0 + 0.005)).min(1.0)
        } else if form < -0.001 {
            let t = (1.0 + form) * (1.0 + form) * 8.0 + 0.005;
            (1.0 - self.age / t).max(0.0)
        } else {
            1.0
        };
        self.age += n as f32 / rate;
        let me_amt = me * kit::mix(p[ENV_INT_LO as usize], p[ENV_INT_HI as usize], self.vel);
        let env_target = p[ENV_TARGET as usize].round() as usize;
        let mut lfo_amt = kit::mix(p[LFO_INT_LO as usize], p[LFO_INT_HI as usize], ctl.mod_wheel);
        if env_target == 7 {
            lfo_amt = (lfo_amt + me_amt).clamp(0.0, 1.0);
        }
        // m: pitch, pulse width, mix, cutoff, resonance, volume, filter FM
        let mut m = [0.0f32; 7];
        m[(p[LFO_TARGET as usize].round() as usize).min(5)] += sh.lfo * lfo_amt;
        if env_target < 7 {
            m[env_target] += me_amt;
        }
        let oct = (p[OCTAVE as usize].round() - 2.0) * 12.0;
        let semis = self.pitch + oct + p[TUNE as usize] / 100.0 + self.det + ctl.bend + m[0] * m[0].abs() * 24.0;
        let hz = lg_dsp::midi_hz(semis);
        let inc = (hz / rate).min(0.45);
        let w = p[WAVE as usize];
        let (wave, pw) = if w < 0.5 {
            (Wave::Triangle, 0.5)
        } else if w < 1.5 {
            (Wave::Saw, 0.5)
        } else {
            (Wave::Pulse, (0.5 - (w - 2.0).max(0.0) * 0.45 + m[1] * 0.45).clamp(0.03, 0.97))
        };
        let sub = p[SUB as usize].round() as usize;
        let mixv = (p[MIX as usize] + m[2]).clamp(0.0, 1.0);
        let boost = p[BOOST as usize] >= 0.5;
        let pre = (0.35 + p[DRIVE as usize] * p[DRIVE as usize] * 12.0) * if boost { 0.316 } else { 1.0 };
        let post = if boost { 3.16 } else { 1.0 };
        let slope = p[SLOPE as usize].round() as i32;
        self.filt.poles = match slope {
            2 => 2,
            3 => 3,
            _ => 4,
        };
        self.filt.fat = slope == 1;
        let res = (p[RESONANCE as usize] + m[4]).clamp(0.0, 1.0);
        let fenv_amt = kit::mix(p[ADSR_VEL_LO as usize], p[ADSR_VEL_HI as usize], self.vel);
        let key = p[KEY as usize] * (self.pitch + ctl.bend - 60.0) / 120.0;
        let cut_base = p[CUTOFF as usize] + key + self.cut_off + m[3] * 0.6 + (ctl.brightness - 0.5) * 0.2;
        let fm = m[6];
        let level_vel = kit::mix(p[LEVEL_VEL_LO as usize], p[LEVEL_VEL_HI as usize], self.vel);
        let gain = 0.5 * level_vel * (1.0 + m[5]).clamp(0.0, 2.0) * post;
        let mut peak = 0.0f32;
        for i in 0..n {
            let main = self.osc.next(wave, inc, pw);
            let s1 = self.sub1.next(Wave::Pulse, inc * 0.5, 0.5);
            let s2 = self.sub2.next(Wave::Pulse, inc * 0.25, if sub == 3 { 0.25 } else { 0.5 });
            let subv = match sub {
                1 => s1,
                2 | 3 => s2,
                4 => (s1 + s2) * 0.6,
                5 => (s1 - s2) * 0.6,
                6 => self.rng.bipolar(),
                7 => sh.ext[i] * 2.0,
                _ => 0.0,
            };
            let x = main * (1.0 - mixv) + subv * mixv + self.rng.bipolar() * 1e-5 + self.kick;
            self.kick = 0.0;
            let e = self.env.next();
            let c = cut_base + e * fenv_amt * 0.8;
            if i % 8 == 0 || fm != 0.0 {
                // Filter FM: the oscillator's triangle moves the cutoff at audio rate
                let f = if fm != 0.0 { fm * 0.25 * (4.0 * (self.osc.phase - 0.5).abs() - 1.0) } else { 0.0 };
                let hzc = kit::cutoff_hz(c + f).min(rate * 0.45);
                self.filt.set_wc(std::f32::consts::PI * hzc / rate, res);
            }
            let y = self.filt.run(x * pre);
            let mut a = self.amp.next() * gain;
            if self.killing {
                a *= self.fade.next();
            }
            let o = y * a;
            peak = peak.max(o.abs());
            l[i] += o;
            r[i] += o;
        }
        self.level = peak;
        if self.amp.idle() || (self.killing && self.fade.gain <= 0.0) {
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

pub struct Es1 {
    params: Params,
    poly: Poly<V>,
    ch: Channel,
    sh: Shared,
    rate: f32,
    lfo: Lfo,
    rng: Rng,
    chorus: [Chorus; 2],
    stack: Stack,
    side: SideBuf,
    bpm: f32,
}

impl Es1 {
    pub fn new(rate: f32) -> Es1 {
        dsp::warm_tables();
        let mut seed = 77;
        let mut s = Es1 {
            params: Params::new(PARAMS),
            poly: Poly::new(16, || {
                seed += 13;
                V::new(seed)
            }),
            ch: Channel::new(rate),
            sh: Shared { rate, p: [0.0; N], lfo: 0.0, last_pitch: 0.0, ext: [0.0; CHUNK] },
            rate,
            lfo: Lfo::default(),
            rng: Rng::new(9),
            chorus: [Chorus::new(rate), Chorus::new(rate)],
            stack: Stack::default(),
            side: SideBuf::new(),
            bpm: 120.0,
        };
        s.update();
        s
    }
    fn update(&mut self) {
        for i in 0..N {
            self.sh.p[i] = self.params.get(i as u32);
        }
        let v = self.sh.p[VOICES_P as usize].round() as usize;
        self.poly.set_max(v.max(1));
        self.ch.bend_up = self.sh.p[BEND as usize];
        let d = self.sh.p[BEND_DOWN as usize].round();
        self.ch.bend_down = if d < 0.5 { self.ch.bend_up } else { d - 1.0 };
        self.ch.update_bend();
    }
    fn mono(&self) -> bool {
        self.sh.p[VOICES_P as usize].round() as i32 <= 1
    }
    /// The LFO's rate in Hz and, when synced, the note length's name.
    pub fn lfo_rate(&self) -> (f32, Option<&'static str>) {
        kit::bipolar_rate(self.sh.p[LFO_RATE as usize], 24.0, self.bpm)
    }
}

impl Plugin for Es1 {
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
        for c in &mut self.chorus {
            c.reset();
        }
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        kit::env_time(self.sh.p[RELEASE as usize], 10.0) + 0.1
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        match id {
            LFO_RATE => Some(match kit::bipolar_rate(v, 24.0, self.bpm) {
                (_, Some(n)) => n.to_string(),
                (hz, None) if hz > 0.0 => format!("{hz:.2} Hz"),
                _ => "DC".to_string(),
            }),
            ATTACK | DECAY | RELEASE => {
                let t = kit::env_time(v, 10.0);
                Some(if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.2} s") })
            }
            FORM => Some(if v > 0.001 { format!("Attack {:.0} ms", (v * v * 8.0 + 0.005) * 1000.0) } else if v < -0.001 { format!("Decay {:.0} ms", ((1.0 + v) * (1.0 + v) * 8.0 + 0.005) * 1000.0) } else { "Full".to_string() }),
            WAVE => Some(if v < 0.5 { "Triangle".into() } else if v < 1.5 { "Sawtooth".into() } else if v < 2.02 { "Square".into() } else { format!("Pulse {:.0} %", (0.5 - (v - 2.0) * 0.45) * 100.0) }),
            ANALOG | MIX | CUTOFF | RESONANCE | DRIVE | KEY | SUSTAIN | GLIDE => Some(format!("{:.0} %", v * 100.0)),
            _ => None,
        }
    }
    /// Logic's own parameter block for ES1 isn't decoded: a project's ES1 keeps the defaults.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for Es1 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.mono() {
            self.stack.push(pitch, velocity);
            let legato = self.sh.p[VOICES_P as usize].round() as i32 == 0;
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                if legato && self.stack.len() > 1 {
                    // single trigger, fingered portamento
                    v.legato(pitch);
                } else {
                    let sh = self.sh;
                    v.start(pitch, velocity, &sh, &self.ch.ctl);
                }
                self.sh.last_pitch = pitch as f32;
                return;
            }
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
        self.sh.last_pitch = pitch as f32;
    }
    fn note_off(&mut self, pitch: u8) {
        if self.mono() {
            self.stack.remove(pitch);
            if let Some((top, _)) = self.stack.last() {
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                    v.legato(top);
                    self.sh.last_pitch = top as f32;
                }
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
        if self.sh.p[MIDI_MODE as usize] >= 0.5
            && let Some(&(_, id)) = REMOTE.iter().find(|x| x.0 == cc)
        {
            let v = kit::cc_value(&PARAMS[id as usize], value);
            return self.set(id, v);
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm as f32;
    }
    fn side_chain(&mut self, l: &[f32], r: &[f32]) {
        self.side.fill(l, r);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.sh.p[OUT as usize]);
        let (hz, _) = self.lfo_rate();
        let shape = match self.sh.p[LFO_WAVE as usize].round() as i32 {
            0 => Shape::Triangle,
            1 => Shape::SawDown,
            2 => Shape::SawUp,
            3 => Shape::Square,
            4 => Shape::SampleHold,
            _ => Shape::Lag,
        };
        let ext_lfo = self.sh.p[LFO_WAVE as usize].round() as i32 == 6;
        let chorus = self.sh.p[CHORUS_P as usize].round() as i32;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.lfo = if ext_lfo {
                (self.side.level * 4.0).min(1.0)
            } else if hz <= 0.0 {
                1.0
            } else {
                self.lfo.step(shape, hz * m as f32 / self.rate, &mut self.rng)
            };
            for i in 0..m {
                self.sh.ext[i] = self.side.at(pos + i);
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly.render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            match chorus {
                1 => self.chorus[0].process(&mut sl[..m], &mut sr[..m], 0.5, 0.5, 0.5),
                2 => self.chorus[0].process(&mut sl[..m], &mut sr[..m], 0.85, 1.0, 0.7),
                3 => {
                    self.chorus[0].process(&mut sl[..m], &mut sr[..m], 0.6, 0.8, 0.6);
                    self.chorus[1].process(&mut sl[..m], &mut sr[..m], 5.7, 0.25, 0.45);
                }
                _ => {}
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
        self.side.done();
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
