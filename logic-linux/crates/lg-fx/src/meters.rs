//! Measuring: loudness after ITU-R BS.1770 / EBU R128 (K-weighting, momentary 400 ms,
//! short-term 3 s, gated integrated loudness, loudness range), true peak (4× oversampled),
//! stereo correlation, a ring buffer for spectra, and the metering plug-ins built on them:
//! Level Meter, Loudness Meter, Correlation Meter, MultiMeter (analyzer, goniometer, levels,
//! correlation, peaks), BPM Counter and the Tuner plug-in. Meters pass audio through untouched
//! and report through `readouts` / `curve`.

use crate::convolve::{C, Fft};
use crate::util::{Biquad, Coefs, Values, plugin_impl};
use lg_dsp::{Effect, Param, Unit};

/// K-weighting (the two stages of BS.1770) for one channel.
#[derive(Clone, Copy, Debug)]
pub struct KWeight {
    shelf: Biquad,
    hp: Biquad,
}

impl KWeight {
    pub fn new(rate: f32) -> KWeight {
        KWeight { shelf: Biquad::new(Coefs::high_shelf(rate, 1681.97, 0.7072, 3.9998)), hp: Biquad::new(Coefs::highpass(rate, 38.135, 0.5003)) }
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.shelf.run(x);
        self.hp.run(y)
    }
}

/// Loudness in LUFS (momentary, short-term, integrated) and loudness range in LU.
pub struct Loudness {
    rate: f32,
    k: [KWeight; 2],
    /// Mean squares of 100 ms blocks.
    acc: f64,
    n: usize,
    block: usize,
    blocks: Vec<f64>,
    pub running: bool,
}

impl Loudness {
    pub fn new(rate: f32) -> Loudness {
        Loudness { rate, k: [KWeight::new(rate); 2], acc: 0.0, n: 0, block: (0.1 * rate) as usize, blocks: Vec::with_capacity(36000), running: true }
    }
    pub fn reset(&mut self) {
        self.blocks.clear();
        self.acc = 0.0;
        self.n = 0;
        self.k = [KWeight::new(self.rate); 2];
    }
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        if !self.running {
            return;
        }
        let a = self.k[0].run(l) as f64;
        let b = self.k[1].run(r) as f64;
        self.acc += a * a + b * b;
        self.n += 1;
        if self.n >= self.block {
            if self.blocks.len() < self.blocks.capacity() {
                self.blocks.push(self.acc / self.n as f64);
            } else {
                // a full hour kept: older blocks give way
                self.blocks.remove(0);
                self.blocks.push(self.acc / self.n as f64);
            }
            self.acc = 0.0;
            self.n = 0;
        }
    }
    fn lufs(ms: f64) -> f32 {
        if ms <= 1e-12 { -120.0 } else { (-0.691 + 10.0 * ms.log10()) as f32 }
    }
    fn window(&self, blocks: usize) -> f32 {
        let b = &self.blocks[self.blocks.len().saturating_sub(blocks)..];
        if b.is_empty() {
            return -120.0;
        }
        Loudness::lufs(b.iter().sum::<f64>() / b.len() as f64)
    }
    /// 400 ms.
    pub fn momentary(&self) -> f32 {
        self.window(4)
    }
    /// 3 s.
    pub fn short_term(&self) -> f32 {
        self.window(30)
    }
    /// The 400 ms gating blocks (75 % overlap) as mean squares.
    fn gating_blocks(&self) -> Vec<f64> {
        if self.blocks.len() < 4 {
            return Vec::new();
        }
        self.blocks.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect()
    }
    /// Gated integrated loudness (absolute −70 LUFS, then relative −10 LU).
    pub fn integrated(&self) -> f32 {
        let g: Vec<f64> = self.gating_blocks().into_iter().filter(|m| Loudness::lufs(*m) > -70.0).collect();
        if g.is_empty() {
            return -120.0;
        }
        let rel = Loudness::lufs(g.iter().sum::<f64>() / g.len() as f64) - 10.0;
        let g2: Vec<f64> = g.into_iter().filter(|m| Loudness::lufs(*m) > rel).collect();
        if g2.is_empty() {
            return -120.0;
        }
        Loudness::lufs(g2.iter().sum::<f64>() / g2.len() as f64)
    }
    /// Loudness range (EBU Tech 3342): the spread of 3 s short-term values, 10th to 95th
    /// percentile, after gating.
    pub fn range(&self) -> f32 {
        if self.blocks.len() < 30 {
            return 0.0;
        }
        let st: Vec<f32> = self.blocks.windows(30).step_by(10).map(|w| Loudness::lufs(w.iter().sum::<f64>() / 30.0)).filter(|v| *v > -70.0).collect();
        if st.is_empty() {
            return 0.0;
        }
        let mean = st.iter().map(|v| 10f64.powf(*v as f64 / 10.0)).sum::<f64>() / st.len() as f64;
        let rel = 10.0 * mean.log10() as f32 - 20.0;
        let mut v: Vec<f32> = st.into_iter().filter(|x| *x > rel).collect();
        if v.len() < 2 {
            return 0.0;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |p: f32| v[((v.len() - 1) as f32 * p).round() as usize];
        at(0.95) - at(0.10)
    }
}

