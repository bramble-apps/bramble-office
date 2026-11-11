//! Track automation, as Logic's (A shows it): each track's lanes (Volume, Pan… by Logic's
//! parameter numbers: 7 volume, 10 pan), drawn over the track's row as a line with points; the
//! header's parameter pop-up and automation mode (Off, Read, Touch, Latch, Write). A click on the
//! line adds a point, a drag moves one (Shift: fine), Option-click removes one. Playing in Read
//! (and Touch/Latch/Write) the channel follows the line; moving a fader while playing in Touch,
//! Latch or Write records points there.
//!
//! Values are kept as Logic stores them, 0–127: volume 90 = 0 dB (127 = +6 dB), pan 64 = centre.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::project::AutoLane;

use crate::app::App;
use crate::look;

pub const VOLUME: u8 = 7;
pub const PAN: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AutoMode {
    Off,
    #[default]
    Read,
    Touch,
    Latch,
    Write,
}

impl AutoMode {
    pub const ALL: [AutoMode; 5] = [AutoMode::Off, AutoMode::Read, AutoMode::Touch, AutoMode::Latch, AutoMode::Write];
    pub fn name(self) -> &'static str {
        match self {
            AutoMode::Off => "Off",
            AutoMode::Read => "Read",
            AutoMode::Touch => "Touch",
            AutoMode::Latch => "Latch",
            AutoMode::Write => "Write",
        }
    }
    pub fn color(self) -> Color32 {
        match self {
            AutoMode::Off => look::text_dim(),
            AutoMode::Read => Color32::from_rgb(0x4c, 0xd9, 0x64),
            AutoMode::Touch => Color32::from_rgb(0xe8, 0xc8, 0x3c),
            AutoMode::Latch => Color32::from_rgb(0xe8, 0x8a, 0x2c),
            AutoMode::Write => look::record_red(),
        }
    }
}

pub fn param_name(p: u8) -> &'static str {
    match p {
        VOLUME => "Volume",
        PAN => "Pan",
        _ => "Parameter",
    }
}

/// Logic's fader value (0–127) in dB: 90 is 0 dB, 127 is +6 dB, 0 is −∞.
pub fn logic_to_db(v: f32) -> f32 {
    if v <= 0.5 {
        -90.0
    } else if v >= 90.0 {
        (v - 90.0) * 6.0 / 37.0
    } else {
        40.0 * (v / 90.0).log10()
    }
}

pub fn db_to_logic(db: f32) -> f32 {
    if db <= -89.0 {
        0.0
    } else if db >= 0.0 {
        (90.0 + db * 37.0 / 6.0).min(127.0)
    } else {
        (90.0 * 10f32.powf(db / 40.0)).clamp(0.0, 90.0)
    }
}

/// The lane's value at `tick` (straight lines between points, as Logic draws them).
#[cfg_attr(not(test), allow(dead_code))]
pub fn value_at(points: &[(u32, u8)], tick: u32) -> Option<f32> {
    let first = points.first()?;
    if tick <= first.0 {
        return Some(first.1 as f32);
    }
    for w in points.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        if tick >= a && tick <= b {
            let t = if b > a { (tick - a) as f32 / (b - a) as f32 } else { 0.0 };
            return Some(va as f32 + (vb as f32 - va as f32) * t);
        }
    }
    points.last().map(|p| p.1 as f32)
}

/// The lane the header shows for a track (Volume unless another was picked).
pub fn shown_param(app: &App, t: usize) -> u8 {
    app.auto_param.get(&t).copied().unwrap_or(VOLUME)
}

pub fn mode(app: &App, t: usize) -> AutoMode {
    app.auto_mode.get(&t).copied().unwrap_or_default()
}

/// Which lane a track shows: a channel parameter (7 volume, 10 pan…) or a plug-in's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub param: u8,
    pub plugin: Option<(u32, u32)>,
}

pub fn shown(app: &App, t: usize) -> Key {
    match app.ta.shown_plugin.get(&t) {
        Some(&pl) => Key { param: 0, plugin: Some(pl) },
        None => Key { param: shown_param(app, t), plugin: None },
    }
}

fn matches(l: &AutoLane, k: Key) -> bool {
    match k.plugin {
        Some(pl) => l.plugin == Some(pl),
        None => l.plugin.is_none() && l.param == k.param,
    }
}

