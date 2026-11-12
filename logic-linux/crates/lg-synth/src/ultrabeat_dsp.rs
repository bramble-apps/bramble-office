//! Ultrabeat's drum voice: the sound of one drum sound (phase-distortion / FM / side-chain
//! oscillator 1, phase / sample / modelled-string oscillator 2, ring modulator, noise generator,
//! filter with distortion circuit, two-band EQ, pan modulation or stereo spread, four envelopes
//! and two LFOs with the mod / via routing). One `Inst` is one sounding hit; a drum sound keeps
//! a few of them so earlier hits keep decaying when a new one starts (Trigger: Multi).

use std::f32::consts::{PI, TAU};

use crate::dsp::{self, Allpass1, Biquad, Mode, OnePole, Rng, Svf, SvfMode};
use crate::synthkit as kit;
use crate::ultrabeat as u;

/// Samples per control step.
pub const CH: usize = 32;
/// Partials of the modelled string.
pub const MODES: usize = 20;

/// A sample for oscillator 2's sample mode.
pub struct Sample {
    pub data: Vec<f32>,
    pub rate: f32,
}

/// What the voices read from the instrument while rendering a step.
pub struct Gl<'a> {
    pub rate: f32,
    pub bpm: f32,
    /// Ctrl A…D (0…1).
    pub ctrl: [f32; 4],
    /// The side chain for this step (mono; at least `n` long).
    pub side: &'a [f32],
    pub sample: Option<&'a Sample>,
    /// Free-running LFO phases (for LFOs with Cycles at maximum).
    pub free: [f32; 2],
    /// Mixer: audible gain (volume, mute and solo applied) and pan −1…1.
    pub gain: f32,
}

#[derive(Clone, Copy)]
pub struct Lfo {
    phase: f32,
    count: u32,
    sh: f32,
}

#[derive(Clone, Copy)]
pub struct Inst {
    pub active: bool,
    pub age: u32,
    pub vel: f32,
    pub gain: f32,
    pub pitch_off: f32,
    pub note: i16,
    pub held: bool,
    /// Samples until the gate ends (negative: held by a key).
    pub gate_left: i64,
    /// Seconds into the hit when it was released (negative: not released).
    pub rel: f32,
    /// Fading out (choked): per-sample step, 0 when not.
    pub fade: f32,
    pub fade_gain: f32,
    pub lanes: [(u8, f32); 4],
    ph1: f32,
    ph2: f32,
    pos: f64,
    modes: [Mode; MODES],
    nmodes: usize,
    nz: Svf,
    nz_hold: f32,
    nz_cnt: u32,
    f: [Svf; 2],
    crush_hold: f32,
    crush_cnt: f32,
    dist_lp: OnePole,
    eq: [Biquad; 2],
    eq_sig: [f32; 8],
    spread_lp: OnePole,
    ap: [Allpass1; 4],
    lfo: [Lfo; 2],
    rng: Rng,
    start_pos: bool,
}

impl Default for Inst {
    fn default() -> Inst {
        Inst {
            active: false,
            age: 0,
            vel: 1.0,
            gain: 1.0,
            pitch_off: 0.0,
            note: -1,
            held: false,
            gate_left: -1,
            rel: -1.0,
            fade: 0.0,
            fade_gain: 1.0,
            lanes: [(0, 0.0); 4],
            ph1: 0.0,
            ph2: 0.0,
            pos: 0.0,
            modes: [Mode::default(); MODES],
            nmodes: 0,
            nz: Svf::default(),
            nz_hold: 0.0,
            nz_cnt: 0,
            f: [Svf::default(); 2],
            crush_hold: 0.0,
            crush_cnt: 0.0,
            dist_lp: OnePole::default(),
            eq: [Biquad::default(); 2],
            eq_sig: [f32::NAN; 8],
            spread_lp: OnePole::default(),
            ap: [Allpass1::default(); 4],
            lfo: [Lfo { phase: 0.0, count: 0, sh: 0.0 }; 2],
            rng: Rng::new(7),
            start_pos: false,
        }
    }
}

/// The parameters with a step's automation offsets added.
pub fn with_lanes(base: &[f32; 256], lanes: &[(u8, f32); 4]) -> [f32; 256] {
    let mut p = *base;
    for &(k1, d) in lanes {
        if k1 > 0 {
            let k = (k1 - 1) as usize;
            if let Some(s) = u::vspec(k as u32) {
                p[k] = (p[k] + d).clamp(s.min, s.max);
            }
        }
    }
    p
}

