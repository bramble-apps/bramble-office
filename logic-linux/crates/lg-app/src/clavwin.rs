//! The Vintage Clav window, laid out as Logic's: the control bar with the Model pop-up and the
//! Main / Effects / Details buttons; Main's central display (the Pickup Position display with
//! the two pickups as dark bars, the Stereo Spread ring, Brilliance and Decay); the lower row of
//! filter switches, the AB and CD pickup switches, Volume and the Damper slider.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::clav;

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

/// The Pickup Position display: each pickup is a dark bar across the strings; its middle handle
/// moves it along them, the handle at either end changes its angle.
fn pickups(ui: &mut egui::Ui, pane: &mut Pane) {
    let (r, _) = ui.allocate_exact_size(vec2(300.0, 150.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, look::lcd_bg());
    for k in 0..12 {
        let y = r.min.y + 14.0 + k as f32 * (r.height() - 28.0) / 11.0;
        p.line_segment([pos2(r.min.x + 8.0, y), pos2(r.max.x - 8.0, y)], Stroke::new(0.8, Color32::from_gray(0x70)));
    }
    // the bridge at the right: positions run 0 (bridge) .. 0.5 (the middle of the string)
    let x_of = |pos: f32| r.max.x - 12.0 - pos / 0.5 * (r.width() - 24.0);
    let pos_of = |x: f32| ((r.max.x - 12.0 - x) / (r.width() - 24.0) * 0.5).clamp(0.02, 0.48);
    let lean = 46.0;
    for (name, pid, aid, col) in [("Upper", 24u32, 25u32, Color32::from_rgb(0x30, 0x30, 0x38)), ("Lower", 26, 27, Color32::from_rgb(0x22, 0x22, 0x28))] {
        let (pos, ang) = (pane.get(pid), pane.get(aid));
        let cx = x_of(pos);
        let top = pos2(cx - ang * lean, r.min.y + 10.0);
        let bot = pos2(cx + ang * lean, r.max.y - 10.0);
        p.line_segment([top, bot], Stroke::new(9.0, col));
        p.line_segment([top, bot], Stroke::new(1.0, Color32::from_gray(0x90)));
        p.text(pos2(cx, r.center().y) + vec2(8.0, -8.0), Align2::LEFT_CENTER, format!("{name} {:+.0}°", ang * 45.0), look::regular(9.5), look::lcd_text());
        for (k, at) in [(0, pos2(cx, r.center().y)), (1, top), (2, bot)] {
            let hr = Rect::from_center_size(at, vec2(14.0, 14.0));
            let resp = ui.interact(hr, egui::Id::unique(("clav-pickup", pane.t, pid, k)), Sense::drag()).on_hover_text(if k == 0 { format!("{name} pickup: position") } else { format!("{name} pickup: angle") });
            p.circle_filled(at, 4.5, if resp.hovered() || resp.dragged() { look::accent() } else { Color32::from_gray(0xd0) });
            if resp.dragged() {
                let dx = resp.drag_delta().x;
                if k == 0 {
                    pane.put(pid, pos_of(cx + dx));
                } else {
                    let sign = if k == 1 { -1.0 } else { 1.0 };
                    pane.put(aid, (ang + sign * dx / lean).clamp(-1.0, 1.0));
                }
            }
        }
    }
}

/// The Stereo Spread ring: drag up or down in its upper half for Key, in its lower half for
/// Pickup. The key range shows in orange, the pickups' pan positions as red lines.
fn spread(ui: &mut egui::Ui, pane: &mut Pane) {
    let (r, _) = ui.allocate_exact_size(vec2(110.0, 150.0), Sense::hover());
    let p = ui.painter().clone();
    let c = pos2(r.center().x, r.min.y + 62.0);
    let rad = 44.0;
    let (key, pick) = (pane.get(28), pane.get(29));
    p.circle_stroke(c, rad, Stroke::new(5.0, look::lcd_bg()));
    let arc = |a0: f32, a1: f32, col: Color32, w: f32| {
        let pts: Vec<egui::Pos2> = (0..=24).map(|k| {
            let a = a0 + (a1 - a0) * k as f32 / 24.0;
            c + vec2(a.sin(), -a.cos()) * rad
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(w, col)));
    };
    // (angles from straight up; the key range opens to both sides)
    if key > 0.0 {
        arc(-key * 1.4, key * 1.4, Color32::from_rgb(0xe8, 0x92, 0x2a), 5.0);
    }
    for side in [-1.0f32, 1.0] {
        let a = std::f32::consts::PI + side * pick * 1.2;
        p.line_segment([c + vec2(a.sin(), -a.cos()) * (rad - 9.0), c + vec2(a.sin(), -a.cos()) * (rad + 5.0)], Stroke::new(2.0, Color32::from_rgb(0xe0, 0x40, 0x40)));
    }
    for (k, (id, label, v)) in [(28u32, "Key", key), (29, "Pickup", pick)].into_iter().enumerate() {
        let half = if k == 0 { Rect::from_min_max(pos2(c.x - rad, c.y - rad), pos2(c.x + rad, c.y)) } else { Rect::from_min_max(pos2(c.x - rad, c.y), pos2(c.x + rad, c.y + rad)) };
        let resp = ui.interact(half, egui::Id::unique(("clav-spread", pane.t, id)), Sense::drag()).on_hover_text(format!("Stereo Spread {label}: {:.0} %", v * 100.0));
        if resp.dragged() {
            pane.put(id, (v - resp.drag_delta().y / 120.0).clamp(0.0, 1.0));
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        p.text(pos2(c.x, c.y + if k == 0 { -14.0 } else { 14.0 }), Align2::CENTER_CENTER, format!("{label} {:.0}", v * 100.0), look::regular(10.0), look::text_dim());
    }
    p.text(pos2(c.x, r.max.y - 8.0), Align2::CENTER_CENTER, "Stereo Spread", look::regular(11.0), look::text_dim());
}

fn main_page(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal(|ui| {
        pickups(ui, pane);
        spread(ui, pane);
        ui.vertical(|ui| {
            pane.knob(ui, 31, "Brilliance");
            pane.knob(ui, 5, "Decay");
        });
    });
}

fn effects_page(ui: &mut egui::Ui, pane: &mut Pane, tempo: f32) {
    ui.horizontal(|ui| {
        pane.switch(ui, 33, "Master");
        ui.add_space(12.0);
        // the chain: the three effects in their order with the compressor at its place
        let order = (pane.get(36).round() as usize).min(5);
        let comp = (pane.get(37).round() as usize).min(3);
        let perms: [[&str; 3]; 6] = [["Distortion", "Modulation", "Wah"], ["Distortion", "Wah", "Modulation"], ["Modulation", "Distortion", "Wah"], ["Modulation", "Wah", "Distortion"], ["Wah", "Distortion", "Modulation"], ["Wah", "Modulation", "Distortion"]];
        let mut names: Vec<&str> = perms[order].to_vec();
        names.insert(comp, "Compressor");
        if let Some((from, to)) = keyswin::reorder(ui, egui::Id::unique(("clav-chain", pane.t)), &names) {
            let x = names.remove(from);
            names.insert(to, x);
            let at = names.iter().position(|n| *n == "Compressor").unwrap_or(0);
            let rest: Vec<&str> = names.iter().copied().filter(|n| *n != "Compressor").collect();
            if let Some(k) = perms.iter().position(|o| o[..] == rest[..]) {
                pane.put(36, k as f32);
            }
            pane.put(37, at as f32);
        }
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "Compressor", |ui| {
            pane.switch(ui, 34, "On");
            pane.knob(ui, 35, "Ratio");
        });
        group(ui, "Distortion", |ui| {
            pane.switch(ui, 13, "On");
            pane.knobs(ui, &[(14, "Gain"), (38, "Tone")]);
        });
        group(ui, "Modulation", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 15, "On");
                pane.menu(ui, 30, "Mode");
            });
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    pane.knob(ui, 16, "Rate");
                    // synced (phaser and flanger): bar and beat values
                    if pane.on(32) && pane.get(30).round() as i32 != 2 {
                        ui.label(egui::RichText::new(lg_synth::ep::sync_rate(pane.get(16), tempo).1).size(10.5).color(look::accent()));
                    }
                });
                pane.knob(ui, 17, "Intensity");
            });
            pane.switch(ui, 32, "Sync");
        });
        group(ui, "Wah", |ui| {
            ui.horizontal(|ui| {
                let on = pane.on(11);
                if keyswin::toggle(ui, on, "On") {
                    pane.put(11, if on { 0.0 } else { 1.0 });
                }
                pane.menu(ui, 39, "Mode");
            });
            pane.knobs(ui, &[(12, "Range"), (40, "Envelope")]);
        });
    });
}

