//! Chroma Hall Verb (working name): the stand-in for Logic's ChromaVerb, an algorithmic reverb.
//!
//! Parameters follow a project's ChromaVerb floats: Dry, Wet, Room Type, Predelay (+ sync),
//! Decay Time (+ sync), Attack, Size, Density, Distance, Early/Late Mix, Width, Mono Maker (+ freq),
//! Freeze, Quality, Modulation (depth, speed, source, smoothing), the four-band Damping EQ
//! (frequency, decay ratio, Q per band) and the six-band Output EQ.
//!
//! The engine, written from scratch:
//! - Predelay, then an Attack stage (a ramped multi-tap smear that makes the onset swell), then
//!   input diffusion (series all-passes whose strength follows Density).
//! - Early reflections: a tapped delay per room type, scaled by Size, softened by Distance.
//! - Late field: a feedback delay network of 8 (Quality Low) or 16 lines with a Hadamard
//!   matrix; line lengths from the room type and Size, read with modulated (sine, triangle or
//!   smoothed random) taps against metallic ringing.
//! - Decay: each line's loss is set from Decay Time per frequency band. The Damping EQ's
//!   "ratio" scales the decay time in its band (0.5 = half as long there), applied as
//!   shelf / peak attenuation filters in every loop — the way ChromaVerb's damping curve works.
//! - The steady-state level is normalised so long and short decays sit at a similar loudness
//!   (as Logic's presets do), then Width (mid/side), Mono Maker, the Output EQ and Dry/Wet.
//! - Freeze makes the loop lossless and stops new input.
//!
//! Room types each set line spread, early pattern, diffusion, colour and onset (Dark Room is
//! dull, Airy and Reflective Hall bright, Bloomy and Smooth Space swell, Strange Room uses
//! unrelated line ratios for an odd, metallic space). Levels were matched to Logic's renders of
//! factory settings (white-noise wet energy and decay time; see tests/reference.rs).
//!
//! Not modelled: tempo sync of predelay and decay (the ms / s values are used as they are).

use crate::util::{Biquad, Coefs, Delay, Drift, OnePole, Phase, Values, db, flush, plugin_impl, sine01, tri};
use lg_dsp::{Param, Unit};

pub const ROOMS: &[&str] = &["Room", "Chamber", "Concert Hall", "Theatre", "Synth Hall", "Dark Room", "Dense Room", "Smooth Space", "Vocal Hall", "Reflective Hall", "Strange Room", "Airy", "Bloomy"];
const ORDERS: &[&str] = &["", "6 dB/Oct", "12 dB/Oct"];

const DRY: u32 = 0;
const WET: u32 = 1;
const ROOM: u32 = 2;
const PREDELAY: u32 = 3;
const DECAY: u32 = 5;
const ATTACK: u32 = 7;
const SIZE: u32 = 8;
const DENSITY: u32 = 9;
const DISTANCE: u32 = 10;
const EARLY_LATE: u32 = 11;
const WIDTH: u32 = 12;
const MONO_MAKER: u32 = 13;
const MONO_FREQ: u32 = 14;
const FREEZE: u32 = 15;
const QUALITY: u32 = 16;
const MOD_DEPTH: u32 = 18;
const MOD_SPEED: u32 = 19;
const MOD_SOURCE: u32 = 20;
const MOD_SMOOTH: u32 = 21;
const DAMP: u32 = 23;
const EQ_ON: u32 = 36;
const EQ: u32 = 37;

