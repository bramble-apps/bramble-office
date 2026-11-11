//! Live Loops (Logic Pro 13): the grid of cells beside the track headers, scenes as columns.
//! From the Logic research: screenshots/ui-live-loops-grid.png (sizes and colours, 2x),
//! interactions/UI-ANIMATIONS.txt (the cell trigger sequence: ▶, then ■ in a ring that fills
//! with the cell's position; the dashed ring of a queued cell), context-menus/liveloops-*.txt,
//! the "Live Loops Grid" key commands, and LOGICX-FORMAT.txt §9 (lg_core::liveloops reads and
//! writes the grid).
//!
//! A cell is a copy of some notes (or an audio file) with its own length; clicking it queues it
//! to start at the next Quantize Start boundary (1 bar by default); while it plays it loops and
//! takes over its track (the track's regions are silent and grey). The engine gets the cells'
//! notes through `notes_for` (and `update` feeds them in itself, each frame).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke};
use lg_core::events::Note;

use crate::app::{App, Drag};
use crate::look;

/// Sizes (points, from ui-live-loops-grid.png at 2x).
pub const CELL_W: f32 = 63.0;
const CELL_GAP: f32 = 4.0;
const PAD: f32 = 2.0;
/// The column of track stop buttons on the grid's right (with ◁▷ above, Stop All below).
const STOP_W: f32 = 25.0;
/// The scene trigger row at the grid's bottom.
const TRIGGER_H: f32 = 44.0;
const RING_R: f32 = 13.0;

/// What the Tracks area shows (Logic: L, ⌥L; DisplayState sectionVisibility 1 / 2 / 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    TracksOnly,
    GridOnly,
    Both,
}

/// When a cell (or scene) starts: Logic's Quantize Start menu.
pub const QUANTIZE_STARTS: [&str; 16] = ["Global", "Cell End", "8 Bars", "6 Bars", "4 Bars", "3 Bars", "2 Bars", "1 Bar", "1/2 Note", "1/2 Triplet (1/3)", "1/4 Note", "1/4 Triplet (1/6)", "1/8 Note", "1/8 Triplet (1/12)", "1/16 Note", "Off"];
pub const TRIGGER_MODES: [&str; 3] = ["Start/Stop", "Momentary", "Retrigger"];
pub const PLAY_FROMS: [&str; 4] = ["Start", "Stop Position", "Playing Cell Position", "Playhead Position"];
pub const SPEEDS: [(&str, f32); 7] = [("⅛", 0.125), ("¼", 0.25), ("½", 0.5), ("1", 1.0), ("2", 2.0), ("4", 4.0), ("8", 8.0)];

/// A cell's contents and its playback options.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub name: String,
    /// From the cell's start.
    pub notes: Vec<Note>,
    pub length: u32,
    pub audio: Option<PathBuf>,
    pub muted: bool,
    pub looped: bool,
    pub reversed: bool,
    pub speed: f32,
    pub transpose: i8,
    /// Note quantize (the Quantize menu), "Off" by default; applied as it plays.
    pub quantize: String,
    pub quantize_start: String,
    pub trigger: String,
    pub play_from: String,
    /// The cell it was read from (lg_core Cell.clip), for saving.
    pub origin: Option<u32>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { name: String::new(), notes: Vec::new(), length: 3840, audio: None, muted: false, looped: true, reversed: false, speed: 1.0, transpose: 0, quantize: "Off".into(), quantize_start: "Global".into(), trigger: "Start/Stop".into(), play_from: "Start".into(), origin: None }
    }
}

/// What a track's cells are doing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Play {
    /// Playing: scene, start tick (where loop 1 began), where it will stop.
    pub current: Option<(usize, u32, Option<u32>)>,
    /// Queued: scene, the boundary it starts at (None: when the transport starts).
    pub next: Option<(usize, Option<u32>)>,
    /// Where the ring stood when the transport stopped (drawn static).
    pub frac: f32,
}

/// Recording into a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rec {
    pub track: usize,
    pub scene: usize,
    pub start: u32,
    /// Stop at this boundary (then the cell plays).
    pub end: Option<u32>,
}

/// Each scene's trigger options.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    pub quantize_start: String,
    pub trigger: String,
}

impl Default for Scene {
    fn default() -> Self {
        Scene { name: String::new(), quantize_start: "Global".into(), trigger: "Retrigger".into() }
    }
}

/// The Live Loops state (one App field).
#[derive(Default)]
pub struct State {
    pub view: View,
    /// The project the grid was read for (its path), so a new project re-reads.
    pub loaded_for: Option<PathBuf>,
    /// As read: cells, the rows' channel objects, scene count (for saving).
    pub file: Option<lg_core::liveloops::Grid>,
    /// Cells by (track, scene).
    pub cells: BTreeMap<(usize, usize), Cell>,
    pub scenes: usize,
    pub scene_opts: BTreeMap<usize, Scene>,
    /// The grid's Quantize Start (Logic's default: 1 Bar).
    pub quantize: String,
    pub plays: BTreeMap<usize, Play>,
    pub selected: BTreeSet<(usize, usize)>,
    /// Queued with ⌥↩ / ⌥-click, waiting for a trigger (↩).
    pub pending: BTreeSet<(usize, usize)>,
    pub focused_scene: Option<usize>,
    pub focused: bool,
    pub rec: Option<Rec>,
    pub performance: bool,
    /// Performance recording: (track, scene, from, to) segments played while recording.
    pub perf_log: Vec<(usize, usize, u32, Option<u32>)>,
    pub wide: bool,
    pub colors_off: bool,
    pub grid_w: f32,
    pub scroll_x: f32,
    pub clipboard: Option<Cell>,
    pub renaming: Option<(Option<(usize, usize)>, usize, String)>,
    pub rec_mode: String,
    pub rec_length: String,
    pub rec_end: String,
    pub grid_rect: Option<Rect>,
    /// A cell being dragged (from), copy with ⌥.
    pub drag: Option<(usize, usize)>,
    /// A region dragged in from the tracks: (track, region, its start before the drag).
    pub region_drag: Option<(usize, usize, u32)>,
    /// What we wrote into the engine per track: (the track's own notes and audio, ours).
    fed: HashMap<usize, (Vec<(u32, u32, u8, u8)>, Vec<(u32, Arc<Vec<f32>>)>, Vec<(u32, u32, u8, u8)>)>,
    was_playing: bool,
    was_recording: bool,
    audio: Arc<Mutex<HashMap<PathBuf, Option<Arc<Vec<f32>>>>>>,
    loops: HashMap<(PathBuf, usize), Arc<Vec<f32>>>,
    momentary: Option<(usize, usize)>,
}

// ------------------------------------------------------------------------------------------------
// pure parts

/// A Quantize Start choice in ticks (None: Off; Some(0): Cell End, resolved by the caller).
pub fn quantize_ticks(label: &str, bar: u32) -> Option<u32> {
    let beat = 960u32;
    Some(match label {
        "Off" => return None,
        "Cell End" => 0,
        "8 Bars" => 8 * bar,
        "6 Bars" => 6 * bar,
        "4 Bars" => 4 * bar,
        "3 Bars" => 3 * bar,
        "2 Bars" => 2 * bar,
        "1/2 Note" => 2 * beat,
        "1/2 Triplet (1/3)" => 4 * beat / 3,
        "1/4 Note" => beat,
        "1/4 Triplet (1/6)" => 2 * beat / 3,
        "1/8 Note" => beat / 2,
        "1/8 Triplet (1/12)" => beat / 3,
        "1/16 Note" => beat / 4,
        _ => bar,
    })
}

/// The first boundary of `q` ticks at or after `pos` (Off: `pos` itself).
pub fn next_boundary(pos: f64, q: Option<u32>) -> u32 {
    match q {
        Some(q) if q > 0 => ((pos / q as f64 - 1e-6).ceil().max(0.0) as u32) * q,
        _ => pos.max(0.0) as u32,
    }
}

/// A note-quantize label as (grid ticks, swing %), None for Off.
pub fn note_grid(label: &str) -> Option<(u32, u32)> {
    let swing = |s: &str| match s.chars().last() {
        Some('A') => 54,
        Some('B') => 58,
        Some('C') => 62,
        Some('D') => 66,
        Some('E') => 71,
        _ => 75,
    };
    Some(match label {
        "1/1 Note" => (3840, 50),
        "1/2 Note" => (1920, 50),
        "1/4 Note" => (960, 50),
        "1/8 Note" => (480, 50),
        "1/16 Note" => (240, 50),
        "1/32 Note" => (120, 50),
        "1/64 Note" => (60, 50),
        "1/2 Triplet (1/3)" => (1280, 50),
        "1/4 Triplet (1/6)" => (640, 50),
        "1/8 Triplet (1/12)" => (320, 50),
        "1/16 Triplet (1/24)" => (160, 50),
        "1/32 Triplet (1/48)" => (80, 50),
        "1/64 Triplet (1/96)" => (40, 50),
        "1/128 Triplet (1/192)" => (20, 50),
        "5-Tuplet/4" => (768, 50),
        "5-Tuplet/8" => (384, 50),
        "7-Tuplet" => (549, 50),
        "9-Tuplet" => (427, 50),
        "1/16 & 1/16 Triplet" | "1/16 & 1/8 Triplet" => (160, 50),
        "1/8 & 1/8 Triplet" => (320, 50),
        l if l.starts_with("1/16 Swing") => (240, swing(l)),
        l if l.starts_with("1/8 Swing") => (480, swing(l)),
        _ => return None,
    })
}

/// A position quantized to a grid with swing (every second step moved later).
pub fn quantize_pos(pos: u32, grid: u32, swing: u32) -> u32 {
    let pair = grid * 2;
    let step = ((pos as f64 / grid as f64).round()) as u32;
    let base = (step / 2) * pair;
    if step % 2 == 1 { base + pair * swing / 100 } else { base }
}

/// A cell's notes as they sound: reversed, quantized, transposed (positions from its start).
pub fn sounding(c: &Cell) -> Vec<Note> {
    let g = note_grid(&c.quantize);
    c.notes
        .iter()
        .filter(|n| n.pos < c.length)
        .map(|n| {
            let mut n = *n;
            if c.reversed {
                n.pos = c.length.saturating_sub(n.pos + n.len.min(c.length - n.pos));
            }
            if let Some((grid, swing)) = g {
                n.pos = quantize_pos(n.pos, grid, swing).min(c.length.saturating_sub(1));
            }
            n.pitch = (n.pitch as i16 + c.transpose as i16).clamp(0, 127) as u8;
            n
        })
        .collect()
}

/// The notes of `cell` looping from `start` (song ticks), cut at `end`, that begin in
/// [from, to): (start, end, pitch, velocity) as the engine plays them.
pub fn loop_notes(c: &Cell, start: u32, end: Option<u32>, from: u32, to: u32) -> Vec<(u32, u32, u8, u8)> {
    let mut out = Vec::new();
    if c.muted || c.length == 0 || to <= from {
        return out;
    }
    let speed = if c.speed > 0.0 { c.speed } else { 1.0 };
    let len = ((c.length as f32 / speed) as u32).max(1);
    let to = end.map_or(to, |e| e.min(to));
    if to <= start {
        return out;
    }
    let notes = sounding(c);
    let first = if from > start { (from - start) / len } else { 0 };
    let last = (to - start).div_ceil(len);
    for k in first..=last {
        if !c.looped && k > 0 {
            break;
        }
        let base = start as u64 + k as u64 * len as u64;
        for n in &notes {
            let at = base + (n.pos as f32 / speed) as u64;
            if at < from as u64 || at >= to as u64 || at >= u32::MAX as u64 {
                continue;
            }
            let at = at as u32;
            let mut stop = at + ((n.len as f32 / speed) as u32).max(1);
            if let Some(e) = end {
                stop = stop.min(e.max(at + 1));
            }
            out.push((at, stop, n.pitch, n.velocity));
        }
    }
    out.sort_by_key(|n| (n.0, n.2));
    out
}

/// How far into its loop a cell is at `pos` (0..1).
pub fn progress(c: &Cell, start: u32, pos: f64) -> f32 {
    let speed = if c.speed > 0.0 { c.speed } else { 1.0 };
    let len = (c.length as f64 / speed as f64).max(1.0);
    let d = (pos - start as f64).max(0.0);
    ((d % len) / len) as f32
}

