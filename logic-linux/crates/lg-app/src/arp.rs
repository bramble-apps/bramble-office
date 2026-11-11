//! The Arpeggiator (Logic's MIDI FX, switched on from the Smart Controls header): the notes
//! held are played one after another in a note order (Up, Down, Up & Down, Outside In, Random,
//! As Played, or the whole Chord repeated) at a rate, over 1 to 4 octaves, with Latch modes
//! (Reflex: plays while keys are held; Latch: keeps playing after release until a new chord;
//! Latch Transpose: a new single key transposes the latched notes).
//!
//! Two paths use the same note order: the regions' notes are arpeggiated when the engine's
//! notes are built (playback), and keys played live go through a running arpeggiator in the
//! engine (`Live`), stepping on the song's grid while playing and at the project tempo when
//! stopped.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    Up,
    Down,
    UpDown,
    OutsideIn,
    Random,
    AsPlayed,
    Chord,
}

impl Order {
    pub const ALL: [Order; 7] = [Order::Up, Order::Down, Order::UpDown, Order::OutsideIn, Order::Random, Order::AsPlayed, Order::Chord];
    pub fn name(self) -> &'static str {
        match self {
            Order::Up => "Up",
            Order::Down => "Down",
            Order::UpDown => "Up & Down",
            Order::OutsideIn => "Outside In",
            Order::Random => "Random",
            Order::AsPlayed => "As Played",
            Order::Chord => "Chord",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Latch {
    /// Plays while keys are held.
    #[default]
    Reflex,
    /// Keeps playing after the keys are released, until new keys are pressed.
    Latch,
    /// As Latch; a single new key transposes the latched notes.
    Transpose,
}

impl Latch {
    pub const ALL: [Latch; 3] = [Latch::Reflex, Latch::Latch, Latch::Transpose];
    pub fn name(self) -> &'static str {
        match self {
            Latch::Reflex => "Reflex",
            Latch::Latch => "Latch",
            Latch::Transpose => "Latch Transpose",
        }
    }
}

/// Rates: name and length in beats (quarter notes).
pub const RATES: &[(&str, f64)] = &[
    ("1/1", 4.0), ("1/2", 2.0), ("1/2T", 4.0 / 3.0), ("1/4.", 1.5), ("1/4", 1.0), ("1/4T", 2.0 / 3.0), ("1/8.", 0.75), ("1/8", 0.5), ("1/8T", 1.0 / 3.0), ("1/16.", 0.375), ("1/16", 0.25), ("1/16T", 1.0 / 6.0), ("1/32", 0.125), ("1/32T", 1.0 / 12.0),
];

/// The default rate (1/16).
pub const DEFAULT_RATE: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub on: bool,
    pub order: Order,
    /// Index in RATES.
    pub rate: usize,
    /// Octave range, 1 to 4.
    pub octaves: u8,
    pub latch: Latch,
    /// Note length as a share of a step.
    pub gate: f32,
    /// A fixed velocity (None: as played).
    pub velocity: Option<u8>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { on: false, order: Order::Up, rate: DEFAULT_RATE, octaves: 1, latch: Latch::Reflex, gate: 0.5, velocity: None }
    }
}

impl Settings {
    pub fn step_beats(&self) -> f64 {
        RATES.get(self.rate).map_or(0.25, |r| r.1)
    }
}