/// True peak: the signal 4× oversampled (cubic interpolation between samples), held max.
#[derive(Clone, Copy, Debug, Default)]
pub struct TruePeak {
    hist: [[f32; 4]; 2],
    pub now: f32,
    pub max: f32,
    fall: f32,
}

impl TruePeak {
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        let mut pk = 0f32;
        for (s, x) in [l, r].into_iter().enumerate() {
            let h = &mut self.hist[s];
            h.rotate_left(1);
            h[3] = x;
            let (y0, y1, y2, y3) = (h[0], h[1], h[2], h[3]);
            pk = pk.max(y2.abs());
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let c1 = 0.5 * (y2 - y0);
                let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
                let v = ((c3 * t + c2) * t + c1) * t + y1;
                pk = pk.max(v.abs());
            }
        }
        self.fall = (self.fall * 0.99995).max(pk);
        self.now = self.fall;
        self.max = self.max.max(pk);
    }
    pub fn now_db(&self) -> f32 {
        lg_dsp::gain_to_db(self.now)
    }
    pub fn max_db(&self) -> f32 {
        lg_dsp::gain_to_db(self.max)
    }
    pub fn clear_max(&mut self) {
        self.max = 0.0;
    }
}

/// Stereo correlation (−1..+1), averaged over about 300 ms.
#[derive(Clone, Copy, Debug)]
pub struct Correlation {
    lr: f32,
    ll: f32,
    rr: f32,
    k: f32,
}

impl Correlation {
    pub fn new(rate: f32) -> Correlation {
        Correlation { lr: 0.0, ll: 0.0, rr: 0.0, k: 1.0 - (-1.0 / (0.3 * rate)).exp() }
    }
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        self.lr += self.k * (l * r - self.lr);
        self.ll += self.k * (l * l - self.ll);
        self.rr += self.k * (r * r - self.rr);
    }
    pub fn value(&self) -> f32 {
        let d = (self.ll * self.rr).sqrt();
        if d < 1e-12 { 1.0 } else { (self.lr / d).clamp(-1.0, 1.0) }
    }
}

/// The last samples (mono and stereo), for spectra and the goniometer.
pub struct Ring {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub at: usize,
}

impl Ring {
    pub fn new(n: usize) -> Ring {
        Ring { l: vec![0.0; n], r: vec![0.0; n], at: 0 }
    }
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        self.l[self.at] = l;
        self.r[self.at] = r;
        self.at = (self.at + 1) % self.l.len();
    }
    /// The buffer in time order (oldest first), as (l, r).
    pub fn ordered(&self) -> (Vec<f32>, Vec<f32>) {
        let n = self.l.len();
        ((0..n).map(|i| self.l[(self.at + i) % n]).collect(), (0..n).map(|i| self.r[(self.at + i) % n]).collect())
    }
}

