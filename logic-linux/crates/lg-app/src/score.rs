//! The Score editor (Logic Pro 13's Score tab): MIDI regions as notation in Linear, Wrapped and
//! Page view, on the track's staff style (Treble, Bass, Piano, Alto, Tenor, Drums, transposing
//! instruments, 8va / 8vb), in the project's key (and the key changes inserted here) and time
//! signature. Notes are quantised for display (the Quantize, Interpretation and Syncopation
//! parameters), with rests filled in, ties, chords on one stem, beams and flags, triplets,
//! ledger lines, accidentals by key, and voice separation (two voices on a staff, or one staff
//! each with Explode Polyphony). Engraving decisions are lg-notation's.
//!
//! Editing as Logic's: a click selects a note (Shift adds), a drag moves it diatonically and in
//! time, a double-click inserts the Part box's symbol (a note of its value, an articulation,
//! a dynamic, a slur or hairpin, a clef change, text, a lyric or a chord symbol), Delete
//! removes the selection. A Part box symbol clicked with notes selected is attached to them.
//! Per-note attributes, staff styles and symbols live in the project (lg-core score), so every
//! edit is one Undo step. The key commands are scorecmd.rs's.

use egui::epaint::{PathShape, PathStroke};
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke};
use lg_core::events::Note;
use lg_core::score::{NoteAttr, Symbol, SymbolKind, ARTICULATIONS};
use lg_notation as nt;

use crate::app::App;
use crate::look;
use crate::scoreinsp::Dialog;

const TABS_H: f32 = 40.0;
const LOCAL_H: f32 = 28.0;
const RULER_H: f32 = 26.0;
const HEAD_W: f32 = 120.0;

/// Which editor the editor area shows (Logic's tabs; pattern regions always open the Step
/// Sequencer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorTab {
    #[default]
    PianoRoll,
    Score,
    StepSequencer,
}

fn tab_id() -> egui::Id {
    egui::Id::unique("lg-editor-tab")
}

/// The editor to show for the selected region.
pub fn editor_tab(app: &App, ctx: &egui::Context) -> EditorTab {
    if let Some((t, r)) = app.selected_region {
        if crate::stepseq::is_pattern(ctx, t, r) {
            return EditorTab::StepSequencer;
        }
    }
    match ctx.data(|d| d.get_temp::<EditorTab>(tab_id())).unwrap_or_default() {
        EditorTab::StepSequencer => EditorTab::PianoRoll,
        t => t,
    }
}

pub fn set_editor_tab(ctx: &egui::Context, tab: EditorTab) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), tab));
}

/// The key-command area for the editor under the pointer (keymap.rs).
pub fn focus(app: &App, ctx: &egui::Context) -> crate::keymap::Focus {
    use crate::keymap::Focus;
    if crate::audioedit::is_audio_region(app) {
        return Focus::PianoRoll;
    }
    let players = app.selected_region.is_some_and(|x| app.players.contains_key(&x));
    match editor_tab(app, ctx) {
        EditorTab::Score if !players => Focus::Score,
        EditorTab::StepSequencer => Focus::StepSeq,
        _ => Focus::PianoRoll,
    }
}

/// Logic's editor tabs, Piano Roll | Score | Smart Tempo, in one capsule; a click switches.
pub fn tabs(app: &mut App, ui: &mut egui::Ui, area: Rect, current: EditorTab) {
    let p = ui.painter_at(area);
    let names = ["Piano Roll", "Score", "Smart Tempo"];
    let tw = 150.0;
    let tx0 = area.center().x - tw * 1.5;
    let pill = Rect::from_min_size(pos2(tx0, area.min.y + 7.0), vec2(tw * 3.0, 26.0));
    p.rect_filled(pill, 8.0, look::group_bg());
    let on = match current {
        EditorTab::Score => 1,
        _ => 0,
    };
    for (i, n) in names.iter().enumerate() {
        let r = Rect::from_min_size(pos2(tx0 + i as f32 * tw, area.min.y + 7.0), vec2(tw, 26.0));
        if i == on {
            p.rect_filled(r.shrink(2.0), 7.0, look::accent());
        }
        p.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if i == on { Color32::WHITE } else { look::text() });
        if i != on && ui.interact(r, ui.scope_id().with("lg-sc").with(("edtab", i)), Sense::click()).clicked() {
            match i {
                0 => set_editor_tab(ui.ctx(), EditorTab::PianoRoll),
                1 => set_editor_tab(ui.ctx(), EditorTab::Score),
                _ => app.tell(format!("The {n} editor isn't available yet")),
            }
        }
    }
}

/// View ▸ View Mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Linear,
    Wrapped,
    Page,
}

/// View ▸ Colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Colors {
    #[default]
    Normal,
    Pitch,
    Velocity,
    Voice,
    BlackWhite,
}

/// A Part box symbol (group, item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Note(usize),
    Rest(usize),
    Artic(usize),
    Dynamic(usize),
    /// Slur up, slur down, crescendo, decrescendo.
    Line(usize),
    Clef(usize),
    /// Text, lyric, chord symbol, then the automatic texts (REGION, INSTR, SONG, DATE).
    Text(usize),
    /// A note head (lg-core NOTE_HEADS).
    Head(usize),
    /// Sustain pedal down (Ped) or up (✱).
    Pedal(usize),
    /// Bar lines, repeats, endings, segno and coda (lg-core BarKind::ALL).
    Bar(usize),
    /// Trills, ornaments, arpeggio, tremolo, trill accidentals (lg-core ORNAMENTS).
    Ornament(usize),
    /// A key signature (fifths + 7).
    Key(usize),
    /// A time signature (TIMES).
    Time(usize),
    /// Line Break, Page Break, No Line Break, No Page Break.
    Break(usize),
}

/// The Part box's symbol groups, in its group buttons' order: (name, number of symbols).
pub const GROUP_NAMES: [&str; 14] = ["Notes", "Pedal", "Clefs", "Dynamics", "Note Heads", "Accents", "Slurs and Crescendi", "Key Signatures", "Time Signatures", "Bars and Repeats", "Trills", "Rests", "Text", "Layout"];

impl Part {
    /// (group, item) and the groups' sizes, for Next / Previous Part Box Symbol and Group.
    pub const GROUPS: [usize; 14] = [7, 2, 5, 8, 12, 10, 4, 15, 11, 13, 13, 6, 7, 4];
    pub fn index(self) -> (usize, usize) {
        match self {
            Part::Note(i) => (0, i),
            Part::Pedal(i) => (1, i),
            Part::Clef(i) => (2, i),
            Part::Dynamic(i) => (3, i),
            Part::Head(i) => (4, i),
            Part::Artic(i) => (5, i),
            Part::Line(i) => (6, i),
            Part::Key(i) => (7, i),
            Part::Time(i) => (8, i),
            Part::Bar(i) => (9, i),
            Part::Ornament(i) => (10, i),
            Part::Rest(i) => (11, i),
            Part::Text(i) => (12, i),
            Part::Break(i) => (13, i),
        }
    }
    pub fn from_index(g: usize, i: usize) -> Part {
        let g = g % Part::GROUPS.len();
        let i = i.min(Part::GROUPS[g] - 1);
        match g {
            0 => Part::Note(i),
            1 => Part::Pedal(i),
            2 => Part::Clef(i),
            3 => Part::Dynamic(i),
            4 => Part::Head(i),
            5 => Part::Artic(i),
            6 => Part::Line(i),
            7 => Part::Key(i),
            8 => Part::Time(i),
            9 => Part::Bar(i),
            10 => Part::Ornament(i),
            11 => Part::Rest(i),
            12 => Part::Text(i),
            _ => Part::Break(i),
        }
    }
    /// The symbol's name (help tags, undo).
    pub fn name(self) -> String {
        match self {
            Part::Note(i) => format!("{} Note", VALUES[i].0),
            Part::Rest(i) => format!("{} Rest", VALUES[i].0),
            Part::Artic(i) => ARTICULATIONS[PART_ARTICS[i]].to_string(),
            Part::Dynamic(i) => DYNAMICS[i].to_string(),
            Part::Line(i) => LINES[i].to_string(),
            Part::Clef(i) => format!("{} Clef", CLEFS[i]),
            Part::Text(i) => TEXTS[i].to_string(),
            Part::Head(i) => format!("Note Head: {}", lg_core::score::NOTE_HEADS[i]),
            Part::Pedal(i) => if i == 0 { "Sustain Pedal Down".into() } else { "Sustain Pedal Up".into() },
            Part::Bar(i) => lg_core::score::BarKind::ALL[i].name().to_string(),
            Part::Ornament(i) => lg_core::score::ORNAMENTS[i].to_string(),
            Part::Key(i) => key_name(i as i8 - 7),
            Part::Time(i) => format!("Time Signature {}", TIMES[i]),
            Part::Break(i) => lg_core::score::Break::ALL[i].name().to_string(),
        }
    }
}

/// The Part box's time signatures; the last is the undefined one (asks for its values).
pub const TIMES: [&str; 11] = ["2/4", "3/4", "4/4", "5/4", "6/8", "7/8", "9/8", "12/8", "2/2", "3/8", "A/B"];

/// The Part box's articulations (indices into lg-core's ARTICULATIONS).
pub const PART_ARTICS: [usize; 10] = [2, 3, 4, 5, 6, 7, 0, 8, 9, 10];
pub const CLEFS: [&str; 5] = ["Treble", "Bass", "Alto", "Tenor", "Percussion"];
pub const LINES: [&str; 4] = ["Slur Up", "Slur Down", "Crescendo", "Decrescendo"];
pub const TEXTS: [&str; 7] = ["Text", "Lyric", "Chord", "REGION", "INSTR", "SONG", "DATE"];

/// The Score's own view settings (kept in egui's memory; edits to the music are in the project).
#[derive(Clone, Debug)]
pub struct State {
    pub part: Part,
    pub dotted: bool,
    pub triplet: bool,
    /// Staff space in points, bar width (linear), scroll (bars, points).
    pub space: f32,
    pub bar_w: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    drag: Option<DragState>,
    pub view: View,
    pub colors: Colors,
    pub explode_poly: bool,
    pub explode_folders: bool,
    pub names: bool,
    pub rulers: bool,
    /// Duration bars: 0 off, 1 selected notes, 2 all notes.
    pub duration_bars: u8,
    pub guides: bool,
    pub snap: bool,
    pub diatonic: bool,
    /// Tracks shown by a score set (track ids); None: the selected track.
    pub score_set: Option<Vec<u64>>,
    /// The project's score set shown (index into Score::sets; None: All Instruments / the
    /// display level).
    pub set: Option<usize>,
    /// Text being typed for a symbol: (track, tick, kind, staff, text, where).
    pub typing: Option<(usize, u32, SymbolKind, u8, String, Pos2)>,
    /// Go to Page… being typed.
    pub goto: Option<String>,
    /// The Score Project Settings window (its tab), the Staff Style window.
    pub settings: Option<usize>,
    pub styles_window: bool,
    /// Settings: bar numbers at line starts, key signature on every line, chord symbol size,
    /// the MIDI Meaning of dynamics (velocity per Part box dynamic).
    pub bar_numbers: bool,
    pub key_every_line: bool,
    pub chord_size: f32,
    pub dyn_velocity: [u8; 8],
    /// The selected text symbol (track id, index) and global bar symbol (index), for the Event
    /// inspector and Delete.
    pub sel_symbol: Option<(u64, usize)>,
    pub sel_bar: Option<usize>,
    /// A selected pedal marking: (track, region, event).
    pub sel_pedal: Option<(usize, usize, usize)>,
    bar_drag_len: bool,
    /// Pages in the Page view (for Go to Page), and the page to bring into view.
    pub pages: usize,
    pub goto_page: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        State {
            part: Part::Note(2),
            dotted: false,
            triplet: false,
            space: 8.0,
            bar_w: 300.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: None,
            view: View::Linear,
            colors: Colors::Normal,
            explode_poly: false,
            explode_folders: false,
            names: false,
            rulers: false,
            duration_bars: 0,
            guides: true,
            snap: true,
            diatonic: false,
            score_set: None,
            set: None,
            typing: None,
            goto: None,
            settings: None,
            styles_window: false,
            bar_numbers: true,
            key_every_line: true,
            chord_size: 1.0,
            dyn_velocity: [16, 32, 48, 64, 80, 96, 112, 127],
            pages: 1,
            goto_page: None,
            sel_symbol: None,
            sel_bar: None,
            sel_pedal: None,
            bar_drag_len: false,
        }
    }
}

#[derive(Clone, Debug)]
struct DragState {
    region: (usize, usize),
    orig: Vec<Note>,
    from: Pos2,
    moved: bool,
}

fn state_id() -> egui::Id {
    egui::Id::unique("lg-score-state")
}

/// Reads and changes the Score's view state.
pub fn with_state<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(state_id())))
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default()
}

/// Part box note values: (label, base).
pub const VALUES: [(&str, u8); 7] = [("1/1", 1), ("1/2", 2), ("1/4", 4), ("1/8", 8), ("1/16", 16), ("1/32", 32), ("1/64", 64)];
/// Dynamics (Logic's MIDI Meaning defaults give each a velocity).
pub const DYNAMICS: [&str; 8] = ["ppp", "pp", "p", "mp", "mf", "f", "ff", "fff"];

/// Paper and ink (Logic's Score is light in dark mode too).
const PAPER: Color32 = Color32::from_rgb(0xbf, 0xbf, 0xb8);
const REGION_PAPER: Color32 = Color32::from_rgb(0xfe, 0xff, 0xf3);
const PAGE: Color32 = Color32::from_rgb(0xff, 0xff, 0xfb);
const INK: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);

/// A staff on screen: clef, its bottom line's y.
#[derive(Clone, Copy)]
struct Staff {
    bottom: f32,
}

/// A staff row of the score: which track, which staff of its style, which voice (Explode
/// Polyphony), and the style.
#[derive(Clone)]
struct Row {
    t: usize,
    id: u64,
    staff: usize,
    voice: Option<u8>,
    style: nt::StaffStyle,
    name: String,
    /// The score set's short name ("" = made from the name), bracket and bar line joins to the
    /// next instrument.
    short: String,
    bracket: bool,
    barline: bool,
    /// The first row of a two-staff (grand staff) track.
    brace: bool,
    /// Offset of the staff's bottom line from the line's top.
    off: f32,
}

/// One engraved stretch: a region's notes on a row between key and clef changes.
struct Seg {
    row: usize,
    rix: usize,
    voice: nt::Voice,
    from: u32,
    hide_rests: bool,
}

/// A line (system) on screen: bars b0..b1, where each bar starts (xs has one more for the
/// end), the y of its top, where the music's header starts.
#[derive(Clone)]
struct Line {
    b0: u32,
    b1: u32,
    xs: Vec<f32>,
    top: f32,
    x_start: f32,
    first: bool,
    /// The first line of its page (Page view; the first line otherwise).
    page_first: bool,
}

impl Line {
    fn x_of(&self, tick: f32, bar_t: f32) -> f32 {
        let b = (tick / bar_t).floor();
        let k = (b as i64 - self.b0 as i64).clamp(0, self.xs.len() as i64 - 2) as usize;
        let frac = tick / bar_t - (self.b0 as f32 + k as f32);
        self.xs[k] + (self.xs[k + 1] - self.xs[k]) * frac
    }
    fn tick_of(&self, x: f32, bar_t: f32) -> f32 {
        let k = self.xs.windows(2).position(|w| x < w[1]).unwrap_or(self.xs.len().saturating_sub(2));
        let w = (self.xs[k + 1] - self.xs[k]).max(1.0);
        (self.b0 as f32 + k as f32 + (x - self.xs[k]) / w) * bar_t
    }
}

/// The clef a row uses from `tick`: the last clef change at or before it, else the style's.
fn clef_at(app: &App, row: &Row, tick: u32) -> (nt::Clef, i8) {
    let base = (row.style.clefs[row.staff.min(row.style.clefs.len() - 1)], row.style.octave);
    let Some(ts) = app.project.as_ref().and_then(|p| p.score.track(row.id)) else { return base };
    ts.symbols.iter().filter(|s| s.kind == SymbolKind::Clef && s.staff as usize == row.staff && s.tick <= tick).max_by_key(|s| s.tick).map_or(base, |s| (clef_named(&s.text), 0))
}

pub fn clef_named(name: &str) -> nt::Clef {
    match name {
        "Bass" => nt::Clef::Bass,
        "Alto" => nt::Clef::Alto,
        "Tenor" => nt::Clef::Tenor,
        "Percussion" => nt::Clef::Percussion,
        _ => nt::Clef::Treble,
    }
}

