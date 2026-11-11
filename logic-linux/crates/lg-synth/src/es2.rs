//! ES2 stand-in ("Trio Synth"): three oscillators (sine with 100 Digiwaves, triangle, sawtooth,
//! rectangle with pulse width; oscillator 1 FM by oscillator 2; oscillators 2 and 3 synced to
//! or ring modulated with oscillator 1; noise on oscillator 3) balanced on the Triangle, two
//! filters (multimode 12 dB and a 12 / 18 / 24 dB / Fat low-pass) in series or parallel with
//! Filter Blend, Drive and Filter FM, three envelopes, a polyphonic and a monophonic LFO, the
//! ten-slot modulation router with via sources, the Planar Pad, the per-voice Vector Envelope
//! (up to 16 points, sustain and loop points, loop modes, time scaling, curves), macros,
//! controller assignments with Learn, randomizing, unison, glide, distortion and
//! chorus / flanger / phaser.

use crate::dsp::{self, sin_cycles, Chorus, Fade, Flanger, Osc, Phaser, Rng, Svf, SvfMode, Wave, CHUNK};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::retro::table_read;
use crate::synthkit::{self as kit, Lfo, Lp, Shape, SideBuf, Stack};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const OSC_START: &[&str] = &["free", "soft", "hard"];
pub const KB_MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const CBD: &[&str] = &["off", "25%", "50%", "75%", "100%"];
pub const OSC1_WAVES: &[&str] = &["Sine", "Triangle", "Sawtooth", "Rectangle", "FM"];
pub const OSC2_WAVES: &[&str] = &["Sine", "Triangle", "Sawtooth", "Rectangle", "Rect Sync", "Saw Sync", "Ring"];
pub const OSC3_WAVES: &[&str] = &["Sine", "Triangle", "Sawtooth", "Rectangle", "Rect Sync", "Saw Sync", "Noise"];
pub const ROUTING: &[&str] = &["Series", "Parallel"];
pub const F1_MODES: &[&str] = &["Lo", "Hi", "Peak", "BR", "BP"];
pub const F2_MODES: &[&str] = &["12 dB", "18 dB", "24 dB"];
pub const LFO_WAVES: &[&str] = &["Triangle", "Sawtooth", "Rectangle", "Rectangle (unipolar)", "Sample & Hold", "Random (smooth)"];
pub const ENV1_MODES: &[&str] = &["D", "R"];
pub const ENV1_TRIG: &[&str] = &["Poly", "Mono", "Retrig"];
pub const DIST_TYPES: &[&str] = &["Soft", "Hard"];
pub const FXMOD_TYPES: &[&str] = &["Chorus", "Flanger", "Phaser"];
pub const VEC_ENV_MODES: &[&str] = &["Normal", "Finish"];
pub const VEC_LOOP_MODES: &[&str] = &["Off", "Forward", "Backward", "Alternate"];
pub const VEC_MODES: &[&str] = &["off", "Mix", "XY", "Mix+XY"];
pub const VEC_CURVES: &[&str] = &["hold+step", "concave 9", "concave 8", "concave 7", "concave 6", "concave 5", "concave 4", "concave 3", "concave 2", "concave 1", "linear", "convex 1", "convex 2", "convex 3", "convex 4", "convex 5", "convex 6", "convex 7", "convex 8", "convex 9", "step+hold"];
pub const DISPLAY: &[&str] = &["Router", "Vector Envelope"];
pub const BOTTOM_VIEWS: &[&str] = &["Macro", "MIDI", "Macro only"];
pub const MONO_MODES: &[&str] = &["Off", "On (with common base channel 1)", "On (with common base channel 16)"];
pub const RND_DEST: &[&str] = &[
    "All",
    "All except router and Pitch",
    "All except Vector Env",
    "Waves",
    "Digiwaves",
    "Filters",
    "Envs",
    "LFOs",
    "Router",
    "FX",
    "Vector Envelope",
    "Vector Env Mix Pad",
    "Vector Env XY Pad",
    "Vector Env XY Pad X only",
    "Vector Env XY Pad Y only",
    "Vec Env Times",
    "Vec Env Structure",
    "Vec Env Shuffle Times",
];
pub const MM_TARGETS: &[&str] = &[
    "Off", "Pitch123", "Pitch 1", "Pitch 2", "Pitch 3", "Detune", "OscWaves", "Osc1Wave", "Osc2Wave", "Osc3Wave", "OscWaveB", "Osc1WaveB", "Osc2WaveB", "Osc3WaveB", "SineLevl", "OscLScle", "Osc1Levl", "Osc2Levl", "Osc3Levl",
    "Cutoff 1", "Reso 1", "Cutoff 2", "Reso 2", "LPF FM", "Cut 1+2", "Cut1inv2", "FltBlend", "Amp", "Pan", "Diversity", "Lfo1Asym", "Lfo1Curve", "LFO1Rate", "Env2Atck", "Env2Dec", "Env2Rel", "Env2Time", "Env3Atck", "Env3Dec",
    "Env3Rel", "Env3Time", "Glide",
];
const NT: usize = 42;
pub const MM_SOURCES: &[&str] = &[
    "Off", "LFO1", "LFO2", "ENV1", "ENV2", "ENV3", "Pad-X", "Pad-Y", "Max", "Kybd", "Velo", "Bender", "ModWhl", "Touch", "Whl+To", "Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D", "Ctrl E", "Ctrl F", "RndNO1", "RndNO2", "SideCh",
];
/// The Ctrl A–F menus: Off, the assignable controller numbers (1…31 and 70…119: 0 and 32 are
/// Bank Select, 33…63 the fine halves of 14-bit pairs, 64…69 pedals, 120…127 channel mode
/// messages), then Learn.
pub const CTRL_MENU: &[&str] = &["Off", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "100", "101", "102", "103", "104", "105", "106", "107", "108", "109", "110", "111", "112", "113", "114", "115", "116", "117", "118", "119", "Learn"];
pub const CTRL_LEARN: usize = CTRL_MENU.len() - 1;

/// The controller number of a Ctrl A–F menu entry.
pub fn ctrl_cc(index: usize) -> Option<u8> {
    match index {
        1..=31 => Some(index as u8),
        32..=81 => Some((index + 38) as u8),
        _ => None,
    }
}

/// The Ctrl A–F menu entry of a controller number (None: not assignable).
pub fn ctrl_index(cc: u8) -> Option<usize> {
    match cc {
        1..=31 => Some(cc as usize),
        70..=119 => Some(cc as usize - 38),
        _ => None,
    }
}

include!("es2_params.rs");

pub const NP: usize = 280;
/// First parameter of the Vector Envelope's points: five each (time, mix x, mix y, pad x, pad y).
pub const VEC_POINTS: u32 = 200;

/// The router's pitch scale: intensity (0…1) to cents. Fine steps near zero for vibrato,
/// up to three octaves at full intensity.
pub fn pitch_cents(intensity: f32) -> f32 {
    let x = intensity.abs().min(1.0) * 100.0;
    let pts = [(0.0, 0.0), (8.0, 10.0), (20.0, 50.0), (28.0, 100.0), (36.0, 200.0), (76.0, 1200.0), (100.0, 3600.0)];
    let mut c = 3600.0;
    for w in pts.windows(2) {
        if x <= w[1].0 {
            c = w[0].1 + (w[1].1 - w[0].1) * (x - w[0].0) / (w[1].0 - w[0].0);
            break;
        }
    }
    c * intensity.signum()
}

/// A scaled target's factor: 0 leaves the value alone, +1 is ten times, −1 a twenty-fifth.
#[inline]
pub fn scaled(v: f32) -> f32 {
    if v >= 0.0 { 10f32.powf(v.min(1.0)) } else { 25f32.powf(v.max(-1.0)) }
}

/// The Vector Envelope's transition shapes: where between two points the pointer is at
/// `u` (0…1) of the time.
pub fn curve(shape: usize, u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    match shape {
        0 => {
            if u >= 1.0 { 1.0 } else { 0.0 }
        }
        20 => 1.0,
        10 => u,
        k if k < 10 => u.powf(1.0 + (10 - k) as f32 * 0.45),
        k => 1.0 - (1.0 - u).powf(1.0 + (k - 10) as f32 * 0.45),
    }
}

