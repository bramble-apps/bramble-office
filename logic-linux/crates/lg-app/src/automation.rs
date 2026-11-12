//! Track automation, as Logic's (A shows it): each track's lanes (Volume, Pan… by Logic's
//! parameter numbers: 7 volume, 10 pan), drawn over the track's row as a line with points; the
//! header's parameter pop-up and automation mode (Off, Read, Touch, Latch, Write). A click on the
//! line adds a point, a drag moves one (Shift: fine), Option-click removes one. Playing in Read
//! (and Touch/Latch/Write) the channel follows the line; moving a fader while playing in Touch,
//! Latch or Write records points there.
//!
//! Values are kept as Logic stores them, 0–127: volume 90 = 0 dB (127 = +6 dB), pan 64 = centre.
//!
//! The rest of Logic's automation (Trim / Relative, region automation, subtracks, snap, the
//! Automation Event List, Quick Access, Move Track Automation with Regions) is in autoext.rs;
//! this file draws and edits one lane (`lane_key`) for the main row and the subtracks.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::project::AutoLane;

use crate::app::App;
use crate::look;

pub const VOLUME: u8 = 7;
pub const PAN: u8 = 10;
/// Logic's channel parameter numbers for Mute and Solo (the group sequences' 0x50 records).
pub const MUTE: u8 = 9;
pub const SOLO: u8 = 3;
/// Lanes that aren't a plug-in's: a Smart Controls screen control (its index), a MIDI
/// controller sent to the instrument (its number; 128 pitch bend, 129 aftertouch).
pub const SC_SLOT: u32 = 0xFFFF_0000;
pub const MIDI_SLOT: u32 = 0xFFFF_0001;

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
            // (Logic's mustard, orange and purple)
            AutoMode::Touch => Color32::from_rgb(0xc8, 0xa8, 0x2c),
            AutoMode::Latch => Color32::from_rgb(0xe8, 0x8a, 0x2c),
            AutoMode::Write => Color32::from_rgb(0xa0, 0x5c, 0xe0),
        }
    }
}

/// The mode pop-up as Logic shows it: its label (T-Touch, R-Latch… with Trim or Relative on),
/// the background and the text colour (Off gray; Read green text on gray without automation,
/// white on green with it; Touch mustard, Latch orange, Write purple).
pub fn mode_look(app: &App, t: usize) -> (String, Color32, Color32) {
    let m = mode(app, t);
    let prefix = match m {
        AutoMode::Touch | AutoMode::Latch if app.ax.trim.contains(&t) => "T-",
        AutoMode::Touch | AutoMode::Latch | AutoMode::Write if app.ax.relative.contains(&t) => "R-",
        _ => "",
    };
    let has_data = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.automation.iter().any(|l| !l.points.is_empty()));
    let gray = Color32::from_gray(0x4a);
    let (bg, fg) = match m {
        AutoMode::Off => (gray, look::text_dim()),
        AutoMode::Read if has_data => (m.color(), Color32::WHITE),
        AutoMode::Read => (gray, m.color()),
        _ => (m.color(), Color32::BLACK),
    };
    (format!("{prefix}{}", m.name()), bg, fg)
}

pub fn param_name(p: u8) -> &'static str {
    match p {
        VOLUME => "Volume",
        PAN => "Pan",
        MUTE => "Mute",
        SOLO => "Solo",
        p => crate::surround::param_name(p).unwrap_or("Parameter"),
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
    /// The parameter's Relative curve (Trim / Relative modes).
    pub rel: bool,
}

impl Key {
    pub fn channel(param: u8) -> Key {
        Key { param, plugin: None, rel: false }
    }
    pub fn plugin(slot: u32, id: u32) -> Key {
        Key { param: 0, plugin: Some((slot, id)), rel: false }
    }
    pub fn of(l: &AutoLane) -> Key {
        Key { param: l.param, plugin: l.plugin, rel: l.relative }
    }
    /// The absolute lane of the same parameter.
    pub fn abs(self) -> Key {
        Key { rel: false, ..self }
    }
}

pub fn shown(app: &App, t: usize) -> Key {
    let rel = app.ax.rel_edit.contains(&t);
    match app.ta.shown_plugin.get(&t) {
        Some(&pl) => Key { param: 0, plugin: Some(pl), rel },
        None => Key { param: shown_param(app, t), plugin: None, rel },
    }
}

pub fn matches(l: &AutoLane, k: Key) -> bool {
    l.relative == k.rel
        && match k.plugin {
            Some(pl) => l.plugin == Some(pl),
            None => l.plugin.is_none() && l.param == k.param,
        }
}

/// Shows `k` on the track's main lane.
pub fn show_key(app: &mut App, t: usize, k: Key) {
    match k.plugin {
        Some(pl) => {
            app.ta.shown_plugin.insert(t, pl);
        }
        None => {
            app.ta.shown_plugin.remove(&t);
            app.auto_param.insert(t, k.param);
        }
    }
    if k.rel {
        app.ax.rel_edit.insert(t);
    } else {
        app.ax.rel_edit.remove(&t);
    }
}

pub fn lane_of(app: &App, t: usize, k: Key) -> Option<&AutoLane> {
    app.project.as_ref()?.tracks.get(t)?.automation.iter().find(|l| matches(l, k))
}

pub fn lane_mut_key(app: &mut App, t: usize, k: Key) -> Option<&mut AutoLane> {
    let tr = app.project.as_mut()?.tracks.get_mut(t)?;
    if !tr.automation.iter().any(|l| matches(l, k)) {
        tr.automation.push(AutoLane { param: k.param, plugin: k.plugin, relative: k.rel, ..Default::default() });
    }
    tr.automation.iter_mut().find(|l| matches(l, k))
}


