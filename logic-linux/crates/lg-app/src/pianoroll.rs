//! The Piano Roll (Logic Pro 13's editor pane): tabs (Piano Roll, Score, Smart Tempo), the
//! local menus, the keyboard on the left (C3 marked) and the grid with the selected region's
//! notes, coloured by velocity as Logic does (blue soft → green → yellow → red loud). The wheel
//! scrolls the pitches, Shift + wheel the time, ⌘ + wheel or a pinch zooms; a click on a key
//! plays it.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::edit::Tool;
use crate::look;

const TABS_H: f32 = 40.0;
const RULER_H: f32 = 34.0;
const KEYS_W: f32 = 56.0;
/// The division lines show (and Smart snap uses them) once they're this far apart (points;
/// lg_core::regions::snap_grid's min_px as rollsnap.rs passes it).
const DIVISION_MIN_PX: f32 = 8.0;
/// Logic's Piano Roll bar: 40 pt (ui-tracks-with-pianoroll.png).
const LOCAL_H: f32 = 40.0;
const ROLL_INSPECTOR_W: f32 = 220.0;
const ROW: f32 = 13.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    // (the key focus, the area clicked last: focus.rs)
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    // the tabs: Piano Roll (on), Score, Smart Tempo
    let tabs = Rect::from_min_size(area.min, vec2(area.width(), TABS_H));
    crate::edpane::tabs(app, ui, tabs, crate::score::EditorTab::PianoRoll);
    // a drummer region's Manual tab: the way back to the Session Player
    if let Some((t, _)) = app.selected_region {
        if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Drummer) {
            let b = Rect::from_min_size(pos2(area.min.x + 10.0, area.min.y + 8.0), vec2(120.0, 24.0));
            if ui.put(b, egui::Button::new("‹ Session Player").corner_radius(6.0)).clicked() {
                app.drum_tab = 0;
            }
        }
    }
    let Some((ti, ri)) = app.selected_region else {
        // (a track with no regions: Logic's empty Piano Roll; nothing selected at all: the hint)
        if app.selected_track.is_some() {
            crate::edpane::empty_roll(app, ui, area, LOCAL_H, KEYS_W, RULER_H, ROW);
        } else {
            p.text(area.center(), Align2::CENTER_CENTER, "Select a MIDI region to see its notes", look::regular(14.0), look::text_dim());
        }
        return;
    };
    let Some((region, kind)) = app.project.as_ref().and_then(|pr| pr.tracks.get(ti)).and_then(|t| t.regions.get(ri).map(|r| (r.clone(), t.kind))) else { return };
    if region.audio.is_some() {
        p.text(area.center(), Align2::CENTER_CENTER, "An audio region: the Audio Track Editor isn't available yet", look::regular(14.0), look::text_dim());
        return;
    }
    // the local menus (Logic's Edit / Functions / View) and, at the left, the Piano Roll's own
    // inspector (Time Quantize, Strength, Swing, Scale Quantize, Velocity)
    let local = Rect::from_min_size(pos2(area.min.x, tabs.max.y), vec2(area.width(), LOCAL_H));
    p.rect_filled(local, 0.0, look::header_bg());
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));
    // (laid out by its width: the buttons that don't fit are in "…"; barfit.rs)
    crate::barfit::roll_bar(app, ui, local);
    let mut left = area.min.x;
    if app.roll_inspector && area.width() > 520.0 {
        let ir = Rect::from_min_max(pos2(area.min.x, local.max.y), pos2(area.min.x + ROLL_INSPECTOR_W, area.max.y));
        inspector(app, ui, ir, ti, ri);
        left = ir.max.x + 1.0;
    }
    let tabs = Rect::from_min_max(pos2(left, tabs.min.y), pos2(tabs.max.x, tabs.max.y + LOCAL_H));
    let grid_top = tabs.max.y + RULER_H;
    // the velocity lane (MIDI Draw) under the notes
    // (more lanes under it: Bramble's, rollextras.rs)
    let n_lanes = if app.midi.velocity_lane { 1 + app.rx.lanes.len() } else { 0 };
    let per_lane = crate::rollextras::lane_layout(n_lanes, area.max.y - grid_top, LANE_H);
    let lane_h = if app.midi.velocity_lane { per_lane * n_lanes as f32 } else { 0.0 };
    let lane = Rect::from_min_max(pos2(left + KEYS_W, area.max.y - lane_h), pos2(area.max.x, area.max.y - lane_h + per_lane));
    let keys = Rect::from_min_max(pos2(left, grid_top), pos2(left + KEYS_W, area.max.y - lane_h));
    let grid = Rect::from_min_max(pos2(keys.max.x, grid_top), pos2(area.max.x, area.max.y - lane_h));
    let ruler = Rect::from_min_max(pos2(grid.min.x, tabs.max.y), pos2(grid.max.x, grid_top));
    let bar_t = app.bar_ticks() as f32;
    // Collapse Mode: only the pitches the region uses, one row each (highest first)
    let rows = Rows::new(app.midi.collapse.then(|| {
        let mut v: Vec<u8> = region.notes.iter().map(|n| n.pitch).collect();
        v.sort_unstable();
        v.dedup();
        v
    }), app.roll_top, grid.min.y);
    // scrolling and zoom
    let hover = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| Rect::from_min_max(keys.min, area.max).contains(h));
    if hover {
        let (scroll, zoom, cmd, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.modifiers.shift));
        if cmd && scroll.y != 0.0 {
            app.roll_bar_w = (app.roll_bar_w * (1.0 + scroll.y * 0.003)).clamp(30.0, 3000.0);
        } else if (zoom - 1.0).abs() > 0.001 {
            app.roll_bar_w = (app.roll_bar_w * zoom).clamp(30.0, 3000.0);
        } else if shift {
            app.roll_scroll_x = (app.roll_scroll_x - scroll.y / app.roll_bar_w).max(0.0);
        } else {
            app.roll_scroll_x = (app.roll_scroll_x - scroll.x / app.roll_bar_w).max(0.0);
            if !app.midi.collapse {
                app.roll_top = (app.roll_top + scroll.y / ROW).clamp(12.0, 127.0);
            }
        }
    }
    let rows = Rows { top: if app.midi.collapse { rows.top } else { app.roll_top }, ..rows };
    let (sx, bw) = (app.roll_scroll_x, app.roll_bar_w);
    // (one time axis for drawing and clicks: rolltools.rs)
    let axis = crate::rolltools::Axis { left: grid.min.x, scroll: sx, bar_w: bw, bar_t };
    let x_of = move |tick: f32| axis.x_of(tick);
    let tick_at = move |x: f32| axis.tick_at(x);
    let ppt = bw / bar_t;
    // (a physical pixel, in points; lines and note starts are drawn in the pixel their
    // position falls in, so a click in that pixel is on them)
    let px = 1.0 / ui.ctx().pixels_per_point();
    let px_floor = move |x: f32| (x / px).floor() * px;
    let gp = ui.painter_at(grid);
    let kp = ui.painter_at(keys);
    // the keyboard's white keys under everything
    kp.rect_filled(keys, 0.0, if rows.collapsed.is_some() { look::header_bg() } else { Color32::from_gray(0xee) });
    let key = app.project.as_ref().map(|pr| pr.key.clone()).unwrap_or_else(|| "C major".into());
    let scale = scale_of(&key);
    // rows: black-key rows darker; with the scale guides, the key's notes lighter
    // (Settings ▸ View ▸ Editors ▸ Bright background: a light grid)
    let bright = crate::prefs::bright_editors();
    if bright {
        gp.rect_filled(grid, 0.0, Color32::from_gray(0xd8));
    }
    for (row, pitch) in rows.visible(grid.height()) {
        let y = rows.y_of_row(row);
        let row_r = Rect::from_min_max(pos2(grid.min.x, y - ROW), pos2(grid.max.x, y));
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        if app.midi.scale_guides {
            if scale[(pitch % 12) as usize] {
                gp.rect_filled(row_r, 0.0, look::accent().gamma_multiply(if pitch % 12 == scale_root(&key) { 0.16 } else { 0.07 }));
            } else {
                gp.rect_filled(row_r, 0.0, look::lane_alt());
            }
        } else if black {
            gp.rect_filled(row_r, 0.0, if bright { Color32::from_gray(0xc6) } else { look::lane_alt() });
        }
        if pitch % 12 == 0 || rows.collapsed.is_some() {
            gp.hline(grid.x_range(), y, Stroke::new(1.0, if rows.collapsed.is_some() { look::grid_line() } else { look::line() }));
        }
        // the keyboard (Collapse Mode: the note names, one per row)
        let key_r = Rect::from_min_max(pos2(keys.min.x, y - ROW), pos2(keys.max.x, y));
        if rows.collapsed.is_some() {
            kp.hline(keys.x_range(), y, Stroke::new(1.0, look::line()));
            kp.text(pos2(keys.max.x - 4.0, key_r.center().y), Align2::RIGHT_CENTER, crate::app::note_name(pitch), look::regular(9.5), look::text());
        } else if black {
            kp.rect_filled(Rect::from_min_max(key_r.min, pos2(keys.min.x + KEYS_W * 0.62, key_r.max.y)), 1.5, Color32::from_gray(0x1a));
        } else if matches!(pitch % 12, 0 | 5) {
            // the line under C and F (between two white keys)
            kp.hline(keys.x_range(), y, Stroke::new(1.0, Color32::from_gray(0x9a)));
        }
        if pitch % 12 == 0 && rows.collapsed.is_none() {
            kp.text(pos2(keys.max.x - 3.0, key_r.center().y), Align2::RIGHT_CENTER, format!("C{}", pitch / 12 - 2), look::semibold(9.5), Color32::from_gray(0x40));
        }
        let kr = ui.interact(key_r, ui.scope_id().with(("key", pitch)), Sense::click());
        crate::menus::context_menu(app, &kr, "pianoroll-keyboard");
        if kr.clicked() {
            preview(app, ti, pitch);
        }
    }
    // bars and beats
    let rp = ui.painter_at(ruler);
    rp.rect_filled(ruler, 0.0, look::header_bg());
    let beats = app.project.as_ref().map_or(4, |p| p.signature.0) as i32;
    let first = app.roll_scroll_x.floor() as i32;
    let division = (lg_core::events::PPQ / 4) as f32;
    let division_w = division * ppt;
    for b in first..first + (grid.width() / app.roll_bar_w).ceil() as i32 + 1 {
        for k in 0..beats {
            let x = x_of(b as f32 * bar_t + k as f32 * bar_t / beats as f32);
            let strong = k == 0;
            gp.vline(x, grid.y_range(), Stroke::new(1.0, if strong { look::line() } else { look::grid_line() }));
            // the divisions between the beats, where Smart snap puts notes (as Logic: shown
            // once they're far enough apart to snap to)
            if division_w >= DIVISION_MIN_PX {
                let n = (bar_t / beats as f32 / division).round() as i32;
                for d in 1..n {
                    let xd = x_of(b as f32 * bar_t + k as f32 * bar_t / beats as f32 + d as f32 * division);
                    gp.vline(xd, grid.y_range(), Stroke::new(1.0, look::grid_line().gamma_multiply(0.5)));
                }
            }
            if strong {
                rp.text(pos2(x + 3.0, ruler.min.y + 7.0), Align2::LEFT_CENTER, format!("{}", b + 1), look::semibold(11.0), look::text());
            } else if app.roll_bar_w > 160.0 {
                rp.text(pos2(x + 3.0, ruler.min.y + 7.0), Align2::LEFT_CENTER, format!("{} {}", b + 1, k + 1), look::regular(10.0), look::text_dim());
            }
        }
    }
    // the region's span, its header strip
    let rx0 = x_of(region.start as f32);
    let rx1 = x_of((region.start + region.length) as f32);
    let base = look::region_color(kind);
    gp.rect_filled(Rect::from_min_max(pos2(rx0, grid.min.y), pos2(rx1, grid.max.y)), 0.0, base.gamma_multiply(0.06));
    let strip = Rect::from_min_max(pos2(rx0, ruler.center().y + 1.0), pos2(rx1, ruler.max.y - 1.0));
    rp.rect_filled(strip, 3.0, look::mix(base, Color32::WHITE, 0.45));
    rp.text(pos2(strip.min.x + 6.0, strip.center().y), Align2::LEFT_CENTER, &region.name, look::regular(11.0), Color32::from_rgb(0x10, 0x30, 0x10));
    // notes: click selects (Shift adds), drag moves (snapped to 1/16; Shift: free; Option:
    // copies), the right edge resizes, double-click on the grid adds a 1/16 note, Delete removes
    // the selection; the other tools as Logic's (Pencil, Eraser, Scissors, Join, Quantize,
    // Velocity, Brush)
    let bg = ui.interact(grid, ui.scope_id().with("roll-bg"), Sense::click_and_drag());
    // (a click or a drag starts where the button went down, not where egui reports it: a few
    // points on for a drag, the release for a click)
    let press = crate::rolltools::press_pos(ui, &bg);
    let snap_at = |app: &App, ctx: &egui::Context, x: f32| axis.snap_floor(x, crate::rollsnap::grid(app, ctx, ppt), px);
    if bg.secondary_clicked() {
        if let Some(pp) = bg.interact_pointer_pos() {
            app.midi.grid_click = Some((tick_at(pp.x).max(0.0) as u32, rows.pitch_at(pp.y)));
        }
    }
    crate::menus::context_menu(app, &bg, "pianoroll-empty-grid");
    let tool = app.roll_tool;
    let mut pressed_note = None;
    let sixteenth = lg_core::events::PPQ as f32 / 4.0;
    let shift_down = ui.input(|i| i.modifiers.shift);
    // Bramble's ghost notes: the other regions' notes faintly behind
    crate::rollextras::draw_ghosts(app, &gp, grid, ti, ri, &x_of, &tick_at, &|pitch| rows.note_y(pitch), ROW);
    let mut note_rects: Vec<(usize, Rect)> = Vec::new();
    for (k, n) in region.notes.iter().enumerate() {
        let Some(y) = rows.note_y(n.pitch) else { continue };
        // (its start in the same pixel as the grid line it's on)
        let (x0, x1) = (px_floor(x_of(n.pos as f32)), px_floor(x_of((n.pos + n.len) as f32)));
        let r = Rect::from_min_max(pos2(x0, y - ROW + 1.5), pos2(x1.max(x0 + 3.0), y - 1.5));
        if !r.intersects(grid) {
            continue;
        }
        note_rects.push((k, r));
        let selected = app.selected_notes.contains(&k);
        let c = if app.artic.color_by { crate::artic::color(crate::artic::note_artic(&region, n)) } else { note_color(app.midi.note_color, n, base) };
        // (muted notes grey, as Logic's)
        let c = if n.muted { Color32::from_gray(120) } else { c };
        gp.rect_filled(r, 2.0, if selected { look::mix(c, Color32::WHITE, 0.45) } else { c });
        gp.rect_stroke(r, 2.0, Stroke::new(if selected { 1.6 } else { 1.0 }, if selected { Color32::WHITE } else { look::mix(c, Color32::BLACK, 0.45) }), egui::StrokeKind::Inside);
        // the velocity bar inside, as Logic draws it; or the note's name (View ▸ Note Labels)
        if app.midi.note_labels && r.width() > 22.0 {
            gp.text(pos2(r.min.x + 3.0, r.center().y), Align2::LEFT_CENTER, crate::app::note_name(n.pitch), look::semibold(9.0), Color32::from_gray(0x10));
        } else if r.width() > 8.0 {
            let w = (r.width() - 4.0) * n.velocity as f32 / 127.0;
            gp.hline(r.min.x + 2.0..=r.min.x + 2.0 + w, r.center().y, Stroke::new(1.0, look::mix(c, Color32::BLACK, 0.5)));
        }
        // (the Brush and the Eraser drag over notes: the grid takes their drags, rolltools.rs)
        let sense = if matches!(tool, Tool::Brush | Tool::Eraser) { Sense::click() } else { Sense::click_and_drag() };
        let resp = ui.interact(r.intersect(grid), ui.scope_id().with(("note", k)), sense);
        if resp.secondary_clicked() && !app.selected_notes.contains(&k) {
            app.selected_notes = vec![k];
        }
        crate::menus::context_menu(app, &resp, "pianoroll-note");
        let near_end = resp.hover_pos().is_some_and(|h| h.x > r.max.x - 5.0 && r.width() > 10.0);
        if resp.hovered() && matches!(tool, Tool::Pointer | Tool::Pencil) && near_end {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if resp.clicked() || resp.drag_started() {
            pressed_note = Some((k, resp.drag_started(), resp.interact_pointer_pos(), near_end));
        }
        if resp.dragged() {
            let pp = resp.interact_pointer_pos();
            match (app.midi.roll_drag.clone(), pp) {
                (Some(RollDrag::Velocity { from_y, orig, targets }), Some(pp)) => {
                    let d = ((from_y - pp.y) / 2.0).round() as i32;
                    // (Option-Shift: every selected note to the pressed note's velocity)
                    let same = ui.input(|i| i.modifiers.alt && i.modifiers.shift);
                    let to = orig.get(k).map(|o| (o.velocity as i32 + d).clamp(1, 127) as u8);
                    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                        for &i in &targets {
                            if let (Some(o), Some(n)) = (orig.get(i), rg.notes.get_mut(i)) {
                                n.velocity = match (same, to) {
                                    (true, Some(v)) => v,
                                    _ => (o.velocity as i32 + d).clamp(1, 127) as u8,
                                };
                            }
                        }
                        if let Some(n) = rg.notes.get(k) {
                            app.help_tag = Some(vec![("Velocity:".into(), format!("{}", n.velocity))]);
                        }
                    }
                }
                (Some(RollDrag::Resize { from_x, orig, targets }), Some(pp)) => {
                    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                    let dt = crate::rollsnap::length_delta(app, ui.ctx(), tick_at(pp.x) - tick_at(from_x), ppt);
                    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                        for &i in &targets {
                            if let (Some(o), Some(n)) = (orig.get(i), rg.notes.get_mut(i)) {
                                n.len = (o.len as f32 + dt).max(1.0) as u32;
                            }
                        }
                        if let Some(n) = rg.notes.get(k) {
                            app.help_tag = Some(vec![("Length:".into(), crate::app::bbdt_len(n.len, sig))]);
                        }
                    }
                }
                _ => {}
            }
            if let (Some(crate::app::Drag::Notes { from_tick, from_pitch, .. }), Some(pp)) = (app.drag, pp) {
                let tick = tick_at(pp.x);
                let pitch = rows.row_at(pp.y);
                let orig = app.drag_notes.clone();
                // (snapped by the pressed note: rollsnap.rs)
                let o_pos = orig.get(k).map_or(from_tick, |n| n.pos as f32);
                let dt = crate::rollsnap::delta(app, ui.ctx(), o_pos, tick - from_tick, ppt);
                let dp = (pitch - from_pitch).round() as i32;
                // (Settings ▸ General ▸ Editing ▸ Limit Dragging to One Direction In: Piano Roll)
                let (dt, dp) = match crate::prefs::roll_drag_axis(app, ui) {
                    Some(true) => (dt, 0),
                    Some(false) => (0.0, dp),
                    None => (dt, dp),
                };
                let sel = app.selected_notes.clone();
                let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)).and_then(|t| t.regions.get_mut(ri)) {
                    for &i in &sel {
                        if let (Some(o), Some(n)) = (orig.get(i), rg.notes.get_mut(i)) {
                            n.pos = (o.pos as f32 + dt).max(0.0) as u32;
                            n.pitch = rows.shift_pitch(o.pitch, dp);
                        }
                    }
                    // Logic's help tag: the pressed note's position, length and pitch
                    if let Some(n) = rg.notes.get(k) {
                        app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(n.pos, sig)), ("Length:".into(), crate::app::bbdt_len(n.len, sig)), ("Pitch:".into(), crate::app::note_name(n.pitch))]);
                    }
                }
            }
        }
        if resp.drag_stopped() {
            app.drag = None;
            app.rp.axis = None;
            app.midi.roll_drag = None;
            app.load_engine();
        }
    }
    if let Some((k, drag, pp, near_end)) = pressed_note {
        let shift = ui.input(|i| i.modifiers.shift);
        if shift && !drag && matches!(tool, Tool::Pointer | Tool::Pencil | Tool::Velocity) {
            if let Some(i) = app.selected_notes.iter().position(|x| *x == k) {
                app.selected_notes.remove(i);
            } else {
                app.selected_notes.push(k);
            }
        } else if !app.selected_notes.contains(&k) {
            app.selected_notes = vec![k];
        }
        let click_tick = pp.map(|p| tick_at(p.x));
        match tool {
            Tool::Eraser => app.delete_selection(),
            // Mute: the clicked note's new state for every selected note (Logic)
            Tool::Mute if !drag => {
                let on = !region.notes.get(k).is_some_and(|n| n.muted);
                crate::midifunc::edit(app, "Mute Notes", |notes, sel, _| {
                    for &i in sel.iter() {
                        if let Some(n) = notes.get_mut(i) {
                            n.muted = on;
                        }
                    }
                });
            }
            // Solo: the note plays
            Tool::Solo => {
                if let Some(n) = region.notes.get(k) {
                    preview(app, ti, n.pitch);
                }
            }
            Tool::Scissors if !drag => {
                if let Some(t) = click_tick {
                    let at = crate::rollsnap::round(app, ui.ctx(), t, ppt) as u32;
                    crate::midifunc::edit(app, "Split Notes", |notes, sel, _| {
                        lg_core::functions::split_notes(notes, sel, &[at]);
                    });
                }
            }
            Tool::Glue if !drag => {
                crate::midifunc::edit(app, "Join Notes", |notes, sel, _| {
                    lg_core::functions::join_notes(notes, sel);
                });
            }
            Tool::Quantize if !drag => {
                let (g, s, w) = (if app.roll_quantize > 0 { app.roll_quantize } else { sixteenth as u32 }, app.roll_strength, app.roll_swing);
                crate::midifunc::edit(app, "Quantize", |notes, sel, i| lg_core::functions::quantize(notes, sel, i.start, g, s, w));
            }
            Tool::Velocity if drag => {
                if let Some(pp) = pp {
                    app.checkpoint("Change Velocity");
                    let orig = app.project.as_ref().and_then(|p| p.tracks.get(ti)?.regions.get(ri)).map(|r| r.notes.clone()).unwrap_or_default();
                    app.midi.roll_drag = Some(RollDrag::Velocity { from_y: pp.y, orig, targets: app.selected_notes.clone() });
                }
            }
            Tool::Pointer | Tool::Pencil | Tool::Finger if drag && (near_end || tool == Tool::Finger) => {
                if let Some(pp) = pp {
                    app.checkpoint("Resize Notes");
                    let orig = app.project.as_ref().and_then(|p| p.tracks.get(ti)?.regions.get(ri)).map(|r| r.notes.clone()).unwrap_or_default();
                    app.midi.roll_drag = Some(RollDrag::Resize { from_x: pp.x, orig, targets: app.selected_notes.clone() });
                }
            }
            _ if drag && !matches!(tool, Tool::Mute | Tool::Solo | Tool::Zoom) => {
                if let Some(pp) = pp {
                    let copy = ui.input(|i| i.modifiers.alt);
                    app.checkpoint(if copy { "Copy Notes" } else { "Move Notes" });
                    if copy {
                        // the copies are what moves; the originals stay where they were
                        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)).and_then(|t| t.regions.get_mut(ri)) {
                            let mut new_sel = Vec::new();
                            for &i in &app.selected_notes {
                                if let Some(n) = rg.notes.get(i).copied() {
                                    rg.notes.push(n);
                                    new_sel.push(rg.notes.len() - 1);
                                }
                            }
                            app.selected_notes = new_sel;
                        }
                    }
                    app.drag_notes = app.project.as_ref().and_then(|p| p.tracks.get(ti)).and_then(|t| t.regions.get(ri)).map(|r| r.notes.clone()).unwrap_or_default();
                    app.drag = Some(crate::app::Drag::Notes { from_tick: tick_at(pp.x), from_pitch: rows.row_at(pp.y), copy });
                }
            }
            _ => {}
        }
    } else if bg.drag_started() && matches!(tool, Tool::Pointer | Tool::Marquee | Tool::Eraser) {
        // Pointer: a rubber band selects the notes it touches (Shift: adds to the selection);
        // Eraser: the notes dragged over go
        if let Some(pp) = bg.interact_pointer_pos() {
            if tool == Tool::Eraser {
                app.checkpoint("Delete Notes");
                app.midi.roll_drag = Some(RollDrag::Erase);
            } else {
                let keep = if shift_down { app.selected_notes.clone() } else { Vec::new() };
                app.midi.roll_drag = Some(RollDrag::Band { from: pp, keep });
            }
        }
    } else if bg.drag_started() && matches!(tool, Tool::Brush | Tool::Pencil) {
        // Brush (and Pencil with Shift): paint notes (or the brush pattern) along the drag at
        // the Time Quantize steps; Pencil: a new note whose length follows the drag
        if let Some(pp) = press {
            let brush = tool == Tool::Brush || shift_down;
            app.checkpoint(if brush { "Brush Notes" } else { "Create Note" });
            let pitch = rows.pitch_at(pp.y);
            let step = if app.roll_quantize > 0 { app.roll_quantize } else { sixteenth as u32 };
            if !brush {
                let at = snap_at(app, ui.ctx(), pp.x) as u32;
                let (len, vel) = app.midi.default_note;
                if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                    rg.notes.push(lg_core::events::Note { channel: 0, pitch, velocity: vel, pos: at, len, muted: false });
                    app.selected_notes = vec![rg.notes.len() - 1];
                    app.midi.roll_drag = Some(RollDrag::Pencil { index: rg.notes.len() - 1, start: at });
                }
            } else {
                // (Pencil + Shift: every note on the pitch it started on)
                app.midi.roll_drag = Some(RollDrag::Brush { pitch, step, done: Vec::new(), last: None, fixed: tool == Tool::Pencil });
            }
        }
    }
    if bg.dragged() {
        if let (Some(drag), Some(pp)) = (app.midi.roll_drag.clone(), bg.interact_pointer_pos()) {
            match drag {
                RollDrag::Pencil { index, start } => {
                    let end = ((tick_at(pp.x) / sixteenth).ceil() * sixteenth).max(start as f32 + sixteenth) as u32;
                    if let Some(n) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)?.notes.get_mut(index)) {
                        n.len = end - start;
                    }
                }
                RollDrag::Band { from, keep } => {
                    let mut v = keep.clone();
                    for k in crate::rolltools::band(from, pp, &note_rects) {
                        if !v.contains(&k) {
                            v.push(k);
                        }
                    }
                    app.selected_notes = v;
                    let r = Rect::from_two_pos(from, pp);
                    gp.rect_filled(r, 0.0, Color32::from_white_alpha(24));
                    gp.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_white_alpha(160)), egui::StrokeKind::Inside);
                }
                RollDrag::Erase => {
                    if let Some(k) = crate::rolltools::hit(pp, &note_rects) {
                        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                            if k < rg.notes.len() {
                                rg.notes.remove(k);
                            }
                        }
                        app.selected_notes.clear();
                    }
                }
                RollDrag::Brush { pitch, step, mut done, last, fixed } => {
                    // the pitch follows the pointer (Shift, or the Pencil: the first pitch)
                    let fixed = fixed || shift_down;
                    let row = if fixed { rows.row_of(pitch).map_or(rows.row_at(pp.y), |r| r + 0.5) } else { rows.row_at(pp.y) };
                    // (the first step where the button went down)
                    let (pp, row) = match (last, press) {
                        (None, Some(o)) => (o, if fixed { row } else { rows.row_at(o.y) }),
                        _ => (pp, row),
                    };
                    let steps = crate::rolltools::brush_steps(last, axis.snap_floor(pp.x, Some(step as f32), px) as u32, row, step);
                    let mut last = last;
                    for (at, row) in steps {
                        last = Some((at, row));
                        let pitch = if fixed { pitch } else { rows.pitch_at(rows.y_of_row(row.floor()) - ROW * 0.5) };
                        if done.contains(&(at, pitch)) {
                            continue;
                        }
                        done.push((at, pitch));
                        let pattern = app.midi.brush.clone();
                        let vel = app.midi.default_note.1;
                        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                            if pattern.is_empty() {
                                if !rg.notes.iter().any(|n| n.pos == at && n.pitch == pitch) {
                                    rg.notes.push(lg_core::events::Note { channel: 0, pitch, velocity: vel, pos: at, len: (step * 3 / 4).max(1), muted: false });
                                }
                            } else {
                                // the pattern repeats every span (rounded to the step), its first
                                // note on the pointer's pitch
                                let span = pattern.iter().map(|n| n.pos + n.len).max().unwrap_or(step).div_ceil(step) * step;
                                if at % span.max(1) == 0 {
                                    let p0 = pattern[0].pitch as i32;
                                    for n in &pattern {
                                        let p = (pitch as i32 + n.pitch as i32 - p0).clamp(0, 127) as u8;
                                        rg.notes.push(lg_core::events::Note { pos: at + n.pos, pitch: p, ..*n });
                                    }
                                }
                            }
                        }
                    }
                    app.midi.roll_drag = Some(RollDrag::Brush { pitch, step, done, last, fixed });
                }
                _ => {}
            }
        }
    }
    if bg.drag_stopped() {
        app.midi.roll_drag = None;
        app.load_engine();
    }
    if pressed_note.is_none() && bg.double_clicked() {
        if let Some(pp) = press {
            let tick = snap_at(app, ui.ctx(), pp.x);
            let pitch = rows.pitch_at(pp.y);
            app.checkpoint("Create Note");
            if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)).and_then(|t| t.regions.get_mut(ri)) {
                let (len, vel) = app.midi.default_note;
                rg.notes.push(lg_core::events::Note { channel: 0, pitch, velocity: vel, pos: tick.max(0.0) as u32, len, muted: false });
                app.selected_notes = vec![rg.notes.len() - 1];
            }
            app.load_engine();
        }
    } else if pressed_note.is_none() && bg.clicked() {
        if tool == Tool::Pencil {
            if let Some(pp) = press {
                // (Control-Shift: exactly where clicked, Snap ignored; else the grid line at or
                // before the Pencil's tip)
                let exact = ui.input(|i| i.modifiers.ctrl && i.modifiers.shift);
                let at = if exact { tick_at(pp.x).max(0.0) as u32 } else { snap_at(app, ui.ctx(), pp.x) as u32 };
                let pitch = rows.pitch_at(pp.y);
                let (len, vel) = app.midi.default_note;
                crate::midifunc::edit(app, "Create Note", |notes, sel, _| {
                    notes.push(lg_core::events::Note { channel: 0, pitch, velocity: vel, pos: at, len, muted: false });
                    *sel = vec![notes.len() - 1];
                });
            }
        } else if tool == Tool::Zoom {
            // Zoom: a click zooms in around it, Option-click out
            if let Some(pp) = bg.interact_pointer_pos() {
                let at = tick_at(pp.x) / bar_t;
                let k = if ui.input(|i| i.modifiers.alt) { 1.0 / 1.5 } else { 1.5 };
                app.roll_bar_w = (app.roll_bar_w * k).clamp(30.0, 3000.0);
                app.roll_scroll_x = (at - (pp.x - grid.min.x) / app.roll_bar_w).max(0.0);
            }
        } else {
            app.selected_notes.clear();
        }
    }
    // Time Handles (Functions ▸ Time Handles, ⌃T): the selection's span in the ruler; dragging
    // an end stretches or squeezes the selected notes
    if app.midi.time_handles && !app.selected_notes.is_empty() {
        if let Some((a, b)) = lg_core::functions::span(&region.notes, &app.selected_notes) {
            let (xa, xb) = (x_of(a as f32), x_of(b as f32));
            let band = Rect::from_min_max(pos2(xa, ruler.min.y + 1.0), pos2(xb, ruler.center().y));
            rp.rect_filled(band, 2.0, look::accent().gamma_multiply(0.35));
            for (right, x) in [(false, xa), (true, xb)] {
                let h = Rect::from_center_size(pos2(x, band.center().y), vec2(9.0, band.height()));
                rp.rect_filled(h, 2.0, look::accent());
                gp.vline(x, grid.y_range(), Stroke::new(1.0, look::accent().gamma_multiply(0.6)));
                let hr = ui.interact(h.expand(3.0), ui.scope_id().with(("time-handle", right)), Sense::drag());
                if hr.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if hr.drag_started() {
                    app.checkpoint("Time Handles");
                    app.midi.roll_drag = Some(RollDrag::Handle { right, span: (a, b), orig: region.notes.clone() });
                }
                if let (true, Some(RollDrag::Handle { right, span, orig }), Some(pp)) = (hr.dragged(), app.midi.roll_drag.clone(), hr.interact_pointer_pos()) {
                    let mut t = tick_at(pp.x).max(0.0);
                    if !shift_down {
                        t = (t / sixteenth).round() * sixteenth;
                    }
                    let t = t as u32;
                    let to = if right { (span.0, t.max(span.0 + 1)) } else { (t.min(span.1 - 1), span.1) };
                    let sel = app.selected_notes.clone();
                    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)?.regions.get_mut(ri)) {
                        rg.notes = orig;
                        lg_core::functions::stretch(&mut rg.notes, &sel, span, to);
                    }
                    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
                    app.help_tag = Some(vec![("Length:".into(), crate::app::bbdt_len(to.1 - to.0, sig))]);
                }
                if hr.drag_stopped() {
                    app.midi.roll_drag = None;
                    app.after_edit();
                }
            }
        }
    }
    // the velocity lane: each note's velocity as a stem; dragging across paints them
    if lane_h > 0.0 {
        velocity_lane(app, ui, lane, ti, ri, &x_of, &tick_at, left);
        crate::rollextras::lanes(app, ui, lane, &mut |app, ui, r| velocity_lane(app, ui, r, ti, ri, &x_of, &tick_at, left));
    }
    // the pointer takes the tool's shape over the grid (rolltools.rs)
    if let Some(h) = ui.input(|i| i.pointer.hover_pos()).filter(|h| grid.contains(*h)) {
        if ui.ctx().layer_id_at(h).is_none_or(|l| l.order == egui::Order::Background) {
            crate::rolltools::cursor(ui.ctx(), tool, h);
        }
    }
    // the playhead
    let ph = x_of(app.position() as f32);
    if grid.x_range().contains(ph) {
        gp.vline(ph, grid.y_range(), Stroke::new(crate::prefs::playhead_width(), look::playhead()));
    }
    p.vline(keys.max.x, keys.y_range(), Stroke::new(1.0, look::line()));
}

