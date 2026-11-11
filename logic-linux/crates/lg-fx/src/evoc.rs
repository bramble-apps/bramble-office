//! EVOC 20 Filterbank and EVOC 20 TrackOscillator, with the guide's controls.
//!
//! - Filterbank: two banks (A and B) of up to 20 band-pass filters between Low and High
//!   Frequency, a level fader per band, Formant Shift, Lowest (BP / HP) and Highest (BP / LP),
//!   Resonance, Boost A / B, Slope (6 / 12 dB), Fade A↔B, a Shift LFO and a Fade LFO (rate,
//!   waveform, intensity), Overdrive, Level, Stereo Width.
//! - TrackOscillator: a vocoder whose analysis is the track's input and whose synthesis source
//!   is the input itself or the tracking oscillator (a two-operator FM oscillator following the
//!   input's pitch, quantised to a root / scale with strength and glide, Max Track); Attack,
//!   Release, Freeze, Bands; U/V detection (Sensitivity, Noise / Noise + Synth / Blend, Level);
//!   Formant Stretch / Shift, Resonance, Low / High Frequency, Lowest / Highest; an LFO for
//!   formant shift and pitch (vibrato); Signal (Vocoder / Synthesis / Analysis), Level, Stereo
//!   Width. Side-chain inputs aren't available in Studio, so SideChain falls back to Track, as
//!   Logic does with no side chain assigned.
//!
//! Ids are our own from 1000 (the blocks aren't decoded).

use crate::kit::{Svf, yin};
use crate::util::{Follower, Noise, Phase, Smooth, Values, db, plugin_impl, sine01, tri};
use lg_dsp::{Effect, Param, Unit};

const B: u32 = 1000;
const MAXB: usize = 20;
const WAVES: &[&str] = &["Triangle", "Saw Down", "Saw Up", "Square", "Sample & Hold", "Smooth Random"];

fn lfo_wave(w: usize, p: f32, hold: &mut (f32, f32), noise: &mut Noise) -> f32 {
    match w {
        0 => tri(p),
        1 => 1.0 - 2.0 * p,
        2 => 2.0 * p - 1.0,
        3 => if p < 0.5 { 1.0 } else { -1.0 },
        _ => {
            if p < hold.1 {
                hold.0 = noise.next();
            }
            hold.1 = p;
            hold.0
        }
    }
}

/// Band centre frequencies between lo and hi, shifted by `shift` octaves and stretched.
fn centres(n: usize, lo: f32, hi: f32, shift: f32, stretch: f32, rate: f32) -> Vec<f32> {
    let n = n.clamp(1, MAXB);
    let (lo, hi) = (lo.max(20.0), hi.max(lo * 1.1));
    let mid = (lo * hi).sqrt();
    (0..n).map(|k| {
        let t = if n == 1 { 0.5 } else { k as f32 / (n - 1) as f32 };
        let f = lo * (hi / lo).powf(t);
        let f = mid * (f / mid).powf(stretch);
        (f * 2f32.powf(shift)).clamp(20.0, rate * 0.45)
    }).collect()
}

// ---------------------------------------------------------------------------------------------
// Filterbank

