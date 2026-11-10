//! "Vintage Keys EP": our electric piano, a stand-in for Logic's Vintage Electric Piano.
//!
//! Physical model per note: the tine (or reed) is a set of decaying partials (fundamental, the
//! tine's inharmonic "bell" mode, a few struck-string harmonics for the Electra-type models) whose
//! *displacement* drives a nonlinear pickup model. A tine passing an electromagnetic pickup gives
//! the classic soft-when-gentle, barking-when-hit-hard tone; an electrostatic reed pickup gives
//! the buzzier reed-piano growl. Dampers act on the displacement. After the voices: tone EQ,
//! warmth, drive, phaser, tremolo/auto-pan and chorus as on the original's effects row.
//!
//! Parameter ids are Logic's float-block indices (plugins/VINTAGE-EP-PARAM-MAP.txt).

use crate::dsp::{
    self, Biquad, CHUNK, Chorus, DcBlock, Delay, Fade, OnePole, Phaser, Rng, Svf, SvfMode,
};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

/// Model names (our stand-ins, in the order of Logic's model list as far as it is known; index
/// 15 is the tine model of Logic's new-track default "Deluxe Classic").
pub const MODELS: &[&str] = &[
    "Suitcase I",
    "Suitcase I Bright",
    "Suitcase II",
    "Suitcase IV",
    "Stage I",
    "Stage II",
    "Stage II Bright",
    "Stage II Hard",
    "Reed 200",
    "Reed 240",
    "Electra Keys",
    "Metal Keys",
    "Attack Keys",
    "Key Bass",
    "Modern Tines",
    "Classic Tines",
    "Reed Classic",
    "Reed Funk",
    "Reed Modern",
    "Reed Soft",
];

