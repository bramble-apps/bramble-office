//! "Pickup Clav": our stand-in for Logic's Vintage Clav. A tangent-struck string (waveguide,
//! with stiffness) read by two electromagnetic pickups that can sit anywhere along the strings
//! at any angle, wired alone, together or out of phase; fourteen string models; the four tone
//! switches, Brilliance and Decay; yarn damping on release with the little "thock" of the string
//! leaving the tangent; then the effects in any order: distortion, modulation (phaser, flanger,
//! chorus), wah (eight types, pedal and envelope follower) and a compressor.
//!
//! Parameter ids 0..22 are the first version's; the rest follow.

use crate::dsp::{self, Biquad, CHUNK, Chorus, Fade, Flanger, OnePole, Phaser, Rng, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::string::WaveString;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const WIRING: &[&str] = &["Lower (AC)", "Upper (BC)", "Lower − Upper (AD)", "Lower + Upper (BD)"];
pub const MODELS: &[&str] = &["Bell", "Classic 1", "Classic 2", "Dulcimer", "Funk", "Harpsi 1", "Harpsi 2", "Mellow", "Plectrum 1", "Plectrum 2", "Sitar", "Worn 1", "Worn 2", "Wood"];
pub const MOD_MODES: &[&str] = &["Phaser", "Flanger", "Chorus"];
pub const WAH_MODES: &[&str] = &["Classic Wah", "Retro Wah", "Modern Wah", "Opto Wah 1", "Opto Wah 2", "Resonant LP", "Resonant HP", "Peak"];
pub const ORDERS: &[&str] = &["Dist-Mod-Wah", "Dist-Wah-Mod", "Mod-Dist-Wah", "Mod-Wah-Dist", "Wah-Dist-Mod", "Wah-Mod-Dist"];
pub const CURVES: &[&str] = &["fx25%", "fx50%", "fx75%", "fx100%", "convx1", "convx2", "linear", "concv1", "concv2"];
/// A control source: 0 off, 1..=119 the controller, 120 aftertouch, 121 Learn, 122 velocity.
pub const SRC_TOUCH: f32 = 120.0;
pub const SRC_LEARN: f32 = 121.0;
pub const SRC_VELOCITY: f32 = 122.0;

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Pickup Wiring", WIRING, 3.0),
    Param::new(1, "Brilliant", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Treble", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(3, "Medium", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(4, "Soft", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(5, "Decay", 0.1, 10.0, 5.0, Unit::Seconds),
    Param::new(6, "String Release", 0.01, 1.0, 0.06, Unit::Seconds),
    Param::new(7, "Damper", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(8, "Stiffness", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(9, "Velocity Sensitivity", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(10, "Click Intensity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(11, "Wah", 0.0, 3.0, 0.0, Unit::OnOff),
    Param::new(12, "Wah Range", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(13, "Distortion", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(14, "Distortion Gain", 1.0, 10.0, 2.0, Unit::Ratio),
    Param::new(15, "Modulation", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(16, "Modulation Rate", 0.05, 10.0, 0.6, Unit::Hz),
    Param::new(17, "Modulation Intensity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(18, "Chorus", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
    Param::new(19, "Chorus Intensity", 0.0, 1.0, 0.5, Unit::Ratio).hide(),
    Param::new(20, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(21, "Pitch Wheel", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(22, "Voices", 1.0, 32.0, 32.0, Unit::None),
    Param::menu(23, "Model", MODELS, 1.0),
    Param::new(24, "Upper Pickup Position", 0.02, 0.48, 0.07, Unit::Ratio),
    Param::new(25, "Upper Pickup Angle", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(26, "Lower Pickup Position", 0.02, 0.48, 0.18, Unit::Ratio),
    Param::new(27, "Lower Pickup Angle", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(28, "Stereo Spread Key", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(29, "Stereo Spread Pickup", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(30, "Modulation Mode", MOD_MODES, 0.0),
    Param::new(31, "Brilliance", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(32, "Modulation Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(33, "Effects", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(34, "Compressor", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(35, "Compressor Ratio", 1.0, 20.0, 4.0, Unit::None),
    Param::menu(36, "Effect Order", ORDERS, 0.0),
    Param::new(37, "Compressor Position", 0.0, 3.0, 0.0, Unit::None),
    Param::new(38, "Distortion Tone", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::menu(39, "Wah Mode", WAH_MODES, 0.0),
    Param::new(40, "Wah Envelope", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(41, "Excite Shape", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(42, "Click Random", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(43, "Click Velocity", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::menu(44, "Velocity Mode", &["KeyOn", "KeyOff", "Auto"], 2.0),
    Param::new(45, "Damping", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(46, "Inharmonicity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(47, "Tension Mod", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(48, "Pitch Fall", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(49, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
    Param::new(50, "Stretch Tuning", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(51, "Warmth", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(52, "Pitch Pressure", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(53, "Voice Mode", &["Poly", "Mono", "Legato"], 0.0),
    Param::menu(54, "Velocity Curve", CURVES, 6.0),
    Param::new(55, "Damper Control", 0.0, 122.0, 0.0, Unit::None),
    Param::new(56, "Wah Control", 0.0, 122.0, 1.0, Unit::None),
    Param::new(57, "Wah Pedal Position", 0.0, 1.0, 0.0, Unit::Ratio),
    // extended parameters (the disclosure arrow at the lower left)
    Param::menu(MONO_MODE, "MIDI Mono Mode", MONO_MODES, 0.0),
    Param::new(MONO_BEND, "Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones),
];

/// What makes a model: its strings and how they are excited.
#[derive(Clone, Copy, Debug)]
pub struct Model {
    /// How slowly the upper partials die (0..1).
    bright: f32,
    decay: f32,
    /// Excitation: sharpness and position along the string.
    shape: f32,
    pos: f32,
    stiff: f32,
    /// A body resonance (Hz, dB).
    body: (f32, f32),
    low_cut: f32,
    /// Timbre jumps between neighbouring keys (0 = even).
    jumps: f32,
    /// A buzzing bridge (sitar).
    buzz: f32,
    level: f32,
    /// Low notes swell, then collapse after 20–30 s.
    swells: bool,
}

const fn model(bright: f32, decay: f32, shape: f32, pos: f32, stiff: f32, body: (f32, f32), low_cut: f32, jumps: f32, buzz: f32, level: f32) -> Model {
    Model { bright, decay, shape, pos, stiff, body, low_cut, jumps, buzz, level, swells: false }
}

pub const MODEL_TABLE: [Model; 14] = [
    model(0.96, 1.3, 0.75, 0.09, 1.0, (2400.0, 5.0), 60.0, 0.15, 0.0, 0.9),
    model(0.72, 1.0, 0.55, 0.06, 0.25, (900.0, 2.0), 40.0, 0.35, 0.0, 1.0),
    model(0.84, 0.9, 0.75, 0.05, 0.3, (1500.0, 4.0), 60.0, 0.35, 0.0, 1.15),
    model(0.8, 1.6, 0.35, 0.2, 0.15, (700.0, 3.0), 80.0, 0.1, 0.0, 0.95),
    Model { swells: true, ..model(0.62, 1.2, 0.5, 0.08, 0.2, (120.0, 7.0), 20.0, 0.3, 0.0, 1.1) },
    model(0.9, 0.8, 1.0, 0.1, 0.1, (1800.0, 3.0), 90.0, 0.2, 0.0, 0.9),
    model(0.95, 0.9, 1.0, 0.07, 0.1, (2800.0, 4.0), 120.0, 0.2, 0.0, 0.9),
    model(0.4, 1.1, 0.25, 0.12, 0.1, (500.0, 2.0), 30.0, 0.0, 0.0, 1.1),
    model(0.78, 1.0, 0.7, 0.25, 0.15, (1100.0, 2.5), 60.0, 0.2, 0.0, 1.0),
    model(0.88, 0.9, 0.9, 0.12, 0.15, (2000.0, 3.0), 80.0, 0.2, 0.0, 1.0),
    model(0.93, 1.8, 0.8, 0.08, 0.2, (1300.0, 6.0), 70.0, 0.15, 0.7, 0.85),
    model(0.6, 0.85, 0.45, 0.06, 0.3, (800.0, 2.0), 40.0, 0.6, 0.0, 0.95),
    model(0.5, 0.7, 0.4, 0.06, 0.35, (650.0, 3.0), 40.0, 0.85, 0.0, 0.95),
    model(0.7, 0.45, 0.6, 0.15, 0.9, (1700.0, 5.0), 220.0, 0.3, 0.0, 1.0),
];

/// A key's own pseudo-random number in -1..1 (the same every time: worn hammers, detuning).
fn key_noise(note: u8, salt: u32) -> f32 {
    let mut x = (note as u32).wrapping_mul(2654435761).wrapping_add(salt.wrapping_mul(40503));
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    (x & 0xffff) as f32 / 32768.0 - 1.0
}

/// The nine velocity curves: four fixed, two convex, linear, two concave.
pub fn velocity_curve(curve: usize, v: f32) -> f32 {
    match curve {
        0 => 0.25,
        1 => 0.5,
        2 => 0.75,
        3 => 1.0,
        4 => v.powf(0.7),
        5 => v.powf(0.45),
        7 => v.powf(1.5),
        8 => v.powf(2.2),
        _ => v,
    }
}

/// Extended parameters: MIDI Mono Mode (MPE: a voice per channel) and its Bend Range.
pub const MONO_MODE: u32 = 58;
pub const MONO_BEND: u32 = 59;
pub const MONO_MODES: &[&str] = &["Off", "On (with common base channel 1)", "On (with common base channel 16)"];

pub(crate) struct ClavShared {
    /// The MIDI channel of the note being started (MIDI Mono Mode).
    chan: u8,
    rate: f32,
    model: Model,
    wiring: u8,
    pick_u: (f32, f32),
    pick_l: (f32, f32),
    spread_key: f32,
    spread_pick: f32,
    decay: f32,
    release: f32,
    damper: f32,
    stiff: f32,
    inharm: f32,
    damping: f32,
    vel_sens: f32,
    curve: usize,
    click: f32,
    click_random: f32,
    click_vel: f32,
    vel_mode: u8,
    excite: f32,
    tension: f32,
    fall: f32,
    tune: f32,
    stretch: f32,
    warmth: f32,
    pressure: f32,
}

pub(crate) struct ClavVoice {
    /// MIDI Mono Mode: the voice's channel and its own pitch bend (semitones).
    chan: u8,
    mpe_bend: f32,
    s: WaveString,
    on: bool,
    released: bool,
    note: u8,
    hz: f32,
    vel: f32,
    gain: f32,
    t60: f32,
    age: u32,
    rel_age: u32,
    fade: Fade,
    killing: bool,
    rng: Rng,
    thock: f32,
    thock_f: Svf,
    buzz_lp: OnePole,
    level: f32,
    silent: u32,
}

impl ClavVoice {
    fn new(rate: f32, seed: u32) -> ClavVoice {
        ClavVoice {
            chan: 0,
            mpe_bend: 0.0,
            s: WaveString::new(8.0, rate),
            on: false,
            released: false,
            note: 60,
            hz: 110.0,
            vel: 1.0,
            gain: 0.0,
            t60: 1.0,
            age: 0,
            rel_age: 0,
            fade: Fade::default(),
            killing: false,
            rng: Rng::new(seed),
            thock: 0.0,
            thock_f: Svf::default(),
            buzz_lp: OnePole::default(),
            level: 0.0,
            silent: 0,
        }
    }

    /// The key's tuning: Tune, Stretch (bass flatter, treble sharper) and Warmth (a fixed random
    /// deviation per key).
    fn pitch(note: f32, key: u8, sh: &ClavShared) -> f32 {
        let x = (note - 60.0) / 36.0;
        let cents = sh.tune + sh.stretch * 25.0 * x * x.abs() + sh.warmth * 9.0 * key_noise(key, 3);
        lg_dsp::midi_hz(note + cents / 100.0)
    }

    /// Moves a sounding voice to another key without striking again (Legato).
    fn glide_to(&mut self, note: u8, sh: &ClavShared) {
        self.note = note;
        self.hz = Self::pitch(note as f32, note, sh);
    }
}

impl Voice<ClavShared> for ClavVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &ClavShared, _ctl: &Ctl) {
        let m = &sh.model;
        let v = velocity_curve(sh.curve, velocity.max(1) as f32 / 127.0);
        let vv = 1.0 - sh.vel_sens + sh.vel_sens * v;
        self.note = note;
        self.vel = v;
        self.chan = sh.chan;
        self.mpe_bend = 0.0;
        self.hz = Self::pitch(note as f32, note, sh);
        let key = (note as f32 - 60.0) / 12.0;
        // worn or uneven instruments: each key a little different
        let jump = m.jumps * key_noise(note, 1);
        self.t60 = (sh.decay * m.decay * 2f32.powf(-key * 0.6)).clamp(0.15, 20.0) * (1.0 + 0.3 * jump);
        self.s.clear();
        // stiffness stretches the overtones; Inharmonicity sets how far down that starts
        let stiff = (sh.stiff * 0.6 + m.stiff * 0.6).min(1.0);
        self.s.set_dispersion(stiff, 1 + (sh.inharm * 3.0).round() as usize);
        let bright = (m.bright + 0.2 * v - 0.25 * sh.damping + 0.12 * jump).clamp(0.05, 0.98);
        self.s.tune(self.hz, self.t60, bright);
        let shape = (m.shape + 0.35 * sh.excite + 0.25 * (v - 0.5) + 0.2 * jump).clamp(0.02, 1.0);
        self.s.pluck(1.0, shape, (m.pos * (1.0 + 0.4 * jump)).clamp(0.02, 0.45), &mut self.rng);
        self.gain = 0.78 * m.level * vv.powf(1.4);
        self.on = true;
        self.released = false;
        self.killing = false;
        self.thock = 0.0;
        self.silent = 0;
        self.age = 0;
        self.rel_age = 0;
        self.level = 1.0;
        self.buzz_lp.lowpass_hz(700.0, sh.rate);
    }
    fn release(&mut self, sh: &ClavShared, _ctl: &Ctl) {
        if self.released {
            return;
        }
        self.released = true;
        self.rel_age = 0;
        self.s.set_decay(sh.release);
        // the key-off click: its level by the key (Random), the velocity (KeyOn: the strike's;
        // KeyOff: the release's, which arrives as a medium one) and how loud the string still is
        let v = match sh.vel_mode {
            1 => 0.5,
            _ => self.vel,
        };
        let by_key = 1.0 - sh.click_random * (0.5 + 0.5 * key_noise(self.note, 2));
        let by_vel = 1.0 - sh.click_vel + sh.click_vel * v;
        self.thock = 0.08 * sh.click * by_key * by_vel * self.level.clamp(0.2, 1.0);
        self.thock_f = Svf::new(SvfMode::Band, (self.hz * 2.0).clamp(200.0, 3000.0), 2.0, sh.rate);
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &ClavShared, ctl: &Ctl) {
        let n = l.len() as u32;
        let m = &sh.model;
        let t = self.age as f32 / sh.rate;
        self.age = self.age.saturating_add(n);
        let mut hz = self.hz * ctl.bend_ratio * dsp::semis(sh.pressure * 2.0 * ctl.aftertouch + self.mpe_bend);
        // the string is stretched by the strike: a moment sharp; let go, it sags
        hz *= 1.0 + sh.tension * 0.012 * self.vel * (-t / 0.05).exp();
        if self.released {
            let tr = self.rel_age as f32 / sh.rate;
            self.rel_age = self.rel_age.saturating_add(n);
            hz *= 1.0 - sh.fall * 0.04 * (1.0 - (-tr / 0.06).exp());
        } else {
            let mut t60 = self.t60 * (1.0 - 0.9 * sh.damper).max(0.03);
            if m.swells && self.note < 52 {
                // low notes ring up, then give way
                t60 = if t < 24.0 { t60 * (1.0 + t * 1.5) } else { 0.25 };
            }
            self.s.set_decay(t60);
        }
        self.s.retune(hz);
        let key = (self.note as f32 - 60.0) / 60.0;
        let pu = (sh.pick_u.0 + sh.pick_u.1 * key * 0.2).clamp(0.01, 0.49);
        let pl = (sh.pick_l.0 + sh.pick_l.1 * key * 0.2).clamp(0.01, 0.49);
        let (gu, gl): (f32, f32) = match sh.wiring {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (-1.2, 1.2),
            _ => (0.6, 0.6),
        };
        // where each pickup sits in the stereo field: the key's place, the pickups apart
        let both = gu != 0.0 && gl != 0.0;
        let kp = (self.note as f32 - 60.0) / 40.0 * sh.spread_key;
        let pan = |x: f32| {
            let a = (x.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
            (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
        };
        let (ul, ur) = pan(kp - if both { sh.spread_pick } else { 0.0 });
        let (ll, lr) = pan(kp + if both { sh.spread_pick } else { 0.0 });
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            self.s.tick(0.0);
            let mut yu = self.s.pickup(pu) * gu;
            let mut yl = self.s.pickup(pl) * gl;
            if m.buzz > 0.0 {
                // a flat bridge: the string rattles on it, most when it swings widest
                let y = yu + yl;
                let rattle = (y.abs() - self.buzz_lp.lp(y.abs())) * m.buzz * 1.5;
                yu += rattle * gu.abs();
                yl += rattle * gl.abs();
            }
            let mut a = (yu * ul + yl * ll) * self.gain;
            let mut b = (yu * ur + yl * lr) * self.gain;
            if self.thock > 1e-6 {
                let k = self.thock_f.run(self.rng.bipolar()) * self.thock;
                a += k;
                b += k;
                self.thock *= 0.996;
            }
            if self.killing {
                let f = self.fade.next();
                a *= f;
                b *= f;
            }
            peak = peak.max(a.abs()).max(b.abs());
            l[i] += a;
            r[i] += b;
        }
        self.level = peak / self.gain.max(1e-6);
        if peak < 1e-5 {
            self.silent += n;
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

struct Learn {
    id: u32,
    before: f32,
    left: i64,
}

pub struct VintageClav {
    params: Params,
    poly: Poly<ClavVoice>,
    ch: Channel,
    sh: ClavShared,
    tone: [[Svf; 4]; 2],
    body: [Biquad; 2],
    cut: [Svf; 2],
    brill: [Biquad; 2],
    // effects
    wah: [Biquad; 2],
    wah_pos: f32,
    env: f32,
    dist_lp: [OnePole; 2],
    phaser: Phaser,
    flanger: Flanger,
    chorus: Chorus,
    old_chorus: Chorus,
    comp_env: f32,
    // control
    pedal: f32,
    damper_ctl: f32,
    expression: f32,
    last_vel: f32,
    /// Legato: the keys held, oldest first, and the note the sounding voice was struck with.
    held: [u8; 16],
    held_n: usize,
    struck: Option<u8>,
    learn: Option<Learn>,
    tempo: f32,
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
                model: MODEL_TABLE[1],
                wiring: 3,
                pick_u: (0.07, 0.0),
                pick_l: (0.18, 0.0),
                spread_key: 0.0,
                spread_pick: 0.0,
                decay: 3.0,
                release: 0.06,
                damper: 0.0,
                stiff: 0.5,
                inharm: 0.5,
                damping: 0.0,
                vel_sens: 0.8,
                curve: 6,
                click: 0.5,
                click_random: 0.2,
                click_vel: 0.3,
                vel_mode: 2,
                excite: 0.0,
                tension: 0.3,
                fall: 0.0,
                tune: 0.0,
                stretch: 0.0,
                warmth: 0.0,
                pressure: 0.0,
                chan: 0,
            },
            tone: [[Svf::default(); 4]; 2],
            body: [Biquad::default(); 2],
            cut: [Svf::default(); 2],
            brill: [Biquad::default(); 2],
            wah: [Biquad::default(); 2],
            wah_pos: 0.0,
            env: 0.0,
            dist_lp: [OnePole::default(); 2],
            phaser: Phaser::default(),
            flanger: Flanger::new(rate),
            chorus: Chorus::new(rate),
            old_chorus: Chorus::new(rate),
            comp_env: 0.0,
            pedal: 0.0,
            damper_ctl: 0.0,
            expression: 1.0,
            last_vel: 0.0,
            held: [0; 16],
            held_n: 0,
            struck: None,
            learn: None,
            tempo: 120.0,
            rate,
        };
        c.update();
        c
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn on(&self, id: u32) -> bool {
        self.params.get(id) >= 0.5
    }
    fn update(&mut self) {
        let rate = self.rate;
        let model = MODEL_TABLE[(self.p(23).round() as usize).min(MODEL_TABLE.len() - 1)];
        self.sh.model = model;
        self.sh.wiring = self.p(0).round() as u8;
        self.sh.pick_u = (self.p(24), self.p(25));
        self.sh.pick_l = (self.p(26), self.p(27));
        self.sh.spread_key = self.p(28);
        self.sh.spread_pick = self.p(29);
        self.sh.decay = self.p(5);
        self.sh.release = self.p(6);
        self.sh.damper = self.p(7).max(self.damper_ctl);
        self.sh.stiff = self.p(8);
        self.sh.inharm = self.p(46);
        self.sh.damping = self.p(45);
        self.sh.vel_sens = self.p(9);
        self.sh.curve = self.p(54).round() as usize;
        self.sh.click = self.p(10);
        self.sh.click_random = self.p(42);
        self.sh.click_vel = self.p(43);
        self.sh.vel_mode = self.p(44).round() as u8;
        self.sh.excite = self.p(41);
        self.sh.tension = self.p(47);
        self.sh.fall = self.p(48);
        self.sh.tune = self.p(49);
        self.sh.stretch = self.p(50);
        self.sh.warmth = self.p(51);
        self.sh.pressure = self.p(52);
        let single = self.p(53).round() as i32 > 0;
        self.poly.set_max(if single { 1 } else { self.p(22) as usize });
        self.ch.bend_up = self.p(21);
        self.ch.bend_down = self.p(21);
        self.ch.update_bend();
        // tone switches: brilliant = nasal with a strong bass cut, treble = sharper with a gentle
        // cut, medium = thinner, soft = muted
        let sw = [(SvfMode::High, if self.on(1) { 300.0 } else { 60.0 }, 0.6), (SvfMode::High, if self.on(2) { 600.0 } else { 30.0 }, 0.6), (SvfMode::Low, if self.on(3) { 5000.0 } else { 9000.0 }, 0.7), (SvfMode::Low, if self.on(4) { 1800.0 } else { 16000.0 }, 0.7)];
        for c in 0..2 {
            for (k, (mode, hz, q)) in sw.iter().enumerate() {
                self.tone[c][k] = Svf::new(*mode, *hz, *q, rate);
            }
            self.body[c].peaking(model.body.0, 1.4, model.body.1, rate);
            self.cut[c] = Svf::new(SvfMode::High, model.low_cut, 0.7, rate);
            // Brilliance: sharper to the right, muted to the left
            let b = self.p(31);
            if b >= 0.0 {
                self.brill[c].high_shelf(2500.0, 12.0 * b, rate);
            } else {
                self.brill[c].lowpass(dsp::lerp(16000.0, 700.0, -b), 0.7, rate);
            }
            self.dist_lp[c].lowpass_hz(dsp::lerp(900.0, 12000.0, self.p(38)), rate);
        }
    }

    /// The modulation's rate in Hz as it runs (synced: the nearest bar or beat value).
    pub fn mod_rate(&self) -> f32 {
        if self.on(32) && self.p(30).round() as i32 != 2 { crate::ep::sync_rate(self.p(16), self.tempo).0 } else { self.p(16) }
    }

    /// A value from a control source for the wah pedal and the damper.
    fn source(&mut self, src: f32, v: f32) {
        if src < 1.0 {
            return;
        }
        if self.p(56).round() == src {
            self.pedal = v;
        }
        if self.p(55).round() == src {
            self.damper_ctl = v;
            self.sh.damper = self.p(7).max(v);
        }
    }

    fn uses(&self, src: f32) -> bool {
        self.p(56).round() == src || self.p(55).round() == src
    }
}

impl VintageClav {
    /// The base channel (0-based) in MIDI Mono Mode, None when it's off.
    fn base_channel(&self) -> Option<u8> {
        match self.p(MONO_MODE).round() as i32 {
            1 => Some(0),
            2 => Some(15),
            _ => None,
        }
    }
    /// One note's pitch bend on a MIDI Mono Mode member channel (by Mono Mode Bend Range); the
    /// base channel (or Mono Mode off) bends every voice by the Pitch Wheel range.
    pub fn pitch_bend_ch(&mut self, channel: u8, value: i16) {
        match self.base_channel() {
            Some(b) if b != channel => {
                let st = value as f32 / 8192.0 * self.p(MONO_BEND);
                for v in self.poly.voices.iter_mut().filter(|v| v.on && v.chan == channel) {
                    v.mpe_bend = st;
                }
            }
            _ => self.pitch_bend(value),
        }
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
        let before = self.params.get(id);
        if !self.params.set(id, v) {
            return;
        }
        let now = self.params.get(id);
        match id {
            // (the first version's Wah menu: 2 and 3 were the envelope and automatic wahs)
            11 if now >= 1.5 => {
                self.params.set(11, 1.0);
                if self.p(40) <= 0.0 {
                    self.params.set(40, 0.7);
                }
            }
            55 | 56 if now == SRC_LEARN => self.learn = Some(Learn { id, before: if before == SRC_LEARN { 0.0 } else { before }, left: (self.rate * 20.0) as i64 }),
            57 => self.pedal = now,
            _ => {}
        }
        self.update();
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
            v.s.clear();
        }
        self.phaser.reset();
        self.flanger.reset();
        self.chorus.reset();
        self.old_chorus.reset();
        self.ch.reset();
        self.held_n = 0;
        self.struck = None;
        self.env = 0.0;
        self.comp_env = 0.0;
        self.expression = 1.0;
        self.update();
    }
    fn tail(&self) -> f32 {
        0.5
    }
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        match id {
            55 | 56 => Some(match value.round() as i32 {
                0 => "Off".into(),
                120 => "Aftertouch".into(),
                121 => "Learn".into(),
                122 => "Velocity".into(),
                n => format!("CC {n}"),
            }),
            37 => Some(["First", "Second", "Third", "Last"][(value.round() as usize).min(3)].into()),
            _ => None,
        }
    }
}

impl Instrument for VintageClav {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.last_vel = velocity as f32 / 127.0;
        self.source(SRC_VELOCITY, self.last_vel);
        if self.p(53).round() as i32 == 2 {
            // Legato: playing over a held key only changes the pitch
            let playing = self.held_n > 0 && self.struck.is_some();
            if self.held_n < 16 {
                self.held[self.held_n] = pitch;
                self.held_n += 1;
            }
            if playing {
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.released) {
                    v.glide_to(pitch, &self.sh);
                    return;
                }
            }
            self.struck = Some(pitch);
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_on_ch(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.sh.chan = channel;
        self.note_on(pitch, velocity);
        self.sh.chan = 0;
    }
    fn note_off(&mut self, pitch: u8) {
        if self.p(53).round() as i32 == 2 {
            if let Some(i) = self.held[..self.held_n].iter().position(|n| *n == pitch) {
                self.held.copy_within(i + 1..self.held_n, i);
                self.held_n -= 1;
            }
            if self.held_n > 0 {
                let back = self.held[self.held_n - 1];
                if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.released) {
                    v.glide_to(back, &self.sh);
                }
                return;
            }
            if let Some(n) = self.struck.take() {
                self.poly.note_off(n, &self.sh, &self.ch.ctl);
            }
            return;
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn pressure(&mut self, value: u8) {
        let v = value as f32 / 127.0;
        self.ch.ctl.aftertouch = v;
        self.source(SRC_TOUCH, v);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.held_n = 0;
            self.struck = None;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        let v = value as f32 / 127.0;
        if let Some(l) = self.learn.take() {
            if (1..120).contains(&cc) && cc != 7 && cc != 10 && cc != 64 {
                self.params.set(l.id, cc as f32);
                self.source(cc as f32, v);
                return;
            }
            self.learn = Some(l);
        }
        self.source(cc as f32, v);
        // CC 11 scales the output unless it is the wah's or the damper's controller
        if cc == 11 {
            if !self.uses(11.0) {
                self.expression = v;
            }
            return;
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm as f32;
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        if let Some(l) = self.learn.as_mut() {
            l.left -= n as i64;
            if l.left <= 0 {
                let (id, before) = (l.id, l.before);
                self.learn = None;
                self.params.set(id, before);
            }
        }
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.p(20)) * self.expression;
        let fx = self.on(33);
        let wah_on = fx && self.on(11);
        let wah_mode = self.p(39).round() as usize;
        let (range, env_amt) = (self.p(12), self.p(40));
        let dist_on = fx && self.on(13);
        let gain = self.p(14);
        let mod_on = fx && self.on(15);
        let mod_mode = self.p(30).round() as usize;
        let (mrate, mint) = (self.mod_rate(), self.p(17));
        let comp_on = fx && self.on(34);
        let ratio = self.p(35);
        let comp_at = self.p(37).round() as usize;
        let order: [u8; 3] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]][(self.p(36).round() as usize).min(5)];
        let old_chorus = self.on(18);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly.render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            // the tone switches, the model's body, Brilliance
            let mut e = 0.0f32;
            for (c, buf) in [&mut sl, &mut sr].into_iter().enumerate() {
                for x in buf[..m].iter_mut() {
                    let mut y = *x;
                    for t in &mut self.tone[c] {
                        y = t.run(y);
                    }
                    y = self.brill[c].run(self.body[c].run(self.cut[c].run(y)));
                    e = e.max(y.abs());
                    *x = y;
                }
            }
            self.env = (self.env * 0.97).max(e);
            for stage in 0..4 {
                if comp_on && comp_at == stage {
                    // the compressor: its place in the chain is free
                    for i in 0..m {
                        let lvl = sl[i].abs().max(sr[i].abs());
                        self.comp_env = if lvl > self.comp_env { lvl } else { self.comp_env * 0.9995 };
                        let over = (self.comp_env / 0.12).max(1.0);
                        let g = over.powf(1.0 / ratio - 1.0) * (1.0 + 0.15 * (ratio - 1.0).min(8.0));
                        sl[i] *= g;
                        sr[i] *= g;
                    }
                }
                let Some(kind) = order.get(stage) else { break };
                match kind {
                    0 if dist_on && gain > 1.0 => {
                        for (c, buf) in [&mut sl, &mut sr].into_iter().enumerate() {
                            for x in buf[..m].iter_mut() {
                                let y = dsp::soft_clip(*x * gain * 2.0) / gain.sqrt();
                                *x = self.dist_lp[c].lp(y);
                            }
                        }
                    }
                    1 if mod_on => match mod_mode {
                        // (high Rate and Intensity: the phaser's feedback starts to sing)
                        0 => self.phaser.process(&mut sl[..m], &mut sr[..m], mrate, (mint * (0.8 + 0.05 * mrate)).min(1.35), 0.5, (mint * 2.0).min(1.0), rate),
                        1 => self.flanger.process(&mut sl[..m], &mut sr[..m], mrate, mint, 0.3 + 0.6 * mint),
                        _ => self.chorus.process(&mut sl[..m], &mut sr[..m], mrate, mint, mint.min(1.0)),
                    },
                    2 if wah_on => {
                        // the pedal and the envelope follower together move the filter
                        let slow = matches!(wah_mode, 3 | 4);
                        let target = (self.pedal + env_amt * (self.env * 6.0).min(1.0) * (0.4 + 0.6 * self.last_vel)).clamp(0.0, 1.0);
                        self.wah_pos += (target - self.wah_pos) * if slow { 0.05 } else { 0.4 };
                        let x = self.wah_pos;
                        let (lo, hi, q): (f32, f32, f32) = [(350.0, 2400.0, 4.0), (300.0, 1800.0, 3.0), (450.0, 3200.0, 5.0), (400.0, 2000.0, 3.0), (450.0, 2800.0, 5.0), (250.0, 5000.0, 2.5), (150.0, 3500.0, 2.5), (400.0, 2600.0, 2.0)][wah_mode.min(7)];
                        let hi = lo * (hi / lo).powf(0.25 + 0.75 * range);
                        let hz = lo * (hi / lo).powf(x);
                        // Modern and the Opto wahs keep their Q; the others sharpen toward the heel
                        let q = if matches!(wah_mode, 2..=4) { q } else { q * (1.3 - 0.6 * x) };
                        for f in &mut self.wah {
                            match wah_mode {
                                5 => f.lowpass(hz, q, rate),
                                6 => f.highpass(hz, q, rate),
                                7 => f.peaking(hz, q, 14.0, rate),
                                _ => f.bandpass(hz, q, rate),
                            }
                        }
                        let g = if wah_mode < 5 { 2.5 } else { 1.0 };
                        for i in 0..m {
                            sl[i] = self.wah[0].run(sl[i]) * g;
                            sr[i] = self.wah[1].run(sr[i]) * g;
                        }
                    }
                    _ => {}
                }
            }
            if old_chorus {
                let d = self.p(19);
                self.old_chorus.process(&mut sl[..m], &mut sr[..m], 0.8, d, d.min(1.0));
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn play(c: &mut VintageClav, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            c.render(a, b);
        }
        (l, r)
    }
    fn tone(set: &[(u32, f32)], note: u8) -> (f32, f32) {
        let mut c = VintageClav::new(48000.0);
        for (id, v) in set {
            c.set(*id, *v);
        }
        c.note_on(note, 100);
        let (l, r) = play(&mut c, 0.4);
        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()));
        (rms(&l), rms(&r))
    }

    #[test]
    fn every_model_sounds_and_differs() {
        let mut seen: Vec<f32> = Vec::new();
        for k in 0..MODELS.len() {
            let (l, _) = tone(&[(23, k as f32)], 48);
            assert!(l > 1e-3, "{} is silent", MODELS[k]);
            assert!(seen.iter().all(|x| (x - l).abs() > 1e-6), "{} sounds like another model", MODELS[k]);
            seen.push(l);
        }
    }

    #[test]
    fn pickups_position_and_wiring() {
        let both = tone(&[(0, 3.0)], 48).0;
        let lower = tone(&[(0, 0.0)], 48).0;
        let upper = tone(&[(0, 1.0)], 48).0;
        assert!(both != lower && lower != upper);
        // moved: another tone
        assert!((tone(&[(0, 0.0), (26, 0.4)], 48).0 - lower).abs() > lower * 0.02);
        // both pickups at one place, out of phase: they cancel
        let cancelled = tone(&[(0, 2.0), (24, 0.2), (26, 0.2), (10, 0.0)], 48).0;
        assert!(cancelled < lower * 0.02, "{cancelled} vs {lower}");
        // an angle changes high notes more than the middle
        let flat = tone(&[(0, 0.0)], 96).0;
        let angled = tone(&[(0, 0.0), (27, 1.0)], 96).0;
        assert!((flat - angled).abs() > flat * 0.01);
    }

    #[test]
    fn stereo_spread_by_key_and_pickup() {
        let (l, r) = tone(&[], 84);
        assert!((l - r).abs() < l * 0.01, "no spread: centred");
        let (l, r) = tone(&[(28, 1.0)], 84);
        assert!(r > l * 1.3, "a high key to the right: {l} {r}");
        let (l, r) = tone(&[(28, 1.0)], 36);
        assert!(l > r * 1.3, "a low key to the left");
        let (l, r) = tone(&[(29, 1.0)], 60);
        assert!((l - r).abs() > l.min(r) * 0.05, "the pickups apart");
        let (l, r) = tone(&[(29, 1.0), (0, 0.0)], 60);
        assert!((l - r).abs() < l * 0.01, "one pickup: nothing to spread");
    }

    #[test]
    fn effects_chain_and_master() {
        let dry = tone(&[], 48).0;
        for (what, set) in [("distortion", vec![(13, 1.0), (14, 8.0)]), ("phaser", vec![(15, 1.0), (17, 1.0)]), ("flanger", vec![(15, 1.0), (30, 1.0)]), ("chorus", vec![(15, 1.0), (30, 2.0), (17, 1.0)]), ("wah", vec![(11, 1.0)]), ("compressor", vec![(34, 1.0), (35, 20.0)])] {
            let wet = tone(&set, 48).0;
            assert!((wet - dry).abs() > dry * 0.02, "{what}: {wet} vs {dry}");
            let mut off = set.clone();
            off.push((33, 0.0));
            let bypassed = tone(&off, 48).0;
            assert!((bypassed - dry).abs() < dry * 1e-3, "{what} with the master off");
        }
        // the order matters
        let a = tone(&[(13, 1.0), (14, 8.0), (11, 1.0), (36, 0.0)], 48).0;
        let b = tone(&[(13, 1.0), (14, 8.0), (11, 1.0), (36, 4.0)], 48).0;
        assert!((a - b).abs() > a * 0.02);
    }

    #[test]
    fn wah_pedal_envelope_and_learn() {
        let heel = tone(&[(11, 1.0), (57, 0.0)], 60).0;
        let toe = tone(&[(11, 1.0), (57, 1.0)], 60).0;
        assert!((heel - toe).abs() > heel.min(toe) * 0.05);
        let auto = tone(&[(11, 1.0), (40, 1.0)], 60).0;
        assert!((auto - heel).abs() > heel * 0.02, "the envelope follower moves the wah");
        // a controller as the pedal, and Learn
        let mut c = VintageClav::new(48000.0);
        c.set(56, 4.0);
        c.controller(4, 127);
        assert_eq!(c.pedal, 1.0);
        c.set(56, SRC_LEARN);
        c.controller(21, 64);
        assert_eq!(c.get(56), 21.0);
        c.set(56, SRC_LEARN);
        c.learn.as_mut().unwrap().left = 100;
        play(&mut c, 0.05);
        assert_eq!(c.get(56), 21.0, "Learn timed out: the old assignment");
        // CC 11 is the level unless it is the wah's controller
        c.controller(11, 0);
        assert_eq!(c.expression, 0.0);
        c.controller(11, 127);
        c.set(56, 11.0);
        c.controller(11, 0);
        assert_eq!(c.expression, 1.0);
    }

    #[test]
    fn damper_mutes_and_follows_a_controller() {
        let open = tone(&[], 48).0;
        let damped = tone(&[(7, 1.0)], 48).0;
        assert!(damped < open * 0.6, "{damped} vs {open}");
        let mut c = VintageClav::new(48000.0);
        c.set(55, 2.0);
        c.controller(2, 127);
        assert_eq!(c.sh.damper, 1.0);
    }

    #[test]
    fn voices_mono_and_legato() {
        let mut c = VintageClav::new(48000.0);
        c.set(53, 1.0);
        c.note_on(60, 100);
        c.note_on(64, 100);
        play(&mut c, 0.05);
        assert_eq!(c.poly.voices.iter().filter(|v| v.on && !v.killing).count(), 1, "Mono: one voice");
        let mut c = VintageClav::new(48000.0);
        c.set(53, 2.0);
        c.note_on(60, 100);
        play(&mut c, 0.05);
        let age = c.poly.voices.iter().find(|v| v.on).unwrap().age;
        c.note_on(64, 100);
        let v = c.poly.voices.iter().find(|v| v.on).unwrap();
        assert!(v.age >= age && v.note == 64, "Legato: the pitch changes, the string isn't struck again");
        c.note_off(64);
        assert_eq!(c.poly.voices.iter().find(|v| v.on).unwrap().note, 60, "back to the key still held");
        c.note_off(60);
        assert!(c.poly.voices.iter().all(|v| !v.on || v.released));
    }

    #[test]
    fn velocity_curves() {
        assert_eq!(velocity_curve(0, 0.9), 0.25);
        assert_eq!(velocity_curve(3, 0.1), 1.0);
        assert!(velocity_curve(4, 0.5) > 0.5 && velocity_curve(8, 0.5) < 0.5);
        assert_eq!(velocity_curve(6, 0.5), 0.5);
        assert_eq!(PARAMS.iter().find(|p| p.id == 54).unwrap().default, 6.0, "linear by default");
    }

    /// The pitch by autocorrelation between `lo` and `hi` Hz.
    fn fundamental(x: &[f32], lo: f32, hi: f32) -> f32 {
        let (a, b) = ((48000.0 / hi) as usize, (48000.0 / lo) as usize);
        let n = x.len() - b - 1;
        let mut best = (0.0f32, a);
        for lag in a..=b {
            let c: f32 = (0..n).step_by(3).map(|i| x[i] * x[i + lag]).sum();
            if c > best.0 {
                best = (c, lag);
            }
        }
        // (parabolic refinement between lags)
        let at = |lag: usize| -> f32 { (0..n).step_by(3).map(|i| x[i] * x[i + lag]).sum() };
        let (l, c, r) = (at(best.1 - 1), best.0, at(best.1 + 1));
        let d = 0.5 * (l - r) / (l - 2.0 * c + r);
        48000.0 / (best.1 as f32 + d)
    }

    #[test]
    fn stiffness_keeps_the_fundamental() {
        let pitch = |set: &[(u32, f32)], note: u8, lo: f32, hi: f32| {
            let mut c = VintageClav::new(48000.0);
            c.set(47, 0.0);
            c.set(0, 0.0);
            for (id, v) in set {
                c.set(*id, *v);
            }
            c.note_on(note, 100);
            play(&mut c, 0.15);
            fundamental(&play(&mut c, 0.5).0, lo, hi)
        };
        let (a, b) = (pitch(&[(8, 0.0)], 45, 95.0, 125.0), pitch(&[(8, 1.0), (46, 1.0)], 45, 95.0, 125.0));
        assert!((a - 110.0).abs() < 0.6, "{a}");
        assert!((a - b).abs() < 0.6, "stiff: {b} vs {a}");
        // Tune, and Stretch Tuning at the top
        let up = pitch(&[(49, 50.0)], 45, 95.0, 125.0);
        assert!((up / a - 2f32.powf(0.5 / 12.0)).abs() < 0.004, "{up}");
        let (flat, stretched) = (pitch(&[], 93, 1500.0, 1900.0), pitch(&[(50, 1.0)], 93, 1500.0, 1900.0));
        assert!(stretched > flat * 1.005, "{flat} {stretched}");
    }

    #[test]
    fn details_change_the_sound() {
        let base = tone(&[], 52).0;
        for (id, v) in [(41u32, 1.0f32), (45, 1.0), (31, 1.0), (31, -1.0), (5, 0.3), (54, 0.0)] {
            let x = tone(&[(id, v)], 52).0;
            assert!((x - base).abs() > base * 0.005, "param {id} = {v}: {x} vs {base}");
        }
    }
}
