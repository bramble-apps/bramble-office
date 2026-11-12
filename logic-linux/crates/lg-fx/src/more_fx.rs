//! More of the Effects guide's plug-ins, each with the controls the guide lists:
//! Sample Delay, Distortion II, Phase Distortion, Expander, Single Band EQ, Linear Phase EQ,
//! Fuzz-Wah, Spectral Gate, Direction Mixer, Stereo Spread, Modulation Delay, Scanner Vibrato,
//! Spreader, Rotor Cabinet, Ringshifter, SubBass and Test Oscillator.
//!
//! Logic's float layouts for these aren't decoded (Linear Phase EQ excepted: it shares Channel
//! EQ's block, so it uses Channel EQ's parameters), so their ids are our own from 1000 and a
//! project's values for them are not read.

use crate::channel_eq::ChannelEq;
use crate::convolve::Convolver;
use crate::kit::{Crossover, Svf};
use crate::pedals::Rotor;
use crate::util::{Biquad, Coefs, Delay, Follower, Noise, OnePole, PI, Phase, TAU, Values, db, flush, plugin_impl, sine01, tri};
use lg_dsp::{Effect, Param, Plugin, Unit};

const B: u32 = 1000;

// ---------------------------------------------------------------------------------------------
// Sample Delay

static SD_P: [Param; 4] = [
    Param::new(B, "Delay L", 0.0, 10000.0, 0.0, Unit::None),
    Param::new(B + 1, "Delay R", 0.0, 10000.0, 0.0, Unit::None),
    Param::new(B + 2, "Link L & R", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(B + 3, "Unit", &["Samples", "ms"], 0.0),
];

pub struct SampleDelay {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    d: [usize; 2],
}

impl SampleDelay {
    pub fn new(rate: f32) -> SampleDelay {
        SampleDelay { p: Values::new(&SD_P), rate, line: [Delay::new((10.1 * rate) as usize + 10001), Delay::new((10.1 * rate) as usize + 10001)], d: [0; 2] }
    }
    fn update(&mut self) {
        if self.p.on(B + 2) {
            let l = self.p.get(B);
            self.p.set(B + 1, l);
        }
        let ms = self.p.idx(B + 3) == 1;
        for s in 0..2 {
            let v = self.p.get(B + s as u32);
            self.d[s] = if ms { (v * 0.001 * self.rate).round() as usize } else { v.round() as usize };
        }
    }
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        self.d[0].max(self.d[1]) as f32 / self.rate
    }
}

impl Effect for SampleDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                self.line[s].push(*x);
                *x = self.line[s].tap(self.d[s]);
            }
        }
    }
}
plugin_impl!(SampleDelay);

// ---------------------------------------------------------------------------------------------
// Distortion II