pub fn lane_of(app: &App, t: usize, k: Key) -> Option<&AutoLane> {
    app.project.as_ref()?.tracks.get(t)?.automation.iter().find(|l| matches(l, k))
}

fn lane_mut_key(app: &mut App, t: usize, k: Key) -> Option<&mut AutoLane> {
    let tr = app.project.as_mut()?.tracks.get_mut(t)?;
    if !tr.automation.iter().any(|l| matches(l, k)) {
        tr.automation.push(AutoLane { param: k.param, plugin: k.plugin, ..Default::default() });
    }
    tr.automation.iter_mut().find(|l| matches(l, k))
}

fn lane_mut(app: &mut App, t: usize, param: u8) -> Option<&mut AutoLane> {
    lane_mut_key(app, t, Key { param, plugin: None })
}

/// A plug-in parameter's name, range and value now: the track's instrument (slot 2) or an
/// Audio FX slot, as the engine builds it.
pub fn plugin_param(app: &App, t: usize, slot: u32, id: u32) -> Option<(String, String, f32, f32, f32)> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let u = tr.plugins.iter().find(|u| u.slot == slot)?;
    let rate = crate::audio::RATE as f32;
    let (params, value): (Vec<lg_dsp::Param>, f32) = if slot == 2 {
        let mut ins = crate::units::instrument(&u.name, rate)?;
        ins.load_floats(&u.params);
        (ins.params().to_vec(), ins.get(id))
    } else {
        let mut fx = crate::units::effect(&u.name, rate)?;
        fx.load_floats(&u.params);
        (fx.params().to_vec(), fx.get(id))
    };
    let p = params.iter().find(|p| p.id == id)?;
    Some((u.name.clone(), p.name.to_string(), p.min, p.max, value))
}

/// The lane's current value when it has no points (0-127): the fader, the pan knob, a plug-in
/// parameter as set.
fn current_value(app: &App, t: usize, k: Key) -> f32 {
    if let Some((slot, id)) = k.plugin {
        return plugin_param(app, t, slot, id).map_or(64.0, |(_, _, lo, hi, v)| ((v - lo) / (hi - lo).max(1e-9) * 127.0).clamp(0.0, 127.0));
    }
    let tr = app.project.as_ref().and_then(|p| p.tracks.get(t));
    match k.param {
        VOLUME => db_to_logic(tr.map_or(0.0, |t| t.volume_db)),
        PAN => 64.0 + tr.map_or(0.0, |t| t.pan) * 63.0,
        _ => 64.0,
    }
}

fn key_label(app: &App, t: usize, k: Key, v: f32) -> String {
    match (k.plugin, k.param) {
        (Some((slot, id)), _) => plugin_param(app, t, slot, id).map_or(format!("{v:.0}"), |(_, _, lo, hi, _)| format!("{:.2}", lo + v / 127.0 * (hi - lo))),
        (None, VOLUME) => format!("{:.1} dB", logic_to_db(v)),
        (None, PAN) => format!("{:+.0}", v - 64.0),
        _ => format!("{v:.0}"),
    }
}

fn key_name(app: &App, t: usize, k: Key) -> String {
    match k.plugin {
        Some((slot, id)) => plugin_param(app, t, slot, id).map_or("Parameter".into(), |(plug, name, ..)| format!("{plug}: {name}")),
        None => param_name(k.param).to_string(),
    }
}

/// Automation editing state: the selected points (on one track's shown lane) and the drag.
#[derive(Default)]
pub struct State {
    pub sel: Option<(usize, Vec<usize>)>,
    drag: Option<ADrag>,
}

#[derive(Clone, Debug)]
enum ADrag {
    /// Points moving (the press's tick and value, the points as they were).
    Points { t: usize, from: (f32, f32), orig: Vec<(u32, u8)>, which: Vec<usize> },
    Segment { t: usize, k: usize, y0: f32, orig: Vec<(u32, u8)> },
    Curve { t: usize, k: usize, y0: f32, orig: i8 },
    Select { t: usize, from: egui::Pos2 },
    Range { t: usize, from: u32, to: u32, y0: f32, orig: AutoLane },
}

