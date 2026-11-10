//! The Vintage EQ collection: Vintage Graphic EQ ("Graph EQ" in projects), Vintage Tube EQ
//! ("Tube EQ") and Vintage Console EQ ("Cnsl EQ") — stand-ins for Logic's three classic-hardware
//! equalisers, each with the shared output section (Drive, Output Model, Phase, Volume).
//!
//! - Graphic: ten bands at 31 Hz-16 kHz, ±12 dB, with proportional Q (the more a band is
//!   boosted or cut, the narrower it gets), as on the console graphic EQ it echoes. Tune moves all
//!   centres together.
//! - Tube: the two-panel passive program EQ. Upper panel: Low Boost and Low Atten at the same
//!   switched frequency (boosting and cutting together gives the famous bass "shelf with a dip"),
//!   High Boost (a peak whose width the Bandwidth knob sets) and High Atten (a shelf at 5, 10 or
//!   20 kHz). Lower panel: Low Peak, Dip and High Peak, each at a switched frequency.
//! - Console: the British channel EQ: high shelf at 12 kHz, a peaking mid at switched
//!   frequencies (0.36-7.2 kHz), low shelf (35-220 Hz), and a high-pass filter.
//! - Output: Drive (1-10) feeds a transformer / tube-stage saturation; Output Model picks its
//!   flavour; Phase "Linear" is approximated (these are minimum-phase filters here).
//!
//! Parameter positions are from the projects' raw floats; band gains and the switches are
//! "likely" in the research (defaults are flat, so not every position is pinned yet).

use crate::util::{Biquad, Coefs, Values, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

const MODELS: &[&str] = &["Silky", "Punchy", "Clean"];
const PHASES: &[&str] = &["Natural", "Linear", "Linear (2)"];

/// The shared output section: drive into a model's saturation, then volume.
#[derive(Clone, Copy, Default)]
struct Output {
    drive: f32,
    model: usize,
    vol: f32,
    dc: [f32; 2],
    dck: f32,
}

impl Output {
    fn set(&mut self, rate: f32, drive: f32, model: usize, vol_db: f32) {
        // Drive 1..10: 5 is a gentle colour, 10 clearly driven
        self.drive = ((drive - 1.0) / 9.0).clamp(0.0, 1.0);
        self.model = model;
        self.vol = db(vol_db);
        self.dck = 1.0 - (-crate::util::TAU * 8.0 / rate).exp();
    }
    #[inline]
    fn run(&mut self, ch: usize, x: f32) -> f32 {
        let k = 0.2 + 4.0 * self.drive * self.drive;
        let y = match self.model {
            // Silky: tube stage, mostly 2nd harmonic
            0 => {
                let u = x * k;
                (u + 0.25 * u * u / (1.0 + u * u)).tanh() / k
            }
            // Punchy: transformer, odd harmonics and a firmer knee
            1 => {
                let u = x * k;
                (u / (1.0 + u.abs().powf(2.5)).powf(0.4)) / k
            }
            _ => x,
        };
        self.dc[ch] = flush(self.dc[ch] + self.dck * (y - self.dc[ch]));
        (y - self.dc[ch]) * self.vol
    }
}

fn clear_all(bq: &mut [[Biquad; 2]]) {
    for b in bq.iter_mut().flatten() {
        b.clear();
    }
}

// ---------------------------------------------------------------------------------------------
// Vintage Graphic EQ

const GEQ_FREQS: [f32; 10] = [31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
const GEQ_NAMES: [&str; 10] = ["31 Hz", "63 Hz", "125 Hz", "250 Hz", "500 Hz", "1.0 kHz", "2.0 kHz", "4.0 kHz", "8.0 kHz", "16.0 kHz"];

static GEQ_PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v: Vec<Param> = (0..10).map(|i| Param::new(i as u32, GEQ_NAMES[i], -12.0, 12.0, 0.0, Unit::Db)).collect();
    v.push(Param::new(10, "Tune", -50.0, 50.0, 0.0, Unit::Percent));
    v.push(Param::new(11, "Volume", -24.0, 24.0, 0.0, Unit::Db));
    v.push(Param::new(12, "IN", 0.0, 1.0, 1.0, Unit::OnOff));
    v.push(Param::new(13, "Drive", 1.0, 10.0, 5.0, Unit::None));
    v.push(Param::menu(14, "Output Model", MODELS, 1.0));
    v.push(Param::menu(15, "Phase", PHASES, 0.0));
    v
});

pub struct GraphicEq {
    p: Values,
    rate: f32,
    bands: [[Biquad; 2]; 10],
    out: Output,
}