/// A plug-in parameter's name, range and value now: the track's instrument (slot 2) or an
/// Audio FX slot, as the engine builds it.
pub fn plugin_param(app: &App, t: usize, slot: u32, id: u32) -> Option<(String, String, f32, f32, f32)> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let u = tr.plugins.iter().find(|u| u.slot == slot)?;
    // a hosted plug-in: its list (made once), the value from the slot's floats (hosting.rs)
    if let Some(params) = crate::hosting::params_of(&u.name) {
        let p = params.iter().find(|p| p.id == id)?;
        return Some((u.name.clone(), p.name.to_string(), p.min, p.max, u.params.get(id as usize).copied().unwrap_or(p.default)));
    }
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
pub fn current_value(app: &App, t: usize, k: Key) -> f32 {
    if k.rel {
        return 64.0;
    }
    if let Some((slot, _)) = k.plugin.filter(|p| p.0 == SC_SLOT || p.0 == MIDI_SLOT) {
        let _ = slot;
        return 64.0;
    }
    if let Some((slot, id)) = k.plugin {
        return plugin_param(app, t, slot, id).map_or(64.0, |(_, _, lo, hi, v)| ((v - lo) / (hi - lo).max(1e-9) * 127.0).clamp(0.0, 127.0));
    }
    let tr = app.project.as_ref().and_then(|p| p.tracks.get(t));
    match k.param {
        VOLUME => db_to_logic(tr.map_or(0.0, |t| t.volume_db)),
        PAN => 64.0 + tr.map_or(0.0, |t| t.pan) * 63.0,
        MUTE => if tr.is_some_and(|t| t.mute) { 127.0 } else { 0.0 },
        SOLO => if tr.is_some_and(|t| t.solo) { 127.0 } else { 0.0 },
        _ => 64.0,
    }
}

pub fn key_label(app: &App, t: usize, k: Key, v: f32) -> String {
    if k.rel {
        return format!("{:+.0}", v - 64.0);
    }
    match (k.plugin, k.param) {
        (Some((SC_SLOT | MIDI_SLOT, _)), _) => format!("{v:.0}"),
        (None, MUTE | SOLO) => if v >= 64.0 { "On".into() } else { "Off".into() },
        (Some((slot, id)), _) => plugin_param(app, t, slot, id).map_or(format!("{v:.0}"), |(_, _, lo, hi, _)| format!("{:.2}", lo + v / 127.0 * (hi - lo))),
        (None, VOLUME) => format!("{:.1} dB", logic_to_db(v)),
        (None, PAN) => format!("{:+.0}", v - 64.0),
        _ => format!("{v:.0}"),
    }
}

pub fn key_name(app: &App, t: usize, k: Key) -> String {
    let base = match k.plugin {
        Some((SC_SLOT, i)) => app.sc.screens.get(&crate::smartcontrols::View::Track(t)).and_then(|s| s.controls.get(i as usize)).map_or(format!("Screen Control {}", i + 1), |c| format!("Smart Controls: {}", c.label)),
        Some((MIDI_SLOT, cc)) => format!("MIDI: {}", crate::autoext::cc_name(cc)),
        Some((slot, id)) => plugin_param(app, t, slot, id).map_or("Parameter".into(), |(plug, name, ..)| format!("{plug}: {name}")),
        None => param_name(k.param).to_string(),
    };
    if k.rel { format!("{base} (Relative)") } else { base }
}

/// Automation editing state: the selected points (on one track's shown lane) and the drag.
#[derive(Default)]
pub struct State {
    pub sel: Option<(usize, Vec<usize>)>,
    /// The lane the selected points are on (None: the track's main lane).
    pub sel_key: Option<Key>,
    drag: Option<ADrag>,
}

/// The selection's lane on track `t`.
pub fn sel_key(app: &App, t: usize) -> Key {
    app.ta.auto.sel_key.unwrap_or_else(|| shown(app, t))
}

fn set_sel(app: &mut App, t: usize, k: Key, which: Vec<usize>) {
    app.ta.auto.sel = (!which.is_empty()).then_some((t, which));
    app.ta.auto.sel_key = Some(k);
}

#[derive(Clone, Debug)]
enum ADrag {
    /// Points moving (the press's tick and value, the points as they were).
    /// (`copy`: Option-drag of selected points; `axis`: 0 free, 1 time only once a horizontal
    /// drag started)
    Points { t: usize, key: Key, from: (f32, f32), orig: Vec<(u32, u8)>, which: Vec<usize>, copy: bool, axis: u8 },
    /// The Pencil drawn across the lane: the points made so far, the drag's first tick.
    Draw { t: usize, key: Key, orig: AutoLane, from: u32 },
    /// Option-Shift-drag between two selected adjacent points: both move in opposite
    /// directions in time (the ramp gets longer or shorter).
    Ramp { t: usize, k: usize, x0: f32, orig: Vec<(u32, u8)> },
    Segment { t: usize, k: usize, y0: f32, orig: Vec<(u32, u8)> },
    Curve { t: usize, k: usize, y0: f32, orig: i8 },
    Select { t: usize, from: egui::Pos2 },
    Range { t: usize, from: u32, to: u32, y0: f32, orig: AutoLane },
}