/// The staff style a track uses (its own, or Logic's automatic choice by range).
pub fn style_of(app: &App, t: usize) -> nt::StaffStyle {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return nt::StaffStyle::ALL[0] };
    if let Some(s) = app.project.as_ref().and_then(|p| p.score.track(tr.extra.id)).and_then(|ts| nt::StaffStyle::by_name(&ts.style)) {
        return s;
    }
    let pitches: Vec<u8> = tr.regions.iter().flat_map(|r| r.notes.iter().map(|n| n.pitch)).collect();
    let lname = tr.name.to_lowercase();
    if tr.kind == lg_core::TrackKind::Drummer || ["drum", "kit", "beat", "perc"].iter().any(|w| lname.contains(w)) {
        return nt::StaffStyle::by_name("Drums").unwrap_or(nt::StaffStyle::ALL[0]);
    }
    let piano = ["piano", "keys", "grand", "steinway", "rhodes", "clav", "organ", "harpsichord"].iter().any(|w| lname.contains(w));
    nt::StaffStyle::auto(&pitches, piano)
}

/// The display Quantize a track reads with.
pub(crate) fn quant_of(app: &App, id: u64) -> (usize, bool, bool) {
    let ts = app.project.as_ref().and_then(|p| p.score.track(id));
    let q = ts.and_then(|t| nt::Quant::CHOICES.iter().position(|c| c.0 == t.quantize)).unwrap_or(9);
    (q, ts.is_none_or(|t| !t.no_interpretation), ts.is_some_and(|t| t.syncopation))
}

/// The tracks the Score shows: the score set, every MIDI track (Explode Folders), or the
/// selected region's track.
/// The region's Max. Dots (Score inspector), 1 by default.
pub fn max_dots_of(app: &App, id: u64) -> u8 {
    app.project.as_ref().and_then(|p| p.score.track(id)).map_or(1, |t| if t.max_dots == 0 { 1 } else { t.max_dots - 1 })
}

fn shown_tracks(app: &App, st: &State) -> Vec<usize> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let hide_muted = crate::scoreset::fmt(&app.proj).hide_muted_tracks;
    let midi = |t: &lg_core::Track| t.kind != lg_core::TrackKind::Audio && t.regions.iter().any(|r| r.audio.is_none()) && !(hide_muted && t.mute);
    // (a project score set: its instruments in its own order)
    if let Some(set) = st.set.and_then(|k| p.score.sets.get(k)) {
        return set.entries.iter().filter_map(|e| p.tracks.iter().position(|t| t.extra.id == e.track)).filter(|&i| midi(&p.tracks[i])).collect();
    }
    if let Some(set) = &st.score_set {
        let v: Vec<usize> = p.tracks.iter().enumerate().filter(|(_, t)| set.contains(&t.extra.id) && midi(t)).map(|(i, _)| i).collect();
        if !v.is_empty() {
            return v;
        }
    }
    if st.explode_folders {
        return p.tracks.iter().enumerate().filter(|(_, t)| midi(t)).map(|(i, _)| i).collect();
    }
    app.selected_region.map(|(t, _)| vec![t]).filter(|v| v.iter().all(|&t| p.tracks.get(t).is_some_and(|tr| !(hide_muted && tr.mute)))).unwrap_or_default()
}

/// The rows for the shown tracks, with their vertical offsets (in staff spaces × sp).
fn rows_for(app: &App, st: &State, tracks: &[usize], sp: f32) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut off = 4.0 * sp;
    for (k, &t) in tracks.iter().enumerate() {
        let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { continue };
        let style = style_of(app, t);
        if k > 0 {
            off += 12.0 * sp;
        }
        let n = style.clefs.len();
        for s in 0..n {
            let voices: &[Option<u8>] = if st.explode_poly { &[Some(0), Some(1)] } else { &[None] };
            for (vk, &v) in voices.iter().enumerate() {
                if s > 0 || vk > 0 {
                    off += 10.0 * sp;
                }
                let entry = st.set.and_then(|k| app.project.as_ref()?.score.sets.get(k)?.entry(tr.extra.id).cloned()).unwrap_or_default();
                let name = if entry.full.is_empty() { tr.name.clone() } else { entry.full.clone() };
                rows.push(Row { t, id: tr.extra.id, staff: s, voice: v, style, name, short: entry.short.clone(), bracket: entry.bracket, barline: entry.barline, brace: n > 1 && s == 0 && vk == 0, off });
            }
        }
    }
    rows
}

/// The notes of a region a row shows, as lg-notation reads them (written pitch, attributes),
/// split into voices: (notes, voice of each).
fn row_notes(app: &App, row: &Row, region: &lg_core::Region) -> Vec<(nt::NoteIn, u8)> {
    let score = app.project.as_ref().map(|p| &p.score);
    let hide_muted = crate::scoreset::fmt(&app.proj).hide_muted_notes;
    let mine: Vec<nt::NoteIn> = region
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| !(hide_muted && n.muted))
        .filter(|(_, n)| {
            let a = score.map(|s| s.attr(row.id, n.pos, n.pitch)).unwrap_or_default();
            let staff = match a.staff {
                1 => 0,
                -1 => row.style.clefs.len() - 1,
                _ => row.style.staff_of(n.pitch),
            };
            staff == row.staff
        })
        .map(|(i, n)| {
            let a = score.map(|s| s.attr(row.id, n.pos, n.pitch)).unwrap_or_default();
            nt::NoteIn { index: i, pitch: row.style.written(n.pitch), pos: n.pos, len: n.len, spell: a.spell, stem: a.stem, beam: a.beam, interp: a.interp, synco: a.synco, tie: a.tie, acc: a.accidental }
        })
        .collect();
    let voices = nt::split_voices(&mine);
    mine.into_iter().zip(voices).collect()
}

fn engrave_rows(app: &App, rows: &[Row]) -> Vec<Seg> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let sig = p.signature;
    let bar_t = lg_core::project::bar_ticks(sig);
    let first_key = nt::Key::parse(&p.key);
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let Some(track) = p.tracks.get(row.t) else { continue };
        let (q, interp, synco) = quant_of(app, row.id);
        // where keys and clefs change (bars)
        let mut cuts: Vec<u32> = p.score.keys.iter().map(|k| k.0 / bar_t * bar_t).collect();
        if let Some(ts) = p.score.track(row.id) {
            cuts.extend(ts.symbols.iter().filter(|s| s.kind == SymbolKind::Clef && s.staff as usize == row.staff).map(|s| s.tick / bar_t * bar_t));
        }
        cuts.sort_unstable();
        cuts.dedup();
        let hide_muted = crate::scoreset::fmt(&app.proj).hide_muted_regions;
        for (rix, region) in track.regions.iter().enumerate() {
            if region.audio.is_some() || (hide_muted && region.muted) {
                continue;
            }
            let from = region.start / bar_t * bar_t;
            let to = ((region.start + region.length).div_ceil(bar_t) * bar_t).max(from + bar_t);
            let notes = row_notes(app, row, region);
            let two = notes.iter().any(|x| x.1 == 1);
            let mut bounds = vec![from];
            bounds.extend(cuts.iter().copied().filter(|&c| c > from && c < to));
            bounds.push(to);
            for w in bounds.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (clef, _) = clef_at(app, row, a);
                let key = nt::Key { fifths: p.score.key_at(a, first_key.fifths) };
                let key = row.style.written_key(key);
                let mut opts = nt::Options::new(sig, key, nt::Quant::CHOICES[q].1, a, b);
                opts.interpretation = interp;
                opts.syncopation = synco;
                opts.max_dots = max_dots_of(app, row.id);
                let voices: Vec<u8> = match row.voice {
                    Some(v) => vec![v],
                    None if two => vec![0, 1],
                    None => vec![0],
                };
                for v in voices {
                    let mut sel: Vec<nt::NoteIn> = notes.iter().filter(|x| x.1 == v || (row.voice.is_none() && !two)).map(|x| x.0).filter(|n| n.pos + n.len > a && n.pos < b).collect();
                    if two && row.voice.is_none() {
                        for n in sel.iter_mut().filter(|n| n.stem == 0) {
                            n.stem = if v == 0 { 1 } else { -1 };
                        }
                    }
                    out.push(Seg { row: ri, rix, voice: nt::engrave(&sel, clef, &opts), from: a, hide_rests: v == 1 && row.voice.is_none() });
                }
            }
        }
    }
    out
}

/// The shown score engraved for export (scorexml.rs): per staff (track, staff, clef, voices
/// with every region's items in time order).
pub(crate) fn engraved_staffs(app: &App, st: &State) -> Vec<(usize, usize, nt::Clef, Vec<nt::Voice>)> {
    let mut st = st.clone();
    st.explode_poly = false;
    let tracks = shown_tracks(app, &st);
    let rows = rows_for(app, &st, &tracks, 8.0);
    let segs = engrave_rows(app, &rows);
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let mut voices: Vec<nt::Voice> = vec![nt::Voice::default(), nt::Voice::default()];
        let mut mine: Vec<&Seg> = segs.iter().filter(|s| s.row == ri).collect();
        mine.sort_by_key(|s| s.from);
        for seg in mine {
            let v = &mut voices[seg.hide_rests as usize];
            let base = v.items.len();
            let nb = v.beams.len();
            v.items.extend(seg.voice.items.iter().cloned().map(|mut it| {
                it.beam = it.beam.map(|b| b + nb);
                it
            }));
            v.beams.extend(seg.voice.beams.iter().cloned().map(|mut b| {
                b.items.iter_mut().for_each(|i| *i += base);
                b
            }));
            v.tuplets.extend(seg.voice.tuplets.iter().cloned().map(|mut t| {
                t.first += base;
                t.last += base;
                t
            }));
        }
        if voices[1].items.is_empty() {
            voices.pop();
        }
        out.push((row.t, row.staff, row.style.clefs[row.staff.min(row.style.clefs.len() - 1)], voices));
    }
    out
}

/// Where a staff position is on a staff whose bottom line is at `bottom`.
fn y_at(bottom: f32, pos: i32, sp: f32) -> f32 {
    bottom - pos as f32 * sp / 2.0
}

