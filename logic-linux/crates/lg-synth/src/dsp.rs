//! Small DSP building blocks shared by the instruments. Everything here is allocation-free once
//! constructed (delay lines allocate in `new`, never while rendering).

use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

/// Samples per control-rate step: envelopes of modulation, filter coefficients and pitch are
/// updated once per chunk of this many samples.
pub const CHUNK: usize = 64;

#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// Seconds to a per-sample multiplier that falls 60 dB in that time.
#[inline]
pub fn t60_coef(seconds: f32, rate: f32) -> f32 {
    if seconds <= 0.0 {
        return 0.0;
    }
    (-6.907_755 / (seconds * rate)).exp()
}

/// Time constant (to 1/e) to a one-pole coefficient.
#[inline]
pub fn tau_coef(seconds: f32, rate: f32) -> f32 {
    if seconds <= 0.0 {
        return 0.0;
    }
    (-1.0 / (seconds * rate)).exp()
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn semis(s: f32) -> f32 {
    (s * (std::f32::consts::LN_2 / 12.0)).exp()
}

/// Soft clip with unity slope at 0.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x > 3.0 {
        1.0
    } else if x < -3.0 {
        -1.0
    } else {
        x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
    }
}

#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    soft_clip(x)
}

// ---------------------------------------------------------------------------------------------
// Random numbers

#[derive(Clone, Copy, Debug)]
pub struct Rng(pub u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed.max(1).wrapping_mul(2_654_435_761).max(1))
    }
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// -1..1
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    /// 0..1
    #[inline]
    pub fn unit(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}

// ---------------------------------------------------------------------------------------------
// Sine table

const SINE_BITS: u32 = 12;
const SINE_LEN: usize = 1 << SINE_BITS;

fn sine_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..=SINE_LEN)
            .map(|i| (i as f32 / SINE_LEN as f32 * TAU).sin())
            .collect()
    })
}

/// Call from constructors so the table is built before the audio thread needs it.
pub fn warm_tables() {
    let _ = sine_table();
}

/// sin(2π·phase), phase in cycles (any value; wraps).
#[inline]
pub fn sin_cycles(phase: f32) -> f32 {
    let t = sine_table();
    let p = phase - phase.floor();
    let x = p * SINE_LEN as f32;
    let i = x as usize;
    let f = x - i as f32;
    let i = i.min(SINE_LEN - 1);
    t[i] + (t[i + 1] - t[i]) * f
}

// ---------------------------------------------------------------------------------------------
// Envelopes

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR with a linear attack and exponential decay/release (analog style).
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub stage: Stage,
    pub level: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    pub sustain: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 1.0,
            decay_coef: 0.0,
            release_coef: 0.0,
            sustain: 1.0,
        }
    }
}

impl Adsr {
    /// Times in seconds (decay/release = time to fall about 60 dB).
    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, rate: f32) {
        self.attack_step = 1.0 / (a.max(0.0005) * rate);
        self.decay_coef = t60_coef(d.max(0.001), rate);
        self.release_coef = t60_coef(r.max(0.002), rate);
        self.sustain = s.clamp(0.0, 1.0);
    }
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }
    /// Restart from zero (a fresh note).
    pub fn trigger(&mut self) {
        self.level = 0.0;
        self.stage = Stage::Attack;
    }
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-5 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
    pub fn idle(&self) -> bool {
        self.stage == Stage::Idle
    }
}

/// A short linear fade used when a voice is stolen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fade {
    pub gain: f32,
    pub step: f32,
}

impl Fade {
    pub fn start(&mut self, seconds: f32, rate: f32) {
        self.gain = 1.0;
        self.step = 1.0 / (seconds * rate).max(1.0);
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.gain = (self.gain - self.step).max(0.0);
        self.gain
    }
}

/// One-pole smoother (for parameters, zipper-free).
#[derive(Clone, Copy, Debug, Default)]
pub struct Smooth {
    pub value: f32,
    pub target: f32,
    pub coef: f32,
}

impl Smooth {
    pub fn new(v: f32, seconds: f32, rate: f32) -> Smooth {
        Smooth {
            value: v,
            target: v,
            coef: tau_coef(seconds, rate),
        }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value = self.target + (self.value - self.target) * self.coef;
        self.value
    }
}

// ---------------------------------------------------------------------------------------------
// Filters