/// ENV 2 / ENV 3: attack, decay to the sustain level, a sustain that can fall to zero or
/// rise to the maximum (Sustain Time), release.
#[derive(Clone, Copy, Default)]
struct Env {
    stage: u8,
    level: f32,
}

impl Env {
    fn trigger(&mut self, restart: bool) {
        if restart {
            self.level = 0.0;
        }
        self.stage = 1;
    }
    fn release(&mut self) {
        if self.stage != 0 {
            self.stage = 4;
        }
    }
    /// One control block of `n` samples: times in seconds, `st` the Sustain Time (−1…1).
    fn step(&mut self, n: usize, a: f32, d: f32, s: f32, st: f32, r: f32, rate: f32) -> f32 {
        let dt = n as f32 / rate;
        match self.stage {
            1 => {
                self.level += dt / a.max(0.0005);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = 2;
                }
            }
            2 => {
                self.level = s + (self.level - s) * dsp::t60_coef(d.max(0.001), rate / n as f32);
                if (self.level - s).abs() < 1e-3 {
                    self.level = s;
                    self.stage = 3;
                }
            }
            3 => {
                if st < -0.01 {
                    // fall: lower is faster
                    let t = 0.05 * 400f32.powf(1.0 + st);
                    self.level *= dsp::t60_coef(t, rate / n as f32);
                } else if st > 0.01 {
                    // rise: higher is faster
                    let t = 0.05 * 400f32.powf(1.0 - st);
                    self.level = 1.0 + (self.level - 1.0) * dsp::t60_coef(t, rate / n as f32);
                }
            }
            4 => {
                self.level *= dsp::t60_coef(r.max(0.002), rate / n as f32);
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = 0;
                }
            }
            _ => {}
        }
        self.level
    }
}

/// ENV 1: attack, then decay (D) or hold and release (R).
fn env1_step(e: &mut Env, n: usize, a: f32, dr: f32, release_mode: bool, rate: f32) -> f32 {
    let dt = n as f32 / rate;
    match e.stage {
        1 => {
            e.level += dt / a.max(0.0005);
            if e.level >= 1.0 {
                e.level = 1.0;
                e.stage = if release_mode { 3 } else { 4 };
            }
        }
        4 => {
            e.level *= dsp::t60_coef(dr.max(0.002), rate / n as f32);
            if e.level < 1e-4 {
                e.level = 0.0;
                e.stage = 0;
            }
        }
        _ => {}
    }
    e.level
}