/// The colour of a note in the View ▸ Colors mode (and its own Color attribute).
fn note_color(colors: Colors, n: &Note, voice: u8, attr: &NoteAttr) -> Color32 {
    const PALETTE: [Color32; 16] = [
        INK,
        Color32::from_rgb(0xd0, 0x30, 0x30),
        Color32::from_rgb(0xe0, 0x80, 0x20),
        Color32::from_rgb(0xc0, 0xa0, 0x10),
        Color32::from_rgb(0x50, 0xa0, 0x30),
        Color32::from_rgb(0x20, 0x90, 0x90),
        Color32::from_rgb(0x30, 0x60, 0xd0),
        Color32::from_rgb(0x80, 0x40, 0xc0),
        Color32::from_rgb(0xc0, 0x40, 0x90),
        Color32::from_rgb(0x70, 0x50, 0x30),
        Color32::from_rgb(0x60, 0x60, 0x60),
        Color32::from_rgb(0x90, 0x20, 0x20),
        Color32::from_rgb(0x20, 0x70, 0x40),
        Color32::from_rgb(0x20, 0x40, 0x90),
        Color32::from_rgb(0xa0, 0x70, 0x00),
        Color32::from_rgb(0x50, 0x20, 0x70),
    ];
    match colors {
        Colors::BlackWhite => INK,
        Colors::Pitch => {
            let h = (n.pitch % 12) as f32 / 12.0;
            egui::ecolor::Hsva::new(h, 0.8, 0.7, 1.0).into()
        }
        Colors::Velocity => {
            // blue (soft) through green to red (loud), as Logic's velocity colours
            let h = (1.0 - n.velocity as f32 / 127.0) * 0.66;
            egui::ecolor::Hsva::new(h, 0.85, 0.75, 1.0).into()
        }
        Colors::Voice => if voice == 0 { Color32::from_rgb(0x20, 0x50, 0xc0) } else { Color32::from_rgb(0xc0, 0x40, 0x20) },
        Colors::Normal => PALETTE[attr.color as usize % 16],
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let mut st = state(&ctx);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let tabs_r = Rect::from_min_size(area.min, vec2(area.width(), TABS_H));
    tabs(app, ui, tabs_r, EditorTab::Score);
    let tracks = shown_tracks(app, &st);
    let Some(&first_t) = tracks.first() else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select a MIDI region to see its score", look::regular(14.0), look::text_dim());
        return;
    };
    if app.selected_region.is_some_and(|(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|r| r.audio.is_some())) {
        p.text(area.center(), Align2::CENTER_CENTER, "An audio region has no score", look::regular(14.0), look::text_dim());
        return;
    }
    let (sig, project_name) = app.project.as_ref().map(|p| (p.signature, p.name.clone())).unwrap_or(((4, 4), String::new()));
    let bar_t = lg_core::project::bar_ticks(sig) as f32;
    let cur_t = app.selected_region.map_or(first_t, |x| x.0);


    // the local menus: Layout / Edit / Functions / View
    let local = Rect::from_min_size(pos2(area.min.x, tabs_r.max.y), vec2(area.width(), LOCAL_H));
    p.rect_filled(local, 0.0, look::header_bg());
    ui.scope_builder(egui::UiBuilder::new().max_rect(local.shrink2(vec2(10.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Layout", "score-Layout"), ("Edit", "score-Edit"), ("Functions", "score-Functions"), ("View", "score-View")]);
    });
    // the Part box strip
    // (the Part box and the region's Score parameters are in the Score inspector: scoreinsp.rs)
    let part = Rect::from_min_size(pos2(area.min.x, local.max.y), vec2(area.width(), 0.0));
    let linear = st.view == View::Linear;
    // the Score project settings (scoreset.rs); the View menu's Instrument Names shows full
    // names beside the staffs as well
    let fmt = crate::scoreset::fmt(&app.proj);
    let names_on = st.names || fmt.names;
    let names_beside = st.names || (fmt.names && !fmt.names_above);
    let head_w = if linear { HEAD_W + if names_on { 80.0 } else { 0.0 } } else { 0.0 };
    let ruler_h = if linear { RULER_H } else { 0.0 };
    let ruler = Rect::from_min_max(pos2(area.min.x + head_w, part.max.y), pos2(area.max.x, part.max.y + ruler_h));
    let body = Rect::from_min_max(pos2(area.min.x, ruler.max.y), area.max);
    let head = Rect::from_min_max(body.min, pos2(area.min.x + head_w, body.max.y));
    let music = Rect::from_min_max(pos2(head.max.x, body.min.y), body.max);

    // scrolling and zoom (⌘ + wheel or a pinch zooms the staff, Shift + wheel scrolls in time)
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| body.contains(h) || ruler.contains(h)) {
        let (scroll, zoom, cmd, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.modifiers.shift));
        if cmd && scroll.y != 0.0 {
            let f = 1.0 + scroll.y * 0.003;
            st.space = (st.space * f).clamp(3.0, 24.0);
            st.bar_w = (st.bar_w * f).clamp(80.0, 2400.0);
        } else if (zoom - 1.0).abs() > 0.001 {
            st.space = (st.space * zoom).clamp(3.0, 24.0);
            st.bar_w = (st.bar_w * zoom).clamp(80.0, 2400.0);
        } else if shift && linear {
            st.scroll_x = (st.scroll_x - scroll.y / st.bar_w).max(0.0);
        } else {
            if linear {
                st.scroll_x = (st.scroll_x - scroll.x / st.bar_w).max(0.0);
            }
            st.scroll_y = (st.scroll_y - scroll.y).clamp(-400.0, 100_000.0);
        }
    }
    // the region in view when it isn't
    if let (true, Some((ti, ri))) = (linear, app.selected_region) {
        if let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(ti)?.regions.get(ri)) {
            let first_bar = (rg.start as f32 / bar_t).floor();
            if (first_bar < st.scroll_x || first_bar > st.scroll_x + music.width() / st.bar_w - 1.0) && !ctx.data(|d| d.get_temp::<(usize, usize)>(egui::Id::unique("lg-score-seen"))).is_some_and(|x| x == (ti, ri)) {
                st.scroll_x = (first_bar - 0.15).max(0.0);
            }
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique("lg-score-seen"), (ti, ri)));
        }
    }
    // the Page view shows a whole page's width (zoom in from there)
    let set_scale = st.set.and_then(|k| app.project.as_ref()?.score.sets.get(k)).map_or(1.0, |s| s.scale.clamp(25, 250) as f32 / 100.0);
    let sp = if st.view == View::Page { st.space.min((music.width() - 24.0) / 120.0).max(3.0) } else { st.space } * set_scale;
    let rows = rows_for(app, &st, &tracks, sp);
    let block_h = rows.last().map_or(4.0 * sp, |r| r.off) + 4.0 * sp;
    let segs = engrave_rows(app, &rows);
    let key0 = app.project.as_ref().map_or(nt::Key::default(), |p| nt::Key::parse(&p.key));
    let score_keys = app.project.as_ref().map(|p| p.score.keys.clone()).unwrap_or_default();
    let key_at = |tick: u32| nt::Key { fifths: score_keys.iter().filter(|k| k.0 <= tick).last().map_or(key0.fifths, |k| k.1) };

    // the bars the shown tracks cover
    let (bar_lo, bar_hi) = {
        let mut lo = u32::MAX;
        let mut hi = 0;
        if let Some(p) = app.project.as_ref() {
            for &t in &tracks {
                for r in p.tracks[t].regions.iter().filter(|r| r.audio.is_none()) {
                    lo = lo.min(r.start / bar_t as u32);
                    hi = hi.max((r.start + r.length).div_ceil(bar_t as u32));
                }
            }
        }
        if lo == u32::MAX { (0, 1) } else { (lo, hi.max(lo + 1)) }
    };

    // paper
    let mp = ui.painter_at(music);
    let headp = ui.painter_at(head);
    mp.rect_filled(music, 0.0, PAPER);
    if linear {
        headp.rect_filled(head, 0.0, PAPER);
        p.vline(head.max.x, body.y_range(), Stroke::new(1.0, Color32::from_gray(0x30)));
    }
    // a line's header: clef, key, time signature
    let header_w = |key: nt::Key, time: bool| (3.6 + key.fifths.unsigned_abs() as f32 * 1.1 + if time { 2.8 } else { 0.6 }) * sp;
    // the lines (systems)
    let mut lines: Vec<Line> = Vec::new();
    let mut pages: Vec<Rect> = Vec::new();
    match st.view {
        View::Linear => {
            let top = body.min.y + 5.0 * sp + ((body.height() - block_h - 10.0 * sp) / 2.0).max(0.0) - st.scroll_y;
            let (sx, bw) = (st.scroll_x, st.bar_w);
            let b0 = sx.floor().max(0.0) as u32;
            let n = (music.width() / bw).ceil() as u32 + 2;
            let xs = (0..=n).map(|k| music.min.x + ((b0 + k) as f32 - sx) * bw).collect();
            lines.push(Line { b0, b1: b0 + n, xs, top, x_start: music.min.x, first: true, page_first: true });
        }
        View::Wrapped | View::Page => {
            let page_w = 120.0 * sp;
            let page_h = 170.0 * sp;
            let names_w = if names_beside { 9.0 * sp } else { 0.0 };
            // (Global: the page margins; the wrapped view keeps a narrow edge)
            let margins = |pg: usize| if st.view == View::Page { let (l, r) = fmt.margins(pg); (l * sp + names_w, r * sp) } else { (2.0 * sp + names_w, 2.0 * sp) };
            let (ml, mr) = margins(0);
            let width = if st.view == View::Page { page_w - ml - mr } else { music.width() - ml - mr };
            let widths: Vec<f32> = (bar_lo..bar_hi)
                .map(|b| segs.iter().filter(|s| (s.from / bar_t as u32) <= b).map(|s| nt::bar_width(&s.voice, b - s.from / bar_t as u32, sp)).fold(12.0 * sp, f32::max))
                .collect();
            let hw = header_w(key0, true);
            // (manual Line / Page Break and No Line Break symbols)
            let breaks = app.project.as_ref().map(|p| p.score.breaks.clone()).unwrap_or_default();
            let brk = |k: usize| match breaks.get(&(bar_lo + k as u32)) {
                Some(lg_core::score::Break::Line | lg_core::score::Break::Page) => 1,
                Some(lg_core::score::Break::NoLine) => -1,
                _ => 0,
            };
            let systems = nt::systems_breaks(&widths, (width - hw).max(20.0 * sp), fmt.max_bars, fmt.justify_last, &brk);
            let line_h = block_h + 7.0 * sp + fmt.line_dist * crate::scoreset::SP_PER_CM * sp;
            let x0 = if st.view == View::Page { music.min.x + ((music.width() - page_w) / 2.0).max(12.0) } else { music.min.x };
            let tops: Vec<(usize, f32)> = if st.view == View::Page {
                let cm = crate::scoreset::SP_PER_CM * sp;
                let page_brk = |l: usize| match systems.get(l).and_then(|x| breaks.get(&(bar_lo + x.0 as u32))) {
                    Some(lg_core::score::Break::Page) => 1,
                    Some(lg_core::score::Break::NoPage) => -1,
                    _ => 0,
                };
                nt::paginate_breaks(systems.len(), line_h, page_h, fmt.top * cm, fmt.bottom * cm, fmt.header * cm, &page_brk)
            } else {
                (0..systems.len()).map(|k| (0, 4.0 * sp + k as f32 * line_h)).collect()
            };
            let n_pages = tops.last().map_or(1, |t| t.0 + 1);
            st.pages = n_pages;
            if let Some(pg) = st.goto_page.take() {
                st.scroll_y = (pg.min(n_pages - 1) as f32 * (page_h + 4.0 * sp)).max(0.0);
            }
            if st.view == View::Page {
                for k in 0..n_pages {
                    let r = Rect::from_min_size(pos2(x0, music.min.y + 12.0 + k as f32 * (page_h + 4.0 * sp) - st.scroll_y), vec2(page_w, page_h));
                    pages.push(r);
                }
            }
            for (li, &(b, n, scale)) in systems.iter().enumerate() {
                let (pg, y) = tops[li];
                let base_y = if st.view == View::Page { pages[pg].min.y } else { music.min.y - st.scroll_y };
                let xs_start = x0 + margins(pg).0;
                let page_first = li == 0 || tops[li - 1].0 != pg;
                let key = key_at((bar_lo + b as u32) * bar_t as u32);
                let key_on = li == 0 || (st.key_every_line && fmt.keys.on(page_first, li == 0));
                let hw = header_w(if key_on { key } else { nt::Key::default() }, fmt.times.on(page_first, li == 0));
                let mut xs = vec![xs_start + hw];
                for k in 0..n {
                    let last = *xs.last().unwrap_or(&0.0);
                    xs.push(last + widths[b + k] * scale);
                }
                lines.push(Line { b0: bar_lo + b as u32, b1: bar_lo + (b + n) as u32, xs, top: base_y + y + 5.0 * sp, x_start: xs_start, first: li == 0, page_first });
            }
        }
    }
    // pages, their rulers and the title
    for (k, pr) in pages.iter().enumerate() {
        if !pr.intersects(music) {
            continue;
        }
        mp.rect_filled(pr.translate(vec2(2.0, 3.0)), 0.0, Color32::from_black_alpha(40));
        mp.rect_filled(*pr, 0.0, PAGE);
        if k == 0 {
            mp.text(pos2(pr.center().x, pr.min.y + 8.0 * sp), Align2::CENTER_CENTER, &project_name, look::semibold(3.0 * sp), INK);
        }
        // (Numbers & Names: the page number's side, top or bottom, distances, prefix)
        let set_name = st.set.and_then(|k| app.project.as_ref()?.score.sets.get(k)).map_or(if st.score_set.is_some() { "Score Set" } else { "All Instruments" }.to_string(), |s| s.name.clone());
        let set_name = set_name.as_str();
        if let Some(text) = fmt.page_text(k, &project_name, set_name, "Region") {
            let cm = crate::scoreset::SP_PER_CM * sp;
            let (hd, vd) = (fmt.page_hdist * cm, fmt.page_vdist * cm);
            let y = if fmt.page_top { pr.min.y + vd } else { pr.max.y - vd };
            let (x, al) = match fmt.page_side(k) {
                0 => (pr.min.x + hd, if fmt.page_top { Align2::LEFT_TOP } else { Align2::LEFT_BOTTOM }),
                1 => (pr.center().x, if fmt.page_top { Align2::CENTER_TOP } else { Align2::CENTER_BOTTOM }),
                _ => (pr.max.x - hd, if fmt.page_top { Align2::RIGHT_TOP } else { Align2::RIGHT_BOTTOM }),
            };
            mp.text(pos2(x, y), al, text, look::regular(1.6 * sp), INK);
        }
        if st.rulers {
            // centimetres (a staff space is 1.75 mm on a 21 cm page)
            let cm = 10.0 / 1.75 * sp;
            let top = Rect::from_min_size(pr.min - vec2(0.0, 2.2 * sp), vec2(pr.width(), 2.0 * sp));
            mp.rect_filled(top, 0.0, look::header_bg());
            let mut i = 0;
            while i as f32 * cm <= pr.width() {
                let x = pr.min.x + i as f32 * cm;
                mp.vline(x, top.max.y - sp..=top.max.y, Stroke::new(1.0, look::text_dim()));
                mp.text(pos2(x + 2.0, top.min.y + 1.0), Align2::LEFT_TOP, i.to_string(), look::regular(9.0), look::text_dim());
                i += 1;
            }
            let left = Rect::from_min_size(pr.min - vec2(2.2 * sp, 0.0), vec2(2.0 * sp, pr.height()));
            mp.rect_filled(left, 0.0, look::header_bg());
            let mut i = 0;
            while i as f32 * cm <= pr.height() {
                mp.hline(left.max.x - sp..=left.max.x, pr.min.y + i as f32 * cm, Stroke::new(1.0, look::text_dim()));
                i += 1;
            }
        }
    }

    // the ruler (linear)
    if linear {
        let rp = ui.painter_at(ruler);
        rp.rect_filled(ruler, 0.0, look::header_bg());
        rp.hline(ruler.x_range(), ruler.max.y - 0.5, Stroke::new(1.0, look::line()));
        let beats = sig.0.max(1) as i32;
        let l = &lines[0];
        for b in l.b0..l.b1 {
            for k in 0..beats {
                let x = l.x_of(b as f32 * bar_t + k as f32 * bar_t / beats as f32, bar_t);
                if k == 0 {
                    rp.vline(x, ruler.y_range(), Stroke::new(1.0, look::text_dim()));
                    rp.text(pos2(x + 4.0, ruler.min.y + 8.0), Align2::LEFT_CENTER, format!("{}", b + 1), look::semibold(12.0), look::text());
                } else {
                    rp.vline(x, ruler.max.y - 6.0..=ruler.max.y, Stroke::new(1.0, look::text_dim()));
                }
            }
        }
        let hp = ui.painter_at(Rect::from_min_max(pos2(area.min.x, part.max.y), pos2(head.max.x, ruler.max.y)));
        hp.rect_filled(Rect::from_min_max(pos2(area.min.x, part.max.y), pos2(head.max.x, ruler.max.y)), 0.0, look::header_bg());
    }

    // the background: clicks, double-clicks (insert), the right-click menu
    let bg = ui.interact(music, ui.scope_id().with("sc-bg"), Sense::click());
    crate::menus::context_menu(app, &bg, "score-background");

    let sel_color = look::mix(look::accent(), Color32::BLACK, 0.1);
    let mut pressed: Option<(usize, usize, usize, bool)> = None;
    let score = app.project.as_ref().map(|p| p.score.clone()).unwrap_or_default();
    let project_tracks: Vec<lg_core::Track> = app.project.as_ref().map(|p| tracks.iter().map(|&t| p.tracks[t].clone()).collect()).unwrap_or_default();
    let track_of = |t: usize| -> Option<&lg_core::Track> { tracks.iter().position(|&x| x == t).and_then(|k| project_tracks.get(k)) };
    let dragging = st.drag.is_some();
    let mut guide_x: Option<f32> = None;
    let mut symbol_clicked = false;
    let repeated = score.repeated_bars(bar_t as u32);
    let mut bar_drag: Option<(usize, u32, bool)> = None;
    let mut bar_drag_done = false;
    let (sel_region_now, sel_notes_now) = (app.selected_region, app.selected_notes.clone());
    for (li, line) in lines.iter().enumerate() {
        let line_x1 = *line.xs.last().unwrap_or(&line.x_start);
        if line.top - 8.0 * sp > music.max.y || line.top + block_h + 8.0 * sp < music.min.y {
            continue;
        }
        let x_of = |tick: f32| line.x_of(tick, bar_t);
        // region paper (linear)
        if linear {
            for row_k in 0..rows.len() {
                if row_k > 0 && rows[row_k].t == rows[row_k - 1].t {
                    continue;
                }
                let row = &rows[row_k];
                let last = rows.iter().rposition(|r| r.t == row.t).unwrap_or(row_k);
                let Some(tr) = track_of(row.t) else { continue };
                for region in tr.regions.iter().filter(|r| r.audio.is_none()) {
                    let (rx0, rx1) = (x_of(region.start as f32), x_of((region.start + region.length) as f32));
                    if rx1 < music.min.x || rx0 > music.max.x || (region.start as f32) >= line.b1 as f32 * bar_t {
                        continue;
                    }
                    mp.rect_filled(Rect::from_min_max(pos2(rx0, line.top + row.off - 9.0 * sp), pos2(rx1, line.top + rows[last].off + 5.0 * sp)), 4.0, REGION_PAPER);
                }
            }
        }
        // staves
        for (rk, row) in rows.iter().enumerate() {
            let bottom = line.top + row.off;
            let x_from = if linear { music.min.x } else { line.x_start };
            for l in 0..5 {
                let y = bottom - l as f32 * sp;
                if row.style.clefs[0] == nt::Clef::Percussion || true {
                    mp.hline(x_from..=if linear { music.max.x } else { line_x1 }, y, Stroke::new(1.0, INK));
                }
            }
            let (clef, octave) = clef_at(app, row, line.b0 * bar_t as u32);
            let key = row.style.written_key(key_at(line.b0 * bar_t as u32));
            if linear {
                // the header panel
                for l in 0..5 {
                    headp.hline(head.min.x + 8.0 + if names_on { 80.0 } else { 0.0 }..=head.max.x, bottom - l as f32 * sp, Stroke::new(1.0, INK));
                }
                let x = head.min.x + 14.0 + if names_on { 80.0 } else { 0.0 };
                let name = if st.names { Some(row.name.clone()) } else { fmt.name_for(&row.name, true) };
                if let (Some(name), true) = (name, rk == 0 || rows[rk - 1].t != row.t) {
                    let (nx, al) = if fmt.names_right && !st.names { (head.min.x + 84.0, Align2::RIGHT_CENTER) } else { (head.min.x + 6.0, Align2::LEFT_CENTER) };
                    headp.text(pos2(nx, bottom - 2.0 * sp), al, name, look::regular(12.0), INK);
                }
                draw_header(&headp, x, bottom, sp, fmt.clefs != crate::scoreset::Show::Hide, clef, octave, if fmt.keys != crate::scoreset::Show::Hide { key } else { nt::Key::default() }, (fmt.times != crate::scoreset::Show::Hide).then_some(sig));
                // a click on the key or the time signature opens its window
                header_clicks(app, ui, Rect::from_min_max(pos2(x, bottom - 4.0 * sp), pos2(head.max.x, bottom)).intersect(head), x, sp, key_at(line.b0 * bar_t as u32), sig, (rk, true));
            } else {
                // (Clefs & Signatures: where clefs, keys and time signatures show)
                let key_on = line.first || (st.key_every_line && fmt.keys.on(line.page_first, line.first));
                let key_on = key_on && fmt.keys != crate::scoreset::Show::Hide;
                let time_on = fmt.times.on(line.page_first, line.first);
                draw_header(&mp, line.x_start + 0.4 * sp, bottom, sp, fmt.clefs.on(line.page_first, line.first), clef, octave, if key_on { key } else { nt::Key::default() }, time_on.then_some(sig));
                header_clicks(app, ui, Rect::from_min_max(pos2(line.x_start, bottom - 4.0 * sp), pos2(line.xs[0], bottom)).intersect(music), line.x_start + 0.4 * sp, sp, key_at(line.b0 * bar_t as u32), sig, (li * 100 + rk, false));
            }
            if !linear && (rk == 0 || rows[rk - 1].t != row.t) {
                let name = if st.names { line.first.then(|| row.name.clone()) } else { fmt.name_for(&row.name, line.first).map(|n| if !row.short.is_empty() && n != row.name { row.short.clone() } else { n }) };
                if let Some(name) = name {
                    if names_beside {
                        mp.text(pos2(line.x_start - 0.6 * sp, bottom - 2.0 * sp), Align2::RIGHT_CENTER, name, look::regular(1.5 * sp), INK);
                    } else {
                        // (above the staff, at the left or the right of the line)
                        let x = if fmt.names_right { line_x1 } else { line.x_start };
                        mp.text(pos2(x, bottom - 4.0 * sp - 2.2 * sp), if fmt.names_right { Align2::RIGHT_BOTTOM } else { Align2::LEFT_BOTTOM }, name, look::regular(1.5 * sp), INK);
                    }
                }
            }
            // (a single staff opens with a line unless Open single staffs; several are joined)
            if !linear && rk == 0 && !(fmt.open_single && rows.len() == 1) {
                let last = line.top + rows.last().map_or(row.off, |r| r.off);
                mp.vline(line.x_start, bottom - 4.0 * sp..=last, Stroke::new(1.0, INK));
            }
            // bar lines
            for b in line.b0..=line.b1 {
                let x = x_of(b as f32 * bar_t);
                if x < music.min.x - 2.0 || x > music.max.x + 2.0 {
                    continue;
                }
                let has = track_of(row.t).is_some_and(|tr| tr.regions.iter().any(|r| r.audio.is_none() && (r.start as f32) < b as f32 * bar_t + 1.0 && (r.start + r.length) as f32 > (b as f32 - 1.0) * bar_t));
                let final_bar = !linear && b == line.b1 && line.b1 == bar_hi;
                // (Bars and Repeats symbols replace the bar line; a double repeat at a line
                // break ends the line and starts the next)
                let sym = score.bar_at(b * bar_t as u32).map(|x| x.kind);
                use lg_core::score::BarKind as BK;
                if !linear && b == line.b0 && matches!(sym, Some(BK::RepeatStart | BK::RepeatBoth)) {
                    draw_bar(&mp, BK::RepeatStart, x + 0.5 * sp, bottom, sp);
                } else if (!linear || has) && (b > line.b0 || !linear) && (linear || b > line.b0) && (!fmt.hide_bar_lines || final_bar || sym.is_some()) {
                    match sym {
                        Some(BK::RepeatBoth) if !linear && b == line.b1 => draw_bar(&mp, BK::RepeatEnd, x, bottom, sp),
                        Some(BK::RepeatStart) if !linear && b == line.b1 => {
                            mp.vline(x, bottom - 4.0 * sp..=bottom, Stroke::new(1.0, INK));
                        }
                        Some(k) => draw_bar(&mp, k, x, bottom, sp),
                        None => {
                            let w = if final_bar { 2.5 } else { 1.0 };
                            mp.vline(x, bottom - 4.0 * sp..=bottom, Stroke::new(w, INK));
                        }
                    }
                }
                // Control-click a bar line: line and page breaks there
                if rk == 0 && b > line.b0 && x > music.min.x && x < music.max.x {
                    let br = Rect::from_center_size(pos2(x, bottom - 2.0 * sp), vec2((0.8 * sp).max(5.0), 4.0 * sp));
                    let resp = ui.interact(br, ui.scope_id().with(("sc-barline", li, b)), Sense::click());
                    resp.context_menu(|ui| crate::scoreinsp::bar_menu(app, ui, b));
                }
                // key changes at a bar inside the line
                if b > line.b0 && b < line.b1 && score_keys.iter().any(|k| k.0 / bar_t as u32 == b) {
                    let kk = row.style.written_key(key_at(b * bar_t as u32));
                    let mut kx = x + 0.6 * sp;
                    for (pos, acc) in kk.signature_positions(clef) {
                        accidental(&mp, pos2(kx, y_at(bottom, pos, sp)), sp, acc, INK);
                        kx += 1.1 * sp;
                    }
                }
            }
            // bar numbers at line starts (wrapped and page)
            // (Numbers & Names: step, offset, start, position; the top staff only, or the bottom
            // one when placed below)
            let top_row = if fmt.bar_v < 0.0 { rk + 1 == rows.len() } else { rk == 0 };
            if st.bar_numbers && (!linear || !fmt.bar_hide_linear) && (top_row || !fmt.bar_top_only) {
                let size = if fmt.bar_follow { 1.3 * sp } else { 11.0 };
                for b in line.b0..line.b1 {
                    let Some(n) = fmt.bar_number(b, b == line.b0 && !linear) else { continue };
                    let x = if fmt.bar_center { (x_of(b as f32 * bar_t) + x_of((b + 1) as f32 * bar_t)) / 2.0 } else if b == line.b0 && !linear { line.x_start } else { x_of(b as f32 * bar_t) };
                    if x < music.min.x || x > music.max.x {
                        continue;
                    }
                    let (y, al) = if fmt.bar_v >= 0.0 { (bottom - 4.0 * sp - fmt.bar_v * sp, if fmt.bar_center { Align2::CENTER_BOTTOM } else { Align2::LEFT_BOTTOM }) } else { (bottom - fmt.bar_v * sp, if fmt.bar_center { Align2::CENTER_TOP } else { Align2::LEFT_TOP }) };
                    mp.text(pos2(x, y), al, n.to_string(), look::regular(size), INK);
                }
            }
            // (score set: a bracket over instruments joined to the next; bar lines through)
            let last_of_track = rows.get(rk + 1).is_none_or(|r| r.t != row.t);
            if last_of_track && row.barline {
                if let Some(next) = rows.get(rk + 1) {
                    for b in line.b0..=line.b1 {
                        let x = x_of(b as f32 * bar_t);
                        if !linear && x >= music.min.x && x <= music.max.x {
                            mp.vline(x, bottom..=line.top + next.off - 4.0 * sp, Stroke::new(1.0, INK));
                        }
                    }
                }
            }
            let starts_bracket = row.bracket && (rk == 0 || !rows[..rk].iter().rev().find(|r| r.t != row.t).is_some_and(|r| r.bracket));
            if starts_bracket && rows.iter().position(|r| r.t == row.t) == Some(rk) {
                // down to the last staff of the last instrument in the run
                let mut end = rk;
                let mut k = rk;
                while k < rows.len() {
                    let t = rows[k].t;
                    let last = rows.iter().rposition(|r| r.t == t).unwrap_or(k);
                    end = last;
                    if !rows[k].bracket {
                        break;
                    }
                    k = last + 1;
                }
                let x = if linear { head.min.x + 4.0 + if names_on { 80.0 } else { 0.0 } } else { line.x_start - 1.2 * sp };
                let pp = if linear { &headp } else { &mp };
                let (y0, y1) = (bottom - 4.0 * sp, line.top + rows[end].off);
                // (a bracket joins at least two instruments)
                if rows[end].t != row.t {
                pp.line_segment([pos2(x, y0 - 0.5 * sp), pos2(x, y1 + 0.5 * sp)], Stroke::new(0.5 * sp, INK));
                smufl(pp, '\u{E003}', pos2(x - 0.25 * sp, y0 - 0.5 * sp), sp, INK, false);
                smufl(pp, '\u{E004}', pos2(x - 0.25 * sp, y1 + 0.5 * sp), sp, INK, false);
                }
            }
            if row.brace {
                let lower = line.top + rows.get(rk + 1).map_or(row.off, |r| r.off);
                let x = if linear { head.min.x + 8.0 + if st.names { 80.0 } else { 0.0 } } else { line.x_start };
                let pp = if linear { &headp } else { &mp };
                pp.vline(x, bottom - 4.0 * sp..=lower, Stroke::new(1.5, INK));
                brace(pp, x - 3.0, bottom - 4.0 * sp, lower, INK);
            }
        }
        // Bars and Repeats symbols inside bars, bar repeats, endings, segno and coda (global:
        // the bar signs on every staff, the rest above the top staff)
        {
            use lg_core::score::BarKind as BK;
            let (lo, hi) = (line.b0 * bar_t as u32, line.b1 * bar_t as u32);
            let top_bottom = line.top + rows.first().map_or(0.0, |r| r.off);
            for (bi, bs) in score.bars.iter().enumerate() {
                if bs.tick < lo || bs.tick >= hi.max(lo + 1) {
                    continue;
                }
                let x = x_of(bs.tick as f32);
                let mut hit: Option<Rect> = None;
                match bs.kind {
                    k if k.is_line() => {
                        if bs.tick % bar_t as u32 != 0 {
                            for row in &rows {
                                draw_bar(&mp, k, x + 0.8 * sp, line.top + row.off, sp);
                            }
                            hit = Some(Rect::from_min_max(pos2(x, top_bottom - 4.0 * sp), pos2(x + 1.6 * sp, top_bottom)));
                        }
                    }
                    BK::Repeat1 | BK::Repeat2 => {
                        let bars = if bs.kind == BK::Repeat1 { 1.0 } else { 2.0 };
                        let cx = (x + x_of(bs.tick as f32 + bars * bar_t)) / 2.0;
                        for row in &rows {
                            smufl(&mp, if bs.kind == BK::Repeat1 { '\u{E500}' } else { '\u{E501}' }, pos2(cx, line.top + row.off - 2.0 * sp), sp, INK, true);
                        }
                        hit = Some(Rect::from_center_size(pos2(cx, top_bottom - 2.0 * sp), vec2(3.0 * sp, 4.0 * sp)));
                    }
                    BK::Ending1 | BK::Ending2 => {
                        let x2 = x_of((bs.tick + bs.len.max(bar_t as u32 / 4)) as f32).min(*line.xs.last().unwrap_or(&x));
                        let y = top_bottom - 4.0 * sp - 3.0 * sp;
                        let ink = Stroke::new(1.0, INK);
                        mp.line_segment([pos2(x + 0.3 * sp, y + 2.0 * sp), pos2(x + 0.3 * sp, y)], ink);
                        mp.line_segment([pos2(x + 0.3 * sp, y), pos2(x2 - 0.3 * sp, y)], ink);
                        // (the second ending is open: no line down at its end)
                        if bs.kind == BK::Ending1 {
                            mp.line_segment([pos2(x2 - 0.3 * sp, y), pos2(x2 - 0.3 * sp, y + 2.0 * sp)], ink);
                        }
                        let style = score.styles().into_iter().find(|s| s.name == "Repeat Endings").unwrap_or_else(|| lg_core::score::TextStyle::default_named("Plain Text").unwrap());
                        mp.text(pos2(x + 0.8 * sp, y + 0.3 * sp), Align2::LEFT_TOP, &bs.text, style_font(&style, sp), INK);
                        hit = Some(Rect::from_min_max(pos2(x, y - 0.5 * sp), pos2(x2, y + 2.0 * sp)));
                    }
                    BK::Segno | BK::Coda => {
                        let c = pos2(x + 0.8 * sp, top_bottom - 4.0 * sp - 2.4 * sp);
                        smufl(&mp, if bs.kind == BK::Segno { '\u{E047}' } else { '\u{E048}' }, c, sp, INK, true);
                        hit = Some(Rect::from_center_size(c - vec2(0.0, 1.0 * sp), vec2(3.0 * sp, 3.0 * sp)));
                    }
                    _ => {}
                }
                if let Some(r) = hit {
                    if st.sel_bar == Some(bi) {
                        mp.rect_stroke(r.expand(2.0), 2.0, Stroke::new(1.2, sel_color), egui::StrokeKind::Outside);
                    }
                    let resp = ui.interact(r.intersect(music), ui.scope_id().with(("sc-bar", li, bi)), Sense::click_and_drag());
                    if resp.clicked() || resp.drag_started() {
                        st.sel_bar = Some(bi);
                        st.sel_symbol = None;
                        app.selected_notes.clear();
                        symbol_clicked = true;
                    }
                    // an ending: dragged by its corner (moves), its right end (length)
                    if matches!(bs.kind, BK::Ending1 | BK::Ending2) {
                        if resp.drag_started() {
                            let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(r.min);
                            st.bar_drag_len = from.x > r.max.x - 1.5 * sp;
                            app.checkpoint(if st.bar_drag_len { "Repeat Ending Length" } else { "Move Repeat Ending" });
                        }
                        if resp.dragged() {
                            if let Some(pp) = resp.interact_pointer_pos() {
                                let grid = bar_t / sig.0.max(1) as f32;
                                let t_at = ((line.tick_of(pp.x, bar_t) / grid).round() * grid).max(0.0) as u32;
                                bar_drag = Some((bi, t_at, st.bar_drag_len));
                            }
                        }
                        if resp.drag_stopped() {
                            bar_drag_done = true;
                        }
                    }
                }
            }
        }
        // the music
        for seg in segs.iter() {
            let row = &rows[seg.row];
            let Some(tr) = track_of(row.t) else { continue };
            let Some(region) = tr.regions.get(seg.rix) else { continue };
            let bottom = line.top + row.off;
            let staff = Staff { bottom };
            // (bars under one- and two-bar repeat signs hide their notes and rests)
            let in_line = |tick: u32| tick >= line.b0 * bar_t as u32 && tick < line.b1 * bar_t as u32 && !repeated.contains(&(tick / bar_t as u32));
            let sel_here = sel_region_now == Some((row.t, seg.rix));
            let selected = |i: usize| sel_here && sel_notes_now.contains(&i);
            let voice_of = |i: usize| seg.voice.items.iter().find(|it| it.heads.iter().any(|h| h.note == i)).map_or(0, |it| (!it.stem_up) as u8);
            let color = |i: usize| -> Color32 {
                if selected(i) {
                    return sel_color;
                }
                let n = region.notes.get(i).copied().unwrap_or(Note { channel: 0, pitch: 60, velocity: 80, pos: 0, len: 0, muted: false });
                note_color(st.colors, &n, voice_of(i), &score.attr(row.id, n.pos, n.pitch))
            };
            let attr = |i: usize| region.notes.get(i).map(|n| score.attr(row.id, n.pos, n.pitch)).unwrap_or_default();
            let opts = DrawOpts { color: &color, attr: &attr, hide_rests: seg.hide_rests, in_line: &in_line };
            draw_voice(&mp, &seg.voice, &staff, sp, &x_of, &opts);
            // duration bars: the notes' real lengths
            if st.duration_bars > 0 {
                for it in seg.voice.items.iter().filter(|it| in_line(it.tick)) {
                    for h in &it.heads {
                        if st.duration_bars == 1 && !selected(h.note) {
                            continue;
                        }
                        if let Some(n) = region.notes.get(h.note) {
                            if n.pos >= it.tick && n.pos < it.tick + it.dur {
                                let y = y_at(bottom, h.pos, sp);
                                let (a, b) = (x_of(n.pos as f32) + 0.8 * sp, x_of((n.pos + n.len) as f32) + 0.8 * sp);
                                mp.rect_filled(Rect::from_min_max(pos2(a, y - 0.18 * sp), pos2(b, y + 0.18 * sp)), 1.0, Color32::from_rgba_unmultiplied(0x30, 0x70, 0xe0, 140));
                            }
                        }
                    }
                }
            }
            // the notes' heads: click, drag (ids by note, so a head keeps its drag as it moves)
            let mut pieces: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
            for it in seg.voice.items.iter() {
                for h in it.heads.iter() {
                    let piece = pieces.entry(h.note).or_default();
                    *piece += 1;
                    if !in_line(it.tick) {
                        continue;
                    }
                    let c = head_center(it, h, &staff, sp, x_of(it.tick as f32));
                    let r = Rect::from_center_size(c, vec2(sp * 1.6, sp * 1.2));
                    if !r.intersects(music) {
                        continue;
                    }
                    let resp = ui.interact(r.intersect(music), ui.scope_id().with("lg-sc").with(("sc-head", li, row.t, seg.rix, seg.row, h.note, *piece)), Sense::click_and_drag());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() || resp.drag_started() {
                        pressed = Some((row.t, seg.rix, h.note, resp.drag_started()));
                    }
                    if resp.secondary_clicked() {
                        app.selected_region = Some((row.t, seg.rix));
                        if !app.selected_notes.contains(&h.note) {
                            app.selected_notes = vec![h.note];
                        }
                    }
                    if dragging && st.guides && selected(h.note) && *piece == 1 {
                        guide_x = Some(c.x);
                    }
                }
            }
        }
        // symbols: dynamics, hairpins, text, lyrics, chords, slurs, clef changes
        for (rk, row) in rows.iter().enumerate() {
            if rk > 0 && rows[rk - 1].t == row.t && rows[rk - 1].staff == row.staff {
                continue;
            }
            let Some(ts) = score.track(row.id) else { continue };
            let bottom = line.top + row.off;
            for (si, s) in ts.symbols.iter().enumerate().filter(|(_, s)| s.staff as usize == row.staff) {
                let lo = line.b0 as f32 * bar_t;
                let hi = line.b1 as f32 * bar_t;
                let (a, b) = (s.tick as f32, (s.tick + s.len) as f32);
                if b < lo || a >= hi {
                    continue;
                }
                let x = x_of(a.max(lo)) + 0.8 * sp;
                let x2 = x_of(b.min(hi)) + 0.8 * sp;
                // (automatic text: REGION, INSTR, SONG, DATE)
                let auto = |k: &str| -> String {
                    match k {
                        "REGION" => track_of(row.t).and_then(|tr| tr.regions.iter().find(|r| r.audio.is_none() && s.tick >= r.start && s.tick < r.start + r.length)).map_or_else(|| project_name.clone(), |r| r.name.clone()),
                        "INSTR" => st.set.and_then(|k| app.project.as_ref()?.score.sets.get(k)).map_or_else(|| row.name.clone(), |s| s.name.clone()),
                        "SONG" => project_name.clone(),
                        _ => crate::scoreset::today(),
                    }
                };
                let ext = || score.styles().into_iter().find(|x| x.name == "Chord Extension").unwrap_or_else(|| score.style_for(s));
                let text = SymText { style: score.style_for(s), auto: &auto, ext: &ext };
                if let Some(r) = draw_symbol(&mp, s, x, x2, bottom, sp, st.chord_size, &text) {
                    if st.sel_symbol == Some((row.id, si)) {
                        mp.rect_stroke(r.expand(2.0), 2.0, Stroke::new(1.2, sel_color), egui::StrokeKind::Outside);
                    }
                    let resp = ui.interact(r.expand(2.0).intersect(music), ui.scope_id().with(("sc-sym", li, row.id, si)), Sense::click());
                    if resp.clicked() {
                        st.sel_symbol = Some((row.id, si));
                        app.selected_notes.clear();
                        symbol_clicked = true;
                    }
                }
            }
            // automatic slurs (a note's Slur attribute): to the next note of the region
            if let Some(tr) = track_of(row.t) {
                for (&(pos, pitch), a) in ts.notes.iter().filter(|(_, a)| a.slur != 0) {
                    let Some(region) = tr.regions.iter().find(|r| r.audio.is_none() && pos >= r.start && pos < r.start + r.length) else { continue };
                    let Some(next) = region.notes.iter().filter(|n| n.pos > pos).min_by_key(|n| n.pos) else { continue };
                    let (lo, hi) = (line.b0 * bar_t as u32, line.b1 * bar_t as u32);
                    if pos < lo || pos >= hi {
                        continue;
                    }
                    let st_ = row.style;
                    let (clef, _) = clef_at(app, row, pos);
                    let k = key_at(pos);
                    let p1 = st_.written_key(k).spell(st_.written(pitch)).step - clef.bottom_line();
                    let p2 = st_.written_key(k).spell(st_.written(next.pitch)).step - clef.bottom_line();
                    let above = match a.slur {
                        1 => true,
                        -1 => false,
                        _ => p1 + p2 >= 8,
                    };
                    let d = if above { -1.0 } else { 1.0 };
                    let pa = pos2(x_of(pos as f32) + 1.4 * sp, y_at(bottom, p1, sp) + d * 0.8 * sp);
                    let pb = pos2(x_of(next.pos as f32) + 1.4 * sp, y_at(bottom, p2, sp) + d * 0.8 * sp);
                    tie(&mp, pa, pb, d * (1.0 * sp + (pb.x - pa.x) * 0.06).min(2.5 * sp), sp, INK);
                }
            }
        }
        // sustain pedal markings: the regions' controller 64 events, below a track's lowest staff
        for (rk, row) in rows.iter().enumerate() {
            if rows.get(rk + 1).is_some_and(|r| r.t == row.t) {
                continue;
            }
            let Some(tr) = track_of(row.t) else { continue };
            let bottom = line.top + row.off;
            let (lo, hi) = (line.b0 * bar_t as u32, line.b1 * bar_t as u32);
            for (rix, region) in tr.regions.iter().enumerate().filter(|(_, r)| r.audio.is_none()) {
                for (ei, e) in region.ext.events.iter().enumerate().filter(|(_, e)| crate::scoreinsp::is_pedal(e)) {
                    let at = region.start + e.pos;
                    if at < lo || at >= hi {
                        continue;
                    }
                    let c = pos2(x_of(at as f32) + 0.8 * sp, bottom + 4.6 * sp);
                    smufl(&mp, if e.value >= 64 { '\u{E650}' } else { '\u{E655}' }, c, sp, INK, true);
                    let r = Rect::from_center_size(c - vec2(0.0, 0.6 * sp), vec2(3.0 * sp, 2.0 * sp));
                    let resp = ui.interact(r.intersect(music), ui.scope_id().with(("sc-ped", li, row.t, rix, ei)), Sense::click());
                    if st.sel_pedal == Some((row.t, rix, ei)) {
                        mp.rect_stroke(r.expand(1.0), 2.0, Stroke::new(1.2, sel_color), egui::StrokeKind::Outside);
                    }
                    if resp.clicked() {
                        st.sel_pedal = Some((row.t, rix, ei));
                        st.sel_symbol = None;
                        st.sel_bar = None;
                        app.selected_notes.clear();
                        symbol_clicked = true;
                    }
                }
            }
        }
        // the playhead
        let ph = app.position() as f32;
        if ph >= line.b0 as f32 * bar_t && ph < line.b1 as f32 * bar_t {
            let x = x_of(ph);
            if music.x_range().contains(x) {
                mp.vline(x, (line.top - 6.0 * sp)..=(line.top + block_h + 2.0 * sp), Stroke::new(1.2, Color32::from_gray(0x30)));
            }
        }
    }
    // a repeat ending dragged: moved by its corner, lengthened by its right end
    if let Some((bi, t_at, len)) = bar_drag {
        if let Some(b) = app.project.as_mut().and_then(|p| p.score.bars.get_mut(bi)) {
            if len {
                b.len = t_at.saturating_sub(b.tick).max(bar_t as u32 / 4);
            } else {
                b.tick = t_at;
            }
        }
    }
    if bar_drag_done {
        app.after_edit();
    }
    if let Some(x) = guide_x {
        mp.vline(x, music.y_range(), Stroke::new(1.0, Color32::from_rgba_unmultiplied(0x30, 0x70, 0xe0, 160)));
    }

    // a press: select (Shift toggles), start a move
    if let Some((t, rix, note, drag)) = pressed {
        st.sel_symbol = None;
        st.sel_bar = None;
        let shift = ui.input(|i| i.modifiers.shift);
        if app.selected_region != Some((t, rix)) {
            app.selected_region = Some((t, rix));
            app.selected_track = Some(t);
            app.selected_notes.clear();
        }
        if shift && !drag {
            if let Some(i) = app.selected_notes.iter().position(|x| *x == note) {
                app.selected_notes.remove(i);
            } else {
                app.selected_notes.push(note);
            }
        } else if !app.selected_notes.contains(&note) {
            if shift {
                app.selected_notes.push(note);
            } else {
                app.selected_notes = vec![note];
            }
        }
        if drag {
            if let (Some(pp), Some(orig)) = (ui.input(|i| i.pointer.press_origin()), app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(rix)).map(|r| r.notes.clone())) {
                st.drag = Some(DragState { region: (t, rix), orig, from: pp, moved: false });
            }
        }
        if !drag {
            if let Some(n) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(rix)?.notes.get(note)).copied() {
                crate::pianoroll::preview(app, t, n.pitch);
            }
        }
    }
    // a drag: diatonic steps up and down, time on the display grid (or the tick with Snap off)
    if let Some(d) = st.drag.clone() {
        let (pointer, down) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let line = lines.iter().find(|l| d.from.y >= l.top - 8.0 * sp && d.from.y <= l.top + block_h + 6.0 * sp).or(lines.first()).cloned();
        if let (Some(pp), true, Some(line)) = (pointer, down, line) {
            let dy = pp.y - d.from.y;
            let steps = -(dy / (sp / 2.0)).round() as i32;
            let (q, _, _) = quant_of(app, app.project.as_ref().and_then(|p| p.tracks.get(d.region.0)).map_or(0, |t| t.extra.id));
            let grid = if st.snap { nt::Quant::CHOICES[q].1.straight as f32 } else { 10.0 };
            let dt = ((line.tick_of(pp.x, bar_t) - line.tick_of(d.from.x, bar_t)) / grid).round() * grid;
            if (steps != 0 || dt != 0.0) && !d.moved {
                app.checkpoint("Move Notes");
                if let Some(x) = st.drag.as_mut() {
                    x.moved = true;
                }
            }
            if steps != 0 || dt != 0.0 || d.moved {
                let sel = app.selected_notes.clone();
                let key = key_at(d.orig.first().map_or(0, |n| n.pos));
                let style = style_of(app, d.region.0);
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(d.region.0)?.regions.get_mut(d.region.1)) {
                    for &i in &sel {
                        if let (Some(o), Some(n)) = (d.orig.get(i), rg.notes.get_mut(i)) {
                            if style.clefs[0] == nt::Clef::Percussion {
                                n.pitch = (o.pitch as i32 + steps).clamp(0, 127) as u8;
                            } else {
                                let wk = style.written_key(key);
                                let step = wk.spell(style.written(o.pitch)).step + steps;
                                n.pitch = (nt::step_pitch(step, wk) as i32 - style.transpose as i32).clamp(0, 127) as u8;
                            }
                            n.pos = (o.pos as f32 + dt).max(0.0) as u32;
                        }
                    }
                    if let Some(n) = sel.first().and_then(|&i| rg.notes.get(i)) {
                        app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(n.pos, sig)), ("Length:".into(), crate::app::bbdt_len(n.len, sig)), ("Pitch:".into(), crate::app::note_name(n.pitch))]);
                    }
                }
            }
        }
        if !down {
            if st.drag.as_ref().is_some_and(|d| d.moved) {
                // the moved notes keep their score attributes
                let (t, r) = d.region;
                let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
                let now = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|r| r.notes.clone()).unwrap_or_default();
                if let Some(p) = app.project.as_mut() {
                    for &i in &app.selected_notes {
                        if let (Some(o), Some(n)) = (d.orig.get(i), now.get(i)) {
                            p.score.moved(id, (o.pos, o.pitch), (n.pos, n.pitch));
                        }
                    }
                }
                app.after_edit();
            }
            st.drag = None;
        }
    }
    // a Part box symbol dropped on the score, or a double-click with one selected
    let dropped = bg.dnd_release_payload::<Part>().map(|p| *p);
    if pressed.is_none() {
        let at = if dropped.is_some() { ui.input(|i| i.pointer.interact_pos()) } else if bg.double_clicked() { bg.interact_pointer_pos() } else { None };
        if let Some(part) = dropped {
            st.part = part;
        }
        if at.is_some() {
            if let Some(pp) = at {
                // the line and staff under the pointer
                if let Some(line) = lines.iter().find(|l| pp.y >= l.top - 9.0 * sp && pp.y <= l.top + block_h + 7.0 * sp && pp.x >= l.x_start) {
                    if let Some((rk, row)) = rows.iter().enumerate().min_by(|a, b| (line.top + a.1.off - 2.0 * sp - pp.y).abs().total_cmp(&(line.top + b.1.off - 2.0 * sp - pp.y).abs())) {
                        let _ = rk;
                        let tick = line.tick_of(pp.x, bar_t).max(0.0);
                        let bottom = line.top + row.off;
                        let pos = ((bottom - pp.y) / (sp / 2.0)).round() as i32;
                        insert_at(app, &mut st, row, tick, pos, pp);
                    }
                }
            }
        } else if bg.clicked() && !symbol_clicked {
            app.selected_notes.clear();
            st.sel_symbol = None;
            st.sel_bar = None;
            st.sel_pedal = None;
        }
    }
    // text being typed for a symbol
    if let Some((t, tick, kind, staff, mut text, at)) = st.typing.clone() {
        let mut done = None;
        let mut tab = false;
        egui::Area::new(egui::Id::unique("lg-score-typing")).fixed_pos(at).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(match kind {
                    SymbolKind::Lyric => "Lyric",
                    SymbolKind::Chord => "Chord symbol",
                    _ => "Text",
                }).size(11.0));
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                    done = Some(true);
                    tab = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        st.typing = Some((t, tick, kind, staff, text.clone(), at));
        if let Some(ok) = done {
            st.typing = None;
            if ok && !text.trim().is_empty() {
                add_symbol(app, t, Symbol { tick, kind, text: text.trim().to_string(), len: 0, staff, ..Default::default() }, "Insert Text");
            }
            // Tab: on to the next note for the next chord symbol or syllable
            if tab {
                let next = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.iter().filter(|r| r.audio.is_none()).flat_map(|r| r.notes.iter().map(|n| n.pos)).filter(|&q| q > tick).min());
                if let Some(nx) = next {
                    st.typing = Some((t, nx, kind, staff, String::new(), at + vec2(((nx - tick) as f32 / bar_t) * st.bar_w.min(400.0), 0.0)));
                }
            }
        }
    }
    // Go to Page…
    if let Some(mut s) = st.goto.clone() {
        let mut done = None;
        egui::Area::new(egui::Id::unique("lg-score-goto")).fixed_pos(music.center() - vec2(80.0, 20.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(format!("Go to Page (1–{}):", st.pages));
                let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        st.goto = Some(s.clone());
        if let Some(ok) = done {
            st.goto = None;
            if let (true, Ok(n)) = (ok, s.trim().parse::<usize>()) {
                st.goto_page = Some(n.max(1) - 1);
            }
        }
    }
    // Delete / Backspace with a symbol selected (text, bar sign, pedal marking): it goes
    if (st.sel_symbol.is_some() || st.sel_bar.is_some() || st.sel_pedal.is_some()) && !ctx.egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        ctx.data_mut(|d| d.insert_temp(state_id(), st.clone()));
        crate::scoreinsp::delete_selected(app);
        st = state(&ctx);
    }
    windows(app, &ctx, &mut st, cur_t);
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
}

