//! The Event List (List Editors ▸ Events, ⌘7 / D), as Logic's: the local bar (Display Level,
//! Edit / Functions / View, Catch, MIDI In / Out), the event type buttons (Notes, Progr. Change,
//! Pitch Bend, Controller, Aftertouch, Poly Aftertouch, Syst. Exclusive, Additional Info), the
//! Create row (+, event type, Quantize, Q), columns L M Position Status Ch Num Val Length/Info
//! (and Articulation), one list of the region's notes and other events by position, the
//! status bar (track, event count).
//!
//! Selection: click, Shift / Command-click, drag across rows, ⇧← ⇧→ extend, ⌘A (visible
//! events), Logic's Select key commands on the list (⇧M muted, the Piano Roll's ones through
//! midifunc), the right-click menu. Edits: values dragged or typed, on a multiple selection
//! together (relative, stopped at a limit; Option goes on past it; Option-Shift sets all to
//! one value), positions dragged by unit or typed (spaces, dots or commas), Mute and Lock
//! columns, Lock / Unlock SMPTE Position, Copy / Cut / Paste (Paste asks for the position;
//! Return keeps the original), Delete; controllers' names pop-up in Length/Info; filtered
//! events are protected. Display Level lists the regions (click a selected region's name to
//! rename it). During playback a line marks the event at the playhead and the list follows it
//! (Catch). The Event Float window (⌥E) shows the selected event as one editable line; Show
//! Event Position and Length as Time (⌃⌥R) switches both to minutes : seconds.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::{Control, Note};

use crate::app::App;
use crate::glyphs;
use crate::look;
use crate::widgets;

pub const COMMANDS: &[&str] = &[
    "Show/Hide Event Float",
    "Show Event Position and Length as Time or Bars/Beats",
    "Select Muted Regions/Cells/Events",
    "Toggle Previous Region/Cell/Event or Move Marquee Start (or Extend Marquee Selection) to Previous Transient",
    "Toggle Next Region/Cell/Event or Move Marquee Start (or Extend Marquee Selection) to Next Transient",
];

const ROW: f32 = 24.0;
/// The event type buttons, in Logic's order (status nibble; 0x90 notes, 0xF0 SysEx).
pub const TYPES: [(&str, u8); 7] = [("Notes", 0x90), ("Progr. Change", 0xC0), ("Pitch Bend", 0xE0), ("Controller", 0xB0), ("Aftertouch", 0xD0), ("Poly Aftertouch", 0xA0), ("Syst. Exclusive", 0xF0)];
const CREATE: [(&str, u8); 6] = [("Notes", 0x90), ("Progr. Change", 0xC0), ("Pitch Bend", 0xE0), ("Controller", 0xB0), ("Aftertouch", 0xD0), ("Poly Aftertouch", 0xA0)];

/// A row: a note (index into the region's notes) or another event (index into ext.events).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    N(usize),
    E(usize),
}

#[derive(Clone, Debug)]
enum Clip {
    N(Note),
    /// an event at its absolute position
    E(Control, u32),
}

#[derive(Clone, Debug)]
pub struct State {
    /// event types shown (TYPES order)
    pub shown: [bool; 7],
    pub info: bool,
    pub as_time: bool,
    pub regions: bool,
    pub float: bool,
    pub catch: bool,
    pub midi_in: bool,
    pub midi_out: bool,
    pub artic_col: bool,
    pub create: usize,
    pub quantize: usize,
    pub sel_ev: Vec<usize>,
    anchor: Option<usize>,
    drag_from: Option<usize>,
    clip: Vec<Clip>,
    /// the Paste position box: (text, original first position)
    paste: Option<(String, u32)>,
    rename: Option<((usize, usize), String)>,
    scroll_to: Option<usize>,
    region: Option<(usize, usize)>,
    /// The columns' widths (times Logic's default; empty: the defaults) and their order (the
    /// column indices left to right; empty: Logic's), as dragged in the column header.
    pub col_scale: Vec<f32>,
    pub col_order: Vec<usize>,
}

impl Default for State {
    fn default() -> Self {
        State { shown: [true; 7], info: false, as_time: false, regions: false, float: false, catch: true, midi_in: false, midi_out: true, artic_col: false, create: 0, quantize: 0, sel_ev: Vec::new(), anchor: None, drag_from: None, clip: Vec::new(), paste: None, rename: None, scroll_to: None, region: None, col_scale: Vec::new(), col_order: Vec::new() }
    }
}

/// The columns left to right: the saved order when it is a whole one, else Logic's.
pub fn column_order(saved: &[usize], n: usize) -> Vec<usize> {
    let mut s = saved.to_vec();
    s.sort_unstable();
    if s == (0..n).collect::<Vec<_>>() {
        saved.to_vec()
    } else {
        (0..n).collect()
    }
}

/// Each column's left edge (from the list's left), given the widths and the order.
pub fn column_offsets(widths: &[f32], order: &[usize]) -> Vec<f32> {
    let mut out = vec![0.0; widths.len()];
    let mut x = 0.0;
    for &i in order {
        out[i] = x;
        x += widths[i];
    }
    out
}

/// Column `col` dropped at `at` (points from the list's left): the new order (L and M stay first).
pub fn move_column(order: &[usize], col: usize, widths: &[f32], at: f32) -> Vec<usize> {
    let mut rest: Vec<usize> = order.iter().copied().filter(|&c| c != col).collect();
    let offs = column_offsets(widths, order);
    let pos = rest.iter().position(|&c| c >= 2 && offs[c] + widths[c] / 2.0 > at).unwrap_or(rest.len()).max(2);
    rest.insert(pos.min(rest.len()), col);
    rest
}

fn id() -> egui::Id {
    egui::Id::unique("lg-evlist")
}

pub fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(id())).unwrap_or_default()
}

pub fn with<R>(ctx: &egui::Context, f: impl FnOnce(&mut State) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<State>(id())))
}

fn type_index(kind: u8) -> usize {
    TYPES.iter().position(|t| t.1 == kind & 0xF0).unwrap_or(6)
}

/// The rows shown: notes and events by absolute position (notes first at one position), the
/// hidden types left out.
pub fn rows(rg: &lg_core::Region, shown: &[bool; 7]) -> Vec<(u32, Row)> {
    let mut v: Vec<(u32, Row)> = Vec::new();
    if shown[0] {
        v.extend(rg.notes.iter().enumerate().map(|(i, n)| (n.pos, Row::N(i))));
    }
    v.extend(rg.ext.events.iter().enumerate().filter(|(_, e)| shown[type_index(e.kind)]).map(|(i, e)| (rg.start + e.pos, Row::E(i))));
    v.sort_by_key(|(p, r)| (*p, matches!(r, Row::E(_))));
    v
}

/// The standard MIDI controller names (Length/Info of a control change).
pub fn cc_name(n: u8) -> &'static str {
    match n {
        0 => "Bank MSB",
        1 => "Modulation",
        2 => "Breath",
        4 => "Foot Control",
        5 => "Portamento Time",
        6 => "Data MSB",
        7 => "Volume",
        8 => "Balance",
        10 => "Pan",
        11 => "Expression",
        12 => "Effect 1",
        13 => "Effect 2",
        16..=19 => "General Purpose",
        32 => "Bank LSB",
        38 => "Data LSB",
        64 => "Sustain",
        65 => "Portamento",
        66 => "Sostenuto",
        67 => "Soft Pedal",
        68 => "Legato",
        69 => "Hold 2",
        70 => "Sound Variation",
        71 => "Resonance",
        72 => "Release Time",
        73 => "Attack Time",
        74 => "Cutoff",
        75..=79 => "Sound Control",
        84 => "Portamento Control",
        91 => "Reverb",
        92 => "Tremolo",
        93 => "Chorus",
        94 => "Detune",
        95 => "Phaser",
        96 => "Data Increment",
        97 => "Data Decrement",
        98 => "NRPN LSB",
        99 => "NRPN MSB",
        100 => "RPN LSB",
        101 => "RPN MSB",
        120 => "All Sound Off",
        121 => "Reset Controllers",
        122 => "Local Control",
        123 => "All Notes Off",
        124 => "Omni Off",
        125 => "Omni On",
        126 => "Mono Mode",
        127 => "Poly Mode",
        _ => "",
    }
}

/// Switch controllers (64-90) read 0 as off, anything else as on.
pub fn is_switch(n: u8) -> bool {
    (64..=90).contains(&n)
}

/// A value change on several selected events: `k`'s value becomes `new`; the others keep their
/// distance. The change stops where the first one reaches `lo` / `hi`, unless `past` (Option);
/// `same` (Option-Shift) sets every one to `new`.
pub fn group_values(vals: &[i32], k: usize, new: i32, lo: i32, hi: i32, past: bool, same: bool) -> Vec<i32> {
    let new = new.clamp(lo, hi);
    if same {
        return vec![new; vals.len()];
    }
    let mut d = new - vals.get(k).copied().unwrap_or(new);
    if !past {
        let up = vals.iter().map(|v| hi - v).min().unwrap_or(0).max(0);
        let down = vals.iter().map(|v| v - lo).min().unwrap_or(0).max(0);
        d = d.clamp(-down, up);
    }
    vals.iter().map(|v| (v + d).clamp(lo, hi)).collect()
}

/// The lock key of a row: (ticks from the region start, status nibble, number / pitch).
fn lock_key(rg: &lg_core::Region, row: Row) -> Option<(u32, u8, u8)> {
    match row {
        Row::N(i) => rg.notes.get(i).map(|n| (n.pos.saturating_sub(rg.start), 0x90, n.pitch)),
        Row::E(i) => rg.ext.events.get(i).map(|e| (e.pos, e.kind & 0xF0, e.number)),
    }
}