/// The Piano Roll's drags besides moving notes (App::midi.roll_drag).
#[derive(Clone, Debug)]
pub enum RollDrag {
    /// Velocity tool: from where, the notes as they were, which.
    Velocity { from_y: f32, orig: Vec<lg_core::events::Note>, targets: Vec<usize> },
    /// A note's right edge (the selected notes' lengths).
    Resize { from_x: f32, orig: Vec<lg_core::events::Note>, targets: Vec<usize> },
    /// A time handle: the right one or the left, the span at the start, the notes then.
    Handle { right: bool, span: (u32, u32), orig: Vec<lg_core::events::Note> },
    /// Pencil: the note made, its start.
    Pencil { index: usize, start: u32 },
    /// Brush: the pitch it started on, the step, the (step, pitch)s painted, the last step and
    /// row, the pitch kept (Pencil + Shift).
    Brush { pitch: u8, step: u32, done: Vec<(u32, u8)>, last: Option<(u32, f32)>, fixed: bool },
    /// The Pointer's rubber band: where it started, the selection it adds to (Shift).
    Band { from: egui::Pos2, keep: Vec<usize> },
    /// The Eraser dragged over notes.
    Erase,
    /// The velocity lane being painted.
    Lane,
}

const LANE_H: f32 = 96.0;

