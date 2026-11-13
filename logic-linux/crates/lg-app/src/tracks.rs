//! The tracks area (Logic Pro 13): a local menu bar (Edit, Functions, View), the track list
//! with headers (number, icon, name, Mute / Solo, volume) and, beside it, the ruler (bars, the
//! cycle area, the playhead) over the lanes where regions sit. Regions show their notes (MIDI)
//! or waveform (audio) in their track's colour.
//!
//! Interactions (as Logic): click a region to select it (and its track), drag to move it (it
//! snaps to beats; Shift: no snapping), double-click it to open the Piano Roll; click or drag in
//! the ruler to move the playhead, drag in its top half to set the cycle; the wheel scrolls the
//! tracks, Shift + wheel (or a sideways swipe) scrolls the time, ⌘ + wheel or a pinch zooms.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::{App, Drag};
use crate::edit::Tool;
use crate::look;

/// The track headers' width: Logic's minimum 265 pt, up to 547 (RESIZING.txt).
pub const HEADER_W: f32 = 265.0;
/// The headers' width in a new window: Logic's 325 pt (win-tracks-1-instrument-track.png).
pub const HEADER_W_DEFAULT: f32 = 325.0;
const GLOBAL_ROW: f32 = 28.0;
/// Logic 13's global tracks (G) and their heights: the Tempo lane is tall, with its scale.
/// The arrangement markers the (+) adds, in Logic's order.
pub const SECTIONS: [&str; 8] = ["Intro", "Verse", "Chorus", "Verse", "Chorus", "Bridge", "Chorus", "Outro"];
/// A track's default height: Logic's 64 pt rows (the last point is the line under the row).
pub const ROW_H: f32 = 64.0;
/// The Transposition global track's height.
const TRANSPOSITION_H: f32 = 44.0;
/// The local menu bar (Edit, Functions, View) and the ruler: Logic's 42 pt bar, a ruler of two
/// 20 pt strips and the 1 pt seam under it.
const LOCAL_H: f32 = 42.0;
const RULER_H: f32 = 41.0;
/// The room before bar 1 in the lanes and the ruler.
const LANE_INSET: f32 = 8.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    crate::trackarea::sync(app);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::ta_lane());
    // ---- the local menu bar
    let local = Rect::from_min_size(area.min, vec2(area.width(), LOCAL_H));
    p.rect_filled(local, 0.0, look::header_bg());
    p.hline(local.x_range(), local.max.y - 0.5, Stroke::new(1.0, look::ta_seam()));
    // (laid out by its width: Logic's full set when it fits, the rest in "…"; barfit.rs)
    crate::barfit::tracks_bar(app, ui, local);
    // ---- the list header and the ruler
    let hw = app.header_w.clamp(HEADER_W, 547.0);
    let head = Rect::from_min_size(pos2(area.min.x, local.max.y), vec2(hw, RULER_H));
    let grid_w = crate::liveloops::grid_width(app, area.width() - hw);
    let ruler = Rect::from_min_max(pos2(head.max.x + grid_w, local.max.y), pos2(area.max.x, local.max.y + RULER_H));
    p.rect_filled(head, 0.0, look::ta_header());
    p.rect_filled(ruler, 0.0, look::ta_ruler());
    // the seam under the ruler and the header row (Logic's black 1 pt line)
    p.hline(head.min.x..=ruler.max.x, ruler.max.y - 0.5, Stroke::new(1.0, look::ta_seam()));
    // Logic's row over the headers: + (New Tracks), Duplicate Track, S (global tracks), 32 pt
    // buttons 43 pt apart from 10 pt in; at the right, the region/track catch button
    let hb = |i: f32| Rect::from_min_size(pos2(head.min.x + 10.0 + i * 43.0, head.min.y + (RULER_H - 1.0 - 24.0) / 2.0), vec2(32.5, 24.0));
    let tips = ["New Tracks (⌥⌘N)", "New Track with Duplicate Settings (⌘D)", "Show/Hide Global Tracks (G)"];
    for (i, tip) in tips.into_iter().enumerate() {
        let r = hb(i as f32);
        let resp = ui.interact(r, ui.scope_id().with(("hbtn", i)), Sense::click()).on_hover_text(tip);
        let on = i == 2 && app.global_tracks;
        p.rect_filled(r, 6.0, if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::header_pill() });
        let c = r.center();
        let col = if on { Color32::WHITE } else { look::text() };
        match i {
            0 => crate::glyphs::paint(&p, r, crate::glyphs::ph::PLUS, 17.0, col),
            1 => crate::glyphs::paint(&p, r, crate::glyphs::ph::COPY, 17.0, col),
            _ => {
                p.text(c, Align2::CENTER_CENTER, "S", look::bold(13.0), col);
            }
        }
        if i == 1 {
            crate::trackhdr::duplicate_button(app, ui, &resp);
        }
        if resp.clicked() {
            match i {
                0 => crate::trackhdr::add_clicked(app, ui),
                1 => {}
                _ => app.global_tracks = !app.global_tracks,
            }
        }
    }
    // (Clear/Recall Solo and the Hide button, once they are in use)
    crate::trackhdr::top_row(app, ui, head);
    // the global tracks (G): Marker, Tempo, Signature, between the ruler and the tracks
    let global_h = if app.global_tracks { crate::globals::total_h(app, TRANSPOSITION_H) } else { 0.0 };
    let global = Rect::from_min_max(pos2(area.min.x, ruler.max.y), pos2(area.max.x, ruler.max.y + global_h));
    // (Logic leaves 8 pt before bar 1: the lanes' time origin is that far in from the divider;
    // the strip shows the rows' backgrounds and the playhead's head at bar 1 in full)
    let lanes = Rect::from_min_max(pos2(ruler.min.x + LANE_INSET, global.max.y), area.max);
    let lanes_bg = Rect::from_min_max(pos2(ruler.min.x, global.max.y), area.max);
    crate::barfit::auto_zoom(app, lanes.height(), lanes.width());
    let headers = Rect::from_min_max(pos2(area.min.x, global.max.y), pos2(head.max.x, area.max.y));
    crate::browser::note_lanes(app, Rect::from_min_max(pos2(area.min.x, lanes.min.y), lanes.max));
    if app.global_tracks {
        global_tracks(app, ui, global, lanes.min.x);
    }
    let bar_t = app.bar_ticks() as f32;
    let bw0 = app.bar_w;
    let x_of0 = move |tick: f32, scroll: f32| lanes.min.x + (tick / bar_t - scroll) * bw0;
    let _ = x_of0;
    // ---- scrolling and zoom (over the tracks)
    let hover = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| Rect::from_min_max(head.min, area.max).contains(h)) && !kn_ui::pointer_over_window(ui.ctx());
    if hover {
        let (scroll, zoom, cmd, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command, i.modifiers.shift));
        if cmd && scroll.y != 0.0 {
            zoom_at(app, 1.0 + scroll.y * 0.003, ui.input(|i| i.pointer.hover_pos()).map_or(lanes.min.x, |h| h.x), lanes.min.x);
            if app.playing() {
                app.catch_off = true;
            }
        } else if (zoom - 1.0).abs() > 0.001 {
            zoom_at(app, zoom, ui.input(|i| i.pointer.hover_pos()).map_or(lanes.min.x, |h| h.x), lanes.min.x);
        } else {
            let (dx, dy) = if shift { (scroll.y, 0.0) } else { (scroll.x, scroll.y) };
            app.scroll_x = (app.scroll_x - dx / app.bar_w).max(crate::projend::min_scroll(app));
            // scrolling by hand while playing turns Catch off until playback stops or ` is
            // pressed, as Logic does (otherwise the view jumps back to the playhead)
            if dx != 0.0 && app.playing() {
                app.catch_off = true;
            }
            let total = crate::trackarea::total_h(&crate::trackarea::rows(app));
            app.scroll_y = (app.scroll_y - dy).clamp(0.0, (total - lanes.height() + 40.0).max(0.0));
            crate::scrollnum::scrolled(app, lanes, dx, dy);
        }
    }
    let scroll = app.scroll_x;
    let bw = app.bar_w;
    let px_per_tick = bw / bar_t;
    let x_of = move |tick: f32, scroll: f32| lanes.min.x + (tick / bar_t - scroll) * bw;
    // ---- ruler: bars, the cycle area, the playhead. Logic's ruler is two 20 pt strips: the
    // upper one the bar numbers and the cycle band, the lower one the ticks (bars full height,
    // beats 8 pt) and the playhead's head
    let rp = ui.painter_at(ruler);
    let first = scroll.floor() as i32;
    let visible = (ruler.width() / app.bar_w).ceil() as i32 + 1;
    let step = if app.bar_w < 18.0 { 8 } else if app.bar_w < 34.0 { 4 } else if app.bar_w < 60.0 { 2 } else { 1 };
    let upper_r = Rect::from_min_max(ruler.min, pos2(ruler.max.x, ruler.min.y + 20.0));
    let lower_r = Rect::from_min_max(pos2(ruler.min.x, upper_r.max.y), pos2(ruler.max.x, ruler.max.y - 1.0));
    let (ca, cb) = app.cycle;
    let cycle_on = app.cycle_on();
    // the cycle area: the upper strip's band (grey while Cycle is off, yellow while on)
    let crect = Rect::from_min_max(pos2(x_of(ca as f32, scroll), upper_r.min.y), pos2(x_of(cb as f32, scroll), upper_r.max.y));
    if cb > ca {
        rp.rect_filled(crect, 0.0, if cycle_on { look::cycle() } else { look::ta_cycle_off() });
    }
    rp.hline(lower_r.x_range(), lower_r.min.y + 0.5, Stroke::new(1.0, Color32::from_black_alpha(110)));
    // (the Project Start and End triangles: projend.rs, after the bars)
    let beats = app.project.as_ref().map_or(4, |p| p.signature.0.max(1)) as f32;
    let num_col = |x: f32| if cycle_on && x > crect.min.x && x < crect.max.x { Color32::from_gray(0x20) } else { Color32::from_gray(0xf7) };
    // (the Project End marker sits right of its bar line: that bar's number moves past it)
    let end_x = x_of(crate::projend::end(app) as f32, scroll);
    let bar_mark = |rp: &egui::Painter, x: f32, n: i32, numbered: bool| {
        if numbered {
            rp.vline(x, upper_r.min.y + 2.0..=upper_r.max.y, Stroke::new(1.0, look::ta_ruler_bar()));
            let nx = if (x - end_x).abs() < 0.5 { x + 11.0 } else { x + 4.0 };
            rp.text(pos2(nx, upper_r.min.y - 0.5), Align2::LEFT_TOP, format!("{n}"), look::regular(13.0), num_col(x + 4.0));
        }
        rp.vline(x, lower_r.min.y..=lower_r.max.y, Stroke::new(1.0, look::ta_ruler_tick()));
    };
    // (the beats between the bars, where there is room)
    let beat_w = app.bar_w / beats;
    // (signature changes: the bars from the signature map)
    let sig_bars = crate::arranging::bar_lines(app, (first.max(0) as f32 * bar_t) as u32, ((first + visible).max(0) as f32 * bar_t) as u32);
    if let Some(bars) = &sig_bars {
        for &(t, n) in bars {
            bar_mark(&rp, x_of(t as f32, scroll), n as i32, (n as i32 - 1) % step == 0);
        }
    }
    for b in first..first + visible {
        // (with signature changes the map draws from bar 1; bars before it, from a Project Start
        // moved earlier, count back in the first signature)
        if sig_bars.is_some() && b >= 0 {
            break;
        }
        let x = x_of(b as f32 * bar_t, scroll);
        bar_mark(&rp, x, b + 1, b.rem_euclid(step) == 0);
        if beat_w >= 6.0 {
            for k in 1..beats as i32 {
                rp.vline(x + k as f32 * beat_w, lower_r.max.y - 8.0..=lower_r.max.y, Stroke::new(1.0, look::ta_ruler_tick()));
            }
        }
    }
    // the ruler (Logic, DRAGS.txt): the upper strip (bar numbers) draws a cycle area and turns
    // Cycle on (dragging inside the area moves it; a click toggles Cycle); the lower strip
    // moves the playhead
    let r_resp = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
    crate::menus::context_menu(app, &r_resp, "ruler");
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    if let Some(pp) = r_resp.interact_pointer_pos() {
        let tick = ((pp.x - lanes.min.x) / bw + scroll) * bar_t;
        let beat = bar_t / sig.0.max(1) as f32;
        let snap = |t: f32| ((t / beat).round() * beat).max(0.0);
        let upper = |y: f32| y < ruler.min.y + RULER_H / 2.0;
        if r_resp.drag_started() {
            let from = r_resp.interact_pointer_pos().map_or(pp, |_| ui.input(|i| i.pointer.press_origin()).unwrap_or(pp));
            let from_tick = ((from.x - lanes.min.x) / bw + scroll) * bar_t;
            app.drag = if upper(from.y) { Some(Drag::Cycle { from_tick, start: app.cycle }) } else { Some(Drag::Playhead) };
        }
        // a cycle dragged down (to drop it on the Marker track) keeps its place; Shift lets it
        // move sideways too
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pp);
        let down_drag = pp.y > ruler.max.y && (pp.y - origin.y).abs() > (pp.x - origin.x).abs() && !ui.input(|i| i.modifiers.shift);
        match app.drag {
            Some(Drag::Cycle { start, .. }) if r_resp.dragged() && down_drag => {
                app.set_cycle(start);
            }
            Some(Drag::Cycle { from_tick, start }) if r_resp.dragged() => {
                let inside = from_tick >= start.0 as f32 && from_tick <= start.1 as f32 && (start.1 - start.0) > 0;
                let c = if inside {
                    let d = snap(tick - from_tick + start.0 as f32) as u32;
                    (d, d + (start.1 - start.0))
                } else {
                    (snap(from_tick.min(tick)) as u32, snap(from_tick.max(tick)) as u32)
                };
                if c.1 > c.0 {
                    app.set_cycle(c);
                    if !app.cycle_on() {
                        app.toggle_cycle();
                    }
                    app.help_tag = Some(vec![
                        ("Cycle Range".into(), String::new()),
                        ("Left:".into(), crate::app::bbdt(c.0, sig)),
                        ("Length:".into(), crate::app::bbdt_len(c.1 - c.0, sig)),
                        ("Right:".into(), crate::app::bbdt(c.1, sig)),
                    ]);
                }
            }
            Some(Drag::Playhead) if r_resp.dragged() => {
                app.locate(snap(tick) as f64);
            }
            _ => {
                if r_resp.clicked() {
                    if upper(pp.y) && ui.input(|i| i.modifiers.shift) && cb > ca {
                        // Shift-click: the nearer cycle border to the click
                        let t = snap(tick) as u32;
                        app.set_cycle(if t.abs_diff(ca) <= t.abs_diff(cb) { (t.min(cb.saturating_sub(1)), cb) } else { (ca, t.max(ca + 1)) });
                    } else if upper(pp.y) && tick >= ca as f32 && tick <= cb as f32 {
                        app.toggle_cycle();
                    } else if !upper(pp.y) {
                        app.locate(snap(tick) as f64);
                    }
                }
            }
        }
    }
    if r_resp.drag_stopped() {
        if let (Some(Drag::Cycle { .. }), Some(pp)) = (app.drag.clone(), ui.input(|i| i.pointer.interact_pos())) {
            crate::markers::cycle_dropped(app, pp, ((pp.x - lanes.min.x) / bw + scroll) * bar_t);
        }
        app.drag = None;
    }
    {
        let xo = |t: f32| x_of(t, scroll);
        let to = |x: f32| ((x - lanes.min.x) / bw + scroll) * bar_t;
        let shown = !(app.global_tracks && crate::globals::rows(app).iter().any(|r| r.0 == crate::globals::MARKER));
        crate::markers::ruler(app, ui, ruler, &xo, &to, shown);
        // the Project End marker (projend.rs)
        crate::projend::ruler(app, ui, upper_r, &xo, &to);
    }
    // Autopunch's punch locators (the red strip)
    crate::recording::ruler(app, ui, ruler, lanes.min.x, scroll * bar_t, px_per_tick);
    // the marquee ruler (View ▸ Marquee Ruler): a strip along the ruler's bottom
    crate::regionparams::marquee_ruler(app, ui, ruler, lanes.min.x, scroll * bar_t, px_per_tick);
    // ---- rows: headers and lanes
    let Some(proj) = app.project.clone() else { return };
    let rows = crate::trackarea::rows(app);
    let rows_h = crate::trackarea::total_h(&rows);
    // (Delete / Backspace: App::keys sends Edit ▸ Delete to the area with key focus, focus.rs)
    let hp = ui.painter_at(headers);
    let lp = ui.painter_at(lanes);
    let bgp = ui.painter_at(lanes_bg);
    hp.rect_filled(headers, 0.0, look::ta_header());
    let mut region_pressed = None;
    // (registered first, so regions on top of it take their own clicks)
    let empty = ui.interact(lanes, ui.scope_id().with("lanes-bg"), Sense::click());
    let below_tracks = empty.interact_pointer_pos().or(ui.ctx().pointer_hover_pos()).is_some_and(|pp| pp.y > headers.min.y + rows_h - app.scroll_y);
    crate::menus::context_menu(app, &empty, if below_tracks { "tracks-empty-below" } else { "tracks-area-empty-lane" });
    // a header being dragged: where it would land (the others slide to make room; the lanes
    // follow on release, as Logic's)
    let reorder = app.ta.reorder.and_then(|(t, y0)| {
        let from = rows.iter().position(|r| r.t == t)?;
        let py = ui.input(|i| i.pointer.hover_pos())?.y;
        let rel = py - headers.min.y + app.scroll_y;
        let to = rows.iter().position(|r| rel < r.y + r.h / 2.0).unwrap_or(rows.len());
        Some((from, to, py - y0))
    });
    // (the bar lines: drawn on each track's lane only, as Logic's)
    let bar_xs: Vec<f32> = match &sig_bars {
        Some(bars) => bars.iter().map(|b| x_of(b.0 as f32, scroll)).collect(),
        None => (first..first + visible).map(|b| x_of(b as f32 * bar_t, scroll)).collect(),
    };
    // (the rows' widgets are clipped below the "+ / duplicate / S" row and the ruler, as the
    // lanes are: a track scrolled half out of view doesn't draw over them)
    let rows_clip_saved = ui.clip_rect();
    ui.set_clip_rect(rows_clip_saved.intersect(Rect::from_min_max(pos2(area.min.x, headers.min.y), area.max)));
    for (vis, row_info) in rows.iter().enumerate() {
        let i = row_info.t;
        let t = &proj.tracks[i];
        // (the automation subtracks below the track's own row: autoext.rs)
        let sub = crate::autoext::sub_h(app, i);
        let rh = row_info.h - sub;
        let y = headers.min.y + row_info.y - app.scroll_y;
        if y + rh < headers.min.y || y > headers.max.y {
            continue;
        }
        // (while a header is dragged: its row floats with the pointer, the others slide)
        let slide = match reorder {
            Some((from, to, _)) if vis != from && to > from && vis > from && vis < to => -rows[from].h,
            Some((from, to, _)) if vis != from && to < from && vis >= to && vis < from => rows[from].h,
            Some((from, _, dy)) if vis == from => dy,
            _ => 0.0,
        };
        let row = Rect::from_min_size(pos2(headers.min.x, y + slide), vec2(hw, rh));
        let lane = Rect::from_min_max(pos2(lanes.min.x, y), pos2(lanes.max.x, y + rh));
        let lane_bg = Rect::from_min_max(pos2(lanes_bg.min.x, y), lane.max);
        let selected = app.selected_track == Some(i) || app.ta.selected.contains(&i);
        // (Logic: plain rows on the column's grey, the selected one lighter; a hairline under
        // each header, a darker line under each lane; no alternating lanes)
        if selected {
            hp.rect_filled(row, 0.0, look::ta_header_selected());
            bgp.rect_filled(lane_bg, 0.0, look::ta_lane_selected());
        }
        if reorder.is_some_and(|r| r.0 == vis) {
            hp.rect_filled(row, 0.0, look::ta_header_selected().gamma_multiply(1.3));
        }
        hp.hline(row.x_range(), row.max.y - 0.5, Stroke::new(1.0, look::ta_header_sep()));
        bgp.hline(lane_bg.x_range(), lane.max.y - 0.5, Stroke::new(1.0, look::ta_lane_line()));
        // the bar lines over the lane
        for &x in &bar_xs {
            bgp.vline(x, lane.min.y..=lane.max.y - 1.0, Stroke::new(1.0, look::ta_bar_line()));
        }
        // the header: number, icon, name, M S, volume (press on it and drag up or down: the
        // track moves)
        // (Logic's header at its default 64 pt: the name's caps from 12 pt, the controls' centre
        // line at 41 pt; taller or shorter rows keep the block centred)
        // (a tall row keeps its name, icon and controls at the top, as Logic's: ui-take-folder-
        // expanded.png; a short one moves them up a little)
        let dy = ((rh - ROW_H) / 2.0).clamp(-10.0, 0.0);
        let row_resp = ui.interact(row, ui.scope_id().with(("trackrow", i)), Sense::click_and_drag());
        // (Shift-click: the track leaves or rejoins the marquee)
        if row_resp.clicked() && ui.input(|i| i.modifiers.shift) {
            crate::regionparams::marquee_toggle_track(app, i);
        }
        if row_resp.clicked() {
            let (shift, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
            crate::trackarea::click_track(app, i, shift, cmd);
            // (Settings ▸ Select regions on track selection; Option-click always)
            if !shift && !cmd {
                // (Option-click: the track alone, the region selection stays)
                if !ui.input(|i| i.modifiers.alt) {
                    crate::prefs::track_clicked(app, i, false);
                }
            }
        }
        if row_resp.secondary_clicked() && !app.ta.selected.contains(&i) {
            crate::trackarea::click_track(app, i, false, false);
        }
        if row_resp.drag_started() && ui.input(|i| i.pointer.press_origin()).is_some_and(|o| o.x > row.min.x + 60.0 && o.y < row.min.y + dy + 30.0) {
            app.ta.reorder = Some((i, ui.input(|i| i.pointer.press_origin()).map_or(row.min.y, |o| o.y)));
            app.selected_track = Some(i);
        }
        if row_resp.drag_stopped() {
            if let (Some((t, _)), Some((_, to, _))) = (app.ta.reorder.take(), reorder) {
                crate::trackarea::move_track(app, t, to);
            }
        }
        if row_resp.dragged() && app.ta.reorder.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        crate::menus::context_menu(app, &row_resp, "track-header");
        let large = crate::trackarea::large_icons(app) && rh >= 44.0;
        let dim = t.extra.hidden;
        // (the colour bar, the groove track's star, the zoom corner; the number: trackhdr.rs)
        let edge = crate::trackhdr::left_edge(app, ui, &hp, row, i);
        crate::trackhdr::number(app, ui, &hp, row, i, edge, dim);
        let indent = crate::trackarea::header_extras(app, ui, row, i) + edge;
        let stack_shift = if t.extra.stack.is_some() { 12.0 } else { 0.0 };
        // (Logic: a 26 pt icon centred 47 pt in and 30 pt down)
        let top_h = rh.min(ROW_H);
        let isz = if large { (top_h - 8.0).clamp(34.0, 64.0) } else { 26.0 };
        let icon_y = if large { row.min.y + top_h / 2.0 } else { row.min.y + 30.0 + dy };
        let icon = Rect::from_center_size(pos2(row.min.x + 33.75 + indent + stack_shift + isz / 2.0, icon_y), vec2(isz, isz));
        // (Track Icons off: none; an icon chosen in the Icon palette over the patch's own)
        let drew = crate::trackhdr::icon(app, ui, &hp, icon, i, large);
        if drew == Some(false) {
            match t.extra.stack {
                Some(st) => crate::trackarea::stack_icon(&hp, icon, st.kind),
                None if large => track_icon_large(&hp, icon, t),
                None => track_icon(&hp, icon, t),
            }
        }
        let name_x = if drew.is_some() { icon.max.x + 12.25 } else { icon.min.x };
        // the name: double-click to rename (Return keeps it, Escape doesn't)
        let alt_w = if app.ta.show_alternatives || !t.extra.alternatives.is_empty() { 74.0 } else { 34.0 };
        let name_r = Rect::from_min_size(pos2(name_x - 2.0, row.min.y + dy + 6.0), vec2((row.max.x - name_x - alt_w).max(40.0), 20.0));
        match app.renaming.as_mut() {
            Some((ti, text)) if *ti == i => {
                let te = ui.put(name_r, egui::TextEdit::singleline(text).font(look::semibold(13.0)));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let (cancel, new) = (ui.input(|i| i.key_pressed(egui::Key::Escape)), text.trim().to_string());
                    app.renaming = None;
                    if !cancel && !new.is_empty() {
                        app.checkpoint("Rename Track");
                        // (several selected tracks take the name: trackops.rs)
                        crate::trackops::renamed(app, i, &new);
                    }
                }
            }
            _ => {
                hp.text(pos2(name_x, row.min.y + dy + 16.0), Align2::LEFT_CENTER, ellipsize(&t.name, (((row.max.x - name_x - alt_w) / 7.0) as usize).clamp(6, 40)), look::regular(13.5), if dim { look::text_dim() } else { look::text() });
                if let Some(extra) = crate::trackhdr::name_column(app, i) {
                    hp.text(pos2(row.max.x - alt_w - 2.0, row.min.y + dy + 16.0), Align2::RIGHT_CENTER, ellipsize(&extra, 16), look::regular(10.5), look::text_dim());
                }
                if ui.interact(name_r, ui.scope_id().with(("trackname", i)), Sense::click()).double_clicked() {
                    app.renaming = Some((i, t.name.clone()));
                }
            }
        }
        // the header's buttons, as its configuration shows them (trackhdr.rs): On/Off, M, S,
        // Protect, Freeze, R, I
        let by = row.min.y + dy + 32.0;
        let pressed = crate::trackhdr::buttons(app, ui, row, i, name_x, by);
        let mut changed = pressed.changed;
        let (mute, solo) = (pressed.mute, pressed.solo);
        // (the controls after the buttons start where they end)
        let bx = pressed.end_x - 98.0;
        // the pan knob at the right (or the send it is set to)
        if app.tm.hdr.pan && !crate::trackhdr::send_knob(app, ui, &hp, row, by, i) {
        let mut pan = t.pan;
        // (Logic's 24.5 pt knob, 11.5 pt from the right: a dark ring with the value arc in
        // green, a grey cap and a white pointer)
        let kc = pos2(row.max.x - 23.75, by + 8.0);
        let kresp = ui.interact(Rect::from_center_size(kc, vec2(24.5, 24.5)), ui.scope_id().with(("hpan", i)), Sense::click_and_drag()).on_hover_text(format!("Pan {:+.0}", pan * 64.0));
        hp.circle_filled(kc, 12.25, Color32::from_black_alpha(190));
        let ang = std::f32::consts::PI * (1.5 + 0.75 * pan);
        {
            let up = std::f32::consts::PI * 1.5;
            let (a0, a1) = if (ang - up).abs() < 0.08 { (up - 0.08, up + 0.08) } else { (up.min(ang), up.max(ang)) };
            let n = 16;
            let pts: Vec<egui::Pos2> = (0..=n).map(|k| { let a = a0 + (a1 - a0) * k as f32 / n as f32; kc + vec2(a.cos(), a.sin()) * 10.25 }).collect();
            hp.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(0x79, 0xd4, 0x76))));
        }
        hp.circle_filled(kc, 8.5, Color32::from_gray(0x74));
        let dir = vec2(ang.cos(), ang.sin());
        hp.line_segment([kc + dir * 3.0, kc + dir * 8.0], Stroke::new(1.5, Color32::WHITE));
        if kresp.dragged() {
            pan = (pan - kresp.drag_delta().y / 100.0).clamp(-1.0, 1.0);
            app.help_tag = Some(vec![("Pan".into(), format!("{:+.0}", pan * 64.0))]);
        }
        if kresp.double_clicked() || (kresp.clicked() && ui.input(|i| i.modifiers.alt)) {
            pan = 0.0;
        }
        if pan != t.pan {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
                tr.pan = pan;
            }
            crate::automation::record(app, i, crate::automation::PAN, 64.0 + pan * 63.0);
            app.dirty = true;
            app.sync_mix();
        }
        kresp.context_menu(|ui| crate::trackhdr::knob_menu(app, ui));
        }
        // volume: a small slider (dB)
        let mut vol = t.volume_db;
        if app.tm.hdr.volume {
        // (Logic: a 20 pt groove 6 pt after the buttons, to 43.5 pt from the right, a 16 pt knob)
        let vr = Rect::from_min_max(pos2(bx + 98.0 + 3.0, by - 1.0), pos2((row.max.x - 43.5).max(bx + 98.0 + 33.0), by + 19.0));
        let vresp = ui.interact(vr, ui.scope_id().with(("vol", i)), Sense::click_and_drag()).on_hover_text(format!("Volume {vol:.1} dB"));
        let frac = db_to_frac(vol);
        hp.rect_filled(vr, 10.0, look::header_groove());
        let inner = vr.shrink2(vec2(10.0, 0.0));
        crate::trackhdr::volume_meter(app, &hp, inner, i);
        hp.circle_filled(pos2(inner.min.x + frac * inner.width(), vr.center().y), 8.0, Color32::from_gray(0x7d));
        if let Some(pp) = vresp.interact_pointer_pos() {
            if vresp.dragged() || vresp.clicked() {
                vol = frac_to_db(((pp.x - vr.min.x - 10.0) / (vr.width() - 20.0)).clamp(0.0, 1.0));
                changed = true;
            }
        }
        if vresp.double_clicked() || (vresp.clicked() && ui.input(|i| i.modifiers.alt)) {
            vol = 0.0;
            changed = true;
        }
        }
        if changed {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
                tr.mute = mute;
                tr.solo = solo;
                tr.volume_db = vol;
            }
            if vol != t.volume_db {
                crate::automation::record(app, i, crate::automation::VOLUME, crate::automation::db_to_logic(vol));
            }
            app.sync_mix();
        }
        // automation shown: the mode and parameter pop-ups in place of the volume slider
        if app.show_automation {
            let ar = Rect::from_min_size(pos2(bx + 98.0, by), vec2((row.max.x - bx - 138.0).max(90.0), 17.0));
            hp.rect_filled(ar.expand(2.0), 0.0, if selected { look::ta_header_selected() } else { look::ta_header() });
            crate::automation::header(app, ui, ar, i);
            crate::autoext::header_extras(app, ui, row, i);
        } else if app.flex.show {
            // Show Flex Pitch/Time: the flex button and mode pop-up there
            let ar = Rect::from_min_size(pos2(bx + 98.0, by), vec2((row.max.x - bx - 138.0).max(90.0), 17.0));
            hp.rect_filled(ar.expand(2.0), 0.0, if selected { look::ta_header_selected() } else { look::ta_header() });
            crate::flex::header(app, ui, ar, i);
        }
        // regions
        for (ri, r) in t.regions.iter().enumerate() {
            let x0 = x_of(r.start as f32, scroll);
            let x1 = x_of((r.start + r.length) as f32, scroll);
            // (a looped region shows until its repetitions end, also once it has scrolled away)
            let x_end = if r.props.looped { x_of(crate::regionops::loop_end(app, i, ri) as f32, scroll).max(x1) } else { x1 };
            if x_end < lanes.min.x || x0 > lanes.max.x {
                continue;
            }
            // (Logic: regions fill the row, 1 pt clear of the line under it)
            let rr = Rect::from_min_max(pos2(x0, y + 1.0), pos2(x1.max(x0 + 4.0), y + rh - 1.0));
            let sel = app.selected_region == Some((i, ri)) || app.selected_regions.contains(&(i, ri));
            if r.props.looped {
                let xo = move |tick: f32| x_of(tick, scroll);
                crate::regionops::draw_loops(app, &lp, rr, i, ri, &xo);
            }
            if r.audio.is_some() {
                // (its file, decoded once, for the waveform: regiontrim.rs)
                crate::regiontrim::ensure_loaded(app, i, ri);
            }
            draw_region(&lp, rr, r, t.kind, sel, t.mute || r.muted || crate::liveloops::takes_over(app, i), app, (i, ri));
            crate::flex::draw_region(app, &lp, rr, i, ri);
            if let Some((rt, rri, text)) = app.renaming_region.as_mut().filter(|x| x.0 == i && x.1 == ri) {
                let (rt, rri) = (*rt, *rri);
                let er = Rect::from_min_size(rr.min, vec2(rr.width().max(120.0), 18.0));
                let te = ui.put(er, egui::TextEdit::singleline(text).font(look::regular(11.0)));
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    let name = text.clone();
                    app.renaming_region = None;
                    app.checkpoint("Rename Region");
                    if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(rt)?.regions.get_mut(rri)) {
                        x.name = name;
                    }
                    app.after_edit();
                }
            }
            let resp = ui.interact(rr.intersect(lanes), ui.scope_id().with(("region", i, ri)), Sense::click_and_drag());
            // the Gain tool: the region's gain shown on hover, dragged up or down
            crate::regionparams::gain_hover(app, ui, rr, i, ri, resp.hovered());
            // the Solo tool: plays while held (regsolo.rs)
            if app.tool == Tool::Solo && resp.is_pointer_button_down_on() && !crate::regsolo::holding(app) {
                let tick = resp.interact_pointer_pos().map(|pp| ((pp.x - lanes.min.x) / px_per_tick + scroll * bar_t).max(0.0) as u32);
                let alt = ui.input(|i| i.modifiers.alt);
                crate::regsolo::hold_start(app, i, ri, tick, alt);
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.drag_started() || resp.clicked() || resp.double_clicked() {
                region_pressed = Some((i, ri, resp.double_clicked(), resp.drag_started(), resp.interact_pointer_pos()));
            }
            // right-click: selects it, then Logic's region menu
            if resp.secondary_clicked() {
                app.selected_region = Some((i, ri));
                app.selected_track = Some(i);
            }
            crate::menus::context_menu(app, &resp, if r.audio.is_some() { "region-audio" } else { "region-midi" });
            if resp.dragged() && !crate::regionparams::gain_drag(app, resp.drag_delta().y, ui.input(|i| i.modifiers.shift)) {
                // (the drag's region: with Option it is the copy, while the pointer is still over
                // the original)
                if let (Some(Drag::Region { track, region, from_tick, start }), Some(pp)) = (app.drag, resp.interact_pointer_pos()) {
                    if track == i {
                        let ri = region;
                        let tick = (pp.x - lanes.min.x) / px_per_tick + scroll * bar_t;
                        // Shift: no snapping (with Option-Shift: the alias's own drag)
                        let free = ui.input(|i| i.modifiers.shift && !i.modifiers.alt);
                        let raw = start as f32 + tick - from_tick;
                        let new = crate::arranging::snap_region(app, raw, start, px_per_tick, free);
                        // Control: by divisions, Control-Shift: by ticks; Limit Dragging to One
                        // Direction (regionparams.rs)
                        let new = crate::regionparams::drag_snap(app, ui, raw, start).unwrap_or(new);
                        let new = if crate::regionparams::drag_axis(app, ui) == Some(false) { start } else { new };
                        move_region(app, track, ri, new);
                        // across tracks: shown on the track under the pointer, moved there on release
                        let under = crate::trackarea::row_at(&rows, pp.y - headers.min.y + app.scroll_y).map(|r| r.t).filter(|&to| to != track);
                        let mut tag = vec![("Position:".into(), crate::app::bbdt(new, sig))];
                        if let Some(to) = under {
                            if let Some(row) = rows.iter().find(|r| r.t == to) {
                                let y = headers.min.y + row.y - app.scroll_y;
                                let len = app.project.as_ref().and_then(|p| p.tracks.get(track)?.regions.get(ri)).map_or(0, |x| x.length);
                                let ghost = Rect::from_min_max(pos2(x_of(new as f32, scroll), y + 2.0), pos2(x_of((new + len) as f32, scroll), y + row.h - 2.0));
                                ui.painter_at(lanes).rect_stroke(ghost, 4.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
                            }
                            tag.push(("Track:".into(), app.project.as_ref().and_then(|p| p.tracks.get(to)).map_or(String::new(), |t| t.name.clone())));
                        }
                        app.help_tag = Some(tag);
                    }
                }
            }
            if resp.drag_stopped() {
                crate::regionparams::gain_end(app);
                // the Drag mode (No Overlap, X-Fade, Shuffle…) on the track it ended on
                if let Some(Drag::Region { track, region, start, .. }) = app.drag {
                    let under = resp.interact_pointer_pos().or(ui.ctx().pointer_latest_pos()).and_then(|pp| crate::trackarea::row_at(&rows, pp.y - headers.min.y + app.scroll_y)).map(|r| r.t);
                    let under = under.filter(|_| crate::regionparams::drag_axis(app, ui) != Some(true));
                    app.rp.axis = None;
                    let (t2, r2) = match under.filter(|&to| to != track).and_then(|to| crate::arranging::move_to_track(app, track, region, to).map(|k| (to, k))) {
                        Some(x) => x,
                        None => (track, region),
                    };
                    crate::autoext::region_moved(app, track, start, t2, r2);
                    crate::arranging::after_move(app, t2, r2);
                }
                app.drag = None;
                app.load_engine();
            }
            // the right edge's lower half: drag to change the length (snaps to beats; Shift:
            // free); its upper half loops the region (Logic's loop cursor)
            let edge = Rect::from_min_max(pos2(rr.max.x - 6.0, rr.center().y), pos2(rr.max.x + 2.0, rr.max.y)).intersect(lanes);
            let eresp = ui.interact(edge, ui.scope_id().with(("region-end", i, ri)), Sense::drag());
            if eresp.hovered() || eresp.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if eresp.drag_started() {
                app.checkpoint("Resize Region");
                app.selected_region = Some((i, ri));
                app.drag = Some(Drag::RegionEnd { track: i, region: ri, length: r.length });
            }
            if let (true, Some(Drag::RegionEnd { track, region, .. }), Some(pp)) = (eresp.dragged(), app.drag, eresp.interact_pointer_pos()) {
                if track == i && region == ri {
                    let tick = (pp.x - lanes.min.x) / px_per_tick + scroll * bar_t;
                    let beat = bar_t / app.project.as_ref().map_or(4.0, |p| p.signature.0 as f32);
                    let free = ui.input(|i| i.modifiers.shift);
                    let end = if free { tick } else { crate::arranging::snap_tick(app, tick, px_per_tick) };
                    let mut len = (end - r.start as f32).max(if free { 10.0 } else { beat.min(crate::arranging::snap_grid(app, px_per_tick)) }) as u32;
                    // audio: not past its file's end (Option stretches instead)
                    if !ui.input(|i| i.modifiers.alt) {
                        if let Some(max) = crate::regiontrim::end_limit(app, i, ri) {
                            len = len.min((max - r.start as f64).round().max(1.0) as u32);
                        }
                    }
                    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)).and_then(|t| t.regions.get_mut(ri)) {
                        rg.length = len;
                    }
                    app.help_tag = Some(vec![("Length Change:".into(), String::new()), ("Position:".into(), crate::app::bbdt(r.start + len, sig)), ("Length:".into(), crate::app::bbdt_len(len, sig))]);
                }
            }
            if eresp.drag_stopped() {
                if let Some(Drag::RegionEnd { track, region, length }) = app.drag {
                    // Option: time stretch (stretch.rs)
                    if ui.input(|i| i.modifiers.alt) {
                        crate::stretch::option_resize(app, track, region, length);
                    } else {
                        crate::arranging::after_resize(app, track, region, length);
                        if let Some((t2, r2)) = app.selected_region.filter(|x| x.0 == track) {
                            crate::regiontrim::after_trim(app, &ui.ctx().clone(), t2, r2);
                        }
                    }
                }
                app.drag = None;
                app.load_engine();
            }
            {
                let to = move |x: f32| (x - lanes.min.x) / px_per_tick + scroll * bar_t;
                let xo = move |tick: f32| x_of(tick, scroll);
                // the left edge's lower half: trim the start (regiontrim.rs)
                crate::regiontrim::left_edge(app, ui, rr, lanes, i, ri, &to, px_per_tick);
                crate::regionops::loop_handle(app, ui, rr, lanes, i, ri, &to);
                let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
                if !app.show_automation {
                    crate::regionops::fades(app, ui, rr, lanes, i, ri, tool, &xo, &to);
                }
            }
        }
        // a stack's main track: its subtracks' regions (closed), a folder's region
        if t.extra.stack.is_some() {
            let xo = move |tick: f32| x_of(tick, scroll);
            crate::trackarea::stack_lane(app, ui, lane, lanes, i, &xo);
        }
        // what's being recorded onto the track, its take folders
        if lane.intersects(lanes) {
            crate::recording::lane(app, ui, lane.intersect(lanes), i, lanes.min.x, scroll * bar_t, px_per_tick);
        }
        // the automation lane over the row (it takes the clicks while it shows, as Logic's)
        if app.show_automation && lane.intersects(lanes) {
            let xo = move |tick: f32| x_of(tick, scroll);
            let to = move |x: f32| (x - lanes.min.x) / px_per_tick + scroll * bar_t;
            crate::automation::lane(app, ui, lane.intersect(lanes), i, &xo, &to);
        }
        if sub > 0.0 {
            let xo = move |tick: f32| x_of(tick, scroll);
            let to = move |x: f32| (x - lanes.min.x) / px_per_tick + scroll * bar_t;
            crate::autoext::subtracks(app, ui, (row.min.x, row.max.x), lanes, y + rh, i, &xo, &to);
        }
    }
    ui.set_clip_rect(rows_clip_saved);
    // the empty header area under the tracks (double-click: a track like the selected one)
    crate::trackhdr::below_headers(app, ui, Rect::from_min_max(pos2(headers.min.x, (headers.min.y + rows_h - app.scroll_y).max(headers.min.y)), headers.max));
    // the Marquee tool (or a marquee already made): a time range over tracks
    {
        let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
        let to = move |x: f32| (x - lanes.min.x) / px_per_tick + scroll * bar_t;
        let xo = move |tick: f32| x_of(tick, scroll);
        crate::regionops::marquee(app, ui, lanes, headers.min.y, &rows, &to, &xo, tool == Tool::Marquee);
    }
    // click on an empty lane: nothing selected; with the Pencil, a new empty region of a bar
    if empty.clicked() && region_pressed.is_none() {
        let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
        let pp = empty.interact_pointer_pos();
        let row = pp.and_then(|pp| crate::trackarea::row_at(&rows, pp.y - headers.min.y + app.scroll_y)).map(|r| r.t);
        if let (Tool::Pencil, Some(pp), Some(t)) = (tool, pp, row.filter(|&t| proj.tracks[t].extra.stack.is_none())) {
            if proj.tracks[t].kind != TrackKind::Audio {
                let tick = ((pp.x - lanes.min.x) / px_per_tick + scroll * bar_t).max(0.0) as u32;
                let bar = bar_t as u32;
                let start = (tick / bar) * bar;
                // (the track's Default Region Type: a pattern region on a Pattern track)
                if proj.tracks[t].extra.set.default_region == lg_core::trackmix::RegionType::Pattern {
                    crate::stepseq::new_pattern_region(app, &ui.ctx().clone(), t, start);
                } else if let Some(tr) = {
                    app.checkpoint("Create Region");
                    app.project.as_mut().and_then(|p| p.tracks.get_mut(t))
                } {
                    let name = tr.name.clone();
                    tr.regions.push(lg_core::Region { name, start, length: bar, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
                    let r = tr.regions.len() - 1;
                    app.selected_track = Some(t);
                    app.selected_region = Some((t, r));
                }
                app.after_edit();
            }
        } else {
            app.selected_region = None;
            app.selected_regions.clear();
            app.selected_notes.clear();
            app.ta.auto.sel = None;
            app.ta.regions.marquee = None;
        }
    }
    tool_menu(app, ui);
    // inside a packed folder: its name over the headers; click to go out (Logic's folder level)
    if let Some(f) = crate::trackarea::inside(app) {
        let name = app.project.as_ref().map_or(String::new(), |p| p.tracks[f].name.clone());
        let r = Rect::from_min_size(pos2(head.min.x + 110.0, head.min.y + 4.0), vec2((hw - 120.0).max(60.0), RULER_H - 8.0));
        let resp = ui.interact(r, ui.scope_id().with("folder-up"), Sense::click()).on_hover_text("Go Out of Folder");
        p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.with_clip_rect(r).text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, format!("‹  {name}"), look::semibold(11.5), look::text());
        if resp.clicked() {
            crate::trackarea::command(app, "Go Out of Folder or Region");
        }
    }
    crate::trackarea::dialogs(app, ui.ctx());
    crate::regionops::dialogs(app, ui.ctx());
    if let Some((t, r, double, drag, pp)) = region_pressed {
        // (Settings ▸ Select tracks on region/marquee selection)
        if crate::prefs::region_selects_track(app) || app.selected_track.is_none() {
            app.selected_track = Some(t);
            if !app.ta.selected.contains(&t) {
                app.ta.selected = vec![t];
            }
        }
        // Shift-click adds or removes a region from the selection (Logic)
        if ui.input(|i| i.modifiers.shift) && !drag {
            if let Some(k) = app.selected_regions.iter().position(|x| *x == (t, r)) {
                app.selected_regions.remove(k);
            } else {
                app.selected_regions.push((t, r));
            }
            app.selected_region = app.selected_regions.last().copied();
            return;
        }
        // the tool in hand (Command held: the Command-click tool)
        let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
        let click_tick = pp.map(|pp| ((pp.x - lanes.min.x) / px_per_tick + scroll * bar_t).max(0.0) as u32);
        if !drag && tool != Tool::Pointer {
            if region_tool(app, ui, tool, t, r, click_tick, pp) {
                return;
            }
        }
        if !app.selected_regions.contains(&(t, r)) {
            app.selected_regions = vec![(t, r)];
        }
        app.selected_region = Some((t, r));
        if double {
            app.bottom = crate::app::Bottom::PianoRoll;
            // a MIDI region: the editor Settings ▸ Double-Clicking a MIDI Region Opens names
            if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Instrument) {
                crate::prefs::open_midi_region(app, &ui.ctx().clone());
            }
        }
        app.center_roll(t, r);
        if drag && tool == Tool::Gain {
            crate::regionparams::gain_start(app, t, r);
            return;
        }
        if drag {
            // Option-drag: a copy moves, the original stays (Logic)
            let copy = ui.input(|i| i.modifiers.alt);
            let alias = copy && ui.input(|i| i.modifiers.shift);
            app.checkpoint(if alias { "Make Alias" } else if copy { "Copy Region" } else { "Move Region" });
            let mut r = r;
            if alias {
                // Option-Shift: an alias (MIDI) or a clone (audio) moves (arranging.rs)
                if let Some(k) = crate::arranging::alias_for_drag(app, t, r) {
                    r = k;
                    app.selected_region = Some((t, r));
                }
            } else if copy {
                if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    if let Some(mut rg) = tr.regions.get(r).cloned() {
                        rg.origin = rg.source.or(rg.origin);
                        rg.source = None;
                        tr.regions.push(rg);
                        r = tr.regions.len() - 1;
                        app.selected_region = Some((t, r));
                    }
                }
            }
            if let (Some(pp), Some(start)) = (pp, app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).map(|x| x.start)) {
                let tick = (pp.x - lanes.min.x) / px_per_tick + scroll * bar_t;
                app.drag = Some(Drag::Region { track: t, region: r, from_tick: tick, start });
            }
        }
    }
    // the playhead
    let ph = x_of(app.position() as f32, scroll);
    if ph >= lanes.min.x && ph <= lanes.max.x {
        // (Logic: a dotted grey line through the bar numbers, a solid white one from the lower
        // strip down, and a grey pin-shaped head in the lower strip)
        let full = ui.painter_at(Rect::from_min_max(ruler.min, area.max));
        let mut dy = ruler.min.y;
        while dy < ruler.min.y + 20.0 {
            full.vline(ph, dy..=dy + 1.0, Stroke::new(1.0, Color32::from_gray(0x80)));
            dy += 3.0;
        }
        full.vline(ph, ruler.min.y + 20.0..=area.max.y, Stroke::new(crate::prefs::playhead_width(), if kn_ui::theme::is_light() { look::playhead() } else { Color32::WHITE }));
        let top = ruler.min.y + 21.0;
        let head = egui::Shape::convex_polygon(vec![pos2(ph - 7.5, top + 2.0), pos2(ph - 5.5, top), pos2(ph + 5.5, top), pos2(ph + 7.5, top + 2.0), pos2(ph + 7.5, top + 12.0), pos2(ph, top + 17.5), pos2(ph - 7.5, top + 12.0)], Color32::from_gray(0x76), Stroke::new(0.5, Color32::from_gray(0x8c)));
        full.add(head);
        full.vline(ph, top..=top + 17.5, Stroke::new(crate::prefs::playhead_width(), if kn_ui::theme::is_light() { look::playhead() } else { Color32::WHITE }));
    }
    crate::scrollnum::draw(ui, lanes);
    // follow the playhead while playing (Logic's Catch; Scroll in Play: nav.rs)
    app.nav.lanes_h = lanes.height();
    crate::windows::area(app, ruler, "ruler");
    crate::windows::area(app, headers, "headers");
    crate::windows::area(app, lanes, "lanes");
    app.nav.tracks_rect = Rect::from_min_max(headers.min, lanes.max);
    app.nav.lanes_x = lanes.min.x;
    crate::nav::follow_playhead(app, lanes.width() / app.bar_w);
    if grid_w > 0.0 {
        crate::liveloops::show_grid(app, ui, Rect::from_min_max(pos2(head.max.x, local.max.y), pos2(head.max.x + grid_w, area.max.y)), headers.min.y);
    }
    // the divider between headers and lanes: drag to widen the headers (265–547 pt)
    p.vline(head.max.x, area.min.y + LOCAL_H..=area.max.y, Stroke::new(1.0, look::ta_seam()));
    let edge = Rect::from_min_max(pos2(head.max.x - 3.0, head.max.y), pos2(head.max.x + 3.0, area.max.y));
    let e = ui.interact(edge, ui.scope_id().with("header-edge"), Sense::drag());
    if e.hovered() || e.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if let (true, Some(pp)) = (e.dragged(), e.interact_pointer_pos()) {
        app.header_w = (pp.x - area.min.x).clamp(HEADER_W, 547.0);
    }
}

