//! Logic's automation beyond one lane (automation.rs draws and edits a lane): Trim and
//! Relative modes, region automation (the Automation button, the region lanes of channel and
//! plug-in parameters and MIDI data, the Convert / Move commands both ways, split and moved
//! with the region), automation subtracks under a track, Snap Automation and its offset, Move
//! Track Automation with Regions (Never / Always / Ask, trails, ramp time), the Automation
//! Event List, Automation Quick Access, Autoselect in Read mode, the Write pass's mode change,
//! Settings ▸ Automation, and the lane's shortcut menu.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::project::AutoLane;

use crate::app::App;
use crate::automation::{self, AutoMode, Key, MIDI_SLOT, PAN, SC_SLOT, VOLUME};
use crate::look;

/// The commands this file runs.
pub const COMMANDS: &[&str] = &[
    "Convert Visible Track Automation to Region Automation", "Convert Visible Region Automation to Track Automation", "Convert All Track Automation to Region Automation", "Convert All Region Automation to Track Automation",
    "Move Visible Track Automation to Region", "Move Visible Region Data to Track Automation", "Move All Track Automation to Region", "Move All Region Data to Track Automation",
    "Move Current Region Data to Track", "Move Current Track Data to Region", "Move All Region Data to Track", "Move All Track Data to Region", "Delete All Region Automation",
    "Enable Automation Quick Access", "Toggle Automation Quick Access", "Autoselect Automation Parameter in Read Mode", "Toggle Autoselect Automation Parameter in Read Mode", "Automation Settings…",
    "Set Selected Tracks or Channel Strips to Automation Read", "Toggle Selected Tracks or Channel Strips Automation Touch/Read", "Toggle Selected Tracks or Channel Strips Automation Write/Read",
    "Toggle Selected Tracks or Channel Strips Automation Trim Mode", "Toggle Selected Tracks or Channel Strips Automation Write Relative Mode", "Toggle Selected Tracks or Channel Strips Record Region Automation Mode",
    "Set All Tracks to Automation Write", "Track Automation Event List…",
    "Toggle Writing Volume Automation in Write Mode", "Toggle Writing Pan Automation in Write Mode", "Toggle Writing Mute Automation in Write Mode", "Toggle Writing Send Levels Automation in Write Mode", "Toggle Writing Plug-in parameters Automation in Write Mode", "Toggle Writing Solo Automation in Write Mode",
    "Set Move Track Automation with Regions to Never", "Set Move Track Automation with Regions to Always", "Set Move Track Automation with Regions to Ask", "Include Track Automation Region Trails, if possible",
    "Never move Track Automation with Regions", "Always move Track Automation with Regions", "Ask to move Track Automation with Regions",
    "Snap Automation", "Toggle Snap Automation", "Automation Snap Offset…",
    "Snap Automation Mode: Automatic", "Snap Automation Mode: Bar", "Snap Automation Mode: 1/2 Note", "Snap Automation Mode: 1/4 Note", "Snap Automation Mode: 1/8 Note", "Snap Automation Mode: 1/16 Note", "Snap Automation Mode: 1/32 Note", "Snap Automation Mode: 1/64 Note",
    "Snap Automation Mode: 1/2 Triplet (1/3)", "Snap Automation Mode: 1/4 Triplet (1/6)", "Snap Automation Mode: 1/8 Triplet (1/12)", "Snap Automation Mode: 1/16 Triplet (1/24)", "Snap Automation Mode: 1/32 Triplet (1/48)", "Snap Automation Mode: 1/64 Triplet (1/96)",
    "Automation: Toggle Track/Region", "Show Region Automation",
    "Toggle Automation Subtracks", "Hide Automation Subtracks", "Show Automation Subtracks", "Reset Automation Subtracks",
    "Region Automation: Volume", "Region Automation: Panorama", "Region Automation: Balance", "Region Automation: Modulation", "Region Automation: Breath", "Region Automation: Foot Control", "Region Automation: Portamento Time", "Region Automation: Expression", "Region Automation: Sustain Pedal",
    "Region Automation: Control Change 20", "Region Automation: Control Change 21", "Region Automation: Control Change 22", "Region Automation: Control Change 23", "Region Automation: Control Change 24", "Region Automation: Control Change 25", "Region Automation: Control Change 26",
    "Region Automation: Control Change 27", "Region Automation: Control Change 28", "Region Automation: Control Change 29", "Region Automation: Control Change 30", "Region Automation: Control Change 31", "Region Automation: Control Change 32",
    "Region Automation: Aftertouch", "Region Automation: Pitch Bend", "Region Automation: Program Change", "Region Automation: Note Velocity",
];

/// The MIDI parameters the automation menus offer (controller numbers; 128 pitch bend, 129
/// aftertouch, 130 program change, 131 note velocity: region automation only).
pub const MIDI_PARAMS: &[u8] = &[1, 2, 4, 5, 7, 8, 10, 11, 64, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 128, 129];
pub const PITCH_BEND: u32 = 128;
pub const AFTERTOUCH: u32 = 129;
pub const PROGRAM: u32 = 130;
pub const VELOCITY: u32 = 131;

pub fn cc_name(cc: u32) -> String {
    match cc {
        1 => "Modulation".into(),
        2 => "Breath".into(),
        4 => "Foot Control".into(),
        5 => "Portamento Time".into(),
        7 => "Volume".into(),
        8 => "Balance".into(),
        10 => "Panorama".into(),
        11 => "Expression".into(),
        64 => "Sustain Pedal".into(),
        PITCH_BEND => "Pitch Bend".into(),
        AFTERTOUCH => "Aftertouch".into(),
        PROGRAM => "Program Change".into(),
        VELOCITY => "Note Velocity".into(),
        n => format!("Control Change {n}"),
    }
}

/// The region automation key commands' parameters.
fn region_command_cc(name: &str) -> Option<u32> {
    let n = name.strip_prefix("Region Automation: ")?;
    Some(match n {
        "Aftertouch" => AFTERTOUCH,
        "Pitch Bend" => PITCH_BEND,
        "Program Change" => PROGRAM,
        "Note Velocity" => VELOCITY,
        n => {
            if let Some(x) = n.strip_prefix("Control Change ") {
                x.parse().ok()?
            } else {
                (0..128).find(|c| cc_name(*c) == n)?
            }
        }
    })
}

/// A track's automation subtracks: the lanes under it and whether they show.
#[derive(Clone, Debug, Default)]
pub struct Subs {
    pub open: bool,
    pub lanes: Vec<Key>,
}

pub const SUB_H: f32 = 44.0;

/// Snap Automation's values: name, ticks (0: Automatic, the Tracks area's zoom-dependent grid;
/// u32::MAX: a bar).
pub const SNAPS: [(&str, &str, u32); 14] = [
    ("Automatic", "Automatic", 0),
    ("Bar", "Bar", u32::MAX),
    ("1/2", "1/2 Note", 1920),
    ("1/4", "1/4 Note", 960),
    ("1/8", "1/8 Note", 480),
    ("1/16", "1/16 Note", 240),
    ("1/32", "1/32 Note", 120),
    ("1/64", "1/64 Note", 60),
    ("1/2 T", "1/2 Triplet (1/3)", 1280),
    ("1/4 T", "1/4 Triplet (1/6)", 640),
    ("1/8 T", "1/8 Triplet (1/12)", 320),
    ("1/16 T", "1/16 Triplet (1/24)", 160),
    ("1/32 T", "1/32 Triplet (1/48)", 80),
    ("1/64 T", "1/64 Triplet (1/96)", 40),
];

#[derive(Clone, Debug)]
enum RDrag {
    /// A region automation point moving: track, region, point, the press's tick (in the
    /// region) and value, the points as they were.
    Point { t: usize, r: usize, k: usize, from: (f32, f32), orig: Vec<(u32, u8)> },
    /// The Pencil across a region's lane.
    Draw { t: usize, r: usize, from: (u32, f32), orig: Vec<(u32, u8)> },
}

/// A region move waiting for the Ask dialog: the track it left and its span there, where it
/// is now.
#[derive(Clone, Debug)]
pub struct PendingMove {
    from_t: usize,
    from: u32,
    to: u32,
    dest_t: usize,
    dest: u32,
    copy: bool,
}

#[derive(Default)]
pub struct State {
    pub subs: HashMap<usize, Subs>,
    /// Tracks showing region automation (the Automation button), and the MIDI parameter
    /// their region lanes show (else the main lane's parameter).
    pub region_view: HashSet<usize>,
    pub region_cc: HashMap<usize, u32>,
    pub trim: HashSet<usize>,
    pub relative: HashSet<usize>,
    /// Tracks whose main lane shows the Relative curve.
    pub rel_edit: HashSet<usize>,
    pub snap_on: bool,
    pub snap: usize,
    pub aqa_learn: bool,
    pub autoselect: bool,
    /// Record Automation with Audio / MIDI Regions: per track, and the default for the others
    /// (on, as Logic's).
    pub region_rec: HashMap<usize, bool>,
    pub region_rec_off: [bool; 2],
    /// The dialog asking whether a Record Automation choice is for this track or all of its
    /// kind (track, the new value).
    region_rec_ask: Option<(usize, bool)>,
    pub event_list: bool,
    pub clip: Option<(Key, Vec<(u32, u8)>)>,
    pub ask: Option<PendingMove>,
    offset_dialog: Option<String>,
    rdrag: Option<RDrag>,
    pub was_playing: bool,
    /// Tracks written in this pass (their mode changes after a Write pass; Trim folded in).
    pub wrote: HashSet<usize>,
    /// What the screen control and MIDI lanes last sent while playing.
    sent: HashMap<(usize, u32, u32), u8>,
    replaying: bool,
    sub_drag: Option<(usize, usize, f32)>,
}

