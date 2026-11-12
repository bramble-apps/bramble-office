//! Markers as Logic's (lg-core markers): the Marker track (markers with their lengths and
//! colours; click selects, Option-click locates, drag moves by the Snap value with a help tag,
//! Option-drag copies, edges resize, a shared edge moves both markers, ⌘-click trims the end and
//! ⇧⌘-click the start, double-click renames, ⌥-double-click opens the Marker Text, the Pencil
//! adds, the Eraser deletes, the Text tool renames; Delete, ⌘C / ⌘V), markers in the ruler
//! when the Marker track is hidden, the cycle area dropped on the Marker track, the Marker List
//! (Position / Marker Name / Length editable, Options, View and Marker Set menus, the Marker
//! Text area), the Marker Text window with its Font, SMPTE locks, colours from the Colors
//! window, marker sets, and markers imported from / exported to audio files.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke};
use lg_core::events::PPQ;
use lg_core::markers::{self as mk, Marker, Rounding};
use lg_core::timeline::Map;

use crate::app::App;
use crate::edit::Tool;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Create Marker",
    "Create Marker without rounding",
    "Create Marker for Region/Marquee Selection",
    "Create Marker by Cycle Area",
    "Delete Marker",
    "Rename Marker",
    "Open Marker List",
    "Open Marker Text",
    "Lock SMPTE Position",
    "Unlock SMPTE Position",
    "Import Marker from Audio File",
    "Export Marker to Audio File",
    "Remove Marker from Audio File",
];