fn zoom_at(app: &mut App, factor: f32, at_x: f32, lanes_x: f32) {
    let bar_at = (at_x - lanes_x) / app.bar_w + app.scroll_x;
    app.bar_w = (app.bar_w * factor).clamp(8.0, 1200.0);
    app.scroll_x = (bar_at - (at_x - lanes_x) / app.bar_w).max(crate::projend::min_scroll(app));
}

fn move_region(app: &mut App, t: usize, r: usize, start: u32) {
    let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) else { return };
    // (Lock Time Position: it stays)
    if crate::regionparams::locked(rg) {
        return;
    }
    let d = start as i64 - rg.start as i64;
    if d == 0 {
        return;
    }
    rg.start = start;
    for n in &mut rg.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
}


fn draw_region(p: &egui::Painter, r: Rect, region: &lg_core::Region, kind: TrackKind, selected: bool, muted: bool, app: &App, at: (usize, usize)) {
    let base = crate::regionparams::region_color(app, region, kind);
    let base = if muted { Color32::from_gray(110) } else { base };
    // Logic (ui-tracks-with-pianoroll.png, …-markers-tempo-signature.png): an opaque body in the
    // colour with 3 pt corners and a dark 1 pt outline; the name in light text on the body. A
    // selected region: an 18 pt name bar in the colour's light tint with dark text, and a 1 pt
    // light border. Notes and waveforms in the light tint.
    let body = base;
    let light = look::region_light(base);
    p.rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(200)), egui::StrokeKind::Outside);
    p.rect_filled(r, 3.0, body);
    let header = Rect::from_min_size(r.min, vec2(r.width(), 18.0_f32.min(r.height() * 0.45)));
    if selected {
        p.rect_filled(header, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, light);
        p.rect_stroke(r, 3.0, Stroke::new(1.0, light), egui::StrokeKind::Inside);
    }
    let name_col = if selected { look::region_dark(base) } else { Color32::from_rgb(0xf2, 0xf2, 0xf4) };
    if r.width() > 24.0 {
        // an alias (or clone) shows an arrow, and its parent's name under its own when zoomed in
        let alias = region.ext.alias_of != 0;
        let name = if alias { format!("↪ {}", if region.name.is_empty() { &region.ext.parent_name } else { &region.name }) } else { region.name.clone() };
        // (a muted region: its name after a small dot)
        let name = if region.muted { format!("• {name}") } else { name };
        p.with_clip_rect(header.shrink2(vec2(1.0, 0.0))).text(pos2(header.min.x + 5.0, header.center().y), Align2::LEFT_CENTER, name, look::regular(11.0), name_col);
        if alias && r.height() > 40.0 && region.name != region.ext.parent_name && !region.ext.parent_name.is_empty() {
            p.with_clip_rect(r).text(pos2(header.min.x + 5.0, header.max.y + 7.0), Align2::LEFT_CENTER, &region.ext.parent_name, look::regular(9.5), Color32::from_rgb(0xf2, 0xf2, 0xf4).gamma_multiply(0.7));
        }
    }
    let inner = Rect::from_min_max(pos2(r.min.x + 2.0, header.max.y + 2.0), pos2(r.max.x - 2.0, r.max.y - 3.0));
    let ink = light;
    if let Some(a) = &region.audio {
        // the waveform: from the file at the region's offset, at the timeline's scale (trimming
        // shows or hides audio, never squeezes it: regiontrim.rs)
        let clip = p.clip_rect().intersect(inner);
        if crate::regiontrim::draw_wave(app, p, r, inner, clip, at.0, at.1, ink) {
            return;
        }
        // else the engine's render of the region (reversed, or the file not decoded)
        let samples = app.engine.lock().ok().and_then(|e| e.tracks.get(at.0).and_then(|t| t.audio.iter().find(|(s, _)| *s == region.start).map(|(_, s)| s.clone())));
        let mid = inner.center().y;
        match samples {
            Some(s) if !s.is_empty() => {
                // (Bramble's smoother waveforms: multi-resolution overviews, wavecache.rs)
                // (Waveform Vertical Zoom: taller than the region, cut at its edges); the render
                // at its own length (seconds), not stretched over the region
                let secs = (s.len() / 2) as f64 / crate::audio::RATE as f64;
                let t0 = app.seconds_at(region.start as f64);
                let end = crate::smarttempo::tick_at(app, t0 + secs) as f32;
                let w = r.width() * (end - region.start as f32) / region.length.max(1) as f32;
                let zr = Rect::from_center_size(inner.center(), vec2(inner.width(), inner.height() * app.keyx.wave_zoom));
                let zr = Rect::from_min_max(pos2(r.min.x + 2.0, zr.min.y), pos2(r.min.x + w - 2.0, zr.max.y));
                crate::wavecache::draw(&p.with_clip_rect(clip), zr, clip, &s, ink);
            }
            _ => {
                p.hline(inner.x_range(), mid, Stroke::new(1.0, ink));
                let _ = a;
            }
        }
        return;
    }
    if region.notes.is_empty() {
        return;
    }
    let lo = region.notes.iter().map(|n| n.pitch).min().unwrap_or(60) as f32;
    let hi = region.notes.iter().map(|n| n.pitch).max().unwrap_or(72) as f32;
    let span = (hi - lo).max(12.0);
    let bar_t = app.bar_ticks() as f32;
    for n in &region.notes {
        // (notes outside the region, trimmed off its edges, are kept but hidden: Logic)
        if n.pos < region.start || n.pos >= region.start + region.length {
            continue;
        }
        let x0 = r.min.x + (n.pos.saturating_sub(region.start)) as f32 / bar_t * app.bar_w;
        let x1 = x0 + (n.len as f32 / bar_t * app.bar_w).max(1.5);
        if x0 > r.max.x {
            continue;
        }
        let y = inner.max.y - (n.pitch as f32 - lo) / span * (inner.height() - 2.0) - 1.0;
        p.line_segment([pos2(x0, y), pos2(x1.min(r.max.x - 1.0), y)], Stroke::new(2.0, ink));
    }
}

