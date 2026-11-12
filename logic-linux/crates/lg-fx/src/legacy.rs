//! Logic's legacy effects (the Legacy submenu: Option-click an Audio FX slot), with the guide's
//! controls and real processing:
//!
//! - Silver Compressor, Silver Gate (look-ahead), Silver EQ, DJ EQ (cuts to −30 dB), Fat EQ (five
//!   bands with switchable types, Q as the slope of the pass bands), the single-band EQs (High
//!   Cut, Low Cut, High Pass / Low Pass Filter with Order and Smoothing, High / Low Shelving EQ,
//!   Parametric EQ).
//! - DeEsser: detector and suppressor bands set apart, the band subtracted (no crossover, so the
//!   phase is untouched), Monitor Det / Sup / Sens / Off, the activity LED.
//! - Ducker: lowers the input while the side chain is over the threshold (attack, hold,
//!   release, look-ahead so the dip can start early), then mixes the side chain in.
//! - AVerb, GoldVerb and PlatinumVerb: early reflections computed from the room's shape (3–7
//!   corners), wall length and the two microphones' stereo base (image sources), a diffuse tail
//!   (density, diffusion, spread, high cut; PlatinumVerb splits it at a crossover with its own
//!   low ratio and level), Balance ER/Reverb.
//! - Bass Amp: nine amp models (their mid emphasis / scoop as the guide gives it), pre gain,
//!   Bass / Mid / Treble with a movable mid, output.
//! - Guitar Amp Pro: eleven amps, fifteen speakers (DI Box bypasses the cabinet), four EQ models,
//!   Amp–Speaker and Amp–EQ links, mic position and type, tremolo / vibrato (free or synced),
//!   three springs, Output after Master.
//! - Denoiser (FFT: threshold, reduce, noise type tilt, frequency / time / transition
//!   smoothing), Speech Enhancer (denoise floor, mic correction models, a multiband voice
//!   compressor with four modes), Grooveshifter (swing of 1/8 or 1/16 by moving the even
//!   divisions, accent, Beat splice or Tonal grains).
//!
//! Parameter ids are our own (Logic's float blocks for these aren't decoded).

use crate::convolve::{C, Fft};
use crate::kit::{Clock, Crossover, Fdn, note_beats};
use crate::util::{Biquad, Coefs, Delay, Follower, OnePole, Phase, Values, coef, db, flush, plugin_impl, sine01};
use lg_dsp::{Effect, Param, Transport, Unit};

pub const NAMES: &[&str] = &[
    "Silver Compressor", "Silver Gate", "Silver EQ", "DJ EQ", "Fat EQ", "High Cut", "Low Cut", "High Pass Filter", "Low Pass Filter", "High Shelving EQ", "Low Shelving EQ", "Parametric EQ",
    "DeEsser", "Ducker", "AVerb", "GoldVerb", "PlatinumVerb", "Bass Amp", "Guitar Amp Pro", "Denoiser", "Speech Enhancer", "Grooveshifter",
];

pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "Silver Compressor" => Box::new(SilverCompressor::new(rate)),
        "Silver Gate" => Box::new(SilverGate::new(rate)),
        "Silver EQ" => Box::new(Eq::new(rate, EqKind::Silver)),
        "DJ EQ" => Box::new(Eq::new(rate, EqKind::Dj)),
        "Fat EQ" => Box::new(FatEq::new(rate)),
        "High Cut" => Box::new(Eq::new(rate, EqKind::HighCut)),
        "Low Cut" => Box::new(Eq::new(rate, EqKind::LowCut)),
        "High Pass Filter" => Box::new(PassFilter::new(rate, true)),
        "Low Pass Filter" => Box::new(PassFilter::new(rate, false)),
        "High Shelving EQ" => Box::new(Eq::new(rate, EqKind::HighShelf)),
        "Low Shelving EQ" => Box::new(Eq::new(rate, EqKind::LowShelf)),
        "Parametric EQ" => Box::new(Eq::new(rate, EqKind::Parametric)),
        "DeEsser" => Box::new(DeEsser::new(rate)),
        "Ducker" => Box::new(Ducker::new(rate)),
        "AVerb" => Box::new(Verb::new(rate, VerbKind::A)),
        "GoldVerb" => Box::new(Verb::new(rate, VerbKind::Gold)),
        "PlatinumVerb" => Box::new(Verb::new(rate, VerbKind::Platinum)),
        "Bass Amp" => Box::new(BassAmp::new(rate)),
        "Guitar Amp Pro" => Box::new(GuitarAmpPro::new(rate)),
        "Denoiser" => Box::new(Denoiser::new(rate, false)),
        "Speech Enhancer" => Box::new(Denoiser::new(rate, true)),
        "Grooveshifter" => Box::new(Grooveshifter::new(rate)),
        _ => return None,
    })
}

fn lin_db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

// ---------------------------------------------------------------------------------------------
// Silver Compressor

static SC_P: [Param; 5] = [
    Param::new(0, "Threshold", -60.0, 0.0, -20.0, Unit::Db),
    Param::new(1, "Ratio", 1.0, 30.0, 4.0, Unit::Ratio),
    Param::new(2, "Attack", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(3, "Release", 5.0, 2000.0, 150.0, Unit::Ms),
    Param::new(4, "Gain", -20.0, 40.0, 0.0, Unit::Db),
];

pub struct SilverCompressor {
    p: Values,
    rate: f32,
    env: Follower,
    gr: f32,
}

impl SilverCompressor {
    pub fn new(rate: f32) -> SilverCompressor {
        let mut c = SilverCompressor { p: Values::new(&SC_P), rate, env: Follower::default(), gr: 0.0 };
        c.update();
        c
    }
    fn update(&mut self) {
        self.env.set(self.rate, self.p.get(2).max(0.05), self.p.get(3));
    }
    fn clear(&mut self) {
        self.env.env = 0.0;
        self.gr = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// Gain reduction (dB ≥ 0) for a detector level (dB).
    pub fn curve(&self, level_db: f32) -> f32 {
        let over = level_db - self.p.get(0);
        if over <= 0.0 { 0.0 } else { over * (1.0 - 1.0 / self.p.get(1)) }
    }
}

impl Effect for SilverCompressor {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mk = db(self.p.get(4));
        let mut peak_gr = 0.0f32;
        for i in 0..left.len().min(right.len()) {
            let lvl = self.env.run(left[i].abs().max(right[i].abs()));
            let gr = self.curve(lin_db(lvl));
            peak_gr = peak_gr.max(gr);
            let g = db(-gr) * mk;
            left[i] *= g;
            right[i] *= g;
        }
        self.gr = peak_gr;
    }
    fn gain_reduction(&self) -> f32 {
        self.gr
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Gain Reduction", self.gr)]
    }
}
plugin_impl!(SilverCompressor);

// ---------------------------------------------------------------------------------------------
// Silver Gate

static SG_P: [Param; 5] = [
    Param::new(0, "Lookahead", 0.0, 10.0, 0.0, Unit::Ms),
    Param::new(1, "Threshold", -100.0, 0.0, -50.0, Unit::Db),
    Param::new(2, "Attack", 0.0, 200.0, 1.0, Unit::Ms),
    Param::new(3, "Hold", 0.0, 2000.0, 50.0, Unit::Ms),
    Param::new(4, "Release", 1.0, 2000.0, 100.0, Unit::Ms),
];

pub struct SilverGate {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    gain: f32,
    held: usize,
    la: usize,
}

impl SilverGate {
    pub fn new(rate: f32) -> SilverGate {
        let n = (0.011 * rate) as usize + 2;
        let mut g = SilverGate { p: Values::new(&SG_P), rate, line: [Delay::new(n), Delay::new(n)], gain: 0.0, held: 0, la: 0 };
        g.update();
        g
    }
    fn update(&mut self) {
        self.la = (self.p.get(0) * 0.001 * self.rate).round() as usize;
    }
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.gain = 0.0;
        self.held = 0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for SilverGate {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let thr = db(self.p.get(1));
        let att = (1.0 / (self.p.get(2).max(0.01) * 0.001 * self.rate)).min(1.0);
        let rel = (1.0 / (self.p.get(4) * 0.001 * self.rate)).min(1.0);
        let hold = (self.p.get(3) * 0.001 * self.rate) as usize;
        for i in 0..left.len().min(right.len()) {
            // the detector sees the input now; the audio is delayed by the look-ahead
            let open = left[i].abs().max(right[i].abs()) >= thr;
            if open {
                self.held = hold;
            } else if self.held > 0 {
                self.held -= 1;
            }
            if open || self.held > 0 {
                self.gain = (self.gain + att).min(1.0);
            } else {
                self.gain = (self.gain - rel).max(0.0);
            }
            self.line[0].push(left[i]);
            self.line[1].push(right[i]);
            left[i] = self.line[0].tap(self.la) * self.gain;
            right[i] = self.line[1].tap(self.la) * self.gain;
        }
    }
    fn latency(&self) -> usize {
        self.la
    }
}
plugin_impl!(SilverGate);

// ---------------------------------------------------------------------------------------------
// Silver EQ, DJ EQ and the single-band EQs (High Cut, Low Cut, shelves, Parametric)

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EqKind {
    Silver,
    Dj,
    HighCut,
    LowCut,
    HighShelf,
    LowShelf,
    Parametric,
}

static SILVER_P: [Param; 7] = [
    Param::new(0, "High Shelf", -18.0, 18.0, 0.0, Unit::Db),
    Param::new(1, "High Frequency", 1000.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(2, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz),
    Param::new(3, "Q-Factor", 0.1, 10.0, 1.0, Unit::Ratio),
    Param::new(4, "Gain", -18.0, 18.0, 0.0, Unit::Db),
    Param::new(5, "Low Shelf", -18.0, 18.0, 0.0, Unit::Db),
    Param::new(6, "Low Frequency", 20.0, 1000.0, 100.0, Unit::Hz),
];
static DJ_P: [Param; 5] = [
    Param::new(0, "High Shelf", -30.0, 12.0, 0.0, Unit::Db),
    Param::new(2, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz),
    Param::new(3, "Q-Factor", 0.1, 10.0, 1.0, Unit::Ratio),
    Param::new(4, "Gain", -30.0, 12.0, 0.0, Unit::Db),
    Param::new(5, "Low Shelf", -30.0, 12.0, 0.0, Unit::Db),
];
static CUT_P: [Param; 1] = [Param::new(0, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz)];
static LOW_CUT_P: [Param; 1] = [Param::new(0, "Frequency", 20.0, 20000.0, 100.0, Unit::Hz)];
static SHELF_P: [Param; 2] = [Param::new(0, "Gain", -30.0, 30.0, 0.0, Unit::Db), Param::new(1, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz)];
static PARA_P: [Param; 3] = [Param::new(0, "Gain", -30.0, 30.0, 0.0, Unit::Db), Param::new(1, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz), Param::new(2, "Q-Factor", 0.1, 100.0, 1.0, Unit::Ratio)];

/// DJ EQ's fixed shelf corners.
pub const DJ_LOW_HZ: f32 = 120.0;
pub const DJ_HIGH_HZ: f32 = 6000.0;

pub struct Eq {
    p: Values,
    rate: f32,
    kind: EqKind,
    bands: [[Biquad; 3]; 2],
    c: [Coefs; 3],
}

impl Eq {
    pub fn new(rate: f32, kind: EqKind) -> Eq {
        let defs: &'static [Param] = match kind {
            EqKind::Silver => &SILVER_P,
            EqKind::Dj => &DJ_P,
            EqKind::HighCut => &CUT_P,
            EqKind::LowCut => &LOW_CUT_P,
            EqKind::HighShelf | EqKind::LowShelf => &SHELF_P,
            EqKind::Parametric => &PARA_P,
        };
        let mut e = Eq { p: Values::new(defs), rate, kind, bands: Default::default(), c: [Coefs::THRU; 3] };
        e.update();
        e
    }
    fn update(&mut self) {
        let (r, p) = (self.rate, &self.p);
        let ny = r * 0.45;
        self.c = match self.kind {
            EqKind::Silver => [Coefs::high_shelf(r, p.get(1).min(ny), 0.707, p.get(0)), Coefs::peak(r, p.get(2).min(ny), p.get(3), p.get(4)), Coefs::low_shelf(r, p.get(6), 0.707, p.get(5))],
            EqKind::Dj => [Coefs::high_shelf(r, DJ_HIGH_HZ, 0.707, p.get(0)), Coefs::peak(r, p.get(2).min(ny), p.get(3), p.get(4)), Coefs::low_shelf(r, DJ_LOW_HZ, 0.707, p.get(5))],
            // gentle 6 dB/octave cuts
            EqKind::HighCut => [Coefs::lp1(r, p.get(0).min(ny)), Coefs::THRU, Coefs::THRU],
            EqKind::LowCut => [Coefs::hp1(r, p.get(0).min(ny)), Coefs::THRU, Coefs::THRU],
            EqKind::HighShelf => [Coefs::high_shelf(r, p.get(1).min(ny), 0.707, p.get(0)), Coefs::THRU, Coefs::THRU],
            EqKind::LowShelf => [Coefs::low_shelf(r, p.get(1).min(ny), 0.707, p.get(0)), Coefs::THRU, Coefs::THRU],
            EqKind::Parametric => [Coefs::peak(r, p.get(1).min(ny), p.get(2), p.get(0)), Coefs::THRU, Coefs::THRU],
        };
        for s in 0..2 {
            for k in 0..3 {
                self.bands[s][k].c = self.c[k];
            }
        }
    }
    fn clear(&mut self) {
        for b in self.bands.iter_mut().flatten() {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The response at a frequency (dB).
    pub fn response_db(&self, f: f32) -> f32 {
        self.c.iter().map(|c| lin_db(c.magnitude(self.rate, f))).sum()
    }
}

impl Effect for Eq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (s, buf) in [left, right].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = *x;
                for b in self.bands[s].iter_mut() {
                    y = b.run(y);
                }
                *x = y;
            }
        }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        if name != "response" {
            return Vec::new();
        }
        (0..128).map(|i| self.response_db(20.0 * 1000f32.powf(i as f32 / 127.0))).collect()
    }
}
plugin_impl!(Eq);

