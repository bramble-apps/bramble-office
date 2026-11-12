//! The Cell inspector (Logic Pro 13: it replaces the Region inspector while Live Loops cells are
//! selected; its title is the cell's name, or how many cells are selected), the cells' loop
//! settings in it (Play From ▸ Start, Loop Start, Loop Length, Cell Length, Q Loop Start), the
//! Tracks area's View ▸ Live Loops display options (Show Start Positions as Offset, Show Length as
//! Position, Position and Length Resolution), the cell selection commands (Muted, Empty,
//! Same-Colored Cells), and undo for cell edits.
//!
//! Undo: a cell edit takes the app's ordinary undo step (App::checkpoint) and keeps the cells as
//! they were beside it, keyed by the undo depth; Undo / Redo put them back with it.

use std::collections::BTreeSet;

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::app::App;
use crate::inspector::{label, slot_rect, ROW};
use crate::liveloops::{Cell, View, PLAY_FROMS, QUANTIZE_STARTS, SPEEDS};
use crate::look;
use crate::widgets;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &[
    "Live Loops: Show Start Positions as Offset",
    "Live Loops: Show Length as Position",
    "Live Loops: Keep Cells Squared When Zooming",
    "Try to Keep Cells Squared When Zooming",
    "Show Start Positions as Offset",
    "Show Length as Position",
    "Select Muted Regions/Cells/Events",
    "Select Same-Colored Regions/Cells/Events",
    "Select Empty Regions/Cells",
    "Muted Cells",
    "Empty Cells",
    "Same-Colored Cells",
];

pub const RESOLUTIONS: [&str; 4] = ["Bars only", "Bars and Beats", "Bars, Beats and Division", "Bars, Beats, Division and Ticks"];
/// The resolution (unset: Bars and Beats, Logic's default).
pub fn res_of(r: u8) -> u8 {
    if r == 0 { 2 } else { r.min(4) }
}

pub const Q_LOOP: [&str; 3] = ["Off", "On", "On, Preserve Start"];

// ---- undo -------------------------------------------------------------------------------------

/// An undo step for a cell edit (call before changing the cells).
pub fn checkpoint(app: &mut App, what: &str) {
    let at = app.undo.len();
    let cells = app.live_loops.cells.clone();
    app.checkpoint(what);
    app.live_loops.cell_undo.push((at, cells));
    app.live_loops.cell_redo.clear();
    app.dirty = true;
}

/// Runs `f` (a cell command) as one undo step when it changed any cell.
pub fn undoable(app: &mut App, what: &str, f: impl FnOnce(&mut App) -> bool) -> bool {
    let before = app.live_loops.cells.clone();
    let depth = app.undo.len();
    let done = f(app);
    if app.live_loops.cells != before {
        // (a command that took the app's undo step itself keeps it)
        let at = if app.undo.len() > depth { depth } else {
            app.checkpoint(what);
            depth
        };
        app.live_loops.cell_undo.push((at, before));
        app.live_loops.cell_redo.clear();
        app.dirty = true;
    }
    done
}

/// After App::undo popped a step: the cells as they were then.
pub fn undone(app: &mut App) {
    let depth = app.undo.len();
    if app.live_loops.cell_undo.last().is_some_and(|u| u.0 == depth) {
        let (at, cells) = app.live_loops.cell_undo.pop().unwrap();
        let now = std::mem::replace(&mut app.live_loops.cells, cells);
        app.live_loops.cell_redo.push((at, now));
        tidy(app);
    }
}

/// After App::redo pushed a step back: the cells again as they were after it.
pub fn redone(app: &mut App) {
    let depth = app.undo.len();
    if app.live_loops.cell_redo.last().is_some_and(|u| u.0 + 1 == depth) {
        let (at, cells) = app.live_loops.cell_redo.pop().unwrap();
        let now = std::mem::replace(&mut app.live_loops.cells, cells);
        app.live_loops.cell_undo.push((at, now));
        tidy(app);
    }
}

fn tidy(app: &mut App) {
    let s = &mut app.live_loops;
    let cells = s.cells.clone();
    for (t, p) in s.plays.iter_mut() {
        if p.current.is_some_and(|c| !cells.contains_key(&(*t, c.0))) {
            p.current = None;
        }
    }
}

