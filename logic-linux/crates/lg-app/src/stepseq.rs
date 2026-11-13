//! The Step Sequencer (Logic Pro 13's, for pattern regions): a grid of rows (one per drum sound
//! or pitch) by steps (16 by default, 1–64), each step on or off with its own velocity, gate,
//! note repeat, chance, start offset, tie, octave and skip. Rows have mute and solo, their own
//! rate and length; the pattern its rate and length.
//!
//! A pattern region plays through the existing engine: the pattern is rendered to notes in the
//! region (`StepPattern::render`) after every edit, so the region's notes are what you hear and
//! what Save writes. The patterns themselves live in egui's memory keyed by (track, region);
//! when the region's notes are changed elsewhere (Undo, the Piano Roll) the pattern is read
//! back from them.
//!
//! Logic's saved patterns (the GenM chunk's NSKeyedArchiver "StepSequencer" state, see the
//! research's format notes) are read by `read_patterns`.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::look;

/// Ticks per step at 1/16.
pub const SIXTEENTH: u32 = 240;
pub const MAX_STEPS: usize = 64;

/// What a step holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub on: bool,
    pub velocity: u8,
    /// Percent of the step (1–100; a tied step carries on through the next).
    pub gate: u8,
    /// Notes played in the step (1 = no repeat).
    pub repeat: u8,
    /// Percent chance it plays.
    pub chance: u8,
    /// Start offset, percent of a step (−50…50).
    pub offset: i8,
    /// Ties the step to the previous one (one longer note).
    pub tie: bool,
    pub octave: i8,
    /// Skipped in playback: the row moves straight on to the next step.
    pub skip: bool,
    /// Note mode: semitones from the row's note.
    pub note: i8,
    /// The inspector's: play condition (Recurrence: iteration of cycles), own rate (ticks),
    /// MIDI channel (1–16), articulation ID, note-repeat velocity ramp / curve / distribution.
    pub cond: crate::stepinsp::Cond,
    pub iter: u8,
    pub cycles: u8,
    pub rate: Option<u32>,
    pub channel: Option<u8>,
    pub articulation: u8,
    pub ramp: i8,
    pub curve: i8,
    pub dist: i8,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, velocity: 100, gate: 100, repeat: 1, chance: 100, offset: 0, tie: false, octave: 0, skip: false, note: 0, cond: crate::stepinsp::Cond::Always, iter: 1, cycles: 2, rate: None, channel: None, articulation: 0, ramp: 0, curve: 0, dist: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub pitch: u8,
    pub mute: bool,
    pub solo: bool,
    /// The row's own length (loop end) and rate (ticks per step), else the pattern's.
    pub len: Option<u8>,
    pub rate: Option<u32>,
    pub steps: Vec<Step>,
    /// Loop start (the first step played, 0-based).
    pub start: u8,
    /// The row's own playback mode (else the pattern's).
    pub play: Option<Play>,
    /// Subrows shown (velocity, gate and chance lanes under the row).
    pub sub: bool,
    /// The inspector's row settings (MIDI channel, automation mode).
    pub opts: crate::stepinsp::RowOpts,
}

impl Row {
    pub fn new(name: impl Into<String>, pitch: u8) -> Row {
        Row { name: name.into(), pitch, mute: false, solo: false, len: None, rate: None, steps: vec![Step::default(); MAX_STEPS], start: 0, play: None, sub: false, opts: Default::default() }
    }
}

/// Playback modes (Logic's: for the pattern and for each row).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Play {
    #[default]
    Forward,
    Reverse,
    PingPong,
    Shuffle,
    Pendulum,
    Wander,
    Random,
}

impl Play {
    pub const ALL: [Play; 7] = [Play::Forward, Play::Reverse, Play::PingPong, Play::Shuffle, Play::Pendulum, Play::Wander, Play::Random];
    pub fn name(self) -> &'static str {
        match self {
            Play::Forward => "Forward",
            Play::Reverse => "Reverse",
            Play::PingPong => "Ping Pong",
            Play::Shuffle => "Shuffle",
            Play::Pendulum => "Pendulum",
            Play::Wander => "Wander",
            Play::Random => "Random",
        }
    }
    pub fn arrow(self) -> &'static str {
        match self {
            Play::Forward => "→",
            Play::Reverse => "←",
            Play::PingPong => "↔",
            Play::Shuffle => "⤮",
            Play::Pendulum => "⟷",
            Play::Wander => "↝",
            Play::Random => "?",
        }
    }
}

/// The order a row's `n` playable steps are played in over `slots` slots: indices into them.
/// Ping Pong goes forward then back playing the ends twice (1 2 3 4 4 3 2 1 …); Pendulum
/// turns without repeating them (1 2 3 4 3 2 …), as Logic's guide describes; Shuffle plays every step once per pass in a
/// new order; Wander moves one step forward or back at random; Random jumps anywhere.
/// `seed` makes the random modes repeatable.
pub fn play_order(mode: Play, n: usize, slots: usize, seed: u64, row: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let rnd = |k: usize| chance(seed ^ 0x5eed, row, k as u32) as usize;
    match mode {
        Play::Forward => (0..slots).map(|k| k % n).collect(),
        Play::Reverse => (0..slots).map(|k| n - 1 - k % n).collect(),
        // Ping-Pong plays the ends twice, so a pass is twice the row (the guide: it doubles the
        // pattern length); Pendulum turns at the ends without repeating them
        Play::PingPong => {
            let period = 2 * n;
            (0..slots).map(|k| { let p = k % period; if p < n { p } else { period - 1 - p } }).collect()
        }
        Play::Pendulum => {
            if n == 1 {
                return vec![0; slots];
            }
            let period = 2 * n - 2;
            (0..slots).map(|k| { let p = k % period; if p < n { p } else { period - p } }).collect()
        }
        Play::Shuffle => {
            let mut out = Vec::with_capacity(slots);
            let mut pass = 0usize;
            while out.len() < slots {
                let mut order: Vec<usize> = (0..n).collect();
                // Fisher–Yates with the repeatable source
                for i in (1..n).rev() {
                    let j = (chance(seed ^ 0x5u64.wrapping_mul(pass as u64 + 1), row, i as u32) as usize * 7919 + i * 31) % (i + 1);
                    order.swap(i, j);
                }
                out.extend(order);
                pass += 1;
            }
            out.truncate(slots);
            out
        }
        Play::Wander => {
            let mut out = Vec::with_capacity(slots);
            let mut cur = 0usize;
            for k in 0..slots {
                out.push(cur);
                cur = if rnd(k) < 50 { (cur + 1) % n } else { (cur + n - 1) % n };
            }
            out
        }
        Play::Random => (0..slots).map(|k| (rnd(k) * 7919 + k) % n).collect(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepPattern {
    /// Pattern length in steps (1–64).
    pub steps: u8,
    /// Ticks per step (240 = 1/16).
    pub rate: u32,
    pub rows: Vec<Row>,
    /// The pattern's playback mode.
    pub play: Play,
    /// The inspector's pattern settings (legato, swing, key, channel …).
    pub opts: crate::stepinsp::PatternOpts,
}

impl Default for StepPattern {
    fn default() -> Self {
        StepPattern::melodic()
    }
}

/// The Row Assignment pop-up's entries: a drum kit's kit pieces (by name), then a Notes submenu
/// by octave. Returns the chosen note.
fn assignment_menu(ui: &mut egui::Ui, cur: u8, drums: bool) -> Option<u8> {
    let mut out = None;
    if drums {
        for p in 35..=81u8 {
            if let Some(n) = drum_name(p) {
                if ui.selectable_label(p == cur, n).clicked() {
                    out = Some(p);
                    ui.close();
                }
            }
        }
        ui.separator();
    }
    ui.menu_button("Notes", |ui| {
        for oct in 0..11u8 {
            let lo = oct * 12;
            ui.menu_button(format!("{} – {}", crate::app::note_name(lo), crate::app::note_name((lo + 11).min(127))), |ui| {
                for p in lo..=(lo + 11).min(127) {
                    if ui.selectable_label(p == cur, crate::app::note_name(p)).clicked() {
                        out = Some(p);
                        ui.close();
                    }
                }
            });
        }
    });
    out
}

/// Reassigns a row to note `p`: its steps play that note, and it takes the kit piece's name
/// (drum rows) or the note's name.
pub fn assign_row(row: &mut Row, p: u8, drums: bool) {
    row.pitch = p.min(127);
    row.name = drum_name(row.pitch).filter(|_| drums).map(str::to_string).unwrap_or_else(|| crate::app::note_name(row.pitch));
}

/// General MIDI drum names for drum rows (Logic names Drum Machine Designer rows after pads).
pub fn drum_name(pitch: u8) -> Option<&'static str> {
    Some(match pitch {
        35 => "Kick 2",
        36 => "Kick 1",
        37 => "Rim 1",
        38 => "Snare 1",
        39 => "Clap 1",
        40 => "Snare 2",
        41 => "Low Tom 2",
        42 => "Hi-Hat 1",
        43 => "Low Tom 1",
        44 => "Hi-Hat 2",
        45 => "Mid Tom 2",
        46 => "Open Hi-Hat",
        47 => "Mid Tom 1",
        48 => "Hi Tom 2",
        49 => "Crash 1",
        50 => "Hi Tom 1",
        51 => "Ride 1",
        52 => "China",
        53 => "Ride Bell",
        54 => "Tambourine",
        55 => "Splash",
        56 => "Cowbell",
        57 => "Crash 2",
        59 => "Ride 2",
        69 => "Cabasa",
        70 => "Maracas",
        75 => "Claves",
        _ => return None,
    })
}

impl StepPattern {
    /// A new pattern for a software instrument: C3 down the C major scale to C2 (Logic's).
    pub fn melodic() -> StepPattern {
        let rows = [72u8, 71, 69, 67, 65, 64, 62, 60].iter().map(|&p| Row::new(crate::app::note_name(p - 12), p - 12)).collect();
        StepPattern { steps: 16, rate: SIXTEENTH, rows, play: Play::Forward, opts: Default::default() }
    }

    /// A new pattern for drums: Logic's kit rows.
    pub fn drums() -> StepPattern {
        StepPattern::with_rows(kit_rows(&Kit::Gm))
    }

    /// A new 16-step pattern of these (note, name) rows.
    pub fn with_rows(rows: Vec<(u8, String)>) -> StepPattern {
        let rows = rows.into_iter().map(|(p, n)| Row::new(n, p)).collect();
        StepPattern { steps: 16, rate: SIXTEENTH, rows, play: Play::Forward, opts: Default::default() }
    }

    pub fn row_len(&self, r: &Row) -> usize {
        r.len.unwrap_or(self.steps).clamp(1, MAX_STEPS as u8) as usize
    }

    /// The notes the pattern plays over `len` ticks (from 0), looping each row over its own
    /// length; `seed` makes Chance repeatable.
    pub fn render(&self, len: u32, seed: u64) -> Vec<Note> {
        let any_solo = self.rows.iter().any(|r| r.solo);
        let mut out = Vec::new();
        for (ri, row) in self.rows.iter().enumerate() {
            if row.mute || (any_solo && !row.solo) {
                continue;
            }
            let n = self.row_len(row);
            let rate = row.rate.unwrap_or(self.rate).max(1);
            // the steps played: loop start to end, skipped ones left out
            let start = (row.start as usize).min(n - 1);
            let active: Vec<usize> = (start..n).filter(|&k| !row.steps[k].skip).collect();
            if active.is_empty() {
                continue;
            }
            // (steps can have their own rate: enough slots for the shortest)
            let min_rate = active.iter().filter_map(|&k| row.steps[k].rate).min().unwrap_or(rate).min(rate).max(1);
            let slots = len.div_ceil(min_rate) as usize;
            let order = play_order(row.play.unwrap_or(self.play), active.len(), slots, seed, ri);
            // loop passes over the region (Condition: First / Last Loop, Recurrence)
            let pass_len: u32 = active.iter().map(|&k| row.steps[k].rate.unwrap_or(rate)).sum::<u32>().max(1);
            let passes = len.div_ceil(pass_len) as usize;
            let o = &self.opts;
            // the note a tie carries on, as an index into `out`
            let mut last: Option<usize> = None;
            let mut t = 0u32;
            for (slot, &oi) in order.iter().enumerate() {
                if t >= len {
                    break;
                }
                let slot = slot as u32;
                let s = row.steps[active[oi]];
                let step_len = s.rate.unwrap_or(rate).max(1);
                let pass = slot as usize / active.len();
                let at = crate::stepinsp::swing(t, o.swing, o.swing_div);
                let gate_len = |l: u32, g: u8| if g >= 100 && o.legato { l + 1 } else { l * g.clamp(1, 100) as u32 / 100 };
                if s.on && s.tie && last.is_some() {
                    if let Some(prev) = last.and_then(|k| out.get_mut(k)) {
                        let prev: &mut Note = prev;
                        let end = (at + gate_len(step_len, s.gate)).min(len);
                        prev.len = end.saturating_sub(prev.pos).max(prev.len);
                    }
                } else if s.on && chance(seed, ri, slot) < s.chance && crate::stepinsp::cond_plays(s.cond, s.iter, s.cycles, pass, passes) {
                    let reps = s.repeat.clamp(1, 16) as u32;
                    let off = (step_len as i64 * s.offset.clamp(-50, 50) as i64 / 100) as i64;
                    let pitch = (row.pitch as i32 + 12 * s.octave as i32 + s.note as i32).clamp(0, 127) as u8;
                    let channel = crate::stepinsp::channel(self, row, s.channel);
                    for k in 0..reps {
                        let f0 = crate::stepinsp::repeat_at(k, reps, s.dist);
                        let f1 = crate::stepinsp::repeat_at(k + 1, reps, s.dist);
                        let pos = (at as i64 + (f0 * step_len as f32) as i64 + off).max(0) as u32;
                        if pos >= len {
                            break;
                        }
                        let sub = (((f1 - f0) * step_len as f32) as u32).max(1);
                        let l = gate_len(sub, s.gate).max(1).min(len - pos);
                        let velocity = crate::stepinsp::ramp_velocity(s.velocity.clamp(1, 127), k, reps, s.ramp, s.curve);
                        out.push(Note { channel, pitch, velocity, pos, len: l, muted: false });
                    }
                    last = Some(out.len() - 1);
                } else {
                    last = None;
                }
                t += step_len;
            }
        }
        out.sort_by_key(|n| (n.pos, std::cmp::Reverse(n.pitch)));
        out
    }

    /// The pattern's musical length in ticks: how long until every row is back at its start
    /// together. Each row loops over its own played steps (loop start to end, skipped steps
    /// left out, each at its own rate); Ping-Pong doubles a row's pass and Pendulum plays the
    /// ends once; rows of different lengths extend it until they realign (the least common
    /// multiple). `None` when a row plays Random, Shuffle or Wander (it never repeats).
    pub fn musical_len(&self) -> Option<u32> {
        let mut total: u64 = 0;
        for row in &self.rows {
            let n = self.row_len(row);
            let rate = row.rate.unwrap_or(self.rate).max(1);
            let start = (row.start as usize).min(n - 1);
            let active: Vec<u32> = (start..n).filter(|&k| !row.steps[k].skip).map(|k| row.steps[k].rate.unwrap_or(rate).max(1)).collect();
            if active.is_empty() {
                continue;
            }
            let pass: u32 = match row.play.unwrap_or(self.play) {
                Play::Forward | Play::Reverse => active.iter().sum(),
                Play::PingPong => 2 * active.iter().sum::<u32>(),
                Play::Pendulum if active.len() > 1 => 2 * active.iter().sum::<u32>() - active[0] - active[active.len() - 1],
                Play::Pendulum => active[0],
                Play::Random | Play::Shuffle | Play::Wander => return None,
            };
            total = if total == 0 { pass as u64 } else { lcm(total, pass as u64) };
        }
        Some(total.min(u32::MAX as u64) as u32)
    }

    /// A pattern read from a region's notes (positions from the region start): one row per
    /// pitch (highest first, as Logic sorts), notes on the 1/16 grid, longer notes tied.
    pub fn from_notes(notes: &[Note], len: u32, drums: bool) -> StepPattern {
        // 16 steps, or as many sixteenths (in whole bars of 16) as the notes need
        let last = notes.iter().map(|n| n.pos / SIXTEENTH + 1).max().unwrap_or(16);
        let steps = (last.div_ceil(16) * 16).min(len.div_ceil(SIXTEENTH).max(1)).clamp(1, MAX_STEPS as u32) as u8;
        let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        pitches.sort_unstable_by(|a, b| b.cmp(a));
        pitches.dedup();
        let mut pat = if pitches.is_empty() {
            if drums { StepPattern::drums() } else { StepPattern::melodic() }
        } else {
            let name = |p: u8| drums.then(|| drum_name(p)).flatten().map(str::to_string).unwrap_or_else(|| crate::app::note_name(p));
            StepPattern { steps: 16, rate: SIXTEENTH, rows: pitches.iter().map(|&p| Row::new(name(p), p)).collect(), play: Play::Forward, opts: Default::default() }
        };
        if !notes.is_empty() {
            pat.steps = steps;
        }
        for n in notes {
            let k = ((n.pos as f32 / SIXTEENTH as f32).round()) as usize;
            if k >= pat.steps as usize {
                continue;
            }
            let Some(row) = pat.rows.iter_mut().find(|r| r.pitch == n.pitch) else { continue };
            let span = (n.len as f32 / SIXTEENTH as f32).max(0.0);
            let whole = span.floor().max(1.0) as usize;
            let s = &mut row.steps[k];
            s.on = true;
            s.velocity = n.velocity;
            s.gate = if span <= 1.0 { ((span * 100.0).round() as u8).clamp(1, 100) } else { 100 };
            // longer notes: the following steps tied on
            for j in 1..whole {
                if let Some(t) = row.steps.get_mut(k + j).filter(|_| k + j < pat.steps as usize) {
                    *t = Step { on: true, tie: true, velocity: n.velocity, ..Step::default() };
                }
            }
            let rest = span - whole as f32;
            if span > 1.0 && rest > 0.05 {
                if let Some(t) = row.steps.get_mut(k + whole).filter(|_| k + whole < pat.steps as usize) {
                    *t = Step { on: true, tie: true, velocity: n.velocity, gate: ((rest * 100.0).round() as u8).clamp(1, 100), ..Step::default() };
                }
            }
        }
        pat
    }
}

/// A repeatable 0..100 per (row, slot).
fn lcm(a: u64, b: u64) -> u64 {
    fn gcd(a: u64, b: u64) -> u64 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    a / gcd(a, b) * b
}

fn chance(seed: u64, row: usize, slot: u32) -> u8 {
    let mut x = seed ^ (row as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (slot as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 31;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 29;
    (x % 100) as u8
}

/// Edit modes (Logic's: Step On/Off, Velocity/Value, Gate, Note Repeat, Chance, Start Offset,
/// Tie, Octave, Skip, Loop Start/End).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    OnOff,
    Velocity,
    Gate,
    Repeat,
    Chance,
    Offset,
    Tie,
    Octave,
    Skip,
    Loop,
    Note,
    /// Step Rate: the step's own note value (stepinsp's per-step rate).
    Rate,
}

impl Mode {
    pub const ALL: [Mode; 12] = [Mode::OnOff, Mode::Velocity, Mode::Gate, Mode::Tie, Mode::Note, Mode::Octave, Mode::Loop, Mode::Repeat, Mode::Chance, Mode::Offset, Mode::Skip, Mode::Rate];
    pub fn name(self) -> &'static str {
        match self {
            Mode::OnOff => "On/Off",
            Mode::Velocity => "Velocity/Value",
            Mode::Gate => "Gate",
            Mode::Repeat => "Note Repeat",
            Mode::Chance => "Chance",
            Mode::Offset => "Start Offset",
            Mode::Tie => "Tie",
            Mode::Octave => "Octave",
            Mode::Skip => "Skip",
            Mode::Loop => "Loop Start/End",
            Mode::Note => "Note",
            Mode::Rate => "Step Rate",
        }
    }
    /// The value of a step in this mode, 0..1 (for the bar drawn in the cell).
    fn get(self, s: &Step) -> f32 {
        match self {
            Mode::Velocity => s.velocity as f32 / 127.0,
            Mode::Gate => s.gate as f32 / 100.0,
            Mode::Repeat => (s.repeat as f32 - 1.0) / 15.0,
            Mode::Rate => {
                let o = rate_order();
                1.0 - o.iter().position(|&r| Some(r) == s.rate).unwrap_or(sixteenth_index(&o)) as f32 / (o.len() - 1) as f32
            }
            Mode::Chance => s.chance as f32 / 100.0,
            Mode::Offset => (s.offset as f32 + 50.0) / 100.0,
            Mode::Octave => (s.octave as f32 + 3.0) / 6.0,
            Mode::Note => (s.note as f32 + 12.0) / 24.0,
            _ => s.on as u8 as f32,
        }
    }
    fn set(self, s: &mut Step, f: f32) {
        let f = f.clamp(0.0, 1.0);
        match self {
            Mode::Velocity => s.velocity = (f * 126.0 + 1.0).round() as u8,
            Mode::Gate => s.gate = (f * 99.0 + 1.0).round() as u8,
            Mode::Repeat => s.repeat = (f * 15.0 + 1.0).round() as u8,
            Mode::Rate => {
                let o = rate_order();
                s.rate = Some(o[((1.0 - f) * (o.len() - 1) as f32).round() as usize]);
            }
            Mode::Chance => s.chance = (f * 100.0).round() as u8,
            Mode::Offset => s.offset = (f * 100.0 - 50.0).round() as i8,
            Mode::Octave => s.octave = (f * 6.0 - 3.0).round() as i8,
            Mode::Note => s.note = (f * 24.0 - 12.0).round() as i8,
            _ => {}
        }
    }
    fn text(self, s: &Step) -> String {
        match self {
            Mode::Velocity => s.velocity.to_string(),
            Mode::Gate => format!("{}%", s.gate),
            Mode::Repeat => s.repeat.to_string(),
            Mode::Chance => format!("{}%", s.chance),
            Mode::Offset => format!("{:+}%", s.offset),
            Mode::Octave => format!("{:+}", s.octave),
            Mode::Note => format!("{:+}", s.note),
            Mode::Rate => s.rate.map_or("–".into(), rate_name),
            _ => String::new(),
        }
    }
}

/// Step rates (Logic's Step Rate menu): 4, 2 and 1 whole notes, /2 … /64, the triplets 1T … 32T
/// and the dotted 1D … 64D (ticks per step; a whole note is 3840).
pub const RATES: [(&str, u32); 22] = [
    ("4", 15360), ("2", 7680), ("1", 3840), ("/2", 1920), ("/4", 960), ("/8", 480), ("/16", 240), ("/32", 120), ("/64", 60),
    ("1T", 2560), ("2T", 1280), ("4T", 640), ("8T", 320), ("16T", 160), ("32T", 80),
    ("1D", 5760), ("2D", 2880), ("4D", 1440), ("8D", 720), ("16D", 360), ("32D", 180), ("64D", 90),
];

/// The Pattern Length menu's sections (Logic's: lengths for 4/4, 3/4, 5/4 and 7/8 bars).
pub const LENGTH_SECTIONS: [(&str, &[u8]); 4] = [
    ("4 Based (4/4)", &[4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 64]),
    ("3 Based (3/4)", &[3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45, 48, 51, 54, 57, 60, 63]),
    ("5 Based (5/4)", &[5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60]),
    ("7 Based (7/8)", &[7, 14, 21, 28, 35, 42, 49, 56, 63]),
];

/// Where the menu's groups start (straight, triplet, dotted).
pub const RATE_GROUPS: [usize; 2] = [9, 15];

fn rate_name(r: u32) -> String {
    RATES.iter().find(|x| x.1 == r).map(|x| x.0.to_string()).unwrap_or_else(|| format!("{r}t"))
}

// ---- the patterns and the editor's state (egui memory) -----------------------------------

#[derive(Clone, Debug, Default)]
struct Entry {
    pattern: StepPattern,
    /// The notes last written (from the region start), to notice edits made elsewhere.
    written: Vec<Note>,
}

#[derive(Clone, Debug, Default)]
struct Store {
    patterns: HashMap<(usize, usize), Entry>,
    /// Per (track, region, row) edit mode.
    modes: HashMap<(usize, usize, usize), Mode>,
    selected_row: usize,
    page: usize,
    /// The press in progress: (row, paint state for toggles), and whether it is a
    /// Command-drag that ties the steps it crosses.
    press: Option<(usize, bool)>,
    press_tie: bool,
    /// A Command-Option-drag ramp: (first step, its value 0..1, Shift held: off steps stay off).
    ramp: Option<(usize, f32, bool)>,
    pressed_cells: Vec<(usize, usize)>,
    changed: bool,
    seed: u64,
    /// The step selection: anchor and cursor (row, step).
    sel: Option<((usize, usize), (usize, usize))>,
    /// Copy Row's row, and the steps copied (rows of steps).
    clip_row: Option<Row>,
    clip_steps: Vec<Vec<Step>>,
    /// The page overview's Copy / Cut (every row's 16 steps).
    clip_page: Vec<Vec<Step>>,
    /// The Pattern Browser shown; step colours by the region's colour; Mono Mode; Live Record;
    /// Learn waiting for a note (true: add a row, false: assign the selected row).
    browser: bool,
    by_region: bool,
    mono: bool,
    live_record: bool,
    /// MIDI In (step recording): on, and notes entered at the current step not yet released.
    step_rec: bool,
    step_pending: bool,
    /// MIDI Out: steps sound while they are edited; the note sounding (track, pitch).
    midi_out_off: bool,
    out_note: Option<(usize, u8)>,
    /// Functions ▸ Live Record Options (Velocity, Note Length, Quantize), and the notes being
    /// live-recorded: (pitch, row, step, start tick).
    live_opts: LiveOpts,
    live_held: Vec<(u8, usize, usize, u32)>,
    /// The Preview button's playback.
    preview: Option<Preview>,
    /// View ▸ Display Edit Mode Values: 0 the selected row (Logic's default), 1 only when
    /// editing, 2 all rows.
    values_display: u8,
    learn: Option<bool>,
    /// The notes held last frame (Learn and Live Record listen for new ones).
    held: Vec<u8>,
    /// Rename Row / Save Pattern / Save Template being typed.
    typing: Option<(Typing, String)>,
    /// The local inspector (stepinsp.rs) shown, its tab, and an edit in it still going on
    /// (one undo step per drag).
    inspector: bool,
    insp_tab: crate::stepinsp::Tab,
    insp_active: bool,
    /// Sort Rows: (by type and note 0 / type 1 / type and name 2, ascending).
    sort: (u8, bool),
    /// The Randomize dialog (Control-click a Randomize button): its rows (None: the pattern)
    /// and where it opens; and its settings.
    rand_dlg: Option<(Option<usize>, egui::Pos2)>,
    rand: RandOpts,
    /// A row was moved by hand: new rows go at the end until Sort Rows is chosen again.
    manual_order: bool,
    /// The menu bar's Vertical Zoom slider (0..1; None: Logic's default, 48 pt rows), Vertical
    /// Auto Zoom, and the minimum Step Width (else the maximum: the steps fill the grid).
    v_zoom: Option<f32>,
    v_auto: bool,
    narrow_steps: bool,
}

/// The Randomize dialog's settings. On/Off, Tie and Skip use the probability (how likely a step
/// is on, tied or skipped); the value modes randomize by Offset (round the current value, a
/// percentage of it either way) or within a Range (lower and upper limits, in the mode's units).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandOpts {
    pub probability: u8,
    pub range_mode: bool,
    pub offset: u8,
    pub low: i32,
    pub high: i32,
}

impl Default for RandOpts {
    fn default() -> Self {
        RandOpts { probability: 35, range_mode: true, offset: 50, low: 1, high: 127 }
    }
}

/// A mode's value as a number in its own units, its limits, and setting it back.
fn mode_units(mode: Mode, s: &Step) -> Option<(i32, i32, i32)> {
    Some(match mode {
        Mode::Velocity => (s.velocity as i32, 1, 127),
        Mode::Gate => (s.gate as i32, 1, 100),
        Mode::Repeat => (s.repeat as i32, 1, 16),
        Mode::Chance => (s.chance as i32, 0, 100),
        Mode::Offset => (s.offset as i32, -50, 50),
        Mode::Octave => (s.octave as i32, -3, 3),
        Mode::Note => (s.note as i32, -12, 12),
        Mode::Rate => {
            let o = rate_order();
            (o.iter().position(|&r| Some(r) == s.rate).unwrap_or(sixteenth_index(&o)) as i32, 0, o.len() as i32 - 1)
        }
        _ => return None,
    })
}

fn set_units(mode: Mode, s: &mut Step, v: i32) {
    match mode {
        Mode::Velocity => s.velocity = v.clamp(1, 127) as u8,
        Mode::Gate => s.gate = v.clamp(1, 100) as u8,
        Mode::Repeat => s.repeat = v.clamp(1, 16) as u8,
        Mode::Chance => s.chance = v.clamp(0, 100) as u8,
        Mode::Offset => s.offset = v.clamp(-50, 50) as i8,
        Mode::Octave => s.octave = v.clamp(-3, 3) as i8,
        Mode::Note => s.note = v.clamp(-12, 12) as i8,
        Mode::Rate => {
            let o = rate_order();
            s.rate = Some(o[v.clamp(0, o.len() as i32 - 1) as usize]);
        }
        _ => {}
    }
}

/// Command-Option-drag: values ramped evenly from step `s0` (value `f0`) to step `s1` (`f1`),
/// in the row's edit mode; the steps are turned on unless `keep_off`.
pub fn ramp_values(steps: &mut [Step], mode: Mode, s0: usize, f0: f32, s1: usize, f1: f32, keep_off: bool) {
    let (a, b) = (s0.min(s1), s0.max(s1));
    for k in a..=b.min(steps.len() - 1) {
        let t = if s1 == s0 { 1.0 } else { (k as f32 - s0 as f32) / (s1 as f32 - s0 as f32) };
        let st = &mut steps[k];
        if keep_off && !st.on {
            continue;
        }
        mode.set(st, f0 + (f1 - f0) * t);
        st.on = true;
    }
}

/// Randomizes one step with the dialog's settings; `v` is a random 0..1.
pub fn randomize_with(mode: Mode, st: &mut Step, v: f32, o: RandOpts) {
    let p = o.probability as f32 / 100.0;
    match mode {
        Mode::OnOff | Mode::Loop => st.on = v < p,
        Mode::Tie => st.tie = st.on && v < p,
        Mode::Skip => st.skip = v < p,
        m => {
            let Some((cur, lo, hi)) = mode_units(m, st) else { return };
            if !st.on {
                return;
            }
            let (a, b) = if o.range_mode {
                (o.low.clamp(lo, hi).min(o.high.clamp(lo, hi)), o.high.clamp(lo, hi).max(o.low.clamp(lo, hi)))
            } else {
                let d = (cur.abs().max(1) * o.offset as i32) / 100;
                ((cur - d).max(lo), (cur + d).min(hi))
            };
            set_units(m, st, a + ((b - a) as f32 * v).round() as i32);
        }
    }
}

/// The Preview button's playback (wall-clock seconds from `start`): notes sent so far in this
/// pass, the ones sounding (pitch, off time).
#[derive(Clone, Debug, Default)]
struct Preview {
    start: f64,
    track: usize,
    sent: usize,
    sounding: Vec<(u8, f64)>,
    pass: u64,
}

fn preview_off(app: &mut App, t: usize, notes: &[(u8, f64)]) {
    if let Ok(mut e) = app.engine.lock() {
        for &(pitch, _) in notes {
            e.live(t, pitch, 0);
        }
    }
}

/// The notes (sorted by position) to start by `tick`, past the `sent` already started.
pub fn preview_due(notes: &[Note], sent: usize, tick: u32) -> &[Note] {
    let end = notes.iter().position(|n| n.pos > tick).unwrap_or(notes.len());
    &notes[sent.min(end)..end]
}

/// Each frame while previewing: starts and stops the pattern's notes on the wall clock (the
/// pattern loops); stops when the project starts playing.
fn preview_frame(app: &mut App, ctx: &egui::Context, pat: &StepPattern) {
    let Some(mut pv) = with_store(ctx, |s| s.preview.clone()) else { return };
    if app.playing() {
        preview_off(app, pv.track, &pv.sounding);
        with_store(ctx, |s| s.preview = None);
        return;
    }
    let now = ctx.input(|i| i.time);
    let ticks_per_s = app.tempo() / 60.0 * 960.0;
    let len = (pat.rate.max(1) * pat.steps.max(1) as u32).max(1);
    let elapsed = ((now - pv.start).max(0.0) * ticks_per_s) as u64;
    let pass = elapsed / len as u64;
    let notes = pat.render(len, pv.pass);
    if pass != pv.pass {
        // the next pass from its start
        pv.sent = 0;
        pv.pass = pass;
    }
    let tick = (elapsed % len as u64) as u32;
    let start_notes: Vec<Note> = preview_due(&notes, pv.sent, tick).to_vec();
    pv.sent += start_notes.len();
    let mut offs = Vec::new();
    pv.sounding.retain(|&(p, off)| if off <= now { offs.push((p, off)); false } else { true });
    preview_off(app, pv.track, &offs);
    if let Ok(mut e) = app.engine.lock() {
        for n in &start_notes {
            e.live(pv.track, n.pitch, n.velocity);
        }
    }
    for n in start_notes {
        pv.sounding.push((n.pitch, now + n.len as f64 / ticks_per_s));
    }
    with_store(ctx, |s| s.preview = Some(pv));
    ctx.request_repaint();
}

/// Functions ▸ Live Record Options (Logic's: all on by default; MPE isn't offered).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveOpts {
    pub velocity: bool,
    pub note_length: bool,
    pub quantize: bool,
}

