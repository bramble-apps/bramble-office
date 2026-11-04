//! Calculator for Linux/Windows/macOS: draws calc-core's layouts and themes with egui.

mod anim;
mod app;
mod convert;
mod draw;
mod fonts;
mod history;
mod menu;
mod notes;
mod png;
mod settings;

fn main() -> eframe::Result {
    let hooks = app::Hooks::from_env();
    let (w, h) = hooks.size.unwrap_or((390.0, 760.0));
    let viewport = egui::ViewportBuilder::default()
        .with_title("Calculator")
        .with_app_id(std::env::var("FLATPAK_ID").unwrap_or_else(|_| "app.bramble.calculator".into())) // (in a Flatpak, its app id, so the desktop finds the icon)
        // the app icon (tools/icon.py), raw 256×256 RGBA
        .with_icon(egui::IconData { rgba: include_bytes!("../assets/icon/icon-256.rgba").to_vec(), width: 256, height: 256 })
        .with_inner_size([w, h])
        .with_min_inner_size([w.min(300.0), h.min(480.0)]);
    let mut opts = eframe::NativeOptions { viewport, ..Default::default() };
    // one frame queued, not two: a key press shows a frame sooner (eframe's default favours
    // throughput, which this light UI doesn't need)
    opts.wgpu_options.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
    eframe::run_native("Calculator", opts, Box::new(move |cc| Ok(Box::new(app::App::new(cc, hooks)))))
}