/// The Score Project Settings window (Layout ▸ Global Format… and the others) and the Staff
/// Style window.
fn windows(app: &mut App, ctx: &egui::Context, st: &mut State, t: usize) {
    crate::scoreinsp::windows(app, ctx, st);
    if let Some(mut tab) = st.settings {
        let mut open = true;
        egui::Window::new("Score Project Settings").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (k, n) in SETTINGS_TABS.iter().enumerate() {
                    if ui.selectable_label(tab == k, *n).clicked() {
                        tab = k;
                    }
                }
            });
            ui.separator();
            match tab {
                0 => {
                    crate::scoreset::pane(ui, &mut app.proj, "Global");
                    ui.separator();
                    ui.add(egui::Slider::new(&mut st.space, 3.0..=24.0).text("Staff space (pt)"));
                    ui.add(egui::Slider::new(&mut st.bar_w, 80.0..=2400.0).text("Linear bar width"));
                }
                1 => crate::scoreset::pane(ui, &mut app.proj, "Numbers & Names"),
                2 => {
                    ui.add(egui::Slider::new(&mut st.chord_size, 0.6..=2.0).text("Chord symbol size"));
                }
                3 => crate::scoreset::pane(ui, &mut app.proj, "Clefs & Signatures"),
                4 => {
                    ui.label("Dynamics: the velocity each Part box dynamic gives notes");
                    for (k, n) in DYNAMICS.iter().enumerate() {
                        let mut v = st.dyn_velocity[k] as f32;
                        if ui.add(egui::Slider::new(&mut v, 1.0..=127.0).text(*n)).changed() {
                            st.dyn_velocity[k] = v as u8;
                        }
                    }
                }
                _ => {
                    for (c, n) in [(Colors::Normal, "Normal"), (Colors::Pitch, "Pitch"), (Colors::Velocity, "Velocity"), (Colors::Voice, "Voice Assignment"), (Colors::BlackWhite, "Black & White")] {
                        if ui.radio(st.colors == c, n).clicked() {
                            st.colors = c;
                        }
                    }
                }
            }
        });
        st.settings = open.then_some(tab);
    }
    if st.styles_window {
        let mut open = true;
        let cur = style_of(app, t);
        let mut pick = None;
        egui::Window::new("Staff Style").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.label(egui::RichText::new("The selected track's staff style").size(11.0));
            if ui.selectable_label(app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| app.project.as_ref()?.score.track(tr.extra.id)).is_none_or(|ts| ts.style.is_empty()), "Auto").clicked() {
                pick = Some("");
            }
            for s in nt::StaffStyle::ALL {
                let detail = format!("{}  ({} staff{}{})", s.name, s.clefs.len(), if s.clefs.len() > 1 { "s" } else { "" }, if s.transpose != 0 { format!(", transpose {:+}", s.transpose) } else { String::new() });
                if ui.selectable_label(cur.name == s.name, detail).clicked() {
                    pick = Some(s.name);
                }
            }
        });
        if let Some(n) = pick {
            set_style(app, t, n);
        }
        st.styles_window = open;
    }
}

