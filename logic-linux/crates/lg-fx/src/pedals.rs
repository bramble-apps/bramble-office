//! The rest of Logic's stompboxes (Amps and Pedals ▸ Stompboxes), each usable on its own in an
//! Audio FX slot and inside Pedalboard: the parameters and behaviours the Effects guide gives
//! every pedal, our own circuits.
//!
//! - Delay: Spring Box (spring reverb: Time short / medium / long, Tone, Style Boutique / Simple
//!   / Vintage / Bright / Resonant, Mix), Tie Dye Delay (reverse delay: Time, Feedback, Tone,
//!   Bright / Dark, Mix, Listen, Sync).
//! - Distortion: Candy Fuzz, Double Dragon, Fuzz Machine, Grinder, Grit, Happy Face Fuzz,
//!   Hi-Drive, Monster Fuzz, OctaFuzz, Rawk! Distortion, Tube Burner — a shared drive circuit
//!   (input filter, clipping curve, tone, level) voiced per pedal with its own knobs.
//! - Dynamics: Squash Compressor (Sustain, Level, Attack Fast / Slow).
//! - Filter: Auto-Funk (envelope wah), Classic Wah, Modern Wah (Q, Mode), Graphic EQ (7 bands).
//! - Modulation: Flange Factory, Heavenly Chorus, Phase Tripper, Phaze 2, Retro Chorus,
//!   Roswell Ringer (ring modulator), Roto Phase, Spin Box (rotor cabinet), The Vibe (scanner
//!   vibrato / chorus), Trem-O-Tone.
//! - Pitch: Dr. Octave (two octaves down plus drive), Wham (pedal pitch shifter).
//!
//! Every pedal starts with its On switch; synced pedals have Sync next, and a synced Rate or
//! Time is a share of a whole note (0.25 = a quarter note) like Blue Echo's. Logic's float
//! layouts for these pedals aren't decoded, so the order after On is the guide's.

use crate::kit::{Shifter, Svf};
use crate::util::{Biquad, Coefs, Delay, Follower, Noise, PI, Phase, Smooth, Values, db, flush, plugin_impl, sine01, tri};
use lg_dsp::{Effect, Param, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SpringBox,
    TieDye,
    CandyFuzz,
    DoubleDragon,
    FuzzMachine,
    Grinder,
    Grit,
    HappyFace,
    HiDrive,
    MonsterFuzz,
    OctaFuzz,
    Rawk,
    TubeBurner,
    Squash,
    AutoFunk,
    ClassicWah,
    ModernWah,
    GraphicEq,
    FlangeFactory,
    HeavenlyChorus,
    PhaseTripper,
    Phaze2,
    RetroChorus,
    Roswell,
    RotoPhase,
    SpinBox,
    Vibe,
    TremOTone,
    DrOctave,
    Wham,
}

/// (Logic's name, kind, category) for every pedal here.
pub const PEDALS: &[(&str, Kind, &str)] = &[
    ("Spring Box", Kind::SpringBox, "Delay"),
    ("Tie Dye Delay", Kind::TieDye, "Delay"),
    ("Candy Fuzz", Kind::CandyFuzz, "Distortion"),
    ("Double Dragon", Kind::DoubleDragon, "Distortion"),
    ("Fuzz Machine", Kind::FuzzMachine, "Distortion"),
    ("Grinder", Kind::Grinder, "Distortion"),
    ("Grit", Kind::Grit, "Distortion"),
    ("Happy Face Fuzz", Kind::HappyFace, "Distortion"),
    ("Hi-Drive", Kind::HiDrive, "Distortion"),
    ("Monster Fuzz", Kind::MonsterFuzz, "Distortion"),
    ("OctaFuzz", Kind::OctaFuzz, "Distortion"),
    ("Rawk! Distortion", Kind::Rawk, "Distortion"),
    ("Tube Burner", Kind::TubeBurner, "Distortion"),
    ("Squash Compressor", Kind::Squash, "Dynamics"),
    ("Auto-Funk", Kind::AutoFunk, "Filter"),
    ("Classic Wah", Kind::ClassicWah, "Filter"),
    ("Modern Wah", Kind::ModernWah, "Filter"),
    ("Graphic EQ", Kind::GraphicEq, "Filter"),
    ("Flange Factory", Kind::FlangeFactory, "Modulation"),
    ("Heavenly Chorus", Kind::HeavenlyChorus, "Modulation"),
    ("Phase Tripper", Kind::PhaseTripper, "Modulation"),
    ("Phaze 2", Kind::Phaze2, "Modulation"),
    ("Retro Chorus", Kind::RetroChorus, "Modulation"),
    ("Roswell Ringer", Kind::Roswell, "Modulation"),
    ("Roto Phase", Kind::RotoPhase, "Modulation"),
    ("Spin Box", Kind::SpinBox, "Modulation"),
    ("The Vibe", Kind::Vibe, "Modulation"),
    ("Trem-O-Tone", Kind::TremOTone, "Modulation"),
    ("Dr. Octave", Kind::DrOctave, "Pitch"),
    ("Wham", Kind::Wham, "Pitch"),
];

const ON: Param = Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff);
const SYNC: Param = Param::new(1, "Sync", 0.0, 1.0, 0.0, Unit::OnOff);
const fn pct(id: u32, name: &'static str, d: f32) -> Param {
    Param::new(id, name, 0.0, 100.0, d, Unit::Percent)
}
const fn lvl(id: u32, name: &'static str, d: f32) -> Param {
    Param::new(id, name, -24.0, 12.0, d, Unit::Db)
}
const fn rate(id: u32, d: f32) -> Param {
    Param::new(id, "Rate", 0.01, 10.0, d, Unit::Hz)
}

