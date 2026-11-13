//! The Play Surface (Window ▸ Show Play Surface, ⌘K): Logic 13's floating "<project> - Keyboard -
//! <track>" window. Two surfaces: Keys & Chords (Chord Strips over an onscreen piano, with the
//! side controls Velocity, Modulation and Pitch Bend) and Drum Pads (pages of 16 pads, with
//! Velocity, Gate and Rate); Current Track (Auto) picks Drum Pads for a drum track.
//!
//! Header: Sustain (drag it right to lock), the chord modifiers 7 · Sus · Dim · Alt, the keyboard
//! overview (drag the range, click beside it for an octave, double-click to reset), the
//! Arpeggiator button (inserts the Arpeggiator MIDI plug-in; its menu opens it and picks the
//! note order), Poly Pitch (slide between keys), Chord Track Input and the More (···) menu:
//! Surface, musical typing (Play Notes / Chords / Drums, Show Key Letters, Send Next Key to
//! Logic Pro), Open Another Play Surface Window, and the chord, keyboard and pad options.
//!
//! Chord Strips: up to six rows of chord types over 12 roots, Chromatic or Circle of Fifths
//! (from the key: Root Auto follows the project key and the Signature track's key changes), a
//! custom row (Learn Chords, Option-click, Define Chords from Chord Track / from Key). A strip's
//! middle plays the chord, its right (top, when narrow) one inversion higher, its left (bottom)
//! one lower; dragging plays the next strips. More ▸ Bass adds the root an octave or two below.
//!
//! The settings are saved with the project (a sidecar next to the other per-project files).

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind};

use crate::app::App;
use crate::look;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Show Play Surface", "Show/Hide Play Surface"];

const NAMES: [&str; 12] = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
const IS_BLACK: [bool; 12] = [false, true, false, true, false, false, true, false, true, false, true, false];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Auto,
    Keys,
    Drums,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Typing {
    Notes,
    Chords,
    Drums,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YAxis {
    Off,
    Modulation,
    ChannelPressure,
    PolyAftertouch,
    Velocity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Chromatic,
    Fifths,
}

/// A row's chord type, bottom row first (rows are added upwards as More ▸ Rows grows).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    Major,
    Minor,
    Power,
    Seventh,
    Sus,
    Dim,
}

pub const ROW_TYPES: [Base; 6] = [Base::Major, Base::Minor, Base::Power, Base::Seventh, Base::Sus, Base::Dim];
/// The chord modifier buttons.
pub const MODS: [&str; 4] = ["7", "Sus", "Dim", "Alt"];
/// The drum pads' Rate slider, bottom to top (index 0: off), and each rate in ticks.
pub const RATES: [(&str, u32); 7] = [("Off", 0), ("1/4", 960), ("1/8", 480), ("1/8T", 320), ("1/16", 240), ("1/16T", 160), ("1/32", 120)];

/// A custom Chord Strip: its name and notes.
#[derive(Clone, Debug, PartialEq)]
pub struct Custom {
    pub name: String,
    pub notes: Vec<u8>,
}

/// What a Play Surface window keeps (saved with the project).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub surface: Surface,
    pub typing: Typing,
    pub key_letters: bool,
    pub rows: u8,
    pub layout: Layout,
    /// None: Auto (the key at the playhead).
    pub root: Option<u8>,
    /// More ▸ Bass: 0 (Off), 12 or 24 semitones below the root.
    pub bass: u8,
    pub mods: [bool; 4],
    pub custom: Vec<Custom>,
    pub show_chords: bool,
    pub show_custom: bool,
    pub y_axis: YAxis,
    /// More ▸ Size: 0 Small, 1 Medium, 2 Large.
    pub size: u8,
    pub show_keyboard: bool,
    /// The piano's lowest note (the overview's range) and musical typing's octave.
    pub low: u8,
    pub octave_c: u8,
    pub velocity: u8,
    pub pad_velocity: bool,
    pub pad_repeat: bool,
    pub page: u8,
    pub gate: u8,
    pub chord_input: bool,
    pub poly_pitch: bool,
    pub sustain_locked: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            surface: Surface::Auto,
            typing: Typing::Notes,
            key_letters: true,
            rows: 3,
            layout: Layout::Chromatic,
            root: None,
            bass: 0,
            mods: [false; 4],
            custom: Vec::new(),
            show_chords: true,
            show_custom: false,
            y_axis: YAxis::Off,
            size: 1,
            show_keyboard: true,
            low: 48,
            octave_c: 60,
            velocity: 90,
            pad_velocity: false,
            pad_repeat: false,
            page: 2,
            gate: 50,
            chord_input: false,
            poly_pitch: false,
            sustain_locked: false,
        }
    }
}

/// A note repeating (drum pads with a Rate): pitch, velocity, the next hit and when the
/// sounding hit ends (seconds).
#[derive(Clone, Debug)]
struct Rep {
    pitch: u8,
    vel: u8,
    next: f64,
    off: f64,
    sounding: bool,
}

#[derive(Default)]
struct Rt {
    /// The mouse's notes on the piano (the key pressed, where the press started, the first key).
    mouse_key: Option<u8>,
    first_key: Option<u8>,
    press_y: f32,
    press_val: u8,
    /// The strip the mouse plays: (row: −1 custom, column, inversion) and its notes.
    strip: Option<(i32, usize, i8)>,
    strip_notes: Vec<u8>,
    pad: Option<u8>,
    typed: HashMap<egui::Key, Vec<u8>>,
    learn: bool,
    bypass_next: bool,
    sustain_down: bool,
    sustain_x: f32,
    modulation: u8,
    bend: i16,
    rate: usize,
    reps: Vec<Rep>,
    last_custom: Option<usize>,
    range_drag: Option<(f32, u8)>,
    /// The piano's width last drawn (the overview's range).
    piano_w: f32,
}

pub struct Win {
    pub open: bool,
    pub s: Settings,
    rt: Rt,
}

impl Win {
    fn new(s: Settings) -> Win {
        Win { open: true, s, rt: Rt::default() }
    }
}

#[derive(Default)]
pub struct State {
    pub wins: Vec<Win>,
}

// ---- chords ----------------------------------------------------------------------------------

/// The modifier buttons a row count leaves (a row of that type already shows it): 7 goes at 4
/// rows, Sus at 5, Dim at 6; Alt stays.
pub fn mod_available(rows: u8, k: usize) -> bool {
    match k {
        0 => rows < 4,
        1 => rows < 5,
        2 => rows < 6,
        _ => true,
    }
}

/// A chord type with the modifiers on: its intervals above the root.
pub fn intervals(base: Base, mods: [bool; 4]) -> Vec<i8> {
    let mut v: Vec<i8> = match base {
        Base::Major => vec![0, 4, 7],
        Base::Minor => vec![0, 3, 7],
        Base::Power => vec![0, 7, 12],
        Base::Seventh => vec![0, 4, 7, 10],
        Base::Sus => vec![0, 5, 7],
        Base::Dim => vec![0, 3, 6],
    };
    let [seven, sus, dim, alt] = mods;
    if sus {
        // the third becomes a fourth (Alt: a second)
        for x in v.iter_mut() {
            if *x == 3 || *x == 4 {
                *x = if alt { 2 } else { 5 };
            }
        }
    }
    if dim && !v.contains(&12) {
        for x in v.iter_mut() {
            match *x {
                4 => *x = 3,
                7 => *x = 6,
                _ => {}
            }
        }
    }
    if seven && !v.contains(&10) && !v.contains(&11) {
        // a major seventh on a major chord, a minor one on the others
        v.push(if v.contains(&4) && !v.contains(&6) { 11 } else { 10 });
    }
    if alt && !sus {
        if v.contains(&4) && v.contains(&7) && !v.contains(&10) && !v.contains(&11) {
            // the augmented triad
            for x in v.iter_mut() {
                if *x == 7 {
                    *x = 8;
                }
            }
        } else if !v.contains(&14) {
            // an extension: the ninth
            v.push(14);
        }
    }
    v.sort();
    v.dedup();
    v
}

/// The chord symbol's suffix for a set of intervals ("" major, "m", "7", "sus4" …).
pub fn suffix(iv: &[i8]) -> String {
    let s = match iv {
        [0, 4, 7] => "",
        [0, 3, 7] => "m",
        [0, 7, 12] => "5",
        [0, 7, 12, 14] => "5(add9)",
        [0, 4, 7, 10] => "7",
        [0, 4, 7, 11] => "maj7",
        [0, 3, 7, 10] => "m7",
        [0, 5, 7] => "sus4",
        [0, 2, 7] => "sus2",
        [0, 5, 7, 10] => "7sus4",
        [0, 2, 7, 10] => "7sus2",
        [0, 5, 7, 11] => "maj7sus4",
        [0, 2, 7, 11] => "maj7sus2",
        [0, 3, 6] => "dim",
        [0, 3, 6, 10] => "m7♭5",
        [0, 3, 6, 9] => "dim7",
        [0, 3, 6, 14] => "dim(add9)",
        [0, 3, 6, 10, 14] => "m9♭5",
        [0, 4, 8] => "aug",
        [0, 4, 7, 14] => "add9",
        [0, 3, 7, 14] => "m(add9)",
        [0, 4, 7, 10, 14] => "9",
        [0, 4, 7, 11, 14] => "maj9",
        [0, 3, 7, 10, 14] => "m9",
        _ => "?",
    };
    s.to_string()
}

/// Reads a chord symbol ("F♯m7", "Bb", "Csus4", "Am"): its root (0 = C) and intervals.
pub fn parse_chord(name: &str) -> Option<(u8, Vec<i8>)> {
    let root = lg_core::timeline::root_of(name)? as u8;
    let mut rest: &str = name.trim();
    let mut it = rest.char_indices();
    it.next();
    let mut cut = it.next().map_or(rest.len(), |(i, _)| i);
    if let Some(c) = rest[cut..].chars().next() {
        if matches!(c, '#' | '♯' | 'b' | '♭') {
            cut += c.len_utf8();
        }
    }
    rest = &rest[cut..];
    // (a slash bass is left out)
    let rest = rest.split('/').next().unwrap_or("");
    for base in ROW_TYPES {
        for bits in 0..16u8 {
            let mods = [bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0];
            let iv = intervals(base, mods);
            if suffix(&iv) == rest {
                return Some((root, iv));
            }
        }
    }
    let iv = match rest {
        "min" | "-" => vec![0, 3, 7],
        "maj" | "M" => vec![0, 4, 7],
        "sus" => vec![0, 5, 7],
        "°" => vec![0, 3, 6],
        "+" => vec![0, 4, 8],
        "M7" | "Δ" | "Δ7" => vec![0, 4, 7, 11],
        "min7" | "-7" => vec![0, 3, 7, 10],
        "6" => vec![0, 4, 7, 9],
        "m6" => vec![0, 3, 7, 9],
        _ if rest.starts_with('m') && !rest.starts_with("maj") => vec![0, 3, 7],
        _ => vec![0, 4, 7],
    };
    Some((root, iv))
}