/// Notes (song ticks) that begin in [from, to) cut out as a cell: positions from `from`.
pub fn cell_from_notes(notes: &[Note], from: u32, to: u32, bar: u32) -> (Vec<Note>, u32) {
    let mut v: Vec<Note> = notes.iter().filter(|n| n.pos >= from && n.pos < to).map(|n| Note { pos: n.pos - from, ..*n }).collect();
    v.sort_by_key(|n| (n.pos, n.pitch));
    let len = (to - from).max(1).div_ceil(bar.max(1)) * bar.max(1);
    (v, len)
}

// ------------------------------------------------------------------------------------------------
// loading and saving

impl State {
    fn play(&mut self, t: usize) -> &mut Play {
        self.plays.entry(t).or_default()
    }

    pub fn used_scenes(&self) -> usize {
        self.cells.keys().map(|k| k.1 + 1).max().unwrap_or(0)
    }

    /// Whether the track has a cell playing (it takes over the track).
    pub fn active(&self, t: usize) -> bool {
        self.plays.get(&t).is_some_and(|p| p.current.is_some())
    }
}

/// Reads the grid when the project changes (from its ProjectData), and picks the view Logic
/// saved (DisplayState): a grid shows next to the tracks; no grid, tracks only.
pub fn sync(app: &mut App) {
    let Some(p) = app.project.as_ref() else { return };
    let path = p.path.clone();
    if app.live_loops.loaded_for.as_ref() == Some(&path) {
        return;
    }
    let sig = p.signature;
    let audio_dir = p.audio_dir.clone();
    let tracks: Vec<(u32, u32)> = p.tracks.iter().map(|t| (t.channel, t.row)).collect();
    let mut s = State { loaded_for: Some(path.clone()), quantize: "1 Bar".into(), rec_mode: "Merge".into(), rec_length: "Automatic (Bars)".into(), rec_end: "Play Cell".into(), grid_w: app.live_loops.grid_w, ..Default::default() };
    let pkg = lg_core::project::package_of(&path);
    let alt = lg_core::project::alternatives(&pkg).into_iter().next().map(|a| a.1);
    let grid = alt.as_ref().and_then(|a| std::fs::read(a.join("ProjectData")).ok()).and_then(|d| lg_core::liveloops::read(&d, sig));
    if let Some(g) = &grid {
        for c in &g.cells {
            // the row's track: by its channel object, else by row number
            let ch = g.row_channels.get(c.row as usize).copied().or(c.channel);
            let t = ch.and_then(|ch| tracks.iter().position(|t| t.0 == ch)).or_else(|| c.track_row.and_then(|r| tracks.iter().position(|t| t.1 == r))).unwrap_or(c.row as usize);
            let audio = c.audio.as_ref().map(|f| audio_dir.as_ref().map_or(PathBuf::from(f), |d| d.join(f)));
            let speed = if c.params.speed > 0.0 && c.params.speed.is_finite() { c.params.speed } else { 1.0 };
            s.cells.insert((t, c.scene as usize), Cell { name: c.name.clone(), notes: c.notes.clone(), length: c.length, audio, speed, origin: Some(c.clip), ..Default::default() });
        }
        s.scenes = (g.scenes as usize).max(s.used_scenes());
        let vis = alt.as_ref().and_then(|a| lg_core::liveloops::section_visibility(a));
        s.view = match vis {
            Some(1) => View::GridOnly,
            Some(3) => View::TracksOnly,
            _ => View::Both,
        };
    }
    s.file = grid;
    app.live_loops = s;
}

/// After Save As (the project's path changes): keep the grid as it is instead of re-reading.
pub fn keep_after_save_as(app: &mut App) {
    app.live_loops.loaded_for = app.project.as_ref().map(|p| p.path.clone());
}

/// The project's ProjectData with the grid's changes written in (`data`: what the main save
/// wrote, or the original). Cells made here (no Logic sequence of their own yet) can't be
/// written: their names come back.
pub fn save_into(app: &App, data: &[u8]) -> (Vec<u8>, Vec<String>) {
    use lg_core::liveloops::Edit;
    let s = &app.live_loops;
    let Some(g) = &s.file else {
        return (data.to_vec(), s.cells.values().map(|c| c.name.clone()).collect());
    };
    let Some(p) = &app.project else { return (data.to_vec(), Vec::new()) };
    let mut edits = Vec::new();
    let mut unsaved = Vec::new();
    let now: HashMap<u32, ((usize, usize), &Cell)> = s.cells.iter().filter_map(|(k, c)| c.origin.map(|o| (o, (*k, c)))).collect();
    for c in &g.cells {
        match now.get(&c.clip) {
            None => edits.push(Edit::Clear { row: c.row, scene: c.scene }),
            Some(&((t, scene), cell)) => {
                let track = p.tracks.get(t);
                let row = track.and_then(|tr| g.row_channels.iter().position(|ch| *ch == tr.channel));
                match (row, track) {
                    (Some(row), Some(tr)) if (row as u32, scene as u32) != (c.row, c.scene) && scene < g.scenes as usize => {
                        edits.push(Edit::Move { from: (c.row, c.scene), to: (row as u32, scene as u32), channel: tr.channel, track_row: tr.row });
                    }
                    (Some(row), _) if (row as u32, scene as u32) == (c.row, c.scene) => {}
                    _ => unsaved.push(cell.name.clone()),
                }
                if cell.notes != c.notes && c.inner.is_some() {
                    edits.push(Edit::Notes { clip: c.clip, notes: cell.notes.clone() });
                }
                if (cell.speed - c.params.speed).abs() > 1e-6 {
                    edits.push(Edit::Speed { clip: c.clip, speed: cell.speed });
                }
            }
        }
    }
    for c in s.cells.values().filter(|c| c.origin.is_none()) {
        unsaved.push(c.name.clone());
    }
    match lg_core::liveloops::write(data, &edits) {
        Some(out) => (out, unsaved),
        None => (data.to_vec(), s.cells.values().map(|c| c.name.clone()).collect()),
    }
}

// ------------------------------------------------------------------------------------------------
// playback

fn bar(app: &App) -> u32 {
    app.bar_ticks().max(1)
}

fn engine_pos(app: &App) -> (f64, bool) {
    let e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    (e.pos, e.playing)
}

/// The boundary a cell (or scene, with `scene_q`) starts at, from `pos`.
fn start_at(app: &App, t: usize, cell_q: &str, pos: f64) -> u32 {
    let s = &app.live_loops;
    let label = if cell_q == "Global" { s.quantize.as_str() } else { cell_q };
    match quantize_ticks(label, bar(app)) {
        Some(0) => {
            // Cell End: when the track's playing cell ends its loop
            if let Some(Some((sc, st, _))) = s.plays.get(&t).map(|p| p.current) {
                if let Some(c) = s.cells.get(&(t, sc)) {
                    let len = ((c.length as f32 / c.speed.max(0.01)) as u32).max(1);
                    return next_boundary(pos - st as f64, Some(len)) + st;
                }
            }
            next_boundary(pos, Some(bar(app)))
        }
        q => next_boundary(pos, q),
    }
}

