//! Flex Time and Flex Pitch in Studio (Logic Pro 13), on lg-audio's `flex` and `flexpitch`.
//!
//! - Each audio track has Flex on or off and a Flex mode (Logic's pop-up: Flex Pitch, Flex
//!   Time - Monophonic, Slicing, Rhythmic, Polyphonic, Speed (FX), Tempophone (FX)); Flex
//!   On/Off (⌃F), the track header's flex button and mode pop-up while Show Flex Pitch/Time is
//!   on, the Audio Track Editor's mode pop-up. A track with flex markers plays them until Flex
//!   is switched off; switching it off keeps the markers (Logic's behaviour).
//! - Flex markers: Create Flex Marker At Playhead Position / Previous / Next Transient, Slice at
//!   Flex / Transient Markers, Quantize (Q) of audio by its transients, Time Stretch Region
//!   Length to Locators / Nearest Bar (as one marker at the region's end: Logic writes a new
//!   file), Move Selection with First Transient to Nearest Beat, the Trim Region Start / End to
//!   Previous / Next Transient commands, Rewind / Forward by Transient, Follow Tempo (the
//!   file's Smart Tempo beats follow the project's tempo).
//! - Flex Pitch: Analyze Audio for Flex Editing tracks the file's pitch on a thread; the Audio
//!   Track Editor shows the notes on a pitch grid with the pitch curve through them; drag a note
//!   (semitones), its hotspots (Pitch Drift at the top corners, Fine Pitch at the top middle,
//!   Gain bottom left, Vibrato bottom middle, Formant Shift bottom right), double-click for
//!   perfect pitch; Pitch Correction (amount, Chromatic or the project key); Create MIDI Track
//!   from Flex Pitch Data.
//! - Remove Silence from Audio Region… (Strip Silence): threshold, minimum time, pre-attack and
//!   post-release; the parts above it become regions.
//!
//! Playback: audioedit's `Plan` (the engine's audio region renders) carries the mode, the
//! markers (Follow Tempo's included) and the pitch edits; renders with flex are cached by plan.
//! Undo: track settings are snapshotted with the project's undo steps (as audioedit's extras).
//!
//! LG_RUN="Command|Command…" (checks): runs those commands by name a few frames in and prints
//! whether each ran.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_audio::flex::{Mode, TimeMap};
use lg_audio::flexpitch::{self, Note};
use lg_core::TrackKind;

use crate::app::App;
use crate::audioedit::{self, RegionAudio};
use crate::look;

const PPQ_F: f64 = lg_core::events::PPQ as f64;

/// The key commands and menu items this file runs (menus enable by them; the checklist counts
/// them). Also the audio editors' key commands (audioedit runs them; checked here by LG_RUN).
pub const COMMANDS: &[&str] = &[
    // Flex (Main Window Tracks, Tracks and Various Editors, the local menus)
    "Flex On/Off", "Show/Hide Flex Pitch/Time", "Show Flex Pitch/Time",
    "Flex Pitch", "Flex Time - Automatic (Polyphonic)", "Flex Time - Monophonic", "Flex Time - Slicing", "Flex Time - Rhythmic", "Flex Time - Polyphonic", "Flex Time - Speed (FX)", "Flex Time - Tempophone (FX)",
    "Create Flex Marker At Playhead Position", "Create Flex Marker At Previous Transient", "Create Flex Marker At Next Transient",
    "Slice at Flex Markers", "Slice at Transient Markers",
    "Time Stretch Region Length to Locators", "Time Stretch Region Length to Nearest Bar",
    "Move Selection with First Transient to Nearest Beat", "First Transient to Nearest Beat",
    "Trim Region Start to Previous Transient", "Trim Region Start to Next Transient", "Trim Region End to Previous Transient", "Trim Region End to Next Transient",
    "Region Start to Previous Transient", "Region Start to Next Transient", "Region End to Previous Transient", "Region End to Next Transient",
    "Rewind by Transient", "Forward by Transient",
    "Analyze Audio for Flex Editing", "Create MIDI Track from Flex Pitch Data",
    "Remove Silence from Audio Region…", "Remove Silence…",
    "Set Flex Tool",
    "Set All to Perfect Pitch", "Set All to Original Pitch",
    // the audio editors' groups (audioedit::command runs them in their tab)
    "Play/Stop All", "Play/Stop Region", "Play/Stop Region to Anchor", "Play/Stop Region from Anchor",
    "Create Backup", "Revert to Backup", "Save A Copy of Audio File As…", "Save Selection As…",
    "Region -> Selection", "Selection -> Region", "Sample Loop -> Selection", "Selection -> Sample Loop",
    "Go to Region Start", "Go to Region End", "Go to Region Anchor", "Go to Previous Transient", "Go to Next Transient",
    "Set Selection Start To Previous Transient", "Set Selection Start To Next Transients", "Set Selection End To Previous Transient", "Set Selection End To Next Transient",
    "Selection Start and End to Previous Transient", "Selection Start and End to Next Transient", "Selection Start and End to Previous Transient and Play", "Selection Start and End to Next Transient and Play",
    "Set Region Anchor to Previous Transient", "Set Region Anchor to Next Transient", "Set Region Anchor to Playhead", "Set Region Anchor to Region Start",
    "Toggle Transient Editing Mode", "Increase Number of Transients", "Decrease Number of Transients", "Detect Transients of Audio File",
    "Select All Previous Samples", "Select All Following Samples", "Create New Region",
    "Normalize", "Change Gain…", "Fade-In", "Fade-Out", "Silence", "Invert", "Reverse", "Trim", "Remove DC Offset", "Time and Pitch Machine…", "Search Peak", "Search Silence",
    "Samples", "Min:Sec:Ms", "SMPTE Time", "Bars/Beats", "Amplitude Percentage", "Amplitude Sample Value", "Lock Position in Track when moving Anchor",
    "Tap Tempo - Smart Tempo Editor", "Tap Tempo (Downbeat)", "Toggle Metronome Click - Smart Tempo Editor", "Scroll Left", "Scroll Right", "Select from Start to Playhead", "Select from Playhead to End",
    "Zoom All", "Center around Playhead", "Half Tempo", "Double Tempo", "Set Average Tempo for Selection", "Set Average Tempo within each Bar", "Extend First Tempo in Selection to Beginning", "Extend Last Tempo in Selection to End",
    "Set Downbeat to Playhead Position", "Trim Region To Selection", "Forward by Bar", "Rewind by Bar", "Forward by Beat", "Rewind by Beat", "Forward 8 Bars", "Rewind 8 Bars",
    "Snap To Transients", "Maintain Time Positions of All Regions", "Apply Region Tempo to Project Tempo", "Apply Project Tempo to Region and Downbeat", "Analyze Tempo", "Clear Hints", "Remove Tempo Information", "Revert to Original Recording Tempo", "Revert Changes",
    "Select Previous Audio File", "Select Next Audio File", "Add Region", "Toggle Snap Edits to Zero Crossings", "Show Files in Finder",
];

/// A track's Flex setting.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TrackFlex {
    pub on: bool,
    pub mode: Mode,
    /// The Track inspector's parameters of the Flex Time algorithms (Logic's defaults).
    pub params: lg_audio::flex::Params,
    /// Flex Pitch's: Formant Shift (semitones, added to the notes' own) and Formants (false:
    /// Process always, true: Keep Unvoiced Formants).
    pub formant_shift: f32,
    pub keep_unvoiced: bool,
}

/// A file's Flex Pitch analysis (the whole file).
pub struct Pitch {
    pub track: flexpitch::Track,
    pub notes: Vec<Note>,
}

#[derive(Clone, Copy, Debug)]
enum Hot {
    Body,
    Start,
    End,
    Fine,
    DriftIn,
    DriftOut,
    Gain,
    Vibrato,
    Formant,
}

#[derive(Clone, Debug)]
struct NoteDrag {
    t: usize,
    r: usize,
    note: usize,
    hot: Hot,
    orig: Note,
    y0: f32,
    row_h: f32,
    /// Moving or resizing in time: where the press was, destination frames a point, the
    /// markers before, (left bound, note start, note end, right bound) in source frames, the
    /// note's start and end as it played (destination frames), moved sideways yet.
    x0: f32,
    fpp: f64,
    flex: Vec<(usize, usize)>,
    span: (f64, f64, f64, f64),
    d: (f64, f64),
    moved: bool,
}

/// Remove Silence's settings (Logic's defaults).
#[derive(Clone, Debug)]
pub struct Silence {
    pub threshold_pct: f32,
    pub min_secs: f32,
    pub pre_secs: f32,
    pub post_secs: f32,
}

impl Default for Silence {
    fn default() -> Self {
        Silence { threshold_pct: 4.0, min_secs: 0.1, pre_secs: 0.02, post_secs: 0.1 }
    }
}

#[derive(Default)]
pub struct State {
    pub tracks: HashMap<usize, TrackFlex>,
    snaps: Vec<(usize, HashMap<usize, TrackFlex>)>,
    redo_snaps: Vec<(usize, HashMap<usize, TrackFlex>)>,
    /// Show Flex Pitch/Time in the tracks area.
    pub show: bool,
    pub pitch: HashMap<PathBuf, Arc<Pitch>>,
    pending: Vec<(PathBuf, mpsc::Receiver<Pitch>)>,
    /// Pitch Correction (0..100 %) and its scale (false: chromatic, true: the project key).
    pub correction: f32,
    pub correct_to_key: bool,
    drag: Option<NoteDrag>,
    /// A Tracks-area pitch bar being dragged: (track, region, note, the note before, press y).
    bar_drag: Option<(usize, usize, usize, Note, f32)>,
    /// The ATE Flex Pitch inspector: Time Quantize (index), Strength (None: 100), Scale
    /// Quantize's key and scale, a Gain drag running.
    pub time_q: usize,
    pub q_strength: Option<f32>,
    pub scale_root: u8,
    pub scale_minor: bool,
    gain_drag: bool,
    param_edit_at: f64,
    /// The Flex Pitch notes selected in the Audio Track Editor: (track, region, note).
    pub note_sel: Vec<(usize, usize, usize)>,
    /// Remove Silence's sheet (open) and its last values.
    pub silence_open: bool,
    pub silence: Silence,
    frames: u32,
    run: Option<VecDeque<String>>,
}

// ---------------------------------------------------------------------------------------------
// track settings and undo

/// A track's Flex: as set, else on when one of its regions has flex data.
pub fn track(app: &App, t: usize) -> TrackFlex {
    if let Some(f) = app.flex.tracks.get(&t) {
        return *f;
    }
    let on = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| {
        (0..tr.regions.len()).any(|r| app.audio_edit.regions.get(&(t, r)).is_some_and(|x| !x.flex.is_empty() || !x.pitch.is_empty() || x.follow))
    });
    TrackFlex { on, mode: Mode::Polyphonic, ..Default::default() }
}

/// Before an edit of flex settings, markers or the project: one undo step for all of it.
pub fn checkpoint(app: &mut App, what: &str) {
    let before = app.flex.tracks.clone();
    audioedit::checkpoint(app, what);
    let n = app.undo.len();
    app.flex.snaps.push((n, before));
    app.flex.redo_snaps.clear();
}

fn sync_undo(app: &mut App) {
    let n = app.undo.len();
    if app.redo.is_empty() {
        app.flex.redo_snaps.clear();
    }
    while app.flex.snaps.last().is_some_and(|s| s.0 > n) {
        let (k, before) = app.flex.snaps.pop().unwrap();
        let now = std::mem::replace(&mut app.flex.tracks, before);
        app.flex.redo_snaps.push((k, now));
    }
    while app.flex.redo_snaps.last().is_some_and(|s| s.0 <= n) {
        let (k, after) = app.flex.redo_snaps.pop().unwrap();
        let now = std::mem::replace(&mut app.flex.tracks, after);
        app.flex.snaps.push((k, now));
    }
}

pub fn set_mode(app: &mut App, t: usize, mode: Mode) {
    checkpoint(app, "Flex Mode");
    let was = track(app, t);
    app.flex.tracks.insert(t, TrackFlex { on: true, mode, ..was });
    freeze_source_only(app, t);
    app.after_edit();
}

/// The mode for every audio track (Shift-choosing it).
pub fn set_mode_all(app: &mut App, mode: Mode) {
    checkpoint(app, "Flex Mode");
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    for t in 0..n {
        if is_audio_track(app, t) {
            let was = track(app, t);
            app.flex.tracks.insert(t, TrackFlex { on: true, mode, ..was });
            freeze_source_only(app, t);
        }
    }
    app.after_edit();
}