/// Where a chord on `root` sits: its root's note (G to F♯ around C3).
pub fn root_pitch(root: u8) -> u8 {
    let r = root % 12;
    if r >= 7 {
        48 + r
    } else {
        60 + r
    }
}

/// A chord's notes: the intervals over the root, `inv` inversions up (+) or down (−), and the
/// bass note `bass` semitones below the root (0: none).
pub fn voicing(root: u8, iv: &[i8], inv: i8, bass: u8) -> Vec<u8> {
    let rp = root_pitch(root) as i32;
    let mut v: Vec<i32> = iv.iter().map(|i| rp + *i as i32).collect();
    v.sort();
    for _ in 0..inv.max(0) {
        let lo = v.remove(0);
        v.push(lo + 12);
    }
    for _ in 0..(-inv).max(0) {
        let hi = v.pop().unwrap_or(rp);
        v.insert(0, hi - 12);
    }
    if bass > 0 {
        v.insert(0, rp - bass as i32);
    }
    v.into_iter().filter(|n| (0..=127).contains(n)).map(|n| n as u8).collect()
}

/// The roots of the 12 strips: C to B (Chromatic) or by fifths from the middle strip (the 7th)
/// on `key` (Circle of Fifths).
pub fn strip_roots(layout: Layout, key: u8) -> [u8; 12] {
    let mut r = [0u8; 12];
    for (j, x) in r.iter_mut().enumerate() {
        *x = match layout {
            Layout::Chromatic => j as u8,
            Layout::Fifths => (key as i32 + 7 * (j as i32 - 6)).rem_euclid(12) as u8,
        };
    }
    r
}

/// The rows shown, top to bottom.
pub fn rows_top_down(rows: u8) -> Vec<Base> {
    ROW_TYPES[..(rows.clamp(1, 6) as usize)].iter().rev().copied().collect()
}

/// The chords of a key (its root; minor keys use the relative major's), in circle-of-fifths
/// order: IV I V ii vi iii vii°.
pub fn key_chords(key_root: u8, minor: bool) -> Vec<Custom> {
    let k = if minor { (key_root + 3) % 12 } else { key_root % 12 };
    [(5u8, Base::Major), (0, Base::Major), (7, Base::Major), (2, Base::Minor), (9, Base::Minor), (4, Base::Minor), (11, Base::Dim)]
        .iter()
        .map(|(o, b)| {
            let r = (k + o) % 12;
            let iv = intervals(*b, [false; 4]);
            Custom { name: format!("{}{}", NAMES[r as usize], suffix(&iv)), notes: voicing(r, &iv, 0, 0) }
        })
        .collect()
}

/// The chords on the Chord track (one per bar), each once, in order.
pub fn chord_track_chords(text: &str) -> Vec<Custom> {
    let mut out: Vec<Custom> = Vec::new();
    for w in text.split_whitespace() {
        if out.iter().any(|c| c.name == w) {
            continue;
        }
        if let Some((r, iv)) = parse_chord(w) {
            out.push(Custom { name: w.to_string(), notes: voicing(r, &iv, 0, 0) });
        }
    }
    out
}

/// Puts `chord` on the Chord track at bar `bar` (0-based; bars before it keep or repeat the
/// chords there).
pub fn chord_track_set(text: &str, bar: usize, chord: &str) -> String {
    let mut w: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
    while w.len() <= bar {
        let fill = w.last().cloned().unwrap_or_else(|| chord.to_string());
        w.push(fill);
    }
    w[bar] = chord.to_string();
    w.join(" ")
}

/// The drum pads of page `page`: 16 notes from the bottom-left pad.
pub fn page_notes(page: u8) -> Vec<u8> {
    let start = 36i32 + 16 * (page as i32 - 2);
    (start..start + 16).filter(|n| (0..=127).contains(n)).map(|n| n as u8).collect()
}

pub const PAGES: u8 = 8;

/// A pad's name: the drum kit's name for the note, else the note.
pub fn pad_name(n: u8) -> String {
    if let Some(s) = crate::stepseq::drum_name(n) {
        return s.to_string();
    }
    if (35..=81).contains(&n) {
        return lg_core::environment::GM_DRUMS[(n - 35) as usize].to_string();
    }
    note_name(n)
}

pub fn note_name(n: u8) -> String {
    format!("{}{}", NAMES[n as usize % 12], n as i32 / 12 - 2)
}

/// A key's musical typing letter (Play Notes) from the octave's C.
fn letter(octave_c: u8, n: u8) -> Option<&'static str> {
    const L: [&str; 18] = ["A", "W", "S", "E", "D", "F", "T", "G", "Y", "H", "U", "J", "K", "O", "L", "P", ";", "'"];
    L.get(n.checked_sub(octave_c)? as usize).copied()
}

const NOTE_KEYS: [egui::Key; 18] = {
    use egui::Key::*;
    [A, W, S, E, D, F, T, G, Y, H, U, J, K, O, L, P, Semicolon, Quote]
};
/// Play Chords: the A row plays the bottom row's strips, the Q row the row above.
const CHORD_KEYS: [[egui::Key; 12]; 2] = {
    use egui::Key::*;
    [[A, S, D, F, G, H, J, K, L, Semicolon, Quote, Backslash], [Q, W, E, R, T, Y, U, I, O, P, OpenBracket, CloseBracket]]
};
/// Play Drums: four rows of four keys, the bottom-left pad first.
const DRUM_KEYS: [egui::Key; 16] = {
    use egui::Key::*;
    [Z, X, C, V, A, S, D, F, Q, W, E, R, Num1, Num2, Num3, Num4]
};
const CHORD_LETTERS: [[&str; 12]; 2] = [["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'", "\\"], ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]"]];
const DRUM_LETTERS: [&str; 16] = ["Z", "X", "C", "V", "A", "S", "D", "F", "Q", "W", "E", "R", "1", "2", "3", "4"];

// ---- settings text ---------------------------------------------------------------------------

fn sname(s: Surface) -> &'static str {
    match s {
        Surface::Auto => "auto",
        Surface::Keys => "keys",
        Surface::Drums => "drums",
    }
}