static SPRING: [Param; 5] = [ON, Param::menu(1, "Time", &["Short", "Medium", "Long"], 1.0), pct(2, "Tone", 55.0), Param::menu(3, "Style", &["Boutique", "Simple", "Vintage", "Bright", "Resonant"], 0.0), pct(4, "Mix", 35.0)];
static TIEDYE: [Param; 8] = [ON, Param::new(1, "Sync", 0.0, 1.0, 1.0, Unit::OnOff), Param::new(2, "Time", 0.02, 2.0, 0.25, Unit::None), pct(3, "Feedback", 35.0), pct(4, "Tone", 60.0), Param::menu(5, "Bright/Dark", &["Bright", "Dark"], 1.0), pct(6, "Mix", 40.0), Param::new(7, "Listen", 0.0, 1.0, 0.0, Unit::OnOff)];
static CANDY: [Param; 3] = [ON, pct(1, "Drive", 60.0), lvl(2, "Level", -18.0)];
static DRAGON: [Param; 9] = [ON, pct(1, "Drive", 50.0), pct(2, "Tone", 50.0), lvl(3, "Level", -6.0), lvl(4, "Input", 0.0), pct(5, "Squash", 30.0), pct(6, "Contour", 40.0), pct(7, "Mix", 100.0), Param::menu(8, "Bright/Fat", &["Bright", "Fat"], 0.0)];
static FUZZM: [Param; 4] = [ON, pct(1, "Fuzz", 60.0), lvl(2, "Level", -8.0), pct(3, "Tone", 50.0)];
static GRINDER: [Param; 5] = [ON, pct(1, "Grind", 60.0), pct(2, "Filter", 50.0), lvl(3, "Level", -8.0), Param::menu(4, "Full/Scoop", &["Full", "Scoop"], 0.0)];
static GRIT: [Param; 4] = [ON, pct(1, "Volume", 50.0), pct(2, "Filter", 50.0), pct(3, "Distortion", 50.0)];
static HAPPY: [Param; 3] = [ON, pct(1, "Fuzz", 55.0), lvl(2, "Volume", -18.0)];
static HIDRIVE: [Param; 3] = [ON, lvl(1, "Level", -3.0), Param::menu(2, "Treble/Full", &["Treble", "Full"], 0.0)];
static MONSTER: [Param; 7] = [ON, pct(1, "Roar", 60.0), pct(2, "Growl", 50.0), pct(3, "Tone", 50.0), pct(4, "Texture", 30.0), pct(5, "Grain", 30.0), lvl(6, "Level", -10.0)];
static OCTA: [Param; 4] = [ON, pct(1, "Fuzz", 60.0), lvl(2, "Level", -20.0), pct(3, "Tone", 30.0)];
static RAWK: [Param; 4] = [ON, pct(1, "Crunch", 60.0), lvl(2, "Level", -10.0), pct(3, "Tone", 55.0)];
static TUBE: [Param; 11] = [
    ON,
    Param::new(1, "Fat", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(2, "Low", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(3, "Mid Freq", 200.0, 5000.0, 800.0, Unit::Hz),
    Param::new(4, "Mid Gain", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(5, "High", -12.0, 12.0, 0.0, Unit::Db),
    pct(6, "Tone", 50.0),
    pct(7, "Bias", 20.0),
    pct(8, "Squash", 20.0),
    pct(9, "Drive", 40.0),
    lvl(10, "Output", -6.0),
];
static SQUASH: [Param; 4] = [ON, pct(1, "Sustain", 50.0), lvl(2, "Level", 0.0), Param::menu(3, "Attack", &["Fast", "Slow"], 0.0)];
static FUNK: [Param; 6] = [ON, pct(1, "Sensitivity", 60.0), Param::new(2, "Cutoff", 200.0, 3000.0, 450.0, Unit::Hz), Param::menu(3, "BP/LP", &["BP", "LP"], 0.0), Param::menu(4, "Hi/Lo", &["Hi", "Lo"], 0.0), Param::menu(5, "Up/Down", &["Up", "Down"], 0.0)];
static CWAH: [Param; 2] = [ON, pct(1, "Pedal", 50.0)];
static MWAH: [Param; 4] = [ON, pct(1, "Pedal", 50.0), pct(2, "Q", 50.0), Param::menu(3, "Mode", &["Retro Wah", "Modern Wah", "Opto Wah 1", "Opto Wah 2", "Volume"], 1.0)];
const GEQ_HZ: [f32; 7] = [100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0];
static GEQ: [Param; 9] = [
    ON,
    Param::new(1, "100 Hz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(2, "200 Hz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(3, "400 Hz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(4, "800 Hz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(5, "1.6 kHz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(6, "3.2 kHz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(7, "6.4 kHz", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(8, "Level", -12.0, 12.0, 0.0, Unit::Db),
];
static FLANGE: [Param; 12] = [
    ON,
    SYNC,
    rate(2, 0.3),
    pct(3, "Depth", 50.0),
    pct(4, "Reso", 40.0),
    pct(5, "Mix", 50.0),
    Param::menu(6, "Wave", &["Sine", "Triangle", "Square"], 0.0),
    pct(7, "Symmetry", 50.0),
    pct(8, "Curve", 50.0),
    Param::new(9, "Manual", 0.1, 10.0, 2.0, Unit::Ms),
    Param::new(10, "Low", 20.0, 2000.0, 20.0, Unit::Hz),
    Param::new(11, "High", 1000.0, 20000.0, 20000.0, Unit::Hz),
];
static HEAVEN: [Param; 7] = [ON, SYNC, rate(2, 0.6), pct(3, "Depth", 50.0), Param::new(4, "Bright", 0.0, 1.0, 0.0, Unit::OnOff), pct(5, "Feedback", 10.0), pct(6, "Density", 50.0)];
static TRIPPER: [Param; 5] = [ON, SYNC, rate(2, 0.5), pct(3, "Depth", 60.0), pct(4, "Feedback", 30.0)];
static PHAZE2: [Param; 15] = [
    ON,
    SYNC,
    Param::new(2, "LFO 1 Rate", 0.01, 10.0, 0.4, Unit::Hz),
    Param::new(3, "LFO 2 Rate", 0.01, 10.0, 0.13, Unit::Hz),
    Param::new(4, "LFO 1 Floor", 50.0, 5000.0, 200.0, Unit::Hz),
    Param::new(5, "LFO 1 Ceiling", 200.0, 18000.0, 3000.0, Unit::Hz),
    Param::new(6, "LFO 2 Floor", 50.0, 5000.0, 400.0, Unit::Hz),
    Param::new(7, "LFO 2 Ceiling", 200.0, 18000.0, 6000.0, Unit::Hz),
    Param::new(8, "LFO 1 Order", 2.0, 12.0, 4.0, Unit::None),
    Param::new(9, "LFO 2 Order", 2.0, 12.0, 6.0, Unit::None),
    pct(10, "LFO 1 Feedback", 30.0),
    pct(11, "LFO 2 Feedback", 30.0),
    Param::new(12, "LFO 1 Tone", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(13, "LFO 2 Tone", -100.0, 100.0, 0.0, Unit::Percent),
    pct(14, "LFO 1/2 Mix", 50.0),
];
static RETRO: [Param; 4] = [ON, SYNC, rate(2, 0.5), pct(3, "Depth", 50.0)];
static ROSWELL: [Param; 6] = [ON, Param::menu(1, "Lin/Exp", &["Lin", "Exp"], 1.0), Param::new(2, "Freq", 0.0, 5000.0, 220.0, Unit::Hz), Param::new(3, "Fine", -50.0, 50.0, 0.0, Unit::Cents), pct(4, "FB", 0.0), pct(5, "Mix", 50.0)];
static ROTO: [Param; 5] = [ON, SYNC, rate(2, 0.4), pct(3, "Intensity", 60.0), Param::menu(4, "Vintage/Modern", &["Vintage", "Modern"], 0.0)];
static SPIN: [Param; 7] = [
    ON,
    Param::menu(1, "Cabinet", &["Wood", "Proline", "Single", "Split Horn"], 0.0),
    Param::new(2, "Fast Rate", 3.0, 9.0, 6.7, Unit::Hz),
    pct(3, "Response", 50.0),
    pct(4, "Drive", 20.0),
    Param::new(5, "Bright", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(6, "Speed", &["Slow", "Brake", "Fast"], 0.0),
];
static VIBE: [Param; 5] = [ON, SYNC, rate(2, 6.0), pct(3, "Depth", 60.0), Param::menu(4, "Type", &["V1", "V2", "V3", "C1", "C2", "C3"], 3.0)];
static TREMO: [Param; 5] = [ON, SYNC, rate(2, 4.0), pct(3, "Depth", 60.0), lvl(4, "Level", 0.0)];
static OCTAVE: [Param; 5] = [ON, pct(1, "Octave 1", 60.0), pct(2, "Octave 2", 30.0), pct(3, "Direct", 50.0), pct(4, "Drive", 20.0)];
static WHAM: [Param; 4] = [ON, pct(1, "Pedal", 0.0), Param::new(2, "Tune", -24.0, 24.0, 12.0, Unit::Semitones), pct(3, "Mix", 100.0)];

fn params_of(k: Kind) -> &'static [Param] {
    use Kind::*;
    match k {
        SpringBox => &SPRING,
        TieDye => &TIEDYE,
        CandyFuzz => &CANDY,
        DoubleDragon => &DRAGON,
        FuzzMachine => &FUZZM,
        Grinder => &GRINDER,
        Grit => &GRIT,
        HappyFace => &HAPPY,
        HiDrive => &HIDRIVE,
        MonsterFuzz => &MONSTER,
        OctaFuzz => &OCTA,
        Rawk => &RAWK,
        TubeBurner => &TUBE,
        Squash => &SQUASH,
        AutoFunk => &FUNK,
        ClassicWah => &CWAH,
        ModernWah => &MWAH,
        GraphicEq => &GEQ,
        FlangeFactory => &FLANGE,
        HeavenlyChorus => &HEAVEN,
        PhaseTripper => &TRIPPER,
        Phaze2 => &PHAZE2,
        RetroChorus => &RETRO,
        Roswell => &ROSWELL,
        RotoPhase => &ROTO,
        SpinBox => &SPIN,
        Vibe => &VIBE,
        TremOTone => &TREMO,
        DrOctave => &OCTAVE,
        Wham => &WHAM,
    }
}

/// A rotating-speaker cabinet: horn and drum turning at their own speeds and inertia, each
/// giving amplitude and Doppler modulation, heard by two microphones (the left and right).
pub struct Rotor {
    rate: f32,
    horn_line: [Delay; 2],
    drum_line: [Delay; 2],
    xo: [Biquad; 2],
    pub horn_hz: f32,
    pub drum_hz: f32,
    horn_ph: f32,
    drum_ph: f32,
}

impl Rotor {
    pub fn new(rate: f32) -> Rotor {
        let d = || Delay::new((0.01 * rate) as usize);
        Rotor { rate, horn_line: [d(), d()], drum_line: [d(), d()], xo: [Biquad::new(Coefs::lowpass(rate, 800.0, 0.707)), Biquad::new(Coefs::highpass(rate, 800.0, 0.707))], horn_hz: 0.8, drum_hz: 0.7, horn_ph: 0.0, drum_ph: 0.25 }
    }
    /// Moves the speeds towards their targets with the horn's and drum's inertia (seconds).
    pub fn spin(&mut self, horn_to: f32, drum_to: f32, horn_secs: f32, drum_secs: f32, n: usize) {
        let k = |s: f32| 1.0 - (-(n as f32) / (s.max(0.01) * self.rate)).exp();
        self.horn_hz += (horn_to - self.horn_hz) * k(horn_secs);
        self.drum_hz += (drum_to - self.drum_hz) * k(drum_secs);
    }
    /// A mono sample in; (left mic, right mic) out. Mic spread 0..1.
    #[inline]
    pub fn run(&mut self, x: f32, spread: f32, horn_level: f32, drum_level: f32) -> (f32, f32) {
        let lo = self.xo[0].run(x);
        let hi = self.xo[1].run(x);
        self.horn_ph = (self.horn_ph + self.horn_hz / self.rate).fract();
        self.drum_ph = (self.drum_ph + self.drum_hz / self.rate).fract();
        let mut out = [0f32; 2];
        for (s, o) in out.iter_mut().enumerate() {
            let off = 0.5 * spread * s as f32;
            let h = sine01(self.horn_ph + off);
            let d = sine01(self.drum_ph + off);
            self.horn_line[s].push(hi);
            self.drum_line[s].push(lo);
            let hd = (0.0012 + 0.0009 * h) * self.rate;
            let dd = (0.0015 + 0.0006 * d) * self.rate;
            let hy = self.horn_line[s].read_lin(hd) * (1.0 - 0.45 * (0.5 - 0.5 * h));
            let dy = self.drum_line[s].read_lin(dd) * (1.0 - 0.25 * (0.5 - 0.5 * d));
            *o = hy * horn_level + dy * drum_level;
        }
        (out[0], out[1])
    }
    pub fn clear(&mut self) {
        for d in self.horn_line.iter_mut().chain(self.drum_line.iter_mut()) {
            d.clear();
        }
    }
}

/// Runs biquads `ks` of a bank in order.
#[inline]
fn bqs(b: &mut [Biquad; 8], ks: &[usize], x: f32) -> f32 {
    ks.iter().fold(x, |y, k| b[*k].run(y))
}

/// A chain of first-order all-passes at one cutoff (a phaser's stages).
#[inline]
fn allpass_chain(st: &mut [f32], x: f32, rate: f32, hz: f32) -> f32 {
    let t = (PI * hz.clamp(20.0, rate * 0.45) / rate).tan();
    let a = (t - 1.0) / (t + 1.0);
    let mut x = x;
    for s in st.iter_mut() {
        let y = a * x + *s;
        *s = flush(x - a * y);
        x = y;
    }
    x
}

/// A clipping curve: 0 soft (tanh), 1 hard, 2 asymmetric fuzz, 3 fold-back, 4 rectified
/// (octave up), 5 tube (biased).
#[inline]
fn clip(kind: u8, x: f32, bias: f32) -> f32 {
    match kind {
        0 => x.tanh(),
        1 => x.clamp(-1.0, 1.0),
        2 => {
            let y = x + bias;
            (if y > 0.0 { y.tanh() } else { (y * 0.6).tanh() / 0.6 * 0.8 }) - bias.tanh()
        }
        3 => {
            let mut y = x;
            for _ in 0..4 {
                if y > 1.0 {
                    y = 2.0 - y;
                } else if y < -1.0 {
                    y = -2.0 - y;
                }
            }
            y
        }
        // rectified (octave up), with the rectifier's DC at rest taken out
        4 => (x.abs() * 1.4 - 0.4).tanh() - (-0.4f32).tanh() + 0.3 * x.tanh(),
        _ => {
            let y = x + bias * 0.5;
            let s = if y >= 0.0 { 1.0 - (-y).exp() } else { -(1.0 - y.exp()) * 0.9 };
            s - (if bias >= 0.0 { 1.0 - (-bias * 0.5).exp() } else { 0.0 })
        }
    }
}

pub struct Pedal {
    kind: Kind,
    p: Values,
    rate: f32,
    bpm: f64,
    bq: [[Biquad; 8]; 2],
    svf: [[Svf; 2]; 2],
    line: [Delay; 2],
    ap: [[f32; 12]; 2],
    ap2: [[f32; 12]; 2],
    ph: [Phase; 2],
    env: [Follower; 2],
    fb: [f32; 2],
    fb2: [f32; 2],
    misc: [f32; 8],
    rotor: Option<Box<Rotor>>,
    shift: Option<Box<[Shifter; 2]>>,
    noise: Noise,
    gain: Smooth,
    pos: usize,
}

impl Pedal {
    pub fn new(kind: Kind, rate: f32) -> Pedal {
        let line_secs = match kind {
            Kind::SpringBox => 0.2,
            Kind::TieDye => 4.2,
            _ => 0.05,
        };
        let mut p = Pedal {
            kind,
            p: Values::new(params_of(kind)),
            rate,
            bpm: 120.0,
            bq: [[Biquad::default(); 8]; 2],
            svf: [[Svf::default(); 2]; 2],
            line: [Delay::new((line_secs * rate) as usize), Delay::new((line_secs * rate) as usize)],
            ap: [[0.0; 12]; 2],
            ap2: [[0.0; 12]; 2],
            ph: [Phase::default(); 2],
            env: [Follower::new(rate, 3.0, 120.0); 2],
            fb: [0.0; 2],
            fb2: [0.0; 2],
            misc: [0.0; 8],
            rotor: matches!(kind, Kind::SpinBox).then(|| Box::new(Rotor::new(rate))),
            shift: matches!(kind, Kind::Wham).then(|| Box::new([Shifter::new(rate, 35.0), Shifter::new(rate, 35.0)])),
            noise: Noise(0x1234_5677),
            gain: Smooth::new(rate, 20.0, 1.0),
            pos: 0,
        };
        p.update();
        p.gain.snap();
        p
    }

    pub fn by_name(name: &str, rate: f32) -> Option<Pedal> {
        PEDALS.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name.trim())).map(|(_, k, _)| Pedal::new(*k, rate))
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    fn hz(&self, id: u32) -> f32 {
        let v = self.p.get(id);
        if self.p.on(1) && params_of(self.kind).get(1).is_some_and(|p| p.name == "Sync") {
            // synced: the nearest note length (1/1 .. 1/32) to the knob's rate
            let beat_hz = self.bpm as f32 / 60.0;
            let beats = 2f32.powf((beat_hz / v.max(0.01)).log2().round().clamp(-3.0, 2.0));
            beat_hz / beats
        } else {
            v
        }
    }

    fn set_bq(&mut self, k: usize, c: Coefs) {
        self.bq[0][k].c = c;
        self.bq[1][k].c = c;
    }

    fn update(&mut self) {
        use Kind::*;
        let (p, r) = (&self.p, self.rate);
        let g = |id| p.get(id);
        let thru = Coefs::THRU;
        let mut c = [thru; 8];
        let mut out_db = 0.0;
        match self.kind {
            CandyFuzz => {
                c[0] = Coefs::highpass(r, 300.0, 0.7);
                c[1] = Coefs::high_shelf(r, 3000.0, 0.7, 4.0);
                c[2] = Coefs::lowpass(r, 7000.0, 0.7);
                out_db = g(2);
            }
            DoubleDragon => {
                c[0] = Coefs::highpass(r, 90.0, 0.7);
                c[1] = Coefs::lowpass(r, 1000.0 * (12.0f32).powf(g(2) / 100.0), 0.7);
                c[2] = Coefs::high_shelf(r, if p.idx(8) == 0 { 3500.0 } else { 1500.0 }, 0.7, if p.idx(8) == 0 { 4.0 } else { -3.0 });
                out_db = g(3);
            }
            FuzzMachine | OctaFuzz | Rawk | Grinder | Grit | HappyFace | MonsterFuzz => {
                let t = match self.kind {
                    FuzzMachine => g(3),
                    OctaFuzz => g(3),
                    Rawk => g(3),
                    Grinder | Grit => g(2),
                    MonsterFuzz => g(3),
                    _ => 40.0,
                } / 100.0;
                c[0] = Coefs::highpass(r, if self.kind == OctaFuzz { 60.0 + 900.0 * t } else { 80.0 }, 0.7);
                c[1] = Coefs::lowpass(r, 1500.0 * (8.0f32).powf(t), 0.7);
                c[2] = match self.kind {
                    Grinder => Coefs::peak(r, 700.0, if p.idx(4) == 0 { 0.7 } else { 1.4 }, if p.idx(4) == 0 { -4.0 } else { -12.0 }),
                    FuzzMachine => Coefs::low_shelf(r, 300.0, 0.7, -8.0 * t),
                    _ => thru,
                };
                out_db = match self.kind {
                    FuzzMachine | OctaFuzz | Rawk => g(2),
                    Grinder => g(3),
                    MonsterFuzz => g(6) - 6.0 * t,
                    HappyFace => g(2),
                    _ => -16.0,
                };
            }
            HiDrive => {
                c[0] = if p.idx(2) == 0 { Coefs::highpass(r, 800.0, 0.7) } else { Coefs::highpass(r, 60.0, 0.7) };
                c[1] = Coefs::lowpass(r, 9000.0, 0.7);
                out_db = g(1);
            }
            TubeBurner => {
                c[0] = Coefs::low_shelf(r, 120.0, 0.7, g(2) + if p.on(1) { 4.0 } else { 0.0 });
                c[1] = Coefs::peak(r, g(3), 0.9, g(4));
                c[2] = Coefs::high_shelf(r, 3000.0, 0.7, g(5));
                c[3] = Coefs::lowpass(r, 2000.0 * (8.0f32).powf(g(6) / 100.0), 0.7);
                out_db = g(10);
            }
            Squash => {
                let att = if p.idx(3) == 0 { 1.0 } else { 25.0 };
                for e in self.env.iter_mut() {
                    e.set(r, att, 180.0);
                }
                out_db = g(2);
            }
            AutoFunk => {
                for e in self.env.iter_mut() {
                    e.set(r, 4.0, 90.0);
                }
            }
            GraphicEq => {
                for (k, f) in GEQ_HZ.iter().enumerate() {
                    c[k] = if g(1 + k as u32).abs() < 0.01 { thru } else { Coefs::peak(r, *f, 1.4, g(1 + k as u32)) };
                }
                out_db = g(8);
            }
            FlangeFactory => {
                c[0] = if g(10) <= 21.0 { thru } else { Coefs::highpass(r, g(10), 0.7) };
                c[1] = if g(11) >= 19999.0 { thru } else { Coefs::lowpass(r, g(11).min(r * 0.45), 0.7) };
            }
            HeavenlyChorus => {
                c[0] = if p.on(4) { Coefs::high_shelf(r, 4000.0, 0.7, 4.0) } else { thru };
            }
            RotoPhase => {
                c[0] = if p.idx(4) == 0 { Coefs::peak(r, 1200.0, 0.7, 3.0) } else { thru };
                c[1] = if p.idx(4) == 0 { Coefs::lowpass(r, 6000.0, 0.7) } else { thru };
            }
            TieDye => {
                let cut = 800.0 * (20.0f32).powf(g(4) / 100.0);
                c[0] = Coefs::lowpass(r, cut.min(r * 0.45), 0.7);
                c[1] = if p.idx(5) == 0 { Coefs::high_shelf(r, 3000.0, 0.7, 3.0) } else { Coefs::high_shelf(r, 2000.0, 0.7, -5.0) };
                c[2] = Coefs::highpass(r, 120.0, 0.7);
            }
            SpringBox => {
                let style = p.idx(3);
                let cut = 1500.0 * (8.0f32).powf(g(2) / 100.0);
                c[0] = Coefs::lowpass(r, cut.min(r * 0.45), 0.7);
                c[1] = Coefs::highpass(r, [250.0, 150.0, 300.0, 400.0, 200.0][style], 0.7);
                c[2] = match style {
                    3 => Coefs::high_shelf(r, 3000.0, 0.7, 5.0),
                    4 => Coefs::peak(r, 1800.0, 4.0, 8.0),
                    2 => Coefs::lowpass(r, 4000.0, 0.7),
                    _ => thru,
                };
            }
            DrOctave => {
                c[0] = Coefs::lowpass(r, 900.0, 0.7);
                c[1] = Coefs::lowpass(r, 900.0, 0.7);
                c[2] = Coefs::lowpass(r, 1200.0, 0.7);
                c[3] = Coefs::lowpass(r, 1200.0, 0.7);
            }
            TremOTone => out_db = g(4),
            _ => {}
        }
        for (k, cc) in c.iter().enumerate() {
            self.set_bq(k, *cc);
        }
        self.gain.target = db(out_db);
    }

    fn clear(&mut self) {
        for b in self.bq.iter_mut().flatten() {
            b.clear();
        }
        for s in self.svf.iter_mut().flatten() {
            s.clear();
        }
        for l in self.line.iter_mut() {
            l.clear();
        }
        self.ap = [[0.0; 12]; 2];
        self.ap2 = [[0.0; 12]; 2];
        self.fb = [0.0; 2];
        self.fb2 = [0.0; 2];
        self.misc = [0.0; 8];
        if let Some(r) = self.rotor.as_mut() {
            r.clear();
        }
        if let Some(s) = self.shift.as_mut() {
            s[0].clear();
            s[1].clear();
        }
    }

    fn tail_secs(&self) -> f32 {
        match self.kind {
            Kind::SpringBox => 3.0,
            Kind::TieDye => 6.0,
            _ => 0.05,
        }
    }

    /// The drive circuits: (pre-gain, curve, bias) per distortion pedal.
    fn drive_setup(&self) -> (f32, u8, f32, f32) {
        use Kind::*;
        let g = |id| self.p.get(id) / 100.0;
        // (input gain, curve, bias, wet mix)
        match self.kind {
            CandyFuzz => (db(6.0 + 40.0 * g(1)), 1, 0.0, 1.0),
            DoubleDragon => (db(self.p.get(4) + 3.0 + 36.0 * g(1)), if g(6) > 0.5 { 3 } else { 0 }, 0.1 * g(6), g(7)),
            FuzzMachine => (db(10.0 + 40.0 * g(1)), 2, 0.25, 1.0),
            Grinder => (db(12.0 + 42.0 * g(1)), 1, 0.05, 1.0),
            Grit => (db(6.0 + 30.0 * g(1)), 1, 0.0, 1.0),
            HappyFace => (db(8.0 + 32.0 * g(1)), 2, 0.4, 1.0),
            HiDrive => (db(20.0), 0, 0.0, 1.0),
            MonsterFuzz => (db(10.0 + 40.0 * g(1)), 2, 0.15 + 0.3 * g(5), 1.0),
            OctaFuzz => (db(10.0 + 36.0 * g(1)), 4, 0.0, 1.0),
            Rawk => (db(14.0 + 40.0 * g(1)), 1, 0.0, 1.0),
            TubeBurner => (db(4.0 + 34.0 * g(9)), 5, 0.6 * g(7), 1.0),
            _ => (1.0, 0, 0.0, 1.0),
        }
    }
}

impl Effect for Pedal {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use Kind::*;
        if !self.p.on(0) {
            return;
        }
        let rate = self.rate;
        let n = left.len().min(right.len());
        let g = |s: &Self, id: u32| s.p.get(id);
        match self.kind {
            CandyFuzz | DoubleDragon | FuzzMachine | Grinder | Grit | HappyFace | HiDrive | MonsterFuzz | OctaFuzz | Rawk | TubeBurner => {
                let (pre, curve, bias, mix) = self.drive_setup();
                let squash = match self.kind {
                    DoubleDragon => g(self, 5) / 100.0,
                    TubeBurner => g(self, 8) / 100.0,
                    _ => 0.0,
                };
                let grit_post = if self.kind == Grit { db(-12.0 + 24.0 * g(self, 3) / 100.0) } else { 1.0 };
                let growl = if self.kind == MonsterFuzz { g(self, 2) / 100.0 } else { 0.0 };
                let texture = if self.kind == MonsterFuzz { g(self, 4) / 100.0 } else { 0.0 };
                for i in 0..n {
                    let gn = self.gain.next();
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let dry = *x;
                        let mut y = if self.kind == TubeBurner { bqs(&mut self.bq[s], &[0, 1, 2], dry) } else { self.bq[s][0].run(dry) };
                        if squash > 0.0 {
                            let e = self.env[s].run(y.abs());
                            y /= 1.0 + squash * 6.0 * e;
                        }
                        let mut z = clip(curve, y * pre, bias);
                        if growl > 0.0 {
                            z = clip(0, z * (1.0 + 6.0 * growl), 0.0);
                        }
                        if texture > 0.0 {
                            // a rougher edge: a little gated noise riding the signal
                            z += self.noise.next() * texture * 0.05 * z.abs();
                        }
                        z *= grit_post;
                        z = match self.kind {
                            TubeBurner => self.bq[s][3].run(z),
                            DoubleDragon => bqs(&mut self.bq[s], &[1, 2], z),
                            _ => {
                                let a = self.bq[s][1].run(z);
                                self.bq[s][2].run(a)
                            }
                        };
                        *x = (dry * (1.0 - mix) + z * mix) * gn;
                    }
                }
            }
            Squash => {
                let thr = db(-6.0 - 36.0 * g(self, 1) / 100.0);
                for i in 0..n {
                    let gn = self.gain.next();
                    let e = self.env[0].run(left[i].abs().max(right[i].abs()));
                    let over = (e / thr).max(1.0);
                    // 4:1 above the threshold, the make-up following Sustain
                    let gr = over.powf(-0.75);
                    let mk = db(18.0 * g(self, 1) / 100.0);
                    left[i] *= gr * mk * gn;
                    right[i] *= gr * mk * gn;
                }
            }
            AutoFunk | ClassicWah | ModernWah => {
                let (q, lp) = match self.kind {
                    AutoFunk => (if self.p.idx(4) == 0 { 5.0 } else { 2.0 }, self.p.idx(3) == 1),
                    ClassicWah => (4.0, false),
                    _ => (1.0 + 9.0 * g(self, 2) / 100.0, false),
                };
                let mode = if self.kind == ModernWah { self.p.idx(3) } else { 0 };
                for i in 0..n {
                    let pos = match self.kind {
                        AutoFunk => {
                            let e = self.env[0].run(left[i].abs().max(right[i].abs()));
                            let m = (e * (1.0 + 20.0 * g(self, 1) / 100.0)).min(1.0);
                            if self.p.idx(5) == 0 { m } else { -m }
                        }
                        _ => g(self, 1) / 100.0,
                    };
                    if mode == 4 {
                        // Volume pedal
                        left[i] *= pos * pos;
                        right[i] *= pos * pos;
                        continue;
                    }
                    let f = match self.kind {
                        AutoFunk => g(self, 2) * (6.0f32).powf(pos),
                        _ => 350.0 * (2400.0f32 / 350.0).powf(pos) * [1.0, 1.0, 0.8, 1.2, 1.0][mode],
                    };
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        self.svf[s][0].set(rate, f, q);
                        let o = self.svf[s][0].run(*x);
                        // the band output normalised to unity at its peak, a little dry kept
                        *x = if lp { o.low } else { o.band / q * 2.0 + if self.kind == ClassicWah { 0.1 * *x } else { 0.3 * *x } };
                    }
                }
            }
            GraphicEq => {
                for i in 0..n {
                    let gn = self.gain.next();
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let mut y = *x;
                        for k in 0..7 {
                            y = self.bq[s][k].run(y);
                        }
                        *x = y * gn;
                    }
                }
            }
            FlangeFactory => self.flange(left, right),
            HeavenlyChorus | RetroChorus => {
                let hz = self.hz(2);
                let depth = g(self, 3) / 100.0;
                let (fbk, dens) = if self.kind == HeavenlyChorus { (g(self, 5) / 100.0 * 0.7, g(self, 6) / 100.0) } else { (0.0, 0.5) };
                for i in 0..n {
                    let ph = self.ph[0].step(rate, hz);
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let m = sine01(ph + 0.25 * s as f32);
                        let d = (0.007 + 0.004 * depth * m) * rate;
                        let dry = *x;
                        self.line[s].push(dry + self.fb[s] * fbk);
                        let mut y = self.line[s].read_lin(d);
                        if self.kind == HeavenlyChorus {
                            let d2 = (0.012 + 0.005 * depth * sine01(ph + 0.5 + 0.25 * s as f32)) * rate;
                            y = 0.6 * y + 0.6 * self.line[s].read_lin(d2);
                            y = self.bq[s][0].run(y);
                        }
                        self.fb[s] = flush(y);
                        *x = dry * (1.0 - 0.5 * dens) + y * dens;
                    }
                }
            }
            PhaseTripper | RotoPhase => {
                let hz = self.hz(2);
                let depth = g(self, 3) / 100.0;
                let fbk = if self.kind == PhaseTripper { g(self, 4) / 100.0 * 0.8 } else { 0.3 };
                let stages = if self.kind == PhaseTripper { 4 } else { 6 };
                for i in 0..n {
                    let ph = self.ph[0].step(rate, hz);
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let m = 0.5 + 0.5 * tri(ph + 0.25 * s as f32);
                        let f = 300.0 * (12.0f32).powf(m * depth);
                        let dry = *x;
                        let y = allpass_chain(&mut self.ap[s][..stages], dry + self.fb[s] * fbk, rate, f);
                        self.fb[s] = flush(y);
                        let mut o = 0.5 * (dry + y);
                        if self.kind == RotoPhase {
                            o = bqs(&mut self.bq[s], &[0, 1], o);
                        }
                        *x = o;
                    }
                }
            }
            Phaze2 => {
                let (h1, h2) = (self.hz(2), self.hz(3));
                let ord = [g(self, 8).round().clamp(2.0, 12.0) as usize, g(self, 9).round().clamp(2.0, 12.0) as usize];
                let fl = [(g(self, 4), g(self, 5)), (g(self, 6), g(self, 7))];
                let fbk = [g(self, 10) / 100.0 * 0.8, g(self, 11) / 100.0 * 0.8];
                let tone = [g(self, 12) / 100.0, g(self, 13) / 100.0];
                let mix = g(self, 14) / 100.0;
                for i in 0..n {
                    let p1 = self.ph[0].step(rate, h1);
                    let p2 = self.ph[1].step(rate, h2);
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let dry = *x;
                        let mut o = [0f32; 2];
                        for k in 0..2 {
                            let m = 0.5 + 0.5 * sine01([p1, p2][k] + 0.25 * s as f32);
                            let (lo, hi) = fl[k];
                            let f = lo * (hi / lo.max(1.0)).max(1.0).powf(m);
                            let fbv = if k == 0 { self.fb[s] } else { self.fb2[s] };
                            let st = if k == 0 { &mut self.ap[s] } else { &mut self.ap2[s] };
                            let y = allpass_chain(&mut st[..ord[k]], dry + fbv * fbk[k], rate, f);
                            if k == 0 { self.fb[s] = flush(y) } else { self.fb2[s] = flush(y) }
                            // Tone: left low-pass, right high-pass
                            self.svf[s][k].set(rate, if tone[k] < 0.0 { 18000.0 * (0.05f32).powf(-tone[k]) } else { 20.0 * (100.0f32).powf(tone[k]) }, 0.7);
                            let t = self.svf[s][k].run(0.5 * (dry + y));
                            o[k] = if tone[k] < 0.0 { t.low } else if tone[k] > 0.0 { t.high } else { 0.5 * (dry + y) };
                        }
                        *x = o[0] * (1.0 - mix) + o[1] * mix;
                    }
                }
            }
            Roswell => {
                let lin = self.p.idx(1) == 0;
                let base = g(self, 2);
                let f = if lin {
                    // 12 notes per octave on the knob: the frequency quantised to semitones
                    if base < 1.0 { 0.0 } else { 440.0 * 2f32.powf(((12.0 * (base / 440.0).log2()).round()) / 12.0) }
                } else {
                    base
                } * 2f32.powf(g(self, 3) / 1200.0);
                let (fbk, mix) = (g(self, 4) / 100.0 * 0.9, g(self, 5) / 100.0);
                for i in 0..n {
                    let c = sine01(self.ph[0].step(rate, f));
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let dry = *x;
                        let y = (dry + self.fb[s] * fbk) * c;
                        self.fb[s] = flush(y);
                        *x = dry * (1.0 - mix) + y * mix;
                    }
                }
            }
            SpinBox => {
                let speed = self.p.idx(6);
                let resp = 0.3 + 4.0 * g(self, 3) / 100.0;
                let fast = g(self, 2);
                let (ht, dt) = match speed {
                    0 => (0.8, 0.67),
                    1 => (0.0, 0.0),
                    _ => (fast, fast * 0.9),
                };
                let drive = db(24.0 * g(self, 4) / 100.0);
                let bright = self.p.on(5);
                let cab = self.p.idx(1);
                let r = self.rotor.as_mut().unwrap();
                r.spin(ht, dt, resp * 0.6, resp * 1.8, n);
                for i in 0..n {
                    let x = 0.5 * (left[i] + right[i]);
                    let x = if drive > 1.01 { (x * drive).tanh() / drive.sqrt() } else { x };
                    let (a, b) = r.run(x, [0.5, 0.7, 0.0, 0.9][cab], if bright { 1.2 } else { 0.9 }, 1.0);
                    left[i] = a;
                    right[i] = b;
                }
            }
            Vibe => {
                let ty = self.p.idx(4);
                let depth = g(self, 3) / 100.0 * [0.35, 0.65, 1.0][ty % 3];
                let chorus = ty >= 3;
                let hz = self.hz(2);
                for i in 0..n {
                    let ph = self.ph[0].step(rate, hz);
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let m = 0.5 + 0.5 * tri(ph + 0.12 * s as f32);
                        let dry = *x;
                        self.line[s].push(dry);
                        let y = self.line[s].read_lin((0.0004 + 0.0035 * depth * m) * rate);
                        *x = if chorus { 0.5 * (dry + y) * 1.3 } else { y };
                    }
                }
            }
            TremOTone => {
                let hz = self.hz(2);
                let depth = g(self, 3) / 100.0;
                for i in 0..n {
                    let gn = self.gain.next();
                    let m = 1.0 - depth * (0.5 + 0.5 * sine01(self.ph[0].step(rate, hz)));
                    left[i] *= m * gn;
                    right[i] *= m * gn;
                }
            }
            DrOctave => {
                let (o1, o2, direct, drive) = (g(self, 1) / 100.0, g(self, 2) / 100.0, g(self, 3) / 100.0, g(self, 4) / 100.0);
                for i in 0..n {
                    let x = 0.5 * (left[i] + right[i]);
                    // flip-flop dividers on the low-passed input's zero crossings
                    let lp = bqs(&mut self.bq[0], &[0, 1], x);
                    let env = self.env[0].run(lp.abs());
                    if (lp >= 0.0) != (self.misc[0] >= 0.0) && lp >= 0.0 {
                        self.misc[1] = -self.misc[1] + if self.misc[1] == 0.0 { 1.0 } else { 0.0 };
                        if self.misc[1] > 0.0 {
                            self.misc[2] = if self.misc[2] > 0.0 { -1.0 } else { 1.0 };
                        }
                    }
                    self.misc[0] = lp;
                    let sq1 = if self.misc[1] > 0.0 { 1.0 } else { -1.0 };
                    let sq2 = self.misc[2];
                    let oct = bqs(&mut self.bq[0], &[2, 3], (sq1 * o1 + sq2 * o2) * env * 2.0);
                    let mut y = x * direct + oct * (1.0 - direct * 0.5);
                    if drive > 0.0 {
                        let k = db(30.0 * drive);
                        y = (y * k).tanh() / k.sqrt();
                    }
                    left[i] = y;
                    right[i] = y;
                }
            }
            Wham => {
                let ratio = 2f32.powf(g(self, 1) / 100.0 * g(self, 2) / 12.0);
                let mix = g(self, 3) / 100.0;
                let sh = self.shift.as_mut().unwrap();
                for i in 0..n {
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let y = sh[s].run(*x, ratio);
                        *x = *x * (1.0 - mix) + y * mix;
                    }
                }
            }
            SpringBox => {
                let fbk = [0.72, 0.84, 0.92][self.p.idx(1)];
                let mix = g(self, 4) / 100.0;
                let lens = [0.0371, 0.0447];
                for i in 0..n {
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let dry = *x;
                        // dispersion: the chirp of a spring (a long all-pass cascade)
                        let mut y = dry + self.fb[s] * fbk;
                        for st in self.ap[s].iter_mut() {
                            let a = 0.62;
                            let o = -a * y + *st;
                            *st = flush(y + a * o);
                            y = o;
                        }
                        self.line[s].push(y);
                        let d = self.line[s].read_lin(lens[s] * rate);
                        // the tone filters in the loop; the Style colour only on the way out
                        // (a resonant peak inside the feedback would run away)
                        let d = bqs(&mut self.bq[s], &[0, 1], d);
                        self.fb[s] = flush(d * 0.98);
                        let d = self.bq[s][2].run(d);
                        *x = dry * (1.0 - 0.5 * mix) + d * mix * 1.4;
                    }
                }
            }
            TieDye => {
                let secs = if self.p.on(1) { g(self, 2) * 4.0 * 60.0 / self.bpm as f32 } else { g(self, 2) };
                let len = (secs * rate).clamp(256.0, 2.0 * rate) as usize;
                let fbk = g(self, 3) / 100.0 * 0.9;
                let mix = g(self, 6) / 100.0;
                let listen = self.p.on(7);
                for i in 0..n {
                    // read the last `len` samples backwards, cross-faded between two heads
                    self.pos = (self.pos + 1) % len;
                    let mut o = [0f32; 2];
                    for (s, ov) in o.iter_mut().enumerate() {
                        let mut y = 0.0;
                        for h in 0..2 {
                            let ph = (self.pos + h * len / 2) % len;
                            let w = (PI * ph as f32 / len as f32).sin();
                            y += self.line[s].read_lin((2 * ph) as f32 + 1.0) * w * w;
                        }
                        *ov = bqs(&mut self.bq[s], &[0, 1, 2], y);
                    }
                    for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                        let dry = *x;
                        self.line[s].push(if listen { 0.0 } else { dry } + o[s] * fbk);
                        *x = dry * (1.0 - mix * 0.5) + o[s] * mix;
                    }
                }
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
    }
}

