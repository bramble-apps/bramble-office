//! Bramble Studio (working name): a music app modelled 1-1 on Logic Pro, opening Logic's own
//! projects. The window, its parts and their behaviour follow the Logic research
//! (logic-research: menus, key commands, screenshots); the files are read by lg-core.

pub mod allfiles;
pub mod app;
pub mod arp;
pub mod arrmarks;
pub mod beatmapping;
pub mod globalclip;
pub mod stmulti;
pub mod tempointerp;
pub mod arranging;
pub mod audio;
pub mod chase;
pub mod audioedit;
mod automation;
mod autoext;
mod spx;
pub mod browser;
mod chrome;
mod controlbar;
pub mod demo;
mod dialogs;
pub mod envhub;
pub mod envwin;
pub mod envsysex;
pub mod metroset;
pub mod midiout;
pub mod movie;
pub mod surfaces;
pub mod surfext;
pub mod syncmidi;
pub mod syncmmc;
pub mod linksync;
pub mod drummer;
pub mod glyphs;
pub mod edit;
pub mod eq;
pub mod flex;
pub mod follow;
mod inspector;
pub mod keymap;
pub mod keycmds;
pub mod keyextra;
pub mod prefs;
pub mod projsettings;
mod live;
pub mod midifunc;
pub mod midifx;
mod midiwin;
pub mod liveloops;
pub mod globals;
pub mod look;
pub mod tempomode;
pub mod stdefaults;
pub mod recsettings;
pub mod hmt;
pub mod markers;
pub mod menus;
mod mixer;
mod mixercmd;
mod nav;
pub mod palette;
pub mod mixstate;
pub mod mixengine;
pub mod mixext;
pub mod mixwin;
pub mod tm;
pub mod trackhdr;
pub mod trackops;
pub mod freeze;
pub mod surround;
pub mod surround_ui;
mod pianoroll;
mod preview;
mod projaudio;
pub mod projfile;
pub mod recording;
mod regionops;
pub mod regionparams;
pub mod rollextras;
mod snap;
mod newtrack;
mod pluginwin;
pub mod plughead;
mod eqview;
mod meqview;
pub mod plugbrowser;
mod instwin;
mod samplerwin;
mod dmdwin;
mod quickwin;
mod drumsynthwin;
mod legacywin;
mod dkdwin;
mod alchemyrack;
mod alchemywin;
mod keyswin;
mod b3win;
mod epwin;
mod clavwin;
mod mellotronwin;
mod studiowin;
mod synthui;
mod sidechain;
mod synthwin;
mod es1win;
mod evocwin;
mod retrowin;
mod salchwin;
mod sculptwin;
mod ubwin;
mod esewin;
mod esmwin;
mod espwin;
mod efm1win;
mod es2win;
mod fxviews;
mod compview;
mod meterview;
mod qrsview;
mod autosampler;
mod pbrouter;
mod ddview;
mod phatview;
mod stemsplit;
mod players;
mod spedit;
mod artic;
mod sbp;
mod spdrum;
mod rightpane;
mod screensets;
mod score;
mod scoreset;
mod scorefile;
mod scoreinsp;
mod scorexml;
mod scorestyle;
mod scoretuplet;
mod scorepdf;
mod scoretools;
mod scoreglobal;
mod scoregrid;
mod stepseq;
pub mod sthints;
mod stepinsp;
mod cellinsp;
mod celledit;
mod cellbounce;
mod scorecmd;
mod stepinput;
mod stepedit;
mod eventedit;
mod evlist;
mod groovecmd;
mod joinaudio;
mod demix;
mod regsolo;
mod stretch;
mod flexedit;
mod varispeed;
mod rollsnap;
pub mod settings;
pub mod share;
pub mod smarttempo;
pub mod takes;
pub mod smartcontrols;
mod scgraph;
pub mod soundlib;
mod stopbutton;
mod projend;
mod brpreview;
mod trackarea;
mod toolbar;
mod tracks;
mod focus;
mod regiontrim;
mod regionclip;
mod hosting;
pub mod units;
pub mod wavecache;
mod widgets;
mod viewprefs;
mod windows;

pub fn run() -> eframe::Result {
    kn_ui::windows_folders();
    kn_ui::update::init(kn_ui::update::AppInfo { name: "Bramble Studio", version: env!("CARGO_PKG_VERSION"), bin: "bramble-studio", appimage: "Bramble-Studio-x86_64.AppImage", mac_bundle: "Bramble Studio.app" });
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
            .with_transparent(!system_title_bar || cfg!(target_os = "linux"))
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
