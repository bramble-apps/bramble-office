//! Remix FX: the DJ-style multi-effect (Logic's Multi Effects ▸ Remix FX), as the guide
//! describes it.
//!
//! - Two XY pads, each set to one of Filter, Wobble, Orbit, Repeater, Reverb or Delay (the pad's
//!   pop-up). Touching a pad switches its effect on with the pad's X and Y; letting go switches
//!   it off unless the pad's FX Lock is on. With a lock on, choosing another effect for the same
//!   pad keeps the locked one running, so effects combine. Reset halts every XY effect (locks
//!   stay on).
//!   - Filter: X cutoff (left of centre a low-pass closing, right a high-pass opening), Y
//!     resonance; Mode Phat (24 dB low- and high-pass, warm) or Classic (12 dB state-variable).
//!   - Wobble: a filter swept by a synced LFO; X the rate (a note division), Y the depth; Time
//!     straight / dotted / triplet.
//!   - Orbit: phaser, phaser + flanger, or flanger; X rate, Y depth.
//!   - Repeater: stutters the slice that was playing when touched; X the slice length (a note
//!     division), Y the mix; Time straight / dotted / triplet.
//!   - Reverb: X the reverb time, Y the mix; Color Dark / Medium / Bright.
//!   - Delay: X the delay time (a note division), Y the feedback; Time straight / dotted /
//!     triplet. Echoes ring out after the pad is released.
//! - Gater: a slider (depth) gating the signal on the beat grid (Gater division), with Noise
//!   that keeps the gating audible in silence.
//! - Downsampler: a slider lowering the sample rate; Extreme adds a ringing resonance at the
//!   new rate, Classic just roughens.
//! - Reverse, Scratch and Tape Stop: held buttons. Reverse plays the incoming audio backwards in
//!   slices of its Time; Scratch rocks the audio back and forth like a record; Tape Stop slows
//!   it to a halt over its Time. Where on the button it was pressed (Variation, -1 left .. +1
//!   right) shortens or lengthens the effect, as Logic's touch-sensitive sides do.
//!
//! The parameter block layout of Logic's Remix FX isn't decoded, so ids here are our own; a
//! project's Remix FX loads with its defaults. Synced effects follow the song position.

use crate::kit::{Clock, DIV_BEATS, Fdn, Svf, VARIANTS, beat_samples, div_beats};
use crate::util::{Delay, Noise, Phase, Smooth, Values, flush, plugin_impl, sine01};
use lg_dsp::{Effect, Param, Transport, Unit};

pub const XY_EFFECTS: &[&str] = &["Filter", "Wobble", "Orbit", "Repeater", "Reverb", "Delay"];
const FILTER_MODES: &[&str] = &["Phat", "Classic"];
const ORBIT_MODES: &[&str] = &["Phaser", "Phaser + Flanger", "Flanger"];
const COLORS: &[&str] = &["Dark", "Medium", "Bright"];
const DS_MODES: &[&str] = &["Extreme", "Classic"];
const GATE_DIVS: &[&str] = &["1/4", "1/8", "1/16", "1/32", "1/4 T", "1/8 T", "1/16 T", "1/8 .", "1/16 ."];
const GATE_BEATS: [f32; 9] = [1.0, 0.5, 0.25, 0.125, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0, 0.75, 0.375];
const BUTTON_DIVS: &[&str] = &["1/1", "1/2", "1/4", "1/8", "1/16"];

