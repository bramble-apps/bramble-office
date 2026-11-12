//! Plug-in windows, as Logic's: the header (bypass, the setting pop-up with ‹ ›, Compare,
//! Copy / Paste, View: Controls) and the Controls view: one row per parameter (name, a slider
//! or a pop-up or a switch, the value), in the plug-in's own order. Instruments open from the
//! instrument slot, effects from their Audio FX slot (every lg-fx effect, on tracks, auxes
//! and the Stereo Out). An effect's settings can be saved and recalled (Save Setting As…, the
//! list in the pop-up, ‹ › and Next / Previous Plug-in Setting), reset, copied and pasted.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};

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
/// `texts`: the plug-in's own text for a value when it has one (hosted plug-ins).
pub(crate) fn controls(ui: &mut egui::Ui, params: &[lg_dsp::Param], values: &[f32], texts: &[Option<String>], salt: &str) -> Vec<(u32, f32)> {
    let mut set = Vec::new();
    egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(ui, |ui| {
        set = control_rows(ui, params, values, texts, salt);
    });
    set
}

/// The Controls view's rows without the scroll area (custom views embed them).
pub(crate) fn control_rows(ui: &mut egui::Ui, params: &[lg_dsp::Param], values: &[f32], texts: &[Option<String>], salt: &str) -> Vec<(u32, f32)> {
    let mut set = Vec::new();
    {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (n, (prm, &v)) in params.iter().zip(values.iter()).enumerate() {
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
            let shown = texts.get(n).cloned().flatten().unwrap_or_else(|| fmt(prm, v));
            p.text(pos2(row.max.x - 4.0, row.center().y), Align2::RIGHT_CENTER, shown, look::regular(12.0), look::text());
        }
    }
    set
}

