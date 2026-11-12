//! Logic's MIDI editing commands by their Logic names (menu labels and key commands): the
//! Piano Roll's Edit / Functions / View menus and right-click menus, and the same functions on
//! the Tracks area's selected MIDI regions (Functions ▸ MIDI Transform, Delete MIDI Events,
//! Note End trims…). The note work itself is lg-core's (functions, transform); this file finds
//! the notes, makes the undo step and keeps the Piano Roll's selection. It also draws the MIDI
//! Transform window and the Repeat Multiple / Change Length / Copy MIDI Events sheets open from
//! here (midiwin.rs draws them).
//!
//! `App::key_command` asks here first; a name this file doesn't take (or a note command with no
//! MIDI region to work on) goes on to app.rs and edit.rs as before. The build and
//! tool/checklist.py read the names below as handled.

use lg_core::events::{Note, PPQ};
use lg_core::functions as f;
use lg_core::transform::{self, Cond, Rng, Transform};

use crate::app::{App, Bottom};
use crate::edit::Tool;

/// How the Piano Roll colours its notes (View ▸ Set Note Color).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteColor {
    #[default]
    Velocity,
    Region,
    Channel,
}

/// The sheets this file opens.
#[derive(Clone, Debug)]
pub enum Sheet {
    Repeat { copies: u32, adjust: usize },
    Length { ticks: u32 },
    Copy { mode: usize, left: u32, right: u32, dest: u32, copies: u32 },
}

/// The MIDI Transform window: the set being edited and the preset it came from.
#[derive(Clone, Debug)]
pub struct TransformWin {
    pub t: Transform,
    /// It was opened from the Piano Roll (works on its region's notes) or the tracks (the
    /// selected regions' notes).
    pub roll: bool,
    pub slot: usize,
}

/// The MIDI editing state (App::midi).
pub struct Midi {
    /// The Piano Roll has the key focus (it was clicked last).
    pub roll_focus: bool,
    /// A command is running from one of the Piano Roll's own menus.
    pub menu_roll: bool,
    /// Copied notes (song positions).
    pub clipboard: Vec<Note>,
    pub transform: Option<TransformWin>,
    pub user_presets: Vec<Option<Transform>>,
    pub sheet: Option<(Sheet, bool)>,
    pub rng: Rng,
    /// Piano Roll View and Functions: Time Handles, Collapse Mode, Note Labels, the note colours,
    /// the velocity lane, the scale guides.
    pub time_handles: bool,
    pub collapse: bool,
    pub note_labels: bool,
    pub note_color: NoteColor,
    pub velocity_lane: bool,
    pub scale_guides: bool,
    /// The Brush tool's pattern (positions from 0) and the Pencil's default note (length,
    /// velocity: Define as Default Note).
    pub brush: Vec<Note>,
    pub default_note: (u32, u8),
    /// Where the empty grid was right-clicked (tick, pitch), for Create Note.
    pub grid_click: Option<(u32, u8)>,
    /// The Piano Roll's tool and lane drags.
    pub roll_drag: Option<crate::pianoroll::RollDrag>,
}

impl Default for Midi {
    fn default() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64 | 1);
        Midi {
            roll_focus: false,
            menu_roll: false,
            clipboard: Vec::new(),
            transform: None,
            user_presets: load_user_presets(),
            sheet: None,
            rng: Rng(seed),
            time_handles: false,
            collapse: false,
            note_labels: false,
            note_color: NoteColor::Velocity,
            velocity_lane: false,
            scale_guides: false,
            brush: Vec::new(),
            default_note: (PPQ / 4, 80),
            grid_click: None,
            roll_drag: None,
        }
    }
}

fn presets_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("transform-presets.txt"))
}

/// The Transform window's 30 user presets (~/.config/bramble-studio/transform-presets.txt:
/// "slot<TAB>preset line").
fn load_user_presets() -> Vec<Option<Transform>> {
    let mut v = vec![None; 30];
    if std::env::var_os("LG_SNAP").is_some() {
        return v;
    }
    let text = presets_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    for line in text.lines() {
        if let Some((slot, rest)) = line.split_once('\t') {
            if let (Ok(k), Some(t)) = (slot.parse::<usize>(), Transform::from_line(rest)) {
                if (1..=30).contains(&k) {
                    v[k - 1] = Some(t);
                }
            }
        }
    }
    v
}

pub(crate) fn save_user_presets(v: &[Option<Transform>]) {
    let Some(p) = presets_path() else { return };
    let text: String = v.iter().enumerate().filter_map(|(k, t)| t.as_ref().map(|t| format!("{}\t{}\n", k + 1, t.to_line()))).collect();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, text);
}