pub fn is_locked(rg: &lg_core::Region, row: Row) -> bool {
    rg.props.locked || lock_key(rg, row).is_some_and(|k| rg.ext.locked.contains(&k))
}

fn set_locked(rg: &mut lg_core::Region, row: Row, on: bool) {
    let Some(k) = lock_key(rg, row) else { return };
    rg.ext.locked.retain(|x| *x != k);
    if on {
        rg.ext.locked.push(k);
    }
}

fn selected_rows(app: &App, st: &State, rows: &[(u32, Row)]) -> Vec<Row> {
    rows.iter().map(|r| r.1).filter(|r| match r {
        Row::N(i) => app.selected_notes.contains(i),
        Row::E(i) => st.sel_ev.contains(i),
    }).collect()
}

fn select_rows(app: &mut App, st: &mut State, sel: &[Row]) {
    app.selected_notes = sel.iter().filter_map(|r| if let Row::N(i) = r { Some(*i) } else { None }).collect();
    st.sel_ev = sel.iter().filter_map(|r| if let Row::E(i) = r { Some(*i) } else { None }).collect();
}

fn region(app: &App) -> Option<(usize, usize, lg_core::Region)> {
    let (t, r) = app.selected_region?;
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    (rg.audio.is_none()).then_some((t, r, rg))
}

/// Positions as Logic shows them: bar beat division tick, or (as time) h:mm:ss.ms.
fn pos_text(app: &App, st: &State, tick: u32) -> String {
    if st.as_time {
        if let Some(p) = &app.project {
            let s = lg_core::timeline::Map::of(p).secs_at(tick as f64);
            return time_text(s);
        }
    }
    crate::rightpane::bbdt(app, tick)
}

pub fn time_text(s: f64) -> String {
    let ms = (s * 1000.0).round().max(0.0) as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn len_text(app: &App, st: &State, from: u32, len: u32) -> String {
    if st.as_time {
        if let Some(p) = &app.project {
            let m = lg_core::timeline::Map::of(p);
            return time_text(m.secs_at((from + len) as f64) - m.secs_at(from as f64));
        }
    }
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    crate::app::bbdt_len(len, sig)
}

fn status_text(kind: u8) -> &'static str {
    match kind & 0xF0 {
        0xA0 => "Poly Pressure",
        0xB0 => "Control",
        0xC0 => "Program",
        0xD0 => "Aftertouch",
        0xE0 => "Pitch Bend",
        _ => "SysEx",
    }
}

fn bytes_text(row: Row, rg: &lg_core::Region) -> String {
    match row {
        Row::N(i) => rg.notes.get(i).map_or(String::new(), |n| format!("${:02X} ${:02X} ${:02X}", 0x90 | n.channel, n.pitch, n.velocity)),
        Row::E(i) => rg.ext.events.get(i).map_or(String::new(), |e| match e.kind & 0xF0 {
            0xC0 | 0xD0 => format!("${:02X} ${:02X}", e.kind & 0xF0 | e.channel, e.number),
            k => format!("${:02X} ${:02X} ${:02X}", k | e.channel, e.number, e.value),
        }),
    }
}

/// One edit of a row's field, collected while drawing.
#[derive(Clone, Copy, Debug)]
enum Field {
    Ch,
    Num,
    Val,
    Len,
    Pos,
}

fn field_of(rg: &lg_core::Region, row: Row, f: Field) -> Option<(i32, i32, i32)> {
    match row {
        Row::N(i) => {
            let n = rg.notes.get(i)?;
            Some(match f {
                Field::Ch => (n.channel as i32, 0, 15),
                Field::Num => (n.pitch as i32, 0, 127),
                Field::Val => (n.velocity as i32, 1, 127),
                Field::Len => (n.len as i32, 1, i32::MAX / 2),
                Field::Pos => (n.pos as i32, rg.start as i32, (rg.start + rg.length.max(1) - 1) as i32),
            })
        }
        Row::E(i) => {
            let e = rg.ext.events.get(i)?;
            Some(match f {
                Field::Ch => (e.channel as i32, 0, 15),
                Field::Num if e.kind & 0xF0 == 0xE0 => (lg_core::regions::bend_value(e) as i32, -8192, 8191),
                Field::Num => (e.number as i32, 0, 127),
                Field::Val if e.kind & 0xF0 == 0xE0 => (lg_core::regions::bend_value(e) as i32, -8192, 8191),
                Field::Val => (e.value as i32, 0, 127),
                Field::Len => return None,
                Field::Pos => ((rg.start + e.pos) as i32, rg.start as i32, (rg.start + rg.length.max(1) - 1) as i32),
            })
        }
    }
}

fn set_field(rg: &mut lg_core::Region, row: Row, f: Field, v: i32) {
    let start = rg.start;
    match row {
        Row::N(i) => {
            let Some(n) = rg.notes.get_mut(i) else { return };
            match f {
                Field::Ch => n.channel = v as u8,
                Field::Num => n.pitch = v as u8,
                Field::Val => n.velocity = v as u8,
                Field::Len => n.len = v.max(1) as u32,
                Field::Pos => n.pos = v.max(0) as u32,
            }
        }
        Row::E(i) => {
            let Some(e) = rg.ext.events.get_mut(i) else { return };
            match f {
                Field::Ch => e.channel = v as u8,
                Field::Num | Field::Val if e.kind & 0xF0 == 0xE0 => *e = lg_core::regions::bend_event(e.pos, e.channel, v as i16),
                Field::Num => e.number = v as u8,
                Field::Val => e.value = v as u8,
                Field::Len => {}
                Field::Pos => e.pos = (v.max(0) as u32).saturating_sub(start),
            }
        }
    }
}

/// Applies one edited value to the edited row and, when it is part of the selection, to the
/// whole selection of that kind (Logic's group change). Locked events keep their position.
fn apply_edit(app: &mut App, st: &mut State, row: Row, f: Field, new: i32, mods: egui::Modifiers) {
    let Some((t, r, rg)) = region(app) else { return };
    let all = rows(&rg, &st.shown);
    let mut targets = selected_rows(app, st, &all);
    if !targets.contains(&row) {
        targets = vec![row];
    }
    // the same kind of value only (a note's pitch and a controller number don't mix)
    let kind = |x: &Row| match x {
        Row::N(_) => 0x90,
        Row::E(i) => rg.ext.events[*i].kind & 0xF0,
    };
    let k0 = kind(&row);
    targets.retain(|x| kind(x) == k0 || matches!(f, Field::Pos | Field::Ch));
    if matches!(f, Field::Pos) {
        targets.retain(|x| !is_locked(&rg, *x));
        if targets.is_empty() {
            app.tell("The event's position is locked (Unlock SMPTE Position)");
            return;
        }
    }
    let vals: Vec<(i32, i32, i32)> = targets.iter().filter_map(|x| field_of(&rg, *x, f)).collect();
    if vals.len() != targets.len() {
        return;
    }
    let k = targets.iter().position(|x| *x == row).unwrap_or(0);
    let (lo, hi) = (vals.iter().map(|v| v.1).max().unwrap_or(0), vals.iter().map(|v| v.2).min().unwrap_or(127));
    let out = group_values(&vals.iter().map(|v| v.0).collect::<Vec<_>>(), k, new, lo, hi, mods.alt && !mods.shift, mods.alt && mods.shift);
    app.checkpoint("Change Event");
    if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        for (x, v) in targets.iter().zip(out) {
            set_field(rgm, *x, f, v);
        }
        if matches!(f, Field::Pos) {
            resort(rgm, app.selected_notes.clone(), st.sel_ev.clone()).apply(app, st);
        }
    }
    app.after_edit();
    if st.midi_out {
        if let (Row::N(i), Some(rg)) = (row, app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r))) {
            if let Some(n) = rg.notes.get(i) {
                let (p, v) = (n.pitch, n.velocity);
                app.preview_note(t, p, v);
            }
        }
    }
}

struct Resorted(Vec<usize>, Vec<usize>);

impl Resorted {
    fn apply(self, app: &mut App, st: &mut State) {
        app.selected_notes = self.0;
        st.sel_ev = self.1;
    }
}

/// Sorts a region's notes and events by position, keeping the selection on the same events.
fn resort(rg: &mut lg_core::Region, sel_n: Vec<usize>, sel_e: Vec<usize>) -> Resorted {
    let mut n: Vec<(Note, bool)> = rg.notes.iter().enumerate().map(|(i, n)| (*n, sel_n.contains(&i))).collect();
    n.sort_by_key(|(x, _)| (x.pos, x.pitch));
    let mut e: Vec<(Control, bool)> = rg.ext.events.iter().enumerate().map(|(i, e)| (*e, sel_e.contains(&i))).collect();
    e.sort_by_key(|(x, _)| x.pos);
    rg.notes = n.iter().map(|x| x.0).collect();
    rg.ext.events = e.iter().map(|x| x.0).collect();
    Resorted(n.iter().enumerate().filter(|x| x.1 .1).map(|x| x.0).collect(), e.iter().enumerate().filter(|x| x.1 .1).map(|x| x.0).collect())
}

// ---------------------------------------------------------------- drawing

fn pill(ui: &mut egui::Ui, r: Rect, idv: egui::Id, label: &str, on: bool, on_col: Color32) -> egui::Response {
    let resp = ui.interact(r, idv, Sense::click());
    let p = ui.painter();
    let bg = if on { on_col } else if resp.hovered() { look::button_on() } else { look::group_bg() };
    p.rect_filled(r, 5.0, bg);
    fit_text(p, r.shrink2(vec2(4.0, 0.0)), label, 12.0, if on { Color32::WHITE } else { look::text() });
    resp
}