/// Forget the last project's per-track state.
pub fn reset(app: &mut App) {
    let keep = (app.ax.snap_on, app.ax.snap, app.ax.autoselect, app.ax.region_rec_off);
    app.ax = State { snap_on: keep.0, snap: keep.1, autoselect: keep.2, region_rec_off: keep.3, ..Default::default() };
}

/// Tracks reordered or deleted (`map`: old index to new).
pub fn remap(app: &mut App, map: &[Option<usize>]) {
    let m = |k: usize| map.get(k).copied().flatten();
    let ax = &mut app.ax;
    ax.subs = std::mem::take(&mut ax.subs).into_iter().filter_map(|(k, v)| Some((m(k)?, v))).collect();
    ax.region_cc = std::mem::take(&mut ax.region_cc).into_iter().filter_map(|(k, v)| Some((m(k)?, v))).collect();
    ax.region_rec = std::mem::take(&mut ax.region_rec).into_iter().filter_map(|(k, v)| Some((m(k)?, v))).collect();
    for s in [&mut ax.region_view, &mut ax.trim, &mut ax.relative, &mut ax.rel_edit] {
        *s = std::mem::take(s).into_iter().filter_map(m).collect();
    }
    ax.sent.clear();
    ax.clip = None;
}

// ---- Settings ▸ Automation ---------------------------------------------------------------------

const K_MOVE: &str = "Automation /  / Move Track Automation with Regions:";
const K_TRAILS: &str = "Automation /  / Include trails, if possible";
const K_PENCIL: &str = "Automation /  / Pencil Tool:";
const K_OFFSET: &str = "Automation /  / Snap Offset:";
const K_RAMP: &str = "Automation /  / Ramp Time:";
const K_WRITE_TO: &str = "Automation /  / Write Mode Changes To:";
const K_AQA: &str = "Automation /  / Automation Quick Access:";
const K_AQA_CC: &str = "Automation /  / Automation Quick Access Controller";

fn get(app: &App, k: &str) -> String {
    app.logic_settings.get(k)
}

fn set(app: &mut App, k: &str, v: &str) {
    app.logic_settings.0.insert(k.to_string(), v.to_string());
    app.logic_settings.save();
}

pub fn move_mode(app: &App) -> String {
    get(app, K_MOVE)
}

pub fn autoselect(app: &App) -> bool {
    app.ax.autoselect
}

fn aqa_on(app: &App) -> bool {
    get(app, K_AQA) == "On"
}

/// Settings ▸ Automation ▸ Write Automation For: the parameters a Write pass writes.
pub fn writes(app: &App, key: Key) -> bool {
    let k = match (key.plugin, key.param) {
        (Some((SC_SLOT | MIDI_SLOT, _)), _) => return true,
        (Some(_), _) => "Plug-in",
        (None, VOLUME) => "Volume",
        (None, PAN) => "Pan",
        (None, automation::MUTE) => "Mute",
        (None, automation::SOLO) => "Solo",
        _ => return true,
    };
    app.logic_settings.on(&format!("Automation /  / {k}"))
}

/// The Pencil's drag makes stepped points (Settings ▸ Automation ▸ Pencil Tool: Hold Option for
/// Stepped Editing: with Option; Hold Option for Curved Editing: without it).
pub fn pencil_stepped(app: &App, alt: bool) -> bool {
    if get(app, K_PENCIL) == "Hold Option for Curved Editing" { !alt } else { alt }
}

fn offset(app: &App) -> i32 {
    get(app, K_OFFSET).trim().parse().unwrap_or(-5)
}

fn ramp_ticks(app: &App) -> u32 {
    let ms: f64 = get(app, K_RAMP).trim().parse().unwrap_or(200.0);
    (ms / (60_000.0 / app.tempo().max(1.0) / lg_core::events::PPQ as f64)).round().max(1.0) as u32
}

/// The Snap Automation value in ticks at this zoom.
pub fn snap_grid(app: &App, px_per_tick: f32) -> f32 {
    match SNAPS[app.ax.snap.min(SNAPS.len() - 1)].2 {
        0 => crate::arranging::snap_grid(app, px_per_tick),
        u32::MAX => app.bar_ticks() as f32,
        g => g as f32,
    }
}

/// A tick on the Snap Automation grid (Snap Automation on; the Snap Offset added).
pub fn snap_tick(app: &App, tick: f32, px_per_tick: f32) -> u32 {
    if !app.ax.snap_on {
        return tick.max(0.0) as u32;
    }
    lg_core::automation::snap(tick.max(0.0) as u32, snap_grid(app, px_per_tick) as u32, offset(app))
}

// ---- modes -------------------------------------------------------------------------------------

/// The tracks a mode or option choice applies to: all selected ones when `t` is one of them.
fn targets(app: &App, t: usize) -> Vec<usize> {
    let sel = crate::trackarea::selection(app);
    if sel.contains(&t) { sel } else { vec![t] }
}

pub fn set_mode(app: &mut App, t: usize, m: AutoMode) {
    for x in targets(app, t) {
        app.auto_mode.insert(x, m);
    }
    app.load_engine();
}

fn is_audio(app: &App, t: usize) -> bool {
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Audio)
}

pub fn region_rec(app: &App, t: usize) -> bool {
    app.ax.region_rec.get(&t).copied().unwrap_or(!app.ax.region_rec_off[usize::from(!is_audio(app, t))])
}

/// The mode pop-up's end: Trim, Relative and Record Automation with Audio / MIDI Regions.
pub fn mode_menu_extras(app: &mut App, ui: &mut egui::Ui, t: usize) {
    for (set, name) in [(0, "Trim"), (1, "Relative")] {
        let on = if set == 0 { app.ax.trim.contains(&t) } else { app.ax.relative.contains(&t) };
        if ui.add(egui::Button::selectable(on, name)).clicked() {
            for x in targets(app, t) {
                let s = if set == 0 { &mut app.ax.trim } else { &mut app.ax.relative };
                if on {
                    s.remove(&x);
                } else {
                    s.insert(x);
                }
                // (Trim and Relative don't go together)
                let other = if set == 0 { &mut app.ax.relative } else { &mut app.ax.trim };
                other.remove(&x);
            }
            ui.close();
        }
    }
    ui.separator();
    let label = if is_audio(app, t) { "Record Automation with Audio Regions" } else { "Record Automation with MIDI Regions" };
    let on = region_rec(app, t);
    if ui.add(egui::Button::selectable(on, label)).clicked() {
        if ui.input(|i| i.modifiers.alt) {
            // Option: every track of this kind, now and later
            let kind = usize::from(!is_audio(app, t));
            app.ax.region_rec_off[kind] = on;
            let same: Vec<usize> = app.ax.region_rec.keys().copied().filter(|&x| usize::from(!is_audio(app, x)) == kind).collect();
            for x in same {
                app.ax.region_rec.remove(&x);
            }
        } else {
            app.ax.region_rec_ask = Some((t, !on));
        }
        ui.close();
    }
}

pub fn toggle_region_view(app: &mut App, t: usize) {
    if !app.ax.region_view.insert(t) {
        app.ax.region_view.remove(&t);
    }
    app.ta.auto.sel = None;
}

// ---- region automation -------------------------------------------------------------------------

/// What a track's region lanes show: a channel / plug-in parameter, or MIDI data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RKind {
    Lane(Key),
    Midi(u32),
}

pub fn region_kind(app: &App, t: usize) -> RKind {
    match app.ax.region_cc.get(&t) {
        Some(&c) => RKind::Midi(c),
        None => RKind::Lane(automation::shown(app, t).abs()),
    }
}

/// A region's points for `kind` (ticks from the region's start, 0-127).
pub fn region_points(r: &lg_core::Region, kind: RKind) -> Vec<(u32, u8)> {
    match kind {
        RKind::Lane(k) => r.ext.automation.iter().find(|l| automation::matches(l, k)).map(|l| l.points.clone()).unwrap_or_default(),
        RKind::Midi(VELOCITY) => r.notes.iter().map(|n| (n.pos.saturating_sub(r.start), n.velocity)).collect(),
        RKind::Midi(c) => {
            let mut v: Vec<(u32, u8)> = r.ext.events.iter().filter(|e| midi_match(e, c)).map(|e| (e.pos, midi_value(e))).collect();
            v.sort_by_key(|p| p.0);
            v
        }
    }
}

fn midi_match(e: &lg_core::events::Control, c: u32) -> bool {
    match c {
        PITCH_BEND => e.kind == 0xE0,
        AFTERTOUCH => e.kind == 0xD0,
        PROGRAM => e.kind == 0xC0,
        c => e.kind == 0xB0 && e.number as u32 == c,
    }
}

/// (Logic's records: the first data byte where the note's pitch is, the second where its
/// velocity is; pitch bend's MSB is the second)
fn midi_value(e: &lg_core::events::Control) -> u8 {
    match e.kind {
        0xC0 | 0xD0 => e.number,
        _ => e.value,
    }
}

