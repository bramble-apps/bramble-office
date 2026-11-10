//! Bramble Studio (working name): a music app modelled 1-1 on Logic Pro, opening Logic's own
//! projects. The window, its parts and their behaviour follow the Logic research
//! (logic-research: menus, key commands, screenshots); the files are read by lg-core.

pub mod app;
pub mod audio;
mod browser;
mod chrome;
mod controlbar;
pub mod demo;
mod dialogs;
pub mod drummer;
pub mod edit;
pub mod eq;
mod inspector;
pub mod keymap;
mod live;
pub mod look;
pub mod menus;
mod mixer;
mod pianoroll;
mod snap;
mod newtrack;
mod pluginwin;
mod players;
mod rightpane;
pub mod settings;
mod smartcontrols;
mod tracks;
pub mod units;
mod widgets;

pub fn run() -> eframe::Result {
    kn_ui::windows_folders();
    let path = std::env::args().nth(1).filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from);
    use_desktop_cursor_theme();
    let look = look::Look::load();
    let system_title_bar = look.system_title_bar && !cfg!(target_os = "windows");
    let _ = kn_ui::winframe::SYSTEM_TITLE_BAR.set(system_title_bar);
    let snapping = std::env::var_os("LG_SNAP").is_some();
    if !system_title_bar && !snapping {
        kn_ui::winframe::request_margin();
    }
    let size = std::env::var("LG_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))).unwrap_or((1400.0, 900.0));
    let m = if system_title_bar || snapping { 0.0 } else { kn_ui::winframe::SHADOW_MARGIN as f32 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(system_title_bar)
            .with_transparent(!system_title_bar)
            .with_title("Studio")
            .with_app_id(std::env::var("FLATPAK_ID").unwrap_or_else(|_| "bramble-studio".into()))
            .with_inner_size([size.0 + 2.0 * m, size.1 + 2.0 * m])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../data/icons/studio-256.png")).unwrap_or_default())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    kn_ui::gpu::retry_other_backend(eframe::run_native("Studio", options, Box::new(move |cc| Ok(Box::new(snap::Snap::new(app::App::new(cc, path)))))))
}

/// The desktop's cursor theme and size for the window (Wayland apps must ask, as GTK does).
fn use_desktop_cursor_theme() {
    let get = |key: &str| {
        std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", key])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string())
            .filter(|v| !v.is_empty())
    };
    if std::env::var_os("XCURSOR_THEME").is_none() {
        if let Some(theme) = get("cursor-theme") {
            // SAFETY: called at startup before any other thread exists
            unsafe { std::env::set_var("XCURSOR_THEME", theme) };
        }
    }
    if std::env::var_os("XCURSOR_SIZE").is_none() {
        if let Some(size) = get("cursor-size").filter(|s| s.parse::<u32>().is_ok()) {
            unsafe { std::env::set_var("XCURSOR_SIZE", size) };
        }
    }
}