static FB_P: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::new(B, "Low Frequency", 20.0, 2000.0, 100.0, Unit::Hz),
        Param::new(B + 1, "High Frequency", 1000.0, 20000.0, 8000.0, Unit::Hz),
        Param::new(B + 2, "Formant Shift", -3.0, 3.0, 0.0, Unit::Octaves),
        Param::new(B + 3, "Bands", 5.0, 20.0, 20.0, Unit::None),
        Param::menu(B + 4, "Lowest", &["BP", "HP"], 0.0),
        Param::menu(B + 5, "Highest", &["BP", "LP"], 0.0),
        Param::new(B + 6, "Resonance", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(B + 7, "Boost A", -24.0, 24.0, 0.0, Unit::Db),
        Param::new(B + 8, "Boost B", -24.0, 24.0, 0.0, Unit::Db),
        Param::menu(B + 9, "Slope", &["1", "2"], 1.0),
        Param::new(B + 10, "Fade A/B", 0.0, 100.0, 0.0, Unit::Percent),
        Param::new(B + 11, "Shift LFO Intensity", 0.0, 3.0, 0.0, Unit::Octaves),
        Param::new(B + 12, "Shift LFO Rate", 0.01, 20.0, 0.5, Unit::Hz),
        Param::menu(B + 13, "Shift LFO Wave", WAVES, 0.0),
        Param::new(B + 14, "Fade LFO Intensity", 0.0, 100.0, 0.0, Unit::Percent),
        Param::new(B + 15, "Fade LFO Rate", 0.01, 20.0, 0.5, Unit::Hz),
        Param::menu(B + 16, "Fade LFO Wave", WAVES, 0.0),
        Param::new(B + 17, "Overdrive", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(B + 18, "Level", -36.0, 12.0, 0.0, Unit::Db),
        Param::new(B + 19, "Stereo Width", 0.0, 100.0, 50.0, Unit::Percent),
    ];
    for bank in 0..2 {
        for k in 0..MAXB {
            v.push(Param::new(B + 100 + 100 * bank + k as u32, crate::kit::leak(format!("Bank {} Band {}", ["A", "B"][bank as usize], k + 1)), 0.0, 1.0, 1.0, Unit::None));
        }
    }
    v
});

pub struct Filterbank {
    p: Values,
    rate: f32,
    f: [[[Svf; 2]; MAXB]; 2],
    cs: Vec<f32>,
    q: f32,
    lfo: [Phase; 2],
    hold: [(f32, f32); 2],
    noise: Noise,
    n: u32,
}