pub const SETTINGS_TABS: [&str; 6] = ["Global", "Numbers & Names", "Chords & Grids", "Clefs & Signatures", "MIDI Meaning", "Colors"];

/// Sets a track's staff style (one undo step).
pub fn set_style(app: &mut App, t: usize, name: &str) {
    region_param(app, t, "Staff Style", |ts| ts.style = name.to_string());
}

pub(crate) fn region_param(app: &mut App, t: usize, what: &str, f: impl FnOnce(&mut lg_core::score::TrackScore)) {
    let Some(id) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|t| t.extra.id) else { return };
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        f(p.score.track_mut(id));
    }
    app.after_edit();
}

/// Adds a symbol to a track's staff (one undo step).
pub fn add_symbol(app: &mut App, t: usize, s: Symbol, what: &str) {
    let Some(id) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|t| t.extra.id) else { return };
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        let ts = p.score.track_mut(id);
        // a clef change replaces one at the same bar
        if s.kind == SymbolKind::Clef {
            ts.symbols.retain(|x| !(x.kind == SymbolKind::Clef && x.tick == s.tick && x.staff == s.staff));
        }
        ts.symbols.push(s);
        ts.symbols.sort_by_key(|x| x.tick);
    }
    app.after_edit();
}

/// The drum a drum staff position stands for (the first in the map).
fn drum_at(pos: i32) -> u8 {
    (35u8..=81).find(|&p| nt::drum_position(p).0 == pos).unwrap_or(38)
}