/// The rows of the grid: every pitch (scrolled by `top`, the pitch at the top edge), or in
/// Collapse Mode only the pitches used, highest first.
#[derive(Clone)]
struct Rows {
    collapsed: Option<Vec<u8>>,
    top: f32,
    y0: f32,
}

impl Rows {
    fn new(collapsed: Option<Vec<u8>>, top: f32, y0: f32) -> Rows {
        let top = collapsed.as_ref().map_or(top, |v| v.len().max(1) as f32);
        Rows { collapsed, top, y0 }
    }

    /// A row's number (counting up from the bottom: the pitch, or its place in the used ones).
    fn row_of(&self, pitch: u8) -> Option<f32> {
        match &self.collapsed {
            None => Some(pitch as f32),
            Some(v) => v.iter().position(|&p| p == pitch).map(|i| i as f32),
        }
    }

    fn y_of_row(&self, row: f32) -> f32 {
        self.y0 + (self.top - row) * ROW
    }

    fn note_y(&self, pitch: u8) -> Option<f32> {
        self.row_of(pitch).map(|r| self.y_of_row(r))
    }

    /// The row number under `y` (fractional, as the note drags use it): row `r` is drawn from
    /// `y_of_row(r) - ROW` to `y_of_row(r)`, so its middle is `r + 0.5`.
    fn row_at(&self, y: f32) -> f32 {
        self.top - (y - self.y0) / ROW
    }