/// The notes, expanded over the octave range, in the order's base sequence (one entry per
/// step; Chord: every step all notes). `held` is in the order played.
pub fn sequence(held: &[(u8, u8)], s: &Settings) -> Vec<Vec<(u8, u8)>> {
    if held.is_empty() {
        return Vec::new();
    }
    let oct = s.octaves.clamp(1, 4);
    let up = |src: &[(u8, u8)]| -> Vec<(u8, u8)> {
        let mut v = Vec::new();
        for o in 0..oct {
            for &(p, vel) in src {
                let q = p as u16 + 12 * o as u16;
                if q <= 127 {
                    v.push((q as u8, vel));
                }
            }
        }
        v
    };
    let mut sorted = held.to_vec();
    sorted.sort_by_key(|n| n.0);
    sorted.dedup_by_key(|n| n.0);
    let one = |v: Vec<(u8, u8)>| v.into_iter().map(|n| vec![n]).collect::<Vec<_>>();
    match s.order {
        Order::Up | Order::Random => one(up(&sorted)),
        Order::Down => {
            let mut v = up(&sorted);
            v.reverse();
            one(v)
        }
        Order::UpDown => {
            let v = up(&sorted);
            let mut out = v.clone();
            if v.len() > 2 {
                out.extend(v[1..v.len() - 1].iter().rev().copied());
            } else if v.len() == 2 {
                // (two notes: up and down is the same as up)
            }
            one(out)
        }
        Order::OutsideIn => {
            let v = up(&sorted);
            let (mut lo, mut hi) = (0usize, v.len());
            let mut out = Vec::new();
            let mut top = true;
            while lo < hi {
                if top {
                    hi -= 1;
                    out.push(v[hi]);
                } else {
                    out.push(v[lo]);
                    lo += 1;
                }
                top = !top;
            }
            one(out)
        }
        Order::AsPlayed => one(up(held)),
        Order::Chord => (0..oct).map(|o| sorted.iter().filter_map(|&(p, v)| (p as u16 + 12 * o as u16 <= 127).then(|| (p + 12 * o, v))).collect()).collect(),
    }
}

/// The notes of step `k` (k counts from the chord's first step).
pub fn step_notes(held: &[(u8, u8)], s: &Settings, k: u64) -> Vec<(u8, u8)> {
    let seq = sequence(held, s);
    if seq.is_empty() {
        return Vec::new();
    }
    let i = if s.order == Order::Random {
        // (the same steps each time: a fixed scramble)
        let mut x = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        x ^= x >> 33;
        (x % seq.len() as u64) as usize
    } else {
        (k % seq.len() as u64) as usize
    };
    seq[i].iter().map(|&(p, v)| (p, s.velocity.unwrap_or(v))).collect()
}

/// Latch: what sounds once `held` (the keys down now) changes, given what was latched. Returns
/// the notes to arpeggiate and whether the step count restarts.
fn latched(prev: &[(u8, u8)], held: &[(u8, u8)], was_empty: bool, base: &mut Option<u8>, s: &Settings) -> (Vec<(u8, u8)>, bool) {
    match s.latch {
        Latch::Reflex => (held.to_vec(), was_empty),
        Latch::Latch => {
            if held.is_empty() {
                (prev.to_vec(), false)
            } else if was_empty {
                (held.to_vec(), true)
            } else {
                (held.to_vec(), false)
            }
        }
        Latch::Transpose => {
            if held.is_empty() {
                (prev.to_vec(), false)
            } else if was_empty && held.len() == 1 && !prev.is_empty() {
                // one key: the latched notes move by its distance from the chord's lowest note
                let low = base.unwrap_or_else(|| prev.iter().map(|n| n.0).min().unwrap_or(60));
                let d = held[0].0 as i16 - low as i16;
                let moved = prev.iter().map(|&(p, v)| ((p as i16 + d).clamp(0, 127) as u8, v)).collect::<Vec<_>>();
                *base = Some(held[0].0);
                (moved, false)
            } else {
                *base = held.iter().map(|n| n.0).min();
                (held.to_vec(), was_empty)
            }
        }
    }
}

