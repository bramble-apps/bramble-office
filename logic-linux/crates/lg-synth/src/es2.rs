//! ES2 stand-in: three oscillators (classic waves, digiwaves, FM, ring, sync, noise) mixed on
//! the triangle mixer, sine level, drive, two filters (multimode 12 dB + 12/18/24 dB low-pass)
//! in series or parallel with blend, three envelopes, two LFOs, the ten-slot modulation router,
//! unison/analog detune, glide, distortion and chorus/flanger/phaser.
//! Parameter ids = Logic's float index as in ES2-PARAM-MAP (paired by order; to be confirmed).

use crate::dsp::{
    self, Adsr, CHUNK, Chorus, Fade, Flanger, Ladder, Osc, Phaser, Rng, Svf, SvfMode, Wave,
};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::retro::{Lfo, table_read};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const OSC_START: &[&str] = &["Free", "Soft", "Hard"];
pub const KB_MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const OSC1_WAVES: &[&str] = &[
    "Sine", "Triangle", "Saw", "Square", "Pulse", "Digiwave", "FM", "Noise",
];
pub const OSC2_WAVES: &[&str] = &[
    "Sine", "Triangle", "Saw", "Square", "Pulse", "Digiwave", "Ring", "Sync", "Noise",
];
pub const OSC3_WAVES: &[&str] = &[
    "Sine", "Triangle", "Saw", "Square", "Pulse", "Digiwave", "Sync", "Noise",
];
pub const ROUTING: &[&str] = &["Series", "Parallel"];
pub const F1_MODES: &[&str] = &["Lowpass", "Highpass", "Peak", "Band Reject", "Bandpass"];
pub const F2_MODES: &[&str] = &["12 dB", "18 dB", "24 dB"];
pub const LFO_WAVES: &[&str] = &[
    "Sine",
    "Triangle",
    "Saw Up",
    "Saw Down",
    "Square",
    "Sample & Hold",
    "Random",
];
pub const DIST_TYPES: &[&str] = &["Soft", "Hard"];
pub const FXMOD_TYPES: &[&str] = &["Chorus", "Flanger", "Phaser", "Off"];
pub const MM_TARGETS: &[&str] = &[
    "Off",
    "Pitch123",
    "Pitch1",
    "Pitch2",
    "Pitch3",
    "Detune",
    "OscWaves",
    "Osc1Wave",
    "Osc2Wave",
    "Osc3Wave",
    "Osc1 Level",
    "OscLevelX",
    "OscLevelY",
    "Cutoff 1",
    "Cut 1+2",
    "Resonance 1",
    "Cutoff 2",
    "Resonance 2",
    "Cut1 inv2",
    "LPF FM",
    "Filter Blend",
    "Amp",
    "Pan",
    "Sine Level",
    "LFO1 Rate",
    "Env Attack",
];
pub const MM_SOURCES: &[&str] = &[
    "Off",
    "LFO1",
    "LFO2",
    "Env1",
    "Env2",
    "Env3",
    "Pad-X",
    "Pad-Y",
    "Max",
    "Kybd",
    "Velocity",
    "Bender",
    "ModWhl",
    "Aftertouch",
    "Random",
    "Ctrl A",
    "Ctrl B",
];

include!("es2_params.rs");

#[derive(Clone, Copy, Debug)]
pub(crate) struct Es2Shared {
    rate: f32,
    p: [f32; 190],
    lfo1: f32,
    lfo2: f32,
    pad: (f32, f32),
    ctrl_a: f32,
    ctrl_b: f32,
}

impl Es2Shared {
    #[inline]
    fn g(&self, i: usize) -> f32 {
        self.p[i]
    }
}

pub(crate) struct Es2Voice {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    osc: [[Osc; 3]; 2],
    sine: Osc,
    fm_ph: [f32; 2],
    env1: Adsr,
    env2: Adsr,
    env3: Adsr,
    f1: [Svf; 2],
    f2: [Ladder; 2],
    rng: Rng,
    rnd: f32,
    analog: [f32; 3],
    fade: Fade,
    killing: bool,
    level: f32,
    uni: usize,
}

