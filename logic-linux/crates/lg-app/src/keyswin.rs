//! What the keyboard and studio instruments' windows share (Vintage B3, Vintage Electric Piano,
//! Vintage Clav, Vintage Mellotron, Studio Piano / Strings / Horns / Bass, Sample Alchemy): which
//! window an instrument gets, the header with the Settings pop-up (save, load, delete, default —
//! files in the plug-in settings folder), and the controls drawn the same way in each: knobs,
//! switches, pop-up menus, segmented switches, sliders, titled groups and tabs.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_dsp::{Param, Unit};

use crate::app::App;
use crate::instwin;
use crate::look;

pub const COMMANDS: &[&str] = &[];

/// Opens the track's instrument window when it is one of these instruments.
pub fn window(app: &mut App, ctx: &egui::Context, t: usize) -> bool {
    #[derive(Clone, Copy)]
    enum K {
        B3,
        Ep,
        Clav,
        Mellotron,
        Piano,
        Ensemble,
        Bass,
    }
    let kind = {
        let Ok(e) = app.engine.lock() else { return false };
        let Some(any) = e.units.get(t).and_then(|u| u.instrument.as_ref()).and_then(|i| i.as_any()) else { return false };
        if any.is::<lg_synth::organ::TonewheelB3>() {
            K::B3
        } else if any.is::<lg_synth::ep::VintageKeysEp>() {
            K::Ep
        } else if any.is::<lg_synth::clav::VintageClav>() {
            K::Clav
        } else if any.is::<lg_synth::mellotron::VintageMellotron>() {
            K::Mellotron
        } else if any.is::<lg_synth::piano::StudioPiano>() {
            K::Piano
        } else if any.is::<lg_synth::ensemble::StudioEnsemble>() {
            K::Ensemble
        } else if any.is::<lg_synth::bass::StudioBass>() {
            K::Bass
        } else {
            return false;
        }
    };
    match kind {
        K::B3 => crate::b3win::window(app, ctx, t),
        K::Ep => crate::epwin::window(app, ctx, t),
        K::Clav => crate::clavwin::window(app, ctx, t),
        K::Mellotron => crate::mellotronwin::window(app, ctx, t),
        K::Piano => crate::studiowin::piano(app, ctx, t),
        K::Ensemble => crate::studiowin::ensemble(app, ctx, t),
        K::Bass => crate::studiowin::bass(app, ctx, t),
    }
    true
}

/// A plain window (header and the Controls view) for an instrument without its own layout yet.
pub fn generic(app: &mut App, ctx: &egui::Context, t: usize, title: &str, plugin: &str) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let mut open = true;
    let resp = egui::Window::new(title).id(egui::Id::unique(("keys-generic", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(560.0, 520.0)).show(ctx, |ui| {
        header(app, ui, &mut pane, plugin);
        let set = crate::pluginwin::controls(ui, &params, &vals, &[], "keys-generic");
        pane.set.extend(set);
    });
    let set = std::mem::take(&mut pane.set);
    finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Instrument Setting", open);
}

/// A value as its unit reads.
pub fn shown(p: &Param, v: f32) -> String {
    match p.unit {
        Unit::Menu => p.choices.get(v.round().max(0.0) as usize).copied().unwrap_or("").to_string(),
        Unit::OnOff => if v >= 0.5 { "On" } else { "Off" }.to_string(),
        Unit::Db => format!("{v:.1} dB"),
        Unit::Hz => if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.2} Hz") },
        Unit::Ms => format!("{v:.0} ms"),
        Unit::Percent => format!("{v:.0} %"),
        Unit::Ratio => format!("{:.0} %", v * 100.0),
        Unit::Semitones => format!("{v:+.1} st"),
        Unit::Cents => format!("{v:+.0} c"),
        Unit::Seconds => format!("{v:.2} s"),
        Unit::Octaves => format!("{v:+.0} oct"),
        Unit::Pan => format!("{v:+.2}"),
        Unit::None => if (p.max - p.min) >= 8.0 && p.max.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.2}") },
    }
}

/// One window's controls: reads the instrument's values, collects the changes.
pub struct Pane<'a> {
    pub t: usize,
    pub params: &'a [Param],
    pub vals: &'a [f32],
    pub set: Vec<(u32, f32)>,
}

