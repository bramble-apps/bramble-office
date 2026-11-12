//! The Cell Editor (Logic Pro 13: the editor below the Live Loops grid while a cell is selected:
//! E, the Editors button, View ▸ Show Editor, or a double-click on the cell). Its ruler shows
//! the cell's start marker, its loop (the yellow cycle area: loop start and end) and its end
//! marker, each dragged to change it (the middle of the loop moves the whole loop; a click in it
//! turns looping off or on); the ruler's shortcut menu has Set Start Position Here, Set Loop Start
//! Here and Set Loop End Here. Below: a MIDI cell's notes (drag to move, double-click to add or
//! remove) or an audio cell's waveform.
//!
//! The Cell inspector (cellinsp.rs) shows the same values as numbers; both edit the same cell.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_core::events::Note;

use crate::app::App;
use crate::liveloops::Cell;
use crate::look;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &[
    "Move Locators Forward by Cycle Length / Loop Forward by Loop Length",
    "Move Locators Backwards by Cycle Length / Loop Backwards by Loop Length",
    "Double Cycle/Loop Length",
    "Halve Cycle/Loop Length",
    "Set Start Position Here",
    "Set Loop Start Here",
    "Set Loop End Here",
];

const RULER_H: f32 = 26.0;
const HEAD_H: f32 = 40.0;
const KEYS_W: f32 = 54.0;
const SNAP: u32 = 240;

/// The cell the editor shows: the selected cell (the first of several).
pub fn current(app: &App) -> Option<(usize, usize)> {
    if !crate::cellinsp::active(app) {
        return None;
    }
    app.live_loops.selected.iter().copied().find(|k| app.live_loops.cells.contains_key(k))
}

pub fn active(app: &App) -> bool {
    app.bottom == crate::app::Bottom::PianoRoll && current(app).is_some()
}

/// Ruler markers (also the values the drags change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Start,
    LoopStart,
    LoopEnd,
    End,
    LoopMove,
}

/// A marker set to `at` (cell ticks); the others kept consistent.
pub fn set_marker(c: &mut Cell, m: Marker, at: u32) {
    let len = c.length.max(1);
    match m {
        Marker::Start => c.start = Some(at.min(len - 1)).filter(|s| *s != c.loop_start),
        Marker::LoopStart => {
            let end = c.loop_end();
            let s = at.min(end.saturating_sub(1)).min(len - 1);
            c.loop_start = s;
            c.loop_len = Some(end - s).filter(|l| s + l != len);
        }
        Marker::LoopEnd => {
            let e = at.clamp(c.loop_start + 1, len);
            c.loop_len = Some(e - c.loop_start).filter(|_| e != len);
        }
        Marker::End => crate::cellinsp::set_length(c, at.max(1)),
        Marker::LoopMove => {
            let l = c.loop_len();
            let s = at.min(len.saturating_sub(l));
            let start = c.start;
            c.loop_start = s;
            c.loop_len = Some(l).filter(|l| s + l != len);
            c.start = start;
        }
    }
}

/// Moves the loop by its length forward (true) or back; it stays inside the cell.
pub fn move_loop(c: &mut Cell, forward: bool) -> bool {
    let l = c.loop_len();
    let s = if forward { c.loop_start + l } else { c.loop_start.saturating_sub(l) };
    if s + l > c.length || (!forward && c.loop_start == 0) {
        return false;
    }
    let start = c.start;
    c.loop_start = s;
    c.loop_len = Some(l).filter(|l| s + l != c.length);
    c.start = start;
    true
}