/// A voice's place in the Vector Envelope.
#[derive(Clone, Copy, Default)]
struct VecEnv {
    /// Mix x, mix y, pad x, pad y now.
    cur: [f32; 4],
    from: [f32; 4],
    to: usize,
    at: usize,
    t: f32,
    dur: f32,
    /// −1 while a loop runs backward.
    back: bool,
    loops: u32,
    holding: bool,
    done: bool,
    released: bool,
    in_loop: bool,
    loop_over: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Sh {
    rate: f32,
    p: [f32; NP],
    lfo2: f32,
    /// ENV 1 in Mono / Retrig trigger mode: one envelope for all voices.
    env1: f32,
    side: f32,
    ctrl: [f32; 6],
    bpm: f32,
    last_pitch: f32,
    mono_unison: bool,
    /// The pitch bend wheel, −1…1 (a source even with a bend range of 0).
    bender: f32,
}

impl Sh {
    #[inline]
    fn g(&self, i: u32) -> f32 {
        self.p[i as usize]
    }
    fn point(&self, k: usize) -> [f32; 4] {
        let b = VEC_POINTS as usize + k.min(15) * 5;
        [self.p[b + 1], self.p[b + 2], self.p[b + 3], self.p[b + 4]]
    }
    fn n_points(&self) -> usize {
        self.g(135).round().clamp(2.0, 15.0) as usize + 1
    }
    /// The loop's rate: None = as set; Some(seconds per cycle) when synced or free.
    fn loop_cycle(&self) -> Option<f32> {
        match kit::bipolar_rate(self.g(132), 30.0, self.bpm) {
            (hz, _) if hz > 0.0 => Some(1.0 / hz),
            _ => None,
        }
    }
    /// Seconds from point `k − 1` to point `k`.
    fn seg_time(&self, k: usize, in_loop: bool) -> f32 {
        let t = self.p[VEC_POINTS as usize + k.min(15) * 5];
        let (s, l) = (self.g(136).round() as usize, self.g(140).round() as usize);
        let (lo, hi) = (s.min(l), s.max(l));
        if in_loop
            && let Some(cycle) = self.loop_cycle()
        {
            let sum: f32 = (lo + 1..=hi).map(|j| self.p[VEC_POINTS as usize + j.min(15) * 5]).sum();
            return if sum > 0.0 { t / sum * cycle } else { cycle / (hi - lo).max(1) as f32 };
        }
        t * self.g(128)
    }
}

impl VecEnv {
    fn start(&mut self, sh: &Sh) {
        *self = VecEnv { cur: sh.point(0), from: sh.point(0), to: 1, at: 0, dur: sh.seg_time(1, false), ..VecEnv::default() };
    }
    fn go(&mut self, to: usize, dur: f32) {
        self.from = self.cur;
        self.to = to;
        self.t = 0.0;
        self.dur = dur;
        self.holding = false;
    }
    fn release(&mut self, sh: &Sh) {
        if self.released {
            return;
        }
        self.released = true;
        let finish = sh.g(137) >= 0.5;
        if finish || self.done {
            return;
        }
        let (s, l) = (sh.g(136).round() as usize, sh.g(140).round() as usize);
        let n = sh.n_points();
        let looping = sh.g(138) >= 0.5;
        // the loop before the sustain point ends with the key; the release starts from where
        // the pointer is
        if looping && l > s {
            if self.holding {
                self.go((s + 1).min(n - 1), sh.seg_time(s + 1, true));
            }
            return;
        }
        if s + 1 < n {
            self.in_loop = false;
            self.loop_over = true;
            self.back = false;
            self.at = s;
            self.go(s + 1, sh.seg_time(s + 1, false));
        } else {
            self.holding = false;
        }
    }
    /// The pointer arrived at point `k`: where next.
    fn arrive(&mut self, k: usize, sh: &Sh, gate: bool) {
        self.at = k;
        self.cur = sh.point(k);
        let n = sh.n_points();
        let (s, l) = (sh.g(136).round().min(n as f32 - 1.0) as usize, sh.g(140).round().min(n as f32 - 1.0) as usize);
        let mode = sh.g(138).round() as i32;
        let finish = sh.g(137) >= 0.5;
        let count = sh.g(139).round() as u32;
        let (lo, hi) = (s.min(l), s.max(l));
        let looping = mode != 0 && lo != hi && !self.loop_over && (finish || if l < s { gate } else { true });
        if looping && (k == hi || (self.in_loop && k == lo)) && (l < s || finish || !gate || self.in_loop) {
            // one more turn of the loop, or leave it
            if self.in_loop && ((mode == 1 && k == hi) || (mode == 2 && k == lo) || (mode == 3 && k == lo)) {
                self.loops += 1;
            }
            if count <= 10 && self.loops >= count {
                self.loop_over = true;
                self.in_loop = false;
                self.back = false;
                // carry on from the sustain point
                if k != s {
                    self.cur = sh.point(s);
                    self.at = s;
                }
                return self.arrive_plain(s, sh, gate, n, finish);
            }
            self.in_loop = true;
            let smooth = sh.g(131);
            match mode {
                1 => {
                    // forward: from the end of the region back to its start
                    if k == hi {
                        if smooth > 0.0 {
                            let cycle: f32 = (lo + 1..=hi).map(|j| sh.seg_time(j, true)).sum();
                            self.go(lo, smooth * 0.5 * cycle);
                            self.at = lo;
                            self.back = false;
                            // the smoothed jump is a segment of its own
                            self.to = lo;
                        } else {
                            self.cur = sh.point(lo);
                            self.at = lo;
                            self.go(lo + 1, sh.seg_time(lo + 1, true));
                        }
                    } else {
                        self.go(k + 1, sh.seg_time(k + 1, true));
                    }
                }
                2 => {
                    // backward: down the region, then from its start up to its end again
                    if k == lo {
                        if smooth > 0.0 {
                            let cycle: f32 = (lo + 1..=hi).map(|j| sh.seg_time(j, true)).sum();
                            self.go(hi, smooth * 0.5 * cycle);
                        } else {
                            self.cur = sh.point(hi);
                            self.at = hi;
                            self.go(hi - 1, sh.seg_time(hi, true));
                        }
                    } else {
                        self.go(k - 1, sh.seg_time(k, true));
                    }
                    self.back = true;
                }
                _ => {
                    // alternate: back and forth
                    if k == hi {
                        self.back = true;
                        self.go(k - 1, sh.seg_time(k, true));
                    } else {
                        self.back = false;
                        self.go(k + 1, sh.seg_time(k + 1, true));
                    }
                }
            }
            return;
        }
        if self.in_loop && k > lo && k < hi {
            // inside the region
            if self.back {
                self.go(k - 1, sh.seg_time(k, true));
            } else {
                self.go(k + 1, sh.seg_time(k + 1, true));
            }
            return;
        }
        self.arrive_plain(k, sh, gate, n, finish);
    }
    fn arrive_plain(&mut self, k: usize, sh: &Sh, gate: bool, n: usize, finish: bool) {
        let s = sh.g(136).round().min(n as f32 - 1.0) as usize;
        let looping_later = sh.g(138) >= 0.5 && sh.g(140).round() as usize > s && !self.loop_over;
        if k + 1 >= n {
            self.done = true;
            self.holding = true;
            return;
        }
        // the sustain point holds while the key is down (Normal mode; Finish plays through,
        // and so does a loop whose loop point lies after the sustain point once the key is up)
        if k == s && gate && !finish {
            self.holding = true;
            return;
        }
        let _ = looping_later;
        self.go(k + 1, sh.seg_time(k + 1, self.in_loop));
    }
    fn step(&mut self, dt: f32, sh: &Sh, gate: bool) {
        if self.done {
            return;
        }
        if self.holding {
            if gate && sh.g(137) < 0.5 {
                return;
            }
            // the key is up (or Finish): leave the sustain point
            self.holding = false;
            let k = self.at;
            let n = sh.n_points();
            if k + 1 >= n {
                self.done = true;
                return;
            }
            let (s, l) = (sh.g(136).round() as usize, sh.g(140).round() as usize);
            let in_loop = sh.g(138) >= 0.5 && l > s && !self.loop_over;
            self.go(k + 1, sh.seg_time(k + 1, in_loop));
            self.in_loop = in_loop;
        }
        self.t += dt;
        let shape = sh.g(168).round() as usize;
        let mut guard = 0;
        while self.t >= self.dur && !self.done && !self.holding && guard < 32 {
            self.t -= self.dur;
            let to = self.to;
            let keep = self.t;
            self.arrive(to, sh, gate);
            self.t = keep;
            guard += 1;
        }
        if !self.done && !self.holding {
            let u = curve(shape, if self.dur > 0.0 { self.t / self.dur } else { 1.0 });
            let b = sh.point(self.to);
            for i in 0..4 {
                self.cur[i] = self.from[i] + (b[i] - self.from[i]) * u;
            }
        }
    }
}

pub(crate) struct Es2Voice {
    on: bool,
    held: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    osc: [Osc; 3],
    sine: f32,
    noise_lp: f32,
    f1: Svf,
    f2: Lp,
    env1: Env,
    env2: Env,
    env3: Env,
    lfo1: Lfo,
    age: f32,
    vec: VecEnv,
    rng: Rng,
    rnd1: f32,
    rnd2: f32,
    rnd2_to: f32,
    analog: f32,
    analog_cut: f32,
    fade: Fade,
    killing: bool,
    level: f32,
    /// Unison: this voice's detune (in units of the Analog amount) and place in the panorama.
    uni: f32,
    upan: f32,
    kick: f32,
    amp_now: f32,
}

impl Es2Voice {
    fn new(seed: u32) -> Es2Voice {
        Es2Voice {
            on: false,
            held: false,
            note: 60,
            vel: 1.0,
            pitch: 60.0,
            target: 60.0,
            osc: [Osc::default(); 3],
            sine: 0.0,
            noise_lp: 0.0,
            f1: Svf::default(),
            f2: Lp::default(),
            env1: Env::default(),
            env2: Env::default(),
            env3: Env::default(),
            lfo1: Lfo::default(),
            age: 0.0,
            vec: VecEnv::default(),
            rng: Rng::new(seed),
            rnd1: 0.0,
            rnd2: 0.0,
            rnd2_to: 0.0,
            analog: 0.0,
            analog_cut: 0.0,
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            uni: 0.0,
            upan: 0.0,
            kick: 0.0,
            amp_now: 0.0,
        }
    }
    pub(crate) fn legato(&mut self, note: u8) {
        self.note = note;
        self.target = note as f32;
        // RndNO2 also changes on legato notes
        self.rnd2_to = self.rng.bipolar();
    }
}

impl Voice<Sh> for Es2Voice {
    fn start(&mut self, note: u8, velocity: u8, sh: &Sh, _ctl: &Ctl) {
        let was = self.on && !self.killing;
        self.on = true;
        self.held = true;
        self.killing = false;
        self.note = note;
        self.target = note as f32;
        if !(sh.g(7) > 0.0 && (was || sh.last_pitch > 0.0)) {
            self.pitch = self.target;
        } else if !was {
            self.pitch = sh.last_pitch;
        }
        self.vel = (velocity.max(1) as f32 - 1.0) / 126.0;
        self.rnd1 = self.rng.bipolar();
        self.rnd2_to = self.rng.bipolar();
        // Analog: every oscillator off by the same random amount, the cutoff a little too. In
        // the mono modes it only spreads the stacked unison voices.
        let analog = sh.g(3);
        let mono = sh.g(8) >= 0.5;
        if mono {
            self.analog = if sh.mono_unison { self.uni * analog * 0.5 } else { 0.0 };
            self.analog_cut = 0.0;
        } else {
            self.analog = self.rng.bipolar() * analog * 0.15 + self.uni * (0.03 + analog * 0.2);
            self.analog_cut = self.rng.bipolar() * analog * 0.03;
        }
        if !was {
            let start = sh.g(5).round() as i32;
            for o in 0..3 {
                self.osc[o].phase = match start {
                    0 => self.rng.unit(),
                    1 => 0.0,
                    _ => 0.25,
                };
            }
            self.sine = 0.0;
            self.f1.reset();
            self.f2.reset();
            self.env3.trigger(true);
            self.env2.trigger(true);
            self.env1.trigger(true);
            // LFO 1 is key-synced: each note starts its cycle from zero
            self.lfo1.reset(0.0);
            self.age = 0.0;
            self.vec.start(sh);
            if sh.g(4) >= 0.5 {
                self.kick = 0.3;
            }
        } else {
            self.env1.trigger(false);
            self.env2.trigger(false);
            self.env3.trigger(false);
            self.vec.start(sh);
        }
    }
    fn release(&mut self, sh: &Sh, _ctl: &Ctl) {
        self.held = false;
        if sh.g(96) >= 0.5 && self.env1.stage == 3 {
            self.env1.stage = 4;
        }
        self.env2.release();
        self.env3.release();
        self.vec.release(sh);
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.003, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &Sh, ctl: &Ctl) {
        let rate = sh.rate;
        let n = l.len();
        let dt = n as f32 / rate;
        let v = self.vel;
        // ---- sources that don't depend on the router
        let lfo1_eg = sh.g(93);
        let fade = if lfo1_eg > 0.001 {
            (self.age / (lfo1_eg * lfo1_eg * 10.0 + 0.001)).min(1.0)
        } else if lfo1_eg < -0.001 {
            (1.0 - self.age / (lfo1_eg * lfo1_eg * 10.0 + 0.001)).max(0.0)
        } else {
            1.0
        };
        self.age += dt;
        // the Vector Envelope moves the Triangle and Planar Pad pointers
        let vmode = sh.g(142).round() as i32;
        let solo = sh.g(134) >= 0.5;
        let sel = sh.point(sh.g(133).round() as usize);
        let (mut mx, mut my, mut px, mut py) = (sh.g(26), sh.g(27), sh.g(122), sh.g(123));
        if vmode != 0 {
            if !solo {
                self.vec.step(dt, sh, self.held);
            }
            let c = if solo { sel } else { self.vec.cur };
            if vmode == 1 || vmode == 3 {
                mx = c[0];
                my = c[1];
            }
            if vmode == 2 || vmode == 3 {
                px = c[2];
                py = c[3];
            }
        }
        let glide_t = sh.g(7);
        self.rnd2 += (self.rnd2_to - self.rnd2) * (1.0 - dsp::tau_coef(glide_t * glide_t * 1.5 + 0.002, rate / n as f32));
        let env1 = if sh.g(97) >= 0.5 { sh.env1 } else { self.env1.level };
        let src = |s: usize, me: &Es2Voice, lfo1: f32| -> f32 {
            match s {
                1 => lfo1,
                2 => sh.lfo2,
                3 => env1,
                4 => me.env2.level,
                5 => me.env3.level,
                6 => px,
                7 => py,
                8 => 1.0,
                9 => (me.note as f32 - 60.0) / 60.0,
                10 => me.vel,
                11 => sh.bender,
                12 => ctl.mod_wheel,
                13 => ctl.aftertouch,
                14 => (ctl.mod_wheel + ctl.aftertouch).min(1.0),
                15..=20 => sh.ctrl[s - 15],
                21 => me.rnd1,
                22 => me.rnd2,
                23 => sh.side,
                _ => 0.0,
            }
        };
        // ---- the router, twice: first the targets that shape LFO 1, then everything
        let mut m = [0.0f32; NT];
        let depth = 1.0 + sh.g(177);
        let mut lfo1 = 0.0;
        for pass in 0..2 {
            m = [0.0; NT];
            for k in 0..10u32 {
                let b = 41 + k * 5;
                if sh.g(147 + k * 2) < 0.5 {
                    continue;
                }
                let t = sh.g(b).round() as usize;
                let s = sh.g(b + 1).round() as usize;
                if t == 0 || t >= NT || s == 0 {
                    continue;
                }
                let via = sh.g(b + 2).round() as usize;
                let amt = if via > 0 {
                    let mut vv = src(via, self, lfo1).clamp(-1.0, 1.0).abs();
                    if sh.g(148 + k * 2) >= 0.5 {
                        vv = 1.0 - vv;
                    }
                    kit::mix(sh.g(b + 3), sh.g(b + 4), vv)
                } else {
                    sh.g(b + 3)
                };
                m[t] += src(s, self, lfo1) * amt * depth;
            }
            // the Planar Pad's two axes
            for (tid, iid, val) in [(124u32, 125u32, px), (126, 127, py)] {
                let t = sh.g(tid).round() as usize;
                if t > 0 && t < NT {
                    m[t] += val * sh.g(iid);
                }
            }
            if pass == 0 {
                // LFO 1: polyphonic, with its rate, asymmetry and curve as targets
                let rate1 = sh.g(92) * scaled(m[32]);
                let val = self.lfo1.step(lfo_shape(sh.g(91)), rate1 * dt, &mut self.rng);
                let ph = self.lfo1.phase;
                lfo1 = shape_lfo(val, ph, sh.g(91).round() as i32, m[30], m[31]) * fade;
            }
        }
        // ---- envelopes (times as scaled targets, macros on top)
        let mac = |i: u32| sh.g(i);
        let tm = |val: f32, max: f32, scale: f32, macro_: f32| kit::env_time(val, max) * scale * 10f32.powf(macro_);
        let a2 = tm(kit::mix(sh.g(101), sh.g(102), v), 10.0, scaled(m[33]) * scaled(m[36]), mac(180));
        let d2 = tm(sh.g(103), 10.0, scaled(m[34]) * scaled(m[36]), mac(181));
        let r2 = tm(sh.g(106), 10.0, scaled(m[35]) * scaled(m[36]), mac(183));
        let a3 = tm(kit::mix(sh.g(108), sh.g(109), v), 10.0, scaled(m[37]) * scaled(m[40]), mac(180));
        let d3 = tm(sh.g(110), 10.0, scaled(m[38]) * scaled(m[40]), mac(181));
        let r3 = tm(sh.g(113), 10.0, scaled(m[39]) * scaled(m[40]), mac(183));
        let s2 = (sh.g(105) + mac(182)).clamp(0.0, 1.0);
        let s3 = (sh.g(112) + mac(182)).clamp(0.0, 1.0);
        env1_step(&mut self.env1, n, kit::env_time(kit::mix(sh.g(98), sh.g(99), v), 10.0), kit::env_time(sh.g(100), 10.0), sh.g(96) >= 0.5, rate);
        self.env2.step(n, a2, d2, s2, sh.g(104), r2, rate);
        let amp_env = self.env3.step(n, a3, d3, s3, sh.g(111), r3, rate);
        // ---- pitch
        let glide = glide_t * glide_t * 1.5 * scaled(m[41]);
        if glide_t > 0.0 {
            self.pitch = self.target + (self.pitch - self.target) * dsp::tau_coef(glide + 0.002, rate / n as f32);
        } else {
            self.pitch = self.target;
        }
        let base = self.pitch + sh.g(2) / 100.0 + ctl.bend + self.analog + pitch_cents(m[1]) / 100.0;
        // Constant Beat Detuning: the detune in cents shrinks up the keyboard so the beats
        // keep their speed (reference: C3, MIDI note 60)
        let cbd = sh.g(10).round() * 0.25;
        let cbd_k = if cbd > 0.0 { 2f32.powf(-(self.pitch - 60.0) / 12.0 * cbd) } else { 1.0 };
        let mdet = sh.g(178);
        let det_mod = pitch_cents(m[5]) / 100.0;
        let mut hz = [0.0f32; 3];
        for o in 0..3 {
            let b = 12 + o as u32 * 5;
            let spread = o as f32 - 1.0;
            let cents = (sh.g(b + 1) + mdet * 30.0 * spread) * cbd_k;
            hz[o] = lg_dsp::midi_hz(base + sh.g(b) + cents / 100.0 + pitch_cents(m[2 + o]) / 100.0 + det_mod * spread).min(rate * 0.45);
        }
        let inc = [hz[0] / rate, hz[1] / rate, hz[2] / rate];
        // ---- waves
        let waves = [sh.g(14).round() as usize, sh.g(19).round() as usize, sh.g(24).round() as usize];
        let wm = sh.g(179);
        let wmod = [m[6] + m[7] + wm, m[6] + m[8] + wm, m[6] + m[9] + wm];
        let wmod_b = [m[10] + m[11], m[10] + m[12], m[10] + m[13]];
        let digi = [sh.g(15), sh.g(20), sh.g(25)];
        // Digiwave position 0…1 across the 100 waves; OscWaveB makes the steps between them hard
        let digi_pos = |o: usize| -> f32 {
            let pos = ((digi[o] - 1.0).max(0.0) / 99.0 + wmod[o] * 0.5).clamp(0.0, 1.0);
            let hard = wmod_b[o].clamp(0.0, 1.0);
            let stepped = (pos * 99.0).round() / 99.0;
            pos + (stepped - pos) * hard
        };
        let dpos = [digi_pos(0), digi_pos(1), digi_pos(2)];
        let fm_amt = (sh.g(190) + wmod[0] * 0.5 + m[11].max(0.0) * 4.0).clamp(0.0, 5.0);
        let pw = [
            (0.5 + wmod[0] * 0.45).clamp(0.03, 0.97),
            (0.5 - sh.g(191) * 0.47 + wmod[1] * 0.45).clamp(0.03, 0.97),
            (0.5 - sh.g(192) * 0.47 + wmod[2] * 0.45).clamp(0.03, 0.97),
        ];
        // noise colour: Osc3Wave below zero darkens (down to 18 Hz), above brightens (to 18 kHz)
        let ncol = wmod[2].clamp(-1.0, 1.0);
        let noise_hz = if ncol < 0.0 { 18.0 * (1000.0f32).powf(1.0 + ncol) } else { 18.0 * (1000.0f32).powf(ncol) };
        let noise_c = (-std::f32::consts::TAU * noise_hz / rate).exp();
        // ---- the Triangle: the three levels
        let (w1, w2, w3) = (my, (1.0 - my) * (1.0 - mx), (1.0 - my) * mx);
        let lscale = (1.0 + m[15] * 3.0).max(0.0);
        let lv = [
            w1 * sh.g(187) * (1.0 + m[16]).max(0.0) * lscale * sh.g(11).round(),
            w2 * sh.g(188) * (1.0 + m[17]).max(0.0) * lscale * sh.g(16).round(),
            w3 * sh.g(189) * (1.0 + m[18]).max(0.0) * lscale * sh.g(21).round(),
        ];
        let sine_lvl = (sh.g(39) + m[14]).clamp(0.0, 1.0);
        // ---- filters
        let filter_on = sh.g(115) >= 0.5;
        let mcut = sh.g(175) * 0.5;
        let key = self.analog_cut + (ctl.brightness - 0.5) * 0.2;
        let c1 = kit::cutoff_hz(sh.g(32) + m[19] + m[24] + m[25] + mcut + key).min(rate * 0.45);
        let c2n = sh.g(36) + m[21] + m[24] - m[25] + mcut + key;
        let r1 = (sh.g(33) + m[20] + sh.g(176)).clamp(0.0, 1.0);
        let r2 = (sh.g(37) + m[22] + sh.g(176)).clamp(0.0, 1.0);
        let f1_mode = sh.g(31).round() as i32;
        self.f1.mode = match f1_mode {
            1 => SvfMode::High,
            3 => SvfMode::Notch,
            2 | 4 => SvfMode::Band,
            _ => SvfMode::Low,
        };
        self.f1.set(c1, 0.5 + r1 * r1 * 120.0, rate);
        let fat = sh.g(35) >= 0.5;
        self.f2.poles = if fat { 4 } else { 2 + sh.g(34).round() as u8 };
        self.f2.fat = fat;
        let blend = (sh.g(30) + m[26]).clamp(-1.0, 1.0);
        let parallel = sh.g(29) >= 0.5;
        let drive = sh.g(28);
        let dg = 1.0 + drive * drive * 24.0;
        let fm2 = (sh.g(38) + m[23]).clamp(0.0, 1.0);
        let c2_wc = std::f32::consts::PI * kit::cutoff_hz(c2n).min(rate * 0.45) / rate;
        if fm2 <= 0.0 {
            self.f2.set_wc(c2_wc, r2);
        }
        // ---- amp, pan
        let vs = sh.g(114);
        let gain = 0.3 * (1.0 - vs + vs * v) * (1.0 + m[27]).max(0.0);
        let pan = (m[28] + self.upan).clamp(-1.0, 1.0);
        let (pl, pr) = ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0));
        let a0 = self.amp_now;
        let a1 = amp_env * gain;
        self.amp_now = a1;
        let mut peak = 0.0f32;
        let ring = waves[1] == 6;
        for i in 0..n {
            // oscillator 2 first: it modulates oscillator 1's frequency and is its ring partner
            let s2raw = match waves[1] {
                0 => {
                    self.osc[1].advance(inc[1]);
                    if digi[1] < 0.5 { sin_cycles(self.osc[1].phase) } else { table_read(dpos[1], self.osc[1].phase, hz[1]) }
                }
                1 => kit::tri(&mut self.osc[1], inc[1]),
                2 | 5 => self.osc[1].next(Wave::Saw, inc[1], 0.5),
                _ => self.osc[1].next(Wave::Pulse, inc[1], pw[1]),
            };
            let s1 = match waves[0] {
                0 => {
                    self.osc[0].advance(inc[0]);
                    if digi[0] < 0.5 { sin_cycles(self.osc[0].phase) } else { table_read(dpos[0], self.osc[0].phase, hz[0]) }
                }
                1 => kit::tri(&mut self.osc[0], inc[0]),
                2 => self.osc[0].next(Wave::Saw, inc[0], 0.5),
                3 => self.osc[0].next(Wave::Pulse, inc[0], pw[0]),
                _ => {
                    // FM: oscillator 2's output moves oscillator 1's frequency up and down
                    let wrapped = self.osc[0].advance((inc[0] * (1.0 + s2raw * fm_amt * 4.0)).clamp(-0.45, 0.45));
                    let _ = wrapped;
                    if self.osc[0].phase < 0.0 {
                        self.osc[0].phase += 1.0;
                    }
                    sin_cycles(self.osc[0].phase)
                }
            };
            let wrapped1 = self.osc[0].phase < inc[0];
            if wrapped1 {
                if waves[1] == 4 || waves[1] == 5 {
                    self.osc[1].phase = 0.0;
                }
                if waves[2] == 4 || waves[2] == 5 {
                    self.osc[2].phase = 0.0;
                }
            }
            let s2 = if ring { s2raw * s1 } else { s2raw };
            let s3 = match waves[2] {
                0 => {
                    self.osc[2].advance(inc[2]);
                    if digi[2] < 0.5 { sin_cycles(self.osc[2].phase) } else { table_read(dpos[2], self.osc[2].phase, hz[2]) }
                }
                1 => kit::tri(&mut self.osc[2], inc[2]),
                2 | 5 => self.osc[2].next(Wave::Saw, inc[2], 0.5),
                6 => {
                    let w = self.rng.bipolar();
                    self.noise_lp = w + (self.noise_lp - w) * noise_c;
                    if ncol < 0.0 { self.noise_lp * 3.0 } else if ncol > 0.0 { w - self.noise_lp } else { w }
                }
                _ => self.osc[2].next(Wave::Pulse, inc[2], pw[2]),
            };
            // the sine at oscillator 1's frequency (Sine Level, Filter FM): there even with
            // oscillator 1 off, and untouched by the FM
            self.sine += inc[0];
            if self.sine >= 1.0 {
                self.sine -= 1.0;
            }
            let sine = sin_cycles(self.sine);
            // headroom into the filters (Drive pushes into their saturation)
            let mut x = (s1 * lv[0] + s2 * lv[1] + s3 * lv[2]) * 0.4 + self.kick;
            self.kick = 0.0;
            let y = if !filter_on {
                x
            } else {
                if fm2 > 0.0 {
                    self.f2.set_wc(c2_wc * (1.0 + sine * fm2 * 0.9).max(0.02), r2);
                }
                let f1 = |f: &mut Svf, x: f32| -> f32 {
                    let o = f.run(x);
                    if f1_mode == 2 { x + o * (1.0 + r1 * 6.0) } else { o }
                };
                if parallel {
                    // Drive sits before both filters; their outputs are mixed by Filter Blend
                    if drive > 0.0 {
                        x = dsp::fast_tanh(x * dg) / dg.sqrt();
                    }
                    let a = f1(&mut self.f1, x);
                    let b = self.f2.run(x);
                    a * (1.0 - blend) * 0.5 + b * (1.0 + blend) * 0.5
                } else if blend >= 0.0 {
                    // series: Filter 1 fades out of the path, one overdrive before Filter 2
                    let a = f1(&mut self.f1, x);
                    let mut inp = a * (1.0 - blend) + x * blend;
                    if drive > 0.0 {
                        inp = dsp::fast_tanh(inp * dg) / dg.sqrt();
                    }
                    self.f2.run(inp)
                } else {
                    // negative blend: Filter 2 fades out, an extra overdrive before Filter 1
                    if drive > 0.0 {
                        x = dsp::fast_tanh(x * dg) / dg.sqrt();
                    }
                    let a = f1(&mut self.f1, x);
                    let mut inp = a;
                    if drive > 0.0 {
                        inp = dsp::fast_tanh(inp * dg) / dg.sqrt();
                    }
                    let b = self.f2.run(inp);
                    b * (1.0 + blend) + a * -blend
                }
            };
            let mut g = a0 + (a1 - a0) * (i as f32 / n as f32);
            if self.killing {
                g *= self.fade.next();
            }
            let out = (y * 2.5 + sine * sine_lvl) * g;
            peak = peak.max(out.abs());
            l[i] += out * pl;
            r[i] += out * pr;
        }
        self.level = peak;
        if self.env3.stage == 0 || (self.killing && self.fade.gain <= 0.0) {
            self.on = false;
            self.amp_now = 0.0;
        }
    }
    fn active(&self) -> bool {
        self.on
    }
    fn level(&self) -> f32 {
        self.level
    }
}

