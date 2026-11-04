//! Bramble Notes (working name): Apple Notes for Linux, on the Bramble suite's crates (kn-ui look,
//! kn-render text). It reads Apple's NoteStore and keeps its own library in the same format.
//!
//! Who owns what (parallel work): app.rs, chrome.rs, sidebar.rs, list.rs, toolbar.rs, menus.rs,
//! settings.rs, dialogs.rs, lock.rs, look.rs, icons.rs, dates.rs, demo.rs, store.rs = the window shell;
//! editor.rs (+ editor/…) = the note editor; nt-core = format and model.
//!
//! Automated checks with no human present (the app screenshots itself and quits):
//! `NT_SNAP=out.png [NT_SIZE=WxH] [NT_SCALE=200] [NT_DARK=1] [NT_NOW=unix-seconds]
//! [NT_NOTITLE=1 (no menu row: frames line up with Apple's screenshots)] [NT_NOTE=<title>]
//! [NT_INACTIVE=1 (drawn as an inactive window: the list's selection grey)]
//! [NT_GLIDE_TRACE=1 (the list's sliding rows per frame: insert, delete, re-sort)]
//! [NT_EDITOR_SCROLLED=1 (the editor's toolbar edge as with the note scrolled: its hairline)]
//! [NT_PERF=1 (each frame's time in the app's code, and the first frame's time since start)]
//! [NT_SCRIPT "swipe x y dx n": a two-finger horizontal trackpad swipe of n steps, with its phases; NT_DUMP prints the swipe's row and offset]
//! [NT_SCRIPT "coast x y dx n" / "coastplain x y dx n": a trackpad's coasting after the fingers lift, marked as momentum or not]
//! [NT_OPENING_PREVIEW="<done> <total>": the library-conversion card, for snapshots]
//! [NT_SYNC=1 (scripted runs sync as the app does, at start and after changes)]
//! [NT_WATCHDOG=1|0 (slow frames and stuck frames logged with their part; on in debug builds)]
//! [NT_STICKY_TRACE=1 (the list's held section header and the next one's distance, per frame)]
//! [NT_MENU_TRACE=1 (a closing context menu's fade-out, per frame)]
//! [NT_LIBRARY=<dir> (a library of the test's own; snapshots otherwise show the demo, never the
//! user's library in ~/.local/share/bramble-notes)]
//! [NT_SCRIPT="click x y; dclick x y; rclick x y; touch down|move|up x y; type …; key ctrl+N;
//! menu as Gallery; wait"]
//! bramble-notes [NoteStore.sqlite]` (points are of the window's content, without the shadow).

mod app;
mod attachments;
mod audio;
mod biometrics;
mod chrome;
mod dates;
mod demo;
mod dialogs;
mod editor;
mod export_sheet;
mod icons;
mod import;
mod list;
mod lock;
mod look;
mod menus;
mod print;
mod settings;
mod sidebar;
mod smart;
mod store;
mod swipe;
mod sync;
mod toolbar;
mod undo;
mod windows;

/// Starts the app.
pub fn run() -> eframe::Result {
    let _ = START.set(std::time::Instant::now());
    kn_ui::windows_folders();
    let args: Vec<String> = std::env::args().collect();
    let store = args.get(1).filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from);
    use_desktop_cursor_theme();
    let look = look::Look::load();
    // (the app's own title bar with its menus, as the suite's apps have by default)
    let system_title_bar = look.system_title_bar && !cfg!(target_os = "windows");
    let _ = kn_ui::winframe::SYSTEM_TITLE_BAR.set(system_title_bar);
    // (snapshots: no shadow margin, so the window is exactly NT_SIZE and fits on the screen;
    // a window near the screen's size gets maximized by the desktop instead)
    // (NT_MARGIN=1 keeps it, to check the window's rounded corners)
    let snapping = std::env::var_os("NT_SNAP").is_some() && std::env::var_os("NT_MARGIN").is_none();
    if !system_title_bar && !snapping {
        kn_ui::winframe::request_margin();
    }
    // NT_SIZE=WxH: the window's content in points (test snapshots), NT_SCALE its scale in percent
    let size = std::env::var("NT_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))).unwrap_or((1240.0, 780.0));
    let m = if system_title_bar || snapping { 0.0 } else { kn_ui::winframe::SHADOW_MARGIN as f32 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(system_title_bar)
            .with_transparent(!system_title_bar)
            .with_title("Notes")
            .with_app_id(std::env::var("FLATPAK_ID").unwrap_or_else(|_| "bramble-notes".into())) // (in a Flatpak, its app id, so the desktop finds the icon)
            .with_inner_size([size.0 + 2.0 * m, size.1 + 2.0 * m])
            .with_min_inner_size([600.0, 400.0])
            .with_icon(app_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    kn_ui::gpu::retry_other_backend(eframe::run_native("Notes", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, store))))))
}