#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    pub z: f32,
    pub a: f32,
}

impl OnePole {
    pub fn lowpass_hz(&mut self, hz: f32, rate: f32) {
        self.a = (-TAU * (hz / rate).min(0.49)).exp();
    }
    #[inline]
    pub fn lp(&mut self, x: f32) -> f32 {
        self.z = x + (self.z - x) * self.a;
        self.z
    }
    #[inline]
    pub fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
}

/// DC blocker.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.995 * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvfMode {
    Low,
    Band,
    High,
    Notch,
    Peak,
}

/// Zero-delay-feedback state-variable filter (Simper/Cytomic form).
#[derive(Clone, Copy, Debug)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    pub mode: SvfMode,
}

impl Default for Svf {
    fn default() -> Self {
        let mut s = Svf {
            ic1: 0.0,
            ic2: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 1.0,
            mode: SvfMode::Low,
        };
        s.set(1000.0, 0.707, 48000.0);
        s
    }
}

impl Svf {
    pub fn new(mode: SvfMode, hz: f32, q: f32, rate: f32) -> Svf {
        let mut s = Svf {
            mode,
            ..Default::default()
        };
        s.set(hz, q, rate);
        s
    }
    pub fn set(&mut self, hz: f32, q: f32, rate: f32) {
        let g = (PI * (hz / rate).clamp(1e-5, 0.49)).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        match self.mode {
            SvfMode::Low => v2,
            SvfMode::Band => v1,
            SvfMode::High => x - self.k * v1 - v2,
            SvfMode::Notch => x - self.k * v1,
            SvfMode::Peak => x - self.k * v1 - 2.0 * v2,
        }
    }
    /// Band-pass normalised to unity gain at the centre.
    #[inline]
    pub fn run_bp_norm(&mut self, x: f32) -> f32 {
        let m = self.mode;
        self.mode = SvfMode::Band;
        let y = self.run(x) * self.k;
        self.mode = m;
        y
    }
}

/// RBJ biquad (direct form 1).
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Biquad {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl Biquad {
    fn norm(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    pub fn peaking(&mut self, hz: f32, q: f32, db: f32, rate: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = TAU * (hz / rate).min(0.49);
        let al = w.sin() / (2.0 * q);
        let c = w.cos();
        self.norm(
            1.0 + al * a,
            -2.0 * c,
            1.0 - al * a,
            1.0 + al / a,
            -2.0 * c,
            1.0 - al / a,
        );
    }
    pub fn low_shelf(&mut self, hz: f32, db: f32, rate: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = TAU * (hz / rate).min(0.49);
        let c = w.cos();
        let al = w.sin() / 2.0 * std::f32::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * al;
        self.norm(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        );
    }
    pub fn high_shelf(&mut self, hz: f32, db: f32, rate: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = TAU * (hz / rate).min(0.49);
        let c = w.cos();
        let al = w.sin() / 2.0 * std::f32::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * al;
        self.norm(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        );
    }
    pub fn lowpass(&mut self, hz: f32, q: f32, rate: f32) {
        let w = TAU * (hz / rate).min(0.49);
        let al = w.sin() / (2.0 * q);
        let c = w.cos();
        self.norm(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + al,
            -2.0 * c,
            1.0 - al,
        );
    }
    pub fn highpass(&mut self, hz: f32, q: f32, rate: f32) {
        let w = TAU * (hz / rate).min(0.49);
        let al = w.sin() / (2.0 * q);
        let c = w.cos();
        self.norm(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + al,
            -2.0 * c,
            1.0 - al,
        );
    }
    pub fn bandpass(&mut self, hz: f32, q: f32, rate: f32) {
        let w = TAU * (hz / rate).min(0.49);
        let al = w.sin() / (2.0 * q);
        let c = w.cos();
        self.norm(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al);
    }
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = flush(y);
        y
    }
}

/// Four-pole transistor-ladder style low-pass with resonance and gentle saturation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ladder {
    s: [f32; 4],
    g: f32,
    res: f32,
    pub drive: f32,
    /// Poles used for the output: 2 (12 dB) or 4 (24 dB).
    pub poles: u8,
    pub edgy: bool,
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder {
            s: [0.0; 4],
            g: 0.1,
            res: 0.0,
            drive: 1.0,
            poles: 4,
            edgy: false,
        }
    }
    pub fn set(&mut self, hz: f32, res: f32, rate: f32) {
        let wc = PI * (hz / rate).clamp(1e-5, 0.45);
        let g = wc.tan();
        self.g = g / (1.0 + g);
        self.res = res.clamp(0.0, 1.05);
    }
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let g = self.g;
        let k = self.res * if self.poles == 4 { 4.0 } else { 2.0 };
        // Zero-delay feedback estimate (linearised), then nonlinear stages.
        let g2 = g * g;
        let big_g = if self.poles == 4 { g2 * g2 } else { g2 };
        let sigma = if self.poles == 4 {
            g * g * g * self.s[0] * (1.0 - g)
                + g * g * self.s[1] * (1.0 - g)
                + g * self.s[2] * (1.0 - g)
                + self.s[3] * (1.0 - g)
        } else {
            g * self.s[0] * (1.0 - g) + self.s[1] * (1.0 - g)
        };
        let comp = if self.edgy { 0.0 } else { 0.5 * k };
        let inp = x * self.drive * (1.0 + comp);
        let u = (inp - k * sigma) / (1.0 + k * big_g);
        let u = if self.edgy {
            fast_tanh(u * 1.4) / 1.4
        } else {
            fast_tanh(u)
        };
        let mut v = u;
        let n = self.poles as usize;
        for i in 0..n {
            let y = g * (v - self.s[i]) + self.s[i];
            self.s[i] = flush(2.0 * y - self.s[i]);
            v = y;
        }
        v
    }
}

