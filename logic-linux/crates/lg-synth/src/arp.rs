//! Building blocks for instruments with a performance section (Alchemy's Arpeggiator page, its
//! step sequencer and MSEG modulators): an arpeggiator that turns held keys into a tempo-synced
//! note pattern (with a 16-step sequencer for which steps play, their velocity and length), and
//! a multi-segment envelope generator. Our own code; they don't allocate while running.

/// Arpeggiator modes (Alchemy's).
pub const ARP_MODES: &[&str] = &["Up", "Down", "Up/Down", "Up & Down", "As Played", "Random", "Chord"];
/// Arpeggiator rates: (name, beats per step).
pub const ARP_RATES: &[(&str, f64)] = &[
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/2T", 4.0 / 3.0),
    ("1/4", 1.0),
    ("1/4T", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/8T", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/16T", 1.0 / 6.0),
    ("1/32", 0.125),
    ("1/32T", 1.0 / 12.0),
];
pub const STEPS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct ArpSettings {
    pub on: bool,
    pub mode: u8,
    pub octaves: u8,
    pub beats: f64,
    /// Note length as a part of the step (0..1).
    pub gate: f32,
    /// Swing: every second step later (0..1 of half a step).
    pub swing: f32,
    pub latch: bool,
    /// The sequencer: steps used, each on / off, its velocity (0..1) and length (0..1).
    pub steps: usize,
    pub step_on: [bool; STEPS],
    pub step_vel: [f32; STEPS],
    pub step_len: [f32; STEPS],
    /// Use the step's velocity (else the played one).
    pub seq_vel: bool,
}

impl Default for ArpSettings {
    fn default() -> Self {
        ArpSettings { on: false, mode: 0, octaves: 1, beats: 0.25, gate: 0.5, swing: 0.0, latch: false, steps: 16, step_on: [true; STEPS], step_vel: [1.0; STEPS], step_len: [1.0; STEPS], seq_vel: false }
    }
}

/// What the arpeggiator wants played now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpEvent {
    On(u8, u8),
    Off(u8),
}

pub struct Arp {
    held: [(u8, u8); 32],
    nheld: usize,
    /// The keys in the order they were played.
    order: [u8; 32],
    norder: usize,
    /// Keys still down (latch keeps the pattern after they're released).
    down: usize,
    beat: f64,
    step: usize,
    index: usize,
    dir: i32,
    sounding: [u8; 8],
    nsounding: usize,
    off_at: f64,
    seed: u32,
    started: bool,
}

impl Default for Arp {
    fn default() -> Self {
        Arp { held: [(0, 0); 32], nheld: 0, order: [0; 32], norder: 0, down: 0, beat: 0.0, step: 0, index: 0, dir: 1, sounding: [0; 8], nsounding: 0, off_at: 0.0, seed: 0x1234_5678, started: false }
    }
}