/// Text centred in `r`, its size lowered (to 9 pt) until it fits, then cut with an ellipsis.
fn fit_text(p: &egui::Painter, r: Rect, text: &str, size: f32, col: Color32) {
    let mut sz = size;
    loop {
        let g = p.layout_no_wrap(text.to_string(), look::regular(sz), col);
        if g.size().x <= r.width() || sz <= 9.0 {
            if g.size().x <= r.width() {
                p.galley(pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0), g, col);
            } else {
                let mut t: String = text.to_string();
                while !t.is_empty() && p.layout_no_wrap(format!("{t}…"), look::regular(sz), col).size().x > r.width() {
                    t.pop();
                }
                p.text(r.center(), Align2::CENTER_CENTER, format!("{t}…"), look::regular(sz), col);
            }
            return;
        }
        sz -= 0.5;
    }
}

fn icon_button(ui: &mut egui::Ui, r: Rect, idv: egui::Id, icon: &str, on: bool, on_col: Color32, enabled: bool) -> egui::Response {
    let resp = ui.interact(r, idv, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(r, 6.0, if on { on_col } else if resp.hovered() && enabled { look::button_on() } else { look::group_bg() });
    let c = if !enabled { look::text_dim().gamma_multiply(0.5) } else if on { Color32::WHITE } else { look::text() };
    glyphs::paint(p, r, icon, 14.0, c);
    resp
}

const BLUE: Color32 = Color32::from_rgb(0x0b, 0x63, 0xb8);
const GREEN: Color32 = Color32::from_rgb(0x22, 0x8b, 0x3a);

pub fn show(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    // (View ▸ Link: Off keeps the region, Same Level lists the regions; rolllink.rs)
    let real = crate::rolllink::enter(app, true);
    if crate::rolllink::links().events == 1 {
        let has = app.selected_region.is_some();
        with(ui.ctx(), |st| st.regions |= has);
    }
    show_linked(app, ui, body);
    crate::rolllink::leave(app, true, real);
}

fn show_linked(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    let ctx = ui.ctx().clone();
    let mut st = state(&ctx);
    crate::eventedit::with(&ctx, |s| s.rect = Some(body));
    if st.region != app.selected_region {
        st.region = app.selected_region;
        st.sel_ev.clear();
        st.anchor = None;
    }
    let p = ui.painter().clone();
    // ---- the local bar: Display Level, Edit / Functions / View, Catch, MIDI In / Out
    let bar = Rect::from_min_size(body.min + vec2(10.0, 0.0), vec2(body.width() - 20.0, 24.0));
    let has_region = region(app).is_some();
    let lvl = Rect::from_min_size(bar.min, vec2(34.0, 24.0));
    if icon_button(ui, lvl, egui::Id::unique("evl-level"), glyphs::ph::ARROW_ELBOW_LEFT_UP, false, BLUE, has_region && !st.regions).on_hover_text("Display Level: the regions and folders").clicked() {
        st.regions = true;
    }
    // (the menus take what is left between Display Level and Catch / MIDI In / Out)
    // (a narrow pane: Catch and MIDI In / Out go to a row of their own under the menus)
    let narrow = bar.width() < 34.0 + 8.0 + 196.0 + 8.0 + 3.0 * 38.0;
    let menus = Rect::from_min_max(pos2(lvl.max.x + 8.0, bar.min.y), pos2(if narrow { bar.max.x } else { (lvl.max.x + 230.0).min(bar.max.x - 3.0 * 38.0 - 12.0) }, bar.max.y));
    let row2 = if narrow { 30.0 } else { 0.0 };
    p.rect_filled(menus, 6.0, look::group_bg());
    let mut run: Option<&'static str> = None;
    {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(menus.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        // (Logic's capsule is 228 pt at a 452 pt window: tight buttons so View fits before Catch)
        c.spacing_mut().item_spacing.x = 4.0;
        c.spacing_mut().button_padding.x = 3.0;
        c.menu_button("Edit ⌄", |ui| edit_menu(ui, &mut run));
        c.menu_button("Functions ⌄", |ui| {
            for n in ["Lock SMPTE Position", "Unlock SMPTE Position"] {
                if ui.button(n).clicked() {
                    run = Some(n);
                    ui.close();
                }
            }
            ui.separator();
            for n in ["Copy MIDI Events…", "Quantize Selected Events", "Note Events to Positions", "MIDI Transform"] {
                if n == "MIDI Transform" {
                    ui.menu_button(n, |ui| {
                        for x in ["Humanize", "Fixed Velocity", "Crescendo", "Double Speed", "Half Speed", "Reverse Position", "Transpose", "Quantize Note Length"] {
                            if ui.button(x).clicked() {
                                run = Some(match x {
                                    "Humanize" => "Humanize",
                                    "Fixed Velocity" => "Fixed Velocity",
                                    "Crescendo" => "Crescendo",
                                    "Double Speed" => "Double Speed",
                                    "Half Speed" => "Half Speed",
                                    "Reverse Position" => "Reverse Position",
                                    "Transpose" => "Transposition",
                                    _ => "Quantize Note Length",
                                });
                                ui.close();
                            }
                        }
                    });
                } else if n != "Note Events to Positions" && ui.button(n).clicked() {
                    run = Some(if n == "Copy MIDI Events…" { "Copy MIDI Events…" } else { "Quantize Selected Events" });
                    ui.close();
                }
            }
        });
        c.menu_button("View ⌄", |ui| {
            let mut rel = crate::eventedit::state(ui.ctx()).relative;
            let mut abs = crate::eventedit::state(ui.ctx()).abs_length;
            if ui.checkbox(&mut rel, "Relative Position").changed() {
                crate::eventedit::with(ui.ctx(), |s| s.relative = rel);
            }
            if ui.checkbox(&mut abs, "Length as Absolute Position").changed() {
                crate::eventedit::with(ui.ctx(), |s| s.abs_length = abs);
            }
            ui.checkbox(&mut st.as_time, "Event Position and Length as Time");
            ui.add_enabled_ui(has_region && !st.regions, |ui| ui.checkbox(&mut st.artic_col, "Articulation"));
            ui.separator();
            ui.checkbox(&mut st.float, "Event Float");
        });
    }
    let catch = if narrow { Rect::from_min_size(pos2(bar.min.x, bar.min.y + row2), vec2(34.0, 24.0)) } else { Rect::from_min_size(pos2(menus.max.x + 8.0, bar.min.y), vec2(34.0, 24.0)) };
    if icon_button(ui, catch, egui::Id::unique("evl-catch"), glyphs::ph::ARROWS_IN_LINE_HORIZONTAL, st.catch, BLUE, true).on_hover_text("Catch: the list follows the playhead").clicked() {
        st.catch = !st.catch;
    }
    let out = Rect::from_min_size(pos2(bar.max.x - 34.0, bar.min.y + row2), vec2(34.0, 24.0));
    let inn = Rect::from_min_size(pos2(out.min.x - 38.0, bar.min.y + row2), vec2(34.0, 24.0));
    if icon_button(ui, inn, egui::Id::unique("evl-in"), glyphs::ph::PIANO_KEYS, st.midi_in, Color32::from_rgb(0xd0, 0x30, 0x30), true).on_hover_text("MIDI In: step input of notes from a keyboard").clicked() {
        st.midi_in = !st.midi_in;
        let on = st.midi_in;
        if on != app_step_input(app) {
            app.key_command(&ctx, "Show/Hide Step Input Keyboard");
        }
    }
    if icon_button(ui, out, egui::Id::unique("evl-out"), glyphs::ph::SPEAKER_HIGH, st.midi_out, GREEN, true).on_hover_text("MIDI Out: play events as they are selected").clicked() {
        st.midi_out = !st.midi_out;
    }
    let mut y = bar.max.y + 8.0 + row2;
    p.hline(body.x_range(), y - 4.0, Stroke::new(1.0, look::line()));
    if st.regions {
        y = region_level(app, ui, &mut st, Rect::from_min_max(pos2(body.min.x, y), body.max));
        let _ = y;
        with(&ctx, |s| *s = st);
        if let Some(n) = run {
            command(app, &ctx, n);
        }
        return;
    }
    // ---- the event type buttons (two rows of four)
    let bw = (body.width() - 20.0 - 3.0 * 8.0) / 4.0;
    for k in 0..8 {
        let r = Rect::from_min_size(pos2(body.min.x + 10.0 + (k % 4) as f32 * (bw + 8.0), y + (k / 4) as f32 * 26.0), vec2(bw, 20.0));
        if k < 7 {
            if pill(ui, r, egui::Id::unique(("evl-type", k)), TYPES[k].0, st.shown[k], BLUE).on_hover_text("Show or hide this event type (hidden events are protected)").clicked() {
                st.shown[k] = !st.shown[k];
                // hidden events leave the selection
                if !st.shown[k] {
                    if k == 0 {
                        app.selected_notes.clear();
                    }
                    if let Some((_, _, rg)) = region(app) {
                        st.sel_ev.retain(|i| rg.ext.events.get(*i).is_some_and(|e| type_index(e.kind) != k));
                    }
                }
            }
        } else if pill(ui, r, egui::Id::unique("evl-info"), "Additional Info", st.info, BLUE).on_hover_text("Show the bytes stored with each event").clicked() {
            st.info = !st.info;
        }
    }
    y += 52.0 + 4.0;
    p.hline(body.x_range(), y, Stroke::new(1.0, look::line()));
    y += 8.0;
    // ---- create row: +, event type, Quantize, Q
    let plus = Rect::from_min_size(pos2(body.min.x + 10.0, y), vec2(34.0, 24.0));
    let mut create = false;
    if icon_button(ui, plus, egui::Id::unique("evl-plus"), glyphs::ph::PLUS, false, BLUE, true).on_hover_text("Add an event of the type shown at the playhead").clicked() {
        create = true;
    }
    // (the type and Quantize pop-ups share what the row has)
    let room = body.max.x - 10.0 - 34.0 - 8.0 - (plus.max.x + 8.0);
    let pop_w = ((room - 8.0 - 70.0) / 2.0).clamp(70.0, 110.0);
    let tsel = Rect::from_min_size(pos2(plus.max.x + 8.0, y), vec2(pop_w, 24.0));
    {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(tsel));
        egui::ComboBox::from_id_salt("evl-create-type").width(tsel.width() - 8.0).wrap_mode(egui::TextWrapMode::Truncate).selected_text(CREATE[st.create].0).show_ui(&mut c, |ui| {
            for (k, (n, _)) in CREATE.iter().enumerate() {
                ui.selectable_value(&mut st.create, k, *n);
            }
        });
    }
    let qbtn = Rect::from_min_size(pos2(body.max.x - 10.0 - 34.0, y), vec2(34.0, 24.0));
    let qsel = Rect::from_min_size(pos2(qbtn.min.x - 8.0 - pop_w, y), vec2(pop_w, 24.0));
    if qsel.min.x > tsel.max.x + 70.0 {
        p.text(pos2(qsel.min.x - 6.0, qsel.center().y), Align2::RIGHT_CENTER, "Quantize:", look::regular(12.0), look::text_dim());
    }
    {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(qsel));
        egui::ComboBox::from_id_salt("evl-quant").width(qsel.width() - 8.0).wrap_mode(egui::TextWrapMode::Truncate).selected_text(lg_core::functions::QUANTIZE_STEPS[st.quantize]).show_ui(&mut c, |ui| {
            for (k, n) in lg_core::functions::QUANTIZE_STEPS.iter().enumerate() {
                ui.selectable_value(&mut st.quantize, k, *n);
            }
        });
    }
    let mut quantize = false;
    if pill(ui, qbtn, egui::Id::unique("evl-q"), "Q", false, BLUE).on_hover_text("Quantize the selected notes to the value shown").clicked() {
        quantize = true;
    }
    y = plus.max.y + 6.0;
    // ---- the list
    let status_h = 26.0;
    let list_area = Rect::from_min_max(pos2(body.min.x, y), pos2(body.max.x, body.max.y - status_h));
    let Some((t, r, rg)) = region(app) else {
        p.text(list_area.center(), Align2::CENTER_CENTER, "Select a MIDI region to list its events", look::regular(12.0), look::text_dim());
        with(&ctx, |s| *s = st);
        return;
    };
    let all = rows(&rg, &st.shown);
    let ev = crate::eventedit::state(&ctx);
    let w = list_area.width() - 12.0;
    // (Logic's widths at its default pane, in points, scaled to the pane)
    let k = (w - 36.0) / 362.0;
    let mut cols: Vec<(&str, f32)> = vec![("L", 18.0), ("M", 18.0), (if ev.relative { "Rel. Position" } else { "Position" }, 100.0 * k), ("Status", 40.0 * k), ("Ch", 30.0 * k), ("Num", 42.0 * k), ("Val", 40.0 * k), (if ev.abs_length { "End" } else { "Length/Info" }, 110.0 * k)];
    let artic_set = app.artic.tracks.get(&t).cloned();
    if st.artic_col {
        cols.push(("Artic.", w * 0.12));
        let scale = (w - 36.0) / cols.iter().skip(2).map(|c| c.1).sum::<f32>();
        cols.iter_mut().skip(2).for_each(|c| c.1 *= scale);
    }
    // the user's widths and order (the header's drags; Reset to Defaults in its menu)
    if st.col_scale.len() != cols.len() {
        st.col_scale = vec![1.0; cols.len()];
    }
    for (c, k) in cols.iter_mut().zip(&st.col_scale) {
        c.1 *= *k;
    }
    let order = column_order(&st.col_order, cols.len());
    let cxo = column_offsets(&cols.iter().map(|c| c.1).collect::<Vec<_>>(), &order);
    let hr = Rect::from_min_size(list_area.min, vec2(list_area.width(), ROW));
    p.rect_filled(hr, 0.0, look::header_bg());
    let hresp = ui.interact(hr, egui::Id::unique("evl-colhead"), Sense::click());
    hresp.context_menu(|ui| {
        if ui.button("Reset to Defaults").clicked() {
            st.col_scale.clear();
            st.col_order.clear();
            ui.close();
        }
    });
    for &i in &order {
        let (n, cw) = cols[i];
        let x = hr.min.x + 4.0 + cxo[i];
        let cr = Rect::from_min_max(pos2(x + 1.0, hr.min.y), pos2(x + cw - 3.0, hr.max.y));
        fit_text(&p, cr, n, 11.5, look::text_dim());
        p.vline(x + cw - 2.0, hr.y_range().shrink(5.0), Stroke::new(1.0, look::line()));
        // the separator: drag to resize (the Resize pointer)
        let sep = Rect::from_center_size(pos2(x + cw - 2.0, hr.center().y), vec2(7.0, hr.height()));
        let sresp = ui.interact(sep, egui::Id::unique("evl-colsep").with(i), Sense::drag());
        if sresp.hovered() || sresp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        if sresp.dragged() {
            let base = cw / st.col_scale[i].max(0.01);
            st.col_scale[i] = ((cw + sresp.drag_delta().x) / base.max(1.0)).clamp(0.2, 4.0);
        }
        // the name: drag left or right to move the column (L and M stay)
        if i >= 2 {
            let nresp = ui.interact(cr.shrink2(vec2(4.0, 0.0)), egui::Id::unique("evl-colname").with(i), Sense::drag());
            if nresp.dragged() {
                if let Some(pp) = nresp.interact_pointer_pos() {
                    p.vline(pp.x, hr.y_range(), Stroke::new(2.0, BLUE));
                }
            }
            if nresp.drag_stopped() {
                if let Some(pp) = nresp.interact_pointer_pos().or(ui.ctx().pointer_latest_pos()) {
                    let at = pp.x - hr.min.x - 4.0;
                    st.col_order = move_column(&order, i, &cols.iter().map(|c| c.1).collect::<Vec<_>>(), at);
                }
            }
        }
    }
    p.hline(hr.x_range(), hr.max.y, Stroke::new(1.0, look::line()));
    let list = Rect::from_min_max(pos2(list_area.min.x, hr.max.y + 1.0), list_area.max);
    let sel_rows = selected_rows(app, &st, &all);
    let playing = app.playing();
    let play_tick = app.position().max(0.0) as u32;
    let play_row = all.iter().rposition(|(p, _)| *p <= play_tick);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    // (rows touch, 24 pt apart, as Logic's stripes do)
    child.spacing_mut().item_spacing.y = 0.0;
    child.set_clip_rect(list);
    let mut sa = egui::ScrollArea::vertical().id_salt(("evl", t, r)).auto_shrink([false, false]);
    if let Some(k) = st.scroll_to.take() {
        sa = sa.vertical_scroll_offset((k as f32 * ROW - list.height() / 3.0).max(0.0));
    } else if playing && st.catch && !app.catch_off {
        if let Some(k) = play_row {
            sa = sa.vertical_scroll_offset((k as f32 * ROW - list.height() / 3.0).max(0.0));
        }
    }
    let mods = ctx.input(|i| i.modifiers);
    let mut edit: Option<(Row, Field, i32)> = None;
    let mut clicked: Option<(usize, bool)> = None;
    let mut dragging_over: Option<usize> = None;
    let mut toggle: Option<(Row, bool)> = None; // (row, lock column?)
    let mut ctx_menu: Option<&'static str> = None;
    let mut cc_pick: Option<(Row, u8)> = None;
    let mut artic_pick: Option<(usize, Option<u8>)> = None;
    let mut type_pos: Option<Row> = None;
    sa.show_rows(&mut child, ROW, all.len(), |ui, range| {
        for k in range {
            let (abs, row) = all[k];
            let (rr, resp) = ui.allocate_exact_size(vec2(list.width(), ROW), Sense::click_and_drag());
            let p = ui.painter().clone();
            let sel = sel_rows.contains(&row);
            if sel {
                p.rect_filled(rr, 0.0, BLUE.gamma_multiply(0.85));
            } else if k % 2 == 1 {
                p.rect_filled(rr, 0.0, look::list_stripe());
            }
            if playing && play_row == Some(k) {
                p.hline(rr.x_range(), rr.min.y + 0.5, Stroke::new(1.5, Color32::WHITE));
            }
            if resp.drag_started() {
                with(ui.ctx(), |s| s.drag_from = Some(k));
                clicked = Some((k, false));
            }
            if resp.clicked() {
                clicked = Some((k, true));
            }
            if ui.input(|i| i.pointer.primary_down()) && rr.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or_default()) {
                dragging_over = Some(k);
            }
            resp.context_menu(|ui| {
                if !sel {
                    ctx_menu = Some("__select_row");
                    with(ui.ctx(), |s| s.anchor = Some(k));
                }
                row_menu(ui, &mut ctx_menu);
            });
            if ctx_menu == Some("__select_row") {
                clicked = Some((k, true));
                ctx_menu = None;
            }
            let txt = if sel { Color32::WHITE } else { look::text() };
            let f = look::regular(12.0);
            let cx: Vec<f32> = cxo.iter().map(|o| rr.min.x + 4.0 + o).collect();
            let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x, rr.min.y + 1.0), vec2(w - 4.0, ROW - 2.0));
            // L, M
            let locked = is_locked(&rg, row);
            let lr = cell(cx[0], cols[0].1);
            if ui.interact(lr, ui.scope_id().with(("evl-L", k)), Sense::click()).on_hover_text("Lock SMPTE Position").clicked() {
                toggle = Some((row, true));
            }
            if locked {
                glyphs::paint(&p, lr, glyphs::ph_fill::LOCK_SIMPLE, 11.0, txt);
            }
            let mr = cell(cx[1], cols[1].1);
            let muted = matches!(row, Row::N(i) if rg.notes[i].muted);
            if matches!(row, Row::N(_)) && ui.interact(mr, ui.scope_id().with(("evl-M", k)), Sense::click()).on_hover_text("Mute").clicked() {
                toggle = Some((row, false));
            }
            if muted {
                p.text(mr.center(), Align2::CENTER_CENTER, "M", look::semibold(11.5), txt);
            }
            // Position: each unit dragged up / down; double-click types it
            let shown_pos = if ev.relative { abs.saturating_sub(rg.start) } else { abs };
            let pr = cell(cx[2], cols[2].1);
            let ptxt = pos_text(app, &st, shown_pos);
            // (right-aligned, as Logic's)
            p.text(pos2(pr.max.x - 4.0, pr.center().y), Align2::RIGHT_CENTER, &ptxt, f.clone(), txt);
            if !st.as_time {
                let bar = app.bar_ticks();
                let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1)) as u32;
                let units = [bar, beat, (beat / 4).max(1), 1];
                let uw = pr.width() / 4.0;
                for (u, size) in units.iter().enumerate() {
                    let ur = Rect::from_min_size(pos2(pr.min.x + u as f32 * uw, pr.min.y), vec2(uw, pr.height()));
                    let ui_id = ui.scope_id().with(("evl-pos", k, u));
                    let ur_resp = ui.interact(ur, ui_id, Sense::click_and_drag());
                    if ur_resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                        let acc_id = ui_id.with("acc");
                        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
                        acc += -ur_resp.drag_delta().y / 6.0;
                        let steps = acc.trunc();
                        ui.data_mut(|d| d.insert_temp(acc_id, acc - steps));
                        if steps != 0.0 {
                            edit = Some((row, Field::Pos, abs as i32 + steps as i32 * *size as i32));
                        }
                    }
                    if ur_resp.double_clicked() {
                        type_pos = Some(row);
                    }
                    if ur_resp.clicked() {
                        clicked = Some((k, true));
                    }
                }
            }
            // Status
            let sr = cell(cx[3], cols[3].1);
            match row {
                Row::N(_) => glyphs::paint(&p, Rect::from_min_size(sr.min, vec2(16.0, sr.height())), glyphs::ph_fill::MUSIC_NOTE_SIMPLE, 12.0, txt),
                Row::E(i) => {
                    // (narrowed to its minimum: one letter)
                    let full = status_text(rg.ext.events[i].kind);
                    let shown = if sr.width() < 30.0 { full.chars().next().map(|c| c.to_string()).unwrap_or_default() } else { full.to_string() };
                    p.text(pos2(sr.min.x + 2.0, sr.center().y), Align2::LEFT_CENTER, shown, f.clone(), txt);
                }
            }
            // Ch, Num, Val
            let slot = |ui: &mut egui::Ui, r: Rect, name: &str, fld: Field, shown: &dyn Fn(f32) -> String, edit: &mut Option<(Row, Field, i32)>| {
                let Some((v, lo, hi)) = field_of(&rg, row, fld) else { return };
                let mut fv = v as f32;
                let step = if matches!((row, fld), (Row::E(i), Field::Num | Field::Val) if rg.ext.events[i].kind & 0xF0 == 0xE0) { 64.0 } else { 1.0 };
                // (clipped to its cell)
                let idv = ui.scope_id().with((name, k));
                let mut c = ui.new_child(egui::UiBuilder::new().max_rect(r));
                c.set_clip_rect(r.intersect(ui.clip_rect()));
                if widgets::value_slot(&mut c, r, idv, &mut fv, lo as f32, hi as f32, v as f32, step, shown) {
                    *edit = Some((row, fld, fv as i32));
                }
            };
            slot(ui, cell(cx[4], cols[4].1), "evl-ch", Field::Ch, &|v| format!("{}", v as i32 + 1), &mut edit);
            let (num_shown, val_shown): (Box<dyn Fn(f32) -> String>, Box<dyn Fn(f32) -> String>) = match row {
                Row::N(_) => (Box::new(|v| crate::app::note_name(v as u8)), Box::new(|v| format!("{v:.0}"))),
                Row::E(i) => match rg.ext.events[i].kind & 0xF0 {
                    0xA0 => (Box::new(|v| crate::app::note_name(v as u8)), Box::new(|v| format!("{v:.0}"))),
                    0xE0 => {
                        let e = rg.ext.events[i];
                        (Box::new(move |_| format!("{}", e.number)), Box::new(move |_| format!("{}", e.value)))
                    }
                    _ => (Box::new(|v| format!("{v:.0}")), Box::new(|v| format!("{v:.0}"))),
                },
            };
            let kind = match row {
                Row::N(_) => 0x90,
                Row::E(i) => rg.ext.events[i].kind & 0xF0,
            };
            if kind == 0xC0 {
                // Program: no bank select sent (÷) and the program number in Val
                p.text(pos2(cx[5] + 2.0, rr.center().y), Align2::LEFT_CENTER, "÷", f.clone(), txt);
            }
            let num_r = cell(cx[5], cols[5].1);
            if kind != 0xC0 && kind != 0xD0 {
                slot(ui, num_r, "evl-num", Field::Num, &*num_shown, &mut edit);
            } else if kind == 0xD0 {
                slot(ui, num_r, "evl-num", Field::Num, &*num_shown, &mut edit);
            }
            let val_r = cell(cx[6], cols[6].1);
            if kind == 0xC0 {
                slot(ui, val_r, "evl-val", Field::Num, &*val_shown, &mut edit);
            } else if kind != 0xD0 {
                slot(ui, val_r, "evl-val", Field::Val, &*val_shown, &mut edit);
            }
            // Length/Info
            let lr = cell(cx[7], cols[7].1);
            match row {
                Row::N(i) => {
                    let n = rg.notes[i];
                    if st.info {
                        p.text(pos2(lr.min.x + 2.0, lr.center().y), Align2::LEFT_CENTER, bytes_text(row, &rg), f.clone(), txt);
                    } else if ev.abs_length {
                        p.text(pos2(lr.min.x + 2.0, lr.center().y), Align2::LEFT_CENTER, pos_text(app, &st, n.pos + n.len), f.clone(), txt);
                    } else if st.as_time {
                        p.text(pos2(lr.min.x + 2.0, lr.center().y), Align2::LEFT_CENTER, len_text(app, &st, n.pos, n.len), f.clone(), txt);
                    } else {
                        let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                        slot(ui, lr, "evl-len", Field::Len, &move |v| crate::app::bbdt_len(v as u32, sig), &mut edit);
                    }
                }
                Row::E(i) => {
                    let e = rg.ext.events[i];
                    let info = if st.info {
                        bytes_text(row, &rg)
                    } else {
                        match e.kind & 0xF0 {
                            0xB0 => {
                                let n = cc_name(e.number);
                                if n.is_empty() { format!("{}", e.number) } else if is_switch(e.number) { format!("{n} {}", if e.value > 0 { "on" } else { "off" }) } else { n.to_string() }
                            }
                            0xE0 => format!("{}", lg_core::regions::bend_value(&e)),
                            _ => String::new(),
                        }
                    };
                    p.with_clip_rect(lr).text(pos2(lr.min.x + 2.0, lr.center().y), Align2::LEFT_CENTER, info, f.clone(), txt);
                    if e.kind & 0xF0 == 0xB0 {
                        let resp = ui.interact(lr, ui.scope_id().with(("evl-ccname", k)), Sense::click());
                        egui::Popup::menu(&resp).show(|ui| {
                            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                for c in 0..128u8 {
                                    let n = cc_name(c);
                                    if ui.button(if n.is_empty() { format!("{c}") } else { format!("{c} {n}") }).clicked() {
                                        cc_pick = Some((row, c));
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                }
            }
            // Articulation
            if st.artic_col {
                if let Row::N(i) = row {
                    let ar = cell(cx[8], cols[8].1);
                    let cur = crate::artic::note_artic(&rg, &rg.notes[i]);
                    let label = match (cur, &artic_set) {
                        (Some(id), Some(set)) => set.artic(id).map_or(format!("{id}"), |a| a.name.clone()),
                        (Some(id), None) => format!("{id}"),
                        (None, _) => "–".into(),
                    };
                    let resp = ui.interact(ar, ui.scope_id().with(("evl-art", k)), Sense::click());
                    p.with_clip_rect(ar).text(pos2(ar.min.x + 2.0, ar.center().y), Align2::LEFT_CENTER, label, f.clone(), txt);
                    egui::Popup::menu(&resp).show(|ui| {
                        if ui.button("Off").clicked() {
                            artic_pick = Some((i, None));
                            ui.close();
                        }
                        match &artic_set {
                            Some(set) => {
                                for a in &set.artics {
                                    if ui.button(format!("{} {}", a.id, a.name)).clicked() {
                                        artic_pick = Some((i, Some(a.id)));
                                        ui.close();
                                    }
                                }
                            }
                            None => {
                                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                    for id in 1..=255u8 {
                                        if ui.button(format!("{id}")).clicked() {
                                            artic_pick = Some((i, Some(id)));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                    });
                }
            }
        }
    });
    // ---- the status bar
    let sb = Rect::from_min_max(pos2(body.min.x, body.max.y - status_h), body.max);
    p.rect_filled(sb, 0.0, look::header_bg());
    p.hline(sb.x_range(), sb.min.y, Stroke::new(1.0, look::line()));
    let tname = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    p.text(pos2(sb.min.x + 10.0, sb.center().y), Align2::LEFT_CENTER, tname, look::regular(12.0), look::text_dim());
    let count = all.len();
    p.text(pos2(sb.max.x - 10.0, sb.center().y), Align2::RIGHT_CENTER, if count == 1 { "1 Event".to_string() } else { format!("{count} Events") }, look::regular(12.0), look::text_dim());

    // ---- what happened
    if let Some((k, click)) = clicked {
        let row = all[k].1;
        let mut cur = selected_rows(app, &st, &all);
        if click && mods.shift {
            // a range from the anchor
            let a = st.anchor.unwrap_or(k);
            let (lo, hi) = (a.min(k), a.max(k));
            for x in &all[lo..=hi] {
                if !cur.contains(&x.1) {
                    cur.push(x.1);
                }
            }
        } else if click && mods.command {
            if let Some(i) = cur.iter().position(|x| *x == row) {
                cur.remove(i);
            } else {
                cur.push(row);
            }
            st.anchor = Some(k);
        } else if !cur.contains(&row) || click {
            cur = vec![row];
            st.anchor = Some(k);
        }
        select_rows(app, &mut st, &cur);
        if st.midi_out {
            preview(app, t, &rg, row);
        }
    }
    let drag_from = state(&ctx).drag_from;
    if let (Some(a), Some(b)) = (drag_from, dragging_over) {
        if a != b {
            let (lo, hi) = (a.min(b), a.max(b));
            let v: Vec<Row> = all[lo..=hi].iter().map(|x| x.1).collect();
            select_rows(app, &mut st, &v);
        }
    }
    if !ctx.input(|i| i.pointer.primary_down()) {
        st.drag_from = None;
    } else {
        st.drag_from = drag_from;
    }
    if let Some((row, lock)) = toggle {
        let mut targets = selected_rows(app, &st, &all);
        if !targets.contains(&row) {
            targets = vec![row];
        }
        if lock {
            let on = !is_locked(&rg, row);
            app.checkpoint(if on { "Lock SMPTE Position" } else { "Unlock SMPTE Position" });
            if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                for x in targets {
                    set_locked(rgm, x, on);
                }
            }
        } else {
            let on = !matches!(row, Row::N(i) if rg.notes[i].muted);
            app.checkpoint("Mute Notes");
            if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                for x in targets {
                    if let Row::N(i) = x {
                        if let Some(n) = rgm.notes.get_mut(i) {
                            n.muted = on;
                        }
                    }
                }
            }
        }
        app.after_edit();
    }
    if let Some((row, f, v)) = edit {
        apply_edit(app, &mut st, row, f, v, mods);
    }
    if let Some((row, c)) = cc_pick {
        apply_edit(app, &mut st, row, Field::Num, c as i32, egui::Modifiers::ALT | egui::Modifiers::SHIFT);
    }
    if let Some((i, id)) = artic_pick {
        app.checkpoint("Articulation");
        let mut targets: Vec<usize> = app.selected_notes.clone();
        if !targets.contains(&i) {
            targets = vec![i];
        }
        if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            for k in targets {
                crate::artic::set_note_artic(rgm, k, id);
            }
        }
        app.after_edit();
        if let Some(n) = rg.notes.get(i) {
            app.preview_note(t, n.pitch, n.velocity);
        }
    }
    if let Some(row) = type_pos {
        if let Row::N(i) = row {
            let text = crate::rightpane::bbdt(app, rg.notes[i].pos);
            crate::eventedit::with(&ctx, |s| s.typing = Some((i, text)));
        }
    }
    if create {
        let kind = CREATE[st.create].1;
        create_event(app, &mut st, kind);
    }
    if quantize {
        quantize_selected(app, &st);
    }
    // the typed position (Numerical Edit, double-click)
    if let Some((i, mut text)) = crate::eventedit::state(&ctx).typing {
        let mut done = None;
        egui::Area::new(egui::Id::unique("lg-ev-typing")).fixed_pos(list.min + vec2(10.0, 10.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Position:");
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0)).request_focus();
                if ui.input(|x| x.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|x| x.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        crate::eventedit::with(&ctx, |s| s.typing = if done.is_some() { None } else { Some((i, text.clone())) });
        if done == Some(true) {
            let locked = rg.notes.get(i).is_some() && is_locked(&rg, Row::N(i));
            if locked {
                app.tell("The event's position is locked (Unlock SMPTE Position)");
            } else {
                crate::eventedit::set_position(app, i, &text);
            }
        }
    }
    // the Paste position box
    if let Some((mut text, orig)) = st.paste.clone() {
        let mut done = None;
        egui::Area::new(egui::Id::unique("lg-evl-paste")).fixed_pos(list.min + vec2(10.0, 10.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Paste at position:");
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0)).request_focus();
                if ui.input(|x| x.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|x| x.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        st.paste = if done.is_some() { None } else { Some((text.clone(), orig)) };
        if done == Some(true) {
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            let at = if text.trim() == crate::rightpane::bbdt(app, orig) { Some(orig) } else { crate::eventedit::parse_position(&text, sig) };
            match at {
                Some(at) => paste(app, &mut st, at),
                None => app.tell("Type a position as bar beat division tick"),
            }
        }
    }
    with(&ctx, |s| *s = st);
    if let Some(n) = ctx_menu.or(run) {
        command(app, &ctx, n);
    }
}

fn app_step_input(app: &App) -> bool {
    crate::stepinput::state(&app.ctx).open
}

fn preview(app: &mut App, t: usize, rg: &lg_core::Region, row: Row) {
    if let Row::N(i) = row {
        if let Some(n) = rg.notes.get(i) {
            app.preview_note(t, n.pitch, n.velocity);
        }
    }
}

fn edit_menu(ui: &mut egui::Ui, run: &mut Option<&'static str>) {
    for n in ["Undo", "Redo"] {
        if ui.button(n).clicked() {
            *run = Some(n);
            ui.close();
        }
    }
    ui.separator();
    for n in ["Cut", "Copy", "Paste", "Delete"] {
        if ui.button(n).clicked() {
            *run = Some(n);
            ui.close();
        }
    }
    ui.separator();
    select_menu(ui, run);
}

fn select_menu(ui: &mut egui::Ui, run: &mut Option<&'static str>) {
    ui.menu_button("Select", |ui| {
        for (label, n) in [
            ("All", "Select All"),
            ("All Following", "Select All Following"),
            ("All Following of Same Pitch", "Select All Following of Same Track/Pitch"),
            ("Inside Locators", "Select All Inside Locators"),
            ("Deselect Outside Locators", "Deselect Outside Locators"),
            ("Muted Events", "Select Muted Regions/Cells/Events"),
            ("Overlapped Events", "Select Overlapped Regions/Events"),
            ("Same-Colored Events", "Select Same-Colored Regions/Cells/Events"),
            ("Equal Events", "Select Equal Regions/Events"),
            ("Similar Events", "Select Similar Regions/Events"),
            ("Same Channels", "Select Same Channels"),
            ("Same Subpositions", "Select Same Subpositions"),
            ("Same Articulation", "Select Same Articulation"),
            ("Highest Notes", "Select Highest Notes"),
            ("Lowest Notes", "Select Lowest Notes"),
            ("Invert Selection", "Invert Selection"),
            ("Deselect All", "Deselect All"),
        ] {
            if ui.button(label).clicked() {
                *run = Some(n);
                ui.close();
            }
        }
    });
}

fn row_menu(ui: &mut egui::Ui, run: &mut Option<&'static str>) {
    for (label, n) in [("Cut", "Cut"), ("Copy Event", "Copy"), ("Paste Event", "Paste"), ("Delete", "Delete")] {
        if ui.button(label).clicked() {
            *run = Some(n);
            ui.close();
        }
    }
    ui.separator();
    select_menu(ui, run);
    ui.separator();
    for n in ["Lock SMPTE Position", "Unlock SMPTE Position"] {
        if ui.button(n).clicked() {
            *run = Some(n);
            ui.close();
        }
    }
    if ui.button("Mute Events On/Off").clicked() {
        *run = Some("Mute Notes/Regions/Cells/Folders On/Off");
        ui.close();
    }
}

/// The Display Level: every region of the main window, by position; double-click opens one's
/// events, clicking a selected region's name renames it.
fn region_level(app: &mut App, ui: &mut egui::Ui, st: &mut State, area: Rect) -> f32 {
    let p = ui.painter().clone();
    let Some(proj) = app.project.as_ref() else { return area.min.y };
    let mut v: Vec<(u32, usize, usize)> = proj.tracks.iter().enumerate().flat_map(|(t, tr)| tr.regions.iter().enumerate().map(move |(r, rg)| (rg.start, t, r))).collect();
    v.sort();
    let names: Vec<(String, String, u32, bool)> = v.iter().map(|&(_, t, r)| {
        let rg = &proj.tracks[t].regions[r];
        (rg.name.clone(), proj.tracks[t].name.clone(), rg.length, rg.audio.is_some())
    }).collect();
    let hr = Rect::from_min_size(area.min, vec2(area.width(), ROW));
    let w = area.width() - 16.0;
    let cols = [("Position", w * 0.28), ("Name", w * 0.32), ("Track", w * 0.2), ("Length", w * 0.2)];
    p.rect_filled(hr, 0.0, look::header_bg());
    let mut x = hr.min.x + 8.0;
    for (n, cw) in cols {
        p.text(pos2(x, hr.center().y), Align2::LEFT_CENTER, n, look::regular(11.5), look::text_dim());
        x += cw;
    }
    let list = Rect::from_min_max(pos2(area.min.x, hr.max.y + 1.0), area.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    // (rows touch, 24 pt apart, as Logic's stripes do)
    child.spacing_mut().item_spacing.y = 0.0;
    child.set_clip_rect(list);
    let sig = proj.signature;
    let mut pick: Option<((usize, usize), bool)> = None;
    let mut rename_done: Option<((usize, usize), String)> = None;
    let mut start_rename: Option<(usize, usize)> = None;
    let selected = app.region_selection();
    egui::ScrollArea::vertical().id_salt("evl-regions").auto_shrink([false, false]).show_rows(&mut child, ROW, v.len(), |ui, range| {
        for k in range {
            let (start, t, r) = v[k];
            let (rr, resp) = ui.allocate_exact_size(vec2(list.width(), ROW), Sense::click());
            let pp = ui.painter().clone();
            let sel = selected.contains(&(t, r));
            if sel {
                pp.rect_filled(rr, 0.0, BLUE.gamma_multiply(0.85));
            } else if k % 2 == 1 {
                pp.rect_filled(rr, 0.0, look::list_stripe());
            }
            let txt = if sel { Color32::WHITE } else { look::text() };
            let mut x = rr.min.x + 8.0;
            pp.text(pos2(x, rr.center().y), Align2::LEFT_CENTER, crate::app::bbdt(start, sig), look::regular(12.0), txt);
            x += cols[0].1;
            let nr = Rect::from_min_size(pos2(x - 4.0, rr.min.y + 1.0), vec2(cols[1].1 - 4.0, ROW - 2.0));
            match &mut st.rename {
                Some((key, text)) if *key == (t, r) => {
                    let te = ui.put(nr, egui::TextEdit::singleline(text).font(look::regular(12.0)));
                    te.request_focus();
                    if te.lost_focus() {
                        rename_done = Some(((t, r), text.clone()));
                    }
                }
                _ => {
                    pp.text(pos2(x, rr.center().y), Align2::LEFT_CENTER, &names[k].0, look::regular(12.0), txt);
                    let nresp = ui.interact(nr, ui.scope_id().with(("evl-rname", k)), Sense::click());
                    if nresp.clicked() {
                        if sel && selected.len() == 1 {
                            start_rename = Some((t, r));
                        } else {
                            pick = Some(((t, r), false));
                        }
                    }
                    if nresp.double_clicked() {
                        pick = Some(((t, r), true));
                    }
                }
            }
            x += cols[1].1;
            pp.text(pos2(x, rr.center().y), Align2::LEFT_CENTER, format!("{} {}", t + 1, names[k].1), look::regular(12.0), txt);
            x += cols[2].1;
            pp.text(pos2(x, rr.center().y), Align2::LEFT_CENTER, crate::app::bbdt_len(names[k].2, sig), look::regular(12.0), txt);
            if resp.clicked() {
                pick = Some(((t, r), false));
            }
            if resp.double_clicked() {
                pick = Some(((t, r), true));
            }
            resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    start_rename = Some((t, r));
                    ui.close();
                }
            });
        }
    });
    if let Some(k) = start_rename {
        let name = app.project.as_ref().map(|p| p.tracks[k.0].regions[k.1].name.clone()).unwrap_or_default();
        st.rename = Some((k, name));
        app.selected_region = Some(k);
        app.selected_regions = vec![k];
    }
    if let Some(((t, r), name)) = rename_done {
        st.rename = None;
        app.checkpoint("Rename Region");
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.name = name;
        }
        app.after_edit();
    }
    if let Some(((t, r), open)) = pick {
        if ui.input(|i| i.modifiers.shift) {
            if !app.selected_regions.contains(&(t, r)) {
                app.selected_regions.push((t, r));
            }
        } else {
            app.selected_regions = vec![(t, r)];
        }
        app.selected_region = Some((t, r));
        app.selected_track = Some(t);
        app.selected_notes.clear();
        if open && app.project.as_ref().is_some_and(|p| p.tracks[t].regions[r].audio.is_none()) {
            st.regions = false;
        }
    }
    area.max.y
}

fn create_event(app: &mut App, st: &mut State, kind: u8) {
    let Some((t, r, rg)) = region(app) else {
        app.tell("Events can only be created inside a region: select a MIDI region first");
        return;
    };
    let at = app.position().max(0.0) as u32;
    if at < rg.start || at >= rg.start + rg.length {
        app.tell("Events can only be created inside a region: move the playhead into the region");
        return;
    }
    app.checkpoint("Create Event");
    let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { return };
    let (sn, se) = if kind == 0x90 {
        rgm.notes.push(Note { channel: 0, pitch: 60, velocity: 80, pos: at, len: 240, muted: false });
        (vec![rgm.notes.len() - 1], vec![])
    } else {
        let number = match kind {
            0xB0 => 1,
            0xA0 => 60,
            _ => 0,
        };
        rgm.ext.events.push(lg_core::regions::lane_event(kind, number, 0, at - rg.start, 64.0));
        (vec![], vec![rgm.ext.events.len() - 1])
    };
    resort(rgm, sn, se).apply(app, st);
    if !st.shown[type_index(kind)] {
        st.shown[type_index(kind)] = true;
    }
    app.after_edit();
}

fn quantize_selected(app: &mut App, st: &State) {
    let Some((t, r, _)) = region(app) else { return };
    let grid = lg_core::functions::quantize_value(lg_core::functions::QUANTIZE_STEPS[st.quantize]).and_then(|g| g.first().copied()).unwrap_or(0);
    if grid == 0 || app.selected_notes.is_empty() {
        return;
    }
    let sel = app.selected_notes.clone();
    app.checkpoint("Quantize");
    if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let locked: Vec<usize> = sel.iter().copied().filter(|&i| is_locked(rgm, Row::N(i))).collect();
        let free: Vec<usize> = sel.iter().copied().filter(|i| !locked.contains(i)).collect();
        let start = rgm.start;
        lg_core::functions::quantize(&mut rgm.notes, &free, start, grid, 100, 50);
    }
    app.after_edit();
}

