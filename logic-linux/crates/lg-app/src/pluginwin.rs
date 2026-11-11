//! Plug-in windows, as Logic's: the header (bypass, the setting pop-up with ‹ ›, Compare,
//! Copy / Paste, View: Controls) and the Controls view: one row per parameter (name, a slider
//! or a pop-up or a switch, the value), in the plug-in's own order. Instruments open from the
//! instrument slot, effects from their Audio FX slot (every lg-fx effect, on tracks, auxes
//! and the Stereo Out). An effect's settings can be saved and recalled (Save Setting As…, the
//! list in the pop-up, ‹ › and Next / Previous Plug-in Setting), reset, copied and pasted.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

pub(crate) fn fmt(p: &lg_dsp::Param, v: f32) -> String {
    use lg_dsp::Unit::*;
    match p.unit {
        Menu => p.choices.get(v.round().max(0.0) as usize).copied().unwrap_or("").to_string(),
        OnOff => if v >= 0.5 { "On".into() } else { "Off".into() },
        Db => format!("{v:.1} dB"),
        Hz => if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.0} Hz") },
        Ms => format!("{v:.0} ms"),
        Seconds => format!("{v:.2} s"),
        Percent => format!("{v:.0} %"),
        Ratio => format!("{v:.1} : 1"),
        Semitones => format!("{v:+.0} st"),
        Cents => format!("{v:+.0} ct"),
        Octaves => format!("{v:.2} oct"),
        Pan => format!("{:+.0}", v * 64.0),
        None => format!("{v:.2}"),
    }
}

/// The Controls view: a row per parameter (the hidden ones, a custom view's stored state,
/// left out); returns the parameters changed (id, value).
pub(crate) fn controls(ui: &mut egui::Ui, params: &[lg_dsp::Param], values: &[f32], salt: &str) -> Vec<(u32, f32)> {
    let mut set = Vec::new();
    egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(ui, |ui| {
        set = control_rows(ui, params, values, salt);
    });
    set
}

/// The Controls view's rows without the scroll area (custom views embed them).
pub(crate) fn control_rows(ui: &mut egui::Ui, params: &[lg_dsp::Param], values: &[f32], salt: &str) -> Vec<(u32, f32)> {
    let mut set = Vec::new();
    {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (prm, &v) in params.iter().zip(values.iter()) {
            if prm.hidden {
                continue;
            }
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
            let p = ui.painter().clone();
            let name_w = (row.width() * 0.34).min(200.0);
            p.text(pos2(row.min.x + name_w, row.center().y), Align2::RIGHT_CENTER, prm.name, look::regular(12.0), look::text_dim());
            let ctl = Rect::from_min_max(pos2(row.min.x + name_w + 12.0, row.min.y + 3.0), pos2(row.max.x - 96.0, row.max.y - 3.0));
            let id = ui.scope_id().with((salt, "pw-param", prm.id));
            match prm.unit {
                lg_dsp::Unit::Menu => {
                    let cur = v.round().max(0.0) as usize;
                    if let Some(k) = crate::widgets::choice_slot(ui, ctl, id, cur, prm.choices) {
                        set.push((prm.id, k as f32));
                    }
                }
                lg_dsp::Unit::OnOff => {
                    let mut on = v >= 0.5;
                    if crate::widgets::check_slot(ui, Rect::from_min_size(ctl.min, vec2(24.0, ctl.height())), id, &mut on) {
                        set.push((prm.id, if on { 1.0 } else { 0.0 }));
                    }
                }
                _ => {
                    let f = ((v - prm.min) / (prm.max - prm.min).max(1e-9)).clamp(0.0, 1.0);
                    let track = Rect::from_center_size(ctl.center(), vec2(ctl.width(), 4.0));
                    p.rect_filled(track, 2.0, look::group_bg());
                    p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * f, 4.0)), 2.0, look::accent());
                    p.circle_filled(pos2(track.min.x + track.width() * f, track.center().y), 6.5, Color32::from_gray(0xe0));
                    let resp = ui.interact(ctl, id, Sense::click_and_drag());
                    if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
                        set.push((prm.id, prm.default));
                    } else if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
                        let nf = ((pp.x - track.min.x) / track.width()).clamp(0.0, 1.0);
                        set.push((prm.id, prm.min + nf * (prm.max - prm.min)));
                    }
                }
            }
            p.text(pos2(row.max.x - 4.0, row.center().y), Align2::RIGHT_CENTER, fmt(prm, v), look::regular(12.0), look::text());
        }
    }
    set
}