    fn pitch_at(&self, y: f32) -> u8 {
        let r = self.row_at(y).floor();
        match &self.collapsed {
            None => r.clamp(0.0, 127.0) as u8,
            Some(v) if !v.is_empty() => v[(r.max(0.0) as usize).min(v.len() - 1)],
            Some(_) => 60,
        }
    }

    /// A pitch moved by `rows` rows (Collapse Mode: to another used pitch).
    fn shift_pitch(&self, pitch: u8, rows: i32) -> u8 {
        match &self.collapsed {
            None => (pitch as i32 + rows).clamp(0, 127) as u8,
            Some(v) => match v.iter().position(|&p| p == pitch) {
                Some(i) => v[(i as i32 + rows).clamp(0, v.len() as i32 - 1) as usize],
                None => pitch,
            },
        }
    }

    /// The rows on screen: (row number, pitch).
    fn visible(&self, height: f32) -> Vec<(f32, u8)> {
        match &self.collapsed {
            Some(v) => v.iter().enumerate().map(|(i, &p)| (i as f32, p)).collect(),
            None => {
                let lo = (self.top - height / ROW).floor() as i32 - 1;
                (lo.max(0)..=(self.top.ceil() as i32).min(127)).map(|p| (p as f32, p as u8)).collect()
            }
        }
    }
}