impl Default for LiveOpts {
    fn default() -> Self {
        LiveOpts { velocity: true, note_length: true, quantize: true }
    }
}

/// Live Pattern Recording: a note played at `tick` (from the region start) turns on a step of
/// its row (a row made when it has none). Quantize on: the nearest step; off: the step it
/// falls in, with a Start Offset for the exact time. Velocity off: the default velocity. Mono
/// Mode: the other rows' steps in that column go off (the last note wins). Returns (row, step).
pub fn live_place(p: &mut StepPattern, pitch: u8, vel: u8, tick: u32, o: LiveOpts, mono: bool, order: Option<(u8, bool)>) -> Option<(usize, usize)> {
    let rate = p.rate.max(1);
    let steps = p.steps.max(1) as usize;
    let (step, offset) = if o.quantize {
        (((tick + rate / 2) / rate) as usize % steps, 0)
    } else {
        let s = (tick / rate) as usize;
        let mut off = ((tick % rate) * 100 / rate) as i32;
        // (Start Offset reaches half a step either way: late in a step is early in the next)
        let s = if off > 50 { off -= 100; s + 1 } else { s };
        (s % steps, off.clamp(-50, 50) as i8)
    };
    if !p.rows.iter().any(|x| x.pitch == pitch) {
        place_row(p, Row::new(crate::app::note_name(pitch), pitch), order);
    }
    let k = p.rows.iter().position(|x| x.pitch == pitch)?;
    let velocity = if o.velocity { vel.max(1) } else { Step::default().velocity };
    p.rows[k].steps[step] = Step { on: true, velocity, offset, ..Step::default() };
    if mono {
        for (ri, row) in p.rows.iter_mut().enumerate() {
            if ri != k {
                row.steps[step].on = false;
            }
        }
    }
    Some((k, step))
}

/// Live Pattern Recording with Note Length: a note held `len` ticks shortens the step's Gate,
/// or ties the steps after it (the last one's Gate for the rest).
pub fn live_release(p: &mut StepPattern, row: usize, step: usize, len: u32) {
    let rate = p.rate.max(1);
    let steps = p.steps.max(1) as usize;
    let Some(r) = p.rows.get_mut(row) else { return };
    let whole = (len / rate) as usize;
    let rest = len % rate;
    if whole == 0 {
        r.steps[step].gate = ((len * 100 / rate).max(1)) as u8;
        return;
    }
    // tied steps for the whole steps after the first, the last with the leftover as its gate
    let extra = if rest == 0 { whole - 1 } else { whole };
    for k in 1..=extra.min(steps - 1) {
        let st = &mut r.steps[(step + k) % steps];
        st.on = true;
        st.tie = true;
        st.gate = 100;
        if k == extra && rest > 0 {
            st.gate = ((rest * 100 / rate).max(1)) as u8;
        }
    }
}

/// The Functions menu's Live Record Options submenu (its items are checkboxes).
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if it.label == "Display Edit Mode Values" {
        ui.menu_button("Display Edit Mode Values", |ui| {
            let cur = with_store(ui.ctx(), |s| s.values_display);
            for (v, name) in [(1u8, "Only when Editing Values"), (0, "Always for Selected Row"), (2, "Always for All Rows")] {
                if ui.selectable_label(cur == v, name).clicked() {
                    with_store(ui.ctx(), |s| s.values_display = v);
                    ui.close();
                }
            }
        });
        return true;
    }
    if it.label != "Live Record Options" {
        return false;
    }
    let _ = app;
    ui.menu_button("Live Record Options", |ui| {
        let mut o = with_store(ui.ctx(), |s| s.live_opts);
        ui.checkbox(&mut o.velocity, "Velocity");
        ui.checkbox(&mut o.note_length, "Note Length");
        ui.checkbox(&mut o.quantize, "Quantize");
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "MPE"));
        with_store(ui.ctx(), |s| s.live_opts = o);
    });
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typing {
    Rename(usize),
    Save(bool),
}

fn store_id() -> egui::Id {
    egui::Id::unique("lg-stepseq-store")
}

fn with_store<R>(ctx: &egui::Context, f: impl FnOnce(&mut Store) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<Store>(store_id())))
}

/// Whether a region is a pattern region (opens in the Step Sequencer).
pub fn is_pattern(ctx: &egui::Context, track: usize, region: usize) -> bool {
    ctx.data(|d| d.get_temp::<Store>(store_id())).is_some_and(|s| s.patterns.contains_key(&(track, region)))
}

/// Sets every row's edit mode (Logic's "Set Global Edit Mode to …" key commands).
#[allow(dead_code)] // for the app's commands (see the wiring notes)
pub fn set_global_mode(ctx: &egui::Context, app: &App, mode: Mode) {
    let Some((t, r)) = app.selected_region else { return };
    with_store(ctx, |s| {
        let rows = s.patterns.get(&(t, r)).map_or(0, |e| e.pattern.rows.len());
        for k in 0..rows {
            s.modes.insert((t, r, k), mode);
        }
    });
}

fn is_drums(app: &App, t: usize) -> bool {
    kit_of(app, t).is_some()
}

/// A drum track's kit, for a new pattern's rows: General MIDI names (Drum Kit Designer, a
/// Drummer, a drum patch), Drum Machine Designer's default Drum Synth Kit, or its own pads.
#[derive(Clone, Debug, PartialEq)]
pub enum Kit {
    Gm,
    DmdDefault,
    DmdPads(Vec<(u8, String)>),
}

/// The kit a track plays, or None for a melodic instrument (by its kind, its instrument or
/// patch, and as a last resort its name).
fn kit_of(app: &App, t: usize) -> Option<Kit> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    if tr.kind == lg_core::TrackKind::Drummer {
        return Some(Kit::Gm);
    }
    let inst = app.instrument_for(t).unwrap_or_default();
    kit_for(&inst, &tr.name)
}

/// The kit an instrument choice (or patch) and track name stand for (kit_of; pure for tests).
pub fn kit_for(inst: &str, track_name: &str) -> Option<Kit> {
    let plugin = crate::soundlib::plugin_of(inst).trim();
    if plugin == "Drum Machine Designer" {
        let pads: Vec<(u8, String)> = crate::soundlib::pads_of(inst).into_iter().map(|(n, f)| (n, f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())).collect();
        return Some(if pads.is_empty() { Kit::DmdDefault } else { Kit::DmdPads(pads) });
    }
    if plugin == lg_synth::dmd::STYLES[0] {
        return Some(Kit::DmdDefault);
    }
    let drum_patch = |n: &str| {
        lg_synth::dmd::STYLES.contains(&n)
            || ["Drum Kit Designer", "Ultrabeat", "Drum Synth"].contains(&n)
            || crate::brpreview::kit_named(n)
            || lg_synth::patch(n).is_some_and(|s| matches!(s.category, Some(lg_synth::patches::Cat::AcousticDrums | lg_synth::patches::Cat::ElectronicDrums)))
    };
    let low = track_name.to_lowercase();
    if drum_patch(plugin) || drum_patch(track_name) || ["drum", " kit", "beat", "perc", "808", "909"].iter().any(|w| low.contains(w) || low.starts_with(w.trim())) {
        return Some(Kit::Gm);
    }
    None
}

/// Drum Machine Designer's default kit (Drum Synth Kit): its pads' sound names.
fn dmd_default_name(pitch: u8) -> Option<&'static str> {
    Some(match pitch {
        36 => "Kick 2",
        37 => "Rim 1",
        38 => "Snare 1",
        39 => "Clap 1",
        40 => "Snare 2",
        41 => "Tom Low",
        42 => "Hi-Hat 1",
        43 => "Tom Mid",
        44 => "Hi-Hat 2",
        45 => "Tom High",
        46 => "Hi-Hat 3",
        49 => "Crash",
        51 => "Ride",
        _ => return None,
    })
}

/// A kit's rows for a new pattern, kick first: kick, snare, rim, clap, the hi-hats (closed,
/// pedal, open), the toms low to high, crash and ride; a DMD of the user's own pads lists its
/// pads in that order, then the rest by note.
pub fn kit_rows(kit: &Kit) -> Vec<(u8, String)> {
    match kit {
        Kit::Gm => [36u8, 38, 37, 39, 42, 44, 46, 45, 47, 50, 49, 51].iter().map(|&p| (p, drum_name(p).unwrap_or_default().to_string())).collect(),
        Kit::DmdDefault => [36u8, 38, 37, 39, 42, 44, 46, 41, 43, 45, 49, 51].iter().map(|&p| (p, dmd_default_name(p).unwrap_or_default().to_string())).collect(),
        Kit::DmdPads(pads) => {
            const ORDER: [u8; 13] = [36, 35, 38, 40, 37, 39, 42, 44, 46, 41, 43, 45, 49];
            let mut v: Vec<(u8, String)> = pads.clone();
            v.sort_by_key(|(n, _)| (ORDER.iter().position(|o| o == n).unwrap_or(ORDER.len()), *n));
            v.dedup_by_key(|(n, _)| *n);
            v
        }
    }
}

/// Names a drum pattern's rows after the kit's pieces (a DMD's own pad names).
fn name_rows(p: &mut StepPattern, kit: &Kit) {
    let names = kit_rows(kit);
    for row in p.rows.iter_mut() {
        if let Some((_, n)) = names.iter().find(|(k, _)| *k == row.pitch).filter(|(_, n)| !n.is_empty()) {
            row.name = n.clone();
        }
    }
}

/// A new pattern's default for a track: its kit's rows for drums, else Logic's melodic rows.
fn default_pattern(app: &App, t: usize) -> StepPattern {
    match kit_of(app, t) {
        Some(kit) => StepPattern::with_rows(kit_rows(&kit)),
        None => StepPattern::melodic(),
    }
}

/// Keeps patterns on their regions when a region is deleted (later regions on the track move
/// down one index); call after removing `tracks[t].regions[r]`.
#[allow(dead_code)] // for the app's region delete (see the wiring notes)
pub fn region_removed(ctx: &egui::Context, t: usize, r: usize) {
    with_store(ctx, |s| {
        let old = std::mem::take(&mut s.patterns);
        s.patterns = old.into_iter().filter(|(k, _)| *k != (t, r)).map(|((kt, kr), e)| ((kt, if kt == t && kr > r { kr - 1 } else { kr }), e)).collect();
        s.modes.retain(|k, _| (k.0, k.1) != (t, r));
    });
}

/// The same when a whole track is deleted (later tracks move up one).
#[allow(dead_code)] // for the app's track delete (see the wiring notes)
pub fn track_removed(ctx: &egui::Context, t: usize) {
    with_store(ctx, |s| {
        let old = std::mem::take(&mut s.patterns);
        s.patterns = old.into_iter().filter(|(k, _)| k.0 != t).map(|((kt, kr), e)| ((if kt > t { kt - 1 } else { kt }, kr), e)).collect();
        s.modes.retain(|k, _| k.0 != t);
    });
}

/// Makes a MIDI region a pattern region (Logic's "Convert to Pattern Region"): its notes become
/// steps, and from then on it opens in the Step Sequencer.
pub fn convert_to_pattern(app: &mut App, ctx: &egui::Context, t: usize, r: usize) {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    if rg.audio.is_some() {
        return;
    }
    let rel: Vec<Note> = rg.notes.iter().map(|n| Note { pos: n.pos.saturating_sub(rg.start), ..*n }).collect();
    let kit = kit_of(app, t);
    let mut pattern = if rel.is_empty() { default_pattern(app, t) } else { StepPattern::from_notes(&rel, rg.length, kit.is_some()) };
    if let Some(kit) = &kit {
        name_rows(&mut pattern, kit);
    }
    with_store(ctx, |s| {
        s.patterns.insert((t, r), Entry { pattern, written: Vec::new() });
    });
    write(app, ctx, t, r, "Convert to Pattern Region");
    crate::score::set_editor_tab(ctx, crate::score::EditorTab::StepSequencer);
}

/// A new one-bar pattern region on a track at `at` (Logic's "Create Pattern Region").
#[allow(dead_code)] // for the app's commands (see the wiring notes)
pub fn new_pattern_region(app: &mut App, ctx: &egui::Context, t: usize, at: u32) {
    let bar = app.bar_ticks();
    let at = at / bar * bar;
    let fresh = default_pattern(app, t);
    let Some(name) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()) else { return };
    app.checkpoint("Create Pattern Region");
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    tr.regions.push(lg_core::Region { name, start: at, length: bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    let r = tr.regions.len() - 1;
    with_store(ctx, |s| {
        // the default pattern template when one is stored (Store as Default Pattern Template)
        let pattern = load_file(&default_path()).unwrap_or(fresh);
        s.patterns.insert((t, r), Entry { pattern, written: Vec::new() });
    });
    app.selected_track = Some(t);
    app.selected_region = Some((t, r));
    app.selected_notes.clear();
    app.after_edit();
}

/// Puts a pattern (from a Logic project, say) on a region.
#[allow(dead_code)] // for the app's commands (see the wiring notes)
pub fn set_pattern(app: &mut App, ctx: &egui::Context, t: usize, r: usize, pattern: StepPattern) {
    with_store(ctx, |s| {
        s.patterns.insert((t, r), Entry { pattern, written: Vec::new() });
    });
    write(app, ctx, t, r, "Load Pattern");
}

/// The pattern's notes into its region (an undo step first), the engine reloaded.
fn write(app: &mut App, ctx: &egui::Context, t: usize, r: usize, what: &str) {
    app.checkpoint(what);
    write_notes(app, ctx, t, r);
    app.after_edit();
}

/// A pattern region resized (regiontrim.rs): its pattern played again from its start, for its
/// new length.
pub fn rerender(app: &mut App, ctx: &egui::Context, t: usize, r: usize) {
    if is_pattern(ctx, t, r) {
        write_notes(app, ctx, t, r);
    }
}

fn write_notes(app: &mut App, ctx: &egui::Context, t: usize, r: usize) {
    let Some((start, len)) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|rg| (rg.start, rg.length)) else { return };
    let Some((notes, _)) = with_store(ctx, |s| {
        let seed = s.seed;
        s.patterns.get_mut(&(t, r)).map(|e| {
            e.written = e.pattern.render(len, seed);
            (e.written.clone(), ())
        })
    }) else {
        return;
    };
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        rg.notes = notes.iter().map(|n| Note { pos: n.pos + start, ..*n }).collect();
    }
    app.selected_notes.clear();
}

// ---- drawing ------------------------------------------------------------------------------

const TOOLBAR_H: f32 = 40.0;
const PATTERN_H: f32 = 40.0;
const ROW_H: f32 = 48.0;
/// Logic's row header width (kRowHeaderWidthKey 367), narrower in a narrow editor.
const HEADER_W: f32 = 358.0;

fn grid_bg() -> Color32 {
    Color32::from_rgb(0x14, 0x14, 0x14)
}
/// Row colour (Logic's default purple) and steps on.
fn row_color() -> Color32 {
    Color32::from_rgb(0x5a, 0x41, 0xce)
}
/// A drum row's lit-step colour by its kit piece, sampled from Logic 13's Drum Machine Designer
/// pattern (the guide's Step Sequencer figure): kicks magenta, snares orange, hi-hats cyan, toms
/// green, all fully saturated; claps, cymbals and other percussion follow the same style.
pub fn kit_color(name: &str) -> Color32 {
    let n = name.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if has(&["kick", "bass drum"]) {
        Color32::from_rgb(0xdb, 0x00, 0xa9)
    } else if has(&["snare", "rim", "stick"]) {
        Color32::from_rgb(0xd6, 0x77, 0x00)
    } else if has(&["clap", "snap"]) {
        Color32::from_rgb(0xd6, 0xb8, 0x00)
    } else if has(&["hat", "hh"]) {
        Color32::from_rgb(0x00, 0xcd, 0xcb)
    } else if has(&["tom"]) {
        Color32::from_rgb(0x00, 0xcd, 0x00)
    } else if has(&["crash", "ride", "china", "splash", "cymbal"]) {
        Color32::from_rgb(0x00, 0x7a, 0xff)
    } else if n.is_empty() {
        row_color()
    } else {
        Color32::from_rgb(0xa8, 0x3c, 0xf0)
    }
}

/// A row's unlit-step tint: Logic's off steps are the lit colour at about 17% over a near-black
/// base (sampled: magenta 340f2b, orange 32240f, purple 201b33; cyan 023130 and green 033114 keep
/// less of the base).
pub fn off_color(on: Color32) -> Color32 {
    match on.to_array() {
        [0x00, 0xcd, 0xcb, _] => return Color32::from_rgb(0x02, 0x31, 0x30),
        [0x00, 0xcd, 0x00, _] => return Color32::from_rgb(0x03, 0x31, 0x14),
        _ => {}
    }
    let ch = |c: u8| (c as f32 * 0.169 + 15.0).round() as u8;
    Color32::from_rgb(ch(on.r()), ch(on.g()), ch(on.b()))
}