/// First-order all-pass (for phasers).
#[derive(Clone, Copy, Debug, Default)]
pub struct Allpass1 {
    z: f32,
}

impl Allpass1 {
    #[inline]
    pub fn run(&mut self, x: f32, a: f32) -> f32 {
        let y = -a * x + self.z;
        self.z = flush(x + a * y);
        y
    }
}

// ---------------------------------------------------------------------------------------------
// Oscillators

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Saw,
    Pulse,
    Triangle,
    Sine,
}

/// Band-limited (polyBLEP) oscillator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f32,
    tri: f32,
}

impl Osc {
    /// `inc` = frequency / rate (cycles per sample). `pw` 0..1 for the pulse.
    #[inline]
    pub fn next(&mut self, wave: Wave, inc: f32, pw: f32) -> f32 {
        let t = self.phase;
        let dt = inc.clamp(1e-7, 0.5);
        let out = match wave {
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Pulse => {
                let pw = pw.clamp(0.02, 0.98);
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                let t2 = (t - pw + 1.0) % 1.0;
                v -= poly_blep(t2, dt);
                v
            }
            Wave::Triangle => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t + 0.5) % 1.0, dt);
                // integrate the square
                self.tri = flush(dt * 4.0 * v + (1.0 - dt * 0.05) * self.tri);
                self.tri
            }
            Wave::Sine => sin_cycles(t),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
    /// Returns true when the phase wrapped this sample (for hard sync).
    #[inline]
    pub fn advance(&mut self, inc: f32) -> bool {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            true
        } else {
            false
        }
    }
}

/// A two-pole resonator (decaying sinusoid): cheap modal synthesis.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mode {
    y1: f32,
    y2: f32,
    c: f32,
    r2: f32,
}