/// Arpeggiates notes for playback: (start, end, pitch, velocity) in song ticks → the notes the
/// arpeggiator plays, on the song's step grid, up to `until` (latched notes run to there).
pub fn arpeggiate(notes: &[(u32, u32, u8, u8)], s: &Settings, ppq: u32, until: u32) -> Vec<(u32, u32, u8, u8)> {
    if notes.is_empty() {
        return Vec::new();
    }
    let step = (s.step_beats() * ppq as f64).max(1.0);
    let first = notes.iter().map(|n| n.0).min().unwrap_or(0);
    let last_end = notes.iter().map(|n| n.1).max().unwrap_or(0);
    let stop = if s.latch == Latch::Reflex { last_end } else { until.max(last_end) };
    let mut out = Vec::new();
    let mut k0 = (first as f64 / step).floor() as u64;
    // (a note starting just after a grid point plays from that point)
    if (k0 as f64 * step) < first as f64 - step * 0.1 {
        k0 += 1;
    }
    let mut count = 0u64;
    let mut prev_held: Vec<(u8, u8)> = Vec::new();
    let mut sounding: Vec<(u8, u8)> = Vec::new();
    let mut base = None;
    let mut k = k0;
    loop {
        let t = k as f64 * step;
        if t >= stop as f64 {
            break;
        }
        // held at this step: started by it (a little early counts) and not yet ended
        let tol = step * 0.1;
        let mut held: Vec<(u32, u8, u8)> = notes.iter().filter(|n| (n.0 as f64) <= t + tol && (n.1 as f64) > t + tol).map(|n| (n.0, n.2, n.3)).collect();
        held.sort_by_key(|n| (n.0, n.1));
        let held: Vec<(u8, u8)> = held.into_iter().map(|n| (n.1, n.2)).collect();
        let was_empty = prev_held.is_empty();
        let (now, restart) = latched(&sounding, &held, was_empty, &mut base, s);
        if restart {
            count = 0;
        }
        sounding = now;
        prev_held = held;
        if sounding.is_empty() {
            count = 0;
        } else {
            let len = (step * s.gate.clamp(0.05, 1.0) as f64).max(1.0) as u32;
            for (p, v) in step_notes(&sounding, s, count) {
                out.push((t as u32, t as u32 + len, p, v));
            }
            count += 1;
        }
        k += 1;
        if out.len() > 200_000 {
            break;
        }
    }
    out
}

/// A live arpeggiator (one per track) in the engine.
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub settings: Settings,
    /// Keys down, in the order pressed.
    held: Vec<(u8, u8)>,
    /// What the arpeggiator plays (held, or latched).
    playing: Vec<(u8, u8)>,
    base: Option<u8>,
    count: u64,
    /// Free-running position (in steps) while stopped.
    phase: f64,
    /// Notes sounding: pitch, frames left.
    sounding: Vec<(u8, f64)>,
    /// A chord just begun: its first step plays in the next block.
    start_now: bool,
}

impl Live {
    /// A key pressed (velocity > 0) or released.
    pub fn key(&mut self, pitch: u8, velocity: u8) {
        let was_empty = self.held.is_empty();
        if velocity > 0 {
            self.held.retain(|n| n.0 != pitch);
            self.held.push((pitch, velocity));
        } else {
            self.held.retain(|n| n.0 != pitch);
        }
        let (now, restart) = latched(&self.playing, &self.held, was_empty && velocity > 0, &mut self.base, &self.settings);
        if restart {
            self.count = 0;
            // (a new chord from silence starts at once)
            self.start_now = true;
        }
        self.playing = now;
        if self.playing.is_empty() {
            self.count = 0;
        }
    }

    /// Stops everything (Latch released by switching the arpeggiator off).
    pub fn reset(&mut self) -> Vec<(u8, u8)> {
        self.held.clear();
        self.playing.clear();
        self.count = 0;
        self.sounding.drain(..).map(|(p, _)| (p, 0)).collect()
    }

    /// One block: notes to start and stop (velocity 0 = stop). `frames` long at `rate`, tempo
    /// `bpm`; while playing, `song` is the block's tick range (steps lie on the song grid).
    pub fn tick(&mut self, frames: usize, rate: f64, bpm: f64, song: Option<(f64, f64, u32)>) -> Vec<(u8, u8)> {
        let mut ev = Vec::new();
        let f = frames as f64;
        // notes ending
        for n in self.sounding.iter_mut() {
            n.1 -= f;
        }
        self.sounding.retain(|n| {
            if n.1 <= 0.0 {
                ev.push((n.0, 0));
                false
            } else {
                true
            }
        });
        if self.playing.is_empty() {
            return ev;
        }
        let step_frames = self.settings.step_beats() * 60.0 / bpm.max(1.0) * rate;
        let start_now = std::mem::take(&mut self.start_now);
        let fire = match song {
            Some((from, to, ppq)) => {
                // a grid point in this block, or a chord just begun
                let step = self.settings.step_beats() * ppq as f64;
                let g = (from / step).ceil() * step;
                g < to || start_now
            }
            None => {
                self.phase += f / step_frames.max(1.0);
                if self.phase >= 1.0 || start_now {
                    self.phase = if start_now { 0.0 } else { self.phase - self.phase.floor() };
                    true
                } else {
                    false
                }
            }
        };
        if fire {
            let len = step_frames * self.settings.gate.clamp(0.05, 1.0) as f64;
            for (p, v) in step_notes(&self.playing, &self.settings, self.count) {
                if let Some(i) = self.sounding.iter().position(|n| n.0 == p) {
                    self.sounding.remove(i);
                    ev.push((p, 0));
                }
                ev.push((p, v));
                self.sounding.push((p, len.max(1.0)));
            }
            self.count += 1;
        }
        ev
    }
}