/// A region's points for `kind` set (the lane, the MIDI events, the notes' velocities).
pub fn set_region_points(r: &mut lg_core::Region, kind: RKind, mut pts: Vec<(u32, u8)>) {
    pts.sort_by_key(|p| p.0);
    match kind {
        RKind::Lane(k) => {
            r.ext.automation.retain(|l| !automation::matches(l, k));
            if !pts.is_empty() {
                let mut l = AutoLane { param: k.param, plugin: k.plugin, relative: k.rel, points: pts, ..Default::default() };
                lg_core::automation::tidy(&mut l);
                r.ext.automation.push(l);
            }
        }
        RKind::Midi(VELOCITY) => {
            for (n, p) in r.notes.iter_mut().zip(pts) {
                n.velocity = p.1.max(1);
            }
        }
        RKind::Midi(c) => {
            let ch = r.ext.events.iter().find(|e| midi_match(e, c)).map_or(0, |e| e.channel);
            r.ext.events.retain(|e| !midi_match(e, c));
            for (pos, v) in pts {
                let (kind, number, value) = match c {
                    PITCH_BEND => (0xE0, 0, v),
                    AFTERTOUCH => (0xD0, v, 0),
                    PROGRAM => (0xC0, v, 0),
                    c => (0xB0, c as u8, v),
                };
                r.ext.events.push(lg_core::events::Control { kind, channel: ch, pos, number, value });
            }
            r.ext.events.sort_by_key(|e| e.pos);
        }
    }
}

fn region_value_label(app: &App, t: usize, kind: RKind, r: &lg_core::Region, k: usize, v: u8) -> String {
    match kind {
        RKind::Lane(key) => automation::key_label(app, t, key, v as f32),
        RKind::Midi(VELOCITY) => r.notes.get(k).map_or(format!("{v}"), |n| format!("{} {v}", crate::app::note_name(n.pitch))),
        RKind::Midi(PITCH_BEND) => format!("{:+}", v as i32 - 64),
        RKind::Midi(_) => format!("{v}"),
    }
}

/// The region lanes over a track's row (the Automation button on): each region's own
/// automation (or MIDI data) inside it. Click inside a region: a point there; drag a point;
/// double- or Option-click: remove it; the Pencil draws (with Note Velocity: the notes it
/// crosses get the line's values).
pub fn region_lane(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { return };
    let kind = region_kind(app, t);
    let p = ui.painter_at(row);
    p.rect_filled(row, 0.0, Color32::from_black_alpha(70));
    let inner = row.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.max.y - v / 127.0 * inner.height();
    let v_of = |y: f32| ((inner.max.y - y) / inner.height() * 127.0).clamp(0.0, 127.0);
    let px_per_tick = ((x_of(960.0) - x_of(0.0)) / 960.0).max(1e-6);
    let col = Color32::from_rgb(0x6a, 0xb0, 0xf0);
    let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
    let alt = ui.input(|i| i.modifiers.alt);
    let name = match kind {
        RKind::Lane(k) => automation::key_name(app, t, k),
        RKind::Midi(c) => cc_name(c),
    };
    let mut edit: Option<(usize, Vec<(u32, u8)>)> = None;
    for (ri, r) in tr.regions.iter().enumerate() {
        let span = Rect::from_x_y_ranges(x_of(r.start as f32)..=x_of((r.start + r.length) as f32), row.y_range()).intersect(row);
        if span.width() < 1.0 {
            continue;
        }
        p.rect_filled(span, 2.0, Color32::from_rgba_unmultiplied(0x40, 0x70, 0xb0, 40));
        p.rect_stroke(span, 2.0, Stroke::new(1.0, col.gamma_multiply(0.4)), egui::StrokeKind::Inside);
        let pts = region_points(r, kind);
        let xr = |tk: u32| x_of((r.start + tk) as f32);
        if pts.is_empty() {
            p.text(pos2(span.min.x + 4.0, span.min.y + 8.0), Align2::LEFT_CENTER, &name, look::regular(9.5), col.gamma_multiply(0.7));
        } else if kind == RKind::Midi(VELOCITY) {
            for &(tk, v) in &pts {
                p.line_segment([pos2(xr(tk), inner.max.y), pos2(xr(tk), y_of(v as f32))], Stroke::new(2.0, col));
            }
        } else {
            // (MIDI data hold their value to the next event; region automation is a line)
            let mut line = vec![pos2(span.min.x, y_of(pts[0].1 as f32))];
            for w in pts.windows(2) {
                line.push(pos2(xr(w[0].0), y_of(w[0].1 as f32)));
                if matches!(kind, RKind::Midi(_)) {
                    line.push(pos2(xr(w[1].0), y_of(w[0].1 as f32)));
                }
            }
            let last = *pts.last().unwrap();
            line.push(pos2(xr(last.0), y_of(last.1 as f32)));
            line.push(pos2(span.max.x, y_of(last.1 as f32)));
            p.add(egui::Shape::line(line, Stroke::new(1.6, col)));
        }
        let mut on_point = false;
        for (k, &(tk, v)) in pts.iter().enumerate() {
            let c = pos2(xr(tk), y_of(v as f32));
            if !span.expand(2.0).contains(c) {
                continue;
            }
            let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("rautopt", t, ri, k)), Sense::click_and_drag());
            on_point |= resp.hovered() || resp.dragged();
            p.circle_filled(c, if resp.hovered() { 5.0 } else { 3.0 }, col);
            if resp.hovered() || pts.len() <= 16 {
                p.text(c + vec2(6.0, -9.0), Align2::LEFT_CENTER, region_value_label(app, t, kind, r, k, v), look::regular(9.5), col);
            }
            if (resp.double_clicked() || (resp.clicked() && alt) || (resp.clicked() && tool == crate::edit::Tool::Eraser)) && kind != RKind::Midi(VELOCITY) {
                app.checkpoint("Delete Region Automation Point");
                let mut n = pts.clone();
                n.remove(k);
                edit = Some((ri, n));
            } else if resp.drag_started() {
                app.checkpoint("Move Region Automation Point");
                if let Some(pp) = resp.interact_pointer_pos() {
                    app.ax.rdrag = Some(RDrag::Point { t, r: ri, k, from: (tick_of(pp.x) - r.start as f32, v_of(pp.y)), orig: pts.clone() });
                }
            } else if let (true, Some(pp), Some(RDrag::Point { t: dt, r: dr, k: dk, from, orig })) = (resp.dragged(), resp.interact_pointer_pos(), app.ax.rdrag.clone()) {
                if dt == t && dr == ri && dk == k {
                    let mut n = orig.clone();
                    let rel = (tick_of(pp.x) - r.start as f32).clamp(0.0, r.length as f32);
                    let nt = if kind == RKind::Midi(VELOCITY) { orig[k].0 } else { snap_tick(app, orig[k].0 as f32 + rel - from.0, px_per_tick).min(r.length) };
                    n[k] = (nt, (orig[k].1 as f32 + v_of(pp.y) - from.1).round().clamp(0.0, 127.0) as u8);
                    app.help_tag = Some(vec![(name.clone(), region_value_label(app, t, kind, r, k, n[k].1))]);
                    edit = Some((ri, n));
                }
            }
            if resp.drag_stopped() {
                app.ax.rdrag = None;
                app.load_engine();
            }
        }
        if on_point {
            continue;
        }
        let bg = ui.interact(span, ui.scope_id().with(("rautolane", t, ri)), Sense::click_and_drag());
        if bg.clicked() && kind != RKind::Midi(VELOCITY) {
            if let Some(pp) = bg.interact_pointer_pos() {
                app.checkpoint("Create Region Automation Point");
                let tk = snap_tick(app, tick_of(pp.x) - r.start as f32, px_per_tick).min(r.length);
                let mut n = pts.clone();
                n.retain(|x| x.0 != tk);
                n.push((tk, v_of(pp.y).round() as u8));
                edit = Some((ri, n));
            }
        }
        if bg.drag_started() && tool == crate::edit::Tool::Pencil {
            let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(span.center());
            app.checkpoint("Draw Region Automation");
            app.ax.rdrag = Some(RDrag::Draw { t, r: ri, from: ((tick_of(o.x) - r.start as f32).max(0.0) as u32, v_of(o.y)), orig: pts.clone() });
        }
        if let (true, Some(pp), Some(RDrag::Draw { t: dt, r: dr, from, orig })) = (bg.dragged(), bg.interact_pointer_pos(), app.ax.rdrag.clone()) {
            if dt == t && dr == ri {
                let now = ((tick_of(pp.x) - r.start as f32).max(0.0) as u32).min(r.length);
                let (vnow, (a, va), (b, vb)) = (v_of(pp.y), from, (now, v_of(pp.y)));
                let _ = vnow;
                let (lo, hi) = (a.min(b), a.max(b));
                let line_at = |tk: u32| if a == b { vb } else { va + (vb - va) * (tk as f32 - a as f32) / (b as f32 - a as f32) };
                let n: Vec<(u32, u8)> = if kind == RKind::Midi(VELOCITY) {
                    orig.iter().map(|&(tk, v)| if tk >= lo && tk <= hi { (tk, line_at(tk).round().clamp(1.0, 127.0) as u8) } else { (tk, v) }).collect()
                } else {
                    let step = (snap_grid(app, px_per_tick) / 4.0).max(30.0) as u32;
                    let mut n: Vec<(u32, u8)> = orig.iter().copied().filter(|p| p.0 < lo || p.0 > hi).collect();
                    let mut tk = lo;
                    while tk <= hi {
                        n.push((tk, line_at(tk).round().clamp(0.0, 127.0) as u8));
                        tk += step;
                    }
                    n.push((hi, line_at(hi).round().clamp(0.0, 127.0) as u8));
                    n.dedup_by_key(|p| p.0);
                    n
                };
                p.line_segment([pos2(xr(a), y_of(va)), pos2(xr(b), y_of(vb))], Stroke::new(1.0, Color32::WHITE));
                edit = Some((ri, n));
            }
        }
        if bg.drag_stopped() {
            app.ax.rdrag = None;
            app.load_engine();
        }
        let shown = automation::shown(app, t);
        lane_menu(app, &bg, t, shown);
    }
    if let Some((ri, mut n)) = edit {
        n.sort_by_key(|p| p.0);
        if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(ri)) {
            set_region_points(r, kind, n);
        }
        app.dirty = true;
        app.load_engine();
    }
}