/// The header's automation controls (under the track's name while automation shows): the mode
/// pop-up (its colour as Logic's) and the parameter pop-up (Volume, Pan, then each plug-in on
/// the channel strip with its parameters).
pub fn header(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    let p = ui.painter().clone();
    let m = mode(app, t);
    let mr = Rect::from_min_size(r.min, vec2(52.0, r.height()));
    let resp = ui.interact(mr, ui.scope_id().with(("automode", t)), Sense::click());
    p.rect_filled(mr, 4.0, m.color().gamma_multiply(0.35));
    p.text(mr.center(), Align2::CENTER_CENTER, m.name(), look::semibold(10.5), m.color());
    egui::Popup::menu(&resp).show(|ui| {
        for x in AutoMode::ALL {
            if ui.add(egui::Button::selectable(x == m, x.name())).clicked() {
                app.auto_mode.insert(t, x);
                app.load_engine();
                ui.close();
            }
        }
    });
    let pr = Rect::from_min_max(pos2(mr.max.x + 4.0, r.min.y), r.max);
    let key = shown(app, t);
    let presp = ui.interact(pr, ui.scope_id().with(("autoparam", t)), Sense::click());
    p.rect_filled(pr, 4.0, look::group_bg());
    let name = key_name(app, t, key);
    p.with_clip_rect(pr).text(pos2(pr.min.x + 6.0, pr.center().y), Align2::LEFT_CENTER, format!("{name} ⌄"), look::regular(10.5), look::text());
    egui::Popup::menu(&presp).show(|ui| {
        ui.set_min_width(200.0);
        let used = |app: &App, k: Key| lane_of(app, t, k).is_some_and(|l| !l.points.is_empty());
        for x in [VOLUME, PAN] {
            let k = Key { param: x, plugin: None };
            let label = if used(app, k) { format!("{} •", param_name(x)) } else { param_name(x).to_string() };
            if ui.add(egui::Button::selectable(key == k, label)).clicked() {
                app.auto_param.insert(t, x);
                app.ta.shown_plugin.remove(&t);
                ui.close();
            }
        }
        // the channel strip's plug-ins: their parameters
        let plugins: Vec<(u32, String)> = app.project.as_ref().map_or(Vec::new(), |p| p.tracks[t].plugins.iter().filter(|u| u.slot >= 2 && !u.name.is_empty()).map(|u| (u.slot, u.name.clone())).collect());
        if !plugins.is_empty() {
            ui.separator();
        }
        let rate = crate::audio::RATE as f32;
        for (slot, plug) in plugins {
            let params: Vec<lg_dsp::Param> = if slot == 2 { crate::units::instrument(&plug, rate).map(|i| i.params().to_vec()) } else { crate::units::effect(&plug, rate).map(|f| f.params().to_vec()) }.unwrap_or_default();
            if params.is_empty() {
                ui.add_enabled(false, egui::Button::new(&plug));
                continue;
            }
            ui.menu_button(&plug, |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for prm in params {
                        let k = Key { param: 0, plugin: Some((slot, prm.id)) };
                        if ui.add(egui::Button::selectable(key == k, prm.name)).clicked() {
                            app.ta.shown_plugin.insert(t, (slot, prm.id));
                            ui.close();
                        }
                    }
                });
            });
        }
    });
}

/// The line through a lane's points, curves sampled, from `x0` to `x1`.
fn polyline(l: &AutoLane, x0: f32, x1: f32, x_of: &dyn Fn(f32) -> f32, y_of: &dyn Fn(f32) -> f32) -> Vec<egui::Pos2> {
    let pts = &l.points;
    let mut line = vec![pos2(x0, y_of(pts[0].1 as f32))];
    for (k, w) in pts.windows(2).enumerate() {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        let _ = k;
        line.push(pos2(x_of(a as f32), y_of(va as f32)));
        if lg_core::automation::curve_of(l, a) != 0 && b > a {
            for s in 1..16 {
                let tick = a as f64 + (b - a) as f64 * s as f64 / 16.0;
                line.push(pos2(x_of(tick as f32), y_of(lg_core::automation::value_at(l, tick).unwrap_or(vb as f32))));
            }
        }
        let _ = vb;
    }
    let last = *pts.last().unwrap();
    line.push(pos2(x_of(last.0 as f32), y_of(last.1 as f32)));
    line.push(pos2(x1, y_of(last.1 as f32)));
    line
}