// ---- positions ---------------------------------------------------------------------------------

/// A position or length as Logic shows it: bars, beats, division (1/16), ticks, as many as the
/// resolution asks for. Positions count from 1 ("1 1 1 1" is the cell's beginning), lengths and
/// offsets from 0.
pub fn fmt_pos(t: u32, bar: u32, res: u8, position: bool) -> String {
    let bar = bar.max(1);
    let o = position as u32;
    let r = t % bar;
    let parts = [t / bar + o, r / 960 + o, r % 960 / 240 + o, r % 240 + o];
    parts[..res.clamp(1, 4) as usize].iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" ")
}

/// Typed text back into ticks ("2 1 1 1", "2.1", "3"); missing parts count as their start.
pub fn parse_pos(text: &str, bar: u32, position: bool) -> Option<u32> {
    let nums: Vec<i64> = text.split(|c: char| c == ' ' || c == '.' || c == ':').filter(|s| !s.is_empty()).map(|s| s.parse::<i64>()).collect::<Result<_, _>>().ok()?;
    if nums.is_empty() || nums.len() > 4 {
        return None;
    }
    let o = position as i64;
    let units = [bar as i64, 960, 240, 1];
    let mut t = 0i64;
    for (k, n) in nums.iter().enumerate() {
        t += (n - o) * units[k];
    }
    (t >= 0).then_some(t as u32)
}

/// A position value: drag a unit (bar, beat, division, tick) up or down to change it by that
/// unit, double-click to type, Return to finish. Returns the new value.
fn pos_slot(ui: &mut egui::Ui, r: Rect, id: egui::Id, value: u32, bar: u32, res: u8, position: bool, base: u32, min: u32, max: u32) -> Option<u32> {
    let typing_id = id.with("typing");
    let shown = fmt_pos(value + base, bar, res, position);
    if let Some(mut t) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        let te = ui.put(r, egui::TextEdit::singleline(&mut t).font(look::regular(12.0)).align(Align2::RIGHT_CENTER));
        widgets::focus_once(ui, &te);
        if te.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            return parse_pos(&t, bar, position).map(|v| v.saturating_sub(base).clamp(min, max)).filter(|v| *v != value);
        }
        ui.data_mut(|d| d.insert_temp(typing_id, t));
        return None;
    }
    let resp = ui.interact(r, id, Sense::click_and_drag());
    let p = ui.painter();
    // (Logic's: a dark rounded field, each unit in its own column, right-aligned)
    let parts: Vec<&str> = shown.split(' ').collect();
    let col = 28.0;
    let field = Rect::from_min_max(pos2(r.max.x - 6.0 - col * parts.len() as f32, r.min.y), r.max);
    p.rect_filled(field, 5.0, look::lcd_bg());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let font = look::regular(12.0);
    let mut spans = Vec::new();
    for (k, part) in parts.iter().enumerate() {
        let x1 = field.min.x + 3.0 + col * (k + 1) as f32 - 6.0;
        p.text(pos2(x1, r.center().y), Align2::RIGHT_CENTER, *part, font.clone(), look::text());
        spans.push((k, x1 - col + 6.0, x1 + 3.0));
    }
    let unit_id = id.with("unit");
    if resp.drag_started() {
        let px = resp.interact_pointer_pos().map_or(r.max.x, |pp| pp.x);
        let k = spans.iter().find(|s| px >= s.1 && px <= s.2 + 3.0).map_or(parts.len() - 1, |s| s.0);
        ui.data_mut(|d| d.insert_temp(unit_id, k));
    }
    if resp.dragged() {
        let k: usize = ui.data(|d| d.get_temp(unit_id)).unwrap_or(0);
        let unit = [bar.max(1), 960, 240, 1][k.min(3)] as i64;
        let acc_id = id.with("acc");
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
        acc += -resp.drag_delta().y / 4.0;
        let steps = acc.trunc();
        ui.data_mut(|d| d.insert_temp(acc_id, acc - steps));
        if steps != 0.0 {
            let v = (value as i64 + steps as i64 * unit).clamp(min as i64, max as i64) as u32;
            return (v != value).then_some(v);
        }
    }
    if resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, shown));
    }
    None
}

// ---- the inspector -----------------------------------------------------------------------------

