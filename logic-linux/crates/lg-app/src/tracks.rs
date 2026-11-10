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
use crate::look;

/// The track headers' width: Logic's minimum 265 pt, up to 547 (RESIZING.txt).
pub const HEADER_W: f32 = 265.0;
pub const ROW_H: f32 = 52.0;
const LOCAL_H: f32 = 32.0;
const RULER_H: f32 = 32.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::lane_bg());
    // ---- the local menu bar
    let local = Rect::from_min_size(area.min, vec2(area.width(), LOCAL_H));
    p.rect_filled(local, 0.0, look::header_bg());
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(local.shrink2(vec2(10.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        local_menus(app, ui);
    });
    // ---- the list header and the ruler
    let hw = app.header_w.clamp(HEADER_W, 547.0);
    let head = Rect::from_min_size(pos2(area.min.x, local.max.y), vec2(hw, RULER_H));
    let ruler = Rect::from_min_max(pos2(head.max.x, local.max.y), pos2(area.max.x, local.max.y + RULER_H));
    p.rect_filled(head, 0.0, look::header_bg());
    p.rect_filled(ruler, 0.0, look::header_bg());
    if ui.put(Rect::from_center_size(pos2(head.min.x + 22.0, head.center().y), vec2(26.0, 22.0)), egui::Button::new("+").corner_radius(5.0)).on_hover_text("New Software Instrument Track").clicked() {
        new_track(app);
    }
    let lanes = Rect::from_min_max(pos2(ruler.min.x, ruler.max.y), area.max);
    let headers = Rect::from_min_max(pos2(area.min.x, ruler.max.y), pos2(head.max.x, area.max.y));
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
        } else if (zoom - 1.0).abs() > 0.001 {
            zoom_at(app, zoom, ui.input(|i| i.pointer.hover_pos()).map_or(lanes.min.x, |h| h.x), lanes.min.x);
        } else {
            let (dx, dy) = if shift { (scroll.y, 0.0) } else { (scroll.x, scroll.y) };
            app.scroll_x = (app.scroll_x - dx / app.bar_w).max(0.0);
            let n = app.project.as_ref().map_or(0, |p| p.tracks.len()) as f32;
            app.scroll_y = (app.scroll_y - dy).clamp(0.0, (n * ROW_H - lanes.height() + 40.0).max(0.0));
        }
    }
    let scroll = app.scroll_x;
    let bw = app.bar_w;
    let px_per_tick = bw / bar_t;
    let x_of = move |tick: f32, scroll: f32| lanes.min.x + (tick / bar_t - scroll) * bw;
    // ---- ruler: bars, the cycle area, the playhead
    let rp = ui.painter_at(ruler);
    let first = scroll.floor() as i32;
    let visible = (ruler.width() / app.bar_w).ceil() as i32 + 1;
    let step = if app.bar_w < 18.0 { 8 } else if app.bar_w < 34.0 { 4 } else if app.bar_w < 60.0 { 2 } else { 1 };
    for b in first..first + visible {
        let x = x_of(b as f32 * bar_t, scroll);
        if b % step == 0 {
            rp.vline(x, ruler.min.y + 2.0..=ruler.max.y, Stroke::new(1.0, look::text_dim()));
            rp.text(pos2(x + 3.0, ruler.min.y + 2.0), Align2::LEFT_TOP, format!("{}", b + 1), look::semibold(11.0), look::text());
        } else {
            rp.vline(x, ruler.max.y - 6.0..=ruler.max.y, Stroke::new(1.0, look::text_dim().gamma_multiply(0.6)));
        }
    }
    let (ca, cb) = app.cycle;
    let cycle_on = app.cycle_on();
    // the cycle area: a band in the upper strip, behind the bar numbers
    let crect = Rect::from_min_max(pos2(x_of(ca as f32, scroll), ruler.min.y + 1.0), pos2(x_of(cb as f32, scroll), ruler.min.y + 16.0));
    rp.rect_filled(crect, 3.0, if cycle_on { look::cycle().gamma_multiply(0.85) } else { look::cycle().gamma_multiply(0.22) });
    // the ruler (Logic, DRAGS.txt): the upper strip (bar numbers) draws a cycle area and turns
    // Cycle on (dragging inside the area moves it; a click toggles Cycle); the lower strip
    // moves the playhead
    let r_resp = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
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
        match app.drag {
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
                    if upper(pp.y) && tick >= ca as f32 && tick <= cb as f32 {
                        app.toggle_cycle();
                    } else if !upper(pp.y) {
                        app.locate(snap(tick) as f64);
                    }
                }
            }
        }
    }
    if r_resp.drag_stopped() {
        app.drag = None;
    }
    // ---- rows: headers and lanes
    let Some(proj) = app.project.clone() else { return };
    let hp = ui.painter_at(headers);
    let lp = ui.painter_at(lanes);
    hp.rect_filled(headers, 0.0, look::track_header_bg());
    let mut region_pressed = None;
    // (registered first, so regions on top of it take their own clicks)
    let empty = ui.interact(lanes, ui.scope_id().with("lanes-bg"), Sense::click());
    for (i, t) in proj.tracks.iter().enumerate() {
        let y = headers.min.y + i as f32 * ROW_H - app.scroll_y;
        if y + ROW_H < headers.min.y || y > headers.max.y {
            continue;
        }
        let row = Rect::from_min_size(pos2(headers.min.x, y), vec2(hw, ROW_H));
        let lane = Rect::from_min_max(pos2(lanes.min.x, y), pos2(lanes.max.x, y + ROW_H));
        let selected = app.selected_track == Some(i);
        if selected {
            hp.rect_filled(row, 0.0, look::track_selected());
            lp.rect_filled(lane, 0.0, look::track_selected().gamma_multiply(0.35));
        } else if i % 2 == 1 {
            lp.rect_filled(lane, 0.0, look::lane_alt());
        }
        hp.hline(row.x_range(), row.max.y, Stroke::new(1.0, look::line()));
        lp.hline(lane.x_range(), lane.max.y, Stroke::new(1.0, look::grid_line()));
        // the header: number, icon, name, M S, volume
        let row_resp = ui.interact(row, ui.scope_id().with(("trackrow", i)), Sense::click());
        if row_resp.clicked() {
            app.selected_track = Some(i);
        }
        hp.text(pos2(row.min.x + 14.0, row.center().y), Align2::CENTER_CENTER, format!("{}", i + 1), look::semibold(12.0), look::text_dim());
        let icon = Rect::from_center_size(pos2(row.min.x + 46.0, row.center().y), vec2(34.0, 34.0));
        track_icon(&hp, icon, t.kind);
        hp.text(pos2(row.min.x + 72.0, row.min.y + 15.0), Align2::LEFT_CENTER, ellipsize(&t.name, 22), look::semibold(13.0), look::text());
        let mut changed = false;
        let mut mute = t.mute;
        let mut solo = t.solo;
        let ms = |ui: &mut egui::Ui, r: Rect, label: &str, on: bool, col: Color32, id: (&str, usize)| {
            let resp = ui.interact(r, ui.scope_id().with(id), Sense::click());
            let p = ui.painter();
            p.rect_filled(r, 4.0, if on { col } else { look::group_bg() });
            p.text(r.center(), Align2::CENTER_CENTER, label, look::bold(11.0), if on { Color32::BLACK } else { look::text() });
            resp.clicked()
        };
        let by = row.min.y + 29.0;
        if ms(ui, Rect::from_min_size(pos2(row.min.x + 72.0, by), vec2(20.0, 17.0)), "M", mute, Color32::from_rgb(0x4f, 0xa8, 0xe8), ("mute", i)) {
            mute = !mute;
            changed = true;
        }
        if ms(ui, Rect::from_min_size(pos2(row.min.x + 95.0, by), vec2(20.0, 17.0)), "S", solo, Color32::from_rgb(0xe8, 0xc8, 0x3c), ("solo", i)) {
            solo = !solo;
            changed = true;
        }
        // volume: a small slider (dB)
        let mut vol = t.volume_db;
        let vr = Rect::from_min_size(pos2(row.min.x + 124.0, by + 1.0), vec2(hw - 142.0, 15.0));
        let vresp = ui.interact(vr, ui.scope_id().with(("vol", i)), Sense::click_and_drag()).on_hover_text(format!("Volume {vol:.1} dB"));
        let frac = db_to_frac(vol);
        hp.rect_filled(Rect::from_center_size(pos2(vr.center().x, vr.center().y), vec2(vr.width(), 4.0)), 2.0, look::group_bg());
        hp.circle_filled(pos2(vr.min.x + frac * vr.width(), vr.center().y), 6.5, look::text_dim());
        if let Some(pp) = vresp.interact_pointer_pos() {
            if vresp.dragged() || vresp.clicked() {
                vol = frac_to_db(((pp.x - vr.min.x) / vr.width()).clamp(0.0, 1.0));
                changed = true;
            }
        }
        if vresp.double_clicked() {
            vol = 0.0;
            changed = true;
        }
        if changed {
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
                tr.mute = mute;
                tr.solo = solo;
                tr.volume_db = vol;
            }
            app.sync_mix();
        }
        // regions
        for (ri, r) in t.regions.iter().enumerate() {
            let x0 = x_of(r.start as f32, scroll);
            let x1 = x_of((r.start + r.length) as f32, scroll);
            if x1 < lanes.min.x || x0 > lanes.max.x {
                continue;
            }
            let rr = Rect::from_min_max(pos2(x0, y + 3.0), pos2(x1.max(x0 + 4.0), y + ROW_H - 3.0));
            let sel = app.selected_region == Some((i, ri));
            draw_region(&lp, rr, r, t.kind, sel, t.mute || r.muted, app);
            let resp = ui.interact(rr.intersect(lanes), ui.scope_id().with(("region", i, ri)), Sense::click_and_drag());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.drag_started() || resp.clicked() || resp.double_clicked() {
                region_pressed = Some((i, ri, resp.double_clicked(), resp.drag_started(), resp.interact_pointer_pos()));
            }
            if resp.dragged() {
                // (the drag's region: with Option it is the copy, while the pointer is still over
                // the original)
                if let (Some(Drag::Region { track, region, from_tick, start }), Some(pp)) = (app.drag, resp.interact_pointer_pos()) {
                    if track == i {
                        let ri = region;
                        let tick = (pp.x - lanes.min.x) / px_per_tick + scroll * bar_t;
                        let free = ui.input(|i| i.modifiers.shift);
                        let beat = bar_t / app.project.as_ref().map_or(4.0, |p| p.signature.0 as f32);
                        let mut new = start as f32 + tick - from_tick;
                        if !free {
                            new = (new / beat).round() * beat;
                        }
                        move_region(app, i, ri, new.max(0.0) as u32);
                        app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(new.max(0.0) as u32, sig))]);
                    }
                }
            }
            if resp.drag_stopped() {
                app.drag = None;
                app.load_engine();
            }
            // the right edge: drag to change the length (snaps to beats; Shift: free)
            let edge = Rect::from_min_max(pos2(rr.max.x - 6.0, rr.min.y + 15.0), pos2(rr.max.x + 2.0, rr.max.y)).intersect(lanes);
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
                    let mut end = tick;
                    if !free {
                        end = (end / beat).round() * beat;
                    }
                    let len = (end - r.start as f32).max(beat) as u32;
                    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)).and_then(|t| t.regions.get_mut(ri)) {
                        rg.length = len;
                    }
                    app.help_tag = Some(vec![("Length Change:".into(), String::new()), ("Position:".into(), crate::app::bbdt(r.start + len, sig)), ("Length:".into(), crate::app::bbdt_len(len, sig))]);
                }
            }
            if eresp.drag_stopped() {
                app.drag = None;
                app.load_engine();
            }
        }
    }
    // click on an empty lane: nothing selected
    if empty.clicked() && region_pressed.is_none() {
        app.selected_region = None;
        app.selected_notes.clear();
    }
    if let Some((t, r, double, drag, pp)) = region_pressed {
        app.selected_track = Some(t);
        app.selected_region = Some((t, r));
        if double {
            app.bottom = crate::app::Bottom::PianoRoll;
        }
        app.center_roll(t, r);
        if drag {
            // Option-drag: a copy moves, the original stays (Logic)
            let copy = ui.input(|i| i.modifiers.alt);
            app.checkpoint(if copy { "Copy Region" } else { "Move Region" });
            let mut r = r;
            if copy {
                if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    if let Some(mut rg) = tr.regions.get(r).cloned() {
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
    // bar lines over the lanes
    for b in first..first + visible {
        let x = x_of(b as f32 * bar_t, scroll);
        lp.vline(x, lanes.y_range(), Stroke::new(1.0, look::grid_line().gamma_multiply(0.7)));
    }
    // the playhead
    let ph = x_of(app.position() as f32, scroll);
    if ph >= lanes.min.x && ph <= lanes.max.x {
        let full = ui.painter_at(Rect::from_min_max(ruler.min, area.max));
        full.vline(ph, ruler.min.y + 14.0..=area.max.y, Stroke::new(1.2, look::playhead()));
        full.add(egui::Shape::convex_polygon(vec![pos2(ph - 6.0, ruler.min.y + 14.0), pos2(ph + 6.0, ruler.min.y + 14.0), pos2(ph, ruler.min.y + 22.0)], look::playhead(), Stroke::NONE));
    }
    // follow the playhead while playing (Logic's Catch)
    if app.playing() {
        let bar_pos = app.position() as f32 / bar_t;
        let visible_bars = lanes.width() / app.bar_w;
        if bar_pos > app.scroll_x + visible_bars * 0.92 || bar_pos < app.scroll_x {
            app.scroll_x = bar_pos.floor().max(0.0);
        }
    }
    // the divider between headers and lanes: drag to widen the headers (265–547 pt)
    p.vline(head.max.x, area.min.y + LOCAL_H..=area.max.y, Stroke::new(1.0, look::line()));
    let edge = Rect::from_min_max(pos2(head.max.x - 3.0, head.max.y), pos2(head.max.x + 3.0, area.max.y));
    let e = ui.interact(edge, ui.scope_id().with("header-edge"), Sense::drag());
    if e.hovered() || e.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if let (true, Some(pp)) = (e.dragged(), e.interact_pointer_pos()) {
        app.header_w = (pp.x - area.min.x).clamp(HEADER_W, 547.0);
    }
}

fn local_menus(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("Edit ⌄", |ui| {
        if ui.button("Select All").clicked() {
            app.tell("Select All isn't available yet");
            ui.close();
        }
        if ui.button("Undo").clicked() {
            app.undo();
            ui.close();
        }
        if ui.button("Delete").clicked() {
            app.delete_selection();
            ui.close();
        }
    });
    ui.menu_button("Functions ⌄", |ui| {
        if ui.button("Set Locators by Selection and Enable Cycle").clicked() {
            if let Some((t, r)) = app.selected_region {
                if let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)) {
                    let c = (rg.start, rg.start + rg.length);
                    app.set_cycle(c);
                    if !app.cycle_on() {
                        app.toggle_cycle();
                    }
                }
            }
            ui.close();
        }
    });
    ui.menu_button("View ⌄", |ui| {
        if ui.button("Zoom In").clicked() {
            app.bar_w = (app.bar_w * 1.25).min(1200.0);
            ui.close();
        }
        if ui.button("Zoom Out").clicked() {
            app.bar_w = (app.bar_w / 1.25).max(8.0);
            ui.close();
        }
        if ui.button("Zoom to Fit").clicked() {
            app.bar_w = 70.0;
            app.scroll_x = 0.0;
            ui.close();
        }
    });
}

fn zoom_at(app: &mut App, factor: f32, at_x: f32, lanes_x: f32) {
    let bar_at = (at_x - lanes_x) / app.bar_w + app.scroll_x;
    app.bar_w = (app.bar_w * factor).clamp(8.0, 1200.0);
    app.scroll_x = (bar_at - (at_x - lanes_x) / app.bar_w).max(0.0);
}

fn move_region(app: &mut App, t: usize, r: usize, start: u32) {
    let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) else { return };
    let d = start as i64 - rg.start as i64;
    if d == 0 {
        return;
    }
    rg.start = start;
    for n in &mut rg.notes {
        n.pos = (n.pos as i64 + d).max(0) as u32;
    }
}