// ---------------------------------------------------------------------------------------------
// High Pass / Low Pass Filter (Frequency, Order, Smoothing)

static HP_P: [Param; 3] = [
    Param::new(0, "Frequency", 20.0, 20000.0, 80.0, Unit::Hz),
    Param::new(1, "Order", 1.0, 6.0, 2.0, Unit::None),
    Param::new(2, "Smoothing", 0.0, 200.0, 0.0, Unit::Ms),
];
static PASS_P: [Param; 3] = [
    Param::new(0, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz),
    Param::new(1, "Order", 1.0, 6.0, 2.0, Unit::None),
    Param::new(2, "Smoothing", 0.0, 200.0, 0.0, Unit::Ms),
];

pub struct PassFilter {
    p: Values,
    rate: f32,
    high: bool,
    st: [[Biquad; 3]; 2],
    first: [Biquad; 2],
    freq: f32,
    k: u32,
}

impl PassFilter {
    pub fn new(rate: f32, high: bool) -> PassFilter {
        let mut f = PassFilter { p: Values::new(if high { &HP_P } else { &PASS_P }), rate, high, st: Default::default(), first: Default::default(), freq: 1000.0, k: 0 };
        f.freq = f.p.get(0);
        f.place(f.freq);
        f
    }
    fn order(&self) -> usize {
        self.p.get(1).round().clamp(1.0, 6.0) as usize
    }
    fn place(&mut self, hz: f32) {
        let r = self.rate;
        let hz = hz.clamp(10.0, r * 0.45);
        let n = self.order();
        // Butterworth: odd orders get a first-order section, then biquads with the Butterworth Qs
        let qs: Vec<f32> = (0..n / 2).map(|k| 1.0 / (2.0 * (std::f32::consts::PI * (2 * k + 1) as f32 / (2 * n) as f32).sin())).collect();
        for s in 0..2 {
            self.first[s].c = if n % 2 == 1 { if self.high { Coefs::hp1(r, hz) } else { Coefs::lp1(r, hz) } } else { Coefs::THRU };
            for k in 0..3 {
                self.st[s][k].c = match qs.get(k) {
                    Some(q) => {
                        if self.high {
                            Coefs::highpass(r, hz, *q)
                        } else {
                            Coefs::lowpass(r, hz, *q)
                        }
                    }
                    None => Coefs::THRU,
                };
            }
        }
    }
    fn update(&mut self) {
        if self.p.get(2) <= 0.0 {
            self.freq = self.p.get(0);
        }
        let f = self.freq;
        self.place(f);
    }
    fn clear(&mut self) {
        for b in self.st.iter_mut().flatten().chain(self.first.iter_mut()) {
            b.clear();
        }
        self.freq = self.p.get(0);
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for PassFilter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let target = self.p.get(0);
        let sm = self.p.get(2);
        for i in 0..left.len().min(right.len()) {
            // Smoothing: the cutoff glides to a new value over the set time (in octaves)
            if (self.freq - target).abs() > 0.01 {
                self.k += 1;
                if self.k % 16 == 0 {
                    let k = 1.0 - coef(self.rate / 16.0, sm);
                    self.freq = if sm <= 0.0 { target } else { (self.freq.ln() + (target.ln() - self.freq.ln()) * k).exp() };
                    let f = self.freq;
                    self.place(f);
                }
            }
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut y = self.first[s].run(*x);
                for b in self.st[s].iter_mut() {
                    y = b.run(y);
                }
                *x = y;
            }
        }
    }
}
plugin_impl!(PassFilter);

// ---------------------------------------------------------------------------------------------
// Fat EQ

const FAT_TYPES: [&[&str]; 5] = [&["High Pass", "Low Shelf"], &["Low Shelf", "Parametric"], &["Parametric"], &["Parametric", "High Shelf"], &["High Shelf", "Low Pass"]];
const FAT_FREQS: [f32; 5] = [80.0, 200.0, 1000.0, 4000.0, 12000.0];

static FAT_P: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let names = ["1", "2", "3", "4", "5"];
    let mut v = Vec::new();
    for b in 0..5 {
        let id = (b * 5) as u32;
        let n = |s: &str| crate::kit::leak(format!("Band {} {s}", names[b]));
        v.push(Param::new(id, n("On"), 0.0, 1.0, if b == 0 || b == 4 { 0.0 } else { 1.0 }, Unit::OnOff));
        v.push(Param::menu(id + 1, n("Type"), FAT_TYPES[b], if b == 1 || b == 3 { 1.0 } else { 0.0 }));
        v.push(Param::new(id + 2, n("Frequency"), 20.0, 20000.0, FAT_FREQS[b], Unit::Hz));
        v.push(Param::new(id + 3, n("Gain"), -20.0, 20.0, 0.0, Unit::Db));
        v.push(Param::new(id + 4, n("Q"), 0.1, 10.0, 1.0, Unit::Ratio));
    }
    v.push(Param::new(25, "Master Gain", -20.0, 20.0, 0.0, Unit::Db));
    v
});

pub struct FatEq {
    p: Values,
    rate: f32,
    f: [[[Biquad; 4]; 5]; 2],
    c: [[Coefs; 4]; 5],
}