fn paste(app: &mut App, st: &mut State, at: u32) {
    let Some((t, r, rg)) = region(app) else { return };
    let first = st.clip.iter().map(|c| match c {
        Clip::N(n) => n.pos,
        Clip::E(_, p) => *p,
    }).min().unwrap_or(at);
    let end = rg.start + rg.length;
    app.checkpoint("Paste Events");
    let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { return };
    let (mut sn, mut se) = (Vec::new(), Vec::new());
    for c in &st.clip {
        match c {
            Clip::N(n) => {
                let pos = n.pos - first + at;
                if pos >= rgm.start && pos < end {
                    rgm.notes.push(Note { pos, ..*n });
                    sn.push(rgm.notes.len() - 1);
                }
            }
            Clip::E(e, p) => {
                let pos = p - first + at;
                if pos >= rgm.start && pos < end {
                    rgm.ext.events.push(Control { pos: pos - rgm.start, ..*e });
                    se.push(rgm.ext.events.len() - 1);
                }
            }
        }
    }
    resort(rgm, sn, se).apply(app, st);
    app.after_edit();
}

/// The Event List has the key focus (clicked last: focus.rs); its key commands go to it.
pub fn focused(app: &App) -> bool {
    crate::focus::area(app) == crate::focus::Area::EventList && region(app).is_some()
}

