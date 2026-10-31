//! Keydeck's look as a library, so Numdeck (numbers-linux) can share it: `theme` (colours,
//! colour themes, controls), `glass` (liquid glass, translucent panes, classic look), `icons`
//! (toolbar and inspector icons), `snap` (alignment guides: snapping while moving / resizing,
//! equal spacing and sizes) and `winframe` (the window frame and title bar).
#![allow(non_snake_case)]

#[cfg(target_os = "android")]
pub mod android;
pub mod chartdata;
pub mod chartpicker;
pub mod chartpanes;
pub mod colors;
pub mod contenthub;
pub mod files;
pub mod glass;
pub mod gpu;
pub mod media;
pub mod icons;
pub mod scale;
pub mod snap;
pub mod spell;
pub mod shapelib;
pub mod theme;
pub mod welcome;
pub mod typing;
pub mod winframe;

/// First thing in every app's update, once per frame: on Android, files picked in the system
/// picker (they arrive as dropped files) and the on-screen keyboard; nothing elsewhere.
pub fn begin_frame(ctx: &egui::Context) {
    #[cfg(target_os = "android")]
    android::frame(ctx);
    let _ = ctx;
}

/// An app's own text editor (not an egui text field) is taking typing this frame: on Android
/// the on-screen keyboard shows while it is.
pub fn typing(ctx: &egui::Context) {
    #[cfg(target_os = "android")]
    android::typing(ctx);
    let _ = ctx;
}

/// The pointer is over a window, popover or menu (anything above the panels): canvases leave
/// the scroll wheel and pinch to it then.
pub fn pointer_over_window(ctx: &egui::Context) -> bool {
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return false };
    ctx.layer_id_at(p).is_some_and(|l| l.order != egui::Order::Background)
}

/// The system's "open this" command for a web address, file or folder (xdg-open on Linux,
/// open on macOS, start on Windows), ready to spawn.
pub fn open_command(target: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(target).creation_flags(0x0800_0000);
        c
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        c.arg(target);
        c
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    }
}

/// Opens a web address (or file) with the system's default app, in the background.
pub fn open_url(url: &str) {
    #[cfg(target_os = "android")]
    if android::open_url(url) {
        return;
    }
    let url = url.to_string();
    std::thread::spawn(move || {
        use std::process::Stdio;
        let _ = open_command(&url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

/// Bramble Office's donation page (the app menu's Support Development…).
pub const DONATE_URL: &str = "https://www.paypal.com/donate/?hosted_button_id=5EP8EM86U9P7Q";

/// Windows: the folders the apps keep things in (settings, recent files, My Shapes, themes)
/// come from HOME and the XDG variables, which Windows doesn't set. Before anything else runs,
/// point them at the usual places: settings and themes in %APPDATA%, caches in %LOCALAPPDATA%.
pub fn windows_folders() {
    #[cfg(target_os = "windows")]
    {
        let set = |k: &str, v: Option<std::ffi::OsString>| {
            if std::env::var_os(k).is_none()
                && let Some(v) = v
            {
                // (at the start of main, before any other thread exists)
                unsafe { std::env::set_var(k, v) };
            }
        };
        set("HOME", std::env::var_os("USERPROFILE"));
        set("XDG_CONFIG_HOME", std::env::var_os("APPDATA"));
        set("XDG_DATA_HOME", std::env::var_os("APPDATA"));
        set("XDG_CACHE_HOME", std::env::var_os("LOCALAPPDATA"));
    }
}
