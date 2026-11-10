//! "Studio Bass" stand-in: modelled electric and upright basses (waveguide string, plucking
//! style, magnetic pickup position, amp tone), for Logic's bass patches.

use crate::dsp::{self, Biquad, CHUNK, Fade, OnePole, Rng, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::string::WaveString;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODELS: &[&str] = &[
    "Finger",
    "Pick",
    "Slap",
    "Upright",
    "Fretless",
    "Muted",
    "Vintage Flatwound",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Model", MODELS, 0.0),
    Param::new(1, "Tone", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(2, "Decay", 0.1, 3.0, 1.0, Unit::Ratio),
    Param::new(3, "Pluck Position", 0.02, 0.5, 0.15, Unit::Ratio),
    Param::new(4, "Pickup Position", 0.02, 0.5, 0.12, Unit::Ratio),
    Param::new(5, "Drive", 0.0, 1.0, 0.15, Unit::Ratio),
    Param::new(6, "Velocity Sensitivity", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(7, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(8, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(9, "Release", 0.01, 1.0, 0.08, Unit::Seconds),
    Param::new(10, "Finger Noise", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(11, "Polyphony", 1.0, 16.0, 6.0, Unit::None),
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct BassShared {
    rate: f32,
    model: u8,
    tone: f32,
    decay: f32,
    pluck: f32,
    pickup: f32,
    vel_sens: f32,
    release: f32,
    noise: f32,
}

pub(crate) struct BassVoice {
    s: WaveString,
    on: bool,
    hz: f32,
    gain: f32,
    released: bool,
    rng: Rng,
    lp: OnePole,
    nf: Svf,
    namp: f32,
    bloom: f32,
    fade: Fade,
    killing: bool,
    level: f32,
    silent: u32,
}

impl Voice<BassShared> for BassVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &BassShared, _ctl: &Ctl) {
        let v = velocity.max(1) as f32 / 127.0;
        let vv = 1.0 - sh.vel_sens + sh.vel_sens * v;
        self.hz = lg_dsp::midi_hz(note as f32);
        let key = (note as f32 - 40.0) / 12.0;
        // (T60 s, brightness, pluck shape, pluck pos multiplier)
        let (t, bright, shape, pos) = match sh.model {
            1 => (5.0, 0.8, 0.85, 0.5),
            2 => (4.0, 0.9, 1.0, 2.2),
            3 => (1.8, 0.45, 0.25, 1.4),
            4 => (6.0, 0.7, 0.5, 1.0),
            5 => (0.5, 0.4, 0.3, 1.0),
            6 => (3.5, 0.45, 0.4, 1.2),
            _ => (5.0, 0.62, 0.45, 1.0),
        };
        let t60 = t * sh.decay * 2f32.powf(-key * 0.4);
        self.s.clear();
        self.s.tune(
            self.hz,
            t60,
            (bright * (0.8 + 0.3 * v) * (0.6 + 0.8 * sh.tone)).min(0.98),
        );
        self.s.pluck(
            1.0,
            (shape * (0.5 + 0.6 * v)).min(1.0),
            sh.pluck * pos,
            &mut self.rng,
        );
        self.gain = 0.5 * vv.powf(1.3);
        self.released = false;
        self.killing = false;
        self.silent = 0;
        let cut = 400.0
            + 4500.0 * sh.tone * sh.tone * (0.5 + 0.5 * v) * if sh.model == 2 { 2.0 } else { 1.0 };
        self.lp.lowpass_hz(cut, sh.rate);
        self.lp.z = 0.0;
        self.nf = Svf::new(
            SvfMode::Band,
            if sh.model == 2 { 2500.0 } else { 1200.0 },
            1.0,
            sh.rate,
        );
        self.namp = sh.noise * 0.08 * v * if sh.model == 2 { 3.0 } else { 1.0 };
        // an upright's pitch blooms down slightly after the pluck
        self.bloom = if sh.model == 3 { 0.012 * v } else { 0.003 * v };
        self.on = true;
        self.level = 1.0;
    }
    fn release(&mut self, sh: &BassShared, _ctl: &Ctl) {
        if !self.released {
            self.released = true;
            self.s.set_decay(sh.release);
            self.namp = self.namp.max(0.01 * sh.noise);
        }
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.005, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &BassShared, ctl: &Ctl) {
        self.bloom *= 0.85;
        self.s.retune(self.hz * ctl.bend_ratio * (1.0 + self.bloom));
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            self.s.tick(0.0);
            let mut y = self.s.pickup(sh.pickup) * 1.4 + self.s.pickup(0.5) * 0.25;
            y = self.lp.lp(y);
            if self.namp > 1e-6 {
                y += self.nf.run(self.rng.bipolar()) * self.namp;
                self.namp *= 0.993;
            }
            let mut g = self.gain;
            if self.killing {
                g *= self.fade.next();
            }
            let out = y * g;
            peak = peak.max(out.abs());
            l[i] += out;
            r[i] += out;
        }
        self.level = peak;
        if peak < 1e-5 {
            self.silent += l.len() as u32;
        } else {
            self.silent = 0;
        }
        if (self.killing && self.fade.gain <= 0.0) || self.silent > (sh.rate * 0.05) as u32 {
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

pub struct StudioBass {
    params: Params,
    poly: Poly<BassVoice>,
    ch: Channel,
    sh: BassShared,
    body: [Biquad; 2],
    rate: f32,
}

impl StudioBass {
    pub fn new(rate: f32) -> StudioBass {
        dsp::warm_tables();
        let mut seed = 500;
        let mut b = StudioBass {
            params: Params::new(PARAMS),
            poly: Poly::new(16, || {
                seed += 3;
                BassVoice {
                    s: WaveString::new(25.0, rate),
                    on: false,
                    hz: 55.0,
                    gain: 0.0,
                    released: false,
                    rng: Rng::new(seed),
                    lp: OnePole::default(),
                    nf: Svf::default(),
                    namp: 0.0,
                    bloom: 0.0,
                    fade: Fade::default(),
                    killing: false,
                    level: 0.0,
                    silent: 0,
                }
            }),
            ch: Channel::new(rate),
            sh: BassShared {
                rate,
                model: 0,
                tone: 0.5,
                decay: 1.0,
                pluck: 0.15,
                pickup: 0.12,
                vel_sens: 0.8,
                release: 0.08,
                noise: 0.4,
            },
            body: [Biquad::default(); 2],
            rate,
        };
        b.update();
        b
    }
    fn p(&self, i: u32) -> f32 {
        self.params.get(i)
    }
    fn update(&mut self) {
        self.sh.model = self.p(0).round() as u8;
        self.sh.tone = self.p(1);
        self.sh.decay = self.p(2);
        self.sh.pluck = self.p(3);
        self.sh.pickup = self.p(4);
        self.sh.vel_sens = self.p(6);
        self.sh.release = self.p(9);
        self.sh.noise = self.p(10);
        self.poly.set_max(self.p(11) as usize);
        self.ch.bend_up = self.p(8);
        self.ch.bend_down = self.p(8);
        self.ch.update_bend();
        let up = self.sh.model == 3;
        self.body[0].peaking(
            if up { 110.0 } else { 80.0 },
            1.0,
            if up { 5.0 } else { 2.0 },
            self.rate,
        );
        self.body[1].peaking(
            if up { 280.0 } else { 700.0 },
            1.2,
            if up { 3.0 } else { -2.0 },
            self.rate,
        );
    }
}

impl Plugin for StudioBass {
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
        0.5
    }
}

impl Instrument for StudioBass {
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
        let vol = lg_dsp::db_to_gain(self.p(7));
        let drive = self.p(5);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                let b0 = self.body[0].run(sl[i]);
                let mut x = self.body[1].run(b0);
                if drive > 0.0 {
                    let g = 1.0 + drive * 6.0;
                    x = dsp::soft_clip(x * g) / g.sqrt();
                }
                left[pos + i] += x * gl * vol;
                right[pos + i] += x * gr * vol;
            }
            pos += m;
        }
        let _ = sr;
    }
}
