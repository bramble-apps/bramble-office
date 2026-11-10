//! About, and the Settings window (settings.rs).

use crate::app::App;
use crate::look;

pub fn show(app: &mut App, ctx: &egui::Context) {
    crate::settings::window(app, ctx);
    kn_ui::update::sheet(ctx);
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
