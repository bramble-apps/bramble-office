//! "Studio Plucks & Mallets": guitars, harp, harpsichord, koto, pizzicato (waveguide strings with
//! a body) and mallet/bell instruments (modal bars, plates and tubes with mallet hardness), for
//! Logic's guitar, mallet, bell and world patches.

use crate::dsp::{self, Biquad, CHUNK, Fade, Mode, OnePole, Rng, Room, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::string::WaveString;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODELS: &[&str] = &[
    "Steel Acoustic",
    "Nylon Classical",
    "Clean Electric",
    "Muted Electric",
    "Harp",
    "Harpsichord",
    "Koto",
    "Pizzicato",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Glockenspiel",
    "Celesta",
    "Music Box",
    "Tubular Bells",
    "Kalimba",
    "Steel Drum",
    "Bell",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Model", MODELS, 0.0),
    Param::new(1, "Brightness", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(2, "Decay", 0.1, 4.0, 1.0, Unit::Ratio),
    Param::new(3, "Release", 0.01, 3.0, 0.2, Unit::Seconds),
    Param::new(4, "Body", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(5, "Velocity Sensitivity", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(6, "Room", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(7, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(8, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(9, "Tremolo (Vibraphone Motor)", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(10, "Polyphony", 1.0, 32.0, 24.0, Unit::None),
    Param::new(11, "Stereo Width", 0.0, 1.0, 0.5, Unit::Ratio),
];

/// Modal ratio tables (partial frequency / note frequency) and relative T60s.
fn mallet_modes(model: u8) -> (&'static [f32], &'static [f32], &'static [f32], f32, bool) {
    // (ratios, amplitudes, decay multipliers, base T60 at C4, dampers)
    match model {
        8 => (
            &[1.0, 4.0, 10.0],
            &[1.0, 0.25, 0.06],
            &[1.0, 0.4, 0.2],
            6.0,
            true,
        ),
        9 => (
            &[1.0, 3.93, 9.2],
            &[1.0, 0.35, 0.12],
            &[1.0, 0.3, 0.12],
            1.4,
            false,
        ),
        10 => (
            &[1.0, 3.0, 6.0],
            &[1.0, 0.5, 0.25],
            &[1.0, 0.5, 0.3],
            0.7,
            false,
        ),
        11 => (
            &[1.0, 2.71, 5.15, 8.5],
            &[1.0, 0.4, 0.2, 0.08],
            &[1.0, 0.6, 0.4, 0.3],
            3.5,
            false,
        ),
        12 => (
            &[1.0, 3.0, 5.6, 9.2],
            &[1.0, 0.25, 0.08, 0.04],
            &[1.0, 0.5, 0.3, 0.2],
            2.5,
            true,
        ),
        13 => (
            &[1.0, 5.4, 13.0],
            &[1.0, 0.2, 0.06],
            &[1.0, 0.3, 0.15],
            2.0,
            false,
        ),
        14 => (
            &[0.5, 1.0, 1.19, 1.5, 2.0, 2.52, 3.0],
            &[0.5, 1.0, 0.5, 0.6, 0.6, 0.3, 0.2],
            &[1.3, 1.0, 0.8, 0.7, 0.6, 0.4, 0.3],
            7.0,
            false,
        ),
        15 => (
            &[1.0, 5.9, 15.0],
            &[1.0, 0.18, 0.05],
            &[1.0, 0.25, 0.1],
            1.6,
            false,
        ),
        16 => (
            &[1.0, 2.0, 3.0, 4.03, 5.1],
            &[1.0, 0.7, 0.35, 0.2, 0.1],
            &[1.0, 0.8, 0.6, 0.4, 0.3],
            1.2,
            false,
        ),
        _ => (
            &[1.0, 2.0, 2.4, 3.0, 4.2, 5.4, 6.8],
            &[1.0, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1],
            &[1.0, 0.7, 0.6, 0.5, 0.35, 0.3, 0.2],
            5.0,
            false,
        ),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PluckShared {
    rate: f32,
    model: u8,
    bright: f32,
    decay: f32,
    release: f32,
    vel_sens: f32,
    width: f32,
}

pub(crate) struct PluckVoice {
    on: bool,
    note: u8,
    s: [WaveString; 2],
    nstr: usize,
    modes: [Mode; 8],
    mhz: [f32; 8],
    mt: [f32; 8],
    nm: usize,
    hz: f32,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    rng: Rng,
    click: f32,
    click_f: Svf,
    damp: f32,
    damp_coef: f32,
    released: bool,
    fade: Fade,
    killing: bool,
    level: f32,
    silent: u32,
    bend: f32,
    lp: OnePole,
    mallet: bool,
}

impl PluckVoice {
    fn new(seed: u32, rate: f32) -> PluckVoice {
        PluckVoice {
            on: false,
            note: 60,
            s: [WaveString::new(30.0, rate), WaveString::new(30.0, rate)],
            nstr: 1,
            modes: [Mode::default(); 8],
            mhz: [0.0; 8],
            mt: [1.0; 8],
            nm: 0,
            hz: 440.0,
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            rng: Rng::new(seed),
            click: 0.0,
            click_f: Svf::default(),
            damp: 1.0,
            damp_coef: 1.0,
            released: false,
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            silent: 0,
            bend: 1.0,
            lp: OnePole::default(),
            mallet: false,
        }
    }
}

impl Voice<PluckShared> for PluckVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &PluckShared, _ctl: &Ctl) {
        let rate = sh.rate;
        let v = velocity.max(1) as f32 / 127.0;
        let vv = 1.0 - sh.vel_sens + sh.vel_sens * v;
        self.note = note;
        self.hz = lg_dsp::midi_hz(note as f32);
        let key = (note as f32 - 60.0) / 12.0;
        self.mallet = sh.model >= 8;
        self.released = false;
        self.killing = false;
        self.damp = 1.0;
        self.damp_coef = 1.0;
        self.silent = 0;
        self.bend = 1.0;
        let p = ((note as f32 - 64.0) / 40.0).clamp(-1.0, 1.0) * sh.width;
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        if self.mallet {
            let (ratios, amps, decs, t, _) = mallet_modes(sh.model);
            let hard = (0.3 + 0.7 * v) * (0.5 + sh.bright);
            self.nm = 0;
            for (k, &ra) in ratios.iter().enumerate() {
                let f = self.hz * ra;
                if f > rate * 0.45 || self.nm >= 8 {
                    break;
                }
                // harder mallets excite the upper modes more
                let a = amps[k] * if k == 0 { 1.0 } else { hard.min(1.5) };
                let t60 = (t * decs[k] * sh.decay * 2f32.powf(-key * 0.5)).max(0.05);
                self.modes[self.nm].tune(f, t60, rate);
                self.modes[self.nm].strike(f, a, rate);
                self.mhz[self.nm] = f;
                self.mt[self.nm] = t60;
                self.nm += 1;
            }
            self.click = 0.15 * hard;
            self.click_f = Svf::new(SvfMode::Band, (self.hz * 6.0).min(rate * 0.4), 1.0, rate);
            // levels pass trims per model
            let trim = match sh.model {
                8 | 9 => 2.7,
                10 | 11 | 12 | 13 => 2.4,
                14 => 2.1,
                _ => 2.2,
            };
            self.gain = 0.11 * trim * vv.powf(1.2);
            self.nstr = 0;
        } else {
            // (T60 s at C4, brightness, pluck shape, pluck position, strings, detune cents, gain)
            let (t, br, shape, pos, nstr, det, g) = match sh.model {
                1 => (3.5, 0.45, 0.25, 0.18, 1, 0.0, 0.5),
                2 => (4.0, 0.65, 0.5, 0.2, 1, 0.0, 0.45),
                3 => (0.25, 0.4, 0.4, 0.15, 1, 0.0, 0.5),
                4 => (6.0, 0.55, 0.3, 0.3, 1, 0.0, 0.5),
                5 => (4.0, 0.92, 0.95, 0.08, 2, 3.0, 0.3),
                6 => (3.0, 0.75, 0.6, 0.12, 1, 0.0, 0.45),
                7 => (0.6, 0.5, 0.3, 0.3, 2, 6.0, 0.4),
                _ => (4.5, 0.7, 0.6, 0.12, 2, 1.5, 0.35),
            };
            self.nstr = nstr;
            for k in 0..nstr {
                let d = if k == 0 { 1.0 } else { dsp::semis(det / 100.0) };
                self.s[k].clear();
                self.s[k].tune(
                    self.hz * d,
                    t * sh.decay * 2f32.powf(-key * 0.5),
                    (br * (0.75 + 0.35 * v) * (0.6 + 0.8 * sh.bright)).min(0.985),
                );
                self.s[k].pluck(
                    1.0,
                    (shape * (0.5 + 0.6 * v)).min(1.0),
                    pos * (0.9 + 0.2 * self.rng.unit()),
                    &mut self.rng,
                );
            }
            self.nm = 0;
            self.click = if sh.model == 5 { 0.06 * v } else { 0.02 * v };
            self.click_f = Svf::new(SvfMode::Band, 3000.0, 1.0, rate);
            let trim = match sh.model {
                0 => 2.5,
                1 => 2.0,
                4 => 1.5,
                5 => 3.2,
                _ => 2.2,
            };
            self.gain = g * 0.3 * trim * vv.powf(1.3);
        }
        self.lp
            .lowpass_hz((2000.0 + 14000.0 * sh.bright * v).min(rate * 0.45), rate);
        self.lp.z = 0.0;
        self.on = true;
        self.level = 1.0;
    }
    fn release(&mut self, sh: &PluckShared, _ctl: &Ctl) {
        if self.released {
            return;
        }
        self.released = true;
        let dampers = if self.mallet {
            mallet_modes(sh.model).4
        } else {
            !matches!(sh.model, 4 | 6)
        };
        if dampers {
            self.damp_coef = dsp::t60_coef(sh.release, sh.rate);
        } else if self.mallet {
            // undamped bars ring on (vibraphone: pedal up damps, handled by dampers flag)
        }
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.005, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &PluckShared, ctl: &Ctl) {
        if (ctl.bend_ratio - self.bend).abs() > 1e-5 {
            self.bend = ctl.bend_ratio;
            for k in 0..self.nstr {
                let d = if k == 0 { 1.0 } else { dsp::semis(0.03) };
                self.s[k].retune(self.hz * self.bend * d);
            }
            for k in 0..self.nm {
                self.modes[k].tune(self.mhz[k] * self.bend, self.mt[k], sh.rate);
            }
        }
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let mut x = 0.0;
            for k in 0..self.nstr {
                self.s[k].tick(0.0);
                x += self.s[k].pickup(0.5);
            }
            for m in &mut self.modes[..self.nm] {
                x += m.next();
            }
            if self.click > 1e-6 {
                x += self.click_f.run(self.rng.bipolar()) * self.click;
                self.click *= 0.99;
            }
            x = self.lp.lp(x);
            self.damp *= self.damp_coef;
            let mut g = self.gain * self.damp;
            if self.killing {
                g *= self.fade.next();
            }
            let y = x * g;
            peak = peak.max(y.abs());
            l[i] += y * self.pan_l;
            r[i] += y * self.pan_r;
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

pub struct StudioPlucks {
    params: Params,
    poly: Poly<PluckVoice>,
    ch: Channel,
    sh: PluckShared,
    body: [[Biquad; 3]; 2],
    room: Room,
    trem_phase: f32,
    rate: f32,
}

impl StudioPlucks {
    pub fn new(rate: f32) -> StudioPlucks {
        dsp::warm_tables();
        let mut seed = 700;
        let mut s = StudioPlucks {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 5;
                PluckVoice::new(seed, rate)
            }),
            ch: Channel::new(rate),
            sh: PluckShared {
                rate,
                model: 0,
                bright: 0.5,
                decay: 1.0,
                release: 0.2,
                vel_sens: 0.8,
                width: 0.5,
            },
            body: [[Biquad::default(); 3]; 2],
            room: Room::new(rate, 0.7),
            trem_phase: 0.0,
            rate,
        };
        s.update();
        s
    }
    fn p(&self, i: u32) -> f32 {
        self.params.get(i)
    }
    fn update(&mut self) {
        let rate = self.rate;
        self.sh.model = self.p(0).round() as u8;
        self.sh.bright = self.p(1);
        self.sh.decay = self.p(2);
        self.sh.release = self.p(3);
        self.sh.vel_sens = self.p(5);
        self.sh.width = self.p(11);
        self.poly.set_max(self.p(10) as usize);
        self.poly.retrigger_kills = self.sh.model < 8;
        self.ch.bend_up = self.p(8);
        self.ch.bend_down = self.p(8);
        self.ch.update_bend();
        let body = self.p(4);
        // guitar-like top plate and air resonances; flatter for the others
        let (f, g): ([f32; 3], [f32; 3]) = match self.sh.model {
            0 => ([100.0, 220.0, 2500.0], [6.0, 4.0, 2.0]),
            1 => ([95.0, 200.0, 1800.0], [6.0, 5.0, -2.0]),
            4 => ([150.0, 400.0, 1500.0], [3.0, 2.0, 1.0]),
            5 => ([200.0, 800.0, 3000.0], [2.0, 2.0, 3.0]),
            _ => ([120.0, 600.0, 3000.0], [0.0, 0.0, 0.0]),
        };
        for c in 0..2 {
            for k in 0..3 {
                self.body[c][k].peaking(f[k], 1.4, g[k] * body, rate);
            }
        }
        self.room.feedback = 0.78;
    }
}

impl Plugin for StudioPlucks {
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
        self.room.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        3.0
    }
}

impl Instrument for StudioPlucks {
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
        let room = self.p(6);
        let vibes = self.sh.model == 8;
        let trem = self.p(9);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                let mut xl = sl[i];
                let mut xr = sr[i];
                for k in 0..3 {
                    xl = self.body[0][k].run(xl);
                    xr = self.body[1][k].run(xr);
                }
                if vibes && trem > 0.0 {
                    // the vibraphone's motor-driven discs in the resonator tubes
                    let a = 1.0 - trem * 0.45 * (0.5 + 0.5 * dsp::sin_cycles(self.trem_phase));
                    xl *= a;
                    xr *= a;
                    self.trem_phase = (self.trem_phase + 5.5 / self.rate).fract();
                }
                sl[i] = xl;
                sr[i] = xr;
            }
            self.room.process(&mut sl[..m], &mut sr[..m], room);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}
