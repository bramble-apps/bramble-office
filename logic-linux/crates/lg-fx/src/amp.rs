//! Guitar and bass amp simulation: Amp Designer and Bass Amp Designer stand-ins (working names
//! "Amp Sculptor" and "Bass Amp Sculptor"), on one modelled amp engine.
//!
//! The chain, as a real amp: input coupling high-pass → two (guitar) or one (bass) preamp
//! triode stages with interstage filtering (asymmetric soft clipping, 4x oversampled) → the
//! model's tone stack (Bass / Mids / Treble: a Fender-style scooped stack, a British TMB stack
//! with forward mids, or a flatter boutique one) → power amp (Master drives a push-pull stage
//! with supply sag; Presence lifts the highs in its feedback loop) → cabinet (speaker
//! resonance, cone break-up roll-off and the cabinet's size colour) → microphone (its frequency
//! character; position X moves off the cone centre = darker, Z back = less proximity bass and a
//! touch of room) → effects (tremolo, spring reverb) → output.
//!
//! Each amp head has its own voicing (gain range, stage bias, stack, sag, bright cap), each
//! cabinet its own speaker model. Positions follow the research maps where they are anchored:
//! Amp Designer's knobs 1-6, Speaker 20, FX 24-25, Reverb 30-32 are value-confirmed; the model
//! menus' ORDER is ours (Logic's enumeration order is not decoded yet). Bass Amp Designer's
//! positions are mostly provisional (graphic EQ 32-38 and the two parametric mids 39-44 are the
//! clearest).

use crate::distortion::Over4;
use crate::util::{Biquad, Coefs, Delay, Follower, Phase, Values, db, flush, plugin_impl, sine01};
use lg_dsp::{Param, Unit};

pub const AMPS: &[&str] = &[
    "British Amp", "Tweed Amp", "Black Panel Amp", "Silver Panel Amp", "Brown Panel Amp", "Blues Blaster Amp", "Vintage British Amp", "Modern British Amp", "Brown Amp", "British Blues Amp", "Modern American Amp", "Studio Amp",
    "Boutique British Amp", "Sunshine Amp", "Stadium Amp", "Boutique Retro Amp", "High Octane Amp", "Turbo Amp", "Pawnshop Amp", "Clean Preamp",
];
pub const EQ_MODELS: &[&str] = &["British Bright", "US Classic", "Boutique", "Custom EQ Model"];
pub const SPEAKERS: &[&str] = &[
    "British 1x12", "British 2x12", "Tweed 1x12", "Tweed 4x10", "Tweed 1x10", "Black Panel 4x10", "Silver Panel 2x12", "Black Panel 1x10", "Brown Panel 1x12", "Brown Panel 1x15", "Vintage British 4x12", "Modern British 4x12", "Brown 4x12",
    "British Blues 2x12", "Modern American 4x12", "Studio 1x12", "Boutique British 2x12", "Sunshine 4x12", "Sunshine 1x12", "Stadium 4x12", "Stadium 2x12", "Boutique Retro 2x12", "High Octane 4x12", "Turbo 4x12", "Pawnshop 1x8", "DI Box",
];
pub const MICS: &[&str] = &["Ribbon 121", "Condenser 414", "Dynamic 57", "Dynamic 609"];
const REVERBS: &[&str] = &["Vintage Spring", "Simple Spring", "Mellow Spring", "Bright Spring", "Dark Spring", "Resonant Spring", "Boutique Spring", "Sweet Reverb", "Rich Reverb", "Warm Reverb"];

/// A head's voicing.
#[derive(Clone, Copy)]
pub(crate) struct Voice {
    /// preamp gain range (dB at Gain 10)
    gain_db: f32,
    /// stage asymmetry (0 = symmetric)
    bias: f32,
    /// number of preamp stages driven hard (1 or 2)
    stages: usize,
    /// tone stack: 0 scooped (US), 1 mid-forward (British), 2 flat (boutique)
    stack: usize,
    /// power-supply sag (0..1) and power stage hardness
    sag: f32,
    hard: f32,
    /// bright cap: treble added at low Gain settings (dB)
    bright: f32,
}