/// Whether the Inspector shows the Cell inspector (cells are the selection).
pub fn active(app: &App) -> bool {
    let s = &app.live_loops;
    s.view != View::TracksOnly && s.cell_sel && !s.selected.is_empty() && app.selected_region.is_none()
}

fn title(app: &App) -> String {
    let s = &app.live_loops;
    if s.selected.len() > 1 {
        return format!("{} Cells selected", s.selected.len());
    }
    let Some(k) = s.selected.iter().next() else { return String::new() };
    match s.cells.get(k) {
        Some(c) if !c.name.is_empty() => c.name.clone(),
        Some(_) => app.project.as_ref().and_then(|p| p.tracks.get(k.0)).map(|t| t.name.clone()).unwrap_or_default(),
        None => {
            let audio = app.project.as_ref().and_then(|p| p.tracks.get(k.0)).is_some_and(|t| t.kind == lg_core::TrackKind::Audio);
            if audio { "Empty Audio Cell".into() } else { "Empty MIDI Cell".into() }
        }
    }
}

/// A disclosure row: ▸/▾ at the left, the label, its value as a pop-up. Returns (open toggled, choice).
fn disclosure_choice(ui: &mut egui::Ui, area: Rect, y: f32, id: egui::Id, name: &str, open: bool, cur: usize, names: &[&str]) -> (bool, Option<usize>) {
    let p = ui.painter().clone();
    let tri = Rect::from_min_size(pos2(area.min.x + 2.0, y), vec2(14.0, ROW));
    crate::glyphs::paint(&p, Rect::from_center_size(tri.center(), vec2(10.0, 10.0)), if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 10.0, look::text_dim());
    label(&p, area, y, name);
    let toggled = ui.interact(Rect::from_min_max(tri.min, pos2(area.min.x + area.width() * 0.46, y + ROW)), id.with("open"), Sense::click()).clicked();
    (toggled, widgets::choice_slot(ui, slot_rect(area, y), id, cur, names))
}