/// The header's bypass button (a power symbol); true when clicked.
fn bypass_button(ui: &mut egui::Ui, p: &egui::Painter, hr: Rect, on: bool, salt: &str) -> bool {
    let bypass = Rect::from_center_size(pos2(hr.min.x + 18.0, hr.center().y), vec2(22.0, 22.0));
    p.circle_stroke(bypass.center(), 7.5, Stroke::new(1.6, if on { look::accent() } else { look::text_dim() }));
    p.vline(bypass.center().x, bypass.center().y - 9.0..=bypass.center().y - 2.0, Stroke::new(1.6, if on { look::accent() } else { look::text_dim() }));
    ui.interact(bypass, ui.scope_id().with((salt, "pw-bypass")), Sense::click()).on_hover_text("Bypass").clicked()
}

/// Compare / Copy / Paste at the header's right; the one clicked (0 Compare, 1 Copy, 2 Paste).
fn header_buttons(ui: &mut egui::Ui, p: &egui::Painter, hr: Rect, salt: &str, comparing: bool) -> Option<u8> {
    let mut x = hr.max.x - 8.0;
    let mut hit = None;
    for (label, act) in [("Paste", 2u8), ("Copy", 1), ("Compare", 0)] {
        let w = p.layout_no_wrap(label.into(), look::regular(12.0), look::text()).size().x + 16.0;
        let r = Rect::from_min_max(pos2(x - w, hr.min.y + 4.0), pos2(x, hr.max.y - 4.0));
        x -= w + 6.0;
        let resp = ui.interact(r, ui.scope_id().with((salt, "pw", act)), Sense::click());
        let lit = resp.hovered() || (act == 0 && comparing);
        p.rect_filled(r, 5.0, if lit { look::button_on() } else { look::group_bg() });
        p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), look::text());
        if resp.clicked() {
            hit = Some(act);
        }
    }
    hit
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    fx_window(app, ctx);
    crate::mixer::group_window(app, ctx);
    let Some(t) = app.plugin_window else { return };
    let Some((params, values)) = app.engine.lock().ok().and_then(|e| e.instrument_params(t)) else {
        app.plugin_window = None;
        app.tell("This track plays through the SoundFont: no plug-in window");
        return;
    };
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let patch = app.instrument_for(t).unwrap_or_default();
    let mut open = true;
    let mut set: Vec<(u32, f32)> = Vec::new();
    egui::Window::new(format!("{track} — {patch}")).id(egui::Id::unique("plugin-window")).open(&mut open).collapsible(false).resizable(true).default_size(vec2(560.0, 520.0)).show(ctx, |ui| {
        // the header
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(hr, 6.0, look::header_bg());
        let on = !app.plugin_bypass.contains(&t);
        if bypass_button(ui, &p, hr, on, "inst") {
            if on {
                app.plugin_bypass.insert(t);
            } else {
                app.plugin_bypass.remove(&t);
            }
        }
        p.text(pos2(hr.min.x + 40.0, hr.center().y), Align2::LEFT_CENTER, &patch, look::semibold(12.5), look::text());
        // Copy / Paste of the settings, Compare (the settings as they were when opened)
        match header_buttons(ui, &p, hr, "inst", app.plugin_compare.contains_key(&t)) {
            Some(1) => app.plugin_clipboard = Some(values.clone()),
            Some(2) => {
                if let Some(v) = app.plugin_clipboard.clone() {
                    for (prm, val) in params.iter().zip(v) {
                        set.push((prm.id, val));
                    }
                }
            }
            Some(_) => {
                // swap the current settings with the ones kept at the last Compare
                let keep = app.plugin_compare.insert(t, values.clone());
                if let Some(v) = keep {
                    for (prm, val) in params.iter().zip(v) {
                        set.push((prm.id, val));
                    }
                }
            }
            None => {}
        }
        ui.add_space(6.0);
        set.extend(controls(ui, &params, &values, "inst"));
    });
    if !set.is_empty() {
        if let Ok(mut e) = app.engine.lock() {
            for (id, v) in set {
                e.set_instrument_param(t, id, v);
            }
        }
        app.dirty = true;
    }
    if !open {
        app.plugin_window = None;
    }
}