/// The regions the Convert / Move commands work on for track `t`: the selected ones there,
/// else all of the track's.
fn regions_of(app: &App, t: usize) -> Vec<usize> {
    let sel: Vec<usize> = app.region_selection().into_iter().filter(|x| x.0 == t).map(|x| x.1).collect();
    if !sel.is_empty() {
        return sel;
    }
    app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(Vec::new(), |tr| (0..tr.regions.len()).collect())
}

/// The tracks the Convert / Move commands work on: those with selected regions, else the
/// selected tracks.
fn convert_tracks(app: &App) -> Vec<usize> {
    let mut v: Vec<usize> = app.region_selection().into_iter().map(|x| x.0).collect();
    if v.is_empty() {
        v = crate::trackarea::selection(app);
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// Track automation moved into the regions under it (`all`: every parameter, else the one
/// shown). Region automation of MIDI data (a region lane showing a controller) comes from the
/// track's MIDI lane of that controller.
pub fn track_to_region(app: &mut App, all: bool) -> bool {
    let tracks = convert_tracks(app);
    if tracks.is_empty() {
        return false;
    }
    app.checkpoint("Convert Track Automation to Region Automation");
    for t in tracks {
        let shown = automation::shown(app, t).abs();
        let midi = app.ax.region_cc.get(&t).copied();
        let regions = regions_of(app, t);
        let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { continue };
        let keys: Vec<Key> = if all { tr.automation.iter().filter(|l| !l.relative).map(Key::of).collect() } else if let Some(c) = midi { vec![Key::plugin(MIDI_SLOT, c)] } else { vec![shown] };
        for k in keys {
            let Some(li) = tr.automation.iter().position(|l| automation::matches(l, k)) else { continue };
            for &ri in &regions {
                let Some((s, len)) = tr.regions.get(ri).map(|r| (r.start, r.length)) else { continue };
                let part = lg_core::automation::take_span(&mut tr.automation[li], s, s + len);
                if part.points.is_empty() {
                    continue;
                }
                let r = &mut tr.regions[ri];
                // (a MIDI lane becomes the region's MIDI events; the rest region automation)
                match k.plugin {
                    Some((MIDI_SLOT, c)) if r.audio.is_none() && c < 128 => set_region_points(r, RKind::Midi(c), part.points),
                    _ => {
                        r.ext.automation.retain(|l| !automation::matches(l, k));
                        r.ext.automation.push(part);
                    }
                }
            }
        }
        tr.automation.retain(|l| !l.points.is_empty());
    }
    app.after_edit();
    true
}

/// Region automation moved onto the track (`all`: every parameter, else the one shown).
pub fn region_to_track(app: &mut App, all: bool) -> bool {
    let tracks = convert_tracks(app);
    if tracks.is_empty() {
        return false;
    }
    app.checkpoint("Convert Region Automation to Track Automation");
    for t in tracks {
        let kind = region_kind(app, t);
        let regions = regions_of(app, t);
        let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { continue };
        for ri in regions {
            let Some(r) = tr.regions.get_mut(ri) else { continue };
            let start = r.start;
            let mut parts: Vec<AutoLane> = Vec::new();
            match kind {
                RKind::Midi(c) if !all && c < 128 => {
                    let pts = region_points(r, kind);
                    set_region_points(r, kind, Vec::new());
                    if !pts.is_empty() {
                        parts.push(AutoLane { param: 0, plugin: Some((MIDI_SLOT, c)), points: pts, ..Default::default() });
                    }
                }
                RKind::Lane(k) if !all => {
                    if let Some(i) = r.ext.automation.iter().position(|l| automation::matches(l, k)) {
                        parts.push(r.ext.automation.remove(i));
                    }
                }
                _ => parts = std::mem::take(&mut r.ext.automation),
            }
            for part in parts {
                let k = Key::of(&part);
                if !tr.automation.iter().any(|l| automation::matches(l, k)) {
                    tr.automation.push(AutoLane { param: k.param, plugin: k.plugin, relative: k.rel, ..Default::default() });
                }
                if let Some(l) = tr.automation.iter_mut().find(|l| automation::matches(l, k)) {
                    lg_core::automation::put_span(l, &part, start);
                }
            }
        }
    }
    app.after_edit();
    true
}

// ---- Move Track Automation with Regions ----------------------------------------------------------

/// A region drag ended: it left track `from_t` at `from_start` and is now region `r` of `t`.
/// The track automation under it goes with it (Settings: Never, Always, Ask).
pub fn region_moved(app: &mut App, from_t: usize, from_start: u32, t: usize, r: usize) {
    let Some((start, len)) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| (x.start, x.length)) else { return };
    if from_t == t && from_start == start {
        return;
    }
    let has = app.project.as_ref().and_then(|p| p.tracks.get(from_t)).is_some_and(|tr| tr.automation.iter().any(|l| l.points.iter().any(|p| p.0 >= from_start && p.0 <= from_start + len)));
    if !has {
        return;
    }
    // (an Option-drag copy leaves the original where it was)
    let copy = app.project.as_ref().and_then(|p| p.tracks.get(from_t)).is_some_and(|tr| tr.regions.iter().enumerate().any(|(k, x)| (from_t, k) != (t, r) && x.start == from_start && x.length == len));
    let mv = PendingMove { from_t, from: from_start, to: from_start + len, dest_t: t, dest: start, copy };
    match move_mode(app).as_str() {
        "Always" => apply_move(app, mv),
        "Ask" => app.ax.ask = Some(mv),
        _ => {}
    }
}

fn apply_move(app: &mut App, mv: PendingMove) {
    let ramp = ramp_ticks(app);
    let trails = app.logic_settings.on(K_TRAILS);
    app.checkpoint("Move Track Automation with Region");
    let Some(p) = app.project.as_mut() else { return };
    // Include trails: the automation after the region up to the next region on the track
    let mut to = mv.to;
    if trails {
        if let Some(tr) = p.tracks.get(mv.from_t) {
            let next = tr.regions.iter().map(|x| x.start).filter(|&s| s > mv.from).filter(|&s| !(mv.dest_t == mv.from_t && s == mv.dest)).min();
            let last = tr.automation.iter().flat_map(|l| l.points.iter().map(|p| p.0)).filter(|&x| x > mv.to && next.is_none_or(|n| x < n)).max();
            if let Some(l) = last {
                to = l;
            }
        }
    }
    let lanes: Vec<AutoLane> = p.tracks.get(mv.from_t).map_or(Vec::new(), |tr| tr.automation.clone());
    for l in lanes {
        if l.points.is_empty() {
            continue;
        }
        let k = Key::of(&l);
        if mv.dest_t == mv.from_t && !mv.copy {
            if let Some(x) = p.tracks[mv.from_t].automation.iter_mut().find(|x| automation::matches(x, k)) {
                lg_core::automation::move_span(x, mv.from, to, mv.dest, ramp);
            }
            continue;
        }
        let part = if mv.copy {
            let mut c = l.clone();
            lg_core::automation::take_span(&mut c, mv.from, to)
        } else {
            let x = p.tracks[mv.from_t].automation.iter_mut().find(|x| automation::matches(x, k)).unwrap();
            lg_core::automation::take_span(x, mv.from, to)
        };
        let Some(dt) = p.tracks.get_mut(mv.dest_t) else { continue };
        if !dt.automation.iter().any(|x| automation::matches(x, k)) {
            dt.automation.push(AutoLane { param: k.param, plugin: k.plugin, relative: k.rel, ..Default::default() });
        }
        let x = dt.automation.iter_mut().find(|x| automation::matches(x, k)).unwrap();
        // (a copy of the part at the destination, the ramp back after it)
        let back = lg_core::automation::value_at(x, (mv.dest + (to - mv.from) + ramp) as f64);
        lg_core::automation::put_span(x, &part, mv.dest);
        if let Some(v) = back {
            x.points.push((mv.dest + (to - mv.from) + ramp, v.round() as u8));
            lg_core::automation::tidy(x);
        }
    }
    app.after_edit();
}

// ---- subtracks ---------------------------------------------------------------------------------

/// The height the track's open subtracks add to its row.
pub fn sub_h(app: &App, t: usize) -> f32 {
    if !app.show_automation {
        return 0.0;
    }
    app.ax.subs.get(&t).filter(|s| s.open).map_or(0.0, |s| s.lanes.len() as f32 * SUB_H)
}

/// The parameters with automation on a track that its main lane doesn't show.
fn used_not_shown(app: &App, t: usize) -> Vec<Key> {
    let main = automation::shown(app, t);
    let mut v: Vec<Key> = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(Vec::new(), |tr| tr.automation.iter().filter(|l| !l.points.is_empty()).map(Key::of).collect());
    v.retain(|k| *k != main);
    v.dedup();
    v
}

/// A lane added under the track: a parameter with automation not shown yet, else the next of
/// Volume, Pan, Mute, Solo.
pub fn add_sub(app: &mut App, t: usize, at: Option<usize>) {
    let main = automation::shown(app, t);
    let have: Vec<Key> = app.ax.subs.get(&t).map_or(Vec::new(), |s| s.lanes.clone());
    let next = used_not_shown(app, t).into_iter().find(|k| !have.contains(k)).or_else(|| [VOLUME, PAN, automation::MUTE, automation::SOLO].into_iter().map(Key::channel).find(|k| *k != main && !have.contains(k))).unwrap_or(Key::channel(VOLUME));
    let s = app.ax.subs.entry(t).or_default();
    s.open = true;
    let i = at.map_or(s.lanes.len(), |i| (i + 1).min(s.lanes.len()));
    s.lanes.insert(i, next);
    app.show_automation = true;
}

/// Reset Automation Subtracks: a lane for each parameter with automation.
pub fn reset_subs(app: &mut App, t: usize) {
    let lanes = used_not_shown(app, t);
    app.ax.subs.insert(t, Subs { open: true, lanes });
    app.show_automation = true;
}

/// The track header's automation extras (automation shown): the disclosure triangle at its
/// lower left (subtracks shown / hidden; Option-click: reset to the lanes with automation) and,
/// on hover, the Plus button that adds a lane.
pub fn header_extras(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize) {
    let p = ui.painter().clone();
    let open = app.ax.subs.get(&t).is_some_and(|s| s.open && !s.lanes.is_empty());
    let dr = Rect::from_min_size(pos2(row.min.x + 2.0, row.max.y - 15.0), vec2(14.0, 13.0));
    let resp = ui.interact(dr, ui.scope_id().with(("autosubs", t)), Sense::click()).on_hover_text("Show / hide automation subtracks (Option: lanes with automation)");
    crate::glyphs::paint(&p, dr, if open { crate::glyphs::ph_fill::CARET_DOWN } else { crate::glyphs::ph_fill::CARET_RIGHT }, 10.0, look::text_dim());
    if resp.clicked() {
        if ui.input(|i| i.modifiers.alt) {
            reset_subs(app, t);
        } else {
            toggle_subs(app, t);
        }
    }
    if ui.rect_contains_pointer(row) {
        let pr = Rect::from_min_size(pos2(row.max.x - 18.0, row.max.y - 16.0), vec2(15.0, 14.0));
        let presp = ui.interact(pr, ui.scope_id().with(("autosubadd", t)), Sense::click()).on_hover_text("Add an automation lane");
        p.rect_filled(pr, 3.0, look::group_bg());
        crate::glyphs::paint(&p, pr, crate::glyphs::ph::PLUS, 10.0, look::text());
        if presp.clicked() {
            add_sub(app, t, None);
        }
    }
}

fn toggle_subs(app: &mut App, t: usize) {
    let empty = app.ax.subs.get(&t).is_none_or(|s| s.lanes.is_empty());
    if empty {
        reset_subs(app, t);
        if app.ax.subs.get(&t).is_some_and(|s| s.lanes.is_empty()) {
            add_sub(app, t, None);
        }
        return;
    }
    if let Some(s) = app.ax.subs.get_mut(&t) {
        s.open = !s.open;
    }
}

/// The subtracks under a track's row: each lane's header (its parameter pop-up; on hover Plus
/// and X: add a lane below, remove this one — its automation stays; drag the header's empty
/// part up or down: reorder) and the lane itself.
#[allow(clippy::too_many_arguments)]
pub fn subtracks(app: &mut App, ui: &mut egui::Ui, headers_x: (f32, f32), lanes: Rect, y0: f32, t: usize, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let Some(s) = app.ax.subs.get(&t).filter(|s| s.open).cloned() else { return };
    if !app.show_automation {
        return;
    }
    let mut remove = None;
    let mut add = None;
    for (i, key) in s.lanes.iter().enumerate() {
        let y = y0 + i as f32 * SUB_H;
        // (only what's inside the tracks' area, scrolled)
        if y + SUB_H < lanes.min.y || y > lanes.max.y {
            continue;
        }
        let ys = (y.max(lanes.min.y))..=((y + SUB_H).min(lanes.max.y));
        let hr = Rect::from_x_y_ranges(headers_x.0..=headers_x.1, ys.clone());
        let lr = Rect::from_x_y_ranges(lanes.x_range(), ys);
        let p = ui.painter_at(hr);
        p.rect_filled(hr, 0.0, look::track_header_bg().gamma_multiply(0.85));
        p.hline(hr.x_range(), hr.max.y, Stroke::new(1.0, look::line()));
        // the empty part: drag to reorder
        let dresp = ui.interact(hr, ui.scope_id().with(("autosubhdr", t, i)), Sense::drag());
        if dresp.drag_started() {
            app.ax.sub_drag = Some((t, i, 0.0));
        }
        if let (true, Some((dt, di, _))) = (dresp.dragged(), app.ax.sub_drag) {
            if dt == t && di == i {
                let dy = dresp.total_drag_delta().map_or(0.0, |d| d.y);
                let to = (i as f32 + (dy / SUB_H).round()).clamp(0.0, s.lanes.len() as f32 - 1.0) as usize;
                if to != i {
                    if let Some(x) = app.ax.subs.get_mut(&t) {
                        let k = x.lanes.remove(i);
                        x.lanes.insert(to, k);
                    }
                    app.ax.sub_drag = None;
                }
            }
        }
        if dresp.drag_stopped() {
            app.ax.sub_drag = None;
        }
        let pr = Rect::from_min_size(pos2(hr.min.x + 24.0, y + 8.0), vec2((hr.width() - 70.0).max(60.0), 18.0));
        let presp = ui.interact(pr, ui.scope_id().with(("autosubparam", t, i)), Sense::click());
        p.rect_filled(pr, 4.0, look::group_bg());
        let name = automation::key_name(app, t, *key);
        p.with_clip_rect(pr).text(pos2(pr.min.x + 6.0, pr.center().y), Align2::LEFT_CENTER, format!("{name} ⌄"), look::regular(10.5), look::text());
        let mut pick = None;
        egui::Popup::menu(&presp).show(|ui| automation::param_menu(app, ui, t, *key, &mut |_, k| pick = Some(k)));
        if let Some(k) = pick {
            if let Some(x) = app.ax.subs.get_mut(&t) {
                x.lanes[i] = k;
            }
        }
        if ui.rect_contains_pointer(hr) {
            let ar = Rect::from_min_size(pos2(hr.max.x - 40.0, y + 9.0), vec2(16.0, 16.0));
            let xr = Rect::from_min_size(pos2(hr.max.x - 20.0, y + 9.0), vec2(16.0, 16.0));
            let aresp = ui.interact(ar, ui.scope_id().with(("autosubplus", t, i)), Sense::click()).on_hover_text("Add an automation lane");
            let xresp = ui.interact(xr, ui.scope_id().with(("autosubx", t, i)), Sense::click()).on_hover_text("Remove this lane (its automation stays)");
            crate::glyphs::paint(&p, ar, crate::glyphs::ph::PLUS, 11.0, look::text());
            crate::glyphs::paint(&p, xr, crate::glyphs::ph::X, 11.0, look::text());
            if aresp.clicked() {
                add = Some(i);
            }
            if xresp.clicked() {
                remove = Some(i);
            }
        }
        automation::lane_key(app, ui, lr, t, *key, x_of, tick_of);
    }
    if let Some(i) = add {
        add_sub(app, t, Some(i));
    }
    if let Some(i) = remove {
        if let Some(x) = app.ax.subs.get_mut(&t) {
            x.lanes.remove(i);
        }
    }
}

// ---- shortcut menu -----------------------------------------------------------------------------

/// The automation lane's shortcut menu (Control-click): the parameter, Move Track Automation
/// with Regions, trails, Snap Automation, Consolidate, the Event List, Automation Settings.
pub fn lane_menu(app: &mut App, resp: &egui::Response, t: usize, key: Key) {
    resp.context_menu(|ui| {
        ui.set_min_width(280.0);
        ui.menu_button("Automation Parameter", |ui| {
            if ui.button("Display off").clicked() {
                app.show_automation = false;
                ui.close();
            }
            if ui.button("Cycle Through").clicked() {
                let ctx = ui.ctx().clone();
                app.key_command(&ctx, "Automation: Cycle Through Used Parameters");
                ui.close();
            }
            ui.separator();
            automation::param_menu(app, ui, t, key, &mut |app, k| automation::show_key(app, t, k));
        });
        ui.separator();
        let m = move_mode(app);
        for (v, label) in [("Never", "Never move Track Automation with Regions"), ("Always", "Always move Track Automation with Regions"), ("Ask", "Ask to move Track Automation with Regions")] {
            if ui.add(egui::Button::selectable(m == v, label)).clicked() {
                set(app, K_MOVE, v);
                ui.close();
            }
        }
        let trails = app.logic_settings.on(K_TRAILS);
        if ui.add(egui::Button::selectable(trails, "Include Track Automation Region Trails, if possible")).clicked() {
            set(app, K_TRAILS, if trails { "0" } else { "1" });
            ui.close();
        }
        ui.separator();
        if ui.add(egui::Button::selectable(app.ax.snap_on, "Snap Automation")).clicked() {
            app.ax.snap_on = !app.ax.snap_on;
            ui.close();
        }
        let has_rel = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.automation.iter().any(|l| l.relative && automation::matches(l, Key { rel: true, ..key }) && !l.points.is_empty()));
        if has_rel && ui.button(format!("Consolidate Relative and Absolute {} Automation", automation::key_name(app, t, key.abs()))).clicked() {
            consolidate(app, t, key.abs());
            ui.close();
        }
        if ui.button("Track Automation Event List…").clicked() {
            app.ax.event_list = true;
            ui.close();
        }
        if ui.button("Automation Settings…").clicked() {
            let ctx = ui.ctx().clone();
            app.command(&ctx, "Automation…");
            ui.close();
        }
    });
}

