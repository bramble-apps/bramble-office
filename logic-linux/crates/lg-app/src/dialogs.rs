//! Settings (Logic's Settings > General / View, Studio's look and SoundFonts) and About.

use crate::app::{App, SoundFontMode};
use crate::look;

pub fn show(app: &mut App, ctx: &egui::Context) {
    if app.show_settings {
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
            ui.label(egui::RichText::new("Appearance").font(look::bold(13.0)));
            let mut changed = false;
            changed |= kn_ui::theme::checkbox(ui, &mut app.look.dark, "Dark").changed();
            changed |= kn_ui::theme::checkbox(ui, &mut app.look.liquid_glass, "Liquid glass").changed();
            ui.horizontal(|ui| {
                ui.label("Colour theme");
                kn_ui::theme::combo("lg-theme").selected_text(kn_ui::theme::COLOR_THEMES.iter().find(|t| t.0 == app.look.color_theme).map_or("Default", |t| t.1)).show_ui(ui, |ui| {
                    for t in kn_ui::theme::COLOR_THEMES.iter() {
                        if ui.selectable_label(app.look.color_theme == t.0, t.1).clicked() {
                            app.look.color_theme = t.0.to_string();
                            changed = true;
                        }
                    }
                });
            });
            if changed {
                app.look.apply(ctx);
                app.look.save();
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("SoundFonts").font(look::bold(13.0)));
            ui.label(egui::RichText::new("When a SoundFont (.sf2 / .sf3) is added:").color(look::text_dim()));
            ui.radio_value(&mut app.soundfont_mode, SoundFontMode::Native, "Play natively (Bramble's SoundFont engine)");
            ui.radio_value(&mut app.soundfont_mode, SoundFontMode::Sampler, "Convert into a Sampler instrument (Logic's way)");
            let sf = app.engine.lock().ok().and_then(|e| e.soundfont.clone());
            ui.label(egui::RichText::new(match sf {
                Some(p) => format!("Instruments: {}", p.display()),
                None => "Instruments: the built-in synth (no SoundFont found)".into(),
            }).color(look::text_dim()).font(look::regular(11.5)));
        });
        app.show_settings = open;
    }
    if app.show_about {
        let mut open = true;
        egui::Window::new("About Bramble Studio").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(egui::RichText::new("Bramble Studio").font(look::bold(18.0)));
            ui.label("Version 0.4.2 — a first draft");
            ui.label("Opens Logic Pro projects (.logicx): tracks, regions, notes, tempo, signatures, drummer parts and audio regions, and plays them.");
            ui.label(egui::RichText::new("Part of Bramble Office. Free and open source (GPL-3.0).").color(look::text_dim()));
        });
        app.show_about = open;
    }
}
