//! Alchemy's arpeggiator (Studio's own, after the guide): Mode (Off, Up, Down, Up/Down, Down/Up,
//! As Played, Random without repeats, Chord), Rate synced to the tempo or free, Octave, Length,
//! Pattern (16 step-sequencer patterns of up to 128 steps), Swing, Note Vel, Split, Latch (Off,
//! Hold, Add), Key Trigger (start on a grid of the song) with Cycle Reset. Each pattern runs
//! parallel sequences: Velocity (0 = rest), Tune (semitones), Pan, Swing, Length, Tie and the two
//! Arp Mod sequences the modulation section can use. It doesn't allocate while running.

use lg_dsp::{Param, Unit};

pub const ARP: u32 = 15000;
pub const MODE: u32 = ARP;
pub const RATE: u32 = ARP + 1;
pub const SYNC: u32 = ARP + 2;
pub const HZ: u32 = ARP + 3;
pub const OCTAVE: u32 = ARP + 4;
pub const LENGTH: u32 = ARP + 5;
pub const PATTERN: u32 = ARP + 6;
pub const SWING: u32 = ARP + 7;
pub const NOTE_VEL: u32 = ARP + 8;
pub const SPLIT: u32 = ARP + 9;
pub const LATCH: u32 = ARP + 10;
pub const KEY_TRIG: u32 = ARP + 11;
pub const CYCLE_RESET: u32 = ARP + 12;
pub const EDIT: u32 = ARP + 13;
pub const TARGET: u32 = ARP + 14;
pub const SNAP: u32 = ARP + 15;
/// Extended parameter: Arp Overlap Legacy Mode (overlapping repeated notes left out).
pub const OVERLAP_LEGACY: u32 = ARP + 16;
/// Pattern p: PAT + p × PAT_STRIDE + (0: steps; SEQ0 + sequence × 128 + step).
pub const PAT: u32 = 40000;
pub const PAT_STRIDE: u32 = 1040;
pub const SEQ0: u32 = 8;
pub const PATTERNS: usize = 16;
pub const STEPS: usize = 128;
/// The sequences in a pattern.
pub const SEQS: &[&str] = &["Velocity", "Tune", "Pan", "Swing", "Length", "Tie", "Arp Mod 1", "Arp Mod 2"];
pub const S_VEL: usize = 0;
pub const S_TUNE: usize = 1;
pub const S_PAN: usize = 2;
pub const S_SWING: usize = 3;
pub const S_LEN: usize = 4;
pub const S_TIE: usize = 5;
pub const S_MOD1: usize = 6;
pub const S_MOD2: usize = 7;