fn lfo_shape(v: f32) -> Shape {
    match v.round() as i32 {
        0 => Shape::Triangle,
        1 => Shape::SawDown,
        2 => Shape::Square,
        3 => Shape::SquareUni,
        4 => Shape::SampleHold,
        _ => Shape::Lag,
    }
}

/// LFO 1 with its Lfo1Asym and Lfo1Curve targets: the rectangle's pulse width, the triangle
/// leaning toward a sawtooth, the sawtooth's zero crossing; smoothed rectangle and random
/// waves, triangle and sawtooth bent convex or concave.
fn shape_lfo(val: f32, ph: f32, wave: i32, asym: f32, curv: f32) -> f32 {
    let mut y = val;
    if asym != 0.0 {
        let a = asym.clamp(-0.95, 0.95);
        y = match wave {
            2 => if ph < 0.5 + a * 0.5 { 1.0 } else { -1.0 },
            3 => if ph < 0.5 + a * 0.5 { 1.0 } else { 0.0 },
            0 => {
                // the peak moves: up for (1+a)/2 of the cycle, down for the rest
                let up = (0.5 + a * 0.5).clamp(0.02, 0.98);
                if ph < up { -1.0 + 2.0 * ph / up } else { 1.0 - 2.0 * (ph - up) / (1.0 - up) }
            }
            1 => (y + a).clamp(-1.0, 1.0),
            _ => y,
        };
    }
    if curv != 0.0 {
        let c = curv.clamp(-1.0, 1.0);
        y = match wave {
            0 | 1 => {
                // convex above zero, concave below
                let e = 2f32.powf(-c * 2.0);
                y.signum() * y.abs().powf(e)
            }
            _ => y * (1.0 - c.abs() * 0.5) + dsp::fast_tanh(y * 2.0) * c.abs() * 0.25,
        };
    }
    y
}