impl Mode {
    /// Sets frequency and T60 decay; keeps the current state (so decay can change mid-note).
    pub fn tune(&mut self, hz: f32, t60: f32, rate: f32) {
        let r = t60_coef(t60.max(0.001), rate);
        let w = TAU * (hz / rate).min(0.499);
        self.c = 2.0 * r * w.cos();
        self.r2 = r * r;
    }
    /// Starts ringing with amplitude `amp` (sine phase).
    pub fn strike(&mut self, hz: f32, amp: f32, rate: f32) {
        let w = TAU * (hz / rate).min(0.499);
        self.y1 = amp * w.sin();
        self.y2 = 0.0;
    }
    pub fn add_impulse(&mut self, x: f32) {
        self.y1 += x;
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let y = self.c * self.y1 - self.r2 * self.y2;
        self.y2 = self.y1;
        self.y1 = flush(y);
        y
    }
    #[inline]
    pub fn next_in(&mut self, x: f32) -> f32 {
        let y = x + self.c * self.y1 - self.r2 * self.y2;
        self.y2 = self.y1;
        self.y1 = flush(y);
        y
    }
    pub fn clear(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
    /// Input gain that makes `next_in` roughly unity-gain at resonance.
    pub fn norm_gain(hz: f32, t60: f32, rate: f32) -> f32 {
        let r = t60_coef(t60.max(0.001), rate);
        let w = TAU * (hz / rate).min(0.499);
        (1.0 - r) * 2.0 * w.sin().max(1e-4)
    }
    pub fn energy(&self) -> f32 {
        self.y1.abs().max(self.y2.abs())
    }
}

// ---------------------------------------------------------------------------------------------
// Delay lines

/// Power-of-two circular delay line with fractional reads.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl Delay {
    pub fn new(max_samples: usize) -> Delay {
        let n = (max_samples + 4).next_power_of_two();
        Delay {
            buf: vec![0.0; n],
            mask: n - 1,
            pos: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }
    /// Integer delay (0 = the sample just pushed).
    #[inline]
    pub fn tap(&self, d: usize) -> f32 {
        self.buf[(self.pos.wrapping_sub(d)) & self.mask]
    }
    /// Linear-interpolated delay in samples.
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.mask - 2) as f32);
        let i = d as usize;
        let f = d - i as f32;
        let a = self.tap(i);
        let b = self.tap(i + 1);
        a + (b - a) * f
    }
    /// Cubic (Hermite) interpolated read.
    #[inline]
    pub fn read_cubic(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.mask - 3) as f32);
        let i = d as usize;
        let f = d - i as f32;
        let xm1 = self.tap(i - 1);
        let x0 = self.tap(i);
        let x1 = self.tap(i + 1);
        let x2 = self.tap(i + 2);
        hermite(xm1, x0, x1, x2, f)
    }
}

#[inline]
pub fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b = w + a;
    ((a * t - b) * t + c) * t + x0
}

// ---------------------------------------------------------------------------------------------
// Stereo effects shared by several instruments

/// Classic stereo chorus (two modulated taps, quadrature LFO).
#[derive(Clone, Debug)]
pub struct Chorus {
    dl: Delay,
    dr: Delay,
    phase: f32,
    rate: f32,
}

