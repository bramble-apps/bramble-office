//! A plug-in's window, as Logic's: the header (bypass, the setting pop-up with ‹ ›, Compare,
//! Copy / Paste, View: Controls) and the Controls view: one row per parameter (name, a slider
//! or a pop-up or a switch, the value), in the plug-in's own order. Opened by a click on the
//! instrument slot of a channel strip.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

fn fmt(p: &lg_dsp::Param, v: f32) -> String {
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

pub fn window(app: &mut App, ctx: &egui::Context) {
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
        let bypass = Rect::from_center_size(pos2(hr.min.x + 18.0, hr.center().y), vec2(22.0, 22.0));
        let on = !app.plugin_bypass.contains(&t);
        p.circle_stroke(bypass.center(), 7.5, Stroke::new(1.6, if on { look::accent() } else { look::text_dim() }));
        p.vline(bypass.center().x, bypass.center().y - 9.0..=bypass.center().y - 2.0, Stroke::new(1.6, if on { look::accent() } else { look::text_dim() }));
        if ui.interact(bypass, ui.scope_id().with("pw-bypass"), Sense::click()).on_hover_text("Bypass").clicked() {
            if on {
                app.plugin_bypass.insert(t);
            } else {
                app.plugin_bypass.remove(&t);
            }
        }
        p.text(pos2(hr.min.x + 40.0, hr.center().y), Align2::LEFT_CENTER, &patch, look::semibold(12.5), look::text());
        // Copy / Paste of the settings, Compare (the settings as they were when opened)
        let mut x = hr.max.x - 8.0;
        for (label, act) in [("Paste", 2), ("Copy", 1), ("Compare", 0)] {
            let w = p.layout_no_wrap(label.into(), look::regular(12.0), look::text()).size().x + 16.0;
            let r = Rect::from_min_max(pos2(x - w, hr.min.y + 4.0), pos2(x, hr.max.y - 4.0));
            x -= w + 6.0;
            let resp = ui.interact(r, ui.scope_id().with(("pw", act)), Sense::click());
            p.rect_filled(r, 5.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
            p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(12.0), look::text());
            if resp.clicked() {
                match act {
                    1 => app.plugin_clipboard = Some(values.clone()),
                    2 => {
                        if let Some(v) = app.plugin_clipboard.clone() {
                            for (prm, val) in params.iter().zip(v) {
                                set.push((prm.id, val));
                            }
                        }
                    }
                    _ => {
                        // swap the current settings with the ones kept at the last Compare
                        let keep = app.plugin_compare.insert(t, values.clone());
                        if let Some(v) = keep {
                            for (prm, val) in params.iter().zip(v) {
                                set.push((prm.id, val));
                            }
                        }
                    }
                }
            }
        }
        ui.add_space(6.0);
        // the Controls view
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (prm, &v) in params.iter().zip(values.iter()) {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                let p = ui.painter().clone();
                let name_w = (row.width() * 0.34).min(200.0);
                p.text(pos2(row.min.x + name_w, row.center().y), Align2::RIGHT_CENTER, prm.name, look::regular(12.0), look::text_dim());
                let ctl = Rect::from_min_max(pos2(row.min.x + name_w + 12.0, row.min.y + 3.0), pos2(row.max.x - 96.0, row.max.y - 3.0));
                let id = ui.scope_id().with(("pw-param", prm.id));
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
        });
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