static PARAMS: [Param; 57] = [
    Param::new(DRY, "Dry", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(WET, "Wet", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(ROOM, "Room Type", ROOMS, 0.0),
    Param::new(PREDELAY, "Predelay", 0.0, 500.0, 8.0, Unit::Ms),
    Param::new(4, "Predelay Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(DECAY, "Decay Time", 0.1, 100.0, 1.1, Unit::Seconds),
    Param::new(6, "DecayTime Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(ATTACK, "Attack", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(SIZE, "Size", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(DENSITY, "Density", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(DISTANCE, "Distance", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(EARLY_LATE, "Early/Late Mix", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(WIDTH, "Width", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(MONO_MAKER, "Mono Maker", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(MONO_FREQ, "Mono Maker Freq", 20.0, 1000.0, 20.0, Unit::Hz),
    Param::new(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(QUALITY, "Quality", &["Low", "High", "Ultra"], 1.0),
    Param::new(MOD_DEPTH, "Modulation Depth", 0.0, 100.0, 25.0, Unit::Percent),
    Param::new(MOD_SPEED, "Modulation Speed", 0.01, 10.0, 0.5, Unit::Hz),
    Param::menu(MOD_SOURCE, "Modulation Source", &["Sine", "Triangle", "Random"], 2.0),
    Param::new(MOD_SMOOTH, "Modulation Smoothing", 0.0, 100.0, 12.0, Unit::Percent),
    Param::new(DAMP, "Damping Low Shelf Freq", 20.0, 20000.0, 210.0, Unit::Hz),
    Param::new(DAMP + 1, "Damping Low Shelf Ratio", 0.1, 4.0, 1.0, Unit::Ratio),
    Param::new(DAMP + 2, "Damping Low Shelf Q", 0.1, 4.0, 0.75, Unit::Ratio),
    Param::new(DAMP + 3, "Damping Low Peak Freq", 20.0, 20000.0, 530.0, Unit::Hz),
    Param::new(DAMP + 4, "Damping Low Peak Ratio", 0.1, 4.0, 0.75, Unit::Ratio),
    Param::new(DAMP + 5, "Damping Low Peak Q", 0.1, 4.0, 2.0, Unit::Ratio),
    Param::new(DAMP + 6, "Damping High Peak Freq", 20.0, 20000.0, 1960.0, Unit::Hz),
    Param::new(DAMP + 7, "Damping High Peak Ratio", 0.1, 4.0, 0.7, Unit::Ratio),
    Param::new(DAMP + 8, "Damping High Peak Q", 0.1, 4.0, 0.88, Unit::Ratio),
    Param::new(DAMP + 9, "Damping High Shelf Freq", 20.0, 20000.0, 5500.0, Unit::Hz),
    Param::new(DAMP + 10, "Damping High Shelf Ratio", 0.1, 4.0, 0.88, Unit::Ratio),
    Param::new(DAMP + 11, "Damping High Shelf Q", 0.1, 4.0, 0.89, Unit::Ratio),
    Param::new(EQ_ON, "EQ On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ, "Low Cut On/Off", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(EQ + 1, "Low Cut Freq", 20.0, 20000.0, 20.0, Unit::Hz),
    Param::menu(EQ + 2, "Low Cut Order", ORDERS, 2.0),
    Param::new(EQ + 3, "Low Cut Q", 0.1, 10.0, 0.71, Unit::Ratio),
    Param::new(EQ + 4, "Low Shelf On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ + 5, "Low Shelf Freq", 20.0, 20000.0, 200.0, Unit::Hz),
    Param::new(EQ + 6, "Low Shelf Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(EQ + 7, "Low Shelf Q", 0.1, 10.0, 1.0, Unit::Ratio),
    Param::new(EQ + 8, "Low Peak On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ + 9, "Low Peak Freq", 20.0, 20000.0, 500.0, Unit::Hz),
    Param::new(EQ + 10, "Low Peak Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(EQ + 11, "Low Peak Q", 0.1, 10.0, 0.6, Unit::Ratio),
    Param::new(EQ + 12, "High Peak On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ + 13, "High Peak Freq", 20.0, 20000.0, 2000.0, Unit::Hz),
    Param::new(EQ + 14, "High Peak Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(EQ + 15, "High Peak Q", 0.1, 10.0, 0.6, Unit::Ratio),
    Param::new(EQ + 16, "High Shelf On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ + 17, "High Shelf Freq", 20.0, 20000.0, 5000.0, Unit::Hz),
    Param::new(EQ + 18, "High Shelf Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(EQ + 19, "High Shelf Q", 0.1, 10.0, 1.0, Unit::Ratio),
    Param::new(EQ + 20, "High Cut On/Off", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(EQ + 21, "High Cut Freq", 20.0, 20000.0, 10400.0, Unit::Hz),
    Param::menu(EQ + 22, "High Cut Order", ORDERS, 2.0),
];

/// One room type's character.
#[derive(Clone, Copy)]
struct RoomKind {
    /// longest late line (ms) at Size 50 %
    long_ms: f32,
    /// shortest line as a share of the longest
    spread: f32,
    /// early reflections: span (ms at Size 50 %), level, count used (of 12)
    er_ms: f32,
    er_level: f32,
    er_taps: usize,
    /// input diffusion strength scale
    diffuse: f32,
    /// extra high-frequency decay ratio (multiplies the High Shelf ratio) and output tilt (dB)
    hf: f32,
    tilt: f32,
    /// onset swell added to the Attack setting (0..1)
    bloom: f32,
    /// line lengths spread unevenly (Strange Room)
    odd: bool,
    /// level trim (dB)
    trim: f32,
}

const KINDS: [RoomKind; 13] = [
    RoomKind { long_ms: 62.0, spread: 0.32, er_ms: 32.0, er_level: 0.9, er_taps: 12, diffuse: 1.0, hf: 1.0, tilt: 0.0, bloom: 0.0, odd: false, trim: 0.0 },
    RoomKind { long_ms: 78.0, spread: 0.35, er_ms: 45.0, er_level: 0.8, er_taps: 12, diffuse: 1.1, hf: 0.9, tilt: 0.0, bloom: 0.05, odd: false, trim: 0.0 },
    RoomKind { long_ms: 140.0, spread: 0.30, er_ms: 85.0, er_level: 0.6, er_taps: 10, diffuse: 1.0, hf: 0.85, tilt: -0.5, bloom: 0.12, odd: false, trim: 0.0 },
    RoomKind { long_ms: 115.0, spread: 0.33, er_ms: 70.0, er_level: 0.75, er_taps: 12, diffuse: 0.9, hf: 0.95, tilt: 0.0, bloom: 0.08, odd: false, trim: 0.0 },
    RoomKind { long_ms: 150.0, spread: 0.28, er_ms: 60.0, er_level: 0.45, er_taps: 8, diffuse: 1.2, hf: 1.15, tilt: 0.5, bloom: 0.15, odd: false, trim: 0.0 },
    RoomKind { long_ms: 60.0, spread: 0.33, er_ms: 30.0, er_level: 0.8, er_taps: 12, diffuse: 1.0, hf: 0.45, tilt: -3.0, bloom: 0.0, odd: false, trim: 0.0 },
    RoomKind { long_ms: 52.0, spread: 0.45, er_ms: 22.0, er_level: 0.95, er_taps: 12, diffuse: 1.3, hf: 0.9, tilt: 0.0, bloom: 0.0, odd: false, trim: 0.0 },
    RoomKind { long_ms: 165.0, spread: 0.25, er_ms: 50.0, er_level: 0.3, er_taps: 6, diffuse: 1.3, hf: 1.0, tilt: 0.0, bloom: 0.3, odd: false, trim: 0.0 },
    RoomKind { long_ms: 120.0, spread: 0.3, er_ms: 55.0, er_level: 0.55, er_taps: 9, diffuse: 1.15, hf: 0.8, tilt: -1.0, bloom: 0.1, odd: false, trim: 0.0 },
    RoomKind { long_ms: 135.0, spread: 0.3, er_ms: 90.0, er_level: 1.0, er_taps: 12, diffuse: 0.75, hf: 1.25, tilt: 1.0, bloom: 0.0, odd: false, trim: 0.0 },
    RoomKind { long_ms: 90.0, spread: 0.5, er_ms: 40.0, er_level: 0.85, er_taps: 12, diffuse: 0.6, hf: 1.1, tilt: 0.0, bloom: 0.0, odd: true, trim: 0.0 },
    RoomKind { long_ms: 145.0, spread: 0.3, er_ms: 60.0, er_level: 0.5, er_taps: 8, diffuse: 1.2, hf: 1.2, tilt: 2.0, bloom: 0.08, odd: false, trim: -2.5 },
    RoomKind { long_ms: 155.0, spread: 0.27, er_ms: 45.0, er_level: 0.25, er_taps: 6, diffuse: 1.35, hf: 1.0, tilt: 0.0, bloom: 0.45, odd: false, trim: 0.0 },
];

/// Early-reflection pattern: (time as a share of the span, gain, pan -1..1).
const ER: [(f32, f32, f32); 12] = [
    (0.043, 0.84, -0.6),
    (0.097, 0.72, 0.7),
    (0.151, 0.66, -0.2),
    (0.213, 0.61, 0.45),
    (0.287, 0.52, -0.85),
    (0.352, 0.47, 0.9),
    (0.441, 0.41, 0.1),
    (0.527, 0.36, -0.5),
    (0.613, 0.3, 0.6),
    (0.732, 0.25, -0.3),
    (0.851, 0.2, 0.75),
    (1.0, 0.16, -0.7),
];

const N: usize = 16;
/// Even spread of line lengths (relative, 0..1) before primes are picked.
const LINE_POS: [f32; N] = [0.0, 0.53, 0.21, 0.79, 0.11, 0.64, 0.37, 0.92, 0.05, 0.58, 0.29, 0.85, 0.16, 0.71, 0.44, 1.0];
/// Strange Room's uneven ratios.
const ODD_POS: [f32; N] = [0.0, 0.97, 0.03, 0.91, 0.12, 0.88, 0.07, 0.99, 0.41, 0.43, 0.45, 0.47, 0.2, 0.22, 0.6, 0.62];
/// Input diffuser all-pass lengths (ms at Size 50 %).
const DIFF_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];

const MAX_SIZE_SCALE: f32 = 2.2;
const MAX_LINE_MS: f32 = 170.0 * MAX_SIZE_SCALE + 20.0;
const ATTACK_TAPS: usize = 16;
const MAX_ATTACK_MS: f32 = 600.0;

#[derive(Clone)]
struct Allpass {
    d: Delay,
    len: f32,
    g: f32,
}

impl Allpass {
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let z = self.d.read(self.len);
        let v = x + self.g * z;
        self.d.push(v);
        z - self.g * v
    }
}

/// A damping-band attenuation filter set for one line: broadband gain and four biquads.
#[derive(Clone, Copy, Default)]
struct LineFilter {
    gain: f32,
    bq: [Biquad; 4],
}

pub struct ChromaVerb {
    p: Values,
    rate: f32,
    kind: RoomKind,
    lines: usize,
    // predelay and attack
    pre: [Delay; 2],
    pre_len: f32,
    att_taps: [(f32, f32); ATTACK_TAPS],
    att_on: bool,
    att_delay: [Delay; 2],
    diff: [[Allpass; 4]; 2],
    // early reflections
    er_delay: [Delay; 2],
    er: [(f32, f32, f32); 12],
    er_n: usize,
    er_lp: [OnePole; 2],
    // late
    delay: Vec<Delay>,
    len: [f32; N],
    filt: [LineFilter; N],
    mod_ph: [Phase; N],
    mod_rnd: [Drift; N],
    mod_amt: f32,
    mod_smooth: OnePole,
    smooth_mod: [f32; N],
    inject: f32,
    late_lp: [OnePole; 2],
    // output
    eq: [[Biquad; 8]; 2],
    eq_n: usize,
    mono_lp: Biquad,
    er_gain: f32,
    late_gain: f32,
    dry: f32,
    wet: f32,
    width: f32,
    freeze: bool,
}

impl ChromaVerb {
    pub fn new(rate: f32) -> ChromaVerb {
        let line_cap = (MAX_LINE_MS * 0.001 * rate) as usize + 64;
        let diff = |scale: usize| -> [Allpass; 4] {
            std::array::from_fn(|i| Allpass { d: Delay::new((DIFF_MS[i] * MAX_SIZE_SCALE * 0.001 * rate) as usize * scale + 8), len: 10.0, g: 0.5 })
        };
        let mut v = ChromaVerb {
            p: Values::new(&PARAMS),
            rate,
            kind: KINDS[0],
            lines: N,
            pre: [Delay::new((0.52 * rate) as usize), Delay::new((0.52 * rate) as usize)],
            pre_len: 1.0,
            att_taps: [(1.0, 0.0); ATTACK_TAPS],
            att_on: false,
            att_delay: [Delay::new((MAX_ATTACK_MS * 0.001 * rate) as usize + 8), Delay::new((MAX_ATTACK_MS * 0.001 * rate) as usize + 8)],
            diff: [diff(1), diff(1)],
            er_delay: [Delay::new((130.0 * MAX_SIZE_SCALE * 0.001 * rate) as usize), Delay::new((130.0 * MAX_SIZE_SCALE * 0.001 * rate) as usize)],
            er: [(1.0, 0.0, 0.0); 12],
            er_n: 0,
            er_lp: [OnePole::new(rate, 12000.0); 2],
            delay: (0..N).map(|_| Delay::new(line_cap)).collect(),
            len: [100.0; N],
            filt: [LineFilter::default(); N],
            mod_ph: std::array::from_fn(|i| Phase { p: i as f32 / N as f32 }),
            mod_rnd: std::array::from_fn(|i| Drift::new(0x9E37_79B9u32.wrapping_mul(i as u32 + 1))),
            mod_amt: 0.0,
            mod_smooth: OnePole::new(rate, 20.0),
            smooth_mod: [0.0; N],
            inject: 0.3,
            late_lp: [OnePole::new(rate, 16000.0); 2],
            eq: [[Biquad::default(); 8]; 2],
            eq_n: 0,
            mono_lp: Biquad::default(),
            er_gain: 1.0,
            late_gain: 1.0,
            dry: 1.0,
            wet: 0.5,
            width: 1.0,
            freeze: false,
        };
        v.update();
        v
    }

    fn update(&mut self) {
        let p = &self.p;
        let rate = self.rate;
        let k = KINDS[p.idx(ROOM).min(KINDS.len() - 1)];
        self.kind = k;
        self.lines = if p.idx(QUALITY) == 0 { 8 } else { 16 };
        let size = p.get(SIZE) / 100.0;
        // Size 0..100 % scales the space 1/2.2x..2.2x (50 % = the room's own size)
        let scale = MAX_SIZE_SCALE.powf(2.0 * size.clamp(0.0, 1.0) - 1.0);
        let decay = p.get(DECAY).max(0.1);
        self.freeze = p.on(FREEZE);
        // predelay
        self.pre_len = (p.get(PREDELAY) * 0.001 * rate).max(1.0);
        // attack swell: taps spread over the attack time with a rising ramp
        let att = (p.get(ATTACK) / 100.0 + k.bloom).clamp(0.0, 1.0);
        let att_ms = att * att * MAX_ATTACK_MS.min(decay * 400.0);
        self.att_on = att_ms > 1.0;
        let mut tot = 0.0;
        for i in 0..ATTACK_TAPS {
            let t = i as f32 / (ATTACK_TAPS - 1) as f32;
            // rising weights: the swell's shape (spaced unevenly so it doesn't buzz)
            let tt = t.powf(0.8);
            let w = 0.25 + t;
            self.att_taps[i] = ((tt * att_ms * 0.001 * rate).max(1.0), w);
            tot += w * w;
        }
        let norm = 1.0 / tot.sqrt();
        for t in self.att_taps.iter_mut() {
            t.1 *= norm;
        }
        // input diffusion
        let dens = p.get(DENSITY) / 100.0;
        let g = ((0.35 + 0.4 * dens) * k.diffuse).clamp(0.1, 0.78);
        for (ch, d) in self.diff.iter_mut().enumerate() {
            for (i, ap) in d.iter_mut().enumerate() {
                let len = DIFF_MS[i] * scale.max(0.3) * 0.001 * rate * if ch == 1 { 1.07 } else { 1.0 };
                ap.len = len.clamp(2.0, (ap.d.capacity() - 4) as f32);
                ap.g = if i < 2 { g } else { g * 0.85 };
            }
        }
        // early reflections
        self.er_n = k.er_taps;
        let span = k.er_ms * scale * 0.001 * rate;
        for i in 0..12 {
            let (t, gain, pan) = ER[i];
            self.er[i] = ((t * span).max(1.0), gain * k.er_level, pan);
        }
        let dist = p.get(DISTANCE) / 100.0;
        let er_cut = 16000.0 - 11000.0 * dist;
        self.er_lp = [OnePole::new(rate, er_cut); 2];
        let late_cut = 18000.0 - 9000.0 * dist;
        for f in self.late_lp.iter_mut() {
            f.set(rate, late_cut);
        }
        // late lines: lengths spread between spread*long and long, nudged to odd integers
        let long = k.long_ms * scale * 0.001 * rate;
        let short = long * k.spread;
        for i in 0..N {
            let pos = if k.odd { ODD_POS[i] } else { LINE_POS[i] };
            let l = short * (long / short).powf(pos);
            let mut li = l.round() as usize | 1;
            // keep lines mutually distinct
            while (0..i).any(|j| (self.len[j] as usize) == li) {
                li += 2;
            }
            self.len[i] = (li as f32).min((self.delay[i].capacity() - 64) as f32);
        }
        // damping: per line, broadband loss for Decay Time, then the four bands' corrections
        let ratios = [p.get(DAMP + 1), p.get(DAMP + 4), p.get(DAMP + 7), p.get(DAMP + 10) * k.hf];
        let freqs = [p.get(DAMP), p.get(DAMP + 3), p.get(DAMP + 6), p.get(DAMP + 9)];
        let qs = [p.get(DAMP + 2), p.get(DAMP + 5), p.get(DAMP + 8), p.get(DAMP + 11)];
        let mut gsum = 0.0;
        for i in 0..N {
            let secs = self.len[i] / rate;
            let base_db = -60.0 * secs / decay;
            let f = &mut self.filt[i];
            f.gain = db(base_db);
            gsum += f.gain * f.gain;
            for b in 0..4 {
                let r = ratios[b].clamp(0.1, 4.0);
                // decay ratio r: this band decays in decay*r, so its loss is base/r
                let extra = base_db * (1.0 / r - 1.0);
                f.bq[b].c = match b {
                    0 => Coefs::low_shelf(rate, freqs[b], qs[b].max(0.3), extra),
                    3 => Coefs::high_shelf(rate, freqs[b], qs[b].max(0.3), extra),
                    _ => Coefs::peak(rate, freqs[b], qs[b].max(0.2), extra),
                };
            }
        }
        // never let a band's loop gain reach 1 (overlapping long-decay bands could): check
        // the response on a log grid and pull the line's gain down if needed
        for i in 0..N {
            let f = &mut self.filt[i];
            let mut worst: f32 = 0.0;
            for k in 0..32 {
                let fr = 20.0 * 1000f32.powf(k as f32 / 31.0);
                if fr >= rate * 0.49 {
                    break;
                }
                let m = f.bq.iter().map(|b| b.c.magnitude(rate, fr)).product::<f32>() * f.gain;
                worst = worst.max(m);
            }
            if worst > 0.999 {
                f.gain *= 0.999 / worst;
            }
        }
        let gmean2 = gsum / N as f32;
        // input level so the late field's steady energy is about the same for any decay
        self.inject = (1.0 - gmean2).max(0.0005).sqrt() * 0.9;
        // modulation
        let depth = p.get(MOD_DEPTH) / 100.0;
        self.mod_amt = depth * depth * 0.0012 * rate * scale.sqrt().max(0.5);
        let sm = p.get(MOD_SMOOTH) / 100.0;
        self.mod_smooth.set(rate, 200.0 * (1.0 - sm) + 2.0);
        // Early/Late and Distance
        let el = p.get(EARLY_LATE) / 100.0;
        self.er_gain = (2.0 * (1.0 - el)).min(1.0) * (1.0 - 0.75 * dist) * 0.55;
        // Logic's wet level grows with Size (about 0.4 dB per %, measured on its renders)
        let size_db = (0.38 * (p.get(SIZE) - 50.0)).clamp(-14.0, 14.0) + 1.5 + k.trim;
        self.er_gain *= db(size_db);
        self.late_gain = (2.0 * el).min(1.0) * 1.35 * db(size_db);
        self.dry = p.get(DRY) / 100.0;
        self.wet = p.get(WET) / 100.0;
        self.width = p.get(WIDTH) / 100.0;
        self.mono_lp.c = Coefs::lowpass(rate, p.get(MONO_FREQ), 0.707);
        // output EQ (and the room's tilt)
        let mut sect: [Coefs; 8] = [Coefs::THRU; 8];
        let mut n = 0;
        if k.tilt != 0.0 {
            sect[n] = Coefs::high_shelf(rate, 3000.0, 0.5, k.tilt);
            n += 1;
        }
        if p.on(EQ_ON) {
            let on = |i: u32| p.on(EQ + i);
            if on(0) {
                if p.idx(EQ + 2) <= 1 {
                    sect[n] = Coefs::hp1(rate, p.get(EQ + 1));
                } else {
                    sect[n] = Coefs::highpass(rate, p.get(EQ + 1), p.get(EQ + 3));
                }
                n += 1;
            }
            for (b, on_id) in [(0, 4u32), (1, 8), (2, 12), (3, 16)] {
                if on(on_id) && p.get(EQ + on_id + 2).abs() > 0.01 {
                    let (f, g, q) = (p.get(EQ + on_id + 1), p.get(EQ + on_id + 2), p.get(EQ + on_id + 3));
                    sect[n] = match b {
                        0 => Coefs::low_shelf(rate, f, q, g),
                        3 => Coefs::high_shelf(rate, f, q, g),
                        _ => Coefs::peak(rate, f, q, g),
                    };
                    n += 1;
                }
            }
            if on(20) {
                if p.idx(EQ + 22) <= 1 {
                    sect[n] = Coefs::lp1(rate, p.get(EQ + 21));
                } else {
                    sect[n] = Coefs::lowpass(rate, p.get(EQ + 21), 0.71);
                }
                n += 1;
            }
        }
        self.eq_n = n;
        for ch in 0..2 {
            for i in 0..n {
                self.eq[ch][i].c = sect[i];
            }
        }
    }

    fn clear(&mut self) {
        for d in self.pre.iter_mut().chain(self.att_delay.iter_mut()).chain(self.er_delay.iter_mut()) {
            d.clear();
        }
        for d in self.delay.iter_mut() {
            d.clear();
        }
        for ch in self.diff.iter_mut() {
            for ap in ch.iter_mut() {
                ap.d.clear();
            }
        }
        for f in self.filt.iter_mut() {
            for b in f.bq.iter_mut() {
                b.clear();
            }
        }
        for ch in self.eq.iter_mut() {
            for b in ch.iter_mut() {
                b.clear();
            }
        }
        self.mono_lp.clear();
        self.smooth_mod = [0.0; N];
    }

    fn tail_secs(&self) -> f32 {
        if self.freeze {
            return 600.0;
        }
        let r = [self.p.get(DAMP + 1), self.p.get(DAMP + 4), self.p.get(DAMP + 7), self.p.get(DAMP + 10)].iter().cloned().fold(1.0, f32::max);
        self.p.get(DECAY) * r + self.p.get(PREDELAY) * 0.001 + 0.7
    }
}

impl lg_dsp::Effect for ChromaVerb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let lines = self.lines;
        let src = self.p.idx(MOD_SOURCE);
        let speed = self.p.get(MOD_SPEED);
        let scale = 1.0 / (lines as f32).sqrt();
        let mono_maker = self.p.on(MONO_MAKER);
        let rate = self.rate;
        for i in 0..n {
            let (xl, xr) = if self.freeze { (0.0, 0.0) } else { (left[i], right[i]) };
            // predelay
            self.pre[0].push(xl);
            self.pre[1].push(xr);
            let mut a = [self.pre[0].read(self.pre_len), self.pre[1].read(self.pre_len)];
            // attack swell
            if self.att_on {
                for ch in 0..2 {
                    self.att_delay[ch].push(a[ch]);
                    let mut s = 0.0;
                    for &(d, w) in self.att_taps.iter() {
                        s += self.att_delay[ch].read_lin(d) * w;
                    }
                    a[ch] = s;
                }
            }
            // early reflections (from the predelayed, un-swollen signal)
            self.er_delay[0].push(a[0]);
            self.er_delay[1].push(a[1]);
            let (mut el, mut er) = (0.0, 0.0);
            for &(d, g, pan) in self.er[..self.er_n].iter() {
                let s = if pan < 0.0 { self.er_delay[0].read_lin(d) } else { self.er_delay[1].read_lin(d) };
                el += s * g * (1.0 - pan) * 0.5;
                er += s * g * (1.0 + pan) * 0.5;
            }
            let el = self.er_lp[0].lp(el);
            let er = self.er_lp[1].lp(er);
            // input diffusion
            let mut d = [a[0] + el * 0.3, a[1] + er * 0.3];
            for ch in 0..2 {
                for ap in self.diff[ch].iter_mut() {
                    d[ch] = ap.run(d[ch]);
                }
            }
            // late field
            let mut y = [0.0f32; N];
            for j in 0..lines {
                let m = match src {
                    0 => sine01(self.mod_ph[j].step(rate, speed * (1.0 + 0.07 * j as f32))),
                    1 => tri(self.mod_ph[j].step(rate, speed * (1.0 + 0.07 * j as f32))),
                    _ => self.mod_rnd[j].next(rate, speed * (1.0 + 0.11 * j as f32)),
                };
                self.smooth_mod[j] = flush(self.smooth_mod[j] + self.mod_smooth.a * (m - self.smooth_mod[j]));
                let len = self.len[j] + self.mod_amt * (1.0 + self.smooth_mod[j]);
                let mut s = self.delay[j].read(len);
                if !self.freeze {
                    let f = &mut self.filt[j];
                    s *= f.gain;
                    for b in f.bq.iter_mut() {
                        s = b.run(s);
                    }
                }
                y[j] = s;
            }
            // outputs: two orthogonal sign patterns over the lines
            let (mut ll, mut lr) = (0.0, 0.0);
            for j in 0..lines {
                let sl = if (j * 7 + 3) % 5 < 2 { -1.0 } else { 1.0 };
                let sr = if (j * 3 + 1) % 4 < 2 { -1.0 } else { 1.0 };
                ll += y[j] * sl;
                lr += y[j] * sr;
            }
            // Hadamard feedback
            hadamard(&mut y[..lines]);
            let inj = self.inject;
            for j in 0..lines {
                let x = if j % 2 == 0 { d[0] } else { d[1] } * if j % 4 < 2 { inj } else { -inj };
                self.delay[j].push(y[j] * scale + x);
            }
            let ll = self.late_lp[0].lp(ll * scale);
            let lr = self.late_lp[1].lp(lr * scale);
            let mut wl = el * self.er_gain + ll * self.late_gain;
            let mut wr = er * self.er_gain + lr * self.late_gain;
            // width and mono maker
            let mid = 0.5 * (wl + wr);
            let mut side = 0.5 * (wl - wr) * self.width;
            if mono_maker {
                side -= self.mono_lp.run(side);
            }
            wl = mid + side;
            wr = mid - side;
            for k in 0..self.eq_n {
                wl = self.eq[0][k].run(wl);
                wr = self.eq[1][k].run(wr);
            }
            left[i] = left[i] * self.dry + wl * self.wet;
            right[i] = right[i] * self.dry + wr * self.wet;
        }
    }
}

/// In-place fast Walsh-Hadamard transform (length a power of two), unnormalised.
#[inline]
fn hadamard(x: &mut [f32]) {
    let n = x.len();
    let mut h = 1;
    while h < n {
        for i in (0..n).step_by(h * 2) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
}

plugin_impl!(ChromaVerb);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    fn impulse_energy_decay(v: &mut ChromaVerb, secs: f32) -> (f32, f32) {
        let n = (secs * 48000.0) as usize;
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        v.process(&mut l, &mut r);
        let e = |a: usize, b: usize| l[a..b].iter().map(|x| x * x).sum::<f32>();
        (e(4800, 9600), e(n - 4800, n))
    }

    #[test]
    fn decays_and_longer_decay_lasts_longer() {
        let mut v = ChromaVerb::new(48000.0);
        v.set(DRY, 0.0);
        let (a, b) = impulse_energy_decay(&mut v, 1.5);
        assert!(b < a * 1e-3, "{a} {b}");
        let mut w = ChromaVerb::new(48000.0);
        w.set(DRY, 0.0);
        w.set(DECAY, 5.0);
        let (c, d) = impulse_energy_decay(&mut w, 1.5);
        assert!(d / c > b / a * 10.0);
    }

    #[test]
    fn every_room_and_quality_is_stable() {
        for room in 0..ROOMS.len() {
            for q in 0..3 {
                let mut v = ChromaVerb::new(44100.0);
                v.set(ROOM, room as f32);
                v.set(QUALITY, q as f32);
                v.set(SIZE, 100.0);
                v.set(DECAY, 20.0);
                v.set(DAMP + 1, 4.0);
                v.set(ATTACK, 100.0);
                let mut nz = crate::util::Noise(11);
                let mut l: Vec<f32> = (0..44100).map(|i| if i < 22050 { nz.next() } else { 0.0 }).collect();
                let mut r = l.clone();
                v.process(&mut l, &mut r);
                assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 20.0), "room {room} q {q}");
            }
        }
    }
}