/// An effect's parameters and their values: from the engine when it runs it, else from a
/// stand-in built with the slot's saved settings.
fn effect_state(app: &App, s: usize, k: usize) -> Option<(String, Vec<lg_dsp::Param>, Vec<f32>)> {
    let u = crate::mixstate::inserts(app, s).get(k)?.clone();
    if let Some((params, values)) = app.engine.lock().ok().and_then(|e| e.effect_params(crate::mixer::fx_strip(s), k)) {
        return Some((u.name, params, values));
    }
    let mut fx = crate::units::effect(&u.name, crate::audio::RATE as f32)?;
    if !u.params.is_empty() {
        fx.load_floats(&u.params);
    }
    let params = fx.params().to_vec();
    let values = params.iter().map(|p| fx.get(p.id)).collect();
    Some((u.name, params, values))
}

/// `effect_state` for the custom views' actions.
pub(crate) fn effect_state_pub(app: &App, s: usize, k: usize) -> Option<(String, Vec<lg_dsp::Param>, Vec<f32>)> {
    effect_state(app, s, k)
}

/// The saved float block for these values (index = parameter id), as a project keeps it.
pub fn floats_of(params: &[lg_dsp::Param], values: &[f32], base: &[f32]) -> Vec<f32> {
    let n = params.iter().map(|p| p.id as usize + 1).max().unwrap_or(0).max(base.len());
    let mut f = base.to_vec();
    f.resize(n, 0.0);
    for (p, v) in params.iter().zip(values) {
        f[p.id as usize] = *v;
    }
    f
}

/// Sets an effect's settings (all of them) in the engine and the slot.
fn apply_floats(app: &mut App, s: usize, k: usize, floats: Vec<f32>, preset: Option<String>) {
    if let Ok(mut e) = app.engine.lock() {
        e.load_effect(crate::mixer::fx_strip(s), k, &floats);
    }
    crate::mixstate::edit_inserts(app, s, |v| {
        if let Some(u) = v.get_mut(k) {
            u.params = floats;
            if let Some(p) = preset {
                u.preset = p;
            }
        }
    });
}

pub(crate) fn settings_dir(plugin: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("plug-in-settings").join(plugin.replace('/', "-")))
}

/// The settings saved for a plug-in (name, path), by name.
pub fn saved_settings(plugin: &str) -> Vec<(String, std::path::PathBuf)> {
    let Some(dir) = settings_dir(plugin) else { return Vec::new() };
    let mut v: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).map(|p| (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p)).collect();
    v.sort();
    v
}

/// A saved setting: "id value" per line.
fn read_setting(path: &std::path::Path) -> Option<Vec<(u32, f32)>> {
    let t = std::fs::read_to_string(path).ok()?;
    Some(t.lines().filter_map(|l| {
        let mut w = l.split_whitespace();
        Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
    }).collect())
}

fn write_setting(plugin: &str, name: &str, params: &[lg_dsp::Param], values: &[f32]) -> Option<()> {
    let dir = settings_dir(plugin)?;
    std::fs::create_dir_all(&dir).ok()?;
    let body: String = params.iter().zip(values).map(|(p, v)| format!("{} {}\n", p.id, v)).collect();
    std::fs::write(dir.join(format!("{}.txt", name.replace('/', "-"))), body).ok()
}

fn load_setting(app: &mut App, s: usize, k: usize, name: &str, path: &std::path::Path) {
    let Some((_, params, values)) = effect_state(app, s, k) else { return };
    let Some(pairs) = read_setting(path) else { return };
    let mut vals = values.clone();
    for (id, v) in pairs {
        if let Some(i) = params.iter().position(|p| p.id == id) {
            vals[i] = v;
        }
    }
    let base = crate::mixstate::inserts(app, s).get(k).map(|u| u.params.clone()).unwrap_or_default();
    apply_floats(app, s, k, floats_of(&params, &vals, &base), Some(name.to_string()));
}

/// The next (or previous) saved setting for the effect (‹ ›, Next / Previous Plug-in Setting).
pub fn step_setting(app: &mut App, s: usize, k: usize, by: i32) {
    let Some(u) = crate::mixstate::inserts(app, s).get(k).cloned() else { return };
    let list = saved_settings(&u.name);
    if list.is_empty() {
        return;
    }
    let at = list.iter().position(|(n, _)| *n == u.preset);
    let next = match at {
        Some(i) => (i as i32 + by).rem_euclid(list.len() as i32) as usize,
        None if by > 0 => 0,
        None => list.len() - 1,
    };
    let (n, p) = list[next].clone();
    load_setting(app, s, k, &n, &p);
}