fn cell_on() -> Color32 {
    Color32::from_rgb(0x8a, 0x70, 0xf2)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::header_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some((t, r)) = app.selected_region else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select a pattern region", look::regular(14.0), look::text_dim());
        return;
    };
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)?.regions.get(r)).cloned() else { return };
    // a region the Step Sequencer hasn't seen, or whose notes changed elsewhere: (re)read
    let rel: Vec<Note> = region.notes.iter().map(|n| Note { pos: n.pos.saturating_sub(region.start), ..*n }).collect();
    let kit = kit_of(app, t);
    let drums = kit.is_some();
    with_store(&ctx, |s| {
        let e = s.patterns.entry((t, r)).or_default();
        let mut sorted = rel.clone();
        sorted.sort_by_key(|n| (n.pos, std::cmp::Reverse(n.pitch)));
        if e.written != sorted && !(e.written.is_empty() && sorted.is_empty() && !e.pattern.rows.is_empty()) {
            let mut fresh = StepPattern::from_notes(&rel, region.length, drums);
            if let Some(kit) = &kit {
                name_rows(&mut fresh, kit);
            }
            // keep rows (and their names, mute, solo) the notes don't mention
            for old in &e.pattern.rows {
                if let Some(nr) = fresh.rows.iter_mut().find(|x| x.pitch == old.pitch) {
                    nr.name = old.name.clone();
                    nr.mute = old.mute;
                    nr.solo = old.solo;
                } else if !rel.is_empty() {
                    let mut keep = old.clone();
                    keep.steps.iter_mut().for_each(|x| x.on = false);
                    if !(keep.mute) {
                        fresh.rows.push(keep);
                    }
                }
            }
            fresh.rows.sort_by(|a, b| b.pitch.cmp(&a.pitch));
            e.pattern = fresh;
            e.written = sorted;
        }
    });
    let mut pat = with_store(&ctx, |s| s.patterns.get(&(t, r)).map(|e| e.pattern.clone())).unwrap_or_default();
    let before = pat.clone();

    // the tab (a pattern region has only the Step Sequencer), Logic's tab capsule
    crate::edpane::draw_tabs(ui, Rect::from_min_size(area.min, vec2(area.width(), 40.0)), &["Step Sequencer"], 0);
    let bar = Rect::from_min_size(pos2(area.min.x, area.min.y + 40.0), vec2(area.width(), TOOLBAR_H));
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::mix(look::header_bg(), Color32::BLACK, 0.35)));
    let mut sel_row = with_store(&ctx, |s| s.selected_row).min(pat.rows.len().saturating_sub(1));
    toolbar(app, ui, bar, t, pat.rows.iter().any(|r| r.steps.iter().any(|s| s.on)));
    // the Pattern Browser on the left
    let area = if with_store(&ctx, |s| s.browser) {
        let br = Rect::from_min_max(pos2(area.min.x, bar.max.y + 1.0), pos2(area.min.x + 200.0, area.max.y));
        browser(app, ui, br);
        Rect::from_min_max(pos2(br.max.x + 1.0, area.min.y), area.max)
    } else {
        area
    };
    // the local inspector (Pattern / Row / Step)
    let area = if with_store(&ctx, |s| s.inspector) && area.width() > crate::stepinsp::WIDTH + 300.0 {
        let ir = Rect::from_min_max(pos2(area.min.x, bar.max.y + 1.0), pos2(area.min.x + crate::stepinsp::WIDTH, area.max.y));
        let (mut tab, rows, cells) = with_store(&ctx, |s| (s.insp_tab, sel_rows(s, pat.rows.len()), sel_cells(s, pat.rows.len())));
        let project_key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_else(|| "C major".into());
        let changed = ui.scope_builder(egui::UiBuilder::new().id_salt(("lg-ss-insp", t, r)).max_rect(ir), |ui| crate::stepinsp::show(ui, ir, &mut tab, &mut pat, &rows, &cells, &project_key)).inner;
        let down = ui.input(|i| i.pointer.primary_down());
        let first = with_store(&ctx, |s| {
            s.insp_tab = tab;
            let first = changed.is_some() && !s.insp_active;
            s.insp_active = down && (changed.is_some() || s.insp_active);
            first
        });
        if let (true, Some(what)) = (first, changed) {
            app.checkpoint(what);
        }
        Rect::from_min_max(pos2(ir.max.x + 1.0, area.min.y), area.max)
    } else {
        area
    };
    // the pattern header: + (add row), rate, playback, rotate, transpose, randomize, clear; key
    // and the pattern length on the right
    let ph = Rect::from_min_size(pos2(area.min.x, bar.max.y), vec2(area.width(), PATTERN_H));
    let head_x = area.min.x;
    let header_w = (area.width() * 0.42).clamp(280.0, HEADER_W);
    // (narrow: the playback box leaves, then rotate; Logic's full header is 358 pt)
    let compact = header_w < 355.0;
    let tiny = header_w < 310.0;
    let mut actions: Vec<Action> = Vec::new();
    {
        let hr = Rect::from_min_max(ph.min, pos2(head_x + header_w, ph.max.y));
        p.rect_filled(hr, 0.0, look::header_bg());
        let cy = hr.center().y;
        let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x, cy - 12.0), vec2(w, 24.0));
        // + ⌄: the Add Row pop-up; in Learn mode a red (Add) or amber (Assign) Learn button
        // takes its place, and a click on it ends Learn
        let add = cell(hr.min.x + 10.5, 41.0);
        let learn = with_store(&ctx, |s| s.learn);
        if let Some(adding) = learn {
            let lr = cell(hr.min.x + 10.5, 56.0);
            let fill = if adding { look::record_red() } else { Color32::from_rgb(0xe8, 0xa3, 0x2e) };
            if bar_btn(ui, lr, ui.scope_id().with("ss-learn"), fill, if adding { "Learn (Add): play a note" } else { "Learn (Assign): play a note" }, true).clicked() {
                with_store(&ctx, |s| s.learn = None);
            }
            p.text(lr.center(), Align2::CENTER_CENTER, "Learn", look::semibold(13.0), Color32::WHITE);
        } else {
        p.rect_filled(add, 6.0, look::group_bg());
        ui.scope_builder(egui::UiBuilder::new().max_rect(add), |ui| {
            plain(ui);
            let pp = ui.painter();
            // + and ⌄ (Logic's: a thin plus, the chevron smaller)
            let pc = pos2(add.min.x + 13.0, add.center().y);
            pp.line_segment([pos2(pc.x - 5.5, pc.y), pos2(pc.x + 5.5, pc.y)], Stroke::new(1.3, look::text()));
            pp.line_segment([pos2(pc.x, pc.y - 5.5), pos2(pc.x, pc.y + 5.5)], Stroke::new(1.3, look::text()));
            pp.line(vec![pos2(add.max.x - 14.5, add.center().y - 0.5), pos2(add.max.x - 11.5, add.center().y + 2.5), pos2(add.max.x - 8.5, add.center().y - 0.5)], Stroke::new(1.2, look::text()));
            ui.menu_button(egui::RichText::new("          ").size(13.0), |ui| {
                ui.set_min_width(160.0);
                let used: Vec<u8> = pat.rows.iter().map(|r| r.pitch).collect();
                let list: Vec<u8> = if drums { (35u8..=59).collect() } else { (36u8..=84).rev().collect() };
                ui.menu_button("Notes", |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for p in list.into_iter().filter(|p| !used.contains(p)) {
                            let name = drums.then(|| drum_name(p)).flatten().map(str::to_string).unwrap_or_else(|| crate::app::note_name(p));
                            if ui.button(name.clone()).clicked() {
                                actions.push(Action::AddRow(name, p));
                                ui.close();
                            }
                        }
                    });
                });
                ui.separator();
                for (label, adding) in [("Learn (Add)", true), ("Learn (Assign)", false)] {
                    if ui.button(label).clicked() {
                        with_store(ui.ctx(), |s| s.learn = Some(adding));
                        ui.close();
                    }
                }
            });
        });
        }
        // (the pattern's controls line up with the rows' below)
        let mut x = hr.min.x + 105.5;
        rate_box(ui, cell(x, 55.0), ("pat-rate", 0), pat.rate, look::group_bg(), &mut |v| actions.push(Action::PatternRate(v)));
        x += 64.0;
        if !compact {
            play_box(ui, cell(x, 38.0), ("pat-play", 0), pat.play, look::group_bg(), &mut |m| actions.push(Action::Play(None, m)));
            x += 46.5;
        }
        if !tiny {
        let rot = cell(x, 39.0);
        pill_pair(&p, rot, look::group_bg());
        for (k, (s, d)) in [(Glyph::RotLeft, -1i32), (Glyph::RotRight, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(rot.min.x + k as f32 * rot.width() / 2.0, rot.min.y), vec2(rot.width() / 2.0, rot.height()));
            glyph(&p, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("pat-rot", k)), Sense::click()).on_hover_text(if *d < 0 { "Rotate Pattern Left" } else { "Rotate Pattern Right" }).clicked() {
                actions.push(Action::Rotate(None, *d));
            }
        }
        x += 47.0;
        }
        let tr = cell(x, 39.0);
        pill_pair(&p, tr, look::group_bg());
        for (k, (s, d)) in [(Glyph::Down, -1i32), (Glyph::Up, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(tr.min.x + k as f32 * tr.width() / 2.0, tr.min.y), vec2(tr.width() / 2.0, tr.height()));
            glyph(&p, rr, *s, look::text());
            let resp = ui.interact(rr, ui.scope_id().with("lg-ss").with(("pat-tr", k)), Sense::click_and_drag()).on_hover_text(if *d < 0 { "Decrement Value" } else { "Increment Value" });
            if let Some(n) = nudge_clicks(&ui.ctx().clone(), &resp, ("pat-tr", 0), *d) {
                actions.push(Action::Nudge(None, n));
            }
        }
        x += 47.5;
        let rb = cell(x, 18.0);
        let rr = small_button_resp(ui, &p, rb, Glyph::Dice, ("pat-rand", 0), "Randomize Current Edit Mode for Pattern…", look::group_bg());
        if rr.secondary_clicked() || rr.clicked() && ui.input(|i| i.modifiers.ctrl) {
            with_store(&ctx, |s| s.rand_dlg = Some((None, rb.left_bottom())));
        } else if rr.clicked() {
            actions.push(Action::Randomize(None));
        }
        x += 26.0;
        let any_on = pat.rows.iter().any(|r| r.steps.iter().any(|s| s.on));
        if clear_button(ui, &p, cell(x, 18.0), ("pat-clear", 0), "Clear Pattern", any_on) {
            actions.push(Action::Clear(None));
        }
        // right: key, Q, steps
        let right = Rect::from_min_max(pos2(hr.max.x, ph.min.y), ph.max);
        p.rect_filled(right, 0.0, look::header_bg());
        // (the headers' edge runs up through the pattern row, as Logic's)
        p.vline(hr.max.x, ph.y_range(), Stroke::new(1.0, Color32::from_gray(0x10)));
        let key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_else(|| "C major".into());
        let mut kx = right.max.x - 12.0;
        let pages_w = if pat.steps > 16 { (pat.steps as usize).div_ceil(16) as f32 * 18.0 + 24.0 } else { 0.0 };
        let left_edge = right.min.x + 10.0 + pages_w;
        let steps_r = Rect::from_min_max(pos2((kx - 121.0).max(left_edge), cy - 12.0), pos2(kx, cy + 12.0));
        // (a narrow editor: "16" alone, and no box at all rather than one cut off)
        let steps_label = if steps_r.width() < 100.0 { format!("{}   ", pat.steps) } else { format!("{} Steps    ", pat.steps) };
        if steps_r.width() >= 44.0 {
        ui.scope_builder(egui::UiBuilder::new().max_rect(steps_r), |ui| {
            plain(ui);
            ui.painter().rect_filled(steps_r, 7.0, look::group_bg());
            crate::glyphs::paint(ui.painter(), Rect::from_min_max(pos2(steps_r.max.x - 22.0, steps_r.min.y), steps_r.max), crate::glyphs::ph::CARET_UP_DOWN, 15.0, look::text());
            ui.menu_button(egui::RichText::new(steps_label).font(look::semibold(13.0)).color(look::text()), |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (k, (title, lens)) in LENGTH_SECTIONS.iter().enumerate() {
                        if k > 0 {
                            ui.separator();
                        }
                        ui.label(egui::RichText::new(*title).size(11.5).color(look::text_dim()));
                        for &n in lens.iter() {
                            if ui.selectable_label(n == pat.steps, format!("{n} Steps")).clicked() {
                                actions.push(Action::Length(None, n));
                                ui.close();
                            }
                        }
                    }
                });
            });
        });
        }
        kx = steps_r.min.x - 12.0;
        let q = Rect::from_min_max(pos2(kx - 30.0, cy - 12.0), pos2(kx, cy + 12.0));
        if q.min.x > left_edge {
            p.rect_filled(q, 7.0, look::group_bg());
            p.text(q.center(), Align2::CENTER_CENTER, "Q", look::regular(13.0), look::text_dim());
        }
        kx = q.min.x - 12.0;
        let kr = Rect::from_min_max(pos2((kx - 176.0).max(left_edge), cy - 12.0), pos2(kx, cy + 12.0));
        if kr.width() > 150.0 {
            p.rect_filled(kr, 7.0, look::group_bg());
            p.text(kr.center(), Align2::CENTER_CENTER, scale_label(&key), look::semibold(13.0), look::text());
        }
        // pages (patterns longer than 16 steps)
        let pages = (pat.steps as usize).div_ceil(16);
        if pages > 1 {
            let mut page = with_store(&ctx, |s| s.page).min(pages - 1);
            for k in 0..pages {
                let c = pos2(right.min.x + 24.0 + k as f32 * 18.0, cy);
                let rr = Rect::from_center_size(c, vec2(16.0, 16.0));
                p.circle_filled(c, 4.5, if k == page { look::text() } else { look::text_dim().gamma_multiply(0.5) });
                let resp = ui.interact(rr, ui.scope_id().with("lg-ss").with(("page", k)), Sense::click()).on_hover_text(format!("Steps {}–{}", k * 16 + 1, ((k + 1) * 16).min(pat.steps as usize)));
                if resp.clicked() {
                    page = k;
                }
                // the page overview's shortcut menu
                let has_clip = with_store(&ctx, |s| !s.clip_page.is_empty());
                resp.context_menu(|ui| {
                    for (name, op, on) in [("Cut", PageOp::Cut, true), ("Copy", PageOp::Copy, true), ("Paste", PageOp::Paste, has_clip), ("Clear", PageOp::Clear, true)] {
                        if ui.add_enabled(on, egui::Button::new(name)).clicked() {
                            actions.push(Action::Page(k, op));
                            ui.close();
                        }
                    }
                });
            }
            with_store(&ctx, |s| s.page = page);
        } else {
            with_store(&ctx, |s| s.page = 0);
        }
    }
    let page = with_store(&ctx, |s| s.page);
    // rows
    let rows_top = ph.max.y;
    let body = Rect::from_min_max(pos2(area.min.x, rows_top), area.max);
    let headers = Rect::from_min_max(body.min, pos2(head_x + header_w, body.max.y));
    let grid = Rect::from_min_max(pos2(headers.max.x, body.min.y), body.max);
    p.rect_filled(headers, 0.0, look::pane_bg());
    p.rect_filled(grid, 0.0, grid_bg());
    p.vline(headers.max.x, body.y_range(), Stroke::new(1.0, Color32::from_gray(0x10)));
    // the rows' height: the Vertical Zoom slider's, or with Vertical Auto Zoom what fills the
    // editor (the slider follows)
    let rh = if with_store(&ctx, |s| s.v_auto) {
        let extra: f32 = pat.rows.iter().map(|r| row_h(r, 0.0)).sum();
        let h = auto_row_h(pat.rows.len(), extra, body.height() - 4.0);
        with_store(&ctx, |s| s.v_zoom = Some(zoom_of_row_h(h)));
        h
    } else {
        row_h_of_zoom(with_store(&ctx, |s| s.v_zoom.unwrap_or(DEFAULT_VZ)))
    };
    // scroll the rows with the wheel
    let scroll_id = egui::Id::unique(("lg-stepseq-scroll", t, r));
    let mut scroll: f32 = ctx.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| body.contains(h)) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        let total: f32 = pat.rows.iter().map(|r| row_h(r, rh)).sum();
        let max = (total - body.height() + 10.0).max(0.0);
        scroll = (scroll - dy).clamp(0.0, max);
    }
    ctx.data_mut(|d| d.insert_temp(scroll_id, scroll));
    let hp = ui.painter_at(headers);
    let gp = ui.painter_at(grid);
    let visible = (pat.steps as usize - page * 16).min(16);
    // (Logic's: 2 pt between steps, 6.5 between groups of four, each step outlined in black)
    let gap_s = 2.0;
    let gap_g = 6.5;
    let gx0 = grid.min.x + 11.0;
    let gw = (grid.max.x - 9.5 - gx0).max(100.0);
    let fill_w = ((gw - gap_g * 3.0 - gap_s * 12.0) / 16.0).max(8.0);
    // Step Width: the minimum, or the maximum (the steps fill the grid)
    let cell_w = if with_store(&ctx, |s| s.narrow_steps) { fill_w.min(NARROW_STEP_W) } else { fill_w };
    let cell_x = |k: usize| gx0 + k as f32 * (cell_w + gap_s) + (k / 4) as f32 * (gap_g - gap_s);
    // the playing step
    // (the project's playhead, or the Preview button's own loop through the pattern)
    let preview_tick = with_store(&ctx, |s| s.preview.as_ref().map(|pv| pv.start)).map(|start| {
        let ticks_per_s = app.tempo() / 60.0 * 960.0;
        let len = (pat.rate.max(1) * pat.steps.max(1) as u32).max(1) as u64;
        ((ui.input(|i| i.time) - start).max(0.0) * ticks_per_s) as u64 % len
    });
    let (playing_tick, in_region) = match preview_tick {
        Some(t) => (t as i64, true),
        None => {
            let t = app.position() as i64 - region.start as i64;
            (t, t >= 0 && (t as u32) < region.length && app.engine.lock().map(|e| e.playing).unwrap_or(false))
        }
    };
    if preview_tick.is_some() {
        ui.ctx().request_repaint();
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let (down, released) = ui.input(|i| (i.pointer.primary_down(), i.pointer.any_released()));
    let mut press = with_store(&ctx, |s| s.press);
    let mut press_tie = with_store(&ctx, |s| s.press_tie);
    let mut ramp = with_store(&ctx, |s| s.ramp);
    let (alt, command, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.modifiers.shift));
    let mut pressed_cells = with_store(&ctx, |s| s.pressed_cells.clone());
    let mut changed = false;
    let (sel, by_region, mono) = with_store(&ctx, |s| (s.sel, s.by_region, s.mono));
    let mut sel_click: Option<(usize, usize)> = None;
    let region_col = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(cell_on(), |tr| look::region_color(tr.kind));
    let mut ys = Vec::with_capacity(pat.rows.len());
    let mut acc = rows_top - scroll;
    for row in &pat.rows {
        ys.push(acc);
        acc += row_h(row, rh);
    }
    for (ri, row) in pat.rows.clone().iter().enumerate() {
        let y = ys[ri];
        // the row's colour (Logic's: purple for notes; drums by kit piece), its steps on in a
        // lighter shade and off in a dark tint of it
        let rc = if drums { kit_color(&row.name) } else { row_color() };
        let on_c = if drums { rc } else { look::mix(rc, Color32::WHITE, 0.06) };
        let off_c = off_color(rc);
        if y + row_h(row, rh) < body.min.y || y > body.max.y {
            continue;
        }
        let hr = Rect::from_min_size(pos2(headers.min.x, y), vec2(header_w, rh));
        let mode = with_store(&ctx, |s| s.modes.get(&(t, r, ri)).copied().unwrap_or_default());
        // header
        if ri == sel_row {
            hp.rect_filled(hr, 0.0, Color32::from_gray(0x40));
        }
        hp.hline(hr.x_range(), hr.max.y - 0.5, Stroke::new(1.0, Color32::from_gray(0x1a)));
        let sel_resp = ui.interact(hr, ui.scope_id().with("lg-ss").with(("rowhdr", ri)), Sense::click_and_drag());
        // dragging a row header up or down moves the row (and ends the automatic sort order)
        if sel_resp.dragged() {
            if let Some(pp) = sel_resp.interact_pointer_pos() {
                let to = ys.iter().rposition(|&yy| yy <= pp.y).unwrap_or(0);
                let ly = if to > ri { ys[to] + row_h(&pat.rows[to], rh) } else { ys[to] };
                hp.hline(hr.x_range(), ly, Stroke::new(2.0, look::accent()));
            }
        }
        if sel_resp.drag_stopped() {
            if let Some(pp) = sel_resp.interact_pointer_pos() {
                let to = ys.iter().rposition(|&yy| yy <= pp.y).unwrap_or(0);
                if to != ri {
                    actions.push(Action::MoveRow(ri, to));
                }
            }
        }
        if sel_resp.clicked() || sel_resp.secondary_clicked() {
            sel_row = ri;
        }
        if sel_resp.double_clicked() {
            with_store(&ctx, |s| s.typing = Some((Typing::Rename(ri), row.name.clone())));
        }
        crate::menus::context_menu(app, &sel_resp, "stepseq-row-header");
        // (Logic's: the disclosure › at 8 pt, the round 22 pt icon a little above the middle)
        let ic = pos2(hr.min.x + 31.75, hr.min.y + (rh / 2.0 - 2.25).max(12.0));
        // the disclosure arrow: shows / hides the row's subrows (Option-click: every row's),
        // turning to ⌄ while open
        let dr = Rect::from_center_size(pos2(hr.min.x + 8.0, ic.y), vec2(16.0, 22.0));
        let dresp = ui.interact(dr, ui.scope_id().with("lg-ss").with(("disc", ri)), Sense::click()).on_hover_text(if row.sub { "Hide Subrows" } else { "Show Subrows" });
        let dcol = if dresp.hovered() { look::text() } else { look::text_dim() };
        if row.sub {
            crate::stepicons::caret(&hp, dr.center(), false, dcol);
        } else {
            hp.text(dr.center(), Align2::CENTER_CENTER, "›", look::regular(18.0), dcol);
        }
        if dresp.clicked() {
            actions.push(Action::Subrows(if alt { None } else { Some(ri) }, !row.sub));
        }
        // a drum row: its kit piece in the row's colour, no circle (Logic's); a note row: ♩ on
        // the purple circle
        if drums {
            crate::stepicons::kit_piece(&hp, ic, &row.name, look::mix(rc, Color32::BLACK, 0.12));
        } else {
            hp.circle_filled(ic, 11.0, rc);
            hp.text(ic, Align2::CENTER_CENTER, "♩", look::regular(15.0), Color32::from_gray(0x16));
        }
        // short rows (below Logic's 48 pt) put everything on one line, centred on the steps
        let one_line = rh < ROW_H - 2.0;
        let line1 = if one_line { (hr.min.y + rh / 2.0 - 2.0).max(hr.min.y + 10.0) } else { hr.min.y + 10.0 };
        let name_pos = pos2(hr.min.x + 55.0, line1);
        let name_r = hp.text(name_pos, Align2::LEFT_CENTER, &row.name, look::semibold(14.0), look::text());
        // the name with its ⌃⌄: the Row Assignment pop-up (a drum kit's rows: its kit pieces;
        // every row: Notes, by octave)
        let ar = Rect::from_min_size(pos2(name_r.max.x + 4.0, name_r.min.y - 2.0), vec2(14.0, 20.0));
        glyph(&hp, ar, Glyph::UpDown, look::text());
        let ar_resp = ui.interact(name_r.union(ar), ui.scope_id().with("lg-ss").with(("rownote", ri)), Sense::click()).on_hover_text("Row Assignment");
        egui::Popup::menu(&ar_resp).show(|ui| {
            if let Some(p) = assignment_menu(ui, row.pitch, drums) {
                actions.push(Action::Assign(ri, p, drums));
            }
        });
        // the row's edit mode
        let mr = Rect::from_min_max(pos2(hr.max.x - 130.0, line1 - 9.0), pos2(hr.max.x - 1.5, line1 + 11.0));
        let mode_w = hp.layout_no_wrap(mode.name().to_string(), look::regular(12.0), look::text()).size().x;
        ui.scope_builder(egui::UiBuilder::new().max_rect(mr).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            plain(ui);
            ui.spacing_mut().button_padding = vec2(0.0, 0.0);
            ui.menu_button(egui::RichText::new(mode.name()).size(12.0).color(look::text()), |ui| {
                for m in Mode::ALL {
                    if ui.selectable_label(m == mode, m.name()).clicked() {
                        actions.push(Action::Mode(ri, m));
                        ui.close();
                    }
                }
            });
        });
        // the controls: M S, rate, playback, rotate, octave, random, clear — on a second line in
        // rows of Logic's 48 pt, else on the name's line between the name and the edit mode.
        // Where they don't all fit, the rate leaves first, then playback, then rotate (as Logic's
        // narrow header); M S, octave, random and clear always stay.
        {
        let (ly, mut x, right) = if one_line {
            // (in one column on every row, after the longest usual names)
            (line1, (ar.max.x + 8.0).max(hr.min.x + 136.0), hr.max.x - 1.5 - mode_w - 8.0)
        } else {
            (hr.min.y + 32.0, hr.min.x + 57.0, hr.max.x - 1.0)
        };
        let mut room = right - x - (46.0 + 46.0 + 26.0 + 22.0);
        let mut fits = |w: f32| {
            let ok = room >= w;
            if ok {
                room -= w;
            }
            ok
        };
        let show_rot = fits(47.0);
        let show_play = show_rot && fits(48.0);
        let show_rate = show_play && fits(64.0);
        let ms = Rect::from_min_size(pos2(x, ly - 10.0), vec2(41.0, 20.0));
        hp.rect_filled(ms, 5.0, Color32::from_gray(0x4f));
        for (k, (lbl, on)) in [("M", row.mute), ("S", row.solo)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(ms.min.x + k as f32 * 20.5, ms.min.y), vec2(20.5, 20.0));
            if *on {
                hp.rect_filled(rr.shrink(1.0), 4.0, if k == 0 { Color32::from_rgb(0x2e, 0x8b, 0xe8) } else { Color32::from_rgb(0xe8, 0xc3, 0x2e) });
            }
            hp.text(rr.center(), Align2::CENTER_CENTER, *lbl, look::semibold(13.0), if *on { Color32::BLACK } else { look::text() });
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("ms", ri, k)), Sense::click()).on_hover_text(if k == 0 { "Mute Row" } else { "Solo Row" }).clicked() {
                actions.push(if k == 0 { Action::Mute(ri) } else { Action::Solo(ri) });
            }
        }
        hp.vline(ms.center().x, ms.y_range().shrink(5.0), Stroke::new(1.0, Color32::from_gray(0x3a)));
        x += 46.0;
        if show_rate {
            let rate_r = Rect::from_min_size(pos2(x, ly - 10.0), vec2(60.0, 20.0));
            let rate = row.rate.unwrap_or(pat.rate);
            rate_box(ui, rate_r, ("row-rate", ri), rate, Color32::from_gray(0x5f), &mut |v| actions.push(Action::RowRate(ri, v)));
            x += 64.0;
        }
        if show_play {
            play_box(ui, Rect::from_min_size(pos2(x, ly - 10.0), vec2(43.0, 20.0)), ("row-play", ri), row.play.unwrap_or(pat.play), Color32::from_gray(0x5f), &mut |m| actions.push(Action::Play(Some(ri), m)));
            x += 48.0;
        }
        if show_rot {
        let rot = Rect::from_min_size(pos2(x, ly - 10.0), vec2(41.0, 20.0));
        pill_pair(&hp, rot, Color32::from_gray(0x4f));
        for (k, (s, d)) in [(Glyph::RotLeft, -1i32), (Glyph::RotRight, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(rot.min.x + k as f32 * 20.5, rot.min.y), vec2(20.5, 20.0));
            glyph(&hp, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("rot", ri, k)), Sense::click()).on_hover_text(if *d < 0 { "Rotate Row Left" } else { "Rotate Row Right" }).clicked() {
                actions.push(Action::Rotate(Some(ri), *d));
            }
        }
        x += 47.0;
        }
        let oc = Rect::from_min_size(pos2(x, ly - 10.0), vec2(41.0, 20.0));
        pill_pair(&hp, oc, Color32::from_gray(0x4f));
        for (k, (s, d)) in [(Glyph::Down, -1i32), (Glyph::Up, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(oc.min.x + k as f32 * 20.5, oc.min.y), vec2(20.5, 20.0));
            glyph(&hp, rr, *s, look::text());
            let resp = ui.interact(rr, ui.scope_id().with("lg-ss").with(("oct", ri, k)), Sense::click_and_drag()).on_hover_text(if *d < 0 { "Decrement Value" } else { "Increment Value" });
            if let Some(n) = nudge_clicks(&ui.ctx().clone(), &resp, ("oct", ri), *d) {
                actions.push(Action::Nudge(Some(ri), n));
            }
        }
        x += 46.0;
        let rb = Rect::from_min_size(pos2(x, ly - 10.0), vec2(22.0, 20.0));
        let rr = small_button_resp(ui, &hp, rb, Glyph::Dice, ("rand", ri), "Randomize Current Edit Mode for Selected Rows…", Color32::from_gray(0x5f));
        if rr.secondary_clicked() || rr.clicked() && ui.input(|i| i.modifiers.ctrl) {
            with_store(&ctx, |s| s.rand_dlg = Some((Some(ri), rb.left_bottom())));
        } else if rr.clicked() {
            actions.push(Action::Randomize(Some(ri)));
        }
        x += 26.0;
        if clear_button(ui, &hp, Rect::from_min_size(pos2(x, ly - 10.0), vec2(22.0, 20.0)), ("clr", ri), "Clear Row", row.steps.iter().any(|s| s.on)) {
            actions.push(Action::Clear(Some(ri)));
        }
        }
        // the row's steps
        let n = pat.row_len(row);
        let gy = Rect::from_min_size(pos2(grid.min.x, y), vec2(grid.width(), rh));
        let muted = row.mute || (pat.rows.iter().any(|x| x.solo) && !row.solo);
        for k in 0..visible {
            let si = page * 16 + k;
            let cr = Rect::from_min_size(pos2(cell_x(k), y + 3.0), vec2(cell_w, rh - 10.0));
            if !cr.intersects(grid) {
                continue;
            }
            let s = row.steps[si];
            let beyond = si >= n || si < row.start as usize;
            let mut bg = off_c;
            if beyond {
                bg = Color32::from_rgb(0x1a, 0x18, 0x22);
            }
            gp.rect_filled(cr, 2.0, bg);
            gp.rect_stroke(cr, 2.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Outside);
            let base_on = if by_region { region_col } else { on_c };
            let on_col = if muted { look::mix(base_on, off_c, 0.55) } else { base_on };
            match mode {
                Mode::OnOff | Mode::Loop => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, on_col);
                    }
                }
                Mode::Tie => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, off_c, 0.5));
                    }
                    if s.tie {
                        gp.rect_filled(Rect::from_min_max(pos2(cr.min.x - gap_g, cr.center().y - 4.0), pos2(cr.max.x - 4.0, cr.center().y + 4.0)), 2.0, on_col);
                    }
                }
                Mode::Skip => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, off_c, 0.5));
                    }
                    if s.skip {
                        let st = Stroke::new(2.0, Color32::from_gray(0xd0));
                        let q = cr.shrink(cr.width().min(cr.height()) * 0.25);
                        gp.line_segment([q.left_top(), q.right_bottom()], st);
                        gp.line_segment([q.left_bottom(), q.right_top()], st);
                    }
                }
                Mode::Gate | Mode::Offset => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, off_c, 0.6));
                        let f = mode.get(&s);
                        if mode == Mode::Gate {
                            gp.rect_filled(Rect::from_min_size(cr.min, vec2(cr.width() * f, cr.height())), 3.0, on_col);
                        } else {
                            let x = cr.min.x + cr.width() * f;
                            gp.vline(x, cr.y_range(), Stroke::new(3.0, on_col));
                        }
                    }
                }
                Mode::Repeat => {
                    if s.on {
                        let reps = s.repeat.max(1) as f32;
                        let w = (cr.width() - (reps - 1.0) * 2.0) / reps;
                        for j in 0..s.repeat.max(1) {
                            gp.rect_filled(Rect::from_min_size(pos2(cr.min.x + j as f32 * (w + 2.0), cr.min.y), vec2(w, cr.height())), 2.0, on_col);
                        }
                    }
                }
                Mode::Velocity | Mode::Chance | Mode::Octave | Mode::Note | Mode::Rate => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, off_c, 0.6));
                        let f = mode.get(&s);
                        if mode == Mode::Octave || mode == Mode::Note {
                            let mid = cr.center().y;
                            let yv = cr.max.y - cr.height() * f;
                            gp.rect_filled(Rect::from_min_max(pos2(cr.min.x, mid.min(yv)), pos2(cr.max.x, mid.max(yv) + 1.0)), 2.0, on_col);
                        } else {
                            gp.rect_filled(Rect::from_min_max(pos2(cr.min.x, cr.max.y - cr.height() * f), cr.max), 3.0, on_col);
                        }
                    }
                }
            }
            // values written in the cells: View ▸ Display Edit Mode Values (Logic's default: the
            // selected row's)
            let show_value = match with_store(&ctx, |st| st.values_display) {
                1 => pressed_cells.contains(&(ri, si)),
                2 => true,
                _ => ri == sel_row,
            };
            if s.on && show_value && cr.width() > 26.0 {
                let txt = mode.text(&s);
                if !txt.is_empty() {
                    gp.text(cr.center(), Align2::CENTER_CENTER, txt, look::semibold(11.0), Color32::WHITE);
                }
            }
            if in_region {
                let rate = row.rate.unwrap_or(pat.rate).max(1);
                if (playing_tick as u32 / rate) as usize % n == si {
                    // the playing step: the column lights up, and a step that plays flashes bright
                    gp.rect_filled(cr, 3.0, Color32::from_white_alpha(if s.on { 90 } else { 22 }));
                    gp.rect_stroke(cr, 3.0, Stroke::new(2.0, Color32::from_gray(0xee)), egui::StrokeKind::Inside);
                }
            }
            if ri == sel_row && k == 0 && pressed_cells.is_empty() && !in_region {
                gp.rect_stroke(cr, 3.0, Stroke::new(1.5, Color32::from_gray(0xd8)), egui::StrokeKind::Inside);
            }
            // a press on a cell starts painting; a drag paints across the row's cells
            let resp = ui.interact(cr, ui.scope_id().with("lg-ss").with(("cell", ri, si)), Sense::click_and_drag());
            crate::menus::context_menu(app, &resp, "stepseq-step-cell");
            // the step selection
            if let Some((a, b)) = sel {
                // (a single clicked step isn't shown as a selection: only a dragged range of steps)
                if a != b && (a.0.min(b.0)..=a.0.max(b.0)).contains(&ri) && (a.1.min(b.1)..=a.1.max(b.1)).contains(&si) {
                    gp.rect_stroke(cr.expand(1.5), 4.0, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc3, 0x2e)), egui::StrokeKind::Outside);
                }
            }
            // Option-click: the step (or every selected step, when it is one of them) back to
            // its defaults, off (Logic's Eraser)
            if alt && !command && resp.clicked() {
                app.checkpoint("Delete Step");
                let cells = match sel {
                    Some((a, b)) if (a.0.min(b.0)..=a.0.max(b.0)).contains(&ri) && (a.1.min(b.1)..=a.1.max(b.1)).contains(&si) => sel_cells_of(a, b, pat.rows.len()),
                    _ => vec![(ri, si)],
                };
                for (rr, ss) in cells {
                    pat.rows[rr].steps[ss] = Step::default();
                }
                changed = true;
                with_store(&ctx, |s| s.changed = true);
            }
            if alt && !command && resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
            }
            if resp.is_pointer_button_down_on() && press.is_none() && down && (!alt || command) {
                press_tie = command && !alt && !matches!(mode, Mode::Tie | Mode::Rate | Mode::Loop);
                // Command-Option-drag: a ramp of values from this step to where the drag ends
                // (with Shift as well, off steps stay off)
                let ramp_ok = command && alt && mode_units(mode, &s).is_some();
                ramp = ramp_ok.then(|| (si, pointer.map_or(0.5, |pp| (cr.max.y - pp.y) / cr.height()).clamp(0.0, 1.0), shift));
                sel_row = ri;
                sel_click = Some((ri, si));
                let paint = match mode {
                    Mode::OnOff => !s.on,
                    Mode::Tie => !s.tie,
                    Mode::Skip => !s.skip,
                    _ => true,
                };
                press = Some((ri, paint));
                pressed_cells.clear();
                app.checkpoint("Change Step");
                // MIDI Out: the step's note sounds while it is edited
                if with_store(&ctx, |s| !s.midi_out_off) && (paint || mode != Mode::OnOff) {
                    let pitch = (row.pitch as i32 + 12 * s.octave as i32 + s.note as i32).clamp(0, 127) as u8;
                    if let Ok(mut e) = app.engine.lock() {
                        e.live(t, pitch, s.velocity.max(1));
                    }
                    with_store(&ctx, |st| st.out_note = Some((t, pitch)));
                }
            }
            if let (Some((pr, paint)), Some(pp)) = (press, pointer) {
                let hot = Rect::from_min_max(pos2(cr.min.x - gap_s, gy.min.y), pos2(cr.max.x + gap_s, gy.max.y));
                if pr == ri && down && hot.contains(pos2(pp.x, (pp.y).clamp(gy.min.y + 1.0, gy.max.y - 1.0))) && hot.x_range().contains(pp.x) {
                    let f = (cr.max.y - pp.y) / cr.height();
                    let first_touch = !pressed_cells.contains(&(ri, si));
                    let st = &mut pat.rows[ri].steps[si];
                    let old = *st;
                    if let Some((s0, f0, keep_off)) = ramp {
                        ramp_values(&mut pat.rows[ri].steps, mode, s0, f0, si, f, keep_off);
                        changed = true;
                    } else if press_tie {
                        // Command-drag: the first step starts the note, the others tie on to it
                        if first_touch {
                            st.on = true;
                            st.tie = !pressed_cells.is_empty();
                        }
                    } else {
                    match mode {
                        Mode::OnOff => {
                            st.on = paint;
                            // Mono Mode: one step per column
                            if paint && mono {
                                for (k, other) in pat.rows.iter_mut().enumerate() {
                                    if k != ri {
                                        other.steps[si].on = false;
                                    }
                                }
                            }
                        }
                        Mode::Tie => {
                            st.tie = paint;
                            if paint {
                                st.on = true;
                            }
                        }
                        Mode::Skip => st.skip = paint,
                        Mode::Loop => {
                            if first_touch {
                                let len = (si + 1) as u8;
                                pat.rows[ri].len = if len == pat.steps { None } else { Some(len) };
                            }
                        }
                        m => {
                            // (changing an off step's value turns it on, as Logic's)
                            m.set(st, f);
                            st.on = true;
                        }
                    }
                    }
                    if pat.rows[ri].steps[si] != old || mode == Mode::Loop && first_touch {
                        changed = true;
                    }
                    if first_touch {
                        pressed_cells.push((ri, si));
                    }
                }
            }
        }
        // the row's end (loop end) and start
        if n < pat.steps as usize && n >= page * 16 && n < page * 16 + visible {
            let x = cell_x(n - page * 16) - gap_s / 2.0 - 1.0;
            gp.vline(x, y + 3.0..=y + rh - 3.0, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc3, 0x2e)));
        }
        let st0 = row.start as usize;
        if st0 > 0 && st0 >= page * 16 && st0 < page * 16 + visible {
            let x = cell_x(st0 - page * 16) - gap_s / 2.0 - 1.0;
            gp.vline(x, y + 3.0..=y + rh - 3.0, Stroke::new(2.0, Color32::from_rgb(0x2e, 0xc3, 0xe8)));
        }
        // subrows: velocity, gate and chance lanes, drawn and edited under the row
        if row.sub {
            for (li, m) in SUB_MODES.iter().enumerate() {
                let ly0 = y + rh + li as f32 * SUB_H;
                let lr = Rect::from_min_size(pos2(headers.min.x, ly0), vec2(header_w, SUB_H));
                hp.text(pos2(lr.max.x - 14.0, lr.center().y), Align2::RIGHT_CENTER, m.name(), look::regular(11.5), look::text_dim());
                hp.hline(lr.x_range(), lr.max.y - 0.5, Stroke::new(1.0, Color32::from_gray(0x1a)));
                for k in 0..visible {
                    let si = page * 16 + k;
                    let cr = Rect::from_min_size(pos2(cell_x(k), ly0 + 3.0), vec2(cell_w, SUB_H - 6.0));
                    let st = pat.rows[ri].steps[si];
                    gp.rect_filled(cr, 2.0, Color32::from_rgb(0x1a, 0x18, 0x22));
                    if st.on {
                        let f = m.get(&st);
                        gp.rect_filled(Rect::from_min_max(pos2(cr.min.x, cr.max.y - cr.height() * f), cr.max), 2.0, look::mix((if by_region { region_col } else { on_c }), off_c, 0.25));
                    }
                    let resp = ui.interact(cr, ui.scope_id().with("lg-ss").with(("sub", ri, li, si)), Sense::click_and_drag());
                    if resp.drag_started() || resp.clicked() {
                        app.checkpoint("Change Step");
                    }
                    if resp.dragged() || resp.clicked() {
                        if let Some(pp) = resp.interact_pointer_pos() {
                            m.set(&mut pat.rows[ri].steps[si], (cr.max.y - pp.y) / cr.height());
                            pat.rows[ri].steps[si].on = true;
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    if released || !down {
        if press.is_some() {
            changed = true;
            with_store(&ctx, |s| s.changed = true);
        }
        if let Some((tt, pitch)) = with_store(&ctx, |s| s.out_note.take()) {
            if let Ok(mut e) = app.engine.lock() {
                e.live(tt, pitch, 0);
            }
        }
        press = None;
        pressed_cells.clear();
    }
    let finishing = with_store(&ctx, |s| {
        if let Some(c) = sel_click {
            s.sel = Some((c, c));
        }
        s.press = press;
        s.press_tie = press_tie;
        s.ramp = if press.is_some() { ramp } else { None };
        s.pressed_cells = pressed_cells.clone();
        s.selected_row = sel_row;
        std::mem::take(&mut s.changed)
    });
    // header and toolbar actions
    let had_actions = !actions.is_empty();
    for a in actions {
        apply(&mut pat, a, &ctx, t, r);
    }
    if pat != before || had_actions {
        with_store(&ctx, |s| {
            if let Some(e) = s.patterns.get_mut(&(t, r)) {
                e.pattern = pat.clone();
            }
        });
        if had_actions && press.is_none() && !finishing {
            app.checkpoint("Change Pattern");
        }
        write_notes(app, &ctx, t, r);
        if !down {
            app.after_edit();
        }
    } else if finishing {
        app.after_edit();
    }
    if changed && !down {
        app.after_edit();
    }
    if pat.rows.is_empty() {
        gp.text(grid.center(), Align2::CENTER_CENTER, "Add a row with +", look::regular(14.0), look::text_dim());
    }
    preview_frame(app, &ctx, &pat);
    // Learn and Live Record listen to the keyboard
    let tick = in_region.then_some(playing_tick as u32 % (pat.rate.max(1) * pat.steps.max(1) as u32));
    listen(app, &ctx, t, r, tick);
    // the Randomize dialog (closes on a click outside it)
    if let Some((rows_of, at)) = with_store(&ctx, |s| s.rand_dlg) {
        let mode = with_store(&ctx, |s| mode_of(s, t, r, rows_of.unwrap_or(s.selected_row)));
        let mut o = with_store(&ctx, |s| s.rand);
        let mut go = false;
        let area = egui::Area::new(egui::Id::new(("lg-ss-rand", t, r))).order(egui::Order::Foreground).fixed_pos(at + vec2(0.0, 4.0)).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(220.0);
                ui.label(egui::RichText::new(format!("Randomize {}", mode.name())).strong());
                ui.add_space(4.0);
                if matches!(mode, Mode::OnOff | Mode::Loop | Mode::Tie | Mode::Skip) {
                    ui.horizontal(|ui| {
                        ui.label("Probability");
                        ui.add(egui::DragValue::new(&mut o.probability).range(0..=100).suffix(" %"));
                    });
                } else {
                    let (_, lo, hi) = mode_units(mode, &Step { on: true, ..Step::default() }).unwrap_or((0, 0, 127));
                    ui.horizontal(|ui| {
                        ui.label("Mode");
                        egui::ComboBox::from_id_salt("lg-ss-rand-mode").selected_text(if o.range_mode { "Range" } else { "Offset" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut o.range_mode, false, "Offset");
                            ui.selectable_value(&mut o.range_mode, true, "Range");
                        });
                    });
                    if o.range_mode {
                        ui.horizontal(|ui| {
                            ui.label("Range");
                            ui.add(egui::DragValue::new(&mut o.low).range(lo..=hi));
                            ui.label("–");
                            ui.add(egui::DragValue::new(&mut o.high).range(lo..=hi));
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Offset");
                            ui.add(egui::DragValue::new(&mut o.offset).range(0..=100).suffix(" %"));
                        });
                    }
                }
                ui.add_space(4.0);
                go = ui.button("Randomize").clicked();
            });
        });
        let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pp| !area.response.rect.contains(pp));
        with_store(&ctx, |s| {
            s.rand = o;
            if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                s.rand_dlg = None;
            }
        });
        if go {
            app.checkpoint("Randomize");
            apply(&mut pat, Action::Randomize(rows_of), &ctx, t, r);
            with_store(&ctx, |s| {
                if let Some(e) = s.patterns.get_mut(&(t, r)) {
                    e.pattern = pat.clone();
                }
            });
            write_notes(app, &ctx, t, r);
            app.after_edit();
        }
    }
    // Rename Row / Save Pattern / Save Template
    if let Some((what, mut text)) = with_store(&ctx, |s| s.typing.clone()) {
        let mut done = None;
        egui::Area::new(egui::Id::unique("lg-ss-typing")).fixed_pos(grid.center() - vec2(110.0, 30.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(match what {
                    Typing::Rename(_) => "Rename Row",
                    Typing::Save(false) => "Save Pattern as",
                    Typing::Save(true) => "Save Template as",
                });
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0)).request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        with_store(&ctx, |s| s.typing = if done.is_some() { None } else { Some((what, text.clone())) });
        let name = text.trim().replace(['/', '\\'], "-");
        if done == Some(true) && !name.is_empty() {
            match what {
                Typing::Rename(k) => {
                    change(app, &ctx, "Rename Row", |p, _, _, _| {
                        if let Some(row) = p.rows.get_mut(k) {
                            row.name = name.clone();
                        }
                    });
                }
                Typing::Save(tpl) => {
                    let path = patterns_dir(tpl).join(format!("{name}.pattern"));
                    match save_file(&path, &pat) {
                        Ok(()) => app.tell(format!("Saved “{name}” to the Pattern Browser")),
                        Err(e) => app.tell(format!("Couldn't save the pattern: {e}")),
                    }
                }
            }
        }
    }
}