#[derive(Clone, Debug)]
enum MDrag {
    Move { k: usize, grab: f32, copy: bool, from: u32 },
    Start { k: usize },
    End { k: usize },
    Junction { k: usize },
    Ruler { k: usize, from_y: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Pos,
    Name,
    Len,
}

pub struct State {
    /// Selected markers (indices into `App::markers`).
    pub sel: Vec<usize>,
    /// Markers have the key focus (the last click was on one): Delete, ⌘C, ⌘V, colours.
    pub focus: bool,
    hit: bool,
    clip: Vec<Marker>,
    pub list_open: bool,
    pub text_open: bool,
    /// The Marker List's Marker Text area.
    pub text_area: bool,
    /// Marker List ▸ View: positions and lengths as time; the length as the end position.
    pub as_time: bool,
    pub len_abs: bool,
    /// The Marker Text window's font (Font window): size, monospaced.
    pub font_size: f32,
    pub font_mono: bool,
    font_open: bool,
    drag: Option<MDrag>,
    edit: Option<(usize, Field, String)>,
    set_name: Option<String>,
    /// Where markers are drawn this frame (the lane, or the ruler), for the rename field.
    shown: Vec<(usize, Rect)>,
    /// The Marker track's lane this frame (cycle drops).
    pub lane: Option<Rect>,
    /// The ruler's upper half (a marker dropped there sets the cycle to it).
    upper: Option<Rect>,
    last_map: Option<Map>,
}

impl Default for State {
    fn default() -> Self {
        State { sel: Vec::new(), focus: false, hit: false, clip: Vec::new(), list_open: false, text_open: false, text_area: false, as_time: false, len_abs: false, font_size: 13.0, font_mono: false, font_open: false, drag: None, edit: None, set_name: None, shown: Vec::new(), lane: None, upper: None, last_map: None }
    }
}

fn map(app: &App) -> Map {
    app.project.as_ref().map_or_else(|| Map::tempo(app.tempo()), Map::of)
}

/// The end markers reach to: the project's end (past the last marker).
fn end(app: &App) -> u32 {
    let bar = app.bar_ticks();
    crate::nav::project_end(app).max(app.markers.last().map_or(0, |m| m.0 + bar))
}

/// The default marker colour (Logic: grey) and the text colour on a fill (inverse on dark).
pub fn fill(m: &Marker) -> Color32 {
    match m.2.color {
        Some(c) => {
            let c = lg_core::trackmix::palette(c);
            Color32::from_rgb(c[0], c[1], c[2])
        }
        None => Color32::from_gray(0x30),
    }
}

fn ink(c: Color32) -> Color32 {
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if l < 140.0 {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

fn pos_text(app: &App, t: u32) -> String {
    let m = map(app);
    if app.mk.as_time {
        return time_text(m.secs_at(t as f64));
    }
    let (a, b, c, d) = m.bbdt(t, division(app));
    format!("{a} {b} {c} {d}")
}

fn time_text(s: f64) -> String {
    let s = s.max(0.0);
    let (h, mi) = ((s / 3600.0) as u32, ((s / 60.0) as u32) % 60);
    format!("{h:02}:{mi:02}:{:06.3}", s % 60.0)
}

fn len_text(app: &App, start: u32, len: u32) -> String {
    let m = map(app);
    if app.mk.len_abs {
        return pos_text(app, start + len);
    }
    if app.mk.as_time {
        return time_text(m.secs_at((start + len) as f64) - m.secs_at(start as f64));
    }
    let sig = m.sig_at(start);
    let (bar, beat) = (Map::bar_len(sig), Map::beat_len(sig));
    let div = division(app).min(beat).max(1);
    format!("{} {} {} {}", len / bar, len % bar / beat, len % beat / div, len % div)
}

fn division(app: &App) -> u32 {
    if app.nav.division == 0 {
        PPQ / 4
    } else {
        app.nav.division
    }
}

/// "bar beat division tick" (missing parts: 1), or a time with colons when shown as time.
fn parse_pos(app: &App, s: &str) -> Option<u32> {
    let m = map(app);
    if s.contains(':') {
        return crate::nav::parse_time(s).map(|t| m.tick_at(t).round().max(0.0) as u32);
    }
    let v: Vec<u32> = s.split(|c: char| c == ' ' || c == '.').filter(|x| !x.is_empty()).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let bar = *v.first()?;
    let start = m.bar_start(bar.max(1) - 1);
    let beat = Map::beat_len(m.sig_of_bar(bar.max(1) - 1));
    let g = |i: usize| v.get(i).copied().unwrap_or(1).max(1) - 1;
    Some(start + g(1) * beat + g(2) * division(app) + g(3))
}

fn parse_len(app: &App, start: u32, s: &str) -> Option<u32> {
    if app.mk.len_abs {
        return parse_pos(app, s).map(|e| e.saturating_sub(start));
    }
    let m = map(app);
    let sig = m.sig_at(start);
    let v: Vec<u32> = s.split(|c: char| c == ' ' || c == '.').filter(|x| !x.is_empty()).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let g = |i: usize| v.get(i).copied().unwrap_or(0);
    Some(g(0) * Map::bar_len(sig) + g(1) * Map::beat_len(sig) + g(2) * division(app) + g(3))
}

fn select_only(app: &mut App, k: usize) {
    app.mk.sel = vec![k];
    app.mk.focus = true;
}

/// A new marker at `at` (rounded), selected. None when one is within a quarter note.
fn create(app: &mut App, at: u32, r: Rounding, what: &str) -> Option<usize> {
    let at = mk::round(&map(app), at, r);
    if app.markers.iter().any(|m| m.0.abs_diff(at) < PPQ) {
        return None;
    }
    app.checkpoint(what);
    let k = mk::create(&mut app.markers, at, None)?;
    // (Settings ▸ View ▸ Tracks ▸ Marker Color: Auto-assign)
    if let Some(c) = crate::viewprefs::marker_color(app) {
        app.markers[k].2.color = Some(c);
    }
    select_only(app, k);
    Some(k)
}

fn delete(app: &mut App, ks: &[usize]) {
    if ks.is_empty() {
        return;
    }
    app.checkpoint("Delete Marker");
    let mut ks = ks.to_vec();
    ks.sort_unstable();
    ks.dedup();
    for k in ks.into_iter().rev() {
        if k < app.markers.len() {
            app.markers.remove(k);
        }
    }
    mk::renumber(&mut app.markers);
    app.mk.sel.clear();
}

/// The marker the playhead is in.
fn at_playhead(app: &App) -> Option<usize> {
    let pos = app.position() as u32;
    app.markers.iter().rposition(|m| m.0 <= pos)
}

fn targets(app: &App) -> Vec<usize> {
    if !app.mk.sel.is_empty() {
        app.mk.sel.clone()
    } else {
        at_playhead(app).into_iter().collect()
    }
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let _ = ctx;
    let pos = app.position() as u32;
    match name {
        "Create Marker" => {
            create(app, pos, Rounding::Bar, "Create Marker");
        }
        "Create Marker without Rounding" | "Create Marker without rounding" | "Create Without Rounding" => {
            create(app, pos, Rounding::None, "Create Marker");
        }
        "Create Marker by Cycle Area" => {
            let (a, b) = app.cycle;
            cycle_marker(app, a, b);
        }
        "Create Marker for Region/Marquee Selection" | "Create Marker for Region Selection" | "Create Markers From Regions" => from_regions(app),
        "Delete Marker" | "Delete Marker at Playhead Position" => {
            let ks = if name == "Delete Marker" && app.mk.focus { targets(app) } else { at_playhead(app).into_iter().collect() };
            delete(app, &ks);
        }
        "Rename Marker" => {
            if let Some(&k) = targets(app).first() {
                app.renaming_marker = Some((k, app.markers[k].1.clone()));
            }
        }
        "Open Marker List" => app.mk.list_open = true,
        "Open Marker Text" => app.mk.text_open = true,
        "Lock SMPTE Position" | "Unlock SMPTE Position" => {
            let ks = targets(app);
            if ks.is_empty() {
                return false;
            }
            app.checkpoint(name);
            let m = map(app);
            for k in ks {
                if name.starts_with("Lock") {
                    mk::lock(&mut app.markers, k, &m);
                } else if !mk::unlock(&mut app.markers, k) {
                    app.tell("Scene markers stay locked");
                }
            }
        }
        "Import Marker from Audio File" | "Export Marker to Audio File" | "Remove Marker from Audio File" => audio_file(app, name),
        // the edit commands, while markers have the focus
        "Copy" | "Cut" if app.mk.focus && !app.mk.sel.is_empty() => {
            app.mk.clip = app.mk.sel.iter().filter_map(|&k| app.markers.get(k).cloned()).collect();
            if name == "Cut" {
                let ks = app.mk.sel.clone();
                delete(app, &ks);
            }
        }
        "Paste" if app.mk.focus && !app.mk.clip.is_empty() => {
            app.checkpoint("Paste Marker");
            let clip = app.mk.clip.clone();
            app.mk.sel = mk::paste(&mut app.markers, &clip, pos);
        }
        "Delete" if app.mk.focus && !app.mk.sel.is_empty() => {
            let ks = app.mk.sel.clone();
            delete(app, &ks);
        }
        "Select All" if app.mk.focus => app.mk.sel = (0..app.markers.len()).collect(),
        _ => return false,
    }
    true
}

/// A marker matching the cycle area (or the marker under it fitted to it).
fn cycle_marker(app: &mut App, a: u32, b: u32) {
    if b <= a {
        return;
    }
    app.checkpoint("Create Marker");
    if let Some(k) = mk::create_span(&mut app.markers, a, b - a, None, None) {
        select_only(app, k);
    }
}

/// Markers from the selected regions: their names, positions, lengths and colours.
fn from_regions(app: &mut App) {
    let sel = app.region_selection();
    let mut new: Vec<(u32, u32, String, Option<u8>)> = Vec::new();
    for (t, r) in sel {
        if let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) {
            let color = crate::tm::set(app, t).and_then(|s| s.strip.color);
            new.push((rg.start, rg.length, rg.name.clone(), color));
        }
    }
    if new.is_empty() {
        return;
    }
    app.checkpoint("Create Marker for Region Selection");
    new.sort_by_key(|n| n.0);
    for (s, l, n, c) in new {
        mk::create_span(&mut app.markers, s, l, Some(&n), c);
    }
}

/// Marker ▸ colours from the Colors window: true when it went to the selected markers.
pub fn assign_color(app: &mut App, color: Option<u8>) -> bool {
    if !app.mk.focus || app.mk.sel.is_empty() {
        return false;
    }
    app.checkpoint("Marker Color");
    for &k in &app.mk.sel {
        if let Some(m) = app.markers.get_mut(k) {
            m.2.color = color;
        }
    }
    true
}

// ---- the Marker track --------------------------------------------------------------------------

const EDGE: f32 = 5.0;

/// The Marker track's lane (and the word "Marker" in its header: Create Markers From Regions).
pub fn lane(app: &mut App, ui: &mut egui::Ui, head: Rect, row: Rect, lane: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    app.mk.lane = Some(lane);
    let lp = ui.painter_at(lane);
    let bar_t = app.bar_ticks() as f32;
    let ppt = (x_of(bar_t) - x_of(0.0)) / bar_t;
    // the header's name: a click offers Create Markers From Regions
    let word = Rect::from_min_size(pos2(head.min.x + 10.0, row.min.y + 4.0), vec2(56.0, 20.0));
    let wr = ui.interact(word, ui.scope_id().with("marker-word"), Sense::click());
    egui::Popup::menu(&wr).show(|ui| {
        if ui.add_enabled(!app.region_selection().is_empty(), egui::Button::new("Create Markers From Regions")).clicked() {
            from_regions(app);
            ui.close();
        }
    });
    let (cmd, alt, shift, ctrl) = ui.input(|i| (i.modifiers.command, i.modifiers.alt, i.modifiers.shift, i.modifiers.ctrl));
    let pressed = ui.input(|i| i.pointer.any_pressed());
    let hover = ui.input(|i| i.pointer.hover_pos());
    if pressed && hover.is_some_and(|h| lane.contains(h)) {
        app.mk.hit = true;
    }
    // the empty lane: double-click adds (bar), the Pencil adds (quarter note), ⌃⌥⌘-click adds
    // and asks for the name
    let bg = ui.interact(lane, ui.scope_id().with("marker-lane"), Sense::click());
    if let Some(pp) = bg.interact_pointer_pos() {
        let t = tick_of(pp.x).max(0.0) as u32;
        if bg.double_clicked() && app.tool == Tool::Pointer {
            create(app, t, Rounding::Bar, "Create Marker");
        } else if bg.clicked() && ctrl && alt && cmd {
            if let Some(k) = create(app, t, Rounding::Quarter, "Create Marker") {
                app.renaming_marker = Some((k, app.markers[k].1.clone()));
            }
        } else if bg.clicked() && app.tool == Tool::Pencil {
            create(app, t, Rounding::Quarter, "Create Marker");
        } else if bg.clicked() {
            app.mk.sel.clear();
            app.mk.focus = true;
        }
    }
    let end_t = end(app);
    let markers = app.markers.clone();
    app.mk.shown.clear();
    for (k, m) in markers.iter().enumerate() {
        let (x0, x1) = (x_of(m.0 as f32), x_of(mk::end_of(&markers, k, end_t) as f32));
        if x1 < lane.min.x || x0 > lane.max.x {
            continue;
        }
        let r = Rect::from_min_max(pos2(x0, row.min.y + 1.0), pos2(x1.max(x0 + 4.0), row.max.y - 1.0));
        app.mk.shown.push((k, r));
        let sel = app.mk.sel.contains(&k);
        draw_marker(&lp, r, m, sel);
        // edges: resize (a shared edge: both markers, as the Junction tool)
        let left = Rect::from_min_max(pos2(r.min.x - 1.0, r.min.y), pos2(r.min.x + EDGE, r.max.y));
        let right = Rect::from_min_max(pos2(r.max.x - EDGE, r.min.y), pos2(r.max.x + 1.0, r.max.y));
        let body = ui.interact(r, ui.scope_id().with(("marker", k)), Sense::click_and_drag());
        let lr = ui.interact(left, ui.scope_id().with(("marker-l", k)), Sense::drag());
        let rr = ui.interact(right, ui.scope_id().with(("marker-r", k)), Sense::drag());
        let shared = markers.get(k + 1).is_some_and(|n| n.0 == mk::end_of(&markers, k, end_t));
        if lr.hovered() || rr.hovered() || lr.dragged() || rr.dragged() {
            ui.ctx().set_cursor_icon(if rr.hovered() && shared { CursorIcon::ResizeColumn } else { CursorIcon::ResizeHorizontal });
        }
        if lr.drag_started() {
            app.checkpoint("Resize Marker");
            app.mk.drag = Some(MDrag::Start { k });
            select_only(app, k);
        }
        if rr.drag_started() {
            app.checkpoint("Resize Marker");
            app.mk.drag = Some(if shared { MDrag::Junction { k } } else { MDrag::End { k } });
            select_only(app, k);
        }
        if body.drag_started() {
            let grab = body.interact_pointer_pos().map_or(0.0, |p| tick_of(p.x) - m.0 as f32);
            app.checkpoint(if alt { "Copy Marker" } else { "Move Marker" });
            app.mk.drag = Some(MDrag::Move { k, grab, copy: alt, from: m.0 });
            if !app.mk.sel.contains(&k) {
                select_only(app, k);
            }
        }
        if body.clicked() {
            if app.tool == Tool::Eraser {
                delete(app, &[k]);
                return;
            }
            if app.tool == Tool::Text {
                app.renaming_marker = Some((k, m.1.clone()));
            } else if let Some(pp) = body.interact_pointer_pos() {
                let at = crate::arranging::snap_tick(app, tick_of(pp.x), ppt) as u32;
                if cmd && shift {
                    app.checkpoint("Trim Marker");
                    mk::trim_start(&mut app.markers, k, at, end_t);
                } else if cmd {
                    app.checkpoint("Trim Marker");
                    mk::trim_end(&mut app.markers, k, at);
                } else if alt {
                    app.locate(m.0 as f64);
                } else if shift {
                    if !app.mk.sel.contains(&k) {
                        app.mk.sel.push(k);
                    }
                    app.mk.focus = true;
                } else {
                    select_only(app, k);
                }
            }
        }
        if body.double_clicked() {
            select_only(app, k);
            if alt {
                app.mk.text_open = true;
            } else {
                app.renaming_marker = Some((k, m.1.clone()));
            }
        }
        if body.secondary_clicked() && !app.mk.sel.contains(&k) {
            select_only(app, k);
        }
        crate::menus::context_menu(app, &body, "marker");
    }
    drag(app, ui, tick_of, ppt, end_t);
    rename_field(app, ui, Some(row));
}

fn draw_marker(p: &egui::Painter, r: Rect, m: &Marker, sel: bool) {
    let c = fill(m);
    let c = if !sel {
        c
    } else if m.2.color.is_none() {
        Color32::from_gray(0x5c)
    } else {
        c.gamma_multiply(1.3)
    };
    // multi-line markers: the colour on the title line only, the text white on grey below
    let lines: Vec<&str> = m.2.text.lines().collect();
    let line_h = 16.0;
    let title = if !lines.is_empty() && r.height() > line_h * 2.0 { Rect::from_min_size(r.min, vec2(r.width(), line_h + 4.0)) } else { r };
    if title != r {
        p.rect_filled(r, 0.0, Color32::from_gray(0x5a));
    }
    p.rect_filled(title, 0.0, c);
    // (Logic: a white line at the marker's start, a dark one after it)
    p.vline(r.min.x + 0.5, r.y_range(), Stroke::new(1.0, Color32::WHITE));
    p.vline(r.min.x + 1.5, r.y_range(), Stroke::new(1.0, Color32::from_gray(0x18)));
    let tc = ink(c);
    let cp = p.with_clip_rect(r.intersect(p.clip_rect()));
    let mut x = r.min.x + 5.0;
    if m.2.lock.is_some() {
        crate::glyphs::paint(&cp, Rect::from_center_size(pos2(x + 5.0, title.center().y), vec2(11.0, 11.0)), crate::glyphs::ph::LOCK_SIMPLE, 11.0, tc);
        x += 13.0;
    }
    cp.text(pos2(x, title.center().y), Align2::LEFT_CENTER, &m.1, look::regular(11.5), tc);
    if title != r {
        for (i, l) in lines.iter().enumerate() {
            let y = title.max.y + 2.0 + i as f32 * line_h;
            if y + line_h > r.max.y + 2.0 {
                break;
            }
            cp.text(pos2(r.min.x + 5.0, y), Align2::LEFT_TOP, *l, look::regular(11.0), Color32::WHITE);
        }
    }
}

fn drag(app: &mut App, ui: &egui::Ui, tick_of: &dyn Fn(f32) -> f32, ppt: f32, end_t: u32) {
    let Some(d) = app.mk.drag.clone() else { return };
    let Some(pp) = ui.input(|i| i.pointer.hover_pos()) else { return };
    let down = ui.input(|i| i.pointer.primary_down());
    let t = tick_of(pp.x);
    let snapped = |app: &App, t: f32| crate::arranging::snap_tick(app, t, ppt) as u32;
    let m = map(app);
    let sig = m.sig_at(0);
    match d {
        MDrag::Move { k, grab, copy, from } => {
            let at = snapped(app, t - grab);
            if app.mk.upper.is_some_and(|u| u.contains(pp)) {
                // over the upper ruler: dropped there, the cycle takes the marker's range
                if !copy {
                    let n = mk::move_to(&mut app.markers, k, from);
                    app.mk.drag = Some(MDrag::Move { k: n, grab, copy, from });
                }
                let k = app.mk.drag.as_ref().map_or(k, |d| if let MDrag::Move { k, .. } = d { *k } else { k });
                let range = (app.markers.get(k).map_or(0, |m| m.0), mk::end_of(&app.markers, k, end_t));
                if !down {
                    app.set_cycle(range);
                    if !app.cycle_on() {
                        app.toggle_cycle();
                    }
                    app.mk.drag = None;
                } else {
                    app.help_tag = Some(vec![("Set Locators".into(), String::new()), ("Left:".into(), crate::app::bbdt(range.0, sig)), ("Right:".into(), crate::app::bbdt(range.1, sig))]);
                }
                return;
            }
            if !down {
                // dropped: moved, or copied (Option)
                if copy && at != from {
                    if let Some(n) = mk::copy_to(&mut app.markers, k, at) {
                        select_only(app, n);
                    }
                }
                app.mk.drag = None;
                return;
            }
            if !copy {
                let n = mk::move_to(&mut app.markers, k, at);
                app.mk.drag = Some(MDrag::Move { k: n, grab, copy, from });
                app.mk.sel = vec![n];
            }
            app.help_tag = Some(vec![(if copy { "Copy Marker".into() } else { "Move Marker".into() }, String::new()), ("Position:".into(), crate::app::bbdt(at, sig))]);
        }
        MDrag::Start { k } | MDrag::End { k } | MDrag::Junction { k } => {
            if !down {
                app.mk.drag = None;
                return;
            }
            let at = snapped(app, t);
            let (s, e) = (app.markers.get(k).map_or(0, |x| x.0), mk::end_of(&app.markers, k, end_t));
            match d {
                MDrag::Start { .. } => mk::set_range(&mut app.markers, k, at, e),
                MDrag::End { .. } => mk::set_range(&mut app.markers, k, s, at),
                _ => mk::junction(&mut app.markers, k, at, end_t),
            }
            let (s, e) = (app.markers.get(k).map_or(0, |x| x.0), mk::end_of(&app.markers, k, end_t));
            app.help_tag = Some(vec![("Resize Marker".into(), String::new()), ("Position:".into(), crate::app::bbdt(s, sig)), ("Length:".into(), crate::app::bbdt_len(e - s, sig))]);
        }
        MDrag::Ruler { k, from_y } => {
            if !down {
                // dragged down out of the ruler: deleted
                if pp.y > from_y + 30.0 {
                    delete(app, &[k]);
                }
                app.mk.drag = None;
                return;
            }
            let to = snapped(app, t);
            let n = mk::move_to(&mut app.markers, k, to);
            app.mk.drag = Some(MDrag::Ruler { k: n, from_y });
            let at = app.markers.get(n).map_or(0, |m| m.0);
            app.help_tag = Some(vec![(if pp.y > from_y + 30.0 { "Delete Marker".into() } else { "Move Marker".into() }, String::new()), ("Position:".into(), crate::app::bbdt(at, sig))]);
        }
    }
}

/// The name field of the marker being renamed (where it is drawn, else at the top of `near`).
fn rename_field(app: &mut App, ui: &mut egui::Ui, near: Option<Rect>) {
    let Some((k, mut text)) = app.renaming_marker.clone() else { return };
    if k >= app.markers.len() {
        app.renaming_marker = None;
        return;
    }
    let at = app.mk.shown.iter().find(|s| s.0 == k).map(|s| s.1.min).or(near.map(|r| r.min));
    let Some(at) = at else { return };
    let er = Rect::from_min_size(at + vec2(0.0, 2.0), vec2(170.0, 22.0));
    let te = ui.put(er, egui::TextEdit::singleline(&mut text).font(look::regular(11.5)));
    crate::widgets::focus_once(ui, &te);
    if te.lost_focus() {
        app.renaming_marker = None;
        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && app.markers[k].1 != text {
            app.checkpoint("Rename Marker");
            app.markers[k].1 = text;
        }
    } else {
        app.renaming_marker = Some((k, text));
    }
}

/// The Marker track's (+): a marker at the playhead, rounded to the quarter note.
pub fn add_button(app: &mut App) {
    let pos = app.position() as u32;
    create(app, pos, Rounding::Quarter, "Create Marker");
}

/// The cycle area dropped on the Marker track (from the ruler): fitted to the marker there, else
/// a new marker matching it.
pub fn cycle_dropped(app: &mut App, at: Pos2, tick: f32) -> bool {
    let Some(lane) = app.mk.lane else { return false };
    if !lane.contains(at) {
        return false;
    }
    let (a, b) = app.cycle;
    let t = tick.max(0.0) as u32;
    let end_t = end(app);
    if let Some(k) = (0..app.markers.len()).find(|&k| app.markers[k].0 <= t && t < mk::end_of(&app.markers, k, end_t)) {
        app.checkpoint("Resize Marker");
        mk::set_range(&mut app.markers, k, a, b);
        select_only(app, k);
    } else {
        cycle_marker(app, a, b);
    }
    true
}

// ---- markers in the ruler (Marker track hidden) ------------------------------------------------

/// Markers drawn in the ruler when the Marker track is hidden: ⌥-click locates, ⇧-click sets the
/// cycle to the marker, ⌘-drag moves it (down out of the ruler: deletes), ⌘-double-click renames;
/// ⌃⇧⌘-double-click on the ruler creates a marker and asks for its name.
pub fn ruler(app: &mut App, ui: &mut egui::Ui, ruler: Rect, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32, shown: bool) {
    app.mk.upper = Some(Rect::from_min_max(ruler.min, pos2(ruler.max.x, ruler.center().y)));
    if !shown {
        return;
    }
    app.mk.lane = None;
    let strip = Rect::from_min_max(pos2(ruler.min.x, ruler.center().y), ruler.max);
    let p = ui.painter_at(ruler);
    let (cmd, alt, shift, ctrl) = ui.input(|i| (i.modifiers.command, i.modifiers.alt, i.modifiers.shift, i.modifiers.ctrl));
    let bar_t = app.bar_ticks() as f32;
    let ppt = (x_of(bar_t) - x_of(0.0)) / bar_t;
    let end_t = end(app);
    if ctrl && shift && cmd && ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
        if let Some(h) = ui.input(|i| i.pointer.interact_pos()).filter(|h| ruler.contains(*h)) {
            if let Some(k) = create(app, tick_of(h.x).max(0.0) as u32, Rounding::Quarter, "Create Marker") {
                app.renaming_marker = Some((k, app.markers[k].1.clone()));
            }
        }
    }
    let markers = app.markers.clone();
    app.mk.shown.clear();
    for (k, m) in markers.iter().enumerate() {
        let x0 = x_of(m.0 as f32);
        if x0 > ruler.max.x || x_of(mk::end_of(&markers, k, end_t) as f32) < ruler.min.x {
            continue;
        }
        let w = (look::regular(10.5).size * 0.6 * m.1.chars().count() as f32 + 12.0).min(160.0);
        let r = Rect::from_min_size(pos2(x0, strip.min.y + 1.0), vec2(w, strip.height() - 2.0));
        app.mk.shown.push((k, r));
        let c = fill(m);
        p.rect_filled(r, 2.0, if app.mk.sel.contains(&k) { c.gamma_multiply(1.35) } else { c });
        p.with_clip_rect(r).text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, &m.1, look::regular(10.5), ink(c));
        let resp = ui.interact(r, ui.scope_id().with(("ruler-marker", k)), Sense::click_and_drag());
        if resp.is_pointer_button_down_on() {
            app.mk.hit = true;
        }
        if resp.double_clicked() && cmd {
            app.renaming_marker = Some((k, m.1.clone()));
        } else if resp.clicked() {
            if shift {
                let e = mk::end_of(&markers, k, end_t);
                app.set_cycle((m.0, e));
                if !app.cycle_on() {
                    app.toggle_cycle();
                }
            } else {
                // (⌥-click or a click: the playhead to the marker)
                let _ = alt;
                app.locate(m.0 as f64);
                select_only(app, k);
            }
        }
        if resp.drag_started() && cmd {
            app.checkpoint("Move Marker");
            app.mk.drag = Some(MDrag::Ruler { k, from_y: ruler.max.y });
            select_only(app, k);
        }
    }
    drag(app, ui, tick_of, ppt, end_t);
    rename_field(app, ui, Some(ruler));
}