/// The Cell inspector section; returns the y below it.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32) -> f32 {
    let p = ui.painter().clone();
    let id = ui.scope_id().with("cell-insp");
    // header: "Cell:" and the name (click the name to rename one cell)
    let hr = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), 32.0));
    let name = title(app);
    let renaming_id = id.with("rename");
    let mut renaming: Option<String> = ui.data(|d| d.get_temp(renaming_id));
    p.text(pos2(hr.min.x + 12.0, hr.center().y), Align2::LEFT_CENTER, "Cell:", look::bold(13.0), look::text());
    let lw = p.layout_no_wrap("Cell:".into(), look::bold(13.0), look::text()).size().x;
    let name_r = Rect::from_min_max(pos2(hr.min.x + 16.0 + lw, hr.min.y + 6.0), pos2(hr.max.x - 30.0, hr.max.y - 6.0));
    let one = app.live_loops.selected.len() == 1 && app.live_loops.selected.iter().next().is_some_and(|k| app.live_loops.cells.contains_key(k));
    if let Some(text) = renaming.as_mut() {
        let te = ui.put(name_r, egui::TextEdit::singleline(text).font(look::regular(13.0)));
        widgets::focus_once(ui, &te);
        if te.lost_focus() {
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let new = text.trim().to_string();
            ui.data_mut(|d| d.remove::<String>(renaming_id));
            if !cancel {
                let keys: Vec<_> = app.live_loops.selected.iter().copied().collect();
                undoable(app, "Rename Cell", |app| {
                    for k in keys {
                        if let Some(c) = app.live_loops.cells.get_mut(&k) {
                            c.name = new.clone();
                        }
                    }
                    true
                });
            }
        } else {
            ui.data_mut(|d| d.insert_temp(renaming_id, text.clone()));
        }
    } else {
        p.with_clip_rect(name_r).text(pos2(name_r.min.x, hr.center().y), Align2::LEFT_CENTER, &name, look::regular(13.0), look::text_dim());
        if one && ui.interact(name_r, id.with("name"), Sense::click()).on_hover_text("Rename Cell").clicked() {
            let cur = app.live_loops.selected.iter().next().and_then(|k| app.live_loops.cells.get(k)).map(|c| c.name.clone()).unwrap_or_default();
            ui.data_mut(|d| d.insert_temp(renaming_id, cur));
        }
    }
    let open = app.inspector_region_open;
    let chev = Rect::from_center_size(pos2(hr.max.x - 18.0, hr.center().y), vec2(24.0, 24.0));
    crate::glyphs::paint(&p, Rect::from_center_size(chev.center(), vec2(14.0, 14.0)), if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 13.0, look::text_dim());
    let head = ui.interact(Rect::from_min_max(hr.min, pos2(hr.min.x + 16.0 + lw, hr.max.y)).union(chev), id.with("head"), Sense::click());
    if head.clicked() {
        app.inspector_region_open = !open;
    }
    p.hline(hr.x_range(), hr.max.y, Stroke::new(1.0, look::line()));
    y += 33.0;
    let body_top = y;
    if !open {
        return y;
    }
    // the shortcut menu (display options): under the rows, so they get the clicks first
    let body_h: f32 = ui.data(|d| d.get_temp(id.with("body-h"))).unwrap_or(0.0);
    let body = Rect::from_min_size(pos2(area.min.x, body_top), vec2(area.width(), body_h));
    ui.interact(body, id.with("body-menu"), Sense::click()).context_menu(|ui| {
        display_menu(app, ui);
    });
    // the cells' values (the first's shown; changes go to all selected)
    let keys: Vec<(usize, usize)> = app.live_loops.selected.iter().copied().filter(|k| app.live_loops.cells.contains_key(k)).collect();
    let Some(c) = keys.first().and_then(|k| app.live_loops.cells.get(k)).cloned() else {
        return y;
    };
    let bar = app.bar_ticks().max(1);
    let (res, as_pos, offset) = (res_of(app.live_loops.resolution), app.live_loops.length_as_position, app.live_loops.start_as_offset);
    let mut edit: Option<(&'static str, Box<dyn Fn(&mut Cell)>)> = None;
    // (a disclosure's rows indented, as Logic's)
    let sub = Rect::from_min_max(pos2(area.min.x + 10.0, area.min.y), area.max);
    let mut b = c.muted;
    label(&p, area, y, "Mute");
    if widgets::check_slot(ui, slot_rect(area, y), id.with("mute"), &mut b) {
        edit = Some(("Mute Cell", Box::new(move |c| c.muted = b)));
    }
    y += ROW;
    let mut b = c.looped;
    label(&p, area, y, "Loop");
    if widgets::check_slot(ui, slot_rect(area, y), id.with("loop"), &mut b) {
        edit = Some(("Loop Cell", Box::new(move |c| c.looped = b)));
    }
    y += ROW;
    label(&p, area, y, "Quantize");
    let qs: Vec<&str> = std::iter::once("Off").chain(crate::inspector::QUANTIZE.iter().skip(1).map(|q| q.0)).collect();
    let cur = qs.iter().position(|q| *q == c.quantize).unwrap_or(0);
    if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("q"), cur, &qs) {
        let v = qs[i].to_string();
        edit = Some(("Cell Quantize", Box::new(move |c| c.quantize = v.clone())));
    }
    y += ROW;
    label(&p, area, y, "Transpose");
    let mut v = c.transpose as f32;
    if widgets::value_slot(ui, slot_rect(area, y), id.with("tr"), &mut v, -36.0, 36.0, 0.0, 1.0, &|v: f32| if v == 0.0 { String::new() } else { format!("{v:+.0}") }) {
        edit = Some(("Cell Transpose", Box::new(move |c| c.transpose = v as i8)));
    }
    y += ROW;
    // Play From ▸ the loop settings
    let cur = PLAY_FROMS.iter().position(|x| *x == c.play_from).unwrap_or(0);
    let (tog, ch) = disclosure_choice(ui, area, y, id.with("pf"), "Play From", app.live_loops.play_from_open, cur, &PLAY_FROMS);
    if tog {
        app.live_loops.play_from_open = !app.live_loops.play_from_open;
    }
    if let Some(i) = ch {
        let v = PLAY_FROMS[i].to_string();
        edit = Some(("Cell Play From", Box::new(move |c| c.play_from = v.clone())));
    }
    y += ROW;
    if app.live_loops.play_from_open {
        // (positions: from the cell's beginning as offsets, or as positions "1 1 1 1")
        let rows: [(&str, u32, u32, u32, u32); 4] = [
            ("Start", c.start_pos(), 0, 0, c.length.saturating_sub(1)),
            ("Loop Start", c.loop_start, 0, 0, c.length.saturating_sub(1)),
            if as_pos { ("Loop End", c.loop_len(), 0, c.loop_start, c.length.saturating_sub(c.loop_start)) } else { ("Loop Length", c.loop_len(), 0, 0, c.length.saturating_sub(c.loop_start)) },
            if as_pos { ("Cell End", c.length, 0, 0, bar * 1024) } else { ("Cell Length", c.length, 0, 0, bar * 1024) },
        ];
        for (k, (name, val, _, base, max)) in rows.into_iter().enumerate() {
            label(&p, sub, y, name);
            // starts as positions unless "as Offset"; lengths as lengths unless "as Position"
            let position = if k < 2 { !offset } else { as_pos };
            let min = if k >= 2 { 1 } else { 0 };
            if let Some(nv) = pos_slot(ui, slot_rect(area, y), id.with(("pos", k)), val, bar, res, position, base, min, max.max(min)) {
                edit = Some(match k {
                    0 => ("Cell Start", Box::new(move |c: &mut Cell| c.start = Some(nv).filter(|v| *v != c.loop_start))),
                    1 => ("Cell Loop Start", Box::new(move |c: &mut Cell| {
                        let end = c.loop_end();
                        c.loop_start = nv.min(c.length.saturating_sub(1));
                        c.loop_len = Some(end.saturating_sub(c.loop_start).max(1)).filter(|l| c.loop_start + l != c.length);
                    })),
                    2 => ("Cell Loop Length", Box::new(move |c: &mut Cell| c.loop_len = Some(nv.max(1)).filter(|l| c.loop_start + l != c.length))),
                    _ => ("Cell Length", Box::new(move |c: &mut Cell| set_length(c, nv))),
                });
            }
            y += ROW;
        }
        label(&p, sub, y, "Q Loop Start");
        if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("ql"), c.q_loop as usize, &Q_LOOP) {
            edit = Some(("Quantize to Loop Start", Box::new(move |c| c.q_loop = i as u8)));
        }
        y += ROW;
    }
    label(&p, area, y, "Quantize Start");
    let cur = QUANTIZE_STARTS.iter().position(|x| *x == c.quantize_start).unwrap_or(0);
    if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("qs"), cur, &QUANTIZE_STARTS) {
        let v = QUANTIZE_STARTS[i].to_string();
        edit = Some(("Cell Quantize Start", Box::new(move |c| c.quantize_start = v.clone())));
    }
    y += ROW;
    // Trigger Mode ▸ Speed, Reverse
    let cur = crate::liveloops::TRIGGER_MODES.iter().position(|x| *x == c.trigger).unwrap_or(0);
    let (tog, ch) = disclosure_choice(ui, area, y, id.with("tm"), "Trigger Mode", app.live_loops.trigger_open, cur, &crate::liveloops::TRIGGER_MODES);
    if tog {
        app.live_loops.trigger_open = !app.live_loops.trigger_open;
    }
    if let Some(i) = ch {
        let v = crate::liveloops::TRIGGER_MODES[i].to_string();
        edit = Some(("Cell Trigger Mode", Box::new(move |c| c.trigger = v.clone())));
    }
    y += ROW;
    if app.live_loops.trigger_open {
        label(&p, sub, y, "Speed");
        let names: Vec<&str> = SPEEDS.iter().map(|s| s.0).collect();
        let cur = SPEEDS.iter().position(|s| (s.1 - c.speed).abs() < 1e-6).unwrap_or(3);
        if let Some(i) = widgets::choice_slot(ui, slot_rect(area, y), id.with("speed"), cur, &names) {
            let v = SPEEDS[i].1;
            edit = Some(("Cell Speed", Box::new(move |c| c.speed = v)));
        }
        y += ROW;
        let mut b = c.reversed;
        label(&p, sub, y, "Reverse");
        if widgets::check_slot(ui, slot_rect(area, y), id.with("rev"), &mut b) {
            edit = Some(("Reverse Cell", Box::new(move |c| c.reversed = b)));
        }
        y += ROW;
    }
    if let Some((what, f)) = edit {
        checkpoint(app, what);
        for k in &keys {
            if let Some(c) = app.live_loops.cells.get_mut(k) {
                f(c);
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(id.with("body-h"), y - body_top));
    y += 4.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    y + 1.0
}

/// Show Length as Position, Show Start Positions as Offset and the resolution (the Cell
/// inspector's shortcut menu and the Tracks area's View menu).
pub fn display_menu(app: &mut App, ui: &mut egui::Ui) {
    let s = &mut app.live_loops;
    if ui.add(egui::Button::selectable(s.length_as_position, "Show Length as Position")).clicked() {
        s.length_as_position = !s.length_as_position;
        ui.close();
    }
    if ui.add(egui::Button::selectable(s.start_as_offset, "Show Start Positions as Offset")).clicked() {
        s.start_as_offset = !s.start_as_offset;
        ui.close();
    }
    ui.menu_button("Position and Length Resolution", |ui| {
        for (k, r) in RESOLUTIONS.iter().enumerate() {
            if ui.add(egui::Button::selectable(res_of(s.resolution) as usize == k + 1, *r)).clicked() {
                s.resolution = k as u8 + 1;
                ui.close();
            }
        }
    });
}

/// The Tracks area's View menu items for Live Loops (menus.rs asks each module in turn).
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    let s = &mut app.live_loops;
    match it.label.as_str() {
        "Try to Keep Cells Squared When Zooming" => {
            if ui.add(egui::Button::selectable(!s.not_square, "Try to Keep Cells Squared When Zooming")).clicked() {
                s.not_square = !s.not_square;
                ui.close();
            }
        }
        "Show Start Positions as Offset" => {
            if ui.add(egui::Button::selectable(s.start_as_offset, "Show Start Positions as Offset")).clicked() {
                s.start_as_offset = !s.start_as_offset;
                ui.close();
            }
        }
        "Show Length as Position" => {
            if ui.add(egui::Button::selectable(s.length_as_position, "Show Length as Position")).clicked() {
                s.length_as_position = !s.length_as_position;
                ui.close();
            }
        }
        "Position and Length Resolution" => {
            ui.menu_button("Position and Length Resolution", |ui| {
                for (k, r) in RESOLUTIONS.iter().enumerate() {
                    if ui.add(egui::Button::selectable(res_of(s.resolution) as usize == k + 1, *r)).clicked() {
                        s.resolution = k as u8 + 1;
                        ui.close();
                    }
                }
            });
        }
        _ => return false,
    }
    true
}