const SUB_H: f32 = 30.0;
const SUB_MODES: [Mode; 3] = [Mode::Velocity, Mode::Gate, Mode::Chance];

/// The Vertical Zoom slider's default (Logic's: 48 pt rows) and the row heights it spans.
const DEFAULT_VZ: f32 = 0.6;
const ROW_MIN: f32 = 30.0;
const ROW_MAX: f32 = 96.0;
/// The minimum Step Width (the maximum: the steps fill the grid).
const NARROW_STEP_W: f32 = 24.0;

/// A row's height at the Vertical Zoom slider's value `v` (0..1): 30 pt rows (name only) up
/// to Logic's 48 at the default, then up to 96.
pub fn row_h_of_zoom(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= DEFAULT_VZ {
        ROW_MIN + (ROW_H - ROW_MIN) * v / DEFAULT_VZ
    } else {
        ROW_H + (ROW_MAX - ROW_H) * (v - DEFAULT_VZ) / (1.0 - DEFAULT_VZ)
    }
}

/// The slider value that gives rows `h` points tall (the inverse of row_h_of_zoom).
pub fn zoom_of_row_h(h: f32) -> f32 {
    let h = h.clamp(ROW_MIN, ROW_MAX);
    if h <= ROW_H {
        (h - ROW_MIN) / (ROW_H - ROW_MIN) * DEFAULT_VZ
    } else {
        DEFAULT_VZ + (h - ROW_H) / (ROW_MAX - ROW_H) * (1.0 - DEFAULT_VZ)
    }
}

/// Vertical Auto Zoom: the row height that fits `n` rows (and `extra` points of open subrows)
/// into `height`, within the slider's range.
pub fn auto_row_h(n: usize, extra: f32, height: f32) -> f32 {
    if n == 0 {
        return ROW_H;
    }
    ((height - extra) / n as f32).clamp(ROW_MIN, ROW_MAX)
}

/// The Vertical Zoom slider as Logic's Step Sequencer draws it: a darker pill with ↕, a 4 pt
/// groove (blue up to the knob) and a 16 × 13 pt capsule knob.
fn vzoom_slider(ui: &mut egui::Ui, r: Rect, id: egui::Id, v: &mut f32) -> bool {
    let p = ui.painter().clone();
    let y = r.center().y;
    p.rect_filled(r, 6.0, look::mix(look::header_bg(), Color32::BLACK, 0.2));
    let groove = Rect::from_min_max(pos2(r.min.x + 20.5, y - 2.0), pos2(r.max.x - 4.0, y + 2.0));
    let resp = ui.interact(groove.expand2(vec2(6.0, 8.0)), id, Sense::click_and_drag()).on_hover_text("Vertical Zoom");
    let mut changed = false;
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        *v = ((pp.x - groove.min.x) / groove.width().max(1.0)).clamp(0.0, 1.0);
        changed = true;
    }
    // (the knob stays inside the groove's ends)
    let travel = groove.width() - 16.0;
    let kx = groove.min.x + 8.0 + travel * v.clamp(0.0, 1.0);
    p.rect_filled(groove, 2.0, Color32::from_gray(0x32));
    p.rect_filled(Rect::from_min_max(groove.min, pos2(kx, groove.max.y)), 2.0, Color32::from_rgb(0x06, 0x65, 0xae));
    let knob = Rect::from_center_size(pos2(kx, y), vec2(16.0, 13.0));
    p.rect_filled(knob, 6.5, Color32::from_gray(0xde));
    p.rect_stroke(knob, 6.5, Stroke::new(0.5, Color32::from_black_alpha(60)), egui::StrokeKind::Outside);
    changed
}

/// A menu-bar button (Logic's 24 pt capsule, 6 pt corners): `fill`, lighter on hover when plain.
fn bar_btn(ui: &mut egui::Ui, r: Rect, id: egui::Id, fill: Color32, tip: &str, enabled: bool) -> egui::Response {
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
    let fill = if enabled && resp.hovered() && fill == look::group_bg() { look::button_on() } else { fill };
    ui.painter().rect_filled(r, 6.0, fill);
    resp
}

