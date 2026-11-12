//! Delay Designer's Tap display, as the guide describes it: the view buttons (Cutoff, Reso,
//! Transp, Pan, Level; Option-Command held shows Level for a moment), the toggle buttons above
//! the taps (the view's switch; with Option-Command they mute), the taps — click selects,
//! Shift-click adds, a drag across the background selects several, dragging one moves all the
//! selected ones together, Command-drag draws values across taps, Command-click then click
//! draws a line the taps in between follow, Option-click resets the view's value (all selected
//! taps). Cutoff shows two lines per tap (low-pass above, high-pass below; drag between them to
//! move both). Pan shows left in blue above the centre and right in green below, drag from a
//! tap's centre to draw its pan, its sides to set the spread. The first edit of a tap's filter
//! or transposition turns that module on (not again once turned off by hand). The shortcut
//! menu: Copy / Paste / Reset sound parameters, 2 x and 1/2 x delay time, Delete tap(s); the
//! Delete key deletes the selected taps. The Identification bar moves the selected taps in time
//! (drag down out of the display deletes them), and a click on empty space adds a tap. The
//! Overview display above shows every tap: with Autozoom off, drag the bright rectangle up and
//! down (or its side bars sideways) to zoom and its middle to scroll; Shift brings it to the
//! front. The tap parameter bar has arrows for the previous and next tap and a letter menu;
//! its edits go to every selected tap.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::delay_designer::*;

use crate::look;

type Set = Vec<(u32, f32)>;

const BLUE: Color32 = Color32::from_rgb(0x46, 0x9e, 0xf5);
const GREEN: Color32 = Color32::from_rgb(0x4c, 0xd0, 0x64);

fn v(params: &[Param], values: &[f32], set: &Set, id: u32) -> f32 {
    set.iter().rev().find(|x| x.0 == id).map_or_else(|| params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i]), |x| x.1)
}

/// The window's own state: the selection, the zoom, the clipboard, a line being drawn, the
/// taps whose filter / pitch was turned off by hand.
#[derive(Clone, Default)]
struct St {
    sel: Vec<usize>,
    autozoom: Option<bool>,
    zoom: (f32, f32),
    clip: Vec<Vec<f32>>,
    line_from: Option<Pos2>,
    marquee: Option<Pos2>,
    manual_off: Vec<(usize, usize)>,
}

/// The view's field and its 0..1 mapping.
fn to_t(view: usize, f: usize, x: f32) -> f32 {
    match (view, f) {
        (0, _) => ((x.max(10.0) / 10.0).log10() / 3.31).clamp(0.0, 1.0),
        (1, _) => x / 100.0,
        (2, _) => 0.5 + x / 48.0,
        (3, _) => 0.5 - x / 200.0,
        _ => x / 100.0,
    }
}
fn from_t(view: usize, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match view {
        0 => 10.0 * 10f32.powf(t * 3.31),
        1 => t * 100.0,
        2 => ((t - 0.5) * 48.0).round(),
        3 => (0.5 - t) * 200.0,
        _ => t * 100.0,
    }
}
fn field_of(view: usize) -> usize {
    [T_LP, T_RESO, T_TRANSP, T_PAN, T_LEVEL][view.min(4)]
}

/// Sets a tap's field, turning its module on the first time (filter for cutoff / resonance,
/// pitch for transposition) unless it was turned off by hand.
fn put(params: &[Param], values: &[f32], set: &mut Set, st: &St, k: usize, f: usize, x: f32) {
    set.push((tap_id(k, f), x));
    let module = match f {
        T_HP | T_LP | T_RESO => Some(T_FILTER),
        T_TRANSP | T_FINE => Some(T_PITCH),
        _ => None,
    };
    if let Some(m) = module
        && v(params, values, set, tap_id(k, m)) < 0.5
        && !st.manual_off.contains(&(k, m))
    {
        set.push((tap_id(k, m), 1.0));
    }
}