fn db(v: f32) -> f32 {
    if v <= -59.9 {
        0.0
    } else {
        10f32.powf(v / 20.0)
    }
}

/// Phase oscillator: a sine whose waveform is reshaped by Asym (towards a sawtooth), Slope
/// (steeper, more nasal) and Saturation (clipped, more rectangular).
pub fn phase_osc(ph: f32, slope: f32, sat: f32, asym: f32) -> f32 {
    let s = dsp::sin_cycles(ph);
    let a = asym.clamp(0.0, 1.0);
    let saw = 1.0 - 2.0 * ph;
    let mut y = s + (saw - s) * a;
    let sl = slope + 0.2 * a;
    if sl > 0.001 {
        let g = 1.0 + sl * 8.0;
        y = dsp::fast_tanh(g * y) / dsp::fast_tanh(g);
    } else if sl < -0.001 {
        y *= 1.0 + sl * 0.6;
    }
    if sat > 0.001 {
        let g = 1.0 + sat * sat * 24.0;
        y = dsp::soft_clip(y * g) / dsp::soft_clip(g);
    }
    y
}

fn lfo_pure(i: usize, ph: f32, sh: f32) -> f32 {
    match i {
        0 => {
            if ph < 0.25 {
                4.0 * ph
            } else if ph < 0.75 {
                2.0 - 4.0 * ph
            } else {
                4.0 * ph - 4.0
            }
        }
        1 => 1.0 - 2.0 * ph,
        2 => dsp::sin_cycles(ph),
        3 => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        4 => {
            if ph < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        _ => sh,
    }
}

/// Where each waveform sits on the Waveform slider: triangle, sawtooth, sine, square (near
/// 0.58), rectangle, random at the far right.
pub const LFO_ANCHORS: [f32; 6] = [0.0, 0.18, 0.38, 0.58, 0.78, 1.0];

pub fn lfo_shape(ph: f32, w: f32, sh: f32) -> f32 {
    let w = w.clamp(0.0, 1.0);
    for i in 0..5 {
        if w <= LFO_ANCHORS[i + 1] {
            let t = (w - LFO_ANCHORS[i]) / (LFO_ANCHORS[i + 1] - LFO_ANCHORS[i]);
            let a = lfo_pure(i, ph, sh);
            return if t <= 0.0 { a } else { a + (lfo_pure(i + 1, ph, sh) - a) * t };
        }
    }
    sh
}

/// An LFO's speed in Hz (synced: from the note value list).
pub fn lfo_hz(sync: bool, rate: f32, bpm: f32) -> f32 {
    if sync {
        let list = u::sync_list();
        let k = ((rate.clamp(0.0, 1.0) * (list.len() - 1) as f32).round()) as usize;
        bpm.max(1.0) / 60.0 / list[k].1
    } else {
        0.01 * 10000f32.powf(rate.clamp(0.0, 1.0))
    }
}

/// One envelope's parameters after velocity modulation.
pub struct EnvP {
    pub a: f32,
    pub ash: f32,
    pub d: f32,
    pub dsh: f32,
    pub sus: bool,
    pub hold: f32,
}

pub fn env_params(p: &[f32; 256], e: usize, vel: f32) -> EnvP {
    let b = u::ENV0 as usize + e * 8;
    let (mut a, mut ash, mut d, mut dsh) = (kit::env_time(p[b], 10.0), p[b + 1], kit::env_time(p[b + 2], 10.0), p[b + 3]);
    let amt = p[b + 7];
    let v = (vel * 2.0 - 1.0) * amt;
    match p[b + 6].round() as i32 {
        0 => a *= 2f32.powf(-v * 2.0),
        1 => ash = (ash + v).clamp(-1.0, 1.0),
        2 => d *= 2f32.powf(-v * 2.0),
        _ => dsh = (dsh + v).clamp(-1.0, 1.0),
    }
    EnvP { a: a.max(0.0005), ash, d: d.max(0.0005), dsh, sus: p[b + 4] >= 0.5, hold: p[b + 5].clamp(0.0, 1.0) }
}

/// Envelope value at `t` seconds (`rel`: seconds when released, negative if not) and whether
/// it has run out.
pub fn env_at(e: &EnvP, t: f32, rel: f32) -> (f32, bool) {
    if t < e.a {
        let x = t / e.a;
        return (x.powf(2f32.powf(2.0 * e.ash)), false);
    }
    let td = t - e.a;
    let x = if e.sus {
        let hold_t = e.hold * e.d;
        if rel < 0.0 {
            (td / e.d).min(e.hold)
        } else {
            let tr = rel - e.a;
            if tr < hold_t { td / e.d } else { (hold_t + (t - rel).max(0.0)) / e.d }
        }
    } else {
        td / e.d
    };
    if x >= 1.0 {
        return (0.0, true);
    }
    ((1.0 - x).powf(2f32.powf(2.0 * e.dsh)), false)
}

/// The modulation depth of one target (in units of its span): the mod source scaled between
/// the mod amount (via at minimum) and the via amount (via at maximum).
fn modv(p: &[f32; 256], t: usize, src: &[f32; 8], vel: f32, ctrl: &[f32; 4]) -> f32 {
    let b = u::MOD0 as usize + t * 4;
    let s = p[b].round() as usize;
    if s == 0 || s > 7 {
        return 0.0;
    }
    let amt = p[b + 1];
    let via = p[b + 2].round() as usize;
    let depth = match via {
        0 => amt,
        1 => amt + (p[b + 3] - amt) * vel,
        2..=5 => amt + (p[b + 3] - amt) * ctrl[via - 2],
        _ => amt,
    };
    src[s] * depth
}

fn lfo_out(l: &mut Lfo, p: &[f32; 256], i: usize, age: f32, n: usize, gl: &Gl) -> f32 {
    let b = u::LFO0 as usize + i * 6;
    if p[b] < 0.5 {
        return 0.0;
    }
    let hz = lfo_hz(p[b + 1] >= 0.5, p[b + 2], gl.bpm);
    let cycles = p[b + 4].round().clamp(1.0, 100.0) as u32;
    let infinite = cycles >= 100;
    let ph = if infinite { gl.free[i] } else { l.phase };
    let mut out = 0.0;
    if infinite || l.count < cycles {
        out = lfo_shape(ph, p[b + 3], l.sh);
        let ramp = p[b + 5];
        if ramp > 0.001 {
            out *= (age / (ramp * 2.0)).min(1.0);
        } else if ramp < -0.001 {
            out *= (1.0 - age / (-ramp * 2.0)).max(0.0);
        }
    }
    if !infinite {
        l.phase += hz * n as f32 / gl.rate;
        if l.phase >= 1.0 {
            l.phase = l.phase.fract();
            l.count += 1;
            l.sh = (l.count as f32 * 12.9898).sin().fract() * 2.0 - 1.0;
        }
    }
    out
}

impl Inst {
    /// Starts a hit. `p` are the voice's parameters (automation offsets applied).
    pub fn start(&mut self, p: &[f32; 256], vel: f32, gain: f32, pitch_off: f32, note: i16, gate: i64, lanes: [(u8, f32); 4], rate: f32, seed: u32) {
        *self = Inst::default();
        self.active = true;
        self.vel = vel;
        self.gain = gain;
        self.pitch_off = pitch_off;
        self.note = note;
        self.held = gate < 0;
        self.gate_left = gate;
        self.lanes = lanes;
        self.rng = Rng::new(seed);
        self.start_pos = true;
        if p[u::O2_MODE as usize].round() as i32 == 2 {
            self.strike_string(p, rate);
        }
    }

    fn strike_string(&mut self, p: &[f32; 256], rate: f32) {
        let hz = 261.626 * dsp::semis(p[u::O2_PITCH as usize] + self.pitch_off);
        let stiff = p[u::M_STIFF as usize].clamp(0.0, 1.0);
        let loss = p[u::M_LOSS as usize].clamp(0.0, 1.0);
        let res = p[u::M_RES as usize].clamp(0.0, 1.0);
        let n = (4.0 + res * (MODES as f32 - 4.0)).round() as usize;
        let b = stiff * stiff * stiff * 0.08;
        let t0 = 2.2 - 1.2 * loss;
        let lc = loss * loss * 2.0 + 0.003;
        let hammer = p[u::M_EXC as usize] >= 0.5;
        self.nmodes = 0;
        for k in 1..=n {
            let kf = k as f32;
            let f = hz * kf * (1.0 + b * kf * kf).sqrt();
            if f > rate * 0.42 {
                break;
            }
            let t60 = (t0 / (1.0 + (kf - 1.0) * (kf - 1.0) * lc)).max(0.02);
            let amp = if hammer { (-kf * 0.55).exp() * 1.3 } else { (PI * kf * 0.18).sin().abs() / kf * 1.4 };
            let m = &mut self.modes[self.nmodes];
            m.tune(f, t60, rate);
            m.strike(f, amp * 0.5, rate);
            self.nmodes += 1;
        }
    }

    /// The gate ended (key up or the step's length): envelopes in sustain mode go on; with the
    /// Gate button on the sound is cut.
    pub fn release(&mut self, gate_cut: bool, rate: f32) {
        if self.rel < 0.0 {
            self.rel = self.age as f32 / rate;
        }
        self.held = false;
        self.gate_left = -1;
        if gate_cut {
            self.choke(0.004, rate);
        }
    }

    pub fn choke(&mut self, secs: f32, rate: f32) {
        if self.active && self.fade == 0.0 {
            self.fade = 1.0 / (secs * rate).max(1.0);
        }
    }

    /// Renders `n` (≤ CH) samples, adding into `l` / `r`. Returns false when the hit is over.
    pub fn render(&mut self, base: &[f32; 256], gl: &Gl, n: usize, l: &mut [f32], r: &mut [f32]) -> bool {
        let p = with_lanes(base, &self.lanes);
        let rate = gl.rate;
        let age = self.age as f32 / rate;
        let dt = n as f32 / rate;
        // gate countdown
        if self.gate_left >= 0 {
            self.gate_left -= n as i64;
            if self.gate_left <= 0 {
                let cut = p[u::GATE as usize] >= 0.5;
                self.release(cut, rate);
            }
        }
        // envelopes at the start and end of the step
        let mut e0 = [0.0f32; 4];
        let mut e1 = [0.0f32; 4];
        let mut over = false;
        for e in 0..4 {
            let ep = env_params(&p, e, self.vel);
            e0[e] = env_at(&ep, age, self.rel).0;
            let (v1, fin) = env_at(&ep, age + dt, self.rel);
            e1[e] = v1;
            if e == 3 && fin {
                over = true;
            }
        }
        // sources: Off, Lfo1, Lfo2, Env1..4, Max
        let l1 = lfo_out(&mut self.lfo[0], &p, 0, age, n, gl);
        let l2 = lfo_out(&mut self.lfo[1], &p, 1, age, n, gl);
        let src = [0.0, l1, l2, e0[0], e0[1], e0[2], e0[3], 1.0];
        let m = |t: usize| modv(&p, t, &src, self.vel, &gl.ctrl);

        // the oscillators
        let o1_on = p[u::O1_ON as usize] >= 0.5;
        let o2_on = p[u::O2_ON as usize] >= 0.5;
        let o1_mode = p[u::O1_MODE as usize].round() as i32;
        let o2_mode = p[u::O2_MODE as usize].round() as i32;
        let hz1 = 261.626 * dsp::semis(p[u::O1_PITCH as usize] + self.pitch_off + m(u::T_O1_PITCH) * 96.0);
        let hz2 = 261.626 * dsp::semis(p[u::O2_PITCH as usize] + self.pitch_off + m(u::T_O2_PITCH) * 96.0);
        let (inc1, inc2) = ((hz1 / rate).clamp(0.0, 0.45), (hz2 / rate).clamp(0.0, 0.45));
        let v1 = db(p[u::O1_VOL as usize] + m(u::T_O1_VOL) * 60.0);
        let v2 = db(p[u::O2_VOL as usize] + m(u::T_O2_VOL) * 60.0);
        let (sl1, sa1, as1) = (
            (p[u::O1_SLOPE as usize] + m(u::T_O1_SLOPE) * 2.0).clamp(-1.0, 1.0),
            (p[u::O1_SAT as usize] + m(u::T_O1_SAT)).clamp(0.0, 1.0),
            (p[u::O1_ASYM as usize] + m(u::T_O1_ASYM)).clamp(0.0, 1.0),
        );
        let (sl2, sa2, as2) = (
            (p[u::O2_SLOPE as usize] + m(u::T_O2_SLOPE) * 2.0).clamp(-1.0, 1.0),
            (p[u::O2_SAT as usize] + m(u::T_O2_SAT)).clamp(0.0, 1.0),
            (p[u::O2_ASYM as usize] + m(u::T_O2_ASYM)).clamp(0.0, 1.0),
        );
        let fm = (p[u::O1_FM as usize] + m(u::T_O1_FM)).clamp(0.0, 1.0);
        // sample playback
        let (s_inc, s_len) = match gl.sample {
            Some(s) if o2_mode == 1 => (s.rate as f64 / rate as f64 * dsp::semis(p[u::O2_PITCH as usize] + self.pitch_off + m(u::T_O2_PITCH) * 96.0) as f64, s.data.len()),
            _ => (0.0, 0),
        };
        let reverse = p[u::S_REV as usize] >= 0.5;
        if self.start_pos {
            self.start_pos = false;
            if s_len > 0 {
                let f = p[u::S_MIN as usize] + (p[u::S_MAX as usize] - p[u::S_MIN as usize]) * self.vel;
                let off = (f.clamp(0.0, 1.0) * s_len as f32) as f64;
                self.pos = if reverse { (s_len as f64 - 1.0 - off).max(0.0) } else { off };
            }
        }
        // ring modulator, noise
        let rm_on = p[u::RM_ON as usize] >= 0.5 && o1_on && o2_on;
        let rm_v = db(p[u::RM_VOL as usize] + m(u::T_RM_VOL) * 60.0);
        let nz_on = p[u::NZ_ON as usize] >= 0.5;
        let nz_v = db(p[u::NZ_VOL as usize] + m(u::T_NZ_VOL) * 60.0);
        let nz_type = p[u::NZ_TYPE as usize].round() as i32;
        let nz_cut = (p[u::NZ_CUT as usize] + m(u::T_NZ_CUT)).clamp(0.0, 1.0);
        let nz_res = (p[u::NZ_RES as usize] + m(u::T_NZ_RES)).clamp(0.0, 1.0);
        let dirt = (p[u::NZ_DIRT as usize] + m(u::T_NZ_DIRT)).clamp(0.0, 1.0);
        if nz_on {
            let q = 0.7 + nz_res * nz_res * 60.0;
            self.nz.set(kit::cutoff_hz(nz_cut).min(rate * 0.45), q, rate);
            self.nz.mode = if nz_type == 1 { SvfMode::High } else { SvfMode::Low };
        }
        let hold_n = 1 + (dirt * 24.0) as u32;
        // flows
        let flow = [p[u::O1_FLOW as usize] >= 0.5, p[u::O2_FLOW as usize] >= 0.5, p[u::RM_FLOW as usize] >= 0.5, p[u::NZ_FLOW as usize] >= 0.5];
        // filter section
        let f_on = p[u::F_ON as usize] >= 0.5;
        let f_type = p[u::F_TYPE as usize].round() as i32;
        let f_24 = p[u::F_SLOPE as usize] >= 0.5;
        let cut = (p[u::F_CUT as usize] + m(u::T_F_CUT)).clamp(0.0, 1.0);
        let res = (p[u::F_RES as usize] + m(u::T_F_RES)).clamp(0.0, 1.0);
        let q = if f_type >= 2 { 0.6 + res * 20.0 } else { 0.55 + res * res * 28.0 };
        let q_each = if f_24 && f_type < 2 { q.sqrt() } else { q };
        if f_on {
            let hz = kit::cutoff_hz(cut).min(rate * 0.45);
            for s in &mut self.f {
                s.set(hz, q_each, rate);
                s.mode = match f_type {
                    0 => SvfMode::Low,
                    1 => SvfMode::High,
                    2 => SvfMode::Band,
                    _ => SvfMode::Notch,
                };
            }
        }
        let d_mode = p[u::D_MODE as usize].round() as i32;
        let drive = (p[u::D_DRIVE as usize] + m(u::T_D_DRIVE)).clamp(0.0, 1.0);
        let color = p[u::D_COLOR as usize].clamp(0.0, 1.0);
        let d_level = p[u::D_LEVEL as usize].clamp(0.0, 1.0);
        let dist_first = p[u::D_POS as usize] >= 0.5;
        self.dist_lp.lowpass_hz(500.0 * 36f32.powf(color), rate);
        let bits = 16.0 - drive * 13.5;
        let q_steps = 2f32.powf(bits - 1.0);
        let crush_n = 1.0 + drive * drive * 11.0;
        // EQ
        let mut sig = [0.0f32; 8];
        for b in 0..2 {
            let o = u::EQ1_ON as usize + b * 5;
            sig[b * 4] = p[o];
            sig[b * 4 + 1] = p[o + 2];
            sig[b * 4 + 2] = p[o + 3];
            sig[b * 4 + 3] = p[o + 4] + p[o + 1] * 1000.0;
        }
        if sig != self.eq_sig {
            self.eq_sig = sig;
            for b in 0..2 {
                let o = u::EQ1_ON as usize + b * 5;
                let (hz, q, g) = (p[o + 2].clamp(20.0, rate * 0.45), p[o + 3].clamp(0.1, 12.0), p[o + 4]);
                if p[o + 1] < 0.5 {
                    if b == 0 {
                        self.eq[b].low_shelf(hz, g, rate);
                    } else {
                        self.eq[b].high_shelf(hz, g, rate);
                    }
                } else {
                    self.eq[b].peaking(hz, q, g, rate);
                }
            }
        }
        let eq_on = [p[u::EQ1_ON as usize] >= 0.5 && p[u::EQ1_GAIN as usize].abs() > 0.01, p[u::EQ2_ON as usize] >= 0.5 && p[u::EQ2_GAIN as usize].abs() > 0.01];
        // pan and spread
        let pan_mode = p[u::PAN_MODE as usize].round() as i32;
        let mut pan = p[u::MIX_PAN as usize];
        if pan_mode == 1 {
            pan += m(u::T_PAN);
        }
        let pan = pan.clamp(-1.0, 1.0);
        let (wlo, whi) = (p[u::SPREAD_LO as usize].clamp(0.0, 1.0), p[u::SPREAD_HI as usize].clamp(0.0, 1.0));
        self.spread_lp.lowpass_hz(350.0, rate);
        let ang = (pan * 0.5 + 0.5) * PI * 0.5;
        let (gl_l, gl_r) = (ang.cos() * std::f32::consts::SQRT_2, ang.sin() * std::f32::consts::SQRT_2);
        let vvol = db(p[u::VOICE_VOL as usize] + {
            let b = u::MOD0 as usize + u::T_VOICE_VOL * 4;
            let via = p[b + 2].round() as usize;
            match via {
                1 => (self.vel * 2.0 - 1.0) * p[b + 3] * 24.0,
                2..=5 => (gl.ctrl[via - 2] * 2.0 - 1.0) * p[b + 3] * 24.0,
                _ => 0.0,
            }
        });
        let vel_gain = 0.35 + 0.65 * self.vel;
        let level = vvol * vel_gain * self.gain * gl.gain * 0.5;

        let step_inv = 1.0 / n as f32;
        for i in 0..n {
            let ft = i as f32 * step_inv;
            // oscillator 2 first (it modulates oscillator 1's frequency in FM mode)
            let mut o2 = 0.0;
            if o2_on {
                match o2_mode {
                    0 => {
                        o2 = phase_osc(self.ph2, sl2, sa2, as2);
                        self.ph2 += inc2;
                        if self.ph2 >= 1.0 {
                            self.ph2 -= 1.0;
                        }
                    }
                    1 => {
                        if let Some(s) = gl.sample {
                            let len = s.data.len();
                            if len > 1 && self.pos >= 0.0 && self.pos < (len - 1) as f64 {
                                let i0 = self.pos as usize;
                                let fr = (self.pos - i0 as f64) as f32;
                                o2 = s.data[i0] + (s.data[i0 + 1] - s.data[i0]) * fr;
                            }
                            self.pos += if reverse { -s_inc } else { s_inc };
                        }
                    }
                    _ => {
                        let mut acc = 0.0;
                        for md in &mut self.modes[..self.nmodes] {
                            acc += md.next();
                        }
                        o2 = acc * 0.45;
                    }
                }
            }
            let mut o1 = 0.0;
            if o1_on {
                match o1_mode {
                    0 => {
                        o1 = phase_osc(self.ph1, sl1, sa1, as1);
                        self.ph1 += inc1;
                    }
                    1 => {
                        o1 = dsp::sin_cycles(self.ph1);
                        self.ph1 += inc1 * (1.0 + fm * 10.0 * o2).max(0.0);
                    }
                    _ => {
                        o1 = gl.side.get(i).copied().unwrap_or(0.0);
                    }
                }
                if self.ph1 >= 1.0 {
                    self.ph1 -= self.ph1.floor();
                }
            } else if o1_mode == 1 {
                // keep the carrier's phase moving so FM works from the first sample on
            }
            // the noise generator
            let mut nz = 0.0;
            if nz_on {
                if self.nz_cnt == 0 {
                    self.nz_hold = self.rng.bipolar();
                    self.nz_cnt = hold_n;
                }
                self.nz_cnt -= 1;
                let white = self.rng.bipolar();
                let raw = white + (self.nz_hold - white) * dirt;
                let raw = if dirt > 0.0 { raw + (raw.signum() * raw.abs().sqrt() - raw) * dirt * 0.3 } else { raw };
                nz = match nz_type {
                    3 => raw,
                    2 => {
                        self.nz.mode = SvfMode::Band;
                        self.nz.run_bp_norm(raw)
                    }
                    _ => self.nz.run(raw),
                };
            }
            let rm = if rm_on { o1 * o2 } else { 0.0 };
            let src_o1 = o1 * v1;
            let src_o2 = o2 * v2;
            let src_rm = rm * rm_v;
            let src_nz = nz * nz_v;
            let mut to_f = 0.0;
            let mut direct = 0.0;
            for (k, s) in [src_o1, src_o2, src_rm, src_nz].into_iter().enumerate() {
                if flow[k] {
                    to_f += s;
                } else {
                    direct += s;
                }
            }
            // filter and distortion
            let mut x = to_f;
            if f_on {
                let dist = |x: f32, s: &mut Inst| -> f32 {
                    match d_mode {
                        1 => {
                            // bit crusher: Level is the input threshold
                            let thr = d_level * 0.5;
                            let g = if x.abs() <= thr { 0.0 } else { x - thr * x.signum() };
                            s.crush_cnt -= 1.0;
                            if s.crush_cnt <= 0.0 {
                                s.crush_cnt += crush_n;
                                s.crush_hold = (g * q_steps).round() / q_steps;
                            }
                            s.crush_hold
                        }
                        2 => {
                            let g = 1.0 + drive * 40.0;
                            let y = dsp::fast_tanh(x * g) / dsp::fast_tanh(g.min(3.0)).max(0.5);
                            let y = s.dist_lp.lp(y);
                            y * d_level * 2.0
                        }
                        _ => x,
                    }
                };
                if dist_first && d_mode > 0 {
                    x = dist(x, self);
                }
                x = self.f[0].run(x);
                if f_24 {
                    x = self.f[1].run(x);
                }
                if !dist_first && d_mode > 0 {
                    x = dist(x, self);
                }
            }
            let mut y = x + direct;
            if eq_on[0] {
                y = self.eq[0].run(y);
            }
            if eq_on[1] {
                y = self.eq[1].run(y);
            }
            // voice volume: Env 4 is wired to it
            let env4 = e0[3] + (e1[3] - e0[3]) * ft;
            let fade = if self.fade > 0.0 {
                self.fade_gain -= self.fade;
                if self.fade_gain <= 0.0 {
                    self.fade_gain = 0.0;
                    over = true;
                }
                self.fade_gain
            } else {
                1.0
            };
            y *= env4 * level * fade;
            if pan_mode == 2 {
                let lo = self.spread_lp.lp(y);
                let hi = y - lo;
                let a_l = self.ap[0].run(lo, 0.7);
                let a_r = self.ap[1].run(lo, -0.7);
                let lo_l = lo + (a_l - lo) * wlo;
                let lo_r = lo + (a_r - lo) * wlo;
                let h_l = self.ap[2].run(hi, 0.35);
                let h_r = self.ap[3].run(hi, -0.35);
                let hi_l = hi + (h_l - hi) * whi;
                let hi_r = hi + (h_r - hi) * whi;
                l[i] += (lo_l + hi_l) * gl_l;
                r[i] += (lo_r + hi_r) * gl_r;
            } else {
                l[i] += y * gl_l;
                r[i] += y * gl_r;
            }
        }
        self.age += n as u32;
        if self.ph2 > 1.0e6 {
            self.ph2 = 0.0;
        }
        if over {
            self.active = false;
        }
        self.active
    }
}

#[allow(dead_code)]
const _TAU: f32 = TAU;