pub struct Es2 {
    params: Params,
    poly: Poly<Es2Voice>,
    ch: Channel,
    sh: Sh,
    rate: f32,
    lfo2: Lfo,
    rng: Rng,
    chorus: Chorus,
    flanger: Flanger,
    phaser: Phaser,
    dist_lp: [dsp::OnePole; 2],
    stack: Stack,
    env1: Env,
    side: SideBuf,
    /// Learn: the Ctrl slot waiting for a controller, what it was set to, seconds left.
    learn: Option<(usize, f32, f32)>,
    /// The coarse halves of 14-bit controller pairs.
    msb: [u8; 32],
}

impl Es2 {
    pub fn new(rate: f32) -> Es2 {
        dsp::warm_tables();
        let _ = crate::retro::tables();
        let mut seed = 41;
        let mut s = Es2 {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 19;
                Es2Voice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: Sh { rate, p: [0.0; NP], lfo2: 0.0, env1: 0.0, side: 0.0, ctrl: [0.0; 6], bpm: 120.0, last_pitch: 0.0, mono_unison: false, bender: 0.0 },
            rate,
            lfo2: Lfo::default(),
            rng: Rng::new(5),
            chorus: Chorus::new(rate),
            flanger: Flanger::new(rate),
            phaser: Phaser::default(),
            dist_lp: [dsp::OnePole::default(); 2],
            stack: Stack::default(),
            env1: Env::default(),
            side: SideBuf::new(),
            learn: None,
            msb: [0; 32],
        };
        s.update();
        s
    }
    fn update(&mut self) {
        for i in 0..NP {
            self.sh.p[i] = self.params.get(i as u32);
        }
        let voices = self.sh.g(1).round().clamp(1.0, 32.0) as usize;
        let unison = self.sh.g(9) >= 0.5;
        self.sh.mono_unison = self.mono() && unison;
        self.poly.set_max(if self.mono() {
            if unison { voices } else { 1 }
        } else {
            voices
        });
        // the upward range can be linked to the downward one
        let up = self.sh.g(6);
        self.ch.bend_down = self.sh.g(167);
        self.ch.bend_up = if up > 36.5 { self.ch.bend_down } else { up };
        self.ch.update_bend();
        for f in &mut self.dist_lp {
            f.lowpass_hz(700.0 * 28f32.powf(self.sh.g(118)), self.rate);
        }
    }
    fn mono(&self) -> bool {
        self.sh.g(8).round() as i32 > 0
    }
    /// Starts the voices of one note: one, two (poly unison) or the whole stack (mono unison).
    fn start_note(&mut self, pitch: u8, velocity: u8) {
        let unison = self.sh.g(9) >= 0.5;
        let count = if !unison {
            1
        } else if self.mono() {
            self.sh.g(1).round().clamp(1.0, 32.0) as usize
        } else {
            2
        };
        for k in 0..count {
            let pos = if count > 1 { k as f32 / (count - 1) as f32 * 2.0 - 1.0 } else { 0.0 };
            for v in &mut self.poly.voices {
                if !v.on {
                    v.uni = pos;
                    v.upan = pos * 0.8;
                }
            }
            self.poly.stacking = k > 0;
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
        }
        self.poly.stacking = false;
        self.sh.last_pitch = pitch as f32;
    }
    /// Where the Vector Envelope has the pointers of the voice started last (mix x, mix y, pad
    /// x, pad y), for the window and for checks; None when no voice sounds.
    pub fn vector_pos(&self) -> Option<[f32; 4]> {
        self.poly.voices.iter().filter(|v| v.on && !v.killing).min_by(|a, b| a.age.total_cmp(&b.age)).map(|v| v.vec.cur)
    }
    /// The Ctrl slot waiting for a controller (Learn), if any.
    pub fn learning(&self) -> Option<usize> {
        self.learn.map(|l| l.0)
    }
    /// The controller value (0…1) reached the Ctrl slots assigned to `cc`.
    fn ctrl_in(&mut self, cc: u8, value: f32) {
        for k in 0..6 {
            if ctrl_cc(self.sh.g(169 + k as u32).round() as usize) == Some(cc) {
                self.sh.ctrl[k] = value;
            }
        }
    }
}