/// A new cell length: the loop is kept inside it.
pub fn set_length(c: &mut Cell, len: u32) {
    c.length = len.max(1);
    c.loop_start = c.loop_start.min(c.length - 1);
    if let Some(l) = c.loop_len {
        let l = l.min(c.length - c.loop_start).max(1);
        c.loop_len = (c.loop_start + l != c.length).then_some(l);
    }
    if c.start.is_some_and(|s| s >= c.length) {
        c.start = None;
    }
}

// ---- commands -----------------------------------------------------------------------------------

/// Runs the display and selection commands; false when it isn't one of them.
pub fn command(app: &mut App, name: &str) -> bool {
    let tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let s = &mut app.live_loops;
    match name {
        "Live Loops: Show Start Positions as Offset" | "Show Start Positions as Offset" => s.start_as_offset = !s.start_as_offset,
        "Live Loops: Keep Cells Squared When Zooming" | "Try to Keep Cells Squared When Zooming" => s.not_square = !s.not_square,
        "Live Loops: Show Length as Position" | "Show Length as Position" => s.length_as_position = !s.length_as_position,
        n if RESOLUTIONS.contains(&n) => s.resolution = RESOLUTIONS.iter().position(|r| *r == n).unwrap_or(1) as u8 + 1,
        "Select Muted Regions/Cells/Events" | "Muted Cells" => {
            s.selected = s.cells.iter().filter(|(_, c)| c.muted).map(|(k, _)| *k).collect();
            s.cell_sel = true;
        }
        "Select Empty Regions/Cells" | "Empty Cells" => {
            let scenes = s.scenes.max(s.used_scenes());
            s.selected = empty_cells(&s.cells.keys().copied().collect(), tracks, scenes);
            s.cell_sel = true;
        }
        "Select Same-Colored Regions/Cells/Events" | "Same-Colored Cells" => {
            let sel: Vec<usize> = s.selected.iter().map(|k| k.0).collect();
            let colors: BTreeSet<[u8; 4]> = sel.into_iter().map(|t| color_key(app, t)).collect();
            let keys: BTreeSet<(usize, usize)> = app.live_loops.cells.keys().copied().filter(|k| colors.contains(&color_key(app, k.0))).collect();
            app.live_loops.selected = keys;
            app.live_loops.cell_sel = true;
        }
        _ => return false,
    }
    true
}