impl Filterbank {
    pub fn new(rate: f32) -> Filterbank {
        let mut f = Filterbank { p: Values::new(FB_P.as_slice()), rate, f: [[[Svf::default(); 2]; MAXB]; 2], cs: Vec::new(), q: 1.0, lfo: [Phase::default(); 2], hold: [(0.0, 0.0); 2], noise: Noise(4242), n: 0 };
        f.update();
        f
    }
    fn tune(&mut self, shift_extra: f32) {
        let n = self.p.get(B + 3).round() as usize;
        self.cs = centres(n, self.p.get(B), self.p.get(B + 1), self.p.get(B + 2) + shift_extra, 1.0, self.rate);
        let q = 1.0 + 20.0 * (self.p.get(B + 6) / 100.0).powi(2) + n as f32 * 0.3;
        self.q = q;
        for (k, c) in self.cs.iter().enumerate() {
            for s in 0..2 {
                for st in 0..2 {
                    self.f[s][k][st].set(self.rate, *c, q);
                }
            }
        }
    }
    fn update(&mut self) {
        self.tune(0.0);
    }
    fn clear(&mut self) {
        for x in self.f.iter_mut().flatten().flatten() {
            x.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl Effect for Filterbank {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let nb = self.cs.len();
        let lvl = db(self.p.get(B + 18));
        let (ba, bb) = (db(self.p.get(B + 7)), db(self.p.get(B + 8)));
        let steep = self.p.idx(B + 9) == 1;
        let width = self.p.get(B + 19) / 100.0;
        let od = self.p.on(B + 17);
        let (lowhp, highlp) = (self.p.idx(B + 4) == 1, self.p.idx(B + 5) == 1);
        let ga: Vec<f32> = (0..nb).map(|k| self.p.get(B + 100 + k as u32)).collect();
        let gb: Vec<f32> = (0..nb).map(|k| self.p.get(B + 200 + k as u32)).collect();
        for i in 0..left.len().min(right.len()) {
            // the LFOs: formant shift (re-tuned every 64 samples) and the A/B fade
            let ps = self.lfo[0].step(self.rate, self.p.get(B + 12));
            let pf = self.lfo[1].step(self.rate, self.p.get(B + 15));
            self.n += 1;
            let si = self.p.get(B + 11);
            if si > 0.0 && self.n % 64 == 0 {
                let m = lfo_wave(self.p.idx(B + 13), ps, &mut self.hold[0], &mut self.noise);
                self.tune(si * m);
            }
            let fm = lfo_wave(self.p.idx(B + 16), pf, &mut self.hold[1], &mut self.noise) * self.p.get(B + 14) / 100.0;
            let fade = (self.p.get(B + 10) / 100.0 + 0.5 * fm).clamp(0.0, 1.0);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let inp = *x;
                let mut y = 0.0;
                for k in 0..nb {
                    let o = self.f[s][k][0].run(inp);
                    let mut band = if k == 0 && lowhp { o.high } else if k == nb - 1 && highlp { o.low } else { o.band };
                    if steep {
                        let o2 = self.f[s][k][1].run(band);
                        band = if k == 0 && lowhp { o2.high } else if k == nb - 1 && highlp { o2.low } else { o2.band };
                    }
                    // each band normalised to unity at its centre
                    band /= if steep { self.q * self.q } else { self.q };
                    let g = ga[k] * ba * (1.0 - fade) + gb[k] * bb * fade;
                    // Stereo Width: odd bands lean left, even right
                    let side = if (k % 2 == 0) == (s == 0) { 1.0 } else { 1.0 - width };
                    y += band * g * side;
                }
                if od {
                    y = (y * 2.0).tanh();
                }
                *x = y * lvl;
            }
        }
    }
}

plugin_impl!(Filterbank);

// ---------------------------------------------------------------------------------------------
// TrackOscillator (vocoder)

const SCALES: &[(&str, [bool; 12])] = &[
    ("Chromatic", [true; 12]),
    ("Major", [true, false, true, false, true, true, false, true, false, true, false, true]),
    ("Minor", [true, false, true, true, false, true, false, true, true, false, true, false]),
    ("Major Chord", [true, false, false, false, true, false, false, true, false, false, false, false]),
    ("Minor Chord", [true, false, false, true, false, false, false, true, false, false, false, false]),
];

static TO_P: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let scales: &'static [&'static str] = Box::leak(SCALES.iter().map(|s| s.0).collect::<Vec<_>>().into_boxed_slice());
    vec![
        Param::new(B, "Attack", 1.0, 200.0, 5.0, Unit::Ms),
        Param::new(B + 1, "Release", 5.0, 2000.0, 80.0, Unit::Ms),
        Param::new(B + 2, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(B + 3, "Bands", 5.0, 20.0, 20.0, Unit::None),
        Param::menu(B + 4, "Analysis In", &["Track", "SideChain"], 0.0),
        Param::new(B + 5, "U/V Sensitivity", 0.0, 100.0, 50.0, Unit::Percent),
        Param::menu(B + 6, "U/V Mode", &["Noise", "N + Syn", "Blend"], 0.0),
        Param::new(B + 7, "U/V Level", 0.0, 100.0, 30.0, Unit::Percent),
        Param::menu(B + 8, "Synthesis In", &["Track", "SideChain", "Osc."], 2.0),
        Param::new(B + 9, "FM Ratio", 0.5, 8.0, 1.0, Unit::None),
        Param::new(B + 10, "FM Int", 0.0, 100.0, 30.0, Unit::Percent),
        Param::new(B + 11, "Coarse Tune", -36.0, 36.0, 0.0, Unit::Semitones),
        Param::new(B + 12, "Fine Tune", -20.0, 20.0, 0.0, Unit::Hz),
        Param::new(B + 13, "Pitch Quantize Strength", 0.0, 100.0, 0.0, Unit::Percent),
        Param::new(B + 14, "Pitch Quantize Glide", 0.0, 1000.0, 20.0, Unit::Ms),
        Param::menu(B + 15, "Root", &["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"], 0.0),
        Param::menu(B + 16, "Scale", scales, 0.0),
        Param::new(B + 17, "Max Track", 100.0, 2000.0, 1000.0, Unit::Hz),
        Param::new(B + 18, "Low Frequency", 20.0, 2000.0, 100.0, Unit::Hz),
        Param::new(B + 19, "High Frequency", 1000.0, 20000.0, 8000.0, Unit::Hz),
        Param::menu(B + 20, "Lowest", &["BP", "HP"], 0.0),
        Param::menu(B + 21, "Highest", &["BP", "LP"], 0.0),
        Param::new(B + 22, "Formant Stretch", 0.3, 2.0, 1.0, Unit::None),
        Param::new(B + 23, "Formant Shift", -3.0, 3.0, 0.0, Unit::Octaves),
        Param::new(B + 24, "Resonance", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(B + 25, "LFO Shift Intensity", 0.0, 3.0, 0.0, Unit::Octaves),
        Param::new(B + 26, "LFO Pitch Intensity", 0.0, 12.0, 0.0, Unit::Semitones),
        Param::menu(B + 27, "LFO Wave", WAVES, 0.0),
        Param::new(B + 28, "LFO Rate", 0.01, 20.0, 5.0, Unit::Hz),
        Param::menu(B + 29, "Signal", &["Voc", "Syn", "Ana"], 0.0),
        Param::new(B + 30, "Level", -36.0, 12.0, 0.0, Unit::Db),
        Param::new(B + 31, "Stereo Width", 0.0, 100.0, 50.0, Unit::Percent),
    ]
});