pub fn to_text(wins: &[Settings]) -> String {
    let mut out = String::from("# Bramble Studio Play Surface\n");
    for s in wins {
        out.push_str("[window]\n");
        let b = |v: bool| if v { "1" } else { "0" };
        out.push_str(&format!("surface={}\ntyping={}\nkey_letters={}\nrows={}\nlayout={}\nroot={}\nbass={}\n", sname(s.surface), s.typing as u8, b(s.key_letters), s.rows, if s.layout == Layout::Fifths { "fifths" } else { "chromatic" }, s.root.map_or("auto".to_string(), |r| r.to_string()), s.bass));
        out.push_str(&format!("mods={}{}{}{}\nshow_chords={}\nshow_custom={}\ny_axis={}\nsize={}\nshow_keyboard={}\nlow={}\noctave={}\nvelocity={}\n", b(s.mods[0]), b(s.mods[1]), b(s.mods[2]), b(s.mods[3]), b(s.show_chords), b(s.show_custom), s.y_axis as u8, s.size, b(s.show_keyboard), s.low, s.octave_c, s.velocity));
        out.push_str(&format!("pad_velocity={}\npad_repeat={}\npage={}\ngate={}\nchord_input={}\npoly_pitch={}\nsustain_locked={}\n", b(s.pad_velocity), b(s.pad_repeat), s.page, s.gate, b(s.chord_input), b(s.poly_pitch), b(s.sustain_locked)));
        for c in &s.custom {
            out.push_str(&format!("custom={}:{}\n", c.name, c.notes.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")));
        }
    }
    out
}

pub fn from_text(t: &str) -> Vec<Settings> {
    let mut out: Vec<Settings> = Vec::new();
    for line in t.lines() {
        let line = line.trim();
        if line == "[window]" {
            out.push(Settings::default());
            continue;
        }
        let (Some(s), Some((k, v))) = (out.last_mut(), line.split_once('=')) else { continue };
        let on = v == "1";
        let n = v.parse::<u8>().unwrap_or(0);
        match k {
            "surface" => s.surface = match v { "keys" => Surface::Keys, "drums" => Surface::Drums, _ => Surface::Auto },
            "typing" => s.typing = [Typing::Notes, Typing::Chords, Typing::Drums][(n as usize).min(2)],
            "key_letters" => s.key_letters = on,
            "rows" => s.rows = n.clamp(1, 6),
            "layout" => s.layout = if v == "fifths" { Layout::Fifths } else { Layout::Chromatic },
            "root" => s.root = v.parse::<u8>().ok().map(|r| r % 12),
            "bass" => s.bass = if n == 12 || n == 24 { n } else { 0 },
            "mods" => {
                for (i, c) in v.chars().take(4).enumerate() {
                    s.mods[i] = c == '1';
                }
            }
            "show_chords" => s.show_chords = on,
            "show_custom" => s.show_custom = on,
            "y_axis" => s.y_axis = [YAxis::Off, YAxis::Modulation, YAxis::ChannelPressure, YAxis::PolyAftertouch, YAxis::Velocity][(n as usize).min(4)],
            "size" => s.size = n.min(2),
            "show_keyboard" => s.show_keyboard = on,
            "low" => s.low = n.min(108),
            "octave" => s.octave_c = n.min(120),
            "velocity" => s.velocity = n.clamp(1, 127),
            "pad_velocity" => s.pad_velocity = on,
            "pad_repeat" => s.pad_repeat = on,
            "page" => s.page = n.min(PAGES - 1),
            "gate" => s.gate = n.clamp(1, 100),
            "chord_input" => s.chord_input = on,
            "poly_pitch" => s.poly_pitch = on,
            "sustain_locked" => s.sustain_locked = on,
            "custom" => {
                if let Some((name, notes)) = v.rsplit_once(':') {
                    let notes: Vec<u8> = notes.split(',').filter_map(|x| x.parse().ok()).collect();
                    if !notes.is_empty() {
                        s.custom.push(Custom { name: name.to_string(), notes });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn sidecar(app: &App) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(&app.file.as_ref()?.0)?;
    Some(side.parent()?.parent()?.join("play-surface").join(side.file_name()?))
}

/// Saves the windows' settings with the project (called on Save).
pub fn save_sidecar(app: &App) {
    let Some(path) = sidecar(app) else { return };
    if app.ps.wins.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let all: Vec<Settings> = app.ps.wins.iter().map(|w| w.s.clone()).collect();
    let _ = std::fs::write(path, to_text(&all));
}

/// The project's saved Play Surface settings (the windows stay closed until ⌘K).
pub fn after_open(app: &mut App) {
    app.ps.wins.clear();
    let Some(path) = sidecar(app) else { return };
    let Ok(t) = std::fs::read_to_string(path) else { return };
    app.ps.wins = from_text(&t).into_iter().map(|s| Win { open: false, s, rt: Rt::default() }).collect();
}

// ---- playing ---------------------------------------------------------------------------------

fn target(app: &App) -> usize {
    app.armed.iter().min().copied().or(app.selected_track).unwrap_or(0)
}

/// The selected track plays drums (a Drummer track or a drum kit's name).
pub fn track_is_drums(app: &App) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(target(app))).is_some_and(|tr| {
        let n = tr.name.to_lowercase();
        tr.kind == lg_core::TrackKind::Drummer || ["drum", "kit", "beat", "perc", "808", "909"].iter().any(|w| n.contains(w))
    })
}

fn shows_drums(app: &App, s: &Settings) -> bool {
    match s.surface {
        Surface::Auto => track_is_drums(app),
        Surface::Keys => false,
        Surface::Drums => true,
    }
}

fn notes_on(app: &mut App, notes: &[u8], vel: u8) {
    for n in notes {
        crate::live::note(app, *n, vel.max(1));
    }
}

fn notes_off(app: &mut App, notes: &[u8]) {
    for n in notes {
        crate::live::note(app, *n, 0);
    }
}

fn control(app: &mut App, cc: u8, v: u16) {
    let t = target(app);
    if let Ok(mut e) = app.engine.lock() {
        e.live_control(t, cc, v);
    }
}

fn message(app: &mut App, status: u8, a: u8, b: u8) {
    let t = target(app);
    if let Ok(mut e) = app.engine.lock() {
        e.live_message(t, status, a, b);
    }
}

/// The key at the playhead as a root (0 = C) and whether it is minor.
fn key_now(app: &App) -> (u8, bool) {
    let Some(p) = app.project.as_ref() else { return (0, false) };
    let tick = app.position().max(0.0) as u32;
    let bar = lg_core::timeline::Map::of(p).bar_of(tick).0;
    let k = lg_core::timeline::key_at(p, bar);
    let minor = k.contains("minor") || k.trim_end().ends_with('m');
    (lg_core::timeline::root_of(&k).map_or(0, |r| r as u8), minor)
}

fn root_key(app: &App, s: &Settings) -> u8 {
    s.root.unwrap_or_else(|| key_now(app).0)
}

/// The predefined strip at (row, column): its name and intervals over its root.
fn strip_chord(app: &App, s: &Settings, row: usize, col: usize) -> (u8, Vec<i8>, String) {
    let base = rows_top_down(s.rows)[row];
    let mut mods = s.mods;
    for (k, m) in mods.iter_mut().enumerate() {
        *m = *m && mod_available(s.rows, k);
    }
    let root = strip_roots(s.layout, root_key(app, s))[col];
    let iv = intervals(base, mods);
    let name = format!("{}{}", NAMES[root as usize], suffix(&iv));
    (root, iv, name)
}

fn play_strip(app: &mut App, w: usize, row: i32, col: usize, inv: i8, add_to_custom: bool) {
    let s = app.ps.wins[w].s.clone();
    let (notes, name) = if row < 0 {
        let Some(c) = s.custom.get(col) else { return };
        let mut v: Vec<i8> = c.notes.iter().map(|n| *n as i8 - c.notes[0] as i8).collect();
        v.sort();
        let root = c.notes[0] % 12;
        let mut notes = voicing(root, &v, inv, 0);
        // (custom chords keep their own register)
        let shift = c.notes[0] as i32 - root_pitch(root) as i32;
        notes = notes.into_iter().map(|n| (n as i32 + shift).clamp(0, 127) as u8).collect();
        if s.bass > 0 {
            notes.insert(0, c.notes[0].saturating_sub(s.bass));
        }
        app.ps.wins[w].rt.last_custom = Some(col);
        (notes, c.name.clone())
    } else {
        let (root, iv, name) = strip_chord(app, &s, row as usize, col);
        (voicing(root, &iv, inv, s.bass), name)
    };
    if row >= 0 && (add_to_custom || app.ps.wins[w].rt.learn) {
        let (root, iv, _) = strip_chord(app, &s, row as usize, col);
        let c = Custom { name: name.clone(), notes: voicing(root, &iv, inv, 0) };
        let ps = &mut app.ps.wins[w];
        ps.s.custom.push(c);
        ps.s.show_custom = true;
        ps.rt.last_custom = Some(ps.s.custom.len() - 1);
        app.dirty = true;
    }
    if s.chord_input {
        if let Some(p) = app.project.as_ref() {
            let bar = lg_core::timeline::Map::of(p).bar_of(app.position().max(0.0) as u32).0 as usize;
            let cur = if app.chord_text.trim().is_empty() { "C Am F G".to_string() } else { app.chord_text.clone() };
            app.chord_text = chord_track_set(&cur, bar, &name);
            app.dirty = true;
            crate::players::regenerate_all(app);
        }
    }
    let vel = s.velocity;
    notes_on(app, &notes, vel);
    app.ps.wins[w].rt.strip_notes = notes;
}

fn stop_strip(app: &mut App, w: usize) {
    let notes = std::mem::take(&mut app.ps.wins[w].rt.strip_notes);
    notes_off(app, &notes);
    app.ps.wins[w].rt.strip = None;
}

fn set_sustain(app: &mut App, w: usize, on: bool) {
    app.ps.wins[w].rt.sustain_down = on;
    control(app, 64, if on { 127 } else { 0 });
}

/// Opens (or, open, closes) the Play Surface.
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Show Play Surface" | "Show/Hide Play Surface" => {
            if app.ps.wins.is_empty() {
                app.ps.wins.push(Win::new(Settings::default()));
            } else {
                let open = app.ps.wins.iter().any(|w| w.open);
                for w in app.ps.wins.iter_mut() {
                    w.open = !open;
                }
                if open {
                    for w in 0..app.ps.wins.len() {
                        release_all(app, w);
                    }
                }
            }
            true
        }
        _ => false,
    }
}

fn release_all(app: &mut App, w: usize) {
    stop_strip(app, w);
    let rt = &mut app.ps.wins[w].rt;
    let mut notes: Vec<u8> = rt.mouse_key.take().into_iter().collect();
    notes.extend(rt.pad.take());
    notes.extend(rt.typed.drain().flat_map(|(_, v)| v));
    notes.extend(rt.reps.drain(..).filter(|r| r.sounding).map(|r| r.pitch));
    notes_off(app, &notes);
}

/// Musical typing into the Play Surface (when it is open and Musical Typing's window isn't):
/// Play Notes, Play Chords or Play Drums, plus the side controls' keys. Escape lets the next key
/// through to the key commands. Returns the keys used.
pub fn keys(app: &mut App, ctx: &egui::Context) -> Vec<egui::Key> {
    let mut used = Vec::new();
    let Some(w) = app.ps.wins.iter().position(|w| w.open) else { return used };
    if app.live.typing_open || ctx.egui_wants_keyboard_input() {
        return used;
    }
    let events: Vec<(egui::Key, bool, bool, egui::Modifiers)> = ctx.input(|i| i.events.iter().filter_map(|e| match e {
        egui::Event::Key { key, pressed, repeat, modifiers, .. } => Some((*key, *pressed, *repeat, *modifiers)),
        _ => None,
    }).collect());
    let drums = shows_drums(app, &app.ps.wins[w].s);
    for (key, pressed, repeat, m) in events {
        if m.command || m.ctrl || m.alt {
            continue;
        }
        // a key down that is held: its release ends what it played
        if !pressed {
            if let Some(notes) = app.ps.wins[w].rt.typed.remove(&key) {
                notes_off(app, &notes);
                used.push(key);
                continue;
            }
        }
        if pressed && !repeat && app.ps.wins[w].rt.bypass_next {
            // Escape before: this key runs its key command
            app.ps.wins[w].rt.bypass_next = false;
            continue;
        }
        if key == egui::Key::Escape && pressed && !repeat {
            app.ps.wins[w].rt.bypass_next = true;
            used.push(key);
            continue;
        }
        let s = app.ps.wins[w].s.clone();
        let typing = if drums { Typing::Drums } else { s.typing };
        // the notes
        let notes: Option<Vec<u8>> = match typing {
            Typing::Notes => NOTE_KEYS.iter().position(|k| *k == key).map(|i| vec![(s.octave_c as usize + i).min(127) as u8]),
            Typing::Chords => CHORD_KEYS.iter().enumerate().find_map(|(r, row)| row.iter().position(|k| *k == key).map(|c| (r, c))).and_then(|(r, c)| {
                let rows = rows_top_down(s.rows).len();
                if r >= rows {
                    return None;
                }
                let (root, iv, _) = strip_chord(app, &s, rows - 1 - r, c);
                Some(voicing(root, &iv, 0, s.bass))
            }),
            Typing::Drums => DRUM_KEYS.iter().position(|k| *k == key).and_then(|i| page_notes(s.page).get(i).map(|n| vec![*n])),
        };
        if let Some(notes) = notes {
            used.push(key);
            if pressed && !repeat && !app.ps.wins[w].rt.typed.contains_key(&key) {
                notes_on(app, &notes, s.velocity);
                app.ps.wins[w].rt.typed.insert(key, notes);
            }
            continue;
        }
        if !pressed {
            match key {
                egui::Key::Num1 | egui::Key::Num2 => {
                    app.ps.wins[w].rt.bend = 0;
                    control(app, 128, 8192);
                    used.push(key);
                }
                egui::Key::Tab => used.push(key),
                _ => {}
            }
            continue;
        }
        if repeat {
            continue;
        }
        let ps = &mut app.ps.wins[w].s;
        match key {
            egui::Key::Z => {
                ps.octave_c = ps.octave_c.saturating_sub(12);
                ps.low = ps.low.saturating_sub(12);
            }
            egui::Key::X => {
                ps.octave_c = (ps.octave_c + 12).min(120);
                ps.low = (ps.low + 12).min(108);
            }
            egui::Key::C => ps.velocity = ps.velocity.saturating_sub(10).max(1),
            egui::Key::V => ps.velocity = (ps.velocity + 10).min(127),
            egui::Key::Tab => {
                let on = !app.ps.wins[w].rt.sustain_down;
                set_sustain(app, w, on);
            }
            egui::Key::Num1 | egui::Key::Num2 => {
                let up = key == egui::Key::Num2;
                app.ps.wins[w].rt.bend = if up { 8191 } else { -8192 };
                control(app, 128, if up { 16383 } else { 0 });
            }
            egui::Key::Num3 | egui::Key::Num4 | egui::Key::Num5 | egui::Key::Num6 | egui::Key::Num7 | egui::Key::Num8 => {
                let k = [egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8].iter().position(|x| *x == key).unwrap_or(0);
                let v = (k as u16 * 127 / 5) as u8;
                app.ps.wins[w].rt.modulation = v;
                control(app, 1, v as u16);
            }
            _ => continue,
        }
        used.push(key);
    }
    used
}

/// Each frame: the drum pads' note repeat.
fn repeat_frame(app: &mut App, ctx: &egui::Context, w: usize) {
    let rt = &app.ps.wins[w].rt;
    let rate = RATES[rt.rate.min(RATES.len() - 1)].1;
    let held: Vec<u8> = rt.pad.into_iter().chain(rt.typed.values().flatten().copied()).collect();
    if rate == 0 && rt.reps.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let every = rate as f64 / 960.0 * 60.0 / app.tempo().max(1.0);
    let gate = app.ps.wins[w].s.gate.clamp(1, 100) as f64 / 100.0;
    let vel = app.ps.wins[w].s.velocity;
    let mut reps = std::mem::take(&mut app.ps.wins[w].rt.reps);
    // released pads (or the rate back at off) stop repeating
    let (gone, mut keep): (Vec<Rep>, Vec<Rep>) = reps.drain(..).partition(|r| rate == 0 || !held.contains(&r.pitch));
    for r in gone.iter().filter(|r| r.sounding && !held.contains(&r.pitch)) {
        crate::live::note(app, r.pitch, 0);
    }
    if rate > 0 {
        for p in &held {
            if !keep.iter().any(|r| r.pitch == *p) {
                keep.push(Rep { pitch: *p, vel, next: now + every, off: now + every * gate, sounding: false });
            }
        }
    }
    for r in keep.iter_mut() {
        if r.sounding && now >= r.off {
            crate::live::note(app, r.pitch, 0);
            r.sounding = false;
        }
        if now >= r.next {
            crate::live::note(app, r.pitch, 0);
            crate::live::note(app, r.pitch, r.vel);
            r.sounding = true;
            r.off = now + every * gate;
            r.next += every;
            if r.next < now {
                r.next = now + every;
            }
        }
    }
    let busy = !keep.is_empty();
    app.ps.wins[w].rt.reps = keep;
    if busy {
        ctx.request_repaint();
    }
}

// ---- the window ------------------------------------------------------------------------------

const BG: Color32 = Color32::from_gray(0x1f);
const CTRL: Color32 = Color32::from_gray(0x36);
const CTRL_ON: Color32 = Color32::from_gray(0x5a);
const SEP: Color32 = Color32::from_gray(0x50);
const BLUE: Color32 = Color32::from_rgb(0x2f, 0x7c, 0xe8);
const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x8a, 0x24);

/// The Play Surface windows.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    for w in 0..app.ps.wins.len() {
        if app.ps.wins[w].open {
            window(app, ctx, w);
            repeat_frame(app, ctx, w);
        }
    }
}