pub const MODES: &[&str] = &["Off", "Up", "Down", "Up/Down", "Down/Up", "As Played", "Random", "Chord"];
pub const RATES: &[&str] = &["1/1", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32", "1/32T"];
pub const RATE_BEATS: &[f32] = &[4.0, 2.0, 4.0 / 3.0, 1.0, 2.0 / 3.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125, 1.0 / 12.0];
pub const LATCHES: &[&str] = &["Off", "Hold", "Add"];
pub const KEY_TRIGGERS: &[&str] = &["Off", "1/16", "1/8", "1/4", "1/2", "1 Bar", "Snap to Rate"];
pub const EDIT_MODES: &[&str] = &["Step", "Multi"];
pub const SNAPS: &[&str] = &["Off", "1/2", "1/3", "1/4", "1/8", "1/16"];
pub const SNAP_N: &[u32] = &[0, 2, 3, 4, 8, 16];

pub fn pat(p: usize, f: u32) -> u32 {
    PAT + p as u32 * PAT_STRIDE + f
}
pub fn step(p: usize, seq: usize, k: usize) -> u32 {
    pat(p, SEQ0 + (seq * STEPS + k) as u32)
}
/// A sequence's range and default.
pub fn seq_range(seq: usize) -> (f32, f32, f32) {
    match seq {
        S_TUNE => (-24.0, 24.0, 0.0),
        S_PAN => (-1.0, 1.0, 0.0),
        S_SWING => (-0.5, 0.5, 0.0),
        S_TIE => (0.0, 1.0, 0.0),
        S_MOD1 | S_MOD2 => (0.0, 1.0, 0.0),
        _ => (0.0, 1.0, 1.0),
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn params() -> Vec<Param> {
    let mut v = vec![
        // knobs (modulation targets): Mode and Latch as numbers, named by value_text
        Param::new(MODE, "Arp Mode", 0.0, 7.0, 0.0, Unit::None),
        Param::new(RATE, "Arp Rate", 0.0, (RATES.len() - 1) as f32, 5.0, Unit::None),
        Param::new(SYNC, "Arp Sync", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(HZ, "Arp Free Rate", 0.5, 30.0, 4.0, Unit::Hz),
        Param::new(OCTAVE, "Arp Octave", 1.0, 4.0, 1.0, Unit::None),
        Param::new(LENGTH, "Arp Length", 0.0, 1.0, 0.8, Unit::Ratio),
        Param::new(PATTERN, "Arp Pattern", 1.0, 16.0, 1.0, Unit::None),
        Param::new(SWING, "Arp Swing", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(NOTE_VEL, "Arp Note Vel", 0.0, 1.0, 1.0, Unit::Ratio),
        Param::new(SPLIT, "Arp Split", 0.0, 127.0, 127.0, Unit::None),
        Param::new(LATCH, "Arp Latch", 0.0, 2.0, 0.0, Unit::None),
        Param::menu(KEY_TRIG, "Arp Key Trigger", KEY_TRIGGERS, 0.0),
        Param::new(CYCLE_RESET, "Arp Cycle Reset", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::menu(EDIT, "Arp Edit Mode", EDIT_MODES, 0.0).hide(),
        Param::menu(TARGET, "Arp Edit Target", SEQS, 0.0).hide(),
        Param::menu(SNAP, "Arp Value Snap", SNAPS, 0.0).hide(),
        Param::new(OVERLAP_LEGACY, "Arp Overlap Legacy Mode", 0.0, 1.0, 0.0, Unit::OnOff),
    ];
    for p in 0..PATTERNS {
        v.push(Param::new(pat(p, 0), leak(format!("Arp Pattern {} Steps", p + 1)), 1.0, STEPS as f32, 16.0, Unit::None).hide());
        for (s, n) in SEQS.iter().enumerate() {
            let (lo, hi, d) = seq_range(s);
            for k in 0..STEPS {
                v.push(Param::new(step(p, s, k), leak(format!("Arp {} {n} {}", p + 1, k + 1)), lo, hi, d, Unit::None).hide());
            }
        }
    }
    v
}

/// The value shown for a knob (Mode, Rate, Latch, Split by name).
pub fn value_text(id: u32, v: f32) -> Option<String> {
    let i = v.round().max(0.0) as usize;
    match id {
        MODE => MODES.get(i).map(|s| s.to_string()),
        RATE => RATES.get(i).map(|s| s.to_string()),
        LATCH => LATCHES.get(i).map(|s| s.to_string()),
        SPLIT => {
            const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
            Some(format!("{}{}", N[i % 12], i as i32 / 12 - 2))
        }
        OCTAVE | PATTERN => Some(format!("{i}")),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub struct PatCfg {
    pub steps: usize,
    pub s: [[f32; STEPS]; 8],
}

impl Default for PatCfg {
    fn default() -> Self {
        let mut s = [[0.0; STEPS]; 8];
        s[S_VEL] = [1.0; STEPS];
        s[S_LEN] = [1.0; STEPS];
        PatCfg { steps: 16, s }
    }
}

pub struct ArpCfg {
    pub mode: u8,
    pub rate: usize,
    pub sync: bool,
    pub hz: f32,
    pub octaves: u8,
    pub length: f32,
    pub pattern: usize,
    pub swing: f32,
    pub note_vel: f32,
    pub split: u8,
    pub latch: u8,
    pub key_trig: u8,
    pub cycle_reset: bool,
    /// Arp Overlap Legacy Mode: a key played again while still held isn't added again.
    pub overlap_legacy: bool,
    pub pats: Box<[PatCfg; PATTERNS]>,
}

impl Default for ArpCfg {
    fn default() -> Self {
        ArpCfg { mode: 0, rate: 5, sync: true, hz: 4.0, octaves: 1, length: 0.8, pattern: 0, swing: 0.0, note_vel: 1.0, split: 127, latch: 0, key_trig: 0, cycle_reset: true, overlap_legacy: false, pats: Box::new([PatCfg::default(); PATTERNS]) }
    }
}

impl ArpCfg {
    pub fn read_knobs(&mut self, p: &dyn Fn(u32) -> f32) {
        self.mode = p(MODE).round().clamp(0.0, 7.0) as u8;
        self.overlap_legacy = p(OVERLAP_LEGACY) >= 0.5;
        self.rate = (p(RATE).round().max(0.0) as usize).min(RATES.len() - 1);
        self.sync = p(SYNC) >= 0.5;
        self.hz = p(HZ);
        self.octaves = p(OCTAVE).round().clamp(1.0, 4.0) as u8;
        self.length = p(LENGTH);
        self.pattern = (p(PATTERN).round().clamp(1.0, 16.0) as usize) - 1;
        self.swing = p(SWING);
        self.note_vel = p(NOTE_VEL);
        self.split = p(SPLIT).round().clamp(0.0, 127.0) as u8;
        self.latch = p(LATCH).round().clamp(0.0, 2.0) as u8;
        self.key_trig = p(KEY_TRIG).round().max(0.0) as u8;
        self.cycle_reset = p(CYCLE_RESET) >= 0.5;
    }
    pub fn read_pattern(&mut self, pi: usize, p: &dyn Fn(u32) -> f32) {
        let c = &mut self.pats[pi];
        c.steps = (p(pat(pi, 0)).round() as usize).clamp(1, STEPS);
        for s in 0..8 {
            for k in 0..STEPS {
                c.s[s][k] = p(step(pi, s, k));
            }
        }
    }
    pub fn read_all(&mut self, p: &dyn Fn(u32) -> f32) {
        self.read_knobs(p);
        for pi in 0..PATTERNS {
            self.read_pattern(pi, p);
        }
    }
    /// Re-reads what a parameter belongs to; false if it isn't the arpeggiator's.
    pub fn read_changed(&mut self, id: u32, p: &dyn Fn(u32) -> f32) -> bool {
        if (ARP..=OVERLAP_LEGACY).contains(&id) {
            self.read_knobs(p);
            true
        } else if (PAT..PAT + PAT_STRIDE * PATTERNS as u32).contains(&id) {
            self.read_pattern(((id - PAT) / PAT_STRIDE) as usize, p);
            true
        } else {
            false
        }
    }
    /// The step duration in seconds.
    pub fn step_secs(&self, bpm: f32) -> f32 {
        if self.sync { RATE_BEATS[self.rate] * 60.0 / bpm.max(1.0) } else { 1.0 / self.hz.max(0.1) }
    }
}

/// A note the arpeggiator starts: pitch, velocity, pan, the Arp Mod values, step, steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArpNote {
    pub pitch: u8,
    pub vel: u8,
    pub pan: f32,
    pub m1: f32,
    pub m2: f32,
    pub step: usize,
    pub steps: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArpEv {
    On(ArpNote),
    Off(u8),
}

pub struct Arp {
    held: [(u8, u8); STEPS],
    nheld: usize,
    order: [(u8, u8); STEPS],
    norder: usize,
    down: usize,
    pub running: bool,
    /// Seconds since the pattern started, and when the next step is due.
    t: f32,
    next: f32,
    pub step: usize,
    index: usize,
    bag: [u8; STEPS],
    nbag: usize,
    last_rand: i32,
    sounding: [(u8, f32); 16],
    nsounding: usize,
    seed: u32,
    /// Waiting for the Key Trigger grid: seconds left.
    wait: f32,
    tied_pitch: i32,
}

impl Default for Arp {
    fn default() -> Self {
        Arp { held: [(0, 0); STEPS], nheld: 0, order: [(0, 0); STEPS], norder: 0, down: 0, running: false, t: 0.0, next: 0.0, step: 0, index: 0, bag: [0; STEPS], nbag: 0, last_rand: -1, sounding: [(0, 0.0); 16], nsounding: 0, seed: 0x2545_F491, wait: 0.0, tied_pitch: -1 }
    }
}

impl Arp {
    fn rand(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }
    pub fn held(&self) -> usize {
        self.nheld
    }
    /// A key: false when it's above Split (the caller plays it directly).
    pub fn note_on(&mut self, c: &ArpCfg, pitch: u8, vel: u8, song_beats: f64, bpm: f32, playing: bool) -> bool {
        if pitch > c.split {
            return false;
        }
        if c.latch == 1 && self.down == 0 {
            // Hold: a new chord replaces the latched one
            self.nheld = 0;
            self.norder = 0;
        }
        self.down += 1;
        let fresh = self.nheld == 0;
        let add = c.latch == 2;
        // a key played again while it's still held: in the arpeggio twice, unless Arp Overlap
        // Legacy Mode leaves it out
        let again = self.held[..self.nheld].iter().any(|h| h.0 == pitch);
        if again && c.overlap_legacy && !add {
            return true;
        }
        if self.nheld < STEPS {
            self.held[self.nheld] = (pitch, vel);
            self.nheld += 1;
            self.held[..self.nheld].sort_by_key(|h| h.0);
        }
        if self.norder < STEPS {
            self.order[self.norder] = (pitch, vel);
            self.norder += 1;
        }
        if fresh && (!self.running || c.key_trig != 0) {
            // the pattern starts (on the Key Trigger grid when the song plays); legato keys don't restart it
            self.running = true;
            self.t = 0.0;
            self.next = 0.0;
            self.step = 0;
            self.index = 0;
            self.nbag = 0;
            self.wait = 0.0;
            if c.key_trig != 0 && playing {
                let grid = match c.key_trig {
                    1 => 0.25,
                    2 => 0.5,
                    3 => 1.0,
                    4 => 2.0,
                    5 => 4.0,
                    _ => RATE_BEATS[c.rate] as f64,
                };
                let to = (grid - song_beats.rem_euclid(grid)) % grid;
                self.wait = (to * 60.0 / bpm.max(1.0) as f64) as f32;
            }
        }
        true
    }
    pub fn note_off(&mut self, c: &ArpCfg, pitch: u8) -> bool {
        if pitch > c.split {
            return false;
        }
        self.down = self.down.saturating_sub(1);
        if c.latch != 0 {
            return true;
        }
        if let Some(i) = self.held[..self.nheld].iter().position(|h| h.0 == pitch) {
            self.held.copy_within(i + 1..self.nheld, i);
            self.nheld -= 1;
        }
        if let Some(i) = self.order[..self.norder].iter().position(|h| h.0 == pitch) {
            self.order.copy_within(i + 1..self.norder, i);
            self.norder -= 1;
        }
        true
    }
    /// Lets go of everything (transport stop, mode Off, reset).
    pub fn clear(&mut self, ev: &mut [ArpEv; 32]) -> usize {
        let mut n = 0;
        for i in 0..self.nsounding {
            ev[n] = ArpEv::Off(self.sounding[i].0);
            n += 1;
        }
        self.nsounding = 0;
        self.nheld = 0;
        self.norder = 0;
        self.down = 0;
        self.running = false;
        n
    }
    /// The pattern's notes (held keys over the octaves) in the mode's base order.
    fn notes(&self, c: &ArpCfg, out: &mut [(u8, u8); STEPS]) -> usize {
        let (src, n) = if c.mode == 5 { (&self.order, self.norder) } else { (&self.held, self.nheld) };
        let mut k = 0;
        for o in 0..c.octaves.max(1) {
            for &(p, v) in &src[..n] {
                let q = p as i32 + 12 * o as i32;
                if q <= 127 && k < STEPS {
                    out[k] = (q as u8, v);
                    k += 1;
                }
            }
        }
        k
    }
    /// Advances `dt` seconds; events land in `ev` (returns how many).
    pub fn advance(&mut self, c: &ArpCfg, dt: f32, bpm: f32, ev: &mut [ArpEv; 32]) -> usize {
        let mut n = 0;
        let mut push = |e: ArpEv, n: &mut usize| {
            if *n < 32 {
                ev[*n] = e;
                *n += 1;
            }
        };
        let end = self.t + dt;
        // notes ending in this block (a tied note keeps going)
        let mut i = 0;
        while i < self.nsounding {
            if self.sounding[i].1 <= end {
                push(ArpEv::Off(self.sounding[i].0), &mut n);
                self.sounding[i] = self.sounding[self.nsounding - 1];
                self.nsounding -= 1;
            } else {
                i += 1;
            }
        }
        if c.mode == 0 || self.nheld == 0 {
            if self.nheld == 0 {
                self.running = false;
            }
            self.t = end;
            return n;
        }
        if self.wait > 0.0 {
            let w = self.wait.min(dt);
            self.wait -= w;
            self.t += w;
            self.next = self.t;
            if self.wait > 0.0 {
                return n;
            }
        }
        let pc = &c.pats[c.pattern.min(PATTERNS - 1)];
        let steps = pc.steps.clamp(1, STEPS);
        let sl = c.step_secs(bpm);
        loop {
            let k = self.step % steps;
            // Swing knob delays the even-numbered steps; the Swing sequence moves any step
            let sw = if k % 2 == 1 { c.swing * 0.5 } else { 0.0 } + pc.s[S_SWING][k];
            let at = self.next + sl * sw.clamp(-0.5, 0.95);
            if at >= end {
                break;
            }
            if k == 0 && self.step > 0 && c.cycle_reset {
                self.index = 0;
            }
            self.next += sl;
            self.step += 1;
            let vstep = pc.s[S_VEL][k];
            let lenq = pc.s[S_LEN][k] * c.length;
            let tie_prev = k > 0 && pc.s[S_TIE][k - 1] >= 0.5 || (k == 0 && pc.s[S_TIE][steps - 1] >= 0.5 && self.step > 1);
            if (vstep <= 0.0 || lenq <= 0.0) && !tie_prev {
                // a rest
                continue;
            }
            let mut notes = [(0u8, 0u8); STEPS];
            let len = self.notes(c, &mut notes);
            if len == 0 {
                break;
            }
            let tied = pc.s[S_TIE][k] >= 0.5;
            // a tied note runs into the next step
            let off_at = at + sl * if tied { 1.05 } else { lenq.clamp(0.02, 1.0) };
            let tune = pc.s[S_TUNE][k].round() as i32;
            let make = |p: u8, v: u8| -> ArpNote {
                // Note Vel: 0 % the step's velocity, 100 % the played one, between a blend
                let vel = (vstep.max(0.05) * 127.0 * (1.0 - c.note_vel) + v as f32 * c.note_vel).round().clamp(1.0, 127.0) as u8;
                ArpNote { pitch: (p as i32 + tune).clamp(0, 127) as u8, vel, pan: pc.s[S_PAN][k], m1: pc.s[S_MOD1][k], m2: pc.s[S_MOD2][k], step: k, steps }
            };
            if c.mode == 7 {
                for &(p, v) in notes.iter().take(len.min(12)) {
                    let note = make(p, v);
                    push(ArpEv::On(note), &mut n);
                    if self.nsounding < 16 {
                        self.sounding[self.nsounding] = (note.pitch, off_at);
                        self.nsounding += 1;
                    }
                }
                continue;
            }
            let i = match c.mode {
                2 => len - 1 - (self.index % len),
                3 => {
                    let period = if len > 1 { 2 * len - 2 } else { 1 };
                    let x = self.index % period;
                    if x < len { x } else { period - x }
                }
                4 => {
                    let period = if len > 1 { 2 * len - 2 } else { 1 };
                    let x = self.index % period;
                    len - 1 - if x < len { x } else { period - x }
                }
                6 => {
                    // random without repeats: a shuffled bag, refilled when empty
                    if self.nbag == 0 {
                        for j in 0..len {
                            self.bag[j] = j as u8;
                        }
                        self.nbag = len;
                    }
                    let mut j = (self.rand() as usize) % self.nbag;
                    if self.nbag > 1 && self.bag[j] as i32 == self.last_rand {
                        j = (j + 1) % self.nbag;
                    }
                    let pick = self.bag[j] as usize;
                    self.bag[j] = self.bag[self.nbag - 1];
                    self.nbag -= 1;
                    self.last_rand = pick as i32;
                    pick.min(len - 1)
                }
                _ => self.index % len,
            };
            self.index += 1;
            let note = make(notes[i].0, notes[i].1);
            if tie_prev && self.tied_pitch == note.pitch as i32 {
                // the tie carries on: the sounding note just gets longer
                if let Some(s) = self.sounding[..self.nsounding].iter_mut().find(|s| s.0 == note.pitch) {
                    s.1 = off_at;
                }
            } else {
                push(ArpEv::On(note), &mut n);
                if self.nsounding < 16 {
                    self.sounding[self.nsounding] = (note.pitch, off_at);
                    self.nsounding += 1;
                }
            }
            self.tied_pitch = if tied { note.pitch as i32 } else { -1 };
        }
        self.t = end;
        n
    }
}

/// Import MIDI: a monophonic pattern's notes (start tick, pitch, velocity) at `ppq` ticks per
/// beat onto a pattern's Tune (from the first note), Velocity and Swing sequences.
pub fn import_midi(notes: &[(u32, u8, u8)], ppq: u32, step_beats: f32) -> Vec<(usize, usize, f32)> {
    let st = (step_beats * ppq as f32).max(1.0);
    let Some(last) = notes.iter().map(|n| n.0).max() else { return Vec::new() };
    let len = (((last as f32 / st).round() as usize) + 1).min(STEPS);
    let root = notes.first().map(|n| n.1).unwrap_or(60) as i32;
    let mut v = vec![(usize::MAX, 0, len as f32)];
    for k in 0..len {
        let at = k as f32 * st;
        let near = notes.iter().filter(|n| (n.0 as f32 - at).abs() <= st * 0.5).min_by(|a, b| (a.0 as f32 - at).abs().total_cmp(&(b.0 as f32 - at).abs()));
        match near {
            Some(n) => {
                v.push((S_VEL, k, n.2 as f32 / 127.0));
                v.push((S_TUNE, k, (n.1 as i32 - root).clamp(-24, 24) as f32));
                v.push((S_SWING, k, ((n.0 as f32 - at) / st).clamp(-0.5, 0.5)));
            }
            None => v.push((S_VEL, k, 0.0)),
        }
    }
    v
}