fn scale_root(key: &str) -> u8 {
    let s = scale_of(key);
    let minor = key.split_whitespace().nth(1).is_some_and(|q| q.starts_with("min"));
    // the root: the scale note whose pattern (major or natural minor) starts there
    let steps: [usize; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    (0..12).find(|&r| steps.iter().all(|&k| s[(r + k) % 12])).unwrap_or(0) as u8
}

/// A note's colour: by velocity (Logic's default), by the region's colour, or by MIDI channel.
fn note_color(by: crate::midifunc::NoteColor, n: &lg_core::events::Note, region: Color32) -> Color32 {
    match by {
        crate::midifunc::NoteColor::Velocity => velocity_color(n.velocity),
        crate::midifunc::NoteColor::Region => look::mix(region, Color32::WHITE, 0.15),
        crate::midifunc::NoteColor::Channel => {
            const C: [(u8, u8, u8); 16] = [(0xe8, 0x4c, 0x3c), (0xf0, 0x8c, 0x30), (0xe8, 0xd0, 0x3c), (0x9c, 0xd0, 0x40), (0x4c, 0xc8, 0x5a), (0x40, 0xc8, 0xa8), (0x3c, 0xb8, 0xe0), (0x3b, 0x8b, 0xf0), (0x6f, 0x6c, 0xe8), (0x9c, 0x5c, 0xe0), (0xd0, 0x5c, 0xc8), (0xe8, 0x5c, 0x90), (0xb0, 0x8c, 0x6c), (0x8c, 0x9c, 0xa8), (0xc8, 0xc8, 0xc8), (0x70, 0x70, 0x78)];
            let (r, g, b) = C[(n.channel & 15) as usize];
            Color32::from_rgb(r, g, b)
        }
    }
}

/// The velocity lane (Logic's MIDI Draw ▸ Note Velocity): a stem per note; a drag across the
/// lane sets the velocities under the pointer (the selected notes only, when some are).
#[allow(clippy::too_many_arguments)]
fn velocity_lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, ti: usize, ri: usize, x_of: &dyn Fn(f32) -> f32, tick_at: &dyn Fn(f32) -> f32, left: f32) {
    // another MIDI Draw lane chosen: controllers, pitch bend, pressure, program (arranging.rs)
    if app.arr.lane.is_some() {
        crate::arranging::event_lane(app, ui, lane, ti, ri, x_of, tick_at, left);
        return;
    }
    let p = ui.painter_at(Rect::from_min_max(pos2(left, lane.min.y), lane.max));
    p.rect_filled(Rect::from_min_max(pos2(left, lane.min.y), lane.max), 0.0, look::pane_bg());
    p.hline(left..=lane.max.x, lane.min.y, Stroke::new(1.0, look::line()));
    p.text(pos2(left + 6.0, lane.min.y + 10.0), Align2::LEFT_CENTER, "Velocity ▾", look::semibold(10.5), look::text());
    let tr = ui.interact(Rect::from_min_size(pos2(left + 4.0, lane.min.y + 2.0), vec2(70.0, 16.0)), ui.scope_id().with("lane-title-vel"), Sense::click());
    egui::Popup::menu(&tr).show(|ui| crate::arranging::lane_menu(app, ui));
    p.text(pos2(left + 6.0, lane.min.y + 24.0), Align2::LEFT_CENTER, "127", look::regular(9.0), look::text_dim());
    p.text(pos2(left + 6.0, lane.max.y - 8.0), Align2::LEFT_CENTER, "1", look::regular(9.0), look::text_dim());
    let inner = lane.shrink2(vec2(0.0, 6.0));
    let y_of = |v: u8| inner.max.y - inner.height() * v as f32 / 127.0;
    for v in [32u8, 64, 96] {
        p.hline(lane.x_range(), y_of(v), Stroke::new(1.0, look::grid_line()));
    }
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(ti)?.regions.get(ri)).cloned() else { return };
    for (k, n) in region.notes.iter().enumerate() {
        let x = x_of(n.pos as f32);
        if !lane.x_range().contains(x) {
            continue;
        }
        let sel = app.selected_notes.contains(&k);
        let c = velocity_color(n.velocity);
        let c = if sel { look::mix(c, Color32::WHITE, 0.4) } else if app.selected_notes.is_empty() { c } else { c.gamma_multiply(0.55) };
        p.vline(x, y_of(n.velocity)..=inner.max.y, Stroke::new(2.0, c));
        p.circle_filled(pos2(x, y_of(n.velocity)), 3.5, c);
    }
    let resp = ui.interact(lane, ui.scope_id().with("velocity-lane"), Sense::click_and_drag());
    if resp.drag_started() || resp.clicked() {
        app.checkpoint("Change Velocity");
        app.midi.roll_drag = Some(RollDrag::Lane);
    }
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        let v = (((inner.max.y - pp.y) / inner.height()).clamp(0.0, 1.0) * 126.0 + 1.0).round() as u8;
        // the notes within a few points of the pointer (the previous frame's position too)
        let prev = pp.x - ui.input(|i| i.pointer.delta().x);
        let (a, b) = (tick_at(pp.x.min(prev) - 4.0), tick_at(pp.x.max(prev) + 4.0));
        let only = app.selected_notes.clone();
        if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(ti)?.regions.get_mut(ri)) {
            for (k, n) in rg.notes.iter_mut().enumerate() {
                if (n.pos as f32) >= a && (n.pos as f32) <= b && (only.is_empty() || only.contains(&k)) {
                    n.velocity = v;
                }
            }
        }
        app.help_tag = Some(vec![("Velocity:".into(), format!("{v}"))]);
    }
    if resp.drag_stopped() || resp.clicked() {
        app.midi.roll_drag = None;
        app.load_engine();
    }
}