impl Plugin for Es2 {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if (169..175).contains(&id) && v.round() as usize == CTRL_LEARN {
            // Learn: the next controller moved becomes the assignment (20 seconds to do it)
            let before = self.params.get(id);
            if before.round() as usize != CTRL_LEARN {
                self.learn = Some(((id - 169) as usize, before, 20.0));
            }
        } else if (169..175).contains(&id) && self.learn.is_some_and(|l| l.0 == (id - 169) as usize) {
            self.learn = None;
        }
        if self.params.set(id, v) {
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.stack.clear();
        self.chorus.reset();
        self.flanger.reset();
        self.phaser.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        kit::env_time(self.sh.g(113), 10.0) + 0.2
    }
    fn value_text(&self, id: u32, v: f32) -> Option<String> {
        let ms = |t: f32| if t < 1.0 { format!("{:.0} ms", t * 1000.0) } else { format!("{t:.2} s") };
        match id {
            6 => Some(if v > 36.5 { "Link".into() } else { format!("{v:.0}") }),
            7 => Some(ms(v * v * 1.5)),
            15 | 20 | 25 => Some(if v < 0.5 { "Sine".into() } else { format!("{v:.0}") }),
            93 => Some(if v.abs() < 0.001 { "0".into() } else { format!("{} {}", if v > 0.0 { "delay" } else { "decay" }, ms(v * v * 10.0)) }),
            95 | 132 => Some(match kit::bipolar_rate(v, 30.0, self.sh.bpm) {
                (_, Some(n)) => n.to_string(),
                (hz, None) if hz > 0.0 => format!("{hz:.2} Hz"),
                _ => if id == 132 { "as set".to_string() } else { "DC".to_string() },
            }),
            98 | 99 | 100 | 101 | 102 | 103 | 106 | 108 | 109 | 110 | 113 => Some(ms(kit::env_time(v, 10.0))),
            104 | 111 => Some(if v < -0.01 { format!("fall {}", ms(0.05 * 400f32.powf(1.0 + v))) } else if v > 0.01 { format!("rise {}", ms(0.05 * 400f32.powf(1.0 - v))) } else { "hold".into() }),
            128 => Some(format!("{:.0} %", v * 100.0)),
            139 => Some(if v > 10.5 { "infinite".into() } else { format!("{v:.0}") }),
            133 | 135 | 136 | 140 => Some(format!("{:.0}", v.round() + 1.0)),
            12 | 17 | 22 => Some(format!("{v:.0} s")),
            13 | 18 | 23 => Some(format!("{v:.0} c")),
            _ => None,
        }
    }
    /// Logic's float block for ES2 isn't decoded: a project's ES2 keeps the defaults.
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for Es2 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        // ENV 1 for all voices: Retrig restarts with every key, Mono only when none was held
        let any_held = self.poly.voices.iter().any(|v| v.on && v.held);
        match self.sh.g(97).round() as i32 {
            2 => self.env1.trigger(false),
            1 if !any_held => self.env1.trigger(true),
            _ => {}
        }
        if self.mono() {
            self.stack.push(pitch, velocity);
            let sounding = self.poly.voices.iter().any(|v| v.on && !v.killing && v.held);
            if self.stack.len() > 1 && sounding {
                // played legato: no new trigger (Mono and Legato alike); the pitch moves on
                for v in self.poly.voices.iter_mut().filter(|v| v.on && !v.killing) {
                    v.legato(pitch);
                }
                self.sh.last_pitch = pitch as f32;
                return;
            }
            // Legato glides only between tied notes
            if self.sh.g(8).round() as i32 == 2 {
                self.sh.last_pitch = 0.0;
            }
            for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                v.kill(self.rate);
            }
        }
        self.start_note(pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.mono() {
            self.stack.remove(pitch);
            if let Some((top, _)) = self.stack.last() {
                for v in self.poly.voices.iter_mut().filter(|v| v.on && !v.killing) {
                    v.legato(top);
                }
                self.sh.last_pitch = top as f32;
                return;
            }
            if !self.ch.ctl.sustain {
                let sh = self.sh;
                for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                    v.release(&sh, &self.ch.ctl);
                }
            }
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
        if !self.poly.voices.iter().any(|v| v.on && v.held) && self.sh.g(96) >= 0.5 && self.env1.stage == 3 {
            self.env1.stage = 4;
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.stack.clear();
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        // Learn takes the next assignable controller
        if let Some((slot, _, _)) = self.learn
            && let Some(idx) = ctrl_index(cc)
        {
            self.learn = None;
            self.params.set(169 + slot as u32, idx as f32);
            self.update();
        }
        if cc < 32 {
            // the coarse half: works by itself at seven bits
            self.msb[cc as usize] = value;
            self.ctrl_in(cc, value as f32 / 127.0);
        } else if (32..64).contains(&cc) {
            // the fine half of a 14-bit pair (32 above its coarse half)
            let coarse = cc - 32;
            let v14 = (self.msb[coarse as usize] as u32) << 7 | value as u32;
            self.ctrl_in(coarse, v14 as f32 / 16383.0);
        } else {
            self.ctrl_in(cc, value as f32 / 127.0);
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
        self.sh.bender = (value as f32 / 8191.0).clamp(-1.0, 1.0);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.sh.bpm = bpm as f32;
    }
    fn side_chain(&mut self, l: &[f32], r: &[f32]) {
        self.side.fill(l, r);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.sh.g(40));
        let dist = self.sh.g(117);
        let hard = self.sh.g(116) >= 0.5;
        let fxm = self.sh.g(119).round() as i32;
        let fx_int = self.sh.g(121);
        let fx_rate = self.sh.g(120);
        let (lfo2_hz, _) = kit::bipolar_rate(self.sh.g(95), 100.0, self.sh.bpm);
        if let Some((slot, before, left_s)) = self.learn {
            let t = left_s - n as f32 / self.rate;
            if t <= 0.0 {
                // nothing arrived: back to the assignment it had
                self.learn = None;
                self.params.set(169 + slot as u32, before);
                self.update();
            } else {
                self.learn = Some((slot, before, t));
            }
        }
        self.sh.side = (self.side.level * 4.0).min(1.0);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.lfo2 = if lfo2_hz > 0.0 { self.lfo2.step(lfo_shape(self.sh.g(94)), lfo2_hz * m as f32 / self.rate, &mut self.rng) } else { 1.0 };
            self.sh.env1 = env1_step(&mut self.env1, m, kit::env_time(self.sh.g(99), 10.0), kit::env_time(self.sh.g(100), 10.0), self.sh.g(96) >= 0.5, self.rate);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly.render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if dist > 0.0 {
                let g = 1.0 + dist * dist * 40.0;
                let make = 1.0 / g.sqrt();
                for i in 0..m {
                    let (a, b) = (sl[i] * g, sr[i] * g);
                    let (a, b) = if hard { (a.clamp(-1.0, 1.0), b.clamp(-1.0, 1.0)) } else { (dsp::fast_tanh(a), dsp::fast_tanh(b)) };
                    sl[i] = self.dist_lp[0].lp(a) * make;
                    sr[i] = self.dist_lp[1].lp(b) * make;
                }
            }
            if fx_int > 0.0 {
                match fxm {
                    0 => self.chorus.process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int),
                    1 => self.flanger.process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int * 0.5),
                    _ => self.phaser.process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int * 0.7, 0.5, 1.0, self.rate),
                }
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
        self.side.done();
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------------------------
// Randomizing and the Vector Envelope's edits (run by the window, checked by the tests)