const MIDI_MODES: &[&str] = &["Poly", "Mono", "Legato"];
const MODES: &[&str] = &["Normal", "Full Remote"];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Model", MODELS, 15.0),
    Param::new(1, "Decay", 0.0, 200.0, 130.0, Unit::Percent),
    Param::new(2, "Release", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(3, "Bell Volume", 0.0, 2.0, 1.0, Unit::Ratio),
    Param::new(4, "Damper Volume", 0.0, 2.0, 1.0, Unit::Ratio),
    Param::new(5, "Lower Stretch", -1.0, 1.0, 0.0, Unit::Semitones),
    Param::new(6, "Upper Stretch", -1.0, 1.0, 0.4, Unit::Semitones),
    Param::new(7, "Stereo", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(8, "Voices", 1.0, 88.0, 88.0, Unit::None),
    Param::new(9, "Tune", -50.0, 50.0, 0.0, Unit::Cents),
    Param::new(10, "Warmth", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(11, "Treble", -12.0, 12.0, 4.0, Unit::Db),
    Param::new(12, "Bass", -12.0, 12.0, 2.0, Unit::Db),
    Param::new(13, "Drive Gain", 0.0, 10.0, 1.0, Unit::Ratio),
    Param::new(14, "Drive Tone", 500.0, 20000.0, 10000.0, Unit::Hz),
    Param::new(15, "Phaser Rate", 0.05, 10.0, 0.5, Unit::Hz),
    Param::new(16, "Phaser Color", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(17, "Phaser Stereo", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(18, "Tremolo Rate", 0.05, 16.0, 0.25, Unit::Hz),
    Param::new(19, "Tremolo Depth", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(20, "Tremolo Stereo", 0.0, 180.0, 180.0, Unit::None),
    Param::new(21, "Chorus Intensity", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(22, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(23, "Bend Range Down", -24.0, 0.0, -2.0, Unit::Semitones),
    Param::new(24, "Bend Range Up", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(25, "Chorus Rate", 0.05, 10.0, 2.0, Unit::Hz),
    Param::new(26, "Delay Pianissimo", 0.0, 10.0, 0.69, Unit::Ms),
    Param::new(27, "Delay Fortissimo", 0.0, 10.0, 0.0, Unit::Ms),
    Param::menu(28, "MIDI Mode", MIDI_MODES, 0.0),
    Param::new(29, "EQ", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(30, "Drive", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(31, "Phaser", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(32, "Tremolo", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(33, "Chorus", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(34, "Bass Boost", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(35, "Phaser Sync", 0.0, 100.0, 100.0, Unit::None),
    Param::new(36, "Tremolo Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(37, "Mode", MODES, 1.0),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pickup {
    /// Electromagnetic pickup next to a tine.
    Tine,
    /// Electrostatic pickup around a reed.
    Reed,
    /// Nearly linear (string with a passive pickup).
    String,
}

/// Physical character of one model.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Model {
    pub pickup: Pickup,
    /// Tine/reed offset from the pickup's centre (asymmetry → even harmonics).
    pub offset: f32,
    /// Displacement scale at full velocity, C3 (how hard the pickup is driven).
    pub drive: f32,
    pub bell_ratio: f32,
    pub bell_level: f32,
    pub bell_decay: f32,
    /// Fundamental T60 at C3 in seconds (before the Decay knob).
    pub decay: f32,
    pub hammer: f32,
    /// Voice low-pass at full velocity, Hz (darker when soft).
    pub tone: f32,
    /// Harmonic partials for string-like models (count; 0 = tine/reed only).
    pub harmonics: u8,
    pub out: f32,
}

const fn m(
    pickup: Pickup,
    offset: f32,
    drive: f32,
    bell_ratio: f32,
    bell_level: f32,
    bell_decay: f32,
    decay: f32,
    hammer: f32,
    tone: f32,
    harmonics: u8,
    out: f32,
) -> Model {
    Model {
        pickup,
        offset,
        drive,
        bell_ratio,
        bell_level,
        bell_decay,
        decay,
        hammer,
        tone,
        harmonics,
        out,
    }
}

use Pickup::*;
pub(crate) static MODEL_TABLE: [Model; 20] = [
    m(Tine, 0.55, 1.0, 6.9, 0.10, 0.25, 9.0, 0.10, 3500.0, 0, 1.00), // Suitcase I
    m(Tine, 0.50, 1.1, 6.9, 0.22, 0.30, 9.0, 0.15, 6000.0, 0, 1.00), // Suitcase I Bright
    m(Tine, 0.60, 1.0, 7.1, 0.12, 0.25, 8.5, 0.10, 3800.0, 0, 1.00), // Suitcase II
    m(
        Tine, 0.45, 1.15, 7.3, 0.16, 0.30, 8.5, 0.12, 4500.0, 0, 1.00,
    ), // Suitcase IV
    m(
        Tine, 0.65, 0.95, 6.8, 0.08, 0.20, 9.5, 0.12, 3000.0, 0, 1.05,
    ), // Stage I
    m(Tine, 0.58, 1.0, 7.0, 0.12, 0.25, 9.0, 0.12, 3600.0, 0, 1.00), // Stage II
    m(Tine, 0.50, 1.1, 7.0, 0.25, 0.35, 9.0, 0.15, 6500.0, 0, 1.00), // Stage II Bright
    m(
        Tine, 0.40, 1.35, 7.2, 0.20, 0.25, 8.0, 0.25, 5500.0, 0, 0.95,
    ), // Stage II Hard
    m(Reed, 0.30, 0.9, 6.3, 0.30, 0.08, 4.0, 0.10, 5000.0, 0, 0.80), // Reed 200
    m(Reed, 0.25, 1.0, 6.3, 0.35, 0.08, 3.5, 0.12, 6000.0, 0, 0.80), // Reed 240
    m(
        String, 0.20, 0.8, 4.1, 0.10, 0.20, 5.0, 0.20, 7000.0, 7, 0.80,
    ), // Electra Keys
    m(Tine, 0.35, 1.0, 5.4, 0.60, 0.80, 7.0, 0.10, 9000.0, 0, 0.85), // Metal Keys
    m(Tine, 0.45, 1.2, 7.0, 0.30, 0.15, 5.0, 0.50, 7000.0, 0, 0.90), // Attack Keys
    m(
        Tine, 0.65, 1.1, 6.6, 0.05, 0.20, 12.0, 0.08, 2000.0, 0, 1.10,
    ), // Key Bass
    m(
        Tine, 0.50, 1.05, 7.0, 0.25, 0.35, 10.0, 0.12, 5200.0, 0, 1.00,
    ), // Modern Tines
    m(
        Tine, 0.55, 1.0, 7.0, 0.12, 0.28, 10.0, 0.10, 3600.0, 0, 1.00,
    ), // Classic Tines
    m(
        Reed, 0.28, 0.95, 6.3, 0.30, 0.08, 4.0, 0.10, 5000.0, 0, 0.80,
    ), // Reed Classic
    m(Reed, 0.22, 1.2, 6.3, 0.40, 0.06, 3.0, 0.20, 7000.0, 0, 0.75), // Reed Funk
    m(Reed, 0.30, 0.9, 6.4, 0.25, 0.10, 4.5, 0.10, 5500.0, 0, 0.85), // Reed Modern
    m(Reed, 0.35, 0.7, 6.3, 0.15, 0.08, 4.5, 0.06, 3000.0, 0, 0.90), // Reed Soft
];

/// Derived settings the voices read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EpShared {
    pub model: Model,
    pub decay_scale: f32,
    pub release_t60: f32,
    pub bell: f32,
    pub damper: f32,
    pub lower_stretch: f32,
    pub upper_stretch: f32,
    pub tune_cents: f32,
    pub stereo: f32,
    pub rate: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Partial {
    phase: f32,
    ratio: f32,
    amp: f32,
    dec: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    On,
    Released,
}

pub(crate) struct EpVoice {
    state: State,
    parts: [Partial; 9],
    nparts: usize,
    hz: f32,
    /// Displacement multiplier (dampers act here).
    damp: f32,
    damp_coef: f32,
    attack: f32,
    attack_step: f32,
    fade: Fade,
    killing: bool,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    lp: OnePole,
    dc: DcBlock,
    rng: Rng,
    thump: f32,
    thump_coef: f32,
    thump_f: Svf,
    damper_noise: f32,
    damper_f: Svf,
    pickup: Pickup,
    offset: f32,
    bias: f32,
    delay_samples: u32,
    level: f32,
}

impl EpVoice {
    pub fn new(seed: u32) -> EpVoice {
        EpVoice {
            state: State::Idle,
            parts: [Partial::default(); 9],
            nparts: 0,
            hz: 440.0,
            damp: 1.0,
            damp_coef: 1.0,
            attack: 0.0,
            attack_step: 1.0,
            fade: Fade::default(),
            killing: false,
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            lp: OnePole::default(),
            dc: DcBlock::default(),
            rng: Rng::new(seed),
            thump: 0.0,
            thump_coef: 0.0,
            thump_f: Svf::default(),
            damper_noise: 0.0,
            damper_f: Svf::default(),
            pickup: Pickup::Tine,
            offset: 0.5,
            bias: 0.0,
            delay_samples: 0,
            level: 0.0,
        }
    }

    #[inline]
    fn pickup_fn(&self, x: f32) -> f32 {
        match self.pickup {
            // Field strength falls off around the pole piece: a rational bump centred off-axis.
            Pickup::Tine => {
                let u = x - self.offset;
                1.0 / (1.0 + u * u)
            }
            // Capacitance between reed and plate ~ 1/(gap - x).
            Pickup::Reed => {
                let u = (x * 0.55).clamp(-0.8, 0.8);
                u / (1.0 - u * self.offset * 1.6) + 0.15 * u * u * u
            }
            Pickup::String => dsp::soft_clip(x + self.offset * x * x),
        }
    }
}

impl Voice<EpShared> for EpVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &EpShared, ctl: &Ctl) {
        let md = sh.model;
        let rate = sh.rate;
        let n = note as f32;
        let v = velocity.max(1) as f32 / 127.0;
        // Stretch tuning: lower/upper halves of the keyboard (semitone fractions at the ends).
        let stretch = if n < 64.0 {
            sh.lower_stretch * (64.0 - n) / 43.0
        } else {
            sh.upper_stretch * (n - 64.0) / 44.0
        } * 0.5;
        self.hz = lg_dsp::midi_hz(n + stretch + sh.tune_cents / 100.0);
        let key = ((n - 60.0) / 12.0).clamp(-3.0, 4.0);
        // How far the tine swings (bigger for the long low tines, and with velocity).
        let swing = md.drive * 2.3 * 2f32.powf(-key * 0.6) * v.powf(1.15);
        let t60 = (md.decay * sh.decay_scale * 2f32.powf(-key * 0.75)).clamp(0.3, 40.0);
        let mut np = 0;
        let add = |parts: &mut [Partial; 9], np: &mut usize, ratio: f32, amp: f32, t: f32| {
            if *np < 9 && self.hz * ratio < rate * 0.45 {
                parts[*np] = Partial {
                    phase: 0.0,
                    ratio,
                    amp,
                    dec: dsp::t60_coef(t, rate),
                };
                *np += 1;
            }
        };
        let mut parts = [Partial::default(); 9];
        add(&mut parts, &mut np, 1.0, swing, t60);
        let bell_amp = swing * md.bell_level * sh.bell * (0.3 + 0.7 * v) * 2f32.powf(key * 0.3);
        add(
            &mut parts,
            &mut np,
            md.bell_ratio,
            bell_amp,
            md.bell_decay * 2f32.powf(-key * 0.3),
        );
        // A faint second tine mode helps the "tonebar" colour.
        add(&mut parts, &mut np, 2.0, swing * 0.04, t60 * 0.5);
        for h in 0..md.harmonics as usize {
            let k = (h + 3) as f32;
            add(
                &mut parts,
                &mut np,
                k - 1.0,
                swing * 0.5 / (k - 1.5) * (0.4 + v),
                t60 / (k * 0.7),
            );
        }
        self.parts = parts;
        self.nparts = np;
        self.pickup = md.pickup;
        self.offset = md.offset;
        self.bias = 0.0;
        self.bias = self.pickup_fn(0.0);
        self.damp = 1.0;
        self.damp_coef = 1.0;
        self.attack = 0.0;
        self.attack_step = 1.0 / (0.0015 * rate);
        self.killing = false;
        // Output gain: the reference levels (Logic's "Deluxe Classic" at C3 v100 ≈ -21 dBFS peak).
        let vel_gain = v.powf(1.3);
        self.gain = 0.19
            * md.out
            * vel_gain
            * 2f32.powf(key.clamp(0.0, 1.0) * 1.2 + (key - 1.0).max(0.0) * 0.3)
            * 2f32.powf(key.min(0.0) * 0.25);
        let p = ((n - 64.0) / 40.0).clamp(-1.0, 1.0) * 0.35 * sh.stereo;
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        let cutoff =
            (md.tone * (0.35 + 0.65 * v) * (1.0 + 0.6 * (ctl.brightness - 0.5))).min(rate * 0.45);
        self.lp.lowpass_hz(cutoff.max(self.hz * 2.0), rate);
        self.lp.z = 0.0;
        self.dc = DcBlock::default();
        self.thump = md.hammer * v * v;
        self.thump_coef = dsp::t60_coef(0.03, rate);
        self.thump_f = Svf::new(
            SvfMode::Band,
            (self.hz * 3.0).clamp(300.0, 4000.0),
            0.8,
            rate,
        );
        self.damper_noise = 0.0;
        self.damper_f = Svf::new(SvfMode::Band, 900.0, 1.2, rate);
        self.delay_samples = 0;
        self.state = State::On;
        self.level = swing;
    }

    fn release(&mut self, sh: &EpShared, _ctl: &Ctl) {
        if self.state != State::On {
            return;
        }
        self.state = State::Released;
        let t = sh.release_t60;
        self.damp_coef = dsp::t60_coef(t.max(0.01), sh.rate);
        self.damper_noise = 0.02 * sh.damper * self.level.min(1.5);
    }

    fn kill(&mut self, rate: f32) {
        if self.state != State::Idle {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &EpShared, ctl: &Ctl) {
        let rate = sh.rate;
        let base_inc = self.hz * ctl.bend_ratio / rate;
        let np = self.nparts;
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            let mut x = 0.0;
            for p in &mut self.parts[..np] {
                x += p.amp * dsp::sin_cycles(p.phase);
                p.phase += base_inc * p.ratio;
                if p.phase >= 1.0 {
                    p.phase -= 1.0;
                }
                p.amp *= p.dec;
            }
            if self.attack < 1.0 {
                self.attack = (self.attack + self.attack_step).min(1.0);
            }
            self.damp *= self.damp_coef;
            let disp = x * self.damp * self.attack;
            let mut y = self.pickup_fn(disp) - self.bias;
            y = self.lp.lp(self.dc.run(y));
            // hammer knock
            if self.thump > 1e-5 {
                y += self.thump_f.run(self.rng.bipolar()) * self.thump;
                self.thump *= self.thump_coef;
            }
            // felt damper touching the tine
            if self.damper_noise > 1e-6 {
                y += self.damper_f.run(self.rng.bipolar()) * self.damper_noise;
                self.damper_noise *= 0.9985;
            }
            let mut g = self.gain;
            if self.killing {
                g *= self.fade.next();
            }
            let out = y * g;
            peak = peak.max(out.abs());
            l[i] += out * self.pan_l;
            r[i] += out * self.pan_r;
        }
        self.level = peak;
        if self.killing && self.fade.gain <= 0.0 {
            self.state = State::Idle;
        }
        if self.state == State::Released && self.damp < 1e-4 && self.damper_noise < 1e-6 {
            self.state = State::Idle;
        }
        if self.state == State::On && self.parts[0].amp < 1e-4 && self.thump < 1e-5 {
            self.state = State::Idle;
        }
    }

    fn active(&self) -> bool {
        self.state != State::Idle
    }

    fn level(&self) -> f32 {
        self.level
    }
}

/// The effects row shared by the keyboard instruments (EP, Clav): EQ, warmth, drive, phaser,
/// tremolo, chorus.
pub(crate) struct KeysFx {
    rate: f32,
    bass: [Biquad; 2],
    treble: [Biquad; 2],
    boost: [Biquad; 2],
    warm: [OnePole; 2],
    drive_lp: [OnePole; 2],
    pub phaser: Phaser,
    pub chorus: Chorus,
    trem_phase: f32,
    widen: Delay,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KeysFxSettings {
    pub eq: bool,
    pub bass_db: f32,
    pub treble_db: f32,
    pub bass_boost: bool,
    pub warmth: f32,
    pub drive: bool,
    pub drive_gain: f32,
    pub drive_tone: f32,
    pub phaser: bool,
    pub phaser_rate: f32,
    pub phaser_color: f32,
    pub phaser_stereo: f32,
    pub tremolo: bool,
    pub trem_rate: f32,
    pub trem_depth: f32,
    pub trem_stereo: f32,
    pub chorus: bool,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub widen: f32,
    pub volume: f32,
}

impl KeysFx {
    pub fn new(rate: f32) -> KeysFx {
        KeysFx {
            rate,
            bass: [Biquad::default(); 2],
            treble: [Biquad::default(); 2],
            boost: [Biquad::default(); 2],
            warm: [OnePole::default(); 2],
            drive_lp: [OnePole::default(); 2],
            phaser: Phaser::default(),
            chorus: Chorus::new(rate),
            trem_phase: 0.0,
            widen: Delay::new((rate * 0.002) as usize),
        }
    }
    pub fn reset(&mut self) {
        for b in self
            .bass
            .iter_mut()
            .chain(self.treble.iter_mut())
            .chain(self.boost.iter_mut())
        {
            b.reset();
        }
        self.phaser.reset();
        self.chorus.reset();
        self.widen.clear();
    }
    pub fn configure(&mut self, s: &KeysFxSettings) {
        let rate = self.rate;
        for b in &mut self.bass {
            b.low_shelf(180.0, if s.eq { s.bass_db } else { 0.0 }, rate);
        }
        for b in &mut self.treble {
            b.high_shelf(3500.0, if s.eq { s.treble_db } else { 0.0 }, rate);
        }
        for b in &mut self.boost {
            b.peaking(90.0, 0.8, if s.bass_boost { 3.0 } else { 0.0 }, rate);
        }
        for w in &mut self.warm {
            w.lowpass_hz(18000.0 - 9000.0 * s.warmth, rate);
        }
        for w in &mut self.drive_lp {
            w.lowpass_hz(s.drive_tone, rate);
        }
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], s: &KeysFxSettings) {
        let rate = self.rate;
        for i in 0..l.len() {
            let mut x = [l[i], r[i]];
            for c in 0..2 {
                let mut v = self.boost[c].run(self.treble[c].run(self.bass[c].run(x[c])));
                // warmth: gentle tube-like asymmetry + top roll-off
                if s.warmth > 0.0 {
                    let w = s.warmth * 0.6;
                    v = self.warm[c].lp(v + w * 0.3 * v * v.abs());
                }
                if s.drive && s.drive_gain > 1.0 {
                    let g = s.drive_gain;
                    v = self.drive_lp[c].lp(dsp::soft_clip(v * g) / g.sqrt());
                }
                x[c] = v;
            }
            // a few hundred microseconds between the sides: the instrument's stereo pickup spread
            self.widen.push(x[1]);
            let d = s.widen * rate * 0.00012;
            if d > 0.0 {
                x[1] = self.widen.read(d);
            }
            l[i] = x[0];
            r[i] = x[1];
        }
        if s.phaser {
            self.phaser.process(
                l,
                r,
                s.phaser_rate,
                s.phaser_color,
                s.phaser_stereo,
                (s.phaser_color * 2.0).min(1.0),
                rate,
            );
        }
        if s.tremolo && s.trem_depth > 0.0 {
            let inc = s.trem_rate / rate;
            let off = s.trem_stereo / 360.0;
            for i in 0..l.len() {
                let ml = 0.5 + 0.5 * dsp::sin_cycles(self.trem_phase);
                let mr = 0.5 + 0.5 * dsp::sin_cycles(self.trem_phase + off);
                l[i] *= 1.0 - s.trem_depth * ml;
                r[i] *= 1.0 - s.trem_depth * mr;
                self.trem_phase += inc;
                if self.trem_phase >= 1.0 {
                    self.trem_phase -= 1.0;
                }
            }
        }
        if s.chorus && s.chorus_depth > 0.0 {
            self.chorus
                .process(l, r, s.chorus_rate, s.chorus_depth, s.chorus_depth.min(1.0));
        }
        for i in 0..l.len() {
            l[i] *= s.volume;
            r[i] *= s.volume;
        }
    }
}

pub struct VintageKeysEp {
    params: Params,
    poly: Poly<EpVoice>,
    ch: Channel,
    shared: EpShared,
    fx: KeysFx,
    fxs: KeysFxSettings,
    rate: f32,
    mono_mode: u8,
    last_note: Option<u8>,
}

impl VintageKeysEp {
    pub fn new(rate: f32) -> VintageKeysEp {
        dsp::warm_tables();
        let mut seed = 1u32;
        let mut s = VintageKeysEp {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 7;
                EpVoice::new(seed)
            }),
            ch: Channel::new(rate),
            shared: EpShared {
                model: MODEL_TABLE[15],
                decay_scale: 1.0,
                release_t60: 0.12,
                bell: 1.0,
                damper: 1.0,
                lower_stretch: 0.0,
                upper_stretch: 0.0,
                tune_cents: 0.0,
                stereo: 1.0,
                rate,
            },
            fx: KeysFx::new(rate),
            fxs: KeysFxSettings::default(),
            rate,
            mono_mode: 0,
            last_note: None,
        };
        s.update();
        s
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }

    fn update(&mut self) {
        let model = (self.p(0).round() as usize).min(MODEL_TABLE.len() - 1);
        self.shared.model = MODEL_TABLE[model];
        self.shared.decay_scale = (self.p(1) / 130.0).max(0.05);
        self.shared.release_t60 = 0.11 * (self.p(2) / 100.0).max(0.05);
        self.shared.bell = self.p(3);
        self.shared.damper = self.p(4);
        self.shared.lower_stretch = self.p(5);
        self.shared.upper_stretch = self.p(6);
        self.shared.stereo = self.p(7) / 100.0;
        self.poly.set_max((self.p(8).round() as usize).clamp(1, 32));
        self.shared.tune_cents = self.p(9);
        self.ch.bend_down = -self.p(23);
        self.ch.bend_up = self.p(24);
        self.ch.update_bend();
        self.mono_mode = self.p(28).round() as u8;
        let trem_rate = if self.p(36) >= 0.5 {
            // synced: the value is a note length in whole notes; 120 bpm until the host passes tempo
            (2.0 / (self.p(18).max(0.03) * 4.0)).clamp(0.1, 16.0)
        } else {
            self.p(18)
        };
        self.fxs = KeysFxSettings {
            eq: true,
            bass_db: self.p(12) * if self.p(29) >= 0.5 { 1.0 } else { 0.25 },
            treble_db: self.p(11) * if self.p(29) >= 0.5 { 1.0 } else { 0.25 },
            bass_boost: self.p(34) >= 0.5,
            warmth: self.p(10),
            drive: self.p(30) >= 0.5,
            drive_gain: self.p(13),
            drive_tone: self.p(14),
            phaser: self.p(31) >= 0.5 && self.p(16) > 0.0,
            phaser_rate: self.p(15),
            phaser_color: self.p(16),
            phaser_stereo: self.p(17),
            tremolo: self.p(32) >= 0.5 || self.ch.ctl.mod_wheel > 0.0,
            trem_rate,
            trem_depth: (self.p(19) / 100.0)
                .max(self.ch.ctl.mod_wheel * if self.p(32) >= 0.5 { 1.0 } else { 0.7 })
                .min(1.0),
            trem_stereo: self.p(20),
            chorus: self.p(33) >= 0.5,
            chorus_rate: self.p(25),
            chorus_depth: self.p(21),
            widen: self.shared.stereo,
            volume: lg_dsp::db_to_gain(self.p(22)),
        };
        self.fx.configure(&self.fxs);
    }
}

impl Plugin for VintageKeysEp {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        if self.params.set(id, value) {
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            *v = EpVoice::new(v.rng.0);
        }
        self.fx.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        1.0
    }
}

impl Instrument for VintageKeysEp {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.mono_mode > 0 {
            if let Some(n) = self.last_note {
                self.poly.note_off(n, &self.shared, &self.ch.ctl);
            }
            self.last_note = Some(pitch);
        }
        self.poly
            .note_on(pitch, velocity, &self.shared, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.last_note == Some(pitch) {
            self.last_note = None;
        }
        self.poly.note_off(pitch, &self.shared, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.poly.all_notes_off(&self.shared, &self.ch.ctl);
            return;
        }
        if let Some(down) = self.ch.cc(cc, value) {
            self.poly.sustain(down, &self.shared, &self.ch.ctl);
        }
        if cc == 1 {
            self.update();
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
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.shared, &self.ch.ctl);
            self.fx.process(&mut sl[..m], &mut sr[..m], &self.fxs);
            for i in 0..m {
                left[pos + i] += sl[i] * gl;
                right[pos + i] += sr[i] * gr;
            }
            pos += m;
        }
        let _ = self.rate;
    }
}