impl Arp {
    pub fn note_on(&mut self, s: &ArpSettings, pitch: u8, vel: u8) {
        if s.latch && self.down == 0 {
            // a new chord after letting go: the latched one is replaced
            self.nheld = 0;
            self.norder = 0;
        }
        self.down += 1;
        if self.nheld < self.held.len() && !self.held[..self.nheld].iter().any(|h| h.0 == pitch) {
            self.held[self.nheld] = (pitch, vel);
            self.nheld += 1;
            self.held[..self.nheld].sort_unstable_by_key(|h| h.0);
            self.order[self.norder.min(31)] = pitch;
            self.norder = (self.norder + 1).min(32);
        }
        if !self.started {
            // the pattern starts on the first key
            self.beat = 0.0;
            self.step = 0;
            self.index = 0;
            self.dir = 1;
            self.off_at = 0.0;
            self.started = true;
        }
    }
    pub fn note_off(&mut self, s: &ArpSettings, pitch: u8) {
        self.down = self.down.saturating_sub(1);
        if s.latch {
            return;
        }
        if let Some(i) = self.held[..self.nheld].iter().position(|h| h.0 == pitch) {
            self.held.copy_within(i + 1..self.nheld, i);
            self.nheld -= 1;
        }
        if let Some(i) = self.order[..self.norder].iter().position(|&p| p == pitch) {
            self.order.copy_within(i + 1..self.norder, i);
            self.norder -= 1;
        }
    }
    pub fn clear(&mut self) {
        self.nheld = 0;
        self.norder = 0;
        self.down = 0;
        self.started = false;
    }
    pub fn active(&self) -> bool {
        self.nheld > 0 || self.nsounding > 0
    }
    fn rand(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }
    /// The notes of the pattern (keys × octaves), in the mode's order.
    fn pattern(&self, s: &ArpSettings, out: &mut [u8; 128]) -> usize {
        let src: &[u8] = &if s.mode == 4 { self.order } else { self.held.map(|h| h.0) };
        let n = if s.mode == 4 { self.norder } else { self.nheld };
        let mut k = 0;
        for o in 0..s.octaves.max(1) {
            for &p in &src[..n] {
                let q = p as i32 + 12 * o as i32;
                if q <= 127 && k < 128 {
                    out[k] = q as u8;
                    k += 1;
                }
            }
        }
        k
    }
    /// Advances `dt` beats; events land in `ev` (returns how many).
    pub fn advance(&mut self, s: &ArpSettings, dt: f64, ev: &mut [ArpEvent; 16]) -> usize {
        let mut n = 0;
        let push = |e: ArpEvent, n: &mut usize, ev: &mut [ArpEvent; 16]| {
            if *n < ev.len() {
                ev[*n] = e;
                *n += 1;
            }
        };
        let end = self.beat + dt;
        // notes ending in this block
        if self.nsounding > 0 && self.off_at <= end {
            for i in 0..self.nsounding {
                push(ArpEvent::Off(self.sounding[i]), &mut n, ev);
            }
            self.nsounding = 0;
        }
        if self.nheld == 0 {
            self.started = false;
            self.beat = end;
            return n;
        }
        let step_len = s.beats.max(1e-3);
        loop {
            // the next step's time (swing delays the odd steps)
            let base = self.step as f64 * step_len;
            let swing = if self.step % 2 == 1 { s.swing as f64 * step_len * 0.5 } else { 0.0 };
            let at = base + swing;
            if at >= end {
                break;
            }
            let k = self.step % s.steps.clamp(1, STEPS);
            self.step += 1;
            if at < self.beat - 1e-9 {
                continue;
            }
            if self.nsounding > 0 {
                for i in 0..self.nsounding {
                    push(ArpEvent::Off(self.sounding[i]), &mut n, ev);
                }
                self.nsounding = 0;
            }
            if !s.step_on[k] {
                continue;
            }
            let mut pat = [0u8; 128];
            let len = self.pattern(s, &mut pat);
            if len == 0 {
                break;
            }
            let played_vel = self.held[..self.nheld].iter().map(|h| h.1).max().unwrap_or(100);
            let vel = if s.seq_vel { (s.step_vel[k] * 127.0).round().clamp(1.0, 127.0) as u8 } else { ((played_vel as f32) * s.step_vel[k]).round().clamp(1.0, 127.0) as u8 };
            if s.mode == 6 {
                // Chord: every held key at once
                for &p in pat.iter().take(len.min(8)) {
                    push(ArpEvent::On(p, vel), &mut n, ev);
                    self.sounding[self.nsounding] = p;
                    self.nsounding += 1;
                }
            } else {
                let i = match s.mode {
                    1 => len - 1 - (self.index % len),
                    2 => {
                        // up then down, the ends once
                        let period = if len > 1 { 2 * len - 2 } else { 1 };
                        let x = self.index % period;
                        if x < len { x } else { period - x }
                    }
                    3 => {
                        // up and down, the ends twice
                        let x = self.index % (2 * len);
                        if x < len { x } else { 2 * len - 1 - x }
                    }
                    5 => (self.rand() as usize) % len,
                    _ => self.index % len,
                };
                self.index += 1;
                let p = pat[i];
                push(ArpEvent::On(p, vel), &mut n, ev);
                self.sounding[0] = p;
                self.nsounding = 1;
            }
            self.off_at = at + step_len * (s.gate * s.step_len[k]).clamp(0.05, 1.0) as f64;
            let _ = self.dir;
        }
        self.beat = end;
        n
    }
}

/// A multi-segment envelope: up to 16 points (time in beats or seconds from the previous, level
/// −1..1), optionally looping between two points while the key is held.
#[derive(Clone, Copy, Debug)]
pub struct MsegShape {
    pub points: [(f32, f32); 16],
    pub n: usize,
    pub loop_from: Option<usize>,
    pub loop_to: Option<usize>,
}

impl Default for MsegShape {
    fn default() -> Self {
        let mut points = [(0.0, 0.0); 16];
        points[0] = (0.0, 0.0);
        points[1] = (0.1, 1.0);
        points[2] = (0.4, 0.3);
        points[3] = (0.6, 0.0);
        MsegShape { points, n: 4, loop_from: None, loop_to: None }
    }
}