fn param(id: u32) -> &'static Param {
    PARAMS.iter().find(|p| p.id == id).unwrap_or(&PARAMS[0])
}

/// The parameters a RND Destination covers.
pub fn rnd_group(dest: usize, get: &dyn Fn(u32) -> f32) -> Vec<u32> {
    let waves: Vec<u32> = vec![14, 19, 24, 15, 20, 25, 190, 191, 192];
    let filters: Vec<u32> = (29..=38).collect();
    let envs: Vec<u32> = (96..=114).collect();
    let lfos: Vec<u32> = (91..=95).collect();
    let router: Vec<u32> = (41..=90).collect();
    let fx: Vec<u32> = (116..=121).collect();
    let n = get(135).round() as u32 + 1;
    let pts = |off: &[u32]| -> Vec<u32> { (0..n).flat_map(|k| off.iter().map(move |o| VEC_POINTS + k * 5 + o)).filter(|id| *id != VEC_POINTS).collect() };
    let vec_all: Vec<u32> = [124, 125, 126, 127, 128, 131, 132, 136, 137, 138, 139, 140, 168].into_iter().chain(pts(&[0, 1, 2, 3, 4])).collect();
    let rest: Vec<u32> = vec![3, 7, 13, 18, 23, 26, 27, 28, 39];
    let semis: Vec<u32> = vec![12, 17, 22];
    match dest {
        0 => [waves, filters, envs, lfos, router, fx, vec_all, rest, semis].concat(),
        1 => [waves, filters, envs, lfos, fx, vec_all, rest].concat(),
        2 => [waves, filters, envs, lfos, router, fx, rest, semis].concat(),
        3 => waves,
        4 => vec![15, 20, 25],
        5 => filters,
        6 => envs,
        7 => lfos,
        8 => router,
        9 => fx,
        10 => vec_all,
        11 => pts(&[1, 2]),
        12 => pts(&[3, 4]),
        13 => pts(&[3]),
        14 => pts(&[4]),
        15 => pts(&[0]),
        16 => [pts(&[0]), vec![135, 136, 138, 139, 140, 132, 131]].concat(),
        _ => {
            // the times inside the loop, and Loop Smooth when the loop runs forward or backward
            let (s, l) = (get(136).round() as u32, get(140).round() as u32);
            let mut v: Vec<u32> = (s.min(l) + 1..=s.max(l)).map(|k| VEC_POINTS + k * 5).collect();
            let mode = get(138).round() as i32;
            if mode == 1 || mode == 2 {
                v.push(131);
            }
            v
        }
    }
}

