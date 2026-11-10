//! The Piano Roll (Logic Pro 13's editor pane): tabs (Piano Roll, Score, Smart Tempo), the
//! local menus, the keyboard on the left (C3 marked) and the grid with the selected region's
//! notes, coloured by velocity as Logic does (blue soft → green → yellow → red loud). The wheel
//! scrolls the pitches, Shift + wheel the time, ⌘ + wheel or a pinch zooms; a click on a key
//! plays it.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

const TABS_H: f32 = 40.0;
const RULER_H: f32 = 34.0;
const KEYS_W: f32 = 56.0;
const LOCAL_H: f32 = 28.0;
const ROLL_INSPECTOR_W: f32 = 220.0;
const ROW: f32 = 13.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    // the tabs: Piano Roll (on), Score, Smart Tempo
    let tabs = Rect::from_min_size(area.min, vec2(area.width(), TABS_H));
    let names = ["Piano Roll", "Score", "Smart Tempo"];
    let tw = 150.0;
    let tx0 = tabs.center().x - tw * 1.5;
    let pill = Rect::from_min_size(pos2(tx0, tabs.min.y + 7.0), vec2(tw * 3.0, 26.0));
    p.rect_filled(pill, 8.0, look::group_bg());
    for (i, n) in names.iter().enumerate() {
        let r = Rect::from_min_size(pos2(tx0 + i as f32 * tw, tabs.min.y + 7.0), vec2(tw, 26.0));
        if i == 0 {
            p.rect_filled(r.shrink(2.0), 7.0, look::accent());
        }
        p.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if i == 0 { Color32::WHITE } else { look::text() });
        if i > 0 && ui.interact(r, ui.scope_id().with(("edtab", i)), Sense::click()).clicked() {
            app.tell(format!("The {n} editor isn't available yet"));
        }
    }
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
        p.text(area.center(), Align2::CENTER_CENTER, "Select a MIDI region to see its notes", look::regular(14.0), look::text_dim());
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
    ui.scope_builder(egui::UiBuilder::new().max_rect(local.shrink2(vec2(10.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        for (title, name) in [("Edit", "pianoroll-Edit"), ("Functions", "pianoroll-Functions"), ("View", "pianoroll-View")] {
            ui.menu_button(format!("{title} ⌄"), |ui| crate::menus::show_items(app, ui, crate::menus::local(name)));
        }
        let label = if app.roll_inspector { "Hide Inspector" } else { "Show Inspector" };
        if ui.small_button("◧").on_hover_text(label).clicked() {
            app.roll_inspector = !app.roll_inspector;
        }
    });
    let mut left = area.min.x;
    if app.roll_inspector && area.width() > 700.0 {
        let ir = Rect::from_min_max(pos2(area.min.x, local.max.y), pos2(area.min.x + ROLL_INSPECTOR_W, area.max.y));
        inspector(app, ui, ir, ti, ri);
        left = ir.max.x + 1.0;
    }
    let tabs = Rect::from_min_max(pos2(left, tabs.min.y), pos2(tabs.max.x, tabs.max.y + LOCAL_H));
    let grid_top = tabs.max.y + RULER_H;
    let keys = Rect::from_min_max(pos2(left, grid_top), pos2(left + KEYS_W, area.max.y));
    let grid = Rect::from_min_max(pos2(keys.max.x, grid_top), area.max);
    let ruler = Rect::from_min_max(pos2(grid.min.x, tabs.max.y), pos2(grid.max.x, grid_top));
    let bar_t = app.bar_ticks() as f32;
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
            app.roll_top = (app.roll_top + scroll.y / ROW).clamp(12.0, 127.0);
        }
    }
    let (sx, bw, top) = (app.roll_scroll_x, app.roll_bar_w, app.roll_top);
    let x_of = move |tick: f32| grid.min.x + (tick / bar_t - sx) * bw;
    let y_of = move |pitch: f32| grid.min.y + (top - pitch) * ROW;
    let gp = ui.painter_at(grid);
    let kp = ui.painter_at(keys);
    // the keyboard's white keys under everything
    kp.rect_filled(keys, 0.0, Color32::from_gray(0xee));
    // rows: black-key rows darker
    let lo = (app.roll_top - grid.height() / ROW).floor() as i32 - 1;
    for pitch in lo.max(0)..=(app.roll_top.ceil() as i32).min(127) {
        let y = y_of(pitch as f32);
        let row = Rect::from_min_max(pos2(grid.min.x, y - ROW), pos2(grid.max.x, y));
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        if black {
            gp.rect_filled(row, 0.0, look::lane_alt());
        }
        if pitch % 12 == 0 {
            gp.hline(grid.x_range(), y, Stroke::new(1.0, look::line()));
        }
        // the keyboard
        let key = Rect::from_min_max(pos2(keys.min.x, y - ROW), pos2(keys.max.x, y));
        if black {
            kp.rect_filled(Rect::from_min_max(key.min, pos2(keys.min.x + KEYS_W * 0.62, key.max.y)), 1.5, Color32::from_gray(0x1a));
        } else if matches!(pitch % 12, 0 | 5) {
            // the line under C and F (between two white keys)
            kp.hline(keys.x_range(), y, Stroke::new(1.0, Color32::from_gray(0x9a)));
        }
        if pitch % 12 == 0 {
            kp.text(pos2(keys.max.x - 3.0, key.center().y), Align2::RIGHT_CENTER, format!("C{}", pitch / 12 - 2), look::semibold(9.5), Color32::from_gray(0x40));
        }
        let kr = ui.interact(key, ui.scope_id().with(("key", pitch)), Sense::click());
        crate::menus::context_menu(app, &kr, "pianoroll-keyboard");
        if kr.clicked() {
            preview(app, ti, pitch as u8);
        }
    }
    // bars and beats
    let rp = ui.painter_at(ruler);
    rp.rect_filled(ruler, 0.0, look::header_bg());
    let beats = app.project.as_ref().map_or(4, |p| p.signature.0) as i32;
    let first = app.roll_scroll_x.floor() as i32;
    for b in first..first + (grid.width() / app.roll_bar_w).ceil() as i32 + 1 {
        for k in 0..beats {
            let x = x_of(b as f32 * bar_t + k as f32 * bar_t / beats as f32);
            let strong = k == 0;
            gp.vline(x, grid.y_range(), Stroke::new(1.0, if strong { look::line() } else { look::grid_line() }));
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
    // copies), double-click on the grid adds a 1/16 note, Delete removes the selection
    let bg = ui.interact(grid, ui.scope_id().with("roll-bg"), Sense::click());
    crate::menus::context_menu(app, &bg, "pianoroll-empty-grid");
    let mut pressed_note = None;
    let sixteenth = lg_core::events::PPQ as f32 / 4.0;
    for (k, n) in region.notes.iter().enumerate() {
        let y = y_of(n.pitch as f32);
        let r = Rect::from_min_max(pos2(x_of(n.pos as f32), y - ROW + 1.5), pos2(x_of((n.pos + n.len) as f32).max(x_of(n.pos as f32) + 3.0), y - 1.5));
        if !r.intersects(grid) {
            continue;
        }
        let selected = app.selected_notes.contains(&k);
        let c = velocity_color(n.velocity);
        gp.rect_filled(r, 2.0, if selected { look::mix(c, Color32::WHITE, 0.45) } else { c });
        gp.rect_stroke(r, 2.0, Stroke::new(if selected { 1.6 } else { 1.0 }, if selected { Color32::WHITE } else { look::mix(c, Color32::BLACK, 0.45) }), egui::StrokeKind::Inside);
        // the velocity bar inside, as Logic draws it
        if r.width() > 8.0 {
            let w = (r.width() - 4.0) * n.velocity as f32 / 127.0;
            gp.hline(r.min.x + 2.0..=r.min.x + 2.0 + w, r.center().y, Stroke::new(1.0, look::mix(c, Color32::BLACK, 0.5)));
        }
        let resp = ui.interact(r.intersect(grid), ui.scope_id().with(("note", k)), Sense::click_and_drag());
        if resp.secondary_clicked() && !app.selected_notes.contains(&k) {
            app.selected_notes = vec![k];
        }
        crate::menus::context_menu(app, &resp, "pianoroll-note");
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() || resp.drag_started() {
            pressed_note = Some((k, resp.drag_started(), resp.interact_pointer_pos()));
        }
        if resp.dragged() {
            if let (Some(crate::app::Drag::Notes { from_tick, from_pitch, .. }), Some(pp)) = (app.drag, resp.interact_pointer_pos()) {
                let tick = (pp.x - grid.min.x) / bw * bar_t + sx * bar_t;
                let pitch = top - (pp.y - grid.min.y) / ROW + 1.0;
                let free = ui.input(|i| i.modifiers.shift);
                let mut dt = tick - from_tick;
                if !free {
                    dt = (dt / sixteenth).round() * sixteenth;
                }
                let dp = (pitch - from_pitch).round() as i32;
                let orig = Some(app.drag_notes.clone());
                let sel = app.selected_notes.clone();
                if let (Some(orig), Some(rg)) = (orig, app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)).and_then(|t| t.regions.get_mut(ri))) {
                    for &i in &sel {
                        if let (Some(o), Some(n)) = (orig.get(i), rg.notes.get_mut(i)) {
                            n.pos = (o.pos as f32 + dt).max(0.0) as u32;
                            n.pitch = (o.pitch as i32 + dp).clamp(0, 127) as u8;
                        }
                    }
                    // Logic's help tag: the pressed note's position, length and pitch
                    if let Some(n) = rg.notes.get(k) {
                        let sig = (4, 4);
                        app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(n.pos, sig)), ("Length:".into(), crate::app::bbdt_len(n.len, sig)), ("Pitch:".into(), crate::app::note_name(n.pitch))]);
                    }
                }
            }
        }
        if resp.drag_stopped() {
            app.drag = None;
            app.load_engine();
        }
    }
    if let Some((k, drag, pp)) = pressed_note {
        let shift = ui.input(|i| i.modifiers.shift);
        if shift && !drag {
            if let Some(i) = app.selected_notes.iter().position(|x| *x == k) {
                app.selected_notes.remove(i);
            } else {
                app.selected_notes.push(k);
            }
        } else if !app.selected_notes.contains(&k) {
            app.selected_notes = vec![k];
        }
        if let (true, Some(pp)) = (drag, pp) {
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
            let tick = (pp.x - grid.min.x) / bw * bar_t + sx * bar_t;
            let pitch = top - (pp.y - grid.min.y) / ROW + 1.0;
            app.drag = Some(crate::app::Drag::Notes { from_tick: tick, from_pitch: pitch, copy });
        }
    } else if bg.double_clicked() {
        if let Some(pp) = bg.interact_pointer_pos() {
            let tick = (((pp.x - grid.min.x) / bw * bar_t + sx * bar_t) / sixteenth).floor() * sixteenth;
            let pitch = (top - (pp.y - grid.min.y) / ROW + 1.0).floor().clamp(0.0, 127.0) as u8;
            app.checkpoint("Create Note");
            if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(ti)).and_then(|t| t.regions.get_mut(ri)) {
                rg.notes.push(lg_core::events::Note { channel: 0, pitch, velocity: 80, pos: tick.max(0.0) as u32, len: sixteenth as u32 });
                app.selected_notes = vec![rg.notes.len() - 1];
            }
            app.load_engine();
        }
    } else if bg.clicked() {
        app.selected_notes.clear();
    }
    // the playhead
    let ph = x_of(app.position() as f32);
    if grid.x_range().contains(ph) {
        gp.vline(ph, grid.y_range(), Stroke::new(1.2, look::playhead()));
    }
    p.vline(keys.max.x, keys.y_range(), Stroke::new(1.0, look::line()));
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
        let (grid, strength, swing) = (app.roll_quantize as f64, app.roll_strength as f64 / 100.0, app.roll_swing as f64);
        let start = region.start as f64;
        let sel = app.selected_notes.clone();
        app.checkpoint("Quantize");
        if let Some(x) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(rg)) {
            for (i, n) in x.notes.iter_mut().enumerate() {
                if !sel.is_empty() && !sel.contains(&i) {
                    continue;
                }
                let rel = n.pos as f64 - start;
                let k = (rel / grid).round();
                let mut target = k * grid;
                if (k as i64) % 2 == 1 {
                    target += grid * (swing - 50.0) / 100.0;
                }
                n.pos = (start + rel + (target - rel) * strength).max(0.0).round() as u32;
            }
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