/// What a note function sees of its region and the song.
pub struct Info {
    pub start: u32,
    pub length: u32,
    pub bar: u32,
    pub beat: u32,
    pub pos: u32,
    pub cycle: (u32, u32),
}

fn is_midi(app: &App, t: usize, r: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|rg| rg.audio.is_none())
}

/// The Piano Roll is the place for the command: open on a MIDI region and focused (clicked
/// last, under the pointer, or its own menu).
pub fn roll_ctx(app: &App) -> bool {
    app.selected_region.is_some_and(|(t, r)| is_midi(app, t, r)) && ((app.bottom == Bottom::PianoRoll && (app.midi.menu_roll || app.midi.roll_focus || app.editor_rect.contains(app.pointer))) || crate::evlist::focused(app))
}

/// Changes notes with `f`: in the Piano Roll its region's selected notes (all when none are
/// selected), else every note of the selected MIDI regions. One undo step. False when there's
/// no MIDI region to work on.
pub fn edit(app: &mut App, what: &str, mut f: impl FnMut(&mut Vec<Note>, &mut Vec<usize>, &Info)) -> bool {
    let roll = roll_ctx(app);
    let regions: Vec<(usize, usize)> = if roll { app.selected_region.into_iter().collect() } else { app.region_selection() };
    let regions: Vec<(usize, usize)> = regions.into_iter().filter(|&(t, r)| is_midi(app, t, r)).collect();
    if regions.is_empty() {
        return false;
    }
    app.checkpoint(what);
    let bar = app.bar_ticks();
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
    let (pos, cycle) = (app.position() as u32, app.cycle);
    let mut new_sel = None;
    for (t, r) in regions {
        let mut sel = if roll { app.selected_notes.clone() } else { Vec::new() };
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            let info = Info { start: rg.start, length: rg.length, bar, beat, pos, cycle };
            f(&mut rg.notes, &mut sel, &info);
            // notes put past the region's end make it longer (by bars), as Logic does
            if let Some(end) = rg.notes.iter().map(|n| n.pos + n.len).max() {
                if end > rg.start + rg.length {
                    rg.length = (end - rg.start).div_ceil(bar.max(1)) * bar.max(1);
                }
            }
        }
        if roll {
            new_sel = Some(sel);
        }
    }
    if let Some(s) = new_sel {
        app.selected_notes = s;
    }
    app.after_edit();
    true
}

/// A selection command (Piano Roll only).
fn select(app: &mut App, f: impl FnOnce(&[Note], &[usize], &Info) -> Vec<usize>) -> bool {
    if !roll_ctx(app) {
        return false;
    }
    let Some((t, r)) = app.selected_region else { return false };
    let bar = app.bar_ticks();
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
    let (pos, cycle) = (app.position() as u32, app.cycle);
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return false };
    let info = Info { start: rg.start, length: rg.length, bar, beat, pos, cycle };
    let mut v = f(&rg.notes, &app.selected_notes, &info);
    v.sort_unstable();
    v.dedup();
    app.selected_notes = v;
    true
}

/// The Piano Roll's region's notes (a copy).
pub fn roll_notes(app: &App) -> Option<(usize, usize, lg_core::Region)> {
    let (t, r) = app.selected_region?;
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    Some((t, r, rg))
}

const SET_TOOLS: [&str; 17] = [
    "Set Pointer Tool",
    "Set Pencil Tool",
    "Set Eraser Tool",
    "Set Scissors Tool",
    "Set Join Tool",
    "Set Text Tool",
    "Set Zoom Tool",
    "Set Solo Tool",
    "Set Mute Tool",
    "Set Quantize Tool",
    "Set Velocity Tool",
    "Set Automation Select Tool",
    "Set Automation Curve Tool",
    "Set Fade Tool",
    "Set Marquee Tool",
    "Set Flex Tool",
    "Set Brush Tool",
];

const CHANNEL_SET: [&str; 16] = [
    "Event Channel =  1",
    "Event Channel =  2",
    "Event Channel =  3",
    "Event Channel =  4",
    "Event Channel =  5",
    "Event Channel =  6",
    "Event Channel =  7",
    "Event Channel =  8",
    "Event Channel =  9",
    "Event Channel = 10",
    "Event Channel = 11",
    "Event Channel = 12",
    "Event Channel = 13",
    "Event Channel = 14",
    "Event Channel = 15",
    "Event Channel = 16",
];