impl FatEq {
    pub fn new(rate: f32) -> FatEq {
        let mut e = FatEq { p: Values::new(&FAT_P), rate, f: Default::default(), c: [[Coefs::THRU; 4]; 5] };
        e.update();
        e
    }
    /// A band's type name.
    pub fn band_type(&self, b: usize) -> &'static str {
        FAT_TYPES[b][self.p.idx(b as u32 * 5 + 1).min(FAT_TYPES[b].len() - 1)]
    }
    fn update(&mut self) {
        let r = self.rate;
        for b in 0..5 {
            let id = b as u32 * 5;
            let (hz, g, q) = (self.p.get(id + 2).min(r * 0.45), self.p.get(id + 3), self.p.get(id + 4));
            let mut c = [Coefs::THRU; 4];
            if self.p.on(id) {
                match self.band_type(b) {
                    "High Pass" | "Low Pass" => {
                        // bands 1 and 5: Q sets the slope (6 to 24 dB/octave)
                        let n = (1.0 + (q.clamp(0.1, 10.0).log10() + 1.0) / 2.0 * 3.0).round().clamp(1.0, 4.0) as usize;
                        let hp = self.band_type(b) == "High Pass";
                        for slot in c.iter_mut().take(n) {
                            *slot = if hp { Coefs::hp1(r, hz) } else { Coefs::lp1(r, hz) };
                        }
                    }
                    "Low Shelf" => c[0] = Coefs::low_shelf(r, hz, q.min(2.0), g),
                    "High Shelf" => c[0] = Coefs::high_shelf(r, hz, q.min(2.0), g),
                    _ => c[0] = Coefs::peak(r, hz, q, g),
                }
            }
            self.c[b] = c;
            for s in 0..2 {
                for k in 0..4 {
                    self.f[s][b][k].c = c[k];
                }
            }
        }
    }
    fn clear(&mut self) {
        for b in self.f.iter_mut().flatten().flatten() {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The response of one band (None: all of them, with Master Gain) at a frequency, dB.
    pub fn response_db(&self, band: Option<usize>, f: f32) -> f32 {
        let bands: Vec<usize> = band.map(|b| vec![b]).unwrap_or((0..5).collect());
        bands.iter().flat_map(|b| self.c[*b].iter()).map(|c| lin_db(c.magnitude(self.rate, f))).sum::<f32>() + if band.is_none() { self.p.get(25) } else { 0.0 }
    }
}

impl Effect for FatEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let g = db(self.p.get(25));
        for (s, buf) in [left, right].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = *x;
                for b in self.f[s].iter_mut().flatten() {
                    y = b.run(y);
                }
                *x = y * g;
            }
        }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        let band = match name {
            "response" => None,
            n => match n.strip_prefix("band").and_then(|b| b.parse::<usize>().ok()) {
                Some(b) if (1..=5).contains(&b) => Some(b - 1),
                _ => return Vec::new(),
            },
        };
        (0..128).map(|i| self.response_db(band, 20.0 * 1000f32.powf(i as f32 / 127.0))).collect()
    }
}
plugin_impl!(FatEq);

// ---------------------------------------------------------------------------------------------
// DeEsser (legacy)