pub(crate) const VOICES: [Voice; 20] = [
    Voice { gain_db: 34.0, bias: 0.25, stages: 2, stack: 1, sag: 0.3, hard: 0.5, bright: 3.0 },
    Voice { gain_db: 30.0, bias: 0.35, stages: 1, stack: 0, sag: 0.6, hard: 0.3, bright: 2.0 },
    Voice { gain_db: 24.0, bias: 0.15, stages: 1, stack: 0, sag: 0.25, hard: 0.4, bright: 4.0 },
    Voice { gain_db: 26.0, bias: 0.18, stages: 1, stack: 0, sag: 0.3, hard: 0.4, bright: 3.5 },
    Voice { gain_db: 30.0, bias: 0.25, stages: 1, stack: 0, sag: 0.45, hard: 0.35, bright: 2.5 },
    Voice { gain_db: 34.0, bias: 0.3, stages: 2, stack: 0, sag: 0.4, hard: 0.45, bright: 2.0 },
    Voice { gain_db: 36.0, bias: 0.25, stages: 2, stack: 1, sag: 0.35, hard: 0.55, bright: 3.0 },
    Voice { gain_db: 46.0, bias: 0.15, stages: 2, stack: 1, sag: 0.2, hard: 0.7, bright: 1.5 },
    Voice { gain_db: 42.0, bias: 0.2, stages: 2, stack: 1, sag: 0.3, hard: 0.6, bright: 1.5 },
    Voice { gain_db: 32.0, bias: 0.3, stages: 2, stack: 1, sag: 0.45, hard: 0.4, bright: 2.0 },
    Voice { gain_db: 50.0, bias: 0.12, stages: 2, stack: 0, sag: 0.15, hard: 0.8, bright: 1.0 },
    Voice { gain_db: 38.0, bias: 0.2, stages: 2, stack: 2, sag: 0.25, hard: 0.5, bright: 2.0 },
    Voice { gain_db: 36.0, bias: 0.28, stages: 2, stack: 2, sag: 0.4, hard: 0.45, bright: 3.0 },
    Voice { gain_db: 40.0, bias: 0.22, stages: 2, stack: 1, sag: 0.3, hard: 0.6, bright: 2.0 },
    Voice { gain_db: 48.0, bias: 0.15, stages: 2, stack: 1, sag: 0.2, hard: 0.75, bright: 1.0 },
    Voice { gain_db: 34.0, bias: 0.3, stages: 2, stack: 2, sag: 0.45, hard: 0.4, bright: 2.5 },
    Voice { gain_db: 52.0, bias: 0.1, stages: 2, stack: 1, sag: 0.15, hard: 0.85, bright: 0.5 },
    Voice { gain_db: 54.0, bias: 0.1, stages: 2, stack: 0, sag: 0.1, hard: 0.9, bright: 0.5 },
    Voice { gain_db: 28.0, bias: 0.4, stages: 1, stack: 2, sag: 0.7, hard: 0.3, bright: 1.0 },
    Voice { gain_db: 18.0, bias: 0.05, stages: 1, stack: 2, sag: 0.0, hard: 0.1, bright: 0.0 },
];

/// A cabinet: low resonance (Hz, Q), roll-off corner (Hz), a mid colour (Hz, dB).
#[derive(Clone, Copy)]
pub(crate) struct Cab {
    pub(crate) res: f32,
    pub(crate) res_q: f32,
    pub(crate) top: f32,
    pub(crate) colour: f32,
    pub(crate) colour_db: f32,
}

pub(crate) fn cab_for(name: &str) -> Cab {
    let n = name;
    let (res, top) = if n.contains("4x12") {
        (95.0, 5000.0)
    } else if n.contains("2x12") {
        (105.0, 5400.0)
    } else if n.contains("4x10") {
        (120.0, 6000.0)
    } else if n.contains("1x15") {
        (75.0, 4200.0)
    } else if n.contains("1x10") || n.contains("1x8") {
        (150.0, 5600.0)
    } else if n.contains("DI") {
        (30.0, 12000.0)
    } else {
        (115.0, 5600.0)
    };
    let (colour, colour_db) = if n.starts_with("British") || n.starts_with("Vintage British") {
        (2400.0, 3.0)
    } else if n.starts_with("Tweed") || n.starts_with("Brown Panel") {
        (1200.0, 2.0)
    } else if n.contains("Black Panel") || n.contains("Silver") {
        (3000.0, 2.0)
    } else if n.contains("Modern") || n.contains("High Octane") || n.contains("Turbo") || n.contains("Stadium") {
        (2000.0, -2.5)
    } else {
        (1800.0, 1.5)
    };
    Cab { res, res_q: if n.contains("DI") { 0.5 } else { 1.6 }, top, colour, colour_db }
}

/// The modelled amp (guitar or bass voicing), stereo-linked (one model per channel).
pub(crate) struct AmpCore {
    rate: f32,
    os: [[Over4; 3]; 2],
    inter: [[Biquad; 2]; 2],
    input_hp: [Biquad; 2],
    stack: [[Biquad; 4]; 2],
    presence: [Biquad; 2],
    cab: [[Biquad; 5]; 2],
    mic: [[Biquad; 3]; 2],
    pub(crate) sag: Follower,
    pub(crate) pre_gain: f32,
    master: f32,
    voice: Voice,
    dc: [[f32; 3]; 2],
    dck: f32,
}

impl AmpCore {
    pub(crate) fn new(rate: f32) -> AmpCore {
        AmpCore {
            rate,
            os: [[Over4::new(rate); 3]; 2],
            inter: [[Biquad::default(); 2]; 2],
            input_hp: [Biquad::default(); 2],
            stack: [[Biquad::default(); 4]; 2],
            presence: [Biquad::default(); 2],
            cab: [[Biquad::default(); 5]; 2],
            mic: [[Biquad::default(); 3]; 2],
            sag: Follower::new(rate, 15.0, 250.0),
            pre_gain: 1.0,
            master: 1.0,
            voice: VOICES[0],
            dc: [[0.0; 3]; 2],
            dck: 1.0 - (-crate::util::TAU * 12.0 / rate).exp(),
        }
    }