/// Logic's velocity colours: violet / blue (soft) through green and yellow to red (loud).
pub fn velocity_color(v: u8) -> Color32 {
    let stops = [(0.0, (0x6f, 0x5c, 0xe8)), (0.3, (0x3b, 0x9b, 0xf0)), (0.55, (0x55, 0xd0, 0x5a)), (0.75, (0xe8, 0xd0, 0x3c)), (1.0, (0xf0, 0x4c, 0x3c))];
    let t = v as f32 / 127.0;
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let k = (t - a) / (b - a);
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
            return Color32::from_rgb(f(ca.0, cb.0), f(ca.1, cb.1), f(ca.2, cb.2));
        }
    }
    Color32::from_rgb(0xf0, 0x4c, 0x3c)
}

/// Plays a key of the keyboard on the track's sound (a short note at the playhead, stopped
/// again a moment later).
pub(crate) fn preview(app: &mut App, track: usize, pitch: u8) {
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    if e.playing {
        return;
    }
    let pos = e.pos as u32;
    if let Some(t) = e.tracks.get_mut(track) {
        t.notes.push((pos + 1, pos + 480, pitch, 100));
        t.notes.sort_by_key(|n| n.0);
    }
    e.playing = true;
    drop(e);
    let engine = app.engine.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(350));
        let mut e = engine.lock().unwrap_or_else(|e| e.into_inner());
        e.playing = false;
        e.all_off();
        e.pos = pos as f64;
        if let Some(t) = e.tracks.get_mut(track) {
            t.notes.retain(|n| !(n.0 == pos + 1 && n.2 == pitch && n.3 == 100));
        }
    });
}

