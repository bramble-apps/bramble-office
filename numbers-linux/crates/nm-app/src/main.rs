//! Values: a Numbers-compatible spreadsheet app for Linux, reading and writing
//! real `.numbers` files through Keydeck's iWork crates.
//!
//! Automated checks with no human present (the app screenshots itself and quits):
//! `NM_SNAP=out.png [NM_SHEET=n] [NM_CELL=row,col[,row2,col2]] [NM_EDIT=text] [NM_INSPECTOR=table|cell|text|arrange] [NM_ZOOM=0.75] [NM_CMD=insert-table,add-sheet,undo,redo] [NM_SCRIPT="click x y; drag x1 y1 x2 y2; type text; key Enter; wait"] numdeck file.numbers`
// Windows: a window app, without a console window behind it
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod canvas;
mod chartedit;
mod chrome;
mod clip;
mod menus;
mod editing;
mod equation;
mod fileops;
mod fonts;
mod imageadj;
mod inspector;
mod objinsp;
mod tblinsp;
mod look;
mod mask;
mod media;
mod myshapes;
mod organize;
mod pdf;
mod pivotui;
mod points;
mod settings;
mod spelling;
mod stocks;
mod tbicons;
mod templates;
mod datarefs;
mod links;
mod textedit;
mod tiles;
mod toolbar;

fn main() -> eframe::Result {
    kn_ui::windows_folders();
    nm_core::init();
    let args: Vec<String> = std::env::args().collect();
    // headless: numdeck --csv in.numbers out-folder   (one CSV per table)
    // numdeck --pdf in.numbers|in.xlsx out.pdf: print it (its print setup) to a PDF
    if args.get(1).map(String::as_str) == Some("--pdf") && args.len() == 4 {
        let p = std::path::Path::new(&args[2]);
        // (an Excel workbook's rotated / diagonal cells as on the sheet)
        let opened = if p.extension().is_some_and(|e| e == "xlsx") { nm_core::xlsx::open_xlsx(p).map(|o| (o.workbook, o.extras)) } else { nm_core::Workbook::open(p).map(|w| (w, Default::default())) };
        let res = opened.map_err(|e| e.to_string()).and_then(|(wb, ex)| {
            let mut by_table = std::collections::HashMap::new();
            for s in wb.sheets() {
                if let (Some(m), Some(t)) = (ex.get(&s.name), wb.tables(&s).first()) {
                    by_table.insert(t.info, m.clone());
                }
            }
            pdf::export_with(&wb, args[3].as_ref(), by_table)
        });
        match res {
            Ok(n) => println!("wrote {n} pages to {}", args[3]),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--csv") && args.len() == 4 {
        match app::export_csv(args[2].as_ref(), args[3].as_ref()) {
            Ok(n) => println!("wrote {n} CSV files to {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // headless: numdeck --xlsx in.numbers out.xlsx / --tsv in.numbers out-folder
    if matches!(args.get(1).map(String::as_str), Some("--xlsx" | "--tsv")) && args.len() == 4 {
        let res = nm_core::Workbook::open(&args[2]).map_err(|e| e.to_string()).and_then(|wb| {
            if args[1] == "--xlsx" {
                nm_core::xlsx::write_xlsx(&wb, args[3].as_ref()).map_err(|e| e.to_string())
            } else {
                app::export_delimited(&wb, args[3].as_ref(), '\t')
            }
        });
        match res {
            Ok(n) => println!("wrote {n} tables to {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // headless: numdeck --pdf in.numbers out.pdf   (one page per sheet)
    if args.get(1).map(String::as_str) == Some("--pdf") && args.len() == 4 {
        let res = nm_core::Workbook::open(&args[2]).map_err(|e| e.to_string()).and_then(|wb| pdf::export(&wb, args[3].as_ref()));
        match res {
            Ok(n) => println!("exported {n} pages to {}", args[3]),
            Err(e) => {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // Learn Spelling keeps Values' words in ~/.config/numdeck/words.txt
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))) {
        kn_ui::spell::set_words_file(base.join("numdeck").join("words.txt"));
    }
    // the dictionaries load in the background from now (spelling is ready when it's needed)
    let _ = kn_ui::spell::dicts();
    let file = args.get(1).map(std::path::PathBuf::from);
    use_desktop_cursor_theme();
    // Keydeck's window: menus in the title bar, our own frame and shadow (or the system's)
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
            // macOS: always transparent, so Translucent sidebars can show the system blur behind the
            // content view with the macOS title bar too (panes still paint opaque when it is off)
            .with_transparent(!system_title_bar || cfg!(target_os = "macos"))
            .with_title("Values")
            .with_app_id("numdeck")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([700.0, 450.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    kn_ui::gpu::retry_other_backend(eframe::run_native("Values", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, file))))))
}

/// The window icon (X11 and others; Wayland takes it from numdeck.desktop).
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../data/icons/numdeck-256.png")).map(|i| i.into_rgba8()).ok();
    img.map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }).unwrap_or_default()
}

/// Wayland windows load their cursor from XCURSOR_THEME / XCURSOR_SIZE, which GNOME does not
/// set, so the window would otherwise show a different (and on HiDPI, blurry) cursor than
/// every other window: use the desktop's own cursor-theme / cursor-size (as Keydeck does).
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
