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
/// Ping Pong goes forward then back without repeating the ends (1 2 3 4 3 2 …); Pendulum
/// plays the ends twice (1 2 3 4 4 3 2 1 …); Shuffle plays every step once per pass in a
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
        Play::PingPong => {
            if n == 1 {
                return vec![0; slots];
            }
            let period = 2 * n - 2;
            (0..slots).map(|k| { let p = k % period; if p < n { p } else { period - p } }).collect()
        }
        Play::Pendulum => {
            let period = 2 * n;
            (0..slots).map(|k| { let p = k % period; if p < n { p } else { period - 1 - p } }).collect()
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
        let rows = [36u8, 38, 37, 39, 42, 44, 46, 45, 47, 50, 49, 51].iter().map(|&p| Row::new(drum_name(p).unwrap_or_default(), p)).collect();
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
            Mode::Rate => 1.0 - RATE_ORDER.iter().position(|&r| Some(r) == s.rate).unwrap_or(7) as f32 / 11.0,
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
            Mode::Rate => s.rate = Some(RATE_ORDER[((1.0 - f) * 11.0).round() as usize]),
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

/// Step rates: Logic's /1 … /64 and the triplets.
pub const RATES: [(&str, u32); 12] = [("/1", 3840), ("/2", 1920), ("/4", 960), ("/8", 480), ("/16", 240), ("/32", 120), ("/64", 60), ("/3", 1280), ("/6", 640), ("/12", 320), ("/24", 160), ("/48", 80)];

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
    /// The press in progress: (row, paint state for toggles).
    press: Option<(usize, bool)>,
    pressed_cells: Vec<(usize, usize)>,
    changed: bool,
    seed: u64,
    /// The step selection: anchor and cursor (row, step).
    sel: Option<((usize, usize), (usize, usize))>,
    /// Copy Row's row, and the steps copied (rows of steps).
    clip_row: Option<Row>,
    clip_steps: Vec<Vec<Step>>,
    /// The Pattern Browser shown; step colours by the region's colour; Mono Mode; Live Record;
    /// Learn waiting for a note (true: add a row, false: assign the selected row).
    browser: bool,
    by_region: bool,
    mono: bool,
    live_record: bool,
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
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| {
        let n = tr.name.to_lowercase();
        tr.kind == lg_core::TrackKind::Drummer || ["drum", "kit", "beat", "perc", "808", "909"].iter().any(|w| n.contains(w))
    })
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
    let pattern = StepPattern::from_notes(&rel, rg.length, is_drums(app, t));
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
    let drums = is_drums(app, t);
    let Some(name) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()) else { return };
    app.checkpoint("Create Pattern Region");
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    tr.regions.push(lg_core::Region { name, start: at, length: bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    let r = tr.regions.len() - 1;
    with_store(ctx, |s| {
        // the default pattern template when one is stored (Store as Default Pattern Template)
        let pattern = load_file(&default_path()).unwrap_or_else(|| if drums { StepPattern::drums() } else { StepPattern::melodic() });
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
const HEADER_W: f32 = 367.0;

fn grid_bg() -> Color32 {
    Color32::from_rgb(0x14, 0x14, 0x14)
}
fn cell_off() -> Color32 {
    Color32::from_rgb(0x20, 0x1b, 0x33)
}
/// Row colour (Logic's default purple) and steps on.
fn row_color() -> Color32 {
    Color32::from_rgb(0x5a, 0x41, 0xce)
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
    let drums = is_drums(app, t);
    with_store(&ctx, |s| {
        let e = s.patterns.entry((t, r)).or_default();
        let mut sorted = rel.clone();
        sorted.sort_by_key(|n| (n.pos, std::cmp::Reverse(n.pitch)));
        if e.written != sorted && !(e.written.is_empty() && sorted.is_empty() && !e.pattern.rows.is_empty()) {
            let mut fresh = StepPattern::from_notes(&rel, region.length, drums);
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

    // the tab (a pattern region has only the Step Sequencer)
    let tab = Rect::from_center_size(pos2(area.center().x, area.min.y + 20.0), vec2(160.0, 26.0));
    p.rect_filled(tab, 8.0, look::group_bg());
    p.rect_filled(tab.shrink(2.0), 7.0, look::accent());
    p.text(tab.center(), Align2::CENTER_CENTER, "Step Sequencer", look::regular(13.0), Color32::WHITE);
    // the toolbar: local menus, and the modes chooser for the selected row
    let bar = Rect::from_min_size(pos2(area.min.x, area.min.y + 40.0), vec2(area.width(), TOOLBAR_H));
    p.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, look::line()));
    let mut sel_row = with_store(&ctx, |s| s.selected_row).min(pat.rows.len().saturating_sub(1));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(12.0, 6.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "stepseq-Edit"), ("Functions", "stepseq-Functions"), ("View", "stepseq-View")]);
        // Pattern Browser | Inspector: one joined pill of two toggles (Logic's)
        ui.add_space(10.0);
        let (pill, _) = ui.allocate_exact_size(vec2(60.0, 26.0), Sense::hover());
        ui.painter().rect_filled(pill, 7.0, look::group_bg());
        let (b_on, i_on) = with_store(ui.ctx(), |s| (s.browser, s.inspector));
        for (k, (icon, on, tip)) in [(crate::glyphs::ph::TABLE, b_on, "Pattern Browser"), (crate::glyphs::ph::SLIDERS_HORIZONTAL, i_on, "Show/Hide Local Inspector")].into_iter().enumerate() {
            let b = Rect::from_min_size(pos2(pill.min.x + k as f32 * 30.0, pill.min.y), vec2(30.0, 26.0));
            if on {
                ui.painter().rect_filled(b.shrink(2.0), 6.0, look::button_on());
            }
            crate::glyphs::paint(ui.painter(), Rect::from_center_size(b.center(), vec2(16.0, 16.0)), icon, 15.0, if on { look::accent() } else { look::text() });
            if ui.interact(b, ui.id().with(("ss-tb", k)), Sense::click()).on_hover_text(tip).clicked() {
                with_store(ui.ctx(), |s| if k == 0 { s.browser = !s.browser } else { s.inspector = !s.inspector });
            }
        }
        ui.painter().vline(pill.center().x, pill.y_range().shrink(6.0), Stroke::new(1.0, look::line()));
        let (learn, live, mono) = with_store(ui.ctx(), |s| (s.learn, s.live_record, s.mono));
        let mut flags = Vec::new();
        if let Some(add) = learn {
            flags.push(if add { "Learn (Add): play a note" } else { "Learn (Assign): play a note" });
        }
        if live {
            flags.push("Live Record");
        }
        if mono {
            flags.push("Mono");
        }
        if !flags.is_empty() {
            ui.label(egui::RichText::new(flags.join("  ·  ")).size(12.0).color(Color32::from_rgb(0xe8, 0xc3, 0x2e)));
        }
    });
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
    let compact = header_w < 360.0;
    let tiny = header_w < 320.0;
    let mut actions: Vec<Action> = Vec::new();
    {
        let hr = Rect::from_min_max(ph.min, pos2(head_x + header_w, ph.max.y));
        p.rect_filled(hr, 0.0, look::header_bg());
        let mut x = hr.min.x + 12.0;
        let cy = hr.center().y;
        // + ⌄: a row for a pitch
        let add = Rect::from_min_size(pos2(x, cy - 13.0), vec2(42.0, 26.0));
        p.rect_filled(add, 6.0, look::group_bg());
        ui.scope_builder(egui::UiBuilder::new().max_rect(add), |ui| {
            plain(ui);
            crate::glyphs::paint(ui.painter(), Rect::from_center_size(pos2(add.max.x - 12.0, add.center().y + 1.0), vec2(12.0, 12.0)), crate::glyphs::ph::CARET_DOWN, 10.0, look::text());
            ui.menu_button(egui::RichText::new("+     ").size(17.0).color(look::text()), |ui| {
                ui.set_min_width(140.0);
                let used: Vec<u8> = pat.rows.iter().map(|r| r.pitch).collect();
                let list: Vec<u8> = if drums { (35u8..=59).collect() } else { (36u8..=84).rev().collect() };
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
        });
        // (the pattern's controls line up with the rows' below)
        x = hr.min.x + 120.0;
        rate_box(ui, Rect::from_min_size(pos2(x, cy - 13.0), vec2(60.0, 26.0)), ("pat-rate", 0), pat.rate, &mut |v| actions.push(Action::PatternRate(v)));
        x += 62.0;
        if !compact {
            play_box(ui, Rect::from_min_size(pos2(x, cy - 13.0), vec2(40.0, 26.0)), ("pat-play", 0), pat.play, &mut |m| actions.push(Action::Play(None, m)));
            x += 44.0;
        }
        if !tiny {
        let rot = Rect::from_min_size(pos2(x, cy - 13.0), vec2(40.0, 26.0));
        p.rect_filled(rot, 6.0, look::group_bg());
        for (k, (s, d)) in [(Glyph::RotLeft, -1i32), (Glyph::RotRight, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(rot.min.x + k as f32 * 20.0, rot.min.y), vec2(20.0, 26.0));
            glyph(&p, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("pat-rot", k)), Sense::click()).on_hover_text(if *d < 0 { "Rotate Pattern Left" } else { "Rotate Pattern Right" }).clicked() {
                actions.push(Action::Rotate(None, *d));
            }
        }
        x += 44.0;
        }
        let tr = Rect::from_min_size(pos2(x, cy - 13.0), vec2(40.0, 26.0));
        p.rect_filled(tr, 6.0, look::group_bg());
        for (k, (s, d)) in [(Glyph::Down, -1i32), (Glyph::Up, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(tr.min.x + k as f32 * 20.0, tr.min.y), vec2(20.0, 26.0));
            glyph(&p, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("pat-tr", k)), Sense::click()).on_hover_text(if *d < 0 { "Transpose -1 Semitone" } else { "Transpose +1 Semitone" }).clicked() {
                actions.push(Action::Transpose(None, *d));
            }
        }
        x += 44.0;
        if small_button(ui, &p, Rect::from_min_size(pos2(x, cy - 13.0), vec2(22.0, 26.0)), Glyph::Dice, ("pat-rand", 0), "Randomize Current Edit Mode for Pattern…") {
            actions.push(Action::Randomize(None));
        }
        x += 26.0;
        if small_button(ui, &p, Rect::from_min_size(pos2(x, cy - 13.0), vec2(22.0, 26.0)), Glyph::Clear, ("pat-clear", 0), "Clear Pattern") {
            actions.push(Action::Clear(None));
        }
        // right: key, Q, steps
        let right = Rect::from_min_max(pos2(hr.max.x, ph.min.y), ph.max);
        p.rect_filled(right, 0.0, look::header_bg());
        let key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_else(|| "C major".into());
        let mut kx = right.max.x - 12.0;
        let pages_w = if pat.steps > 16 { (pat.steps as usize).div_ceil(16) as f32 * 18.0 + 24.0 } else { 0.0 };
        let left_edge = right.min.x + 10.0 + pages_w;
        let steps_r = Rect::from_min_max(pos2((kx - 130.0).max(left_edge), cy - 14.0), pos2(kx.max(left_edge + 100.0), cy + 14.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(steps_r), |ui| {
            plain(ui);
            ui.painter().rect_filled(steps_r, 7.0, look::group_bg());
            glyph(ui.painter(), Rect::from_min_max(pos2(steps_r.max.x - 22.0, steps_r.min.y), steps_r.max), Glyph::UpDown, look::text());
            ui.menu_button(egui::RichText::new(format!("{} Steps    ", pat.steps)).size(15.0).strong().color(look::text()), |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for n in 1..=MAX_STEPS as u8 {
                        if ui.button(format!("{n} Steps")).clicked() {
                            actions.push(Action::Length(None, n));
                            ui.close();
                        }
                    }
                });
            });
        });
        kx = steps_r.min.x - 10.0;
        let q = Rect::from_min_max(pos2(kx - 34.0, cy - 14.0), pos2(kx, cy + 14.0));
        if q.min.x > left_edge {
            p.rect_filled(q, 7.0, look::group_bg());
            p.text(q.center(), Align2::CENTER_CENTER, "Q", look::regular(13.0), look::text_dim());
        }
        kx = q.min.x - 10.0;
        let kr = Rect::from_min_max(pos2((kx - 200.0).max(left_edge), cy - 14.0), pos2(kx, cy + 14.0));
        if kr.width() > 150.0 {
            p.rect_filled(kr, 7.0, look::group_bg());
            p.text(kr.center(), Align2::CENTER_CENTER, scale_label(&key), look::semibold(14.0), look::text());
        }
        // pages (patterns longer than 16 steps)
        let pages = (pat.steps as usize).div_ceil(16);
        if pages > 1 {
            let mut page = with_store(&ctx, |s| s.page).min(pages - 1);
            for k in 0..pages {
                let c = pos2(right.min.x + 24.0 + k as f32 * 18.0, cy);
                let rr = Rect::from_center_size(c, vec2(16.0, 16.0));
                p.circle_filled(c, 4.5, if k == page { look::text() } else { look::text_dim().gamma_multiply(0.5) });
                if ui.interact(rr, ui.scope_id().with("lg-ss").with(("page", k)), Sense::click()).on_hover_text(format!("Steps {}–{}", k * 16 + 1, ((k + 1) * 16).min(pat.steps as usize))).clicked() {
                    page = k;
                }
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
    // scroll the rows with the wheel
    let scroll_id = egui::Id::unique(("lg-stepseq-scroll", t, r));
    let mut scroll: f32 = ctx.data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| body.contains(h)) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        let total: f32 = pat.rows.iter().map(row_h).sum();
        let max = (total - body.height() + 10.0).max(0.0);
        scroll = (scroll - dy).clamp(0.0, max);
    }
    ctx.data_mut(|d| d.insert_temp(scroll_id, scroll));
    let hp = ui.painter_at(headers);
    let gp = ui.painter_at(grid);
    let visible = (pat.steps as usize - page * 16).min(16);
    let gap_s = 3.0;
    let gap_g = 7.0;
    let gx0 = grid.min.x + 18.0;
    let gw = (grid.max.x - 16.0 - gx0).max(100.0);
    let cell_w = ((gw - gap_g * 3.0 - gap_s * 12.0) / 16.0).max(8.0);
    let cell_x = |k: usize| gx0 + k as f32 * (cell_w + gap_s) + (k / 4) as f32 * (gap_g - gap_s);
    // the playing step
    let playing_tick = app.position() as i64 - region.start as i64;
    let in_region = playing_tick >= 0 && (playing_tick as u32) < region.length && app.engine.lock().map(|e| e.playing).unwrap_or(false);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let (down, released) = ui.input(|i| (i.pointer.primary_down(), i.pointer.any_released()));
    let mut press = with_store(&ctx, |s| s.press);
    let mut pressed_cells = with_store(&ctx, |s| s.pressed_cells.clone());
    let mut changed = false;
    let (sel, by_region, mono) = with_store(&ctx, |s| (s.sel, s.by_region, s.mono));
    let mut sel_click: Option<(usize, usize)> = None;
    let region_col = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(cell_on(), |tr| look::region_color(tr.kind));
    let mut ys = Vec::with_capacity(pat.rows.len());
    let mut acc = rows_top - scroll;
    for row in &pat.rows {
        ys.push(acc);
        acc += row_h(row);
    }
    for (ri, row) in pat.rows.clone().iter().enumerate() {
        let y = ys[ri];
        if y + row_h(row) < body.min.y || y > body.max.y {
            continue;
        }
        let hr = Rect::from_min_size(pos2(headers.min.x, y), vec2(header_w, ROW_H));
        let mode = with_store(&ctx, |s| s.modes.get(&(t, r, ri)).copied().unwrap_or_default());
        // header
        if ri == sel_row {
            hp.rect_filled(hr, 0.0, Color32::from_gray(0x40));
        }
        hp.hline(hr.x_range(), hr.max.y - 0.5, Stroke::new(1.0, Color32::from_gray(0x1a)));
        let sel_resp = ui.interact(hr, ui.scope_id().with("lg-ss").with(("rowhdr", ri)), Sense::click());
        if sel_resp.clicked() || sel_resp.secondary_clicked() {
            sel_row = ri;
        }
        if sel_resp.double_clicked() {
            with_store(&ctx, |s| s.typing = Some((Typing::Rename(ri), row.name.clone())));
        }
        crate::menus::context_menu(app, &sel_resp, "stepseq-row-header");
        hp.text(pos2(hr.min.x + 12.0, hr.center().y), Align2::CENTER_CENTER, "›", look::regular(18.0), look::text_dim());
        let ic = pos2(hr.min.x + 42.0, hr.center().y);
        hp.circle_filled(ic, 14.0, row_color());
        hp.text(ic, Align2::CENTER_CENTER, "♩", look::regular(15.0), Color32::from_gray(0x16));
        let name_pos = pos2(hr.min.x + 66.0, hr.min.y + 13.0);
        let name_r = hp.text(name_pos, Align2::LEFT_CENTER, &row.name, look::semibold(14.0), look::text());
        // ⌃⌄: the row's note
        let ar = Rect::from_min_size(pos2(name_r.max.x + 4.0, name_r.min.y - 2.0), vec2(14.0, 20.0));
        glyph(&hp, ar, Glyph::UpDown, look::text_dim());
        let ar_resp = ui.interact(ar, ui.scope_id().with("lg-ss").with(("rownote", ri)), Sense::click());
        if ar_resp.clicked() {
            let up = ar_resp.interact_pointer_pos().is_some_and(|q| q.y < ar.center().y);
            actions.push(Action::RowPitch(ri, if up { 1 } else { -1 }));
        }
        // the row's edit mode
        let mr = Rect::from_min_max(pos2(hr.max.x - 130.0, hr.min.y + 3.0), pos2(hr.max.x - 10.0, hr.min.y + 23.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(mr).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            plain(ui);
            ui.menu_button(egui::RichText::new(mode.name()).size(12.5).color(look::text()), |ui| {
                for m in Mode::ALL {
                    if ui.selectable_label(m == mode, m.name()).clicked() {
                        actions.push(Action::Mode(ri, m));
                        ui.close();
                    }
                }
            });
        });
        // second line: M S, rate, playback, rotate, octave, random, clear
        let ly = hr.min.y + 34.0;
        let mut x = hr.min.x + 62.0;
        let ms = Rect::from_min_size(pos2(x, ly - 11.0), vec2(54.0, 22.0));
        hp.rect_filled(ms, 5.0, Color32::from_gray(0x4f));
        for (k, (lbl, on)) in [("M", row.mute), ("S", row.solo)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(ms.min.x + k as f32 * 27.0, ms.min.y), vec2(27.0, 22.0));
            if *on {
                hp.rect_filled(rr.shrink(1.0), 4.0, if k == 0 { Color32::from_rgb(0x2e, 0x8b, 0xe8) } else { Color32::from_rgb(0xe8, 0xc3, 0x2e) });
            }
            hp.text(rr.center(), Align2::CENTER_CENTER, *lbl, look::semibold(13.0), if *on { Color32::BLACK } else { look::text() });
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("ms", ri, k)), Sense::click()).on_hover_text(if k == 0 { "Mute Row" } else { "Solo Row" }).clicked() {
                actions.push(if k == 0 { Action::Mute(ri) } else { Action::Solo(ri) });
            }
        }
        hp.vline(ms.center().x, ms.y_range().shrink(5.0), Stroke::new(1.0, Color32::from_gray(0x3a)));
        x += 58.0;
        let rate_r = Rect::from_min_size(pos2(x, ly - 11.0), vec2(60.0, 22.0));
        let rate = row.rate.unwrap_or(pat.rate);
        rate_box(ui, rate_r, ("row-rate", ri), rate, &mut |v| actions.push(Action::RowRate(ri, v)));
        x += 62.0;
        if !compact {
            play_box(ui, Rect::from_min_size(pos2(x, ly - 11.0), vec2(40.0, 22.0)), ("row-play", ri), row.play.unwrap_or(pat.play), &mut |m| actions.push(Action::Play(Some(ri), m)));
            x += 44.0;
        }
        if !tiny {
        let rot = Rect::from_min_size(pos2(x, ly - 11.0), vec2(40.0, 22.0));
        hp.rect_filled(rot, 5.0, look::group_bg());
        for (k, (s, d)) in [(Glyph::RotLeft, -1i32), (Glyph::RotRight, 1)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(rot.min.x + k as f32 * 20.0, rot.min.y), vec2(20.0, 22.0));
            glyph(&hp, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("rot", ri, k)), Sense::click()).on_hover_text(if *d < 0 { "Rotate Row Left" } else { "Rotate Row Right" }).clicked() {
                actions.push(Action::Rotate(Some(ri), *d));
            }
        }
        x += 44.0;
        }
        let oc = Rect::from_min_size(pos2(x, ly - 11.0), vec2(40.0, 22.0));
        hp.rect_filled(oc, 5.0, look::group_bg());
        for (k, (s, d)) in [(Glyph::Down, -12i32), (Glyph::Up, 12)].iter().enumerate() {
            let rr = Rect::from_min_size(pos2(oc.min.x + k as f32 * 20.0, oc.min.y), vec2(20.0, 22.0));
            glyph(&hp, rr, *s, look::text());
            if ui.interact(rr, ui.scope_id().with("lg-ss").with(("oct", ri, k)), Sense::click()).on_hover_text(if *d < 0 { "Row Octave Down" } else { "Row Octave Up" }).clicked() {
                actions.push(Action::Transpose(Some(ri), *d));
            }
        }
        x += 44.0;
        if small_button(ui, &hp, Rect::from_min_size(pos2(x, ly - 11.0), vec2(22.0, 22.0)), Glyph::Dice, ("rand", ri), "Randomize Current Edit Mode for Selected Rows…") {
            actions.push(Action::Randomize(Some(ri)));
        }
        x += 26.0;
        if small_button(ui, &hp, Rect::from_min_size(pos2(x, ly - 11.0), vec2(22.0, 22.0)), Glyph::Clear, ("clr", ri), "Clear Row") {
            actions.push(Action::Clear(Some(ri)));
        }
        // the row's steps
        let n = pat.row_len(row);
        let gy = Rect::from_min_size(pos2(grid.min.x, y), vec2(grid.width(), ROW_H));
        let muted = row.mute || (pat.rows.iter().any(|x| x.solo) && !row.solo);
        for k in 0..visible {
            let si = page * 16 + k;
            let cr = Rect::from_min_size(pos2(cell_x(k), y + 5.0), vec2(cell_w, ROW_H - 10.0));
            if !cr.intersects(grid) {
                continue;
            }
            let s = row.steps[si];
            let beyond = si >= n || si < row.start as usize;
            let mut bg = cell_off();
            if beyond {
                bg = Color32::from_rgb(0x1a, 0x18, 0x22);
            }
            gp.rect_filled(cr, 3.0, bg);
            let base_on = if by_region { region_col } else { cell_on() };
            let on_col = if muted { look::mix(base_on, cell_off(), 0.55) } else { base_on };
            match mode {
                Mode::OnOff | Mode::Loop => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, on_col);
                    }
                }
                Mode::Tie => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, cell_off(), 0.5));
                    }
                    if s.tie {
                        gp.rect_filled(Rect::from_min_max(pos2(cr.min.x - gap_g, cr.center().y - 4.0), pos2(cr.max.x - 4.0, cr.center().y + 4.0)), 2.0, on_col);
                    }
                }
                Mode::Skip => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, cell_off(), 0.5));
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
                        gp.rect_filled(cr, 3.0, look::mix(on_col, cell_off(), 0.6));
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
                        gp.rect_filled(cr, 3.0, look::mix(on_col, cell_off(), 0.6));
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
            // values written in the cells of the selected row (Logic's default)
            if s.on && ri == sel_row && cr.width() > 26.0 {
                let txt = mode.text(&s);
                if !txt.is_empty() {
                    gp.text(cr.center(), Align2::CENTER_CENTER, txt, look::semibold(11.0), Color32::WHITE);
                }
            }
            if in_region {
                let rate = row.rate.unwrap_or(pat.rate).max(1);
                if (playing_tick as u32 / rate) as usize % n == si {
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
                if (a.0.min(b.0)..=a.0.max(b.0)).contains(&ri) && (a.1.min(b.1)..=a.1.max(b.1)).contains(&si) {
                    gp.rect_stroke(cr.expand(1.5), 4.0, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc3, 0x2e)), egui::StrokeKind::Outside);
                }
            }
            if resp.is_pointer_button_down_on() && press.is_none() && down {
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
            }
            if let (Some((pr, paint)), Some(pp)) = (press, pointer) {
                let hot = Rect::from_min_max(pos2(cr.min.x - gap_s, gy.min.y), pos2(cr.max.x + gap_s, gy.max.y));
                if pr == ri && down && hot.contains(pos2(pp.x, (pp.y).clamp(gy.min.y + 1.0, gy.max.y - 1.0))) && hot.x_range().contains(pp.x) {
                    let f = (cr.max.y - pp.y) / cr.height();
                    let first_touch = !pressed_cells.contains(&(ri, si));
                    let st = &mut pat.rows[ri].steps[si];
                    let old = *st;
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
            gp.vline(x, y + 3.0..=y + ROW_H - 3.0, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc3, 0x2e)));
        }
        let st0 = row.start as usize;
        if st0 > 0 && st0 >= page * 16 && st0 < page * 16 + visible {
            let x = cell_x(st0 - page * 16) - gap_s / 2.0 - 1.0;
            gp.vline(x, y + 3.0..=y + ROW_H - 3.0, Stroke::new(2.0, Color32::from_rgb(0x2e, 0xc3, 0xe8)));
        }
        // subrows: velocity, gate and chance lanes, drawn and edited under the row
        if row.sub {
            for (li, m) in SUB_MODES.iter().enumerate() {
                let ly0 = y + ROW_H + li as f32 * SUB_H;
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
                        gp.rect_filled(Rect::from_min_max(pos2(cr.min.x, cr.max.y - cr.height() * f), cr.max), 2.0, look::mix(on_col_of(by_region, region_col), cell_off(), 0.25));
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
        press = None;
        pressed_cells.clear();
    }
    let finishing = with_store(&ctx, |s| {
        if let Some(c) = sel_click {
            s.sel = Some((c, c));
        }
        s.press = press;
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
    // Learn and Live Record listen to the keyboard
    let step = in_region.then(|| (playing_tick as u32 / pat.rate.max(1)) as usize % (pat.steps.max(1) as usize));
    listen(app, &ctx, t, r, step);
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

fn row_h(row: &Row) -> f32 {
    ROW_H + if row.sub { SUB_H * SUB_MODES.len() as f32 } else { 0.0 }
}

fn on_col_of(by_region: bool, region: Color32) -> Color32 {
    if by_region { region } else { cell_on() }
}

/// The playback mode pop-up (the arrow box beside the rate).
fn play_box(ui: &mut egui::Ui, r: Rect, id: (&str, usize), cur: Play, set: &mut dyn FnMut(Play)) {
    ui.painter().rect_filled(r, 6.0, look::group_bg());
    glyph(ui.painter(), Rect::from_min_max(pos2(r.max.x - 16.0, r.min.y), r.max), Glyph::UpDown, look::text_dim());
    ui.scope_builder(egui::UiBuilder::new().max_rect(r).id_salt(id), |ui| {
        plain(ui);
        ui.menu_button(egui::RichText::new(format!("{}   ", cur.arrow())).size(14.0).color(look::text()), |ui| {
            for m in Play::ALL {
                if ui.selectable_label(m == cur, m.name()).clicked() {
                    set(m);
                    ui.close();
                }
            }
        });
    })
    .response
    .on_hover_text(format!("Playback Mode: {}", cur.name()));
}

/// Header and toolbar actions, applied once the frame's drawing is done.
enum Action {
    AddRow(String, u8),
    PatternRate(u32),
    RowRate(usize, u32),
    Length(Option<usize>, u8),
    Rotate(Option<usize>, i32),
    Transpose(Option<usize>, i32),
    Randomize(Option<usize>),
    Clear(Option<usize>),
    Mute(usize),
    Solo(usize),
    RowPitch(usize, i32),
    Mode(usize, Mode),
    Play(Option<usize>, Play),
}

fn apply(pat: &mut StepPattern, a: Action, ctx: &egui::Context, t: usize, r: usize) {
    let all: Vec<usize> = (0..pat.rows.len()).collect();
    let rows = |sel: Option<usize>| sel.map(|x| vec![x]).unwrap_or_else(|| all.clone());
    match a {
        Action::AddRow(name, pitch) => {
            pat.rows.push(Row::new(name, pitch));
            pat.rows.sort_by(|a, b| b.pitch.cmp(&a.pitch));
        }
        Action::PatternRate(v) => pat.rate = v,
        Action::RowRate(k, v) => {
            if let Some(row) = pat.rows.get_mut(k) {
                row.rate = if v == pat.rate { None } else { Some(v) };
            }
        }
        Action::Length(None, n) => pat.steps = n.clamp(1, MAX_STEPS as u8),
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
        Action::Transpose(sel, d) => {
            for k in rows(sel) {
                let row = &mut pat.rows[k];
                row.pitch = (row.pitch as i32 + d).clamp(0, 127) as u8;
                if drum_name(row.pitch).is_none() || !row.name.chars().next().is_some_and(|c| c.is_alphabetic() && c.is_lowercase()) {
                    let looks_like_note = row.name.chars().next().is_some_and(|c| ('A'..='G').contains(&c)) && row.name.chars().any(|c| c.is_ascii_digit());
                    if looks_like_note {
                        row.name = crate::app::note_name(row.pitch);
                    }
                }
            }
        }
        Action::Randomize(sel) => {
            let seed = ctx.input(|i| i.time.to_bits());
            for k in rows(sel) {
                let mode = ctx.data(|d| d.get_temp::<Store>(store_id())).and_then(|s| s.modes.get(&(t, r, k)).copied()).unwrap_or_default();
                let n = pat.row_len(&pat.rows[k]);
                for (j, s) in pat.rows[k].steps[..n].iter_mut().enumerate() {
                    let v = chance(seed, k, j as u32) as f32 / 99.0;
                    match mode {
                        Mode::OnOff | Mode::Loop => s.on = v < 0.35,
                        Mode::Tie => s.tie = s.on && v < 0.2,
                        Mode::Skip => s.skip = v < 0.15,
                        m => {
                            if s.on {
                                m.set(s, v);
                            }
                        }
                    }
                }
            }
        }
        Action::Clear(sel) => {
            for k in rows(sel) {
                pat.rows[k].steps = vec![Step::default(); MAX_STEPS];
            }
        }
        Action::Mute(k) => pat.rows[k].mute = !pat.rows[k].mute,
        Action::Solo(k) => pat.rows[k].solo = !pat.rows[k].solo,
        Action::RowPitch(k, d) => {
            let row = &mut pat.rows[k];
            row.pitch = (row.pitch as i32 + d).clamp(0, 127) as u8;
            row.name = drum_name(row.pitch).filter(|_| row.name.chars().next().is_some_and(|c| !('A'..='G').contains(&c) || row.name.contains(' '))).map(str::to_string).unwrap_or_else(|| crate::app::note_name(row.pitch));
        }
        Action::Play(None, m) => pat.play = m,
        Action::Play(Some(k), m) => {
            let base = pat.play;
            if let Some(row) = pat.rows.get_mut(k) {
                row.play = if m == base { None } else { Some(m) };
            }
        }
        Action::Mode(k, m) => with_store(ctx, |s| {
            s.modes.insert((t, r, k), m);
        }),
    }
}

fn plain(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
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
    use crate::glyphs::ph;
    let (icon, size) = match g {
        Glyph::Up => (ph::CARET_UP, 12.0),
        Glyph::Down => (ph::CARET_DOWN, 12.0),
        Glyph::UpDown => (ph::CARET_UP_DOWN, 12.0),
        Glyph::RotLeft => (ph::ARROW_ARC_LEFT, 15.0),
        Glyph::RotRight => (ph::ARROW_ARC_RIGHT, 15.0),
        Glyph::Dice => (ph::DICE_FIVE, 15.0),
        Glyph::Clear => (ph::X_SQUARE, 15.0),
    };
    crate::glyphs::paint(p, r, icon, size, col);
}

fn small_button(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, g: Glyph, id: (&str, usize), tip: &str) -> bool {
    let resp = ui.interact(r, ui.scope_id().with("lg-ss").with(id), Sense::click()).on_hover_text(tip);
    p.rect_filled(r, 5.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    glyph(p, r, g, look::text());
    resp.clicked()
}


/// "/16 ⌃⌄": a step rate pop-up.
fn rate_box(ui: &mut egui::Ui, r: Rect, id: (&str, usize), rate: u32, set: &mut dyn FnMut(u32)) {
    ui.painter().rect_filled(r, 6.0, Color32::from_gray(0x4f));
    glyph(ui.painter(), Rect::from_min_max(pos2(r.max.x - 18.0, r.min.y), r.max), Glyph::UpDown, look::text_dim());
    ui.scope_builder(egui::UiBuilder::new().max_rect(r).id_salt(id), |ui| {
        plain(ui);
        ui.menu_button(egui::RichText::new(format!("{}    ", rate_name(rate))).size(13.0).color(look::text()), |ui| {
            for (name, v) in RATES {
                if ui.selectable_label(v == rate, name).clicked() {
                    set(v);
                    ui.close();
                }
            }
        });
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
    "Live Record", "Mono Mode", "Rename Row", "Select Next Pattern Page", "Save Pattern", "Save Template",
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
const RATE_ORDER: [u32; 12] = [3840, 1920, 1280, 960, 640, 480, 320, 240, 160, 120, 80, 60];

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

fn randomize(mode: Mode, st: &mut Step, v: f32) {
    match mode {
        Mode::OnOff | Mode::Loop => st.on = v < 0.35,
        Mode::Tie => st.tie = st.on && v < 0.2,
        Mode::Skip => st.skip = v < 0.15,
        m => {
            if st.on {
                m.set(st, v);
            }
        }
    }
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
        let k = RATE_ORDER.iter().position(|&x| x == cur).unwrap_or(7) as i32;
        RATE_ORDER[(k + d).clamp(0, RATE_ORDER.len() as i32 - 1) as usize]
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
            with_store(ctx, |s| s.sort = (by, asc));
            change(app, ctx, "Sort Rows", |p, _, _, _| crate::stepinsp::sort_rows(p, by, asc))
        }
        "Show/Hide Pattern Browser" | "Pattern Browser" => view(app, ctx, |s, _, _, _| s.browser = !s.browser),
        "Set Step Color by Region Color" | "By Region" => view(app, ctx, |s, _, _, _| s.by_region = true),
        "Set Step Color by Row Color" | "By Row" => view(app, ctx, |s, _, _, _| s.by_region = false),
        "Live Record" => view(app, ctx, |s, _, _, _| s.live_record = !s.live_record),
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
        _ if name.starts_with("Randomize Current Edit Mode") => change(app, ctx, "Randomize", |p, s, t, r| {
            if name.ends_with("Selected Steps") {
                for (k, (rr, st)) in sel_cells(s, p.rows.len()).into_iter().enumerate() {
                    let m = mode_of(s, t, r, rr);
                    randomize(m, &mut p.rows[rr].steps[st], chance(seed, rr, k as u32) as f32 / 99.0);
                }
                return;
            }
            let rows = if name.contains("Selected Rows") { sel_rows(s, p.rows.len()) } else { (0..p.rows.len()).collect() };
            for k in rows {
                let m = mode_of(s, t, r, k);
                let n = p.row_len(&p.rows[k]);
                for (j, st) in p.rows[k].steps[..n].iter_mut().enumerate() {
                    randomize(m, st, chance(seed, k, j as u32) as f32 / 99.0);
                }
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
                p.rows[k].steps.iter_mut().filter(|x| x.on).for_each(|st| nudge_value(m, st, d));
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
fn listen(app: &mut App, ctx: &egui::Context, t: usize, _r: usize, playing_step: Option<usize>) {
    let held: Vec<(u8, u8)> = app.live.held.iter().map(|(&p, &(_, v))| (p, v)).collect();
    let (fresh, learn, live) = with_store(ctx, |s| {
        let fresh: Vec<(u8, u8)> = held.iter().copied().filter(|(p, _)| !s.held.contains(p)).collect();
        s.held = held.iter().map(|x| x.0).collect();
        (fresh, s.learn, s.live_record)
    });
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
                    p.rows.push(Row::new(name, pitch));
                    p.rows.sort_by(|a, b| b.pitch.cmp(&a.pitch));
                }
            } else if let Some(row) = p.rows.get_mut(s.selected_row) {
                row.pitch = pitch;
                row.name = name;
            }
            s.learn = None;
        });
        return;
    }
    if let (true, Some(step)) = (live, playing_step) {
        change(app, ctx, "Live Record", |p, _, _, _| {
            for (pitch, vel) in fresh {
                if !p.rows.iter().any(|x| x.pitch == pitch) {
                    p.rows.push(Row::new(crate::app::note_name(pitch), pitch));
                    p.rows.sort_by(|a, b| b.pitch.cmp(&a.pitch));
                }
                if let Some(row) = p.rows.iter_mut().find(|x| x.pitch == pitch) {
                    row.steps[step] = Step { on: true, velocity: vel.max(1), ..Step::default() };
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
        // ping pong turns at the ends without playing them twice; pendulum plays them twice
        assert_eq!(play_order(Play::PingPong, 4, 8, 0, 0), [0, 1, 2, 3, 2, 1, 0, 1]);
        assert_eq!(play_order(Play::Pendulum, 4, 10, 0, 0), [0, 1, 2, 3, 3, 2, 1, 0, 0, 1]);
        assert_eq!(play_order(Play::PingPong, 1, 3, 0, 0), [0, 0, 0]);
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
}