/// Logic 13's Step Sequencer menu bar (measured on ui-step-sequencer.png, @2x, in points):
/// Edit / Functions / View; Pattern Browser | Inspector in one 60 pt pill; MIDI In and MIDI Out
/// (33 pt, 2.5 apart, green when on); Mono Mode (× over a square); then at the right Live
/// Pattern Recording and Preview (dimmed while off), Vertical Auto Zoom, the Vertical Zoom
/// slider and ↔ with the two Step Width buttons, 12 pt apart. As the bar narrows, controls
/// leave from the right end into its "…" menu (barfit), as the other bars' do.
fn toolbar(app: &mut App, ui: &mut egui::Ui, bar: Rect, t: usize, any_on: bool) {
    use crate::barfit::{any_hidden, fit, more_button, Slot, GAP, MORE_W};
    use crate::stepicons as si;
    let ctx = ui.ctx().clone();
    let id = ui.scope_id().with("ss-bar");
    let cy = bar.center().y;
    let x0 = bar.min.x + 10.0;
    let mut end = x0 + 230.0;
    let menus = Rect::from_min_max(pos2(x0 + 4.0, bar.min.y + 3.0), pos2(bar.max.x - 10.0, bar.max.y - 3.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(menus).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "stepseq-Edit"), ("Functions", "stepseq-Functions"), ("View", "stepseq-View")]);
        end = ui.min_rect().max.x - 8.0;
    });
    const B: f32 = 33.0;
    // each control's width with the gap before it
    let widths = [10.0 + 60.0, 10.0 + 2.0 * B + 2.5, 10.0 + B, 12.0 + B, 12.0 + B, 12.0 + B, 12.0 + 72.0, 30.5 + 60.0];
    let slots: Vec<Slot> = widths.iter().map(|&w| Slot { w, drop: 1 }).collect();
    let shown = fit(&slots, bar.max.x - 10.0 - end - 16.0, MORE_W + GAP);
    let (b_on, i_on, in_on, out_on, mono_on, live_on, previewing, v_auto, vz, narrow) = with_store(&ctx, |s| (s.browser, s.inspector, s.step_rec, !s.midi_out_off, s.mono, s.live_record, s.preview.is_some(), s.v_auto, s.v_zoom.unwrap_or(DEFAULT_VZ), s.narrow_steps));
    let green = Color32::from_rgb(0x22, 0x7c, 0x38);
    let dim_bg = look::mix(look::header_bg(), look::group_bg(), 0.5);
    // (Logic's dimmed icons: grey, about halfway to the button)
    let off_ink = look::mix(look::text_dim(), dim_bg, 0.3);
    let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x, cy - 12.0), vec2(w, 24.0));
    let ink = |on: bool| if on { Color32::WHITE } else { look::text() };
    let mut hits: Vec<&'static str> = Vec::new();
    let mut new_vz: Option<f32> = None;
    let divider = |ui: &egui::Ui, pill: Rect| ui.painter().vline(pill.center().x, pill.y_range().shrink(6.0), Stroke::new(1.0, look::text_dim().gamma_multiply(0.55)));
    // a pill of two halves: (name, on, tip, icon painter)
    let mut lx = end;
    if shown[0] {
        let pill = cell(lx + 10.0, 60.0);
        lx = pill.max.x;
        ui.painter().rect_filled(pill, 6.0, look::group_bg());
        for (k, (name, on, tip)) in [("browser", b_on, "Pattern Browser"), ("inspector", i_on, "Show/Hide Local Inspector")].into_iter().enumerate() {
            let h = Rect::from_min_size(pos2(pill.min.x + k as f32 * 30.0, pill.min.y), vec2(30.0, 24.0));
            let resp = ui.interact(h, id.with(name), Sense::click()).on_hover_text(tip);
            let cr = if k == 0 { egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 } } else { egui::CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 } };
            let fill = if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() };
            ui.painter().rect_filled(h, cr, fill);
            if k == 0 {
                si::browser(ui.painter(), h.center(), ink(on), fill);
            } else {
                si::inspector(ui.painter(), h.center(), ink(on), fill);
            }
            if resp.clicked() {
                hits.push(name);
            }
        }
        if !b_on && !i_on {
            divider(ui, pill);
        }
    }
    if shown[1] {
        for (k, (name, on, tip)) in [("in", in_on, "MIDI In (Step Recording)"), ("out", out_on, "MIDI Out")].into_iter().enumerate() {
            let r = cell(lx + if k == 0 { 10.0 } else { 2.5 }, B);
            lx = r.max.x;
            let resp = bar_btn(ui, r, id.with(name), if on { green } else { look::group_bg() }, tip, true);
            si::midi(ui.painter(), r.center(), k == 1, ink(on));
            if resp.clicked() {
                hits.push(name);
            }
        }
    }
    if shown[2] {
        let r = cell(lx + 10.0, B);
        if bar_btn(ui, r, id.with("mono"), if mono_on { look::accent() } else { look::group_bg() }, "Mono Mode", true).clicked() {
            hits.push("mono");
        }
        si::mono(ui.painter(), r.center(), ink(mono_on));
    }
    // the right-hand group, flush right (before the "…" when something left the bar)
    let hidden = any_hidden(&shown);
    let right_end = if hidden { bar.max.x - 10.0 - MORE_W - GAP } else { bar.max.x - 10.0 };
    let mut rx = right_end - (3..8).filter(|&i| shown[i]).map(|i| widths[i]).sum::<f32>();
    if shown[3] {
        let r = cell(rx + 12.0, B);
        rx = r.max.x;
        if bar_btn(ui, r, id.with("live"), if live_on { look::record_red() } else { dim_bg }, "Live Pattern Recording", true).clicked() {
            hits.push("live");
        }
        si::live(ui.painter(), r.center(), if live_on { Color32::WHITE } else { off_ink });
    }
    if shown[4] {
        let r = cell(rx + 12.0, B);
        rx = r.max.x;
        let enabled = any_on || previewing;
        if bar_btn(ui, r, id.with("preview"), if previewing { green } else { dim_bg }, "Preview", enabled).clicked() {
            hits.push("preview");
        }
        si::preview(ui.painter(), r.center(), if previewing { Color32::WHITE } else if enabled { look::text() } else { off_ink });
    }
    if shown[5] {
        let r = cell(rx + 12.0, B);
        rx = r.max.x;
        if bar_btn(ui, r, id.with("auto"), if v_auto { look::accent() } else { look::group_bg() }, "Vertical Auto Zoom", true).clicked() {
            hits.push("auto");
        }
        si::auto_zoom(ui.painter(), r.center(), ink(v_auto));
    }
    if shown[6] {
        let r = cell(rx + 12.0, 72.0);
        rx = r.max.x;
        let mut v = vz;
        if vzoom_slider(ui, r, id.with("vzoom"), &mut v) {
            new_vz = Some(v);
        }
        si::double_arrow(ui.painter(), pos2(r.min.x + 10.0, cy), true, look::text());
    }
    if shown[7] {
        si::double_arrow(ui.painter(), pos2(rx + 19.0, cy), false, look::text());
        let pill = cell(rx + 30.5, 60.0);
        ui.painter().rect_filled(pill, 6.0, look::group_bg());
        for (k, (name, tip)) in [("narrow", "Minimum Step Width"), ("wide", "Maximum Step Width")].into_iter().enumerate() {
            let h = Rect::from_min_size(pos2(pill.min.x + k as f32 * 30.0, pill.min.y), vec2(30.0, 24.0));
            let resp = ui.interact(h, id.with(name), Sense::click()).on_hover_text(tip);
            // (Logic draws both dimmed; the one not in use lights on hover)
            let lit = resp.hovered() && narrow != (k == 0);
            si::step_width(ui.painter(), h.center(), k == 1, if lit { look::text() } else { off_ink });
            if resp.clicked() {
                hits.push(name);
            }
        }
        divider(ui, pill);
    }
    if hidden {
        let resp = more_button(ui, cell(bar.max.x - 10.0 - MORE_W, MORE_W), id.with("more"));
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            let items: [&[(&'static str, &str, bool)]; 8] = [
                &[("browser", "Pattern Browser", b_on), ("inspector", "Local Inspector", i_on)],
                &[("in", "MIDI In", in_on), ("out", "MIDI Out", out_on)],
                &[("mono", "Mono Mode", mono_on)],
                &[("live", "Live Pattern Recording", live_on)],
                &[("preview", "Preview", previewing)],
                &[("auto", "Vertical Auto Zoom", v_auto)],
                &[],
                &[("narrow", "Minimum Step Width", narrow), ("wide", "Maximum Step Width", !narrow)],
            ];
            for (i, list) in items.iter().enumerate() {
                if shown[i] {
                    continue;
                }
                if i == 6 {
                    let mut v = vz;
                    ui.horizontal(|ui| {
                        ui.label("Vertical Zoom");
                        if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                            new_vz = Some(v);
                        }
                    });
                }
                for &(name, label, on) in list.iter() {
                    if ui.add(egui::Button::selectable(on, label)).clicked() {
                        hits.push(name);
                        ui.close();
                    }
                }
            }
        });
    }
    if let Some(v) = new_vz {
        with_store(&ctx, |s| {
            s.v_zoom = Some(v);
            s.v_auto = false;
        });
    }
    for h in hits {
        match h {
            "browser" => with_store(&ctx, |s| s.browser = !s.browser),
            "inspector" => with_store(&ctx, |s| s.inspector = !s.inspector),
            "in" => with_store(&ctx, |s| {
                s.step_rec = !s.step_rec;
                s.step_pending = false;
            }),
            "out" => with_store(&ctx, |s| s.midi_out_off = !s.midi_out_off),
            "mono" => {
                command(app, &ctx, "Mono Mode");
            }
            "live" => with_store(&ctx, |s| s.live_record = !s.live_record),
            "auto" => with_store(&ctx, |s| s.v_auto = !s.v_auto),
            "narrow" | "wide" => with_store(&ctx, |s| s.narrow_steps = h == "narrow"),
            "preview" => {
                // plays the pattern while the project is stopped; again stops it
                let now = ctx.input(|i| i.time);
                let stop = with_store(&ctx, |s| match s.preview.take() {
                    Some(pv) => pv.sounding,
                    None => {
                        s.preview = Some(Preview { start: now, track: t, sent: 0, sounding: Vec::new(), pass: 0 });
                        Vec::new()
                    }
                });
                preview_off(app, t, &stop);
            }
            _ => {}
        }
    }
}

fn row_h(row: &Row, rh: f32) -> f32 {
    rh + if row.sub { SUB_H * SUB_MODES.len() as f32 } else { 0.0 }
}


/// The playback mode pop-up (the arrow box beside the rate).
fn play_box(ui: &mut egui::Ui, r: Rect, id: (&str, usize), cur: Play, fill: Color32, set: &mut dyn FnMut(Play)) {
    let big = r.height() > 22.0;
    let resp = pop_field(ui, r, id, fill).on_hover_text(format!("Playback Mode: {}", cur.name()));
    crate::glyphs::paint(ui.painter(), Rect::from_min_max(pos2(r.max.x - 16.0, r.min.y), r.max), crate::glyphs::ph::CARET_UP_DOWN, if big { 15.0 } else { 12.0 }, if big { look::text() } else { look::text_dim() });
    ui.painter().text(pos2(r.min.x + if big { 9.5 } else { 15.5 }, r.center().y), Align2::CENTER_CENTER, cur.arrow(), look::regular(if big { 16.0 } else { 14.0 }), look::text());
    egui::Popup::menu(&resp).show(|ui| {
        for m in Play::ALL {
            if ui.selectable_label(m == cur, m.name()).clicked() {
                set(m);
                ui.close();
            }
        }
    });
}

/// A pop-up field's body: its fill, and exactly its rect (same size and corners) lit on hover or
/// while its menu is open.
fn pop_field(ui: &mut egui::Ui, r: Rect, id: (&str, usize), fill: Color32) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with("lg-ss").with(id), Sense::click());
    ui.painter().rect_filled(r, 6.0, fill);
    if resp.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp)) {
        ui.painter().rect_filled(r, 6.0, Color32::from_white_alpha(18));
    }
    resp
}

/// Header and toolbar actions, applied once the frame's drawing is done.
enum Action {
    AddRow(String, u8),
    PatternRate(u32),
    RowRate(usize, u32),
    Length(Option<usize>, u8),
    Rotate(Option<usize>, i32),
    Randomize(Option<usize>),
    Clear(Option<usize>),
    /// Increment / Decrement Value: the row's (else every row's) current edit mode values.
    Nudge(Option<usize>, i32),
    /// The page overview's shortcut menu on page k.
    Page(usize, PageOp),
    /// The disclosure arrow: show / hide the row's (None: every row's) subrows.
    Subrows(Option<usize>, bool),
    /// A row header dragged from one place to another.
    MoveRow(usize, usize),
    Mute(usize),
    Solo(usize),
    /// The Row Assignment pop-up: the row's new note (and whether it's a drum kit's row).
    Assign(usize, u8, bool),
    Mode(usize, Mode),
    Play(Option<usize>, Play),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageOp {
    Cut,
    Copy,
    Paste,
    Clear,
}

/// Cut / Copy / Paste / Clear on a 16-step page of every row (`clip`: rows of steps).
fn page_op(pat: &mut StepPattern, page: usize, op: PageOp, clip: &mut Vec<Vec<Step>>) {
    let range = page * 16..((page + 1) * 16).min(MAX_STEPS);
    if matches!(op, PageOp::Cut | PageOp::Copy) {
        *clip = pat.rows.iter().map(|r| r.steps[range.clone()].to_vec()).collect();
    }
    match op {
        PageOp::Cut | PageOp::Clear => pat.rows.iter_mut().for_each(|r| r.steps[range.clone()].fill(Step::default())),
        PageOp::Paste => {
            for (row, src) in pat.rows.iter_mut().zip(clip.iter()) {
                for (k, st) in src.iter().enumerate() {
                    if let Some(d) = row.steps.get_mut(range.start + k) {
                        *d = *st;
                    }
                }
            }
        }
        PageOp::Copy => {}
    }
}

fn apply(pat: &mut StepPattern, a: Action, ctx: &egui::Context, t: usize, r: usize) {
    // a row header control acts on every selected row when its row is one of them
    let selected = ctx.data(|d| d.get_temp::<Store>(store_id())).map(|s| sel_rows(&s, pat.rows.len())).unwrap_or_default();
    let n_rows = pat.rows.len();
    let rows = |sel: Option<usize>| header_rows(sel, &selected, n_rows);
    match a {
        Action::AddRow(name, pitch) => {
            let order = with_store(ctx, |s| (!s.manual_order).then_some(s.sort));
            place_row(pat, Row::new(name, pitch), order);
        }
        Action::PatternRate(v) => pat.rate = v,
        Action::RowRate(k, v) => {
            for k in rows(Some(k)) {
                let row = &mut pat.rows[k];
                row.rate = if v == pat.rate { None } else { Some(v) };
            }
        }
        Action::Length(None, n) => set_length(pat, n),
        Action::MoveRow(from, to) => {
            if from < pat.rows.len() && to < pat.rows.len() {
                let row = pat.rows.remove(from);
                pat.rows.insert(to, row);
                // the rows' edit modes move with them
                with_store(ctx, |s| {
                    let n = pat.rows.len();
                    let mut order: Vec<usize> = (0..n).collect();
                    let x = order.remove(from);
                    order.insert(to, x);
                    let old: Vec<Option<Mode>> = (0..n).map(|k| s.modes.remove(&(t, r, k))).collect();
                    for (new_k, &old_k) in order.iter().enumerate() {
                        if let Some(m) = old[old_k] {
                            s.modes.insert((t, r, new_k), m);
                        }
                    }
                    s.manual_order = true;
                    s.selected_row = to;
                });
            }
        }
        Action::Page(k, op) => {
            let mut clip = with_store(ctx, |s| s.clip_page.clone());
            page_op(pat, k, op, &mut clip);
            with_store(ctx, |s| s.clip_page = clip);
        }
        Action::Nudge(sel, d) => {
            for k in rows(sel) {
                let mode = ctx.data(|d| d.get_temp::<Store>(store_id())).and_then(|s| s.modes.get(&(t, r, k)).copied()).unwrap_or_default();
                let n = pat.row_len(&pat.rows[k]);
                nudge_steps(&mut pat.rows[k].steps[..n], mode, d);
            }
        }
        Action::Length(Some(k), n) => {
            if let Some(row) = pat.rows.get_mut(k) {
                row.len = Some(n);
            }
        }
        Action::Rotate(sel, d) => {
            for k in rows(sel) {
                let n = pat.row_len(&pat.rows[k]);
                let s = &mut pat.rows[k].steps[..n];
                if d > 0 { s.rotate_right(1) } else { s.rotate_left(1) }
            }
        }
        Action::Randomize(sel) => {
            let seed = ctx.input(|i| i.time.to_bits());
            let (mono, o) = ctx.data(|d| d.get_temp::<Store>(store_id())).map_or((false, RandOpts::default()), |s| (s.mono, s.rand));
            let rows_hit = rows(sel);
            for k in rows_hit.clone() {
                let mode = ctx.data(|d| d.get_temp::<Store>(store_id())).and_then(|s| s.modes.get(&(t, r, k)).copied()).unwrap_or_default();
                let n = pat.row_len(&pat.rows[k]);
                for (j, s) in pat.rows[k].steps[..n].iter_mut().enumerate() {
                    randomize_with(mode, s, chance(seed, k, j as u32) as f32 / 99.0, o);
                }
            }
            if mono {
                mono_columns(pat, &rows_hit);
            }
        }
        Action::Clear(sel) => {
            for k in rows(sel) {
                pat.rows[k].steps = vec![Step::default(); MAX_STEPS];
            }
        }
        Action::Subrows(k, on) => match k {
            Some(k) => pat.rows[k].sub = on,
            None => pat.rows.iter_mut().for_each(|r| r.sub = on),
        },
        Action::Mute(k) => {
            let v = !pat.rows[k].mute;
            rows(Some(k)).into_iter().for_each(|k| pat.rows[k].mute = v);
        }
        Action::Solo(k) => {
            let v = !pat.rows[k].solo;
            rows(Some(k)).into_iter().for_each(|k| pat.rows[k].solo = v);
        }
        Action::Assign(k, p, drums) => assign_row(&mut pat.rows[k], p, drums),
        Action::Play(None, m) => pat.play = m,
        Action::Play(Some(k), m) => {
            let base = pat.play;
            for k in rows(Some(k)) {
                pat.rows[k].play = if m == base { None } else { Some(m) };
            }
        }
        Action::Mode(k, m) => with_store(ctx, |s| {
            s.modes.insert((t, r, k), m);
        }),
    }
}

/// A new row: where View ▸ Sort Rows puts it, or (after a row was moved by hand: `order` None)
/// at the end.
pub fn place_row(p: &mut StepPattern, row: Row, order: Option<(u8, bool)>) {
    p.rows.push(row);
    if let Some((by, asc)) = order {
        crate::stepinsp::sort_rows(p, by, asc);
    }
}

/// An Increment / Decrement Value button: a click is one notch (`d`); dragging up or down on
/// it changes the values continuously, a notch per 6 points.
fn nudge_clicks(ctx: &egui::Context, resp: &egui::Response, id: (&str, usize), d: i32) -> Option<i32> {
    let key = egui::Id::new("lg-ss-nudge").with(id);
    if resp.drag_started() {
        ctx.data_mut(|m| m.insert_temp(key, 0.0f32));
    }
    if resp.dragged() {
        let acc = ctx.data(|m| m.get_temp::<f32>(key)).unwrap_or(0.0) - resp.drag_delta().y;
        let notches = (acc / 6.0).trunc();
        ctx.data_mut(|m| m.insert_temp(key, acc - notches * 6.0));
        return (notches != 0.0).then_some(notches as i32);
    }
    resp.clicked().then_some(d)
}

/// The rows a header control acts on: its own row, or every selected row when it is one of
/// several selected; `None` (the pattern's controls) is every row.
fn header_rows(sel: Option<usize>, selected: &[usize], n: usize) -> Vec<usize> {
    match sel {
        Some(k) if selected.len() > 1 && selected.contains(&k) => selected.iter().copied().filter(|&x| x < n).collect(),
        Some(k) if k < n => vec![k],
        Some(_) => Vec::new(),
        None => (0..n).collect(),
    }
}

/// Pattern Length: a longer pattern repeats what it had into the added steps (Logic's), on the
/// rows that follow the pattern's length.
pub fn set_length(pat: &mut StepPattern, n: u8) {
    let old = pat.steps.clamp(1, MAX_STEPS as u8) as usize;
    let n = n.clamp(1, MAX_STEPS as u8);
    if n as usize > old {
        for row in pat.rows.iter_mut().filter(|r| r.len.is_none()) {
            for k in old..n as usize {
                row.steps[k] = row.steps[k % old];
            }
        }
    }
    pat.steps = n;
}

/// The Increment / Decrement Value buttons on a row's steps: in Step On/Off they turn every
/// step on or off, in Tie they tie every step to the next (but the last) or untie them all;
/// other modes move each on step's value one notch.
pub fn nudge_steps(steps: &mut [Step], mode: Mode, d: i32) {
    match mode {
        Mode::OnOff => steps.iter_mut().for_each(|s| s.on = d > 0),
        Mode::Tie => {
            for (k, s) in steps.iter_mut().enumerate() {
                if d > 0 {
                    s.on = true;
                    s.tie = k > 0;
                } else {
                    s.tie = false;
                }
            }
        }
        Mode::Skip => steps.iter_mut().for_each(|s| s.skip = d > 0),
        Mode::Rate => {
            let o = rate_order();
            for s in steps.iter_mut().filter(|s| s.on) {
                let k = o.iter().position(|&x| Some(x) == s.rate).unwrap_or(sixteenth_index(&o)) as i32;
                s.rate = Some(o[(k + d).clamp(0, o.len() as i32 - 1) as usize]);
            }
        }
        m => steps.iter_mut().filter(|s| s.on).for_each(|s| nudge_value(m, s, d)),
    }
}

/// Mono Mode: one step per column. Where `keep`'s rows have a step on, the other rows' steps
/// in that column go off (the first of `keep` wins).
pub fn mono_columns(pat: &mut StepPattern, keep: &[usize]) {
    for k in 0..MAX_STEPS {
        let winner = keep.iter().copied().find(|&r| pat.rows.get(r).is_some_and(|row| row.steps[k].on))
            .or_else(|| pat.rows.iter().position(|row| row.steps[k].on));
        if let Some(w) = winner {
            for (ri, row) in pat.rows.iter_mut().enumerate() {
                if ri != w {
                    row.steps[k].on = false;
                }
            }
        }
    }
}

fn plain(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    // (hovered / pressed / open: a light tint that leaves the drawn symbol visible, not an
    // opaque fill over it)
    for w in [&mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.weak_bg_fill = Color32::from_white_alpha(18);
        w.bg_fill = Color32::from_white_alpha(18);
        w.bg_stroke = Stroke::NONE;
    }
}

/// The header's small symbols, drawn (the UI font has no such glyphs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Up,
    Down,
    UpDown,
    RotLeft,
    RotRight,
    Dice,
    Clear,
}

fn glyph(p: &egui::Painter, r: Rect, g: Glyph, col: Color32) {
    use crate::stepicons as si;
    let c = r.center();
    match g {
        Glyph::Up => si::caret(p, c, true, col),
        Glyph::Down => si::caret(p, c, false, col),
        Glyph::UpDown => crate::glyphs::paint(p, r, crate::glyphs::ph::CARET_UP_DOWN, 12.0, col),
        Glyph::RotLeft => si::rotate(p, c, false, col),
        Glyph::RotRight => si::rotate(p, c, true, col),
        Glyph::Dice => si::dice(p, c, col),
        Glyph::Clear => si::clear(p, c, col),
    }
}

fn small_button_resp(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, g: Glyph, id: (&str, usize), tip: &str, fill: Color32) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with("lg-ss").with(id), Sense::click()).on_hover_text(tip);
    p.rect_filled(r, 5.0, if resp.hovered() { look::button_on() } else { fill });
    glyph(p, r, g, look::text());
    resp
}



/// A Reset (clear) button: dimmed, and not clickable, while there is nothing to clear (Logic's).
fn clear_button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: (&str, usize), tip: &str, enabled: bool) -> bool {
    let resp = ui.interact(r, ui.scope_id().with("lg-ss").with(id), if enabled { Sense::click() } else { Sense::hover() }).on_hover_text(tip);
    let fill = if !enabled { look::mix(look::header_bg(), look::group_bg(), 0.5) } else if resp.hovered() { look::button_on() } else { look::group_bg() };
    p.rect_filled(r, 5.0, fill);
    glyph(p, r, Glyph::Clear, if enabled { look::text() } else { look::text_dim() });
    enabled && resp.clicked()
}

/// A pill of two buttons (rotate, increment / decrement): one capsule, Logic's thin divider.
fn pill_pair(p: &egui::Painter, r: Rect, fill: Color32) {
    p.rect_filled(r, 5.0, fill);
    p.vline(r.center().x, r.y_range().shrink(3.0), Stroke::new(1.0, look::header_bg()));
}

/// "/16 ⌃⌄": a step rate pop-up.
fn rate_box(ui: &mut egui::Ui, r: Rect, id: (&str, usize), rate: u32, fill: Color32, set: &mut dyn FnMut(u32)) {
    let big = r.height() > 22.0;
    let resp = pop_field(ui, r, id, fill);
    crate::glyphs::paint(ui.painter(), Rect::from_min_max(pos2(r.max.x - 18.0, r.min.y), r.max), crate::glyphs::ph::CARET_UP_DOWN, if big { 15.0 } else { 12.0 }, if big { look::text() } else { look::text_dim() });
    // (the text centred on the field's middle, as Logic's)
    ui.painter().text(pos2(r.min.x + if big { 9.0 } else { 7.5 }, r.center().y), Align2::LEFT_CENTER, rate_name(rate), if big { look::semibold(13.5) } else { look::regular(12.0) }, look::text());
    egui::Popup::menu(&resp).show(|ui| {
        for (k, (name, v)) in RATES.into_iter().enumerate() {
            if RATE_GROUPS.contains(&k) {
                ui.separator();
            }
            if ui.selectable_label(v == rate, name).clicked() {
                set(v);
                ui.close();
            }
        }
    });
}

/// "C - Major (Ionian)" from "C major".
fn scale_label(key: &str) -> String {
    let mut it = key.split_whitespace();
    let root = it.next().unwrap_or("C");
    let minor = it.next().is_some_and(|m| m.starts_with("min"));
    format!("{root} - {}", if minor { "Minor (Aeolian)" } else { "Major (Ionian)" })
}