impl Pedal {
    fn flange(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let g = |s: &Self, id: u32| s.p.get(id);
        let hz = self.hz(2);
        let depth = g(self, 3) / 100.0;
        let reso = g(self, 4) / 100.0 * 0.92;
        let mix = g(self, 5) / 100.0;
        let wave = self.p.idx(6);
        let sym = g(self, 7) / 100.0;
        let curve = g(self, 8) / 100.0;
        let manual = g(self, 9) * 0.001 * rate;
        for i in 0..left.len().min(right.len()) {
            let ph = self.ph[0].step(rate, hz);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                // the waveform, its symmetry (rise / fall split) and curve (rounded corners)
                let mut p = (ph + 0.25 * s as f32).fract();
                p = if p < sym.max(0.01) { 0.5 * p / sym.max(0.01) } else { 0.5 + 0.5 * (p - sym) / (1.0 - sym).max(0.01) };
                let mut m = match wave {
                    0 => sine01(p),
                    1 => tri(p),
                    _ => if p < 0.5 { 1.0 } else { -1.0 },
                };
                m = m * (1.0 - curve) + (m * 1.5).tanh() / (1.5f32).tanh() * curve;
                let d = (manual * (1.0 + 0.9 * depth * m)).max(1.0);
                let dry = *x;
                self.line[s].push(dry + self.fb[s] * reso);
                let y = bqs(&mut self.bq[s], &[0, 1], self.line[s].read_lin(d));
                self.fb[s] = flush(y);
                *x = dry * (1.0 - 0.5 * mix) + y * mix;
            }
        }
    }
}