/// The engine's arpeggiators by track.
#[derive(Default)]
pub struct EngineArps {
    pub tracks: HashMap<usize, Live>,
}

impl EngineArps {
    pub fn active(&self, t: usize) -> bool {
        self.tracks.get(&t).is_some_and(|l| l.settings.on)
    }

    /// New settings from the app (tracks switched off let their notes go).
    pub fn set(&mut self, all: &HashMap<usize, Settings>) -> Vec<(usize, u8, u8)> {
        let mut offs = Vec::new();
        for (t, l) in self.tracks.iter_mut() {
            let s = all.get(t).copied().unwrap_or_default();
            if !s.on && l.settings.on {
                offs.extend(l.reset().into_iter().map(|(p, v)| (*t, p, v)));
            }
            l.settings = s;
        }
        for (t, s) in all {
            self.tracks.entry(*t).or_insert_with(|| Live { settings: *s, ..Default::default() });
        }
        offs
    }

    pub fn tick(&mut self, frames: usize, rate: f64, bpm: f64, song: Option<(f64, f64, u32)>) -> Vec<(usize, u8, u8)> {
        let mut out = Vec::new();
        for (t, l) in self.tracks.iter_mut() {
            if l.settings.on || !l.sounding.is_empty() {
                out.extend(l.tick(frames, rate, bpm, song).into_iter().map(|(p, v)| (*t, p, v)));
            }
        }
        out
    }
}

/// Playback: a track's notes through its Arpeggiator (when on); latched notes run to the end
/// of the track's last region.
pub fn for_playback(app: &crate::app::App, t: usize, notes: &mut Vec<(u32, u32, u8, u8)>) {
    let Some(s) = app.sc.arps.get(&t).filter(|s| s.on) else { return };
    let until = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.iter().filter(|r| !r.muted).map(|r| r.start + r.length).max()).unwrap_or(0);
    *notes = arpeggiate(notes, s, lg_core::events::PPQ, until);
    notes.sort_by_key(|n| n.0);
}