fn color_key(app: &App, t: usize) -> [u8; 4] {
    crate::liveloops::track_color(app, t).to_array()
}

/// The grid's empty cells (tracks × scenes without a cell).
pub fn empty_cells(full: &BTreeSet<(usize, usize)>, tracks: usize, scenes: usize) -> BTreeSet<(usize, usize)> {
    (0..tracks).flat_map(|t| (0..scenes).map(move |sc| (t, sc))).filter(|k| !full.contains(k)).collect()
}

// ---- kept beside the project ---------------------------------------------------------------------

/// The cells' playback and loop settings that the Logic file reader doesn't decode, kept next to
/// the project in Studio's settings (as follow.rs keeps follow actions).
pub fn text(cells: &std::collections::BTreeMap<(usize, usize), Cell>) -> String {
    let mut out = String::from("# Bramble Studio: Live Loops cell settings\n");
    let d = Cell::default();
    let opt = |v: Option<u32>| v.map_or("-".to_string(), |x| x.to_string());
    for ((t, sc), c) in cells {
        let same = c.muted == d.muted && c.looped == d.looped && c.reversed == d.reversed && c.transpose == d.transpose && c.quantize == d.quantize && c.quantize_start == d.quantize_start && c.trigger == d.trigger && c.play_from == d.play_from && c.start.is_none() && c.loop_start == 0 && c.loop_len.is_none() && c.q_loop == 0;
        if same {
            continue;
        }
        out += &format!(
            "cell\t{t}\t{sc}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.muted as u8, c.looped as u8, c.reversed as u8, c.transpose, c.quantize, c.quantize_start, c.trigger, c.play_from, opt(c.start), c.loop_start, opt(c.loop_len), c.q_loop
        );
    }
    out
}