/// The header's automation controls (under the track's name while automation shows): the
/// Automation button (track / region automation), the mode pop-up (its colour as Logic's; Trim,
/// Relative and the Record Automation with Regions choices at its end) and the parameter pop-up
/// (the shown parameter on top, then Main, Smart Controls, each plug-in by slot, MIDI). Hovering
/// the parameter pop-up shows its On/Off button: off, the parameter's automation stays but
/// isn't played (the curve turns gray).
pub fn header(app: &mut App, ui: &mut egui::Ui, r: Rect, t: usize) {
    let p = ui.painter().clone();
    // the Automation button: track or region automation
    let region = app.ax.region_view.contains(&t);
    let br = Rect::from_min_size(r.min, vec2(18.0, r.height()));
    let bresp = ui.interact(br, ui.scope_id().with(("autobtn", t)), Sense::click()).on_hover_text(if region { "Region automation (click: track automation)" } else { "Track automation (click: region automation)" });
    p.rect_filled(br, 4.0, if region { Color32::from_rgb(0x3c, 0x7c, 0xd8) } else { look::group_bg() });
    crate::glyphs::paint(&p, br, crate::glyphs::ph::LINE_SEGMENTS, 12.0, if region { Color32::WHITE } else { look::text_dim() });
    if bresp.clicked() {
        crate::autoext::toggle_region_view(app, t);
    }
    let m = mode(app, t);
    let (label, bg, fg) = mode_look(app, t);
    let mr = Rect::from_min_size(pos2(br.max.x + 3.0, r.min.y), vec2(52.0, r.height()));
    let resp = ui.interact(mr, ui.scope_id().with(("automode", t)), Sense::click());
    p.rect_filled(mr, 4.0, bg);
    p.text(mr.center(), Align2::CENTER_CENTER, label, look::semibold(10.5), fg);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        for x in AutoMode::ALL {
            if ui.add(egui::Button::selectable(x == m, x.name())).clicked() {
                crate::autoext::set_mode(app, t, x);
                ui.close();
            }
        }
        ui.separator();
        crate::autoext::mode_menu_extras(app, ui, t);
    });
    // (the value field at the right: the value at the playhead; hovered: Trim)
    let has_points = lane_of(app, t, shown(app, t)).is_some_and(|l| !l.points.is_empty());
    let fw = if has_points && r.width() > 150.0 { 40.0 } else { 0.0 };
    if fw > 0.0 {
        crate::autoext::trim_field(app, ui, Rect::from_min_max(pos2(r.max.x - fw, r.min.y), r.max), t);
    }
    let pr = Rect::from_min_max(pos2(mr.max.x + 4.0, r.min.y), pos2(r.max.x - fw - if fw > 0.0 { 3.0 } else { 0.0 }, r.max.y));
    let key = shown(app, t);
    let presp = ui.interact(pr, ui.scope_id().with(("autoparam", t)), Sense::click());
    p.rect_filled(pr, 4.0, look::group_bg());
    let name = key_name(app, t, key);
    let muted = lane_of(app, t, key).is_some_and(|l| l.muted);
    let hover = ui.rect_contains_pointer(pr);
    let onr = Rect::from_min_size(pos2(pr.max.x - 18.0, pr.min.y + 1.0), vec2(16.0, pr.height() - 2.0));
    let text_right = if hover || muted { onr.min.x - 2.0 } else { pr.max.x };
    p.with_clip_rect(Rect::from_min_max(pr.min, pos2(text_right, pr.max.y))).text(pos2(pr.min.x + 6.0, pr.center().y), Align2::LEFT_CENTER, format!("{name} ⌄"), look::regular(10.5), if muted { look::text_dim() } else { look::text() });
    if hover || muted {
        // the On/Off button (automation of this parameter muted or not)
        let oresp = ui.interact(onr, ui.scope_id().with(("autoon", t)), Sense::click()).on_hover_text(if muted { "Automation off for this parameter (click: on)" } else { "Automation on (click: off)" });
        p.rect_filled(onr, 3.0, if muted { Color32::from_gray(0x55) } else { Color32::from_rgb(0x3c, 0x7c, 0xd8) });
        crate::glyphs::paint(&p, onr, crate::glyphs::ph::POWER, 10.0, Color32::WHITE);
        if oresp.clicked() {
            app.checkpoint("Automation On/Off");
            if let Some(l) = lane_mut_key(app, t, key) {
                l.muted = !l.muted;
            }
            app.after_edit();
            return;
        }
    }
    egui::Popup::menu(&presp).show(|ui| param_menu(app, ui, t, key, &mut |app, k| show_key(app, t, k)));
}