const USER_APPLY: [&str; 30] = [
    "Apply Transform User Preset  1 to selected Events",
    "Apply Transform User Preset  2 to selected Events",
    "Apply Transform User Preset  3 to selected Events",
    "Apply Transform User Preset  4 to selected Events",
    "Apply Transform User Preset  5 to selected Events",
    "Apply Transform User Preset  6 to selected Events",
    "Apply Transform User Preset  7 to selected Events",
    "Apply Transform User Preset  8 to selected Events",
    "Apply Transform User Preset  9 to selected Events",
    "Apply Transform User Preset 10 to selected Events",
    "Apply Transform User Preset 11 to selected Events",
    "Apply Transform User Preset 12 to selected Events",
    "Apply Transform User Preset 13 to selected Events",
    "Apply Transform User Preset 14 to selected Events",
    "Apply Transform User Preset 15 to selected Events",
    "Apply Transform User Preset 16 to selected Events",
    "Apply Transform User Preset 17 to selected Events",
    "Apply Transform User Preset 18 to selected Events",
    "Apply Transform User Preset 19 to selected Events",
    "Apply Transform User Preset 20 to selected Events",
    "Apply Transform User Preset 21 to selected Events",
    "Apply Transform User Preset 22 to selected Events",
    "Apply Transform User Preset 23 to selected Events",
    "Apply Transform User Preset 24 to selected Events",
    "Apply Transform User Preset 25 to selected Events",
    "Apply Transform User Preset 26 to selected Events",
    "Apply Transform User Preset 27 to selected Events",
    "Apply Transform User Preset 28 to selected Events",
    "Apply Transform User Preset 29 to selected Events",
    "Apply Transform User Preset 30 to selected Events",
];

const USER_SELECT: [&str; 30] = [
    "Select And Operate using Transform User Preset  1",
    "Select And Operate using Transform User Preset  2",
    "Select And Operate using Transform User Preset  3",
    "Select And Operate using Transform User Preset  4",
    "Select And Operate using Transform User Preset  5",
    "Select And Operate using Transform User Preset  6",
    "Select And Operate using Transform User Preset  7",
    "Select And Operate using Transform User Preset  8",
    "Select And Operate using Transform User Preset  9",
    "Select And Operate using Transform User Preset 10",
    "Select And Operate using Transform User Preset 11",
    "Select And Operate using Transform User Preset 12",
    "Select And Operate using Transform User Preset 13",
    "Select And Operate using Transform User Preset 14",
    "Select And Operate using Transform User Preset 15",
    "Select And Operate using Transform User Preset 16",
    "Select And Operate using Transform User Preset 17",
    "Select And Operate using Transform User Preset 18",
    "Select And Operate using Transform User Preset 19",
    "Select And Operate using Transform User Preset 20",
    "Select And Operate using Transform User Preset 21",
    "Select And Operate using Transform User Preset 22",
    "Select And Operate using Transform User Preset 23",
    "Select And Operate using Transform User Preset 24",
    "Select And Operate using Transform User Preset 25",
    "Select And Operate using Transform User Preset 26",
    "Select And Operate using Transform User Preset 27",
    "Select And Operate using Transform User Preset 28",
    "Select And Operate using Transform User Preset 29",
    "Select And Operate using Transform User Preset 30",
];

/// Functions ▸ MIDI Transform: each opens the MIDI Transform window on its preset (lg-core's
/// transform::PRESETS; Scale 14bit PitchBd needs pitch bend, which regions don't keep yet).
const TRANSFORM_MENU: [&str; 17] = [
    "Fixed Velocity",
    "Random Pitch",
    "Random Velocity",
    "Random Pitch, Velocity, Length",
    "Crescendo",
    "Double Speed",
    "Half Speed",
    "Humanize",
    "Reverse Position",
    "Reverse Pitch",
    "Transposition",
    "Exponential Velocity",
    "Velocity Limiter",
    "Fixed Note Length",
    "Maximum Note Length",
    "Minimum Note Length",
    "Quantize Note Length",
];

/// The Quantize key commands' values (Logic's names after "Quantize ").
const QUANTIZE_KEYS: [&str; 18] = [
    "Quantize Off",
    "Quantize 1/1 Note",
    "Quantize 1/2 Note",
    "Quantize 1/4 Note",
    "Quantize 1/8 Note",
    "Quantize 1/16 Note",
    "Quantize 1/32 Note",
    "Quantize 1/64 Note",
    "Quantize 1/2 Triplet (1/3)",
    "Quantize 1/4 Triplet (1/6)",
    "Quantize 1/8 Triplet (1/12)",
    "Quantize 1/16 Triplet (1/24)",
    "Quantize 1/32 Triplet (1/48)",
    "Quantize 1/64 Triplet (1/96)",
    "Quantize 1/128 Triplet (1/192)",
    "Quantize 1/16 & 1/16 Triplet",
    "Quantize 1/16 & 1/8 Triplet",
    "Quantize 1/8 & 1/8 Triplet",
];