fn window(app: &mut App, ctx: &egui::Context, w: usize) {
    let proj = app.file.as_ref().and_then(|f| f.0.file_name().map(|n| n.to_string_lossy().into_owned())).or_else(|| app.project.as_ref().map(|p| p.name.clone())).unwrap_or_default();
    let track = app.project.as_ref().and_then(|p| p.tracks.get(target(app))).map(|t| t.name.clone()).unwrap_or_default();
    let drums = shows_drums(app, &app.ps.wins[w].s);
    let title = format!("{proj} - {} - {track}", if drums { "Drum Pads" } else { "Keyboard" });
    let screen = ctx.content_rect();
    let mut open = true;
    let frame = egui::Frame::window(&ctx.global_style()).fill(BG).inner_margin(egui::Margin::same(0));
    egui::Window::new(title)
        .id(egui::Id::unique(("lg-play-surface", w)))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .frame(frame)
        .default_size(vec2(1390.0f32.min(screen.width() * 0.92), 569.0f32.min(screen.height() * 0.8)))
        .min_size(vec2(560.0, 300.0))
        .show(ctx, |ui| {
            let size = ui.available_size().max(vec2(560.0, 300.0));
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(r, 0.0, BG);
            header(app, ui, &p, r, w, drums);
            if drums {
                drum_side(app, ui, &p, r, w);
                pads(app, ui, &p, Rect::from_min_max(r.min + vec2(64.0, 36.8), r.max - vec2(17.0, 14.0)), w);
            } else {
                key_side(app, ui, &p, r, w);
                let s = app.ps.wins[w].s.clone();
                let n_rows = if s.show_chords { s.rows as usize } else { 0 } + s.show_custom as usize;
                let piano_h = if s.show_keyboard { 281.0f32.min(r.height() * 0.5) } else { 0.0 };
                let top = r.min.y + 36.8;
                let bottom = r.max.y - 16.5 - piano_h - if s.show_keyboard && n_rows > 0 { 8.0 } else { 0.0 };
                if n_rows > 0 {
                    strips(app, ui, &p, Rect::from_min_max(pos2(r.min.x + 64.0, top), pos2(r.max.x - 17.0, bottom)), w);
                }
                if s.show_keyboard {
                    let pr = Rect::from_min_max(pos2(r.min.x + 64.0, if n_rows > 0 { r.max.y - 16.5 - piano_h } else { top }), pos2(r.max.x, r.max.y - 16.5));
                    piano(app, ui, &p, pr, w);
                }
            }
        });
    if !open {
        release_all(app, w);
        app.ps.wins[w].open = false;
        app.ps.wins[w].rt.learn = false;
    }
}

fn pill(p: &egui::Painter, r: Rect, fill: Color32) {
    p.rect_filled(r, 6.0, fill);
}

fn label(p: &egui::Painter, r: Rect, t: &str, col: Color32) {
    p.text(r.center(), Align2::CENTER_CENTER, t, look::regular(13.0), col);
}