/// A double-click on a staff: the Part box's symbol there.
fn insert_at(app: &mut App, st: &mut State, row: &Row, tick: f32, pos: i32, at: Pos2) {
    let bar_t = app.bar_ticks();
    let (q, _, _) = quant_of(app, row.id);
    let grid = nt::Quant::CHOICES[q].1.straight as f32;
    let tick_q = ((tick / grid).floor() * grid).max(0.0) as u32;
    let key = app.project.as_ref().map_or(nt::Key::default(), |p| nt::Key { fifths: p.score.key_at(tick_q, nt::Key::parse(&p.key).fifths) });
    match st.part {
        Part::Note(v) => {
            let (clef, _) = clef_at(app, row, tick_q);
            let pitch = if clef == nt::Clef::Percussion {
                drum_at(pos)
            } else {
                let wk = row.style.written_key(key);
                (nt::pitch_at(clef, pos, wk) as i32 - row.style.transpose as i32).clamp(0, 127) as u8
            };
            let mut len = nt::Value { base: VALUES[v].1, dots: st.dotted as u8 }.ticks();
            if st.triplet {
                len = len * 2 / 3;
            }
            insert_note(app, row.t, tick_q, pitch, len, 80);
        }
        Part::Rest(_) => app.tell("Rests are drawn by the Score itself (user rests aren't available yet)"),
        Part::Artic(_) | Part::Dynamic(_) => {
            // on the note nearest the click, or (a dynamic) at the place
            if let Some((rix, i)) = nearest_note(app, row.t, tick_q) {
                app.selected_region = Some((row.t, rix));
                app.selected_notes = vec![i];
                apply_part(app, st, st.part);
            } else if let Part::Dynamic(d) = st.part {
                add_symbol(app, row.t, Symbol { tick: tick_q, kind: SymbolKind::Dynamic, text: DYNAMICS[d].into(), len: 0, staff: row.staff as u8, ..Default::default() }, "Insert Dynamic");
            }
        }
        Part::Line(k) => {
            let kind = [SymbolKind::SlurUp, SymbolKind::SlurDown, SymbolKind::Crescendo, SymbolKind::Decrescendo][k];
            add_symbol(app, row.t, Symbol { tick: tick_q, kind, text: String::new(), len: bar_t / 2, staff: row.staff as u8, ..Default::default() }, LINES[k]);
        }
        Part::Clef(k) => {
            let at_bar = tick_q / bar_t * bar_t;
            add_symbol(app, row.t, Symbol { tick: at_bar, kind: SymbolKind::Clef, text: CLEFS[k].into(), len: 0, staff: row.staff as u8, ..Default::default() }, "Insert Clef");
        }
        Part::Text(k) if k >= 3 => {
            add_symbol(app, row.t, Symbol { tick: tick_q, kind: SymbolKind::AutoText, text: TEXTS[k].into(), staff: row.staff as u8, ..Default::default() }, "Insert Text");
        }
        Part::Text(k) => {
            let kind = [SymbolKind::Text, SymbolKind::Lyric, SymbolKind::Chord][k];
            st.typing = Some((row.t, tick_q, kind, row.staff as u8, String::new(), at));
        }
        Part::Head(k) => {
            if let Some((rix, i)) = nearest_note(app, row.t, tick_q) {
                app.selected_region = Some((row.t, rix));
                if !app.selected_notes.contains(&i) {
                    app.selected_notes = vec![i];
                }
                apply_part(app, st, Part::Head(k));
            }
        }
        Part::Pedal(_) => crate::scoreinsp::insert_pedal(app, row.t, tick_q),
        Part::Bar(k) => {
            let kind = lg_core::score::BarKind::ALL[k];
            crate::scoreinsp::insert_bar(app, crate::scoreinsp::bar_snap(tick, bar_t, grid as u32, kind), kind);
        }
        Part::Ornament(k) => {
            add_symbol(app, row.t, Symbol { tick: tick_q, kind: SymbolKind::Ornament, text: lg_core::score::ORNAMENTS[k].into(), staff: row.staff as u8, ..Default::default() }, "Insert Symbol");
        }
        Part::Key(k) => insert_key(app, tick_q / bar_t * bar_t, k as i8 - 7),
        Part::Time(k) => crate::scoreinsp::insert_time(app, tick_q / bar_t, k),
        Part::Break(k) => crate::scoreinsp::insert_break(app, ((tick + bar_t as f32 / 2.0) / bar_t as f32).floor() as u32, lg_core::score::Break::ALL[k]),
    }
}

/// The note of a track nearest a tick: (region, index).
fn nearest_note(app: &App, t: usize, tick: u32) -> Option<(usize, usize)> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    tr.regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.audio.is_none())
        .flat_map(|(rix, r)| r.notes.iter().enumerate().map(move |(i, n)| (rix, i, n.pos.abs_diff(tick))))
        .filter(|x| x.2 <= 480)
        .min_by_key(|x| x.2)
        .map(|x| (x.0, x.1))
}

/// A note into the track's region at `tick` (or the selected region), selected (one undo step).
pub fn insert_note(app: &mut App, t: usize, tick: u32, pitch: u8, len: u32, velocity: u8) -> bool {
    let Some(track) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return false };
    let rix = track.regions.iter().position(|r| r.audio.is_none() && tick >= r.start && tick < r.start + r.length).or(app.selected_region.filter(|x| x.0 == t).map(|x| x.1));
    let Some(rix) = rix else { return false };
    app.checkpoint("Insert events");
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(rix)) {
        rg.notes.push(Note { channel: 0, pitch, velocity, pos: tick, len, muted: false });
        rg.notes.sort_by_key(|n| (n.pos, n.pitch));
        let idx = rg.notes.iter().position(|n| n.pos == tick && n.pitch == pitch).unwrap_or(0);
        app.selected_region = Some((t, rix));
        app.selected_notes = vec![idx];
    }
    app.after_edit();
    crate::pianoroll::preview(app, t, pitch);
    true
}

/// The selected notes as (pos, pitch) keys, with their track's id.
pub fn selected_keys(app: &App) -> Option<(u64, Vec<(u32, u8)>)> {
    let (t, r) = app.selected_region?;
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let rg = tr.regions.get(r)?;
    let keys: Vec<(u32, u8)> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).map(|n| (n.pos, n.pitch)).collect();
    (!keys.is_empty()).then_some((tr.extra.id, keys))
}

/// Changes the selected notes' attributes (one undo step); false with nothing selected.
pub fn edit_attrs(app: &mut App, what: &str, f: impl Fn(&mut NoteAttr)) -> bool {
    let Some((id, keys)) = selected_keys(app) else { return false };
    app.checkpoint(what);
    if let Some(p) = app.project.as_mut() {
        for (pos, pitch) in keys {
            p.score.edit(id, pos, pitch, &f);
        }
    }
    app.after_edit();
    true
}

/// A Part box symbol applied to the selected notes (articulations toggle; dynamics set the
/// velocity by MIDI Meaning and place the mark; lines span the selection; clefs change there).
pub fn apply_part(app: &mut App, st: &State, part: Part) -> bool {
    let Some((t, r)) = app.selected_region else { return false };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
    let sel: Vec<Note> = app.selected_notes.iter().filter_map(|&i| rg.notes.get(i)).copied().collect();
    if sel.is_empty() {
        return false;
    }
    let first = sel.iter().map(|n| n.pos).min().unwrap_or(0);
    let last = sel.iter().map(|n| n.pos + n.len).max().unwrap_or(first);
    let staff = style_of(app, t).staff_of(sel[0].pitch) as u8;
    match part {
        Part::Artic(k) => {
            let bit = 1u32 << PART_ARTICS[k];
            let all = selected_keys(app).is_some_and(|(id, keys)| keys.iter().all(|&(p, q)| app.project.as_ref().is_some_and(|pr| pr.score.attr(id, p, q).artic & bit != 0)));
            edit_attrs(app, "Attach Symbol", |a| if all { a.artic &= !bit } else { a.artic |= bit })
        }
        Part::Dynamic(d) => {
            let vel = st.dyn_velocity[d];
            let id = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0, |t| t.extra.id);
            app.checkpoint("Insert Dynamic");
            let idx = app.selected_notes.clone();
            if let Some(p) = app.project.as_mut() {
                if let Some(region) = p.tracks.get_mut(t).and_then(|tr| tr.regions.get_mut(r)) {
                    for &k in &idx {
                        if let Some(n) = region.notes.get_mut(k) {
                            n.velocity = vel;
                        }
                    }
                }
                let ts = p.score.track_mut(id);
                ts.symbols.retain(|s| !(s.kind == SymbolKind::Dynamic && s.tick == first && s.staff == staff));
                ts.symbols.push(Symbol { tick: first, kind: SymbolKind::Dynamic, text: DYNAMICS[d].into(), len: 0, staff, ..Default::default() });
                ts.symbols.sort_by_key(|s| s.tick);
            }
            app.after_edit();
            true
        }
        Part::Line(k) => {
            let kind = [SymbolKind::SlurUp, SymbolKind::SlurDown, SymbolKind::Crescendo, SymbolKind::Decrescendo][k];
            add_symbol(app, t, Symbol { tick: first, kind, text: String::new(), len: (last - first).max(240), staff, ..Default::default() }, LINES[k]);
            true
        }
        Part::Clef(k) => {
            let bar = app.bar_ticks();
            add_symbol(app, t, Symbol { tick: first / bar * bar, kind: SymbolKind::Clef, text: CLEFS[k].into(), len: 0, staff, ..Default::default() }, "Insert Clef");
            true
        }
        // note heads are visual only; the normal head restores the standard one
        Part::Head(k) => edit_attrs(app, "Note Head", |a| a.head = k as u8),
        _ => false,
    }
}

/// The staff header's key and time signature: a click (Linear view) or Control-click opens
/// the Key Signature or Time Signature window.
fn header_clicks(app: &mut App, ui: &mut egui::Ui, r: Rect, x: f32, sp: f32, key: nt::Key, sig: (u8, u8), salt: (usize, bool)) {
    if r.width() <= 0.0 {
        return;
    }
    let resp = ui.interact(r, ui.scope_id().with(("sc-header", salt)), Sense::click());
    let key_end = x + (3.0 + key.fifths.unsigned_abs() as f32 * 1.1) * sp;
    let which = |pp: Pos2| if pp.x < x + 2.6 * sp { None } else if pp.x < key_end + 0.4 * sp { Some(true) } else { Some(false) };
    let ctx = ui.ctx().clone();
    let key_dlg = Dialog::Key { at: 0, fifths: key.fifths, minor: app.project.as_ref().is_some_and(|p| p.key.contains("minor")) };
    let time_dlg = Dialog::Time { bar: 0, beats: sig.0.to_string(), value: [1u8, 2, 4, 8, 16, 32].iter().position(|&d| d == sig.1).unwrap_or(2) };
    if salt.1 && resp.clicked() {
        match resp.interact_pointer_pos().and_then(which) {
            Some(true) => crate::scoreinsp::open_dialog(&ctx, key_dlg.clone()),
            Some(false) => crate::scoreinsp::open_dialog(&ctx, time_dlg.clone()),
            None => {}
        }
    }
    resp.context_menu(|ui| {
        if ui.button("Edit Key Signature…").clicked() {
            crate::scoreinsp::open_dialog(&ctx, key_dlg.clone());
            ui.close();
        }
        if ui.button("Edit Time Signature…").clicked() {
            crate::scoreinsp::open_dialog(&ctx, time_dlg.clone());
            ui.close();
        }
    });
}

/// A key change into the score at a bar (at bar 1: the project's key itself).
pub fn insert_key(app: &mut App, at: u32, fifths: i8) {
    app.checkpoint("Insert Key Signature");
    if let Some(p) = app.project.as_mut() {
        if at == 0 {
            p.key = key_name(fifths);
            p.score.keys.retain(|k| k.0 != 0);
        } else {
            p.score.keys.retain(|k| k.0 != at);
            p.score.keys.push((at, fifths));
            p.score.keys.sort_unstable();
        }
    }
    app.after_edit();
}

pub fn key_name(fifths: i8) -> String {
    const N: [&str; 15] = ["C♭", "G♭", "D♭", "A♭", "E♭", "B♭", "F", "C", "G", "D", "A", "E", "B", "F♯", "C♯"];
    format!("{} major", N[(fifths + 7).clamp(0, 14) as usize])
}

// ---- drawing -------------------------------------------------------------------------------

pub(crate) const GCLEF: char = '\u{E050}';
const GCLEF_8VB: char = '\u{E052}';
const GCLEF_8VA: char = '\u{E053}';
pub(crate) const CCLEF: char = '\u{E05C}';
pub(crate) const FCLEF: char = '\u{E062}';
const FCLEF_8VB: char = '\u{E064}';
pub(crate) const PERC_CLEF: char = '\u{E069}';

/// Clef, key signature and (the first line's) time signature from `x`.
fn draw_header(p: &egui::Painter, x: f32, bottom: f32, sp: f32, show_clef: bool, clef: nt::Clef, octave: i8, key: nt::Key, time: Option<(u8, u8)>) {
    let mut x = x;
    // SMuFL clefs: origin on their line (G: 2nd, F: 4th, C: its line, percussion: middle)
    let (ch, line) = match clef {
        nt::Clef::Treble => (if octave < 0 { GCLEF_8VB } else if octave > 0 { GCLEF_8VA } else { GCLEF }, 1.0),
        nt::Clef::Bass => (if octave < 0 { FCLEF_8VB } else { FCLEF }, 3.0),
        nt::Clef::Alto => (CCLEF, 2.0),
        nt::Clef::Tenor => (CCLEF, 3.0),
        nt::Clef::Percussion => (PERC_CLEF, 2.0),
    };
    if show_clef {
        smufl(p, ch, pos2(x, bottom - line * sp), sp, INK, false);
        x += 3.4 * sp;
    }
    if clef != nt::Clef::Percussion {
        for (pos, acc) in key.signature_positions(clef) {
            accidental(p, pos2(x, bottom - pos as f32 * sp / 2.0), sp, acc, INK);
            x += 1.1 * sp;
        }
    }
    x += 0.8 * sp;
    if let Some(sig) = time {
        // SMuFL time signature digits, centred on the fourth and second lines
        let digits = |n: u8| -> String { n.to_string().chars().map(|d| char::from_u32(0xE080 + d.to_digit(10).unwrap_or(0)).unwrap_or(' ')).collect() };
        for (n, line) in [(sig.0, 3.0), (sig.1, 1.0)] {
            let g = p.layout_no_wrap(digits(n), look::music(4.0 * sp), INK);
            let base = g.rows.first().and_then(|r| r.glyphs.first()).map_or(0.0, |gl| gl.pos.y);
            let w = g.size().x;
            p.galley(pos2(x + sp - w / 2.0, bottom - line * sp - base), g, INK);
        }
    }
}

/// "mf" → SMuFL dynamics letters (p m f r s z n).
pub(crate) fn dynamic_glyphs(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'p' => '\u{E520}',
            'm' => '\u{E521}',
            'f' => '\u{E522}',
            'r' => '\u{E523}',
            's' => '\u{E524}',
            'z' => '\u{E525}',
            'n' => '\u{E526}',
            c => c,
        })
        .collect()
}

/// A string of SMuFL glyphs centred at `at` (its baseline).
pub(crate) fn smufl_text(p: &egui::Painter, s: &str, at: Pos2, sp: f32, col: Color32) {
    let g = p.layout_no_wrap(s.to_string(), look::music(4.0 * sp), col);
    let base = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    p.galley(pos2(at.x - g.size().x / 2.0, at.y - base), g, col);
}

/// An articulation (index into ARTICULATIONS) at `at`, above or below the note.
pub(crate) fn artic_glyph(p: &egui::Painter, a: usize, at: Pos2, sp: f32, above: bool, col: Color32) {
    let pick = |up: u32| char::from_u32(if above { up } else { up + 1 }).unwrap_or(' ');
    let ch = match a {
        0 => '\u{E4C0}',
        1 => '\u{E4C1}',
        2 => pick(0xE4A2),
        3 => pick(0xE4A6),
        4 => pick(0xE4A0),
        5 => pick(0xE4AC),
        6 => pick(0xE4B4),
        7 => pick(0xE4A4),
        8 => '\u{E612}',
        9 => '\u{E610}',
        10 => '\u{E614}',
        11 => '\u{E633}',
        14 => pick(0xE4B6),
        15 => pick(0xE4AE),
        16 => '\u{E5D0}',
        17 => '\u{E5D1}',
        18 => '\u{E5D4}',
        19 => '\u{E5D7}',
        20 => '\u{E5E0}',
        21 => '\u{E5E3}',
        _ => {
            p.text(at, Align2::CENTER_CENTER, if a == 12 { "L.H." } else { "R.H." }, look::regular(1.6 * sp), col);
            return;
        }
    };
    let g = p.layout_no_wrap(ch.to_string(), look::music(4.0 * sp), col);
    // centred on `at` by its ink (the glyph box's middle)
    let base = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    p.galley(pos2(at.x - g.size().x / 2.0, at.y - base), g, col);
}