/// The large track icons (wide headers, as Logic's pictures): the instrument's icon big on a
/// card in the track's colour.
fn track_icon_large(p: &egui::Painter, r: Rect, t: &lg_core::Track) {
    let c = look::region_color(t.kind);
    p.rect_stroke(r, 8.0, Stroke::new(1.0, c.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    crate::glyphs::paint(p, r, crate::glyphs::TrackIcon::of(t).glyph(), r.width() * 0.8, c);
}

/// A track's icon: what it plays (keys, drums, guitar, synth, a microphone or a waveform for
/// audio) in the track's colour.
fn track_icon(p: &egui::Painter, r: Rect, t: &lg_core::Track) {
    let c = look::region_color(t.kind);
    crate::glyphs::paint(p, r, crate::glyphs::TrackIcon::of(t).glyph(), 27.0, icon_ink(c));
}

/// The icon in the track's colour, a shade darker on the light theme (to stand out on its card).
fn icon_ink(c: Color32) -> Color32 {
    if kn_ui::theme::is_light() { look::mix(c, Color32::BLACK, 0.3) } else { look::icon_color(c) }
}

pub fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

/// Logic's fader law (roughly): -inf..+6 dB over 0..1, 0 dB at about 0.73.
pub fn db_to_frac(db: f32) -> f32 {
    if db <= -60.0 { 0.0 } else { ((db + 60.0) / 66.0).powf(1.6).clamp(0.0, 1.0) }
}

pub fn frac_to_db(f: f32) -> f32 {
    if f <= 0.001 { -96.0 } else { f.powf(1.0 / 1.6) * 66.0 - 60.0 }
}

/// A click on a region with a tool other than the Pointer (Logic's Tracks-area tools). Returns
/// true when the tool used the click.
fn region_tool(app: &mut App, ui: &egui::Ui, tool: Tool, t: usize, r: usize, tick: Option<u32>, pp: Option<egui::Pos2>) -> bool {
    let bar = app.bar_ticks();
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32);
    let div = (beat / 4).max(1);
    match tool {
        Tool::Eraser => {
            app.selected_region = Some((t, r));
            app.selected_regions = vec![(t, r)];
            app.delete_selection();
        }
        Tool::Scissors => {
            let Some(tick) = tick else { return true };
            // (snapped to the division; Option: cuts it into equal pieces of that length)
            let cut = if ui.input(|i| i.modifiers.shift) { tick } else { ((tick as f64 / div as f64).round() as u32) * div };
            app.selected_region = Some((t, r));
            app.selected_regions = vec![(t, r)];
            if ui.input(|i| i.modifiers.alt) {
                if let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) {
                    let step = cut.saturating_sub(rg.start).max(div);
                    let cuts: Vec<u32> = (1..).map(|k| rg.start + k * step).take_while(|c| *c < rg.start + rg.length).collect();
                    app.split_at(&cuts);
                }
            } else {
                app.split_at(&[cut]);
            }
        }
        Tool::Glue => {
            let mut v = app.region_selection();
            if !v.contains(&(t, r)) {
                v.push((t, r));
            }
            app.selected_regions = v;
            let ctx = ui.ctx().clone();
            app.edit_command(&ctx, "Join Regions/Notes");
        }
        Tool::Mute => {
            app.checkpoint("Mute Region");
            if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                rg.muted = !rg.muted;
            }
            app.after_edit();
        }
        Tool::Solo => {
            // played while held (regsolo.rs, on the press)
        }
        Tool::Zoom => {
            if let Some(pp) = pp {
                let f = if ui.input(|i| i.modifiers.alt) { 0.5 } else { 2.0 };
                let lanes_x = pp.x - (tick.unwrap_or(0) as f32 / bar as f32 - app.scroll_x) * app.bar_w;
                zoom_at(app, f, pp.x, lanes_x);
            }
        }
        Tool::Gain => return crate::regionparams::gain_click(app, ui, t, r),
        Tool::Text => {
            let cur = app.project.as_ref().map(|p| p.tracks[t].regions[r].name.clone()).unwrap_or_default();
            app.renaming_region = Some((t, r, cur));
        }
        _ => return false,
    }
    true
}