// ---- per frame: SMPTE locks, the windows, the focus --------------------------------------------

pub fn frame(app: &mut App, ctx: &egui::Context) {
    // locked markers keep their time when the tempo changes
    let m = map(app);
    if app.mk.last_map.as_ref() != Some(&m) {
        if app.markers.iter().any(|x| x.2.lock.is_some()) {
            mk::relock(&mut app.markers, &m);
        }
        app.mk.last_map = Some(m);
    }
    app.mk.sel.retain(|&k| k < app.markers.len());
    // Delete / Backspace: the selected markers (while they have the focus)
    if app.mk.focus && !app.mk.sel.is_empty() && !ctx.egui_wants_keyboard_input() && ctx.input(|i| (i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete)) && i.modifiers.is_none()) {
        let ks = app.mk.sel.clone();
        delete(app, &ks);
    }
    let mut hit = std::mem::take(&mut app.mk.hit);
    if app.mk.list_open {
        let mut open = true;
        let r = egui::Window::new("Marker List").id(egui::Id::unique("marker-list-win")).open(&mut open).default_size(vec2(460.0, 420.0)).show(ctx, |ui| {
            let (body, _) = ui.allocate_exact_size(vec2(460.0, 380.0), Sense::hover());
            list(app, ui, body);
        });
        app.mk.list_open = open;
        hit |= r.is_some_and(|r| ctx.input(|i| i.pointer.press_origin()).is_some_and(|p| r.response.rect.contains(p)));
    }
    if app.mk.text_open {
        let mut open = true;
        let r = egui::Window::new("Marker Text").id(egui::Id::unique("marker-text-win")).open(&mut open).default_size(vec2(360.0, 260.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Font…").on_hover_text("The Marker Text window's font").clicked() {
                    app.mk.font_open = !app.mk.font_open;
                }
            });
            ui.set_min_size(vec2(340.0, 220.0));
            text_editor(app, ui, true);
        });
        app.mk.text_open = open;
        hit |= r.is_some_and(|r| ctx.input(|i| i.pointer.press_origin()).is_some_and(|p| r.response.rect.contains(p)));
    }
    if app.mk.font_open {
        let mut open = true;
        egui::Window::new("Fonts").id(egui::Id::unique("marker-font-win")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Family:");
                ui.selectable_value(&mut app.mk.font_mono, false, "Proportional");
                ui.selectable_value(&mut app.mk.font_mono, true, "Monospaced");
            });
            ui.horizontal(|ui| {
                ui.label("Size:");
                ui.add(egui::Slider::new(&mut app.mk.font_size, 9.0..=36.0).step_by(1.0));
            });
        });
        app.mk.font_open = open;
    }
    // (a press in another window, e.g. Colors, keeps the focus)
    if ctx.input(|i| i.pointer.any_pressed()) && !hit && !kn_ui::pointer_over_window(ctx) {
        app.mk.focus = false;
    }
}