pub struct TrackOscillator {
    p: Values,
    rate: f32,
    ana: [[Svf; 2]; MAXB],
    syn: [[Svf; 2]; MAXB],
    env: [Follower; MAXB],
    frozen: [f32; MAXB],
    nb: usize,
    q: f32,
    qa: f32,
    // the tracking oscillator
    buf: Vec<f32>,
    at: usize,
    hop: usize,
    count: usize,
    pub hz: Option<f32>,
    pitch: Smooth,
    car: f32,
    modp: f32,
    noise: Noise,
    hp: Svf,
    lfo: Phase,
    hold: (f32, f32),
    voiced: f32,
}

impl TrackOscillator {
    pub fn new(rate: f32) -> TrackOscillator {
        let mut t = TrackOscillator {
            p: Values::new(TO_P.as_slice()),
            rate,
            ana: [[Svf::default(); 2]; MAXB],
            syn: [[Svf::default(); 2]; MAXB],
            env: [Follower::new(rate, 5.0, 80.0); MAXB],
            frozen: [0.0; MAXB],
            nb: 20,
            q: 1.0,
            qa: 1.0,
            buf: vec![0.0; 1536],
            at: 0,
            hop: (rate / 50.0) as usize,
            count: 0,
            hz: None,
            pitch: Smooth::new(rate, 20.0, 220.0),
            car: 0.0,
            modp: 0.0,
            noise: Noise(777),
            hp: Svf::new(rate, 4000.0, 0.7),
            lfo: Phase::default(),
            hold: (0.0, 0.0),
            voiced: 1.0,
        };
        t.update();
        t
    }
    fn tune(&mut self, shift_extra: f32) {
        let n = self.p.get(B + 3).round() as usize;
        let ca = centres(n, self.p.get(B + 18), self.p.get(B + 19), 0.0, 1.0, self.rate);
        let cs = centres(n, self.p.get(B + 18), self.p.get(B + 19), self.p.get(B + 23) + shift_extra, self.p.get(B + 22), self.rate);
        self.nb = ca.len();
        let q = 2.0 + 18.0 * (self.p.get(B + 24) / 100.0).powi(2) + n as f32 * 0.3;
        self.q = q;
        self.qa = 2.0 + n as f32 * 0.3;
        for k in 0..self.nb {
            for st in 0..2 {
                self.ana[k][st].set(self.rate, ca[k], 2.0 + n as f32 * 0.3);
                self.syn[k][st].set(self.rate, cs[k], q);
            }
        }
    }
    fn update(&mut self) {
        self.tune(0.0);
        for e in self.env.iter_mut() {
            e.set(self.rate, self.p.get(B), self.p.get(B + 1));
        }
        self.pitch.set_time(self.rate, self.p.get(B + 14).max(0.5));
    }
    fn clear(&mut self) {
        for f in self.ana.iter_mut().chain(self.syn.iter_mut()).flatten() {
            f.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.2
    }
    /// The oscillator's pitch: the detected one, pulled to the scale by Strength.
    fn quantise(&self, hz: f32) -> f32 {
        let s = self.p.get(B + 13) / 100.0;
        if s <= 0.0 {
            return hz;
        }
        let note = 69.0 + 12.0 * (hz / 440.0).log2();
        let root = self.p.idx(B + 15);
        let sc = SCALES[self.p.idx(B + 16).min(SCALES.len() - 1)].1;
        let base = note.round() as i32;
        let best = (-6..=6).map(|d| base + d).filter(|n| sc[((n - root as i32).rem_euclid(12)) as usize]).min_by(|a, b| (*a as f32 - note).abs().partial_cmp(&(*b as f32 - note).abs()).unwrap()).unwrap_or(base) as f32;
        440.0 * 2f32.powf((note + (best - note) * s - 69.0) / 12.0)
    }
}

impl Effect for TrackOscillator {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let osc = self.p.idx(B + 8) == 2;
        let freeze = self.p.on(B + 2);
        let uv_mode = self.p.idx(B + 6);
        let uv_level = self.p.get(B + 7) / 100.0;
        let sens = self.p.get(B + 5) / 100.0;
        let (ratio, fmi) = (self.p.get(B + 9), self.p.get(B + 10) / 100.0 * 4.0);
        let coarse = 2f32.powf(self.p.get(B + 11) / 12.0);
        let fine = self.p.get(B + 12);
        let signal = self.p.idx(B + 29);
        let lvl = db(self.p.get(B + 30));
        let width = self.p.get(B + 31) / 100.0;
        let (lowhp, highlp) = (self.p.idx(B + 20) == 1, self.p.idx(B + 21) == 1);
        let maxtrack = self.p.get(B + 17);
        let lfo_pitch = self.p.get(B + 26);
        let lfo_shift = self.p.get(B + 25);
        for i in 0..left.len().min(right.len()) {
            let x = 0.5 * (left[i] + right[i]);
            // pitch tracking
            self.buf[self.at] = x;
            self.at = (self.at + 1) % self.buf.len();
            self.count += 1;
            if self.count >= self.hop {
                self.count = 0;
                let n = self.buf.len();
                let w: Vec<f32> = (0..n).map(|k| self.buf[(self.at + k) % n]).collect();
                self.hz = yin(&w, self.rate, 50.0, maxtrack);
                if let Some(f) = self.hz {
                    self.pitch.target = self.quantise(f);
                }
                if lfo_shift > 0.0 {
                    let m = lfo_wave(self.p.idx(B + 27), self.lfo.p, &mut self.hold, &mut self.noise);
                    self.tune(lfo_shift * m);
                }
            }
            let lp = self.lfo.step(self.rate, self.p.get(B + 28));
            let vib = if lfo_pitch > 0.0 { 2f32.powf(lfo_pitch * sine01(lp) / 12.0) } else { 1.0 };
            let f = (self.pitch.next() * coarse + fine) * vib;
            // two-operator FM oscillator
            self.modp = (self.modp + f * ratio / self.rate).fract();
            let m = (crate::util::TAU * self.modp).sin() * fmi;
            self.car = (self.car + f / self.rate).fract();
            let oscv = (crate::util::TAU * self.car + m).sin() * 0.5;
            // U/V: unvoiced (no pitch) parts replaced by noise / the high-passed input
            let hi = self.hp.run(x).high;
            let unvoiced = self.hz.is_none() && x.abs() > 0.002 * (1.0 - sens);
            self.voiced += 0.01 * ((if unvoiced { 0.0 } else { 1.0 }) - self.voiced);
            let uv = match uv_mode {
                0 => self.noise.next() * 0.3,
                1 => self.noise.next() * 0.2 + oscv * 0.5,
                _ => hi,
            } * uv_level * (1.0 - self.voiced);
            let carrier = if osc { oscv } else { x };
            let mut out = [0f32; 2];
            for k in 0..self.nb {
                let a = self.ana[k][0].run(x);
                let a = self.ana[k][1].run(a.band).band / (self.qa * self.qa);
                let e = if freeze { self.frozen[k] } else { self.env[k].run(a.abs()) };
                self.frozen[k] = e;
                let mut band = self.syn[k][0].run(carrier);
                let o = self.syn[k][1].run(if k == 0 && lowhp { band.high } else if k == self.nb - 1 && highlp { band.low } else { band.band });
                band = o;
                let b = if k == 0 && lowhp { band.high } else if k == self.nb - 1 && highlp { band.low } else { band.band };
                let y = b / (self.q * self.q) * e * 120.0;
                if k % 2 == 0 {
                    out[0] += y;
                    out[1] += y * (1.0 - width);
                } else {
                    out[1] += y;
                    out[0] += y * (1.0 - width);
                }
            }
            let (l, r) = match signal {
                1 => (carrier, carrier),
                2 => (x, x),
                _ => (out[0] + uv, out[1] + uv),
            };
            left[i] = l * lvl;
            right[i] = r * lvl;
        }
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        self.hz.map(|f| vec![("Tracked Pitch", f)]).unwrap_or_default()
    }
}