/// The engine's live Arpeggiators follow the tracks' settings.
pub fn to_engine(app: &crate::app::App, e: &mut crate::audio::Engine) {
    for (t, p, v) in e.arp.set(&app.sc.arps) {
        e.live_note(t, p, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pitches(seq: Vec<Vec<(u8, u8)>>) -> Vec<Vec<u8>> {
        seq.into_iter().map(|s| s.into_iter().map(|n| n.0).collect()).collect()
    }

    #[test]
    fn note_orders() {
        let held = [(64, 100), (60, 90), (67, 80)];
        let mut s = Settings::default();
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![60], vec![64], vec![67]]);
        s.order = Order::Down;
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![67], vec![64], vec![60]]);
        s.order = Order::UpDown;
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![60], vec![64], vec![67], vec![64]]);
        s.order = Order::OutsideIn;
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![67], vec![60], vec![64]]);
        s.order = Order::AsPlayed;
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![64], vec![60], vec![67]]);
        s.order = Order::Chord;
        s.octaves = 2;
        assert_eq!(pitches(sequence(&held, &s)), vec![vec![60, 64, 67], vec![72, 76, 79]]);
        s.order = Order::Up;
        assert_eq!(sequence(&held, &s).len(), 6);
        assert_eq!(sequence(&held, &s)[3][0].0, 72);
        // random: every step is one of the notes, and the same each run
        s.order = Order::Random;
        let a: Vec<u8> = (0..16).map(|k| step_notes(&held, &s, k)[0].0).collect();
        let b: Vec<u8> = (0..16).map(|k| step_notes(&held, &s, k)[0].0).collect();
        assert_eq!(a, b);
        assert!(a.iter().all(|p| [60, 64, 67, 72, 76, 79].contains(p)));
        // a fixed velocity
        s.velocity = Some(77);
        assert_eq!(step_notes(&held, &s, 0)[0].1, 77);
    }

    #[test]
    fn playback_arpeggiates_on_the_grid() {
        // a C major chord held for a beat at 1/16: four steps, C E G C
        let notes = [(0, 960, 60, 100), (0, 960, 64, 100), (0, 960, 67, 100)];
        let s = Settings { on: true, ..Default::default() };
        let out = arpeggiate(&notes, &s, 960, 3840);
        assert_eq!(out.iter().map(|n| (n.0, n.2)).collect::<Vec<_>>(), vec![(0, 60), (240, 64), (480, 67), (720, 60)]);
        assert!(out.iter().all(|n| n.1 - n.0 == 120));
        // 1/8 triplets: 3 per beat
        let s = Settings { on: true, rate: 8, ..Default::default() };
        assert_eq!(arpeggiate(&notes, &s, 960, 3840).len(), 3);
        // latched: runs on after the keys go up, to `until`
        let s = Settings { on: true, latch: Latch::Latch, ..Default::default() };
        let out = arpeggiate(&notes, &s, 960, 1920);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn latch_transpose_moves_the_chord() {
        // a chord, released, then one key a tone up
        let notes = [(0, 480, 60, 100), (0, 480, 64, 100), (960, 1200, 62, 100)];
        let s = Settings { on: true, latch: Latch::Transpose, rate: 4, ..Default::default() };
        let out = arpeggiate(&notes, &s, 960, 1920);
        // beat 1: C (then E latched), beat 2: D, F# (transposed by +2)
        let p: Vec<(u32, u8)> = out.iter().map(|n| (n.0, n.2)).collect();
        assert_eq!(p, vec![(0, 60), (960, 62)]);
        let s = Settings { rate: 7, ..s };
        let p: Vec<u8> = arpeggiate(&notes, &s, 960, 1920).iter().map(|n| n.2).collect();
        assert_eq!(p, vec![60, 64, 62, 66]);
    }

    #[test]
    fn live_steps_at_the_tempo() {
        let mut l = Live { settings: Settings { on: true, rate: 4, ..Default::default() }, ..Default::default() };
        l.key(60, 100);
        l.key(64, 100);
        // 120 bpm, quarter notes, 48 kHz: a step each 24000 frames
        let mut starts = Vec::new();
        let mut frame = 0usize;
        while frame < 48_000 * 2 {
            for (p, v) in l.tick(64, 48_000.0, 120.0, None) {
                if v > 0 {
                    starts.push((frame, p));
                }
            }
            frame += 64;
        }
        assert_eq!(starts.iter().map(|s| s.1).collect::<Vec<_>>(), vec![60, 64, 60, 64]);
        assert!(starts[1].0.abs_diff(starts[0].0 + 24_000) <= 64, "{starts:?}");
        // released in Reflex: it stops
        l.key(60, 0);
        l.key(64, 0);
        let ev: Vec<_> = (0..800).flat_map(|_| l.tick(64, 48_000.0, 120.0, None)).filter(|e| e.1 > 0).collect();
        assert!(ev.is_empty());
    }

    #[test]
    fn live_latch_keeps_playing() {
        let mut l = Live { settings: Settings { on: true, latch: Latch::Latch, rate: 4, ..Default::default() }, ..Default::default() };
        l.key(60, 100);
        l.key(60, 0);
        let ons = (0..1000).flat_map(|_| l.tick(64, 48_000.0, 120.0, None)).filter(|e| e.1 > 0).count();
        assert!(ons >= 2);
        assert!(!l.reset().is_empty() || l.sounding.is_empty());
    }
}