/// Randomize (the RND button): the destination's parameters moved from where they are by up
/// to `intensity` of their range (menus and switches change with that probability; the
/// Digiwaves destination always picks new waves). Master level, the filter button and the
/// oscillators' on / off buttons are never touched; randomizing the Vector Envelope turns
/// Solo Point off.
pub fn randomize(get: &dyn Fn(u32) -> f32, intensity: f32, dest: usize, seed: u32) -> Vec<(u32, f32)> {
    let mut rng = Rng::new(seed.wrapping_mul(2654435761).max(1));
    let mut out: Vec<(u32, f32)> = Vec::new();
    for id in rnd_group(dest, get) {
        if [40, 115, 11, 16, 21].contains(&id) {
            continue;
        }
        let p = param(id);
        let cur = get(id);
        let v = if dest == 4 {
            1.0 + (rng.unit() * 99.99).floor()
        } else if matches!(p.unit, Unit::Menu | Unit::OnOff) {
            if rng.unit() < intensity { (p.min + rng.unit() * (p.max - p.min + 0.999)).floor().min(p.max) } else { cur }
        } else {
            let v = (cur + rng.bipolar() * intensity * (p.max - p.min)).clamp(p.min, p.max);
            if [12, 17, 22, 15, 20, 25, 135, 136, 139, 140].contains(&id) { v.round() } else { v }
        };
        if v != cur {
            out.push((id, v));
        }
    }
    if matches!(dest, 0 | 1 | 10..=17) && get(134) >= 0.5 {
        out.push((134, 0.0));
    }
    // the sustain and loop points stay inside the envelope
    let n = out.iter().rev().find(|x| x.0 == 135).map(|x| x.1).unwrap_or(get(135));
    for id in [136, 140] {
        let v = out.iter().rev().find(|x| x.0 == id).map(|x| x.1).unwrap_or(get(id));
        if v > n {
            out.push((id, n));
        }
    }
    out
}

/// One Vector Envelope point as stored: time, mix x, mix y, pad x, pad y.
pub type VPoint = [f32; 5];

/// The envelope's points now.
pub fn vec_points(get: &dyn Fn(u32) -> f32) -> Vec<VPoint> {
    let n = get(135).round().clamp(2.0, 15.0) as u32 + 1;
    (0..n).map(|k| {
        let b = VEC_POINTS + k * 5;
        [get(b), get(b + 1), get(b + 2), get(b + 3), get(b + 4)]
    }).collect()
}

/// The parameter changes that store `pts` (3…16 points) with the given sustain, loop and
/// selected points.
pub fn vec_store(pts: &[VPoint], sustain: usize, loop_pt: usize, selected: usize) -> Vec<(u32, f32)> {
    let n = pts.len().clamp(3, 16);
    let mut out = vec![(135, (n - 1) as f32), (136, sustain.min(n - 1) as f32), (140, loop_pt.min(n - 1) as f32), (133, selected.min(n - 1) as f32)];
    for (k, p) in pts.iter().take(n).enumerate() {
        let b = VEC_POINTS + k as u32 * 5;
        out.push((b, if k == 0 { 0.0 } else { p[0] }));
        for j in 1..5 {
            out.push((b + j as u32, p[j]));
        }
    }
    out
}

/// The Vector Envelope's shortcut menu and clicks. `clip` is a copied point.
#[derive(Clone, Debug, PartialEq)]
pub enum VecEdit {
    /// Delete Selected Point (Control-click on a point): never below three points.
    Delete(usize),
    /// Insert Point to the Right of Selected Point: the segment after it splits in two halves.
    InsertAfter(usize),
    /// Shift-click between two points: a new point at that time (seconds from the start,
    /// before time scaling), on the line between its neighbours.
    InsertAt(f32),
    Paste(usize, VPoint),
    PasteAll(VPoint),
    AverageTime,
    /// Init to 8/8 Loop or 16/16 Loop: that many equal points.
    InitLoop(usize),
    Clear,
    /// A point's time changed; with `keep_later` (Control-drag) the next point's time makes up
    /// for it so the later points stay where they are.
    SetTime(usize, f32, bool),
    /// Fix Timing: Time Scaling goes into the times (and a synced Loop Rate becomes "as set").
    FixTiming,
}

pub fn vec_edit(get: &dyn Fn(u32) -> f32, edit: &VecEdit) -> Vec<(u32, f32)> {
    let mut pts = vec_points(get);
    let (mut s, mut l, mut sel) = (get(136).round() as usize, get(140).round() as usize, get(133).round() as usize);
    let shift_after = |i: &mut usize, at: usize, by: i32| {
        if *i >= at {
            *i = (*i as i32 + by).max(0) as usize;
        }
    };
    let mut extra: Vec<(u32, f32)> = Vec::new();
    match edit {
        VecEdit::Delete(k) => {
            let k = *k;
            if pts.len() <= 3 || k >= pts.len() {
                return Vec::new();
            }
            // the next point takes over the deleted one's time, so the later ones stay put
            if k + 1 < pts.len() && k > 0 {
                pts[k + 1][0] += pts[k][0];
            }
            pts.remove(k);
            shift_after(&mut s, k + 1, -1);
            shift_after(&mut l, k + 1, -1);
            sel = sel.min(pts.len() - 1);
        }
        VecEdit::InsertAfter(k) => {
            let k = *k;
            if pts.len() >= 16 || k >= pts.len() {
                return Vec::new();
            }
            let new = if k + 1 < pts.len() {
                let (a, b) = (pts[k], pts[k + 1]);
                pts[k + 1][0] = b[0] * 0.5;
                [b[0] * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5, (a[3] + b[3]) * 0.5, (a[4] + b[4]) * 0.5]
            } else {
                let a = pts[k];
                [0.5, a[1], a[2], a[3], a[4]]
            };
            pts.insert(k + 1, new);
            shift_after(&mut s, k + 1, 1);
            shift_after(&mut l, k + 1, 1);
            sel = k + 1;
        }
        VecEdit::InsertAt(t) => {
            if pts.len() >= 16 {
                return Vec::new();
            }
            let mut acc = 0.0;
            for k in 1..pts.len() {
                let end = acc + pts[k][0];
                if *t > acc && *t < end {
                    let u = (*t - acc) / pts[k][0];
                    let (a, b) = (pts[k - 1], pts[k]);
                    let new = [*t - acc, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u, a[3] + (b[3] - a[3]) * u, a[4] + (b[4] - a[4]) * u];
                    pts[k][0] = end - *t;
                    pts.insert(k, new);
                    shift_after(&mut s, k, 1);
                    shift_after(&mut l, k, 1);
                    sel = k;
                    break;
                }
                acc = end;
            }
        }
        VecEdit::Paste(k, p) => {
            if let Some(q) = pts.get_mut(*k) {
                for j in 1..5 {
                    q[j] = p[j];
                }
            }
        }
        VecEdit::PasteAll(p) => {
            for q in &mut pts {
                for j in 1..5 {
                    q[j] = p[j];
                }
            }
        }
        VecEdit::AverageTime => {
            let total: f32 = pts.iter().skip(1).map(|p| p[0]).sum();
            let avg = total / (pts.len() - 1) as f32;
            for q in pts.iter_mut().skip(1) {
                q[0] = avg;
            }
        }
        VecEdit::InitLoop(n) => {
            let n = (*n).clamp(3, 16);
            let first = pts[0];
            pts = (0..n).map(|k| [if k == 0 { 0.0 } else { 0.25 }, first[1], first[2], first[3], first[4]]).collect();
            // a loop over the whole envelope
            l = 0;
            s = n - 1;
            sel = 0;
            extra.push((138, 1.0));
        }
        VecEdit::Clear => {
            let first = pts[0];
            pts = (0..3).map(|k| [if k == 0 { 0.0 } else { 0.5 }, first[1], first[2], first[3], first[4]]).collect();
            s = 1;
            l = 0;
            sel = 0;
            extra.push((138, 0.0));
        }
        VecEdit::SetTime(k, t, keep_later) => {
            let k = *k;
            if k == 0 || k >= pts.len() {
                return Vec::new();
            }
            let old = pts[k][0];
            let mut t = t.clamp(0.0, 10.0);
            if *keep_later && k + 1 < pts.len() {
                t = t.min(old + pts[k + 1][0]);
                pts[k + 1][0] -= t - old;
            }
            pts[k][0] = t;
        }
        VecEdit::FixTiming => {
            let scale = get(128);
            for q in pts.iter_mut().skip(1) {
                q[0] = (q[0] * scale).min(10.0);
            }
            extra.push((128, 1.0));
            // a synced rate becomes "as set" so the absolute speed stays
            if get(132) < -0.001 {
                extra.push((132, 0.0));
            }
        }
    }
    let mut out = vec_store(&pts, s, l, sel);
    out.extend(extra);
    out
}
