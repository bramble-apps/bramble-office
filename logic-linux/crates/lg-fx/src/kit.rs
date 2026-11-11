//! More shared parts for the newer effects (Remix FX, Beat Breaker, Step FX, the pedals, the
//! reverbs and pitch effects): a state-variable filter, a small feedback-delay-network reverb,
//! a pitch shifter on two cross-faded delay taps, note divisions for synced effects and a pitch
//! detector.

use crate::util::{Delay, Noise, OnePole, PI, TAU, flush};

/// Topology-preserving state-variable filter (Simper's): low, band, high and notch outputs at
/// 12 dB/octave, stable for fast cutoff sweeps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

/// What one sample of an Svf gives.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    pub fn new(rate: f32, hz: f32, q: f32) -> Svf {
        let mut s = Svf::default();
        s.set(rate, hz, q);
        s
    }
    /// Cutoff (Hz) and Q (0.5 gentle .. 20 ringing).
    #[inline]
    pub fn set(&mut self, rate: f32, hz: f32, q: f32) {
        let hz = hz.clamp(10.0, rate * 0.49);
        self.g = (PI * hz / rate).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        SvfOut { low: v2, band: v1, high: x - self.k * v1 - v2 }
    }
    pub fn clear(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// An eight-line feedback delay network reverb (Householder mixing), stereo in and out.
/// Decay is the RT60 in seconds, size scales the line lengths, damping (Hz) the high-frequency
/// loss in the loop; a short diffuser of four all-passes softens the attack.
pub struct Fdn {
    rate: f32,
    lines: [Delay; 8],
    len: [f32; 8],
    gain: [f32; 8],
    damp: [OnePole; 8],
    diff: [Delay; 4],
    diff_len: [usize; 4],
    pre: [Delay; 2],
    pub predelay: f32,
    lfo: f32,
    pub modulation: f32,
}

const FDN_BASE: [f32; 8] = [0.0297, 0.0371, 0.0411, 0.0437, 0.0503, 0.0571, 0.0617, 0.0683];

impl Fdn {
    pub fn new(rate: f32) -> Fdn {
        let mk = |s: f32| Delay::new((s * rate * 2.2) as usize + 8);
        let diff_len = [(0.0047 * rate) as usize, (0.0036 * rate) as usize, (0.0127 * rate) as usize, (0.0093 * rate) as usize];
        let mut f = Fdn {
            rate,
            lines: FDN_BASE.map(mk),
            len: FDN_BASE.map(|s| s * rate),
            gain: [0.0; 8],
            damp: [OnePole::new(rate, 8000.0); 8],
            diff: diff_len.map(|n| Delay::new(n + 4)),
            diff_len,
            pre: [Delay::new((0.5 * rate) as usize), Delay::new((0.5 * rate) as usize)],
            predelay: 0.0,
            lfo: 0.0,
            modulation: 0.0,
        };
        f.set(2.0, 1.0, 8000.0);
        f
    }
    /// RT60 decay (s), size (0.3..2), damping cutoff (Hz).
    pub fn set(&mut self, decay: f32, size: f32, damp_hz: f32) {
        let size = size.clamp(0.2, 2.0);
        for k in 0..8 {
            self.len[k] = FDN_BASE[k] * self.rate * size;
            let secs = self.len[k] / self.rate;
            self.gain[k] = 10f32.powf(-3.0 * secs / decay.max(0.05));
            self.damp[k].set(self.rate, damp_hz.clamp(200.0, self.rate * 0.45));
        }
    }
    pub fn clear(&mut self) {
        for d in self.lines.iter_mut().chain(self.diff.iter_mut()).chain(self.pre.iter_mut()) {
            d.clear();
        }
        for d in self.damp.iter_mut() {
            d.y = 0.0;
        }
    }
    /// One stereo sample in, the wet stereo sample out.
    #[inline]
    pub fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.pre[0].push(l);
        self.pre[1].push(r);
        let pd = (self.predelay * self.rate).max(0.0);
        let (l, r) = if pd >= 1.0 { (self.pre[0].read_lin(pd), self.pre[1].read_lin(pd)) } else { (l, r) };
        // diffusion on the mono sum
        let mut x = 0.5 * (l + r);
        for (d, n) in self.diff.iter_mut().zip(self.diff_len) {
            let z = d.tap(n.max(1) - 1);
            let v = x + 0.6 * z;
            d.push(v);
            x = z - 0.6 * v;
        }
        self.lfo += 0.7 / self.rate;
        if self.lfo > 1.0 {
            self.lfo -= 1.0;
        }
        let m = (TAU * self.lfo).sin() * self.modulation * 0.0015 * self.rate;
        let mut o = [0f32; 8];
        for k in 0..8 {
            let d = self.len[k] + if k < 2 { m * (1.0 - 2.0 * k as f32) } else { 0.0 };
            o[k] = self.damp[k].lp(self.lines[k].read(d.max(2.0))) * self.gain[k];
        }
        let sum: f32 = o.iter().sum::<f32>() * 0.25;
        let inj = [l, r, x, x, l, r, x, x];
        for k in 0..8 {
            self.lines[k].push(o[k] - sum + inj[k] * 0.35);
        }
        (o[0] + o[2] + o[4] + o[6], o[1] + o[3] + o[5] + o[7])
    }
}