/// Levels (dB) in `bands` log-spaced bands from 20 Hz to 20 kHz of a mono buffer (Hann window).
pub fn spectrum(x: &[f32], rate: f32, bands: usize) -> Vec<f32> {
    let n = x.len().next_power_of_two().min(x.len().max(2).next_power_of_two());
    let n = if n > x.len() { n / 2 } else { n }.max(64);
    let fft = Fft::new(n);
    let off = x.len().saturating_sub(n);
    let mut buf: Vec<C> = (0..n).map(|i| {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
        C { re: x.get(off + i).copied().unwrap_or(0.0) * w, im: 0.0 }
    }).collect();
    fft.run(&mut buf, false);
    let mag: Vec<f32> = buf[..n / 2].iter().map(|c| (c.re * c.re + c.im * c.im) * 4.0 / (n as f32 * n as f32 * 0.25)).collect();
    (0..bands).map(|b| {
        let f0 = 20.0 * 1000f32.powf(b as f32 / bands as f32);
        let f1 = 20.0 * 1000f32.powf((b + 1) as f32 / bands as f32);
        let (i0, i1) = ((f0 / rate * n as f32) as usize, ((f1 / rate * n as f32) as usize).max((f0 / rate * n as f32) as usize + 1));
        let p: f32 = mag[i0.min(n / 2 - 1)..i1.min(n / 2)].iter().sum::<f32>().max(1e-14);
        10.0 * p.log10()
    }).collect()
}

// ---------------------------------------------------------------------------------------------
// The metering plug-ins

const METERS: &[&str] = &["Level Meter", "Loudness Meter", "Correlation Meter", "MultiMeter", "BPM Counter", "Tuner"];

static LEVEL_P: [Param; 3] = [Param::menu(0, "Characteristic", &["Peak", "RMS", "Peak & RMS"], 2.0), Param::new(1, "Reset", 0.0, 1.0, 0.0, Unit::OnOff), Param::menu(2, "Scale", &["Linear", "Logarithmic"], 1.0)];
static LOUD_P: [Param; 3] = [Param::new(0, "Running", 0.0, 1.0, 1.0, Unit::OnOff), Param::new(1, "Reset", 0.0, 1.0, 0.0, Unit::OnOff), Param::new(2, "Target", -36.0, 0.0, -14.0, Unit::Db)];
static CORR_P: [Param; 0] = [];
static MULTI_P: [Param; 6] = [
    Param::menu(0, "View", &["Analyzer", "Goniometer", "Level", "Loudness", "Correlation"], 0.0),
    Param::menu(1, "Analyzer Bands", &["31 Bands", "63 Bands"], 0.0),
    Param::menu(2, "Analyzer Channels", &["Left & Right", "Mid & Side", "Left", "Right", "Mid", "Side"], 0.0),
    Param::new(3, "Peak Hold", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(4, "Goniometer Auto Gain", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(5, "Reset", 0.0, 1.0, 0.0, Unit::OnOff),
];
static BPM_P: [Param; 2] = [Param::new(0, "Reset", 0.0, 1.0, 0.0, Unit::OnOff), Param::menu(1, "Range", &["Normal (60–160)", "Wide (40–240)"], 0.0)];
static TUNER_P: [Param; 2] = [Param::new(0, "Reference Pitch", 400.0, 480.0, 440.0, Unit::Hz), Param::new(1, "Mute", 0.0, 1.0, 0.0, Unit::OnOff)];

/// One metering plug-in: which meter it is and the measuring parts.
pub struct Meter {
    kind: usize,
    p: Values,
    rate: f32,
    loud: Loudness,
    tp: TruePeak,
    corr: Correlation,
    ring: Ring,
    rms: [f32; 2],
    peak: [f32; 2],
    hold: [f32; 2],
    // BPM counter: onset envelope history and the estimate
    onset: Vec<f32>,
    env: f32,
    env_prev: f32,
    hop: usize,
    hop_n: usize,
    acc: f32,
    pub bpm: f32,
}

impl Meter {
    pub fn new(name: &str, rate: f32) -> Option<Meter> {
        let kind = METERS.iter().position(|m| *m == name)?;
        let defs: &'static [Param] = match kind {
            0 => &LEVEL_P,
            1 => &LOUD_P,
            2 => &CORR_P,
            3 => &MULTI_P,
            4 => &BPM_P,
            _ => &TUNER_P,
        };
        Some(Meter { kind, p: Values::new(defs), rate, loud: Loudness::new(rate), tp: TruePeak::default(), corr: Correlation::new(rate), ring: Ring::new(8192), rms: [0.0; 2], peak: [0.0; 2], hold: [0.0; 2], onset: Vec::with_capacity(2048), env: 0.0, env_prev: 0.0, hop: (rate / 100.0) as usize, hop_n: 0, acc: 0.0, bpm: 0.0 })
    }
    pub fn names() -> &'static [&'static str] {
        METERS
    }
    fn update(&mut self) {
        if self.p.get(1) >= 0.5 && matches!(self.kind, 0 | 1) {
            self.p.set(1, 0.0);
            self.loud.reset();
            self.tp.clear_max();
            self.hold = [0.0; 2];
        }
        if self.kind == 1 {
            self.loud.running = self.p.on(0);
        }
        if self.kind == 3 && self.p.on(5) {
            self.p.set(5, 0.0);
            self.loud.reset();
            self.tp.clear_max();
            self.hold = [0.0; 2];
        }
        if self.kind == 4 && self.p.on(0) {
            self.p.set(0, 0.0);
            self.onset.clear();
            self.bpm = 0.0;
        }
    }
    fn clear(&mut self) {}
    fn tail_secs(&self) -> f32 {
        0.0
    }

    /// Tempo by autocorrelation of the onset envelope (10 ms hops, the last ~10 s).
    fn estimate_bpm(&mut self) {
        let o = &self.onset;
        if o.len() < 300 {
            return;
        }
        let (lo, hi) = if self.p.idx(1) == 0 { (60.0, 160.0) } else { (40.0, 240.0) };
        let mean = o.iter().sum::<f32>() / o.len() as f32;
        let x: Vec<f32> = o.iter().map(|v| v - mean).collect();
        let mut best = (0.0, 0f32);
        let lag_lo = (6000.0 / hi) as usize;
        let lag_hi = (6000.0 / lo) as usize;
        for lag in lag_lo..=lag_hi.min(x.len() / 2) {
            let mut s = 0.0;
            for i in lag..x.len() {
                s += x[i] * x[i - lag];
            }
            // a little preference for the tempo's double-time relative
            let s = s / (x.len() - lag) as f32;
            if s > best.1 {
                best = (lag as f32, s);
            }
        }
        if best.0 > 0.0 {
            self.bpm = 6000.0 / best.0;
        }
    }
}