impl MsegShape {
    /// The level `t` seconds after the start (held: the loop runs).
    pub fn level(&self, t: f32, held: bool) -> f32 {
        let n = self.n.clamp(1, 16);
        let mut start = 0.0;
        let total: f32 = self.points[1..n].iter().map(|p| p.0.max(0.0)).sum();
        let mut t = t;
        if held
            && let (Some(a), Some(b)) = (self.loop_from, self.loop_to)
            && b > a
            && b < n
        {
            let ta: f32 = self.points[1..=a].iter().map(|p| p.0.max(0.0)).sum();
            let tb: f32 = self.points[1..=b].iter().map(|p| p.0.max(0.0)).sum();
            if t > tb && tb > ta {
                t = ta + (t - ta) % (tb - ta);
            }
        }
        if t >= total {
            return self.points[n - 1].1;
        }
        for k in 1..n {
            let d = self.points[k].0.max(0.0);
            if t <= start + d {
                let f = if d > 0.0 { (t - start) / d } else { 1.0 };
                return self.points[k - 1].1 + (self.points[k].1 - self.points[k - 1].1) * f;
            }
            start += d;
        }
        self.points[n - 1].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &ArpSettings, a: &mut Arp, beats: f64) -> Vec<ArpEvent> {
        let mut out = Vec::new();
        let mut ev = [ArpEvent::Off(0); 16];
        let mut b = 0.0;
        while b < beats {
            let n = a.advance(s, 0.01, &mut ev);
            out.extend_from_slice(&ev[..n]);
            b += 0.01;
        }
        out
    }

    fn ons(v: &[ArpEvent]) -> Vec<u8> {
        v.iter().filter_map(|e| if let ArpEvent::On(p, _) = e { Some(*p) } else { None }).collect()
    }

    #[test]
    fn modes_order_the_held_keys() {
        let mut s = ArpSettings { on: true, ..Default::default() };
        let mut a = Arp::default();
        for p in [64, 60, 67] {
            a.note_on(&s, p, 100);
        }
        assert_eq!(ons(&run(&s, &mut a, 1.45)), [60, 64, 67, 60, 64, 67], "Up, 1/16 at one beat and a half");
        s.mode = 1;
        let mut a = Arp::default();
        for p in [64, 60, 67] {
            a.note_on(&s, p, 100);
        }
        assert_eq!(&ons(&run(&s, &mut a, 0.9))[..3], [67, 64, 60]);
        s.mode = 2;
        let mut a = Arp::default();
        for p in [60, 64, 67] {
            a.note_on(&s, p, 100);
        }
        assert_eq!(&ons(&run(&s, &mut a, 1.4))[..5], [60, 64, 67, 64, 60]);
        s.mode = 4;
        let mut a = Arp::default();
        for p in [67, 60, 64] {
            a.note_on(&s, p, 100);
        }
        assert_eq!(&ons(&run(&s, &mut a, 0.7))[..3], [67, 60, 64], "As Played");
        s.mode = 0;
        s.octaves = 2;
        let mut a = Arp::default();
        a.note_on(&s, 60, 100);
        assert_eq!(&ons(&run(&s, &mut a, 0.5))[..2], [60, 72]);
    }

    #[test]
    fn steps_gate_and_latch() {
        let mut s = ArpSettings { on: true, steps: 4, ..Default::default() };
        s.step_on[1] = false;
        let mut a = Arp::default();
        a.note_on(&s, 60, 100);
        let ev = run(&s, &mut a, 0.99);
        assert_eq!(ons(&ev).len(), 3, "step 2 is off");
        assert!(ev.iter().any(|e| matches!(e, ArpEvent::Off(60))), "the gate ends notes");
        s.latch = true;
        let mut a = Arp::default();
        a.note_on(&s, 62, 100);
        a.note_off(&s, 62);
        assert!(!ons(&run(&s, &mut a, 0.5)).is_empty(), "latched keys keep playing");
    }

    #[test]
    fn mseg_levels_and_loop() {
        let m = MsegShape::default();
        assert!((m.level(0.1, false) - 1.0).abs() < 1e-6);
        assert!((m.level(5.0, false)).abs() < 1e-6);
        let l = MsegShape { loop_from: Some(1), loop_to: Some(2), ..Default::default() };
        let a = l.level(0.2, true);
        let b = l.level(0.2 + 0.4 * 3.0, true);
        assert!((a - b).abs() < 1e-4, "the loop repeats: {a} {b}");
    }
}
