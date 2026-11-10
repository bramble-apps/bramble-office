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
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, velocity: 100, gate: 100, repeat: 1, chance: 100, offset: 0, tie: false, octave: 0, skip: false }
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
}

impl Row {
    pub fn new(name: impl Into<String>, pitch: u8) -> Row {
        Row { name: name.into(), pitch, mute: false, solo: false, len: None, rate: None, steps: vec![Step::default(); MAX_STEPS] }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepPattern {
    /// Pattern length in steps (1–64).
    pub steps: u8,
    /// Ticks per step (240 = 1/16).
    pub rate: u32,
    pub rows: Vec<Row>,
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
        StepPattern { steps: 16, rate: SIXTEENTH, rows }
    }

    /// A new pattern for drums: Logic's kit rows.
    pub fn drums() -> StepPattern {
        let rows = [36u8, 38, 37, 39, 42, 44, 46, 45, 47, 50, 49, 51].iter().map(|&p| Row::new(drum_name(p).unwrap_or_default(), p)).collect();
        StepPattern { steps: 16, rate: SIXTEENTH, rows }
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
            if row.steps[..n].iter().all(|s| s.skip) {
                continue;
            }
            let mut idx = 0usize;
            let mut slot = 0u32;
            // the note a tie carries on, as an index into `out`
            let mut last: Option<usize> = None;
            while slot * rate < len {
                while row.steps[idx % n].skip {
                    idx += 1;
                }
                let s = row.steps[idx % n];
                let t = slot * rate;
                if s.on && s.tie && last.is_some() {
                    if let Some(prev) = last.and_then(|k| out.get_mut(k)) {
                        let prev: &mut Note = prev;
                        let end = (t + rate * s.gate.clamp(1, 100) as u32 / 100).min(len);
                        prev.len = end.saturating_sub(prev.pos).max(prev.len);
                    }
                } else if s.on && chance(seed, ri, slot) < s.chance {
                    let reps = s.repeat.clamp(1, 16) as u32;
                    let sub = rate / reps;
                    let off = (rate as i64 * s.offset.clamp(-50, 50) as i64 / 100) as i64;
                    let pitch = (row.pitch as i32 + 12 * s.octave as i32).clamp(0, 127) as u8;
                    for k in 0..reps {
                        let pos = (t as i64 + k as i64 * sub as i64 + off).max(0) as u32;
                        if pos >= len {
                            break;
                        }
                        let l = (sub * s.gate.clamp(1, 100) as u32 / 100).max(1).min(len - pos);
                        out.push(Note { channel: 0, pitch, velocity: s.velocity.clamp(1, 127), pos, len: l });
                    }
                    last = Some(out.len() - 1);
                } else {
                    last = None;
                }
                idx += 1;
                slot += 1;
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
            StepPattern { steps: 16, rate: SIXTEENTH, rows: pitches.iter().map(|&p| Row::new(name(p), p)).collect() }
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
}

impl Mode {
    pub const ALL: [Mode; 10] = [Mode::OnOff, Mode::Velocity, Mode::Gate, Mode::Tie, Mode::Octave, Mode::Loop, Mode::Repeat, Mode::Chance, Mode::Offset, Mode::Skip];
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
        }
    }
    /// The value of a step in this mode, 0..1 (for the bar drawn in the cell).
    fn get(self, s: &Step) -> f32 {
        match self {
            Mode::Velocity => s.velocity as f32 / 127.0,
            Mode::Gate => s.gate as f32 / 100.0,
            Mode::Repeat => (s.repeat as f32 - 1.0) / 7.0,
            Mode::Chance => s.chance as f32 / 100.0,
            Mode::Offset => (s.offset as f32 + 50.0) / 100.0,
            Mode::Octave => (s.octave as f32 + 3.0) / 6.0,
            _ => s.on as u8 as f32,
        }
    }
    fn set(self, s: &mut Step, f: f32) {
        let f = f.clamp(0.0, 1.0);
        match self {
            Mode::Velocity => s.velocity = (f * 126.0 + 1.0).round() as u8,
            Mode::Gate => s.gate = (f * 99.0 + 1.0).round() as u8,
            Mode::Repeat => s.repeat = (f * 7.0 + 1.0).round() as u8,
            Mode::Chance => s.chance = (f * 100.0).round() as u8,
            Mode::Offset => s.offset = (f * 100.0 - 50.0).round() as i8,
            Mode::Octave => s.octave = (f * 6.0 - 3.0).round() as i8,
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
    tr.regions.push(lg_core::Region { name, start: at, length: bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default() });
    let r = tr.regions.len() - 1;
    with_store(ctx, |s| {
        s.patterns.insert((t, r), Entry { pattern: if drums { StepPattern::drums() } else { StepPattern::melodic() }, written: Vec::new() });
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
    });
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
            chevron(ui.painter(), pos2(add.max.x - 12.0, add.center().y + 1.0), 8.0, false, look::text());
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
            icon_box(&p, Rect::from_min_size(pos2(x, cy - 13.0), vec2(40.0, 26.0)), Glyph::Forward);
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
        let max = (pat.rows.len() as f32 * ROW_H - body.height() + 10.0).max(0.0);
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
    for (ri, row) in pat.rows.clone().iter().enumerate() {
        let y = rows_top + ri as f32 * ROW_H - scroll;
        if y + ROW_H < body.min.y || y > body.max.y {
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
        if sel_resp.clicked() {
            sel_row = ri;
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
            icon_box(&hp, Rect::from_min_size(pos2(x, ly - 11.0), vec2(40.0, 22.0)), Glyph::Forward);
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
            let beyond = si >= n;
            let mut bg = cell_off();
            if beyond {
                bg = Color32::from_rgb(0x1a, 0x18, 0x22);
            }
            gp.rect_filled(cr, 3.0, bg);
            let on_col = if muted { look::mix(cell_on(), cell_off(), 0.55) } else { cell_on() };
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
                Mode::Velocity | Mode::Chance | Mode::Octave => {
                    if s.on {
                        gp.rect_filled(cr, 3.0, look::mix(on_col, cell_off(), 0.6));
                        let f = mode.get(&s);
                        if mode == Mode::Octave {
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
            if resp.is_pointer_button_down_on() && press.is_none() && down {
                sel_row = ri;
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
                        Mode::OnOff => st.on = paint,
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
                            if st.on {
                                m.set(st, f);
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
        // the row's end (loop end)
        if n < pat.steps as usize && n >= page * 16 && n < page * 16 + visible {
            let x = cell_x(n - page * 16) - gap_s / 2.0 - 1.0;
            gp.vline(x, y + 3.0..=y + ROW_H - 3.0, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc3, 0x2e)));
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
    let _ = changed;
    if pat.rows.is_empty() {
        gp.text(grid.center(), Align2::CENTER_CENTER, "Add a row with +", look::regular(14.0), look::text_dim());
    }
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
    /// Playback direction "→" with its ⌃⌄.
    Forward,
}

fn chevron(p: &egui::Painter, c: egui::Pos2, w: f32, up: bool, col: Color32) {
    let h = w * 0.5 * if up { -1.0 } else { 1.0 };
    let st = Stroke::new(1.4, col);
    p.line_segment([pos2(c.x - w / 2.0, c.y - h / 2.0), pos2(c.x, c.y + h / 2.0)], st);
    p.line_segment([pos2(c.x, c.y + h / 2.0), pos2(c.x + w / 2.0, c.y - h / 2.0)], st);
}

fn glyph(p: &egui::Painter, r: Rect, g: Glyph, col: Color32) {
    let c = r.center();
    let st = Stroke::new(1.3, col);
    match g {
        Glyph::Up => chevron(p, c, 10.0, true, col),
        Glyph::Down => chevron(p, c, 10.0, false, col),
        Glyph::UpDown => {
            chevron(p, c - vec2(0.0, 3.5), 6.0, true, col);
            chevron(p, c + vec2(0.0, 3.5), 6.0, false, col);
        }
        Glyph::RotLeft | Glyph::RotRight => {
            let s = if g == Glyph::RotLeft { 1.0 } else { -1.0 };
            let b = Rect::from_center_size(c + vec2(2.5 * s, 0.0), vec2(6.0, 10.0));
            p.rect_stroke(b, 1.5, st, egui::StrokeKind::Middle);
            let tip = c + vec2(-5.0 * s, 0.0);
            p.add(egui::Shape::convex_polygon(vec![tip, tip + vec2(3.5 * s, -3.0), tip + vec2(3.5 * s, 3.0)], col, Stroke::NONE));
        }
        Glyph::Dice => {
            let b = Rect::from_center_size(c, vec2(13.0, 13.0));
            p.rect_stroke(b, 3.0, st, egui::StrokeKind::Middle);
            for (dx, dy) in [(-3.0, -3.0), (3.0, -3.0), (0.0, 0.0), (-3.0, 3.0), (3.0, 3.0)] {
                p.circle_filled(c + vec2(dx, dy), 1.2, col);
            }
        }
        Glyph::Clear => {
            let b = Rect::from_center_size(c, vec2(12.0, 12.0));
            p.rect_stroke(b, 2.0, Stroke::new(1.0, col), egui::StrokeKind::Middle);
            let q = b.shrink(3.0);
            p.line_segment([q.left_top(), q.right_bottom()], st);
            p.line_segment([q.left_bottom(), q.right_top()], st);
        }
        Glyph::Forward => {
            let a = pos2(r.min.x + 7.0, c.y);
            let b = pos2(r.max.x - 17.0, c.y);
            p.line_segment([a, b], Stroke::new(1.6, col));
            p.line_segment([b, b + vec2(-4.0, -4.0)], Stroke::new(1.6, col));
            p.line_segment([b, b + vec2(-4.0, 4.0)], Stroke::new(1.6, col));
            let u = pos2(r.max.x - 8.0, c.y);
            chevron(p, u - vec2(0.0, 3.5), 6.0, true, col);
            chevron(p, u + vec2(0.0, 3.5), 6.0, false, col);
        }
    }
}

fn icon_box(p: &egui::Painter, r: Rect, g: Glyph) {
    p.rect_filled(r, 6.0, look::group_bg());
    glyph(p, r, g, look::text());
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
    let mut pat = StepPattern { steps: end.saturating_sub(start).clamp(1, MAX_STEPS as u8), rate: SIXTEENTH, rows: Vec::new() };
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
        let mut p = StepPattern { steps: 16, rate: SIXTEENTH, rows: Vec::new() };
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
        let long = [Note { channel: 0, pitch: 60, velocity: 90, pos: 0, len: 720 }];
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
}