/// A delay-line pitch shifter: two taps sweep through a short window in opposite phase and
/// are cross-faded (sine windows), shifting by `ratio` (2 = an octave up).
pub struct Shifter {
    line: Delay,
    phase: f32,
    window: f32,
}

impl Shifter {
    pub fn new(rate: f32, window_ms: f32) -> Shifter {
        let window = (window_ms * 0.001 * rate).max(64.0);
        Shifter { line: Delay::new(window as usize * 2 + 8), phase: 0.0, window }
    }
    pub fn set_window(&mut self, rate: f32, ms: f32) {
        self.window = (ms * 0.001 * rate).clamp(64.0, (self.line.capacity() / 2) as f32);
    }
    #[inline]
    pub fn run(&mut self, x: f32, ratio: f32) -> f32 {
        self.line.push(x);
        // the read point moves at (1 - ratio) samples per sample relative to the write point
        self.phase += (1.0 - ratio) / self.window;
        self.phase -= self.phase.floor();
        let mut y = 0.0;
        for k in 0..2 {
            let p = (self.phase + 0.5 * k as f32).fract();
            let d = 1.0 + p * self.window;
            let w = (PI * p).sin();
            y += self.line.read_lin(d) * w * w;
        }
        y
    }
    pub fn clear(&mut self) {
        self.line.clear();
    }
}

/// Straight note divisions synced effects offer, as beats (quarter notes).
pub const DIV_NAMES: &[&str] = &["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"];
pub const DIV_BEATS: [f32; 7] = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125, 0.0625];

/// A division with its variant: straight, dotted (×1.5) or triplet (×2/3).
pub const VARIANTS: &[&str] = &["Straight", "Dotted", "Triplet"];

pub fn div_beats(div: usize, variant: usize) -> f32 {
    DIV_BEATS[div.min(DIV_BEATS.len() - 1)] * [1.0, 1.5, 2.0 / 3.0][variant.min(2)]
}

/// The longer division list (straight, triplet and dotted), as Logic's synced menus give them.
pub const NOTE_NAMES: &[&str] = &["1/1", "1/2", "1/2 T", "1/2 .", "1/4", "1/4 T", "1/4 .", "1/8", "1/8 T", "1/8 .", "1/16", "1/16 T", "1/16 .", "1/32", "1/32 T", "1/64"];
pub const NOTE_BEATS: [f32; 16] = [4.0, 2.0, 4.0 / 3.0, 3.0, 1.0, 2.0 / 3.0, 1.5, 0.5, 1.0 / 3.0, 0.75, 0.25, 1.0 / 6.0, 0.375, 0.125, 1.0 / 12.0, 0.0625];

pub fn note_beats(i: usize) -> f32 {
    NOTE_BEATS[i.min(NOTE_BEATS.len() - 1)]
}

/// Samples per beat at a tempo.
#[inline]
pub fn beat_samples(rate: f32, bpm: f64) -> f64 {
    rate as f64 * 60.0 / bpm.max(1.0)
}

/// Follows the song position sample by sample from the block's transport: free-running at the
/// tempo when the song is stopped (so synced effects still move).
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub beats: f64,
    pub bpm: f64,
    pub playing: bool,
    step: f64,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock { beats: 0.0, bpm: 120.0, playing: false, step: 0.0 }
    }
}

impl Clock {
    pub fn sync(&mut self, t: lg_dsp::Transport, rate: f32) {
        self.bpm = if t.bpm > 1.0 { t.bpm } else { self.bpm };
        if t.playing {
            self.beats = t.beats;
        }
        self.playing = t.playing;
        self.step = self.bpm / 60.0 / rate as f64;
    }
    pub fn set_tempo(&mut self, bpm: f64, rate: f32) {
        self.bpm = bpm.max(1.0);
        self.step = self.bpm / 60.0 / rate as f64;
    }
    /// The position of this sample (beats), then advances one sample.
    #[inline]
    pub fn tick(&mut self) -> f64 {
        let b = self.beats;
        self.beats += self.step;
        b
    }
}

/// White noise at a level (for test signals and noise injections).
pub fn noise(n: &mut Noise, level: f32) -> f32 {
    n.next() * level
}

