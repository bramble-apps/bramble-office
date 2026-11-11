//! The Vintage Mellotron window, as Logic's: the upper section chooses and mixes the two sounds
//! (Sound A and Sound B pop-up menus with their Transpose buttons, the Blend A/B knob between
//! them), the lower section has the global Tape Speed, Tone and Volume knobs, and the extended
//! parameters (Attack, Release, Pitch Bend Range, Fixed Velocity) open with the disclosure arrow.

use egui::vec2;

use crate::app::App;
use crate::instwin;
use crate::keyswin::{self, group, Pane};
use crate::look;

/// A sound: its pop-up menu and the Transpose buttons (an octave down / up).
fn sound(ui: &mut egui::Ui, pane: &mut Pane, title: &str, id: u32, octave: u32) {
    group(ui, title, |ui| {
        ui.set_min_width(170.0);
        pane.menu(ui, id, "");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Transpose").size(11.0).color(look::text_dim()));
            let cur = pane.get(octave).round() as i32;
            if ui.add_enabled(cur > -2, egui::Button::new("−").small()).clicked() {
                pane.put(octave, (cur - 1) as f32);
            }
            ui.label(egui::RichText::new(format!("{cur:+}")).size(12.0).strong());
            if ui.add_enabled(cur < 2, egui::Button::new("+").small()).clicked() {
                pane.put(octave, (cur + 1) as f32);
            }
        });
    });
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let mut pane = Pane::new(t, &params, &vals);
    let mut open = true;
    let resp = egui::Window::new("Vintage Mellotron").id(egui::Id::unique(("mellotron-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(520.0, 330.0)).show(ctx, |ui| {
        keyswin::header(app, ui, &mut pane, "Vintage Mellotron");
        ui.horizontal(|ui| {
            sound(ui, &mut pane, "Sound A", 7, 9);
            pane.knob(ui, 4, "Blend A/B");
            sound(ui, &mut pane, "Sound B", 8, 10);
        });
        ui.add_space(6.0);
        group(ui, "Global", |ui| {
            ui.set_min_width(ui.available_width());
            pane.knobs(ui, &[(11, "Tape Speed"), (2, "Tone"), (1, "Volume")]);
        });
        keyswin::extended(ui, egui::Id::unique(("mellotron-ext", t)), |ui| {
            pane.slider(ui, 5, "Attack");
            pane.slider(ui, 6, "Release");
            pane.slider(ui, 12, "Pitch Bend Range");
            let mut fixed = pane.on(13);
            if ui.checkbox(&mut fixed, "Fixed Velocity").changed() {
                pane.put(13, fixed as u8 as f32);
            }
        });
    });
    let set = std::mem::take(&mut pane.set);
    keyswin::finish(app, ctx, t, resp.map(|r| r.response.rect), set, "Vintage Mellotron Setting", open);
}