impl<'a> Pane<'a> {
    pub fn new(t: usize, params: &'a [Param], vals: &'a [f32]) -> Pane<'a> {
        Pane { t, params, vals, set: Vec::new() }
    }
    pub fn get(&self, id: u32) -> f32 {
        // (a change made this frame shows at once)
        if let Some(x) = self.set.iter().rev().find(|x| x.0 == id) {
            return x.1;
        }
        self.params.iter().position(|x| x.id == id).map(|i| self.vals[i]).unwrap_or(0.0)
    }
    pub fn on(&self, id: u32) -> bool {
        self.get(id) >= 0.5
    }
    pub fn param(&self, id: u32) -> Option<&'a Param> {
        self.params.iter().find(|x| x.id == id)
    }
    pub fn put(&mut self, id: u32, v: f32) {
        self.set.push((id, v));
    }

    /// A rotary knob with its label under it.
    pub fn knob(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 74.0), Sense::hover());
        self.knob_at(ui, r, id, label);
    }
    pub fn knob_at(&mut self, ui: &mut egui::Ui, r: Rect, id: u32, label: &str) {
        let Some(p) = self.param(id) else { return };
        let mut v = self.get(id);
        if crate::widgets::knob(ui, r, egui::Id::unique(("keys-knob", self.t, id)), &mut v, p.min, p.max, p.default, label, &|x| shown(p, x)) {
            self.put(id, if p.unit == Unit::Menu { v.round() } else { v });
        }
    }
    /// A row of knobs: (id, label).
    pub fn knobs(&mut self, ui: &mut egui::Ui, list: &[(u32, &str)]) {
        ui.horizontal(|ui| {
            for (id, label) in list {
                self.knob(ui, *id, label);
            }
        });
    }
    /// An on/off switch (a lit capsule) with its label.
    pub fn switch(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        let on = self.on(id);
        if toggle(ui, on, label) {
            self.put(id, if on { 0.0 } else { 1.0 });
        }
    }
    /// A pop-up menu of the parameter's choices.
    pub fn menu(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        let Some(p) = self.param(id) else { return };
        let cur = self.get(id).round().max(0.0) as usize;
        ui.horizontal(|ui| {
            if !label.is_empty() {
                ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
            }
            ui.menu_button(format!("{} ⌄", p.choices.get(cur).copied().unwrap_or("")), |ui| {
                for (k, c) in p.choices.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == cur, *c)).clicked() {
                        self.set.push((id, k as f32));
                        ui.close();
                    }
                }
            });
        });
    }
    /// A segmented switch of the parameter's choices.
    pub fn seg(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        let Some(p) = self.param(id) else { return };
        let cur = self.get(id).round().max(0.0) as usize;
        ui.horizontal(|ui| {
            if !label.is_empty() {
                ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
            }
            ui.spacing_mut().item_spacing.x = 1.0;
            for (k, c) in p.choices.iter().enumerate() {
                if ui.add(egui::Button::selectable(k == cur, egui::RichText::new(*c).size(11.0))).clicked() && k != cur {
                    self.set.push((id, k as f32));
                }
            }
        });
    }
    /// A horizontal slider with its label and value.
    pub fn slider(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        let Some(p) = self.param(id) else { return };
        let mut v = self.get(id);
        ui.horizontal(|ui| {
            ui.add_sized(vec2(118.0, 16.0), egui::Label::new(egui::RichText::new(label).size(11.0).color(look::text_dim())).truncate());
            let whole = p.unit == Unit::None && p.max.fract() == 0.0 && (p.max - p.min) >= 8.0;
            let mut s = egui::Slider::new(&mut v, p.min..=p.max).show_value(false);
            if whole {
                s = s.step_by(1.0);
            }
            let r = ui.add(s);
            if r.changed() {
                self.set.push((id, v));
            }
            if r.double_clicked() {
                self.set.push((id, p.default));
            }
            ui.label(egui::RichText::new(shown(p, v)).size(11.0));
        });
    }
    /// A MIDI controller choice (0 off, 1..119, 120 aftertouch, 121 Learn).
    pub fn controller_menu(&mut self, ui: &mut egui::Ui, id: u32, label: &str) {
        self.source_menu(ui, id, label, false);
    }
    /// The same, with Velocity (122) when `velocity`.
    pub fn source_menu(&mut self, ui: &mut egui::Ui, id: u32, label: &str, velocity: bool) {
        let cur = self.get(id).round() as i32;
        let name = |n: i32| match n {
            0 => "Off".to_string(),
            120 => "Aftertouch".to_string(),
            121 => "Learn…".to_string(),
            122 => "Velocity".to_string(),
            1 => "1 Modulation".to_string(),
            2 => "2 Breath".to_string(),
            4 => "4 Foot".to_string(),
            11 => "11 Expression".to_string(),
            n => format!("{n}"),
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
            ui.menu_button(format!("{} ⌄", name(cur)), |ui| {
                for n in [0, 121, 120, 122] {
                    if n == 122 && !velocity {
                        continue;
                    }
                    if ui.add(egui::Button::selectable(n == cur, name(n))).clicked() {
                        self.set.push((id, n as f32));
                        ui.close();
                    }
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for n in 1..120 {
                        if ui.add(egui::Button::selectable(n == cur, name(n))).clicked() {
                            self.set.push((id, n as f32));
                            ui.close();
                        }
                    }
                });
            });
        });
    }
}