/// The fundamental of a mono buffer (Hz) by the YIN difference function, or None when it is
/// unvoiced (no clear period between `lo` and `hi` Hz).
pub fn yin(buf: &[f32], rate: f32, lo: f32, hi: f32) -> Option<f32> {
    let max_lag = ((rate / lo) as usize).min(buf.len() / 2);
    let min_lag = ((rate / hi) as usize).max(2);
    if max_lag <= min_lag + 2 {
        return None;
    }
    let w = buf.len() - max_lag;
    let energy: f32 = buf.iter().map(|x| x * x).sum();
    if energy < 1e-6 * buf.len() as f32 {
        return None;
    }
    let mut d = vec![0f32; max_lag + 1];
    for (lag, dv) in d.iter_mut().enumerate().take(max_lag + 1).skip(1) {
        let mut s = 0.0;
        for i in 0..w {
            let e = buf[i] - buf[i + lag];
            s += e * e;
        }
        *dv = s;
    }
    // cumulative mean normalised difference
    let mut run = 0.0;
    let mut cm = vec![1f32; max_lag + 1];
    for lag in 1..=max_lag {
        run += d[lag];
        cm[lag] = if run > 0.0 { d[lag] * lag as f32 / run } else { 1.0 };
    }
    let mut best = None;
    for lag in min_lag..max_lag {
        if cm[lag] < 0.15 && cm[lag] <= cm[lag + 1] {
            best = Some(lag);
            break;
        }
    }
    let lag = best.or_else(|| (min_lag..max_lag).min_by(|&a, &b| cm[a].partial_cmp(&cm[b]).unwrap()).filter(|&l| cm[l] < 0.35))?;
    // parabolic refinement
    let (a, b, c) = (cm[lag - 1], cm[lag], cm[lag + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
    Some(rate / (lag as f32 + off.clamp(-1.0, 1.0)))
}

/// A name built at run time (parameter names of slots, slices, taps), kept for the program's life
/// (each distinct name once).
pub fn leak(s: String) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut v = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = v.iter().find(|n| **n == s) {
        return n;
    }
    let n: &'static str = Box::leak(s.into_boxed_str());
    v.push(n);
    n
}

/// A small deterministic random source (randomize buttons).
#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Linkwitz-Riley crossover (4th order): low and high bands that sum back flat.
#[derive(Clone, Copy, Debug, Default)]
pub struct Crossover {
    lo: [crate::util::Biquad; 2],
    hi: [crate::util::Biquad; 2],
}

impl Crossover {
    pub fn new(rate: f32, hz: f32) -> Crossover {
        let mut c = Crossover::default();
        c.set(rate, hz);
        c
    }
    pub fn set(&mut self, rate: f32, hz: f32) {
        let q = std::f32::consts::FRAC_1_SQRT_2;
        for k in 0..2 {
            self.lo[k].c = crate::util::Coefs::lowpass(rate, hz.clamp(10.0, rate * 0.45), q);
            self.hi[k].c = crate::util::Coefs::highpass(rate, hz.clamp(10.0, rate * 0.45), q);
        }
    }
    /// (low, high); low + high is an all-pass of the input.
    #[inline]
    pub fn run(&mut self, x: f32) -> (f32, f32) {
        let l = self.lo[0].run(x);
        let l = self.lo[1].run(l);
        let h = self.hi[0].run(x);
        let h = self.hi[1].run(h);
        (l, h)
    }
    pub fn clear(&mut self) {
        for b in self.lo.iter_mut().chain(self.hi.iter_mut()) {
            b.clear();
        }
    }
}

/// RMS level of a block in dB.
pub fn rms_db(x: &[f32]) -> f32 {
    if x.is_empty() {
        return -120.0;
    }
    let s: f32 = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    10.0 * s.max(1e-12).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yin_finds_a_sine() {
        let rate = 48000.0;
        let buf: Vec<f32> = (0..4096).map(|i| (TAU * 220.0 * i as f32 / rate).sin()).collect();
        let f = yin(&buf, rate, 50.0, 1000.0).unwrap();
        assert!((f - 220.0).abs() < 1.0, "{f}");
    }

    #[test]
    fn svf_lowpass_attenuates_highs() {
        let rate = 48000.0;
        let mut s = Svf::new(rate, 500.0, 0.707);
        let mut peak: f32 = 0.0;
        for i in 0..48000 {
            let y = s.run((TAU * 8000.0 * i as f32 / rate).sin()).low;
            if i > 4800 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01, "{peak}");
    }

    #[test]
    fn shifter_octave_up() {
        let rate = 48000.0;
        let mut s = Shifter::new(rate, 40.0);
        let out: Vec<f32> = (0..24000).map(|i| s.run((TAU * 200.0 * i as f32 / rate).sin(), 2.0)).collect();
        let f = yin(&out[12000..16096], rate, 50.0, 2000.0).unwrap();
        assert!((f - 400.0).abs() < 8.0, "{f}");
    }

    #[test]
    fn fdn_decays() {
        let mut f = Fdn::new(48000.0);
        f.set(1.0, 1.0, 10000.0);
        let mut e_early = 0.0;
        let mut e_late = 0.0;
        for i in 0..96000 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (l, r) = f.run(x, x);
            if (4800..9600).contains(&i) {
                e_early += l * l + r * r;
            }
            if (52800..57600).contains(&i) {
                e_late += l * l + r * r;
            }
        }
        // one second later: about 60 dB down
        let drop = 10.0 * (e_early / e_late.max(1e-30)).log10();
        assert!(drop > 45.0 && drop < 80.0, "{drop}");
    }
}