/// The looping notes of the track's playing (and queued) cell that begin in [from_tick, to_tick),
/// in song ticks, for the engine to merge into the track's notes.
pub fn notes_for(app: &App, track: usize, from_tick: u32, to_tick: u32) -> Vec<(u32, u32, u8, u8)> {
    let s = &app.live_loops;
    let Some(play) = s.plays.get(&track) else { return Vec::new() };
    let mut out = Vec::new();
    let next_at = play.next.and_then(|n| n.1);
    if let Some((scene, start, stop)) = play.current {
        if let Some(c) = s.cells.get(&(track, scene)) {
            let end = match (stop, next_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            out.extend(loop_notes(c, start, end, from_tick, to_tick));
        }
    }
    match play.next {
        Some((scene, Some(at))) => {
            if let Some(c) = s.cells.get(&(track, scene)) {
                out.extend(loop_notes(c, at, None, from_tick, to_tick));
            }
        }
        Some((scene, None)) if play.current.is_none() => {
            // waiting for the transport: ready to sound from the playhead
            if let Some(c) = s.cells.get(&(track, scene)) {
                let at = engine_pos(app).0.max(0.0) as u32;
                out.extend(loop_notes(c, at, None, from_tick, to_tick));
            }
        }
        _ => {}
    }
    out.sort_by_key(|n| (n.0, n.2));
    out
}

/// The audio cell's loop copies that begin before `to_tick` and still sound after `from_tick`.
fn audio_for(app: &mut App, track: usize, from_tick: u32, to_tick: u32) -> Vec<(u32, Arc<Vec<f32>>)> {
    let tempo = app.tempo();
    let plays = app.live_loops.plays.get(&track).copied().unwrap_or_default();
    let mut segs = Vec::new();
    if let Some((sc, st, stop)) = plays.current {
        segs.push((sc, st, stop.or(plays.next.and_then(|n| n.1))));
    }
    if let Some((sc, Some(at))) = plays.next {
        segs.push((sc, at, None));
    }
    let mut out = Vec::new();
    for (sc, start, end) in segs {
        let Some(c) = app.live_loops.cells.get(&(track, sc)).cloned() else { continue };
        let Some(path) = c.audio.clone() else { continue };
        if c.muted {
            continue;
        }
        let Some(samples) = audio_samples(&mut app.live_loops, &path) else { continue };
        let speed = c.speed.max(0.01);
        let len = ((c.length as f32 / speed) as u32).max(1);
        let frames = (len as f64 / lg_core::events::PPQ as f64 * 60.0 / tempo * crate::audio::RATE as f64) as usize;
        let key = (path.clone(), frames);
        let piece = app.live_loops.loops.entry(key).or_insert_with(|| Arc::new(samples.iter().copied().take(frames * 2).collect())).clone();
        let first = from_tick.saturating_sub(start) / len;
        let mut k = first.saturating_sub(1);
        loop {
            let at = start + k * len;
            if at >= to_tick || end.is_some_and(|e| at >= e) || (!c.looped && k > 0) {
                break;
            }
            if at + len > from_tick {
                out.push((at, piece.clone()));
            }
            k += 1;
        }
    }
    out
}

/// An audio file's samples (read in the background the first time).
fn audio_samples(s: &mut State, path: &std::path::Path) -> Option<Arc<Vec<f32>>> {
    let mut m = s.audio.lock().unwrap_or_else(|e| e.into_inner());
    match m.get(path) {
        Some(v) => v.clone(),
        None => {
            m.insert(path.to_path_buf(), None);
            let (store, p) = (s.audio.clone(), path.to_path_buf());
            std::thread::spawn(move || {
                let v = crate::audio::load_audio(&p).map(Arc::new);
                if let Ok(mut m) = store.lock() {
                    m.insert(p, v);
                }
            });
            None
        }
    }
}

/// Once a frame: queued cells start at their boundary, stops happen, recordings end, and the
/// playing cells' notes go into the engine (each playing track's own regions are silent).
pub fn update(app: &mut App) {
    sync(app);
    let (pos, playing) = engine_pos(app);
    let b = bar(app);
    let recording = app.recording();
    let tracks: Vec<usize> = app.live_loops.plays.keys().copied().collect();
    {
        let s = &mut app.live_loops;
        // the transport started: cells waiting for it start at the playhead
        if playing && !s.was_playing {
            for p in s.plays.values_mut() {
                if let Some((sc, None)) = p.next {
                    p.current = Some((sc, pos.max(0.0) as u32, None));
                    p.next = None;
                }
            }
            if let Some(r) = s.rec.as_mut() {
                r.start = next_boundary(pos, Some(b));
            }
        }
        // stopped: playing cells wait for the next start, their ring kept
        if !playing && s.was_playing {
            for (&t, p) in s.plays.iter_mut() {
                if let Some((sc, st, stop)) = p.current.take() {
                    if let Some(c) = s.cells.get(&(t, sc)) {
                        p.frac = progress(c, st, pos);
                    }
                    if stop.is_none() && p.next.is_none() {
                        p.next = Some((sc, None));
                    } else if let Some((nsc, _)) = p.next {
                        p.next = Some((nsc, None));
                    }
                }
            }
        }
        s.was_playing = playing;
        for t in &tracks {
            let Some(p) = s.plays.get_mut(t) else { continue };
            // cycle jumps back: the loop keeps going from there
            if let Some((sc, st, stop)) = p.current {
                if pos + 1.0 < st as f64 && playing {
                    let len = s.cells.get(&(*t, sc)).map_or(b, |c| ((c.length as f32 / c.speed.max(0.01)) as u32).max(1));
                    let back = ((st as f64 - pos) / len as f64).ceil() as u32 * len;
                    p.current = Some((sc, st.saturating_sub(back), stop.map(|x| x.saturating_sub(back))));
                }
                if let Some(x) = stop {
                    if pos >= x as f64 {
                        if s.performance {
                            s.perf_log.push((*t, sc, st, Some(x)));
                        }
                        p.current = None;
                    }
                }
            }
            if let Some((sc, Some(at))) = p.next {
                if playing && pos >= at as f64 {
                    if let Some((osc, ost, _)) = p.current {
                        if s.performance && recording {
                            s.perf_log.push((*t, osc, ost, Some(at)));
                        }
                    }
                    p.current = Some((sc, at, None));
                    p.next = None;
                    if s.performance && recording {
                        s.perf_log.push((*t, sc, at, None));
                    }
                }
            }
        }
        s.plays.retain(|_, p| p.current.is_some() || p.next.is_some());
    }
    // recording into a cell: ends at its boundary (or keeps going)
    if let Some(r) = app.live_loops.rec {
        if let Some(end) = r.end {
            if playing && pos >= end as f64 {
                finish_cell_recording(app, end);
            }
        }
    }
    // performance recording: the cells played become regions when recording ends
    if app.live_loops.was_recording && !recording {
        perform_to_tracks(app, pos as u32);
    }
    app.live_loops.was_recording = recording;
    feed(app, pos, b);
}

/// Writes the playing cells' notes and audio into the engine's tracks (and gives the tracks
/// their own back when their cells stop).
fn feed(app: &mut App, pos: f64, b: u32) {
    let from = (pos as u32).saturating_sub(b);
    let to = pos as u32 + 4 * b;
    let busy: Vec<usize> = app.live_loops.plays.keys().copied().collect();
    let mut want: Vec<(usize, Vec<(u32, u32, u8, u8)>, Vec<(u32, Arc<Vec<f32>>)>)> = Vec::new();
    for &t in &busy {
        let n = notes_for(app, t, from, to);
        let a = audio_for(app, t, from, to);
        want.push((t, n, a));
    }
    let engine = app.engine.clone();
    let mut e = engine.lock().unwrap_or_else(|e| e.into_inner());
    let fed = &mut app.live_loops.fed;
    for (t, notes, audio) in want {
        let Some(tr) = e.tracks.get_mut(t) else { continue };
        let ours = fed.get(&t).map(|f| &f.2);
        if ours != Some(&tr.notes) {
            // the engine's own (first time, or rebuilt since)
            fed.insert(t, (tr.notes.clone(), tr.audio.clone(), Vec::new()));
        }
        tr.notes = notes.clone();
        tr.audio = audio;
        if let Some(f) = fed.get_mut(&t) {
            f.2 = notes;
        }
    }
    let done: Vec<usize> = fed.keys().copied().filter(|t| !busy.contains(t)).collect();
    for t in done {
        if let (Some((notes, audio, ours)), Some(tr)) = (fed.remove(&t), e.tracks.get_mut(t)) {
            if tr.notes == ours {
                tr.notes = notes;
                tr.audio = audio;
            }
            let ch = tr.channel;
            let _ = ch;
        }
    }
}

/// Whether the track's cell has taken it over (draw its regions grey; they don't sound).
pub fn takes_over(app: &App, track: usize) -> bool {
    app.live_loops.active(track)
}

// ------------------------------------------------------------------------------------------------
// actions

/// Triggers a cell: queued for its Quantize Start boundary (transport stopped: it starts now,
/// from the playhead's bar); a click on the playing cell stops it at the boundary.
pub fn trigger(app: &mut App, t: usize, scene: usize, retrigger: bool) {
    let Some(c) = app.live_loops.cells.get(&(t, scene)).cloned() else { return };
    let (pos, playing) = engine_pos(app);
    app.live_loops.pending.remove(&(t, scene));
    if !playing {
        // Logic: the transport starts at once and the cell with it
        let b = bar(app) as f64;
        let at = (pos / b).floor() * b;
        app.locate(at);
        let p = app.live_loops.play(t);
        p.next = Some((scene, None));
        p.frac = 0.0;
        app.play_stop();
        return;
    }
    let p = app.live_loops.plays.get(&t).copied().unwrap_or_default();
    let is_current = p.current.is_some_and(|c| c.0 == scene);
    let label = c.quantize_start.clone();
    let at = start_at(app, t, &label, pos);
    let s = app.live_loops.play(t);
    if p.next.is_some_and(|n| n.0 == scene) && !retrigger {
        // a second click un-queues it
        s.next = None;
        return;
    }
    if is_current && !retrigger && c.trigger != "Retrigger" {
        if let Some(cur) = s.current.as_mut() {
            cur.2 = Some(at);
        }
        return;
    }
    s.next = Some((scene, Some(at)));
}

/// Stops the track's cell at its boundary (Off: now).
pub fn stop_track(app: &mut App, t: usize) {
    let (pos, playing) = engine_pos(app);
    let label = app.live_loops.cells.get(&(t, app.live_loops.plays.get(&t).and_then(|p| p.current).map_or(0, |c| c.0))).map_or("Global".to_string(), |c| c.quantize_start.clone());
    let at = start_at(app, t, &label, pos);
    let Some(p) = app.live_loops.plays.get_mut(&t) else { return };
    p.next = None;
    if !playing {
        p.current = None;
        return;
    }
    if let Some(c) = p.current.as_mut() {
        c.2 = Some(at);
    }
}

pub fn stop_all(app: &mut App) {
    let tracks: Vec<usize> = app.live_loops.plays.keys().copied().collect();
    for t in tracks {
        stop_track(app, t);
    }
    app.live_loops.pending.clear();
}

/// A scene's trigger: its cells start together; tracks with nothing in the scene stop.
pub fn trigger_scene(app: &mut App, scene: usize) {
    let (pos, playing) = engine_pos(app);
    app.live_loops.focused_scene = Some(scene);
    let q = app.live_loops.scene_opts.get(&scene).map_or("Global".to_string(), |s| s.quantize_start.clone());
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let in_scene: Vec<usize> = (0..n_tracks).filter(|t| app.live_loops.cells.contains_key(&(*t, scene))).collect();
    if !playing {
        let b = bar(app) as f64;
        app.locate((pos / b).floor() * b);
        for t in 0..n_tracks {
            if in_scene.contains(&t) {
                let p = app.live_loops.play(t);
                p.next = Some((scene, None));
                p.frac = 0.0;
            } else if let Some(p) = app.live_loops.plays.get_mut(&t) {
                p.next = None;
            }
        }
        if !in_scene.is_empty() {
            app.play_stop();
        }
        return;
    }
    let at = start_at(app, usize::MAX, &q, pos);
    for t in 0..n_tracks {
        if in_scene.contains(&t) {
            app.live_loops.play(t).next = Some((scene, Some(at)));
        } else if let Some(p) = app.live_loops.plays.get_mut(&t) {
            p.next = None;
            if let Some(c) = p.current.as_mut() {
                c.2 = Some(at);
            }
        }
    }
}

/// Record into Cell: starts at the next bar (the transport starts if stopped); a second time,
/// it stops at the next bar and the cell plays (Rec-End Action: Play Cell).
pub fn record_into_cell(app: &mut App, t: usize, scene: usize) {
    let (pos, playing) = engine_pos(app);
    let b = bar(app);
    if let Some(r) = app.live_loops.rec {
        if r.end.is_none() {
            let end = if playing { next_boundary(pos, Some(b)).max(r.start + b) } else { pos as u32 };
            app.live_loops.rec = Some(Rec { end: Some(end), ..r });
            if !playing {
                finish_cell_recording(app, end);
            }
        }
        return;
    }
    app.selected_track = Some(t);
    let start = if playing {
        next_boundary(pos, Some(b))
    } else {
        let at = (pos / b as f64).floor() * b as f64;
        app.locate(at);
        app.play_stop();
        at as u32
    };
    app.played.retain(|n| (n.pos as f64) < start as f64);
    app.live_loops.rec = Some(Rec { track: t, scene, start, end: None });
    stop_track(app, t);
}

/// The notes played since the recording began go into the cell (merged with what it had).
fn finish_cell_recording(app: &mut App, end: u32) {
    let Some(r) = app.live_loops.rec.take() else { return };
    let b = bar(app);
    let end = end.max(r.start + b);
    let played: Vec<Note> = app.played.iter().copied().filter(|n| n.pos >= r.start && n.pos < end).collect();
    let (notes, len) = cell_from_notes(&played, r.start, end, b);
    let name = app.project.as_ref().and_then(|p| p.tracks.get(r.track)).map(|t| t.name.clone()).unwrap_or_default();
    let mode = app.live_loops.rec_mode.clone();
    let s = &mut app.live_loops;
    let cell = s.cells.entry((r.track, r.scene)).or_insert_with(|| Cell { name, length: len, ..Default::default() });
    if mode == "Replace" || cell.notes.is_empty() {
        cell.notes = notes;
        cell.length = len;
    } else {
        cell.notes.extend(notes);
        cell.notes.sort_by_key(|n| (n.pos, n.pitch));
        cell.length = cell.length.max(len);
    }
    s.scenes = s.scenes.max(r.scene + 1);
    if s.rec_end == "Play Cell" {
        let p = s.play(r.track);
        p.current = Some((r.scene, end, None));
        p.next = None;
    }
    app.dirty = true;
}

/// Performance recording: what the cells played while recording becomes regions.
fn perform_to_tracks(app: &mut App, now: u32) {
    let log = std::mem::take(&mut app.live_loops.perf_log);
    if log.is_empty() || app.project.is_none() {
        return;
    }
    app.checkpoint("Performance Recording");
    for (t, sc, from, to) in log {
        let to = to.unwrap_or(now);
        let Some(c) = app.live_loops.cells.get(&(t, sc)).cloned() else { continue };
        if to <= from {
            continue;
        }
        let notes: Vec<Note> = loop_notes(&c, from, Some(to), from, to).into_iter().map(|(a, e, p, v)| Note { channel: 0, pitch: p, velocity: v, pos: a, len: e - a, muted: false }).collect();
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            tr.regions.push(lg_core::Region { name: c.name.clone(), start: from, length: to - from, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
            tr.regions.sort_by_key(|r| r.start);
        }
    }
    app.after_edit();
}

/// A region as a cell (notes from its start).
pub fn cell_of_region(r: &lg_core::Region) -> Cell {
    let notes = r.notes.iter().filter(|n| n.pos >= r.start && n.pos < r.start + r.length).map(|n| Note { pos: n.pos - r.start, ..*n }).collect();
    Cell { name: r.name.clone(), notes, length: r.length.max(1), audio: r.audio.as_ref().map(|a| a.file.clone()), transpose: r.props.transpose, looped: true, ..Default::default() }
}

/// Copies the region into a cell (Logic: the cell gets its own copy).
pub fn region_to_cell(app: &mut App, track: usize, region: usize, to: (usize, usize)) {
    let Some(r) = app.project.as_ref().and_then(|p| p.tracks.get(track)).and_then(|t| t.regions.get(region)).cloned() else { return };
    let s = &mut app.live_loops;
    s.cells.insert(to, cell_of_region(&r));
    s.scenes = s.scenes.max(to.1 + 1);
    s.selected = [to].into();
    app.dirty = true;
}

/// Copy to Live Loops Scene (⌘⤒): the selected regions into the first scene with room, each on
/// its track; with `cycle`, what lies in the cycle area of every track.
pub fn copy_to_scene(app: &mut App, cycle: bool) -> bool {
    let Some(p) = app.project.as_ref() else { return false };
    let b = bar(app);
    let mut cells: Vec<(usize, Cell)> = Vec::new();
    if cycle {
        let (a, z) = app.cycle;
        if z <= a {
            return false;
        }
        for (t, tr) in p.tracks.iter().enumerate() {
            let notes: Vec<Note> = tr.regions.iter().filter(|r| !r.muted).flat_map(|r| r.notes.iter().copied()).collect();
            let (n, _) = cell_from_notes(&notes, a, z, b);
            if !n.is_empty() {
                cells.push((t, Cell { name: tr.name.clone(), notes: n, length: z - a, ..Default::default() }));
            }
        }
    } else {
        let mut sel = app.selected_regions.clone();
        if sel.is_empty() {
            sel.extend(app.selected_region);
        }
        for (t, r) in sel {
            if let Some(rg) = p.tracks.get(t).and_then(|tr| tr.regions.get(r)) {
                if !cells.iter().any(|c| c.0 == t) {
                    cells.push((t, cell_of_region(rg)));
                }
            }
        }
    }
    if cells.is_empty() {
        return false;
    }
    let s = &mut app.live_loops;
    let scene = (0..).find(|sc| cells.iter().all(|(t, _)| !s.cells.contains_key(&(*t, *sc)))).unwrap_or(0);
    s.selected.clear();
    for (t, c) in cells {
        s.cells.insert((t, scene), c);
        s.selected.insert((t, scene));
    }
    s.scenes = s.scenes.max(scene + 1);
    s.focused_scene = Some(scene);
    if s.view == View::TracksOnly {
        s.view = View::Both;
    }
    app.dirty = true;
    true
}

/// A scene's cells as regions at the playhead's bar (insert: what follows moves later).
pub fn scene_to_playhead(app: &mut App, scene: usize, insert: bool) {
    let b = bar(app);
    let at = (app.position() as u32 / b) * b;
    let cells: Vec<(usize, Cell)> = app.live_loops.cells.iter().filter(|(k, _)| k.1 == scene).map(|(k, c)| (k.0, c.clone())).collect();
    if cells.is_empty() {
        return;
    }
    let len = cells.iter().map(|(_, c)| c.length).max().unwrap_or(b);
    app.checkpoint(if insert { "Insert Scene at Playhead" } else { "Copy Scene to Playhead" });
    let Some(p) = app.project.as_mut() else { return };
    if insert {
        for tr in &mut p.tracks {
            for r in tr.regions.iter_mut().filter(|r| r.start >= at) {
                r.start += len;
                for n in &mut r.notes {
                    n.pos += len;
                }
            }
        }
    }
    for (t, c) in cells {
        let Some(tr) = p.tracks.get_mut(t) else { continue };
        let notes = sounding(&c).into_iter().map(|n| Note { pos: n.pos + at, ..n }).collect();
        let audio = c.audio.clone().map(|f| lg_core::project::AudioRef { file: f, seconds: 0.0 });
        tr.regions.push(lg_core::Region { name: c.name.clone(), start: at, length: c.length, notes, audio, muted: c.muted, source: None, origin: None, props: Default::default(), ext: Default::default() });
        tr.regions.sort_by_key(|r| r.start);
    }
    app.after_edit();
}

fn insert_scene(s: &mut State, at: usize) {
    let moved: Vec<((usize, usize), Cell)> = s.cells.iter().filter(|(k, _)| k.1 >= at).map(|(k, c)| (*k, c.clone())).collect();
    for (k, _) in &moved {
        s.cells.remove(k);
    }
    for ((t, sc), c) in moved {
        s.cells.insert((t, sc + 1), c);
    }
    let opts: Vec<(usize, Scene)> = s.scene_opts.iter().filter(|(k, _)| **k >= at).map(|(k, v)| (*k, v.clone())).collect();
    for (k, _) in &opts {
        s.scene_opts.remove(k);
    }
    for (k, v) in opts {
        s.scene_opts.insert(k + 1, v);
    }
    for p in s.plays.values_mut() {
        if let Some(c) = p.current.as_mut() {
            if c.0 >= at {
                c.0 += 1;
            }
        }
        if let Some(n) = p.next.as_mut() {
            if n.0 >= at {
                n.0 += 1;
            }
        }
    }
    s.scenes = s.scenes.max(s.used_scenes()) + 1;
}

fn delete_scene(s: &mut State, at: usize) {
    s.cells.retain(|k, _| k.1 != at);
    let moved: Vec<((usize, usize), Cell)> = s.cells.iter().filter(|(k, _)| k.1 > at).map(|(k, c)| (*k, c.clone())).collect();
    for (k, _) in &moved {
        s.cells.remove(k);
    }
    for ((t, sc), c) in moved {
        s.cells.insert((t, sc - 1), c);
    }
    s.plays.retain(|_, p| !p.current.is_some_and(|c| c.0 == at));
    for p in s.plays.values_mut() {
        if let Some(c) = p.current.as_mut() {
            if c.0 > at {
                c.0 -= 1;
            }
        }
        if p.next.is_some_and(|n| n.0 == at) {
            p.next = None;
        }
        if let Some(n) = p.next.as_mut() {
            if n.0 > at {
                n.0 -= 1;
            }
        }
    }
    s.scenes = s.scenes.saturating_sub(1).max(s.used_scenes());
}

fn selection_or(app: &App, at: Option<(usize, usize)>) -> Vec<(usize, usize)> {
    let s = &app.live_loops;
    match at {
        Some(k) if !s.selected.contains(&k) => vec![k],
        _ if !s.selected.is_empty() => s.selected.iter().copied().collect(),
        Some(k) => vec![k],
        None => Vec::new(),
    }
}

// ------------------------------------------------------------------------------------------------
// commands

/// Runs a Live Loops command by its Logic name (the "Live Loops Grid" key commands, the view keys,
/// Stop All Cells, Record into Cell, Copy to Live Loops Scene, and the cell menus' plain items on
/// the selected cells); false when it isn't one.
pub fn command(app: &mut App, name: &str) -> bool {
    command_at(app, name, None, None)
}

fn command_at(app: &mut App, name: &str, cell: Option<(usize, usize)>, scene: Option<usize>) -> bool {
    sync(app);
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let sel_or_focus = |app: &App| -> Vec<(usize, usize)> {
        let s = &app.live_loops;
        if !s.selected.is_empty() {
            s.selected.iter().copied().collect()
        } else if let Some(sc) = s.focused_scene {
            s.cells.keys().filter(|k| k.1 == sc).copied().collect()
        } else {
            Vec::new()
        }
    };
    match name {
        // views
        "Switch between Live Loops Grid and Tracks View" => {
            let v = &mut app.live_loops.view;
            *v = if *v == View::GridOnly { View::TracksOnly } else { View::GridOnly };
        }
        "Show Both, Live Loops Grid and Tracks View" => app.live_loops.view = View::Both,
        "Show Live Loops Grid only" => app.live_loops.view = View::GridOnly,
        "Show Tracks View only" => app.live_loops.view = View::TracksOnly,
        "Show/Hide Live Loops Grid" => {
            let v = &mut app.live_loops.view;
            *v = if *v == View::TracksOnly { View::Both } else { View::TracksOnly };
        }
        "Show/Hide Tracks area" => {
            let v = &mut app.live_loops.view;
            *v = match *v {
                View::Both => View::GridOnly,
                View::GridOnly => View::Both,
                View::TracksOnly => View::TracksOnly,
            };
        }
        "Toggle Grid Horizontal Zoom" => app.live_loops.wide = !app.live_loops.wide,
        "Copy to Live Loops Scene" => return copy_to_scene(app, false),
        "Copy Cycle to Live Loops Scene" => return copy_to_scene(app, true),
        // playing
        "Stop All Cells" => stop_all(app),
        "Trigger Cells or Scene" => {
            let mut cells: Vec<(usize, usize)> = app.live_loops.pending.iter().copied().collect();
            cells.extend(app.live_loops.selected.iter().copied());
            cells.dedup();
            if cells.is_empty() {
                let Some(sc) = app.live_loops.focused_scene else { return true };
                trigger_scene(app, sc);
            } else {
                for (t, sc) in cells {
                    trigger(app, t, sc, false);
                }
            }
        }
        "Queue Cell/Scene Playback" | "Queue Cell Playback" => {
            let cells = if cell.is_some() { selection_or(app, cell) } else { sel_or_focus(app) };
            let s = &mut app.live_loops;
            for k in cells {
                if s.cells.contains_key(&k) && !s.pending.remove(&k) {
                    s.pending.insert(k);
                }
            }
        }
        "Trigger Focused Scene" => {
            let Some(sc) = app.live_loops.focused_scene else { return true };
            trigger_scene(app, sc);
        }
        "Retrigger Selected Cells or Focused Scene" => {
            let cells: Vec<(usize, usize)> = app.live_loops.selected.iter().copied().collect();
            if cells.is_empty() {
                if let Some(sc) = app.live_loops.focused_scene {
                    let ks: Vec<(usize, usize)> = app.live_loops.cells.keys().filter(|k| k.1 == sc).copied().collect();
                    for (t, s) in ks {
                        trigger(app, t, s, true);
                    }
                }
            } else {
                for (t, s) in cells {
                    trigger(app, t, s, true);
                }
            }
        }
        "Play Previous Scene" | "Play Next Scene" => {
            let cur = app.live_loops.focused_scene.unwrap_or(0);
            let sc = if name == "Play Next Scene" { cur + 1 } else { cur.saturating_sub(1) };
            trigger_scene(app, sc);
        }
        "Insert Empty Scene" => {
            let at = scene.or(app.live_loops.focused_scene).map_or(app.live_loops.scenes, |s| s + 1);
            insert_scene(&mut app.live_loops, at);
            app.live_loops.focused_scene = Some(at);
        }
        "Copy Playing Cells Here" => {
            let Some(sc) = scene.or(app.live_loops.focused_scene).or(cell.map(|c| c.1)) else { return true };
            let s = &mut app.live_loops;
            let playing: Vec<(usize, usize)> = s.plays.iter().filter_map(|(t, p)| p.current.map(|c| (*t, c.0))).collect();
            for (t, from) in playing {
                if let Some(c) = s.cells.get(&(t, from)).cloned() {
                    s.cells.insert((t, sc), Cell { origin: None, ..c });
                }
            }
            s.scenes = s.scenes.max(sc + 1);
        }
        "Select Playing Cells" => {
            let s = &mut app.live_loops;
            s.selected = s.plays.iter().filter_map(|(t, p)| p.current.map(|c| (*t, c.0))).collect();
        }
        "Select All Cells of Same Scene" | "All Cells of Same Scene" => {
            let sc = cell.map(|c| c.1).or_else(|| app.live_loops.selected.iter().next().map(|k| k.1)).or(app.live_loops.focused_scene);
            if let Some(sc) = sc {
                let s = &mut app.live_loops;
                s.selected = s.cells.keys().filter(|k| k.1 == sc).copied().collect();
            }
        }
        "Select Next Lower Cell, or Next Track" | "Select Next Upper Cell, or Previous Track" | "Extend Cell, or Track Selection Up" | "Extend Cell, or Track Selection Down" => {
            let down = name.contains("Lower") || name.ends_with("Down");
            let extend = name.starts_with("Extend");
            let s = &mut app.live_loops;
            let Some(&cur) = (if down { s.selected.iter().max() } else { s.selected.iter().min() }) else {
                let t = app.selected_track.map_or(0, |t| if down { (t + 1).min(n_tracks.saturating_sub(1)) } else { t.saturating_sub(1) });
                app.selected_track = Some(t);
                return true;
            };
            let t = if down { (cur.0 + 1).min(n_tracks.saturating_sub(1)) } else { cur.0.saturating_sub(1) };
            if !extend {
                s.selected.clear();
            }
            s.selected.insert((t, cur.1));
            app.selected_track = Some(t);
        }
        "Select Previous Scene" | "Select Next Scene" => {
            let s = &mut app.live_loops;
            let cur = s.focused_scene.or(s.selected.iter().next().map(|k| k.1)).unwrap_or(0);
            let sc = if name.contains("Next") { cur + 1 } else { cur.saturating_sub(1) };
            s.focused_scene = Some(sc);
            let t = s.selected.iter().next().map(|k| k.0).or(app.selected_track).unwrap_or(0);
            s.selected = [(t, sc)].into();
            s.scenes = s.scenes.max(sc + 1);
        }
        "Enable Performance Recording On/Off" => app.live_loops.performance = !app.live_loops.performance,
        "Record into Cell" => {
            let target = cell.or_else(|| app.live_loops.selected.iter().next().copied()).or_else(|| app.selected_track.map(|t| (t, app.live_loops.focused_scene.unwrap_or(0))));
            let Some((t, sc)) = target else { return true };
            record_into_cell(app, t, sc);
        }
        "Stop Playing Cell" => {
            let tracks: Vec<usize> = selection_or(app, cell).into_iter().map(|k| k.0).collect();
            for t in tracks {
                stop_track(app, t);
            }
        }
        "Create MIDI Cell" => {
            let Some(k) = cell.or_else(|| app.live_loops.selected.iter().next().copied()) else { return true };
            let name = app.project.as_ref().and_then(|p| p.tracks.get(k.0)).map(|t| t.name.clone()).unwrap_or_default();
            let len = bar(app);
            let s = &mut app.live_loops;
            s.cells.entry(k).or_insert(Cell { name, length: len, ..Default::default() });
            s.scenes = s.scenes.max(k.1 + 1);
            s.selected = [k].into();
            app.dirty = true;
        }
        // cells: Edit
        "Delete" if scene.is_some() && cell.is_none() => {
            if let Some(sc) = scene {
                delete_scene(&mut app.live_loops, sc);
                app.dirty = true;
            }
        }
        "Cut" | "Copy" | "Delete" => {
            let keys = selection_or(app, cell);
            let s = &mut app.live_loops;
            if name != "Delete" {
                s.clipboard = keys.first().and_then(|k| s.cells.get(k)).cloned();
            }
            if name != "Copy" {
                for k in keys {
                    s.cells.remove(&k);
                    if let Some(p) = s.plays.get_mut(&k.0) {
                        if p.current.is_some_and(|c| c.1 == 0 || c.0 == k.1) {
                            p.current = None;
                        }
                        if p.next.is_some_and(|n| n.0 == k.1) {
                            p.next = None;
                        }
                    }
                }
                app.dirty = true;
            }
        }
        "Paste" => {
            let Some(k) = cell.or_else(|| app.live_loops.selected.iter().next().copied()) else { return false };
            let s = &mut app.live_loops;
            if let Some(c) = s.clipboard.clone() {
                s.cells.insert(k, Cell { origin: None, ..c });
            } else if let Some(r) = app.clipboard.clone() {
                s.cells.insert(k, cell_of_region(&r));
                // (Logic: pasting into a cell starts the transport and the cell)
            } else {
                return false;
            }
            s.scenes = s.scenes.max(k.1 + 1);
            app.dirty = true;
            if !app.playing() {
                trigger(app, k.0, k.1, false);
            }
        }
        "Repeat" => {
            for k in selection_or(app, cell) {
                if let Some(c) = app.live_loops.cells.get_mut(&k) {
                    let l = c.length;
                    let extra: Vec<Note> = c.notes.iter().filter(|n| n.pos < l).map(|n| Note { pos: n.pos + l, ..*n }).collect();
                    c.notes.extend(extra);
                    c.length = l * 2;
                }
            }
        }
        "All Cells" => {
            let s = &mut app.live_loops;
            s.selected = s.cells.keys().copied().collect();
        }
        "All Following Cells" | "All Following Cells of Same Track" | "All Cells of Same Track" => {
            let Some(k) = cell.or_else(|| app.live_loops.selected.iter().next().copied()) else { return true };
            let s = &mut app.live_loops;
            s.selected = s.cells.keys().filter(|c| match name {
                "All Following Cells" => c.1 >= k.1,
                "All Following Cells of Same Track" => c.0 == k.0 && c.1 >= k.1,
                _ => c.0 == k.0,
            }).copied().collect();
        }
        "Invert Cell Selection" => {
            let s = &mut app.live_loops;
            s.selected = s.cells.keys().filter(|k| !s.selected.contains(k)).copied().collect();
        }
        "Deselect All" => app.live_loops.selected.clear(),
        "Same-Colored Cells" => {
            let s = &mut app.live_loops;
            s.selected = s.cells.keys().copied().collect();
        }
        "Mute" => {
            for k in selection_or(app, cell) {
                if let Some(c) = app.live_loops.cells.get_mut(&k) {
                    c.muted = !c.muted;
                }
            }
        }
        "Loop" => {
            for k in selection_or(app, cell) {
                if let Some(c) = app.live_loops.cells.get_mut(&k) {
                    c.looped = !c.looped;
                }
            }
        }
        "Reverse" => {
            for k in selection_or(app, cell) {
                if let Some(c) = app.live_loops.cells.get_mut(&k) {
                    c.reversed = !c.reversed;
                }
            }
        }
        "Rename Cell" | "Rename Regions/Cells/Scenes" => {
            let Some(k) = cell.or_else(|| app.live_loops.selected.iter().next().copied()) else { return false };
            let n = app.live_loops.cells.get(&k).map(|c| c.name.clone()).unwrap_or_default();
            app.live_loops.renaming = Some((Some(k), k.1, n));
        }
        "Name Cell by Track" => {
            for k in selection_or(app, cell) {
                let n = app.project.as_ref().and_then(|p| p.tracks.get(k.0)).map(|t| t.name.clone()).unwrap_or_default();
                if let Some(c) = app.live_loops.cells.get_mut(&k) {
                    c.name = n;
                }
            }
        }
        "Color Cell by Track" => {}
        "Show/Hide Colors" => app.live_loops.colors_off = !app.live_loops.colors_off,
        // scenes
        "Queue Scene" => {
            let Some(sc) = scene else { return true };
            let s = &mut app.live_loops;
            let ks: Vec<(usize, usize)> = s.cells.keys().filter(|k| k.1 == sc).copied().collect();
            s.pending.extend(ks);
        }
        "Duplicate" => {
            let Some(sc) = scene else { return true };
            insert_scene(&mut app.live_loops, sc + 1);
            let s = &mut app.live_loops;
            let copies: Vec<((usize, usize), Cell)> = s.cells.iter().filter(|(k, _)| k.1 == sc).map(|(k, c)| ((k.0, sc + 1), Cell { origin: None, ..c.clone() })).collect();
            s.cells.extend(copies);
            if let Some(o) = s.scene_opts.get(&sc).cloned() {
                s.scene_opts.insert(sc + 1, o);
            }
        }
        "Rename" => {
            let Some(sc) = scene else { return true };
            let n = app.live_loops.scene_opts.get(&sc).map(|s| s.name.clone()).unwrap_or_default();
            app.live_loops.renaming = Some((None, sc, n));
        }
        "Copy Scene to Playhead" | "Insert Scene at Playhead" => {
            let Some(sc) = scene.or(app.live_loops.focused_scene) else { return true };
            scene_to_playhead(app, sc, name.starts_with("Insert"));
        }
        "Insert Scenes at Playhead" => {
            let Some(sc) = app.live_loops.focused_scene else { return true };
            scene_to_playhead(app, sc, true);
        }
        _ => return false,
    }
    true
}

/// A submenu item: "Quantize Start" + "1 Bar" and the like.
fn submenu(app: &mut App, path: &str, item: &str, cell: Option<(usize, usize)>, scene: Option<usize>) -> bool {
    let cells = selection_or(app, cell);
    let in_scene = path.starts_with("Scene");
    let set = |app: &mut App, f: &dyn Fn(&mut Cell)| {
        for k in &cells {
            if let Some(c) = app.live_loops.cells.get_mut(k) {
                f(c);
            }
        }
    };
    let leaf = path.rsplit('/').next().unwrap_or(path);
    match (leaf, in_scene) {
        ("Quantize Start", true) if path.contains("Set All") => {
            let n = app.live_loops.scenes.max(app.live_loops.used_scenes());
            for sc in 0..n {
                app.live_loops.scene_opts.entry(sc).or_default().quantize_start = item.to_string();
            }
        }
        ("Trigger Mode", true) if path.contains("Set All") => {
            let n = app.live_loops.scenes.max(app.live_loops.used_scenes());
            for sc in 0..n {
                app.live_loops.scene_opts.entry(sc).or_default().trigger = item.to_string();
            }
        }
        ("Quantize Start", true) => {
            let Some(sc) = scene else { return true };
            app.live_loops.scene_opts.entry(sc).or_default().quantize_start = item.to_string();
        }
        ("Trigger Mode", true) => {
            let Some(sc) = scene else { return true };
            app.live_loops.scene_opts.entry(sc).or_default().trigger = item.to_string();
        }
        ("Grid Quantize Start", _) => app.live_loops.quantize = item.to_string(),
        ("Quantize Start", false) => set(app, &|c| c.quantize_start = item.to_string()),
        ("Trigger Mode", false) => set(app, &|c| c.trigger = item.to_string()),
        ("Play From", _) => set(app, &|c| c.play_from = item.to_string()),
        ("Speed", _) => {
            let Some(v) = SPEEDS.iter().find(|s| s.0 == item).map(|s| s.1) else { return false };
            set(app, &|c| c.speed = v);
        }
        ("Quantize", _) => set(app, &|c| c.quantize = item.to_string()),
        ("Transpose", _) => {
            let v: i8 = item.replace('±', "").replace('+', "").parse().unwrap_or(0);
            set(app, &|c| c.transpose = v);
        }
        ("Mode", _) => app.live_loops.rec_mode = item.to_string(),
        ("Rec-Length", _) => app.live_loops.rec_length = item.to_string(),
        ("Rec-End Action", _) => app.live_loops.rec_end = item.to_string(),
        ("Playback", _) | ("Edit", _) | ("Select", _) | ("Name and Color", _) => return command_at(app, item, cell, scene),
        _ => return false,
    }
    true
}

/// What the menus can run (others are greyed, as in the rest of Studio).
fn handled(path: &str, item: &str) -> bool {
    let unbuilt = ["Create Pattern Cell", "Create Session Player Cell", "Takes", "Cell Length", "Automatic (Beats)", "Continue Recording", "Bounce Cells in Place…", "Join Regions and Fill Cell", "Convert to Pattern Cell", "Replace with Session Player Cell", "Stem Splitter…", "Add to Browser…", "Export as Audio File…", "Quantize to Loop Start", "Same-Colored Cells", "Color Cell by Track"];
    if unbuilt.contains(&item) || path.contains("Quantize to Loop Start") {
        return false;
    }
    if path.contains("Play From") {
        return item == "Start";
    }
    true
}

fn checked(app: &App, path: &str, item: &str, cell: Option<(usize, usize)>, scene: Option<usize>) -> bool {
    let s = &app.live_loops;
    let c = cell.and_then(|k| s.cells.get(&k));
    let sc = scene.map(|k| s.scene_opts.get(&k).cloned().unwrap_or_default());
    let leaf = path.rsplit('/').next().unwrap_or("");
    match (leaf, item) {
        ("Mode", _) => s.rec_mode == item,
        ("Rec-Length", _) => s.rec_length == item,
        ("Rec-End Action", _) => s.rec_end == item,
        ("Quantize Start", _) if path.contains("Set All") => false,
        ("Quantize Start", _) => c.map(|c| c.quantize_start.as_str()).or(sc.as_ref().map(|s| s.quantize_start.as_str())) == Some(item),
        ("Trigger Mode", _) if path.contains("Set All") => false,
        ("Trigger Mode", _) => c.map(|c| c.trigger.as_str()).or(sc.as_ref().map(|s| s.trigger.as_str())) == Some(item),
        ("Play From", _) => c.is_some_and(|c| c.play_from == item),
        ("Speed", _) => c.is_some_and(|c| SPEEDS.iter().any(|s| s.0 == item && (s.1 - c.speed).abs() < 1e-6)),
        ("Quantize", _) => c.is_some_and(|c| c.quantize == item),
        ("Transpose", _) => c.is_some_and(|c| item.replace('±', "").replace('+', "").parse::<i8>().ok() == Some(c.transpose)),
        ("Quantize to Loop Start", "Off") => true,
        ("Playback", "Mute") => c.is_some_and(|c| c.muted),
        ("Playback", "Loop") => c.is_some_and(|c| c.looped),
        ("Playback", "Reverse") => c.is_some_and(|c| c.reversed),
        (_, "Stop Playing Cell") => cell.is_some_and(|k| s.plays.get(&k.0).is_some_and(|p| p.current.is_some_and(|c| c.0 == k.1))),
        _ => false,
    }
}

/// A Live Loops right-click menu (the research's), run here.
fn context_menu(app: &mut App, resp: &egui::Response, place: &str, cell: Option<(usize, usize)>, scene: Option<usize>) {
    let items = crate::menus::context(place);
    if items.is_empty() {
        return;
    }
    resp.context_menu(|ui| {
        ui.set_min_width(230.0);
        let mut fired: Option<(String, String)> = None;
        for it in items {
            menu_item(app, ui, it, if place.ends_with("scene-trigger") { "Scene" } else { "" }, cell, scene, &mut fired);
        }
        if let Some((path, item)) = fired {
            if path.is_empty() || path == "Scene" {
                command_at(app, &item, cell, scene);
            } else {
                submenu(app, &path, &item, cell, scene);
            }
        }
    });
}

fn menu_item(app: &App, ui: &mut egui::Ui, it: &crate::menus::Item, path: &str, cell: Option<(usize, usize)>, scene: Option<usize>, fired: &mut Option<(String, String)>) {
    if it.separator {
        ui.separator();
        return;
    }
    if !it.children.is_empty() {
        let full = if path.is_empty() { it.label.clone() } else { format!("{path}/{}", it.label) };
        ui.menu_button(&it.label, |ui| {
            for c in &it.children {
                menu_item(app, ui, c, &full, cell, scene, fired);
            }
        });
        return;
    }
    let on = handled(path, &it.label) && enabled(app, &it.label, cell);
    let label = if checked(app, path, &it.label, cell, scene) { format!("✓ {}", it.label) } else { it.label.clone() };
    if ui.add_enabled(on, egui::Button::new(label).shortcut_text(it.shortcut.clone())).clicked() {
        *fired = Some((path.to_string(), it.label.clone()));
        ui.close();
    }
}

fn enabled(app: &App, label: &str, cell: Option<(usize, usize)>) -> bool {
    match label {
        "Paste" => app.live_loops.clipboard.is_some() || app.clipboard.is_some(),
        "Stop Playing Cell" => cell.is_some_and(|k| app.live_loops.active(k.0)),
        _ => true,
    }
}

/// Logic's keys for the grid: the "Live Loops Grid" group while the grid has the focus, and the
/// Live Loops commands bound elsewhere (L, ⌥L, ⌘↩ Stop All Cells, ⌥R Record into Cell, ⌘⤒).
/// Call before the other key commands; true when it took the key.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers) -> bool {
    let grid_on = app.live_loops.view != View::TracksOnly;
    let focused = grid_on && app.live_loops.focused;
    if focused && app.live_loops.renaming.is_some() {
        return false;
    }
    for ctrl in [false, true] {
        for b in crate::keymap::bindings() {
            if b.key != key || b.cmd != m.command || b.alt != m.alt || b.shift != m.shift || b.ctrl != ctrl {
                continue;
            }
            let ours = match b.area.as_str() {
                "Live Loops Grid" => focused,
                _ => matches!(
                    b.command.as_str(),
                    "Switch between Live Loops Grid and Tracks View" | "Show Both, Live Loops Grid and Tracks View" | "Copy to Live Loops Scene" | "Stop All Cells" | "Record into Cell" | "Insert Scenes at Playhead"
                ) || (focused && matches!(b.command.as_str(), "Rename Regions/Cells/Scenes" | "Cut" | "Copy" | "Paste" | "Select All")),
            };
            if !ours {
                continue;
            }
            let name = if b.command == "Select All" { "All Cells" } else { b.command.as_str() };
            if name == "Record into Cell" && !grid_on {
                continue;
            }
            if command(app, name) {
                return true;
            }
        }
    }
    // ⌫ in the grid: delete the selected cells
    if focused && matches!(key, egui::Key::Backspace | egui::Key::Delete) && m.is_none() && !app.live_loops.selected.is_empty() {
        return command(app, "Delete");
    }
    false
}

// ------------------------------------------------------------------------------------------------
// drawing

/// The grid's width in a Tracks area `avail` wide (after the headers): 0 with Tracks only, all
/// of it with the grid only.
pub fn grid_width(app: &mut App, avail: f32) -> f32 {
    sync(app);
    let s = &app.live_loops;
    match s.view {
        View::TracksOnly => 0.0,
        View::GridOnly => avail.max(0.0),
        View::Both => {
            let pitch = col_w(s) + CELL_GAP;
            let want = if s.grid_w > 0.0 { s.grid_w } else { PAD * 2.0 + pitch * (s.scenes.max(2) as f32).min(4.0) + STOP_W };
            want.clamp(PAD * 2.0 + pitch + STOP_W, (avail - 120.0).max(PAD * 2.0 + pitch + STOP_W))
        }
    }
}

fn col_w(s: &State) -> f32 {
    if s.wide { CELL_W * 2.0 + CELL_GAP } else { CELL_W }
}

fn grid_bg() -> Color32 {
    if kn_ui::theme::is_light() { Color32::from_gray(0xe6) } else { Color32::from_rgb(21, 21, 20) }
}
fn empty_cell() -> Color32 {
    if kn_ui::theme::is_light() { Color32::from_gray(0xd4) } else { Color32::from_gray(38) }
}
fn column_bg() -> Color32 {
    if kn_ui::theme::is_light() { Color32::from_gray(0xdc) } else { Color32::from_gray(25) }
}
fn button_bg() -> Color32 {
    if kn_ui::theme::is_light() { Color32::from_gray(0xc8) } else { Color32::from_rgb(44, 43, 43) }
}
fn focus_blue() -> Color32 {
    Color32::from_rgb(0x46, 0x66, 0x96)
}

/// The track's colour for its cells (Logic: the region colour by kind).
fn track_color(app: &App, t: usize) -> Color32 {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(look::region_color(lg_core::TrackKind::Instrument), |tr| look::region_color(tr.kind))
}

/// Whether the grid and the tracks (timeline) are shown: for the Tracks area's view buttons.
pub fn shown(app: &mut App) -> (bool, bool) {
    sync(app);
    let v = app.live_loops.view;
    (v != View::TracksOnly, v != View::GridOnly)
}

fn play_glyph(p: &egui::Painter, c: Pos2, size: f32, col: Color32) {
    p.add(Shape::convex_polygon(vec![pos2(c.x - size * 0.4, c.y - size * 0.55), pos2(c.x + size * 0.6, c.y), pos2(c.x - size * 0.4, c.y + size * 0.55)], col, Stroke::NONE));
}

fn stop_glyph(p: &egui::Painter, c: Pos2, size: f32, col: Color32) {
    p.rect_filled(Rect::from_center_size(c, vec2(size, size)), 1.0, col);
}

fn arc(c: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = ((to - from).abs() * 24.0).ceil().max(2.0) as usize;
    (0..=n).map(|i| {
        let a = from + (to - from) * i as f32 / n as f32;
        pos2(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

/// The circle in a cell's middle: ▶ to start, ■ while playing (hover), the ring filling with
/// the cell's position, dashed while queued.
fn draw_ring(p: &egui::Painter, c: Pos2, state: RingState, frac: f32, hovered: bool, t: f64) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let white = Color32::WHITE;
    p.circle_filled(c, RING_R, Color32::from_black_alpha(110));
    match state {
        RingState::Idle => play_glyph(p, c, 9.0, white),
        RingState::Queued => {
            // the dashed ring turning while it waits
            let spin = (t * 2.0) as f32 % TAU;
            for k in 0..12 {
                let a = spin + k as f32 * TAU / 12.0;
                p.add(egui::epaint::PathShape::line(arc(c, RING_R - 1.0, a, a + TAU / 24.0), Stroke::new(1.6, white)));
            }
            if frac > 0.0 {
                p.add(egui::epaint::PathShape::line(arc(c, RING_R - 1.0, -FRAC_PI_2, -FRAC_PI_2 + TAU * frac), Stroke::new(2.0, white.gamma_multiply(0.6))));
            }
            play_glyph(p, c, 9.0, white);
        }
        RingState::Playing | RingState::Stopping => {
            p.circle_stroke(c, RING_R - 1.0, Stroke::new(2.0, Color32::from_white_alpha(60)));
            p.add(egui::epaint::PathShape::line(arc(c, RING_R - 1.0, -FRAC_PI_2, -FRAC_PI_2 + TAU * frac.max(0.01)), Stroke::new(2.0, white)));
            let blink = state == RingState::Stopping && (t * 4.0) as i64 % 2 == 0;
            if hovered || state == RingState::Stopping {
                if !blink {
                    stop_glyph(p, c, 8.0, white);
                }
            } else {
                play_glyph(p, c, 9.0, white);
            }
        }
        RingState::Recording | RingState::RecQueued => {
            let red = look::record_red();
            if state == RingState::RecQueued {
                let spin = (t * 2.0) as f32 % TAU;
                for k in 0..12 {
                    let a = spin + k as f32 * TAU / 12.0;
                    p.add(egui::epaint::PathShape::line(arc(c, RING_R - 1.0, a, a + TAU / 24.0), Stroke::new(1.6, red)));
                }
            } else {
                p.circle_stroke(c, RING_R - 1.0, Stroke::new(2.0, red));
                p.add(egui::epaint::PathShape::line(arc(c, RING_R - 1.0, -FRAC_PI_2, -FRAC_PI_2 + TAU * frac.max(0.01)), Stroke::new(2.0, white)));
            }
            p.circle_filled(c, 4.0, red);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum RingState {
    Idle,
    Queued,
    Playing,
    Stopping,
    Recording,
    RecQueued,
}

/// A cell's mini notes or waveform.
fn draw_preview(p: &egui::Painter, r: Rect, c: &Cell, ink: Color32, samples: Option<&Arc<Vec<f32>>>) {
    if let Some(s) = samples {
        let w = r.width().max(1.0) as usize;
        let per = (s.len() / 2 / w).max(1);
        let mid = r.center().y;
        for k in 0..w {
            let from = k * per * 2;
            let peak = s.get(from..(from + per * 2).min(s.len())).map_or(0.0, |c| c.iter().fold(0f32, |m, x| m.max(x.abs())));
            let h = (peak * r.height() * 0.5).max(0.5);
            p.vline(r.min.x + k as f32, mid - h..=mid + h, Stroke::new(1.0, ink));
        }
        return;
    }
    if c.audio.is_some() {
        p.hline(r.x_range(), r.center().y, Stroke::new(1.0, ink));
        return;
    }
    let notes = sounding(c);
    if notes.is_empty() {
        return;
    }
    let lo = notes.iter().map(|n| n.pitch).min().unwrap_or(60) as f32;
    let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(72) as f32;
    let span = (hi - lo).max(12.0);
    let len = c.length.max(1) as f32;
    for n in &notes {
        let x0 = r.min.x + n.pos as f32 / len * r.width();
        let x1 = (x0 + (n.len as f32 / len * r.width()).max(1.5)).min(r.max.x);
        let y = r.max.y - (n.pitch as f32 - lo) / span * (r.height() - 2.0) - 1.0;
        p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.5, ink));
    }
}

/// The grid: `area` from the ruler's top to the bottom of the Tracks area, between the headers
/// and the timeline; rows line up with the track headers, whose first starts at `rows_top`
/// (scrolled by app.scroll_y, ROW_H each).
pub fn show_grid(app: &mut App, ui: &mut egui::Ui, area: Rect, rows_top: f32) {
    sync(app);
    if area.width() < 10.0 {
        return;
    }
    app.live_loops.grid_rect = Some(area);
    let now = ui.input(|i| i.time);
    let (pos, playing) = engine_pos(app);
    let ruler_h = rows_top.min(area.max.y) - area.min.y;
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, grid_bg());
    // focus: a press in the grid takes it, a press elsewhere gives it up
    if let Some(pp) = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None }) {
        app.live_loops.focused = area.contains(pp);
    }
    let cw = col_w(&app.live_loops);
    let pitch = cw + CELL_GAP;
    let cells_r = Rect::from_min_max(pos2(area.min.x + PAD, rows_top), pos2(area.max.x - STOP_W, area.max.y - TRIGGER_H));
    let stop_col = Rect::from_min_max(pos2(area.max.x - STOP_W, area.min.y), area.max);
    p.rect_filled(stop_col, 0.0, column_bg());
    p.vline(stop_col.min.x, area.y_range(), Stroke::new(1.0, look::line()));
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let visible_cols = ((cells_r.width() + CELL_GAP) / pitch).ceil().max(1.0) as usize;
    {
        let s = &mut app.live_loops;
        let needed = s.scenes.max(s.used_scenes());
        let max_scroll = (needed as f32 * pitch - cells_r.width()).max(0.0);
        s.scroll_x = s.scroll_x.clamp(0.0, max_scroll);
    }
    // horizontal scroll: Shift-wheel (or sideways) over the grid
    if ui.rect_contains_pointer(cells_r) {
        let (d, shift) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.shift));
        let dx = if shift { d.y } else { d.x };
        if dx != 0.0 {
            app.live_loops.scroll_x = (app.live_loops.scroll_x - dx).max(0.0);
        }
    }
    let sx = app.live_loops.scroll_x;
    let first_col = (sx / pitch).floor() as usize;
    let n_cols = (app.live_loops.scenes.max(app.live_loops.used_scenes())).max(first_col + visible_cols + 1);
    let col_x = |sc: usize| cells_r.min.x + sc as f32 * pitch - sx;
    // ---- header (over the ruler): Quantize Start, Performance Recording, horizontal zoom, ◁▷
    let head = Rect::from_min_max(area.min, pos2(area.max.x, area.min.y + ruler_h.min(32.0).max(0.0)));
    if head.height() > 16.0 {
        let hp = ui.painter_at(head);
        hp.rect_filled(Rect::from_min_max(head.min, pos2(stop_col.min.x, head.max.y)), 0.0, look::header_bg());
        let bh = (head.height() - 8.0).min(22.0);
        let mut x = head.min.x + 6.0;
        let room = stop_col.min.x - head.min.x;
        let buttons: [(&str, f32); 3] = [("q", 36.0), ("perf", 28.0), ("zoom", 36.0)];
        for (i, (kind, w)) in buttons.into_iter().enumerate() {
            if x + w > head.min.x + room - 4.0 {
                break;
            }
            let r = Rect::from_min_size(pos2(x, head.center().y - bh / 2.0), vec2(w, bh));
            x += w + 6.0;
            let id = ui.scope_id().with(("ll-head", i));
            let resp = ui.interact(r, id, Sense::click());
            let on = (kind == "perf" && app.live_loops.performance) || (kind == "zoom" && app.live_loops.wide);
            hp.rect_filled(r, 6.0, if on && kind == "zoom" { look::accent() } else if resp.hovered() { look::button_on() } else { button_bg() });
            let col = if on && kind == "zoom" { Color32::WHITE } else { look::text() };
            let c = r.center();
            match kind {
                "q" => {
                    for k in 0..3 {
                        let y = c.y - 4.0 + 4.0 * k as f32;
                        hp.hline(c.x - 10.0..=c.x + if k == 1 { 0.0 } else { 3.0 }, y, Stroke::new(1.6, col));
                    }
                    hp.text(pos2(c.x + 7.0, c.y + 2.0), Align2::CENTER_CENTER, "Q", look::semibold(9.0), col);
                    let resp = resp.on_hover_text(format!("Quantize Start: {}", app.live_loops.quantize));
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(180.0);
                        ui.label(egui::RichText::new("Quantize Start").weak());
                        for q in QUANTIZE_STARTS.iter().skip(2) {
                            if ui.add(egui::Button::selectable(app.live_loops.quantize == *q, *q)).clicked() {
                                app.live_loops.quantize = q.to_string();
                                ui.close();
                            }
                        }
                    });
                }
                "perf" => {
                    hp.circle_filled(c, 7.0, if on { look::record_red() } else { Color32::from_gray(30) });
                    if resp.on_hover_text("Enable Performance Recording (⌃P)").clicked() {
                        command(app, "Enable Performance Recording On/Off");
                    }
                }
                _ => {
                    hp.line_segment([pos2(c.x - 8.0, c.y), pos2(c.x + 8.0, c.y)], Stroke::new(1.5, col));
                    for s in [-1.0f32, 1.0] {
                        hp.line_segment([pos2(c.x + 8.0 * s, c.y), pos2(c.x + 4.5 * s, c.y - 3.5)], Stroke::new(1.5, col));
                        hp.line_segment([pos2(c.x + 8.0 * s, c.y), pos2(c.x + 4.5 * s, c.y + 3.5)], Stroke::new(1.5, col));
                    }
                    if resp.on_hover_text("Toggle Grid Horizontal Zoom (⌃⌥Z)").clicked() {
                        command(app, "Toggle Grid Horizontal Zoom");
                    }
                }
            }
        }
        // ◁▷ over the stop column: grid only / grid and tracks
        let sr = Rect::from_min_max(pos2(stop_col.min.x, head.min.y), pos2(stop_col.max.x, head.max.y));
        hp.rect_filled(sr, 0.0, look::header_bg());
        let c = sr.center();
        hp.add(Shape::closed_line(vec![pos2(c.x - 1.5, c.y - 4.0), pos2(c.x - 1.5, c.y + 4.0), pos2(c.x - 7.0, c.y)], Stroke::new(1.2, look::text())));
        hp.add(Shape::convex_polygon(vec![pos2(c.x + 1.5, c.y - 4.0), pos2(c.x + 7.0, c.y), pos2(c.x + 1.5, c.y + 4.0)], look::text(), Stroke::NONE));
        let resp = ui.interact(sr, ui.scope_id().with("ll-split"), Sense::click()).on_hover_text("Show/Hide Tracks area");
        if resp.clicked() {
            command(app, "Show/Hide Tracks area");
        }
        p.hline(area.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    }
    // ---- rows of cells
    let held = app.live_loops.drag;
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut drop_target = None;
    let alt = ui.input(|i| i.modifiers.alt);
    let cp = ui.painter_at(Rect::from_min_max(cells_r.min, pos2(cells_r.max.x, cells_r.max.y)));
    let audio_cache: HashMap<PathBuf, Option<Arc<Vec<f32>>>> = app.live_loops.audio.lock().map(|m| m.clone()).unwrap_or_default();
    for t in 0..n_tracks {
        // (rows as the tracks area draws them: their own heights, stacks and hidden tracks)
        let Some((y, row_h)) = crate::trackarea::row_span(app, t, rows_top) else { continue };
        if y + row_h < cells_r.min.y || y > cells_r.max.y {
            continue;
        }
        let tcol = track_color(app, t);
        for sc in first_col..n_cols {
            let x = col_x(sc);
            if x > cells_r.max.x {
                break;
            }
            let r = Rect::from_min_size(pos2(x, y + 2.0), vec2(cw, row_h - 4.0));
            let key = (t, sc);
            let id = ui.scope_id().with(("ll-cell", t, sc));
            let resp = ui.interact(r.intersect(cells_r), id, Sense::click_and_drag());
            let hovered = resp.hovered();
            let selected = app.live_loops.selected.contains(&key);
            let cell = app.live_loops.cells.get(&key).cloned();
            let play = app.live_loops.plays.get(&t).copied().unwrap_or_default();
            let rec = app.live_loops.rec.filter(|r| r.track == t && r.scene == sc);
            if pointer.is_some_and(|pp| r.contains(pp)) && (held.is_some() || app.live_loops.region_drag.is_some()) {
                drop_target = Some(key);
            }
            match &cell {
                None => {
                    cp.rect_filled(r, 4.0, if hovered { look::mix(empty_cell(), Color32::WHITE, 0.06) } else { empty_cell() });
                    let armed = app.armed.contains(&t);
                    if rec.is_some() {
                        let st = if playing && rec.is_some_and(|r| pos >= r.start as f64) { RingState::Recording } else { RingState::RecQueued };
                        let frac = rec.map_or(0.0, |r| ((pos - r.start as f64).max(0.0) / bar(app) as f64).fract() as f32);
                        draw_ring(&cp, r.center(), st, frac, false, now);
                    } else if selected && armed {
                        cp.circle_filled(r.center(), RING_R + 2.0, Color32::from_gray(30));
                        cp.circle_filled(r.center(), 4.5, look::record_red());
                    }
                }
                Some(c) => {
                    let active = play.current.is_some_and(|x| x.0 == sc);
                    let base = if app.live_loops.colors_off { Color32::from_gray(120) } else { tcol };
                    let base = if c.muted { Color32::from_gray(90) } else { base };
                    let fill = if active { base } else { look::mix(base, grid_bg(), 0.45) };
                    cp.rect_filled(r, 4.0, if hovered && !active { look::mix(fill, Color32::WHITE, 0.08) } else { fill });
                    // the name, then the notes (or waveform)
                    let name_r = Rect::from_min_size(r.min + vec2(4.0, 2.0), vec2(r.width() - 8.0, 13.0));
                    cp.with_clip_rect(name_r).text(name_r.left_center(), Align2::LEFT_CENTER, &c.name, look::semibold(10.0), Color32::WHITE);
                    let inner = Rect::from_min_max(pos2(r.min.x + 3.0, name_r.max.y + 1.0), pos2(r.max.x - 3.0, r.max.y - 3.0));
                    let ink = if active { look::mix(base, Color32::BLACK, 0.5) } else { look::mix(base, Color32::WHITE, 0.45) };
                    let samples = c.audio.as_ref().and_then(|a| audio_cache.get(a).cloned().flatten());
                    if c.audio.is_some() && samples.is_none() {
                        if let Some(a) = c.audio.clone() {
                            let _ = audio_samples(&mut app.live_loops, &a);
                        }
                    }
                    draw_preview(&cp, inner, c, ink, samples.as_ref());
                    // the ring
                    let queued = play.next.is_some_and(|n| n.0 == sc) || app.live_loops.pending.contains(&key);
                    let stopping = active && play.current.is_some_and(|x| x.2.is_some());
                    let state = if rec.is_some() {
                        if playing && rec.is_some_and(|r| pos >= r.start as f64) { RingState::Recording } else { RingState::RecQueued }
                    } else if stopping {
                        RingState::Stopping
                    } else if active && !queued {
                        RingState::Playing
                    } else if queued {
                        RingState::Queued
                    } else {
                        RingState::Idle
                    };
                    let frac = match (state, play.current) {
                        (RingState::Playing | RingState::Stopping, Some((_, st, _))) => progress(c, st, pos),
                        (RingState::Queued, _) if play.current.is_none() => play.frac,
                        (RingState::Recording, _) => rec.map_or(0.0, |r| ((pos - r.start as f64).max(0.0) / bar(app) as f64).fract() as f32),
                        _ => 0.0,
                    };
                    if state != RingState::Idle || hovered {
                        draw_ring(&cp, r.center(), state, frac, hovered, now);
                    }
                    if state != RingState::Idle {
                        ui.ctx().request_repaint();
                    }
                }
            }
            if selected {
                cp.rect_stroke(r, 4.0, Stroke::new(1.5, Color32::from_gray(200)), egui::StrokeKind::Inside);
            }
            if drop_target == Some(key) {
                cp.rect_stroke(r, 4.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
            }
            // renaming
            if let Some((Some(k), _, text)) = app.live_loops.renaming.as_mut() {
                if *k == key {
                    let te = ui.put(Rect::from_min_size(r.min + vec2(2.0, 2.0), vec2(r.width() - 4.0, 18.0)), egui::TextEdit::singleline(text).font(look::regular(11.0)));
                    crate::widgets::focus_once(ui, &te);
                    if te.lost_focus() {
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let new = text.trim().to_string();
                        app.live_loops.renaming = None;
                        if !cancel {
                            if let Some(c) = app.live_loops.cells.get_mut(&key) {
                                c.name = new;
                            }
                        }
                    }
                }
            }
            // clicks
            let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            if resp.clicked() {
                app.selected_track = Some(t);
                app.live_loops.focused_scene = Some(sc);
                let s = &mut app.live_loops;
                if shift || cmd {
                    if !s.selected.remove(&key) {
                        s.selected.insert(key);
                    }
                } else {
                    s.selected = [key].into();
                    let centre = resp.interact_pointer_pos().is_some_and(|pp| pp.distance(r.center()) <= RING_R + 4.0);
                    if cell.is_some() && alt {
                        command_at(app, "Queue Cell Playback", Some(key), None);
                    } else if cell.as_ref().is_some_and(|c| c.trigger != "Momentary") && centre {
                        trigger(app, t, sc, false);
                    } else if cell.is_none() && centre && app.armed.contains(&t) {
                        record_into_cell(app, t, sc);
                    }
                }
            }
            // Momentary: plays while held
            if cell.as_ref().is_some_and(|c| c.trigger == "Momentary") {
                if resp.is_pointer_button_down_on() && app.live_loops.momentary.is_none() {
                    app.live_loops.momentary = Some(key);
                    trigger(app, t, sc, true);
                }
            }
            if resp.secondary_clicked() {
                app.selected_track = Some(t);
                if !app.live_loops.selected.contains(&key) {
                    app.live_loops.selected = [key].into();
                }
            }
            if resp.double_clicked() && cell.is_none() {
                command_at(app, "Create MIDI Cell", Some(key), None);
            }
            if resp.drag_started() && cell.is_some() {
                app.live_loops.drag = Some(key);
            }
            context_menu(app, &resp, if cell.is_some() { "liveloops-midi-cell" } else { "liveloops-empty-cell" }, Some(key), None);
        }
        // the track's stop button
        let sb = Rect::from_center_size(pos2(stop_col.center().x, y + row_h / 2.0), vec2(14.0, 14.0));
        if sb.max.y < cells_r.max.y && sb.min.y > cells_r.min.y {
            let active = app.live_loops.active(t);
            let resp = ui.interact(sb.expand(3.0), ui.scope_id().with(("ll-stop", t)), Sense::click());
            if active {
                p.rect_filled(Rect::from_center_size(sb.center(), vec2(9.0, 9.0)), 1.0, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(199) });
                if resp.clicked() {
                    stop_track(app, t);
                }
            }
        }
        p.hline(stop_col.x_range(), y + row_h, Stroke::new(1.0, look::line().gamma_multiply(0.6)));
    }
    // momentary release
    if ui.input(|i| i.pointer.any_released()) {
        if let Some((t, _)) = app.live_loops.momentary.take() {
            stop_track(app, t);
        }
    }
    // a cell dragged onto another: moved (⌥: copied)
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(from) = app.live_loops.drag.take() {
            if let Some(to) = drop_target.filter(|k| *k != from) {
                let s = &mut app.live_loops;
                if let Some(c) = s.cells.get(&from).cloned() {
                    if !alt {
                        s.cells.remove(&from);
                        if let Some(pl) = s.plays.get_mut(&from.0) {
                            if pl.current.is_some_and(|x| x.0 == from.1) {
                                pl.current = None;
                            }
                        }
                    }
                    s.cells.insert(to, if alt { Cell { origin: None, ..c } } else { c });
                    s.selected = [to].into();
                    s.scenes = s.scenes.max(to.1 + 1);
                    app.dirty = true;
                }
            }
        }
    }
    region_drop(app, ui, drop_target);
    // ---- the scene triggers
    let trig = Rect::from_min_max(pos2(area.min.x, area.max.y - TRIGGER_H), pos2(stop_col.min.x, area.max.y));
    let tp = ui.painter_at(trig);
    tp.rect_filled(trig, 0.0, column_bg());
    p.hline(area.x_range(), trig.min.y, Stroke::new(1.0, look::line()));
    for sc in first_col..n_cols {
        let x = col_x(sc) - CELL_GAP / 2.0;
        if x > trig.max.x {
            break;
        }
        let r = Rect::from_min_size(pos2(x, trig.min.y), vec2(pitch, TRIGGER_H));
        let up = Rect::from_min_size(r.min, vec2(pitch, 25.0));
        let resp = ui.interact(r.intersect(trig), ui.scope_id().with(("ll-scene", sc)), Sense::click());
        let focused = app.live_loops.focused_scene == Some(sc);
        let playing_scene = (0..n_tracks).any(|t| app.live_loops.cells.contains_key(&(t, sc))) && (0..n_tracks).filter(|t| app.live_loops.cells.contains_key(&(*t, sc))).all(|t| app.live_loops.plays.get(&t).is_some_and(|p| p.current.is_some_and(|c| c.0 == sc)));
        if focused || resp.hovered() {
            tp.rect_filled(up, 0.0, if resp.hovered() { Color32::from_gray(52) } else { Color32::from_gray(44) });
        }
        tp.vline(r.max.x, trig.y_range(), Stroke::new(1.0, look::line().gamma_multiply(0.7)));
        let c = up.center();
        let col = if playing_scene { look::region_color(lg_core::TrackKind::Instrument) } else { look::text() };
        tp.add(egui::epaint::PathShape::line(vec![pos2(c.x - 7.0, c.y + 3.0), pos2(c.x, c.y - 3.0), pos2(c.x + 7.0, c.y + 3.0)], Stroke::new(1.8, col)));
        let label = app.live_loops.scene_opts.get(&sc).map(|s| s.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{}", sc + 1));
        match app.live_loops.renaming.as_mut() {
            Some((None, k, text)) if *k == sc => {
                let te = ui.put(Rect::from_min_size(pos2(r.min.x + 2.0, up.max.y), vec2(pitch - 4.0, 18.0)), egui::TextEdit::singleline(text).font(look::regular(11.0)));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let new = text.trim().to_string();
                    app.live_loops.renaming = None;
                    app.live_loops.scene_opts.entry(sc).or_default().name = new;
                }
            }
            _ => {
                tp.with_clip_rect(r).text(pos2(c.x, up.max.y + 9.0), Align2::CENTER_CENTER, label, look::regular(12.0), look::text());
            }
        }
        if resp.clicked() {
            trigger_scene(app, sc);
        }
        if resp.secondary_clicked() {
            app.live_loops.focused_scene = Some(sc);
        }
        context_menu(app, &resp, "liveloops-scene-trigger", None, Some(sc));
    }
    // Stop All Cells
    let sa = Rect::from_center_size(pos2(stop_col.center().x, trig.center().y + 3.0), vec2(9.0, 9.0));
    let resp = ui.interact(sa.expand(6.0), ui.scope_id().with("ll-stop-all"), Sense::click()).on_hover_text("Stop All Cells (⌘↩)");
    p.rect_filled(sa, 1.0, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(199) });
    if resp.clicked() {
        stop_all(app);
    }
    // the focus ring
    if app.live_loops.focused {
        ui.painter().rect_stroke(area.shrink(0.5), 2.0, Stroke::new(1.5, focus_blue()), egui::StrokeKind::Inside);
    }
    // ---- the divider to the timeline (grid and tracks): drag to resize
    if app.live_loops.view == View::Both {
        let edge = Rect::from_min_max(pos2(area.max.x - 3.0, area.min.y), pos2(area.max.x + 3.0, area.max.y));
        let e = ui.interact(edge, ui.scope_id().with("ll-edge"), Sense::drag());
        if e.hovered() || e.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if let (true, Some(pp)) = (e.dragged(), e.interact_pointer_pos()) {
            app.live_loops.grid_w = (pp.x - area.min.x).max(PAD * 2.0 + pitch + STOP_W);
        }
        ui.painter().vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    }
    let _ = pos;
}

