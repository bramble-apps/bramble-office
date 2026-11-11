//! About, and the Settings window (settings.rs).

use crate::app::App;

pub fn show(app: &mut App, ctx: &egui::Context) {
    crate::settings::window(app, ctx);
    kn_ui::update::sheet(ctx);
    // (About, the Tuner, Quick Help, the navigation sheets: windows.rs)
    crate::windows::show(app, ctx);
}
