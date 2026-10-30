//! Keynote for Linux — application entry point.
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
use kn_ui::{glass, icons, snap, theme, winframe};
mod welcome;
mod pptx_text;
mod pptx;
mod builds;
mod canvas;
mod chartedit;
mod editing;
mod export;
mod filemenu;
mod find;
mod settings;
mod spellwin;
mod webmedia;
mod condwin;
mod warnings;
mod imageadj;
mod toolbar;
mod toolwins;
mod mediabrowser;
mod versions;
mod tableops;
mod equation;
mod spell;
mod inspector;
mod links;
mod media;
mod menus;
mod navigator;
mod objectlist;
mod play;
mod replacefonts;
mod textedit;
mod themes;
mod textures;
mod transitions;

fn main() -> eframe::Result {
    kn_ui::windows_folders();
    let args: Vec<String> = std::env::args().collect();
    // headless: kn-app --export-pdf in.key out.pdf
    if args.get(1).map(String::as_str) == Some("--export-pdf") && args.len() == 4 {
        match export::export_pdf_headless(args[2].as_ref(), args[3].as_ref()) {
            Ok(n) => println!("exported {n} slides to {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // headless: kn-app --export in.key out.{pdf,mp4,gif} [seconds per slide]
    if args.get(1).map(String::as_str) == Some("--export") && (4..=5).contains(&args.len()) {
        let secs = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(5.0);
        match export::export_any_headless(args[2].as_ref(), args[3].as_ref(), secs) {
            Ok(n) => println!("exported {n} slides to {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // headless: kn-app --convert in.{key,pptx} out.{key,pptx} [hybrid] (PowerPoint both ways)
    if args.get(1).map(String::as_str) == Some("--convert") && (4..=5).contains(&args.len()) {
        if let Err(e) = pptx::convert(args[2].as_ref(), args[3].as_ref(), args.get(4).is_some_and(|a| a == "hybrid")) {
            eprintln!("convert failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--list-unimplemented") {
        menus::list_unimplemented();
        return Ok(());
    }
    // every file given opens in its own tab
    let files: Vec<std::path::PathBuf> = args.iter().skip(1).map(std::path::PathBuf::from).collect();
    use_desktop_cursor_theme();
    move_old_folders();
    // table rows grow to fit what's typed into them, whatever their stored height (Keynote)
    kn_model::table::set_all_rows_fit(true);
    let startup = settings::Settings::load();
    // (Windows: always the app's own title bar with its menus; the system one is for Linux and macOS)
    let system_title_bar = startup.system_title_bar && !cfg!(target_os = "windows");
    if !startup.camera.is_empty() {
        media::set_camera_device(&startup.camera);
    }
    let _ = winframe::SYSTEM_TITLE_BAR.set(system_title_bar);
    // menus in the title bar: the window draws its own frame and shadow where the desktop
    // leaves that to apps
    if !system_title_bar {
        winframe::request_margin();
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(system_title_bar)
            // macOS: always transparent, so Translucent sidebars can show the system blur behind the
            // content view with the macOS title bar too (panes still paint opaque when it is off)
            .with_transparent(!system_title_bar || cfg!(target_os = "macos"))
            .with_title("Keydeck")
            .with_app_id("keydeck")
            .with_icon(app_icon())
            // KN_SIZE=WxH (automated checks): the window's starting size
            .with_inner_size(std::env::var("KN_SIZE").ok().and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))).unwrap_or([1440.0, 900.0]))
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    kn_ui::gpu::retry_other_backend(eframe::run_native("Keydeck", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, files))))))
}

/// The window icon (X11 and other systems; on Wayland the desktop takes it from keydeck.desktop).
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../data/icons/keydeck-256.png")).map(|i| i.into_rgba8()).ok();
    img.map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }).unwrap_or_default()
}

/// Wayland windows load their cursor from XCURSOR_THEME/XCURSOR_SIZE, which GNOME does not set,
/// so the app would otherwise show a different cursor than every other window. Use the
/// desktop's own setting (GNOME's cursor-theme / cursor-size) unless already given.
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

/// The app was first called "keynote-linux": its settings, themes and caches move to the
/// "keydeck" folders once (when those don't exist yet).
fn move_old_folders() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return };
    let env_dir = |var: &str, fallback: &str| std::env::var_os(var).map(std::path::PathBuf::from).unwrap_or_else(|| home.join(fallback));
    for base in [env_dir("XDG_CONFIG_HOME", ".config"), env_dir("XDG_DATA_HOME", ".local/share"), env_dir("XDG_CACHE_HOME", ".cache")] {
        let (old, new) = (base.join("keynote-linux"), base.join("keydeck"));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