pub fn display(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set) {
    let g = |set: &Set, id| v(params, values, set, id);
    let sid = ui.scope_id().with("dd-state");
    let mut st: St = ui.data(|d| d.get_temp(sid)).unwrap_or_default();
    let view_id = ui.scope_id().with("dd-view");
    let mut view = ui.data(|d| d.get_temp::<usize>(view_id)).unwrap_or(4);
    let (opt, cmd, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.modifiers.shift));
    let autozoom = st.autozoom.unwrap_or(true);
    ui.horizontal(|ui| {
        for (k, n) in ["Cutoff", "Reso", "Transp", "Pan", "Level"].iter().enumerate() {
            if ui.add(egui::Button::selectable(view == k, *n)).clicked() {
                view = k;
            }
        }
        ui.separator();
        if ui.add(egui::Button::selectable(autozoom, "Autozoom")).clicked() {
            st.autozoom = Some(!autozoom);
        }
    });
    ui.data_mut(|d| d.insert_temp(view_id, view));
    // Option-Command: Level view for the moment, toggles mute
    let temp_level = opt && cmd;
    let view = if temp_level { 4 } else { view };
    let taps: Vec<(usize, f32)> = (0..TAPS).filter(|k| g(set, tap_id(*k, T_ON)) >= 0.5).map(|k| (k, g(set, tap_id(k, T_DELAY)))).collect();
    st.sel.retain(|k| taps.iter().any(|t| t.0 == *k));
    let primary = g(set, SELECTED).round() as usize;
    if st.sel.is_empty() && taps.iter().any(|t| t.0 == primary) {
        st.sel.push(primary);
    }
    let full = taps.iter().map(|t| t.1).fold(500.0f32, f32::max) * 1.15;
    if autozoom || st.zoom.1 <= st.zoom.0 {
        st.zoom = (0.0, full);
    }
    let (z0, z1) = st.zoom;
    // the Overview display
    let (ov, _) = ui.allocate_exact_size(vec2(ui.available_width(), if shift { 34.0 } else { 18.0 }), Sense::hover());
    // the toggles, the display, the identification bar
    let (toggle, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
    let (idbar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(area, 6.0, look::lcd_bg());
    p.rect_filled(idbar, 3.0, look::group_bg());
    let x_of = |ms: f32| area.min.x + area.width() * ((ms - z0) / (z1 - z0));
    let ms_of = |x: f32| z0 + (x - area.min.x) / area.width() * (z1 - z0);
    let fy = |t: f32| area.max.y - area.height() * t.clamp(0.0, 1.0);
    let ty = |y: f32| ((area.max.y - y) / area.height()).clamp(0.0, 1.0);
    if view == 3 {
        p.hline(area.x_range(), area.center().y, Stroke::new(0.5, look::line()));
    }
    let clip = p.with_clip_rect(area);
    let mut hit_tap = false;
    for (k, ms) in &taps {
        let x = x_of(*ms);
        if x < area.min.x - 5.0 || x > area.max.x + 5.0 {
            continue;
        }
        let selected = st.sel.contains(k);
        let on_col = if selected { Color32::WHITE } else { look::accent() };
        clip.vline(x, area.y_range(), Stroke::new(1.0, look::mix(look::lcd_bg(), on_col, 0.3)));
        match view {
            0 => {
                let (hp, lp) = (g(set, tap_id(*k, T_HP)), g(set, tap_id(*k, T_LP)));
                let (yh, yl) = (fy(to_t(0, T_HP, hp)), fy(to_t(0, T_LP, lp)));
                // the band between (filled), low-pass above, high-pass below
                clip.line_segment([pos2(x, yh), pos2(x, yl)], Stroke::new(3.0, on_col.gamma_multiply(0.35)));
                clip.line_segment([pos2(x - 5.0, yl), pos2(x + 5.0, yl)], Stroke::new(2.5, on_col));
                clip.line_segment([pos2(x - 5.0, yh), pos2(x + 5.0, yh)], Stroke::new(2.5, on_col.gamma_multiply(0.7)));
            }
            1 => {
                clip.line_segment([pos2(x, area.max.y), pos2(x, fy(g(set, tap_id(*k, T_RESO)) / 100.0))], Stroke::new(4.0, on_col));
            }
            2 => {
                clip.circle_filled(pos2(x, fy(0.5 + g(set, tap_id(*k, T_TRANSP)) / 48.0)), 4.0, on_col);
            }
            3 => {
                let pan = g(set, tap_id(*k, T_PAN));
                let spread = g(set, tap_id(*k, T_SPREAD)) / 100.0;
                let c = area.center().y;
                let y = fy(to_t(3, T_PAN, pan));
                let col = if pan < 0.0 { BLUE } else { GREEN };
                clip.line_segment([pos2(x, c), pos2(x, y)], Stroke::new(3.0, if selected { Color32::WHITE } else { col }));
                clip.line_segment([pos2(x - 14.0 * spread, y), pos2(x + 14.0 * spread, y)], Stroke::new(2.0, col));
                clip.circle_filled(pos2(x, y), 3.5, col);
            }
            _ => {
                clip.line_segment([pos2(x, area.max.y), pos2(x, fy(g(set, tap_id(*k, T_LEVEL)) / 100.0))], Stroke::new(4.0, on_col));
            }
        }
        // the toggle above the tap: the view's switch (Option-Command: mute)
        let tr = Rect::from_center_size(pos2(x, toggle.center().y), vec2(14.0, 14.0));
        let tf = if temp_level { T_MUTE } else { [T_FILTER, T_SLOPE, T_PITCH, T_FLIP, T_MUTE][view] };
        let ton = g(set, tap_id(*k, tf)) >= 0.5;
        p.circle(tr.center(), 5.0, if ton { look::accent() } else { look::group_bg() }, Stroke::new(1.0, look::line()));
        if ui.interact(tr, ui.scope_id().with(("dd-tog", *k)), Sense::click()).clicked() {
            set.push((tap_id(*k, tf), if ton { 0.0 } else { 1.0 }));
            if ton && matches!(tf, T_FILTER | T_PITCH) {
                st.manual_off.push((*k, tf));
            }
        }
        // the tap in the display
        let col = Rect::from_center_size(pos2(x, area.center().y), vec2(10.0, area.height()));
        let resp = ui.interact(col, ui.scope_id().with(("dd-edit", *k)), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            hit_tap = true;
        }
        if resp.clicked() || resp.drag_started() {
            if opt && !cmd {
                // Option-click: the view's value back to its default, for every selected tap
                let f = field_of(view);
                let targets = if selected { st.sel.clone() } else { vec![*k] };
                for t in targets {
                    set.push((tap_id(t, f), FIELDS[f].3));
                    if view == 0 {
                        set.push((tap_id(t, T_HP), FIELDS[T_HP].3));
                    }
                    if view == 3 {
                        set.push((tap_id(t, T_SPREAD), FIELDS[T_SPREAD].3));
                    }
                }
            } else if shift {
                if selected {
                    st.sel.retain(|s| s != k);
                } else {
                    st.sel.push(*k);
                }
            } else if !selected {
                st.sel = vec![*k];
            }
            set.push((SELECTED, *k as f32));
        }
        if resp.dragged() && !cmd && !opt
            && let Some(pp) = resp.interact_pointer_pos()
        {
            let dy = -resp.drag_delta().y / area.height();
            let start = resp.interact_pointer_pos().map_or(pp, |q| q - resp.drag_delta());
            for t in st.sel.clone() {
                match view {
                    0 => {
                        let (hp, lp) = (g(set, tap_id(t, T_HP)), g(set, tap_id(t, T_LP)));
                        let (yh, yl) = (fy(to_t(0, T_HP, hp)), fy(to_t(0, T_LP, lp)));
                        // between the lines: both move; near one: that one
                        let between = start.y > yl + 4.0 && start.y < yh - 4.0;
                        let near_hp = (start.y - yh).abs() < (start.y - yl).abs();
                        if between || t != *k {
                            put(params, values, set, &st, t, T_LP, from_t(0, to_t(0, T_LP, lp) + dy).min(20480.0));
                            put(params, values, set, &st, t, T_HP, from_t(0, to_t(0, T_HP, hp) + dy).min(20000.0));
                        } else if near_hp {
                            put(params, values, set, &st, t, T_HP, from_t(0, to_t(0, T_HP, hp) + dy).min(20000.0));
                        } else {
                            put(params, values, set, &st, t, T_LP, from_t(0, to_t(0, T_LP, lp) + dy).min(20480.0));
                        }
                    }
                    3 => {
                        // from the centre: pan; at the sides of the dot: spread
                        let px = x_of(g(set, tap_id(t, T_DELAY)));
                        if t == *k && (start.x - px).abs() > 5.0 {
                            let sp = (g(set, tap_id(t, T_SPREAD)) + resp.drag_delta().x * if start.x > px { 3.0 } else { -3.0 }).clamp(0.0, 100.0);
                            set.push((tap_id(t, T_SPREAD), sp));
                        } else {
                            let pan = g(set, tap_id(t, T_PAN));
                            set.push((tap_id(t, T_PAN), from_t(3, to_t(3, T_PAN, pan) + dy).clamp(-100.0, 100.0)));
                        }
                    }
                    _ => {
                        let f = field_of(view);
                        let cur = g(set, tap_id(t, f));
                        let nv = if view == 2 { (cur + dy * 48.0).clamp(-24.0, 24.0) } else { from_t(view, to_t(view, f, cur) + dy) };
                        put(params, values, set, &st, t, f, nv);
                    }
                }
            }
            if view == 2 {
                // (transposition moves in whole semitones: keep the remainder for the next frame)
                for t in st.sel.clone() {
                    let id = tap_id(t, T_TRANSP);
                    if let Some(x) = set.iter_mut().rev().find(|x| x.0 == id) {
                        x.1 = x.1.round();
                    }
                }
            }
        }
        // the identification bar: drag the selected taps in time, down out of the display deletes
        let lr = Rect::from_center_size(pos2(x, idbar.center().y), vec2(16.0, idbar.height()));
        p.text(lr.center(), Align2::CENTER_CENTER, tap_letter(*k).to_string(), look::semibold(12.0), on_col);
        let lresp = ui.interact(lr, ui.scope_id().with(("dd-tap", *k)), Sense::click_and_drag());
        if lresp.clicked() || lresp.drag_started() {
            if shift {
                if !selected {
                    st.sel.push(*k);
                }
            } else if !selected {
                st.sel = vec![*k];
            }
            set.push((SELECTED, *k as f32));
        }
        if lresp.dragged()
            && let Some(pp) = lresp.interact_pointer_pos()
        {
            let dms = lresp.drag_delta().x / area.width() * (z1 - z0);
            if pp.y > idbar.max.y + 30.0 && taps.len() > st.sel.len() {
                for t in st.sel.clone() {
                    set.push((tap_id(t, T_ON), 0.0));
                }
            } else {
                for t in st.sel.clone() {
                    let d = g(set, tap_id(t, T_DELAY));
                    set.push((tap_id(t, T_DELAY), (d + dms).clamp(1.0, 10000.0)));
                }
            }
        }
    }
    // the background: marquee selection, Command-drag drawing, Command-click lines
    let bg = ui.interact(area, ui.scope_id().with("dd-bg"), Sense::click_and_drag());
    if !hit_tap {
        if cmd && !opt && bg.dragged()
            && let Some(pp) = bg.interact_pointer_pos()
        {
            // the taps under the pointer take its value
            for (k, ms) in &taps {
                if (x_of(*ms) - pp.x).abs() < 6.0 {
                    let f = field_of(view);
                    put(params, values, set, &st, *k, f, from_t(view, ty(pp.y)));
                }
            }
        } else if bg.drag_started() && !cmd {
            st.marquee = bg.interact_pointer_pos();
        } else if bg.clicked() && cmd {
            match (st.line_from, bg.interact_pointer_pos()) {
                (None, Some(a)) => st.line_from = Some(a),
                (Some(a), Some(b)) => {
                    // the taps between follow the line
                    let (l, r) = if a.x < b.x { (a, b) } else { (b, a) };
                    for (k, ms) in &taps {
                        let x = x_of(*ms);
                        if x >= l.x && x <= r.x {
                            let y = if r.x > l.x { l.y + (r.y - l.y) * (x - l.x) / (r.x - l.x) } else { l.y };
                            put(params, values, set, &st, *k, field_of(view), from_t(view, ty(y)));
                        }
                    }
                    st.line_from = None;
                }
                _ => {}
            }
        } else if bg.clicked() {
            st.sel.clear();
        }
    }
    if let (Some(a), Some(pp)) = (st.line_from, ui.ctx().pointer_hover_pos()) {
        clip.line_segment([a, pp], Stroke::new(1.0, Color32::WHITE));
    }
    if let Some(a) = st.marquee {
        if let Some(b) = bg.interact_pointer_pos().filter(|_| bg.dragged()) {
            let r = Rect::from_two_pos(a, b);
            clip.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
            st.sel = taps.iter().filter(|(_, ms)| r.x_range().contains(x_of(*ms))).map(|t| t.0).collect();
        }
        if !bg.dragged() {
            st.marquee = None;
            if let Some(f) = st.sel.first() {
                set.push((SELECTED, *f as f32));
            }
        }
    }
    // the shortcut menu
    bg.context_menu(|ui| {
        let sound: Vec<usize> = (2..FIELDS.len()).filter(|f| *f != T_DELAY).collect();
        if ui.button("Copy sound parameters").clicked() {
            st.clip = st.sel.iter().map(|t| sound.iter().map(|f| g(set, tap_id(*t, *f))).collect()).collect();
            ui.close();
        }
        if ui.add_enabled(!st.clip.is_empty(), egui::Button::new("Paste sound parameters")).clicked() {
            // (clipboard taps beyond the selection are ignored)
            for (t, vals) in st.sel.clone().iter().zip(st.clip.clone()) {
                for (f, x) in sound.iter().zip(vals) {
                    set.push((tap_id(*t, *f), x));
                }
            }
            ui.close();
        }
        if ui.button("Reset sound parameters to default values").clicked() {
            for t in &st.sel {
                for f in &sound {
                    set.push((tap_id(*t, *f), FIELDS[*f].3));
                }
            }
            ui.close();
        }
        ui.separator();
        for (label, k) in [("2 x delay time", 2.0f32), ("1/2 x delay time", 0.5)] {
            if ui.button(label).clicked() {
                for t in &st.sel {
                    let d = g(set, tap_id(*t, T_DELAY));
                    set.push((tap_id(*t, T_DELAY), (d * k).clamp(1.0, 10000.0)));
                }
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(taps.len() > st.sel.len(), egui::Button::new("Delete tap(s)")).clicked() {
            for t in &st.sel {
                set.push((tap_id(*t, T_ON), 0.0));
            }
            ui.close();
        }
    });
    // Delete: the selected taps go (while the display has the key focus: clicked last)
    let focus_id = ui.scope_id().with("dd-key-focus");
    if let Some(p) = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()) {
        ui.data_mut(|d| d.insert_temp(focus_id, area.union(idbar).contains(p)));
    }
    if ui.data(|d| d.get_temp::<bool>(focus_id)).unwrap_or(false) && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) && taps.len() > st.sel.len() {
        for t in &st.sel {
            set.push((tap_id(*t, T_ON), 0.0));
        }
    }
    // a click on the identification bar (not on a letter) adds a tap there
    let resp = ui.interact(idbar, ui.scope_id().with("dd-idbar"), Sense::click());
    if resp.clicked()
        && let Some(pp) = resp.interact_pointer_pos()
        && !taps.iter().any(|t| (x_of(t.1) - pp.x).abs() < 9.0)
        && let Some(k) = (0..TAPS).find(|k| g(set, tap_id(*k, T_ON)) < 0.5)
    {
        let ms = ms_of(pp.x).clamp(1.0, 10000.0);
        for (f, (_, _, _, d, _)) in FIELDS.iter().enumerate() {
            set.push((tap_id(k, f), *d));
        }
        set.push((tap_id(k, T_ON), 1.0));
        set.push((tap_id(k, T_DELAY), ms));
        set.push((SELECTED, k as f32));
        st.sel = vec![k];
    }
    // the Overview display: every tap, the visible part as a bright rectangle
    let ovp = if shift { ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("dd-ov"))) } else { p.clone() };
    ovp.rect_filled(ov, 3.0, look::group_bg());
    let ox = |ms: f32| ov.min.x + ov.width() * (ms / full);
    for (_, ms) in &taps {
        ovp.vline(ox(*ms), ov.y_range(), Stroke::new(1.0, look::accent()));
    }
    let vis = Rect::from_min_max(pos2(ox(z0), ov.min.y + 1.0), pos2(ox(z1).min(ov.max.x), ov.max.y - 1.0));
    ovp.rect_stroke(vis, 2.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
    if !autozoom {
        let mid = ui.interact(vis.shrink2(vec2(5.0, 0.0)), ui.scope_id().with("dd-ov-mid"), Sense::drag());
        if mid.dragged() {
            // sideways: scroll; up / down: zoom
            let d = mid.drag_delta();
            let span = z1 - z0;
            let shift_ms = d.x / ov.width() * full;
            let zoom = (1.0 + d.y * 0.01).clamp(0.5, 2.0);
            let c = (z0 + z1) / 2.0 + shift_ms;
            let ns = (span * zoom).clamp(20.0, full);
            let a = (c - ns / 2.0).clamp(0.0, full - ns);
            st.zoom = (a, a + ns);
        }
        for (k, edge) in [vis.min.x, vis.max.x].into_iter().enumerate() {
            let er = Rect::from_center_size(pos2(edge, vis.center().y), vec2(8.0, vis.height()));
            ovp.rect_filled(er.shrink2(vec2(2.0, 2.0)), 1.0, Color32::from_white_alpha(90));
            let r = ui.interact(er, ui.scope_id().with(("dd-ov-edge", k)), Sense::drag());
            if r.dragged() {
                let d = r.drag_delta().x / ov.width() * full;
                if k == 0 {
                    st.zoom.0 = (st.zoom.0 + d).clamp(0.0, st.zoom.1 - 20.0);
                } else {
                    st.zoom.1 = (st.zoom.1 + d).clamp(st.zoom.0 + 20.0, full);
                }
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(sid, st));
}

/// The tap parameter bar's head: arrows for the previous / next tap (in time) and the letter
/// menu; edits in the bar go to every selected tap.
pub fn bar_head(ui: &mut egui::Ui, params: &[Param], values: &[f32], set: &mut Set) {
    let g = |id| v(params, values, set, id);
    let taps: Vec<(usize, f32)> = {
        let mut t: Vec<(usize, f32)> = (0..TAPS).filter(|k| g(tap_id(*k, T_ON)) >= 0.5).map(|k| (k, g(tap_id(k, T_DELAY)))).collect();
        t.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        t
    };
    let sel = g(SELECTED).round() as usize;
    let at = taps.iter().position(|t| t.0 == sel).unwrap_or(0);
    ui.horizontal(|ui| {
        if ui.small_button("◀").on_hover_text("Previous tap").clicked() && at > 0 {
            set.push((SELECTED, taps[at - 1].0 as f32));
        }
        if ui.small_button("▶").on_hover_text("Next tap").clicked() && at + 1 < taps.len() {
            set.push((SELECTED, taps[at + 1].0 as f32));
        }
        egui::ComboBox::from_id_salt("dd-letter").selected_text(format!("Tap {}", tap_letter(sel))).width(70.0).show_ui(ui, |ui| {
            for (k, _) in &taps {
                if ui.selectable_label(*k == sel, format!("Tap {}", tap_letter(*k))).clicked() {
                    set.push((SELECTED, *k as f32));
                }
            }
        });
    });
}

/// Copies a change the bar made to the selected tap onto the other selected taps.
pub fn spread_bar_edit(ui: &egui::Ui, before: usize, set: &mut Set) {
    let sid = ui.scope_id().with("dd-state");
    let st: St = ui.data(|d| d.get_temp(sid)).unwrap_or_default();
    let primary = set.iter().rev().find(|x| x.0 == SELECTED).map(|x| x.1 as usize);
    let edits: Vec<(u32, f32)> = set[before..].iter().filter(|x| x.0 >= SEL && x.0 < SEL + FIELDS.len() as u32).copied().collect();
    for (id, x) in edits {
        let f = (id - SEL) as usize;
        for t in st.sel.iter().filter(|t| Some(**t) != primary) {
            set.push((tap_id(*t, f), x));
        }
    }
}