/// Consolidate Relative and Absolute: the two curves as one absolute curve (plays the same).
pub fn consolidate(app: &mut App, t: usize, k: Key) {
    let fallback = automation::current_value(app, t, k);
    app.checkpoint("Consolidate Relative and Absolute Automation");
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    let Some(ri) = tr.automation.iter().position(|l| automation::matches(l, Key { rel: true, ..k })) else { return };
    let rel = tr.automation.remove(ri);
    let abs = tr.automation.iter().position(|l| automation::matches(l, k));
    let merged = lg_core::automation::consolidate(abs.map(|i| &tr.automation[i]), &rel, fallback);
    match abs {
        Some(i) => tr.automation[i] = merged,
        None => tr.automation.push(merged),
    }
    app.ax.rel_edit.remove(&t);
    app.after_edit();
}

// ---- menus, commands -----------------------------------------------------------------------------

/// Submenus whose items share names with others: Mix ▸ Move Track Automation with Regions, the
/// Snap menu's Snap Automation.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Move Track Automation with Regions" if !it.children.is_empty() => {
            ui.menu_button("Move Track Automation with Regions", |ui| {
                let m = move_mode(app);
                for v in ["Never", "Always", "Ask"] {
                    if ui.add(egui::Button::selectable(m == v, v)).clicked() {
                        set(app, K_MOVE, v);
                        ui.close();
                    }
                }
            });
            true
        }
        "Snap Automation" if !it.children.is_empty() => {
            ui.menu_button("Snap Automation", |ui| {
                if ui.add(egui::Button::selectable(app.ax.snap_on, "Active")).clicked() {
                    app.ax.snap_on = !app.ax.snap_on;
                    ui.close();
                }
                ui.separator();
                for (i, (short, _, _)) in SNAPS.iter().enumerate() {
                    if i == 8 {
                        ui.separator();
                    }
                    // (choosing a value turns snapping on)
                    if ui.add(egui::Button::selectable(app.ax.snap == i, *short)).clicked() {
                        app.ax.snap = i;
                        app.ax.snap_on = true;
                        ui.close();
                    }
                }
            });
            true
        }
        _ => false,
    }
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Enable Automation Quick Access" | "Toggle Automation Quick Access" => aqa_on(app),
        "Autoselect Automation Parameter in Read Mode" | "Toggle Autoselect Automation Parameter in Read Mode" => app.ax.autoselect,
        "Snap Automation" | "Toggle Snap Automation" => app.ax.snap_on,
        "Never move Track Automation with Regions" => move_mode(app) == "Never",
        "Always move Track Automation with Regions" => move_mode(app) == "Always",
        "Ask to move Track Automation with Regions" => move_mode(app) == "Ask",
        "Include Track Automation Region Trails, if possible" => app.logic_settings.on(K_TRAILS),
        _ => return None,
    })
}