/// The editor's loop commands (when the Cell Editor shows a cell).
pub fn command(app: &mut App, name: &str) -> bool {
    let Some(k) = current(app).filter(|_| active(app)) else { return false };
    let Some(mut c) = app.live_loops.cells.get(&k).cloned() else { return false };
    let ok = match name {
        "Move Locators Forward by Cycle Length / Loop Forward by Loop Length" => move_loop(&mut c, true),
        "Move Locators Backwards by Cycle Length / Loop Backwards by Loop Length" => move_loop(&mut c, false),
        "Double Cycle/Loop Length" => {
            let l = (c.loop_len() * 2).min(c.length - c.loop_start);
            let at = c.loop_start + l;
            set_marker(&mut c, Marker::LoopEnd, at);
            true
        }
        "Halve Cycle/Loop Length" => {
            let at = c.loop_start + (c.loop_len() / 2).max(1);
            set_marker(&mut c, Marker::LoopEnd, at);
            true
        }
        _ => return false,
    };
    if ok && app.live_loops.cells.get(&k) != Some(&c) {
        crate::cellinsp::checkpoint(app, name);
        app.live_loops.cells.insert(k, c);
    }
    true
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let Some(key) = current(app) else { return };
    let Some(cell) = app.live_loops.cells.get(&key).cloned() else { return };
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::header_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let id = ui.scope_id().with(("cell-editor", key));
    // header: the cell's name and its kind
    let head = Rect::from_min_size(area.min, vec2(area.width(), HEAD_H));
    let track_name = app.project.as_ref().and_then(|p| p.tracks.get(key.0)).map(|t| t.name.clone()).unwrap_or_default();
    let name = if cell.name.is_empty() { track_name.clone() } else { cell.name.clone() };
    p.text(pos2(head.min.x + 14.0, head.center().y - 7.0), Align2::LEFT_CENTER, &name, look::semibold(13.0), look::text());
    let kind = if cell.audio.is_some() { "Audio Cell" } else { "MIDI Cell" };
    p.text(pos2(head.min.x + 14.0, head.center().y + 8.0), Align2::LEFT_CENTER, format!("{kind} · {track_name}"), look::regular(11.0), look::text_dim());
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    // the time axis: the whole cell (a little more) across the width
    let body = Rect::from_min_max(pos2(area.min.x + KEYS_W, head.max.y + RULER_H), area.max);
    let ruler = Rect::from_min_max(pos2(body.min.x, head.max.y), pos2(body.max.x, body.min.y));
    let bar = app.bar_ticks().max(1);
    let span = (cell.length.max(cell.notes.iter().map(|n| n.pos + n.len).max().unwrap_or(0)).div_ceil(bar) * bar + bar).max(bar * 2);
    let px = (body.width() - 12.0).max(50.0) / span as f32;
    let x_of = |t: u32| body.min.x + 6.0 + t as f32 * px;
    let t_of = |x: f32| (((x - body.min.x - 6.0) / px).max(0.0) as u32 + SNAP / 2) / SNAP * SNAP;
    // ruler
    p.rect_filled(ruler, 0.0, look::pane_bg());
    p.hline(ruler.x_range(), ruler.max.y, Stroke::new(1.0, look::line()));
    let mut b = 0;
    while b <= span {
        let x = x_of(b);
        p.vline(x, (ruler.max.y - 10.0)..=ruler.max.y, Stroke::new(1.0, look::text_dim()));
        p.text(pos2(x + 3.0, ruler.min.y + 8.0), Align2::LEFT_CENTER, format!("{}", b / bar + 1), look::regular(11.0), look::text_dim());
        b += bar;
    }
    let (ls, le) = (cell.loop_start, cell.loop_end());
    let band = Rect::from_min_max(pos2(x_of(ls), ruler.min.y + 14.0), pos2(x_of(le), ruler.max.y - 2.0));
    let loop_col = Color32::from_rgb(0xe8, 0xc3, 0x2e);
    p.rect_filled(band, 3.0, if cell.looped { loop_col.gamma_multiply(0.85) } else { Color32::from_gray(0x60) });
    // markers: start (a flag), end (a line with a handle)
    let start_x = x_of(cell.start_pos());
    p.add(egui::Shape::convex_polygon(vec![pos2(start_x, ruler.min.y + 2.0), pos2(start_x + 9.0, ruler.min.y + 7.0), pos2(start_x, ruler.min.y + 12.0)], look::accent(), Stroke::NONE));
    p.vline(start_x, ruler.min.y + 2.0..=area.max.y, Stroke::new(1.0, look::accent().gamma_multiply(0.7)));
    let end_x = x_of(cell.length);
    p.rect_filled(Rect::from_min_size(pos2(end_x - 4.0, ruler.min.y + 2.0), vec2(8.0, 10.0)), 2.0, Color32::from_gray(0xd0));
    p.vline(end_x, ruler.min.y + 2.0..=area.max.y, Stroke::new(1.0, Color32::from_gray(0xd0)));
    // ruler interaction: which marker is under the pointer
    let resp = ui.interact(ruler, id.with("ruler"), Sense::click_and_drag());
    let hit = |x: f32, y: f32| -> Option<Marker> {
        let near = |a: f32| (x - a).abs() <= 5.0;
        if y < ruler.min.y + 13.0 {
            if near(start_x) || (x >= start_x && x <= start_x + 9.0) {
                return Some(Marker::Start);
            }
            if near(end_x) {
                return Some(Marker::End);
            }
            return None;
        }
        if near(band.min.x) {
            Some(Marker::LoopStart)
        } else if near(band.max.x) {
            Some(Marker::LoopEnd)
        } else if band.x_range().contains(x) {
            Some(Marker::LoopMove)
        } else {
            None
        }
    };
    if let Some(h) = resp.hover_pos() {
        match hit(h.x, h.y) {
            Some(Marker::LoopMove) => ui.ctx().set_cursor_icon(CursorIcon::Grab),
            Some(_) => ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal),
            None => {}
        }
    }
    if resp.drag_started() {
        if let Some(pp) = resp.interact_pointer_pos() {
            if let Some(m) = hit(pp.x, pp.y) {
                crate::cellinsp::checkpoint(app, "Change Cell Loop");
                let grab = if m == Marker::LoopMove { t_of(pp.x).saturating_sub(ls) } else { 0 };
                app.live_loops.marker_drag = Some((m as u8, key));
                ui.data_mut(|d| d.insert_temp(id.with("grab"), grab));
            }
        }
    }
    if resp.dragged() {
        if let (Some((m, k)), Some(pp)) = (app.live_loops.marker_drag, resp.interact_pointer_pos()) {
            let m = [Marker::Start, Marker::LoopStart, Marker::LoopEnd, Marker::End, Marker::LoopMove][m as usize];
            let grab: u32 = ui.data(|d| d.get_temp(id.with("grab"))).unwrap_or(0);
            if m == Marker::LoopMove {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            if let Some(c) = app.live_loops.cells.get_mut(&k) {
                set_marker(c, m, t_of(pp.x).saturating_sub(grab));
            }
        }
    }
    if resp.drag_stopped() {
        app.live_loops.marker_drag = None;
    }
    if resp.clicked() {
        // a click in the cycle area turns looping off (and on again)
        if resp.interact_pointer_pos().is_some_and(|pp| pp.y >= ruler.min.y + 13.0 && band.x_range().contains(pp.x)) {
            crate::cellinsp::checkpoint(app, "Loop Cell");
            if let Some(c) = app.live_loops.cells.get_mut(&key) {
                c.looped = !c.looped;
            }
        }
    }
    let menu_at = id.with("menu-at");
    if resp.secondary_clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            ui.data_mut(|d| d.insert_temp(menu_at, t_of(pp.x)));
        }
    }
    resp.context_menu(|ui| {
        let at: u32 = ui.data(|d| d.get_temp(menu_at)).unwrap_or(0);
        for (label, m) in [("Set Start Position Here", Marker::Start), ("Set Loop Start Here", Marker::LoopStart), ("Set Loop End Here", Marker::LoopEnd)] {
            if ui.button(label).clicked() {
                crate::cellinsp::checkpoint(app, label);
                if let Some(c) = app.live_loops.cells.get_mut(&key) {
                    set_marker(c, m, at);
                }
                ui.close();
            }
        }
    });
    // the body
    let bp = ui.painter_at(Rect::from_min_max(pos2(area.min.x, body.min.y), area.max));
    bp.rect_filled(body, 0.0, look::lane_bg());
    // the parts outside the loop dimmed
    for r in [Rect::from_min_max(body.min, pos2(x_of(ls), body.max.y)), Rect::from_min_max(pos2(x_of(le), body.min.y), body.max)] {
        bp.rect_filled(r, 0.0, Color32::from_black_alpha(50));
    }
    b = 0;
    while b <= span {
        bp.vline(x_of(b), body.y_range(), Stroke::new(1.0, look::grid_line()));
        b += bar;
    }
    let col = crate::liveloops::track_color(app, key.0);
    if let Some(path) = cell.audio.clone() {
        audio_body(app, &bp, body, &path, &cell, px, col);
    } else {
        notes_body(app, ui, &bp, area, body, key, &cell, px, col);
    }
}