/// The header's bypass button (a power symbol); true when clicked.
fn bypass_button(ui: &mut egui::Ui, p: &egui::Painter, hr: Rect, on: bool, salt: &str) -> bool {
    let bypass = Rect::from_center_size(pos2(hr.min.x + 18.0, hr.center().y), vec2(22.0, 22.0));
    crate::glyphs::paint(p, bypass, crate::glyphs::ph::POWER, 19.0, if on { look::accent() } else { look::text_dim() });
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

/// Link mode Multi: the open plug-in windows follow the selected track (its instrument, and the
/// plug-in in the same insert row of its strip).
fn follow_link(app: &mut App, ctx: &egui::Context) {
    if crate::plughead::link_mode(ctx) != 2 {
        return;
    }
    let Some(sel) = app.selected_track else { return };
    if let Some(t) = app.plugin_window
        && t != sel
        && app.instrument_for(sel).is_some()
    {
        app.plugin_window = Some(sel);
    }
    if let Some((s, k)) = app.mix.fx_window
        && s != sel
        && s < crate::mixer::AUX
        && crate::mixstate::inserts(app, sel).get(k).is_some_and(|u| !u.name.is_empty())
    {
        app.mix.fx_window = Some((sel, k));
    }
}

/// Checks (headless): LG_FX="Compressor" adds that effect to the first track's Audio FX and
/// opens its window; LG_FX_SIDE=1 also picks Bus 1 as its side chain.
fn test_hook(app: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Ok(name) = std::env::var("LG_FX") else { return };
    if app.project.is_none() || DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    crate::mixstate::edit_inserts(app, 0, |v| v.push(crate::mixstate::new_plugin(&name)));
    app.load_engine();
    let k = crate::mixstate::inserts(app, 0).len().saturating_sub(1);
    app.mix.fx_window = Some((0, k));
    if std::env::var_os("LG_FX_SIDE").is_some() {
        app.side.by_slot.insert((0, k), crate::sidechain::Choice::Bus(0));
        crate::sidechain::sync(app);
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    test_hook(app);
    follow_link(app, ctx);
    fx_window(app, ctx);
    crate::mixer::group_window(app, ctx);
    if crate::instwin::window(app, ctx) {
        return;
    }
    let Some(t) = app.plugin_window else { return };
    let Some((params, values)) = app.engine.lock().ok().and_then(|e| e.instrument_params(t)) else {
        app.plugin_window = None;
        app.tell("This track plays through the SoundFont: no plug-in window");
        return;
    };
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let patch = app.instrument_for(t).unwrap_or_default();
    let texts = crate::hosting::texts(app, crate::hosting::Slot::Inst(t), &params, &values);
    let mut open = true;
    let mut set: Vec<(u32, f32)> = Vec::new();
    let can_undo = !app.inst.undo.is_empty();
    let can_redo = !app.inst.redo.is_empty();
    let mut hist: Option<bool> = None;
    let scale = crate::plughead::scale(ctx, &patch);
    let wresp = egui::Window::new(track.clone()).id(egui::Id::unique("plugin-window")).open(&mut open).collapsible(false).resizable(true).default_size(vec2(560.0, 520.0)).show(ctx, |ui| {
        let on = !app.plugin_bypass.contains(&t);
        let comparing = app.plugin_compare.contains_key(&t);
        let head = crate::plughead::Head {
            id: egui::Id::unique(("inst-head", t)),
            plugin: &patch,
            bypassed: !on,
            setting: "Default Preset",
            comparing,
            can_compare: true,
            can_paste: app.plugin_clipboard.as_ref().is_some_and(|v| v.len() == params.len()),
            can_undo,
            can_redo,
            side_chain: false,
            editor: None,
        };
        let out = crate::plughead::header(ui, &head);
        use crate::plughead::Act;
        match out.act {
            Some(Act::Bypass) => {
                if on {
                    app.plugin_bypass.insert(t);
                } else {
                    app.plugin_bypass.remove(&t);
                }
            }
            Some(Act::Copy) => app.plugin_clipboard = Some(values.clone()),
            Some(Act::Paste) => {
                if let Some(v) = app.plugin_clipboard.clone() {
                    for (prm, val) in params.iter().zip(v) {
                        set.push((prm.id, val));
                    }
                }
            }
            Some(Act::Compare) => {
                // swap the current settings with the ones kept at the last Compare
                let keep = app.plugin_compare.insert(t, values.clone());
                if let Some(v) = keep {
                    for (prm, val) in params.iter().zip(v) {
                        set.push((prm.id, val));
                    }
                }
            }
            Some(Act::Undo) => hist = Some(false),
            Some(Act::Redo) => hist = Some(true),
            _ => {}
        }
        egui::Popup::menu(&out.setting).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            if ui.button("Copy Setting").clicked() {
                app.plugin_clipboard = Some(values.clone());
                ui.close();
            }
            if ui.button("Recall Default").clicked() {
                set.extend(params.iter().map(|p| (p.id, p.default)));
                ui.close();
            }
        });
        ui.add_space(6.0);
        crate::hosting::view_row(app, ui, crate::hosting::Slot::Inst(t), &format!("{track} — {patch}"));
        let room = (ui.available_height() - crate::plughead::FOOTER - 6.0).max(120.0);
        egui::ScrollArea::vertical().id_salt(("inst-body", t)).max_height(room).auto_shrink([false, true]).show(ui, |ui| {
            set.extend(controls(ui, &params, &values, &texts, "inst"));
        });
        crate::plughead::footer(ui, &patch, None);
    });
    if let Some(r) = &wresp {
        crate::plughead::apply_scale(ctx, r.response.layer_id, r.response.rect.min, scale);
        app.inst.rect = Some(egui::Rect::from_min_size(r.response.rect.min, r.response.rect.size() * scale));
    }
    match hist {
        Some(true) => {
            crate::instwin::redo(app);
        }
        Some(false) => {
            crate::instwin::undo(app);
        }
        None => {}
    }
    if !set.is_empty() {
        crate::instwin::checkpoint_once(app, ctx, t, &format!("{patch} Setting"));
        if let Ok(mut e) = app.engine.lock() {
            for (id, v) in set {
                e.set_instrument_param(t, id, v);
            }
            crate::hosting::mark_changed(crate::hosting::Slot::Inst(t));
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
    if lg_host::parse_studio_name(&u.name).is_some() {
        // (a hosted plug-in the engine couldn't load: no window)
        return None;
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
pub(crate) fn read_setting(path: &std::path::Path) -> Option<Vec<(u32, f32)>> {
    let t = std::fs::read_to_string(path).ok()?;
    Some(t.lines().filter_map(|l| {
        let mut w = l.split_whitespace();
        Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
    }).collect())
}

pub(crate) fn write_setting(plugin: &str, name: &str, params: &[lg_dsp::Param], values: &[f32]) -> Option<()> {
    let dir = settings_dir(plugin)?;
    std::fs::create_dir_all(&dir).ok()?;
    let body: String = params.iter().zip(values).map(|(p, v)| format!("{} {}\n", p.id, v)).collect();
    std::fs::write(dir.join(format!("{}.txt", name.replace('/', "-"))), body).ok()
}

fn load_setting(app: &mut App, s: usize, k: usize, name: &str, path: &std::path::Path) {
    // a hosted plug-in's preset carries its whole state
    if let Some(f) = crate::hosting::preset_floats(path) {
        apply_floats(app, s, k, f, Some(name.to_string()));
        return;
    }
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

type History = (Vec<Vec<f32>>, Vec<Vec<f32>>);

/// Whether an effect has a side chain input (its header shows the Side Chain pop-up).
pub fn has_side_chain(name: &str) -> bool {
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<String, bool>> = Default::default();
    }
    KNOWN.with(|k| *k.borrow_mut().entry(name.to_string()).or_insert_with(|| crate::units::effect(name, crate::audio::RATE as f32).is_some_and(|e| e.has_side_chain())))
}

fn history_id(s: usize, k: usize) -> egui::Id {
    egui::Id::unique(("fx-undo", s, k))
}

/// The window's plug-in undo / redo steps.
fn fx_history_len(ctx: &egui::Context, s: usize, k: usize) -> (usize, usize) {
    ctx.data(|d| d.get_temp::<History>(history_id(s, k))).map(|h| (h.0.len(), h.1.len())).unwrap_or((0, 0))
}

fn push_history(ctx: &egui::Context, s: usize, k: usize, before: Vec<f32>) {
    ctx.data_mut(|d| {
        let h = d.get_temp_mut_or_default::<History>(history_id(s, k));
        h.0.push(before);
        if h.0.len() > 100 {
            h.0.remove(0);
        }
        h.1.clear();
    });
}

/// Plug-in undo (or redo) of the effect window's slot: its whole setting as it was.
fn fx_undo(app: &mut App, ctx: &egui::Context, s: usize, k: usize, redo: bool) -> bool {
    let Some((name, params, values)) = effect_state(app, s, k) else { return false };
    let base = crate::mixstate::inserts(app, s).get(k).map(|u| u.params.clone()).unwrap_or_default();
    let now = floats_of(&params, &values, &base);
    let got = ctx.data_mut(|d| {
        let h = d.get_temp_mut_or_default::<History>(history_id(s, k));
        let (from, to) = if redo { (&mut h.1, &mut h.0) } else { (&mut h.0, &mut h.1) };
        let v = from.pop()?;
        to.push(now);
        Some(v)
    });
    let Some(v) = got else { return false };
    apply_floats(app, s, k, v, None);
    app.tell(format!("{} {name} Setting", if redo { "Redo" } else { "Undo" }));
    true
}

/// ⌘Z / ⇧⌘Z with the pointer over the effect window: its plug-in undo.
pub fn fx_key(app: &mut App, ctx: &egui::Context, key: egui::Key, m: egui::Modifiers) -> bool {
    let Some((s, k)) = app.mix.fx_window else { return false };
    if !(m.command && key == egui::Key::Z) || ctx.egui_wants_keyboard_input() {
        return false;
    }
    let r: Option<egui::Rect> = ctx.data(|d| d.get_temp(egui::Id::unique("fx-window-rect")));
    if !r.is_some_and(|r| ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p))) {
        return false;
    }
    fx_undo(app, ctx, s, k, m.shift)
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
    let hslot = crate::hosting::Slot::Fx(s, k);
    let hosted = crate::hosting::hosted_info(app, hslot).is_some();
    let texts = crate::hosting::texts(app, hslot, &params, &values);
    let mut fx_action: Option<crate::fxviews::Action> = None;
    let size = crate::fxviews::default_size(&name);
    let has_editor = crate::fxviews::has_editor(&name) && !hosted;
    let side_ok = !hosted && has_side_chain(&name);
    let view_id = egui::Id::unique(("fx-view", s, k));
    let mut editor = has_editor && ctx.data(|d| d.get_temp::<bool>(view_id)).unwrap_or(true);
    let (undo_n, redo_n) = fx_history_len(ctx, s, k);
    let can_compare = comparing || app.mix.compare_back.contains_key(&(s, k));
    let can_paste = app.mix.clipboard.as_ref().is_some_and(|c| c.0 == name);
    let mut hist: Option<bool> = None;
    let win_id = egui::Id::unique("fx-plugin-window");
    let scale = crate::plughead::scale(ctx, &name);
    let wresp = egui::Window::new(strip.clone()).id(win_id).open(&mut open).collapsible(false).resizable(true).default_size(size).show(ctx, |ui| {
        let shown = if slot.preset.is_empty() || slot.preset == "#default" { "Default Preset".to_string() } else { slot.preset.trim_end_matches(".pst").to_string() };
        let head = crate::plughead::Head {
            id: egui::Id::unique(("fx-head", s, k)),
            plugin: &name,
            bypassed: slot.bypass,
            setting: &shown,
            comparing,
            can_compare,
            can_paste,
            can_undo: undo_n > 0,
            can_redo: redo_n > 0,
            side_chain: side_ok,
            editor: has_editor.then_some(editor),
        };
        let out = crate::plughead::header(ui, &head);
        if let Some(r) = out.side_chain {
            crate::sidechain::fx_popup(app, ui, r, s, k, if name == "Compressor" { "Internal" } else { "None" });
        }
        match out.act {
            Some(crate::plughead::Act::Bypass) => toggle_bypass = true,
            Some(crate::plughead::Act::Prev) => step = -1,
            Some(crate::plughead::Act::Next) => step = 1,
            Some(crate::plughead::Act::Compare) => action = Some(0),
            Some(crate::plughead::Act::Copy) => action = Some(1),
            Some(crate::plughead::Act::Paste) => action = Some(2),
            Some(crate::plughead::Act::Undo) => hist = Some(false),
            Some(crate::plughead::Act::Redo) => hist = Some(true),
            Some(crate::plughead::Act::Editor(e)) => {
                editor = e;
                ui.ctx().data_mut(|d| d.insert_temp(view_id, e));
            }
            None => {}
        }
        egui::Popup::menu(&out.setting).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            if ui.button("Copy Setting").clicked() {
                action = Some(1);
                ui.close();
            }
            if ui.add_enabled(can_paste, egui::Button::new("Paste Setting")).clicked() {
                action = Some(2);
                ui.close();
            }
            if ui.button("Reset Setting").clicked() {
                reset = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Next Setting").clicked() {
                step = 1;
                ui.close();
            }
            if ui.button("Previous Setting").clicked() {
                step = -1;
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
        ui.add_space(4.0);
        // the body scrolls; the footer stays at the bottom
        let room = (ui.available_height() - crate::plughead::FOOTER - 6.0).max(120.0);
        egui::ScrollArea::vertical().id_salt(("fx-body", s, k)).max_height(room).auto_shrink([false, true]).show(ui, |ui| {
        if hosted {
            // (a hosted plug-in: its own editor beside the Controls view)
            crate::hosting::view_row(app, ui, hslot, &format!("{strip} — {name}"));
            ui.add_space(4.0);
            set.extend(controls(ui, &params, &values, &texts, "fx"));
        } else {
            if comparing {
                ui.label(egui::RichText::new("Compare: the setting as it was").color(look::text_dim()));
            }
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
                set.extend(controls(ui, &params, &values, &texts, "fx"));
            }
        }
        });
        crate::plughead::footer(ui, &name, None);
    });
    if let Some(r) = &wresp {
        crate::plughead::apply_scale(ctx, r.response.layer_id, r.response.rect.min, scale);
        let rr = r.response.rect;
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("fx-window-rect"), egui::Rect::from_min_size(rr.min, rr.size() * scale)));
    }
    // (a hosted plug-in's own state, fresh, for Copy, Compare and Save Setting As…)
    if hosted && (matches!(action, Some(0) | Some(1)) || save.is_some()) {
        crate::hosting::sync_slot(app, hslot);
    }
    let base = if hosted { crate::mixstate::inserts(app, s).get(k).map(|u| u.params.clone()).unwrap_or_default() } else { slot.params.clone() };
    // plug-in undo: the setting before this frame's change goes on the window's history
    let before = floats_of(&params, &values, &base);
    let changes = !set.is_empty() || fx_action.is_some() || matches!(action, Some(2)) || reset || load.is_some() || step != 0;
    if changes {
        let gesture_id = egui::Id::unique(("fx-gesture", s, k));
        let in_gesture = ctx.data(|d| d.get_temp::<bool>(gesture_id)).unwrap_or(false);
        if !in_gesture {
            push_history(ctx, s, k, before.clone());
        }
        let down = ctx.input(|i| i.pointer.any_down());
        ctx.data_mut(|d| d.insert_temp(gesture_id, down));
    } else if !ctx.input(|i| i.pointer.any_down()) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("fx-gesture", s, k)), false));
    }
    if let Some(redo) = hist {
        fx_undo(app, ctx, s, k, redo);
    }
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
            if let (Some(extra), Some(dir)) = (crate::hosting::preset_extra(&name, &base), settings_dir(&name)) {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(dir.join(format!("{}.txt", n.replace('/', "-")))) {
                    let _ = f.write_all(extra.as_bytes());
                }
            }
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