/// The lane over a track's row: the line (curves bent), its points, and editing as Logic's:
/// click the lane: a point on the line; drag a point (Shift: fine; selected points move
/// together); drag the line between two points: the segment up or down; Option- or
/// Control-Shift-drag the line: bend it; Option-click a point: remove it. The Automation Select
/// tool drags a rectangle around points; inside a marquee selection a drag moves that range.
pub fn lane(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let key = shown(app, t);
    let lane_now = lane_of(app, t, key).cloned().unwrap_or(AutoLane { param: key.param, plugin: key.plugin, ..Default::default() });
    let pts = lane_now.points.clone();
    let p = ui.painter_at(row);
    p.rect_filled(row, 0.0, Color32::from_black_alpha(70));
    let inner = row.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.max.y - v / 127.0 * inner.height();
    let v_of = |y: f32| ((inner.max.y - y) / inner.height() * 127.0).clamp(0.0, 127.0);
    let col = Color32::from_rgb(0xa8, 0xd8, 0x6a);
    let sel_col = Color32::from_rgb(0xf0, 0xd8, 0x4a);
    let current = current_value(app, t, key);
    let selected: Vec<usize> = app.ta.auto.sel.as_ref().filter(|s| s.0 == t).map(|s| s.1.clone()).unwrap_or_default();
    if pts.is_empty() {
        p.hline(row.x_range(), y_of(current), Stroke::new(1.5, col.gamma_multiply(0.6)));
    } else {
        p.add(egui::Shape::line(polyline(&lane_now, row.min.x, row.max.x, x_of, &y_of), Stroke::new(1.8, col)));
    }
    let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
    let (alt, ctrl_shift, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.ctrl && i.modifiers.shift, i.modifiers.shift));
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let mut edit: Option<AutoLane> = None;
    let mut on_point = false;
    // the background: click (a point on the line), the select rectangle, segment and range drags
    let bg = ui.interact(row, ui.scope_id().with(("autolane", t)), Sense::click_and_drag());
    // the segment under the pointer (within 5 pt of the line)
    let seg_at = |pos: egui::Pos2| -> Option<usize> {
        if pts.len() < 2 {
            return None;
        }
        let tick = tick_of(pos.x).max(0.0);
        let k = pts.windows(2).position(|w| tick >= w[0].0 as f32 && tick <= w[1].0 as f32)?;
        let v = lg_core::automation::value_at(&lane_now, tick as f64)?;
        ((y_of(v) - pos.y).abs() < 5.0).then_some(k)
    };
    let marquee = app.ta.regions.marquee.clone().filter(|m| m.tracks.contains(&t));
    for (k, (tk, v)) in pts.iter().enumerate() {
        let c = pos2(x_of(*tk as f32), y_of(*v as f32));
        if !row.contains(c) {
            continue;
        }
        let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("autopt", t, k)), Sense::click_and_drag());
        on_point |= resp.hovered() || resp.dragged();
        let is_sel = selected.contains(&k);
        p.circle_filled(c, if resp.hovered() || resp.dragged() { 5.0 } else { 3.5 }, if is_sel { sel_col } else { col });
        if is_sel {
            p.text(c + vec2(6.0, -9.0), Align2::LEFT_CENTER, key_label(app, t, key, *v as f32), look::regular(9.5), sel_col);
        }
        if resp.clicked() && alt {
            let mut n = lane_now.clone();
            n.points.remove(k);
            app.checkpoint("Delete Automation Point");
            app.ta.auto.sel = None;
            edit = Some(n);
        } else if resp.clicked() {
            app.ta.auto.sel = Some((t, vec![k]));
        } else if resp.drag_started() {
            app.checkpoint("Move Automation Point");
            let which = if is_sel { selected.clone() } else { vec![k] };
            if let Some(pp) = resp.interact_pointer_pos() {
                app.ta.auto.drag = Some(ADrag::Points { t, from: (tick_of(pp.x), v_of(pp.y)), orig: pts.clone(), which });
            }
        } else if let (true, Some(pp), Some(ADrag::Points { t: dt, from, orig, which })) = (resp.dragged(), resp.interact_pointer_pos(), app.ta.auto.drag.clone()) {
            if dt == t {
                let mut n = lane_now.clone();
                let dv = if shift { -resp.total_drag_delta().map_or(0.0, |d| d.y) * 0.2 } else { v_of(pp.y) - from.1 };
                let dt_ticks = tick_of(pp.x) - from.0;
                let lo = which.iter().min().and_then(|&m| m.checked_sub(1)).map_or(0, |m| orig[m].0 + 1);
                let hi = which.iter().max().and_then(|&m| orig.get(m + 1)).map_or(u32::MAX, |x| x.0.saturating_sub(1));
                for &w in &which {
                    let (ot, ov) = orig[w];
                    let nt = ((ot as f32 + dt_ticks).max(0.0) as u32).clamp(lo, hi.max(lo));
                    n.points[w] = (nt, (ov as f32 + dv).round().clamp(0.0, 127.0) as u8);
                }
                let lead = n.points[k];
                app.help_tag = Some(vec![(key_name(app, t, key), key_label(app, t, key, lead.1 as f32)), ("Position".into(), crate::app::bbdt(lead.0, sig))]);
                edit = Some(n);
            }
        }
        if resp.drag_stopped() {
            app.ta.auto.drag = None;
        }
    }
    if !on_point {
        if bg.hovered() {
            if let Some(h) = bg.hover_pos() {
                if seg_at(h).is_some() {
                    ui.ctx().set_cursor_icon(if alt || ctrl_shift { egui::CursorIcon::ResizeNeSw } else { egui::CursorIcon::ResizeVertical });
                }
            }
        }
        if bg.drag_started() {
            let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(row.center());
            let tick = tick_of(o.x).max(0.0) as u32;
            app.ta.auto.drag = if let Some(m) = marquee.as_ref().filter(|m| tick >= m.from && tick <= m.to && tool != crate::edit::Tool::Marquee) {
                app.checkpoint("Move Automation");
                Some(ADrag::Range { t, from: m.from, to: m.to, y0: o.y, orig: lane_now.clone() })
            } else if tool == crate::edit::Tool::Automation && !(alt || ctrl_shift) {
                Some(ADrag::Select { t, from: o })
            } else if let Some(k) = seg_at(o) {
                if alt || ctrl_shift {
                    app.checkpoint("Automation Curve");
                    Some(ADrag::Curve { t, k, y0: o.y, orig: lg_core::automation::curve_of(&lane_now, pts[k].0) })
                } else {
                    app.checkpoint("Move Automation Segment");
                    Some(ADrag::Segment { t, k, y0: o.y, orig: pts.clone() })
                }
            } else if tool == crate::edit::Tool::Automation {
                Some(ADrag::Select { t, from: o })
            } else {
                None
            };
        }
        if let (true, Some(pp)) = (bg.dragged(), bg.interact_pointer_pos()) {
            match app.ta.auto.drag.clone() {
                Some(ADrag::Segment { t: dt, k, y0, orig }) if dt == t => {
                    let mut n = lane_now.clone();
                    n.points = orig;
                    lg_core::automation::move_segment(&mut n, k, (y0 - pp.y) / inner.height() * 127.0);
                    app.help_tag = Some(vec![(key_name(app, t, key), key_label(app, t, key, n.points[k].1 as f32))]);
                    edit = Some(n);
                }
                Some(ADrag::Curve { t: dt, k, y0, orig }) if dt == t => {
                    let mut n = lane_now.clone();
                    // (down bends it to start slow when the line rises; Logic's feel)
                    let rising = pts.get(k + 1).is_some_and(|b| b.1 >= pts[k].1);
                    let d = (pp.y - y0) * if rising { 1.0 } else { -1.0 };
                    let c = (orig as f32 + d).round().clamp(-99.0, 99.0) as i8;
                    lg_core::automation::set_curve(&mut n, k, c);
                    app.help_tag = Some(vec![("Curve".into(), format!("{c}"))]);
                    edit = Some(n);
                }
                Some(ADrag::Range { t: dt, from, to, y0, orig }) if dt == t => {
                    let mut n = orig;
                    lg_core::automation::shift_range(&mut n, from, to, (y0 - pp.y) / inner.height() * 127.0, current);
                    edit = Some(n);
                }
                Some(ADrag::Select { t: dt, from }) if dt == t => {
                    let r = Rect::from_two_pos(from, pp);
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0x60, 0x90, 0xf0, 50));
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0x60, 0x90, 0xf0)), egui::StrokeKind::Inside);
                    let which: Vec<usize> = pts.iter().enumerate().filter(|(_, (tk, v))| r.contains(pos2(x_of(*tk as f32), y_of(*v as f32)))).map(|(k, _)| k).collect();
                    app.ta.auto.sel = (!which.is_empty()).then_some((t, which));
                }
                _ => {}
            }
        }
        if bg.drag_stopped() {
            app.ta.auto.drag = None;
            app.load_engine();
        }
        if bg.clicked() && !alt {
            if let Some(pp) = bg.interact_pointer_pos() {
                if tool == crate::edit::Tool::Automation {
                    app.ta.auto.sel = None;
                } else {
                    // a point on the line where clicked (Logic: the click's height is ignored),
                    // with one at the start the first time
                    app.checkpoint("Create Automation Point");
                    let tk = tick_of(pp.x).max(0.0) as u32;
                    let mut n = lane_now.clone();
                    if n.points.is_empty() {
                        n.points.push((0, current.round() as u8));
                    }
                    lg_core::automation::add_on_line(&mut n, tk, current);
                    app.ta.auto.sel = n.points.iter().position(|x| x.0 == tk).map(|k| (t, vec![k]));
                    edit = Some(n);
                }
            }
        }
    }
    if let Some(mut n) = edit {
        lg_core::automation::tidy(&mut n);
        if let Some(l) = lane_mut_key(app, t, key) {
            *l = n;
        }
        app.dirty = true;
        app.load_engine();
    }
}