// ---- key commands, the Pattern Browser and pattern files ------------------------------------

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &[
    "Show/Hide Pattern Browser", "Pattern Browser", "Repeat Step", "Copy Row", "Paste Row", "Paste Steps", "Paste Row Settings", "Paste to New Row", "Duplicate Row with Next Assignment", "Duplicate Row", "Delete Row", "Delete Unused Rows",
    "Clear Step", "Clear Row", "Clear Pattern", "Learn (Add)", "Learn (Assign)", "Store as Default Pattern Template", "Revert to Default Pattern Template",
    "Transpose +1 Semitone", "Transpose -1 Semitone", "Transpose +1 Octave", "Transpose -1 Octave", "Transpose to Project Key",
    "Clear Current Edit Mode Values for Row", "Clear Current Edit Mode Values for All Rows", "Clear Current Edit Mode Values for All Note Rows",
    "Randomize Current Edit Mode Values for Selected Rows", "Randomize Current Edit Mode Values for All Rows", "Randomize Current Edit Mode Values for All Note Rows", "Randomize Current Edit Mode Values for Selected Steps",
    "Randomize Current Edit Mode for Selected Rows…", "Randomize Current Edit Mode for Pattern…",
    "Set Step Color by Region Color", "Set Step Color by Row Color", "By Region", "By Row",
    "Select Step Above", "Select Step Below", "Select Previous Step", "Select Next Step", "Extend Selection Up", "Extend Selection Down", "Extend Selection Left", "Extend Selection Right",
    "Live Record", "MIDI In", "MIDI Out", "Mono Mode", "Rename Row", "Select Next Pattern Page", "Save Pattern", "Save Template",
    "Set Pattern Playback Mode to Forward", "Set Pattern Playback Mode to Reverse", "Set Pattern Playback Mode to Ping Pong", "Set Pattern Playback Mode to Shuffle", "Set Pattern Playback Mode to Pendulum", "Set Pattern Playback Mode to Wander", "Set Pattern Playback Mode to Random",
    "Set Row Playback Mode to Forward", "Set Row Playback Mode to Reverse", "Set Row Playback Mode to Ping Pong", "Set Row Playback Mode to Shuffle", "Set Row Playback Mode to Pendulum", "Set Row Playback Mode to Wander", "Set Row Playback Mode to Random",
    "Rotate Pattern Right", "Rotate Pattern Left", "Rotate Row Right", "Rotate Row Left", "Rotate Values for Current Edit Mode Right", "Rotate Values for Current Edit Mode Left",
    "Solo Row", "Mute Row", "Increment Loop Start for Row", "Decrement Loop Start for Row", "Increment Loop End for Row", "Decrement Loop End for Row",
    "Show Subrows for Row", "Hide Subrows for Row", "Show Subrows for All Rows", "Hide Subrows for All Rows", "Toggle Selected Step",
    "Set Global Edit Mode to Steps On/Off", "Set Global Edit Mode to Velocity/Value", "Set Global Edit Mode to Gate", "Set Global Edit Mode to Tie", "Set Global Edit Mode to Note", "Set Global Edit Mode to Octave", "Set Global Edit Mode to Loop Start/End", "Set Global Edit Mode to Note Repeat", "Set Global Edit Mode to Chance", "Set Global Edit Mode to Start Offset", "Set Global Edit Mode to Skip",
    "Increment Current Edit Mode Values for Row", "Decrement Current Edit Mode Values for Row", "Increment Pattern Step Rate", "Decrement Pattern Step Rate", "Increment Row Step Rate", "Decrement Row Step Rate",
    "Open Step Sequencer", "Open Step Sequencer…", "Show/Hide Step Sequencer",
];

/// Step rates from slow to fast (Increment = the next faster).
fn rate_order() -> Vec<u32> {
    let mut v: Vec<u32> = RATES.iter().map(|r| r.1).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v
}

/// The index of 1/16 in `rate_order()` (a step with no own rate).
fn sixteenth_index(order: &[u32]) -> usize {
    order.iter().position(|&r| r == SIXTEENTH).unwrap_or(0)
}

fn current(app: &App, ctx: &egui::Context) -> Option<(usize, usize)> {
    let (t, r) = app.selected_region?;
    (app.bottom == crate::app::Bottom::PianoRoll && is_pattern(ctx, t, r)).then_some((t, r))
}

/// The rows the selection covers (else the selected row).
fn sel_rows(s: &Store, n: usize) -> Vec<usize> {
    match s.sel {
        Some((a, b)) => (a.0.min(b.0)..=a.0.max(b.0)).filter(|&k| k < n).collect(),
        None if n > 0 => vec![s.selected_row.min(n - 1)],
        None => Vec::new(),
    }
}

/// The steps selected: (row, step).
fn sel_cells(s: &Store, n: usize) -> Vec<(usize, usize)> {
    let Some((a, b)) = s.sel else { return Vec::new() };
    sel_cells_of(a, b, n)
}

fn sel_cells_of(a: (usize, usize), b: (usize, usize), n: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for r in a.0.min(b.0)..=a.0.max(b.0) {
        for k in a.1.min(b.1)..=a.1.max(b.1) {
            if r < n {
                out.push((r, k));
            }
        }
    }
    out
}

fn mode_of(s: &Store, t: usize, r: usize, row: usize) -> Mode {
    s.modes.get(&(t, r, row)).copied().unwrap_or_default()
}

/// A mode's value back to its default on a step.
fn clear_value(mode: Mode, st: &mut Step) {
    let d = Step::default();
    match mode {
        Mode::OnOff | Mode::Loop => st.on = false,
        Mode::Velocity => st.velocity = d.velocity,
        Mode::Gate => st.gate = d.gate,
        Mode::Repeat => st.repeat = d.repeat,
        Mode::Chance => st.chance = d.chance,
        Mode::Offset => st.offset = d.offset,
        Mode::Tie => st.tie = false,
        Mode::Octave => st.octave = 0,
        Mode::Skip => st.skip = false,
        Mode::Note => st.note = 0,
        Mode::Rate => st.rate = None,
    }
}

fn randomize(mode: Mode, st: &mut Step, v: f32, o: RandOpts) {
    randomize_with(mode, st, v, o);
}

/// One step of a mode's value up or down.
fn nudge_value(mode: Mode, st: &mut Step, d: i32) {
    match mode {
        Mode::Velocity => st.velocity = (st.velocity as i32 + d * 8).clamp(1, 127) as u8,
        Mode::Gate => st.gate = (st.gate as i32 + d * 10).clamp(1, 100) as u8,
        Mode::Repeat => st.repeat = (st.repeat as i32 + d).clamp(1, 16) as u8,
        Mode::Chance => st.chance = (st.chance as i32 + d * 10).clamp(0, 100) as u8,
        Mode::Offset => st.offset = (st.offset as i32 + d * 5).clamp(-50, 50) as i8,
        Mode::Octave => st.octave = (st.octave as i32 + d).clamp(-3, 3) as i8,
        Mode::Note => st.note = (st.note as i32 + d).clamp(-12, 12) as i8,
        _ => {}
    }
}

/// Changes the shown pattern (one undo step) and plays it from its notes.
fn change(app: &mut App, ctx: &egui::Context, what: &str, f: impl FnOnce(&mut StepPattern, &mut Store, usize, usize)) -> bool {
    let Some((t, r)) = current(app, ctx) else { return false };
    app.checkpoint(what);
    with_store(ctx, |s| {
        let mut e = s.patterns.remove(&(t, r)).unwrap_or_default();
        f(&mut e.pattern, s, t, r);
        s.patterns.insert((t, r), e);
    });
    write_notes(app, ctx, t, r);
    app.after_edit();
    true
}

/// View state only (no undo step).
fn view(app: &App, ctx: &egui::Context, f: impl FnOnce(&mut Store, usize, usize, usize)) -> bool {
    let Some((t, r)) = current(app, ctx) else { return false };
    with_store(ctx, |s| {
        let n = s.patterns.get(&(t, r)).map_or(0, |e| e.pattern.rows.len());
        f(s, t, r, n)
    });
    true
}

/// Runs a Step Sequencer command; false when it isn't one or no pattern is shown.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    // the Edit menu's Copy / Cut / Paste act on the selected steps
    if matches!(name, "Copy" | "Cut" | "Paste" | "Delete") {
        let Some((t, r)) = current(app, ctx) else { return false };
        let has = with_store(ctx, |s| s.sel.is_some() || (name == "Paste" && !s.clip_steps.is_empty()));
        if !has || !crate::focus::editor(app) {
            return false;
        }
        if name == "Delete" {
            return command(app, ctx, "Clear Step");
        }
        if name == "Paste" {
            return command(app, ctx, "Paste Steps");
        }
        with_store(ctx, |s| {
            let pat = s.patterns.get(&(t, r)).map(|e| e.pattern.clone()).unwrap_or_default();
            let cells = sel_cells(s, pat.rows.len());
            let rows: Vec<usize> = { let mut v: Vec<usize> = cells.iter().map(|c| c.0).collect(); v.dedup(); v };
            s.clip_steps = rows.iter().map(|&rr| cells.iter().filter(|c| c.0 == rr).map(|c| pat.rows[rr].steps[c.1]).collect()).collect();
        });
        if name == "Cut" {
            return command(app, ctx, "Clear Step");
        }
        return true;
    }
    if !COMMANDS.contains(&name) {
        return false;
    }
    match name {
        "Open Step Sequencer" | "Open Step Sequencer…" | "Show/Hide Step Sequencer" => {
            // a pattern region opens it; otherwise the selected MIDI region becomes one
            if name == "Show/Hide Step Sequencer" && current(app, ctx).is_some() {
                app.bottom = crate::app::Bottom::None;
                return true;
            }
            app.bottom = crate::app::Bottom::PianoRoll;
            if current(app, ctx).is_none() {
                if let Some((t, r)) = app.selected_region {
                    convert_to_pattern(app, ctx, t, r);
                } else if let Some(t) = app.selected_track {
                    let at = app.position() as u32;
                    new_pattern_region(app, ctx, t, at);
                }
            }
            return true;
        }
        _ => {}
    }
    let Some((t, r)) = current(app, ctx) else { return false };
    let key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_else(|| "C major".into());
    let seed = ctx.input(|i| i.time.to_bits());
    let rate_step = |cur: u32, d: i32| -> u32 {
        let o = rate_order();
        let k = o.iter().position(|&x| x == cur).unwrap_or(sixteenth_index(&o)) as i32;
        o[(k + d).clamp(0, o.len() as i32 - 1) as usize]
    };
    let play_of = |n: &str| Play::ALL.iter().copied().find(|p| n.ends_with(p.name())).unwrap_or_default();
    match name {
        "Show Local Inspector" | "Hide Local Inspector" | "Show/Hide Local Inspector" => view(app, ctx, |s, _, _, _| s.inspector = !s.inspector),
        "By Type" | "By Type and Note" | "By Type and Name" | "Ascending" | "Descending" => {
            let (mut by, mut asc) = with_store(ctx, |s| s.sort);
            match name {
                "By Type" => by = 1,
                "By Type and Note" => by = 0,
                "By Type and Name" => by = 2,
                "Ascending" => asc = true,
                _ => asc = false,
            }
            with_store(ctx, |s| {
                s.sort = (by, asc);
                s.manual_order = false;
            });
            change(app, ctx, "Sort Rows", |p, _, _, _| crate::stepinsp::sort_rows(p, by, asc))
        }
        "Show/Hide Pattern Browser" | "Pattern Browser" => view(app, ctx, |s, _, _, _| s.browser = !s.browser),
        "Set Step Color by Region Color" | "By Region" => view(app, ctx, |s, _, _, _| s.by_region = true),
        "Set Step Color by Row Color" | "By Row" => view(app, ctx, |s, _, _, _| s.by_region = false),
        "Live Record" => view(app, ctx, |s, _, _, _| s.live_record = !s.live_record),
        "Step Input" | "MIDI In" => view(app, ctx, |s, _, _, _| {
            s.step_rec = !s.step_rec;
            s.step_pending = false;
        }),
        "MIDI Out" => view(app, ctx, |s, _, _, _| s.midi_out_off = !s.midi_out_off),
        "Learn (Add)" => view(app, ctx, |s, _, _, _| s.learn = Some(true)),
        "Learn (Assign)" => view(app, ctx, |s, _, _, _| s.learn = Some(false)),
        "Rename Row" => view(app, ctx, |s, t, r, n| {
            if n > 0 {
                let k = s.selected_row.min(n - 1);
                let name = s.patterns.get(&(t, r)).map(|e| e.pattern.rows[k].name.clone()).unwrap_or_default();
                s.typing = Some((Typing::Rename(k), name));
            }
        }),
        "Save Pattern" | "Save Template" => view(app, ctx, |s, _, _, _| s.typing = Some((Typing::Save(name == "Save Template"), String::new()))),
        "Select Next Pattern Page" => view(app, ctx, |s, t, r, _| {
            let steps = s.patterns.get(&(t, r)).map_or(16, |e| e.pattern.steps) as usize;
            s.page = (s.page + 1) % steps.div_ceil(16).max(1);
        }),
        "Select Step Above" | "Select Step Below" | "Select Previous Step" | "Select Next Step" | "Extend Selection Up" | "Extend Selection Down" | "Extend Selection Left" | "Extend Selection Right" => view(app, ctx, |s, t, r, n| {
            if n == 0 {
                return;
            }
            let steps = s.patterns.get(&(t, r)).map_or(16, |e| e.pattern.steps) as usize;
            let (anchor, cur) = s.sel.unwrap_or(((s.selected_row.min(n - 1), s.page * 16), (s.selected_row.min(n - 1), s.page * 16)));
            let (dr, dk): (i32, i32) = match name.rsplit(' ').next().unwrap_or("") {
                "Above" | "Up" => (-1, 0),
                "Below" | "Down" => (1, 0),
                "Left" => (0, -1),
                "Right" => (0, 1),
                _ => if name.contains("Previous") { (0, -1) } else { (0, 1) },
            };
            let nc = ((cur.0 as i32 + dr).clamp(0, n as i32 - 1) as usize, (cur.1 as i32 + dk).clamp(0, steps as i32 - 1) as usize);
            let extend = name.starts_with("Extend");
            s.sel = Some((if extend { anchor } else { nc }, nc));
            s.selected_row = nc.0;
            s.page = nc.1 / 16;
        }),
        "Toggle Selected Step" => change(app, ctx, "Toggle Step", |p, s, _, _| {
            let cells = sel_cells(s, p.rows.len());
            let on = !cells.iter().all(|&(r, k)| p.rows[r].steps[k].on);
            for (r, k) in cells {
                p.rows[r].steps[k].on = on;
            }
        }),
        "Repeat Step" => change(app, ctx, "Repeat Step", |p, s, _, _| {
            let Some((a, b)) = s.sel else { return };
            let (k0, k1) = (a.1.min(b.1), a.1.max(b.1));
            let w = k1 - k0 + 1;
            for r in a.0.min(b.0)..=a.0.max(b.0) {
                for k in k0..=k1 {
                    if k + w < MAX_STEPS {
                        p.rows[r].steps[k + w] = p.rows[r].steps[k];
                    }
                }
            }
            if k1 + w < p.steps as usize {
                s.sel = Some(((a.0, k0 + w), (b.0, k1 + w)));
            }
        }),
        "Copy Row" => view(app, ctx, |s, t, r, n| {
            if n > 0 {
                s.clip_row = s.patterns.get(&(t, r)).map(|e| e.pattern.rows[s.selected_row.min(n - 1)].clone());
            }
        }),
        "Paste Row" | "Paste Row Settings" | "Paste to New Row" => {
            let Some(clip) = with_store(ctx, |s| s.clip_row.clone()) else { return true };
            change(app, ctx, name, |p, s, _, _| {
                if name == "Paste to New Row" {
                    p.rows.insert((s.selected_row + 1).min(p.rows.len()), clip);
                    return;
                }
                let Some(row) = p.rows.get_mut(s.selected_row) else { return };
                if name == "Paste Row" {
                    row.steps = clip.steps.clone();
                } else {
                    (row.mute, row.solo, row.len, row.rate, row.start, row.play) = (clip.mute, clip.solo, clip.len, clip.rate, clip.start, clip.play);
                }
            })
        }
        "Paste Steps" => {
            let clip = with_store(ctx, |s| s.clip_steps.clone());
            if clip.is_empty() {
                return true;
            }
            change(app, ctx, "Paste Steps", |p, s, _, _| {
                let at = s.sel.map_or((s.selected_row, s.page * 16), |(a, b)| (a.0.min(b.0), a.1.min(b.1)));
                for (dr, steps) in clip.iter().enumerate() {
                    for (dk, st) in steps.iter().enumerate() {
                        if let Some(slot) = p.rows.get_mut(at.0 + dr).and_then(|row| row.steps.get_mut(at.1 + dk)) {
                            *slot = *st;
                        }
                    }
                }
            })
        }
        "Duplicate Row with Next Assignment" | "Duplicate Row" => change(app, ctx, "Duplicate Row", |p, s, _, _| {
            let Some(row) = p.rows.get(s.selected_row).cloned() else { return };
            let used: Vec<u8> = p.rows.iter().map(|r| r.pitch).collect();
            // the next note (or drum) not already a row
            let pitch = (1..=127).map(|d| (row.pitch as i32 - d).clamp(0, 127) as u8).find(|q| !used.contains(q)).unwrap_or(row.pitch);
            let mut dup = row.clone();
            dup.pitch = pitch;
            dup.name = drum_name(pitch).filter(|_| drum_name(row.pitch).is_some_and(|n| n == row.name)).map(str::to_string).unwrap_or_else(|| crate::app::note_name(pitch));
            p.rows.insert(s.selected_row + 1, dup);
            s.selected_row += 1;
        }),
        "Delete Row" => change(app, ctx, "Delete Row", |p, s, _, _| {
            if s.selected_row < p.rows.len() {
                p.rows.remove(s.selected_row);
                s.selected_row = s.selected_row.min(p.rows.len().saturating_sub(1));
                s.sel = None;
            }
        }),
        "Delete Unused Rows" => change(app, ctx, "Delete Unused Rows", |p, s, _, _| {
            p.rows.retain(|r| r.steps.iter().any(|x| x.on));
            s.selected_row = 0;
            s.sel = None;
        }),
        "Clear Step" => change(app, ctx, "Clear Step", |p, s, _, _| {
            for (r, k) in sel_cells(s, p.rows.len()) {
                p.rows[r].steps[k] = Step::default();
            }
        }),
        "Clear Row" => change(app, ctx, "Clear Row", |p, s, _, _| {
            for r in sel_rows(s, p.rows.len()) {
                p.rows[r].steps = vec![Step::default(); MAX_STEPS];
            }
        }),
        "Clear Pattern" => change(app, ctx, "Clear Pattern", |p, _, _, _| p.rows.iter_mut().for_each(|r| r.steps = vec![Step::default(); MAX_STEPS])),
        "Store as Default Pattern Template" => {
            let pat = with_store(ctx, |s| s.patterns.get(&(t, r)).map(|e| e.pattern.clone())).unwrap_or_default();
            match save_file(&default_path(), &pat) {
                Ok(()) => app.tell("Stored as the default pattern template"),
                Err(e) => app.tell(format!("Couldn't store the template: {e}")),
            }
            true
        }
        "Revert to Default Pattern Template" => {
            let Some(def) = load_file(&default_path()) else {
                app.tell("No default pattern template stored yet");
                return true;
            };
            change(app, ctx, "Revert to Default Pattern Template", |p, _, _, _| *p = def)
        }
        "Transpose +1 Semitone" | "Transpose -1 Semitone" | "Transpose +1 Octave" | "Transpose -1 Octave" => {
            let d = match name {
                "Transpose +1 Semitone" => 1,
                "Transpose -1 Semitone" => -1,
                "Transpose +1 Octave" => 12,
                _ => -12,
            };
            change(app, ctx, name, |p, _, _, _| {
                for row in p.rows.iter_mut() {
                    transpose_row(row, d);
                }
            })
        }
        "Transpose to Project Key" => change(app, ctx, name, |p, _, _, _| {
            let k = lg_notation::Key::parse(&key);
            for row in p.rows.iter_mut() {
                let spelled = k.spell(row.pitch);
                let in_key = lg_notation::step_pitch(spelled.step, k);
                if in_key != row.pitch {
                    transpose_row(row, in_key as i32 - row.pitch as i32);
                }
            }
        }),
        _ if name.starts_with("Clear Current Edit Mode Values for ") => change(app, ctx, "Clear Values", |p, s, t, r| {
            let rows = if name.ends_with("for Row") { sel_rows(s, p.rows.len()) } else { (0..p.rows.len()).collect() };
            for k in rows {
                let m = mode_of(s, t, r, k);
                p.rows[k].steps.iter_mut().for_each(|st| clear_value(m, st));
            }
        }),
        "Randomize Current Edit Mode for Selected Rows…" | "Randomize Current Edit Mode for Pattern…" => view(app, ctx, |s, _, _, _| {
            let rows = if name.contains("Selected Rows") { Some(s.selected_row) } else { None };
            s.rand_dlg = Some((rows, ctx.content_rect().center() - vec2(110.0, 60.0)));
        }),
        _ if name.starts_with("Randomize Current Edit Mode") => change(app, ctx, "Randomize", |p, s, t, r| {
            if name.ends_with("Selected Steps") {
                for (k, (rr, st)) in sel_cells(s, p.rows.len()).into_iter().enumerate() {
                    let m = mode_of(s, t, r, rr);
                    randomize(m, &mut p.rows[rr].steps[st], chance(seed, rr, k as u32) as f32 / 99.0, s.rand);
                }
                return;
            }
            let rows = if name.contains("Selected Rows") { sel_rows(s, p.rows.len()) } else { (0..p.rows.len()).collect() };
            for &k in &rows {
                let m = mode_of(s, t, r, k);
                let n = p.row_len(&p.rows[k]);
                for (j, st) in p.rows[k].steps[..n].iter_mut().enumerate() {
                    randomize(m, st, chance(seed, k, j as u32) as f32 / 99.0, s.rand);
                }
            }
            // Mono Mode: never two notes in a column
            if s.mono {
                mono_columns(p, &rows);
            }
        }),
        "Mono Mode" => change(app, ctx, "Mono Mode", |p, s, _, _| {
            s.mono = !s.mono;
            if s.mono {
                // one step per column: the top row's
                for k in 0..MAX_STEPS {
                    let mut seen = false;
                    for row in p.rows.iter_mut() {
                        if row.steps[k].on {
                            row.steps[k].on = !seen;
                            seen = true;
                        }
                    }
                }
            }
        }),
        _ if name.starts_with("Set Pattern Playback Mode to ") => change(app, ctx, "Playback Mode", |p, _, _, _| p.play = play_of(name)),
        _ if name.starts_with("Set Row Playback Mode to ") => change(app, ctx, "Playback Mode", |p, s, _, _| {
            let m = play_of(name);
            for k in sel_rows(s, p.rows.len()) {
                p.rows[k].play = if m == p.play { None } else { Some(m) };
            }
        }),
        "Rotate Pattern Right" | "Rotate Pattern Left" | "Rotate Row Right" | "Rotate Row Left" => change(app, ctx, name, |p, s, _, _| {
            let rows = if name.contains("Pattern") { (0..p.rows.len()).collect() } else { sel_rows(s, p.rows.len()) };
            for k in rows {
                let n = p.row_len(&p.rows[k]);
                let st = &mut p.rows[k].steps[..n];
                if name.ends_with("Right") { st.rotate_right(1) } else { st.rotate_left(1) }
            }
        }),
        "Rotate Values for Current Edit Mode Right" | "Rotate Values for Current Edit Mode Left" => change(app, ctx, "Rotate Values", |p, s, t, r| {
            for k in sel_rows(s, p.rows.len()) {
                let m = mode_of(s, t, r, k);
                let n = p.row_len(&p.rows[k]);
                let mut vals: Vec<Step> = p.rows[k].steps[..n].to_vec();
                if name.ends_with("Right") { vals.rotate_right(1) } else { vals.rotate_left(1) }
                for (st, v) in p.rows[k].steps[..n].iter_mut().zip(vals) {
                    set_mode_value(m, st, &v);
                }
            }
        }),
        "Solo Row" | "Mute Row" => change(app, ctx, name, |p, s, _, _| {
            for k in sel_rows(s, p.rows.len()) {
                if name == "Solo Row" { p.rows[k].solo = !p.rows[k].solo } else { p.rows[k].mute = !p.rows[k].mute }
            }
        }),
        "Increment Loop Start for Row" | "Decrement Loop Start for Row" | "Increment Loop End for Row" | "Decrement Loop End for Row" => change(app, ctx, "Loop Start/End", |p, s, _, _| {
            let d: i32 = if name.starts_with("Increment") { 1 } else { -1 };
            let steps = p.steps;
            for k in sel_rows(s, p.rows.len()) {
                let end = p.row_len(&p.rows[k]) as i32;
                let row = &mut p.rows[k];
                if name.contains("Start") {
                    row.start = (row.start as i32 + d).clamp(0, end - 1) as u8;
                } else {
                    let e = (end + d).clamp(row.start as i32 + 1, MAX_STEPS as i32) as u8;
                    row.len = if e == steps { None } else { Some(e) };
                }
            }
        }),
        "Show Subrows for Row" | "Hide Subrows for Row" | "Show Subrows for All Rows" | "Hide Subrows for All Rows" => change(app, ctx, "Subrows", |p, s, _, _| {
            let on = name.starts_with("Show");
            let rows = if name.ends_with("All Rows") { (0..p.rows.len()).collect() } else { sel_rows(s, p.rows.len()) };
            for k in rows {
                p.rows[k].sub = on;
            }
        }),
        _ if name.starts_with("Set Global Edit Mode to ") => {
            let m = match &name["Set Global Edit Mode to ".len()..] {
                "Steps On/Off" => Mode::OnOff,
                "Velocity/Value" => Mode::Velocity,
                "Gate" => Mode::Gate,
                "Tie" => Mode::Tie,
                "Note" => Mode::Note,
                "Octave" => Mode::Octave,
                "Loop Start/End" => Mode::Loop,
                "Note Repeat" => Mode::Repeat,
                "Chance" => Mode::Chance,
                "Start Offset" => Mode::Offset,
                _ => Mode::Skip,
            };
            set_global_mode(ctx, app, m);
            true
        }
        "Increment Current Edit Mode Values for Row" | "Decrement Current Edit Mode Values for Row" => change(app, ctx, "Change Values", |p, s, t, r| {
            let d = if name.starts_with("Increment") { 1 } else { -1 };
            for k in sel_rows(s, p.rows.len()) {
                let m = mode_of(s, t, r, k);
                let n = p.row_len(&p.rows[k]);
                nudge_steps(&mut p.rows[k].steps[..n], m, d);
            }
        }),
        "Increment Pattern Step Rate" | "Decrement Pattern Step Rate" => change(app, ctx, "Step Rate", |p, _, _, _| p.rate = rate_step(p.rate, if name.starts_with("Increment") { 1 } else { -1 })),
        "Increment Row Step Rate" | "Decrement Row Step Rate" => change(app, ctx, "Step Rate", |p, s, _, _| {
            let d = if name.starts_with("Increment") { 1 } else { -1 };
            let base = p.rate;
            for k in sel_rows(s, p.rows.len()) {
                let v = rate_step(p.rows[k].rate.unwrap_or(base), d);
                p.rows[k].rate = if v == base { None } else { Some(v) };
            }
        }),
        _ => false,
    }
}