fn sel_tracks(app: &App) -> Vec<usize> {
    let mut v = crate::trackarea::selection(app);
    if v.is_empty() {
        v.extend(app.selected_track);
    }
    v
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if let Some(c) = region_command_cc(name) {
        // the region lanes of the tracks with selected regions (or the selected track) show it
        let tracks = convert_tracks(app);
        for t in tracks {
            app.ax.region_cc.insert(t, c);
            app.ax.region_view.insert(t);
        }
        app.show_automation = true;
        return true;
    }
    if let Some(m) = name.strip_prefix("Snap Automation Mode: ") {
        if let Some(i) = SNAPS.iter().position(|s| s.1 == m) {
            app.ax.snap = i;
            app.ax.snap_on = true;
        }
        return true;
    }
    if !COMMANDS.contains(&name) {
        return copy_paste(app, name);
    }
    match name {
        "Convert Visible Track Automation to Region Automation" | "Move Visible Track Automation to Region" | "Move Current Track Data to Region" => return track_to_region(app, false),
        "Convert All Track Automation to Region Automation" | "Move All Track Automation to Region" | "Move All Track Data to Region" => return track_to_region(app, true),
        "Convert Visible Region Automation to Track Automation" | "Move Visible Region Data to Track Automation" | "Move Current Region Data to Track" => return region_to_track(app, false),
        "Convert All Region Automation to Track Automation" | "Move All Region Data to Track Automation" | "Move All Region Data to Track" => return region_to_track(app, true),
        "Delete All Region Automation" => {
            let sel = app.region_selection();
            if sel.is_empty() {
                return false;
            }
            app.checkpoint("Delete All Region Automation");
            for (t, r) in sel {
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    rg.ext.automation.clear();
                }
            }
            app.after_edit();
        }
        "Enable Automation Quick Access" | "Toggle Automation Quick Access" => {
            let on = !aqa_on(app);
            set(app, K_AQA, if on { "On" } else { "Off" });
            // (turned on without a controller: learn one)
            if on && get(app, K_AQA_CC).is_empty() {
                app.ax.aqa_learn = true;
                app.tell("Automation Quick Access: move a controller to assign it");
            }
        }
        "Autoselect Automation Parameter in Read Mode" | "Toggle Autoselect Automation Parameter in Read Mode" => app.ax.autoselect = !app.ax.autoselect,
        "Automation Settings…" => app.command(ctx, "Automation…"),
        "Set Selected Tracks or Channel Strips to Automation Read" => {
            for t in sel_tracks(app) {
                app.auto_mode.insert(t, AutoMode::Read);
            }
            app.load_engine();
        }
        "Toggle Selected Tracks or Channel Strips Automation Touch/Read" | "Toggle Selected Tracks or Channel Strips Automation Write/Read" => {
            let other = if name.contains("Touch") { AutoMode::Touch } else { AutoMode::Write };
            for t in sel_tracks(app) {
                let cur = automation::mode(app, t);
                app.auto_mode.insert(t, if cur == other { AutoMode::Read } else { other });
            }
            app.load_engine();
        }
        "Toggle Selected Tracks or Channel Strips Automation Trim Mode" | "Toggle Selected Tracks or Channel Strips Automation Write Relative Mode" => {
            let trim = name.contains("Trim");
            for t in sel_tracks(app) {
                let (s, o) = if trim { (&mut app.ax.trim, &mut app.ax.relative) } else { (&mut app.ax.relative, &mut app.ax.trim) };
                if !s.insert(t) {
                    s.remove(&t);
                }
                o.remove(&t);
            }
        }
        "Toggle Selected Tracks or Channel Strips Record Region Automation Mode" => {
            for t in sel_tracks(app) {
                let on = region_rec(app, t);
                app.ax.region_rec.insert(t, !on);
            }
        }
        "Set All Tracks to Automation Write" => {
            for t in 0..app.project.as_ref().map_or(0, |p| p.tracks.len()) {
                app.auto_mode.insert(t, AutoMode::Write);
            }
            app.load_engine();
        }
        "Track Automation Event List…" => app.ax.event_list = true,
        n if n.starts_with("Toggle Writing ") => {
            let what = match n {
                x if x.contains("Volume") => "Volume",
                x if x.contains("Pan ") => "Pan",
                x if x.contains("Mute") => "Mute",
                x if x.contains("Send") => "Send",
                x if x.contains("Plug-in") => "Plug-in",
                _ => "Solo",
            };
            let k = format!("Automation /  / {what}");
            let on = app.logic_settings.on(&k);
            set(app, &k, if on { "0" } else { "1" });
            app.tell(format!("Write mode writes {what} automation: {}", if on { "off" } else { "on" }));
        }
        "Set Move Track Automation with Regions to Never" | "Never move Track Automation with Regions" => set(app, K_MOVE, "Never"),
        "Set Move Track Automation with Regions to Always" | "Always move Track Automation with Regions" => set(app, K_MOVE, "Always"),
        "Set Move Track Automation with Regions to Ask" | "Ask to move Track Automation with Regions" => set(app, K_MOVE, "Ask"),
        "Include Track Automation Region Trails, if possible" => {
            let on = app.logic_settings.on(K_TRAILS);
            set(app, K_TRAILS, if on { "0" } else { "1" });
        }
        "Snap Automation" | "Toggle Snap Automation" => app.ax.snap_on = !app.ax.snap_on,
        "Automation Snap Offset…" => app.ax.offset_dialog = Some(offset(app).to_string()),
        "Automation: Toggle Track/Region" | "Show Region Automation" => {
            let Some(t) = app.selected_track else { return false };
            if name == "Show Region Automation" {
                app.ax.region_view.insert(t);
            } else {
                toggle_region_view(app, t);
            }
            app.show_automation = true;
        }
        "Toggle Automation Subtracks" | "Hide Automation Subtracks" | "Show Automation Subtracks" | "Reset Automation Subtracks" => {
            for t in sel_tracks(app) {
                match name {
                    "Toggle Automation Subtracks" => toggle_subs(app, t),
                    "Hide Automation Subtracks" => {
                        if let Some(s) = app.ax.subs.get_mut(&t) {
                            s.open = false;
                        }
                    }
                    "Show Automation Subtracks" => {
                        let s = app.ax.subs.entry(t).or_default();
                        s.open = true;
                        if s.lanes.is_empty() {
                            reset_subs(app, t);
                        }
                    }
                    _ => reset_subs(app, t),
                }
            }
            app.show_automation = true;
        }
        _ => return false,
    }
    true
}