/// The marquee's range deleted from the lane a track shows.
pub fn delete_marquee(app: &mut App, t: usize, from: u32, to: u32) {
    let key = shown(app, t);
    if let Some(l) = lane_mut_key(app, t, key) {
        lg_core::automation::delete_range(l, from, to);
    }
}

/// Delete with points selected: they go (true: done).
pub fn delete_selected(app: &mut App) -> bool {
    let Some((t, which)) = app.ta.auto.sel.take() else { return false };
    if which.is_empty() || !app.show_automation {
        return false;
    }
    app.checkpoint("Delete Automation Points");
    let key = shown(app, t);
    if let Some(l) = lane_mut_key(app, t, key) {
        let mut k = 0;
        l.points.retain(|_| {
            k += 1;
            !which.contains(&(k - 1))
        });
        lg_core::automation::tidy(l);
    }
    app.after_edit();
    true
}

/// The spans automation commands work on: the marquee's, else the selected regions'.
fn spans(app: &App) -> Vec<(usize, u32, u32)> {
    if let Some(m) = &app.ta.regions.marquee {
        return m.tracks.iter().map(|&t| (t, m.from, m.to)).collect();
    }
    let p = app.project.as_ref();
    app.region_selection().into_iter().filter_map(|(t, r)| p.and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| (t, x.start, x.start + x.length))).collect()
}