/// The Piano Roll's inspector: what's selected, Time Quantize (value, Strength, Swing, Q),
/// Scale Quantize (to the project's key, Q), Velocity of the selected notes.
fn inspector(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize, rg: usize) {
    // (its rows past the pane's bottom are cut off, buttons too: nothing floats below)
    let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(r));
    ui.set_clip_rect(r.intersect(ui.clip_rect()));
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, look::pane_bg());
    p.vline(r.max.x, r.y_range(), Stroke::new(1.0, look::line()));
    let Some(region) = app.project.as_ref().and_then(|pr| pr.tracks.get(t)?.regions.get(rg)).cloned() else { return };
    let n_sel = app.selected_notes.len();
    let what = match n_sel {
        0 => format!("{} notes", region.notes.len()),
        1 => "One note selected".to_string(),
        n => format!("{n} notes selected"),
    };
    let mut y = r.min.y + 10.0;
    p.text(pos2(r.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, what, look::semibold(13.0), look::text());
    p.text(pos2(r.min.x + 12.0, y + 26.0), Align2::LEFT_CENTER, format!("in {}", region.name), look::regular(11.5), look::text_dim());
    y += 46.0;
    p.hline(r.x_range(), y, Stroke::new(1.0, look::line()));
    y += 10.0;
    p.text(pos2(r.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, "Time Quantize", look::regular(12.5), look::text());
    y += 22.0;
    let names: Vec<&str> = crate::inspector::QUANTIZE.iter().map(|q| q.0).collect();
    let cur = crate::inspector::QUANTIZE.iter().position(|q| q.1 == app.roll_quantize).unwrap_or(0);
    if let Some(k) = crate::widgets::choice_slot(ui, Rect::from_min_size(pos2(r.min.x + 10.0, y), vec2(r.width() - 60.0, 22.0)), ui.scope_id().with("rq"), cur, &names) {
        app.roll_quantize = crate::inspector::QUANTIZE[k].1;
    }
    let qb = Rect::from_min_size(pos2(r.max.x - 44.0, y), vec2(32.0, 22.0));
    if ui.put(qb, egui::Button::new("Q")).on_hover_text("Quantize the selected notes (all, with none selected)").clicked() && app.roll_quantize > 0 {
        let (grid, strength, swing) = (app.roll_quantize, app.roll_strength, app.roll_swing);
        let sel = app.selected_notes.clone();
        app.checkpoint("Quantize");
        if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(rg)) {
            lg_core::functions::quantize(&mut x.notes, &sel, region.start, grid, strength, swing);
        }
        app.after_edit();
    }
    y += 32.0;
    for (label, v) in [("Strength", 0usize), ("Swing", 1)] {
        let val = if v == 0 { &mut app.roll_strength } else { &mut app.roll_swing };
        p.text(pos2(r.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, label, look::regular(12.0), look::text_dim());
        p.text(pos2(r.max.x - 12.0, y + 8.0), Align2::RIGHT_CENTER, format!("{}", *val), look::regular(12.0), look::text());
        y += 18.0;
        let sr = Rect::from_min_size(pos2(r.min.x + 12.0, y), vec2(r.width() - 24.0, 16.0));
        let resp = ui.interact(sr, ui.scope_id().with(("rqs", v)), Sense::click_and_drag());
        if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
            *val = (((pp.x - sr.min.x) / sr.width()).clamp(0.0, 1.0) * 100.0).round() as u8;
        }
        let f = *val as f32 / 100.0;
        p.rect_filled(Rect::from_center_size(sr.center(), vec2(sr.width(), 4.0)), 2.0, look::group_bg());
        p.rect_filled(Rect::from_min_size(pos2(sr.min.x, sr.center().y - 2.0), vec2(sr.width() * f, 4.0)), 2.0, look::accent());
        p.circle_filled(pos2(sr.min.x + sr.width() * f, sr.center().y), 7.0, Color32::from_gray(0xe8));
        y += 26.0;
    }
    p.hline(r.x_range(), y, Stroke::new(1.0, look::line()));
    y += 10.0;
    p.text(pos2(r.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, "Scale Quantize", look::regular(12.5), look::text());
    y += 22.0;
    let key = app.project.as_ref().map(|pr| pr.key.clone()).unwrap_or_else(|| "C major".into());
    p.text(pos2(r.min.x + 12.0, y + 11.0), Align2::LEFT_CENTER, &key, look::regular(12.0), look::text_dim());
    let sq = Rect::from_min_size(pos2(r.max.x - 44.0, y), vec2(32.0, 22.0));
    if ui.put(sq, egui::Button::new("Q")).on_hover_text("Move the notes onto the key's scale").clicked() {
        let scale = scale_of(&key);
        let sel = app.selected_notes.clone();
        app.checkpoint("Scale Quantize");
        if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(rg)) {
            for (i, n) in x.notes.iter_mut().enumerate() {
                if (!sel.is_empty() && !sel.contains(&i)) || scale[(n.pitch % 12) as usize] {
                    continue;
                }
                // the nearest scale note (down first, as Logic does)
                n.pitch = if n.pitch > 0 && scale[((n.pitch - 1) % 12) as usize] { n.pitch - 1 } else { (n.pitch + 1).min(127) };
            }
        }
        app.after_edit();
    }
    y += 34.0;
    p.hline(r.x_range(), y, Stroke::new(1.0, look::line()));
    y += 10.0;
    // Velocity: the selected notes' (their mean shown; dragging sets them all, keeping spreads)
    let sel: Vec<usize> = if app.selected_notes.is_empty() { (0..region.notes.len()).collect() } else { app.selected_notes.clone() };
    let mean = if sel.is_empty() { 0.0 } else { sel.iter().filter_map(|&i| region.notes.get(i)).map(|n| n.velocity as f32).sum::<f32>() / sel.len() as f32 };
    p.text(pos2(r.min.x + 12.0, y + 8.0), Align2::LEFT_CENTER, "Velocity", look::regular(12.5), look::text());
    p.text(pos2(r.max.x - 12.0, y + 8.0), Align2::RIGHT_CENTER, format!("{mean:.0}"), look::regular(12.0), look::text());
    y += 20.0;
    let sr = Rect::from_min_size(pos2(r.min.x + 12.0, y), vec2(r.width() - 24.0, 16.0));
    let resp = ui.interact(sr, ui.scope_id().with("rvel"), Sense::click_and_drag());
    if resp.drag_started() {
        app.checkpoint("Change Velocity");
    }
    if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
        let target = (((pp.x - sr.min.x) / sr.width()).clamp(0.0, 1.0) * 126.0 + 1.0).round();
        let d = target - mean;
        if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(rg)) {
            for &i in &sel {
                if let Some(n) = x.notes.get_mut(i) {
                    n.velocity = (n.velocity as f32 + d).round().clamp(1.0, 127.0) as u8;
                }
            }
        }
    }
    if resp.drag_stopped() {
        app.after_edit();
    }
    let f = mean / 127.0;
    p.rect_filled(Rect::from_center_size(sr.center(), vec2(sr.width(), 4.0)), 2.0, look::group_bg());
    p.rect_filled(Rect::from_min_size(pos2(sr.min.x, sr.center().y - 2.0), vec2(sr.width() * f, 4.0)), 2.0, velocity_color(mean as u8));
    p.circle_filled(pos2(sr.min.x + sr.width() * f, sr.center().y), 7.0, Color32::from_gray(0xe8));
}