/// The Automation Parameter menu (the header's pop-up, the lane's shortcut menu, a
/// subtrack's): the shown parameter at the top, Main (Volume, Pan, Solo, Mute; Absolute /
/// Relative), Smart Controls, the plug-ins by slot, MIDI on instrument tracks.
pub fn param_menu(app: &mut App, ui: &mut egui::Ui, t: usize, key: Key, pick: &mut dyn FnMut(&mut App, Key)) {
    ui.set_min_width(220.0);
    let used = |app: &App, k: Key| lane_of(app, t, k).is_some_and(|l| !l.points.is_empty());
    let mark = |app: &App, k: Key, name: String| if used(app, k) { format!("{name} •") } else { name };
    ui.add_enabled(false, egui::Button::new(key_name(app, t, key)));
    ui.separator();
    let mut chosen: Option<Key> = None;
    ui.menu_button("Main", |ui| {
        for x in [VOLUME, PAN, SOLO, MUTE].into_iter().chain(crate::surround::auto_params(app, t)) {
            let k = Key::channel(x);
            if matches!(x, VOLUME | PAN) {
                ui.menu_button(param_name(x), |ui| {
                    for (rel, n) in [(false, "Absolute"), (true, "Relative (±)")] {
                        let kk = Key { rel, ..k };
                        if ui.add(egui::Button::selectable(key == kk, mark(app, kk, n.to_string()))).clicked() {
                            chosen = Some(kk);
                            ui.close();
                        }
                    }
                });
            } else if ui.add(egui::Button::selectable(key == k, mark(app, k, param_name(x).to_string()))).clicked() {
                chosen = Some(k);
                ui.close();
            }
        }
    });
    // Smart Controls: the screen controls (their movements, not the parameters underneath)
    let controls: Vec<String> = crate::smartcontrols::screen(app, crate::smartcontrols::View::Track(t)).controls.iter().map(|c| c.label.clone()).collect();
    ui.menu_button("Smart Controls", |ui| {
        if controls.is_empty() {
            ui.add_enabled(false, egui::Button::new("No screen controls"));
        }
        for (i, c) in controls.iter().enumerate() {
            let k = Key::plugin(SC_SLOT, i as u32);
            if ui.add(egui::Button::selectable(key == k, mark(app, k, c.clone()))).clicked() {
                chosen = Some(k);
                ui.close();
            }
        }
    });
    // the channel strip's plug-ins, numbered and named by slot (1 Sampler…)
    let plugins: Vec<(u32, String)> = app.project.as_ref().map_or(Vec::new(), |p| p.tracks[t].plugins.iter().filter(|u| u.slot >= 2 && !u.name.is_empty()).map(|u| (u.slot, u.name.clone())).collect());
    let rate = crate::audio::RATE as f32;
    for (slot, plug) in plugins {
        let params: Vec<lg_dsp::Param> = if let Some(p) = crate::hosting::params_of(&plug) {
            Some(p)
        } else if slot == 2 { crate::units::instrument(&plug, rate).map(|i| i.params().to_vec()) } else { crate::units::effect(&plug, rate).map(|f| f.params().to_vec()) }.unwrap_or_default();
        let title = format!("{} {plug}", slot.saturating_sub(1));
        if params.is_empty() {
            ui.add_enabled(false, egui::Button::new(title));
            continue;
        }
        ui.menu_button(title, |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for prm in params {
                    let k = Key::plugin(slot, prm.id);
                    if ui.add(egui::Button::selectable(key == k, mark(app, k, prm.name.to_string()))).clicked() {
                        chosen = Some(k);
                        ui.close();
                    }
                }
            });
        });
    }
    // MIDI (software instrument tracks): the controllers sent to the instrument
    if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == lg_core::TrackKind::Instrument) {
        ui.separator();
        ui.menu_button("MIDI", |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for cc in crate::autoext::MIDI_PARAMS {
                    let k = Key::plugin(MIDI_SLOT, *cc as u32);
                    if ui.add(egui::Button::selectable(key == k, mark(app, k, crate::autoext::cc_name(*cc as u32)))).clicked() {
                        chosen = Some(k);
                        ui.close();
                    }
                }
            });
        });
    }
    if let Some(k) = chosen {
        pick(app, k);
    }
}

/// The line through a lane's points, curves sampled, steps held, from `x0` to `x1`.
pub fn polyline(l: &AutoLane, x0: f32, x1: f32, x_of: &dyn Fn(f32) -> f32, y_of: &dyn Fn(f32) -> f32) -> Vec<egui::Pos2> {
    let pts = &l.points;
    let mut line = vec![pos2(x0, y_of(pts[0].1 as f32))];
    for w in pts.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        line.push(pos2(x_of(a as f32), y_of(va as f32)));
        if l.steps.contains(&a) {
            line.push(pos2(x_of(b as f32), y_of(va as f32)));
        } else if lg_core::automation::curve_of(l, a) != 0 && b > a {
            for s in 1..16 {
                let tick = a as f64 + (b - a) as f64 * s as f64 / 16.0;
                line.push(pos2(x_of(tick as f32), y_of(lg_core::automation::value_at(l, tick).unwrap_or(vb as f32))));
            }
        }
    }
    let last = *pts.last().unwrap();
    line.push(pos2(x_of(last.0 as f32), y_of(last.1 as f32)));
    line.push(pos2(x1, y_of(last.1 as f32)));
    line
}

/// The track's main automation lane over its row (or its region automation, with the
/// Automation button on).
pub fn lane(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    if app.ax.region_view.contains(&t) {
        crate::autoext::region_lane(app, ui, row, t, x_of, tick_of);
        return;
    }
    let key = shown(app, t);
    lane_key(app, ui, row, t, key, x_of, tick_of);
}