/// Automation commands (Mix ▸ Create / Delete Automation, region menu ▸ Automation, Views
/// Showing Automation). False: not one of these.
pub fn command(app: &mut App, name: &str) -> bool {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    match name {
        "Create 1 Track Automation Point at Region Borders" | "Create 2 Track Automation Points at Region Borders" | "Create 1 Automation Point at Region Borders" | "Create 2 Automation Points at Region Borders"
        | "Create 1 Track Automation Point for Visible Parameter" | "Create 2 Track Automation Points for Visible Parameter" | "Create 1 Automation Point for Visible Parameter" | "Create 2 Automation Points for Visible Parameter"
        | "Create 1 Track Automation Point each for Volume, Pan, Sends" | "Create 2 Track Automation Points each for Volume, Pan, Sends" | "Create 1 Automation Point each for Volume, Pan, Sends" | "Create 2 Automation Points each for Volume, Pan, Sends"
        | "Create 1 Track Automation Point at Every Region Border" | "Create 2 Track Automation Points at Every Region Border" => {
            let two = name.starts_with("Create 2");
            let mut sp = spans(app);
            if name.contains("Every Region Border") {
                let tracks = crate::trackarea::selection(app);
                sp = tracks.iter().flat_map(|&t| app.project.as_ref().map_or(Vec::new(), |p| p.tracks[t].regions.iter().map(|r| (t, r.start, r.start + r.length)).collect())).collect();
            }
            if sp.is_empty() {
                return false;
            }
            app.checkpoint("Create Automation Points");
            let each = name.contains("each for");
            for (t, a, b) in sp {
                let keys: Vec<Key> = if each { vec![Key { param: VOLUME, plugin: None }, Key { param: PAN, plugin: None }] } else { vec![shown(app, t)] };
                for k in keys {
                    let cur = current_value(app, t, k);
                    if let Some(l) = lane_mut_key(app, t, k) {
                        lg_core::automation::points_at_borders(l, a, b, two, cur);
                    }
                }
            }
            app.show_automation = true;
            app.after_edit();
        }
        "Nudge Automation Up 1 Step" | "Nudge Automation Down 1 Step" | "Nudge Automation Up 10 Steps" | "Nudge Automation Down 10 Steps" => return nudge(app, name),
        "Delete Orphaned Automation on Selected Tracks" | "Delete Orphaned Automation on Selected Track" => {
            let sel = crate::trackarea::selection(app);
            app.checkpoint("Delete Orphaned Automation");
            if let Some(p) = app.project.as_mut() {
                for t in sel {
                    let tr = &mut p.tracks[t];
                    let slots: Vec<u32> = tr.plugins.iter().map(|u| u.slot).collect();
                    tr.automation.retain(|l| l.plugin.is_none_or(|(s, _)| slots.contains(&s)) && !l.points.is_empty());
                }
            }
            app.after_edit();
        }
        "Delete All Track Automation" => {
            app.checkpoint("Delete All Track Automation");
            if let Some(p) = app.project.as_mut() {
                for t in &mut p.tracks {
                    t.automation.clear();
                }
            }
            app.after_edit();
        }
        "Delete Redundant Automation Points" => {
            app.checkpoint("Delete Redundant Automation Points");
            if let Some(p) = app.project.as_mut() {
                for tr in &mut p.tracks {
                    for l in &mut tr.automation {
                        lg_core::automation::delete_redundant(l);
                    }
                }
            }
            app.after_edit();
        }
        "Automation: Cycle Through Used Parameters" => {
            let Some(t) = app.selected_track.filter(|&t| t < n) else { return false };
            let used: Vec<Key> = app.project.as_ref().map_or(Vec::new(), |p| p.tracks[t].automation.iter().filter(|l| !l.points.is_empty()).map(|l| Key { param: l.param, plugin: l.plugin }).collect());
            if used.is_empty() {
                return true;
            }
            let cur = shown(app, t);
            let next = used[(used.iter().position(|k| *k == cur).map_or(0, |i| i + 1)) % used.len()];
            match next.plugin {
                Some(pl) => {
                    app.ta.shown_plugin.insert(t, pl);
                }
                None => {
                    app.ta.shown_plugin.remove(&t);
                    app.auto_param.insert(t, next.param);
                }
            }
            app.show_automation = true;
        }
        "Automation: Display Off" => app.show_automation = false,
        "Show Track Automation" => app.show_automation = true,
        "Set Automation Select Tool" | "Set Automation Curve Tool" => app.tool = crate::edit::Tool::Automation,
        "Set Fade Tool" => app.tool = crate::edit::Tool::Fade,
        "Set Marquee Tool" => app.tool = crate::edit::Tool::Marquee,
        _ => return false,
    }
    true
}