fn audio_body(app: &mut App, p: &egui::Painter, body: Rect, path: &std::path::Path, cell: &Cell, px: f32, col: Color32) {
    let Some(samples) = crate::liveloops::samples(app, path) else {
        p.text(body.center(), Align2::CENTER_CENTER, "Reading audio…", look::regular(12.0), look::text_dim());
        return;
    };
    let tempo = app.tempo();
    let frames_per_tick = crate::audio::RATE as f64 * 60.0 / tempo / lg_core::events::PPQ as f64;
    let frames = samples.len() / 2;
    let mid = body.center().y;
    let h = body.height() * 0.45;
    let w = ((cell.length as f32 * px) as usize).min(body.width() as usize);
    for x in 0..w {
        let f0 = ((x as f32 / px) as f64 * frames_per_tick) as usize;
        let f1 = (((x + 1) as f32 / px) as f64 * frames_per_tick) as usize;
        if f0 >= frames {
            break;
        }
        let peak = (f0..f1.min(frames)).step_by(((f1 - f0) / 64).max(1)).map(|f| samples[f * 2].abs().max(samples[f * 2 + 1].abs())).fold(0.0f32, f32::max);
        let xx = body.min.x + 6.0 + x as f32;
        p.vline(xx, (mid - peak * h)..=(mid + peak * h), Stroke::new(1.0, col));
    }
}

