//! "Vintage Clav" stand-in: a tangent-struck string (waveguide) read by two electromagnetic
//! pickups (neck/bridge, in or out of phase), the four tone switches, yarn damping on release with
//! the little "thock" of the string leaving the tangent, then wah, phaser, drive and chorus.

use crate::dsp::{self, CHUNK, Fade, Rng, Svf, SvfMode};
use crate::ep::{KeysFx, KeysFxSettings};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::string::WaveString;
use lg_dsp::{Instrument, Param, Plugin, Unit};

const PICKUPS: &[&str] = &[
    "Neck (CA)",
    "Bridge (DB)",
    "Both (AC)",
    "Both, Out of Phase (AD)",
];
const WAH: &[&str] = &["Off", "Mod Wheel", "Envelope", "Auto"];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Pickups", PICKUPS, 2.0),
    Param::new(1, "Brilliant", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Treble", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(3, "Medium", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(4, "Soft", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(5, "Decay", 0.1, 10.0, 5.0, Unit::Seconds),
    Param::new(6, "Release", 0.01, 1.0, 0.06, Unit::Seconds),
    Param::new(7, "Damper", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(8, "Stiffness", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(9, "Velocity Sensitivity", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(10, "Click", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(11, "Wah", WAH, 0.0),
    Param::new(12, "Wah Range", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(13, "Drive", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(14, "Drive Gain", 1.0, 10.0, 2.0, Unit::Ratio),
    Param::new(15, "Phaser", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(16, "Phaser Rate", 0.05, 10.0, 0.6, Unit::Hz),
    Param::new(17, "Phaser Color", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(18, "Chorus", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(19, "Chorus Intensity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(20, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(21, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(22, "Voices", 1.0, 32.0, 32.0, Unit::None),
];

pub(crate) struct ClavShared {
    rate: f32,
    pickup: u8,
    decay: f32,
    release: f32,
    damper: f32,
    stiff: f32,
    vel_sens: f32,
    click: f32,
}

pub(crate) struct ClavVoice {
    s: WaveString,
    on: bool,
    released: bool,
    hz: f32,
    gain: f32,
    fade: Fade,
    killing: bool,
    rng: Rng,
    thock: f32,
    thock_f: Svf,
    level: f32,
    silent: u32,
}

impl ClavVoice {
    fn new(rate: f32, seed: u32) -> ClavVoice {
        ClavVoice {
            s: WaveString::new(25.0, rate),
            on: false,
            released: false,
            hz: 110.0,
            gain: 0.0,
            fade: Fade::default(),
            killing: false,
            rng: Rng::new(seed),
            thock: 0.0,
            thock_f: Svf::default(),
            level: 0.0,
            silent: 0,
        }
    }
}

impl Voice<ClavShared> for ClavVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &ClavShared, _ctl: &Ctl) {
        let v = velocity.max(1) as f32 / 127.0;
        let vv = 1.0 - sh.vel_sens + sh.vel_sens * v;
        // the tangent presses the string: harder hits are a touch sharp
        self.hz = lg_dsp::midi_hz(note as f32 + 0.06 * v * v);
        let key = (note as f32 - 60.0) / 12.0;
        let t60 = (sh.decay * 2f32.powf(-key * 0.6)).clamp(0.2, 15.0) * (1.0 - 0.6 * sh.damper);
        self.s.clear();
        self.s
            .tune(self.hz, t60, (0.55 + 0.35 * v + 0.1 * sh.stiff).min(0.97));
        self.s
            .pluck(1.0, 0.35 + 0.6 * v * sh.stiff.max(0.3), 0.06, &mut self.rng);
        self.gain = 0.55 * vv.powf(1.4);
        self.on = true;
        self.released = false;
        self.killing = false;
        self.thock = 0.0;
        self.silent = 0;
        self.level = 1.0;
    }
    fn release(&mut self, sh: &ClavShared, _ctl: &Ctl) {
        if self.released {
            return;
        }
        self.released = true;
        self.s.set_decay(sh.release);
        self.thock = 0.04 * sh.click * self.level.min(1.0).max(0.2);
        self.thock_f = Svf::new(
            SvfMode::Band,
            (self.hz * 2.0).clamp(200.0, 3000.0),
            2.0,
            sh.rate,
        );
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &ClavShared, ctl: &Ctl) {
        self.s.retune(self.hz * ctl.bend_ratio);
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            self.s.tick(0.0);
            let y = match sh.pickup {
                0 => self.s.pickup(0.18),
                1 => self.s.pickup(0.07),
                2 => 0.6 * (self.s.pickup(0.18) + self.s.pickup(0.07)),
                _ => 1.2 * (self.s.pickup(0.18) - self.s.pickup(0.07)),
            };
            let mut out = y * self.gain;
            if self.thock > 1e-6 {
                out += self.thock_f.run(self.rng.bipolar()) * self.thock;
                self.thock *= 0.996;
            }
            if self.killing {
                out *= self.fade.next();
            }
            peak = peak.max(out.abs());
            l[i] += out;
            r[i] += out;
        }
        self.level = peak / self.gain.max(1e-6);
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

pub struct VintageClav {
    params: Params,
    poly: Poly<ClavVoice>,
    ch: Channel,
    sh: ClavShared,
    tone: [Svf; 4],
    wah: Svf,
    env: f32,
    auto_phase: f32,
    fx: KeysFx,
    fxs: KeysFxSettings,
    rate: f32,
}

impl VintageClav {
    pub fn new(rate: f32) -> VintageClav {
        dsp::warm_tables();
        let mut seed = 11;
        let mut c = VintageClav {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 13;
                ClavVoice::new(rate, seed)
            }),
            ch: Channel::new(rate),
            sh: ClavShared {
                rate,
                pickup: 2,
                decay: 3.0,
                release: 0.06,
                damper: 0.0,
                stiff: 0.5,
                vel_sens: 0.8,
                click: 0.5,
            },
            tone: [Svf::default(); 4],
            wah: Svf::new(SvfMode::Band, 800.0, 3.0, rate),
            env: 0.0,
            auto_phase: 0.0,
            fx: KeysFx::new(rate),
            fxs: KeysFxSettings::default(),
            rate,
        };
        c.update();
        c
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn update(&mut self) {
        let rate = self.rate;
        self.sh.pickup = self.p(0).round() as u8;
        self.sh.decay = self.p(5);
        self.sh.release = self.p(6);
        self.sh.damper = self.p(7);
        self.sh.stiff = self.p(8);
        self.sh.vel_sens = self.p(9);
        self.sh.click = self.p(10);
        self.poly.set_max(self.p(22) as usize);
        self.ch.bend_up = self.p(21);
        self.ch.bend_down = self.p(21);
        self.ch.update_bend();
        // tone switches: brilliant = treble lift, treble = less lows, medium = mid, soft = top cut
        self.tone[0] = Svf::new(
            SvfMode::High,
            if self.p(1) >= 0.5 { 300.0 } else { 60.0 },
            0.6,
            rate,
        );
        self.tone[1] = Svf::new(
            SvfMode::High,
            if self.p(2) >= 0.5 { 600.0 } else { 30.0 },
            0.6,
            rate,
        );
        self.tone[2] = Svf::new(
            SvfMode::Low,
            if self.p(3) >= 0.5 { 5000.0 } else { 9000.0 },
            0.7,
            rate,
        );
        self.tone[3] = Svf::new(
            SvfMode::Low,
            if self.p(4) >= 0.5 { 1800.0 } else { 16000.0 },
            0.7,
            rate,
        );
        self.fxs = KeysFxSettings {
            eq: false,
            drive: self.p(13) >= 0.5,
            drive_gain: self.p(14),
            drive_tone: 8000.0,
            phaser: self.p(15) >= 0.5,
            phaser_rate: self.p(16),
            phaser_color: self.p(17),
            phaser_stereo: 0.5,
            chorus: self.p(18) >= 0.5,
            chorus_rate: 0.8,
            chorus_depth: self.p(19),
            volume: lg_dsp::db_to_gain(self.p(20)),
            ..Default::default()
        };
        self.fx.configure(&self.fxs);
    }
}

impl Plugin for VintageClav {
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
        self.fx.reset();
        self.ch.reset();
        self.wah.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        0.5
    }
}

impl Instrument for VintageClav {
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
        let wah_mode = self.p(11).round() as u8;
        let range = self.p(12);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            // mono path through the tone switches and wah
            let mut e = 0.0f32;
            for i in 0..m {
                let mut x = sl[i];
                for t in &mut self.tone {
                    x = t.run(x);
                }
                e = e.max(x.abs());
                sl[i] = x;
            }
            if wah_mode > 0 {
                self.env = (self.env * 0.97).max(e);
                let ctl = match wah_mode {
                    1 => self.ch.ctl.mod_wheel,
                    2 => (self.env * 6.0).min(1.0),
                    _ => {
                        self.auto_phase = (self.auto_phase + m as f32 * 1.5 / self.rate).fract();
                        0.5 + 0.5 * dsp::sin_cycles(self.auto_phase)
                    }
                };
                let f = 350.0 * (1.0 + 6.0 * range * ctl);
                self.wah.set(f, 4.0, self.rate);
                for x in sl[..m].iter_mut() {
                    *x = self.wah.run_bp_norm(*x) * 2.5;
                }
            }
            sr[..m].copy_from_slice(&sl[..m]);
            self.fx.process(&mut sl[..m], &mut sr[..m], &self.fxs);
            for i in 0..m {
                left[pos + i] += sl[i] * gl;
                right[pos + i] += sr[i] * gr;
            }
            pos += m;
        }
    }
}