/// The T menu at the pointer: the tools with their letters (Logic's Tool Menu).
fn tool_menu(app: &mut App, ui: &mut egui::Ui) {
    if !app.tool_menu_open {
        return;
    }
    let at = app.pointer;
    let roll = app.bottom == crate::app::Bottom::PianoRoll && app.editor_rect.contains(at);
    let tools: &[Tool] = if roll { &Tool::PIANO_ROLL } else { &Tool::TRACKS };
    let cur = if roll { app.roll_tool } else { app.tool };
    // (the Pointer tool, first, under the pointer)
    let area = egui::Area::new(egui::Id::unique("tool-menu")).fixed_pos(at - vec2(24.0, 14.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if let Some(t) = crate::rolltools::menu_rows(ui, tools, cur) {
                if roll {
                    app.roll_tool = t;
                } else {
                    app.tool = t;
                }
                app.tool_menu_open = false;
            }
        });
    });
    if ui.input(|i| i.pointer.any_pressed()) && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p)) {
        app.tool_menu_open = false;
    }
    // a tool's key picks it while the menu is open
    let typed = ui.input(|i| i.events.iter().find_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }));
    if let Some(t) = typed.and_then(|s| crate::rmb::tool_for_key(tools, |t| t.key(), &s)) {
        if roll {
            app.roll_tool = t;
        } else {
            app.tool = t;
        }
        app.tool_menu_open = false;
    }
}

