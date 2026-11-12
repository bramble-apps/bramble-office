//! Sculpture stand-in: component modelling. A string (waveguide) of a chosen material (inner loss,
//! stiffness via a dispersion all-pass, media loss) is excited by up to two objects (impulse,
//! strike, pluck, bow with a stick-slip friction curve, blow, noise), read by two pickups, shaped
//! by a waveshaper, a filter and a body resonator, with an amplitude envelope and vibrato.

use crate::dsp::{self, Adsr, Allpass1, Biquad, CHUNK, Fade, OnePole, Rng, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::string::WaveString;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const OBJECTS: &[&str] = &[
    "Off", "Impulse", "Strike", "Pick", "Bow", "Bow Wide", "Blow", "Noise",
];
pub const BODIES: &[&str] = &[
    "Off",
    "Guitar",
    "Violin",
    "Bass",
    "Flute",
    "Metal Plate",
    "Wood Bar",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Object 1 Type", OBJECTS, 3.0),
    Param::new(1, "Object 1 Strength", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(2, "Object 1 Position", 0.0, 1.0, 0.15, Unit::Ratio),
    Param::menu(3, "Object 2 Type", OBJECTS, 0.0),
    Param::new(4, "Object 2 Strength", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(5, "Object 2 Position", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(6, "Inner Loss", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(7, "Stiffness", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(8, "Media Loss", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(9, "Tension Mod", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(10, "Pickup A", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(11, "Pickup B", 0.0, 1.0, 0.45, Unit::Ratio),
    Param::menu(12, "Body", BODIES, 1.0),
    Param::new(13, "Body Intensity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(14, "Waveshaper", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(15, "Filter Cutoff", 0.0, 1.0, 0.85, Unit::Ratio),
    Param::new(16, "Filter Resonance", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(17, "Attack", 0.0, 5.0, 0.002, Unit::Seconds),
    Param::new(18, "Release", 0.01, 10.0, 0.4, Unit::Seconds),
    Param::new(19, "Vibrato", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(20, "Velocity Sensitivity", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(21, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(22, "Polyphony", 1.0, 16.0, 8.0, Unit::None),
    Param::new(23, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct SculptShared {
    rate: f32,
    obj: [(u8, f32, f32); 2],
    loss: f32,
    stiff: f32,
    media: f32,
    tension: f32,
    pick: [f32; 2],
    shaper: f32,
    cutoff: f32,
    res: f32,
    attack: f32,
    release: f32,
    vib: f32,
    vel_sens: f32,
    vib_phase: f32,
}

pub(crate) struct SculptVoice {
    on: bool,
    s: WaveString,
    disp: [Allpass1; 4],
    hz: f32,
    vel: f32,
    env: Adsr,
    exc_env: f32,
    lp: OnePole,
    filt: Svf,
    rng: Rng,
    bow_v: f32,
    fade: Fade,
    killing: bool,
    released: bool,
    level: f32,
    silent: u32,
}

#[inline]
fn bow_table(dv: f32) -> f32 {
    // stick-slip friction: strong grip near zero relative velocity, falling off as it slips
    let x = dv * 3.0;
    (x * (-x.abs()).exp() * 2.7).clamp(-1.0, 1.0)
}

impl Voice<SculptShared> for SculptVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &SculptShared, _ctl: &Ctl) {
        let rate = sh.rate;
        self.vel = velocity.max(1) as f32 / 127.0;
        self.hz = lg_dsp::midi_hz(note as f32);
        let key = (note as f32 - 60.0) / 12.0;
        let t60 = (12.0 * (1.0 - sh.loss).powi(2) * 2f32.powf(-key * 0.5)).clamp(0.05, 30.0);
        self.s.clear();
        self.s
            .tune(self.hz, t60, (1.0 - sh.media * 0.9).clamp(0.05, 0.99));
        for d in &mut self.disp {
            *d = Allpass1::default();
        }
        for (k, &(ty, st, pos)) in sh.obj.iter().enumerate() {
            let amp = st * (1.0 - sh.vel_sens + sh.vel_sens * self.vel);
            match ty {
                1 => self.s.pluck(amp, 1.0, pos.max(0.02), &mut self.rng),
                2 => self
                    .s
                    .pluck(amp, 0.3 + 0.5 * self.vel, pos.max(0.02), &mut self.rng),
                3 => self
                    .s
                    .pluck(amp, 0.6 + 0.4 * self.vel, pos.max(0.02), &mut self.rng),
                _ => {}
            }
            let _ = k;
        }
        self.env.set(sh.attack, 1.0, 1.0, sh.release, rate);
        self.env.trigger();
        self.exc_env = 1.0;
        self.lp
            .lowpass_hz((20.0 * 1000f32.powf(sh.cutoff)).min(rate * 0.45), rate);
        self.filt = Svf::new(
            SvfMode::Low,
            (20.0 * 1000f32.powf(sh.cutoff)).min(rate * 0.45),
            0.6 + sh.res * 8.0,
            rate,
        );
        self.bow_v = 0.0;
        self.on = true;
        self.killing = false;
        self.released = false;
        self.silent = 0;
        self.level = 1.0;
    }
    fn release(&mut self, sh: &SculptShared, _ctl: &Ctl) {
        self.released = true;
        self.env.gate_off();
        self.s.set_decay(sh.release.max(0.02));
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &SculptShared, ctl: &Ctl) {
        let vib = 1.0 + (sh.vib + ctl.mod_wheel * 0.5) * 0.006 * dsp::sin_cycles(sh.vib_phase);
        // tension modulation: louder = sharper
        let tens = 1.0 + sh.tension * 0.01 * self.level.min(1.0);
        self.s.retune(self.hz * ctl.bend_ratio * vib * tens);
        // stiffness: dispersion all-pass coefficient (higher partials travel faster)
        let a = -0.6 * sh.stiff;
        let mut peak = 0.0f32;
        let vel_amt = 1.0 - sh.vel_sens + sh.vel_sens * self.vel;
        for i in 0..l.len() {
            let held = !self.released;
            let mut force = 0.0;
            for &(ty, st, pos) in &sh.obj {
                match ty {
                    4 | 5 if held => {
                        let bow_speed = st * vel_amt * 0.25;
                        let sv = self.s.pickup(pos.max(0.02));
                        let dv = bow_speed - (sv - self.bow_v) * 8.0;
                        self.bow_v = sv;
                        let w = if ty == 5 { 0.6 } else { 1.0 };
                        force += bow_table(dv) * 0.02 * w * st + self.rng.bipolar() * 0.0005 * st;
                    }
                    6 if held => {
                        // jet: breath noise into the resonator, with a little self-oscillation
                        let sv = self.s.pickup(pos.max(0.02));
                        force += (self.rng.bipolar() * 0.004 + dsp::soft_clip(sv * 3.0) * 0.004)
                            * st
                            * vel_amt;
                    }
                    7 if held => force += self.rng.bipolar() * 0.003 * st * vel_amt,
                    _ => {}
                }
            }
            let mut y = self.s.tick(force);
            for d in &mut self.disp {
                y = d.run(y, a);
            }
            let pa = self.s.pickup(sh.pick[0].max(0.01));
            let pb = self.s.pickup(sh.pick[1].max(0.01));
            let mut x = (pa + pb) * 0.6 + y * 0.2;
            if sh.shaper > 0.0 {
                let g = 1.0 + sh.shaper * 6.0;
                x = dsp::soft_clip(x * g) / g.sqrt();
            }
            x = self.filt.run(x);
            let mut g = self.env.next() * 0.45;
            if self.killing {
                g *= self.fade.next();
            }
            let out = x * g;
            peak = peak.max(out.abs());
            l[i] += out;
            r[i] += out;
        }
        self.level = peak * 4.0;
        if peak < 1e-5 {
            self.silent += l.len() as u32;
        } else {
            self.silent = 0;
        }
        if (self.killing && self.fade.gain <= 0.0)
            || self.env.idle()
            || self.silent > (sh.rate * 0.1) as u32
        {
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

pub struct Sculpture {
    params: Params,
    poly: Poly<SculptVoice>,
    ch: Channel,
    sh: SculptShared,
    body: [[Biquad; 3]; 2],
    body_on: bool,
    rate: f32,
}

impl Sculpture {
    pub fn new(rate: f32) -> Sculpture {
        dsp::warm_tables();
        let mut seed = 303;
        let mut s = Sculpture {
            params: Params::new(PARAMS),
            poly: Poly::new(16, || {
                seed += 9;
                SculptVoice {
                    on: false,
                    s: WaveString::new(25.0, rate),
                    disp: [Allpass1::default(); 4],
                    hz: 220.0,
                    vel: 1.0,
                    env: Adsr::default(),
                    exc_env: 0.0,
                    lp: OnePole::default(),
                    filt: Svf::default(),
                    rng: Rng::new(seed),
                    bow_v: 0.0,
                    fade: Fade::default(),
                    killing: false,
                    released: false,
                    level: 0.0,
                    silent: 0,
                }
            }),
            ch: Channel::new(rate),
            sh: SculptShared {
                rate,
                obj: [(3, 0.6, 0.15), (0, 0.4, 0.3)],
                loss: 0.25,
                stiff: 0.2,
                media: 0.2,
                tension: 0.0,
                pick: [0.1, 0.45],
                shaper: 0.0,
                cutoff: 0.85,
                res: 0.1,
                attack: 0.002,
                release: 0.4,
                vib: 0.0,
                vel_sens: 0.7,
                vib_phase: 0.0,
            },
            body: [[Biquad::default(); 3]; 2],
            body_on: true,
            rate,
        };
        s.update();
        s
    }
    fn p(&self, i: u32) -> f32 {
        self.params.get(i)
    }
    fn update(&mut self) {
        let p = |i| self.params.get(i);
        self.sh.obj = [
            (p(0).round() as u8, p(1), p(2)),
            (p(3).round() as u8, p(4), p(5)),
        ];
        self.sh.loss = p(6);
        self.sh.stiff = p(7);
        self.sh.media = p(8);
        self.sh.tension = p(9);
        self.sh.pick = [p(10), p(11)];
        self.sh.shaper = p(14);
        self.sh.cutoff = p(15);
        self.sh.res = p(16);
        self.sh.attack = p(17);
        self.sh.release = p(18);
        self.sh.vib = p(19);
        self.sh.vel_sens = p(20);
        self.poly.set_max(p(22) as usize);
        self.ch.bend_up = p(23);
        self.ch.bend_down = p(23);
        self.ch.update_bend();
        let body = p(12).round() as usize;
        let k = p(13);
        let (f, g, q): ([f32; 3], [f32; 3], f32) = match body {
            1 => ([100.0, 220.0, 2500.0], [7.0, 5.0, 2.0], 1.4),
            2 => ([280.0, 450.0, 3000.0], [6.0, 4.0, 4.0], 2.0),
            3 => ([60.0, 120.0, 700.0], [6.0, 4.0, -2.0], 1.2),
            4 => ([500.0, 1500.0, 3500.0], [4.0, 3.0, 2.0], 3.0),
            5 => ([700.0, 1900.0, 4700.0], [6.0, 6.0, 6.0], 6.0),
            6 => ([400.0, 1600.0, 4400.0], [6.0, 5.0, 3.0], 4.0),
            _ => ([100.0, 1000.0, 5000.0], [0.0; 3], 1.0),
        };
        self.body_on = body != 0;
        for c in 0..2 {
            for i in 0..3 {
                self.body[c][i].peaking(f[i], q, g[i] * k, self.rate);
            }
        }
    }
}

impl Plugin for Sculpture {
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
            v.s.clear();
        }
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.p(18) + 1.0
    }
}

impl Instrument for Sculpture {
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
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
        let vol = lg_dsp::db_to_gain(self.p(21));
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.vib_phase = (self.sh.vib_phase + 5.0 * m as f32 / self.rate).fract();
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                let (mut a, mut b) = (sl[i], sr[i]);
                if self.body_on {
                    for k in 0..3 {
                        a = self.body[0][k].run(a);
                        b = self.body[1][k].run(b);
                    }
                }
                left[pos + i] += a * gl * vol;
                right[pos + i] += b * gr * vol;
            }
            pos += m;
        }
    }
}