/// Opens the Event Float (a double-click on a note in the Piano Roll).
pub fn open_float(ctx: &egui::Context) {
    with(ctx, |s| s.float = true);
}

/// Commands: the Event Float and time display anywhere; the edit and select commands while
/// the Event List has the pointer. False lets others run the command.
pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Show/Hide Event Float" => {
            with(ctx, |s| s.float = !s.float);
            return true;
        }
        "Show Event Position and Length as Time or Bars/Beats" => {
            with(ctx, |s| s.as_time = !s.as_time);
            return true;
        }
        _ => {}
    }
    if !focused(app) {
        return false;
    }
    let Some((t, r, rg)) = region(app) else { return false };
    let mut st = state(ctx);
    let all = rows(&rg, &st.shown);
    let sel = selected_rows(app, &st, &all);
    let done = match name {
        "Select All" => {
            let v: Vec<Row> = all.iter().map(|x| x.1).collect();
            select_rows(app, &mut st, &v);
            true
        }
        "Deselect All" => {
            select_rows(app, &mut st, &[]);
            true
        }
        "Select Muted Regions/Cells/Events" => {
            let v: Vec<Row> = all.iter().map(|x| x.1).filter(|x| matches!(x, Row::N(i) if rg.notes[*i].muted)).collect();
            select_rows(app, &mut st, &v);
            true
        }
        n if n.starts_with("Toggle Previous Region/Cell/Event") || n.starts_with("Toggle Next Region/Cell/Event") || n == "Select Previous Event" || n == "Select Next Event" => {
            let next = n.starts_with("Toggle Next") || n == "Select Next Event";
            let extend = n.starts_with("Toggle");
            let idx: Vec<usize> = all.iter().enumerate().filter(|(_, x)| sel.contains(&x.1)).map(|(k, _)| k).collect();
            let k = if next { idx.last().map_or(0, |k| (k + 1).min(all.len().saturating_sub(1))) } else { idx.first().map_or(0, |k| k.saturating_sub(1)) };
            if let Some(&(_, row)) = all.get(k) {
                let mut v = if extend { sel.clone() } else { Vec::new() };
                if !v.contains(&row) {
                    v.push(row);
                }
                select_rows(app, &mut st, &v);
                st.scroll_to = Some(k);
                if st.midi_out {
                    preview(app, t, &rg, row);
                }
            }
            true
        }
        "Scroll to Selection" => {
            st.scroll_to = all.iter().position(|x| sel.contains(&x.1));
            true
        }
        "Copy" | "Cut" => {
            st.clip = sel.iter().map(|x| match *x {
                Row::N(i) => Clip::N(rg.notes[i]),
                Row::E(i) => Clip::E(rg.ext.events[i], rg.start + rg.ext.events[i].pos),
            }).collect();
            if name == "Cut" {
                delete_rows(app, &mut st, t, r, &sel, "Cut");
            }
            true
        }
        "Paste" | "Paste Event" => {
            if !st.clip.is_empty() {
                let orig = st.clip.iter().map(|c| match c {
                    Clip::N(n) => n.pos,
                    Clip::E(_, p) => *p,
                }).min().unwrap_or(0);
                st.paste = Some((crate::rightpane::bbdt(app, orig), orig));
            }
            true
        }
        "Delete" => {
            delete_rows(app, &mut st, t, r, &sel, "Delete");
            true
        }
        "Lock SMPTE Position" | "Unlock SMPTE Position" | "Lock/Unlock SMPTE Position" => {
            if sel.is_empty() {
                return false;
            }
            let on = match name {
                "Lock SMPTE Position" => true,
                "Unlock SMPTE Position" => false,
                _ => !sel.iter().all(|x| is_locked(&rg, *x)),
            };
            app.checkpoint(if on { "Lock SMPTE Position" } else { "Unlock SMPTE Position" });
            if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                for x in &sel {
                    set_locked(rgm, *x, on);
                }
            }
            app.after_edit();
            true
        }
        _ => false,
    };
    with(ctx, |s| *s = st);
    done
}