pub fn apply(cells: &mut std::collections::BTreeMap<(usize, usize), Cell>, text: &str) {
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = l.split('\t').collect();
        if w.len() < 15 || w[0] != "cell" {
            continue;
        }
        let (Ok(t), Ok(sc)) = (w[1].parse::<usize>(), w[2].parse::<usize>()) else { continue };
        let Some(c) = cells.get_mut(&(t, sc)) else { continue };
        c.muted = w[3] == "1";
        c.looped = w[4] == "1";
        c.reversed = w[5] == "1";
        c.transpose = w[6].parse().unwrap_or(0);
        c.quantize = w[7].to_string();
        c.quantize_start = w[8].to_string();
        c.trigger = w[9].to_string();
        c.play_from = w[10].to_string();
        c.start = w[11].parse().ok();
        c.loop_start = w[12].parse().unwrap_or(0);
        c.loop_len = w[13].parse().ok();
        c.q_loop = w[14].parse().unwrap_or(0);
    }
}

fn path_for(project: &std::path::Path) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(project)?;
    Some(side.parent()?.parent()?.join("live-loops-cells").join(side.file_name()?))
}

/// At save (liveloops::save_into).
pub fn save(app: &App) {
    let Some(path) = app.file.as_ref().and_then(|f| path_for(&f.0)) else { return };
    let t = text(&app.live_loops.cells);
    if t.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, t);
}