/// A lit capsule switch; true when clicked.
pub fn toggle(ui: &mut egui::Ui, on: bool, label: &str) -> bool {
    let font = look::regular(11.0);
    let w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), look::text()).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(w + 38.0, 20.0), Sense::click());
    let p = ui.painter();
    let sw = Rect::from_min_size(pos2(r.min.x, r.center().y - 7.0), vec2(26.0, 14.0));
    p.rect_filled(sw, 7.0, if on { look::accent() } else { Color32::from_gray(0x3a) });
    p.circle_filled(pos2(if on { sw.max.x - 7.0 } else { sw.min.x + 7.0 }, sw.center().y), 5.5, Color32::WHITE);
    p.text(pos2(sw.max.x + 6.0, r.center().y), Align2::LEFT_CENTER, label, font, if resp.hovered() { look::text() } else { look::text_dim() });
    resp.clicked()
}

/// Names in a row that can be dragged onto each other to change their order (an effects
/// chain); returns (from, to) when one was dropped on another.
pub fn reorder(ui: &mut egui::Ui, id: egui::Id, names: &[&str]) -> Option<(usize, usize)> {
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(id));
    let mut drop = None;
    let released = ui.input(|x| x.pointer.any_released());
    for (i, n) in names.iter().enumerate() {
        let resp = ui.add(egui::Button::new(egui::RichText::new(*n).size(11.0)).sense(Sense::click_and_drag())).on_hover_text("Drag to reorder the effects");
        if resp.drag_started() {
            dragging = Some(i);
        }
        if let Some(from) = dragging {
            if released && from != i && ui.rect_contains_pointer(resp.rect) {
                drop = Some((from, i));
            }
        }
        if i + 1 < names.len() {
            ui.label("›");
        }
    }
    if released {
        dragging = None;
    }
    ui.data_mut(|d| match dragging {
        Some(v) => {
            d.insert_temp(id, v);
        }
        None => d.remove::<usize>(id),
    });
    drop
}

/// The extended parameters behind the disclosure arrow at the lower left.
pub fn extended(ui: &mut egui::Ui, id: egui::Id, body: impl FnOnce(&mut egui::Ui)) {
    let mut open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    ui.add_space(4.0);
    if ui.add(egui::Button::new(egui::RichText::new(if open { "▾ Extended Parameters" } else { "▸ Extended Parameters" }).size(11.0).color(look::text_dim())).frame(false)).clicked() {
        open = !open;
    }
    if open {
        egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(8.0).show(ui, body);
    }
    ui.data_mut(|d| d.insert_temp(id, open));
}

/// A titled group of controls.
pub fn group<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).size(11.0).strong().color(look::text_dim()));
            ui.add_space(2.0);
            body(ui)
        })
        .inner
    })
    .inner
}