impl GraphicEq {
    pub fn new(rate: f32) -> GraphicEq {
        let mut g = GraphicEq { p: Values::new(&GEQ_PARAMS), rate, bands: [[Biquad::default(); 2]; 10], out: Output::default() };
        g.update();
        g
    }
    fn update(&mut self) {
        let tune = 1.0 + self.p.get(10) / 100.0;
        for i in 0..10 {
            let g = self.p.get(i as u32);
            // proportional Q: broad (0.7) for small moves, ~2.5 at full boost or cut
            let q = 0.7 + 1.8 * (g.abs() / 12.0);
            let c = if g.abs() < 0.01 { Coefs::THRU } else { Coefs::peak(self.rate, (GEQ_FREQS[i] * tune).min(self.rate * 0.45), q, g) };
            self.bands[i][0].c = c;
            self.bands[i][1].c = c;
        }
        self.out.set(self.rate, self.p.get(13), self.p.idx(14), self.p.get(11));
    }
    fn clear(&mut self) {
        clear_all(&mut self.bands);
    }
    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl lg_dsp::Effect for GraphicEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(12) {
            return;
        }
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut v = *x;
                for b in self.bands.iter_mut() {
                    v = b[s].run(v);
                }
                *x = self.out.run(s, v);
            }
        }
    }
}

plugin_impl!(GraphicEq);

// ---------------------------------------------------------------------------------------------
// Vintage Tube EQ

static TUBE_PARAMS: [Param; 20] = [
    Param::new(1, "IN (Program EQ)", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Low Boost", 0.0, 10.0, 0.0, Unit::None),
    Param::new(3, "Low Atten", 0.0, 10.0, 0.0, Unit::None),
    Param::new(4, "Low Freq", 20.0, 100.0, 60.0, Unit::Hz),
    Param::new(5, "High Boost", 0.0, 10.0, 0.0, Unit::None),
    Param::new(6, "High Atten Sel", 5.0, 20.0, 20.0, Unit::None),
    Param::new(7, "High Atten", 0.0, 10.0, 0.0, Unit::None),
    Param::new(8, "High Freq", 3.0, 16.0, 6.0, Unit::None),
    Param::new(9, "High Bandwidth", 0.0, 10.0, 0.5, Unit::None),
    Param::new(11, "IN (Mid EQ)", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(12, "Low Peak", 0.0, 10.0, 0.0, Unit::None),
    Param::new(13, "Low Peak Freq", 200.0, 1000.0, 200.0, Unit::Hz),
    Param::new(14, "Dip", 0.0, 10.0, 0.0, Unit::None),
    Param::new(15, "Dip Freq", 0.2, 7.0, 0.7, Unit::None),
    Param::new(17, "High Peak", 0.0, 10.0, 0.0, Unit::None),
    Param::new(18, "High Peak Freq", 1.5, 5.0, 1.5, Unit::None),
    Param::new(21, "Drive", 1.0, 10.0, 4.0, Unit::None),
    Param::menu(22, "Output Model", MODELS, 0.0),
    Param::menu(19, "Phase", PHASES, 0.0),
    Param::new(20, "Volume", -24.0, 24.0, 0.0, Unit::Db),
];

pub struct TubeEq {
    p: Values,
    rate: f32,
    f: [[Biquad; 2]; 7],
    out: Output,
}

impl TubeEq {
    pub fn new(rate: f32) -> TubeEq {
        let mut t = TubeEq { p: Values::new(&TUBE_PARAMS), rate, f: [[Biquad::default(); 2]; 7], out: Output::default() };
        t.update();
        t
    }
    fn update(&mut self) {
        let (p, r) = (&self.p, self.rate);
        let mut c = [Coefs::THRU; 7];
        if p.on(1) {
            let lf = p.get(4);
            // passive network: boost is a broad low shelf, atten a slightly higher, narrower
            // cut — both at once give the boost-with-dip curve
            c[0] = Coefs::low_shelf(r, lf, 0.5, p.get(2) * 1.35);
            c[1] = Coefs::low_shelf(r, lf * 1.6, 0.6, -p.get(3) * 1.75);
            let bw = p.get(9) / 10.0;
            c[2] = Coefs::peak(r, (p.get(8) * 1000.0).min(r * 0.45), 2.5 - 2.1 * bw, p.get(5) * 1.8);
            c[3] = Coefs::high_shelf(r, (p.get(6) * 1000.0 * 0.5).min(r * 0.45), 0.6, -p.get(7) * 1.6);
        }
        if p.on(11) {
            c[4] = Coefs::peak(r, p.get(13), 1.0, p.get(12));
            c[5] = Coefs::peak(r, (p.get(15) * 1000.0).min(r * 0.45), 1.2, -p.get(14));
            c[6] = Coefs::peak(r, (p.get(18) * 1000.0).min(r * 0.45), 1.0, p.get(17));
        }
        for i in 0..7 {
            self.f[i][0].c = c[i];
            self.f[i][1].c = c[i];
        }
        self.out.set(r, p.get(21), p.idx(22), p.get(20));
    }
    fn clear(&mut self) {
        clear_all(&mut self.f);
    }
    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl lg_dsp::Effect for TubeEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut v = *x;
                for b in self.f.iter_mut() {
                    v = b[s].run(v);
                }
                *x = self.out.run(s, v);
            }
        }
    }
}