/// Logic's global tracks: Marker (markers as tabs; double-click the lane: a marker there; drag
/// one to move it), Tempo (the tempo as a line; drag up or down), Signature (time and key).
fn global_tracks(app: &mut App, ui: &mut egui::Ui, area: Rect, lanes_x: f32) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::ta_header());
    // the headers' right-click menu: which global tracks show (Studio adds Transposition)
    let heads = Rect::from_min_max(area.min, pos2(lanes_x, area.max.y));
    let hresp = ui.interact(heads, ui.scope_id().with("global-heads"), Sense::click());
    crate::globals::header_menu(app, &hresp);
    if app.ta.show_transposition && !app.gt.single {
        let tr = Rect::from_min_max(pos2(area.min.x, area.max.y - TRANSPOSITION_H), area.max);
        crate::trackarea::transposition_lane(app, ui, tr, lanes_x);
    }
    let bar_t = app.bar_ticks() as f32;
    let (scroll, bw) = (app.scroll_x, app.bar_w);
    let x_of = |tick: f32| lanes_x + (tick / bar_t - scroll) * bw;
    let tick_of = |x: f32| ((x - lanes_x) / bw + scroll) * bar_t;
    let mut top = area.min.y;
    // (the global tracks shown, in their order: globals.rs)
    let placed: Vec<(usize, Rect)> = crate::globals::rows(app)
        .into_iter()
        .map(|(k, h)| {
            let r = Rect::from_min_size(pos2(area.min.x, top), vec2(area.width(), h));
            top += h;
            (k, r)
        })
        .collect();
    for &(kind, row) in &placed {
        let n = crate::globals::KINDS[kind];
        // (Logic: grey lanes for Arrangement, Marker and Signature, near-black for Tempo and
        // Chord; the headers' grey; names in white 13.5 pt, 20 pt in)
        let split = lanes_x - LANE_INSET;
        let lane_fill = match kind {
            crate::globals::TEMPO | crate::globals::CHORD => Color32::from_gray(0x0d),
            crate::globals::ARRANGEMENT | crate::globals::MARKER | crate::globals::SIGNATURE => Color32::from_gray(0x30),
            _ => look::ta_lane(),
        };
        if !kn_ui::theme::is_light() {
            p.rect_filled(Rect::from_min_max(pos2(split, row.min.y), row.max), 0.0, lane_fill);
        }
        if matches!(kind, crate::globals::TEMPO | crate::globals::CHORD) {
            let lp = ui.painter_at(Rect::from_min_max(pos2(lanes_x, row.min.y), row.max));
            let b0 = scroll.floor() as i32;
            for b in b0..b0 + ((row.max.x - lanes_x) / bw).ceil() as i32 + 1 {
                lp.vline(x_of(b as f32 * bar_t), row.min.y..=row.max.y - 1.0, Stroke::new(1.0, look::ta_bar_line()));
            }
        }
        crate::globals::header(app, ui, kind, row, lanes_x);
        p.hline(row.min.x..=split, row.max.y - 0.5, Stroke::new(1.0, look::ta_header_sep()));
        p.hline(split..=row.max.x, row.max.y - 0.5, Stroke::new(1.0, look::ta_lane_line()));
        if kind != crate::globals::MOVIE {
            let n = if kind == crate::globals::ARRANGEMENT { crate::arrmarks::header_name(app) } else { n };
            p.text(pos2(row.min.x + 20.0, row.min.y + 10.5), Align2::LEFT_CENTER, n, look::regular(13.5), look::text());
        }
        let lane = Rect::from_min_max(pos2(lanes_x, row.min.y), row.max);
        let lp = ui.painter_at(lane);
        let i = match kind {
            crate::globals::ARRANGEMENT => 0,
            crate::globals::MARKER => 1,
            crate::globals::TEMPO => 2,
            crate::globals::SIGNATURE => 3,
            crate::globals::CHORD => 4,
            crate::globals::MOVIE => {
                crate::movie::track_lane(app, ui, row, lanes_x);
                continue;
            }
            _ => {
                crate::beatmapping::header(app, ui, row, lanes_x);
                crate::beatmapping::lane(app, ui, lane, &x_of, &tick_of);
                continue;
            }
        };
        // (+) at the right of the Arrangement, Marker, Signature and Chord headers
        let plus = Rect::from_center_size(pos2(lanes_x - LANE_INSET - 18.0, row.min.y + GLOBAL_ROW / 2.0), vec2(18.0, 18.0));
        let presp = ui.interact(plus, ui.scope_id().with(("gplus", i)), Sense::click()).on_hover_text(format!("Add to the {n} track"));
        if i != 2 {
            crate::glyphs::paint(&p, plus, crate::glyphs::ph_fill::PLUS_CIRCLE, 19.0, if presp.hovered() { Color32::WHITE } else { look::text() });
        }
        let i = match i {
            0 => {
                // Arrangement: each (+) appends an 8-bar section after the last (Logic)
                if presp.clicked() {
                    let start = app.arrangement.last().map_or(0, |a| a.0 + a.1);
                    let name = SECTIONS[app.arrangement.len() % SECTIONS.len()].to_string();
                    app.arrangement.push((start, 8 * bar_t as u32, name));
                    app.dirty = true;
                }
                crate::arrmarks::lane(app, ui, Rect::from_min_max(row.min, pos2(lanes_x, row.max.y)), row, lane, &x_of, &tick_of);
                continue;
            }
            1 => {
                if presp.clicked() {
                    crate::markers::add_button(app);
                }
                0
            }
            2 => {
                // the scale: the range's ends and middle
                crate::arranging::tempo_scale(app, &p, row, lanes_x);
                1
            }
            3 => 2,
            _ => {
                // the Chord track: one chord per bar (the players follow it); click to type them
                let text = if app.chord_text.trim().is_empty() { "C Am F G".to_string() } else { app.chord_text.clone() };
                for (k, name) in text.split_whitespace().enumerate() {
                    let x = x_of(k as f32 * bar_t);
                    let cr = Rect::from_min_size(pos2(x + 1.0, row.min.y + 3.0), vec2(bw - 2.0, row.height() - 6.0));
                    lp.rect_filled(cr, 4.0, Color32::from_rgb(0x8e, 0x6c, 0xd6).gamma_multiply(0.75));
                    lp.with_clip_rect(cr).text(pos2(cr.min.x + 5.0, cr.center().y), Align2::LEFT_CENTER, name, look::semibold(11.5), Color32::WHITE);
                }
                let id = ui.scope_id().with("chord-edit");
                let editing: Option<String> = ui.data(|d| d.get_temp(id));
                if let Some(mut t) = editing {
                    let er = Rect::from_min_size(pos2(lane.min.x + 4.0, row.min.y + 3.0), vec2(320.0, 22.0));
                    let te = ui.put(er, egui::TextEdit::singleline(&mut t).hint_text("C Am F G (one per bar)").font(look::regular(12.0)));
                    crate::widgets::focus_once(ui, &te);
                    if te.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        app.chord_text = t;
                        app.dirty = true;
                        crate::players::regenerate_all(app);
                    } else {
                        ui.data_mut(|d| d.insert_temp(id, t));
                    }
                } else if ui.interact(lane, ui.scope_id().with("chord-lane"), Sense::click()).on_hover_text("Click to type the chords (one per bar)").clicked() || presp.clicked() {
                    ui.data_mut(|d| d.insert_temp(id, text));
                }
                continue;
            }
        };
        let row = Rect::from_min_max(row.min, pos2(row.max.x, row.max.y));
        let _ = GLOBAL_ROW;
        match i {
            0 => crate::markers::lane(app, ui, Rect::from_min_max(row.min, pos2(lanes_x, row.max.y)), row, lane, &x_of, &tick_of),
            1 => {
                crate::globalclip::lane_focus(app, ui, lane, crate::globalclip::Focus::Tempo, &tick_of);
                crate::arranging::tempo_lane(app, ui, row, lane, &x_of, &tick_of)
            }
            _ => {
                crate::globalclip::lane_focus(app, ui, lane, crate::globalclip::Focus::Signature, &tick_of);
                crate::arranging::signature_lane(app, ui, row, lane, &x_of, &tick_of)
            }
        }
    }
    crate::globals::after_rows(app, ui, &placed, lanes_x);
}