/// Nudge Automation Up / Down (the selected points, else the shown lane's points in the
/// marquee): one step is one of Logic's 0-127 values.
fn nudge(app: &mut App, name: &str) -> bool {
    let d: i16 = if name.contains("10 Steps") { 10 } else { 1 } * if name.contains("Up") { 1 } else { -1 };
    let target: Option<(usize, Vec<usize>)> = app.ta.auto.sel.clone().or_else(|| {
        let m = app.ta.regions.marquee.clone()?;
        let t = *m.tracks.first()?;
        let l = lane_of(app, t, shown(app, t))?;
        Some((t, lg_core::automation::points_in(l, m.from, m.to)))
    });
    let Some((t, which)) = target.filter(|x| !x.1.is_empty()) else { return false };
    app.checkpoint("Nudge Automation");
    let key = shown(app, t);
    if let Some(l) = lane_mut_key(app, t, key) {
        for &k in &which {
            if let Some(p) = l.points.get_mut(k) {
                p.1 = (p.1 as i16 + d).clamp(0, 127) as u8;
            }
        }
    }
    app.after_edit();
    true
}

/// The plug-in lanes the engine plays on a track: (the Audio FX insert's index, or None for
/// the instrument; the parameter; its range; the lane).
pub type ParamLane = (Option<usize>, u32, f32, f32, AutoLane);

pub fn params_for_engine(app: &App, t: usize) -> Vec<ParamLane> {
    if mode(app, t) == AutoMode::Off {
        return Vec::new();
    }
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return Vec::new() };
    // (the engine's inserts: the strip's slots from 3, the Channel EQ left out)
    let inserts: Vec<u32> = tr.plugins.iter().filter(|u| u.slot >= 3 && u.name != "Channel EQ" && !u.name.is_empty()).map(|u| u.slot).collect();
    tr.automation.iter().filter(|l| !l.points.is_empty()).filter_map(|l| {
        let (slot, id) = l.plugin?;
        let (_, _, lo, hi, _) = plugin_param(app, t, slot, id)?;
        let at = if slot == 2 { None } else { Some(inserts.iter().position(|&s| s == slot)?) };
        Some((at, id, lo, hi, l.clone()))
    }).collect()
}