/// Time Quantize on the notes: the Piano Roll's value, Strength and Swing (1/16 when Off).
fn quantize_notes(app: &mut App, grids: Option<Vec<u32>>) -> bool {
    let (g, strength, swing) = (if app.roll_quantize > 0 { app.roll_quantize } else { PPQ / 4 }, app.roll_strength, app.roll_swing);
    edit(app, "Quantize", |notes, sel, i| match &grids {
        Some(gs) if gs.len() > 1 => f::quantize_multi(notes, sel, i.start, gs),
        Some(gs) if gs.len() == 1 => f::quantize(notes, sel, i.start, gs[0], strength, swing),
        Some(_) => {}
        None => f::quantize(notes, sel, i.start, g, strength, swing),
    })
}

fn open_transform(app: &mut App, t: Transform) {
    let roll = roll_ctx(app);
    app.midi.transform = Some(TransformWin { t, roll, slot: 1 });
}

/// Runs a user preset (1-based slot): on the selected notes, or Select and Operate.
fn run_user(app: &mut App, slot: usize, select_first: bool) -> bool {
    let Some(t) = app.midi.user_presets.get(slot - 1).cloned().flatten() else {
        app.tell(format!("Transform user preset {slot} is empty: save one in the MIDI Transform window"));
        return true;
    };
    run_transform(app, &t, if select_first { 2 } else { 1 })
}

/// How many regions the Transform window works on, and how many of their notes match.
pub(crate) fn transform_count(app: &App, t: &Transform) -> (usize, usize) {
    let regions: Vec<(usize, usize)> = if roll_ctx(app) { app.selected_region.into_iter().collect() } else { app.region_selection() };
    let Some(p) = &app.project else { return (0, 0) };
    let mut n = 0;
    let mut k = 0;
    for (tt, r) in regions {
        if let Some(rg) = p.tracks.get(tt).and_then(|x| x.regions.get(r)).filter(|g| g.audio.is_none()) {
            k += 1;
            n += t.select(&rg.notes, rg.start).len();
        }
    }
    (k, n)
}

/// The Transform window's buttons: 0 Select Only, 1 Operate Only, 2 Select and Operate.
pub(crate) fn run_transform(app: &mut App, t: &Transform, how: u8) -> bool {
    let mut tb = t.clone();
    tb.bar = app.bar_ticks();
    let t = &tb;
    if how == 0 {
        return select(app, |notes, _, i| t.select(notes, i.start));
    }
    let mut rng = app.midi.rng.clone();
    let roll = roll_ctx(app);
    let ok = edit(app, &t.name, |notes, sel, i| {
        if how == 2 {
            let s = t.select(notes, i.start);
            if s.is_empty() {
                sel.clear();
                return;
            }
            *sel = s;
        } else if !roll {
            sel.clear();
        }
        // the Mode's deleting and copying ways (lg-core transform apply)
        if t.mode != 0 {
            let mut tt = t.clone();
            tt.bar = i.bar;
            let scope = sel.clone();
            *sel = tt.apply(notes, &scope, i.start, &mut rng);
            return;
        }
        let o = transform::origin_for(t, notes, sel, i.start);
        let scope = sel.clone();
        // (Position conditions count from the region's start, the operations from `o`)
        if o == i.start {
            t.operate(notes, &scope, o, &mut rng);
        } else {
            let chosen: Vec<usize> = f::targets(notes, &scope).into_iter().filter(|&k| t.matches(&notes[k], i.start)).collect();
            let mut ops_only = t.clone();
            ops_only.conds = [Cond::All; 5];
            if !chosen.is_empty() {
                ops_only.operate(notes, &chosen, o, &mut rng);
            }
        }
    });
    app.midi.rng = rng;
    ok
}