/// Which pitch classes are in a key ("C major", "A minor"; natural minor).
pub fn scale_of(key: &str) -> [bool; 12] {
    let mut it = key.split_whitespace();
    let root = it.next().unwrap_or("C");
    let minor = it.next().is_some_and(|q| q.starts_with("min"));
    let names = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
    let alt = ["C", "D♭", "D", "D♯", "E", "F", "G♭", "G", "G♯", "A", "A♯", "B"];
    let r = names.iter().position(|n| *n == root).or_else(|| alt.iter().position(|n| *n == root)).unwrap_or(0);
    let steps: [usize; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    let mut out = [false; 12];
    for s in steps {
        out[(r + s) % 12] = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_in_a_drawn_row_is_that_rows_pitch() {
        // every vertical scroll (fractional too) and both row layouts
        for top in [127.0, 84.0, 72.5, 60.25, 30.0] {
            let rows = Rows::new(None, top, 200.0);
            for (row, pitch) in rows.visible(400.0) {
                let y = rows.y_of_row(row);
                // the row is drawn from y - ROW to y: its middle, and just inside both edges
                for yy in [y - ROW * 0.5, y - ROW + 0.5, y - 0.5] {
                    assert_eq!(rows.pitch_at(yy), pitch, "top {top} row {row} y {yy}");
                }
            }
        }
        let rows = Rows::new(Some(vec![40, 52, 67]), 0.0, 100.0);
        for (row, pitch) in rows.visible(400.0) {
            assert_eq!(rows.pitch_at(rows.y_of_row(row) - ROW * 0.5), pitch);
        }
        // a note is drawn in its pitch's row
        let rows = Rows::new(None, 80.0, 0.0);
        let y = rows.note_y(64).unwrap();
        assert_eq!(rows.pitch_at(y - ROW * 0.5), 64);
    }
}