plugin_impl!(TubeEq);

// ---------------------------------------------------------------------------------------------
// Vintage Console EQ

static CONSOLE_PARAMS: [Param; 12] = [
    Param::new(0, "High Gain", -16.0, 16.0, 0.0, Unit::Db),
    Param::new(1, "Mid Gain", -18.0, 18.0, 0.0, Unit::Db),
    Param::new(2, "Mid Freq", 0.36, 7.2, 3.2, Unit::None),
    Param::new(3, "Low Gain", -16.0, 16.0, 0.0, Unit::Db),
    Param::new(4, "Low Freq", 35.0, 220.0, 110.0, Unit::Hz),
    Param::new(5, "High Pass", 50.0, 300.0, 50.0, Unit::Hz),
    Param::new(6, "IN", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(7, "High Pass On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(8, "Mid Q High", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(12, "Drive", 1.0, 10.0, 4.0, Unit::None),
    Param::menu(13, "Output Model", MODELS, 1.0),
    Param::new(9, "Volume", -24.0, 24.0, 0.0, Unit::Db),
];

pub struct ConsoleEq {
    p: Values,
    rate: f32,
    f: [[Biquad; 2]; 5],
    out: Output,
}

impl ConsoleEq {
    pub fn new(rate: f32) -> ConsoleEq {
        let mut c = ConsoleEq { p: Values::new(&CONSOLE_PARAMS), rate, f: [[Biquad::default(); 2]; 5], out: Output::default() };
        c.update();
        c
    }
    fn update(&mut self) {
        let (p, r) = (&self.p, self.rate);
        let mut c = [Coefs::THRU; 5];
        if p.on(6) {
            c[0] = Coefs::high_shelf(r, 12000.0f32.min(r * 0.45), 0.6, p.get(0));
            let q = if p.on(8) { 1.4 } else { 0.7 };
            c[1] = Coefs::peak(r, (p.get(2) * 1000.0).min(r * 0.45), q, p.get(1));
            // the inductor low shelf has a small overshoot above its corner (Q above 0.707)
            c[2] = Coefs::low_shelf(r, p.get(4), 0.9, p.get(3));
            if p.on(7) {
                // an 18 dB/oct high-pass: a first-order plus a second-order section
                c[3] = Coefs::hp1(r, p.get(5));
                c[4] = Coefs::highpass(r, p.get(5), 1.0);
            }
        }
        for i in 0..5 {
            self.f[i][0].c = c[i];
            self.f[i][1].c = c[i];
        }
        self.out.set(r, p.get(12), p.idx(13), p.get(9));
    }
    fn clear(&mut self) {
        clear_all(&mut self.f);
    }
    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl lg_dsp::Effect for ConsoleEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut v = *x;
                for b in self.f.iter_mut() {
                    v = b[s].run(v);
                }
                *x = self.out.run(s, v);
            }
        }
    }
}

plugin_impl!(ConsoleEq);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    fn gain_at(fx: &mut dyn Effect, f: f32) -> f32 {
        let x: Vec<f32> = (0..24000).map(|i| 0.05 * (crate::util::TAU * f * i as f32 / 48000.0).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        fx.process(&mut l, &mut r);
        let rms = |v: &[f32]| (v[12000..].iter().map(|a| a * a).sum::<f32>() / 12000.0).sqrt();
        lg_dsp::gain_to_db(rms(&l) / rms(&x))
    }

    #[test]
    fn graphic_band_boosts_its_frequency() {
        let mut g = GraphicEq::new(48000.0);
        g.set(13, 1.0);
        assert!(gain_at(&mut g, 1000.0).abs() < 0.3);
        g.set(5, 12.0);
        let b = gain_at(&mut g, 1000.0);
        assert!((b - 12.0).abs() < 0.8, "{b}");
    }

    #[test]
    fn console_low_shelf_and_flat_default() {
        let mut c = ConsoleEq::new(48000.0);
        c.set(12, 1.0);
        assert!(gain_at(&mut c, 1000.0).abs() < 0.3);
        c.set(3, 10.0);
        c.set(7, 0.0);
        assert!(gain_at(&mut c, 40.0) > 7.0);
    }

    #[test]
    fn tube_low_boost_lifts_bass() {
        let mut t = TubeEq::new(48000.0);
        t.set(21, 1.0);
        let flat = gain_at(&mut t, 30.0);
        t.set(2, 10.0);
        assert!(gain_at(&mut t, 30.0) > flat + 8.0);
    }
}