/// Runs a MIDI command by its Logic name; false when it isn't one (or there's nothing to do it
/// on), so the caller tries its own.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let roll = roll_ctx(app);
    if let Some(k) = SET_TOOLS.iter().position(|n| *n == name) {
        return app.edit_command(ctx, &SET_TOOLS[k][4..]);
    }
    if let Some(k) = CHANNEL_SET.iter().position(|n| *n == name) {
        return edit(app, "Event Channel", |notes, sel, _| f::change_channel(notes, sel, Some(k as u8), 0));
    }
    if let Some(k) = USER_APPLY.iter().position(|n| *n == name) {
        return run_user(app, k + 1, false);
    }
    if let Some(k) = USER_SELECT.iter().position(|n| *n == name) {
        return run_user(app, k + 1, true);
    }
    if TRANSFORM_MENU.contains(&name) {
        if let Some(p) = transform::preset(name) {
            open_transform(app, p);
            return true;
        }
    }
    if let Some(k) = QUANTIZE_KEYS.iter().position(|n| *n == name) {
        let grids = f::quantize_value(&QUANTIZE_KEYS[k]["Quantize ".len()..]).unwrap_or_default();
        if roll {
            app.roll_quantize = grids.first().copied().unwrap_or(0);
            if !grids.is_empty() && !app.selected_notes.is_empty() {
                return quantize_notes(app, Some(grids));
            }
            return true;
        }
        // the Tracks area: the selected regions' Quantize parameter (single values)
        if grids.len() > 1 || app.region_selection().is_empty() {
            return false;
        }
        let g = grids.first().copied().unwrap_or(0);
        let sel = app.region_selection();
        app.checkpoint("Quantize");
        if let Some(p) = app.project.as_mut() {
            for (t, r) in sel {
                if let Some(rg) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                    rg.props.quantize = g;
                }
            }
        }
        app.after_edit();
        return true;
    }
    match name {
        // ---- Edit: clipboard (Piano Roll)
        "Copy" | "Cut" if roll => {
            let Some((_, _, rg)) = roll_notes(app) else { return false };
            if app.selected_notes.is_empty() {
                return true;
            }
            app.midi.clipboard = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i).copied()).collect();
            if name == "Cut" {
                app.delete_selection();
            }
            true
        }
        "Paste" | "Paste at Original Position" | "Paste Replace" if roll => {
            let clip = app.midi.clipboard.clone();
            if clip.is_empty() {
                return true;
            }
            let at_playhead = name != "Paste at Original Position";
            edit(app, name, |notes, sel, i| {
                let first = clip.iter().map(|n| n.pos).min().unwrap_or(0);
                let d = if at_playhead { i.pos as i64 - first as i64 } else { 0 };
                let moved: Vec<Note> = clip.iter().map(|n| Note { pos: (n.pos as i64 + d).max(0) as u32, ..*n }).collect();
                if name == "Paste Replace" {
                    let a = moved.iter().map(|n| n.pos).min().unwrap_or(0);
                    let b = moved.iter().map(|n| n.pos + n.len).max().unwrap_or(0);
                    f::remove_where(notes, sel, |_, n| n.pos >= a && n.pos < b);
                }
                sel.clear();
                for n in moved {
                    notes.push(n);
                    sel.push(notes.len() - 1);
                }
            })
        }
        // ---- Edit ▸ Select (Piano Roll)
        "Select All Following" | "All Following" | "Select All Following of Same Track/Pitch" | "All Following of Same Pitch" if roll => {
            let same = name.ends_with("Same Pitch") || name.ends_with("Track/Pitch");
            select(app, |notes, sel, i| f::select_following(notes, sel, i.pos, same))
        }
        "Select All Inside Locators" | "All Inside Locators" if roll => select(app, |notes, _, i| f::select_inside(notes, i.cycle.0, i.cycle.1)),
        "Deselect Outside Locators" if roll => select(app, |notes, sel, i| f::deselect_outside(notes, sel, i.cycle.0, i.cycle.1)),
        "Select Overlapped Regions/Events" | "Overlapped Notes" | "Select Overlapped Notes" if roll => select(app, |notes, _, _| f::select_overlapped(notes)),
        "Select Equal Regions/Events" | "Same Note Pitch" if roll => select(app, |notes, sel, _| f::select_same_pitch(notes, sel)),
        "Select Similar Regions/Events" | "Same Note Name" if roll => select(app, |notes, sel, _| f::select_same_name(notes, sel)),
        "Select Same Channels" | "Same MIDI Channel" => select(app, |notes, sel, _| f::select_same_channel(notes, sel)),
        "Select Same Subpositions" | "Same Subposition" if roll => select(app, |notes, sel, i| f::select_same_subposition(notes, sel, i.start, i.bar)),
        "Select Same-Colored Regions/Cells/Events" | "Same-Colored Notes" if roll => {
            let by = app.midi.note_color;
            select(app, |notes, sel, _| match by {
                NoteColor::Velocity => f::select_same_velocity(notes, sel),
                NoteColor::Channel => f::select_same_channel(notes, sel),
                NoteColor::Region => (0..notes.len()).collect(),
            })
        }
        "Select Highest Notes" | "Select Lowest Notes" | "Highest Notes" | "Lowest Notes" if roll => {
            let high = name.contains("Highest");
            select(app, |notes, _, _| f::select_extreme(notes, &[], high))
        }
        "Next Event" | "Previous Event" => {
            let next = name == "Next Event";
            select(app, |notes, sel, _| f::step_selection(notes, sel, next))
        }
        n if roll && (n.starts_with("Select Next Region/Cell/Event") || n.starts_with("Select Previous Region/Cell/Event")) => {
            let next = n.starts_with("Select Next");
            select(app, |notes, sel, _| f::step_selection(notes, sel, next))
        }
        "Invert Selection" if roll => select(app, |notes, sel, _| f::invert(notes, sel)),
        // ---- Edit: repeat, length, split, join, move, trim
        "Repeat Regions/Cells/Events" | "Once" if roll => edit(app, "Repeat Notes", |notes, sel, _| {
            f::repeat_notes(notes, sel, 1, PPQ / 4);
        }),
        "Repeat Regions/Events Multiple Times…" | "Multiple…" if roll => {
            app.midi.sheet = Some((Sheet::Repeat { copies: 1, adjust: 0 }, true));
            true
        }
        "Double Region/Cell/Event/Chord Length" | "Double" if roll => edit(app, "Double Length", |notes, sel, _| f::scale_lengths(notes, sel, 2, 1)),
        "Halve Region/Cell/Event/Chord Length" | "Halve" if roll => edit(app, "Halve Length", |notes, sel, _| f::scale_lengths(notes, sel, 1, 2)),
        "Change Region/Cell/Event Length…" | "Change…" if roll => {
            let len = roll_notes(app).and_then(|(_, _, rg)| app.selected_notes.first().and_then(|&i| rg.notes.get(i)).map(|n| n.len)).unwrap_or(PPQ / 4);
            app.midi.sheet = Some((Sheet::Length { ticks: len }, true));
            true
        }
        "Split Regions/Events at Playhead Position" | "Notes at Playhead" | "Split Regions/Events at Rounded Playhead Position" | "Notes at Rounded Playhead" if roll => {
            let rounded = name.contains("Rounded");
            edit(app, "Split Notes", |notes, sel, i| {
                let d = i.beat / 4;
                let at = if rounded { ((i.pos as f64 / d.max(1) as f64).round() as u32) * d } else { i.pos };
                f::split_notes(notes, sel, &[at]);
            })
        }
        "Split Regions/Events at Locators or Marquee Selection" | "Notes at Locators" if roll => edit(app, "Split Notes", |notes, sel, i| {
            f::split_notes(notes, sel, &[i.cycle.0, i.cycle.1]);
        }),
        "Join Regions/Notes" | "Join Notes" if roll => edit(app, "Join Notes", |notes, sel, _| {
            f::join_notes(notes, sel);
        }),
        "Move Region/Event to Playhead Position (Pickup Clock)" | "To Playhead" | "Move Region/Event to Playhead Position and Select Next Region/Event (Pickup Clock+)" if roll => {
            let next = name.contains("Select Next");
            edit(app, "Pickup Clock", |notes, sel, i| {
                f::move_to(notes, sel, i.pos);
                if next {
                    *sel = f::step_selection(notes, sel, true);
                }
            })
        }
        "Set Region/Event/Marquee Start to Playhead Position" | "Note Start to Playhead" if roll => edit(app, "Trim Note Start", |notes, sel, i| f::start_to(notes, sel, i.pos)),
        "Set Region/Event/Marquee End to Playhead Position" | "Note End to Playhead" if roll => edit(app, "Trim Note End", |notes, sel, i| f::end_to(notes, sel, i.pos)),
        "Trim Note End to Remove Overlaps to Selected Notes (8 Ticks Gap)" | "Note End to Remove Overlaps to Selected Notes (8 Ticks Gap)" | "Note Overlap Correction (Selected/Selected)" => edit(app, "Trim Note Ends", |notes, sel, _| f::overlap_correction(notes, sel)),
        "Trim Note End to Remove Overlaps to Following Notes (8 Ticks Gap)" | "Note End to Remove Overlaps to Following Notes (8 Ticks Gap)" => edit(app, "Trim Note Ends", |notes, sel, _| f::trim_ends(notes, sel, f::TrimTo::Following, false, true)),
        "Trim Note End to Remove Overlaps to Repeated Notes (8 Ticks Gap)" | "Note End to Remove Overlaps to Repeated Notes (8 Ticks Gap)" => edit(app, "Trim Note Ends", |notes, sel, _| f::trim_ends(notes, sel, f::TrimTo::Repeated, false, true)),
        "Trim Note End to Selected Notes (8 Ticks Gap)" | "Note End to Selected Notes (8 Ticks Gap)" => edit(app, "Trim Note Ends", |notes, sel, _| f::trim_ends(notes, sel, f::TrimTo::Selected, false, false)),
        "Trim Note End to Selected Notes (4 Ticks Overlap)" | "Note End to Selected Notes (4 Ticks Overlap)" => edit(app, "Trim Note Ends", |notes, sel, _| f::trim_ends(notes, sel, f::TrimTo::Selected, true, false)),
        "Trim Note End to Following Notes (8 Ticks Gap)" | "Note End to Following Notes (8 Ticks Gap)" | "Note Force Legato (Selected/Any)" => edit(app, "Trim Note Ends", |notes, sel, _| f::force_legato(notes, sel)),
        "Trim Note End to Following Notes (4 Ticks Overlap)" | "Note End to Following Notes (4 Ticks Overlap)" => edit(app, "Trim Note Ends", |notes, sel, _| f::trim_ends(notes, sel, f::TrimTo::Following, true, false)),
        "+1 Semitone" | "-1 Semitone" | "+12 Semitones" | "-12 Semitones" => {
            let by = if name.starts_with('+') { 1 } else { -1 } * if name.ends_with("Semitones") { 12 } else { 1 };
            if roll {
                edit(app, "Transpose", |notes, sel, _| f::transpose(notes, sel, by))
            } else {
                let long = match by {
                    1 => "Transpose KeySignature/Chord/Region/Event +1",
                    -1 => "Transpose KeySignature/Chord/Region/Event -1",
                    12 => "Transpose Region/Event +12",
                    _ => "Transpose Region/Event -12",
                };
                app.edit_command(ctx, long)
            }
        }
        // ---- Edit ▸ Delete MIDI Events, Copy MIDI Events
        "Delete Duplicated MIDI Events" | "Duplicates" | "Delete Duplicates" => edit(app, "Delete Duplicates", |notes, sel, _| {
            f::delete_duplicates(notes, sel);
        }),
        "Delete MIDI Events Inside Locators" | "Delete MIDI Events Outside Locators" => {
            let inside = name.contains("Inside");
            edit(app, name, |notes, sel, i| {
                f::delete_by_locators(notes, sel, i.cycle.0, i.cycle.1, inside);
            })
        }
        "Inside Locators" | "Outside Locators" if roll => {
            let inside = name == "Inside Locators";
            edit(app, "Delete Events", |notes, sel, i| {
                f::delete_by_locators(notes, sel, i.cycle.0, i.cycle.1, inside);
            })
        }
        "Delete MIDI Events Outside Region Boundaries" | "Outside Region Boundaries" => edit(app, "Delete Events", |notes, sel, i| {
            f::delete_outside_region(notes, sel, i.start, i.length);
        }),
        "Delete Unselected MIDI Events Within Selection" | "Unselected Within Selection" if roll => edit(app, "Delete Events", |notes, sel, _| {
            f::delete_unselected_within(notes, sel);
        }),
        "Copy MIDI Events…" => {
            if app.selected_region.is_none_or(|(t, r)| !is_midi(app, t, r)) {
                return false;
            }
            let (a, b) = app.cycle;
            app.midi.sheet = Some((Sheet::Copy { mode: 0, left: a, right: b.max(a + 1), dest: b, copies: 1 }, roll));
            true
        }
        // ---- Functions
        "Quantize Selected Regions/Cells/Events" | "Quantize Notes" | "Quantize Selected Events" | "Quantize" if roll => quantize_notes(app, None),
        "Set Quantize Parameter to Next Value" | "Set Quantize Parameter to Previous Value" if roll => {
            let steps: Vec<u32> = f::QUANTIZE_STEPS.iter().filter_map(|s| f::quantize_value(s)).map(|g| g.first().copied().unwrap_or(0)).collect();
            let cur = steps.iter().position(|&g| g == app.roll_quantize).unwrap_or(0);
            let k = if name.contains("Next") { (cur + 1).min(steps.len() - 1) } else { cur.saturating_sub(1) };
            app.roll_quantize = steps[k];
            app.tell(format!("Time Quantize: {}", f::QUANTIZE_STEPS[k]));
            true
        }
        "Set MIDI Channel to Voice Number" => edit(app, "Set MIDI Channel to Voice Number", |notes, sel, _| f::channel_by_voice(notes, sel)),
        "Event Channel +1" | "Event Channel -1" => {
            let by = if name == "Event Channel +1" { 1 } else { -1 };
            edit(app, "Event Channel", |notes, sel, _| f::change_channel(notes, sel, None, by))
        }
        "Open Transform…" | "Show/Hide Transform" => {
            if app.midi.transform.is_some() && name.starts_with("Show/Hide") {
                app.midi.transform = None;
            } else {
                open_transform(app, transform::preset("Fixed Velocity").unwrap_or_else(|| Transform::new("Fixed Velocity")));
            }
            true
        }
        "Time Handles" | "Toggle Time Handles" => {
            app.midi.time_handles = !app.midi.time_handles;
            true
        }
        "Toggle Collapse Mode" => {
            app.midi.collapse = !app.midi.collapse;
            true
        }
        "Note Labels" => {
            app.midi.note_labels = !app.midi.note_labels;
            true
        }
        "By Region Color" | "Set Note Color By Region Color" | "Region Colors" => {
            app.midi.note_color = NoteColor::Region;
            true
        }
        "By Velocity" | "Set Note Color By Velocity" | "Velocity Colors" => {
            app.midi.note_color = NoteColor::Velocity;
            true
        }
        "By MIDI Channel" | "Set Note Color By MIDI Channel" | "MIDI Channel Colors" => {
            app.midi.note_color = NoteColor::Channel;
            true
        }
        "Define Brush Pattern and Set Brush Tool" => {
            if let Some((_, _, rg)) = roll_notes(app) {
                let picked: Vec<Note> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i).copied()).collect();
                if let Some(first) = picked.iter().map(|n| n.pos).min() {
                    app.midi.brush = picked.iter().map(|n| Note { pos: n.pos - first, ..*n }).collect();
                    app.midi.brush.sort_by_key(|n| n.pos);
                }
            }
            app.roll_tool = Tool::Brush;
            true
        }
        "Reset Brush Pattern" => {
            app.midi.brush.clear();
            true
        }
        "Define as Default Note" if roll => {
            if let Some(n) = roll_notes(app).and_then(|(_, _, rg)| app.selected_notes.first().and_then(|&i| rg.notes.get(i).copied())) {
                app.midi.default_note = (n.len, n.velocity);
            }
            true
        }
        "Create Note" => {
            let Some((tick, pitch)) = app.midi.grid_click.take() else { return false };
            let (len, vel) = app.midi.default_note;
            edit(app, "Create Note", |notes, sel, i| {
                let d = (i.beat / 4).max(1);
                notes.push(Note { channel: 0, pitch, velocity: vel, pos: tick / d * d, len, muted: false });
                *sel = vec![notes.len() - 1];
            })
        }
        // (the same labels in the Tracks area's menus: the regions)
        "All Inside Locators" => app.edit_command(ctx, "Select All Inside Locators"),
        "All Following of Same Pitch" => app.edit_command(ctx, "Select All Following of Same Track/Pitch"),
        "Once" => app.key_command(ctx, "Repeat Regions/Cells/Events"),
        "Deselect Outside Locators" => {
            let (a, b) = app.cycle;
            let keep: Vec<(usize, usize)> = app.region_selection().into_iter().filter(|&(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|x| x.start >= a && x.start + x.length <= b)).collect();
            app.selected_region = keep.last().copied();
            app.selected_regions = keep;
            true
        }
        _ => false,
    }
}

