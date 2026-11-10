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

fn lane_mut(app: &mut App, t: usize, param: u8) -> Option<&mut AutoLane> {
    let tr = app.project.as_mut()?.tracks.get_mut(t)?;
    if !tr.automation.iter().any(|l| l.param == param) {
        tr.automation.push(AutoLane { param, points: Vec::new() });
    }
    tr.automation.iter_mut().find(|l| l.param == param)
}

/// The header's automation controls (under the track's name while automation shows): the mode
/// pop-up (its colour as Logic's) and the parameter pop-up.
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
    let param = shown_param(app, t);
    let presp = ui.interact(pr, ui.scope_id().with(("autoparam", t)), Sense::click());
    p.rect_filled(pr, 4.0, look::group_bg());
    p.text(pos2(pr.min.x + 6.0, pr.center().y), Align2::LEFT_CENTER, format!("{} ⌄", param_name(param)), look::regular(10.5), look::text());
    egui::Popup::menu(&presp).show(|ui| {
        for x in [VOLUME, PAN] {
            if ui.add(egui::Button::selectable(x == param, param_name(x))).clicked() {
                app.auto_param.insert(t, x);
                ui.close();
            }
        }
    });
}

/// The lane over a track's row: the line, its points, and editing.
pub fn lane(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let param = shown_param(app, t);
    let pts: Vec<(u32, u8)> = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.automation.iter().find(|l| l.param == param)).map(|l| l.points.clone()).unwrap_or_default();
    let p = ui.painter_at(row);
    p.rect_filled(row, 0.0, Color32::from_black_alpha(70));
    let inner = row.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.max.y - v / 127.0 * inner.height();
    let v_of = |y: f32| ((inner.max.y - y) / inner.height() * 127.0).clamp(0.0, 127.0);
    let col = Color32::from_rgb(0xa8, 0xd8, 0x6a);
    // the current value as a line when there are no points
    let current = if param == VOLUME {
        db_to_logic(app.project.as_ref().map_or(0.0, |p| p.tracks[t].volume_db))
    } else {
        64.0 + app.project.as_ref().map_or(0.0, |p| p.tracks[t].pan) * 63.0
    };
    if pts.is_empty() {
        p.hline(row.x_range(), y_of(current), Stroke::new(1.5, col.gamma_multiply(0.6)));
    } else {
        let mut line = vec![pos2(row.min.x, y_of(pts[0].1 as f32))];
        line.extend(pts.iter().map(|(tk, v)| pos2(x_of(*tk as f32), y_of(*v as f32))));
        line.push(pos2(row.max.x, y_of(pts.last().unwrap().1 as f32)));
        p.add(egui::Shape::line(line, Stroke::new(1.8, col)));
    }
    // the points: drag, Option-click removes
    let mut edit: Option<Vec<(u32, u8)>> = None;
    let mut on_point = false;
    for (k, (tk, v)) in pts.iter().enumerate() {
        let c = pos2(x_of(*tk as f32), y_of(*v as f32));
        if !row.contains(c) {
            continue;
        }
        let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("autopt", t, k)), Sense::click_and_drag());
        on_point |= resp.hovered() || resp.dragged();
        p.circle_filled(c, if resp.hovered() || resp.dragged() { 5.0 } else { 3.5 }, col);
        if resp.clicked() && ui.input(|i| i.modifiers.alt) {
            let mut n = pts.clone();
            n.remove(k);
            edit = Some(n);
        } else if resp.drag_started() {
            app.checkpoint("Move Automation Point");
        } else if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let mut n = pts.clone();
            let fine = ui.input(|i| i.modifiers.shift);
            let nv = if fine { (*v as f32 - resp.drag_delta().y * 0.2).clamp(0.0, 127.0) } else { v_of(pp.y) };
            let lo = if k > 0 { pts[k - 1].0 } else { 0 };
            let hi = pts.get(k + 1).map_or(u32::MAX, |x| x.0);
            let nt = (tick_of(pp.x).max(0.0) as u32).clamp(lo, hi);
            n[k] = (nt, nv.round() as u8);
            let label = if param == VOLUME { format!("{:.1} dB", logic_to_db(nv)) } else { format!("{:+.0}", nv - 64.0) };
            app.help_tag = Some(vec![(param_name(param).into(), label), ("Position".into(), crate::app::bbdt(nt, app.project.as_ref().map_or((4, 4), |p| p.signature)))]);
            edit = Some(n);
        }
    }
    // a click on the lane: a point there, on the line
    let bg = ui.interact(row, ui.scope_id().with(("autolane", t)), Sense::click());
    if !on_point && bg.clicked() {
        if let Some(pp) = bg.interact_pointer_pos() {
            app.checkpoint("Create Automation Point");
            let tk = tick_of(pp.x).max(0.0) as u32;
            let v = if pts.is_empty() { current } else { v_of(pp.y) };
            let mut n = pts.clone();
            if n.is_empty() {
                // (the first click keeps the current value from the start, as Logic does)
                n.push((0, current.round() as u8));
            }
            n.push((tk, v.round() as u8));
            n.sort_by_key(|x| x.0);
            edit = Some(n);
        }
    }
    if let Some(n) = edit {
        if let Some(l) = lane_mut(app, t, param) {
            l.points = n;
        }
        app.dirty = true;
        app.load_engine();
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

/// What the engine plays: a track's volume and pan lines in gain and −1..1 (empty when the
/// track's mode is Off or it has none).
pub fn for_engine(app: &App, t: usize) -> (Vec<(u32, f32)>, Vec<(u32, f32)>) {
    if mode(app, t) == AutoMode::Off {
        return (Vec::new(), Vec::new());
    }
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return (Vec::new(), Vec::new()) };
    let vol = tr.automation.iter().find(|l| l.param == VOLUME).map(|l| l.points.iter().map(|(tk, v)| (*tk, crate::app::db_to_gain(logic_to_db(*v as f32)))).collect()).unwrap_or_default();
    let pan = tr.automation.iter().find(|l| l.param == PAN).map(|l| l.points.iter().map(|(tk, v)| (*tk, ((*v as f32 - 64.0) / 63.0).clamp(-1.0, 1.0))).collect()).unwrap_or_default();
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