/// The effect window open on a slot.
fn fx_window(app: &mut App, ctx: &egui::Context) {
    let Some((s, k)) = app.mix.fx_window else { return };
    let Some((name, params, values)) = effect_state(app, s, k) else {
        app.mix.fx_window = None;
        return;
    };
    let strip = crate::mixstate::strip_name(app, s);
    let slot = crate::mixstate::inserts(app, s).get(k).cloned().unwrap_or_else(|| crate::mixstate::new_plugin(&name));
    let mut open = true;
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut action: Option<u8> = None;
    let mut toggle_bypass = false;
    let mut step = 0;
    let mut load: Option<(String, std::path::PathBuf)> = None;
    let mut reset = false;
    let mut save: Option<String> = None;
    let comparing = app.mix.compare.contains_key(&(s, k));
    let mut fx_action: Option<crate::fxviews::Action> = None;
    let size = crate::fxviews::default_size(&name);
    egui::Window::new(format!("{strip} — {name}")).open(&mut open).collapsible(false).resizable(true).default_size(size).show(ctx, |ui| {
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(hr, 6.0, look::header_bg());
        toggle_bypass = bypass_button(ui, &p, hr, !slot.bypass, "fx");
        // the setting pop-up with ‹ ›
        let lr = Rect::from_min_size(pos2(hr.min.x + 36.0, hr.min.y + 5.0), vec2(16.0, 20.0));
        let rr = Rect::from_min_size(pos2(lr.max.x + 2.0, hr.min.y + 5.0), vec2(16.0, 20.0));
        p.text(lr.center(), Align2::CENTER_CENTER, "‹", look::regular(14.0), look::text());
        p.text(rr.center(), Align2::CENTER_CENTER, "›", look::regular(14.0), look::text());
        if ui.interact(lr, ui.scope_id().with("fx-prev"), Sense::click()).on_hover_text("Previous Setting").clicked() {
            step = -1;
        }
        if ui.interact(rr, ui.scope_id().with("fx-next"), Sense::click()).on_hover_text("Next Setting").clicked() {
            step = 1;
        }
        let pr = Rect::from_min_size(pos2(rr.max.x + 6.0, hr.min.y + 4.0), vec2(170.0, 22.0));
        p.rect_filled(pr, 5.0, look::group_bg());
        let shown = if slot.preset.is_empty() { "#default".to_string() } else { slot.preset.trim_end_matches(".pst").to_string() };
        p.with_clip_rect(pr.shrink(2.0)).text(pr.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("{shown} ⌄"), look::regular(12.0), look::text());
        let presp = ui.interact(pr, ui.scope_id().with("fx-setting"), Sense::click());
        egui::Popup::menu(&presp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            if ui.button("Copy Setting").clicked() {
                action = Some(1);
                ui.close();
            }
            if ui.add_enabled(app.mix.clipboard.as_ref().is_some_and(|c| c.0 == name), egui::Button::new("Paste Setting")).clicked() {
                action = Some(2);
                ui.close();
            }
            if ui.button("Reset Setting").clicked() {
                reset = true;
                ui.close();
            }
            ui.separator();
            ui.horizontal(|ui| {
                let typed = app.mix.save_as.get_or_insert_with(String::new);
                ui.add(egui::TextEdit::singleline(typed).hint_text("Setting name").desired_width(110.0));
                if ui.add_enabled(!typed.trim().is_empty(), egui::Button::new("Save Setting As…")).clicked() {
                    save = Some(typed.trim().to_string());
                    ui.close();
                }
            });
            let list = saved_settings(&name);
            if !list.is_empty() {
                ui.separator();
                for (n, path) in list {
                    if ui.add(egui::Button::selectable(n == slot.preset, &n)).clicked() {
                        load = Some((n, path));
                        ui.close();
                    }
                }
            }
        });
        if let Some(a) = header_buttons(ui, &p, hr, "fx", comparing) {
            action = Some(a);
        }
        // View: Logic's editor (where Studio has one for the plug-in) or Controls
        let has_editor = crate::fxviews::has_editor(&name);
        let view_id = ui.scope_id().with(("fx-view", s, k));
        let mut editor = has_editor && ui.data(|d| d.get_temp::<bool>(view_id)).unwrap_or(true);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("View:").color(look::text_dim()));
            if has_editor {
                if ui.add(egui::Button::selectable(editor, "Editor")).clicked() {
                    editor = true;
                }
                if ui.add(egui::Button::selectable(!editor, "Controls")).clicked() {
                    editor = false;
                }
                ui.data_mut(|d| d.insert_temp(view_id, editor));
            } else {
                ui.label("Controls");
            }
            if comparing {
                ui.label(egui::RichText::new("Compare: the setting as it was").color(look::text_dim()));
            }
        });
        ui.add_space(4.0);
        if editor {
            let live = app.engine.lock().map(|e| {
                let strip = crate::mixer::fx_strip(s);
                let mut curves = std::collections::HashMap::new();
                let mut readouts = Vec::new();
                for c in crate::fxviews::curves_for(&name) {
                    let (r, v) = e.effect_readouts(strip, k, c);
                    readouts = r;
                    curves.insert(c.to_string(), v);
                }
                if readouts.is_empty() {
                    readouts = e.effect_readouts(strip, k, "").0;
                }
                crate::fxviews::Live { readouts, curves }
            }).unwrap_or_default();
            let (more, act) = crate::fxviews::editor(ui, &name, &params, &values, &live, (s, k));
            set.extend(more);
            fx_action = act;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        } else {
            set.extend(controls(ui, &params, &values, "fx"));
        }
    });
    let base = slot.params.clone();
    if let Some(a) = fx_action.take() {
        set.extend(crate::fxviews::run_action(app, (s, k), &name, a));
    }
    if toggle_bypass {
        crate::mixstate::checkpoint(app, "Bypass Plug-in");
        crate::mixstate::edit_inserts(app, s, |v| {
            if let Some(u) = v.get_mut(k) {
                u.bypass = !u.bypass;
            }
        });
        app.load_engine();
    }
    match action {
        Some(1) => app.mix.clipboard = Some((name.clone(), floats_of(&params, &values, &base))),
        Some(2) => {
            if let Some((_, f)) = app.mix.clipboard.clone().filter(|c| c.0 == name) {
                apply_floats(app, s, k, f, None);
            }
        }
        Some(0) => {
            // Compare: swap between the edited setting and the one before the first edit
            let now = floats_of(&params, &values, &base);
            if let Some(kept) = app.mix.compare.remove(&(s, k)) {
                app.mix.compare_back.insert((s, k), now);
                apply_floats(app, s, k, kept, None);
            } else if let Some(back) = app.mix.compare_back.remove(&(s, k)) {
                app.mix.compare.insert((s, k), now);
                apply_floats(app, s, k, back, None);
            }
        }
        _ => {}
    }
    if reset {
        let defaults: Vec<f32> = params.iter().map(|p| p.default).collect();
        apply_floats(app, s, k, floats_of(&params, &defaults, &base), Some("#default".into()));
    }
    if let Some(n) = save {
        if write_setting(&name, &n, &params, &values).is_some() {
            crate::mixstate::edit_inserts(app, s, |v| {
                if let Some(u) = v.get_mut(k) {
                    u.preset = n.clone();
                }
            });
            app.mix.save_as = None;
            app.tell(format!("Saved the setting “{n}”"));
        }
    }
    if let Some((n, path)) = load {
        load_setting(app, s, k, &n, &path);
    }
    if step != 0 {
        step_setting(app, s, k, step);
    }
    if !set.is_empty() {
        // the first edit keeps the setting as it was, for Compare
        app.mix.compare.entry((s, k)).or_insert_with(|| floats_of(&params, &values, &base));
        app.mix.compare_back.remove(&(s, k));
        let mut vals = values.clone();
        // the engine's state after the edit: an edit can move other stored values (a custom
        // view's selected-slice controls, a Pedalboard slot's new pedal), so it is read back
        let after = app.engine.lock().ok().and_then(|mut e| {
            for (id, v) in &set {
                e.set_effect_param(crate::mixer::fx_strip(s), k, *id, *v);
            }
            e.effect_params(crate::mixer::fx_strip(s), k)
        });
        let f = if let Some((p2, v2)) = after {
            floats_of(&p2, &v2, &base)
        } else {
            // (not running: a stand-in built from the slot's settings takes the edit)
            let mut fx = crate::units::effect(&name, crate::audio::RATE as f32);
            if let Some(fx) = fx.as_mut() {
                if !base.is_empty() {
                    fx.load_floats(&base);
                }
                for (id, v) in &set {
                    fx.set(*id, *v);
                }
                let p2 = fx.params().to_vec();
                let v2: Vec<f32> = p2.iter().map(|p| fx.get(p.id)).collect();
                floats_of(&p2, &v2, &base)
            } else {
                for (id, v) in set {
                    if let Some(i) = params.iter().position(|p| p.id == id) {
                        vals[i] = v;
                    }
                }
                floats_of(&params, &vals, &base)
            }
        };
        crate::mixstate::edit_inserts(app, s, |v| {
            if let Some(u) = v.get_mut(k) {
                u.params = f;
            }
        });
    }
    if !open {
        app.mix.fx_window = None;
    }
}