/// Whether a View / Functions item shows a check mark.
pub fn checked(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Time Handles" => app.midi.time_handles,
        "Note Labels" => app.midi.note_labels,
        "By Region Color" | "Region Colors" => app.midi.note_color == NoteColor::Region,
        "By Velocity" | "Velocity Colors" => app.midi.note_color == NoteColor::Velocity,
        "By MIDI Channel" | "MIDI Channel Colors" => app.midi.note_color == NoteColor::Channel,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<&'static str> {
        include_str!("../data/commands.txt").lines().filter_map(|l| l.split('\t').nth(1)).collect()
    }

    #[test]
    fn key_command_names_are_logics() {
        let all = commands();
        for n in SET_TOOLS.iter().chain(&CHANNEL_SET).chain(&USER_APPLY).chain(&USER_SELECT).chain(&QUANTIZE_KEYS) {
            assert!(all.contains(n), "not a Logic key command: {n:?}");
        }
        // every Quantize key command's value is one lg-core knows
        for n in QUANTIZE_KEYS {
            assert!(f::quantize_value(&n["Quantize ".len()..]).is_some(), "{n}");
        }
    }

    #[test]
    fn transform_presets_are_the_menus() {
        let menu = crate::menus::local("pianoroll-Functions");
        let sub = menu.iter().find(|i| i.label.ends_with(" Transform")).expect("the Transform submenu");
        let labels: Vec<&str> = sub.children.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, transform::PRESETS.to_vec());
        for n in TRANSFORM_MENU {
            assert!(transform::preset(n).is_some(), "{n}");
        }
    }

    #[test]
    fn user_presets_file_lines() {
        let t = transform::preset("Crescendo").unwrap();
        let line = format!("{}\t{}", 3usize, t.to_line());
        let (slot, rest) = line.split_once('\t').unwrap();
        assert_eq!(slot.parse::<usize>().ok(), Some(3));
        assert_eq!(Transform::from_line(rest), Some(t));
    }
}