/// A text style's font at staff space `sp` (Logic's sizes are points on the page, where a staff
/// space is about 7 points at the default scale).
pub(crate) fn style_font(st: &lg_core::score::TextStyle, sp: f32) -> egui::FontId {
    let size = st.size / 7.0 * sp;
    match (st.family.as_str(), st.bold) {
        ("Monospace", _) => egui::FontId::monospace(size),
        ("Bravura", _) => look::music(size),
        (_, true) => look::bold(size),
        _ => egui::FontId::new(size, egui::FontFamily::Proportional),
    }
}

/// How a staff symbol's text is drawn: its style and what automatic text stands for.
pub(crate) struct SymText<'a> {
    pub style: lg_core::score::TextStyle,
    pub auto: &'a dyn Fn(&str) -> String,
    /// The Chord Extension style.
    pub ext: &'a dyn Fn() -> lg_core::score::TextStyle,
}

/// A chord symbol's lower line (root, letters before the first number), its raised extension
/// and its slash bass: "Cmaj7/E" → ("Cmaj", "7", "/E"); a comma splits by hand ("Cm,add9").
pub(crate) fn chord_parts(text: &str) -> (String, String, String) {
    let (main, bass) = match text.rfind('/') {
        Some(i) if i > 0 && text[i + 1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => (&text[..i], text[i..].to_string()),
        _ => (text, String::new()),
    };
    if let Some((a, b)) = main.split_once(',') {
        return (a.to_string(), b.replace(',', ""), bass);
    }
    match main.char_indices().skip(1).find(|(_, c)| c.is_ascii_digit()) {
        Some((i, _)) => (main[..i].to_string(), main[i..].to_string(), bass),
        None => (main.to_string(), String::new(), bass),
    }
}

/// Text at `at` with a style (underline drawn), aligned by the symbol's Align.
fn styled_text(p: &egui::Painter, s: &Symbol, at: Pos2, text: &str, t: &SymText, sp: f32, col: Color32) -> Rect {
    let al = match s.align {
        1 => Align2::CENTER_CENTER,
        2 => Align2::RIGHT_CENTER,
        _ => Align2::LEFT_CENTER,
    };
    let at = at + vec2(s.dx as f32 * sp / 10.0, -(s.dy as f32) * sp / 10.0);
    // (italic and underline as the style says)
    let fmt = egui::TextFormat { font_id: style_font(&t.style, sp), color: col, italics: t.style.italic, underline: if t.style.underline { Stroke::new(1.0, col) } else { Stroke::NONE }, ..Default::default() };
    let g = p.layout_job(egui::text::LayoutJob::single_section(text.to_string(), fmt));
    let r = al.anchor_size(at, g.size());
    p.galley(r.min, g, col);
    r
}

/// A staff symbol between x and x2 on a staff whose bottom line is at `bottom`; text symbols
/// return where their text went (for clicks).
fn draw_symbol(p: &egui::Painter, s: &Symbol, x: f32, x2: f32, bottom: f32, sp: f32, chord_size: f32, t: &SymText) -> Option<Rect> {
    let top = bottom - 4.0 * sp;
    match s.kind {
        SymbolKind::Dynamic => smufl_text(p, &dynamic_glyphs(&s.text), pos2(x + 0.6 * sp, bottom + 3.6 * sp), sp, INK),
        SymbolKind::Crescendo | SymbolKind::Decrescendo => {
            let y = bottom + 3.2 * sp;
            let (open, close) = if s.kind == SymbolKind::Crescendo { (x2, x) } else { (x, x2) };
            let st = Stroke::new(1.2, INK);
            p.line_segment([pos2(close, y), pos2(open, y - 0.7 * sp)], st);
            p.line_segment([pos2(close, y), pos2(open, y + 0.7 * sp)], st);
        }
        SymbolKind::Text => return Some(styled_text(p, s, pos2(x, top - 2.6 * sp), &s.text, t, sp, INK)),
        SymbolKind::AutoText => return Some(styled_text(p, s, pos2(x, top - 2.6 * sp), &(t.auto)(&s.text), t, sp, INK)),
        SymbolKind::Chord => {
            // root (and letters before the first number) on the line, the rest raised in the
            // Chord Extension style; a comma splits them by hand; a slash bass follows the root
            let (lower, upper, bass) = chord_parts(&s.text);
            let mut root = t.style.clone();
            root.size *= chord_size;
            let mut ext = (t.ext)();
            ext.size *= chord_size;
            let at = pos2(x, top - 4.4 * sp) + vec2(s.dx as f32 * sp / 10.0, -(s.dy as f32) * sp / 10.0);
            let r1 = p.text(at, Align2::LEFT_CENTER, &lower, style_font(&root, sp), INK);
            let mut r = r1;
            if !upper.is_empty() {
                let r2 = p.text(pos2(r1.max.x + 0.1 * sp, r1.min.y + 0.1 * sp), Align2::LEFT_CENTER, &upper, style_font(&ext, sp), INK);
                r = r.union(r2);
            }
            if !bass.is_empty() {
                let r3 = p.text(pos2(r.max.x, at.y), Align2::LEFT_CENTER, &bass, style_font(&root, sp), INK);
                r = r.union(r3);
            }
            return Some(r);
        }
        SymbolKind::Lyric => {
            let mut s2 = s.clone();
            if s2.align == 0 {
                s2.align = 1;
            }
            return Some(styled_text(p, &s2, pos2(x + 0.6 * sp, bottom + 6.0 * sp), &s.text, t, sp, INK));
        }
        SymbolKind::Ornament => ornament(p, &s.text, x, x2, top, bottom, sp, INK),
        SymbolKind::SlurUp | SymbolKind::SlurDown => {
            let up = s.kind == SymbolKind::SlurUp;
            let y = if up { top - 1.0 * sp } else { bottom + 1.0 * sp };
            let d = if up { -1.0 } else { 1.0 };
            tie(p, pos2(x + 0.6 * sp, y), pos2(x2.max(x + 2.0 * sp), y), d * (1.2 * sp + (x2 - x) * 0.05).min(3.0 * sp), sp, INK);
        }
        SymbolKind::Clef => {
            let c = clef_named(&s.text);
            let (ch, line) = match c {
                nt::Clef::Treble => (GCLEF, 1.0),
                nt::Clef::Bass => (FCLEF, 3.0),
                nt::Clef::Alto => (CCLEF, 2.0),
                nt::Clef::Tenor => (CCLEF, 3.0),
                nt::Clef::Percussion => (PERC_CLEF, 2.0),
            };
            // a clef change is drawn smaller, before the bar's first note
            smufl(p, ch, pos2(x - 2.4 * sp, bottom - line * sp), sp * 0.75, INK, false);
        }
    }
    None
}

/// A bar line of a Bars and Repeats kind at x on a staff (repeat dots in the 2nd and 3rd spaces).
pub(crate) fn draw_bar(p: &egui::Painter, k: lg_core::score::BarKind, x: f32, bottom: f32, sp: f32) {
    use lg_core::score::BarKind as BK;
    let (y0, y1) = (bottom - 4.0 * sp, bottom);
    let thin = Stroke::new(1.0, INK);
    let thick = Stroke::new((0.5 * sp).max(2.0), INK);
    let dots = |dx: f32| {
        for y in [bottom - 1.5 * sp, bottom - 2.5 * sp] {
            p.circle_filled(pos2(x + dx, y), 0.25 * sp, INK);
        }
    };
    match k {
        BK::Double => {
            p.vline(x - 0.5 * sp, y0..=y1, thin);
            p.vline(x, y0..=y1, thin);
        }
        BK::Final => {
            p.vline(x - 0.8 * sp, y0..=y1, thin);
            p.vline(x - 0.25 * sp, y0..=y1, thick);
        }
        BK::Dashed => {
            let mut y = y0;
            while y < y1 {
                p.vline(x, y..=(y + 0.5 * sp).min(y1), thin);
                y += 0.8 * sp;
            }
        }
        BK::RepeatStart => {
            p.vline(x + 0.25 * sp, y0..=y1, thick);
            p.vline(x + 0.8 * sp, y0..=y1, thin);
            dots(1.3 * sp);
        }
        BK::RepeatEnd => {
            p.vline(x - 0.25 * sp, y0..=y1, thick);
            p.vline(x - 0.8 * sp, y0..=y1, thin);
            dots(-1.3 * sp);
        }
        BK::RepeatBoth => {
            p.vline(x, y0..=y1, thick);
            p.vline(x - 0.55 * sp, y0..=y1, thin);
            p.vline(x + 0.55 * sp, y0..=y1, thin);
            dots(-1.05 * sp);
            dots(1.05 * sp);
        }
        _ => {
            p.vline(x, y0..=y1, thin);
        }
    }
}

/// A Trills group symbol (lg-core ORNAMENTS) above the staff at x (a trill line to x2).
pub(crate) fn ornament(p: &egui::Painter, name: &str, x: f32, x2: f32, top: f32, bottom: f32, sp: f32, col: Color32) {
    let above = pos2(x + 0.6 * sp, top - 1.6 * sp);
    match name {
        "Trill" => smufl(p, '\u{E566}', above, sp, col, true),
        "Trill Line" => {
            smufl(p, '\u{E566}', above, sp, col, true);
            let mut cx = above.x + 1.6 * sp;
            while cx < x2.max(x + 6.0 * sp) {
                smufl(p, '\u{EAA4}', pos2(cx, above.y), sp, col, false);
                cx += 0.9 * sp;
            }
        }
        "Mordent" => smufl(p, '\u{E56D}', above, sp, col, true),
        "Inverted Mordent" => smufl(p, '\u{E56C}', above, sp, col, true),
        "Turn" => smufl(p, '\u{E567}', above, sp, col, true),
        "Inverted Turn" => smufl(p, '\u{E568}', above, sp, col, true),
        "Arpeggio" => {
            let mut y = bottom + 0.5 * sp;
            while y > top - 0.5 * sp {
                smufl(p, '\u{EAA9}', pos2(x - 0.4 * sp, y), sp, col, true);
                y -= 1.0 * sp;
            }
        }
        "Tremolo 1" => smufl(p, '\u{E220}', pos2(x + 0.6 * sp, top + 1.0 * sp), sp, col, true),
        "Tremolo 2" => smufl(p, '\u{E221}', pos2(x + 0.6 * sp, top + 1.0 * sp), sp, col, true),
        "Tremolo 3" => smufl(p, '\u{E222}', pos2(x + 0.6 * sp, top + 1.0 * sp), sp, col, true),
        "Sharp" => smufl(p, '\u{E262}', above, sp * 0.7, col, true),
        "Flat" => smufl(p, '\u{E260}', above, sp * 0.7, col, true),
        _ => smufl(p, '\u{E261}', above, sp * 0.7, col, true),
    }
}

/// How draw_voice colours and decorates: per note colour and attributes, rests shown or not,
/// which ticks belong to the line being drawn.
struct DrawOpts<'a> {
    color: &'a dyn Fn(usize) -> Color32,
    attr: &'a dyn Fn(usize) -> NoteAttr,
    hide_rests: bool,
    in_line: &'a dyn Fn(u32) -> bool,
}

fn y_of(staff: &Staff, pos: i32, sp: f32) -> f32 {
    staff.bottom - pos as f32 * sp / 2.0
}

/// Where a head is drawn: right of the item's time, set off for seconds.
fn head_center(it: &nt::Item, h: &nt::Head, staff: &Staff, sp: f32, x: f32) -> Pos2 {
    let w = head_w(it.value, sp);
    let mut cx = x + 0.8 * sp + w / 2.0;
    if h.shifted {
        cx += if it.stem_up { w - 0.1 * sp } else { -(w - 0.1 * sp) };
    }
    pos2(cx, y_of(staff, h.pos, sp))
}

fn head_w(v: nt::Value, sp: f32) -> f32 {
    if v.base == 1 { 1.6 * sp } else { 1.25 * sp }
}

fn ellipse(c: Pos2, rx: f32, ry: f32, angle: f32, n: usize) -> Vec<Pos2> {
    let (s, co) = angle.sin_cos();
    (0..n)
        .map(|k| {
            let t = k as f32 / n as f32 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            pos2(c.x + x * co - y * s, c.y + x * s + y * co)
        })
        .collect()
}

/// A notehead: filled (quarter and shorter), open (half), whole.
pub(crate) fn notehead(p: &egui::Painter, c: Pos2, sp: f32, base: u8, col: Color32, paper: Color32) {
    let _ = paper;
    // SMuFL noteheadWhole / noteheadHalf / noteheadBlack: centred on the baseline
    let ch = match base {
        1 => '\u{E0A2}',
        2 => '\u{E0A3}',
        _ => '\u{E0A4}',
    };
    smufl(p, ch, c, sp, col, true);
}

/// A Part box note head (lg-core NOTE_HEADS), SMuFL: black / white forms by the value.
pub(crate) fn special_head(p: &egui::Painter, c: Pos2, sp: f32, base: u8, head: u8, col: Color32) {
    let open = base <= 2;
    let ch = match head {
        1 => ['\u{E0A7}', '\u{E0A8}', '\u{E0A9}'][(base.max(1) as usize).min(4).saturating_sub(1).min(2)],
        2 => '\u{E0B3}',
        3 => if open { '\u{E0D9}' } else { '\u{E0DB}' },
        4 => '\u{E0D9}',
        5 => if open { '\u{E103}' } else { '\u{E101}' },
        6 => if open { '\u{E0BD}' } else { '\u{E0BE}' },
        7 => if open { '\u{E0B8}' } else { '\u{E0B9}' },
        8 => '\u{E0B8}',
        9 => '\u{E0D9}',
        10 => {
            notehead(p, c, sp, base, col, Color32::TRANSPARENT);
            smufl(p, '\u{E0F5}', c - vec2(0.95 * sp, 0.0), sp, col, true);
            smufl(p, '\u{E0F6}', c + vec2(0.95 * sp, 0.0), sp, col, true);
            return;
        }
        // gray: shown grey on screen, left out of print
        _ => {
            notehead(p, c, sp, base, Color32::from_gray(0x90), Color32::TRANSPARENT);
            return;
        }
    };
    smufl(p, ch, c, sp, col, true);
}

