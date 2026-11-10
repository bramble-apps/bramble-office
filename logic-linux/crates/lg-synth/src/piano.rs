//! "Studio Grand" stand-in: a modelled acoustic piano (no samples).
//!
//! Each note is a bank of string partials (modal synthesis with two-pole resonators):
//! - stiff-string inharmonicity (fₙ = n·f₀·√(1+Bn²), B rising up the keyboard) and stretch tuning;
//! - the felt hammer's spectrum (harder and brighter with velocity) times the strike-position comb;
//! - unison strings slightly mistuned, giving the beating and the two-stage decay (prompt sound
//!   then aftersound) of a real piano;
//! - frequency-dependent losses, dampers on release (none in the top octaves);
//! then shared parts: hammer knock and soundboard (a modal body excited by the hammers),
//! sympathetic string resonance (undamped strings ringing along when the sustain pedal is down),
//! key-position stereo and a little room.

use crate::dsp::{self, Biquad, CHUNK, Delay, Fade, Mode, OnePole, Rng, Room};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODELS: &[&str] = &[
    "Concert Grand",
    "Bright Grand",
    "Studio Upright",
    "Felt Piano",
    "Honky-Tonk",
    "Old Upright",
];

pub static PARAMS: &[Param] = &[
    Param::new(0, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(1, "Brightness", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(2, "Hammer Hardness", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(3, "Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(4, "Damper Release", 0.05, 2.0, 0.25, Unit::Seconds),
    Param::new(5, "Sympathetic Resonance", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(6, "Stereo Width", 0.0, 1.0, 0.7, Unit::Ratio),
    Param::new(7, "Ambience", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(8, "Stretch Tuning", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(9, "Velocity Curve", 0.3, 3.0, 1.0, Unit::Ratio),
    Param::new(10, "Pedal Noise", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(11, "Hammer Noise", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(12, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::menu(13, "Model", MODELS, 0.0),
    Param::new(14, "Polyphony", 1.0, 64.0, 48.0, Unit::None),
    Param::new(15, "Unison Detune", 0.0, 1.0, 0.25, Unit::Ratio),
];

const MAXP: usize = 48;
const UNISON_PARTIALS: usize = 14;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PianoShared {
    rate: f32,
    bright: f32,
    hard: f32,
    decay: f32,
    release: f32,
    width: f32,
    stretch: f32,
    vel_curve: f32,
    tune: f32,
    detune: f32,
    model: u8,
    hammer_noise: f32,
}

pub(crate) struct PianoVoice {
    on: bool,
    note: u8,
    modes: [Mode; MAXP + UNISON_PARTIALS],
    hz: [f32; MAXP + UNISON_PARTIALS],
    t60: [f32; MAXP + UNISON_PARTIALS],
    n: usize,
    bend: f32,
    damp: f32,
    damp_coef: f32,
    damped: bool,
    fade: Fade,
    killing: bool,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    level: f32,
    rng: Rng,
    /// Hammer impulse to send to the shared soundboard on the next render.
    pub knock: f32,
    knock_lp: OnePole,
    silent: u32,
}

impl PianoVoice {
    fn new(seed: u32) -> PianoVoice {
        PianoVoice {
            on: false,
            note: 60,
            modes: [Mode::default(); MAXP + UNISON_PARTIALS],
            hz: [0.0; MAXP + UNISON_PARTIALS],
            t60: [1.0; MAXP + UNISON_PARTIALS],
            n: 0,
            bend: 1.0,
            damp: 1.0,
            damp_coef: 1.0,
            damped: false,
            fade: Fade::default(),
            killing: false,
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            level: 0.0,
            rng: Rng::new(seed),
            knock: 0.0,
            knock_lp: OnePole::default(),
            silent: 0,
        }
    }
}

/// Model character: (decay ×, inharmonicity ×, brightness, unison spread ×, strike position).
fn model_char(m: u8) -> (f32, f32, f32, f32, f32) {
    match m {
        1 => (1.0, 1.0, 0.35, 1.0, 1.0 / 8.5),
        2 => (0.7, 1.6, 0.1, 1.3, 1.0 / 7.5),
        3 => (0.8, 1.0, -0.6, 1.0, 1.0 / 8.0),
        4 => (0.8, 1.4, 0.2, 8.0, 1.0 / 7.0),
        5 => (0.55, 2.0, -0.15, 3.0, 1.0 / 7.0),
        _ => (1.0, 1.0, 0.0, 1.0, 1.0 / 8.0),
    }
}

impl Voice<PianoShared> for PianoVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &PianoShared, ctl: &Ctl) {
        let rate = sh.rate;
        let (mdecay, minh, mbright, mdet, strike) = model_char(sh.model);
        let k = note as f32;
        let v = (velocity.max(1) as f32 / 127.0).powf(sh.vel_curve);
        let rel = (k - 60.0) / 12.0;
        // stretch tuning (cents), larger at the extremes
        let st = rel.signum() * (rel.abs() / 3.0).powi(2) * 25.0 * sh.stretch;
        let f0 = lg_dsp::midi_hz(k + (st + sh.tune) / 100.0);
        // inharmonicity coefficient: wound bass strings low, rising toward the treble
        let b = (2.2e-4 * 2f32.powf((k - 60.0) / 13.0)).max(1.2e-4 * 2f32.powf((40.0 - k) / 18.0))
            * minh;
        // fundamental decay (aftersound), seconds
        let t1 = (24.0 * 2f32.powf(-(k - 48.0) / 17.0)).clamp(1.2, 45.0) * sh.decay * mdecay;
        // hammer: softer = a lower corner; the felt gets harder at high velocity
        let hard = (sh.hard + 0.6 * v + 0.4 * (sh.bright + mbright) + 0.5 * (ctl.brightness - 0.5))
            .clamp(0.05, 2.5);
        let fc = (350.0 + 2600.0 * hard * hard) * (1.0 + (k - 60.0).max(0.0) / 30.0);
        let ceiling = (rate * 0.45).min(14_000.0);
        let mut idx = 0;
        let mut total = 0.0;
        for n in 1..=MAXP {
            let nf = n as f32;
            let fh = nf * f0 * (1.0 + b * nf * nf).sqrt();
            if fh > ceiling {
                break;
            }
            // strike position comb and hammer low-pass (2nd order roll-off past the corner)
            let pos = (std::f32::consts::PI * nf * strike).sin().abs().max(0.03);
            let ham = 1.0 / (1.0 + (fh / fc).powi(2));
            let amp = pos * ham / nf.powf(0.7);
            if amp < 1e-4 && n > 4 {
                continue;
            }
            // losses rise with frequency
            let tn = t1 / (1.0 + fh / 1200.0 + (fh / 4000.0).powi(2));
            if n <= UNISON_PARTIALS && idx + 1 < self.modes.len() {
                let d = (0.15 + sh.detune * 1.6) * mdet * (0.6 + 0.4 * self.rng.unit());
                let r = dsp::semis(d / 100.0);
                // prompt sound: strings in phase, fast decay; aftersound: slow, slightly detuned
                for (j, (fr, a, t)) in [
                    (fh / r.sqrt(), amp * 0.6, tn * 0.3),
                    (fh * r.sqrt(), amp * 0.38, tn),
                ]
                .into_iter()
                .enumerate()
                {
                    let i = idx + j;
                    self.hz[i] = fr;
                    self.t60[i] = t;
                    self.modes[i].tune(fr, t, rate);
                    self.modes[i].strike(fr, a, rate);
                }
                idx += 2;
            } else if idx < self.modes.len() {
                self.hz[idx] = fh;
                self.t60[idx] = tn * 0.5;
                self.modes[idx].tune(fh, tn * 0.5, rate);
                self.modes[idx].strike(fh, amp, rate);
                idx += 1;
            }
            total += amp;
        }
        self.n = idx;
        self.note = note;
        self.on = true;
        self.killing = false;
        self.damped = false;
        self.damp = 1.0;
        self.damp_coef = 1.0;
        self.bend = 1.0;
        self.silent = 0;
        // loudness: velocity and a gentle key curve (treble strings are quieter)
        self.gain =
            0.19 * v.powf(1.1) * (1.0 - 0.012 * (k - 60.0).max(0.0)) / total.max(0.2).sqrt();
        let p = ((k - 64.0) / 44.0).clamp(-1.0, 1.0) * sh.width;
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        self.knock = sh.hammer_noise * v * v * 0.6;
        self.knock_lp.lowpass_hz(500.0 + 5000.0 * v, rate);
        self.level = 1.0;
    }
    fn release(&mut self, sh: &PianoShared, _ctl: &Ctl) {
        // the top keys have no dampers
        if self.note >= 89 || self.damped {
            return;
        }
        self.damped = true;
        let t = sh.release * (1.0 + (60.0 - self.note as f32).max(0.0) / 30.0);
        self.damp_coef = dsp::t60_coef(t, sh.rate);
    }
    fn pedal(&mut self, _down: bool, _sh: &PianoShared, _ctl: &Ctl) {}
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.005, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &PianoShared, ctl: &Ctl) {
        if (ctl.bend_ratio - self.bend).abs() > 1e-5 {
            self.bend = ctl.bend_ratio;
            for i in 0..self.n {
                self.modes[i].tune(self.hz[i] * self.bend, self.t60[i], sh.rate);
            }
        }
        let mut peak = 0.0f32;
        let n = self.n;
        for i in 0..l.len() {
            let mut x = 0.0;
            for m in &mut self.modes[..n] {
                x += m.next();
            }
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
        if peak < 2e-5 {
            self.silent += l.len() as u32;
        } else {
            self.silent = 0;
        }
        if (self.killing && self.fade.gain <= 0.0) || self.silent > (sh.rate * 0.03) as u32 {
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

/// Soundboard: a modal body the hammer knocks ring.
struct Soundboard {
    modes: [Mode; 10],
    gains: [f32; 10],
    rng: Rng,
    exc: f32,
    exc_lp: OnePole,
}

impl Soundboard {
    fn new(rate: f32) -> Soundboard {
        let freqs = [
            92.0, 135.0, 188.0, 260.0, 345.0, 470.0, 640.0, 910.0, 1350.0, 2100.0,
        ];
        let t = [0.25, 0.22, 0.2, 0.16, 0.13, 0.1, 0.08, 0.06, 0.05, 0.04];
        let mut modes = [Mode::default(); 10];
        let mut gains = [0.0; 10];
        for i in 0..10 {
            modes[i].tune(freqs[i], t[i], rate);
            gains[i] = Mode::norm_gain(freqs[i], t[i], rate);
        }
        let mut lp = OnePole::default();
        lp.lowpass_hz(3000.0, rate);
        Soundboard {
            modes,
            gains,
            rng: Rng::new(5),
            exc: 0.0,
            exc_lp: lp,
        }
    }
    #[inline]
    fn next(&mut self) -> f32 {
        let e = if self.exc > 1e-6 {
            let v = self.exc_lp.lp(self.rng.bipolar()) * self.exc;
            self.exc *= 0.995;
            v
        } else {
            0.0
        };
        let mut y = 0.0;
        for (i, m) in self.modes.iter_mut().enumerate() {
            y += m.next_in(e * self.gains[i] * if i < 4 { 1.0 } else { 0.6 });
        }
        y
    }
}

/// Undamped strings ringing in sympathy: feedback combs at the lowest 36 string pitches.
struct Resonance {
    lines: Vec<(Delay, f32, f32)>,
    lp: [f32; 36],
}

impl Resonance {
    fn new(rate: f32) -> Resonance {
        let lines = (0..36)
            .map(|i| {
                let hz = lg_dsp::midi_hz(28.0 + i as f32);
                let d = rate / hz;
                (Delay::new(d as usize + 4), d - 1.0, 0.0)
            })
            .collect();
        Resonance {
            lines,
            lp: [0.0; 36],
        }
    }
    fn clear(&mut self) {
        for l in &mut self.lines {
            l.0.clear();
        }
        self.lp = [0.0; 36];
    }
}

pub struct StudioPiano {
    params: Params,
    poly: Poly<PianoVoice>,
    ch: Channel,
    sh: PianoShared,
    rate: f32,
    board: Soundboard,
    res: Resonance,
    res_gain: f32,
    res_fb: f32,
    room: Room,
    body: [Biquad; 2],
    eq: [[Biquad; 2]; 2],
    pedal_noise: f32,
    pedal_f: Biquad,
    rng: Rng,
}

impl StudioPiano {
    pub fn new(rate: f32) -> StudioPiano {
        dsp::warm_tables();
        let mut seed = 21;
        let mut p = StudioPiano {
            params: Params::new(PARAMS),
            poly: Poly::new(48, || {
                seed += 31;
                PianoVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: PianoShared {
                rate,
                bright: 0.0,
                hard: 0.5,
                decay: 1.0,
                release: 0.25,
                width: 0.7,
                stretch: 0.6,
                vel_curve: 1.0,
                tune: 0.0,
                detune: 0.25,
                model: 0,
                hammer_noise: 0.4,
            },
            rate,
            board: Soundboard::new(rate),
            res: Resonance::new(rate),
            res_gain: 0.0,
            res_fb: 0.0,
            room: Room::new(rate, 0.6),
            body: [Biquad::default(); 2],
            eq: [[Biquad::default(); 2]; 2],
            pedal_noise: 0.0,
            pedal_f: Biquad::default(),
            rng: Rng::new(8),
        };
        p.pedal_f.bandpass(400.0, 0.8, rate);
        p.room.feedback = 0.78;
        p.room.damp = 0.4;
        p.update();
        p
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn update(&mut self) {
        let rate = self.rate;
        self.sh.bright = self.p(1);
        self.sh.hard = self.p(2);
        self.sh.decay = self.p(3);
        self.sh.release = self.p(4);
        self.sh.width = self.p(6);
        self.sh.stretch = self.p(8);
        self.sh.vel_curve = self.p(9);
        self.sh.tune = self.p(12);
        self.sh.model = self.p(13).round() as u8;
        self.sh.detune = self.p(15);
        self.sh.hammer_noise = self.p(11);
        self.poly.set_max(self.p(14) as usize);
        self.poly.retrigger_kills = false;
        let up = matches!(self.sh.model, 2 | 5);
        for c in 0..2 {
            self.body[c].peaking(
                if up { 420.0 } else { 220.0 },
                0.9,
                if up { 3.0 } else { 2.0 },
                rate,
            );
            self.eq[c][0].high_shelf(4000.0, self.p(1) * 4.0, rate);
            self.eq[c][1].low_shelf(120.0, 1.5, rate);
        }
    }
}

impl Plugin for StudioPiano {
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
        self.res.clear();
        self.room.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        3.0
    }
}

impl Instrument for StudioPiano {
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
            self.pedal_noise += self.p(10) * if d { 0.02 } else { 0.012 };
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.p(0));
        let symp = self.p(5);
        // sustain pedal lifts all dampers: the open strings ring along
        let target_fb = if self.ch.ctl.sustain { 0.9985 } else { 0.0 };
        let target_g = if self.ch.ctl.sustain {
            symp * 0.05
        } else {
            symp * 0.01
        };
        let amb = self.p(7);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.res_fb += (target_fb - self.res_fb) * 0.05;
            self.res_gain += (target_g - self.res_gain) * 0.05;
            // collect hammer knocks for the soundboard
            for v in &mut self.poly.voices {
                if v.knock > 0.0 {
                    self.board.exc += v.knock;
                    v.knock = 0.0;
                }
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                let mono = (sl[i] + sr[i]) * 0.5;
                let b = self.board.next() * 0.5;
                // sympathetic strings
                let mut s = 0.0;
                if self.res_gain > 1e-5 {
                    for (k, line) in self.res.lines.iter_mut().enumerate() {
                        let y = line.0.read(line.1);
                        self.res.lp[k] = dsp::flush(self.res.lp[k] + (y - self.res.lp[k]) * 0.5);
                        line.0.push(mono * 0.02 + self.res.lp[k] * self.res_fb);
                        s += y;
                    }
                    s *= self.res_gain;
                }
                let mut pn = 0.0;
                if self.pedal_noise > 1e-6 {
                    pn = self.pedal_f.run(self.rng.bipolar()) * self.pedal_noise;
                    self.pedal_noise *= 0.9993;
                }
                let add = b + s + pn;
                let mut xl = sl[i] + add;
                let mut xr = sr[i] + add;
                for (c, x) in [&mut xl, &mut xr].into_iter().enumerate() {
                    let a = self.body[c].run(*x);
                    let b2 = self.eq[c][0].run(a);
                    *x = self.eq[c][1].run(b2);
                }
                sl[i] = xl;
                sr[i] = xr;
            }
            self.room.process(&mut sl[..m], &mut sr[..m], amb * 0.9);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
        let _ = rate;
    }
}