/// A mode's value copied from one step to another.
fn set_mode_value(mode: Mode, st: &mut Step, from: &Step) {
    match mode {
        Mode::OnOff | Mode::Loop => st.on = from.on,
        Mode::Velocity => st.velocity = from.velocity,
        Mode::Gate => st.gate = from.gate,
        Mode::Repeat => st.repeat = from.repeat,
        Mode::Chance => st.chance = from.chance,
        Mode::Offset => st.offset = from.offset,
        Mode::Tie => st.tie = from.tie,
        Mode::Octave => st.octave = from.octave,
        Mode::Skip => st.skip = from.skip,
        Mode::Note => st.note = from.note,
        Mode::Rate => st.rate = from.rate,
    }
}

fn transpose_row(row: &mut Row, d: i32) {
    let named = row.name == crate::app::note_name(row.pitch);
    row.pitch = (row.pitch as i32 + d).clamp(0, 127) as u8;
    if named {
        row.name = crate::app::note_name(row.pitch);
    }
}

/// New notes held on a keyboard (Musical Typing or MIDI): Learn adds or assigns a row, Live
/// Record turns on the playing step of the note's row.
/// Step recording: `notes` (pitch, velocity) turned on at the selected step, rows added for
/// notes without one (in the sort order).
fn step_record(p: &mut StepPattern, s: &mut Store, notes: &[(u8, u8)], drums: bool) {
    let step = s.sel.map_or(0, |(_, b)| b.1).min(MAX_STEPS - 1);
    let mut last_row = None;
    for &(pitch, vel) in notes {
        if !p.rows.iter().any(|x| x.pitch == pitch) {
            let name = drums.then(|| drum_name(pitch)).flatten().map(str::to_string).unwrap_or_else(|| crate::app::note_name(pitch));
            place_row(p, Row::new(name, pitch), (!s.manual_order).then_some(s.sort));
        }
        if let Some(k) = p.rows.iter().position(|x| x.pitch == pitch) {
            p.rows[k].steps[step] = Step { on: true, velocity: vel.max(1), ..Step::default() };
            last_row = Some(k);
        }
    }
    if let Some(k) = last_row {
        s.sel = Some(((k, step), (k, step)));
        s.selected_row = k;
    }
}

/// Step recording: the next step selected (back to the first after the pattern's last).
fn step_advance(s: &mut Store, steps: usize) {
    let (row, step) = s.sel.map_or((s.selected_row, 0), |(_, b)| b);
    let next = (step + 1) % steps.max(1);
    s.sel = Some(((row, next), (row, next)));
    s.page = next / 16;
}

fn listen(app: &mut App, ctx: &egui::Context, t: usize, _r: usize, playing_tick: Option<u32>) {
    let held: Vec<(u8, u8)> = app.live.held.iter().map(|(&p, &(_, v))| (p, v)).collect();
    let (fresh, learn, live, step_rec) = with_store(ctx, |s| {
        let fresh: Vec<(u8, u8)> = held.iter().copied().filter(|(p, _)| !s.held.contains(p)).collect();
        s.held = held.iter().map(|x| x.0).collect();
        (fresh, s.learn, s.live_record, s.step_rec)
    });
    // MIDI In (step recording): notes go into the selected step (a row made for a note that has
    // none); the next step is selected once every held note is released, so chords go in one step
    if step_rec && learn.is_none() {
        let drums = is_drums(app, t);
        let release = held.is_empty() && with_store(ctx, |s| s.step_pending);
        if !fresh.is_empty() {
            change(app, ctx, "Step Record", |p, s, _, _| {
                step_record(p, s, &fresh, drums);
                s.step_pending = true;
            });
        } else if release {
            with_store(ctx, |s| {
                let n = s.patterns.get(&(t, _r)).map_or(16, |e| e.pattern.steps.max(1) as usize);
                step_advance(s, n);
                s.step_pending = false;
            });
        }
        return;
    }
    // Live Record with Note Length: notes released set their steps' Gate and Tie
    if live {
        let gone: Vec<(u8, usize, usize, u32)> = with_store(ctx, |s| {
            let (gone, keep): (Vec<_>, Vec<_>) = s.live_held.iter().copied().partition(|x| !held.iter().any(|h| h.0 == x.0));
            s.live_held = keep;
            if s.live_opts.note_length { gone } else { Vec::new() }
        });
        if let (false, Some(now)) = (gone.is_empty(), playing_tick) {
            change(app, ctx, "Live Record", |p, _, _, _| {
                let loop_len = p.rate.max(1) * p.steps.max(1) as u32;
                for (_, row, step, start) in gone {
                    let len = (now + loop_len - start) % loop_len;
                    live_release(p, row, step, len.max(1));
                }
            });
        }
    }
    if fresh.is_empty() {
        return;
    }
    if let Some(add) = learn {
        let (pitch, _) = fresh[0];
        let drums = is_drums(app, t);
        change(app, ctx, "Learn", |p, s, _, _| {
            let name = drums.then(|| drum_name(pitch)).flatten().map(str::to_string).unwrap_or_else(|| crate::app::note_name(pitch));
            if add {
                if !p.rows.iter().any(|x| x.pitch == pitch) {
                    place_row(p, Row::new(name, pitch), (!s.manual_order).then_some(s.sort));
                }
            } else if let Some(row) = p.rows.get_mut(s.selected_row) {
                row.pitch = pitch;
                row.name = name;
            }
            s.learn = None;
        });
        return;
    }
    if let (true, Some(tick)) = (live, playing_tick) {
        change(app, ctx, "Live Record", |p, s, _, _| {
            for (pitch, vel) in fresh {
                if let Some((row, step)) = live_place(p, pitch, vel, tick, s.live_opts, s.mono, (!s.manual_order).then_some(s.sort)) {
                    s.live_held.push((pitch, row, step, tick));
                }
            }
        });
    }
}

// ---- pattern files (the Pattern Browser's patterns and templates) -------------------------

fn patterns_dir(templates: bool) -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))).unwrap_or_else(|| ".".into());
    base.join("bramble-studio").join(if templates { "pattern-templates" } else { "patterns" })
}

fn default_path() -> std::path::PathBuf {
    patterns_dir(true).join("Default.pattern")
}

/// A pattern as text: its length, rate and playback mode, then each row and its set steps.
pub fn to_text(p: &StepPattern) -> String {
    let o = &p.opts;
    let opt = |v: Option<String>| v.unwrap_or_else(|| "-".into());
    let auto = |a: crate::stepinsp::AutoMode| if a == crate::stepinsp::AutoMode::Slide { "slide" } else { "latch" };
    let mut out = format!(
        "pattern\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        p.steps,
        p.rate,
        p.play.name(),
        o.legato as u8,
        o.swing,
        o.swing_div,
        opt(o.key.map(|k| k.0.to_string())),
        opt(o.key.map(|k| k.1.to_string())),
        o.no_transpose as u8,
        o.scale_quantize as u8,
        o.channel,
        auto(o.auto_mode)
    );
    for r in &p.rows {
        out += &format!(
            "row\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            r.pitch,
            r.name.replace('\t', " "),
            r.mute as u8,
            r.solo as u8,
            r.len.map_or("-".into(), |x| x.to_string()),
            r.rate.map_or("-".into(), |x| x.to_string()),
            r.start,
            r.play.map_or("-", |x| x.name()),
            opt(r.opts.channel.map(|c| c.to_string())),
            r.opts.auto_mode.map_or("-", auto)
        );
        for (k, s) in r.steps.iter().enumerate() {
            if *s != Step::default() {
                let cond = crate::stepinsp::Cond::ALL.iter().position(|c| *c == s.cond).unwrap_or(0);
                out += &format!(
                    "step\t{k}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{cond}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    s.on as u8, s.velocity, s.gate, s.repeat, s.chance, s.offset, s.tie as u8, s.octave, s.skip as u8, s.note,
                    s.iter, s.cycles, opt(s.rate.map(|x| x.to_string())), opt(s.channel.map(|x| x.to_string())), s.articulation, s.ramp, s.curve, s.dist
                );
            }
        }
    }
    out
}

pub fn from_text(text: &str) -> Option<StepPattern> {
    let mut p: Option<StepPattern> = None;
    let play = |s: &str| Play::ALL.iter().copied().find(|m| m.name() == s);
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("pattern") if f.len() >= 4 => {
                p = Some(StepPattern { steps: f[1].parse().ok()?, rate: f[2].parse().ok()?, rows: Vec::new(), play: play(f[3]).unwrap_or_default(), opts: Default::default() });
                if f.len() >= 13 {
                    let o = &mut p.as_mut()?.opts;
                    o.legato = f[4] == "1";
                    o.swing = f[5].parse().unwrap_or(0);
                    o.swing_div = f[6].parse().unwrap_or(240);
                    o.key = f[7].parse().ok().zip(f[8].parse().ok());
                    o.no_transpose = f[9] == "1";
                    o.scale_quantize = f[10] == "1";
                    o.channel = f[11].parse().unwrap_or(1);
                    o.auto_mode = if f[12] == "slide" { crate::stepinsp::AutoMode::Slide } else { crate::stepinsp::AutoMode::Latch };
                }
            }
            Some("row") if f.len() >= 9 => {
                let mut r = Row::new(f[2], f[1].parse().ok()?);
                r.mute = f[3] == "1";
                r.solo = f[4] == "1";
                r.len = f[5].parse().ok();
                r.rate = f[6].parse().ok();
                r.start = f[7].parse().unwrap_or(0);
                r.play = play(f[8]);
                if f.len() >= 11 {
                    r.opts.channel = f[9].parse().ok();
                    r.opts.auto_mode = match f[10] {
                        "slide" => Some(crate::stepinsp::AutoMode::Slide),
                        "latch" => Some(crate::stepinsp::AutoMode::Latch),
                        _ => None,
                    };
                }
                p.as_mut()?.rows.push(r);
            }
            Some("step") if f.len() >= 12 => {
                let k: usize = f[1].parse().ok()?;
                let n = |i: usize| f[i].parse::<i32>().unwrap_or(0);
                let s = Step { on: n(2) == 1, velocity: n(3) as u8, gate: n(4) as u8, repeat: n(5) as u8, chance: n(6) as u8, offset: n(7) as i8, tie: n(8) == 1, octave: n(9) as i8, skip: n(10) == 1, note: n(11) as i8, ..Step::default() };
                let mut s = s;
                if f.len() >= 21 {
                    s.cond = crate::stepinsp::Cond::ALL.get(n(12) as usize).copied().unwrap_or_default();
                    s.iter = n(13).max(1) as u8;
                    s.cycles = n(14).max(1) as u8;
                    s.rate = f[15].parse().ok();
                    s.channel = f[16].parse().ok();
                    s.articulation = n(17) as u8;
                    s.ramp = n(18) as i8;
                    s.curve = n(19) as i8;
                    s.dist = n(20) as i8;
                }
                if let Some(slot) = p.as_mut()?.rows.last_mut()?.steps.get_mut(k) {
                    *slot = s;
                }
            }
            _ => {}
        }
    }
    p
}

fn save_file(path: &std::path::Path, p: &StepPattern) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, to_text(p))
}

fn load_file(path: &std::path::Path) -> Option<StepPattern> {
    from_text(&std::fs::read_to_string(path).ok()?)
}

/// The saved patterns and templates: (name, path, template).
fn saved() -> Vec<(String, std::path::PathBuf, bool)> {
    let mut out = Vec::new();
    for tpl in [false, true] {
        if let Ok(rd) = std::fs::read_dir(patterns_dir(tpl)) {
            let mut v: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pattern")).collect();
            v.sort();
            for p in v {
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                out.push((name, p, tpl));
            }
        }
    }
    out
}

/// The Pattern Browser (⌥⇧B): Patterns and Templates saved here; a click loads one onto the
/// region (one undo step).
fn browser(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    p.text(pos2(area.min.x + 12.0, area.min.y + 14.0), Align2::LEFT_CENTER, "Pattern Browser", look::semibold(13.0), look::text());
    let list = saved();
    let mut pick = None;
    let mut y = area.min.y + 34.0;
    for (tpl, title) in [(false, "Patterns"), (true, "Templates")] {
        p.text(pos2(area.min.x + 12.0, y), Align2::LEFT_CENTER, title, look::semibold(11.0), look::text_dim());
        y += 18.0;
        let mine: Vec<&(String, std::path::PathBuf, bool)> = list.iter().filter(|x| x.2 == tpl).collect();
        if mine.is_empty() {
            p.text(pos2(area.min.x + 20.0, y), Align2::LEFT_CENTER, if tpl { "Save Template (⇧⌘T)" } else { "Save Pattern (⇧⌘P)" }, look::regular(11.0), look::text_dim());
            y += 20.0;
        }
        for (name, path, _) in mine {
            let r = Rect::from_min_size(pos2(area.min.x + 6.0, y - 10.0), vec2(area.width() - 12.0, 20.0));
            let resp = ui.interact(r, ui.scope_id().with(("ss-browse", path.to_string_lossy().to_string())), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, 4.0, look::group_bg());
            }
            p.text(pos2(area.min.x + 20.0, y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
            if resp.clicked() {
                pick = Some(path.clone());
            }
            y += 20.0;
        }
        y += 8.0;
    }
    if let Some(path) = pick {
        if let Some(pat) = load_file(&path) {
            let ctx = ui.ctx().clone();
            change(app, &ctx, "Load Pattern", |p, _, _, _| *p = pat);
        }
    }
}

// ---- Logic's saved patterns (GenM "StepSequencer", NSKeyedArchiver) ------------------------

/// A binary property list's objects.
#[derive(Clone, Debug)]
enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Data(Vec<u8>),
    Str(String),
    Uid(usize),
    Array(Vec<usize>),
    Dict(Vec<(usize, usize)>),
}

fn bplist(b: &[u8]) -> Option<(Vec<Obj>, usize)> {
    if !b.starts_with(b"bplist00") || b.len() < 40 {
        return None;
    }
    let tr = &b[b.len() - 32..];
    let off_size = tr[6] as usize;
    let ref_size = tr[7] as usize;
    let be = |s: &[u8]| s.iter().fold(0u64, |a, &x| (a << 8) | x as u64);
    let n = be(&tr[8..16]) as usize;
    let top = be(&tr[16..24]) as usize;
    let table = be(&tr[24..32]) as usize;
    if n > 1_000_000 || table + n * off_size > b.len() {
        return None;
    }
    let offset = |i: usize| be(&b[table + i * off_size..table + (i + 1) * off_size]) as usize;
    let mut objs = Vec::with_capacity(n);
    for i in 0..n {
        let o = offset(i);
        let m = *b.get(o)?;
        let (hi, lo) = (m >> 4, (m & 0xf) as usize);
        // a length, either in the marker or as an int object after it
        let len_at = |o: usize| -> Option<(usize, usize)> {
            if lo != 0xf {
                return Some((lo, o + 1));
            }
            let im = *b.get(o + 1)?;
            let w = 1usize << (im & 0xf);
            Some((be(b.get(o + 2..o + 2 + w)?) as usize, o + 2 + w))
        };
        let refs = |start: usize, count: usize| -> Option<Vec<usize>> { (0..count).map(|k| b.get(start + k * ref_size..start + (k + 1) * ref_size).map(|s| be(s) as usize)).collect() };
        let obj = match hi {
            0 => match m {
                8 => Obj::Bool(false),
                9 => Obj::Bool(true),
                _ => Obj::Null,
            },
            1 => {
                let w = 1usize << lo;
                let v = be(b.get(o + 1..o + 1 + w)?);
                Obj::Int(if w == 8 { v as i64 } else if w < 8 && v >> (w * 8 - 1) & 1 == 1 && w >= 8 { v as i64 } else { v as i64 })
            }
            2 => {
                let w = 1usize << lo;
                let s = b.get(o + 1..o + 1 + w)?;
                Obj::Real(if w == 4 { f32::from_be_bytes(s.try_into().ok()?) as f64 } else { f64::from_be_bytes(s.try_into().ok()?) })
            }
            3 => Obj::Real(f64::from_be_bytes(b.get(o + 1..o + 9)?.try_into().ok()?)),
            4 => {
                let (l, s) = len_at(o)?;
                Obj::Data(b.get(s..s + l)?.to_vec())
            }
            5 => {
                let (l, s) = len_at(o)?;
                Obj::Str(String::from_utf8_lossy(b.get(s..s + l)?).into_owned())
            }
            6 => {
                let (l, s) = len_at(o)?;
                let u: Vec<u16> = b.get(s..s + 2 * l)?.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Obj::Str(String::from_utf16_lossy(&u))
            }
            8 => Obj::Uid(be(b.get(o + 1..o + 2 + lo)?) as usize),
            0xa | 0xc => {
                let (l, s) = len_at(o)?;
                Obj::Array(refs(s, l)?)
            }
            0xd => {
                let (l, s) = len_at(o)?;
                let k = refs(s, l)?;
                let v = refs(s + l * ref_size, l)?;
                Obj::Dict(k.into_iter().zip(v).collect())
            }
            _ => Obj::Null,
        };
        objs.push(obj);
    }
    Some((objs, top))
}

/// NSKeyedArchiver values, resolved.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Data(Vec<u8>),
    List(Vec<Val>),
    Map(Vec<(String, Val)>),
}

