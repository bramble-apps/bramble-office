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
        self.push_power(a * a + b * b);
    }
    /// One frame's channel-weighted sum of K-weighted squares (surround: the caller weights
    /// each channel after BS.1770 and leaves out the LFE).
    #[inline]
    pub fn push_power(&mut self, p: f64) {
        if !self.running {
            return;
        }
        self.acc += p;
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
    /// The averaging time (the Correlation Meter's Reaction).
    pub fn set_time(&mut self, rate: f32, secs: f32) {
        self.k = 1.0 - (-1.0 / (secs * rate)).exp();
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
//
// Level Meter, Loudness Meter and MultiMeter have native surround versions: on a surround bus
// they meter every channel (`process_surround`), the loudness of all channels but the LFE
// (BS.1770 weights), and MultiMeter adds Balance mode, per-channel analyzer buttons and the
// goniometer's channel pairs. Peak holds and the integrated loudness reset when playback
// starts; the windows' Reset buttons bump a counter parameter.

use lg_dsp::surround::Ch;

const METERS: &[&str] = &["Level Meter", "Loudness Meter", "Correlation Meter", "MultiMeter", "BPM Counter", "Tuner"];

/// The meters that have a surround version (they meter the surround bus as it is).
pub const SURROUND: &[&str] = &["Level Meter", "Loudness Meter", "MultiMeter"];

pub const CHARACTERISTICS: &[&str] = &["Peak", "RMS", "Peak & RMS", "True Peak", "True Peak & RMS"];
pub const HOLD_TIMES: &[&str] = &["2 sec", "4 sec", "6 sec", "Infinite"];
pub const RETURN_RATES: &[&str] = &["3 dB/s", "6 dB/s", "12 dB/s", "20 dB/s", "40 dB/s"];
const RETURN_DB: [f32; 5] = [3.0, 6.0, 12.0, 20.0, 40.0];
const ORIENT: &[&str] = &["Vertical", "Horizontal"];

/// MultiMeter's parameter ids.
pub mod mm {
    pub const MODE: u32 = 0;
    pub const BANDS: u32 = 1;
    pub const DETECTION: u32 = 2;
    pub const HOLD: u32 = 3;
    pub const AUTO_GAIN: u32 = 4;
    pub const RESET: u32 = 5;
    pub const AN_MODE: u32 = 6;
    pub const TOP: u32 = 7;
    pub const RANGE: u32 = 8;
    pub const DECAY: u32 = 9;
    pub const PEAK_TIME: u32 = 10;
    pub const RETURN: u32 = 11;
    pub const LEVEL: u32 = 12;
    pub const TARGET: u32 = 13;
    pub const LEVEL_TARGET: u32 = 14;
    pub const C_TARGET: u32 = 15;
    pub const LFE_TARGET: u32 = 16;
    pub const VIEW: u32 = 17;
    pub const PAIR: u32 = 18;
    pub const SUM_MAX: u32 = 19;
    pub const CHANNELS: u32 = 20;
    pub const LFE_SRC: u32 = 21;
}

static LEVEL_P: [Param; 7] = [
    Param::menu(0, "Characteristic", CHARACTERISTICS, 2.0),
    Param::new(1, "Reset", 0.0, 1.0e6, 0.0, Unit::None).hide(),
    Param::menu(2, "Scale", &["Linear", "Logarithmic"], 1.0),
    Param::new(3, "Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::new(4, "Center Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::new(5, "LFE Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::menu(6, "Orientation", ORIENT, 0.0).hide(),
];
static LOUD_P: [Param; 4] = [
    Param::new(0, "Running", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Reset", 0.0, 1.0e6, 0.0, Unit::None).hide(),
    Param::new(2, "Target", -30.0, 0.0, -14.0, Unit::Db),
    Param::menu(6, "Orientation", ORIENT, 0.0).hide(),
];
static CORR_P: [Param; 3] = [
    Param::menu(0, "Reaction", &["Fast", "Medium", "Slow"], 1.0),
    Param::new(1, "Reset", 0.0, 1.0e6, 0.0, Unit::None).hide(),
    Param::menu(6, "Orientation", ORIENT, 1.0).hide(),
];
static MULTI_P: [Param; 22] = [
    Param::menu(mm::MODE, "Mode", &["Analyzer", "Goniometer", "Balance"], 0.0),
    Param::menu(mm::BANDS, "Bands", &["31 Bands (Third-Octave)", "63 Bands (Major Second)"], 0.0),
    Param::menu(mm::DETECTION, "Detection", &["Left", "Right", "LRmax", "Mono"], 2.0),
    Param::new(mm::HOLD, "Hold", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(mm::AUTO_GAIN, "Goniometer Auto Gain", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(mm::RESET, "Reset", 0.0, 1.0e6, 0.0, Unit::None).hide(),
    Param::menu(mm::AN_MODE, "Analyzer Mode", &["Peak", "Slow RMS", "Fast RMS"], 2.0),
    Param::new(mm::TOP, "Top", -40.0, 20.0, 0.0, Unit::Db),
    Param::new(mm::RANGE, "Range", 20.0, 100.0, 60.0, Unit::Db),
    Param::new(mm::DECAY, "Goniometer Decay", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(mm::PEAK_TIME, "Peak", HOLD_TIMES, 0.0),
    Param::menu(mm::RETURN, "Return Rate", RETURN_RATES, 2.0),
    Param::menu(mm::LEVEL, "Level", CHARACTERISTICS, 2.0),
    Param::new(mm::TARGET, "Loudness Target", -30.0, 0.0, -14.0, Unit::Db),
    Param::new(mm::LEVEL_TARGET, "Level Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::new(mm::C_TARGET, "Center Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::new(mm::LFE_TARGET, "LFE Target", -30.0, 0.0, -6.0, Unit::Db),
    Param::menu(mm::VIEW, "View", &["Full", "Meters", "Display"], 0.0).hide(),
    Param::menu(mm::PAIR, "Goniometer Detection", &["L-R", "Ls-Rs", "Lm-Rm", "Lc-Rc"], 0.0),
    Param::menu(mm::SUM_MAX, "Analyzer Sum/Max", &["Sum", "Max"], 0.0),
    Param::new(mm::CHANNELS, "Analyzer Channels", 0.0, 131071.0, 131071.0, Unit::None).hide(),
    Param::menu(mm::LFE_SRC, "LFE Correlation", &["C", "LCR", "Sum"], 0.0),
];
static BPM_P: [Param; 3] = [
    Param::new(0, "Reset", 0.0, 1.0e6, 0.0, Unit::None).hide(),
    Param::menu(1, "Range", &["80–160 bpm", "40–240 bpm"], 0.0),
    Param::menu(2, "Display", &[":2", "x1", "x2"], 1.0).hide(),
];
static TUNER_P: [Param; 3] = [
    Param::new(0, "Reference Pitch", 410.0, 470.0, 440.0, Unit::Hz),
    Param::new(1, "Mute", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(6, "Orientation", ORIENT, 1.0).hide(),
];

/// Static readout names per channel slot ("Peak 3").
fn names(kind: &str) -> &'static [&'static str] {
    use std::sync::OnceLock;
    static T: OnceLock<Vec<(String, Vec<&'static str>)>> = OnceLock::new();
    let t = T.get_or_init(|| {
        ["Ch", "Peak", "RMS", "TP", "Hold", "Seg", "SegRMS", "PairA", "PairB", "Bal", "PCorr", "PCorrMin"]
            .iter()
            .map(|k| (k.to_string(), (0..17).map(|i| &*Box::leak(format!("{k} {i}").into_boxed_str())).collect()))
            .collect()
    });
    t.iter().find(|x| x.0 == kind).map(|x| x.1.as_slice()).unwrap_or(&[])
}

/// A level that holds its peak for a while, then falls at the return rate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Held {
    pub v: f32,
    left: f32,
}

impl Held {
    /// `x` the new level (linear), `hold` seconds to hold (inf for ever), `fall` the factor per
    /// sample once the hold is over, `dt` the seconds this step covers.
    #[inline]
    pub fn push(&mut self, x: f32, hold: f32, fall: f32, dt: f32) {
        if x >= self.v {
            self.v = x;
            self.left = hold;
        } else if self.left > 0.0 {
            self.left -= dt;
        } else {
            self.v = (self.v * fall).max(x);
        }
    }
}

/// Per-channel measuring.
#[derive(Clone)]
struct Chan {
    pk: f32,
    ms: f32,
    tp_hist: [f32; 4],
    tp: f32,
    hold_pk: f32,
    hold_rms: f32,
    hold_tp: f32,
    seg: Held,
    seg_rms: Held,
    kw: KWeight,
    ring: Vec<f32>,
}

impl Chan {
    fn new(rate: f32) -> Chan {
        Chan { pk: 0.0, ms: 0.0, tp_hist: [0.0; 4], tp: 0.0, hold_pk: 0.0, hold_rms: 0.0, hold_tp: 0.0, seg: Held::default(), seg_rms: Held::default(), kw: KWeight::new(rate), ring: vec![0.0; RING] }
    }
    /// The 4× oversampled peak of the newest sample (cubic interpolation).
    #[inline]
    fn true_peak(&mut self, x: f32) -> f32 {
        let h = &mut self.tp_hist;
        h.rotate_left(1);
        h[3] = x;
        let (y0, y1, y2, y3) = (h[0], h[1], h[2], h[3]);
        let mut pk = y2.abs();
        for k in 1..4 {
            let t = k as f32 / 4.0;
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            pk = pk.max((((c3 * t + c2) * t + c1) * t + y1).abs());
        }
        pk
    }
}

const RING: usize = 4096;
/// Samples between analyzer frames.
const HOP: usize = 1024;

/// BS.1770 channel weights (surrounds +1.5 dB, LFE left out).
fn bs1770_weight(c: Ch) -> f64 {
    match c {
        Ch::Lfe => 0.0,
        Ch::Ls | Ch::Rs | Ch::Lm | Ch::Rm | Ch::S | Ch::Lts | Ch::Rts => 1.41,
        _ => 1.0,
    }
}

/// One metering plug-in: which meter it is and the measuring parts.
pub struct Meter {
    kind: usize,
    p: Values,
    rate: f32,
    layout: Vec<Ch>,
    ch: Vec<Chan>,
    at: usize,
    hop_left: usize,
    loud: Loudness,
    tp_max: f32,
    corr: Correlation,
    corr_min: f32,
    corr_hist: Vec<f32>,
    corr_quiet: f32,
    /// Surround: adjacent ear-level speaker pairs (layout slots), their correlation and balance.
    pairs: Vec<(usize, usize)>,
    pair_corr: Vec<Correlation>,
    pair_min: Vec<f32>,
    lfe_corr: Correlation,
    lfe_min: f32,
    /// Analyzer bands (dB), their hold segments.
    bands: Vec<f32>,
    band_hold: Vec<Held>,
    playing: bool,
    resets: f32,
    // BPM counter: onset envelope history and the estimate
    onset: Vec<f32>,
    env: f32,
    env_prev: f32,
    hop: usize,
    hop_n: usize,
    acc: f32,
    pub bpm: f32,
    /// The tempo found and kept (0 while still measuring).
    pub bpm_locked: f32,
    bpm_agree: u32,
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
        let mut m = Meter {
            kind,
            p: Values::new(defs),
            rate,
            layout: Vec::new(),
            ch: Vec::new(),
            at: 0,
            hop_left: HOP,
            loud: Loudness::new(rate),
            tp_max: 0.0,
            corr: Correlation::new(rate),
            corr_min: 1.0,
            corr_hist: Vec::new(),
            corr_quiet: 0.0,
            pairs: Vec::new(),
            pair_corr: Vec::new(),
            pair_min: Vec::new(),
            lfe_corr: Correlation::new(rate),
            lfe_min: 1.0,
            bands: Vec::new(),
            band_hold: Vec::new(),
            playing: false,
            resets: 0.0,
            onset: Vec::with_capacity(2048),
            env: 0.0,
            env_prev: 0.0,
            hop: (rate / 100.0) as usize,
            hop_n: 0,
            acc: 0.0,
            bpm: 0.0,
            bpm_locked: 0.0,
            bpm_agree: 0,
        };
        m.configure(&[Ch::L, Ch::R]);
        Some(m)
    }
    pub fn names() -> &'static [&'static str] {
        METERS
    }
    /// The channels metered (stereo: L R; a surround bus: its layout).
    pub fn configure(&mut self, layout: &[Ch]) {
        if self.layout == layout {
            return;
        }
        self.layout = layout.to_vec();
        self.ch = layout.iter().map(|_| Chan::new(self.rate)).collect();
        // ear-level speakers round the room, each joined to the next
        let mut ring: Vec<(f32, usize)> = layout
            .iter()
            .enumerate()
            .filter(|(_, c)| lg_dsp::surround::position(lg_dsp::surround::Format::S71, **c).1 == 0.0 && **c != Ch::Lfe)
            .map(|(k, c)| (lg_dsp::surround::position(lg_dsp::surround::Format::of_channels(layout).unwrap_or(lg_dsp::surround::Format::S51), *c).0.rem_euclid(360.0), k))
            .collect();
        ring.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        self.pairs = if ring.len() < 3 {
            if ring.len() == 2 { vec![(ring[1].1, ring[0].1)] } else { Vec::new() }
        } else {
            (0..ring.len()).map(|k| (ring[k].1, ring[(k + 1) % ring.len()].1)).collect()
        };
        self.pair_corr = self.pairs.iter().map(|_| Correlation::new(self.rate)).collect();
        self.pair_min = vec![1.0; self.pairs.len()];
        self.clear_holds();
    }
    pub fn layout(&self) -> &[Ch] {
        &self.layout
    }
    fn clear_holds(&mut self) {
        for c in &mut self.ch {
            c.hold_pk = 0.0;
            c.hold_rms = 0.0;
            c.hold_tp = 0.0;
            c.seg = Held::default();
            c.seg_rms = Held::default();
        }
        self.tp_max = 0.0;
        self.corr_min = 1.0;
        self.lfe_min = 1.0;
        self.pair_min.iter_mut().for_each(|v| *v = 1.0);
        self.band_hold.iter_mut().for_each(|h| *h = Held::default());
    }
    fn reset_id(&self) -> u32 {
        match self.kind {
            3 => mm::RESET,
            4 => 0,
            _ => 1,
        }
    }
    fn update(&mut self) {
        let r = self.p.get(self.reset_id());
        if r != self.resets {
            self.resets = r;
            if self.kind == 4 {
                self.onset.clear();
                self.bpm = 0.0;
                self.bpm_locked = 0.0;
                self.bpm_agree = 0;
            } else {
                self.clear_holds();
                self.loud.reset();
            }
        }
        if self.kind == 1 {
            self.loud.running = self.p.on(0);
        }
        if self.kind == 2 {
            self.corr.set_time(self.rate, [0.1, 0.3, 1.0][self.p.idx(0).min(2)]);
        }
        if self.kind == 3 && !self.p.on(mm::HOLD) {
            // (turning Hold off clears the held figures)
            self.band_hold.iter_mut().for_each(|h| *h = Held::default());
            self.corr_min = 1.0;
            self.lfe_min = 1.0;
            self.pair_min.iter_mut().for_each(|v| *v = 1.0);
        }
    }
    fn clear(&mut self) {}
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The hold time (seconds) of peak segments: MultiMeter's Peak menu with Hold on (off: no
    /// hold); the Level Meter's line holds briefly.
    fn hold_secs(&self) -> f32 {
        match self.kind {
            3 if self.p.on(mm::HOLD) => [2.0, 4.0, 6.0, f32::INFINITY][self.p.idx(mm::PEAK_TIME).min(3)],
            3 => 0.0,
            _ => 1.5,
        }
    }
    fn return_db(&self) -> f32 {
        if self.kind == 3 { RETURN_DB[self.p.idx(mm::RETURN).min(4)] } else { 12.0 }
    }

    /// Tempo by autocorrelation of the onset envelope (10 ms hops, the last ~10 s).
    fn estimate_bpm(&mut self) {
        let o = &self.onset;
        if o.len() < 300 {
            return;
        }
        let (lo, hi) = if self.p.idx(1) == 0 { (80.0, 160.0) } else { (40.0, 240.0) };
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
            let s = s / (x.len() - lag) as f32;
            if s > best.1 {
                best = (lag as f32, s);
            }
        }
        if best.0 > 0.0 {
            self.bpm = 6000.0 / best.0;
            // found once three estimates agree; a change starts the measuring again
            if self.bpm_locked > 0.0 && (self.bpm / self.bpm_locked - 1.0).abs() > 0.03 {
                self.bpm_locked = 0.0;
                self.bpm_agree = 0;
            }
            if self.bpm_locked == 0.0 {
                self.bpm_agree += 1;
                if self.bpm_agree >= 3 {
                    self.bpm_locked = self.bpm;
                }
            }
        }
    }

    /// The analyzer's source buffer: the chosen channels' spectra combined (Sum: power average;
    /// Max: the loudest per band), or for stereo Left / Right / LRmax / Mono.
    fn analyze(&mut self) {
        let n_bands = if self.p.idx(mm::BANDS) == 1 { 63 } else { 31 };
        if self.bands.len() != n_bands {
            self.bands = vec![-120.0; n_bands];
            self.band_hold = vec![Held::default(); n_bands];
        }
        let ordered = |c: &Chan, at: usize| -> Vec<f32> { (0..RING).map(|i| c.ring[(at + i) % RING]).collect() };
        let stereo = self.layout.len() == 2 && self.layout[0] == Ch::L;
        let mut spec: Vec<f32> = vec![-120.0; n_bands];
        if stereo {
            let (l, r) = (ordered(&self.ch[0], self.at), ordered(&self.ch[1], self.at));
            spec = match self.p.idx(mm::DETECTION) {
                0 => spectrum(&l, self.rate, n_bands),
                1 => spectrum(&r, self.rate, n_bands),
                3 => spectrum(&l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect::<Vec<_>>(), self.rate, n_bands),
                _ => {
                    let (a, b) = (spectrum(&l, self.rate, n_bands), spectrum(&r, self.rate, n_bands));
                    a.iter().zip(&b).map(|(x, y)| x.max(*y)).collect()
                }
            };
        } else {
            let mask = self.p.get(mm::CHANNELS) as u32;
            let max = self.p.idx(mm::SUM_MAX) == 1;
            let mut acc = vec![0f32; n_bands];
            let mut cnt = 0;
            for (k, c) in self.layout.iter().enumerate() {
                // (height channels aren't analyzed)
                if mask & (1 << k) == 0 || lg_dsp::surround::position(lg_dsp::surround::Format::S714, *c).1 > 0.0 {
                    continue;
                }
                let s = spectrum(&ordered(&self.ch[k], self.at), self.rate, n_bands);
                for (a, v) in acc.iter_mut().zip(&s) {
                    let pw = 10f32.powf(v / 10.0);
                    *a = if max { a.max(pw) } else { *a + pw };
                }
                cnt += 1;
            }
            if cnt > 0 {
                spec = acc.iter().map(|a| 10.0 * (if max { *a } else { a / cnt as f32 }).max(1e-14).log10()).collect();
            }
        }
        // Peak: jumps up, falls at the return rate; RMS: averaged (slow ~1 s, fast ~0.3 s)
        let dt = HOP as f32 / self.rate;
        let mode = self.p.idx(mm::AN_MODE);
        let fall_db = self.return_db() * dt;
        let k = match mode {
            1 => 1.0 - (-dt / 1.0).exp(),
            2 => 1.0 - (-dt / 0.3).exp(),
            _ => 1.0,
        };
        let hold = self.hold_secs();
        let fall = lg_dsp::db_to_gain(-fall_db);
        for (b, v) in self.bands.iter_mut().zip(&spec) {
            *b = if mode == 0 { if *v > *b { *v } else { (*b - fall_db).max(*v) } } else { *b + k * (v - *b) };
        }
        for (h, b) in self.band_hold.iter_mut().zip(&self.bands) {
            h.push(lg_dsp::db_to_gain(*b), hold, fall, dt);
        }
    }

    fn frame(&mut self, x: &[f32]) {
        let dt = 1.0 / self.rate;
        let hold = self.hold_secs();
        let fall = lg_dsp::db_to_gain(-self.return_db() * dt);
        let k_rms = 1.0 - (-1.0 / (0.3 * self.rate)).exp();
        let mut lp = 0f64;
        for (i, c) in self.ch.iter_mut().enumerate() {
            let v = x[i];
            c.pk = (c.pk * fall).max(v.abs());
            c.ms += k_rms * (v * v - c.ms);
            let tp = c.true_peak(v);
            c.tp = (c.tp * fall).max(tp);
            c.hold_pk = c.hold_pk.max(v.abs());
            c.hold_tp = c.hold_tp.max(tp);
            let rms = c.ms.sqrt();
            c.hold_rms = c.hold_rms.max(rms);
            c.seg.push(v.abs(), hold, fall, dt);
            c.seg_rms.push(rms, hold, fall, dt);
            c.ring[self.at] = v;
            let y = c.kw.run(v) as f64;
            lp += bs1770_weight(self.layout[i]) * y * y;
        }
        self.at = (self.at + 1) % RING;
        self.loud.push_power(lp);
        // correlation: the first pair (stereo L R)
        if x.len() >= 2 {
            let (a, b) = (x[0], x[self.layout.iter().position(|c| *c == Ch::R).unwrap_or(1)]);
            self.corr.push(a, b);
            self.corr_quiet = if a.abs() + b.abs() > 1e-5 { 0.0 } else { self.corr_quiet + dt };
        }
        for (k, (a, b)) in self.pairs.iter().enumerate() {
            self.pair_corr[k].push(x[*a], x[*b]);
        }
        if let Some(l) = self.layout.iter().position(|c| *c == Ch::Lfe) {
            let src = match self.p.idx(mm::LFE_SRC) {
                0 => self.layout.iter().position(|c| *c == Ch::C).map_or(0.0, |k| x[k]),
                1 => self.layout.iter().enumerate().filter(|(_, c)| matches!(c, Ch::L | Ch::C | Ch::R)).map(|(k, _)| x[k]).sum(),
                _ => self.layout.iter().enumerate().filter(|(_, c)| **c != Ch::Lfe).map(|(k, _)| x[k]).sum(),
            };
            self.lfe_corr.push(x[l], src);
        }
        if self.kind == 4 {
            // onset strength: rise of a rectified envelope, per 10 ms hop
            let m = x.iter().map(|v| v.abs()).sum::<f32>() / x.len().max(1) as f32;
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
        self.hop_left -= 1;
        if self.hop_left == 0 {
            self.hop_left = HOP;
            self.block_end();
        }
    }

    /// Every hop: the correlation history and holds, the analyzer.
    fn block_end(&mut self) {
        let c = self.corr.value();
        // (no signal for two seconds: the momentary line goes back to 0)
        let c = if self.corr_quiet > 2.0 { 0.0 } else { c };
        self.corr_hist.push(c);
        let keep = (3.0 * self.rate / HOP as f32) as usize;
        if self.corr_hist.len() > keep {
            self.corr_hist.remove(0);
        }
        let hold = self.kind != 3 || self.p.on(mm::HOLD);
        if hold {
            self.corr_min = self.corr_min.min(c);
            self.lfe_min = self.lfe_min.min(self.lfe_corr.value());
            for (k, pc) in self.pair_corr.iter().enumerate() {
                self.pair_min[k] = self.pair_min[k].min(pc.value());
            }
        }
        if self.kind == 3 {
            self.analyze();
        }
    }

    fn rms_db(c: &Chan) -> f32 {
        lg_dsp::gain_to_db(c.ms.sqrt())
    }
}

impl Effect for Meter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.layout.len() != 2 {
            self.configure(&[Ch::L, Ch::R]);
        }
        for i in 0..left.len().min(right.len()) {
            self.frame(&[left[i], right[i]]);
        }
        if self.kind == 5 && self.p.on(1) {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        if !SURROUND.contains(&METERS[self.kind]) {
            return false;
        }
        self.configure(layout);
        let n = chans.first().map_or(0, |c| c.len());
        let mut x = vec![0f32; chans.len()];
        for i in 0..n {
            for (k, c) in chans.iter().enumerate() {
                x[k] = c[i];
            }
            self.frame(&x);
        }
        true
    }
    fn set_transport(&mut self, t: lg_dsp::Transport) {
        // playback starting resets the holds and the integrated loudness
        if t.playing && !self.playing && self.kind != 4 {
            self.clear_holds();
            self.loud.reset();
        }
        self.playing = t.playing;
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        let d = lg_dsp::gain_to_db;
        let mut v = vec![("Channels", self.layout.len() as f32)];
        let (chn, pk, rms, tp, hold, seg, segr) = (names("Ch"), names("Peak"), names("RMS"), names("TP"), names("Hold"), names("Seg"), names("SegRMS"));
        let ch_kind = self.p.idx(if self.kind == 3 { mm::LEVEL } else { 0 });
        for (k, c) in self.ch.iter().enumerate().take(17) {
            v.push((chn[k], Ch::ALL.iter().position(|x| *x == self.layout[k]).unwrap_or(0) as f32));
            v.push((pk[k], d(c.pk)));
            v.push((rms[k], Meter::rms_db(c)));
            v.push((tp[k], d(c.tp)));
            // the peak-hold number follows the characteristic (RMS alone: the RMS maximum)
            let h = match ch_kind {
                1 => c.hold_rms,
                3 | 4 => c.hold_tp,
                _ => c.hold_pk,
            };
            v.push((hold[k], d(h)));
            v.push((seg[k], d(c.seg.v)));
            v.push((segr[k], d(c.seg_rms.v)));
        }
        let pick = |i: usize| self.ch.get(i).map_or(-200.0, |c| d(c.pk));
        let rmsd = |i: usize| self.ch.get(i).map_or(-200.0, Meter::rms_db);
        let holdd = |i: usize| self.ch.get(i).map_or(-200.0, |c| d(c.hold_pk));
        v.extend([
            ("Peak L", pick(0)),
            ("Peak R", pick(1)),
            ("RMS L", rmsd(0)),
            ("RMS R", rmsd(1)),
            ("Hold L", holdd(0)),
            ("Hold R", holdd(1)),
            ("True Peak", self.ch.iter().map(|c| d(c.tp)).fold(-200.0, f32::max)),
            ("True Peak Max", self.ch.iter().map(|c| d(c.hold_tp)).fold(-200.0, f32::max)),
            ("Momentary", self.loud.momentary()),
            ("Short-Term", self.loud.short_term()),
            ("Integrated", self.loud.integrated()),
            ("LU Range", self.loud.range()),
            ("Correlation", if self.corr_quiet > 2.0 { 0.0 } else { self.corr.value() }),
            ("Corr Min", self.corr_min),
            ("Corr Lo", self.corr_hist.iter().copied().fold(1.0, f32::min)),
            ("Corr Hi", self.corr_hist.iter().copied().fold(-1.0, f32::max)),
            ("LFE Corr", self.lfe_corr.value()),
            ("LFE Corr Min", self.lfe_min),
            ("Pairs", self.pairs.len() as f32),
        ]);
        let (pa, pb, bal, pc, pm) = (names("PairA"), names("PairB"), names("Bal"), names("PCorr"), names("PCorrMin"));
        let mut bx = 0f32;
        let mut by = 0f32;
        let mut tot = 0f32;
        let fmt = lg_dsp::surround::Format::of_channels(&self.layout).unwrap_or(lg_dsp::surround::Format::S51);
        for (k, (a, b)) in self.pairs.iter().enumerate().take(17) {
            let (ra, rb) = (self.ch[*a].ms.sqrt(), self.ch[*b].ms.sqrt());
            v.push((pa[k], *a as f32));
            v.push((pb[k], *b as f32));
            // -1 all at A, +1 all at B
            v.push((bal[k], if ra + rb > 1e-6 { (rb - ra) / (ra + rb) } else { 0.0 }));
            v.push((pc[k], self.pair_corr[k].value()));
            v.push((pm[k], self.pair_min[k]));
        }
        for (k, c) in self.layout.iter().enumerate() {
            if *c == Ch::Lfe {
                continue;
            }
            let (az, _) = lg_dsp::surround::position(fmt, *c);
            let r = self.ch[k].ms.sqrt();
            bx += r * az.to_radians().sin();
            by += r * az.to_radians().cos();
            tot += r;
        }
        v.push(("Bal X", if tot > 1e-6 { bx / tot } else { 0.0 }));
        v.push(("Bal Y", if tot > 1e-6 { by / tot } else { 0.0 }));
        if self.kind == 4 {
            let f = [0.5, 1.0, 2.0][self.p.idx(2).min(2)];
            v.push(("BPM", self.bpm * f));
            v.push(("BPM Locked", self.bpm_locked * f));
        }
        if self.kind == 5 {
            let src = &self.ch[0];
            let m: Vec<f32> = (0..RING).map(|i| {
                let j = (self.at + i) % RING;
                0.5 * (src.ring[j] + self.ch.get(1).map_or(src.ring[j], |c| c.ring[j]))
            }).collect();
            if let Some(f) = crate::kit::yin(&m, self.rate, 30.0, 2000.0) {
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
        match name {
            "spectrum" => self.bands.clone(),
            "spectrum_hold" => self.band_hold.iter().map(|h| lg_dsp::gain_to_db(h.v)).collect(),
            // the goniometer: interleaved (side, mid) points of the chosen pair's last 2048 samples
            "goniometer" => {
                let pair = [(Ch::L, Ch::R), (Ch::Ls, Ch::Rs), (Ch::Lm, Ch::Rm), (Ch::Lc, Ch::Rc)][self.p.idx(mm::PAIR).min(3)];
                let find = |c: Ch| self.layout.iter().position(|x| *x == c);
                let (Some(a), Some(b)) = (find(pair.0), find(pair.1)) else { return Vec::new() };
                (RING - 2048..RING)
                    .flat_map(|i| {
                        let j = (self.at + i) % RING;
                        let (l, r) = (self.ch[a].ring[j], self.ch[b].ring[j]);
                        [(l - r) * 0.7071, (l + r) * 0.7071]
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

plugin_impl!(Meter);


#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

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

    fn sine(n: usize, f: f32, a: f32) -> Vec<f32> {
        (0..n).map(|i| a * (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect()
    }

    /// The surround MultiMeter meters each channel; the LFE is left out of the loudness.
    #[test]
    fn surround_loudness_skips_lfe() {
        let mut m = Meter::new("MultiMeter", 48000.0).unwrap();
        let layout = vec![Ch::L, Ch::R, Ch::C, Ch::Lfe, Ch::Ls, Ch::Rs];
        let x = sine(48000 * 3, 1000.0, 0.1);
        let mut chans: Vec<Vec<f32>> = layout.iter().map(|c| if *c == Ch::L || *c == Ch::R { x.clone() } else { vec![0.0; x.len()] }).collect();
        let mut lay = layout.clone();
        assert!(m.process_surround(&mut chans, &mut lay));
        let i = m.readouts().iter().find(|r| r.0 == "Integrated").unwrap().1;
        assert!((i + 20.0).abs() < 0.5, "{i}");
        // a loud LFE alone changes nothing
        let mut m2 = Meter::new("MultiMeter", 48000.0).unwrap();
        let mut chans: Vec<Vec<f32>> = layout.iter().map(|c| if *c == Ch::Lfe { sine(48000 * 3, 60.0, 0.9) } else { vec![0.0; x.len()] }).collect();
        let mut lay = layout.clone();
        m2.process_surround(&mut chans, &mut lay);
        assert!(m2.readouts().iter().find(|r| r.0 == "Integrated").unwrap().1 < -100.0);
        let n = m2.readouts().iter().find(|r| r.0 == "Channels").unwrap().1;
        assert_eq!(n, 6.0);
        // balance pairs round the room: L-C, C-R, R-Rs, Rs-Ls, Ls-L
        assert_eq!(m2.pairs.len(), 5);
    }

    /// Peak hold numbers stay until Reset or playback starting.
    #[test]
    fn holds_reset_on_play_and_reset() {
        let mut m = Meter::new("Level Meter", 48000.0).unwrap();
        let (mut l, mut r) = (sine(4800, 440.0, 0.5), sine(4800, 440.0, 0.5));
        m.process(&mut l, &mut r);
        let hold = |m: &Meter| m.readouts().iter().find(|r| r.0 == "Hold 0").unwrap().1;
        assert!((hold(&m) + 6.0).abs() < 0.2);
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        m.process(&mut l, &mut r);
        assert!((hold(&m) + 6.0).abs() < 0.2);
        m.set_transport(lg_dsp::Transport { beats: 0.0, bpm: 120.0, playing: true });
        assert!(hold(&m) < -100.0);
        let (mut l, mut r) = (sine(4800, 440.0, 0.25), sine(4800, 440.0, 0.25));
        m.process(&mut l, &mut r);
        m.set(1, 1.0);
        assert!(hold(&m) < -100.0);
    }

    /// The analyzer's detection: Left shows only the left channel's tone.
    #[test]
    fn analyzer_detection() {
        let mut m = Meter::new("MultiMeter", 48000.0).unwrap();
        m.set(mm::DETECTION, 0.0);
        m.set(mm::AN_MODE, 0.0);
        let (mut l, mut r) = (sine(16384, 1000.0, 0.5), sine(16384, 100.0, 0.5));
        m.process(&mut l, &mut r);
        let s = m.curve("spectrum");
        assert_eq!(s.len(), 31);
        let top = (0..31).max_by(|a, b| s[*a].partial_cmp(&s[*b]).unwrap()).unwrap();
        let f0 = 20.0 * 1000f32.powf(top as f32 / 31.0);
        assert!(f0 > 500.0 && f0 < 1100.0, "{f0}");
        m.set(mm::BANDS, 1.0);
        m.process(&mut l, &mut r);
        assert_eq!(m.curve("spectrum").len(), 63);
    }

    /// BPM Counter: found after measuring, :2 halves the display.
    #[test]
    fn bpm_lock_and_halve() {
        let mut m = Meter::new("BPM Counter", 48000.0).unwrap();
        let mut l: Vec<f32> = (0..48000 * 12).map(|i| if i % 24000 < 200 { 0.9 } else { 0.0 }).collect();
        let mut r = l.clone();
        m.process(&mut l, &mut r);
        assert!((m.bpm_locked - 120.0).abs() < 2.0, "{}", m.bpm_locked);
        m.set(2, 0.0);
        let shown = m.readouts().iter().find(|r| r.0 == "BPM Locked").unwrap().1;
        assert!((shown - 60.0).abs() < 1.0);
    }
}