/// The view buttons across the top (Logic's control bar buttons); true when changed.
pub fn tabs(ui: &mut egui::Ui, tab: &mut usize, names: &[&str]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (k, n) in names.iter().enumerate() {
            if ui.add(egui::Button::selectable(*tab == k, egui::RichText::new(*n).size(12.0))).clicked() && *tab != k {
                *tab = k;
                changed = true;
            }
        }
    });
    changed
}

fn read_setting(path: &std::path::Path) -> Option<Vec<(u32, f32)>> {
    let t = std::fs::read_to_string(path).ok()?;
    Some(parse_setting(&t))
}

/// A saved setting's text: "id value" per line.
pub fn parse_setting(t: &str) -> Vec<(u32, f32)> {
    t.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
        })
        .collect()
}

pub fn setting_text(params: &[Param], vals: &[f32]) -> String {
    params.iter().zip(vals).map(|(p, v)| format!("{} {}\n", p.id, v)).collect()
}

/// Saves the instrument's settings under a name (the plug-in settings folder).
pub fn save_setting(plugin: &str, name: &str, params: &[Param], vals: &[f32]) -> Option<std::path::PathBuf> {
    let dir = crate::pluginwin::settings_dir(plugin)?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.txt", name.replace('/', "-")));
    std::fs::write(&path, setting_text(params, vals)).ok()?;
    Some(path)
}

/// The window's header: the Settings pop-up (the saved settings; Save As, Delete, Recall
/// Default), Copy / Paste of the settings, and the track's name.
pub fn header(app: &mut App, ui: &mut egui::Ui, pane: &mut Pane, plugin: &str) {
    let t = pane.t;
    let name_id = egui::Id::unique(("keys-setting-name", t, plugin.to_string()));
    let typed_id = egui::Id::unique(("keys-setting-typed", t));
    let mut current: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
    let mut typed: String = ui.data(|d| d.get_temp(typed_id)).unwrap_or_default();
    let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    ui.painter().rect_filled(hr, 6.0, look::header_bg());
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(hr.shrink2(vec2(8.0, 3.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let mut load: Option<Vec<(u32, f32)>> = None;
    bar.menu_button(format!("{} ⌄", if current.is_empty() { "Default" } else { current.as_str() }), |ui| {
        ui.set_min_width(210.0);
        if ui.button("Recall Default").clicked() {
            load = Some(pane.params.iter().map(|p| (p.id, p.default)).collect());
            current.clear();
            ui.close();
        }
        if !current.is_empty() && ui.button("Save").clicked() {
            save_setting(plugin, &current, pane.params, pane.vals);
            ui.close();
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut typed).hint_text("Save As…").desired_width(130.0));
            if ui.add_enabled(!typed.trim().is_empty(), egui::Button::new("Save")).clicked() {
                if save_setting(plugin, typed.trim(), pane.params, pane.vals).is_some() {
                    current = typed.trim().to_string();
                    typed.clear();
                }
                ui.close();
            }
        });
        let saved = crate::pluginwin::saved_settings(plugin);
        if !saved.is_empty() {
            ui.separator();
        }
        for (n, path) in &saved {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::selectable(*n == current, n.as_str())).clicked() {
                    if let Some(s) = read_setting(path) {
                        load = Some(s);
                        current = n.clone();
                    }
                    ui.close();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Delete").clicked() {
                        let _ = std::fs::remove_file(path);
                        if *n == current {
                            current.clear();
                        }
                        ui.close();
                    }
                });
            });
        }
    });
    if bar.small_button("Copy").on_hover_text("Copy the settings").clicked() {
        app.inst.synth_clip = Some(pane.params.iter().zip(pane.vals).map(|(p, v)| (p.id, *v)).collect());
    }
    if bar.add_enabled(app.inst.synth_clip.is_some(), egui::Button::new("Paste").small()).on_hover_text("Paste the copied settings").clicked() {
        load = app.inst.synth_clip.clone();
    }
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    ui.painter().text(pos2(hr.max.x - 10.0, hr.center().y), Align2::RIGHT_CENTER, track, look::regular(11.5), look::text_dim());
    if let Some(s) = load {
        // (only what this instrument has; the rest of a pasted list is left out)
        for (id, v) in s {
            if pane.param(id).is_some() {
                pane.put(id, v);
            }
        }
    }
    ui.data_mut(|d| {
        d.insert_temp(name_id, current);
        d.insert_temp(typed_id, typed);
    });
    ui.add_space(4.0);
}