/// The header row: Sustain + lock, 7 · Sus · Dim · Alt (keys) or the pad pages (drums), the
/// overview, Arpeggiator, Poly Pitch, Chord Track Input and More.
fn header(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, w: usize, drums: bool) {
    let y0 = r.min.y + 1.5;
    let h = 23.0;
    let id = egui::Id::unique(("ps-head", w));
    let row = |x0: f32, x1: f32| Rect::from_min_max(pos2(r.min.x + x0, y0), pos2(r.min.x + x1, y0 + h));
    // Sustain | lock
    let sus = row(9.7, 82.0);
    let lock = row(82.0, 108.0);
    pill(p, sus.union(lock), CTRL);
    let held = app.ps.wins[w].rt.sustain_down || app.ps.wins[w].s.sustain_locked;
    p.rect_filled(sus, CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, if held { CTRL_ON } else { Color32::from_gray(0x48) });
    label(p, sus, "Sustain", Color32::WHITE);
    let locked = app.ps.wins[w].s.sustain_locked;
    crate::glyphs::paint(p, lock, if locked { crate::glyphs::ph_fill::LOCK } else { crate::glyphs::ph::LOCK }, 12.0, if locked { Color32::WHITE } else { Color32::from_gray(0x90) });
    let sr = ui.interact(sus, id.with("sus"), Sense::click_and_drag()).on_hover_text("Sustain (drag right to lock)");
    if sr.is_pointer_button_down_on() && !app.ps.wins[w].rt.sustain_down && !locked {
        app.ps.wins[w].rt.sustain_x = sr.interact_pointer_pos().map_or(0.0, |q| q.x);
        set_sustain(app, w, true);
    }
    if sr.dragged() && sr.interact_pointer_pos().is_some_and(|q| q.x - app.ps.wins[w].rt.sustain_x > 24.0) && !app.ps.wins[w].s.sustain_locked {
        app.ps.wins[w].s.sustain_locked = true;
    }
    if (sr.drag_stopped() || sr.clicked()) && !app.ps.wins[w].s.sustain_locked {
        set_sustain(app, w, false);
    }
    if sr.clicked() && locked {
        app.ps.wins[w].s.sustain_locked = false;
        set_sustain(app, w, false);
    }
    if ui.interact(lock, id.with("lock"), Sense::click()).on_hover_text("Lock Sustain").clicked() {
        let l = !app.ps.wins[w].s.sustain_locked;
        app.ps.wins[w].s.sustain_locked = l;
        set_sustain(app, w, l);
    }
    if drums {
        // the page buttons: one per grid of 16 pads
        let page = app.ps.wins[w].s.page;
        let g = row(119.5, 119.5 + 28.0 * PAGES as f32);
        pill(p, g, CTRL);
        for k in 0..PAGES {
            let b = row(119.5 + 28.0 * k as f32, 119.5 + 28.0 * (k + 1) as f32);
            if k == page {
                p.rect_filled(b.shrink(1.5), 5.0, CTRL_ON);
            }
            label(p, b, &(k + 1).to_string(), if k == page { Color32::WHITE } else { Color32::from_gray(0xc8) });
            let notes = page_notes(k);
            let tip = format!("Pads {}–{}", note_name(notes[0]), note_name(*notes.last().unwrap_or(&notes[0])));
            if ui.interact(b, id.with(("page", k)), Sense::click()).on_hover_text(tip).clicked() {
                app.ps.wins[w].s.page = k;
            }
        }
    } else {
        // 7 · Sus · Dim · Alt
        let g = row(119.5, 345.4);
        pill(p, g, CTRL);
        let shown: Vec<usize> = (0..4).filter(|k| mod_available(app.ps.wins[w].s.rows, *k)).collect();
        let bw = g.width() / 4.0;
        for (i, k) in shown.iter().enumerate() {
            let b = Rect::from_min_size(pos2(g.min.x + i as f32 * bw, g.min.y), vec2(bw, h));
            let on = app.ps.wins[w].s.mods[*k];
            if on {
                p.rect_filled(b.shrink(1.5), 5.0, BLUE);
            }
            if i > 0 && !on && !app.ps.wins[w].s.mods[shown[i - 1]] {
                p.vline(b.min.x, (b.min.y + 5.0)..=(b.max.y - 5.0), Stroke::new(1.0, SEP));
            }
            label(p, b, MODS[*k], Color32::WHITE);
            if ui.interact(b, id.with(("mod", *k)), Sense::click()).clicked() {
                app.ps.wins[w].s.mods[*k] = !on;
            }
        }
        overview(app, ui, p, Rect::from_min_max(pos2(r.min.x + 507.0, r.min.y), pos2(r.min.x + 881.0, r.min.y + 26.0)), w);
    }
    // the right group (from the right edge)
    let rx = |a: f32, b: f32| Rect::from_min_max(pos2(r.max.x - a, y0), pos2(r.max.x - b, y0 + h));
    let more = rx(72.0, 10.0);
    let chord_in = rx(117.0, 83.0);
    let poly = rx(213.0, 129.0);
    let arp = rx(287.0, 222.0);
    // Arpeggiator: the button and its menu
    let t = target(app);
    let arp_k = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.extra.midi_fx.iter().position(|s| s.name == "Arpeggiator"));
    pill(p, arp, CTRL);
    let arp_main = Rect::from_min_max(arp.min, pos2(arp.min.x + 38.0, arp.max.y));
    let arp_menu = Rect::from_min_max(pos2(arp_main.max.x, arp.min.y), arp.max);
    if arp_k.is_some() {
        p.rect_filled(arp_main.shrink(1.5), 5.0, BLUE);
    }
    crate::glyphs::paint(p, arp_main, crate::glyphs::ph_fill::CHART_SCATTER, 14.0, Color32::WHITE);
    p.vline(arp_main.max.x, (arp.min.y + 5.0)..=(arp.max.y - 5.0), Stroke::new(1.0, SEP));
    crate::glyphs::paint(p, arp_menu, crate::glyphs::ph::CARET_DOWN, 11.0, Color32::from_gray(0xd0));
    if ui.interact(arp_main, id.with("arp"), Sense::click()).on_hover_text("Arpeggiator").clicked() {
        toggle_arp(app, t, arp_k);
    }
    let ar = ui.interact(arp_menu, id.with("arp-menu"), Sense::click()).on_hover_text("Arpeggiator Presets");
    egui::Popup::menu(&ar).show(|ui| {
        ui.set_min_width(170.0);
        if ui.button("Open Plug-in").clicked() {
            let k = arp_k.unwrap_or_else(|| toggle_arp(app, t, None).unwrap_or(0));
            app.mfx.window = Some((t, k));
            ui.close();
        }
        ui.separator();
        let order = arp_k.and_then(|k| app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.extra.midi_fx.get(k)).and_then(|s| s.values.get(lg_dsp::midifx::arp::ORDER as usize).copied()));
        for (i, n) in ["Up", "Down", "Up/Down", "Outside-in", "Random", "As Played"].iter().enumerate() {
            if ui.add(egui::Button::selectable(order.is_some_and(|o| o.round() as usize == i), *n)).clicked() {
                let k = arp_k.or_else(|| toggle_arp(app, t, None));
                if let Some(k) = k {
                    crate::midifx::edit_slots(app, t, "Arpeggiator Note Order", |v| {
                        if let Some(s) = v.get_mut(k) {
                            let at = lg_dsp::midifx::arp::ORDER as usize;
                            if s.values.len() <= at {
                                s.values.resize(at + 1, 0.0);
                            }
                            s.values[at] = i as f32;
                        }
                    });
                }
                ui.close();
            }
        }
    });
    // Poly Pitch
    let pp = app.ps.wins[w].s.poly_pitch;
    pill(p, poly, if pp { BLUE } else { CTRL });
    label(p, poly, "Poly Pitch", Color32::WHITE);
    if ui.interact(poly, id.with("poly"), Sense::click()).on_hover_text("Polyphonic pitch bend: slide between keys").clicked() {
        app.ps.wins[w].s.poly_pitch = !pp;
    }
    // Chord Track Input
    let ci = app.ps.wins[w].s.chord_input;
    pill(p, chord_in, if ci { Color32::from_rgb(0x6a, 0x22, 0x22) } else { CTRL });
    crate::glyphs::paint(p, chord_in, crate::glyphs::ph_fill::MUSIC_NOTES_PLUS, 14.0, look::record_red());
    if ui.interact(chord_in, id.with("chordin"), Sense::click()).on_hover_text("Record to Chord Track").clicked() {
        app.ps.wins[w].s.chord_input = !ci;
    }
    // More
    pill(p, more, CTRL);
    let dots = Rect::from_min_max(more.min, pos2(more.min.x + 38.0, more.max.y));
    crate::glyphs::paint(p, dots, crate::glyphs::ph::DOTS_THREE, 15.0, Color32::WHITE);
    crate::glyphs::paint(p, Rect::from_min_max(pos2(dots.max.x, more.min.y), more.max), crate::glyphs::ph::CARET_DOWN, 11.0, Color32::from_gray(0xd0));
    let mr = ui.interact(more, id.with("more"), Sense::click()).on_hover_text("More");
    egui::Popup::menu(&mr).show(|ui| more_menu(app, ui, w, drums));
}

/// Inserts the Arpeggiator as the track's first MIDI plug-in, or (in) removes it. Returns its slot.
fn toggle_arp(app: &mut App, t: usize, at: Option<usize>) -> Option<usize> {
    match at {
        Some(k) => {
            crate::midifx::edit_slots(app, t, "Remove MIDI Plug-in", |v| {
                v.remove(k);
            });
            if app.mfx.window.is_some_and(|x| x.0 == t) {
                app.mfx.window = None;
            }
            None
        }
        None => {
            crate::midifx::edit_slots(app, t, "Insert MIDI Plug-in", |v| v.insert(0, crate::midifx::slot("Arpeggiator")));
            Some(0)
        }
    }
}

fn check(ui: &mut egui::Ui, on: bool, text: &str) -> bool {
    let r = ui.add(egui::Button::selectable(on, text)).clicked();
    if r {
        ui.close();
    }
    r
}

/// More (···): Logic's menu for the Play Surface.
fn more_menu(app: &mut App, ui: &mut egui::Ui, w: usize, drums: bool) {
    ui.set_min_width(250.0);
    let s = app.ps.wins[w].s.clone();
    ui.menu_button("Surface", |ui| {
        ui.set_min_width(190.0);
        for (v, n) in [(Surface::Auto, "Current Track (Auto)"), (Surface::Keys, "Keys & Chords"), (Surface::Drums, "Drum Pads")] {
            if check(ui, s.surface == v, n) {
                release_all(app, w);
                app.ps.wins[w].s.surface = v;
            }
        }
    });
    ui.separator();
    for (v, n) in [(Typing::Notes, "Play Notes"), (Typing::Chords, "Play Chords"), (Typing::Drums, "Play Drums")] {
        if check(ui, s.typing == v, n) {
            app.ps.wins[w].s.typing = v;
        }
    }
    if check(ui, s.key_letters, "Show Key Letters") {
        app.ps.wins[w].s.key_letters = !s.key_letters;
    }
    if check(ui, false, "Send Next Key to Logic Pro") {
        app.ps.wins[w].rt.bypass_next = true;
    }
    ui.separator();
    if check(ui, false, "Open Another Play Surface Window") {
        let mut n = Win::new(s.clone());
        n.s.custom = s.custom.clone();
        app.ps.wins.push(n);
    }
    ui.separator();
    if drums {
        if check(ui, s.pad_velocity, "Velocity") {
            app.ps.wins[w].s.pad_velocity = !s.pad_velocity;
        }
        if check(ui, s.pad_repeat, "Repeat") {
            app.ps.wins[w].s.pad_repeat = !s.pad_repeat;
        }
        return;
    }
    ui.menu_button("Rows", |ui| {
        for n in 1..=6u8 {
            if check(ui, s.rows == n, &n.to_string()) {
                app.ps.wins[w].s.rows = n;
            }
        }
    });
    ui.menu_button("Layout", |ui| {
        ui.set_min_width(150.0);
        for (v, n) in [(Layout::Fifths, "Circle of Fifths"), (Layout::Chromatic, "Chromatic")] {
            if check(ui, s.layout == v, n) {
                app.ps.wins[w].s.layout = v;
            }
        }
    });
    ui.menu_button("Root", |ui| {
        if check(ui, s.root.is_none(), "Auto") {
            app.ps.wins[w].s.root = None;
        }
        ui.separator();
        for (i, n) in NAMES.iter().enumerate() {
            if check(ui, s.root == Some(i as u8), n) {
                app.ps.wins[w].s.root = Some(i as u8);
            }
        }
    });
    ui.menu_button("Bass", |ui| {
        for (v, n) in [(0u8, "Off"), (12, "-12"), (24, "-24")] {
            if check(ui, s.bass == v, n) {
                app.ps.wins[w].s.bass = v;
            }
        }
    });
    ui.separator();
    if check(ui, app.ps.wins[w].rt.learn, "Learn Chords") {
        let l = !app.ps.wins[w].rt.learn;
        app.ps.wins[w].rt.learn = l;
        if l {
            app.ps.wins[w].s.show_custom = true;
        }
    }
    if check(ui, false, "Define Chords from Chord Track") {
        let text = if app.chord_text.trim().is_empty() { "C Am F G".to_string() } else { app.chord_text.clone() };
        app.ps.wins[w].s.custom = chord_track_chords(&text);
        app.ps.wins[w].s.show_custom = true;
        app.dirty = true;
    }
    if check(ui, false, "Define Chords from Key") {
        let (k, minor) = key_now(app);
        let k = app.ps.wins[w].s.root.unwrap_or(k);
        app.ps.wins[w].s.custom.extend(key_chords(k, minor));
        app.ps.wins[w].s.show_custom = true;
        app.dirty = true;
    }
    let last = app.ps.wins[w].rt.last_custom.filter(|k| *k < s.custom.len());
    if ui.add_enabled(last.is_some(), egui::Button::new("Delete last pressed Custom Chord")).clicked() {
        if let Some(k) = last {
            app.ps.wins[w].s.custom.remove(k);
            app.ps.wins[w].rt.last_custom = None;
            app.dirty = true;
        }
        ui.close();
    }
    if ui.add_enabled(!s.custom.is_empty(), egui::Button::new("Delete All Custom Chords")).clicked() {
        app.ps.wins[w].s.custom.clear();
        app.dirty = true;
        ui.close();
    }
    ui.separator();
    if check(ui, s.show_chords, "Show Chords") {
        app.ps.wins[w].s.show_chords = !s.show_chords;
    }
    if check(ui, s.show_custom, "Show Custom Chord Row") {
        app.ps.wins[w].s.show_custom = !s.show_custom;
    }
    if check(ui, s.show_keyboard, "Show Keyboard") {
        app.ps.wins[w].s.show_keyboard = !s.show_keyboard;
    }
    ui.separator();
    ui.menu_button("Y-Axis", |ui| {
        ui.set_min_width(180.0);
        for (v, n) in [(YAxis::Off, "Off"), (YAxis::Modulation, "Modulation"), (YAxis::ChannelPressure, "Channel Pressure"), (YAxis::PolyAftertouch, "Polyphonic Aftertouch"), (YAxis::Velocity, "Velocity")] {
            if check(ui, s.y_axis == v, n) {
                app.ps.wins[w].s.y_axis = v;
            }
        }
    });
    ui.menu_button("Size", |ui| {
        for (v, n) in [(0u8, "Small"), (1, "Medium"), (2, "Large")] {
            if check(ui, s.size == v, n) {
                app.ps.wins[w].s.size = v;
            }
        }
    });
}