/// A region dragged from the tracks onto a cell: the cell gets a copy, the region goes back.
fn region_drop(app: &mut App, ui: &egui::Ui, target: Option<(usize, usize)>) {
    match app.drag {
        Some(Drag::Region { track, region, start, .. }) => {
            if app.live_loops.region_drag.is_none() {
                app.live_loops.region_drag = Some((track, region, start));
            }
            // over the grid: the region stays where it was
            if ui.input(|i| i.pointer.hover_pos()).is_some_and(|pp| app.live_loops.grid_rect.is_some_and(|g| g.contains(pp))) {
                put_back(app, track, region, start);
                app.help_tag = Some(vec![("Copy to Cell".into(), String::new())]);
            }
        }
        _ => {}
    }
    if ui.input(|i| i.pointer.any_released()) {
        if let Some((track, region, start)) = app.live_loops.region_drag.take() {
            if let Some(to) = target {
                put_back(app, track, region, start);
                region_to_cell(app, track, region, to);
                app.load_engine();
            }
        }
    }
}

fn put_back(app: &mut App, track: usize, region: usize, start: u32) {
    let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(track)).and_then(|t| t.regions.get_mut(region)) else { return };
    let d = start as i64 - rg.start as i64;
    if d == 0 {
        return;
    }
    rg.start = start;
    for n in &mut rg.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pos: u32, pitch: u8) -> Note {
        Note { channel: 0, pitch, velocity: 100, pos, len: 240, muted: false }
    }

    fn cell(notes: Vec<Note>, length: u32) -> Cell {
        Cell { notes, length, ..Default::default() }
    }

    #[test]
    fn quantize_start_boundaries() {
        let bar = 3840;
        assert_eq!(quantize_ticks("1 Bar", bar), Some(3840));
        assert_eq!(quantize_ticks("4 Bars", bar), Some(4 * 3840));
        assert_eq!(quantize_ticks("1/4 Triplet (1/6)", bar), Some(640));
        assert_eq!(quantize_ticks("Off", bar), None);
        assert_eq!(quantize_ticks("Cell End", bar), Some(0));
        // the next bar after the playhead; on a bar line: now
        assert_eq!(next_boundary(100.0, Some(3840)), 3840);
        assert_eq!(next_boundary(3840.0, Some(3840)), 3840);
        assert_eq!(next_boundary(3841.0, Some(3840)), 7680);
        assert_eq!(next_boundary(1234.5, None), 1234);
        assert_eq!(QUANTIZE_STARTS.len(), 16);
    }

    #[test]
    fn cells_loop_from_their_start() {
        let c = cell(vec![note(0, 60), note(1920, 64)], 3840);
        // started at bar 3 (7680): two loops in [7680, 15360)
        let n = loop_notes(&c, 7680, None, 7680, 15360);
        assert_eq!(n.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(7680, 60), (9600, 64), (11520, 60), (13440, 64)]);
        // a window inside a later loop
        let n = loop_notes(&c, 7680, None, 12000, 14000);
        assert_eq!(n.iter().map(|x| x.0).collect::<Vec<_>>(), vec![13440]);
        // stopped at 11520: nothing after, and the last notes cut there
        let n = loop_notes(&c, 7680, Some(11520), 0, 20000);
        assert_eq!(n.len(), 2);
        // not looped: once
        let once = Cell { looped: false, ..c.clone() };
        assert_eq!(loop_notes(&once, 0, None, 0, 20000).len(), 2);
        // muted: silent
        assert!(loop_notes(&Cell { muted: true, ..c.clone() }, 0, None, 0, 20000).is_empty());
        // double speed: the loop takes half the time
        let fast = Cell { speed: 2.0, ..c.clone() };
        assert_eq!(loop_notes(&fast, 0, None, 0, 3840).iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 960, 1920, 2880]);
        // nothing before the start
        assert!(loop_notes(&c, 7680, None, 0, 7680).is_empty());
    }

    #[test]
    fn progress_ring_fills_through_the_loop() {
        let c = cell(vec![], 2 * 3840);
        assert_eq!(progress(&c, 3840, 3840.0), 0.0);
        assert!((progress(&c, 3840, 3840.0 + 3840.0) - 0.5).abs() < 1e-6);
        assert!((progress(&c, 3840, 3840.0 + 3.0 * 3840.0) - 0.5).abs() < 1e-6);
        assert_eq!(progress(&c, 3840, 0.0), 0.0);
    }

    #[test]
    fn transpose_reverse_and_quantize_as_it_plays() {
        let c = Cell { transpose: 12, ..cell(vec![note(10, 60)], 3840) };
        assert_eq!(sounding(&c)[0].pitch, 72);
        let r = Cell { reversed: true, ..cell(vec![note(0, 60)], 3840) };
        assert_eq!(sounding(&r)[0].pos, 3840 - 240);
        let q = Cell { quantize: "1/16 Note".into(), ..cell(vec![note(250, 60), note(470, 62)], 3840) };
        assert_eq!(sounding(&q).iter().map(|n| n.pos).collect::<Vec<_>>(), vec![240, 480]);
        assert_eq!(note_grid("1/16 Swing A"), Some((240, 54)));
        // swing moves the off-beats later
        assert_eq!(quantize_pos(240, 240, 75), 360);
        assert_eq!(quantize_pos(480, 240, 75), 480);
        assert_eq!(note_grid("Off"), None);
    }

    #[test]
    fn recorded_notes_become_a_cell() {
        let played = vec![note(3800, 60), note(3840 + 100, 62), note(3840 * 2 + 5, 64), note(3840 * 3, 65)];
        let (n, len) = cell_from_notes(&played, 3840, 3840 * 3, 3840);
        assert_eq!(n.iter().map(|n| (n.pos, n.pitch)).collect::<Vec<_>>(), vec![(100, 62), (3845, 64)]);
        assert_eq!(len, 2 * 3840);
    }

    #[test]
    fn regions_become_cells_from_their_start() {
        let r = lg_core::Region { name: "R".into(), start: 7680, length: 7680, notes: vec![note(7680, 60), note(9000, 62), note(20000, 64)], audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        let c = cell_of_region(&r);
        assert_eq!(c.notes.iter().map(|n| n.pos).collect::<Vec<_>>(), vec![0, 1320]);
        assert_eq!((c.length, c.name.as_str(), c.looped), (7680, "R", true));
    }

    #[test]
    fn scenes_insert_and_delete() {
        let mut s = State::default();
        s.cells.insert((0, 0), cell(vec![], 3840));
        s.cells.insert((0, 1), cell(vec![note(0, 61)], 3840));
        s.cells.insert((1, 2), cell(vec![], 3840));
        s.scenes = 3;
        insert_scene(&mut s, 1);
        assert_eq!(s.cells.keys().copied().collect::<Vec<_>>(), vec![(0, 0), (0, 2), (1, 3)]);
        assert_eq!(s.scenes, 5);
        delete_scene(&mut s, 0);
        assert_eq!(s.cells.keys().copied().collect::<Vec<_>>(), vec![(0, 1), (1, 2)]);
        assert_eq!(s.cells[&(0, 1)].notes[0].pitch, 61);
    }

    #[test]
    fn the_grid_keys_are_logics() {
        let names: Vec<&str> = crate::keymap::bindings().iter().filter(|b| b.area == "Live Loops Grid").map(|b| b.command.as_str()).collect();
        assert!(names.contains(&"Trigger Cells or Scene"));
        assert!(names.contains(&"Queue Cell/Scene Playback"));
        let l = crate::keymap::bindings().iter().find(|b| b.command == "Switch between Live Loops Grid and Tracks View").unwrap();
        assert!(l.key == egui::Key::L && !l.alt && !l.cmd);
        let stop = crate::keymap::bindings().iter().find(|b| b.command == "Stop All Cells").unwrap();
        assert!(stop.key == egui::Key::Enter && stop.cmd);
        // every item of the cell menus is either run here or greyed on purpose
        for place in ["liveloops-midi-cell", "liveloops-empty-cell", "liveloops-scene-trigger"] {
            assert!(!crate::menus::context(place).is_empty(), "{place}");
        }
    }
}