/// One lane (`key`) of a track, and editing as Logic's: click the lane: a point on the line
/// (double-click too); drag a point (Shift or Control: fine; selected points move together;
/// Option: a copy; a drag that starts sideways moves in time only); Shift-click points: add
/// to the selection; Option-click a point or the line: it and all that follow; double-click a
/// point: remove it; drag the line between two points: the segment up or down; Option- or
/// Control-Shift-drag the line: bend it; Option-Shift-drag between two selected points: the
/// ramp longer or shorter; Option-click the line with the Automation Curve tool (or
/// Control-Option-Shift-click with the Pointer): stepped / continuous. The Pencil draws
/// points as it drags, the Eraser removes the points it crosses. The Automation Select tool
/// drags a rectangle around points; inside a marquee selection a drag moves that range. Snap
/// Automation puts the points on its grid.
#[allow(clippy::too_many_arguments)]
pub fn lane_key(app: &mut App, ui: &mut egui::Ui, row: Rect, t: usize, key: Key, x_of: &dyn Fn(f32) -> f32, tick_of: &dyn Fn(f32) -> f32) {
    let lane_now = lane_of(app, t, key).cloned().unwrap_or(AutoLane { param: key.param, plugin: key.plugin, relative: key.rel, ..Default::default() });
    let pts = lane_now.points.clone();
    let p = ui.painter_at(row);
    p.rect_filled(row, 0.0, Color32::from_black_alpha(70));
    let inner = row.shrink2(vec2(0.0, 6.0));
    let y_of = |v: f32| inner.max.y - v / 127.0 * inner.height();
    let v_of = |y: f32| ((inner.max.y - y) / inner.height() * 127.0).clamp(0.0, 127.0);
    let px_per_tick = ((x_of(960.0) - x_of(0.0)) / 960.0).max(1e-6);
    let snap = |app: &App, tick: f32| crate::autoext::snap_tick(app, tick.max(0.0), px_per_tick) as f32;
    let col = if lane_now.muted { Color32::from_gray(0x80) } else if key.rel { Color32::from_rgb(0x6a, 0xc8, 0xe8) } else { Color32::from_rgb(0xa8, 0xd8, 0x6a) };
    let sel_col = Color32::WHITE;
    let current = current_value(app, t, key);
    let selected: Vec<usize> = app.ta.auto.sel.as_ref().filter(|s| s.0 == t && sel_key(app, t) == key).map(|s| s.1.clone()).unwrap_or_default();
    // the relative curve shown: the absolute one behind it
    if key.rel {
        if let Some(abs) = lane_of(app, t, key.abs()).filter(|l| !l.points.is_empty()) {
            p.add(egui::Shape::line(polyline(abs, row.min.x, row.max.x, x_of, &y_of), Stroke::new(1.0, Color32::from_rgb(0xa8, 0xd8, 0x6a).gamma_multiply(0.4))));
        }
    }
    // the selected range: a gray area between the first and last selected point
    if selected.len() > 1 {
        let (a, b) = (selected.iter().filter_map(|&k| pts.get(k)).map(|p| p.0).min().unwrap_or(0), selected.iter().filter_map(|&k| pts.get(k)).map(|p| p.0).max().unwrap_or(0));
        p.rect_filled(Rect::from_x_y_ranges(x_of(a as f32)..=x_of(b as f32), row.y_range()), 0.0, Color32::from_white_alpha(18));
    }
    if pts.is_empty() {
        p.hline(row.x_range(), y_of(current), Stroke::new(1.5, col.gamma_multiply(0.6)));
    } else {
        let line = polyline(&lane_now, row.min.x, row.max.x, x_of, &y_of);
        // (a stepped curve is drawn a little thinner)
        let w = if lane_now.steps.is_empty() { 1.8 } else { 1.4 };
        p.add(egui::Shape::line(line, Stroke::new(w, col)));
        // the selected points' segments white
        for w2 in selected.windows(2) {
            if w2[1] == w2[0] + 1 {
                if let (Some(a), Some(b)) = (pts.get(w2[0]), pts.get(w2[1])) {
                    let mut seg = AutoLane { points: vec![*a, *b], ..lane_now.clone() };
                    seg.curves.retain(|c| c.0 == a.0);
                    seg.steps.retain(|s| *s == a.0);
                    let l = polyline(&seg, x_of(a.0 as f32), x_of(b.0 as f32), x_of, &y_of);
                    p.add(egui::Shape::line(l, Stroke::new(2.0, sel_col)));
                }
            }
        }
    }
    let tool = if ui.input(|i| i.modifiers.command) { app.cmd_tool } else { app.tool };
    let (alt, ctrl_shift, shift, ctrl) = ui.input(|i| (i.modifiers.alt, i.modifiers.ctrl && i.modifiers.shift, i.modifiers.shift, i.modifiers.ctrl));
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let mut edit: Option<AutoLane> = None;
    let mut on_point = false;
    // the background: click (a point on the line), the select rectangle, segment and range drags
    let bg = ui.interact(row, ui.scope_id().with(("autolane", t, key.param, key.plugin, key.rel)), Sense::click_and_drag());
    crate::autoext::lane_menu(app, &bg, t, key);
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
        let resp = ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), ui.scope_id().with(("autopt", t, key.param, key.plugin, key.rel, k)), Sense::click_and_drag());
        on_point |= resp.hovered() || resp.dragged();
        let is_sel = selected.contains(&k);
        p.circle_filled(c, if resp.hovered() || resp.dragged() { 5.0 } else { 3.5 }, if is_sel { sel_col } else { col });
        // (the value next to each point)
        if is_sel || resp.hovered() || pts.len() <= 24 {
            p.text(c + vec2(6.0, -9.0), Align2::LEFT_CENTER, key_label(app, t, key, *v as f32), look::regular(9.5), if is_sel { sel_col } else { col.gamma_multiply(0.8) });
        }
        if resp.double_clicked() || (resp.clicked() && tool == crate::edit::Tool::Eraser) {
            let mut n = lane_now.clone();
            n.points.remove(k);
            app.checkpoint("Delete Automation Point");
            app.ta.auto.sel = None;
            edit = Some(n);
        } else if resp.clicked() && alt {
            // it and all that follow
            set_sel(app, t, key, (k..pts.len()).collect());
        } else if resp.clicked() && shift {
            let mut w = selected.clone();
            if let Some(i) = w.iter().position(|x| *x == k) {
                w.remove(i);
            } else {
                w.push(k);
                w.sort_unstable();
            }
            set_sel(app, t, key, w);
        } else if resp.clicked() {
            set_sel(app, t, key, vec![k]);
        } else if resp.drag_started() {
            let copy = alt && is_sel;
            app.checkpoint(if copy { "Copy Automation" } else { "Move Automation Point" });
            let which = if is_sel { selected.clone() } else { vec![k] };
            if let Some(pp) = resp.interact_pointer_pos() {
                app.ta.auto.drag = Some(ADrag::Points { t, key, from: (tick_of(pp.x), v_of(pp.y)), orig: pts.clone(), which, copy, axis: 2 });
            }
        } else if let (true, Some(pp), Some(ADrag::Points { t: dt, key: dk, from, orig, which, copy, axis })) = (resp.dragged(), resp.interact_pointer_pos(), app.ta.auto.drag.clone()) {
            if dt == t && dk == key {
                // (the drag's direction decided over its first pixels: sideways = time only)
                let total = resp.total_drag_delta().unwrap_or_default();
                let axis = if axis == 2 && total.length() > 4.0 { u8::from(total.x.abs() > total.y.abs() * 2.0) } else { axis };
                if let Some(ADrag::Points { axis: a, .. }) = app.ta.auto.drag.as_mut() {
                    *a = axis;
                }
                let mut n = lane_now.clone();
                let dv = if axis == 1 { 0.0 } else if shift || ctrl { -total.y * 0.2 } else { v_of(pp.y) - from.1 };
                let lead_orig = orig[k.min(orig.len() - 1)].0 as f32;
                let dt_ticks = snap(app, lead_orig + tick_of(pp.x) - from.0) - lead_orig;
                if copy {
                    // the copy lands there, replacing what was in its span; the originals stay
                    let moved: Vec<(u32, u8)> = which.iter().map(|&w| ((orig[w].0 as f32 + dt_ticks).max(0.0) as u32, (orig[w].1 as f32 + dv).round().clamp(0.0, 127.0) as u8)).collect();
                    let (lo, hi) = (moved.iter().map(|m| m.0).min().unwrap_or(0), moved.iter().map(|m| m.0).max().unwrap_or(0));
                    n.points = orig.iter().copied().filter(|p| p.0 < lo || p.0 > hi).chain(moved).collect();
                } else {
                    let lo = which.iter().min().and_then(|&m| m.checked_sub(1)).map_or(0, |m| orig[m].0 + 1);
                    let hi = which.iter().max().and_then(|&m| orig.get(m + 1)).map_or(u32::MAX, |x| x.0.saturating_sub(1));
                    for &w in &which {
                        let (ot, ov) = orig[w];
                        let nt = ((ot as f32 + dt_ticks).max(0.0) as u32).clamp(lo, hi.max(lo));
                        n.points[w] = (nt, (ov as f32 + dv).round().clamp(0.0, 127.0) as u8);
                    }
                }
                let lead = n.points.get(k).copied().unwrap_or_default();
                app.help_tag = Some(vec![(key_name(app, t, key), key_label(app, t, key, lead.1 as f32)), ("Position".into(), crate::app::bbdt(lead.0, sig))]);
                edit = Some(n);
            }
        }
        if resp.drag_stopped() {
            app.ta.auto.drag = None;
            app.load_engine();
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
            let seg = seg_at(o);
            app.ta.auto.drag = if tool == crate::edit::Tool::Pencil || tool == crate::edit::Tool::Eraser {
                app.checkpoint(if tool == crate::edit::Tool::Pencil { "Draw Automation" } else { "Erase Automation" });
                Some(ADrag::Draw { t, key, orig: lane_now.clone(), from: tick })
            } else if let Some(m) = marquee.as_ref().filter(|m| tick >= m.from && tick <= m.to && tool != crate::edit::Tool::Marquee) {
                app.checkpoint("Move Automation");
                Some(ADrag::Range { t, from: m.from, to: m.to, y0: o.y, orig: lane_now.clone() })
            } else if let Some(k) = seg.filter(|k| alt && shift && selected.contains(k) && selected.contains(&(k + 1))) {
                app.checkpoint("Change Automation Ramp");
                Some(ADrag::Ramp { t, k, x0: o.x, orig: pts.clone() })
            } else if tool == crate::edit::Tool::Automation && !(alt || ctrl_shift) {
                Some(ADrag::Select { t, from: o })
            } else if let Some(k) = seg {
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
                Some(ADrag::Draw { t: dt, key: dk, orig, from }) if dt == t && dk == key => {
                    // the Pencil: points along the drag replace what was under it; the Eraser:
                    // the points crossed go
                    let now = tick_of(pp.x).max(0.0) as u32;
                    let (a, b) = (from.min(now), from.max(now));
                    let mut n = lane_now.clone();
                    if tool == crate::edit::Tool::Eraser {
                        n.points = orig.points.iter().copied().filter(|p| p.0 < a || p.0 > b).collect();
                    } else {
                        let step = (crate::autoext::snap_grid(app, px_per_tick) / 4.0).max(30.0) as u32;
                        let mut keep: Vec<(u32, u8)> = n.points.iter().copied().filter(|p| p.0 < a || p.0 > b).collect();
                        // (the drawn line from the press to here, values from the pointer)
                        let v = v_of(pp.y).round() as u8;
                        let mut drawn: Vec<(u32, u8)> = n.points.iter().copied().filter(|p| p.0 >= a && p.0 <= b).collect();
                        drawn.retain(|p| p.0.abs_diff(now) > step / 2);
                        drawn.push((snap(app, now as f32) as u32, v));
                        keep.extend(drawn);
                        n.points = keep;
                        // Pencil Tool setting: Option for stepped (or curved) editing
                        let stepped = crate::autoext::pencil_stepped(app, alt);
                        lg_core::automation::tidy(&mut n);
                        if stepped {
                            n.steps = n.points.iter().filter(|p| p.0 >= a && p.0 <= b).map(|p| p.0).collect();
                        } else {
                            n.steps.retain(|s| *s < a || *s > b);
                        }
                    }
                    edit = Some(n);
                }
                Some(ADrag::Ramp { t: dt, k, x0, orig }) if dt == t => {
                    let mut n = lane_now.clone();
                    n.points = orig.clone();
                    let d = ((pp.x - x0) / px_per_tick) as i64;
                    let lo = if k > 0 { orig[k - 1].0 as i64 + 1 } else { 0 };
                    let hi = orig.get(k + 2).map_or(i64::MAX, |x| x.0 as i64 - 1);
                    let a = (orig[k].0 as i64 - d).clamp(lo, orig[k + 1].0 as i64 - 1);
                    let b = (orig[k + 1].0 as i64 + d).clamp(a + 1, hi);
                    n.points[k].0 = a as u32;
                    n.points[k + 1].0 = b as u32;
                    app.help_tag = Some(vec![("Ramp".into(), crate::app::bbdt_len((b - a) as u32, sig))]);
                    edit = Some(n);
                }
                Some(ADrag::Segment { t: dt, k, y0, orig }) if dt == t => {
                    let mut n = lane_now.clone();
                    n.points = orig;
                    let f = if ctrl || shift { 0.2 } else { 1.0 };
                    lg_core::automation::move_segment(&mut n, k, (y0 - pp.y) / inner.height() * 127.0 * f);
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
                    // (Option: by value and time; else every point in the time range)
                    let which: Vec<usize> = pts.iter().enumerate().filter(|(_, (tk, v))| if alt { r.contains(pos2(x_of(*tk as f32), y_of(*v as f32))) } else { r.x_range().contains(x_of(*tk as f32)) }).map(|(k, _)| k).collect();
                    // (Shift: added to the selection)
                    let mut w = if shift { selected.clone() } else { Vec::new() };
                    for k in which {
                        if !w.contains(&k) {
                            w.push(k);
                        }
                    }
                    w.sort_unstable();
                    set_sel(app, t, key, w);
                }
                _ => {}
            }
        }
        if bg.drag_stopped() {
            app.ta.auto.drag = None;
            app.load_engine();
        }
        let toggle_step = (tool == crate::edit::Tool::Automation && alt) || (tool == crate::edit::Tool::Pointer && alt && ctrl_shift);
        if bg.clicked() {
            if let Some(pp) = bg.interact_pointer_pos() {
                if let (true, Some(k)) = (toggle_step, seg_at(pp)) {
                    app.checkpoint("Automation Curve");
                    let mut n = lane_now.clone();
                    lg_core::automation::toggle_step(&mut n, k);
                    edit = Some(n);
                } else if let (true, Some(k)) = (alt, seg_at(pp)) {
                    // Option-click the line: all points that follow
                    set_sel(app, t, key, (k + 1..pts.len()).collect());
                } else if let (true, Some(k)) = (tool == crate::edit::Tool::Automation, seg_at(pp)) {
                    // the Automation Select tool: a click on the line selects its segment
                    // (Shift: added; the Pointer adds a point there, as Logic's)
                    let mut w = if shift { selected.clone() } else { Vec::new() };
                    for x in [k, k + 1] {
                        if !w.contains(&x) {
                            w.push(x);
                        }
                    }
                    w.sort_unstable();
                    set_sel(app, t, key, w);
                } else if tool == crate::edit::Tool::Automation {
                    app.ta.auto.sel = None;
                } else if !alt {
                    // a point on the line where clicked (Logic: the click's height is ignored;
                    // the Pencil: at the click), with one at the start the first time
                    app.checkpoint("Create Automation Point");
                    let tk = snap(app, tick_of(pp.x)) as u32;
                    let mut n = lane_now.clone();
                    if n.points.is_empty() {
                        n.points.push((0, current.round() as u8));
                    }
                    if tool == crate::edit::Tool::Pencil {
                        n.points.retain(|x| x.0 != tk);
                        n.points.push((tk, v_of(pp.y).round() as u8));
                        lg_core::automation::tidy(&mut n);
                    } else {
                        lg_core::automation::add_on_line(&mut n, tk, current);
                    }
                    let k = n.points.iter().position(|x| x.0 == tk);
                    set_sel(app, t, key, k.into_iter().collect());
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
    let key = sel_key(app, t);
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
                let keys: Vec<Key> = if each { vec![Key::channel(VOLUME), Key::channel(PAN)] } else { vec![shown(app, t)] };
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
                    // (screen controls' lanes go with the last plug-in; MIDI lanes stay)
                    let any = !slots.is_empty();
                    tr.automation.retain(|l| l.plugin.is_none_or(|(s, _)| slots.contains(&s) || s == MIDI_SLOT || (s == SC_SLOT && any)) && !l.points.is_empty());
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
            let used: Vec<Key> = app.project.as_ref().map_or(Vec::new(), |p| p.tracks[t].automation.iter().filter(|l| !l.points.is_empty()).map(Key::of).collect());
            if used.is_empty() {
                return true;
            }
            let cur = shown(app, t);
            let next = used[(used.iter().position(|k| *k == cur).map_or(0, |i| i + 1)) % used.len()];
            show_key(app, t, next);
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
    let key = if app.ta.auto.sel.is_some() { sel_key(app, t) } else { shown(app, t) };
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
    played(app, t).into_iter().filter_map(|l| {
        let (slot, id) = l.plugin?;
        let (_, _, lo, hi, _) = plugin_param(app, t, slot, id)?;
        let at = if slot == 2 { None } else { Some(inserts.iter().position(|&s| s == slot)?) };
        Some((at, id, lo, hi, l))
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
                // (k is the place in the strip's insert list; effects without a stand-in aren't built)
                if let Some(fx) = u.insert_index.iter().position(|x| x == k).and_then(|j| u.inserts.get_mut(j)) {
                    fx.set(*id, x);
                }
            }
        }
    }
}

/// A fader moved while playing: in Touch, Latch or Write, a point at the playhead (Trim and
/// Relative: on the parameter's Relative curve, the move's offset from the absolute line; Trim
/// folds it into the absolute curve when the pass ends). Settings ▸ Automation ▸ Write
/// Automation For picks the parameters written. Not playing, in Read with Autoselect on: the
/// moved parameter's lane shows.
pub fn record(app: &mut App, t: usize, param: u8, value: f32) {
    record_key(app, t, Key::channel(param), value);
}

pub fn record_key(app: &mut App, t: usize, key: Key, value: f32) {
    if mode(app, t) == AutoMode::Read && crate::autoext::autoselect(app) && shown(app, t).abs() != key {
        show_key(app, t, key);
    }
    // (Read, recording, Record Automation with Regions on: into the region under the playhead)
    if mode(app, t) == AutoMode::Read && !key.rel && crate::autoext::record_into_region(app, t, key, value) {
        return;
    }
    let write = mode(app, t) == AutoMode::Write;
    if !app.playing() || !matches!(mode(app, t), AutoMode::Touch | AutoMode::Latch | AutoMode::Write) || (write && !crate::autoext::writes(app, key)) {
        return;
    }
    let tick = app.position() as u32;
    let rel = matches!(key.plugin, None) && matches!(key.param, VOLUME | PAN) && (app.ax.relative.contains(&t) || (app.ax.trim.contains(&t) && !write));
    let (key, value) = if rel {
        let abs = lane_of(app, t, key).and_then(|l| lg_core::automation::value_at(l, tick as f64)).unwrap_or(value);
        (Key { rel: true, ..key }, (64.0 + value - abs).clamp(0.0, 127.0))
    } else {
        (key, value)
    };
    app.ax.wrote.insert(t);
    if let Some(l) = lane_mut_key(app, t, key) {
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
    let lanes = played(app, t);
    let find = |param: u8| lanes.iter().find(|l| l.plugin.is_none() && l.param == param).map(sampled).unwrap_or_default();
    let mut vol: Vec<(u32, f32)> = find(VOLUME).into_iter().map(|(tk, v)| (tk, crate::app::db_to_gain(logic_to_db(v)))).collect();
    // Mute automation: silent where it's on (folded into the volume line, stepped)
    if let Some(m) = lanes.iter().find(|l| l.plugin.is_none() && l.param == MUTE && !l.points.is_empty()) {
        let fader = crate::app::db_to_gain(app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(0.0, |x| x.volume_db));
        vol = mute_fold(&vol, m, fader);
    }
    let pan = find(PAN).into_iter().map(|(tk, v)| (tk, ((v - 64.0) / 63.0).clamp(-1.0, 1.0))).collect();
    (vol, pan)
}

/// The lanes a track plays: its own with each region's automation over the region, the
/// Relative curves added to their absolute ones, lanes switched off left out.
pub fn played(app: &App, t: usize) -> Vec<AutoLane> {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return Vec::new() };
    let mut keys: Vec<Key> = tr.automation.iter().map(|l| Key::of(l).abs()).collect();
    keys.extend(tr.regions.iter().flat_map(|r| r.ext.automation.iter().map(|l| Key::of(l).abs())));
    let mut seen = Vec::new();
    keys.retain(|k| {
        let fresh = !seen.contains(k);
        seen.push(*k);
        fresh
    });
    let mut out = Vec::new();
    for k in keys {
        let abs = tr.automation.iter().find(|l| matches(l, k)).filter(|l| !l.muted);
        let spans: Vec<(u32, u32, &AutoLane)> = tr.regions.iter().filter(|r| !r.muted).filter_map(|r| r.ext.automation.iter().find(|l| matches(l, k)).map(|l| (r.start, r.start + r.length, l))).collect();
        let Some(mut l) = lg_core::automation::merge_regions(abs, &spans) else {
            if let Some(rel) = tr.automation.iter().find(|l| matches(l, Key { rel: true, ..k }) && !l.muted && !l.points.is_empty()) {
                out.push(lg_core::automation::consolidate(None, rel, current_value(app, t, k)));
            }
            continue;
        };
        if let Some(rel) = tr.automation.iter().find(|x| matches(x, Key { rel: true, ..k }) && !x.muted && !x.points.is_empty()) {
            l = lg_core::automation::consolidate(Some(&l), rel, current_value(app, t, k));
        }
        if !l.points.is_empty() {
            out.push(l);
        }
    }
    out
}

/// A volume line with a Mute lane folded in: gain 0 where mute is on.
fn mute_fold(vol: &[(u32, f32)], mute: &AutoLane, fader: f32) -> Vec<(u32, f32)> {
    let at = |tk: u32| curve_at(vol, tk as f64).unwrap_or(fader);
    let on = |tk: u32| lg_core::automation::value_at(mute, tk as f64).unwrap_or(0.0) >= 64.0;
    let mut ticks: Vec<u32> = vol.iter().map(|p| p.0).collect();
    for p in &mute.points {
        ticks.push(p.0.saturating_sub(1));
        ticks.push(p.0);
    }
    ticks.sort_unstable();
    ticks.dedup();
    ticks.into_iter().map(|tk| (tk, if on(tk) { 0.0 } else { at(tk) })).collect()
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

    #[test]
    fn mute_lane_silences_the_volume_line() {
        let mute = AutoLane { param: MUTE, points: vec![(0, 0), (960, 127), (1920, 0)], steps: vec![0, 960], ..Default::default() };
        let v = mute_fold(&[(0, 1.0)], &mute, 1.0);
        assert_eq!(curve_at(&v, 500.0), Some(1.0));
        assert_eq!(curve_at(&v, 1200.0), Some(0.0));
        assert_eq!(curve_at(&v, 1920.0), Some(1.0));
    }
}
