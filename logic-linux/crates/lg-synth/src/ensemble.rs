//! "Studio Ensemble": sustained-section sounds for Logic's strings, brass, woodwind, choir, pad
//! and tape-replay (Mellotron-type) patches. Per voice: a stack of detuned saw/pulse oscillators
//! (players in a section), breath noise, a three-formant body/vowel filter bank, low-pass with an
//! envelope that opens with velocity (brass "blat"), delayed vibrato; then a three-phase ensemble
//! chorus, tape wow/flutter for the tape models, and a hall.

use crate::dsp::{self, Adsr, CHUNK, Chorus, Fade, Osc, Rng, Room, Svf, SvfMode, Wave};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODELS: &[&str] = &[
    "Strings Section",
    "Chamber Strings",
    "Synth Strings",
    "Brass Section",
    "Synth Brass",
    "Warm Pad",
    "Bright Pad",
    "Choir Aah",
    "Choir Ooh",
    "Flute",
    "Reed (Clarinet/Sax)",
    "Tape Strings",
    "Tape Flute",
    "Tape Choir",
    "Harmonium",
    "Solo Strings",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Model", MODELS, 0.0),
    Param::new(1, "Attack", 0.001, 5.0, 0.25, Unit::Seconds),
    Param::new(2, "Release", 0.01, 8.0, 0.6, Unit::Seconds),
    Param::new(3, "Brightness", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(4, "Section Size", 1.0, 5.0, 3.0, Unit::None),
    Param::new(5, "Detune", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(6, "Vibrato", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(7, "Ensemble", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(8, "Hall", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(9, "Velocity Sensitivity", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(10, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(11, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(12, "Polyphony", 1.0, 32.0, 16.0, Unit::None),
];

#[derive(Clone, Copy, Debug)]
struct Character {
    wave: Wave,
    pw: f32,
    unison: usize,
    detune_cents: f32,
    breath: f32,
    formants: [(f32, f32, f32); 3],
    formant_mix: f32,
    cutoff: f32,
    env_open: f32,
    attack: f32,
    release: f32,
    vibrato: f32,
    vib_delay: f32,
    tape: bool,
    level: f32,
}

fn character(model: u8) -> Character {
    let strings = [(450.0, 1.2, 1.0), (1100.0, 1.5, 0.6), (2600.0, 2.0, 0.35)];
    let ah = [(800.0, 6.0, 1.0), (1150.0, 6.0, 0.6), (2900.0, 8.0, 0.3)];
    let oo = [(350.0, 6.0, 1.0), (600.0, 6.0, 0.5), (2400.0, 8.0, 0.15)];
    let brass = [(1100.0, 1.5, 1.0), (1700.0, 2.0, 0.6), (3000.0, 2.5, 0.3)];
    let base = Character {
        wave: Wave::Saw,
        pw: 0.5,
        unison: 3,
        detune_cents: 12.0,
        breath: 0.0,
        formants: strings,
        formant_mix: 0.5,
        cutoff: 5000.0,
        env_open: 0.0,
        attack: 0.25,
        release: 0.6,
        vibrato: 0.15,
        vib_delay: 0.4,
        tape: false,
        level: 1.0,
    };
    match model {
        1 => Character {
            unison: 2,
            detune_cents: 7.0,
            cutoff: 6000.0,
            attack: 0.12,
            ..base
        },
        2 => Character {
            wave: Wave::Saw,
            unison: 4,
            detune_cents: 18.0,
            formant_mix: 0.1,
            cutoff: 3500.0,
            vibrato: 0.0,
            ..base
        },
        3 => Character {
            unison: 3,
            detune_cents: 8.0,
            formants: brass,
            formant_mix: 0.6,
            cutoff: 900.0,
            env_open: 3500.0,
            attack: 0.04,
            release: 0.25,
            vibrato: 0.1,
            level: 1.8,
            ..base
        },
        4 => Character {
            unison: 2,
            detune_cents: 10.0,
            formant_mix: 0.0,
            cutoff: 700.0,
            env_open: 4000.0,
            attack: 0.03,
            release: 0.3,
            vibrato: 0.0,
            ..base
        },
        5 => Character {
            wave: Wave::Pulse,
            pw: 0.35,
            unison: 3,
            detune_cents: 14.0,
            formant_mix: 0.0,
            cutoff: 1400.0,
            attack: 0.6,
            release: 1.5,
            vibrato: 0.0,
            ..base
        },
        6 => Character {
            unison: 4,
            detune_cents: 20.0,
            formant_mix: 0.0,
            cutoff: 7000.0,
            attack: 0.3,
            release: 1.2,
            vibrato: 0.0,
            ..base
        },
        7 => Character {
            wave: Wave::Saw,
            unison: 4,
            detune_cents: 10.0,
            breath: 0.05,
            formants: ah,
            formant_mix: 0.95,
            cutoff: 4000.0,
            attack: 0.3,
            release: 0.8,
            vibrato: 0.25,
            level: 2.5,
            ..base
        },
        8 => Character {
            wave: Wave::Triangle,
            unison: 4,
            detune_cents: 10.0,
            breath: 0.05,
            formants: oo,
            formant_mix: 0.95,
            cutoff: 2500.0,
            attack: 0.35,
            release: 0.8,
            vibrato: 0.25,
            level: 2.5,
            ..base
        },
        9 => Character {
            wave: Wave::Sine,
            unison: 1,
            detune_cents: 0.0,
            breath: 0.12,
            formant_mix: 0.0,
            cutoff: 6000.0,
            attack: 0.06,
            release: 0.15,
            vibrato: 0.3,
            vib_delay: 0.3,
            level: 0.8,
            ..base
        },
        10 => Character {
            wave: Wave::Pulse,
            pw: 0.5,
            unison: 1,
            detune_cents: 0.0,
            breath: 0.06,
            formants: [(1400.0, 2.0, 1.0), (2200.0, 3.0, 0.5), (3500.0, 3.0, 0.2)],
            formant_mix: 0.4,
            cutoff: 2500.0,
            env_open: 1500.0,
            attack: 0.04,
            release: 0.12,
            vibrato: 0.2,
            ..base
        },
        11 => Character {
            tape: true,
            unison: 3,
            detune_cents: 9.0,
            cutoff: 3500.0,
            attack: 0.08,
            release: 0.2,
            vibrato: 0.05,
            ..base
        },
        12 => Character {
            tape: true,
            wave: Wave::Sine,
            unison: 1,
            breath: 0.1,
            formant_mix: 0.0,
            cutoff: 4000.0,
            attack: 0.07,
            release: 0.2,
            vibrato: 0.2,
            level: 1.4,
            ..base
        },
        13 => Character {
            tape: true,
            unison: 3,
            detune_cents: 9.0,
            breath: 0.03,
            formants: ah,
            formant_mix: 0.95,
            cutoff: 3000.0,
            attack: 0.1,
            release: 0.25,
            vibrato: 0.1,
            level: 2.5,
            ..base
        },
        14 => Character {
            wave: Wave::Pulse,
            pw: 0.3,
            unison: 2,
            detune_cents: 4.0,
            formants: [(900.0, 2.0, 1.0), (1800.0, 2.0, 0.5), (3200.0, 2.0, 0.3)],
            formant_mix: 0.4,
            cutoff: 3000.0,
            attack: 0.08,
            release: 0.15,
            vibrato: 0.0,
            ..base
        },
        15 => Character {
            unison: 1,
            detune_cents: 0.0,
            breath: 0.02,
            cutoff: 7000.0,
            attack: 0.1,
            vibrato: 0.35,
            vib_delay: 0.25,
            level: 1.3,
            ..base
        },
        _ => base,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnsShared {
    rate: f32,
    ch: Character,
    attack: f32,
    release: f32,
    bright: f32,
    size: usize,
    detune: f32,
    vibrato: f32,
    vel_sens: f32,
    vib_phase: f32,
    wow: f32,
}

pub(crate) struct EnsVoice {
    on: bool,
    hz: f32,
    vel: f32,
    osc: [Osc; 5],
    det: [f32; 5],
    pans: [(f32, f32); 5],
    env: Adsr,
    fenv: Adsr,
    lp: [Svf; 2],
    form: [Svf; 3],
    rng: Rng,
    age: f32,
    fade: Fade,
    killing: bool,
    gain: f32,
    level: f32,
    tape_left: u32,
}

impl Voice<EnsShared> for EnsVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &EnsShared, _ctl: &Ctl) {
        let c = sh.ch;
        let rate = sh.rate;
        self.hz = lg_dsp::midi_hz(note as f32);
        self.vel = velocity.max(1) as f32 / 127.0;
        let n = (c.unison.min(sh.size).max(1)).min(5);
        for i in 0..5 {
            self.osc[i].phase = self.rng.unit();
            let x = if n > 1 {
                i as f32 / (n - 1) as f32 * 2.0 - 1.0
            } else {
                0.0
            };
            self.det[i] = dsp::semis(
                x * c.detune_cents * (0.3 + 1.4 * sh.detune) / 100.0
                    + 0.02 * self.rng.bipolar() / 100.0,
            );
            let p = x * 0.7;
            self.pans[i] = ((1.0 - p).sqrt(), (1.0 + p).sqrt());
        }
        self.env.set(
            sh.attack * c.attack / 0.25,
            1.0,
            1.0,
            sh.release * c.release / 0.6,
            rate,
        );
        self.env.trigger();
        self.fenv.set(0.005 + c.attack * 0.5, 0.4, 0.3, 0.3, rate);
        self.fenv.trigger();
        for (k, f) in self.form.iter_mut().enumerate() {
            let (fq, q, _) = c.formants[k];
            *f = Svf::new(SvfMode::Band, fq, q, rate);
        }
        for f in &mut self.lp {
            f.reset();
            f.mode = SvfMode::Low;
        }
        self.age = 0.0;
        self.killing = false;
        let vv = 1.0 - sh.vel_sens + sh.vel_sens * self.vel;
        self.gain = 0.16 * c.level * vv / (n as f32).sqrt();
        self.on = true;
        self.level = 1.0;
        // a tape frame lasts about eight seconds
        self.tape_left = if c.tape {
            (8.0 * rate) as u32
        } else {
            u32::MAX
        };
    }
    fn release(&mut self, _sh: &EnsShared, _ctl: &Ctl) {
        self.env.gate_off();
        self.fenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.005, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &EnsShared, ctl: &Ctl) {
        let c = sh.ch;
        let rate = sh.rate;
        let n = c.unison.min(sh.size).clamp(1, 5);
        self.age += l.len() as f32 / rate;
        let vib_amt = ((self.age - c.vib_delay) / 0.6).clamp(0.0, 1.0)
            * (c.vibrato + sh.vibrato * 0.3)
            + ctl.mod_wheel * 0.5;
        let vib = 1.0 + 0.004 * vib_amt * dsp::sin_cycles(sh.vib_phase);
        let base = self.hz * ctl.bend_ratio * vib * (1.0 + sh.wow);
        let open = c.env_open * self.fenv.level * (0.3 + 0.7 * self.vel);
        let fc = ((c.cutoff * (0.4 + 1.2 * sh.bright) + open)
            * (0.6 + 0.4 * self.vel)
            * (1.0 + (ctl.brightness - 0.5)))
            .clamp(100.0, rate * 0.45);
        for f in &mut self.lp {
            f.set(fc, 0.7, rate);
        }
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let mut sl = 0.0;
            let mut sr = 0.0;
            for k in 0..n {
                let s = self.osc[k].next(c.wave, base * self.det[k] / rate, c.pw);
                sl += s * self.pans[k].0;
                sr += s * self.pans[k].1;
            }
            let breath = self.rng.bipolar() * c.breath;
            let mut ys = [sl + breath, sr + breath];
            if c.formant_mix > 0.0 {
                for y in ys.iter_mut() {
                    let x = *y;
                    let mut f = 0.0;
                    for (k, fm) in self.form.iter_mut().enumerate() {
                        f += fm.run_bp_norm(x) * c.formants[k].2;
                    }
                    *y = x * (1.0 - c.formant_mix) + f * c.formant_mix * 1.5;
                }
            }
            let fe = self.fenv.next();
            let _ = fe;
            let a = self.env.next();
            let mut g = a * self.gain;
            if self.tape_left != u32::MAX {
                if self.tape_left == 0 {
                    g = 0.0;
                } else {
                    self.tape_left -= 1;
                    if self.tape_left < 2400 {
                        g *= self.tape_left as f32 / 2400.0;
                    }
                }
            }
            if self.killing {
                g *= self.fade.next();
            }
            let yl = self.lp[0].run(ys[0]) * g;
            let yr = self.lp[1].run(ys[1]) * g;
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.env.idle() || (self.killing && self.fade.gain <= 0.0) || self.tape_left == 0 {
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

pub struct StudioEnsemble {
    params: Params,
    poly: Poly<EnsVoice>,
    ch: Channel,
    sh: EnsShared,
    chorus: Chorus,
    room: Room,
    rate: f32,
    rng: Rng,
    wow_phase: f32,
    flutter: f32,
    tape_lp: [Svf; 2],
}

impl StudioEnsemble {
    pub fn new(rate: f32) -> StudioEnsemble {
        dsp::warm_tables();
        let mut seed = 900;
        let mut e = StudioEnsemble {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 7;
                EnsVoice {
                    on: false,
                    hz: 440.0,
                    vel: 1.0,
                    osc: [Osc::default(); 5],
                    det: [1.0; 5],
                    pans: [(1.0, 1.0); 5],
                    env: Adsr::default(),
                    fenv: Adsr::default(),
                    lp: [Svf::default(); 2],
                    form: [Svf::default(); 3],
                    rng: Rng::new(seed),
                    age: 0.0,
                    fade: Fade::default(),
                    killing: false,
                    gain: 0.0,
                    level: 0.0,
                    tape_left: 0,
                }
            }),
            ch: Channel::new(rate),
            sh: EnsShared {
                rate,
                ch: character(0),
                attack: 0.25,
                release: 0.6,
                bright: 0.5,
                size: 3,
                detune: 0.4,
                vibrato: 0.3,
                vel_sens: 0.7,
                vib_phase: 0.0,
                wow: 0.0,
            },
            chorus: Chorus::new(rate),
            room: Room::new(rate, 1.0),
            rate,
            rng: Rng::new(31),
            wow_phase: 0.0,
            flutter: 0.0,
            tape_lp: [Svf::new(SvfMode::Low, 6000.0, 0.7, rate); 2],
        };
        e.room.feedback = 0.84;
        e.room.damp = 0.45;
        e.update();
        e
    }
    fn p(&self, i: u32) -> f32 {
        self.params.get(i)
    }
    fn update(&mut self) {
        self.sh.ch = character(self.p(0).round() as u8);
        self.sh.attack = self.p(1);
        self.sh.release = self.p(2);
        self.sh.bright = self.p(3);
        self.sh.size = self.p(4).round() as usize;
        self.sh.detune = self.p(5);
        self.sh.vibrato = self.p(6);
        self.sh.vel_sens = self.p(9);
        self.ch.bend_up = self.p(11);
        self.ch.bend_down = self.p(11);
        self.ch.update_bend();
        self.poly.set_max(self.p(12) as usize);
    }
}

impl Plugin for StudioEnsemble {
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
        self.chorus.reset();
        self.room.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.p(2) + 2.0
    }
}

impl Instrument for StudioEnsemble {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
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
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.p(10));
        let ens = self.p(7);
        let hall = self.p(8);
        let tape = self.sh.ch.tape;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.vib_phase = (self.sh.vib_phase + 5.2 * m as f32 / self.rate).fract();
            if tape {
                self.wow_phase = (self.wow_phase + 0.7 * m as f32 / self.rate).fract();
                self.flutter += (self.rng.bipolar() - self.flutter) * 0.2;
                self.sh.wow = 0.0025 * dsp::sin_cycles(self.wow_phase) + 0.0012 * self.flutter;
            } else {
                self.sh.wow = 0.0;
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if tape {
                for i in 0..m {
                    sl[i] = self.tape_lp[0].run(sl[i] + self.rng.bipolar() * 0.0015);
                    sr[i] = self.tape_lp[1].run(sr[i] + self.rng.bipolar() * 0.0015);
                }
            }
            if ens > 0.0 {
                self.chorus
                    .process(&mut sl[..m], &mut sr[..m], 0.6, ens, ens * 0.8);
            }
            self.room.process(&mut sl[..m], &mut sr[..m], hall);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}
