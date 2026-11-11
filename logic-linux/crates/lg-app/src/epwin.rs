//! The Vintage Electric Piano window, laid out as Logic's: the control bar with the Model pop-up
//! and the Effects / Details buttons, the main display (the effects, or the model's tone, playing
//! and pitch parameters), the central Bass Boost and Volume, and the extended parameters behind
//! the disclosure arrow at the lower left.

use egui::vec2;
use lg_synth::ep;

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

/// A rate knob that shows bar and beat values when its Sync is on.
fn rate(ui: &mut egui::Ui, pane: &mut Pane, id: u32, sync: u32, tempo: f32, whole_notes: bool) {
    ui.vertical(|ui| {
        pane.knob(ui, id, "Rate");
        if pane.on(sync) {
            // (the tremolo's synced value is a note length; the phaser's the nearest length to its rate)
            let hz = if whole_notes { tempo / 60.0 / (pane.get(id).max(0.03) * 4.0) } else { pane.get(id) };
            ui.label(egui::RichText::new(ep::sync_rate(hz, tempo).1).size(10.5).color(look::accent()));
        }
    });
}

fn effects(ui: &mut egui::Ui, pane: &mut Pane, tempo: f32) {
    ui.horizontal_top(|ui| {
        group(ui, "Drive", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 30, "On");
                pane.seg(ui, 38, "");
            });
            pane.knobs(ui, &[(13, "Gain"), (14, "Tone")]);
        });
        group(ui, "EQ", |ui| {
            pane.switch(ui, 29, "On");
            pane.knobs(ui, &[(12, "Bass"), (11, "Treble")]);
        });
        group(ui, "Chorus", |ui| {
            pane.switch(ui, 33, "On");
            pane.knobs(ui, &[(25, "Rate"), (21, "Intensity")]);
        });
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        group(ui, "Phaser", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 31, "On");
                pane.switch(ui, 41, "Sync");
            });
            ui.horizontal(|ui| {
                rate(ui, pane, 15, 41, tempo, false);
                pane.knob(ui, 16, "Color");
                pane.knob(ui, 17, "Stereo");
            });
        });
        group(ui, "Tremolo", |ui| {
            ui.horizontal(|ui| {
                pane.switch(ui, 32, "On");
                pane.switch(ui, 36, "Sync");
            });
            ui.horizontal(|ui| {
                rate(ui, pane, 18, 36, tempo, true);
                pane.knob(ui, 19, "Intensity");
                pane.knob(ui, 20, "Stereo");
            });
        });
    });
}

fn details(ui: &mut egui::Ui, pane: &mut Pane) {
    ui.horizontal_top(|ui| {
        group(ui, "Model", |ui| {
            pane.knobs(ui, &[(8, "Voices"), (1, "Decay"), (2, "Release")]);
            pane.knobs(ui, &[(7, "Stereo Width"), (3, "Tine Bell"), (4, "Damper Noise")]);
        });
        group(ui, "Pitch", |ui| {
            pane.knobs(ui, &[(9, "Tune"), (23, "Down"), (24, "Up")]);
            pane.knobs(ui, &[(10, "Warmth"), (5, "Lower"), (6, "Upper")]);
        });
    });
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let tempo = app.engine.lock().map(|e| e.tempo as f32).unwrap_or(120.0);
    let tab_id = egui::Id::unique(("ep-tab", t));
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let mut open = true;
    let resp = egui::Window::new("Vintage Electric Piano").id(egui::Id::unique(("ep-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(620.0, 430.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Vintage Electric Piano");
        // the control bar: Model, then Effects / Details
        ui.horizontal(|ui| {
            let cur = pane.get(0).round() as usize;
            ui.label(egui::RichText::new("Model").size(11.0).color(look::text_dim()));
            ui.menu_button(format!("{} ⌄", ep::MODELS.get(cur).copied().unwrap_or("")), |ui| {
                for (k, n) in ep::MODELS.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == cur, *n)).clicked() {
                        if k != cur {
                            // a new model: the other parameters go back to their defaults
                            pane.put(0, k as f32);
                            for (id, v) in ep::VintageKeysEp::model_defaults() {
                                pane.put(id, v);
                            }
                        }
                        ui.close();
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut shown = 1 - tab.min(1);
                keyswin::tabs(ui, &mut shown, &["Details", "Effects"]);
                tab = 1 - shown;
            });
        });
        ui.add_space(4.0);
        if tab == 1 {
            details(ui, &mut pane);
        } else {
            effects(ui, &mut pane, tempo);
        }
        ui.add_space(4.0);
        // the central area: Bass Boost and Volume
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 220.0).max(0.0) / 2.0);
            ui.vertical(|ui| {
                pane.knob(ui, 39, "Bass Boost");
                pane.switch(ui, 34, "On");
            });
            ui.add_space(20.0);
            pane.knob(ui, 22, "Volume");
        });
        keyswin::extended(ui, egui::Id::unique(("ep-ext", t)), |ui| {
            pane.slider(ui, 26, "Delay Pianissimo");
            pane.slider(ui, 27, "Delay Fortissimo");
            pane.menu(ui, 37, "MIDI Mode");
            pane.menu(ui, 28, "Voice Mode");
        });
    });
    ctx.data_mut(|d| d.insert_temp(tab_id, tab));
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Vintage Electric Piano Setting", open);
}