#[allow(clippy::too_many_arguments)]
fn notes_body(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, area: Rect, body: Rect, key: (usize, usize), cell: &Cell, px: f32, col: Color32) {
    // the pitch range: the notes' (at least two octaves), C3 around the middle when empty
    let (lo, hi) = if cell.notes.is_empty() { (48u8, 72u8) } else {
        let lo = cell.notes.iter().map(|n| n.pitch).min().unwrap_or(60).saturating_sub(3);
        let hi = cell.notes.iter().map(|n| n.pitch).max().unwrap_or(60).saturating_add(3).min(127);
        if hi - lo < 24 { (lo.saturating_sub((24 - (hi - lo)) / 2), (lo.saturating_sub((24 - (hi - lo)) / 2) + 24).min(127)) } else { (lo, hi) }
    };
    let rows = (hi - lo + 1) as f32;
    let rh = (body.height() / rows).max(3.0);
    let y_of = |pitch: u8| body.max.y - (pitch - lo + 1) as f32 * rh;
    let pitch_at = |y: f32| (lo as f32 + ((body.max.y - y) / rh).floor()).clamp(lo as f32, hi as f32) as u8;
    let x_of = |t: u32| body.min.x + 6.0 + t as f32 * px;
    let t_of = |x: f32| (((x - body.min.x - 6.0) / px).max(0.0) as u32) / SNAP * SNAP;
    // keys
    let keys = Rect::from_min_max(pos2(area.min.x, body.min.y), pos2(body.min.x, body.max.y));
    for pitch in lo..=hi {
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        let r = Rect::from_min_max(pos2(keys.min.x, y_of(pitch)), pos2(keys.max.x, y_of(pitch) + rh));
        p.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, if black { Color32::from_gray(0x30) } else { Color32::from_gray(0xd8) });
        if pitch % 12 == 0 && rh >= 6.0 {
            p.text(pos2(r.max.x - 3.0, r.center().y), Align2::RIGHT_CENTER, crate::app::note_name(pitch), look::regular(9.5), Color32::from_gray(0x40));
        }
        if black {
            p.rect_filled(Rect::from_min_max(pos2(body.min.x, r.min.y), pos2(body.max.x, r.max.y)), 0.0, Color32::from_black_alpha(25));
        }
    }
    // notes: drag to move, double-click to remove; double-click empty space to add a 1/16
    let drag_id = ui.scope_id().with(("cell-note-drag", key));
    let mut remove: Option<usize> = None;
    for (i, n) in cell.notes.iter().enumerate() {
        let r = Rect::from_min_max(pos2(x_of(n.pos), y_of(n.pitch)), pos2(x_of(n.pos + n.len).max(x_of(n.pos) + 3.0), y_of(n.pitch) + rh - 1.0));
        p.rect_filled(r, 2.0, col);
        p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_black_alpha(90)), egui::StrokeKind::Inside);
        let resp = ui.interact(r, drag_id.with(i), Sense::click_and_drag());
        if resp.double_clicked() {
            remove = Some(i);
        }
        if resp.drag_started() {
            crate::cellinsp::checkpoint(app, "Move Note");
            let pp = resp.interact_pointer_pos().unwrap_or(r.min);
            ui.data_mut(|d| d.insert_temp(drag_id, (pp.x - r.min.x, n.pos, n.pitch)));
        }
        if resp.dragged() {
            if let (Some(pp), Some((dx, _, _))) = (resp.interact_pointer_pos(), ui.data(|d| d.get_temp::<(f32, u32, u8)>(drag_id))) {
                let pos = t_of(pp.x - dx + px * SNAP as f32 / 2.0);
                let pitch = pitch_at(pp.y);
                if let Some(nn) = app.live_loops.cells.get_mut(&key).and_then(|c| c.notes.get_mut(i)) {
                    nn.pos = pos.min(cell.length.saturating_sub(1));
                    nn.pitch = pitch;
                }
            }
        }
    }
    if let Some(i) = remove {
        crate::cellinsp::checkpoint(app, "Delete Note");
        if let Some(c) = app.live_loops.cells.get_mut(&key) {
            if i < c.notes.len() {
                c.notes.remove(i);
            }
        }
        return;
    }
    let bg = ui.interact(body, ui.scope_id().with(("cell-body", key)), Sense::click());
    if bg.double_clicked() {
        if let Some(pp) = bg.interact_pointer_pos() {
            let pos = t_of(pp.x);
            if pos < cell.length && !cell.notes.iter().any(|n| n.pitch == pitch_at(pp.y) && n.pos <= pos && pos < n.pos + n.len) {
                crate::cellinsp::checkpoint(app, "Add Note");
                if let Some(c) = app.live_loops.cells.get_mut(&key) {
                    c.notes.push(Note { channel: 0, pitch: pitch_at(pp.y), velocity: 100, pos, len: SNAP, muted: false });
                    c.notes.sort_by_key(|n| (n.pos, n.pitch));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_keep_the_loop_in_the_cell() {
        let mut c = Cell { length: 7680, ..Default::default() };
        set_marker(&mut c, Marker::LoopStart, 1920);
        assert_eq!((c.loop_start, c.loop_end()), (1920, 7680));
        set_marker(&mut c, Marker::LoopEnd, 3840);
        assert_eq!(c.loop_len(), 1920);
        set_marker(&mut c, Marker::LoopMove, 7000);
        assert_eq!((c.loop_start, c.loop_end()), (5760, 7680));
        assert!(move_loop(&mut c, false));
        assert_eq!(c.loop_start, 3840);
        assert!(!move_loop(&mut Cell { length: 3840, ..Default::default() }, true));
        set_marker(&mut c, Marker::Start, 960);
        assert_eq!(c.start_pos(), 960);
        set_marker(&mut c, Marker::End, 3840);
        assert!(c.loop_end() <= 3840);
    }
}