impl Effect for Meter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let k = 1.0 - (-1.0 / (0.3 * self.rate)).exp();
        for i in 0..left.len().min(right.len()) {
            let (l, r) = (left[i], right[i]);
            self.loud.push(l, r);
            self.tp.push(l, r);
            self.corr.push(l, r);
            self.ring.push(l, r);
            for (s, x) in [l, r].into_iter().enumerate() {
                self.rms[s] += k * (x * x - self.rms[s]);
                self.peak[s] = (self.peak[s] * 0.9998).max(x.abs());
                self.hold[s] = self.hold[s].max(x.abs());
            }
            if self.kind == 4 {
                // onset strength: rise of a rectified envelope, per 10 ms hop
                let m = 0.5 * (l.abs() + r.abs());
                self.env += 0.02 * (m - self.env);
                self.acc += (self.env - self.env_prev).max(0.0);
                self.env_prev = self.env;
                self.hop_n += 1;
                if self.hop_n >= self.hop {
                    self.hop_n = 0;
                    if self.onset.len() >= 1000 {
                        self.onset.remove(0);
                    }
                    self.onset.push(self.acc);
                    self.acc = 0.0;
                    if self.onset.len() % 100 == 0 {
                        self.estimate_bpm();
                    }
                }
            }
        }
        if self.kind == 5 && self.p.on(1) {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        let d = lg_dsp::gain_to_db;
        let mut v = vec![
            ("Peak L", d(self.peak[0])),
            ("Peak R", d(self.peak[1])),
            ("RMS L", d(self.rms[0].sqrt())),
            ("RMS R", d(self.rms[1].sqrt())),
            ("Hold L", d(self.hold[0])),
            ("Hold R", d(self.hold[1])),
            ("True Peak", self.tp.now_db()),
            ("True Peak Max", self.tp.max_db()),
            ("Momentary", self.loud.momentary()),
            ("Short-Term", self.loud.short_term()),
            ("Integrated", self.loud.integrated()),
            ("LU Range", self.loud.range()),
            ("Correlation", self.corr.value()),
        ];
        if self.kind == 4 {
            v.push(("BPM", self.bpm));
        }
        if self.kind == 5 {
            let (l, r) = self.ring.ordered();
            let m: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
            if let Some(f) = crate::kit::yin(&m[m.len() - 4096..], self.rate, 30.0, 2000.0) {
                let a4 = self.p.get(0);
                let note = 69.0 + 12.0 * (f / a4).log2();
                v.push(("Frequency", f));
                v.push(("Note", note.round()));
                v.push(("Cents", (note - note.round()) * 100.0));
            }
        }
        v
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        let (l, r) = self.ring.ordered();
        match name {
            "spectrum" => {
                let bands = if self.p.idx(1) == 1 { 63 } else { 31 };
                let ch = self.p.idx(2);
                let pick: Vec<f32> = l.iter().zip(&r).map(|(a, b)| match ch {
                    2 => *a,
                    3 => *b,
                    5 => 0.5 * (a - b),
                    _ => 0.5 * (a + b),
                }).collect();
                spectrum(&pick, self.rate, bands)
            }
            // the goniometer: interleaved (side, mid) points of the last 2048 samples
            "goniometer" => l[l.len() - 2048..].iter().zip(&r[r.len() - 2048..]).flat_map(|(a, b)| [(a - b) * 0.7071, (a + b) * 0.7071]).collect(),
            _ => Vec::new(),
        }
    }
}