/// Copy / Paste with automation points selected: the points (Copy), pasted at the playhead on
/// the selected track's shown lane; they replace the points in the pasted span.
fn copy_paste(app: &mut App, name: &str) -> bool {
    match name {
        "Copy" | "Cut" => {
            let Some((t, which)) = app.ta.auto.sel.clone().filter(|_| app.show_automation) else {
                app.ax.clip = None;
                return false;
            };
            let key = automation::sel_key(app, t);
            let Some(l) = automation::lane_of(app, t, key) else { return false };
            let pts: Vec<(u32, u8)> = which.iter().filter_map(|&k| l.points.get(k).copied()).collect();
            let Some(first) = pts.iter().map(|p| p.0).min() else { return false };
            app.ax.clip = Some((key, pts.iter().map(|p| (p.0 - first, p.1)).collect()));
            if name == "Cut" {
                automation::delete_selected(app);
            }
            true
        }
        "Paste" => {
            let (Some((_, pts)), Some(t), true) = (app.ax.clip.clone(), app.selected_track, app.show_automation) else { return false };
            let key = automation::shown(app, t);
            let at = app.position() as u32;
            app.checkpoint("Paste Automation");
            if let Some(l) = automation::lane_mut_key(app, t, key) {
                lg_core::automation::paste(l, at, &pts);
            }
            app.after_edit();
            true
        }
        _ => false,
    }
}

// ---- MIDI: Automation Quick Access ---------------------------------------------------------------

/// A controller from the MIDI inputs: learned for Automation Quick Access, or (Quick Access on)
/// moving the selected track's shown parameter. True: used.
pub fn midi_cc(app: &mut App, cc: u8, value: u8) -> bool {
    if app.ax.aqa_learn {
        app.ax.aqa_learn = false;
        set(app, K_AQA_CC, &cc.to_string());
        set(app, K_AQA, "On");
        app.tell(format!("Automation Quick Access: controller {cc}"));
        return true;
    }
    if !aqa_on(app) || get(app, K_AQA_CC).parse::<u8>().ok() != Some(cc) {
        return false;
    }
    let Some(t) = app.selected_track else { return false };
    let key = automation::shown(app, t);
    let v = value as f32;
    if key.plugin.is_none() && !key.rel {
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            match key.param {
                VOLUME => tr.volume_db = automation::logic_to_db(v),
                PAN => tr.pan = (v - 64.0) / 63.0,
                _ => {}
            }
        }
        app.sync_mix();
    }
    automation::record_key(app, t, key, v);
    true
}

/// Settings ▸ Automation's buttons (Learn Message: the next controller moved is Quick
/// Access's).
pub fn button(app: &mut App, key: &str) -> bool {
    match key {
        "Automation /  / Learn Message" => {
            app.ax.aqa_learn = true;
            app.tell("Move a controller through its range to assign it to Automation Quick Access");
            true
        }
        _ => false,
    }
}

/// A screen control moved by hand (smartcontrols.rs): written as its own lane while
/// recording automation.
pub fn screen_control_moved(app: &mut App, v: crate::smartcontrols::View, ci: usize, value: f32) {
    if app.ax.replaying {
        return;
    }
    if let crate::smartcontrols::View::Track(t) = v {
        automation::record_key(app, t, Key::plugin(SC_SLOT, ci as u32), value * 127.0);
    }
}

// ---- each frame ----------------------------------------------------------------------------------

/// Each frame: the Write pass's end (mode change, Trim folded in), screen control and MIDI
/// lanes played, the Event List, the Ask and offset dialogs.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let playing = app.playing();
    if app.ax.was_playing && !playing {
        end_pass(app);
    }
    app.ax.was_playing = playing;
    if playing {
        play_lanes(app);
    }
    event_list(app, ctx);
    dialogs(app, ctx);
}

/// A pass ended: Write tracks switch to the mode Settings ▸ Automation chooses (Touch or
/// Latch); Trim's offsets are folded into the absolute curves.
fn end_pass(app: &mut App) {
    let wrote: Vec<usize> = app.ax.wrote.drain().collect();
    let to = if get(app, K_WRITE_TO) == "Latch" { AutoMode::Latch } else { AutoMode::Touch };
    for t in wrote {
        if automation::mode(app, t) == AutoMode::Write {
            app.auto_mode.insert(t, to);
        }
        if app.ax.trim.contains(&t) {
            for p in [VOLUME, PAN] {
                let has = automation::lane_of(app, t, Key { rel: true, ..Key::channel(p) }).is_some_and(|l| !l.points.is_empty());
                if has {
                    consolidate(app, t, Key::channel(p));
                }
            }
        }
    }
}

/// Screen control and MIDI lanes while playing: their values at the playhead sent when they
/// change.
fn play_lanes(app: &mut App) {
    let tick = app.position();
    let Some(p) = app.project.as_ref() else { return };
    let mut out: Vec<(usize, u32, u32, u8)> = Vec::new();
    for (t, tr) in p.tracks.iter().enumerate() {
        if matches!(automation::mode(app, t), AutoMode::Off) {
            continue;
        }
        for l in tr.automation.iter().filter(|l| !l.muted && !l.points.is_empty()) {
            let Some((slot, id)) = l.plugin.filter(|x| x.0 == SC_SLOT || x.0 == MIDI_SLOT) else { continue };
            if let Some(v) = lg_core::automation::value_at(l, tick) {
                out.push((t, slot, id, v.round() as u8));
            }
        }
    }
    for (t, slot, id, v) in out {
        if app.ax.sent.get(&(t, slot, id)) == Some(&v) {
            continue;
        }
        app.ax.sent.insert((t, slot, id), v);
        if slot == SC_SLOT {
            app.ax.replaying = true;
            crate::smartcontrols::move_control(app, crate::smartcontrols::View::Track(t), id as usize, v as f32 / 127.0);
            app.ax.replaying = false;
        } else if let Ok(mut e) = app.engine.lock() {
            match id {
                PITCH_BEND => e.live_control(t, 128, (v as u16) << 7),
                AFTERTOUCH => e.live_message(t, 0xD0, v, 0),
                c => e.live_control(t, c as u8, v as u16),
            }
        }
    }
}