/// Sets the plug-ins' automated parameters at `tick` (the engine, each block).
pub fn apply_params(lanes: &[ParamLane], u: &mut crate::audio::Units, tick: f64) {
    for (at, id, lo, hi, l) in lanes {
        let Some(v) = lg_core::automation::value_at(l, tick) else { continue };
        let x = lo + v / 127.0 * (hi - lo);
        match at {
            None => {
                if let Some(ins) = u.instrument.as_mut() {
                    ins.set(*id, x);
                }
            }
            Some(k) => {
                if let Some(fx) = u.inserts.get_mut(*k) {
                    fx.set(*id, x);
                }
            }
        }
    }
}

/// A fader moved while playing: in Touch, Latch or Write, a point at the playhead.
pub fn record(app: &mut App, t: usize, param: u8, value: f32) {
    if !app.playing() || !matches!(mode(app, t), AutoMode::Touch | AutoMode::Latch | AutoMode::Write) {
        return;
    }
    let tick = app.position() as u32;
    let write = mode(app, t) == AutoMode::Write;
    if let Some(l) = lane_mut(app, t, param) {
        // (Write replaces what was there since the last point it wrote)
        let since = l.points.iter().map(|x| x.0).filter(|&x| x <= tick).max().unwrap_or(0);
        if write {
            l.points.retain(|x| x.0 <= since || x.0 > tick);
        }
        l.points.retain(|x| x.0 != tick);
        l.points.push((tick, value.round().clamp(0.0, 127.0) as u8));
        l.points.sort_by_key(|x| x.0);
    }
}

/// A lane's points for the engine: curved segments sampled into short straight pieces.
fn sampled(l: &AutoLane) -> Vec<(u32, f32)> {
    let mut out = Vec::new();
    for (k, &(tk, v)) in l.points.iter().enumerate() {
        out.push((tk, v as f32));
        if let Some(&(b, _)) = l.points.get(k + 1) {
            if lg_core::automation::curve_of(l, tk) != 0 && b > tk + 16 {
                for s in 1..16 {
                    let at = tk + (b - tk) * s / 16;
                    out.push((at, lg_core::automation::value_at(l, at as f64).unwrap_or(v as f32)));
                }
            }
        }
    }
    out
}

/// What the engine plays: a track's volume and pan lines in gain and −1..1 (empty when the
/// track's mode is Off or it has none).
pub fn for_engine(app: &App, t: usize) -> (Vec<(u32, f32)>, Vec<(u32, f32)>) {
    if mode(app, t) == AutoMode::Off {
        return (Vec::new(), Vec::new());
    }
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return (Vec::new(), Vec::new()) };
    let find = |param: u8| tr.automation.iter().find(|l| l.plugin.is_none() && l.param == param).map(sampled).unwrap_or_default();
    let vol = find(VOLUME).into_iter().map(|(tk, v)| (tk, crate::app::db_to_gain(logic_to_db(v)))).collect();
    let pan = find(PAN).into_iter().map(|(tk, v)| (tk, ((v - 64.0) / 63.0).clamp(-1.0, 1.0))).collect();
    (vol, pan)
}

/// A line's value at `tick` (engine side, already in gain or pan).
pub fn curve_at(points: &[(u32, f32)], tick: f64) -> Option<f32> {
    let first = points.first()?;
    if tick <= first.0 as f64 {
        return Some(first.1);
    }
    for w in points.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        if tick >= a as f64 && tick <= b as f64 {
            let t = if b > a { ((tick - a as f64) / (b - a) as f64) as f32 } else { 0.0 };
            return Some(va + (vb - va) * t);
        }
    }
    points.last().map(|p| p.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_scale_and_lines() {
        assert!(logic_to_db(90.0).abs() < 0.01);
        assert!((logic_to_db(127.0) - 6.0).abs() < 0.01);
        assert!((db_to_logic(logic_to_db(60.0)) - 60.0).abs() < 0.01);
        let pts = [(0u32, 90u8), (960, 0)];
        assert_eq!(value_at(&pts, 480), Some(45.0));
        assert_eq!(value_at(&pts, 2000), Some(0.0));
    }
}