/// The text of the selected marker (else the one at the playhead): its first paragraph is the
/// name. `window`: the Marker Text window's font.
fn text_editor(app: &mut App, ui: &mut egui::Ui, window: bool) {
    let Some(k) = app.mk.sel.first().copied().or_else(|| at_playhead(app)) else {
        ui.weak("No marker");
        return;
    };
    let mut text = app.markers[k].full_text();
    let before = text.clone();
    let font = if window { egui::FontId::new(app.mk.font_size, if app.mk.font_mono { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional }) } else { look::regular(12.0) };
    let size = ui.available_size();
    let te = ui.add_sized(size, egui::TextEdit::multiline(&mut text).font(font));
    if te.has_focus() || te.clicked() {
        app.mk.hit = true;
    }
    if text != before {
        // (one undo step per editing session)
        if !app.undo.last().is_some_and(|u| u.0 == "Edit Marker Text") {
            app.checkpoint("Edit Marker Text");
        }
        app.markers[k].set_full_text(&text);
        app.dirty = true;
    }
}

// ---- the Marker List ---------------------------------------------------------------------------

const ROW: f32 = 22.0;

/// The Marker List (List Editors ▸ Marker, and its own window).
pub fn list(app: &mut App, ui: &mut egui::Ui, body: Rect) {
    if ui.input(|i| i.pointer.press_origin()).is_some_and(|p| body.contains(p)) && ui.input(|i| i.pointer.any_pressed()) {
        app.mk.hit = true;
    }
    let p = ui.painter().clone();
    // the bar: Options, View, Create (+), Marker Set, the Marker Text area button
    let bar = Rect::from_min_size(body.min, vec2(body.width(), 28.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(2.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    toolbar(app, &mut child);
    let text_w = if app.mk.text_area { (body.width() * 0.4).max(140.0) } else { 0.0 };
    let table = Rect::from_min_max(pos2(body.min.x, bar.max.y + 4.0), pos2(body.max.x - text_w, body.max.y - 22.0));
    if app.mk.text_area {
        let tr = Rect::from_min_max(pos2(table.max.x + 6.0, table.min.y), pos2(body.max.x, table.max.y));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(tr));
        text_editor(app, &mut c, false);
    }
    let w = table.width() - 16.0;
    let cols = [("Position", w * 0.34), ("Marker Name", w * 0.4), (if app.mk.len_abs { "End" } else { "Length" }, w * 0.26)];
    let hr = Rect::from_min_size(table.min, vec2(table.width(), ROW));
    p.rect_filled(hr, 0.0, look::header_bg());
    let mut x = hr.min.x + 8.0;
    for (n, cw) in cols {
        p.text(pos2(x, hr.center().y), Align2::LEFT_CENTER, n, look::semibold(11.0), look::text_dim());
        x += cw;
    }
    let mut y = hr.max.y + 1.0;
    let end_t = end(app);
    let markers = app.markers.clone();
    for (i, m) in markers.iter().enumerate() {
        let row = Rect::from_min_size(pos2(table.min.x, y), vec2(table.width(), ROW));
        y += ROW;
        if row.max.y > table.max.y {
            break;
        }
        let sel = app.mk.sel.contains(&i);
        if sel {
            p.rect_filled(row, 0.0, look::accent().gamma_multiply(0.45));
        } else if i % 2 == 1 {
            p.rect_filled(row, 0.0, look::lane_alt());
        }
        let len = mk::end_of(&markers, i, end_t) - m.0;
        let cells = [
            (Field::Pos, Rect::from_min_size(pos2(row.min.x + 4.0, row.min.y), vec2(cols[0].1, ROW)), pos_text(app, m.0)),
            (Field::Name, Rect::from_min_size(pos2(row.min.x + 4.0 + cols[0].1, row.min.y), vec2(cols[1].1, ROW)), m.1.clone()),
            (Field::Len, Rect::from_min_size(pos2(row.min.x + 4.0 + cols[0].1 + cols[1].1, row.min.y), vec2(cols[2].1, ROW)), len_text(app, m.0, len)),
        ];
        for (f, cr, s) in cells {
            if let Some((ek, ef, buf)) = app.mk.edit.as_mut().filter(|e| e.0 == i && e.1 == f) {
                let _ = (ek, ef);
                let te = ui.put(cr.shrink2(vec2(0.0, 1.0)), egui::TextEdit::singleline(buf).font(look::regular(12.0)));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let v = buf.clone();
                    app.mk.edit = None;
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        commit(app, i, f, &v);
                    }
                }
                continue;
            }
            let mut tx = pos2(cr.min.x + 4.0, cr.center().y);
            if f == Field::Name && m.2.lock.is_some() {
                crate::glyphs::paint(&p, Rect::from_center_size(pos2(tx.x + 5.0, tx.y), vec2(11.0, 11.0)), crate::glyphs::ph::LOCK_SIMPLE, 11.0, look::text_dim());
                tx.x += 14.0;
            }
            if f == Field::Name && m.2.color.is_some() {
                p.circle_filled(pos2(cr.max.x - 10.0, cr.center().y), 4.0, fill(m));
            }
            p.with_clip_rect(cr).text(tx, Align2::LEFT_CENTER, &s, look::regular(12.0), look::text());
            let r = ui.interact(cr, ui.scope_id().with(("ml", i, f as u8)), Sense::click_and_drag());
            if r.clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    app.mk.sel.push(i);
                    app.mk.focus = true;
                } else {
                    select_only(app, i);
                }
                if app.tool == Tool::Eraser {
                    delete(app, &[i]);
                    return;
                }
            }
            if r.double_clicked() {
                app.mk.edit = Some((i, f, if f == Field::Name { m.1.clone() } else { s.clone() }));
                app.locate(m.0 as f64);
            }
            // Position and Length slide: drag up / down by the division (Shift: ticks)
            if f != Field::Name && r.drag_started() {
                app.checkpoint(if f == Field::Pos { "Move Marker" } else { "Resize Marker" });
            }
            if f != Field::Name && r.dragged() {
                let step = if ui.input(|i| i.modifiers.shift) { 1.0 } else { division(app) as f32 };
                let d = (-r.drag_delta().y / 3.0 * step).round() as i64;
                if d != 0 {
                    let cur = app.markers.get(i).map_or(0, |x| x.0);
                    if f == Field::Pos {
                        let n = mk::move_to(&mut app.markers, i, (cur as i64 + d).max(0) as u32);
                        app.mk.sel = vec![n];
                    } else {
                        let e = mk::end_of(&app.markers, i, end_t);
                        mk::set_range(&mut app.markers, i, cur, (e as i64 + d).max(cur as i64 + 1) as u32);
                    }
                }
            }
        }
    }
    // the Pencil in the list's empty area: a marker at the playhead whose position is typed
    let rest = Rect::from_min_max(pos2(table.min.x, y), table.max);
    if rest.height() > 2.0 && ui.interact(rest, ui.scope_id().with("ml-empty"), Sense::click()).clicked() {
        if app.tool == Tool::Pencil {
            let pos = app.position() as u32;
            if let Some(k) = create(app, pos, Rounding::None, "Create Marker") {
                let s = pos_text(app, app.markers[k].0);
                app.mk.edit = Some((k, Field::Pos, s));
            }
        } else {
            app.mk.sel.clear();
        }
    }
    let foot = Rect::from_min_max(pos2(body.min.x, body.max.y - 20.0), body.max);
    p.text(pos2(foot.max.x - 8.0, foot.center().y), Align2::RIGHT_CENTER, format!("{} Markers", app.markers.len()), look::regular(11.5), look::text_dim());
}