plugin_impl!(Meter);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudness_of_a_1k_sine() {
        // a 1 kHz sine of peak 0.1 (-20 dBFS) in both channels reads about -20 LUFS
        let rate = 48000.0;
        let mut m = Loudness::new(rate);
        for i in 0..(rate as usize * 5) {
            let x = 0.1 * (std::f32::consts::TAU * 1000.0 * i as f32 / rate).sin();
            m.push(x, x);
        }
        let i = m.integrated();
        assert!((i - (-20.0)).abs() < 0.3, "{i}");
        assert!((m.momentary() - i).abs() < 0.3);
        assert!(m.range() < 0.5);
    }

    #[test]
    fn true_peak_catches_intersample_peaks() {
        // a sine at fs/4 phase-shifted by 45°: samples at 0.707, true peak 1.0
        let mut t = TruePeak::default();
        for i in 0..64 {
            let x = (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin();
            t.push(x, x);
        }
        assert!(t.max_db() > -1.5, "{}", t.max_db());
    }

    #[test]
    fn correlation_values() {
        let mut c = Correlation::new(48000.0);
        for i in 0..48000 {
            let x = (i as f32 * 0.01).sin();
            c.push(x, -x);
        }
        assert!(c.value() < -0.99);
    }

    #[test]
    fn bpm_counter_finds_120() {
        let mut m = Meter::new("BPM Counter", 48000.0).unwrap();
        // clicks every 0.5 s for 12 s
        let mut l: Vec<f32> = (0..48000 * 12).map(|i| if i % 24000 < 200 { 0.9 } else { 0.0 }).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            m.process(a, b);
        }
        assert!((m.bpm - 120.0).abs() < 2.0, "{}", m.bpm);
    }

    #[test]
    fn spectrum_peaks_at_the_tone() {
        let x: Vec<f32> = (0..8192).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 48000.0).sin()).collect();
        let s = spectrum(&x, 48000.0, 31);
        let top = (0..31).max_by(|a, b| s[*a].partial_cmp(&s[*b]).unwrap()).unwrap();
        let f0 = 20.0 * 1000f32.powf(top as f32 / 31.0);
        let f1 = 20.0 * 1000f32.powf((top + 1) as f32 / 31.0);
        assert!(f0 <= 1000.0 && f1 >= 1000.0, "{f0} {f1}");
    }
}