    /// Sets the amp. Knobs are 0..10.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set(&mut self, voice: Voice, gain: f32, bass: f32, mids: f32, treble: f32, presence: f32, master: f32, stack: usize, cab: Cab, mic: usize, mic_x: f32, mic_z: f32, low_hp: f32) {
        let r = self.rate;
        self.voice = voice;
        self.pre_gain = db(gain / 10.0 * voice.gain_db - 6.0);
        self.master = db((master / 10.0) * 24.0 - 12.0);
        let bright = voice.bright * (1.0 - gain / 10.0);
        let b = (bass - 5.0) / 5.0;
        let m = (mids - 5.0) / 5.0;
        let t = (treble - 5.0) / 5.0;
        // the stacks: passive stacks lose level and interact; approximated with shelves/peaks
        let (mid_f, mid_base, bass_f, treb_f) = match stack {
            0 => (500.0, -6.0, 120.0, 2500.0),
            1 => (700.0, -1.5, 150.0, 2800.0),
            _ => (800.0, -3.0, 110.0, 3200.0),
        };
        let stack_c = [
            Coefs::low_shelf(r, bass_f, 0.7, b * 10.0),
            Coefs::peak(r, mid_f, 0.8, mid_base * (1.0 - (mids / 10.0)) + m * 6.0),
            Coefs::high_shelf(r, treb_f, 0.7, t * 10.0 + bright),
            Coefs::highpass(r, low_hp, 0.6),
        ];
        let pres = Coefs::high_shelf(r, 3500.0, 0.8, presence / 10.0 * 8.0 - 2.0);
        let mut cab_c = [
            Coefs::highpass(r, cab.res * 0.8, 0.6),
            Coefs::peak(r, cab.res, cab.res_q, 4.0),
            Coefs::lowpass(r, cab.top.min(r * 0.45), 0.9),
            Coefs::lowpass(r, (cab.top * 1.3).min(r * 0.45), 0.6),
            Coefs::peak(r, cab.colour, 1.0, cab.colour_db),
        ];
        if cab.res < 40.0 {
            cab_c[1] = Coefs::THRU;
        }
        // microphones: (presence peak Hz, dB, top roll-off Hz); X off-axis darkens, Z adds space
        let (pk, pk_db, top) = match mic {
            0 => (1200.0, 1.5, 7000.0),
            1 => (6000.0, 2.0, 16000.0),
            2 => (4500.0, 4.0, 11000.0),
            _ => (2500.0, 3.0, 12000.0),
        };
        let off = mic_x.clamp(0.0, 1.0);
        let mic_c = [Coefs::peak(r, pk, 1.0, pk_db * (1.0 - off * 0.5)), Coefs::lowpass(r, (top * (1.0 - 0.55 * off)).min(r * 0.45), 0.707), Coefs::low_shelf(r, 200.0, 0.7, 3.0 * (1.0 - mic_z.clamp(0.0, 1.0)) - 1.5)];
        for s in 0..2 {
            for i in 0..4 {
                self.stack[s][i].c = stack_c[i];
            }
            self.presence[s].c = pres;
            for i in 0..5 {
                self.cab[s][i].c = cab_c[i];
            }
            for i in 0..3 {
                self.mic[s][i].c = mic_c[i];
            }
            self.input_hp[s].c = Coefs::highpass(r, 70.0, 0.6);
            self.inter[s][0].c = Coefs::lowpass(r, 9000.0, 0.6);
            self.inter[s][1].c = Coefs::highpass(r, 120.0, 0.6);
        }
    }

    pub(crate) fn clear(&mut self) {
        for s in 0..2 {
            for o in self.os[s].iter_mut() {
                o.clear();
            }
            for b in self.inter[s].iter_mut().chain(self.stack[s].iter_mut()).chain(self.cab[s].iter_mut()).chain(self.mic[s].iter_mut()) {
                b.clear();
            }
            self.input_hp[s].clear();
            self.presence[s].clear();
        }
        self.dc = [[0.0; 3]; 2];
        self.sag.env = 0.0;
    }

    /// One sample of one channel (`sag` shared, updated by the caller).
    #[inline]
    pub(crate) fn run(&mut self, s: usize, x: f32, sag_amt: f32, cab_on: bool) -> f32 {
        let v = self.voice;
        let bias = v.bias;
        let tri = |u: f32| -> f32 {
            // triode: asymmetric soft clip (positive side squashes sooner)
            let u = u + bias;
            let y = if u > 0.0 { u / (1.0 + u * 1.3).powf(0.77) } else { (u * 0.8).tanh() / 0.8 };
            y - bias / (1.0 + bias * 1.3).powf(0.77)
        };
        let mut y = self.input_hp[s].run(x) * self.pre_gain;
        y = self.os[s][0].run(y, tri);
        if v.stages > 1 {
            let i0 = self.inter[s][0].run(y);
            y = self.inter[s][1].run(i0) * 6.0;
            y = self.os[s][1].run(y, tri);
        }
        self.dc[s][0] = flush(self.dc[s][0] + self.dck * (y - self.dc[s][0]));
        y -= self.dc[s][0];
        for b in self.stack[s].iter_mut() {
            y = b.run(y);
        }
        // power amp: push-pull (symmetric), harder as Master rises, softened by sag
        let drive = self.master * (1.0 - v.sag * sag_amt * 0.5);
        let hard = v.hard;
        y = self.presence[s].run(y);
        y = self.os[s][2].run(y * drive, |u| {
            let soft = u.tanh();
            let firm = u / (1.0 + u.abs().powf(4.0)).powf(0.25);
            soft * (1.0 - hard) + firm * hard
        }) / drive.max(0.3).sqrt();
        if cab_on {
            for b in self.cab[s].iter_mut() {
                y = b.run(y);
            }
            for b in self.mic[s].iter_mut() {
                y = b.run(y);
            }
        }
        y
    }
}

