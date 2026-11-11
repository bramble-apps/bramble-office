//! The stompboxes: Robo Flanger, Total Tremolo, Blue Echo, Tru-Tape Delay and the Vintage Drive
//! pedal — Logic's Pedalboard pedals that also exist as their own plug-ins — and Pedalboard
//! itself, a chain of them.
//!
//! The pedal family stores [On (footswitch), Sync, Rate or Time, ...knobs]; a synced time is a
//! share of a whole note (Blue Echo 0.125 = 1/8 note, Tru-Tape 0.25 = 1/4 at their defaults).
//! - Robo Flanger: Rate, Depth, Feedback, Manual (the sweep's centre, ms) — a through-zero-less
//!   classic flanger with a sine sweep.
//! - Total Tremolo: Rate (beats when synced), Depth, Wave (ten shapes), Smooth, Volume.
//! - Blue Echo: Time, Repeats, Mix, Tone Cut (Off / Hi / Lo in the repeats), Mute.
//! - Tru-Tape Delay: Time, Feedback, Mix, Hi Cut, Lo Cut, Dirt (tape saturation), Flutter, and
//!   Norm / Reverse (reverse plays each delay-length slice backwards, cross-faded).
//! - Vintage Drive: Drive, Tone, Level, Fat (the overdrive pedal the sample Pedalboard holds).
//!
//! Pedalboard: in a project the board's own float block is a slot table (mostly empty-slot
//! sentinels); each pedal follows as its own record ('xoBSH', then a GAMETSPP block). Every
//! pedal block carries the same block id, and where the pedal TYPE is stored is not decoded yet,
//! so `Pedalboard::from_aucu` recognises the pedals above by their float signature and plays
//! them in series. Split / Mix routing (the two buses) is not modelled.

use crate::util::{Biquad, Coefs, Delay, Drift, OnePole, Phase, Smooth, Values, beats_hz, db, flush, plugin_impl, sine01, tri};
use lg_dsp::{Effect, Param, Unit};

/// A pedal's synced or free time in samples (synced: a share of a whole note; free: seconds).
fn pedal_time(sync: bool, v: f32, bpm: f64, rate: f32) -> f32 {
    let secs = if sync { v * 4.0 * 60.0 / bpm as f32 } else { v };
    (secs * rate).max(1.0)
}

// ---------------------------------------------------------------------------------------------

static ROBO_PARAMS: [Param; 6] = [
    Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(2, "Rate", 0.01, 10.0, 0.37, Unit::Hz),
    Param::new(3, "Depth", 0.0, 100.0, 28.5, Unit::Percent),
    Param::new(4, "Feedback", -100.0, 100.0, 32.0, Unit::Percent),
    Param::new(5, "Manual", 0.1, 10.0, 7.4, Unit::Ms),
];

pub struct RoboFlanger {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    ph: Phase,
    fb: [f32; 2],
}

impl RoboFlanger {
    pub fn new(rate: f32) -> RoboFlanger {
        RoboFlanger { p: Values::new(&ROBO_PARAMS), rate, bpm: 120.0, line: [Delay::new((0.03 * rate) as usize), Delay::new((0.03 * rate) as usize)], ph: Phase::default(), fb: [0.0; 2] }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.fb = [0.0; 2];
    }
    fn tail_secs(&self) -> f32 {
        0.2
    }
}

impl Effect for RoboFlanger {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(0) {
            return;
        }
        let rate = self.rate;
        // synced: the rate value is the cycle length as a share of a whole note
        let hz = if self.p.on(1) { beats_hz(4.0 * self.p.get(2).max(0.01), self.bpm) } else { self.p.get(2) };
        let depth = self.p.get(3) / 100.0;
        let fb = (self.p.get(4) / 100.0).clamp(-0.95, 0.95);
        let centre = self.p.get(5) * 0.001 * rate;
        for i in 0..left.len().min(right.len()) {
            let ph = self.ph.step(rate, hz);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let m = sine01(ph + 0.25 * s as f32);
                let d = (centre * (1.0 + 0.95 * depth * m)).max(1.0);
                self.line[s].push(*x + self.fb[s] * fb);
                let y = self.line[s].read_lin(d);
                self.fb[s] = flush(y);
                *x = 0.5 * (*x + y);
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
    }
}

plugin_impl!(RoboFlanger);

// ---------------------------------------------------------------------------------------------