pub const LEFT_FX: u32 = 0;
pub const RIGHT_FX: u32 = 1;
pub const LEFT_LOCK: u32 = 2;
pub const RIGHT_LOCK: u32 = 3;
/// Effect `e`'s on switch, X and Y: ids 10 + 4e, 11 + 4e, 12 + 4e.
pub const fn fx_on(e: usize) -> u32 {
    10 + 4 * e as u32
}
pub const fn fx_x(e: usize) -> u32 {
    11 + 4 * e as u32
}
pub const fn fx_y(e: usize) -> u32 {
    12 + 4 * e as u32
}
pub const FILTER_MODE: u32 = 40;
pub const WOBBLE_TIME: u32 = 41;
pub const ORBIT_MODE: u32 = 42;
pub const REPEATER_TIME: u32 = 43;
pub const REVERB_COLOR: u32 = 44;
pub const DELAY_TIME: u32 = 45;
pub const GATER: u32 = 50;
pub const GATER_TIME: u32 = 51;
pub const NOISE: u32 = 52;
pub const DOWNSAMPLER: u32 = 53;
pub const DS_MODE: u32 = 54;
pub const REVERSE: u32 = 55;
pub const REVERSE_TIME: u32 = 56;
pub const SCRATCH: u32 = 57;
pub const SCRATCH_TIME: u32 = 58;
pub const TAPE_STOP: u32 = 59;
pub const TAPE_STOP_TIME: u32 = 60;
pub const REVERSE_VAR: u32 = 61;
pub const SCRATCH_VAR: u32 = 62;
pub const TAPE_STOP_VAR: u32 = 63;
pub const RESET: u32 = 64;

