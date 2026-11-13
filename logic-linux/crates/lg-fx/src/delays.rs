//! The delays: Stereo Delay, Tape Delay and Echo, stand-ins for Logic's.
//!
//! Stereo Delay (project floats, current version): Left / Right Mix, Delay Time, Feedback,
//! Crossfeed L->R / R->L, Beat Sync, Note and Deviation per side, Input per side, the four
//! feedback / crossfeed phases (+1 normal, -1 inverted), Low / High Cut per side, Stereo Link.
//! Mix is a crossfade (dry = 1 - mix), as Logic's renders show; the cuts sit in the loops.
//! Synced times are Logic's note values (4 = 1/4, 6 = 1/4 triplet, 5.33 = dotted 1/8) plus the
//! deviation in percent.
//!
//! Tape Delay: a tape echo — the delay is a moving read head with wow (LFO, slow) and flutter
//! (fast, slightly random), a band-limited loop (Low Cut / High Cut), soft tape saturation
//! above the Clip Threshold in the loop, Smoothing for time changes (the "tape speeds up"
//! glide), Freeze (loop at unity, input muted), Stereo Spread (the right head offset) and the
//! Diffuse head mode (a smearing all-pass in the loop). Dry and Wet are separate levels.
//!
//! Echo: a simple synced echo — Time (a note menu), Repeat (feedback), Color (tilts the repeats
//! dark or bright), Dry and Wet.

use crate::util::{Biquad, Coefs, Delay, Drift, OnePole, Phase, Smooth, Values, db, flush, note_div_beats, plugin_impl_edit, sine01, sync_index_beats};
use lg_dsp::{Param, Unit};

const MAX_DELAY_S: f32 = 10.0;
const INPUTS: &[&str] = &["Off", "Left", "Right", "Left + Right", "Left - Right"];