plugin_impl!(Pedal);

/// Whether a pedal changes the stereo image (the modulation pedals, which Pedalboard's Router
/// can switch between mono and stereo).
pub fn is_modulation(k: Kind) -> bool {
    PEDALS.iter().any(|(_, kk, c)| *kk == k && *c == "Modulation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TAU;
    use lg_dsp::Plugin;

    fn sine(n: usize, hz: f32, a: f32) -> Vec<f32> {
        (0..n).map(|i| (TAU * hz * i as f32 / 48000.0).sin() * a).collect()
    }

    #[test]
    fn every_pedal_runs_and_stays_finite() {
        for (name, k, _) in PEDALS {
            let mut p = Pedal::new(*k, 48000.0);
            let mut l = sine(9600, 220.0, 0.5);
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 20.0), "{name}");
            assert_eq!(p.params()[0].name, "On", "{name}");
        }
    }

    #[test]
    fn off_switch_bypasses() {
        let mut p = Pedal::new(Kind::Rawk, 48000.0);
        p.set(0, 0.0);
        let mut l = sine(480, 220.0, 0.5);
        let want = l.clone();
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        assert_eq!(l, want);
    }

    #[test]
    fn fuzz_adds_harmonics_and_level_sets_output() {
        let mut p = Pedal::new(Kind::FuzzMachine, 48000.0);
        let mut l = sine(48000, 200.0, 0.3);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        // the third harmonic (600 Hz) is strong after clipping
        let h3: f32 = l[24000..].iter().enumerate().map(|(i, v)| v * (TAU * 600.0 * i as f32 / 48000.0).sin()).sum::<f32>().abs();
        assert!(h3 > 50.0, "{h3}");
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let a = rms(&l[24000..]);
        p.set(2, -18.0);
        let mut l2 = sine(48000, 200.0, 0.3);
        let mut r2 = l2.clone();
        p.reset();
        p.process(&mut l2, &mut r2);
        let b = rms(&l2[24000..]);
        assert!((lg_dsp::gain_to_db(a / b) - 10.0).abs() < 0.5);
    }

    #[test]
    fn graphic_eq_band_boost() {
        let mut p = Pedal::new(Kind::GraphicEq, 48000.0);
        p.set(4, 12.0); // 800 Hz
        let mut l = sine(48000, 800.0, 0.1);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let peak = l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((lg_dsp::gain_to_db(peak / 0.1) - 12.0).abs() < 0.3, "{peak}");
    }

    #[test]
    fn wham_shifts_up_an_octave() {
        let mut p = Pedal::new(Kind::Wham, 48000.0);
        p.set(1, 100.0);
        let mut l = sine(24000, 200.0, 0.5);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let f = crate::kit::yin(&l[12000..16096], 48000.0, 50.0, 2000.0).unwrap();
        assert!((f - 400.0).abs() < 10.0, "{f}");
    }

    #[test]
    fn dr_octave_goes_down() {
        let mut p = Pedal::new(Kind::DrOctave, 48000.0);
        p.set(3, 0.0); // octave only
        p.set(2, 0.0);
        p.set(4, 0.0);
        let mut l = sine(24000, 220.0, 0.5);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let f = crate::kit::yin(&l[12000..20000], 48000.0, 50.0, 1000.0).unwrap();
        assert!((f - 110.0).abs() < 4.0, "{f}");
    }

    #[test]
    fn squash_compresses() {
        let mut p = Pedal::new(Kind::Squash, 48000.0);
        p.set(1, 100.0);
        let peak = |a: f32, p: &mut Pedal| {
            p.reset();
            let mut l = sine(24000, 300.0, a);
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            l[12000..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        let loud = peak(0.9, &mut p);
        let soft = peak(0.09, &mut p);
        // 20 dB in, far less out
        assert!(lg_dsp::gain_to_db(loud / soft) < 10.0);
    }
}