/// The keyboard overview: all 128 notes, the piano's range highlighted (blue bar above).
fn overview(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, w: usize) {
    let keys = Rect::from_min_max(pos2(r.min.x, r.min.y + 6.0), pos2(r.max.x, r.max.y));
    p.rect_filled(keys, 2.0, Color32::from_gray(0x2a));
    let whites: Vec<u8> = (0..128u8).filter(|n| !IS_BLACK[*n as usize % 12]).collect();
    let ww = keys.width() / whites.len() as f32;
    let x_of = |n: u8| keys.min.x + (0..n).filter(|m| !IS_BLACK[*m as usize % 12]).count() as f32 * ww;
    let span = visible_whites(app, w) as f32;
    let lo = app.ps.wins[w].s.low;
    let x0 = x_of(lo);
    let x1 = (x0 + span * ww).min(keys.max.x);
    for (i, n) in whites.iter().enumerate() {
        let x = keys.min.x + i as f32 * ww;
        let inside = x >= x0 - 0.5 && x < x1 - 0.5;
        let kr = Rect::from_min_size(pos2(x + 0.3, keys.min.y + 1.0), vec2((ww - 0.6).max(0.4), keys.height() - 2.0));
        p.rect_filled(kr, 0.0, if inside { Color32::from_gray(0xf2) } else { Color32::from_gray(0x7a) });
        let _ = n;
    }
    for n in (0..128u8).filter(|n| IS_BLACK[*n as usize % 12]) {
        let kr = Rect::from_min_size(pos2(x_of(n) - ww * 0.32, keys.min.y + 1.0), vec2(ww * 0.64, (keys.height() - 2.0) * 0.62));
        p.rect_filled(kr, 0.0, Color32::from_gray(0x16));
    }
    p.rect_filled(Rect::from_min_max(pos2(x0, r.min.y + 1.0), pos2(x1, r.min.y + 3.6)), 1.3, BLUE);
    let resp = ui.interact(r, egui::Id::unique(("ps-overview", w)), Sense::click_and_drag()).on_hover_text("Keyboard overview");
    let white_at = |x: f32| whites[(((x - keys.min.x) / ww).floor().max(0.0) as usize).min(whites.len() - 1)];
    if resp.double_clicked() {
        app.ps.wins[w].s.low = 48;
        return;
    }
    if resp.drag_started() {
        if let Some(q) = resp.interact_pointer_pos().filter(|q| q.x >= x0 && q.x <= x1) {
            app.ps.wins[w].rt.range_drag = Some((q.x, lo));
        }
    }
    if let (Some((x_start, lo0)), Some(q)) = (app.ps.wins[w].rt.range_drag, resp.interact_pointer_pos()) {
        if resp.dragged() {
            let steps = ((q.x - x_start) / ww).round() as i32;
            let i0 = whites.iter().position(|n| *n == lo0).unwrap_or(0) as i32;
            let i = (i0 + steps).clamp(0, (whites.len() as i32 - span as i32).max(0));
            app.ps.wins[w].s.low = whites[i as usize];
        }
    }
    if resp.drag_stopped() {
        app.ps.wins[w].rt.range_drag = None;
    }
    if resp.clicked() {
        if let Some(q) = resp.interact_pointer_pos() {
            // beside the range: an octave that way
            if q.x < x0 {
                app.ps.wins[w].s.low = lo.saturating_sub(12);
            } else if q.x > x1 {
                app.ps.wins[w].s.low = (lo + 12).min(108);
            }
            let _ = white_at;
        }
    }
}

fn key_width(size: u8) -> f32 {
    [38.0, 52.0, 66.0][size.min(2) as usize]
}

fn visible_whites(app: &App, w: usize) -> usize {
    let width = if app.ps.wins[w].rt.piano_w > 0.0 { app.ps.wins[w].rt.piano_w } else { 1309.0 };
    ((width / key_width(app.ps.wins[w].s.size)).ceil() as usize).max(7)
}


/// A vertical slider of the side column; returns the new value when dragged.
fn vslider(ui: &mut egui::Ui, p: &egui::Painter, r: Rect, id: egui::Id, v: f32, col: Color32, text: Option<String>) -> (Option<f32>, egui::Response) {
    p.rect_filled(r, 6.0, Color32::from_gray(0x10));
    p.rect_stroke(r, 6.0, Stroke::new(1.0, Color32::from_gray(0x30)), StrokeKind::Inside);
    let inner = r.shrink2(vec2(0.0, 10.0));
    let y = inner.max.y - v.clamp(0.0, 1.0) * inner.height();
    p.line_segment([pos2(r.center().x - 9.5, y), pos2(r.center().x + 9.5, y)], Stroke::new(3.0, col));
    if let Some(t) = text {
        p.text(pos2(r.center().x, y + 12.0), Align2::CENTER_CENTER, t, look::regular(10.5), Color32::WHITE);
    }
    let resp = ui.interact(r, id, Sense::click_and_drag());
    let nv = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()).map(|q| ((inner.max.y - q.y) / inner.height()).clamp(0.0, 1.0));
    (nv, resp)
}

fn side_label(p: &egui::Painter, r: Rect, y: f32, t: &str) {
    p.text(pos2(r.min.x + 33.7, r.min.y + y), Align2::CENTER_CENTER, t, look::regular(12.0), Color32::from_gray(0xd8));
}

fn side_slots(r: Rect) -> [(f32, Rect); 3] {
    let h = ((r.height() - 36.0 - 16.5) / 3.0).max(60.0);
    let mk = |k: f32| {
        let top = r.min.y + 36.8 + k * h;
        (top + 7.7, Rect::from_min_max(pos2(r.min.x + 13.2, top + 21.0), pos2(r.min.x + 54.2, top + h - 4.0)))
    };
    [mk(0.0), mk(1.0), mk(2.0)]
}