fn wave_of(v: usize) -> Option<Wave> {
    match v {
        0 => Some(Wave::Sine),
        1 => Some(Wave::Triangle),
        2 => Some(Wave::Saw),
        3 | 4 => Some(Wave::Pulse),
        _ => None,
    }
}

impl Es2Voice {
    fn new(seed: u32) -> Es2Voice {
        Es2Voice {
            on: false,
            note: 60,
            vel: 1.0,
            pitch: 60.0,
            target: 60.0,
            osc: [[Osc::default(); 3]; 2],
            sine: Osc::default(),
            fm_ph: [0.0; 2],
            env1: Adsr::default(),
            env2: Adsr::default(),
            env3: Adsr::default(),
            f1: [Svf::default(); 2],
            f2: [Ladder::new(); 2],
            rng: Rng::new(seed),
            rnd: 0.0,
            analog: [0.0; 3],
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            uni: 1,
        }
    }
    pub(crate) fn legato(&mut self, note: u8) {
        self.note = note;
        self.target = note as f32;
    }
}

impl Voice<Es2Shared> for Es2Voice {
    fn start(&mut self, note: u8, velocity: u8, sh: &Es2Shared, _ctl: &Ctl) {
        let rate = sh.rate;
        let was = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.target = note as f32;
        if !(was && sh.g(7) > 0.0) {
            self.pitch = self.target;
        }
        self.vel = velocity.max(1) as f32 / 127.0;
        self.rnd = self.rng.bipolar();
        let analog = sh.g(3);
        for a in &mut self.analog {
            *a = self.rng.bipolar() * analog * 0.12;
        }
        self.uni = if sh.g(9) >= 0.5 { 2 } else { 1 };
        let start = sh.g(5).round() as i32;
        for u in 0..2 {
            for o in 0..3 {
                self.osc[u][o].phase = if start == 0 { self.rng.unit() } else { 0.0 };
            }
        }
        let v = self.vel;
        let lerpv = |a: f32, b: f32| a + (b - a) * v;
        self.env1.set(
            lerpv(sh.g(98), sh.g(99)).max(0.0005),
            sh.g(100),
            0.0,
            sh.g(100),
            rate,
        );
        self.env2.set(
            lerpv(sh.g(101), sh.g(102)).max(0.0005),
            sh.g(103),
            sh.g(105),
            sh.g(106),
            rate,
        );
        self.env3.set(
            lerpv(sh.g(108), sh.g(109)).max(0.0005),
            sh.g(110),
            sh.g(112),
            sh.g(113),
            rate,
        );
        for e in [&mut self.env1, &mut self.env2, &mut self.env3] {
            if was { e.gate_on() } else { e.trigger() }
        }
        if !was || sh.g(4) >= 0.5 {
            for f in &mut self.f1 {
                f.reset();
            }
            for f in &mut self.f2 {
                f.reset();
            }
        }
    }
    fn release(&mut self, _sh: &Es2Shared, _ctl: &Ctl) {
        self.env1.gate_off();
        self.env2.gate_off();
        self.env3.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.003, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &Es2Shared, ctl: &Ctl) {
        let rate = sh.rate;
        let glide = sh.g(7);
        if glide > 0.0 {
            self.pitch = self.target
                + (self.pitch - self.target) * dsp::tau_coef(glide * 0.3, rate / CHUNK as f32);
        } else {
            self.pitch = self.target;
        }
        // modulation router
        let src = |s: usize, v: &Es2Voice| -> f32 {
            match s {
                1 => sh.lfo1,
                2 => sh.lfo2,
                3 => v.env1.level,
                4 => v.env2.level,
                5 => v.env3.level,
                6 => sh.pad.0,
                7 => sh.pad.1,
                8 => 1.0,
                9 => (v.note as f32 - 60.0) / 60.0,
                10 => v.vel,
                11 => ctl.bend / 12.0,
                12 => ctl.mod_wheel,
                13 => ctl.aftertouch,
                14 => v.rnd,
                15 => sh.ctrl_a,
                16 => sh.ctrl_b,
                _ => 0.0,
            }
        };
        let mut m = [0.0f32; 26];
        for k in 0..10 {
            let b = 41 + k * 5;
            if sh.g(147 + k * 2) < 0.5 {
                continue;
            }
            let t = sh.g(b).round() as usize;
            let s = sh.g(b + 1).round() as usize;
            if t == 0 || t >= 26 || s == 0 {
                continue;
            }
            let via = sh.g(b + 2).round() as usize;
            let mut amt = sh.g(b + 3);
            if via > 0 {
                let mut vv = src(via, self);
                if sh.g(148 + k * 2) >= 0.5 {
                    vv = 1.0 - vv;
                }
                amt += sh.g(b + 4) * vv;
            }
            m[t] += src(s, self) * amt;
        }
        let analog_rnd = self.analog;
        let base = self.pitch + sh.g(2) / 100.0 + ctl.bend + m[1] * 24.0;
        let semis = [
            sh.g(12) + sh.g(13) / 100.0 + m[2] * 24.0,
            sh.g(17) + sh.g(18) / 100.0 + m[3] * 24.0,
            sh.g(22) + sh.g(23) / 100.0 + m[4] * 24.0,
        ];
        let det = m[5] * 0.5;
        let mut hz = [0.0f32; 3];
        for o in 0..3 {
            hz[o] = lg_dsp::midi_hz(base + semis[o] + analog_rnd[o] + det * (o as f32 - 1.0));
        }
        let waves = [
            sh.g(14).round() as usize,
            sh.g(19).round() as usize,
            sh.g(24).round() as usize,
        ];
        let shape = [
            (m[6] + m[7]).clamp(-1.0, 1.0),
            (m[6] + m[8]).clamp(-1.0, 1.0),
            (m[6] + m[9]).clamp(-1.0, 1.0),
        ];
        let digi = [sh.g(15), sh.g(20), sh.g(25)];
        // triangle mixer → osc weights
        let x = (sh.g(26) + m[11]).clamp(0.0, 1.0);
        let y = (sh.g(27) + m[12]).clamp(0.0, 1.0);
        let w1 = y;
        let w2 = (1.0 - y) * (1.0 - x);
        let w3 = (1.0 - y) * x;
        let lv = [
            w1 * sh.g(187) * (1.0 + m[10]).max(0.0) * if sh.g(11) >= 0.5 { 1.0 } else { 0.0 },
            w2 * sh.g(188) * if sh.g(16) >= 0.5 { 1.0 } else { 0.0 },
            w3 * sh.g(189) * if sh.g(21) >= 0.5 { 1.0 } else { 0.0 },
        ];
        let sine_lvl = (sh.g(39) + m[23]).clamp(0.0, 1.0);
        // filters
        let cut = |c: f32| (20.0 * 1000f32.powf(c.clamp(0.0, 1.0))).min(rate * 0.45);
        let c1 = cut(sh.g(32) + m[13] + m[14] + m[18] + (ctl.brightness - 0.5) * 0.2);
        let c2 = cut(sh.g(36) + m[16] + m[14] - m[18] + (ctl.brightness - 0.5) * 0.2);
        let r1 = (sh.g(33) + m[15]).clamp(0.0, 1.0);
        let r2 = (sh.g(37) + m[17]).clamp(0.0, 1.0);
        let f1_mode = match sh.g(31).round() as i32 {
            1 => SvfMode::High,
            2 => SvfMode::Peak,
            3 => SvfMode::Notch,
            4 => SvfMode::Band,
            _ => SvfMode::Low,
        };
        for c in 0..2 {
            self.f1[c].mode = f1_mode;
            self.f1[c].set(c1, 0.5 + r1 * 15.0, rate);
            self.f2[c].set(c2, r2, rate);
            self.f2[c].poles = if sh.g(34) < 0.5 { 2 } else { 4 };
            self.f2[c].drive = 1.0 + sh.g(28) * 4.0;
        }
        let blend = (sh.g(30) + m[20]).clamp(-1.0, 1.0);
        let parallel = sh.g(29) >= 0.5;
        let fat = sh.g(35);
        let fm2 = sh.g(38) + m[19];
        let amp_vel = 1.0 - sh.g(114) + sh.g(114) * self.vel;
        let gain = 0.3 * amp_vel * (1.0 + m[21]).max(0.0) / (self.uni as f32).sqrt();
        let pan = m[22].clamp(-1.0, 1.0);
        let (pl, pr) = ((1.0 - pan).sqrt(), (1.0 + pan).sqrt());
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let mut out = [0.0f32; 2];
            for u in 0..self.uni {
                let ud = if self.uni > 1 {
                    dsp::semis(if u == 0 { -0.08 } else { 0.08 })
                } else {
                    1.0
                };
                let mut s = [0.0f32; 3];
                // osc 2 first (osc 1 FM, ring and sync partners)
                let inc2 = hz[1] * ud / rate;
                let inc1 = hz[0] * ud / rate;
                let inc3 = hz[2] * ud / rate;
                // osc 1
                s[0] = match waves[0] {
                    5 => {
                        self.osc[u][0].advance(inc1);
                        table_read(
                            ((digi[0] / 99.0) + shape[0] * 0.5).clamp(0.0, 1.0),
                            self.osc[u][0].phase,
                            hz[0],
                        )
                    }
                    6 => {
                        self.fm_ph[u] = (self.fm_ph[u] + inc2).fract();
                        self.osc[u][0].advance(inc1);
                        let idx = (0.5 + shape[0] * 0.5).max(0.0) * 3.0;
                        dsp::sin_cycles(
                            self.osc[u][0].phase
                                + dsp::sin_cycles(self.fm_ph[u]) * idx / std::f32::consts::TAU,
                        )
                    }
                    7 => self.rng.bipolar(),
                    w => {
                        let pw = if w == 4 {
                            0.5 + 0.45 * (shape[0] * 0.5 + 0.3)
                        } else {
                            0.5 + 0.45 * shape[0] * if w == 3 { 1.0 } else { 0.0 }
                        };
                        self.osc[u][0].next(wave_of(w).unwrap_or(Wave::Saw), inc1, pw)
                    }
                };
                let wrapped1 = self.osc[u][0].phase < inc1;
                for o in 1..3 {
                    let inc = if o == 1 { inc2 } else { inc3 };
                    let w = waves[o];
                    let (ring, sync, noise, digiw) = if o == 1 {
                        (w == 6, w == 7, w == 8, w == 5)
                    } else {
                        (false, w == 6, w == 7, w == 5)
                    };
                    if sync && wrapped1 {
                        self.osc[u][o].phase = 0.0;
                    }
                    s[o] = if noise {
                        self.rng.bipolar()
                    } else if digiw {
                        self.osc[u][o].advance(inc);
                        table_read(
                            ((digi[o] / 99.0) + shape[o] * 0.5).clamp(0.0, 1.0),
                            self.osc[u][o].phase,
                            hz[o],
                        )
                    } else if ring {
                        self.osc[u][o].next(Wave::Pulse, inc, 0.5 + 0.45 * shape[o]) * s[0]
                    } else if sync {
                        self.osc[u][o].next(Wave::Saw, inc, 0.5)
                    } else {
                        let pw = 0.5
                            + 0.45 * shape[o] * if w >= 3 { 1.0 } else { 0.0 }
                            + if w == 4 { 0.25 } else { 0.0 };
                        self.osc[u][o].next(
                            wave_of(w).unwrap_or(Wave::Saw),
                            inc,
                            pw.clamp(0.05, 0.95),
                        )
                    };
                }
                let mut x = s[0] * lv[0] + s[1] * lv[1] + s[2] * lv[2];
                if u == 0 && sine_lvl > 0.0 {
                    x += self.sine.next(Wave::Sine, inc1, 0.5) * sine_lvl;
                }
                out[u] = x;
            }
            if self.uni == 1 {
                out[1] = out[0];
            }
            let e1 = self.env1.next();
            let e2 = self.env2.next();
            let _ = (e1, e2);
            let a = self.env3.next();
            let mut ys = [0.0f32; 2];
            for c in 0..2 {
                let xin = out[c];
                let fmx = if fm2 > 0.0 { xin * fm2 * 0.5 } else { 0.0 };
                let _ = fmx;
                let y = if parallel {
                    let a1 = self.f1[c].run(xin);
                    let a2 = self.f2[c].run(xin);
                    a1 * (1.0 - blend) * 0.5 + a2 * (1.0 + blend) * 0.5
                } else {
                    let a1 = self.f1[c].run(xin);
                    let inp = if blend < 0.0 {
                        a1
                    } else {
                        a1 * (1.0 - blend) + xin * blend
                    };
                    let a2 = self.f2[c].run(inp);
                    if blend < 0.0 {
                        a2 * (1.0 + blend) + a1 * -blend
                    } else {
                        a2
                    }
                };
                ys[c] = y + xin * fat * 0.3;
            }
            let mut g = a * gain;
            if self.killing {
                g *= self.fade.next();
            }
            let yl = ys[0] * g * pl;
            let yr = ys[1] * g * pr;
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.env3.idle() || (self.killing && self.fade.gain <= 0.0) {
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

pub struct Es2 {
    params: Params,
    poly: Poly<Es2Voice>,
    ch: Channel,
    sh: Es2Shared,
    rate: f32,
    lfo1: Lfo,
    lfo2: Lfo,
    rng: Rng,
    chorus: Chorus,
    flanger: Flanger,
    phaser: Phaser,
    dist_lp: dsp::OnePole,
    stack: [u8; 16],
    nstack: usize,
    age: f32,
}

impl Es2 {
    pub fn new(rate: f32) -> Es2 {
        dsp::warm_tables();
        let _ = crate::retro::tables();
        let mut seed = 41;
        let mut s = Es2 {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 19;
                Es2Voice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: Es2Shared {
                rate,
                p: [0.0; 190],
                lfo1: 0.0,
                lfo2: 0.0,
                pad: (0.0, 0.0),
                ctrl_a: 0.0,
                ctrl_b: 0.0,
            },
            rate,
            lfo1: Lfo::default(),
            lfo2: Lfo::default(),
            rng: Rng::new(5),
            chorus: Chorus::new(rate),
            flanger: Flanger::new(rate),
            phaser: Phaser::default(),
            dist_lp: dsp::OnePole::default(),
            stack: [0; 16],
            nstack: 0,
            age: 0.0,
        };
        s.update();
        s
    }
    fn update(&mut self) {
        for i in 0..190 {
            self.sh.p[i] = self.params.get(i as u32);
        }
        let mono = self.sh.g(8).round() as i32 > 0 || self.sh.g(184).round() as i32 > 0;
        self.poly.set_max(if mono {
            1
        } else {
            self.sh.g(1).round().clamp(1.0, 32.0) as usize
        });
        self.ch.bend_up = self.sh.g(6);
        self.ch.bend_down = if self.sh.g(167) > 0.0 {
            self.sh.g(167)
        } else {
            self.sh.g(6)
        };
        self.ch.update_bend();
        self.dist_lp
            .lowpass_hz(1500.0 + 15000.0 * self.sh.g(118), self.rate);
    }
    fn mono(&self) -> bool {
        self.sh.g(8).round() as i32 > 0 || self.sh.g(184).round() as i32 > 0
    }
}

impl Plugin for Es2 {
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
        self.nstack = 0;
        self.chorus.reset();
        self.flanger.reset();
        self.phaser.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.sh.g(113) + 0.2
    }
}

impl Instrument for Es2 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.mono() {
            if self.nstack < 16 {
                self.stack[self.nstack] = pitch;
                self.nstack += 1;
            }
            let legato = self.sh.g(8).round() as i32 == 2 || self.sh.g(184).round() as i32 == 2;
            if legato && self.nstack > 1 {
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                    v.legato(pitch);
                    return;
                }
            }
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                let sh = self.sh;
                v.start(pitch, velocity, &sh, &self.ch.ctl);
                return;
            }
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.mono() {
            if let Some(i) = self.stack[..self.nstack].iter().position(|&n| n == pitch) {
                self.stack.copy_within(i + 1..self.nstack, i);
                self.nstack -= 1;
            }
            if self.nstack > 0 {
                let top = self.stack[self.nstack - 1];
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                    v.legato(top);
                }
                return;
            }
            let sh = self.sh;
            if !self.ch.ctl.sustain {
                for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                    v.release(&sh, &self.ch.ctl);
                }
            }
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.nstack = 0;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        match cc {
            74 => self.sh.ctrl_a = value as f32 / 127.0,
            2 => self.sh.ctrl_b = value as f32 / 127.0,
            _ => {}
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
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.sh.g(40));
        let dist = self.sh.g(117);
        let hard = self.sh.g(116) >= 0.5;
        let fxm = self.sh.g(119).round() as i32;
        let fx_int = self.sh.g(121);
        let fx_rate = self.sh.g(120);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            let dt = m as f32 / self.rate;
            self.age += dt;
            // LFO1 with its delay/fade (LFO1 EG: + = fade in, − = fade out), rate modulated by the router
            let eg = self.sh.g(93);
            let fade = if eg > 0.0 {
                (self.age / (eg * 4.0)).min(1.0)
            } else if eg < 0.0 {
                (1.0 - self.age / (-eg * 4.0)).max(0.0)
            } else {
                1.0
            };
            self.sh.lfo1 = self.lfo1.step(
                self.sh.g(91).round() as u8,
                self.sh.g(92) * dt,
                &mut self.rng,
            ) * fade;
            self.sh.lfo2 = self.lfo2.step(
                self.sh.g(94).round() as u8,
                self.sh.g(95) * dt,
                &mut self.rng,
            );
            self.sh.pad = (self.sh.g(122), self.sh.g(123));
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if dist > 0.0 {
                let g = 1.0 + dist * 20.0;
                for i in 0..m {
                    for x in [&mut sl[i], &mut sr[i]] {
                        let y = if hard {
                            (*x * g).clamp(-1.0, 1.0)
                        } else {
                            dsp::soft_clip(*x * g)
                        };
                        *x = y / g.sqrt();
                    }
                    sl[i] = self.dist_lp.lp(sl[i]);
                    sr[i] = sl[i] * 0.5 + sr[i] * 0.5;
                }
            }
            if fx_int > 0.0 {
                match fxm {
                    0 => self
                        .chorus
                        .process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int),
                    1 => self.flanger.process(
                        &mut sl[..m],
                        &mut sr[..m],
                        fx_rate,
                        fx_int,
                        fx_int * 0.5,
                    ),
                    2 => self.phaser.process(
                        &mut sl[..m],
                        &mut sr[..m],
                        fx_rate,
                        fx_int * 0.7,
                        0.5,
                        1.0,
                        self.rate,
                    ),
                    _ => {}
                }
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
        if self.poly.voices.iter().all(|v| !v.on) {
            self.age = 0.0;
        }
    }
}