static STEREO_PARAMS: [Param; 30] = [
    Param::new(0, "Left Mix", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(1, "Right Mix", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(2, "Left Delay Time", 0.0, 10000.0, 500.0, Unit::Ms),
    Param::new(3, "Right Delay Time", 0.0, 10000.0, 250.0, Unit::Ms),
    Param::new(4, "Left Feedback", 0.0, 100.0, 35.0, Unit::Percent),
    Param::new(5, "Right Feedback", 0.0, 100.0, 35.0, Unit::Percent),
    Param::new(6, "Crossfeed Left to Right", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(7, "Crossfeed Right to Left", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(10, "Beat Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(11, "Left Note", 0.25, 128.0, 4.0, Unit::None),
    Param::new(12, "Left Deviation", -50.0, 50.0, 0.0, Unit::Percent),
    Param::new(13, "Right Note", 0.25, 128.0, 8.0, Unit::None),
    Param::new(14, "Right Deviation", -50.0, 50.0, 0.0, Unit::Percent),
    Param::menu(15, "Left Input", INPUTS, 1.0),
    Param::menu(16, "Right Input", INPUTS, 2.0),
    Param::new(17, "Left Feedback Phase", -1.0, 1.0, 1.0, Unit::None),
    Param::new(18, "Right Feedback Phase", -1.0, 1.0, 1.0, Unit::None),
    Param::new(19, "Crossfeed Left to Right Phase", -1.0, 1.0, 1.0, Unit::None),
    Param::new(20, "Crossfeed Right to Left Phase", -1.0, 1.0, 1.0, Unit::None),
    Param::new(22, "Left High Cut", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(23, "Left Low Cut", 20.0, 20000.0, 20.0, Unit::Hz),
    Param::new(24, "Right High Cut", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(25, "Right Low Cut", 20.0, 20000.0, 20.0, Unit::Hz),
    Param::new(26, "Stereo Link", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(21, "(reserved)", 0.0, 1.0, 0.0, Unit::None),
    // (our own ids) the :2 and x2 buttons beside each Delay Time: halve / double it (momentary)
    Param::new(1000, "Left :2", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1001, "Left x2", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1002, "Right :2", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1003, "Right x2", 0.0, 1.0, 0.0, Unit::OnOff),
    // (our own id) the Routing pop-up: a preset of inputs, feedback and crossfeed
    Param::menu(ROUTING_ID, "Routing", ROUTINGS, 1.0),
];

pub const ROUTING_ID: u32 = 1004;
pub const ROUTINGS: &[&str] = &["Customized", "Straight", "Crossfeed", "90/10", "10/90", "Ping Pong L/R", "Pan L/R", "Rotate L/R"];

/// Stereo Delay's routings: (Left Input, Right Input, Left / Right Feedback, Crossfeed L→R,
/// R→L); None keeps that value. Inputs index INPUTS.
fn routing(k: usize) -> Option<(f32, f32, Option<f32>, f32, f32)> {
    Some(match ROUTINGS.get(k)? {
        &"Straight" => (1.0, 2.0, None, 0.0, 0.0),
        &"Crossfeed" => (1.0, 2.0, None, 50.0, 50.0),
        &"90/10" => (1.0, 2.0, None, 10.0, 10.0),
        &"10/90" => (1.0, 2.0, Some(10.0), 90.0, 90.0),
        // one input, bouncing between the sides
        &"Ping Pong L/R" => (3.0, 0.0, Some(0.0), 100.0, 50.0),
        // one echo on each side, left then right
        &"Pan L/R" => (3.0, 0.0, Some(0.0), 100.0, 0.0),
        // the sides swap on every repeat
        &"Rotate L/R" => (1.0, 2.0, Some(0.0), 100.0, 100.0),
        _ => return None,
    })
}

/// A routing pick sets its parameters and turns Stereo Link off; changing one of those
/// parameters by hand makes the routing Customized.
pub fn stereo_delay_links(set: &[(u32, f32)]) -> Vec<(u32, f32)> {
    let mut out = set.to_vec();
    if let Some(&(_, v)) = set.iter().find(|x| x.0 == ROUTING_ID) {
        if let Some((li, ri, fb, xl, xr)) = routing(v.round().max(0.0) as usize) {
            out.extend([(15, li), (16, ri), (6, xl), (7, xr), (26, 0.0)]);
            if let Some(f) = fb {
                out.extend([(4, f), (5, f)]);
            }
        }
    } else if set.iter().any(|x| matches!(x.0, 4..=7 | 15 | 16)) {
        out.push((ROUTING_ID, 0.0));
    }
    out
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    #[test]
    fn routing_menu_sets_paths_and_customized() {
        assert_eq!(ROUTINGS.len(), 8);
        let set = stereo_delay_links(&[(ROUTING_ID, 5.0)]);
        // Ping Pong: both inputs into the left line, no own feedback, crossfeed both ways, link off
        for want in [(15, 3.0), (16, 0.0), (4, 0.0), (5, 0.0), (6, 100.0), (7, 50.0), (26, 0.0)] {
            assert!(set.contains(&want), "{want:?}");
        }
        // a hand edit of a routed parameter: Customized
        assert_eq!(stereo_delay_links(&[(6, 20.0)]).last(), Some(&(ROUTING_ID, 0.0)));
        assert_eq!(stereo_delay_links(&[(0, 20.0)]).len(), 1);
    }
}

pub struct StereoDelay {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: [Smooth; 2],
    hc: [Biquad; 2],
    lc: [Biquad; 2],
}

impl StereoDelay {
    pub fn new(rate: f32) -> StereoDelay {
        let cap = (MAX_DELAY_S * rate) as usize + 8;
        let mut d = StereoDelay {
            p: Values::new(&STEREO_PARAMS),
            rate,
            bpm: 120.0,
            line: [Delay::new(cap), Delay::new(cap)],
            time: [Smooth::new(rate, 60.0, 1.0), Smooth::new(rate, 60.0, 1.0)],
            hc: [Biquad::default(); 2],
            lc: [Biquad::default(); 2],
        };
        d.update();
        d.time[0].snap();
        d.time[1].snap();
        d
    }

    /// The delay of one side, in samples.
    pub fn delay_samples(&self, side: usize) -> f32 {
        let p = &self.p;
        let ms = if p.on(10) {
            let (note, dev) = if side == 0 { (p.get(11), p.get(12)) } else { (p.get(13), p.get(14)) };
            note_div_beats(note) * 60000.0 / self.bpm as f32 * (1.0 + dev / 100.0)
        } else {
            p.get(2 + side as u32)
        };
        (ms * 0.001 * self.rate).clamp(1.0, MAX_DELAY_S * self.rate)
    }

    fn update(&mut self) {
        for s in 0..2 {
            self.time[s].target = self.delay_samples(s);
            let (hc, lc) = (self.p.get(22 + 2 * s as u32), self.p.get(23 + 2 * s as u32));
            self.hc[s].c = if hc >= 19999.0 { Coefs::THRU } else { Coefs::lowpass(self.rate, hc, 0.707) };
            self.lc[s].c = if lc <= 20.0 { Coefs::THRU } else { Coefs::highpass(self.rate, lc, 0.707) };
        }
    }

    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.hc[s].clear();
            self.lc[s].clear();
            self.time[s].snap();
        }
    }

    fn tail_secs(&self) -> f32 {
        let fb = self.p.get(4).max(self.p.get(5)).max(self.p.get(6)).max(self.p.get(7)) / 100.0;
        let t = self.delay_samples(0).max(self.delay_samples(1)) / self.rate;
        // repeats until -60 dB
        if fb >= 0.999 { 60.0 } else { t * (1.0 + (-3.0 / fb.max(1e-3).log10()).min(200.0)) }
    }
}

#[inline]
fn input(sel: usize, l: f32, r: f32) -> f32 {
    match sel {
        1 => l,
        2 => r,
        3 => 0.5 * (l + r),
        4 => 0.5 * (l - r),
        _ => 0.0,
    }
}

impl lg_dsp::Effect for StereoDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let n = left.len().min(right.len());
        let (mix_l, mix_r) = (p.get(0) / 100.0, p.get(1) / 100.0);
        let (fb_l, fb_r) = (p.get(4) / 100.0 * p.get(17).signum(), p.get(5) / 100.0 * p.get(18).signum());
        let (x_lr, x_rl) = (p.get(6) / 100.0 * p.get(19).signum(), p.get(7) / 100.0 * p.get(20).signum());
        let (in_l, in_r) = (p.idx(15), p.idx(16));
        for i in 0..n {
            let (l, r) = (left[i], right[i]);
            let tl = self.time[0].next();
            let tr = self.time[1].next();
            let dl = self.lc[0].run(self.hc[0].run(self.line[0].read(tl)));
            let dr = self.lc[1].run(self.hc[1].run(self.line[1].read(tr)));
            self.line[0].push(input(in_l, l, r) + dl * fb_l + dr * x_rl);
            self.line[1].push(input(in_r, l, r) + dr * fb_r + dl * x_lr);
            left[i] = l * (1.0 - mix_l) + dl * mix_l;
            right[i] = r * (1.0 - mix_r) + dr * mix_r;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 && (bpm - self.bpm).abs() > 1e-6 {
            self.bpm = bpm;
            self.update();
        }
    }
}

/// Left / right parameter pairs that Stereo Link moves together.
const LINKED: [(u32, u32); 10] = [(0, 1), (2, 3), (4, 5), (6, 7), (11, 13), (12, 14), (17, 18), (19, 20), (22, 24), (23, 25)];

/// A delay time halved (k = 0.5) or doubled: a note value (stored as a whole-note divisor)
/// or milliseconds.
fn scale_time(p: &mut Values, synced: bool, note: u32, ms: u32, k: f32) {
    if synced {
        p.set(note, p.get(note) / k);
    } else {
        p.set(ms, p.get(ms) * k);
    }
}

impl StereoDelay {
    /// A user edit: :2 / x2 halve or double a side's delay; with Stereo Link on, a change to
    /// one side moves the other by the same amount, keeping their relative values.
    pub fn edit(&mut self, id: u32, value: f32) {
        let link = self.p.on(26);
        if (1000..=1003).contains(&id) {
            if value > 0.5 {
                let k = if id % 2 == 0 { 0.5 } else { 2.0 };
                let sync = self.p.on(10);
                let right = id >= 1002;
                if !right || link {
                    scale_time(&mut self.p, sync, 11, 2, k);
                }
                if right || link {
                    scale_time(&mut self.p, sync, 13, 3, k);
                }
                self.update();
            }
            return;
        }
        if link {
            if let Some(&(a, b)) = LINKED.iter().find(|(a, b)| *a == id || *b == id) {
                let other = if id == a { b } else { a };
                let delta = value - self.p.get(id);
                self.p.set(other, self.p.get(other) + delta);
            }
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
}

plugin_impl_edit!(StereoDelay);

// ---------------------------------------------------------------------------------------------

static TAPE_PARAMS: [Param; 20] = [
    Param::new(3, "Feedback", 0.0, 100.0, 61.0, Unit::Percent),
    Param::new(4, "High Cut", 200.0, 20000.0, 1500.0, Unit::Hz),
    Param::new(5, "Low Cut", 20.0, 5000.0, 520.0, Unit::Hz),
    Param::new(6, "Tempo Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(7, "Note", 0.25, 128.0, 4.0, Unit::None),
    Param::new(8, "Deviation", -50.0, 50.0, 0.0, Unit::Percent),
    Param::new(9, "Smoothing", 0.0, 1000.0, 40.0, Unit::Ms),
    Param::new(10, "Flutter Rate", 0.1, 10.0, 1.6, Unit::Hz),
    Param::new(11, "Flutter Intensity", 0.0, 100.0, 13.0, Unit::Percent),
    Param::new(12, "LFO Rate", 0.01, 10.0, 0.38, Unit::Hz),
    Param::new(13, "LFO Intensity", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(14, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(19, "Dry", 0.0, 100.0, 90.0, Unit::Percent),
    Param::new(20, "Wet", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(21, "Clip Threshold", -30.0, 0.0, -5.4, Unit::Db),
    Param::new(22, "Delay Time", 1.0, 5000.0, 666.62, Unit::Ms),
    Param::new(23, "Stereo Spread", 0.0, 100.0, 0.0, Unit::Percent),
    Param::menu(24, "Tape Head Mode", &["Clean", "Diffuse"], 0.0),
    // (our own ids) the :2 and x2 buttons: halve / double Delay Time (momentary)
    Param::new(1000, ":2", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1001, "x2", 0.0, 1.0, 0.0, Unit::OnOff),
];

const DIFFUSE_MS: [f32; 3] = [7.3, 11.9, 17.1];

pub struct TapeDelay {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: Smooth,
    hc: [Biquad; 2],
    lc: [Biquad; 2],
    tone: [OnePole; 2],
    wow: Phase,
    flutter: [Drift; 2],
    flutter_ph: Phase,
    diffuse: [[Delay; 3]; 2],
    clip: f32,
}

impl TapeDelay {
    pub fn new(rate: f32) -> TapeDelay {
        let cap = (5.2 * rate) as usize;
        let dcap = (0.02 * rate) as usize;
        let mut d = TapeDelay {
            p: Values::new(&TAPE_PARAMS),
            rate,
            bpm: 120.0,
            line: [Delay::new(cap), Delay::new(cap)],
            time: Smooth::new(rate, 40.0, 1.0),
            hc: [Biquad::default(); 2],
            lc: [Biquad::default(); 2],
            tone: [OnePole::new(rate, 9000.0); 2],
            wow: Phase::default(),
            flutter: [Drift::new(77), Drift::new(91)],
            flutter_ph: Phase { p: 0.3 },
            diffuse: std::array::from_fn(|_| std::array::from_fn(|_| Delay::new(dcap))),
            clip: 1.0,
        };
        d.update();
        d.time.snap();
        d
    }

    pub fn delay_samples(&self) -> f32 {
        let p = &self.p;
        let ms = if p.on(6) { note_div_beats(p.get(7)) * 60000.0 / self.bpm as f32 * (1.0 + p.get(8) / 100.0) } else { p.get(22) };
        (ms * 0.001 * self.rate).clamp(4.0, 5.0 * self.rate)
    }

    fn update(&mut self) {
        let p = &self.p;
        // Smoothing: how slowly the tape changes speed to a new time
        self.time.set_time(self.rate, p.get(9).max(1.0) * 4.0);
        self.time.target = self.delay_samples();
        for s in 0..2 {
            self.hc[s].c = Coefs::lowpass(self.rate, p.get(4), 0.6);
            self.lc[s].c = Coefs::highpass(self.rate, p.get(5), 0.6);
        }
        self.clip = db(p.get(21));
    }

    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.hc[s].clear();
            self.lc[s].clear();
            for d in self.diffuse[s].iter_mut() {
                d.clear();
            }
        }
        self.time.snap();
    }

    fn tail_secs(&self) -> f32 {
        let fb = self.p.get(3) / 100.0;
        if self.p.on(14) || fb >= 0.99 {
            return 60.0;
        }
        self.delay_samples() / self.rate * (1.0 + (-3.0 / fb.max(1e-3).log10()).min(200.0))
    }
}

/// Tape saturation: linear below the threshold, rounding off smoothly above it.
#[inline]
fn tape_sat(x: f32, t: f32) -> f32 {
    let a = x.abs();
    if a <= t {
        x
    } else {
        (t + t * ((a - t) / t).tanh()).copysign(x)
    }
}

impl lg_dsp::Effect for TapeDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let n = left.len().min(right.len());
        let rate = self.rate;
        let freeze = p.on(14);
        let fb = if freeze { 1.0 } else { p.get(3) / 100.0 };
        let (dry, wet) = (p.get(19) / 100.0, p.get(20) / 100.0);
        let wow_amt = p.get(13) / 100.0 * 0.004 * rate; // up to ~4 ms of wow
        let wow_hz = p.get(12);
        let fl_amt = p.get(11) / 100.0 * 0.0006 * rate; // up to ~0.6 ms of flutter
        let fl_hz = p.get(10);
        let spread = p.get(23) / 100.0 * 0.03 * rate;
        let diffuse = p.idx(24) == 1;
        for i in 0..n {
            let (l, r) = (left[i], right[i]);
            let base = self.time.next();
            let w = sine01(self.wow.step(rate, wow_hz)) * wow_amt;
            let fph = sine01(self.flutter_ph.step(rate, fl_hz));
            let mut out = [0.0f32; 2];
            for s in 0..2 {
                let fl = (0.6 * fph + 0.4 * self.flutter[s].next(rate, fl_hz * 2.3)) * fl_amt;
                let d = base + w + fl + if s == 1 { spread } else { 0.0 } + wow_amt + fl_amt + 2.0;
                let mut y = self.line[s].read(d);
                y = self.lc[s].run(self.hc[s].run(y));
                if diffuse {
                    for (k, dl) in self.diffuse[s].iter_mut().enumerate() {
                        let len = DIFFUSE_MS[k] * 0.001 * rate * if s == 1 { 1.09 } else { 1.0 };
                        let z = dl.read(len);
                        let v = y + 0.5 * z;
                        dl.push(v);
                        y = z - 0.5 * v;
                    }
                }
                out[s] = y;
            }
            for s in 0..2 {
                let x = if freeze { 0.0 } else if s == 0 { l } else { r };
                // the record head saturates; the loop is band-limited by the cuts
                let rec = tape_sat(x + out[s] * fb, self.clip);
                let rec = self.tone[s].lp(rec);
                self.line[s].push(flush(rec));
            }
            left[i] = l * dry + out[0] * wet;
            right[i] = r * dry + out[1] * wet;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 && (bpm - self.bpm).abs() > 1e-6 {
            self.bpm = bpm;
            self.update();
        }
    }
}

impl TapeDelay {
    pub fn edit(&mut self, id: u32, value: f32) {
        if id == 1000 || id == 1001 {
            if value > 0.5 {
                let sync = self.p.on(6);
                scale_time(&mut self.p, sync, 7, 22, if id == 1000 { 0.5 } else { 2.0 });
                self.update();
            }
            return;
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
}

plugin_impl_edit!(TapeDelay);

// ---------------------------------------------------------------------------------------------

/// Echo's Time menu (index 7 = 1/8, as Logic stores its default).
const ECHO_TIMES: &[&str] = &["1/64.", "1/32", "1/16t", "1/32.", "1/16", "1/8t", "1/16.", "1/8", "1/4t", "1/8.", "1/4", "1/2t", "1/4.", "1/2", "1/1t", "1/2.", "1/1", "2/1t", "1/1.", "2/1", "4/1t", "2/1.", "4/1"];

static ECHO_PARAMS: [Param; 7] = [
    Param::menu(16, "Time", ECHO_TIMES, 7.0),
    Param::new(17, "Repeat", 0.0, 100.0, 64.0, Unit::Percent),
    Param::new(18, "Color", 0.0, 100.0, 37.0, Unit::Percent),
    Param::new(19, "Dry", 0.0, 100.0, 96.0, Unit::Percent),
    Param::new(20, "Wet", 0.0, 100.0, 20.0, Unit::Percent),
    // (our own ids) :2 / x2: half or double the note value (momentary)
    Param::new(1000, ":2", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1001, "x2", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct Echo {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: Smooth,
    lp: [OnePole; 2],
    hp: [OnePole; 2],
}

impl Echo {
    pub fn new(rate: f32) -> Echo {
        let cap = (4.2 * rate) as usize;
        let mut e = Echo { p: Values::new(&ECHO_PARAMS), rate, bpm: 120.0, line: [Delay::new(cap), Delay::new(cap)], time: Smooth::new(rate, 50.0, 1.0), lp: [OnePole::default(); 2], hp: [OnePole::default(); 2] };
        e.update();
        e.time.snap();
        e
    }

    /// The echo time in beats for a Time menu index (index 7 = 1/8; same note order as
    /// Tremolo's sync rates, four places earlier — unconfirmed).
    pub fn time_beats(index: f32) -> f32 {
        sync_index_beats(index + 4.0)
    }

    fn update(&mut self) {
        let beats = Echo::time_beats(self.p.get(16));
        self.time.target = (beats * 60.0 / self.bpm as f32 * self.rate).clamp(1.0, 4.0 * self.rate);
        // Color: 0 = dark repeats, 100 = bright (thin) repeats
        let c = self.p.get(18) / 100.0;
        let lp = 1500.0 * 12f32.powf(c);
        let hp = 40.0 * 15f32.powf(c);
        for s in 0..2 {
            self.lp[s].set(self.rate, lp.min(20000.0));
            self.hp[s].set(self.rate, hp);
        }
    }

    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.lp[s].y = 0.0;
            self.hp[s].y = 0.0;
        }
        self.time.snap();
    }

    fn tail_secs(&self) -> f32 {
        let fb = self.p.get(17) / 100.0;
        self.time.target / self.rate * (1.0 + (-3.0 / fb.max(1e-3).log10()).min(200.0))
    }
}

impl lg_dsp::Effect for Echo {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let fb = (self.p.get(17) / 100.0).min(0.99);
        let (dry, wet) = (self.p.get(19) / 100.0, self.p.get(20) / 100.0);
        for i in 0..n {
            let t = self.time.next();
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let y = self.line[s].read(t);
                let c = self.hp[s].hp(self.lp[s].lp(y));
                self.line[s].push(*x + c * fb);
                *x = *x * dry + y * wet;
            }
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 && (bpm - self.bpm).abs() > 1e-6 {
            self.bpm = bpm;
            self.update();
        }
    }
}

impl Echo {
    pub fn edit(&mut self, id: u32, value: f32) {
        if id == 1000 || id == 1001 {
            if value > 0.5 {
                // the menu runs straight / triplet / dotted, so three steps = a factor of two
                let i = self.p.get(16) + if id == 1000 { -3.0 } else { 3.0 };
                if (0.0..=(ECHO_TIMES.len() - 1) as f32).contains(&i) {
                    self.p.set(16, i);
                    self.update();
                }
            }
            return;
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
}

plugin_impl_edit!(Echo);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_double_buttons_and_stereo_link() {
        use lg_dsp::Plugin;
        let mut d = StereoDelay::new(48000.0);
        lg_dsp::Effect::set_tempo(&mut d, 120.0);
        let l0 = d.delay_samples(0);
        let r0 = d.delay_samples(1);
        d.set(1000, 1.0);
        assert!((d.delay_samples(0) - l0 * 0.5).abs() < 1.0);
        assert!((d.delay_samples(1) - r0).abs() < 1e-3);
        d.set(1003, 1.0);
        assert!((d.delay_samples(1) - r0 * 2.0).abs() < 1.0);
        // free times too
        d.set(10, 0.0);
        d.set(2, 400.0);
        d.set(1001, 1.0);
        assert_eq!(d.get(2), 800.0);
        // Stereo Link: both sides move by the same amount
        d.set(26, 1.0);
        let (f4, f5) = (d.get(4), d.get(5));
        d.set(4, f4 - 10.0);
        assert_eq!((d.get(4), d.get(5)), (f4 - 10.0, f5 - 10.0));
        d.set(1000, 1.0);
        assert_eq!(d.get(2), 400.0);
        // Tape Delay and Echo
        let mut t = TapeDelay::new(48000.0);
        let n = t.get(7);
        t.set(1000, 1.0);
        assert_eq!(t.get(7), n * 2.0);
        t.set(1001, 1.0);
        assert_eq!(t.get(7), n);
        let mut e = Echo::new(48000.0);
        assert_eq!(ECHO_TIMES[e.get(16) as usize], "1/8");
        e.set(1000, 1.0);
        assert_eq!(ECHO_TIMES[e.get(16) as usize], "1/16");
        e.set(1001, 1.0);
        e.set(1001, 1.0);
        assert_eq!(ECHO_TIMES[e.get(16) as usize], "1/4");
    }
    use lg_dsp::{Effect, Plugin};

    fn first_echo(fx: &mut dyn Effect, n: usize) -> usize {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = vec![0.0; n];
        fx.process(&mut l, &mut r);
        (1..n).max_by(|&a, &b| l[a].abs().partial_cmp(&l[b].abs()).unwrap()).unwrap()
    }

    #[test]
    fn stereo_delay_time_is_a_quarter_at_120() {
        let mut d = StereoDelay::new(48000.0);
        d.set_tempo(120.0);
        assert_eq!(first_echo(&mut d, 30000), 24000);
        d.set(10, 0.0);
        d.set(2, 123.0);
        d.reset();
        let at = first_echo(&mut d, 30000);
        assert!((at as i32 - 5904).abs() <= 1, "{at}");
    }

    #[test]
    fn tape_delay_time_close_to_note() {
        let mut d = TapeDelay::new(48000.0);
        d.set(13, 0.0);
        d.set(11, 0.0);
        d.set_tempo(120.0);
        let at = first_echo(&mut d, 40000);
        assert!((at as i32 - 24000).abs() < 30, "{at}");
    }

    #[test]
    fn echo_eighth() {
        let mut e = Echo::new(48000.0);
        e.set_tempo(120.0);
        assert_eq!(first_echo(&mut e, 20000), 12000);
    }
}