const WAVES: &[&str] = &["Square", "Saw Up", "Saw Down", "Triangle", "Pulse 25%", "Pulse 75%", "Random", "Stepped", "Sine", "Smooth Random"];

static TREM_PARAMS: [Param; 7] = [
    Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Rate", 0.01, 20.0, 0.1667, Unit::None),
    Param::new(3, "Depth", 0.0, 100.0, 80.0, Unit::Percent),
    Param::menu(4, "Wave", WAVES, 8.0),
    Param::new(5, "Smooth", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(6, "Volume", -24.0, 12.0, 3.0, Unit::Db),
];

pub struct TotalTremolo {
    p: Values,
    rate: f32,
    bpm: f64,
    ph: Phase,
    rnd: Drift,
    held: f32,
    smooth: OnePole,
}

impl TotalTremolo {
    pub fn new(rate: f32) -> TotalTremolo {
        let mut t = TotalTremolo { p: Values::new(&TREM_PARAMS), rate, bpm: 120.0, ph: Phase::default(), rnd: Drift::new(5), held: 0.0, smooth: OnePole::default() };
        t.update();
        t
    }
    fn hz(&self) -> f32 {
        if self.p.on(1) { beats_hz(4.0 * self.p.get(2).max(0.01), self.bpm) } else { self.p.get(2).max(0.01) }
    }
    fn update(&mut self) {
        let sm = self.p.get(5) / 100.0;
        self.smooth.set(self.rate, self.hz() * (1.5 + 200.0 * (1.0 - sm).powi(2)));
    }
    fn clear(&mut self) {
        self.smooth.y = 1.0;
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for TotalTremolo {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(0) {
            return;
        }
        let depth = self.p.get(3) / 100.0;
        let wave = self.p.idx(4);
        let vol = db(self.p.get(6) - 3.0);
        let hz = self.hz();
        for i in 0..left.len().min(right.len()) {
            let before = self.ph.p;
            let q = self.ph.step(self.rate, hz);
            if q < before {
                self.held = self.rnd.next(self.rate, hz) * 0.5 + 0.5;
            }
            // unipolar 0..1 shape
            let w = match wave {
                0 => if q < 0.5 { 1.0 } else { 0.0 },
                1 => q,
                2 => 1.0 - q,
                3 => 0.5 + 0.5 * tri(q),
                4 => if q < 0.25 { 1.0 } else { 0.0 },
                5 => if q < 0.75 { 1.0 } else { 0.0 },
                6 => self.held,
                7 => (q * 4.0).floor() / 3.0,
                8 => 0.5 + 0.5 * sine01(q),
                _ => self.rnd.next(self.rate, hz) * 0.5 + 0.5,
            };
            let w = self.smooth.lp(w);
            let g = (1.0 - depth * (1.0 - w)) * vol;
            left[i] *= g;
            right[i] *= g;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
        self.update();
    }
}

plugin_impl!(TotalTremolo);

// ---------------------------------------------------------------------------------------------

static BLUE_PARAMS: [Param; 7] = [
    Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Time", 0.01, 4.0, 0.125, Unit::None),
    Param::new(3, "Repeats", 0.0, 100.0, 73.0, Unit::Percent),
    Param::new(4, "Mix", 0.0, 100.0, 20.0, Unit::Percent),
    Param::menu(5, "Tone Cut", &["Off", "Hi", "Lo"], 1.0),
    Param::new(6, "Mute", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct BlueEcho {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: Smooth,
    tone: [Biquad; 2],
}

impl BlueEcho {
    pub fn new(rate: f32) -> BlueEcho {
        let mut b = BlueEcho { p: Values::new(&BLUE_PARAMS), rate, bpm: 120.0, line: [Delay::new((4.1 * rate) as usize), Delay::new((4.1 * rate) as usize)], time: Smooth::new(rate, 60.0, 1.0), tone: [Biquad::default(); 2] };
        b.update();
        b.time.snap();
        b
    }
    fn update(&mut self) {
        self.time.target = pedal_time(self.p.on(1), self.p.get(2), self.bpm, self.rate).min(4.0 * self.rate);
        let c = match self.p.idx(5) {
            1 => Coefs::lowpass(self.rate, 3000.0, 0.707),
            2 => Coefs::highpass(self.rate, 400.0, 0.707),
            _ => Coefs::THRU,
        };
        self.tone[0].c = c;
        self.tone[1].c = c;
    }
    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.tone[s].clear();
        }
        self.time.snap();
    }
    fn tail_secs(&self) -> f32 {
        let fb = self.p.get(3) / 100.0;
        self.time.target / self.rate * (1.0 + (-3.0 / fb.max(1e-3).log10()).min(100.0))
    }
}

impl Effect for BlueEcho {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(0) {
            return;
        }
        let fb = (self.p.get(3) / 100.0).min(0.98);
        let mix = self.p.get(4) / 100.0;
        let mute = self.p.on(6);
        for i in 0..left.len().min(right.len()) {
            let t = self.time.next();
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let y = self.line[s].read(t);
                let inp = if mute { 0.0 } else { *x };
                let rep = self.tone[s].run(y);
                // a little analogue softness in the loop
                self.line[s].push((inp + rep * fb).tanh());
                *x = *x * (1.0 - mix) + y * mix;
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
        self.update();
    }
}

plugin_impl!(BlueEcho);

// ---------------------------------------------------------------------------------------------

static TRU_PARAMS: [Param; 10] = [
    Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(2, "Time", 0.01, 4.0, 0.25, Unit::None),
    Param::new(3, "Feedback", 0.0, 100.0, 68.0, Unit::Percent),
    Param::new(4, "Mix", 0.0, 100.0, 19.5, Unit::Percent),
    Param::new(5, "Hi Cut", 200.0, 20000.0, 1600.0, Unit::Hz),
    Param::new(6, "Lo Cut", 20.0, 2000.0, 170.0, Unit::Hz),
    Param::new(7, "Dirt", 0.0, 100.0, 82.0, Unit::Percent),
    Param::new(8, "Flutter", 0.0, 100.0, 80.0, Unit::Percent),
    Param::menu(9, "Norm/Reverse", &["Norm", "Reverse"], 0.0),
];

pub struct TruTapeDelay {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    time: Smooth,
    hc: [Biquad; 2],
    lc: [Biquad; 2],
    flutter: Drift,
    wow: Phase,
    rev_pos: f32,
}

impl TruTapeDelay {
    pub fn new(rate: f32) -> TruTapeDelay {
        let mut t = TruTapeDelay {
            p: Values::new(&TRU_PARAMS),
            rate,
            bpm: 120.0,
            line: [Delay::new((8.5 * rate) as usize), Delay::new((8.5 * rate) as usize)],
            time: Smooth::new(rate, 80.0, 1.0),
            hc: [Biquad::default(); 2],
            lc: [Biquad::default(); 2],
            flutter: Drift::new(17),
            wow: Phase::default(),
            rev_pos: 0.0,
        };
        t.update();
        t.time.snap();
        t
    }
    fn update(&mut self) {
        self.time.target = pedal_time(self.p.on(1), self.p.get(2), self.bpm, self.rate).min(4.0 * self.rate);
        for s in 0..2 {
            self.hc[s].c = Coefs::lowpass(self.rate, self.p.get(5).min(self.rate * 0.45), 0.707);
            self.lc[s].c = Coefs::highpass(self.rate, self.p.get(6), 0.707);
        }
    }
    fn clear(&mut self) {
        for s in 0..2 {
            self.line[s].clear();
            self.hc[s].clear();
            self.lc[s].clear();
        }
        self.time.snap();
        self.rev_pos = 0.0;
    }
    fn tail_secs(&self) -> f32 {
        let fb = self.p.get(3) / 100.0;
        2.0 * self.time.target / self.rate * (1.0 + (-3.0 / fb.max(1e-3).log10()).min(100.0))
    }
}

impl Effect for TruTapeDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(0) {
            return;
        }
        let rate = self.rate;
        let fb = (self.p.get(3) / 100.0).min(0.98);
        let mix = self.p.get(4) / 100.0;
        let dirt = self.p.get(7) / 100.0;
        let fl = self.p.get(8) / 100.0 * 0.0008 * rate;
        let reverse = self.p.idx(9) == 1;
        let drive = 1.0 + 3.0 * dirt;
        for i in 0..left.len().min(right.len()) {
            let t = self.time.next();
            let wob = (0.6 * sine01(self.wow.step(rate, 0.7)) + 0.4 * self.flutter.next(rate, 6.0)) * fl;
            // reverse: read backwards through the last slice of length t, cross-fading at the ends
            self.rev_pos += 1.0;
            if self.rev_pos >= t {
                self.rev_pos -= t;
            }
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let y = if reverse {
                    let a = self.rev_pos;
                    let d = (2.0 * a + 2.0).min(2.0 * t);
                    let w = (a / t * std::f32::consts::PI).sin();
                    self.line[s].read(d + 2.0 + fl) * w
                } else {
                    self.line[s].read(t + wob + fl + 2.0)
                };
                let rep = self.lc[s].run(self.hc[s].run(y));
                let rec = (*x + rep * fb) * drive;
                self.line[s].push(rec.tanh() / drive);
                *x = *x * (1.0 - mix) + y * mix;
            }
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
        self.update();
    }
}

plugin_impl!(TruTapeDelay);

// ---------------------------------------------------------------------------------------------

static DRIVE_PARAMS: [Param; 5] = [
    Param::new(0, "On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1, "Drive", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(2, "Tone", 200.0, 10000.0, 1300.0, Unit::Hz),
    Param::new(3, "Level", -40.0, 12.0, -10.0, Unit::Db),
    Param::new(4, "Fat", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct VintageDrive {
    p: Values,
    rate: f32,
    os: [crate::distortion::Over4; 2],
    pre: [Biquad; 2],
    tone: [Biquad; 2],
    fat: [Biquad; 2],
}

impl VintageDrive {
    pub fn new(rate: f32) -> VintageDrive {
        let mut v = VintageDrive { p: Values::new(&DRIVE_PARAMS), rate, os: [crate::distortion::Over4::new(rate); 2], pre: [Biquad::default(); 2], tone: [Biquad::default(); 2], fat: [Biquad::default(); 2] };
        v.update();
        v
    }
    fn update(&mut self) {
        let r = self.rate;
        for s in 0..2 {
            // the classic mid-hump: low end trimmed before the clipper
            self.pre[s].c = Coefs::highpass(r, 720.0 * if self.p.on(4) { 0.35 } else { 1.0 }, 0.5);
            self.tone[s].c = Coefs::lowpass(r, self.p.get(2), 0.7);
            self.fat[s].c = if self.p.on(4) { Coefs::low_shelf(r, 200.0, 0.7, 3.0) } else { Coefs::THRU };
        }
    }
    fn clear(&mut self) {
        for s in 0..2 {
            self.os[s].clear();
            for b in [&mut self.pre[s], &mut self.tone[s], &mut self.fat[s]] {
                b.clear();
            }
        }
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for VintageDrive {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.p.on(0) {
            return;
        }
        let g = db(self.p.get(1) / 100.0 * 40.0 + 6.0);
        let lvl = db(self.p.get(3) + 6.0);
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let hp = self.pre[s].run(*x);
                let v = (*x - hp) * 0.3 + hp; // keep some lows
                let c = self.os[s].run(v * g, |u| (u * 1.1).tanh() * 0.9 + 0.1 * (u / (1.0 + u.abs())));
                *x = self.fat[s].run(self.tone[s].run(c)) * lvl;
            }
        }
    }
}

plugin_impl!(VintageDrive);

// ---------------------------------------------------------------------------------------------

/// Pedalboard: a series chain of pedals.
pub struct Pedalboard {
    p: Values,
    rate: f32,
    pedals: Vec<Box<dyn Effect>>,
}

static BOARD_PARAMS: [Param; 0] = [];

impl Pedalboard {
    pub fn new(rate: f32) -> Pedalboard {
        Pedalboard { p: Values::new(&BOARD_PARAMS), rate, pedals: Vec::new() }
    }

    /// Adds a pedal (called while loading, not from the audio thread).
    pub fn push(&mut self, pedal: Box<dyn Effect>) {
        self.pedals.push(pedal);
    }

    pub fn pedal_names(&self) -> usize {
        self.pedals.len()
    }

    /// The pedal a float block most likely belongs to (see the module notes): its name.
    pub fn pedal_name_for(floats: &[f32]) -> &'static str {
        let f = |i: usize| floats.get(i).copied().unwrap_or(0.0);
        if floats.len() >= 9 && f(5) > 300.0 && f(6) >= 20.0 && f(6) < 2000.0 {
            "Tru-Tape Delay"
        } else if (100.0..20000.0).contains(&f(2)) && (-60.0..=12.0).contains(&f(3)) {
            "Vintage Drive"
        } else if f(4) >= 0.0 && f(4).fract() == 0.0 && f(4) < 10.0 && f(3) <= 100.0 && floats.len() >= 7 && f(6) > -24.0 && f(6) < 12.0 && f(5) <= 100.0 && f(5) > 1.0 {
            "Total Tremolo"
        } else if f(5) > 0.0 && f(5) <= 10.0 && f(5).fract() != 0.0 {
            "Robo Flanger"
        } else {
            "Blue Echo"
        }
    }

    /// The pedal a float block most likely belongs to, as a plug-in.
    pub fn pedal_for(floats: &[f32], rate: f32) -> Box<dyn Effect> {
        let mut b: Box<dyn Effect> = match Pedalboard::pedal_name_for(floats) {
            "Tru-Tape Delay" => Box::new(TruTapeDelay::new(rate)),
            "Vintage Drive" => Box::new(VintageDrive::new(rate)),
            "Total Tremolo" => Box::new(TotalTremolo::new(rate)),
            "Robo Flanger" => Box::new(RoboFlanger::new(rate)),
            _ => Box::new(BlueEcho::new(rate)),
        };
        b.load_floats(floats);
        b
    }

    /// The pedal records of a Pedalboard AuCU payload: each 'xoBSH' record's GAMETSPP floats.
    pub fn records(payload: &[u8]) -> Vec<Vec<f32>> {
        let find = |h: &[u8], n: &[u8], from: usize| h.get(from..).and_then(|t| t.windows(n.len()).position(|w| w == n)).map(|p| p + from);
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(rec) = find(payload, b"xoBSH", at) {
            let Some(tag) = find(payload, b"GAMETSPP", rec) else { break };
            let start = tag + 16;
            let end = find(payload, b"90PL", start).unwrap_or(payload.len()).min(start + 4 * 64);
            let floats: Vec<f32> = payload[start..end].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            if !floats.is_empty() {
                out.push(floats);
            }
            at = end;
        }
        out
    }

    /// Builds the board from a Pedalboard AuCU payload: each 'xoBSH' record's GAMETSPP block
    /// (block id, a zero word, then floats up to the '90PL' marker) becomes a pedal.
    pub fn from_aucu(payload: &[u8], rate: f32) -> Pedalboard {
        let mut board = Pedalboard::new(rate);
        for floats in Pedalboard::records(payload) {
            board.push(Pedalboard::pedal_for(&floats, rate));
        }
        board
    }

    fn update(&mut self) {}
    fn clear(&mut self) {
        for p in self.pedals.iter_mut() {
            p.reset();
        }
    }
    fn tail_secs(&self) -> f32 {
        self.pedals.iter().map(|p| p.tail()).sum::<f32>().max(0.0)
    }
}

impl Effect for Pedalboard {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for p in self.pedals.iter_mut() {
            p.process(left, right);
        }
        let _ = self.rate;
    }
    fn set_tempo(&mut self, bpm: f64) {
        for p in self.pedals.iter_mut() {
            p.set_tempo(bpm);
        }
    }
    fn latency(&self) -> usize {
        self.pedals.iter().map(|p| p.latency()).sum()
    }
}

plugin_impl!(Pedalboard);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_pick_the_right_pedal() {
        // the sample project's pedal (Drive, Tone, Level, Fat) and the standalone defaults
        let names = |f: &[f32]| Pedalboard::pedal_for(f, 48000.0).params().iter().map(|p| p.name).collect::<Vec<_>>().join(",");
        assert!(names(&[1.0, 30.0, 1300.0, -10.0, 1.0]).contains("Fat"));
        assert!(names(&[1.0, 1.0, 0.25, 68.0, 19.5, 1600.0, 170.0, 82.0, 80.0, 0.0]).contains("Dirt"));
        assert!(names(&[1.0, 0.0, 0.37, 28.5, 32.0, 7.4]).contains("Manual"));
        assert!(names(&[1.0, 1.0, 0.1667, 80.0, 8.0, 30.0, 3.0, 0.0, 0.0, 2.0, 0.0, 1.0]).contains("Wave"));
        assert!(names(&[1.0, 1.0, 0.125, 73.0, 20.0, 1.0, 0.0]).contains("Repeats"));
    }

    #[test]
    fn blue_echo_eighth_at_120() {
        let mut b = BlueEcho::new(48000.0);
        let mut l = vec![0.0; 20000];
        l[0] = 1.0;
        let mut r = l.clone();
        b.process(&mut l, &mut r);
        let at = (1..20000).max_by(|&a, &c| l[a].abs().partial_cmp(&l[c].abs()).unwrap()).unwrap();
        assert_eq!(at, 12000);
    }
}