/// Choosing a flex algorithm switches the track's Freeze Mode to Source Only (Logic's).
fn freeze_source_only(app: &mut App, t: usize) {
    if let Some(s) = crate::tm::set_mut(app, t) {
        s.freeze_mode = lg_core::trackmix::FreezeMode::SourceOnly;
    }
}

/// The track's Flex Time parameters and Flex Pitch's Formants choice, as the engine plays them.
pub fn params_for(app: &App, t: usize) -> (lg_audio::flex::Params, bool) {
    let f = track(app, t);
    (f.params, f.keep_unvoiced)
}

fn is_audio_track(app: &App, t: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == TrackKind::Audio || tr.regions.iter().any(|r| r.audio.is_some()))
}

/// Each frame: undo follows, analyses finish, LG_RUN.
pub fn update(app: &mut App) {
    sync_undo(app);
    let mut done = Vec::new();
    app.flex.pending.retain(|(path, rx)| match rx.try_recv() {
        Ok(p) => {
            done.push((path.clone(), p));
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for (path, p) in done {
        app.flex.pitch.insert(path, Arc::new(p));
        app.ctx.request_repaint();
    }
    if !app.flex.pending.is_empty() {
        app.ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    silence_sheet(app);
    // LG_RUN (checks)
    app.flex.frames += 1;
    if app.flex.frames == 1 {
        app.flex.run = std::env::var("LG_RUN").ok().map(|s| s.split('|').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
    }
    if app.flex.frames >= 5 {
        // one a frame (some wait for a thread: Analyze runs before the next)
        if let Some(name) = app.flex.run.as_mut().and_then(|q| q.pop_front()) {
            if name == "wait" {
                for _ in 0..200 {
                    if app.flex.pending.is_empty() && !tempo_busy(app) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    update_pending(app);
                }
            } else {
                let ctx = app.ctx.clone();
                let ok = audioedit::command(app, &name) || app.key_command(&ctx, &name);
                eprintln!("LG_RUN {name}: {}", if ok { "ran" } else { "not run" });
            }
        }
    }
}

/// A Smart Tempo analysis of one of the project's files is running.
fn tempo_busy(app: &App) -> bool {
    let Some(p) = &app.project else { return false };
    p.tracks.iter().flat_map(|t| t.regions.iter()).filter_map(|r| r.audio.as_ref()).any(|a| crate::smarttempo::analysing(app, &a.file))
}

/// LG_RUN still has commands to run (the snapshot waits).
pub fn busy(app: &App) -> bool {
    app.flex.frames < 5 || app.flex.run.as_ref().is_some_and(|q| !q.is_empty())
}

fn update_pending(app: &mut App) {
    crate::smarttempo::poll(app);
    let mut done = Vec::new();
    app.flex.pending.retain(|(path, rx)| match rx.try_recv() {
        Ok(p) => {
            done.push((path.clone(), p));
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for (path, p) in done {
        app.flex.pitch.insert(path, Arc::new(p));
    }
}

// ---------------------------------------------------------------------------------------------
// regions

/// An audio region and what's needed to place things in it.
struct Reg {
    t: usize,
    r: usize,
    rg: lg_core::Region,
    x: RegionAudio,
    file: PathBuf,
    rate: f64,
    start_sec: f64,
    /// Destination frames (as it plays).
    frames: f64,
    map: TimeMap,
}

impl Reg {
    fn tick_of(&self, app: &App, dst: f64) -> f64 {
        crate::smarttempo::tick_at(app, self.start_sec + dst / self.rate)
    }

    fn dst_of_tick(&self, app: &App, tick: f64) -> f64 {
        (app.seconds_at(tick) - self.start_sec) * self.rate
    }

    /// The region's source span in the file (frames from the file's start).
    fn src_end(&self) -> usize {
        self.x.offset + self.map.src(self.frames).max(0.0) as usize
    }
}

fn reg(app: &mut App, t: usize, r: usize) -> Option<Reg> {
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    let file = rg.audio.as_ref()?.file.clone();
    let rate = audioedit::load(app, &file)?.audio.rate as f64;
    let x = audioedit::extras(app, t, r);
    let start_sec = app.seconds_at(rg.start as f64);
    let frames = (app.seconds_at((rg.start + rg.length) as f64) - start_sec) * rate;
    let map = TimeMap::new(&x.flex);
    Some(Reg { t, r, rg, x, file, rate, start_sec, frames, map })
}

/// The audio regions a command works on: the selected ones (else the one under the playhead
/// on the selected track).
fn targets(app: &App) -> Vec<(usize, usize)> {
    let Some(p) = &app.project else { return Vec::new() };
    let is_audio = |&(t, r): &(usize, usize)| p.tracks.get(t).and_then(|tr| tr.regions.get(r)).is_some_and(|g| g.audio.is_some());
    let mut v: Vec<(usize, usize)> = app.selected_regions.iter().copied().chain(app.selected_region).filter(is_audio).collect();
    v.sort_unstable();
    v.dedup();
    if v.is_empty() {
        if let Some(t) = app.selected_track {
            let pos = app.position() as u32;
            if let Some(r) = p.tracks.get(t).and_then(|tr| tr.regions.iter().position(|g| g.audio.is_some() && g.start <= pos && pos < g.start + g.length)) {
                v.push((t, r));
            }
        }
    }
    v
}

/// The file's transients relative to the region's start (source frames), inside it.
fn region_transients(app: &mut App, g: &Reg) -> Vec<usize> {
    let end = g.src_end();
    audioedit::transients_of(app, &g.file).into_iter().filter(|&f| f > g.x.offset && f < end).map(|f| f - g.x.offset).collect()
}

fn stretch_reg(app: &mut App, g: Reg, len: f64) {
    let src_end = g.map.src(g.frames);
    let new_frames = (app.seconds_at(g.rg.start as f64 + len) - g.start_sec) * g.rate;
    let k = new_frames / g.frames.max(1.0);
    let mut x = g.x.clone();
    x.flex = x.flex.iter().filter(|m| (m.0 as f64) < src_end - 1.0).map(|m| (m.0, ((m.1 as f64) * k).round().max(1.0) as usize)).collect();
    x.flex.push((src_end.round() as usize, new_frames.round() as usize));
    audioedit::set_extras(app, g.t, g.r, x);
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(g.t)?.regions.get_mut(g.r)) {
        rg.length = len.round() as u32;
    }
    // the Time Stretching Algorithm on a track without Flex of its own (stretch.rs)
    if !app.flex.tracks.contains_key(&g.t) {
        let mode = crate::stretch::mode(&app.ctx);
        app.flex.tracks.insert(g.t, TrackFlex { on: true, mode, ..Default::default() });
    }
}

/// Time stretches an audio region to `len` ticks (no undo step of its own).
pub fn stretch_region(app: &mut App, t: usize, r: usize, len: u32) -> bool {
    let Some(g) = reg(app, t, r) else { return false };
    stretch_reg(app, g, len as f64);
    true
}

/// An audio region's transients as project ticks (groove templates take them as positions).
pub fn transient_ticks(app: &mut App, t: usize, r: usize) -> Vec<u32> {
    let Some(g) = reg(app, t, r) else { return Vec::new() };
    let trs = region_transients(app, &g);
    trs.into_iter().map(|f| {
        let d = g.map.dst(f as f64);
        crate::smarttempo::tick_at(app, g.start_sec + d / g.rate).max(0.0).round() as u32
    }).collect()
}

/// The Region inspector's Quantize on audio regions: Flex turned on where it is off, then the
/// audio quantized to the grid (ticks).
pub fn quantize_audio(app: &mut App, grid: u32) -> bool {
    let ts: Vec<(usize, usize)> = targets(app);
    if ts.is_empty() || grid == 0 {
        return false;
    }
    for &(t, _) in &ts {
        if !track(app, t).on {
            app.flex.tracks.insert(t, TrackFlex { on: true, mode: Mode::Polyphonic, ..Default::default() });
        }
    }
    let keep = app.quantize_grid;
    app.quantize_grid = grid;
    let ok = quantize(app);
    app.quantize_grid = keep;
    ok
}

/// A marker added at a source frame (where it plays now), unless one is there.
fn add_marker(x: &mut RegionAudio, map: &TimeMap, src: usize) -> bool {
    if src == 0 || x.flex.iter().any(|m| m.0.abs_diff(src) < 2) {
        return false;
    }
    let dst = map.dst(src as f64).round().max(1.0) as usize;
    x.flex.push((src, dst));
    x.flex.sort_by_key(|m| m.0);
    true
}

/// The part of a region from source frame `s0` to `s1` (relative to its start) as a region of
/// its own: start and length where that audio plays, the markers inside kept.
fn piece(app: &App, g: &Reg, s0: f64, s1: f64) -> (lg_core::Region, RegionAudio) {
    let (d0, d1) = (g.map.dst(s0), g.map.dst(s1));
    let t0 = g.tick_of(app, d0);
    let t1 = g.tick_of(app, d1);
    let mut rg = g.rg.clone();
    rg.start = t0.round().max(0.0) as u32;
    rg.length = (t1 - t0).round().max(1.0) as u32;
    let mut x = g.x.clone();
    x.offset = g.x.offset + s0.round() as usize;
    x.flex = g.x.flex.iter().filter(|m| (m.0 as f64) > s0 + 1.0 && (m.0 as f64) < s1).map(|m| ((m.0 as f64 - s0).round() as usize, (m.1 as f64 - d0).round().max(1.0) as usize)).collect();
    if s0 > 0.0 {
        x.fades.in_ms = 0;
    }
    if (s1 - g.map.src(g.frames)).abs() > 1.0 {
        x.fades.out_ms = 0;
    }
    (rg, x)
}

/// Replaces region `r` with `parts` (the first in its place, the others after the track's
/// regions, as Split does).
fn replace_with(app: &mut App, t: usize, r: usize, parts: Vec<(lg_core::Region, RegionAudio)>) {
    let mut it = parts.into_iter();
    let Some((first, fx)) = it.next() else { return };
    let mut extra = Vec::new();
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        if let Some(g) = tr.regions.get_mut(r) {
            *g = first;
        }
        for (rg, x) in it {
            let mut rg = rg;
            rg.source = None;
            rg.origin = tr.regions[r].source.clone().or(tr.regions[r].origin.clone());
            tr.regions.push(rg);
            extra.push((tr.regions.len() - 1, x));
        }
    }
    audioedit::set_extras(app, t, r, fx);
    for (k, x) in extra {
        audioedit::set_extras(app, t, k, x);
    }
}

// ---------------------------------------------------------------------------------------------
// playback

/// What the engine plays for an audio region: the mode, the markers (Follow Tempo's beats
/// included) and the pitch edits (frames of the file). Flex off: none.
pub fn plan_for(app: &App, t: usize, rg: &lg_core::Region, x: &RegionAudio) -> (Mode, Vec<(usize, usize)>, Vec<Note>) {
    let tf = track(app, t);
    if !tf.on {
        return (Mode::Polyphonic, Vec::new(), Vec::new());
    }
    let notes = if tf.mode != Mode::Pitch {
        Vec::new()
    } else if tf.formant_shift.abs() > 1e-3 {
        // (the track's Formant Shift: on every note)
        let file = rg.audio.as_ref().map(|a| a.file.clone()).unwrap_or_default();
        let mut v = region_notes(app, x, &file, usize::MAX);
        v.iter_mut().for_each(|n| n.formant_cents += tf.formant_shift * 100.0);
        v
    } else {
        x.pitch.iter().filter(|n| !n.is_neutral()).cloned().collect()
    };
    let mut flex = x.flex.clone();
    if x.follow {
        if let Some(f) = follow_anchors(app, rg, x) {
            flex = f;
        }
    }
    (tf.mode, flex, notes)
}

/// Follow Tempo: the file's beats (Smart Tempo) each played on the project's beat, the
/// region's markers carried along.
fn follow_anchors(app: &App, rg: &lg_core::Region, x: &RegionAudio) -> Option<Vec<(usize, usize)>> {
    let file = &rg.audio.as_ref()?.file;
    let map = &app.audio_edit.st.maps.get(file)?.map;
    let rate = audioedit::rate_of(app, file)? as f64;
    let user = TimeMap::new(&x.flex);
    let start_sec = app.seconds_at(rg.start as f64);
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let beat_t = PPQ_F * 4.0 / sig.1.max(1) as f64;
    let srcs: Vec<f64> = map.beats.iter().map(|b| b * rate - x.offset as f64).filter(|&s| s > 0.0).collect();
    let first = *srcs.first()?;
    // the first beat stays where it plays; the next ones a project beat apart
    let t0 = crate::smarttempo::tick_at(app, start_sec + user.dst(first) / rate);
    let pts: Vec<(f64, f64)> = srcs.iter().enumerate().map(|(i, &s)| (s, (app.seconds_at(t0 + i as f64 * beat_t) - start_sec) * rate)).collect();
    let follow = |u: f64| -> f64 {
        // u: where the source plays at the file's own speed; through the beats' points
        let mut prev = (0.0, 0.0);
        for &(s, d) in &pts {
            if u <= s {
                return if s > prev.0 { prev.1 + (u - prev.0) * (d - prev.1) / (s - prev.0) } else { d };
            }
            prev = (s, d);
        }
        let n = pts.len();
        let slope = if n >= 2 { (pts[n - 1].1 - pts[n - 2].1) / (pts[n - 1].0 - pts[n - 2].0).max(1.0) } else { 1.0 };
        prev.1 + (u - prev.0) * slope
    };
    let mut keys: Vec<f64> = srcs.clone();
    keys.extend(x.flex.iter().map(|m| m.0 as f64));
    keys.sort_by(f64::total_cmp);
    keys.dedup_by(|a, b| (*a - *b).abs() < 2.0);
    let mut out: Vec<(usize, usize)> = Vec::new();
    for s in keys {
        let d = follow(user.dst(s)).round();
        if d < 1.0 {
            continue;
        }
        let (s, d) = (s.round() as usize, d as usize);
        if out.last().is_none_or(|l| s > l.0 && d > l.1) {
            out.push((s, d));
        }
    }
    Some(out)
}

/// Renders with flex are kept (by plan, and the file's modification time): the engine loads
/// again on every edit, and the phase vocoder takes about a second a minute.
pub fn cached(plan: &audioedit::Plan, render: impl FnOnce() -> Option<Vec<f32>>) -> Option<Vec<f32>> {
    if plan.flex.is_empty() && plan.notes.is_empty() {
        return render();
    }
    static CACHE: OnceLock<Mutex<VecDeque<(String, Arc<Vec<f32>>)>>> = OnceLock::new();
    let stamp = std::fs::metadata(&plan.file).and_then(|m| m.modified()).ok();
    let key = format!("{plan:?}{stamp:?}");
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok().and_then(|c| c.iter().find(|e| e.0 == key).map(|e| e.1.clone())) {
        return Some(v.as_ref().clone());
    }
    let v = render()?;
    if let Ok(mut c) = cache.lock() {
        c.push_back((key, Arc::new(v.clone())));
        while c.len() > 12 {
            c.pop_front();
        }
    }
    Some(v)
}

// ---------------------------------------------------------------------------------------------
// Flex Pitch analysis

/// Starts the file's pitch analysis (a thread), unless it's there or running.
pub fn analyze(app: &mut App, file: &Path) {
    if app.flex.pitch.contains_key(file) || app.flex.pending.iter().any(|p| p.0 == file) {
        return;
    }
    let Some(l) = audioedit::load(app, file) else { return };
    let (data, c, rate) = (l.audio.data.clone(), l.audio.channels, l.audio.rate);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (track, notes) = flexpitch::analyze(&data, c, rate);
        let _ = tx.send(Pitch { track, notes });
    });
    app.flex.pending.push((file.to_path_buf(), rx));
}

pub fn analysing(app: &App, file: &Path) -> bool {
    app.flex.pending.iter().any(|p| p.0 == file)
}

/// The notes a region shows: its edited ones, else the analysis's inside it.
fn region_notes(app: &App, x: &RegionAudio, file: &Path, src_end: usize) -> Vec<Note> {
    if !x.pitch.is_empty() {
        return x.pitch.clone();
    }
    app.flex.pitch.get(file).map(|p| p.notes.iter().filter(|n| n.end > x.offset && n.start < src_end).cloned().collect()).unwrap_or_default()
}

/// The pitch analysis now (waiting for the thread, or running it here).
fn pitch_now(app: &mut App, file: &Path) -> Option<Arc<Pitch>> {
    analyze(app, file);
    for _ in 0..600 {
        if let Some(p) = app.flex.pitch.get(file) {
            return Some(p.clone());
        }
        update_pending(app);
        if !analysing(app, file) && !app.flex.pitch.contains_key(file) {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

/// Pitch Correction over a region's notes (the amount and scale as set).
fn apply_correction(app: &mut App, t: usize, r: usize) {
    let Some(g) = reg(app, t, r) else { return };
    let mut notes = region_notes(app, &g.x, &g.file, g.src_end());
    if notes.is_empty() {
        return;
    }
    let scale = if app.flex.correct_to_key { crate::pianoroll::scale_of(&app.project.as_ref().map_or("C major".to_string(), |p| p.key.clone())) } else { [true; 12] };
    flexpitch::correct(&mut notes, &scale, app.flex.correction / 100.0);
    let mut x = g.x;
    x.pitch = notes;
    audioedit::set_extras(app, t, r, x);
}

// ---------------------------------------------------------------------------------------------
// commands

/// Runs a Flex command by its Logic name; false when it isn't one or doesn't apply now.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let _ = ctx;
    match name {
        "Set All to Perfect Pitch" => return set_all(app, true),
        "Set All to Original Pitch" => return set_all(app, false),
        _ => {}
    }
    if let Some(mode) = Mode::from_label(name) {
        let Some(t) = app.selected_track.filter(|&t| is_audio_track(app, t)) else { return false };
        set_mode(app, t, mode);
        return true;
    }
    match name {
        "Flex On/Off" => {
            let mut ts = crate::trackarea::selection(app);
            ts.retain(|&t| is_audio_track(app, t));
            if ts.is_empty() {
                return false;
            }
            checkpoint(app, "Flex On/Off");
            let on = !track(app, ts[0]).on;
            for t in ts {
                let mut f = track(app, t);
                f.on = on;
                app.flex.tracks.insert(t, f);
            }
            app.after_edit();
            true
        }
        "Show/Hide Flex Pitch/Time" | "Show Flex Pitch/Time" => {
            app.flex.show = !app.flex.show;
            app.audio_edit.show_flex = app.flex.show;
            true
        }
        "Set Flex Tool" => {
            app.audio_edit.tool = crate::edit::Tool::Flex;
            app.tool = crate::edit::Tool::Flex;
            true
        }
        "Create Flex Marker At Playhead Position" | "Create Flex Marker At Previous Transient" | "Create Flex Marker At Next Transient" => {
            let pos = app.position();
            let Some((t, r)) = targets(app).into_iter().next() else { return false };
            let Some(g) = reg(app, t, r) else { return false };
            let d = g.dst_of_tick(app, pos);
            if d <= 0.0 || d >= g.frames {
                return false;
            }
            let here = g.map.src(d);
            let src = if name.ends_with("Playhead Position") {
                Some(here.round() as usize)
            } else {
                let trs = region_transients(app, &g);
                if name.contains("Previous") { trs.into_iter().rev().find(|&s| (s as f64) < here - 1.0) } else { trs.into_iter().find(|&s| (s as f64) > here + 1.0) }
            };
            let Some(src) = src else { return false };
            let mut x = g.x.clone();
            if !add_marker(&mut x, &g.map, src) {
                return false;
            }
            checkpoint(app, "Create Flex Marker");
            audioedit::set_extras(app, t, r, x);
            app.after_edit();
            true
        }
        "Slice at Flex Markers" | "Slice at Transient Markers" => {
            let ts = targets(app);
            if ts.is_empty() {
                return false;
            }
            let mut cuts_all = Vec::new();
            for &(t, r) in &ts {
                let Some(g) = reg(app, t, r) else { continue };
                let mut cuts: Vec<f64> = if name.contains("Flex") { g.x.flex.iter().map(|m| m.0 as f64).collect() } else { region_transients(app, &g).into_iter().map(|s| s as f64).collect() };
                let end = g.map.src(g.frames);
                cuts.retain(|&s| s > 1.0 && s < end - 1.0);
                if !cuts.is_empty() {
                    cuts_all.push((g, cuts));
                }
            }
            if cuts_all.is_empty() {
                return false;
            }
            checkpoint(app, name);
            for (g, cuts) in cuts_all {
                let end = g.map.src(g.frames);
                let mut bounds = vec![0.0];
                bounds.extend(cuts);
                bounds.push(end);
                let parts: Vec<_> = bounds.windows(2).map(|w| piece(app, &g, w[0], w[1])).collect();
                replace_with(app, g.t, g.r, parts);
            }
            app.after_edit();
            true
        }
        "Time Stretch Region Length to Locators" | "Time Stretch Region Length to Nearest Bar" => {
            let ts = targets(app);
            let bar = app.bar_ticks() as f64;
            let (a, b) = app.cycle;
            let mut todo = Vec::new();
            for (t, r) in ts {
                let Some(g) = reg(app, t, r) else { continue };
                let len = if name.ends_with("Locators") { (b.saturating_sub(a)) as f64 } else { ((g.rg.length as f64 / bar).round().max(1.0)) * bar };
                if len < 1.0 || (len - g.rg.length as f64).abs() < 1.0 {
                    continue;
                }
                todo.push((g, len));
            }
            if todo.is_empty() {
                return false;
            }
            checkpoint(app, "Time Stretch Region");
            for (g, len) in todo {
                stretch_reg(app, g, len);
            }
            app.after_edit();
            true
        }
        "Move Selection with First Transient to Nearest Beat" | "First Transient to Nearest Beat" => {
            let ts = targets(app);
            let Some(&(t, r)) = ts.first() else { return false };
            let Some(g) = reg(app, t, r) else { return false };
            let Some(&first) = region_transients(app, &g).first() else { return false };
            let tick = g.tick_of(app, g.map.dst(first as f64));
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            let beat = PPQ_F * 4.0 / sig.1.max(1) as f64;
            let delta = ((tick / beat).round() * beat - tick).round() as i64;
            if delta == 0 {
                return true;
            }
            checkpoint(app, "Move Region");
            if let Some(p) = app.project.as_mut() {
                for &(t, r) in &ts {
                    if let Some(rg) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                        rg.start = (rg.start as i64 + delta).max(0) as u32;
                    }
                }
            }
            app.after_edit();
            true
        }
        "Trim Region Start to Previous Transient" | "Trim Region Start to Next Transient" | "Region Start to Previous Transient" | "Region Start to Next Transient" | "Trim Region End to Previous Transient" | "Trim Region End to Next Transient" | "Region End to Previous Transient" | "Region End to Next Transient" => trim_to_transient(app, name.contains("Start"), name.contains("Next")),
        "Rewind by Transient" | "Forward by Transient" => {
            let pos = app.position();
            let mut ticks = Vec::new();
            let Some(p) = &app.project else { return false };
            let tracks: Vec<usize> = match app.selected_track {
                Some(t) if p.tracks.get(t).is_some_and(|tr| tr.regions.iter().any(|g| g.audio.is_some())) => vec![t],
                _ => (0..p.tracks.len()).collect(),
            };
            let regions: Vec<(usize, usize)> = tracks.iter().flat_map(|&t| p.tracks[t].regions.iter().enumerate().filter(|(_, g)| g.audio.is_some() && !g.muted).map(move |(r, _)| (t, r))).collect();
            for (t, r) in regions {
                let Some(g) = reg(app, t, r) else { continue };
                for s in region_transients(app, &g) {
                    ticks.push(g.tick_of(app, g.map.dst(s as f64)));
                }
            }
            ticks.sort_by(f64::total_cmp);
            let to = if name.starts_with("Forward") { ticks.into_iter().find(|&k| k > pos + 1.0) } else { ticks.into_iter().rev().find(|&k| k < pos - 1.0) };
            let Some(to) = to else { return false };
            app.locate(to);
            true
        }
        "Analyze Audio for Flex Editing" => {
            let ts = targets(app);
            if ts.is_empty() {
                return false;
            }
            for (t, r) in ts {
                let Some(file) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)?.audio.as_ref().map(|a| a.file.clone())) else { continue };
                let _ = audioedit::transients_of(app, &file);
                analyze(app, &file);
            }
            app.tell("Analyzing audio for Flex editing…");
            true
        }
        "Create MIDI Track from Flex Pitch Data" => {
            let Some((t, r)) = targets(app).into_iter().next() else { return false };
            let Some(g) = reg(app, t, r) else { return false };
            if pitch_now(app, &g.file).is_none() {
                return false;
            }
            let notes = region_notes(app, &g.x, &g.file, g.src_end());
            let notes: Vec<lg_core::events::Note> = flexpitch::to_midi(&notes).into_iter().filter_map(|(s, e, key, vel)| {
                let (s, e) = (s.max(g.x.offset) - g.x.offset, e.saturating_sub(g.x.offset));
                if e <= s {
                    return None;
                }
                let a = g.tick_of(app, g.map.dst(s as f64));
                let b = g.tick_of(app, g.map.dst(e as f64));
                Some(lg_core::events::Note { channel: 0, pitch: key, velocity: vel, pos: a.round().max(0.0) as u32, len: (b - a).round().max(1.0) as u32, muted: false })
            }).collect();
            if notes.is_empty() {
                app.tell("No Flex Pitch notes in the region");
                return true;
            }
            checkpoint(app, "Create MIDI Track from Flex Pitch Data");
            if let Some(p) = app.project.as_mut() {
                let row = p.tracks.len() as u32 + 1;
                let mut tr = lg_core::Track::new("Deluxe Classic", TrackKind::Instrument);
                tr.row = row;
                tr.regions.push(lg_core::Region { name: g.rg.name.clone(), start: g.rg.start, length: g.rg.length, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
                p.tracks.push(tr);
                app.programs.push(4);
                let nt = p.tracks.len() - 1;
                app.selected_track = Some(nt);
                app.selected_region = Some((nt, 0));
            }
            app.after_edit();
            true
        }
        "Remove Silence from Audio Region…" | "Remove Silence…" => {
            if targets(app).is_empty() {
                return false;
            }
            app.flex.silence_open = true;
            true
        }
        "Quantize Selected Regions/Cells/Events" | "Quantize Selected Events" | "Quantize" => quantize(app),
        _ => false,
    }
}

/// Quantize (Q) on audio regions of Flex tracks: their transients moved to the grid.
fn quantize(app: &mut App) -> bool {
    let grid = app.quantize_grid as f64;
    if grid <= 0.0 {
        return false;
    }
    let ts: Vec<(usize, usize)> = targets(app).into_iter().filter(|&(t, _)| track(app, t).on).collect();
    // (only when the selection is audio: MIDI quantizes as before)
    if ts.is_empty() || app.selected_region.is_some_and(|s| !ts.contains(&s)) {
        return false;
    }
    let mut todo = Vec::new();
    for (t, r) in ts {
        let Some(g) = reg(app, t, r) else { continue };
        let trs = region_transients(app, &g);
        let base = TimeMap::new(&[]);
        let start_tick = g.rg.start as f64;
        let start_sec = g.start_sec;
        let rate = g.rate;
        let to_tick = |d: f64| crate::smarttempo::tick_at(app, start_sec + d / rate);
        let to_dst = |tick: f64| (app.seconds_at(tick) - start_sec) * rate;
        let q = lg_audio::flex::quantize(&trs, &base, |d| to_dst(((to_tick(d) / grid).round() * grid).max(start_tick)), strength(app) as f64 / 100.0, (to_dst(start_tick + grid / 2.0)).max(1.0));
        todo.push((g, q));
    }
    checkpoint(app, "Quantize");
    for (g, q) in todo {
        let mut x = g.x;
        x.flex = q;
        x.flex_auto = x.flex.iter().map(|m| m.0).collect();
        audioedit::set_extras(app, g.t, g.r, x);
    }
    app.after_edit();
    true
}

fn trim_to_transient(app: &mut App, start: bool, next: bool) -> bool {
    let ts = targets(app);
    let mut todo = Vec::new();
    for (t, r) in ts {
        let Some(g) = reg(app, t, r) else { continue };
        let all = audioedit::transients_of(app, &g.file);
        let (s0, s1) = (g.x.offset, g.src_end());
        let pick = if start {
            if next { all.iter().copied().find(|&f| f > s0 && f < s1) } else { all.iter().copied().rev().find(|&f| f < s0) }
        } else if next {
            all.iter().copied().find(|&f| f > s1)
        } else {
            all.iter().copied().rev().find(|&f| f < s1 && f > s0)
        };
        if let Some(f) = pick {
            todo.push((g, f));
        }
    }
    if todo.is_empty() {
        return false;
    }
    checkpoint(app, if start { "Trim Region Start" } else { "Trim Region End" });
    for (g, f) in todo {
        let rel = f as f64 - g.x.offset as f64;
        let end_src = g.map.src(g.frames);
        let (rg, x) = if start { piece(app, &g, rel, end_src) } else { piece(app, &g, 0.0, rel) };
        // (before the region: at the file's speed)
        let (rg, x) = if start && rel < 0.0 {
            let mut rg2 = rg;
            let t0 = g.tick_of(app, rel);
            if t0 < 0.0 {
                continue;
            }
            rg2.start = t0.round() as u32;
            rg2.length = (g.rg.start + g.rg.length).saturating_sub(rg2.start).max(1);
            let mut x2 = x;
            x2.offset = f;
            x2.flex = g.x.flex.iter().map(|m| ((m.0 as f64 - rel) as usize, (m.1 as f64 - rel) as usize)).collect();
            (rg2, x2)
        } else {
            (rg, x)
        };
        if let Some(slot) = app.project.as_mut().and_then(|p| p.tracks.get_mut(g.t)?.regions.get_mut(g.r)) {
            *slot = rg;
        }
        audioedit::set_extras(app, g.t, g.r, x);
    }
    app.after_edit();
    true
}

// ---------------------------------------------------------------------------------------------
// Remove Silence

fn silence_sheet(app: &mut App) {
    if !app.flex.silence_open {
        return;
    }
    let ctx = app.ctx.clone();
    let mut open = true;
    let mut ok = false;
    let mut cancel = false;
    egui::Window::new("Remove Silence from Audio Region").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(&ctx, |ui| {
        let s = &mut app.flex.silence;
        egui::Grid::new("silence").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label("Threshold:");
            ui.add(egui::DragValue::new(&mut s.threshold_pct).range(0.1..=100.0).speed(0.1).suffix(" %"));
            ui.end_row();
            ui.label("Minimum Time to Accept as Silence:");
            ui.add(egui::DragValue::new(&mut s.min_secs).range(0.001..=10.0).speed(0.005).suffix(" s"));
            ui.end_row();
            ui.label("Pre Attack-Time:");
            ui.add(egui::DragValue::new(&mut s.pre_secs).range(0.0..=2.0).speed(0.001).suffix(" s"));
            ui.end_row();
            ui.label("Post Release-Time:");
            ui.add(egui::DragValue::new(&mut s.post_secs).range(0.0..=5.0).speed(0.005).suffix(" s"));
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            if ui.button("OK").clicked() {
                ok = true;
            }
        });
    });
    if ok {
        remove_silence(app);
    }
    if ok || cancel || !open {
        app.flex.silence_open = false;
    }
}

/// The parts of `mono` above the threshold: (start, end) frames, gaps shorter than `min` kept,
/// each widened by `pre` / `post`.
pub fn loud_parts(mono: &[f32], threshold: f32, min: usize, pre: usize, post: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < mono.len() {
        if mono[i].abs() >= threshold {
            let a = i;
            let mut last = i;
            while i < mono.len() && i - last < min.max(1) {
                if mono[i].abs() >= threshold {
                    last = i;
                }
                i += 1;
            }
            runs.push((a, last + 1));
        } else {
            i += 1;
        }
    }
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in runs {
        let (a, b) = (a.saturating_sub(pre), (b + post).min(mono.len()));
        match out.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn remove_silence(app: &mut App) {
    let ts = targets(app);
    let s = app.flex.silence.clone();
    let mut todo = Vec::new();
    for (t, r) in ts {
        let Some(g) = reg(app, t, r) else { continue };
        let Some(l) = audioedit::load(app, &g.file) else { continue };
        let a = &l.audio;
        let (s0, s1) = (g.x.offset.min(a.frames()), g.src_end().min(a.frames()));
        let c = a.channels.max(1);
        let mono: Vec<f32> = (s0..s1).map(|f| (0..c).map(|ch| a.data[f * c + ch].abs()).fold(0.0, f32::max)).collect();
        let parts = loud_parts(&mono, s.threshold_pct / 100.0, (s.min_secs as f64 * g.rate) as usize, (s.pre_secs as f64 * g.rate) as usize, (s.post_secs as f64 * g.rate) as usize);
        if parts.is_empty() || parts.len() == 1 && parts[0] == (0, mono.len()) {
            continue;
        }
        todo.push((g, parts));
    }
    if todo.is_empty() {
        app.tell("No silence found");
        return;
    }
    checkpoint(app, "Remove Silence");
    // the later regions first: each part after the first goes at the track's end
    for (g, parts) in todo {
        let pieces: Vec<_> = parts.iter().map(|&(a, b)| piece(app, &g, a as f64, b as f64)).collect();
        replace_with(app, g.t, g.r, pieces);
    }
    app.after_edit();
}

// ---------------------------------------------------------------------------------------------
// the tracks area: header and regions

/// The header's flex button and mode pop-up (Show Flex Pitch/Time on, audio tracks).
pub fn header(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    if !is_audio_track(app, t) {
        return;
    }
    let p = ui.painter().clone();
    let f = track(app, t);
    let br = Rect::from_min_size(r.min, vec2(22.0, r.height()));
    let resp = ui.interact(br, ui.scope_id().with(("flexon", t)), Sense::click()).on_hover_text("Flex On/Off");
    p.rect_filled(br, 4.0, if f.on { Color32::from_rgb(0x9b, 0x6b, 0xe8) } else { look::group_bg() });
    crate::glyphs::paint(&p, br, crate::glyphs::br::FLEX, 15.0, if f.on { Color32::WHITE } else { look::text_dim() });
    if resp.clicked() {
        checkpoint(app, "Flex On/Off");
        app.flex.tracks.insert(t, TrackFlex { on: !f.on, ..f });
        app.after_edit();
    }
    let mr = Rect::from_min_max(pos2(br.max.x + 4.0, r.min.y), r.max);
    let names: Vec<&str> = Mode::ALL.iter().map(|m| m.name()).collect();
    let k = Mode::ALL.iter().position(|m| *m == f.mode).unwrap_or(0);
    p.rect_filled(mr, 4.0, look::group_bg());
    if let Some(k) = crate::widgets::choice_slot(ui, mr, ui.scope_id().with(("flexmode", t)), k, &names) {
        // (Shift-choosing: every audio track)
        if ui.input(|i| i.modifiers.shift) {
            set_mode_all(app, Mode::ALL[k]);
        } else {
            set_mode(app, t, Mode::ALL[k]);
        }
    }
}

/// On an audio region in the tracks area (Show Flex on): its flex markers, and Flex Pitch
/// notes when the analysis is there.
pub fn draw_region(app: &App, p: &egui::Painter, rr: Rect, t: usize, r: usize) {
    if !app.flex.show {
        return;
    }
    let Some(rg) = app.project.as_ref().and_then(|pp| pp.tracks.get(t)?.regions.get(r)) else { return };
    let Some(a) = &rg.audio else { return };
    let Some(rate) = audioedit::rate_of(app, &a.file) else { return };
    let x = audioedit::extras(app, t, r);
    let secs = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
    let frames = secs * rate as f64;
    if frames <= 0.0 {
        return;
    }
    let tf = track(app, t);
    let col = Color32::from_rgb(0xf0, 0x9a, 0x2a).gamma_multiply(if tf.on { 1.0 } else { 0.4 });
    let x_of = |d: f64| rr.min.x + (d / frames) as f32 * rr.width();
    for m in &x.flex {
        let px = x_of(m.1 as f64);
        p.vline(px, rr.min.y + 15.0..=rr.max.y, Stroke::new(1.2, col));
    }
    if tf.mode == Mode::Pitch {
        let map = TimeMap::new(&x.flex);
        let src_end = x.offset + map.src(frames) as usize;
        let notes = region_notes(app, &x, &a.file, src_end);
        if notes.is_empty() {
            return;
        }
        // the pitch bars: from the zero line (perfect pitch), up when sharp, down when flat,
        // ±50 cents to the edges
        let inner = Rect::from_min_max(pos2(rr.min.x, rr.min.y + 16.0), pos2(rr.max.x, rr.max.y - 2.0));
        let (zero, half) = (inner.center().y, inner.height() / 2.0);
        p.hline(inner.x_range(), zero, Stroke::new(1.0, Color32::from_rgba_unmultiplied(0x20, 0x10, 0x40, 140)));
        let bar = Color32::from_rgba_unmultiplied(0x20, 0x10, 0x40, 200);
        for n in notes {
            let (s, e) = (n.start.saturating_sub(x.offset) as f64, n.end.saturating_sub(x.offset) as f64);
            let (x0, x1) = (x_of(map.dst(s)).max(rr.min.x), x_of(map.dst(e)).min(rr.max.x));
            if x1 <= x0 {
                continue;
            }
            let y = zero - deviation(&n) / 50.0 * half;
            let (a, b) = if (y - zero).abs() < 1.5 { (zero - 1.5, zero + 1.5) } else { (y.min(zero), y.max(zero)) };
            p.rect_filled(Rect::from_min_max(pos2(x0 + 0.5, a), pos2((x1 - 0.5).max(x0 + 1.0), b)), 1.0, bar);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the Audio Track Editor

/// The editor bar's Flex controls after the mode pop-up: Follow Tempo, and in Flex Pitch the
/// Pitch Correction amount and scale (as `room` allows). Returns where the next control goes.
pub fn editor_bar(app: &mut App, ui: &mut egui::Ui, at: Pos2, room: f32, t: usize, r: usize) -> f32 {
    let mut bx = at.x;
    if room < 96.0 {
        return bx;
    }
    let cy = at.y;
    let p = ui.painter().clone();
    let f = track(app, t);
    let id = ui.scope_id().with("flexbar");
    if f.mode == Mode::Pitch {
        let x = audioedit::extras(app, t, r);
        let file = x.file.clone();
        if !app.flex.pitch.contains_key(&file) {
            analyze(app, &file);
        }
        // Pitch Correction: the amount and the scale
        let sr = Rect::from_min_size(pos2(bx, cy - 12.0), vec2(150.0f32.min(room), 24.0));
        p.rect_filled(sr, 6.0, look::group_bg());
        let mut v = app.flex.correction;
        if crate::widgets::value_slot(ui, sr, id.with("corr"), &mut v, 0.0, 100.0, 0.0, 1.0, &|v| format!("Pitch Correction {v:.0}")) {
            app.flex.correction = v;
            let fresh = app.flex.drag.is_none();
            if fresh {
                checkpoint(app, "Pitch Correction");
            }
            apply_correction(app, t, r);
            app.after_edit();
        }
        bx += sr.width() + 6.0;
        if room < 280.0 {
            return bx;
        }
        let kr = Rect::from_min_size(pos2(bx, cy - 12.0), vec2(110.0, 24.0));
        p.rect_filled(kr, 6.0, look::group_bg());
        let choices = ["Chromatic", "Project Key"];
        if let Some(k) = crate::widgets::choice_slot(ui, kr, id.with("scale"), app.flex.correct_to_key as usize, &choices) {
            app.flex.correct_to_key = k == 1;
            if app.flex.correction > 0.0 {
                checkpoint(app, "Pitch Correction");
                apply_correction(app, t, r);
                app.after_edit();
            }
        }
        bx += 116.0;
        if analysing(app, &file) {
            p.text(pos2(bx, cy), Align2::LEFT_CENTER, "Analyzing…", look::regular(12.0), look::text_dim());
            bx += 80.0;
        }
    } else {
        // Follow Tempo (Logic's Region inspector "Flex & Follow")
        let x = audioedit::extras(app, t, r);
        let fr = Rect::from_min_size(pos2(bx, cy - 12.0), vec2(96.0, 24.0));
        let resp = ui.interact(fr, id.with("follow"), Sense::click()).on_hover_text("Flex & Follow: the region follows the project tempo (needs the file's Smart Tempo analysis)");
        p.rect_filled(fr, 6.0, if x.follow { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(fr.center(), Align2::CENTER_CENTER, "Follow Tempo", look::regular(12.0), if x.follow { Color32::WHITE } else { look::text() });
        if resp.clicked() {
            checkpoint(app, "Flex & Follow");
            let mut nx = x;
            nx.follow = !nx.follow;
            if nx.follow && !app.audio_edit.st.maps.contains_key(&nx.file) {
                app.tell("Follow Tempo: analyse the file in the Smart Tempo Editor first");
            }
            audioedit::set_extras(app, t, r, nx);
            if !track(app, t).on {
                app.flex.tracks.insert(t, TrackFlex { on: true, ..f });
            }
            app.after_edit();
        }
        bx += 102.0;
    }
    bx
}

/// Flex markers that play the source span [s0, s1) (source frames from the region's start) at
/// destination [d0, d1): the audio from `lo` to `s0` and from `s1` to `hi` stretched to fit,
/// `lo` and `hi` staying where they play. Markers between `lo` and `hi` give way.
pub fn place_span(flex: &[(usize, usize)], lo: f64, s0: f64, s1: f64, hi: f64, d0: f64, d1: f64) -> Vec<(usize, usize)> {
    let map = TimeMap::new(flex);
    let (dlo, dhi) = (map.dst(lo), map.dst(hi));
    let d0 = d0.clamp(dlo + 1.0, (dhi - 2.0).max(dlo + 1.0));
    let d1 = d1.clamp(d0 + 1.0, (dhi - 1.0).max(d0 + 1.0));
    let mut out: Vec<(usize, usize)> = flex.iter().copied().filter(|m| (m.0 as f64) <= lo + 1.0 || (m.0 as f64) >= hi - 1.0).collect();
    for (s, d) in [(lo, dlo), (s0, d0), (s1, d1), (hi, dhi)] {
        if s < 1.0 || out.iter().any(|m| (m.0 as f64 - s).abs() < 2.0) {
            continue;
        }
        out.push((s.round() as usize, d.round().max(1.0) as usize));
    }
    out.sort_by_key(|m| m.0);
    // (both columns rising)
    let mut v: Vec<(usize, usize)> = Vec::new();
    for m in out {
        if v.last().is_none_or(|l| m.0 > l.0 && m.1 > l.1) {
            v.push(m);
        }
    }
    v
}

/// Note `k` split at file frame `at`: two notes with its edits.
pub fn split_note(notes: &mut Vec<Note>, k: usize, at: usize) -> bool {
    let Some(n) = notes.get(k).cloned() else { return false };
    if at <= n.start + 64 || at + 64 >= n.end {
        return false;
    }
    let mut b = n.clone();
    b.start = at;
    notes[k].end = at;
    notes.insert(k + 1, b);
    true
}

/// The notes `ks` (indexes) made one: from the first's start to the last's end, the pitch and
/// edits of the longest.
pub fn join_notes(notes: &mut Vec<Note>, ks: &[usize]) -> bool {
    let mut ks: Vec<usize> = ks.iter().copied().filter(|&k| k < notes.len()).collect();
    ks.sort_unstable();
    ks.dedup();
    if ks.len() < 2 {
        return false;
    }
    let longest = ks.iter().copied().max_by_key(|&k| notes[k].end - notes[k].start).unwrap();
    let mut n = notes[longest].clone();
    n.start = ks.iter().map(|&k| notes[k].start).min().unwrap();
    n.end = ks.iter().map(|&k| notes[k].end).max().unwrap();
    n.level_db = ks.iter().map(|&k| notes[k].level_db).fold(f32::MIN, f32::max);
    for &k in ks.iter().rev() {
        notes.remove(k);
    }
    let at = notes.iter().position(|m| m.start > n.start).unwrap_or(notes.len());
    notes.insert(at, n);
    true
}

/// Set to Perfect Pitch: to the nearest semitone (the other edits kept).
pub fn perfect(n: &mut Note) {
    n.shift_cents = (n.target().round() - n.pitch) * 100.0;
}

/// Set to Original Pitch: as played.
pub fn original(n: &mut Note) {
    n.shift_cents = 0.0;
}

/// The region's notes, edited by `f` (one undo step).
fn edit_notes(app: &mut App, t: usize, r: usize, what: &str, f: impl FnOnce(&mut Vec<Note>) -> bool) -> bool {
    let Some(g) = reg(app, t, r) else { return false };
    let mut notes = region_notes(app, &g.x, &g.file, g.src_end());
    if notes.is_empty() {
        return false;
    }
    let before = notes.clone();
    if !f(&mut notes) || notes == before {
        return false;
    }
    checkpoint(app, what);
    let mut x = audioedit::extras(app, t, r);
    x.pitch = notes;
    audioedit::set_extras(app, t, r, x);
    app.after_edit();
    true
}

/// The Flex Pitch notes selected in the Audio Track Editor (their region and indexes).
fn note_sel(app: &App, t: usize, r: usize) -> Vec<usize> {
    app.flex.note_sel.iter().filter(|s| s.0 == t && s.1 == r).map(|s| s.2).collect()
}

/// Join (⌘J) with Flex Pitch notes selected in the Audio Track Editor: they become one note.
pub fn join_command(app: &mut App, name: &str) -> bool {
    if !matches!(name, "Join" | "Join Regions" | "Join Regions/Notes") || app.flex.note_sel.is_empty() || !app.audio_edit.show_flex {
        return false;
    }
    let (t, r) = (app.flex.note_sel[0].0, app.flex.note_sel[0].1);
    if app.selected_region != Some((t, r)) || track(app, t).mode != Mode::Pitch {
        return false;
    }
    let ks = note_sel(app, t, r);
    if edit_notes(app, t, r, "Join", |n| join_notes(n, &ks)) {
        app.flex.note_sel.clear();
    }
    true
}

/// The Flex Pitch notes over the Track editor's region (Flex shown, the track in Flex Pitch):
/// drawn and edited. `x_of`: the x of a destination frame (from the region's start). True when
/// a note took the pointer.
///
/// As Logic's: drag a note up or down for its pitch, sideways to move it in time (the audio
/// around it stretched by flex markers), its left or right edge to resize it; the hotspots: Pitch
/// Drift at the start (upper left) and end (upper right), Vibrato (upper middle), Gain (lower
/// left), Fine Pitch (lower middle), Formant Shift (lower right). The Scissors split a note, the
/// Join tool (or ⌘J) joins selected notes, the Vibrato and Volume tools drag a note's vibrato or
/// gain; double-click sets perfect pitch; the shortcut menu has Set to Perfect / Original Pitch.
pub fn pitch_view(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize, x_of: &dyn Fn(f64) -> f32) -> bool {
    if !app.audio_edit.show_flex || track(app, t).mode != Mode::Pitch {
        return false;
    }
    let Some(g) = reg(app, t, r) else { return false };
    let analysis = app.flex.pitch.get(&g.file).cloned();
    if analysis.is_none() {
        analyze(app, &g.file);
        ui.painter_at(body).text(body.center(), Align2::CENTER_CENTER, "Analyzing for Flex Pitch…", look::regular(13.0), Color32::from_white_alpha(200));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        return false;
    }
    let analysis = analysis.unwrap();
    let notes = region_notes(app, &g.x, &g.file, g.src_end());
    let p = ui.painter_at(body);
    // the pitch grid: the notes' range with room
    let lo = notes.iter().map(|n| n.target().min(n.pitch)).fold(f32::MAX, f32::min).floor() - 2.0;
    let hi = notes.iter().map(|n| n.target().max(n.pitch)).fold(f32::MIN, f32::max).ceil() + 2.0;
    let (lo, hi) = if notes.is_empty() { (55.0, 67.0) } else if hi - lo < 12.0 { let m = (hi + lo) / 2.0; (m - 6.0, m + 6.0) } else { (lo, hi) };
    let row_h = body.height() / (hi - lo);
    let y_of = |m: f32| body.max.y - (m - lo) * row_h;
    let names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    for k in lo.ceil() as i32..=hi.floor() as i32 {
        let y = y_of(k as f32);
        p.hline(body.x_range(), y, Stroke::new(1.0, Color32::from_white_alpha(if k % 12 == 0 { 40 } else { 14 })));
        if row_h > 9.0 {
            p.text(pos2(body.min.x + 3.0, y - row_h / 2.0), Align2::LEFT_CENTER, format!("{}{}", names[k.rem_euclid(12) as usize], k / 12 - 1), look::regular(9.5), Color32::from_white_alpha(90));
        }
    }
    let off = g.x.offset;
    let hop = analysis.track.hop;
    let note_h = (row_h * 0.85).clamp(6.0, 18.0);
    let hover = ui.input(|i| i.pointer.hover_pos());
    let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
    let tool = if cmd { app.audio_edit.cmd_tool } else { app.audio_edit.tool };
    // destination frames a point (x_of is linear)
    let fpp = 1000.0 / ((x_of(1000.0) - x_of(0.0)) as f64).max(1e-6);
    let dst_at = |x: f32| ((x - x_of(0.0)) as f64 * fpp).max(0.0);
    let src_len = g.src_end().saturating_sub(off) as f64;
    let selected = note_sel(app, t, r);
    let mut took = false;
    let accent = Color32::from_rgb(0x9b, 0x6b, 0xe8);
    for (k, n) in notes.iter().enumerate() {
        let (s, e) = (n.start.saturating_sub(off) as f64, n.end.saturating_sub(off) as f64);
        let (x0, x1) = (x_of(g.map.dst(s)), x_of(g.map.dst(e)));
        if x1 < body.min.x || x0 > body.max.x || x1 - x0 < 1.0 {
            continue;
        }
        let yc = y_of(n.target());
        let nr = Rect::from_min_max(pos2(x0, yc - note_h / 2.0), pos2(x1, yc + note_h / 2.0));
        let edited = !n.is_neutral();
        let sel = selected.contains(&k);
        p.rect_filled(nr, 3.0, if sel { accent } else if edited { accent.gamma_multiply(0.85) } else { accent.gamma_multiply(0.45) });
        p.rect_stroke(nr, 3.0, Stroke::new(1.0, if sel { Color32::WHITE } else { accent }), egui::StrokeKind::Inside);
        // the pitch curve as it will play
        let centre = n.pitch * 100.0;
        let mut pts = Vec::new();
        let (ka, kb) = (n.start / hop, (n.end / hop).min(analysis.track.f0.len()));
        let smooth = ((0.075 * analysis.track.rate as f64) as usize / hop).max(1);
        let step = ((kb - ka) / 200).max(1);
        let mut k2 = ka;
        while k2 < kb {
            let f = analysis.track.f0[k2];
            if f > 0.0 {
                let here = flexpitch::hz_to_midi(f) * 100.0;
                let v: Vec<f32> = (k2.saturating_sub(smooth).max(ka)..(k2 + smooth + 1).min(kb)).filter_map(|q| (analysis.track.f0[q] > 0.0).then(|| flexpitch::hz_to_midi(analysis.track.f0[q]) * 100.0)).collect();
                let slow = if v.is_empty() { centre } else { v.iter().sum::<f32>() / v.len() as f32 };
                let at = (k2 - ka) as f32 / (kb - ka).max(1) as f32;
                let new = centre + n.shift_cents + n.drift_at(at) * (slow - centre) + n.vibrato * (here - slow);
                let src = (k2 * hop).saturating_sub(off) as f64;
                pts.push(pos2(x_of(g.map.dst(src)), y_of(new / 100.0)));
            } else if pts.len() > 1 {
                p.add(egui::Shape::line(std::mem::take(&mut pts), Stroke::new(1.3, Color32::WHITE)));
            } else {
                pts.clear();
            }
            k2 += step;
        }
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, Stroke::new(1.3, Color32::WHITE)));
        }
        // hotspots (Logic's: drift at the top corners, vibrato top middle; gain, fine pitch and
        // formant shift along the bottom)
        let hots = [
            (Hot::DriftIn, pos2(nr.min.x + 4.0, nr.min.y)),
            (Hot::Vibrato, pos2(nr.center().x, nr.min.y)),
            (Hot::DriftOut, pos2(nr.max.x - 4.0, nr.min.y)),
            (Hot::Gain, pos2(nr.min.x + 4.0, nr.max.y)),
            (Hot::Fine, pos2(nr.center().x, nr.max.y)),
            (Hot::Formant, pos2(nr.max.x - 4.0, nr.max.y)),
        ];
        let id = ui.scope_id().with(("fpnote", t, r, k));
        let resp = ui.interact(nr.expand(4.0), id, Sense::click_and_drag());
        let over = hover.is_some_and(|h| nr.expand(5.0).contains(h));
        let pointer = tool == crate::edit::Tool::Pointer;
        if over && pointer && nr.width() > 24.0 {
            for (_, c) in hots {
                p.circle_filled(c, 3.0, Color32::WHITE);
                p.circle_stroke(c, 3.0, Stroke::new(1.0, accent));
            }
        }
        if resp.hovered() || resp.dragged() {
            took = true;
            let tip = format!("{}{}  {:+.0} ct", names[(n.target().round() as i32).rem_euclid(12) as usize], n.target().round() as i32 / 12 - 1, (n.target() - n.target().round()) * 100.0);
            app.help_tag = Some(vec![("Flex Pitch:".into(), tip)]);
            if let Some(h) = hover {
                let edge = pointer && h.y > nr.min.y + 3.0 && h.y < nr.max.y - 3.0 && ((h.x - nr.min.x).abs() < 4.0 || (h.x - nr.max.x).abs() < 4.0);
                ui.ctx().set_cursor_icon(match tool {
                    crate::edit::Tool::Scissors => egui::CursorIcon::Crosshair,
                    crate::edit::Tool::Vibrato | crate::edit::Tool::Volume => egui::CursorIcon::ResizeVertical,
                    _ if edge => egui::CursorIcon::ResizeHorizontal,
                    _ => egui::CursorIcon::Default,
                });
            }
        }
        // clicks: select (Shift adds), split, join
        if resp.clicked() {
            let pp = resp.interact_pointer_pos().unwrap_or(nr.center());
            match tool {
                crate::edit::Tool::Scissors => {
                    let at = off + g.map.src(dst_at(pp.x)).round().max(0.0) as usize;
                    edit_notes(app, t, r, "Split", |ns| split_note(ns, k, at));
                    app.flex.note_sel.clear();
                }
                crate::edit::Tool::Glue => {
                    let mut ks = selected.clone();
                    if !ks.contains(&k) {
                        ks = vec![k];
                        // (an unselected note joins the next one)
                        if k + 1 < notes.len() {
                            ks.push(k + 1);
                        }
                    }
                    edit_notes(app, t, r, "Join", |ns| join_notes(ns, &ks));
                    app.flex.note_sel.clear();
                }
                _ => {
                    if !shift {
                        app.flex.note_sel.clear();
                    }
                    if let Some(i) = app.flex.note_sel.iter().position(|s| *s == (t, r, k)) {
                        app.flex.note_sel.remove(i);
                    } else {
                        app.flex.note_sel.push((t, r, k));
                    }
                }
            }
        }
        resp.context_menu(|ui| {
            let ks = if selected.contains(&k) { selected.clone() } else { vec![k] };
            if ui.button("Set to Perfect Pitch").clicked() {
                edit_notes(app, t, r, "Set to Perfect Pitch", |ns| {
                    ks.iter().for_each(|&i| if let Some(n) = ns.get_mut(i) { perfect(n) });
                    true
                });
                ui.close();
            }
            if ui.button("Set to Original Pitch").clicked() {
                edit_notes(app, t, r, "Set to Original Pitch", |ns| {
                    ks.iter().for_each(|&i| if let Some(n) = ns.get_mut(i) { original(n) });
                    true
                });
                ui.close();
            }
        });
        if resp.drag_started() {
            let pp = ui.input(|i| i.pointer.press_origin()).unwrap_or(nr.center());
            let edge_l = (pp.x - nr.min.x).abs() < 4.0 && pp.y > nr.min.y + 3.0 && pp.y < nr.max.y - 3.0;
            let edge_r = (pp.x - nr.max.x).abs() < 4.0 && pp.y > nr.min.y + 3.0 && pp.y < nr.max.y - 3.0;
            let hot = match tool {
                crate::edit::Tool::Vibrato => Some(Hot::Vibrato),
                crate::edit::Tool::Volume => Some(Hot::Gain),
                crate::edit::Tool::Pointer | crate::edit::Tool::Pencil => Some(if nr.width() > 24.0 { hots.iter().find(|(_, c)| c.distance(pp) < 6.0).map(|h| h.0) } else { None }.unwrap_or(if edge_l { Hot::Start } else if edge_r { Hot::End } else { Hot::Body })),
                _ => None,
            };
            if let Some(hot) = hot {
                checkpoint(app, "Flex Pitch");
                let mut x = audioedit::extras(app, t, r);
                if x.pitch.is_empty() {
                    x.pitch = notes.clone();
                    audioedit::set_extras(app, t, r, x.clone());
                }
                // the stretch bounds: the neighbouring notes (else the region's ends)
                let lo_s = if k > 0 { notes[k - 1].end.saturating_sub(off) as f64 } else { 0.0 }.min(s);
                let hi_s = notes.get(k + 1).map_or(src_len, |m| m.start.saturating_sub(off) as f64).max(e);
                app.flex.drag = Some(NoteDrag { t, r, note: k, hot, orig: n.clone(), y0: pp.y, row_h, x0: pp.x, fpp, flex: x.flex.clone(), span: (lo_s, s, e, hi_s), d: (g.map.dst(s), g.map.dst(e)), moved: false });
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(pp)) = (app.flex.drag.clone(), resp.interact_pointer_pos()) {
                if d.t == t && d.r == r && d.note == k {
                    let dy = d.y0 - pp.y;
                    let dx = (pp.x - d.x0) as f64 * d.fpp;
                    let mut nn = d.orig.clone();
                    let mut flex = None;
                    let (lo_s, s0, s1, hi_s) = d.span;
                    match d.hot {
                        Hot::Body => {
                            nn.shift_cents = d.orig.shift_cents + (dy / d.row_h.max(1.0)).round() * 100.0;
                            // (sideways past a few points: moved in time)
                            if d.moved || (pp.x - d.x0).abs() > 4.0 {
                                if let Some(dd) = app.flex.drag.as_mut() {
                                    dd.moved = true;
                                }
                                flex = Some(place_span(&d.flex, lo_s, s0, s1, hi_s, d.d.0 + dx, d.d.1 + dx));
                            }
                        }
                        Hot::Start => flex = Some(place_span(&d.flex, lo_s, s0, s1, hi_s, d.d.0 + dx, d.d.1)),
                        Hot::End => flex = Some(place_span(&d.flex, lo_s, s0, s1, hi_s, d.d.0, d.d.1 + dx)),
                        Hot::Fine => nn.shift_cents = d.orig.shift_cents + dy.round(),
                        Hot::DriftIn => nn.drift = (d.orig.drift + dy / 60.0).clamp(0.0, 2.0),
                        Hot::DriftOut => nn.drift_out = (d.orig.drift_out + dy / 60.0).clamp(0.0, 2.0),
                        Hot::Vibrato => nn.vibrato = (d.orig.vibrato + dy / 60.0).clamp(0.0, 2.0),
                        Hot::Gain => nn.gain_db = (d.orig.gain_db + dy / 4.0).clamp(-30.0, 12.0).round(),
                        Hot::Formant => nn.formant_cents = (d.orig.formant_cents + dy * 5.0).clamp(-1200.0, 1200.0).round(),
                    }
                    app.help_tag = Some(vec![match d.hot {
                        Hot::Body | Hot::Fine => ("Pitch:".into(), format!("{:+.0} ct", nn.shift_cents)),
                        Hot::Start | Hot::End => ("Length:".into(), format!("{:.3} s", (s1 - s0) / g.rate)),
                        Hot::DriftIn | Hot::DriftOut => ("Pitch Drift:".into(), format!("{:.0} %", if matches!(d.hot, Hot::DriftIn) { nn.drift } else { nn.drift_out } * 100.0)),
                        Hot::Vibrato => ("Vibrato:".into(), format!("{:.0} %", nn.vibrato * 100.0)),
                        Hot::Gain => ("Gain:".into(), format!("{:+.0} dB", nn.gain_db)),
                        Hot::Formant => ("Formant Shift:".into(), format!("{:+.0} ct", nn.formant_cents)),
                    }]);
                    let mut x = audioedit::extras(app, t, r);
                    if let Some(slot) = x.pitch.get_mut(k) {
                        *slot = nn;
                    }
                    if let Some(f) = flex {
                        x.flex = f;
                    }
                    audioedit::set_extras(app, t, r, x);
                }
            }
        }
        if resp.drag_stopped() {
            app.flex.drag = None;
            app.after_edit();
        }
        if resp.double_clicked() && tool == crate::edit::Tool::Pointer {
            edit_notes(app, t, r, "Set to Perfect Pitch", |ns| ns.get_mut(k).is_some_and(|n| {
                perfect(n);
                true
            }));
        }
    }
    took
}

/// The audio regions on Flex Pitch tracks a region command works on.
fn pitch_targets(app: &App) -> Vec<(usize, usize)> {
    targets(app).into_iter().filter(|&(t, _)| track(app, t).on && track(app, t).mode == Mode::Pitch).collect()
}

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    match label {
        "Set All to Perfect Pitch" | "Set All to Original Pitch" => Some(!pitch_targets(app).is_empty()),
        _ => None,
    }
}

/// Set All to Perfect / Original Pitch on the selected regions.
fn set_all(app: &mut App, perfect_pitch: bool) -> bool {
    let what = if perfect_pitch { "Set All to Perfect Pitch" } else { "Set All to Original Pitch" };
    let ts = pitch_targets(app);
    let mut first = true;
    for (t, r) in ts {
        let Some(g) = reg(app, t, r) else { continue };
        if !app.flex.pitch.contains_key(&g.file) && pitch_now(app, &g.file).is_none() {
            continue;
        }
        let Some(g) = reg(app, t, r) else { continue };
        let mut notes = region_notes(app, &g.x, &g.file, g.src_end());
        for n in &mut notes {
            if perfect_pitch { perfect(n) } else { original(n) }
        }
        if first {
            checkpoint(app, what);
            first = false;
        }
        let mut x = audioedit::extras(app, t, r);
        x.pitch = notes;
        audioedit::set_extras(app, t, r, x);
    }
    if !first {
        app.after_edit();
    }
    true
}

/// The Tracks area's quick Flex Pitch edits (Show Flex on, a Flex Pitch track): each note's
/// pitch bar, from the region's zero line (perfect pitch) up when sharp, down when flat, ±50
/// cents to the edge. Drag a bar toward the line to tune the note within its semitone; drag one
/// on the line away to move it a semitone up or down; double-click for the original pitch; the
/// shortcut menu: Set to Perfect Pitch, Set to Original Pitch. (The other Flex Pitch edits are
/// in the Audio Track Editor.)
pub fn region_pitch_ui(app: &mut App, ui: &mut egui::Ui, rr: Rect, t: usize, r: usize) {
    if !app.flex.show || rr.height() < 24.0 {
        return;
    }
    let tf = track(app, t);
    if !tf.on || tf.mode != Mode::Pitch {
        return;
    }
    let Some(g) = reg(app, t, r) else { return };
    if g.frames <= 0.0 || !app.flex.pitch.contains_key(&g.file) {
        return;
    }
    let notes = region_notes(app, &g.x, &g.file, g.src_end());
    let inner = Rect::from_min_max(pos2(rr.min.x, rr.min.y + 16.0), pos2(rr.max.x, rr.max.y - 2.0));
    let half = inner.height() / 2.0;
    let x_of = |d: f64| rr.min.x + (d / g.frames) as f32 * rr.width();
    for (k, n) in notes.iter().enumerate() {
        let (s, e) = (n.start.saturating_sub(g.x.offset) as f64, n.end.saturating_sub(g.x.offset) as f64);
        let (x0, x1) = (x_of(g.map.dst(s)).max(rr.min.x), x_of(g.map.dst(e)).min(rr.max.x));
        if x1 - x0 < 2.0 {
            continue;
        }
        let hit = Rect::from_min_max(pos2(x0, inner.min.y), pos2(x1, inner.max.y));
        let resp = ui.interact(hit, ui.scope_id().with(("fpbar", t, r, k)), Sense::click_and_drag());
        if resp.hovered() {
            app.help_tag = Some(vec![("Flex Pitch:".into(), format!("{:+.0} ct", deviation(n)))]);
        }
        if resp.drag_started() {
            checkpoint(app, "Flex Pitch");
            let mut x = audioedit::extras(app, t, r);
            if x.pitch.is_empty() {
                x.pitch = notes.clone();
                audioedit::set_extras(app, t, r, x);
            }
            let y0 = ui.input(|i| i.pointer.press_origin()).map_or(inner.center().y, |p| p.y);
            app.flex.bar_drag = Some((t, r, k, n.clone(), y0));
        }
        if resp.dragged() {
            if let (Some((dt, dr, dk, orig, y0)), Some(pp)) = (app.flex.bar_drag.clone(), resp.interact_pointer_pos()) {
                if (dt, dr, dk) == (t, r, k) {
                    let nn = bar_drag(&orig, y0 - pp.y, half);
                    app.help_tag = Some(vec![("Flex Pitch:".into(), format!("{:+.0} ct", deviation(&nn)))]);
                    let mut x = audioedit::extras(app, t, r);
                    if let Some(slot) = x.pitch.get_mut(k) {
                        *slot = nn;
                    }
                    audioedit::set_extras(app, t, r, x);
                }
            }
        }
        if resp.drag_stopped() {
            app.flex.bar_drag = None;
            app.after_edit();
        }
        if resp.double_clicked() {
            edit_notes(app, t, r, "Set to Original Pitch", |ns| ns.get_mut(k).is_some_and(|n| {
                original(n);
                true
            }));
        }
        resp.context_menu(|ui| {
            if ui.button("Set to Perfect Pitch").clicked() {
                edit_notes(app, t, r, "Set to Perfect Pitch", |ns| ns.get_mut(k).is_some_and(|n| {
                    perfect(n);
                    true
                }));
                ui.close();
            }
            if ui.button("Set to Original Pitch").clicked() {
                edit_notes(app, t, r, "Set to Original Pitch", |ns| ns.get_mut(k).is_some_and(|n| {
                    original(n);
                    true
                }));
                ui.close();
            }
        });
    }
}

/// How far a note plays from its nearest semitone (cents; + sharp).
pub fn deviation(n: &Note) -> f32 {
    (n.target() - n.target().round()) * 100.0
}

/// A pitch bar dragged up by `dy` points (`half`: points from the zero line to the edge, 50
/// cents): a note off the line is tuned within its semitone (stopping on the line); a note on
/// the line moves a semitone each time the drag passes the edge.
pub fn bar_drag(orig: &Note, dy: f32, half: f32) -> Note {
    let mut n = orig.clone();
    let dev0 = deviation(orig);
    let cents = dy / half.max(1.0) * 50.0;
    if dev0.abs() < 3.0 {
        let steps = (cents / 50.0).trunc();
        n.shift_cents = orig.shift_cents - dev0 + steps * 100.0;
    } else {
        let mut new = (dev0 + cents).clamp(-49.0, 49.0);
        // (toward the line, it stops there)
        if new * dev0 < 0.0 {
            new = 0.0;
        }
        n.shift_cents = orig.shift_cents + (new - dev0);
    }
    n
}

/// Time Quantize's choices (Logic's ATE pop-up) and their grids (ticks).
const TIME_Q: [(&str, u32); 7] = [("Off", 0), ("1/1 Note", 3840), ("1/2 Note", 1920), ("1/4 Note", 960), ("1/8 Note", 480), ("1/16 Note", 240), ("1/32 Note", 120)];
const ROOTS: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/// Time quantize strength (percent; Logic's default 100).
pub fn strength(app: &App) -> f32 {
    app.flex.q_strength.unwrap_or(100.0)
}

/// The Audio Track Editor's inspector column for Flex Pitch (Logic's): Time Quantize and its
/// Strength, Scale Quantize (key, scale and its Q button), and Gain for the selected notes
/// (drag sideways).
pub fn pitch_inspector(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::group_bg().gamma_multiply(0.6));
    p.vline(area.max.x - 0.5, area.y_range(), Stroke::new(1.0, Color32::from_black_alpha(90)));
    let id = ui.scope_id().with(("fpinsp", t, r));
    let x0 = area.min.x + 8.0;
    let w = area.width() - 16.0;
    let mut y = area.min.y + 10.0;
    let label = |p: &egui::Painter, y: f32, s: &str| p.text(pos2(x0, y), Align2::LEFT_TOP, s, look::regular(11.0), look::text_dim());
    let slot = |y: f32, w: f32, dx: f32| Rect::from_min_size(pos2(x0 + dx, y), vec2(w, 20.0));
    // Time Quantize, Strength
    label(&p, y, "Time Quantize");
    y += 15.0;
    let tq = slot(y, w, 0.0);
    p.rect_filled(tq, 4.0, look::group_bg());
    if let Some(k) = crate::widgets::choice_slot(ui, tq, id.with("tq"), app.flex.time_q, &TIME_Q.map(|q| q.0)) {
        app.flex.time_q = k;
        if TIME_Q[k].1 > 0 {
            let keep = (app.selected_region, app.selected_regions.clone());
            app.selected_region = Some((t, r));
            app.selected_regions = vec![(t, r)];
            quantize_audio(app, TIME_Q[k].1);
            (app.selected_region, app.selected_regions) = keep;
        }
    }
    y += 26.0;
    label(&p, y, "Strength");
    y += 15.0;
    let sr = slot(y, w, 0.0);
    let mut v = strength(app);
    if hslider(ui, sr, id.with("str"), &mut v, 0.0, 100.0, &|v| format!("{v:.0}")) {
        app.flex.q_strength = Some(v.round());
    }
    y += 30.0;
    // Scale Quantize: key and scale, Q
    label(&p, y, "Scale Quantize");
    y += 15.0;
    let kr = slot(y, 44.0, 0.0);
    p.rect_filled(kr, 4.0, look::group_bg());
    if let Some(k) = crate::widgets::choice_slot(ui, kr, id.with("sqk"), app.flex.scale_root as usize, &ROOTS) {
        app.flex.scale_root = k as u8;
    }
    let mr = slot(y, w - 44.0 - 34.0, 48.0);
    p.rect_filled(mr, 4.0, look::group_bg());
    if let Some(k) = crate::widgets::choice_slot(ui, mr, id.with("sqm"), app.flex.scale_minor as usize, &["Major", "Minor"]) {
        app.flex.scale_minor = k == 1;
    }
    let qr = slot(y, 28.0, w - 28.0);
    let q = ui.interact(qr, id.with("sqq"), Sense::click()).on_hover_text("Scale Quantize the notes");
    p.rect_filled(qr, 4.0, if q.hovered() { look::button_on() } else { look::group_bg() });
    p.text(qr.center(), Align2::CENTER_CENTER, "Q", look::regular(12.0), look::text());
    if q.clicked() {
        let scale = flexpitch::key_scale(app.flex.scale_root, app.flex.scale_minor);
        edit_notes(app, t, r, "Scale Quantize", |ns| {
            flexpitch::correct(ns, &scale, 1.0);
            true
        });
    }
    y += 30.0;
    // Gain of the selected notes (dragged sideways)
    label(&p, y, "Gain");
    y += 15.0;
    let gr = slot(y, w, 0.0);
    let sel = note_sel(app, t, r);
    let mut g = {
        let x = audioedit::extras(app, t, r);
        sel.first().and_then(|&k| x.pitch.get(k)).map_or(0.0, |n| n.gain_db)
    };
    let resp_id = id.with("gain");
    if hslider(ui, gr, resp_id, &mut g, -30.0, 12.0, &|v| format!("{v:+.1} dB")) && !sel.is_empty() {
        if app.flex.drag.is_none() && ui.ctx().is_being_dragged(resp_id) && !app.flex.gain_drag {
            checkpoint(app, "Flex Pitch Gain");
            app.flex.gain_drag = true;
        }
        let mut x = audioedit::extras(app, t, r);
        for &k in &sel {
            if let Some(n) = x.pitch.get_mut(k) {
                n.gain_db = g;
            }
        }
        audioedit::set_extras(app, t, r, x);
    }
    if app.flex.gain_drag && !ui.ctx().is_being_dragged(resp_id) {
        app.flex.gain_drag = false;
        app.after_edit();
    }
}

/// A horizontal value bar: drag sideways (Shift: finer), Option-click resets to the middle.
fn hslider(ui: &mut egui::Ui, r: Rect, id: egui::Id, v: &mut f32, lo: f32, hi: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let resp = ui.interact(r, id, Sense::click_and_drag());
    // (Logic's: a thin track with a round thumb, the value to its right)
    let p = ui.painter();
    let f = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let track = Rect::from_min_max(pos2(r.min.x + 6.0, r.center().y - 1.5), pos2(r.max.x - 58.0, r.center().y + 1.5));
    p.rect_filled(track, 1.5, look::group_bg());
    let tx = track.min.x + track.width() * f;
    p.rect_filled(Rect::from_min_max(track.min, pos2(tx, track.max.y)), 1.5, look::accent());
    p.circle_filled(pos2(tx, r.center().y), 6.0, Color32::from_gray(0xe8));
    p.circle_stroke(pos2(tx, r.center().y), 6.0, Stroke::new(0.5, Color32::from_black_alpha(90)));
    p.text(pos2(r.max.x - 2.0, r.center().y), Align2::RIGHT_CENTER, shown(*v), look::regular(11.0), look::text());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let mut changed = false;
    if resp.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
        let nv = (*v + resp.drag_delta().x / r.width() * (hi - lo) * fine).clamp(lo, hi);
        if nv != *v {
            *v = nv;
            changed = true;
        }
    }
    changed
}

/// The Track inspector's Flex rows on an audio track (Logic's): Flex Mode, then the chosen
/// algorithm's parameters (Slicing: Fill Gaps, Decay, Slice Length; Rhythmic: Loop Length,
/// Decay, Loop Offset; Monophonic: Percussive; Polyphonic: Complex; Tempophone: Grain Size,
/// Crossfade; Flex Pitch: Formant Shift, Formants). Shift-choosing a mode sets every audio track.
pub fn inspector_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, t: usize) -> f32 {
    use crate::inspector::{label, slot_rect, ROW};
    if !is_audio_track(app, t) {
        return y;
    }
    let p = ui.painter().clone();
    let id = ui.scope_id().with(("trk-flex", t));
    let f = track(app, t);
    let names: Vec<&str> = std::iter::once("Off").chain(Mode::ALL.iter().map(|m| m.name())).collect();
    let cur = if f.on { Mode::ALL.iter().position(|m| *m == f.mode).map_or(0, |k| k + 1) } else { 0 };
    label(&p, area, y, "Flex Mode");
    if let Some(k) = crate::widgets::choice_slot(ui, slot_rect(area, y), id.with("mode"), cur, &names) {
        if k == 0 {
            checkpoint(app, "Flex On/Off");
            app.flex.tracks.insert(t, TrackFlex { on: false, ..f });
            app.after_edit();
        } else if ui.input(|i| i.modifiers.shift) {
            set_mode_all(app, Mode::ALL[k - 1]);
        } else {
            set_mode(app, t, Mode::ALL[k - 1]);
        }
    }
    y += ROW;
    if !f.on {
        return y;
    }
    let mut n = f;
    let check = |ui: &mut egui::Ui, y: &mut f32, key: &str, name: &str, v: &mut bool| {
        label(&p, area, *y, name);
        crate::widgets::check_slot(ui, slot_rect(area, *y), id.with(key), v);
        *y += ROW;
    };
    let num = |ui: &mut egui::Ui, y: &mut f32, key: &str, name: &str, v: &mut f32, lo: f32, hi: f32, def: f32, step: f32, shown: &dyn Fn(f32) -> String| {
        label(&p, area, *y, name);
        crate::widgets::value_slot(ui, slot_rect(area, *y), id.with(key), v, lo, hi, def, step, shown);
        *y += ROW;
    };
    let d = lg_audio::flex::Params::default();
    match f.mode {
        Mode::Slicing => {
            check(ui, &mut y, "fill", "Fill Gaps", &mut n.params.fill_gaps);
            num(ui, &mut y, "decay", "Decay", &mut n.params.decay, 0.0, 1.0, d.decay, 0.01, &|v| format!("{v:.2}"));
            num(ui, &mut y, "slen", "Slice Length", &mut n.params.slice_length, 0.0, 100.0, 100.0, 1.0, &|v| format!("{v:.0} %"));
        }
        Mode::Rhythmic => {
            num(ui, &mut y, "llen", "Loop Length", &mut n.params.loop_length, 0.0, 100.0, 100.0, 1.0, &|v| format!("{v:.0} %"));
            num(ui, &mut y, "ldecay", "Decay", &mut n.params.loop_decay, 0.0, 1.0, 0.0, 0.01, &|v| format!("{v:.2}"));
            num(ui, &mut y, "loff", "Loop Offset", &mut n.params.loop_offset_ms, 0.0, 100.0, 0.0, 1.0, &|v| format!("{v:.0} ms"));
        }
        Mode::Monophonic => check(ui, &mut y, "perc", "Percussive", &mut n.params.percussive),
        Mode::Polyphonic => check(ui, &mut y, "complex", "Complex", &mut n.params.complex),
        Mode::Tempophone => {
            num(ui, &mut y, "grain", "Grain Size", &mut n.params.grain_ms, 5.0, 500.0, d.grain_ms, 1.0, &|v| format!("{v:.0} ms"));
            num(ui, &mut y, "xfade", "Crossfade", &mut n.params.crossfade, 0.0, 1.0, d.crossfade, 0.01, &|v| format!("{v:.2}"));
        }
        Mode::Pitch => {
            num(ui, &mut y, "fshift", "Formant Shift", &mut n.formant_shift, -12.0, 12.0, 0.0, 1.0, &|v| format!("{v:+.0}"));
            label(&p, area, y, "Formants");
            if let Some(k) = crate::widgets::choice_slot(ui, slot_rect(area, y), id.with("formants"), n.keep_unvoiced as usize, &["Process always", "Keep Unvoiced Formants"]) {
                n.keep_unvoiced = k == 1;
            }
            y += ROW;
        }
        Mode::Speed => {}
    }
    if n != f {
        // (one undo step a drag: a new step only after a pause)
        let now = ui.input(|i| i.time);
        if now - app.flex.param_edit_at > 0.6 {
            checkpoint(app, "Flex Parameter");
        }
        app.flex.param_edit_at = now;
        app.flex.tracks.insert(t, n);
        app.after_edit();
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loud_parts_keep_short_gaps_and_pad() {
        let mut x = vec![0.0f32; 1000];
        for v in &mut x[100..200] {
            *v = 0.5;
        }
        for v in &mut x[210..300] {
            *v = 0.5;
        }
        for v in &mut x[600..700] {
            *v = 0.5;
        }
        // a 10-frame gap is kept (min 50), the 300-frame one cuts
        let p = loud_parts(&x, 0.1, 50, 5, 20);
        assert_eq!(p, vec![(95, 320), (595, 720)]);
        assert!(loud_parts(&vec![0.0; 100], 0.1, 10, 0, 0).is_empty());
    }

    #[test]
    fn plans_play_flex_time_and_flex_pitch() {
        let dir = std::env::temp_dir().join(format!("lg-flex-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        // a 220 Hz voice-like tone, one second
        let rate = 48_000;
        let data: Vec<f32> = (0..rate).map(|i| (1..=8).map(|h| 0.2 / h as f32 * (2.0 * std::f32::consts::PI * 220.0 * h as f32 * i as f32 / rate as f32).sin()).sum()).collect();
        let mut a = lg_audio::AudioFile::new(rate as u32, 1, data);
        a.bits = 24;
        a.write(&path).unwrap();
        let pitch_of = |s: &[f32]| {
            let l: Vec<f32> = s.chunks(2).map(|f| f[0]).collect();
            lg_audio::stretch::detect_pitch(&l[20_000..26_000], 48_000, 60.0, 1500.0).unwrap()
        };
        // stretched to 0.75 s → 1.0 s by a marker at the end: same pitch, longer
        let plan = audioedit::Plan { file: path.clone(), offset: 0, seconds: 1.0, gain_db: 0.0, fades: Default::default(), flex: vec![(36_000, 48_000)], reverse: false, mode: Mode::Polyphonic, notes: Vec::new(), params: Default::default(), keep_unvoiced: false };
        let out = plan.render().unwrap();
        assert_eq!(out.len(), 96_000);
        assert!((pitch_of(&out) - 220.0).abs() < 3.0, "{}", pitch_of(&out));
        // Flex Pitch: the note up a fifth
        let mut n = Note::new(0, 48_000, 57.0, -12.0);
        n.shift_cents = 700.0;
        let plan = audioedit::Plan { flex: Vec::new(), seconds: 0.9, mode: Mode::Pitch, notes: vec![n], ..plan };
        let out = plan.render().unwrap();
        assert!((pitch_of(&out) - 329.6).abs() < 4.0, "{}", pitch_of(&out));
        // (the second render comes from the cache, the same)
        assert_eq!(plan.render().unwrap(), out);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_moved_note_stretches_its_neighbours_only() {
        // a note from 1000 to 2000 (bounds 500 and 3000) moved 200 later
        let m = place_span(&[], 500.0, 1000.0, 2000.0, 3000.0, 1200.0, 2200.0);
        assert_eq!(m, vec![(500, 500), (1000, 1200), (2000, 2200), (3000, 3000)]);
        let map = TimeMap::new(&m);
        assert!((map.dst(1500.0) - 1700.0).abs() < 1.0);
        assert!((map.dst(4000.0) - 4000.0).abs() < 1.0);
        // resized: only the end moves; it can't pass the bound
        let m = place_span(&[], 500.0, 1000.0, 2000.0, 3000.0, 1000.0, 5000.0);
        assert_eq!(m[2], (2000, 2999));
        // an old marker inside gives way, one outside stays
        let m = place_span(&[(1500, 1500), (4000, 4100)], 500.0, 1000.0, 2000.0, 3000.0, 900.0, 1900.0);
        assert!(m.contains(&(4000, 4100)) && !m.iter().any(|x| x.0 == 1500));
    }

    #[test]
    fn notes_split_join_and_retune() {
        let mut ns = vec![Note::new(0, 1000, 60.0, -10.0), Note::new(1000, 3000, 62.3, -12.0)];
        ns[1].shift_cents = 20.0;
        assert!(split_note(&mut ns, 1, 2000));
        assert_eq!(ns.len(), 3);
        assert_eq!((ns[1].start, ns[1].end, ns[2].start, ns[2].end), (1000, 2000, 2000, 3000));
        assert_eq!(ns[2].shift_cents, 20.0);
        // (not at the very edge)
        assert!(!split_note(&mut ns, 0, 10));
        assert!(join_notes(&mut ns, &[0, 1]));
        assert_eq!(ns.len(), 2);
        assert_eq!((ns[0].start, ns[0].end), (0, 2000));
        assert!(!join_notes(&mut ns, &[1]));
        // 62.3 + 20 ct = 62.5: perfect → 63 (rounds half up), original → as played
        let mut n = ns[1].clone();
        perfect(&mut n);
        assert!((n.target() - 63.0).abs() < 1e-3);
        original(&mut n);
        assert_eq!(n.target(), 62.3);
    }

    #[test]
    fn pitch_bars_tune_within_the_semitone_or_step_off_the_line() {
        let mut n = Note::new(0, 1000, 60.0, -10.0);
        n.shift_cents = 30.0;
        assert!((deviation(&n) - 30.0).abs() < 1e-3);
        // dragged down toward the line: stops on it
        assert!(deviation(&bar_drag(&n, -40.0, 20.0)).abs() < 1e-3);
        assert!((deviation(&bar_drag(&n, -4.0, 20.0)) - 20.0).abs() < 1e-3);
        // on the line: a semitone each time the drag reaches the edge
        let mut on = n.clone();
        on.shift_cents = 0.0;
        assert_eq!(bar_drag(&on, 10.0, 20.0).target(), 60.0);
        assert_eq!(bar_drag(&on, 21.0, 20.0).target(), 61.0);
        assert_eq!(bar_drag(&on, -45.0, 20.0).target(), 58.0);
    }

    #[test]
    fn modes_by_logic_label() {
        assert_eq!(Mode::from_label("Flex Time - Slicing"), Some(Mode::Slicing));
        assert_eq!(Mode::from_label("Flex Time - Automatic (Polyphonic)"), Some(Mode::Polyphonic));
        assert_eq!(Mode::from_label("Flex Pitch"), Some(Mode::Pitch));
        // the popup's items are all commands here
        for m in Mode::ALL {
            assert!(COMMANDS.contains(&m.label()), "{}", m.label());
        }
    }
}