plugin_impl!(TrackOscillator);

pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "EVOC 20 Filterbank" => Box::new(Filterbank::new(rate)),
        "EVOC 20 TrackOscillator" => Box::new(TrackOscillator::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["EVOC 20 Filterbank", "EVOC 20 TrackOscillator"];

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    #[test]
    fn filterbank_band_fader_cuts_its_band() {
        let mut f = Filterbank::new(48000.0);
        f.set(B + 3, 10.0);
        let x: Vec<f32> = (0..48000).map(|i| (crate::util::TAU * 1000.0 * i as f32 / 48000.0).sin() * 0.3).collect();
        let run = |f: &mut Filterbank| {
            let mut l = x.clone();
            let mut r = x.clone();
            f.reset();
            f.process(&mut l, &mut r);
            l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        let a = run(&mut f);
        // the band nearest 1 kHz to zero
        let k = f.cs.iter().enumerate().min_by(|a, b| (a.1 - 1000.0).abs().partial_cmp(&(b.1 - 1000.0).abs()).unwrap()).unwrap().0;
        f.set(B + 100 + k as u32, 0.0);
        let b = run(&mut f);
        assert!(b < a * 0.6, "{a} {b}");
    }

    #[test]
    fn vocoder_follows_the_input_and_tracks_pitch() {
        let mut t = TrackOscillator::new(48000.0);
        let x: Vec<f32> = (0..48000).map(|i| {
            let s = i as f32 / 48000.0;
            0.3 * (crate::util::TAU * 200.0 * s).sin() + 0.15 * (crate::util::TAU * 400.0 * s).sin()
        }).collect();
        let mut l = x.clone();
        let mut r = x.clone();
        t.process(&mut l, &mut r);
        assert!((t.hz.unwrap() - 200.0).abs() < 3.0);
        assert!(l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.01);
        // silence in, silence out
        let mut l = vec![0.0; 9600];
        let mut r = l.clone();
        t.reset();
        let mut t2 = TrackOscillator::new(48000.0);
        t2.process(&mut l, &mut r);
        assert!(l.iter().all(|v| v.abs() < 1e-3));
    }
}