fn new_track(app: &mut App) {
    app.checkpoint("Create New Track");
    let Some(p) = app.project.as_mut() else { return };
    let row = p.tracks.len() as u32 + 1;
    p.tracks.push(lg_core::Track { name: "Deluxe Classic".into(), kind: TrackKind::Instrument, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false });
    app.programs.push(4);
    app.selected_track = Some(p.tracks.len() - 1);
    app.load_engine();
}

fn draw_region(p: &egui::Painter, r: Rect, region: &lg_core::Region, kind: TrackKind, selected: bool, muted: bool, app: &App) {
    let base = look::region_color(kind);
    let base = if muted { Color32::from_gray(110) } else { base };
    let body = if selected { look::mix(base, Color32::WHITE, 0.25) } else { base };
    p.rect_filled(r, 4.0, body.gamma_multiply(0.9));
    let header = Rect::from_min_size(r.min, vec2(r.width(), 15.0));
    p.rect_filled(header, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, look::mix(body, Color32::WHITE, 0.35));
    if r.width() > 24.0 {
        p.with_clip_rect(header).text(pos2(header.min.x + 4.0, header.center().y), Align2::LEFT_CENTER, &region.name, look::semibold(10.5), Color32::from_rgb(0x10, 0x20, 0x10));
    }
    let inner = Rect::from_min_max(pos2(r.min.x + 2.0, header.max.y + 2.0), pos2(r.max.x - 2.0, r.max.y - 3.0));
    let ink = look::mix(body, Color32::BLACK, 0.55);
    if let Some(a) = &region.audio {
        // the waveform (from the engine's decoded samples)
        let samples = app.engine.lock().ok().and_then(|e| e.tracks.iter().flat_map(|t| t.audio.iter()).find(|(s, _)| *s == region.start).map(|(_, s)| s.clone()));
        let mid = inner.center().y;
        match samples {
            Some(s) if !s.is_empty() => {
                let w = inner.width().max(1.0) as usize;
                let per = (s.len() / 2 / w).max(1);
                for k in 0..w {
                    let from = k * per * 2;
                    let peak = s.get(from..(from + per * 2).min(s.len())).map_or(0.0, |c| c.iter().fold(0f32, |m, x| m.max(x.abs())));
                    let h = peak * inner.height() * 0.5;
                    let x = inner.min.x + k as f32;
                    p.vline(x, mid - h..=mid + h, Stroke::new(1.0, ink));
                }
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
        let x0 = r.min.x + (n.pos.saturating_sub(region.start)) as f32 / bar_t * app.bar_w;
        let x1 = x0 + (n.len as f32 / bar_t * app.bar_w).max(1.5);
        if x0 > r.max.x {
            continue;
        }
        let y = inner.max.y - (n.pitch as f32 - lo) / span * (inner.height() - 2.0) - 1.0;
        p.line_segment([pos2(x0, y), pos2(x1.min(r.max.x - 1.0), y)], Stroke::new(1.6, ink));
    }
}

fn track_icon(p: &egui::Painter, r: Rect, kind: TrackKind) {
    let c = look::region_color(kind);
    p.rect_filled(r, 6.0, c.gamma_multiply(0.22));
    let m = r.center();
    let st = Stroke::new(1.5, c);
    match kind {
        TrackKind::Instrument => {
            // a keyboard
            let k = Rect::from_center_size(m, vec2(22.0, 14.0));
            p.rect_stroke(k, 2.0, st, egui::StrokeKind::Middle);
            for i in 1..4 {
                let x = k.min.x + i as f32 * k.width() / 4.0;
                p.vline(x, k.min.y..=k.max.y, Stroke::new(1.0, c));
            }
        }
        TrackKind::Audio => {
            for (i, h) in [4.0, 9.0, 6.0, 11.0, 5.0, 8.0].iter().enumerate() {
                let x = m.x - 10.0 + i as f32 * 4.0;
                p.vline(x, m.y - h..=m.y + h, st);
            }
        }
        TrackKind::Drummer => {
            p.circle_stroke(m, 9.0, st);
            p.line_segment([m + vec2(-9.0, -2.0), m + vec2(9.0, -2.0)], st);
            p.line_segment([m + vec2(-12.0, -13.0), m + vec2(-3.0, -5.0)], st);
            p.line_segment([m + vec2(12.0, -13.0), m + vec2(3.0, -5.0)], st);
        }
    }
}

fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

/// Logic's fader law (roughly): -inf..+6 dB over 0..1, 0 dB at about 0.73.
pub fn db_to_frac(db: f32) -> f32 {
    if db <= -60.0 { 0.0 } else { ((db + 60.0) / 66.0).powf(1.6).clamp(0.0, 1.0) }
}

pub fn frac_to_db(f: f32) -> f32 {
    if f <= 0.001 { -96.0 } else { f.powf(1.0 / 1.6) * 66.0 - 60.0 }
}