/// After the window: its rectangle (keys and drops go to it), the changes with one undo step per
/// gesture, and closing.
pub fn finish(app: &mut App, ctx: &egui::Context, t: usize, rect: Option<Rect>, set: Vec<(u32, f32)>, what: &str, open: bool) {
    app.inst.rect = rect;
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, what);
        instwin::set_params(app, t, &set);
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}

/// Plays a note on the track for a moment (a click on a drawn key).
pub fn audition(app: &App, t: usize, note: u8, velocity: u8) {
    let engine = app.engine.clone();
    if let Ok(mut e) = engine.lock() {
        e.live(t, note, velocity);
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(350));
        if let Ok(mut e) = engine.lock() {
            e.live(t, note, 0);
        }
    });
}

/// A small keyboard (white and black keys) over `r` for notes lo..=hi; returns the key under
/// a click. `mark` colours a key (zones, keyswitches).
pub fn keyboard(ui: &mut egui::Ui, r: Rect, id: egui::Id, lo: u8, hi: u8, mark: &dyn Fn(u8) -> Option<Color32>) -> Option<u8> {
    let p = ui.painter().clone();
    let is_black = |n: u8| matches!(n % 12, 1 | 3 | 6 | 8 | 10);
    let whites: Vec<u8> = (lo..=hi).filter(|n| !is_black(*n)).collect();
    let w = r.width() / whites.len().max(1) as f32;
    let resp = ui.interact(r, id, Sense::click());
    let mut rects: Vec<(u8, Rect, bool)> = Vec::new();
    for (i, n) in whites.iter().enumerate() {
        rects.push((*n, Rect::from_min_size(pos2(r.min.x + i as f32 * w, r.min.y), vec2(w, r.height())), false));
    }
    for (i, n) in whites.iter().enumerate() {
        if *n < hi && is_black(n + 1) {
            rects.push((n + 1, Rect::from_min_size(pos2(r.min.x + (i as f32 + 1.0) * w - w * 0.3, r.min.y), vec2(w * 0.6, r.height() * 0.62)), true));
        }
    }
    for (n, kr, black) in &rects {
        let base = if *black { Color32::from_gray(0x18) } else { Color32::from_gray(0xe8) };
        let fill = mark(*n).map(|c| look::mix(base, c, if *black { 0.75 } else { 0.55 })).unwrap_or(base);
        p.rect_filled(kr.shrink(0.5), 1.5, fill);
        if !*black {
            p.rect_stroke(kr.shrink(0.5), 1.5, Stroke::new(0.5, Color32::from_gray(0x60)), egui::StrokeKind::Inside);
        }
        if !*black && n % 12 == 0 && w > 9.0 {
            p.text(pos2(kr.center().x, kr.max.y - 6.0), Align2::CENTER_CENTER, format!("C{}", *n as i32 / 12 - 2), look::regular(7.5), Color32::from_gray(0x50));
        }
    }
    if resp.clicked() {
        let at = resp.interact_pointer_pos()?;
        // black keys lie on top
        return rects.iter().rev().find(|(_, kr, _)| kr.contains(at)).map(|x| x.0);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_text_round_trips() {
        let params = lg_synth::organ::params();
        let vals: Vec<f32> = params.iter().map(|p| p.default).collect();
        let text = setting_text(params, &vals);
        let back = parse_setting(&text);
        assert_eq!(back.len(), params.len());
        assert!(back.iter().zip(params).all(|(b, p)| b.0 == p.id && b.1 == p.default));
    }

    #[test]
    fn values_read_by_their_unit() {
        let p = Param::new(0, "x", -24.0, 6.0, 0.0, Unit::Db);
        assert_eq!(shown(&p, -6.0), "-6.0 dB");
        let m = Param::menu(1, "m", &["A", "B"], 0.0);
        assert_eq!(shown(&m, 1.0), "B");
    }
}