fn details_page(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            group(ui, "Excite and Click", |ui| {
                for (id, n) in [(41u32, "Excite Shape"), (10, "Click Intensity"), (42, "Click Random"), (43, "Click Velocity")] {
                    pane.slider(ui, id, n);
                }
                pane.seg(ui, 44, "Velocity");
            });
            ui.add_space(4.0);
            group(ui, "Pitch", |ui| {
                for (id, n) in [(49u32, "Tune"), (50, "Stretch Tuning"), (51, "Warmth"), (21, "Pitch Wheel"), (52, "Pitch Pressure")] {
                    pane.slider(ui, id, n);
                }
            });
        });
        ui.vertical(|ui| {
            group(ui, "String", |ui| {
                for (id, n) in [(6u32, "Release"), (45, "Damping"), (46, "Inharmonicity"), (8, "Stiffness"), (47, "Tension Mod"), (48, "Pitch Fall")] {
                    pane.slider(ui, id, n);
                }
            });
            ui.add_space(4.0);
            group(ui, "Misc", |ui| {
                // Voices: a number, or the single-voice Mono and Legato settings
                let mode = pane.get(53).round() as usize;
                let voices = pane.get(22).round() as usize;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Voices").size(11.0).color(look::text_dim()));
                    let shown = match mode {
                        1 => "Mono".to_string(),
                        2 => "Legato".to_string(),
                        _ => format!("{voices}"),
                    };
                    ui.menu_button(format!("{shown} ⌄"), |ui| {
                        for (k, n) in [(1usize, "Mono"), (2, "Legato")] {
                            if ui.add(egui::Button::selectable(mode == k, n)).clicked() {
                                pane.put(53, k as f32);
                                ui.close();
                            }
                        }
                        ui.separator();
                        for v in [2usize, 3, 4, 6, 8, 10, 12, 16, 24, 32] {
                            if ui.add(egui::Button::selectable(mode == 0 && voices == v, format!("{v}"))).clicked() {
                                pane.put(53, 0.0);
                                pane.put(22, v as f32);
                                ui.close();
                            }
                        }
                    });
                });
                pane.menu(ui, 54, "Velocity Curve");
                pane.source_menu(ui, 55, "Damper Control", true);
                pane.source_menu(ui, 56, "Wah Control", true);
                pane.slider(ui, 57, "Wah Pedal Position");
                pane.slider(ui, 9, "Velocity Sensitivity");
            });
        });
    });
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let tempo = app.engine.lock().map(|e| e.tempo as f32).unwrap_or(120.0);
    let tab_id = egui::Id::unique(("clav-tab", t));
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let mut open = true;
    let resp = egui::Window::new("Vintage Clav").id(egui::Id::unique(("clav-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(640.0, 430.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Vintage Clav");
        ui.horizontal(|ui| {
            pane.menu(ui, 23, "Model");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut shown = 2 - tab.min(2);
                keyswin::tabs(ui, &mut shown, &["Details", "Effects", "Main"]);
                tab = 2 - shown;
            });
        });
        ui.add_space(4.0);
        match tab {
            1 => effects_page(ui, &mut pane, tempo),
            2 => details_page(ui, &mut pane),
            _ => main_page(ui, &mut pane),
        }
        ui.add_space(6.0);
        // the lower row: filter switches, pickup switches, Volume, Damper
        ui.horizontal(|ui| {
            group(ui, "Filter", |ui| {
                ui.horizontal(|ui| {
                    for (id, n) in [(1u32, "Brilliant"), (2, "Treble"), (3, "Medium"), (4, "Soft")] {
                        let on = pane.on(id);
                        if ui.add(egui::Button::selectable(on, egui::RichText::new(n).size(11.0))).clicked() {
                            pane.put(id, if on { 0.0 } else { 1.0 });
                        }
                    }
                });
            });
            group(ui, "Pickups", |ui| {
                // AB and CD: the wiring (A/C lower, B/C upper, A/D lower − upper, B/D both)
                let w = pane.get(0).round() as usize;
                let (ab, cd) = (w & 1, w >> 1);
                ui.horizontal(|ui| {
                    for (bit, cur, names) in [(0usize, ab, ["A", "B"]), (1, cd, ["C", "D"])] {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (k, n) in names.iter().enumerate() {
                            if ui.add(egui::Button::selectable(cur == k, egui::RichText::new(*n).size(11.0))).clicked() {
                                let (a, c) = if bit == 0 { (k, cd) } else { (ab, k) };
                                pane.put(0, (a + 2 * c) as f32);
                            }
                        }
                        ui.add_space(6.0);
                    }
                    ui.label(egui::RichText::new(clav::WIRING[w.min(3)]).size(10.5).color(look::text_dim()));
                });
            });
            pane.knob(ui, 20, "Volume");
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Damper").size(11.0).color(look::text_dim()));
                let mut v = pane.get(7);
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                    pane.put(7, v);
                }
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(tab_id, tab));
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Vintage Clav Setting", open);
}