fn draw_voice(p: &egui::Painter, v: &nt::Voice, staff: &Staff, sp: f32, x_of: &dyn Fn(f32) -> f32, o: &DrawOpts) {
    let stem_w = (0.12 * sp).max(1.0);
    // stem ends, for beams
    let mut stems: Vec<Option<(f32, f32, f32)>> = vec![None; v.items.len()];
    // pitches tied into the item from the one before (no articulation repeated on them)
    let mut tied_in: Vec<u8> = Vec::new();
    for (k, it) in v.items.iter().enumerate() {
        let here = (o.in_line)(it.tick);
        let x = x_of(it.tick as f32);
        if it.is_rest() {
            if here && !o.hide_rests {
                let cx = if it.full_bar { x_of((it.tick + it.dur / 2) as f32) } else { x + 0.8 * sp + 0.6 * sp };
                rest(p, cx, staff.bottom, sp, it.value, it.full_bar, INK);
            }
            tied_in.clear();
            continue;
        }
        let next_tied: Vec<u8> = it.heads.iter().filter(|h| h.tie).map(|h| h.pitch).collect();
        if !here {
            tied_in = next_tied;
            continue;
        }
        let stem_col = it.heads.first().map_or(INK, |h| (o.color)(h.note));
        let w = head_w(it.value, sp);
        let hi = it.heads.iter().map(|h| h.pos).max().unwrap_or(0);
        let lo = it.heads.iter().map(|h| h.pos).min().unwrap_or(0);
        // ledger lines (none on a drum staff's cross heads above it… they still need them)
        let base_x = x + 0.8 * sp;
        let shifted = it.heads.iter().any(|h| h.shifted);
        let lx0 = base_x - 0.35 * sp - if shifted && !it.stem_up { w } else { 0.0 };
        let lx1 = base_x + w + 0.35 * sp + if shifted && it.stem_up { w } else { 0.0 };
        let mut l = 10;
        while l <= hi {
            p.hline(lx0..=lx1, y_of(staff, l, sp), Stroke::new(1.0, INK));
            l += 2;
        }
        let mut l = -2;
        while l >= lo {
            p.hline(lx0..=lx1, y_of(staff, l, sp), Stroke::new(1.0, INK));
            l -= 2;
        }
        // heads, accidentals, dots, ties
        let mut acc_x = base_x - 0.9 * sp - if shifted && !it.stem_up { w } else { 0.0 };
        let mut last_acc_pos = i32::MAX;
        let n_heads = it.heads.len();
        for (h_k, h) in it.heads.iter().enumerate().rev() {
            let c = head_center(it, h, staff, sp, x);
            let col = (o.color)(h.note);
            let head = (o.attr)(h.note).head;
            if head != 0 {
                special_head(p, c, sp, it.value.base, head, col);
            } else if h.cross {
                smufl(p, '\u{E0A9}', c, sp, col, true);
            } else {
                notehead(p, c, sp, it.value.base, col, REGION_PAPER);
            }
            if let Some(a) = h.acc {
                if last_acc_pos != i32::MAX && (last_acc_pos - h.pos).abs() < 6 {
                    acc_x -= 1.1 * sp;
                }
                if h.guide {
                    // a guide accidental, in brackets
                    p.text(pos2(acc_x - 0.9 * sp, c.y), Align2::CENTER_CENTER, "(", egui::FontId::proportional(2.2 * sp), col);
                    p.text(pos2(acc_x + 0.75 * sp, c.y), Align2::CENTER_CENTER, ")", egui::FontId::proportional(2.2 * sp), col);
                }
                accidental(p, pos2(acc_x, c.y), sp, a, col);
                if h.guide {
                    acc_x -= 0.9 * sp;
                }
                last_acc_pos = h.pos;
            }
            for d in 0..it.value.dots {
                let dy = if h.pos % 2 == 0 { -sp / 2.0 } else { 0.0 };
                p.circle_filled(pos2(base_x + w + 0.5 * sp + d as f32 * 0.6 * sp + if shifted && it.stem_up { w } else { 0.0 }, c.y + dy), 0.18 * sp, col);
            }
            if h.tie && !h.tie_hidden {
                let next = v.items.get(k + 1).and_then(|next| next.heads.iter().find(|x| x.pitch == h.pitch).map(|nh| (next, nh)));
                // a chord's lower notes tie below, upper above; one note away from the stem
                let below = match h.tie_dir {
                    1 => false,
                    -1 => true,
                    _ => if n_heads > 1 && (n_heads % 2 == 0 || h_k != n_heads / 2) { h_k < n_heads / 2 } else { it.stem_up },
                };
                let dir = if below { 1.0 } else { -1.0 };
                let a = pos2(c.x + 0.5 * sp, c.y + dir * 0.45 * sp);
                let b = match next {
                    // into the next line: a tie to the line's end
                    Some((next, nh)) if (o.in_line)(next.tick) => {
                        let c2 = head_center(next, nh, staff, sp, x_of(next.tick as f32));
                        pos2(c2.x - 0.5 * sp, c2.y + dir * 0.45 * sp)
                    }
                    _ => pos2(c.x + 3.0 * sp, c.y + dir * 0.45 * sp),
                };
                let lift = dir * (0.6 * sp + (b.x - a.x) * 0.05).min(1.6 * sp);
                tie(p, a, b, lift, sp, col);
            }
        }
        // articulations and fingering: on the side away from the stem (fermatas above), not on
        // tied continuations
        let fresh: Vec<&nt::Head> = it.heads.iter().filter(|h| !tied_in.contains(&h.pitch)).collect();
        let mut bits = 0u32;
        let mut fingers: Vec<String> = Vec::new();
        for h in &fresh {
            let a = (o.attr)(h.note);
            bits |= a.artic;
            if a.finger > 0 {
                fingers.push(a.finger.to_string());
            }
            if a.pick > 0 {
                fingers.push(["p", "i", "m", "a", "c"][(a.pick as usize - 1).min(4)].to_string());
            }
        }
        if bits != 0 || !fingers.is_empty() {
            let cx = base_x + w / 2.0;
            let above = !it.stem_up || it.stem_hidden || it.value.base == 1;
            let mut y = if above { y_of(staff, hi.max(7), sp) - 1.4 * sp } else { y_of(staff, lo.min(1), sp) + 1.4 * sp };
            for a in 0..ARTICULATIONS.len() {
                if bits & (1 << a) == 0 || a <= 1 {
                    continue;
                }
                artic_glyph(p, a, pos2(cx, y + if above { 0.0 } else { 0.4 * sp }), sp, above, stem_col);
                y += if above { -1.3 * sp } else { 1.3 * sp };
            }
            for f in &fingers {
                p.text(pos2(cx, y), Align2::CENTER_CENTER, f, look::semibold(1.4 * sp), stem_col);
                y += if above { -1.5 * sp } else { 1.5 * sp };
            }
            if bits & 0b11 != 0 {
                let up = bits & 1 != 0;
                let fy = if up { y_of(staff, hi.max(9), sp) - 1.6 * sp } else { y_of(staff, lo.min(-1), sp) + 1.6 * sp };
                artic_glyph(p, if up { 0 } else { 1 }, pos2(cx, fy), sp, up, stem_col);
            }
        }
        tied_in = next_tied;
        // the stem
        if it.value.base > 1 && !it.stem_hidden {
            let up = it.stem_up;
            let attr = it.heads.first().map(|h| (o.attr)(h.note)).unwrap_or_default();
            let sx = if attr.stem_pos == 1 { base_x + w / 2.0 } else if up { base_x + w - stem_w / 2.0 } else { base_x + stem_w / 2.0 };
            let (from_pos, to_pos) = if up { (lo, hi) } else { (hi, lo) };
            let y0 = y_of(staff, from_pos, sp);
            let mut y1 = y_of(staff, to_pos, sp) + if up { -3.5 * sp } else { 3.5 * sp };
            // far from the staff: the stem reaches the middle line
            let mid = y_of(staff, 4, sp);
            if up && y1 > mid {
                y1 = mid;
            }
            if !up && y1 < mid {
                y1 = mid;
            }
            // Stem End: Move Up / Down (half spaces)
            y1 -= attr.stem_end as f32 * sp / 2.0;
            stems[k] = Some((sx, y0, y1));
            if it.beam.is_none() {
                p.line_segment([pos2(sx, y0), pos2(sx, y1)], Stroke::new(stem_w, stem_col));
                for f in 0..it.value.flags() {
                    flag(p, pos2(sx, y1 + if up { f as f32 * 0.8 * sp } else { -(f as f32) * 0.8 * sp }), sp, up, stem_col);
                }
            }
        }
    }
    // beams: a straight beam (slope kept gentle), stems lengthened to meet it
    for b in &v.beams {
        let pts: Vec<(usize, f32, f32, f32)> = b.items.iter().filter_map(|&i| stems[i].map(|(x, y0, y1)| (i, x, y0, y1))).collect();
        if pts.len() < 2 {
            for &(i, x, y0, y1) in &pts {
                // a beam cut by the line's end: the stem and its flags
                p.line_segment([pos2(x, y0), pos2(x, y1)], Stroke::new(stem_w, INK));
                for f in 0..v.items[i].value.flags() {
                    flag(p, pos2(x, y1 + if b.up { f as f32 * 0.8 * sp } else { -(f as f32) * 0.8 * sp }), sp, b.up, INK);
                }
            }
            continue;
        }
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        let dx = (l.1 - f.1).max(1.0);
        let slope = ((l.3 - f.3) / dx).clamp(-0.18, 0.18);
        // move the line so every stem is at least 2.5 spaces
        let mut off = 0.0f32;
        for &(_, x, y0, _) in &pts {
            let line_y = f.3 + slope * (x - f.1);
            let need = if b.up { y0 - 2.6 * sp } else { y0 + 2.6 * sp };
            if b.up && line_y + off > need {
                off = need - line_y;
            }
            if !b.up && line_y + off < need {
                off = need - line_y;
            }
        }
        let line = |x: f32| f.3 + off + slope * (x - f.1);
        let thick = 0.5 * sp;
        let dir = if b.up { 1.0 } else { -1.0 };
        let col = v.items[b.items[0]].heads.first().map_or(INK, |h| (o.color)(h.note));
        for &(_, x, y0, _) in &pts {
            p.line_segment([pos2(x, y0), pos2(x, line(x))], Stroke::new(stem_w, col));
        }
        let max_level = pts.iter().map(|&(i, ..)| v.items[i].value.flags()).max().unwrap_or(1);
        for level in 0..max_level {
            let dy = dir * level as f32 * 0.75 * sp;
            let has = |i: usize| v.items[i].value.flags() > level;
            for (k, &(i, x, ..)) in pts.iter().enumerate() {
                if !has(i) {
                    continue;
                }
                let next = pts.get(k + 1).filter(|n| has(n.0));
                let prev = k.checked_sub(1).map(|j| pts[j]).filter(|n| has(n.0));
                let (xa, xb) = if let Some(n) = next {
                    (x, n.1)
                } else if prev.is_some() {
                    continue;
                } else if k + 1 < pts.len() {
                    (x, x + 1.1 * sp)
                } else {
                    (x - 1.1 * sp, x)
                };
                let (ya, yb) = (line(xa) + dy, line(xb) + dy);
                p.add(Shape::convex_polygon(vec![pos2(xa - stem_w / 2.0, ya), pos2(xb + stem_w / 2.0, yb), pos2(xb + stem_w / 2.0, yb + dir * thick), pos2(xa - stem_w / 2.0, ya + dir * thick)], col, Stroke::NONE));
            }
        }
    }
    // triplet numbers
    for t in &v.tuplets {
        let (a, b) = (&v.items[t.first], &v.items[t.last]);
        if !(o.in_line)(a.tick) {
            continue;
        }
        let xa = x_of(a.tick as f32) + 0.8 * sp;
        let xb = x_of(b.tick as f32) + 0.8 * sp + 1.25 * sp;
        let ys: Vec<f32> = (t.first..=t.last).filter_map(|i| stems[i].map(|s| s.2)).collect();
        let y = if t.up {
            ys.iter().copied().fold(staff.bottom - 4.0 * sp, f32::min) - 1.0 * sp
        } else {
            ys.iter().copied().fold(staff.bottom, f32::max) + 1.1 * sp
        };
        if a.beam.is_none() || a.beam != b.beam {
            let d = if t.up { 0.5 * sp } else { -0.5 * sp };
            p.line_segment([pos2(xa, y + d), pos2(xa, y)], Stroke::new(1.0, INK));
            p.line_segment([pos2(xa, y), pos2(xb, y)], Stroke::new(1.0, INK));
            p.line_segment([pos2(xb, y), pos2(xb, y + d)], Stroke::new(1.0, INK));
        }
        let c = pos2((xa + xb) / 2.0, y);
        p.rect_filled(Rect::from_center_size(c, vec2(sp * 1.1, sp * 1.3)), 0.0, REGION_PAPER);
        p.text(c, Align2::CENTER_CENTER, "3", look::bold(sp * 1.5), INK);
    }
}

/// A filled band between two curves of as many points (a tie's crescent, a flag): one mesh,
/// its edges stroked thinly for smoothness.
fn band(p: &egui::Painter, a: &[Pos2], b: &[Pos2], col: Color32) {
    let n = a.len().min(b.len());
    if n < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for k in 0..n {
        mesh.colored_vertex(a[k], col);
        mesh.colored_vertex(b[k], col);
    }
    for k in 0..n as u32 - 1 {
        let i = 2 * k;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::Path(PathShape::line(a[..n].to_vec(), PathStroke::new(0.7, col))));
    p.add(Shape::Path(PathShape::line(b[..n].to_vec(), PathStroke::new(0.7, col))));
}

pub(crate) fn bezier(pts: [Pos2; 4], n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            pos2(f(pts[0].x, pts[1].x, pts[2].x, pts[3].x), f(pts[0].y, pts[1].y, pts[2].y, pts[3].y))
        })
        .collect()
}

pub(crate) fn tie(p: &egui::Painter, a: Pos2, b: Pos2, lift: f32, sp: f32, col: Color32) {
    let c1 = pos2(a.x + (b.x - a.x) * 0.25, a.y + lift);
    let c2 = pos2(a.x + (b.x - a.x) * 0.75, b.y + lift);
    let outer = bezier([a, c1, c2, b], 24);
    let thick = lift.signum() * 0.22 * sp;
    let inner = bezier([a, pos2(c1.x, c1.y - thick), pos2(c2.x, c2.y - thick), b], 24);
    band(p, &outer, &inner, col);
}

pub(crate) fn flag(p: &egui::Painter, at: Pos2, sp: f32, up: bool, col: Color32) {
    // SMuFL flag8thUp / flag8thDown: their origin is the stem's end (the stem is 0.12 sp wide)
    let ch = if up { '\u{E240}' } else { '\u{E241}' };
    smufl(p, ch, pos2(at.x - 0.06 * sp, at.y), sp, col, false);
}


/// Rests at x (centre) on a staff whose bottom line is at `bottom`.
pub(crate) fn rest(p: &egui::Painter, x: f32, bottom: f32, sp: f32, v: nt::Value, full_bar: bool, col: Color32) {
    let _ = full_bar;
    let y = |pos: f32| bottom - pos * sp / 2.0;
    // SMuFL rests: the whole rest hangs from the fourth line, the others sit on the middle one
    let (ch, line) = match v.base {
        1 => ('\u{E4E3}', 6.0),
        2 => ('\u{E4E4}', 4.0),
        4 => ('\u{E4E5}', 4.0),
        8 => ('\u{E4E6}', 4.0),
        16 => ('\u{E4E7}', 4.0),
        32 => ('\u{E4E8}', 4.0),
        _ => ('\u{E4E9}', 4.0),
    };
    smufl(p, ch, pos2(x, y(line)), sp, col, true);
}

/// ♯ ♭ ♮ (and double signs as two) centred at `c`.
pub(crate) fn accidental(p: &egui::Painter, c: Pos2, sp: f32, alter: i8, col: Color32) {
    // SMuFL accidentalFlat / Natural / Sharp / DoubleSharp / DoubleFlat
    let ch = match alter {
        -2 => '\u{E264}',
        -1 => '\u{E260}',
        1 => '\u{E262}',
        2 => '\u{E263}',
        _ => '\u{E261}',
    };
    smufl(p, ch, c, sp, col, true);
}

/// A SMuFL glyph with its origin (the baseline's left end, or its centre with `centre`) at `at`.
pub(crate) fn smufl(p: &egui::Painter, ch: char, at: Pos2, sp: f32, col: Color32, centre: bool) {
    let g = p.layout_no_wrap(ch.to_string(), look::music(4.0 * sp), col);
    let base = g.rows.first().and_then(|r| r.glyphs.first()).map_or(g.size().y * 0.8, |gl| gl.pos.y);
    let w = g.size().x;
    let x = if centre { at.x - w / 2.0 } else { at.x };
    p.galley(pos2(x, at.y - base), g, col);
}

/// A smooth curve through points (Catmull-Rom), sampled.
fn spline(pts: &[(f32, f32)], per: usize) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for k in 0..per {
            let t = k as f32 / per as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}


fn brace(p: &egui::Painter, x: f32, y0: f32, y1: f32, col: Color32) {
    let h = y1 - y0;
    let m = (y0 + y1) / 2.0;
    let pts = [(0.0, y0), (-4.0, y0 + h * 0.15), (0.0, m - h * 0.12), (-5.0, m), (0.0, m + h * 0.12), (-4.0, y1 - h * 0.15), (0.0, y1)];
    let s = spline(&pts, 8);
    let pts: Vec<Pos2> = s.iter().map(|&(dx, y)| pos2(x + dx, y)).collect();
    p.add(Shape::Path(PathShape::line(pts, PathStroke::new(2.0, col))));
}

/// A Part box note icon: head, stem, flags.
pub(crate) fn note_icon(p: &egui::Painter, r: Rect, v: nt::Value, col: Color32) {
    // (Logic's Part box notes are tall: the size follows the cell's height)
    let sp = (r.height() / 6.6).clamp(3.5, 6.2);
    let c = pos2(r.center().x - 0.2 * sp, r.center().y + 1.5 * sp);
    notehead(p, c, sp, v.base, col, Color32::TRANSPARENT);
    if v.base == 1 || v.base == 2 {
        // the icons on a dark strip: open heads drawn as rings
        p.add(Shape::closed_line(ellipse(c, if v.base == 1 { 0.8 * sp } else { 0.64 * sp }, 0.45 * sp, if v.base == 1 { 0.0 } else { -0.35 }, 24), Stroke::new(1.3, col)));
    }
    if v.base > 1 {
        let x = c.x + 0.62 * sp;
        let top = c.y - 3.4 * sp;
        p.line_segment([pos2(x, c.y), pos2(x, top)], Stroke::new(1.0, col));
        for f in 0..v.flags() {
            flag(p, pos2(x, top + f as f32 * 0.7 * sp), sp * 0.8, true, col);
        }
    }
    for k in 0..v.dots {
        p.circle_filled(pos2(c.x + (1.1 + 0.6 * k as f32) * sp, c.y - 0.1 * sp), 0.22 * sp, col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_passes_through_points() {
        let s = spline(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], 4);
        assert_eq!(s.first(), Some(&(0.0, 0.0)));
        assert_eq!(s.last(), Some(&(2.0, 0.0)));
        assert!(s.iter().any(|&(x, y)| (x - 1.0).abs() < 1e-4 && (y - 1.0).abs() < 1e-4));
    }

    #[test]
    fn dynamics_rise() {
        assert!(State::default().dyn_velocity.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn chord_symbol_lines() {
        let f = |t: &str| super::chord_parts(t);
        assert_eq!(f("Cmaj7/E"), ("Cmaj".into(), "7".into(), "/E".into()));
        assert_eq!(f("F#m7b5"), ("F#m".into(), "7b5".into(), String::new()));
        assert_eq!(f("Cm,add9"), ("Cm".into(), "add9".into(), String::new()));
        assert_eq!(f("Bb"), ("Bb".into(), String::new(), String::new()));
    }
}