fn delete_rows(app: &mut App, st: &mut State, t: usize, r: usize, sel: &[Row], what: &str) {
    if sel.is_empty() {
        return;
    }
    app.checkpoint(what);
    if let Some(rgm) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        let mut n: Vec<usize> = sel.iter().filter_map(|x| if let Row::N(i) = x { Some(*i) } else { None }).collect();
        let mut e: Vec<usize> = sel.iter().filter_map(|x| if let Row::E(i) = x { Some(*i) } else { None }).collect();
        n.sort_unstable();
        e.sort_unstable();
        for i in n.into_iter().rev() {
            if i < rgm.notes.len() {
                rgm.notes.remove(i);
            }
        }
        for i in e.into_iter().rev() {
            if i < rgm.ext.events.len() {
                rgm.ext.events.remove(i);
            }
        }
    }
    app.selected_notes.clear();
    st.sel_ev.clear();
    app.after_edit();
}

/// The Event Float window: the selected event (or region) as one Event List line.
pub fn float_window(app: &mut App, ctx: &egui::Context) {
    let mut st = state(ctx);
    if !st.float {
        return;
    }
    let mut open = true;
    let title = match app.selected_region.and_then(|(t, _)| app.project.as_ref()?.tracks.get(t)) {
        Some(tr) => format!("{} - Event Float", tr.name),
        None => "Event Float".to_string(),
    };
    let mut edit: Option<(Row, Field, i32)> = None;
    egui::Window::new(title).id(egui::Id::unique("lg-event-float")).open(&mut open).resizable(false).collapsible(false).default_width(420.0).show(ctx, |ui| {
        let Some((_, _, rg)) = region(app) else {
            match app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)) {
                Some(rg) => {
                    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                    ui.horizontal(|ui| {
                        ui.monospace(crate::app::bbdt(rg.start, sig));
                        ui.label(&rg.name);
                        ui.monospace(crate::app::bbdt_len(rg.length, sig));
                    });
                }
                None => {
                    ui.label(egui::RichText::new("No event selected").color(look::text_dim()));
                }
            }
            return;
        };
        let all = rows(&rg, &[true; 7]);
        let Some(&row) = selected_rows(app, &st, &all).first() else {
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            ui.horizontal(|ui| {
                ui.monospace(crate::app::bbdt(rg.start, sig));
                ui.label(&rg.name);
                ui.monospace(crate::app::bbdt_len(rg.length, sig));
            });
            return;
        };
        let (abs, _) = all.iter().find(|x| x.1 == row).copied().unwrap_or((0, row));
        ui.horizontal(|ui| {
            ui.monospace(pos_text(app, &st, abs));
            ui.add_space(8.0);
            match row {
                Row::N(i) => {
                    let n = rg.notes[i];
                    ui.label("Note");
                    let mut ch = n.channel as i32 + 1;
                    if ui.add(egui::DragValue::new(&mut ch).range(1..=16)).changed() {
                        edit = Some((row, Field::Ch, ch - 1));
                    }
                    let mut pitch = n.pitch as i32;
                    if ui.add(egui::DragValue::new(&mut pitch).range(0..=127).custom_formatter(|v, _| crate::app::note_name(v as u8))).changed() {
                        edit = Some((row, Field::Num, pitch));
                    }
                    let mut vel = n.velocity as i32;
                    if ui.add(egui::DragValue::new(&mut vel).range(1..=127)).changed() {
                        edit = Some((row, Field::Val, vel));
                    }
                    ui.monospace(len_text(app, &st, n.pos, n.len));
                }
                Row::E(i) => {
                    let e = rg.ext.events[i];
                    ui.label(status_text(e.kind));
                    let mut ch = e.channel as i32 + 1;
                    if ui.add(egui::DragValue::new(&mut ch).range(1..=16)).changed() {
                        edit = Some((row, Field::Ch, ch - 1));
                    }
                    if let Some((v, lo, hi)) = field_of(&rg, row, Field::Num) {
                        let mut v = v;
                        if ui.add(egui::DragValue::new(&mut v).range(lo..=hi)).changed() {
                            edit = Some((row, Field::Num, v));
                        }
                    }
                    if e.kind & 0xF0 != 0xC0 && e.kind & 0xF0 != 0xD0 && e.kind & 0xF0 != 0xE0 {
                        let mut v = e.value as i32;
                        if ui.add(egui::DragValue::new(&mut v).range(0..=127)).changed() {
                            edit = Some((row, Field::Val, v));
                        }
                    }
                }
            }
        });
    });
    if !open {
        st.float = false;
    }
    if let Some((row, f, v)) = edit {
        apply_edit(app, &mut st, row, f, v, egui::Modifiers::NONE);
    }
    with(ctx, |s| *s = st);
}