/// Keys & Chords: Velocity, Modulation and Pitch Bend (which springs back).
fn key_side(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, w: usize) {
    let [(ly0, s0), (ly1, s1), (ly2, s2)] = side_slots(r);
    side_label(p, r, ly0 - r.min.y, "Velocity");
    side_label(p, r, ly1 - r.min.y, "Mod");
    side_label(p, r, ly2 - r.min.y, "Pitch");
    let vel = app.ps.wins[w].s.velocity;
    if let (Some(v), _) = vslider(ui, p, s0, egui::Id::unique(("ps-vel", w)), vel as f32 / 127.0, BLUE, Some(vel.to_string())) {
        app.ps.wins[w].s.velocity = (v * 127.0).round().clamp(1.0, 127.0) as u8;
    }
    let m = app.ps.wins[w].rt.modulation;
    if let (Some(v), _) = vslider(ui, p, s1, egui::Id::unique(("ps-mod", w)), m as f32 / 127.0, ORANGE, None) {
        let nv = (v * 127.0).round() as u8;
        if nv != m {
            app.ps.wins[w].rt.modulation = nv;
            control(app, 1, nv as u16);
        }
    }
    let b = app.ps.wins[w].rt.bend;
    let (nv, resp) = vslider(ui, p, s2, egui::Id::unique(("ps-bend", w)), (b as f32 + 8192.0) / 16383.0, BLUE, None);
    if let Some(v) = nv {
        let nb = (v * 16383.0 - 8192.0).round() as i16;
        if nb != b {
            app.ps.wins[w].rt.bend = nb;
            control(app, 128, (nb as i32 + 8192).clamp(0, 16383) as u16);
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        app.ps.wins[w].rt.bend = 0;
        control(app, 128, 8192);
    }
}

/// Drum Pads: Velocity, Gate and Rate (note repeat while held; springs back to off).
fn drum_side(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, w: usize) {
    let [(ly0, s0), (ly1, s1), (ly2, s2)] = side_slots(r);
    side_label(p, r, ly0 - r.min.y, "Velocity");
    side_label(p, r, ly1 - r.min.y, "Gate");
    side_label(p, r, ly2 - r.min.y, "Rate");
    let vel = app.ps.wins[w].s.velocity;
    if let (Some(v), _) = vslider(ui, p, s0, egui::Id::unique(("ps-vel", w)), vel as f32 / 127.0, BLUE, Some(vel.to_string())) {
        app.ps.wins[w].s.velocity = (v * 127.0).round().clamp(1.0, 127.0) as u8;
    }
    let g = app.ps.wins[w].s.gate;
    if let (Some(v), _) = vslider(ui, p, s1, egui::Id::unique(("ps-gate", w)), g as f32 / 100.0, ORANGE, Some(format!("{g}%"))) {
        app.ps.wins[w].s.gate = (v * 100.0).round().clamp(1.0, 100.0) as u8;
    }
    let rate = app.ps.wins[w].rt.rate;
    let steps = (RATES.len() - 1) as f32;
    let (nv, resp) = vslider(ui, p, s2, egui::Id::unique(("ps-rate", w)), rate as f32 / steps, BLUE, Some(RATES[rate].0.to_string()));
    if let Some(v) = nv {
        app.ps.wins[w].rt.rate = (v * steps).round() as usize;
    }
    if resp.drag_stopped() || resp.clicked() {
        app.ps.wins[w].rt.rate = 0;
    }
}

/// The Chord Strips: the predefined rows, then the custom row.
fn strips(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect, w: usize) {
    let s = app.ps.wins[w].s.clone();
    let rows: Vec<Option<Base>> = if s.show_chords { rows_top_down(s.rows).into_iter().map(Some).collect() } else { Vec::new() };
    let mut all: Vec<i32> = (0..rows.len() as i32).collect();
    if s.show_custom {
        all.push(-1);
    }
    let gap = 4.2;
    let rh = ((area.height() - gap * (all.len() as f32 - 1.0)) / all.len() as f32).min(73.0);
    let cw = (area.width() - gap * 11.0) / 12.0;
    // narrow strips split top / bottom instead of left / right
    let vertical = cw < rh * 0.8;
    let key = root_key(app, &s);
    let roots = strip_roots(s.layout, key);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let down = ui.input(|i| i.pointer.primary_down());
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let alt = ui.input(|i| i.modifiers.alt);
    let ctrl = ui.input(|i| i.modifiers.ctrl || i.pointer.secondary_pressed());
    let mut hit: Option<(i32, usize, i8)> = None;
    let mut on_area = false;
    for (ri, row) in all.iter().enumerate() {
        let y = area.min.y + ri as f32 * (rh + gap);
        for c in 0..12usize {
            let sr = Rect::from_min_size(pos2(area.min.x + c as f32 * (cw + gap), y), vec2(cw, rh));
            let (text, dark, empty) = if *row >= 0 {
                let (root, iv, _) = strip_chord(app, &s, *row as usize, c);
                let _ = rows[*row as usize];
                (format!("{}{}", NAMES[root as usize], suffix(&iv)), IS_BLACK[roots[c] as usize], false)
            } else {
                match s.custom.get(c) {
                    Some(cc) => (cc.name.clone(), IS_BLACK[(cc.notes[0] % 12) as usize], false),
                    None => (String::new(), false, true),
                }
            };
            let playing = app.ps.wins[w].rt.strip.is_some_and(|x| x.0 == *row && x.1 == c);
            let fill = if empty {
                Color32::from_gray(0x2c)
            } else if playing {
                BLUE
            } else if dark {
                Color32::from_gray(0xa8)
            } else {
                Color32::from_gray(0xdb)
            };
            p.rect_filled(sr, 5.6, fill);
            if empty {
                p.rect_stroke(sr, 5.6, Stroke::new(1.0, Color32::from_gray(0x3c)), StrokeKind::Inside);
                if app.ps.wins[w].rt.learn {
                    p.text(sr.center(), Align2::CENTER_CENTER, "+", look::regular(16.0), Color32::from_gray(0x80));
                }
            } else {
                let line = if playing { Color32::from_rgb(0x5a, 0x9a, 0xf0) } else if dark { Color32::from_gray(0x96) } else { Color32::from_gray(0xc6) };
                if vertical {
                    for k in [1.0, 2.0] {
                        let yy = sr.min.y + sr.height() * k / 3.0;
                        p.hline((sr.min.x + 6.0)..=(sr.max.x - 6.0), yy, Stroke::new(1.0, line));
                    }
                } else {
                    for k in [1.0, 2.0] {
                        let xx = sr.min.x + sr.width() * k / 3.0;
                        p.vline(xx, (sr.min.y + 6.0)..=(sr.max.y - 6.0), Stroke::new(1.0, line));
                    }
                }
                p.text(sr.center(), Align2::CENTER_CENTER, &text, look::semibold(12.5), if playing { Color32::WHITE } else { Color32::from_gray(0x6e) });
            }
            if s.key_letters && s.typing == Typing::Chords && *row >= 0 {
                let from_bottom = rows.len() - 1 - *row as usize;
                if from_bottom < 2 {
                    p.text(pos2(sr.min.x + 7.0, sr.max.y - 9.0), Align2::LEFT_CENTER, CHORD_LETTERS[from_bottom][c], look::semibold(10.0), Color32::from_gray(0x80));
                }
            }
            let resp = ui.interact(sr, egui::Id::unique(("ps-strip", w, *row, c)), Sense::click_and_drag());
            if *row < 0 {
                // Control-click a custom strip: delete it (or all); on an empty one: Learn Chords
                egui::Popup::context_menu(&resp).show(|ui| {
                    if !empty && ui.button("Delete Custom Chord").clicked() {
                        app.ps.wins[w].s.custom.remove(c);
                        app.ps.wins[w].rt.last_custom = None;
                        app.dirty = true;
                        ui.close();
                    }
                    if ui.add_enabled(!s.custom.is_empty(), egui::Button::new("Delete All Custom Chords")).clicked() {
                        app.ps.wins[w].s.custom.clear();
                        app.dirty = true;
                        ui.close();
                    }
                    if empty && ui.button("Learn Chords").clicked() {
                        app.ps.wins[w].rt.learn = true;
                        ui.close();
                    }
                });
            } else if resp.secondary_clicked() {
                let _ = ctrl;
            }
            if let Some(q) = pointer.filter(|q| sr.contains(*q)) {
                on_area = true;
                if !empty {
                    let inv = if vertical {
                        let f = (q.y - sr.min.y) / sr.height();
                        if f < 1.0 / 3.0 { 1 } else if f > 2.0 / 3.0 { -1 } else { 0 }
                    } else {
                        let f = (q.x - sr.min.x) / sr.width();
                        if f < 1.0 / 3.0 { -1 } else if f > 2.0 / 3.0 { 1 } else { 0 }
                    };
                    hit = Some((*row, c, inv));
                }
            }
        }
    }
    let cur = app.ps.wins[w].rt.strip;
    let dragging_from_here = cur.is_some() && down;
    let new = if (pressed || dragging_from_here) && down { hit } else { None };
    if new != cur {
        if cur.is_some() {
            let notes = std::mem::take(&mut app.ps.wins[w].rt.strip_notes);
            notes_off(app, &notes);
        }
        app.ps.wins[w].rt.strip = None;
        if let Some((row, c, inv)) = new {
            if pressed || cur.is_some() {
                play_strip(app, w, row, c, inv, pressed && alt && row >= 0);
                app.ps.wins[w].rt.strip = Some((row, c, inv));
            }
        }
    }
    let _ = on_area;
}

/// The onscreen piano from the overview's lowest note; a click plays at the Velocity slider's
/// value, a vertical drag sends the Y-Axis choice, Poly Pitch slides between keys.
fn piano(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect, w: usize) {
    let s = app.ps.wins[w].s.clone();
    app.ps.wins[w].rt.piano_w = area.width();
    let kw = key_width(s.size);
    let mut whites: Vec<u8> = Vec::new();
    let mut n = s.low;
    while whites.len() as f32 * kw < area.width() && n <= 127 {
        if !IS_BLACK[n as usize % 12] {
            whites.push(n);
        }
        n += 1;
    }
    let held: Vec<u8> = app.live.held.keys().copied().collect();
    let clip = p.with_clip_rect(area);
    let mut rects: Vec<(u8, Rect)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        let kr = Rect::from_min_size(pos2(area.min.x + i as f32 * kw, area.min.y), vec2(kw - 1.2, area.height()));
        let on = held.contains(n);
        clip.rect_filled(kr, CornerRadius { nw: 0, ne: 0, sw: 5, se: 5 }, if on { BLUE } else { Color32::from_gray(0xe6) });
        if n % 12 == 0 {
            clip.text(pos2(kr.center().x, kr.max.y - 16.0), Align2::CENTER_CENTER, format!("C{}", *n as i32 / 12 - 2), look::semibold(12.0), Color32::from_gray(0x8a));
        }
        if s.key_letters && s.typing == Typing::Notes {
            if let Some(k) = letter(s.octave_c, *n) {
                clip.text(pos2(kr.center().x, area.min.y + area.height() * 0.712), Align2::CENTER_CENTER, k, look::semibold(12.5), Color32::from_gray(0x8a));
            }
        }
        rects.push((*n, kr));
    }
    let mut blacks: Vec<(u8, Rect)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        let b = n + 1;
        if b > 127 || !IS_BLACK[b as usize % 12] {
            continue;
        }
        let kr = Rect::from_min_size(pos2(area.min.x + (i + 1) as f32 * kw - kw * 0.335 - 0.6, area.min.y), vec2(kw * 0.67, area.height() * 0.653));
        let on = held.contains(&b);
        clip.rect_filled(kr.expand(1.0), CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 }, Color32::from_gray(0x55));
        clip.rect_filled(kr, CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 }, if on { BLUE } else { Color32::from_gray(0x1b) });
        clip.rect_filled(Rect::from_min_max(pos2(kr.min.x + 2.5, kr.max.y - 9.0), pos2(kr.max.x - 2.5, kr.max.y - 2.5)), 2.0, if on { BLUE } else { Color32::from_gray(0x2c) });
        if s.key_letters && s.typing == Typing::Notes {
            if let Some(k) = letter(s.octave_c, b) {
                clip.text(pos2(kr.center().x, kr.max.y - 17.0), Align2::CENTER_CENTER, k, look::semibold(12.5), Color32::from_gray(0xa0));
            }
        }
        blacks.push((b, kr));
    }
    let resp = ui.interact(area, egui::Id::unique(("ps-piano", w)), Sense::click_and_drag());
    let pos = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on());
    let hit = pos.and_then(|q| blacks.iter().chain(rects.iter()).find(|(_, kr)| kr.contains(q)).map(|(n, _)| *n));
    let cur = app.ps.wins[w].rt.mouse_key;
    // the Y-Axis: a vertical drag on the held key
    if let (Some(q), Some(k)) = (pos, cur) {
        if resp.dragged() {
            let dy = app.ps.wins[w].rt.press_y - q.y;
            let v = (app.ps.wins[w].rt.press_val as f32 + dy * 0.8).clamp(0.0, 127.0) as u8;
            match s.y_axis {
                YAxis::Off => {}
                YAxis::Modulation => {
                    app.ps.wins[w].rt.modulation = v;
                    control(app, 1, v as u16);
                }
                YAxis::ChannelPressure => message(app, 0xD0, v, 0),
                YAxis::PolyAftertouch => message(app, 0xA0, k, v),
                YAxis::Velocity => {
                    let v = v.max(1);
                    if let Some(h) = app.live.held.get_mut(&k) {
                        h.1 = v;
                    }
                    app.ps.wins[w].s.velocity = v;
                }
            }
        }
    }
    if hit != cur && (hit.is_some() || pos.is_none()) {
        // Poly Pitch: sliding from the first key bends it (up to two semitones away)
        if s.poly_pitch && pos.is_some() {
            if let (Some(first), Some(h)) = (app.ps.wins[w].rt.first_key, hit) {
                let d = h as i32 - first as i32;
                if d.abs() <= 2 && cur.is_some() {
                    let v = (8192 + d * 4096).clamp(0, 16383) as u16;
                    control(app, 128, v);
                    return;
                }
            }
        }
        if let Some(k) = cur {
            notes_off(app, &[k]);
            app.ps.wins[w].rt.mouse_key = None;
            if s.poly_pitch {
                control(app, 128, 8192);
            }
        }
        if let (Some(k), Some(q)) = (hit, pos) {
            notes_on(app, &[k], s.velocity);
            let rt = &mut app.ps.wins[w].rt;
            rt.mouse_key = Some(k);
            if cur.is_none() {
                rt.first_key = Some(k);
            }
            rt.press_y = q.y;
            rt.press_val = match s.y_axis {
                YAxis::Velocity => s.velocity,
                YAxis::Modulation => rt.modulation,
                _ => 0,
            };
        } else {
            app.ps.wins[w].rt.first_key = None;
        }
    }
}