fn commit(app: &mut App, k: usize, f: Field, v: &str) {
    let Some(start) = app.markers.get(k).map(|m| m.0) else { return };
    match f {
        Field::Name => {
            app.checkpoint("Rename Marker");
            app.markers[k].1 = v.to_string();
        }
        Field::Pos => {
            if let Some(t) = parse_pos(app, v) {
                app.checkpoint("Move Marker");
                let n = mk::move_to(&mut app.markers, k, t);
                app.mk.sel = vec![n];
            }
        }
        Field::Len => {
            if let Some(l) = parse_len(app, start, v) {
                app.checkpoint("Resize Marker");
                mk::set_len(&mut app.markers, k, l);
            }
        }
    }
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.x = 6.0;
    let any = !app.mk.sel.is_empty();
    ui.menu_button("Options ▾", |ui| {
        if ui.button("Create").clicked() {
            app.mk.focus = true;
            let pos = app.position() as u32;
            create(app, pos, Rounding::Bar, "Create Marker");
            ui.close();
        }
        if ui.button("Create Without Rounding").clicked() {
            app.mk.focus = true;
            let pos = app.position() as u32;
            create(app, pos, Rounding::None, "Create Marker");
            ui.close();
        }
        ui.separator();
        for n in ["Lock SMPTE Position", "Unlock SMPTE Position"] {
            if ui.add_enabled(any, egui::Button::new(n)).clicked() {
                app.mk.focus = true;
                let ctx = ui.ctx().clone();
                command(app, &ctx, n);
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(any, egui::Button::new("Delete")).clicked() {
            let ks = app.mk.sel.clone();
            delete(app, &ks);
            ui.close();
        }
    });
    ui.menu_button("View ▾", |ui| {
        if ui.add(egui::Button::selectable(app.mk.as_time, "Show Event Position and Length as Time").shortcut_text("⌃⌥R")).clicked() {
            app.mk.as_time = !app.mk.as_time;
            ui.close();
        }
        if ui.add(egui::Button::selectable(app.mk.len_abs, "Length as Absolute Position").shortcut_text("⌃⇧A")).clicked() {
            app.mk.len_abs = !app.mk.len_abs;
            ui.close();
        }
    });
    if ui.button("+").on_hover_text("Add Marker (at the playhead, rounded)").clicked() {
        app.mk.focus = true;
        let pos = app.position() as u32;
        create(app, pos, Rounding::Bar, "Create Marker");
    }
    ui.label(egui::RichText::new("Marker Set:").color(look::text_dim()));
    set_popup(app, ui);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(egui::Button::new("¶").selected(app.mk.text_area)).on_hover_text("Marker Text area").clicked() {
            app.mk.text_area = !app.mk.text_area;
        }
    });
    // the list's own keys: ⌃⌥R, ⌃⇧A
    let (r, a) = ui.input(|i| (i.modifiers.ctrl && i.modifiers.alt && i.key_pressed(egui::Key::R), i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::A)));
    if r {
        app.mk.as_time = !app.mk.as_time;
    }
    if a {
        app.mk.len_abs = !app.mk.len_abs;
    }
}