impl Val {
    fn get(&self, k: &str) -> Option<&Val> {
        match self {
            Val::Map(m) => m.iter().find(|x| x.0 == k).map(|x| &x.1),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }
    fn truth(&self) -> bool {
        self.num().is_some_and(|n| n != 0.0)
    }
}

fn unarchive(b: &[u8]) -> Option<Val> {
    let (objs, top) = bplist(b)?;
    let plain_str = |i: usize| match objs.get(i) {
        Some(Obj::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let dict_get = |d: &[(usize, usize)], k: &str| d.iter().find(|(kk, _)| plain_str(*kk).as_deref() == Some(k)).map(|x| x.1);
    let Some(Obj::Dict(root)) = objs.get(top) else { return None };
    let archived = dict_get(root, "$objects").and_then(|i| match objs.get(i) {
        Some(Obj::Array(a)) => Some(a.clone()),
        _ => None,
    })?;
    let top_d = dict_get(root, "$top").and_then(|i| match objs.get(i) {
        Some(Obj::Dict(d)) => Some(d.clone()),
        _ => None,
    })?;
    let root_uid = dict_get(&top_d, "root").and_then(|i| match objs.get(i) {
        Some(Obj::Uid(u)) => Some(*u),
        _ => None,
    })?;
    fn res(objs: &[Obj], archived: &[usize], i: usize, uid: bool, depth: usize) -> Val {
        if depth > 40 {
            return Val::Null;
        }
        let idx = if uid { archived.get(i).copied() } else { Some(i) };
        let Some(o) = idx.and_then(|k| objs.get(k)) else { return Val::Null };
        let follow = |k: usize| match objs.get(k) {
            Some(Obj::Uid(u)) => res(objs, archived, *u, true, depth + 1),
            _ => res(objs, archived, k, false, depth + 1),
        };
        match o {
            Obj::Null => Val::Null,
            Obj::Bool(b) => Val::Bool(*b),
            Obj::Int(n) => Val::Num(*n as f64),
            Obj::Real(n) => Val::Num(*n),
            Obj::Data(d) => Val::Data(d.clone()),
            Obj::Str(s) if s == "$null" => Val::Null,
            Obj::Str(s) => Val::Str(s.clone()),
            Obj::Uid(u) => res(objs, archived, *u, true, depth + 1),
            Obj::Array(a) => Val::List(a.iter().map(|&k| follow(k)).collect()),
            Obj::Dict(d) => {
                let key = |k: usize| match objs.get(k) {
                    Some(Obj::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let find = |name: &str| d.iter().find(|(k, _)| key(*k) == name).map(|x| x.1);
                if let (Some(ks), Some(vs)) = (find("NS.keys"), find("NS.objects")) {
                    let (Val::List(ks), Val::List(vs)) = (follow(ks), follow(vs)) else { return Val::Null };
                    return Val::Map(ks.into_iter().zip(vs).map(|(k, v)| (if let Val::Str(s) = k { s } else { String::new() }, v)).collect());
                }
                if let Some(vs) = find("NS.objects") {
                    return follow(vs);
                }
                if let Some(s) = find("NS.string") {
                    return follow(s);
                }
                if let Some(s) = find("NS.data") {
                    return follow(s);
                }
                Val::Map(d.iter().filter(|(k, _)| key(*k) != "$class").map(|&(k, v)| (key(k), follow(v))).collect())
            }
        }
    }
    Some(res(&objs, &archived, root_uid, true, 0))
}

/// A pattern from Logic's "StepSequencer" archive (rows, steps on, ties, skips, row lengths,
/// per-step values where saved).
fn pattern_from_archive(v: &Val) -> Option<StepPattern> {
    let seq = v.get("StepSequencer")?.get("kSessionDataKey")?.get("kSequenceDataKey")?;
    let end = seq.get("kStepSequenceEndIndexKey").and_then(Val::num).unwrap_or(16.0) as u8;
    let start = seq.get("kStepSequenceStartIndexKey").and_then(Val::num).unwrap_or(0.0) as u8;
    let Val::List(rows) = seq.get("kStepSequenceRowsKey")? else { return None };
    let mut pat = StepPattern { steps: end.saturating_sub(start).clamp(1, MAX_STEPS as u8), rate: SIXTEENTH, rows: Vec::new(), play: Play::Forward, opts: Default::default() };
    for r in rows {
        let nk = r.get("propertyNodeKey").and_then(|p| p.get("kStepSequenceNoteKey"));
        let note = nk.and_then(|n| n.get("kStepSequenceNoteKey")).and_then(Val::num).unwrap_or(60.0);
        let oct = nk.and_then(|n| n.get("kStepSequenceOctaveTransposeKey")).and_then(Val::num).unwrap_or(0.0);
        let tr = nk.and_then(|n| n.get("kStepSequenceNoteTransposeKey")).and_then(Val::num).unwrap_or(0.0);
        let pitch = (note + 12.0 * oct + tr).clamp(0.0, 127.0) as u8;
        let mut row = Row::new(crate::app::note_name(pitch), pitch);
        let rend = r.get("kStepSequenceEndIndexKey").and_then(Val::num).map(|e| e as u8);
        let rstart = r.get("kStepSequenceStartIndexKey").and_then(Val::num).unwrap_or(0.0) as u8;
        if let Some(e) = rend.filter(|&e| e.saturating_sub(rstart) != pat.steps) {
            row.len = Some(e.saturating_sub(rstart).clamp(1, MAX_STEPS as u8));
        }
        if let Some(Val::List(steps)) = r.get("kStepSequenceStepsKey") {
            for (k, s) in steps.iter().enumerate().take(MAX_STEPS) {
                let st = &mut row.steps[k];
                st.on = s.get("kStepSequenceStepEnabledKey").is_some_and(Val::truth);
                st.tie = s.get("kStepSequenceTieEnabledKey").is_some_and(Val::truth);
                st.skip = s.get("kStepSequenceSkipEnabledKey").is_some_and(Val::truth);
                // per-step values: names as Logic's edit modes (0..1 floats or plain numbers)
                if let Some(Val::Map(props)) = s.get("propertyNodeKey") {
                    for (name, val) in props {
                        let Some(x) = val.num().or_else(|| val.get(name).and_then(Val::num)) else { continue };
                        let frac = if x <= 1.0 { x } else { f64::NAN };
                        let n = name.to_lowercase();
                        if n.contains("velocity") {
                            st.velocity = if frac.is_nan() { x.clamp(1.0, 127.0) as u8 } else { (frac * 127.0).round().max(1.0) as u8 };
                        } else if n.contains("gate") {
                            st.gate = if frac.is_nan() { x.clamp(1.0, 100.0) as u8 } else { (frac * 100.0).round().max(1.0) as u8 };
                        } else if n.contains("repeat") {
                            st.repeat = x.clamp(1.0, 16.0) as u8;
                        } else if n.contains("chance") || n.contains("probab") {
                            st.chance = if frac.is_nan() { x.clamp(0.0, 100.0) as u8 } else { (frac * 100.0).round() as u8 };
                        } else if n.contains("offset") {
                            st.offset = if frac.is_nan() { x.clamp(-50.0, 50.0) as i8 } else { (frac * 100.0 - 50.0).round() as i8 };
                        } else if n.contains("octave") {
                            st.octave = x.clamp(-4.0, 4.0) as i8;
                        }
                    }
                }
            }
        }
        pat.rows.push(row);
    }
    pat.rows.sort_by(|a, b| b.pitch.cmp(&a.pitch));
    Some(pat)
}

/// The Step Sequencer patterns saved in a Logic project's ProjectData, in file order.
#[allow(dead_code)] // for the app's commands (see the wiring notes)
pub fn read_patterns(project_data: &[u8]) -> Vec<StepPattern> {
    let Ok(chunks) = lg_core::chunks::walk(project_data) else { return Vec::new() };
    chunks
        .iter()
        .filter(|c| c.is("GenM"))
        .filter_map(|c| {
            let at = c.payload.windows(8).position(|w| w == b"bplist00")?;
            let v = unarchive(&c.payload[at..])?;
            pattern_from_archive(&v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drums_with(rows: &[(u8, &str)]) -> StepPattern {
        let mut p = StepPattern { steps: 16, rate: SIXTEENTH, rows: Vec::new(), play: Play::Forward, opts: Default::default() };
        for (pitch, grid) in rows {
            let mut r = Row::new(drum_name(*pitch).unwrap_or("x"), *pitch);
            for (k, c) in grid.chars().enumerate() {
                r.steps[k].on = c == 'x';
            }
            p.rows.push(r);
        }
        p
    }

    #[test]
    fn four_on_the_floor() {
        let p = drums_with(&[(36, "x...x...x...x..."), (38, "....x.......x...")]);
        let n = p.render(3840, 0);
        let kicks: Vec<u32> = n.iter().filter(|x| x.pitch == 36).map(|x| x.pos).collect();
        assert_eq!(kicks, [0, 960, 1920, 2880]);
        let snares: Vec<u32> = n.iter().filter(|x| x.pitch == 38).map(|x| x.pos).collect();
        assert_eq!(snares, [960, 2880]);
        assert!(n.iter().all(|x| x.len == 240 && x.velocity == 100));
        // two bars: the pattern loops
        assert_eq!(p.render(7680, 0).iter().filter(|x| x.pitch == 36).count(), 8);
    }

    #[test]
    fn mute_and_solo() {
        let mut p = drums_with(&[(36, "x..............."), (38, "x...............")]);
        p.rows[0].mute = true;
        assert_eq!(p.render(3840, 0).iter().map(|n| n.pitch).collect::<Vec<_>>(), [38]);
        p.rows[0].mute = false;
        p.rows[0].solo = true;
        assert_eq!(p.render(3840, 0).iter().map(|n| n.pitch).collect::<Vec<_>>(), [36]);
    }

    #[test]
    fn step_values() {
        let mut p = drums_with(&[(60, "xx..x...x...x...")]);
        let s = &mut p.rows[0].steps;
        s[0].velocity = 40;
        s[0].gate = 50;
        s[1].tie = true; // carries step 0 on through step 1
        s[4].repeat = 4;
        s[8].octave = 1;
        s[12].offset = 25;
        let n = p.render(3840, 0);
        assert_eq!((n[0].pos, n[0].len, n[0].velocity), (0, 480, 40));
        let reps: Vec<u32> = n.iter().filter(|x| x.pos >= 960 && x.pos < 1200).map(|x| x.pos).collect();
        assert_eq!(reps, [960, 1020, 1080, 1140]);
        assert!(n.iter().any(|x| x.pos == 1920 && x.pitch == 72));
        assert!(n.iter().any(|x| x.pos == 2880 + 60));
    }

    #[test]
    fn chance_skip_and_length() {
        let mut p = drums_with(&[(36, "xxxxxxxxxxxxxxxx")]);
        p.rows[0].steps.iter_mut().for_each(|s| s.chance = 0);
        assert!(p.render(3840, 7).is_empty());
        let mut p = drums_with(&[(36, "x.x.")]);
        p.steps = 4;
        // skip the 2nd step: x x . (x) … plays every other slot from step 3 on
        p.rows[0].steps[1].skip = true;
        let pos: Vec<u32> = p.render(240 * 6, 0).iter().map(|n| n.pos).collect();
        // slots: 0→s0 x, 1→s2 x, 2→s3 ., 3→s0 x, 4→s2 x, 5→s3 .
        assert_eq!(pos, [0, 240, 720, 960]);
        // a row of 3 steps against a 4-step pattern: polyrhythm
        let mut p = drums_with(&[(36, "x...")]);
        p.steps = 4;
        p.rows[0].len = Some(3);
        assert_eq!(p.render(240 * 9, 0).iter().map(|n| n.pos).collect::<Vec<_>>(), [0, 720, 1440]);
        // an eighth-note rate
        let mut p = drums_with(&[(36, "xx")]);
        p.steps = 2;
        p.rate = 480;
        assert_eq!(p.render(1920, 0).iter().map(|n| n.pos).collect::<Vec<_>>(), [0, 480, 960, 1440]);
    }

    #[test]
    fn notes_round_trip() {
        let p = drums_with(&[(42, "x.x.x.x.x.x.x.x."), (36, "x.......x.......")]);
        let notes = p.render(3840, 0);
        let q = StepPattern::from_notes(&notes, 3840, true);
        assert_eq!(q.rows.len(), 2);
        assert_eq!(q.render(3840, 0), notes);
        // a long note becomes tied steps, and plays back as one note
        let long = [Note { channel: 0, pitch: 60, velocity: 90, pos: 0, len: 720, muted: false }];
        let q = StepPattern::from_notes(&long, 3840, false);
        assert!(q.rows[0].steps[1].tie && q.rows[0].steps[2].tie);
        assert_eq!(q.render(3840, 0), long.to_vec());
    }

    #[test]
    fn logic_pattern_from_genm() {
        // E23 (research sample): a Drum Machine Designer pattern, Kick |x...x...x...x...|
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files/E23-live-loops-base.logicx/Alternatives/000/ProjectData");
        let Ok(data) = std::fs::read(&path) else { return };
        let pats = read_patterns(&data);
        assert_eq!(pats.len(), 1);
        let p = &pats[0];
        assert_eq!(p.steps, 16);
        assert_eq!(p.rows.len(), 23);
        let grid = |pitch: u8| -> String { p.rows.iter().find(|r| r.pitch == pitch).map(|r| r.steps[..16].iter().map(|s| if s.on { 'x' } else { '.' }).collect()).unwrap_or_default() };
        assert_eq!(grid(24), "x...x...x...x...");
        assert_eq!(grid(66), "....x.......x...");
    }

    #[test]
    fn playback_orders() {
        assert_eq!(play_order(Play::Forward, 4, 6, 0, 0), [0, 1, 2, 3, 0, 1]);
        assert_eq!(play_order(Play::Reverse, 4, 6, 0, 0), [3, 2, 1, 0, 3, 2]);
        // pendulum turns at the ends without playing them twice; ping pong plays them twice
        assert_eq!(play_order(Play::Pendulum, 4, 8, 0, 0), [0, 1, 2, 3, 2, 1, 0, 1]);
        assert_eq!(play_order(Play::PingPong, 4, 10, 0, 0), [0, 1, 2, 3, 3, 2, 1, 0, 0, 1]);
        assert_eq!(play_order(Play::Pendulum, 1, 3, 0, 0), [0, 0, 0]);
        // shuffle: every step once per pass
        let s = play_order(Play::Shuffle, 8, 16, 42, 1);
        for pass in s.chunks(8) {
            let mut v = pass.to_vec();
            v.sort_unstable();
            assert_eq!(v, (0..8).collect::<Vec<_>>());
        }
        // wander moves one step at a time; random stays in range; both repeatable
        let w = play_order(Play::Wander, 8, 64, 7, 0);
        assert!(w.windows(2).all(|p| (p[0] + 8 - p[1]) % 8 == 1 || (p[1] + 8 - p[0]) % 8 == 1));
        assert_eq!(w, play_order(Play::Wander, 8, 64, 7, 0));
        assert!(play_order(Play::Random, 5, 50, 3, 0).iter().all(|&k| k < 5));
    }

    #[test]
    fn reverse_pattern_and_row_modes() {
        let mut p = drums_with(&[(36, "x..."), (38, "x...")]);
        p.steps = 4;
        p.play = Play::Reverse;
        // the kick's step 1 comes last in each pass
        assert_eq!(p.render(960, 0).iter().filter(|n| n.pitch == 36).map(|n| n.pos).collect::<Vec<_>>(), [720]);
        // a row of its own plays forward
        p.rows[1].play = Some(Play::Forward);
        assert_eq!(p.render(960, 0).iter().filter(|n| n.pitch == 38).map(|n| n.pos).collect::<Vec<_>>(), [0]);
        // loop start: the row starts at its third step
        let mut p = drums_with(&[(36, "..x.")]);
        p.steps = 4;
        p.rows[0].start = 2;
        assert_eq!(p.render(960, 0).iter().map(|n| n.pos).collect::<Vec<_>>(), [0, 480]);
        // note mode: a step a fifth up
        let mut p = drums_with(&[(60, "x...")]);
        p.rows[0].steps[0].note = 7;
        assert_eq!(p.render(3840, 0)[0].pitch, 67);
    }

    #[test]
    fn musical_length() {
        // rows that loop after 4 steps at 1/16: one beat, even with 16 steps
        let mut p = drums_with(&[(36, "x..."), (42, "xx..")]);
        p.steps = 16;
        p.rows.iter_mut().for_each(|r| r.len = Some(4));
        assert_eq!(p.musical_len(), Some(960));
        // the longest row decides when the others divide it
        p.rows[1].len = Some(8);
        assert_eq!(p.musical_len(), Some(1920));
        // 3 and 4 steps realign only after 12
        p.rows[0].len = Some(3);
        p.rows[1].len = Some(4);
        assert_eq!(p.musical_len(), Some(12 * 240));
        // and playback really repeats there, not before
        let a = p.render(12 * 240, 0);
        let b = p.render(24 * 240, 0);
        assert_eq!(b.len(), 2 * a.len());
        assert!(b[a.len()..].iter().zip(&a).all(|(x, y)| x.pos == y.pos + 12 * 240 && x.pitch == y.pitch));
        let c = p.render(4 * 240, 0);
        assert!(b.iter().filter(|n| n.pos >= 4 * 240 && n.pos < 8 * 240).map(|n| (n.pos - 4 * 240, n.pitch)).collect::<Vec<_>>() != c.iter().map(|n| (n.pos, n.pitch)).collect::<Vec<_>>());
        // Skip shortens a row; Ping-Pong doubles it; Random never repeats
        let mut q = drums_with(&[(36, "x...")]);
        q.rows[0].len = Some(4);
        assert_eq!(q.musical_len(), Some(960));
        q.rows[0].steps[3].skip = true;
        assert_eq!(q.musical_len(), Some(720));
        q.rows[0].steps[3].skip = false;
        q.play = Play::PingPong;
        assert_eq!(q.musical_len(), Some(1920));
        q.play = Play::Pendulum;
        assert_eq!(q.musical_len(), Some(6 * 240));
        q.rows[0].play = Some(Play::Random);
        assert_eq!(q.musical_len(), None);
    }

    #[test]
    fn longer_pattern_repeats_and_rates() {
        let mut p = drums_with(&[(36, "x.x.")]);
        p.steps = 4;
        set_length(&mut p, 12);
        assert_eq!(p.steps, 12);
        let on: Vec<bool> = p.rows[0].steps[..12].iter().map(|s| s.on).collect();
        assert_eq!(on, [true, false, true, false, true, false, true, false, true, false, true, false]);
        // the Pattern Length menu's sections
        assert_eq!(LENGTH_SECTIONS.map(|s| s.0.split(' ').next().unwrap().to_string()), ["4", "3", "5", "7"]);
        assert!(LENGTH_SECTIONS[3].1.iter().all(|n| n % 7 == 0));
        // the rates: whole-note multiples, triplets, dotted
        let tick = |n: &str| RATES.iter().find(|r| r.0 == n).unwrap().1;
        assert_eq!((tick("4"), tick("2"), tick("1")), (4 * 3840, 2 * 3840, 3840));
        assert_eq!(tick("8T") * 3, 480 * 2);
        assert_eq!(tick("16D"), 240 * 3 / 2);
        for n in ["1T", "2T", "4T", "8T", "16T", "32T", "1D", "2D", "4D", "8D", "16D", "32D", "64D", "/64"] {
            tick(n);
        }
        // a dotted 1/8 step plays three sixteenths long
        let mut d = drums_with(&[(36, "xx")]);
        d.steps = 2;
        d.rate = tick("8D");
        assert_eq!(d.render(1440, 0).iter().map(|n| n.pos).collect::<Vec<_>>(), [0, 720]);
    }

    #[test]
    fn live_recording_options() {
        let o = LiveOpts::default();
        let mut p = drums_with(&[(36, "....")]);
        p.steps = 16;
        // quantize: a note just before step 3 lands on it
        assert_eq!(live_place(&mut p, 36, 90, 2 * 240 - 30, o, false, Some((0, false))), Some((0, 2)));
        assert_eq!(p.rows[0].steps[2].offset, 0);
        // quantize off: the step it falls in, offset for the exact time
        let q = LiveOpts { quantize: false, velocity: false, ..o };
        assert_eq!(live_place(&mut p, 36, 90, 4 * 240 + 60, q, false, Some((0, false))), Some((0, 4)));
        assert_eq!((p.rows[0].steps[4].offset, p.rows[0].steps[4].velocity), (25, 100));
        // a new note gets a row; mono keeps only the last note in the column
        assert_eq!(live_place(&mut p, 38, 80, 4 * 240, o, true, Some((0, false))).map(|x| x.1), Some(4));
        let k36 = p.rows.iter().position(|r| r.pitch == 36).unwrap();
        assert!(!p.rows[k36].steps[4].on);
        // note length: short notes shorten the gate, long ones tie on
        live_release(&mut p, k36, 2, 120);
        assert_eq!(p.rows[k36].steps[2].gate, 50);
        live_release(&mut p, k36, 8, 240 * 2 + 120);
        let s = &p.rows[k36].steps;
        assert!(s[9].tie && s[10].tie && !s[11].on && s[10].gate == 50);
        p.rows[k36].steps[8].on = true;
        let notes: Vec<_> = p.render(16 * 240, 0).into_iter().filter(|n| n.pitch == 36 && n.pos == 8 * 240).collect();
        assert_eq!(notes[0].len, 240 * 2 + 120);
    }

    #[test]
    fn new_rows_follow_the_sort_order() {
        let mut p = drums_with(&[(40, ""), (36, "")]);
        place_row(&mut p, Row::new("x", 38), Some((0, false)));
        assert_eq!(p.rows.iter().map(|r| r.pitch).collect::<Vec<_>>(), [40, 38, 36]);
        place_row(&mut p, Row::new("y", 39), Some((0, true)));
        assert_eq!(p.rows.iter().map(|r| r.pitch).collect::<Vec<_>>(), [36, 38, 39, 40]);
        // after a row was moved by hand: at the end
        place_row(&mut p, Row::new("z", 37), None);
        assert_eq!(p.rows.last().unwrap().pitch, 37);
    }

    #[test]
    fn randomize_dialog_settings() {
        let on = Step { on: true, velocity: 64, ..Step::default() };
        // offset: 50% of 64 gives 32–96
        let o = RandOpts { range_mode: false, offset: 50, ..RandOpts::default() };
        let vals: Vec<u8> = (0..=10).map(|k| { let mut s = on; randomize_with(Mode::Velocity, &mut s, k as f32 / 10.0, o); s.velocity }).collect();
        assert_eq!((vals[0], vals[10]), (32, 96));
        // range: between the limits (velocity 1–127, repeats 1–16)
        let o = RandOpts { range_mode: true, low: 100, high: 200, ..RandOpts::default() };
        let mut s = on;
        randomize_with(Mode::Velocity, &mut s, 1.0, o);
        assert_eq!(s.velocity, 127);
        randomize_with(Mode::Velocity, &mut s, 0.0, o);
        assert_eq!(s.velocity, 100);
        randomize_with(Mode::Repeat, &mut s, 1.0, o);
        assert_eq!(s.repeat, 16);
        // probability for on/off, tie and skip
        let o = RandOpts { probability: 70, ..RandOpts::default() };
        let mut s = Step::default();
        randomize_with(Mode::OnOff, &mut s, 0.6, o);
        assert!(s.on);
        randomize_with(Mode::Skip, &mut s, 0.8, o);
        assert!(!s.skip);
    }

    #[test]
    fn ramps() {
        let mut steps = vec![Step::default(); 8];
        steps[2].on = true;
        ramp_values(&mut steps, Mode::Velocity, 0, 0.0, 4, 1.0, false);
        assert_eq!(steps[..5].iter().map(|s| s.velocity).collect::<Vec<_>>(), [1, 33, 64, 96, 127]);
        assert!(steps[..5].iter().all(|s| s.on) && !steps[5].on);
        // with Shift: only the steps already on
        let mut steps = vec![Step::default(); 8];
        steps[2].on = true;
        ramp_values(&mut steps, Mode::Gate, 4, 0.0, 0, 1.0, true);
        assert!(steps.iter().filter(|s| s.on).count() == 1 && steps[2].gate == 51);
    }

    #[test]
    fn preview_sends_notes_as_they_come() {
        let p = drums_with(&[(36, "x...x...x...x...")]);
        let notes = p.render(16 * 240, 0);
        assert_eq!(preview_due(&notes, 0, 0).len(), 1);
        assert_eq!(preview_due(&notes, 1, 959).len(), 0);
        assert_eq!(preview_due(&notes, 1, 960).len(), 1);
        assert_eq!(preview_due(&notes, 0, 16 * 240).len(), 4);
    }

    #[test]
    fn step_recording() {
        let mut p = drums_with(&[(36, "....")]);
        let mut s = Store::default();
        // a chord: both notes in step 1, a row made for the new pitch
        step_record(&mut p, &mut s, &[(36, 90), (40, 70)], false);
        assert_eq!(p.rows.len(), 2);
        let row = |p: &StepPattern, pitch: u8| p.rows.iter().find(|r| r.pitch == pitch).unwrap().steps[0];
        assert!(row(&p, 36).on && row(&p, 40).on && row(&p, 40).velocity == 70);
        // released: the next step; the next note goes there
        step_advance(&mut s, 4);
        step_record(&mut p, &mut s, &[(36, 100)], false);
        assert!(p.rows.iter().find(|r| r.pitch == 36).unwrap().steps[1].on);
        // after the last step, back to the first
        s.sel = Some(((0, 3), (0, 3)));
        step_advance(&mut s, 4);
        assert_eq!(s.sel.unwrap().1 .1, 0);
    }

    #[test]
    fn page_cut_copy_paste_clear() {
        let mut p = drums_with(&[(36, "x...............x.x.")]);
        p.steps = 32;
        let mut clip = Vec::new();
        page_op(&mut p, 1, PageOp::Copy, &mut clip);
        page_op(&mut p, 0, PageOp::Paste, &mut clip);
        assert!(p.rows[0].steps[0].on && p.rows[0].steps[2].on && !p.rows[0].steps[1].on);
        page_op(&mut p, 1, PageOp::Cut, &mut clip);
        assert!(p.rows[0].steps[16..32].iter().all(|s| !s.on));
        page_op(&mut p, 0, PageOp::Clear, &mut clip);
        assert!(p.rows[0].steps[..16].iter().all(|s| !s.on));
        page_op(&mut p, 1, PageOp::Paste, &mut clip);
        assert!(p.rows[0].steps[16].on && p.rows[0].steps[18].on);
    }

    #[test]
    fn increment_decrement_and_mono() {
        let mut p = drums_with(&[(36, "x..."), (38, "....")]);
        p.steps = 4;
        nudge_steps(&mut p.rows[0].steps[..4], Mode::OnOff, 1);
        assert!(p.rows[0].steps[..4].iter().all(|s| s.on));
        nudge_steps(&mut p.rows[0].steps[..4], Mode::Tie, 1);
        assert_eq!(p.rows[0].steps[..4].iter().map(|s| s.tie).collect::<Vec<_>>(), [false, true, true, true]);
        assert_eq!(p.render(960, 0).len(), 1);
        nudge_steps(&mut p.rows[0].steps[..4], Mode::Tie, -1);
        assert!(p.rows[0].steps[..4].iter().all(|s| !s.tie));
        let v0 = p.rows[0].steps[0].velocity;
        nudge_steps(&mut p.rows[0].steps[..4], Mode::Velocity, -1);
        assert!(p.rows[0].steps[0].velocity < v0);
        nudge_steps(&mut p.rows[0].steps[..4], Mode::OnOff, -1);
        assert!(p.rows[0].steps[..4].iter().all(|s| !s.on));
        // Mono Mode: never two notes in one column
        p.rows[0].steps[1].on = true;
        p.rows[1].steps[1].on = true;
        p.rows[1].steps[2].on = true;
        mono_columns(&mut p, &[1]);
        assert!(!p.rows[0].steps[1].on && p.rows[1].steps[1].on && p.rows[1].steps[2].on);
        // a header control acts on every selected row when its row is one of them
        assert_eq!(header_rows(Some(2), &[1, 2, 3], 5), [1, 2, 3]);
        assert_eq!(header_rows(Some(4), &[1, 2, 3], 5), [4]);
        assert_eq!(header_rows(None, &[], 2), [0, 1]);
    }

    #[test]
    fn pattern_files_round_trip() {
        let mut p = drums_with(&[(36, "x...x..."), (42, "xxxxxxxx")]);
        p.play = Play::PingPong;
        p.rate = 480;
        p.rows[0].steps[4] = Step { on: true, velocity: 40, gate: 50, repeat: 3, chance: 70, offset: -10, tie: false, octave: 1, skip: false, note: -2, ..Step::default() };
        p.rows[1].len = Some(6);
        p.rows[1].start = 1;
        p.rows[1].play = Some(Play::Wander);
        p.rows[1].mute = true;
        assert_eq!(from_text(&to_text(&p)), Some(p));
        assert_eq!(from_text("nonsense"), None);
    }

    #[test]
    fn vertical_zoom_default_is_logics_48pt_rows_and_inverts() {
        assert_eq!(row_h_of_zoom(DEFAULT_VZ), 48.0);
        assert_eq!(row_h_of_zoom(0.0), ROW_MIN);
        assert_eq!(row_h_of_zoom(1.0), ROW_MAX);
        for v in [0.0f32, 0.2, 0.6, 0.8, 1.0] {
            assert!((zoom_of_row_h(row_h_of_zoom(v)) - v).abs() < 1e-4);
        }
    }

    #[test]
    fn vertical_auto_zoom_fills_the_height() {
        assert_eq!(auto_row_h(8, 0.0, 480.0), 60.0);
        // too many rows: the smallest; too few: the largest
        assert_eq!(auto_row_h(40, 0.0, 480.0), ROW_MIN);
        assert_eq!(auto_row_h(1, 0.0, 480.0), ROW_MAX);
        // open subrows take their room first
        assert_eq!(auto_row_h(4, 90.0, 290.0), 50.0);
    }

    #[test]
    fn row_assignment_renames_and_keeps_steps() {
        let mut row = Row::new("Kick 1", 36);
        row.steps[2].on = true;
        assign_row(&mut row, 38, true);
        assert_eq!((row.pitch, row.name.as_str(), row.steps[2].on), (38, "Snare 1", true));
        assign_row(&mut row, 60, false);
        assert_eq!(row.name, crate::app::note_name(60));
    }

    #[test]
    fn logic_step_colours() {
        assert_eq!(off_color(kit_color("Kick 1")), Color32::from_rgb(0x34, 0x0f, 0x2c));
        assert_eq!(off_color(kit_color("Snare 1")), Color32::from_rgb(0x33, 0x23, 0x0f));
        assert_eq!(off_color(row_color()), Color32::from_rgb(0x1e, 0x1a, 0x32));
        assert_eq!(off_color(kit_color("Hi-Hat 1")), Color32::from_rgb(0x02, 0x31, 0x30));
    }

    #[test]
    fn drum_instruments_get_kit_rows() {
        assert_eq!(kit_for("Drum Machine Designer", "Beat"), Some(Kit::DmdDefault));
        assert_eq!(kit_for("Drum Synth Kit", "Track 3"), Some(Kit::DmdDefault));
        assert_eq!(kit_for("Drum Kit Designer", "SoCal"), Some(Kit::Gm));
        assert_eq!(kit_for("Retro Synth", "Lead"), None);
        assert_eq!(kit_for("Deluxe Classic", "Deluxe Classic"), None);
        // a DMD of the user's own samples: rows named after its pads, kick first
        let c = crate::soundlib::sampler_choice("Drum Machine Designer", &[(42, "/k/Closed Hat.wav".into()), (36, "/k/Big Kick.wav".into()), (38, "/k/Snare.wav".into())]);
        let Some(kit @ Kit::DmdPads(_)) = kit_for(&c, "Drum Machine Designer") else { panic!("pads") };
        let rows = kit_rows(&kit);
        assert_eq!(rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["Big Kick", "Snare", "Closed Hat"]);
        // the default kit: kick, snare, rim, clap, hi-hats, toms, crash, ride
        let rows = kit_rows(&Kit::DmdDefault);
        assert_eq!(rows[0], (36, "Kick 2".to_string()));
        assert_eq!(rows[1].1, "Snare 1");
        assert_eq!(rows.len(), 12);
        assert!(rows.iter().all(|r| !r.1.is_empty()));
        assert!(kit_rows(&Kit::Gm).iter().all(|r| !r.1.is_empty()));
    }
}