impl Chorus {
    pub fn new(rate: f32) -> Chorus {
        let n = (rate * 0.05) as usize;
        Chorus {
            dl: Delay::new(n),
            dr: Delay::new(n),
            phase: 0.0,
            rate,
        }
    }
    pub fn reset(&mut self) {
        self.dl.clear();
        self.dr.clear();
    }
    /// `depth` 0..1, `speed` Hz, `mix` 0..1.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], speed: f32, depth: f32, mix: f32) {
        let inc = speed / self.rate;
        let base = 0.007 * self.rate;
        let sweep = 0.004 * self.rate * depth.clamp(0.0, 1.0);
        for i in 0..l.len() {
            self.dl.push(l[i]);
            self.dr.push(r[i]);
            let ml = sin_cycles(self.phase);
            let mr = sin_cycles(self.phase + 0.25);
            let wl = self.dl.read(base + sweep * (0.5 + 0.5 * ml));
            let wr = self.dr.read(base + sweep * (0.5 + 0.5 * mr));
            l[i] = l[i] * (1.0 - 0.5 * mix) + wl * mix * 0.7;
            r[i] = r[i] * (1.0 - 0.5 * mix) + wr * mix * 0.7;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

/// Flanger (short modulated delay with feedback), stereo.
#[derive(Clone, Debug)]
pub struct Flanger {
    dl: Delay,
    dr: Delay,
    phase: f32,
    rate: f32,
    fbl: f32,
    fbr: f32,
}

impl Flanger {
    pub fn new(rate: f32) -> Flanger {
        let n = (rate * 0.02) as usize;
        Flanger {
            dl: Delay::new(n),
            dr: Delay::new(n),
            phase: 0.0,
            rate,
            fbl: 0.0,
            fbr: 0.0,
        }
    }
    pub fn reset(&mut self) {
        self.dl.clear();
        self.dr.clear();
        self.fbl = 0.0;
        self.fbr = 0.0;
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], speed: f32, depth: f32, feedback: f32) {
        let inc = speed / self.rate;
        let base = 0.0008 * self.rate;
        let sweep = 0.005 * self.rate * depth.clamp(0.0, 1.0);
        for i in 0..l.len() {
            self.dl.push(l[i] + self.fbl * feedback);
            self.dr.push(r[i] + self.fbr * feedback);
            let ml = 0.5 + 0.5 * sin_cycles(self.phase);
            let mr = 0.5 + 0.5 * sin_cycles(self.phase + 0.5);
            let wl = self.dl.read(base + sweep * ml);
            let wr = self.dr.read(base + sweep * mr);
            self.fbl = flush(wl);
            self.fbr = flush(wr);
            l[i] = (l[i] + wl) * 0.7;
            r[i] = (r[i] + wr) * 0.7;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

/// Six-stage stereo phaser.
#[derive(Clone, Debug, Default)]
pub struct Phaser {
    apl: [Allpass1; 6],
    apr: [Allpass1; 6],
    phase: f32,
    fbl: f32,
    fbr: f32,
}

impl Phaser {
    pub fn reset(&mut self) {
        *self = Phaser::default();
    }
    /// `color` 0..1 = feedback, `stereo` 0..1 = LFO phase offset between sides.
    pub fn process(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        speed: f32,
        color: f32,
        stereo: f32,
        mix: f32,
        rate: f32,
    ) {
        let inc = speed / rate;
        let fb = color.clamp(0.0, 1.0) * 0.7;
        for i in 0..l.len() {
            let ml = 0.5 + 0.5 * sin_cycles(self.phase);
            let mr = 0.5 + 0.5 * sin_cycles(self.phase + 0.5 * stereo);
            let al = coef_for(200.0 * (16.0f32).powf(ml), rate);
            let ar = coef_for(200.0 * (16.0f32).powf(mr), rate);
            let mut xl = l[i] + self.fbl * fb;
            let mut xr = r[i] + self.fbr * fb;
            for s in 0..6 {
                xl = self.apl[s].run(xl, al);
                xr = self.apr[s].run(xr, ar);
            }
            self.fbl = flush(xl);
            self.fbr = flush(xr);
            l[i] = l[i] * (1.0 - 0.5 * mix) + xl * 0.5 * mix;
            r[i] = r[i] * (1.0 - 0.5 * mix) + xr * 0.5 * mix;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
        fn coef_for(hz: f32, rate: f32) -> f32 {
            let t = (PI * (hz / rate).min(0.45)).tan();
            (t - 1.0) / (t + 1.0)
        }
    }
}

/// Small stereo room (Schroeder/Freeverb style) for body and ambience; fixed buffers.
#[derive(Clone, Debug)]
pub struct Room {
    combs: [(Delay, f32, usize); 8],
    aps: [(Delay, usize); 4],
    pub feedback: f32,
    pub damp: f32,
}

impl Room {
    pub fn new(rate: f32, size: f32) -> Room {
        let s = rate / 44100.0 * size;
        let cl = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let al = [556, 441, 341, 225];
        let combs = cl.map(|n| {
            let d = (n as f32 * s) as usize;
            (Delay::new(d + 32), 0.0f32, d)
        });
        let aps = al.map(|n| {
            let d = (n as f32 * rate / 44100.0) as usize;
            (Delay::new(d + 32), d)
        });
        Room {
            combs,
            aps,
            feedback: 0.7,
            damp: 0.3,
        }
    }
    pub fn reset(&mut self) {
        for c in &mut self.combs {
            c.0.clear();
            c.1 = 0.0;
        }
        for a in &mut self.aps {
            a.0.clear();
        }
    }
    /// Adds the room's wet signal (`mix`) into l/r from the mono sum of the input.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], mix: f32) {
        if mix <= 0.0 {
            return;
        }
        for i in 0..l.len() {
            let x = (l[i] + r[i]) * 0.015;
            let mut ol = 0.0;
            let mut or = 0.0;
            for (k, c) in self.combs.iter_mut().enumerate() {
                let y = c.0.tap(c.2);
                c.1 = flush(y * (1.0 - self.damp) + c.1 * self.damp);
                c.0.push(x + c.1 * self.feedback);
                if k & 1 == 0 { ol += y } else { or += y }
            }
            let mut s = [ol, or];
            for (k, a) in self.aps.iter_mut().enumerate() {
                let ch = k & 1;
                let b = a.0.tap(a.1);
                let inp = s[ch];
                a.0.push(inp + b * 0.5);
                s[ch] = b - inp;
            }
            l[i] += s[0] * mix;
            r[i] += s[1] * mix;
        }
    }
}