/// The Marker Set pop-up: the sets, New Set, Rename Set…, Delete Set.
fn set_popup(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = app.project.as_ref() else { return };
    let names: Vec<String> = if p.global.marker_sets.is_empty() { vec!["Marker Set 1".into()] } else { p.global.marker_sets.iter().map(|s| s.name.clone()).collect() };
    let cur = p.global.marker_set.min(names.len() - 1);
    if let Some(mut n) = app.mk.set_name.clone() {
        let te = ui.add(egui::TextEdit::singleline(&mut n).desired_width(120.0));
        crate::widgets::focus_once(ui, &te);
        if te.lost_focus() {
            app.mk.set_name = None;
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && !n.trim().is_empty() {
                app.checkpoint("Rename Marker Set");
                let ms = app.markers.clone();
                if let Some(p) = app.project.as_mut() {
                    mk::rename_set(&mut p.global.marker_sets, cur, &ms, n.trim());
                }
            }
        } else {
            app.mk.set_name = Some(n);
        }
        return;
    }
    let mut choose = None;
    let mut action = 0;
    egui::ComboBox::from_id_salt("marker-set").selected_text(names[cur].clone()).width(130.0).show_ui(ui, |ui| {
        for (k, n) in names.iter().enumerate() {
            if ui.selectable_label(k == cur, n).clicked() {
                choose = Some(k);
            }
        }
        ui.separator();
        if ui.button("New Set").clicked() {
            action = 1;
        }
        if ui.button("Rename Set…").clicked() {
            action = 2;
        }
        if ui.add_enabled(names.len() > 1, egui::Button::new("Delete Set")).clicked() {
            action = 3;
        }
    });
    if choose.is_none() && action == 0 {
        return;
    }
    let what = match action {
        1 => "New Marker Set",
        2 => "",
        3 => "Delete Marker Set",
        _ => "Marker Set",
    };
    if action == 2 {
        app.mk.set_name = Some(names[cur].clone());
        return;
    }
    app.checkpoint(what);
    let mut ms = std::mem::take(&mut app.markers);
    if let Some(p) = app.project.as_mut() {
        let g = &mut p.global;
        match (action, choose) {
            (1, _) => mk::new_set(&mut g.marker_sets, &mut g.marker_set, &mut ms, None),
            (3, _) => {
                mk::delete_set(&mut g.marker_sets, &mut g.marker_set, &mut ms);
            }
            (_, Some(k)) => {
                if g.marker_sets.is_empty() {
                    g.marker_sets.push(mk::MarkerSet { name: "Marker Set 1".into(), markers: ms.clone() });
                }
                mk::choose_set(&mut g.marker_sets, &mut g.marker_set, &mut ms, k);
            }
            _ => {}
        }
    }
    app.markers = ms;
    app.mk.sel.clear();
}