/// A short spring reverb: a dispersive all-pass chain (the chirp) in a damped feedback loop.
pub(crate) struct Spring {
    ap: [[(Delay, f32); 4]; 2],
    loop_d: [Delay; 2],
    lp: [crate::util::OnePole; 2],
    fb: f32,
}

impl Spring {
    pub(crate) fn new(rate: f32) -> Spring {
        let mk = |ms: f32| (Delay::new((ms * 0.001 * rate) as usize + 4), ms * 0.001 * rate);
        Spring {
            ap: [[mk(2.1), mk(3.7), mk(5.3), mk(7.9)], [mk(2.3), mk(3.9), mk(5.9), mk(8.3)]],
            loop_d: [Delay::new((0.06 * rate) as usize), Delay::new((0.06 * rate) as usize)],
            lp: [crate::util::OnePole::new(rate, 4500.0); 2],
            fb: 0.75,
        }
    }
    pub(crate) fn set(&mut self, rate: f32, kind: usize) {
        let (cut, fb) = match kind {
            2 | 4 => (2800.0, 0.72),
            3 => (7000.0, 0.72),
            5 => (4500.0, 0.85),
            7..=9 => (6000.0, 0.82),
            _ => (4500.0, 0.76),
        };
        self.fb = fb;
        for l in self.lp.iter_mut() {
            l.set(rate, cut);
        }
    }
    pub(crate) fn clear(&mut self) {
        for c in self.ap.iter_mut() {
            for (d, _) in c.iter_mut() {
                d.clear();
            }
        }
        for d in self.loop_d.iter_mut() {
            d.clear();
        }
    }
    #[inline]
    pub(crate) fn run(&mut self, s: usize, x: f32, rate: f32) -> f32 {
        let l = self.loop_d[s].read(0.033 * rate * if s == 1 { 1.13 } else { 1.0 });
        let mut y = x + l * self.fb;
        for (d, len) in self.ap[s].iter_mut() {
            let z = d.read(*len);
            let v = y + 0.6 * z;
            d.push(v);
            y = z - 0.6 * v;
        }
        let y = self.lp[s].lp(y);
        self.loop_d[s].push(y);
        y
    }
}

// ---------------------------------------------------------------------------------------------
// Amp Designer