static DE_P: [Param; 6] = [
    Param::new(0, "Detector Frequency", 500.0, 20000.0, 6000.0, Unit::Hz),
    Param::new(1, "Detector Sensitivity", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(2, "Monitor", &["Det", "Sup", "Sens", "Off"], 3.0),
    Param::new(3, "Suppressor Frequency", 500.0, 20000.0, 6000.0, Unit::Hz),
    Param::new(4, "Strength", 0.0, 40.0, 8.0, Unit::Db),
    Param::new(5, "Smoothing", 1.0, 200.0, 10.0, Unit::Ms),
];

pub struct DeEsser {
    p: Values,
    rate: f32,
    det: [Biquad; 2],
    sup: [[Biquad; 2]; 2],
    env: Follower,
    gain: f32,
    active: bool,
}

impl DeEsser {
    pub fn new(rate: f32) -> DeEsser {
        let mut d = DeEsser { p: Values::new(&DE_P), rate, det: Default::default(), sup: Default::default(), env: Follower::default(), gain: 1.0, active: false };
        d.update();
        d
    }
    fn update(&mut self) {
        let r = self.rate;
        let ny = r * 0.45;
        for s in 0..2 {
            self.det[s].c = Coefs::highpass(r, self.p.get(0).min(ny), 0.707);
            // the suppressor band: a band-pass pair around the frequency (subtracted, so the
            // rest passes untouched)
            self.sup[s][0].c = Coefs::bandpass(r, self.p.get(3).min(ny), 0.9);
            self.sup[s][1].c = Coefs::THRU;
        }
        let sm = self.p.get(5);
        self.env.set(r, sm * 0.3, sm * 3.0);
    }
    fn clear(&mut self) {
        for b in self.det.iter_mut().chain(self.sup.iter_mut().flatten()) {
            b.clear();
        }
        self.env.env = 0.0;
        self.gain = 1.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The detector threshold for the Sensitivity (higher sensitivity, lower threshold).
    fn threshold(&self) -> f32 {
        db(-6.0 - self.p.get(1) * 0.5)
    }
}

impl Effect for DeEsser {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let thr = self.threshold();
        let strength = 1.0 - db(-self.p.get(4));
        let mon = self.p.idx(2);
        let mut active = false;
        for i in 0..left.len().min(right.len()) {
            let dl = self.det[0].run(left[i]);
            let dr = self.det[1].run(right[i]);
            let e = self.env.run(dl.abs().max(dr.abs()));
            // over the threshold: the band comes down by up to Strength
            let over = if e > thr { (1.0 - thr / e).clamp(0.0, 1.0) } else { 0.0 };
            active |= over > 0.01;
            let k = over * strength;
            for (s, (x, d)) in [(&mut left[i], dl), (&mut right[i], dr)].into_iter().enumerate() {
                let band = self.sup[s][0].run(*x);
                *x = match mon {
                    0 => d,
                    1 => band,
                    2 => {
                        if e > thr {
                            d * (1.0 - thr / e)
                        } else {
                            0.0
                        }
                    }
                    _ => *x - band * k,
                };
            }
        }
        self.active = active;
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Activity", if self.active { 1.0 } else { 0.0 })]
    }
}
plugin_impl!(DeEsser);

// ---------------------------------------------------------------------------------------------
// Ducker

static DU_P: [Param; 7] = [
    Param::new(0, "Ducking", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Amount", 0.0, 60.0, 12.0, Unit::Db),
    Param::new(2, "Threshold", -60.0, 0.0, -30.0, Unit::Db),
    Param::new(3, "Attack", 0.0, 1000.0, 50.0, Unit::Ms),
    Param::new(4, "Hold", 0.0, 2000.0, 300.0, Unit::Ms),
    Param::new(5, "Release", 1.0, 5000.0, 500.0, Unit::Ms),
    Param::new(6, "Lookahead", 0.0, 1.0, 1.0, Unit::OnOff),
];

/// Ducker's look-ahead: how far ahead it reads the side chain (the dip can start before the
/// voice).
pub const DUCK_LOOKAHEAD_S: f32 = 0.25;

pub struct Ducker {
    p: Values,
    rate: f32,
    side: (Vec<f32>, Vec<f32>),
    line: [Delay; 2],
    sline: [Delay; 2],
    env: Follower,
    gain: f32,
    held: usize,
    la: usize,
}

impl Ducker {
    pub fn new(rate: f32) -> Ducker {
        let n = (DUCK_LOOKAHEAD_S * rate) as usize + 4;
        let mut d = Ducker { p: Values::new(&DU_P), rate, side: (Vec::new(), Vec::new()), line: [Delay::new(n), Delay::new(n)], sline: [Delay::new(n), Delay::new(n)], env: Follower::new(rate, 1.0, 40.0), gain: 1.0, held: 0, la: 0 };
        d.update();
        d
    }
    fn update(&mut self) {
        self.la = if self.p.on(6) { (DUCK_LOOKAHEAD_S * self.rate) as usize } else { 0 };
    }
    fn clear(&mut self) {
        for d in self.line.iter_mut().chain(self.sline.iter_mut()) {
            d.clear();
        }
        self.gain = 1.0;
        self.held = 0;
        self.env.env = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for Ducker {
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side.0.clear();
        self.side.0.extend_from_slice(left);
        self.side.1.clear();
        self.side.1.extend_from_slice(right);
    }
    fn has_side_chain(&self) -> bool {
        true
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let on = self.p.on(0);
        let floor = db(-self.p.get(1));
        let thr = db(self.p.get(2));
        let att_n = (self.p.get(3) * 0.001 * self.rate).max(1.0);
        let rel_n = (self.p.get(5) * 0.001 * self.rate).max(1.0);
        let hold = (self.p.get(4) * 0.001 * self.rate) as usize;
        // with look-ahead the dip is reached when the voice arrives: it starts Attack earlier
        for i in 0..n {
            let (sl, sr) = (self.side.0.get(i).copied().unwrap_or(0.0), self.side.1.get(i).copied().unwrap_or(0.0));
            let e = self.env.run(sl.abs().max(sr.abs()));
            if e >= thr {
                self.held = hold + self.la;
            } else if self.held > 0 {
                self.held -= 1;
            }
            let target = if on && (e >= thr || self.held > 0) { floor } else { 1.0 };
            if target < self.gain {
                self.gain = (self.gain - (1.0 - floor) / att_n).max(target);
            } else {
                self.gain = (self.gain + (1.0 - floor) / rel_n).min(target);
            }
            self.line[0].push(left[i]);
            self.line[1].push(right[i]);
            self.sline[0].push(sl);
            self.sline[1].push(sr);
            // after processing, the side chain is mixed into the output
            left[i] = self.line[0].tap(self.la) * self.gain + self.sline[0].tap(self.la);
            right[i] = self.line[1].tap(self.la) * self.gain + self.sline[1].tap(self.la);
        }
        self.side.0.clear();
        self.side.1.clear();
    }
    fn latency(&self) -> usize {
        self.la
    }
    fn gain_reduction(&self) -> f32 {
        -lin_db(self.gain)
    }
}
plugin_impl!(Ducker);

// ---------------------------------------------------------------------------------------------
// AVerb, GoldVerb, PlatinumVerb

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VerbKind {
    A,
    Gold,
    Platinum,
}

static AV_P: [Param; 5] = [
    Param::new(0, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(1, "Reflectivity", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(2, "Room Size", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(3, "Density/Time", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(4, "Mix", 0.0, 100.0, 30.0, Unit::Percent),
];
static GV_P: [Param; 12] = [
    Param::new(0, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(1, "Room Shape", 3.0, 7.0, 5.0, Unit::None),
    Param::new(2, "Room Size", 2.0, 30.0, 10.0, Unit::None),
    Param::new(3, "Stereo Base", 0.0, 10.0, 2.0, Unit::None),
    Param::new(4, "Initial Delay", 0.0, 200.0, 20.0, Unit::Ms),
    Param::new(5, "Spread", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(6, "High Cut", 500.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(7, "Density", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(8, "Reverb Time", 0.1, 20.0, 2.0, Unit::Seconds),
    Param::new(9, "Diffusion", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(10, "Balance ER/Reverb", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(11, "Mix", 0.0, 100.0, 30.0, Unit::Percent),
];
static PV_P: [Param; 17] = [
    Param::new(0, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(1, "Room Shape", 3.0, 7.0, 5.0, Unit::None),
    Param::new(2, "Room Size", 2.0, 30.0, 10.0, Unit::None),
    Param::new(3, "Stereo Base", 0.0, 10.0, 2.0, Unit::None),
    Param::new(4, "Initial Delay", 0.0, 200.0, 20.0, Unit::Ms),
    Param::new(5, "Spread", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(6, "High Cut", 500.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(7, "Density", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(8, "Reverb Time", 0.1, 20.0, 2.0, Unit::Seconds),
    Param::new(9, "Diffusion", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(10, "Balance ER/Reverb", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(11, "Crossover", 50.0, 2000.0, 400.0, Unit::Hz),
    Param::new(12, "Low Ratio", 10.0, 400.0, 100.0, Unit::Percent),
    Param::new(13, "Low Freq Level", -30.0, 12.0, 0.0, Unit::Db),
    Param::new(14, "Dry", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(15, "Wet", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(16, "ER Scale", 50.0, 200.0, 100.0, Unit::Percent),
];

/// Early reflections of a regular polygon room (n corners, wall length `side` m) heard by two
/// microphones `base` m apart at the centre, from a source in front: (delay s, gain) per mic,
/// image sources of first and second order.
pub fn reflections(n: usize, side: f32, base: f32) -> [Vec<(f32, f32)>; 2] {
    use std::f32::consts::PI;
    let n = n.clamp(3, 7);
    let r = side / (2.0 * (PI / n as f32).sin());
    // the walls: lines through consecutive corners (normal, distance from the centre)
    let apothem = r * (PI / n as f32).cos();
    let walls: Vec<(f32, f32, f32)> = (0..n).map(|k| {
        let a = 2.0 * PI * (k as f32 + 0.5) / n as f32;
        (a.cos(), a.sin(), apothem)
    }).collect();
    let mirror = |(x, y): (f32, f32), (nx, ny, d): (f32, f32, f32)| {
        let dist = x * nx + y * ny - d;
        (x - 2.0 * dist * nx, y - 2.0 * dist * ny)
    };
    let src = (0.0, apothem * 0.5);
    let mics = [(-base / 2.0, 0.0), (base / 2.0, 0.0)];
    let mut images = Vec::new();
    for (i, w) in walls.iter().enumerate() {
        let a = mirror(src, *w);
        images.push((a, 0.7f32));
        for (j, w2) in walls.iter().enumerate() {
            if i != j {
                images.push((mirror(a, *w2), 0.45));
            }
        }
    }
    let mut out: [Vec<(f32, f32)>; 2] = [Vec::new(), Vec::new()];
    for (m, mic) in mics.iter().enumerate() {
        for ((x, y), g) in &images {
            let d = ((x - mic.0).powi(2) + (y - mic.1).powi(2)).sqrt().max(0.5);
            out[m].push((d / 343.0, g / d));
        }
    }
    out
}

/// A Schroeder all-pass diffuser chain with a variable coefficient.
#[derive(Clone)]
struct Diffuser {
    d: Vec<(Delay, usize)>,
    g: f32,
}

impl Diffuser {
    fn new(rate: f32, ms: &[f32]) -> Diffuser {
        Diffuser { d: ms.iter().map(|m| (Delay::new((m * 0.001 * rate) as usize + 2), (m * 0.001 * rate) as usize)).collect(), g: 0.5 }
    }
    fn run(&mut self, mut x: f32) -> f32 {
        for (d, n) in self.d.iter_mut() {
            let z = d.tap((*n).max(1) - 1);
            let v = x + self.g * z;
            d.push(v);
            x = z - self.g * v;
        }
        x
    }
    fn clear(&mut self) {
        for (d, _) in self.d.iter_mut() {
            d.clear();
        }
    }
}

pub struct Verb {
    p: Values,
    rate: f32,
    kind: VerbKind,
    pre: [Delay; 2],
    er_line: [Delay; 2],
    taps: [Vec<(usize, f32)>; 2],
    diff: [Diffuser; 2],
    hi: Fdn,
    lo: Fdn,
    xo: [Crossover; 2],
    cut: [OnePole; 2],
}

impl Verb {
    pub fn new(rate: f32, kind: VerbKind) -> Verb {
        let defs: &'static [Param] = match kind {
            VerbKind::A => &AV_P,
            VerbKind::Gold => &GV_P,
            VerbKind::Platinum => &PV_P,
        };
        let mut v = Verb {
            p: Values::new(defs),
            rate,
            kind,
            pre: [Delay::new((0.25 * rate) as usize), Delay::new((0.25 * rate) as usize)],
            er_line: [Delay::new((0.4 * rate) as usize), Delay::new((0.4 * rate) as usize)],
            taps: [Vec::new(), Vec::new()],
            diff: [Diffuser::new(rate, &[4.7, 3.6, 12.7, 9.3]), Diffuser::new(rate, &[5.1, 3.3, 11.9, 8.7])],
            hi: Fdn::new(rate),
            lo: Fdn::new(rate),
            xo: [Crossover::new(rate, 400.0); 2],
            cut: [OnePole::new(rate, 8000.0); 2],
        };
        v.update();
        v
    }
    /// Reverb time of the high band and the low band (s).
    pub fn times(&self) -> (f32, f32) {
        match self.kind {
            VerbKind::A => {
                let t = 0.2 + self.p.get(3) / 100.0 * 5.0 * (0.4 + 0.6 * self.p.get(1) / 100.0);
                (t, t)
            }
            VerbKind::Gold => (self.p.get(8), self.p.get(8)),
            VerbKind::Platinum => (self.p.get(8), self.p.get(8) * self.p.get(12) / 100.0),
        }
    }
    fn update(&mut self) {
        let r = self.rate;
        let p = &self.p;
        let (thi, tlo) = self.times();
        let (size, cut, density, diffusion) = match self.kind {
            VerbKind::A => (0.4 + p.get(2) / 100.0 * 1.4, 6000.0 + p.get(1) * 120.0, p.get(3), p.get(3)),
            _ => (0.6 + p.get(2) / 30.0, p.get(6), p.get(7), p.get(9)),
        };
        // Density: lower = longer, sparser lines (grainier)
        let size = size * (1.0 + (1.0 - density / 100.0) * 0.8);
        self.hi.set(thi, size, cut);
        self.lo.set(tlo, size, cut);
        let init = if self.kind == VerbKind::A { 0.0 } else { p.get(4) * 0.001 };
        self.hi.predelay = init;
        self.lo.predelay = init;
        for d in self.diff.iter_mut() {
            d.g = 0.15 + diffusion / 100.0 * 0.6;
        }
        for c in self.cut.iter_mut() {
            c.set(r, cut.min(r * 0.45));
        }
        if self.kind == VerbKind::Platinum {
            for x in self.xo.iter_mut() {
                x.set(r, p.get(11));
            }
        }
        // the early reflections from the room
        let (n, side, base, scale) = match self.kind {
            VerbKind::A => (5, 3.0 + p.get(2) / 100.0 * 20.0, 2.0, 1.0),
            VerbKind::Gold => (p.get(1).round() as usize, p.get(2), p.get(3), 1.0),
            VerbKind::Platinum => (p.get(1).round() as usize, p.get(2), p.get(3), p.get(16) / 100.0),
        };
        let refl = reflections(n, side, base);
        let gain = if self.kind == VerbKind::A { p.get(1) / 100.0 } else { 1.0 };
        let max = self.er_line[0].capacity() - 2;
        for m in 0..2 {
            self.taps[m] = refl[m].iter().map(|(t, g)| (((t * scale * r) as usize).min(max), g * gain * 3.0)).collect();
        }
    }
    fn clear(&mut self) {
        for d in self.pre.iter_mut().chain(self.er_line.iter_mut()) {
            d.clear();
        }
        for d in self.diff.iter_mut() {
            d.clear();
        }
        self.hi.clear();
        self.lo.clear();
        for x in self.xo.iter_mut() {
            x.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        let (a, b) = self.times();
        a.max(b) + 0.3
    }
    /// Early reflections' delays for the left microphone (s), for the room display.
    pub fn er_times(&self) -> Vec<f32> {
        self.taps[0].iter().map(|(n, _)| *n as f32 / self.rate).collect()
    }
}

impl Effect for Verb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let pd = p.get(0) * 0.001 * self.rate;
        // Balance: −100 only early reflections, +100 only the tail
        let (er_g, tail_g) = match self.kind {
            VerbKind::A => (0.6, 1.0),
            _ => {
                let b = p.get(10) / 100.0;
                ((1.0 - b).min(1.0), (1.0 + b).min(1.0))
            }
        };
        let (dry, wet) = match self.kind {
            VerbKind::Platinum => (p.get(14) / 100.0, p.get(15) / 100.0),
            _ => (1.0 - p.get(if self.kind == VerbKind::A { 4 } else { 11 }) / 100.0, p.get(if self.kind == VerbKind::A { 4 } else { 11 }) / 100.0),
        };
        let spread = if self.kind == VerbKind::A { 1.0 } else { p.get(5) / 100.0 };
        let low_lvl = if self.kind == VerbKind::Platinum { db(p.get(13)) } else { 1.0 };
        let two = self.kind == VerbKind::Platinum;
        for i in 0..left.len().min(right.len()) {
            let (xl, xr) = (left[i], right[i]);
            self.pre[0].push(xl);
            self.pre[1].push(xr);
            let (il, ir) = if pd >= 1.0 { (self.pre[0].read_lin(pd), self.pre[1].read_lin(pd)) } else { (xl, xr) };
            let mono = 0.5 * (il + ir);
            self.er_line[0].push(il);
            self.er_line[1].push(ir);
            let mut er = [0.0f32; 2];
            for m in 0..2 {
                // each microphone hears both input sides' images (the room is shared)
                for (n, g) in &self.taps[m] {
                    er[m] += (self.er_line[0].tap(*n) + self.er_line[1].tap(*n)) * 0.5 * g;
                }
            }
            let dl = self.diff[0].run(mono + er[0] * 0.3);
            let dr = self.diff[1].run(mono + er[1] * 0.3);
            let (tl, tr) = if two {
                let (ll, hl) = self.xo[0].run(dl);
                let (lr, hr) = self.xo[1].run(dr);
                let (a, b) = self.hi.run(hl, hr);
                let (c, d) = self.lo.run(ll, lr);
                (a + c * low_lvl, b + d * low_lvl)
            } else {
                self.hi.run(dl, dr)
            };
            // Spread: 0 mono, 100 % as is, 200 % wider than the speakers
            let (m, s) = (0.5 * (tl + tr), 0.5 * (tl - tr) * spread);
            let (tl, tr) = (self.cut[0].lp(m + s), self.cut[1].lp(m - s));
            left[i] = xl * dry + (er[0] * er_g + tl * tail_g) * wet;
            right[i] = xr * dry + (er[1] * er_g + tr * tail_g) * wet;
        }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        if name == "reflections" { self.er_times() } else { Vec::new() }
    }
}
plugin_impl!(Verb);

// ---------------------------------------------------------------------------------------------
// Bass Amp (legacy)

pub const BASS_MODELS: &[&str] = &["American Basic", "American Deep", "American Scoop", "American Bright", "New American Basic", "New American Bright", "Top Class DI Warm", "Top Class DI Deep", "Top Class DI Mid"];

static BA_P: [Param; 7] = [
    Param::menu(0, "Model", BASS_MODELS, 0.0),
    Param::new(1, "Pre Gain", -20.0, 20.0, 0.0, Unit::Db),
    Param::new(2, "Bass", -20.0, 20.0, 0.0, Unit::Db),
    Param::new(3, "Mid", -20.0, 20.0, 0.0, Unit::Db),
    Param::new(4, "Mid Freq", 200.0, 3000.0, 500.0, Unit::Hz),
    Param::new(5, "Treble", -20.0, 20.0, 0.0, Unit::Db),
    Param::new(6, "Output Level", -40.0, 12.0, 0.0, Unit::Db),
];

/// A model's voicing: (filters as (kind, Hz, Q, dB), cabinet top Hz). Kinds: 0 peak, 1 low
/// shelf, 2 high shelf.
fn bass_voicing(m: usize) -> (&'static [(u8, f32, f32, f32)], f32) {
    match m {
        // a 1970s 8x10 amp: low punch, rolled-off top
        0 => (&[(1, 90.0, 0.7, 3.0)], 4500.0),
        1 => (&[(1, 90.0, 0.7, 3.0), (0, 500.0, 0.8, 6.0)], 4500.0),
        2 => (&[(1, 90.0, 0.7, 3.0), (0, 500.0, 0.8, 5.0), (0, 4500.0, 0.8, 5.0), (0, 1500.0, 0.6, -4.0)], 6000.0),
        3 => (&[(1, 90.0, 0.7, 3.0), (0, 4500.0, 0.8, 6.0)], 7000.0),
        4 => (&[(1, 80.0, 0.7, 1.5)], 7000.0),
        5 => (&[(1, 80.0, 0.7, 1.5), (2, 2000.0, 0.7, 6.0)], 9000.0),
        // DI boxes: Warm dips the mids 500–5000 Hz, Deep peaks near 700 Hz, Mid is nearly flat
        6 => (&[(0, 1600.0, 0.35, -6.0)], 16000.0),
        7 => (&[(0, 700.0, 0.9, 5.0)], 14000.0),
        _ => (&[(0, 1000.0, 0.5, 0.5)], 18000.0),
    }
}

pub struct BassAmp {
    p: Values,
    rate: f32,
    voice: [[Biquad; 5]; 2],
    tone: [[Biquad; 3]; 2],
    cab: [Biquad; 2],
    drive: f32,
    out: f32,
    c: Vec<Coefs>,
}

impl BassAmp {
    pub fn new(rate: f32) -> BassAmp {
        let mut b = BassAmp { p: Values::new(&BA_P), rate, voice: Default::default(), tone: Default::default(), cab: Default::default(), drive: 1.0, out: 1.0, c: Vec::new() };
        b.update();
        b
    }
    fn update(&mut self) {
        let r = self.rate;
        let (v, top) = bass_voicing(self.p.idx(0));
        let mut c: Vec<Coefs> = v.iter().map(|(k, f, q, g)| match k {
            1 => Coefs::low_shelf(r, *f, *q, *g),
            2 => Coefs::high_shelf(r, *f, *q, *g),
            _ => Coefs::peak(r, *f, *q, *g),
        }).collect();
        c.resize(5, Coefs::THRU);
        let tone = [Coefs::low_shelf(r, 100.0, 0.7, self.p.get(2)), Coefs::peak(r, self.p.get(4), 0.9, self.p.get(3)), Coefs::high_shelf(r, 2500.0, 0.7, self.p.get(5))];
        for s in 0..2 {
            for k in 0..5 {
                self.voice[s][k].c = c[k];
            }
            for k in 0..3 {
                self.tone[s][k].c = tone[k];
            }
            self.cab[s].c = Coefs::lowpass(r, top.min(r * 0.45), 0.707);
        }
        let mut all = c;
        all.extend(tone);
        all.push(Coefs::lowpass(r, top.min(r * 0.45), 0.707));
        self.c = all;
        self.drive = db(self.p.get(1));
        self.out = db(self.p.get(6));
    }
    fn clear(&mut self) {
        for b in self.voice.iter_mut().flatten().chain(self.tone.iter_mut().flatten()).chain(self.cab.iter_mut()) {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The model and tone response at a frequency (dB), without the preamp's saturation.
    pub fn response_db(&self, f: f32) -> f32 {
        self.c.iter().map(|c| lin_db(c.magnitude(self.rate, f))).sum()
    }
}

impl Effect for BassAmp {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (s, buf) in [left, right].into_iter().enumerate() {
            for x in buf.iter_mut() {
                // the preamp: soft saturation, level-matched at small signals
                let mut y = (*x * self.drive).tanh();
                for b in self.voice[s].iter_mut().chain(self.tone[s].iter_mut()) {
                    y = b.run(y);
                }
                *x = self.cab[s].run(y) * self.out;
            }
        }
    }
}
plugin_impl!(BassAmp);

// ---------------------------------------------------------------------------------------------
// Guitar Amp Pro

pub const GAP_AMPS: &[&str] = &["UK Combo 30W", "UK Top 50W", "US Combo 40W", "US Hot Combo 40W", "US Hot Top 100W", "Custom 50W", "British Clean", "British Gain", "American Clean", "American Gain", "Clean Tube"];
pub const GAP_SPEAKERS: &[&str] = &[
    "UK 1x12 open back", "UK 2x12 open back", "UK 2x12 closed", "UK 4x12 closed slanted", "US 1x10 open back", "US 1x12 open back 1", "US 1x12 open back 2", "US 1x12 open back 3", "US broad range", "Analog simulation",
    "UK 1x12", "UK 4x12", "US 1x12 open back", "US 1x12 bass reflex", "DI Box",
];
pub const GAP_EQS: &[&str] = &["British1", "British2", "American", "Modern"];
const GAP_REVERBS: &[&str] = &["Spring 1", "Spring 2", "Spring 3"];

/// Each amp's head voicing (index into amp::VOICES), associated speaker and EQ model.
const GAP_ASSOC: [(usize, usize, usize); 11] = [(6, 1, 0), (0, 3, 1), (2, 5, 2), (5, 6, 2), (10, 11, 3), (7, 3, 3), (6, 10, 0), (0, 11, 1), (2, 12, 2), (5, 13, 2), (19, 7, 2)];

static GAP_P: [Param; 23] = [
    Param::menu(0, "Amp", GAP_AMPS, 0.0),
    Param::menu(1, "Speaker", GAP_SPEAKERS, 1.0),
    Param::menu(2, "EQ", GAP_EQS, 0.0),
    Param::new(3, "Amp-Speaker Link", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(4, "Amp-EQ Link", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(5, "Gain", 0.0, 10.0, 5.0, Unit::None),
    Param::new(6, "Bass", 0.0, 10.0, 5.0, Unit::None),
    Param::new(7, "Mids", 0.0, 10.0, 5.0, Unit::None),
    Param::new(8, "Treble", 0.0, 10.0, 5.0, Unit::None),
    Param::new(9, "Presence", 0.0, 10.0, 5.0, Unit::None),
    Param::new(10, "Master", 0.0, 10.0, 5.0, Unit::None),
    Param::new(11, "FX", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(12, "FX Type", &["Tremolo", "Vibrato"], 0.0),
    Param::new(13, "Depth", 0.0, 10.0, 5.0, Unit::None),
    Param::new(14, "Speed", 0.1, 20.0, 4.0, Unit::Hz),
    Param::new(15, "Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(16, "Sync Speed", crate::kit::NOTE_NAMES, 7.0),
    Param::new(17, "Reverb", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(18, "Reverb Type", GAP_REVERBS, 0.0),
    Param::new(19, "Reverb Level", 0.0, 10.0, 3.0, Unit::None),
    Param::menu(20, "Mic Position", &["Centered", "Off-Center"], 0.0),
    Param::menu(21, "Mic Type", &["Condenser", "Dynamic"], 1.0),
    Param::new(22, "Output", -40.0, 12.0, 0.0, Unit::Db),
];

pub struct GuitarAmpPro {
    p: Values,
    rate: f32,
    core: crate::amp::AmpCore,
    spring: crate::amp::Spring,
    ph: Phase,
    vib: [Delay; 2],
    clock: Clock,
    out: f32,
}

impl GuitarAmpPro {
    pub fn new(rate: f32) -> GuitarAmpPro {
        let mut a = GuitarAmpPro { p: Values::new(&GAP_P), rate, core: crate::amp::AmpCore::new(rate), spring: crate::amp::Spring::new(rate), ph: Phase::default(), vib: [Delay::new((0.02 * rate) as usize), Delay::new((0.02 * rate) as usize)], clock: Clock::default(), out: 1.0 };
        a.update();
        a
    }
    /// The speaker's cabinet model (as Amp Designer's speakers): DI Box bypasses it.
    fn cab_name(i: usize) -> &'static str {
        match i {
            0 | 10 => "British 1x12",
            1 => "British 2x12",
            2 => "British Blues 2x12",
            3 | 11 => "Vintage British 4x12",
            4 => "Black Panel 1x10",
            5 | 12 => "Brown Panel 1x12",
            6 => "Tweed 1x12",
            7 => "Studio 1x12",
            8 => "Sunshine 1x12",
            9 => "Pawnshop 1x8",
            13 => "Brown Panel 1x15",
            _ => "DI Box",
        }
    }
    fn update(&mut self) {
        let p = &self.p;
        let amp = p.idx(0).min(GAP_AMPS.len() - 1);
        let voice = crate::amp::VOICES[GAP_ASSOC[amp].0];
        let stack = match p.idx(2) {
            0 | 1 => 1,
            2 => 0,
            _ => 2,
        };
        let spk = p.idx(1).min(GAP_SPEAKERS.len() - 1);
        let mut cab = crate::amp::cab_for(Self::cab_name(spk));
        if spk == 9 {
            // Analog simulation: a British tube preamp's tone, no speaker resonance
            cab.res = 30.0;
            cab.top = 7000.0;
        }
        // Gain: Clean Tube distorts only at very high settings
        let gain = if amp == 10 { p.get(5) * 0.6 } else { p.get(5) };
        let mic = if p.idx(21) == 0 { 1 } else { 2 };
        let off = if p.idx(20) == 1 { 0.8 } else { 0.0 };
        // British2 EQ: forward upper mids
        let mids = if p.idx(2) == 1 { (p.get(7) + 1.0).min(10.0) } else { p.get(7) };
        self.core.set(voice, gain, p.get(6), mids, p.get(8), p.get(9), p.get(10), stack, cab, mic, off, 0.3, 70.0);
        let kind = [0, 2, 5][p.idx(18).min(2)];
        self.spring.set(self.rate, kind);
        self.out = db(p.get(22) - 8.0);
    }
    fn clear(&mut self) {
        self.core.clear();
        self.spring.clear();
        for d in self.vib.iter_mut() {
            d.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        if self.p.on(17) { 2.0 } else { 0.1 }
    }
    /// The modulation rate (Hz): free, or the synced note at the tempo.
    pub fn speed_hz(&self) -> f32 {
        if self.p.on(15) { (self.clock.bpm as f32 / 60.0) / note_beats(self.p.idx(16)) } else { self.p.get(14) }
    }
}

impl Effect for GuitarAmpPro {
    fn set_tempo(&mut self, bpm: f64) {
        self.clock.set_tempo(bpm, self.rate);
    }
    fn set_transport(&mut self, t: Transport) {
        self.clock.sync(t, self.rate);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let fx = self.p.on(11);
        let vibrato = self.p.idx(12) == 1;
        let depth = self.p.get(13) / 10.0;
        let speed = self.speed_hz();
        let rev = if self.p.on(17) { self.p.get(19) / 10.0 } else { 0.0 };
        let cab_on = self.p.idx(1) != 14;
        for i in 0..left.len().min(right.len()) {
            let lvl = left[i].abs().max(right[i].abs()) * self.core.pre_gain;
            let sag = self.core.sag.run(lvl.min(4.0)) / 4.0;
            let ph = if fx { self.ph.step(rate, speed) } else { 0.0 };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut y = self.core.run(s, *x, sag, cab_on);
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
}

impl GuitarAmpPro {
    /// Choosing an amp with the links on loads its speaker and EQ (Logic's Amp-Speaker and
    /// Amp-EQ Link).
    fn follow_links(&mut self) {
        let amp = self.p.idx(0).min(GAP_AMPS.len() - 1);
        let (_, spk, eq) = GAP_ASSOC[amp];
        if self.p.on(3) {
            self.p.set(1, spk as f32);
        }
        if self.p.on(4) {
            self.p.set(2, eq as f32);
        }
    }
}

impl lg_dsp::Plugin for GuitarAmpPro {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        let was = self.p.idx(0);
        if self.p.set(id, value) {
            if id == 0 && self.p.idx(0) != was {
                self.follow_links();
            }
            self.update();
        }
    }
    fn reset(&mut self) {
        self.clear();
    }
    fn tail(&self) -> f32 {
        self.tail_secs()
    }
    fn load_floats(&mut self, floats: &[f32]) {
        for d in self.p.defs {
            if let Some(v) = floats.get(d.id as usize) {
                self.p.set(d.id, *v);
            }
        }
        self.update();
    }
}

// ---------------------------------------------------------------------------------------------
// Denoiser and Speech Enhancer (FFT)

const NFFT: usize = 1024;
const HOP: usize = NFFT / 4;

static DN_P: [Param; 6] = [
    Param::new(0, "Threshold", -100.0, 0.0, -60.0, Unit::Db),
    Param::new(1, "Reduce", -100.0, 0.0, -20.0, Unit::Db),
    Param::new(2, "Noise Type", -10.0, 10.0, 0.0, Unit::None),
    Param::new(3, "Frequency", 0.0, 10.0, 2.0, Unit::None),
    Param::new(4, "Time", 0.0, 2000.0, 100.0, Unit::Ms),
    Param::new(5, "Transition", 0.0, 10.0, 2.0, Unit::None),
];
pub const MIC_MODELS: &[&str] = &["Generic", "MacBook", "MacBook Pro", "MacBook Air", "iMac", "iSight"];
pub const ENHANCE_MODES: &[&str] = &["Female Solo", "Male Solo", "Female Voice Over", "Male Voice Over"];
static SE_P: [Param; 5] = [
    Param::new(0, "Denoise", -60.0, -20.0, -40.0, Unit::Db),
    Param::new(1, "Mic Correction", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(2, "Mic Model", MIC_MODELS, 0.0),
    Param::new(3, "Voice Enhance", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(4, "Enhance Mode", ENHANCE_MODES, 2.0),
];

/// Overlap-add spectral processing of one channel with a per-bin gain.
struct Stft {
    fft: Fft,
    win: Vec<f32>,
    inbuf: Vec<f32>,
    outbuf: Vec<f32>,
    pos: usize,
    work: Vec<C>,
}

impl Stft {
    fn new() -> Stft {
        let win = (0..NFFT).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / NFFT as f32).cos()).collect();
        Stft { fft: Fft::new(NFFT), win, inbuf: vec![0.0; NFFT], outbuf: vec![0.0; NFFT], pos: 0, work: vec![C::default(); NFFT] }
    }
    fn clear(&mut self) {
        self.inbuf.iter_mut().for_each(|v| *v = 0.0);
        self.outbuf.iter_mut().for_each(|v| *v = 0.0);
        self.pos = 0;
    }
    /// One sample in, one out (NFFT samples late); `gains` decides each frame's bins.
    #[inline]
    fn run(&mut self, x: f32, gains: &mut dyn FnMut(&[f32]) -> Vec<f32>) -> f32 {
        let n = NFFT;
        self.inbuf[n - HOP + self.pos] = x;
        let y = self.outbuf[self.pos];
        self.pos += 1;
        if self.pos == HOP {
            self.pos = 0;
            for i in 0..n {
                self.work[i] = C { re: self.inbuf[i] * self.win[i], im: 0.0 };
            }
            self.fft.run(&mut self.work, false);
            let mags: Vec<f32> = self.work[..n / 2 + 1].iter().map(|c| (c.re * c.re + c.im * c.im).sqrt() * 2.0 / n as f32).collect();
            let g = gains(&mags);
            for k in 0..=n / 2 {
                let gk = g.get(k).copied().unwrap_or(1.0);
                self.work[k].re *= gk;
                self.work[k].im *= gk;
                if k > 0 && k < n / 2 {
                    self.work[n - k] = C { re: self.work[k].re, im: -self.work[k].im };
                }
            }
            self.fft.run(&mut self.work, true);
            // Hann analysis at 75 % overlap sums to 2: scale back
            let sc = 0.5 / n as f32;
            self.outbuf.copy_within(HOP.., 0);
            for v in self.outbuf[n - HOP..].iter_mut() {
                *v = 0.0;
            }
            for i in 0..n {
                self.outbuf[i] += self.work[i].re * sc;
            }
            self.inbuf.copy_within(HOP.., 0);
        }
        y
    }
}

/// The Denoiser's gain per bin from the magnitudes (and its smoothing state).
#[derive(Clone)]
pub struct NoiseRule {
    pub threshold_db: f32,
    pub reduce_db: f32,
    pub tilt: f32,
    pub freq_smooth: usize,
    pub transition_db: f32,
    pub time_k: f32,
}

impl NoiseRule {
    /// Gains for one frame, smoothed against the previous frame's `last`.
    pub fn gains(&self, mags: &[f32], rate: f32, last: &mut Vec<f32>) -> Vec<f32> {
        let n = mags.len();
        let floor = db(self.reduce_db);
        let mut g: Vec<f32> = (0..n).map(|k| {
            let hz = (k as f32 * rate / NFFT as f32).max(20.0);
            // Noise Type: positive = pink (more noise low: threshold higher at low bins),
            // negative = blue (higher at the top); 1 unit ≈ 0.3 dB/octave
            let thr = self.threshold_db - self.tilt * 0.3 * (hz / 1000.0).log2();
            let lvl = lin_db(mags[k]);
            if lvl >= thr {
                1.0
            } else if lvl > thr - self.transition_db && self.transition_db > 0.0 {
                // Transition: levels just under the threshold are reduced less
                let t = (thr - lvl) / self.transition_db;
                1.0 + (floor - 1.0) * t
            } else {
                floor
            }
        }).collect();
        if self.freq_smooth > 0 {
            // Frequency: a bin kept open keeps its neighbours open too (no lone "musical"
            // bins; the more neighbours, the smoother)
            let w = self.freq_smooth;
            let src = g.clone();
            for k in 0..n {
                let (a, b) = (k.saturating_sub(w), (k + w).min(n - 1));
                g[k] = src[a..=b].iter().fold(0.0f32, |m, v| m.max(*v));
            }
        }
        if last.len() != n {
            *last = vec![1.0; n];
        }
        for k in 0..n {
            last[k] += (g[k] - last[k]) * (1.0 - self.time_k);
            g[k] = last[k];
        }
        g
    }
}

pub struct Denoiser {
    p: Values,
    rate: f32,
    speech: bool,
    stft: [Stft; 2],
    last: [Vec<f32>; 2],
    shown: Vec<f32>,
    mic: [[Biquad; 3]; 2],
    bands: [Crossover; 4],
    comp: [Follower; 3],
}

impl Denoiser {
    pub fn new(rate: f32, speech: bool) -> Denoiser {
        let mut d = Denoiser {
            p: Values::new(if speech { &SE_P as &'static [Param] } else { &DN_P }),
            rate,
            speech,
            stft: [Stft::new(), Stft::new()],
            last: [Vec::new(), Vec::new()],
            shown: Vec::new(),
            mic: Default::default(),
            bands: [Crossover::new(rate, 300.0), Crossover::new(rate, 300.0), Crossover::new(rate, 3000.0), Crossover::new(rate, 3000.0)],
            comp: [Follower::new(rate, 5.0, 120.0); 3],
        };
        d.update();
        d
    }
    pub fn rule(&self) -> NoiseRule {
        let frame_ms = HOP as f32 / self.rate * 1000.0;
        if self.speech {
            NoiseRule { threshold_db: self.p.get(0), reduce_db: -12.0 - (self.p.get(0) + 60.0) * 0.4, tilt: 0.0, freq_smooth: 2, transition_db: 6.0, time_k: coef(1000.0 / frame_ms, 60.0) }
        } else {
            NoiseRule {
                threshold_db: self.p.get(0),
                reduce_db: self.p.get(1),
                tilt: self.p.get(2),
                freq_smooth: self.p.get(3).round() as usize,
                transition_db: self.p.get(5) * 3.0,
                time_k: coef(1000.0 / frame_ms, self.p.get(4)),
            }
        }
    }
    fn update(&mut self) {
        if !self.speech {
            return;
        }
        let r = self.rate;
        // built-in microphones: thin lows, a resonant upper-mid bump; correction undoes it
        let (lo, bump, bump_db) = match self.p.idx(2) {
            1 => (180.0, 3500.0, 4.0),
            2 => (150.0, 4000.0, 3.0),
            3 => (200.0, 3000.0, 5.0),
            4 => (140.0, 2500.0, 3.0),
            5 => (220.0, 4500.0, 4.0),
            _ => (160.0, 3500.0, 2.0),
        };
        let on = self.p.on(1);
        let c = if on { [Coefs::low_shelf(r, lo, 0.7, 6.0), Coefs::peak(r, bump, 1.2, -bump_db), Coefs::highpass(r, 60.0, 0.7)] } else { [Coefs::THRU; 3] };
        for s in 0..2 {
            for k in 0..3 {
                self.mic[s][k].c = c[k];
            }
        }
        // Enhance Mode: a male voice's split points sit lower
        let (a, b) = if self.p.idx(4) % 2 == 1 { (200.0, 2500.0) } else { (300.0, 3500.0) };
        self.bands[0].set(r, a);
        self.bands[1].set(r, a);
        self.bands[2].set(r, b);
        self.bands[3].set(r, b);
    }
    fn clear(&mut self) {
        for s in self.stft.iter_mut() {
            s.clear();
        }
        self.last = [Vec::new(), Vec::new()];
        for b in self.mic.iter_mut().flatten() {
            b.clear();
        }
        for b in self.bands.iter_mut() {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for Denoiser {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rule = self.rule();
        let rate = self.rate;
        let enhance = self.speech && self.p.on(3);
        // Voice Over modes compress harder (the voice over a bed)
        let (thr, ratio, mk) = if self.p.idx(4) >= 2 { (db(-30.0), 4.0, 2.2) } else { (db(-24.0), 2.5, 1.6) };
        for (s, buf) in [left, right].into_iter().enumerate() {
            let mut shown = None;
            for x in buf.iter_mut() {
                let last = &mut self.last[s];
                let mut y = self.stft[s].run(*x, &mut |m: &[f32]| {
                    let g = rule.gains(m, rate, last);
                    shown = Some(g.clone());
                    g
                });
                if self.speech {
                    for b in self.mic[s].iter_mut() {
                        y = b.run(y);
                    }
                    if enhance {
                        let (low, rest) = self.bands[s].run(y);
                        let (mid, high) = self.bands[2 + s].run(rest);
                        let mut out = 0.0;
                        for (k, band) in [low, mid, high].into_iter().enumerate() {
                            let e = self.comp[k].run(band.abs());
                            let g = if e > thr { (thr / e).powf(1.0 - 1.0 / ratio) } else { 1.0 };
                            out += band * g * if k == 1 { 1.2 } else { 1.0 };
                        }
                        y = out * mk;
                    }
                }
                *x = y;
            }
            if s == 0
                && let Some(g) = shown
            {
                self.shown = g;
            }
        }
    }
    fn latency(&self) -> usize {
        NFFT
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        if name == "reduction" { self.shown.clone() } else { Vec::new() }
    }
}
plugin_impl!(Denoiser);

// ---------------------------------------------------------------------------------------------
// Grooveshifter

static GS_P: [Param; 5] = [
    Param::menu(0, "Mode", &["Beat", "Tonal"], 0.0),
    Param::new(1, "Grain Size", 0.0, 20.0, 0.0, Unit::Ms),
    Param::menu(2, "Grid", &["1/8", "1/16"], 0.0),
    Param::new(3, "Swing", 50.0, 75.0, 50.0, Unit::Percent),
    Param::new(4, "Accent", -12.0, 12.0, 0.0, Unit::Db),
];

pub struct Grooveshifter {
    p: Values,
    rate: f32,
    clock: Clock,
    line: [Delay; 2],
    cur: f32,
    from: f32,
    fade: f32,
    acc: f32,
}

impl Grooveshifter {
    pub fn new(rate: f32) -> Grooveshifter {
        let n = (2.0 * rate) as usize;
        Grooveshifter { p: Values::new(&GS_P), rate, clock: Clock::default(), line: [Delay::new(n), Delay::new(n)], cur: 0.0, from: 0.0, fade: 1.0, acc: 1.0 }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.cur = 0.0;
        self.from = 0.0;
        self.fade = 1.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The crossfade (ms) at each splice: Beat a short click guard; Tonal the grain (Auto
    /// follows the division's length).
    pub fn grain_ms(&self, sub_s: f32) -> f32 {
        if self.p.idx(0) == 0 {
            1.5
        } else if self.p.get(1) < 1.0 {
            (sub_s * 1000.0 / 8.0).clamp(1.0, 20.0)
        } else {
            self.p.get(1)
        }
    }
    /// Where a position (beats) is moved: the delay (s) applied to it and whether it is an
    /// even (swung) division. Even divisions start `(swing − 50 %) × 2` divisions late.
    pub fn shift(&self, beats: f64, bpm: f64) -> (f32, bool) {
        let div = if self.p.idx(2) == 0 { 0.5 } else { 0.25 };
        let pair = beats.rem_euclid(div * 2.0);
        let late = (self.p.get(3) as f64 / 100.0 - 0.5) * 2.0 * div;
        if pair >= div {
            ((late * 60.0 / bpm.max(1.0)) as f32, pair >= div + late)
        } else {
            (0.0, false)
        }
    }
}

impl Effect for Grooveshifter {
    fn set_tempo(&mut self, bpm: f64) {
        self.clock.set_tempo(bpm, self.rate);
    }
    fn set_transport(&mut self, t: Transport) {
        self.clock.sync(t, self.rate);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sub_s = (if self.p.idx(2) == 0 { 0.5 } else { 0.25 }) * 60.0 / self.clock.bpm as f32;
        let fade_step = 1.0 / (self.grain_ms(sub_s) * 0.001 * self.rate).max(1.0);
        let accent = db(self.p.get(4));
        for i in 0..left.len().min(right.len()) {
            let b = self.clock.tick();
            let (d, even) = self.shift(b, self.clock.bpm);
            let d = d * self.rate;
            if (d - self.cur).abs() > 0.5 {
                // a splice: crossfade from the old read position to the new one
                self.from = self.cur;
                self.cur = d;
                self.fade = 0.0;
            }
            let target_acc = if even { accent } else { 1.0 };
            self.acc += (target_acc - self.acc) * 0.002;
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                self.line[s].push(*x);
                let new = self.line[s].read_lin(self.cur.max(0.0));
                let y = if self.fade < 1.0 { let old = self.line[s].read_lin(self.from.max(0.0)); old * (1.0 - self.fade) + new * self.fade } else { new };
                *x = flush(y * self.acc);
            }
            if self.fade < 1.0 {
                self.fade = (self.fade + fade_step).min(1.0);
            }
        }
    }
}
plugin_impl!(Grooveshifter);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    fn sine(n: usize, hz: f32, a: f32) -> Vec<f32> {
        (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin() * a).collect()
    }
    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn gain_at(fx: &mut dyn Effect, hz: f32) -> f32 {
        fx.reset();
        let mut l = sine(24000, hz, 0.1);
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        lin_db(rms(&l[12000..]) / rms(&sine(24000, hz, 0.1)[12000..]))
    }

    #[test]
    fn all_run_finite() {
        for n in NAMES {
            let mut fx = by_name(n, 48000.0).unwrap();
            let mut l = sine(9600, 330.0, 0.5);
            let mut r = sine(9600, 220.0, 0.5);
            fx.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 50.0), "{n}");
        }
    }

    #[test]
    fn silver_compressor_reduces_above_threshold() {
        let mut c = SilverCompressor::new(48000.0);
        c.set(0, -20.0);
        c.set(1, 4.0);
        assert!((c.curve(-8.0) - 9.0).abs() < 1e-3);
        let mut l = sine(48000, 1000.0, 1.0);
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert!(c.gain_reduction() > 6.0);
        assert!(rms(&l[24000..]) < 0.5 * rms(&sine(48000, 1000.0, 1.0)));
    }

    #[test]
    fn silver_gate_closes_below_threshold_and_reports_lookahead() {
        let mut g = SilverGate::new(48000.0);
        g.set(1, -20.0);
        g.set(0, 5.0);
        assert_eq!(g.latency(), 240);
        let mut l = sine(24000, 500.0, 0.01);
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        assert!(rms(&l[12000..]) < 1e-4);
        let mut l = sine(24000, 500.0, 0.5);
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        assert!(rms(&l[12000..]) > 0.3);
    }

    #[test]
    fn eqs_shape_as_named() {
        let mut s = Eq::new(48000.0, EqKind::Silver);
        s.set(4, 12.0);
        s.set(2, 1000.0);
        assert!((gain_at(&mut s, 1000.0) - 12.0).abs() < 0.5);
        let mut dj = Eq::new(48000.0, EqKind::Dj);
        dj.set(5, -30.0);
        assert!(gain_at(&mut dj, 40.0) < -25.0);
        let mut hc = Eq::new(48000.0, EqKind::HighCut);
        hc.set(0, 1000.0);
        assert!(gain_at(&mut hc, 8000.0) < -15.0);
        assert!(gain_at(&mut hc, 100.0) > -0.5);
        let mut lp = PassFilter::new(48000.0, false);
        lp.set(0, 1000.0);
        lp.set(1, 1.0);
        let one = gain_at(&mut lp, 4000.0);
        lp.set(1, 4.0);
        let four = gain_at(&mut lp, 4000.0);
        assert!(four < one - 20.0, "order 1 {one} order 4 {four}");
    }

    #[test]
    fn fat_eq_bands_and_pass_slope() {
        let mut f = FatEq::new(48000.0);
        f.set(13, 6.0); // band 3 gain
        assert!((f.response_db(None, 1000.0) - 6.0).abs() < 0.3);
        f.set(0, 1.0); // band 1 on: High Pass at 80 Hz
        let gentle = f.response_db(Some(0), 20.0);
        f.set(4, 10.0); // Q = steeper
        assert!(f.response_db(Some(0), 20.0) < gentle - 10.0);
        f.set(1, 1.0); // Low Shelf
        assert_eq!(f.band_type(0), "Low Shelf");
        f.set(25, -6.0);
        assert!((f.response_db(None, 1000.0) - 0.0).abs() < 0.5);
    }

    #[test]
    fn deesser_reduces_sibilance_only() {
        let mut d = DeEsser::new(48000.0);
        d.set(4, 20.0);
        let hi = gain_at(&mut d, 6000.0);
        let lo = gain_at(&mut d, 300.0);
        assert!(hi < -3.0, "6 kHz {hi}");
        assert!(lo.abs() < 0.5, "300 Hz {lo}");
        d.set(2, 0.0); // Det: the detector band
        assert!(gain_at(&mut d, 300.0) < -20.0);
    }

    #[test]
    fn ducker_ducks_under_the_side_chain_and_mixes_it_in() {
        let mut d = Ducker::new(48000.0);
        d.set(6, 0.0);
        let n = 24000;
        let mut l = vec![0.5; n];
        let mut r = vec![0.5; n];
        let voice = sine(n, 300.0, 0.3);
        d.side_chain(&voice, &voice);
        d.process(&mut l, &mut r);
        // music down 12 dB plus the voice
        let music: Vec<f32> = l[20000..].iter().zip(&voice[20000..]).map(|(o, v)| o - v).collect();
        assert!((music.iter().sum::<f32>() / music.len() as f32 - 0.5 * db(-12.0)).abs() < 0.01);
        // look-ahead: reported latency
        d.set(6, 1.0);
        assert_eq!(d.latency(), (DUCK_LOOKAHEAD_S * 48000.0) as usize);
    }

    #[test]
    fn reverbs_follow_room_and_time() {
        let a = reflections(3, 10.0, 2.0);
        let b = reflections(7, 10.0, 2.0);
        assert_eq!(a[0].len(), 3 + 3 * 2);
        assert_eq!(b[0].len(), 7 + 7 * 6);
        let big = reflections(5, 20.0, 2.0);
        let small = reflections(5, 5.0, 2.0);
        assert!(big[0][0].0 > small[0][0].0);
        // stereo base: wider = the two mics hear different times
        let narrow = reflections(5, 10.0, 0.0);
        assert!(narrow[0].iter().zip(&narrow[1]).all(|(x, y)| (x.0 - y.0).abs() < 1e-6));
        let wide = reflections(5, 10.0, 6.0);
        assert!(wide[0].iter().zip(&wide[1]).any(|(x, y)| (x.0 - y.0).abs() > 1e-3));
        // a longer Reverb Time rings longer
        let ring = |t: f32| {
            let mut v = Verb::new(48000.0, VerbKind::Gold);
            v.set(8, t);
            v.set(11, 100.0);
            let mut l = vec![0.0; 96000];
            l[0] = 1.0;
            let mut r = l.clone();
            v.process(&mut l, &mut r);
            rms(&l[48000..])
        };
        assert!(ring(4.0) > 3.0 * ring(0.5));
        // Platinum's Low Ratio shortens the low band
        let mut p = Verb::new(48000.0, VerbKind::Platinum);
        p.set(12, 50.0);
        assert_eq!(p.times(), (2.0, 1.0));
    }

    #[test]
    fn bass_amp_models_emphasize_as_the_guide_says() {
        let mut b = BassAmp::new(48000.0);
        b.set(0, 1.0); // American Deep
        assert!(b.response_db(500.0) > b.response_db(1500.0) + 3.0);
        b.set(0, 6.0); // Top Class DI Warm
        assert!(b.response_db(1600.0) < b.response_db(100.0) - 3.0);
        b.set(0, 8.0); // DI Mid: nearly linear
        assert!((b.response_db(1000.0) - b.response_db(200.0)).abs() < 1.0);
        b.set(3, 10.0);
        b.set(4, 1000.0);
        assert!(b.response_db(1000.0) > 9.0);
    }

    #[test]
    fn guitar_amp_pro_links_and_di_box() {
        let mut g = GuitarAmpPro::new(48000.0);
        g.set(0, 4.0); // US Hot Top 100W
        assert_eq!(g.get(1) as usize, GAP_ASSOC[4].1);
        assert_eq!(g.get(2) as usize, GAP_ASSOC[4].2);
        g.set(3, 0.0);
        g.set(4, 0.0);
        g.set(1, 0.0);
        g.set(0, 2.0);
        assert_eq!(g.get(1), 0.0, "links off: the speaker stays");
        g.set(15, 1.0);
        g.set(16, 4.0); // 1/4 at 120 bpm = 2 Hz
        g.set_tempo(120.0);
        assert!((g.speed_hz() - 2.0).abs() < 1e-3);
        assert_eq!(GAP_AMPS.len(), 11);
        assert_eq!(GAP_SPEAKERS.len(), 15);
    }

    #[test]
    fn denoiser_removes_quiet_noise_keeps_loud_tone() {
        let mut d = Denoiser::new(48000.0, false);
        d.set(0, -40.0);
        d.set(1, -40.0);
        let mut nz = crate::util::Noise(77);
        let mut l: Vec<f32> = (0..48000).map(|_| nz.next() * 0.001).collect();
        let mut r = l.clone();
        let before = rms(&l);
        d.process(&mut l, &mut r);
        assert!(rms(&l[24000..]) < before * 0.1);
        let mut d = Denoiser::new(48000.0, false);
        d.set(0, -40.0);
        let mut l = sine(48000, 1000.0, 0.5);
        let mut r = l.clone();
        d.process(&mut l, &mut r);
        let out = rms(&l[NFFT * 4..]);
        assert!((out / rms(&sine(48000, 1000.0, 0.5)) - 1.0).abs() < 0.1, "{out}");
    }

    #[test]
    fn grooveshifter_moves_even_divisions() {
        let mut g = Grooveshifter::new(48000.0);
        g.set(3, 75.0);
        // 1/8 at 120 bpm: the second eighth of each pair starts 1/16 (0.125 s) late
        let (d, _) = g.shift(0.5, 120.0);
        assert!((d - 0.125).abs() < 1e-4);
        assert_eq!(g.shift(0.25, 120.0).0, 0.0);
        g.set(2, 1.0);
        let (d, _) = g.shift(0.3, 120.0);
        assert!((d - 0.0625).abs() < 1e-4);
        g.set(3, 50.0);
        assert_eq!(g.shift(0.3, 120.0).0, 0.0);
    }
}