/// The drum pads: 4 × 4, the page's lowest note bottom-left. More ▸ Velocity: the click's height
/// sets the velocity (scale on the right); More ▸ Repeat: its left-right place the repeat rate
/// (scale along the bottom).
fn pads(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect, w: usize) {
    let s = app.ps.wins[w].s.clone();
    let notes = page_notes(s.page);
    let gap = 6.0;
    let pw = (area.width() - gap * 3.0) / 4.0;
    let ph = (area.height() - gap * 3.0) / 4.0;
    let held: Vec<u8> = app.live.held.keys().copied().collect();
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let down = ui.input(|i| i.pointer.primary_down());
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let mut hit: Option<(u8, f32, f32)> = None;
    for (i, n) in notes.iter().enumerate() {
        let (col, row_from_bottom) = (i % 4, i / 4);
        let pr = Rect::from_min_size(pos2(area.min.x + col as f32 * (pw + gap), area.max.y - (row_from_bottom + 1) as f32 * ph - row_from_bottom as f32 * gap), vec2(pw, ph));
        let on = held.contains(n);
        p.rect_filled(pr, 7.0, if on { BLUE } else { Color32::from_gray(0x3a) });
        p.rect_stroke(pr, 7.0, Stroke::new(1.0, Color32::from_gray(0x4c)), StrokeKind::Inside);
        p.text(pos2(pr.min.x + 10.0, pr.min.y + 14.0), Align2::LEFT_CENTER, pad_name(*n), look::semibold(12.5), Color32::WHITE);
        p.text(pos2(pr.min.x + 10.0, pr.min.y + 30.0), Align2::LEFT_CENTER, note_name(*n), look::regular(10.5), Color32::from_gray(0xa0));
        if s.key_letters && (s.typing == Typing::Drums || shows_drums(app, &s)) {
            p.text(pos2(pr.max.x - 10.0, pr.min.y + 14.0), Align2::RIGHT_CENTER, DRUM_LETTERS[i], look::semibold(11.0), Color32::from_gray(0x90));
        }
        if s.pad_velocity {
            // the velocity scale along the right
            for k in 0..=4 {
                let y = pr.max.y - 8.0 - (pr.height() - 16.0) * k as f32 / 4.0;
                p.hline((pr.max.x - 9.0)..=(pr.max.x - 4.0), y, Stroke::new(1.0, Color32::from_gray(0x80)));
            }
        }
        if s.pad_repeat {
            // the rates along the bottom
            let rates = &RATES[1..];
            for (k, (rn, _)) in rates.iter().enumerate() {
                let x = pr.min.x + 8.0 + (pr.width() - 16.0) * (k as f32 + 0.5) / rates.len() as f32;
                p.text(pos2(x, pr.max.y - 9.0), Align2::CENTER_CENTER, *rn, look::regular(9.0), Color32::from_gray(0x9a));
            }
        }
        if let Some(q) = pointer.filter(|q| pr.contains(*q)) {
            hit = Some((*n, (q.x - pr.min.x) / pr.width(), (pr.max.y - q.y) / pr.height()));
        }
        let _ = ui.interact(pr, egui::Id::unique(("ps-pad", w, i)), Sense::click_and_drag());
    }
    let cur = app.ps.wins[w].rt.pad;
    let new = if down && (pressed || cur.is_some()) { hit } else { None };
    if new.map(|h| h.0) != cur {
        if let Some(n) = cur {
            if !app.ps.wins[w].rt.reps.iter().any(|r| r.pitch == n && r.sounding) {
                notes_off(app, &[n]);
            }
            app.ps.wins[w].rt.pad = None;
            if s.pad_repeat {
                app.ps.wins[w].rt.rate = 0;
            }
        }
        if let Some((n, fx, fy)) = new {
            let vel = if s.pad_velocity { (fy * 127.0).round().clamp(1.0, 127.0) as u8 } else { s.velocity };
            if s.pad_repeat {
                app.ps.wins[w].rt.rate = 1 + ((fx * (RATES.len() - 1) as f32).floor() as usize).min(RATES.len() - 2);
            }
            notes_on(app, &[n], vel);
            app.ps.wins[w].rt.pad = Some(n);
            if let Some(r) = app.ps.wins[w].rt.reps.iter_mut().find(|r| r.pitch == n) {
                r.vel = vel;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_power_minor_major_by_default() {
        assert_eq!(rows_top_down(3), vec![Base::Power, Base::Minor, Base::Major]);
        assert_eq!(rows_top_down(6).len(), 6);
        assert_eq!(rows_top_down(1), vec![Base::Major]);
        // the modifier a row already shows goes
        assert!(mod_available(3, 0) && !mod_available(4, 0));
        assert!(mod_available(4, 1) && !mod_available(5, 1));
        assert!(!mod_available(6, 2) && mod_available(6, 3));
    }

    #[test]
    fn chord_types_and_modifiers() {
        let none = [false; 4];
        assert_eq!(suffix(&intervals(Base::Major, none)), "");
        assert_eq!(suffix(&intervals(Base::Minor, none)), "m");
        assert_eq!(suffix(&intervals(Base::Power, none)), "5");
        // 7: a major seventh on major, a minor one on minor
        assert_eq!(suffix(&intervals(Base::Major, [true, false, false, false])), "maj7");
        assert_eq!(suffix(&intervals(Base::Minor, [true, false, false, false])), "m7");
        // Sus: the third becomes a fourth (with Alt a second)
        assert_eq!(suffix(&intervals(Base::Major, [false, true, false, false])), "sus4");
        assert_eq!(suffix(&intervals(Base::Minor, [false, true, false, true])), "sus2");
        // Dim: third and fifth flattened
        assert_eq!(intervals(Base::Major, [false, false, true, false]), vec![0, 3, 6]);
        assert_eq!(suffix(&intervals(Base::Minor, [true, false, true, false])), "m7♭5");
        // Alt: augmented or extended
        assert_eq!(suffix(&intervals(Base::Major, [false, false, false, true])), "aug");
        assert_eq!(suffix(&intervals(Base::Minor, [false, false, false, true])), "m(add9)");
        assert_eq!(suffix(&intervals(Base::Major, [true, false, false, true])), "maj9");
    }

    #[test]
    fn inversions_and_bass() {
        let iv = intervals(Base::Major, [false; 4]);
        assert_eq!(voicing(0, &iv, 0, 0), vec![60, 64, 67]);
        // right side: one inversion higher; left: one lower
        assert_eq!(voicing(0, &iv, 1, 0), vec![64, 67, 72]);
        assert_eq!(voicing(0, &iv, -1, 0), vec![55, 60, 64]);
        // Bass −12 / −24: the root below
        assert_eq!(voicing(0, &iv, 0, 12), vec![48, 60, 64, 67]);
        assert_eq!(voicing(0, &iv, 0, 24)[0], 36);
        // G to B sit below C3
        assert_eq!(voicing(7, &iv, 0, 0)[0], 55);
    }

    #[test]
    fn layouts() {
        assert_eq!(strip_roots(Layout::Chromatic, 5), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        // fifths from the middle strip, on the key
        let f = strip_roots(Layout::Fifths, 0);
        assert_eq!(f[6], 0);
        assert_eq!(f[7], 7);
        assert_eq!(f[5], 5);
        assert_eq!(f[11], 11);
        let g = strip_roots(Layout::Fifths, 7);
        assert_eq!(g[6], 7);
        assert_eq!(g[7], 2);
    }

    #[test]
    fn chords_from_key_and_chord_track() {
        let k = key_chords(0, false);
        let names: Vec<&str> = k.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["F", "C", "G", "Dm", "Am", "Em", "Bdim"]);
        // A minor: its relative major's chords
        assert_eq!(key_chords(9, true)[1].name, "C");
        let t = chord_track_chords("C Am F G C Am7 F♯m Bb");
        let names: Vec<&str> = t.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["C", "Am", "F", "G", "Am7", "F♯m", "Bb"]);
        assert_eq!(t[4].notes, vec![57, 60, 64, 67]);
        assert_eq!(t[6].notes, vec![58, 62, 65]);
        assert_eq!(parse_chord("Csus4"), Some((0, vec![0, 5, 7])));
        assert_eq!(parse_chord("Ebmaj7"), Some((3, vec![0, 4, 7, 11])));
        assert_eq!(parse_chord("x"), None);
    }

    #[test]
    fn chord_track_input_puts_the_chord_at_the_bar() {
        assert_eq!(chord_track_set("C Am F G", 1, "Dm"), "C Dm F G");
        assert_eq!(chord_track_set("C Am", 4, "E"), "C Am Am Am E");
        assert_eq!(chord_track_set("", 0, "F"), "F");
    }

    #[test]
    fn pad_pages() {
        assert_eq!(page_notes(2)[0], 36);
        assert_eq!(page_notes(2).len(), 16);
        assert_eq!(page_notes(3)[0], 52);
        assert_eq!(page_notes(7).last(), Some(&127));
        assert_eq!(pad_name(36), "Kick 1");
        assert_eq!(pad_name(60), "Hi Bongo");
        assert_eq!(pad_name(100), "E6");
        assert_eq!(RATES.iter().skip(1).map(|r| r.0).collect::<Vec<_>>(), vec!["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"]);
    }

    #[test]
    fn settings_round_trip() {
        let mut a = Settings::default();
        a.surface = Surface::Drums;
        a.typing = Typing::Chords;
        a.rows = 5;
        a.layout = Layout::Fifths;
        a.root = Some(3);
        a.bass = 24;
        a.mods = [true, false, true, false];
        a.custom = vec![Custom { name: "F♯m".into(), notes: vec![54, 57, 61] }];
        a.y_axis = YAxis::PolyAftertouch;
        a.size = 2;
        a.low = 36;
        a.velocity = 77;
        a.pad_repeat = true;
        a.page = 4;
        a.gate = 30;
        a.chord_input = true;
        a.sustain_locked = true;
        let b = Settings::default();
        let back = from_text(&to_text(&[a.clone(), b.clone()]));
        assert_eq!(back, vec![a, b]);
    }

    #[test]
    fn typing_letters() {
        assert_eq!(letter(60, 60), Some("A"));
        assert_eq!(letter(60, 61), Some("W"));
        assert_eq!(letter(60, 77), Some("'"));
        assert_eq!(letter(60, 59), None);
        assert_eq!(NOTE_KEYS.len(), 18);
        assert_eq!(DRUM_KEYS[0], egui::Key::Z);
    }
}