// ---- markers in audio files ---------------------------------------------------------------------

/// Navigate ▸ Other ▸ Import / Export / Remove Marker (the selected audio region's file).
fn audio_file(app: &mut App, name: &str) {
    let Some((t, r)) = app.region_selection().first().copied().or(app.selected_region) else {
        app.tell("Select an audio region first");
        return;
    };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let Some(a) = rg.audio.clone() else {
        app.tell("Select an audio region first");
        return;
    };
    let Ok(bytes) = std::fs::read(&a.file) else {
        app.tell(format!("Couldn't read {}", a.file.display()));
        return;
    };
    let sr = wav_rate(&bytes).unwrap_or(44100) as f64;
    let m = map(app);
    let start_s = m.secs_at(rg.start as f64);
    let offset = rg.props.audio_offset as f64 / sr;
    match name {
        "Import Marker from Audio File" => {
            let cues = mk::wav_markers(&bytes);
            if cues.is_empty() {
                app.tell("That audio file has no markers");
                return;
            }
            app.checkpoint("Import Marker from Audio File");
            for (s, n) in cues {
                let secs = start_s + s as f64 / sr - offset;
                if secs < start_s {
                    continue;
                }
                mk::create(&mut app.markers, m.tick_at(secs).round() as u32, Some(&n));
            }
        }
        _ => {
            let keep: Vec<(u32, String)> = if name.starts_with("Export") {
                let end_t = rg.start + rg.length;
                app.markers.iter().filter(|x| x.0 >= rg.start && x.0 < end_t).map(|x| (((m.secs_at(x.0 as f64) - start_s + offset) * sr).round().max(0.0) as u32, x.1.clone())).collect()
            } else {
                Vec::new()
            };
            match mk::with_wav_markers(&bytes, &keep) {
                Some(out) if std::fs::write(&a.file, &out).is_ok() => app.tell(if keep.is_empty() { "Markers removed from the audio file".to_string() } else { format!("{} markers written to the audio file", keep.len()) }),
                _ => app.tell("Only WAV files can hold markers"),
            }
        }
    }
}