#[cfg(test)]
mod tests {
    #[test]
    fn columns_move_and_reset() {
        let w = [18.0, 18.0, 100.0, 40.0, 30.0, 42.0, 40.0, 110.0];
        let o = super::column_order(&[], 8);
        assert_eq!(o, (0..8).collect::<Vec<_>>());
        // Status dragged to the far right
        let o2 = super::move_column(&o, 3, &w, 1000.0);
        assert_eq!(o2, [0, 1, 2, 4, 5, 6, 7, 3]);
        let off = super::column_offsets(&w, &o2);
        assert_eq!(off[3], 18.0 + 18.0 + 100.0 + 30.0 + 42.0 + 40.0 + 110.0);
        // Length/Info dragged before Position
        assert_eq!(super::move_column(&o, 7, &w, 40.0), [0, 1, 7, 2, 3, 4, 5, 6]);
        // a stale order (another column count) falls back to Logic's
        assert_eq!(super::column_order(&o2, 9), (0..9).collect::<Vec<_>>());
    }

    use super::*;

    fn region_at(start: u32, length: u32) -> lg_core::Region {
        lg_core::Region { name: String::new(), start, length, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    #[test]
    fn group_changes_keep_distance_and_stop_at_limits() {
        // +20 on the second: the first would pass 127, so the change stops at +17
        assert_eq!(group_values(&[110, 60, 30], 1, 80, 0, 127, false, false), vec![127, 77, 47]);
        // Option: goes on, the first stays at the limit
        assert_eq!(group_values(&[110, 60, 30], 1, 80, 0, 127, true, false), vec![127, 80, 50]);
        // Option-Shift: all the same
        assert_eq!(group_values(&[110, 60, 30], 1, 80, 0, 127, true, true), vec![80, 80, 80]);
        // down past 0
        assert_eq!(group_values(&[10, 60], 1, 40, 0, 127, false, false), vec![0, 50]);
    }

    #[test]
    fn rows_mix_notes_and_events_and_filter() {
        let mut rg = region_at(3840, 3840);
        rg.notes.push(Note { channel: 0, pitch: 60, velocity: 90, pos: 3840 + 480, len: 240, muted: false });
        rg.ext.events.push(Control { kind: 0xB0, channel: 0, pos: 0, number: 7, value: 100 });
        rg.ext.events.push(Control { kind: 0xE0, channel: 0, pos: 960, number: 0, value: 64 });
        let all = rows(&rg, &[true; 7]);
        assert_eq!(all.iter().map(|x| x.1).collect::<Vec<_>>(), vec![Row::E(0), Row::N(0), Row::E(1)]);
        let mut shown = [true; 7];
        shown[0] = false; // notes hidden
        shown[2] = false; // pitch bend hidden
        assert_eq!(rows(&rg, &shown).iter().map(|x| x.1).collect::<Vec<_>>(), vec![Row::E(0)]);
    }

    #[test]
    fn locks_and_names() {
        let mut rg = region_at(0, 3840);
        rg.notes.push(Note { channel: 0, pitch: 64, velocity: 90, pos: 0, len: 240, muted: false });
        assert!(!is_locked(&rg, Row::N(0)));
        set_locked(&mut rg, Row::N(0), true);
        assert!(is_locked(&rg, Row::N(0)));
        set_locked(&mut rg, Row::N(0), false);
        assert!(!is_locked(&rg, Row::N(0)));
        assert_eq!(cc_name(7), "Volume");
        assert_eq!(cc_name(10), "Pan");
        assert!(is_switch(64) && !is_switch(7));
        assert_eq!(time_text(61.5), "00:01:01.500");
    }

    #[test]
    fn resort_keeps_selection() {
        let mut rg = region_at(0, 3840);
        rg.notes.push(Note { channel: 0, pitch: 60, velocity: 90, pos: 960, len: 240, muted: false });
        rg.notes.push(Note { channel: 0, pitch: 62, velocity: 90, pos: 0, len: 240, muted: false });
        let s = resort(&mut rg, vec![0], vec![]);
        assert_eq!(rg.notes[1].pitch, 60);
        assert_eq!(s.0, vec![1]);
    }
}