/// When the grid is read (liveloops::sync).
pub fn load(app: &App, s: &mut crate::liveloops::State) {
    let Some(path) = app.file.as_ref().and_then(|f| path_for(&f.0)) else { return };
    if let Ok(t) = std::fs::read_to_string(path) {
        apply(&mut s.cells, &t);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_round_trip() {
        let mut cells = std::collections::BTreeMap::new();
        let c = Cell { muted: true, reversed: true, transpose: -5, quantize: "1/16 Note".into(), quantize_start: "2 Bars".into(), trigger: "Retrigger".into(), play_from: "Start".into(), start: Some(240), loop_start: 960, loop_len: Some(1920), q_loop: 2, length: 7680, ..Default::default() };
        cells.insert((1, 2), c.clone());
        let mut back = std::collections::BTreeMap::new();
        back.insert((1, 2), Cell { length: 7680, ..Default::default() });
        apply(&mut back, &text(&cells));
        assert_eq!(back[&(1, 2)], c);
    }

    use super::*;

    #[test]
    fn positions_print_and_parse() {
        assert_eq!(fmt_pos(0, 3840, 4, true), "1 1 1 1");
        assert_eq!(fmt_pos(3840 + 960 + 240 + 5, 3840, 4, true), "2 2 2 6");
        assert_eq!(fmt_pos(3840, 3840, 2, false), "1 0");
        assert_eq!(fmt_pos(7680, 3840, 1, false), "2");
        assert_eq!(parse_pos("2 2 2 6", 3840, true), Some(3840 + 960 + 240 + 5));
        assert_eq!(parse_pos("2", 3840, false), Some(7680));
        assert_eq!(parse_pos("1.2", 3840, true), Some(960));
        assert_eq!(parse_pos("x", 3840, true), None);
    }

    #[test]
    fn lengths_keep_the_loop_inside() {
        let mut c = Cell { length: 7680, loop_start: 3840, ..Default::default() };
        assert_eq!(c.loop_len(), 3840);
        set_length(&mut c, 3840);
        assert!(c.loop_end() <= c.length);
    }

    #[test]
    fn empty_cells_are_the_rest() {
        let full: BTreeSet<_> = [(0, 0), (1, 1)].into();
        assert_eq!(empty_cells(&full, 2, 2), [(0, 1), (1, 0)].into());
    }

    fn note(pos: u32) -> lg_core::events::Note {
        lg_core::events::Note { channel: 0, pitch: 60, velocity: 100, pos, len: 100, muted: false }
    }

    #[test]
    fn loop_settings_play() {
        use crate::liveloops::{first_pass, loop_notes, loop_notes_q, progress};
        // notes on each beat of a 2-bar cell; loop the second bar, start at beat 2
        let c = Cell { notes: (0..8).map(|k| note(k * 960)).collect(), length: 7680, start: Some(960), loop_start: 3840, ..Default::default() };
        let at: Vec<u32> = loop_notes(&c, 0, None, 0, 7680 + 3840).iter().map(|n| n.0).collect();
        // first pass: beats 2..8 (6 notes) from tick 0, then bar 2 again (4 notes)
        assert_eq!(at, vec![0, 960, 1920, 2880, 3840, 4800, 5760, 6720, 7680, 8640, 9600, 10560]);
        // Q Loop Start On: skips so the loop start falls on the bar (2880 % 3840 = 2880 skipped)
        let on = Cell { q_loop: 1, ..c.clone() };
        assert_eq!(first_pass(&on, Some(3840)), (0, 3840));
        // On, Preserve Start: waits a beat so the loop start lands on the next bar
        let keep = Cell { q_loop: 2, ..c.clone() };
        assert_eq!(first_pass(&keep, Some(3840)), (960, 960));
        assert_eq!(loop_notes_q(&keep, Some(3840), 0, None, 0, 3840)[0].0, 960);
        // the ring: through the loop
        assert!((progress(&c, 0, 2880.0 + 1920.0) - 0.5).abs() < 1e-3);
        // a one-shot plays from its start to its end, once
        let once = Cell { looped: false, ..c.clone() };
        assert_eq!(loop_notes(&once, 0, None, 0, 40000).len(), 7);
    }

    #[test]
    fn marquee_and_swap() {
        let cells: Vec<_> = crate::liveloops::marquee_cells((2, 3), (1, 1)).collect();
        assert_eq!(cells, vec![(1, 1), (1, 2), (1, 3), (2, 1), (2, 2), (2, 3)]);
        let mut s = crate::liveloops::State::default();
        s.cells.insert((0, 0), Cell { name: "A".into(), ..Default::default() });
        s.cells.insert((0, 1), Cell { name: "B".into(), ..Default::default() });
        crate::liveloops::swap_cells(&mut s, (0, 0), (0, 1));
        assert_eq!((s.cells[&(0, 0)].name.as_str(), s.cells[&(0, 1)].name.as_str()), ("B", "A"));
    }
}