/// Track Automation Event List (⌃⌘E): the selected track's recorded automation, one row per
/// point: position, length (to the lane's next point), value and parameter, all editable.
fn event_list(app: &mut App, ctx: &egui::Context) {
    if !app.ax.event_list {
        return;
    }
    let mut open = true;
    let t = app.selected_track;
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    egui::Window::new("Automation Event List").open(&mut open).default_size(vec2(460.0, 360.0)).show(ctx, |ui| {
        let Some(t) = t else {
            ui.label("Select a track to list its automation.");
            return;
        };
        let lanes: Vec<AutoLane> = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(Vec::new(), |tr| tr.automation.clone());
        let mut rows: Vec<(usize, usize, u32, u32, u8)> = Vec::new();
        for (li, l) in lanes.iter().enumerate() {
            for (k, &(tk, v)) in l.points.iter().enumerate() {
                let len = l.points.get(k + 1).map_or(0, |n| n.0 - tk);
                rows.push((li, k, tk, len, v));
            }
        }
        rows.sort_by_key(|r| (r.2, r.0));
        if rows.is_empty() {
            ui.label("This track has no automation.");
            return;
        }
        let mut change: Option<(usize, usize, Option<u32>, Option<u32>, Option<u8>)> = None;
        let mut delete: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("autoevents").striped(true).num_columns(5).show(ui, |ui| {
                for h in ["Position", "Length", "Value", "Parameter", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for &(li, k, tk, len, v) in &rows {
                    let key = Key::of(&lanes[li]);
                    let mut pos = tk as i64;
                    let r = ui.add(egui::DragValue::new(&mut pos).range(0..=i64::MAX).custom_formatter(|x, _| crate::app::bbdt(x as u32, sig)).speed(10.0));
                    if r.drag_started() || r.gained_focus() {
                        app.checkpoint("Edit Automation Event");
                    }
                    if r.changed() {
                        change = Some((li, k, Some(pos.max(0) as u32), None, None));
                    }
                    let mut l = len as i64;
                    if len > 0 {
                        let r = ui.add(egui::DragValue::new(&mut l).range(1..=i64::MAX).custom_formatter(|x, _| crate::app::bbdt_len(x as u32, sig)).speed(10.0));
                        if r.drag_started() || r.gained_focus() {
                            app.checkpoint("Edit Automation Event");
                        }
                        if r.changed() {
                            change = Some((li, k, None, Some(l.max(1) as u32), None));
                        }
                    } else {
                        ui.label("");
                    }
                    let mut val = v as i32;
                    let r = ui.add(egui::DragValue::new(&mut val).range(0..=127));
                    if r.drag_started() || r.gained_focus() {
                        app.checkpoint("Edit Automation Event");
                    }
                    if r.changed() {
                        change = Some((li, k, None, None, Some(val as u8)));
                    }
                    ui.label(format!("{} ({})", automation::key_name(app, t, key), automation::key_label(app, t, key, val as f32)));
                    if ui.small_button("✕").on_hover_text("Delete").clicked() {
                        delete = Some((li, k));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((li, k)) = delete {
            app.checkpoint("Delete Automation Event");
            if let Some(l) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.automation.get_mut(li)) {
                l.points.remove(k);
                lg_core::automation::tidy(l);
            }
            app.after_edit();
        }
        if let Some((li, k, pos, len, val)) = change {
            if let Some(l) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.automation.get_mut(li)) {
                edit_event(l, k, pos, len, val);
            }
            app.dirty = true;
            app.load_engine();
        }
    });
    if !open {
        app.ax.event_list = false;
    }
}

/// One Event List edit on point `k`: its position (kept between its neighbours), its length
/// (the next point moves), its value.
pub fn edit_event(l: &mut AutoLane, k: usize, pos: Option<u32>, len: Option<u32>, val: Option<u8>) {
    let n = l.points.len();
    if k >= n {
        return;
    }
    if let Some(p) = pos {
        let lo = if k > 0 { l.points[k - 1].0 + 1 } else { 0 };
        let hi = if k + 1 < n { l.points[k + 1].0 - 1 } else { u32::MAX };
        let old = l.points[k].0;
        let new = p.clamp(lo, hi.max(lo));
        for c in l.curves.iter_mut().filter(|c| c.0 == old) {
            c.0 = new;
        }
        for s in l.steps.iter_mut().filter(|s| **s == old) {
            *s = new;
        }
        l.points[k].0 = new;
    }
    if let (Some(len), true) = (len, k + 1 < n) {
        let hi = if k + 2 < n { l.points[k + 2].0 - 1 } else { u32::MAX };
        let old = l.points[k + 1].0;
        let new = (l.points[k].0 + len).min(hi);
        for c in l.curves.iter_mut().filter(|c| c.0 == old) {
            c.0 = new;
        }
        for s in l.steps.iter_mut().filter(|s| **s == old) {
            *s = new;
        }
        l.points[k + 1].0 = new;
    }
    if let Some(v) = val {
        l.points[k].1 = v.min(127);
    }
    lg_core::automation::tidy(l);
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    // Move Track Automation with Regions: Ask
    if let Some(mv) = app.ax.ask.clone() {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("automove-ask")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(egui::RichText::new("Move the track automation with the region?").strong());
            ui.label("The automation under the region can move with it to its new place.");
            ui.horizontal(|ui| {
                if ui.button("Don’t Move").clicked() {
                    done = Some(false);
                }
                if ui.button("Move").clicked() {
                    done = Some(true);
                }
            });
        });
        if let Some(m) = done {
            app.ax.ask = None;
            if m {
                apply_move(app, mv);
            }
        }
    }
    // Record Automation with Regions: this track or all of its kind
    if let Some((t, on)) = app.ax.region_rec_ask {
        let kind = if is_audio(app, t) { "audio" } else { "software instrument" };
        let mut done = None;
        egui::Modal::new(egui::Id::unique("autorec-ask")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new(format!("{} recording automation with regions for:", if on { "Turn on" } else { "Turn off" })).strong());
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(0);
                }
                if ui.button("Current Track").clicked() {
                    done = Some(1);
                }
                if ui.button(format!("All {kind} tracks")).clicked() {
                    done = Some(2);
                }
            });
        });
        if let Some(d) = done {
            app.ax.region_rec_ask = None;
            match d {
                1 => {
                    app.ax.region_rec.insert(t, on);
                }
                2 => {
                    let audio = is_audio(app, t);
                    for x in 0..app.project.as_ref().map_or(0, |p| p.tracks.len()) {
                        if is_audio(app, x) == audio {
                            app.ax.region_rec.insert(x, on);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Automation Snap Offset…
    if let Some(mut text) = app.ax.offset_dialog.clone() {
        let mut done = None;
        egui::Modal::new(egui::Id::unique("autosnap-offset")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(egui::RichText::new("Automation Snap Offset").strong());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0));
                ui.label("Ticks");
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
            });
        });
        app.ax.offset_dialog = Some(text.clone());
        if let Some(ok) = done {
            app.ax.offset_dialog = None;
            if let (true, Ok(v)) = (ok, text.trim().parse::<i32>()) {
                set(app, K_OFFSET, &v.clamp(-9999, 9999).to_string());
            }
        }
    }
}

/// Region automation recording (Read mode with Record Automation with Regions, while
/// recording): the point goes into the region under the playhead. True: written there.
pub fn record_into_region(app: &mut App, t: usize, key: Key, value: f32) -> bool {
    if !app.recording() || !region_rec(app, t) {
        return false;
    }
    let tick = app.position() as u32;
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return false };
    let Some(r) = tr.regions.iter_mut().find(|r| tick >= r.start && tick < r.start + r.length) else { return false };
    let rel = tick - r.start;
    let mut pts = region_points(r, RKind::Lane(key));
    pts.retain(|p| p.0 != rel);
    pts.push((rel, value.round().clamp(0.0, 127.0) as u8));
    set_region_points(r, RKind::Lane(key), pts);
    app.dirty = true;
    true
}

/// The value line a header-trim drag offsets (the track header's value field while hovered:
/// Trim): the whole curve up or down, keeping the points' differences, stopping at the ends
/// (Option: past them). Returns the offset applied.
pub fn trim_lane(app: &mut App, t: usize, dv: f32, force: bool) -> f32 {
    let key = automation::shown(app, t);
    let only = app.ta.auto.sel.clone().filter(|s| s.0 == t && automation::sel_key(app, t) == key).map(|s| s.1);
    let Some(l) = automation::lane_mut_key(app, t, key) else { return 0.0 };
    let d = lg_core::automation::trim(l, only.as_deref(), dv, force);
    app.dirty = true;
    d
}

/// The header's value field over the automation (Logic's numerical display): the shown
/// parameter's value at the playhead; hovered it reads Trim, and dragging it up or down
/// offsets the whole curve (Option: past the ends).
pub fn trim_field(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    let key = automation::shown(app, t);
    let Some(l) = automation::lane_of(app, t, key).filter(|l| !l.points.is_empty()).cloned() else { return };
    let resp = ui.interact(r, ui.scope_id().with(("autotrim", t)), Sense::drag()).on_hover_text("Drag up or down: trim the whole curve (Option: past its ends)");
    let p = ui.painter();
    p.rect_filled(r, 3.0, look::lcd_bg());
    let v = lg_core::automation::value_at(&l, app.position()).unwrap_or(0.0);
    let text = if resp.hovered() || resp.dragged() { "Trim".to_string() } else { automation::key_label(app, t, key, v) };
    p.text(r.center(), Align2::CENTER_CENTER, text, look::regular(10.0), look::text());
    if resp.drag_started() {
        app.checkpoint("Trim Automation");
    }
    if resp.dragged() {
        let dv = -resp.drag_delta().y * 0.5;
        let force = ui.input(|i| i.modifiers.alt);
        if dv != 0.0 {
            let d = trim_lane(app, t, dv, force);
            app.help_tag = Some(vec![("Trim".into(), format!("{d:+.0}"))]);
        }
    }
    if resp.drag_stopped() {
        app.load_engine();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_list_edits_keep_order() {
        let mut l = AutoLane { param: 7, points: vec![(0, 10), (960, 20), (1920, 30)], steps: vec![960], ..Default::default() };
        edit_event(&mut l, 1, Some(5000), None, None);
        assert_eq!(l.points[1].0, 1919);
        assert_eq!(l.steps, vec![1919]);
        edit_event(&mut l, 0, None, Some(100), Some(99));
        assert_eq!(l.points, vec![(0, 99), (100, 20), (1920, 30)]);
    }

    #[test]
    fn region_command_names() {
        assert_eq!(region_command_cc("Region Automation: Modulation"), Some(1));
        assert_eq!(region_command_cc("Region Automation: Control Change 27"), Some(27));
        assert_eq!(region_command_cc("Region Automation: Pitch Bend"), Some(PITCH_BEND));
        assert_eq!(region_command_cc("Region Automation: Panorama"), Some(10));
        for c in COMMANDS.iter().filter(|c| c.starts_with("Region Automation: ")) {
            assert!(region_command_cc(c).is_some(), "{c}");
        }
    }

    #[test]
    fn region_points_round_trip() {
        let mut r = lg_core::Region { name: String::new(), start: 960, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        set_region_points(&mut r, RKind::Midi(1), vec![(0, 10), (480, 90)]);
        assert_eq!(r.ext.events.len(), 2);
        assert_eq!(region_points(&r, RKind::Midi(1)), vec![(0, 10), (480, 90)]);
        set_region_points(&mut r, RKind::Midi(PITCH_BEND), vec![(10, 64)]);
        assert_eq!(region_points(&r, RKind::Midi(PITCH_BEND)), vec![(10, 64)]);
        assert_eq!(region_points(&r, RKind::Midi(1)).len(), 2);
        let k = Key::channel(VOLUME);
        set_region_points(&mut r, RKind::Lane(k), vec![(0, 90), (100, 40)]);
        assert_eq!(region_points(&r, RKind::Lane(k)), vec![(0, 90), (100, 40)]);
    }
}
