//! Letters: a word processor and page-layout app for Linux that reads and writes Apple Pages
//! (`.pages`) documents through the suite's iWork crates.
//!
//! Automated checks with no human present (the app screenshots itself and quits):
//! `CS_SNAP=out.png [CS_PAGE=n] [CS_ZOOM=z] [CS_CMD=menu command,…] [CS_SCRIPT="click x y; type …; key …; menu …"] [CS_SAVE=out.pages] letters file.pages`
//! Headless conversion: `letters --pdf|--docx|--rtf|--txt|--epub|--html|--png in out`.
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod canvas;
mod chartedit;
mod chrome;
mod dialogs;
mod editing;
mod fileops;
mod find;
mod fonts;
mod hfnumber;
mod imageadj;
mod listcap;
mod condwin;
mod guides;
mod media;
mod mediainsp;
mod insert;
mod inspector;
mod look;
mod mask;
mod menus;
mod notes;
mod objinsp;
mod points;
mod ruler;
mod settings;
mod sidebar;
mod spelling;
mod tables;
mod tabs;
mod tbicons;
mod tblinsp;
mod text;
mod textinsp;
mod toolbar;

fn main() -> eframe::Result {
    kn_ui::windows_folders();
    move_cardstock_folders();
    pg_core::init();
    let args: Vec<String> = std::env::args().collect();
    // headless: letters --pdf in.pages out.pdf (and --docx --rtf --txt --epub --html --png)
    if let Some(fmt) = args.get(1).and_then(|a| a.strip_prefix("--")).filter(|f| ["pdf", "docx", "rtf", "txt", "epub", "html", "png"].contains(f)) {
        if args.len() != 4 {
            eprintln!("usage: letters --{fmt} in.pages out.{fmt}");
            std::process::exit(2);
        }
        let src = std::path::Path::new(&args[2]);
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let doc = if ext == "pages" || src.is_dir() { pg_core::Pages::open(src) } else { pg_core::import::import_file(src) };
        let res = doc.map_err(|e| e.to_string()).and_then(|p| pg_core::export::export(&p, std::path::Path::new(&args[3])));
        match res {
            Ok(()) => println!("wrote {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // Learn Spelling keeps Letters' words in ~/.config/letters/words.txt
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))) {
        kn_ui::spell::set_words_file(base.join("letters").join("words.txt"));
    }
    let _ = kn_ui::spell::dicts();
    // every file named opens, each in a tab
    let files: Vec<std::path::PathBuf> = args.iter().skip(1).filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from).collect();
    use_desktop_cursor_theme();
    let look = look::Look::load();
    // (Windows: always the app's own title bar with its menus; the system one is for Linux and macOS)
    let system_title_bar = look.system_title_bar && !cfg!(target_os = "windows");
    let _ = kn_ui::winframe::SYSTEM_TITLE_BAR.set(system_title_bar);
    if !system_title_bar {
        kn_ui::winframe::request_margin();
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(system_title_bar)
            .with_transparent(!system_title_bar)
            .with_title("Letters")
            .with_app_id("letters")
            .with_inner_size([1360.0, 900.0])
            .with_min_inner_size([700.0, 450.0])
            .with_icon(app_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    kn_ui::gpu::retry_other_backend(eframe::run_native("Letters", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, files))))))
}

/// The window icon (X11 and others; Wayland takes it from letters.desktop).
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../data/icons/letters-256.png")).map(|i| i.into_rgba8()).ok();
    img.map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }).unwrap_or_default()
}

/// The desktop's own cursor theme and size (Keydeck's: Wayland windows otherwise get another).
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

/// Letters was called Cardstock: its settings, recent files, versions and words move over to
/// the new name the first time (each folder only if there's no Letters one yet).
fn move_cardstock_folders() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let bases = [
        std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config"))),
        std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share"))),
        std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".cache"))),
    ];
    for base in bases.into_iter().flatten() {
        let (old, new) = (base.join("cardstock"), base.join("letters"));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}