fn wav_rate(b: &[u8]) -> Option<u32> {
    let i = b.windows(4).position(|w| w == b"fmt ")?;
    Some(u32::from_le_bytes(b.get(i + 12..i + 16)?.try_into().ok()?))
}

// ---- the project file: colours, locks, sets (Studio's extras) ---------------------------------

/// Extras lines for the markers (colours and SMPTE locks by index, the sets).
pub fn extras_text(p: &lg_core::Project) -> String {
    let mut out = String::new();
    let line = |m: &Marker| format!("{}|{}|{}|{}|{}", m.0, m.2.len, m.2.color.map_or(-1, |c| c as i32), m.2.lock.map_or("-".into(), |s| s.to_string()), m.full_text().replace('\n', "\\n").replace('\t', " "));
    for (k, m) in p.markers.iter().enumerate() {
        if m.2.color.is_some() || m.2.lock.is_some() {
            out += &format!("mkx\t{k}\t{}\t{}\n", m.2.color.map_or(-1, |c| c as i32), m.2.lock.map_or("-".into(), |s| s.to_string()));
        }
    }
    for (k, s) in p.global.marker_sets.iter().enumerate() {
        let ms: Vec<String> = if k == p.global.marker_set { Vec::new() } else { s.markers.iter().map(line).collect() };
        out += &format!("mset\t{k}\t{}\t{}\n", s.name, ms.join("\t"));
    }
    if !p.global.marker_sets.is_empty() {
        out += &format!("mcur\t{}\n", p.global.marker_set);
    }
    out
}

pub fn apply_extras(p: &mut lg_core::Project, l: &str) -> bool {
    let w: Vec<&str> = l.split('\t').collect();
    match w.as_slice() {
        ["mkx", k, c, lock] => {
            if let Some(m) = k.parse::<usize>().ok().and_then(|k| p.markers.get_mut(k)) {
                m.2.color = c.parse::<i32>().ok().filter(|c| *c >= 0).map(|c| c as u8);
                m.2.lock = lock.parse().ok();
            }
            true
        }
        ["mset", _, name, rest @ ..] => {
            let markers = rest
                .iter()
                .filter_map(|s| {
                    let f: Vec<&str> = s.splitn(5, '|').collect();
                    let mut m = Marker::new(f.first()?.parse().ok()?, "");
                    m.2.len = f.get(1)?.parse().ok()?;
                    m.2.color = f.get(2)?.parse::<i32>().ok().filter(|c| *c >= 0).map(|c| c as u8);
                    m.2.lock = f.get(3)?.parse().ok();
                    m.set_full_text(&f.get(4)?.replace("\\n", "\n"));
                    Some(m)
                })
                .collect();
            p.global.marker_sets.push(mk::MarkerSet { name: name.to_string(), markers });
            true
        }
        ["mcur", k] => {
            p.global.marker_set = k.parse().unwrap_or(0);
            if let Some(s) = p.global.marker_sets.get_mut(p.global.marker_set) {
                s.markers = p.markers.clone();
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_round_trip() {
        let mut p = crate::demo::empty();
        p.markers = vec![Marker::new(0, "Intro"), Marker::new(3840, "Verse")];
        p.markers[1].2.color = Some(5);
        p.markers[1].2.lock = Some(2.0);
        let mut cur = p.markers.clone();
        mk::new_set(&mut p.global.marker_sets, &mut p.global.marker_set, &mut cur, Some("Alt"));
        mk::create(&mut cur, 1920, Some("Solo"));
        cur[0].set_full_text("Solo\nlouder");
        p.markers = cur;
        let text = extras_text(&p);
        let mut q = crate::demo::empty();
        q.markers = p.markers.iter().map(|m| Marker(m.0, m.1.clone(), mk::MarkerInfo { text: m.2.text.clone(), ..Default::default() })).collect();
        for l in text.lines() {
            assert!(apply_extras(&mut q, l), "{l}");
        }
        assert_eq!(q.global.marker_sets[0].markers, p.global.marker_sets[0].markers);
        assert_eq!(q.global.marker_sets[0].markers[1].2.color, Some(5));
        assert_eq!(q.global.marker_set, 1);
        assert_eq!(q.markers, p.markers);
    }

    #[test]
    fn positions_parse_as_shown() {
        let mut app = crate::tm::test_app();
        app.project = Some(crate::demo::empty());
        let t = parse_pos(&app, "3 2 1 1").unwrap();
        assert_eq!(t, 2 * 4 * PPQ + PPQ);
        assert_eq!(pos_text(&app, t), "3 2 1 1");
        assert_eq!(parse_len(&app, 0, "1 1 0 0"), Some(5 * PPQ));
        assert_eq!(len_text(&app, 0, 5 * PPQ), "1 1 0 0");
    }
}