/// When the process started (NT_PERF=1 reports the first frame's time from it).
pub(crate) static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// The watchdog (debug builds, or NT_WATCHDOG=1; NT_WATCHDOG=0 turns it off): a frame over 50 ms
/// is logged with its slowest parts ("watchdog: slow frame 73 ms: menu Delete 61.0, list 8.2"), and
/// a frame that hasn't finished after a second is reported while it lasts, with the part it's in
/// ("watchdog: stuck 1250 ms in editor").
fn watching() -> bool {
    static W: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *W.get_or_init(|| match std::env::var("NT_WATCHDOG") {
        Ok(v) => v != "0",
        Err(_) => cfg!(debug_assertions),
    })
}

fn timing() -> bool {
    static P: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *P.get_or_init(|| std::env::var_os("NT_PERF").is_some())
}

type PartName = std::borrow::Cow<'static, str>;

/// The frame being timed: when it started, the parts running (outermost first), the parts done.
#[derive(Default)]
struct Watch {
    frame: Option<std::time::Instant>,
    running: Vec<PartName>,
    done: Vec<(PartName, f64)>,
}

static WATCH: std::sync::Mutex<Watch> = std::sync::Mutex::new(Watch { frame: None, running: Vec::new(), done: Vec::new() });

fn watch() -> std::sync::MutexGuard<'static, Watch> {
    WATCH.lock().unwrap_or_else(|e| e.into_inner())
}

/// The watchdog's thread: reports a frame stuck for over a second (again each second it lasts).
fn start_watcher() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            let mut told: Option<(std::time::Instant, u64)> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let w = watch();
                let Some(t) = w.frame else {
                    told = None;
                    continue;
                };
                let ms = t.elapsed().as_millis() as u64;
                let secs = ms / 1000;
                if secs >= 1 && told != Some((t, secs)) {
                    let at = if w.running.is_empty() { "the frame".to_string() } else { w.running.join(" > ") };
                    eprintln!("watchdog: stuck {ms} ms in {at}");
                    told = Some((t, secs));
                }
            }
        });
    });
}

/// NT_PERF=1: each frame's time in the app's own code (ms), and the first frame's time since start.
/// (Also the watchdog's frame.)
pub(crate) struct FrameTimer(std::time::Instant);

impl FrameTimer {
    pub(crate) fn start() -> Option<FrameTimer> {
        if !(timing() || watching()) {
            return None;
        }
        if watching() {
            start_watcher();
            let mut w = watch();
            w.frame = Some(std::time::Instant::now());
            w.running.clear();
            w.done.clear();
        }
        Some(FrameTimer(std::time::Instant::now()))
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        static FIRST: std::sync::Once = std::sync::Once::new();
        let ms = self.0.elapsed().as_secs_f64() * 1000.0;
        let mut first = false;
        FIRST.call_once(|| {
            first = true;
            if timing() {
                let since = START.get().map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(f64::NAN);
                eprintln!("perf first-frame-done {since:.1} ms after start");
            }
        });
        if timing() {
            eprintln!("perf frame {ms:.2}");
        }
        if watching() {
            let mut w = watch();
            w.frame = None;
            // (the first frame lays out the fonts: slow on purpose)
            if ms > 50.0 && !first {
                let mut parts = std::mem::take(&mut w.done);
                parts.sort_by(|a, b| b.1.total_cmp(&a.1));
                let list: Vec<String> = parts.iter().take(4).map(|(n, t)| format!("{n} {t:.1}")).collect();
                eprintln!("watchdog: slow frame {ms:.0} ms: {}", if list.is_empty() { "(no part timed)".to_string() } else { list.join(", ") });
            }
        }
    }
}

/// NT_PERF=1: how long a part of the frame took (list, sidebar, editor, toolbar, a menu command…);
/// the watchdog names the slow ones.
pub(crate) struct PartTimer(PartName, std::time::Instant);

impl PartTimer {
    pub(crate) fn start(name: impl Into<PartName>) -> Option<PartTimer> {
        if !(timing() || watching()) {
            return None;
        }
        let name = name.into();
        if watching() {
            watch().running.push(name.clone());
        }
        Some(PartTimer(name, std::time::Instant::now()))
    }
}

impl Drop for PartTimer {
    fn drop(&mut self) {
        let ms = self.1.elapsed().as_secs_f64() * 1000.0;
        if timing() {
            eprintln!("perf part {} {ms:.2}", self.0);
        }
        if watching() {
            let mut w = watch();
            w.running.pop();
            let name = std::mem::take(&mut self.0);
            w.done.push((name, ms));
        }
    }
}

/// NT_PERF=1: how long a phase took (startup's parts).
pub(crate) struct PhaseTimer(&'static str, std::time::Instant);

impl PhaseTimer {
    pub(crate) fn new(name: &'static str) -> PhaseTimer {
        PhaseTimer(name, std::time::Instant::now())
    }
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        let since = START.get().map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(f64::NAN);
        eprintln!("perf {} {:.1} ms (done {since:.1} ms after start)", self.0, self.1.elapsed().as_secs_f64() * 1000.0);
    }
}

/// The window icon (X11 and others; Wayland takes it from bramble-notes.desktop).
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../data/icons/notes-256.png")).map(|i| i.into_rgba8()).ok();
    img.map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }).unwrap_or_default()
}

/// The desktop's own cursor theme and size (Letters' and Keydeck's: Wayland windows otherwise get
/// another).
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