static AMP_PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    vec![
        Param::new(1, "Gain", 0.0, 10.0, 4.5, Unit::None),
        Param::new(2, "Bass", 0.0, 10.0, 6.7, Unit::None),
        Param::new(3, "Mids", 0.0, 10.0, 3.1, Unit::None),
        Param::new(4, "Treble", 0.0, 10.0, 7.4, Unit::None),
        Param::new(5, "Presence", 0.0, 10.0, 8.3, Unit::None),
        Param::new(6, "Master", 0.0, 10.0, 9.4, Unit::None),
        Param::new(7, "Output Level", -40.0, 12.0, -7.0, Unit::Db),
        Param::new(9, "FX Speed", 0.0, 10.0, 5.0, Unit::None),
        Param::menu(18, "Amp", AMPS, 0.0),
        Param::menu(19, "EQ Model", EQ_MODELS, 0.0),
        Param::menu(20, "Speaker", SPEAKERS, 1.0),
        Param::menu(21, "Microphone Type", MICS, 0.0),
        Param::new(22, "Mic Position X", 0.0, 1.0, 0.0, Unit::None),
        Param::new(23, "Mic Position Z", 0.0, 1.0, 0.0, Unit::None),
        Param::new(24, "FX Enable", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(25, "FX Depth", 0.0, 10.0, 7.7, Unit::None),
        Param::menu(28, "FX Type", &["Tremolo", "Vibrato"], 0.0),
        Param::new(30, "Reverb Enable", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(31, "Reverb Type", REVERBS, 8.0),
        Param::new(32, "Reverb Level", 0.0, 10.0, 2.2, Unit::None),
        // (our own id) the Sync/Free switch: synced, Speed steps through note values from four
        // bars to 1/64 (straight, triplet, dotted) at the project tempo
        Param::new(1000, "FX Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    ]
});

/// Amp Designer's modulation speed: free (Hz from the Speed knob) or synced to a note value.
fn amp_fx_hz(knob: f32, sync: bool, bpm: f64) -> f32 {
    if sync {
        let idx = 26.0 - (knob.clamp(0.0, 10.0) / 10.0 * 24.0).round();
        crate::util::beats_hz(crate::util::sync_index_beats(-idx), bpm)
    } else {
        0.5 + knob * 0.9
    }
}

pub struct AmpDesigner {
    p: Values,
    rate: f32,
    core: AmpCore,
    spring: Spring,
    fx_ph: Phase,
    vib: [Delay; 2],
    out: f32,
    bpm: f64,
}

impl AmpDesigner {
    pub fn new(rate: f32) -> AmpDesigner {
        let mut a = AmpDesigner { p: Values::new(&AMP_PARAMS), rate, core: AmpCore::new(rate), spring: Spring::new(rate), fx_ph: Phase::default(), vib: [Delay::new((0.02 * rate) as usize), Delay::new((0.02 * rate) as usize)], out: 1.0, bpm: 120.0 };
        a.update();
        a
    }
    fn update(&mut self) {
        let p = &self.p;
        let voice = VOICES[p.idx(18).min(VOICES.len() - 1)];
        let stack = match p.idx(19) {
            0 => 1,
            1 => 0,
            2 => 2,
            _ => voice.stack,
        };
        let cab = cab_for(SPEAKERS[p.idx(20).min(SPEAKERS.len() - 1)]);
        self.core.set(voice, p.get(1), p.get(2), p.get(3), p.get(4), p.get(5), p.get(6), stack, cab, p.idx(21), p.get(22), p.get(23), 70.0);
        self.spring.set(self.rate, p.idx(31));
        // the models are hot; Output Level -7 dB (the default) brings a typical amp near unity
        self.out = db(p.get(7) - 10.0);
    }
    fn clear(&mut self) {
        self.core.clear();
        self.spring.clear();
        for d in self.vib.iter_mut() {
            d.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        if self.p.on(30) { 2.0 } else { 0.1 }
    }
}

impl lg_dsp::Effect for AmpDesigner {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let fx = self.p.on(24);
        let depth = self.p.get(25) / 10.0;
        let speed = amp_fx_hz(self.p.get(9), self.p.on(1000), self.bpm);
        let vibrato = self.p.idx(28) == 1;
        let rev = if self.p.on(30) { self.p.get(32) / 10.0 } else { 0.0 };
        for i in 0..left.len().min(right.len()) {
            // a guitar amp is mono: both sides through the same model, kept separate for
            // stereo sources
            let lvl = left[i].abs().max(right[i].abs()) * self.core.pre_gain;
            let sag = self.core.sag.run(lvl.min(4.0)) / 4.0;
            let ph = if fx { self.fx_ph.step(rate, speed) } else { 0.0 };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut y = self.core.run(s, *x, sag, true);
                if fx {
                    if vibrato {
                        self.vib[s].push(y);
                        y = self.vib[s].read_lin((0.004 + 0.003 * depth * sine01(ph)) * rate);
                    } else {
                        y *= 1.0 - depth * (0.5 + 0.5 * sine01(ph));
                    }
                }
                if rev > 0.0 {
                    y += self.spring.run(s, y, rate) * rev * 0.8;
                }
                *x = y * self.out;
            }
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
}

plugin_impl!(AmpDesigner);

// ---------------------------------------------------------------------------------------------
// Bass Amp Designer

const BASS_AMPS: &[&str] = &["Modern Amp", "Classic Amp", "Flip-Top Amp", "Direct Box", "Customized"];
const BASS_CABS: &[&str] = &["Modern 15\"", "Modern 10\"", "Modern 6\"", "Modern Distant", "Classic 8x10\"", "Flip Top 1x15\"", "Direct (PowerAmp Out)", "Direct (PreAmp Out)"];
const BASS_MICS: &[&str] = &["Condenser 87", "Dynamic 20", "Dynamic 421"];
const GEQ_BASS: [f32; 7] = [40.0, 100.0, 250.0, 625.0, 1560.0, 3900.0, 6600.0];
const GEQ_BASS_NAMES: [&str; 7] = ["40 Hz", "100 Hz", "250 Hz", "625 Hz", "1560 Hz", "3900 Hz", "6600 Hz"];

static BASS_PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::new(2, "Compressor", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(3, "EQ", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(6, "Compressor Threshold", -40.0, 0.0, -18.0, Unit::Db),
        Param::new(10, "Gain", 0.0, 10.0, 3.2, Unit::None),
        Param::new(12, "Bass", 0.0, 10.0, 3.4, Unit::None),
        Param::new(14, "Mids", 0.0, 10.0, 6.7, Unit::None),
        Param::new(16, "Treble", 0.0, 10.0, 6.7, Unit::None),
        Param::new(20, "Master", 0.0, 10.0, 4.5, Unit::None),
    ];
    for i in 0..7 {
        v.push(Param::new(32 + i as u32, GEQ_BASS_NAMES[i], -15.0, 15.0, [5.6, 3.6, 0.4, 3.0, 5.2, 5.6, 6.2][i], Unit::Db));
    }
    v.extend([
        Param::new(39, "LMF Gain", -15.0, 15.0, 2.1, Unit::Db),
        Param::new(40, "LMF Freq", 100.0, 2000.0, 742.49, Unit::Hz),
        Param::new(41, "LMF Q", 0.1, 10.0, 1.2875, Unit::Ratio),
        Param::new(42, "HMF Gain", -15.0, 15.0, 1.6, Unit::Db),
        Param::new(43, "HMF Freq", 500.0, 8000.0, 2576.89, Unit::Hz),
        Param::new(44, "HMF Q", 0.1, 10.0, 1.0183, Unit::Ratio),
        Param::menu(45, "Graph/Par EQ", &["Graphic", "Parametric"], 0.0),
        Param::menu(46, "Amp", BASS_AMPS, 1.0),
        Param::menu(47, "Speaker", BASS_CABS, 1.0),
        Param::menu(48, "Microphone Type", BASS_MICS, 0.0),
        Param::new(63, "Amp/DI Mix", 0.0, 100.0, 0.0, Unit::Percent),
        Param::new(64, "Output Gain", -40.0, 12.0, 4.5, Unit::Db),
        // (our own id) where the additional EQ sits: before (default) or after the compressor
        Param::menu(1000, "EQ Position", &["Pre", "Post"], 0.0),
        // (our own ids) the compressor's Hard / Soft algorithm and its Gain relative to the
        // always-on AutoGain
        Param::menu(1001, "Compressor Mode", &["Hard", "Soft"], 1.0),
        Param::new(1002, "Compressor Gain", -12.0, 12.0, 0.0, Unit::Db),
        // (our own ids) channel 2, the D.I. box: Boost (input gain), HF Cut (noise filter) and
        // Tone (six preset curves, with its on/off button)
        Param::new(1003, "D.I. Boost", 0.0, 20.0, 0.0, Unit::Db),
        Param::new(1004, "D.I. HF Cut", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(1005, "D.I. Tone On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(1006, "D.I. Tone", &["1", "2", "3", "4", "5", "6"], 0.0),
        // (our own ids) the amp's Channel switch (I active 0 dB, II passive -15 dB) and Bright
        Param::menu(1007, "Channel", &["I", "II"], 0.0),
        Param::new(1008, "Bright", 0.0, 1.0, 0.0, Unit::OnOff),
    ]);
    v
});

pub struct BassAmp {
    p: Values,
    rate: f32,
    core: AmpCore,
    eq: [[Biquad; 7]; 2],
    comp: Follower,
    out: f32,
    /// The D.I. box: HF Cut plus the Tone curve's three sections, per side.
    di: [[Biquad; 4]; 2],
    /// The Bright switch's lift of the highs and upper mids before the pre-amp.
    bright: [Biquad; 2],
}

/// Bass Amp Designer's compressor gain for a detected level `e` (dB): Hard about 8:1, Soft
/// 2.5:1; AutoGain (always on) gives back about half of what it takes from a full-scale
/// signal, and Gain (`trim`, dB) adds or removes gain relative to that.
fn bass_comp(e: f32, thr: f32, hard: bool, trim: f32) -> f32 {
    let slope = if hard { 0.875 } else { 0.6 };
    db(-((e - thr).max(0.0) * slope) - thr * slope * 0.5 + trim)
}

/// The D.I. box's six Tone curves as up to three filter sections.
fn di_tone(r: f32, tone: usize) -> [Coefs; 3] {
    let t = Coefs::THRU;
    match tone {
        // -6 dB scoop over 100 Hz-10 kHz, deepest near 800 Hz
        0 => [Coefs::peak(r, 800.0, 0.35, -6.0), t, t],
        // a deep (-24 dB) V-shaped notch centred near 800 Hz
        1 => [Coefs::peak(r, 800.0, 0.45, -24.0), t, t],
        // a gentle -3 dB scoop, deepest around 800-1200 Hz
        2 => [Coefs::peak(r, 1000.0, 0.35, -3.0), t, t],
        // -3 dB near 8 kHz, a slight lift from 60 Hz to 1 kHz, +3 dB above 10 kHz
        3 => [Coefs::peak(r, 8000.0f32.min(r * 0.4), 1.2, -3.0), Coefs::peak(r, 250.0, 0.5, 1.5), Coefs::high_shelf(r, 12000.0f32.min(r * 0.45), 0.7, 3.0)],
        // a ramp from -24 dB at 10 Hz up to +3 dB near 900 Hz
        4 => [Coefs::highpass(r, 55.0, 0.6), Coefs::peak(r, 900.0, 0.5, 3.0), t],
        // the same ramp, rolling off about 6 dB between 10 and 20 kHz
        _ => [Coefs::highpass(r, 55.0, 0.6), Coefs::peak(r, 900.0, 0.5, 3.0), Coefs::high_shelf(r, 14000.0f32.min(r * 0.45), 0.7, -6.0)],
    }
}

impl BassAmp {
    pub fn new(rate: f32) -> BassAmp {
        let mut b = BassAmp { p: Values::new(&BASS_PARAMS), rate, core: AmpCore::new(rate), eq: [[Biquad::default(); 7]; 2], comp: Follower::new(rate, 10.0, 200.0), out: 1.0, di: [[Biquad::default(); 4]; 2], bright: [Biquad::default(); 2] };
        b.update();
        b
    }
    fn update(&mut self) {
        let (p, r) = (&self.p, self.rate);
        let amp = p.idx(46);
        // bass voicings: clean headroom, one stage; Flip-Top tube warmth; Direct box: no amp colour
        let mut voice = Voice { gain_db: 26.0, bias: 0.15, stages: 1, stack: 2, sag: 0.25, hard: 0.3, bright: 1.0 };
        match amp {
            1 => voice = Voice { gain_db: 30.0, bias: 0.25, sag: 0.45, stack: 1, ..voice },
            2 => voice = Voice { gain_db: 28.0, bias: 0.35, sag: 0.6, stack: 0, ..voice },
            3 => voice = Voice { gain_db: 12.0, bias: 0.0, sag: 0.0, hard: 0.05, bright: 0.0, ..voice },
            _ => {}
        }
        let cab_name = BASS_CABS[p.idx(47).min(BASS_CABS.len() - 1)];
        let cab = if cab_name.starts_with("Direct") {
            Cab { res: 30.0, res_q: 0.5, top: 12000.0, colour: 1000.0, colour_db: 0.0 }
        } else if cab_name.contains("15") {
            Cab { res: 60.0, res_q: 1.4, top: 3200.0, colour: 800.0, colour_db: 1.5 }
        } else if cab_name.contains("8x10") {
            Cab { res: 70.0, res_q: 1.3, top: 3800.0, colour: 600.0, colour_db: 2.5 }
        } else if cab_name.contains("6") {
            Cab { res: 120.0, res_q: 1.2, top: 7000.0, colour: 2000.0, colour_db: 2.0 }
        } else {
            Cab { res: 75.0, res_q: 1.4, top: 4500.0, colour: 1500.0, colour_db: 1.5 }
        };
        let mic = match p.idx(48) {
            0 => 1,
            1 => 0,
            _ => 3,
        };
        self.core.set(voice, p.get(10), p.get(12), p.get(14), p.get(16), 5.0, p.get(20), voice.stack, cab, mic, 0.2, 0.3, 30.0);
        for s in 0..2 {
            for i in 0..7 {
                self.eq[s][i].c = if !p.on(3) {
                    Coefs::THRU
                } else if p.idx(45) == 0 {
                    Coefs::peak(r, GEQ_BASS[i].min(r * 0.45), 1.2, p.get(32 + i as u32))
                } else if i == 0 {
                    Coefs::peak(r, p.get(40), p.get(41), p.get(39))
                } else if i == 1 {
                    Coefs::peak(r, p.get(43).min(r * 0.45), p.get(44), p.get(42))
                } else {
                    Coefs::THRU
                };
            }
        }
        self.out = db(p.get(64) - 6.0);
        // Hard: fast and firm; Soft: slow attack, long release (more sustain)
        if p.idx(1001) == 0 {
            self.comp.set(r, 2.0, 120.0);
        } else {
            self.comp.set(r, 30.0, 400.0);
        }
        let br = if p.on(1008) { Coefs::high_shelf(r, 1500.0, 0.7, 6.0) } else { Coefs::THRU };
        self.bright[0].c = br;
        self.bright[1].c = br;
        let hf = if p.on(1004) { Coefs::lowpass(r, 6000.0f32.min(r * 0.45), 0.707) } else { Coefs::THRU };
        let tone = if p.on(1005) { di_tone(r, p.idx(1006)) } else { [Coefs::THRU; 3] };
        for s in 0..2 {
            self.di[s][0].c = hf;
            for k in 0..3 {
                self.di[s][k + 1].c = tone[k];
            }
        }
    }
    fn clear(&mut self) {
        self.core.clear();
        for b in self.eq.iter_mut().flatten() {
            b.clear();
        }
        self.comp.env = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl lg_dsp::Effect for BassAmp {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let comp_on = self.p.on(2);
        let thr = self.p.get(6);
        let di = self.p.get(63) / 100.0;
        let hard = self.p.idx(1001) == 0;
        let trim = self.p.get(1002);
        let boost = db(self.p.get(1003));
        let chan = if self.p.idx(1007) == 1 { db(-15.0) } else { 1.0 };
        let post = self.p.idx(1000) == 1;
        for i in 0..left.len().min(right.len()) {
            // Pre: the EQ shapes what the compressor hears; Post: it shapes the compressed signal
            let mut pre = [self.bright[0].run(left[i] * chan), self.bright[1].run(right[i] * chan)];
            if !post {
                for (s, v) in pre.iter_mut().enumerate() {
                    for b in self.eq[s].iter_mut() {
                        *v = b.run(*v);
                    }
                }
            }
            let lvl = pre[0].abs().max(pre[1].abs());
            let raw = left[i].abs().max(right[i].abs());
            let sag = self.core.sag.run((raw * self.core.pre_gain).min(4.0)) / 4.0;
            // the pre-amp compressor: about 4:1 above its threshold
            let g = if comp_on {
                let e = lg_dsp::gain_to_db(self.comp.run(lvl).max(1e-6));
                bass_comp(e, thr, hard, trim)
            } else {
                1.0
            };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut v = pre[s] * g;
                if post {
                    for b in self.eq[s].iter_mut() {
                        v = b.run(v);
                    }
                }
                let amp = self.core.run(s, v, sag, true);
                // channel 2: the D.I. box on the dry input
                let mut d = if di > 0.0 { (*x * boost * 1.2).tanh() / 1.2 } else { 0.0 };
                if di > 0.0 {
                    for b in self.di[s].iter_mut() {
                        d = b.run(d);
                    }
                }
                *x = (amp * (1.0 - di) + d * di) * self.out;
            }
        }
    }
}

plugin_impl!(BassAmp);

#[cfg(test)]
mod fx_sync_tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    #[test]
    fn bass_di_box_and_compressor_controls() {
        let tone = |f: f32, a: f32| -> Vec<f32> { (0..24000).map(|i| a * (crate::util::TAU * f * i as f32 / 48000.0).sin()).collect() };
        let rms = |v: &[f32]| (v[12000..].iter().map(|x| x * x).sum::<f32>() / 12000.0).sqrt();
        let run = |set: &[(u32, f32)], x: &[f32]| {
            let mut b = BassAmp::new(48000.0);
            for &(id, v) in set {
                b.set(id, v);
            }
            let (mut l, mut r) = (x.to_vec(), x.to_vec());
            b.process(&mut l, &mut r);
            l
        };
        // Blend fully right: the D.I. box alone, clean at a low level
        let x = tone(800.0, 0.05);
        let di = [(63, 100.0), (2, 0.0)];
        let flat = rms(&run(&di, &x));
        // Tone 2 notches 800 Hz deeply, Tone 1 a little, and only with Tone on
        let t2 = rms(&run(&[di[0], di[1], (1005, 1.0), (1006, 1.0)], &x));
        let t1 = rms(&run(&[di[0], di[1], (1005, 1.0), (1006, 0.0)], &x));
        let off = rms(&run(&[di[0], di[1], (1005, 0.0), (1006, 1.0)], &x));
        assert!(lg_dsp::gain_to_db(t2 / flat) < -18.0, "{}", lg_dsp::gain_to_db(t2 / flat));
        let g1 = lg_dsp::gain_to_db(t1 / flat);
        assert!(g1 < -4.0 && g1 > -8.0, "{g1}");
        assert!((off - flat).abs() < 1e-4);
        // Tone 5: -24 dB region at very low frequencies
        let lo = tone(12.0, 0.05);
        let t5 = rms(&run(&[di[0], di[1], (1005, 1.0), (1006, 4.0)], &lo)) / rms(&run(&di, &lo));
        assert!(lg_dsp::gain_to_db(t5) < -18.0);
        // Boost raises the input gain; HF Cut takes the highs down
        assert!(rms(&run(&[di[0], di[1], (1003, 12.0)], &x)) > 3.0 * flat);
        let hi = tone(12000.0, 0.05);
        assert!(rms(&run(&[di[0], di[1], (1004, 1.0)], &hi)) < 0.5 * rms(&run(&di, &hi)));
        // compressor: Hard squeezes more than Soft; AutoGain lifts what sits under the
        // threshold; Gain trims relative to it
        let red = |hard| lg_dsp::gain_to_db(bass_comp(0.0, -30.0, hard, 0.0) / bass_comp(-30.0, -30.0, hard, 0.0));
        assert!(red(true) < -25.0 && red(false) > -20.0, "{} {}", red(true), red(false));
        assert!(lg_dsp::gain_to_db(bass_comp(-40.0, -30.0, true, 0.0)) > 10.0);
        assert!((lg_dsp::gain_to_db(bass_comp(-40.0, -30.0, true, 6.0) / bass_comp(-40.0, -30.0, true, 0.0)) - 6.0).abs() < 1e-3);
        // Soft attacks slower
        let mut h = BassAmp::new(48000.0);
        h.set(1001, 0.0);
        let mut so = BassAmp::new(48000.0);
        so.set(1001, 1.0);
        for _ in 0..240 {
            h.comp.run(1.0);
            so.comp.run(1.0);
        }
        assert!(h.comp.env > so.comp.env + 0.1, "{} {}", h.comp.env, so.comp.env);
    }

    #[test]
    fn bass_channel_and_bright() {
        let tone = |f: f32| -> Vec<f32> { (0..24000).map(|i| 0.01 * (crate::util::TAU * f * i as f32 / 48000.0).sin()).collect() };
        let rms = |v: &[f32]| (v[12000..].iter().map(|x| x * x).sum::<f32>() / 12000.0).sqrt();
        let run = |set: &[(u32, f32)], x: &[f32]| {
            let mut b = BassAmp::new(48000.0);
            b.set(2, 0.0);
            b.set(3, 0.0);
            b.set(47, 6.0); // Direct (PowerAmp Out)
            for &(id, v) in set {
                b.set(id, v);
            }
            let (mut l, mut r) = (x.to_vec(), x.to_vec());
            b.process(&mut l, &mut r);
            rms(&l)
        };
        let x = tone(200.0);
        let g = lg_dsp::gain_to_db(run(&[(1007, 1.0)], &x) / run(&[], &x));
        assert!(g < -8.0, "{g}");
        let h = tone(4000.0);
        assert!(run(&[(1008, 1.0)], &h) > 1.4 * run(&[], &h));
        assert!((run(&[(1008, 1.0)], &tone(60.0)) / run(&[], &tone(60.0)) - 1.0).abs() < 0.15);
    }

    #[test]
    fn bass_eq_pre_post_matters_only_with_the_compressor() {
        let x: Vec<f32> = (0..24000).map(|i| 0.4 * (crate::util::TAU * 100.0 * i as f32 / 48000.0).sin()).collect();
        let run = |comp: f32, pos: f32| {
            let mut b = BassAmp::new(48000.0);
            b.set(2, comp);
            b.set(33, 15.0); // +15 dB at 100 Hz
            b.set(1000, pos);
            let (mut l, mut r) = (x.clone(), x.clone());
            b.process(&mut l, &mut r);
            l
        };
        assert_eq!(BASS_PARAMS.iter().find(|p| p.id == 1000).unwrap().default, 0.0);
        let d = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(p, q)| (p - q).abs()).fold(0.0, f32::max);
        assert!(d(&run(0.0, 0.0), &run(0.0, 1.0)) < 1e-4);
        assert!(d(&run(1.0, 0.0), &run(1.0, 1.0)) > 0.01);
    }

    #[test]
    fn amp_fx_sync_free() {
        // free: Hz from the knob
        assert!((amp_fx_hz(5.0, false, 120.0) - 5.0).abs() < 1e-5);
        // synced: knob 0 = four bars, 10 = 1/64; values in between land on note values,
        // triplets included
        assert!((amp_fx_hz(0.0, true, 120.0) - 0.125).abs() < 1e-5);
        assert!((amp_fx_hz(10.0, true, 120.0) - 32.0).abs() < 1e-3);
        let triplet = (0..=100).any(|k| {
            let beats = 2.0 / amp_fx_hz(k as f32 / 10.0, true, 120.0);
            ((beats * 1.5) - (beats * 1.5).round()).abs() < 1e-3 && (beats.log2() - beats.log2().round()).abs() > 0.1
        });
        assert!(triplet);
        // and it follows the tempo
        assert!((amp_fx_hz(0.0, true, 60.0) - 0.0625).abs() < 1e-5);
    }
}