macro_rules! xy {
    ($e:expr, $name:literal) => {
        [
            Param::new(fx_on($e), concat!($name, " On"), 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(fx_x($e), concat!($name, " X"), 0.0, 1.0, 0.5, Unit::None),
            Param::new(fx_y($e), concat!($name, " Y"), 0.0, 1.0, 0.5, Unit::None),
        ]
    };
}

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::menu(LEFT_FX, "Left Pad", XY_EFFECTS, 0.0),
        Param::menu(RIGHT_FX, "Right Pad", XY_EFFECTS, 3.0),
        Param::new(LEFT_LOCK, "Left FX Lock", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(RIGHT_LOCK, "Right FX Lock", 0.0, 1.0, 0.0, Unit::OnOff),
    ];
    v.extend(xy!(0, "Filter"));
    v.extend(xy!(1, "Wobble"));
    v.extend(xy!(2, "Orbit"));
    v.extend(xy!(3, "Repeater"));
    v.extend(xy!(4, "Reverb"));
    v.extend(xy!(5, "Delay"));
    v.extend([
        Param::menu(FILTER_MODE, "Filter Mode", FILTER_MODES, 0.0),
        Param::menu(WOBBLE_TIME, "Wobble Time", VARIANTS, 0.0),
        Param::menu(ORBIT_MODE, "Orbit Mode", ORBIT_MODES, 0.0),
        Param::menu(REPEATER_TIME, "Repeater Time", VARIANTS, 0.0),
        Param::menu(REVERB_COLOR, "Reverb Color", COLORS, 1.0),
        Param::menu(DELAY_TIME, "Delay Time", VARIANTS, 0.0),
        Param::new(GATER, "Gater", 0.0, 1.0, 0.0, Unit::None),
        Param::menu(GATER_TIME, "Gater Time", GATE_DIVS, 2.0),
        Param::new(NOISE, "Gater Noise", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(DOWNSAMPLER, "Downsampler", 0.0, 1.0, 0.0, Unit::None),
        Param::menu(DS_MODE, "Downsampler Mode", DS_MODES, 1.0),
        Param::new(REVERSE, "Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(REVERSE_TIME, "Reverse Time", BUTTON_DIVS, 2.0),
        Param::new(SCRATCH, "Scratch", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(SCRATCH_TIME, "Scratch Time", BUTTON_DIVS, 3.0),
        Param::new(TAPE_STOP, "Tape Stop", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(TAPE_STOP_TIME, "Tape Stop Time", BUTTON_DIVS, 1.0),
        Param::new(REVERSE_VAR, "Reverse Variation", -1.0, 1.0, 0.0, Unit::None),
        Param::new(SCRATCH_VAR, "Scratch Variation", -1.0, 1.0, 0.0, Unit::None),
        Param::new(TAPE_STOP_VAR, "Tape Stop Variation", -1.0, 1.0, 0.0, Unit::None),
        Param::new(RESET, "Reset", 0.0, 1.0, 0.0, Unit::OnOff),
    ]);
    v
});

/// The X division choices of Wobble, Repeater and Delay (index into DIV_BEATS).
fn x_div(x: f32, lo: usize, hi: usize) -> usize {
    lo + ((x.clamp(0.0, 1.0) * (hi - lo + 1) as f32) as usize).min(hi - lo)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hold {
    Off,
    Reverse { start: f64 },
    Scratch { start: f64 },
    TapeStop { start: f64, lag: f64, speed: f64 },
}

pub struct RemixFx {
    p: Values,
    rate: f32,
    clock: Clock,
    // tape for the held buttons and the Repeater
    tape: [Delay; 2],
    written: f64,
    hold: Hold,
    hold_mix: Smooth,
    hold_out: [f32; 2],
    // repeater: the slice captured at the touch
    rep_on: bool,
    rep_start: f64,
    rep_len: f64,
    rep_mix: Smooth,
    // XY smoothing
    xs: [Smooth; 6],
    ys: [Smooth; 6],
    on: [Smooth; 6],
    // filter
    f_lo: [[Svf; 2]; 2],
    f_hi: [[Svf; 2]; 2],
    wob: [Svf; 2],
    // orbit
    ap: [[f32; 6]; 2],
    ap_ph: Phase,
    fl: [Delay; 2],
    fl_fb: [f32; 2],
    // delay
    dly: [Delay; 2],
    dly_send: Smooth,
    // reverb
    verb: Fdn,
    verb_send: Smooth,
    color: [Svf; 2],
    // gater / downsampler
    gate: Smooth,
    noise: Noise,
    ds_hold: [f32; 2],
    ds_ph: f32,
    ring: [Svf; 2],
}

const TAPE_SECS: f32 = 12.0;

impl RemixFx {
    pub fn new(rate: f32) -> RemixFx {
        let sm = |v| Smooth::new(rate, 12.0, v);
        let mut r = RemixFx {
            p: Values::new(PARAMS.as_slice()),
            rate,
            clock: Clock::default(),
            tape: [Delay::new((TAPE_SECS * rate) as usize), Delay::new((TAPE_SECS * rate) as usize)],
            written: 0.0,
            hold: Hold::Off,
            hold_mix: Smooth::new(rate, 3.0, 0.0),
            hold_out: [0.0; 2],
            rep_on: false,
            rep_start: 0.0,
            rep_len: 1.0,
            rep_mix: Smooth::new(rate, 3.0, 0.0),
            xs: [sm(0.5); 6],
            ys: [sm(0.5); 6],
            on: [Smooth::new(rate, 5.0, 0.0); 6],
            f_lo: [[Svf::default(); 2]; 2],
            f_hi: [[Svf::default(); 2]; 2],
            wob: [Svf::default(); 2],
            ap: [[0.0; 6]; 2],
            ap_ph: Phase::default(),
            fl: [Delay::new((0.02 * rate) as usize), Delay::new((0.02 * rate) as usize)],
            fl_fb: [0.0; 2],
            dly: [Delay::new((4.5 * rate) as usize), Delay::new((4.5 * rate) as usize)],
            dly_send: Smooth::new(rate, 5.0, 0.0),
            verb: Fdn::new(rate),
            verb_send: Smooth::new(rate, 5.0, 0.0),
            color: [Svf::default(); 2],
            gate: Smooth::new(rate, 1.5, 1.0),
            noise: Noise(0x2545_f491),
            ds_hold: [0.0; 2],
            ds_ph: 0.0,
            ring: [Svf::default(); 2],
        };
        r.update();
        r
    }

    /// The effect a pad controls now.
    pub fn pad_effect(&self, right: bool) -> usize {
        self.p.idx(if right { RIGHT_FX } else { LEFT_FX })
    }

    fn update(&mut self) {
        if self.p.on(RESET) {
            for e in 0..6 {
                self.p.set(fx_on(e), 0.0);
            }
            self.p.set(RESET, 0.0);
        }
        for e in 0..6 {
            self.xs[e].target = self.p.get(fx_x(e));
            self.ys[e].target = self.p.get(fx_y(e));
            self.on[e].target = if self.p.on(fx_on(e)) { 1.0 } else { 0.0 };
        }
        let (fd, rate) = (self.p.idx(REVERB_COLOR), self.rate);
        for c in self.color.iter_mut() {
            c.set(rate, [3500.0, 9000.0, 300.0][fd], 0.6);
        }
    }

    fn clear(&mut self) {
        for d in self.tape.iter_mut().chain(self.fl.iter_mut()).chain(self.dly.iter_mut()) {
            d.clear();
        }
        self.verb.clear();
        self.hold = Hold::Off;
        self.rep_on = false;
        self.ap = [[0.0; 6]; 2];
        self.fl_fb = [0.0; 2];
    }

    fn tail_secs(&self) -> f32 {
        4.0
    }

    fn button_len(&self, time: u32, var: u32) -> f64 {
        let beats = [4.0, 2.0, 1.0, 0.5, 0.25][self.p.idx(time).min(4)] as f64;
        // left side of the button: half as long, right side: twice
        let v = self.p.get(var) as f64;
        beats * 2f64.powf(v) * beat_samples(self.rate, self.clock.bpm)
    }

    /// The tape sample `back` samples before the latest written one (fractional).
    #[inline]
    fn tape_at(&self, s: usize, back: f64) -> f32 {
        self.tape[s].read_lin(back.clamp(0.0, (TAPE_SECS * self.rate) as f64 - 4.0) as f32)
    }

    fn held_buttons(&mut self, l: &mut f32, r: &mut f32) {
        let now = self.written;
        // which button is held (Tape Stop over Scratch over Reverse)
        let want = if self.p.on(TAPE_STOP) {
            1
        } else if self.p.on(SCRATCH) {
            2
        } else if self.p.on(REVERSE) {
            3
        } else {
            0
        };
        let kind = match self.hold {
            Hold::Off => 0,
            Hold::TapeStop { .. } => 1,
            Hold::Scratch { .. } => 2,
            Hold::Reverse { .. } => 3,
        };
        if want != kind && want != 0 {
            self.hold = match want {
                1 => Hold::TapeStop { start: now, lag: 0.0, speed: 1.0 },
                2 => Hold::Scratch { start: now },
                _ => Hold::Reverse { start: now },
            };
        }
        self.hold_mix.target = if want != 0 { 1.0 } else { 0.0 };
        let m = self.hold_mix.next();
        if want == 0 && m < 1e-4 {
            self.hold = Hold::Off;
        }
        let (ts_len, sc_len, rv_len) = (self.button_len(TAPE_STOP_TIME, TAPE_STOP_VAR), self.button_len(SCRATCH_TIME, SCRATCH_VAR), self.button_len(REVERSE_TIME, REVERSE_VAR));
        let back = match &mut self.hold {
            Hold::Off => return,
            Hold::TapeStop { start, lag, speed } => {
                let len = ts_len;
                let t = ((now - *start) / len.max(1.0)).min(1.0);
                // the platter slows with a curve (faster at the end, like a motor letting go)
                *speed = (1.0 - t).powf(1.6);
                *lag += 1.0 - *speed;
                *lag
            }
            Hold::Scratch { start } => {
                let p = sc_len;
                let ph = (now - *start) / p.max(1.0);
                // position swings back and forth: read speed between -1 and 3
                (p / std::f64::consts::PI) * (1.0 - (std::f64::consts::TAU * ph).cos())
            }
            Hold::Reverse { start } => {
                let len = rv_len.min((TAPE_SECS * 48000.0) as f64 * 0.45);
                let mut el = now - *start;
                if el >= len {
                    *start += len;
                    el -= len;
                }
                // each slice plays the slice before it backwards
                2.0 * el
            }
        };
        let speed_gain = match self.hold {
            Hold::TapeStop { speed, .. } => (speed * 4.0).min(1.0) as f32,
            _ => 1.0,
        };
        let a = self.tape_at(0, back) * speed_gain;
        let b = self.tape_at(1, back) * speed_gain;
        self.hold_out = [a, b];
        *l = *l * (1.0 - m) + a * m;
        *r = *r * (1.0 - m) + b * m;
    }

    fn repeater(&mut self, l: &mut f32, r: &mut f32, active: bool, x: f32, y: f32) {
        let now = self.written;
        if active && !self.rep_on {
            let div = x_div(x, 2, 6);
            let beats = div_beats(div, self.p.idx(REPEATER_TIME)) as f64;
            self.rep_len = (beats * beat_samples(self.rate, self.clock.bpm)).max(32.0);
            self.rep_start = now - self.rep_len;
            self.rep_on = true;
        } else if active {
            // X moved: a new length from the same start
            let div = x_div(x, 2, 6);
            let beats = div_beats(div, self.p.idx(REPEATER_TIME)) as f64;
            self.rep_len = (beats * beat_samples(self.rate, self.clock.bpm)).max(32.0);
        } else if self.rep_mix.cur < 1e-4 {
            self.rep_on = false;
        }
        self.rep_mix.target = if active { y } else { 0.0 };
        let m = self.rep_mix.next();
        if !self.rep_on {
            return;
        }
        let el = (now - self.rep_start).rem_euclid(self.rep_len);
        let pos = self.rep_start + el;
        let back = now - pos;
        // short fades at the slice edges
        let edge = (el.min(self.rep_len - el) / (0.002 * self.rate as f64)).min(1.0) as f32;
        let a = self.tape_at(0, back) * edge;
        let b = self.tape_at(1, back) * edge;
        *l = *l * (1.0 - m) + a * m;
        *r = *r * (1.0 - m) + b * m;
    }
}

impl Effect for RemixFx {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let spb = beat_samples(rate, self.clock.bpm);
        let gater = self.p.get(GATER);
        let gate_beats = GATE_BEATS[self.p.idx(GATER_TIME).min(GATE_BEATS.len() - 1)] as f64;
        let noise_on = self.p.on(NOISE);
        let ds = self.p.get(DOWNSAMPLER);
        let ds_extreme = self.p.idx(DS_MODE) == 0;
        let ds_hz = rate * 0.5 * (0.012f32).powf(ds);
        if ds_extreme {
            for s in 0..2 {
                self.ring[s].set(rate, (ds_hz * 0.5).max(40.0), 6.0);
            }
        }
        let filter_phat = self.p.idx(FILTER_MODE) == 0;
        let orbit_mode = self.p.idx(ORBIT_MODE);
        for i in 0..left.len().min(right.len()) {
            let beat = self.clock.tick();
            let (mut l, mut r) = (left[i], right[i]);
            self.tape[0].push(l);
            self.tape[1].push(r);
            self.written += 1.0;
            self.held_buttons(&mut l, &mut r);
            // Downsampler
            if ds > 0.001 {
                self.ds_ph += ds_hz / rate;
                if self.ds_ph >= 1.0 {
                    self.ds_ph -= 1.0;
                    // a little bit reduction with the rate
                    let q = 2f32.powf(16.0 - 10.0 * ds);
                    self.ds_hold = [(l * q).round() / q, (r * q).round() / q];
                }
                let (mut a, mut b) = (self.ds_hold[0], self.ds_hold[1]);
                if ds_extreme {
                    a += self.ring[0].run(a).band * 1.5 * ds;
                    b += self.ring[1].run(b).band * 1.5 * ds;
                }
                let w = (ds * 4.0).min(1.0);
                l = l * (1.0 - w) + a * w;
                r = r * (1.0 - w) + b * w;
            }
            // Gater: open for the first half of each division
            if gater > 0.001 {
                if noise_on {
                    let n = self.noise.next() * 0.02 * gater;
                    l += n;
                    r += n;
                }
                let ph = (beat / gate_beats).rem_euclid(1.0);
                self.gate.target = if ph < 0.5 { 1.0 } else { 1.0 - gater };
            } else {
                self.gate.target = 1.0;
            }
            let g = self.gate.next();
            l *= g;
            r *= g;
            // the XY effects
            let mut xs = [0f32; 6];
            let mut ys = [0f32; 6];
            let mut on = [0f32; 6];
            for e in 0..6 {
                xs[e] = self.xs[e].next();
                ys[e] = self.ys[e].next();
                on[e] = self.on[e].next();
            }
            // Filter
            if on[0] > 1e-4 {
                let (x, q) = (xs[0], 0.6 + ys[0] * ys[0] * 9.0);
                let lp = if x < 0.5 { 60.0 * (20000.0f32 / 60.0).powf(x * 2.0) } else { 20000.0 };
                let hp = if x > 0.5 { 20.0 * (12000.0f32 / 20.0).powf((x - 0.5) * 2.0) } else { 10.0 };
                for (s, v) in [&mut l, &mut r].into_iter().enumerate() {
                    let dry = *v;
                    let mut y = dry;
                    if filter_phat {
                        for k in 0..2 {
                            self.f_lo[s][k].set(rate, lp, if k == 0 { q } else { 0.7 });
                            self.f_hi[s][k].set(rate, hp, if k == 0 { q } else { 0.7 });
                        }
                        y = (y * 1.2).tanh() / 1.2;
                        y = self.f_lo[s][0].run(y).low;
                        y = self.f_lo[s][1].run(y).low;
                        y = self.f_hi[s][0].run(y).high;
                        y = self.f_hi[s][1].run(y).high;
                    } else {
                        self.f_lo[s][0].set(rate, lp, q);
                        self.f_hi[s][0].set(rate, hp, q);
                        y = self.f_lo[s][0].run(y).low;
                        y = self.f_hi[s][0].run(y).high;
                    }
                    *v = dry + (y - dry) * on[0];
                }
            }
            // Wobble
            if on[1] > 1e-4 {
                let div = x_div(xs[1], 0, 5);
                let beats = div_beats(div, self.p.idx(WOBBLE_TIME));
                let ph = ((beat / beats as f64).rem_euclid(1.0)) as f32;
                let lfo = 0.5 - 0.5 * (crate::util::TAU * ph).cos();
                let cut = 6000.0 * (0.03f32).powf(ys[1] * lfo);
                for (s, v) in [&mut l, &mut r].into_iter().enumerate() {
                    self.wob[s].set(rate, cut, 2.5);
                    let y = self.wob[s].run(*v).low;
                    *v += (y - *v) * on[1];
                }
            }
            // Orbit
            if on[2] > 1e-4 {
                let hz = 0.05 * (160.0f32).powf(xs[2]);
                let ph = self.ap_ph.step(rate, hz);
                for (s, v) in [&mut l, &mut r].into_iter().enumerate() {
                    let m = 0.5 + 0.5 * sine01(ph + 0.25 * s as f32);
                    let dry = *v;
                    let mut wet = 0.0;
                    if orbit_mode <= 1 {
                        let f = 200.0 * (30.0f32).powf(m * ys[2]);
                        let a = ((crate::util::PI * f / rate).tan() - 1.0) / ((crate::util::PI * f / rate).tan() + 1.0);
                        let mut x = dry + self.fl_fb[s] * 0.0;
                        for st in self.ap[s].iter_mut() {
                            let y = a * x + *st;
                            *st = flush(x - a * y);
                            x = y;
                        }
                        wet += x;
                    }
                    if orbit_mode >= 1 {
                        let d = (0.0008 + 0.005 * m * ys[2]) * rate;
                        self.fl[s].push(dry + self.fl_fb[s] * 0.6);
                        let y = self.fl[s].read_lin(d);
                        self.fl_fb[s] = flush(y);
                        wet += y;
                    }
                    if orbit_mode == 1 {
                        wet *= 0.5;
                    }
                    let mixed = 0.5 * (dry + wet);
                    *v = dry + (mixed - dry) * on[2];
                }
            }
            // Repeater
            let rep_active = self.p.on(fx_on(3));
            self.repeater(&mut l, &mut r, rep_active, xs[3], ys[3]);
            // Delay (echoes ring out after release)
            {
                self.dly_send.target = if self.p.on(fx_on(5)) { 1.0 } else { 0.0 };
                let send = self.dly_send.next();
                let div = x_div(xs[5], 2, 5);
                let d = (div_beats(div, self.p.idx(DELAY_TIME)) as f64 * spb).clamp(1.0, 4.4 * rate as f64) as f32;
                let fb = ys[5] * 0.92;
                let (a, b) = (self.dly[0].read(d), self.dly[1].read(d));
                // ping-pong
                self.dly[0].push(l * send + b * fb);
                self.dly[1].push(r * send + a * fb);
                l += a * 0.6;
                r += b * 0.6;
            }
            // Reverb (the tail rings out after release)
            {
                self.verb_send.target = if self.p.on(fx_on(4)) { ys[4] } else { 0.0 };
                let send = self.verb_send.next();
                if send > 1e-5 || self.verb_send.cur > 0.0 || i % 64 == 0 {
                    self.verb.set(0.4 + 7.6 * xs[4] * xs[4], 1.0, 9000.0);
                }
                let (a, b) = self.verb.run(l * send, r * send);
                let col = self.p.idx(REVERB_COLOR);
                let (a, b) = match col {
                    0 => (self.color[0].run(a).low, self.color[1].run(b).low),
                    2 => (self.color[0].run(a).high, self.color[1].run(b).high),
                    _ => (a, b),
                };
                l = l * (1.0 - 0.5 * send) + a * 0.5;
                r = r * (1.0 - 0.5 * send) + b * 0.5;
            }
            left[i] = l;
            right[i] = r;
        }
        let _ = DIV_BEATS;
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.clock.set_tempo(bpm, self.rate);
    }
    fn set_transport(&mut self, t: Transport) {
        self.clock.sync(t, self.rate);
    }
}

plugin_impl!(RemixFx);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    fn sine(n: usize, hz: f32) -> Vec<f32> {
        (0..n).map(|i| (crate::util::TAU * hz * i as f32 / 48000.0).sin() * 0.5).collect()
    }

    #[test]
    fn idle_is_transparent() {
        let mut fx = RemixFx::new(48000.0);
        let mut l = sine(4800, 440.0);
        let mut r = l.clone();
        let want = l.clone();
        fx.process(&mut l, &mut r);
        let err = l.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-4, "{err}");
    }

    #[test]
    fn gater_cuts_the_off_beats() {
        let mut fx = RemixFx::new(48000.0);
        fx.set_tempo(120.0);
        fx.set(GATER, 1.0);
        fx.set(GATER_TIME, 0.0); // 1/4: open the first eighth of every beat
        let mut l = vec![0.5; 48000];
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        // beat = 24000 samples at 120: open 0..12000, closed 12000..24000
        assert!(l[6000] > 0.45);
        assert!(l[18000].abs() < 0.01);
    }

    #[test]
    fn filter_left_is_a_lowpass() {
        let mut fx = RemixFx::new(48000.0);
        fx.set(fx_x(0), 0.1);
        fx.set(fx_y(0), 0.0);
        fx.set(fx_on(0), 1.0);
        let mut l = sine(48000, 6000.0);
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        let peak = l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.02, "{peak}");
    }

    #[test]
    fn tape_stop_slows_to_silence() {
        let mut fx = RemixFx::new(48000.0);
        fx.set_tempo(120.0);
        fx.set(TAPE_STOP, 1.0);
        fx.set(TAPE_STOP_TIME, 2.0); // 1/4 = half a second
        let mut l = sine(48000, 440.0);
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        let tail = l[30000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(tail < 1e-3, "{tail}");
    }

    #[test]
    fn reverse_plays_backwards() {
        let mut fx = RemixFx::new(48000.0);
        fx.set_tempo(120.0);
        // a ramp in, held Reverse with 1/4 slices (24000 samples)
        let mut l: Vec<f32> = (0..48000).map(|i| i as f32 / 48000.0).collect();
        let mut r = l.clone();
        fx.set(REVERSE, 1.0);
        fx.process(&mut l, &mut r);
        // inside a slice the output falls while the input rises
        assert!(l[30000] < l[29000], "{} {}", l[29000], l[30000]);
    }

    #[test]
    fn delay_echo_at_an_eighth() {
        let mut fx = RemixFx::new(48000.0);
        fx.set_tempo(120.0);
        fx.set(fx_x(5), 0.3); // index 2 + 1 = 1/8
        fx.set(fx_y(5), 0.0);
        fx.set(fx_on(5), 1.0);
        let mut l = vec![0.0; 24000];
        // wait for the send to open
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        let mut l = vec![0.0; 24000];
        l[0] = 1.0;
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        let at = (100..24000).max_by(|&a, &b| l[a].abs().partial_cmp(&l[b].abs()).unwrap()).unwrap();
        assert!((at as i64 - 12000).abs() < 4, "{at}");
    }
}