static D2_P: [Param; 7] = [
    Param::new(B, "PreGain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(B + 1, "Drive", 0.0, 100.0, 40.0, Unit::Percent),
    Param::new(B + 2, "Tone", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(B + 3, "Type", &["Growl", "Bity", "Nasty", "Class AB soft", "Class AB hard"], 0.0),
    Param::new(B + 4, "Output", -24.0, 12.0, -6.0, Unit::Db),
    Param::new(B + 5, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 6, "Level Compensation", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct DistortionII {
    p: Values,
    rate: f32,
    pre: [Biquad; 2],
    post: [Biquad; 2],
    dc: [OnePole; 2],
}

impl DistortionII {
    pub fn new(rate: f32) -> DistortionII {
        let mut d = DistortionII { p: Values::new(&D2_P), rate, pre: [Biquad::default(); 2], post: [Biquad::default(); 2], dc: [OnePole::new(rate, 10.0); 2] };
        d.update();
        d
    }
    fn update(&mut self) {
        let g = (self.p.get(B + 2) - 50.0) / 50.0 * 9.0;
        for s in 0..2 {
            self.pre[s].c = Coefs::high_shelf(self.rate, 2500.0, 0.7, g);
            self.post[s].c = Coefs::high_shelf(self.rate, 3500.0, 0.7, g * 0.6 - 3.0);
        }
    }
    fn clear(&mut self) {
        for b in self.pre.iter_mut().chain(self.post.iter_mut()) {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

/// Distortion II's curves.
fn d2_curve(t: usize, x: f32) -> f32 {
    match t {
        // two tube stages (asymmetric, soft)
        0 => {
            let a = (x + 0.15).tanh() - 0.15f32.tanh();
            (1.4 * a).tanh()
        }
        // an overdriven blues amp
        1 => (x * 1.2).atan() * 0.8 + 0.2 * (x * 3.0).tanh(),
        // hard
        2 => (x * 2.0).clamp(-1.0, 1.0),
        // crossover distortion: a dead zone around zero, soft or hard
        3 => {
            let d = 0.05;
            let y = if x.abs() < d { x * 0.2 } else { x.signum() * (x.abs() - d * 0.8) };
            y.tanh()
        }
        _ => {
            let d = 0.12;
            let y = if x.abs() < d { 0.0 } else { x.signum() * (x.abs() - d) };
            y.clamp(-0.9, 0.9)
        }
    }
}

impl Effect for DistortionII {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let pre = db(self.p.get(B));
        let drive = db(36.0 * self.p.get(B + 1) / 100.0);
        let t = self.p.idx(B + 3);
        let comp = if self.p.on(B + 6) { 1.0 / (1.0 + 0.2 * drive.log2().max(0.0)) } else { 1.0 };
        let out = db(self.p.get(B + 4)) * comp;
        let mix = self.p.get(B + 5) / 100.0;
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                let y = self.pre[s].run(dry * pre);
                let y = d2_curve(t, y * drive);
                let y = self.post[s].run(y);
                let y = self.dc[s].hp(y);
                *x = dry * (1.0 - mix) + y * out * mix;
            }
        }
    }
}
plugin_impl!(DistortionII);

// ---------------------------------------------------------------------------------------------
// Phase Distortion

static PD_P: [Param; 7] = [
    Param::new(B, "Monitor", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 1, "Cutoff", 20.0, 2000.0, 300.0, Unit::Hz),
    Param::new(B + 2, "Resonance", 0.0, 100.0, 20.0, Unit::Percent),
    Param::new(B + 3, "Intensity", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Phase Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 5, "Delay", 0.1, 10.0, 2.0, Unit::Ms),
    Param::new(B + 6, "Mix", 0.0, 100.0, 50.0, Unit::Percent),
];

pub struct PhaseDistortion {
    p: Values,
    rate: f32,
    lp: [Svf; 2],
    line: [Delay; 2],
}

impl PhaseDistortion {
    pub fn new(rate: f32) -> PhaseDistortion {
        let mut d = PhaseDistortion { p: Values::new(&PD_P), rate, lp: [Svf::default(); 2], line: [Delay::new((0.012 * rate) as usize), Delay::new((0.012 * rate) as usize)] };
        d.update();
        d
    }
    fn update(&mut self) {
        let q = 0.6 + 8.0 * self.p.get(B + 2) / 100.0;
        for f in self.lp.iter_mut() {
            f.set(self.rate, self.p.get(B + 1), q);
        }
    }
    fn clear(&mut self) {
        for f in self.lp.iter_mut() {
            f.clear();
        }
        self.line[0].clear();
        self.line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.01
    }
}

impl Effect for PhaseDistortion {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let maxd = self.p.get(B + 5) * 0.001 * self.rate;
        let int = self.p.get(B + 3) / 100.0;
        let mix = self.p.get(B + 6) / 100.0;
        let monitor = self.p.on(B);
        let rev = self.p.on(B + 4);
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                let low = self.lp[s].run(dry).low;
                if monitor {
                    *x = low;
                    continue;
                }
                // the low band's level moves the delay time: phase modulation of the input
                let m = (low.abs() * 4.0).min(1.0) * int;
                let d = if rev && s == 1 { maxd * (1.0 - m) } else { maxd * m } + 1.0;
                self.line[s].push(dry);
                let y = self.line[s].read_lin(d);
                *x = dry * (1.0 - mix) + y * mix;
            }
        }
    }
}
plugin_impl!(PhaseDistortion);

// ---------------------------------------------------------------------------------------------
// Expander (upward)

static EX_P: [Param; 9] = [
    Param::new(B, "Threshold", -60.0, 0.0, -20.0, Unit::Db),
    Param::new(B + 1, "Ratio", 0.5, 1.0, 0.8, Unit::Ratio),
    Param::new(B + 2, "Gain", -30.0, 30.0, 0.0, Unit::Db),
    Param::new(B + 3, "Auto Gain", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 4, "Knee", 0.0, 1.0, 0.2, Unit::None),
    Param::new(B + 5, "Attack", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(B + 6, "Release", 5.0, 2000.0, 100.0, Unit::Ms),
    Param::menu(B + 7, "Output Clip", &["Off", "Soft", "Hard"], 0.0),
    Param::menu(B + 8, "Detection", &["Peak", "RMS"], 0.0),
];

pub struct Expander {
    p: Values,
    rate: f32,
    env: Follower,
    rms: f32,
    gain_db: f32,
}

impl Expander {
    pub fn new(rate: f32) -> Expander {
        let mut e = Expander { p: Values::new(&EX_P), rate, env: Follower::new(rate, 10.0, 100.0), rms: 0.0, gain_db: 0.0 };
        e.update();
        e
    }
    fn update(&mut self) {
        self.env.set(self.rate, self.p.get(B + 5).max(0.05), self.p.get(B + 6));
    }
    fn clear(&mut self) {
        self.env.env = 0.0;
        self.rms = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// The static curve: output level (dB) for an input level (dB).
    pub fn curve(&self, x: f32) -> f32 {
        let (t, r, k) = (self.p.get(B), self.p.get(B + 1), self.p.get(B + 4) * 12.0);
        let slope = 1.0 / r - 1.0;
        let d = x - t;
        let extra = if d <= -k / 2.0 { 0.0 } else if d >= k / 2.0 || k < 1e-3 { slope * d } else { slope * (d + k / 2.0).powi(2) / (2.0 * k) };
        x + extra.min(24.0)
    }
}

impl Effect for Expander {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rms_mode = self.p.idx(B + 8) == 1;
        let kr = 1.0 - (-1.0 / (0.01 * self.rate)).exp();
        let t = self.p.get(B);
        let auto = if self.p.on(B + 3) { -(self.curve(0.0) - 0.0) } else { 0.0 };
        let gain = db(self.p.get(B + 2) + auto);
        let clip = self.p.idx(B + 7);
        for i in 0..left.len().min(right.len()) {
            let a = left[i].abs().max(right[i].abs());
            self.rms += kr * (a * a - self.rms);
            let lvl = if rms_mode { self.rms.sqrt() } else { a };
            let e = self.env.run(lvl);
            let edb = lg_dsp::gain_to_db(e.max(1e-6));
            let g = self.curve(edb) - edb;
            self.gain_db = g;
            let gg = db(g) * gain;
            for x in [&mut left[i], &mut right[i]] {
                let y = *x * gg;
                *x = match clip {
                    1 => y.tanh(),
                    2 => y.clamp(-1.0, 1.0),
                    _ => y,
                };
            }
        }
        let _ = t;
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Expansion", self.gain_db)]
    }
}
plugin_impl!(Expander);

// ---------------------------------------------------------------------------------------------
// Single Band EQ

static SB_P: [Param; 5] = [
    Param::menu(B, "Mode", &["Low Cut", "Low Shelf", "Parametric", "High Shelf", "High Cut"], 2.0),
    Param::new(B + 1, "Frequency", 20.0, 20000.0, 1000.0, Unit::Hz),
    Param::new(B + 2, "Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::menu(B + 3, "Slope", &["6 dB/Oct", "12 dB/Oct", "18 dB/Oct", "24 dB/Oct", "36 dB/Oct", "48 dB/Oct"], 1.0),
    Param::new(B + 4, "Q Factor", 0.1, 100.0, 0.71, Unit::None),
];

pub struct SingleBandEq {
    p: Values,
    rate: f32,
    bq: [[Biquad; 4]; 2],
}

impl SingleBandEq {
    pub fn new(rate: f32) -> SingleBandEq {
        let mut e = SingleBandEq { p: Values::new(&SB_P), rate, bq: [[Biquad::default(); 4]; 2] };
        e.update();
        e
    }
    fn update(&mut self) {
        let (r, f, g, q) = (self.rate, self.p.get(B + 1).min(self.rate * 0.45), self.p.get(B + 2), self.p.get(B + 4));
        let order = [1, 2, 3, 4, 6, 8][self.p.idx(B + 3).min(5)];
        let mut c = [Coefs::THRU; 4];
        match self.p.idx(B) {
            m @ (0 | 4) => {
                let hp = m == 0;
                let mut k = 0;
                let mut left = order;
                if left % 2 == 1 {
                    c[k] = if hp { Coefs::hp1(r, f) } else { Coefs::lp1(r, f) };
                    k += 1;
                    left -= 1;
                }
                let n2 = left / 2;
                for j in 0..n2 {
                    // Butterworth sections; the last one carries Q
                    let qq = if j + 1 == n2 { q } else { 0.5 / (PI * (2 * j + 1) as f32 / (2.0 * left as f32)).cos() };
                    c[k] = if hp { Coefs::highpass(r, f, qq) } else { Coefs::lowpass(r, f, qq) };
                    k += 1;
                    if k >= 4 {
                        break;
                    }
                }
            }
            1 => c[0] = Coefs::low_shelf(r, f, q.min(4.0), g),
            3 => c[0] = Coefs::high_shelf(r, f, q.min(4.0), g),
            _ => c[0] = Coefs::peak(r, f, q, g),
        }
        for s in 0..2 {
            for k in 0..4 {
                self.bq[s][k].c = c[k];
            }
        }
    }
    fn clear(&mut self) {
        for b in self.bq.iter_mut().flatten() {
            b.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for SingleBandEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                for k in 0..4 {
                    *x = self.bq[s][k].run(*x);
                }
            }
        }
    }
}
plugin_impl!(SingleBandEq);

// ---------------------------------------------------------------------------------------------
// Linear Phase EQ: Channel EQ's bands and block, applied as a symmetric FIR (no phase shift)

const LP_TAPS: usize = 4096;

pub struct LinearPhaseEq {
    rate: f32,
    dry: [Delay; 2],
    eq: ChannelEq,
    conv: [Convolver; 2],
    fir: Vec<f32>,
}

impl LinearPhaseEq {
    pub fn new(rate: f32) -> LinearPhaseEq {
        let mut e = LinearPhaseEq { rate, dry: [Delay::new(LP_TAPS / 2 + 4), Delay::new(LP_TAPS / 2 + 4)], eq: ChannelEq::new(rate), conv: [Convolver::new(LP_TAPS), Convolver::new(LP_TAPS)], fir: vec![0.0; LP_TAPS] };
        e.design(rate);
        e
    }
    /// Frequency sampling: the Channel EQ magnitude on an FFT grid, zero phase, windowed.
    fn design(&mut self, rate: f32) {
        let n = LP_TAPS;
        let mut re = vec![0f32; n];
        let mut im = vec![0f32; n];
        for k in 0..=n / 2 {
            let f = (k as f32 * rate / n as f32).max(1.0);
            let m = db(self.eq.response_db(f));
            re[k] = m;
            if k > 0 && k < n / 2 {
                re[n - k] = m;
            }
        }
        let fft = crate::convolve::Fft::new(n);
        let mut buf: Vec<crate::convolve::C> = re.iter().zip(&im).map(|(a, b)| crate::convolve::C { re: *a, im: *b }).collect();
        fft.run(&mut buf, true);
        for (i, c) in buf.iter().enumerate() {
            im[i] = c.re / n as f32;
        }
        // centre the impulse (circular shift by n/2) and window it
        for i in 0..n {
            let src = (i + n / 2) % n;
            let w = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
            self.fir[i] = im[src] * w;
        }
        for c in self.conv.iter_mut() {
            c.load(&self.fir);
        }
    }
}

impl Plugin for LinearPhaseEq {
    fn params(&self) -> &[Param] {
        self.eq.params()
    }
    fn get(&self, id: u32) -> f32 {
        self.eq.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.eq.set(id, v);
        self.design(self.rate);
    }
    fn reset(&mut self) {
        for c in self.conv.iter_mut() {
            c.clear();
        }
    }
    fn load_floats(&mut self, f: &[f32]) {
        self.eq.load_floats(f);
        self.design(self.rate);
    }
}

impl Effect for LinearPhaseEq {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use crate::channel_eq::{from_proc, to_proc};
        let mode = self.eq.processing();
        let (analyze, post, size, rms, decay) = self.eq.analyzer_settings();
        if analyze && !post {
            self.eq.analyzer.push(left, right, self.rate, size, rms, decay);
        }
        for i in 0..left.len().min(right.len()) {
            let (a, b) = to_proc(mode, left[i], right[i]);
            let e = (self.conv[0].run(a), self.conv[1].run(b));
            // (the dry part waits for the FIR's latency so the halves stay aligned)
            self.dry[0].push(a);
            self.dry[1].push(b);
            let d = (self.dry[0].tap(LP_TAPS / 2), self.dry[1].tap(LP_TAPS / 2));
            (left[i], right[i]) = from_proc(mode, d, e);
        }
        if analyze && post {
            self.eq.analyzer.push(left, right, self.rate, size, rms, decay);
        }
    }
    fn curve(&self, name: &str) -> Vec<f32> {
        self.eq.curve(name)
    }
    fn latency(&self) -> usize {
        LP_TAPS / 2
    }
}

// ---------------------------------------------------------------------------------------------
// Fuzz-Wah

const WAH_TYPES: &[&str] = &["Classic Wah", "Retro Wah", "Modern Wah", "Opto Wah 1", "Opto Wah 2", "Resonant LP", "Resonant HP", "Peak"];

static FW_P: [Param; 16] = [
    Param::new(B, "Auto Wah", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(B + 1, "Wah Type", WAH_TYPES, 0.0),
    Param::new(B + 2, "Auto Gain", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 3, "Relative Q", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Wah Level", -24.0, 12.0, -12.0, Unit::Db),
    Param::new(B + 5, "Depth", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 6, "Attack", 0.0, 500.0, 10.0, Unit::Ms),
    Param::new(B + 7, "Release", 1.0, 2000.0, 200.0, Unit::Ms),
    Param::new(B + 8, "Range Min", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 9, "Range Max", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 10, "Pedal", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(B + 11, "Compressor", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 12, "Ratio", 1.0, 20.0, 3.0, Unit::Ratio),
    Param::new(B + 13, "Fuzz", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 14, "Fuzz Gain", 0.0, 100.0, 40.0, Unit::Percent),
    Param::new(B + 15, "Fuzz Tone", 0.0, 100.0, 50.0, Unit::Percent),
];

pub struct FuzzWah {
    p: Values,
    rate: f32,
    env: Follower,
    comp: Follower,
    wah: [Svf; 2],
    tone: [OnePole; 2],
    agc: Follower,
}

impl FuzzWah {
    pub fn new(rate: f32) -> FuzzWah {
        let mut f = FuzzWah { p: Values::new(&FW_P), rate, env: Follower::new(rate, 10.0, 200.0), comp: Follower::new(rate, 5.0, 150.0), wah: [Svf::default(); 2], tone: [OnePole::new(rate, 3000.0); 2], agc: Follower::new(rate, 50.0, 500.0) };
        f.update();
        f
    }
    fn update(&mut self) {
        self.env.set(self.rate, self.p.get(B + 6).max(0.1), self.p.get(B + 7));
        let t = 800.0 * (10.0f32).powf(self.p.get(B + 15) / 100.0);
        for o in self.tone.iter_mut() {
            o.set(self.rate, t);
        }
    }
    fn clear(&mut self) {
        for w in self.wah.iter_mut() {
            w.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for FuzzWah {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let ty = self.p.idx(B + 1);
        let depth = self.p.get(B + 5) / 100.0;
        let (rmin, rmax) = (self.p.get(B + 8) / 100.0, self.p.get(B + 9) / 100.0);
        let pedal = self.p.get(B + 10) / 100.0;
        let relq = self.p.get(B + 3) / 100.0;
        let level = db(self.p.get(B + 4));
        let ratio = self.p.get(B + 12);
        let fuzz_g = db(6.0 + 40.0 * self.p.get(B + 14) / 100.0);
        let (wah_on, comp_on, fuzz_on, agc_on) = (self.p.on(B), self.p.on(B + 11), self.p.on(B + 13), self.p.on(B + 2));
        for i in 0..left.len().min(right.len()) {
            let lvl = left[i].abs().max(right[i].abs());
            let e = self.env.run(lvl);
            // the pedal plus the automatic wah, inside the Range
            let pos = rmin + (rmax - rmin) * (pedal + depth * (e * 6.0).min(1.0)).clamp(0.0, 1.0);
            let f = 350.0 * (2500.0f32 / 350.0).powf(pos) * if ty == 1 { 0.8 } else { 1.0 };
            let q = (if ty == 2 || ty >= 3 { 4.0 } else { 3.0 + 3.0 * pos }) * (0.4 + 1.2 * relq);
            let cg = if comp_on {
                let c = self.comp.run(lvl);
                let over = lg_dsp::gain_to_db(c.max(1e-6)) + 24.0;
                db(-(over.max(0.0)) * (1.0 - 1.0 / ratio) + 6.0)
            } else {
                1.0
            };
            let agc = if agc_on { 1.0 / (0.3 + 3.0 * self.agc.run(lvl)).max(0.3) } else { 1.0 };
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let mut y = *x * cg;
                if fuzz_on {
                    y = self.tone[s].lp((y * fuzz_g + 0.1).tanh() - 0.1f32.tanh());
                }
                if wah_on {
                    self.wah[s].set(self.rate, f, q);
                    let o = self.wah[s].run(y);
                    y = match ty {
                        5 => o.low,
                        6 => o.high,
                        7 => y + o.band * 2.0,
                        _ => o.band * 1.8 + 0.05 * y,
                    } * level;
                }
                *x = y * agc.min(4.0);
            }
        }
    }
}
plugin_impl!(FuzzWah);

// ---------------------------------------------------------------------------------------------
// Spectral Gate

static SG_P: [Param; 11] = [
    Param::new(B, "Threshold", -60.0, 0.0, -30.0, Unit::Db),
    Param::new(B + 1, "Speed", 0.1, 20.0, 2.0, Unit::Hz),
    Param::new(B + 2, "CF Modulation", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 3, "BW Modulation", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 4, "Center Frequency", 20.0, 20000.0, 1500.0, Unit::Hz),
    // in hertz, as Logic's: the band runs from Center − Bandwidth/2 to Center + Bandwidth/2
    Param::new(B + 5, "Bandwidth", 1.0, 20000.0, 2250.0, Unit::Hz),
    Param::new(B + 6, "Low Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 7, "Super Energy", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 8, "Sub Energy", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 9, "High Level", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 10, "Gain", -24.0, 24.0, 0.0, Unit::Db),
];

pub struct SpectralGate {
    p: Values,
    rate: f32,
    lo: [Crossover; 2],
    hi: [Crossover; 2],
    env: Follower,
    ph: Phase,
    k: u32,
    band: (f32, f32),
}

impl SpectralGate {
    pub fn new(rate: f32) -> SpectralGate {
        let mut g = SpectralGate { p: Values::new(&SG_P), rate, lo: [Crossover::default(); 2], hi: [Crossover::default(); 2], env: Follower::new(rate, 2.0, 60.0), ph: Phase::default(), k: 0, band: (0.0, 0.0) };
        g.place(0.0);
        g
    }
    fn place(&mut self, m: f32) {
        let cf = self.p.get(B + 4) * 2f32.powf(m * 2.0 * self.p.get(B + 2) / 100.0);
        let bw = self.p.get(B + 5) * (1.0 + m * self.p.get(B + 3) / 100.0).max(0.1);
        // (clipped at 20 Hz at the bottom, the band keeps its width upwards)
        let a = (cf - bw / 2.0).max(20.0);
        let b = (a + bw).max(a * 1.01);
        for s in 0..2 {
            self.lo[s].set(self.rate, a.max(20.0));
            self.hi[s].set(self.rate, b.min(self.rate * 0.45));
        }
        self.band = (a.max(20.0), b.min(self.rate * 0.45));
    }
    /// The band's edges (Hz) as last placed.
    pub fn band(&self) -> (f32, f32) {
        self.band
    }
    fn update(&mut self) {
        self.place(0.0);
    }
    fn clear(&mut self) {
        for c in self.lo.iter_mut().chain(self.hi.iter_mut()) {
            c.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for SpectralGate {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let thr = db(self.p.get(B));
        let (lowl, sup, sub, highl) = (self.p.get(B + 6) / 100.0, self.p.get(B + 7) / 100.0, self.p.get(B + 8) / 100.0, self.p.get(B + 9) / 100.0);
        let g = db(self.p.get(B + 10));
        let moving = self.p.get(B + 2) > 0.0 || self.p.get(B + 3) > 0.0;
        for i in 0..left.len().min(right.len()) {
            let m = sine01(self.ph.step(self.rate, self.p.get(B + 1)));
            self.k += 1;
            if moving && self.k % 32 == 0 {
                self.place(m);
            }
            let mut bands = [[0f32; 3]; 2];
            for (s, x) in [left[i], right[i]].into_iter().enumerate() {
                let (low, rest) = self.lo[s].run(x);
                let (band, high) = self.hi[s].run(rest);
                bands[s] = [low, band, high];
            }
            let e = self.env.run(bands[0][1].abs().max(bands[1][1].abs()));
            let bg = if e > thr { sup } else { sub };
            left[i] = (bands[0][0] * lowl + bands[0][1] * bg + bands[0][2] * highl) * g;
            right[i] = (bands[1][0] * lowl + bands[1][1] * bg + bands[1][2] * highl) * g;
        }
    }
}
plugin_impl!(SpectralGate);

// ---------------------------------------------------------------------------------------------
// Direction Mixer

static DM_P: [Param; 9] = [
    Param::menu(B, "Input", &["LR", "MS"], 0.0),
    Param::new(B + 1, "Direction", -180.0, 180.0, 0.0, Unit::None),
    Param::new(B + 2, "Spread", 0.0, 2.0, 1.0, Unit::None),
    Param::new(B + 3, "Split", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 4, "Crossover", 50.0, 10000.0, 500.0, Unit::Hz),
    Param::new(B + 5, "Direction Low", -180.0, 180.0, 0.0, Unit::None),
    Param::new(B + 6, "Spread Low", 0.0, 2.0, 1.0, Unit::None),
    Param::new(B + 7, "Direction High", -180.0, 180.0, 0.0, Unit::None),
    Param::new(B + 8, "Spread High", 0.0, 2.0, 1.0, Unit::None),
];

pub struct DirectionMixer {
    p: Values,
    rate: f32,
    xo: [Crossover; 2],
}

impl DirectionMixer {
    pub fn new(rate: f32) -> DirectionMixer {
        let mut d = DirectionMixer { p: Values::new(&DM_P), rate, xo: [Crossover::new(rate, 500.0); 2] };
        d.update();
        d
    }
    fn update(&mut self) {
        let f = self.p.get(B + 4);
        let r = self.rate;
        for x in self.xo.iter_mut() {
            x.set(r, f);
        }
    }
    fn clear(&mut self) {
        for x in self.xo.iter_mut() {
            x.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

/// Mid / side through a direction (degrees: 0 centre, +90 the mid to the left, −90 to the
/// right, ±180 the sides swapped) and spread (0 mono, 1 as is, 2 twice as wide): (L, R).
pub fn direct(m: f32, s: f32, dir: f32, spread: f32) -> (f32, f32) {
    let a = dir.to_radians();
    let p = a.sin();
    let s2 = s * spread * a.cos();
    (m * (1.0 + p) + s2, m * (1.0 - p) - s2)
}

impl Effect for DirectionMixer {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let ms = self.p.idx(B) == 1;
        let split = self.p.on(B + 3);
        for i in 0..left.len().min(right.len()) {
            let (l, r) = (left[i], right[i]);
            let (m, s) = if ms { (l, r) } else { (0.5 * (l + r), 0.5 * (l - r)) };
            let (a, b) = if split {
                let (ml, mh) = self.xo[0].run(m);
                let (sl, sh) = self.xo[1].run(s);
                let lo = direct(ml, sl, self.p.get(B + 5), self.p.get(B + 6));
                let hi = direct(mh, sh, self.p.get(B + 7), self.p.get(B + 8));
                (lo.0 + hi.0, lo.1 + hi.1)
            } else {
                direct(m, s, self.p.get(B + 1), self.p.get(B + 2))
            };
            left[i] = a;
            right[i] = b;
        }
    }
}

plugin_impl!(DirectionMixer);

// ---------------------------------------------------------------------------------------------
// Stereo Spread

static SS_P: [Param; 5] = [
    Param::new(B, "Lower Intensity", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(B + 1, "Upper Intensity", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(B + 2, "Lower Frequency", 20.0, 2000.0, 300.0, Unit::Hz),
    Param::new(B + 3, "Upper Frequency", 1000.0, 20000.0, 12000.0, Unit::Hz),
    Param::new(B + 4, "Order", 2.0, 12.0, 8.0, Unit::None),
];

pub struct StereoSpread {
    p: Values,
    rate: f32,
    bands: Vec<[Biquad; 2]>,
    weights: Vec<f32>,
}

impl StereoSpread {
    pub fn new(rate: f32) -> StereoSpread {
        let mut s = StereoSpread { p: Values::new(&SS_P), rate, bands: Vec::new(), weights: Vec::new() };
        s.update();
        s
    }
    fn update(&mut self) {
        let n = self.p.get(B + 4).round().clamp(2.0, 12.0) as usize;
        let (lo, hi) = (self.p.get(B + 2), self.p.get(B + 3).min(self.rate * 0.45));
        let (il, iu) = (self.p.get(B) / 100.0, self.p.get(B + 1) / 100.0);
        self.bands.clear();
        self.weights.clear();
        for k in 0..n {
            let f = lo * (hi / lo).powf((k as f32 + 0.5) / n as f32);
            let q = (n as f32 / (hi / lo).log2().max(0.5)) * 1.4;
            let c = Coefs::bandpass(self.rate, f, q.max(0.5));
            self.bands.push([Biquad::new(c), Biquad::new(c)]);
            let t = k as f32 / (n - 1).max(1) as f32;
            // alternating sides: odd bands to the left, even to the right
            self.weights.push((il + (iu - il) * t) * if k % 2 == 0 { 1.0 } else { -1.0 });
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
}

impl Effect for StereoSpread {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            let m = 0.5 * (left[i] + right[i]);
            let mut s = 0.0;
            for (b, w) in self.bands.iter_mut().zip(&self.weights) {
                s += b[0].run(m) * w;
            }
            left[i] += s;
            right[i] -= s;
        }
    }
}
plugin_impl!(StereoSpread);

// ---------------------------------------------------------------------------------------------
// Modulation Delay

static MD_P: [Param; 16] = [
    Param::new(B, "Mod Intensity", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(B + 1, "Time", 0.5, 400.0, 10.0, Unit::Ms),
    Param::new(B + 2, "Feedback", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 3, "De-Warble", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 4, "Constant Mod", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 5, "D-Mode", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 6, "LFO 1 Rate", 0.01, 20.0, 0.5, Unit::Hz),
    Param::new(B + 7, "LFO 2 Rate", 0.01, 20.0, 0.6, Unit::Hz),
    Param::new(B + 8, "Link L & R", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 9, "LFO Mix", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 10, "Phase", 0.0, 360.0, 90.0, Unit::None),
    Param::new(B + 11, "Filter", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 12, "Low Cut", 20.0, 2000.0, 80.0, Unit::Hz),
    Param::new(B + 13, "High Cut", 1000.0, 20000.0, 12000.0, Unit::Hz),
    Param::new(B + 14, "Volume Mod", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 15, "Output Mix", 0.0, 100.0, 50.0, Unit::Percent),
];

pub struct ModulationDelay {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    ph: [Phase; 2],
    fb: [f32; 2],
    lc: [Biquad; 2],
    hc: [Biquad; 2],
    ap: [Biquad; 2],
    dmode: [Biquad; 2],
}

impl ModulationDelay {
    pub fn new(rate: f32) -> ModulationDelay {
        let mut m = ModulationDelay { p: Values::new(&MD_P), rate, line: [Delay::new((0.45 * rate) as usize), Delay::new((0.45 * rate) as usize)], ph: [Phase::default(); 2], fb: [0.0; 2], lc: [Biquad::default(); 2], hc: [Biquad::default(); 2], ap: [Biquad::default(); 2], dmode: [Biquad::default(); 2] };
        m.update();
        m.ph[1].p = 0.25;
        m
    }
    fn update(&mut self) {
        let r = self.rate;
        for s in 0..2 {
            self.lc[s].c = Coefs::highpass(r, self.p.get(B + 12), 0.7);
            self.hc[s].c = Coefs::lowpass(r, self.p.get(B + 13).min(r * 0.45), 0.7);
            self.ap[s].c = Coefs::allpass(r, if s == 0 { self.p.get(B + 12) * 4.0 } else { self.p.get(B + 13) / 4.0 }, 0.7);
            self.dmode[s].c = Coefs::peak(r, if s == 0 { 900.0 } else { 1300.0 }, 1.0, if s == 0 { 3.0 } else { -3.0 });
        }
        if self.p.on(B + 8) {
            let v = self.p.get(B + 6);
            self.p.set(B + 7, v);
            self.ph[1].p = (self.ph[0].p + self.p.get(B + 10) / 360.0).fract();
        }
    }
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.fb = [0.0; 2];
    }
    fn tail_secs(&self) -> f32 {
        0.5
    }
}

impl Effect for ModulationDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let r = self.rate;
        let base = self.p.get(B + 1) * 0.001 * r;
        let int = self.p.get(B) / 100.0;
        let fbk = (self.p.get(B + 2) / 100.0).clamp(-0.95, 0.95);
        let rates = [self.p.get(B + 6), self.p.get(B + 7)];
        let lfo_mix = self.p.get(B + 9) / 100.0;
        let constant = self.p.on(B + 4);
        let dewarble = self.p.on(B + 3);
        let vmod = self.p.get(B + 14) / 100.0;
        let mix = self.p.get(B + 15) / 100.0;
        let (filter, dmode) = (self.p.on(B + 11), self.p.on(B + 5));
        for i in 0..left.len().min(right.len()) {
            let a = [self.ph[0].step(r, rates[0]), self.ph[1].step(r, rates[1])];
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                // the two LFOs mixed (Mix), the depth fixed or shrinking with the rate
                let m = sine01(a[s]) * (1.0 - lfo_mix) + tri(a[1 - s]) * lfo_mix;
                let depth = if constant { 1.0 / rates[s].max(0.1) } else { 1.0 } * int;
                let span = if dewarble { base.min(0.004 * r) * 0.3 } else { (0.003 * r).min(base * 0.95 + 0.002 * r) };
                let d = (base + span * depth * m).max(1.0);
                let dry = *x;
                let mut inp = dry + self.fb[s] * fbk;
                if filter {
                    inp = self.ap[s].run(inp);
                }
                self.line[s].push(inp);
                let mut y = self.line[s].read(d);
                y = { let t_ = self.lc[s].run(y); self.hc[s].run(t_) };
                if dmode {
                    y = self.dmode[s].run(y);
                }
                self.fb[s] = flush(y);
                let vol = 1.0 - vmod * (0.5 + 0.5 * m);
                *x = dry * (1.0 - mix) + y * vol * mix;
            }
        }
    }
}
plugin_impl!(ModulationDelay);

// ---------------------------------------------------------------------------------------------
// Scanner Vibrato

static SV_P: [Param; 5] = [
    Param::menu(B, "Type", &["V1", "V2", "V3", "C1", "C2", "C3"], 3.0),
    Param::new(B + 1, "Depth", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 2, "Stereo Phase", 0.0, 361.0, 361.0, Unit::None),
    Param::new(B + 3, "Rate Left", 0.5, 12.0, 6.8, Unit::Hz),
    Param::new(B + 4, "Rate Right", 0.5, 12.0, 6.8, Unit::Hz),
];

/// A scanner: nine taps along a short delay line (the organ's line box) read in turn by a
/// rotating scanner (triangle sweep), giving pitch vibrato; chorus mixes the dry signal back.
pub struct ScannerVibrato {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    ph: [Phase; 2],
}

impl ScannerVibrato {
    pub fn new(rate: f32) -> ScannerVibrato {
        ScannerVibrato { p: Values::new(&SV_P), rate, line: [Delay::new((0.01 * rate) as usize), Delay::new((0.01 * rate) as usize)], ph: [Phase::default(); 2] }
    }
    fn update(&mut self) {
        let sp = self.p.get(B + 2);
        if sp <= 360.0 {
            self.ph[1].p = (self.ph[0].p + sp / 360.0).fract();
        }
    }
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.01
    }
}

impl Effect for ScannerVibrato {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let ty = self.p.idx(B);
        let span = [0.0012, 0.0022, 0.0038][ty % 3] * self.rate;
        let chorus = ty >= 3;
        let cdepth = self.p.get(B + 1) / 100.0;
        let free = self.p.get(B + 2) > 360.0;
        let rl = self.p.get(B + 3);
        let rr = if free { self.p.get(B + 4) } else { rl };
        for i in 0..left.len().min(right.len()) {
            let a = [self.ph[0].step(self.rate, rl), self.ph[1].step(self.rate, rr)];
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                self.line[s].push(dry);
                // the scanner sweeps the taps back and forth
                let pos = 0.5 + 0.5 * tri(a[s]);
                let y = self.line[s].read_lin(1.0 + span * pos);
                *x = if chorus { dry * (1.0 - 0.5 * cdepth) + y * 0.5 * (1.0 + cdepth) } else { y };
            }
        }
    }
}
plugin_impl!(ScannerVibrato);

// ---------------------------------------------------------------------------------------------
// Spreader

static SP_P: [Param; 4] = [
    Param::new(B, "Rate", 0.01, 10.0, 0.3, Unit::Hz),
    Param::new(B + 1, "Intensity", 0.0, 100.0, 40.0, Unit::Percent),
    Param::new(B + 2, "Channel Delay", 0.0, 1000.0, 20.0, Unit::None),
    Param::new(B + 3, "Mix", 0.0, 100.0, 50.0, Unit::Percent),
];

pub struct Spreader {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    ph: Phase,
}

impl Spreader {
    pub fn new(rate: f32) -> Spreader {
        Spreader { p: Values::new(&SP_P), rate, line: [Delay::new(2048), Delay::new(2048)], ph: Phase::default() }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.03
    }
}

impl Effect for Spreader {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let int = self.p.get(B + 1) / 100.0;
        let cd = self.p.get(B + 2);
        let mix = self.p.get(B + 3) / 100.0;
        for i in 0..left.len().min(right.len()) {
            let m = sine01(self.ph.step(self.rate, self.p.get(B)));
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                self.line[s].push(dry);
                // opposite modulation on the two channels, the right one also delayed
                let d = 2.0 + 0.003 * self.rate * int * (0.5 + 0.5 * if s == 0 { m } else { -m }) + if s == 1 { cd } else { 0.0 };
                let y = self.line[s].read_lin(d);
                *x = dry * (1.0 - mix) + y * mix;
            }
        }
    }
}
plugin_impl!(Spreader);

// ---------------------------------------------------------------------------------------------
// Rotor Cabinet

static RC_P: [Param; 13] = [
    Param::menu(B, "Cabinet", &["Real Cabinet", "Wood", "Proline", "Single Cabinet", "Split Horn"], 0.0),
    Param::menu(B + 1, "Rotation", &["Slow", "Brake", "Fast"], 0.0),
    Param::menu(B + 2, "Motor Control", &["Normal", "Inv", "Sync", "910"], 0.0),
    Param::new(B + 3, "Acceleration", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Max Rate", 3.0, 9.0, 6.7, Unit::Hz),
    Param::menu(B + 5, "Mic Position", &["Front", "Rear"], 0.0),
    Param::menu(B + 6, "Upper Microphone", &["Dynamic", "Condenser", "Mid-Side Mic"], 0.0),
    Param::menu(B + 7, "Lower Microphone", &["Dynamic", "Condenser", "Mid-Side Mic"], 1.0),
    Param::new(B + 8, "Horn", 0.0, 100.0, 80.0, Unit::Percent),
    Param::new(B + 9, "Drum", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 10, "Distance", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(B + 11, "Angle", 0.0, 180.0, 90.0, Unit::None),
    Param::new(B + 12, "Balance", -100.0, 100.0, 0.0, Unit::Percent),
];

pub struct RotorCabinet {
    p: Values,
    rate: f32,
    rotor: Rotor,
    dark: [OnePole; 2],
    bright: [Biquad; 2],
}

impl RotorCabinet {
    pub fn new(rate: f32) -> RotorCabinet {
        let mut r = RotorCabinet { p: Values::new(&RC_P), rate, rotor: Rotor::new(rate), dark: [OnePole::new(rate, 12000.0); 2], bright: [Biquad::default(); 2] };
        r.update();
        r
    }
    fn update(&mut self) {
        let d = self.p.get(B + 10) / 100.0;
        for o in self.dark.iter_mut() {
            o.set(self.rate, 14000.0 * (0.25f32).powf(d));
        }
        let dynamic = self.p.idx(B + 6) == 0;
        for b in self.bright.iter_mut() {
            b.c = if dynamic { Coefs::peak(self.rate, 3500.0, 1.0, 3.0) } else { Coefs::THRU };
        }
    }
    fn clear(&mut self) {
        self.rotor.clear();
    }
    fn tail_secs(&self) -> f32 {
        0.02
    }
}

impl Effect for RotorCabinet {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let max = self.p.get(B + 4);
        let (slow_h, slow_d) = (0.8, 0.67);
        let single = self.p.idx(B) == 3;
        // Motor Control does nothing for the single cabinet (it has no separate rotors)
        let motor = if single { 0 } else { self.p.idx(B + 2) };
        let (ht, dt) = match (self.p.idx(B + 1), motor) {
            (1, _) => (0.0, 0.0),
            (2, 1) => (slow_h, max * 0.9),
            (0, 1) => (max, slow_d),
            // 910 (Memphis): the bass drum stands still at slow speed; the horn still switches
            (0, 3) => (slow_h, 0.0),
            (2, _) => (max, max * 0.9),
            _ => (slow_h, slow_d),
        };
        let acc_pct = self.p.get(B + 3);
        let acc = 0.2 + 4.0 * acc_pct / 100.0;
        let sync = motor == 2;
        if acc_pct <= 0.0 {
            // Acceleration fully left: straight to the target speed
            self.rotor.horn_hz = ht;
            self.rotor.drum_hz = dt;
        } else {
            self.rotor.spin(ht, dt, acc * 0.5, if sync { acc * 0.5 } else { acc * 1.6 }, n);
        }
        let rear = self.p.idx(B + 5) == 1;
        let spread = if self.p.idx(B) == 0 { self.p.get(B + 8) / 100.0 } else { self.p.get(B + 11) / 180.0 };
        let bal = self.p.get(B + 12) / 100.0;
        let (hl, dl) = ((1.0 + bal).min(1.0), (1.0 - bal).min(1.0));
        for i in 0..n {
            let x = 0.5 * (left[i] + right[i]);
            let (mut a, mut b) = self.rotor.run(x, spread, if single { 0.7 } else { hl }, if single { 0.7 } else { dl });
            if rear {
                std::mem::swap(&mut a, &mut b);
            }
            left[i] = self.dark[0].lp(self.bright[0].run(a));
            right[i] = self.dark[1].lp(self.bright[1].run(b));
        }
    }
}
plugin_impl!(RotorCabinet);

// ---------------------------------------------------------------------------------------------
// Ringshifter (frequency shifter / ring modulator)

/// A Hilbert transformer: two all-pass chains whose outputs are 90° apart (Niemitalo's
/// coefficients), for single-sideband frequency shifting.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hilbert {
    a: [[f32; 4]; 4],
    b: [[f32; 4]; 4],
    delay: f32,
}

const HA: [f32; 4] = [0.479_401_6, 0.876_218_6, 0.976_597_6, 0.997_499_2];
const HB: [f32; 4] = [0.161_758_5, 0.733_028_9, 0.945_349_7, 0.990_599_7];

/// y[n] = c (x[n] + y[n−2]) − x[n−2]; state [x1, x2, y1, y2].
#[inline]
fn ap2(st: &mut [f32; 4], c: f32, x: f32) -> f32 {
    let y = c * (x + st[3]) - st[1];
    st[1] = st[0];
    st[0] = x;
    st[3] = st[2];
    st[2] = flush(y);
    y
}

impl Hilbert {
    /// (in-phase, quadrature).
    #[inline]
    pub fn run(&mut self, x: f32) -> (f32, f32) {
        let mut i_ = x;
        let mut q = x;
        for k in 0..4 {
            i_ = ap2(&mut self.a[k], HA[k] * HA[k], i_);
            q = ap2(&mut self.b[k], HB[k] * HB[k], q);
        }
        // the in-phase branch is one sample late relative to the quadrature
        let ip = self.delay;
        self.delay = i_;
        (ip, q)
    }
}

static RS_P: [Param; 24] = [
    Param::menu(B, "Mode", &["Single Freq Shift", "Dual Freq Shift", "OSC Ring Mod", "Side Chain Ring Mod"], 0.0),
    Param::new(B + 1, "Frequency", -5000.0, 5000.0, 50.0, Unit::Hz),
    Param::menu(B + 2, "Scaling", &["Lin", "Exp"], 1.0),
    Param::new(B + 3, "Osc Env Follow", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 4, "Osc LFO", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 5, "Delay Time", 1.0, 2000.0, 250.0, Unit::Ms),
    Param::new(B + 6, "Delay Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 7, "Delay Level", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 8, "Delay Low Cut", 20.0, 2000.0, 20.0, Unit::Hz),
    Param::new(B + 9, "Delay High Cut", 1000.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(B + 10, "Env Follower", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 11, "Sensitivity", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 12, "Env Attack", 0.5, 200.0, 10.0, Unit::Ms),
    Param::new(B + 13, "Env Decay", 5.0, 2000.0, 200.0, Unit::Ms),
    Param::new(B + 14, "LFO", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 15, "LFO Rate", 0.01, 20.0, 1.0, Unit::Hz),
    Param::new(B + 16, "LFO Symmetry", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 17, "Dry/Wet", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 18, "Feedback", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 19, "Stereo Width", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(B + 20, "Mix Env Follow", -100.0, 100.0, 0.0, Unit::Percent),
    // Output section's LFO slider: how far the LFO moves Dry/Wet
    Param::new(B + 21, "Mix LFO", -100.0, 100.0, 0.0, Unit::Percent),
    // LFO Smooth rounds the waveform's corners (0 = the straight-sided shape Symmetry gives)
    Param::new(B + 22, "LFO Smooth", 0.0, 100.0, 0.0, Unit::Percent),
    // LFO Sync: the rate locks to the nearest note value at the project tempo
    Param::new(B + 23, "LFO Sync", 0.0, 1.0, 0.0, Unit::OnOff),
];

/// Ringshifter's LFO at a phase: Symmetry moves the peak (saw down, triangle, saw up), Smooth
/// bends the straight ramps into rounded (sine-like) ones.
fn rs_lfo(p: f32, sym: f32, smooth: f32) -> f32 {
    let t = if p < sym.max(0.01) { 2.0 * p / sym.max(0.01) - 1.0 } else { 1.0 - 2.0 * (p - sym) / (1.0 - sym).max(0.01) };
    t * (1.0 - smooth) + (0.5 * std::f32::consts::PI * t).sin() * smooth
}

pub struct Ringshifter {
    p: Values,
    rate: f32,
    hil: [Hilbert; 2],
    osc: Phase,
    lfo: Phase,
    env: Follower,
    line: [Delay; 2],
    lc: [Biquad; 2],
    hc: [Biquad; 2],
    fb: [f32; 2],
    bpm: f64,
    /// The side chain's block (Side Chain Ring Mod).
    side: Option<(Vec<f32>, Vec<f32>)>,
}

impl Ringshifter {
    pub fn new(rate: f32) -> Ringshifter {
        let mut r = Ringshifter { p: Values::new(&RS_P), rate, hil: [Hilbert::default(); 2], osc: Phase::default(), lfo: Phase::default(), env: Follower::new(rate, 10.0, 200.0), line: [Delay::new((2.1 * rate) as usize), Delay::new((2.1 * rate) as usize)], lc: [Biquad::default(); 2], hc: [Biquad::default(); 2], fb: [0.0; 2], bpm: 120.0, side: None };
        r.update();
        r
    }
    fn update(&mut self) {
        self.env.set(self.rate, self.p.get(B + 12), self.p.get(B + 13));
        for s in 0..2 {
            self.lc[s].c = if self.p.get(B + 8) <= 20.5 { Coefs::THRU } else { Coefs::highpass(self.rate, self.p.get(B + 8), 0.7) };
            self.hc[s].c = if self.p.get(B + 9) >= 19999.0 { Coefs::THRU } else { Coefs::lowpass(self.rate, self.p.get(B + 9).min(self.rate * 0.45), 0.7) };
        }
    }
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.fb = [0.0; 2];
        self.hil = [Hilbert::default(); 2];
    }
    fn tail_secs(&self) -> f32 {
        2.0
    }
}

impl Effect for Ringshifter {
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = Some((left.to_vec(), right.to_vec()));
    }
    fn has_side_chain(&self) -> bool {
        true
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let side = self.side.take();
        let mode = self.p.idx(B);
        let f0 = self.p.get(B + 1);
        let exp = self.p.idx(B + 2) == 1;
        let base = if exp { f0.signum() * (f0.abs() / 5000.0).powi(3) * 5000.0 } else { f0 };
        let (envamt, lfoamt) = (self.p.get(B + 3) / 100.0, self.p.get(B + 4) / 100.0);
        let env_on = self.p.on(B + 10);
        let lfo_on = self.p.on(B + 14);
        let sens = 1.0 + 20.0 * self.p.get(B + 11) / 100.0;
        let sym = self.p.get(B + 16) / 100.0;
        let mix0 = self.p.get(B + 17) / 100.0;
        let fbk = self.p.get(B + 18) / 100.0 * 0.9;
        let width = self.p.get(B + 19) / 100.0;
        let mixenv = self.p.get(B + 20) / 100.0;
        let mixlfo = self.p.get(B + 21) / 100.0;
        let smooth = self.p.get(B + 22) / 100.0;
        let lfo_hz = if self.p.on(B + 23) { crate::util::nearest_sync_hz(self.p.get(B + 15), self.bpm) } else { self.p.get(B + 15) };
        let dsec = if self.p.on(B + 6) { (self.p.get(B + 5) / 250.0).round().max(1.0) * 0.25 * 60.0 / self.bpm as f32 } else { self.p.get(B + 5) * 0.001 };
        let d = (dsec * self.rate).clamp(1.0, 2.0 * self.rate);
        let dlevel = self.p.get(B + 7) / 100.0;
        for i in 0..left.len().min(right.len()) {
            let e = if env_on { (self.env.run(left[i].abs().max(right[i].abs())) * sens).min(1.0) } else { 0.0 };
            let l = if lfo_on {
                rs_lfo(self.lfo.step(self.rate, lfo_hz), sym, smooth)
            } else {
                0.0
            };
            let f = base * (1.0 + envamt * e * 2.0) + lfoamt * l * 500.0;
            let ph = self.osc.step(self.rate, f.abs()) * if f < 0.0 { -1.0 } else { 1.0 };
            let (c, s_) = ((TAU * ph).cos(), (TAU * ph).sin());
            let mix = (mix0 + mixenv * e + mixlfo * l * 0.5).clamp(0.0, 1.0);
            let mut wet = [0f32; 2];
            for (s, x) in [left[i], right[i]].into_iter().enumerate() {
                let inp = x + self.fb[s] * fbk;
                wet[s] = match mode {
                    0 | 1 => {
                        let (a, b) = self.hil[s].run(inp);
                        // up for the left, down for the right in Dual mode
                        if mode == 1 && s == 1 { a * c - b * s_ } else { a * c + b * s_ }
                    }
                    // Side Chain Ring Mod: the input times the side chain audio (the sine
                    // oscillator is off; no side chain = silence, as an unconnected input)
                    3 => inp * side.as_ref().map(|(a, b)| if s == 0 { a.get(i) } else { b.get(i) }.copied().unwrap_or(0.0)).unwrap_or(0.0),
                    _ => inp * c,
                };
                if dlevel > 0.0 {
                    let y = self.line[s].read(d);
                    self.line[s].push(wet[s]);
                    let y = { let t_ = self.lc[s].run(y); self.hc[s].run(t_) };
                    wet[s] = wet[s] * (1.0 - dlevel) + y * dlevel;
                }
                self.fb[s] = flush(wet[s]);
            }
            let m = 0.5 * (wet[0] + wet[1]);
            let (wl, wr) = (m + (wet[0] - m) * (1.0 + width), m + (wet[1] - m) * (1.0 + width));
            left[i] = left[i] * (1.0 - mix) + wl * mix;
            right[i] = right[i] * (1.0 - mix) + wr * mix;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
    }
}
plugin_impl!(Ringshifter);

// ---------------------------------------------------------------------------------------------
// SubBass

static SBASS_P: [Param; 9] = [
    Param::new(B, "High Ratio", 1.0, 8.0, 2.0, Unit::Ratio),
    Param::new(B + 1, "High Center", 20.0, 500.0, 120.0, Unit::Hz),
    Param::new(B + 2, "High Bandwidth", 0.1, 4.0, 1.0, Unit::Octaves),
    Param::new(B + 3, "Freq Mix", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Low Ratio", 1.0, 8.0, 2.0, Unit::Ratio),
    Param::new(B + 5, "Low Center", 20.0, 500.0, 60.0, Unit::Hz),
    Param::new(B + 6, "Low Bandwidth", 0.1, 4.0, 1.0, Unit::Octaves),
    Param::new(B + 7, "Dry", -60.0, 6.0, 0.0, Unit::Db),
    Param::new(B + 8, "Wet", -60.0, 6.0, -6.0, Unit::Db),
];

/// One SubBass band: band-pass, track the period by zero crossings, and play a sine at
/// frequency / ratio that follows the band's level.
#[derive(Clone, Copy, Debug, Default)]
struct SubBand {
    bp: [Biquad; 2],
    last: f32,
    since: f32,
    period: f32,
    ph: f32,
    env: f32,
}

impl SubBand {
    fn set(&mut self, rate: f32, f: f32, bw: f32) {
        let q = 1.0 / (2.0 * (2f32.powf(bw / 2.0) - 2f32.powf(-bw / 2.0))).max(0.1) * 2.0;
        let c = Coefs::bandpass(rate, f, q.clamp(0.3, 10.0));
        self.bp = [Biquad::new(c), Biquad::new(c)];
    }
    #[inline]
    fn run(&mut self, x: f32, ratio: f32) -> f32 {
        let y = self.bp[0].run(x);
        let y = self.bp[1].run(y);
        self.since += 1.0;
        if self.last < 0.0 && y >= 0.0 {
            if self.since > 20.0 {
                self.period = 0.7 * self.period + 0.3 * self.since;
            }
            self.since = 0.0;
        }
        self.last = y;
        self.env += 0.002 * (y.abs() - self.env);
        if self.period > 0.0 {
            self.ph += 1.0 / (self.period * ratio);
            self.ph -= self.ph.floor();
        }
        (TAU * self.ph).sin() * self.env * 1.6
    }
}

pub struct SubBass {
    p: Values,
    rate: f32,
    hi: SubBand,
    lo: SubBand,
}

impl SubBass {
    pub fn new(rate: f32) -> SubBass {
        let mut s = SubBass { p: Values::new(&SBASS_P), rate, hi: SubBand::default(), lo: SubBand::default() };
        s.update();
        s
    }
    fn update(&mut self) {
        self.hi.set(self.rate, self.p.get(B + 1), self.p.get(B + 2));
        self.lo.set(self.rate, self.p.get(B + 5), self.p.get(B + 6));
    }
    fn clear(&mut self) {
        self.hi.env = 0.0;
        self.lo.env = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl Effect for SubBass {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (hr, lr) = (self.p.get(B), self.p.get(B + 4));
        let fm = self.p.get(B + 3) / 100.0;
        let (dry, wet) = (db(self.p.get(B + 7)), db(self.p.get(B + 8)));
        for i in 0..left.len().min(right.len()) {
            let m = 0.5 * (left[i] + right[i]);
            let w = self.hi.run(m, hr) * fm + self.lo.run(m, lr) * (1.0 - fm);
            left[i] = left[i] * dry + w * wet;
            right[i] = right[i] * dry + w * wet;
        }
    }
}
plugin_impl!(SubBass);

// ---------------------------------------------------------------------------------------------
// Test Oscillator

static TO_P: [Param; 12] = [
    Param::menu(B, "Generator", &["Sine", "Square", "Needle Pulse", "White Noise", "Pink Noise", "Sine Sweep"], 0.0),
    Param::new(B + 1, "Decorrelated", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 2, "Anti Aliased", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 3, "Frequency", 1.0, 22000.0, 1000.0, Unit::Hz),
    Param::new(B + 4, "Level", -96.0, 0.0, -18.0, Unit::Db),
    Param::new(B + 5, "Dim", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 6, "Sweep Time", 0.1, 60.0, 5.0, Unit::Seconds),
    Param::menu(B + 7, "Sweep Mode", &["Linear", "Logarithmic"], 1.0),
    Param::new(B + 8, "Start Frequency", 1.0, 22000.0, 20.0, Unit::Hz),
    Param::new(B + 9, "End Frequency", 1.0, 22000.0, 20000.0, Unit::Hz),
    Param::menu(B + 10, "Trigger Mode", &["Single", "Continuous"], 0.0),
    Param::new(B + 11, "Trigger", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct TestOscillator {
    p: Values,
    rate: f32,
    ph: f64,
    noise: [Noise; 2],
    pink: [[f32; 3]; 2],
    sweep_t: f64,
    pub sweep_hz: f32,
}

impl TestOscillator {
    pub fn new(rate: f32) -> TestOscillator {
        TestOscillator { p: Values::new(&TO_P), rate, ph: 0.0, noise: [Noise(0x1234_5671), Noise(0x7654_3211)], pink: [[0.0; 3]; 2], sweep_t: 0.0, sweep_hz: 0.0 }
    }
    fn update(&mut self) {
        if !self.p.on(B + 11) {
            self.sweep_t = 0.0;
        }
    }
    fn clear(&mut self) {
        self.ph = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

/// PolyBLEP: the band-limited correction for a step at phase 0.
#[inline]
fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

impl Effect for TestOscillator {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let gen_ = self.p.idx(B);
        let level = db(self.p.get(B + 4) - if self.p.on(B + 5) { 18.0 } else { 0.0 });
        let aa = self.p.on(B + 2);
        let decor = self.p.on(B + 1);
        for i in 0..left.len().min(right.len()) {
            let hz = if gen_ == 5 {
                if !self.p.on(B + 11) {
                    left[i] = 0.0;
                    right[i] = 0.0;
                    continue;
                }
                let (a, b) = (self.p.get(B + 8) as f64, self.p.get(B + 9) as f64);
                let dur = self.p.get(B + 6) as f64;
                let mut u = self.sweep_t / dur;
                if u >= 1.0 {
                    if self.p.idx(B + 10) == 1 {
                        self.sweep_t = 0.0;
                        u = 0.0;
                    } else {
                        self.p.set(B + 11, 0.0);
                        u = 1.0;
                    }
                }
                self.sweep_t += 1.0 / self.rate as f64;
                let f = if self.p.idx(B + 7) == 1 { a * (b / a).powf(u) } else { a + (b - a) * u };
                self.sweep_hz = f as f32;
                f
            } else {
                self.p.get(B + 3) as f64
            };
            let dt = hz / self.rate as f64;
            self.ph = (self.ph + dt).fract();
            let t = self.ph;
            let (l, r) = match gen_ {
                0 | 5 => {
                    let v = (std::f64::consts::TAU * t).sin() as f32;
                    (v, v)
                }
                1 => {
                    let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                    if aa {
                        v += blep(t, dt) - blep((t + 0.5).fract(), dt);
                    }
                    (v as f32, v as f32)
                }
                2 => {
                    // a one-sample (or band-limited) pulse per period, unipolar
                    let v = if aa { (blep(t, dt) * -1.0).max(0.0) as f32 } else if t < dt { 1.0 } else { 0.0 };
                    (v, v)
                }
                3 => {
                    let a = self.noise[0].next();
                    (a, if decor { self.noise[1].next() } else { a })
                }
                _ => {
                    // pink: Paul Kellet's economy filter on white noise
                    let mut out = [0f32; 2];
                    for c in 0..2 {
                        let w = if c == 1 && !decor { out[0] } else { self.noise[c].next() };
                        if c == 1 && !decor {
                            out[1] = out[0];
                            break;
                        }
                        let s = &mut self.pink[c];
                        s[0] = 0.99765 * s[0] + w * 0.099_046;
                        s[1] = 0.96300 * s[1] + w * 0.296_516_4;
                        s[2] = 0.57000 * s[2] + w * 1.052_691_3;
                        out[c] = (s[0] + s[1] + s[2] + w * 0.1848) * 0.25;
                    }
                    (out[0], out[1])
                }
            };
            left[i] = l * level;
            right[i] = r * level;
        }
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        vec![("Sweep Frequency", self.sweep_hz)]
    }
}
plugin_impl!(TestOscillator);

/// The effects here, by Logic's name.
pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "Sample Delay" | "SmpleDly" => Box::new(SampleDelay::new(rate)),
        "Distortion II" => Box::new(DistortionII::new(rate)),
        "Phase Distortion" => Box::new(PhaseDistortion::new(rate)),
        "Expander" => Box::new(Expander::new(rate)),
        "Single Band EQ" => Box::new(SingleBandEq::new(rate)),
        "Linear Phase EQ" | "Linear EQ" => Box::new(LinearPhaseEq::new(rate)),
        "Fuzz-Wah" => Box::new(FuzzWah::new(rate)),
        "Spectral Gate" => Box::new(SpectralGate::new(rate)),
        "Direction Mixer" => Box::new(DirectionMixer::new(rate)),
        "Stereo Spread" => Box::new(StereoSpread::new(rate)),
        "Modulation Delay" => Box::new(ModulationDelay::new(rate)),
        "Scanner Vibrato" => Box::new(ScannerVibrato::new(rate)),
        "Spreader" => Box::new(Spreader::new(rate)),
        "Rotor Cabinet" => Box::new(RotorCabinet::new(rate)),
        "Ringshifter" => Box::new(Ringshifter::new(rate)),
        "SubBass" => Box::new(SubBass::new(rate)),
        "Test Oscillator" => Box::new(TestOscillator::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["Sample Delay", "Distortion II", "Phase Distortion", "Expander", "Single Band EQ", "Linear Phase EQ", "Fuzz-Wah", "Spectral Gate", "Direction Mixer", "Stereo Spread", "Modulation Delay", "Scanner Vibrato", "Spreader", "Rotor Cabinet", "Ringshifter", "SubBass", "Test Oscillator"];

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(n: usize, hz: f32, a: f32) -> Vec<f32> {
        (0..n).map(|i| (TAU * hz * i as f32 / 48000.0).sin() * a).collect()
    }
    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn spectral_gate_full_range() {
        let g = SpectralGate::new(48000.0);
        let cf = g.params().iter().find(|p| p.name == "Center Frequency").unwrap().clone();
        let bw = g.params().iter().find(|p| p.name == "Bandwidth").unwrap().clone();
        assert_eq!((cf.min, bw.max), (20.0, 20000.0));
        // Center at minimum, Bandwidth at maximum: the band covers everything
        let mut g = SpectralGate::new(48000.0);
        g.set(cf.id, 20.0);
        g.set(bw.id, 20000.0);
        let (lo, hi) = g.band();
        assert!(lo <= 20.0 && hi >= 20000.0, "{lo} {hi}");
    }

    #[test]
    fn rotor_motor_910_instant_acceleration_and_single_cabinet() {
        let spin = |set: &[(u32, f32)]| {
            let mut r = RotorCabinet::new(48000.0);
            for &(id, v) in set {
                r.set(id, v);
            }
            let (mut l, mut rr) = (vec![0.1; 512], vec![0.1; 512]);
            for _ in 0..400 {
                r.process(&mut l, &mut rr);
            }
            (r.rotor.horn_hz, r.rotor.drum_hz)
        };
        // 910 at slow: drum stopped, horn turning; fast: both run
        let (h, d) = spin(&[(B + 2, 3.0), (B + 1, 0.0), (B + 3, 0.0)]);
        assert!(h > 0.5 && d.abs() < 1e-3, "{h} {d}");
        let (h, d) = spin(&[(B + 2, 3.0), (B + 1, 2.0), (B + 3, 0.0)]);
        assert!(h > 5.0 && d > 5.0);
        // single cabinet: Motor Control (Inv) has no effect
        assert_eq!(spin(&[(B, 3.0), (B + 2, 1.0), (B + 1, 2.0)]), spin(&[(B, 3.0), (B + 2, 0.0), (B + 1, 2.0)]));
        // Acceleration 0: the new speed at once (one block)
        let mut r = RotorCabinet::new(48000.0);
        r.set(B + 3, 0.0);
        r.set(B + 1, 2.0);
        let (mut l, mut rr) = (vec![0.1; 64], vec![0.1; 64]);
        r.process(&mut l, &mut rr);
        assert!((r.rotor.horn_hz - r.p.get(B + 4)).abs() < 1e-4);
    }

    #[test]
    fn ringshifter_lfo_moves_dry_wet_smooth_and_sync() {
        // Mix LFO: with the oscillator at 0 Hz in ring mod (wet = input), Dry/Wet 0 and a mix
        // LFO, a constant input… use OSC ring mod at 1 kHz: the output level swings with the LFO
        let run = |set: &[(u32, f32)]| {
            let mut r = Ringshifter::new(48000.0);
            for &(id, v) in [(B, 2.0), (B + 1, 1000.0), (B + 2, 0.0), (B + 17, 50.0), (B + 14, 1.0), (B + 15, 2.0)].iter().chain(set) {
                r.set(id, v);
            }
            let x = sine(48000, 1000.0, 0.5);
            let (mut l, mut rr) = (x.clone(), x);
            r.process(&mut l, &mut rr);
            let lv: Vec<f32> = l.chunks(2400).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
            lv.iter().cloned().fold(0.0, f32::max) - lv.iter().cloned().fold(1.0, f32::min)
        };
        assert!(run(&[]) < 0.02, "{}", run(&[]));
        assert!(run(&[(B + 21, 100.0)]) > 0.08, "{}", run(&[(B + 21, 100.0)]));
        // Smooth rounds the triangle: three-eighths of the way up it sits higher
        assert!((rs_lfo(0.375, 0.5, 0.0) - 0.5).abs() < 1e-5);
        assert!(rs_lfo(0.375, 0.5, 1.0) > 0.7);
        assert!((rs_lfo(0.5, 0.5, 1.0) - 1.0).abs() < 1e-5);
        let mut r = Ringshifter::new(48000.0);
        // Sync: a 1.9 Hz rate locks to a quarter note at 120 bpm
        assert!((crate::util::nearest_sync_hz(1.9, 120.0) - 2.0).abs() < 1e-4);
        r.set(B + 23, 1.0);
        assert!(r.p.on(B + 23));
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
    fn sample_delay_exact() {
        let mut d = SampleDelay::new(48000.0);
        d.set(B, 17.0);
        d.set(B + 1, 3.0);
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        l[0] = 1.0;
        r[0] = 1.0;
        d.process(&mut l, &mut r);
        assert_eq!(l[17], 1.0);
        assert_eq!(r[3], 1.0);
        d.set(B + 3, 1.0); // ms
        d.set(B, 1.0);
        assert_eq!(d.d[0], 48);
    }

    #[test]
    fn single_band_eq_parametric_and_cut() {
        let mut e = SingleBandEq::new(48000.0);
        e.set(B + 2, 6.0);
        let mut l = sine(48000, 1000.0, 0.1);
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        assert!((lg_dsp::gain_to_db(peak(&l[24000..]) / 0.1) - 6.0).abs() < 0.2);
        let mut e = SingleBandEq::new(48000.0);
        e.set(B, 0.0);
        e.set(B + 3, 3.0); // 24 dB/oct
        let mut l = sine(48000, 125.0, 0.1);
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        // three octaves below 1 kHz: about -72 dB
        assert!(lg_dsp::gain_to_db(peak(&l[24000..]) / 0.1) < -60.0);
    }

    #[test]
    fn linear_phase_eq_matches_channel_eq_magnitude_with_latency() {
        let mut e = LinearPhaseEq::new(48000.0);
        // Peak 3 (band 4 at 4..7: on, freq, gain, Q): +6 dB at 1040 Hz
        e.set(4 * 4 + 2, 6.0);
        let lat = e.latency();
        let mut l = sine(48000, 1040.0, 0.1);
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        let g = lg_dsp::gain_to_db(peak(&l[24000..]) / 0.1);
        assert!((g - e.eq.response_db(1040.0)).abs() < 0.3, "{g}");
        // an impulse comes out symmetric around the latency
        let mut e = LinearPhaseEq::new(48000.0);
        e.set(4 * 4 + 2, 6.0);
        let mut l = vec![0.0; 8192];
        l[0] = 1.0;
        let mut r = l.clone();
        e.process(&mut l, &mut r);
        let at = (0..8192).max_by(|a, b| l[*a].abs().partial_cmp(&l[*b].abs()).unwrap()).unwrap();
        assert_eq!(at, lat);
        assert!((l[lat - 5] - l[lat + 5]).abs() < 1e-4);
    }

    #[test]
    fn expander_raises_loud_parts() {
        let mut e = Expander::new(48000.0);
        e.set(B + 1, 0.5); // 0.5:1
        e.set(B + 4, 0.0);
        assert!((e.curve(-10.0) - 0.0).abs() < 0.01, "{}", e.curve(-10.0));
        assert!((e.curve(-30.0) - (-30.0)).abs() < 0.01);
    }

    #[test]
    fn direction_mixer_swaps_and_narrows() {
        let (l, r) = direct(0.0, 0.5, 180.0, 1.0);
        assert!(l < 0.0 && r > 0.0, "{l} {r}");
        let (l, r) = direct(0.5, 0.5, 0.0, 0.0);
        assert!((l - r).abs() < 1e-6);
    }

    #[test]
    fn ringshifter_single_shift_moves_frequency() {
        let mut rs = Ringshifter::new(48000.0);
        rs.set(B + 2, 0.0); // Lin
        rs.set(B + 1, 100.0);
        rs.set(B + 17, 100.0);
        let mut l = sine(48000, 400.0, 0.5);
        let mut r = l.clone();
        rs.process(&mut l, &mut r);
        let f = crate::kit::yin(&l[24000..28096], 48000.0, 100.0, 2000.0).unwrap();
        assert!((f - 500.0).abs() < 10.0, "{f}");
    }

    #[test]
    fn subbass_adds_an_octave_below() {
        let mut s = SubBass::new(48000.0);
        s.set(B + 7, -60.0);
        s.set(B + 8, 0.0);
        s.set(B + 3, 100.0); // high band only: 120 Hz centre, ratio 2
        let mut l = sine(48000, 120.0, 0.5);
        let mut r = l.clone();
        s.process(&mut l, &mut r);
        let f = crate::kit::yin(&l[24000..32000], 48000.0, 30.0, 500.0).unwrap();
        assert!((f - 60.0).abs() < 3.0, "{f}");
    }

    #[test]
    fn test_oscillator_level_and_frequency() {
        let mut t = TestOscillator::new(48000.0);
        let mut l = vec![0.0; 4800];
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        assert!((lg_dsp::gain_to_db(peak(&l)) + 18.0).abs() < 0.05);
        let f = crate::kit::yin(&l[..4096], 48000.0, 100.0, 4000.0).unwrap();
        assert!((f - 1000.0).abs() < 2.0);
    }

    #[test]
    fn sample_delay_and_spread_keep_mono_sum() {
        // Stereo Spread moves energy between sides but the mid stays
        let mut s = StereoSpread::new(48000.0);
        let mut l = sine(4800, 1000.0, 0.3);
        let mut r = l.clone();
        let orig = l.clone();
        s.process(&mut l, &mut r);
        assert!(l.iter().zip(&r).zip(&orig).all(|((a, b), o)| ((a + b) * 0.5 - o).abs() < 1e-5));
    }
}
